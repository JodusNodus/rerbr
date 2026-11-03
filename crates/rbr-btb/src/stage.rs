//! Playing a BTB stage: the plugin makes the game load retail map 41, variant M, replacing five of its files
//! (`Maps\track-41_M.{lbs,dls,col,trk,mat}`) with ones built from the BTB folder (`notes/btb.md` "Playing").
//! [`prepare`] writes those five files into a folder of ours and the stage name `<out>/track-41_M` then loads as a
//! folder stage, the other files of map 41 coming from the install's `Maps/` (`rbr_formats::stage_files`).

use crate::classic;
use crate::track::{Physics, Track};
use rbr_formats::{ini, stage_files};
use std::path::{Path, PathBuf};

/// The stage name the plugin plays every BTB stage as: retail map 41, variant M.
pub const TRACK: &str = "track-41_M";
/// The file naming the BTB folder a prepared folder was built from.
pub const SOURCE_FILE: &str = "btb.ini";

/// Writes `<out>/track-41_M.{lbs,dls,col,trk,mat}` and `<out>/btb.ini` for `track` (always rebuilt, as the plugin does
/// on every load) and returns the stage name `<out>/track-41_M`. `rsf_root` is the RallySimFans install, recorded in
/// `btb.ini`.
pub fn prepare(track: &Track, rsf_root: &Path, out: &Path) -> Result<String, String> {
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
    let source = format!("[BTB]\nfolder={}\nrsf={}\n", track.folder.display(), rsf_root.display());
    std::fs::write(out.join(SOURCE_FILE), source).map_err(|e| format!("{SOURCE_FILE}: {e}"))?;
    Ok(format!("{}/{TRACK}", out.display()))
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
}
