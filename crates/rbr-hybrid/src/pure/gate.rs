//! The image's gate in check builds (the `poison` feature): the memory [`super::Pure::image`] and
//! [`super::Pure::image_mut`] hand out, and `Pure::mem`. While the image is stale (the typed state held across steps
//! changed since the last lower), an access to a held word through it panics: a reader between steps that keeps a clone
//! of the image, or reads `Pure::mem` without the gate, fails instead of reading an old word.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use rbr_physics::mem::{Backend, Mem};

pub(super) struct Gate {
    inner: Mem,
    /// The held words, sorted by start, with their owners' names.
    held: RwLock<Vec<(u32, u32, String)>>,
    stale: AtomicBool,
}

impl Gate {
    pub(super) fn new(inner: Mem) -> Self {
        Gate { inner, held: RwLock::default(), stale: AtomicBool::new(false) }
    }

    /// The held words of the state just lifted.
    pub(super) fn hold(&self, owned: &[(Range<u32>, String)]) {
        let mut held: Vec<_> = owned.iter().map(|(r, n)| (r.start, r.end, n.clone())).collect();
        held.sort_unstable_by_key(|&(lo, _, _)| lo);
        *self.held.write().unwrap() = held;
    }

    pub(super) fn set_stale(&self, stale: bool) {
        self.stale.store(stale, Ordering::Relaxed);
    }

    fn check(&self, at: u32, len: usize) {
        if !self.stale.load(Ordering::Relaxed) {
            return;
        }
        let end = at as u64 + len as u64;
        let held = self.held.read().unwrap();
        let k = held.partition_point(|&(_, hi, _)| u64::from(hi) <= at as u64);
        if let Some((lo, _, name)) = held.get(k).filter(|&&(lo, _, _)| u64::from(lo) < end) {
            panic!("stale image read at {at:#x}: {name} ({lo:#x}) is held across steps; read through Pure::image");
        }
    }
}

impl Backend for Gate {
    fn read(&self, at: u32, out: &mut [u8]) -> bool {
        self.check(at, out.len());
        self.inner.try_read_into(at, out)
    }

    fn write(&self, at: u32, bytes: &[u8]) {
        self.check(at, bytes.len());
        self.inner.clone().map(at, bytes);
    }
}
