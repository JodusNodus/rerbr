//! The community's V3 co-driver packs (Luppis's list, `get_page::VOICES_URL`, read 2026-11-22): who speaks each
//! voice folder and in which language, for the Co-drivers page, and each pack's calls and page for Get more. Names,
//! credits and links only; the voices are their authors' downloads.
//!
//! In the V3 setup the call style is the plugin's whole install: its corners file maps each grade to a recording
//! (`EASYLEFT` plays `six_left` in Janne Laahanen's Numeric A, `five_left` in his Numeric B) and its texts say how
//! (`language/*/…/corners/strings.ini`), so a pack ships one install per style. Janne's folder names carry the style
//! (`Janne Laahanen EnNu A NF`, [`scale`]); other packs give A and B the same folder (benofficial2's, DDFRacer's), so
//! the page names those by the install's own texts instead.

/// A pack on the list: its title, who made it, the language, its calls, its page and its voice folders.
#[derive(Debug)]
pub struct Pack {
    pub title: &'static str,
    pub by: &'static str,
    pub language: &'static str,
    /// The styles it comes in, in a few words.
    pub calls: &'static str,
    /// Where its download is: the author's page, video or file.
    pub page: &'static str,
    pub voices: &'static [Voice],
}

/// A voice folder: its name under the plugin's `sounds/` (its last part; a trailing `*` takes every folder starting
/// so, named by [`scale`]), and who speaks it.
#[derive(Debug)]
pub struct Voice {
    pub folder: &'static str,
    pub person: &'static str,
}

const fn voice(folder: &'static str, person: &'static str) -> Voice {
    Voice { folder, person }
}

