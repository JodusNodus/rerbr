//! The brush (brush-v1, [`crate::brush`]) on a car's wheels, the tyre model the game steps. On a hard surface (no
//! soil) it takes the contact patch's forces at every speed, down to standstill (weight 1); on soil they stay NGP's.
//! At rest the slips take the brush's rolling-speed floor and the carcass deflections hold (Rill's transport speed);
//! NGP's torsional stiffness, a tenth below 10 m/s, only carries the force through the seam. NGP sets the grip: its
//! static friction (surface, wetness, temperature grip) scales the brush's friction levels, solved per wheel against
//! NGP's own peaks ([`crate::fit`]). Each wheel's carcass deflections (Rill's first-order force dynamics,
//! [`brush::step_dynamic`]) are the model's state: they start from zero when the wheel touches down or comes off soil.

use crate::brush::{self, Calibration, EvalInput, TransState};
use crate::{fit, ForceInput, ForceOutput, Model, Wheel};

/// One wheel's calibration and transient.
struct Tyre {
    at: u32,
    cal: Calibration,
    state: TransState,
    /// Whether the brush ran at the wheel's last step.
    running: bool,
}

/// The brush on a car's wheels.
pub struct Brush {
    tyres: Vec<Tyre>,
}

impl Brush {
    /// The brush calibrated for each of `wheels`.
    pub fn new(wheels: &[Wheel]) -> Result<Brush, String> {
        let tyres = wheels
            .iter()
            .map(|w| Ok(Tyre { at: w.tyre, cal: fit::calibration(w)?, state: TransState::default(), running: false }))
            .collect::<Result<_, String>>()?;
        Ok(Brush { tyres })
    }
}

impl Model for Brush {
    fn forces(&mut self, tyre: u32, x: &ForceInput) -> Option<ForceOutput> {
        let t = self.tyres.iter_mut().find(|t| t.at == tyre)?;
        let usable = x.soil_thickness == 0.0
            && x.normal_force > 0.0
            && x.lateral_stiffness > 0.0
            && x.torsional_stiffness > 0.0
            && x.effective_radius > 0.0;
        if !usable {
            t.running = false;
            return None;
        }
        if !(t.running && x.touching) {
            t.state = TransState::default();
        }
        let pressure_factor = f64::from(x.stiffness_factor) * f64::from(x.stiffness_scale);
        let e = EvalInput {
            fz: f64::from(x.normal_force),
            half_length: f64::from(x.patch_length) / 2.0,
            vx: f64::from(x.longitudinal_velocity),
            vy: f64::from(x.lateral_velocity),
            spin: f64::from(x.spin),
            radius: f64::from(x.effective_radius),
            bulk_temp: f64::from(x.tread_temperature),
            tread_remaining: f64::from(x.tread_left),
            // The wet loss stays NGP's (its wet surface friction): the pressure only enters the brush's.
            pressure: 0.0,
            // NGP's stiffness factor at the pressure, and the slip stiffnesses' fall on a low-friction surface (ice).
            pressure_factor: pressure_factor * brush::low_mu_stiffness_factor(f64::from(x.grip)),
            wetness: 0.0,
            temp_grip: 1.0,
            surf_static: f64::from(x.grip),
            surf_kinetic: f64::from(x.grip),
        };
        let steady = brush::eval(&e, &t.cal);
        // The longitudinal carcass stiffness (ours), at the pressure (the carcass does not soften on ice); the lateral
        // one NGP's.
        let k_x = brush::carcass_x(&t.cal, pressure_factor);
        let mut state = t.state;
        let limit = steady.mu * e.fz;
        let vr = e.spin * e.radius;
        let (fx, fy) =
            brush::step_dynamic(&mut state, &steady, vr, k_x, f64::from(x.lateral_stiffness), limit, f64::from(x.dt));
        if !(fx.is_finite() && fy.is_finite() && steady.trail.is_finite()) {
            t.running = false;
            return None;
        }
        t.state = state;
        t.running = true;
        Some(ForceOutput {
            weight: 1.0,
            lateral_force: fy as f32,
            longitudinal_force: fx as f32,
            pneumatic_trail: steady.trail as f32,
            phase: steady.psi.min(2.0) as f32,
        })
    }

