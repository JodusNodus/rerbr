//! Building the launcher's form from the settings file ([`load`]).
use std::path::{Path, PathBuf};

use rbr_formats::data::Data;
use rbr_gameplay::crt::CrtRand;
use rbr_gameplay::markers::MarkerSettings;
use rbr_gameplay::race_options::{DriverAids, RaceOptions};
use rbr_gameplay::weather::{Preset, Season};
use rbr_render::coords;
use rbr_render::playback::CHASE_FOV_DEGREES;
use rbr_render::present::Present;
use rbr_sound::custom_codriver;

use crate::catalog::{self, Catalog, TyreChoice};
use crate::controls_page::ControlsPage;
use crate::graphics_page::{
    Graphics, HudChoice, DEFAULT_GRAPHICS, TRIPLE_BEZEL_DEFAULT, TRIPLE_DIAGONAL_DEFAULT, TRIPLE_DISTANCE_DEFAULT,
};
use crate::pages::Page;
use crate::school_page::SchoolPage;
use crate::setup_page::SetupPage;
use crate::{install, preview, Form, Paths, Start};

use super::weather::WeatherChoice;
use super::{pick_car, Settings, DEFAULT_SIZE};
#[cfg(test)]
use crate::catalog::Car;

/// The settings file's settings (the defaults without one).
pub fn read_settings(path: &Path) -> Settings {
    std::fs::read_to_string(path).map(|t| Settings::parse(&t)).unwrap_or_default()
}

/// The form for the game of `settings` (their install folder, or `start`'s overrides), or why there is none.
pub fn load(start: &Start, mut settings: Settings) -> Result<Form, String> {
    let folder = install::Folder::read(settings.install_folder.as_deref().unwrap_or_default());
    let data = start.data.clone().or_else(|| folder.game.clone());
    let paths = Paths {
        data: data.ok_or("no game: set the RBR install folder on the Settings page (or --data)")?,
        patch: start.patch.clone().or_else(|| folder.game.clone()),
        viewer: start.viewer.clone(),
        standalone: start.standalone.clone(),
        settings: start.settings.clone(),
    };
    // Unsaved, the NGP paths come from the environment; and NGP physics is on by default when its archive
    // is there.
    let env = |key| std::env::var(key).ok().filter(|v| !v.is_empty());
    settings.ngp_physics = settings.ngp_physics.or_else(|| env("RBR_NGP_PHYSICS"));
    settings.ngp_dll = settings.ngp_dll.or_else(|| env("RBR_NGP_DLL"));
    let here = paths.settings.parent().unwrap_or(Path::new(""));
    use_kept_ngp(&mut settings, here);
    let mut catalog = Catalog::scan(&paths.data)?;
    catalog.hide_unlisted();
    let root = paths.game_root();
    catalog.set_community_stages(&root);
    catalog.add_btb_root(&here.join(rbr_btb::converted::FOLDER));
    catalog.set_community_cars(&paths.data, &root, &here.join(crate::car_packs::DIR));
    move_renamed_keys(&mut settings, &catalog.renamed);
    catalog.offer(&catalog::ngp_cars(Path::new(&or_folder(settings.ngp_physics.as_deref(), &folder.ngp_physics))));
    let data = Data::new(&paths.data, paths.patch.clone());
    let season = Season::load(&data)?;
    // Without community cars the form still loads: the Settings page and the status line say what is missing.
    if catalog.maps.is_empty() {
        return Err(format!("no maps in {}", paths.data.display()));
    }
    let startable = catalog.startable_maps(&season);
    eprintln!("launcher: {} cars, {}/{} maps startable", catalog.cars.len(), startable.len(), catalog.maps.len());
    let form = Form::new(catalog, &settings, paths, data, season, folder);
    Ok(Form { session_physics: start.physics, ffb_trial: start.ffb_trial.clone(), ..form })
}

/// The FFB sensitivities and camera nudges saved under a pack car's earlier key (`Catalog::renamed`) go to its key
/// now (at the load for the form, at each save for the file, which the viewer reads); one already there stays.
fn move_renamed_keys(settings: &mut Settings, renamed: &[(String, String)]) {
    for (old, now) in renamed {
        if let Some(value) = settings.ffb_cars.cars.remove(old) {
            settings.ffb_cars.cars.entry(now.clone()).or_insert(value);
        }
        settings.ffb_load.rename(old, now);
        if let Some(nudges) = settings.camera.cars.remove(old) {
            settings.camera.cars.entry(now.clone()).or_insert(nudges);
        }
    }
}

