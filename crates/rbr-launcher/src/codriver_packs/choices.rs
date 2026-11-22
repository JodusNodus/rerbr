//! The voice folders of every source (the RallySimFans install's plugin, each owned pack) as the Co-drivers page
//! offers them: by language, then how they call the corners, then who speaks. A V3 pack installs one call style
//! each (its corners file and its texts), so a folder calls as its source does: how the texts say the preview's
//! corners (`rbr_sound::preview::LADDER`), tightest to fastest ([`Ladder`]). The catalogue ([`catalog`]) names a
//! folder's person and language; any other folder is a person of its own, in the language its path or its install
//! names. Each folder is checked for the preview's corners in its source's style: one missing some is incomplete,
//! one with none is left out. What the catalogue has and no source does is offered to get. Read at rescan, never
//! per draw.

use std::collections::HashMap;
use std::path::Path;

use rbr_formats::pacenote::codriver;
use rbr_formats::stage_files;
use rbr_sound::preview::LADDER;

use super::catalog::{self, Pack, PACKS};

/// A voice source: an owned pack (none: the RallySimFans install), its plugin folder and voice folders, and the
/// style and range set it plays.
pub struct Source<'a> {
    pub pack: Option<&'a str>,
    pub dir: &'a Path,
    pub voices: &'a [String],
    pub style: &'a str,
    pub ranges: &'a str,
}

/// How an install says the preview's corners: the grades, tightest to fastest, without the direction word
/// (`Hairpin, 1, 2, 3, 4, 5, 6, Flat`; Janne Laahanen's texts' `HP` reads as `Hairpin`, as the RallySimFans
/// install's), and the second fastest corner's whole call, which shows the order (`6 Left`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ladder {
    pub grades: Vec<String>,
    pub said: String,
}

/// What a ladder's grades are: numbers (with one word at most among the inner six, as Square), numbers and words,
/// or words only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Numbers,
    Hybrid,
    Words,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Numbers, Kind::Hybrid, Kind::Words];

    /// Its heading in the Calls list.
    pub fn heading(self) -> &'static str {
        match self {
            Kind::Numbers => "Numbers",
            Kind::Hybrid => "Numbers and words (hybrid)",
            Kind::Words => "Words (descriptive)",
        }
    }

    /// Its name before a ladder.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Numbers => "Numbers",
            Kind::Hybrid => "Hybrid",
            Kind::Words => "Words",
        }
    }
}

/// One to nine in words: English, French, German, Italian, Portuguese and Spanish.
const NUMBER_WORDS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", //
    "un", "deux", "trois", "quatre", "cinq", "sept", "huit", "neuf", //
    "eins", "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun", //
    "uno", "due", "tre", "quattro", "cinque", "sei", "sette", "otto", "nove", //
    "um", "dois", "três", "quatro", "cinco", "seis", "sete", "oito", //
    "dos", "tres", "cuatro", "siete", "ocho", "nueve",
];

/// Whether a grade is a number: up to 19 (a `90` is the square corner's angle), or one in words.
fn is_number(grade: &str) -> bool {
    let grade = grade.to_lowercase();
    grade.parse::<u32>().is_ok_and(|n| n < 20) || NUMBER_WORDS.contains(&grade.as_str())
}

/// A grade as the page shows it: `HP` spelled out.
fn grade(text: &str) -> String {
    match text.eq_ignore_ascii_case("HP") {
        true => "Hairpin".to_owned(),
        false => text.to_owned(),
    }
}

impl Ladder {
    /// A ladder as the catalogue writes it: the grades `, `-separated.
    pub fn parse(grades: &str, said: &str) -> Ladder {
        Ladder { grades: grades.split(", ").map(grade).collect(), said: said.to_owned() }
    }

    /// The grades, `, `-separated.
    pub fn text(&self) -> String {
        self.grades.join(", ")
    }

    /// Whether it says the grades `other` says, in any case.
    pub fn same_grades(&self, other: &Ladder) -> bool {
        self.grades.len() == other.grades.len()
            && self.grades.iter().zip(&other.grades).all(|(a, b)| a.to_lowercase() == b.to_lowercase())
    }

