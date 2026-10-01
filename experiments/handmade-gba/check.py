"""Assembles the same source with LLVM and compares it with asm.py's output, word by word.

LLVM is used only as a referee: nothing in life.gba comes from it.
"""

import subprocess
import sys
import tempfile
from pathlib import Path

from build import VARIANTS, build

BASE = 0x08000000


def llvm(src):
    # LLVM's .org counts from the start of the section rather than from the load address.
    src = src.replace(".org    0x080000A0", ".org    0xA0")
    src = ".syntax unified\n.arm\n.text\n.global _start\n_start:\n" + src
    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        (d / "life.s").write_text(src)
        r = subprocess.run(["llvm-mc", "-triple=armv4t-none-eabi", "-filetype=obj",
                            "-o", str(d / "life.o"), str(d / "life.s")],
                           capture_output=True, text=True)
        if r.returncode:
            print(r.stderr)
            sys.exit(1)
        if r.stderr:
            print("llvm-mc says:\n" + r.stderr)
        subprocess.run(["ld.lld", f"-Ttext={BASE:#x}", "-o", str(d / "life.elf"),
                        str(d / "life.o")], check=True)
        subprocess.run(["llvm-objcopy", "-O", "binary", str(d / "life.elf"),
                        str(d / "life.bin")], check=True)
        return (d / "life.bin").read_bytes()


def disasm(word):
    hexes = " ".join(f"0x{b:02x}" for b in word.to_bytes(4, "little"))
    r = subprocess.run(["llvm-mc", "-triple=armv4t-none-eabi", "--disassemble"],
                       input=hexes, capture_output=True, text=True)
    return " ".join(r.stdout.split()[1:]).replace(".text ", "") or "?"


def compare(name):
    rom, a, src = build(name)
    ref = llvm(src)
    rom = rom[:len(ref)]  # the padding is build.py's too
    ref = bytearray(ref)
    ref[0xBD] = rom[0xBD]  # the checksum is build.py's, not part of either assembler
    by_addr = {addr: text for addr, _, text in a.listing}
    diffs = 0
    for off in range(0, max(len(rom), len(ref)), 4):
        mine = int.from_bytes(rom[off:off + 4], "little")
        theirs = int.from_bytes(ref[off:off + 4], "little")
        if mine != theirs:
            diffs += 1
            text = by_addr.get(BASE + off, "(data)")
            print(f"{BASE + off:08x}  mine {mine:08x}  llvm {theirs:08x}   {text}")
            print(f"          mine reads as: {disasm(mine)}")
            print(f"          llvm reads as: {disasm(theirs)}")
    n = sum(1 for _, _, t in a.listing if not t.startswith("."))
    print(f"{name}.gba: {len(rom)} bytes, {n} instructions, {diffs} words differ from LLVM's")
    return diffs


if __name__ == "__main__":
    sys.exit(1 if sum(compare(name) for name in VARIANTS) else 0)
