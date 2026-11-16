//! The installer's script, read from its inflated header without running anything: the strings with their variable
//! codes, the entries (an instruction each) and a walk over them that follows `SetOutPath`, remembers `StrCpy`'s
//! literals (control flow is ignored: the last assignment before a use wins) and reports the files, the INI writes
//! and the `nsis7z` extractions. Entry opcodes 11 (`SetOutPath`), 20 (`File`), 25 (`StrCpy`), 31 (`Push`/`Pop`), 44
//! (`CallInstDLL`) and 48 (`WriteINIStr`) are the same in NSIS 2 and 3, ANSI and Unicode; the strings differ.

use std::collections::HashMap;

use super::{Fail, IniWrite};
use crate::archive::safe_path;

const ENTRY_SIZE: usize = 28;
const SET_OUT_PATH: u32 = 11;
const FILE: u32 = 20;
const STR_CPY: u32 = 25;
const PUSH_POP: u32 = 31;
const CALL_DLL: u32 = 44;
const WRITE_INI: u32 = 48;

/// How the strings table spells a variable, a shell folder, a language string or an escaped character: a code
/// followed by a value of two bytes (ANSI) or one 16-bit unit (Unicode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// NSIS 2: codes 252 to 255 (skip, variable, shell, language), two bytes of value.
    Ansi2,
    /// NSIS 3 ANSI: codes 1 to 4 (language, shell, variable, skip), two bytes of value.
    Ansi3,
    /// NSIS 3 Unicode: codes 1 to 4, one unit of value.
    Unicode3,
    /// Unicode NSIS 2 builds: codes 0xE000 to 0xE003 (skip, variable, shell, language), one unit of value.
    Unicode2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Char(char),
    Var(u32),
    Shell(u32),
    Lang(u32),
}

/// A 14-bit value stored as two 7-bit halves, each with its top bit set.
fn value(lo: u32, hi: u32) -> u32 {
    (lo & 0x7F) | ((hi & 0x7F) << 7)
}

/// The block (offset, count) pairs follow the flags: pages, sections, entries, strings, language tables, ...
fn block(header: &[u8], n: usize) -> Option<(usize, usize)> {
    let at = 4 + 8 * n;
    let get = |at: usize| Some(u32::from_le_bytes(header.get(at..at + 4)?.try_into().ok()?) as usize);
    Some((get(at)?, get(at + 4)?))
}

pub struct Header<'a> {
    data: &'a [u8],
    entries: usize,
    count: usize,
    strings: &'a [u8],
    style: Style,
}

/// The style the strings table is written in: Unicode when most of its odd bytes are zero, and the code pattern
/// (a code then two high-bit bytes) that occurs more.
fn guess(strings: &[u8]) -> Style {
    let odd = strings.iter().skip(1).step_by(2);
    let (zeros, all) = (odd.clone().filter(|b| **b == 0).count(), odd.count());
    if all > 0 && zeros * 2 >= all {
        let units: Vec<u16> = strings.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let count = |code: std::ops::RangeInclusive<u16>| {
            units.windows(2).filter(|w| code.contains(&w[0]) && w[1] & 0x8080 == 0x8080).count()
        };
        if count(0xE000..=0xE003) > count(1..=4) {
            Style::Unicode2
        } else {
            Style::Unicode3
        }
    } else {
        let count = |code: std::ops::RangeInclusive<u8>| {
            strings.windows(3).filter(|w| code.contains(&w[0]) && w[1] & 0x80 != 0 && w[2] & 0x80 != 0).count()
        };
        if count(252..=255) > count(1..=4) {
            Style::Ansi2
        } else {
            Style::Ansi3
        }
    }
}

