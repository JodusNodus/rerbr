//! The championship's results as the page reads them: a stage's times and winners, the places a stage moved, the
//! player's neighbours in the standings, each rally's finishes and a driver's record over the championship.

use rbr_season::event::{Event, Standing};

use super::view::{driven, stage_time};

/// The player (`None`) and every entrant.
pub(super) fn drivers(event: &Event) -> impl Iterator<Item = Option<usize>> {
    std::iter::once(None).chain((0..event.field.len()).map(Some))
}

/// Every finished time on `rally`'s `stage`, fastest first.
pub(super) fn stage_times(event: &Event, rally: usize, stage: usize) -> Vec<(Option<usize>, f32)> {
    let mut times: Vec<_> = drivers(event).filter_map(|e| Some((e, stage_time(event, rally, stage, e)?))).collect();
    times.sort_by(|a, b| a.1.total_cmp(&b.1));
    times
}

/// The winner of each of `rally`'s driven stages; none where nobody finished.
pub(super) fn stage_winners(event: &Event, rally: usize) -> Vec<Option<Option<usize>>> {
    (0..driven(event, rally).len()).map(|s| stage_times(event, rally, s).first().map(|&(e, _)| e)).collect()
}

/// The places `who` gained (positive) or lost from `before` to `now`; none without a total in both.
pub(super) fn place_change(before: &[Standing], now: &[Standing], who: Option<usize>) -> Option<i32> {
    let at = |lines: &[Standing]| lines.iter().position(|l| l.entrant == who && l.total.is_some());
    Some(at(before)? as i32 - at(now)? as i32)
}

/// The player's place in the standings and the drivers either side, each with the gap to the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rivals {
    pub place: usize,
    pub ahead: Option<(Option<usize>, f32)>,
    pub behind: Option<(Option<usize>, f32)>,
}

/// The player's rivals in `lines`, while the player has a total.
pub(super) fn rivals(lines: &[Standing]) -> Option<Rivals> {
    let at = lines.iter().position(|l| l.entrant.is_none())?;
    let mine = lines[at].total?;
    let ahead = at.checked_sub(1).and_then(|k| Some((lines[k].entrant, mine - lines[k].total?)));
    let behind = lines.get(at + 1).and_then(|l| Some((l.entrant, l.total? - mine)));
    Some(Rivals { place: at + 1, ahead, behind })
}

/// A driver's result in a rally that is over: the place (none when retired) and the points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Finish {
    pub place: Option<usize>,
    pub points: u32,
}

/// Whether `rally` is over: the field drove all its stages.
pub(super) fn rally_over(event: &Event, rally: usize) -> bool {
    event.results.get(rally).is_some_and(|r| !r.is_empty() && r.iter().all(|(_, ai)| !ai.is_empty()))
}

/// `rally`'s finishes in standings order, once it is over.
pub(super) fn rally_finishes(event: &Event, rally: usize) -> Option<Vec<(Option<usize>, Finish)>> {
    rally_over(event, rally).then(|| {
        let lines = event.rally_standings(rally);
        let finish =
            |(k, l): (usize, &Standing)| (l.entrant, Finish { place: l.total.map(|_| k + 1), points: l.points });
        lines.iter().enumerate().map(finish).collect()
    })
}

/// What a driver achieved over the championship's rallies that are over.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Record {
    pub stage_wins: usize,
    pub wins: usize,
    pub podiums: usize,
    pub retirements: usize,
    pub best: Option<usize>,
}

/// `who`'s record: stage wins over every stage driven, and the finishes of the rallies that are over.
pub(super) fn record(event: &Event, who: Option<usize>) -> Record {
    let mut r = Record::default();
    for rally in 0..event.rallies.len() {
        r.stage_wins += stage_winners(event, rally).iter().filter(|w| **w == Some(who)).count();
        let finish = rally_finishes(event, rally).and_then(|f| f.into_iter().find(|(e, _)| *e == who));
        match finish.map(|(_, f)| f.place) {
            Some(Some(place)) => {
                r.wins += usize::from(place == 1);
                r.podiums += usize::from(place <= 3);
                r.best = Some(r.best.map_or(place, |b| b.min(place)));
            }
            Some(None) => r.retirements += 1,
            None => {}
        }
    }
    r
}

