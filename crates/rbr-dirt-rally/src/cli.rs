//! Command dispatch of the `rbr-dirt-rally` executable.
use crate::install::Install;
use std::path::Path;

const HELP: &str = "rbr-dirt-rally: convert stages of a locally installed DiRT Rally to BTB stages for ReRBR

Usage:
  rbr-dirt-rally list [INSTALL]

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
