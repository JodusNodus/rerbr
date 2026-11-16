//! The timing of a circuit: laps and sectors from the car's distance along a closed drive line.
//!
//! The line's seam is the lap line: the distance wraps from the length back to 0 there (the closed-line search of
//! `notes/session.md`), so a pass of the line is a jump of more than half the length between two steps. The
//! caller gives each step's distance and time and gets the events back. Ours, so the passes' times are interpolated
//! between the two steps (the 1.02 timekeeper's are the step's).

use crate::{Course, EndRule, TimingEvent};

/// The lap timer of one circuit session.
#[derive(Debug, Clone, PartialEq)]
pub struct LapTiming {
    length: f32,
    sectors: Vec<f32>,
    rule: EndRule,
    /// The previous step's distance and time.
    last: Option<(f32, f64)>,
    /// The first lap starts with the next step, wherever the car is ([`Self::start_now`]): a standing start on the line.
    start_pending: bool,
    /// The time of the first pass of the line, once there was one.
    started: Option<f64>,
    /// When the lap in progress began.
    lap_start: f64,
    /// The next sector gate of the lap in progress.
    next_sector: usize,
    /// Passes of the line backwards not yet made good: the car is behind the line by this many.
    behind: u32,
    times: Vec<f32>,
    /// The sector times of the lap in progress (since the lap began).
    sector_times: Vec<f32>,
    /// The best lap to beat: the best completed lap, or the one [`Self::seed_best`] gave; with its sector times.
    best: Option<(f32, Vec<f32>)>,
    /// The session's total time once its end rule was met.
    finish_time: Option<f32>,
}

impl LapTiming {
    /// The timer of `course` ending by `rule`. `None` for a course that is not a circuit.
    pub fn new(course: &Course, rule: EndRule) -> Option<Self> {
        let Course::Circuit { length, sectors } = course else { return None };
        Some(Self {
            length: *length,
            sectors: sectors.clone(),
            rule,
            last: None,
            start_pending: false,
            started: None,
            lap_start: 0.0,
            next_sector: 0,
            behind: 0,
            times: Vec::new(),
            sector_times: Vec::new(),
            best: None,
            finish_time: None,
        })
    }

    /// A standing start: the first lap starts with the next step, the car on the line, instead of at the first pass of
    /// it (a flying start).
    pub fn start_now(&mut self) {
        self.start_pending = true;
    }

    /// One step: the car is `x` metres along the line at time `t` (s). Returns the events it passed, in order. A
    /// step that is not a finite distance, or after the session finished, passes nothing.
    pub fn step(&mut self, x: f32, t: f64) -> Vec<TimingEvent> {
        let mut events = Vec::new();
        if !x.is_finite() || self.finish_time.is_some() {
            return events;
        }
        if self.start_pending && self.started.is_none() {
            (self.started, self.lap_start, self.next_sector) = (Some(t), t, 0);
            self.sector_times.clear();
            events.push(TimingEvent::Started);
        }
        self.start_pending = false;
        if let Some((px, pt)) = self.last {
            let half = self.length * 0.5;
            // The stretch of this step that lies in the lap in progress: from the line when the step crossed it.
            let (mut from, mut from_t) = (px, pt);
            if px - x > half {
                // Forward over the line: `length - px` to the seam, then `x` past it.
                let at = pt + (t - pt) * f64::from((self.length - px) / (self.length - px + x));
                self.cross_forward(at, &mut events);
                (from, from_t) = (0.0, at);
            } else if x - px > half {
                self.cross_back();
            }
            if self.started.is_some() && self.behind == 0 {
                self.pass_sectors(from, x, from_t, t, &mut events);
            }
        }
        self.last = Some((x, t));
        events
    }

