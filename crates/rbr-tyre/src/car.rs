//! The brush (brush-v1, [`crate::brush`]) on a car's wheels, the tyre model the game steps. It takes the contact
//! patch's forces at every speed, down to standstill (weight 1): on a hard surface the brush's, on soil (gravel, snow)
//! the loose-surface tyre's (loose-v1, [`crate::loose`]), with NGP's soil lateral force on top.
//! At rest the slips take the brush's rolling-speed floor and the carcass deflections hold (Rill's transport speed);
//! NGP's torsional stiffness, a tenth below 10 m/s, only carries the force through the seam. NGP sets the grip: its
//! static friction (surface, wetness, temperature grip) scales the brush's friction levels, solved per wheel against
//! NGP's own peaks ([`crate::fit`]). Each wheel's carcass deflections (Rill's first-order force dynamics,
//! [`brush::step_dynamic`]) are the model's state, on either surface and across the change between them: they start
//! from zero when the wheel touches down. Its temperatures are the model's too ([`crate::thermal`]): taken from NGP's
//! where the brush starts on a wheel (start, restart, touch-down), stepped with the patch's friction power, read by
//! the grip and the stiffness, and handed back for NGP's tyre to hold in place of its heat step's.

use crate::brush::{self, Calibration, EvalInput, TransState};
use crate::loose::{self, Loose};
use crate::thermal::{self, Temperatures};
use crate::tuning::Tuning;
use crate::{fit, ForceInput, ForceOutput, Model, Wheel};

/// One wheel's calibration and transient.
struct Tyre {
    at: u32,
    cal: Calibration,
    /// The slip stiffnesses (cornering, longitudinal) of NGP's own slope ([`fit::ngp_calibration`]), which ice falls
    /// from.
    ngp: (f64, f64),
    loose: Loose,
    /// NGP's stiffness factor at the reference pressure ([`brush::reference_stiffness_factor`]).
    sf_ref: f64,
    /// NGP's damp tarmac tyre's base, wet slope and design wetness on the tarmac class ([`brush::damp_tyre_level`]);
    /// `None` for every other tyre.
    damp_tarmac: Option<(f64, f64, f64)>,
    /// Whether the tyre is studded ([`Wheel::is_studded`]).
    studded: bool,
    state: TransState,
    /// The surface and bulk temperatures ([`thermal`]); `None` until the brush runs with NGP's thermal input.
    temperatures: Option<Temperatures>,
    /// NGP's temperature axis and `RubMyFct` over it.
    rubber: (Vec<f32>, Vec<f32>),
    /// Whether the brush ran at the wheel's last step.
    running: bool,
}

/// The brush on a car's wheels.
pub struct Brush {
    tyres: Vec<Tyre>,
    tuning: Tuning,
}

