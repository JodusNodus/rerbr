//! Validation and runtime preparation of a converted stage.
use std::path::Path;

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
