//! The BTB stage folders under `RBR_RSF` (a RallySimFans install, `RX_CONTENT/Tracks/*`): every folder reads, and
//! every `CACHE/` file the plugin wrote rebuilds byte-identical from the sources. Skipped when `RBR_RSF` is unset.
//!
//! The `trk` builder here takes the cached `ksh.ksh` bytes through, so it checks the driveline encoding and the framing;
//! `tests/ksh.rs` builds the collision meshes themselves.

use rbr_btb::{classic, ids, track};
use rbr_formats::{dls, lbs, markers, mat, trk};
use std::path::{Path, PathBuf};

/// Where `got` and `want` first differ, short enough to read.
fn first_difference(got: &[u8], want: Option<&[u8]>) -> String {
    match want {
        None => "no cached file".to_owned(),
        Some(want) => match got.iter().zip(want).position(|(a, b)| a != b) {
            Some(at) => format!("first difference at byte {at} (of {} and {})", got.len(), want.len()),
            None => format!("lengths {} and {}", got.len(), want.len()),
        },
    }
}

fn root() -> Option<PathBuf> {
    std::env::var_os("RBR_RSF").map(PathBuf::from)
}

fn cache(folder: &Path, file: &str) -> Option<Vec<u8>> {
    std::fs::read(folder.join("CACHE").join(file)).ok()
}

fn tracks() -> Option<Vec<Result<track::Track, (PathBuf, String)>>> {
    let root = root()?;
    let tracks = track::scan(&root);
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
    assert!(folders > 0, "no BTB folder read");
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
        let built_trk = classic::trk(&driveline.points, false, ksh_count, &ksh);
        assert_eq!(built_trk, want_trk, "{}: trk.trk", track.name);
        if ksh.is_empty() {
            assert_eq!(ksh_count, 0, "{}: empty ksh with nonzero count", track.name);
        }
        trk::parse(&built_trk).unwrap_or_else(|e| panic!("{}: built trk: {e}", track.name));

        // mat: the nine identical entries from the surface maps; missing maps and rows read zero.
        let maps = track.surface_maps().unwrap();
        let built_mat = classic::mat(&maps);
        assert!(
            want_mat.as_deref() == Some(&built_mat),
            "{}: mat.mat: {}",
            track.name,
            first_difference(&built_mat, want_mat.as_deref())
        );
        mat::parse(&built_mat).unwrap_or_else(|e| panic!("{}: built mat: {e}", track.name));

        // dls: the fixed template with the first `count` pace notes as keys.
        let notes = track.pace_notes().unwrap();
        let built_dls = classic::dls(&notes.notes);
        match track.source_newer_than_cache("dls.dls", &["pacenotes.ini"]) {
            Some(file) if want_dls.as_ref() != Some(&built_dls) => {
                let cached = want_dls.as_deref().and_then(|d| markers::parse(d).ok()).map(|m| m.keys.len());
                eprintln!(
                    "{}: dls.dls not compared, {file} is newer than the cache ({} notes now, {cached:?} cached; {})",
                    track.name,
                    notes.count,
                    first_difference(&built_dls, want_dls.as_deref())
                )
            }
            _ => assert!(
                want_dls.as_deref() == Some(&built_dls),
                "{}: dls.dls: {}",
                track.name,
                first_difference(&built_dls, want_dls.as_deref())
            ),
        }
        dls::parse(&built_dls).unwrap_or_else(|e| panic!("{}: built dls: {e}", track.name));
        let stage_markers = markers::parse(&built_dls).unwrap();
        assert_eq!(stage_markers.keys.len(), notes.count, "{}: marker keys", track.name);

        // lbs: the start chunk from the spawn matrix and angle, then the empty `(2, 10)` and `(1, 0)` stubs (the
        // latter's four zero bytes are a count and no sky word: `lbs::parse` accepts it). The game never
        // dispatches these stubs: BTB geometry comes from the `.x` meshes through the plugin's own scene.
        let spawn = track.spawn().unwrap();
        let built_lbs = classic::lbs(&spawn);
        match track.source_newer_than_cache("lbs.lbs", &["spawn.ini"]) {
            Some(file) if want_lbs.as_ref() != Some(&built_lbs) => {
                eprintln!("{}: lbs.lbs not compared, {file} is newer than the cache", track.name)
            }
            _ => assert!(
                want_lbs.as_deref() == Some(&built_lbs),
                "{}: lbs.lbs: {}",
                track.name,
                first_difference(&built_lbs, want_lbs.as_deref())
            ),
        }
        let lbs = lbs::parse(&built_lbs).unwrap_or_else(|e| panic!("{}: built lbs: {e}", track.name));
        assert_eq!(lbs.chunks.len(), 3, "{}: lbs chunks", track.name);
        let rbr_formats::lbs::Chunk::StartLocation(start) = &lbs.chunks[0] else {
            panic!("{}: no start chunk", track.name)
        };
        let f32_of = |v: f64| v as f32;
        assert_eq!(start.position, [f32_of(spawn.matrix[12]), f32_of(spawn.matrix[13]), f32_of(spawn.matrix[14])]);
        assert_eq!(start.rotation, [f32_of(spawn.angles[0]), f32_of(spawn.angles[2]), 0.0]);
        assert_eq!(&built_lbs[124..], &want_lbs.as_ref().unwrap()[124..], "{}: lbs stub chunk", track.name);

        checked += 1;
    }
    eprintln!("{checked} of {cached} cached folders rebuild byte-identical");
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
    let (matched, unmatched) = ids::match_ids(&root);
    for (folder, id, source) in &matched {
        eprintln!("{id} {source:?} {}", folder.file_name().unwrap().to_string_lossy());
    }
    eprintln!("{} matched, {} unmatched", matched.len(), unmatched.len());
    for folder in &unmatched {
        eprintln!("unmatched: {}", folder.file_name().unwrap().to_string_lossy());
    }
    // Every folder is matched or reported, never both; the ids are RSF's own (1000 and up).
    let folders = tracks().map_or(0, |t| t.len());
    assert_eq!(matched.len() + unmatched.len(), folders, "every folder is matched or reported");
    assert!(matched.iter().all(|(_, id, _)| *id >= 1000), "an id below 1000");
}
