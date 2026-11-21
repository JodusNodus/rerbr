//! The steering wheel's rev lights (ours; `[ffb] rev_lights`): the wheel's LEDs fill over the last
//! [`SPAN`] rpm before the car's upshift point for the gear (SHF1, as the working dials' shift lights; the rev limit in
//! a gear without one) and flash from it on. Presentation only: read from the frame's HUD state, never written back.
//!
//! - Linux: the LED class the wheel drivers give the wheel's HID device (`leds/<device>::RPM1` to `RPM5`: hid-lg4ff,
//!   new-lg4ff), written like the rotation ([`crate::wheel_rotation`]); writing needs the wheel tools' udev rule.
//! - Elsewhere: Logitech's classic LED command (`f8 12 <mask>`, bit 0 the first LED) as an HID output report through
//!   SDL's HID API, for the wheels that take it ([`CLASSIC`]); as crossover-ffb does on macOS. Not tried on a wheel.
//!
//! A thread writes the lights on a change, at most every [`PERIOD`], and leaves them dark when it stops.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rbr_recording::shift::ShiftPoints;

/// The LEDs a wheel has.
pub const LEDS: u32 = 5;
/// The rpm before the shift point over which the LEDs fill (the dials' shift lights come on about 1500 rpm before it).
pub const SPAN: f32 = 1500.0;
/// The flash's rate past the shift point, Hz (on and off once each per period).
const FLASH_HZ: f32 = 4.0;
/// The thread's period: each write is a USB report.
const PERIOD: Duration = Duration::from_millis(30);

/// The LEDs lit (bit 0 the first) at `rpm` in `gear` (0 reverse, 1 neutral, 2 first gear, as SHF1), `time` s on a
/// running clock for the flash; dark without a shift point or rev limit.
pub fn mask(rpm: f32, gear: i32, shift: &ShiftPoints, time: f32) -> u8 {
    let up = usize::try_from(gear).ok().and_then(|g| shift.up.get(g)).copied().filter(|u| *u > 0.0);
    let Some(point) = up.or(Some(shift.limit).filter(|l| *l > 0.0)) else { return 0 };
    if rpm >= point {
        return if ((time * FLASH_HZ * 2.0) as u32).is_multiple_of(2) { all() } else { 0 };
    }
    let lit = ((rpm - (point - SPAN)) / SPAN * LEDS as f32).ceil().clamp(0.0, LEDS as f32) as u32;
    ((1u32 << lit) - 1) as u8
}

fn all() -> u8 {
    ((1u32 << LEDS) - 1) as u8
}

/// What the lights show: the engine speed, rpm, the gear and the car's shift points; `None` dark.
pub type Showing = Option<(f32, i32, ShiftPoints)>;

/// The lights' thread; dropping it darkens them.
pub struct Lights {
    showing: Arc<Mutex<Showing>>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The wheels lit, by name.
    pub names: Vec<String>,
}