/// The packs, by language.
pub static PACKS: &[Pack] = &[
    Pack {
        title: "Picure Chinese V3",
        by: "Picure",
        language: "Chinese",
        calls: "Hairpin, 1 to 7, direction first",
        page: "https://drive.google.com/file/d/1V10N1SFrJPPvdTVfa0Hq9st6vi2hNtCr/view",
        voices: &[voice("Picure-V3", "Picure")],
    },
    Pack {
        title: "TTR RBR Navigátor V3",
        by: "JackCzech & JordanT106",
        language: "Czech",
        calls: "Numbers and words, several scales; one voice and two synthetic ones. Installers (.exe) only: \
                extract one, then add the folder",
        page: "https://drive.google.com/drive/folders/1Z6fui3FldD288XjFUs2R492hkB_k7PJF",
        voices: &[voice("JackCzech", "JackCzech")],
    },
    Pack {
        title: "Darren V3",
        by: "Darren Blythe",
        language: "English",
        calls: "Numeric A, number first",
        page: "https://drive.google.com/file/d/1F7e3Vn4Rogkh33F-jMXwNUI7SEHoJziX/view",
        voices: &[voice("Darren V3", "Darren")],
    },
    Pack {
        title: "DDFRacer co-driver for V3",
        by: "DDFRacer",
        language: "English",
        calls: "Numeric A and B",
        page: "https://drive.google.com/file/d/1ZoDZLfnevupS-ohbOun-X5godHQBpLhA/view",
        voices: &[voice("DDFV3_2", "DDFRacer")],
    },
    Pack {
        title: "Janne Laahanen V3",
        by: "Janne Laahanen",
        language: "English",
        calls: "Numeric A to H, Hybrid A to H and Descriptive, number or direction first, each also simple",
        page: "https://drive.google.com/file/d/1ncS9fzM7rwuNoKzNWEIqE8wSEyasM_-X/view",
        voices: &[voice("Janne Laahanen En*", "Janne Laahanen")],
    },
    Pack {
        title: "Nicky Grist V3",
        by: "benofficial2",
        language: "English",
        calls: "Numeric A or B, a download each",
        page: "https://youtu.be/OvbvRu2pFSY",
        voices: &[voice("NickyGristV3 (benofficial2)", "Nicky Grist")],
    },
    Pack {
        title: "Rita Fullon V3",
        by: "benofficial2",
        language: "English",
        calls: "Numeric A or B, a download each",
        page: "https://www.youtube.com/watch?v=JdH9ryF8gwg",
        voices: &[voice("RitaFullonV3 (benofficial2)", "Rita Fullon")],
    },
    Pack {
        title: "Robert Star V3",
        by: "benofficial2",
        language: "English",
        calls: "Numeric A or B, a download each",
        page: "https://youtu.be/XaxLWx2_r2g",
        voices: &[voice("RobertStarV3 (benofficial2)", "Robert Star")],
    },
    Pack {
        title: "Janne Laahanen V3",
        by: "Janne Laahanen",
        language: "Finnish",
        calls: "Numeric A to H, Hybrid A to H and Descriptive A and B, each also simple",
        page: "https://drive.google.com/file/d/1MP_-p6dLBseavlbLTrgeF7akxyewz2SL/view",
        voices: &[voice("Janne Laahanen Fi*", "Janne Laahanen")],
    },
    Pack {
        title: "Julie Gaudin V3",
        by: "benofficial2",
        language: "French",
        calls: "Numeric A or B, a download each",
        page: "https://youtu.be/X8zEZgKDNqY",
        voices: &[voice("JulieGaudinV3 (benofficial2)", "Julie Gaudin")],
    },
    Pack {
        title: "Pierre Dufond V3",
        by: "benofficial2",
        language: "French",
        calls: "Numeric A or B, a download each",
        page: "https://www.youtube.com/watch?v=FMBgB8516n8",
        voices: &[voice("PierreDufondV3 (benofficial2)", "Pierre Dufond")],
    },
    Pack {
        title: "Anna Keller",
        by: "Smo",
        language: "German",
        calls: "Three voices: Voll, 6 to 3, 90, 1, Kehre, Spitz, direction first",
        page: "https://github.com/Smo-RBR/RBR-Janne-V3-Beifahrer",
        voices: &[
            voice("Anna Keller", "Anna Keller"),
            voice("Anna Lorenz", "Anna Lorenz"),
            voice("Selena Winter", "Selena Winter"),
        ],
    },
    Pack {
        title: "Beppe IA V3",
        by: "Bisto & L'Ema",
        language: "Italian",
        calls: "6 to 1 and tornante, direction first",
        page: "https://drive.google.com/file/d/1Qmz5o5Q5l6upjceLcyRZO-d5BlTB7aWo/view",
        voices: &[voice("Beppe IA", "Beppe")],
    },
    Pack {
        title: "Paolo Lombardi V3",
        by: "Paolo Lombardi",
        language: "Italian",
        calls: "Numbers",
        page: "https://mega.nz/file/4mABHTIS#uJtS02TmmcJgu2dCgMK5GVqLLe8-HN6WjiTZti9_0JE",
        voices: &[],
    },
    Pack {
        title: "Persian Co-Driver",
        by: "ruhollahkuhsar",
        language: "Persian",
        calls: "Numeric A or B, number or direction first, each also simple",
        page: "https://drive.google.com/file/d/1k7vUKtWj6ae58tME78mvnHKVFrnuf-cO/view",
        voices: &[voice("R Kuhsar *", "R. Kuhsar")],
    },
    Pack {
        title: "Hugo Mendonça V3",
        by: "benofficial2",
        language: "Portuguese",
        calls: "Numeric A or B, a download each, also with \"topo\" for crests",
        page: "https://youtu.be/HaWkt-WA3fo",
        voices: &[voice("HugoMendoncaV3 (benofficial2)", "Hugo Mendonça")],
    },
    Pack {
        title: "Hrenovalke V3",
        by: "Hrenovalke",
        language: "Russian",
        calls: "Numeric A, number first. A folder of loose files: download it whole",
        page: "https://drive.google.com/drive/folders/1Q1xtVqRh8oxZkttVOrzJ1coJYw99d1y4",
        voices: &[voice("hrenovalke", "Hrenovalke")],
    },
    Pack {
        title: "SpaCry V3",
        by: "Cestomano",
        language: "Spanish",
        calls: "Hybrid B Simple, synthetic voices",
        page: "https://www.cestomano.com/spacry",
        voices: &[voice("spacry", "SpaCry")],
    },
];

