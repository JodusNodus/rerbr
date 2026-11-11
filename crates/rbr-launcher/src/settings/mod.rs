//! The settings file, `launcher-settings.ini` in the player's folder (`home`): the form read from it and written to it.

use std::str::FromStr;

use rbr_formats::ini;
use rbr_gameplay::race_options::DamageLevel;
use rbr_render::bindings::Bindings;
use rbr_render::camera_prefs::CameraPrefs;
use rbr_render::coords;
use rbr_render::ffb;
use rbr_render::glass::model::Switch;
use rbr_render::hud::model::STACK_MAX;
use rbr_render::hud::Parts;
use rbr_render::mirrors::Rate;
use rbr_render::present::Present;
use rbr_render::renderer::Detail;
use rbr_render::shifter::{CarBox, Engagement};

use crate::catalog::{Car, TyreChoice, WEARS};
use crate::ghosts::GhostChoice;
use crate::graphics_page::{
    WetRoads, FRAME_CAPS, MAX_GROUP_VOLUME, MAX_VOLUME, RENDER_SCALES, TRIPLE_ANGLE_RANGE, TRIPLE_BEZEL_RANGE,
    TRIPLE_DIAGONAL_RANGE, TRIPLE_DISTANCE_RANGE,
};
use crate::pages::Choice;
use crate::physics::PhysicsProfile;
use crate::time_of_day::StartTime;
#[cfg(test)]
use rbr_gameplay::weather::Preset;
#[cfg(test)]
use std::path::Path;

mod form;
mod weather;

pub use form::{load, or_folder, read_settings};
pub use weather::{parse_wetness, wetness_name, Clouds, Fog, Precipitation, WeatherChoice};

const DEFAULT_SIZE: (u32, u32) = (1280, 720);

/// `minutes` past midnight as `HH:MM`.
pub fn clock_text(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Parses `triple_monitors`' `LEFT,CENTRE,RIGHT` names; empty or unset is the one wide display.
fn parse_monitors(text: &str) -> Option<[String; 3]> {
    let mut parts = text.split(',').map(|part| part.trim().to_owned());
    let monitors = [parts.next()?, parts.next()?, parts.next()?];
    (parts.next().is_none() && monitors.iter().all(|name| !name.is_empty())).then_some(monitors)
}

/// Parses `WxH`.
fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// The launcher's last selection, the viewer's bindings and its force feedback (the `[ffb]` section).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub car: Option<String>,
    pub map: Option<u32>,
    /// The last cars (their names) and maps picked, newest first (`recent_cars = A|B`, `recent_maps = 54,129`).
    pub recent_cars: Vec<String>,
    pub recent_maps: Vec<u32>,
    /// The cars (their names) and stages starred, in the order starred (`favourite_cars = A|B`, `favourite_maps =
    /// 54,129`).
    pub favourite_cars: Vec<String>,
    pub favourite_maps: Vec<u32>,
    pub stage_browser: crate::browser_state::State,
    pub car_browser: crate::browser_state::State,
    pub weather: Option<WeatherChoice>,
    /// The road wetness over the weather row's (0 dry, 1 damp, 2 wet); none takes it from the weather.
    pub wetness: Option<u32>,
    /// Our fog over the stage; none takes it from the weather.
    pub fog: Option<Fog>,
    /// Our cloud cover over the stage; none takes it from the weather.
    pub clouds: Option<Clouds>,
    /// Our rain or snow over the stage; none takes it from the weather.
    pub precipitation: Option<Precipitation>,
    pub wear: Option<u32>,
    pub tyre: Option<TyreChoice>,
    pub size: Option<(u32, u32)>,
    /// Every view's horizontal field of view, degrees (`viewer --fov`).
    pub fov: Option<f32>,
    /// The chase view's field of view, degrees (`viewer --chase-fov`).
    pub chase_fov: Option<f32>,
    /// The sound volume, linear, 0 to [`MAX_VOLUME`] (`viewer --volume`).
    pub volume: Option<f32>,
    /// The sound group volumes, linear, 0 to [`MAX_GROUP_VOLUME`] (`viewer --volumes E,C,F,T`).
    pub engine_volume: Option<f32>,
    pub codriver_volume: Option<f32>,
    pub effects_volume: Option<f32>,
    pub tyres_volume: Option<f32>,
    /// The co-driver calls the pace notes (`viewer --no-codriver` when off).
    pub codriver: Option<bool>,
    /// The co-driver mod's voice (`1.02` the original co-driver; none: the plugin's), its call style and range set
    /// (`viewer --codriver-voice`, `--codriver-style`, `--codriver-ranges`).
    pub codriver_voice: Option<String>,
    pub codriver_style: Option<String>,
    pub codriver_ranges: Option<String>,
    /// The locally owned voice pack (`codriver_packs::VOICES_DIR`) and stage-note pack
    /// (`codriver_packs::NOTES_DIR`); none each for the game folder's.
    pub codriver_pack: Option<String>,
    pub notes_pack: Option<String>,
    /// The note set chosen in it (`codriver_packs::NoteSet`); none for its largest.
    pub notes_set: Option<String>,
    /// The stage's NGPCarMenu MyPacenotes notes are called (`viewer --pacenotes`); on when unset.
    pub my_pacenotes: Option<bool>,
    /// The HUD shown (`viewer --no-hud` when off).
    pub hud: Option<bool>,
    /// The speed in mph (`viewer --mph`), else km/h.
    pub mph: Option<bool>,
    /// The FPS counter (`viewer --fps`).
    pub fps: Option<bool>,
    /// The pace stack's notes (`viewer --pace-stack`, 0 to 8).
    pub pace_stack: Option<u8>,
    /// The working dials (`viewer --no-dials` when off).
    pub dials: Option<bool>,
    /// The HUD's parts shown (`viewer --hud-hide`), kept as the hidden ones' list.
    pub hud_hide: Option<Parts>,
    /// The wiper switch at the start (`viewer --wipers`).
    pub wipers: Option<Switch>,
    /// The Quality section's switches ([`Graphics`]), each kept under its name.
    pub lighting: Option<bool>,
    pub particles: Option<bool>,
    pub cabin: Option<bool>,
    pub contact: Option<bool>,
    pub grass: Option<bool>,
    pub windscreen: Option<bool>,
    pub marks: Option<bool>,
    pub motion_blur: Option<bool>,
    /// One of [`RENDER_SCALES`].
    pub render_scale: Option<u32>,
    /// The wet roads (`viewer --enhance wet`, `--reflections`).
    pub wet_roads: Option<WetRoads>,
    /// The additions' detail (`viewer --detail`).
    pub detail: Option<Detail>,
    /// The car's and the stage's textures upscaled (`viewer --upscale-textures`).
    pub upscale_textures: Option<bool>,
    /// The driving window's frame presentation (`viewer --present`).
    pub present: Option<Present>,
    /// The driving window's frame cap (`viewer --max-fps`; 0: none), one of [`FRAME_CAPS`].
    pub max_fps: Option<u32>,
    /// How often the cockpit's mirrors are drawn (`viewer --mirrors`).
    pub mirrors: Option<Rate>,
    /// The driving window fills the monitor (`viewer --fullscreen`).
    pub fullscreen: Option<bool>,
    /// Triple screens (`viewer --triple`); off by default, the field of view is the centre screen's.
    pub triple: Option<bool>,
    /// Each screen's diagonal, inches (27 by default, 10 to 65).
    pub triple_diagonal: Option<f32>,
    /// The frame around each screen's image, millimetres (10 by default, 0 to 60).
    pub triple_bezel: Option<f32>,
    /// From the eye to the centre screen, centimetres (70 by default, 30 to 200).
    pub triple_distance: Option<f32>,
    /// The side screens' turn, degrees; `None` faces them at the eye (`auto`, the default; 0 to 90).
    pub triple_angle: Option<Option<f32>>,
    /// The three monitors' names (left, centre, right); none: one wide display.
    pub triple_monitors: Option<[String; 3]>,
    /// The driving window shows the highlights past an HDR display's SDR white (`viewer --hdr`).
    pub hdr: Option<bool>,
    /// The viewer writes a performance log (`viewer --perf-log`, in the player's `logs` folder).
    pub perf_log: Option<bool>,
    /// The Custom weather's start time (`viewer --clock`).
    pub start_time: Option<StartTime>,
    pub callout_distance: Option<f32>,
    pub marker_threshold: Option<f32>,
    /// The Drive page's race options (`rbr_gameplay::race_options`).
    pub damage: Option<DamageLevel>,
    pub auto_gears: Option<bool>,
    pub gear_guard: Option<bool>,
    pub neutral_lock: Option<bool>,
    pub clutch_help: Option<bool>,
    /// The quick race's ghost (`off`, `best` or a `GhostCars` file name).
    pub ghost: Option<GhostChoice>,
    /// The RBR install folder (`crate::install`).
    pub install_folder: Option<String>,
    /// NGP's `physics.rbz` (`rbr-standalone --ngp-physics`) when not the install folder's.
    pub ngp_physics: Option<String>,
    /// Selected NGP DLL; empty follows the installed plugin.
    pub ngp_dll: Option<String>,
    /// NGP's `headGasketDurability` (`rbr-standalone --head-gasket-durability`; 1, NGP's default, leaves the engine
    /// as it is).
    pub head_gasket: Option<i32>,
    /// The physics (`rbr-standalone --physics`); none for NGP 7's.
    pub physics: Option<PhysicsProfile>,
    /// The brush tyre's experimental tuning (`crate::tyre_tuning`), when off the model's.
    pub tyre_tuning: Option<rbr_tyre::Tuning>,
    pub bindings: Bindings,
    pub ffb: ffb::Options,
    /// Each car's own force-feedback sensitivity (`[ffb_cars]`, `rbr_render::ffb::CarSensitivity`): the viewer owns
    /// it while driving, the launcher carries it over and the Controls page edits it.
    pub ffb_cars: ffb::CarSensitivity,
    /// Each car's steering load per surface (`[ffb_load]`, `rbr_render::ffb::Loads`): the viewer tracks it, the
    /// launcher carries it over and shows it.
    pub ffb_load: ffb::Loads,
    /// The viewer's camera choices (`[camera]`, `rbr_render::camera_prefs`): the viewer writes them, the launcher
    /// carries them over and resets a car's.
    pub camera: CameraPrefs,
    /// Each car's gearbox for the advanced H-shifter (`[gearbox]`).
    pub gearboxes: Gearboxes,
    /// The setup each car drives with on each surface (`[setup]`).
    pub setups: SetupChoices,
}

