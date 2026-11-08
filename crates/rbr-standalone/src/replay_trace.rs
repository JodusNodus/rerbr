//! `--replay-trace FILE` (with `--replay`): one text line per frame of a replayed drive that stepped, the car's
//! state and each wheel's tyre after it, for research on a player's drive (ours, read-only: physics memory is read
//! after the step, never written). Columns are [`HEADER`]'s; the wheels FL FR RL RR, each with [`WHEEL_HEADER`]'s.

use std::io::Write;
use std::path::Path;

use rbr_hybrid::pure::{self, CAR_BYTES, CAR_PTR};
use rbr_physics::mem::Mem;
use rbr_physics::tyre::store::TyreStore;
use rbr_physics::wheel::layout::TYRE_SCRATCH;
use rbr_recording::pose;
use rbr_recording::{read_f32, CONTROL_BYTES, WHEEL_BYTES};

use crate::live::Live;
use crate::physics::DRIVELINE_OFFSET;

/// The wheel pointers in the car (`car+0x470`), as `tyre_probe` reads them.
const WHEELS: u32 = 0x470;

/// The frame's columns: step, the time since the first traced step (s), whether the time card started (0/1), the
/// substeps of the frame, the car's control
/// the last step took (CTL1: throttle, brake, handbrake, clutch 0-1, steering -1 left to 1 right), the gear (HUD),
/// the car's position (m), orientation (quaternion x y z w) and velocity (m/s, world, z up), its driveline
/// parameter (`CCar` +0x17c) and the time card's distance (m).
pub const HEADER: &str =
    "# step time started substeps thr brk hb clutch steer gear x y z qx qy qz qw vx vy vz param dist";
/// A wheel's columns: its axle in the car frame (x y, a unit vector out of the car: the steering angle), the tyre's
/// normal force (N), the contact point's lateral and longitudinal velocity (m/s, forward motion negative), the slip
/// velocity along the rolling axis (`R·ω + v`, m/s), the spin (rad/s), the lateral and longitudinal force (N, NGP's
/// signs), the slip phase (0-1 to the peak, 1-2 past it, 2 sliding) and the sliding model's weight.
pub const WHEEL_HEADER: &str = "ax ay fz vlat vlong vslip spin flat flong phase wslide";

/// An open trace.
pub struct ReplayTrace {
    file: std::io::BufWriter<std::fs::File>,
    path: String,
    /// The first traced frame's step.
    first: Option<u32>,
}

impl ReplayTrace {
    pub fn create(path: &Path) -> Result<Self, String> {
        let file =
            std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
        let mut trace = ReplayTrace { file, path: path.display().to_string(), first: None };
        let wheels = ["fl", "fr", "rl", "rr"]
            .map(|w| WHEEL_HEADER.split(' ').map(|c| format!(" {w}_{c}")).collect::<String>())
            .concat();
        writeln!(trace.file, "{HEADER}{wheels}").map_err(|e| trace.failed(e))?;
        Ok(trace)
    }

    fn failed(&self, e: std::io::Error) -> String {
        format!("{}: {e}", self.path)
    }

    /// The line of the frame `live` just ran in `substeps` steps.
    pub fn frame(&mut self, live: &Live, substeps: u32, control: &[u8; CONTROL_BYTES]) -> Result<(), String> {
        let mem = live.mem();
        let word = |i: usize| f32::from_le_bytes(control[4 * i..4 * i + 4].try_into().unwrap());
        let bytes = pure::frame_bytes(mem);
        let car = &bytes[..CAR_BYTES];
        let pose = pose::car_pose(car).ok_or("the car block is short")?;
        let card = pure::time_keeper(mem).card;
        let step = live.next_step() - 1;
        let first = *self.first.get_or_insert(step);
        let time = f64::from(step - first) * f64::from(live.step_clock().step);
        let [x, y, z] = pose.position;
        let [qx, qy, qz, qw] = pose.orientation;
        let [vx, vy, vz] = pose.velocity;
        let mut line = format!(
            "{step} {time:.4} {} {substeps} {:.4} {:.4} {:.4} {:.4} {:.4} {} {x:.3} {y:.3} {z:.3} {qx:.6} {qy:.6} {qz:.6} \
             {qw:.6} {vx:.4} {vy:.4} {vz:.4} {:.3} {:.3}",
            u8::from(card.started),
            word(0),
            word(1),
            word(2),
            word(3),
            word(4),
            crate::live::hud_gear(mem),
            read_f32(car, DRIVELINE_OFFSET).unwrap_or(f32::NAN),
            card.last,
        );
        for k in 0..4 {
            let block = &bytes[CAR_BYTES + k * WHEEL_BYTES..CAR_BYTES + (k + 1) * WHEEL_BYTES];
            let axle = pose::wheel_pose(block).map_or([f32::NAN; 3], |w| w.axle);
            line += &wheel(mem, mem.u32(mem.u32(CAR_PTR) + WHEELS + 4 * k as u32), axle);
        }
        writeln!(self.file, "{line}").map_err(|e| self.failed(e))
    }

    pub fn finish(mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| self.failed(e))
    }
}

/// A wheel's columns ([`WHEEL_HEADER`]).
fn wheel(mem: &Mem, wheel: u32, axle: [f32; 3]) -> String {
    let s = mem.load_frame(wheel + TYRE_SCRATCH).scratch;
    format!(
        " {:.5} {:.5} {:.1} {:.4} {:.4} {:.4} {:.3} {:.1} {:.1} {:.4} {:.3}",
        axle[0],
        axle[1],
        s.normal_force,
        s.contact_velocity[0],
        s.contact_velocity[1],
        s.long_slip_velocity,
        s.spin,
        s.lateral_force,
        s.longitudinal_force,
        s.slip_phase,
        s.sliding_weight,
    )
}
