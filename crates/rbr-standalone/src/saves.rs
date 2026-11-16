//! The stage record and the personal best of the player's save files (`--saves DIR [--profile NAME]`): the standing the
//! split display and the finish panel compare with, and the new best written at the race's end
//! (`rbr_gameplay::record`). `DIR` holds `pfresult.rst` and `pf<name>.rbr`, as the game's `SavedGames` does.

use std::path::Path;

use rbr_formats::saves::profile::{InputFilter, ProfileRecord};
use rbr_formats::saves::results::ResultRecord;
use rbr_formats::saves::SaveDir;
use rbr_formats::times::StageTimes;
use rbr_gameplay::record::{self, Book, Finish, Update};
use rbr_recording::splits::{Mark, Standing, Verdict};
use rbr_school::{lesson, Lesson};
use rbr_session::{BestLap, LapBook};

/// One stage's records, loaded when the race starts.
pub struct Saves {
    dir: SaveDir,
    profile: String,
    book: Book,
    /// The best laps of a circuit session (`rbr_session::book`).
    laps: LapBook,
    stage: u32,
    car: u32,
    /// The record's row when the race started: what the splits are compared with, whatever the race writes.
    before: ResultRecord,
    /// What the race wrote, once it has finished.
    result: Option<(Update, f32)>,
    /// The files are not written (a replay's finish only compares).
    dry: bool,
}

impl Saves {
    /// Loads the records of `stage` for the profile `profile` (the first profile in `dir` without one named, else `player`, the launcher's default).
    pub fn open(dir: &Path, profile: Option<&str>, times: &[StageTimes], stage: u32, car: u32) -> Result<Self, String> {
        let dir = SaveDir { path: dir.to_path_buf() };
        let profile = match profile {
            Some(name) => name.to_owned(),
            None => {
                dir.profiles().map_err(|e| e.to_string())?.into_iter().next().unwrap_or_else(|| "player".to_owned())
            }
        };
        let book = Book::load(&dir, &profile, times)?;
        let before = record::stage_row(&book.table, stage);
        let laps = LapBook::load(&dir.path).map_err(|e| format!("{}: {e}", rbr_session::book::FILE))?;
        Ok(Self { dir, profile, book, laps, stage, car, before, result: None, dry: false })
    }

    /// The records compared as the race ends, the files not written (a replay).
    pub fn dry(self) -> Self {
        Self { dry: true, ..self }
    }

    /// The profile's input filter grid (`E16Filter::ReadBin` 0x48bd60, as `E20ReadProfile` reads it), when the profile
    /// file exists.
    pub fn filter(&self) -> Option<InputFilter> {
        self.dir.load_profile(&self.profile).ok().map(|file| file.filter)
    }

    /// The grid a profile this race creates is written with (the profile file is absent until then).
    pub fn set_new_filter(&mut self, filter: InputFilter) {
        if let Some(file) = &mut self.book.file {
            file.filter = filter;
        }
    }

    /// The profile's best lap on this circuit, from the files.
    pub fn best_lap(&self) -> Option<&BestLap> {
        self.laps.best(self.stage, &self.profile)
    }

    /// Offers a completed lap to the best laps and writes the file when it is the best (unless dry). Whether it was.
    pub fn lap(&mut self, lap: f32, sectors: &[f32]) -> Result<bool, String> {
        let best = BestLap { car: self.car, lap, sectors: sectors.to_vec() };
        let kept = self.laps.submit(self.stage, &self.profile, best);
        if kept && !self.dry {
            self.laps.save(&self.dir.path).map_err(|e| format!("{}: {e}", rbr_session::book::FILE))?;
        }
        Ok(kept)
    }

    /// The splits' and the finish's reference times, from the record before the race.
    pub fn reference(&self) -> [f32; 3] {
        self.before.times
    }

    /// The race has been written to the files.
    pub fn written(&self) -> bool {
        self.result.is_some()
    }

    /// Re-arms the saves after an in-process restart (`Live::restart`): the next finish writes again, compared
    /// against the record as it now stands. A fresh boot's book has no submitted race (`community_submitted`).
    pub fn rearm(&mut self) {
        self.result = None;
        self.book.community_submitted = false;
        self.before = record::stage_row(&self.book.table, self.stage);
    }

    /// Offers the finished stage to the files: the personal best and the record, and writes what changed. Once.
    pub fn finish(&mut self, times: [f32; 3]) -> Result<Update, String> {
        let finish = Finish { stage: self.stage, car: self.car, times };
        let update = self.book.submit(&finish);
        self.result = Some((update, times[2]));
        if !self.dry {
            self.book.save(&self.dir, &self.profile, update)?;
        }
        Ok(update)
    }

