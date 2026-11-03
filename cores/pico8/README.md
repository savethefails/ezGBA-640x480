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
link changed to the part's file. The cart is written as `<name>_patched.p8.png`
(`libryinth-10.p8.png` becomes `libryinth-10_patched.p8.png`), and its parts go in a folder
beside it, `libryinth-10_parts/`, which the shelf does not look in, so only the cart shows; copy
both into `Games/PICO8`. Your cart is left as it was.

A link is a `#part_id` in a string, alone or in a comma-separated list (`split"#game,#game_2"`),
or one of the file names a cart loads its parts by when it runs from a file rather than the
forum (`game_2.p8`). PICO-8 looks for a cart named in a `load` from its root, the cart's own
folder, so a part in the folder is named with the folder's name in front.

```
python3 tools/p8multicart.py libryinth-10.p8.png         # copies in libryinth-10-offline/
python3 tools/p8multicart.py libryinth-10.p8.png --list  # what it loads, nothing written
python3 tools/p8multicart.py game.p8.png --parts ~/Downloads --offline
```

Run it on a computer and copy the folder it writes into `Games/PICO8`. Python 3 and nothing
else. Parts are looked for on disk first: in `--parts` folders, beside the cart, and in the
folder where PICO-8 keeps what it downloads (`~/Library/Application Support/pico-8/bbs` on a
Mac), taking the revision PICO-8 chose from its `.nfo` file there. So once a game has been
played in PICO-8 on that computer, nothing needs downloading. Otherwise it asks the forum the
way PICO-8 does.

The core starts PICO-8 with the cart's own folder as its root (`-root_path`), which is where a
`load("name.p8.png")` looks, so the parts are found beside the cart.
