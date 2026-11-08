//! A BeamNG level zip (`RBR_BEAMNG`, the player's own copy; skipped without it): every `.cdae` reads, every `.dds`
//! but the HDR ones decodes, its materials and ground models read, and its first time-trial mission converts into a
//! temporary folder the BTB path opens.

use rbr_beamng::{cdae, convert, cutout, level, materials, zipfs::Archive};
use std::path::PathBuf;

fn level_zip() -> Option<PathBuf> {
    let zip = std::env::var_os("RBR_BEAMNG").map(PathBuf::from);
    if zip.is_none() {
        eprintln!("skipping: RBR_BEAMNG not set");
    }
    zip
}

#[test]
fn every_shape_and_material_reads_and_the_level_converts() {
    let Some(zip) = level_zip() else { return };
    let mut archive = Archive::open(&zip).unwrap();
    let shapes: Vec<String> = archive.names().into_iter().filter(|n| n.to_lowercase().ends_with(".cdae")).collect();
    assert!(!shapes.is_empty());
    for name in &shapes {
        let bytes = archive.read(name).unwrap();
        cdae::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    for name in archive.names().into_iter().filter(|n| n.to_lowercase().ends_with(".dds")) {
        let bytes = archive.read(&name).unwrap();
        // BC6H (DXGI 95, 96): HDR cube maps, never a colour map.
        let hdr = bytes.get(84..88) == Some(b"DX10") && matches!(bytes.get(128), Some(95 | 96));
        assert!(hdr || cutout::decode(&bytes).is_some(), "{name}");
    }
    let name = level::names(&archive).into_iter().next().expect("a level");
    assert!(!materials::read(&mut archive, &name).unwrap().is_empty());
    materials::ground_models(&mut archive, &name).unwrap();
    let mut missions: Vec<String> = archive
        .names()
        .into_iter()
        .filter(|n| n.contains("/timeTrial/") && n.ends_with("/race.race.json"))
        .map(|n| n.trim_end_matches("/race.race.json").to_owned())
        .collect();
    missions.sort();
    let mission = missions.first().expect("a time-trial mission");
    let out = std::env::temp_dir().join(format!("rbr-beamng-test-{}", std::process::id()));
    let mut options = convert::Options::new(mission, &out);
    options.draw_corridor = 10.0;
    let report = convert::convert(&zip, &options).unwrap();
    let track = rbr_btb::track::Track::open(&report.folder).unwrap();
    assert!(track.folder.join("Objects.zip").is_file() && track.folder.join("pacenotes.ini").is_file());
    std::fs::remove_dir_all(&out).unwrap();
}
