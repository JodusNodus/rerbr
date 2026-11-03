//! The built `col.col` of every BTB folder with a plugin cache equals the cache; every folder builds a col that
//! parses. Skipped when `RBR_RSF` is unset.

// The caches are the plugin's own: 18 of them were written on Windows, `Kutemajarvi 1994` under Wine.

use rbr_btb::{col, track};
use rbr_formats::col as col_format;
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
        let built =
            if track.name == "Kutemajarvi 1994" { col::build_with(track, col::D3dx::Wine) } else { col::build(track) };
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

#[test]
fn every_folder_builds_a_col_that_parses() {
    let Some(tracks) = tracks() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let only = std::env::var("COL_ONLY").ok();
    let mut built = 0;
    for track in tracks.iter().filter(|t| only.as_ref().is_none_or(|o| *o == t.name)) {
        let start = std::time::Instant::now();
        let bytes = col::build(track).unwrap_or_else(|e| panic!("{}: {e}", track.name));
        let file = col_format::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", track.name));
        eprintln!("{}: {} bytes, {} cells, {:.1?}", track.name, bytes.len(), file.cells.len(), start.elapsed());
        built += 1;
    }
    assert!(built > 0);
}

#[test]
fn the_input_shift_lowers_the_magnitude_by_one_ulp() {
    assert_eq!(col::shift_input(1.5).to_bits(), 1.5f32.to_bits() - 1);
    assert_eq!(col::shift_input(-1.5).to_bits(), (-1.5f32).to_bits() - 1);
    assert!(col::shift_input(-1.5) > -1.5);
    assert_eq!(col::shift_input(0.0).to_bits(), 0);
    assert_eq!(col::shift_input(-0.0).to_bits(), (-0.0f32).to_bits());
}

#[test]
fn only_the_exact_identity_skips_the_transform() {
    let mut m = [0.0f32; 16];
    for i in [0, 5, 10, 15] {
        m[i] = 1.0;
    }
    assert!(col::is_identity(&m));
    m[12] = 1e-30;
    assert!(!col::is_identity(&m));
    m[12] = -0.0;
    assert!(col::is_identity(&m), "-0 compares equal to 0");
}

#[test]
fn the_transform_sums_in_pairs() {
    // x*A + y*B = 1 + 2^-24 rounds to 1 on its own, then (z*C + D) = 2^-24 + 2^-24 is exact: the pair sum keeps it.
    let eps = 2f32.powi(-24);
    let mut m = [0.0f32; 16];
    m[0] = 1.0;
    m[4] = eps;
    m[8] = eps;
    m[12] = eps;
    let p = [1.0, 1.0, 1.0];
    assert_eq!(col::transform(&m, p)[0], 1.0 + 2.0 * eps);
    assert_eq!(col::transform_sequential(&m, p)[0], 1.0);
}

#[test]
fn nibbles_truncate_and_keep_the_low_byte_of_u() {
    assert_eq!(col::nibbles([0.0, 0.0]), 0);
    // 6.25 -> 6, 14.3 -> 14.
    assert_eq!(col::nibbles([0.416667, 0.955743]), 0xe6);
    // u * 15 = 2834.9 truncates to 2834 = 0xb12: the byte 0x12 reaches the high nibble; v * 15 = -548.4 -> -548, low nibble 0xc.
    assert_eq!(col::nibbles([188.99248, -36.560368]), 0x12 | 0xc0);
    // Negative u: -0.5 * 15 = -7.5 -> -7 = 0xf9.
    assert_eq!(col::nibbles([-0.5, 0.0]), 0xf9);
}
