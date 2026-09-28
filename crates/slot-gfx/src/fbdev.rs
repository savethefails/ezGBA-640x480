//! EGL on the framebuffer, which is how the device presents. libEGL and libGLESv2 are opened
//! at runtime for the same reason the libretro core is: neither exists on the build machine,
//! and a link time dependency on either would make the cross build need the device rootfs.

use std::ffi::{c_char, c_void, CString};

use libloading::Library;

use crate::surface::{GfxError, Surface};

/// What the panel is if the framebuffer will not say. This build is for the RG35XXSP, whose
/// panel is 640x480 and is what the UI is laid out for. The surface is still sized from the
/// driver wherever it answers, so on a 720x480 RG34XXSP the same picture comes up centred with
/// a bar either side rather than off the edge.
const FALLBACK_PANEL: (u32, u32) = (640, 480);

const FB0: &str = "/sys/class/graphics/fb0";
const FB0_DEV: &str = "/dev/fb0";

/// `FBIOGET_VSCREENINFO`: fills a `struct fb_var_screeninfo`, whose first two fields are the
/// visible width and height.
const FBIOGET_VSCREENINFO: std::ffi::c_ulong = 0x4600;
/// `FBIOBLANK`, with `FB_BLANK_UNBLANK` as its argument: lights a panel the kernel, or
/// whatever ran before, left blanked. Every fbdev driver takes it and it is a no-op on a
/// panel that is already lit.
const FBIOBLANK: std::ffi::c_ulong = 0x4611;

extern "C" {
    fn ioctl(fd: std::ffi::c_int, request: std::ffi::c_ulong, ...) -> std::ffi::c_int;
}

/// `fb_var_screeninfo` as words. The struct is 160 bytes; the buffer is larger so a kernel
/// that has grown it can never write past it.
fn var_info() -> Option<[u32; 64]> {
    use std::os::fd::AsRawFd;
    let fb = std::fs::File::open(FB0_DEV).ok()?;
    let mut info = [0u32; 64];
    let ok = unsafe { ioctl(fb.as_raw_fd(), FBIOGET_VSCREENINFO, info.as_mut_ptr()) };
    (ok == 0).then_some(info)
}

/// The visible resolution as the driver scans it out, straight from the framebuffer device.
/// The sysfs files are text a driver may leave empty or fill with the double buffered
/// allocation; this is the one answer every fbdev driver has to give.
fn panel_ioctl() -> Option<(u32, u32)> {
    let info = var_info()?;
    (info[0] > 0 && info[1] > 0).then_some((info[0], info[1]))
}

/// Everything the driver says about the scanout, on one line. A dark panel with a running
/// frontend has nothing else to go on, and these are the numbers that tell a blanked panel,
/// a pan to the wrong buffer and a mode the panel cannot take apart.
fn report_fb(when: &str) {
    match var_info() {
        Some(v) => eprintln!(
            "slot: fb0 {when}: {}x{} virtual {}x{} offset {},{} bpp {} rgba {}/{} {}/{} {}/{} {}/{} activate {}",
            v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[8], v[9], v[11], v[12], v[14], v[15],
            v[17], v[18], v[21]
        ),
        None => eprintln!("slot: fb0 {when}: {FB0_DEV} did not answer"),
    }
    let attr = |name: &str| {
        std::fs::read_to_string(format!("{FB0}/{name}"))
            .map(|t| t.trim().replace('\n', " | "))
            .unwrap_or_else(|e| format!("<{e}>"))
    };
    eprintln!(
        "slot: fb0 {when}: mode [{}] modes [{}] virtual_size [{}] blank [{}]",
        attr("mode"),
        attr("modes"),
        attr("virtual_size"),
        attr("blank")
    );
}

/// Light the panel if it was left blanked, and say whether the driver took it.
fn unblank() {
    use std::os::fd::AsRawFd;
    let Ok(fb) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(FB0_DEV)
    else {
        eprintln!("slot: fb0 unblank: {FB0_DEV} would not open");
        return;
    };
    let ok = unsafe { ioctl(fb.as_raw_fd(), FBIOBLANK, 0 as std::ffi::c_ulong) };
    let err = std::io::Error::last_os_error();
    match ok {
        0 => eprintln!("slot: fb0 unblank: ok"),
        _ => eprintln!("slot: fb0 unblank: {err}"),
    }
}

const EGL_SUCCESS: i32 = 0x3000;
const EGL_NONE: i32 = 0x3038;
const EGL_ALPHA_SIZE: i32 = 0x3021;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_HEIGHT: i32 = 0x3056;
const EGL_WIDTH: i32 = 0x3057;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const EGL_OPENGL_ES_API: u32 = 0x30A0;
const EGL_WINDOW_BIT: i32 = 0x0004;
const EGL_OPENGL_ES2_BIT: i32 = 0x0004;

