//! The viewer command Start runs: the selection resolved to a stage and weather, the viewer's and
//! `rbr-standalone`'s arguments, and the viewer as a child process.

use std::path::{Path, PathBuf};
use std::process::Command;

use rbr_formats::pacenote::codriver;
use rbr_formats::rsf::stage_list;
use rbr_formats::saves::SaveDir;
use rbr_gameplay::crt::CrtRand;
use rbr_gameplay::markers::MarkerSettings;
use rbr_gameplay::race_options::RaceOptions;
use rbr_gameplay::weather::{self, Draws, Preset, Weather};
use rbr_render::glass::model::Switch;
use rbr_render::renderer::{Detail, Enhance, Reflections};
use rbr_render::shifter::CarBox;
use rbr_school::Lesson;
use rbr_sound::mixer::Volumes;

use crate::catalog::{self, Car, Stage, TyreChoice};
use crate::graphics_page::{Graphics, HudChoice, WetRoads, ALWAYS_ON};
use crate::settings::{clock_text, wetness_name, Clouds, Fog, Precipitation};
use crate::{pages::Choice, time_of_day, Form};

/// The viewer's `--clock` arguments for a start time, none for the stage's own light.
pub fn clock_args(start: Option<u32>) -> Vec<String> {
    start.map_or_else(Vec::new, |minutes| vec!["--clock".into(), clock_text(minutes)])
}

/// The Environment's extra arguments: the viewer's `--sky` (the row's exact `TrackSettings.ini` section) and
/// `--fog`, `--clouds` and `--precipitation` (ours, still editable with a ghost), and `rbr-standalone`'s `--wetness` (the override; a ghost keeps its
/// own wetness). The viewer's `--weather` word and the auto tyre already follow the override through the weather.
///
/// The first list goes to the viewer, the second to `rbr-standalone`.
pub fn environment_args(
    section: Option<String>,
    fog: Option<Fog>,
    clouds: Option<Clouds>,
    precipitation: Option<Precipitation>,
    wetness: Option<u32>,
    ghosted: bool,
) -> (Vec<String>, Vec<String>) {
    let mut viewer = Vec::new();
    if let Some(section) = section {
        viewer.extend(["--sky".to_owned(), section]);
    }
    if let Some(fog) = fog {
        viewer.extend(["--fog".to_owned(), fog.name().to_owned()]);
    }
    if let Some(clouds) = clouds {
        viewer.extend(["--clouds".to_owned(), clouds.name().to_owned()]);
    }
    if let Some(precipitation) = precipitation {
        viewer.extend(["--precipitation".to_owned(), precipitation.name().to_owned()]);
    }
    let mut standalone = Vec::new();
    if !ghosted {
        if let Some(wetness) = wetness {
            standalone.extend(["--wetness".to_owned(), wetness_name(wetness).to_owned()]);
        }
    }
    (viewer, standalone)
}

/// The viewer's `--perf-log` into `dir`, when the form has the performance log on.
fn perf_log_args(on: bool, dir: &Path) -> Vec<String> {
    if on {
        vec!["--perf-log".to_owned(), dir.display().to_string()]
    } else {
        Vec::new()
    }
}

/// The viewer's `--render-scale` arguments for `percent`, none at full size.
pub fn render_scale_args(percent: u32) -> Vec<String> {
    match percent {
        100 => Vec::new(),
        percent => vec!["--render-scale".into(), (f64::from(percent) / 100.0).to_string()],
    }
}

/// The `viewer --enhance` list of the additions `graphics` turn on, and those always on.
pub fn enhance_list(graphics: &Graphics) -> String {
    let on = |name: &str| match name {
        "cabin" => graphics.cabin,
        "contact" => graphics.contact,
        "grass" => graphics.grass,
        "windscreen" => graphics.windscreen,
        "lighting" => graphics.lighting,
        "marks" => graphics.marks,
        "particles" => graphics.particles,
        "wet" => graphics.wet_roads != WetRoads::Off,
        name => ALWAYS_ON.contains(&name),
    };
    Enhance::NAMES.into_iter().filter(|name| on(name)).collect::<Vec<_>>().join(",")
}

/// The Pacenote plugin in play: its folder and the voices, styles and range sets it offers.
#[derive(Debug, Clone, PartialEq)]
pub struct CodriverMods {
    pub dir: PathBuf,
    pub voices: Vec<String>,
    pub styles: Vec<String>,
    pub ranges: Vec<String>,
    /// A locally owned pack's default voice (`PaceNote.ini`'s, else its first discovered voice): what an unset
    /// choice plays, so packs without a `PaceNote.ini` still speak. None for the RSF install, whose unset choice
    /// stays `PaceNote.ini`'s.
    pub default_voice: Option<String>,
}

/// Whether the installed voice `have` is the configured one `wanted`: case-insensitively, with `\` and `/`
/// meaning the same folder, so a stale-cased `PaceNote.ini` still resolves to the installed spelling.
fn voices_equal(have: &str, wanted: &str) -> bool {
    let normal = |v: &str| v.replace('\\', "/").to_ascii_lowercase();
    normal(have) == normal(wanted)
}

impl CodriverMods {
    /// The plugin of the game folder `root`, or `None` without one.
    pub fn scan(root: &str) -> Option<Self> {
        let dir = codriver::plugin_dir(Path::new(root.trim()))?;
        let (voices, styles, ranges) = (codriver::voices(&dir), codriver::styles(&dir), codriver::range_sets(&dir));
        Some(Self { dir, voices, styles, ranges, default_voice: None })
    }

    /// The plugin in the folder `dir` itself (a locally owned pack): its voices, styles and range sets. The
    /// default voice is the installed voice matching `PaceNote.ini`'s (case-insensitively, `\` or `/`), else
    /// the first installed voice: a stale or foreign setting never names a voice that is not there.
    pub fn from_dir(dir: PathBuf) -> Option<Self> {
        let (voices, styles, ranges) = (codriver::voices(&dir), codriver::styles(&dir), codriver::range_sets(&dir));
        let configured = codriver::settings(&dir).voice;
        let default_voice = match configured {
            Some(wanted) => {
                voices.iter().find(|v| voices_equal(v, &wanted)).cloned().or_else(|| voices.first().cloned())
            }
            None => voices.first().cloned(),
        };
        Some(Self { dir, voices, styles, ranges, default_voice })
    }

    /// The voice `choice` names: the plugin's default when unset, none for the original 1.02 co-driver
    /// ([`ORIGINAL_VOICE`]).
    pub fn voice(&self, choice: Option<&str>) -> Option<String> {
        match choice {
            Some(ORIGINAL_VOICE) => None,
            Some(voice) => Some(voice.to_owned()),
            None => self.default_voice.clone().or_else(|| codriver::settings(&self.dir).voice),
        }
    }
}

/// The voice choice for the original 1.02 co-driver.
pub const ORIGINAL_VOICE: &str = "1.02";

/// The viewer's co-driver mod arguments without the stage notes: the plugin with the voice, style and range set.
pub fn codriver_base_args(mods: &CodriverMods, voice: Option<&str>, style: &str, ranges: &str) -> Vec<String> {
    let Some(voice) = mods.voice(voice) else { return Vec::new() };
    ["--codriver-mod", &mods.dir.display().to_string()]
        .into_iter()
        .chain(["--codriver-voice", &voice, "--codriver-style", style, "--codriver-ranges", ranges])
        .map(str::to_owned)
        .collect()
}

/// The viewer command Start runs: the selection resolved to a stage and weather, and every argument the viewer
/// and `rbr-standalone` take for a boot launch.
pub struct ViewerLaunch {
    pub stage: Stage,
    pub car: Car,
    pub options: RaceOptions,
    pub weather: Weather,
    pub tyre: u32,
    pub size: (u32, u32),
    pub fov: f32,
    /// The chase view's field of view, degrees (`viewer --chase-fov`).
    pub chase_fov: f32,
    pub volume: f32,
    pub volumes: Volumes,
    pub codriver: bool,
    pub hud: HudChoice,
    pub graphics: Graphics,
    pub wipers: Switch,
    pub standalone: PathBuf,
    pub data: PathBuf,
    pub marker: MarkerSettings,
    pub bindings: Option<PathBuf>,
    pub setup: Option<PathBuf>,
    /// A rally school lesson (`--school N`): the lesson's tyre, and with car 5 its setup; without `--tyre`,
    /// `--setup`, `--surface`, `--championship` and `--season-row`.
    pub school: Option<Lesson>,
}

#[cfg(test)]
impl Default for ViewerLaunch {
    /// Filler for the tests' struct update syntax.
    fn default() -> Self {
        Self {
            stage: Stage::default(),
            car: Car::default(),
            options: RaceOptions::default(),
            weather: Weather {
                map: 0,
                row: None,
                letter: 0,
                cloud: 0,
                cloud_letter: 0,
                preset: 0,
                wetness: 0,
                wear: 0,
            },
            tyre: 0,
            size: (0, 0),
            fov: 0.0,
            chase_fov: 0.0,
            volume: 0.0,
            volumes: Volumes::default(),
            codriver: false,
            hud: HudChoice::default(),
            graphics: Graphics::default(),
            wipers: Switch::default(),
            standalone: PathBuf::new(),
            data: PathBuf::new(),
            marker: MarkerSettings::default(),
            bindings: None,
            setup: None,
            school: None,
        }
    }
}

