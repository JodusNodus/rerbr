//! `steer_speed <physics.bin>`: CSV `step,steering,speed,faded_speed` per frame of an FRM1 recording, for the
//! steering-response fit (`harness/steer_fit.py`).
//!
//! `steering` is control word 4 (the wheel's fraction of lock, -1..1). `speed` is the car's forward speed
//! (`CCar` +0x15f8, `-MB560[1]`), clamped at 0 as `CCar::Vf28x` does. `faded_speed` is `E16Filter::NotifyA`'s result
//! (`rbr_gameplay::filter::notify_a`), with the velocity taken in the body frame (`R^T v`, forward is body -Y) as
//! the vector it reads (assumed to be that frame's x and y; not checked against the original's MB560).

use rbr_recording::pose::car_pose;
use rbr_recording::{read_f32, Frames};
use std::io::Write;

const FORWARD_SPEED: usize = 0x15f8;
const SIN_60: f32 = 0.866_025_4;

fn notify_a(speed: f32, [x, y]: [f32; 2]) -> f32 {
    let length = x.hypot(y);
    if 0.01 < length && y / length < 0.0 {
        let across = (x / length / SIN_60).abs().min(1.0);
        return (1.0 - across) * speed;
    }
    0.0
}

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("usage: steer_speed <physics.bin>")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    writeln!(out, "step,steering,speed,faded_speed").map_err(|e| e.to_string())?;
    for frame in Frames::new(&bytes) {
        let frame = frame.map_err(|e| format!("{path}: {e}"))?;
        let (Some(pose), Some(forward)) = (car_pose(frame.car), read_f32(frame.car, FORWARD_SPEED)) else { continue };
        let speed = forward.max(0.0);
        // R^T v: the rows of R are the columns of R^T, so rotate the velocity by the conjugate.
        let [x, y, z, w] = pose.orientation;
        let inverse = rbr_recording::pose::CarPose { orientation: [-x, -y, -z, w], ..pose };
        let body = inverse.rotate(pose.velocity);
        let faded = notify_a(speed, [body[0], body[1]]);
        let steering = frame.control_f32s()[4];
        writeln!(out, "{},{steering},{speed},{faded}", frame.post).map_err(|e| e.to_string())?;
    }
    Ok(())
}
