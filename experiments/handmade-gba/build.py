"""Builds life.gba: life.s through asm.py, the age ramp appended, the header checksummed."""

import colorsys
from pathlib import Path

from asm import assemble

HERE = Path(__file__).parent


def bgr555(r, g, b):
    return (round(b * 31) << 10) | (round(g * 31) << 5) | round(r * 31)


def ramp():
    """32 colours by age. 0 is the dead background, 1 a white flash of birth, then a walk
    from yellow round through red and magenta to a deep, dim blue."""
    out = [bgr555(0.03, 0.03, 0.08), bgr555(1, 1, 1)]
    for i in range(30):
        t = i / 29
        hue = (0.16 - 0.5 * t) % 1.0           # yellow → red → magenta → blue
        sat = 0.55 + 0.45 * min(1, t * 3)
        val = 1.0 - 0.55 * t
        out.append(bgr555(*colorsys.hsv_to_rgb(hue, sat, val)))
    return out


VARIANTS = {
    "life-rom-slow": {"SYNC"},
    "life-rom-fast": {"FAST", "SYNC"},
    "life": {"FAST", "IWRAM", "SYNC"},
    # Unsynced, to time a generation itself rather than the frames it is rounded up to.
    "life-rom-slow-nosync": set(),
    "life-rom-fast-nosync": {"FAST"},
    "life-nosync": {"FAST", "IWRAM"},
    "life-iwram-slow-nosync": {"IWRAM"},
}


def source(flags):
    """life.s with each `?FLAG` line kept when FLAG is on. ROM is on whenever IWRAM is not."""
    flags = set(flags) | ({"ROM"} if "IWRAM" not in flags else set())
    out = []
    for line in (HERE / "life.s").read_text().splitlines():
        if line.startswith("?"):
            flag, _, rest = line[1:].partition(" ")
            if flag not in flags:
                continue
            rest = rest.strip()
            line = rest if rest.endswith(":") else "        " + rest
        out.append(line)
    return "\n".join(out) + "\n"


def build(name="life"):
    src = source(VARIANTS[name])
    src += "".join(f"        .hword  {c:#06x}\n" for c in ramp())
    rom, listing = assemble(src)
    rom = bytearray(rom)
    chk = (-sum(rom[0xA0:0xBD]) - 0x19) & 0xFF
    rom[0xBD] = chk
    # Padded to 512 KB, the size of a small real cart, with the 0xFF an erased ROM reads as.
    # Not for the hardware, which always boots a cart from ROM: for mGBA, which loads a file
    # of 256 KB or less as a multiboot download into EWRAM whenever the code near its entry
    # holds more EWRAM addresses than ROM ones. Run from EWRAM, this program's first act,
    # clearing EWRAM for its worlds, erases itself.
    rom.extend(b"\xff" * (512 * 1024 - len(rom)))
    (HERE / f"{name}.gba").write_bytes(rom)
    return rom, listing, src


if __name__ == "__main__":
    for name in VARIANTS:
        rom, a, _ = build(name)
        n = sum(1 for line in a.listing if not line[2].startswith("."))
        print(f"{name}.gba: {len(rom)} bytes, {n} instructions")