impl ViewerLaunch {
    /// The argument vector: the viewer's flags, then `rbr-standalone`'s after the second `--live`.
    pub fn args(&self) -> Vec<String> {
        let ViewerLaunch {
            stage,
            car,
            options,
            weather,
            tyre,
            size,
            fov,
            chase_fov,
            volume,
            volumes,
            codriver,
            hud,
            graphics,
            wipers,
            standalone,
            data,
            marker,
            bindings,
            setup,
            school,
        } = self;
        // The folder the viewer loads: an archive car's own, a pack car's model folder; then the flags resolving
        // a pack car, empty for an archive car.
        let folder = match &car.physics {
            catalog::Physics::Pack(_) => car
                .model
                .as_ref()
                .ok()
                .and_then(|m| m.entry.file_name.split(['\\', '/']).rev().nth(1))
                .unwrap_or(&car.name)
                .to_owned(),
            catalog::Physics::Archive => car.name.clone(),
        };
        let mut args: Vec<String> = vec![stage.name.clone(), "--car".into(), folder];
        args.extend(pack_car_args(car));
        args.extend(vec![
            "--weather".into(),
            weather.mat_words().1.into(),
            "--size".into(),
            format!("{}x{}", size.0, size.1),
            "--fov".into(),
            fov.to_string(),
            "--chase-fov".into(),
            chase_fov.to_string(),
            "--volume".into(),
            volume.to_string(),
            "--volumes".into(),
            format!("{},{},{},{}", volumes.engine, volumes.codriver, volumes.effects, volumes.tyres),
        ]);
        if !codriver {
            args.push("--no-codriver".into());
        }
        if !hud.show {
            args.push("--no-hud".into());
        }
        if hud.mph {
            args.push("--mph".into());
        }
        args.extend(["--wipers".into(), wipers.name().into()]);
        if hud.fps {
            args.push("--fps".into());
        }
        if hud.stack > 0 {
            args.extend(["--pace-stack".into(), hud.stack.to_string()]);
        }
        if !hud.dials {
            args.push("--no-dials".into());
        }
        if hud.hide_wheel {
            args.push("--hide-wheel".into());
        }
        if let Some(list) = hud.parts.hidden() {
            args.extend(["--hud-hide".into(), list]);
        }
        if let Some(path) = bindings {
            let path = path.display().to_string();
            args.extend(["--bindings".into(), path.clone(), "--cameras".into(), path]);
        }
        args.extend(["--enhance".into(), enhance_list(graphics)]);
        args.extend([
            "--live".into(),
            standalone.display().to_string(),
            "--live".into(),
            "--boot".into(),
            "--weather".into(),
            options.weather.id().to_string(),
            "--wear".into(),
            catalog::WEARS[weather.wear.min(2) as usize].into(),
        ]);
        if school.is_none() {
            args.extend(["--tyre".into(), tyre.to_string()]);
        }
        args.extend(["--stage".into(), stage.name.clone()]);
        if matches!(car.physics, catalog::Physics::Archive) {
            args.extend(["--car".into(), car.slot.to_string()]);
        }
        if let Some(lesson) = school {
            args.extend(["--school".into(), lesson.index().to_string()]);
        }
        args.extend([
            "--data".into(),
            data.display().to_string(),
            "--callout-distance".into(),
            marker.callout_distance.to_string(),
            "--marker-threshold".into(),
            marker.marker_threshold.to_string(),
        ]);
        if school.is_none() {
            if let (Preset::Random, Some(row)) = (options.weather, weather.row) {
                args.extend(["--season-row".into(), row.to_string()]);
            }
        }
        let on_off = |on: bool| if on { "on" } else { "off" }.to_owned();
        let aids = options.aids;
        args.extend([
            "--damage".into(),
            options.damage.name().into(),
            "--auto-gears".into(),
            on_off(aids.auto_gears),
            "--gear-guard".into(),
            on_off(aids.gear_guard),
            "--neutral-lock".into(),
            on_off(aids.neutral_lock),
            "--clutch-help".into(),
            on_off(aids.clutch_help),
        ]);
        if let (None, Some(path)) = (school, setup) {
            args.extend(["--setup".into(), path.display().to_string()]);
        }
        args.extend(pack_car_args(car));
        args
    }
}

/// The flags resolving a pack car in the viewer, `rbr-standalone` and `rbr-sound`: its pack, its model's `.sgc`
/// path, its bank and its saved key ([`car_pack::Flags`]). Empty for an archive car.
pub fn pack_car_args(car: &Car) -> Vec<String> {
    let catalog::Physics::Pack(pack) = &car.physics else { return Vec::new() };
    rbr_formats::car_pack::Flags {
        pack: Some(pack.clone()),
        model: car.model_path.clone(),
        bank: car.bank.clone(),
        key: Some(car.key.clone()),
    }
    .argv()
}

/// The viewer's `--save-replay` for a live start: the drive's replay in `replays/` of the player's folder `home`,
/// named by the start's nanoseconds and the stage. The stepping process appends its frames to it
/// (`rbr-standalone --replay-out`), and `viewer --replay` watches it again.
// No date crate in the workspace: the name carries seconds since the epoch, which sort it with the others.
pub fn replay_args(home: &Path, stage: &str) -> Vec<String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    // A community stage is its folder's path: the replay takes the file name.
    let name = Path::new(stage).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| stage.into());
    let stage: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let path = home.join("replays").join(format!("{stamp}-{stage}.rrp"));
    vec!["--save-replay".to_owned(), path.display().to_string()]
}

/// The viewer's arguments for the advanced H-shifter: the car's gearbox (`--car-gearbox`) and the grind from
/// RBRControls' folder in the game folder `root`, `Shifter_Error.wav` (or the `.wav.sample` RBRControls ships it
/// as; `--grind-sound`).
pub fn shifter_args(gearbox: CarBox, root: &Path) -> Vec<String> {
    let mut args = vec!["--car-gearbox".into(), gearbox.arg().into()];
    if root.as_os_str().is_empty() {
        return args;
    }
    let dir = root.join("Plugins").join("RBRControls");
    let grind =
        ["Shifter_Error.wav", "Shifter_Error.wav.sample"].map(|n| dir.join(n)).into_iter().find(|p| p.is_file());
    if let Some(wav) = grind {
        args.extend(["--grind-sound".into(), wav.display().to_string()]);
    }
    args
}

/// What a launch drives: the car, its tyre and setup (`None` leaves `--tyre` and `--setup` out), and the rally
/// school lesson (`--school N`: the lesson's tyre, and with car 5 its setup).
struct LaunchCar {
    car: catalog::Car,
    tyre: Option<u32>,
    setup: Option<PathBuf>,
    school: Option<Lesson>,
}

impl Form {
    /// Resolves the current preset into its stage variant and weather row: the random weather's from the next draw of
    /// the stream (`rand`), without taking it.
    pub fn selection(&self) -> Result<(catalog::Stage, Weather), String> {
        self.selection_from(&mut self.rand.clone()).map(|(stage, weather, _)| (stage, weather))
    }

    /// Resolves the Environment choices into the stage, the weather and the preset the launch runs as: a specific
    /// row runs like Random with that row (the chosen wear set after), and the Custom weather's wetness replaces its
    /// base's. A saved ghost brings its own weather, and the launch keeps the user's preset.
    fn selection_from(&self, rand: &mut CrtRand) -> Result<(catalog::Stage, Weather, Preset), String> {
        let map = self.catalog.maps[self.map].number;
        if let Some((stage, weather)) = self.ghost_weather() {
            return Ok((stage, weather, self.options.weather));
        }
        let season = self.catalog.maps[self.map].season(&self.season);
        let row = self.weather_row.as_deref().and_then(|s| catalog::row_by_section(&season, map, s));
        let (preset, draws, specific) = match row.filter(|_| self.clouds.is_none()) {
            // The Custom weather: our sky over the stage's good weather, or its bad one on a damp or wet road
            // (its light and textures made for the wet), with the chosen wear.
            None if self.clouds.is_some() => (self.custom_base(), Draws { row: None, wear: self.wear }, true),
            Some(k) => (Preset::Random, Draws { row: Some(k), wear: 1 }, true),
            None if self.options.weather == Preset::Random => (
                Preset::Random,
                weather::random_draws(&season, map, rand)
                    .ok_or(format!("map {map} has no season row (the original stops at its int3)"))?,
                false,
            ),
            None => (self.options.weather, Draws { row: None, wear: self.wear }, false),
        };
        let (stage, mut weather) = self.catalog.stage_weather(&self.season, map, preset, draws)?;
        if specific {
            // A specific row takes the chosen wear (`Draws` takes the random weather's 1 or 2).
            weather.wear = self.wear;
        }
        if self.custom() {
            if let Some(wetness) = self.wetness {
                weather.wetness = wetness.min(2);
            }
        }
        Ok((stage, weather, preset))
    }

    /// Whether the weather is Custom (a cloud cover chosen): ours over the stage, its fog, rain, wetness and time of
    /// day too; else the game's variant as authored, at the hour it was baked at.
    pub(crate) fn custom(&self) -> bool {
        self.clouds.is_some()
    }

    /// The Custom weather's base: the stage's good weather on a dry road, its bad weather on a damp or wet one.
    pub(crate) fn custom_base(&self) -> Preset {
        if self.wetness.unwrap_or(0) >= 1 {
            Preset::Bad
        } else {
            Preset::Good
        }
    }

    pub fn tyre_id(&self, weather: &Weather) -> Result<u32, String> {
        match self.tyre {
            TyreChoice::Auto => weather::tyre_kind(self.catalog.maps[self.map].surface(), weather.wetness)
                .ok_or_else(|| "the map surface and wetness have no tyre mapping".into()),
            TyreChoice::Fixed(id) if id <= 6 => Ok(id),
            TyreChoice::Fixed(id) => Err(format!("tyre {id}: expected 0 through 6")),
        }
    }

