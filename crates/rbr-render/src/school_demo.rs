//! The rally school's demonstration and ride (viewer-only): the animated replay `Ghosts\rse%d.arp` in the
//! install's `Root/ghosts.rbz` (`rbr_formats::arp`), played while the lesson's SCH1 record is in the brief state.
//!
//! Every timed lesson has one (lessons 1, 2, 3, 5, 6, 7, 8, 9, 11, 12, 15); lesson 5's is the ride with Richard
//! Burns, twice the size of the others. The stepping process plays nothing new for them: the viewer draws the
//! player's own car at the replay's pose and holds the player's car at the start until the replay ends or is
//! skipped, when the brief shows (its Enter then starts the drive as usual). Lesson 5's ride instead sends the
//! brief's confirm when it ends, so the lesson ends with its award and the verdict panel (`rbr_school::run` ends
//! the ride on its first driving frame). A missing or unreadable `.arp` shows the brief straight away.
//!
//! The replay's pose is the physics body frame (`rbr_recording::pose::CarPose`): the sampler's position and rotation
//! drive the car as a recording's do. The replay holds no wheel samples, so the wheels stay at their rest pose (the
//! renderer never steers the exterior wheels, and the sampler's suspension bytes are not decoded); the sampled steer
//! only turns the crew's wheel, which the demonstration hides with the crew.

use rbr_formats::arp::{BodyPose, Replay};
use rbr_formats::data::Data;
use rbr_formats::ghost::Stream;
use rbr_school::Lesson;

use crate::views::{CarView, Mounts};

/// A lesson's demonstration or ride.
#[derive(Debug, Clone)]
pub struct Demo {
    lesson: Lesson,
    replay: Replay,
}

impl Demo {
    /// The lesson's `Ghosts\rse{lesson}.arp` from the install's `Root/ghosts.rbz`, if it reads. `None` (a missing or
    /// unreadable file, or a non-positive duration) shows the brief straight away.
    pub fn load(data: &Data, lesson: Lesson) -> Option<Demo> {
        let path = data.resolve("Root\\ghosts.rbz")?;
        let mut archive = rbr_formats::rbz::open(&path).ok()?;
        let name = format!("Ghosts\\rse{}.arp", lesson.index());
        let mut entry = rbr_formats::rbz::by_name(&mut archive, &name).ok()?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).ok()?;
        let replay = Replay::parse(&bytes).ok()?;
        let demo = Demo { lesson, replay };
        let duration = demo.duration();
        (duration.is_finite() && duration > 0.0).then_some(demo)
    }

    pub fn lesson(&self) -> Lesson {
        self.lesson
    }

    /// Whether the lesson is the ride (lesson 5): played from the in-car camera, ending with the brief's confirm.
    pub fn ride(&self) -> bool {
        self.lesson.ride()
    }

    /// The ride's camera: the cockpit, or the bonnet when the car has no cockpit mount.
    pub fn ride_view(&self, mounts: &Mounts) -> CarView {
        ride_view(mounts)
    }

    /// The replay's length, s.
    pub fn duration(&self) -> f32 {
        self.replay.sections.first().map_or(0.0, |s| s.duration())
    }

    /// The demo car's body pose at `t` seconds.
    pub fn pose(&self, t: f32) -> Option<BodyPose> {
        self.replay.sections.first()?.sample_body(t)
    }
}

/// The judged replay's ghost file: Richard Burns' `Ghosts\rse{lesson}.gho` (case 6 of the school flow), which a
/// replay of a school lesson races as the ghost car (`rbr_render::ghost`).
pub fn ghost_name(lesson: Lesson) -> String {
    format!("Ghosts\\rse{}.gho", lesson.index())
}

