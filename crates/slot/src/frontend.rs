//! Everything the binary does with a compositor except own one. The window is the only
//! difference between the host and the device, so it is the only thing left above this.

use std::time::{Duration, Instant};

use slot_gfx::{Compositor, Draw, TexId, OUT_H, OUT_W};
use slot_input::{InputSource, Millis};
use slot_power::{Platform, Power};
// Aliased because `slot_power::Platform` above is already `Platform` here and is a different
// thing entirely: that one is the machine slot is running on, this one is the machine a shelf's
// cartridges were made for.
use slot_store::{format_stamp, Pico8Bezel, Platform as CartPlatform};
use slot_ui::{
    arrows_hint_face, badge_face, bezel_face, cart_face, cart_face_with, cart_shadow, chip_face,
    chip_shadow_face, date_time_text, gb_cart_shadow, hhmm, hint_face, icon_face, mark_face,
    menu_face, photo_face, pico8_cart_shadow, quick_caret_face, quick_label_face,
    quick_legend_faces, quick_value_face, set_clock_hint_face, snes_cart_shadow, socket_face,
    sticker_face, title_face, toast_face, wallpaper_face, word_face, CartFace, GbShell, Icon,
    LinkBadge, PowerChoice, QuickMenuFaces, QuickRow, QuickValue, StickerFields, Toast, UndoFace,
    ALERT_PX, BOLT_PX, HUD_ICON_PX, HUD_INK, LEGEND,
};

use crate::app::{App, LinkRow, Phase};
use crate::art_cache::label_art;
use crate::box_art::BoxArtLoader;
use crate::build_info::Build;
use crate::face_builder::FaceBuilder;
use crate::link_art_builder::LinkArtBuilder;
use crate::link_screen::{LinkSprites, Sprite};
use crate::link_start::{LinkFail, LinkStep};
use crate::session::Session;
use crate::wallpaper;

/// How long a dark panel waits before the machine actually stops. The dark is immediate —
/// the lid or the button kills the backlight on the edge — but the device is still running
/// flat out behind it at 400-700 mA, so this is the window in which the user might come
/// straight back, not a power saving.
///
/// Three minutes, and then the device powers off rather than sleeping. It cannot wake itself
/// from a sleep — the RTC alarm never fires on this board — so a standby would be a leak with
/// no end, and a power off is the honest version of putting it down.
const DOZE_TIMEOUT: Duration = Duration::from_secs(180);

/// Amber. The only warning colour in the tree, and the reason it is not the HUD's ink: a
/// refusal that looks like a volume glyph is a refusal nobody reads as one.
const ALERT_INK: [u8; 3] = [0xf0, 0xb4, 0x3c];

pub struct Frontend {
    session: Session,
    /// The last frame before the screen was covered by the power menu, a doze or the shutdown
    /// screen: what the next boot shows. See `boot_picture`.
    scene: Option<Vec<u8>>,
    /// Whether the previous frame was covered, so the frame before the cover is read once,
    /// on the edge, and never while playing.
    covered: bool,
    /// Whether anything has been composed yet: before that the offscreen target holds nothing.
    composed: bool,
    /// The faces `UiFaces` builds off the boot path, until they arrive. See `poll_ui_faces`.
    ui_faces: Option<std::sync::mpsc::Receiver<UiFaces>>,
    start: Instant,
    last: Instant,
    draws: Vec<Draw>,
    /// One texture per ring slot, reused every time the switcher opens.
    polaroid_texes: Vec<TexId>,
    /// The top plate's line of type, re-rasterised whenever the selection moves.
    title_tex: Option<TexId>,
    /// Builds the open cart's faces off the frame loop.
    faces: FaceBuilder,
    /// Loads the box art around the selection off the frame loop.
    box_art: BoxArtLoader,
    /// Builds the link screen's artwork off the frame loop, once, at boot.
    link_art: LinkArtBuilder,
    /// Whether the link art has been uploaded and handed to `App` already.
    link_art_done: bool,
    /// The cart last asked for.
    core_asked: Option<String>,
    /// The open cart and its lid, and which cart they were built for.
    core_board_tex: Option<TexId>,
    core_lid_tex: Option<TexId>,
    core_built: Option<String>,
    /// The undo cap's label, which changes with what is on offer.
    undo_tex: Option<TexId>,
    switcher: Switcher,
    clocks: Clocks,
    about: AboutFace,
    quick_clock: QuickClock,
    greeting: GreetingFace,
}

/// The greeting's picture, and which frame is in it.
#[derive(Default)]
struct GreetingFace {
    tex: Option<TexId>,
    shown: Option<usize>,
}

/// Date & Time's value in the quick menu, grey and lit, and the text they were built for.
#[derive(Default)]
struct QuickClock {
    dim: Option<TexId>,
    lit: Option<TexId>,
    shown: String,
}

