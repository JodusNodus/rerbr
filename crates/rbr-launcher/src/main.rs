//! The launcher (ReRBR's): one window to pick a car, map, environment, resolution and field of view, and Start.
//! It derives the stage variant and render wetness from the 1.02 season table, then runs the viewer with
//! `rbr-standalone --live --boot`. The viewer and `rbr-standalone` run as child processes.
//!
//! ```text
//! rerbr [--data DIR] [--viewer PATH] [--standalone PATH] [--list] [--command]
//!     [--screenshot DIR [--screenshot-damage FILE]]
//! ```
//!
//! The game is the RBR install folder the Settings page sets (`install_folder` in `launcher-settings.ini`, `install`):
//! RBR with NGP, read in place: its game data, NGP's physics, and its plugins and community content when it has
//! them (a RallySimFans install has both). Without a folder the window asks for it, offering the first
//! usual install place with NGP (`install::detect`). `--data` (or `RBR_DATA`) and
//! `RBR_PATCH` override its game data and 1.02 patch files, the Settings page's Advanced field its NGP archive;
//! the viewer and `rbr-standalone` get the game folder as `RBR_DATA`, `RBR_PATCH` and `RBR_RSF`.
//!
//! `--command` prints the viewer command Start runs with the saved selection (`RBR_DATA=` and `RBR_PATCH=`, the
//! program, then one argument a line) and exits, for starting it unattended. `--list`, `--command` and `--screenshot`
//! never write the settings file.
//!
//! Good, random and bad are the game's weather presets, and the Drive page's Environment offers one entry per season
//! row of the map besides (the game's own weathers for the stage, exactly as authored; a saved row that is not the
//! map's falls back to good). Random draws the season row and the road wear as the game's `E74_FindBest` does, from
//! the CRT's `rand()` stream seeded with the time when the launcher starts (the game seeds it
//! at its start too; the menus' own `rand()` calls between are not modelled, ours); the Drive page shows the next
//! draw, and each Start takes it. A preset chooses the stage variant and dry, damp or wet render
//! folder; map 68's one row is in neither list, so the original stops at its int3 for good and bad; ours gives it that row. Road wetness overrides the row's (the
//! viewer's road, the auto tyre and `rbr-standalone --wetness` follow); road wear is the `.mat` road wear (random
//! draws it, a specific row takes the chosen wear); the time of day is ours, not the stage's baked light; fog is ours
//! over the stage (`viewer --fog`). The viewer draws the row's exact `TrackSettings.ini` section (`viewer --sky`).
//! Road wear defaults to new, and the tyre
//! defaults to `E74_WeatherKind` for the map surface and derived wetness. `--list` prints the startable map count and
//! any unavailable-map reasons without opening a window. The field of view is the horizontal one of the in-car views and
//! the free-fly camera (`viewer --fov`), 30 to 120 degrees, default 50; `viewer --chase-fov` sets the chase view's, default 65. The viewer keeps
//! the view last chosen and each car's camera nudges in the settings file's `[camera]` section (`viewer --cameras`,
//! `rbr_render::camera_prefs`); the launcher carries that section over when it writes the file, and the Cameras row's
//! "Reset this car's cameras" drops the selected car's nudges. The sound volume
//! (`viewer --volume`) is linear, 0 to 4, default 1 (the game's). The Co-driver dropdown turns the pace notes off
//! (`viewer --no-codriver`); the sound's rain is the viewer's, by the row's section (`--sky`). The co-driver voice,
//! call style, distance calls and stage notes come from a co-driver pack copied into the player's folder
//! (`codriver_packs`: `codrivers/` and `notes/`, `launcher --import-codriver SRC`) or from the RallySimFans
//! install's Pacenote plugin and MyPacenotes, read in place; without either the original 1.02 voice drives. The
//! launcher plays the
//! service park's ambience, at the sound volume, while a championship is at a service stop (its page and the Setup
//! page; `rbr_sound::menu`), and nothing while the viewer runs. The game's music is not played (user decision).
//! The Ghost field races a ghost car in the quick race (`viewer --ghost`): off, the best one (`--best-ghost
//! GhostCars/Best_<map:02>.ghc`, replaced when a finished drive beats it, as `BeginWrite` keeps the faster run; ours:
//! the game keeps it in memory and saves on request), or a saved `GhostCars/<name>_<map:02>.ghc` of the stage's map.
//! The Graphics & sound page's Quality section sets the renderer's additions beyond the game's look (`viewer --enhance`,
//! `--reflections`, `--detail`, `--mirrors`; `graphics_page::Graphics`, one `launcher-settings.ini` key each), by hand
//! or by preset; the free ones are always on. The game's own quality settings are not offered: the stage always draws
//! at the highest (user decision).
//! The Time of day box draws a quick race's stage at a start time on its rally's date (`viewer --clock`, `start_time`
//! in the settings): the sun, its light and shadows, dusk and night lit by the car's lamps; the start board shows it
//! (`rbr-standalone --start-time`). The choice is a moment of the day solved on the rally's date (`time_of_day`, with the
//! sun's path drawn under the box), golden hour by default (user decision), or an exact clock; As baked is the sun the
//! stage was baked with (the viewer gives the start board its hour), and a championship stage always has it, with the
//! season's start clock.
//! The Wipers box is the wiper switch at the start (`viewer --wipers`: off, intermittent, continuous, or auto by the
//! weather and the mud on the glass); the wiper key (I by default, in the Controls section) steps it while driving.
//! The headlight key (L by default) switches the headlights and tail lights off and on.
//! The physics is NGP's (`rbr-standalone --ngp-physics`, from the install's `physics.rbz`; user decision: the game
//! steps NGP physics only). The cars are
//! those NGP drives (`catalog::ngp_offered`): the retail slots its archive has NGP cars in (`catalog::ngp_cars`), and
//! the cars of the game folder's cache (`rsfdata/cache/cars.json`, with their group), raced from their packs as
//! RSF installs them in slot 5 (`--rsf-car`, `rbr_formats::rsf`); the
//! retail slot 5 is then left out (it is the RSF car last picked). The cars downloaded from gvrc.de's NGP car
//! packs ("Download cars…" on the Settings page: `car_packs`, in `cars/` of the player's folder) are raced the
//! same way; a car RSF also has
//! is listed once, the added one. Cars without a
//! pack are listed as unavailable with the reason (also by `--list`); cars without their model folder use the retail
//! model RSF names as their fallback. The stage records, personal bests and ghosts are kept in the game's files
//! under `NGP/` in the player's folder (`home`; `NGP/SavedGames/pfresult.rst`, `NGP/SavedGames/pf<name>.rbr`,
//! `NGP/GhostCars/`; ours: the game has no NGP convention and RSF keeps its records online). RSF ghosts (`.gch`
//! files) are not supported: every race offers the game's own ghosts above.
//! The Stage and Car fields' Browse… open the stage browser (`stage_browser`) and the car browser (`car_browser`) in
//! place of the page's body. The stage browser has collections (favourites, recent, all, each country with the 1.02
//! and the RallySimFans stages together, those that cannot start), the surface and length filters, sorting, a search
//! over every stage, and the stage's picture and weathers. The car browser has the classes newest to oldest (the 1.02
//! cars in theirs), a drive filter, a search and the car turning; a car that cannot start stays in its
//! class, greyed, with the reason. The stages and cars starred in them are one click away under the fields
//! (`favourite_maps`, `favourite_cars`); the Championship page's new-season car is chosen in the car browser too. The
//! last selection, the recent picks (`recent_cars`, `recent_maps`) and the favourites are kept in
//! `launcher-settings.ini` in the player's folder; a saved car not offered gives way to the most recent one that is.
//! The maps add the game folder's community stages (`rbr_formats::rsf`); one the launcher cannot start
//! (an encrypted one) stays listed with the reason. Until saved, the NGP field defaults to `$RBR_NGP_PHYSICS`.
//!
//! The tabs follow what the player wants to do (ours, after Content Manager): Drive (car and stage, conditions, race
//! options, the previews and Start), Championship, Car setup, Replays (saved drives, playback and video export),
//! Controls, Graphics & sound (window, camera, HUD and pace notes, sound, the renderer's additions) and Settings
//! (install folder, advanced paths, about). The status line under every page says what the install lacks
//! (the programs, NGP's archive, a car NGP drives), with its fix
//! (`Form::readiness`); the Drive page
//! says what else blocks Start (`Form::blocker`). The pages change only where a field is shown: the settings file and
//! the viewer command are those of the earlier pages (`command::tests::the_viewer_command_follows_the_saved_settings`).
//!
//! The Controls page (`controls_page`) sets the live mode's bindings (`rbr_render::bindings`), the wheel, pedals and
//! shifter first, the keyboard's keys collapsed below: a device input (`rbr_render::devices`) and a
//! key for each driving action. Click a binding, then move a wheel axis or press a button, or press a key; Escape cancels.
//! The launcher opens on it when a gamepad is connected and nothing drives from one yet. They are kept in the same
//! file's `[controls]` section, saved on every change, and the viewer reads that file (`viewer --bindings`); the
//! viewer, not the launcher, reads the inputs while driving. The advanced H-shifter
//! (`rbr_render::shifter`) and the four driver aids are set there too. With it on, the Drive page's Gearbox row picks
//! each car's gearbox (sequential until picked; `[gearbox]` in the settings file): for an H-pattern one the launcher
//! turns clutch help and the automatic gearbox off and passes the gearbox and the grind sample from RBRControls'
//! folder in the game folder (`command::shifter_args`); a sequential car keeps its aids and the shifter stays off.
//!
//! The menu's race options (`rbr_gameplay::race_options`): the Drive page's damage level, which also sets the
//! outside-assistance penalty, and the Controls page's four driver aids, passed as `rbr-standalone --damage`,
//! `--auto-gears`, `--gear-guard`, `--neutral-lock` and `--clutch-help`.
//!
//! The Car setup page edits the car's setup for the map's surface (`setup_page`): the values the game's setup pages edit,
//! within the car's ranges and by its steps, with named setups in the game's format. A changed setup goes to
//! `rbr-standalone --setup`. `--screenshot DIR` draws the pages, and the Car setup page's areas, offscreen
//! to PNGs and exits.
//!
//! The Championship page (`rally_page`) runs the season: new season or continue (the profile's save in
//! `SavedGames/`), each rally stage by stage with the service park between stages, the AI drivers' times, the rally
//! and season standings, the bulletins and the trials between rallies. New season takes the championship's own
//! options (the team, skill level and car damage, kept in `launcher-settings.ini` as `championship_team`,
//! `championship_skill` and `championship_damage`); the season drives its team's car when NGP drives it, else a car
//! picked for it (ours; each profile's in the settings file's `[season]` section, so the game's save stays the
//! game's), and its stages
//! run as the game's championship on the season's (`rbr-standalone --championship`) with the Controls page's aids.
//! `--screenshot` also draws it, and with `--screenshot-damage FILE` (`rbr-standalone --damage-out`) its service park on that car.

