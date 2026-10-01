use slot::thumb;
use slot_retro::{GBA_H, GBA_W};

fn decode(png_bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut reader = png::Decoder::new(png_bytes).read_info().expect("read info");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("decode");
    assert_eq!(info.color_type, png::ColorType::Rgb);
    buf.truncate(info.buffer_size());
    (buf, info.width, info.height)
}

/// libretro hands over XRGB8888 little endian, so the bytes arrive B, G, R, X. A polaroid
/// with two channels swapped looks plausible until it sits next to the game it came from.
#[test]
fn a_thumbnail_keeps_the_frames_colours() {
    let mut frame = vec![0u8; (GBA_W * GBA_H * 4) as usize];
    for px in frame.chunks_exact_mut(4) {
        px.copy_from_slice(&[0x20, 0x40, 0xd0, 0xff]);
    }
    let encoded = thumb::png(&frame, (GBA_W, GBA_H)).expect("encode");
    let (rgb, w, h) = decode(&encoded);
    assert_eq!((w, h), (GBA_W, GBA_H));
    assert_eq!(&rgb[..3], &[0xd0, 0x40, 0x20]);
}

/// A save taken before the core has produced a frame has no picture to encode, and a
/// truncated read of one is worse than no polaroid at all.
#[test]
fn a_short_frame_is_not_a_thumbnail() {
    assert!(thumb::png(&[], (GBA_W, GBA_H)).is_none());
    assert!(thumb::png(&[], (0, 0)).is_none());
    assert!(thumb::png(&vec![0u8; (GBA_W * GBA_H * 4) as usize - 4], (GBA_W, GBA_H)).is_none());
}

/// Every console's picture is photographed at its own size: a Game Boy's 160x144, a SNES's
/// 256x224, with nothing of a GBA-sized buffer around it.
#[test]
fn a_thumbnail_is_the_size_the_core_drew() {
    for (w, h) in [(160u32, 144u32), (256, 224), (512, 448)] {
        let frame = vec![0x80u8; (w * h * 4) as usize];
        let encoded = thumb::png(&frame, (w, h)).expect("encode");
        let (_, got_w, got_h) = decode(&encoded);
        assert_eq!((got_w, got_h), (w, h));
    }
}
