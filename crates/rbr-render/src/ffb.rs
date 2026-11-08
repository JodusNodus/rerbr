//! The steering wheel's force feedback in live mode, through SDL3's haptic API (`notes/ngp.md` "Force feedback"): one
//! constant-force effect on the wheel's steering axis, its level the latest magnitude (`rbr_gameplay::ffb`). A thread
//! owns the device and, at `rate` Hz as NGP's FFB thread does (`ffbRate`, 1000 by default), updates and restarts the
//! effect with the latest level whether or not it changed, which is what RSF's `SetParameters(DIEP_START)` gives at
//! the wheel's evdev device under Proton (`harness/ffb_trace.sh`); under NGP it applies the dead zone there against the
//! latest steering. SDL's open sets the device's gain to 100 % and its autocentre off, as Proton's SDL bus does.
//!
//! A positive magnitude turns the wheel left (SDL's level on `SDL_HAPTIC_STEERING_AXIS`, measured on a Logitech
//! G923 under Linux): the force opposes the steering, so it centres the wheel. `inverse` flips it for a wheel that
//! turns the other way.

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use rbr_gameplay::ffb::{self, Settings, MAX_MAGNITUDE};
use sdl3_sys::error::SDL_GetError;
use sdl3_sys::haptic::*;
use sdl3_sys::init::{SDL_InitSubSystem, SDL_QuitSubSystem, SDL_INIT_HAPTIC, SDL_INIT_JOYSTICK};

/// The surfaces of [`Options::sensitivity`], as its keys name them.
pub const SURFACES: [&str; 3] = ["gravel", "tarmac", "snow"];

/// How the wheel is driven.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub settings: Settings,
    /// How often the thread resends the level, Hz (NGP's `ffbRate`, capped at 1000).
    pub rate: u32,
    /// A part of the device's name; `None` takes the first device with a constant force.
    pub device: Option<String>,
    /// NGP's force-feedback sensitivity for gravel, tarmac and snow, percent (`ForceFeedbackSensitivity<Surface>`;
    /// `rbr-standalone --ffb-sensitivity`): below 100 the force is stronger and its peaks flatter, above 100 lighter.
    pub sensitivity: [i32; 3],
    /// The wheel's most lock-to-lock rotation, degrees (`crate::wheel_rotation`); `None` leaves the wheel's own.
    pub rotation: Option<u16>,
    /// The rotation is the car's own lock ([`crate::wheel_rotation::car_degrees`]), up to [`Options::rotation`].
    pub rotation_per_car: bool,
    /// The sensitivity is automatic ([`AutoCars`]): each car's from the player's own driving, with
    /// [`Options::sensitivity`] and [`CarSensitivity`] for a surface the car has no reference on yet. Off, those two
    /// alone (manual).
    pub auto: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            settings: Settings::default(),
            rate: 1000,
            device: None,
            sensitivity: [100; 3],
            rotation: None,
            rotation_per_car: false,
            auto: true,
        }
    }
}

impl Options {
    /// The `[ffb]` section of `text` (the launcher's settings file); the defaults for whatever it leaves out.
    pub fn parse(text: &str) -> Options {
        let mut out = Options::default();
        let sections = rbr_formats::ini::parse(text).unwrap_or_default();
        let Some(section) = sections.iter().find(|s| s.name == "ffb") else {
            return out;
        };
        let flag = |key: &str, default: bool| section.get(key).map_or(default, |v| v == "1" || v == "true");
        let number = |key: &str, default: f32| section.get(key).and_then(|v| v.parse().ok()).unwrap_or(default);
        let s = &mut out.settings;
        s.enabled = flag("enabled", s.enabled);
        s.strength = number("strength", s.strength).clamp(0.0, 1.0);
        s.inverse = flag("inverse", s.inverse);
        s.dead_zone = number("dead_zone", s.dead_zone).max(0.0);
        s.dead_zone_threshold = number("dead_zone_threshold", s.dead_zone_threshold).max(0.0);
        out.rate = section.get("rate").and_then(|v| v.parse().ok()).unwrap_or(out.rate).clamp(1, 1000);
        out.device = section.get("device").filter(|v| !v.is_empty()).map(str::to_owned);
        for (value, surface) in out.sensitivity.iter_mut().zip(SURFACES) {
            *value = section.get(&format!("sensitivity_{surface}")).and_then(|v| v.parse().ok()).unwrap_or(100);
        }
        out.rotation =
            section.get("rotation").and_then(|v| v.parse().ok()).filter(|v| crate::wheel_rotation::DEGREES.contains(v));
        out.rotation_per_car = flag("rotation_per_car", false);
        out.auto = flag("auto", true);
        out
    }

    /// The rotation a drive takes, `car` the car's own lock if known.
    pub fn rotation_for(&self, car: Option<u16>) -> Option<u16> {
        let max = self.rotation?;
        let min = *crate::wheel_rotation::DEGREES.start();
        Some(car.filter(|_| self.rotation_per_car).map_or(max, |d| d.clamp(min, max)))
    }

    pub fn to_text(&self) -> String {
        let s = &self.settings;
        format!(
            "[ffb]\nenabled = {}\nstrength = {}\ninverse = {}\ndead_zone = {}\n\
             dead_zone_threshold = {}\nrate = {}\ndevice = {}\nsensitivity_gravel = {}\nsensitivity_tarmac = {}\n\
             sensitivity_snow = {}\nrotation = {}\nrotation_per_car = {}\nauto = {}\n",
            u8::from(s.enabled),
            s.strength,
            u8::from(s.inverse),
            s.dead_zone,
            s.dead_zone_threshold,
            self.rate,
            self.device.as_deref().unwrap_or(""),
            self.sensitivity[0],
            self.sensitivity[1],
            self.sensitivity[2],
            self.rotation.map_or(String::new(), |d| d.to_string()),
            u8::from(self.rotation_per_car),
            u8::from(self.auto),
        )
    }
}

/// The sensitivities' range, percent, in the launcher and the pause menu (community cars go up to 372).
pub const SENSITIVITY_RANGE: std::ops::RangeInclusive<i32> = 10..=500;

/// Each car's own NGP sensitivities, in place of [`Options::sensitivity`]: the `[ffb_cars]` section of the launcher's
/// settings file, by `crate::camera_prefs::car_key`, which the pause menu's Force feedback page
/// (`crate::hud::ffb_page`) and the launcher's Controls page both write in place, as `[camera]`:
///
/// ```ini
/// [ffb_cars]
/// rsf-107 = 352,280,372
/// ```
///
/// An older `[ffb]` `sensitivity_car<id> = g,t,s` (RSF car `id`) is read as `rsf-<id>` where the section has none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CarSensitivity {
    pub cars: BTreeMap<String, [i32; 3]>,
}

/// [`CarSensitivity`]'s section.
pub const CARS_SECTION: &str = "ffb_cars";

