//! Assetto Corsa tracks: each track folder of a dropped folder or archive (one with a KN5 model of its own and a
//! `data/surfaces.ini`, its own or in a layout folder; names ignoring case) is unpacked into a
//! hidden folder beside `<player folder>/tracks/` and converted there, a stage for each layout, by `rbr_ac_tracks`
//! (`notes/assetto.md`). Every path goes through [`crate::archive::safe_folder`] and programs and sound banks are left
//! out. A stage appears whole or not at all, and a cancel leaves nothing behind. Each stage keeps the converter's
//! `ac-conversion.json`, which marks it as an import in the Stages list ([`crate::btb::Btb::ac`]).

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rbr_ac_tracks::btb::control::Control;
use rbr_ac_tracks::import::{self, Error, Layout, Request, Status};

use super::{wanted, within, Ctx, Found, Item, Kind};
use crate::archive::safe_folder;
use crate::content::source::{Entry, Source};
use crate::content::{stem, Goto, Outcome, Refresh};
use crate::job::{Counter, Temp};

/// File endings that are never copied: programs, and the sound banks and videos a track brings that a stage cannot use.
const SKIPPED: [&str; 14] =
    ["exe", "dll", "msi", "bat", "cmd", "com", "scr", "sh", "so", "bank", "ogg", "wav", "mp4", "avi"];

const KIND: &str = "Assetto Corsa track";

pub struct Ac;

/// The track folders among `entries`: each folder with a `.kn5` of its own and a `data/surfaces.ini` in it or in a
/// folder of it (a car has no surfaces; a track without an AI line is still one, and says what it lacks), the outermost of
/// nested ones.
pub fn track_dirs(entries: &[Entry]) -> Vec<Vec<String>> {
    let mut found: Vec<Vec<String>> = Vec::new();
    let mut candidates: Vec<&[String]> = entries
        .iter()
        .filter(|e| e.path.last().is_some_and(|f| f.to_ascii_lowercase().ends_with(".kn5")))
        .map(|e| &e.path[..e.path.len() - 1])
        .collect();
    // The shortest first: a folder comes before the ones inside it.
    candidates.sort_by_key(|d| d.len());
    for dir in candidates {
        if found.iter().any(|f| within(dir, f) || f == dir) {
            continue;
        }
        // The file's path below the folder, at most a layout folder deep.
        let has = |file: &str| {
            entries.iter().filter(|e| within(&e.path, dir) && e.path.len() <= dir.len() + 3).any(|e| {
                let rest = e.path[dir.len()..].join("/").to_ascii_lowercase();
                rest == file || rest.ends_with(&format!("/{file}"))
            })
        };
        if has("data/surfaces.ini") {
            found.push(dir.to_vec());
        }
    }
    found
}

/// The stage folder of a layout: `AC_` and the track's folder, and the layout's name when the track has several.
fn stage_folder(track: &str, layout: &str) -> Option<String> {
    safe_folder(&match layout {
        "." => format!("AC_{track}"),
        layout => format!("AC_{track}_{layout}"),
    })
}

