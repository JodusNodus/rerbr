//! The circuit's format: laps timed from the drive's distance, ending by an [`EndRule`].

use rbr_gameplay::format::{Format, FormatEnd, StepView};

use crate::{Course, EndRule, LapTiming, TimingEvent};

/// A circuit session's rules for the stage loop. The course's line must be closed (`notes/session.md`), so the
/// car's distance wraps at the lap line.
#[derive(Debug, Clone)]
pub struct LapRules {
    timing: LapTiming,
    events: Vec<TimingEvent>,
}

impl LapRules {
    /// The rules of `course` ending by `rule`. `None` for a course that is not a circuit.
    pub fn new(course: &Course, rule: EndRule) -> Option<Self> {
        Some(Self { timing: LapTiming::new(course, rule)?, events: Vec::new() })
    }

    pub fn timing(&self) -> &LapTiming {
        &self.timing
    }

    /// The timing events since the last call, in order (the HUD's, the records').
    pub fn take_events(&mut self) -> Vec<TimingEvent> {
        std::mem::take(&mut self.events)
    }
}

impl Format for LapRules {
    /// A standing start: the first lap starts with the first step of the drive.
    fn start(&mut self) {
        self.timing.start_now();
    }

    fn after_step(&mut self, step: &StepView) {
        if let Some(distance) = step.distance {
            self.events.extend(self.timing.step(distance, step.elapsed));
        }
    }

    /// The end rule was met at a pass of the line.
    fn frame(&mut self, _dt: f32, _race_time: f32) -> Option<FormatEnd> {
        self.timing.finished().then_some(FormatEnd::Finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_formats::markers::StageMarkers;
    use rbr_gameplay::control::CarControl;
    use rbr_gameplay::filter::Grid;
    use rbr_gameplay::flow::EndCause;
    use rbr_gameplay::game::{FrameInput, Game, PhysicsStep, StepSample};
    use rbr_gameplay::ini::Ini;
    use rbr_gameplay::markers::MarkerSettings;
    use rbr_gameplay::stage::{Stage, State};

    /// A closed line of `LENGTH` entries whose distance is the entry's index, driven `STRIDE` entries a step round and
    /// round: the physics' progress value wrapping at the seam.
    const LENGTH: usize = 1000;
    const STRIDE: f32 = 4.0;
    const DT: f64 = 0.007;

    struct Looping(f32);

    impl PhysicsStep for Looping {
        fn step(&mut self, _control: Option<CarControl>, _index: u32, inside_step: &mut dyn FnMut(StepSample)) {
            self.0 = (self.0 + STRIDE) % LENGTH as f32;
            inside_step(StepSample { driveline_parameter: self.0, ..StepSample::default() });
        }
    }

    fn game() -> Game {
        let values = (0..=LENGTH).map(|i| i as f32).collect();
        Game::new(
            Stage::driving(54, 6),
            Grid::from_settings(&Ini::parse("")),
            &StageMarkers { keys: Vec::new() },
            values,
            DT as f32,
            0.0,
            MarkerSettings::default(),
        )
    }

    #[test]
    fn the_game_runs_a_two_lap_session_and_ends_it_at_the_line() {
        let course = Course::circuit(LENGTH as f32, [500.0]).unwrap();
        let mut rules = LapRules::new(&course, EndRule::Laps(2)).unwrap();
        let (mut game, mut physics) = (game(), Looping(980.0));
        let mut now = 0.0;
        let mut events = Vec::new();
        while game.stage().state() == State::Driving && now < 60.0 {
            now += DT;
            game.frame_with(now, FrameInput::default(), &mut physics, &mut rules);
            events.extend(rules.take_events());
        }
        // A lap is 250 steps of 7 ms.
        let lap = 250.0 * DT as f32;
        let laps: Vec<f32> = events
            .iter()
            .filter_map(|e| match e {
                TimingEvent::Lap { time, .. } => Some(*time),
                _ => None,
            })
            .collect();
        assert_eq!(laps.len(), 2, "{events:?}");
        assert!(laps.iter().all(|l| (l - lap).abs() < 0.01), "{laps:?}");
        assert!(matches!(events.first(), Some(TimingEvent::Started)));
        assert!(matches!(events.last(), Some(TimingEvent::Finished { time }) if (time - 2.0 * lap).abs() < 0.01));
        assert_eq!(events.iter().filter(|e| matches!(e, TimingEvent::Sector { .. })).count(), 2);
        assert_eq!(game.flow().end().map(|end| end.cause), Some(EndCause::Format));
        assert_eq!(game.stage().state(), State::Ended);
    }

    #[test]
    fn a_stage_is_not_a_circuit() {
        let stage = Course::PointToPoint { start: 0.0, sectors: vec![], finish: 100.0 };
        assert!(LapRules::new(&stage, EndRule::Open).is_none());
    }
}