impl Brush {
    /// The brush calibrated for each of `wheels`, tuned by `tuning` ([`crate::tuning`]).
    pub fn new(wheels: &[Wheel], tuning: &Tuning) -> Result<Brush, String> {
        let tyres = wheels
            .iter()
            .map(|w| {
                let cal = fit::calibration(w, tuning)?;
                let ngp = fit::ngp_calibration(w)?;
                let loose = fit::loose(w, tuning)?;
                Ok(Tyre {
                    at: w.tyre,
                    cal,
                    ngp: (ngp.cy0 * tuning.stiffness, ngp.cx0 * tuning.stiffness),
                    loose,
                    sf_ref: brush::reference_stiffness_factor(&w.pressure_axis, &w.stiffness_factor),
                    damp_tarmac: w.is_damp_tarmac().then(|| {
                        let class = brush::TARMAC_CLASS as usize;
                        let base = f64::from(w.class_friction[class]);
                        (base, f64::from(w.class_wet_slope[class]), f64::from(w.design_wetness))
                    }),
                    studded: w.is_studded(),
                    state: TransState::default(),
                    temperatures: None,
                    rubber: (w.temperature_axis.clone(), w.rubber_friction.clone()),
                    running: false,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Brush { tyres, tuning: *tuning })
    }
}

impl Model for Brush {
    fn forces(&mut self, tyre: u32, x: &ForceInput) -> Option<ForceOutput> {
        let t = self.tyres.iter_mut().find(|t| t.at == tyre)?;
        let usable = x.normal_force > 0.0
            && x.lateral_stiffness > 0.0
            && x.torsional_stiffness > 0.0
            && x.effective_radius > 0.0;
        if !usable {
            t.running = false;
            return None;
        }
        let soil = x.soil_thickness > 0.0;
        // On soil NGP's heat step keeps the temperatures: no published heat share or soil conduction for a tyre
        // sliding on gravel or snow, where much of the power goes into the soil.
        let thermal = x.thermal.filter(|th| th.rubber_grip > 0.0 && !soil);
        if !(t.running && x.touching) {
            t.state = TransState::default();
        }
        if !(t.running && x.touching) || thermal.is_none() || t.temperatures.is_none() {
            // NGP's temperatures as they stand (its relaxation cools the tyre while it is off the ground).
            t.temperatures = thermal.map(|th| Temperatures {
                surface: f64::from(th.surface_temperature),
                bulk: f64::from(x.tread_temperature),
            });
        }
        let temperatures = t.temperatures;
        // The model's temperatures in place of NGP's: NGP's rubber grip divided out and taken at them.
        let (rubber_grip, bulk_temp) = match (thermal, temperatures) {
            (Some(th), Some(temps)) => {
                (thermal::rubber_grip(&t.rubber.0, &t.rubber.1, temps) / f64::from(th.rubber_grip), temps.bulk)
            }
            _ => (1.0, f64::from(x.tread_temperature)),
        };
        let scale = f64::from(x.stiffness_scale);
        let pressure_factor = f64::from(x.stiffness_factor) * scale;
        let mut grip = f64::from(x.grip) * rubber_grip;
        // A studded tyre on bare ice (the hard surface of the ice class): its studs' level in place of NGP's.
        if t.studded && !soil {
            let ice = loose::class_share(x.surface_classes, x.surface_blend, crate::tuning::ICE_CLASS);
            grip += (brush::STUDDED_ICE - brush::BARE_ICE) * ice;
        }
        grip *= self.tuning.grip(soil, x.surface_classes, x.surface_blend);
        // NGP's damp tarmac tyre on its tarmac share: the dry tyre's factor times R(W) in place of NGP's.
        if let Some((base, slope, design)) = t.damp_tarmac {
            let share = loose::class_share(x.surface_classes, x.surface_blend, brush::TARMAC_CLASS);
            grip *= 1.0 + (brush::damp_tyre_level(f64::from(x.water_factor), base, slope, design) - 1.0) * share;
        }
        // NGP's stiffness factor at the pressure, and on a hard surface the slip stiffnesses' fall on low friction (ice).
        let low_mu = if soil { 1.0 } else { brush::low_mu_stiffness_factor(grip) };
        let fz = f64::from(x.normal_force);
        let e = EvalInput {
            fz,
            // The aligning moment's half length: the patch's, with the trail's own load dependence.
            half_length: brush::trail_half_length(f64::from(x.patch_length) / 2.0, fz, t.cal.fz0),
            vx: f64::from(x.longitudinal_velocity),
            // The inclination's camber thrust as a lateral slip velocity of the rolling tyre.
            vy: f64::from(x.lateral_velocity)
                + brush::camber_slip_velocity(f64::from(x.inclination_sin), f64::from(x.spin * x.effective_radius)),
            spin: f64::from(x.spin),
            radius: f64::from(x.effective_radius),
            bulk_temp,
            tread_remaining: f64::from(x.tread_left),
            // NGP's wet surface friction carries the wet level; the wetness lowers the brush's sliding friction, its
            // damp level to 0.82 of dry, fades its dry-wear gain and lowers the friction with speed (the thin film;
            // worn, the partial hydroplaning). On gravel it slows the shear law's rise and takes NGP's damp loss back
            // out (`loose::gravel_damp_level`).
            pressure: 0.0,
            pressure_factor: low_mu
                * if soil {
                    pressure_factor
                } else {
                    brush::slip_pressure_factor(t.sf_ref, f64::from(x.stiffness_factor)) * scale
                },
            wetness: f64::from(x.water_factor),
            hydroplaning: true,
            temp_grip: 1.0,
            surf_static: grip,
            surf_kinetic: grip,
        };
        let steady = if soil {
            let share = |class| loose::class_share(x.surface_classes, x.surface_blend, class);
            loose::eval(&e, &t.loose, share(loose::GRAVEL_CLASS), share(loose::SNOW_CLASS), f64::from(x.soil_thickness))
        } else {
            let cy0 = brush::low_mu_slip_stiffness(grip, t.cal.cy0, t.ngp.0);
            let cx0 = brush::low_mu_slip_stiffness(grip, t.cal.cx0, t.ngp.1);
            brush::eval(&e, &Calibration { cy0, cx0, ..t.cal })
        };
        // The carcass stiffnesses at the pressure (the carcass does not soften on ice): the longitudinal one ours, the
        // lateral one NGP's moved to the published pressure law.
        // The tuning's response lengthens the relaxation lengths by softening the carcass.
        let k_x = brush::carcass_x(&t.cal, pressure_factor) / self.tuning.response;
        let k_y = brush::carcass_y(&t.cal, f64::from(x.lateral_stiffness), pressure_factor) / self.tuning.response;
        let mut state = t.state;
        let limit = steady.mu * e.fz;
        let vr = e.spin * e.radius;
        let (fx, fy) = brush::step_dynamic(&mut state, &steady, vr, k_x, k_y, limit, f64::from(x.dt));
        if !(fx.is_finite() && fy.is_finite() && steady.trail.is_finite()) {
            t.running = false;
            return None;
        }
        t.state = state;
        t.running = true;
        // The patch's friction power: the force the tyre holds times the slip velocity, each axis.
        let temperatures = thermal.zip(temperatures).map(|(th, temps)| {
            let power = (fx * (e.vx + vr)).abs() + (fy * f64::from(x.lateral_velocity)).abs();
            let input = thermal::Input {
                dt: f64::from(x.dt),
                friction_power: power,
                rolling_power: f64::from(th.rolling_power),
                wetness: e.wetness,
                speed: f64::from(x.speed),
                air: f64::from(th.air_temperature),
                carcass: f64::from(th.carcass_temperature),
                surface_capacity: f64::from(th.surface_capacity),
                bulk_capacity: f64::from(th.tread_capacity),
                tread_depth: f64::from(th.tread_depth),
                tread_width: f64::from(th.tread_width),
                diameter: 2.0 * f64::from(th.outer_radius),
                tread_area: f64::from(th.tread_area),
                contact_area: f64::from(th.contact_area),
                carcass_conductance: f64::from(th.carcass_conductance),
                contact_conductance: f64::from(th.contact_conductance),
            };
            thermal::step(temps, &input)
        });
        let temperatures = temperatures.filter(|n| n.surface.is_finite() && n.bulk.is_finite());
        if temperatures.is_some() {
            t.temperatures = temperatures;
        }
        Some(ForceOutput {
            weight: 1.0,
            lateral_force: fy as f32,
            longitudinal_force: fx as f32,
            pneumatic_trail: steady.trail as f32,
            phase: steady.psi.min(2.0) as f32,
            temperatures: temperatures.map(|n| [n.surface as f32, n.bulk as f32]),
        })
    }

    fn restart(&mut self) {
        for t in &mut self.tyres {
            t.state = TransState::default();
            t.temperatures = None;
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
            water_factor: 0.0,
            inclination_sin: 0.0,
            surface_classes: [rbr_physics::tyre::model::UNKNOWN_CLASS; 2],
            surface_blend: 0.0,
            lateral_stiffness: 150000.0,
            torsional_stiffness: 20000.0,
            thermal: None,
        }
    }

    #[test]
    fn it_takes_every_surface_at_any_speed_down_to_standstill() {
        let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
        let at = wheel().tyre;
        assert_eq!(b.forces(at + 4, &rolling(20.0, 0.05)), None);
        let gravel = [loose::GRAVEL_CLASS; 2];
        for (soil, surface_classes) in [(0.0, gravel), (0.009, [3, 3]), (0.009, gravel)] {
            for speed in [0.0, 0.3, 1.9, 2.5, 20.0] {
                let x = ForceInput { soil_thickness: soil, surface_classes, ..rolling(speed, 0.05) };
                let out = b.forces(at, &x).unwrap();
                assert_eq!(out.weight, 1.0, "{speed}");
                assert!(out.lateral_force.is_finite() && out.longitudinal_force.is_finite(), "{speed} {out:?}");
            }
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
    fn a_wet_surface_lowers_the_locked_wheel_s_friction() {
        // Locked at 20 m/s on the same grip, settled: wet slides at less of it than dry.
        let locked = |water_factor: f32| {
            let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
            let x = ForceInput { spin: 0.0, water_factor, ..rolling(20.0, 0.0) };
            (0..720).map(|_| b.forces(wheel().tyre, &x).unwrap()).last().unwrap().longitudinal_force
        };
        let (dry, wet) = (locked(0.0), locked(1.0));
        assert!(wet > 0.0 && wet / dry < 0.9, "dry {dry} wet {wet}");
    }

    #[test]
    fn the_force_builds_over_the_relaxation_length_to_the_steady_brush() {
        let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
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
            hydroplaning: false,
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
    fn the_tuning_scales_the_grip_by_surface_and_the_relaxation_length() {
        let at = wheel().tyre;
        let tuned = |t: Tuning| Brush::new(&[wheel()], &t).unwrap();
        let x = rolling(15.0, 20f32.to_radians());
        let gravel = ForceInput { soil_thickness: 0.009, grip: 0.9, surface_classes: [loose::GRAVEL_CLASS; 2], ..x };
        let ice = ForceInput { grip: 0.1, surface_classes: [crate::tuning::ICE_CLASS; 2], ..x };
        let lateral = |t: Tuning, x: &ForceInput| f64::from(settle(&mut tuned(t), x, 2000).lateral_force);
        let base = Tuning::default();
        // Sliding whole (20°), the force follows the grip; each surface takes its own factor only.
        for (x, t) in [(&x, Tuning { tarmac: 0.8, ..base }), (&gravel, Tuning { gravel: 0.8, ..base })] {
            assert!((lateral(t, x) / lateral(base, x) - 0.8).abs() < 0.03, "{t:?}");
        }
        assert_eq!(lateral(Tuning { tarmac: 0.8, snow: 1.2, ..base }, &ice), lateral(base, &ice));
        assert!((lateral(Tuning { ice: 1.2, ..base }, &ice) / lateral(base, &ice) - 1.2).abs() < 0.03);
        // The response doubles the relaxation length: the steps to 63 % of a 2° step's force.
        let x = rolling(20.0, 2f32.to_radians());
        let steps = |t: Tuning| {
            let mut b = tuned(t);
            let end = f64::from(settle(&mut tuned(t), &x, 2000).lateral_force);
            (1..).find(|_| f64::from(b.forces(at, &x).unwrap().lateral_force) / end > 0.632).unwrap()
        };
        let (one, two) = (steps(base), steps(Tuning { response: 2.0, ..base }));
        assert!((f64::from(two) / f64::from(one) - 2.0).abs() < 0.15, "{one} {two}");
    }

    #[test]
    fn a_studded_tyre_holds_on_bare_ice_and_the_others_keep_ngp_s_level() {
        // NGP's snow tyre's surface words (probe `tyre_classes`, kind 6) against the dry tarmac tyre's (kind 0).
        let snow = Wheel { class_friction: [0.95, 0.95, 0.75, 1.0, 0.95, 1.0], ..wheel() };
        let tarmac = Wheel { class_friction: [0.9, 0.9, 1.0, 0.525, 0.9, 0.95], ..wheel() };
        assert!(snow.is_studded() && !tarmac.is_studded() && !wheel().is_studded());
        let ice = ForceInput { grip: 0.1, surface_classes: [crate::tuning::ICE_CLASS; 2], ..rolling(20.0, 0.5) };
        let mu = |w: &Wheel, x: &ForceInput| {
            let out = settle(&mut Brush::new(std::slice::from_ref(w), &Tuning::default()).unwrap(), x, 2000);
            f64::from(out.lateral_force.abs() / x.normal_force)
        };
        assert!(mu(&tarmac, &ice) < 0.12, "{}", mu(&tarmac, &ice));
        assert!((0.6..0.9).contains(&mu(&snow, &ice)), "{}", mu(&snow, &ice));
        // Off the ice the studs change nothing.
        let road = ForceInput { surface_classes: [brush::TARMAC_CLASS; 2], ..ice };
        assert_eq!(mu(&snow, &road), mu(&tarmac, &road));
    }

    #[test]
    fn the_damp_tarmac_tyre_takes_the_dry_tyre_s_factor_times_r_on_tarmac_only() {
        // NGP's tarmac tyres' surface words (probe `tyre_classes`): damp (kind 1), damp gravel (kind 4).
        let damp = Wheel {
            class_friction: [0.925, 0.925, 1.0, 0.55, 0.925, 0.975],
            class_wet_slope: [-0.075, -0.075, -0.075, 0.0, -0.075, -0.075],
            design_wetness: 0.5,
            ..wheel()
        };
        let gravel = Wheel { class_friction: [1.0, 1.0, 0.85, 0.625, 1.0, 1.0], ..damp.clone() };
        assert!(damp.is_damp_tarmac() && !gravel.is_damp_tarmac() && !wheel().is_damp_tarmac());
        let lateral = |w: &Wheel, x: &ForceInput| {
            f64::from(
                settle(&mut Brush::new(std::slice::from_ref(w), &Tuning::default()).unwrap(), x, 2000).lateral_force,
            )
        };
        // Sliding whole (20°), the force follows the grip: the tarmac share takes the level, nothing else moves.
        for (water_factor, blend, level) in [(0.0, 0.0, 0.98125 / 0.9625), (0.25, 0.0, 1.0 / 0.925), (0.25, 0.5, 0.0)] {
            let x = ForceInput {
                water_factor,
                surface_classes: [brush::TARMAC_CLASS, 0],
                surface_blend: blend,
                ..rolling(15.0, 20f32.to_radians())
            };
            let level = if blend == 0.0 { level } else { 1.0 + (1.0 / 0.925 - 1.0) / 2.0 };
            assert!(
                (lateral(&damp, &x) / lateral(&wheel(), &x) - level).abs() < 0.01,
                "W {water_factor} blend {blend}"
            );
            assert_eq!(lateral(&gravel, &x), lateral(&wheel(), &x));
        }
        let x = ForceInput { water_factor: 0.25, surface_classes: [0, 0], ..rolling(15.0, 20f32.to_radians()) };
        assert_eq!(lateral(&damp, &x), lateral(&wheel(), &x));
    }

    /// The wheel's output after `n` steps of `x` from the state `b` holds.
    fn settle(b: &mut Brush, x: &ForceInput, n: usize) -> ForceOutput {
        (0..n).map(|_| b.forces(wheel().tyre, x).unwrap()).last().unwrap()
    }

    #[test]
    fn a_slide_heats_the_surface_on_hard_ground_and_soil_leaves_the_temperatures_to_ngp() {
        let thermal = rbr_physics::tyre::model::ThermalInput {
            rubber_grip: 1.0,
            surface_temperature: 340.0,
            carcass_temperature: 340.0,
            air_temperature: 293.0,
            surface_capacity: 307.0,
            tread_capacity: 5836.0,
            tread_depth: 0.01,
            tread_width: 0.2,
            outer_radius: 0.315,
            tread_area: 0.396,
            contact_area: 0.03,
            carcass_conductance: 613.0,
            contact_conductance: 1.0,
            rolling_power: 0.0,
        };
        let slide = ForceInput { thermal: Some(thermal), tread_temperature: 340.0, ..rolling(20.0, 0.5) };
        let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
        let [surface, bulk] = settle(&mut b, &slide, 720).temperatures.unwrap();
        assert!(surface > 360.0 && bulk > 340.0 && surface > bulk, "{surface} {bulk}");
        let soil = ForceInput { soil_thickness: 0.009, grip: 0.9, ..slide };
        assert_eq!(settle(&mut b, &soil, 10).temperatures, None);
    }

    #[test]
    fn soil_runs_the_loose_tyre_through_the_same_carcass() {
        let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
        let hard = rolling(15.0, 2f32.to_radians());
        let soil = ForceInput { soil_thickness: 0.009, grip: 0.9, ..hard };
        let on_hard = settle(&mut b, &hard, 1440);
        // The change of surface keeps the carcass deflection: below both surfaces' friction the force moves to the
        // loose tyre's over the relaxation length, not in one step (above the new friction it is cut to it at once).
        let first = b.forces(wheel().tyre, &soil).unwrap();
        let on_soil = settle(&mut b, &soil, 1440);
        let gap = (on_soil.lateral_force - on_hard.lateral_force).abs();
        assert!((first.lateral_force - on_hard.lateral_force).abs() < 0.2 * gap, "{first:?} {on_hard:?} {on_soil:?}");
        let e = EvalInput {
            fz: 4000.0,
            half_length: 0.05,
            vx: f64::from(soil.longitudinal_velocity),
            vy: f64::from(soil.lateral_velocity),
            spin: f64::from(soil.spin),
            radius: 0.3,
            bulk_temp: 350.0,
            tread_remaining: 1.0,
            pressure: 0.0,
            pressure_factor: 1.0,
            wetness: 0.0,
            hydroplaning: false,
            temp_grip: 1.0,
            surf_static: 0.9,
            surf_kinetic: 0.9,
        };
        let steady = loose::eval(&e, &b.tyres[0].loose, 0.0, 0.0, 0.0);
        assert!((f64::from(on_soil.lateral_force) / steady.fy - 1.0).abs() < 1e-4, "{on_soil:?} {steady:?}");
        let back = b.forces(wheel().tyre, &hard).unwrap();
        assert!((back.lateral_force - on_soil.lateral_force).abs() < 0.2 * gap, "{back:?} {on_soil:?} {on_hard:?}");
    }

    #[test]
    fn a_locked_wheel_on_soil_stops_and_holds_without_creep() {
        let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
        let soil = |speed: f32| ForceInput {
            soil_thickness: 0.009,
            grip: 0.9,
            spin: 0.0,
            torsional_stiffness: 2000.0,
            ..rolling(speed, 0.0)
        };
        // Sliding locked at 10 m/s the berm brakes it at the plough level; held, the force stays as it was, and with
        // no sliding at all it neither grows nor flips.
        let sliding = settle(&mut b, &soil(10.0), 720);
        let level = b.tyres[0].loose.levels.at(4000.0).1 * 0.9 * 4000.0;
        assert!((f64::from(sliding.longitudinal_force) / level - loose::PLOUGH).abs() < 1e-3, "{sliding:?}");
        let crawl = settle(&mut b, &soil(0.05), 720);
        assert!(crawl.longitudinal_force > 0.0 && crawl.longitudinal_force < sliding.longitudinal_force, "{crawl:?}");
        let held = b.forces(wheel().tyre, &soil(0.0)).unwrap();
        let rest: Vec<f32> = (0..720).map(|_| b.forces(wheel().tyre, &soil(0.0)).unwrap().longitudinal_force).collect();
        assert!(rest.iter().all(|&f| f == held.longitudinal_force), "{held:?}");
    }

    #[test]
    fn on_ice_the_side_force_builds_over_a_wider_slip_angle() {
        // The settled side force over its friction limit at 1° and 4°. On ice (0.095) the slip stiffness is NGP's
        // slope's times 0.095/0.55, on tarmac the brush's (1.4 times that here, for 1.3 times the slope): over the
        // friction ice's stays 1.2/(0.55 · 1.4) = 1.6 times tarmac's (without the fall 9 times, and 1° would slide
        // whole), so ice takes about 0.6 of tarmac's slip angle to its peak.
        let share = |grip: f32, deg: f32| {
            let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
            let x = ForceInput { grip, ..rolling(15.0, deg.to_radians()) };
            let f = (0..1440).map(|_| b.forces(wheel().tyre, &x).unwrap()).last().unwrap();
            f.lateral_force.abs() / (grip * x.normal_force)
        };
        let (tarmac, ice) = ((share(1.2, 1.0), share(1.2, 4.0)), (share(0.095, 1.0), share(0.095, 4.0)));
        let ratio = ice.0 / tarmac.0;
        assert!(ice.0 < 0.6 && (1.4..1.8).contains(&ratio), "tarmac {tarmac:?} ice {ice:?}");
        assert!(ice.1 > 0.9 && tarmac.1 < 0.9, "tarmac {tarmac:?} ice {ice:?}");
    }

    #[test]
    fn negative_camber_pushes_inwards_while_rolling_only() {
        // The wheel's frame: a positive lateral velocity is outwards and gives a negative (inward) force. Negative
        // camber pushes inwards as 0.05 of its sine in slip would: -2° as 0.1° of slip angle.
        let settled = |x: ForceInput| {
            let mut b = Brush::new(&[wheel()], &Tuning::default()).unwrap();
            (0..1440).map(|_| b.forces(wheel().tyre, &x).unwrap()).last().unwrap().lateral_force
        };
        let incl = (-2f32).to_radians().sin();
        let at = |deg: f32, sin: f32| settled(ForceInput { inclination_sin: sin, ..rolling(15.0, deg.to_radians()) });
        assert_eq!(at(0.0, 0.0), 0.0);
        let (thrust, slip) = (at(0.0, incl), at(0.1, 0.0));
        assert!(thrust < 0.0 && (thrust / slip - 1.0).abs() < 0.01, "{thrust} {slip}");
        // Past the peak the patch slides whole and the camber adds nothing (but its share of the sliding speed).
        let (slide, upright) = (at(20.0, incl), at(20.0, 0.0));
        assert!((slide / upright - 1.0).abs() < 0.002, "{slide} {upright}");
        // At rest the camber builds no deflection.
        assert_eq!(settled(ForceInput { inclination_sin: incl, ..rolling(0.0, 0.0) }), 0.0);
    }

    #[test]
    fn the_names_build_their_models() {
        assert!(crate::model("ngp", &[wheel()], &Tuning::default()).unwrap().is_none());
        assert!(crate::model("brush", &[wheel()], &Tuning::default()).unwrap().is_some());
        assert!(crate::model("magic", &[wheel()], &Tuning::default()).is_err());
    }
}
