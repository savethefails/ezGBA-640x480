# ezGBA

**No hotkeys. No combos. No menus to get lost in. Simple, simple, simple.**

ezGBA turns an Anbernic RG SP into a Game Boy Advance that anyone can pick up and play.
Made for kids, and for grown-ups who just want a simple GBA.

This fork is laid out for the **RG35XXSP** and its 640×480 screen.

<p align="center">
  <img src="media/shelf-advance-wars.png" width="45%" alt="The shelf: Advance Wars selected, its box art filling the screen above the carts">
  <img src="media/shelf-wario-land.png" width="45%" alt="The shelf: Wario Land 4 selected">
</p>

## See it in action

| Pick a game, play, eject | Brightness on L2/R2 | Close the lid, pick up where you left off |
|:---:|:---:|:---:|
| <img src="media/play-and-eject.webp" width="240" alt="Pressing A starts Zelda; MENU ejects back to the shelf"> | <img src="media/brightness.webp" width="240" alt="L2 and R2 dim and brighten the screen"> | <img src="media/lid-close-and-resume.webp" width="240" alt="Closing the lid mid-game, reopening, then powering on from off and resuming"> |
| A starts it. MENU ejects it. | One press, one step. | Your game is always saved. |

## How it works

| Button | What it does |
|---|---|
| **D-pad** | Pick a game |
| **A** | Play it |
| **MENU** | In a game: save and go back to your games. On the shelf: settings. With only one game on the card, it boots straight into it, and MENU still takes you to the shelf and its settings. |
| **L2 / R2** | Screen darker / brighter |
| **Close the lid** | Saves and sleeps. Open it within 3 minutes to keep playing. After that it turns itself off to save battery, and the next time you turn it on, your game picks up right where you left off. |

That's everything. Every button does one thing, on its own. No holding, no combos.

## What's different from slot.

