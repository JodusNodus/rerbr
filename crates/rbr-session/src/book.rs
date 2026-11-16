//! The best laps of a circuit (ours): per course (the stage id) and profile, the best lap and its sector times, in the
//! save folder's [`FILE`]. The stage records (`rbr_gameplay::record`) are one start, N splits, one finish: a lap is
//! kept apart from them.
//!
//! Text, one section per course: `[<stage>]` and `best.<profile> = <car> <lap> <sector>...`, times in s.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use rbr_formats::saves::write_atomic;

/// The file in the save folder.
pub const FILE: &str = "lap-records.ini";

/// One profile's best lap on a course.
#[derive(Debug, Clone, PartialEq)]
pub struct BestLap {
    pub car: u32,
    pub lap: f32,
    /// The sector times, since the lap began.
    pub sectors: Vec<f32>,
}

/// The best laps of every course and profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LapBook {
    courses: BTreeMap<u32, BTreeMap<String, BestLap>>,
}

impl LapBook {
    pub fn parse(text: &str) -> LapBook {
        let mut book = LapBook::default();
        let mut course = None;
        for line in text.lines().map(str::trim) {
            if let Some(id) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                course = id.trim().parse().ok();
                continue;
            }
            let (Some(course), Some((key, value))) = (course, line.split_once('=')) else { continue };
            let Some(profile) = key.trim().strip_prefix("best.") else { continue };
            let mut words = value.split_whitespace();
            let Some(car) = words.next().and_then(|w| w.parse().ok()) else { continue };
            let times: Option<Vec<f32>> = words.map(|w| w.parse().ok().filter(|t: &f32| t.is_finite())).collect();
            let Some((&lap, sectors)) = times.as_deref().and_then(<[f32]>::split_first) else { continue };
            book.courses
                .entry(course)
                .or_default()
                .insert(profile.to_owned(), BestLap { car, lap, sectors: sectors.to_vec() });
        }
        book
    }

    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for (course, bests) in &self.courses {
            text += &format!("[{course}]\n");
            for (profile, best) in bests {
                let times: String =
                    std::iter::once(best.lap).chain(best.sectors.iter().copied()).map(|t| format!(" {t}")).collect();
                text += &format!("best.{profile} = {}{times}\n", best.car);
            }
        }
        text
    }

    /// The file `dir` holds, or an empty book without one.
    pub fn load(dir: &Path) -> io::Result<LapBook> {
        match std::fs::read_to_string(dir.join(FILE)) {
            Ok(text) => Ok(LapBook::parse(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(LapBook::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        write_atomic(&dir.join(FILE), self.to_text().as_bytes())
    }

    pub fn best(&self, course: u32, profile: &str) -> Option<&BestLap> {
        self.courses.get(&course)?.get(profile)
    }

    /// Keeps `lap` as the profile's best on the course when it is faster than the one kept (or the first). Whether it was kept.
    pub fn submit(&mut self, course: u32, profile: &str, lap: BestLap) -> bool {
        let bests = self.courses.entry(course).or_default();
        if bests.get(profile).is_some_and(|best| best.lap <= lap.lap) {
            return false;
        }
        bests.insert(profile.to_owned(), lap);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(car: u32, time: f32) -> BestLap {
        BestLap { car, lap: time, sectors: vec![time / 3.0, time * 2.0 / 3.0] }
    }

    #[test]
    fn the_book_round_trips_and_ignores_what_it_cannot_read() {
        let mut book = LapBook::default();
        assert!(book.submit(1654149, "Ann", lap(2, 440.25)));
        assert!(book.submit(1654149, "Bo Peep", lap(5, 450.5)));
        assert!(book.submit(77, "Ann", BestLap { car: 1, lap: 61.5, sectors: Vec::new() }));
        assert_eq!(LapBook::parse(&book.to_text()), book);
        let noisy = format!("junk\n{}best.Cy = x 1 2\nbest.Di = 3 nan\n[x]\nbest.Ed = 1 2\n", book.to_text());
        assert_eq!(LapBook::parse(&noisy), book);
    }

    #[test]
    fn only_a_faster_lap_replaces_the_best() {
        let mut book = LapBook::default();
        assert!(book.submit(1, "Ann", lap(2, 100.0)));
        assert!(!book.submit(1, "Ann", lap(3, 100.0)), "an equal lap keeps the first");
        assert!(!book.submit(1, "Ann", lap(3, 101.0)));
        assert!(book.submit(1, "Ann", lap(3, 99.0)));
        assert_eq!(book.best(1, "Ann"), Some(&lap(3, 99.0)));
        assert_eq!(book.best(1, "Bo"), None);
        assert_eq!(book.best(2, "Ann"), None);
    }

    #[test]
    fn the_file_loads_empty_when_missing_and_saves_whole() {
        let dir = std::env::temp_dir().join(format!("rbr-lap-book-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(LapBook::load(&dir).unwrap(), LapBook::default());
        let mut book = LapBook::default();
        book.submit(9, "Ann", lap(2, 10.0));
        book.save(&dir).unwrap();
        assert_eq!(LapBook::load(&dir).unwrap(), book);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
