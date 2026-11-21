//! `--steer-breakdown FILE` (ours, research tooling; `notes/ngp.md` "Force feedback"): what NGP's steering torque is
//! made of over a recorded drive, a CSV row per physics step. Each steered wheel's add to the car's torque sum
//! (`rbr_physics::wheel::layout::SteerTorque`, kept by the runner at the wheel step's `AddADC`) is split by the force or
//! moment that makes it ([`parts`]). Reads the physics memory, never writes it.
//!
//! The first line, `#`, holds the car's `SteeringRackRatio` and force-feedback sensitivity `S`. Columns: `pre` (the
//! frame's), `sub` (the step in the frame), `speed` (m/s, after the frame), `throttle`, `brake`, `handbrake`,
//! `steering` (the step's control), `sum` (the car's torque sum after the frame, on the frame's last step only),
//! `tyres` (`tyre_ffb::torque` at its suggested trail, after the frame, last step only), then for each steered wheel
//! `wN_`: `fx`, `fy`, `fz` (N, the hub's force along the contact's axes), `lateral`, `longitudinal`, `vertical`,
//! `aligning` (the parts, which add up to `added`), `added` and `gain` (`inv_arm * inv_rack`). A wheel off the ground
//! has `added` and `gain` only. The parts are in the sum's units: times the rack ratio they are the torque NGP's
//! filter takes.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use rbr_gameplay::control::CarControl;
use rbr_physics::car_body::store::CarStore;
use rbr_physics::mem::Mem;
use rbr_physics::wheel::layout::SteerTorque;
use rbr_step::runner::CAR_PTR;

use crate::steer_parts::{parts, steps};

/// An open breakdown.
pub struct SteerLog {
    file: BufWriter<File>,
    path: String,
    /// The steered wheels the header named, once a frame has stepped.
    wheels: Option<Vec<u32>>,
}

impl SteerLog {
    pub fn create(path: &Path) -> Result<Self, String> {
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(SteerLog { file: BufWriter::new(file), path: path.display().to_string(), wheels: None })
    }

    /// The rows of the frame `pre` whose steps kept `torques`, with each step's control `controls` (by step).
    pub fn frame(
        &mut self,
        mem: &Mem,
        pre: u32,
        torques: &[SteerTorque],
        controls: &[Option<CarControl>],
    ) -> Result<(), String> {
        self.rows(mem, pre, torques, controls).map_err(|e| format!("{}: {e}", self.path))
    }

    fn rows(
        &mut self,
        mem: &Mem,
        pre: u32,
        torques: &[SteerTorque],
        controls: &[Option<CarControl>],
    ) -> std::io::Result<()> {
        let car = mem.u32(CAR_PTR);
        let steps = steps(torques);
        let Some(first) = steps.first() else { return Ok(()) };
        let out = &mut self.file;
        if self.wheels.is_none() {
            writeln!(out, "# rack_ratio={} sensitivity={}", mem.steering_rack_ratio(car), mem.max_ff_torque(car))?;
            write!(out, "pre,sub,speed,throttle,brake,handbrake,steering,sum,tyres")?;
            for t in first.iter() {
                for name in ["fx", "fy", "fz", "lateral", "longitudinal", "vertical", "aligning", "added", "gain"] {
                    write!(out, ",w{}_{name}", t.wheel)?;
                }
            }
            writeln!(out)?;
        }
        let wheels = self.wheels.get_or_insert_with(|| first.iter().map(|t| t.wheel).collect());
        let speed = mem.speed(car);
        for (sub, step) in steps.iter().enumerate() {
            write!(out, "{pre},{sub},{speed}")?;
            match controls.get(sub).copied().flatten() {
                Some(c) => write!(out, ",{},{},{},{}", c.throttle, c.brake, c.handbrake, c.steering)?,
                None => write!(out, ",,,,")?,
            }
            if sub + 1 == steps.len() {
                let tyres = crate::tyre_ffb::torque(mem, crate::tyre_ffb::SUGGESTED_TRAIL);
                write!(out, ",{},{tyres}", mem.steer_torque_sum(car).value)?;
            } else {
                write!(out, ",,")?;
            }
            for wheel in wheels.iter() {
                match step.iter().find(|t| t.wheel == *wheel) {
                    Some(t) => {
                        match parts(t) {
                            Some(p) => {
                                let [fx, fy, fz] = p.force;
                                write!(out, ",{fx:.1},{fy:.1},{fz:.1}")?;
                                let (l, g, v, a) = (p.lateral, p.longitudinal, p.vertical, p.aligning);
                                write!(out, ",{l:.4},{g:.4},{v:.4},{a:.4}")?
                            }
                            None => write!(out, ",,,,,,,")?,
                        }
                        write!(out, ",{},{}", t.added, t.gain)?
                    }
                    None => write!(out, ",,,,,,,,,")?,
                }
            }
            writeln!(out)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| format!("{}: {e}", self.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frames_adds_split_into_steps() {
        let t = |wheel| SteerTorque {
            wheel,
            on_ground: false,
            force: [0.0; 3],
            torque: [0.0; 3],
            arm: [0.0; 3],
            strut_dir: [0.0; 3],
            axle: [[0.0; 3]; 2],
            travel: [[0.0; 3]; 2],
            normal: [0.0; 3],
            gain: 0.0,
            added: 0.0,
        };
        let adds = [t(0), t(1), t(0), t(1), t(0)];
        let lens: Vec<usize> = steps(&adds).iter().map(|s| s.len()).collect();
        assert_eq!(lens, [2, 2, 1]);
        assert!(steps(&[]).is_empty());
    }
}
