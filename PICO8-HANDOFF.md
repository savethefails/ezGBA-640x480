# PICO-8 save states and ezGBA: handoff

## Rules from the user
- Never ship or commit the licensed PICO-8 binaries (`pico8_64`, `pico8_dyn`, `pico8.dat`). Users supply their own.
- Don't open PRs. New branches are fine.
- Commits must end with:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_016SVBSLttXdH8LBziKG1wX7
  ```
- Keep the original faux PICO-8 cart (240x184 with grip ridges). A slimmer one was tried and reverted at the user's request.

## Repos and branches
- **savethefails/pico-8-save-states**, branch `prototype/ld-preload`, pushed.
  - `src/p8ss.c` is the save-state library loaded with `LD_PRELOAD`.
  - `host/pico8_libretro.c` is the libretro core. It runs PICO-8 as a child process on a stand-in `libSDL2` (`host/sdl.c`) over shared memory.
  - States are about 90 KB, using a page dictionary (about 9 MB, written on the first save).
  - `make test` runs the full matrix, which passes on x86-64 and on aarch64 under qemu.
- **savethefails/ezgba-640x480**, branch `pico8`, pushed (3 commits on top of main 78438fe):
  - **Platform:** `Platform::Pico8` and `Core::Pico8` reading `Games/PICO8` (`.p8.png` or `.p8`).
  - **Faux cart:** `pico8_cart.svg`, with the `.p8.png`'s 128x128 picture as the label.
  - **Picture:** drawn at 480x480, centred (`Fit::Aspect(1.0)`).
  - **Rewind and run-ahead:** off for PICO-8 (`emu.set_history(false)`).
  - **Core sources:** vendored in `cores/pico8/`, built by `task core:pico8` and copied by `dist:device`.
  - **README:** has a PICO-8 section.

## Device layout
- `BIOS/pico8/`: `pico8_64` and `pico8.dat` (from the user's Raspberry Pi zip).
- `System/`: `pico8_libretro.so`.
- `System/pico8/`: `p8ss.so` and `lib/libSDL2-2.0.so.0`.
- `Saves/pico8/p8ss/`: the live state and the page dictionary.

## Testing
- **End-to-end:** `crates/slot/tests/pico8.rs` runs a real cart through the frontend (shelf, play at 480x480, eject and resume, SELECT+R1 save). It's skipped unless set up:
  ```
  make -C cores/pico8 OUT=/tmp/p8core
  PICO8_DIR=<dir with pico8_dyn + pico8.dat> PICO8_CORE_DIR=/tmp/p8core SCRATCH_PNG_DIR=/tmp \
    cargo test --release -p slot --test pico8 -- --nocapture
  ```
- **Headless rendering:** needs Mesa EGL (`apt install libegl1 libegl-mesa0 libgles2 libgl1-mesa-dri`).
- **Host builds:** need `libudev-dev` and `libasound2-dev`.
- **Failures already on main** (not caused by this work): the 7 tests `both_b_and_menu_take_the_about_screen_back_to_the_quick_menu`, `the_rewind_bar_is_up_while_l2_is_held_and_gone_once_it_is_let_go`, `the_compliance_block_is_the_credits`, `the_badge_follows_the_latch_rather_than_the_button`, `an_unchanged_battery_save_is_not_rewritten`, and the two `sfx` tests. With the real mGBA core in `vendor/`, `a_game_that_will_not_load_again_comes_back_out_of_the_slot` fails too.
- **Timing-flaky under load:** `a_kick_a_frame_brings_the_present_right_after_it`, `sound_keeps_real_time_with_the_display_at_16_8_ms_and_a_kick_missed_now_and_then` (about one run in eight, before and after the cool-rubin merge), and `colour_depth_and_the_snes_emulator_are_kept_on_the_card`.

## Not done yet
- Never run on real RG35XXSP / H700 hardware. First-save latency there is unknown (about 0.5 s on x86, about 10 s under qemu).
- `task dist:device` not run end to end. The aarch64 core was cross-built by hand with `aarch64-linux-gnu-gcc`.
- Untested: Splore, the editor, GIF recording, and the 32-bit Pi build.

## Gotchas and the workarounds in place

### p8ss (the save-state library)
- **Resuming in exit mode:** when a state is loaded, `do_save()` returns a second time, with 1. `save_and_exit` checks for that and doesn't quit, or a resumed game would exit straight away.
- **Memory differs between runs:** PICO-8's memory pages aren't the same from run to run, so a page dictionary built at frame 1 by another process barely matched (states came out at 9 MB). The dictionary is now built at the first save, from that save's own arena. A new one is made when a save would store more than 4096 pages in full. Each state records its dictionary's id, and loading checks it.
- **glibc 2.38 symbols:** with newer glibc headers, `atoi`/`strtol` link to `__isoc23_strtol` (GLIBC_2.38), which BaseOS (2.35) doesn't have. The code parses numbers by hand instead. `make` fails if any symbol needs glibc newer than 2.35.
- **Unused-result warnings:** `asprintf` was replaced with `snprintf` into static buffers. The build passes `-Wno-format-truncation`.
- **Include guard:** it was renamed to `P8HOST_H_INCLUDED` because `P8HOST_H` clashed with another name.

### libretro core
- **When saving happens:** frontends call `retro_serialize_size` before `retro_serialize`. The save therefore happens in `serialize_size`: remove the old file, send SIGUSR1, wait up to 30 s for the new file, and cache it for `serialize`.
- **Why 30 s:** under qemu the first save, which also writes the dictionary, takes about 10 s. On x86 it takes about 0.5 s.
- **Loading:** `unserialize` writes the buffer to `live.p8ss`, waits for `ready`, then sends SIGUSR2. The child runs with `P8SS_LOAD_FILE=1` so every load reads that file.
- **Pausing:** PICO-8 waits at present when the host's heartbeat (`host_ns`) is more than 200 ms old, so pausing the frontend pauses PICO-8. This is intended.
- **Child process:** PICO-8 is started with `PR_SET_PDEATHSIG` so it dies with the frontend.
- **Stand-in SDL picture:** PICO-8 draws a 512x512 texture to 80,0 at 480x480. The stand-in keeps the largest `RenderCopy` texture and scales it down to 128x128.
- **Audio:** PICO-8's callback is driven from the stand-in's own thread, keeping about 2048 samples queued.

### Core tests (pico-8-save-states)
- **Buffered output:** cart output is block-buffered on a pipe, so the tests run PICO-8 under a pty (`script -qefc`).
- **aarch64 under qemu:** there's no binfmt, so exec fails with 127. `P8CORE_WRAPPER=<path to qemu-aarch64>` runs the child through qemu.
- **Pause check:** the pause test prints `pause start`/`pause end` and counts frames only between them, so the save doesn't skew the count.
- **pkill:** never `pkill -f 'Xvfb :77'` from a shell whose own command line contains that text. It kills the shell.

### ezGBA
- **Rewind and run-ahead off:** both call serialize every frame or two, and a PICO-8 state takes about 0.5 s. `Session` calls `emu.set_history(core != Core::Pico8)`.
- **`.png` scanning:** a `.png` in `Games/PICO8` only counts if its stem ends in `.p8`. The label is cropped from (16,24), 128x128, out of the 160x205 `.p8.png`.
- **Folder order:** `root::DIRS` must list `BIOS` before `BIOS/pico8`, because the tests create folders with plain `create_dir`.
- **One cart boots straight in:** a card with exactly one cart seats it at boot, so `tests/pico8.rs` adds a GBA cart to reach the shelf.
- **Black frame in tests:** `Session::frame()` takes the latest frame. Compose straight after taking one and there's nothing to draw (black), so step once first.
- **Headless rendering:** it needs Mesa EGL. Without it the GPU tests return early and "pass" in 0.00s, so check the time.
- **Running the whole suite:** `cargo test --workspace` stops at the first failing test binary, so use `--no-fail-fast`.
- **rustfmt:** the local rustfmt reformats `slot_chrome.rs` even though it's unchanged. The branch keeps main's text, so don't commit that.
- **Upstream tracking:** this clone's fetch refspec only covered `main`, so `git status` said `pico8` was unpushed. The fix was `git config --add remote.origin.fetch '+refs/heads/pico8:refs/remotes/origin/pico8'` and then `git branch -u origin/pico8`.

### Merged from `claude/cool-rubin-0nm03s` (ports from slot)
`pico8` now carries that branch. What it changed that PICO-8 work runs into:
- **Audio is 48 kHz** (`audio::OUTPUT_HZ`, was `GBA_HZ` at 32768) through a 48-tap windowed sinc. `Resampler`'s API is unchanged; it holds 24 source frames of history, and the ring is sized for 48 kHz. Recheck the device trace's `audio:` and `pico8:` lines after this merge.
- **Cart headers are read on demand:** after a scan `Cart.title` and `Cart.code` are empty; call `slot_store::header_ids(&cart)`. It answers empty for PICO-8.
- **The scan lists each art folder once** (lowercase keys). A `.p8.png` with no picture in `Labels/PICO8/` is its own label; the key is the stem plus `.png`, so `name.p8.png`.
- **Faces off the boot path:** the first frame's faces (the PICO-8 cart shadow among them) upload at boot; the rest are built on a thread. A new face the first frame can show belongs in the boot set.
- **Link:** `System/slotlink.sh` replaces `ags-net`; `dist:device` and `deploy:device` copy all of `card/System/*`.
- **Saves:** `persist::try_read_sav` and `try_read_resume` tell a missing file from an unreadable one, and the session refuses to write over the latter. Use them for any new save path.
- **Settings files are `.txt`**, renamed from `.ini` once at boot.
- **mGBA ships a flash bank patch** (`cores/mgba/flash-bank-survives-mask.patch`); its test needs a `vendor/mgba_libretro.so` built with it.
- **Test builds:** `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0` keeps `target/` near 1.2 GB instead of about 29 GB.
