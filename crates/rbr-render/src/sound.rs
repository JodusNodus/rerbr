//! The live drive's sound (`rbr_sound::Player`): the car's `Cars.ini` index from its folder name, the player the
//! viewer feeds each step's SND1 record (`rbr_recording::sound`), and the camera and car it hears them from.

use glam::Vec3;
use rbr_formats::cars;
use rbr_formats::data::Data;
use rbr_recording::pose::CarPose;
pub use rbr_sound::custom_codriver::Choice;
use rbr_sound::custom_codriver::{self, CustomCoDriver};
pub use rbr_sound::data::Wave;
pub use rbr_sound::mixer::Volumes;
pub use rbr_sound::{Events, Player, Scene, Sky};

use crate::camera::Lens;
use crate::characters::scripted::Event;
use crate::hud::school::{self, State as SchoolState};
use crate::particles::wheel_spray::{self, ground::Kind};
use crate::particles::Particles;
use crate::roadside::Roadside;

/// The `Cars/Cars.ini` index of the car in folder `name` (`COROLLA`), or `None`.
pub fn car_index(data: &Data, name: &str) -> Option<u32> {
    let text = std::fs::read(data.resolve("Cars\\Cars.ini")?).ok()?;
    let entries = data.car_slots(cars::parse(&String::from_utf8_lossy(&text)).ok()?);
    let folder = format!("cars\\{}\\", name.to_ascii_lowercase());
    entries
        .iter()
        .find(|e| e.ini_file.to_ascii_lowercase().replace('/', "\\").contains(&folder))
        .map(|e| e.index as u32)
}

/// The car's sound at `volume` under `sky`, with the co-driver's pace notes or not and the player's group
/// volumes (`rbr_sound::mixer::Volumes`), on the audio device or, with `offline`'s sample rate, rendered by the caller;
/// or `None` (with the reason on stderr) when the data or the audio device is missing.
pub fn start(
    data: &Data,
    car: &str,
    volume: f32,
    sky: Sky,
    codriver: bool,
    volumes: Volumes,
    offline: Option<u32>,
) -> Option<Player> {
    let index = car_index(data, car).unwrap_or_else(|| {
        eprintln!("viewer: sound: {car} is not in Cars.ini; car 0's sound");
        0
    });
    let player = match offline {
        Some(rate) => Player::offline(data, index, rate, volume, sky, codriver, volumes),
        None => Player::start(data, index, volume, sky, codriver, volumes),
    };
    match player {
        Ok(player) => Some(player),
        Err(e) => {
            eprintln!("viewer: no sound: {e}");
            None
        }
    }
}

/// Plays the community car's FMOD bank at `path` (`rbr_sound::fmod`) instead of its slot's 1.02 engine, turbo and
/// transmission sounds; without it (the reason on stderr) those stay.
pub fn use_fmod(player: &mut Player, path: &std::path::Path) {
    match rbr_sound::fmod::FmodCar::load(path) {
        Ok(car) => {
            eprintln!("viewer: sound: {} ({} recordings)", path.display(), car.waves.len());
            player.use_fmod(car);
        }
        Err(e) => eprintln!("viewer: sound: no FMOD bank: {e}"),
    }
}

/// Calls the pace notes with the co-driver mod `choice` instead of 1.02's, for the stage `stage` (a name or path, its
/// `.dls` the notes without a `choice.notes` file) and the call-out distance `callout_time`; on an error (on stderr) the
/// 1.02 co-driver stays.
pub fn use_codriver(player: &mut Player, data: &Data, stage: &str, choice: &Choice, callout_time: f32) {
    let load = || {
        let dls = if choice.notes.is_none() { custom_codriver::stage_notes(data, stage)? } else { Vec::new() };
        CustomCoDriver::load(choice, dls, callout_time)
    };
    match load() {
        Ok(custom) => {
            eprintln!("viewer: sound: co-driver {} ({} samples)", choice.voice, custom.samples().len());
            player.use_codriver(custom);
        }
        Err(e) => eprintln!("viewer: co-driver mod: {e}"),
    }
}

/// The sound's scene for the camera `lens` (stage frame) and the car at `car` (physics frame): the stage's y up is
/// the physics' z up (`crate::coords`), so the eye and its right swap y and z. With the stage's `roadside` the
/// engine's reflections follow it (ours), probed at the car, its sides as the camera has them.
pub fn scene(lens: &Lens, car: &CarPose, inside: bool, replay: bool, roadside: Option<&Roadside>) -> Scene {
    let swap = |v: Vec3| [v.x, v.z, v.y];
    // Left-handed, y up: right is up x forward.
    let right = lens.up.cross(lens.forward).normalize_or_zero();
    let forward = Vec3::from(car.forward());
    let roadside =
        roadside.map(|r| heard(r.probe(Vec3::from(car.position), forward), forward, Vec3::from(swap(right))));
    Scene {
        listener: swap(lens.eye),
        right: swap(right),
        car: car.position,
        velocity: car.velocity,
        inside,
        replay,
        roadside,
        ..Scene::default()
    }
}

/// Each wheel's water for the sound (`rbr_sound::tyre::Water`, ours) from the particles' last frame: the puddle depth
/// the spray's plume takes ([`wheel_spray::puddle`]), whether it is wet tarmac's film, the ground's wetness and whether
/// it is loose.
pub fn water(particles: &Particles) -> [rbr_sound::tyre::Water; 4] {
    particles.wheel_water().map(|w| rbr_sound::tyre::Water {
        puddle: wheel_spray::puddle(w.standing),
        wet: w.ground.wet,
        loose: matches!(w.ground.kind, Kind::Gravel | Kind::Dirt | Kind::Grass),
        film: w.film,
    })
}

