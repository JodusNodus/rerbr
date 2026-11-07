//! The gameplay sound of Richard Burns Rally 1.02 (`notes/sound.md`), in two layers:
//!
//! - pure models that turn one physics step's inputs (`rbr_recording::sound`, the SND1 record) into voice commands
//!   ([`voice`]): the granular engine ([`engine`]: rpm slew, bank choice, crossfade, grains, the renderer), the gear
//!   and diff whine, the turbo and the blow-off valve ([`car`]), the tyre loops and off-road rattles ([`tyre`]), the
//!   impacts ([`impact`]), the tyres on grass and the vegetation against the body (ours, [`verge`]), the
//!   suspension's squeaks and knocks ([`suspension`]), the broken driveshaft's clunks ([`driveshaft`]), the exhaust
//!   pops, bangs and the engine fire ([`exhaust`]), the co-driver's pace notes, countdown and damage calls
//!   ([`codriver`]) or a co-driver mod's calls ([`custom_codriver`]), the wind, birds, siren and rain around the car
//!   ([`world`]), and the
//!   rescue helicopter, bird hit and rally incident ([`scripted`]); [`Models`] runs them all on one step's [`Inputs`]
//!   and the camera's [`Scene`];
//! - the backend: the [`mixer`] plays the commands (engine stream, loops, one-shots, the in-car low-pass, the car
//!   voices' distance from the camera) into stereo frames, and [`device`] sends them to the audio output through
//!   cpal.
//!
//! The sound is NGP 7.4.778's (its NSD1 record, [`Inputs::ngp`]): the models take NGP's patches of the 1.02 sound code:
//! the engine voice's low rpm and the whine caps from NGP.dll ([`car`], [`exhaust`]), the tyres' second surface, road
//! list and skid threshold ([`tyre::wheel`]), the max distances NGP's fix of `E30DXSound` +0x74 gives (1.02's sets the
//! min: `world::DEFAULT_MAX_DISTANCE`), and the damper's and the fire's damage calls the right way round. NGP's FMOD
//! sound is modelled only for a car with a bank ([`fmod`]).
//!
//! [`Player`] puts them together for a live drive; the `rbr-sound` binary renders a recorded drive to a WAV. The
//! menus' music and service park ambience are [`menu`], with their own mixer and player.

pub mod car;
pub mod codriver;
pub mod custom_codriver;
pub mod data;
pub mod device;
pub mod driveshaft;
pub mod effects;
pub mod engine;
pub mod exhaust;
pub mod fmod;
pub mod impact;
pub mod menu;
pub mod mixer;
mod posted;
pub mod preview;
pub mod reflections;
pub mod scripted;
pub mod suspension;
pub mod tyre;
pub mod verge;
pub mod voice;
pub mod world;

use std::sync::{Arc, Mutex};

use rbr_formats::data::Data;
pub use rbr_recording::sound::{Inputs, Sound};

use crate::car::Car;
use crate::codriver::CoDriver;
use crate::custom_codriver::CustomCoDriver;
use crate::data::CarBank;
use crate::driveshaft::DriveShafts;
use crate::effects::Effects;
use crate::engine::{EngineNoise, Rng};
use crate::exhaust::Exhaust;
use crate::fmod::FmodCar;
use crate::impact::Impacts;
use crate::mixer::Mixer;
pub use crate::mixer::Volumes;
use crate::posted::Fed;
pub use crate::scripted::Events;
use crate::scripted::Scripted;
use crate::suspension::Suspension;
use crate::tyre::Tyres;
use crate::verge::Verges;
use crate::voice::Command;
use crate::world::World;
pub use crate::world::{Scene, Sky, Sphere};

/// The H-shifter's grind gain: the sample at full volume.
const GRIND_GAIN: f32 = 1.0;
/// The physics step, seconds.
const STEP: f32 = rbr_recording::STEP_SECONDS;
/// The pause after each word while the race clock is not running (`E31Speech::Vf08` 0x4d2e30: 0x7354e8 when the
/// HUD's +0x140 is at most 0), seconds.
const SPEECH_GAP: f32 = 0.371_519_27;