    pub fn start(&mut self) {
        let mut rand = self.rand;
        if let Some(args) = self.start_args(&mut rand) {
            if self.spawn(args) {
                self.rand = rand;
            }
        }
    }

    /// The viewer's arguments for the quick race Start runs, drawing the random weather from `rand`, with the
    /// Environment's sky, fog and wetness.
    pub fn start_args(&mut self, rand: &mut CrtRand) -> Option<Vec<String>> {
        let (stage, weather, preset) = self.selection_from(rand).ok()?;
        // Only the quick race races and keeps a ghost (`rbr_gameplay::ghost::clears`: the championship clears it).
        let words = weather.ghost_words(&self.season);
        let ghost = self.ghost.viewer_args(&self.records(), stage.map, &words);
        // The quick race keeps the stage record and the personal best in the game's save files of the player's folder.
        let saves = ["--saves".to_owned(), SaveDir::new(&self.records()).path.display().to_string()];
        let mut extra = [saves.as_slice(), &["--profile".to_owned(), self.championship.profile.clone()]].concat();
        // The Custom weather's Time of day is the quick race's: the stage is drawn at it and the start board shows
        // it. The Stage weather and a championship stage keep the stage's own light and the season's start clock.
        let custom = self.custom();
        let site = self.catalog.map(stage.map).map(time_of_day::site);
        let start_time = site.and_then(|site| self.start_time.minutes(site)).filter(|_| custom);
        if let Some(m) = start_time {
            extra.extend(["--start-time".to_owned(), format!("{}:{:02}", m / 60, m % 60)]);
        }
        let viewer_extra = [ghost, clock_args(start_time)].concat();
        // The Environment: the exact sky of the row or the Custom weather's base, and the Custom weather's fog, clouds,
        // rain and wetness (a ghost keeps its own wetness).
        let (sky_fog, wetness_args) = environment_args(
            weather.section(&self.catalog.maps[self.map].season(&self.season)),
            self.fog.filter(|_| custom),
            self.clouds,
            self.precipitation.filter(|_| custom),
            self.wetness.filter(|_| custom),
            self.ghost_weather().is_some(),
        );
        let viewer_extra = [viewer_extra, sky_fog].concat();
        extra.extend(wetness_args);
        let options = RaceOptions { weather: preset, ..self.options };
        // The quick race drives with the Drive page's setup (read again, a championship may have put its own).
        self.select_setup();
        self.setup.drive_chosen();
        self.launch_args(stage, options, weather, extra, viewer_extra)
    }

    /// The viewer's arguments for `stage`; `extra` goes to `rbr-standalone`, `viewer_extra` to the viewer. `None`, with
    /// the status saying why, when it cannot start.
    pub(crate) fn launch_args(
        &mut self,
        stage: catalog::Stage,
        options: RaceOptions,
        weather: Weather,
        extra: Vec<String>,
        viewer_extra: Vec<String>,
    ) -> Option<Vec<String>> {
        let (stage, extra) = self.btb_launch(stage, extra)?;
        if let Some(problem) = self.ngp_dll.problem(&self.data, &self.game_root()) {
            self.status = problem;
            return None;
        }
        let tyre = self.tyre_id(&weather).ok()?;
        let Some(car) = self.car().cloned() else {
            self.status = "no car: the game folder has none NGP drives".into();
            return None;
        };
        self.select_setup();
        let setup = match self.setup.current_file(&self.here()) {
            Ok(setup) => setup,
            Err(e) => {
                self.status = format!("setup: {e}");
                return None;
            }
        };
        self.launch_with(
            stage,
            LaunchCar { car, tyre: Some(tyre), setup, school: None },
            options,
            weather,
            extra,
            viewer_extra,
        )
    }

    /// Starts a rally school lesson like a quick race: `lesson` on its map's `M` variant on good weather with the
    /// lesson's tyre, in the Drive page's car (`rbr-standalone --school N`, with `--car` or `--car-pack`); without
    /// the flags `--school` refuses (`--tyre`, `--setup`, `--surface`, `--championship`). The result goes to the
    /// profile's saves (`--saves`, `--profile`).
    pub fn start_school(&mut self, lesson: Lesson) {
        let map = lesson.map();
        // Like `rbr-standalone --school` without `--weather`: the good weather, new wear.
        let draws = Draws { row: None, wear: 0 };
        match self.catalog.stage_weather(&self.season, map, Preset::Good, draws) {
            Ok((stage, weather)) => {
                let Some(car) = self.car().cloned() else {
                    self.status = "no car on the Drive page".into();
                    return;
                };
                let mut extra = vec!["--school".to_owned(), lesson.index().to_string()];
                let saves = SaveDir::new(&self.records()).path.display().to_string();
                extra.extend(["--saves".to_owned(), saves, "--profile".to_owned(), self.championship.profile.clone()]);
                let options = RaceOptions { weather: Preset::Good, ..self.options };
                if let Some(args) = self.launch_with(
                    stage,
                    LaunchCar { car, tyre: None, setup: None, school: Some(lesson) },
                    options,
                    weather,
                    extra,
                    Vec::new(),
                ) {
                    self.spawn(args);
                }
            }
            Err(e) => self.status = e,
        }
    }

    /// The viewer command for `stage`: the shifter and co-driver arguments, the bindings, the setup and every
    /// viewer and `rbr-standalone` argument around them ([`LaunchCar`]: what the launch drives).
    fn launch_with(
        &mut self,
        stage: catalog::Stage,
        launch: LaunchCar,
        mut options: RaceOptions,
        weather: Weather,
        extra: Vec<String>,
        mut viewer_extra: Vec<String>,
    ) -> Option<Vec<String>> {
        let LaunchCar { car, tyre, setup, school } = launch;
        let car = &car;
        if self.bindings.shifter.advanced {
            let gearbox = self.gearboxes.of(&car.name);
            // Clutch help ignores the clutch pedal the advanced H-shifter needs; the lever, not the game, picks the
            // gear. A sequential car keeps them: the viewer leaves its shifter off.
            if !gearbox.sequential {
                (options.aids.clutch_help, options.aids.auto_gears) = (false, false);
            }
            viewer_extra.extend(shifter_args(gearbox, Path::new(&self.game_root())));
        }
        if let Some(mods) = self.codriver_mods.as_ref().filter(|_| self.codriver) {
            // The stage's names for the note lookup: RSF's aliases where there are any, then the launcher's own
            // catalogue names, so the lookup works without an RSF stage list.
            let names = self.stage_note_names(&stage);
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let mut args =
                codriver_base_args(mods, self.codriver_voice.as_deref(), &self.codriver_style, &self.codriver_ranges);
            // The chosen note set's file for the stage (the local pack's or the RSF install's MyPacenotes).
            if let Some(file) = self.resolve_notes(&refs).filter(|_| !args.is_empty()) {
                args.extend(["--pacenotes".into(), file.display().to_string()]);
            }
            viewer_extra.extend(args);
        }
        // Read-only, the file already holds the selection and the bindings, when there is one.
        let saved = self.save() || self.read_only && self.paths.settings.is_file();
        let bindings = saved.then_some(self.paths.settings.as_path());
        let mut args = ViewerLaunch {
            stage: stage.clone(),
            car: car.clone(),
            options,
            weather,
            tyre: tyre.unwrap_or(0),
            size: self.size,
            fov: self.fov,
            chase_fov: self.chase_fov,
            volume: self.volume,
            volumes: Volumes {
                engine: self.engine_volume,
                codriver: self.codriver_volume,
                effects: self.effects_volume,
                tyres: self.tyres_volume,
            },
            codriver: self.codriver,
            hud: self.hud,
            graphics: self.graphics,
            wipers: self.wipers,
            standalone: self.paths.standalone.clone(),
            data: self.paths.data.clone(),
            marker: self.marker,
            bindings: bindings.map(Path::to_path_buf),
            setup,
            school,
        }
        .args();
        // The viewer's `--live` command is the rest of the line.
        let live = args.iter().position(|a| a == "--live").unwrap_or(args.len());
        let scale = render_scale_args(self.render_scale);
        let reflections = self.graphics.wet_roads.reflections();
        let reflections = (reflections != Reflections::default())
            .then(|| ["--reflections".to_owned(), reflections.name().to_owned()])
            .into_iter()
            .flatten();
        let detail = (self.graphics.detail != Detail::default())
            .then(|| ["--detail".to_owned(), self.graphics.detail.name().to_owned()])
            .into_iter()
            .flatten();
        let present = ["--present".to_owned(), self.present.name().to_owned()];
        let max_fps = (self.max_fps > 0).then(|| ["--max-fps".to_owned(), self.max_fps.to_string()]);
        let mirrors = ["--mirrors".to_owned(), self.graphics.mirrors.name()];
        let fullscreen = self.fullscreen.then(|| "--fullscreen".to_owned());
        let hdr = (self.hdr && rbr_render::present::HDR_OFFERED).then(|| "--hdr".to_owned());
        let perf_log = perf_log_args(self.perf_log, &self.logs_dir());
        let upscale = (self.upscale_textures && self.upscaler)
            .then(|| ["--upscale-textures".to_owned(), self.upscaler_dir().display().to_string()]);
        // The loading screen names the stage and the car (a school lesson: the lesson and the car).
        let map = match school {
            Some(lesson) => format!("Rally school lesson {}", lesson.index() + 1),
            None => self.catalog.map(stage.map).map_or(stage.name.clone(), catalog::Map::name),
        };
        let title = ["--title".to_owned(), format!("{map} · {}", car.name)];
        // The car's lock, the wheel's rotation when it is per car.
        let lock = self.setup.steering_lock().map(rbr_render::wheel_rotation::car_degrees);
        let car_lock = lock.into_iter().flat_map(|d| ["--car-lock".to_owned(), d.to_string()]);
        // Every live start records a replay next to the player's files (watched again with `viewer --replay`).
        let replay = replay_args(&self.here(), &stage.name);
        let viewer_extra = viewer_extra
            .into_iter()
            .chain(replay)
            .chain(scale)
            .chain(reflections)
            .chain(detail)
            .chain(present)
            .chain(max_fps.into_iter().flatten())
            .chain(mirrors)
            .chain(fullscreen)
            .chain(hdr)
            .chain(perf_log)
            .chain(upscale.into_iter().flatten())
            .chain(title)
            .chain(car_lock);
        args.splice(live..live, viewer_extra);
        args.extend(extra);
        args.extend(["--ngp-physics".to_owned(), self.ngp_archive()]);
        let sensitivity = self.ffb_cars.of(Some(&car.key), self.ffb.sensitivity);
        if sensitivity != [100; 3] {
            let [gravel, tarmac, snow] = sensitivity;
            args.extend(["--ffb-sensitivity".to_owned(), format!("{gravel},{tarmac},{snow}")]);
        }
        if self.head_gasket > 1 {
            args.extend(["--head-gasket-durability".to_owned(), self.head_gasket.to_string()]);
        }
        args.extend(self.engine_model.args(car));
        Some(args)
    }

