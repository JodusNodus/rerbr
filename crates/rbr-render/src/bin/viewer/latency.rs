//! The live drive's input latency (`RBR_LATENCY`, ours): when a change of the steering is sent, first stepped, seen
//! by the frame loop, drawn, presented and done on the GPU, and each drawn frame's own times, printed as a summary
//! when the window closes.
//!
//! `RBR_LATENCY=script` (or `script:MS`) replaces the steering with a square wave of ±[`SCRIPT_STEER`] that flips
//! every 500 ms (or MS): each flip is due at a known time, as a device event would arrive, so the first stage holds
//! the wait for the input thread's poll. Any other value times the frames only. The presentation feedback
//! ([`crate::shown`], on Wayland and macOS) gives when each frame went on screen.
//!
//! The input thread sends the scripted steering (`window::feed`), so a probe's stages are the input sent, its first
//! step (the reader thread had the step record of a frame that took it), the frame loop seeing that step, and the
//! frame that drew it; the counts are the step records'.

use crate::shown::{self, Feedback};
use rbr_render::live::Step;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// The scripted steering's size.
const SCRIPT_STEER: f32 = 0.5;
/// Step records kept, at most (an hour at 720 a second).
const MOST_STEPS: usize = 3_000_000;

/// The scripted steering: ±[`SCRIPT_STEER`], flipping every `period` from `started`.
#[derive(Debug, Clone, Copy)]
pub struct Script {
    started: Instant,
    period: Duration,
}

impl Script {
    fn flips(&self, at: Instant) -> u32 {
        (at.saturating_duration_since(self.started).as_nanos() / self.period.as_nanos()) as u32
    }

    /// The steering at `at`.
    pub fn steer(&self, at: Instant) -> f32 {
        if self.flips(at).is_multiple_of(2) {
            -SCRIPT_STEER
        } else {
            SCRIPT_STEER
        }
    }
}
/// Flips kept, at most (a summary over a long run keeps the first).
const MOST_PROBES: usize = 10_000;
const MOST_FRAMES: usize = 100_000;

/// One flip of the scripted steering and when each stage first had it.
#[derive(Debug, Clone, Copy)]
struct Probe {
    due: Instant,
    steer: f32,
    /// When the input thread sent it, and the input's number.
    sent: Instant,
    seq: u32,
    received: Option<Instant>,
    taken: Option<Instant>,
    /// The frame that drew it: its number, its commands submitted, presented, and its GPU work done.
    frame: Option<usize>,
    submitted: Option<Instant>,
    presented: Option<Instant>,
    gpu_done: Option<Instant>,
}

/// A drawn frame's times: the surface's texture asked for and had, the input read, the commands submitted and the
/// frame presented, and the step it shows.
#[derive(Debug, Clone, Copy)]
struct Frame {
    acquire: (Instant, Instant),
    read: Instant,
    step: Option<u32>,
    submitted: Instant,
    presented: Instant,
}

/// The measurement of one drive.
pub struct Latency {
    script: Option<Script>,
    /// Each step record: when it came, its step and its force.
    steps: Vec<(Instant, u32, f32)>,
    probes: Vec<Probe>,
    frames: Vec<Frame>,
    /// When the GPU finished a probe's frame ([`Latency::watch_gpu`]).
    gpu: (Sender<Instant>, Receiver<Instant>),
    /// The presentation feedback per frame number.
    feedback: HashMap<u64, Feedback>,
    /// This frame's times so far.
    acquire: (Instant, Instant),
    read: Instant,
    /// The first frame of the frames' summary: the race's go ([`Latency::go`]), else the first.
    from: usize,
}

impl Latency {
    /// The measurement `RBR_LATENCY` asks for, if any.
    pub fn from_env() -> Option<Latency> {
        let value = std::env::var("RBR_LATENCY").ok()?;
        let now = Instant::now();
        let script = value.strip_prefix("script").map(|rest| Script {
            started: now,
            period: Duration::from_millis(rest.strip_prefix(':').and_then(|ms| ms.parse().ok()).unwrap_or(500)),
        });
        eprintln!("viewer: timing the live input (RBR_LATENCY={value})");
        Some(Latency {
            script,
            steps: Vec::new(),
            probes: Vec::new(),
            frames: Vec::new(),
            gpu: mpsc::channel(),
            feedback: HashMap::new(),
            acquire: (now, now),
            read: now,
            from: 0,
        })
    }

