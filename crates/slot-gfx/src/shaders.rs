//! Shader sources are GLSL ES 1.00 so the device build compiles them unchanged. Only the
//! preamble differs: the device supplies `precision` defaults and
//! `#define FRAG_COLOR gl_FragColor`, the host maps the ES names onto GL 3.3 core.

use crate::surface::GfxError;

const VERT_PREAMBLE: &str = "#version 330 core\n#define attribute in\n#define varying out\n";

const FRAG_PREAMBLE: &str = "#version 330 core\n#define varying in\n\
                             #define texture2D texture\nout vec4 FRAG_COLOR;\n";

/// ES 1.00 is the language these are written in, so the device adds nothing but the name of
/// the output. A `#version` line is omitted rather than set: 100 is the default, and the
/// drivers that reject `#version 100` outnumber the ones that require it.
const VERT_PREAMBLE_ES: &str = "";
const FRAG_PREAMBLE_ES: &str = "#define FRAG_COLOR gl_FragColor\n";

pub fn program(vert: &str, frag: &str) -> Result<gl::types::GLuint, GfxError> {
    let (vp, fp) = match crate::gl::es() {
        true => (VERT_PREAMBLE_ES, FRAG_PREAMBLE_ES),
        false => (VERT_PREAMBLE, FRAG_PREAMBLE),
    };
    crate::gl::program(&format!("{vp}{vert}"), &format!("{fp}{frag}"))
}

/// Unit quad to a rect in target pixels, origin top left. The y flip lives here, so every
/// pass drawing into the offscreen target thinks in screen coordinates and only the blit
/// deals with the framebuffer being stored bottom up.
pub const RECT_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_rect;
uniform vec2 u_target;
varying vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 p = (u_rect.xy + a_pos * u_rect.zw) / u_target;
    gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}
"#;

/// `RECT_VERT` for sprites, turned about the rect's centre by `u_turn`, which holds the cosine
/// and sine of the angle. The corner is placed exactly as `RECT_VERT` places it, plus the
/// difference between the corner turned and unturned; the sprite loop passes exactly (1, 0)
/// for anything that is not turned, which makes that difference exactly zero. A shader of its
/// own rather than a change to `RECT_VERT`, because the game pass links that one too and would
/// read an unset `u_turn` as (0, 0).
pub const SPRITE_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_rect;
uniform vec2 u_target;
uniform vec2 u_turn;
varying vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 mid = u_rect.zw * 0.5;
    vec2 local = a_pos * u_rect.zw - mid;
    vec2 turned = vec2(u_turn.x * local.x - u_turn.y * local.y,
                       u_turn.y * local.x + u_turn.x * local.y);
    vec2 p = (u_rect.xy + a_pos * u_rect.zw + (turned - local)) / u_target;
    gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}
"#;

