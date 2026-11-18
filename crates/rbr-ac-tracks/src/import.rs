//! Import a locally owned Assetto Corsa track as a BTB stage: the interface a launcher or the CLI calls.
//!
//! `layouts` and `discover` list what a track folder offers without reading any geometry. `import` converts one layout,
//! choosing the options a person would otherwise pick after a failure, and reports through a `Control`.
//! Nothing here prints, reads the environment or exits the process.
use crate::assetto::stage::{self, source_path, Input, Options, Report};
use crate::btb::{control::Control, control::CANCELLED, stage::CELL_LIMIT};
use std::fmt;
use std::path::{Path, PathBuf};

/// One layout of an AC track folder (`content/tracks/<track>/`); the layout `.` is a single-layout track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub track: PathBuf,
    pub layout: String,
    pub status: Status,
}

/// What a quick look at the files says. `Ready` is not a promise: timing gates are read by `import`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ready,
    Unsupported(String),
}

impl Layout {
    /// The stage folder name when none is given: `AC_` and the layout, or the track folder for a single layout.
    pub fn default_name(&self) -> String {
        let base = match self.layout.as_str() {
            "." => self.track.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            layout => layout.to_owned(),
        };
        format!("AC_{base}")
    }
}

/// The layouts of one track folder, sorted: one per `models_<layout>.ini`, else the single layout `.`.
pub fn layouts(track: &Path) -> Vec<Layout> {
    let mut names: Vec<String> = std::fs::read_dir(track)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let stem = file.get(..file.len().checked_sub(4)?).filter(|_| file.to_lowercase().ends_with(".ini"))?;
            stem.get(..7).filter(|p| p.eq_ignore_ascii_case("models_")).map(|_| stem[7..].to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    if names.is_empty() {
        let base = track.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let single = source_path(track, "models.ini").is_ok() || source_path(track, &format!("{base}.kn5")).is_ok();
        names = single.then(|| ".".to_owned()).into_iter().collect();
    }
    names.into_iter().map(|layout| Layout { track: track.to_owned(), status: status(track, &layout), layout }).collect()
}

/// Every layout of every track folder in `tracks` (AC's `content/tracks`), by track then layout.
pub fn discover(tracks: &Path) -> Result<Vec<Layout>, String> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(tracks)
        .map_err(|e| format!("{}: {e}", tracks.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    Ok(folders.iter().flat_map(|track| layouts(track)).collect())
}

fn status(track: &Path, layout: &str) -> Status {
    for file in ["ai/fast_lane.ai", "data/surfaces.ini"] {
        if source_path(track, &format!("{layout}/{file}")).is_err() {
            return Status::Unsupported(format!("missing {file}"));
        }
    }
    Status::Ready
}

/// What to convert and where to put it.
#[derive(Debug, Clone)]
pub struct Request {
    pub layout: Layout,
    /// The folder that receives the stage, normally the player's `tracks`.
    pub out: PathBuf,
    /// The stage's folder name; `Layout::default_name` when `None`.
    pub name: Option<String>,
    /// Replace an earlier import of the same name (never a folder this converter did not write).
    pub replace: bool,
}

/// The options the import settled on, for the player to see.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// A closed AI loop without timing gates became a ReRBR circuit.
    pub circuit: bool,
    /// The road collision was smoothed because it overflowed a collision cell.
    pub remesh: bool,
    pub collision_corridor: f64,
}

#[derive(Debug)]
pub struct Imported {
    pub folder: PathBuf,
    pub report: Report,
    pub applied: Applied,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    /// The layout cannot be converted; the text says why.
    Unsupported(String),
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cancelled => f.write_str(CANCELLED),
            Error::Unsupported(why) | Error::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(message: String) -> Self {
        if message == CANCELLED {
            Error::Cancelled
        } else {
            Error::Failed(message)
        }
    }
}

const DRAW_CORRIDOR: f64 = 300.;
const LOD: f64 = 1000.;
const COLLISION_CORRIDOR: f64 = 75.;
/// Smoothed road collision with a narrower corridor, which brings an oversized collision cell within the limit.
const NARROW_COLLISION_CORRIDOR: f64 = 25.;

/// Converts one layout into `request.out`. The stage appears whole or not at all.
pub fn import(request: &Request, control: &Control) -> Result<Imported, Error> {
    if let Status::Unsupported(why) = &request.layout.status {
        return Err(Error::Unsupported(why.clone()));
    }
    let Layout { track, layout, .. } = &request.layout;
    control.check()?;
    control.say(&format!("reading {}", track.display()));
    let (input, circuit) = match Input::read(track, layout) {
        Ok(input) => (input, false),
        // A loop of AI points with no gates is a circuit; any other failure is the source's.
        Err(first) if first.starts_with(stage::MISSING_GATE) => {
            let input = Input::read_with_ai(track, layout, None, false, true).map_err(|_| Error::Unsupported(first))?;
            control.say("no timing gates; importing the closed AI loop as a circuit");
            (input, true)
        }
        Err(other) => return Err(Error::Unsupported(other)),
    };
    let name = request.name.clone().unwrap_or_else(|| request.layout.default_name());
    let attempt = |remesh: bool| {
        let collision_corridor = if remesh { NARROW_COLLISION_CORRIDOR } else { COLLISION_CORRIDOR };
        let options = Options {
            out: request.out.clone(),
            name: name.clone(),
            draw_corridor: DRAW_CORRIDOR,
            collision_corridor,
            lod: LOD,
            replace: request.replace,
            remesh,
            surface_overrides: Default::default(),
            control: control.clone(),
        };
        stage::convert(&input, &options).map(|report| Imported {
            folder: report.folder.clone(),
            report,
            applied: Applied { circuit, remesh, collision_corridor },
        })
    };
    match attempt(false) {
        Err(e) if e.starts_with(CELL_LIMIT) => {
            control.say("a collision cell is over the limit; smoothing the road and narrowing collision");
            Ok(attempt(true)?)
        }
        done => Ok(done?),
    }
}