    /// Whether the direction comes first (`Left 6`): the whole call does not start with its grade.
    pub fn direction_first(&self) -> bool {
        let grade = self.grades.iter().rev().nth(1).map(|g| g.to_lowercase()).unwrap_or_default();
        !self.said.to_lowercase().starts_with(&grade)
    }

    pub fn kind(&self) -> Kind {
        let inner = self.grades.get(1..self.grades.len().saturating_sub(1)).unwrap_or_default();
        match (self.grades.iter().any(|g| is_number(g)), inner.iter().filter(|g| !is_number(g)).count()) {
            (false, _) => Kind::Words,
            (true, 0 | 1) => Kind::Numbers,
            _ => Kind::Hybrid,
        }
    }

    /// Whether it calls as `ladder` with `simple`: the same grades, order and simplicity.
    fn calls_as(&self, simple: bool, other: &Ladder, other_simple: bool) -> bool {
        self.same_grades(other) && self.direction_first() == other.direction_first() && simple == other_simple
    }
}

/// How the install with `calls` and texts `strings` says the preview's corners; none when it does not say them all.
/// The direction word is the one every corner says (its first five letters, so Italian's `Sinistra` and `Sinistro`
/// are one), else `LEFT`'s text.
fn ladder(calls: &codriver::Calls, strings: &HashMap<String, String>) -> Option<Ladder> {
    let said: Vec<&str> = LADDER
        .iter()
        .map(|id| calls.notes.get(id).and_then(|c| strings.get(&c.name.to_ascii_uppercase())).map(String::as_str))
        .collect::<Option<_>>()?;
    let words = |text: &str| -> Vec<String> {
        let trim = |w: &str| w.trim_matches(|c: char| c.is_ascii_punctuation()).to_owned();
        text.split_whitespace().map(trim).filter(|w| !w.is_empty()).collect()
    };
    let stem = |word: &str| word.chars().take(5).collect::<String>().to_lowercase();
    let in_all = |word: &String| said.iter().all(|t| words(t).iter().any(|w| stem(w) == stem(word)));
    let left = words(said[0]).into_iter().find(in_all).or_else(|| strings.get("LEFT").cloned());
    let left = stem(left.as_deref().unwrap_or("Left"));
    let grades: Vec<String> = said
        .iter()
        .map(|t| grade(&words(t).into_iter().filter(|w| stem(w) != left).collect::<Vec<_>>().join(" ")))
        .collect();
    if grades.iter().any(String::is_empty) {
        return None;
    }
    Some(Ladder { grades, said: said[said.len() - 2].to_owned() })
}

/// One voice folder of one source as the page offers it: who speaks it in which language, how its source calls the
/// corners (none: its texts do not say them all), whether that is a simple version (only the plain grades, `Simple`
/// in the folder's name), and how many of the preview's corners it has a recording for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub pack: Option<String>,
    pub folder: String,
    pub person: String,
    pub language: String,
    pub ladder: Option<Ladder>,
    pub simple: bool,
    pub corners: usize,
}

impl Choice {
    pub fn complete(&self) -> bool {
        self.corners == LADDER.len()
    }

    /// Whether it is the voice folder `folder` of source `pack`.
    pub fn is(&self, pack: Option<&str>, folder: &str) -> bool {
        self.pack.as_deref() == pack && self.folder == folder
    }

    /// Whether it says the grades `other` says (or neither says them all).
    pub fn same_grades(&self, other: &Choice) -> bool {
        match (&self.ladder, &other.ladder) {
            (Some(a), Some(b)) => a.same_grades(b),
            (a, b) => a.is_none() && b.is_none(),
        }
    }

    pub fn direction_first(&self) -> Option<bool> {
        self.ladder.as_ref().map(Ladder::direction_first)
    }

    pub fn kind(&self) -> Option<Kind> {
        self.ladder.as_ref().map(Ladder::kind)
    }

    /// Whether it calls as `other`: the same grades, order and simplicity.
    pub fn calls_as(&self, other: &Choice) -> bool {
        self.same_grades(other) && self.direction_first() == other.direction_first() && self.simple == other.simple
    }

    /// Whether it calls as the catalogue's `calls`.
    fn calls_as_listed(&self, calls: &catalog::Calls) -> bool {
        let listed = Ladder::parse(calls.grades, calls.said);
        self.ladder.as_ref().is_some_and(|l| l.calls_as(self.simple, &listed, calls.simple))
    }
}

