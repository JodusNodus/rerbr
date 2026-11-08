//! Reader for the game's `.ini` files, as used by the car folders under `Cars/`.
//!
//! The retail files are ASCII with CRLF line endings. A `;` at the start of a line is a comment.
//! A `[Name]` line opens a section, and `key = value` lines follow it. Keys may contain spaces
//! (`Car Part`, `Fade In Time`) and values may be separated by tabs. Inline comments do not occur.
//! Values are returned trimmed, with quotes kept; callers that expect quoted paths strip them.
//! See `notes/formats.md` "Cars".
//!
//! The game reads them with `GetPrivateProfileStringA` (`NLBLib2::NFileCore::ReadIniString`, `src/nlblib2/NFileUtils.cpp`),
//! which rejects no line: it only finds `key = value` lines by key in a named section. So lines no lookup can find are
//! skipped here: lines without `=` (`____`, a `[` without `]`), lines with an empty key (`=== Extralights ===`), and
//! lines before the first section. RallySimFans car folders have all of these.

use std::fmt::{self, Display, Formatter};

/// One `[Name]` section with its `key = value` entries in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl Section {
    /// Value of the first entry whose key matches `key` ignoring ASCII case.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

/// Parse failure with a 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: &'static str,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parses a whole `.ini` document, skipping the lines a lookup cannot find (module comment). It does not fail; the
/// `Result` stays for the callers.
pub fn parse(text: &str) -> Result<Vec<Section>, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut sections: Vec<Section> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        // Text after the `]` is ignored (RallySimFans's `TrackSettings<id>.ini` puts a `;` comment there).
        if let Some((name, _)) = line.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            sections.push(Section { name: name.trim().to_owned(), entries: Vec::new() });
            continue;
        }
        let (Some((key, value)), Some(section)) = (line.split_once('='), sections.last_mut()) else {
            continue;
        };
        let key = key.trim();
        if !key.is_empty() {
            section.entries.push((key.to_owned(), value.trim().to_owned()));
        }
    }
    Ok(sections)
}

/// `file` (a whole `.ini` document) with its `[name]` section left out and `section` (a section's text) appended, the
/// rest kept line for line.
pub fn replace_section(file: &str, name: &str, section: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in file.lines() {
        if let Some((header, _)) = line.trim().strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            inside = header.trim() == name;
        }
        if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(section);
    out
}

/// `file` with `key = value` in its `[name]` section: the key's first line replaced, else the line added at the
/// section's end, else the section appended; the rest kept line for line.
pub fn set_value(file: &str, name: &str, key: &str, value: &str) -> String {
    let line_of = |k: &str| format!("{k} = {value}\n");
    let mut out = String::new();
    let (mut inside, mut done) = (false, false);
    for line in file.lines() {
        if let Some((header, _)) = line.trim().strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            if inside && !done {
                insert_before_blank(&mut out, &line_of(key));
                done = true;
            }
            inside = header.trim() == name;
        } else if inside && !done {
            if let Some((k, _)) = line.split_once('=').filter(|(k, _)| k.trim().eq_ignore_ascii_case(key)) {
                out.push_str(&line_of(k.trim()));
                done = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if inside && !done {
        insert_before_blank(&mut out, &line_of(key));
    } else if !done {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&format!("[{name}]\n{}", line_of(key)));
    }
    out
}

/// Adds `line` to `out` before the blank lines it ends with.
fn insert_before_blank(out: &mut String, line: &str) {
    let kept = out.trim_end_matches('\n').len() + 1;
    let at = kept.min(out.len());
    out.insert_str(at, line);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, value: &str) -> (String, String) {
        (key.to_owned(), value.to_owned())
    }

    #[test]
    fn sections_comments_and_spaced_keys() {
        let text = "; header\r\n\r\n[Light0]\r\nId \t\t\t= 16\r\nCar Part\t\t= Point_light_lb\r\n\
                    ; inside\r\n[Light1]\r\nLens Flare Color\t= 1.0 0.0 0.0 1.0\r\n";
        let sections = parse(text).unwrap();
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name, "Light0");
        assert_eq!(sections[0].entries, vec![entry("Id", "16"), entry("Car Part", "Point_light_lb")]);
        assert_eq!(sections[1].get("lens flare color"), Some("1.0 0.0 0.0 1.0"));
    }

    #[test]
    fn value_keeps_later_equals_and_quotes() {
        let sections = parse("[Car00]\nFileName = \"Cars\\xsara\\x=y.sgc\"\n").unwrap();
        assert_eq!(sections[0].get("FileName"), Some("\"Cars\\xsara\\x=y.sgc\""));
    }

    #[test]
    fn duplicate_keys_kept_in_order_first_wins_on_get() {
        let sections = parse("[S]\nA = 1\na = 2\n").unwrap();
        assert_eq!(sections[0].entries, vec![entry("A", "1"), entry("a", "2")]);
        assert_eq!(sections[0].get("A"), Some("1"));
        assert_eq!(sections[0].get("missing"), None);
    }

    #[test]
    fn byte_order_mark_and_empty_document() {
        assert_eq!(parse("\u{feff}[S]\nK=V").unwrap()[0].get("K"), Some("V"));
        assert!(parse("").unwrap().is_empty());
        assert!(parse("; only a comment\n").unwrap().is_empty());
    }

    #[test]
    fn lines_no_lookup_can_find_are_skipped() {
        let text = "K = V\n[S]\n=== Extralights ===\n____\n[\nA = 1\n[T] ; note\nB=2\n";
        let sections = parse(text).unwrap();
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].entries, vec![entry("A", "1")]);
        assert_eq!(sections[1].name, "T");
        assert_eq!(sections[1].get("B"), Some("2"));
    }

    #[test]
    fn a_value_is_set_in_its_section() {
        let file = "[ffb]\nenabled = 1\nStrength = 1\n\n[camera]\nview = chase\n";
        assert_eq!(
            set_value(file, "ffb", "strength", "0.5"),
            "[ffb]\nenabled = 1\nStrength = 0.5\n\n[camera]\nview = chase\n"
        );
        assert_eq!(
            set_value(file, "camera", "head", "off"),
            "[ffb]\nenabled = 1\nStrength = 1\n\n[camera]\nview = chase\nhead = off\n"
        );
        assert_eq!(set_value("[ffb]\nrate = 9\n\n[x]\n", "ffb", "auto", "0"), "[ffb]\nrate = 9\nauto = 0\n\n[x]\n");
        assert_eq!(set_value("[x]\na = 1\n", "ffb", "strength", "0.5"), "[x]\na = 1\n\n[ffb]\nstrength = 0.5\n");
        assert_eq!(set_value("", "ffb", "strength", "0.5"), "[ffb]\nstrength = 0.5\n");
    }
}