ezGBA is built on [slot.](https://github.com/BrandonKowalski/slot). Here's what changed:

- **No hotkeys.** No button combos to learn or hit by accident.
- **L2/R2 control brightness.** In slot. they rewind and fast-forward the game, which is
  easy to press by accident mid-game.
- **One tap of MENU saves and ejects.** slot. needs MENU held for about a second.
- **The game's box art fills the screen.** Scroll to a game and see its cover, so you can
  find games by picture.
- **Your own colors.** Give each console its own background color in `System/theme.txt`.
- **12-hour clock.** Shows `4:39 PM`, not `16:39`.
- **A tiny settings menu.** Just Date & Time and About. Nothing in it can mess anything up.

Everything else, like lid-close saving, wireless trading, and the cartridge shelf, is slot.'s
work, unchanged.

## Settings

Tap **MENU** on the shelf for the settings. Up and Down pick a row; Left and Right change it:

| Row | What it does |
|---|---|
| **Date & Time** | Set the clock (A opens it). |
| **GBA Picture** | `4:3` fills the screen; `3:2` is the GBA's own shape, with thin bars. |
| **SNES Picture** | `Sharp` (the default): full width, every row exactly 2 screen rows, thin bars above and below. `4:3`: the whole screen, rows stretched, so thin lines can blur. |
| **LCD Grid** | `Off`, `On`, `Strict` or `LCD`, as described below. |
| **Grid Depth** | How dark the grid's lines are, from 10% to 100% in steps of 10. |
| **Scaler** | `Pixel AA` (the default) or `Shimmerless`: how game pixels are scaled up. |
| **Sharpness** | How hard Pixel AA's pixel edges are: `0.5`, `1.0` (the default, and the only one with no shimmer when the screen scrolls), `1.5` or `2.0`. |
| **Colour Depth** | `Off` (the default), `Rich`, `Deep` or `Custom`: deeper middle tones and fuller colour, after the image-adjustment settings RG35XXSP players use in RetroArch. See below. |
| **Run-Ahead** | `Off`, `1 Frame` (the default) or `2 Frames`. See below. |
| **SNES Emulator** | `Snes9x 2005` (the default, fast enough for run-ahead) or `Snes9x` (slower, more accurate). For the next SNES game started. |
| **About** | Credits (A opens it). |

All of these settings but Date & Time are saved to `System/theme.txt`, so the card remembers them,
and they're read back from it at startup, so editing the file by hand still works.

`System/theme.txt` holds a few more options that aren't in the menu:

| Line | What it does |
|---|---|
| `menu off` | Hide the settings menu, so little hands can't change anything. With one game on the card, MENU then does nothing in the game: it's a one-game console. |
| `scrim #F7E7CE` | Background color behind the shelf |
| `picture 3:2` | Show games in the GBA's own shape, with thin black bars above and below. Without it, games fill the whole screen (`picture 4:3`). |
| `sharpness 1.5` | How hard pixel edges are, from `0` (soft) to `2` (nearly hard). `1` is the default and the only setting with no shimmer at all. |
| `scaler shimmerless` | Scale games with sharp-shimmerless instead of Pixel AA. |
| `grid on` | Draw an LCD grid over games: a line between every pixel. The lines sit exactly where the pixels meet, so they're evenly spaced across the whole screen, and each pixel is brightened to make up for its lines. On white and fully saturated colours the lines are lighter, so those colours keep most of their brightness. |
| `grid strict` | The same grid, but brightness is never reduced: on white and fully saturated colours the grid fades out instead. |
| `grid lcd` | The grid as a real backlit LCD draws it: the lines darken every colour by the same amount and nothing is brightened to make up for it, so every colour keeps its place against every other and nothing washes out. The picture is dimmer (about a third at the default depth); turn the brightness up with R2 to make up for it. |
| `grid-depth 40` | How dark the grid lines are, from `5` (barely there) to `100` (black at their middle). `40` is the default. Try `60` if you can't see the grid at arm's length. With `grid lcd`, deeper also means a dimmer picture. |
| `colour-depth deep` | The Colour Depth row. `rich` deepens the middle tones as RetroArch's Monitor Gamma 2.0 does and adds a touch of colour; `deep` is Target Gamma 2.5, Saturation 1.10, Contrast 1.05. Black and white stay where they are. |
| `colour-gamma 1.14` | With `colour-depth custom`: how much darker the middle tones get, as RetroArch's Target Gamma ÷ Monitor Gamma (`1` changes nothing, `2.5 ÷ 2.2` is `1.14`). From `0.5` to `2`. |
| `colour-saturation 1.1` | With `colour-depth custom`: colour, from `0.5` to `2`. `1` changes nothing. |
| `colour-contrast 1.05` | With `colour-depth custom`: contrast about mid grey, from `0.5` to `2`. `1` changes nothing. |
| `snes-core snes9x` | The SNES Emulator row: `snes9x2005` or `snes9x`. A game's line in `System/selected_core.ini` wins over it. |
| `boot-picture off` | Start up with BaseOS's own logo instead of the last screen (see below). `boot-picture last` is the default. |

## Starting where you left off

The POWER button does two things:

| Press | What happens |
|---|---|
| Tap | The screen goes dark (the same as closing the lid). Tap again to carry on. After three minutes dark, the SP saves and powers off by itself. |
| Hold for 3 seconds | The SP saves your game, takes the picture below and powers off. |

When the SP powers off, slot saves the screen as it was just before: the game at
that moment, or the shelf. The next time you switch on, that picture is what the SP starts up
with, marked with a small "Starting" label so it reads as the SP starting up rather than a
screen that has stopped: low in the middle over a game, above the carts on the shelf. It's the
frame before the screen went dark or the shutdown screen came up, so nothing
else is drawn over it. The clock and battery in the corner of a shelf picture are as
they were at power off.

When it's a game, slot keeps the same picture on screen while the game loads, then cuts
straight to the game the moment it draws its first frame, with no cart animation in between,
so the picture simply comes to life. The screen stays at the bootloader's dim level the whole
time and comes up to your brightness with the game's first frame (or after ten seconds, if the
game never starts). When it's the shelf, slot opens on the cart that was highlighted, behind
the same wallpaper, and the brightness comes up with slot's first frame. With the boot picture
on, the wallpaper is kept from one session to the next rather than changing at each start, so
the shelf you start up on is the shelf you left. For this it keeps a copy in `System/last-screen.png`,
written only once the boot picture itself has been saved, and removed before every attempt,
so slot never shows a picture the SP didn't start up with.

