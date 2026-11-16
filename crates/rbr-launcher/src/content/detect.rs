//! What one dropped or picked item is, from its file name and the relative paths inside it (a folder walk or an
//! archive listing; only names, never contents). Pure, so every rule is tested over lists of names.

use std::collections::{BTreeMap, HashSet};

use crate::car_packs::{strip_end, PHYSICS};

const AUDIO: [&str; 5] = [".ogg", ".wav", ".mp3", ".flac", ".oga"];

/// A BTB stage's files, by name (ignoring case): the rule of `rbr_btb::track::is_track_folder`.
const STAGE_FILES: [&str; 4] = ["track.ini", "objectlist.ini", "objects.zip", "textures.zip"];

pub const RAR: &str = "RAR archives can't be read: extract it, then add the folder";
pub const INSTALLER: &str = "installers can't be read: extract it with 7-Zip, then add the folder";
pub const CLASSIC: &str = "classic-format stages aren't supported yet; BTB stages are";
pub const UNKNOWN: &str = "not recognised: ReRBR adds BTB stages, co-drivers, pace notes, NGP car packs and NGP";

/// A BTB stage's folder inside an item: its path parts (empty: the item itself) and the folder name it is added as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageDir {
    pub dir: Vec<String>,
    pub name: String,
}

/// One thing an item holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// The BTB stages it holds, outermost folders only.
    Stages(Vec<StageDir>),
    /// A co-driver and/or pace notes.
    Codriver,
    /// An NGP car's `<Car> - NGP Physics.zip`, its `<Model> - Model.7z`, its `<Car> - banks.7z`.
    Physics,
    Model,
    Banks,
    /// NGP's own package (`.7z` or folder).
    Ngp,
    /// The retail disc, an `.iso` or a folder.
    Disc,
    /// Not added, and why.
    Refused(String),
}

impl Kind {
    /// The kind as the player reads it.
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Stages(s) if s.len() == 1 => "BTB stage",
            Kind::Stages(_) => "BTB stages",
            Kind::Codriver => "co-driver or pace notes",
            Kind::Physics => "NGP car",
            Kind::Model => "NGP car model",
            Kind::Banks => "NGP car sounds",
            Kind::Ngp => "NGP",
            Kind::Disc => "disc image",
            Kind::Refused(_) => "not supported",
        }
    }
}

/// An item's entry path as lowercase and original parts, empty and `.` parts dropped.
fn parts(entry: &str) -> Vec<&str> {
    entry.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect()
}

/// An item's name without its archive end.
pub fn stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    for end in [".zip", ".7z"] {
        if lower.ends_with(end) {
            return &name[..name.len() - end.len()];
        }
    }
    name
}

/// Whether an archive named `archive` with these entry names looks like a co-driver or pace-note pack.
pub fn looks_like_pack(archive: &str, entries: &[String]) -> bool {
    let lower = archive.to_ascii_lowercase();
    let named = ["pacenote", "luppis", "codriver", "co-driver"].iter().any(|k| lower.contains(k));
    entries.iter().any(|entry| {
        let e = entry.replace('\\', "/").to_ascii_lowercase();
        e.rsplit('/').next() == Some("pacenote.ini")
            || e.contains("config/pacenotes/")
            || e.contains("mypacenotes/")
            || (e.contains("sounds/") && AUDIO.iter().any(|a| e.ends_with(a)))
            || (named && e.contains('/') && e.ends_with(".ini"))
    })
}