    fn cross_forward(&mut self, at: f64, events: &mut Vec<TimingEvent>) {
        if self.behind > 0 {
            self.behind -= 1;
            self.next_sector = 0;
            return;
        }
        let Some(started) = self.started else {
            self.started = Some(at);
            self.lap_start = at;
            self.next_sector = 0;
            self.sector_times.clear();
            events.push(TimingEvent::Started);
            return;
        };
        let time = (at - self.lap_start) as f32;
        let best = self.best().is_none_or(|best| time < best);
        if best {
            self.best = Some((time, self.sector_times.clone()));
        }
        self.times.push(time);
        let lap = self.times.len() as u32;
        events.push(TimingEvent::Lap { lap, time, best });
        let total = (at - started) as f32;
        if self.rule.reached(lap, total) {
            self.finish_time = Some(total);
            events.push(TimingEvent::Finished { time: total });
        }
        self.lap_start = at;
        self.next_sector = 0;
        self.sector_times.clear();
    }

    /// Backwards over the line: the sectors of the lap behind were all passed.
    fn cross_back(&mut self) {
        if self.started.is_some() {
            self.behind += 1;
            self.next_sector = self.sectors.len();
        }
    }

    /// The sector gates the car reached going from `from` at time `from_t` to `to` at `t`.
    fn pass_sectors(&mut self, from: f32, to: f32, from_t: f64, t: f64, events: &mut Vec<TimingEvent>) {
        while let Some(&gate) = self.sectors.get(self.next_sector).filter(|&&gate| gate <= to) {
            let frac = if to > from { f64::from((gate.max(from) - from) / (to - from)) } else { 1.0 };
            let at = from_t + (t - from_t) * frac;
            let time = (at - self.lap_start) as f32;
            events.push(TimingEvent::Sector { lap: self.times.len() as u32 + 1, index: self.next_sector, time });
            self.sector_times.push(time);
            self.next_sector += 1;
        }
    }

    /// The completed laps' times, in order.
    pub fn lap_times(&self) -> &[f32] {
        &self.times
    }

    /// The sector times of the lap in progress so far, since it began.
    pub fn sector_times(&self) -> &[f32] {
        &self.sector_times
    }

    /// The sector times of the best lap.
    pub fn best_sector_times(&self) -> &[f32] {
        self.best.as_ref().map_or(&[], |(_, sectors)| sectors)
    }

    /// A best lap from an earlier session to beat (a stored record): the sector gaps and the `best` flag of a
    /// [`TimingEvent::Lap`] compare with it.
    pub fn seed_best(&mut self, lap: f32, sectors: Vec<f32>) {
        self.best = Some((lap, sectors));
    }

    /// How the session ends.
    pub fn rule(&self) -> EndRule {
        self.rule
    }

    /// The best lap's time: this session's best completed lap, or the seeded one if faster.
    pub fn best(&self) -> Option<f32> {
        self.best.as_ref().map(|(time, _)| *time)
    }

    /// The lap in progress (from 1), or `None` before the first pass of the line.
    pub fn lap(&self) -> Option<u32> {
        self.started.map(|_| self.times.len() as u32 + 1)
    }

    /// Time into the lap in progress at `t`, or `None` before the first pass or once finished.
    pub fn lap_elapsed(&self, t: f64) -> Option<f32> {
        (self.started.is_some() && !self.finished()).then_some((t - self.lap_start) as f32)
    }

    /// The end rule was met.
    pub fn finished(&self) -> bool {
        self.finish_time.is_some()
    }