/// With neither NGP path set, NGP 7.4.778's copy downloaded before, when the player's folder `here` keeps it
/// (`ngp_install::kept`).
fn use_kept_ngp(settings: &mut Settings, here: &Path) {
    let unset = |field: &Option<String>| field.as_deref().is_none_or(|f| f.trim().is_empty());
    if unset(&settings.ngp_dll) && unset(&settings.ngp_physics) {
        if let Some((dll, physics)) = crate::ngp_install::kept(here) {
            settings.ngp_dll = Some(dll.display().to_string());
            settings.ngp_physics = Some(physics.display().to_string());
        }
    }
}

/// A path field's text, or the install folder's path when it is empty.
pub fn or_folder(field: Option<&str>, folder: &Option<PathBuf>) -> String {
    match field.map(str::trim).filter(|f| !f.is_empty()) {
        Some(field) => field.to_owned(),
        None => folder.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
    }
}

impl Form {
    pub fn new(
        catalog: Catalog,
        settings: &Settings,
        paths: Paths,
        data: Data,
        season: Season,
        folder: install::Folder,
    ) -> Form {
        // A saved car not in the list gives way to the most recent one that is, else the first that starts.
        let car = pick_car(&catalog.cars, settings.car.iter().chain(&settings.recent_cars));
        let default_map = catalog.maps.iter().position(|map| catalog.availability(&season, map).is_ok()).unwrap_or(0);
        let map = settings.map.and_then(|number| catalog.map_index(number)).unwrap_or(default_map);
        let aids = DriverAids::default();
        // A specific row launches like Random with that row; a section that is not this map's falls back to Good.
        let (mut preset, mut weather_row) =
            match settings.weather.clone().unwrap_or(WeatherChoice::Preset(Preset::Good)) {
                WeatherChoice::Preset(preset) => (preset, None),
                WeatherChoice::Row(section) => (Preset::Random, Some(section)),
            };
        let map_number = catalog.maps[map].number;
        if weather_row
            .as_deref()
            .is_some_and(|s| catalog::row_by_section(&catalog.maps[map].season(&season), map_number, s).is_none())
        {
            (preset, weather_row) = (Preset::Good, None);
        }
        let options = RaceOptions {
            weather: preset,
            damage: settings.damage.unwrap_or_default(),
            aids: DriverAids {
                auto_gears: settings.auto_gears.unwrap_or(aids.auto_gears),
                gear_guard: settings.gear_guard.unwrap_or(aids.gear_guard),
                neutral_lock: settings.neutral_lock.unwrap_or(aids.neutral_lock),
                clutch_help: settings.clutch_help.unwrap_or(aids.clutch_help),
            },
        };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let wear = settings.wear.unwrap_or(0).min(2);
        let tyre = settings.tyre.unwrap_or(TyreChoice::Auto);
        let size = settings.size.unwrap_or(DEFAULT_SIZE);
        let fov = settings.fov.unwrap_or(coords::FOV_DEGREES);
        let chase_fov = settings.chase_fov.unwrap_or(CHASE_FOV_DEGREES);
        let volume = settings.volume.unwrap_or(1.0);
        let engine_volume = settings.engine_volume.unwrap_or(1.0);
        let codriver_volume = settings.codriver_volume.unwrap_or(1.0);
        let effects_volume = settings.effects_volume.unwrap_or(1.0);
        let tyres_volume = settings.tyres_volume.unwrap_or(1.0);
        let defaults = MarkerSettings::default();
        let marker = MarkerSettings {
            callout_distance: settings.callout_distance.unwrap_or(defaults.callout_distance),
            marker_threshold: settings.marker_threshold.unwrap_or(defaults.marker_threshold),
        };
        let adding = crate::car_packs::Adding::new(&paths.settings.with_file_name(crate::car_packs::DIR));
        let home = paths.settings.parent().unwrap_or(Path::new(".")).to_path_buf();
        let game_root = paths.game_root();
        let mut form = Form {
            catalog,
            paths,
            previews: preview::Previews::new(data.clone()),
            school: SchoolPage::new(&data),
            championship: crate::championship_page::ChampionshipPage::new(&data),
            data,
            season,
            page: Page::Drive,
            setup: SetupPage::new(settings.setups.clone()),
            car,
            map,
            // Filled by `list_cars`.
            car_browser: Default::default(),
            gearboxes: settings.gearboxes.clone(),
            // Filled by `list_maps`.
            stage_browser: Default::default(),
            replays: Default::default(),
            recent_cars: settings.recent_cars.clone(),
            recent_maps: settings.recent_maps.clone(),
            favourite_cars: settings.favourite_cars.clone(),
            favourite_maps: settings.favourite_maps.clone(),
            options,
            rand: CrtRand::srand(now.as_secs() as u32),
            weather_row,
            wetness: settings.wetness,
            fog: settings.fog,
            clouds: settings.clouds,
            precipitation: settings.precipitation,
            weather_adjust: false,
            wear,
            tyre,
            size,
            sizes: vec![size],
            fov,
            chase_fov,
            cockpit: settings.camera.cockpit,
            volume,
            engine_volume,
            codriver_volume,
            effects_volume,
            tyres_volume,
            codriver: settings.codriver.unwrap_or(true),
            codriver_mods: crate::command::CodriverMods::scan(&game_root),
            codriver_pack: settings.codriver_pack.clone(),
            notes_pack: settings.notes_pack.clone(),
            notes_set: settings.notes_set.clone(),
            rsf_note_sets: Vec::new(),
            rsf_plugin: false,
            rsf_voices: Vec::new(),
            codriver_styles_differ: false,
            codriver_ranges_differ: false,
            cached_notes_map: u32::MAX,
            notes_summary: Default::default(),
            cached_notes_dirty: true,
            owned_voices: crate::codriver_packs::voice_packs(&home),
            owned_notes: crate::codriver_packs::note_packs(&home),
            codriver_job: None,
            codriver_status: None,
            codriver_voice: settings.codriver_voice.clone(),
            codriver_style: settings.codriver_style.clone().unwrap_or_else(|| custom_codriver::DEFAULT_STYLE.into()),
            codriver_ranges: settings.codriver_ranges.clone().unwrap_or_else(|| custom_codriver::DEFAULT_RANGES.into()),
            my_pacenotes: settings.my_pacenotes.unwrap_or(true),
            ghost: settings.ghost.clone().unwrap_or_default(),
            menu_sound: None,
            hud: HudChoice {
                show: settings.hud.unwrap_or(true),
                mph: settings.mph.unwrap_or(false),
                fps: settings.fps.unwrap_or(false),
                stack: settings.pace_stack.unwrap_or(0),
                dials: settings.dials.unwrap_or(true),
                parts: settings.hud_hide.unwrap_or_default(),
                layout: settings.hud_layout.unwrap_or_default(),
            },
            graphics: Graphics {
                lighting: settings.lighting.unwrap_or(DEFAULT_GRAPHICS.lighting),
                particles: settings.particles.unwrap_or(DEFAULT_GRAPHICS.particles),
                cabin: settings.cabin.unwrap_or(DEFAULT_GRAPHICS.cabin),
                contact: settings.contact.unwrap_or(DEFAULT_GRAPHICS.contact),
                grass: settings.grass.unwrap_or(DEFAULT_GRAPHICS.grass),
                windscreen: settings.windscreen.unwrap_or(DEFAULT_GRAPHICS.windscreen),
                marks: settings.marks.unwrap_or(DEFAULT_GRAPHICS.marks),
                motion_blur: settings.motion_blur.unwrap_or(DEFAULT_GRAPHICS.motion_blur),
                wet_roads: settings.wet_roads.unwrap_or(DEFAULT_GRAPHICS.wet_roads),
                detail: settings.detail.unwrap_or(DEFAULT_GRAPHICS.detail),
                mirrors: settings.mirrors.unwrap_or(DEFAULT_GRAPHICS.mirrors),
            },
            render_scale: settings.render_scale.unwrap_or(100),
            upscale_textures: settings.upscale_textures.unwrap_or(false),
            upscaler: rbr_render::upscale::Upscaler::find(&home.join(rbr_render::upscale::DIR)).is_some(),
            upscaler_download: Default::default(),
            present: settings.present.map_or_else(Present::default, Present::offered),
            max_fps: settings.max_fps.unwrap_or(0),
            fullscreen: settings.fullscreen.unwrap_or(true),
            triple: settings.triple.unwrap_or(false),
            triple_diagonal: settings.triple_diagonal.unwrap_or(TRIPLE_DIAGONAL_DEFAULT),
            triple_bezel: settings.triple_bezel.unwrap_or(TRIPLE_BEZEL_DEFAULT),
            triple_distance: settings.triple_distance.unwrap_or(TRIPLE_DISTANCE_DEFAULT),
            triple_angle: settings.triple_angle.unwrap_or(None),
            triple_monitors: settings.triple_monitors.clone(),
            monitors: Vec::new(),
            monitor: None,
            hdr: settings.hdr.unwrap_or(false),
            perf_log: settings.perf_log.unwrap_or(false),
            start_time: settings.start_time.unwrap_or_default(),
            wipers: settings.wipers.unwrap_or_default(),
            install_folder: settings.install_folder.clone().unwrap_or_default(),
            folder,
            disc_setup: Default::default(),
            adding,
            downloads: Default::default(),
            reload: false,
            ngp_physics: settings.ngp_physics.clone().unwrap_or_default(),
            ngp_dll: crate::ngp_dll::Choice::new(settings.ngp_dll.clone().unwrap_or_default()),
            // Filled by `list_cars`.
            ngp_revision: None,
            ngp_label: String::new(),
            ngp_cars: Vec::new(),
            marker,
            child: None,
            crash: Default::default(),
            status: String::new(),
            bindings: settings.bindings.clone(),
            ffb: settings.ffb.clone(),
            ffb_cars: settings.ffb_cars.clone(),
            ffb_load: settings.ffb_load.clone(),
            head_gasket: settings.head_gasket.unwrap_or(1),
            physics: settings.physics.unwrap_or_default(),
            session_physics: None,
            ffb_trial: crate::FfbTrial::default(),
            tyre_tuning: settings.tyre_tuning.unwrap_or_default(),
            controls: ControlsPage::new(),
            read_only: false,
        };
        form.list_cars();
        form.list_maps();
        form.stage_browser.restore(&settings.stage_browser);
        form.car_browser.restore(&settings.car_browser);
        form.rescan_codrivers();
        form
    }

