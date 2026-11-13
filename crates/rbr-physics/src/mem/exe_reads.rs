//! The 1.02 exe's data words a run reads before anything writes them, for refreshing the embedded copy
//! (`rbr_formats::exe_data`, `scripts/gen-exe-data.py`). Only with the `exe-reads` feature; otherwise every function
//! is empty. The pages report each word access (`read`, `write`) between [`start`] and the end of the process.

/// The exe's `.rdata` and `.data`: the addresses tracked.
#[cfg(feature = "exe-reads")]
const SPAN: std::ops::Range<u32> = 0x0072_9000..0x007e_b000;

#[cfg(feature = "exe-reads")]
mod on {
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::OnceLock;

    use super::SPAN;

    const WRITTEN: u8 = 1;
    const READ: u8 = 2;

    static ON: AtomicBool = AtomicBool::new(false);

    /// Per word of [`SPAN`]: 0 untouched, else whether its first access was a write or a read.
    fn words() -> &'static [AtomicU8] {
        static WORDS: OnceLock<Vec<AtomicU8>> = OnceLock::new();
        WORDS.get_or_init(|| SPAN.step_by(4).map(|_| AtomicU8::new(0)).collect())
    }

    pub fn start() {
        words();
        ON.store(true, Ordering::Relaxed);
    }

    #[inline]
    pub fn first(at: u32, how: u8) {
        if ON.load(Ordering::Relaxed) && SPAN.contains(&at) {
            let word = &words()[((at - SPAN.start) / 4) as usize];
            let _ = word.compare_exchange(0, how, Ordering::Relaxed, Ordering::Relaxed);
        }
    }

    pub fn read(at: u32) {
        first(at, READ)
    }

    pub fn write(at: u32) {
        first(at, WRITTEN)
    }

    pub fn dump(path: &std::path::Path) -> std::io::Result<()> {
        let read = words().iter().zip(SPAN.step_by(4)).filter(|(w, _)| w.load(Ordering::Relaxed) == READ);
        std::fs::write(path, read.map(|(_, at)| format!("{at:08x}\n")).collect::<String>())
    }
}

/// Starts tracking (after the exe's words are mapped).
pub fn start() {
    #[cfg(feature = "exe-reads")]
    on::start();
}

/// The word holding `at` is read.
#[inline(always)]
pub(super) fn read(_at: u32) {
    #[cfg(feature = "exe-reads")]
    on::read(_at);
}

/// The word holding `at` is written.
#[inline(always)]
pub(super) fn write(_at: u32) {
    #[cfg(feature = "exe-reads")]
    on::write(_at);
}

/// Writes the words read first, one hex address a line, to `$RBR_EXE_READS` when it is set.
pub fn dump() -> std::io::Result<()> {
    #[cfg(feature = "exe-reads")]
    if let Some(path) = std::env::var_os("RBR_EXE_READS") {
        return on::dump(path.as_ref());
    }
    Ok(())
}