impl CarSensitivity {
    /// The cars' sensitivities in `text` (a whole settings file); a malformed line is left out.
    pub fn parse(text: &str) -> CarSensitivity {
        let sections = rbr_formats::ini::parse(text).unwrap_or_default();
        let values = |value: &str| -> Option<[i32; 3]> {
            let values: Vec<i32> = value.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
            values.try_into().ok()
        };
        let mut cars = BTreeMap::new();
        for (key, value) in sections.iter().filter(|s| s.name == CARS_SECTION).flat_map(|s| &s.entries) {
            if let Some(values) = values(value) {
                cars.insert(key.clone(), values);
            }
        }
        for (key, value) in sections.iter().filter(|s| s.name == "ffb").flat_map(|s| &s.entries) {
            let id = key.strip_prefix("sensitivity_car").and_then(|c| c.parse::<u32>().ok());
            if let (Some(id), Some(values)) = (id, values(value)) {
                cars.entry(format!("rsf-{id}")).or_insert(values);
            }
        }
        CarSensitivity { cars }
    }

    /// Car `key`'s sensitivities, else `global`.
    pub fn of(&self, key: Option<&str>, global: [i32; 3]) -> [i32; 3] {
        key.and_then(|k| self.cars.get(k)).copied().unwrap_or(global)
    }

    /// The section's text.
    pub fn to_text(&self) -> String {
        let cars: String = self.cars.iter().map(|(car, [g, t, s])| format!("{car} = {g},{t},{s}\n")).collect();
        format!("[{CARS_SECTION}]\n{cars}")
    }

    /// `file` (a whole settings file) with this section in place of its own, and without the older `[ffb]` entries.
    pub fn replace_in(&self, file: &str) -> String {
        let mut section = "";
        let kept: String = file
            .lines()
            .filter(|line| {
                if let Some((name, _)) = line.trim().strip_prefix('[').and_then(|r| r.split_once(']')) {
                    section = name.trim();
                }
                !(section == "ffb" && line.trim_start().starts_with("sensitivity_car"))
            })
            .map(|line| format!("{line}\n"))
            .collect();
        rbr_formats::ini::replace_section(&kept, CARS_SECTION, &self.to_text())
    }
}

/// The automatic sensitivity (ours, `notes/ngp.md` "Force feedback"): NGP's force is `1 - 0.975^(|T| 100 / S)` with
/// `S` the car's `ForceFeedbackSensitivity_NGP` times the percentage / 100, and the cars' steering torque `T` differs
/// about 4× at the same grip, so one percentage is light in one car and flat at the wheel's end in another. The
/// statistic [`torque_ratio`] is `|T| 100 / S` of the car, read back from the force; a car's reference is its 90th
/// percentile over the player's own driving, per surface, and the percentage that puts the reference at
/// [`AUTO_TARGET`] evens the cars out.
pub mod auto {
    /// NGP's curve's base.
    const BASE: f32 = 0.975;
    /// The force a car's reference gives at its automatic percentage.
    pub const TARGET: f32 = 0.5;
    /// The automatic percentage's range, after the trim.
    pub const RANGE: std::ops::RangeInclusive<i32> = 10..=1000;
    /// The trim's range, percent of the automatic percentage.
    pub const TRIM_RANGE: std::ops::RangeInclusive<i32> = 50..=200;
    /// Seconds of kept driving a car with no reference on the surface learns one from, once per drive.
    pub const LEARN_SECONDS: f32 = 60.0;
    /// Seconds the wheel takes to move to the learned percentage.
    pub const RAMP_SECONDS: f32 = 3.0;
    /// The fewest seconds of kept driving a drive's end adds to the car's reference.
    pub const KEEP_SECONDS: f32 = 20.0;
    /// The most seconds a reference rests on: older driving weighs no more than this.
    pub const MAX_SECONDS: f32 = 600.0;
    /// The slowest kept speed, m/s.
    pub const MIN_SPEED: f32 = 10.0;
    /// A force past this, after the reshape, is the wheel maxed out.
    pub const CLIPPED: f32 = 0.95;
    /// The clipping share's window, seconds of kept driving.
    pub const CLIP_WINDOW: f32 = 60.0;

    /// NGP's percentage as its `sensitivity` takes it: 0 or less is 100.
    pub(super) fn percent(p: i32) -> f32 {
        if p > 0 {
            p as f32
        } else {
            100.0
        }
    }

    /// `|T| 100 / S_car` of a force `force` NGP gave at percentage `started` on the drive's surface: its curve
    /// inverted, `(p0 / 100) ln(1 - |f|) / ln(0.975)`. None at the curve's end (`|f| ≥ 0.9999`), where it says little.
    pub fn torque_ratio(force: f32, started: i32) -> Option<f32> {
        let f = force.abs();
        (f < 0.9999).then(|| percent(started) / 100.0 * (-f).ln_1p() / BASE.ln())
    }

    /// The 90th percentile of `samples` (the nearest rank).
    pub fn percentile90(samples: &[f32]) -> Option<f32> {
        let mut v = samples.to_vec();
        let k = ((v.len().checked_sub(1)?) as f32 * 0.9).round() as usize;
        Some(*v.select_nth_unstable_by(k, f32::total_cmp).1)
    }

    /// The percentage that puts the ratio `x` at [`TARGET`]: `100 x ln(0.975) / ln(1 - TARGET)`.
    pub fn percent_for(x: f32) -> f32 {
        100.0 * x * BASE.ln() / (1.0 - TARGET).ln()
    }

    /// The percentage a drive takes for reference ratio `x` and trim `trim` (percent), within [`RANGE`].
    pub fn effective(x: f32, trim: i32) -> i32 {
        ((percent_for(x) * trim as f32 / 100.0).round() as i32).clamp(*RANGE.start(), *RANGE.end())
    }

    /// A car's reference on one surface: the ratio `x` and the seconds of driving it rests on.
    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    pub struct Reference {
        pub x: f32,
        pub seconds: f32,
    }

    impl Reference {
        /// This one and a drive's `new`, weighted by their seconds, resting on at most [`MAX_SECONDS`].
        pub fn blend(self, new: Reference) -> Reference {
            let total = self.seconds + new.seconds;
            if total <= 0.0 {
                return self;
            }
            Reference { x: (self.x * self.seconds + new.x * new.seconds) / total, seconds: total.min(MAX_SECONDS) }
        }
    }

    /// One drive's samples on its surface (the viewer's): the ratios of the kept frames, the kept seconds, the
    /// reference learned during it, and the frames of the last [`CLIP_WINDOW`] seconds that maxed the wheel out.
    #[derive(Debug, Clone, Default)]
    pub struct Drive {
        pub surface: usize,
        samples: Vec<f32>,
        pub seconds: f32,
        /// The reference learned at [`LEARN_SECONDS`], and the seconds since.
        pub learned: Option<(Reference, f32)>,
        clips: std::collections::VecDeque<(f32, bool)>,
    }

    impl Drive {
        pub fn new(surface: usize) -> Drive {
            Drive { surface, ..Drive::default() }
        }