impl<'a> Header<'a> {
    pub fn new(data: &'a [u8]) -> Result<Header<'a>, Fail> {
        let bad = || format!("{}: its script header is malformed", super::container::UNSUPPORTED);
        let (entries, count) = block(data, 2).ok_or_else(bad)?;
        let (strings, next) = (block(data, 3).ok_or_else(bad)?.0, block(data, 4).ok_or_else(bad)?.0);
        let size = count.checked_mul(ENTRY_SIZE).and_then(|n| n.checked_add(entries)).ok_or_else(bad)?;
        if size > data.len() || strings > next || next > data.len() {
            return Err(bad());
        }
        let strings = &data[strings..next];
        Ok(Header { data, entries, count, strings, style: guess(strings) })
    }

    /// The `n`th entry: its opcode and six parameters.
    fn entry(&self, n: usize) -> (u32, [u32; 6]) {
        let at = self.entries + n * ENTRY_SIZE;
        let word = |k: usize| u32::from_le_bytes(self.data[at + 4 * k..at + 4 * k + 4].try_into().unwrap_or_default());
        (word(0), std::array::from_fn(|k| word(k + 1)))
    }

    /// The string at `index` as tokens; `None` when it lies outside the table.
    fn tokens(&self, index: u32) -> Option<Vec<Tok>> {
        if index >= 0x8000_0000 {
            return Some(vec![Tok::Lang(!index)]);
        }
        let unicode = matches!(self.style, Style::Unicode2 | Style::Unicode3);
        let at = index as usize * if unicode { 2 } else { 1 };
        let rest = self.strings.get(at..)?;
        let units: Vec<u32> = if unicode {
            rest.chunks_exact(2).map(|c| u32::from(u16::from_le_bytes([c[0], c[1]]))).collect()
        } else {
            rest.iter().map(|b| u32::from(*b)).collect()
        };
        let (skip, var, shell, lang) = match self.style {
            Style::Ansi2 => (252, 253, 254, 255),
            Style::Ansi3 | Style::Unicode3 => (4, 3, 2, 1),
            Style::Unicode2 => (0xE000, 0xE001, 0xE002, 0xE003),
        };
        let mut out = Vec::new();
        let mut i = 0;
        let mut text = Vec::new();
        let flush = |text: &mut Vec<u32>, out: &mut Vec<Tok>| {
            if unicode {
                let units: Vec<u16> = text.drain(..).map(|u| u as u16).collect();
                out.extend(char::decode_utf16(units).map(|c| Tok::Char(c.unwrap_or('\u{FFFD}'))));
            } else {
                out.extend(text.drain(..).map(|b| Tok::Char(char::from(b as u8))));
            }
        };
        loop {
            let c = *units.get(i)?;
            i += 1;
            if c == 0 {
                break;
            }
            if c == skip {
                // The escaped unit itself follows.
                text.push(*units.get(i)?);
                i += 1;
            } else if [var, shell, lang].contains(&c) {
                let width = if unicode { 1 } else { 2 };
                let v = units.get(i..i + width)?;
                i += width;
                let v = if unicode { value(v[0], v[0] >> 8) } else { value(v[0], v[1]) };
                flush(&mut text, &mut out);
                out.push(match c {
                    _ if c == var => Tok::Var(v),
                    _ if c == shell => Tok::Shell(v),
                    _ => Tok::Lang(v),
                });
            } else {
                text.push(c);
            }
        }
        flush(&mut text, &mut out);
        Some(out)
    }
}

/// The names NSIS gives the variables below 32.
fn var_name(n: u32) -> String {
    const NAMES: [&str; 12] = [
        "CMDLINE",
        "INSTDIR",
        "OUTDIR",
        "EXEDIR",
        "LANGUAGE",
        "TEMP",
        "PLUGINSDIR",
        "EXEPATH",
        "EXEFILE",
        "HWNDPARENT",
        "_CLICK",
        "_OUTDIR",
    ];
    match n {
        0..=9 => n.to_string(),
        10..=19 => format!("R{}", n - 10),
        20..=31 => NAMES[n as usize - 20].to_owned(),
        _ => format!("var{n}"),
    }
}

const INSTDIR: u32 = 21;
const OUTDIR: u32 = 22;

/// A file the script extracts: its place below the install folder and where its data is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOp {
    pub path: Vec<String>,
    pub offset: u32,
}

/// An `nsis7z` extraction: the archive's place below the install folder (as the script names it) and the folder it
/// unpacks into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extract {
    pub archive: String,
    pub into: Vec<String>,
}

#[derive(Default)]
pub struct Script {
    pub files: Vec<FileOp>,
    pub ini: Vec<IniWrite>,
    pub extracts: Vec<Extract>,
}

/// The walk's state.
struct Walk<'a> {
    header: &'a Header<'a>,
    vars: HashMap<u32, String>,
    /// The variable the script keeps the game's folder in (see [`root_var`]): the install root, like `$INSTDIR`.
    root: Option<u32>,
    /// `$OUTDIR` as a rooted path below the install folder (`""` is the folder itself).
    out: String,
    stack: Vec<(String, bool)>,
}

impl Walk<'_> {
    /// Whether `var` is the install root.
    fn is_root(&self, var: u32) -> bool {
        var == INSTDIR || Some(var) == self.root
    }

    /// The string at `index` with the variables it names replaced by what is known of them; the rest as `${name}`.
    /// The install root is nothing. Also whether the string starts at the root (so is not relative to `$OUTDIR`).
    fn text(&self, index: u32) -> (String, bool) {
        let Some(tokens) = self.header.tokens(index) else { return (String::new(), false) };
        let rooted = matches!(tokens.first(), Some(Tok::Var(v)) if self.is_root(*v) || *v == OUTDIR);
        let from_var = !matches!(tokens.first(), Some(Tok::Char(_)) | None);
        let text = tokens
            .into_iter()
            .map(|t| match t {
                Tok::Char(c) => c.to_string(),
                Tok::Var(v) if self.is_root(v) => String::new(),
                Tok::Var(OUTDIR) => self.out.clone(),
                Tok::Var(n) => self.vars.get(&n).cloned().unwrap_or_else(|| format!("${{{}}}", var_name(n))),
                Tok::Shell(n) => format!("${{shell{n}}}"),
                Tok::Lang(n) => format!("${{lang{n}}}"),
            })
            .collect::<String>()
            .replace('/', "\\");
        // A path from a variable the walk can't resolve (`$PLUGINSDIR`, a shell folder) is not below `$OUTDIR` either.
        let rooted = rooted || text.starts_with('\\') || from_var && text.starts_with("${");
        (text, rooted)
    }

    /// The string at `index` as a rooted path below the install folder, `\Maps\a.txt`.
    fn path(&self, index: u32) -> String {
        let (text, rooted) = self.text(index);
        self.place(&text, rooted)
    }

    /// `text` as a rooted path: as it is when `rooted`, else below `$OUTDIR`.
    fn place(&self, text: &str, rooted: bool) -> String {
        let base = if rooted { "" } else { &self.out };
        format!("{base}\\{}", text.trim_start_matches('\\'))
    }
}