/// The car at the verge for the sound (`rbr_sound::verge::Verge`, ours): each wheel's share on grass from the
/// particles' last frame and the vegetation of the stage's `roadside` the body of the car at `car` is in
/// ([`crate::verge`]); `None` without either, which plays nothing of it.
pub fn verge(
    particles: Option<&Particles>,
    roadside: Option<&Roadside>,
    car: &CarPose,
) -> Option<rbr_sound::verge::Verge> {
    if particles.is_none() && roadside.is_none() {
        return None;
    }
    Some(crate::verge::verge(particles.map_or([0.0; 4], Particles::wheel_grass), roadside, car))
}

/// The car's roadside `sides` (left, right as the car facing `forward` has them) as a listener whose right is
/// `right` hears them: swapped for a camera facing the car.
pub fn heard(
    roadside: rbr_sound::reflections::Roadside,
    forward: Vec3,
    right: Vec3,
) -> rbr_sound::reflections::Roadside {
    let car_right = Vec3::new(forward.y, -forward.x, 0.0);
    let [left, right_side] = roadside.sides;
    if car_right.dot(right) < 0.0 {
        rbr_sound::reflections::Roadside { sides: [right_side, left], ..roadside }
    } else {
        roadside
    }
}

/// The rally school's speech on the school voice (`E25School::OnSignal` 0x4b8200), fed from the live record's
/// school state (SCH1): the stop when the flow moved, `Trigger`'s ids at priority 1 after a flush of the pending
/// queue, the briefing's id once per brief (kind 1, ours: no sender of it is reconstructed) and the end page's id
/// once per verdict (kind by the verdict, `sub` from the viewer's frame count: never the game's CRT `rand()`
/// stream). The co-driver switch does not mute it (the handler does not test it); the co-driver volume applies
/// (the mixer's speech gain).
#[derive(Debug)]
pub struct SchoolSpeech {
    data: Data,
    /// The bank failed to load (the reason is on stderr): no retries.
    failed: bool,
    /// The lesson whose briefing id was queued, until the brief ends.
    brief_done: Option<u32>,
    /// The lesson and verdict whose end page's id was queued, until the end panel goes.
    verdict_done: Option<(u32, u32)>,
}

impl SchoolSpeech {
    /// A feed loading its samples from `data` on the first school frame.
    pub fn new(data: Data) -> SchoolSpeech {
        SchoolSpeech { data, failed: false, brief_done: None, verdict_done: None }
    }

    /// Feeds the player the school state's speech: no school drive clears the once-per-panel latches.
    pub fn feed(&mut self, player: &mut Player, school: Option<&SchoolState>, stop: bool, ids: &[u32], frame: u32) {
        use rbr_sound::voice::{Command, Speech};
        let Some(school) = school else {
            self.brief_done = None;
            self.verdict_done = None;
            return;
        };
        if !self.failed {
            if let Err(e) = player.load_school(&self.data) {
                self.failed = true;
                eprintln!("viewer: no school speech: {e}");
                return;
            }
        } else {
            return;
        }
        if stop {
            player.apply(&[Command::StopSpeech]);
        }
        if !ids.is_empty() {
            let mut commands = Vec::with_capacity(ids.len() + 1);
            commands.push(Command::FlushSpeech);
            commands.extend(ids.iter().map(|&id| Command::Speech(Speech::School { id, priority: 1 })));
            player.apply(&commands);
        }
        if school.phase == school::Phase::Brief {
            if self.brief_done != Some(school.lesson) {
                self.brief_done = Some(school.lesson);
                if let Some(id) = rbr_school::briefing(school.lesson) {
                    player.apply(&[Command::Speech(Speech::School { id, priority: 0 })]);
                }
            }
        } else {
            self.brief_done = None;
        }
        match (school.phase, school.verdict) {
            (school::Phase::Ended, Some(verdict)) => {
                if self.verdict_done != Some((school.lesson, verdict)) {
                    self.verdict_done = Some((school.lesson, verdict));
                    let id = rbr_school::end_page(verdict, frame);
                    player.apply(&[Command::Speech(Speech::School { id, priority: 0 })]);
                }
            }
            _ => self.verdict_done = None,
        }
    }
}

/// The sound's scripted events from the characters' `events` and the helicopter body at `helicopter` (stage frame,
/// `Characters::rescue_position`).
pub fn events(events: &[Event], helicopter: Option<Vec3>) -> Events {
    let physics = |p: Vec3| [p.x, p.z, p.y];
    Events {
        helicopter: helicopter.map(physics),
        birdstrikes: events
            .iter()
            .filter_map(|e| if let Event::Birdstrike(p) = e { Some(physics(*p)) } else { None })
            .collect(),
        rescue_called: events.iter().any(|e| matches!(e, Event::RescueCalled(_))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_sound::reflections::{Roadside as Heard, Side};

    #[test]
    fn a_camera_facing_the_car_hears_its_sides_swapped() {
        let left = Side { distance: 3.0, size: 1.0, hardness: 1.0 };
        let car = Heard { sides: [left, Side::default()], overhead: Side::default() };
        // The car faces +y: its right is +x.
        assert_eq!(heard(car, Vec3::Y, Vec3::X), car);
        assert_eq!(heard(car, Vec3::Y, Vec3::NEG_X).sides, [Side::default(), left]);
    }
}