/// The about label, and what it was last built for. The gauge is the only thing on it that
/// moves, so the reading is what decides whether it is rebuilt.
#[derive(Default)]
struct AboutFace {
    tex: Option<TexId>,
    /// `None` is a board with no gauge, which is a different thing from not having built one
    /// yet — `tex` says that.
    battery: Option<u8>,
}

/// The clock screen's two faces and the shelf's one, with what each was last built for. The
/// picker's line changes under the caret; the shelf clock changes once a minute; the battery
/// percent changes whenever the reading does.
#[derive(Default)]
struct Clocks {
    line: Option<TexId>,
    /// Uploaded once, at boot: it never changes what it says.
    hint: Option<TexId>,
    shelf: Option<TexId>,
    picked: Option<String>,
    shown: String,
    battery: String,
    battery_tex: Option<TexId>,
    /// The letter in the slot's face, and the letter it was made for.
    letter: Option<char>,
    letter_tex: Option<TexId>,
}

/// What the switcher's textures were built for. The photos and the undo cap are per opening;
/// the title is per selection.
#[derive(Default)]
struct Switcher {
    open: bool,
    titled: Option<String>,
}

impl Frontend {
    pub fn boot(platform: Box<dyn Platform>) -> Self {
        Self::boot_with(platform, crate::audio::opened_sink())
    }

    /// `boot`, with the audio sink the device opened on a thread.
    pub fn boot_with(platform: Box<dyn Platform>, sink: Box<dyn crate::audio::AudioSink>) -> Self {
        let now = Instant::now();
        let box_art = BoxArtLoader::spawn(platform.root().to_path_buf());
        let mut session = Session::boot_with(platform.root().to_path_buf(), sink);
        crate::boot_time::mark("session");
        session
            .app_mut()
            .set_power(Power::new(platform, DOZE_TIMEOUT));
        Frontend {
            session,
            start: now,
            last: now,
            draws: Vec::new(),
            polaroid_texes: Vec::new(),
            title_tex: None,
            faces: FaceBuilder::spawn(),
            box_art,
            link_art: LinkArtBuilder::spawn(),
            link_art_done: false,
            core_asked: None,
            core_board_tex: None,
            core_lid_tex: None,
            core_built: None,
            undo_tex: None,
            switcher: Switcher::default(),
            clocks: Clocks::default(),
            about: AboutFace::default(),
            quick_clock: QuickClock::default(),
            greeting: GreetingFace::default(),
            scene: None,
            covered: false,
            composed: false,
            ui_faces: None,
        }
    }

    /// The app behind the screen, to look at.
    pub fn app(&self) -> &App {
        self.session.app()
    }

    /// Everything that never changes: the carts, the HUD glyphs and the key caps, waiting for
    /// the ones `upload_boot_faces` leaves to a thread. All of it needs a live context, so it
    /// happens after the compositor and not at boot.
    pub fn upload_faces(&mut self, compositor: &mut Compositor) {
        self.upload_boot_faces(compositor);
        if let Some(faces) = self.ui_faces.take().and_then(|rx| rx.recv().ok()) {
            self.place_ui_faces(compositor, faces);
        }
    }