    /// A rally school lesson's verdict into the profile's school results (`lesson::award`), written unless dry: the
    /// results after it.
    pub fn school(&mut self, lesson: Lesson, verdict: u32) -> Result<[u32; 17], String> {
        let file = self.book.file.as_mut().ok_or("no profile file")?;
        let at = (file.profiles.current as usize).min(file.profiles.list.len().saturating_sub(1));
        let entry = file.profiles.list.get_mut(at).ok_or("the profile file has no profile")?;
        let results = lesson::award(entry.school, lesson, verdict);
        if results != entry.school {
            entry.school = results;
            if !self.dry {
                self.dir.save_profile(&self.profile, file).map_err(|e| format!("pf{}.rbr: {e}", self.profile))?;
            }
        }
        Ok(results)
    }

    /// The record, the personal best and the result page's message.
    pub fn standing(&self) -> Standing {
        let (row, best) = self.book.standing(self.stage);
        let verdict = match self.result {
            Some((_, time)) => match record::verdict(&row, best.as_ref(), time) {
                record::Verdict::None => Verdict::None,
                record::Verdict::PersonalBest => Verdict::PersonalBest,
                record::Verdict::StageRecord => Verdict::StageRecord,
            },
            None => Verdict::None,
        };
        let mark = |car, splits, time, holder: String| Mark { car, splits, time, holder };
        Standing {
            record: mark(row.w04, [row.times[0], row.times[1]], row.times[2], row.name()),
            personal: best.map(|b: ProfileRecord| mark(b.w04, [b.f08[0], b.f08[1]], b.f08[2], String::new())),
            verdict,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rearm` after a finish: the next finish writes again, compared against the record as it now stands.
    #[test]
    fn rearm_writes_the_next_finish_against_the_new_record() {
        let dir = std::env::temp_dir().join(format!("rbr-saves-rearm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let times = vec![StageTimes { stage: 11, splits: [60.0, 120.0], finish: 185.0 }];
        let mut saves = Saves::open(&dir, Some("Ann"), &times, 11, 0).unwrap();
        assert_eq!(saves.reference(), [60.0, 120.0, 185.0]);
        saves.finish([50.0, 100.0, 150.0]).unwrap();
        assert!(saves.written());
        assert!(dir.join("pfresult.rst").is_file());
        saves.rearm();
        assert!(!saves.written());
        assert_eq!(saves.reference(), [50.0, 100.0, 150.0]);
        // Without the re-arm the second finish would be dropped (`race_end` checks `written`).
        saves.finish([51.0, 101.0, 151.0]).unwrap();
        assert!(saves.written());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A dry run (a replay, an experimental engine model) compares its finish with the record and writes no file.
    #[test]
    fn a_dry_finish_is_compared_and_not_written() {
        let dir = std::env::temp_dir().join(format!("rbr-saves-dry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let times = vec![StageTimes { stage: 11, splits: [60.0, 120.0], finish: 185.0 }];
        let mut saves = Saves::open(&dir, Some("Ann"), &times, 11, 0).unwrap().dry();
        saves.finish([50.0, 100.0, 150.0]).unwrap();
        assert_eq!(saves.standing().verdict, Verdict::StageRecord);
        assert!(!dir.join("pfresult.rst").exists() && !dir.join("pfAnn.rbr").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A circuit's best lap is kept per profile in its own file, read again on the next open; a dry run keeps none.
    #[test]
    fn the_best_lap_is_kept_for_the_next_session_unless_dry() {
        let dir = std::env::temp_dir().join(format!("rbr-saves-laps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let open = || Saves::open(&dir, Some("Ann"), &[], 1654149, 2).unwrap();
        let mut saves = open();
        assert!(saves.best_lap().is_none());
        assert!(saves.lap(450.0, &[150.0, 300.0]).unwrap());
        assert!(!saves.lap(451.0, &[150.0, 300.0]).unwrap());
        assert!(saves.lap(449.5, &[149.0, 299.0]).unwrap());
        let best = open().best_lap().cloned().unwrap();
        assert_eq!((best.car, best.lap, best.sectors), (2, 449.5, vec![149.0, 299.0]));
        let mut dry = open().dry();
        assert!(dry.lap(400.0, &[]).unwrap());
        assert_eq!(open().best_lap().map(|b| b.lap), Some(449.5));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
