mod draw;
mod fbdev;
mod fbo;
mod gl;
mod grade;
#[cfg(target_os = "macos")]
mod headless;
#[cfg(feature = "host")]
mod host;
mod lcd3x;
mod pipeline;
mod power;
mod quad;
mod shaders;
mod surface;

pub use draw::{Draw, TexId};
// Built on the host too, so the port stays under the type checker and the linter that only
// ever run there. Opening it away from the device fails at the first dlopen, not at compile.
pub use fbdev::{egl_error, panel_mode, panel_size, FbdevSurface, PbufferSurface};
pub use fbo::{Compositor, BACKDROP};
pub use grade::{blue_light_gain, BLUE_LIGHT_MAX};
#[cfg(target_os = "macos")]
pub use headless::HeadlessSurface;
/// Linux's offscreen context for the readback tests: EGL and GLES2, as on the device.
#[cfg(target_os = "linux")]
pub struct HeadlessSurface(PbufferSurface);

#[cfg(target_os = "linux")]
impl HeadlessSurface {
    pub fn new() -> Result<Self, GfxError> {
        PbufferSurface::new((OUT_W, OUT_H)).map(HeadlessSurface)
    }
}

#[cfg(target_os = "linux")]
impl Surface for HeadlessSurface {
    fn make_current(&mut self) -> Result<(), GfxError> {
        self.0.make_current()
    }
    fn window_size(&self) -> (u32, u32) {
        self.0.window_size()
    }
    fn swap(&mut self) -> Result<(), GfxError> {
        self.0.swap()
    }
    fn proc_address(&self, name: &str) -> *const std::ffi::c_void {
        self.0.proc_address(name)
    }
}
#[cfg(feature = "host")]
pub use host::HostSurface;
pub use lcd3x::{lcd3x_mask, mask_texture_rgba8};
pub use pipeline::{SRC_H, SRC_W, WHOLE_TEXTURE};
pub use power::{screen_brightness, screen_scale, screen_width};
pub use surface::{
    blit_is_whole, blit_rect, blit_rect_fit, fit_rect, fit_scale, game_rect, picture, set_picture,
    GfxError, Picture, Surface, OUT_H, OUT_W,
};
