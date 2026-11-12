//! The voice commands the models give and the mixer plays: what the original sets on its DirectSound voices
//! through the voice vtable (rbr-recomp `notes/sound.md` 2.2) and on the engine stream.

use crate::engine::EngineNoise;

/// The looping voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Loop {
    /// The granular engine stream.
    Engine,
    /// `CAR::TRANSMISSION Transmission`, at the gearbox and the diff pitch.
    Gearbox,
    Diff,
    /// `CAR::OTHER Turbo`.
    Turbo,
    /// `AMBIENTFX wind` (`CAmbientSounds::Vf48` 0x4ab310).
    Wind,
    /// `Audio\Ambient\siren.wav` (`E31_5bf0` 0x4d5bf0).
    Siren,
    /// `Audio\Ambient\heli_g.wav` (`E31_5a30` 0x4d5a30).
    Helicopter,
    /// `Audio\Cars\Effects\cooler.wav` (`CCarSound::Md2b0` 0x4ad2b0).
    Cooler,
    /// The H-shifter's grind (ours, `Player::set_grind`): silent until a sample is given.
    Grind,
    /// Ours: the wiper motor while the blades move (`effects::Glass`, `data::GlassBank::motor`).
    WiperMotor,
    /// Ours (`crate::verge`): the tyres' swish on grass and the bushes brushing the body (`data::CarBank::grass`,
    /// `data::CarBank::brush`).
    Grass,
    Brush,
    /// Ours (`crate::impact`): the body scraping along the road or a bank (`data::CarBank::scrape`).
    Scrape,
}

/// The one-shots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shot {
    /// `CAR::TRANSMISSION GearChange1-3`, one at random.
    GearChange,
    /// `CAR::OTHER Valve`.
    BlowOff,
    /// `CAR::OTHER Offroad1-7`, by index.
    Offroad(usize),
    /// `IMPACT::RANGED Snd0-19`, by index.
    Ranged(usize),
    /// `SFX::INGAME GO` (`Audio\Game\Go.wav`).
    Go,
    /// `IMPACT::SPEECH` ("ugh"), by index.
    Scream(usize),
    /// `AMBIENTFX bird1-7`, by index.
    Bird(usize),
    /// `IMPACT::VARIOUS` sample 0, `birdhit` (`CCollisionSnd::Vf68` 0x4b2c50).
    BirdHit,
    /// `IMPACT::GRUS Snd1-21`, a gravel hit (`CTireSnd::M7330` 0x4d7330), by index.
    Grus(usize),
    /// `CAR::SQUEEKS Sqe1-18` (`CTireSnd::M7700` 0x4d7700) and `CAR::SUSPENSION Sus1-4` (`M77c0` 0x4d77c0).
    Squeak(usize),
    Bottoming(usize),
    /// Ours: a tyre running into a puddle (`tyre::Water`), one of the embedded splashes (`data::CarBank::splashes`).
    Splash(usize),
    /// Ours: a branch slapping the body (`crate::verge`), one of the embedded hits (`data::CarBank::branches`).
    Branch(usize),
    /// `CAR::DRIVESHAFT DS1-13` (`CTireSnd::M7590` 0x4d7590), by index.
    DriveShaft(usize),
    /// `CAR::EXHAUST Pop1-11` (`CCarSound::Mf280` 0x4af280), `CARn::EXHAUSTNEW Pop1-11` (`Mf180` 0x4af180) and
    /// `CAR::BANGS bang1-6` (`Mf3f0` 0x4af3f0), by index.
    ExhaustPop(usize),
    Pop(usize),
    Bang(usize),
    /// `Audio\Cars\Effects\fire.wav` (`CCarSound::Md1c0` 0x4ad1c0).
    Fire,
    /// `CAR::IGNITION Ignition1` (`CCarSound::Vf54` 0x4ad090).
    Ignition,
    /// `CAR::DOGRINGS DR1-8` (`E31Transmission::M3510` 0x4d3510), by index.
    DogRing(usize),
    /// `CAR::WIPER WiperEngine`, `WiperUp` and `WiperDown` (`CCarSound::Md490` 0x4ad490).
    WiperEngine,
    WiperUp,
    WiperDown,
    /// Ours: a wiper stroke on wet glass and on dry glass (`effects::Glass`, `data::GlassBank`), by index, and water
    /// slapping the windscreen.
    WiperWet(usize),
    WiperDry(usize),
    GlassSplat(usize),
}

/// The per-wheel loops of `CTireSnd` besides the surface voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelLoop {
    /// `CAR::RUMBLE Rumble` (`M6750` 0x4d6750), in the car only.
    Rumble,
    /// `TIRE::SURFACES SkidWater`, the splash (`M5fd0`, from `M6560`'s `CSigTireParticles` +0x98); ours, also the
    /// swish in a puddle (`tyre::Water`).
    Wetness,
    /// Ours: the wet road's tyre roar (`tyre::Water`, `data::CarBank::wet_road`).
    Hiss,
}

/// How a one-shot treats the last one of its kind still playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    /// Both play.
    Overlap,
    /// The last one stops (the pops and the bang: one voice each, freed before the next).
    Restart,
    /// The new one is skipped (the fire: `Md1c0` asks whether its voice still plays; the blow-off, ours).
    IfIdle,
}

