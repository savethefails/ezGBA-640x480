//! Whether a core's sound survives run-ahead: the same frames played straight through, and played
//! the way `emu::run_ahead` plays them (a frame, save, a frame thrown away, load), should be the
//! same samples. A core whose load disturbs its sound output differs, and the player hears that
//! sixty times a second as crackle. It exists to check a core or a patch to one. It never ships.
//!
//! ```text
//! cargo run --release -p slot-retro --example runahead_audio -- CORE ROM [--frames N] [--option KEY=VALUE]...
//! ```

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use slot_retro::{ButtonMask, LibretroCore, RetroCore};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("runahead_audio: {e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let core_path = PathBuf::from(args.next().ok_or("usage: runahead_audio CORE ROM")?);
    let rom = PathBuf::from(args.next().ok_or("usage: runahead_audio CORE ROM")?);
    let mut frames = 600;
    let mut options = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => frames = args.next().ok_or("--frames takes N")?.parse()?,
            "--option" => {
                let kv = args.next().ok_or("--option takes KEY=VALUE")?;
                let (k, v) = kv.split_once('=').ok_or("--option takes KEY=VALUE")?;
                options.push((k.to_string(), v.to_string()));
            }
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }

    let tmp = std::env::temp_dir();
    let mut core = LibretroCore::open_with(&core_path, &tmp, &tmp)?;
    for (k, v) in &options {
        core.set_option(k, v);
    }
    core.load(&rom)?;
    let idle = ButtonMask(0);
    // Into the music, so there is sound to compare.
    for _ in 0..300 {
        core.run_frame(idle);
    }
    core.take_audio();
    let start = core.serialize()?;

    core.unserialize(&start)?;
    let mut straight = Vec::new();
    for _ in 0..frames {
        core.run_frame(idle);
        straight.extend(core.take_audio());
    }

    core.unserialize(&start)?;
    let mut ahead = Vec::new();
    for _ in 0..frames {
        core.set_frame_skip(true);
        core.run_frame(idle);
        ahead.extend(core.take_audio());
        let state = core.serialize()?;
        core.set_frame_skip(false);
        core.run_frame(idle);
        core.take_audio();
        core.unserialize(&state)?;
    }

    let loudest = straight.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    let differ = straight.iter().zip(&ahead).filter(|(a, b)| a != b).count();
    let first = straight.iter().zip(&ahead).position(|(a, b)| a != b);
    let worst = straight
        .iter()
        .zip(&ahead)
        .map(|(a, b)| (i32::from(*a) - i32::from(*b)).unsigned_abs())
        .max()
        .unwrap_or(0);
    println!(
        "straight {} samples, run-ahead {} samples, loudest {loudest}",
        straight.len(),
        ahead.len()
    );
    println!("{differ} samples differ, first at {first:?}, worst by {worst}");
    Ok(straight.len() == ahead.len() && differ == 0 && loudest > 0)
}
