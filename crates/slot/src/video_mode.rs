//! How each console's picture is placed on the panel, and for a Game Boy cart which of its two
//! sizes it was last left in.
//!
//! A GBA picture follows the card's `picture` setting, 4:3 or 3:2. A SNES picture fills the
//! panel's width with its rows at exactly 2x (1x in hi-res), 640x448: within 7% of the 4:3
//! television its games were drawn for, and with every row the same height. A Game Boy picture is its own
//! 10:9 at a whole multiple: 160x144 at 3x is 480x432, every pixel three panel pixels square.
//!
//! The Game Boy and the Game Boy Color had no shoulder buttons, so on one of their carts slot
//! takes L and R: L stretches the picture to fill the panel, R gives back its own size,
//! centred. On a GBA or SNES cart the two are the console's own and this file has nothing to
//! say.
//!
//! The stretch distorts, deliberately. 160x144 is 10:9 against the 4:3 panel, so a fullscreen
//! Game Boy picture comes out about 20% wider than it is tall. That is what a Game Boy picture
//! blown up to fill a television looked like, and it is the mode the user asked for by name.

use std::path::Path;

use slot_gfx::Fit;
use slot_store::{ini, Platform, SnesPicture};

/// A sibling of `selected_core.ini`, in the same `<stem> = <value>` shape and read by the same
/// parser. Flat and stem-keyed like that one, which means a `GB/Tetris.gb` and a
/// `GBC/Tetris.gbc` share a line. Deliberately: it is a two-value cosmetic preference that
/// cannot lose anybody's data, and the worst it can do is open a Colour cart stretched because
/// its same-named sibling was.
pub const VIDEO_MODE_FILE: &str = "Config/video_mode.ini";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VideoMode {
    /// The picture at the largest whole multiple of itself the panel holds, centred: a Game
    /// Boy's 160x144 comes out 480x432, square pixels and the right shape.
    #[default]
    Actual,
    /// The picture over the whole panel, aspect and all.
    Stretch,
}

impl VideoMode {
    /// The ini's spelling, meant to be typed by hand into a text editor on a computer.
    pub fn as_str(self) -> &'static str {
        match self {
            VideoMode::Actual => "actual",
            VideoMode::Stretch => "stretch",
        }
    }

    pub fn parse(s: &str) -> Option<VideoMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "actual" => Some(VideoMode::Actual),
            "stretch" => Some(VideoMode::Stretch),
            _ => None,
        }
    }
}

/// The mode one cart was last left in. A cart the file does not name reads `Actual`, and so
/// does a cart whose line nobody can parse — exactly as `core_for` answers for a cart with no
/// line.
pub fn video_mode_for(root: &Path, stem: &str) -> VideoMode {
    ini::value(root, VIDEO_MODE_FILE, stem)
        .as_deref()
        .and_then(VideoMode::parse)
        .unwrap_or_default()
}

/// Set one cart's mode, leaving the rest of the file exactly as it was.
pub fn write_video_mode(root: &Path, stem: &str, mode: VideoMode) -> std::io::Result<()> {
    ini::write(root, VIDEO_MODE_FILE, stem, mode.as_str())
}

/// How a platform's picture is placed. `mode` only moves a Game Boy's: the GBA follows the
/// card's `picture` setting and a SNES is always the 4:3 its games were drawn for.
pub fn fit_for(platform: Platform, mode: VideoMode, snes: SnesPicture) -> Fit {
    match platform {
        Platform::Gba => Fit::Gba,
        Platform::Gb | Platform::Gbc => match mode {
            VideoMode::Actual => Fit::Whole,
            VideoMode::Stretch => Fit::Fill,
        },
        // Full width, rows at a whole multiple: 224 at 2x and 448 hi-res at 1x are both 448,
        // every source row the same height, so no one-pixel outline is thinned into its
        // neighbour, as at 224 to 480. A 4:3 television's shape to within 7%.
        // `snes-picture 4:3` is the whole panel instead, rows at 2 or 3 panel rows each.
        Platform::Snes => match snes {
            SnesPicture::Sharp => Fit::Rows,
            SnesPicture::FourThree => Fit::Aspect(4.0 / 3.0),
        },
    }
}
