//! Classic-format stages: the RSF-shaped stage folders (`Maps/<id>-<name>/` with a `Tracks<id>.ini`) and the loose
//! ones (`track-<n>_<letter>.*` files, the stage's `[Map<n>]` section of a `Tracks.ini` or an installer's INI write,
//! its weathers in `TrackSettings*.ini` files or INI writes) of any [`Source`], added once into
//! `<player folder>/tracks/<number>-<name>/` (beside the BTB stages): the RSF stage folder `rbr_formats::rsf::stages` reads, with a number of
//! its own when the stage's is taken, so nothing at run time needs a special case. The stage is built in a hidden
//! folder beside its destination and renamed into place only when it opens: a failure or a cancel leaves nothing.
//! Refused, with the reason: a stage RallySimFans or the RBR Czech plugin encrypts, one missing a required file, one
//! already added. `find` reads the names, `renumber` and `write` hold the pure rewrites.

mod find;
mod renumber;
mod write;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rbr_formats::ini::Section;
use rbr_formats::rsf::{self, REQUIRED};
use rbr_formats::stage_files;

use super::{Ctx, Found, Item, Kind};
use crate::archive::join;
use crate::content::source::{file_lower, Entry, Source};
use crate::content::{Goto, Outcome, Refresh};
use crate::job::{Counter, Temp};

pub const LABEL: &str = "classic stage";

/// Why an encrypted stage is refused.
pub const ENCRYPTED: &str = "this stage is encrypted for the RBR Czech plugin / RallySimFans: it can't be used";

type Fail = String;

/// The small files read before anything is copied: path in the source and bytes.
type Texts = Vec<(Vec<String>, Vec<u8>)>;

pub struct Classic;

/// Whether the source's names look like a classic stage, complete or not.
pub fn is_classic(src: &dyn Source) -> bool {
    src.entries().iter().any(|e| {
        let file = e.file_lower();
        let in_maps = e.path.len() >= 2 && e.path[..e.path.len() - 1].iter().any(|d| d.eq_ignore_ascii_case("maps"));
        (in_maps && file.starts_with("track-"))
            || file == "tracks.ini"
            || find::tracks_number(&file).is_some()
            || (file.starts_with("tracksettings") && file.ends_with(".ini"))
            || find::variant_file(&file).is_some()
    })
}

impl Kind for Classic {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        let found = find::stages(src.entries()).len();
        (found > 0 || is_classic(src)).then(|| Found::new(if found > 1 { "classic stages" } else { LABEL }))
    }

    fn add(&self, item: &Item, ctx: &Ctx) -> Vec<Outcome> {
        let (src, name) = (item.src, item.src.name());
        let stages = find::stages(src.entries());
        if stages.is_empty() {
            let why =
                "no stage in it: a stage needs its track-<number>_<letter>.lbs, .col, .trk, .dls, .mat and .ini files";
            return vec![Outcome::failed(&name, LABEL, why)];
        }
        ctx.step("Adding classic stage");
        let texts = match read_texts(src) {
            Ok(texts) => texts,
            Err(why) => return vec![Outcome::failed(&name, LABEL, why)],
        };
        let one = |stage: &find::Found| match ctx.cancelled() {
            true => Outcome::failed(&name, LABEL, "cancelled"),
            false => match import(src, stage, &texts, ctx) {
                Ok(done) => done.outcome(&name),
                Err(Refused { name, why }) => Outcome::failed(&name, LABEL, why),
            },
        };
        stages.iter().map(one).collect()
    }
}

/// What an added stage is.
struct Added {
    name: String,
    author: Option<String>,
    folder: String,
    licence: Option<PathBuf>,
    /// The variants that lack a required file and were not added.
    left_out: Vec<char>,
}

impl Added {
    fn outcome(self, source: &str) -> Outcome {
        let by = self.author.map(|a| format!(" by {a}")).unwrap_or_default();
        let words: Vec<String> = self.left_out.iter().map(char::to_string).collect();
        let left = match words.len() {
            0 => String::new(),
            1 => format!("; its {} variant is incomplete and was left out", words[0]),
            _ => format!("; its {} variants are incomplete and were left out", words.join(" and ")),
        };
        let text = format!("Added {} ({LABEL}){by}{left}", self.name);
        Outcome::said(source, LABEL, text, Goto::Stage(self.folder)).opening(self.licence).refreshing(Refresh::Stages)
    }
}

/// A stage that was not added: what to call it and why.
struct Refused {
    name: String,
    why: Fail,
}

/// Reads the source's `Tracks`/`TrackSettings` inis and text files (small) in one go.
fn read_texts(src: &dyn Source) -> Result<Texts, Fail> {
    let metas = find::meta(src.entries());
    let temp = Temp::create(&std::env::temp_dir(), "classic-read")?;
    let wanted: Vec<_> = metas.iter().map(|e| (e.path.clone(), e.path.clone())).collect();
    src.extract(&wanted, &temp.0, &mut |_| Ok(()))?;
    metas
        .iter()
        .map(|e| {
            let file = join(&temp.0, &e.path);
            Ok((e.path.clone(), fs::read(&file).map_err(|err| format!("{}: {err}", e.path.join("/")))?))
        })
        .collect()
}

/// Whether `file` (lowercase) is the `Tracks.ini` or the `Tracks<n>.ini`.
fn is_tracks_ini(file: &str, n: u32) -> bool {
    file == "tracks.ini" || find::tracks_number(file) == Some(n)
}

fn dir_of(path: &[String]) -> &[String] {
    path.split_last().map_or(&[], |(_, d)| d)
}