    /// Writes the selection and the bindings to the settings file; whether it worked.
    pub fn save(&self) -> bool {
        if self.read_only {
            return false;
        }
        let mut viewer = read_settings(&self.paths.settings);
        move_renamed_keys(&mut viewer, &self.catalog.renamed);
        let settings = Settings {
            car: self.car().map(|c| c.name.clone()),
            map: Some(self.catalog.maps[self.map].number),
            recent_cars: self.recent_cars.clone(),
            recent_maps: self.recent_maps.clone(),
            favourite_cars: self.favourite_cars.clone(),
            favourite_maps: self.favourite_maps.clone(),
            stage_browser: self.stage_browser.state(),
            car_browser: self.car_browser.state(),
            weather: Some(match &self.weather_row {
                Some(section) => WeatherChoice::Row(section.clone()),
                None => WeatherChoice::Preset(self.options.weather),
            }),
            wetness: self.wetness,
            fog: self.fog,
            clouds: self.clouds,
            precipitation: self.precipitation,
            wear: Some(self.wear),
            tyre: Some(self.tyre),
            size: Some(self.size),
            fov: Some(self.fov),
            chase_fov: Some(self.chase_fov),
            volume: Some(self.volume),
            engine_volume: Some(self.engine_volume),
            codriver_volume: Some(self.codriver_volume),
            effects_volume: Some(self.effects_volume),
            tyres_volume: Some(self.tyres_volume),
            codriver: Some(self.codriver),
            codriver_voice: self.codriver_voice.clone(),
            codriver_style: Some(self.codriver_style.clone()),
            codriver_ranges: Some(self.codriver_ranges.clone()),
            codriver_pack: self.codriver_pack.clone(),
            notes_pack: self.notes_pack.clone(),
            notes_set: self.notes_set.clone(),
            my_pacenotes: Some(self.my_pacenotes),
            hud: Some(self.hud.show),
            mph: Some(self.hud.mph),
            fps: Some(self.hud.fps),
            pace_stack: Some(self.hud.stack),
            dials: Some(self.hud.dials),
            hud_hide: Some(self.hud.parts),
            hud_layout: Some(self.hud.layout),
            lighting: Some(self.graphics.lighting),
            particles: Some(self.graphics.particles),
            cabin: Some(self.graphics.cabin),
            contact: Some(self.graphics.contact),
            grass: Some(self.graphics.grass),
            windscreen: Some(self.graphics.windscreen),
            marks: Some(self.graphics.marks),
            motion_blur: Some(self.graphics.motion_blur),
            wet_roads: Some(self.graphics.wet_roads),
            detail: Some(self.graphics.detail),
            mirrors: Some(self.graphics.mirrors),
            render_scale: Some(self.render_scale),
            upscale_textures: Some(self.upscale_textures),
            present: Some(self.present),
            max_fps: Some(self.max_fps),
            fullscreen: Some(self.fullscreen),
            triple: Some(self.triple),
            triple_diagonal: Some(self.triple_diagonal),
            triple_bezel: Some(self.triple_bezel),
            triple_distance: Some(self.triple_distance),
            triple_angle: Some(self.triple_angle),
            triple_monitors: self.triple_monitors().cloned(),
            hdr: Some(self.hdr),
            perf_log: Some(self.perf_log),
            start_time: Some(self.start_time),
            wipers: Some(self.wipers),
            callout_distance: Some(self.marker.callout_distance),
            marker_threshold: Some(self.marker.marker_threshold),
            damage: Some(self.options.damage),
            auto_gears: Some(self.options.aids.auto_gears),
            gear_guard: Some(self.options.aids.gear_guard),
            neutral_lock: Some(self.options.aids.neutral_lock),
            clutch_help: Some(self.options.aids.clutch_help),
            ghost: Some(self.ghost.clone()),
            install_folder: Some(self.install_folder.trim().to_owned()).filter(|p| !p.is_empty()),
            ngp_physics: Some(self.ngp_physics.trim().to_owned()).filter(|p| !p.is_empty()),
            ngp_dll: Some(self.ngp_dll.path.trim().to_owned()),
            head_gasket: Some(self.head_gasket),
            physics: Some(self.physics),
            tyre_tuning: Some(self.tyre_tuning).filter(|t| !t.is_default()),
            bindings: self.bindings.clone(),
            // The strength, the smoothing and the curve too: the pause menu sets them while driving, the Controls
            // page writes them at once.
            ffb: rbr_render::ffb::Options {
                settings: rbr_gameplay::ffb::Settings { strength: viewer.ffb.settings.strength, ..self.ffb.settings },
                smoothing: viewer.ffb.smoothing,
                curve: viewer.ffb.curve,
                ..self.ffb.clone()
            },
            // The viewer owns these: carried over from the file as it is now, earlier car keys moved.
            ffb_cars: viewer.ffb_cars,
            ffb_load: viewer.ffb_load,
            camera: viewer.camera,
            gearboxes: self.gearboxes.clone(),
            setups: self.setup.choices.clone(),
        };
        std::fs::write(&self.paths.settings, settings.to_text())
            .map_err(|e| eprintln!("launcher: {}: {e}", self.paths.settings.display()))
            .is_ok()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// NGP 7.4.778's kept copy (with the real DLL: `RBR_NGP_DLL`, or the data's) fills both NGP paths while
    /// neither is set.
    /// Saved FFB sensitivities and camera nudges follow a car to its new key; one already there stays.
    #[test]
    fn saved_car_settings_move_to_the_new_keys() {
        let mut settings = Settings::parse(
            "[ffb_cars]\nrsf-24 = 90,100,110\nrsf-1 = 1,2,3\npack-b = 7,8,9\n[camera]\nrsf-24 chase = 0 0.5 -1\n",
        );
        let renamed = [("rsf-24", "pack-a"), ("rsf-1", "pack-b")].map(|(a, b)| (a.to_owned(), b.to_owned()));
        move_renamed_keys(&mut settings, &renamed);
        let ffb: Vec<(&str, [i32; 3])> = settings.ffb_cars.cars.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(ffb, [("pack-a", [90, 100, 110]), ("pack-b", [7, 8, 9])]);
        assert_eq!(settings.camera.cars.keys().collect::<Vec<_>>(), ["pack-a"]);
    }

    #[test]
    fn a_kept_ngp_fills_unset_paths() {
        let data = Data::from_env().unwrap_or_else(|| Data::new("", None));
        let Some(dll) = rbr_formats::ngp::path(&data) else {
            return;
        };
        let here = crate::test_dir("settings-kept-ngp");
        let kept = here.join(crate::ngp_install::KEPT);
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::copy(dll, kept.join("NGP.dll")).unwrap();
        std::fs::write(kept.join("physics.rbz"), b"archive").unwrap();
        let mut settings = Settings::parse("[launcher]\nngp_dll = \n");
        use_kept_ngp(&mut settings, &here);
        assert_eq!(settings.ngp_dll, Some(kept.join("NGP.dll").display().to_string()));
        assert_eq!(settings.ngp_physics, Some(kept.join("physics.rbz").display().to_string()));
        let mut chosen = Settings { ngp_physics: Some("/games/ngp/physics.rbz".into()), ..Settings::default() };
        use_kept_ngp(&mut chosen, &here);
        assert_eq!((chosen.ngp_dll, chosen.ngp_physics.as_deref()), (None, Some("/games/ngp/physics.rbz")));
        std::fs::remove_dir_all(&here).unwrap();
    }
    /// Saving carries the viewer's `[ffb_cars]`, `[ffb_load]` and strength over from the file as it
    /// is, leaving the form's own alone, earlier car keys moved.
    #[test]
    fn saving_carries_the_viewer_s_ffb_cars_over() {
        let here = crate::test_dir("settings-ffb-cars-carry");
        let settings = here.join(crate::SETTINGS_FILE);
        std::fs::write(&settings, "[launcher]\ncar = COROLLA\n[ffb_cars]\nrsf-24 = 90,100,110\n").unwrap();
        let catalog = Catalog {
            cars: vec![Car { name: "COROLLA".into(), ..Default::default() }],
            all_cars: Vec::new(),
            unlisted: Vec::new(),
            renamed: Vec::new(),
            maps: vec![catalog::Map {
                number: 54,
                stages: vec![catalog::Stage { name: "track-54_O".into(), map: 54, letter: 'O' }],
                origin: catalog::Origin::Retail,
            }],
        };
        let paths = Paths {
            data: here.join("data"),
            patch: None,
            viewer: PathBuf::from("/bin/viewer"),
            standalone: PathBuf::from("/bin/rbr-standalone"),
            settings: settings.clone(),
        };
        let season = Season { rows: Vec::new(), nice: Vec::new(), bad: Vec::new(), sky: None };
        let data = Data::new(here.join("data"), None);
        let mut form = Form::new(catalog, &read_settings(&settings), paths, data, season, install::Folder::default());
        assert_eq!(form.ffb_cars.cars, [("rsf-24".to_owned(), [90, 100, 110])].into());
        assert!(form.save());
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(text.contains("[ffb_cars]\nrsf-24 = 90,100,110\n"), "{text}");
        // The form's own edits do not overwrite the viewer's: the file's section is carried over.
        form.ffb_cars.cars.insert("COROLLA".into(), [120, 120, 130]);
        assert!(form.save());
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(text.contains("[ffb_cars]\nrsf-24 = 90,100,110\n"), "{text}");
        assert!(!text.contains("COROLLA"), "{text}");
        // The viewer's steering loads and strength are carried over the same way.
        let viewer = rbr_render::ffb::Loads::parse("[ffb_load]\nrsf-24 tarmac = 0,12,340\n");
        let file = rbr_formats::ini::set_value(&viewer.replace_in(&text), "ffb", "strength", "0.35");
        let file = rbr_formats::ini::set_value(&file, "ffb", "smoothing", "8");
        let file = rbr_formats::ini::set_value(&file, "ffb", "curve", "40");
        std::fs::write(&settings, file).unwrap();
        form.ffb.settings.strength = 0.8;
        form.ffb.sensitivity = [280, 250, 280];
        form.ffb_load = Default::default();
        assert!(form.save());
        let saved = read_settings(&settings);
        assert_eq!((saved.ffb.settings.strength, saved.ffb.sensitivity), (0.35, [280, 250, 280]));
        assert_eq!((saved.ffb.smoothing, saved.ffb.curve), (8, 40));
        assert_eq!(saved.ffb_load, viewer);
        // A car's earlier key is written as its key now: the viewer reads the file.
        form.catalog.renamed = vec![("rsf-24".into(), "pack-evo".into())];
        assert!(form.save());
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(text.contains("[ffb_cars]\npack-evo = 90,100,110\n") && !text.contains("rsf-24"), "{text}");
        assert!(text.contains("[ffb_load]\npack-evo tarmac = 0,12,340\n"), "{text}");
        std::fs::remove_dir_all(&here).unwrap();
    }
}