/// Converts `layout` into `tracks`, stopping when `cancel` is set: the conversion runs on a thread of its own while this
/// one watches the flag.
fn convert(layout: Layout, name: String, tracks: &Path, cancel: &AtomicBool) -> Result<(), Error> {
    let stop = Arc::new(AtomicBool::new(false));
    let control = Control::new(|_| {}, stop.clone());
    let request = Request { layout, out: tracks.to_owned(), name: Some(name), replace: false };
    std::thread::scope(|scope| {
        let work = scope.spawn(|| import::import(&request, &control));
        while !work.is_finished() {
            if cancel.load(Ordering::Relaxed) {
                stop.store(true, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        work.join().unwrap_or_else(|_| Err(Error::Failed("it stopped unexpectedly".into())))
    })
    .map(|_| ())
}

impl Kind for Ac {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        let at = track_dirs(src.entries());
        let label = if at.len() == 1 { KIND } else { "Assetto Corsa tracks" };
        // A track's own folders (`ai`, `data`, `extension`) look like other kinds' to them: it is all the source is.
        (!at.is_empty()).then(|| Found { at, only: true, ..Found::new(label) })
    }

    fn add(&self, item: &Item, ctx: &Ctx) -> Vec<Outcome> {
        let name = item.src.name();
        let tracks = ctx.home.join(rbr_btb::player::TRACKS);
        item.found
            .at
            .iter()
            .flat_map(|dir| {
                let track = dir.last().cloned().unwrap_or_else(|| stem(&name).to_owned());
                add_track(item.src, dir, &track, &tracks, ctx)
                    .unwrap_or_else(|e| vec![Outcome::failed(&track, KIND, e)])
            })
            .map(|outcome| outcome.refreshing(Refresh::Stages))
            .collect()
    }
}

/// Unpacks the track folder `dir` of `src` and converts each of its layouts into `tracks`: an outcome for each.
fn add_track(src: &dyn Source, dir: &[String], track: &str, tracks: &Path, ctx: &Ctx) -> Result<Vec<Outcome>, String> {
    let folder = safe_folder(track).ok_or_else(|| format!("a bad track folder name {track}"))?;
    let wanted = wanted(src, dir, &SKIPPED);
    let pairs: Vec<(Vec<String>, Vec<String>)> =
        wanted.iter().map(|e| (e.path.clone(), e.path[dir.len()..].to_vec())).collect();
    fs::create_dir_all(tracks).map_err(|e| format!("{}: {e}", tracks.display()))?;
    let temp = Temp::create(tracks, "ac-adding")?;
    let unpacked = temp.0.join(&folder);
    let total = wanted.iter().map(|e| e.size).sum();
    let mut count =
        Counter { step: "Unpacking Assetto Corsa track", done: 0, total, report: ctx.report, cancel: ctx.cancel };
    count.add(0)?;
    src.extract(&pairs, &unpacked, &mut |n| count.add(n))?;
    fs::create_dir_all(&unpacked).map_err(|e| format!("{}: {e}", unpacked.display()))?;
    let mut outcomes = Vec::new();
    let layouts = import::layouts(&unpacked);
    if layouts.is_empty() {
        return Err("it has no model to convert".into());
    }
    for layout in layouts {
        ctx.step("Converting Assetto Corsa track");
        let shown = if layout.layout == "." { track.to_owned() } else { format!("{track} [{}]", layout.layout) };
        let Some(name) = stage_folder(track, &layout.layout) else {
            outcomes.push(Outcome::failed(&shown, KIND, "a bad stage folder name"));
            continue;
        };
        if let Status::Unsupported(why) = &layout.status {
            outcomes.push(Outcome::failed(&shown, KIND, format!("can't be converted: {why}")));
            continue;
        }
        if tracks.join(&name).exists() {
            outcomes.push(Outcome::failed(&shown, KIND, "already added"));
            continue;
        }
        outcomes.push(match convert(layout, name.clone(), tracks, ctx.cancel) {
            Ok(()) => Outcome::added(&shown, KIND, &shown, "it's in Stages", Goto::Stage(name)),
            Err(Error::Cancelled) => Outcome::failed(&shown, KIND, "cancelled"),
            Err(e) => Outcome::failed(&shown, KIND, format!("can't be converted: {e}")),
        });
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(files: &[&str]) -> Vec<String> {
        let entries: Vec<Entry> =
            files.iter().map(|f| Entry { path: f.split('/').map(str::to_owned).collect(), size: 0 }).collect();
        track_dirs(&entries).iter().map(|d| d.join("/")).collect()
    }

    #[test]
    fn tracks_are_found_in_a_content_tree_with_layouts_and_not_among_cars() {
        assert_eq!(dirs(&["t/t.kn5", "t/ai/fast_lane.ai", "t/data/surfaces.ini"]), ["t"]);
        assert_eq!(
            dirs(&[
                "content/tracks/Foo/foo.kn5",
                "content/tracks/Foo/AI/Fast_Lane.ai",
                "content/tracks/Foo/Data/Surfaces.ini"
            ]),
            ["content/tracks/Foo"]
        );
        // Layouts keep their own AI and data folders.
        let layouts = [
            "f/f.kn5",
            "f/models_a.ini",
            "f/a/ai/fast_lane.ai",
            "f/a/data/surfaces.ini",
            "f/b/ai/fast_lane.ai",
            "f/b/data/surfaces.ini",
        ];
        assert_eq!(dirs(&layouts), ["f"]);
        // A car has a model and data but no surfaces; a model alone is not a track. One without an AI line is, and the
        // conversion says what it lacks.
        assert!(dirs(&["content/cars/x/x.kn5", "content/cars/x/data/car.ini"]).is_empty());
        assert!(dirs(&["t/t.kn5", "t/ai/fast_lane.ai"]).is_empty());
        assert_eq!(dirs(&["t/t.kn5", "t/ai/_fast_lane.ai", "t/data/surfaces.ini"]), ["t"]);
        // Two tracks of a pack, and a sibling's files do not make a folder one.
        assert_eq!(dirs(&["p/a/a.kn5", "p/a/ai/fast_lane.ai", "p/a/data/surfaces.ini", "p/b/b.kn5"]), ["p/a"]);
    }

    fn zipped(src: &Path, archive: &Path, top: &str) {
        let mut writer = zip::ZipWriter::new(fs::File::create(archive).unwrap());
        for (rel, path) in crate::codriver_packs::walk(src) {
            writer.start_file(format!("{top}/{rel}"), zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut writer, &fs::read(path).unwrap()).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn a_zipped_track_becomes_a_listed_stage_marked_as_an_import() {
        let dir = crate::test_dir("content-ac-zip");
        rbr_ac_tracks::fixture::fixture(&dir.join("src"));
        // The programs and sound banks a mod ships are not unpacked.
        crate::content::kind::btb::tests::write(&dir.join("src/helper.exe"), b"MZ");
        crate::content::kind::btb::tests::write(&dir.join("src/sfx/track.bank"), b"bank");
        zipped(&dir.join("src"), &dir.join("synthetic.zip"), "content/tracks/synthetic");
        let home = dir.join("home");
        let done = crate::content::run(
            &home,
            &Default::default(),
            &[dir.join("synthetic.zip")],
            &|_| {},
            &AtomicBool::new(false),
        );
        assert_eq!(done.len(), 1, "{done:?}");
        assert!(done[0].ok(), "{}", done[0].line());
        assert_eq!(done[0].goto, Some(Goto::Stage("AC_synthetic_test".into())));
        let tracks = home.join("tracks");
        assert!(tracks.join("AC_synthetic_test").join(rbr_ac_tracks::assetto::stage::CONVERSION_FILE).is_file());
        let left: Vec<_> = fs::read_dir(&tracks).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["AC_synthetic_test"], "the unpacked files and the programs are gone");
        let maps = crate::btb::folder_maps(&tracks, |_| false);
        assert_eq!(maps.len(), 1);
        assert!(maps[0].btb().unwrap().ac);
        // Again: already added, nothing else appears.
        let again = crate::content::run(
            &home,
            &Default::default(),
            &[dir.join("synthetic.zip")],
            &|_| {},
            &AtomicBool::new(false),
        );
        assert_eq!(again[0].result, Err("already added".into()), "{again:?}");
        assert_eq!(fs::read_dir(&tracks).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cancel_leaves_no_stage_and_no_hidden_folder() {
        let dir = crate::test_dir("content-ac-cancel");
        rbr_ac_tracks::fixture::fixture(&dir.join("src"));
        let home = dir.join("home");
        let done = crate::content::run(&home, &Default::default(), &[dir.join("src")], &|_| {}, &AtomicBool::new(true));
        assert!(done.iter().all(|o| !o.ok()), "{done:?}");
        assert!(fs::read_dir(home.join("tracks")).map_or(true, |mut r| r.next().is_none()));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// With `RBR_ASSETTO` (an Assetto Corsa track folder): the track is added from the folder into a temp home and lists
    /// as an import.
    #[test]
    fn a_real_track_adds_and_lists() {
        let Some(track) = std::env::var_os("RBR_ASSETTO").map(std::path::PathBuf::from) else {
            eprintln!("skipping: RBR_ASSETTO not set");
            return;
        };
        let dir = crate::test_dir("content-ac-real");
        let home = dir.join("home");
        let done = crate::content::run(&home, &Default::default(), &[track], &|_| {}, &AtomicBool::new(false));
        done.iter().for_each(|o| eprintln!("{}", o.line()));
        assert!(done.iter().all(Outcome::ok), "{done:?}");
        let maps = crate::btb::folder_maps(&home.join("tracks"), |_| false);
        assert!(!maps.is_empty() && maps.iter().all(|m| m.btb().is_some_and(|b| b.ac)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_layout_has_its_own_stage_folder() {
        assert_eq!(stage_folder("puech_arnal_ac", ".").as_deref(), Some("AC_puech_arnal_ac"));
        assert_eq!(stage_folder("ruapuke", "short").as_deref(), Some("AC_ruapuke_short"));
        assert_eq!(stage_folder("a/b", "."), None);
    }
}
