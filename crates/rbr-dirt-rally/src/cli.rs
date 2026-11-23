//! Command dispatch of the `rbr-dirt-rally` executable.
use crate::install::Install;
use std::path::Path;

const HELP: &str = "rbr-dirt-rally: convert stages of a locally installed DiRT Rally to BTB stages for ReRBR

Usage:
  rbr-dirt-rally list [INSTALL]
  rbr-dirt-rally import LOCATION/AREA/route_N [--install INSTALL] [--name NAME] [--out DIR] [--replace]
         [--draw-corridor M] [--collision-corridor M] [--lod M]

INSTALL is the game folder (or its data folder); without it, the Steam libraries are searched.
Source game files are read in place.";

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
                println!("{}{}", route.id(), if route.is_rally() { "" } else { " (not a rally stage)" });
            }
            Ok(())
        }
        Some("import") => import(args),
        Some(other) => Err(format!("unknown command {other}; see --help")),
    }
}

fn install(path: Option<&str>) -> Result<Install, String> {
    match path {
        Some(path) => Install::at(Path::new(path)),
        None => {
            Install::find().ok_or_else(|| "no DiRT Rally install found in the Steam libraries; pass its folder".into())
        }
    }
}

fn import(args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut args = args.peekable();
    let id = args.next().filter(|a| !a.starts_with("--")).ok_or("import needs LOCATION/AREA/route_N")?;
    let log = |m: &str| eprintln!("rbr-dirt-rally: {m}");
    let control = rbr_ac_tracks::btb::control::Control::new(log, Default::default());
    let mut options = crate::import::Options::new(control);
    let mut path = None;
    while let Some(flag) = args.next() {
        if flag == "--replace" {
            options.replace = true;
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
    Ok(())
}
