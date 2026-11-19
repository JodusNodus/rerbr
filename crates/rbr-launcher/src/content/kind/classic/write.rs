//! What an added classic stage's text files say: its `Tracks<n>.ini`, its `TrackSettings<n>.ini` (its own weathers,
//! else a retail stage's of the same surface), the number it gets and the folder it is kept in. Pure.

use rbr_formats::ini::{self, Section};
use rbr_formats::rsf::{WeatherRow, LETTERS};

/// The first number a stage gets when its own is taken: above every RallySimFans id (they reach 1301) and the
/// retail maps, below the BTB stages' synthetic numbers (`crate::btb::SYNTHETIC`, 1_000_000 up).
pub const FIRST: u32 = 10_000;

/// A stage's own number is kept only below this one (the BTB stages' range starts there).
pub const LIMIT: u32 = 1_000_000;

/// The number a stage added gets: its own when it is free, else the first free one from [`FIRST`].
pub fn pick_number(own: u32, taken: &dyn Fn(u32) -> bool) -> Option<u32> {
    let keep = (1..LIMIT).contains(&own) && !taken(own);
    keep.then_some(own).or_else(|| (FIRST..LIMIT).find(|&n| !taken(n)))
}

/// The bytes of a text file as characters, one for each byte (so what is read can be written back unchanged), without
/// a UTF-8 byte order mark.
pub fn latin1(bytes: &[u8]) -> String {
    bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes).iter().map(|&b| b as char).collect()
}

/// [`latin1`]'s inverse.
pub fn unlatin1(text: &str) -> Vec<u8> {
    text.chars().map(|c| c as u32 as u8).collect()
}

/// A text read as [`latin1`] for the player to read: UTF-8 when its bytes are.
pub fn shown(text: &str) -> String {
    String::from_utf8(unlatin1(text)).unwrap_or_else(|_| text.to_owned())
}

/// A value without its quotes and spaces.
pub fn unquoted(value: &str) -> &str {
    value.trim().trim_matches('"').trim()
}

/// A folder name from a stage's name: no separator, no character Windows refuses, not hidden.
pub fn folder_name(name: &str) -> String {
    let clean: String =
        name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).chars().take(80).collect::<String>();
    if clean.is_empty() {
        "stage".to_owned()
    } else {
        clean
    }
}

/// One section as text.
fn section_text(name: &str, entries: &[(String, String)]) -> String {
    let lines: String = entries.iter().map(|(key, value)| format!("{key} = {value}\r\n")).collect();
    format!("[{name}]\r\n{lines}")
}

/// What `Tracks<new>.ini` is made of: the stage's `[Map<old>]` entries, with the place of its files and what it
/// brings along, as the stage folder `folder` holds them.
pub struct Tracks<'a> {
    pub entries: &'a [(String, String)],
    pub new: u32,
    pub folder: &'a str,
    pub name: &'a str,
    /// `Particles`, when a `PS_` file of the download is in the folder.
    pub particles: Option<&'a str>,
    /// `SplashScreen`, when the splash is in the folder.
    pub splash: Option<&'a str>,
}

impl Tracks<'_> {
    pub fn text(&self) -> String {
        // Paths from the player's folder, as RSF's are from its root (`rsf::root_of`).
        let place = format!("{}\\{}\\", rbr_btb::player::TRACKS, self.folder);
        let mut entries: Vec<(String, String)> = vec![("TrackName".into(), format!("\"{place}track-{}\"", self.new))];
        let set = |entries: &mut Vec<(String, String)>, key: &str, value: Option<String>| match (
            entries.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)),
            value,
        ) {
            (Some(at), Some(v)) => entries[at].1 = v,
            (None, Some(v)) => entries.push((key.to_owned(), v)),
            _ => {}
        };
        entries.extend(self.entries.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("TrackName")).cloned());
        set(&mut entries, "StageName", (!has(self.entries, "StageName")).then(|| self.name.to_owned()));
        set(&mut entries, "Particles", self.particles.map(|p| format!("\"{place}{p}\"")));
        set(&mut entries, "SplashScreen", self.splash.map(|s| format!("\"{place}{s}\"")));
        section_text(&format!("Map{}", self.new), &entries)
    }
}

fn has(entries: &[(String, String)], key: &str) -> bool {
    entries.iter().any(|(k, v)| k.eq_ignore_ascii_case(key) && !unquoted(v).is_empty())
}

/// The sections of a settings file that are weathers of stage `old`, renamed to `new`.
pub fn own_weathers(sections: &[Section], old: u32, new: u32) -> Vec<Section> {
    sections
        .iter()
        .filter_map(|s| {
            WeatherRow::parse(old, &s.name)?;
            let name = format!("{new}{}", s.name.trim().strip_prefix(&old.to_string())?);
            Some(Section { name, entries: s.entries.clone() })
        })
        .collect()
}

/// The weathers of a retail stage of the same surface kind (`catalog::surface_kind`: 0 gravel, 1 tarmac, 2 snow), for
/// each of the stage's `letters`: the retail stage with most rows of that letter, its sections renamed to `new`.
pub fn borrowed_weathers(retail: &[Section], kind: u32, letters: &[char], new: u32) -> Vec<Section> {
    let rows: Vec<(u32, WeatherRow, &Section)> = retail
        .iter()
        .filter_map(|s| {
            let id: u32 = s.name.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
            Some((id, WeatherRow::parse(id, &s.name)?, s))
        })
        .filter(|(id, ..)| crate::catalog::surface_kind(*id) == kind)
        .collect();
    let mut out = Vec::new();
    for &letter in letters {
        let count = |id: u32| rows.iter().filter(|(i, w, _)| *i == id && w.letter_char() == letter).count();
        let best = rows.iter().map(|(id, ..)| *id).max_by_key(|&id| (count(id), std::cmp::Reverse(id)));
        for (id, w, s) in &rows {
            if Some(*id) == best && w.letter_char() == letter {
                if let Some(name) = WeatherRow::section_name(new, w.letter, w.cloud_letter, w.preset, w.cloud) {
                    out.push(Section { name, entries: s.entries.clone() });
                }
            }
        }
    }
    out
}

