//! Where the classic stages are in a source's file names: the RSF-shaped stage folders (`Tracks<id>.ini` and the
//! stage's files together) and the loose ones (`track-<n>_<letter>.*` files, the stage's `Tracks.ini` section and
//! weathers elsewhere in the tree). Names only; pure.

use crate::content::source::Entry;

/// The files of a stage variant a stage needs to play are `rbr_formats::rsf::REQUIRED`; these also name one.
const VARIANT_EXTENSIONS: [&str; 8] = ["lbs", "col", "trk", "dls", "mat", "ini", "fnc", "lb2"];

/// Files that are never copied: programs and logs.
const SKIPPED: [&str; 11] = ["exe", "dll", "msi", "bat", "cmd", "com", "scr", "sh", "so", "log", "tmp"];

/// One stage in a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub number: u32,
    /// The stage folder (RSF shape), or the folder holding the loose `track-<n>_*` files.
    pub dir: Vec<String>,
    /// A stage folder with its own `Tracks<n>.ini`.
    pub rsf: bool,
}

/// `Tracks<n>.ini`'s number.
pub fn tracks_number(file: &str) -> Option<u32> {
    let digits = file.to_ascii_lowercase().strip_prefix("tracks")?.strip_suffix(".ini")?.to_owned();
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok())?
}

/// The number and variant letter of `track-<n>_<L>.<ext>` where the extension names a variant file.
pub fn variant_file(file: &str) -> Option<(u32, char)> {
    let (stem, ext) = file.rsplit_once('.')?;
    let lower = ext.to_ascii_lowercase();
    VARIANT_EXTENSIONS.contains(&lower.as_str()).then_some(())?;
    let rest = stem.get(..6).filter(|p| p.eq_ignore_ascii_case("track-")).and(stem.get(6..))?;
    let (number, letter) = rest.split_once('_')?;
    let mut letters = letter.chars();
    match (number.parse().ok(), letters.next(), letters.next()) {
        (Some(n), Some(l), None) if l.is_ascii_alphabetic() => Some((n, l.to_ascii_uppercase())),
        _ => None,
    }
}

fn same(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && within_or_same(a, b)
}

fn within_or_same(path: &[String], dir: &[String]) -> bool {
    path.len() >= dir.len() && path.iter().zip(dir).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// The stages the file names show: first the folders with a `Tracks<n>.ini`, then the loose ones.
pub fn stages(entries: &[Entry]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for entry in entries {
        let Some((file, dir)) = entry.path.split_last() else { continue };
        if let Some(number) = tracks_number(file) {
            if !found.iter().any(|f| f.number == number && same(&f.dir, dir)) {
                found.push(Found { number, dir: dir.to_vec(), rsf: true });
            }
        }
    }
    for entry in entries {
        let Some((file, dir)) = entry.path.split_last() else { continue };
        let Some((number, _)) = variant_file(file) else { continue };
        let inside = found.iter().any(|f| f.rsf && within_or_same(dir, &f.dir));
        if !inside && !found.iter().any(|f| f.number == number && same(&f.dir, dir)) {
            found.push(Found { number, dir: dir.to_vec(), rsf: false });
        }
    }
    found
}

fn skipped(file: &str) -> bool {
    file.rsplit_once('.').is_some_and(|(_, e)| SKIPPED.iter().any(|s| e.eq_ignore_ascii_case(s)))
}

/// The stage's own files, each with its path in the stage folder: all of a stage folder's but its `Tracks<n>.ini` and
/// `TrackSettings<n>.ini` (rewritten), or a loose stage's `track-<n>_*` files and folders and its
/// `TextureFilenameMap<n>.ini`.
pub fn files<'a>(entries: &'a [Entry], stage: &Found) -> Vec<(&'a Entry, Vec<String>)> {
    let own = format!("track-{}_", stage.number);
    let remap = format!("texturefilenamemap{}.ini", stage.number);
    let mut out = Vec::new();
    for entry in entries {
        if entry.path.len() <= stage.dir.len() || !within_or_same(&entry.path, &stage.dir) {
            continue;
        }
        let rel = entry.path[stage.dir.len()..].to_vec();
        let (first, file) = (rel[0].to_ascii_lowercase(), entry.file_lower());
        let settings = file.starts_with("tracksettings") && file.ends_with(".ini");
        let keep = if stage.rsf {
            !(rel.len() == 1 && (tracks_number(&file).is_some() || settings))
        } else {
            first.starts_with(&own) || (rel.len() == 1 && file == remap)
        };
        if keep && !skipped(&file) {
            out.push((entry, rel));
        }
    }
    out
}

/// The files the Add reads before it extracts anything: the `Tracks`/`TrackSettings` inis and the text files, small.
pub fn meta(entries: &[Entry]) -> Vec<&Entry> {
    entries
        .iter()
        .filter(|e| {
            let file = e.file_lower();
            let ini = file.ends_with(".ini") && (file.starts_with("tracks") || file.starts_with("tracksettings"));
            (ini || file.ends_with(".txt")) && e.size <= 8 << 20
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(files: &[&str]) -> Vec<Entry> {
        files.iter().map(|f| Entry { path: f.split('/').map(str::to_owned).collect(), size: 1 }).collect()
    }

    #[test]
    fn names_tell_the_two_shapes_apart() {
        let all = entries(&[
            "Maps/101-Autio/Tracks101.ini",
            "Maps/101-Autio/track-101_O.lbs",
            "Maps/101-Autio/CustomData/x.ini",
            "Maps/Track-8994_O.lbs",
            "Maps/track-8994_M.lbs",
            "Maps/track-8994_O_textures.rbz",
            "Maps/track-77_E.col",
            "Maps/Tracks.ini",
        ]);
        let found = stages(&all);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].rsf && found[0].number == 101 && found[0].dir == ["Maps", "101-Autio"]);
        assert!(!found[1].rsf && found[1].number == 8994 && found[1].dir == ["Maps"]);
        assert_eq!(found[2].number, 77);
        let rel: Vec<_> = files(&all, &found[0]).iter().map(|(_, r)| r.join("/")).collect();
        assert_eq!(rel, ["track-101_O.lbs", "CustomData/x.ini"]);
        let rel: Vec<_> = files(&all, &found[1]).iter().map(|(_, r)| r.join("/")).collect();
        assert_eq!(rel, ["Track-8994_O.lbs", "track-8994_M.lbs", "track-8994_O_textures.rbz"]);
    }

    #[test]
    fn the_variant_files_and_the_marker() {
        assert_eq!(variant_file("track-12_O.lbs"), Some((12, 'O')));
        assert_eq!(variant_file("Track-12_e.INI"), Some((12, 'E')));
        assert_eq!(variant_file("track-12_O_textures.rbz"), None);
        assert_eq!(variant_file("track-103_21_Falstone II"), None);
        assert_eq!(variant_file("track-12_O.dds"), None);
        assert_eq!(tracks_number("Tracks101.ini"), Some(101));
        assert_eq!(tracks_number("tracks.ini"), None);
        assert_eq!(tracks_number("TrackSettings101.ini"), None);
    }
}