/// Each car's gearbox for the advanced H-shifter, the Drive page's choice by car name (`<car> = synchro|dog`,
/// ours). A car without one is sequential, which leaves the shifter off and the gearbox aids on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gearboxes(pub Vec<(String, CarBox)>);

const GEARBOX: &str = "gearbox";

/// A sequential gearbox, every car's until one is chosen.
pub const SEQUENTIAL: CarBox = CarBox { engagement: Engagement::Synchro, sequential: true };

impl Gearboxes {
    fn parse(sections: &[ini::Section]) -> Gearboxes {
        let Some(section) = sections.iter().find(|s| s.name == GEARBOX) else { return Gearboxes::default() };
        let boxes = section
            .entries
            .iter()
            .filter_map(|(car, value)| CarBox::parse_arg(value).filter(|b| !b.sequential).map(|b| (car.clone(), b)));
        Gearboxes(boxes.collect())
    }

    fn to_text(&self) -> String {
        if self.0.is_empty() {
            return String::new();
        }
        let lines = self.0.iter().map(|(car, gearbox)| format!("{car} = {}\n", gearbox.arg()));
        format!("\n[{GEARBOX}]\n{}", lines.collect::<String>())
    }

    /// The gearbox of `car`.
    pub fn of(&self, car: &str) -> CarBox {
        self.0.iter().find(|(c, _)| c == car).map_or(SEQUENTIAL, |(_, b)| *b)
    }

    pub fn set(&mut self, car: &str, gearbox: CarBox) {
        self.0.retain(|(c, _)| c != car);
        if !gearbox.sequential {
            self.0.push((car.to_owned(), gearbox));
        }
    }
}

/// The saved setup each car drives with on each surface, the Drive page's choice by car name (`<surface> <car> =
/// <setup>`, ours; `setup_page`). A car without one on a surface drives the default setup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetupChoices(pub Vec<(String, String, String)>);

const SETUP: &str = "setup";

impl SetupChoices {
    fn parse(sections: &[ini::Section]) -> SetupChoices {
        let Some(section) = sections.iter().find(|s| s.name == SETUP) else { return SetupChoices::default() };
        let choices = section.entries.iter().filter_map(|(key, name)| {
            let (surface, car) = key.split_once(' ')?;
            let car = car.trim();
            (!car.is_empty() && !name.is_empty()).then(|| (car.to_owned(), surface.to_owned(), name.clone()))
        });
        SetupChoices(choices.collect())
    }

    fn to_text(&self) -> String {
        if self.0.is_empty() {
            return String::new();
        }
        let lines = self.0.iter().map(|(car, surface, name)| format!("{surface} {car} = {name}\n"));
        format!("\n[{SETUP}]\n{}", lines.collect::<String>())
    }

    /// The setup `car` drives with on `surface`; `None` for the default.
    pub fn of(&self, car: &str, surface: &str) -> Option<&str> {
        self.0.iter().find(|(c, s, _)| c == car && s == surface).map(|(_, _, n)| n.as_str())
    }

    pub fn set(&mut self, car: &str, surface: &str, name: Option<&str>) {
        self.0.retain(|(c, s, _)| !(c == car && s == surface));
        if let Some(name) = name {
            self.0.push((car.to_owned(), surface.to_owned(), name.to_owned()));
        }
    }
}

/// The car of `cars` the first of `names` names, else the first that can start, else the first.
pub fn pick_car<'a>(cars: &[Car], names: impl IntoIterator<Item = &'a String>) -> usize {
    names
        .into_iter()
        .find_map(|name| cars.iter().position(|c| c.name == *name))
        .or_else(|| cars.iter().position(|c| c.unavailable().is_none()))
        .unwrap_or(0)
}

fn on_off(value: Option<String>) -> Option<bool> {
    match value?.as_str() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

fn on_off_text(value: Option<bool>) -> Option<String> {
    value.map(|v| if v { "on" } else { "off" }.to_owned())
}

/// A [`Choice`]'s settings word.
fn choice<T: Choice>(value: Option<T>) -> Option<String> {
    value.map(|c| c.name().to_owned())
}

/// A `|` list of names, when any.
fn joined_names(names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| names.join("|"))
}

/// A `,` list of ids, when any.
fn joined_ids(ids: &[u32]) -> Option<String> {
    (!ids.is_empty()).then(|| ids.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
}

/// The `[launcher]` section's values, read once per key.
struct Launcher<'a> {
    section: Option<&'a ini::Section>,
}