        /// One frame of `dt` seconds: with `kept`, NGP's force `force` at percentage `started` and the wheel's
        /// `shaped` one (after the reshape) are sampled; with `learn`, the reference is learned once the kept
        /// seconds reach [`LEARN_SECONDS`].
        pub fn frame(&mut self, force: f32, shaped: f32, started: i32, dt: f32, kept: bool, learn: bool) {
            if let Some((_, since)) = &mut self.learned {
                *since += dt;
            }
            if !kept {
                return;
            }
            self.samples.extend(torque_ratio(force, started));
            self.seconds += dt;
            self.clips.push_back((self.seconds, shaped.abs() > CLIPPED));
            while self.clips.front().is_some_and(|&(at, _)| self.seconds - at > CLIP_WINDOW) {
                self.clips.pop_front();
            }
            if learn && self.learned.is_none() && self.seconds >= LEARN_SECONDS {
                self.learned = self.reference().map(|r| (r, 0.0));
            }
        }

        /// The drive's own reference so far.
        pub fn reference(&self) -> Option<Reference> {
            percentile90(&self.samples).map(|x| Reference { x, seconds: self.seconds })
        }

        /// The share of the kept frames of the last [`CLIP_WINDOW`] seconds that maxed the wheel out.
        pub fn clipped(&self) -> Option<f32> {
            let n = self.clips.len();
            (n > 0).then(|| self.clips.iter().filter(|(_, c)| *c).count() as f32 / n as f32)
        }

        /// The percentage on the way from `from` to `to` [`RAMP_SECONDS`] after learning.
        pub fn ramp(&self, from: i32, to: i32) -> i32 {
            let share = self.learned.map_or(1.0, |(_, since)| (since / RAMP_SECONDS).min(1.0));
            (from as f32 + (to - from) as f32 * share).round() as i32
        }
    }
}

use auto::{Drive, Reference};

/// Each car's automatic-sensitivity state ([`auto`]), by `crate::camera_prefs::car_key`: its reference per surface
/// (`[ffb_auto]`, `x/seconds` each, `0/0` none) and the player's trim per surface (`[ffb_trim]`, percent of the
/// automatic percentage, written only when not all 100). The viewer writes the references at a drive's end and the
/// pause menu the trims, each car's entry in place; the launcher's Controls page edits both and its save carries the
/// file's over.
///
/// ```ini
/// [ffb_auto]
/// rsf-81 = 145.2/320, 130.5/600, 0/0
///
/// [ffb_trim]
/// rsf-81 = 100,110,100
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoCars {
    pub references: BTreeMap<String, [Reference; 3]>,
    pub trims: BTreeMap<String, [i32; 3]>,
}

/// Three comma-separated values, each read by `one`.
fn three<T>(value: &str, one: impl Fn(&str) -> Option<T>) -> Option<[T; 3]> {
    value.split(',').map(|v| one(v.trim())).collect::<Option<Vec<_>>>()?.try_into().ok()
}

/// [`AutoCars`]'s sections.
pub const AUTO_SECTION: &str = "ffb_auto";
pub const TRIM_SECTION: &str = "ffb_trim";

impl AutoCars {
    /// The cars' states in `text` (a whole settings file); a malformed line is left out.
    pub fn parse(text: &str) -> AutoCars {
        let sections = rbr_formats::ini::parse(text).unwrap_or_default();
        let entries = |name: &'static str| sections.iter().filter(move |s| s.name == name).flat_map(|s| &s.entries);
        let reference = |v: &str| {
            let (x, seconds) = v.split_once('/')?;
            let r = Reference { x: x.trim().parse().ok()?, seconds: seconds.trim().parse().ok()? };
            (r.x.is_finite() && r.x >= 0.0 && r.seconds.is_finite() && r.seconds >= 0.0).then_some(r)
        };
        let references = entries(AUTO_SECTION)
            .filter_map(|(key, value)| Some((key.clone(), three(value, reference)?)))
            .filter(|(_, r)| r.iter().any(|r| r.seconds > 0.0))
            .collect();
        let trim = |v: &str| v.parse::<i32>().ok();
        let trims = entries(TRIM_SECTION).filter_map(|(key, value)| Some((key.clone(), three(value, trim)?))).collect();
        AutoCars { references, trims }
    }

    /// Car `key`'s reference on `surface`, if it has one.
    pub fn reference(&self, key: &str, surface: usize) -> Option<Reference> {
        self.references.get(key).map(|r| r[surface.min(2)]).filter(|r| r.seconds > 0.0)
    }

    /// Car `key`'s trim on `surface`, percent.
    pub fn trim(&self, key: &str, surface: usize) -> i32 {
        self.trims.get(key).map_or(100, |t| t[surface.min(2)])
    }

    /// The automatic percentage of car `key` on `surface`, with its trim; none without a reference.
    pub fn effective(&self, key: &str, surface: usize) -> Option<i32> {
        self.reference(key, surface).map(|r| auto::effective(r.x, self.trim(key, surface)))
    }

    /// Sets car `key`'s trim on `surface`; a car at 100 on every surface has no entry.
    pub fn set_trim(&mut self, key: &str, surface: usize, trim: i32) {
        let mut trims = self.trims.get(key).copied().unwrap_or([100; 3]);
        trims[surface.min(2)] = trim;
        if trims == [100; 3] {
            self.trims.remove(key);
        } else {
            self.trims.insert(key.to_owned(), trims);
        }
    }

    /// Sets car `key`'s reference on `surface`.
    pub fn set_reference(&mut self, key: &str, surface: usize, reference: Reference) {
        self.references.entry(key.to_owned()).or_default()[surface.min(2)] = reference;
    }

    /// Car `key`'s state, from `other` (the file as it is now) into this one.
    pub fn take_car(&mut self, key: &str, other: &AutoCars) {
        match other.references.get(key) {
            Some(r) => _ = self.references.insert(key.to_owned(), *r),
            None => _ = self.references.remove(key),
        }
        match other.trims.get(key) {
            Some(t) => _ = self.trims.insert(key.to_owned(), *t),
            None => _ = self.trims.remove(key),
        }
    }

    /// The state saved under a car's earlier key `old` goes to `now`; one already there stays.
    pub fn rename(&mut self, old: &str, now: &str) {
        if let Some(r) = self.references.remove(old) {
            self.references.entry(now.to_owned()).or_insert(r);
        }
        if let Some(t) = self.trims.remove(old) {
            self.trims.entry(now.to_owned()).or_insert(t);
        }
    }

    /// The two sections' text.
    pub fn to_text(&self) -> String {
        format!("{}\n{}", self.references_text(), self.trims_text())
    }

    fn references_text(&self) -> String {
        let line = |(car, r): (&String, &[Reference; 3])| {
            let r = r.map(|r| if r.seconds > 0.0 { format!("{:.1}/{:.0}", r.x, r.seconds) } else { "0/0".into() });
            format!("{car} = {}\n", r.join(", "))
        };
        format!("[{AUTO_SECTION}]\n{}", self.references.iter().map(line).collect::<String>())
    }

