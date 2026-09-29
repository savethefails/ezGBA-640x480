use crate::lcd3x::mask_texture_rgba8;
use crate::power::{screen_brightness, screen_rect, screen_rect_in};
use crate::quad::Quad;
use crate::shaders::{GAME_FRAG, RECT_VERT};
use crate::surface::{
    fit, game_rect, grid, scaler, set_source_size, Fit, GfxError, Scaler, OUT_H, OUT_W,
};

/// The GBA's own picture: the size the live texture starts at, and the only one the LCD3x
/// grille was ever built for.
pub const SRC_W: u32 = 240;
pub const SRC_H: u32 = 160;

/// Whether the LCD3x grille can be drawn at all. It is a 3x3 table that only lines up with the
/// source at exactly 3x both ways, which a 640 wide panel never is, so on the RG35XXSP it is
/// off whichever shape the picture is.
fn grille_fits() -> bool {
    let (_, _, w, h) = game_rect();
    crate::surface::source_size() == (SRC_W, SRC_H) && w == SRC_W * 3 && h == SRC_H * 3
}

/// Origin then size, in texture coordinates: everything there is. The default, what a GBA
/// picture is always drawn with, and what a still is always drawn with.
pub const WHOLE_TEXTURE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

pub struct GamePass {
    prog: gl::types::GLuint,
    game: gl::types::GLuint,
    mask: gl::types::GLuint,
    u_rect: gl::types::GLint,
    u_src: gl::types::GLint,
    u_bright: gl::types::GLint,
    u_uv: gl::types::GLint,
    u_grille: gl::types::GLint,
    u_out: gl::types::GLint,
    u_scaler: gl::types::GLint,
    u_sharp: gl::types::GLint,
    u_gap: gl::types::GLint,
    u_keep: gl::types::GLint,
    u_even: gl::types::GLint,
    /// Whether the LCD3x grille is drawn. See `set_grille`.
    grille: bool,
    /// A compositor with nobody driving it is a screen that is on.
    power: f32,
    /// The live game texture's width and height, which is the last frame's: a console's own
    /// size, reallocated when a frame of another size arrives.
    size: (u32, u32),
}

impl GamePass {
    pub fn new() -> Result<Self, GfxError> {
        let prog = crate::shaders::program(RECT_VERT, GAME_FRAG)?;
        // Linear, because sharp-shimmerless blends the one boundary texel pair a panel pixel
        // straddles with the linear tap; it snaps every other sample to a texel centre itself.
        let game = crate::gl::texture(SRC_W, SRC_H, gl::LINEAR, gl::CLAMP_TO_EDGE, gl::BGRA, None);
        let mask = crate::gl::texture(
            3,
            3,
            gl::NEAREST,
            gl::REPEAT,
            gl::RGBA,
            Some(&mask_texture_rgba8()),
        );
        let (
            u_rect,
            u_src,
            u_bright,
            u_uv,
            u_grille,
            u_out,
            u_scaler,
            u_sharp,
            u_gap,
            u_keep,
            u_even,
        );
        unsafe {
            // The other two are fixed for the life of the program: the mask always tiles once
            // per source pixel and the target is always the offscreen frame.
            gl::UseProgram(prog);
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_game"), 0);
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_mask"), 1);
            gl::Uniform2f(
                crate::gl::uniform_location(prog, "u_target"),
                OUT_W as f32,
                OUT_H as f32,
            );
            u_rect = crate::gl::uniform_location(prog, "u_rect");
            u_src = crate::gl::uniform_location(prog, "u_src");
            u_bright = crate::gl::uniform_location(prog, "u_bright");
            u_uv = crate::gl::uniform_location(prog, "u_uv");
            u_grille = crate::gl::uniform_location(prog, "u_grille");
            u_out = crate::gl::uniform_location(prog, "u_out");
            u_scaler = crate::gl::uniform_location(prog, "u_scaler");
            u_sharp = crate::gl::uniform_location(prog, "u_sharp");
            u_gap = crate::gl::uniform_location(prog, "u_gap");
            u_keep = crate::gl::uniform_location(prog, "u_keep");
            u_even = crate::gl::uniform_location(prog, "u_even");
        }
        Ok(GamePass {
            prog,
            game,
            mask,
            u_rect,
            u_src,
            u_bright,
            u_uv,
            u_grille,
            u_out,
            u_scaler,
            u_sharp,
            u_gap,
            u_keep,
            u_even,
            grille: true,
            power: 1.0,
            size: (SRC_W, SRC_H),
        })
    }