type Ptr = *mut c_void;
type GetDisplay = unsafe extern "C" fn(Ptr) -> Ptr;
type Initialize = unsafe extern "C" fn(Ptr, *mut i32, *mut i32) -> u32;
type BindApi = unsafe extern "C" fn(u32) -> u32;
type ChooseConfig = unsafe extern "C" fn(Ptr, *const i32, *mut Ptr, i32, *mut i32) -> u32;
type CreateWindowSurface = unsafe extern "C" fn(Ptr, Ptr, Ptr, *const i32) -> Ptr;
type CreateContext = unsafe extern "C" fn(Ptr, Ptr, Ptr, *const i32) -> Ptr;
type MakeCurrent = unsafe extern "C" fn(Ptr, Ptr, Ptr, Ptr) -> u32;
type QuerySurface = unsafe extern "C" fn(Ptr, Ptr, i32, *mut i32) -> u32;
type SwapBuffers = unsafe extern "C" fn(Ptr, Ptr) -> u32;
type SwapInterval = unsafe extern "C" fn(Ptr, i32) -> u32;
type GetProcAddress = unsafe extern "C" fn(*const c_char) -> *const c_void;
type GetError = unsafe extern "C" fn() -> i32;
type Terminate = unsafe extern "C" fn(Ptr) -> u32;
type CreatePbufferSurface = unsafe extern "C" fn(Ptr, Ptr, *const i32) -> Ptr;

struct Egl {
    get_display: GetDisplay,
    initialize: Initialize,
    bind_api: BindApi,
    choose_config: ChooseConfig,
    create_window_surface: CreateWindowSurface,
    create_context: CreateContext,
    make_current: MakeCurrent,
    query_surface: QuerySurface,
    swap_buffers: SwapBuffers,
    swap_interval: SwapInterval,
    get_proc_address: GetProcAddress,
    get_error: GetError,
    terminate: Terminate,
    create_pbuffer_surface: CreatePbufferSurface,
    /// Last, so the symbols above are still valid while the rest of the struct drops.
    _lib: Library,
}

/// Mali's fbdev EGL takes a pointer to this as its native window and keeps reading it, so it
/// outlives the call that made it rather than living on the stack.
#[repr(C)]
struct FbdevWindow {
    width: u16,
    height: u16,
}

pub struct FbdevSurface {
    egl: Egl,
    /// Core GLES entry points come from here. Mali's `eglGetProcAddress` answers for
    /// extensions and returns null for the rest, which would leave the loader with a
    /// program it cannot draw.
    gles: Library,
    display: Ptr,
    surface: Ptr,
    context: Ptr,
    size: (u32, u32),
    /// Presented so far, for the few frames that are checked and reported.
    frames: u32,
    _window: Box<FbdevWindow>,
}

