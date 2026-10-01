use std::path::Path;

use slot::boot_picture::{
    apply, bmp_pixels, find_partition, on_baseos, paint, pixel_offset, Painted, Want, BACKUP,
};

const W: usize = 640;
const H: usize = 480;
const PIXELS: usize = W * H * 3;

/// A logo laid out the way BaseOS's `make-bootlogo.sh` writes one (ImageMagick's BMP3):
/// a 54-byte header, 24 bits, bottom up, every pixel `fill`.
fn logo(w: i32, h: i32, bpp: u16, compression: u32, fill: u8) -> Vec<u8> {
    let pixels = (w.unsigned_abs() as usize) * (h.unsigned_abs() as usize) * (bpp as usize / 8);
    let mut b = Vec::new();
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&((54 + pixels) as u32).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&bpp.to_le_bytes());
    b.extend_from_slice(&compression.to_le_bytes());
    b.extend_from_slice(&(pixels as u32).to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&[0; 8]);
    b.extend(std::iter::repeat_n(fill, pixels));
    b
}

fn baseos_logo(fill: u8) -> Vec<u8> {
    logo(W as i32, H as i32, 24, 0, fill)
}

/// RGBA top row first, as `Compositor::read_frame` gives it.
fn frame(rgba: [u8; 4]) -> Vec<u8> {
    rgba.iter().copied().cycle().take(W * H * 4).collect()
}

#[test]
fn baseos_s_own_logo_is_a_picture_slot_will_write() {
    let b = baseos_logo(0);
    assert_eq!(pixel_offset(&b, b.len() as u64), Some(54));
}

#[test]
fn anything_but_the_exact_format_is_refused() {
    let bad = [
        ("another panel's size", logo(720, 480, 24, 0, 0)),
        ("top down", logo(W as i32, -(H as i32), 24, 0, 0)),
        ("32 bits", logo(W as i32, H as i32, 32, 0, 0)),
        ("compressed", logo(W as i32, H as i32, 24, 1, 0)),
    ];
    for (why, b) in bad {
        assert_eq!(pixel_offset(&b, b.len() as u64), None, "{why}");
    }
    let mut short = baseos_logo(0);
    short.pop();
    assert_eq!(
        pixel_offset(&short, short.len() as u64),
        None,
        "a byte short"
    );
    let mut long = baseos_logo(0);
    long.push(0);
    assert_eq!(pixel_offset(&long, long.len() as u64), None, "a byte long");
    let mut png = baseos_logo(0);
    png[0..2].copy_from_slice(b"\x89P");
    assert_eq!(pixel_offset(&png, png.len() as u64), None, "not a BMP");
}

/// The frame's top-left pixel is the last row's first in a bottom-up BMP, stored blue first.
#[test]
fn a_frame_becomes_bottom_up_bgr() {
    let mut rgba = frame([0, 0, 0, 255]);
    rgba[0..4].copy_from_slice(&[10, 20, 30, 255]);
    let px = bmp_pixels(&rgba).unwrap();
    assert_eq!(px.len(), PIXELS);
    let last_row = (H - 1) * W * 3;
    assert_eq!(&px[last_row..last_row + 3], &[30, 20, 10]);
    assert_eq!(&px[0..3], &[0, 0, 0]);
    assert_eq!(bmp_pixels(&rgba[4..]), None, "a frame short");
}

#[test]
fn painting_replaces_only_the_pixels_and_keeps_the_header_and_length() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("bootlogo.bmp");
    let before = baseos_logo(7);
    std::fs::write(&path, &before).unwrap();
    let pixels = vec![200u8; PIXELS];
    assert_eq!(paint(&path, &pixels).unwrap(), Painted::Written);
    let after = std::fs::read(&path).unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(after[..54], before[..54], "the header changed");
    assert_eq!(&after[54..], &pixels[..]);
    assert_eq!(paint(&path, &pixels).unwrap(), Painted::Unchanged);
}

#[test]
fn a_logo_slot_does_not_recognise_is_left_untouched() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("bootlogo.bmp");
    let other = logo(720, 480, 24, 0, 7);
    std::fs::write(&path, &other).unwrap();
    let err = paint(&path, &vec![1u8; PIXELS]).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&path).unwrap(), other);
}

#[test]
fn a_missing_logo_is_never_created() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("bootlogo.bmp");
    assert!(paint(&path, &vec![1u8; PIXELS]).is_err());
    assert!(!path.exists());
}

#[test]
fn baseos_s_logo_is_kept_on_the_card_before_it_is_first_painted_over_and_can_come_back() {
    let part = tempfile::tempdir().unwrap();
    let card = tempfile::tempdir().unwrap();
    let logo_path = part.path().join("bootlogo.bmp");
    let backup = card.path().join(BACKUP);
    let original = baseos_logo(7);
    std::fs::write(&logo_path, &original).unwrap();

    apply(&logo_path, &backup, &Want::Scene(frame([255, 0, 0, 255]))).unwrap();
    assert_eq!(
        std::fs::read(&backup).unwrap(),
        original,
        "the copy is not BaseOS's logo"
    );
    assert_ne!(std::fs::read(&logo_path).unwrap(), original);

    // A second power off paints again but never copies a painted logo over the original.
    apply(&logo_path, &backup, &Want::Scene(frame([0, 255, 0, 255]))).unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let painted = std::fs::read(&logo_path).unwrap();
    assert_eq!(&painted[54..57], &[0, 255, 0]);

    apply(&logo_path, &backup, &Want::Original).unwrap();
    assert_eq!(std::fs::read(&logo_path).unwrap(), original);
}

