//! What a format's timing tells the rest of the game.

use rbr_gameplay::timing::{Checkpoint, CheckpointKind};

/// A timing event. Times are seconds, as the original's checkpoint signal carries them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimingEvent {
    /// The clock started: the line was passed (a stage's start, a circuit's first pass).
    Started,
    /// A sector gate was passed: `index` counts from 0 in the lap `lap` (from 1), `time` since the lap began.
    Sector { lap: u32, index: usize, time: f32 },
    /// A lap was completed in `time`; `best` when it beat the best so far (the first lap does, without a stored one).
    Lap { lap: u32, time: f32, best: bool },
    /// The session's end rule was met, `time` since the start.
    Finished { time: f32 },
}

/// The 1.02 timekeeper's checkpoints are the one lap of a stage: the start, its splits as sectors, the finish.
impl From<Checkpoint> for TimingEvent {
    fn from(checkpoint: Checkpoint) -> Self {
        match checkpoint.kind {
            CheckpointKind::Start => TimingEvent::Started,
            CheckpointKind::Split(index) => TimingEvent::Sector { lap: 1, index, time: checkpoint.time },
            CheckpointKind::Finish => TimingEvent::Finished { time: checkpoint.time },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkpoint_is_the_event_of_a_one_lap_session() {
        let at = |kind| TimingEvent::from(Checkpoint { kind, time: 12.5, fired: true });
        assert_eq!(at(CheckpointKind::Start), TimingEvent::Started);
        assert_eq!(at(CheckpointKind::Split(1)), TimingEvent::Sector { lap: 1, index: 1, time: 12.5 });
        assert_eq!(at(CheckpointKind::Finish), TimingEvent::Finished { time: 12.5 });
    }
}
