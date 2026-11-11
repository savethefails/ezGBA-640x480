# Ground-up PICO-8 firmware

A firmware for the H700 (RG SP) built for one thing: playing PICO-8. It looks like ezGBA
from the outside. Inside, it drops slot's general-purpose frontend and libretro, and runs
PICO-8 directly on BaseOS's kernel and drivers.

Each item says whether it is **new**, **adapted** from what ezGBA and pico-8-save-states
already have, or **reused** as is.

## Why

In ezGBA a press passes through three frame loops (slot's, the libretro core's, and
PICO-8's) and a copy into shared memory and back. Each hop costs part of a frame. Measured
on the device, pico_ball's press-to-screen is about 20 ms after the frame-alignment work,
not counting the kernel's button scan or the panel. A purpose-built system has one loop,
inside PICO-8's own process, driven by the display's refresh.

Rough estimate, button to screen:

| | ezGBA now | Ground up |
|---|---|---|
| Waiting for the frame to pick up the press | ~8 ms (slot collects once per frame) | ~1 ms (read at frame start) |
| PICO-8 hand-off and core | ~6 ms | 0 (no hand-off) |
| Compositor, then swap | ~6 ms | ~1 ms (direct page flip) |
| PICO-8's own frame | (in the hand-off) | ~5–6 ms |
| Kernel button scan | ~5 ms average | ~0.5 ms with a custom kernel |

These are estimates from measured numbers, not measurements.

## Experiments first

Settle these before committing to the design:

- [ ] Can the BaseOS kernel do direct page flips (DRM/KMS) and multiple hardware display layers? Sections 2 and 3 depend on it.
- [ ] Does `pico8_64 -run #id` work? If so, multicarts need no patching.
- [ ] How much lag does a bare-bones version remove? `sdl.c` flipping pages itself, PICO-8 running and nothing else.

## 1. Boot and system base

- [ ] **Card layout (new):** BaseOS boot files plus our own root filesystem. System files read-only; user data (carts, saves, PICO-8 home, logs) on the FAT partition.
- [ ] **Init (new):** a tiny PID 1 that mounts, sets up hardware, starts the supervisor, and restarts anything that dies. Replaces BaseOS's userspace start-up.
- [ ] **Supervisor (new):** owns the device's state (shelf, playing, menu over game, dozing, powering off). Starts and stops PICO-8, routes buttons, coordinates the radio. Rust, borrowing slot's state machine.
- [ ] **First-boot setup (adapted):** check for the user's PICO-8 files (`pico8_64`, `pico8.dat`, verified by hash), set the clock, offer Wi-Fi.
- [ ] **Boot splash (adapted):** the boot picture until the shelf is ready. Target: shelf in under 3 s.

## 2. PICO-8 runtime

- [ ] **SDL stand-in, rewritten (adapted from `sdl.c`):** PICO-8 runs on it directly; no libretro, no shared-memory hop.
  - [ ] Video: scale 128×128 to the panel and flip pages itself through DRM/KMS, on its own hardware layer.
  - [ ] Input: read buttons straight from evdev, at the start of each PICO-8 frame.
  - [ ] Audio: play straight through ALSA with ~5 ms buffers.
  - [ ] Timing: steer PICO-8's clock to the display refresh (the frame alignment already built), so each frame starts as late as possible before the flip.
- [ ] **p8ss save states (reused):** suspend and resume across power-off, crash recovery.
- [ ] **Pre-started PICO-8 (new):** a paused PICO-8 process kept ready on the shelf, so a cart starts at once.
- [ ] **Sleep without saving (new):** pause the process and turn the panel off; wake is instant.
- [ ] **Launcher (new):** start carts by file, or by BBS id (`-run #id`, if it works).
- [ ] **Extra inputs (new):** L/R/Select for PICO-8's extras: mouse emulation, a keyboard overlay for the console and editor, pause-menu access.

## 3. Display and overlay

- [ ] **Overlay layer (new):** menus, HUD, toasts and the bezel on a second hardware layer over the game. The game's frames are never copied or composited.
- [ ] **UI renderer (adapted from slot-ui):** the current look (shelf, carts, plates, quick menu), drawing to the overlay layer.
- [ ] **Scaling and bezels (adapted):** integer 3× (384×384) with a frame, or a filtered full-screen option.

## 4. Library and carts

- [ ] **Cart scanner (adapted):** `.p8` and `.p8.png`, labels from the PNG, titles and authors from the code. Skips `._` files and caches results so boot doesn't rescan everything.
- [ ] **Shelf (adapted):** sorting, letter jumps, favourites, recently played, a separate shelf for multicarts.
- [ ] **Multicart handling (adapted from `pico8_patch` and `pico8_cache`):** parts detected when a cart is added, fetched in the background, and a cart shown as ready offline once complete. No patch button needed if `-run #id` works.
- [ ] **Cart metadata (new):** per-cart settings (scaling, button mapping), play time, last played.

## 5. Network and BBS

- [ ] **Wi-Fi (reused):** wpa_supplicant and udhcpc, the Wi-Fi page and keyboard.
- [ ] **BBS client (adapted from the wget wrapper and `pico8_cache`):**
  - [ ] revision lookups answered from the card, refreshed in the background;
  - [ ] rate-limit (HTTP 429) handling;
  - [ ] one download queue shared by Splore, PICO-8's own requests and the background prefetch.
- [ ] **Splore (new):** PICO-8's built-in BBS browser as a menu entry; downloaded carts added to the shelf.
- [ ] **DNS cache (new):** fast lookups, quick failures.
- [ ] **Time sync (new):** set the clock over the network when Wi-Fi is up; HTTPS certificates need it.
- [ ] **Cart import over Wi-Fi (optional, new):** a small web page served from the device.

## 6. Saves and data

- [ ] **Per-cart save data (reused):** PICO-8's own `cartdata` in its home folder.
- [ ] **Save states (adapted):** resume slot plus manual slots, with screenshots (the Polaroids view).
- [ ] **Safe writes everywhere (reused):** write to a temp file and rename. Detect and survive a card that has gone read-only.
- [ ] **Backup (new):** copy saves to a dated folder on request.

## 7. Power and hardware

- [ ] **CPU policy (new):** PICO-8 pinned to one core, the others off during play, clock speed just high enough for each frame. Probably the biggest battery win.
- [ ] **Lid and power button (adapted):** close to sleep, instant wake, long press to power off.
- [ ] **Battery (adapted):** gauge, low-battery warning, a save state before the battery runs out.
- [ ] **Brightness, volume, mute, blue light (reused):** the Select chords and HUD bars.
- [ ] **Headphone detection (reused).**

## 8. Updates and install

- [ ] **Installer (new):** a card image or zip, with clear steps for adding the user's own PICO-8 files.
- [ ] **Updates (new):** a new image dropped on the card and applied at boot, with rollback on failure. Later, over Wi-Fi from GitHub releases.

## 9. Diagnostics

- [ ] **Logs (reused, extended):** `pico8.log`, the download log and the latency trace, on by default with size limits, plus a "send logs" export.
- [ ] **Info page (new):** frame rate, alignment state, input-to-screen latency, battery, Wi-Fi.
- [ ] **Crash handling (adapted):** PICO-8 crashes caught, logged, and offered a resume from the last good state.

## 10. Build and test tooling

- [ ] **Cross-build (adapted):** the GitHub Actions release build, plus a build of the root filesystem.
- [ ] **ARM test rig (adapted):** qemu with PICO-8's ARM build for timing, saves, downloads and latency (`core_test --tap --period`), run on every change.
- [ ] **Device test hooks (new):** scripted button input and log capture on a real device.

## Later: a custom kernel

Once the above works on BaseOS's kernel, a kernel built from the H700 work in projects
such as ROCKNIX and Knulli could add:

- [ ] a 1000 Hz tick and interrupt-driven buttons (button scan from ~5 ms to under 1);
- [ ] suspend to RAM that wakes;
- [ ] faster boot.

That is a much bigger job (drivers, Wi-Fi firmware, the panel), worth it only after the
userspace design has proved itself.

## Order

1. The experiments above.
2. The runtime (2) and display (3).
3. The supervisor (1) and library (4).
4. Network and saves (5–6), mostly reuse.
5. Power, updates and diagnostics (7–9).

## Risks

- PICO-8 is a closed binary with undocumented behaviour (the ARM build drops to 30 fps on
  a clock that moves in whole frames). Every runtime change is tested on the ARM build, and
  on the device where possible.
- Direct display access under BaseOS is the main thing to prove before committing.
- PICO-8 is licensed: the firmware never ships it; the user supplies their own copy.
