//! Frame pacing for the input latency (ours): starts frames no faster than the GPU finishes them, so a frame's
//! commands never wait behind the last frame's and the input it read is drawn at once, and under fifo no faster than
//! the screen takes them. `--pacing off` turns it off.
//!
//! A GPU-bound loop otherwise runs ahead of the GPU until the swapchain holds it back: a frame read its input, then
//! waited a frame or two in the queue (sway, FIFO, 55 fps at 4K: 37 ms from the present to the screen, 18 of them
//! the frame's own GPU time). The pacer keeps the time between frame starts at the GPU's frame time and [`IDLE`] more:
//! after each frame it compares when the GPU finished the frame before with when this one was submitted (the gap,
//! negative while the GPU was still busy) and moves the period by [`GAIN`] of the gap's miss of [`IDLE`]. A CPU-bound
//! loop leaves a wide gap, and the period falls to nothing.
//!
//! A refresh-bound loop under fifo (the GPU idle between frames, the loop held by the swapchain) otherwise runs ahead
//! of the screen: frames waited a refresh or two in the swapchain's queue (M3 Pro, 120 Hz, a light drive: 30 ms from
//! the present to the screen, 14 of them the GPU's). With the presentation feedback ([`crate::shown`]) and the
//! display's refresh, [`Refresh`] locks the frames to the refreshes instead.
//!
//! When the GPU finished a frame comes from `Queue::on_submitted_work_done`, which a thread polling the device every
//! [`POLL`] runs.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// The GPU's idle time a frame the pacer aims for, ms: a margin for the frame times' spread.
const IDLE: f64 = 0.5;
/// The share of the gap's miss of [`IDLE`] that moves the period, a frame.
const GAIN: f64 = 0.1;
/// The longest period, ms.
const MOST: f64 = 100.0;
/// How often the thread polls the device for finished frames.
const POLL: Duration = Duration::from_micros(250);
/// The frames whose times and feedback the refresh pacing keeps.
const KEEP: usize = 60;
/// How much the refresh pacing's allowance for the compositor falls a frame on time, ms.
const DECAY: f64 = 0.001;
/// How far from a whole number of refreshes apart two frames on screen show a variable refresh, ms.
const OFF_GRID: f64 = 1.0;

pub struct Pacer {
    /// The least time between frame starts, ms (0: unpaced).
    period: f64,
    /// The frames submitted and not yet paired with the end of the one before: their number and when.
    submitted: VecDeque<(u64, Instant)>,
    /// When the GPU finished frames, by number, from the polling thread, and those not yet paired.
    done: Receiver<(u64, Instant)>,
    send: Sender<(u64, Instant)>,
    finished: VecDeque<(u64, Instant)>,
    /// The last frame's start (its surface texture came), which the period counts from.
    started: Option<Instant>,
    refresh: Option<Refresh>,
    stop: Arc<AtomicBool>,
}

impl Pacer {
    /// Starts the polling thread on `device`. `refresh`, the display's refresh in ms, paces a refresh-bound loop
    /// (under fifo, with the presentation feedback).
    pub fn new(device: &wgpu::Device, refresh: Option<f64>) -> Pacer {
        let (send, done) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (device, stopped) = (device.clone(), stop.clone());
        thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let _ = device.poll(wgpu::PollType::Poll);
                thread::sleep(POLL);
            }
        });
        let (submitted, finished) = (VecDeque::new(), VecDeque::new());
        let refresh = refresh.map(Refresh::new);
        Pacer { period: 0.0, submitted, done, send, finished, started: None, refresh, stop }
    }

    /// Frame `frame`, started at `started`, went to `queue` at `at`.
    pub fn submitted(&mut self, queue: &wgpu::Queue, frame: u64, started: Instant, at: Instant) {
        let send = self.send.clone();
        queue.on_submitted_work_done(move || {
            let _ = send.send((frame, Instant::now()));
        });
        self.submitted.push_back((frame, at));
        self.started = Some(started);
        if let Some(refresh) = &mut self.refresh {
            refresh.submitted(frame, started, at);
        }
    }

    /// Frame `frame` went on screen at `at`.
    pub fn shown(&mut self, frame: u64, at: Instant) {
        if let Some(refresh) = &mut self.refresh {
            refresh.shown(frame, at);
        }
    }

    /// The least time between frame starts, after the frames finished since the last call: the GPU's, else under
    /// fifo the refreshes'.
    pub fn period(&mut self) -> Duration {
        for (frame, at) in self.done.try_iter() {
            if let Some(refresh) = &mut self.refresh {
                refresh.done(frame, at);
            }
            self.finished.push_back((frame, at));
        }
        // Each submitted frame whose last frame is done: the GPU's gap before it.
        while let Some(&(frame, at)) = self.submitted.front() {
            let Some(last) = frame.checked_sub(1) else {
                self.submitted.pop_front();
                continue;
            };
            while self.finished.front().is_some_and(|&(done, _)| done < last) {
                self.finished.pop_front();
            }
            let Some(&(done, when)) = self.finished.front() else { break };
            self.submitted.pop_front();
            if done == last {
                self.period = next_period(self.period, ms_between(when, at));
            }
        }
        let gpu = Duration::from_secs_f64(self.period / 1000.0);
        let Some(refresh) = &mut self.refresh else { return gpu };
        if self.period > 0.0 {
            // A skip asked for under the GPU's pace does not hold when the refreshes take over.
            (refresh.plan, refresh.skip) = (None, false);
            return gpu;
        }
        match (refresh.start(Instant::now()), self.started) {
            (Some(start), Some(started)) => start.saturating_duration_since(started),
            _ => gpu,
        }
    }
}

