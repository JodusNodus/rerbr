//! Your times (ours): every finished stage run, kept by stage, car, road and physics (`rbr_recording::physics`), so a
//! run is ranked only with runs on the same road under the same physics, and by class across cars.

pub mod log;
pub mod query;

pub use log::Run;

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

/// The date of `unix` seconds in UTC, `2026-11-23` (no date crate in the workspace: the civil-from-days algorithm).
pub fn date(unix: u64) -> String {
    let z = (unix / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02}")
}

/// `text` as a folder name on every system: ASCII letters, digits, `.`, `@` and `-`, the rest as `_`.
#[allow(dead_code)] // for the per-physics folders of the ghosts, not used here
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
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_795_000_000), "2026-11-18");
        assert_eq!(slug("Group A/B: 4WD"), "Group_A_B__4WD");
        assert_eq!(slug("experimental@0.1.0"), "experimental@0.1.0");
    }
}
