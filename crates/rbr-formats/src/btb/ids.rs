//! RSF stage ids for BTB folders: which `stages_data.json` id (1000 and up) names each `RX_CONTENT/Tracks/<folder>/`.
//!
//! Three independent traces, in precedence order:
//!
//! - `splash_id`: a `<id>_*.png` splash picture in the folder (physical evidence; `Kinamba Yala`'s
//!   `1079_kinambayala.png`);
//! - `rallysimfans_personal.ini` `[stage<id>] name=<folder>` (a driven stage; only ids 1000 and up count: `Makkola`
//!   and `Nechanice - Presin` also name older non-BTB stages 549 and 560);
//! - `rsfdata/cache/stages_data.json` names, normalized ([`normalize`]): trailing `-` dropped, case and
//!   non-alphanumerics dropped, accents folded (`Gästabudstrofen-` finds `Gastabudstrofen`).
//!
//! Two folders match nothing reliable: `BikolTardos12` (the json's `Bikol Tardos`, 1002, lacks the folder's `12`)
//! and `Myttaala Gravel` (the json's `Myttäälä`, 1059, lacks `Gravel`; the personal ini's mojibake `Mytt..l..`
//! matches neither exactly). Both are reported, not guessed.

use crate::rsf::{stage_list, ListEntry};
use crate::{ini, stage_files};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where a folder's id came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdSource {
    /// A `<id>_*.png` in the folder.
    Splash,
    /// A `[stage<id>]` entry of `rallysimfans_personal.ini` naming the folder.
    PersonalIni,
    /// A `stages_data.json` name that normalizes to the folder name.
    StageList,
}

/// The splash id: the leading digits of the first `<digits>_*.png` in `folder` (case-insensitive).
pub fn splash_id(folder: &Path) -> Option<u32> {
    let mut names: Vec<String> =
        std::fs::read_dir(folder).ok()?.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    names.sort();
    names.into_iter().find_map(|n| {
        let lower = n.to_ascii_lowercase();
        let stem = lower.strip_suffix(".png")?;
        let (digits, rest) = stem.split_at(stem.find(|c: char| !c.is_ascii_digit()).unwrap_or(stem.len()));
        rest.strip_prefix('_').and(digits.parse().ok())
    })
}

/// The `[stage<id>] name=...` entries of the install's `rallysimfans_personal.ini` (lossy, as read).
pub fn personal_names(root: &Path) -> HashMap<u32, String> {
    let path = root.join("rallysimfans_personal.ini");
    let bytes = std::fs::read(path).unwrap_or_default();
    let Ok(sections) = ini::parse(&String::from_utf8_lossy(&bytes)) else { return HashMap::new() };
    sections
        .into_iter()
        .filter_map(|s| {
            let id: u32 = s
                .name
                .strip_prefix("stage")?
                .bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| s.name[5..].parse().ok())??;
            Some((id, s.get("name")?.trim().to_owned()))
        })
        .collect()
}

/// Folds a stage name for comparison: lowercase, accents to ASCII, only alphanumerics kept.
pub fn normalize(name: &str) -> String {
    name.chars().filter_map(fold).collect()
}

fn fold(c: char) -> Option<char> {
    if c.is_ascii_alphanumeric() {
        return Some(c.to_ascii_lowercase());
    }
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ŵ' => 'w',
        'ź' | 'ż' | 'ž' => 'z',
        _ if c.is_alphanumeric() => c.to_lowercase().next().unwrap_or(c),
        _ => return None,
    })
}

