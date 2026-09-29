# Third-party licenses

`slot` itself is MIT (see the repo's top-level `LICENSE`). The release also distributes three
compiled libretro cores it did not write:

| Core            | Source                                   | License  | Text here                |
|-----------------|------------------------------------------|----------|---------------------------|
| `gpsp_libretro`  | https://github.com/libretro/gpsp        | GPL-2.0  | `gpsp-GPL-2.0.txt`        |
| `mgba_libretro`  | https://github.com/libretro/mgba        | MPL-2.0  | `mgba-MPL-2.0.txt`        |
| `snes9x_libretro`| https://github.com/libretro/snes9x      | Snes9x (non-commercial) | `snes9x-LICENSE.txt` |

gpSP was originally written by Gilead "Exophase" Kutnick; the libretro core above is the
actively maintained fork slot's fetch script pulls from. mGBA is by Jeffrey "endrift" Pfau.
libretro/mgba is libretro's fork of https://github.com/mgba-emu/mgba.

Both cores are built by this repo, and both are patched. `cores/gpsp/build.sh`, run by
`taskfile.yml`'s `core:gpsp`, builds libretro/gpsp at a pinned commit from the source archive
that ships here, with gpSP's own arm64 recipe and the patches in `cores/gpsp/` applied. mGBA
takes the same treatment: `cores/mgba/build.sh`, run by
`core:device` and `core:mgba:host`, builds libretro/mgba at a pinned commit with the patches in
`cores/mgba/` applied. `slot` never links against either. `taskfile.yml`'s `dist:device` task
copies this directory into the shipped tree alongside the cores it licenses, so a card built
from this repo carries the same notice the release zip does.

- **MPL-2.0 (mGBA): this build is modified, and the modifications ship in this directory.**
  The core is libretro/mgba at the commit recorded in `mgba-<commit>.meta`, with every patch
  from `cores/mgba/` applied. Each patch ships here too, its file name prefixed `mgba-`, and is
  itself under MPL-2.0; the rest of the Source Code Form is public at
  https://github.com/libretro/mgba at that commit. That is what MPL-2.0 sections 3.1 and 3.2
  require recipients be told, and this paragraph is that notice.

  The one patch today is upstream mGBA's own fix for the Classic NES Series audio,
  https://github.com/mgba-emu/mgba/commit/685023e05d90d87050fb357f46f7bd2d907083f5, which
  libretro/mgba had not picked up when this build was set up. Once it has, the patch can go.

- **GPL-2.0 (gpSP): the corresponding source ships in this directory, under section 3(a).**
  Section 3 allows conveying object code three ways: with the corresponding source, with a
  written offer for it, or — noncommercial only — by passing along an offer you received. This
  release takes the first and makes no offer: the source is here, in the same directory and
  the same zip and on the same card as the binary it corresponds to. There is nothing to
  request and nobody to request it from.

  `taskfile.yml`'s `core:gpsp` task downloads the source archive of the commit pinned as
  `GPSP_COMMIT` from GitHub, then compiles the binary from that archive with
  `cores/gpsp/build.sh` — all as one set (see below). `dist:device` and `deploy:device` carry
  the result right here, next to this notice, as:

  ```
  licenses/gpsp-<commit>.tar.gz
  licenses/gpsp-<commit>.meta
  licenses/gpsp-<patch>.patch
  ```

  named for the exact commit built, so the archive identifies its own source without needing a
  release page to point back to — which matters, because a card built and copied by hand never
  has one. The `.meta` file is the build's own record, in `key=value` form: the `commit`, the
  `source` archive's URL, the `recipe` it was built with (`make platform=arm64`, gpSP's own
  Makefile target), the `device_cflags` added to that recipe's flags, and a `patch=` line per
  patch with its sha256.

  **This build is modified, and these are the modifications** — that is what GPL-2.0 section
  2(a) asks be carried in the changed files, and this paragraph is the notice. Every patch
  `cores/gpsp/` holds ships here beside the archive, its file name prefixed `gpsp-`. Today
  there is one, slot's own: gpSP never reset its Advance Wars serial state when a netplay
  session began, so a session started while the game already sat on its link screen drained a
  master-side buffer as a slave, underflowed a length and overran a fixed array, which killed
  the frontend. It resets that state when a session starts and ends, and bounds the drain.

  "Corresponding" is exact here, not inferred: the binary is compiled from this archive plus
  those patches, and nothing else. The archive is GitHub's snapshot of `libretro/gpsp` at that
  commit, unmodified, its Makefile included, and the build adds only the patches and the
  compiler flags the `.meta` names. Earlier, slot shipped the libretro buildbot's nightly
  binary, which does not say which commit built it, and could only infer the source from the
  binary's timestamp. Building from the archive closed that gap.

  **The binary and the source are made, and remade, as one set.** `core:gpsp`'s status check
  requires the archive, the recorded commit, the binary and both metadata files to agree with
  the pin and with the build script's stamp. If any one does not, all of them are cleared, and
  the archive is refetched and the binary rebuilt from it in the same run, so nothing here can
  pair a binary from one build with a source recorded by another.

## snes9x

