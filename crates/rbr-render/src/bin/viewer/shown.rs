//! When each presented frame went on screen, for the pacer ([`crate::pacer`]) and `RBR_LATENCY`: Wayland's
//! presentation feedback ([`wayland`]) or the Metal drawable's presented time ([`metal`]); none elsewhere.

use std::time::{Duration, Instant};

#[cfg(all(unix, not(target_vendor = "apple")))]
mod wayland;
#[cfg(all(unix, not(target_vendor = "apple")))]
pub use wayland::{Shown, VSYNC, ZERO_COPY};

#[cfg(target_os = "macos")]
mod metal;
#[cfg(target_os = "macos")]
pub use metal::{Shown, VSYNC, ZERO_COPY};

/// One frame's feedback: on screen at `at` with the feedback's flags ([`VSYNC`], [`ZERO_COPY`]), or discarded.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(any(target_os = "macos", all(unix, not(target_vendor = "apple")))), allow(dead_code))]
pub enum Feedback {
    Shown { at: Instant, flags: u32 },
    Discarded,
}

/// The instant of `at` ns on a clock that reads `now_ns` now: in the past.
#[allow(dead_code)]
fn instant(at: u64, now_ns: u64) -> Instant {
    Instant::now() - Duration::from_nanos(now_ns.saturating_sub(at))
}

/// No feedback where the window is not a Wayland or macOS one.
#[cfg(not(any(target_os = "macos", all(unix, not(target_vendor = "apple")))))]
mod none {
    pub const VSYNC: u32 = 1;
    pub const ZERO_COPY: u32 = 8;
    pub struct Shown;
    impl Shown {
        pub fn new(_: &winit::window::Window, _: &wgpu::Surface) -> Result<Shown, String> {
            Err("not a Wayland or macOS window".into())
        }
        pub fn ask(&mut self, _: u64) {}
        pub fn take(&mut self) -> Vec<(u64, super::Feedback)> {
            Vec::new()
        }
    }
}
#[cfg(not(any(target_os = "macos", all(unix, not(target_vendor = "apple")))))]
pub use none::{Shown, VSYNC, ZERO_COPY};