// No console window on Windows: stderr goes to the log file (`rbr_render::console`).
#![windows_subsystem = "windows"]

mod archive;
mod browser;
mod browser_state;
mod btb;
mod capture;
mod car_browser;
mod car_packs;
mod catalog;
mod codriver_packs;
mod codriver_page;
mod command;
mod controls_page;
mod crash;
mod disc_install;
mod drive_page;
mod ghosts;
mod graphics_page;
mod gvrc;
mod home;
mod install;
mod job;
mod ngp_dll;
mod ngp_install;
mod pages;
mod preview;
mod rally_page;
mod replay_page;
mod settings;
mod settings_page;
mod setup_page;
mod stage_browser;
mod time_of_day;
mod upscaler;
mod window;

use std::path::{Path, PathBuf};
use std::process::{Child, ExitCode};

use catalog::{Catalog, TyreChoice};
use controls_page::ControlsPage;
use ghosts::GhostChoice;
use pages::Page;
use rally_page::RallyPage;
use rbr_formats::data::Data;
use rbr_gameplay::crt::CrtRand;
use rbr_gameplay::markers::MarkerSettings;
use rbr_gameplay::race_options::RaceOptions;
use rbr_gameplay::weather::Season;
use rbr_render::bindings::Bindings;
use rbr_render::glass::model::Switch;
use rbr_sound::menu::MenuPlayer;
use settings::{load, or_folder, read_settings, Gearboxes, SeasonCars};
use setup_page::SetupPage;
use window::App;
use winit::event_loop::EventLoop;