snes9x runs the SNES shelf. It is by Gary Henderson, Jerremy Koot and the many authors its
licence names; libretro/snes9x is libretro's fork of https://github.com/snes9xgit/snes9x.
`cores/snes9x/build.sh`, run by `taskfile.yml`'s `core:snes9x`, builds it unpatched from
libretro/snes9x at the pinned `SNES9X_COMMIT`, and keeps that checkout's own `LICENSE`.
`dist:device` ships the licence as `snes9x-LICENSE.txt` and the build record, naming the commit,
as `snes9x-<commit>.meta`.

**snes9x is not open source in the way the other two cores are.** Its licence allows the binary
and its source to be copied and passed on **for non-commercial purposes only**, free of charge,
and only with the licence and its copyright notice beside every copy — which is what
`snes9x-LICENSE.txt` is for. A release of ezGBA is free, so it may carry snes9x; but nothing
built from ezGBA may be sold, bundled into something sold, or used to promote something sold
with snes9x inside it without the snes9x authors' permission. Its source is public at the
commit the `.meta` names.

## Shaders

The game picture is scaled by a GLSL ES port of **Pixel AA** by fishku, released into the
public domain under CC0: the single pass `pixel_aa_gamma` path of libretro's slang-shaders
`pixel-art-scaling/shaders/pixel_aa/` (https://github.com/libretro/slang-shaders). Its
`slopestep` and the blending around it live in `GAME_FRAG` in
`crates/slot-gfx/src/shaders.rs`.

`scaler shimmerless` in `theme.txt` switches to a GLSL ES port of **sharp-shimmerless** by
zadpos, which its author released into the public domain. The port lives in `GAME_FRAG` in
`crates/slot-gfx/src/shaders.rs` and was taken from libretro's slang-shaders,
`pixel-art-scaling/shaders/sharp-shimmerless.slang`
(https://github.com/libretro/slang-shaders), itself Hyllian's optimisation of the original at
https://github.com/Woohyun-Kang/Sharp-Shimmerless-Shader. Public domain asks nothing; this is
credit, not a condition.

## Artwork

`slot` draws its own cartridges, its own slot and its own wordmark, and the two fonts it sets
type and glyphs in each ship with their licence beside them in `crates/slot-ui/assets/`. Three
drawings in that directory are somebody else's, and this is where they are credited. The SNES
shelf's cartridge (`snes_cart.svg`, `snes_cart_detail.svg`) and its mark (`platform_snes.svg`)
are ezGBA's own.

| File                                    | Drawing               | Creator              | Licence |
|-----------------------------------------|-----------------------|----------------------|---------|
| `crates/slot-ui/assets/platform_gba.svg` | Game Boy Advance SP  | O R I M Λ T          | CC BY   |
| `crates/slot-ui/assets/platform_gb.svg`  | Game Boy (DMG)       | costantino montanari | CC BY   |
| `crates/slot-ui/assets/platform_gbc.svg` | Game Boy Color       | Ryan Beck            | CC BY   |

They are the marks in the top plate's right corner that say which shelf the carousel is standing
on, one per platform. Each is a free download from the Noun Project, whose free tier licenses an
icon under Creative Commons Attribution in exchange for crediting the person who drew it:

- "Game Boy Advance SP" by O R I M Λ T, from the Noun Project:
  https://thenounproject.com/icon/game-boy-advance-sp-208211/
- "Gameboy Color" by costantino montanari, from the Noun Project:
  https://thenounproject.com/icon/gameboy-color-3633999/
- "Game Boy Color" by Ryan Beck, from the Noun Project:
  https://thenounproject.com/icon/game-boy-color-44993/

No version is named because the Noun Project does not name one: its own record of all three
reads `CREATIVECOMMONS`, and the icon pages above are the authority on the terms. Each icon is
titled here by the drawing rather than by the upload — costantino montanari's is titled
"Gameboy Color" on the Noun Project and draws the original Game Boy, which is why the file it
downloaded as, `noun_GameboyColor_3633999.svg`, is not what it is called in this repository.

**This section is the attribution, and the drawings no longer carry their own.** The free
download bakes the credit into the file, as two lines of type under the artwork reading
"Created by … from the Noun Project". They are stripped: the mark is drawn 32 px tall in the
corner of a status plate and cannot carry a sentence, and a credit rasterised down to that would
be a smudge rather than a credit. Stripping it is only allowed because the credit moved here,
where it is legible — and this directory ships: `taskfile.yml`'s `dist:device` copies it onto the
card beside the cores, so a device built from this repo carries this notice as well as the
artwork.

Nothing else about the drawings was changed. Every path is the artist's, and the only other
edit is to the `viewBox`, which had been sized to hold the credit line and is re-fitted to the
artwork it now holds; each file's own comment says what its bounds were and what they became.

Two things slot does to them at draw time, stated here because CC BY asks for changes to be
indicated and neither is visible in the files: the artist's black is replaced by the interface's
own near-white, since the mark is drawn over a dark plate and black would be a hole in it, and
the coverage the rasteriser reports is raised by a gamma before that ink goes through it, because
these strokes are thinner than a pixel at this size and would otherwise render grey. Both are
presentation, and `crates/slot-ui/src/mark.rs` is where they live.
