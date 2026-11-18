//! The downloads' archives (the car packs' zips and 7z's, the co-driver packs', NGP's own package): their file
//! entries' safe paths below the top, and extracting the wanted entries into a folder. Every entry path goes
//! through [`safe_path`], so an archive cannot escape the folder it is written to.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

use rbr_formats::rbz;

/// A failed listing or extraction's message, without the archive's path (each caller names it as its messages do).
type Fail = String;

/// An archive entry's path (`\` or `/` separated) as safe components below the folder it is written to, or `None`.
pub fn safe_path(name: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = name.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
    let bad = |p: &&str| *p == ".." || p.contains(':');
    (!parts.is_empty() && !parts.iter().any(bad)).then_some(parts)
}

/// One folder name of ours, from untrusted text: no separators, not hidden.
pub fn safe_folder(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && !name.starts_with('.') && safe_path(name)?.len() == 1).then(|| name.to_owned())
}

/// [`safe_path`] as owned parts.
fn owned_path(name: &str) -> Option<Vec<String>> {
    safe_path(name).map(|p| p.into_iter().map(str::to_owned).collect())
}

/// One file or folder entry of a zip or 7z download: its raw name, its safe path parts below the top (`None` when
/// the path is unsafe), and its size (folders: 0).
pub struct Entry {
    pub name: String,
    pub parts: Option<Vec<String>>,
    pub size: u64,
    pub dir: bool,
}

/// Opens the 7z at `path` for reading its entries.
pub(crate) fn open_seven(path: &Path) -> Result<sevenz_rust2::ArchiveReader<BufReader<File>>, Fail> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    sevenz_rust2::ArchiveReader::new(BufReader::new(file), sevenz_rust2::Password::empty())
        .map_err(|e| format!("not a .7z: {e}"))
}

/// The zip's entries.
pub fn zip_entries(path: &Path) -> Result<Vec<Entry>, Fail> {
    let mut zip = rbz::open(path).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for k in 0..zip.len() {
        let name = zip.name_for_index(k).and_then(Result::ok).ok_or("a bad entry")?.into_owned();
        let dir = name.ends_with(['/', '\\']);
        let parts = owned_path(&name);
        let size = match dir {
            true => 0,
            false => zip.by_index_raw(k).map_err(|e| e.to_string())?.size(),
        };
        entries.push(Entry { name, parts, size, dir });
    }
    Ok(entries)
}

/// The 7z's entries.
pub fn seven_entries(path: &Path) -> Result<Vec<Entry>, Fail> {
    let archive = open_seven(path)?;
    Ok(archive
        .archive()
        .files
        .iter()
        .map(|f| Entry { name: f.name.clone(), parts: owned_path(&f.name), size: f.size, dir: f.is_directory })
        .collect())
}

/// The rar's file entries (a rar's names are bytes: invalid UTF-8 is replaced).
pub fn rar_entries(path: &Path) -> Result<Vec<Entry>, Fail> {
    let archive = rars::ArchiveReader::read_path(path).map_err(|e| e.to_string())?;
    Ok(archive
        .members()
        .map(|m| {
            let name = m.meta.name_lossy();
            Entry { parts: owned_path(&name), name, size: m.meta.unpacked_size, dir: m.meta.is_directory }
        })
        .collect())
}

/// A file being written whose bytes are counted where the extraction can read them.
struct Counted {
    out: io::BufWriter<File>,
    bytes: std::rc::Rc<std::cell::Cell<u64>>,
}

