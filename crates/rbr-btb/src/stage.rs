//! Playing a BTB stage: a stage of its own, a folder stage (`rbr_formats::stage_files`) named `track-<number>_M` whose
//! files [`prepare`] writes into a folder of ours (`notes/btb.md` "Playing"). The RSF plugin makes the game load retail
//! map 41 instead; ReRBR has no file opener to hook, so nothing of map 41 is kept but what [`SKY_MAP`] says.

use crate::classic;
use crate::track::{Physics, Track};
use rbr_formats::{ini, stage_files};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

/// The variant letter of a prepared stage's name: its files are the same whatever the weather, so there is one.
pub const LETTER: char = 'M';
/// The retail map whose presentation a BTB stage borrows: the weather rows (and their `TrackSettings.ini` sky sections,
/// `Season::of_btb`), the particles' wind and the time of day's site. One place to change it. Physics inputs
/// (wear, wetness, wet meshes, `.mat` entry) come from the row as for any stage.
pub const SKY_MAP: u32 = 41;
/// The stage whose `Maps/` files dress a BTB stage (`SKY_MAP`'s variant [`LETTER`]).
pub const SKY_STAGE: &str = "track-41_M";
/// The fence file of a BTB stage: retail's empty one (map 4's, 16 bytes: the magic, no groups, the tail), so the
/// stage has no fence posts. Map 41's own `.fnc` has posts for map 41's road, not the BTB one. Putting a retail
/// map's `.fnc` bytes here instead would give the stage its fences.
const FENCE: [u8; 16] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x20, 0, 0, 0];
/// The file naming the BTB folder a prepared folder was built from.
pub const SOURCE_FILE: &str = "btb.ini";
/// Raised when the built files change, so that the folders an older build prepared are built again.
const BUILD: u32 = 2;
/// The prepared files' extensions.
const EXTENSIONS: [&str; 6] = ["lbs", "dls", "mat", "trk", "col", "fnc"];

/// The stage name of the BTB stage `number` prepared in `out`: `<out>/track-<number>_M`.
pub fn stage_name(out: &Path, number: u32) -> String {
    format!("{}/track-{number}_{LETTER}", out.display())
}

/// Writes `<out>/track-<number>_M.{lbs,dls,mat,trk,col,fnc}` and `<out>/btb.ini` for `track` and returns the stage name
/// ([`stage_name`]). The plugin rebuilds the files on every load; we keep them while `btb.ini` holds the same [`stamp`]
/// of the track folder (half a second to two on each load).
pub fn prepare(track: &Track, number: u32, out: &Path) -> Result<String, String> {
    let name = stage_name(out, number);
    let source = format!("[BTB]\nfolder={}\nstamp={}\n", track.folder.display(), stamp(&track.folder));
    let built = EXTENSIONS.iter().all(|ext| Path::new(&format!("{name}.{ext}")).is_file());
    if built && std::fs::read_to_string(out.join(SOURCE_FILE)).is_ok_and(|text| text == source) {
        return Ok(name);
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |ext: &str, bytes: Vec<u8>| {
        let path = format!("{name}.{ext}");
        std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))
    };
    write("lbs", classic::lbs(&track.spawn()?))?;
    write("dls", classic::dls(&track.pace_notes()?.notes))?;
    write("mat", classic::mat(&track.surface_maps()?.maps))?;
    write("trk", classic::trk_from(track, crate::D3dx::Windows)?)?;
    write("col", crate::col::build(track)?)?;
    write("fnc", FENCE.to_vec())?;
    // Last: a build cut short is not taken for a finished one.
    std::fs::write(out.join(SOURCE_FILE), source).map_err(|e| format!("{SOURCE_FILE}: {e}"))?;
    Ok(name)
}

/// [`BUILD`] and the names, sizes and modification times of the files in the track `folder`, hashed.
fn stamp(folder: &Path) -> String {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| (entry.file_name(), meta.len(), meta.modified().ok()))
        })
        .collect();
    files.sort();
    let mut hasher = DefaultHasher::new();
    (BUILD, files).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The BTB folder a prepared stage folder (`<out>`, or the stage name's folder) was built from; `None` when it has no
/// `btb.ini` or the folder is gone.
pub fn source(stage_folder: &Path) -> Option<PathBuf> {
    let text = std::fs::read(stage_files::in_dir(stage_folder, SOURCE_FILE)?).ok()?;
    let sections = ini::parse(&String::from_utf8_lossy(&text)).ok()?;
    let folder = crate::track::find_entry(crate::track::find_section(&sections, "BTB")?, "folder")?;
    Some(PathBuf::from(folder)).filter(|f| f.is_dir())
}

/// The `--surface` argument of the stage's physics: `track.ini` `physics=`; the plugin's `E14_4a1330` override.
pub fn surface(track: &Track) -> Option<&'static str> {
    Some(match track.info.physics()? {
        Physics::Gravel => "gravel",
        Physics::Tarmac => "tarmac",
        Physics::Snow => "snow",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stage_is_named_by_its_own_number_and_has_no_fence() {
        let name = stage_name(Path::new("/files/btb/Pirttikulma"), 1_234_567);
        assert_eq!(name, "/files/btb/Pirttikulma/track-1234567_M");
        assert_eq!(stage_files::parts(&name), Some((1_234_567, LETTER)));
        assert_eq!(stage_files::folder(&name), Some(Path::new("/files/btb/Pirttikulma")));
        assert!(rbr_formats::fnc::parse(&FENCE).unwrap().groups.is_empty());
    }

    #[test]
    fn source_reads_btb_ini() {
        let root = std::env::temp_dir().join(format!("rbr-btb-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (out, tracks) = (root.join("out"), root.join("tracks"));
        std::fs::create_dir_all(&out).unwrap();
        std::fs::create_dir_all(&tracks).unwrap();
        assert_eq!(source(&out), None);
        std::fs::write(out.join(SOURCE_FILE), format!("[BTB]\nfolder={}\n", tracks.display())).unwrap();
        assert_eq!(source(&out), Some(tracks.clone()));
        std::fs::remove_dir(&tracks).unwrap();
        assert_eq!(source(&out), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_stamp_follows_the_folders_files() {
        let root = std::env::temp_dir().join(format!("rbr-btb-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("track.ini"), "a").unwrap();
        let first = stamp(&root);
        assert_eq!(stamp(&root), first);
        std::fs::write(root.join("track.ini"), "ab").unwrap();
        assert_ne!(stamp(&root), first);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
