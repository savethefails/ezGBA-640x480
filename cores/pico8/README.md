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
at commit 5f6ff08: `src/` is its `src/` and `host/` its `host/`. Copy them across again to update.

## Multicarts

Some carts are several carts that load one another from the PICO-8 forum with
`load("#part_id")`. With no network the load stalls. `tools/p8multicart.py` makes such a game
play offline: it finds every part the cart loads, gets each one from a folder you name or by
downloading it the way PICO-8 does, and writes copies of the cart and its parts with each
`#part_id` changed to the part's file name. Your cart is left as it was.

```
python3 tools/p8multicart.py Libryinth.p8.png            # copies in Libryinth-offline/
python3 tools/p8multicart.py Libryinth.p8.png --list     # what it loads, nothing written
python3 tools/p8multicart.py game.p8.png --parts ~/Downloads --offline
```

Run it on a computer, which needs the network only for parts it has to download, and copy the
folder it writes into `Games/PICO8`. Python 3 and nothing else.