    fn trims_text(&self) -> String {
        let line = |(car, [g, t, s]): (&String, &[i32; 3])| format!("{car} = {g},{t},{s}\n");
        format!("[{TRIM_SECTION}]\n{}", self.trims.iter().map(line).collect::<String>())
    }

    /// `file` (a whole settings file) with these sections in place of its own.
    pub fn replace_in(&self, file: &str) -> String {
        let file = rbr_formats::ini::replace_section(file, AUTO_SECTION, &self.references_text());
        rbr_formats::ini::replace_section(&file, TRIM_SECTION, &self.trims_text())
    }
}

/// The percentages a drive starts NGP's car `key` with ([`started`]'s `--ffb-sensitivity`): per surface, in
/// automatic mode the car's automatic percentage where it has a reference; otherwise its own manual ones
/// ([`CarSensitivity`]), else every car's ([`Options::sensitivity`]).
pub fn launch(options: &Options, cars: &CarSensitivity, autos: &AutoCars, key: Option<&str>) -> [i32; 3] {
    let manual = cars.of(key, options.sensitivity);
    std::array::from_fn(|s| key.filter(|_| options.auto).and_then(|k| autos.effective(k, s)).unwrap_or(manual[s]))
}

/// The pause menu's force-feedback choices for one drive (`crate::hud::ffb_page`), and in automatic mode its
/// learning: the settings file the car's own choices live in (`viewer --bindings`), the car's key, every car's and
/// the ones NGP's car started with, the strength, and the drive's samples ([`auto::Drive`]).
#[derive(Debug, Clone, Default)]
pub struct Session {
    path: Option<PathBuf>,
    car: Option<String>,
    pub cars: CarSensitivity,
    pub autos: AutoCars,
    /// [`Options::sensitivity`].
    pub global: [i32; 3],
    /// What NGP's car started with ([`started`]).
    pub started: [i32; 3],
    /// [`Options::auto`].
    pub auto: bool,
    /// The wheel's strength, 0 to 1 (`Settings::strength`).
    pub strength: f32,
    /// The drive's samples, from its first kept frame.
    drive: Option<Drive>,
}

impl Session {
    /// The choices in `path` (none without it or when it does not read) for car `car`
    /// (`crate::camera_prefs::car_key`), with `options` the file's `[ffb]`.
    pub fn load(path: Option<&Path>, car: Option<String>, options: &Options, started: [i32; 3]) -> Session {
        let text = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        Session {
            path: path.map(Path::to_owned),
            car,
            cars: CarSensitivity::parse(&text),
            autos: AutoCars::parse(&text),
            global: options.sensitivity,
            started,
            auto: options.auto,
            strength: options.settings.strength,
            drive: None,
        }
    }

    /// The car's manual sensitivities now.
    pub fn now(&self) -> [i32; 3] {
        self.cars.of(self.car.as_deref(), self.global)
    }

    /// Whether the car has its own manual sensitivities.
    pub fn own(&self) -> bool {
        self.car.as_ref().is_some_and(|c| self.cars.cars.contains_key(c))
    }

    /// Sets the car's sensitivity on `surface` (0 gravel, 1 tarmac, 2 snow), or with `None` drops the car's own, and
    /// writes it to the file: this car's entry only, into the file as it is now.
    pub fn set(&mut self, surface: usize, value: Option<i32>) {
        let Some(car) = self.car.clone() else { return };
        let change = |cars: &mut CarSensitivity, now: [i32; 3]| match value {
            Some(value) => {
                let mut values = now;
                values[surface.min(2)] = value;
                cars.cars.insert(car.clone(), values);
            }
            None => _ = cars.cars.remove(&car),
        };
        let now = self.now();
        change(&mut self.cars, now);
        self.write(|file| {
            let mut cars = CarSensitivity::parse(file);
            change(&mut cars, now);
            cars.replace_in(file)
        });
    }

    /// The car's trim on `surface`, percent.
    pub fn trim(&self, surface: usize) -> i32 {
        self.car.as_deref().map_or(100, |c| self.autos.trim(c, surface))
    }

    /// Sets the car's trim on `surface` and writes it to the file, this car's entry only.
    pub fn set_trim(&mut self, surface: usize, trim: i32) {
        let Some(car) = self.car.clone() else { return };
        self.autos.set_trim(&car, surface, trim);
        let autos = self.autos.clone();
        self.write(|file| {
            let mut now = AutoCars::parse(file);
            now.trims.remove(&car);
            if let Some(t) = autos.trims.get(&car) {
                now.trims.insert(car.clone(), *t);
            }
            now.replace_in(file)
        });
    }

    /// Sets the wheel's strength (0 to 1) and writes it to the file's `[ffb]`.
    pub fn set_strength(&mut self, strength: f32) {
        self.strength = strength.clamp(0.0, 1.0);
        let value = self.strength.to_string();
        self.write(|file| rbr_formats::ini::set_value(file, "ffb", "strength", &value));
    }

    /// The car's reference on `surface`: the stored one, else the one this drive learned.
    pub fn reference(&self, surface: usize) -> Option<Reference> {
        let stored = self.car.as_deref().and_then(|c| self.autos.reference(c, surface));
        stored.or_else(|| self.drive.as_ref().filter(|d| d.surface == surface).and_then(|d| d.learned.map(|l| l.0)))
    }

    /// The kept seconds while the car learns its reference on `surface` in automatic mode.
    pub fn learning(&self, surface: usize) -> Option<f32> {
        (self.auto && self.reference(surface).is_none())
            .then(|| self.drive.as_ref().filter(|d| d.surface == surface).map_or(0.0, |d| d.seconds))
    }

    /// The percentage the wheel shapes the force to on `surface` ([`reshape`] from [`Session::started`]): in
    /// automatic mode the car's automatic one with its trim (on its way there for [`auto::RAMP_SECONDS`] after
    /// learning it), else the manual one.
    pub fn wheel(&self, surface: usize) -> i32 {
        let manual = self.now()[surface.min(2)];
        let Some(reference) = self.reference(surface).filter(|_| self.auto) else { return manual };
        let to = auto::effective(reference.x, self.trim(surface));
        let learned_now = self.drive.as_ref().is_some_and(|d| d.surface == surface && d.learned.is_some());
        let stored = self.car.as_deref().is_some_and(|c| self.autos.reference(c, surface).is_some());
        match (learned_now && !stored, &self.drive) {
            (true, Some(drive)) => drive.ramp(manual, to),
            _ => to,
        }
    }

