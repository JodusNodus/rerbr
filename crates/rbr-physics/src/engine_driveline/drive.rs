//! The drive's accessors: the setters of the input lanes (throttle, brake, clutch), the commanded gear, and the
//! getters of the engine speed, gear and drive speed key. `tests/engine_driveline_vectors.rs` checks each against
//! recorded calls of the original.

use super::layout::{Drive, DriveState};

/// The clutch slip (1 = slipping).
pub fn set_clutch_slip(drive: &mut Drive, v: f32) {
    drive.clutch_slip.set(v);
}

/// The engine speed, rad/s.
pub fn engine_speed(state: &DriveState) -> f32 {
    state.engine_speed
}

/// The gear.
pub fn gear(state: &DriveState) -> i32 {
    state.gear
}

/// The drive speed key the diff locks read. The wheel workers set it to the speed of the driven terms; the clutch
/// torque worker locks the engine speed to it times a ratio.
pub fn drive_speed_key(state: &DriveState) -> f32 {
    state.drive_speed_key
}

/// One gear row of the setup: the ratio of the gear's teeth to the pinion's (`gear` over
/// `pinion`, each converted to float), or 0 for a row with no pinion.
pub fn gear_ratio(gear: i32, pinion: i32) -> f32 {
    if pinion == 0 {
        0.0
    } else {
        gear as f32 / pinion as f32
    }
}

/// The values derived from the drive setup for the drive definition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetupTerms {
    /// The final drive's ratio over the drop gear's.
    pub final_ratio: f32,
    /// The footbrake pressure: the rear and front pressures summed.
    pub footbrake: f32,
    /// The front and rear wheels' brake ratios: each axle's share of the footbrake pressure (0 when it is 0).
    pub front: f32,
    pub rear: f32,
}

/// The derived setup values from the final drive and drop gear ratios and the front and rear brake pressures.
/// The shares are the pressures times the footbrake's reciprocal, which a NaN footbrake does not skip.
pub fn setup_terms(final_drive: f32, drop_gear: f32, front: f32, rear: f32) -> SetupTerms {
    let footbrake = rear + front;
    let (front, rear) = if footbrake != 0.0 {
        let inverse = 1.0 / footbrake;
        (inverse * front, inverse * rear)
    } else {
        (0.0, 0.0)
    };
    SetupTerms { final_ratio: final_drive / drop_gear, footbrake, front, rear }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_footbrake_leaves_no_brake_ratios() {
        let terms = setup_terms(3.5, 1.0, 0.0, 0.0);
        assert_eq!((terms.footbrake, terms.front, terms.rear), (0.0, 0.0, 0.0));
        assert_eq!(setup_terms(3.5, 1.0, 3.0, 1.0).front, 0.75);
        assert_eq!(gear_ratio(-32, 14), -32.0 / 14.0);
        assert_eq!(gear_ratio(38, 0), 0.0);
    }
}
