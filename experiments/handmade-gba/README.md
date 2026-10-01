# A GBA program written without a compiler

An afternoon's curiosity, not part of ezGBA: can a Game Boy Advance program be written with no
compiler and no assembler, each ARM instruction's bits put in place by hand, and run on the
same mGBA core ezGBA ships?

It can. `life.gba` is Conway's Game of Life on a 120×80 world in 2×2 pixel cells. Cells are
coloured by age: white at birth, through yellow, orange, red and magenta, into a deep blue for
the still lifes. A reseeds, and so does every 1200th generation. It runs on the SP: copy
`life.gba` into `Games/GBA/`.

| file | what it is |
|---|---|
| `life.s` | the program, 127 ARM instructions |
| `asm.py` | the assembler: a two-pass encoder for the ARMv4 subset `life.s` needs, written from the instruction formats |
| `build.py` | `life.s` → seven `.gba` variants, with the age ramp and the header checksum |
| `check.py` | assembles the same source with LLVM (`llvm-mc` + `ld.lld`) and compares every word |
| `run.py` | a libretro frontend in 150 lines of Python `ctypes`: runs a ROM on `vendor/mgba_libretro.so`, records `life.gif`, checks every generation against Life computed in Python, and `run.py bench` times the variants |

```
python3 build.py && python3 check.py && python3 run.py bench
```

## What I found out

**The encodings came out right the first time.** Of the 109 instructions in the first version,
every one LLVM assembled to the same bits as `asm.py`. The 40 words that differed all came
from `ldr rX, =constant`, where LLVM does two things I had not: when the constant fits ARM's
rotated 8-bit immediate it emits a `mov` (or an `mvn` of the complement) instead of a load
from memory, and it shares one pool word between loads of the same constant. With both taught
to `asm.py`, every build is byte-identical to LLVM's.

**One difference comes from how LLVM is built, not from a mistake.** `ldr r2, =(step_end - step) / 4`
names a label further down. `asm.py` makes three passes, knows the value is 61, and wants a
`mov`. LLVM decides each instruction's form where it meets it, so a forward reference stays a
pool load. `asm.py` now follows the same rule.

**LLVM accepts `mla r11, r11, r3, r4` without a word**, though on the GBA's ARM7TDMI a multiply
whose destination is also its first operand is unpredictable. The fix is to swap the two
multiplicands.

**mGBA guesses what a small file is, and guessed wrong.** A file of 256 KB or less might be a
multiboot program, the kind a GBA downloads over the link cable and runs from EWRAM.
mGBA's `GBAIsMB` decides by scanning the first 128 words after the header. If it finds an
address into EWRAM (`0x02xxxxxx`) and fewer than two into cartridge ROM (`0x08xxxxxx`), it calls
the file multiboot and loads it at `0x02000000`. Two of the first builds hold the worlds'
EWRAM addresses and only one ROM address, so they ran from EWRAM. The program's first act, clearing
EWRAM for the worlds, then erased itself, and the screen stayed black. Another build happened to
load two ROM addresses and worked. Real hardware never guesses: a cart boots from ROM. The
builds are now padded to 512 KB, a real cart's size, which takes them out of the guess.

**mGBA's libretro core reports the wrong size for a GBA's work RAM.** `retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM)`
returns the GBA's 256 KB EWRAM, but `retro_get_memory_size` returns `GB_SIZE_WORKING_RAM`,
the Game Boy's 32 KB, whatever the platform (`src/platform/libretro/libretro.c`). A frontend
that trusts the size sees only the first eighth. Both worlds fit in that eighth, so the check
still works.

**Where the code runs matters more than anything in it.** These are the same instructions, timed
unsynced over 600 frames at 280,896 cycles a frame, for 9,600 cells a generation:

| where the inner loop runs | cycles per cell | generations per second |
|---|---|---|
| cartridge ROM, power-on wait states | 351 | 5.0 |
| cartridge ROM, `WAITCNT = 0x4317` (3/1 wait states, prefetch) | 193 | 9.1 |
| IWRAM, power-on ROM wait states | 96 | 18.3 |
| IWRAM, `WAITCNT = 0x4317` | 94 | 18.7 |

ROM sits on a 16-bit bus with wait states, so every 32-bit ARM instruction is two slow reads,
and the loop spends most of its time fetching itself. IWRAM is 32 bits wide with no wait
states. With the loop copied there, the ROM's timing barely matters: only the outer loop still
runs from ROM. What is left is close to the sum of the parts: about 42 one-cycle
instructions, plus nine byte loads from EWRAM, whose two wait states put a few cycles on each.

`life.gba` runs from IWRAM and waits for vblank between generations, so its 3.2 frames round up
to 4: 15 generations a second. In `run.py bench`, every one of its 150 generations in ten
seconds matched Life computed in Python, ages included.
