//! Playing a BTB stage: the plugin makes the game load retail map 41, variant M, replacing five of its files
//! (`Maps\track-41_M.{lbs,dls,col,trk,mat}`) with ones built from the BTB folder (`notes/btb.md` "Playing").
//! [`prepare`] writes those five files into a folder of ours and the stage name `<out>/track-41_M` then loads as a
//! folder stage, the other files of map 41 coming from the install's `Maps/` (`rbr_formats::stage_files`).

use crate::classic;
use crate::track::{Physics, Track};
use rbr_formats::{ini, stage_files};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

/// The stage name the plugin plays every BTB stage as: retail map 41, variant M.
pub const TRACK: &str = "track-41_M";
/// The file naming the BTB folder a prepared folder was built from.
pub const SOURCE_FILE: &str = "btb.ini";
/// Raised when the built files change, so that the folders an older build prepared are built again.
const BUILD: u32 = 1;

/// Writes `<out>/track-41_M.{lbs,dls,col,trk,mat}` and `<out>/btb.ini` for `track` and returns the stage name
/// `<out>/track-41_M`. `rsf_root` is the RallySimFans install, recorded in `btb.ini`. The plugin rebuilds them on every
/// load; we keep them while `btb.ini` holds the same [`stamp`] of the track folder (half a second to two on each load).
pub fn prepare(track: &Track, rsf_root: &Path, out: &Path) -> Result<String, String> {
    let name = format!("{}/{TRACK}", out.display());
    let source = format!(
        "[BTB]\nfolder={}\nrsf={}\nstamp={}\n",
        track.folder.display(),
        rsf_root.display(),
        stamp(&track.folder)
    );
    let built = ["lbs", "dls", "mat", "trk", "col"].iter().all(|ext| out.join(format!("{TRACK}.{ext}")).is_file());
    if built && std::fs::read_to_string(out.join(SOURCE_FILE)).is_ok_and(|text| text == source) {
        return Ok(name);
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |ext: &str, bytes: Vec<u8>| {
        let path = out.join(format!("{TRACK}.{ext}"));
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    write("lbs", classic::lbs(&track.spawn()?))?;
    write("dls", classic::dls(&track.pace_notes()?.notes))?;
    write("mat", classic::mat(&track.surface_maps()?.maps))?;
    write("trk", classic::trk_from(track, crate::D3dx::Windows)?)?;
    write("col", crate::col::build(track)?)?;
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
