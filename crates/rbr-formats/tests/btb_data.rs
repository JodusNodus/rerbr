//! The BTB stage folders under `RBR_RSF` (a RallySimFans install, `RX_CONTENT/Tracks/*`): every folder reads, and
//! every `CACHE/` file the plugin wrote rebuilds byte-identical from the sources. Skipped when `RBR_RSF` is unset.
//!
//! The `trk` builder takes the `ksh.ksh` collision-mesh bytes as input (built from the `.x` meshes, another
//! worker's scope): the test passes the cached ones through, so it checks the driveline encoding and the framing.
//! Four folders build it empty.

use rbr_formats::btb::{self, build};
use rbr_formats::{dls, lbs, markers, mat, trk};
use std::path::{Path, PathBuf};

fn root() -> Option<PathBuf> {
    std::env::var_os("RBR_RSF").map(PathBuf::from)
}

fn cache(folder: &Path, file: &str) -> Option<Vec<u8>> {
    std::fs::read(folder.join("CACHE").join(file)).ok()
}

fn tracks() -> Option<Vec<Result<btb::Track, (PathBuf, String)>>> {
    let root = root()?;
    let tracks = btb::scan(&root);
    if tracks.is_empty() {
        eprintln!("skipping: {}: no BTB stage folders", root.display());
        return None;
    }
    Some(tracks)
}

#[test]
fn every_btb_folder_reads() {
    let Some(tracks) = tracks() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let mut folders = 0;
    for track in &tracks {
        let track = track.as_ref().unwrap_or_else(|(f, e)| panic!("{}: {e}", f.display()));
        let spawn = track.spawn().unwrap_or_else(|e| panic!("{}: {e}", track.folder.display()));
        let driveline = track.driveline().unwrap_or_else(|e| panic!("{}: {e}", track.folder.display()));
        let settings = track.settings().unwrap_or_else(|e| panic!("{}: {e}", track.folder.display()));
        let objects = track.objects().unwrap_or_else(|e| panic!("{}: {e}", track.folder.display()));
        let materials = track.materials().unwrap_or_else(|e| panic!("{}: {e}", track.folder.display()));
        let maps = track.surface_maps().unwrap_or_else(|e| panic!("{}: {e}", track.folder.display()));
        let notes = track.pace_notes().unwrap_or_else(|e| panic!("{}: {e}", track.folder.display()));
        assert_eq!(driveline.points.len(), driveline.count, "{}: driveline count", track.name);
        assert_eq!(maps.maps.len(), maps.count, "{}: surface map count", track.name);
        assert_eq!(notes.notes.len(), notes.count, "{}: pace note count", track.name);
        eprintln!(
            "{} {:?} {} objects, {} materials, {} maps, {} notes, sun {:?}",
            track.name,
            track.info.physics(),
            objects.len(),
            materials.names.len(),
            maps.count,
            notes.count,
            settings.sun_direction,
        );
        assert!(settings.sun_direction.is_some(), "{}: no sun direction", track.name);
        assert!(track.info.physics().is_some(), "{}: unknown physics {:?}", track.name, track.info.physics_raw);
        let _ = spawn;
        folders += 1;
    }
    eprintln!("{folders} BTB folders read");
    assert_eq!(folders, 72, "the local install holds 72 BTB folders");
}

