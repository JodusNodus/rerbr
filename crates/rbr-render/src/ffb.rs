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

/// The sensitivities' range, percent, in the launcher and the pause menu (RSF's cars go up to 372).
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

/// The pause menu's force-feedback choices for one drive (`crate::hud::ffb_page`): the settings file the car's own
/// sensitivities live in (`viewer --bindings`), the car's key, every car's and the ones NGP's car started with.
#[derive(Debug, Clone, Default)]
pub struct Session {
    path: Option<PathBuf>,
    car: Option<String>,
    pub cars: CarSensitivity,
    /// [`Options::sensitivity`].
    pub global: [i32; 3],
    /// What NGP's car started with ([`started`]).
    pub started: [i32; 3],
}

impl Session {
    /// The choices in `path` (none without it or when it does not read) for car `car`
    /// (`crate::camera_prefs::car_key`).
    pub fn load(path: Option<&Path>, car: Option<String>, global: [i32; 3], started: [i32; 3]) -> Session {
        let text = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        Session { path: path.map(Path::to_owned), car, cars: CarSensitivity::parse(&text), global, started }
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
        let Some(path) = &self.path else { return };
        let file = std::fs::read_to_string(path).unwrap_or_default();
        let mut cars = CarSensitivity::parse(&file);
        change(&mut cars, now);
        if let Err(e) = std::fs::write(path, cars.replace_in(&file)) {
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
    let percent = |p: i32| if p > 0 { p as f32 } else { 100.0 };
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
}

impl Wheel {
    /// Opens the device and starts its thread, or says why it cannot.
    pub fn open(options: Options) -> Result<Wheel, String> {
        let shared = Arc::new(Shared::default());
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
        self.set_magnitude(ffb::magnitude(reshape(force, was, now), &self.settings), steering);
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
    use super::{level, reshape, CarSensitivity, Options};

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
        let mut session = super::Session::load(Some(&path), Some("rsf-107".into()), [280, 250, 280], [280, 250, 280]);
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