fn in_dir(path: &[String], dir: &[String]) -> bool {
    dir_of(path).len() == dir.len() && dir_of(path).iter().zip(dir).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// The stage's `[Map<n>]` entries: the stage folder's `Tracks<n>.ini`, else a `Tracks.ini` of the tree, then the
/// installer's INI writes over them; empty when nothing has one. An encrypted `Tracks<n>.ini` is refused.
fn map_entries(src: &dyn Source, stage: &find::Found, texts: &Texts) -> Result<Vec<(String, String)>, Fail> {
    let (n, map) = (stage.number, format!("Map{}", stage.number));
    let mut files: Vec<&(Vec<String>, Vec<u8>)> =
        texts.iter().filter(|(path, _)| is_tracks_ini(&file_lower(path), n)).collect();
    files.sort_by_key(|(path, _)| !in_dir(path, &stage.dir));
    let mut entries: Option<Vec<(String, String)>> = None;
    for (path, bytes) in files {
        if stage_files::encrypted(bytes) {
            if stage.rsf && in_dir(path, &stage.dir) {
                return Err(ENCRYPTED.into());
            }
            continue;
        }
        let found = write::parse_sections(bytes).into_iter().find(|s| s.name.eq_ignore_ascii_case(&map));
        if let Some(section) = found {
            entries = Some(section.entries);
            break;
        }
    }
    let mut entries = entries.unwrap_or_default();
    for w in src.ini_writes() {
        if is_tracks_ini(&file_lower(&w.file), n) && w.section.eq_ignore_ascii_case(&map) {
            // An empty key is a section delete (an installer's uninstall part follows its install part): ignored.
            if w.key.is_empty() {
                continue;
            }
            match entries.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&w.key)) {
                Some(entry) => entry.1 = write::latin1(w.value.as_bytes()),
                None => entries.push((w.key.clone(), write::latin1(w.value.as_bytes()))),
            }
        }
    }
    Ok(entries)
}

/// A stage folder of an RSF install (a dropped folder) gets what RSF's stage list (`rsfdata/cache/stages_data.json`)
/// says about it and its `Tracks<id>.ini` does not: the surface, length, author and country.
fn fill_from_rsf_list(src: &dyn Source, stage: &find::Found, entries: &mut Vec<(String, String)>) {
    let dir = join(src.path(), &stage.dir);
    let Some(root) = src.is_dir().then(|| rsf::root_of(&dir)).flatten() else { return };
    let list = rsf::stage_list(root);
    let Some(row) = list.get(&stage.number) else { return };
    let surface = row.get("surface_id").and_then(|s| match s.as_str() {
        "1" => Some("0"),
        "2" => Some("1"),
        "3" => Some("2"),
        _ => None,
    });
    let km = row.get("length").and_then(|m| m.parse::<f32>().ok()).map(|m| (m / 1000.0).to_string());
    let from = [
        ("StageName", row.get("name").cloned()),
        ("Surface", surface.map(str::to_owned)),
        ("Length", km),
        ("Author", row.get("author").cloned()),
        ("CountryCode", row.get("short_country").cloned()),
    ];
    for (key, value) in from {
        let missing = !entries.iter().any(|(k, v)| k.eq_ignore_ascii_case(key) && !write::unquoted(v).is_empty());
        if let (true, Some(value)) = (missing, value.filter(|v| !v.is_empty())) {
            entries.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
            entries.push((key.to_owned(), write::latin1(value.as_bytes())));
        }
    }
}

/// Every weather section of the source's `TrackSettings*.ini` files and INI writes.
fn settings_sections(src: &dyn Source, texts: &Texts) -> Vec<Section> {
    let named = |path: &[String]| {
        let file = file_lower(path);
        file.starts_with("tracksettings") && file.ends_with(".ini")
    };
    let mut sections: Vec<Section> = texts
        .iter()
        .filter(|(path, bytes)| named(path) && !stage_files::encrypted(bytes))
        .flat_map(|(_, bytes)| write::parse_sections(bytes))
        .collect();
    for w in src.ini_writes().iter().filter(|w| named(&w.file)) {
        if w.key.is_empty() {
            continue;
        }
        let at = match sections.iter().position(|s| s.name == w.section) {
            Some(at) => at,
            None => {
                sections.push(Section { name: w.section.clone(), entries: Vec::new() });
                sections.len() - 1
            }
        };
        sections[at].entries.push((w.key.clone(), write::latin1(w.value.as_bytes())));
    }
    sections
}

/// The first of `names` (ignoring case) that an entry of the source is called, with where it is: the files of the
/// stage first.
fn find_named<'a>(entries: &'a [Entry], stage: &find::Found, name: &str) -> Option<&'a Entry> {
    let called = |e: &&Entry| e.file_lower() == name.to_ascii_lowercase();
    let mut all: Vec<&Entry> = entries.iter().filter(called).collect();
    all.sort_by_key(|e| !in_dir(&e.path, &stage.dir));
    all.into_iter().next()
}

/// The numbers and names in the player's `tracks/`: its folders `<number>-<name>`.
fn folders(tracks: &Path) -> Vec<(u32, PathBuf)> {
    let list = fs::read_dir(tracks).into_iter().flatten().filter_map(Result::ok);
    list.filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        Some((name.split_once('-')?.0.parse().ok()?, e.path()))
    })
    .collect()
}

/// What makes two stages the same: the `StageName` and the variant files' letters, extensions and sizes (the files
/// the import copies as they are).
type Signature = (String, Vec<(char, String, u64)>);

fn signature(name: &str, files: impl Iterator<Item = (String, u64)>) -> Signature {
    let mut parts: Vec<_> = files
        .filter_map(|(file, size)| {
            let (_, letter) = find::variant_file(&file)?;
            Some((letter, file.rsplit_once('.')?.1.to_ascii_lowercase(), size))
        })
        .collect();
    parts.sort();
    (name.trim().to_lowercase(), parts)
}

/// The signature of a stage folder of ours.
fn signature_of(dir: &Path) -> Option<Signature> {
    let files: Vec<(String, u64)> = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.metadata().ok()?.len())))
        .collect();
    let tracks = files.iter().find(|(f, _)| find::tracks_number(f).is_some())?;
    let sections = write::parse_sections(&fs::read(dir.join(&tracks.0)).ok()?);
    let section = sections.iter().find(|s| s.name.len() > 3 && s.name[..3].eq_ignore_ascii_case("map"))?;
    let name = write::unquoted(section.get("StageName")?).to_owned();
    Some(signature(&name, files.into_iter()))
}

