//! The player's camera choices, ours: the view last chosen and each car's nudges of the four views
//! ([`crate::views`]), set on the pause menu's Camera page ([`crate::hud::camera_page`]). The viewer keeps them in the
//! `[camera]` section of the launcher's `launcher-settings.ini` (`viewer --cameras`), which the launcher carries over
//! when it writes the file:
//!
//! ```ini
//! [camera]
//! view = cockpit
//! head = off
//! codriver = off
//! COROLLA cockpit = 0 0.04 -0.1 -2
//! COROLLA bonnet = 0 0.02 0
//! pack-mitsubishilancerevoixn4 chase = 0 0.5 -1
//! ```
//!
//! A car is the launcher's `--car-key` (a pack car's `pack-<pack>`), else its `Cars.ini` folder ([`car_key`]). A
//! nudge is right, up and forward in metres in the car's frame, and for the fixed views optionally a tilt in degrees
//! up, written only when it is not 0; the chase takes up (its height) and forward (nearer the car;
//! [`crate::game_camera::Settings::nudged`]). `head = off` keeps the in-car view fixed to the car ([`crate::head`]);
//! `driver = off`, `codriver = off` and `wheel = off` leave the driver's arms, the co-driver and the car's steering
//! wheel out of the cockpit view ([`Cockpit`]). Each is on unless set off, from the pause menu's Camera page or the
//! launcher's Graphics page.
//!
//! The Camera page's fields of view go to the launcher's own keys instead, `fov` and `chase_fov` in `[launcher]`
//! ([`Session::save_fov`]), the ones its Graphics page sets and the viewer takes as `--fov` and `--chase-fov`.

use crate::game_camera::Settings;
use crate::playback::Playback;
use crate::views::{CarView, Mounts, Nudge};
use glam::Vec3;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The section's name.
pub const SECTION: &str = "camera";

/// One step of the Camera page, m: the fixed views' and the chase's; and of the tilt, degrees.
pub const STEP: f32 = 0.02;
pub const CHASE_STEP: f32 = 0.1;
pub const TILT_STEP: f32 = 0.5;
/// One step of the Camera page's field of view, degrees.
pub const FOV_STEP: f32 = 1.0;
/// How far a fixed view moves from the car's own place, m, and tilts, degrees, each way (ours: keeps a typo from
/// losing the car).
pub const REACH: f32 = 1.0;
pub const TILT_REACH: f32 = 15.0;

/// One car's nudges, by [`CarView::index`].
pub type Nudges = [Nudge; CarView::ALL.len()];

/// What the cockpit view has besides its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cockpit {
    /// The head's motion ([`crate::head`]).
    pub head: bool,
    /// The driver's arms and the co-driver (`characters::incar`).
    pub driver: bool,
    pub codriver: bool,
    /// The car's steering wheel.
    pub wheel: bool,
}

impl Default for Cockpit {
    fn default() -> Cockpit {
        Cockpit { head: true, driver: true, codriver: true, wheel: true }
    }
}

impl Cockpit {
    /// The file's keys, in its order, each with its field.
    fn keys(&mut self) -> [(&'static str, &mut bool); 4] {
        [
            ("head", &mut self.head),
            ("driver", &mut self.driver),
            ("codriver", &mut self.codriver),
            ("wheel", &mut self.wheel),
        ]
    }
}

/// The view last chosen, the cockpit's choices and the cars' nudges.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraPrefs {
    pub view: Option<CarView>,
    pub cockpit: Cockpit,
    pub cars: BTreeMap<String, Nudges>,
}

/// The key of a game car without the launcher's `--car-key`: its folder in capitals.
pub fn car_key(folder: &str) -> String {
    folder.to_ascii_uppercase()
}

impl CameraPrefs {
    /// The `[camera]` section of `text`; nothing for what is missing or malformed.
    pub fn parse(text: &str) -> CameraPrefs {
        let sections = rbr_formats::ini::parse(text).unwrap_or_default();
        let Some(section) = sections.iter().find(|s| s.name == SECTION) else {
            return CameraPrefs::default();
        };
        let mut prefs = CameraPrefs::default();
        for (key, value) in &section.entries {
            if key == "view" {
                prefs.view = CarView::parse(value);
                continue;
            }
            if let Some((_, on)) = prefs.cockpit.keys().into_iter().find(|(name, _)| name == key) {
                *on = value.trim() != "off";
                continue;
            }
            let Some((car, view)) = key.rsplit_once(' ').and_then(|(car, v)| Some((car.trim(), CarView::parse(v)?)))
            else {
                continue;
            };
            let words: Option<Vec<f32>> = value.split_whitespace().map(|w| w.parse().ok()).collect();
            let nudge = match words.as_deref() {
                Some(&[x, y, z]) => Nudge { shift: Vec3::new(x, y, z), tilt: 0.0 },
                Some(&[x, y, z, tilt]) => Nudge { shift: Vec3::new(x, y, z), tilt },
                _ => continue,
            };
            prefs.cars.entry(car.to_owned()).or_default()[view.index()] = nudge;
        }
        prefs
    }

