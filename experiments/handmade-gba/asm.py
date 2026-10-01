"""A small two-pass assembler for the ARMv4 instructions one GBA program needs.

Written from the instruction formats rather than from any existing assembler: the point of the
exercise was to put every bit in place by hand and then let LLVM's disassembler say whether
they landed where they were meant to (see check.py).

Supported:
  data processing   and eor sub rsb add adc sbc rsc tst teq cmp cmn orr mov bic mvn  (+cond, +s)
                    operand 2: #imm (any value an 8-bit rotate can make), rm, rm <shift> #n,
                    rm <shift> rs
  multiply          mul rd, rm, rs      mla rd, rm, rs, rn
  load/store        ldr str ldrb strb   [rn], [rn, #±n], [rn, ±rm{, shift #n}], [rn], #±n,
                                        optional ! writeback
                    ldrh strh           [rn], [rn, #±n], [rn, ±rm], [rn], #±n
                    ldr rd, =value      through a literal pool, emitted at .pool or the end
  branches          b bl (+cond) label, bx rm
  directives        .word .hword .byte .ascii .align n .pool .org addr, `name = value`
"""

import re
import sys

COND = {c: i for i, c in enumerate(
    "eq ne cs cc mi pl vs vc hi ls ge lt gt le al".split())}
COND["hs"], COND["lo"] = COND["cs"], COND["cc"]

DP = {op: i for i, op in enumerate(
    "and eor sub rsb add adc sbc rsc tst teq cmp cmn orr mov bic mvn".split())}
SHIFT = {"lsl": 0, "lsr": 1, "asr": 2, "ror": 3}


class AsmError(Exception):
    pass


def reg(tok):
    tok = tok.strip().lower()
    alias = {"sp": 13, "lr": 14, "pc": 15}
    if tok in alias:
        return alias[tok]
    m = re.fullmatch(r"r(\d+)", tok)
    if not m or int(m.group(1)) > 15:
        raise AsmError(f"not a register: {tok!r}")
    return int(m.group(1))


def rot_imm(value):
    """The rotate/imm8 pair that makes `value`, or None. Smallest rotation first."""
    value &= 0xFFFFFFFF
    for rot in range(16):
        # Rotating left by 2*rot undoes the core's rotate right.
        v = ((value << (2 * rot)) | (value >> (32 - 2 * rot))) & 0xFFFFFFFF if rot else value
        if v < 256:
            return rot, v
    return None


def split_ops(text):
    """Commas that are not inside [ ]."""
    out, depth, cur = [], 0, ""
    for ch in text:
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


