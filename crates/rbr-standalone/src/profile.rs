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
    /// The aero model (`rbr_hybrid::ngp::set_aero_model`): `ngp` is NGP's downforce and drag, in every profile;
    /// `--aero yaw` (a prototype) takes the front downforce and the drag with the car's sideslip.
    pub aero: &'static str,
}

impl Profile {
    /// NGP 7's physics, as the original steps it: nothing replaced (the default).
    pub const NGP: Profile =
        Profile { name: "ngp", engine: "ngp", tyre: "ngp", tyre_tuning: Tuning::MODEL, aero: "ngp" };
    /// ReRBR experimental physics, built on NGP 7: its engine model (released; the launcher's Settings > Physics).
    pub const EXPERIMENTAL: Profile =
        Profile { name: "experimental", engine: "realistic", tyre: "ngp", tyre_tuning: Tuning::MODEL, aero: "ngp" };
    /// The experimental physics with the brush tyre: in development, not released. Only `--physics
    /// experimental-brush` reaches it; the launcher does not offer it.
    pub const EXPERIMENTAL_BRUSH: Profile = Profile {
        name: "experimental-brush",
        engine: "realistic",
        tyre: "brush",
        tyre_tuning: Tuning::MODEL,
        aero: "ngp",
    };
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
        assert_eq!(Profile::named("realistic"), None);
    }
}