/// The lesson's judged-replay ghost from the install's `Root/ghosts.rbz`: its map, its car's `Cars.ini` index and
/// its sample stream, or `None` when the file is missing or unreadable. The retail archive has no `rse4`, `rse10`,
/// `rse14` or `rse16` (all untimed lessons): a replay of one of those races without a ghost.
pub fn ghost(data: &Data, lesson: Lesson) -> Option<(u32, u32, Stream)> {
    let path = data.resolve("Root\\ghosts.rbz")?;
    let mut archive = rbr_formats::rbz::open(&path).ok()?;
    let mut entry = rbr_formats::rbz::by_name(&mut archive, &ghost_name(lesson)).ok()?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes).ok()?;
    let ghost = rbr_formats::ghost::Ghost::parse(&bytes).ok()?;
    if ghost.stream.samples.is_empty() {
        return None;
    }
    Some((ghost.map, u32::from(ghost.car), ghost.stream))
}

/// The lesson a replay judges: `--school N` in the stepping `command`, when the viewer watches a replay
/// (`--replay`) and no explicit `--ghost` names one. A live lesson drive is vanilla: no ghost. `None` otherwise.
pub fn judged_lesson(command: &[String], watching_replay: bool, explicit_ghost: bool) -> Option<Lesson> {
    if !watching_replay || explicit_ghost {
        return None;
    }
    command
        .iter()
        .position(|a| a == "--school")
        .and_then(|i| command.get(i + 1))
        .and_then(|n| n.parse().ok())
        .and_then(Lesson::new)
}

/// The ride's camera: the cockpit, or the bonnet when the car has no cockpit mount.
pub fn ride_view(mounts: &Mounts) -> CarView {
    [CarView::Cockpit, CarView::Bonnet].into_iter().find(|view| mounts.of(*view).is_some()).unwrap_or(CarView::Bonnet)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The judged ghost is `Ghosts\rse<lesson>.gho` for every lesson, the school flow's case 6 file.
    #[test]
    fn ghost_names_follow_the_lesson() {
        for lesson in Lesson::all() {
            assert_eq!(ghost_name(lesson), format!("Ghosts\\rse{}.gho", lesson.index()));
        }
    }

    /// A replay judges the lesson its live command carries `--school N` for: a live drive stays vanilla, an
    /// explicit `--ghost` wins, and a replay of no lesson (or a bad number) judges nothing.
    #[test]
    fn judged_lesson_needs_a_replay_of_a_school_lesson_without_an_explicit_ghost() {
        let school = ["--live", "standalone", "--live", "--boot", "--school", "2"].map(str::to_owned).to_vec();
        assert_eq!(judged_lesson(&school, true, false), Lesson::new(2));
        assert_eq!(judged_lesson(&school, false, false), None, "a live lesson drive keeps no ghost");
        assert_eq!(judged_lesson(&school, true, true), None, "an explicit --ghost wins");
        let quick = ["--live", "standalone", "--live", "--boot"].map(str::to_owned).to_vec();
        assert_eq!(judged_lesson(&quick, true, false), None);
        let bad = ["--school"].map(str::to_owned).to_vec();
        assert_eq!(judged_lesson(&bad, true, false), None);
        let late = ["--school", "2", "--school", "16"].map(str::to_owned).to_vec();
        assert_eq!(judged_lesson(&late, true, false), Lesson::new(2), "the live command's own lesson");
    }

    /// The judged-replay ghosts (`RBR_DATA`): every timed lesson's `rse<N>.gho` reads with a non-empty stream;
    /// lessons 4, 10, 14 and 16 have none and race without a ghost.
    #[test]
    fn judged_ghosts() {
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        for lesson in Lesson::all() {
            match ghost(&data, lesson) {
                Some((map, car, stream)) => {
                    assert!(!stream.samples.is_empty(), "{lesson:?}");
                    assert!(car <= 7, "{lesson:?}: car {car}");
                    if lesson.index() == 2 {
                        assert_eq!(map, lesson.map(), "lesson 2's ghost runs map 83");
                        let last = stream.samples.last().map(|s| s.seconds()).unwrap_or(0.0);
                        assert!(last > 8.0 && last < 10.0, "lesson 2's ghost beats its 9 s perfect time: {last}");
                    }
                }
                None => assert!(
                    matches!(lesson.index(), 4 | 10 | 14 | 16),
                    "{lesson:?}: the retail archive holds its rse file"
                ),
            }
        }
    }
}
