//! Command dispatch of the `rbr-dirt-rally` executable.
use crate::install::Install;
use std::path::{Path, PathBuf};

pub(crate) const HELP: &str = "rbr-dirt-rally: convert stages of a locally installed DiRT Rally to BTB stages for ReRBR

Usage:
  rbr-dirt-rally list [INSTALL]
  rbr-dirt-rally add-car CAR [--livery N] [--cars DIR] [--install INSTALL]
  rbr-dirt-rally car CAR --class CLASS --template CARFOLDER --out DIR [--livery N] [--name NAME] [--install INSTALL]
  rbr-dirt-rally import LOCATION/AREA/route_N [--install INSTALL] [--name NAME] [--out DIR] [--replace]
         [--draw-corridor M] [--collision-corridor M] [--lod M] [--no-preview]
  rbr-dirt-rally preview STAGE_FOLDER... [--viewer PATH] [--maps DIR]

import renders the stage's preview picture afterwards with the `viewer` beside this program (and the install's Maps folder,
$RBR_DATA/Maps): a failure there is reported and does not fail the import. preview makes or remakes it for stages already
converted (`preview.png` and `track.ini`'s splashscreen).

INSTALL is the game folder (or its data folder); without it, the Steam libraries are searched.
Source game files are read in place. CAR is the folder under cars/models (cme); the livery is its first without
--livery. add-car adds it to the launcher's cars (DIR, the player's cars/ without --cars) as a car of its own, with the
physics of the gvrc.de car of its model, added before (cme, 131, str, m3r, mta, 20b, ers, 555, 306, imp, for, c4r,
mer, sti, mcr, ffr, pol). car writes its model alone: CLASS its generic textures under cars/generics (historic, rally,
groupb, rallyx, trailblazer), CARFOLDER a car of the player's of the same model.

Developer: xml FILE, pacenotes ROUTE, pssg-tree FILE [DEPTH], car-textures PSSG, car-texture PSSG NAME OUT.dds, car-info PSSG [-v].";

pub fn main_entry(args: impl IntoIterator<Item = String>) {
    if let Err(error) = run(args) {
        eprintln!("rbr-dirt-rally: {error}");
        std::process::exit(1);
    }
}

pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("help" | "-h" | "--help") => {
            println!("{HELP}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("rbr-dirt-rally {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("list") => {
            let install = install(args.next().as_deref())?;
            for route in install.routes() {
                let kind = match (route.is_rally(), route.is_rallycross()) {
                    (true, _) => "",
                    (_, true) => " (rallycross circuit)",
                    _ => " (not a rally stage)",
                };
                println!("{}{kind}", route.id());
            }
            Ok(())
        }
        Some("import") => import(args),
        Some("preview") => preview(args),
        Some("car") => car(args),
        Some("add-car") => add_car(args),
        Some(other) => crate::dev::run(other, args),
    }
}

pub(crate) fn install(path: Option<&str>) -> Result<Install, String> {
    match path {
        Some(path) => Install::at(Path::new(path)),
        None => {
            Install::find().ok_or_else(|| "no DiRT Rally install found in the Steam libraries; pass its folder".into())
        }
    }
}

fn car(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let car = args.next().filter(|a| !a.starts_with("--")).ok_or("car needs the car's folder (cme)")?;
    let (mut path, mut out, mut template, mut class, mut livery, mut name) = (None, None, None, None, None, None);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--install" => path = Some(value),
            "--out" => out = Some(value),
            "--template" => template = Some(value),
            "--class" => class = Some(value),
            "--livery" => livery = Some(value.parse().map_err(|e| format!("{flag}: {e}"))?),
            "--name" => name = Some(value),
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    // A car folder names its model after itself.
    let template = PathBuf::from(template.ok_or("car needs --template, the folder of a car of the same model")?);
    let template = match template.file_name().filter(|_| template.is_dir()) {
        Some(folder) => template.join(format!("{}.sgc", folder.to_string_lossy())),
        None => template,
    };
    let options = crate::car_pack::Options {
        name: name.unwrap_or_else(|| format!("DiRT_{car}")),
        car,
        class: class.ok_or("car needs --class, its generic textures under cars/generics (historic, rally, ...)")?,
        livery,
        template,
    };
    let out = out.ok_or("car needs --out")?;
    let model = crate::car_pack::convert(&install(path.as_deref())?.data, &options, Path::new(&out))?;
    println!("{}", model.display());
    Ok(())
}

fn add_car(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    use crate::added_car;
    let car = args.next().filter(|a| !a.starts_with("--")).ok_or("add-car needs the car's folder (cme)")?;
    let (mut path, mut cars, mut livery) = (None, None, None);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--install" => path = Some(value),
            "--cars" => cars = Some(PathBuf::from(value)),
            "--livery" => livery = Some(value.parse().map_err(|e| format!("{flag}: {e}"))?),
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    let cars = cars.or_else(added_car::cars_folder).ok_or("no player folder; pass --cars")?;
    let name = added_car::add(&install(path.as_deref())?.data, &cars, &car, livery)?;
    println!("added {name} to {}", cars.display());
    Ok(())
}

fn import(args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut args = args.peekable();
    let id = args.next().filter(|a| !a.starts_with("--")).ok_or("import needs LOCATION/AREA/route_N")?;
    let log = |m: &str| eprintln!("rbr-dirt-rally: {m}");
    let control = rbr_btb_writer::control::Control::new(log, Default::default());
    let mut options = crate::import::Options::new(control);
    let mut path = None;
    let mut previews = true;
    while let Some(flag) = args.next() {
        if flag == "--replace" {
            options.replace = true;
            continue;
        }
        if flag == "--no-preview" {
            previews = false;
            continue;
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let metres = || value.parse::<f64>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--install" => path = Some(value.clone()),
            "--name" => options.name = Some(value.clone()),
            "--out" => options.out = Some(value.clone().into()),
            "--draw-corridor" => options.draw_corridor = metres()?,
            "--collision-corridor" => options.collision_corridor = metres()?,
            "--lod" => options.lod = metres()?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    let report = crate::import::import(&install(path.as_deref())?, &id, &options)?;
    println!("{}", report.folder.display());
    println!("{report:?}");
    if previews {
        if let Err(e) = preview_stage(&report.folder, None, None) {
            eprintln!("rbr-dirt-rally: no preview picture ({e}); the launcher shows the driveline");
        }
    }
    Ok(())
}

/// Renders `folder`'s preview with the `viewer` (default: the one beside this program) and the maps (default:
/// `$RBR_DATA/Maps`).
fn preview_stage(folder: &Path, viewer: Option<&Path>, maps: Option<&Path>) -> Result<(), String> {
    let viewer = match viewer {
        Some(v) => v.to_owned(),
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name(format!("viewer{}", std::env::consts::EXE_SUFFIX)),
    };
    if !viewer.is_file() {
        return Err(format!("no viewer at {}", viewer.display()));
    }
    let maps = match maps {
        Some(m) => m.to_owned(),
        None => std::env::var_os("RBR_DATA").map(|d| Path::new(&d).join("Maps")).ok_or("RBR_DATA is not set")?,
    };
    let scratch = std::env::temp_dir().join(format!("rbr-dirt-rally-preview-{}", std::process::id()));
    let png = crate::preview::render(folder, &viewer, &maps, &scratch, &[])?;
    println!("{}", png.display());
    Ok(())
}

fn preview(args: impl Iterator<Item = String>) -> Result<(), String> {
    let (mut folders, mut viewer, mut maps) = (Vec::new(), None, None);
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--viewer" => viewer = Some(PathBuf::from(args.next().ok_or("--viewer needs a path")?)),
            "--maps" => maps = Some(PathBuf::from(args.next().ok_or("--maps needs a folder")?)),
            _ => folders.push(PathBuf::from(arg)),
        }
    }
    if folders.is_empty() {
        return Err("preview needs a stage folder".into());
    }
    let mut failed = 0;
    for folder in &folders {
        if let Err(e) = preview_stage(folder, viewer.as_deref(), maps.as_deref()) {
            eprintln!("rbr-dirt-rally: {}: {e}", folder.display());
            failed += 1;
        }
    }
    if failed > 0 {
        Err(format!("{failed} of {} previews failed", folders.len()))
    } else {
        Ok(())
    }
}
