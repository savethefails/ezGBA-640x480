use std::ffi::c_void;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

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

/// How the game picture is scaled up to its area. Both are pixel art scalers: every source
/// pixel stays a solid block and only the panel pixel a boundary crosses is blended.
///
/// Pixel AA at 1.0 is the default. Compared against sharp-shimmerless on Skyland and GBAlatro
/// through the real core, it draws the same boundaries with light text on dark coming out
/// cleaner, and it is the only one of the two whose light stays constant as a picture scrolls:
/// a one pixel line slid across the panel carries the same light at every position under Pixel
/// AA at 1.0, and pulses by 9.6% under sharp-shimmerless, which mixes stored values rather than
/// light. Sharper settings look crisper in a still and pulse again (6.8% at 1.5, 11.8% at 2.0).
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Scaler {
    /// zadpos's sharp-shimmerless: the blend weighted by area, mixed as stored.
    SharpShimmerless,
    /// fishku's Pixel AA at a sharpness: 1.0 is the same area weighting, higher narrows the
    /// blend. Mixed in linear light.
    PixelAa(f32),
}

impl Default for Scaler {
    fn default() -> Self {
        Scaler::PixelAa(1.0)
    }
}

static SCALER: AtomicU8 = AtomicU8::new(1);
static SHARPNESS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

pub fn set_scaler(scaler: Scaler) {
    let (which, sharp) = match scaler {
        Scaler::SharpShimmerless => (0, 1.0f32),
        Scaler::PixelAa(sharp) => (1, sharp),
    };
    SHARPNESS.store(sharp.to_bits(), Ordering::Relaxed);
    SCALER.store(which, Ordering::Relaxed);
}

pub fn scaler() -> Scaler {
    match SCALER.load(Ordering::Relaxed) {
        1 => Scaler::PixelAa(f32::from_bits(SHARPNESS.load(Ordering::Relaxed))),
        _ => Scaler::SharpShimmerless,
    }
}

/// ezGBA's LCD grid: a soft line on every game pixel edge, taking `gap` panel pixels' worth of
/// light (across, down), with each game pixel's light given back so the picture is as bright as
/// without it. `keep` is how strictly: 1.0 lightens a bright pixel's lines until its light fits
/// under white, 0.0 keeps every line full strength and lets the brightest pixels fall short. A
/// gap of 0.0 on both axes is no grid.
///
/// `even` gives nothing back: the lines take the same share of every colour's light, as a real
/// LCD's gaps do, and every colour keeps its place against every other. Given back, a colour
/// with no room below white cannot follow the ones that can, so the brightest colours all land
/// on one level and the picture flattens; even, it only dims, and the backlight wins it back.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Grid {
    pub gap: [f32; 2],
    pub keep: f32,
    pub even: bool,
}

/// The width of a grid line in panel pixels. Must match `LINE_W` in `GAME_FRAG`: the depth a
/// card asks for is turned into light taken through it.
const LINE_WIDTH: f32 = 2.5;

/// How dark the middle of a line is, in percent, when the card does not say. First chosen at 20
/// by eye against Skyland and GBAlatro in screenshots, but at the panel's own 229 ppi and arm's
/// length that all but vanished: a line 2.67 panel pixels from the next is a detail the eye
/// barely resolves, so it needs the contrast to survive. 40 is double that, and still to be judged
/// on the device. The three settings differ only in what a colour with no headroom (white, or any fully
/// saturated channel) does, since it cannot be brightened to make up for its lines.
pub const GRID_DEPTH: f32 = 40.0;

/// Light a line of `depth` percent takes, in panel pixels. A raised cosine `LINE_WIDTH` wide
/// with `gap` under it peaks at `2 * gap / LINE_WIDTH`, so full depth is half the width.
fn lines(depth: f32) -> [f32; 2] {
    let gap = depth.clamp(0.0, 100.0) / 100.0 * LINE_WIDTH / 2.0;
    [gap, gap]
}

impl Grid {
    /// `grid on`: such a colour keeps a fifth of its lines' depth and most of its light, so
    /// the grid shows, faintly, on every colour.
    pub fn on() -> Self {
        Grid {
            gap: lines(GRID_DEPTH),
            keep: 0.8,
            even: false,
        }
    }

    /// `grid strict`: such a colour keeps all of its light and none of its lines.
    pub fn strict() -> Self {
        Grid {
            gap: lines(GRID_DEPTH),
            keep: 1.0,
            even: false,
        }
    }

    /// `grid lcd`: a backlit LCD's gaps. Every colour has lines of the same depth and gives up
    /// the same share of its light to them, about a third at the default depth, and nothing is
    /// brightened to make up for it: white stays the brightest thing on screen, every colour
    /// stays where it was against every other, and the whole picture is dimmer, for the
    /// backlight to win back.
    pub fn lcd() -> Self {
        Grid {
            gap: lines(GRID_DEPTH),
            keep: 0.0,
            even: true,
        }
    }

    /// The same grid with lines `depth` percent dark at their middle, from `grid-depth` on the
    /// card. No grid stays no grid.
    pub fn with_depth(self, depth: f32) -> Self {
        if self.gap == [0.0, 0.0] {
            return self;
        }
        Grid {
            gap: lines(depth),
            ..self
        }
    }
}

static GRID_X: AtomicU32 = AtomicU32::new(0);
static GRID_Y: AtomicU32 = AtomicU32::new(0);
static GRID_KEEP: AtomicU32 = AtomicU32::new(0);
static GRID_EVEN: AtomicBool = AtomicBool::new(false);

pub fn set_grid(grid: Grid) {
    GRID_X.store(grid.gap[0].max(0.0).to_bits(), Ordering::Relaxed);
    GRID_Y.store(grid.gap[1].max(0.0).to_bits(), Ordering::Relaxed);
    GRID_KEEP.store(grid.keep.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    GRID_EVEN.store(grid.even, Ordering::Relaxed);
}

pub fn grid() -> Grid {
    let f = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
    Grid {
        gap: [f(&GRID_X), f(&GRID_Y)],
        keep: f(&GRID_KEEP),
        even: GRID_EVEN.load(Ordering::Relaxed),
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