impl Voice {
    /// Whether the installed folder `folder` (its path under `sounds/`) is this voice: by its last part, ignoring case.
    pub fn is(&self, folder: &str) -> bool {
        let name = folder.rsplit(['/', '\\']).next().unwrap_or(folder);
        match self.folder.strip_suffix('*') {
            Some(prefix) => name.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix)),
            None => self.folder.eq_ignore_ascii_case(name),
        }
    }

    /// The style its folder `folder` names, for a prefix entry ([`scale`] of the rest).
    pub fn scale(&self, folder: &str) -> Option<String> {
        let prefix = self.folder.strip_suffix('*')?;
        let name = folder.rsplit(['/', '\\']).next().unwrap_or(folder);
        // The prefix ends in the language letters of the code (`En*`): the code starts there.
        let at = prefix.rfind(' ').map_or(0, |k| k + 1);
        scale(name.get(at..)?)
    }
}

/// The pack and voice of the installed folder `folder` (its path under `sounds/`).
pub fn find(folder: &str) -> Option<(&'static Pack, &'static Voice)> {
    PACKS.iter().find_map(|p| p.voices.iter().find(|v| v.is(folder)).map(|v| (p, v)))
}

/// The place of the installed folder `folder` in the catalogue, if it is in it.
pub fn rank(folder: &str) -> Option<usize> {
    PACKS.iter().flat_map(|p| p.voices).position(|v| v.is(folder))
}

/// The style a Janne Laahanen code names (`EnNu A NF` is "Numeric A, number first"): two letters of language, `Nu`,
/// `Hy` or `De`, then a letter, `Simple`, `NF` or `DF`, each optional.
pub fn scale(code: &str) -> Option<String> {
    let mut words = code.split_whitespace();
    let family = match words.next()?.get(2..)? {
        "Nu" => "Numeric",
        "Hy" => "Hybrid",
        "De" => "Descriptive",
        _ => return None,
    };
    let mut out = family.to_owned();
    for word in words {
        match word {
            "Simple" => out += ", simple",
            "NF" => out += ", number first",
            "DF" => out += ", direction first",
            letter if letter.len() == 1 && letter.bytes().all(|b| b.is_ascii_uppercase()) => {
                out = format!("{out} {letter}")
            }
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_found_by_their_last_part_in_any_case() {
        let (pack, voice) = find("nested/ritafullonv3 (BENOFFICIAL2)").unwrap();
        assert_eq!((pack.language, voice.person), ("English", "Rita Fullon"));
        assert_eq!(find("english/male/Darren V3").unwrap().1.person, "Darren");
        assert!(find("english/male/steve").is_none());
        // A prefix entry takes its codes, in its own language.
        let (pack, voice) = find("Janne Laahanen FiNu B Simple").unwrap();
        assert_eq!((pack.language, voice.person), ("Finnish", "Janne Laahanen"));
        assert_eq!(voice.scale("Janne Laahanen FiNu B Simple").as_deref(), Some("Numeric B, simple"));
        assert_eq!(find("Janne Laahanen EnNu A").unwrap().0.language, "English");
        assert_eq!(
            rank("R Kuhsar EnNu A DF"),
            PACKS.iter().flat_map(|p| p.voices).position(|v| v.person == "R. Kuhsar")
        );
    }

    #[test]
    fn janne_laahanens_codes_name_their_styles() {
        assert_eq!(scale("EnNu A NF").as_deref(), Some("Numeric A, number first"));
        assert_eq!(scale("EnNu A Simple DF").as_deref(), Some("Numeric A, simple, direction first"));
        assert_eq!(scale("EnNu A").as_deref(), Some("Numeric A"));
        assert_eq!(scale("EnHy G").as_deref(), Some("Hybrid G"));
        assert_eq!(scale("EnDe").as_deref(), Some("Descriptive"));
        assert_eq!(scale("FiDe B Simple").as_deref(), Some("Descriptive B, simple"));
        assert_eq!(scale("EnXx A"), None);
        assert_eq!(scale("EnNu Fast"), None);
    }

    #[test]
    fn every_folder_is_listed_once() {
        let mut folders: Vec<String> =
            PACKS.iter().flat_map(|p| p.voices.iter().map(|v| v.folder.to_lowercase())).collect();
        let n = folders.len();
        folders.sort();
        folders.dedup();
        assert_eq!(folders.len(), n);
    }
}
