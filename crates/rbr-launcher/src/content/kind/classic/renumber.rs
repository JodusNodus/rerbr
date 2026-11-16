//! Giving a stage another number: the file names, the text that holds the number, and the entry names of the
//! stage's texture archives. Pure, over names and bytes. The binaries (`.lbs` `.col` `.trk` `.dls` `.mat` `.fnc`) hold
//! no `track-<n>` name (none of the 4126 plain ones of an RSF install does), so they are copied as they are.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

type Fail = String;

/// The name of a stage file with its number `old` turned into `new`: `track-<old>_*`, `Tracks<old>.ini`,
/// `TrackSettings<old>.ini` and `TextureFilenameMap<old>.ini`; any other name as it is.
pub fn file_name(name: &str, old: u32, new: u32) -> String {
    let lower = name.to_ascii_lowercase();
    for (prefix, tail) in
        [("track-", "_"), ("tracks", ".ini"), ("tracksettings", ".ini"), ("texturefilenamemap", ".ini")]
    {
        let head = format!("{prefix}{old}");
        if lower.starts_with(&head) && lower[head.len()..].starts_with(tail) {
            return format!("{}{new}{}", &name[..prefix.len()], &name[head.len()..]);
        }
    }
    name.to_owned()
}

/// A path of parts with its first file or folder name renamed ([`file_name`]; a texture folder is `track-<old>_*`
/// too).
pub fn path(parts: &[String], old: u32, new: u32) -> Vec<String> {
    let mut out = parts.to_vec();
    if let Some(first) = out.first_mut() {
        *first = file_name(first, old, new);
    }
    out
}

/// `text` with every `track-<old>_` (ignoring case) turned into `track-<new>_`: a texture remap's keys and values.
pub fn track_references(text: &str, old: u32, new: u32) -> String {
    let (lower, needle) = (text.to_ascii_lowercase(), format!("track-{old}_"));
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(&needle) {
        let start = at + found;
        out.push_str(&text[at..start]);
        out.push_str(&format!("{}{new}_", &text[start..start + 6]));
        at = start + needle.len();
    }
    out.push_str(&text[at..]);
    out
}

/// A texture archive's entry name, renamed when it starts with `track-<old>_` (either slash).
pub fn entry_name(name: &str, old: u32, new: u32) -> String {
    file_name(name, old, new)
}

/// Copies the zip `from` to `to` with its entries renamed ([`entry_name`]), without recompressing them. Fails when
/// `from` is not a zip.
pub fn rewrite_zip(
    from: &Path,
    to: &Path,
    old: u32,
    new: u32,
    progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
) -> Result<(), Fail> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", from.display());
    let mut input = zip::ZipArchive::new(BufReader::new(File::open(from).map_err(|e| at(&e))?)).map_err(|e| at(&e))?;
    let mut out = zip::ZipWriter::new(BufWriter::new(File::create(to).map_err(|e| format!("{}: {e}", to.display()))?));
    for k in 0..input.len() {
        let file = input.by_index_raw(k).map_err(|e| at(&e))?;
        let name = file.name().map_err(|e| at(&e))?.into_owned();
        let (name, size) = (entry_name(&name, old, new), file.compressed_size());
        out.raw_copy_file_rename(file, name).map_err(|e| at(&e))?;
        progress(size)?;
    }
    out.finish().map_err(|e| at(&e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn names_with_the_number_are_renamed_and_the_others_are_not() {
        let n = |name: &str| file_name(name, 101, 10_000);
        assert_eq!(n("track-101_O.lbs"), "track-10000_O.lbs");
        assert_eq!(n("Track-101_o_textures.RBZ"), "Track-10000_o_textures.RBZ");
        assert_eq!(n("track-101_13_Autiovaara II"), "track-10000_13_Autiovaara II");
        assert_eq!(n("Tracks101.ini"), "Tracks10000.ini");
        assert_eq!(n("TrackSettings101.ini"), "TrackSettings10000.ini");
        assert_eq!(n("TextureFilenameMap101.ini"), "TextureFilenameMap10000.ini");
        assert_eq!(n("track-1010_O.lbs"), "track-1010_O.lbs");
        assert_eq!(n("track-101.ini"), "track-101.ini");
        assert_eq!(n("101-Autiovaara.dds"), "101-Autiovaara.dds");
        assert_eq!(path(&["track-101_O_textures".into(), "a.dds".into()], 101, 7), ["track-7_O_textures", "a.dds"]);
    }

    #[test]
    fn a_remap_keeps_other_stages_requests() {
        let text = "; 101\nTrack-101_o_textures\\dry\\a.dds\ttrack-13_o_textures\\dry\\a.dds\n\
                    track-1010_o_textures\\x\ttrack-101_O_textures\\y\n";
        assert_eq!(
            track_references(text, 101, 5),
            "; 101\nTrack-5_o_textures\\dry\\a.dds\ttrack-13_o_textures\\dry\\a.dds\n\
             track-1010_o_textures\\x\ttrack-5_O_textures\\y\n"
        );
    }

    #[test]
    fn a_zip_is_renamed_without_recompressing() {
        let dir = crate::test_dir("classic-rbz");
        let (from, to) = (dir.join("a.rbz"), dir.join("b.rbz"));
        let mut w = zip::ZipWriter::new(File::create(&from).unwrap());
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file("track-101_O_textures\\dry\\a.dds", deflate).unwrap();
        w.write_all(b"aaaa").unwrap();
        w.start_file("Track-101_O_textures/b.dds", deflate).unwrap();
        w.write_all(b"bb").unwrap();
        w.start_file("other/c.dds", deflate).unwrap();
        w.write_all(b"c").unwrap();
        w.finish().unwrap();
        rewrite_zip(&from, &to, 101, 12_345, &mut |_| Ok(())).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&to).unwrap()).unwrap();
        let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        assert_eq!(names, ["track-12345_O_textures\\dry\\a.dds", "Track-12345_O_textures/b.dds", "other/c.dds"]);
        let mut text = String::new();
        zip.by_index(0).unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "aaaa");
        assert!(rewrite_zip(&dir.join("none"), &dir.join("x"), 1, 2, &mut |_| Ok(())).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
