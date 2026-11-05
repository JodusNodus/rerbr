//! The stage pieces a rally takes from 1.02: the tyres on offer ([`tyre_choices`]) and a stage's end as
//! `rbr-standalone --result-out` reports it ([`StageReport`]).

use rbr_gameplay::flow::{EndCause, EndRace, StageEnd};

/// The tyres the player may pick after the setup choice, the dry one first.
/// Tarmac: dry, intermediate, wet (0-2); gravel: dry, normal, wet (3-5);
/// snow: snow (6). There is no tyre stock: the pick is a compound. Tyre wear
/// is carried damage into a stage without a service, and each service's
/// refresh clears it.
pub fn tyre_choices(surface: u32) -> &'static [u32] {
    match surface {
        0 => &[3, 4, 5],
        1 => &[0, 1, 2],
        _ => &[6],
    }
}

/// A stage's end as the rally takes it from `rbr-standalone --result-out` (ours: `key=value` lines `finished`,
/// `time`, `split1`, `split2`, `drivable`, `cause`, `false_start`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageReport {
    /// The race's end: the `CSigEndRace` payload (the HUD's race time with the penalties and its two splits) the
    /// season takes in mode 3.
    pub end: StageEnd,
    /// `CCarDamageHandler::Test3B20` on the car at the end: the engine runs and two wheels are on.
    pub drivable: bool,
    /// The race had a jump start: the season counts it (`IncT174` 0x5ce2e0, the rally's +0x174).
    pub false_start: bool,
}

impl StageReport {
    pub fn to_text(&self) -> String {
        let race = self.end.end_race;
        let [s1, s2] = race.splits;
        format!(
            "finished={}\ntime={}\nsplit1={s1}\nsplit2={s2}\ndrivable={}\ncause={:?}\nfalse_start={}\n",
            u8::from(self.end.finished()),
            race.time,
            u8::from(self.drivable),
            self.end.cause,
            u8::from(self.false_start)
        )
    }

    /// [`StageReport::to_text`]'s lines back: a missing number reads 0, an unknown cause [`EndCause::Left`].
    pub fn parse(text: &str) -> Self {
        let value = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='));
        let get = |key: &str| -> f32 { value(key).and_then(|v| v.parse().ok()).unwrap_or(0.0) };
        let cause = match value("cause") {
            Some("Finish") => EndCause::Finish,
            Some("Marker") => EndCause::Marker,
            Some("Bystander") => EndCause::Bystander,
            Some("Offstage") => EndCause::Offstage,
            Some("Excluded") => EndCause::Excluded,
            Some("Retired") => EndCause::Retired,
            Some("FinishZone") => EndCause::FinishZone,
            _ => EndCause::Left,
        };
        let end_race =
            EndRace { time: get("time"), splits: [get("split1"), get("split2")], retired: get("finished") == 0.0 };
        Self {
            end: StageEnd { cause, end_race },
            drivable: get("drivable") != 0.0,
            false_start: get("false_start") != 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stage_report_reads_back_as_written() {
        let end_race = EndRace { time: 123.456_79, splits: [40.1, 80.25], retired: false };
        let result =
            StageReport { end: StageEnd { cause: EndCause::FinishZone, end_race }, drivable: false, false_start: true };
        assert_eq!(StageReport::parse(&result.to_text()), result);
        let left = StageReport::parse("");
        assert_eq!((left.end.cause, left.end.finished(), left.drivable), (EndCause::Left, false, false));
    }

    #[test]
    fn each_surface_offers_its_own_tyres() {
        assert_eq!(tyre_choices(0), [3, 4, 5]);
        assert_eq!(tyre_choices(1), [0, 1, 2]);
        assert_eq!(tyre_choices(2), [6]);
    }
}
