//! Synthetic full-format fixture, plus an optional test over the player's local track files.
use rbr_ac_tracks::assetto::{
    kn5::{self, IDENTITY},
    stage::{self, Input, Options},
};
use std::collections::BTreeMap;

use rbr_ac_tracks::fixture::{fixture, floats, word};

#[test]
fn complete_stage_preserves_axes_geometry_surface_and_timing() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("ac-stage-test-{}-{stamp}", std::process::id()));
    let source = root.join("source");
    fixture(&source);
    let input = Input::read(&source, "test").unwrap();
    assert_eq!(input.splits.len(), 2);
    assert!((input.finish - input.start - 80.004).abs() < 0.01);
    let mut options = Options {
        out: root.join("output"),
        name: "test".into(),
        draw_corridor: 100.,
        collision_corridor: 50.,
        lod: 500.,
        replace: false,
        remesh: false,
        surface_overrides: BTreeMap::new(),
        control: Default::default(),
    };
    let report = stage::convert(&input, &options).unwrap();
    assert_eq!(report.drawn_triangles, 240);
    assert_eq!(report.collision_triangles, 241);
    assert_eq!(report.missing_textures, 0);
    let provenance = std::fs::read_to_string(report.folder.join("ac-conversion.json")).unwrap();
    let provenance: serde_json::Value = serde_json::from_str(&provenance).unwrap();
    assert_eq!(provenance["turned_wall_faces"], 1);
    assert_eq!(provenance["surface_triangles"]["WALL -> 64"], 1);
    let track = rbr_btb::track::Track::open(&report.folder).unwrap();
    let spawn = track.spawn().unwrap();
    assert!((spawn.matrix[13] - 10.14).abs() < 0.001);
    assert!((spawn.matrix[12] + 1.).abs() < 0.001);
    // The fixture runs along +z in AC's right-handed frame, so it faces -z in the stage frame.
    assert!((spawn.angles[2].abs() - std::f64::consts::PI).abs() < 0.001);
    let line = track.driveline().unwrap();
    assert!((line.points[0].position[2] - 10.).abs() < 0.001);
    let maps = track.surface_maps().unwrap();
    // The declared 0.94 of clean asphalt's 0.98 is nearest coarse clean tarmac (69, 0.96 of 37).
    assert_eq!(maps.maps, vec![[64; 256], [69; 256]]);
    assert_eq!(maps.damp, vec![[64; 256], [70; 256]]);
    assert_eq!(maps.wet, vec![[64; 256], [71; 256]]);
    let notes = track.pace_notes().unwrap();
    assert_eq!(notes.notes.iter().filter(|n| n.tag == 0x15).count(), 1);
    assert_eq!(notes.notes.iter().filter(|n| n.tag == 0x16).count(), 1);
    let finish = notes.notes.iter().find(|n| n.tag == 0x16).unwrap().distance;
    let end = notes.notes.iter().find(|n| n.tag == 0x18).unwrap().distance;
    assert!((end - finish - 15.).abs() < 0.001);
    let scene = rbr_btb::scene::Scene::load(&report.folder).unwrap();
    assert!(scene.materials.iter().all(|m| m.diffuse.is_some()));
    let mesh = &scene.meshes[0];
    let p = |i: usize| mesh.positions[mesh.indices[i] as usize];
    let (a, b, c) = (p(0), p(1), p(2));
    // Up-facing file winding, before the BTB physics loader swaps y/z.
    assert!((b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]) > 0.);
    let col = rbr_formats::col::parse(&rbr_btb::col::build(&track).unwrap()).unwrap();
    assert!(!col.cells.is_empty());
    assert!(report.max_cell_vertices <= 65536);
    assert!(stage::convert(&input, &options).unwrap_err().contains("exists"));
    options.replace = true;
    stage::convert(&input, &options).unwrap();
    options.name = ".".into();
    assert!(stage::convert(&input, &options).unwrap_err().contains("folder name"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn single_layout_paths_and_explicit_reversed_ai_follow_ac_conventions() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("ac-root-test-{}-{stamp}", std::process::id()));
    let source = root.join("single");
    fixture(&source);
    std::fs::rename(source.join("test/ai"), source.join("ai")).unwrap();
    std::fs::rename(source.join("test/data"), source.join("data")).unwrap();
    std::fs::rename(source.join("test.kn5"), source.join("single.kn5")).unwrap();
    std::fs::remove_file(source.join("models_test.ini")).unwrap();
    std::fs::write(source.join("MODELS.INI"), "[MODEL_0]\nFILE=SINGLE.KN5\n").unwrap();
    let input = Input::read(&source, ".").unwrap();
    assert_eq!(input.models.len(), 1);
    assert_eq!(input.title, "single");
    std::fs::remove_file(source.join("MODELS.INI")).unwrap();
    assert_eq!(Input::read(&source, "").unwrap().models.len(), 1);
    let mut data = vec![];
    for n in [7, 25, 0, 0] {
        word(&mut data, n);
    }
    for i in 0..25 {
        let z = (24 - i) as f32 * 5.;
        floats(&mut data, &[0., 10. + z * 0.01, z, 0.]);
        word(&mut data, i);
    }
    let reverse = root.join("reverse.ai");
    std::fs::write(&reverse, data).unwrap();
    // Gates that run the other way along the file reverse it without being asked.
    assert!(Input::read_with_ai(&source, ".", Some(&reverse), false, false).unwrap().reversed_ai);
    let reversed = Input::read_with_ai(&source, ".", Some(&reverse), true, false).unwrap();
    assert!(reversed.reversed_ai);
    assert_eq!(reversed.line.points, input.line.points);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_closed_ai_loop_is_one_lap_of_a_circuit() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("ac-circuit-test-{}-{stamp}", std::process::id()));
    fixture(&root);
    // A 200 m square sampled every 4 m; the last point is 4 m short of the first. AC's (x, y, z) is (x, height, z).
    let mut data = vec![];
    for n in [7, 200, 0, 0] {
        word(&mut data, n);
    }
    for i in 0..200 {
        let (side, d) = (i / 50, (i % 50) as f32 * 4.);
        let (x, z) = [(d, 0.), (200., d), (200. - d, 200.), (0., 200. - d)][side as usize];
        floats(&mut data, &[x, 10., z, 0.]);
        word(&mut data, i);
    }
    let ai = root.join("loop.ai");
    std::fs::write(&ai, data).unwrap();
    let input = Input::read_with_ai(&root, "test", Some(&ai), false, true).unwrap();
    assert!(input.circuit);
    assert!((input.finish - input.start - 800.).abs() < 1e-6);
    assert_eq!(input.line.points.first(), input.line.points.last());
    assert!(Input::read_with_ai(&root, "test", Some(&root.join("test/ai/fast_lane.ai")), false, true).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_vertex_values_do_not_poison_valid_geometry() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("ac-invalid-test-{}-{stamp}", std::process::id()));
    fixture(&root);
    let mut bytes = std::fs::read(root.join("test.kn5")).unwrap();
    let start = bytes.windows(8).position(|w| w == b"1ROAD001").unwrap() + 8 + 4 + 1 + 3 + 4;
    bytes[start..start + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    bytes[start + 44 + 24..start + 44 + 28].copy_from_slice(&f32::NAN.to_le_bytes());
    bytes[start + 88 + 12..start + 88 + 16].copy_from_slice(&f32::NAN.to_le_bytes());
    let model = kn5::parse(&bytes, true, true, &IDENTITY).unwrap();
    assert_eq!(model.dropped_triangles, 1);
    assert_eq!(model.repaired_uvs, 1);
    assert_eq!(model.repaired_normals, 1);
    let mesh = &model.meshes[0];
    assert_eq!(mesh.triangles.len(), 239);
    assert_eq!(mesh.vertices[1].uv[0], 0.);
    assert!(mesh.vertices[2].normal.iter().all(|v| v.is_finite()));
    for t in &mesh.triangles {
        assert!(t.iter().all(|&i| mesh.vertices[i as usize].position.iter().all(|v| v.is_finite())));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn reads_every_kn5_and_both_layouts_of_the_local_track() {
    let Some(root) = std::env::var_os("RBR_ASSETTO").map(std::path::PathBuf::from) else {
        eprintln!("skipping local-track test; set RBR_ASSETTO to the Crequy track folder");
        return;
    };
    let mut files = 0;
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("kn5")) {
            let model = kn5::read(&path, true, false, &IDENTITY).unwrap();
            assert!(!model.meshes.is_empty());
            for mesh in &model.meshes {
                assert_eq!(mesh.triangles.len(), mesh.triangle_count);
            }
            files += 1;
        }
    }
    assert!(files > 0);
    for layout in ["crequy_2024", "crequy_2025"] {
        let input = Input::read(&root, layout).unwrap();
        assert!(input.finish > input.start + 10_000.);
        assert_eq!(input.models.len(), 8);
    }
}

