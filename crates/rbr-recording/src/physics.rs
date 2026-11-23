//! A run's physics (ours): the `--physics` profile it stepped and the version of that profile's models, so times and
//! ghosts of different physics are never ranked together. NGP 7's version is NGP's own. Each ReRBR profile has its own
//! number, raised whenever a change moves the car differently; `rbr-standalone` pins a drive of each to its number.

/// Every `--physics` profile and its version now.
pub const VERSIONS: [(&str, &str); 4] =
    [("ngp", NGP_VERSION), ("experimental", "0.1.0"), ("experimental-brush", "0.1.0"), ("ngp-brush", "0.1.0")];

/// The only NGP read (`notes/ngp.md`).
pub const NGP_VERSION: &str = "7.4.778";

/// The version of a ReRBR profile's run recorded before the profiles had numbers.
pub const UNVERSIONED: &str = "unversioned";

/// The physics a run stepped: its profile (`ngp`, `experimental`, `experimental-brush`, `ngp-brush`) and version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PhysicsId {
    pub profile: String,
    pub version: String,
}

impl PhysicsId {
    /// The physics a run on profile `profile` steps now; `None` for a name no profile has.
    pub fn current(profile: &str) -> Option<PhysicsId> {
        let (profile, version) = VERSIONS.iter().find(|(name, _)| *name == profile)?;
        Some(PhysicsId { profile: (*profile).to_owned(), version: (*version).to_owned() })
    }

    /// The physics of a run on `profile` recorded before the profiles had numbers: NGP's is NGP's version (it never
    /// changes), a ReRBR profile's is [`UNVERSIONED`].
    pub fn unversioned(profile: &str) -> Option<PhysicsId> {
        let current = PhysicsId::current(profile)?;
        Some(match current.profile.as_str() {
            "ngp" => current,
            _ => PhysicsId { version: UNVERSIONED.to_owned(), ..current },
        })
    }

    /// Reads [`PhysicsId::text`]'s `profile@version`.
    pub fn parse(text: &str) -> Option<PhysicsId> {
        let (profile, version) = text.split_once('@')?;
        let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c));
        (valid(profile) && valid(version))
            .then(|| PhysicsId { profile: profile.to_owned(), version: version.to_owned() })
    }

    /// `profile@version`, as files keep it (`experimental@0.1.0`).
    pub fn text(&self) -> String {
        format!("{}@{}", self.profile, self.version)
    }

    /// The player's name for it: `NGP 7.4.778`, `ReRBR engine 0.1.0`, `ReRBR engine and tyre 0.1.0`,
    /// `NGP 7.4.778 with ReRBR tyre 0.1.0`; an unversioned one says "before versioning".
    pub fn label(&self) -> String {
        let version = match self.version.as_str() {
            UNVERSIONED => "before versioning".to_owned(),
            version => version.to_owned(),
        };
        match self.profile.as_str() {
            "ngp" => format!("NGP {version}"),
            "experimental" => format!("ReRBR engine {version}"),
            "experimental-brush" => format!("ReRBR engine and tyre {version}"),
            "ngp-brush" => format!("NGP {NGP_VERSION} with ReRBR tyre {version}"),
            other => format!("{other} {version}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_profile_has_a_version_written_and_read_back() {
        for (profile, version) in VERSIONS {
            let id = PhysicsId::current(profile).unwrap();
            assert_eq!(id.version, version);
            assert_eq!(PhysicsId::parse(&id.text()), Some(id.clone()));
        }
        assert_eq!(PhysicsId::current("brush"), None);
        assert_eq!(PhysicsId::current("ngp").unwrap().text(), "ngp@7.4.778");
        assert_eq!(PhysicsId::current("ngp").unwrap().label(), "NGP 7.4.778");
        for bad in ["", "ngp", "@1", "ngp@", "ngp@7 4", "a@b@c"] {
            assert_eq!(PhysicsId::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_run_from_before_the_versions_keeps_ngp_and_marks_rerbr() {
        assert_eq!(PhysicsId::unversioned("ngp"), PhysicsId::current("ngp"));
        let old = PhysicsId::unversioned("experimental-brush").unwrap();
        assert_eq!(old.text(), "experimental-brush@unversioned");
        assert_eq!(old.label(), "ReRBR engine and tyre before versioning");
        assert_ne!(Some(old), PhysicsId::current("experimental-brush"));
    }
}
