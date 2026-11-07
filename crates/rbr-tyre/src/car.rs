//! The brush (brush-v1, [`crate::brush`]) on a car's wheels, the tyre model the game steps. On a hard surface (no
//! soil) it takes the contact patch's forces from NGP's sliding model by a weight rising from 0 at 2 m/s, where NGP's
//! sliding model starts to run alone, to 1 at 3 m/s; elsewhere they stay NGP's. NGP sets the grip: its static friction
//! (surface, wetness, temperature grip) scales the brush's friction levels, solved per wheel against NGP's own peaks
//! ([`crate::fit`]). Each wheel's carcass deflections (Rill's first-order force dynamics, [`brush::step_dynamic`]) are
//! the model's state: they start from zero when the wheel touches down or the brush takes over.

use crate::brush::{self, Calibration, EvalInput, TransState};
use crate::{fit, ForceInput, ForceOutput, Model, Wheel};

/// The contact speed (m/s) from which the brush takes a share, and the speed over which it rises to all of it.
const FROM_SPEED: f32 = 2.0;
const OVER_SPEED: f32 = 1.0;

/// One wheel's calibration and transient.
struct Tyre {
    at: u32,
    cal: Calibration,
    state: TransState,
    /// Whether the brush took a share at the wheel's last step.
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
        let weight = ((x.speed - FROM_SPEED) / OVER_SPEED).min(1.0);
        let usable = x.soil_thickness == 0.0
            && x.normal_force > 0.0
            && x.lateral_stiffness > 0.0
            && x.torsional_stiffness > 0.0
            && x.effective_radius > 0.0;
        if !(weight > 0.0 && usable) {
            t.running = false;
            return None;
        }
        if !(t.running && x.touching) {
            t.state = TransState::default();
        }
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
            pressure_factor: f64::from(x.stiffness_factor) * f64::from(x.stiffness_scale),
            wetness: 0.0,
            temp_grip: 1.0,
            surf_static: f64::from(x.grip),
            surf_kinetic: f64::from(x.grip),
        };
        let steady = brush::eval(&e, &t.cal);
        // The longitudinal carcass stiffness (ours), at the pressure; the lateral one NGP's.
        let k_x = brush::carcass_x(&t.cal, e.pressure_factor);
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
            weight,
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
    fn it_takes_over_from_2_to_3_m_s_on_a_hard_surface_only() {
        let mut b = Brush::new(&[wheel()]).unwrap();
        let at = wheel().tyre;
        assert_eq!(b.forces(at, &rolling(1.9, 0.05)), None);
        assert_eq!(b.forces(at, &ForceInput { soil_thickness: 0.01, ..rolling(20.0, 0.05) }), None);
        assert_eq!(b.forces(at + 4, &rolling(20.0, 0.05)), None);
        assert!((b.forces(at, &rolling(2.5, 0.05)).unwrap().weight - 0.5).abs() < 1e-6);
        assert_eq!(b.forces(at, &rolling(20.0, 0.05)).unwrap().weight, 1.0);
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