    /// The race's go: the frames' summary starts at the next frame (the loading and the start left out).
    pub fn go(&mut self) {
        self.from = self.frames.len();
    }

    /// A frame began by asking for the surface's texture at `start` and had it at `had`.
    pub fn acquired(&mut self, start: Instant, had: Instant) {
        self.acquire = (start, had);
    }

    /// The scripted steering, for the input thread.
    pub fn script(&self) -> Option<Script> {
        self.script
    }

    /// Opens a probe when `steer`, sent at `at` as input `seq`, is a flip.
    fn flip(&mut self, script: Script, at: Instant, steer: f32, seq: u32) {
        let flips = script.flips(at);
        if flips > 0 && self.probes.last().is_none_or(|p| p.steer != steer) && self.probes.len() < MOST_PROBES {
            self.probes.push(Probe {
                due: script.started + script.period * flips,
                steer,
                sent: at,
                seq,
                frame: None,
                received: None,
                taken: None,
                submitted: None,
                presented: None,
                gpu_done: None,
            });
        }
    }

    /// A frame's input: the input thread's sends since the last call (when, the input's number, its steering) open
    /// the probes, the step records since date each probe's first step, and the newest
    /// step the frame shows takes them.
    pub fn input(&mut self, sends: Vec<(Instant, u32, f32)>, steps: Vec<(Instant, Step)>, newest: Option<Step>) {
        let now = Instant::now();
        self.read = now;
        if let Some(script) = self.script {
            for (at, seq, steer) in sends {
                self.flip(script, at, steer, seq);
            }
        }
        for (at, step) in &steps {
            for probe in self.probes.iter_mut().rev().take_while(|p| p.received.is_none()) {
                if probe.seq <= step.seq {
                    probe.received = Some(*at);
                }
            }
        }
        let room = MOST_STEPS.saturating_sub(self.steps.len());
        self.steps.extend(steps.into_iter().take(room).map(|(at, s)| (at, s.step, s.force)));
        if let (Some(probe), Some(newest)) = (self.probes.last_mut(), newest) {
            if probe.taken.is_none() && probe.seq <= newest.seq {
                probe.taken = Some(now);
            }
        }
    }

    /// The frame showing `step` was submitted and presented as the frame numbered as many as were drawn before it,
    /// with the feedback that came since the last; whether it is the first to draw the open probe's step (live play
    /// shows the newest step seen), when the caller calls [`Latency::watch_gpu`].
    pub fn drawn(&mut self, submitted: Instant, step: Option<u32>, feedback: &[(u64, Feedback)]) -> bool {
        let presented = Instant::now();
        self.feedback.extend(feedback.iter().copied());
        if let (Ok(done), Some(probe)) =
            (self.gpu.1.try_recv(), self.probes.iter_mut().rev().find(|p| p.submitted.is_some()))
        {
            probe.gpu_done = Some(done);
        }
        if self.frames.len() < MOST_FRAMES {
            let read = self.read;
            self.frames.push(Frame { acquire: self.acquire, read, step, submitted, presented });
        }
        match self.probes.last_mut() {
            Some(probe) if probe.submitted.is_none() && probe.taken.is_some() => {
                (probe.submitted, probe.presented) = (Some(submitted), Some(presented));
                probe.frame = Some(self.frames.len() - 1);
                true
            }
            _ => false,
        }
    }

