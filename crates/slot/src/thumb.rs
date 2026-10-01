/// The polaroid picture: one frame, PNG, at the size the core drew it. There is nothing to
/// downscale because the frame is already no bigger than the switcher shows, and the drawn
/// still is placed from its own size, as the live game is.
///
/// `xrgb8888` is libretro's frame buffer, little endian, so its bytes arrive B, G, R, X.
pub fn png(xrgb8888: &[u8], (w, h): (u32, u32)) -> Option<Vec<u8>> {
    let n = (w * h) as usize;
    if n == 0 || xrgb8888.len() < n * 4 {
        return None;
    }
    let mut rgb = Vec::with_capacity(n * 3);
    for px in xrgb8888[..n * 4].chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }

    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().ok()?;
    writer.write_image_data(&rgb).ok()?;
    writer.finish().ok()?;
    Some(out)
}
