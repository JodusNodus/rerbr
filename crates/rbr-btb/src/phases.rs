//! Load-phase timing (ours, for measuring): with `RBR_LOAD_PHASES=1` each [`mark`] prints one line to stderr with the
//! wall time since the last mark and since the first, and the process's resident and peak memory (`/proc/self/status`,
//! Linux only). [`add`] sums the time of calls inside a loop, printed under the next mark. Off, every call is a
//! check of one flag.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

struct State {
    first: Instant,
    last: Instant,
    sums: BTreeMap<&'static str, (Duration, u32)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RBR_LOAD_PHASES").is_some())
}

/// `VmRSS` and `VmHWM` in MiB.
fn memory() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        let line = status.lines().find(|l| l.starts_with(name));
        line.and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()).unwrap_or(0) / 1024
    };
    (field("VmRSS:"), field("VmHWM:"))
}

/// The start of a timed call for [`add`] (`None` when off).
pub fn start() -> Option<Instant> {
    on().then(Instant::now)
}

/// Adds the time since `since` to the sum `name`.
pub fn add(name: &'static str, since: Option<Instant>) {
    let Some(since) = since else { return };
    let mut state = STATE.lock().unwrap();
    let state = state.get_or_insert_with(|| State { first: since, last: since, sums: BTreeMap::new() });
    let sum = state.sums.entry(name).or_default();
    sum.0 += since.elapsed();
    sum.1 += 1;
}

/// Ends the phase `name`: prints its line, then the sums [`add`] made in it.
pub fn mark(name: &str) {
    if !on() {
        return;
    }
    let now = Instant::now();
    let mut state = STATE.lock().unwrap();
    let state = state.get_or_insert_with(|| State { first: now, last: now, sums: BTreeMap::new() });
    let (rss, peak) = memory();
    eprintln!(
        "load-phase {name:<44} {:8.3} s  at {:8.3} s  rss {rss:6} MiB  peak {peak:6} MiB",
        (now - state.last).as_secs_f64(),
        (now - state.first).as_secs_f64()
    );
    for (sum, (time, calls)) in std::mem::take(&mut state.sums) {
        eprintln!("load-phase   of which {sum:<35} {:8.3} s  in {calls} calls", time.as_secs_f64());
    }
    state.last = Instant::now();
}