    /// The stage's names for the stage-note lookup: RSF's aliases where there are any (its stage list's name and
    /// a community stage's own), then the launcher's own catalogue names (the map's name, the game's title and the
    /// stage variant), so the lookup works without an RSF stage list.
    pub fn stage_note_names(&self, stage: &catalog::Stage) -> Vec<String> {
        let root = self.game_root();
        let listed = stage_list(Path::new(root.trim())).remove(&stage.map).and_then(|mut e| e.remove("name"));
        let map = self.catalog.map(stage.map);
        listed
            .into_iter()
            .chain(map.and_then(catalog::Map::community).map(|s| s.name.clone()))
            .chain(map.map(catalog::Map::name))
            .chain(catalog::title(stage.map).map(str::to_owned))
            .chain([stage.name.clone()])
            .collect::<Vec<_>>()
            .into_iter()
            .fold(Vec::new(), |mut seen: Vec<String>, name| {
                if !name.is_empty() && !seen.contains(&name) {
                    seen.push(name);
                }
                seen
            })
    }
    /// The viewer as a child process with `args`: the game and the RSF install in its environment, without
    /// `RBR_PATCH` when the install folder is the game.
    pub(crate) fn viewer_command(&self, args: &[String]) -> Command {
        let mut command = Command::new(&self.paths.viewer);
        command.args(args).envs(self.child_env());
        if self.paths.patch.is_none() {
            command.env_remove("RBR_PATCH");
        }
        // A panic's backtrace in the log (`crash`).
        if std::env::var_os("RUST_BACKTRACE").is_none() {
            command.env("RUST_BACKTRACE", "1");
        }
        command
    }

    /// Runs the viewer with `args`; whether it started.
    pub(crate) fn spawn(&mut self, args: Vec<String>) -> bool {
        eprintln!("launcher: {} {}", self.paths.viewer.display(), args.join(" "));
        let title = args.iter().position(|a| a == "--title").and_then(|n| args.get(n + 1)).cloned().unwrap_or_default();
        let mut command = self.viewer_command(&args);
        match self.crash.start(&self.logs_dir(), &title) {
            Ok(log) => _ = command.stderr(log),
            Err(e) => eprintln!("launcher: no drive log: {e}"),
        }
        match command.spawn() {
            Ok(child) => {
                self.status = format!("viewer running (pid {})", child.id());
                self.child = Some(child);
                true
            }
            Err(e) => {
                self.crash.cancel();
                self.status = format!("{}: {e}", self.paths.viewer.display());
                false
            }
        }
    }