/// NGP's code for a damage call of the damper or the fire handler: its patches of the two pushes (0x4ae2eb in the
/// damper handler, 0x4ae3c0 in the fire handler; read in the `ngp-c6-bis` snapshots) swap 1.02's 7 and 6, so the
/// broken damper says `sus_broke` and the fire `weve_got_fire`.
fn ngp_call(code: u32) -> u32 {
    match code {
        suspension::BROKEN_CALL => exhaust::FIRE_CALL,
        exhaust::FIRE_CALL => suspension::BROKEN_CALL,
        other => other,
    }
}

/// Every model of the gameplay sound, stepped together.
pub struct Models {
    pub car: Car,
    pub tyres: Tyres,
    /// The tyres on grass and the vegetation against the body (ours, [`verge`]).
    pub verges: Verges,
    pub impacts: Impacts,
    pub suspension: Suspension,
    pub drive_shafts: DriveShafts,
    pub exhaust: Exhaust,
    pub codriver: CoDriver,
    /// A co-driver mod, calling the notes instead of 1.02's co-driver (whose countdown and damage calls stay).
    pub custom: Option<CustomCoDriver>,
    pub world: World,
    pub scripted: Scripted,
    pub effects: Effects,
    /// The car's FMOD bank, playing instead of the 1.02 engine, turbo and transmission voices (`fmod`, ours).
    pub fmod: Option<FmodCar>,
    /// The random numbers the models pick with (ours; the original uses the CRT `rand()`).
    rng: Rng,
    /// The part of a game frame ([`tyre::FRAME_RATE`]) not yet run.
    frame: f32,
    /// The camera's position at the last step, and its velocity from it (for the Doppler shift).
    listener: Option<[f32; 3]>,
    listener_velocity: [f32; 3],
}

impl Models {
    /// The car's models on a stage with weather `sky` and table-6 spheres `spheres`.
    pub fn new(bank: &CarBank, sky: Sky, spheres: Vec<Sphere>) -> Models {
        let g = bank.gains;
        Models {
            car: Car::new(g),
            tyres: {
                let mut tyres = Tyres::new(g.tyre, g.offroad, bank.grus.len(), bank.road_surfaces.clone());
                tyres.splash_samples = bank.splashes.len();
                tyres
            },
            verges: Verges::new(bank.branches.len()),
            impacts: Impacts::new(g.ranged, g.scream, bank.ranged.len(), bank.screams.len()),
            suspension: Suspension::new(
                [g.squeak, g.suspension, g.ranged],
                [bank.squeaks.len(), bank.bottomings.len(), bank.ranged.len()],
            ),
            drive_shafts: DriveShafts::new(g.drive_shaft, bank.drive_shafts.len()),
            exhaust: Exhaust::new(
                [g.exhaust_pop, g.pop, g.bang, g.fire],
                [bank.exhaust_pops.len(), bank.pops.len(), bank.bangs.len()],
            ),
            codriver: CoDriver::default(),
            custom: None,
            world: World::new(bank.world_gains, sky, spheres),
            scripted: Scripted::new(bank.scripted_gains),
            effects: Effects::new(bank.effect_gains, bank.dog_rings.len()),
            fmod: None,
            rng: Rng::new(0x51d2),
            frame: 0.0,
            listener: None,
            listener_velocity: [0.0; 3],
        }
    }