The picture is BaseOS's `bootlogo.bmp`, which the bootloader shows from a small hidden
partition (`boot-resource`) on the card the system boots from. That partition also holds files
the SP needs to start, so slot writes to it as carefully as it can:

- It only ever replaces the picture inside the existing file: the same file, the same length,
  the same header. Nothing is created, renamed, resized or deleted, so the partition's layout
  is never touched. If the power is cut mid-write, the worst case is a picture half old and
  half new.
- It writes only if the file is already exactly the picture BaseOS ships: 640×480, 24-bit,
  uncompressed. Anything else and it leaves it alone.
- It finds the partition by name, on the card the system booted from, and only on BaseOS.
  If it can't tell which partition is the right one, it does nothing.
- Before the first time, it copies BaseOS's logo to `System/bootlogo-baseos.bmp` on your card.
  `boot-picture off` in `System/theme.txt` puts it back at the next power off and stops the
  screenshots. Keep that file: without it the original logo can only come back by
  reflashing BaseOS.
- It never holds up the power off: anything that goes wrong is logged to `/tmp/slot.log`
  and skipped, and it gives up after five seconds.

If the battery runs flat, or you keep holding POWER past six seconds so the SP's hardware cuts
the power itself, slot gets no chance to save,
and the next start shows the last picture it did save.

## The RG35XXSP's 640×480 screen

Everything is drawn for 640×480 directly, not made for another screen and shrunk to fit:

- **The shelf, menus, clock and About label are laid out for 640×480**, pixel for pixel.
- **The game fills the whole screen.** The GBA is a little wider than 4:3, so games look
  about 11% narrower than on a real GBA. To keep the GBA's exact shape instead, add
  `picture 3:2` to `System/theme.txt`: the game is then 640×427, with thin black bars above
  and below.
- **Sharp pixels, no shimmer.** 240 doesn't go into 640 a whole number of times, so each game
  pixel is 2 or 3 screen pixels wide. The picture is scaled with Pixel AA, the RetroArch
  shader, which keeps every pixel solid and blends only the one screen pixel where two meet,
  mixing them as light rather than as numbers, so edges keep their weight and scrolling
  doesn't shimmer. Down the screen, 4:3 is exactly 3 rows per game pixel, so nothing is
  blended at all.
- **An optional LCD grid made for this screen.** `grid on` in `System/theme.txt` draws evenly
  spaced lines between the pixels, with each pixel brightened to make up for them. It's
  off unless you turn it on.
- **Game Boy and Game Boy Color games are exactly 3×**: 480×432, every pixel three screen
  pixels square, with a border round them. L stretches one to fill the screen; R puts it back.
- **SNES games fill the screen's width, with every row exactly 2 screen rows** (1 in the
  512×448 hi-res mode): 640×448, with a thin bar above and below. Stretched to the full 480,
  rows would come out 2 or 3 screen rows tall, and a one-pixel outline on a letter could thin
  out or blend away; this way every line is kept. It's within 7% of the 4:3 TV shape. Set
  SNES Picture to `4:3` in the menu, or `snes-picture 4:3` in `System/theme.txt`, for the
  whole screen instead. SNES games take no LCD grid, since a TV has none.

## Responsive controls

Most games take a frame or two to answer a button, on top of the device's own delay. ezGBA
takes both down:

- **Run-ahead**, as RetroArch has it. Every frame, the emulator runs the real frame, saves the
  game, runs one frame further on the same buttons and shows that, then goes back. What you
  see is where the game will be a frame later, so a frame of the game's own lag is gone. It
  costs one extra frame of emulation and a save and load, every frame. If a game is too heavy
  for that, ezGBA switches run-ahead off for it by itself and plays it normally. Set it in the
  menu, or with `runahead 0`, `1` or `2` in `System/theme.txt`. Two frames removes more lag
  but costs more, and in a few games shows a brief flicker when a guess is wrong. It is
  always off in a link session.
- **In step with the screen.** The emulator runs each frame right after the buttons are read
  and just before the frame is shown, instead of on a clock of its own that drifts against
  the screen's, which could leave a press waiting up to a frame longer.
