//! Command dispatch of the `rbr-ac-tracks` executable.
use std::path::Path;

pub mod assetto;
pub mod import;

const HELP: &str = "rbr-ac-tracks: convert locally owned Assetto Corsa tracks to BTB stages for ReRBR

Usage:
  rbr-ac-tracks ac --track TRACK_FOLDER [--layout LAYOUT] [OPTIONS]
  rbr-ac-tracks import AC_TRACKS_FOLDER [--track NAME [--layout LAYOUT]]
  rbr-ac-tracks prepare --track STAGE_FOLDER --out PREPARED_DIR [--maps MAPS_DIR]
  rbr-ac-tracks probe PREPARED_DIR

Run a command with --help for its options. Outputs default to the player's tracks folder. Source game assets are
read in place. The launcher's Import does the same as `import`, from a folder or an archive.";

pub fn main_entry(args: impl IntoIterator<Item = String>) {
    if let Err(error) = run(args) {
        eprintln!("rbr-ac-tracks: {error}");
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
            println!("rbr-ac-tracks {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("ac" | "assetto") => assetto::run(args),
        Some("import") => import::run(args),
        Some("prepare") => prepare(args),
        Some("probe") => {
            let dir = args.next().ok_or("probe needs PREPARED_DIR")?;
            if matches!(dir.as_str(), "-h" | "--help") {
                println!("rbr-ac-tracks probe PREPARED_DIR\nChecks both wheel tracks, road creases, steps and collision cell sizes.");
                return Ok(());
            }
            if let Some(extra) = args.next() {
                return Err(format!("unexpected probe argument {extra}"));
            }
            rbr_btb_writer::probe::run(Path::new(&dir))
        }
        Some(other) => Err(format!("unknown command {other}\n{HELP}")),
    }
}

fn prepare(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    let (mut track, mut out, mut maps) = (None, None, None);
    while let Some(arg) = args.next() {
        if matches!(arg.as_str(), "-h" | "--help") {
            println!("rbr-ac-tracks prepare --track STAGE_FOLDER --out PREPARED_DIR [--maps MAPS_DIR]\nMaps defaults to $RBR_DATA/Maps.");
            return Ok(());
        }
        let target = match arg.as_str() {
            "--track" => &mut track,
            "--out" | "--prepare" => &mut out,
            "--maps" => &mut maps,
            _ => return Err(format!("unknown prepare argument {arg}")),
        };
        *target = Some(std::path::PathBuf::from(args.next().ok_or_else(|| format!("{arg} needs a value"))?));
    }
    let name = rbr_btb_writer::stage::prepare(
        &track.ok_or("prepare needs --track STAGE_FOLDER")?,
        &out.ok_or("prepare needs --out PREPARED_DIR")?,
        maps.as_deref(),
    )?;
    println!("Prepared: {name}");
    Ok(())
}