    /// The section's text: the view and every nudge that is not zero.
    pub fn to_text(&self) -> String {
        let mut out = format!("[{SECTION}]\n");
        if let Some(view) = self.view {
            out.push_str(&format!("view = {}\n", view.name()));
        }
        for (key, on) in self.cockpit.clone().keys() {
            if !*on {
                out.push_str(&format!("{key} = off\n"));
            }
        }
        for (car, nudges) in &self.cars {
            for view in CarView::ALL {
                let Nudge { shift: v, tilt } = nudges[view.index()];
                if nudges[view.index()] != Nudge::default() {
                    let tilt = if tilt != 0.0 { format!(" {tilt}") } else { String::new() };
                    out.push_str(&format!("{car} {} = {} {} {}{tilt}\n", view.name(), v.x, v.y, v.z));
                }
            }
        }
        out
    }

    /// Car `key`'s nudges (zeros when it has none).
    pub fn nudges(&self, key: &str) -> Nudges {
        self.cars.get(key).copied().unwrap_or_default()
    }

    /// Sets car `key`'s nudges, dropping the car when they are all zero.
    pub fn set_nudges(&mut self, key: &str, nudges: Nudges) {
        match nudges.iter().all(|v| *v == Nudge::default()) {
            true => self.cars.remove(key),
            false => self.cars.insert(key.to_owned(), nudges),
        };
    }

    /// `file` (a whole settings file) with its `[camera]` section replaced by this one, the rest kept as it is.
    pub fn replace_in(&self, file: &str) -> String {
        rbr_formats::ini::replace_section(file, SECTION, &self.to_text())
    }
}

/// The viewer's camera choices for one drive: the file they live in and the car's key.
#[derive(Debug, Clone, Default)]
pub struct Session {
    path: Option<PathBuf>,
    car: Option<String>,
    pub prefs: CameraPrefs,
}

impl Session {
    /// The choices in `path` (none without it or when it does not read) for the car `car` ([`car_key`]).
    pub fn load(path: Option<&Path>, car: Option<String>) -> Session {
        let prefs = path.and_then(|p| std::fs::read_to_string(p).ok()).map(|t| CameraPrefs::parse(&t));
        Session { path: path.map(Path::to_path_buf), car, prefs: prefs.unwrap_or_default() }
    }

    /// The view a drive starts in: `given` (`--view`), else the one last chosen, else the chase.
    pub fn start_view(&self, given: Option<CarView>) -> CarView {
        given.or(self.prefs.view).unwrap_or_default()
    }

    /// The car's nudges.
    pub fn nudges(&self) -> Nudges {
        self.car.as_deref().map(|car| self.prefs.nudges(car)).unwrap_or_default()
    }

    /// The cockpit's choices.
    pub fn cockpit(&self) -> Cockpit {
        self.prefs.cockpit
    }

    /// Puts the car's nudges on its fixed views and its chase, and the cockpit's choices on the in-car view.
    pub fn apply(&self, mounts: &mut Mounts, playback: &mut Playback) {
        let nudges = self.nudges();
        mounts.nudges = nudges;
        playback.set_chase_settings(Settings::default().nudged(nudges[CarView::Chase.index()].shift));
        playback.set_cockpit(self.cockpit());
    }

    /// The player chose `view` (C): kept for the next drive.
    pub fn chose(&mut self, view: CarView) {
        self.prefs.view = Some(view);
        self.save();
    }

    /// The view, the car's nudges and the cockpit's choices the Camera page set (not yet written: [`Session::save`]).
    pub fn set(&mut self, view: CarView, nudges: Nudges, cockpit: Cockpit) {
        self.prefs.view = Some(view);
        self.prefs.cockpit = cockpit;
        if let Some(car) = &self.car {
            self.prefs.set_nudges(car, nudges);
        }
    }

    /// Writes the choices into the file's `[camera]` section, the rest of the file kept.
    pub fn save(&self) {
        self.write(|file| self.prefs.replace_in(file));
    }

    /// Writes a field of view the Camera page set, radians, as the launcher's `key` (`fov` or `chase_fov`) in whole
    /// degrees, the rest of the file kept.
    pub fn save_fov(&self, key: &str, fov: f32) {
        let degrees = fov.to_degrees().round().to_string();
        self.write(|file| rbr_formats::ini::set_value(file, "launcher", key, &degrees));
    }