/// The variable the installer keeps the game's folder in, if it has one: most scripts (the stage installers') ask
/// the registry for RBR's folder into a variable and write every file below it. It is the variable most paths of
/// `SetOutPath`, `File` and `WriteINIStr` start with (followed by `\`), apart from the built-in ones that name a folder of their own
/// (`$TEMP`, `$PLUGINSDIR`, `$EXEDIR`, ...).
fn root_var(header: &Header) -> Option<u32> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for n in 0..header.count {
        let (op, p) = header.entry(n);
        let path = match op {
            SET_OUT_PATH if p[1] != 0 => p[0],
            FILE => p[1],
            WRITE_INI => p[3],
            _ => continue,
        };
        if let Some([Tok::Var(v), Tok::Char('\\'), ..]) = header.tokens(path).as_deref() {
            if !(20..30).contains(v) {
                *counts.entry(*v).or_default() += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|(v, n)| (*n, std::cmp::Reverse(*v))).map(|(v, _)| v)
}

fn parts(path: &str) -> Option<Vec<String>> {
    safe_path(path).map(|p| p.into_iter().map(str::to_owned).collect())
}

/// Whether the path starts from something the walk could not place: a variable it never saw assigned.
fn unplaced(path: &[String]) -> bool {
    path.first().is_some_and(|p| p.starts_with("${"))
}

/// `a\b` for comparing paths the script spells in different cases or with slashes of either kind.
fn same(a: &str, b: &str) -> bool {
    let key = |s: &str| s.replace('/', "\\").to_ascii_lowercase().trim_matches('\\').to_owned();
    key(a) == key(b)
}

pub fn walk(header: &Header) -> Script {
    let mut walk = Walk { header, vars: HashMap::new(), root: root_var(header), out: String::new(), stack: Vec::new() };
    let mut script = Script::default();
    for n in 0..header.count {
        let (op, p) = header.entry(n);
        match op {
            SET_OUT_PATH if p[1] != 0 => walk.out = walk.path(p[0]).trim_end_matches('\\').to_owned(),
            FILE => {
                if let Some(path) = parts(&walk.path(p[1])).filter(|path| !unplaced(path)) {
                    script.files.push(FileOp { path, offset: p[2] });
                }
            }
            STR_CPY => {
                // With a length or an offset the value is cut, which is not followed.
                if walk.is_root(p[0]) || p[0] == OUTDIR || p[2] != 0 || p[3] != 0 {
                    walk.vars.remove(&p[0]);
                } else {
                    let (text, _) = walk.text(p[1]);
                    walk.vars.insert(p[0], text);
                }
            }
            PUSH_POP => match p[1] {
                0 => {
                    let text = walk.text(p[0]);
                    walk.stack.push(text);
                }
                1 => {
                    let (top, _) = walk.stack.pop().unwrap_or_default();
                    walk.vars.insert(p[0], top);
                }
                _ => {
                    let len = walk.stack.len();
                    if len >= 2 {
                        walk.stack.swap(len - 1, len - 2);
                    }
                }
            },
            CALL_DLL => {
                let (dll, function) = (walk.text(p[0]).0.to_ascii_lowercase(), walk.text(p[1]).0.to_ascii_lowercase());
                if dll.contains("nsis7z") && function.starts_with("extract") {
                    // The plugin takes the archive from the top of the stack; the details text is below it.
                    let archive = walk.stack.iter().rev().find(|s| s.0.to_ascii_lowercase().ends_with(".7z"));
                    let archive = archive.map(|(text, rooted)| walk.place(text, *rooted));
                    let into = parts(&walk.out).unwrap_or_default();
                    script.extracts.extend(archive.map(|archive| Extract { archive, into }));
                }
            }
            WRITE_INI => {
                let (section, key, value) = (walk.text(p[0]).0, walk.text(p[1]).0, walk.text(p[2]).0);
                let file = parts(&walk.path(p[3]));
                if let Some(file) = file.filter(|_| !section.is_empty()) {
                    script.ini.push(IniWrite { file, section, key, value });
                }
            }
            _ => {}
        }
    }
    script
}

impl Extract {
    /// Whether this extraction is of the file written to `path`.
    pub fn is(&self, path: &[String]) -> bool {
        same(&self.archive, &path.join("\\"))
    }
}
