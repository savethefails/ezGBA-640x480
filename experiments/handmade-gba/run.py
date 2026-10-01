"""Runs life.gba on the mGBA libretro core with no frontend but this file, and checks it.

  python3 run.py [frames]

Writes life.gif (the screen, every other frame) and life-sheet.png, and checks every frame's
pair of worlds in work RAM against a Life step computed here, ages included.
"""

import ctypes as C
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).parent
CORE = HERE.parents[1] / "vendor" / "mgba_libretro.so"

W, H = 122, 82
WORLD_A, WORLD_B = 0x0000, 0x2800  # offsets into EWRAM
RETRO_MEMORY_SYSTEM_RAM = 2
JOYPAD_A = 8

ENV = C.CFUNCTYPE(C.c_bool, C.c_uint, C.c_void_p)
VIDEO = C.CFUNCTYPE(None, C.c_void_p, C.c_uint, C.c_uint, C.c_size_t)
SAMPLE = C.CFUNCTYPE(None, C.c_int16, C.c_int16)
BATCH = C.CFUNCTYPE(C.c_size_t, C.c_void_p, C.c_size_t)
POLL = C.CFUNCTYPE(None)
STATE = C.CFUNCTYPE(C.c_int16, C.c_uint, C.c_uint, C.c_uint, C.c_uint)


class GameInfo(C.Structure):
    _fields_ = [("path", C.c_char_p), ("data", C.c_void_p), ("size", C.c_size_t),
                ("meta", C.c_char_p)]


class Host:
    def __init__(self, rom):
        self.core = C.CDLL(str(CORE))
        self.frame = None
        self.fmt = 0
        self.held = set()
        self.sysdir = C.c_char_p(str(HERE).encode())

        def env(cmd, data):
            cmd &= 0xFFFF
            if cmd == 10:  # SET_PIXEL_FORMAT
                self.fmt = C.cast(data, C.POINTER(C.c_int)).contents.value
                return self.fmt in (1, 2)
            if cmd in (9, 31):  # system and save directories
                C.cast(data, C.POINTER(C.c_char_p))[0] = self.sysdir.value
                return True
            return False

        def video(data, w, h, pitch):
            if not data:
                return  # a duped frame: the last one stands
            raw = C.string_at(data, pitch * h)
            if self.fmt == 1:
                img = Image.frombuffer("RGBX", (w, h), raw, "raw", "BGRX", pitch, 1)
            else:
                img = Image.frombuffer("RGB", (w, h), raw, "raw", "BGR;16", pitch, 1)
            self.frame = img.convert("RGB")

        # Kept on self: ctypes does not keep a callback alive for the library holding it.
        self.cbs = [ENV(env), VIDEO(video), SAMPLE(lambda l, r: None),
                    BATCH(lambda d, n: n), POLL(lambda: None),
                    STATE(lambda port, dev, idx, i: int(port == 0 and dev == 1 and i in self.held))]
        for name, cb in zip(["environment", "video_refresh", "audio_sample",
                             "audio_sample_batch", "input_poll", "input_state"], self.cbs):
            getattr(self.core, f"retro_set_{name}")(cb)
        self.core.retro_init()
        self.data = rom.read_bytes()
        self.buf = C.create_string_buffer(self.data, len(self.data))
        info = GameInfo(str(rom).encode(), C.cast(self.buf, C.c_void_p), len(self.data), None)
        if not self.core.retro_load_game(C.byref(info)):
            raise SystemExit("the core would not load the rom")
        self.core.retro_get_memory_data.restype = C.c_void_p
        self.core.retro_get_memory_size.restype = C.c_size_t

    def run(self):
        self.core.retro_run()

    def ewram(self):
        p = self.core.retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM)
        n = self.core.retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM)
        return C.string_at(p, n) if p else b""


def step(world):
    """One Life generation by the program's own rules: ages count up to 255, births are 1."""
    out = bytearray(W * H)
    for y in range(1, H - 1):
        for x in range(1, W - 1):
            i = y * W + x
            n = sum(1 for d in (-W - 1, -W, -W + 1, -1, 1, W - 1, W, W + 1) if world[i + d])
            age = world[i]
            if age:
                out[i] = min(age + 1, 255) if n in (2, 3) else 0
            else:
                out[i] = 1 if n == 3 else 0
    return bytes(out)


COUNTER = 0x5000


def bench(name, frames=600):
    """Generations in `frames` frames, from frame 60 on, and how many of the world pairs
    seen along the way were an exact Life step apart."""
    host = Host(HERE / f"{name}.gba")
    exact = 0
    for _ in range(60):
        host.run()
    start = int.from_bytes(host.ewram()[COUNTER:COUNTER + 4], "little")
    for _ in range(frames):
        host.run()
        ram = host.ewram()
        a, b = ram[WORLD_A:WORLD_A + W * H], ram[WORLD_B:WORLD_B + W * H]
        if step(b) == a or step(a) == b:
            exact += 1
    gens = int.from_bytes(host.ewram()[COUNTER:COUNTER + 4], "little") - start
    return gens, exact


def main(frames):
    host = Host(HERE / "life.gba")
    shots, gif = {}, []
    checked = matched = 0
    seen = set()
    for f in range(frames):
        # A is pressed for a few frames at 900: the reseed on demand.
        host.held = {JOYPAD_A} if 900 <= f < 904 else set()
        host.run()
        if f % 2 == 0 and host.frame:
            gif.append(host.frame.resize((480, 320), Image.NEAREST))
        if f in (1, 30, 120, 300, 600, 899, 910, 1199):
            shots[f] = host.frame.copy()

        ram = host.ewram()
        a, b = ram[WORLD_A:WORLD_A + W * H], ram[WORLD_B:WORLD_B + W * H]
        key = (a, b)
        if key in seen or not any(a) or not any(b):
            continue
        seen.add(key)
        # At a frame's end the program is often part way through writing one world from the
        # other. When it is not, one world is exactly the Life step of the other, ages and all.
        checked += 1
        if step(b) == a or step(a) == b:
            matched += 1
    print(f"{checked} distinct world pairs read from work RAM, {matched} one generation exactly the step of the other")

    gif[0].save(HERE / "life.gif", save_all=True, append_images=gif[1:], duration=33, loop=0)
    keys = sorted(shots)
    sheet = Image.new("RGB", (4 * 250 + 10, 2 * 190 + 10), (24, 24, 28))
    for k, f in enumerate(keys):
        sheet.paste(shots[f], (10 + (k % 4) * 250, 10 + (k // 4) * 190))
    sheet = sheet.resize((sheet.width * 2, sheet.height * 2), Image.NEAREST)
    sheet.save(HERE / "life-sheet.png")
    print("frames:", keys)


if __name__ == "__main__":
    if sys.argv[1:2] == ["bench"]:
        # A frame is 228 lines of 1232 cycles at 16.78 MHz; a generation is 9600 cells.
        for name in ("life-rom-slow-nosync", "life-rom-fast-nosync", "life-iwram-slow-nosync",
                     "life-nosync", "life-rom-slow", "life-rom-fast", "life"):
            gens, exact = bench(name)
            cycles = 600 * 280896 / gens / 9600
            print(f"{name:24} {gens / 10:5.1f} generations/s  {600 / gens:5.2f} frames each  "
                  f"{cycles:4.0f} cycles a cell  ({exact} frames caught an exact step)")
    else:
        main(int(sys.argv[1]) if len(sys.argv) > 1 else 1200)