/// The variants of a stage's files (with their paths in the stage folder) that have every required file, and the
/// letters of those that have a `.lbs` but not all of them (left out). A `.lbs`-less letter is no variant, as for
/// `rsf::Stage` (RSF stage 368 has a stray `track-368_M.ini`). With no complete variant, the first required file the
/// most complete one lacks.
fn variants(files: &[(&Entry, Vec<String>)], number: u32) -> Result<(Vec<char>, Vec<char>), Fail> {
    let mut letters: Vec<char> = files
        .iter()
        .filter(|(_, rel)| rel.len() == 1 && rel[0].to_ascii_lowercase().ends_with(".lbs"))
        .filter_map(|(_, rel)| find::variant_file(&rel[0]).map(|(_, l)| l))
        .collect();
    letters.sort_unstable();
    letters.dedup();
    let have = |name: &str| files.iter().any(|(_, rel)| rel.len() == 1 && rel[0].eq_ignore_ascii_case(name));
    let lacking = |letter: char| -> Vec<String> {
        let names = REQUIRED.iter().map(|ext| format!("track-{number}_{letter}.{ext}"));
        names.filter(|name| !have(name)).collect()
    };
    let (whole, partial): (Vec<char>, Vec<char>) = letters.iter().partition(|&&l| lacking(l).is_empty());
    if !whole.is_empty() {
        return Ok((whole, partial));
    }
    let first = letters.iter().map(|&l| lacking(l)).min_by_key(Vec::len).and_then(|l| l.into_iter().next());
    Err(format!("{} is missing", first.unwrap_or_else(|| format!("track-{number}_<letter>.lbs"))))
}

/// Whether `rel` (a stage file's path in the stage folder) belongs to variant `letter` of stage `number`.
fn of_variant(rel: &[String], number: u32, letter: char) -> bool {
    let name = rel[0].to_ascii_lowercase();
    let prefix = format!("track-{number}_{}", letter.to_ascii_lowercase());
    name.strip_prefix(&prefix).is_some_and(|rest| rest.starts_with(['.', '_']))
}

/// The first four bytes of a file.
fn head(path: &Path) -> [u8; 4] {
    let mut head = [0u8; 4];
    let _ = fs::File::open(path).and_then(|mut f| f.read_exact(&mut head));
    head
}

/// Refuses a built folder with an encrypted file: RallySimFans' header on a stage file or texture archive, a `.col`
/// that is not `OC7R`.
fn check_encrypted(staged: &Path) -> Result<(), Fail> {
    for entry in fs::read_dir(staged).map_err(|e| format!("{}: {e}", staged.display()))?.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
        let checked = REQUIRED.contains(&ext) || ext == "rbz" || name.starts_with("texturefilenamemap");
        if !entry.path().is_file() || !checked {
            continue;
        }
        let first = head(&entry.path());
        if stage_files::encrypted(&first) || (ext == "col" && &first != b"OC7R") {
            return Err(ENCRYPTED.into());
        }
    }
    Ok(())
}

/// The licence or readme to offer: the text file called so, else the first.
fn licence_of(staged: &Path) -> Option<String> {
    let mut texts: Vec<String> = fs::read_dir(staged)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| f.to_ascii_lowercase().ends_with(".txt"))
        .collect();
    texts.sort();
    let named = |f: &String| ["licen", "read", "info"].iter().any(|w| f.to_ascii_lowercase().contains(w));
    texts.iter().find(|f| named(f)).or(texts.first()).cloned()
}

fn read_error(path: &Path, e: impl std::fmt::Display) -> Fail {
    format!("{}: {e}", path.display())
}

/// Turns the number in the built folder's remap and texture archives.
fn renumber_contents(staged: &Path, old: u32, new: u32, count: &mut Counter) -> Result<(), Fail> {
    for entry in fs::read_dir(staged).map_err(|e| read_error(staged, e))?.filter_map(Result::ok) {
        let (path, name) = (entry.path(), entry.file_name().to_string_lossy().to_ascii_lowercase());
        if name.starts_with("texturefilenamemap") && name.ends_with(".ini") {
            let text = write::latin1(&fs::read(&path).map_err(|e| read_error(&path, e))?);
            fs::write(&path, write::unlatin1(&renumber::track_references(&text, old, new)))
                .map_err(|e| read_error(&path, e))?;
        } else if name.starts_with(&format!("track-{new}_")) && name.ends_with(".rbz") {
            let temp = path.with_extension("rbz-renumbering");
            renumber::rewrite_zip(&path, &temp, old, new, &mut |n| count.add(n))?;
            fs::rename(&temp, &path).map_err(|e| read_error(&path, e))?;
        }
    }
    Ok(())
}

/// Adds the stage `stage` of `src` to the player's `tracks/`.
fn import(src: &dyn Source, stage: &find::Found, texts: &Texts, ctx: &Ctx) -> Result<Added, Refused> {
    let mut label = format!("track-{}", stage.number);
    build(src, stage, texts, ctx, &mut label).map_err(|why| Refused { name: label, why })
}

