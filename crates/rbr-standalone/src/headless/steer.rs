//! `--steer-breakdown FILE` (ours, research tooling; `notes/ngp.md` "Force feedback"): what NGP's steering torque is
//! made of over a recorded drive, a CSV row per physics step. Each steered wheel's add to the car's torque sum
//! (`rbr_physics::wheel::layout::SteerTorque`, kept by the runner at the wheel step's `AddADC`) is split by the force or
//! moment that makes it ([`parts`]). Reads the physics memory, never writes it.
//!
//! The first line, `#`, holds the car's `SteeringRackRatio` and force-feedback sensitivity `S`. Columns: `pre` (the
//! frame's), `sub` (the step in the frame), `speed` (m/s, after the frame), `throttle`, `brake`, `handbrake`,
//! `steering` (the step's control), `sum` (the car's torque sum after the frame, on the frame's last step only),
//! `tyres` (`tyre_ffb::torque` at its suggested trail, after the frame, last step only), `yaw` (the car's angular
//! velocity about the world vertical, rad/s) and `forward` (its forward speed, m/s), then for each of the four tyres
//! `tK_` (`K` the wheel's place in the car, the front pair first): `alpha` (the slip angle, degrees, `atan2` of the
//! contact's lateral and longitudinal velocity, in its own, mirrored, frame), `fy`, `fz` (N, the tyre's lateral and
//! normal force), `slide` (the sliding model's weight), `class` (the surface class the wheel's blend weighs most,
//! `wheel::layout::surface_classes`), `mu` (the surface's static friction) and `soil` (its loose soil's thickness), empty
//! off the ground (all last step only), then for each
//! steered wheel `wN_`: `fx`, `fy`, `fz` (N, the hub's force along the contact's axes), `lateral`, `longitudinal`,
//! `vertical`, `aligning` (the parts, which add up to `added`), `added` and `gain` (`inv_arm * inv_rack`). A wheel off
//! the ground has `added` and `gain` only. The parts are in the sum's units: times the rack ratio they are the torque
//! NGP's filter takes.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use rbr_gameplay::control::CarControl;
use rbr_physics::car_body::store::CarStore;
use rbr_physics::mem::Mem;
use rbr_physics::tyre::layout::frame_of;
use rbr_physics::wheel::layout::{surface_classes, SteerTorque, ON_GROUND, TYRE_SCRATCH};
use rbr_step::runner::CAR_PTR;

use crate::tyre_ffb::WHEELS;

type V = [f64; 3];

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `a + b * s`.
fn add_scaled(a: V, b: V, s: f64) -> V {
    std::array::from_fn(|k| a[k] + b[k] * s)
}

fn unit(a: V) -> Option<V> {
    let length = dot(a, a).sqrt();
    (length > 1e-6).then(|| a.map(|x| x / length))
}

/// The world vector `v` in the car's frame, from two directions known in both (`world`, `car`; not parallel): its
/// coordinates on the two and their cross product are the same in either frame.
fn to_car(v: V, world: [V; 2], car: [V; 2]) -> Option<V> {
    let [a, u] = world;
    let (aa, au, uu) = (dot(a, a), dot(a, u), dot(u, u));
    // |a x u|^2.
    let det = aa * uu - au * au;
    if det < 1e-9 {
        return None;
    }
    let (va, vu) = (dot(v, a), dot(v, u));
    let on = [(va * uu - vu * au) / det, (vu * aa - va * au) / det, dot(v, cross(a, u)) / det];
    let [a, u] = car;
    Some(add_scaled(add_scaled(a.map(|x| x * on[0]), u, on[1]), cross(a, u), on[2]))
}

/// A steered wheel's add to the steering torque sum, split along the contact's axes: `n` the surface normal, `y` the
/// axle across it, `x = y x n`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parts {
    /// The hub's force along `x`, `y` and `n`, N.
    pub force: [f64; 3],
    /// The lateral force's lever about the steering axis and the tyre's moment about `x` (the lateral force at the
    /// ground, under the hub): the caster trail's part.
    pub lateral: f64,
    /// The longitudinal force's lever and the tyre's moment about `y`: the scrub radius's part.
    pub longitudinal: f64,
    /// The vertical force's lever: the steering axis's inclination.
    pub vertical: f64,
    /// The tyre's moment about the normal: its aligning moment.
    pub aligning: f64,
}

/// `torque`'s parts, which add up to its `added`; none for a wheel off the ground, whose contact is stale.
pub fn parts(torque: &SteerTorque) -> Option<Parts> {
    if !torque.on_ground {
        return None;
    }
    let d = |v: [f32; 3]| v.map(f64::from);
    let axle = d(torque.axle[0]);
    let n = unit(to_car(d(torque.normal), [d(torque.axle[1]), d(torque.travel[1])], [axle, d(torque.travel[0])])?)?;
    let y = unit(add_scaled(axle, n, -dot(axle, n)))?;
    let x = cross(y, n);
    let (f, m, r, k) = (d(torque.force), d(torque.torque), d(torque.arm), d(torque.strut_dir));
    let gain = f64::from(torque.gain);
    let force = [x, y, n].map(|e| dot(f, e));
    let lever = |e: V| dot(k, cross(r, e));
    Some(Parts {
        force,
        lateral: gain * (lever(y) * force[1] + dot(k, x) * dot(m, x)),
        longitudinal: gain * (lever(x) * force[0] + dot(k, y) * dot(m, y)),
        vertical: gain * lever(n) * force[2],
        aligning: gain * dot(k, n) * dot(m, n),
    })
}

