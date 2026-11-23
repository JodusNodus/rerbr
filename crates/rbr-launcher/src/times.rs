//! Your times (ours): every finished stage run, kept by stage, car, road and physics (`rbr_recording::physics`), so a
//! run is ranked only with runs on the same road under the same physics, and by class across cars.

/// The road a run was driven on: the weather's surface words (`Weather::mat_words`, the viewer's `--weather`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Road {
    Dry,
    Damp,
    Wet,
}

impl Road {
    pub const ALL: [Road; 3] = [Road::Dry, Road::Damp, Road::Wet];

    /// The viewer's `--weather` word.
    pub fn parse(word: &str) -> Option<Road> {
        Road::ALL.into_iter().find(|road| road.word() == word)
    }

    pub fn word(self) -> &'static str {
        match self {
            Road::Dry => "dry",
            Road::Damp => "damp",
            Road::Wet => "wet",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Road::Dry => "Dry",
            Road::Damp => "Damp",
            Road::Wet => "Wet",
        }
    }
}

/// A car's class as the times rank it: its group (`Car::group`), or "Other" without one.
pub fn class_of(group: Option<&str>) -> String {
    group.map(str::trim).filter(|g| !g.is_empty()).unwrap_or("Other").to_owned()
}

/// `text` as a folder name on every system: ASCII letters, digits, `.`, `@` and `-`, the rest as `_`.
pub fn slug(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_alphanumeric() || ".@-".contains(c) { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roads_classes_and_folder_names() {
        for road in Road::ALL {
            assert_eq!(Road::parse(road.word()), Some(road));
        }
        assert_eq!(Road::parse("snow"), None);
        assert_eq!(class_of(Some("Rally 2")), "Rally 2");
        assert_eq!(class_of(Some(" ")), "Other");
        assert_eq!(class_of(None), "Other");
        assert_eq!(slug("Group A/B: 4WD"), "Group_A_B__4WD");
        assert_eq!(slug("experimental@0.1.0"), "experimental@0.1.0");
    }
}