    /// The commands for one step's inputs, with the camera and the car at `scene`.
    pub fn step(&mut self, inputs: &Inputs, scene: &Scene) -> Vec<Command> {
        self.listener_velocity = world::listener_velocity(self.listener.replace(scene.listener), scene.listener, STEP);
        let scene = &Scene { listener_velocity: self.listener_velocity, ..*scene };
        let sound = &inputs.sound;
        let inside = scene.inside;
        let rng = &mut self.rng;
        let ngp = &inputs.ngp;
        // The world first: its listener command places the car voices that follow.
        let mut out = self.world.step(scene, STEP, || rng.rand());
        // A door stays off once it has come off (DET1 leaves out a sleeping part).
        self.car.doors_off = std::array::from_fn(|i| self.car.doors_off[i] || inputs.doors_off[i]);
        // A game frame in this step (at most one: a step is shorter than a frame).
        self.frame += STEP * tyre::FRAME_RATE;
        let frame = self.frame >= 1.0;
        if frame {
            self.frame -= 1.0;
        }
        self.car.gear_click = self.fmod.as_ref().and_then(FmodCar::gear_click);
        out.extend(self.car.step(sound, inside, ngp, frame));
        out.push(Command::Roadside(scene.roadside));
        let car_distance = (0..3).map(|i| (scene.car[i] - scene.listener[i]).powi(2)).sum::<f32>().sqrt();
        out.extend(self.tyres.step(
            &sound.tyres,
            &inputs.splash,
            sound.speed,
            STEP,
            inside,
            (car_distance, scene.listener),
            || rng.rand(),
        ));
        out.extend(self.verges.step(sound.speed, car_distance, STEP));
        out.extend(self.impacts.step(&sound.hit, inside, || rng.rand()));
        let wheels = std::array::from_fn(|k| tyre::distance(&sound.tyres[k], scene.listener));
        let (suspension, mut calls) = self.suspension.step(sound, inside, wheels, STEP, || rng.rand());
        if frame {
            out.extend(self.drive_shafts.frame(sound, wheels, 1.0 / tyre::FRAME_RATE, || rng.rand()));
        }
        let min_rpm = ngp.min_rpm;
        let (exhaust, fire) = self.exhaust.step(sound, min_rpm, STEP, || rng.rand());
        out.extend(suspension.into_iter().chain(exhaust));
        let stopped = EngineNoise::from_payload(&sound.engine, min_rpm).stopped;
        let precipitation = self.world.sky.precipitation;
        out.extend(self.effects.step(sound, stopped, inside, precipitation, STEP, || rng.rand()));
        calls.extend(fire);
        calls.iter_mut().for_each(|c| *c = ngp_call(*c));
        let callouts = if self.custom.is_some() { &[][..] } else { &inputs.callouts };
        out.extend(self.codriver.step(sound, callouts, inputs.countdown));
        if let (Some(custom), Some(x)) = (&mut self.custom, inputs.driveline) {
            out.extend(custom.step(x, sound.speed, || rng.rand()));
        }
        out.extend(self.codriver.damage_calls(&calls));
        if frame {
            out.push(Command::Frame);
        }
        out.push(Command::SpeechGap(if inputs.race_time.is_some_and(|t| t <= 0.0) { SPEECH_GAP } else { 0.0 }));
        if let Some(fmod) = &mut self.fmod {
            out.retain(|c| !fmod.replaces(c));
            out.iter_mut().for_each(|c| fmod.mute(c));
            out.push(Command::Gain { voice: voice::Loop::Engine, gain: 0.0 });
            out.push(Command::Reflections { gain: 0.0, doors_off: self.car.doors_off });
            // NGP sends the daemon its block in app states 1 and 8 only, not in the start orbit (app state 10), where the
            // HUD's countdown has not started.
            let sent = !inputs.countdown.is_some_and(|c| c >= rbr_gameplay::race_clock::COUNTDOWN);
            fmod.model_pop = self.exhaust.model_pop;
            out.extend(fmod.step(inputs, scene, stopped, sent));
        }
        out
    }

    /// The commands for the scripted events of a step (the helicopter, bird strikes, the rescue call).
    pub fn events(&mut self, events: &Events, scene: &Scene) -> Vec<Command> {
        self.scripted.step(&Scene { listener_velocity: self.listener_velocity, ..*scene }, events)
    }
}

/// What [`Player::make`] opens the mixer with: the device's output (none offline), the fed mixer and its queue.
type Opened = Result<(Option<device::Output>, Arc<Mutex<Fed>>, Arc<Mutex<Vec<Command>>>), String>;

/// One car's sound, playing on the audio device, or rendered by the caller ([`Player::offline`]).
pub struct Player {
    models: Models,
    mixer: Arc<Mutex<Fed>>,
    /// The commands for the mixer, taken before its next block ([`posted`]).
    posted: Arc<Mutex<Vec<Command>>>,
    scene: Scene,
    /// `None` offline.
    output: Option<device::Output>,
    /// The rally school's bank is swapped into the mixer ([`Player::load_school`]).
    school_loaded: bool,
}

