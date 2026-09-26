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

void main() {
    vec2 source = u_src * u_uv.zw;
    vec2 texel = sharp_shimmerless(v_uv * u_out, source);
    vec2 uv = u_uv.xy + texel / u_src;
    vec3 mask = mix(vec3(1.0), texture2D(u_mask, v_uv * u_src).rgb, u_grille);
    vec3 rgb = texture2D(u_game, uv).rgb * mask;
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
