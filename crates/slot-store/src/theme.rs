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
    /// `grid-depth 40`: how dark the middle of a grid line is, in percent, from 5 to 100. `None`
    /// is the grid's own default.
    pub grid_depth: Option<f32>,
    /// `runahead 1`: frames to run ahead of the game, 0 to 2, to take its own lag off a press.
    /// `None` is the default, one.
    pub runahead: Option<u8>,
    /// `snes-picture sharp` or `snes-picture 4:3`. See `SnesPicture`.
    pub snes_picture: SnesPicture,
    /// `boot-picture last` or `boot-picture off`. See `BootPicture`.
    pub boot_picture: BootPicture,
    /// `colour-depth off`, `rich`, `deep` or `custom`. See `ColourDepth`.
    pub colour_depth: ColourDepth,
    /// `colour-gamma 1.14`, `colour-saturation 1.1` and `colour-contrast 1.05`: what
    /// `colour-depth custom` draws with. Each is 1.0 when the card does not say.
    pub colour_custom: [Option<f32>; 3],
    /// `snes-core snes9x2005` or `snes-core snes9x`: which emulator runs a SNES cart that
    /// `selected_core.ini` does not name. snes9x2005 is the default, being fast enough for
    /// run-ahead.
    pub snes_core: crate::Core,
}

/// How much depth the game picture is given, after the image-adjustment settings RG35XXSP
/// players share for RetroArch. Each is a gamma exponent, a saturation and a contrast; see
/// `ColourDepth::values`.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ColourDepth {
    /// The picture as the core drew it.
    #[default]
    Off,
    /// Middle tones a little deeper (Monitor Gamma 2.0 against a target of 2.2) and colour a
    /// touch fuller.
    Rich,
    /// Target Gamma 2.5, Saturation 1.10, Contrast 1.05.
    Deep,
    /// The card's own `colour-gamma`, `colour-saturation` and `colour-contrast`.
    Custom,
}

impl ColourDepth {
    /// Gamma exponent, saturation and contrast. `custom` is what the card's lines say, each 1.0
    /// where it says nothing.
    pub fn values(self, custom: [Option<f32>; 3]) -> [f32; 3] {
        match self {
            ColourDepth::Off => [1.0, 1.0, 1.0],
            ColourDepth::Rich => [2.2 / 2.0, 1.05, 1.0],
            ColourDepth::Deep => [2.5 / 2.2, 1.10, 1.05],
            ColourDepth::Custom => custom.map(|v| v.unwrap_or(1.0)),
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            ColourDepth::Off => "off",
            ColourDepth::Rich => "rich",
            ColourDepth::Deep => "deep",
            ColourDepth::Custom => "custom",
        }
    }
}

/// What the bootloader shows while the device starts.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum BootPicture {
    /// The screen as it was when the device last powered off: the game at that moment, or the
    /// shelf.
    #[default]
    Last,
    /// BaseOS's own logo, put back from the copy slot kept before it first replaced it.
    Off,
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

/// How a SNES picture is placed.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum SnesPicture {
    /// The panel's width, rows at a whole multiple: 640x448, every row the same height.
    #[default]
    Sharp,
    /// The whole panel at 4:3, rows stretched to 2 or 3 panel rows each.
    FourThree,
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
            grid_depth: None,
            runahead: None,
            snes_picture: SnesPicture::Sharp,
            boot_picture: BootPicture::Last,
            colour_depth: ColourDepth::Off,
            colour_custom: [None; 3],
            snes_core: crate::Core::Snes9x2005,
        }
    }
}

impl Theme {
    /// Best effort. A missing file is the default theme, not an error.
    pub fn read(root: &Path) -> Self {
        match std::fs::read_to_string(root.join(crate::CONFIG_DIR).join(THEME_FILE)) {
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
                ("snes-picture", "sharp") => theme.snes_picture = SnesPicture::Sharp,
                ("snes-picture", "4:3") => theme.snes_picture = SnesPicture::FourThree,
                ("boot-picture", "last") => theme.boot_picture = BootPicture::Last,
                ("boot-picture", "off") => theme.boot_picture = BootPicture::Off,
                ("colour-depth", "off") => theme.colour_depth = ColourDepth::Off,
                ("colour-depth", "rich") => theme.colour_depth = ColourDepth::Rich,
                ("colour-depth", "deep") => theme.colour_depth = ColourDepth::Deep,
                ("colour-depth", "custom") => theme.colour_depth = ColourDepth::Custom,
                ("snes-core", "snes9x") => theme.snes_core = crate::Core::Snes9x,
                ("snes-core", "snes9x2005" | "snes9x2005_plus") => {
                    theme.snes_core = crate::Core::Snes9x2005
                }
                ("colour-gamma" | "colour-saturation" | "colour-contrast", v) => {
                    if let Some(x) = v.parse::<f32>().ok().filter(|x| (0.5..=2.0).contains(x)) {
                        let at = match name.as_str() {
                            "colour-gamma" => 0,
                            "colour-saturation" => 1,
                            _ => 2,
                        };
                        theme.colour_custom[at] = Some(x);
                    }
                }
                ("runahead", "off") => theme.runahead = Some(0),
                ("runahead", v) => {
                    if let Some(n) = v.parse::<u8>().ok().filter(|n| *n <= 2) {
                        theme.runahead = Some(n);
                    }
                }
                ("grid-depth", v) => {
                    if let Some(d) = v.parse::<f32>().ok().filter(|d| (5.0..=100.0).contains(d)) {
                        theme.grid_depth = Some(d);
                    }
                }
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

/// Sets one line of `System/theme.txt`, as the settings menu does, and leaves every other line
/// as it was: colours, comments and anything misspelt all survive. The first line setting
/// `name` is rewritten in place and any later ones are dropped, since the last of them was the
/// one being read; a file with none gets the line added at the end, and a card with no file
/// gets one with just that line. Written whole and renamed into place, so a card pulled
/// mid-write keeps the old file rather than half of the new one.
pub fn write_theme_setting(root: &Path, name: &str, value: &str) -> std::io::Result<()> {
    let dir = root.join(crate::CONFIG_DIR);
    let path = dir.join(THEME_FILE);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let line = format!("{name} {value}");
    let mut out: Vec<String> = Vec::new();
    let mut written = false;
    for l in old.lines() {
        let t = l.trim();
        let key = t.split_whitespace().next().unwrap_or("");
        if !t.starts_with('#') && key.eq_ignore_ascii_case(name) {
            if !written {
                out.push(line.clone());
                written = true;
            }
            continue;
        }
        out.push(l.to_string());
    }
    if !written {
        out.push(line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    std::fs::create_dir_all(&dir)?;
    crate::atomic::atomic_write(&path, text.as_bytes())
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