impl Launcher<'_> {
    /// The key's text, when the section sets it.
    fn get(&self, key: &str) -> Option<String> {
        self.section.and_then(|s| s.get(key)).map(str::to_owned)
    }

    /// An `on`/`off` switch.
    fn on_off(&self, key: &str) -> Option<bool> {
        on_off(self.get(key))
    }

    /// A [`Choice`] by its settings word.
    fn choice<T: Choice>(&self, key: &str) -> Option<T> {
        self.get(key).as_deref().and_then(T::parse)
    }

    /// A value parsed from its text.
    fn parsed<T>(&self, key: &str, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
        self.get(key).as_deref().and_then(parse)
    }

    /// A number as written.
    fn number<T: FromStr>(&self, key: &str) -> Option<T> {
        self.get(key).and_then(|v| v.parse().ok())
    }

    /// A number within `range`.
    fn ranged<T: FromStr + PartialOrd>(&self, key: &str, range: std::ops::RangeInclusive<T>) -> Option<T> {
        self.number(key).filter(|v| range.contains(v))
    }

    /// A word that must not be empty.
    fn word(&self, key: &str) -> Option<String> {
        self.get(key).filter(|v| !v.is_empty())
    }

    /// A `|` list of names, in file order.
    fn names(&self, key: &str) -> Vec<String> {
        self.get(key)
            .map(|v| v.split('|').map(str::trim).filter(|c| !c.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// A `,` list of ids, in file order.
    fn ids(&self, key: &str) -> Vec<u32> {
        self.get(key).map(|v| v.split(',').filter_map(|m| m.trim().parse().ok()).collect()).unwrap_or_default()
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let sections = ini::parse(text).unwrap_or_default();
        let launcher = Launcher { section: sections.iter().find(|s| s.name == "launcher") };
        Settings {
            car: launcher.get("car"),
            map: launcher.number("map"),
            recent_cars: launcher.names("recent_cars"),
            recent_maps: launcher.ids("recent_maps"),
            favourite_cars: launcher.names("favourite_cars"),
            favourite_maps: launcher.ids("favourite_maps"),
            stage_browser: crate::browser_state::State::read(&sections, "stage_browser"),
            car_browser: crate::browser_state::State::read(&sections, "car_browser"),
            weather: launcher.parsed("weather", WeatherChoice::parse),
            wetness: launcher.parsed("wetness", parse_wetness),
            fog: launcher.choice("fog"),
            clouds: launcher.choice("clouds"),
            precipitation: launcher.choice("precipitation"),
            wear: launcher
                .get("wear")
                .and_then(|v| WEARS.iter().position(|wear| *wear == v.as_str()))
                .map(|wear| wear as u32),
            tyre: launcher.get("tyre").and_then(|v| {
                if v == "auto" {
                    Some(TyreChoice::Auto)
                } else {
                    v.parse::<u32>().ok().filter(|n| *n <= 6).map(TyreChoice::Fixed)
                }
            }),
            size: launcher.parsed("size", parse_size),
            fov: launcher.ranged("fov", coords::FOV_RANGE_DEGREES),
            chase_fov: launcher.ranged("chase_fov", coords::FOV_RANGE_DEGREES),
            volume: launcher.ranged("volume", 0.0..=MAX_VOLUME),
            engine_volume: launcher.ranged("engine_volume", 0.0..=MAX_GROUP_VOLUME),
            codriver_volume: launcher.ranged("codriver_volume", 0.0..=MAX_GROUP_VOLUME),
            effects_volume: launcher.ranged("effects_volume", 0.0..=MAX_GROUP_VOLUME),
            tyres_volume: launcher.ranged("tyres_volume", 0.0..=MAX_GROUP_VOLUME),
            codriver: launcher.on_off("codriver"),
            codriver_voice: launcher.word("codriver_voice"),
            codriver_style: launcher.word("codriver_style"),
            codriver_ranges: launcher.word("codriver_ranges"),
            codriver_pack: launcher.word("codriver_pack"),
            notes_pack: launcher.word("notes_pack"),
            notes_set: launcher.get("notes_set"),
            my_pacenotes: launcher.on_off("my_pacenotes"),
            hud: launcher.on_off("hud"),
            mph: launcher.get("units").and_then(|v| match v.as_str() {
                "mph" => Some(true),
                "kmh" => Some(false),
                _ => None,
            }),
            fps: launcher.on_off("fps"),
            pace_stack: launcher.number("pace_stack").filter(|n| *n <= STACK_MAX),
            dials: launcher.on_off("dials"),
            hud_hide: launcher.get("hud_hide").and_then(|v| Parts::hiding(&v).ok()),
            wipers: launcher.parsed("wipers", Switch::parse),
            lighting: launcher.on_off("lighting"),
            particles: launcher.on_off("particles"),
            cabin: launcher.on_off("cabin"),
            contact: launcher.on_off("contact"),
            grass: launcher.on_off("grass"),
            windscreen: launcher.on_off("windscreen"),
            marks: launcher.on_off("marks"),
            motion_blur: launcher.on_off("motion_blur"),
            render_scale: launcher.number("render_scale").filter(|v| RENDER_SCALES.contains(v)),
            wet_roads: launcher.choice("wet_roads"),
            detail: launcher.choice("detail"),
            upscale_textures: launcher.on_off("upscale_textures"),
            present: launcher.parsed("present", Present::parse),
            max_fps: launcher.number("max_fps").filter(|v| FRAME_CAPS.contains(v)),
            mirrors: launcher.parsed("mirrors", Rate::parse),
            fullscreen: launcher.on_off("fullscreen"),
            triple: launcher.on_off("triple"),
            triple_diagonal: launcher.ranged("triple_diagonal", TRIPLE_DIAGONAL_RANGE),
            triple_bezel: launcher.ranged("triple_bezel", TRIPLE_BEZEL_RANGE),
            triple_distance: launcher.ranged("triple_distance", TRIPLE_DISTANCE_RANGE),
            triple_angle: match launcher.get("triple_angle").as_deref() {
                None => None,
                Some("auto") => Some(None),
                Some(degrees) => degrees.parse::<f32>().ok().filter(|a| TRIPLE_ANGLE_RANGE.contains(a)).map(Some),
            },
            triple_monitors: launcher.parsed("triple_monitors", parse_monitors),
            hdr: launcher.on_off("hdr"),
            perf_log: launcher.on_off("perf_log"),
            start_time: launcher.parsed("start_time", StartTime::parse),
            callout_distance: launcher.number("callout_distance"),
            marker_threshold: launcher.number("marker_threshold"),
            damage: launcher.choice("damage"),
            auto_gears: launcher.on_off("auto_gears"),
            gear_guard: launcher.on_off("gear_guard"),
            neutral_lock: launcher.on_off("neutral_lock"),
            clutch_help: launcher.on_off("clutch_help"),
            ghost: launcher.get("ghost").map(|v| GhostChoice::parse(&v)),
            install_folder: launcher.word("install_folder"),
            ngp_physics: launcher.word("ngp_physics"),
            ngp_dll: launcher.get("ngp_dll"),
            head_gasket: launcher.number("head_gasket").filter(|v| *v >= 1),
            physics: launcher.choice("physics"),
            tyre_tuning: launcher.get("tyre_tuning").and_then(|v| rbr_tyre::Tuning::parse(&v).ok()),
            bindings: Bindings::parse(text),
            ffb: ffb::Options::parse(text),
            ffb_cars: ffb::CarSensitivity::parse(text),
            ffb_load: ffb::Loads::parse(text),
            camera: {
                // The launcher's own `hide_wheel` of before is the cockpit's `wheel = off`.
                let mut camera = CameraPrefs::parse(text);
                camera.cockpit.wheel &= launcher.on_off("hide_wheel") != Some(true);
                camera
            },
            gearboxes: Gearboxes::parse(&sections),
            setups: SetupChoices::parse(&sections),
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("[launcher]\n");
        for (key, value) in [
            ("car", self.car.clone()),
            ("map", self.map.map(|v| v.to_string())),
            ("recent_cars", joined_names(&self.recent_cars)),
            ("recent_maps", joined_ids(&self.recent_maps)),
            ("favourite_cars", joined_names(&self.favourite_cars)),
            ("favourite_maps", joined_ids(&self.favourite_maps)),
            ("weather", self.weather.as_ref().map(WeatherChoice::to_text)),
            ("wetness", self.wetness.map(wetness_name).map(str::to_owned)),
            ("fog", choice(self.fog)),
            ("clouds", choice(self.clouds)),
            ("precipitation", choice(self.precipitation)),
            ("wear", self.wear.map(|v| WEARS[v.min(2) as usize].to_owned())),
            (
                "tyre",
                self.tyre.map(|v| match v {
                    TyreChoice::Auto => "auto".to_owned(),
                    TyreChoice::Fixed(n) => n.to_string(),
                }),
            ),
            ("size", self.size.map(|(w, h)| format!("{w}x{h}"))),
            ("fov", self.fov.map(|v| v.to_string())),
            ("chase_fov", self.chase_fov.map(|v| v.to_string())),
            ("volume", self.volume.map(|v| v.to_string())),
            ("engine_volume", self.engine_volume.map(|v| v.to_string())),
            ("codriver_volume", self.codriver_volume.map(|v| v.to_string())),
            ("effects_volume", self.effects_volume.map(|v| v.to_string())),
            ("tyres_volume", self.tyres_volume.map(|v| v.to_string())),
            ("codriver", on_off_text(self.codriver)),
            ("codriver_voice", self.codriver_voice.clone()),
            ("codriver_style", self.codriver_style.clone()),
            ("codriver_ranges", self.codriver_ranges.clone()),
            ("codriver_pack", self.codriver_pack.clone()),
            ("notes_pack", self.notes_pack.clone()),
            ("notes_set", self.notes_set.clone()),
            ("my_pacenotes", on_off_text(self.my_pacenotes)),
            ("hud", on_off_text(self.hud)),
            ("units", self.mph.map(|v| if v { "mph" } else { "kmh" }.to_owned())),
            ("fps", on_off_text(self.fps)),
            ("pace_stack", self.pace_stack.map(|v| v.to_string())),
            ("wipers", self.wipers.map(|v| v.name().to_owned())),
            ("dials", on_off_text(self.dials)),
            ("hud_hide", self.hud_hide.and_then(|p| p.hidden())),
            ("callout_distance", self.callout_distance.map(|v| v.to_string())),
            ("marker_threshold", self.marker_threshold.map(|v| v.to_string())),
            ("damage", choice(self.damage)),
            ("auto_gears", on_off_text(self.auto_gears)),
            ("gear_guard", on_off_text(self.gear_guard)),
            ("neutral_lock", on_off_text(self.neutral_lock)),
            ("clutch_help", on_off_text(self.clutch_help)),
            ("ghost", self.ghost.as_ref().map(GhostChoice::to_text)),
            ("install_folder", self.install_folder.clone()),
            ("ngp_physics", self.ngp_physics.clone()),
            ("ngp_dll", self.ngp_dll.clone()),
            ("head_gasket", self.head_gasket.map(|v| v.to_string())),
            ("physics", choice(self.physics)),
            ("tyre_tuning", self.tyre_tuning.map(|t| t.to_string())),
            ("render_scale", self.render_scale.map(|v| v.to_string())),
            ("wet_roads", choice(self.wet_roads)),
            ("detail", choice(self.detail)),
            ("upscale_textures", on_off_text(self.upscale_textures)),
            ("present", self.present.map(|v| v.name().to_owned())),
            ("max_fps", self.max_fps.map(|v| v.to_string())),
            ("mirrors", self.mirrors.map(Rate::name)),
            ("fullscreen", on_off_text(self.fullscreen)),
            ("triple", on_off_text(self.triple)),
            ("triple_diagonal", self.triple_diagonal.map(|v| v.to_string())),
            ("triple_bezel", self.triple_bezel.map(|v| v.to_string())),
            ("triple_distance", self.triple_distance.map(|v| v.to_string())),
            ("triple_angle", self.triple_angle.map(|angle| angle.map_or_else(|| "auto".to_owned(), |a| a.to_string()))),
            ("triple_monitors", self.triple_monitors.clone().map(|monitors| monitors.join(","))),
            ("hdr", on_off_text(self.hdr)),
            ("start_time", self.start_time.map(StartTime::to_text)),
            ("perf_log", on_off_text(self.perf_log)),
            ("lighting", on_off_text(self.lighting)),
            ("particles", on_off_text(self.particles)),
            ("cabin", on_off_text(self.cabin)),
            ("contact", on_off_text(self.contact)),
            ("grass", on_off_text(self.grass)),
            ("windscreen", on_off_text(self.windscreen)),
            ("marks", on_off_text(self.marks)),
            ("motion_blur", on_off_text(self.motion_blur)),
        ] {
            if let Some(v) = value {
                out.push_str(&format!("{key} = {v}\n"));
            }
        }
        out.push_str(&self.stage_browser.to_text("stage_browser"));
        out.push_str(&self.car_browser.to_text("car_browser"));
        out.push('\n');
        out.push_str(&self.bindings.to_text());
        out.push('\n');
        out.push_str(&self.ffb.to_text());
        out.push('\n');
        out.push_str(&self.ffb_cars.to_text());
        out.push('\n');
        out.push_str(&self.ffb_load.to_text());
        out.push('\n');
        out.push_str(&self.camera.to_text());
        out.push_str(&self.gearboxes.to_text());
        out.push_str(&self.setups.to_text());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{clock_args, render_scale_args};

    #[test]
    fn an_empty_dll_choice_preserves_automatic_discovery() {
        let settings = Settings::parse("[launcher]\nngp_dll = \n");
        assert_eq!(settings.ngp_dll.as_deref(), Some(""));
        assert_eq!(Settings::parse(&settings.to_text()).ngp_dll.as_deref(), Some(""));
        assert_eq!(Settings::default().ngp_dll, None);
    }

    /// Every field set: `to_text` then `parse` is the identity, and `full_settings_golden` pins the text.
    fn full_settings() -> Settings {
        Settings {
            car: Some("COROLLA".into()),
            map: Some(54),
            recent_cars: vec!["COROLLA".into(), "Mitsubishi Lancer Evo IX N4".into()],
            recent_maps: vec![54, 129],
            favourite_cars: vec!["XSARA".into(), "Mitsubishi Lancer Evo IX N4".into()],
            favourite_maps: vec![129, 54, 300],
            stage_browser: Default::default(),
            car_browser: Default::default(),
            weather: Some(WeatherChoice::Preset(Preset::Bad)),
            wetness: Some(2),
            fog: Some(Fog::Heavy),
            clouds: Some(Clouds::Partly),
            precipitation: Some(Precipitation::Moderate),
            wear: Some(2),
            tyre: Some(TyreChoice::Fixed(5)),
            size: Some((1920, 1080)),
            fov: Some(72.5),
            chase_fov: Some(80.0),
            volume: Some(0.75),
            engine_volume: Some(0.5),
            codriver_volume: Some(1.5),
            effects_volume: Some(0.0),
            tyres_volume: Some(2.0),
            codriver: Some(false),
            codriver_voice: Some("Janne Laahanen EnNu A".into()),
            codriver_style: Some("Numeric.ini".into()),
            codriver_ranges: Some("Rbr.ini".into()),
            codriver_pack: Some("luppis".into()),
            notes_pack: Some("tanner-notes".into()),
            notes_set: Some(String::new()),
            my_pacenotes: Some(false),
            hud: Some(false),
            mph: Some(true),
            fps: Some(true),
            pace_stack: Some(2),
            dials: Some(false),
            hud_hide: Some(Parts::hiding("progress,car").unwrap()),
            wipers: Some(Switch::Intermittent),
            lighting: Some(false),
            particles: Some(false),
            cabin: Some(false),
            contact: Some(false),
            grass: Some(true),
            windscreen: Some(false),
            marks: Some(true),
            motion_blur: Some(true),
            render_scale: Some(75),
            wet_roads: Some(WetRoads::Low),
            detail: Some(Detail::Ultra),
            upscale_textures: Some(true),
            present: Some(Present::Immediate),
            max_fps: Some(120),
            mirrors: Some(Rate::Every(3)),
            fullscreen: Some(false),
            triple: Some(true),
            triple_diagonal: Some(32.0),
            triple_bezel: Some(8.0),
            triple_distance: Some(65.0),
            triple_angle: Some(Some(45.0)),
            triple_monitors: Some(["HDMI-1".into(), "DP-1".into(), "HDMI-2".into()]),
            hdr: Some(true),
            perf_log: Some(true),
            start_time: Some(StartTime::Baked),
            callout_distance: Some(0.5),
            marker_threshold: Some(12.5),
            damage: Some(DamageLevel::Realistic),
            auto_gears: Some(true),
            gear_guard: Some(false),
            neutral_lock: Some(true),
            clutch_help: Some(false),
            ghost: Some(GhostChoice::File("me_54.ghc".into())),
            install_folder: Some("/games/Richard Burns Rally".into()),
            ngp_physics: Some("/games/rbr-ngp/physics.rbz".into()),
            ngp_dll: Some("/games/ngp/Plugins/NGP.dll".into()),
            head_gasket: Some(10),
            physics: Some(PhysicsProfile::Experimental),
            tyre_tuning: Some(rbr_tyre::Tuning { slide_grip: Some(0.8), snow: 1.15, ..rbr_tyre::Tuning::MODEL }),
            bindings: Bindings { input_rate: 500, ..Default::default() },
            ffb: ffb::Options { rate: 500, ..Default::default() },
            ffb_cars: ffb::CarSensitivity {
                cars: [("rsf-24".to_owned(), [90, 100, 110]), ("COROLLA".to_owned(), [120, 120, 130])].into(),
            },
            ffb_load: ffb::Loads::parse("[ffb_load]\nrsf-24 tarmac = 0,12,340\n"),
            camera: CameraPrefs::parse("[camera]\nview = bonnet\nrsf-24 chase = 0 0.5 -1\n"),
            gearboxes: Gearboxes(vec![(
                "Ford Escort Mk II".into(),
                CarBox { engagement: Engagement::Dog, sequential: false },
            )]),
            setups: SetupChoices(vec![
                ("Skoda Fabia R5".into(), "gravel".into(), "fast".into()),
                ("XSARA".into(), "tarmac".into(), "dry grip".into()),
            ]),
        }
    }

    #[test]
    fn picker_state_is_saved_in_the_players_settings() {
        let state = crate::browser_state::State {
            collection: Some("country:FI".into()),
            query: "forest 50%".into(),
            sort: "newest".into(),
            filter: "2".into(),
            length: "1".into(),
            selected: Some("621".into()),
            scroll: 720.0,
        };
        let settings = Settings {
            stage_browser: state,
            car_browser: crate::browser_state::State {
                collection: Some("class:Group A".into()),
                query: "bmw".into(),
                selected: Some("BMW M3".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let restored = Settings::parse(&settings.to_text());
        assert_eq!(restored.stage_browser, settings.stage_browser);
        assert_eq!(restored.car_browser, settings.car_browser);
    }

    #[test]
    fn settings_round_trip() {
        let settings = full_settings();
        let text = settings.to_text();
        assert!(
            text.ends_with(
                "\n[ffb_cars]\nCOROLLA = 120,120,130\nrsf-24 = 90,100,110\n\n[ffb_load]\nrsf-24 tarmac = 0,12,340\n\n\
                 [camera]\nview = bonnet\n\
                 rsf-24 chase = 0 0.5 -1\n\n[gearbox]\nFord Escort Mk II = dog\n\n\
                 [setup]\ngravel Skoda Fabia R5 = fast\ntarmac XSARA = dry grip\n"
            ),
            "{text}"
        );
        // An older `[ffb] sensitivity_car<id>` reads as `rsf-<id>`, losing to the section's own entry.
        assert_eq!(
            Settings::parse("[ffb]\nsensitivity_car24 = 1,2,3\n[ffb_cars]\nrsf-24 = 90,100,110\n").ffb_cars.cars,
            [("rsf-24".to_owned(), [90, 100, 110])].into()
        );
        // A car without a gearbox is sequential; picking sequential forgets it.
        let mut gearboxes = settings.gearboxes.clone();
        assert_eq!(gearboxes.of("Skoda Fabia R5"), SEQUENTIAL);
        assert!(!gearboxes.of("Ford Escort Mk II").sequential);
        gearboxes.set("Ford Escort Mk II", SEQUENTIAL);
        assert_eq!(gearboxes, Gearboxes::default());
        // A setup is kept per car and surface; the default forgets it.
        let mut setups = settings.setups.clone();
        assert_eq!(setups.of("Skoda Fabia R5", "gravel"), Some("fast"));
        assert_eq!(setups.of("Skoda Fabia R5", "tarmac"), None);
        setups.set("Skoda Fabia R5", "gravel", None);
        setups.set("XSARA", "tarmac", None);
        assert_eq!(setups, SetupChoices::default());
        assert_eq!(Settings::parse(&text), settings);
        assert!(text.contains("map = 54\nrecent_cars = COROLLA|Mitsubishi Lancer Evo IX N4\nrecent_maps = 54,129\nfavourite_cars = XSARA|Mitsubishi Lancer Evo IX N4\nfavourite_maps = 129,54,300\n"));
        assert!(text.contains("fov = 72.5\nchase_fov = 80\nvolume = 0.75\n"), "the game chase's camera page is gone");
        // Any other word is a season row's section, not a preset.
        assert_eq!(Settings::parse("[launcher]\nweather = dry\n").weather, Some(WeatherChoice::Row("dry".into())));
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("[launcher]\nfov = 121\n").fov, None);
        assert_eq!(Settings::parse("[launcher]\nfov = 30\n").fov, Some(30.0));
        assert_eq!(Settings::parse("[launcher]\nchase_fov = 121\n").chase_fov, None);
        assert_eq!(Settings::parse("[launcher]\nchase_fov = 100\n").chase_fov, Some(100.0));
        assert_eq!(Settings::parse("[launcher]\nvolume = 5\n").volume, None);
        assert!(text.contains("volume = 0.75\n"));
        assert!(
            text.contains("engine_volume = 0.5\ncodriver_volume = 1.5\neffects_volume = 0\ntyres_volume = 2\n"),
            "{text}"
        );
        assert_eq!(Settings::parse("[launcher]\nengine_volume = 3\n").engine_volume, None);
        assert_eq!(Settings::parse("[launcher]\ncodriver_volume = loud\n").codriver_volume, None);
        assert_eq!(Settings::parse("[launcher]\neffects_volume = -1\n").effects_volume, None);
        assert_eq!(Settings::parse("[launcher]\ntyres_volume = \n").tyres_volume, None);
        assert!(text.contains("codriver = off\ncodriver_voice = Janne Laahanen EnNu A\ncodriver_style = Numeric.ini\n"));
        assert!(
            text.contains("codriver_ranges = Rbr.ini\ncodriver_pack = luppis\nnotes_pack = tanner-notes\n"),
            "{text}"
        );
        assert!(text.contains("my_pacenotes = off\nhud = off\n"), "{text}");
        let unset = Settings::parse("[launcher]\ncodriver_voice = \nmy_pacenotes = maybe\n");
        assert_eq!((unset.codriver_voice, unset.my_pacenotes), (None, None));
        assert!(text.contains("hud = off\nunits = mph\nfps = on\npace_stack = 2\nwipers = intermittent\n"));
        assert_eq!(Settings::parse("[launcher]\npace_stack = 9\n").pace_stack, None);
        assert!(text.contains("wipers = intermittent\ndials = off\nhud_hide = progress,car\n"), "{text}");
        assert!(
            text.contains("perf_log = on\nlighting = off\nparticles = off\ncabin = off\ncontact = off\ngrass = on\nwindscreen = off\nmarks = on\nmotion_blur = on\n"),
            "{text}"
        );
        assert!(text.contains("start_time = baked\nperf_log = on\n"), "{text}");
        assert_eq!(Settings::parse("[launcher]\nperf_log = maybe\n").perf_log, None);
        assert_eq!(Settings::parse("").perf_log, None);
        assert_eq!(Settings::parse("[launcher]\nstart_time = 16:30\n").start_time, None);
        assert_eq!(
            Settings::parse("[launcher]\nstart_time = golden\n").start_time,
            Some(StartTime::Moment(crate::time_of_day::Moment::Golden))
        );
        assert_eq!(Settings::parse("[launcher]\nstart_time = baked\n").start_time, Some(StartTime::Baked));
        assert_eq!(clock_args(Some(5 * 60)), ["--clock", "05:00"]);
        assert!(clock_args(None).is_empty());
        assert_eq!(Settings::parse("[launcher]\nwipers = fast\n").wipers, None);
        assert!(text.contains("render_scale = 75\nwet_roads = low\ndetail = ultra\n"), "{text}");
        assert_eq!(Settings::parse("[launcher]\ndetail = high\n").detail, Some(Detail::High));
        assert_eq!(Settings::parse("[launcher]\ndetail = max\n").detail, None);
        assert_eq!(Settings::parse("[launcher]\nwet_roads = medium\n").wet_roads, Some(WetRoads::Medium));
        assert_eq!(Settings::parse("[launcher]\nwet_roads = on\n").wet_roads, None);
        assert!(text.contains("present = immediate\nmax_fps = 120\nmirrors = 3\n"), "{text}");
        assert_eq!(Settings::parse("[launcher]\nmax_fps = 100\n").max_fps, None);
        assert_eq!(Settings::parse("[launcher]\nmirrors = 4\n").mirrors, None);
        assert_eq!(Settings::parse("[launcher]\nmirrors = off\n").mirrors, Some(Rate::Off));
        assert!(
            text.contains(
                "triple = on\ntriple_diagonal = 32\ntriple_bezel = 8\ntriple_distance = 65\ntriple_angle = 45\n\
                 triple_monitors = HDMI-1,DP-1,HDMI-2\n"
            ),
            "{text}"
        );
        assert_eq!(Settings::parse("[launcher]\ntriple = maybe\n").triple, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_angle = auto\n").triple_angle, Some(None));
        assert_eq!(Settings::parse("[launcher]\ntriple_angle = 45\n").triple_angle, Some(Some(45.0)));
        assert_eq!(Settings::parse("[launcher]\ntriple_angle = 91\n").triple_angle, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_monitors = HDMI-1,DP-1\n").triple_monitors, None);
        assert_eq!(Settings::parse("[launcher]\nrender_scale = 80\n").render_scale, None);
        assert_eq!(render_scale_args(67), ["--render-scale", "0.67"]);
        assert_eq!(render_scale_args(87), ["--render-scale", "0.87"]);
        assert!(render_scale_args(100).is_empty());
        assert_eq!(Settings::parse("[launcher]\nrender_scale = 0\n").render_scale, None);
        assert!(text.contains("damage = realistic\nauto_gears = on\ngear_guard = off\n"));
        assert_eq!(
            Settings::parse("[launcher]\nweather = random\n").weather,
            Some(WeatherChoice::Preset(Preset::Random))
        );
        // A specific row is its section name, and the wetness and fog overrides round-trip (absent: from weather).
        assert_eq!(
            Settings::parse("[launcher]\nweather = 21O_E_heavyfog_heavycloud\n").weather,
            Some(WeatherChoice::Row("21O_E_heavyfog_heavycloud".into()))
        );
        assert!(
            text.contains("weather = bad\nwetness = wet\nfog = heavy\nclouds = partly\nprecipitation = moderate\n"),
            "{text}"
        );
        assert_eq!(Settings::parse(&text).weather, Some(WeatherChoice::Preset(Preset::Bad)), "{text}");
        assert_eq!((Settings::parse(&text).wetness, Settings::parse(&text).fog), (Some(2), Some(Fog::Heavy)));
        assert_eq!(Settings::parse("[launcher]\nweather = \nwetness = soaked\nfog = mist\n").weather, None);
        assert_eq!(Settings::parse("[launcher]\nwetness = damp\n").wetness, Some(1));
        assert_eq!(Settings::parse("[launcher]\nfog = light\n").fog, Some(Fog::Light));
        assert_eq!(
            (Settings::parse(&text).clouds, Settings::parse(&text).precipitation),
            (Some(Clouds::Partly), Some(Precipitation::Moderate))
        );
        assert_eq!(Settings::parse("[launcher]\nclouds = storm\nprecipitation = hail\n").clouds, None);
        assert_eq!(Settings::parse("[launcher]\nprecipitation = none\n").precipitation, Some(Precipitation::None));
        assert_eq!(Settings::parse("[launcher]\nghost = best\n").ghost, Some(GhostChoice::Best));
        let args = GhostChoice::Best.viewer_args(Path::new("/l"), 54, &Default::default());
        let best = Path::new("/l").join("GhostCars").join("Best_54.ghc");
        assert_eq!(args[..2], ["--best-ghost".to_owned(), best.display().to_string()]);
        assert!(GhostChoice::File("gone_54.ghc".into())
            .viewer_args(Path::new("/l"), 54, &Default::default())
            .is_empty());
        assert_eq!(Settings::parse("[launcher]\ndamage = hard\nclutch_help = yes\n"), Settings::default());
        // The old 1.02 season's keys are no longer read.
        assert_eq!(
            Settings::parse("[launcher]\nchampionship_skill = champion\nchampionship_team = 5\n"),
            Settings::default()
        );
    }

    #[test]
    fn full_settings_golden() {
        // Every key in file order, every section: any move, rename or reword fails here.
        let text = full_settings().to_text();
        assert_eq!(
            text,
            concat!(
                "[launcher]\n",
                "car = COROLLA\n",
                "map = 54\n",
                "recent_cars = COROLLA|Mitsubishi Lancer Evo IX N4\n",
                "recent_maps = 54,129\n",
                "favourite_cars = XSARA|Mitsubishi Lancer Evo IX N4\n",
                "favourite_maps = 129,54,300\n",
                "weather = bad\n",
                "wetness = wet\n",
                "fog = heavy\n",
                "clouds = partly\n",
                "precipitation = moderate\n",
                "wear = worn\n",
                "tyre = 5\n",
                "size = 1920x1080\n",
                "fov = 72.5\n",
                "chase_fov = 80\n",
                "volume = 0.75\n",
                "engine_volume = 0.5\n",
                "codriver_volume = 1.5\n",
                "effects_volume = 0\n",
                "tyres_volume = 2\n",
                "codriver = off\n",
                "codriver_voice = Janne Laahanen EnNu A\n",
                "codriver_style = Numeric.ini\n",
                "codriver_ranges = Rbr.ini\n",
                "codriver_pack = luppis\n",
                "notes_pack = tanner-notes\n",
                "notes_set = \n",
                "my_pacenotes = off\n",
                "hud = off\n",
                "units = mph\n",
                "fps = on\n",
                "pace_stack = 2\n",
                "wipers = intermittent\n",
                "dials = off\n",
                "hud_hide = progress,car\n",
                "callout_distance = 0.5\n",
                "marker_threshold = 12.5\n",
                "damage = realistic\n",
                "auto_gears = on\n",
                "gear_guard = off\n",
                "neutral_lock = on\n",
                "clutch_help = off\n",
                "ghost = me_54.ghc\n",
                "install_folder = /games/Richard Burns Rally\n",
                "ngp_physics = /games/rbr-ngp/physics.rbz\n",
                "ngp_dll = /games/ngp/Plugins/NGP.dll\n",
                "head_gasket = 10\n",
                "physics = experimental\n",
                "tyre_tuning = slide-grip=0.8,snow=1.15\n",
                "render_scale = 75\n",
                "wet_roads = low\n",
                "detail = ultra\n",
                "upscale_textures = on\n",
                "present = immediate\n",
                "max_fps = 120\n",
                "mirrors = 3\n",
                "fullscreen = off\n",
                "triple = on\n",
                "triple_diagonal = 32\n",
                "triple_bezel = 8\n",
                "triple_distance = 65\n",
                "triple_angle = 45\n",
                "triple_monitors = HDMI-1,DP-1,HDMI-2\n",
                "hdr = on\n",
                "start_time = baked\n",
                "perf_log = on\n",
                "lighting = off\n",
                "particles = off\n",
                "cabin = off\n",
                "contact = off\n",
                "grass = on\n",
                "windscreen = off\n",
                "marks = on\n",
                "motion_blur = on\n",
                "\n",
                "[controls]\n",
                "h-shifter = normal\n",
                "clutch threshold = 0.35\n",
                "invert axes = \n",
                "input rate = 500\n",
                "throttle = ArrowUp\n",
                "throttle pad = \n",
                "brake = ArrowDown\n",
                "brake pad = \n",
                "steer left = ArrowLeft\n",
                "steer left pad = \n",
                "steer right = ArrowRight\n",
                "steer right pad = \n",
                "handbrake = Space\n",
                "handbrake pad = \n",
                "clutch = \n",
                "clutch pad = \n",
                "gear up = KeyA\n",
                "gear up pad = \n",
                "gear down = KeyZ\n",
                "gear down pad = \n",
                "restart = KeyR\n",
                "restart pad = \n",
                "call for help = KeyH\n",
                "call for help pad = \n",
                "wipers = KeyI\n",
                "wipers pad = \n",
                "headlights = KeyL\n",
                "headlights pad = \n",
                "ignition = KeyE\n",
                "ignition pad = \n",
                "reverse = \n",
                "reverse pad = \n",
                "neutral = \n",
                "neutral pad = \n",
                "gear 1 = \n",
                "gear 1 pad = \n",
                "gear 2 = \n",
                "gear 2 pad = \n",
                "gear 3 = \n",
                "gear 3 pad = \n",
                "gear 4 = \n",
                "gear 4 pad = \n",
                "gear 5 = \n",
                "gear 5 pad = \n",
                "gear 6 = \n",
                "gear 6 pad = \n",
                "look back = KeyB\n",
                "look back pad = \n",
                "camera = KeyC\n",
                "camera pad = \n",
                "pause = Escape\n",
                "pause pad = \n",
                "gauge mode = End\n",
                "gauge mode pad = \n",
                "\n",
                "[ffb]\n",
                "enabled = 1\n",
                "strength = 1\n",
                "inverse = 0\n",
                "dead_zone = 0\n",
                "dead_zone_threshold = 0\n",
                "rate = 500\n",
                "device = \n",
                "sensitivity_gravel = 100\n",
                "sensitivity_tarmac = 100\n",
                "sensitivity_snow = 100\n",
                "rotation = \n",
                "rotation_per_car = 0\n",
                "\n",
                "[ffb_cars]\n",
                "COROLLA = 120,120,130\n",
                "rsf-24 = 90,100,110\n",
                "\n",
                "[ffb_load]\n",
                "rsf-24 tarmac = 0,12,340\n",
                "\n",
                "[camera]\n",
                "view = bonnet\n",
                "rsf-24 chase = 0 0.5 -1\n",
                "\n",
                "[gearbox]\n",
                "Ford Escort Mk II = dog\n",
                "\n",
                "[setup]\n",
                "gravel Skoda Fabia R5 = fast\n",
                "tarmac XSARA = dry grip\n"
            )
        );
    }

    #[test]
    fn settings_parse_edges() {
        // Out-of-range numbers keep the default; the edges parse.
        assert_eq!(Settings::parse("[launcher]\nfov = 121\n").fov, None);
        assert_eq!(Settings::parse("[launcher]\nfov = 29.9\n").fov, None);
        assert_eq!(Settings::parse("[launcher]\nfov = 30\n").fov, Some(30.0));
        assert_eq!(Settings::parse("[launcher]\nfov = 120\n").fov, Some(120.0));
        assert_eq!(Settings::parse("[launcher]\nfov = wide\n").fov, None);
        assert_eq!(Settings::parse("[launcher]\nvolume = 5\n").volume, None);
        assert_eq!(Settings::parse("[launcher]\nvolume = -0.1\n").volume, None);
        assert_eq!(Settings::parse("[launcher]\nvolume = 4\n").volume, Some(4.0));
        assert_eq!(Settings::parse("[launcher]\nengine_volume = 2\n").engine_volume, Some(2.0));
        assert_eq!(Settings::parse("[launcher]\nengine_volume = 2.1\n").engine_volume, None);
        assert_eq!(Settings::parse("[launcher]\npace_stack = 4\n").pace_stack, Some(4));
        // The launcher's `hide_wheel` of before becomes the cockpit's `wheel = off` and is not written again.
        let hidden = Settings::parse("[launcher]\nhide_wheel = on\n");
        assert!(!hidden.camera.cockpit.wheel);
        let text = hidden.to_text();
        assert!(text.contains("[camera]\nwheel = off\n") && !text.contains("hide_wheel"), "{text}");
        assert!(Settings::parse("[launcher]\nhide_wheel = off\n").camera.cockpit.wheel);
        assert_eq!(Settings::parse("[launcher]\npace_stack = 8\n").pace_stack, Some(8));
        assert_eq!(Settings::parse("[launcher]\npace_stack = 9\n").pace_stack, None);
        assert_eq!(Settings::parse("[launcher]\nrender_scale = 50\n").render_scale, Some(50));
        assert_eq!(Settings::parse("[launcher]\nrender_scale = 87\n").render_scale, Some(87));
        assert_eq!(Settings::parse("[launcher]\nrender_scale = 80\n").render_scale, None);
        assert_eq!(Settings::parse("[launcher]\nhead_gasket = 0\n").head_gasket, None);
        assert_eq!(Settings::parse("[launcher]\nhead_gasket = -2\n").head_gasket, None);
        assert_eq!(Settings::parse("[launcher]\nhead_gasket = 1\n").head_gasket, Some(1));
        assert_eq!(Settings::parse("[launcher]\nphysics = ngp\n").physics, Some(PhysicsProfile::Ngp));
        assert_eq!(Settings::parse("[launcher]\nphysics = turbo\n").physics, None);
        assert_eq!(Settings::parse("[launcher]\ntyre = 6\n").tyre, Some(TyreChoice::Fixed(6)));
        assert_eq!(Settings::parse("[launcher]\ntyre = 7\n").tyre, None);
        assert_eq!(Settings::parse("[launcher]\nmap = abc\n").map, None);
        assert_eq!(Settings::parse("[launcher]\ncallout_distance = -1.5\n").callout_distance, Some(-1.5));
        assert_eq!(Settings::parse("[launcher]\ncallout_distance = 15\n").callout_distance, Some(15.0));
        // Unknown choice words keep the default.
        assert_eq!(Settings::parse("[launcher]\nfog = mist\n").fog, None);
        assert_eq!(Settings::parse("[launcher]\nclouds = storm\n").clouds, None);
        assert_eq!(Settings::parse("[launcher]\nprecipitation = hail\n").precipitation, None);
        assert_eq!(Settings::parse("[launcher]\nwetness = soaked\n").wetness, None);
        assert_eq!(Settings::parse("[launcher]\ndamage = hard\n").damage, None);
        assert_eq!(Settings::parse("[launcher]\ndetail = max\n").detail, None);
        assert_eq!(Settings::parse("[launcher]\nwet_roads = on\n").wet_roads, None);
        assert_eq!(Settings::parse("[launcher]\nmirrors = 4\n").mirrors, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_diagonal = 9\n").triple_diagonal, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_diagonal = 10\n").triple_diagonal, Some(10.0));
        assert_eq!(Settings::parse("[launcher]\ntriple_diagonal = 65\n").triple_diagonal, Some(65.0));
        assert_eq!(Settings::parse("[launcher]\ntriple_diagonal = 66\n").triple_diagonal, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_bezel = 60\n").triple_bezel, Some(60.0));
        assert_eq!(Settings::parse("[launcher]\ntriple_bezel = 61\n").triple_bezel, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_distance = 29\n").triple_distance, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_distance = 200\n").triple_distance, Some(200.0));
        assert_eq!(Settings::parse("[launcher]\ntriple_angle = 0\n").triple_angle, Some(Some(0.0)));
        assert_eq!(Settings::parse("[launcher]\ntriple_angle = 90\n").triple_angle, Some(Some(90.0)));
        assert_eq!(Settings::parse("[launcher]\ntriple_angle = wide\n").triple_angle, None);
        assert_eq!(
            Settings::parse("[launcher]\ntriple_monitors = A,B,C\n").triple_monitors,
            Some(["A".into(), "B".into(), "C".into()])
        );
        assert_eq!(Settings::parse("[launcher]\ntriple_monitors = A,B,C,D\n").triple_monitors, None);
        assert_eq!(Settings::parse("[launcher]\ntriple_monitors = A,,C\n").triple_monitors, None);
        assert_eq!(Settings::parse("[launcher]\nwipers = fast\n").wipers, None);
        // The wipers, the presentation and the mirrors parse case-insensitively.
        assert_eq!(Settings::parse("[launcher]\nwipers = INTERMITTENT\n").wipers, Some(Switch::Intermittent));
        assert_eq!(Settings::parse("[launcher]\npresent = FIFO\n").present, Some(Present::Fifo));
        assert_eq!(Settings::parse("[launcher]\nmirrors = off\n").mirrors, Some(Rate::Off));
        // Missing keys keep the default; an empty file is the default settings.
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("[launcher]\n"), Settings::default());
        assert_eq!(Settings::parse("[launcher]\ncar = \n").car, Some(String::new()));
        // A saved `rsf_root` from before the one game folder is ignored on read and dropped on save.
        assert_eq!(Settings::parse("[launcher]\nrsf_root = /games/rsf\n"), Settings::default());
        assert!(!Settings::parse("[launcher]\nrsf_root = /games/rsf\n").to_text().contains("rsf_root"));
        // The units word is `mph`/`kmh`, not the field's name.
        assert_eq!(Settings::parse("[launcher]\nunits = mph\n").mph, Some(true));
        assert_eq!(Settings::parse("[launcher]\nunits = kmh\n").mph, Some(false));
        assert_eq!(Settings::parse("[launcher]\nunits = kph\n").mph, None);
        // Any ghost word parses: empty and `off` are none, anything else a file.
        assert_eq!(Settings::parse("[launcher]\n").ghost, None);
        assert_eq!(Settings::parse("[launcher]\nghost = \n").ghost, Some(GhostChoice::Off));
        assert_eq!(Settings::parse("[launcher]\nghost = off\n").ghost, Some(GhostChoice::Off));
        assert_eq!(
            Settings::parse("[launcher]\nghost = me_54.ghc\n").ghost,
            Some(GhostChoice::File("me_54.ghc".into()))
        );
        // Empty strings are dropped, but the notes set and the NGP DLL keep an empty choice.
        assert_eq!(Settings::parse("[launcher]\ncodriver_voice = \n").codriver_voice, None);
        assert_eq!(Settings::parse("[launcher]\ninstall_folder = \n").install_folder, None);
        assert_eq!(Settings::parse("[launcher]\nnotes_set = \n").notes_set, Some(String::new()));
        // Empty lists are empty; blanks and words that do not parse are skipped.
        assert_eq!(Settings::parse("[launcher]\nrecent_cars = \n").recent_cars, Vec::<String>::new());
        assert_eq!(
            Settings::parse("[launcher]\nrecent_cars = A|| B |\n").recent_cars,
            ["A".to_owned(), "B".to_owned()]
        );
        assert_eq!(Settings::parse("[launcher]\nrecent_maps = \n").recent_maps, Vec::<u32>::new());
        assert_eq!(Settings::parse("[launcher]\nrecent_maps = 54,,abc,129\n").recent_maps, [54, 129]);
        // An empty hidden list reads as all shown, which the writer omits.
        assert_eq!(Settings::parse("[launcher]\nhud_hide = \n").hud_hide, Some(Parts::default()));
        assert!(!Settings::parse("[launcher]\nhud_hide = \n").to_text().contains("hud_hide"));
        // The wear and the size: unknown words keep the default, and the writer clamps the wear.
        assert_eq!(Settings::parse("[launcher]\nwear = worn\n").wear, Some(2));
        assert_eq!(Settings::parse("[launcher]\nwear = old\n").wear, None);
        assert!(Settings { wear: Some(9), ..Default::default() }.to_text().contains("wear = worn\n"));
        assert_eq!(Settings::parse("[launcher]\nsize = 1920x1080\n").size, Some((1920, 1080)));
        assert_eq!(Settings::parse("[launcher]\nsize = 1920\n").size, None);
        // The weather: empty keeps the default, any other unknown word is a season row.
        assert_eq!(Settings::parse("[launcher]\nweather = \n").weather, None);
        assert_eq!(Settings::parse("[launcher]\nweather = dry\n").weather, Some(WeatherChoice::Row("dry".into())));
    }
}