/// A fresh empty scratch folder for a test, unique per `name` (with the process id): names stay distinct so
/// parallel tests never share one, and reruns start empty.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbr-launcher-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SETTINGS_FILE: &str = "launcher-settings.ini";
/// The folder in the player's folder with NGP's own `SavedGames/` and `GhostCars/` (ours).
const NGP_RECORDS: &str = "NGP";
/// `--help`'s text.
const USAGE: &str = "usage: rerbr [OPTIONS]
Set your RBR install folder (with NGP; RallySimFans has both) on the Settings page, or at the first start; it is
only read. From it: the game data at its top, NGP's physics.rbz, and its plugins and community content when it has
them (rsfdata/, Maps/<id>-<name>/, Cars/).
  --data DIR                the game data instead of the install folder's (default $RBR_DATA; $RBR_PATCH: the 1.02
                            patch files)
  --viewer PATH             the viewer (default: next to the launcher)
  --standalone PATH         rbr-standalone (default: next to the launcher)
  --list                    print the startable maps and cars, the unavailable maps and cars, and what the install lacks, and exit
  --command                 print the viewer command Start runs with the saved selection (RBR_DATA= and RBR_PATCH=,
                            the program, one argument a line), and exit
  --screenshot DIR          draw the pages to PNGs in DIR, and exit
  --screenshot-damage FILE  with --screenshot: also the service park on this damage file's car
  --install-from-disc DISC NGP|- OUT
                            set up a game folder OUT (new or empty) from the disc (.iso or folder) and the NGP
                            package (.7z or folder; - downloads it from gvrc.de), and exit
  --extract-test-data DISC PATCH OUT
                            write the test data (docs/development.md) to OUT (new or empty) from the disc and the
                            1.02 patch: OUT/retail-install for RBR_DATA and OUT/Main_group for RBR_PATCH, and exit
  --download-ngp            download NGP 7.4.778 from gvrc.de and keep its NGP.dll and physics.rbz in your files'
                            NGP 7.4.778 folder (no game folder is changed), and exit
  --download-cars [NAME...] download the NGP cars NAME (part of a name from gvrc.de's list) from gvrc.de and add them
                            to the cars folder of your files, and exit; without names, print the list
  --import-codriver SRC     copy a co-driver download SRC (an extracted folder, a .zip/.7z archive, or a whole
                            RBR/RSF install to copy from, only read) into your files' codrivers/ and notes/, and exit
  -V, --version             print the version
  -h, --help                print this text
Your files (settings, saves, records, ghosts, setups, added cars) are kept in ~/Library/Application Support/ReRBR on
macOS, $XDG_DATA_HOME/rerbr (default ~/.local/share/rerbr) on Linux, or $RERBR_HOME; the settings in
launcher-settings.ini there. --list, --command and --screenshot do not write it.
The Settings page's Advanced field overrides the install folder's NGP physics.rbz; until saved it comes
from RBR_NGP_PHYSICS. RBR_RSF is an unsaved developer override: the game folder in use when set, for pairing
extracted retail data with sample community packs.";

/// The game's data and 1.02 patch files (`rbr_formats::data`), the programs and the settings file.
struct Paths {
    data: PathBuf,
    patch: Option<PathBuf>,
    viewer: PathBuf,
    standalone: PathBuf,
    settings: PathBuf,
}

impl Paths {
    /// The game folder in use: `RBR_RSF` when set (an unsaved developer override pairing the game data with
    /// sample community packs), else the game data folder (`--data` or `RBR_DATA` when overridden, the install
    /// folder otherwise). The plugins and the community content are read from it.
    pub(crate) fn game_root(&self) -> String {
        std::env::var("RBR_RSF")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| self.data.display().to_string())
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("launcher: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--version" || a == "-V") {
        println!("rerbr {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--install-from-disc") {
        return disc_install::cli(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "--extract-test-data") {
        return disc_install::test_data_cli(&args[1..]);
    }
    let here = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    let home = home::dir(&here);
    let notice = home::migrate(&here, &home).map_err(|e| format!("copying your files to {}: {e}", home.display()))?;
    // `migrate` makes it only when there are old files; the settings are written into it without making it.
    let _made = std::fs::create_dir_all(&home).is_ok();
    #[cfg(windows)]
    if _made {
        rbr_render::console::log_to(&home.join(rbr_render::console::LOG_FILE));
    }
    if args.first().is_some_and(|a| a == "--download-ngp") {
        notice.iter().for_each(|n| eprintln!("launcher: {n}"));
        return gvrc::ngp_cli(&home, &args[1..]);
    }
    if args.first().is_some_and(|a| a == "--download-cars") {
        notice.iter().for_each(|n| eprintln!("launcher: {n}"));
        return gvrc::cars_cli(&home.join(car_packs::DIR), &args[1..]);
    }
    if args.first().is_some_and(|a| a == "--import-codriver") {
        notice.iter().for_each(|n| eprintln!("launcher: {n}"));
        return codriver_packs::cli(&home, &args[1..]);
    }
    let (start, list, command, screenshot, rally_damage) = parse_args(std::env::args().skip(1), &here, &home)?;
    let settings = read_settings(&start.settings);
    if !list && !command && screenshot.is_none() {
        // A panic of the window's is shown at the next start (`crash`).
        crash::hook(home.join(home::LOGS));
        // The window opens without a game too, to choose its folder.
        let mut form = load(&start, settings.clone()).map_err(|e| eprintln!("launcher: {e}")).ok();
        if let (Some(form), Some(notice)) = (&mut form, &notice) {
            form.status = notice.clone();
        }
        if let Some(form) = &mut form {
            form.crash.report = crash::last_crash(&home.join(home::LOGS));
        }
        let folder = settings.install_folder;
        let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
        let mut app = App::new(start, form, folder, notice);
        return event_loop.run_app(&mut app).map_err(|e| e.to_string());
    }
    notice.iter().for_each(|n| eprintln!("launcher: {n}"));
    let mut form = match (load(&start, settings.clone()), &screenshot) {
        (Ok(form), _) => form,
        // Without a game, the page that asks for it.
        (Err(e), Some(dir)) => return App::new(start, None, settings.install_folder, None).screenshots(&e, dir),
        (Err(e), None) => return Err(e),
    };
    // Only the window writes the settings file.
    form.read_only = true;
    if list {
        let (catalog, season) = (&form.catalog, &form.season);
        println!("Startable maps: {}/{}", catalog.startable_maps(season).len(), catalog.maps.len());
        for map in &catalog.maps {
            match catalog.availability(season, map) {
                Ok(_) => println!("Startable: {}", catalog::map_label(map)),
                Err(reason) => println!("Unavailable: {} — {reason}", catalog::map_label(map)),
            }
        }
        form.log_missing_programs();
        if let Some(issue) = form.no_ngp() {
            println!("{issue}");
        }
        for car in &catalog.cars {
            if let Some(reason) = car.unavailable() {
                println!("Unavailable car: {} — {reason}", catalog::car_label(car));
                continue;
            }
            println!("Startable car: {}", catalog::car_label(car));
            if let Some(added) = car.community.as_ref().filter(|r| r.added) {
                let model = match &added.model {
                    Ok(Some(stand_in)) => format!(" — model: {stand_in}"),
                    _ => String::new(),
                };
                let pack = added.pack.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                println!("Added car: {} — {pack}{model}", catalog::car_label(car));
            } else if let Some(Ok(Some(stand_in))) = car.community.as_ref().map(|r| &r.model) {
                println!("RSF car: {} — model: {stand_in}", catalog::car_label(car));
            }
        }
        for pack in &form.owned_voices {
            match &pack.problem {
                Some(problem) => println!("Co-driver pack: {} — not ready: {problem}", pack.name),
                None => println!(
                    "Co-driver pack: {} — {} voices, {} call styles, {} range sets",
                    pack.name,
                    pack.voices.len(),
                    pack.styles.len(),
                    pack.ranges.len()
                ),
            }
        }
        for pack in &form.owned_notes {
            println!("Stage-note pack: {} — {} files", pack.name, pack.files);
        }
        return Ok(());
    }
    if command {
        // What Start runs with the saved selection, without starting it: the game's paths, the program, then one
        // argument a line.
        let mut rand = form.rand;
        let args = form.start_args(&mut rand).ok_or_else(|| format!("cannot start: {}", form.status))?;
        form.child_env().iter().for_each(|(key, value)| println!("{key}={}", value.display()));
        println!("{}", form.paths.viewer.display());
        args.iter().for_each(|a| println!("{a}"));
        return Ok(());
    }
    if let Some(dir) = screenshot {
        capture::pages(&mut form, &dir, rally_damage)?;
    }
    Ok(())
}

/// The programs, the settings file and the game's overrides of the install folder: `--data` (or `RBR_DATA`) and
/// `RBR_PATCH`.
#[derive(Clone)]
struct Start {
    viewer: PathBuf,
    standalone: PathBuf,
    settings: PathBuf,
    data: Option<PathBuf>,
    patch: Option<PathBuf>,
}

/// The start, `--list`, `--command`, `--screenshot` and `--screenshot-damage`.
type Parsed = (Start, bool, bool, Option<PathBuf>, Option<PathBuf>);

/// The programs default to `here`'s, the settings file is `home`'s (`home::dir`).
fn parse_args(mut args: impl Iterator<Item = String>, here: &Path, home: &Path) -> Result<Parsed, String> {
    let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut data = env("RBR_DATA");
    let exe = |name| here.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let mut viewer = exe("viewer");
    let mut standalone = exe("rbr-standalone");
    let (mut list, mut command) = (false, false);
    let mut screenshot = None;
    let mut rally_damage = None;
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            println!("{USAGE}");
            std::process::exit(0);
        }
        if flag == "--list" || flag == "--command" {
            list |= flag == "--list";
            command |= flag == "--command";
            continue;
        }
        let value = PathBuf::from(args.next().ok_or(format!("{flag} needs a value"))?);
        match flag.as_str() {
            "--data" => data = Some(value),
            "--viewer" => viewer = value,
            "--standalone" => standalone = value,
            "--screenshot" => screenshot = Some(value),
            "--screenshot-damage" => rally_damage = Some(value),
            _ => return Err(format!("unknown argument {flag}\n{USAGE}")),
        }
    }
    let start = Start { viewer, standalone, settings: home.join(SETTINGS_FILE), data, patch: env("RBR_PATCH") };
    Ok((start, list, command, screenshot, rally_damage))
}

/// The selection and the started viewer.
struct Form {
    catalog: Catalog,
    paths: Paths,
    data: Data,
    season: Season,
    page: Page,
    setup: SetupPage,
    rally: RallyPage,
    /// The Drive page's stage and car previews.
    previews: preview::Previews,
    car: usize,
    map: usize,
    /// The car and stage browsers, the recent picks (car names, map numbers) and the starred ones (car names and map
    /// numbers, in the order starred).
    car_browser: car_browser::CarBrowser,
    stage_browser: stage_browser::StageBrowser,
    replays: replay_page::Replays,
    recent_cars: Vec<String>,
    recent_maps: Vec<u32>,
    favourite_cars: Vec<String>,
    favourite_maps: Vec<u32>,
    /// The car a new season takes, and each profile's season car (`[season]`).
    season_pick: usize,
    season_cars: SeasonCars,
    /// Each car's gearbox for the advanced H-shifter (`[gearbox]`).
    gearboxes: Gearboxes,
    /// The weather preset, the damage level and the driver aids.
    options: RaceOptions,
    /// The random weather's `rand()` stream (`srand(time)` at the launcher's start).
    rand: CrtRand,
    /// The Drive page's specific season row (its `TrackSettings.ini` section); none for the preset in `options`.
    weather_row: Option<String>,
    /// The road wetness over the weather row's (`rbr-standalone --wetness`); none takes it from the weather.
    wetness: Option<u32>,
    /// Our fog over the stage (`viewer --fog`); none takes it from the weather.
    fog: Option<settings::Fog>,
    /// Our cloud cover and our rain or snow over the stage (`viewer --clouds`, `--precipitation`); none takes them
    /// from the weather.
    clouds: Option<settings::Clouds>,
    precipitation: Option<settings::Precipitation>,
    /// Whether the Drive page shows the Custom weather's own controls.
    weather_adjust: bool,
    wear: u32,
    tyre: TyreChoice,
    size: (u32, u32),
    sizes: Vec<(u32, u32)>,
    /// Every view's horizontal field of view, degrees.
    fov: f32,
    /// The chase view's horizontal field of view, degrees.
    chase_fov: f32,
    /// The in-car view's head (`[camera] head`, `rbr_render::head`), written when changed: the viewer owns the section.
    head: bool,
    /// The sound volume, linear (`viewer --volume`).
    volume: f32,
    /// The sound group volumes, linear (`viewer --volumes E,C,F,T`).
    engine_volume: f32,
    codriver_volume: f32,
    effects_volume: f32,
    tyres_volume: f32,
    /// The co-driver calls the pace notes.
    codriver: bool,
    /// The co-driver mod: the active Pacenote plugin (the chosen local pack, else the game folder's; none
    /// without either), the voice chosen (none: the plugin's, `command::ORIGINAL_VOICE`: 1.02's), the style and
    /// range set, and the stage's MyPacenotes notes.
    codriver_mods: Option<command::CodriverMods>,
    /// The locally owned voice pack (`codriver_packs::VOICES_DIR`), or the game folder's plugin when unset.
    codriver_pack: Option<String>,
    /// The locally owned stage-note pack (`codriver_packs::NOTES_DIR`), or the game folder's notes when unset.
    notes_pack: Option<String>,
    /// The note set chosen in that source (`codriver_packs::NoteSet`); unset or gone: the source's largest.
    notes_set: Option<String>,
    /// The game folder's MyPacenotes note sets, re-read by `rescan_codrivers`.
    rsf_note_sets: Vec<codriver_packs::NoteSet>,
    /// Whether the game folder holds a Pacenote plugin (offered as a source); refreshed by the rescans, never drawn.
    rsf_plugin: bool,
    /// The game folder's plugin voices, re-read by `rescan_codrivers`: the Co-driver dropdown lists them beside
    /// the owned packs' (no directory reads while drawing).
    rsf_voices: Vec<String>,
    /// Whether the active plugin's listed styles (and range sets) actually say different things, re-read by
    /// `rescan_codrivers`: with one distinct result the row hides (the voice decides the words).
    codriver_styles_differ: bool,
    codriver_ranges_differ: bool,
    /// What the co-driver says on the cached map; recomputed when the map, the notes sources, the co-driver switch
    /// or voice, or an import changes it, never per draw.
    cached_notes_map: u32,
    notes_summary: codriver_packs::NotesSummary,
    cached_notes_dirty: bool,
    /// The owned packs, re-read by `rescan_codrivers`.
    owned_voices: Vec<codriver_packs::VoicePack>,
    owned_notes: Vec<codriver_packs::NotePack>,
    /// "Import co-driver…" and its last result (`codriver_packs`).
    codriver_job: Option<job::Job<Vec<String>>>,
    codriver_status: Option<Result<String, String>>,
    codriver_voice: Option<String>,
    codriver_style: String,
    codriver_ranges: String,
    my_pacenotes: bool,
    /// The quick race's ghost car.
    ghost: GhostChoice,
    /// The menus' sound, once something has played (`None` without an audio device).
    menu_sound: Option<Option<MenuPlayer>>,
    /// The HUD shown, its speed unit, and the FPS counter.
    hud: graphics_page::HudChoice,
    /// The Quality section's choices.
    graphics: graphics_page::Graphics,
    /// The render scale, percent ([`graphics_page::RENDER_SCALES`]).
    render_scale: u32,
    /// The car's and the stage's textures upscaled (`viewer --upscale-textures`), when [`Form::upscaler`].
    upscale_textures: bool,
    /// Whether the upscaler is installed ([`rbr_render::upscale::Upscaler::find`]), looked for at the start and after
    /// its download.
    upscaler: bool,
    /// "Download upscaler…" ([`upscaler`]).
    upscaler_download: upscaler::Download,
    /// How the driving window presents its frames (`viewer --present`).
    present: rbr_render::present::Present,
    /// The driving window's frames a second at most (`viewer --max-fps`; 0: none), one of
    /// [`graphics_page::FRAME_CAPS`].
    max_fps: u32,
    /// The driving window fills the monitor (`viewer --fullscreen`); `size` applies when off.
    fullscreen: bool,
    /// The viewer writes a performance log to the player's `logs` folder (`viewer --perf-log`).
    perf_log: bool,
    /// The Custom weather's start time (`viewer --clock`).
    start_time: time_of_day::StartTime,
    /// The wiper switch at the start (`viewer --wipers`).
    wipers: Switch,
    /// The RBR install folder's field, and what the form was loaded from (`install`).
    install_folder: String,
    folder: install::Folder,
    /// The install folder's "Set up from the disc…" panel.
    disc_setup: disc_install::DiscSetup,
    /// "Download cars…" and the added cars (`car_packs`).
    adding: car_packs::Adding,
    /// The Settings page's downloads from gvrc.de: NGP and cars (`gvrc`).
    downloads: gvrc::Downloads,
    /// Set when the install folder changed: the window loads the form again.
    reload: bool,
    /// NGP's `physics.rbz` field (empty: the install folder's).
    ngp_physics: String,
    /// Selected DLL override, or a link to the installed plugin.
    ngp_dll: ngp_dll::Choice,
    /// The archive's NGP revision (none: not NGP's), its name (`install::ngp_label`), and the car numbers it has NGP
    /// cars in (`catalog::ngp_cars`).
    ngp_revision: Option<u32>,
    ngp_label: String,
    ngp_cars: Vec<u32>,
    marker: MarkerSettings,
    child: Option<Child>,
    /// The viewer's log, and the report of a run that failed.
    crash: crash::Watch,
    status: String,
    bindings: Bindings,
    /// The wheel's force feedback in live mode (the settings file's `[ffb]` section).
    ffb: rbr_render::ffb::Options,
    /// Each car's own force-feedback sensitivity (the settings file's `[ffb_cars]` section).
    ffb_cars: rbr_render::ffb::CarSensitivity,
    /// NGP's `headGasketDurability` for every drive (1 leaves the engine as it is).
    head_gasket: i32,
    controls: ControlsPage,
    /// Never writes the settings file (`--command`, `--screenshot`).
    read_only: bool,
}

impl Form {
    /// Fills the stage browser with the maps and their availability.
    fn list_maps(&mut self) {
        let catalog = &self.catalog;
        let rows = (catalog.maps.iter().enumerate())
            .map(|(i, map)| stage_browser::StageRow::new(i, map, catalog.availability(&self.season, map).err()))
            .collect();
        self.stage_browser.set_rows(rows);
    }

    /// Offers the cars NGP's archive drives, keeping the selected ones when they are still there, and fills the car
    /// lists (the Drive page's and the season's) and NGP's name.
    fn list_cars(&mut self) {
        let archive = self.ngp_archive();
        self.ngp_revision = install::ngp_revision(Path::new(&archive));
        self.ngp_label = install::ngp_label(self.ngp_revision);
        self.ngp_cars = catalog::ngp_cars(Path::new(&archive));
        let name = self.car().map(|c| c.name.clone());
        let season = self.catalog.cars.get(self.season_pick).map(|c| c.name.clone());
        self.catalog.offer(&self.ngp_cars);
        self.car = settings::pick_car(&self.catalog.cars, name.iter().chain(&self.recent_cars));
        self.season_pick = settings::pick_car(&self.catalog.cars, season.iter().chain(&self.recent_cars));
        let rows = self.catalog.cars.iter().enumerate().map(|(i, car)| car_browser::CarRow::new(i, car)).collect();
        self.car_browser.set_rows(rows);
    }

    /// The selected car, none when NGP drives none.
    fn car(&self) -> Option<&catalog::Car> {
        self.catalog.cars.get(self.car)
    }

    /// NGP's `physics.rbz`: the field's, else the install folder's.
    fn ngp_archive(&self) -> String {
        or_folder(Some(&self.ngp_physics), &self.folder.ngp_physics)
    }

    /// The game folder in use: the viewer and `rbr-standalone` read the game, its plugins and its community
    /// content from it.
    fn game_root(&self) -> String {
        self.paths.game_root()
    }

    /// The viewer's and `rbr-standalone`'s game (`rbr_formats::data::Data::from_env`): the game folder in use
    /// (`RBR_DATA`), the 1.02 patch (`RBR_PATCH`), its plugins and community content (`RBR_RSF`), and the
    /// NGP DLL (`RBR_NGP_DLL`).
    fn child_env(&self) -> Vec<(&'static str, PathBuf)> {
        let patch = self.paths.patch.clone().map(|p| ("RBR_PATCH", p));
        let root = self.game_root();
        let dll = self.ngp_dll.resolved(&self.data, &root).map(|p| ("RBR_NGP_DLL", p));
        [("RBR_DATA", self.paths.data.clone())]
            .into_iter()
            .chain(patch)
            .chain([("RBR_RSF", PathBuf::from(root))])
            .chain(dll)
            .collect()
    }

    /// The player's folder (`home`), where the settings and the saved setups are.
    fn here(&self) -> PathBuf {
        self.paths.settings.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The viewer's performance logs' folder (`home::LOGS`).
    fn logs_dir(&self) -> PathBuf {
        self.here().join(home::LOGS)
    }

    /// The added cars' folder (`car_packs`).
    fn cars_dir(&self) -> PathBuf {
        self.here().join(car_packs::DIR)
    }

    /// Where the stage records, personal bests and ghosts are: the player's folder's `NGP/` (ours), apart from the
    /// 1.02 ones earlier versions kept beside it.
    fn records(&self) -> PathBuf {
        self.here().join(NGP_RECORDS)
    }

    /// Points the Setup page at the selected car and the map's surface.
    fn select_setup(&mut self) {
        let surface = setup_page::SURFACES[self.catalog.maps[self.map].surface() as usize];
        let here = self.here();
        let archive = self.ngp_archive();
        let root = PathBuf::from(self.game_root());
        if let Some(car) = self.catalog.cars.get(self.car) {
            self.setup.select(&here, &root, car, Path::new(&archive), surface);
        }
    }
}