#[test]
fn nothing_is_copied_or_written_when_the_logo_is_not_one_slot_writes() {
    let part = tempfile::tempdir().unwrap();
    let card = tempfile::tempdir().unwrap();
    let logo_path = part.path().join("bootlogo.bmp");
    let backup = card.path().join(BACKUP);
    let other = logo(720, 480, 24, 0, 7);
    std::fs::write(&logo_path, &other).unwrap();
    assert!(apply(&logo_path, &backup, &Want::Scene(frame([255, 0, 0, 255]))).is_err());
    assert!(!backup.exists());
    assert_eq!(std::fs::read(&logo_path).unwrap(), other);
}

#[test]
fn putting_the_logo_back_needs_the_copy() {
    let part = tempfile::tempdir().unwrap();
    let card = tempfile::tempdir().unwrap();
    let logo_path = part.path().join("bootlogo.bmp");
    std::fs::write(&logo_path, baseos_logo(9)).unwrap();
    assert!(apply(&logo_path, &card.path().join(BACKUP), &Want::Original).is_err());
    assert_eq!(std::fs::read(&logo_path).unwrap(), baseos_logo(9));
}

fn sys(cmdline: &str, parts: &[(&str, &str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("proc")).unwrap();
    std::fs::write(d.path().join("proc/cmdline"), cmdline).unwrap();
    for (disk, dev, name) in parts {
        let p = d.path().join("sys/class/block").join(disk).join(dev);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            p.join("uevent"),
            format!("MAJOR=179\nDEVNAME={dev}\nDEVTYPE=partition\nPARTNAME={name}\n"),
        )
        .unwrap();
    }
    d
}

#[test]
fn the_partition_is_found_by_name_on_the_disk_the_system_booted_from() {
    let d = sys(
        "console=ttyS0 root=/dev/mmcblk0p5 rootwait",
        &[
            ("mmcblk0", "mmcblk0p1", "special"),
            ("mmcblk0", "mmcblk0p2", "boot-resource"),
            ("mmcblk0", "mmcblk0p5", "rootfs"),
            // The second card: never this one, even with a partition of the same name.
            ("mmcblk1", "mmcblk1p2", "boot-resource"),
        ],
    );
    assert_eq!(find_partition(d.path()).as_deref(), Some("mmcblk0p2"));
}

#[test]
fn two_candidates_or_none_and_nothing_is_chosen() {
    let two = sys(
        "root=/dev/mmcblk0p5",
        &[
            ("mmcblk0", "mmcblk0p2", "boot-resource"),
            ("mmcblk0", "mmcblk0p3", "boot-resource"),
        ],
    );
    assert_eq!(find_partition(two.path()), None);
    let none = sys("root=/dev/mmcblk0p5", &[("mmcblk0", "mmcblk0p5", "rootfs")]);
    assert_eq!(find_partition(none.path()), None);
    let no_root = sys(
        "console=ttyS0",
        &[("mmcblk0", "mmcblk0p2", "boot-resource")],
    );
    assert_eq!(find_partition(no_root.path()), None);
}

#[test]
fn only_baseos_with_an_unturned_panel_is_written_to() {
    let release = |text: Option<&str>| {
        let d = tempfile::tempdir().unwrap();
        if let Some(text) = text {
            std::fs::create_dir_all(d.path().join("etc")).unwrap();
            std::fs::write(d.path().join("etc/baseos-release"), text).unwrap();
        }
        d
    };
    let check = |d: &tempfile::TempDir| on_baseos(Path::new(d.path()));
    assert!(check(&release(Some(
        "BASEOS_TARGET=rg35xxsp\nBASEOS_PANEL_ROTATION_CCW=0\n"
    ))));
    assert!(check(&release(Some("BASEOS_TARGET=rg35xxsp\n"))));
    assert!(!check(&release(Some("BASEOS_PANEL_ROTATION_CCW=90\n"))));
    assert!(!check(&release(None)), "not BaseOS");
}

#[test]
fn the_card_keeps_the_same_picture_only_once_the_partition_has_it() {
    use slot::boot_picture::{read_last_screen, write_both, LAST_SCREEN};
    let card = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(card.path().join("System")).unwrap();
    let rgba = frame([12, 34, 56, 255]);

    let ok = write_both(card.path(), &Want::Scene(rgba.clone()), |_, _| {
        Ok(Painted::Written)
    });
    assert_eq!(ok, Ok(Painted::Written));
    assert_eq!(read_last_screen(card.path()), Some(rgba.clone()));

    // A partition that would not take it: the card's copy, which now disagrees, goes.
    let failed = write_both(card.path(), &Want::Scene(frame([1, 2, 3, 255])), |_, _| {
        Err("no".into())
    });
    assert!(failed.is_err());
    assert!(!card.path().join(LAST_SCREEN).exists());

    // Gone before the partition is even touched, so a power off that cuts the write short
    // can never leave the old copy beside a new picture.
    write_both(card.path(), &Want::Scene(rgba.clone()), |_, _| {
        Ok(Painted::Written)
    })
    .unwrap();
    write_both(card.path(), &Want::Scene(rgba), |card, _| {
        assert!(
            !card.join(LAST_SCREEN).exists(),
            "the old copy was still there"
        );
        Ok(Painted::Written)
    })
    .unwrap();

    // Putting BaseOS's logo back leaves no copy: the next boot is an ordinary one.
    write_both(card.path(), &Want::Original, |_, _| Ok(Painted::Written)).unwrap();
    assert!(!card.path().join(LAST_SCREEN).exists());
}
