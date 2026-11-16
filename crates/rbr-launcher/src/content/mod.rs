//! One "Add" for everything a player downloads: files dropped on the launcher window, or picked from the Settings
//! page's Add content section. Each item is opened as a file tree ([`source`]), each kind of content ([`kind`]) says
//! whether it is in it, and the result is a notice on every page ([`ui`]) saying whether it worked and where to find
//! it, or why it did not.
//!
//! Where each kind goes (user decision 2026-11-16): BTB stages to `tracks/`; co-drivers and pace notes through
//! `codriver_packs::import`; NGP car pieces (physics zip, model and sound-bank archives dropped together) as one
//! `car_packs::Download`; NGP's package to the kept `NGP 7.4.778/` folder, as the Settings download does; a disc image
//! opens "Set up from the disc…" with the disc filled in. RAR files, installers, classic-format stages (into `maps/`, renumbered when their number is taken) and anything
//! unknown are refused with the reason. A new kind or container is one module and one line below.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::job::Report;
use kind::{Ctx, Found, Item, Kind, Known};
use source::Source;

pub mod downloads;
mod drop;
pub mod kind;
pub mod source;
mod ui;

pub use ui::Add;

/// The kinds, in priority order: the ones that are all of a source first, the refusals last (asked only when nothing
/// else was found).
const KINDS: &[&dyn Kind] = &[
    &kind::disc::Disc,
    &kind::ngp::Ngp,
    &kind::btb::Btb,
    &kind::codriver::Codriver,
    &kind::car::Car,
    &kind::classic::Classic,
    &kind::unknown::Unknown,
];

/// A source's name without its archive end.
pub fn stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    [".zip", ".7z"].iter().find(|e| lower.ends_with(*e)).map_or(name, |e| &name[..name.len() - e.len()])
}

/// What each kind finds in `src`, by the kind's place in [`KINDS`].
pub fn detect(src: &dyn Source) -> Vec<(usize, Found)> {
    let mut found = Vec::new();
    for (at, kind) in KINDS.iter().enumerate() {
        if kind.fallback() && !found.is_empty() {
            continue;
        }
        if let Some(f) = kind.detect(src) {
            let only = f.only;
            found.push((at, f));
            if only {
                break;
            }
        }
    }
    found
}

/// Where an outcome's Open button goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Goto {
    /// The stage picker, on the stage folder.
    Stage(String),
    /// The Settings page.
    Settings,
    /// The Settings page, with the disc setup open on this disc.
    Disc(PathBuf),
}

/// What a launcher list an outcome changed (or tried to): read again when a run is done.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Refresh {
    #[default]
    Nothing,
    Stages,
    Cars,
    Codriver,
    Ngp,
}

/// What happened to one item (or one thing in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The dropped or picked item's name.
    pub source: String,
    pub kind: &'static str,
    /// What was added and where to find it, or why not.
    pub result: Result<String, String>,
    pub goto: Option<Goto>,
    /// A file the notice offers to open (a stage's licence).
    pub open: Option<PathBuf>,
    pub refresh: Refresh,
}

impl Outcome {
    pub fn added(source: &str, kind: &'static str, what: &str, place: &str, goto: Goto) -> Outcome {
        let text = format!("Added {what} ({kind}): {place}");
        Outcome {
            source: source.to_owned(),
            kind,
            result: Ok(text),
            goto: Some(goto),
            open: None,
            refresh: Refresh::Nothing,
        }
    }

    pub fn failed(source: &str, kind: &'static str, reason: impl Into<String>) -> Outcome {
        let reason = reason.into();
        let reason = if reason.ends_with("cancelled") { "cancelled".to_owned() } else { reason };
        Outcome {
            source: source.to_owned(),
            kind,
            result: Err(reason),
            goto: None,
            open: None,
            refresh: Refresh::Nothing,
        }
    }

    /// A warning that belongs to no item.
    pub fn warning(text: &str) -> Outcome {
        Outcome::failed("", "", text)
    }

    /// An outcome that says `text` in full.
    pub fn said(source: &str, kind: &'static str, text: String, goto: Goto) -> Outcome {
        Outcome { result: Ok(text), ..Outcome::added(source, kind, "", "", goto) }
    }

    pub fn opening(self, file: Option<PathBuf>) -> Outcome {
        Outcome { open: file, ..self }
    }

    pub fn refreshing(self, refresh: Refresh) -> Outcome {
        Outcome { refresh, ..self }
    }

    pub fn ok(&self) -> bool {
        self.result.is_ok()
    }

