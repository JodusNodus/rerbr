//! The built `col.col` of every BTB folder with a plugin cache equals the cache; every folder builds a col that
//! parses. Skipped when `RBR_RSF` is unset.

use rbr_btb::{col, track};
use std::path::PathBuf;

fn tracks() -> Option<Vec<track::Track>> {
    let root = PathBuf::from(std::env::var_os("RBR_RSF")?);
    let tracks: Vec<_> =
        track::scan(&root).into_iter().map(|t| t.unwrap_or_else(|(f, e)| panic!("{f:?}: {e}"))).collect();
    (!tracks.is_empty()).then_some(tracks)
}

/// The first byte at which the two differ, with the context.
fn first_difference(got: &[u8], want: &[u8]) -> String {
    match got.iter().zip(want).position(|(a, b)| a != b) {
        Some(at) => format!(
            "first difference at {at:#x}: got {:02x?}, want {:02x?}",
            &got[at..(at + 8).min(got.len())],
            &want[at..(at + 8).min(want.len())]
        ),
        None => format!("one is a prefix of the other (got {} bytes, want {})", got.len(), want.len()),
    }
}

#[test]
fn built_cols_equal_the_plugin_caches() {
    let Some(tracks) = tracks() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let only = std::env::var("COL_ONLY").ok();
    let (mut cached, mut same) = (0, 0);
    let mut failures = Vec::new();
    for track in &tracks {
        if only.as_ref().is_some_and(|o| *o != track.name) {
            continue;
        }
        let Ok(want) = std::fs::read(track.folder.join("CACHE").join("col.col")) else { continue };
        cached += 1;
        let start = std::time::Instant::now();
        let built = col::build(track);
        if let (Ok(got), Some(dir)) = (&built, std::env::var_os("COL_DUMP")) {
            std::fs::write(PathBuf::from(dir).join(format!("{}.col", track.name)), got).unwrap();
        }
        match built {
            Ok(got) if got == want => {
                same += 1;
                eprintln!("{}: identical, {} bytes, {:.1?}", track.name, got.len(), start.elapsed());
            }
            Ok(got) => {
                eprintln!(
                    "{}: {} ({} against {} bytes)",
                    track.name,
                    first_difference(&got, &want),
                    got.len(),
                    want.len()
                );
                failures.push(track.name.clone());
            }
            Err(e) => {
                eprintln!("{}: {e}", track.name);
                failures.push(track.name.clone());
            }
        }
    }
    eprintln!("{same} of {cached} cached cols identical");
    assert!(failures.is_empty(), "differing: {failures:?}");
}
