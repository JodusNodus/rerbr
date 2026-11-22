//! The voice folders of every source (the RallySimFans install's plugin, each owned pack) as the people who speak
//! them, for the Co-drivers page's Co-driver and Calls rows. A V3 pack installs one call style each (its own
//! corners file and texts), so a person's styles are their folders across the sources: a catalogued folder
//! ([`catalog`]) joins its person's others, any other folder is a speaker of its own. A style is named by its
//! folder's code ([`catalog::scale`]), else by how its install says the corners (the editor's texts), else by the
//! folder. Each folder is checked for the corners the preview says (`rbr_sound::preview::LADDER`) in its source's
//! style: one missing some is incomplete, one with none is left out. Read at rescan, never per draw.

use std::collections::HashMap;
use std::path::Path;

use rbr_formats::pacenote::codriver::{self, Calls};
use rbr_formats::stage_files;
use rbr_sound::preview::LADDER;

use super::catalog;

/// A voice source: an owned pack (none: the RallySimFans install), its plugin folder and voice folders, and the
/// style and range set it plays.
pub struct Source<'a> {
    pub pack: Option<&'a str>,
    pub dir: &'a Path,
    pub voices: &'a [String],
    pub style: &'a str,
    pub ranges: &'a str,
}

/// A person and their voice folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    pub name: String,
    pub language: Option<&'static str>,
    pub styles: Vec<Style>,
}

/// One voice folder of a speaker: its source and folder, how it calls, and how many of the preview's corners it
/// has a recording for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub pack: Option<String>,
    pub folder: String,
    pub label: String,
    /// The corners as its install says them, tightest to fastest (`HP, 1, 2, 3, 4, 5, 6, Flat ("6 Left")`), when it says.
    pub ladder: Option<String>,
    pub corners: usize,
}

impl Style {
    pub fn complete(&self) -> bool {
        self.corners == LADDER.len()
    }

    /// The label, with the corners it has when it lacks some.
    pub fn text(&self) -> String {
        match self.complete() {
            true => self.label.clone(),
            false => format!("{} · {} of {} corners", self.label, self.corners, LADDER.len()),
        }
    }

    /// Whether it is the voice folder `folder` of source `pack`.
    pub fn is(&self, pack: Option<&str>, folder: &str) -> bool {
        self.pack.as_deref() == pack && self.folder == folder
    }
}

impl Speaker {
    /// Whether one of the speaker's folders says every corner.
    pub fn complete(&self) -> bool {
        self.styles.iter().any(Style::complete)
    }

    /// The name with the language.
    pub fn label(&self) -> String {
        match self.language {
            Some(language) => format!("{} · {language}", self.name),
            None => self.name.clone(),
        }
    }

    /// The style of folder `folder` in source `pack`, if it is the speaker's.
    pub fn style(&self, pack: Option<&str>, folder: &str) -> Option<&Style> {
        self.styles.iter().find(|s| s.is(pack, folder))
    }

    /// The style to call with when the speaker is picked over calls labelled `label`: the same calls when the
    /// speaker has them complete, else its first complete style, else its first.
    pub fn style_for(&self, label: Option<&str>) -> &Style {
        let same = self.styles.iter().find(|s| Some(s.label.as_str()) == label && s.complete());
        same.or_else(|| self.styles.iter().find(|s| s.complete())).unwrap_or(&self.styles[0])
    }
}

/// How the install with `calls` and texts `strings` says the preview's corners, tightest to fastest, with the
/// direction word left out (`HP, 1, 2, 3, 4, 5, 6, Flat`) and one call whole for the order (`"6 Left"`); none when it does
/// not say them all. The direction word is the one every corner says (its first five letters, so Italian's
/// `Sinistra` and `Sinistro` are one), else `LEFT`'s text.
fn ladder(calls: &Calls, strings: &HashMap<String, String>) -> Option<String> {
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
    let grades: Vec<String> =
        said.iter().map(|t| words(t).into_iter().filter(|w| stem(w) != left).collect::<Vec<_>>().join(" ")).collect();
    if grades.iter().any(String::is_empty) {
        return None;
    }
    Some(format!("{} (\"{}\")", grades.join(", "), said[said.len() - 2]))
}

