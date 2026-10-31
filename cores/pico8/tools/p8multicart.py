#!/usr/bin/env python3
"""Make a PICO-8 multicart play with no internet.

A multicart loads its other parts from the PICO-8 forum by id, with load("#part_id"). With no
network, as on a handheld, the load stalls. This finds every part a cart loads, gets each one
(from files you already have, or by downloading it the way PICO-8 does), and writes copies of
the cart and its parts with every "#part_id" changed to the part's file name, so PICO-8 loads
it from next to the cart instead.

    python3 p8multicart.py Libryinth.p8.png
    python3 p8multicart.py Libryinth.p8.png --list        # only say what it loads
    python3 p8multicart.py game.p8.png --parts ~/Downloads   # parts already downloaded

The copies go in a new folder beside the cart, <name>-offline/; your cart is not changed.
Copy that folder's files into Games/PICO8 on the card. Needs Python 3 and nothing else.

Downloads, when needed, ask lexaloffle.com for the part's current revision (cpost_lister3.php)
and then fetch that revision (get_cart.php, or the cposts/ file), which is what PICO-8 does.
"""
import argparse
import re
import struct
import sys
import urllib.request
import zlib
from pathlib import Path

BBS = "https://www.lexaloffle.com/bbs"
CODE, ROM = 0x4300, 0x8000
CODE_MAX = ROM - CODE  # what PICO-8 will load back out of a .p8.png
PNG_SIG = b"\x89PNG\r\n\x1a\n"

# ---- PNG, just enough for a .p8.png: 8-bit RGBA, not interlaced ----------------------------