#[test]
fn cached_builds_match_and_parse() {
    let Some(tracks) = tracks() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let (mut cached, mut checked) = (0, 0);
    for track in tracks.into_iter().map(|t| t.unwrap_or_else(|(f, e)| panic!("{f:?}: {e}"))) {
        let folder = &track.folder;
        let Some(want_trk) = cache(folder, "trk.trk") else { continue };
        let (want_mat, want_dls, want_lbs) =
            (cache(folder, "mat.mat"), cache(folder, "dls.dls"), cache(folder, "lbs.lbs"));
        cached += 1;

        // trk: the driveline chunk from the sources, the collision chunk from the cached `ksh.ksh`.
        let driveline = track.driveline().unwrap();
        let (ksh_count, ksh) = ksh_of(&want_trk);
        let built_trk = build::trk(&driveline.points, ksh_count, &ksh);
        assert_eq!(built_trk, want_trk, "{}: trk.trk", track.name);
        if ksh.is_empty() {
            assert_eq!(ksh_count, 0, "{}: empty ksh with nonzero count", track.name);
        }
        trk::parse(&built_trk).unwrap_or_else(|e| panic!("{}: built trk: {e}", track.name));

        // mat: the nine identical entries from the surface maps; missing maps and rows read zero.
        let maps = track.surface_maps().unwrap();
        let built_mat = build::mat(&maps.maps);
        assert_eq!(Some(built_mat.clone()), want_mat, "{}: mat.mat", track.name);
        mat::parse(&built_mat).unwrap_or_else(|e| panic!("{}: built mat: {e}", track.name));

        // dls: the fixed template with the first `count` pace notes as keys.
        let notes = track.pace_notes().unwrap();
        let built_dls = build::dls(&notes.notes);
        assert_eq!(Some(built_dls.clone()), want_dls, "{}: dls.dls", track.name);
        dls::parse(&built_dls).unwrap_or_else(|e| panic!("{}: built dls: {e}", track.name));
        let stage_markers = markers::parse(&built_dls).unwrap();
        assert_eq!(stage_markers.keys.len(), notes.count, "{}: marker keys", track.name);

        // lbs: the start chunk from the spawn matrix and angle. The trailing stub `(1, 0)` chunk (four zero
        // bytes) is not valid retail geometry, so the retail parser only reads the `(2, 11)` and `(2, 10)`
        // prefix; the trailing 20 bytes are asserted as framing below. The game never dispatches these stubs:
        // BTB geometry comes from the `.x` meshes through the plugin's own scene.
        let spawn = track.spawn().unwrap();
        let built_lbs = build::lbs(&spawn);
        assert_eq!(Some(built_lbs.clone()), want_lbs, "{}: lbs.lbs", track.name);
        let prefix = &built_lbs[..124];
        let lbs = lbs::parse(prefix).unwrap_or_else(|e| panic!("{}: built lbs: {e}", track.name));
        assert_eq!(lbs.chunks.len(), 2, "{}: lbs chunks", track.name);
        let rbr_formats::lbs::Chunk::StartLocation(start) = &lbs.chunks[0] else {
            panic!("{}: no start chunk", track.name)
        };
        let f32_of = |v: f64| v as f32;
        assert_eq!(start.position, [f32_of(spawn.matrix[12]), f32_of(spawn.matrix[13]), f32_of(spawn.matrix[14])]);
        assert_eq!(start.rotation, [0.0, f32_of(spawn.angles[2]), 0.0]);
        assert_eq!(&built_lbs[124..], &want_lbs.as_ref().unwrap()[124..], "{}: lbs stub chunk", track.name);

        checked += 1;
    }
    eprintln!("{checked} of {cached} cached folders rebuild byte-identical");
    assert_eq!(cached, 19, "the local install caches 19 BTB folders");
    assert_eq!(checked, cached);
}

/// The `(3, 0x16)` payload of a cached `trk.trk`, split into its mesh count and the `ksh.ksh` mesh bytes.
fn ksh_of(cached: &[u8]) -> (u32, Vec<u8>) {
    let header_len = u32::from_le_bytes(cached[0..4].try_into().unwrap()) as usize;
    let mut offset = 4 + header_len + 4;
    // Skip the driveline chunk: its payload length follows its header.
    let driveline_len = u32::from_le_bytes(cached[offset - 4..offset].try_into().unwrap()) as usize;
    offset += driveline_len;
    let collision_header_len = u32::from_le_bytes(cached[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4 + collision_header_len;
    let collision_len = u32::from_le_bytes(cached[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    let payload = &cached[offset..offset + collision_len];
    let count = u32::from_le_bytes(payload[0..4].try_into().unwrap());
    (count, payload[4..].to_vec())
}

#[test]
fn btb_stage_ids_map() {
    let Some(root) = root() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let (matched, unmatched) = btb::ids::match_ids(&root);
    for (folder, id, source) in &matched {
        eprintln!("{id} {source:?} {}", folder.file_name().unwrap().to_string_lossy());
    }
    eprintln!("{} matched, {} unmatched", matched.len(), unmatched.len());
    for folder in &unmatched {
        eprintln!("unmatched: {}", folder.file_name().unwrap().to_string_lossy());
    }
    let mut unmatched: Vec<String> =
        unmatched.into_iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    unmatched.sort();
    assert_eq!(unmatched, ["BikolTardos12", "Myttaala Gravel"]);
    assert_eq!(matched.len(), 70);
}
