//! The import interface over the synthetic track: discovery, automatic options, cancellation.
use rbr_ac_tracks::btb::control::Control;
use rbr_ac_tracks::import::{self, Error, Request, Status};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

mod common;
use common::{floats, word};

fn temp(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("btb-import-{name}-{}-{stamp}", std::process::id()))
}

/// `content/tracks/<name>` holding the synthetic track's layout `test`.
fn tracks(root: &Path) -> PathBuf {
    let tracks = root.join("content/tracks");
    common::fixture(&tracks.join("synthetic"));
    tracks
}

fn request(tracks: &Path, out: &Path) -> Request {
    let layout = import::layouts(&tracks.join("synthetic")).remove(0);
    Request { layout, out: out.to_owned(), name: None, replace: false }
}

#[test]
fn discovery_lists_layouts_and_what_they_lack() {
    let root = temp("discover");
    let tracks = tracks(&root);
    let found = import::discover(&tracks).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].layout.as_str(), &found[0].status), ("test", &Status::Ready));
    assert_eq!(found[0].default_name(), "AC_test");
    std::fs::remove_file(tracks.join("synthetic/test/data/surfaces.ini")).unwrap();
    let found = import::discover(&tracks).unwrap();
    assert!(matches!(&found[0].status, Status::Unsupported(why) if why.contains("surfaces")));
    let unsupported = Request { layout: found[0].clone(), ..request(&tracks, &root.join("out")) };
    assert!(matches!(import::import(&unsupported, &Control::default()), Err(Error::Unsupported(_))));
    assert!(!root.join("out").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn imports_a_layout_and_reports_progress() {
    let root = temp("import");
    let tracks = tracks(&root);
    let lines = Arc::new(Mutex::new(Vec::new()));
    let log = lines.clone();
    let control = Control::new(move |line| log.lock().unwrap().push(line.to_owned()), Arc::default());
    let mut request = request(&tracks, &root.join("out"));
    let imported = import::import(&request, &control).unwrap();
    assert_eq!(imported.folder, root.join("out/AC_test"));
    assert!(!imported.applied.circuit && !imported.applied.remesh);
    assert!(imported.report.drawn_triangles > 0);
    assert!(rbr_btb::track::Track::open(&imported.folder).is_ok());
    assert!(lines.lock().unwrap().iter().any(|l| l.contains("spawn from")));
    // A second import needs --replace, and never replaces a folder the converter did not write.
    assert!(import::import(&request, &control).is_err());
    request.replace = true;
    import::import(&request, &control).unwrap();
    std::fs::remove_file(imported.folder.join("ac-conversion.json")).unwrap();
    assert!(import::import(&request, &control).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelling_leaves_no_stage_behind() {
    let root = temp("cancel");
    let tracks = tracks(&root);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    // Cancel as soon as the conversion reports its first model.
    let control = Control::new(
        move |line| {
            if line.starts_with("reading ") {
                flag.store(true, Ordering::Relaxed)
            }
        },
        cancel,
    );
    let result = import::import(&request(&tracks, &root.join("out")), &control);
    assert_eq!(result.unwrap_err(), Error::Cancelled);
    let left: Vec<_> = std::fs::read_dir(root.join("out")).map(|d| d.flatten().collect()).unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_closed_loop_without_gates_imports_as_a_circuit() {
    let root = temp("circuit");
    let tracks = tracks(&root);
    let source = tracks.join("synthetic");
    let mut kn5 = std::fs::read(source.join("test.kn5")).unwrap();
    for name in [&b"AC_AB_START_L"[..], b"AC_AB_START_R", b"AC_AB_FINISH_L", b"AC_AB_FINISH_R"] {
        let at = kn5.windows(name.len()).position(|w| w == name).unwrap();
        kn5[at] = b'X';
    }
    std::fs::write(source.join("test.kn5"), kn5).unwrap();
    // A 200 m square sampled every 4 m: the last point is 4 m short of the first.
    let mut ai = vec![];
    for n in [7, 200, 0, 0] {
        word(&mut ai, n);
    }
    for i in 0..200 {
        let (side, d) = (i / 50, (i % 50) as f32 * 4.);
        let (x, z) = [(d, 0.), (200., d), (200. - d, 200.), (0., 200. - d)][side as usize];
        floats(&mut ai, &[x, 10., z, 0.]);
        word(&mut ai, i);
    }
    std::fs::write(source.join("test/ai/fast_lane.ai"), ai).unwrap();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let log = lines.clone();
    let control = Control::new(move |line| log.lock().unwrap().push(line.to_owned()), Arc::default());
    // The fixture's ground is a 100 m strip under a 200 m loop, so the stage itself may fail; the choice is made first.
    let _ = import::import(&request(&tracks, &root.join("out")), &control);
    assert!(lines.lock().unwrap().iter().any(|l| l.contains("as a circuit")));
    std::fs::remove_dir_all(root).unwrap();
}
