//! The aero model (ours, experimental) of the runs on this thread: NGP's downforce update ([`super::steps`]) takes
//! its factors on the file's downforce, front and rear, and on the drag factor. The experimental profiles run
//! `wings+downforce`; `yaw` is a prototype. With none (the default) all are NGP's: the file's downforce, the drag of
//! the car file's `Cd * A`, a road shell's whatever the downforce, and the front downforce and drag of a car that
//! slides.
//!
//! `yaw`: both with the car's sideslip. Dominy & Richardson, SAE 2004-01-3508 (a 20 % model of a generic 2004 World
//! Rally Car on a moving ground, to 15 degrees of slip, three ground clearances; `notes/tyre-model.md`, Aero): the
//! front axle's downforce falls about in line with the slip and is 19 %, 14 % and -6 % of its straight value at 15
//! degrees (Fig. 14b), the rear's stays (Fig. 14c), and the drag along the path rises 21-54 % (Fig. 10).
//!
//! `wings`: the drag of the downforce devices, a drag area of [`WING_DRAG`] times the square of the file's downforce
//! area (the form of induced drag), on top of the file's. The 2017-21 World Rally Cars at full throttle on straights
//! (WRC telemetry, 244 drives on tarmac; `notes/tyre-model.md`, Aero) have a drag area of 1.74 m^2 against their
//! files' 0.81 m^2 with 3.54 m^2 of downforce area; the same rule on the 2006 files gives 0.82 and 1.08 m^2 against
//! the 2004 model's 0.85.
//!
//! `downforce`: [`DOWNFORCE`] of the files' downforce on both axles, so the balance stays. The same cars' hard
//! straight braking gains with the dynamic pressure as 1.3-1.5 m^2 of downforce area would give (nine tarmac rallies,
//! their drag of 1.74 m^2 taken off), where the game's i20 Coupe WRC on the same measure reads 2.4-3.1 m^2 with the
//! file's 3.54 and 0.9-1.7 with half (`notes/tyre-model.md`, Aero). The wings' drag stays on the file's figure, which
//! it was fitted with.

use std::cell::Cell;

use rbr_physics::ngp::downforce::{REFERENCE_DENSITY, REFERENCE_SPEED};

/// The models' names (`ngp`: NGP's own).
pub const AERO_NAMES: [&str; 8] =
    ["ngp", "yaw", "wings", "wings+yaw", "downforce", "downforce+yaw", "wings+downforce", "wings+downforce+yaw"];

/// The sideslip at which the front downforce is gone, degrees: the mean of the three clearances' straight lines
/// through their 0 and 15 degree values (18.6, 17.5, 14.2).
const FRONT_GONE_DEG: f32 = 17.0;
/// The drag's rise per degree of sideslip: the three clearances' mean rise at 15 degrees (35 %).
const DRAG_PER_DEG: f32 = 0.35 / 15.0;
/// The sideslip the measurements end at, degrees: the drag holds its value past it.
const MEASURED_DEG: f32 = 15.0;
/// The speed below which the car's direction of travel has no meaning, m/s.
const MIN_SPEED: f32 = 1.0;

/// The devices' drag area over the square of the downforce area, 1/m^2: (1.74 - 0.808) / 3.54^2.
const WING_DRAG: f32 = 0.074;

/// The factor on the files' downforce: the telemetry's 1.3-1.5 m^2 over the 2.4-3.1 m^2 the game's reads as.
const DOWNFORCE: f32 = 0.5;

/// The effects set.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Model {
    yaw: bool,
    wings: bool,
    downforce: bool,
}

const NGP: Model = Model { yaw: false, wings: false, downforce: false };

thread_local! {
    static MODEL: Cell<Model> = const { Cell::new(NGP) };
}

/// The aero model the runs on this thread step from now on, by name ([`AERO_NAMES`]).
pub fn set_aero_model(name: &str) -> Result<(), String> {
    if !AERO_NAMES.contains(&name) {
        return Err(format!("aero model {name}: one of {}", AERO_NAMES.join(", ")));
    }
    let has = |effect| name.split('+').any(|part| part == effect);
    MODEL.set(Model { yaw: has("yaw"), wings: has("wings"), downforce: has("downforce") });
    Ok(())
}

/// The factors on the front downforce and on the drag at `sideslip_deg` of sideslip (unsigned).
pub fn yaw_factors(sideslip_deg: f32) -> (f32, f32) {
    let front = (1.0 - sideslip_deg / FRONT_GONE_DEG).max(0.0);
    let drag = 1.0 + DRAG_PER_DEG * sideslip_deg.min(MEASURED_DEG);
    (front, drag)
}

