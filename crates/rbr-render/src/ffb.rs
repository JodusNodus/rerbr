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
             sensitivity_snow = {}\nrotation = {}\nrotation_per_car = {}\n",
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

/// The steering load the player drives each car at (ours, `notes/ngp.md` "Force feedback"), tracked to show, never to
/// change a setting. NGP's force is `1 - 0.975^(|T| 100 / S)` with `S` the car's `ForceFeedbackSensitivity_NGP`
/// times the percentage / 100; [`load::torque_ratio`] reads `x = |T| 100 / S_car` back from the force, so a car's
/// histogram of `x` ([`load::Load`]) gives the force at any percentage: how often the wheel maxes out, and the
/// percentage that puts the player's usual cornering at half force ([`load::suggested`]).
pub mod load {
    /// NGP's curve's base.
    const BASE: f32 = 0.975;
    /// The force the suggestion puts the player's usual cornering (the 90th percentile) at.
    pub const TARGET: f32 = 0.5;
    /// A force past this is the wheel maxed out.
    pub const MAXED: f32 = 0.95;
    /// The histogram's bin width in `x`, and its bins: `x` 0 to 400, then one for more.
    pub const BIN: f32 = 5.0;
    pub const BINS: usize = 81;
    /// The fewest seconds of kept driving a drive's end adds to the car's histogram.
    pub const KEEP_SECONDS: f32 = 20.0;
    /// The most seconds a stored histogram holds: past it, it is scaled down, so recent driving weighs more.
    pub const MAX_SECONDS: f32 = 600.0;
    /// The slowest kept speed, m/s.
    pub const MIN_SPEED: f32 = 10.0;

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

    /// NGP's force for the ratio `x` at percentage `p`.
    pub fn force(x: f32, p: i32) -> f32 {
        1.0 - BASE.powf(x * 100.0 / percent(p))
    }

    /// The ratio at which the force reaches `f` at percentage `p`.
    pub fn ratio_at(f: f32, p: i32) -> f32 {
        percent(p) / 100.0 * (-f).ln_1p() / BASE.ln()
    }

    /// The percentage that puts the ratio `x` at [`TARGET`]: `100 x ln(0.975) / ln(1 - TARGET)` (≈ 3.653 x).
    pub fn percent_for(x: f32) -> f32 {
        100.0 * x * BASE.ln() / (1.0 - TARGET).ln()
    }

    /// A histogram of `x`: seconds × 10 per [`BIN`].
    #[derive(Debug, Clone, PartialEq)]
    pub struct Load {
        pub tenths: [f32; BINS],
    }

    impl Default for Load {
        fn default() -> Self {
            Load { tenths: [0.0; BINS] }
        }
    }

    impl Load {
        /// `dt` seconds at ratio `x`.
        pub fn add(&mut self, x: f32, dt: f32) {
            let bin = ((x / BIN) as usize).min(BINS - 1);
            self.tenths[bin] += dt * 10.0;
        }

        pub fn seconds(&self) -> f32 {
            self.tenths.iter().sum::<f32>() / 10.0
        }

        /// This and `other` together.
        pub fn plus(&self, other: &Load) -> Load {
            Load { tenths: std::array::from_fn(|k| self.tenths[k] + other.tenths[k]) }
        }

        /// This and a drive's `new` together, scaled down to [`MAX_SECONDS`] past it, in whole tenths.
        pub fn merge(&self, new: &Load) -> Load {
            let sum = self.plus(new);
            let scale = (MAX_SECONDS / sum.seconds()).min(1.0);
            Load { tenths: sum.tenths.map(|t| (t * scale).round()) }
        }

        /// The bin's middle ratio (the last bin: its start).
        pub fn middle(bin: usize) -> f32 {
            if bin + 1 == BINS {
                bin as f32 * BIN
            } else {
                (bin as f32 + 0.5) * BIN
            }
        }

        /// The share of the time the force would be past [`MAXED`] at percentage `p`.
        pub fn maxed_out(&self, p: i32) -> Option<f32> {
            let total: f32 = self.tenths.iter().sum();
            let maxed: f32 = self
                .tenths
                .iter()
                .enumerate()
                .filter(|&(k, _)| force(Load::middle(k), p) > MAXED)
                .map(|(_, t)| t)
                .sum();
            (total > 0.0).then(|| maxed / total)
        }