impl Player {
    /// Car `car`'s (the `Cars.ini` index) sound at `volume` (linear, 1 = the game's), under `sky`, with the
    /// co-driver's pace notes or not, and the player's group volumes (`mixer::Volumes`).
    pub fn start(
        data: &Data,
        car: u32,
        volume: f32,
        sky: Sky,
        codriver: bool,
        volumes: Volumes,
    ) -> Result<Player, String> {
        Player::make(data, car, sky, codriver, |bank| {
            let mut posted = None;
            let (output, mixer) = device::Output::start(|rate| {
                let (fed, queue) = Fed::new(mixer(bank, rate, volume, volumes));
                posted = Some(queue);
                fed
            })?;
            Ok((Some(output), mixer, posted.expect("the device's mixer")))
        })
    }

    /// [`Player::start`] without a device: the caller takes its samples at `rate` ([`Player::render`]).
    pub fn offline(
        data: &Data,
        car: u32,
        rate: u32,
        volume: f32,
        sky: Sky,
        codriver: bool,
        volumes: Volumes,
    ) -> Result<Player, String> {
        Player::make(data, car, sky, codriver, |bank| {
            let (fed, posted) = Fed::new(mixer(bank, rate, volume, volumes));
            Ok((None, Arc::new(Mutex::new(fed)), posted))
        })
    }

    fn make(
        data: &Data,
        car: u32,
        sky: Sky,
        codriver: bool,
        output: impl FnOnce(&CarBank) -> Opened,
    ) -> Result<Player, String> {
        let bank = CarBank::load(data, car)?;
        let (output, mixer, posted) = output(&bank)?;
        let mut models = Models::new(&bank, sky, Vec::new());
        models.codriver.muted = !codriver;
        Ok(Player { models, mixer, posted, scene: Scene::default(), output, school_loaded: false })
    }

    /// Posts `commands` for the mixer's next block (`posted`).
    fn post(&self, commands: &[Command]) {
        let mut posted = posted::lock(&self.posted);
        posted.extend_from_slice(commands);
        if posted.len() > posted::MOST {
            drop(posted);
            if let Ok(mut fed) = self.mixer.lock() {
                fed.catch_up();
            }
        }
    }

    /// Fills `out` with the next samples (an offline player's; a device's own are taken from it).
    pub fn render(&mut self, out: &mut [[f32; 2]]) {
        if let Ok(mut fed) = self.mixer.lock() {
            device::Render::render(&mut *fed, out);
        }
    }

    /// Each wheel's water as the renderer sees it (ours, [`tyre::Water`]); `None`, the default, plays 1.02's splash.
    pub fn set_water(&mut self, water: Option<[tyre::Water; 4]>) {
        self.models.tyres.set_water(water);
    }

    /// The windscreen as the renderer's glass model shows it after a frame (ours, [`effects::Glass`]): the wipers
    /// and splats play from it; `None`, the default, plays 1.02's wipers.
    pub fn set_glass(&mut self, glass: Option<effects::Glass>) {
        self.models.effects.set_glass(glass);
    }

    /// The car at the verge as the renderer sees it (ours, [`verge::Verge`]); `None`, the default, plays nothing of
    /// it.
    pub fn set_verge(&mut self, verge: Option<verge::Verge>) {
        self.models.verges.set(verge);
    }

    /// One physics step's inputs.
    pub fn step(&mut self, inputs: &Inputs) {
        let commands = self.models.step(inputs, &self.scene);
        self.post(&commands);
    }

    /// Applies `commands` to the mixer (the rally school's speech, which comes from the live record rather than
    /// a step's inputs; the co-driver switch does not mute it, as the original's handler does not test it).
    pub fn apply(&mut self, commands: &[Command]) {
        self.post(commands);
    }