/// The factor on the drag of a car whose file gives `downforce` N (both axles, at the reference speed and density)
/// and a drag of `cd_area` (`Cd * A`, which NGP's drag takes at the reference density: `cd_area / rho0` m^2).
pub fn wing_drag_factor(downforce: f32, cd_area: f32) -> f32 {
    if cd_area <= 0.0 {
        return 1.0;
    }
    let lift_area = downforce / (0.5 * REFERENCE_DENSITY * REFERENCE_SPEED * REFERENCE_SPEED);
    1.0 + REFERENCE_DENSITY * WING_DRAG * lift_area * lift_area / cd_area
}

/// The model set, if any effect is.
pub(super) fn model() -> Option<Model> {
    Some(MODEL.get()).filter(|m| *m != NGP)
}

impl Model {
    /// The factors on the front downforce, on the rear's and on the drag for a car with `forward` m/s along its axis
    /// of `speed` m/s, `downforce` N and `cd_area` from its file ([`wing_drag_factor`]; the damage to its aero parts
    /// not taken).
    pub(super) fn factors(self, forward: f32, speed: f32, downforce: f32, cd_area: f32) -> (f32, f32, f32) {
        let cos = if speed > MIN_SPEED { (forward.abs() / speed).min(1.0) } else { 1.0 };
        let (front, yaw_drag) = if self.yaw { yaw_factors(cos.acos().to_degrees()) } else { (1.0, 1.0) };
        let level = if self.downforce { DOWNFORCE } else { 1.0 };
        let drag = if self.wings { yaw_drag * wing_drag_factor(downforce, cd_area) } else { yaw_drag };
        (front * level, level, drag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_factors_follow_the_measured_points() {
        assert_eq!(yaw_factors(0.0), (1.0, 1.0));
        let (front, drag) = yaw_factors(15.0);
        assert!((front - 0.118).abs() < 0.001, "{front}: between the clearances' 0.19 and -0.06");
        assert!((drag - 1.35).abs() < 1e-6);
        assert_eq!(yaw_factors(40.0), (0.0, drag), "no lift, and the drag holds past the data");
    }

    #[test]
    fn the_wing_drag_gives_the_measured_drag_area() {
        // The 2019 World Rally Car files: 3700 N, Cd * A 0.9725.
        let area = 0.9725 / REFERENCE_DENSITY * wing_drag_factor(3700.0, 0.9725);
        assert!((area - 1.74).abs() < 0.01, "{area} m^2 against the telemetry's 1.74");
        // The 2006 files (1610 N on 0.774, 1950 N on 0.989) against the 2004 model's 0.79-1.05 m^2.
        let xsara = 0.774 / REFERENCE_DENSITY * wing_drag_factor(1610.0, 0.774);
        let focus = 0.989 / REFERENCE_DENSITY * wing_drag_factor(1950.0, 0.989);
        assert!((0.79..1.1).contains(&xsara) && (0.79..1.1).contains(&focus), "{xsara} {focus}");
        assert_eq!(wing_drag_factor(0.0, 0.8), 1.0);
        assert_eq!(wing_drag_factor(500.0, 0.0), 1.0);
    }

    #[test]
    fn no_model_no_factors_and_a_slow_car_has_no_sideslip() {
        assert!(model().is_none());
        set_aero_model("yaw").unwrap();
        let yaw = model().unwrap();
        assert_eq!(yaw.factors(0.1, 0.5, 3700.0, 0.9725), (1.0, 1.0, 1.0));
        assert_eq!(yaw.factors(-30.0, 30.0, 3700.0, 0.9725), (1.0, 1.0, 1.0));
        let (front, rear, drag) = yaw.factors(30.0 * 10f32.to_radians().cos(), 30.0, 3700.0, 0.9725);
        assert!((front - (1.0 - 10.0 / 17.0)).abs() < 1e-3 && (drag - 1.2333).abs() < 1e-3, "{front} {drag}");
        assert_eq!(rear, 1.0);
        set_aero_model("wings").unwrap();
        let wings = wing_drag_factor(3700.0, 0.9725);
        assert_eq!(model().unwrap().factors(20.0, 30.0, 3700.0, 0.9725), (1.0, 1.0, wings));
        set_aero_model("wings+yaw").unwrap();
        assert_eq!(model().unwrap().factors(30.0, 30.0, 0.0, 0.8), (1.0, 1.0, 1.0));
        set_aero_model("wings+downforce").unwrap();
        assert_eq!(model().unwrap().factors(30.0, 30.0, 3700.0, 0.9725), (DOWNFORCE, DOWNFORCE, wings));
        set_aero_model("downforce+yaw").unwrap();
        let (slid, rear, _) = model().unwrap().factors(30.0 * 10f32.to_radians().cos(), 30.0, 3700.0, 0.9725);
        assert_eq!((slid, rear), (front * DOWNFORCE, DOWNFORCE));
        assert!(set_aero_model("wing").is_err());
        set_aero_model("ngp").unwrap();
        assert!(model().is_none());
    }
}