    /// Notes the viewer's exit.
    pub fn poll(&mut self) {
        let replay = self.replays.job.is_some();
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.status = format!("viewer exited ({status})");
                    self.crash.finished(status);
                    self.replays.finished(status.success());
                    self.child = None;
                }
                Ok(None) => {}
                Err(e) => {
                    self.status = format!("viewer: {e}");
                    self.replays.finished(false);
                    self.child = None;
                }
            }
            if self.child.is_none() {
                // The pause menu's Camera and Force feedback pages may have changed them.
                let saved = crate::settings::read_settings(&self.paths.settings);
                self.head = !saved.camera.head_off;
                self.ffb_cars = saved.ffb_cars;
                if !replay {
                    self.championship.stage_closed(&self.here(), &self.championship.profile.clone());
                }
                self.replays.refresh(&self.here());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::controls_page::EngineModel;
    use crate::graphics_page::{Quality, DEFAULT_GRAPHICS};
    use crate::settings::{load, Settings, SEQUENTIAL};
    use crate::{install, Paths, Start, SETTINGS_FILE};
    use rbr_formats::data::Data;
    use rbr_gameplay::race_options::{DamageLevel, DriverAids};
    use rbr_gameplay::weather::Season;
    use rbr_render::hud::Parts;
    use rbr_render::shifter::Engagement;
    use std::path::PathBuf;

    /// The tests' `Start`: the canned viewer and standalone, the scratch `settings` file, and the game folders.
    fn start(settings: PathBuf, data: Option<PathBuf>, patch: Option<PathBuf>) -> Start {
        Start {
            viewer: PathBuf::from("/bin/viewer"),
            standalone: PathBuf::from("/bin/rbr-standalone"),
            settings,
            data,
            patch,
        }
    }

    /// A non-empty environment variable.
    fn env(key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    /// A non-empty environment variable as a path.
    fn env_path(key: &str) -> Option<PathBuf> {
        std::env::var_os(key).map(PathBuf::from)
    }

    /// The flag's value in a launch's arguments.
    fn value(args: &[String], flag: &str) -> Option<String> {
        args.iter().position(|a| a == flag).map(|i| args[i + 1].clone())
    }

    /// The viewer command a form starts, with the tests' seed.
    fn launch(form: &mut Form) -> Vec<String> {
        form.start_args(&mut CrtRand::srand(1234)).unwrap()
    }

    /// Saved settings and the viewer command Start runs for them (`testdata/commands.txt`): the pages may move,
    /// the command stays. Needs `RBR_DATA`, `RBR_PATCH`, `RBR_NGP_PHYSICS` and `RBR_RSF` (unsaved, the last two are
    /// the defaults); `{ngp}` and `{rsf}` are theirs. `RBR_LAUNCHER_BLESS=1` writes the file.
    const COMMAND_CASES: [(&str, &str); 8] = [
        ("defaults", ""),
        (
            "everything changed",
            "[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 54\nweather = bad\nwear = worn\ntyre = 5\n\
             size = 1920x1080\nfov = 72.5\nchase_fov = 80\nvolume = 0.75\ncodriver = off\nhud = off\nunits = mph\nfps = on\n\
             pace_stack = 2\npace_3d = on\nwipers = intermittent\ncallout_distance = 0.5\nmarker_threshold = 12.5\n\
             damage = realistic\nauto_gears = on\ngear_guard = off\nneutral_lock = on\nclutch_help = off\n\
             ghost = best\nrender_scale = 75\nreflections = off\npresent = immediate\nfullscreen = off\nhdr = on\nstart_time = dusk\n",
        ),
        (
            "random weather, advanced shifter",
            "[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 71\nweather = random\nsize = 1280x720\n\
             [controls]\nh-shifter = advanced\n",
        ),
        ("NGP", "[launcher]\ncar = COROLLA\nmap = 33\nngp_physics = {ngp}\nweather = good\n"),
        (
            "RSF car on a retail stage",
            "[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 54\nngp_physics = {ngp}\nghost = best\n",
        ),
        (
            "RSF car and stage",
            "[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 129\nngp_physics = {ngp}\nghost = best\n",
        ),
        (
            "force feedback sensitivity",
            "[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 54\nhead_gasket = 10\n[ffb]\n\
             sensitivity_gravel = 70\nsensitivity_snow = 130\nsensitivity_car24 = 90,100,110\n",
        ),
        (
            "force feedback per car",
            "[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 54\nhead_gasket = 10\n[ffb]\n\
             sensitivity_gravel = 70\nsensitivity_snow = 130\n[ffb_cars]\nrsf-24 = 80,90,100\n",
        ),
    ];

    /// A rally school lesson's command: `--school N` with car 5 on the lesson's stage, without the flags
    /// `--school` refuses (tyre, setup, surface, championship, season row), and with the viewer's car.
    #[test]
    fn school_args_carry_the_lesson_without_its_refused_flags() {
        use rbr_school::Lesson;
        let lesson = Lesson::new(2).unwrap();
        let stage = Stage { name: "track-83_M".into(), map: 83, letter: 'M' };
        let car = Car { name: "IMPREZA03".into(), slot: 5, key: "IMPREZA03".into(), ..Car::default() };
        let weather =
            Weather { map: 83, row: Some(0), letter: 0, cloud: 0, cloud_letter: 0, preset: 0, wetness: 0, wear: 0 };
        let args = ViewerLaunch {
            stage,
            car,
            options: RaceOptions { weather: Preset::Good, ..RaceOptions::default() },
            weather,
            tyre: lesson.tyre(),
            size: (1280, 720),
            fov: 65.0,
            volume: 1.0,
            codriver: true,
            graphics: Quality::Low.graphics(false),
            wipers: Switch::Off,
            standalone: PathBuf::from("/b/rbr-standalone"),
            data: PathBuf::from("/d"),
            school: Some(lesson),
            ..Default::default()
        }
        .args();
        let live = args.iter().position(|a| a == "--live").unwrap();
        let (viewer, standalone) = (&args[..live], &args[live..]);
        assert_eq!(viewer[1..3], ["--car", "IMPREZA03"]);
        assert_eq!(value(standalone, "--school"), Some("2".into()));
        assert_eq!(value(standalone, "--stage"), Some("track-83_M".into()));
        assert_eq!(value(standalone, "--car"), Some("5".into()));
        for refused in ["--tyre", "--setup", "--surface", "--car-pack", "--championship", "--season-row"] {
            assert!(!standalone.contains(&refused.to_owned()), "{refused} refused: {args:?}");
        }
    }

    /// A school lesson in a pack car names the pack to `rbr-standalone` too, still without a setup.
    #[test]
    fn school_args_carry_a_pack_cars_pack() {
        use rbr_school::Lesson;
        let lesson = Lesson::new(2).unwrap();
        let car = Car {
            name: "Mitsubishi Lancer Evo IX N4".into(),
            slot: 5,
            physics: catalog::Physics::Pack("/rsf/rsfdata/cars/evo".into()),
            model_path: Some("/rsf/Cars/evo_IX/evo_IX.sgc".into()),
            key: "rsf-24".into(),
            ..Car::default()
        };
        let args = ViewerLaunch {
            stage: Stage { name: "track-83_M".into(), map: 83, letter: 'M' },
            car,
            tyre: lesson.tyre(),
            standalone: PathBuf::from("/b/rbr-standalone"),
            data: PathBuf::from("/d"),
            setup: Some(PathBuf::from("/l/setup-current.lsp")),
            school: Some(lesson),
            ..Default::default()
        }
        .args();
        let live = args.iter().position(|a| a == "--live").unwrap();
        let standalone = &args[live..];
        assert_eq!(value(standalone, "--car-pack"), Some("/rsf/rsfdata/cars/evo".into()));
        assert_eq!(value(standalone, "--school"), Some("2".into()));
        for refused in ["--car", "--tyre", "--setup"] {
            assert!(!standalone.contains(&refused.to_owned()), "{refused}: {args:?}");
        }
    }

    /// A pack car goes to the viewer and to `rbr-standalone` as its pack, in its slot.
    #[test]
    fn viewer_args_name_a_pack_cars_pack() {
        let stage = Stage { name: "track-54_E".into(), map: 54, letter: 'E' };
        let car = Car {
            name: "Mitsubishi Lancer Evo IX N4".into(),
            slot: 5,
            physics: catalog::Physics::Pack("/rsf/rsfdata/cars/evo".into()),
            model: Ok(rbr_formats::car_pack::Model {
                entry: rbr_formats::car_pack::model_entry("evo_IX", "evo_IX"),
                fallback: None,
            }),
            model_path: Some("/rsf/Cars/evo_IX/evo_IX.sgc".into()),
            key: "rsf-24".into(),
            ..Car::default()
        };
        assert_eq!(car.unavailable(), None);
        let weather =
            Weather { map: 54, row: Some(162), letter: 4, cloud: 1, cloud_letter: 0, preset: 0, wetness: 1, wear: 0 };
        let args = ViewerLaunch {
            stage,
            car,
            weather,
            tyre: 4,
            size: (1280, 720),
            fov: 65.0,
            chase_fov: 65.0,
            volume: 1.0,
            codriver: true,
            graphics: Quality::Low.graphics(false),
            wipers: Switch::Off,
            standalone: PathBuf::from("/b/rbr-standalone"),
            data: PathBuf::from("/d"),
            bindings: Some(PathBuf::from("/l/launcher-settings.ini")),
            setup: Some(PathBuf::from("/l/setup-current.lsp")),
            ..Default::default()
        }
        .args();
        let live = args.iter().position(|a| a == "--live").unwrap();
        assert_eq!(
            args[1..9],
            [
                "--car",
                "evo_IX",
                "--car-pack",
                "/rsf/rsfdata/cars/evo",
                "--car-model",
                "/rsf/Cars/evo_IX/evo_IX.sgc",
                "--car-key",
                "rsf-24"
            ]
        );
        assert_eq!(value(&args, "--car-key").as_deref(), Some("rsf-24"));
        assert!(!args[live..].contains(&"--car".to_owned()), "a pack car races in slot 5, named by its pack");
        assert!(args[live..].contains(&"--car-pack".to_owned()));
    }

    #[test]
    fn viewer_args_include_game_weather_wear_and_tyre() {
        let stage = Stage { name: "track-54_O".into(), map: 54, letter: 'O' };
        let car = Car { slot: 6, name: "COROLLA".into(), key: "COROLLA".into(), ..Car::default() };
        let weather =
            Weather { map: 54, row: Some(167), letter: 3, cloud: 3, cloud_letter: 2, preset: 4, wetness: 2, wear: 2 };
        let options = RaceOptions { weather: Preset::Bad, ..RaceOptions::default() };
        let args = ViewerLaunch {
            stage: stage.clone(),
            car: car.clone(),
            options,
            weather,
            tyre: 5,
            size: (1280, 720),
            fov: 65.0,
            chase_fov: 65.0,
            volume: 0.5,
            volumes: Volumes { engine: 0.5, codriver: 1.0, effects: 1.5, tyres: 2.0 },
            codriver: false,
            hud: HudChoice {
                show: false,
                mph: true,
                fps: true,
                stack: 3,
                dials: false,
                hide_wheel: true,
                parts: Parts::hiding("inputs,view").unwrap(),
            },
            graphics: Quality::Low.graphics(false),
            wipers: Switch::Continuous,
            standalone: PathBuf::from("/b/rbr-standalone"),
            data: PathBuf::from("/d"),
            bindings: Some(PathBuf::from("/l/launcher-settings.ini")),
            setup: Some(PathBuf::from("/l/setup-current.lsp")),
            ..Default::default()
        }
        .args();
        assert_eq!(
            args.join(" "),
            "track-54_O --car COROLLA --weather wet --size 1280x720 --fov 65 --chase-fov 65 --volume 0.5 --volumes 0.5,1,1.5,2 --no-codriver \
             --no-hud --mph --wipers continuous --fps --pace-stack 3 --no-dials --hide-wheel --hud-hide inputs,view --bindings /l/launcher-settings.ini \
             --cameras /l/launcher-settings.ini --enhance anisotropy,decals,water --live /b/rbr-standalone --live --boot --weather 2 --wear worn --tyre 5 \
             --stage track-54_O --car 6 --data /d --callout-distance 4 --marker-threshold 25 \
             --damage safe --auto-gears off --gear-guard on --neutral-lock off --clutch-help on --setup /l/setup-current.lsp"
        );
        let options = RaceOptions {
            weather: Preset::Random,
            damage: DamageLevel::Realistic,
            aids: DriverAids { auto_gears: true, gear_guard: false, neutral_lock: true, clutch_help: false },
        };
        let args = ViewerLaunch {
            stage,
            car,
            options,
            weather,
            tyre: 5,
            size: (1280, 720),
            fov: 65.0,
            chase_fov: 65.0,
            volume: 0.5,
            codriver: true,
            graphics: Graphics { lighting: true, marks: true, ..Quality::Low.graphics(false) },
            wipers: Switch::Auto,
            standalone: PathBuf::from("/b/rbr-standalone"),
            data: PathBuf::from("/d"),
            ..Default::default()
        }
        .args()
        .join(" ");
        assert!(args.contains("--boot --weather 1 --wear worn"), "{args}");
        assert!(args.contains("--enhance anisotropy,decals,lighting,marks,water --live "), "{args}");
        assert!(
            args.ends_with(
                "--season-row 167 --damage realistic --auto-gears on --gear-guard off --neutral-lock on --clutch-help off"
            ),
            "{args}"
        );
    }

    /// The Environment's extra arguments: the row's exact sky, our fog, clouds and rain for the viewer, the wetness
    /// override for `rbr-standalone` (a ghost keeps its own wetness, while the fog, clouds and rain stay editable).
    #[test]
    fn environment_args_name_the_sky_fog_clouds_rain_and_wetness() {
        let (viewer, standalone) = environment_args(
            Some("54O_E_heavyfog_heavycloud".into()),
            Some(Fog::Heavy),
            Some(Clouds::Partly),
            Some(Precipitation::None),
            Some(2),
            false,
        );
        assert_eq!(
            viewer,
            ["--sky", "54O_E_heavyfog_heavycloud", "--fog", "heavy", "--clouds", "partly", "--precipitation", "none"]
        );
        assert_eq!(standalone, ["--wetness", "wet"]);
        let (viewer, standalone) = environment_args(None, None, None, None, None, false);
        assert!(viewer.is_empty() && standalone.is_empty());
        let (viewer, standalone) = environment_args(
            Some("54M_norain_partcloud".into()),
            Some(Fog::Off),
            Some(Clouds::Heavy),
            Some(Precipitation::Heavy),
            Some(0),
            true,
        );
        assert_eq!(
            viewer,
            ["--sky", "54M_norain_partcloud", "--fog", "off", "--clouds", "heavy", "--precipitation", "heavy"]
        );
        assert!(standalone.is_empty());
    }

    /// The wetness override changes the viewer's `--weather` word (it replaces the weather's wetness after
    /// selection; the auto tyre and `rbr-standalone --wetness` follow the same weather).
    #[test]
    fn the_wetness_override_changes_the_viewer_weather_word() {
        let stage = Stage { name: "track-54_O".into(), map: 54, letter: 'O' };
        let car = Car { slot: 6, name: "COROLLA".into(), key: "COROLLA".into(), ..Car::default() };
        let weather = |wetness| Weather {
            map: 54,
            row: Some(7),
            letter: 3,
            cloud: 3,
            cloud_letter: 1,
            preset: 4,
            wetness,
            wear: 1,
        };
        let word = |wetness| {
            let args = ViewerLaunch {
                stage: stage.clone(),
                car: car.clone(),
                weather: weather(wetness),
                tyre: 3,
                size: (1280, 720),
                fov: 65.0,
                chase_fov: 65.0,
                volume: 1.0,
                codriver: true,
                graphics: Quality::Low.graphics(false),
                wipers: Switch::Off,
                standalone: PathBuf::from("/b/rbr-standalone"),
                data: PathBuf::from("/d"),
                ..Default::default()
            }
            .args();
            let at = args.iter().position(|a| a == "--weather").unwrap();
            args[at + 1].clone()
        };
        assert_eq!(word(0), "dry");
        assert_eq!(word(2), "wet");
    }

    /// A specific row resolves like Random with that row: the chosen wear is set after, the wetness override
    /// replaces the row's, and the launch runs as Random. A section that is not the map's falls back to Good at
    /// load. Needs no game data.
    #[test]
    fn a_specific_row_resolves_like_random_with_the_chosen_wear_and_override() {
        use crate::catalog::{Map, Stage};
        use crate::settings::WeatherChoice;
        use rbr_gameplay::weather::Row;
        let season = || Season {
            rows: vec![
                Row { map: 54, letter: 3, cloud: 3, cloud_letter: 1, preset: 4, wetness: 2 },
                Row { map: 54, letter: 3, cloud: 1, cloud_letter: 0, preset: 2, wetness: 1 },
                Row { map: 54, letter: 3, cloud: 3, cloud_letter: 1, preset: 4, wetness: 0 },
            ],
            nice: vec![0],
            bad: vec![1],
            sky: None,
        };
        let catalog = || Catalog {
            cars: Vec::new(),
            all_cars: Vec::new(),
            unlisted: Vec::new(),
            renamed: Vec::new(),
            maps: vec![Map {
                number: 54,
                stages: vec![Stage { name: "track-54_O".into(), map: 54, letter: 'O' }],
                origin: catalog::Origin::Retail,
            }],
        };
        let section = catalog::environment_rows(&season(), 54)[1].1.clone();
        assert_eq!(section, "54O_M_norain_partcloud");
        let here = crate::test_dir("command-environment");
        let paths = || Paths {
            data: here.join("data"),
            patch: None,
            viewer: PathBuf::from("/bin/viewer"),
            standalone: PathBuf::from("/bin/rbr-standalone"),
            settings: here.join(SETTINGS_FILE),
        };
        let settings = Settings {
            map: Some(54),
            weather: Some(WeatherChoice::Row(section.clone())),
            wetness: Some(2),
            fog: Some(Fog::Heavy),
            wear: Some(2),
            ..Settings::default()
        };
        let form = Form::new(
            catalog(),
            &settings,
            paths(),
            Data::new(here.join("data"), None),
            season(),
            install::Folder::default(),
        );
        assert_eq!(form.weather_row.as_deref(), Some(section.as_str()));
        assert_eq!(form.options.weather, Preset::Random);
        let (stage, weather, preset) = form.selection_from(&mut CrtRand::srand(1)).unwrap();
        assert_eq!((stage.name.as_str(), preset), ("track-54_O", Preset::Random));
        // The stage's weather keeps the row's wetness; the saved one is the Custom weather's.
        assert_eq!((weather.row, weather.wear, weather.wetness), (Some(1), 2, 1));
        assert_eq!(weather.section(&form.season).as_deref(), Some(section.as_str()));
        assert_eq!(weather.mat_words(), ("worn", "damp"));
        // A section that is not the map's falls back to Good at load.
        let settings = Settings { weather: Some(WeatherChoice::Row("21O_E_heavyfog_heavycloud".into())), ..settings };
        let form = Form::new(
            catalog(),
            &settings,
            paths(),
            Data::new(here.join("data"), None),
            season(),
            install::Folder::default(),
        );
        assert_eq!(form.weather_row, None);
        assert_eq!(form.options.weather, Preset::Good);
        // The Custom weather (a cloud cover) leaves the row: the good weather on a dry road, the bad on a wet one,
        // both with the chosen wear.
        for (wetness, preset) in [(0, Preset::Good), (2, Preset::Bad)] {
            let custom = Settings {
                weather: Some(WeatherChoice::Row(section.clone())),
                wetness: Some(wetness),
                clouds: Some(Clouds::Light),
                precipitation: Some(Precipitation::Heavy),
                ..settings.clone()
            };
            let form = Form::new(
                catalog(),
                &custom,
                paths(),
                Data::new(here.join("data"), None),
                season(),
                install::Folder::default(),
            );
            let (_, weather, chosen) = form.selection_from(&mut CrtRand::srand(1)).unwrap();
            assert_eq!((chosen, weather.wear, weather.wetness), (preset, 2, wetness));
        }
        std::fs::remove_dir_all(&here).unwrap();
    }

    /// A specific row's start carries its sky, fog, wetness and season row, and the wetness override changes the
    /// viewer's weather word. Needs `RBR_DATA`, `RBR_PATCH`, `RBR_NGP_PHYSICS` and `RBR_RSF`.
    #[test]
    fn the_performance_log_goes_to_the_logs_folder_when_on() {
        let dir = Path::new("/l/logs");
        assert_eq!(perf_log_args(true, dir), ["--perf-log", "/l/logs"]);
        assert!(perf_log_args(false, dir).is_empty());
    }

    #[test]
    fn a_specific_row_starts_with_its_sky_and_the_overrides() {
        let (Some(data), Some(patch), Some(_), Some(_)) =
            (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS"), env("RBR_RSF"))
        else {
            return;
        };
        let here = crate::test_dir("command-row-start");
        let start = start(here.join(SETTINGS_FILE), Some(PathBuf::from(&data)), Some(PathBuf::from(&patch)));
        let mut form = load(&start, Settings::parse("[launcher]\nmap = 54\n")).unwrap();
        form.read_only = true;
        form.map = form.catalog.map_index(54).unwrap();
        let rows = {
            let map = &form.catalog.maps[form.map];
            catalog::environment_rows(&map.season(&form.season), map.number)
        };
        assert!(rows.len() > 1);
        form.weather_row = Some(rows[1].1.clone());
        form.wetness = Some(2);
        form.fog = Some(Fog::Heavy);
        form.precipitation = Some(Precipitation::Heavy);
        form.start_time = time_of_day::StartTime::Moment(time_of_day::Moment::Dusk);
        form.wear = 1;
        let split = |args: Vec<String>| {
            let live = args.iter().position(|a| a == "--live").unwrap();
            let standalone = args[live..].to_vec();
            (args[..live].to_vec(), standalone)
        };
        // The stage's weather is the row as authored: the Custom fog, rain, wetness and time stay out.
        let (_, weather) = form.selection().unwrap();
        let (viewer, standalone) = split(form.start_args(&mut CrtRand::srand(1)).unwrap());
        assert_eq!(value(&viewer, "--sky"), weather.section(&form.season));
        for flag in ["--fog", "--clouds", "--precipitation", "--clock"] {
            assert_eq!(value(&viewer, flag), None, "{flag}");
        }
        assert_eq!(weather.wetness, 1);
        assert_eq!(value(&viewer, "--weather").as_deref(), Some("damp"));
        assert_eq!(value(&standalone, "--wetness"), None);
        assert_eq!(value(&standalone, "--wear").as_deref(), Some("normal"));
        assert_eq!(value(&standalone, "--season-row"), weather.row.map(|row| row.to_string()));
        // The Custom weather takes them all over the bad weather (a wet road).
        form.clouds = Some(Clouds::Light);
        let (_, weather) = form.selection().unwrap();
        let (viewer, standalone) = split(form.start_args(&mut CrtRand::srand(1)).unwrap());
        assert_eq!(value(&viewer, "--sky"), weather.section(&form.season));
        assert_eq!(value(&viewer, "--fog").as_deref(), Some("heavy"));
        assert_eq!(value(&viewer, "--clouds").as_deref(), Some("light"));
        assert_eq!(value(&viewer, "--precipitation").as_deref(), Some("heavy"));
        form.precipitation = Some(Precipitation::Moderate);
        let (viewer, _) = split(form.start_args(&mut CrtRand::srand(1)).unwrap());
        assert_eq!(value(&viewer, "--precipitation").as_deref(), Some("moderate"));
        form.precipitation = Some(Precipitation::Heavy);
        let dusk = time_of_day::Moment::Dusk.minutes(time_of_day::site(&form.catalog.maps[form.map]));
        assert_eq!(value(&viewer, "--clock"), Some(clock_text(dusk)));
        assert_eq!(value(&viewer, "--weather").as_deref(), Some("wet"));
        assert_eq!(value(&standalone, "--wetness").as_deref(), Some("wet"));
        assert_eq!(value(&standalone, "--weather").as_deref(), Some("2"));
        assert_eq!(value(&standalone, "--wear").as_deref(), Some("normal"));
        let lock = viewer.iter().position(|a| a == "--car-lock").map(|at| viewer[at + 1].clone());
        assert_eq!(lock, form.setup.steering_lock().map(|l| rbr_render::wheel_rotation::car_degrees(l).to_string()));
        std::fs::remove_dir_all(&here).unwrap();
    }

    #[test]
    fn codriver_args_name_the_mod_and_voice() {
        let tmp = crate::test_dir("command-codriver");
        let plugin = tmp.join("Plugins/Pacenote");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("PaceNote.ini"), "[SETTINGS]\nsounds=english/steve\n").unwrap();
        let mods = CodriverMods::scan(&tmp.display().to_string()).unwrap();
        let args = |voice| codriver_base_args(&mods, voice, "Rbr.ini", "Rbr.ini").join(" ");
        let head = format!("--codriver-mod {} --codriver-voice", mods.dir.display());
        let tail = "--codriver-style Rbr.ini --codriver-ranges Rbr.ini";
        assert_eq!(args(None), format!("{head} english/steve {tail}"));
        assert_eq!(args(Some("Janne")), format!("{head} Janne {tail}"));
        assert_eq!(args(Some(ORIGINAL_VOICE)), "");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_enhance_list_names_what_the_viewer_takes() {
        assert_eq!(enhance_list(&Quality::Low.graphics(false)), "anisotropy,decals,water");
        assert_eq!(
            enhance_list(&DEFAULT_GRAPHICS),
            "anisotropy,cabin,contact,decals,grass,lighting,particles,water,wet,windscreen"
        );
        // Ultra with the tyre marks is every addition.
        assert_eq!(Enhance::parse(&enhance_list(&Quality::Ultra.graphics(true))), Enhance::parse("all,grass"));
        for preset in Quality::ALL {
            assert_eq!(Quality::of(preset.graphics(true)), Some(preset));
        }
        assert_eq!(Quality::of(Graphics { grass: false, ..DEFAULT_GRAPHICS }), None);
    }

    /// The advanced H-shifter takes the car's gearbox and the grind from RBRControls' folder.
    #[test]
    fn shifter_args_read_the_grind() {
        let root = crate::test_dir("command-shifter");
        let dir = root.join("Plugins").join("RBRControls");
        std::fs::create_dir_all(&dir).unwrap();
        let dog = CarBox { engagement: Engagement::Dog, sequential: false };
        assert_eq!(shifter_args(dog, &root), ["--car-gearbox", "dog"]);
        assert_eq!(shifter_args(SEQUENTIAL, Path::new("")), ["--car-gearbox", "sequential"]);
        std::fs::write(dir.join("Shifter_Error.wav.sample"), []).unwrap();
        let grind = dir.join("Shifter_Error.wav.sample").display().to_string();
        assert_eq!(shifter_args(dog, &root), ["--car-gearbox".to_owned(), "dog".into(), "--grind-sound".into(), grind]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The Time of day reaches a quick race's viewer and start board, and not a championship stage (`launch_args`,
    /// for a plain championship stage), which keeps the stage's own light and the season's start clock.
    #[test]
    fn the_time_of_day_is_the_quick_races_only() {
        let (Some(data), Some(patch), Some(_)) = (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_RSF")) else {
            return;
        };
        let here = crate::test_dir("command-clock");
        let text = "[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 54\nclouds = partly\nstart_time = night\n";
        let start = start(here.join(SETTINGS_FILE), Some(PathBuf::from(&data)), Some(PathBuf::from(&patch)));
        let mut form = load(&start, Settings::parse(text)).unwrap();
        form.read_only = true;
        let quick = form.start_args(&mut CrtRand::srand(1234)).unwrap();
        let night = time_of_day::Moment::Night.minutes(time_of_day::site(&form.catalog.maps[form.map]));
        assert_eq!(value(&quick, "--clock"), Some(clock_text(night)));
        assert_eq!(value(&quick, "--start-time"), Some(format!("{}:{:02}", night / 60, night % 60)));
        let (stage, weather) = form.selection().unwrap();
        let stage_args = vec!["--championship".to_owned(), "rookie".to_owned()];
        let rally = form.launch_args(stage, form.options, weather, stage_args, Vec::new()).unwrap();
        assert!(!rally.iter().any(|a| a == "--clock" || a == "--start-time"), "{rally:?}");
        std::fs::remove_dir_all(&here).unwrap();
    }

    /// The realistic engine model reaches `rbr-standalone` (after `--live`) in a quick race, a championship stage
    /// and a school lesson; NGP's, the default, adds nothing.
    #[test]
    fn the_engine_model_reaches_the_stepping_process() {
        // The car's group and year go with the model where the catalog has them: an RSF car both, an added car its
        // group, an archive car neither.
        let rsf = Car { group: Some("Rally 2".into()), year: Some(2019), ..Car::default() };
        let added = Car { group: Some("Group A8".into()), added: true, ..Car::default() };
        assert!(EngineModel::Ngp.args(&rsf).is_empty());
        let realistic = ["--engine-model", "realistic"];
        let with = |extra: &[&str]| realistic.iter().chain(extra).map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(EngineModel::Realistic.args(&rsf), with(&["--car-group", "Rally 2", "--car-year", "2019"]));
        assert_eq!(EngineModel::Realistic.args(&added), with(&["--car-group", "Group A8"]));
        assert_eq!(EngineModel::Realistic.args(&Car::default()), with(&[]));
        let (Some(data), Some(patch), Some(_)) = (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_RSF")) else {
            return;
        };
        let here = crate::test_dir("command-engine");
        for (line, wanted) in
            [("", None), ("engine_model = ngp\n", None), ("engine_model = realistic\n", Some("realistic"))]
        {
            let text = format!("[launcher]\ncar = Mitsubishi Lancer Evo IX N4\nmap = 54\n{line}");
            let start = start(here.join(SETTINGS_FILE), Some(PathBuf::from(&data)), Some(PathBuf::from(&patch)));
            let mut form = load(&start, Settings::parse(&text)).unwrap();
            form.read_only = true;
            let quick = form.start_args(&mut CrtRand::srand(1234)).unwrap();
            let (stage, weather) = form.selection().unwrap();
            let stage_args = vec!["--championship".to_owned(), "rookie".to_owned()];
            let rally = form.launch_args(stage, form.options, weather, stage_args, Vec::new()).unwrap();
            let lesson = Lesson::new(0).unwrap();
            let draws = Draws { row: None, wear: 0 };
            let (stage, weather) = form.catalog.stage_weather(&form.season, lesson.map(), Preset::Good, draws).unwrap();
            let car = form.car().cloned().unwrap();
            let launch = LaunchCar { car, tyre: None, setup: None, school: Some(lesson) };
            let school = form.launch_with(stage, launch, form.options, weather, Vec::new(), Vec::new()).unwrap();
            let picked = form.car().cloned().unwrap();
            assert!(picked.group.is_some() && picked.year.is_some(), "{picked:?}");
            let (group, year) = match wanted {
                Some(_) => (picked.group.clone(), picked.year.map(|y| y.to_string())),
                None => (None, None),
            };
            for args in [&quick, &rally, &school] {
                let live = args.iter().position(|a| a == "--live").unwrap();
                assert_eq!(value(&args[live..], "--engine-model").as_deref(), wanted, "{text}: {args:?}");
                assert_eq!(value(&args[live..], "--car-group"), group, "{text}: {args:?}");
                assert_eq!(value(&args[live..], "--car-year"), year, "{text}: {args:?}");
                assert!(!args[..live].contains(&"--engine-model".to_owned()), "{args:?}");
            }
        }
        std::fs::remove_dir_all(&here).unwrap();
    }

    #[test]
    fn the_viewer_command_follows_the_saved_settings() {
        let (Some(data), Some(patch), Some(ngp), Some(rsf)) =
            (env("RBR_DATA"), env("RBR_PATCH"), env("RBR_NGP_PHYSICS"), env("RBR_RSF"))
        else {
            return;
        };
        let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata/commands.txt");
        let mut cases = Vec::new();
        for (n, (name, text)) in COMMAND_CASES.into_iter().enumerate() {
            let text = text.replace("{ngp}", &ngp).replace("{rsf}", &rsf);
            let here = crate::test_dir(&format!("command-case-{n}"));
            let settings = here.join(SETTINGS_FILE);
            std::fs::write(&settings, &text).unwrap();
            let start = start(settings.clone(), Some(PathBuf::from(&data)), Some(PathBuf::from(&patch)));
            let mut form = load(&start, Settings::parse(&text)).unwrap();
            form.read_only = true;
            let mut rand = CrtRand::srand(1234);
            let args = form.start_args(&mut rand).unwrap_or_else(|| panic!("{name}: {}", form.status));
            let env: Vec<String> = form.child_env().iter().map(|(k, v)| format!("{k}={}", v.display())).collect();
            let lines = env.into_iter().chain(args);
            let mut case: String = lines.map(|line| line + "\n").collect();
            if let Some(dll) = form.ngp_dll.resolved(&form.data, &form.game_root()) {
                case = case.replace(&dll.display().to_string(), "<ngp-dll>");
            }
            case = case.replace(&here.display().to_string(), "<here>");
            for (path, tag) in [(&data, "<data>"), (&patch, "<patch>"), (&ngp, "<ngp>"), (&rsf, "<rsf>")] {
                case = case.replace(path.as_str(), tag);
            }
            // The replay's name carries the start's nanoseconds: keep the stage, drop the time.
            if let Some(at) = case.find("--save-replay\n") {
                let start = at + "--save-replay\n".len();
                let end = start + case[start..].find('\n').unwrap_or(case[start..].len());
                let stamp =
                    "replays/".len() + case[start..end].find("replays/").expect("a replay in the player's replays");
                let dash = case[start + stamp..end].find('-').expect("seconds, then the stage");
                case.replace_range(start + stamp..start + stamp + dash, "<time>");
            }
            cases.push((name, case));
            let _ = std::fs::remove_dir_all(&here);
        }
        if std::env::var_os("RBR_LAUNCHER_BLESS").is_some() {
            let text: String = cases.iter().map(|(name, case)| format!("== {name}\n{case}")).collect();
            std::fs::write(&golden, text).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&golden).unwrap();
        let expected: Vec<(&str, &str)> =
            expected.split("== ").skip(1).filter_map(|block| block.split_once('\n')).collect();
        for (name, case) in &cases {
            let found = expected.iter().find(|(n, _)| n == name).map(|(_, c)| *c);
            assert_eq!(found, Some(case.as_str()), "{name}");
        }
    }

    /// The installed voice and Tanner notes reach the viewer; choosing the original voice or disabling calls
    /// removes the mod without changing the stepping process's arguments.
    #[test]
    fn the_installed_codriver_reaches_the_viewer_on_tanner() {
        let (Some(data), Some(patch), Some(ngp), Some(rsf)) =
            (env_path("RBR_DATA"), env_path("RBR_PATCH"), env_path("RBR_NGP_PHYSICS"), env_path("RBR_RSF"))
        else {
            return;
        };
        let Some(mods) = CodriverMods::scan(&rsf.to_string_lossy()) else { return };
        let Some(voice) = mods.voice(None) else { return };
        let here = crate::test_dir("command-tanner-codriver");
        let start = start(here.join(SETTINGS_FILE), Some(data), Some(patch));
        let settings =
            Settings { map: Some(54), ngp_physics: Some(ngp.to_string_lossy().into_owned()), ..Settings::default() };
        let mut form = load(&start, settings).unwrap();
        form.read_only = true;
        let defaults = launch(&mut form);
        assert_eq!(value(&defaults, "--codriver-mod"), Some(mods.dir.display().to_string()));
        assert_eq!(value(&defaults, "--codriver-voice"), Some(voice.clone()));
        // Unset, the install's largest note set plays (LuppisV3 in an RSF install).
        let largest = form.rsf_note_sets.first().map(|set| set.name.clone()).unwrap_or_default();
        let notes = rbr_formats::pacenote::notes::my_pacenotes_dir(&rsf)
            .and_then(|dir| crate::codriver_packs::find_in_set(&dir, &["Tanner"], &largest));
        assert_eq!(value(&defaults, "--pacenotes"), notes.map(|p| p.display().to_string()));
        let stepping = defaults[defaults.iter().position(|a| a == "--live").unwrap()..].to_vec();

        form.codriver_voice = Some(voice.clone());
        form.codriver_style = "Numeric.ini".into();
        form.codriver_ranges = "Rbr.ini".into();
        form.my_pacenotes = false;
        let custom = launch(&mut form);
        assert_eq!(value(&custom, "--codriver-voice"), Some(voice));
        assert_eq!(value(&custom, "--codriver-style").as_deref(), Some("Numeric.ini"));
        assert_eq!(value(&custom, "--codriver-ranges").as_deref(), Some("Rbr.ini"));
        assert!(value(&custom, "--pacenotes").is_none());

        form.codriver_voice = Some(ORIGINAL_VOICE.into());
        let original = launch(&mut form);
        assert!(value(&original, "--codriver-mod").is_none());
        assert_eq!(&original[original.iter().position(|a| a == "--live").unwrap()..], stepping);
        form.codriver_voice = None;
        form.codriver = false;
        let muted = launch(&mut form);
        assert!(muted.iter().any(|a| a == "--no-codriver"));
        assert!(value(&muted, "--codriver-mod").is_none());
        std::fs::remove_dir_all(here).unwrap();
    }

    /// Without a chosen pack, a locally owned voice pack and Tanner notes reach the viewer on the same flags;
    /// the imported choices persist in the settings and work after the sources are removed. The stepping
    /// process's arguments never change.
    #[test]
    fn local_codriver_packs_drive_without_rsf() {
        use crate::codriver_packs;
        use rbr_formats::pacenote::codriver;
        let (Some(data), Some(patch), Some(ngp)) =
            (env_path("RBR_DATA"), env_path("RBR_PATCH"), env_path("RBR_NGP_PHYSICS"))
        else {
            return;
        };
        let root = crate::test_dir("command-local-codriver");
        let write = |path: PathBuf, text: &str| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        };
        // A voice download: an extracted folder with a wrapper directory.
        let download = root.join("download").join("wrapper");
        write(download.join("PaceNote.ini"), "[SETTINGS]\nsounds=english/steve\n");
        write(download.join("config/pacenotes/Rbr.ini"), "[PACKAGE::RBR]\nfile=calls.ini\n");
        write(download.join("config/pacenotes/calls.ini"), "[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=one_left.ogg\n");
        write(download.join("config/ranges/Rbr.ini"), "[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(download.join("sounds/english/steve/one_left.ogg"), "OggS fake");
        // A notes-only download: a MyPacenotes folder.
        let notes_src = root.join("notes-download");
        write(
            notes_src.join("Tanner/tanner.ini"),
            "[PACENOTES]\ncount = 1\n\n[P0]\ntype = 16\ndistance = 100.0\nflag = 0\n",
        );
        // The player's folder owns the copies; the chosen packs drive over the game folder's plugin.
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        codriver_packs::import(&home, &root.join("download"), &std::sync::atomic::AtomicBool::new(false)).unwrap();
        codriver_packs::import(&home, &notes_src, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        let start = start(home.join(SETTINGS_FILE), Some(data), Some(patch));
        let settings = Settings {
            map: Some(54),
            ngp_physics: Some(ngp.to_string_lossy().into_owned()),
            codriver_pack: Some("download".into()),
            notes_pack: Some("notes-download".into()),
            ..Settings::default()
        };
        // The choices persist: written and read back.
        let text = settings.to_text();
        assert!(text.contains("codriver_pack = download\nnotes_pack = notes-download\n"), "{text}");
        assert_eq!(Settings::parse(&text).codriver_pack.as_deref(), Some("download"));
        let mut form = load(&start, settings).unwrap();
        form.read_only = true;
        assert!(form.codriver_mods.is_some());
        let args = launch(&mut form);
        let pack = home.join(codriver_packs::VOICES_DIR).join("download");
        assert_eq!(value(&args, "--codriver-mod"), Some(pack.display().to_string()));
        assert_eq!(value(&args, "--codriver-voice").as_deref(), Some("english/steve"));
        let notes =
            codriver_packs::find_in_set(&home.join(codriver_packs::NOTES_DIR).join("notes-download"), &["Tanner"], "");
        assert_eq!(value(&args, "--pacenotes"), notes.map(|p| p.display().to_string()));
        assert!(value(&args, "--pacenotes").is_some());
        let stepping = args[args.iter().position(|a| a == "--live").unwrap()..].to_vec();
        assert!(!stepping.iter().any(|a| a == "--codriver-mod" || a == "--pacenotes"));
        // Imported choices work after the sources are removed.
        std::fs::remove_dir_all(root.join("download")).unwrap();
        std::fs::remove_dir_all(&notes_src).unwrap();
        form.rescan_codrivers();
        assert!(codriver::load(&pack, "Rbr.ini", "Rbr.ini").unwrap().notes.contains_key(&0));
        let again = launch(&mut form);
        assert_eq!(value(&again, "--codriver-mod"), Some(pack.display().to_string()));
        assert_eq!(&again[again.iter().position(|a| a == "--live").unwrap()..], stepping);
        // The original voice and mute keep working with local packs chosen.
        form.codriver_voice = Some(ORIGINAL_VOICE.into());
        let original = launch(&mut form);
        assert!(value(&original, "--codriver-mod").is_none());
        assert_eq!(&original[original.iter().position(|a| a == "--live").unwrap()..], stepping);
        form.codriver_voice = None;
        form.codriver = false;
        let muted = launch(&mut form);
        assert!(muted.iter().any(|a| a == "--no-codriver"));
        assert!(value(&muted, "--codriver-mod").is_none());
        // Switching packs normalizes stale choices: the old voice and style do not travel along.
        let second = root.join("second");
        write(second.join("config/pacenotes/Voice.ini"), "[PACKAGE::V]\nfile=z-calls.ini\n");
        write(second.join("config/pacenotes/z-calls.ini"), "[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=one_left.ogg\n");
        write(second.join("config/ranges/Voice.ini"), "[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(second.join("sounds/other/ann/one_left.ogg"), "OggS fake");
        let no_cancel = std::sync::atomic::AtomicBool::new(false);
        codriver_packs::import(&home, &second, &no_cancel).unwrap();
        form.codriver = true;
        form.codriver_voice = Some("english/steve".into());
        form.codriver_style = "Rbr.ini".into();
        form.codriver_pack = Some("second".into());
        form.rescan_codrivers();
        form.normalize_codriver();
        assert_eq!(form.codriver_voice, None);
        assert_eq!(form.codriver_style, "Voice.ini");
        assert_eq!(form.codriver_ranges, "Voice.ini");
        // The pack without a PaceNote.ini speaks its discovered voice, and the normalized choices save.
        let switched = launch(&mut form);
        assert_eq!(value(&switched, "--codriver-voice").as_deref(), Some("other/ann"));
        form.read_only = false;
        assert!(form.save());
        let saved = std::fs::read_to_string(home.join(SETTINGS_FILE)).unwrap();
        assert!(saved.contains("codriver_pack = second\n"), "{saved}");
        assert!(!saved.contains("codriver_voice"), "{saved}");
        assert!(saved.contains("codriver_style = Voice.ini\n"), "{saved}");
        // The explicit 1.02 choice survives a pack switch.
        form.codriver_voice = Some(ORIGINAL_VOICE.into());
        form.normalize_codriver();
        assert_eq!(form.codriver_voice.as_deref(), Some(ORIGINAL_VOICE));
        std::fs::remove_dir_all(&root).unwrap();
    }

    // A live end-to-end test of a built event through the launcher's turns (the old two-stage 1.02 season test's
    // successor) is still to write: it needs the events' stage launch (`Form::start_championship_stage`) and `Event` wired
    // for the service and bulletins.
}