        /// The 90th percentile of the ratio (the bin's middle).
        pub fn percentile90(&self) -> Option<f32> {
            let total: f32 = self.tenths.iter().sum();
            let mut seen = 0.0;
            for (k, t) in self.tenths.iter().enumerate() {
                seen += t;
                if total > 0.0 && seen >= 0.9 * total {
                    return Some(Load::middle(k));
                }
            }
            None
        }

        /// The text of a `[ffb_load]` entry: the tenths per bin, the trailing empty ones left out.
        pub fn to_text(&self) -> String {
            let n = self.tenths.iter().rposition(|t| t.round() > 0.0).map_or(0, |k| k + 1);
            self.tenths[..n].iter().map(|t| format!("{:.0}", t.round())).collect::<Vec<_>>().join(",")
        }

        pub fn parse(text: &str) -> Option<Load> {
            let mut load = Load::default();
            for (k, v) in text.split(',').filter(|v| !v.trim().is_empty()).enumerate() {
                let t: f32 = v.trim().parse().ok()?;
                (k < BINS && t.is_finite() && t >= 0.0).then_some(())?;
                load.tenths[k] = t;
            }
            Some(load)
        }
    }

    /// The fewest seconds of a car's load the page gives advice from.
    pub const ADVICE_SECONDS: f32 = 20.0;
    /// Below this share of the suggested percentage the wheel maxes out too often; above [`HIGH`] the player's
    /// cornering uses little of its range.
    pub const LOW: f32 = 0.85;
    pub const HIGH: f32 = 1.15;

    /// What the player could do at percentage `current` with the load `load` (one criterion: `current` against
    /// [`suggested`]).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Advice {
        /// Too little driving yet: the seconds so far.
        DriveMore(f32),
        /// The wheel maxes out often (the share): raise to the suggestion.
        Raise(f32, i32),
        /// The cornering uses little of the range: lower to the suggestion.
        Lower(i32),
        /// Mid-range (the maxed-out share).
        Good(f32),
    }

    pub fn advice(load: &Load, current: i32, range: std::ops::RangeInclusive<i32>) -> Advice {
        let (seconds, maxed) = (load.seconds(), load.maxed_out(current).unwrap_or(0.0));
        let Some(to) = suggested(load, range).filter(|_| seconds >= ADVICE_SECONDS) else {
            return Advice::DriveMore(seconds);
        };
        let ratio = percent(current) / to as f32;
        if ratio < LOW {
            Advice::Raise(maxed, to)
        } else if ratio > HIGH {
            Advice::Lower(to)
        } else {
            Advice::Good(maxed)
        }
    }

    /// The percentage that puts the player's usual cornering in `load` (its 90th percentile) at half force, to the
    /// nearest 5 within `range`: a hint, never applied.
    pub fn suggested(load: &Load, range: std::ops::RangeInclusive<i32>) -> Option<i32> {
        let p = percent_for(load.percentile90()?);
        Some(((p / 5.0).round() as i32 * 5).clamp(*range.start(), *range.end()))
    }
}

use load::Load;

/// Each car's steering load per surface ([`load`]): the `[ffb_load]` section, keyed `<car key> <surface>`, the
/// tenths of a second per bin. The viewer writes a car's entry at a drive's end; the launcher's save carries the
/// file's over.
///
/// ```ini
/// [ffb_load]
/// rsf-81 tarmac = 0,12,340,1010,96
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loads {
    pub cars: BTreeMap<(String, usize), Load>,
}

/// [`Loads`]'s section.
pub const LOAD_SECTION: &str = "ffb_load";

impl Loads {
    /// The loads in `text` (a whole settings file); a malformed line is left out.
    pub fn parse(text: &str) -> Loads {
        let sections = rbr_formats::ini::parse(text).unwrap_or_default();
        let entry = |key: &str, value: &str| {
            let (car, surface) = key.rsplit_once(' ')?;
            let surface = SURFACES.iter().position(|s| *s == surface.trim())?;
            Some(((car.trim().to_owned(), surface), Load::parse(value)?))
        };
        let cars = sections
            .iter()
            .filter(|s| s.name == LOAD_SECTION)
            .flat_map(|s| &s.entries)
            .filter_map(|(k, v)| entry(k, v))
            .collect();
        Loads { cars }
    }

    /// Car `key`'s load on `surface`.
    pub fn of(&self, key: &str, surface: usize) -> Option<&Load> {
        self.cars.get(&(key.to_owned(), surface.min(2)))
    }

    /// The loads saved under a car's earlier key `old` go to `now`; one already there stays.
    pub fn rename(&mut self, old: &str, now: &str) {
        for surface in 0..3 {
            if let Some(load) = self.cars.remove(&(old.to_owned(), surface)) {
                self.cars.entry((now.to_owned(), surface)).or_insert(load);
            }
        }
    }