/// Pacing a refresh-bound loop under fifo: frames locked to the refreshes they go on screen at, one a refresh, each
/// started so its GPU work is done [`IDLE`] and `extra` before the refresh it aims at. A frame on screen later than its
/// refresh that started late (its surface texture waited: the swapchain was full) or found the one before in its place
/// (the swapchain's queue) makes the next skip a refresh; one later with its place free (the compositor's time) adds a
/// quarter refresh to `extra`, which falls by [`DECAY`] a frame on time. It stands aside while the CPU's or the GPU's
/// time does not fit in a refresh (locked to every other one, the GPU would idle and the GPU's pace never take over),
/// and for good once two frames in a row go on screen over [`OFF_GRID`] from whole refreshes apart (a variable refresh,
/// which follows the frames: sway's adaptive sync, up to 8 ms off, against 0.4 at most with a fixed refresh).
struct Refresh {
    /// The display's refresh, ms.
    tick: f64,
    /// Frames submitted and not yet done on the GPU (number, start, submit), and the last done (number, when).
    pending: VecDeque<(u64, Instant, Instant)>,
    last_done: Option<(u64, Instant)>,
    /// The last [`KEEP`] frames' CPU (start to submit), GPU and both times, ms: the start to GPU done of a frame the
    /// GPU takes at once, as it does once paced (a GPU-bound frame's wait behind the last does not count).
    cpu: VecDeque<f64>,
    gpu: VecDeque<f64>,
    lead: VecDeque<f64>,
    /// The last [`KEEP`] frames' target refreshes and whether they started late, their times on screen, and the
    /// newest time on screen (the grid).
    targets: VecDeque<(u64, Instant, bool)>,
    screen: VecDeque<(u64, Instant)>,
    anchor: Option<Instant>,
    /// The display's refresh varies: no grid to lock to.
    variable: bool,
    extra: f64,
    skip: bool,
    /// The first frame planned after the last correction: the feedback of those before it does not count.
    settle: u64,
    /// The next frame's target refresh and start once planned, the last frame's target, and the next frame's number.
    plan: Option<(Instant, Instant)>,
    last: Option<Instant>,
    next: u64,
}

impl Refresh {
    fn new(tick: f64) -> Refresh {
        let (pending, cpu, gpu, lead, targets, screen) = Default::default();
        Refresh {
            tick,
            pending,
            last_done: None,
            cpu,
            gpu,
            lead,
            targets,
            screen,
            anchor: None,
            variable: false,
            extra: 0.0,
            skip: false,
            settle: 0,
            plan: None,
            last: None,
            next: 0,
        }
    }

    fn submitted(&mut self, frame: u64, started: Instant, at: Instant) {
        self.pending.push_back((frame, started, at));
        let plan = self.plan.take();
        if let Some((target, start)) = plan {
            keep(&mut self.targets, (frame, target, started > start + ms(1.0)));
        }
        self.last = plan.map(|(target, _)| target);
        self.next = frame + 1;
    }