/// `u_src` is the source size in pixels, which is also the number of times the 3x3 mask
/// tiles across the target: one RGB triad per source pixel, exactly.
///
/// `u_uv` is the part of the texture the quad shows — origin in `xy`, size in `zw`, both in
/// texture coordinates. `(0, 0, 1, 1)` is the whole texture and is what every caller that has
/// not asked for anything else gets, which makes that case `0.0 + v_uv * 1.0`: the arithmetic
/// this line did before the uniform existed, to the bit.
///
/// Only the picture is read through it. The mask stays on `v_uv`, which runs 0..1 across the
/// quad — and the quad for the game is the whole panel — so the grille repeats every `u_src.x`
/// of the panel's width whatever the picture is doing. That is what makes a stretch cheap: it
/// changes which part of the texture fills the panel, never the regularity of the grille over
/// it. What it does change is the relationship between the two. At 3x one mask cell sits on
/// exactly one source pixel; stretched, a source pixel is wider than a cell and the grille
/// stops landing on pixel edges. That is what a blown-up Game Boy picture looked like, and it
/// is the honest consequence of the stretch rather than a defect to design around.
///
/// `u_grille` is 1.0 only where the picture is exactly 3x its source, the one scale the 3x3
/// mask lines up at. The RG35XXSP's 640x427 is not, so there it is 0.0 and the picture is drawn
/// at the core's own colours. It is not dimmed to the grille's average: without the grille's
/// bright stripes that reads as a grey veil over the whole game, not as the same brightness.
///
/// The scaling is sharp-shimmerless, by zadpos, released into the public domain: libretro's
/// slang-shaders `pixel-art-scaling/shaders/sharp-shimmerless.slang`, ported to GLSL ES 1.00.
/// It treats every source pixel as a solid rectangle and gives each panel pixel the colour of
/// whatever covers it, blended by area where a boundary crosses it. So every source pixel keeps
/// its full width, only the one panel pixel a boundary lands in is mixed, and nothing shimmers
/// as the picture scrolls at a non-integer scale. It needs the texture filtered linearly: the
/// blend is the linear tap, placed so its weights are the two areas.
///
/// The port's names are the original's. `pixel` is this fragment in panel pixels from the
/// picture's corner, `u_out` being the size the picture is drawn at, and everything the
/// original measured over the whole texture is measured over the `u_uv` part of it instead, so
/// a stretched Game Boy picture is scaled from its own 160x144.
///
/// The other scaler is Pixel AA, by fishku, released into the public domain (CC0): libretro's
/// slang-shaders `pixel-art-scaling/shaders/pixel_aa/pixel_aa_single_pass.slang`, the
/// `pixel_aa_gamma` path of `shared.inc` without the subpixel variant. It places the same
/// boundaries sharp-shimmerless does, but eases across each one with `slopestep` — steeper as
/// `u_sharp` rises past 1.0 — and mixes the four texels around it in linear light rather than
/// in the stored gamma values, so a blended edge is neither darker nor thinner than either side.
/// `u_scaler` picks between the two: 0.0 sharp-shimmerless, 1.0 Pixel AA.
///
/// The LCD grid is ezGBA's own, and replaces both scalers when it is on (`u_gap` above zero).
/// It models a real LCD's black matrix: a dark gap of `u_gap` panel pixels centred on every
/// source pixel edge, each axis on its own. Everything is by area, in linear light:
///
/// - Each panel pixel takes, from each of the one or two source pixels under it, only the part
///   of that source pixel outside its gaps. Gaps are centred on the edge, so every source pixel
///   gives up the same share of itself wherever it lands on the panel, and nothing pulses as a
///   picture scrolls.
/// - That share is given back as gain on what is left lit, so a source pixel emits the same
///   light it would without the grid. The panel cannot go past white, so a bright pixel narrows
///   its own gaps to what its headroom allows — fully at `u_keep` 1.0, half way at 0.5.
/// - The trick: at 640 across 240 an edge lands a third or two thirds of the way into a panel
///   column, which is where a scaler has to blend. A gap two thirds of a panel pixel wide,
///   centred there, covers exactly the other source pixel's part of that column, so no panel
///   column holds two colours: the non-integer scale disappears into the grid.
///
/// High precision where the GPU has it, which the H700's Mali does: `pixel` runs to 640, and
/// at mediump's 16 bit float that is half a pixel apart by the right hand side of the picture —
/// far too coarse to find the boundary inside one.
pub const GAME_FRAG: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform sampler2D u_game;
uniform sampler2D u_mask;
uniform vec2 u_src;
uniform vec4 u_uv;
uniform vec2 u_out;
uniform float u_bright;
uniform float u_grille;
uniform float u_scaler;
uniform float u_sharp;
uniform vec2 u_gap;
uniform float u_keep;
varying vec2 v_uv;

#define FIX(c) max(abs(c), 1e-5)

vec2 sharp_shimmerless(vec2 pixel, vec2 source) {
    vec2 scale = u_out / source;
    vec2 invscale = 1.0 / scale;
    vec4 pixel_borders = vec4(floor(pixel), ceil(pixel));
    vec4 texel_borders = floor(invscale.xyxy * pixel_borders);
    vec2 same_texel = step(FIX(0.0), abs(texel_borders.xy - texel_borders.zw));
    return texel_borders.zw + 0.5 - (scale * texel_borders.zw - pixel_borders.xy) * same_texel;
}

// Similar to smoothstep, but has a configurable slope at x = 0.5.
vec2 slopestep(vec2 edge0, vec2 edge1, vec2 x, float slope) {
    x = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    vec2 s = sign(x - 0.5);
    vec2 o = (1.0 + s) * 0.5;
    return o - 0.5 * s * pow(2.0 * (o - s * x), vec2(slope));
}

vec3 to_lin(vec3 x) { return pow(x, vec3(2.2)); }
vec3 to_srgb(vec3 x) { return pow(x, vec3(1.0 / 2.2)); }

// `tx_coord` is in texels of the shown part of the texture; `fetch` turns one back into a
// sample of the whole of it.
vec3 fetch(vec2 texel) {
    return texture2D(u_game, u_uv.xy + texel / u_src).rgb;
}