    /// Notes when the GPU is done with the work submitted so far, without holding the frames back: a thread polls
    /// the device every 0.2 ms until the queue's callback has run.
    pub fn watch_gpu(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let (send, done) = (self.gpu.0.clone(), Arc::new(AtomicBool::new(false)));
        let flag = done.clone();
        queue.on_submitted_work_done(move || {
            let _ = send.send(Instant::now());
            flag.store(true, Ordering::Release);
        });
        let device = device.clone();
        thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                let _ = device.poll(wgpu::PollType::Poll);
                thread::sleep(Duration::from_micros(200));
            }
        });
    }

    /// The summary: per stage of the flips, ms from the flip being due (median, p95), and the frames' own times.
    pub fn summary(&self) -> String {
        let ms = |a: Instant, b: Instant| (b.saturating_duration_since(a)).as_secs_f64() * 1000.0;
        let line = |name: &str, values: Vec<f64>| {
            let (p50, p95, p99, max) = quantiles(values.clone());
            format!("\n  {name:<34} n {:>5}  p50 {p50:6.2}  p95 {p95:6.2}  p99 {p99:6.2}  max {max:6.2}", values.len())
        };
        let done: Vec<&Probe> = self.probes.iter().filter(|p| p.gpu_done.is_some()).collect();
        let from_due = |stage: fn(&Probe) -> Option<Instant>| {
            done.iter().filter_map(|p| Some(ms(p.due, stage(p)?))).collect::<Vec<_>>()
        };
        let between = |a: fn(&Probe) -> Option<Instant>, b: fn(&Probe) -> Option<Instant>| {
            done.iter().filter_map(|p| Some(ms(a(p)?, b(p)?))).collect::<Vec<_>>()
        };
        let mut out =
            format!("live latency: {} flips, {} drawn; ms from the flip being due:", self.probes.len(), done.len());
        if !done.is_empty() {
            out += &line("input sent (input thread)", from_due(|p| Some(p.sent)));
            out += &line("first step (reader thread)", from_due(|p| p.received));
            out += &line("step seen (frame loop)", from_due(|p| p.taken));
            out += &line("frame submitted", from_due(|p| p.submitted));
            out += &line("frame presented", from_due(|p| p.presented));
            out += &line("frame done on the GPU", from_due(|p| p.gpu_done));
            out += &line("frame on screen", done.iter().filter_map(|p| self.on_screen(p.frame?, p.due)).collect());
            out += "\n stage by stage:";
            out += &line("sent -> first step (pipes, clock)", between(|p| Some(p.sent), |p| p.received));
            out += &line("first step -> seen", between(|p| p.received, |p| p.taken));
            out += &line("taken -> submitted", between(|p| p.taken, |p| p.submitted));
            out += &line("submitted -> presented", between(|p| p.submitted, |p| p.presented));
            out += &line("submitted -> GPU done", between(|p| p.submitted, |p| p.gpu_done));
        }
        let (from, frames) = (self.from, &self.frames[self.from..]);
        let since = if from > 0 { " from the go" } else { "" };
        out += &format!("\n frames ({}{since}), ms:", frames.len());
        out += &line("period", frames.windows(2).map(|w| ms(w[0].acquire.0, w[1].acquire.0)).collect());
        out += &line("acquire wait", frames.iter().map(|f| ms(f.acquire.0, f.acquire.1)).collect());
        out += &line("acquired -> input read", frames.iter().map(|f| ms(f.acquire.1, f.read)).collect());
        out += &line("input read -> submitted", frames.iter().map(|f| ms(f.read, f.submitted)).collect());
        out += &line("submitted -> presented", frames.iter().map(|f| ms(f.submitted, f.presented)).collect());
        out += &line("input read -> presented", frames.iter().map(|f| ms(f.read, f.presented)).collect());
        out += &line(
            "presented -> on screen",
            (0..frames.len()).filter_map(|i| self.on_screen(from + i, frames[i].presented)).collect(),
        );
        if let Some(base) = frames.first().map(|f| f.read) {
            let timed = (0..frames.len()).map(|i| (ms(base, frames[i].read), self.on_screen(from + i, base)));
            out += &line("any input -> on screen", any_input(&timed.collect::<Vec<_>>()));
            // The shown step's age on screen: from its step record until the frame showing it went on screen.
            let aged: Vec<Option<f64>> = (0..frames.len())
                .map(|i| {
                    let made = self.step_at(frames[i].step?)?;
                    Some(self.on_screen(from + i, base)? - ms(base, made))
                })
                .collect();
            out += &line("shown step -> on screen", aged.iter().flatten().copied().collect());
            out += &line("judder (age change, shown frames)", judder(&aged));
        }
        out += &self.screen_counts();
        out += &self.step_counts();
        out
    }
}

impl Latency {
    /// When the step record of `step` came, if kept.
    fn step_at(&self, step: u32) -> Option<Instant> {
        let i = self.steps.partition_point(|s| s.1 < step);
        self.steps.get(i).filter(|s| s.1 == step).map(|s| s.0)
    }

    /// Ms from `from` until frame `frame` went on screen, if the feedback said.
    fn on_screen(&self, frame: usize, from: Instant) -> Option<f64> {
        let Some(&Feedback::Shown { at, .. }) = self.feedback.get(&(frame as u64)) else { return None };
        Some(at.saturating_duration_since(from).as_secs_f64() * 1000.0)
    }