    fn done(&mut self, frame: u64, at: Instant) {
        while self.pending.front().is_some_and(|p| p.0 < frame) {
            self.pending.pop_front();
        }
        if let Some((_, started, submitted)) = self.pending.pop_front_if(|p| p.0 == frame) {
            // The GPU started on it when it came or when it finished the frame before.
            let free = match self.last_done {
                Some((last, when)) if last + 1 == frame => submitted.max(when),
                _ => submitted,
            };
            let (cpu, gpu) = (ms_between(started, submitted), ms_between(free, at));
            keep(&mut self.cpu, cpu);
            keep(&mut self.gpu, gpu);
            keep(&mut self.lead, cpu + gpu);
        }
        self.last_done = Some((frame, at));
    }

    fn shown(&mut self, frame: u64, at: Instant) {
        let before = self.screen.iter().find(|s| s.0 + 1 == frame).map(|s| s.1);
        if let Some(gap) = before.map(|before| ms_between(before, at)).filter(|_| !self.variable) {
            if (gap - self.tick * (gap / self.tick).round()).abs() > OFF_GRID {
                eprintln!("viewer: the display's refresh varies (frames {gap:.2} ms apart): refresh pacing off");
                self.variable = true;
            }
        }
        keep(&mut self.screen, (frame, at));
        self.anchor = Some(self.anchor.map_or(at, |anchor| anchor.max(at)));
        let Some(&(_, target, started_late)) = self.targets.iter().find(|t| t.0 == frame) else { return };
        let half = ms(self.tick / 2.0);
        if frame < self.settle {
            return;
        }
        if at <= target + half {
            self.extra = (self.extra - DECAY).max(0.0);
            return;
        }
        if started_late || before.is_some_and(|before| before + half >= target) {
            self.skip = true;
        } else {
            self.extra = (self.extra + self.tick / 4.0).min(2.0 * self.tick);
        }
        self.settle = self.next;
    }

    /// When the next frame starts: early enough before its target refresh, the first on the grid it can make after
    /// the last frame's (one more after a frame queued). None before the first feedback, with a variable refresh, or
    /// while a frame's CPU or GPU time does not fit in a refresh.
    fn start(&mut self, now: Instant) -> Option<Instant> {
        let anchor = self.anchor.filter(|_| !self.variable)?;
        let tick = self.tick;
        if self.lead.is_empty() || quantile(&self.cpu, 0.95).max(quantile(&self.gpu, 0.95)) + IDLE > tick {
            self.plan = None;
            return None;
        }
        let ahead = quantile(&self.lead, 0.95) + IDLE + self.extra;
        if self.plan.is_none() {
            // The grid: whole refreshes from the newest frame on screen, the first a frame can make, half a refresh or
            // more after the last's.
            let after_last = self.last.map_or(f64::MIN, |last| ms_between(anchor, last) + tick / 2.0);
            let slots = ((ms_between(anchor, now) + ahead).max(after_last) / tick).ceil().max(0.0);
            let target = anchor + ms(tick * (slots + if std::mem::take(&mut self.skip) { 1.0 } else { 0.0 }));
            self.plan = Some((target, target - ms(ahead)));
        }
        self.plan.map(|(_, start)| start)
    }
}

/// `value` kept with the last [`KEEP`] before it.
fn keep<T>(values: &mut VecDeque<T>, value: T) {
    if values.len() == KEEP {
        values.pop_front();
    }
    values.push_back(value);
}

/// The `q` quantile of `values` (0 for none).
fn quantile(values: &VecDeque<f64>, q: f64) -> f64 {
    let mut sorted: Vec<f64> = values.iter().copied().collect();
    sorted.sort_by(f64::total_cmp);
    sorted.get(((sorted.len().saturating_sub(1)) as f64 * q).round() as usize).copied().unwrap_or(0.0)
}

/// `ms` milliseconds.
fn ms(ms: f64) -> Duration {
    Duration::from_secs_f64(ms / 1000.0)
}

impl Drop for Pacer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Ms from `a` to `b`, negative when `b` is earlier.
fn ms_between(a: Instant, b: Instant) -> f64 {
    match b.checked_duration_since(a) {
        Some(d) => d.as_secs_f64() * 1000.0,
        None => -(a - b).as_secs_f64() * 1000.0,
    }
}

