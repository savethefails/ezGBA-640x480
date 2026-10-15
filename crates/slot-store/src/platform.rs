use std::path::Path;

/// Which console a cart is for, and therefore which folder every one of its files lives in —
/// and, since there is one shelf per platform, which shelf of the carousel it stands on.
///
/// One variant per card directory. There is deliberately no variant meaning "loose at
/// the root": nothing stays loose, and a file's platform is a property of *where it is*, which
/// is what lets the scan answer it without opening the file at all.
///
/// There is no separate grouping type. A Game Boy and a Game Boy Color cartridge are the same
/// object dimensionally, and they were grouped onto one shelf for exactly that reason; the
/// shelves are one per platform now, which leaves nothing for a second type to say. The name
/// collision with `slot_ui::Shelf` — the carousel widget — is unchanged: `slot::app` holds one
/// of those per `Platform`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Platform {
    #[default]
    Gba,
    Gb,
    Gbc,
    Snes,
    /// PICO-8, the fantasy console: carts run on the official PICO-8, by way of
    /// `pico8_libretro`, which the user supplies the PICO-8 half of.
    Pico8,
}

impl Platform {
    /// Every variant, once, in the order the shelves are switched through.
    pub const ALL: [Platform; 5] = [
        Platform::Gba,
        Platform::Gb,
        Platform::Gbc,
        Platform::Snes,
        Platform::Pico8,
    ];

    /// The card directory this platform's files live under, in `Games/`, `Saves/`, `States/`
    /// and `Labels/` alike. Every platform has one — see the type's own comment.
    pub fn dir_name(self) -> &'static str {
        match self {
            Platform::Gba => "GBA",
            Platform::Gb => "GB",
            Platform::Gbc => "GBC",
            Platform::Snes => "SNES",
            Platform::Pico8 => "PICO8",
        }
    }

    /// The ROM extensions this folder holds. A `.gba` sitting in `GB/` is not a Game Boy cart
    /// and is not scanned as one: the folder says where a cart's files go, but it cannot make
    /// a GBA ROM into a Game Boy game.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Platform::Gba => &["gba"],
            Platform::Gb | Platform::Gbc => &["gb", "gbc"],
            Platform::Snes => &["sfc", "smc"],
            // A `.png` only as a `.p8.png`: see `accepts`.
            Platform::Pico8 => &["p8", "png"],
        }
    }

    /// The picture this console draws, in pixels, before a core has drawn one: a SNES can
    /// also go to 512 across and 448 down, and the frame says which.
    ///
    /// A Game Boy Color draws the same 160x144 as a Game Boy — the colour is in the pixels,
    /// not in how many of them there are.
    ///
    /// Spelled out here rather than taken from `slot_retro`: this crate is the card's own view
    /// of what a platform is, and it does not know a libretro core exists.
    pub fn picture(self) -> (u32, u32) {
        match self {
            Platform::Gba => (240, 160),
            Platform::Gb | Platform::Gbc => (160, 144),
            Platform::Snes => (256, 224),
            Platform::Pico8 => (128, 128),
        }
    }

    pub fn accepts(self, path: &Path) -> bool {
        // A PICO-8 cart saved as a picture is `name.p8.png`. Any other picture in the folder
        // (a screenshot, the box art someone dropped in the wrong place) is not a cart.
        if self == Platform::Pico8
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        {
            return path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.to_ascii_lowercase().ends_with(".p8"));
        }
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                self.extensions()
                    .iter()
                    .any(|k| ext.eq_ignore_ascii_case(k))
            })
    }
}