def png_read(data):
    if data[:8] != PNG_SIG:
        raise ValueError("not a PNG")
    at, idat, head = 8, b"", None
    while at < len(data):
        n, kind = struct.unpack(">I4s", data[at : at + 8])
        body = data[at + 8 : at + 8 + n]
        if kind == b"IHDR":
            head = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        at += 12 + n
    w, h, depth, colour, _, _, interlace = head
    if depth != 8 or interlace or colour not in (2, 6):
        raise ValueError("not a picture PICO-8 saved (8-bit RGB or RGBA, not interlaced)")
    bpp = 4 if colour == 6 else 3
    raw, stride = zlib.decompress(idat), w * bpp
    px, prev, i = bytearray(), bytearray(stride), 0
    for _ in range(h):
        f, line = raw[i], bytearray(raw[i + 1 : i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b, c = prev[x], prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        if bpp == 3:
            for x in range(w):
                px += line[x * 3 : x * 3 + 3] + b"\xff"
        else:
            px += line
        prev = line
    return w, h, px


def png_write(w, h, px):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    raw = b"".join(b"\0" + bytes(px[y * w * 4 : (y + 1) * w * 4]) for y in range(h))
    return (
        PNG_SIG
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def rom_of(px):
    # Each byte of the cart is hidden in the low two bits of a pixel's A, R, G and B.
    return bytearray(
        ((px[i + 3] & 3) << 6) | ((px[i] & 3) << 4) | ((px[i + 1] & 3) << 2) | (px[i + 2] & 3)
        for i in range(0, len(px), 4)
    )


def rom_into(px, rom):
    for n, v in enumerate(rom):
        i = n * 4
        px[i] = (px[i] & ~3) | (v >> 4 & 3)
        px[i + 1] = (px[i + 1] & ~3) | (v >> 2 & 3)
        px[i + 2] = (px[i + 2] & ~3) | (v & 3)
        px[i + 3] = (px[i + 3] & ~3) | (v >> 6 & 3)


# ---- The code's compression: PICO-8's "pxa", and the older ":c:" it still reads ------------


class Bits:
    def __init__(self, data):
        self.d, self.p = data, 0

    def bit(self):
        v = (self.d[self.p >> 3] >> (self.p & 7)) & 1
        self.p += 1
        return v

    def bits(self, n):
        return sum(self.bit() << i for i in range(n))


def pxa_decode(d):
    size = (d[4] << 8) | d[5]
    r, mtf, out = Bits(d[8:]), list(range(256)), bytearray()
    while len(out) < size:
        if r.bit():
            e = 0
            while r.bit():
                e += 1
            i = r.bits(4 + e) + (1 << (4 + e)) - 16
            c = mtf.pop(i)
            mtf.insert(0, c)
            out.append(c)
        else:
            width = (5 if r.bit() else 10) if r.bit() else 15
            off = r.bits(width) + 1
            if width == 10 and off == 1:  # a run of plain bytes
                while True:
                    c = r.bits(8)
                    if not c:
                        break
                    out.append(c)
            else:
                n = 3
                while True:
                    part = r.bits(3)
                    n += part
                    if part != 7:
                        break
                for _ in range(n):
                    out.append(out[-off])
    return bytes(out)


OLD_LUT = b"\n 0123456789abcdefghijklmnopqrstuvwxyz!#%(){}[]<>+=/*:;.,~_"


def old_decode(d):
    size, i, out = (d[4] << 8) | d[5], 8, bytearray()
    while len(out) < size:
        b = d[i]
        i += 1
        if b == 0:
            out.append(d[i])
            i += 1
        elif b < 0x3C:
            out.append(OLD_LUT[b - 1])
        else:
            b2 = d[i]
            i += 1
            off, n = (b - 0x3C) * 16 + (b2 & 15), (b2 >> 4) + 2
            for _ in range(n):
                out.append(out[-off])
    return bytes(out)


class Writer:
    def __init__(self):
        self.b, self.n = bytearray(), 0

    def bit(self, v):
        if self.n % 8 == 0:
            self.b.append(0)
        if v:
            self.b[-1] |= 1 << (self.n % 8)
        self.n += 1

    def bits(self, v, k):
        for i in range(k):
            self.bit((v >> i) & 1)


def literal_cost(i):
    e = 0
    while i >= (1 << (4 + e + 1)) - 16:
        e += 1
    return e, 2 + 2 * e + 4


def ref_cost(off, n):
    o = off - 1
    return (8 if o < 32 else 13 if o < 1024 else 17) + 3 * ((n - 3) // 7 + 1)


def pxa_encode(src):
    """Within a few bytes of PICO-8's own: a cheapest-path parse over back references and
    literals, the literals' cost read back off the move-to-front list of the last pass."""
    size = len(src)
    seen, matches = {}, []
    for p in range(size):
        best = {}
        if p + 3 <= size:
            key = src[p : p + 3]
            for q in reversed(seen.get(key, [])[-400:]):
                off = p - q
                if off > 32768:
                    break
                n = 3
                while p + n < size and src[q + n] == src[p + n] and n < 4000:
                    n += 1
                band = 0 if off <= 32 else 1 if off <= 1024 else 2
                if band not in best or best[band][1] < n:
                    best[band] = (off, n)
            seen.setdefault(key, []).append(p)
        matches.append(list(best.values()))
    lit = [8.0] * size
    smallest = None
    for _ in range(8):
        cost, choice = [float("inf")] * (size + 1), [None] * size
        cost[size] = 0
        for p in range(size - 1, -1, -1):
            c, ch = lit[p] + cost[p + 1], None
            for off, n in matches[p]:
                for k in list(range(3, min(n, 40) + 1)) + ([n] if n > 40 else []):
                    v = ref_cost(off, k) + cost[p + k]
                    if v < c:
                        c, ch = v, (off, k)
            cost[p], choice[p] = c, ch
        w, mtf, p, lit = Writer(), list(range(256)), 0, list(lit)
        while p < size:
            if choice[p] is None:
                b = src[p]
                i = mtf.index(b)
                e, lit[p] = literal_cost(i)
                w.bit(1)
                for _ in range(e):
                    w.bit(1)
                w.bit(0)
                w.bits(i - ((1 << (4 + e)) - 16), 4 + e)
                mtf.pop(i)
                mtf.insert(0, b)
                p += 1
            else:
                off, n = choice[p]
                for q in range(p, p + n):
                    lit[q] = literal_cost(mtf.index(src[q]))[1]
                w.bit(0)
                o = off - 1
                if o < 32:
                    w.bits(0b11, 2)
                    w.bits(o, 5)
                elif o < 1024:
                    w.bits(0b01, 2)
                    w.bits(o, 10)
                else:
                    w.bit(0)
                    w.bits(o, 15)
                k = n - 3
                while k >= 7:
                    w.bits(7, 3)
                    k -= 7
                w.bits(k, 3)
                p += n
        out = b"\0pxa" + struct.pack(">HH", size, len(w.b) + 8) + bytes(w.b)
        if smallest is None or len(out) < len(smallest):
            smallest = out
    if pxa_decode(smallest) != src:
        raise AssertionError("the compressed code does not read back")
    return smallest


# ---- A cart: its code in, its code out -----------------------------------------------------


class Cart:
    def __init__(self, path, data):
        self.path, self.data = path, data
        self.png = data[:8] == PNG_SIG
        if self.png:
            self.w, self.h, self.px = png_read(data)
            self.rom = rom_of(self.px)
            code = bytes(self.rom[CODE:ROM])
            if code[:4] == b"\0pxa":
                self.code = pxa_decode(code)
            elif code[:4] == b":c:\0":
                self.code = old_decode(code)
            else:
                self.code = code.split(b"\0")[0]
        elif data.lstrip().startswith(b"pico-8 cartridge"):
            m = re.search(rb"^__lua__\r?\n(.*?)(?=^__\w+__\r?$|\Z)", data, re.S | re.M)
            self.code = m.group(1) if m else b""
        else:
            raise ValueError("neither a .p8.png nor a .p8")

    def ids(self):
        """Every "#bbs_id" string in the code, in the order they first appear."""
        out = []
        for m in ID.finditer(self.code):
            if m.group(2).decode() not in out:
                out.append(m.group(2).decode())
        return out

    def patched(self, names):
        code = ID.sub(
            lambda m: m.group(1) + names[m.group(2).decode()].encode() + m.group(1)
            if m.group(2).decode() in names
            else m.group(0),
            self.code,
        )
        if not self.png:
            return self.data.replace(self.code, code, 1)
        packed = pxa_encode(code)
        if len(packed) > CODE_MAX:
            raise ValueError(
                f"the code no longer fits in a .p8.png with longer names "
                f"({len(packed)} of {CODE_MAX} bytes)"
            )
        rom = bytearray(self.rom)
        rom[CODE:ROM] = packed + bytes(CODE_MAX - len(packed))
        px = bytearray(self.px)
        rom_into(px, rom[: len(px) // 4])
        out = png_write(self.w, self.h, px)
        check = Cart(self.path, out)
        assert check.code == code and check.rom[:CODE] == self.rom[:CODE]
        return out


ID = re.compile(rb"""(["'])#([A-Za-z0-9_\-]+)\1""")

# ---- Getting a part ------------------------------------------------------------------------


def local_part(part_id, folders):
    """A part already on disk: <id>.p8.png or <id>.p8, or a download named with a revision,
    <id>-<n>.p8.png, the newest revision when there are several."""
    for folder in folders:
        for name in (f"{part_id}.p8.png", f"{part_id}.p8"):
            for f in folder.iterdir():
                if f.name.lower() == name.lower():
                    return f
        revs = []
        for f in folder.iterdir():
            m = re.fullmatch(re.escape(part_id) + r"-(\d+)\.p8(\.png)?", f.name, re.I)
            if m:
                revs.append((int(m.group(1)), f))
        if revs:
            return max(revs)[1]
    return None


def fetch(url):
    req = urllib.request.Request(url, headers={"User-Agent": "p8multicart"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


def download_part(part_id, bbs):
    lid = part_id
    try:
        info = fetch(f"{bbs}/cpost_lister3.php?nfo=1&version=000207e&lid={part_id}").decode(
            "utf-8", "replace"
        )
        m = re.search(r"lid:([A-Za-z0-9_\-]+)", info)
        if m:
            lid = m.group(1)
    except OSError as e:
        print(f"    could not look up {part_id}'s revision ({e}); trying it as given")
    folder = lid[:1] if lid[:2].isdigit() else lid[:2]
    tried = []
    for url in (
        f"{bbs}/get_cart.php?cat=7&play_src=0&lid={lid}",
        f"{bbs}/cposts/{folder}/{lid}.p8.png",
    ):
        try:
            data = fetch(url)
        except OSError as e:
            tried.append(f"{url}: {e}")
            continue
        if data[:8] == PNG_SIG or data.lstrip().startswith(b"pico-8 cartridge"):
            return lid, data
        tried.append(f"{url}: not a cart ({data[:40]!r})")
    raise OSError("no cart for #" + part_id + ":\n      " + "\n      ".join(tried))


# ---- The whole job -------------------------------------------------------------------------


def stem_of(path):
    name = path.name
    for ext in (".p8.png", ".png", ".p8"):
        if name.lower().endswith(ext):
            return name[: -len(ext)]
    return path.stem


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cart", type=Path, help="the cart you start the game from (.p8.png or .p8)")
    ap.add_argument("--list", action="store_true", help="only list the parts it loads")
    ap.add_argument(
        "--parts", type=Path, action="append", default=[],
        help="a folder of parts already downloaded (the cart's own folder is always looked in)",
    )
    ap.add_argument("--out", type=Path, help="where the copies go (default: <name>-offline/)")
    ap.add_argument("--offline", action="store_true", help="never download; use --parts only")
    ap.add_argument("--bbs", default=BBS, help=argparse.SUPPRESS)
    a = ap.parse_args()

    root = Cart(a.cart, a.cart.read_bytes())
    root_id = stem_of(a.cart).lower()
    out = a.out or a.cart.parent / f"{stem_of(a.cart)}-offline"
    folders = [p for p in a.parts] + [a.cart.parent]

    # Every cart in the game: the one given, then each part as a part names it, until every
    # part's own parts are found too. A part that loads the cart it came from is the cart.
    carts, files, todo = {root_id: root}, {root_id: a.cart.name}, list(root.ids())
    print(f"{a.cart.name} loads: " + (", ".join("#" + i for i in todo) or "nothing"))
    if a.list or not todo:
        if not todo:
            print("It is not a multicart, or it names its parts some other way.")
        return 0
    failed = []
    while todo:
        part = todo.pop(0)
        if part.lower() in carts:
            continue
        found = local_part(part, folders)
        try:
            if found:
                print(f"  #{part}: {found}")
                data = found.read_bytes()
            elif a.offline:
                raise OSError(f"not in {', '.join(map(str, folders))}")
            else:
                lid, data = download_part(part, a.bbs)
                print(f"  #{part}: downloaded {lid}")
            cart = Cart(Path(part), data)
        except (OSError, ValueError) as e:
            print(f"  #{part}: {e}")
            failed.append(part)
            continue
        carts[part.lower()] = cart
        files[part.lower()] = part + (".p8.png" if cart.png else ".p8")
        new = [i for i in cart.ids() if i.lower() not in carts and i not in todo]
        if new:
            print(f"    which loads: " + ", ".join("#" + i for i in new))
        todo += new
    if failed:
        print("\nCould not get: " + ", ".join("#" + i for i in failed))
        print("To download one by hand, open its lookup address, which answers lid:<id>-<n>,")
        print("then the cart address with that lid. Save it as <id>.p8.png in a folder and run")
        print("this again with --parts <that folder>.")
        for i in failed:
            print(f"  {a.bbs}/cpost_lister3.php?nfo=1&version=000207e&lid={i}")
            print(f"  {a.bbs}/get_cart.php?cat=7&play_src=0&lid=<the lid it gave>")
        return 1

    names = {i: files[i.lower()] for c in carts.values() for i in c.ids() if i.lower() in files}
    out.mkdir(parents=True, exist_ok=True)
    print(f"\nWriting to {out}/")
    for key, cart in carts.items():
        try:
            data = cart.patched(names)
        except ValueError as e:
            print(f"  {files[key]}: {e}")
            return 1
        (out / files[key]).write_bytes(data)
        changed = [i for i in cart.ids() if i in names]
        print(f"  {files[key]}" + (f"  ({', '.join('#' + i for i in changed)} -> local files)" if changed else ""))
    print(f"\nCopy every file in {out.name}/ into Games/PICO8 on the card, keeping these names,")
    print(f"and start the game from {files[root_id]}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
