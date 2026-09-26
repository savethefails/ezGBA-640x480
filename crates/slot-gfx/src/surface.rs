use std::ffi::c_void;
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

/// The RG35XXSP's panel, and the size everything is composed at. The UI is laid out for it
/// directly, so on the device the blit is 1:1.
pub const OUT_W: u32 = 640;
pub const OUT_H: u32 = 480;

/// The shape the game picture is drawn at, chosen by `picture` in `System/theme.txt`.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Picture {
    /// The whole 640x480 panel. The GBA's 3:2 is squeezed to 4:3, about 11% narrower than
    /// it should be, and in exchange there are no bars and every source row is exactly three
    /// panel rows: only the columns are blended.
    #[default]
    FourThree,
    /// The GBA's own shape at the full panel width: 240x160 at 2.67x is 640x426.7, rounded
    /// to whole rows so its edges land on pixel boundaries, leaving a 26 px bar above and 27 px
    /// below.
    ThreeTwo,
}

/// Read by every draw, set once at boot from the card. An atomic rather than a `OnceLock`
/// because the tests draw both shapes in one process.
static PICTURE: AtomicU8 = AtomicU8::new(0);

pub fn set_picture(picture: Picture) {
    PICTURE.store(picture as u8, Ordering::Relaxed);
}

pub fn picture() -> Picture {
    match PICTURE.load(Ordering::Relaxed) {
        1 => Picture::ThreeTwo,
        _ => Picture::FourThree,
    }
}

/// Where the game picture sits in the frame, as x, y, width and height in panel pixels.
/// Neither shape is a whole multiple of the source across, so the game pass scales it with
/// sharp-shimmerless rather than nearest (see `GAME_FRAG`).
pub fn game_rect() -> (u32, u32, u32, u32) {
    match picture() {
        Picture::FourThree => (0, 0, OUT_W, OUT_H),
        Picture::ThreeTwo => {
            let h = 427;
            (0, (OUT_H - h) / 2, OUT_W, h)
        }
    }
}

#[derive(Debug)]
pub enum GfxError {
    Context(String),
    Shader(String),
    Framebuffer(u32),
}

impl fmt::Display for GfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GfxError::Context(m) => write!(f, "gl context: {m}"),
            GfxError::Shader(m) => write!(f, "shader: {m}"),
            GfxError::Framebuffer(s) => write!(f, "framebuffer incomplete: 0x{s:x}"),
        }
    }
}

impl std::error::Error for GfxError {}

pub trait Surface {
    fn make_current(&mut self) -> Result<(), GfxError>;
    fn window_size(&self) -> (u32, u32);
    fn swap(&mut self) -> Result<(), GfxError>;
    fn proc_address(&self, name: &str) -> *const c_void;
}

/// Largest whole multiple of the 640x480 output that fits in the window, floored at 1.
/// A fractional blit would resample the whole UI and soften it, so a desktop window is shown
/// at whole multiples only.
pub fn fit_scale(win_w: u32, win_h: u32) -> u32 {
    (win_w / OUT_W).min(win_h / OUT_H).max(1)
}

/// Origin and size of the blit rect inside a window, centred with black bars.
pub fn fit_rect(win_w: u32, win_h: u32) -> (i32, i32, i32, i32) {
    let s = fit_scale(win_w, win_h) as i32;
    let (w, h) = (OUT_W as i32 * s, OUT_H as i32 * s);
    ((win_w as i32 - w) / 2, (win_h as i32 - h) / 2, w, h)
}

/// Whether a target can hold the 640x480 composite at a whole multiple. When it cannot the blit is a fractional downscale, which has to be filtered
/// rather than nearest (nearest drops every ninth row and column) and cannot carry the LCD3x
/// grille (its 3 px triads beat against the 8/9 resample into bands).
pub fn blit_is_whole(window: (u32, u32)) -> bool {
    window.0 >= OUT_W && window.1 >= OUT_H
}

/// The composite scaled to fit, aspect preserved, for a panel too small to hold it whole.
/// The scale is fractional, so the picture is filtered and slightly soft. Only a panel smaller
/// than the RG35XXSP's takes this path.
pub fn blit_rect_fit(panel: (u32, u32), shake: f32) -> (i32, i32, i32, i32) {
    let scale = (panel.0 as f32 / OUT_W as f32).min(panel.1 as f32 / OUT_H as f32);
    let w = (OUT_W as f32 * scale).round() as i32;
    let h = (OUT_H as f32 * scale).round() as i32;
    let dx = (shake * scale).round() as i32;
    (
        (panel.0 as i32 - w) / 2 + dx,
        (panel.1 as i32 - h) / 2,
        w,
        h,
    )
}

/// The rect the composite is presented in, displaced by a refusal. `shake` is in offscreen
/// pixels and is multiplied by the blit scale, so the flinch is the same fraction of the
/// picture at any size. Horizontal only: side to side is the gesture that means no, and
/// adding the other axis would make it a rumble.
pub fn blit_rect(window: (u32, u32), shake: f32) -> (i32, i32, i32, i32) {
    // Below native there is no whole multiple left to crop to, so a target that cannot hold
    // the composite shows all of it softly rather than part of it sharply.
    if !blit_is_whole(window) {
        return blit_rect_fit(window, shake);
    }
    let (x, y, w, h) = fit_rect(window.0, window.1);
    let dx = shake * fit_scale(window.0, window.1) as f32;
    (x + dx.round() as i32, y, w, h)
}