/// The speakers of the `sources`: the catalogued people first in the catalogue's order, their styles by label, then
/// the other folders by name.
pub fn speakers(sources: &[Source]) -> Vec<Speaker> {
    let mut found = Vec::new();
    for source in sources {
        let calls = codriver::load(source.dir, source.style, source.ranges).ok();
        let strings = codriver::strings(source.dir);
        let ladder = calls.as_ref().and_then(|c| ladder(c, &strings));
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
            let entry = catalog::find(folder);
            let label =
                entry.and_then(|(_, v)| v.scale(folder)).or_else(|| ladder.clone()).unwrap_or_else(|| folder.clone());
            let style = Style {
                pack: source.pack.map(str::to_owned),
                folder: folder.clone(),
                label,
                ladder: ladder.clone(),
                corners,
            };
            found.push((catalog::rank(folder), entry, style));
        }
    }
    found.sort_by(|a, b| (a.0.is_none(), a.0, &a.2.label).cmp(&(b.0.is_none(), b.0, &b.2.label)));
    let mut out: Vec<Speaker> = Vec::new();
    for (_, entry, style) in found {
        match entry {
            Some((pack, voice)) => {
                match out.iter_mut().find(|s| s.name == voice.person && s.language == Some(pack.language)) {
                    Some(speaker) => speaker.styles.push(style),
                    None => out.push(Speaker {
                        name: voice.person.to_owned(),
                        language: Some(pack.language),
                        styles: vec![style],
                    }),
                }
            }
            None => out.push(Speaker { name: style.folder.clone(), language: None, styles: vec![style] }),
        }
    }
    // Two styles alike in one speaker (the same download added twice, or A and B under one texts): told by source.
    for speaker in &mut out {
        let labels: Vec<String> = speaker.styles.iter().map(|s| s.label.clone()).collect();
        for style in &mut speaker.styles {
            if labels.iter().filter(|l| **l == style.label).count() > 1 {
                style.label = format!("{} ({})", style.label, super::source_name(style.pack.as_deref()));
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

    #[test]
    fn folders_group_by_person_across_sources_and_are_checked_for_the_corners() {
        let root = crate::test_dir("codriver-speakers");
        let all = LADDER.len();
        let (rsf, a, b) = (root.join("rsf"), root.join("a"), root.join("b"));
        plugin(&rsf, None, &[("english/male/steve", 5), ("Janne Laahanen EnNu A", all), ("english", 0)]);
        // benofficial2's A and B: one folder name, told apart by their installs' texts.
        let a_grades = ["HP", "1", "2", "3", "4", "5", "6", "Flat"];
        plugin(&a, Some(a_grades), &[("RitaFullonV3 (benofficial2)", all), ("Janne Laahanen EnNu B NF", all)]);
        plugin(&b, Some(["HP", "1", "Square", "2", "3", "4", "5", "6"]), &[("RitaFullonV3 (benofficial2)", all)]);
        let voices = [rsf.as_path(), &a, &b].map(codriver::voices);
        let speakers = speakers(&[
            source(None, &rsf, &voices[0]),
            source(Some("A"), &a, &voices[1]),
            source(Some("B"), &b, &voices[2]),
        ]);
        let labels: Vec<String> = speakers.iter().map(Speaker::label).collect();
        // The catalogued people first, then the others; the folder with no corner is left out.
        assert_eq!(labels, ["Janne Laahanen · English", "Rita Fullon · English", "english/male/steve"]);
        let janne: Vec<&str> = speakers[0].styles.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(janne, ["Numeric A", "Numeric B, number first"]);
        assert_eq!(speakers[0].style(Some("A"), "Janne Laahanen EnNu B NF").unwrap().label, "Numeric B, number first");
        let rita: Vec<&str> = speakers[1].styles.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(rita, ["HP, 1, 2, 3, 4, 5, 6, Flat (\"6 Left\")", "HP, 1, Square, 2, 3, 4, 5, 6 (\"5 Left\")"]);
        assert!(!speakers[2].complete());
        assert_eq!(speakers[2].styles[0].text(), format!("english/male/steve · 5 of {all} corners"));
        // Picked over Numeric B, Janne keeps it; Rita has no such calls and takes her first.
        assert_eq!(speakers[0].style_for(Some("Numeric B, number first")).folder, "Janne Laahanen EnNu B NF");
        assert_eq!(speakers[1].style_for(Some("Numeric B, number first")).pack.as_deref(), Some("A"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_ladder_leaves_out_the_direction_word_every_corner_says() {
        let calls = Calls {
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
        assert_eq!(ladder(&calls, &strings).unwrap(), "Tornantino, 1, 2, 3, 4, 5, 6, Accenno (\"Sinistra 6\")");
        // A corner it does not say: no ladder.
        strings.remove("C26");
        assert_eq!(ladder(&calls, &strings), None);
    }

    #[test]
    fn alike_styles_are_told_by_their_source() {
        let root = crate::test_dir("codriver-speakers-alike");
        let (a, b) = (root.join("a"), root.join("b"));
        for dir in [&a, &b] {
            plugin(dir, None, &[("Darren V3", LADDER.len())]);
        }
        let voices = [a.as_path(), &b].map(codriver::voices);
        let speakers = speakers(&[source(Some("one"), &a, &voices[0]), source(Some("two"), &b, &voices[1])]);
        let labels: Vec<&str> = speakers[0].styles.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["Darren V3 (one)", "Darren V3 (two)"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