/// Language names a voice folder's path may start with (`french/male/christophe`).
const LANGUAGES: &[&str] = &[
    "chinese",
    "czech",
    "dutch",
    "english",
    "finnish",
    "french",
    "german",
    "hungarian",
    "italian",
    "japanese",
    "norwegian",
    "persian",
    "polish",
    "portuguese",
    "russian",
    "spanish",
    "swedish",
];

/// An uncatalogued folder's language: its path's first part when that names one, else its install's (the editor's
/// `language`), capitalised; else `Other`.
fn language(folder: &str, install: Option<&str>) -> String {
    let first = folder.split(['/', '\\']).next().unwrap_or(folder).to_lowercase();
    let name = Some(first).filter(|f| LANGUAGES.contains(&f.as_str())).or_else(|| install.map(str::to_lowercase));
    let mut letters = name.as_deref().unwrap_or_default().trim().chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => "Other".to_owned(),
    }
}

/// The choices of the `sources`: the catalogued people first in the catalogue's order, then the other folders by
/// name, each in its sources' order.
pub fn choices(sources: &[Source]) -> Vec<Choice> {
    let mut found = Vec::new();
    for source in sources {
        let calls = codriver::load(source.dir, source.style, source.ranges).ok();
        let ladder = calls.as_ref().and_then(|c| ladder(c, &codriver::strings(source.dir)));
        let install = codriver::settings(source.dir).language;
        for folder in source.voices {
            let corners = match (&calls, codriver::voice_dir(source.dir, folder)) {
                (Some(calls), Some(path)) => {
                    let recorded = |file: &String| stage_files::in_dir(&path, file).is_some();
                    LADDER
                        .iter()
                        .filter(|id| calls.notes.get(id).is_some_and(|c| c.sounds.iter().any(recorded)))
                        .count()
                }
                _ => 0,
            };
            if corners == 0 {
                continue;
            }
            let (person, language) = match catalog::find(folder) {
                Some((pack, voice)) => (voice.person.to_owned(), pack.language.to_owned()),
                None => (folder.clone(), language(folder, install.as_deref())),
            };
            let name = folder.rsplit(['/', '\\']).next().unwrap_or(folder);
            let simple = name.split_whitespace().any(|w| w.eq_ignore_ascii_case("simple"));
            let pack = source.pack.map(str::to_owned);
            let choice =
                Choice { pack, folder: folder.clone(), person, language, ladder: ladder.clone(), simple, corners };
            found.push((catalog::rank(folder), choice));
        }
    }
    found.sort_by(|a, b| (a.0.is_none(), a.0, &a.1.person).cmp(&(b.0.is_none(), b.0, &b.1.person)));
    found.into_iter().map(|(_, c)| c).collect()
}

/// The languages of the choices, by name.
pub fn languages(choices: &[Choice]) -> Vec<&str> {
    let mut out: Vec<&str> = choices.iter().map(|c| c.language.as_str()).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The calls of the choices in `language`, one choice for each (the first): by kind (those not said last), most
/// people first.
pub fn calls_in<'a>(choices: &'a [Choice], language: &str) -> Vec<&'a Choice> {
    let here: Vec<&Choice> = choices.iter().filter(|c| c.language == language).collect();
    let mut out: Vec<(&Choice, usize)> = Vec::new();
    for c in &here {
        if !out.iter().any(|(o, _)| o.same_grades(c)) {
            let mut people: Vec<&str> = here.iter().filter(|o| o.same_grades(c)).map(|o| o.person.as_str()).collect();
            people.sort_unstable();
            people.dedup();
            out.push((c, people.len()));
        }
    }
    out.sort_by_key(|(c, people)| (c.kind().is_none(), c.kind(), std::cmp::Reverse(*people)));
    out.into_iter().map(|(c, _)| c).collect()
}