    fn write(&self, change: impl FnOnce(&str) -> String) {
        let Some(path) = &self.path else {
            return;
        };
        let file = std::fs::read_to_string(path).unwrap_or_default();
        if let Err(e) = std::fs::write(path, change(&file)) {
            eprintln!("viewer: {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nudge(x: f32, y: f32, z: f32, tilt: f32) -> Nudge {
        Nudge { shift: Vec3::new(x, y, z), tilt }
    }

    #[test]
    fn prefs_round_trip_and_keep_the_rest_of_the_file() {
        let cockpit = Cockpit { head: false, codriver: false, ..Default::default() };
        let mut prefs = CameraPrefs { view: Some(CarView::Cockpit), cockpit, ..Default::default() };
        let mut nudges = prefs.nudges("COROLLA");
        nudges[CarView::Cockpit.index()] = nudge(0.0, 0.04, -0.1, -2.0);
        nudges[CarView::Bonnet.index()] = nudge(0.0, 0.02, 0.0, 0.0);
        prefs.set_nudges("COROLLA", nudges);
        let mut chase = prefs.nudges("rsf-24");
        chase[CarView::Chase.index()] = nudge(0.0, 0.5, -1.0, 0.0);
        prefs.set_nudges("rsf-24", chase);
        let text = prefs.to_text();
        // The tilt is a 4th value only when it is not 0.
        assert_eq!(
            text,
            "[camera]\nview = cockpit\nhead = off\ncodriver = off\nCOROLLA cockpit = 0 0.04 -0.1 -2\n\
             COROLLA bonnet = 0 0.02 0\nrsf-24 chase = 0 0.5 -1\n"
        );
        assert_eq!(CameraPrefs::parse(&text), prefs);
        // A tilt alone is written too.
        let mut tilted = CameraPrefs::default();
        tilted.set_nudges("XSARA", [nudge(0.0, 0.0, 0.0, 1.5), Nudge::default(), Nudge::default(), Nudge::default()]);
        assert_eq!(tilted.to_text(), "[camera]\nXSARA cockpit = 0 0 0 1.5\n");
        // Replaced in place of the old section, the other sections kept.
        let file = "[launcher]\ncar = COROLLA\n\n[camera]\nview = chase\n\n[controls]\nbrake = KeyB\n";
        let replaced = prefs.replace_in(file);
        assert_eq!(replaced, format!("[launcher]\ncar = COROLLA\n\n[controls]\nbrake = KeyB\n\n{text}"));
        assert_eq!(CameraPrefs::parse(&replaced), prefs);
        assert_eq!(rbr_formats::ini::parse(&replaced).unwrap().len(), 3);
        // Zeros drop the car; bad lines are skipped.
        prefs.set_nudges("COROLLA", Default::default());
        assert!(!prefs.cars.contains_key("COROLLA"));
        let bad = CameraPrefs::parse("[camera]\nview = roof\nX cockpit = 1 2\nY roof = 1 2 3\nZ bonnet = 1 2 3 4 5\n");
        assert_eq!(bad, CameraPrefs::default());
    }

    #[test]
    fn a_file_of_three_value_lines_is_written_back_the_same() {
        let file = "[launcher]\ncar = COROLLA\n\n[camera]\nview = bumper\nCOROLLA cockpit = 0 0.04 -0.1\n\
                    COROLLA chase = 0 0.5 -1\nrsf-24 bumper = -0.02 0.06 0\n";
        let prefs = CameraPrefs::parse(file);
        assert_eq!(prefs.nudges("COROLLA")[CarView::Cockpit.index()], nudge(0.0, 0.04, -0.1, 0.0));
        assert_eq!(prefs.replace_in(file), file);
    }

    #[test]
    fn a_session_keeps_the_view_and_the_car_s_nudges() {
        let path = std::env::temp_dir().join(format!("camera-prefs-{}.ini", std::process::id()));
        std::fs::write(&path, "[launcher]\ncar = COROLLA\n").unwrap();
        let mut session = Session::load(Some(&path), Some("COROLLA".into()));
        assert_eq!(session.start_view(None), CarView::Chase);
        let mut nudges = session.nudges();
        nudges[CarView::Cockpit.index()] = nudge(0.0, STEP, STEP, -TILT_STEP);
        nudges[CarView::Chase.index()] = nudge(0.0, CHASE_STEP, 0.0, 0.0);
        assert_eq!(session.cockpit(), Cockpit::default(), "on unless set off");
        let cockpit = Cockpit { driver: false, wheel: false, ..Default::default() };
        session.set(CarView::Cockpit, nudges, cockpit);
        session.save();
        session.chose(CarView::Bumper);
        session.save_fov("chase_fov", 80.0f32.to_radians());
        let again = Session::load(Some(&path), Some("COROLLA".into()));
        assert_eq!((again.start_view(None), again.nudges(), again.cockpit()), (CarView::Bumper, nudges, cockpit));
        assert_eq!(again.start_view(Some(CarView::Chase)), CarView::Chase);
        let file = std::fs::read_to_string(&path).unwrap();
        assert!(file.starts_with("[launcher]\ncar = COROLLA\nchase_fov = 80\n\n[camera]\n"), "{file}");
        // Another car has none.
        assert_eq!(Session::load(Some(&path), Some("XSARA".into())).nudges(), Nudges::default());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn car_keys() {
        assert_eq!(car_key("corolla"), "COROLLA");
    }
}
