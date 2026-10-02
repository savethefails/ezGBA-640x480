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

/// Colour depth on the game picture: RetroArch's `image-adjustment` pass, the part of it the
/// RG35XXSP's players reach for, in its order. Saturation first, scaled the way that pass scales
/// it (HSV saturation, so each colour moves away from its own brightest channel and the brightest
/// stays put), then contrast about mid grey, then gamma, as `target ÷ monitor` gamma: above 1
/// darkens the middle tones and leaves black and white where they were.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Tone {
    pub gamma: f32,
    pub saturation: f32,
    pub contrast: f32,
}

impl Tone {
    /// The picture as the core drew it.
    pub const NEUTRAL: Tone = Tone {
        gamma: 1.0,
        saturation: 1.0,
        contrast: 1.0,
    };
}

impl Default for Tone {
    fn default() -> Self {
        Tone::NEUTRAL
    }
}

static TONE_GAMMA: AtomicU32 = AtomicU32::new(0x3f80_0000);
static TONE_SATURATION: AtomicU32 = AtomicU32::new(0x3f80_0000);
static TONE_CONTRAST: AtomicU32 = AtomicU32::new(0x3f80_0000);

pub fn set_tone(tone: Tone) {
    TONE_GAMMA.store(tone.gamma.clamp(0.5, 2.0).to_bits(), Ordering::Relaxed);
    TONE_SATURATION.store(tone.saturation.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    TONE_CONTRAST.store(tone.contrast.clamp(0.5, 2.0).to_bits(), Ordering::Relaxed);
}

pub fn tone() -> Tone {
    let f = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
    Tone {
        gamma: f(&TONE_GAMMA),
        saturation: f(&TONE_SATURATION),
        contrast: f(&TONE_CONTRAST),
    }
}

/// How a console's picture is placed on the panel. The console decides, not the picture: a
/// GBA follows the card's `picture` setting, a Game Boy is drawn at a whole multiple, a SNES at
/// the 4:3 its games were made for.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub enum Fit {
    /// A GBA's: the shape `picture` chooses, whatever the size of the frame.
    #[default]
    Gba,
    /// Square pixels at the largest whole multiple of the frame that fits, centred. A Game
    /// Boy's 160x144 is exactly 3x on a 640x480 panel: 480x432, every pixel three by three.
    Whole,
    /// A display shape, as large as fits and centred, at whatever scale that takes. A SNES
    /// draws 256x224 (or 512x448) for a 4:3 television, which fills a 4:3 panel exactly.
    Aspect(f32),
    /// The whole panel, shape and all: a Game Boy picture stretched to fill it.
    Fill,
    /// The panel's full width, and its height at the largest whole multiple of the frame's rows:
    /// a SNES's 224 at 2x is 448, with a 16 px bar above and below, and its 448 hi-res rows at
    /// 1x are the same 448. Every source row is exactly the same number of panel rows, so a one
    /// pixel line across a letter can never be thinned into its neighbour by the scale, as it is
    /// at 224 to 480 (2.14 rows each, two or three). About 7% shorter than 4:3, which is the
    /// price of rows that are all the same height.
    Rows,
}

impl Fit {
    /// How a still is placed. A photograph is a picture of the game, not of the display
    /// setting it was later viewed at, so a stretched Game Boy's polaroids stay the shape the
    /// game is.
    pub fn still(self) -> Fit {
        match self {
            Fit::Fill => Fit::Whole,
            other => other,
        }
    }

    /// Where a `w` by `h` frame goes on the panel, as x, y, width and height in panel pixels,
    /// every edge on a whole pixel.
    pub fn rect(self, (w, h): (u32, u32)) -> (u32, u32, u32, u32) {
        let centred = |dw: u32, dh: u32| ((OUT_W - dw) / 2, (OUT_H - dh) / 2, dw, dh);
        let aspect = |a: f32| {
            if (OUT_W as f32 / OUT_H as f32) > a {
                centred(((OUT_H as f32 * a).round() as u32).min(OUT_W), OUT_H)
            } else {
                centred(OUT_W, ((OUT_W as f32 / a).round() as u32).min(OUT_H))
            }
        };
        match self {
            Fit::Gba => match picture() {
                Picture::FourThree => (0, 0, OUT_W, OUT_H),
                Picture::ThreeTwo => centred(OUT_W, 427),
            },
            Fit::Fill => (0, 0, OUT_W, OUT_H),
            Fit::Rows if h > 0 && h <= OUT_H => centred(OUT_W, (OUT_H / h) * h),
            Fit::Aspect(a) if a > 0.0 => aspect(a),
            _ if w == 0 || h == 0 => (0, 0, OUT_W, OUT_H),
            Fit::Whole => match (OUT_W / w).min(OUT_H / h) {
                // A frame bigger than the panel has no whole multiple, and is fitted instead.
                0 => aspect(w as f32 / h as f32),
                s => centred(w * s, h * s),
            },
            Fit::Aspect(_) | Fit::Rows => aspect(w as f32 / h as f32),
        }
    }
}

static FIT: AtomicU8 = AtomicU8::new(0);
static FIT_ASPECT: AtomicU32 = AtomicU32::new(0);
/// The live frame's width and height, packed, as last uploaded.
static SOURCE: AtomicU32 = AtomicU32::new((240 << 16) | 160);

/// Set by the app for the cart in the slot, and read by every draw.
pub fn set_fit(fit: Fit) {
    let (which, aspect) = match fit {
        Fit::Gba => (0, 0.0),
        Fit::Whole => (1, 0.0),
        Fit::Aspect(a) => (2, a),
        Fit::Fill => (3, 0.0),
        Fit::Rows => (4, 0.0),
    };
    FIT_ASPECT.store(f32::to_bits(aspect), Ordering::Relaxed);
    FIT.store(which, Ordering::Relaxed);
}

pub fn fit() -> Fit {
    match FIT.load(Ordering::Relaxed) {
        1 => Fit::Whole,
        2 => Fit::Aspect(f32::from_bits(FIT_ASPECT.load(Ordering::Relaxed))),
        3 => Fit::Fill,
        4 => Fit::Rows,
        _ => Fit::Gba,
    }
}

/// The size of the live frame, set whenever one of a new size is uploaded.
pub fn set_source_size((w, h): (u32, u32)) {
    SOURCE.store((w.min(0xffff) << 16) | h.min(0xffff), Ordering::Relaxed);
}

pub fn source_size() -> (u32, u32) {
    let v = SOURCE.load(Ordering::Relaxed);
    (v >> 16, v & 0xffff)
}

/// Where the live game picture sits in the frame, as x, y, width and height in panel pixels:
/// the fit in force for the frame being shown. Only a Game Boy's is a whole multiple, so the
/// game pass scales with Pixel AA or sharp-shimmerless (see `GAME_FRAG`).
pub fn game_rect() -> (u32, u32, u32, u32) {
    fit().rect(source_size())
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
