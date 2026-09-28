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
| **Picture** | `4:3` fills the screen; `3:2` is the GBA's own shape, with thin bars. |
| **LCD Grid** | `Off`, `On`, `Strict` or `LCD`, as described below. |
| **Grid Depth** | How dark the grid's lines are, from 10% to 100% in steps of 10. |
| **About** | Credits (A opens it). |

The picture and grid settings are saved to `System/theme.txt`, so the card remembers them,
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

## Updating

On a card that's already set up, only `System/slot` changes between builds. The **slot**
workflow builds just that file on every push to this repo, in a couple of minutes. Open the
latest **slot** run under Actions, download the artifact at the bottom, unzip it, and copy
`slot` into the card's `System` folder, replacing the old one. Your saves, `theme.txt` and
settings are left alone.

The **release** workflow builds the whole card, with both emulators, for setting up a new card.

## AI disclosure

Built with Claude's help, on top of slot., which was also built with Claude's help. Every
change was tested on real hardware.

## Credit

The hard work - the shelf, the look, the feel - is
[BrandonKowalski/slot](https://github.com/BrandonKowalski/slot). ezGBA is a small layer on top.
