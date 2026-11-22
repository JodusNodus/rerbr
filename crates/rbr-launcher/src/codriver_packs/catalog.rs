//! The community's V3 co-driver packs (Luppis's list, `get_page::VOICES_URL`, read 2026-11-22): who speaks each
//! voice folder and in which language, how each pack calls the corners, and its page, for the Co-drivers page's
//! "Not added yet" and Get more. Names, credits, links and the corners' words only; the voices are their authors'
//! downloads.
//!
//! In the V3 setup the call style is the plugin's whole install: its corners file maps each grade to a recording
//! (`EASYLEFT` plays `six_left` in Janne Laahanen's Numeric A, `five_left` in his Numeric B) and its texts say how
//! (`language/*/…/corners/strings.ini`), so a pack ships one install per style. Each pack's [`Calls`] are read from
//! its downloads as `choices` reads an added one; packs not read (an installer, loose files, no texts) have none.

/// A pack on the list: its title, who made it, the language, its calls in a few words, its page, its voice folders
/// and its call styles.
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
    /// Its call styles, one per install, as `choices::Ladder` reads them (none: not read).
    pub styles: &'static [Calls],
}

/// A call style: the preview's corners tightest to fastest (`, `-separated grades) and the second fastest one's
/// whole call, and whether it is a simple version (only the plain grades).
#[derive(Debug)]
pub struct Calls {
    pub grades: &'static str,
    pub said: &'static str,
    pub simple: bool,
}

const fn calls(grades: &'static str, said: &'static str) -> Calls {
    Calls { grades, said, simple: false }
}

const fn simple(grades: &'static str, said: &'static str) -> Calls {
    Calls { grades, said, simple: true }
}

