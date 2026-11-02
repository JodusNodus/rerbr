//! Frame pacing for the input latency (ours): starts frames no faster than the GPU finishes them, so a frame's
//! commands never wait behind the last frame's and the input it read is drawn at once. `--pacing off` turns it off.
//!
//! A GPU-bound loop otherwise runs ahead of the GPU until the swapchain holds it back: a frame read its input, then
//! waited a frame or two in the queue (sway, FIFO, 55 fps at 4K: 37 ms from the present to the screen, 18 of them
//! the frame's own GPU time). The pacer keeps the time between frame starts at the GPU's frame time and [`IDLE`] more:
//! after each frame it compares when the GPU finished the frame before with when this one was submitted (the gap,
//! negative while the GPU was still busy) and moves the period by [`GAIN`] of the gap's miss of [`IDLE`]. A CPU-bound
//! loop leaves a wide gap, and the period falls to nothing. The swapchain's own waits (the display's refresh under
//! fifo above the GPU's rate) are not paced.
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

pub struct Pacer {
    /// The least time between frame starts, ms (0: unpaced).
    period: f64,
    /// The frames submitted and not yet paired with the end of the one before: their number and when.
    submitted: VecDeque<(u64, Instant)>,
    /// When the GPU finished frames, by number, from the polling thread, and those not yet paired.
    done: Receiver<(u64, Instant)>,
    send: Sender<(u64, Instant)>,
    finished: VecDeque<(u64, Instant)>,
    /// The next frame's number.
    next: u64,
    stop: Arc<AtomicBool>,
}

impl Pacer {
    /// Starts the polling thread on `device`.
    pub fn new(device: &wgpu::Device) -> Pacer {
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
        Pacer { period: 0.0, submitted, done, send, finished, next: 0, stop }
    }

    /// The frame's commands went to `queue` at `at`.
    pub fn submitted(&mut self, queue: &wgpu::Queue, at: Instant) {
        let (frame, send) = (self.next, self.send.clone());
        queue.on_submitted_work_done(move || {
            let _ = send.send((frame, Instant::now()));
        });
        self.submitted.push_back((frame, at));
        self.next += 1;
    }

    /// The least time between frame starts, after the frames finished since the last call.
    pub fn period(&mut self) -> Duration {
        self.finished.extend(self.done.try_iter());
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
        Duration::from_secs_f64(self.period / 1000.0)
    }
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

    #[test]
    fn the_gap_counts_before_and_after() {
        let a = Instant::now();
        let b = a + Duration::from_millis(3);
        assert_eq!((ms_between(a, b), ms_between(b, a)), (3.0, -3.0));
    }
}