    /// One frame of the drive on `surface` (FFB1's), `force` NGP's: sampled when `kept` (a live NGP drive,
    /// racing, not paused, not a replay, faster than [`auto::MIN_SPEED`]); `dt` 0 while paused.
    pub fn frame(&mut self, surface: usize, force: f32, dt: f32, kept: bool) {
        let surface = surface.min(2);
        if self.drive.is_none() && !kept {
            return;
        }
        let learn = self.car.as_deref().is_none_or(|c| self.autos.reference(c, surface).is_none());
        let (started, shaped) = (self.started[surface], reshape(force, self.started[surface], self.wheel(surface)));
        let drive = self.drive.get_or_insert_with(|| Drive::new(surface));
        if drive.surface != surface {
            return;
        }
        let learning = drive.learned.is_none();
        drive.frame(force, shaped, started, dt, kept, learn);
        if let (true, Some((r, _))) = (learning, drive.learned) {
            let p = auto::effective(r.x, self.trim(surface));
            let car = self.car.as_deref().unwrap_or("?");
            eprintln!("viewer: force feedback of {car} on {} learned: {:.1}, {p} %", SURFACES[surface], r.x);
        }
    }

    /// The share of the last minute's kept frames that maxed the wheel out.
    pub fn clipped(&self) -> Option<f32> {
        self.drive.as_ref().and_then(Drive::clipped)
    }

    /// The drive's end (stage end, retire, restart, exit): with [`auto::KEEP_SECONDS`] kept, its reference blends
    /// into the car's and is written to the file, this car's entry only. The next frames start another drive.
    pub fn finish(&mut self) {
        let (Some(drive), Some(car)) = (self.drive.take(), self.car.clone()) else { return };
        let Some(new) = drive.reference().filter(|r| r.seconds >= auto::KEEP_SECONDS) else { return };
        let stored = self.autos.reference(&car, drive.surface);
        let blended = stored.map_or(new, |old| old.blend(new));
        self.autos.set_reference(&car, drive.surface, blended);
        eprintln!(
            "viewer: force feedback reference of {car} on {}: {:.1} over {:.0} s (this drive {:.1} over {:.0} s)",
            SURFACES[drive.surface], blended.x, blended.seconds, new.x, new.seconds
        );
        self.write(|file| {
            let mut now = AutoCars::parse(file);
            now.set_reference(&car, drive.surface, blended);
            now.replace_in(file)
        });
    }

    /// Writes `change` of the file as it is now.
    fn write(&self, change: impl FnOnce(&str) -> String) {
        let Some(path) = &self.path else { return };
        let file = std::fs::read_to_string(path).unwrap_or_default();
        if let Err(e) = std::fs::write(path, change(&file)) {
            eprintln!("viewer: {}: {e}", path.display());
        }
    }
}

/// The sensitivities a live command (`viewer --live`'s) starts NGP's car with: its `--ffb-sensitivity G,T,S`, else
/// 100 each.
pub fn started(command: &[String]) -> [i32; 3] {
    let given = command.iter().position(|a| a == "--ffb-sensitivity").and_then(|k| command.get(k + 1));
    let values = given.and_then(|v| v.split(',').map(|w| w.trim().parse().ok()).collect::<Option<Vec<i32>>>());
    values.and_then(|v| v.try_into().ok()).unwrap_or([100; 3])
}

/// The force NGP's curve gives at sensitivity percentage `now` for a force `force` it gave at `was`, both for the
/// drive's surface (`rbr_physics::ngp::ffb`): its `1 - 0.975^(|T| 100 / S)` with `S` scaled by `now / was`, which
/// is `1 - (1 - |force|)^(was / now)` with the sign kept. A percentage of 0 or less, or 100, leaves `S` as it is.
/// The pause menu's changes take effect on the running drive this way, NGP's car keeping the percentage it started
/// with (`rbr-standalone --ffb-sensitivity`).
pub fn reshape(force: f32, was: i32, now: i32) -> f32 {
    let percent = auto::percent;
    if percent(was) == percent(now) || force == 0.0 || force.is_nan() {
        return force;
    }
    let left = (1.0 - force.abs()).max(0.0);
    force.signum() * (1.0 - left.powf(percent(was) / percent(now)))
}

/// The wheel's force-feedback thread; dropping it stops the force.
pub struct Wheel {
    shared: Arc<Shared>,
    settings: Settings,
    thread: Option<JoinHandle<()>>,
    /// The device's name.
    pub name: String,
}

#[derive(Default)]
struct Shared {
    magnitude: AtomicI32,
    /// The steering input's bits, for NGP's dead zone.
    steering: AtomicU32,
    /// The drive is paused: no force, whatever the step records still set.
    paused: AtomicBool,
    stop: AtomicBool,
    /// The sensitivity NGP's car gives the force at and the one it takes now, percent ([`reshape`]; 0 for none).
    was: AtomicI32,
    now: AtomicI32,
    /// `Settings::strength`'s bits, which the pause menu changes while driving.
    strength: AtomicU32,
}

