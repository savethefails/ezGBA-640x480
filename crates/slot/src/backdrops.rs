//! Per-cart backdrops, loaded around the selection instead of all at boot.
//!
//! A backdrop is a whole screen of RGBA, 1.2 MB, and only the selected cart's is ever drawn.
//! Loading every one at boot cost a full-size decode per cart before the first frame and kept
//! all of them on the GPU for good: a hundred carts was over a hundred megabytes of a memory
//! the H700 shares with everything else. So only the selection and its neighbours either side
//! are kept, built on a thread of their own (through `art_cache`, so usually a file read rather
//! than a decode) and handed to the shelf as they arrive. Textures that fall out of range are
//! reused for the ones coming in, since every backdrop is the same size.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use slot_gfx::{Compositor, TexId, OUT_H, OUT_W};
use slot_store::Cart;

use crate::app::App;
use crate::art_cache::backdrop_art;

/// Carts either side of the selection whose backdrops are kept ready. Two covers a held
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

pub struct Backdrops {
    queue: Shared,
    built: Receiver<(Key, Option<Vec<u8>>)>,
    /// Asked of the worker and not yet back, queued or being built.
    pending: HashSet<Key>,
    /// What was wanted when the queue was last set, so it is only rebuilt when that changes.
    asked: Vec<Key>,
    free: Vec<TexId>,
}

impl Backdrops {
    pub fn spawn(root: PathBuf) -> Self {
        let queue: Shared = Arc::default();
        let (outbox, built) = mpsc::channel();
        let worker = queue.clone();
        let spawned = thread::Builder::new()
            .name("slot-backdrops".into())
            .spawn(move || work(&worker, &outbox, &root));
        if let Err(e) = spawned {
            eprintln!("slot: backdrops: worker thread failed to start: {e}");
        }
        Backdrops {
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
        let wants = app.backdrop_wants();
        if wants != self.asked {
            self.free.extend(app.shed_backdrops(&wants));
            self.requeue(app, &wants);
            self.asked = wants;
        }
    }

    /// `sync`, then waits for the selected cart's backdrop: at boot, so the first frame of the
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
        // otherwise leaves a trail of backdrops being built for carts long since passed.
        for (key, _) in queue.jobs.drain(..) {
            self.pending.remove(&key);
        }
        for &key in wants {
            if self.pending.contains(&key) || app.backdrop_loaded(key) {
                continue;
            }
            if let Some(cart) = app.backdrop_cart(key) {
                queue.jobs.push_back((key, cart.clone()));
                self.pending.insert(key);
            }
        }
        wake.notify_all();
    }

    fn place(
        &mut self,
        app: &mut App,
        compositor: &mut Compositor,
        key: Key,
        art: Option<Vec<u8>>,
    ) {
        self.pending.remove(&key);
        // Built for a cart the selection has already moved away from.
        let Some(rgba) = art.filter(|_| self.asked.contains(&key)) else {
            return;
        };
        let tex = match self.free.pop() {
            Some(tex) => {
                compositor.update_texture(tex, OUT_W, OUT_H, &rgba);
                tex
            }
            None => compositor.create_texture(OUT_W, OUT_H, &rgba),
        };
        self.free.extend(app.set_backdrop(key, tex));
    }
}

impl Drop for Backdrops {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        wake.notify_all();
    }
}

fn work(queue: &Shared, outbox: &Sender<(Key, Option<Vec<u8>>)>, root: &Path) {
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
        if outbox.send((key, backdrop_art(root, &cart))).is_err() {
            return;
        }
    }
}