/// [`import`], with `label` the stage's name once it is known.
fn build(src: &dyn Source, stage: &find::Found, texts: &Texts, ctx: &Ctx, label: &mut String) -> Result<Added, Fail> {
    let (entries, old) = (src.entries(), stage.number);
    let mut own = map_entries(src, stage, texts)?;
    fill_from_rsf_list(src, stage, &mut own);
    let get = |key: &str| own.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| write::unquoted(v));
    let fallback = || match stage.dir.last().filter(|_| stage.rsf).and_then(|d| d.split_once('-')) {
        Some((_, name)) => name.to_owned(),
        None => crate::content::stem(&src.name()).to_owned(),
    };
    let name = write::shown(get("StageName").filter(|n| !n.is_empty()).map_or_else(fallback, str::to_owned).as_str());
    *label = name.clone();
    if get("Key").is_some() {
        return Err(ENCRYPTED.into());
    }
    let author = get("Author").map(write::shown).filter(|a| !a.is_empty());

    let mut files = find::files(entries, stage);
    let (letters, left_out) = variants(&files, old)?;
    files.retain(|(_, rel)| !left_out.iter().any(|&l| of_variant(rel, old, l)));
    let tracks = ctx.home.join(rbr_btb::player::TRACKS);
    let existing = folders(&tracks);
    let taken = |n: u32| ctx.known.numbers.contains(&n) || existing.iter().any(|(e, _)| *e == n);
    let new = write::pick_number(old, &taken).ok_or("no free stage number")?;
    let folder = format!("{new}-{}", write::folder_name(&name));

    // What is copied: the stage's files renamed, with the splash, the particle files and (loose stages) the text files.
    let mut wanted: Vec<(Vec<String>, Vec<String>)> =
        files.iter().map(|(e, rel)| (e.path.clone(), renumber::path(rel, old, new))).collect();
    let mut size: u64 = files.iter().map(|(e, _)| e.size).sum();
    let mut also = |called: String, wanted: &mut Vec<(Vec<String>, Vec<String>)>| -> Option<String> {
        let entry = find_named(entries, stage, &called)?;
        let to = vec![entry.path.last()?.clone()];
        if !wanted.iter().any(|(_, t)| t.len() == 1 && t[0].eq_ignore_ascii_case(&to[0])) {
            wanted.push((entry.path.clone(), to.clone()));
            size += entry.size;
        }
        Some(to[0].clone())
    };
    let base = |value: &str| value.rsplit(['\\', '/']).next().unwrap_or(value).to_owned();
    let splash = get("SplashScreen").map(base).filter(|s| !s.is_empty()).and_then(|s| also(s, &mut wanted));
    let particles = get("Particles").map(base).filter(|p| !p.is_empty()).and_then(|p| {
        let found = also(format!("{p}.ini"), &mut wanted)?;
        also(format!("{p}_LOW.ini"), &mut wanted);
        Some(found.strip_suffix(".ini").map(str::to_owned).unwrap_or(p))
    });
    let loose_texts: Vec<(&String, &Vec<u8>)> = match stage.rsf {
        true => Vec::new(),
        false => {
            texts.iter().filter(|(p, _)| file_lower(p).ends_with(".txt")).map(|(p, b)| (p.last().unwrap(), b)).collect()
        }
    };

    let temp = Temp::create(&tracks, "adding")?;
    let staged = temp.0.join(&folder);
    let rbz: u64 = files.iter().filter(|(e, _)| e.file_lower().ends_with(".rbz")).map(|(e, _)| e.size).sum();
    let mut count =
        Counter { step: "Adding classic stage", done: 0, total: size + rbz, report: ctx.report, cancel: ctx.cancel };
    count.add(0)?;
    src.extract(&wanted, &staged, &mut |n| count.add(n))?;
    fs::create_dir_all(&staged).map_err(|e| read_error(&staged, e))?;
    for (file, bytes) in loose_texts {
        fs::write(staged.join(file), bytes).map_err(|e| read_error(&staged, e))?;
    }
    check_encrypted(&staged)?;
    renumber_contents(&staged, old, new, &mut count)?;

    let tracks_ini = write::Tracks {
        entries: &own,
        new,
        folder: &folder,
        name: &name,
        particles: particles.as_deref(),
        splash: splash.as_deref(),
    };
    let write_text = |file: String, text: String| {
        fs::write(staged.join(&file), write::unlatin1(&text)).map_err(|e| read_error(&staged, e))
    };
    write_text(format!("Tracks{new}.ini"), tracks_ini.text())?;
    let mut weathers = write::own_weathers(&settings_sections(src, texts), old, new);
    weathers.retain(|w| !left_out.iter().any(|l| w.name[new.to_string().len()..].starts_with(*l)));
    if weathers.is_empty() {
        let retail = ctx.known.weather.as_deref().and_then(|p| fs::read(p).ok()).unwrap_or_default();
        let kind = write::surface_kind(get("Surface"));
        weathers =
            write::borrowed_weathers(&write::parse_sections(&retail), kind, &write::letters_in_order(&letters), new);
    }
    if !weathers.is_empty() {
        write_text(format!("TrackSettings{new}.ini"), write::settings_text(&weathers))?;
    }

    // The listed sizes of an installer's files are stored sizes: the copies' own are compared.
    let copied = signature_of(&staged);
    if copied.is_some() && existing.iter().any(|(_, dir)| signature_of(dir) == copied) {
        return Err("already added".into());
    }
    let opened = rsf::Stage::open(&staged, &Default::default()).map_err(|e| format!("the stage doesn't open: {e}"))?;
    if let Some(problem) = opened.problem {
        return Err(format!("the stage doesn't open: {problem}"));
    }
    let licence = licence_of(&staged);
    let dest = tracks.join(&folder);
    if dest.exists() {
        return Err("already added".into());
    }
    fs::rename(&staged, &dest).map_err(|e| read_error(&dest, e))?;
    Ok(Added { name, author, folder, licence: licence.map(|l| dest.join(l)), left_out })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::content::kind::btb::tests::write;
    use crate::content::kind::Known;
    use crate::content::source::{self, IniWrite, Wanted};

    /// A source that adds installer INI writes to another.
    pub struct Writes(pub Box<dyn Source>, pub Vec<IniWrite>);

    impl Source for Writes {
        fn path(&self) -> &Path {
            self.0.path()
        }
        fn is_dir(&self) -> bool {
            self.0.is_dir()
        }
        fn entries(&self) -> &[Entry] {
            self.0.entries()
        }
        fn extract(&self, w: &Wanted, d: &Path, p: &mut dyn FnMut(u64) -> Result<(), Fail>) -> Result<(), Fail> {
            self.0.extract(w, d, p)
        }
        fn ini_writes(&self) -> &[IniWrite] {
            &self.1
        }
    }

    pub fn ini_write(file: &str, section: &str, key: &str, value: &str) -> IniWrite {
        let file = file.split('/').map(str::to_owned).collect();
        IniWrite { file, section: section.into(), key: key.into(), value: value.into() }
    }

    fn col() -> Vec<u8> {
        b"OC7Rcol".to_vec()
    }

    /// An `.rbz` with entries under `track-<id>_<l>_textures/`.
    fn textures(path: &Path, id: u32, letter: char) {
        let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for entry in [format!("track-{id}_{letter}_textures/dry/a.dds"), format!("track-{id}_{letter}_textures/b.dds")]
        {
            w.start_file(entry, stored).unwrap();
            w.write_all(b"dds").unwrap();
        }
        w.finish().unwrap();
    }

    /// The files of one variant of stage `id` in `dir`, as the game's: all required, a texture archive, the
    /// texture remap and a marker.
    pub fn variant(dir: &Path, id: u32, letter: char) {
        for (ext, bytes) in
            [("lbs", b"lbs1".to_vec()), ("col", col()), ("trk", b"trk1".to_vec()), ("dls", b"dls1".to_vec())]
        {
            write(&dir.join(format!("track-{id}_{letter}.{ext}")), &bytes);
        }
        write(&dir.join(format!("track-{id}_{letter}.mat")), b"mat1");
        write(&dir.join(format!("track-{id}_{letter}.ini")), b"[Texture7]\nTexture7=a.dds\n");
        textures(&dir.join(format!("track-{id}_{letter}_textures.rbz")), id, letter);
    }

    fn tracks(id: u32, folder: &str, extra: &str) -> String {
        format!(
            "[Map{id}]\r\nTrackName=\"Maps\\{folder}\\track-{id}\"\r\nParticles=\"Maps\\ps_british\"\r\nStageName=\"Kamenak\"\r\nSurface=0\r\nAuthor=Miro Kurek\r\n{extra}"
        )
    }

    /// An RSF-shaped stage folder `<dir>/<id>-Kamenak/` with the variants `letters`.
    pub fn rsf_stage(dir: &Path, id: u32, letters: &str) -> PathBuf {
        let folder = format!("{id}-Kamenak");
        let stage = dir.join(&folder);
        write(
            &stage.join(format!("Tracks{id}.ini")),
            tracks(id, &folder, "SplashScreen=\"Textures\\Splash\\x.dds\"\r\n").as_bytes(),
        );
        let weather: String = letters.chars().map(|l| format!("[{id}{l}_crisp_clear]\r\nFogEnd = 5\r\n\r\n")).collect();
        write(&stage.join(format!("TrackSettings{id}.ini")), weather.as_bytes());
        write(
            &stage.join(format!("TextureFilenameMap{id}.ini")),
            format!("; remap\r\ntrack-{id}_o_textures\\dry\\a.dds\ttrack-13_o_textures\\dry\\a.dds\r\n").as_bytes(),
        );
        write(&stage.join(format!("track-{id}_13_Kamenak")), b"");
        write(&stage.join("Track-licence.txt"), b"free to use");
        write(&stage.join("x.dds"), b"splash");
        write(&stage.join("lbsloading.log"), b"log");
        for l in letters.chars() {
            variant(&stage, id, l);
        }
        stage
    }

    pub fn add(home: &Path, src: Box<dyn Source>, known: &Known) -> Vec<Outcome> {
        let item = Item { src: &*src, found: Found::new(LABEL) };
        let report = &|_: &crate::job::Progress| {};
        let cancel = AtomicBool::new(false);
        Classic.add(&item, &Ctx { home, known, report, cancel: &cancel })
    }

    fn lines(done: &[Outcome]) -> Vec<String> {
        done.iter().map(|o| format!("{} {}", if o.ok() { "ok" } else { "no" }, o.line())).collect()
    }

    fn names(dir: &Path) -> Vec<String> {
        let list = fs::read_dir(dir).into_iter().flatten();
        let mut names: Vec<String> = list.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn a_stage_folder_keeps_its_number_when_free_and_is_the_rsf_shape() {
        let dir = crate::test_dir("classic-keep");
        rsf_stage(&dir.join("drop"), 101, "OM");
        let home = dir.join("home");
        let done = add(&home, source::open(&dir.join("drop")).unwrap(), &Known::default());
        assert_eq!(lines(&done), ["ok Added Kamenak (classic stage) by Miro Kurek"], "{done:?}");
        assert_eq!(done[0].goto, Some(Goto::Stage("101-Kamenak".into())));
        assert_eq!(done[0].open, Some(home.join("tracks/101-Kamenak/Track-licence.txt")));
        let stage = rsf::open(&home.join("tracks/101-Kamenak")).unwrap();
        assert_eq!((stage.id, stage.variants.clone(), stage.problem.clone()), (101, vec!['M', 'O'], None));
        assert_eq!(stage.weathers.len(), 2);
        assert!(names(&home.join("tracks")).iter().all(|n| !n.starts_with('.')), "{:?}", names(&home.join("tracks")));
        assert!(!home.join("tracks/101-Kamenak/lbsloading.log").exists());
        // The same stage again.
        let again = add(&home, source::open(&dir.join("drop")).unwrap(), &Known::default());
        assert_eq!(lines(&again), ["no Couldn't add Kamenak: already added"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_taken_number_is_renamed_everywhere_it_appears() {
        let dir = crate::test_dir("classic-renumber");
        rsf_stage(&dir.join("drop"), 101, "OM");
        let home = dir.join("home");
        write(&home.join("tracks/10000-Other/Tracks10000.ini"), b"[Map10000]\r\nStageName=Other\r\n");
        let known = Known { numbers: vec![101], ..Known::default() };
        let done = add(&home, source::open(&dir.join("drop")).unwrap(), &known);
        assert_eq!(lines(&done), ["ok Added Kamenak (classic stage) by Miro Kurek"], "{done:?}");
        let folder = home.join("tracks/10001-Kamenak");
        assert_eq!(done[0].goto, Some(Goto::Stage("10001-Kamenak".into())));
        assert_eq!(
            names(&folder),
            [
                "TextureFilenameMap10001.ini",
                "Track-licence.txt",
                "TrackSettings10001.ini",
                "Tracks10001.ini",
                "track-10001_13_Kamenak",
                "track-10001_M.col",
                "track-10001_M.dls",
                "track-10001_M.ini",
                "track-10001_M.lbs",
                "track-10001_M.mat",
                "track-10001_M.trk",
                "track-10001_M_textures.rbz",
                "track-10001_O.col",
                "track-10001_O.dls",
                "track-10001_O.ini",
                "track-10001_O.lbs",
                "track-10001_O.mat",
                "track-10001_O.trk",
                "track-10001_O_textures.rbz",
                "x.dds"
            ]
        );
        let text = |f: &str| String::from_utf8(fs::read(folder.join(f)).unwrap()).unwrap();
        let tracks = text("Tracks10001.ini");
        assert!(tracks.starts_with("[Map10001]\r\nTrackName = \"tracks\\10001-Kamenak\\track-10001\"\r\n"), "{tracks}");
        assert!(tracks.contains("SplashScreen = \"tracks\\10001-Kamenak\\x.dds\""), "{tracks}");
        assert!(!tracks.contains("101"), "{tracks}");
        let settings = text("TrackSettings10001.ini");
        assert!(settings.contains("[10001O_crisp_clear]") && settings.contains("[10001M_crisp_clear]"), "{settings}");
        assert!(!settings.contains("101"), "{settings}");
        let remap = text("TextureFilenameMap10001.ini");
        assert!(remap.contains("track-10001_o_textures\\dry\\a.dds\ttrack-13_o_textures\\dry\\a.dds"), "{remap}");
        let mut zip = zip::ZipArchive::new(fs::File::open(folder.join("track-10001_M_textures.rbz")).unwrap()).unwrap();
        let entries: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        assert_eq!(entries, ["track-10001_M_textures/dry/a.dds", "track-10001_M_textures/b.dds"]);
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut zip.by_index(0).unwrap(), &mut bytes).unwrap();
        assert_eq!(bytes, b"dds");
        // The stage reads as a stage folder with its own textures, and starts from the launcher's path.
        let stage = rsf::open(&folder).unwrap();
        assert_eq!((stage.id, stage.problem.clone(), stage.weathers.len()), (10_001, None, 2));
        assert_eq!(rsf::folder_id(&stage.stage('O')), Some(10_001));
        assert!(stage.texture_map().unwrap().pairs().any(|(k, _)| k.starts_with("track-10001_o_")));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A loose stage of number `id` (letters `letters`) in `dir/Maps`, the way an installer unpacks one.
    fn loose_stage(dir: &Path, id: u32, letters: &str) {
        for l in letters.chars() {
            variant(&dir.join("Maps"), id, l);
        }
        write(
            &dir.join(format!("Maps/TextureFilenameMap{id}.ini")),
            format!("track-{id}_o_textures\\dry\\a.dds\tx\r\n").as_bytes(),
        );
        write(&dir.join("Readme.txt"), b"licence");
        write(&dir.join("Maps/splash.dds"), b"splash");
    }

    fn installer(dir: &Path, id: u32) -> Box<dyn Source> {
        let map = |key, value| ini_write("Maps/Tracks.ini", &format!("Map{id}"), key, value);
        let settings = |section: &str, key, value| ini_write("Maps/TrackSettings.ini", section, key, value);
        let writes = vec![
            map("TrackName", "Maps\\x\\track-8994"),
            map("StageName", "Zaton"),
            map("Author", "Mihkel Kütt"),
            map("Surface", "1"),
            map("SplashScreen", "Maps\\splash.dds"),
            map("Particles", "Maps\\PS_Zaton"),
            map("", ""),
            settings(&format!("{id}M_hazy_clear"), "", ""),
            settings(&format!("{id}M_crisp_clear"), "FogEnd", "7"),
            settings(&format!("{id}M_hazy_clear"), "FogEnd", "8"),
            settings("9999M_crisp_clear", "FogEnd", "9"),
        ];
        Box::new(Writes(source::open(dir).unwrap(), writes))
    }

    #[test]
    fn loose_files_with_an_installers_ini_writes() {
        let dir = crate::test_dir("classic-loose");
        loose_stage(&dir.join("drop"), 8994, "MO");
        write(&dir.join("drop/Maps/PS_Zaton.ini"), b"[PS]\n");
        let home = dir.join("home");
        let done = add(&home, installer(&dir.join("drop"), 8994), &Known::default());
        assert_eq!(lines(&done), ["ok Added Zaton (classic stage) by Mihkel Kütt"], "{done:?}");
        let folder = home.join("tracks/8994-Zaton");
        let tracks = fs::read_to_string(folder.join("Tracks8994.ini")).unwrap();
        assert!(
            tracks.contains("Particles = \"tracks\\8994-Zaton\\PS_Zaton\"") && tracks.contains("Surface = 1"),
            "{tracks}"
        );
        assert!(tracks.contains("SplashScreen = \"tracks\\8994-Zaton\\splash.dds\""), "{tracks}");
        assert!(folder.join("splash.dds").is_file() && folder.join("PS_Zaton.ini").is_file());
        assert_eq!(done[0].open, Some(folder.join("Readme.txt")));
        let stage = rsf::open(&folder).unwrap();
        assert_eq!((stage.id, stage.problem.clone(), stage.surface), (8994, None, Some(rsf::Surface::Gravel)));
        assert_eq!(stage.author.as_deref(), Some("Mihkel Kütt"));
        assert_eq!(
            stage.weathers.iter().map(|w| w.section.as_str()).collect::<Vec<_>>(),
            ["8994M_crisp_clear", "8994M_hazy_clear"]
        );
        assert!(stage.particle_file(&rbr_formats::data::Data::new(&dir, None), false).is_some_and(|p| p.is_file()));
        assert!(folder.join("TextureFilenameMap8994.ini").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tracks_ini_and_settings_files_of_the_tree_do_for_the_writes() {
        let dir = crate::test_dir("classic-tree");
        loose_stage(&dir.join("drop"), 77, "E");
        write(
            &dir.join("drop/Maps/Tracks.ini"),
            b"[Map5]\r\nStageName=Not this\r\n[Map77]\r\nStageName=Seventy\r\nSurface=2\r\n",
        );
        write(&dir.join("drop/Maps/TrackSettings_77.ini"), b"[77E_crisp_clear]\r\nFogEnd=1\r\n");
        let known = Known { numbers: vec![77], ..Known::default() };
        let done = add(&dir.join("home"), source::open(&dir.join("drop")).unwrap(), &known);
        assert_eq!(lines(&done), ["ok Added Seventy (classic stage)"], "{done:?}");
        let stage = rsf::open(&dir.join("home/tracks/10000-Seventy")).unwrap();
        assert_eq!((stage.id, stage.problem.clone(), stage.surface), (10_000, None, Some(rsf::Surface::Snow)));
        assert_eq!(stage.weathers[0].section, "10000E_crisp_clear");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stage_without_weathers_borrows_a_retail_stages() {
        let dir = crate::test_dir("classic-weather");
        loose_stage(&dir.join("drop"), 8994, "MO");
        write(&dir.join("drop/Maps/Tracks.ini"), b"[Map8994]\r\nStageName=Dry\r\nSurface=0\r\n");
        let retail = "[41M_crisp_clear]\r\nFogEnd=1\r\n[41O_M_lightfog_heavycloud]\r\nFogEnd=2\r\n[54M_crisp_clear]\r\nFogEnd=3\r\n[54E_crisp_clear]\r\nFogEnd=3\r\n";
        write(&dir.join("TrackSettings.ini"), retail.as_bytes());
        let known = Known { weather: Some(dir.join("TrackSettings.ini")), ..Known::default() };
        let done = add(&dir.join("home"), source::open(&dir.join("drop")).unwrap(), &known);
        assert_eq!(lines(&done), ["ok Added Dry (classic stage)"], "{done:?}");
        let stage = rsf::open(&dir.join("home/tracks/8994-Dry")).unwrap();
        // Tarmac: the tarmac stage 41's rows of the stage's letters.
        let sections: Vec<&str> = stage.weathers.iter().map(|w| w.section.as_str()).collect();
        assert_eq!(sections, ["8994M_crisp_clear", "8994O_M_lightfog_heavycloud"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn several_stages_of_one_source_are_all_added() {
        let dir = crate::test_dir("classic-several");
        loose_stage(&dir.join("drop"), 8994, "MO");
        loose_stage(&dir.join("drop"), 8995, "N");
        write(&dir.join("drop/Maps/Tracks.ini"), b"[Map8994]\r\nStageName=One\r\n[Map8995]\r\nStageName=Two\r\n");
        let done = add(&dir.join("home"), source::open(&dir.join("drop")).unwrap(), &Known::default());
        assert_eq!(lines(&done), ["ok Added One (classic stage)", "ok Added Two (classic stage)"], "{done:?}");
        assert_eq!(names(&dir.join("home/tracks")), ["8994-One", "8995-Two"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_is_refused_and_why() {
        let dir = crate::test_dir("classic-refused");
        let home = dir.join("home");
        let refused = |name: &str, build: &dyn Fn(&Path)| -> String {
            let drop = dir.join(name);
            build(&drop);
            let done = add(&home, source::open(&drop).unwrap(), &Known::default());
            assert_eq!(done.len(), 1, "{done:?}");
            assert!(!done[0].ok(), "{done:?}");
            assert!(names(&home.join("tracks")).is_empty(), "{:?}", names(&home.join("tracks")));
            done[0].line()
        };
        let key = refused("key", &|d| {
            loose_stage(d, 5, "O");
            write(&d.join("Maps/Tracks.ini"), b"[Map5]\r\nStageName=Czech\r\nKey=abc\r\n");
        });
        assert_eq!(key, format!("Couldn't add Czech: {ENCRYPTED}"));
        let rsf_header = refused("header", &|d| {
            rsf_stage(d, 6, "O");
            write(&d.join("6-Kamenak/track-6_O.lbs"), &[0x01, 0xa0, 0, 0, 1, 2]);
        });
        assert_eq!(rsf_header, format!("Couldn't add Kamenak: {ENCRYPTED}"));
        let ini_header = refused("tracksini", &|d| {
            rsf_stage(d, 6, "O");
            write(&d.join("6-Kamenak/Tracks6.ini"), &[0x01, 0xa1, 0, 0, 1, 2]);
        });
        assert_eq!(ini_header, format!("Couldn't add track-6: {ENCRYPTED}"));
        let col = refused("col", &|d| {
            rsf_stage(d, 7, "O");
            write(&d.join("7-Kamenak/track-7_O.col"), b"NOPE-not-a-col");
        });
        assert_eq!(col, format!("Couldn't add Kamenak: {ENCRYPTED}"));
        let rbz = refused("rbz", &|d| {
            rsf_stage(d, 8, "O");
            write(&d.join("8-Kamenak/track-8_O_textures.rbz"), &[0x01, 0xa0, 0, 0, 1, 2]);
        });
        assert_eq!(rbz, format!("Couldn't add Kamenak: {ENCRYPTED}"));
        let missing = refused("missing", &|d| {
            rsf_stage(d, 9, "O");
            fs::remove_file(d.join("9-Kamenak/track-9_O.trk")).unwrap();
        });
        assert_eq!(missing, "Couldn't add Kamenak: track-9_O.trk is missing");
        let nothing = refused("nothing", &|d| write(&d.join("Maps/Tracks.ini"), b"[Map1]\r\n"));
        assert!(nothing.starts_with("Couldn't add nothing: no stage in it"), "{nothing}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_incomplete_variant_is_left_out_and_said() {
        let dir = crate::test_dir("classic-incomplete");
        loose_stage(&dir.join("drop"), 94, "M");
        write(&dir.join("drop/Maps/track-94_O.ini"), b"[x]\n");
        write(&dir.join("drop/Maps/track-94_O.lbs"), b"lbs1");
        write(&dir.join("drop/Maps/Tracks.ini"), b"[Map94]\r\nStageName=Stry\r\n");
        write(
            &dir.join("drop/Maps/TrackSettings.ini"),
            b"[94O_crisp_clear]\r\nFogEnd=1\r\n[94M_crisp_clear]\r\nFogEnd=1\r\n",
        );
        let done = add(&dir.join("home"), source::open(&dir.join("drop")).unwrap(), &Known::default());
        assert_eq!(
            lines(&done),
            ["ok Added Stry (classic stage); its O variant is incomplete and was left out"],
            "{done:?}"
        );
        let folder = dir.join("home/tracks/94-Stry");
        assert!(!names(&folder).iter().any(|n| n.to_lowercase().starts_with("track-94_o")), "{:?}", names(&folder));
        let stage = rsf::open(&folder).unwrap();
        assert_eq!((stage.variants.clone(), stage.problem.clone()), (vec!['M'], None));
        assert_eq!(stage.weathers.iter().map(|w| w.section.as_str()).collect::<Vec<_>>(), ["94M_crisp_clear"]);
        // No complete variant: the most complete one's first missing file.
        let drop = dir.join("none/Maps");
        write(&drop.join("track-5_O.lbs"), b"lbs1");
        write(&drop.join("track-5_O.ini"), b"x");
        write(&drop.join("track-5_N.lbs"), b"lbs1");
        write(&drop.join("track-5_N.ini"), b"x");
        write(&drop.join("track-5_N.col"), b"OC7R");
        let done = add(&dir.join("home2"), source::open(&dir.join("none")).unwrap(), &Known::default());
        assert_eq!(lines(&done), ["no Couldn't add none: track-5_N.trk is missing"], "{done:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn numbers_above_the_btb_range_are_not_kept() {
        let dir = crate::test_dir("classic-big");
        loose_stage(&dir.join("drop"), 3_000_000, "O");
        let done = add(&dir.join("home"), source::open(&dir.join("drop")).unwrap(), &Known::default());
        assert!(done[0].ok(), "{done:?}");
        assert_eq!(names(&dir.join("home/tracks")).len(), 1);
        assert!(names(&dir.join("home/tracks"))[0].starts_with("10000-"));
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod real {
    use std::collections::HashMap;

    use super::super::Known;
    use super::tests::*;
    use super::*;
    use crate::content::source;

    fn folder_size(dir: &Path) -> u64 {
        let list = fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok);
        list.map(|e| e.metadata().map_or(0, |m| m.len())).sum()
    }

    /// Every unencrypted stage folder of the RSF install `RBR_RSF` (or `RBR_CLASSIC_STAGES` of them, evenly spread, of
    /// under 600 MB each; `all` takes every one) added with a number of its own forced, into
    /// `RBR_CLASSIC_OUT/tracks` (kept for `scripts/stage-sweep.sh`) or a temporary player folder, and listed in the
    /// catalog with the variants, weathers and surface it had.
    #[test]
    fn rsf_stage_folders_are_added_under_new_numbers_and_listed() {
        let Some(root) = std::env::var_os("RBR_RSF").map(PathBuf::from) else {
            eprintln!("skipping: RBR_RSF not set");
            return;
        };
        let all = std::env::var("RBR_CLASSIC_STAGES").is_ok_and(|v| v == "all");
        let count: usize = std::env::var("RBR_CLASSIC_STAGES").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
        let list: Vec<_> =
            rsf::scan(&root).into_iter().filter_map(Result::ok).filter(|s| s.problem.is_none()).collect();
        let small: Vec<_> = list.iter().filter(|s| all || folder_size(&s.folder) < 600 << 20).collect();
        assert!(!small.is_empty(), "no stage folder to add in {}", root.display());
        let step = (small.len() / count.max(1)).max(1);
        let picked: Vec<_> = small.iter().step_by(step).take(if all { usize::MAX } else { count }).collect();
        let (out, keep) = match std::env::var_os("RBR_CLASSIC_OUT") {
            Some(out) => (PathBuf::from(out), true),
            None => (crate::test_dir("classic-real"), false),
        };
        let _ = fs::remove_dir_all(out.join(rbr_btb::player::TRACKS));
        let mut expected: HashMap<u32, &rsf::Stage> = HashMap::new();
        for stage in &picked {
            let known = Known { numbers: vec![stage.id], ..Known::default() };
            let done = add(&out, source::open(&stage.folder).unwrap(), &known);
            assert!(done.len() == 1 && done[0].ok(), "{}: {done:?}", stage.folder.display());
            let Some(Goto::Stage(folder)) = &done[0].goto else { panic!("{done:?}") };
            let number: u32 = folder.split('-').next().unwrap().parse().unwrap();
            assert!(number >= write::FIRST && expected.insert(number, stage).is_none(), "{folder}");
        }
        let mut catalog = crate::catalog::Catalog::default();
        catalog.add_classic_stages(&out.join(rbr_btb::player::TRACKS));
        assert_eq!(catalog.maps.len(), picked.len());
        for map in &catalog.maps {
            let crate::catalog::Origin::Community(added) = &map.origin else { panic!("{:?}", map.number) };
            let was = expected[&map.number];
            assert_eq!(added.problem, None, "{}", added.folder.display());
            assert_eq!(
                (
                    &added.variants,
                    added.weathers.len(),
                    added.surface,
                    &added.name,
                    &added.author,
                    added.length,
                    &added.country
                ),
                (&was.variants, was.weathers.len(), was.surface, &was.name, &was.author, was.length, &was.country),
                "{}",
                was.folder.display()
            );
            assert_eq!(added.texture_map().unwrap().pairs().count(), was.texture_map().unwrap().pairs().count());
            assert_eq!(map.stages.len(), was.variants.len());
        }
        eprintln!("added {} stage folders into {}", picked.len(), out.display());
        if !keep {
            fs::remove_dir_all(&out).unwrap();
        }
    }

    /// The RBR Czech installers of `RBR_INSTALLERS` (`*.exe`), each added into a temporary player folder: every stage
    /// is either added and opens, or refused as encrypted. Skips until a source reads `.exe` files.
    #[test]
    fn installers_add_or_are_refused_as_encrypted() {
        let Some(dir) = std::env::var_os("RBR_INSTALLERS").map(PathBuf::from) else {
            eprintln!("skipping: RBR_INSTALLERS not set");
            return;
        };
        let mut exes: Vec<PathBuf> = fs::read_dir(&dir).unwrap().filter_map(|e| Some(e.ok()?.path())).collect();
        exes.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")));
        exes.sort();
        for exe in exes {
            let src = match source::open(&exe) {
                Ok(src) => src,
                Err(why) if why.starts_with("installers can't be read") => {
                    eprintln!("skipping {}: {why}", exe.display());
                    continue;
                }
                Err(why) => panic!("{}: {why}", exe.display()),
            };
            let home = crate::test_dir("classic-installer");
            let done = add(&home, src, &Known::default());
            for outcome in &done {
                eprintln!("{}: {}", exe.display(), outcome.line());
                assert!(outcome.ok() || outcome.line().ends_with(ENCRYPTED), "{}: {}", exe.display(), outcome.line());
            }
            assert!(!done.is_empty());
            for stage in rsf::scan_maps(&home.join(rbr_btb::player::TRACKS), &Default::default()) {
                assert_eq!(stage.unwrap().problem, None);
            }
            fs::remove_dir_all(&home).unwrap();
        }
    }
}