/// `/sys/class/graphics/fb0/modes`, printed as `<name>:<w>x<h><p|i>-<hz>`, and the panel as
/// the driver is actually scanning it out.
pub fn panel_mode(text: &str) -> Option<(u32, u32)> {
    // The name is optional and the trailer says how it is scanned and how fast. Only the size
    // is wanted, and it is the one part every driver spells the same way.
    let body = text.lines().next()?.rsplit(':').next()?;
    let (w, rest) = body.split_once('x')?;
    let h: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let (w, h) = (w.trim().parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// `/sys/class/graphics/fb0/virtual_size`, which the driver prints as `width,height`. This is
/// what was allocated rather than what is shown: a double buffered panel reports two screens
/// of height here, so it is a last resort behind the mode.
pub fn panel_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.trim().split_once(',')?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// EGL refuses a request by returning false with nothing queued, so the code read back is
/// EGL_SUCCESS. Printing it as an error number sends bring-up after a fault that never was.
pub fn egl_error(what: &str, code: i32) -> GfxError {
    match code {
        EGL_SUCCESS => GfxError::Context(format!("{what}: refused, egl flagged nothing")),
        _ => GfxError::Context(format!("{what}: egl error 0x{code:x}")),
    }
}

fn open(name: &str) -> Result<Library, GfxError> {
    unsafe { Library::new(name) }.map_err(|e| GfxError::Context(format!("{name}: {e}")))
}

/// The symbol's address, copied out of the borrow. Every one of these outlives the call
/// because the `Library` it came from is owned for the life of the surface.
unsafe fn sym<T: Copy>(lib: &Library, name: &str) -> Result<T, GfxError> {
    lib.get::<T>(name.as_bytes())
        .map(|s| *s)
        .map_err(|e| GfxError::Context(format!("{name}: {e}")))
}

impl Egl {
    fn load() -> Result<Self, GfxError> {
        let lib = open("libEGL.so.1").or_else(|_| open("libEGL.so"))?;
        unsafe {
            Ok(Egl {
                get_display: sym(&lib, "eglGetDisplay")?,
                initialize: sym(&lib, "eglInitialize")?,
                bind_api: sym(&lib, "eglBindAPI")?,
                choose_config: sym(&lib, "eglChooseConfig")?,
                create_window_surface: sym(&lib, "eglCreateWindowSurface")?,
                create_context: sym(&lib, "eglCreateContext")?,
                make_current: sym(&lib, "eglMakeCurrent")?,
                query_surface: sym(&lib, "eglQuerySurface")?,
                swap_buffers: sym(&lib, "eglSwapBuffers")?,
                swap_interval: sym(&lib, "eglSwapInterval")?,
                get_proc_address: sym(&lib, "eglGetProcAddress")?,
                get_error: sym(&lib, "eglGetError")?,
                terminate: sym(&lib, "eglTerminate")?,
                create_pbuffer_surface: sym(&lib, "eglCreatePbufferSurface")?,
                _lib: lib,
            })
        }
    }

    fn fail(&self, what: &str) -> GfxError {
        egl_error(what, unsafe { (self.get_error)() })
    }
}

impl FbdevSurface {
    pub fn new() -> Result<Self, GfxError> {
        report_fb("at start");
        unblank();
        let attr = |name: &str| std::fs::read_to_string(format!("{FB0}/{name}")).ok();
        // The driver's own answer, then the mode in use, then the modes on offer, then what
        // was allocated. `mode` is empty on drivers that never implemented it, and
        // `virtual_size` describes the scrollback rather than the screen: sizing a window from
        // it asks for a surface half of which is off the bottom of the panel.
        let hint = panel_ioctl()
            .or_else(|| attr("mode").as_deref().and_then(panel_mode))
            .or_else(|| attr("modes").as_deref().and_then(panel_mode))
            .or_else(|| attr("virtual_size").as_deref().and_then(panel_size))
            .unwrap_or(FALLBACK_PANEL);
        FbdevSurface::open(hint)
    }

    fn open(hint: (u32, u32)) -> Result<Self, GfxError> {
        let egl = Egl::load()?;
        let gles = open("libGLESv2.so.2").or_else(|_| open("libGLESv2.so"))?;
        unsafe {
            let display = (egl.get_display)(std::ptr::null_mut());
            if display.is_null() {
                return Err(egl.fail("eglGetDisplay"));
            }
            if (egl.initialize)(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                return Err(egl.fail("eglInitialize"));
            }
            if (egl.bind_api)(EGL_OPENGL_ES_API) == 0 {
                return Err(egl.fail("eglBindAPI"));
            }
            let attrs = [
                EGL_SURFACE_TYPE,
                EGL_WINDOW_BIT,
                EGL_RED_SIZE,
                8,
                EGL_GREEN_SIZE,
                8,
                EGL_BLUE_SIZE,
                8,
                EGL_ALPHA_SIZE,
                0,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES2_BIT,
                EGL_NONE,
            ];
            let mut config: Ptr = std::ptr::null_mut();
            let mut found = 0;
            if (egl.choose_config)(display, attrs.as_ptr(), &mut config, 1, &mut found) == 0 {
                return Err(egl.fail("eglChooseConfig"));
            }
            if found == 0 {
                return Err(GfxError::Context(
                    "eglChooseConfig: no es2 window config with 8/8/8 colour".into(),
                ));
            }
            let mut window = Box::new(FbdevWindow {
                width: hint.0 as u16,
                height: hint.1 as u16,
            });
            let native = &mut *window as *mut FbdevWindow as Ptr;
            // A driver that wants no native window at all takes null, and one that wants the
            // fbdev struct takes the pointer. Which of the two this Mali is has never been
            // checked on hardware, so both are tried before giving up.
            let mut surface =
                (egl.create_window_surface)(display, config, native, std::ptr::null());
            let mut path = "fbdev window";
            if surface.is_null() {
                path = "null window";
                surface = (egl.create_window_surface)(
                    display,
                    config,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
            }
            if surface.is_null() {
                return Err(egl.fail("eglCreateWindowSurface"));
            }
            let context_attrs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
            let context = (egl.create_context)(
                display,
                config,
                std::ptr::null_mut(),
                context_attrs.as_ptr(),
            );
            if context.is_null() {
                return Err(egl.fail("eglCreateContext"));
            }
            if (egl.make_current)(display, surface, surface, context) == 0 {
                return Err(egl.fail("eglMakeCurrent"));
            }
            // Present is locked to the panel; the GBA to panel drift is absorbed by audio
            // rate control, exactly as it is on the host.
            (egl.swap_interval)(display, 1);
            let size = query_size(&egl, display, surface).unwrap_or(hint);
            eprintln!(
                "slot: panel {}x{}, surface {}x{} through the {path}",
                hint.0, hint.1, size.0, size.1
            );
            report_gl(&gles);
            Ok(FbdevSurface {
                egl,
                gles,
                display,
                surface,
                context,
                size,
                frames: 0,
                _window: window,
            })
        }
    }
}

type GetString = unsafe extern "C" fn(u32) -> *const c_char;
type GetGlError = unsafe extern "C" fn() -> u32;
type ReadPixels = unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void);

const GL_VENDOR: u32 = 0x1F00;
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;

/// Which GPU and driver the context landed on.
fn report_gl(gles: &Library) {
    let Ok(get) = (unsafe { sym::<GetString>(gles, "glGetString") }) else {
        return;
    };
    let text = |name| {
        let p = unsafe { get(name) };
        match p.is_null() {
            true => "<null>".to_string(),
            false => unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned(),
        }
    };
    eprintln!(
        "slot: gl {} | {} | {}",
        text(GL_VENDOR),
        text(GL_RENDERER),
        text(GL_VERSION)
    );
}

impl FbdevSurface {
    /// What the frame about to be presented holds, at the centre and near each corner, and
    /// whether GL has flagged anything. Colour here and a dark panel is a scanout or backlight
    /// fault, not a drawing one.
    fn report_frame(&self) {
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        let read = unsafe { sym::<ReadPixels>(&self.gles, "glReadPixels") };
        let error = unsafe { sym::<GetGlError>(&self.gles, "glGetError") };
        let (Ok(read), Ok(error)) = (read, error) else {
            return;
        };
        let mut line = format!("slot: frame {}:", self.frames);
        for (x, y) in [(w / 2, h / 2), (w / 8, h / 8), (w * 7 / 8, h * 7 / 8)] {
            let mut px = [0u8; 4];
            unsafe {
                read(
                    x,
                    y,
                    1,
                    1,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    px.as_mut_ptr().cast(),
                )
            };
            line += &format!(" ({x},{y})=#{:02x}{:02x}{:02x}", px[0], px[1], px[2]);
        }
        line += &format!(" gl error 0x{:x}", unsafe { error() });
        eprintln!("{line}");
        report_fb(&format!("frame {}", self.frames));
    }
}

fn query_size(egl: &Egl, display: Ptr, surface: Ptr) -> Option<(u32, u32)> {
    let (mut w, mut h) = (0, 0);
    unsafe {
        if (egl.query_surface)(display, surface, EGL_WIDTH, &mut w) == 0
            || (egl.query_surface)(display, surface, EGL_HEIGHT, &mut h) == 0
        {
            return None;
        }
    }
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

impl Surface for FbdevSurface {
    fn make_current(&mut self) -> Result<(), GfxError> {
        let ok = unsafe {
            (self.egl.make_current)(self.display, self.surface, self.surface, self.context)
        };
        match ok {
            0 => Err(self.egl.fail("eglMakeCurrent")),
            _ => Ok(()),
        }
    }

    fn window_size(&self) -> (u32, u32) {
        self.size
    }

    fn swap(&mut self) -> Result<(), GfxError> {
        self.frames += 1;
        if matches!(self.frames, 1 | 60 | 600) {
            self.report_frame();
        }
        match unsafe { (self.egl.swap_buffers)(self.display, self.surface) } {
            0 => Err(self.egl.fail("eglSwapBuffers")),
            _ => Ok(()),
        }
    }

    fn proc_address(&self, name: &str) -> *const c_void {
        let Ok(c) = CString::new(name) else {
            return std::ptr::null();
        };
        // The library first. Mali's eglGetProcAddress answers for extensions and returns null
        // for core GLES entry points, which would leave the loader with a program it cannot
        // draw a single triangle with.
        let exported = unsafe { self.gles.get::<unsafe extern "C" fn()>(name.as_bytes()) };
        match exported {
            Ok(f) => *f as *const c_void,
            Err(_) => unsafe { (self.egl.get_proc_address)(c.as_ptr()) },
        }
    }
}

impl Drop for FbdevSurface {
    fn drop(&mut self) {
        unsafe {
            (self.egl.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.egl.terminate)(self.display);
        }
    }
}

const EGL_PBUFFER_BIT: i32 = 0x0001;
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;

type GetPlatformDisplay = unsafe extern "C" fn(u32, Ptr, *const isize) -> Ptr;

/// An offscreen GLES2 context through the same EGL entry points the device uses, for readback
/// checks on a Linux machine with no panel and no display server. Mesa's surfaceless platform
/// renders it in software, which puts every shader through the GLES2 compiler the H700 has.
pub struct PbufferSurface {
    egl: Egl,
    gles: Library,
    display: Ptr,
    surface: Ptr,
    context: Ptr,
    size: (u32, u32),
}

impl PbufferSurface {
    pub fn new(size: (u32, u32)) -> Result<Self, GfxError> {
        let egl = Egl::load()?;
        let gles = open("libGLESv2.so.2").or_else(|_| open("libGLESv2.so"))?;
        unsafe {
            // Surfaceless first: it needs no X server. The default display is what is left on a
            // machine whose EGL does not know the Mesa platform.
            let name = CString::new("eglGetPlatformDisplayEXT").unwrap_or_default();
            let platform = (egl.get_proc_address)(name.as_ptr());
            let mut display = std::ptr::null_mut();
            if !platform.is_null() {
                let get: GetPlatformDisplay = std::mem::transmute(platform);
                display = get(
                    EGL_PLATFORM_SURFACELESS_MESA,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
            }
            if display.is_null() {
                display = (egl.get_display)(std::ptr::null_mut());
            }
            if display.is_null() {
                return Err(egl.fail("eglGetDisplay"));
            }
            if (egl.initialize)(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                return Err(egl.fail("eglInitialize"));
            }
            if (egl.bind_api)(EGL_OPENGL_ES_API) == 0 {
                return Err(egl.fail("eglBindAPI"));
            }
            let attrs = [
                EGL_SURFACE_TYPE,
                EGL_PBUFFER_BIT,
                EGL_RED_SIZE,
                8,
                EGL_GREEN_SIZE,
                8,
                EGL_BLUE_SIZE,
                8,
                EGL_ALPHA_SIZE,
                8,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES2_BIT,
                EGL_NONE,
            ];
            let mut config: Ptr = std::ptr::null_mut();
            let mut found = 0;
            if (egl.choose_config)(display, attrs.as_ptr(), &mut config, 1, &mut found) == 0
                || found == 0
            {
                return Err(egl.fail("eglChooseConfig"));
            }
            let pbuffer = [
                EGL_WIDTH,
                size.0 as i32,
                EGL_HEIGHT,
                size.1 as i32,
                EGL_NONE,
            ];
            let surface = (egl.create_pbuffer_surface)(display, config, pbuffer.as_ptr());
            if surface.is_null() {
                return Err(egl.fail("eglCreatePbufferSurface"));
            }
            let context_attrs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
            let context = (egl.create_context)(
                display,
                config,
                std::ptr::null_mut(),
                context_attrs.as_ptr(),
            );
            if context.is_null() {
                return Err(egl.fail("eglCreateContext"));
            }
            if (egl.make_current)(display, surface, surface, context) == 0 {
                return Err(egl.fail("eglMakeCurrent"));
            }
            Ok(PbufferSurface {
                egl,
                gles,
                display,
                surface,
                context,
                size,
            })
        }
    }
}

impl Surface for PbufferSurface {
    fn make_current(&mut self) -> Result<(), GfxError> {
        let ok = unsafe {
            (self.egl.make_current)(self.display, self.surface, self.surface, self.context)
        };
        match ok {
            0 => Err(self.egl.fail("eglMakeCurrent")),
            _ => Ok(()),
        }
    }

    fn window_size(&self) -> (u32, u32) {
        self.size
    }

    fn swap(&mut self) -> Result<(), GfxError> {
        unsafe { gl::Finish() };
        Ok(())
    }

    fn proc_address(&self, name: &str) -> *const c_void {
        let Ok(c) = CString::new(name) else {
            return std::ptr::null();
        };
        let exported = unsafe { self.gles.get::<unsafe extern "C" fn()>(name.as_bytes()) };
        match exported {
            Ok(f) => *f as *const c_void,
            Err(_) => unsafe { (self.egl.get_proc_address)(c.as_ptr()) },
        }
    }
}

impl Drop for PbufferSurface {
    fn drop(&mut self) {
        unsafe {
            (self.egl.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.egl.terminate)(self.display);
        }
    }
}
