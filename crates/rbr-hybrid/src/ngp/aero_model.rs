//! The aero model (ours, experimental, a prototype) of the runs on this thread: with `yaw` set, NGP's downforce update
//! ([`super::steps`]) scales its front downforce and the drag factor with the car's sideslip; with none (the default)
//! both are NGP's, which keeps the front downforce and the drag of a car that slides.
//!
//! The data is Dominy & Richardson, SAE 2004-01-3508 (a 20 % model of a generic 2004 World Rally Car on a moving
//! ground, to 15 degrees of slip, three ground clearances; `notes/tyre-model.md`, Aero): the front axle's downforce
//! falls about in line with the slip and is 19 %, 14 % and -6 % of its straight value at 15 degrees (Fig. 14b), the
//! rear's stays (Fig. 14c), and the drag along the path rises 21-54 % (Fig. 10).

use std::cell::Cell;

/// The models' names (`ngp`: NGP's own).
pub const AERO_NAMES: [&str; 2] = ["ngp", "yaw"];

/// The sideslip at which the front downforce is gone, degrees: the mean of the three clearances' straight lines
/// through their 0 and 15 degree values (18.6, 17.5, 14.2).
const FRONT_GONE_DEG: f32 = 17.0;
/// The drag's rise per degree of sideslip: the three clearances' mean rise at 15 degrees (35 %).
const DRAG_PER_DEG: f32 = 0.35 / 15.0;
/// The sideslip the measurements end at, degrees: the drag holds its value past it.
const MEASURED_DEG: f32 = 15.0;
/// The speed below which the car's direction of travel has no meaning, m/s.
const MIN_SPEED: f32 = 1.0;

thread_local! {
    static YAW: Cell<bool> = const { Cell::new(false) };
}

/// The aero model the runs on this thread step from now on, by name ([`AERO_NAMES`]).
pub fn set_aero_model(name: &str) -> Result<(), String> {
    match name {
        "ngp" => YAW.set(false),
        "yaw" => YAW.set(true),
        _ => return Err(format!("aero model {name}: one of {}", AERO_NAMES.join(", "))),
    }
    Ok(())
}

/// The factors on the front downforce and on the drag at `sideslip_deg` of sideslip (unsigned).
pub fn yaw_factors(sideslip_deg: f32) -> (f32, f32) {
    let front = (1.0 - sideslip_deg / FRONT_GONE_DEG).max(0.0);
    let drag = 1.0 + DRAG_PER_DEG * sideslip_deg.min(MEASURED_DEG);
    (front, drag)
}

/// The set model's factors for a car with `forward` m/s along its axis of `speed` m/s (`None` without a model).
pub(super) fn factors(forward: f32, speed: f32) -> Option<(f32, f32)> {
    if !YAW.get() {
        return None;
    }
    if !(speed > MIN_SPEED) {
        return Some((1.0, 1.0));
    }
    Some(yaw_factors((forward.abs() / speed).min(1.0).acos().to_degrees()))
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
    fn no_model_no_factors_and_a_slow_car_has_no_sideslip() {
        assert_eq!(factors(10.0, 20.0), None);
        set_aero_model("yaw").unwrap();
        assert_eq!(factors(0.1, 0.5), Some((1.0, 1.0)));
        assert_eq!(factors(-30.0, 30.0), Some((1.0, 1.0)));
        let (front, drag) = factors(30.0 * 10f32.to_radians().cos(), 30.0).unwrap();
        assert!((front - (1.0 - 10.0 / 17.0)).abs() < 1e-3 && (drag - 1.2333).abs() < 1e-3, "{front} {drag}");
        assert!(set_aero_model("wing").is_err());
        set_aero_model("ngp").unwrap();
        assert_eq!(factors(10.0, 20.0), None);
    }
}