    /// What the first frame can show, uploaded now; the menus, the link screen and the core
    /// picker are rasterised on a thread and uploaded by `poll_ui_faces` when they are ready,
    /// a few frames into the boot, before anyone can open one.
    pub fn upload_boot_faces(&mut self, compositor: &mut Compositor) {
        let app = self.session.app();
        let faces = build_cart_faces(app.root(), &app.carts().collect::<Vec<_>>())
            .into_iter()
            .map(|f| compositor.create_texture(f.w, f.h, &f.rgba))
            .collect();
        self.session.app_mut().set_faces(faces);
        // Before the first frame, so the panel goes from the bootloader's picture to the same
        // picture drawn by slot, with nothing between them.
        if self.session.app().resuming_at_boot() {
            if let Some(rgba) = self
                .session
                .app()
                .root()
                .and_then(crate::boot_picture::read_last_screen)
            {
                let tex = compositor.create_texture(OUT_W, OUT_H, &rgba);
                self.session.app_mut().set_boot_still(tex);
            }
        }
        self.box_art
            .sync_selected(self.session.app_mut(), compositor);
        let icons = Icon::ALL
            .iter()
            .map(|i| {
                let f = icon_face(*i, HUD_ICON_PX, HUD_INK);
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        self.session.app_mut().set_icon_faces(icons);
        // Its own upload rather than one of the HUD's: it is drawn on a cart, at its own
        // size, and in a warning colour the level glyphs have no business borrowing.
        let alert = icon_face(Icon::Alert, ALERT_PX, ALERT_INK);
        let alert = compositor.create_texture(alert.w, alert.h, &alert.rgba);
        self.session.app_mut().set_alert_face(alert);
        // The faces no first frame shows, rasterised on a thread while the boot goes on and
        // uploaded by `poll_ui_faces` on the first frame after they are ready. From slot 8a52c5d.
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("slot-ui-faces".into())
            .spawn(move || {
                let _ = tx.send(UiFaces::build());
            });
        match spawned {
            Ok(_) => self.ui_faces = Some(rx),
            Err(_) => self.place_ui_faces(compositor, UiFaces::build()),
        }
        let toasts = Toast::ALL
            .iter()
            .map(|t| {
                let f = toast_face(*t);
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        self.session.app_mut().set_toast_faces(toasts);
        let legend = legend_faces(compositor, &LEGEND);
        self.session.app_mut().set_legend_faces(legend);
        // The clock screen's one instruction, which never changes what it says. Uploaded here
        // with the other key caps, so moving the caret rasterises only the line above it.
        let hint = set_clock_hint_face();
        self.clocks.hint = Some(compositor.create_texture(hint.w, hint.h, &hint.rgba));
        let shadow = cart_shadow();
        let id = compositor.create_texture(shadow.w, shadow.h, &shadow.rgba);
        self.session.app_mut().set_cart_shadow(id);
        // One per Game Pak mould: the two shells' corners differ, and a shared backing was
        // showing through a dimmed cart at the corner where they disagree.
        let notched = gb_cart_shadow(GbShell::Notched);
        let notched = compositor.create_texture(notched.w, notched.h, &notched.rgba);
        let rounded = gb_cart_shadow(GbShell::Rounded);
        let rounded = compositor.create_texture(rounded.w, rounded.h, &rounded.rgba);
        self.session.app_mut().set_gb_cart_shadows(notched, rounded);
        let snes = snes_cart_shadow();
        let snes = compositor.create_texture(snes.w, snes.h, &snes.rgba);
        self.session.app_mut().set_snes_cart_shadow(snes);
        let pico8 = pico8_cart_shadow();
        let pico8 = compositor.create_texture(pico8.w, pico8.h, &pico8.rgba);
        self.session.app_mut().set_pico8_cart_shadow(pico8);
        // `draw_gauge` now draws the bolt beside the capsule, on the housing, in its own
        // reserved slot rather than over the fill. The housing tint was only ever needed to
        // hide the bolt inside the fill it sat on; out here it sits where every other HUD
        // glyph does, so it takes the same ink they do.
        let bolt = icon_face(Icon::Charging, BOLT_PX, HUD_INK);
        let bolt_id = compositor.create_texture(bolt.w, bolt.h, &bolt.rgba);
        self.session.app_mut().set_bolt_face(bolt_id);
        // One mark per shelf, in `Platform::ALL` order, beside the bolt because they are the
        // same kind of thing: a small tinted drawing that never changes. At boot and not on the
        // press that needs one, because each is an SVG through a rasteriser, which is the one
        // thing this device must never do on a frame.
        let marks = CartPlatform::ALL
            .iter()
            .map(|p| {
                let f = mark_face(*p);
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        self.session.app_mut().set_mark_faces(marks);
        // Every PICO-8 bezel, so the menu can switch between them without drawing on a frame.
        // Nearest, as pixel art at its own size has nothing to blend.
        let bezels = [Pico8Bezel::Quiet, Pico8Bezel::Palette, Pico8Bezel::Soft].map(|b| {
            let rgba = bezel_face(b).expect("a bezel that draws something");
            compositor.create_texture_nearest(OUT_W, OUT_H, &rgba)
        });
        self.session.app_mut().set_bezel_faces(bezels);
        self.upload_wallpaper(compositor);
    }

    /// Uploads the faces `UiFaces` built, once they are ready; called every frame and doing
    /// nothing after the first time.
    fn poll_ui_faces(&mut self, compositor: &mut Compositor) {
        let Some(rx) = &self.ui_faces else {
            return;
        };
        match rx.try_recv() {
            Ok(faces) => {
                self.ui_faces = None;
                self.place_ui_faces(compositor, faces);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.ui_faces = None,
        }
    }

    fn place_ui_faces(&mut self, compositor: &mut Compositor, f: UiFaces) {
        let mut tex = |c: &CartFace| compositor.create_texture(c.w, c.h, &c.rgba);
        let link_badges = f.link_badges.iter().map(&mut tex).collect();
        let sockets = f.sockets.iter().map(&mut tex).collect();
        let chips = f.chips.iter().map(&mut tex).collect();
        let blank = tex(&f.blank_chip);
        let shadow = tex(&f.chip_shadow);
        let mut line = |f: &UndoFace| (compositor.create_texture(f.w, f.h, &f.rgba), f.w, f.h);
        let shutdown = f.shutdown.iter().map(&mut line).collect();
        let quick = QuickMenuFaces {
            labels: f.quick_labels.iter().map(&mut line).collect(),
            values: f
                .quick_values
                .iter()
                .map(|pair| pair.each_ref().map(&mut line))
                .collect(),
            carets: f.quick_carets.each_ref().map(&mut line),
            legend: f.quick_legend.each_ref().map(|f| key(line(f))),
        };
        let core_legend = f.core_legend.iter().map(|f| key(line(f))).collect();
        let link_roles = f.link_roles.iter().map(&mut line).collect();
        let linked = line(&f.linked);
        let link_legend = f.link_legend.iter().map(|f| key(line(f))).collect();
        let link_steps = f.link_steps.iter().map(&mut line).collect();
        let link_fails = f.link_fails.iter().map(&mut line).collect();
        let app = self.session.app_mut();
        app.set_link_badge_faces(link_badges);
        app.set_core_part_faces(sockets, chips, blank, shadow);
        app.set_shutdown_faces(shutdown);
        app.set_quick_menu_faces(quick);
        app.set_core_legend_faces(core_legend);
        app.set_link_menu_faces(link_roles);
        app.set_link_linked_face(linked);
        app.set_link_legend_faces(link_legend);
        app.set_link_step_faces(link_steps);
        app.set_link_fail_faces(link_fails);
    }

    /// One decode, at boot. A card with no `Wallpapers`, no readable picture in it, or a
    /// picture the decoder will not take, gets the plain ground it had before.
    fn upload_wallpaper(&mut self, compositor: &mut Compositor) {
        let app = self.session.app();
        let seed = app.wall_secs().unsigned_abs();
        let Some(root) = app.root() else {
            return;
        };
        // The same picture as last time when this boot opens on it, so the shelf slot draws is
        // the shelf the bootloader just showed. Otherwise a new one, as every boot gets.
        let kept = app
            .remembered_wallpaper()
            .filter(|_| app.backlight_held())
            .map(|name| root.join("Wallpapers").join(name))
            .filter(|path| path.is_file());
        let Some(path) = kept.or_else(|| wallpaper::pick(root, seed)) else {
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let Some(rgba) = wallpaper_face(&path) else {
            return;
        };
        let id = compositor.create_texture(OUT_W, OUT_H, &rgba);
        self.session.app_mut().set_wallpaper(id);
        if let Some(name) = name {
            self.session.app_mut().set_wallpaper_name(name);
        }
    }

    /// One frame into the offscreen target and out to a surface of `window` pixels. The
    /// caller swaps: only it knows what presenting costs.
    pub fn render(&mut self, compositor: &mut Compositor, window: (u32, u32)) {
        self.compose(compositor);
        compositor.end_frame(window);
    }

    /// One frame into the offscreen target and no further: what `render` presents, and what a
    /// test with no window to present to reads back with `Compositor::read_frame`.
    pub fn compose(&mut self, compositor: &mut Compositor) {
        self.poll_ui_faces(compositor);
        // Before anything is drawn: the offscreen target still holds the previous frame, the
        // last one with nothing over it. One read on the edge, never one a frame.
        let covered = self.session.app().scene_covered();
        if covered && !self.covered && self.composed {
            // Marked as a start before it is kept: the boot partition and the card's copy both
            // get this one frame, so the picture the bootloader shows and the one slot carries
            // on drawing are the same to the pixel, pill and all.
            let mut frame = compositor.read_frame();
            let at = if self.session.app().seated_cart().is_some() {
                slot_ui::PillAt::Game
            } else {
                slot_ui::PillAt::Shelf
            };
            slot_ui::stamp_starting(&mut frame, at);
            self.scene = Some(frame);
        }
        self.covered = covered;
        self.composed = true;
        // Set every frame rather than on the edge: the grade is part of the final blit, so
        // it has to be right whether or not anything just changed it.
        compositor.set_blue_light(self.session.app().blue_light());
        compositor.set_shake(self.session.app().screen_shake());
        compositor.set_screen_power(self.session.app().screen_power());
        // Every frame rather than on the edge, for the same reason the grade and the power are:
        // the pass has to be told what to draw whether or not anything just changed it.
        slot_gfx::set_fit(self.session.app().fit());
        compositor.begin_frame();
        if let Some(frame) = self.session.frame() {
            compositor.upload_game(&frame, frame.size());
            crate::latency::drawn();
        }
        self.box_art.sync(self.session.app_mut(), compositor);
        sync_clock(self.session.app_mut(), compositor, &mut self.clocks);
        sync_about(self.session.app_mut(), compositor, &mut self.about);
        sync_greeting(self.session.app_mut(), compositor, &mut self.greeting);
        sync_quick_clock(self.session.app_mut(), compositor, &mut self.quick_clock);
        sync_core_picker(
            self.session.app_mut(),
            compositor,
            &self.faces,
            &mut self.core_asked,
            &mut self.core_board_tex,
            &mut self.core_lid_tex,
            &mut self.core_built,
        );
        if !self.link_art_done {
            if let Some(art) = self.link_art.take() {
                let mut up = |f: &slot_ui::CartFace| Sprite {
                    tex: compositor.create_texture(f.w, f.h, &f.rgba),
                    w: f.w,
                    h: f.h,
                };
                let sprites = LinkSprites {
                    port: up(&art.port),
                    plug_host: up(&art.plug_host),
                    plug_join: up(&art.plug_join),
                    adapter: up(&art.adapter),
                    arcs_right: [
                        up(&art.arcs_right[0]),
                        up(&art.arcs_right[1]),
                        up(&art.arcs_right[2]),
                    ],
                    arcs_left: [
                        up(&art.arcs_left[0]),
                        up(&art.arcs_left[1]),
                        up(&art.arcs_left[2]),
                    ],
                    clicks: up(&art.clicks),
                    arrow_left: up(&art.arrow_left),
                    arrow_right: up(&art.arrow_right),
                };
                self.session.app_mut().set_link_sprites(sprites);
                self.link_art_done = true;
            }
        }
        sync_switcher(
            self.session.app_mut(),
            compositor,
            Faces {
                pool: &mut self.polaroid_texes,
                title: &mut self.title_tex,
                undo: &mut self.undo_tex,
            },
            &mut self.switcher,
        );
        self.draws.clear();
        self.session.app().draw(&mut self.draws);
        compositor.draw_list(&self.draws);
    }

    /// Input and time, after the frame is on screen. The gesture windows expire on this
    /// whether or not anything was pressed, so it is called every frame.
    pub fn advance(&mut self, input: &mut dyn InputSource) {
        let now = self.now();
        let events = input.poll(now);
        self.session.feed(events, now);
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        self.session.update(dt);
    }

    fn now(&self) -> Millis {
        self.start.elapsed().as_millis() as Millis
    }

    /// Kicks the core on the buttons just fed and waits, at most `timeout`, for the frame it
    /// makes, so this refresh can draw it. See `Session::kick_and_wait`.
    pub fn kick_and_wait(&self, timeout: std::time::Duration) -> bool {
        self.session.kick_and_wait(timeout)
    }

    /// Frames to run ahead, as the theme set it at boot.
    pub fn runahead(&self) -> u8 {
        self.session.app().runahead()
    }

    /// The frame the next boot should open on, taken once.
    pub fn take_scene(&mut self) -> Option<Vec<u8>> {
        self.scene.take()
    }

    /// What a test reads the phase and the core's frame from.
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn powering_off(&self) -> bool {
        self.session.app().ready_to_power_off()
    }

    pub fn restarting(&self) -> bool {
        self.session.app().ready_to_restart()
    }

    pub fn restart(&mut self) {
        self.session.app_mut().restart();
    }

    /// The state was flushed on the edge that set `powering_off`, so there is nothing left to
    /// do but go.
    pub fn poweroff(&mut self) {
        self.session.app_mut().poweroff();
    }
}

/// A line of menu type per label, in the order they were handed over, each with the size it
/// was rastered at. Every menu on the device is drawn from a list shaped exactly like this,
/// so the four the in-game menu needs are built through one function rather than four copies
/// of the same three lines.
/// Every cart's face, in the order given. Each one needs its label art, scaled; with a content
/// root that comes from `art_cache`, so only a label new since the last boot is decoded. They
/// are built a share per core rather than one after another; only the compositor can mint a
/// texture, so that stays with the caller.
fn build_cart_faces(
    root: Option<&std::path::Path>,
    carts: &[&slot_store::Cart],
) -> Vec<slot_ui::CartFace> {
    let face = |c: &slot_store::Cart| match root {
        Some(root) => cart_face_with(c, label_art(root, c)),
        None => cart_face(c),
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let share = carts.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let builders: Vec<_> = carts
            .chunks(share)
            .map(|chunk| scope.spawn(move || chunk.iter().map(|c| face(c)).collect::<Vec<_>>()))
            .collect();
        builders
            .into_iter()
            .flat_map(|b| b.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    })
}

/// A screen's key caps, in the order the legend names them. None of them ever changes what
/// it says, so they are uploaded once and outlive every visit to that screen.
fn legend_faces(compositor: &mut Compositor, legend: &[(&str, &str)]) -> Vec<TexId> {
    legend
        .iter()
        .map(|(key, label)| {
            let f = hint_face(key, label);
            compositor.create_texture(f.w, f.h, &f.rgba)
        })
        .collect()
}

/// The switcher's textures, which outlive any one opening.
struct Faces<'a> {
    pool: &'a mut Vec<TexId>,
    title: &'a mut Option<TexId>,
    undo: &'a mut Option<TexId>,
}

/// Photos and the undo cap are built once per opening, on the way in, while the game is
/// already paused. Rebuilt each time rather than cached because the ring changes underneath
/// them. The title names the selection, so it follows a flick instead.
fn sync_switcher(app: &mut App, compositor: &mut Compositor, texes: Faces, state: &mut Switcher) {
    if !matches!(app.phase(), Phase::Polaroids { .. }) {
        state.open = false;
        return;
    }
    if !state.open {
        state.open = true;
        state.titled = None;
        let faces: Vec<_> = app.polaroid_entries().iter().map(photo_face).collect();
        let ids = faces
            .iter()
            .enumerate()
            .map(|(i, f)| match texes.pool.get(i) {
                Some(id) => {
                    compositor.update_texture(*id, f.w, f.h, &f.rgba);
                    *id
                }
                None => {
                    // Linear, as the live game's texture is: the scaler places its blend with the
                    // linear tap, and a nearest texture would give the still hard, uneven edges
                    // the game behind it does not have.
                    let id = compositor.create_texture(f.w, f.h, &f.rgba);
                    texes.pool.push(id);
                    id
                }
            })
            .collect();
        app.set_polaroid_faces(ids);

        // An offer can expire while the switcher is up but it cannot change into the other
        // kind, so the cap only has to be rasterised on the way in. Whether it is drawn at
        // all is the app's call.
        let label = app
            .undo_label()
            .map(|l| upload(compositor, texes.undo, hint_face("X", l)));
        app.set_undo_face(label);
    }
    if state.titled.as_deref() != app.polaroid_stamp() {
        state.titled = app.polaroid_stamp().map(str::to_string);
        let face = title_face(&app.polaroid_title(&format_stamp(app.wall_secs())));
        let id = upload(compositor, texes.title, face);
        app.set_polaroid_title_face(id);
    }
}

/// The picker is rasterised on every change under the caret, which is once per press. The
/// shelf clock follows the wall clock, so it is rebuilt when the minute turns and not on the
/// fifty nine seconds either side of it.
fn sync_clock(app: &mut App, compositor: &mut Compositor, clocks: &mut Clocks) {
    // The letter a jump along the row landed on, made once a letter.
    let letter = app.slot_letter();
    if letter != clocks.letter {
        clocks.letter = letter;
        if let Some(letter) = letter {
            let face = word_face(&letter.to_string());
            let w = face.w;
            let id = upload(compositor, &mut clocks.letter_tex, face);
            app.set_slot_letter_face(id, w);
        }
    }
    let picked = app.picker().map(|p| p.text());
    if picked != clocks.picked {
        clocks.picked = picked;
        // Only the line. The hint under it never changes what it says and was uploaded with the
        // other key caps at boot: the screen can be opened at any time from the quick menu, and
        // every press here is a rasterisation on the H700.
        if let (Some(face), Some(hint)) = (app.picker().map(|p| p.face()), clocks.hint) {
            let line = upload(compositor, &mut clocks.line, face);
            app.set_clock_faces(line, hint);
        }
    }
    let shown = hhmm(app.wall_secs());
    if shown != clocks.shown {
        let face = word_face(&shown);
        clocks.shown = shown;
        let w = face.w;
        let id = upload(compositor, &mut clocks.shelf, face);
        app.set_shelf_clock_face(id, w);
    }
    let battery_shown = app
        .battery()
        .map(|b| format!("{}%", b.percent))
        .unwrap_or_default();
    if battery_shown != clocks.battery {
        clocks.battery = battery_shown.clone();
        if !battery_shown.is_empty() {
            let face = word_face(&battery_shown);
            let w = face.w;
            let id = upload(compositor, &mut clocks.battery_tex, face);
            app.set_battery_percent_face(id, w);
        }
    }
}

/// Date & Time's value, in both inks so the bar can land on it without anything being rastered.
/// Built only while the quick menu is up, and then only when the minute has turned since it was
/// last built, as the shelf clock is: a clock nobody is looking at is not worth a rasterisation a
/// minute on the H700.
fn sync_quick_clock(app: &mut App, compositor: &mut Compositor, state: &mut QuickClock) {
    if app.quick_menu().is_none() {
        return;
    }
    let text = date_time_text(app.wall_secs());
    if text == state.shown {
        return;
    }
    let (dim, lit) = (
        quick_value_face(&text, false),
        quick_value_face(&text, true),
    );
    let (dim_size, lit_size) = ((dim.w, dim.h), (lit.w, lit.h));
    let dim = upload(compositor, &mut state.dim, dim);
    let lit = upload(compositor, &mut state.lit, lit);
    app.set_quick_clock_faces((dim, dim_size.0, dim_size.1), (lit, lit_size.0, lit_size.1));
    state.shown = text;
}

/// One PNG decode each time the greeting's clock reaches a new frame.
fn sync_greeting(app: &mut App, compositor: &mut Compositor, state: &mut GreetingFace) {
    let Some(i) = app.greeting_frame() else {
        state.shown = None;
        return;
    };
    if state.shown == Some(i) {
        return;
    }
    let Some(root) = app.root() else {
        return;
    };
    let path = crate::app::greeting_frame_path(root, i);
    state.shown = Some(i);
    let Some(rgba) = wallpaper_face(&path) else {
        return;
    };
    let id = upload_rgba(compositor, &mut state.tex, OUT_W, OUT_H, &rgba);
    app.set_greeting_face(id);
}

/// Built only once the screen is up: it is a 612 by 212 rasterisation and most sessions never
/// open it.
fn sync_about(app: &mut App, compositor: &mut Compositor, state: &mut AboutFace) {
    if !matches!(app.phase(), Phase::About) {
        return;
    }
    let battery = app.battery().map(|b| b.percent);
    if state.tex.is_some() && state.battery == battery {
        return;
    }
    state.battery = battery;
    let build = Build::current();
    let face = sticker_face(&StickerFields {
        battery,
        serial: &build.serial(),
        dirty_digit: build.dirty_digit(),
    });
    let id = upload(compositor, &mut state.tex, face);
    app.set_sticker_face(id);
}

/// The open cart's faces, asked for as soon as the caret lands on a cart and uploaded when the
/// worker hands them back, so they are normally on the GPU before START. The worker is the only
/// place they are built: rasterised on the frame loop, a board freezes the shelf for the better
/// part of half a second on the H700.
fn sync_core_picker(
    app: &mut App,
    compositor: &mut Compositor,
    builder: &FaceBuilder,
    asked: &mut Option<String>,
    board: &mut Option<TexId>,
    lid: &mut Option<TexId>,
    built: &mut Option<String>,
) {
    let highlighted = app.selected_stem().map(str::to_string);
    if highlighted.is_some() && *asked != highlighted {
        if let Some(cart) = app
            .carts()
            .find(|c| highlighted.as_deref() == Some(c.stem.as_str()))
        {
            builder.request(cart.clone());
        }
        *asked = highlighted.clone();
    }
    let Some(faces) = builder.take() else {
        return;
    };
    // A build for a cart the caret has since left is dropped; the one it is on is on its way.
    if highlighted.as_deref() != Some(faces.stem.as_str()) || *built == highlighted {
        return;
    }
    let board_id = upload_rgba(
        compositor,
        board,
        faces.board.w,
        faces.board.h,
        &faces.board.rgba,
    );
    let lid_id = upload_rgba(compositor, lid, faces.lid.w, faces.lid.h, &faces.lid.rgba);
    app.set_core_board_faces(board_id, lid_id);
    *built = Some(faces.stem);
}

fn upload(compositor: &mut Compositor, slot: &mut Option<TexId>, face: slot_ui::UndoFace) -> TexId {
    upload_rgba(compositor, slot, face.w, face.h, &face.rgba)
}

/// Into the slot's own texture if it has one, so the pool stops growing after the first time.
fn upload_rgba(
    compositor: &mut Compositor,
    slot: &mut Option<TexId>,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> TexId {
    match *slot {
        Some(id) => {
            compositor.update_texture(id, w, h, rgba);
            id
        }
        None => {
            let id = compositor.create_texture(w, h, rgba);
            *slot = Some(id);
            id
        }
    }
}

/// A key cap's texture and width, as the legends take them.
fn key((tex, w, _): (TexId, u32, u32)) -> (TexId, u32) {
    (tex, w)
}

/// The faces no first frame shows, rasterised on a thread at boot: the link badges, the
/// shutdown screen's words, the quick menu, the core picker's parts and legend, and the link
/// screen's lines and legend. Pixels only; `Frontend::place_ui_faces` uploads them. Moved out of
/// the boot path from slot 8a52c5d. The toasts stay on it: the card alarm is one of them.
struct UiFaces {
    link_badges: Vec<CartFace>,
    shutdown: Vec<UndoFace>,
    quick_labels: Vec<UndoFace>,
    quick_values: Vec<[UndoFace; 2]>,
    quick_carets: [UndoFace; 2],
    quick_legend: [UndoFace; 3],
    sockets: Vec<CartFace>,
    chips: Vec<CartFace>,
    blank_chip: CartFace,
    chip_shadow: CartFace,
    core_legend: Vec<UndoFace>,
    link_roles: Vec<UndoFace>,
    linked: UndoFace,
    link_legend: Vec<UndoFace>,
    link_steps: Vec<UndoFace>,
    link_fails: Vec<UndoFace>,
}

impl UiFaces {
    fn build() -> Self {
        UiFaces {
            link_badges: LinkBadge::FACES
                .iter()
                .map(|b| {
                    let (badge, ink) = (
                        b.badge().expect("a face has a glyph"),
                        b.colour().expect("and a colour"),
                    );
                    badge_face(badge, HUD_ICON_PX, ink)
                })
                .collect(),
            // One line per way the device can go down, in `PowerChoice::ALL` order.
            shutdown: PowerChoice::ALL
                .iter()
                .map(|c| {
                    menu_face(match c {
                        PowerChoice::Restart => "Restarting",
                        PowerChoice::PowerOff => "Powering Down",
                    })
                })
                .collect(),
            quick_labels: QuickRow::ALL.iter().map(|r| quick_label_face(*r)).collect(),
            quick_values: QuickValue::ALL
                .iter()
                .map(|v| [false, true].map(|lit| quick_value_face(v.text(), lit)))
                .collect(),
            quick_carets: [false, true].map(quick_caret_face),
            quick_legend: quick_legend_faces(),
            // In `Core::GBA` order: only a GBA cart opens.
            sockets: slot_store::Core::GBA
                .iter()
                .map(|c| socket_face(*c))
                .collect(),
            chips: slot_store::Core::GBA
                .iter()
                .map(|c| chip_face(Some(*c)))
                .collect(),
            blank_chip: chip_face(None),
            chip_shadow: chip_shadow_face(),
            // The way out first and the choice last, as the switcher's legend is ordered.
            core_legend: vec![
                hint_face("B", "Cancel"),
                arrows_hint_face("Swap"),
                hint_face("A", "Choose"),
            ],
            link_roles: LinkRow::ALL.iter().map(|r| menu_face(r.text())).collect(),
            linked: menu_face("Linked"),
            // In `LinkLegend::ALL` order, which is how `App` finds each one.
            link_legend: vec![
                hint_face("B", "Cancel"),
                hint_face("SELECT", "Mode"),
                arrows_hint_face("Swap"),
                hint_face("A", "Link"),
                hint_face("A", "OK"),
                hint_face("B", "Back"),
                hint_face("A", "End Link"),
            ],
            link_steps: LinkStep::ALL.iter().map(|s| menu_face(s.line())).collect(),
            link_fails: LinkFail::SHOWN
                .iter()
                .map(|f| menu_face(f.line()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The thread builds one face for every entry the app's setters index by, each with ink in
    /// it, so moving them off the boot path left none out.
    #[test]
    fn the_faces_built_off_the_boot_path_are_all_there() {
        let f = UiFaces::build();
        let counts = [
            (f.link_badges.len(), LinkBadge::FACES.len(), "link badges"),
            (f.shutdown.len(), PowerChoice::ALL.len(), "shutdown lines"),
            (f.quick_labels.len(), QuickRow::ALL.len(), "quick menu rows"),
            (
                f.quick_values.len(),
                QuickValue::ALL.len(),
                "quick menu values",
            ),
            (f.sockets.len(), slot_store::Core::GBA.len(), "sockets"),
            (f.chips.len(), slot_store::Core::GBA.len(), "chips"),
            (f.link_roles.len(), LinkRow::ALL.len(), "link roles"),
            (f.link_steps.len(), LinkStep::ALL.len(), "link steps"),
            (f.link_fails.len(), LinkFail::SHOWN.len(), "link failures"),
        ];
        for (built, wanted, what) in counts {
            assert_eq!(built, wanted, "{what}");
        }
        let inked = |rgba: &[u8]| rgba.chunks(4).any(|p| p[3] > 0);
        let lines = f
            .shutdown
            .iter()
            .chain(&f.quick_labels)
            .chain(f.quick_values.iter().flatten())
            .chain(&f.quick_carets)
            .chain(&f.quick_legend)
            .chain(&f.core_legend)
            .chain(&f.link_roles)
            .chain([&f.linked])
            .chain(&f.link_legend)
            .chain(&f.link_steps)
            .chain(&f.link_fails);
        for face in lines {
            assert!(inked(&face.rgba), "a face came out blank");
        }
        for face in f.link_badges.iter().chain(&f.sockets).chain(&f.chips) {
            assert!(inked(&face.rgba), "a face came out blank");
        }
    }
}