    /// The LCD3x grille on or off. It only exists at exactly 3x from source to panel, so the
    /// compositor turns it off when the composite is downscaled onto a smaller panel, and the
    /// picture is shaded by the grille's average instead.
    pub fn set_grille(&mut self, on: bool) {
        self.grille = on;
    }

    pub fn set_power(&mut self, t: f32) {
        self.power = t.clamp(0.0, 1.0);
    }

    /// A frame at its own size. A size the texture does not have yet reallocates it, which
    /// happens when a cart of another console goes in and when a SNES goes in or out of hi-res,
    /// never frame to frame.
    pub fn upload(&mut self, xrgb8888: &[u8], (w, h): (u32, u32)) {
        if w == 0 || h == 0 || xrgb8888.len() < (w * h * 4) as usize {
            return;
        }
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.game);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            if (w, h) != self.size {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    crate::gl::internal_format(gl::BGRA),
                    w as i32,
                    h as i32,
                    0,
                    gl::BGRA,
                    gl::UNSIGNED_BYTE,
                    xrgb8888.as_ptr() as *const std::ffi::c_void,
                );
                self.size = (w, h);
            } else {
                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    0,
                    0,
                    w as i32,
                    h as i32,
                    gl::BGRA,
                    gl::UNSIGNED_BYTE,
                    xrgb8888.as_ptr() as *const std::ffi::c_void,
                );
            }
        }
        set_source_size(self.size);
    }

    pub fn draw(&self, quad: &Quad) {
        self.draw_source(self.game, quad, self.size, screen_rect(self.power));
    }

    /// The same pass over a still: a saved shot, filtered at draw time as the game is rather
    /// than blitted flat beside a game that is. Placed from its own size by the fit in force,
    /// as the live game is — except that a stretched picture's stills are not stretched (see
    /// `Fit::still`): a polaroid is a picture of the game, not of the display setting it is
    /// being viewed at.
    pub fn draw_still(&self, tex: gl::types::GLuint, size: (u32, u32), quad: &Quad) {
        let rect = screen_rect_in(fit().still().rect(size), self.power);
        self.draw_source(tex, quad, size, rect);
    }

    fn draw_source(
        &self,
        tex: gl::types::GLuint,
        quad: &Quad,
        size: (u32, u32),
        (x, y, w, h): (f32, f32, f32, f32),
    ) {
        unsafe {
            gl::UseProgram(self.prog);
            gl::Uniform4f(self.u_rect, x, y, w, h);
            // Per draw, because the live game and a still can be different sizes.
            gl::Uniform2f(self.u_src, size.0 as f32, size.1 as f32);
            let [u0, v0, uw, vh] = WHOLE_TEXTURE;
            gl::Uniform4f(self.u_uv, u0, v0, uw, vh);
            gl::Uniform1f(self.u_bright, screen_brightness(self.power));
            let grille = self.grille && grille_fits();
            gl::Uniform1f(self.u_grille, if grille { 1.0 } else { 0.0 });
            // The size the picture is drawn at this frame, which the power curve squeezes: the
            // scaler measures its panel pixels against the rect they are actually in.
            gl::Uniform2f(self.u_out, w, h);
            let (which, sharp) = match scaler() {
                Scaler::SharpShimmerless => (0.0, 1.0),
                Scaler::PixelAa(sharp) => (1.0, sharp),
            };
            gl::Uniform1f(self.u_scaler, which);
            gl::Uniform1f(self.u_sharp, sharp);
            // The grid is an LCD's, and a console placed at a television's shape was played on a
            // television, which has none. A SNES's hi-res frames would put a line every 1.25
            // panel pixels besides, which is no grid at all, only a dimming.
            let grid = match fit() {
                Fit::Aspect(_) => crate::surface::Grid::default(),
                _ => grid(),
            };
            gl::Uniform2f(self.u_gap, grid.gap[0], grid.gap[1]);
            gl::Uniform1f(self.u_keep, grid.keep);
            gl::Uniform1f(self.u_even, if grid.even { 1.0 } else { 0.0 });
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, tex);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.mask);
            gl::ActiveTexture(gl::TEXTURE0);
        }
        quad.draw();
    }
}

impl Drop for GamePass {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.game);
            gl::DeleteTextures(1, &self.mask);
            gl::DeleteProgram(self.prog);
        }
    }
}