vec3 pixel_aa_gamma(vec2 tx_coord, vec2 tx_per_px) {
    float sharpness_upper = min(1.0, u_sharp);
    vec2 trans_lb = sharpness_upper * (0.5 - 0.5 * tx_per_px);
    vec2 trans_ub = 1.0 - sharpness_upper * (1.0 - (0.5 + 0.5 * tx_per_px));
    float trans_slope = max(1.0, u_sharp);

    vec2 period = floor(tx_coord - 0.5);
    vec2 phase = tx_coord - 0.5 - period;
    vec2 offset = slopestep(trans_lb, trans_ub, phase, trans_slope);
    return to_srgb(
        mix(mix(to_lin(fetch(period + 0.5)), to_lin(fetch(period + vec2(1.5, 0.5))), offset.x),
            mix(to_lin(fetch(period + vec2(0.5, 1.5))), to_lin(fetch(period + 1.5)), offset.x),
            offset.y));
}

// The LCD grid. Along one axis, for panel column `i` at `scale` panel pixels per source pixel:
// the part of source pixel `j`, with a gap of `half_gap` on each side of it, inside the column.
float lit(float i, float j, float scale, float half_gap) {
    float lo = max(i, j * scale + half_gap);
    float hi = min(i + 1.0, (j + 1.0) * scale - half_gap);
    // Less than a ten thousandth of a pixel is rounding, not coverage: an edge meant to sit
    // exactly on a gap's edge would otherwise leave a sliver of the next colour, which linear
    // light turns into a visible trace against black.
    return max(hi - lo - 1e-4, 0.0);
}

// Source pixel `j` (x and y) seen through panel pixel `i`: its light, the grid taken out of it
// and given back as gain. `t` narrows its gaps to what its brightness leaves room to give back.
vec3 grid_tap(vec2 i, vec2 j, vec2 scale) {
    vec3 c = to_lin(fetch(j + 0.5));
    float cmax = max(c.r, max(c.g, c.b));
    vec2 share = u_gap / scale;
    float room = (1.0 - cmax) / max(share.x + share.y, 1e-4);
    float t = mix(1.0, clamp(room, 0.0, 1.0), u_keep);
    vec2 half_gap = 0.5 * t * u_gap;
    float area = lit(i.x, j.x, scale.x, half_gap.x) * lit(i.y, j.y, scale.y, half_gap.y);
    vec2 kept = 1.0 - t * share;
    return c * area / (kept.x * kept.y);
}

vec3 grid(vec2 source) {
    vec2 scale = u_out / source;
    vec2 i = floor(v_uv * u_out);
    // The one or two source pixels under this panel pixel on each axis.
    vec2 j0 = floor(i / scale + 1e-4);
    vec2 j1 = floor((i + 1.0) / scale - 1e-4);
    vec3 light = grid_tap(i, j0, scale);
    if (j1.x > j0.x) light += grid_tap(i, vec2(j1.x, j0.y), scale);
    if (j1.y > j0.y) light += grid_tap(i, vec2(j0.x, j1.y), scale);
    if (j1.x > j0.x && j1.y > j0.y) light += grid_tap(i, j1, scale);
    return to_srgb(light);
}

void main() {
    vec2 source = u_src * u_uv.zw;
    vec3 picture;
    if (u_gap.x + u_gap.y > 0.0) {
        picture = grid(source);
    } else if (u_scaler < 0.5) {
        vec2 texel = sharp_shimmerless(v_uv * u_out, source);
        picture = texture2D(u_game, u_uv.xy + texel / u_src).rgb;
    } else {
        picture = pixel_aa_gamma(v_uv * source, source / u_out);
    }
    vec3 mask = mix(vec3(1.0), texture2D(u_mask, v_uv * u_src).rgb, u_grille);
    vec3 rgb = picture * mask;
    FRAG_COLOR = vec4(rgb * u_bright, 1.0);
}
"#;

pub const SPRITE_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec4 u_colour;
varying vec2 v_uv;
void main() {
    FRAG_COLOR = texture2D(u_tex, v_uv) * u_colour;
}
"#;

pub const BLIT_VERT: &str = r#"
attribute vec2 a_pos;
varying vec2 v_uv;
void main() {
    v_uv = a_pos;
    gl_Position = vec4(a_pos * 2.0 - 1.0, 0.0, 1.0);
}
"#;

pub const BLIT_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec3 u_gain;
varying vec2 v_uv;
void main() {
    FRAG_COLOR = vec4(texture2D(u_tex, v_uv).rgb * u_gain, 1.0);
}
"#;