impl Wheel {
    /// Opens the device and starts its thread, or says why it cannot.
    pub fn open(options: Options) -> Result<Wheel, String> {
        let shared = Arc::new(Shared { strength: options.settings.strength.to_bits().into(), ..Shared::default() });
        let (tx, rx) = mpsc::channel();
        let thread = {
            let (shared, options) = (shared.clone(), options.clone());
            std::thread::Builder::new()
                .name("ffb".into())
                .spawn(move || run(&shared, &options, &tx))
                .map_err(|e| e.to_string())?
        };
        match rx.recv() {
            Ok(Ok(name)) => Ok(Wheel { shared, settings: options.settings, thread: Some(thread), name }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("the force-feedback thread ended".into()),
        }
    }

    /// The force for the latest step (`live::Hud::force`) and the steering input now (-1 to 1), at the sensitivity
    /// [`Wheel::set_sensitivity`] asked for.
    pub fn set(&self, force: f32, steering: f32) {
        let (was, now) = (self.shared.was.load(Ordering::Relaxed), self.shared.now.load(Ordering::Relaxed));
        let settings =
            Settings { strength: f32::from_bits(self.shared.strength.load(Ordering::Relaxed)), ..self.settings };
        self.set_magnitude(ffb::magnitude(reshape(force, was, now), &settings), steering);
    }

    /// The wheel's strength (0 to 1, `Settings::strength`) from the next step on.
    pub fn set_strength(&self, strength: f32) {
        self.shared.strength.store(strength.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// The force at sensitivity `now` rather than `was`, NGP's car's, on the drive's surface ([`reshape`]).
    pub fn set_sensitivity(&self, was: i32, now: i32) {
        self.shared.was.store(was, Ordering::Relaxed);
        self.shared.now.store(now, Ordering::Relaxed);
    }

    /// A magnitude (-10000 to 10000) as the game passes it to DirectInput, and the steering input now (public for
    /// `examples/ffb_play.rs`, which plays recorded magnitudes).
    pub fn set_magnitude(&self, magnitude: i32, steering: f32) {
        self.shared.magnitude.store(magnitude, Ordering::Relaxed);
        self.shared.steering.store(steering.to_bits(), Ordering::Relaxed);
    }

    /// Holds the force at 0 while `paused` (the pause menu, the stage result), also against the step records still on
    /// their way; the latest one's force comes back after.
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }
}

impl Drop for Wheel {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn sdl_error() -> String {
    // SAFETY: SDL_GetError returns a valid C string owned by SDL.
    unsafe { CStr::from_ptr(SDL_GetError()).to_string_lossy().into_owned() }
}

/// The evdev level Proton gives a DirectInput magnitude (-10000 to 10000): dinput's `scale_to_logical_value`
/// (`dlls/dinput/joystick_hid.c`) onto winebus's PID magnitude, logical -32768 to 32767 for physical -10000 to 10000
/// (`dlls/winebus.sys/hid.c`), passed on at the effect's 100 % gain (`bus_sdl.c`). 0 gives -1.
fn level(magnitude: i32) -> i16 {
    let m = magnitude.clamp(-MAX_MAGNITUDE, MAX_MAGNITUDE);
    (-32768 + (m + MAX_MAGNITUDE) * 65535 / (2 * MAX_MAGNITUDE)) as i16
}

/// The thread: opens the device, reports its name or the error on `ready`, then updates the effect until stopped.
fn run(shared: &Shared, options: &Options, ready: &mpsc::Sender<Result<String, String>>) {
    // SAFETY: the SDL calls below run on this thread only, on the haptic handle it opened and closes.
    unsafe {
        if !SDL_InitSubSystem(SDL_INIT_HAPTIC | SDL_INIT_JOYSTICK) {
            let _ = ready.send(Err(format!("SDL haptic: {}", sdl_error())));
            return;
        }
        let device = open_device(options.device.as_deref());
        let (haptic, name) = match device {
            Ok(found) => found,
            Err(e) => {
                let _ = ready.send(Err(e));
                SDL_QuitSubSystem(SDL_INIT_HAPTIC | SDL_INIT_JOYSTICK);
                return;
            }
        };
        let mut effect: SDL_HapticEffect = std::mem::zeroed();
        effect.constant.r#type = SDL_HAPTIC_CONSTANT;
        effect.constant.direction.r#type = SDL_HAPTIC_STEERING_AXIS;
        effect.constant.length = SDL_HAPTIC_INFINITY;
        let id = SDL_CreateHapticEffect(haptic, &effect);
        if id.0 < 0 || !SDL_RunHapticEffect(haptic, id, 1) {
            let _ = ready.send(Err(format!("{name}: the constant force: {}", sdl_error())));
            SDL_CloseHaptic(haptic);
            SDL_QuitSubSystem(SDL_INIT_HAPTIC | SDL_INIT_JOYSTICK);
            return;
        }
        let _ = ready.send(Ok(name));
        let period = Duration::from_secs_f64(1.0 / f64::from(options.rate.clamp(1, 1000)));
        while !shared.stop.load(Ordering::Relaxed) {
            let steering = f32::from_bits(shared.steering.load(Ordering::Relaxed));
            let m = if shared.paused.load(Ordering::Relaxed) {
                0
            } else {
                ffb::dead_zone(shared.magnitude.load(Ordering::Relaxed), steering, &options.settings)
            };
            effect.constant.level = level(m);
            SDL_UpdateHapticEffect(haptic, id, &effect);
            SDL_RunHapticEffect(haptic, id, 1);
            std::thread::sleep(period);
        }
        SDL_StopHapticEffect(haptic, id);
        SDL_DestroyHapticEffect(haptic, id);
        SDL_CloseHaptic(haptic);
        SDL_QuitSubSystem(SDL_INIT_HAPTIC | SDL_INIT_JOYSTICK);
    }
}

/// The first haptic device with a constant force whose name contains `part` (any, without one).
unsafe fn open_device(part: Option<&str>) -> Result<(*mut SDL_Haptic, String), String> {
    let mut count = 0;
    let ids = SDL_GetHaptics(&mut count);
    if ids.is_null() {
        return Err(format!("SDL haptic: {}", sdl_error()));
    }
    let mut seen = Vec::new();
    for k in 0..usize::try_from(count).unwrap_or(0) {
        let id = *ids.add(k);
        let raw = SDL_GetHapticNameForID(id);
        let name = if raw.is_null() { String::new() } else { CStr::from_ptr(raw).to_string_lossy().into_owned() };
        if part.is_some_and(|p| !name.to_lowercase().contains(&p.to_lowercase())) {
            seen.push(name);
            continue;
        }
        let haptic = SDL_OpenHaptic(id);
        if haptic.is_null() {
            seen.push(format!("{name} (cannot open: {})", sdl_error()));
            continue;
        }
        if SDL_GetHapticFeatures(haptic) & u32::from(SDL_HAPTIC_CONSTANT.0) != 0 {
            sdl3_sys::stdinc::SDL_free(ids.cast());
            return Ok((haptic, name));
        }
        SDL_CloseHaptic(haptic);
        seen.push(format!("{name} (no constant force)"));
    }
    sdl3_sys::stdinc::SDL_free(ids.cast());
    Err(if seen.is_empty() {
        "no force-feedback device".to_owned()
    } else {
        format!("no force-feedback device fits: {}", seen.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::auto::{self, Reference};
    use super::{launch, level, reshape, AutoCars, CarSensitivity, Options, Session};

    #[test]
    fn options_round_trip_and_default() {
        let mut o = Options {
            rate: 500,
            device: Some("G923".into()),
            sensitivity: [80, 100, 120],
            rotation: Some(540),
            rotation_per_car: true,
            auto: false,
            ..Options::default()
        };
        o.settings.strength = 0.6;
        o.settings.inverse = true;
        o.settings.dead_zone = 0.04;
        assert_eq!(Options::parse(&o.to_text()), o);
        assert_eq!(Options::parse("[controls]\nbrake = KeyB\n"), Options::default());
        assert!(Options::parse("[ffb]\nstrength = 1\n").auto, "automatic unless the file says otherwise");
        assert_eq!(Options::parse("[ffb]\nstrength = 3\nrate = 0\n").settings.strength, 1.0);
        assert_eq!(
            (o.rotation_for(Some(479)), o.rotation_for(Some(565)), o.rotation_for(None)),
            (Some(479), Some(540), Some(540))
        );
        o.rotation_per_car = false;
        assert_eq!(o.rotation_for(Some(479)), Some(540));
        o.rotation = None;
        assert_eq!(o.rotation_for(Some(479)), None);
    }

    #[test]
    fn each_car_keeps_its_own_sensitivity_in_place() {
        let file = "[ffb]\nstrength = 1\nsensitivity_car88 = 300,250,280\nsensitivity_car24 = 1,2\n\n\
                    [ffb_cars]\nrsf-24 = 90,100,110\nCOROLLA = 120, 120, 130\nrsf-9 = x,1,2\n\n[camera]\nview = bonnet\n";
        let cars = CarSensitivity::parse(file);
        let expected = [("COROLLA", [120, 120, 130]), ("rsf-24", [90, 100, 110]), ("rsf-88", [300, 250, 280])];
        assert_eq!(cars.cars, expected.map(|(k, v)| (k.to_owned(), v)).into());
        assert_eq!(
            (cars.of(Some("rsf-88"), [100; 3]), cars.of(Some("rsf-1"), [7; 3]), cars.of(None, [7; 3])),
            ([300, 250, 280], [7; 3], [7; 3])
        );
        // Written back as its own section: the older entries go, the rest of the file stays.
        let mut changed = cars.clone();
        changed.cars.remove("COROLLA");
        let text = changed.replace_in(file);
        assert_eq!(CarSensitivity::parse(&text), changed);
        assert!(text.starts_with("[ffb]\nstrength = 1\n\n") && text.contains("[camera]\nview = bonnet\n"), "{text}");
        assert!(!text.contains("sensitivity_car") && !text.contains("COROLLA"), "{text}");
        assert_eq!(Options::parse(&text).settings.strength, 1.0);
    }

    #[test]
    fn the_session_keeps_the_cars_own_in_the_file() {
        let dir = std::env::temp_dir().join(format!("rerbr-ffb-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("launcher-settings.ini");
        std::fs::write(&path, "[ffb]\nsensitivity_gravel = 280\n\n[camera]\nview = chase\n").unwrap();
        let options = Options { sensitivity: [280, 250, 280], ..Options::default() };
        let mut session = super::Session::load(Some(&path), Some("rsf-107".into()), &options, [280, 250, 280]);
        assert_eq!((session.now(), session.own()), ([280, 250, 280], false));
        // Another writer's car meanwhile is kept.
        let other = std::fs::read_to_string(&path).unwrap() + "\n[ffb_cars]\nrsf-67 = 300,300,300\n";
        std::fs::write(&path, other).unwrap();
        session.set(0, Some(350));
        assert_eq!((session.now(), session.own()), ([350, 250, 280], true));
        let text = std::fs::read_to_string(&path).unwrap();
        let cars = CarSensitivity::parse(&text).cars;
        assert_eq!((cars.get("rsf-107"), cars.get("rsf-67")), (Some(&[350, 250, 280]), Some(&[300; 3])));
        assert!(text.contains("[camera]\nview = chase\n"), "{text}");
        session.set(0, None);
        assert!(!CarSensitivity::parse(&std::fs::read_to_string(&path).unwrap()).cars.contains_key("rsf-107"));
        std::fs::remove_dir_all(&dir).unwrap();
        let live = ["rbr-standalone", "--ffb-sensitivity", "280,250,300", "--boot"].map(String::from);
        assert_eq!((super::started(&live), super::started(&live[..1])), ([280, 250, 300], [100; 3]));
    }

    #[test]
    fn a_new_sensitivity_reshapes_the_curve() {
        // NGP's curve at S = 100 and at 150: the force at 100 reshaped to 150 is the one at 150.
        let curve = |t: f32, s: f32| 1.0 - 0.975f32.powf(t * 100.0 / s);
        for t in [5.0, 27.4, 90.0] {
            assert!((reshape(curve(t, 100.0), 100, 150) - curve(t, 150.0)).abs() < 1e-5);
            assert!((reshape(-curve(t, 280.0), 280, 140) + curve(t, 140.0)).abs() < 1e-5);
        }
        assert_eq!((reshape(0.4, 100, 100), reshape(0.4, 0, 100), reshape(0.0, 100, 300)), (0.4, 0.4, 0.0));
        assert_eq!(reshape(1.0, 100, 300), 1.0);
    }

    /// NGP's curve at `S` = `s` scaled by `p` percent.
    fn curve(t: f32, s: f32, p: i32) -> f32 {
        1.0 - 0.975f32.powf(t * 100.0 / (s * p as f32 / 100.0))
    }

    #[test]
    fn the_ratio_inverts_ngps_curve() {
        for (t, s, p) in [(44.0, 75.0, 100), (183.0, 105.0, 280), (90.0, 130.0, 40), (5.0, 110.0, 477)] {
            let x = auto::torque_ratio(-curve(t, s, p), p).unwrap();
            assert!((x - t * 100.0 / s).abs() < 2e-3 * x.max(1.0), "{t} {s} {p}: {x}");
        }
        assert_eq!(auto::torque_ratio(0.0, 300), Some(0.0));
        assert_eq!(auto::torque_ratio(0.99995, 100), None);
        // 0 or less is 100, as NGP's sensitivity takes it.
        assert_eq!(auto::torque_ratio(0.4, 0), auto::torque_ratio(0.4, 100));
        assert_eq!(auto::percentile90(&[]), None);
        let samples: Vec<f32> = (0..=100).rev().map(|k| k as f32).collect();
        assert_eq!(auto::percentile90(&samples), Some(90.0));
    }

    #[test]
    fn the_automatic_percentage_puts_the_reference_at_half() {
        // At the automatic percentage the reference's force is 0.5, whatever the car.
        for x in [40.0, 122.0, 174.0] {
            let p = auto::percent_for(x);
            assert!((1.0 - 0.975f32.powf(x * 100.0 / p) - auto::TARGET).abs() < 1e-5);
        }
        assert!((auto::percent_for(1.0) - 3.6532).abs() < 1e-3);
        // The trim scales it; the result stays within 10 to 1000.
        assert_eq!((auto::effective(122.0, 100), auto::effective(122.0, 110)), (446, 490));
        assert_eq!((auto::effective(0.0, 100), auto::effective(400.0, 200)), (10, 1000));
    }

    #[test]
    fn a_drive_blends_into_the_reference_by_its_seconds() {
        let old = Reference { x: 100.0, seconds: 300.0 };
        assert_eq!(old.blend(Reference { x: 130.0, seconds: 600.0 }), Reference { x: 120.0, seconds: 600.0 });
        assert_eq!(old.blend(Reference { x: 130.0, seconds: 100.0 }), Reference { x: 107.5, seconds: 400.0 });
        assert_eq!(Reference::default().blend(Reference::default()), Reference::default());
    }

    #[test]
    fn the_cars_states_round_trip_and_set_the_start() {
        let file = "[ffb]\nsensitivity_tarmac = 250\n\n[ffb_auto]\nrsf-81 = 145.2/320, 130.5/600, 0/0\n\
                    rsf-9 = 0/0,0/0,0/0\nbad = 1/2\n\n[ffb_trim]\nrsf-81 = 100,110,100\nrsf-5 = 90,x,1\n\n\
                    [camera]\nview = chase\n";
        let autos = AutoCars::parse(file);
        assert_eq!(autos.references.keys().collect::<Vec<_>>(), ["rsf-81"]);
        assert_eq!(autos.reference("rsf-81", 0), Some(Reference { x: 145.2, seconds: 320.0 }));
        assert_eq!((autos.reference("rsf-81", 2), autos.trim("rsf-81", 1), autos.trim("rsf-5", 0)), (None, 110, 100));
        let text = autos.replace_in(file);
        assert_eq!(AutoCars::parse(&text), autos);
        assert!(text.contains("[ffb_auto]\nrsf-81 = 145.2/320, 130.5/600, 0/0\n"), "{text}");
        assert!(text.contains("[ffb_trim]\nrsf-81 = 100,110,100\n") && text.contains("[camera]\nview = chase\n"));
        // A trim back at 100 everywhere drops its line.
        let mut trimmed = autos.clone();
        trimmed.set_trim("rsf-81", 1, 100);
        assert!(trimmed.trims.is_empty());
        // The start: automatic where the car has a reference, else its own, else every car's; manual as before.
        let options = Options { sensitivity: [280, 250, 280], ..Options::default() };
        let cars = CarSensitivity::parse("[ffb_cars]\nrsf-81 = 300,300,372\n");
        let effective = [auto::effective(145.2, 100), auto::effective(130.5, 110)];
        assert_eq!(launch(&options, &cars, &autos, Some("rsf-81")), [effective[0], effective[1], 372]);
        assert_eq!(launch(&options, &cars, &autos, Some("rsf-1")), [280, 250, 280]);
        let manual = Options { auto: false, ..options };
        assert_eq!(launch(&manual, &cars, &autos, Some("rsf-81")), [300, 300, 372]);
        assert_eq!(launch(&manual, &cars, &autos, None), [280, 250, 280]);
    }

    /// `seconds` of frames of 1/8 s (exact in binary) at force `force`.
    fn drive(session: &mut Session, surface: usize, force: f32, seconds: f32, kept: bool) {
        for _ in 0..(seconds * 8.0).round() as usize {
            session.frame(surface, force, 0.125, kept);
        }
    }

    #[test]
    fn a_car_learns_its_reference_once_and_moves_the_wheel_there() {
        let options = Options { sensitivity: [200; 3], ..Options::default() };
        let mut session = Session::load(None, Some("rsf-81".into()), &options, [200; 3]);
        let force = curve(120.0, 100.0, 200);
        drive(&mut session, 1, force, 5.0, false);
        assert_eq!((session.learning(1), session.wheel(1)), (Some(0.0), 200), "nothing kept yet");
        drive(&mut session, 1, force, 59.0, true);
        assert_eq!(session.wheel(1), 200);
        assert!((session.learning(1).unwrap() - 59.0).abs() < 1e-3);
        drive(&mut session, 1, force, 1.0, true);
        let x = session.reference(1).unwrap().x;
        assert!((x - 120.0).abs() < 0.05, "{x}");
        let to = auto::effective(x, 100);
        assert_eq!((session.learning(1), session.wheel(1)), (None, 200), "at the ramp's start");
        drive(&mut session, 1, force, 1.5, false);
        assert_eq!(session.wheel(1), (200 + to) / 2, "half way, kept or not");
        drive(&mut session, 1, force, 1.5, true);
        assert_eq!(session.wheel(1), to);
        // Another surface goes on manual; manual mode ignores the reference.
        assert_eq!(session.wheel(0), 200);
        session.auto = false;
        assert_eq!((session.wheel(1), session.learning(1)), (200, None));
    }

    #[test]
    fn a_car_with_a_reference_keeps_it_through_the_drive() {
        let options = Options { sensitivity: [200; 3], ..Options::default() };
        let mut session = Session::load(None, Some("rsf-81".into()), &options, [446; 3]);
        session.autos.set_reference("rsf-81", 0, Reference { x: 122.0, seconds: 600.0 });
        assert_eq!(session.wheel(0), 446);
        drive(&mut session, 0, curve(200.0, 100.0, 446), 120.0, true);
        assert_eq!((session.wheel(0), session.learning(0)), (446, None));
        session.set_trim(0, 110);
        assert_eq!(session.wheel(0), 490);
    }

    #[test]
    fn the_clipping_share_is_the_last_minutes() {
        let mut session = Session::load(None, Some("c".into()), &Options::default(), [100; 3]);
        assert_eq!(session.clipped(), None);
        drive(&mut session, 0, 0.97, 30.0, true);
        drive(&mut session, 0, 0.5, 30.0, true);
        assert!((session.clipped().unwrap() - 0.5).abs() < 0.02);
        drive(&mut session, 0, 0.5, 30.0, true);
        assert!(session.clipped().unwrap() < 0.02, "{:?}", session.clipped());
        // After the reshape: 0.97 at 100 % is 0.83 at 200 %.
        let mut session =
            Session::load(None, Some("c".into()), &Options { auto: false, ..Options::default() }, [100; 3]);
        session.cars.cars.insert("c".into(), [200; 3]);
        drive(&mut session, 0, 0.97, 10.0, true);
        assert_eq!(session.clipped(), Some(0.0));
    }

    #[test]
    fn the_drives_end_writes_the_cars_reference_in_place() {
        let dir = std::env::temp_dir().join(format!("rerbr-ffb-auto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("launcher-settings.ini");
        std::fs::write(&path, "[ffb]\nstrength = 1\n\n[ffb_auto]\nrsf-81 = 0/0, 100/300, 0/0\n").unwrap();
        let options = Options::parse(&std::fs::read_to_string(&path).unwrap());
        let mut session = Session::load(Some(&path), Some("rsf-81".into()), &options, [100; 3]);
        // Another writer meanwhile: kept.
        let other = std::fs::read_to_string(&path).unwrap() + "rsf-7 = 50/60, 0/0, 0/0\n\n[camera]\nview = chase\n";
        std::fs::write(&path, other).unwrap();
        // Under 20 s kept: nothing written.
        drive(&mut session, 1, curve(130.0, 100.0, 100), 19.0, true);
        session.finish();
        assert_eq!(AutoCars::parse(&std::fs::read_to_string(&path).unwrap()).reference("rsf-81", 1).unwrap().x, 100.0);
        drive(&mut session, 1, curve(130.0, 100.0, 100), 150.0, true);
        session.finish();
        let text = std::fs::read_to_string(&path).unwrap();
        let autos = AutoCars::parse(&text);
        let r = autos.reference("rsf-81", 1).unwrap();
        assert!((r.x - 110.0).abs() < 0.1 && r.seconds == 450.0, "{r:?}");
        assert_eq!(autos.reference("rsf-7", 0), Some(Reference { x: 50.0, seconds: 60.0 }));
        assert!(text.contains("[camera]\nview = chase\n"), "{text}");
        // The strength and the trim go into the file as it is.
        session.set_strength(0.35);
        session.set_trim(1, 90);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(Options::parse(&text).settings.strength, 0.35);
        assert_eq!(AutoCars::parse(&text).trim("rsf-81", 1), 90);
        assert!(text.contains("[camera]\nview = chase\n") && text.contains("rsf-7 = 50.0/60"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn levels_are_protons() {
        // Measured at the G923's evdev device for these magnitudes (harness/ffb_trace.sh probe).
        assert_eq!(level(-263), -863);
        assert_eq!(level(0), -1);
        assert_eq!(level(10_000), 32767);
        assert_eq!(level(-10_000), -32768);
        assert_eq!(level(5_000), 16383);
        assert_eq!(level(20_000), 32767);
    }
}