    /// The session's total time, from the first lap's start to the pass that met the end rule.
    pub fn finish_time(&self) -> Option<f32> {
        self.finish_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LENGTH: f32 = 1000.0;

    fn timing(rule: EndRule) -> LapTiming {
        LapTiming::new(&Course::circuit(LENGTH, [250.0, 600.0]).unwrap(), rule).unwrap()
    }

    /// Drives `timing` at `speed` m/s from `start` metres at time 0 until `end_t`, in steps of `dt`; the distance wraps
    /// at the length. Returns the timer and every event with the step time that produced it.
    fn drive(timing: LapTiming, start: f32, speed: f32, dt: f64, end_t: f64) -> (LapTiming, Vec<(f64, TimingEvent)>) {
        drive_from(timing, start, speed, dt, 0.0, end_t)
    }

    /// [`drive`] from time `from_t`, the car at `start` then.
    fn drive_from(
        mut timing: LapTiming,
        start: f32,
        speed: f32,
        dt: f64,
        from_t: f64,
        end_t: f64,
    ) -> (LapTiming, Vec<(f64, TimingEvent)>) {
        let mut all = Vec::new();
        let mut t = from_t;
        while t <= end_t {
            let x = (start as f64 + speed as f64 * (t - from_t)).rem_euclid(LENGTH as f64) as f32;
            let events = timing.step(x, t);
            all.extend(events.into_iter().map(|e| (t, e)));
            t += dt;
        }
        (timing, all)
    }

    #[test]
    fn only_a_circuit_has_lap_timing() {
        let stage = Course::PointToPoint { start: 0.0, sectors: vec![], finish: 500.0 };
        assert!(LapTiming::new(&stage, EndRule::Open).is_none());
    }

    #[test]
    fn a_standing_start_times_the_lap_from_the_line_it_stands_on() {
        let mut t = timing(EndRule::Laps(1));
        t.start_now();
        // From the line at 0 m at 2 s, 50 m/s: the lap is 20 s, the sector gates 5 and 12 s in.
        let (t, events) = drive_from(t, 0.0, 50.0, 0.01, 2.0, 30.0);
        assert_eq!(events[0].1, TimingEvent::Started);
        assert!(matches!(events.last(), Some((_, TimingEvent::Finished { time })) if (time - 20.0).abs() < 1e-3));
        assert_eq!(t.lap_times().len(), 1);
        assert_eq!(events.iter().filter(|(_, e)| matches!(e, TimingEvent::Sector { .. })).count(), 2);
    }

    #[test]
    fn the_first_pass_starts_and_each_next_one_completes_a_lap() {
        // 50 m/s from 900 m: the line at 2 s, a lap every 20 s.
        let (timing, events) = drive(timing(EndRule::Open), 900.0, 50.0, 0.01, 45.0);
        let events: Vec<TimingEvent> = events.into_iter().map(|(_, e)| e).collect();
        let laps: Vec<_> = events.iter().filter(|e| matches!(e, TimingEvent::Lap { .. })).collect();
        assert_eq!(events[0], TimingEvent::Started);
        assert_eq!(laps.len(), 2);
        for lap in timing.lap_times() {
            assert!((lap - 20.0).abs() < 1e-3, "{lap}");
        }
        assert_eq!(timing.lap(), Some(3));
        assert!(!timing.finished());
    }

    #[test]
    fn sectors_come_in_order_with_their_lap_and_time() {
        let (_, events) = drive(timing(EndRule::Open), 900.0, 50.0, 0.01, 36.0);
        let sectors: Vec<_> = events
            .iter()
            .filter_map(|(_, e)| match e {
                TimingEvent::Sector { lap, index, time } => Some((*lap, *index, *time)),
                _ => None,
            })
            .collect();
        // Gates at 250 m and 600 m: 5 s and 12 s after the line.
        assert_eq!(sectors.len(), 4);
        assert_eq!((sectors[0].0, sectors[0].1), (1, 0));
        assert!((sectors[0].2 - 5.0).abs() < 1e-3 && (sectors[1].2 - 12.0).abs() < 1e-3);
        assert_eq!((sectors[2].0, sectors[2].1), (2, 0));
    }

    #[test]
    fn the_pass_is_timed_between_the_steps() {
        // Steps of 0.7 s at 50 m/s are 35 m apart: the line at 2.0 s falls inside the step at 2.1 s.
        let (timing, _) = drive(timing(EndRule::Open), 900.0, 50.0, 0.7, 45.0);
        assert!((timing.lap_times()[0] - 20.0).abs() < 1e-3, "{:?}", timing.lap_times());
    }

    #[test]
    fn the_end_rule_finishes_at_the_line() {
        let (timing, events) = drive(timing(EndRule::Laps(2)), 900.0, 50.0, 0.01, 100.0);
        let finished: Vec<_> = events.iter().filter(|(_, e)| matches!(e, TimingEvent::Finished { .. })).collect();
        assert_eq!(finished.len(), 1);
        assert!(timing.finished() && timing.lap_times().len() == 2);
        match finished[0].1 {
            TimingEvent::Finished { time } => assert!((time - 40.0).abs() < 1e-3),
            _ => unreachable!(),
        }
        assert_eq!(timing.lap_elapsed(99.0), None);
    }

    #[test]
    fn a_hotlap_ends_at_its_first_lap() {
        let (timing, _) = drive(timing(EndRule::Finish), 900.0, 50.0, 0.01, 60.0);
        assert!(timing.finished());
        assert_eq!(timing.lap_times().len(), 1);
    }

    #[test]
    fn reversing_over_the_line_and_driving_on_does_not_count_a_second_pass() {
        let mut t = timing(EndRule::Open);
        let mut events = Vec::new();
        // On the grid, over the line (start), back over it, forward over it again, then ahead to a sector gate.
        for (i, x) in [990.0, 10.0, 990.0, 10.0, 100.0, 300.0].into_iter().enumerate() {
            events.extend(t.step(x, i as f64));
        }
        assert_eq!(events[0], TimingEvent::Started);
        assert!(matches!(events[1], TimingEvent::Sector { lap: 1, index: 0, .. }));
        assert_eq!(events.len(), 2);
        assert!(t.lap_times().is_empty());
    }

    #[test]
    fn the_sector_times_follow_the_lap_in_progress_and_the_best_lap() {
        let (t, _) = drive(timing(EndRule::Open), 900.0, 50.0, 0.01, 31.0);
        // 31 s: the second lap began at 22 s, its first gate (5 s in) is at 27 s, its second (12 s in) not yet.
        assert_eq!(t.sector_times().len(), 1);
        assert!((t.sector_times()[0] - 5.0).abs() < 1e-3);
        assert_eq!(t.best_sector_times().len(), 2);
        assert!((t.best_sector_times()[1] - 12.0).abs() < 1e-3);
        assert_eq!(t.rule(), EndRule::Open);
    }

    #[test]
    fn a_seeded_best_is_beaten_only_by_a_faster_lap() {
        let mut t = timing(EndRule::Open);
        t.seed_best(10.0, vec![3.0, 6.0]);
        assert_eq!(t.best(), Some(10.0));
        assert_eq!(t.best_sector_times(), [3.0, 6.0]);
        let (t, events) = drive(t, 900.0, 50.0, 0.01, 23.0);
        let lap = events.iter().find_map(|(_, e)| matches!(e, TimingEvent::Lap { .. }).then_some(*e)).unwrap();
        assert!(matches!(lap, TimingEvent::Lap { best: false, .. }), "a 22 s lap does not beat 10 s: {lap:?}");
        assert_eq!(t.best(), Some(10.0));
    }

    #[test]
    fn best_lap_is_the_fastest_and_nothing_before_one() {
        let t = timing(EndRule::Open);
        assert_eq!(t.best(), None);
        let (t, _) = drive(t, 900.0, 50.0, 0.01, 45.0);
        assert!((t.best().unwrap() - 20.0).abs() < 1e-3);
    }

    #[test]
    fn a_step_that_is_not_a_distance_passes_nothing() {
        let mut t = timing(EndRule::Open);
        let events = t.step(f32::NAN, 1.0);
        assert!(events.is_empty() && t.lap().is_none());
    }
}