class Assembler:
    def __init__(self, base):
        self.base = base
        self.symbols = {}

    def value(self, expr):
        expr = expr.strip().lstrip("#")
        try:
            return int(eval(expr, {"__builtins__": {}}, dict(self.symbols)))
        except NameError:
            if self.final:
                raise AsmError(f"unknown symbol in {expr!r}")
            return 0

    # ---- operand 2 -----------------------------------------------------------------------------

    def operand2(self, ops):
        """(I bit, 12-bit field) for a data-processing operand."""
        if ops[0].startswith("#"):
            v = self.value(ops[0])
            r = rot_imm(v)
            if r is None:
                if self.final:
                    raise AsmError(f"{v:#x} is not a rotated 8-bit immediate")
                r = (0, 0)
            return 1, (r[0] << 8) | r[1]
        rm = reg(ops[0])
        if len(ops) == 1:
            return 0, rm
        kind, amount = ops[1].split(None, 1)
        st = SHIFT[kind.lower()]
        if amount.strip().startswith("#"):
            n = self.value(amount)
            if not 0 <= n < 32:
                raise AsmError(f"shift {n} out of range")
            return 0, (n << 7) | (st << 5) | rm
        return 0, (reg(amount) << 8) | (st << 5) | (1 << 4) | rm

    # ---- one instruction -----------------------------------------------------------------------

    def encode(self, mnem, ops, addr):
        m = mnem.lower()

        if m == "bx" or (m.startswith("bx") and m[2:] in COND):
            cond = COND[m[2:] or "al"]
            return cond << 28 | 0x012FFF10 | reg(ops[0])

        mm = re.fullmatch(r"(bl|b)(" + "|".join(COND) + r")?", m)
        if mm:
            link = mm.group(1) == "bl"
            cond = COND[mm.group(2) or "al"]
            target = self.value(ops[0])
            off = (target - (addr + 8)) >> 2
            if self.final and not -(1 << 23) <= off < (1 << 23):
                raise AsmError("branch out of range")
            return cond << 28 | 0b101 << 25 | link << 24 | (off & 0xFFFFFF)

        mm = re.fullmatch(r"(mul|mla)(" + "|".join(COND) + r")?(s)?", m)
        if mm:
            cond = COND[mm.group(2) or "al"]
            s = 1 if mm.group(3) else 0
            rd, rm, rs = reg(ops[0]), reg(ops[1]), reg(ops[2])
            acc = mm.group(1) == "mla"
            rn = reg(ops[3]) if acc else 0
            return cond << 28 | acc << 21 | s << 20 | rd << 16 | rn << 12 | rs << 8 | 0x90 | rm

        mm = re.fullmatch(r"(ldr|str)(" + "|".join(COND) + r")?(b|h)?", m)
        if mm:
            return self.mem(mm, ops, addr)

        mm = re.fullmatch(r"(" + "|".join(DP) + r")(" + "|".join(COND) + r")?(s)?", m)
        if mm:
            op = DP[mm.group(1)]
            cond = COND[mm.group(2) or "al"]
            s = 1 if mm.group(3) else 0
            if op in (8, 9, 10, 11):  # tst teq cmp cmn: no rd, always set flags
                rd, rn, rest, s = 0, reg(ops[0]), ops[1:], 1
            elif op in (13, 15):      # mov mvn: no rn
                rd, rn, rest = reg(ops[0]), 0, ops[1:]
            else:
                rd, rn, rest = reg(ops[0]), reg(ops[1]), ops[2:]
            i, op2 = self.operand2(rest)
            return cond << 28 | i << 25 | op << 21 | s << 20 | rn << 16 | rd << 12 | op2

        raise AsmError(f"unknown instruction {mnem!r}")

    def mem(self, mm, ops, addr):
        load = mm.group(1) == "ldr"
        cond = COND[mm.group(2) or "al"]
        size = mm.group(3)  # None word, "b" byte, "h" halfword
        rd = reg(ops[0])
        src = ops[1].strip()

        if src.startswith("="):
            if not load or size:
                raise AsmError("only ldr takes =value")
            expr = src[1:].strip()
            # What LLVM does, learned by comparing against it: a value one data-processing
            # immediate can make costs no memory read at all, as a mov, or as an mvn of its
            # complement.
            if self.known(expr):
                v = self.value(expr) & 0xFFFFFFFF
                for op, imm in ((13, v), (15, ~v & 0xFFFFFFFF)):
                    r = rot_imm(imm)
                    if r is not None:
                        return cond << 28 | 1 << 25 | op << 21 | rd << 12 | r[0] << 8 | r[1]
            target = self.literal(expr, addr)
            off = target - (addr + 8)
            u = 1 if off >= 0 else 0
            return cond << 28 | 0b01 << 26 | 1 << 24 | u << 23 | 1 << 20 | 15 << 16 | rd << 12 | abs(off)

        m = re.fullmatch(r"\[([^\]]*)\](!?)", src)
        if not m:
            raise AsmError(f"bad address {src!r}")
        inner = split_ops(m.group(1))
        wb = 1 if m.group(2) else 0
        rn = reg(inner[0])
        pre = 1
        offs = inner[1:]
        if len(ops) > 2:  # post-indexed: [rn], offset
            pre, wb, offs = 0, 0, ops[2:]

        if size == "h":
            base = cond << 28 | pre << 24 | wb << 21 | load << 20 | rn << 16 | rd << 12 | 0xB0
            if not offs:
                return base | 1 << 23 | 1 << 22
            o = offs[0]
            if o.startswith("#"):
                n = self.value(o)
                if not -255 <= n <= 255:
                    raise AsmError(f"halfword offset {n} out of range")
                u = 1 if n >= 0 else 0
                n = abs(n)
                return base | u << 23 | 1 << 22 | (n >> 4) << 8 | (n & 15)
            u = 0 if o.startswith("-") else 1
            return base | u << 23 | reg(o.lstrip("+-"))

        b = 1 if size == "b" else 0
        base = cond << 28 | 0b01 << 26 | pre << 24 | b << 22 | wb << 21 | load << 20 | rn << 16 | rd << 12
        if not offs:
            return base | 1 << 23
        o = offs[0]
        if o.startswith("#"):
            n = self.value(o)
            if not -4095 <= n <= 4095:
                raise AsmError(f"offset {n} out of range")
            return base | (1 if n >= 0 else 0) << 23 | abs(n)
        u = 0 if o.startswith("-") else 1
        rm = reg(o.lstrip("+-"))
        field = rm
        if len(offs) > 1:
            kind, amount = offs[1].split(None, 1)
            field |= self.value(amount) << 7 | SHIFT[kind.lower()] << 5
        return base | 1 << 25 | u << 23 | field

    def known(self, expr):
        """Whether every symbol in `expr` is defined above this line. LLVM decides a load's
        form where it meets it, so a label further down keeps it a pool load even though a
        later pass could have folded it; matching that is what makes the two byte-identical."""
        try:
            eval(expr, {"__builtins__": {}}, {k: self.symbols[k] for k in self.above})
            return True
        except NameError:
            return False

    def literal(self, expr, addr):
        """Address of a pool word holding `expr`, shared with any earlier load of the same
        expression waiting for the same pool (LLVM's other trick)."""
        if expr not in self.pending:
            self.pending.append(expr)
        return self.pool_addr.get((self.pool_number, expr), addr)

    # ---- passes --------------------------------------------------------------------------------

    def assemble(self, source):
        lines = []
        for raw in source.splitlines():
            line = raw.split(";")[0].split("@")[0].strip()
            if line:
                lines.append(line)
        self.pool_addr = {}
        # Twice before the last: a forward label decides whether a load is a mov or a pool
        # word, which moves every pool after it, which moves labels again.
        for self.final in (False, False, True):
            out = self.run(lines)
        return bytes(out)

    def run(self, lines):
        out = bytearray()
        self.pending = []  # expressions waiting for the next pool
        self.above = set()  # symbols defined so far in this pass
        self.pool_number = 0
        self.listing = []

        def addr():
            return self.base + len(out)

        def flush():
            while len(out) % 4:
                out.append(0)
            for expr in self.pending:
                self.pool_addr[(self.pool_number, expr)] = addr()
                v = self.value(expr) & 0xFFFFFFFF
                self.listing.append((addr(), v, f".word {expr}"))
                out.extend(v.to_bytes(4, "little"))
            self.pending.clear()
            self.pool_number += 1

        for line in lines:
            while True:
                m = re.match(r"([A-Za-z_.][\w.]*):\s*(.*)", line)
                if not m:
                    break
                self.symbols[m.group(1)] = addr()
                self.above.add(m.group(1))
                line = m.group(2)
            if not line:
                continue
            m = re.fullmatch(r"([A-Za-z_]\w*)\s*=\s*(.+)", line)
            if m:
                self.symbols[m.group(1)] = self.value(m.group(2))
                self.above.add(m.group(1))
                continue
            parts = line.split(None, 1)
            mnem, rest = parts[0], parts[1] if len(parts) > 1 else ""
            ops = split_ops(rest)

            if mnem.startswith("."):
                d = mnem.lower()
                if d == ".pool":
                    flush()
                elif d == ".align":
                    n = 1 << self.value(ops[0])
                    while len(out) % n:
                        out.append(0)
                elif d == ".org":
                    target = self.value(ops[0]) - self.base
                    if target < len(out):
                        raise AsmError(".org moves backwards")
                    out.extend(b"\0" * (target - len(out)))
                elif d in (".word", ".hword", ".byte"):
                    n = {".word": 4, ".hword": 2, ".byte": 1}[d]
                    for o in ops:
                        v = self.value(o) & ((1 << (8 * n)) - 1)
                        out.extend(v.to_bytes(n, "little"))
                elif d == ".ascii":
                    out.extend(eval(rest, {"__builtins__": {}}).encode("ascii"))
                else:
                    raise AsmError(f"unknown directive {mnem}")
                continue

            if len(out) % 4:
                raise AsmError(f"instruction at unaligned {addr():#x}")
            word = self.encode(mnem, ops, addr())
            self.listing.append((addr(), word, line))
            out.extend(word.to_bytes(4, "little"))
        flush()
        return out


def assemble(source, base=0x08000000):
    a = Assembler(base)
    return a.assemble(source), a


if __name__ == "__main__":
    src = open(sys.argv[1]).read()
    data, a = assemble(src)
    for addr, word, text in a.listing:
        print(f"{addr:08x}  {word:08x}  {text}")