    /// Swaps the rally school's English samples (`Audio\RS\ENG\<id>.ogg`) into the mixer, once; later calls
    /// with the same data are free. The samples are copied before taking the mixer lock; only the swap runs
    /// under it.
    pub fn load_school(&mut self, data: &Data) -> Result<(), String> {
        if self.school_loaded {
            return Ok(());
        }
        let path = data.audio_dat().ok_or("no Root/audio.dat")?;
        let dat = rbr_formats::audio::AudioDat::open(&path)?;
        let bank = crate::data::SchoolBank::load_eng(&dat)?;
        let prepared: std::collections::HashMap<u32, std::sync::Arc<crate::data::Wave>> =
            bank.words.into_iter().map(|(id, wave)| (id, std::sync::Arc::new(wave))).collect();
        self.swap(prepared, Mixer::swap_school);
        self.school_loaded = true;
        Ok(())
    }

    /// The scripted events since the last call (`rbr_render::characters`).
    pub fn events(&mut self, events: &Events) {
        let commands = self.models.events(events, &self.scene);
        self.post(&commands);
    }

    /// Swaps `prepared` into the mixer with `swap`, dropping what it replaces after the lock is released, so
    /// the audio callback never waits on the work.
    fn swap<T, U>(&self, prepared: T, swap: impl FnOnce(&mut Mixer, T) -> U) {
        // The commands posted before it apply first, as they would have.
        let swapped = self.mixer.lock().map(|mut fed| {
            fed.catch_up();
            swap(&mut fed.mixer, prepared)
        });
        let old = swapped.ok();
        drop(old);
    }

    /// Plays the car's FMOD bank (`fmod`) instead of its 1.02 engine, turbo and transmission voices. The loop
    /// waves are cloned and the mixing tree is built before taking the mixer lock; only the swap runs under it,
    /// and the old bank drops after it is released, so the audio callback never waits on the work.
    pub fn use_fmod(&mut self, car: FmodCar) {
        let rate = self.mixer.lock().map(|fed| fed.mixer.rate()).unwrap_or(44100);
        let prepared = Mixer::prepare_fmod(car.loop_waves(), car.waves.clone(), &car.buses, rate);
        self.swap(prepared, Mixer::swap_fmod);
        self.models.fmod = Some(car);
    }

    /// Calls the notes with a co-driver mod instead of 1.02's co-driver, muted as the co-driver switch is. The
    /// samples are copied before taking the mixer lock; only the swap runs under it.
    pub fn use_codriver(&mut self, mut custom: CustomCoDriver) {
        let prepared: Vec<Arc<data::Wave>> = custom.samples().iter().cloned().map(Arc::new).collect();
        self.swap(prepared, Mixer::swap_voice);
        custom.muted = self.models.codriver.muted;
        self.models.custom = Some(custom);
    }

    /// Gives the H-shifter's grind its sample (`Loop::Grind`). The seamless loop is built before taking the mixer
    /// lock; only the swap runs under it.
    pub fn set_grind(&mut self, wave: data::Wave) {
        let prepared = Arc::new(crate::mixer::voices::seamless(&wave));
        drop(wave);
        self.swap(prepared, Mixer::swap_grind);
    }

    /// The H-shifter's grind at `pitch` (1 = the sample's rate), or silent.
    pub fn grind(&mut self, pitch: Option<f32>) {
        let (gain, pitch) = pitch.map_or((0.0, 1.0), |p| (GRIND_GAIN, p));
        self.post(&[Command::Loop { voice: voice::Loop::Grind, gain, pitch }]);
    }

    /// Pauses or resumes the sound (the viewer's pause menu): the device stops asking the mixer, so every voice and
    /// the co-driver go on from where they stopped.
    pub fn set_paused(&self, paused: bool) {
        if let Some(output) = &self.output {
            output.set_paused(paused);
        }
    }

    /// Where the camera and the car are, and whether the camera is in the car (the in-car low-pass and the
    /// transmission voices).
    pub fn set_scene(&mut self, scene: Scene) {
        self.scene = scene;
    }
}