/// A gap as a co-driver reads it: "4.2 s" under a minute, "1:04.2" over.
pub(super) fn seconds(gap: f32) -> String {
    let gap = gap.max(0.0);
    if gap < 60.0 {
        format!("{gap:.1} s")
    } else {
        format!("{}:{:04.1}", (gap / 60.0) as u32, gap % 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::championship_page::fixture;
    use rbr_season::event::{PlayerResult, Turn};

    /// The fixture season with the player `factor` off each stage's reference, services skipped.
    fn played(stages: usize, factor: f32) -> Event {
        let master = fixture::ai().0;
        let mut event = fixture::fixture_event();
        event.turn_done();
        for _ in 0..stages {
            while !matches!(event.turn, Turn::Stage { .. }) {
                event.turn_done();
            }
            let Turn::Stage { rally, stage } = event.turn else { unreachable!() };
            let time = event.rallies[rally].stages[stage].reference * factor;
            event.stage_done(&master, PlayerResult { time, penalties: 0.0, retired: false });
        }
        event
    }

    #[test]
    fn a_stages_times_run_fastest_first_and_name_its_winner() {
        let event = played(2, 0.9);
        let times = stage_times(&event, 0, 1);
        let finished = drivers(&event).filter(|&e| stage_time(&event, 0, 1, e).is_some()).count();
        assert!(times.len() == finished && finished > 1);
        assert!(times.windows(2).all(|w| w[0].1 <= w[1].1));
        // 10 % under the reference wins every stage.
        assert_eq!(stage_winners(&event, 0), [Some(None), Some(None)]);
        assert!(stage_winners(&event, 1).is_empty());
    }

    #[test]
    fn rivals_and_place_changes_read_the_standings() {
        let line = |entrant, total| Standing { entrant, total, points: 0, bonus: 0 };
        let before = [line(Some(0), Some(100.0)), line(None, Some(104.0)), line(Some(1), Some(110.0))];
        let now = [line(None, Some(200.0)), line(Some(0), Some(203.5)), line(Some(1), None)];
        assert_eq!(place_change(&before, &now, None), Some(1));
        assert_eq!(place_change(&before, &now, Some(0)), Some(-1));
        assert_eq!(place_change(&before, &now, Some(1)), None);
        let r = rivals(&before).unwrap();
        assert_eq!((r.place, r.ahead, r.behind), (2, Some((Some(0), 4.0)), Some((Some(1), 6.0))));
        let r = rivals(&now).unwrap();
        assert_eq!((r.place, r.ahead, r.behind), (1, None, Some((Some(0), 3.5))));
        assert_eq!(rivals(&[line(None, None)]), None);
        assert_eq!([4.25, 64.25, -1.0].map(seconds), ["4.2 s", "1:04.2", "0.0 s"]);
    }

    #[test]
    fn a_record_counts_the_rallies_that_are_over() {
        let event = played(6, 0.9);
        assert!(rally_over(&event, 0) && !rally_over(&event, 1));
        let finishes = rally_finishes(&event, 0).unwrap();
        // 25 for the win, 5 for the power stage.
        assert_eq!(finishes[0], (None, Finish { place: Some(1), points: 30 }));
        assert!(rally_finishes(&event, 1).is_none());
        let mine = record(&event, None);
        assert_eq!(mine, Record { stage_wins: 6, wins: 1, podiums: 1, retirements: 0, best: Some(1) });
        let winless = record(&event, Some(0));
        assert_eq!((winless.stage_wins, winless.wins), (0, 0));
    }
}