impl Write for Counted {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.out.write(buf)?;
        self.bytes.set(self.bytes.get() + n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Extracts the rar's `wanted` into `dest`, as [`extract_zip`], but `progress` gets the bytes of each file as the next
/// one starts (a rar decodes its files in order, solid ones all the way through, so the others are decoded and dropped).
pub fn extract_rar(
    src: &Path,
    dest: &Path,
    wanted: &[(Vec<String>, Vec<String>)],
    progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
) -> Result<(), Fail> {
    let error = |e: String| format!("{}: {e}", src.display());
    let archive = rars::ArchiveReader::read_path(src).map_err(|e| error(e.to_string()))?;
    progress(0)?;
    let written = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut stopped: Option<Fail> = None;
    let result = archive.extract_to(None, |meta| {
        if let Err(e) = progress(written.take()) {
            stopped = Some(e.clone());
            return Err(rars::Error::from(io::Error::other(e)));
        }
        let wanted = owned_path(&meta.name_lossy()).and_then(|parts| wanted.iter().find(|(from, _)| *from == parts));
        match wanted.filter(|_| !meta.is_directory) {
            None => Ok(Box::new(io::sink())),
            Some((_, to)) => {
                let path = to.iter().fold(dest.to_owned(), |p, part| p.join(part));
                fs::create_dir_all(path.parent().unwrap_or(dest))?;
                let out = io::BufWriter::new(File::create(&path)?);
                Ok(Box::new(Counted { out, bytes: written.clone() }))
            }
        }
    });
    match (stopped, result) {
        (Some(e), _) => Err(e),
        (None, Err(e)) => Err(error(e.to_string())),
        (None, Ok(())) => progress(written.take()),
    }
}

/// Writes `reader` to `dest`/`parts`, making its folders.
fn write_file(dest: &Path, parts: &[String], reader: &mut dyn Read) -> io::Result<u64> {
    let path = parts.iter().fold(dest.to_owned(), |p, part| p.join(part));
    fs::create_dir_all(path.parent().unwrap_or(dest))?;
    let mut out = io::BufWriter::new(File::create(&path)?);
    let n = io::copy(reader, &mut out)?;
    out.flush()?;
    Ok(n)
}

/// Extracts the zip's `wanted` (archive path to below-`dest` path) into `dest`. `progress` gets each file's bytes
/// (and none before the archive is open); it stops the extraction by failing, as on cancel.
pub fn extract_zip(
    src: &Path,
    dest: &Path,
    wanted: &[(Vec<String>, Vec<String>)],
    progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
) -> Result<(), Fail> {
    let error = |e: String| format!("{}: {e}", src.display());
    let mut zip = rbz::open(src).map_err(|e| error(e.to_string()))?;
    progress(0)?;
    for k in 0..zip.len() {
        let Some(parts) = zip.name_for_index(k).and_then(Result::ok).and_then(|n| owned_path(&n)) else { continue };
        let Some((_, to)) = wanted.iter().find(|(from, _)| *from == parts) else { continue };
        let mut file = zip.by_index(k).map_err(|e| error(e.to_string()))?;
        let n = write_file(dest, to, &mut file).map_err(|e| format!("{}: {e}", to.join("/")))?;
        progress(n)?;
    }
    Ok(())
}

/// Extracts the 7z's `wanted` into `dest`, as [`extract_zip`].
pub fn extract_seven(
    src: &Path,
    dest: &Path,
    wanted: &[(Vec<String>, Vec<String>)],
    progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
) -> Result<(), Fail> {
    let error = |e: String| format!("{}: {e}", src.display());
    let mut archive = open_seven(src).map_err(error)?;
    progress(0)?;
    let mut failed: Option<Fail> = None;
    archive
        .for_each_entries(|entry, reader| {
            if failed.is_some() {
                return Ok(false);
            }
            if entry.is_directory() {
                return Ok(true);
            }
            let Some(parts) = owned_path(entry.name()) else { return Ok(true) };
            let Some((_, to)) = wanted.iter().find(|(from, _)| *from == parts) else { return Ok(true) };
            match write_file(dest, to, reader) {
                Err(e) => {
                    failed = Some(format!("{}: {e}", to.join("/")));
                    Ok(false)
                }
                Ok(n) => match progress(n) {
                    Ok(()) => Ok(true),
                    Err(e) => {
                        failed = Some(e);
                        Ok(false)
                    }
                },
            }
        })
        .map_err(|e| error(e.to_string()))?;
    failed.map_or(Ok(()), Err)
}
