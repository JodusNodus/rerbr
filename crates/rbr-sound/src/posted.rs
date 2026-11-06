//! The commands the frame loop posts for the mixer ([`Fed`]): a queue of their own, which the audio callback takes
//! before each block it renders, so that posting never waits for a block's mix (the mixer's lock is held for all of
//! it). Every command still lands between the same two blocks: one posted while a block renders applies before the
//! next, as one waiting for the lock did.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::device::Render;
use crate::mixer::Mixer;
use crate::voice::Command;

/// Commands posted and not taken past this many (no callback runs: a device that stopped) are applied by the poster.
pub(crate) const MOST: usize = 1 << 14;

/// The mixer and the commands posted for it.
pub(crate) struct Fed {
    pub mixer: Mixer,
    posted: Arc<Mutex<Vec<Command>>>,
    /// The last commands taken, cleared: its capacity goes back to the queue (no allocation per block).
    taken: Vec<Command>,
}

impl Fed {
    /// `mixer` fed from a new queue, and the queue.
    pub fn new(mixer: Mixer) -> (Fed, Arc<Mutex<Vec<Command>>>) {
        let posted = Arc::new(Mutex::new(Vec::new()));
        (Fed { mixer, posted: Arc::clone(&posted), taken: Vec::new() }, posted)
    }

    /// Applies the commands posted so far, in order.
    pub fn catch_up(&mut self) {
        std::mem::swap(&mut *lock(&self.posted), &mut self.taken);
        self.mixer.apply(&self.taken);
        self.taken.clear();
    }
}

impl Render for Fed {
    fn render(&mut self, out: &mut [[f32; 2]]) {
        self.catch_up();
        self.mixer.render(out);
    }
}

/// The queue's lock (a poisoned one still holds whole commands).
pub(crate) fn lock(posted: &Mutex<Vec<Command>>) -> MutexGuard<'_, Vec<Command>> {
    posted.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