    /// The step records a second, the steps a record stands for (1 unless a frame woke
    /// late), per drawn frame, and how often the force the wheel gets changed a second.
    fn step_counts(&self) -> String {
        let (Some(first), Some(last)) = (self.steps.first(), self.steps.last()) else { return String::new() };
        let seconds = (last.0 - first.0).as_secs_f64().max(1e-9);
        let per_record = quantiles(self.steps.windows(2).map(|w| f64::from(w[1].1 - w[0].1)).collect());
        let changes = self.steps.windows(2).filter(|w| w[0].2 != w[1].2).count();
        format!(
            "\n  steps: {:.0}/s in {:.0} step records/s, steps per record p50 {:.0} max {:.0}, per drawn frame {:.1}; \
             force changes {:.0}/s, all given to the wheel",
            f64::from(last.1 - first.1) / seconds,
            self.steps.len() as f64 / seconds,
            per_record.0,
            per_record.3,
            f64::from(last.1 - first.1) / self.frames.len().max(1) as f64,
            changes as f64 / seconds,
        )
    }

    /// The feedback's counts: shown with vsync, without (torn), zero-copy (direct scan-out), and discarded.
    fn screen_counts(&self) -> String {
        if self.feedback.is_empty() {
            return String::new();
        }
        let count = |test: &dyn Fn(&Feedback) -> bool| self.feedback.values().filter(|f| test(f)).count();
        let flag = |bit| move |f: &Feedback| matches!(f, Feedback::Shown { flags, .. } if flags & bit != 0);
        // Metal does not say which frames were scanned out directly.
        let zero_copy = if cfg!(target_os = "macos") {
            String::new()
        } else {
            format!(", {} zero-copy", count(&flag(shown::ZERO_COPY)))
        };
        format!(
            "\n  feedback: {} shown ({} vsync, {} not vsync{zero_copy}), {} discarded",
            count(&|f| matches!(f, Feedback::Shown { .. })),
            count(&flag(shown::VSYNC)),
            count(&|f| matches!(f, Feedback::Shown { flags, .. } if flags & shown::VSYNC == 0)),
            count(&|f| *f == Feedback::Discarded),
        )
    }
}

/// The picture's latency for an input at any moment: from each whole ms of `frames`' span until the first frame that
/// read its input after it went on screen. `frames` are each frame's input read and when it went on screen (`None`:
/// not shown), ms on one clock, in order.
fn any_input(frames: &[(f64, Option<f64>)]) -> Vec<f64> {
    let shown: Vec<(f64, f64)> = frames.iter().filter_map(|&(read, screen)| Some((read, screen?))).collect();
    let (Some(first), Some(last)) = (shown.first(), shown.last()) else { return Vec::new() };
    let mut next = shown.iter().peekable();
    (first.0.ceil() as u64..=last.0.floor() as u64)
        .map(|t| t as f64)
        .filter_map(|t| {
            while next.peek().is_some_and(|(read, _)| *read < t) {
                next.next();
            }
            next.peek().map(|(_, screen)| screen - t)
        })
        .collect()
}

/// How much the shown step's age changes from one frame on screen to the next, ms: 0 when the picture moves on by as
/// much game time as the screen does. `ages` are each frame's age on screen (`None`: not shown), in order.
fn judder(ages: &[Option<f64>]) -> Vec<f64> {
    let shown: Vec<f64> = ages.iter().flatten().copied().collect();
    shown.windows(2).map(|w| (w[1] - w[0]).abs()).collect()
}

/// The median, 95th and 99th percentiles and largest of `values` (0 for none).
fn quantiles(mut values: Vec<f64>) -> (f64, f64, f64, f64) {
    values.sort_by(f64::total_cmp);
    let at =
        |q: f64| values.get(((values.len().saturating_sub(1)) as f64 * q).round() as usize).copied().unwrap_or(0.0);
    (at(0.5), at(0.95), at(0.99), at(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_waits_for_the_next_frame_shown() {
        // Frames read at 0, 10 (discarded) and 20 ms, on screen at 30 and 45 ms.
        let latency = any_input(&[(0.0, Some(30.0)), (10.0, None), (20.0, Some(45.0))]);
        assert_eq!(latency.len(), 21);
        assert_eq!((latency[0], latency[1], latency[10], latency[20]), (30.0, 44.0, 35.0, 25.0));
        assert!(any_input(&[(0.0, None)]).is_empty());
    }
}
