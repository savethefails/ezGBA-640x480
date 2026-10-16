//! `cart_shell.ini`: the shell a player picked for a cart, by its rom stem. One line a cart,
//! `<stem> = <outline> <rrggbb> <finish>`, for example `Pokemon Emerald = auto 249c60 clear`.
//!
//! Two files, read in layers. `Config/cart_shell.ini` is the player's own; a
//! `Labels/cart_shell.ini` sits beside label art and travels with it, so a label pack can
//! carry the shells its scans were made for. A line in the labels file wins, and a labels line
//! whose value is just `auto` takes a cart back to the built-in table.

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

pub const CART_SHELL_FILE: &str = "Config/cart_shell.ini";
pub const LABELS_SHELL_FILE: &str = "Labels/cart_shell.ini";

/// Which mould: `auto` asks the rom. A Game Boy pak is `notched` (classes A and B) or `rounded`
/// (class C). Carts of the other consoles have one mould each and read every word as `auto`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Outline {
    Auto,
    Notched,
    Rounded,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ShellFinish {
    Solid,
    Clear,
    Glitter,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ShellChoice {
    pub outline: Outline,
    pub colour: [u8; 3],
    pub finish: ShellFinish,
}

impl ShellChoice {
    /// `None` for anything but exactly three words, each one it knows. A typo costs that cart
    /// its choice and nothing else.
    pub fn parse(value: &str) -> Option<ShellChoice> {
        let mut words = value.split_whitespace();
        let outline = match words.next()? {
            "auto" => Outline::Auto,
            "notched" => Outline::Notched,
            "rounded" => Outline::Rounded,
            _ => return None,
        };
        let hex = words.next()?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let colour = [0, 2, 4].map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0));
        let finish = match words.next()? {
            "solid" => ShellFinish::Solid,
            "clear" => ShellFinish::Clear,
            "glitter" => ShellFinish::Glitter,
            _ => return None,
        };
        words.next().is_none().then_some(ShellChoice {
            outline,
            colour,
            finish,
        })
    }

    pub fn to_value(&self) -> String {
        let outline = match self.outline {
            Outline::Auto => "auto",
            Outline::Notched => "notched",
            Outline::Rounded => "rounded",
        };
        let finish = match self.finish {
            ShellFinish::Solid => "solid",
            ShellFinish::Clear => "clear",
            ShellFinish::Glitter => "glitter",
        };
        let [r, g, b] = self.colour;
        format!("{outline} {r:02x}{g:02x}{b:02x} {finish}")
    }
}

/// A stem as the lookup holds it. Normalised, because an accent can be one code point or two
/// depending on what wrote it: a Mac writes filenames decomposed and Windows Notepad writes the
/// ini composed, so `Pokémon` on the card and `Pokémon` in the file are different strings
/// until both are put into the same form.
pub fn key(stem: &str) -> String {
    stem.nfc().collect()
}

pub fn choices(text: &str) -> HashMap<String, ShellChoice> {
    crate::ini::parse(text)
        .into_iter()
        .filter_map(|(stem, value)| Some((key(&stem), ShellChoice::parse(&value)?)))
        .collect()
}

/// The player's file, then the labels file over it. See the module's comment.
pub fn layered(config: &str, labels: &str) -> HashMap<String, ShellChoice> {
    let mut out = choices(config);
    for (stem, value) in crate::ini::parse(labels) {
        if value.trim() == "auto" {
            out.remove(&key(&stem));
        } else if let Some(choice) = ShellChoice::parse(&value) {
            out.insert(key(&stem), choice);
        }
    }
    out
}
