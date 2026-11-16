//! The circuit session's lap clock (ours, `notes/session.md`), from the live lap record (`rbr_recording::laps`): in a
//! circuit session the HUD's clock slot shows the lap in progress and its time, with a row for the sector just passed
//! against the best lap's, the last lap and the best, in place of the race clock, its split rows and the stage's
//! progress bar (`stock`).

use rbr_recording::laps::Laps;

use super::{clock_label, time_label};

/// What a value's colour says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Dim,
    Ahead,
    Behind,
}

/// A row under the clock: its name and its values from the right edge leftwards, each in its tone.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub values: Vec<(String, Tone)>,
}

/// The lap clock: its title over the time, and the rows under it.
#[derive(Debug, Clone, PartialEq)]
pub struct LapClock {
    pub title: String,
    pub time: String,
    pub rows: Vec<Row>,
}

/// The lap clock of `laps`; `None` before the first lap starts (the stock clock shows until then).
pub fn clock(laps: &Laps) -> Option<LapClock> {
    if laps.lap == 0 {
        return None;
    }
    let title = match (laps.laps, laps.finished) {
        (_, true) => "finished".to_owned(),
        (0, false) => format!("lap {}", laps.lap),
        (n, false) => format!("lap {} / {n}", laps.lap),
    };
    // Once finished the time shown is the last lap's.
    let time = if laps.finished { clock_label(f64::from(laps.last)) } else { clock_label(f64::from(laps.elapsed)) };
    let mut rows = Vec::new();
    if let Some((index, &time)) = laps.sectors.iter().enumerate().next_back() {
        let mut values = vec![(time_label(f64::from(time)), Tone::Normal)];
        if let Some(best) = laps.best_sectors.get(index) {
            let delta = time - best;
            let tone = if delta <= 0.0 { Tone::Ahead } else { Tone::Behind };
            values.insert(0, (format!("{delta:+.2}"), tone));
        }
        rows.push(Row { name: format!("sector {}", index + 1), values });
    }
    for (name, time) in [("last lap", laps.last), ("best lap", laps.best)] {
        if time > 0.0 {
            rows.push(Row { name: name.to_owned(), values: vec![(time_label(f64::from(time)), Tone::Normal)] });
        }
    }
    Some(LapClock { title, time, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laps() -> Laps {
        Laps {
            lap: 2,
            laps: 3,
            elapsed: 83.456,
            last: 445.25,
            best: 440.0,
            sectors: vec![150.0, 301.5],
            best_sectors: vec![149.0, 302.0],
            ..Laps::default()
        }
    }

    fn row(clock: &LapClock, name: &str) -> Vec<(String, Tone)> {
        clock.rows.iter().find(|r| r.name == name).unwrap().values.clone()
    }

    #[test]
    fn the_clock_shows_the_lap_its_time_the_sector_against_the_best_and_the_laps() {
        let clock = clock(&laps()).unwrap();
        assert_eq!((clock.title.as_str(), clock.time.as_str()), ("lap 2 / 3", "01:23.456"));
        assert_eq!(row(&clock, "sector 2"), [("-0.50".to_owned(), Tone::Ahead), ("5:01.50".to_owned(), Tone::Normal)]);
        assert_eq!(row(&clock, "last lap"), [("7:25.25".to_owned(), Tone::Normal)]);
        assert_eq!(row(&clock, "best lap"), [("7:20.00".to_owned(), Tone::Normal)]);
    }

    #[test]
    fn a_sector_slower_than_the_best_is_behind_and_one_without_a_best_has_no_gap() {
        let slower = Laps { sectors: vec![151.0], ..laps() };
        assert_eq!(row(&clock(&slower).unwrap(), "sector 1")[0], ("+2.00".to_owned(), Tone::Behind));
        let first = Laps { best_sectors: Vec::new(), ..slower };
        assert_eq!(row(&clock(&first).unwrap(), "sector 1"), [("2:31.00".to_owned(), Tone::Normal)]);
    }

    #[test]
    fn free_practice_has_no_lap_count_and_the_end_says_finished_with_the_last_lap() {
        let open = Laps { laps: 0, ..laps() };
        assert_eq!(clock(&open).unwrap().title, "lap 2");
        let done = clock(&Laps { finished: true, elapsed: 0.0, ..laps() }).unwrap();
        assert_eq!((done.title.as_str(), done.time.as_str()), ("finished", "07:25.250"));
    }

    #[test]
    fn nothing_shows_before_the_first_lap_and_no_lap_times_make_no_rows() {
        assert_eq!(clock(&Laps::default()), None);
        let first = Laps { lap: 1, elapsed: 2.0, ..Laps::default() };
        assert!(clock(&first).unwrap().rows.is_empty());
    }
}
