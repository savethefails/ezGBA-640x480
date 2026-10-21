//! `slot.state` written on a thread of its own, for the settings that change while a game runs:
//! volume, brightness and mute. A write with its sync could hold the frame loop long enough to
//! stutter the game on every press. Only the newest state waiting is ever written, and every
//! other write of `slot.state` waits for this one first, so the two cannot land out of order.
//! From slot a02fcd5.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use slot_store::{write_slot_state, SlotState};

use crate::card_log::CardWrites;

#[derive(Default)]
struct Queue {
    next: Option<(PathBuf, SlotState)>,
    busy: bool,
    closed: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct StateWriter {
    shared: Shared,
    threaded: bool,
    card: Arc<CardWrites>,
}

impl StateWriter {
    /// A failed write raises the card alarm through `card`, as a write on the game's thread does.
    pub fn spawn(card: Arc<CardWrites>) -> Self {
        let shared: Shared = Arc::default();
        let worker = shared.clone();
        let flag = card.clone();
        let threaded = thread::Builder::new()
            .name("slot-state".into())
            .spawn(move || work(&worker, &flag))
            .is_ok();
        StateWriter {
            shared,
            threaded,
            card,
        }
    }

    /// Queues `state`, replacing whatever was waiting. Written here instead when there is no
    /// thread to write it.
    pub fn save(&self, root: &Path, state: &SlotState) {
        if !self.threaded {
            return write(root, state, &self.card);
        }
        let (lock, wake) = &*self.shared;
        let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
        q.next = Some((root.to_path_buf(), state.clone()));
        wake.notify_all();
    }

    /// Returns once nothing is waiting and nothing is being written.
    pub fn flush(&self) {
        let (lock, wake) = &*self.shared;
        let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
        while q.next.is_some() || q.busy {
            q = wake.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for StateWriter {
    fn drop(&mut self) {
        self.flush();
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        wake.notify_all();
    }
}

fn work(shared: &Shared, card: &CardWrites) {
    let (lock, wake) = &**shared;
    loop {
        let (root, state) = {
            let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(job) = q.next.take() {
                    q.busy = true;
                    break job;
                }
                if q.closed {
                    return;
                }
                q = wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        write(&root, &state, card);
        lock.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
        wake.notify_all();
    }
}

fn write(root: &Path, state: &SlotState, card: &CardWrites) {
    let written = write_slot_state(root, state);
    if let Err(e) = &written {
        eprintln!("slot: slot.state: {e}");
    }
    card.wrote("slot.state", &written);
}