/// The choice passing `keep` most like `current`: complete first, then the same grades, kind, person, order and
/// simplicity, else the first in the list.
pub fn closest<'a>(
    choices: &'a [Choice],
    current: Option<&Choice>,
    keep: impl Fn(&Choice) -> bool,
) -> Option<&'a Choice> {
    let score = |c: &Choice| match current {
        Some(now) => [
            c.complete(),
            c.same_grades(now),
            c.kind() == now.kind(),
            c.person == now.person,
            c.direction_first() == now.direction_first(),
            c.simple == now.simple,
        ],
        None => [c.complete(), false, false, false, false, false],
    };
    let mut best: Option<(&Choice, [bool; 6])> = None;
    for c in choices.iter().filter(|c| keep(c)) {
        let s = score(c);
        if best.is_none_or(|(_, b)| s > b) {
            best = Some((c, s));
        }
    }
    best.map(|(c, _)| c)
}

/// The catalogue's languages no choice is in.
pub fn languages_to_get(choices: &[Choice]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for pack in PACKS {
        if !out.contains(&pack.language) && !choices.iter().any(|c| c.language == pack.language) {
            out.push(pack.language);
        }
    }
    out.sort_unstable();
    out
}

/// The catalogue's packs in `language`.
pub fn packs_in(language: &str) -> Vec<&'static Pack> {
    PACKS.iter().filter(|p| p.language == language).collect()
}