/// The mixer of `bank` at `rate`, `volume` and the group `volumes`.
fn mixer(bank: &CarBank, rate: u32, volume: f32, volumes: Volumes) -> Mixer {
    let mut mixer = Mixer::new(bank, rate, volume);
    mixer.volumes = volumes;
    mixer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_damper_and_fire_calls_are_swapped_only() {
        assert_eq!([7, 6, 8].map(ngp_call), [6, 7, 8]);
    }

    /// The verge adds its own voices and leaves the stones' timing, samples, gains and pitches as they were: it takes
    /// none of the shared random numbers.
    #[test]
    fn the_verge_keeps_the_stone_hits() {
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA is not set");
            return;
        };
        let bank = CarBank::load(&data, 0).unwrap();
        // Every wheel on surface 1 (gravel first) at 15 m/s, a force sum of 2 (a hit every 24 frames, 1.02's rate).
        let mut inputs = Inputs::default();
        inputs.sound.speed = 15.0;
        for block in &mut inputs.sound.tyres {
            (block[0x14], block[0x17], block[9], block[2]) = (1, 1, 15f32.to_bits(), 2f32.to_bits());
        }
        let mut touches = [None; verge::TOUCHES];
        touches[0] = Some(verge::Touch { id: 3, depth: 0.4, density: 1.0, branches: true });
        let verge = verge::Verge { grass: [1.0; 4], touches };
        let run = |with: bool| {
            let mut models = Models::new(&bank, Sky::default(), Vec::new());
            (0..600)
                .flat_map(|step| {
                    // A new frame every other step, off and on the tree.
                    if with && step % 2 == 0 {
                        let mut v = verge;
                        if step % 100 >= 50 {
                            v.touches = [None; verge::TOUCHES];
                        }
                        models.verges.set(Some(v));
                    }
                    models.step(&inputs, &Scene::default())
                })
                .collect::<Vec<_>>()
        };
        let (without, with) = (run(false), run(true));
        let stones = |c: &[Command]| -> Vec<Command> {
            c.iter().filter(|c| matches!(c, Command::Hit { shot: voice::Shot::Grus(_), .. })).copied().collect()
        };
        assert!(!stones(&without).is_empty());
        assert_eq!(stones(&without), stones(&with));
        let branches = with.iter().filter(|c| matches!(c, Command::Hit { shot: voice::Shot::Branch(_), .. })).count();
        assert_eq!(branches, 6);
        let grass = |c: &Command| matches!(c, Command::Loop { voice: voice::Loop::Grass, gain, .. } if *gain > 0.0);
        assert!(with.iter().any(grass) && !without.iter().any(grass));
    }

    /// The player's posted commands play as the mixer applying them at once did: the same samples, block for block,
    /// with a grind and a few steps between blocks.
    #[test]
    fn posted_commands_play_as_applied() {
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA is not set");
            return;
        };
        let (rate, volumes) = (44100, Volumes::default());
        let bank = CarBank::load(&data, 0).unwrap();
        let mut player = Player::offline(&data, 0, rate, 1.0, Sky::default(), true, volumes).unwrap();
        let mut models = Models::new(&bank, Sky::default(), Vec::new());
        let mut direct = mixer(&bank, rate, 1.0, volumes);
        let mut inputs = Inputs::default();
        let (mut posted, mut applied) = (vec![[0.0; 2]; 300], vec![[0.0; 2]; 300]);
        let mut heard = false;
        for step in 0..400 {
            inputs.sound.speed = step as f32 * 0.1;
            player.step(&inputs);
            direct.apply(&models.step(&inputs, &Scene::default()));
            if step % 50 == 0 {
                let pitch = (step % 100 == 0).then_some(1.0 + step as f32 * 0.001);
                player.grind(pitch);
                let (gain, pitch) = pitch.map_or((0.0, 1.0), |p| (GRIND_GAIN, p));
                direct.apply(&[Command::Loop { voice: voice::Loop::Grind, gain, pitch }]);
            }
            if step % 3 == 0 {
                player.render(&mut posted);
                direct.render(&mut applied);
                assert_eq!(posted, applied, "step {step}");
                heard |= applied.iter().any(|s| s[0] != 0.0);
            }
        }
        assert!(heard);
    }
}