/// The steps of a frame's adds, in order: a step ends where a wheel adds again.
fn steps(torques: &[SteerTorque]) -> Vec<&[SteerTorque]> {
    let mut out = Vec::new();
    let mut from = 0;
    for k in 1..torques.len() {
        if torques[from..k].iter().any(|t| t.wheel == torques[k].wheel) {
            out.push(&torques[from..k]);
            from = k;
        }
    }
    if from < torques.len() {
        out.push(&torques[from..]);
    }
    out
}

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
            write!(out, "pre,sub,speed,throttle,brake,handbrake,steering,sum,tyres,yaw,forward")?;
            for k in 0..4 {
                write!(out, ",t{k}_alpha,t{k}_fy,t{k}_fz,t{k}_slide,t{k}_class,t{k}_mu,t{k}_soil")?;
            }
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
                let yaw = mem.angular_velocity(car).z;
                write!(out, ",{},{tyres},{yaw},{}", mem.steer_torque_sum(car).value, mem.forward_speed(car))?;
                for k in 0..4 {
                    let wheel = mem.u32(car + WHEELS + 4 * k);
                    if mem.u32(wheel + ON_GROUND) == 0 {
                        write!(out, ",,,,,,,")?;
                        continue;
                    }
                    let s = frame_of(mem, wheel + TYRE_SCRATCH).scratch;
                    let [lat, long, _] = s.contact_velocity;
                    let alpha = lat.atan2(long.abs()).to_degrees();
                    write!(out, ",{alpha:.3},{:.1},{:.1},{:.3}", s.lateral_force, s.normal_force, s.sliding_weight)?;
                    let (classes, blend) = surface_classes(mem, wheel + TYRE_SCRATCH);
                    let class = classes[usize::from(blend > 0.5)];
                    write!(out, ",{class},{:.3},{:.4}", s.static_friction, s.soil_thickness)?;
                }
            } else {
                write!(out, ",,,,{}", ",".repeat(28))?;
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

    /// A rotation about a tilted axis, as rows.
    fn rotation() -> [V; 3] {
        let k = unit([0.3, -0.5, 0.8]).unwrap();
        let (s, c) = 0.7f64.sin_cos();
        // Rodrigues: each basis vector turned about `k`.
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            .map(|e| add_scaled(add_scaled(e.map(|x| x * c), cross(k, e), s), k, dot(k, e) * (1.0 - c)))
    }

    fn turn(m: &[V; 3], v: V) -> V {
        add_scaled(add_scaled(m[0].map(|x| x * v[0]), m[1], v[1]), m[2], v[2])
    }

    fn f(v: V) -> [f32; 3] {
        v.map(|x| x as f32)
    }

    #[test]
    fn a_world_vector_comes_back_in_the_cars_frame() {
        let m = rotation();
        let car = [unit([0.99, 0.02, 0.1]).unwrap(), unit([0.1, 0.97, -0.2]).unwrap()];
        let v = [0.2, -0.4, 0.9];
        let got = to_car(turn(&m, v), car.map(|e| turn(&m, e)), car).unwrap();
        for k in 0..3 {
            assert!((got[k] - v[k]).abs() < 1e-12, "{got:?}");
        }
        assert_eq!(to_car(v, [car[0], car[0]], car), None);
    }

    #[test]
    fn the_parts_add_up_to_the_wheels_add() {
        let m = rotation();
        let axle = unit([0.98, 0.05, -0.15]).unwrap();
        let travel = unit([0.05, 0.99, 0.12]).unwrap();
        let normal = unit([0.03, 0.999, 0.02]).unwrap();
        let (force, torque) = ([310.0, 4100.0, -2900.0], [-880.0, 35.0, -95.0]);
        let (arm, strut_dir, gain) = ([0.21, -0.52, 0.03], unit([0.12, 0.98, -0.14]).unwrap(), 9.5);
        let added = gain * dot(strut_dir, add_scaled(cross(arm, force), torque, 1.0));
        let t = SteerTorque {
            wheel: 0,
            on_ground: true,
            force: f(force),
            torque: f(torque),
            arm: f(arm),
            strut_dir: f(strut_dir),
            axle: [f(axle), f(turn(&m, axle))],
            travel: [f(travel), f(turn(&m, travel))],
            normal: f(turn(&m, normal)),
            gain: gain as f32,
            added: added as f32,
        };
        let p = parts(&t).unwrap();
        let sum = p.lateral + p.longitudinal + p.vertical + p.aligning;
        assert!((sum - added).abs() < 1e-3 * added.abs(), "{sum} against {added}");
        // The vertical force is the force along the normal.
        assert!((p.force[2] - dot(force, normal)).abs() < 0.5, "{p:?}");
        assert_eq!(parts(&SteerTorque { on_ground: false, ..t }), None);
    }

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
