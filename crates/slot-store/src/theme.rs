use std::path::Path;

/// `System/theme.txt`: one `name #rrggbb` per line. A line whose first character is `#` is a
/// comment; there are no trailing comments, because `#` is also how a colour is written and a
/// mark that means two things is a mark that gets one of them wrong.
///
/// There is no settings screen, so this file is the whole of it. Anything unreadable,
/// misspelt or malformed leaves that colour at its default and the rest of the file still
/// applies: a card edited on a desktop must never be able to produce a device that will not
/// boot.
pub const THEME_FILE: &str = "theme.txt";

/// The case around the slot, in the order they stack from the outside in. Only the bar for
/// now; the names are what a theme file addresses, so they are part of the format.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Theme {
    /// The outer plastic.
    pub housing: [u8; 3],
    /// The floor of the bay, stepped down from the shell.
    pub recess: [u8; 3],
    /// The opening itself, and the inside of the thumb scoop.
    pub opening: [u8; 3],
    /// The lit edge of the plastic: the top of the slot and the rim of the scoop.
    pub edge: [u8; 3],
    /// The tint behind the shelf, under the wallpaper's fixed opacity. Black by default,
    /// matching what every card had before this existed.
    pub scrim: [u8; 3],
    /// `menu off` keeps MENU on the shelf from opening the settings menu.
    pub menu: bool,
    /// `picture 4:3` fills the panel; `picture 3:2` keeps the GBA's own shape with thin bars.
    pub picture: Aspect,
    /// `scaler shimmerless` scales the game with sharp-shimmerless instead of Pixel AA.
    pub scaler: Scaling,
    /// `sharpness 1.5`: how narrow Pixel AA's blend at a pixel edge is, from 0.0 (soft) through
    /// 1.0 (area weighted, the default) to 2.0 (nearly hard). Pixel AA only.
    pub sharpness: f32,
    /// `grid on` draws ezGBA's LCD grid over the game; `grid strict` does too, but never at the
    /// cost of any brightness; `grid lcd` darkens every colour alike, as a backlit LCD does;
    /// `grid off` is the default.
    pub grid: LcdGrid,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum LcdGrid {
    #[default]
    Off,
    On,
    Strict,
    Lcd,
}

/// Which pixel art scaler draws the game picture.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Scaling {
    #[default]
    PixelAa,
    Shimmerless,
}

/// The shape of the game picture on a 640x480 panel.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Aspect {
    /// The whole panel, the GBA picture a little narrower than it should be.
    #[default]
    FourThree,
    /// The GBA's own 3:2 at the full panel width, with a thin bar above and below.
    ThreeTwo,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            housing: [0x24, 0x24, 0x29],
            recess: [0x1a, 0x1a, 0x1d],
            opening: [0x05, 0x05, 0x08],
            edge: [0x4d, 0x4d, 0x57],
            scrim: [0x00, 0x00, 0x00],
            menu: true,
            picture: Aspect::FourThree,
            scaler: Scaling::PixelAa,
            sharpness: 1.0,
            grid: LcdGrid::Off,
        }
    }
}

impl Theme {
    /// Best effort. A missing file is the default theme, not an error.
    pub fn read(root: &Path) -> Self {
        match std::fs::read_to_string(root.join("System").join(THEME_FILE)) {
            Ok(text) => Self::parse(&text),
            Err(_) => Theme::default(),
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut theme = Theme::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
                continue;
            };
            // A third word means the line was meant as something else. Guessing at it is how
            // a typo silently becomes a colour nobody chose.
            if parts.next().is_some() {
                continue;
            }
            let name = name.to_ascii_lowercase();
            match (name.as_str(), value.to_ascii_lowercase().as_str()) {
                ("menu", "off") => theme.menu = false,
                ("menu", "on") => theme.menu = true,
                ("picture", "4:3") => theme.picture = Aspect::FourThree,
                ("picture", "3:2") => theme.picture = Aspect::ThreeTwo,
                ("grid", "on") => theme.grid = LcdGrid::On,
                ("grid", "strict") => theme.grid = LcdGrid::Strict,
                ("grid", "lcd") => theme.grid = LcdGrid::Lcd,
                ("grid", "off") => theme.grid = LcdGrid::Off,
                ("scaler", "pixel-aa") => theme.scaler = Scaling::PixelAa,
                ("scaler", "shimmerless") => theme.scaler = Scaling::Shimmerless,
                ("sharpness", v) => {
                    if let Some(s) = v.parse::<f32>().ok().filter(|s| (0.0..=2.0).contains(s)) {
                        theme.sharpness = s;
                    }
                }
                _ => {}
            }
            let Some(rgb) = hex(value) else {
                continue;
            };
            match name.as_str() {
                "housing" => theme.housing = rgb,
                "recess" => theme.recess = rgb,
                "opening" => theme.opening = rgb,
                "edge" => theme.edge = rgb,
                "scrim" => theme.scrim = rgb,
                _ => {}
            }
        }
        theme
    }
}

/// `rrggbb`, with or without the leading hash. Both are written in the wild and neither is
/// worth refusing a card over.
fn hex(value: &str) -> Option<[u8; 3]> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}