- **Faster button scanning.** The SP checks its buttons on a timer, every 20 ms out of the
  box, so a press waited 10 ms on average before anything could see it. ezGBA asks for every
  10 ms, the fastest the system allows.

### Measuring it

To see the timing on your own SP, create an empty file named `latency-trace.log` at the top of
the SD card and play. ezGBA adds lines to it as it runs; delete the file to stop.

- A `press:` line follows one button press from the moment ezGBA reads it to the moment the
  screen shows the frame it changed, stage by stage, in milliseconds.
- A `pace:` line every ten seconds sums up the screen's real refresh period, how long the
  emulator's frames take (run-ahead included), and how long each finished frame waited
  before the screen showed it.

It can't see the game's own frames of lag, or the wait before the button scan.

## SNES games

Put `.sfc` or `.smc` files in `Games/SNES/`. Every game, whatever it's for, stands on the one
shelf in order of its name, each in its own console's cartridge. Every button is the
SNES's own, X and Y included, and L2/R2 are still brightness. Box art and labels work as they
do for the GBA, from `Backdrops/SNES/` and `Labels/SNES/`.

SNES games run on **snes9x**. Unlike the two GBA emulators, snes9x's licence allows it to be
shared only for free and non-commercially, with its licence beside it (it ships as
`System/licenses/snes9x-LICENSE.txt`). ezGBA is free; just don't sell anything with it inside.

SNES games can also run on **snes9x2005_plus**, an older and lighter snes9x (about three
times as fast on the SP) so that run-ahead fits, and that is the default: choose between the
two with SNES Emulator in the menu. Put `snes9x2005_plus_libretro.so` in `System/` (from the
**slot** workflow's `snes9x2005` artifact, with its `licenses/` beside it). Each emulator keeps
its own save states, so a game suspended on one starts from its last in-game save on the other;
battery saves are shared. To keep one game on one emulator whatever the menu says, add
`Game Name = snes9x` (or `snes9x2005_plus`) to `System/selected_core.ini`.

## Setup

1. Flash **BaseOS for the RG35XXSP**: `baseos-rg35xxsp-<version>.img.zip` from
   [BaseOS's releases](https://github.com/pvaibhav/BaseOS/releases), following its
   [install guide](https://github.com/pvaibhav/BaseOS/wiki/BaseOS-Install-Guide). BaseOS
   starts ezGBA by itself. Don't use AGS-102's `ags102.img`: it is built for the 720×480
   RG SP, and on an RG35XXSP the screen stays dark.
2. Download the latest ezGBA from [releases](../../releases).
3. Unzip it and copy the **contents** of the folder inside onto the card: onto the second
   SD card if you use two, or onto the card's `BASEOS` drive if you use one.
4. **Add box art (optional).** Box art isn't included, since it belongs to the publishers.
   Put a 640×480 PNG in `Backdrops/GBA/`, named the same as the game, for example
   `Backdrops/GBA/Pokemon - FireRed Version (USA).png`. Keep the art in the top 270 pixels
   so the carts don't cover it. Art made for the 720×480 RG SP still works: it is centred
   and loses 40 pixels off each side.
   The first boot after adding art is a little slower: ezGBA keeps a ready-scaled copy of
   each picture in `System/Cache`, so later boots and scrolling don't have to decode it again.
   Deleting that folder is safe; it is rebuilt.

## Updating

On a card that's already set up, only `System/slot` changes between builds. The **slot**
workflow builds just that file on every push to this repo, in a couple of minutes. Open the
latest **slot** run under Actions, download the artifact at the bottom, unzip it, and copy
`slot` into the card's `System` folder, replacing the old one. Your saves, `theme.txt` and
settings are left alone.

The **release** workflow builds the whole card, with all three emulators, for setting up a new
card. A card set up before SNES support needs `System/snes9x_libretro.so` from a release as
well as the new `System/slot`.

## AI disclosure

Built with Claude's help, on top of slot., which was also built with Claude's help. Every
change was tested on real hardware.

## Credit

The hard work - the shelf, the look, the feel - is
[BrandonKowalski/slot](https://github.com/BrandonKowalski/slot). ezGBA is a small layer on top.
