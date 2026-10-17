# PICO-8

PICO-8 carts run on the official PICO-8, which the player supplies: it is licensed, so it never
ships with ezGBA. This folder builds the three files that run it:

- `pico8_libretro.so`, the core. PICO-8 can't be loaded as a library, so the core starts it as a
  separate process and trades its picture, sound and buttons through shared memory.
- `pico8/lib/libSDL2-2.0.so.0`, a stand-in for SDL that PICO-8 runs on. It implements only the SDL
  calls PICO-8 makes, and draws nothing itself, so PICO-8 needs no display or sound card of its own.
- `pico8/p8ss.so`, loaded into PICO-8 for save states, so suspend and resume work as they do for
  every other game.

The source is copied from [savethefails/pico-8-save-states](https://github.com/savethefails/pico-8-save-states)
at commit f766ecb: `src/` is its `src/` and `host/` its `host/`. Copy them across again to update.