/// The calls in `language` the catalogue has and no choice says: each with the packs that have it, by kind.
pub fn calls_to_get(choices: &[Choice], language: &str) -> Vec<(Ladder, Vec<&'static Pack>)> {
    let said: Vec<&Ladder> =
        choices.iter().filter(|c| c.language == language).filter_map(|c| c.ladder.as_ref()).collect();
    let mut out: Vec<(Ladder, Vec<&'static Pack>)> = Vec::new();
    for pack in packs_in(language) {
        for calls in pack.styles {
            let ladder = Ladder::parse(calls.grades, calls.said);
            if said.iter().any(|s| s.same_grades(&ladder)) {
                continue;
            }
            match out.iter_mut().find(|(l, _)| l.same_grades(&ladder)) {
                Some((_, packs)) if !packs.iter().any(|p| std::ptr::eq(*p, pack)) => packs.push(pack),
                Some(_) => {}
                None => out.push((ladder, vec![pack])),
            }
        }
    }
    out.sort_by_key(|(l, packs)| (l.kind(), std::cmp::Reverse(packs.len())));
    out
}

/// The people the catalogue has calling as `current` in its language and no choice has so: each with their pack.
pub fn voices_to_get(choices: &[Choice], current: &Choice) -> Vec<(&'static str, &'static Pack)> {
    let has = |person: &str| {
        choices.iter().any(|c| c.person == person && c.language == current.language && c.calls_as(current))
    };
    let mut out: Vec<(&'static str, &'static Pack)> = Vec::new();
    for pack in packs_in(&current.language) {
        if pack.styles.iter().any(|calls| current.calls_as_listed(calls)) {
            for voice in pack.voices {
                if !has(voice.person) && !out.iter().any(|(p, _)| *p == voice.person) {
                    out.push((voice.person, pack));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A plugin whose style names every ladder corner `<id>.ogg` with the text `<grade> Left` (`grades`, or none), and
    /// voice folders with the first `n` of them.
    fn plugin(dir: &Path, grades: Option<[&str; 8]>, folders: &[(&str, usize)]) {
        let calls: String =
            LADDER.iter().map(|id| format!("[PACENOTE::C{id}]\nid={id}\nSounds=1\nSnd0={id}.ogg\n")).collect();
        write(&dir.join("config/pacenotes/Rbr.ini"), &calls);
        write(&dir.join("config/ranges/Rbr.ini"), "[RANGE::100]\nSounds=1\nSnd0=100.ogg\n");
        write(&dir.join("PaceNote.ini"), "[SETTINGS]\nlanguage=english\n");
        if let Some(grades) = grades {
            let texts: String = LADDER.iter().zip(grades).map(|(id, g)| format!("C{id}={g} Left\n")).collect();
            write(&dir.join("language/english/pacenotes/strings.ini"), &format!("[STRINGS]\nLEFT=Left\n{texts}"));
        }
        for (folder, n) in folders {
            write(&dir.join("sounds").join(folder).join("100.ogg"), "OggS");
            for id in &LADDER[..*n] {
                write(&dir.join("sounds").join(folder).join(format!("{id}.ogg")), "OggS");
            }
        }
    }

    fn source<'a>(pack: Option<&'a str>, dir: &'a Path, voices: &'a [String]) -> Source<'a> {
        Source { pack, dir, voices, style: "Rbr.ini", ranges: "Rbr.ini" }
    }

    const NUMERIC_A: [&str; 8] = ["HP", "1", "2", "3", "4", "5", "6", "Flat"];
    const NUMERIC_B: [&str; 8] = ["HP", "1", "Square", "2", "3", "4", "5", "6"];

    #[test]
    fn folders_are_people_in_a_language_with_their_sources_calls() {
        let root = crate::test_dir("codriver-choices");
        let all = LADDER.len();
        let (rsf, a, b) = (root.join("rsf"), root.join("a"), root.join("b"));
        plugin(&rsf, None, &[("french/male/christophe", 5), ("Unknown", all), ("english", 0)]);
        // benofficial2's A and B: one folder name, told apart by their installs' texts.
        plugin(&a, Some(NUMERIC_A), &[("RitaFullonV3 (benofficial2)", all), ("Janne Laahanen EnNu A Simple NF", all)]);
        plugin(&b, Some(NUMERIC_B), &[("RitaFullonV3 (benofficial2)", all)]);
        let voices = [rsf.as_path(), &a, &b].map(codriver::voices);
        let choices = choices(&[
            source(None, &rsf, &voices[0]),
            source(Some("A"), &a, &voices[1]),
            source(Some("B"), &b, &voices[2]),
        ]);
        let people: Vec<(&str, &str)> = choices.iter().map(|c| (c.person.as_str(), c.language.as_str())).collect();
        // The catalogued people first, then the others; the folder with no corner is left out. An uncatalogued
        // folder takes its path's language, else its install's.
        assert_eq!(
            people,
            [
                ("Janne Laahanen", "English"),
                ("Rita Fullon", "English"),
                ("Rita Fullon", "English"),
                ("Unknown", "English"),
                ("french/male/christophe", "French"),
            ]
        );
        assert!(choices[0].simple && !choices[1].simple);
        assert_eq!(choices[1].ladder.as_ref().unwrap().text(), "Hairpin, 1, 2, 3, 4, 5, 6, Flat");
        assert_eq!(choices[2].ladder.as_ref().unwrap().said, "5 Left");
        assert!(choices[0].same_grades(&choices[1]) && !choices[0].calls_as(&choices[1]));
        assert!(choices[3].ladder.is_none() && !choices[4].complete());
        assert_eq!(languages(&choices), ["English", "French"]);
        // Rita's A and Janne's say the same grades: one entry, the most people first.
        let calls: Vec<&str> = calls_in(&choices, "English").iter().map(|c| c.folder.as_str()).collect();
        assert_eq!(calls, ["Janne Laahanen EnNu A Simple NF", "RitaFullonV3 (benofficial2)", "Unknown"]);
        // Picking: the same person's other calls; another language's closest.
        let b = closest(&choices, Some(&choices[1]), |c| c.ladder.as_ref().is_some_and(|l| l.grades[2] == "Square"));
        assert_eq!(b.unwrap().pack.as_deref(), Some("B"));
        let rita_a = closest(&choices, Some(&choices[2]), |c| c.same_grades(&choices[0]));
        assert_eq!(rita_a.unwrap().pack.as_deref(), Some("A"), "the same person over the first");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_ladder_leaves_out_the_direction_word_every_corner_says() {
        let calls = codriver::Calls {
            notes: LADDER
                .iter()
                .map(|&id| (id, codriver::Call { name: format!("C{id}"), ..Default::default() }))
                .collect(),
            ..Default::default()
        };
        let texts = [
            "Tornantino Sinistro",
            "Sinistra 1",
            "Sinistra 2",
            "Sinistra 3",
            "Sinistra 4",
            "Sinistra 5",
            "Sinistra 6",
            "Accenno Sinistro",
        ];
        let mut strings: HashMap<String, String> =
            LADDER.iter().zip(texts).map(|(id, t)| (format!("C{id}"), t.to_owned())).collect();
        let ladder_of = |strings: &HashMap<String, String>| ladder(&calls, strings);
        let found = ladder_of(&strings).unwrap();
        assert_eq!(
            (found.text().as_str(), found.said.as_str()),
            ("Tornantino, 1, 2, 3, 4, 5, 6, Accenno", "Sinistra 6")
        );
        assert!(found.direction_first());
        // A corner it does not say: no ladder.
        strings.remove("C26");
        assert_eq!(ladder_of(&strings), None);
    }

    #[test]
    fn ladders_are_numbers_words_or_both_in_any_language() {
        let kind = |grades: &str| Ladder::parse(grades, "").kind();
        assert_eq!(kind("HP, 1, 2, 3, 4, 5, 6, Flat"), Kind::Numbers);
        assert_eq!(kind("HP, 1, Square, 2, 3, 4, 5, 6"), Kind::Numbers);
        assert_eq!(kind("13, 11, 9, 8, 6, 4, 2, 1"), Kind::Numbers);
        assert_eq!(kind("Kehre, 1, 90, 3, 4, 5, 6, Voll"), Kind::Numbers);
        assert_eq!(kind("Épingle, un, deux, trois, quatre, cinq, six, à fond"), Kind::Numbers);
        assert_eq!(kind("gancho, um, reta, dois, três, quatro, cinco, seis"), Kind::Numbers);
        assert_eq!(kind("HP, Bad, Square, 3, 4, 5, 6, Flat"), Kind::Hybrid);
        assert_eq!(kind("Serpo, Hidas, 90, 8, 6, 4, 2, 1"), Kind::Hybrid);
        assert_eq!(kind("HP, Bad, Square, K, Medium, Fast, Easy, Flat"), Kind::Words);
        assert_eq!(kind("Serpo, Hidas, 90, Nopee, K, Eri, Helppo, Täys"), Kind::Words);
        // The order: by whether the call starts with its grade.
        assert!(!Ladder::parse("HP, 1, 2, 3, 4, 5, 6, Flat", "6 Left").direction_first());
        assert!(Ladder::parse("Épingle, un, deux, trois, quatre, cinq, six, à fond", "Gauche, six").direction_first());
        assert!(!Ladder::parse("HP, Bad, Square, K, Medium, Fast, Easy, Flat", "Easy Left").direction_first());
    }

    #[test]
    fn what_the_catalogue_has_and_no_source_does_is_offered_to_get() {
        let ladder = |grades: &[&str; 8], said: &str| Some(Ladder::parse(&grades.join(", "), said));
        let rita = |grades, said| Choice {
            pack: Some("Rita".into()),
            folder: "RitaFullonV3 (benofficial2)".into(),
            person: "Rita Fullon".into(),
            language: "English".into(),
            ladder: ladder(grades, said),
            simple: false,
            corners: LADDER.len(),
        };
        let choices = [rita(&NUMERIC_A, "6 Left")];
        assert!(languages_to_get(&choices).contains(&"French") && !languages_to_get(&choices).contains(&"English"));
        // English descriptive and Numeric B are Janne's (and benofficial2's) to get; Numeric A is here.
        let calls = calls_to_get(&choices, "English");
        let words = calls.iter().find(|(l, _)| l.kind() == Kind::Words).unwrap();
        assert_eq!(words.1.iter().map(|p| p.by).collect::<Vec<_>>(), ["Janne Laahanen"]);
        let listed = |grades: &[&str; 8]| Ladder::parse(&grades.join(", "), "");
        let b = calls.iter().find(|(l, _)| l.same_grades(&listed(&NUMERIC_B))).unwrap();
        assert!(b.1.len() >= 4, "{:?}", b.1.iter().map(|p| p.title).collect::<Vec<_>>());
        assert!(!calls.iter().any(|(l, _)| l.same_grades(&listed(&NUMERIC_A))));
        // Numeric A, number first, is said by others too; direction first only by Janne.
        let people: Vec<&str> = voices_to_get(&choices, &choices[0]).iter().map(|(p, _)| *p).collect();
        assert!(
            people.contains(&"Nicky Grist") && people.contains(&"Janne Laahanen") && !people.contains(&"Rita Fullon")
        );
        let first = rita(&NUMERIC_A, "Left 6");
        let people: Vec<&str> = voices_to_get(&choices, &first).iter().map(|(p, _)| *p).collect();
        assert_eq!(people, ["Janne Laahanen"]);
    }
}
