//! `rbr-ac-tracks import`: list and import the layouts of an Assetto Corsa `content/tracks` folder.
use crate::btb::control::Control;
use crate::import::{self, Layout, Request, Status};
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

const HELP: &str =
    "rbr-ac-tracks import AC_TRACKS_FOLDER [--track NAME [--layout LAYOUT] [--name NAME] [--out DIR] [--replace]]

Without --track, lists every track layout in AC's content/tracks and whether it can be imported.
With --track, imports that layout (omit --layout for a single-layout track) into the player's tracks folder, or --out,
choosing the circuit, smoothing and collision options itself. For fixed options use `rbr-ac-tracks ac`.";

pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let (mut tracks, mut track, mut layout, mut name, mut out, mut replace) = (None, None, None, None, None, false);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value\n{HELP}"));
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "--replace" => replace = true,
            "--track" => track = Some(value()?),
            "--layout" => layout = Some(value()?),
            "--name" => name = Some(value()?),
            "--out" => out = Some(PathBuf::from(value()?)),
            _ if tracks.is_none() && !arg.starts_with('-') => tracks = Some(PathBuf::from(arg)),
            _ => return Err(format!("unknown argument {arg}\n{HELP}")),
        }
    }
    let tracks = tracks.ok_or(HELP)?;
    let Some(track) = track else {
        for Layout { track, layout, status } in import::discover(&tracks)? {
            let track = track.file_name().unwrap_or_default().to_string_lossy();
            match status {
                Status::Ready => println!("{track} [{layout}]"),
                Status::Unsupported(why) => println!("{track} [{layout}]: unsupported, {why}"),
            }
        }
        return Ok(());
    };
    let layout = layout.unwrap_or_else(|| ".".into());
    let layout = import::layouts(&tracks.join(&track))
        .into_iter()
        .find(|l| l.layout == layout)
        .ok_or_else(|| format!("{track} has no layout {layout}"))?;
    let out = out.or_else(rbr_btb::player::tracks).ok_or("--out required without a player folder")?;
    let control = Control::new(|message| eprintln!("rbr-ac-tracks: {message}"), Arc::new(AtomicBool::new(false)));
    let imported = import::import(&Request { layout, out, name, replace }, &control).map_err(|e| e.to_string())?;
    println!("{}\n{:?}\n{:?}", imported.folder.display(), imported.report, imported.applied);
    Ok(())
}
