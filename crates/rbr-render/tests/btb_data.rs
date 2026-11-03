//! A BTB stage of an RSF install (`RBR_RSF`) loads through `stage::load` with its textures. Skipped without it.

use rbr_render::stage::{self, Selection};
use std::path::{Path, PathBuf};

#[test]
fn a_btb_stage_loads_with_its_textures() {
    let Some(root) = std::env::var_os("RBR_RSF").map(PathBuf::from) else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let folder = root.join("RX_CONTENT/Tracks/Pirttikulma");
    if !folder.is_dir() {
        eprintln!("skipping: no Pirttikulma");
        return;
    }
    let name = folder.join("track-41_M").to_string_lossy().into_owned();
    let mut stage = stage::load(Path::new("/nonexistent/Maps"), &name, Selection::default()).expect("loads");
    stage.load_textures(Path::new("/nonexistent/Maps")).expect("textures");
    assert!(stage.triangle_count() > 100_000);
    assert!(stage.start.is_some() && !stage.driveline.is_empty());
    assert!(stage.report.missing_textures.is_empty(), "{:?}", stage.report.missing_textures);
    assert!(stage.textures.values().all(|t| t.image.is_some()));
    // Every draw's texture names one the stage has.
    for draw in stage.meshes.iter().flat_map(|m| &m.draws) {
        for texture in draw.state.textures.into_iter().flatten() {
            assert!(stage.textures.contains_key(&texture));
        }
    }
}
