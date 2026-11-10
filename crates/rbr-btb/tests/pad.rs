//! The flat pad prepares into a stage of its own: the written folder opens, its scene draws and collides, and
//! `prepare` builds the stage files. The `prepare` test needs `RBR_DATA` (track-41's retail `fnc`/`ini`).

use rbr_btb::pad;
use std::path::PathBuf;

fn maps() -> Option<PathBuf> {
    let data = rbr_formats::data::Data::from_env()?;
    let maps = data.install().join("Maps");
    maps.is_dir().then_some(maps)
}

/// A fresh pads' folder of the test's own.
fn base(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbr-pad-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn prepare_builds_a_stage_of_its_own() {
    let Some(maps) = maps() else {
        eprintln!("skipping: RBR_DATA not set");
        return;
    };
    let base = base("prepare");
    for material in [37u8, 17, 101] {
        let prepared = pad::prepare_in(&base, material, &maps).unwrap();
        assert_eq!(prepared.surface, pad::surface(material));
        let (number, letter) = rbr_formats::stage_files::parts(&prepared.stage).unwrap();
        assert_eq!((number, letter), (pad::number(material), 'M'));
        for ext in ["lbs", "dls", "mat", "trk", "col", "fnc", "ini"] {
            let path = format!("{}.{ext}", prepared.stage);
            assert!(std::path::Path::new(&path).is_file(), "{path}");
        }
        // The prepared stage opens as a BTB stage: the scene draws the one mesh, the col is level ground.
        let folder = rbr_formats::stage_files::folder(&prepared.stage).unwrap();
        let source = rbr_btb::stage::source(folder).unwrap();
        assert!(rbr_btb::track::is_track_folder(&source));
        let track = rbr_btb::track::Track::open(&source).unwrap();
        let scene = rbr_btb::scene::Scene::load(&source).unwrap();
        assert_eq!((scene.meshes.len(), scene.objects.len()), (1, 1));
        let bytes = rbr_btb::col::build(&track).unwrap();
        let file = rbr_formats::col::parse(&bytes).unwrap();
        let faces: Vec<_> = file
            .cells
            .iter()
            .flat_map(|c| c.tree.triangles.iter().map(|t| t.vertices.map(|i| c.vertices[i as usize])))
            .collect();
        assert_eq!(faces.len(), 20000);
        assert!(faces.iter().flatten().all(|v| v[2] == 0.0));
    }
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn concurrent_prepares_share_one_directory() {
    let Some(maps) = maps() else {
        eprintln!("skipping: RBR_DATA not set");
        return;
    };
    // Eight simultaneous cold starts (the handling harness runs 8 jobs in parallel): all share the build.
    let base = base("concurrent");
    let final_dir = base.join(format!("{}-b{}", pad::dir_name(65), pad::BUILD));
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..8).map(|_| s.spawn(|| pad::prepare_in(&base, 65, &maps))).collect();
        let mut stages = Vec::new();
        for h in handles {
            stages.push(h.join().unwrap().unwrap().stage);
        }
        assert!(stages.iter().all(|s| *s == stages[0]));
    });
    assert!(final_dir.is_dir());
    // No temporary build is left beside it.
    assert_eq!(std::fs::read_dir(&base).unwrap().count(), 1);
    std::fs::remove_dir_all(&base).unwrap();
}