/// The surface materials of the tyre loops (`TIRE::SURFACES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Material {
    Gravel,
    Tarmac,
    Snow,
}

/// What the speech voice says (rbr-recomp `notes/sound.md` 10.3): each is one sample, played back to back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Speech {
    /// A pace-note word id (`Word5880` 0x4b5880): a random take of its bank.
    Word(u32),
    /// `NUMBER::NUMBERS` sample `n - 1` (the countdown's numbers).
    Number(u32),
    /// A damage call (`audio\speech\broken\eng\<name>.ogg`), by its code 1-8.
    Damage(u32),
    /// A co-driver mod's sample (`crate::custom_codriver`), by its index in the mod's voice.
    Sample(u32),
    /// A rally-school sample (`Audio\RS\ENG\<id>.ogg`, English only): the briefing and the end pages queue at
    /// priority 0, `Trigger`'s ids at priority 1 (`E25School::OnSignal` 0x4b8200; [`crate::mixer::speech_slot`]
    /// orders them with the pace notes' 0 and the damage calls' 2).
    School { id: u32, priority: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// The engine stream's input for the next firing periods.
    Engine(EngineNoise),
    /// The camera is in the car: the engine takes the in-car low-pass.
    Inside(bool),
    /// A voice's linear gain.
    Gain { voice: Loop, gain: f32 },
    /// A loop's gain and pitch (1 = the recorded rate).
    Loop { voice: Loop, gain: f32, pitch: f32 },
    /// Plays a one-shot once.
    Shot { shot: Shot, gain: f32 },
    /// Plays a one-shot once on side `pan` (-1 left to 1 right), its distance gain already in `gain`.
    ShotAt { shot: Shot, gain: f32, pan: f32 },
    /// A one-shot at pitch `pitch` (1 = the recorded rate), at the car.
    Effect { shot: Shot, gain: f32, pitch: f32, repeat: Repeat },
    /// A gravel hit, squeak, bottoming or driveshaft clunk at pitch `pitch`, `distance` m from the listener (at its wheel).
    Hit { shot: Shot, gain: f32, pitch: f32, distance: f32 },
    /// A wheel's rumble or wetness loop.
    WheelLoop { wheel: u8, voice: WheelLoop, gain: f32, pitch: f32 },
    /// One of a wheel's two surface voices: its material's normal or skid loop.
    Tyre { wheel: u8, slot: u8, material: Material, skid: bool, gain: f32, pitch: f32 },
    /// Queues a sample on the speech voice.
    Speech(Speech),
    /// Stops the speech voice and clears its queue (`E25VBack::S10` and `E32_SpClearAll`, as `OnSignal`'s stop does).
    StopSpeech,
    /// Clears the speech queue's pending entries, leaving the playing word (`E32_SpClearAll` as `M80c0x`
    /// calls it before queueing `Trigger`'s ids).
    FlushSpeech,
    /// A game frame: the original's mixer counts the sounds playing (`E31Master::Vf08` 0x4d4980), the count the
    /// one-shots asked for until the next frame see ([`crate::mixer::SOUND_LIMIT`]).
    Frame,
    /// The pause after each word (`E31Speech::Vf08` 0x4d2e30: before the race clock runs), seconds.
    SpeechGap(f32),
    /// Where the car is from the camera: its distance, metres, and its side (-1 left to 1 right). The car's voices
    /// take their `SOUNDDEF` distance gain from it, and the engine's outside low-pass its cutoff. `car_doppler` is the
    /// pitch factor their motion gives them (1 from inside the car; ours).
    Listener { car_distance: f32, car_pan: f32, car_doppler: f32 },
    /// A loop's side (-1 left to 1 right).
    Pan { voice: Loop, pan: f32 },
    /// The rain stream's gain (0 stops it) and the precipitation amount (50 light, 100 heavy).
    Rain { gain: f32, amount: f32 },
    /// Ours: the recorded rain heard in the car (`world::cabin_rain`): the light and heavy loops' gains, and their
    /// pitch.
    CabinRain { gains: [f32; 2], pitch: f32 },
    /// The engine reflections' gain, and whether each door (left, right) is off the car (`crate::reflections`).
    Reflections { gain: f32, doors_off: [bool; 2] },
    /// The roadside the reflections follow (ours; `None`: 1.02's fixed delays).
    Roadside(Option<crate::reflections::Roadside>),
    /// An FMOD bank's loop (`crate::fmod`, ours): slot `slot`'s gain and pitch, outside the car or in it.
    FmodLoop { slot: u16, gain: f32, pitch: f32, outside: bool },
    /// An FMOD bus's fader gain (`mixer::fmod_mix`).
    FmodBus { bus: u16, gain: f32 },
    /// Parameter `param` of an FMOD bus's effect `effect` (those before its fader, then those after; `mixer::dsp`).
    FmodEffect { bus: u16, effect: u8, param: u8, value: f32 },
    /// An FMOD bank's recording `wave` played once into bus `bus` (`None`: the master), as slot `slot`'s one-shot.
    FmodShot { wave: u16, gain: f32, pitch: f32, outside: bool, bus: Option<u16>, slot: Option<u16> },
    /// The gain and pitch slot `slot`'s playing one-shots take now (FMOD follows a playing instrument's automations).
    FmodTrack { slot: u16, gain: f32, pitch: f32, outside: bool },
}
