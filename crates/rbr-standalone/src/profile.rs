//! Physics profiles (`--physics`): NGP 7's, or ReRBR experimental physics (built on NGP 7), each naming the model of
//! every system ReRBR can replace. A new system is one more field, set in each profile.

use rbr_tyre::Tuning;

/// The model of each system a run steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// `--physics`'s name for it.
    pub name: &'static str,
    /// The engine model (`rbr_engine::model`): `ngp` is NGP's torque formula.
    pub engine: &'static str,
    /// The tyre model (`rbr_tyre::model`): `ngp` is NGP's sliding model of the contact patch.
    pub tyre: &'static str,
    /// `--tyre-tuning`: the brush's experimental tuning (`rbr_tyre::Tuning`), the model's in every profile.
    pub tyre_tuning: Tuning,
    /// The aero model (`rbr_hybrid::ngp::set_aero_model`): `ngp` is NGP's downforce and drag; `--aero` sets another:
    /// `yaw` takes the front downforce and the drag with the car's sideslip, `wings` adds the drag of the downforce
    /// devices, `downforce` halves the file's downforce, and their sums joined by `+`.
    pub aero: &'static str,
    /// The four-wheel-drive cars' drivetrain loss, percent (`rbr_hybrid::ngp::set_four_wheel_drive_loss`): `None` is
    /// the car file's `DriveTrainLoss_NGP` (12).
    pub drive_loss: Option<u32>,
}

impl Profile {
    /// NGP 7's physics, as the original steps it: nothing replaced (the default).
    pub const NGP: Profile =
        Profile { name: "ngp", engine: "ngp", tyre: "ngp", tyre_tuning: Tuning::MODEL, aero: "ngp", drive_loss: None };
    /// ReRBR experimental physics, built on NGP 7 (released; the launcher's Settings > Physics): its engine model, the
    /// aero fitted on the real cars (`wings+downforce`), and four-wheel-drive cars that lose 17 % from the engine's
    /// whole torque to the road, the low end of the 2017-21 World Rally Cars' 17-20 % (`notes/engine-model.md`,
    /// "Wheel power"; user 2026-11-11).
    pub const EXPERIMENTAL: Profile = Profile {
        name: "experimental",
        engine: "realistic",
        tyre: "ngp",
        tyre_tuning: Tuning::MODEL,
        aero: "wings+downforce",
        drive_loss: Some(17),
    };
    /// The experimental physics with the brush tyre: in development, not released. Only `--physics
    /// experimental-brush` reaches it; the launcher does not offer it. Gravel's level is fitted with the profile's
    /// aero (`notes/tyre-model.md`, "Gravel's level by axis and by depth").
    pub const EXPERIMENTAL_BRUSH: Profile =
        Profile { name: "experimental-brush", tyre: "brush", ..Profile::EXPERIMENTAL };
    /// The profiles `--physics` takes.
    pub const ALL: [Profile; 3] = [Profile::NGP, Profile::EXPERIMENTAL, Profile::EXPERIMENTAL_BRUSH];

    /// The profile `name` names.
    pub fn named(name: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.name == name)
    }

    /// Whether it is NGP 7's.
    pub fn is_ngp(self) -> bool {
        self == Profile::NGP
    }
}

impl Default for Profile {
    fn default() -> Profile {
        Profile::NGP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profiles_name_each_system_s_model() {
        assert_eq!(Profile::named("ngp"), Some(Profile::NGP));
        assert!(Profile::default().is_ngp());
        let experimental = Profile::named("experimental").unwrap();
        assert!(!experimental.is_ngp());
        assert!(rbr_engine::NAMES.contains(&experimental.engine) && experimental.engine != "ngp");
        assert_eq!(experimental.tyre, "ngp", "the brush tyre is not released");
        let brush = Profile::named("experimental-brush").unwrap();
        assert_eq!(brush.engine, experimental.engine);
        assert!(rbr_tyre::NAMES.contains(&brush.tyre) && brush.tyre != "ngp");
        assert_eq!(brush, Profile { name: brush.name, tyre: brush.tyre, ..experimental }, "the tyre alone differs");
        assert!(rbr_hybrid::ngp::AERO_NAMES.contains(&experimental.aero) && experimental.aero != "ngp");
        assert_eq!((Profile::NGP.drive_loss, experimental.drive_loss), (None, Some(17)));
        assert_eq!(Profile::named("realistic"), None);
    }
}