/// `TrackSettings<new>.ini`'s text.
pub fn settings_text(sections: &[Section]) -> String {
    sections.iter().map(|s| section_text(&s.name, &s.entries) + "\r\n").collect()
}

/// The retail `Maps/TrackSettings.ini`'s sections.
pub fn parse_sections(bytes: &[u8]) -> Vec<Section> {
    ini::parse(&latin1(bytes)).unwrap_or_default()
}

/// The surface kind of a `Tracks.ini` `Surface` value (0 tarmac, 1 gravel, 2 snow in `Tracks<id>.ini`), as
/// `catalog::surface_kind` numbers them: gravel when it names none.
pub fn surface_kind(value: Option<&str>) -> u32 {
    match value.map(unquoted) {
        Some("0") => 1,
        Some("2") => 2,
        _ => 0,
    }
}

/// The letters in [`LETTERS`]' order that `letters` holds.
pub fn letters_in_order(letters: &[char]) -> Vec<char> {
    LETTERS.into_iter().filter(|l| letters.contains(l)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn a_number_is_kept_when_free_and_else_taken_from_a_range_above_the_game_s() {
        let taken = |n: u32| [8994, 10_000, 10_001].contains(&n);
        assert_eq!(pick_number(555, &taken), Some(555));
        assert_eq!(pick_number(8994, &taken), Some(10_002));
        assert_eq!(pick_number(0, &taken), Some(10_002));
        assert_eq!(pick_number(2_000_000, &taken), Some(10_002));
        assert_eq!(pick_number(5, &|_| true), None);
    }

    #[test]
    fn tracks_ini_names_the_new_place() {
        let own = entries(&[
            ("TrackName", "\"Maps\\8994-Old\\track-8994\""),
            ("Particles", "\"Maps\\ps_british\""),
            ("StageName", "Kamenak"),
            ("Surface", "0"),
            ("SplashScreen", "\"Textures\\Splash\\8994-Old.dds\""),
        ]);
        let text = Tracks {
            entries: &own,
            new: 10_000,
            folder: "10000-Kamenak",
            name: "Kamenak",
            particles: None,
            splash: Some("s.dds"),
        }
        .text();
        assert_eq!(
            text,
            "[Map10000]\r\nTrackName = \"tracks\\10000-Kamenak\\track-10000\"\r\nParticles = \"Maps\\ps_british\"\r\n\
             StageName = Kamenak\r\nSurface = 0\r\nSplashScreen = \"tracks\\10000-Kamenak\\s.dds\"\r\n"
        );
        let bare =
            Tracks { entries: &[], new: 7, folder: "7-X", name: "X", particles: Some("PS_X"), splash: None }.text();
        assert_eq!(
            bare,
            "[Map7]\r\nTrackName = \"tracks\\7-X\\track-7\"\r\nStageName = X\r\nParticles = \"tracks\\7-X\\PS_X\"\r\n"
        );
    }

    #[test]
    fn weathers_are_the_stages_own_or_a_retail_stages_of_the_surface() {
        let sections = |names: &[&str]| -> Vec<Section> {
            names.iter().map(|n| Section { name: n.to_string(), entries: entries(&[("Fog", "1")]) }).collect()
        };
        let own =
            own_weathers(&sections(&["8994O_M_crisp_clear", "8995O_M_crisp_clear", "Evening_Clear"]), 8994, 10_000);
        assert_eq!(own.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["10000O_M_crisp_clear"]);
        let retail = sections(&[
            "54E_crisp_clear",
            "54M_crisp_clear",
            "54O_E_heavyrain_heavycloud",
            "41M_crisp_clear",
            "41M_hazy_clear",
            "41O_M_lightfog_heavycloud",
            "11O_N_hazy_partcloud",
        ]);
        let names = |kind, letters: &[char]| {
            borrowed_weathers(&retail, kind, letters, 10_001).into_iter().map(|s| s.name).collect::<Vec<_>>()
        };
        assert_eq!(names(0, &['E', 'O']), ["10001E_crisp_clear", "10001O_E_heavyrain_heavycloud"]);
        assert_eq!(names(1, &['M']), ["10001M_crisp_clear", "10001M_hazy_clear"]);
        assert_eq!(names(2, &['O']), ["10001O_N_hazy_partcloud"]);
        assert!(names(0, &['S']).is_empty());
        assert_eq!(settings_text(&own), "[10000O_M_crisp_clear]\r\nFog = 1\r\n\r\n");
        assert_eq!(surface_kind(Some("0")), 1);
        assert_eq!(surface_kind(Some("\"1\"")), 0);
        assert_eq!(surface_kind(None), 0);
        assert_eq!(surface_kind(Some("2")), 2);
    }

    #[test]
    fn text_survives_a_round_trip_and_folder_names_are_safe() {
        let bytes = b"Author=Miro Ku\xe8ek\r\n";
        assert_eq!(unlatin1(&latin1(bytes)), bytes);
        assert_eq!(shown(&latin1("Kuřek".as_bytes())), "Kuřek");
        assert_eq!(folder_name("../a:b?"), "-a-b-");
        assert_eq!(folder_name("..."), "stage");
        assert_eq!(folder_name("Autiovaara II"), "Autiovaara II");
    }
}