/// The BTB stage folders among the files `entries`: each folder holding all of [`STAGE_FILES`], the outermost of
/// nested ones. `item` names the item, for a stage at its top.
fn stage_dirs(item: &str, entries: &[Vec<&str>]) -> Vec<StageDir> {
    // Lowercase folder path to its original parts and the lowercase names of the files directly in it.
    let mut folders: BTreeMap<String, (Vec<&str>, HashSet<String>)> = BTreeMap::new();
    for entry in entries {
        let Some((file, dir)) = entry.split_last() else { continue };
        let key = dir.join("/").to_ascii_lowercase();
        folders.entry(key).or_insert_with(|| (dir.to_vec(), HashSet::new())).1.insert(file.to_ascii_lowercase());
    }
    let mut found: Vec<(String, Vec<&str>)> = Vec::new();
    // The sorted keys put a folder before the ones inside it.
    for (key, (dir, files)) in &folders {
        let complete = STAGE_FILES.iter().all(|f| files.contains(*f));
        let inside = found.iter().any(|(outer, _)| outer.is_empty() || key.starts_with(&format!("{outer}/")));
        if complete && !inside {
            found.push((key.clone(), dir.clone()));
        }
    }
    found
        .into_iter()
        .map(|(_, dir)| {
            let name = dir.last().map_or_else(|| stem(item).to_owned(), |n| (*n).to_owned());
            StageDir { dir: dir.iter().map(|p| (*p).to_owned()).collect(), name }
        })
        .collect()
}

/// Whether `entries` hold NGP's package at their top.
fn is_ngp(entries: &[String]) -> bool {
    let have = crate::disc_install::with_folders(
        entries
            .iter()
            .map(|e| e.replace('\\', "/"))
            .map(|e| e.trim_start_matches('/').to_owned())
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str),
    );
    crate::ngp_install::NGP.iter().all(|w| have.contains(&w.to_lowercase()))
}

/// Whether the files look like a classic-format stage: `Maps/track-*` files, or `Tracks.ini` / `TrackSettings*.ini`.
fn is_classic(entries: &[Vec<&str>]) -> bool {
    entries.iter().any(|e| {
        let file = e.last().map(|f| f.to_ascii_lowercase()).unwrap_or_default();
        let in_maps = e.len() >= 2 && e[..e.len() - 1].iter().any(|d| d.eq_ignore_ascii_case("maps"));
        (in_maps && file.starts_with("track-"))
            || file == "tracks.ini"
            || (file.starts_with("tracksettings") && file.ends_with(".ini"))
    })
}

/// What the item named `name` is. `dir`: a folder. `entries`: the paths of its files below it (a walk or an
/// archive's listing). Everything it holds, or why it is refused.
pub fn classify(name: &str, dir: bool, entries: &[String]) -> Vec<Kind> {
    let lower = name.to_ascii_lowercase();
    if !dir {
        if lower.ends_with(".rar") {
            return vec![Kind::Refused(RAR.into())];
        }
        if [".exe", ".msi"].iter().any(|e| lower.ends_with(e)) {
            return vec![Kind::Refused(INSTALLER.into())];
        }
        if lower.ends_with(".iso") {
            return vec![Kind::Disc];
        }
    }
    let seven = lower.ends_with(".7z");
    let zip = lower.ends_with(".zip");
    if !dir && !seven && !zip {
        return vec![Kind::Refused(UNKNOWN.into())];
    }
    let hdr = |e: &String| matches!(parts(e)[..], [f] if f.eq_ignore_ascii_case("data1.hdr"));
    if dir && entries.iter().any(hdr) {
        return vec![Kind::Disc];
    }
    if (dir || seven) && is_ngp(entries) {
        return vec![Kind::Ngp];
    }
    let split: Vec<Vec<&str>> = entries.iter().map(|e| parts(e)).filter(|p| !p.is_empty()).collect();
    let mut kinds = Vec::new();
    let stages = stage_dirs(name, &split);
    // The rest of the item, outside the stages: a stage's own sounds are not a co-driver's.
    let outside = |e: &Vec<&str>| {
        !stages
            .iter()
            .any(|s| e.len() > s.dir.len() && s.dir.iter().zip(e.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b)))
    };
    let rest: Vec<String> = split.iter().filter(|e| outside(e)).map(|e| e.join("/")).collect();
    if !stages.is_empty() {
        kinds.push(Kind::Stages(stages.clone()));
    }
    if looks_like_pack(name, &rest) {
        kinds.push(Kind::Codriver);
    }
    if zip && split.iter().any(|e| e.last().is_some_and(|f| f.eq_ignore_ascii_case("common.lsp")) && e.len() <= 2) {
        kinds.push(Kind::Physics);
    }
    if seven {
        let ends = |e: &Vec<&str>, end: &str| e.last().is_some_and(|f| f.to_ascii_lowercase().ends_with(end));
        if split.iter().any(|e| e.len() >= 2 && ends(e, ".sgc")) {
            kinds.push(Kind::Model);
        }
        if split.iter().any(|e| ends(e, ".bank") && !ends(e, ".strings.bank")) {
            kinds.push(Kind::Banks);
        }
    }
    if kinds.is_empty() {
        kinds.push(Kind::Refused(if is_classic(&split) { CLASSIC } else { UNKNOWN }.into()));
    }
    kinds
}

