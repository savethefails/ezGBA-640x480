//! Writes every shell the table knows, on every mould, to one PNG for looking at:
//! `cargo run -p slot-ui --example cart_gallery -- out.png`.

use std::path::PathBuf;

use slot_store::{Cart, Outline, Platform, ShellChoice, ShellFinish};
use slot_ui::cart_face;

fn cart(platform: Platform, stem: &str, shell: Option<ShellChoice>) -> Cart {
    Cart {
        platform,
        stem: stem.into(),
        rom: PathBuf::from("/nonexistent"),
        label: None,
        box_art: None,
        title: String::new(),
        code: String::new(),
        shell,
    }
}

fn choice(outline: Outline, colour: [u8; 3], finish: ShellFinish) -> Option<ShellChoice> {
    Some(ShellChoice {
        outline,
        colour,
        finish,
    })
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "carts.png".into());
    let gba = |code: &str, stem: &str| {
        let mut c = cart(Platform::Gba, stem, None);
        c.code = code.into();
        c
    };
    let rows: Vec<Vec<Cart>> = vec![
        vec![
            gba("AMTE", "Metroid Fusion"),
            gba("BPEE", "Pokemon Emerald"),
            gba("BPRE", "Pokemon FireRed"),
            gba("U3IE", "Boktai"),
            gba("RZWE", "WarioWare Twisted"),
        ],
        vec![
            cart(Platform::Gb, "Tetris", None),
            cart(
                Platform::Gbc,
                "Wario Land 3",
                choice(Outline::Rounded, [0x7c, 0x7a, 0x8a], ShellFinish::Clear),
            ),
            cart(
                Platform::Gbc,
                "Pokemon Crystal",
                choice(Outline::Rounded, [0x86, 0xb9, 0xbf], ShellFinish::Glitter),
            ),
            cart(
                Platform::Gb,
                "Pokemon Gold",
                choice(Outline::Notched, [0xb3, 0x8b, 0x3a], ShellFinish::Solid),
            ),
            cart(
                Platform::Gbc,
                "Kirby Tilt n Tumble",
                choice(Outline::Rounded, [0xec, 0x94, 0xb4], ShellFinish::Clear),
            ),
        ],
        vec![
            cart(Platform::Snes, "Super Mario World", None),
            cart(
                Platform::Snes,
                "Killer Instinct",
                choice(Outline::Auto, [0x2c, 0x2b, 0x2e], ShellFinish::Solid),
            ),
            cart(
                Platform::Snes,
                "Clear test",
                choice(Outline::Auto, [0xd9, 0xdb, 0xd8], ShellFinish::Clear),
            ),
        ],
        // A label scan, when one is given as the second argument.
        std::env::args()
            .nth(2)
            .map(|label| {
                let mut c = cart(Platform::Snes, "Super Mario Kart", None);
                c.label = Some(PathBuf::from(label));
                vec![c]
            })
            .unwrap_or_default(),
    ];
    let gap = 12u32;
    let faces: Vec<Vec<_>> = rows
        .iter()
        .map(|r| r.iter().map(cart_face).collect())
        .collect();
    let w = faces
        .iter()
        .map(|r| r.iter().map(|f| f.w + gap).sum::<u32>() + gap)
        .max()
        .unwrap_or(0);
    let h = faces
        .iter()
        .map(|r| r.iter().map(|f| f.h).max().unwrap_or(0) + gap)
        .sum::<u32>()
        + gap;
    let mut px = vec![0u8; (w * h * 4) as usize];
    for p in px.chunks_exact_mut(4) {
        p.copy_from_slice(&[0x16, 0x16, 0x1c, 0xff]);
    }
    let mut y0 = gap;
    for row in &faces {
        let mut x0 = gap;
        for f in row {
            for y in 0..f.h {
                for x in 0..f.w {
                    let s = ((y * f.w + x) * 4) as usize;
                    let d = (((y0 + y) * w + x0 + x) * 4) as usize;
                    let a = f.rgba[s + 3] as u32;
                    for c in 0..3 {
                        px[d + c] =
                            ((f.rgba[s + c] as u32 * a + px[d + c] as u32 * (255 - a)) / 255) as u8;
                    }
                }
            }
            x0 += f.w + gap;
        }
        y0 += row.iter().map(|f| f.h).max().unwrap_or(0) + gap;
    }
    let file = std::fs::File::create(&out).expect("create");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
    println!("wrote {out}");
}