#[test]
fn an_ai_line_running_from_the_finish_to_the_start_is_reversed() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let source = std::env::temp_dir().join(format!("btb-ac-backwards-{}-{stamp}", std::process::id()));
    fixture(&source);
    let forward = Input::read(&source, "test").unwrap();
    assert!(!forward.reversed_ai);
    // The same line recorded the other way: records of 20 bytes after a 16-byte header, each numbered in order.
    let file = source.join("test/ai/fast_lane.ai");
    let bytes = std::fs::read(&file).unwrap();
    let mut backwards = bytes[..16].to_vec();
    for (i, record) in bytes[16..].chunks_exact(20).rev().enumerate() {
        backwards.extend_from_slice(&record[..16]);
        backwards.extend_from_slice(&(i as u32).to_le_bytes());
    }
    std::fs::write(&file, backwards).unwrap();
    let input = Input::read(&source, "test").unwrap();
    assert!(input.reversed_ai);
    assert!(input.finish > input.start + 50.);
    assert!((input.finish - input.start - (forward.finish - forward.start)).abs() < 1.);
    let _ = std::fs::remove_dir_all(source);
}

#[test]
fn a_safe_spawn_is_used_when_the_source_has_no_grid_helper() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("btb-ac-spawn-{}-{stamp}", std::process::id()));
    let source = root.join("source");
    fixture(&source);
    let mut bytes = std::fs::read(source.join("test.kn5")).unwrap();
    let at = bytes.windows(10).position(|b| b == b"AC_START_0").unwrap();
    bytes[at + 9] = b'X';
    std::fs::write(source.join("test.kn5"), bytes).unwrap();
    let input = Input::read(&source, "test").unwrap();
    let options = Options {
        out: root.join("out"),
        name: "fallback".into(),
        draw_corridor: 300.,
        collision_corridor: 75.,
        lod: 1000.,
        replace: false,
        remesh: false,
        surface_overrides: BTreeMap::new(),
        control: Default::default(),
    };
    stage::convert(&input, &options).unwrap();
    let track = rbr_btb::track::Track::open(&root.join("out/fallback")).unwrap();
    let spawn = track.spawn().unwrap();
    assert!((spawn.matrix[13] - 10.15).abs() < 0.001);
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(track.folder.join("ac-conversion.json")).unwrap()).unwrap();
    assert_eq!(meta["spawn_source"], "driveline_before_start");
    assert!(meta["spawn_before_start_m"].as_f64().unwrap() > 4.9);
    std::fs::remove_dir_all(root).unwrap();
}