/// The car a physics, model or banks download is for, from its file name: its name without the end gvrc.de gives.
pub fn car_name(file: &str, end: &str) -> String {
    strip_end(file, end).unwrap_or_else(|| stem(file)).to_owned()
}

/// The car a physics zip is for.
pub fn physics_name(file: &str) -> String {
    car_name(file, PHYSICS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn stage(root: &str) -> Vec<String> {
        ["track.ini", "ObjectList.ini", "objects.zip", "TEXTURES.ZIP", "driveline.ini"]
            .iter()
            .map(|f| if root.is_empty() { f.to_string() } else { format!("{root}/{f}") })
            .collect()
    }

    fn kinds(name: &str, dir: bool, entries: &[String]) -> Vec<Kind> {
        classify(name, dir, entries)
    }

    fn refused(kinds: &[Kind]) -> &str {
        match kinds {
            [Kind::Refused(why)] => why,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn packs_are_recognised_by_their_entries() {
        assert!(looks_like_pack("a.zip", &names(&["Voice/PaceNote.ini"])));
        assert!(looks_like_pack("a.zip", &names(&["x\\config\\pacenotes\\v3.ini"])));
        assert!(looks_like_pack("a.zip", &names(&["Plugins/Pacenote/sounds/pkg/1.ogg"])));
        assert!(looks_like_pack("a.zip", &names(&["MyPacenotes/Finland/x.ini"])));
        assert!(!looks_like_pack("a.zip", &names(&["sounds/readme.txt", "game.exe"])));
        assert!(!looks_like_pack("a.zip", &names(&["Finland/x.ini"])));
        assert!(looks_like_pack("Luppis-notes.zip", &names(&["Finland/x.ini"])));
        assert!(looks_like_pack("My Co-Driver.7z", &names(&["Finland/x.ini"])));
        assert!(!looks_like_pack("Luppis.zip", &names(&["x.ini"])));
    }

    #[test]
    fn a_btb_stage_is_found_at_the_top_or_in_a_folder() {
        let at_top = kinds("Autio I Gravel.zip", false, &stage(""));
        let expect = StageDir { dir: vec![], name: "Autio I Gravel".into() };
        assert_eq!(at_top, [Kind::Stages(vec![expect])]);
        let wrapped = kinds("a.7z", false, &stage("Autio I Gravel"));
        let expect = StageDir { dir: vec!["Autio I Gravel".into()], name: "Autio I Gravel".into() };
        assert_eq!(wrapped, [Kind::Stages(vec![expect])]);
        assert_eq!(wrapped[0].label(), "BTB stage");
        let folder = kinds("Autio", true, &stage("").iter().map(|e| e.replace('/', "\\")).collect::<Vec<_>>());
        assert_eq!(folder, [Kind::Stages(vec![StageDir { dir: vec![], name: "Autio".into() }])]);
    }

    #[test]
    fn an_archive_with_two_stages_and_an_rx_content_tree() {
        let mut two = stage("Pack/A");
        two.extend(stage("Pack/B"));
        let Kind::Stages(found) = &kinds("pack.zip", false, &two)[0] else { panic!() };
        let found: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(found, ["A", "B"]);
        let tree = stage("RX_CONTENT/Tracks/Lappi");
        let got = kinds("rsf.7z", false, &tree);
        assert_eq!(
            got,
            [Kind::Stages(vec![StageDir {
                dir: ["RX_CONTENT", "Tracks", "Lappi"].map(Into::into).into(),
                name: "Lappi".into()
            }])]
        );
        // A folder missing one of the four files is no stage.
        let mut partial = stage("Lappi");
        partial.retain(|e| !e.to_lowercase().ends_with("objects.zip"));
        assert_eq!(refused(&kinds("lappi.zip", false, &partial)), UNKNOWN);
    }

    #[test]
    fn a_mixed_archive_holds_each_kind_and_a_stages_sounds_are_its_own() {
        let mut mixed = stage("Lappi");
        mixed.extend(names(&["Lappi/sounds/engine.ogg", "Pacenote/PaceNote.ini", "Pacenote/sounds/en/1.ogg"]));
        let got = kinds("both.zip", false, &mixed);
        assert!(matches!(got[0], Kind::Stages(_)) && got[1] == Kind::Codriver && got.len() == 2, "{got:?}");
        // Sounds inside the stage alone do not make a co-driver.
        let mut alone = stage("Lappi");
        alone.push("Lappi/sounds/engine.ogg".into());
        assert_eq!(kinds("lappi.zip", false, &alone).len(), 1);
    }

    #[test]
    fn co_drivers_and_pace_notes() {
        assert_eq!(kinds("Janne V3.zip", false, &names(&["Pacenote/PaceNote.ini"])), [Kind::Codriver]);
        assert_eq!(kinds("notes", true, &names(&["MyPacenotes/Finland/x.ini"])), [Kind::Codriver]);
    }

    #[test]
    fn car_pieces() {
        let physics = names(&["common.lsp", "engine.lsp"]);
        assert_eq!(kinds("Ford Fiesta R5 - NGP Physics.zip", false, &physics), [Kind::Physics]);
        let nested = names(&["Ford_Fiesta_R5/common.lsp"]);
        assert_eq!(kinds("x.zip", false, &nested), [Kind::Physics]);
        let model = names(&["Cars/Ford Fiesta/Ford Fiesta.sgc", "Cars/Ford Fiesta/a.dds"]);
        assert_eq!(kinds("Ford Fiesta - Model.7z", false, &model), [Kind::Model]);
        let banks = names(&["AudioFMOD/fiesta.bank", "AudioFMOD/fiesta.strings.bank"]);
        assert_eq!(kinds("Ford Fiesta R5 - banks.7z", false, &banks), [Kind::Banks]);
        assert_eq!(physics_name("Ford Fiesta R5 - NGP Physics.zip"), "Ford Fiesta R5");
        assert_eq!(car_name("Ford Fiesta - Model.7z", " - Model.7z"), "Ford Fiesta");
    }

    #[test]
    fn ngp_and_the_disc() {
        let ngp = names(&["physics.rbz", "Plugins/NGP.dll", "AudioFMOD/a.bank", "AudioFMOD/b.bank"]);
        assert_eq!(kinds("RBR_NGP-7.4.778.507.7z", false, &ngp), [Kind::Ngp]);
        assert_eq!(kinds("NGP", true, &ngp), [Kind::Ngp]);
        assert_eq!(kinds("RBR.iso", false, &[]), [Kind::Disc]);
        assert_eq!(kinds("disc", true, &names(&["Data1.hdr", "data1.cab"])), [Kind::Disc]);
    }

    #[test]
    fn what_is_refused_and_why() {
        assert_eq!(refused(&kinds("stage.rar", false, &[])), RAR);
        assert_eq!(refused(&kinds("Setup.EXE", false, &[])), INSTALLER);
        assert_eq!(refused(&kinds("setup.msi", false, &[])), INSTALLER);
        let classic = names(&["Maps/track-111_E.lbs", "Maps/Track-111_E.dds"]);
        assert_eq!(refused(&kinds("stage.zip", false, &classic)), CLASSIC);
        assert_eq!(refused(&kinds("stage.zip", false, &names(&["Tracks.ini"]))), CLASSIC);
        assert_eq!(refused(&kinds("stage.zip", false, &names(&["x/TrackSettings_111.ini"]))), CLASSIC);
        assert_eq!(refused(&kinds("holiday.zip", false, &names(&["a.txt"]))), UNKNOWN);
        assert_eq!(refused(&kinds("readme.pdf", false, &[])), UNKNOWN);
        assert_eq!(refused(&kinds("folder", true, &names(&["a.txt"]))), UNKNOWN);
    }
}
