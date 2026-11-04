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
use rbr_school::Lesson;

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

    /// The replay's length, s.
    pub fn duration(&self) -> f32 {
        self.replay.sections.first().map_or(0.0, |s| s.duration())
    }

    /// The demo car's body pose at `t` seconds.
    pub fn pose(&self, t: f32) -> Option<BodyPose> {
        self.replay.sections.first()?.sample_body(t)
    }
}
