//! Physics profiles (`--physics`): NGP 7's, or ReRBR experimental physics (built on NGP 7), each naming the model of
//! every system ReRBR can replace. A new system is one more field, set in each profile.

/// The model of each system a run steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// `--physics`'s name for it.
    pub name: &'static str,
    /// The engine model (`rbr_engine::model`): `ngp` is NGP's torque formula.
    pub engine: &'static str,
    /// The tyre model (`rbr_tyre::model`): `ngp` is NGP's sliding model of the contact patch.
    pub tyre: &'static str,
}

impl Profile {
    /// NGP 7's physics, as the original steps it: nothing replaced (the default).
    pub const NGP: Profile = Profile { name: "ngp", engine: "ngp", tyre: "ngp" };
    /// ReRBR experimental physics, built on NGP 7.
    pub const EXPERIMENTAL: Profile = Profile { name: "experimental", engine: "realistic", tyre: "brush" };
    /// The profiles `--physics` takes.
    pub const ALL: [Profile; 2] = [Profile::NGP, Profile::EXPERIMENTAL];

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
        assert!(rbr_tyre::NAMES.contains(&experimental.tyre) && experimental.tyre != "ngp");
        assert_eq!(Profile::named("realistic"), None);
    }
}