/// A voice folder: its name under the plugin's `sounds/` (its last part; a trailing `*` takes every folder starting
/// so), and who speaks it.
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
        styles: &[calls("Hairpin, 1, 2, 3, 4, 5, 6, 7", "Left 6")],
    },
    Pack {
        title: "TTR RBR Navigátor V3",
        by: "JackCzech & JordanT106",
        language: "Czech",
        calls: "Numbers and words, several scales; one voice and two synthetic ones. Installers (.exe) only: \
                extract one, then add the folder",
        page: "https://drive.google.com/drive/folders/1Z6fui3FldD288XjFUs2R492hkB_k7PJF",
        voices: &[voice("JackCzech", "JackCzech")],
        styles: &[],
    },
    Pack {
        title: "Darren V3",
        by: "Darren Blythe",
        language: "English",
        calls: "Numeric A, number first",
        page: "https://drive.google.com/file/d/1F7e3Vn4Rogkh33F-jMXwNUI7SEHoJziX/view",
        voices: &[voice("Darren V3", "Darren")],
        styles: &[calls("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left")],
    },
    Pack {
        title: "DDFRacer co-driver for V3",
        by: "DDFRacer",
        language: "English",
        calls: "Numeric A and B",
        page: "https://drive.google.com/file/d/1ZoDZLfnevupS-ohbOun-X5godHQBpLhA/view",
        voices: &[voice("DDFV3_2", "DDFRacer")],
        styles: &[calls("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"), calls("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left")],
    },
    Pack {
        title: "Janne Laahanen V3",
        by: "Janne Laahanen",
        language: "English",
        calls: "Numeric A to H, Hybrid A to H and Descriptive, number or direction first, each also simple",
        page: "https://drive.google.com/file/d/1ncS9fzM7rwuNoKzNWEIqE8wSEyasM_-X/view",
        voices: &[voice("Janne Laahanen En*", "Janne Laahanen")],
        styles: &[
            calls("HP, Bad, Square, K, Medium, Fast, Easy, Flat", "Easy Left"),
            simple("HP, Bad, Square, K, Medium, Fast, Easy, Flat", "Easy Left"),
            calls("HP, Bad, Square, 3, 4, 5, 6, Flat", "6 Left"),
            simple("HP, Bad, Square, 3, 4, 5, 6, Flat", "6 Left"),
            calls("HP, Bad, Square, 2, 3, 4, 5, 6", "5 Left"),
            simple("HP, Bad, Square, 2, 3, 4, 5, 6", "5 Left"),
            calls("HP, Bad, Square, 4, 3, 2, 1, Flat", "1 Left"),
            simple("HP, Bad, Square, 4, 3, 2, 1, Flat", "1 Left"),
            calls("HP, Bad, Square, 5, 4, 3, 2, 1", "2 Left"),
            simple("HP, Bad, Square, 5, 4, 3, 2, 1", "2 Left"),
            calls("HP, Bad, Square, 8, 6, 4, 2, 1", "2 Left"),
            calls("HP, Bad, Square, 2, 4, 6, 8, 9", "8 Left"),
            calls("HP, Bad, Square, 2, 4, 5, 6, Flat", "6 Left"),
            calls("HP, Bad, Square, 5, 3, 2, 1, Flat", "1 Left"),
            calls("HP, 1, 2, 3, 4, 5, 6, Flat", "Left 6"),
            calls("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"),
            simple("HP, 1, 2, 3, 4, 5, 6, Flat", "Left 6"),
            simple("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"),
            calls("HP, 1, Square, 2, 3, 4, 5, 6", "Left 5"),
            calls("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left"),
            simple("HP, 1, Square, 2, 3, 4, 5, 6", "Left 5"),
            simple("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left"),
            calls("HP, 6, 5, 4, 3, 2, 1, Flat", "Left 1"),
            calls("HP, 6, 5, 4, 3, 2, 1, Flat", "1 Left"),
            simple("HP, 6, 5, 4, 3, 2, 1, Flat", "Left 1"),
            simple("HP, 6, 5, 4, 3, 2, 1, Flat", "1 Left"),
            calls("HP, 6, Square, 5, 4, 3, 2, 1", "Left 2"),
            calls("HP, 6, Square, 5, 4, 3, 2, 1", "2 Left"),
            simple("HP, 6, Square, 5, 4, 3, 2, 1", "Left 2"),
            simple("HP, 6, Square, 5, 4, 3, 2, 1", "2 Left"),
            calls("13, 11, 9, 8, 6, 4, 2, 1", "Left 2"),
            calls("13, 11, 9, 8, 6, 4, 2, 1", "2 Left"),
            calls("8, 7, 6, 5, 4, 3, 2, 1", "Left 2"),
            calls("8, 7, 6, 5, 4, 3, 2, 1", "2 Left"),
            simple("8, 7, 6, 5, 4, 3, 2, 1", "Left 2"),
            simple("8, 7, 6, 5, 4, 3, 2, 1", "2 Left"),
            calls("2, 3, 4, 5, 6, 7, 8, 9", "Left 8"),
            calls("2, 3, 4, 5, 6, 7, 8, 9", "8 Left"),
            simple("2, 3, 4, 5, 6, 7, 8, 9", "Left 8"),
            simple("2, 3, 4, 5, 6, 7, 8, 9", "8 Left"),
            calls("10, 8, 6, 5, 4, 3, 2, 1", "Left 2"),
            calls("10, 8, 6, 5, 4, 3, 2, 1", "2 Left"),
        ],
    },
    Pack {
        title: "Nicky Grist V3",
        by: "benofficial2",
        language: "English",
        calls: "Numeric A or B, a download each",
        page: "https://youtu.be/OvbvRu2pFSY",
        voices: &[voice("NickyGristV3 (benofficial2)", "Nicky Grist")],
        styles: &[calls("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"), calls("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left")],
    },
    Pack {
        title: "Rita Fullon V3",
        by: "benofficial2",
        language: "English",
        calls: "Numeric A or B, a download each",
        page: "https://www.youtube.com/watch?v=JdH9ryF8gwg",
        voices: &[voice("RitaFullonV3 (benofficial2)", "Rita Fullon")],
        styles: &[calls("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"), calls("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left")],
    },
    Pack {
        title: "Robert Star V3",
        by: "benofficial2",
        language: "English",
        calls: "Numeric A or B, a download each",
        page: "https://youtu.be/XaxLWx2_r2g",
        voices: &[voice("RobertStarV3 (benofficial2)", "Robert Star")],
        styles: &[calls("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"), calls("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left")],
    },
    Pack {
        title: "Janne Laahanen V3",
        by: "Janne Laahanen",
        language: "Finnish",
        calls: "Numeric A to H, Hybrid A to H and Descriptive A and B, each also simple",
        page: "https://drive.google.com/file/d/1MP_-p6dLBseavlbLTrgeF7akxyewz2SL/view",
        voices: &[voice("Janne Laahanen Fi*", "Janne Laahanen")],
        styles: &[
            calls("Serpo, Hidas, 90, 3, 4, 5, 6, Täys", "Vasen 6"),
            simple("Serpo, Hidas, 90, 3, 4, 5, 6, Täys", "Vasen 6"),
            calls("Serpo, Hidas, 90, 2, 3, 4, 5, 6", "Vasen 5"),
            simple("Serpo, Hidas, 90, 2, 3, 4, 5, 6", "Vasen 5"),
            calls("Serpo, Hidas, 90, 4, 3, 2, 1, Täys", "Vasen 1"),
            simple("Serpo, Hidas, 90, 4, 3, 2, 1, Täys", "Vasen 1"),
            calls("Serpo, Hidas, 90, 5, 4, 3, 2, 1", "Vasen 2"),
            simple("Serpo, Hidas, 90, 5, 4, 3, 2, 1", "Vasen 2"),
            calls("Serpo, Hidas, 90, 8, 6, 4, 2, 1", "Vasen 2"),
            calls("Serpo, Hidas, 90, 2, 4, 6, 8, 9", "Vasen 8"),
            calls("Serpo, Hidas, 90, 2, 4, 5, 6, Täys", "Vasen 6"),
            calls("Serpo, Hidas, 90, 5, 3, 2, 1, Täys", "Vasen 1"),
            calls("Serpo, Hidas, 90, Nopee, K, Eri, Helppo, Täys", "Vasen Helppo"),
            simple("Serpo, Hidas, 90, Nopee, K, Eri, Helppo, Täys", "Vasen Helppo"),
            calls("Serpo, Hidas, 90, K, Nopee, Eri, Helppo, Täys", "Vasen Helppo"),
            simple("Serpo, Hidas, 90, K, Nopee, Eri, Helppo, Täys", "Vasen Helppo"),
            calls("Serpo, 1, 2, 3, 4, 5, 6, Täys", "Vasen 6"),
            simple("Serpo, 1, 2, 3, 4, 5, 6, Täys", "Vasen 6"),
            calls("Serpo, 1, 90, 2, 3, 4, 5, 6", "Vasen 5"),
            simple("Serpo, 1, 90, 2, 3, 4, 5, 6", "Vasen 5"),
            calls("Serpo, 6, 5, 4, 3, 2, 1, Täys", "Vasen 1"),
            simple("Serpo, 6, 5, 4, 3, 2, 1, Täys", "Vasen 1"),
            calls("Serpo, 6, 90, 5, 4, 3, 2, 1", "Vasen 2"),
            simple("Serpo, 6, 90, 5, 4, 3, 2, 1", "Vasen 2"),
            calls("13, 11, 9, 8, 6, 4, 2, 1", "Vasen 2"),
            calls("8, 7, 6, 5, 4, 3, 2, 1", "Vasen 2"),
            simple("8, 7, 6, 5, 4, 3, 2, 1", "Vasen 2"),
            calls("2, 3, 4, 5, 6, 7, 8, 9", "Vasen 8"),
            simple("2, 3, 4, 5, 6, 7, 8, 9", "Vasen 8"),
            calls("10, 8, 6, 5, 4, 3, 2, 1", "Vasen 2"),
        ],
    },
    Pack {
        title: "Julie Gaudin V3",
        by: "benofficial2",
        language: "French",
        calls: "Numeric A or B, a download each",
        page: "https://youtu.be/X8zEZgKDNqY",
        voices: &[voice("JulieGaudinV3 (benofficial2)", "Julie Gaudin")],
        styles: &[
            calls("Épingle, un, deux, trois, quatre, cinq, six, à fond", "Gauche, six"),
            calls("Épingle, un, carré, deux, trois, quatre, cinq, six", "Gauche cinq"),
        ],
    },
    Pack {
        title: "Pierre Dufond V3",
        by: "benofficial2",
        language: "French",
        calls: "Numeric A or B, a download each",
        page: "https://www.youtube.com/watch?v=FMBgB8516n8",
        voices: &[voice("PierreDufondV3 (benofficial2)", "Pierre Dufond")],
        styles: &[
            calls("Épingle, un, deux, trois, quatre, cinq, six, à fond", "Gauche six"),
            calls("Épingle, un, carré, deux, trois, quatre, cinq, six", "Gauche cinq"),
        ],
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
        styles: &[calls("Kehre, 1, 90, 3, 4, 5, 6, Voll", "6 Links")],
    },
    Pack {
        title: "Beppe IA V3",
        by: "Bisto & L'Ema",
        language: "Italian",
        calls: "6 to 1 and tornante, direction first",
        page: "https://drive.google.com/file/d/1Qmz5o5Q5l6upjceLcyRZO-d5BlTB7aWo/view",
        voices: &[voice("Beppe IA", "Beppe")],
        styles: &[calls("Tornantino, 1, 2, 3, 4, 5, 6, Accenno", "Sinistra 6")],
    },
    Pack {
        title: "Paolo Lombardi V3",
        by: "Paolo Lombardi",
        language: "Italian",
        calls: "Numbers",
        page: "https://mega.nz/file/4mABHTIS#uJtS02TmmcJgu2dCgMK5GVqLLe8-HN6WjiTZti9_0JE",
        voices: &[],
        styles: &[],
    },
    Pack {
        title: "Persian Co-Driver",
        by: "ruhollahkuhsar",
        language: "Persian",
        calls: "Numeric A or B, number or direction first, each also simple",
        page: "https://drive.google.com/file/d/1k7vUKtWj6ae58tME78mvnHKVFrnuf-cO/view",
        voices: &[voice("R Kuhsar *", "R. Kuhsar")],
        styles: &[
            calls("HP, 1, 2, 3, 4, 5, 6, Flat", "Left 6"),
            calls("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"),
            simple("HP, 1, 2, 3, 4, 5, 6, Flat", "Left 6"),
            simple("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left"),
            calls("HP, 1, Square, 2, 3, 4, 5, 6", "Left 5"),
            calls("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left"),
            simple("HP, 1, Square, 2, 3, 4, 5, 6", "Left 5"),
            simple("HP, 1, Square, 2, 3, 4, 5, 6", "5 Left"),
        ],
    },
    Pack {
        title: "Hugo Mendonça V3",
        by: "benofficial2",
        language: "Portuguese",
        calls: "Numeric A or B, a download each, also with \"topo\" for crests",
        page: "https://youtu.be/HaWkt-WA3fo",
        voices: &[voice("HugoMendoncaV3 (benofficial2)", "Hugo Mendonça")],
        styles: &[
            calls("gancho, um, dois, três, quatro, cinco, seis, a fundo", "Esquerda seis"),
            calls("gancho, um, reta, dois, três, quatro, cinco, seis", "Esquerda cinco"),
        ],
    },
    Pack {
        title: "Hrenovalke V3",
        by: "Hrenovalke",
        language: "Russian",
        calls: "Numeric A, number first. A folder of loose files: download it whole",
        page: "https://drive.google.com/drive/folders/1Q1xtVqRh8oxZkttVOrzJ1coJYw99d1y4",
        voices: &[voice("hrenovalke", "Hrenovalke")],
        styles: &[],
    },
    Pack {
        title: "SpaCry V3",
        by: "Cestomano",
        language: "Spanish",
        calls: "Hybrid B Simple, synthetic voices",
        page: "https://www.cestomano.com/spacry",
        voices: &[voice("spacry", "SpaCry")],
        styles: &[],
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
}

