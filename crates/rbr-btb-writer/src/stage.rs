//! Validation and runtime preparation of a converted stage.
use std::path::{Path, PathBuf};

pub const MAX_CELL_VERTICES: usize = 65_536;
/// Starts the error of a collision cell over the limit, so a caller can retry with narrower collision.
pub const CELL_LIMIT: &str = "collision cell has";

#[derive(Debug)]
pub struct CellStats {
    pub cells: usize,
    pub max_vertices: usize,
}

/// Validate the exact collision cells ReRBR builds, before a stage is used.
pub fn check_cells(folder: &Path) -> Result<CellStats, String> {
    let track = rbr_btb::track::Track::open(folder)?;
    let col = rbr_formats::col::parse(&rbr_btb::col::build(&track)?).map_err(|e| format!("col: {e}"))?;
    let stats = CellStats {
        cells: col.cells.len(),
        max_vertices: col.cells.iter().map(|c| c.vertices.len()).max().unwrap_or(0),
    };
    if stats.cells == 0 {
        return Err("BTB collision has no cells".into());
    }
    if stats.max_vertices > MAX_CELL_VERTICES {
        return Err(format!("{CELL_LIMIT} {} vertices; narrow --collision-corridor", stats.max_vertices));
    }
    Ok(stats)
}

/// Prepares the stage `folder` into `dir` for a probe or a drive, from `maps` or `$RBR_DATA/Maps`; the stage's name.
pub fn prepare(folder: &Path, dir: &Path, maps: Option<&Path>) -> Result<String, String> {
    let maps = maps.map(Path::to_owned).or_else(|| std::env::var_os("RBR_DATA").map(|p| Path::new(&p).join("Maps")));
    let maps = maps.ok_or("preparation needs --maps MAPS_DIR or RBR_DATA")?;
    let track = rbr_btb::track::Track::open(folder)?;
    rbr_btb::stage::prepare(&track, 1_999_999, &maps, dir)
}

/// Checks the corridor and LOD distances of an import: each between 1 and 5000 m.
pub fn check_distances(draw_corridor: f64, collision_corridor: f64, lod: f64) -> Result<(), String> {
    for (key, value) in [("draw corridor", draw_corridor), ("collision corridor", collision_corridor), ("LOD", lod)] {
        if !value.is_finite() || !(1. ..=5000.).contains(&value) {
            return Err(format!("{key} must be between 1 and 5000 m"));
        }
    }
    Ok(())
}

/// Where a converted stage is written and what it may replace.
pub struct Destination<'a> {
    /// The folder the stage's own folder `name` goes in.
    pub out: &'a Path,
    pub name: &'a str,
    pub replace: bool,
    /// The provenance file only the converter's stages carry: `--replace` needs it in the folder it replaces.
    pub marker: &'a str,
    /// The converter's short name, in the hidden staging and backup folders (`.<prefix>-stage-…`).
    pub prefix: &'a str,
    /// Completes "--replace only replaces stages …" in the error of an existing folder that may not be replaced.
    pub replaces: &'a str,
}

/// Writes a stage into `<out>/<name>` through a staging folder: `build` fills the staging folder, a failed or
/// cancelled conversion leaves nothing, a replaced stage is swapped in whole. Its result and the final folder.
pub fn publish<T>(to: &Destination, build: impl FnOnce(&Path) -> Result<T, String>) -> Result<(T, PathBuf), String> {
    std::fs::create_dir_all(to.out).map_err(|e| e.to_string())?;
    let destination = to.out.join(to.name);
    if destination.exists() && (!to.replace || !destination.join(to.marker).is_file()) {
        return Err(format!("{} exists; --replace only replaces stages {}", destination.display(), to.replaces));
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let staging = to.out.join(format!(".{}-stage-{}-{stamp}", to.prefix, std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let built = match build(&staging) {
        Ok(built) => built,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    if destination.exists() {
        let backup = to.out.join(format!(".{}-backup-{}-{stamp}", to.prefix, std::process::id()));
        std::fs::rename(&destination, &backup).map_err(|e| e.to_string())?;
        if let Err(e) = std::fs::rename(&staging, &destination) {
            let _ = std::fs::rename(&backup, &destination);
            return Err(e.to_string());
        }
        std::fs::remove_dir_all(backup).map_err(|e| e.to_string())?;
    } else {
        std::fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
    }
    Ok((built, destination))
}