impl Lights {
    /// Finds the wheels with rev lights and starts the thread, or says why there are none.
    pub fn open() -> Result<Lights, String> {
        let mut wheels = backend::open()?;
        let names = wheels.iter().map(|w| w.name()).collect();
        let (showing, paused, stop) =
            (Arc::new(Mutex::new(None)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let thread = {
            let (showing, paused, stop) = (Arc::clone(&showing), Arc::clone(&paused), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("wheel-lights".into())
                .spawn(move || {
                    let start = Instant::now();
                    let mut last = None;
                    let mut failed = false;
                    while !stop.load(Ordering::Relaxed) {
                        let now = if paused.load(Ordering::Relaxed) { None } else { *showing.lock().unwrap() };
                        let lit =
                            now.map_or(0, |(rpm, gear, shift)| mask(rpm, gear, &shift, start.elapsed().as_secs_f32()));
                        if last != Some(lit) {
                            last = Some(lit);
                            for wheel in &mut wheels {
                                if let Err(e) = wheel.set(lit) {
                                    if !failed {
                                        eprintln!("viewer: rev lights: {}: {e}", wheel.name());
                                    }
                                    failed = true;
                                }
                            }
                        }
                        std::thread::sleep(PERIOD);
                    }
                    for wheel in &mut wheels {
                        let _ = wheel.set(0);
                    }
                })
                .map_err(|e| e.to_string())?
        };
        Ok(Lights { showing, paused, stop, thread: Some(thread), names })
    }

    /// The lights from the next write on.
    pub fn show(&self, showing: Showing) {
        *self.showing.lock().unwrap() = showing;
    }

    /// Dark while `paused` (the pause menu, the stage result); what [`Lights::show`] set comes back after.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}

impl Drop for Lights {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A wheel's LEDs.
trait Leds: Send {
    fn name(&self) -> String;
    fn set(&mut self, mask: u8) -> Result<(), String>;
}

#[cfg(target_os = "linux")]
mod backend {
    use std::path::{Path, PathBuf};

    use super::Leds;

    /// Where the HID devices are.
    const HID_DEVICES: &str = "/sys/bus/hid/devices";

    /// A wheel's `RPM<n>` LEDs: each one's `brightness` and its `max_brightness`, first LED first.
    #[derive(Debug, PartialEq)]
    pub(super) struct Sysfs {
        pub(super) name: String,
        pub(super) leds: Vec<(PathBuf, String)>,
        /// The mask each LED shows now (none written yet).
        was: Option<u8>,
    }

    impl Leds for Sysfs {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn set(&mut self, mask: u8) -> Result<(), String> {
            for (k, (path, max)) in self.leds.iter().enumerate() {
                let on = mask >> k & 1;
                if self.was.is_some_and(|was| was >> k & 1 == on) {
                    continue;
                }
                std::fs::write(path, if on == 1 { max.as_str() } else { "0" })
                    .map_err(|e| format!("{}: {e}; the wheel tools' udev rule allows it", path.display()))?;
            }
            self.was = Some(mask);
            Ok(())
        }
    }

    pub(super) fn open() -> Result<Vec<Box<dyn Leds>>, String> {
        let wheels = wheels_in(Path::new(HID_DEVICES));
        if wheels.is_empty() {
            return Err("no wheel with rev lights (a wheel driver's RPM LEDs)".to_owned());
        }
        Ok(wheels.into_iter().map(|w| Box::new(w) as Box<dyn Leds>).collect())
    }

    /// The HID devices under `root` with `leds/*::RPM<n>`, by path.
    pub(super) fn wheels_in(root: &Path) -> Vec<Sysfs> {
        let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
        let mut wheels: Vec<(PathBuf, Sysfs)> = entries
            .flatten()
            .filter_map(|entry| {
                let dir = entry.path();
                let mut leds: Vec<(u32, PathBuf, String)> = std::fs::read_dir(dir.join("leds"))
                    .ok()?
                    .flatten()
                    .filter_map(|led| {
                        let name = led.file_name().into_string().ok()?;
                        let n = name.rsplit_once("::RPM")?.1.parse().ok()?;
                        let max = std::fs::read_to_string(led.path().join("max_brightness")).ok()?;
                        Some((n, led.path().join("brightness"), max.trim().to_owned()))
                    })
                    .collect();
                if leds.is_empty() {
                    return None;
                }
                leds.sort();
                let uevent = std::fs::read_to_string(dir.join("uevent")).unwrap_or_default();
                let name = uevent.lines().find_map(|l| l.strip_prefix("HID_NAME=")).unwrap_or("wheel").to_owned();
                let leds = leds.into_iter().map(|(_, path, max)| (path, max)).collect();
                Some((dir, Sysfs { name, leds, was: None }))
            })
            .collect();
        wheels.sort_by(|a, b| a.0.cmp(&b.0));
        wheels.into_iter().map(|(_, w)| w).collect()
    }
}

#[cfg(not(target_os = "linux"))]
mod backend {
    use std::ffi::CStr;

    use sdl3_sys::hidapi::*;

    use super::{Leds, CLASSIC};

    /// A wheel opened through SDL's HID API.
    struct Hid {
        name: String,
        device: *mut SDL_hid_device,
    }

    // SAFETY: the handle is used by the lights' thread alone after the move.
    unsafe impl Send for Hid {}

    impl Leds for Hid {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn set(&mut self, mask: u8) -> Result<(), String> {
            let report = super::classic_report(mask);
            // SAFETY: the device is open and the report outlives the call.
            let sent = unsafe { SDL_hid_write(self.device, report.as_ptr(), report.len()) };
            if sent < 0 {
                Err(crate::ffb::sdl_error())
            } else {
                Ok(())
            }
        }
    }

    impl Drop for Hid {
        fn drop(&mut self) {
            // SAFETY: opened by `open` and closed once.
            unsafe { SDL_hid_close(self.device) };
        }
    }

    pub(super) fn open() -> Result<Vec<Box<dyn Leds>>, String> {
        // SAFETY: SDL's HID API, initialised here and left initialised (SDL's joysticks share it); the enumeration is
        // freed.
        unsafe {
            if SDL_hid_init() < 0 {
                return Err(format!("SDL HID: {}", crate::ffb::sdl_error()));
            }
            let list = SDL_hid_enumerate(super::LOGITECH, 0);
            let mut out: Vec<Box<dyn Leds>> = Vec::new();
            let mut info = list;
            while !info.is_null() {
                let i = &*info;
                // The wheel's joystick collection (usage page 1, usage 4), not its other interfaces.
                let name = CLASSIC.iter().find(|(id, _)| *id == i.product_id).map(|(_, name)| *name);
                if let Some(name) = name.filter(|_| i.usage_page == 1 && i.usage == 4) {
                    let device = SDL_hid_open_path(i.path);
                    if !device.is_null() {
                        out.push(Box::new(Hid { name: name.to_owned(), device }));
                    } else {
                        let path = CStr::from_ptr(i.path).to_string_lossy();
                        eprintln!("viewer: rev lights: {name} ({path}): {}", crate::ffb::sdl_error());
                    }
                }
                info = i.next;
            }
            SDL_hid_free_enumeration(list);
            if out.is_empty() {
                return Err("no wheel with rev lights (Logitech G27, G29, G923 for PlayStation)".to_owned());
            }
            Ok(out)
        }
    }
}

/// Logitech's vendor ID.
#[cfg_attr(target_os = "linux", allow(dead_code))]
const LOGITECH: u16 = 0x046d;

/// The wheels that take Logitech's classic LED command, by product ID.
#[cfg_attr(target_os = "linux", allow(dead_code))]
const CLASSIC: [(u16, &str); 3] =
    [(0xc29b, "Logitech G27"), (0xc24f, "Logitech G29"), (0xc266, "Logitech G923 for PlayStation and PC")];

/// The classic LED command as SDL's HID API writes it: report ID 0 (the wheels number none), then `f8 12 <mask>`
/// padded to the 7-byte output report.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn classic_report(mask: u8) -> [u8; 8] {
    [0x00, 0xf8, 0x12, mask & all(), 0, 0, 0, 0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift() -> ShiftPoints {
        let mut shift = ShiftPoints { limit: 7500.0, ..ShiftPoints::default() };
        shift.up[2] = 6000.0;
        shift
    }

    #[test]
    fn the_lights_fill_up_to_the_gears_shift_point_and_flash_past_it() {
        let lit = |rpm| mask(rpm, 2, &shift(), 0.0);
        assert_eq!([4500.0, 4501.0, 4800.0, 4801.0, 5999.0].map(lit), [0, 0b1, 0b1, 0b11, 0b11111]);
        // Past it: all on, then off for the flash's other half.
        assert_eq!(lit(6000.0), 0b11111);
        assert_eq!(mask(6500.0, 2, &shift(), 1.0 / (2.0 * FLASH_HZ) + 0.01), 0);
    }

    #[test]
    fn a_gear_without_a_shift_point_takes_the_limit_and_none_is_dark() {
        assert_eq!(mask(7400.0, 7, &shift(), 0.0), 0b11111);
        assert_eq!(mask(6000.0, 7, &shift(), 0.0), 0);
        assert_eq!(mask(9000.0, -1, &ShiftPoints::default(), 0.0), 0);
    }

    #[test]
    fn the_classic_report_carries_five_leds() {
        assert_eq!(classic_report(0xff), [0, 0xf8, 0x12, 0x1f, 0, 0, 0, 0]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_the_rpm_leds_in_order_and_writes_changes_only() {
        let root = std::env::temp_dir().join(format!("wheel-lights-{}", std::process::id()));
        // sysfs names them `0003:046D:C266.0009::RPM1`; Windows takes no `:` in a name, so the test uses `_`.
        let wheel = root.join("0003_046D_C266.0009");
        for n in [2, 1, 3, 5, 4] {
            let led = wheel.join("leds").join(format!("wheel::RPM{n}"));
            std::fs::create_dir_all(&led).unwrap();
            std::fs::write(led.join("max_brightness"), "1\n").unwrap();
            std::fs::write(led.join("brightness"), "0\n").unwrap();
        }
        std::fs::create_dir_all(root.join("mouse").join("leds").join("mouse::scrolllock")).unwrap();
        std::fs::write(wheel.join("uevent"), "HID_NAME=Logitech G923\n").unwrap();
        let mut wheels = backend::wheels_in(&root);
        assert_eq!(wheels.len(), 1);
        assert_eq!(wheels[0].name, "Logitech G923");
        let read = |n: u32| std::fs::read_to_string(wheel.join(format!("leds/wheel::RPM{n}/brightness"))).unwrap();
        wheels[0].set(0b00101).unwrap();
        assert_eq!((1..=5).map(read).collect::<Vec<_>>(), ["1", "0", "1", "0", "0"]);
        // Only the LED that changes is written again.
        std::fs::write(wheel.join("leds/wheel::RPM2/brightness"), "kept").unwrap();
        wheels[0].set(0b00100).unwrap();
        assert_eq!((1..=5).map(read).collect::<Vec<_>>(), ["0", "kept", "1", "0", "0"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