/// The pack and voice of the installed folder `folder` (its path under `sounds/`).
pub fn find(folder: &str) -> Option<(&'static Pack, &'static Voice)> {
    PACKS.iter().find_map(|p| p.voices.iter().find(|v| v.is(folder)).map(|v| (p, v)))
}

/// The place of the installed folder `folder` in the catalogue, if it is in it.
pub fn rank(folder: &str) -> Option<usize> {
    PACKS.iter().flat_map(|p| p.voices).position(|v| v.is(folder))
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
        assert_eq!(find("Janne Laahanen EnNu A").unwrap().0.language, "English");
        assert_eq!(
            rank("R Kuhsar EnNu A DF"),
            PACKS.iter().flat_map(|p| p.voices).position(|v| v.person == "R. Kuhsar")
        );
    }

    #[test]
    fn every_style_names_the_previews_corners_once() {
        for pack in PACKS {
            for (k, calls) in pack.styles.iter().enumerate() {
                assert_eq!(calls.grades.split(", ").count(), rbr_sound::preview::LADDER.len(), "{}", pack.title);
                let same = |o: &Calls| (o.grades, o.said, o.simple) == (calls.grades, calls.said, calls.simple);
                assert!(!pack.styles[..k].iter().any(same), "{}: {calls:?} twice", pack.title);
            }
        }
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