    /// The notice line, without its tick.
    pub fn line(&self) -> String {
        match (&self.result, self.source.is_empty()) {
            (Ok(text), _) => text.clone(),
            (Err(reason), true) => reason.clone(),
            (Err(reason), false) => format!("Couldn't add {}: {reason}", self.source),
        }
    }
}

/// Adds every item of `paths` to the player's folder `home`, in turn, and says what happened to each.
pub fn run(home: &Path, known: &Known, paths: &[PathBuf], report: Report<'_>, cancel: &AtomicBool) -> Vec<Outcome> {
    let ctx = Ctx { home, known, report, cancel };
    let mut outcomes = Vec::new();
    let mut sources = Vec::new();
    for path in paths {
        match source::open(path) {
            Ok(src) => sources.push(src),
            Err(why) => {
                let name =
                    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                outcomes.push(Outcome::failed(&name, "", why));
            }
        }
    }
    let mut per_kind: Vec<Vec<Item>> = KINDS.iter().map(|_| Vec::new()).collect();
    for src in &sources {
        for (at, found) in detect(&**src) {
            per_kind[at].push(Item { src: &**src, found });
        }
    }
    for (kind, items) in KINDS.iter().zip(&per_kind) {
        if !items.is_empty() {
            outcomes.extend(kind.add_all(items, &ctx));
        }
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::kind::btb::tests::{synthetic_stage, write};
    use source::Fake;
    use std::fs;

    fn labels(src: &Fake) -> Vec<&'static str> {
        detect(src).into_iter().map(|(_, f)| f.label).collect()
    }

    #[test]
    fn a_mixed_archive_holds_each_kind_and_a_stages_sounds_are_its_own() {
        let stage = ["Lappi/track.ini", "Lappi/objectlist.ini", "Lappi/Objects.zip", "Lappi/Textures.zip"];
        let mut files = stage.to_vec();
        files.extend(["Lappi/sounds/engine.ogg", "Pacenote/PaceNote.ini", "Pacenote/sounds/en/1.ogg"]);
        assert_eq!(labels(&Fake::new("both.zip", false, &files)), ["BTB stage", "co-driver or pace notes"]);
        let mut alone = stage.to_vec();
        alone.push("Lappi/sounds/engine.ogg");
        assert_eq!(labels(&Fake::new("lappi.zip", false, &alone)), ["BTB stage"]);
    }

    #[test]
    fn ngp_and_the_disc_are_all_of_their_source() {
        let ngp = ["physics.rbz", "Plugins/NGP.dll", "AudioFMOD/a.bank", "AudioFMOD/b.bank"];
        assert_eq!(labels(&Fake::new("RBR_NGP-7.4.778.507.7z", false, &ngp)), ["NGP"]);
        assert_eq!(labels(&Fake::new("NGP", true, &ngp)), ["NGP"]);
        assert_eq!(labels(&Fake::new("RBR.iso", false, &[])), ["disc image"]);
        assert_eq!(labels(&Fake::new("disc", true, &["Data1.hdr", "data1.cab"])), ["disc image"]);
    }

    #[test]
    fn what_is_refused_and_why() {
        use kind::unknown::UNKNOWN;
        let why = |name: &str, dir: bool, files: &[&str]| {
            let found = detect(&Fake::new(name, dir, files));
            assert_eq!(found.len(), 1, "{name}");
            assert_eq!(found[0].1.label, "not supported");
            found[0].1.tag
        };
        assert_eq!(why("holiday.zip", false, &["a.txt"]), UNKNOWN);
        assert_eq!(why("readme.pdf", false, &[]), UNKNOWN);
        assert_eq!(why("folder", true, &["a.txt"]), UNKNOWN);
    }

    #[test]
    fn classic_stages_are_recognised_complete_or_not() {
        assert_eq!(
            labels(&Fake::new("stage.zip", false, &["Maps/track-111_E.lbs", "Maps/Track-111_E.dds"])),
            ["classic stage"]
        );
        assert_eq!(labels(&Fake::new("stage.zip", false, &["Tracks.ini"])), ["classic stage"]);
        assert_eq!(labels(&Fake::new("stage.zip", false, &["x/TrackSettings_111.ini"])), ["classic stage"]);
        let two = ["a/Tracks5.ini", "a/track-5_O.lbs", "b/track-6_M.lbs"];
        assert_eq!(labels(&Fake::new("two", true, &two)), ["classic stages"]);
        // Not taken for notes.
        assert_eq!(
            labels(&Fake::new("classic", true, &["Maps/track-1.lbs", "Tracks/Tracks.ini", "x/y.ini"])),
            ["classic stage"]
        );
    }

    #[test]
    fn co_drivers_and_pace_notes_are_recognised() {
        assert_eq!(labels(&Fake::new("Janne V3.zip", false, &["Pacenote/PaceNote.ini"])), ["co-driver or pace notes"]);
        assert_eq!(labels(&Fake::new("notes", true, &["MyPacenotes/Finland/x.ini"])), ["co-driver or pace notes"]);
        assert_eq!(labels(&Fake::new("mynotes", true, &["Tanner/tanner.ini"])), ["co-driver or pace notes"]);
        assert_eq!(labels(&Fake::new("a.zip", false, &["Finland/x.ini"])), ["not supported"]);
    }

    fn run_all(home: &Path, paths: &[PathBuf]) -> Vec<Outcome> {
        run(home, &Known::default(), paths, &|_| {}, &AtomicBool::new(false))
    }

    fn lines(done: &[Outcome]) -> Vec<String> {
        done.iter().map(|o| format!("{} {}", if o.ok() { "ok" } else { "no" }, o.line())).collect()
    }

    #[test]
    fn a_run_adds_each_item_and_says_why_the_others_were_not() {
        let dir = crate::test_dir("content-run");
        let home = dir.join("home");
        let stage = dir.join("drop/Autio I Gravel");
        synthetic_stage(&stage, "Autio I Gravel");
        let voice = dir.join("drop/Voice");
        write(&voice.join("PaceNote.ini"), b"[SETTINGS]\nsounds=english/steve\n");
        write(&voice.join("config/pacenotes/Rbr.ini"), b"[PACKAGE::RBR]\nfile=calls.ini\n");
        write(&voice.join("config/pacenotes/calls.ini"), b"[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=one_left.ogg\n");
        write(&voice.join("config/ranges/Rbr.ini"), b"[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        write(&voice.join("sounds/english/steve/one_left.ogg"), b"OggS fake");
        for (name, text) in [("stage.rar", "Rar!"), ("Setup.exe", "MZ"), ("readme.txt", "hi"), ("RBR.iso", "iso")] {
            write(&dir.join("drop").join(name), text.as_bytes());
        }
        let names = ["Autio I Gravel", "Voice", "stage.rar", "Setup.exe", "readme.txt", "RBR.iso", "gone.zip"];
        let paths: Vec<PathBuf> = names.iter().map(|n| dir.join("drop").join(n)).collect();
        let done = run_all(&home, &paths);
        let got = lines(&done);
        let has = |start: &str| got.iter().any(|l| l.starts_with(start));
        assert!(has("ok Added Autio I Gravel (BTB stage): it's in the stage picker"), "{got:?}");
        assert!(has("ok Added Voice (co-driver): pick it in Settings"), "{got:?}");
        assert!(has("no Couldn't add stage.rar: RAR archives can't be read"), "{got:?}");
        assert!(has("no Couldn't add Setup.exe: installers can't be read: extract it with 7-Zip"), "{got:?}");
        assert!(has("no Couldn't add readme.txt: not recognised: ReRBR adds BTB stages"), "{got:?}");
        assert!(has("ok Disc image RBR.iso"), "{got:?}");
        assert!(has("no Couldn't add gone.zip: it doesn't exist"), "{got:?}");
        assert!(done.iter().any(|o| o.goto == Some(Goto::Disc(dir.join("drop/RBR.iso")))));
        assert!(
            done.iter().any(|o| o.refresh == Refresh::Stages) && done.iter().any(|o| o.refresh == Refresh::Codriver)
        );
        assert!(home.join("tracks/Autio I Gravel/track.ini").is_file());
        assert!(home.join(crate::codriver_packs::VOICES_DIR).join("Voice").is_dir());
        // The same stage again.
        let again = run_all(&home, &paths[..1]);
        assert_eq!(lines(&again), ["no Couldn't add Autio I Gravel: already added"]);
        // A cancelled run adds nothing.
        let stopped = run(&home, &Known::default(), &paths[..1], &|_| {}, &AtomicBool::new(true));
        assert!(!stopped[0].ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_model_without_its_physics_zip_is_refused() {
        let dir = crate::test_dir("content-model");
        let model = dir.join("Ford Fiesta - Model.7z");
        let mut out = sevenz_rust2::ArchiveWriter::create(&model).unwrap();
        let entry = sevenz_rust2::ArchiveEntry::new_file("Cars/Ford Fiesta/Ford Fiesta.sgc");
        out.push_archive_entry(entry, Some(&b"x"[..])).unwrap();
        out.finish().unwrap();
        let done = run_all(&dir.join("home"), &[model]);
        assert_eq!(
            lines(&done),
            ["no Couldn't add Ford Fiesta - Model.7z: this is a car model/sound; add it together with its NGP Physics zip"]
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
