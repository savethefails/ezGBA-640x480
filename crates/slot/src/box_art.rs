//! Box art over the shelf: a picture of the selected cart's box (or its title screen) floating
//! in the space above the row, loaded around the selection instead of all at boot.
//!
//! Only the selected cart's is ever drawn, so only it and its neighbours either side are kept,
//! built on a thread of their own (through `art_cache`, so usually a file read rather than a
//! decode) and handed to the shelf as they arrive. Textures that fall out of range are reused
//! for the ones coming in. Loading all of it at boot cost a decode per cart before the first
//! frame and kept every picture on the GPU, whose memory the H700 shares with everything else.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use slot_gfx::{Compositor, TexId};
use slot_store::Cart;
use slot_ui::{box_art_at, box_art_space};

use crate::app::{App, SHELF_ROW_LOWER};
use crate::art_cache::{box_art, Art};

/// Carts either side of the selection whose box art is kept ready. Two covers a held
/// direction's repeat with the next one already up.
pub const AHEAD: i32 = 2;

/// A cart by shelf and place on it.
pub type Key = (usize, usize);

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Key, Cart)>,
    closed: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct BoxArtLoader {
    queue: Shared,
    built: Receiver<(Key, Option<Art>)>,
    /// Asked of the worker and not yet back, queued or being built.
    pending: HashSet<Key>,
    /// What was wanted when the queue was last set, so it is only rebuilt when that changes.
    asked: Vec<Key>,
    free: Vec<TexId>,
}

impl BoxArtLoader {
    pub fn spawn(root: PathBuf) -> Self {
        let queue: Shared = Arc::default();
        let (outbox, built) = mpsc::channel();
        let worker = queue.clone();
        let spawned = thread::Builder::new()
            .name("slot-box-art".into())
            .spawn(move || work(&worker, &outbox, &root));
        if let Err(e) = spawned {
            eprintln!("slot: box art: worker thread failed to start: {e}");
        }
        BoxArtLoader {
            queue,
            built,
            pending: HashSet::new(),
            asked: Vec::new(),
            free: Vec::new(),
        }
    }

    /// Once a frame: places whatever has been built, lets go of what has fallen out of range
    /// and asks for what has come into it. Never waits.
    pub fn sync(&mut self, app: &mut App, compositor: &mut Compositor) {
        while let Ok((key, art)) = self.built.try_recv() {
            self.place(app, compositor, key, art);
        }
        let wants = app.box_art_wants();
        if wants != self.asked {
            self.free.extend(app.shed_box_art(&wants));
            self.requeue(app, &wants);
            self.asked = wants;
        }
    }

    /// `sync`, then waits for the selected cart's box art: at boot, so the first frame of the
    /// shelf is already the right picture rather than the wallpaper for a moment.
    pub fn sync_selected(&mut self, app: &mut App, compositor: &mut Compositor) {
        self.sync(app, compositor);
        let Some(&first) = self.asked.first() else {
            return;
        };
        while self.pending.contains(&first) {
            let Ok((key, art)) = self.built.recv() else {
                return;
            };
            self.place(app, compositor, key, art);
        }
    }

    fn requeue(&mut self, app: &App, wants: &[Key]) {
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        // What is still queued is dropped and asked again in the new order: a held direction
        // otherwise leaves a trail of pictures being built for carts long since passed.
        for (key, _) in queue.jobs.drain(..) {
            self.pending.remove(&key);
        }
        for &key in wants {
            if self.pending.contains(&key) || app.box_art_loaded(key) {
                continue;
            }
            if let Some(cart) = app.box_art_cart(key) {
                queue.jobs.push_back((key, cart.clone()));
                self.pending.insert(key);
            }
        }
        wake.notify_all();
    }

    fn place(&mut self, app: &mut App, compositor: &mut Compositor, key: Key, art: Option<Art>) {
        self.pending.remove(&key);
        // Built for a cart the selection has already moved away from.
        let Some((rgba, w, h)) = art.filter(|_| self.asked.contains(&key)) else {
            return;
        };
        let tex = match self.free.pop() {
            Some(tex) => {
                compositor.update_texture(tex, w, h, &rgba);
                tex
            }
            None => compositor.create_texture(w, h, &rgba),
        };
        self.free.extend(app.set_box_art(key, (tex, w, h)));
    }
}

impl Drop for BoxArtLoader {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        wake.notify_all();
    }
}

fn work(queue: &Shared, outbox: &Sender<(Key, Option<Art>)>, root: &Path) {
    let (lock, wake) = &**queue;
    loop {
        let (key, cart) = {
            let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if q.closed {
                    return;
                }
                if let Some(job) = q.jobs.pop_front() {
                    break job;
                }
                q = wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        if outbox
            .send((key, box_art(root, &cart, bound(&cart))))
            .is_err()
        {
            return;
        }
    }
}

/// The space over a selected `cart`, in whole pixels: what its box art is fitted to.
pub fn bound(cart: &Cart) -> (u32, u32) {
    let (_, _, w, h) = box_art_space(cart.platform, SHELF_ROW_LOWER);
    (w as u32, h as u32)
}

/// Where box art of `w` by `h` for a selected `cart` stands, as `(x, y)`.
pub fn place(cart: &Cart, w: u32, h: u32) -> (f32, f32) {
    box_art_at(box_art_space(cart.platform, SHELF_ROW_LOWER), w, h)
}