/// The BTB id of `folder`: the splash id, else a `personal` entry (id 1000 and up) naming the folder exactly, else a
/// `stages_data.json` BTB name (id 1000 and up) that normalizes to the folder name after dropping trailing `-`.
pub fn btb_id(
    folder: &Path,
    personal: &HashMap<u32, String>,
    list: &HashMap<u32, ListEntry>,
) -> Option<(u32, IdSource)> {
    if let Some(id) = splash_id(folder) {
        return Some((id, IdSource::Splash));
    }
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some((&id, _)) = personal.iter().find(|(&id, n)| id >= 1000 && **n == name) {
        return Some((id, IdSource::PersonalIni));
    }
    let want = normalize(&name);
    let mut found: Option<u32> = None;
    let mut ambiguous = false;
    for (&id, entry) in list {
        if id < 1000 {
            continue;
        }
        let Some(stage) = entry.get("name") else { continue };
        if normalize(stage.trim_end_matches('-')) == want {
            ambiguous |= found.is_some();
            found = Some(id);
        }
    }
    (!ambiguous).then_some(found).flatten().map(|id| (id, IdSource::StageList))
}

/// Every `RX_CONTENT/Tracks/*` folder's BTB id: matches in folder order, then the unmatched folders. `root` is the
/// RSF install.
pub fn match_ids(root: &Path) -> (Vec<(PathBuf, u32, IdSource)>, Vec<PathBuf>) {
    let personal = personal_names(root);
    let list = stage_list(root);
    let tracks = stage_files::in_dir(root, "RX_CONTENT\\Tracks");
    let mut folders: Vec<PathBuf> = tracks
        .and_then(|t| std::fs::read_dir(t).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    let mut matched = Vec::new();
    let mut unmatched = Vec::new();
    for folder in folders {
        match btb_id(&folder, &personal, &list) {
            Some((id, source)) => matched.push((folder, id, source)),
            None => unmatched.push(folder),
        }
    }
    (matched, unmatched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_normalize_across_accents_and_punctuation() {
        assert_eq!(normalize("Gästabudstrofen-"), normalize("Gastabudstrofen"));
        assert_eq!(normalize("Serra d'Agua"), normalize("Serra d Agua"));
        assert_eq!(normalize("Vyskälä SS3"), normalize("VyskalaSS3"));
        assert_eq!(normalize("Bikol Tardos"), "bikoltardos");
        assert_ne!(normalize("Bikol Tardos"), normalize("BikolTardos12"));
        assert_eq!(normalize("Pirttikulma Snow"), normalize("Pirttikulma snow"));
    }

    #[test]
    fn splash_prefers_the_folder_picture() {
        let root = std::env::temp_dir().join(format!("rbr-btb-ids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("Kinamba Yala");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("1079_kinambayala.png"), b"png").unwrap();
        std::fs::write(folder.join("kinamba_yala.jpg"), b"jpg").unwrap();
        assert_eq!(splash_id(&folder), Some(1079));
        let personal = HashMap::from([(1079, "Kinamba Yala".into())]);
        assert_eq!(btb_id(&folder, &personal, &HashMap::new()), Some((1079, IdSource::Splash)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn old_stage_ids_do_not_claim_btb_folders() {
        // `Makkola` and `Nechanice - Presin` also name pre-BTB stages 549/560: only 1000-and-up ids count, so a
        // personal entry alone never decides when the json names the BTB stage.
        let root = std::env::temp_dir().join(format!("rbr-btb-ids-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("Makkola");
        std::fs::create_dir_all(&folder).unwrap();
        let personal = HashMap::from([(549, "Makkola".into())]);
        let list = HashMap::from([(1068, ListEntry::from([("name".into(), "Makkola-".into())]))]);
        assert_eq!(btb_id(&folder, &personal, &list), Some((1068, IdSource::StageList)));
        assert_eq!(btb_id(&folder, &personal, &HashMap::new()), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ambiguous_names_stay_unmatched() {
        let list = HashMap::from([
            (1002, ListEntry::from([("name".into(), "Bikol Tardos".into())])),
            (1059, ListEntry::from([("name".into(), "Myttäälä".into())])),
        ]);
        let root = std::env::temp_dir().join(format!("rbr-btb-ids-amb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["BikolTardos12", "Myttaala Gravel"] {
            let folder = root.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            assert_eq!(btb_id(&folder, &HashMap::new(), &list), None, "{name}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