/// The period after a frame submitted `gap` ms after the GPU finished the one before (negative: it was still busy).
fn next_period(period: f64, gap: f64) -> f64 {
    (period + GAIN * (IDLE - gap)).clamp(0.0, MOST)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The period a loop settles at whose GPU takes `gpu` ms a frame and whose CPU `cpu`, from unpaced.
    fn settle(gpu: f64, cpu: f64) -> f64 {
        let (mut period, mut gpu_free) = (0.0, 0.0f64);
        let mut start = 0.0;
        for _ in 0..500 {
            // The frame starts at its period (or when the CPU is free), submits after its CPU time, and the GPU
            // runs it after the last one.
            let submit = start + cpu;
            let gap = submit - gpu_free;
            gpu_free = gpu_free.max(submit) + gpu;
            period = next_period(period, gap);
            start = (start + period).max(submit);
        }
        period
    }

    #[test]
    fn a_gpu_bound_loop_is_paced_at_the_gpus_frame_time_and_a_cpu_bound_one_not() {
        assert!((settle(18.0, 10.0) - 18.5).abs() < 0.1, "{}", settle(18.0, 10.0));
        assert_eq!(settle(5.0, 10.0), 0.0);
    }

    /// A refresh pacing at 60 Hz whose frames took 4 ms of CPU and 6 of GPU, the last on screen at `t0`.
    fn refresh_at(t0: Instant) -> Refresh {
        let mut refresh = Refresh::new(16.0);
        for frame in 0..3 {
            let started = t0 + ms(16.0 * frame as f64);
            refresh.submitted(frame, started, started + ms(4.0));
            refresh.done(frame, started + ms(10.0));
        }
        refresh.shown(2, t0);
        refresh
    }

    #[test]
    fn a_refresh_bound_frame_aims_its_gpu_work_at_the_next_refresh_it_can_make() {
        let t0 = Instant::now();
        let mut refresh = refresh_at(t0);
        // 10 ms from start to GPU done and IDLE ahead: the refresh at 16 ms is the first after 1 + 10.5 ms.
        let start = refresh.start(t0 + ms(1.0)).unwrap();
        assert_eq!(start, t0 + ms(16.0 - 10.5));
        assert_eq!(refresh.start(t0 + ms(3.0)), Some(start), "planned once");
        refresh.submitted(3, start, start + ms(4.0));
        // The next a refresh on, even when it could start sooner.
        assert_eq!(refresh.start(t0 + ms(6.0)), Some(t0 + ms(32.0 - 10.5)));
    }

    #[test]
    fn a_frame_late_behind_the_last_skips_and_one_late_alone_starts_earlier() {
        let t0 = Instant::now();
        let mut refresh = refresh_at(t0);
        let start = refresh.start(t0 + ms(1.0)).unwrap();
        refresh.submitted(3, start, start + ms(4.0));
        // Frame 3 aimed at 16 ms; on at 32 with frame 2 there at 16 (moved for the test): queued, so the next skips.
        refresh.screen.clear();
        refresh.shown(2, t0 + ms(16.0));
        refresh.shown(3, t0 + ms(32.0));
        assert!(refresh.skip);
        // Frame 4 would aim at 32, where frame 3 is: it skips to 48.
        assert_eq!(refresh.start(t0 + ms(17.0)), Some(t0 + ms(48.0 - 10.5)));
        // Late with its place free: the compositor's, so frames start a quarter refresh earlier.
        let mut refresh = refresh_at(t0);
        let start = refresh.start(t0 + ms(1.0)).unwrap();
        refresh.submitted(3, start, start + ms(4.0));
        refresh.shown(3, t0 + ms(32.0));
        assert_eq!((refresh.skip, refresh.extra), (false, 4.0));
        // A frame planned before the correction does not count.
        refresh.submitted(4, start, start);
        refresh.targets.push_back((4, t0 + ms(32.0), false));
        refresh.shown(4, t0 + ms(48.0));
        assert_eq!(refresh.extra, 4.0);
    }

    #[test]
    fn it_stands_aside_for_a_frame_over_a_refresh_and_a_variable_refresh() {
        let t0 = Instant::now();
        let mut refresh = refresh_at(t0);
        refresh.gpu.push_back(20.0);
        refresh.gpu.push_back(20.0);
        assert_eq!(refresh.start(t0 + ms(1.0)), None, "the GPU's p95 over a refresh");
        let mut refresh = refresh_at(t0);
        refresh.shown(3, t0 + ms(17.5));
        assert_eq!(refresh.start(t0 + ms(18.0)), None, "1.5 ms off the grid");
    }

    #[test]
    fn the_gap_counts_before_and_after() {
        let a = Instant::now();
        let b = a + Duration::from_millis(3);
        assert_eq!((ms_between(a, b), ms_between(b, a)), (3.0, -3.0));
    }
}