    fn restart(&mut self) {
        for t in &mut self.tyres {
            t.state = TransState::default();
            t.running = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::tests::wheel;

    /// A wheel rolling at `speed` (m/s) on a hard surface at the slip angle `alpha` (rad), free rolling.
    fn rolling(speed: f32, alpha: f32) -> ForceInput {
        let (vlat, vlong) = (speed * alpha.sin(), -speed * alpha.cos());
        ForceInput {
            dt: 1.0 / 720.0,
            touching: true,
            speed,
            lateral_velocity: vlat,
            longitudinal_velocity: vlong,
            spin: -vlong / 0.3,
            effective_radius: 0.3,
            normal_force: 4000.0,
            patch_length: 0.1,
            grip: 1.2,
            stiffness_scale: 1.0,
            stiffness_factor: 1.0,
            tread_temperature: 350.0,
            tread_left: 1.0,
            soil_thickness: 0.0,
            lateral_stiffness: 150000.0,
            torsional_stiffness: 20000.0,
        }
    }

    #[test]
    fn it_takes_hard_surfaces_at_any_speed_down_to_standstill() {
        let mut b = Brush::new(&[wheel()]).unwrap();
        let at = wheel().tyre;
        assert_eq!(b.forces(at, &ForceInput { soil_thickness: 0.01, ..rolling(20.0, 0.05) }), None);
        assert_eq!(b.forces(at + 4, &rolling(20.0, 0.05)), None);
        for speed in [0.0, 0.3, 1.9, 2.5, 20.0] {
            let out = b.forces(at, &rolling(speed, 0.05)).unwrap();
            assert_eq!(out.weight, 1.0, "{speed}");
            assert!(out.lateral_force.is_finite() && out.longitudinal_force.is_finite(), "{speed} {out:?}");
        }
        // At rest (no speed, no spin) with NGP's torsional stiffness at its tenth, a deflection holds.
        b.restart();
        let push = ForceInput { torsional_stiffness: 2000.0, ..rolling(1.0, 0.05) };
        let pushed = (0..720).map(|_| b.forces(at, &push).unwrap()).last().unwrap();
        let rest = ForceInput { lateral_velocity: 0.0, longitudinal_velocity: 0.0, spin: 0.0, speed: 0.0, ..push };
        let held = b.forces(at, &rest).unwrap();
        assert!(held.lateral_force < 0.0 && (held.lateral_force / pushed.lateral_force - 1.0).abs() < 0.05, "{held:?}");
        assert_eq!(b.forces(at, &rest).unwrap().lateral_force, held.lateral_force);
    }

    #[test]
    fn the_force_builds_over_the_relaxation_length_to_the_steady_brush() {
        let mut b = Brush::new(&[wheel()]).unwrap();
        let at = wheel().tyre;
        let x = rolling(20.0, 2f32.to_radians());
        let first = b.forces(at, &x).unwrap();
        let mut last = first;
        for _ in 0..720 {
            last = b.forces(at, &x).unwrap();
        }
        let e = EvalInput {
            fz: 4000.0,
            half_length: 0.05,
            vx: f64::from(x.longitudinal_velocity),
            vy: f64::from(x.lateral_velocity),
            spin: f64::from(x.spin),
            radius: 0.3,
            bulk_temp: 350.0,
            tread_remaining: 1.0,
            pressure: 0.0,
            pressure_factor: 1.0,
            wetness: 0.0,
            temp_grip: 1.0,
            surf_static: 1.2,
            surf_kinetic: 1.2,
        };
        let steady = brush::eval(&e, &b.tyres[0].cal);
        // It opposes the sliding, starts small (the carcass damping's d Vs, 300 N, and one step's 2.8 cm of a
        // relaxation length of about 0.4 m) and settles on the steady state.
        assert!(last.lateral_force < 0.0, "{last:?}");
        assert!(first.lateral_force.abs() < 0.2 * last.lateral_force.abs(), "{first:?} {last:?}");
        assert!((f64::from(last.lateral_force) / steady.fy - 1.0).abs() < 1e-4, "{last:?} {steady:?}");
        assert!(last.longitudinal_force.abs() < 1e-3 * last.lateral_force.abs(), "{last:?}");
        // A touch down or a restart starts it from zero again.
        let landed = b.forces(at, &ForceInput { touching: false, ..x }).unwrap();
        assert_eq!(landed, first);
        b.forces(at, &x);
        b.restart();
        assert_eq!(b.forces(at, &x).unwrap(), first);
    }

    #[test]
    fn the_names_build_their_models() {
        assert!(crate::model("ngp", &[wheel()]).unwrap().is_none());
        assert!(crate::model("brush", &[wheel()]).unwrap().is_some());
        assert!(crate::model("magic", &[wheel()]).is_err());
    }
}