    /// The section's text.
    pub fn to_text(&self) -> String {
        let line =
            |((car, s), load): (&(String, usize), &Load)| format!("{car} {} = {}\n", SURFACES[*s], load.to_text());
        format!("[{LOAD_SECTION}]\n{}", self.cars.iter().map(line).collect::<String>())
    }

    /// `file` (a whole settings file) with this section in place of its own.
    pub fn replace_in(&self, file: &str) -> String {
        rbr_formats::ini::replace_section(file, LOAD_SECTION, &self.to_text())
    }
}

/// The pause menu's force-feedback choices for one drive (`crate::hud::ffb_page`), and the steering load it tracks:
/// the settings file the car's own choices live in (`viewer --bindings`), the car's key, every car's and the ones
/// NGP's car started with, the strength, the stored loads and this drive's ([`load`]).
#[derive(Debug, Clone, Default)]
pub struct Session {
    path: Option<PathBuf>,
    car: Option<String>,
    pub cars: CarSensitivity,
    pub loads: Loads,
    /// [`Options::sensitivity`].
    pub global: [i32; 3],
    /// What NGP's car started with ([`started`]).
    pub started: [i32; 3],
    /// The wheel's strength, 0 to 1 (`Settings::strength`).
    pub strength: f32,
    /// This drive's surface and load, from its first kept frame.
    drive: Option<(usize, Load)>,
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
            loads: Loads::parse(&text),
            global: options.sensitivity,
            started,
            strength: options.settings.strength,
            drive: None,
        }
    }

    /// The car's sensitivities now.
    pub fn now(&self) -> [i32; 3] {
        self.cars.of(self.car.as_deref(), self.global)
    }

    /// Whether the car has its own.
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

    /// Sets the wheel's strength (0 to 1) and writes it to the file's `[ffb]`.
    pub fn set_strength(&mut self, strength: f32) {
        self.strength = strength.clamp(0.0, 1.0);
        let value = self.strength.to_string();
        self.write(|file| rbr_formats::ini::set_value(file, "ffb", "strength", &value));
    }

    /// One frame of the drive on `surface` (FFB1's), `force` NGP's: sampled when `kept` (a live NGP drive, racing,
    /// not paused, not a replay, faster than [`load::MIN_SPEED`]).
    pub fn frame(&mut self, surface: usize, force: f32, dt: f32, kept: bool) {
        let surface = surface.min(2);
        let Some(x) = load::torque_ratio(force, self.started[surface]).filter(|_| kept) else { return };
        let (on, drive) = self.drive.get_or_insert_with(|| (surface, Load::default()));
        if *on == surface {
            drive.add(x, dt);
        }
    }

    /// The car's load on `surface`: the stored one with this drive's.
    pub fn load_on(&self, surface: usize) -> Load {
        let stored = self.car.as_deref().and_then(|c| self.loads.of(c, surface)).cloned().unwrap_or_default();
        match &self.drive {
            Some((on, drive)) if *on == surface => stored.plus(drive),
            _ => stored,
        }
    }

    /// The drive's end (stage end, retire, restart, exit): with [`load::KEEP_SECONDS`] kept, its load merges into the
    /// car's and is written to the file, this car's entry only. The next kept frames start another drive.
    pub fn finish(&mut self) {
        let (Some((surface, drive)), Some(car)) = (self.drive.take(), self.car.clone()) else { return };
        if drive.seconds() < load::KEEP_SECONDS {
            return;
        }
        let key = (car.clone(), surface);
        let merged = self.loads.cars.get(&key).cloned().unwrap_or_default().merge(&drive);
        eprintln!(
            "viewer: steering load of {car} on {}: {:.0} s this drive, {:.0} s kept",
            SURFACES[surface],
            drive.seconds(),
            merged.seconds()
        );
        self.loads.cars.insert(key.clone(), merged.clone());
        self.write(|file| {
            let mut now = Loads::parse(file);
            now.cars.insert(key, merged);
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
    let percent = load::percent;
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
    use super::load::{self, Load};
    use super::{level, reshape, CarSensitivity, Loads, Options, Session, SENSITIVITY_RANGE};

    #[test]
    fn options_round_trip_and_default() {
        let mut o = Options {
            rate: 500,
            device: Some("G923".into()),
            sensitivity: [80, 100, 120],
            rotation: Some(540),
            rotation_per_car: true,
            ..Options::default()
        };
        o.settings.strength = 0.6;
        o.settings.inverse = true;
        o.settings.dead_zone = 0.04;
        assert_eq!(Options::parse(&o.to_text()), o);
        assert_eq!(Options::parse("[controls]\nbrake = KeyB\n"), Options::default());
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
            let x = load::torque_ratio(-curve(t, s, p), p).unwrap();
            assert!((x - t * 100.0 / s).abs() < 2e-3 * x.max(1.0), "{t} {s} {p}: {x}");
            assert!((load::force(x, p) - curve(t, s, p)).abs() < 1e-5);
        }
        assert_eq!(load::torque_ratio(0.0, 300), Some(0.0));
        assert_eq!(load::torque_ratio(0.99995, 100), None);
        // 0 or less is 100, as NGP's sensitivity takes it.
        assert_eq!(load::torque_ratio(0.4, 0), load::torque_ratio(0.4, 100));
        assert!((load::force(load::ratio_at(0.95, 250), 250) - 0.95).abs() < 1e-5);
    }

    #[test]
    fn the_histogram_bins_merges_scales_and_round_trips() {
        let mut l = Load::default();
        l.add(0.0, 1.0);
        l.add(4.99, 1.0);
        l.add(122.0, 2.5);
        l.add(9000.0, 0.5);
        assert_eq!((l.tenths[0], l.tenths[24], l.tenths[load::BINS - 1]), (20.0, 25.0, 5.0));
        assert_eq!(l.seconds(), 5.0);
        assert_eq!(Load::parse(&l.to_text()), Some(l.clone()));
        assert_eq!(
            Load::parse("20,0,1"),
            Some(Load { tenths: std::array::from_fn(|k| [20.0, 0.0, 1.0].get(k).copied().unwrap_or(0.0)) })
        );
        assert_eq!((Load::parse("1,x"), Load::parse(&vec!["1"; load::BINS + 1].join(","))), (None, None));
        // Merged: added up; past 600 s scaled down to 600.
        let mut a = Load::default();
        a.add(50.0, 500.0);
        let mut b = Load::default();
        b.add(150.0, 300.0);
        let m = a.merge(&b);
        assert_eq!((m.tenths[10], m.tenths[30], m.seconds()), (3750.0, 2250.0, 600.0));
        assert_eq!(Load::default().merge(&b).seconds(), 300.0);
    }

    #[test]
    fn the_maxed_out_share_and_the_suggestion_follow_the_sensitivity() {
        let mut l = Load::default();
        l.add(40.0, 80.0);
        l.add(140.0, 20.0);
        // x 142.5 maxes out (> 0.95) at 100 % (0.97), not at 200 % (0.83); x 42.5 at neither.
        assert_eq!((l.maxed_out(100), l.maxed_out(200)), (Some(0.2), Some(0.0)));
        assert_eq!(Load::default().maxed_out(100), None);
        // The 90th percentile is the second bin's middle: the suggestion puts it at half force, to 5 %.
        assert_eq!(l.percentile90(), Some(142.5));
        let p = load::suggested(&l, SENSITIVITY_RANGE).unwrap();
        assert_eq!(p, 520.min(*SENSITIVITY_RANGE.end()));
        assert!((load::force(142.5, (load::percent_for(142.5)) as i32) - 0.5).abs() < 2e-3);
        assert!((load::percent_for(1.0) - 3.6532).abs() < 1e-3);
        let mut small = Load::default();
        small.add(22.0, 10.0);
        assert_eq!(load::suggested(&small, SENSITIVITY_RANGE), Some(80));
        assert_eq!(load::suggested(&Load::default(), SENSITIVITY_RANGE), None);
    }

    #[test]
    fn the_advice_compares_the_sensitivity_with_the_suggestion() {
        use load::Advice;
        let mut l = Load::default();
        l.add(40.0, 15.0);
        assert_eq!(load::advice(&l, 100, SENSITIVITY_RANGE), Advice::DriveMore(15.0));
        l.add(40.0, 15.0);
        // The p90 is x 42.5: suggested 155 %; 0.85 and 1.15 of it are 131.75 and 178.25.
        assert_eq!(load::suggested(&l, SENSITIVITY_RANGE), Some(155));
        assert!(matches!(load::advice(&l, 130, SENSITIVITY_RANGE), Advice::Raise(_, 155)));
        assert!(matches!(load::advice(&l, 135, SENSITIVITY_RANGE), Advice::Good(_)));
        assert!(matches!(load::advice(&l, 175, SENSITIVITY_RANGE), Advice::Good(_)));
        assert_eq!(load::advice(&l, 180, SENSITIVITY_RANGE), Advice::Lower(155));
        // The share maxed out is the evidence: x 42.5 maxes out at 30 %.
        assert_eq!(load::advice(&l, 30, SENSITIVITY_RANGE), Advice::Raise(1.0, 155));
    }

    #[test]
    fn the_loads_round_trip_and_move_with_the_car() {
        let file = "[ffb]\nstrength = 1\n\n[ffb_load]\nrsf-81 tarmac = 0,12,340\npack-a b gravel = 5\n\
                    rsf-9 mud = 1\nrsf-9 = 1\nrsf-7 snow = 1,x\n\n[camera]\nview = chase\n";
        let loads = Loads::parse(file);
        assert_eq!(
            loads.cars.keys().cloned().collect::<Vec<_>>(),
            [("pack-a b".to_owned(), 0), ("rsf-81".to_owned(), 1)]
        );
        assert_eq!(loads.of("rsf-81", 1).unwrap().tenths[2], 340.0);
        let text = loads.replace_in(file);
        assert_eq!(Loads::parse(&text), loads);
        assert!(text.contains("[ffb_load]\npack-a b gravel = 5\nrsf-81 tarmac = 0,12,340\n"), "{text}");
        assert!(text.contains("[camera]\nview = chase\n"), "{text}");
        let mut moved = loads.clone();
        moved.rename("rsf-81", "pack-x");
        assert!(moved.of("pack-x", 1).is_some() && moved.of("rsf-81", 1).is_none());
    }

    /// `seconds` of frames of 1/8 s (exact in binary) at force `force`.
    fn drive(session: &mut Session, surface: usize, force: f32, seconds: f32, kept: bool) {
        for _ in 0..(seconds * 8.0).round() as usize {
            session.frame(surface, force, 0.125, kept);
        }
    }

    #[test]
    fn a_drive_tracks_its_load_and_changes_no_setting() {
        let options = Options { sensitivity: [200; 3], ..Options::default() };
        let mut session = Session::load(None, Some("rsf-81".into()), &options, [200; 3]);
        let force = curve(121.0, 100.0, 200);
        drive(&mut session, 1, force, 5.0, false);
        assert_eq!(session.load_on(1).seconds(), 0.0, "nothing kept");
        drive(&mut session, 1, force, 90.0, true);
        let load = session.load_on(1);
        assert_eq!((load.seconds(), load.percentile90()), (90.0, Some(122.5)));
        assert_eq!((session.now(), session.load_on(0).seconds()), ([200; 3], 0.0));
    }

    #[test]
    fn the_drives_end_writes_the_cars_load_in_place() {
        let dir = std::env::temp_dir().join(format!("rerbr-ffb-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("launcher-settings.ini");
        std::fs::write(&path, "[ffb]\nstrength = 1\n\n[ffb_load]\nrsf-81 tarmac = 0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,3000\n").unwrap();
        let options = Options::parse(&std::fs::read_to_string(&path).unwrap());
        let mut session = Session::load(Some(&path), Some("rsf-81".into()), &options, [100; 3]);
        // Another writer meanwhile: kept.
        let other = std::fs::read_to_string(&path).unwrap() + "rsf-7 snow = 10\n\n[camera]\nview = chase\n";
        std::fs::write(&path, other).unwrap();
        // Under 20 s kept: nothing written.
        drive(&mut session, 1, curve(130.0, 100.0, 100), 19.0, true);
        session.finish();
        assert_eq!(Loads::parse(&std::fs::read_to_string(&path).unwrap()).of("rsf-81", 1).unwrap().seconds(), 300.0);
        drive(&mut session, 1, curve(130.0, 100.0, 100), 450.0, true);
        session.finish();
        let text = std::fs::read_to_string(&path).unwrap();
        let loads = Loads::parse(&text);
        let l = loads.of("rsf-81", 1).unwrap();
        // 300 s at bin 25 and 450 s at bin 26, scaled to 600 s.
        assert_eq!((l.tenths[25], l.tenths[26], l.seconds()), (2400.0, 3600.0, 600.0));
        assert_eq!(loads.of("rsf-7", 2).unwrap().seconds(), 1.0);
        assert!(text.contains("[camera]\nview = chase\n"), "{text}");
        // The strength goes into the file as it is.
        session.set_strength(0.35);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(Options::parse(&text).settings.strength, 0.35);
        assert!(text.contains("[camera]\nview = chase\n") && text.contains("rsf-7 snow = 10"), "{text}");
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
