//! A Nullsoft (NSIS) installer, read without running it: its script is parsed from the header and shown as the tree
//! it would write below `$INSTDIR`, and the `WriteINIStr`s it makes are listed by [`Source::ini_writes`].
//!
//! - [`container`]: the first header, the packed header and files (raw deflate, raw LZMA; bzip2 is refused), solid
//!   or one block per file.
//! - [`script`]: the strings table (NSIS 2 and 3, ANSI and Unicode) and the walk over the entries: `SetOutPath`,
//!   `File`, `StrCpy` literals (followed in script order, control flow ignored), `WriteINIStr`, and the `nsis7z`
//!   plugin's `Extract`/`ExtractWithDetails`.
//!
//! A `.7z` the script unpacks with `nsis7z` is taken out of the installer into a temp file and opened with the
//! plain 7z source ([`super::sevenz`]); its files appear in the folder the extract runs in, in place of the archive.
//! Files written to `$PLUGINSDIR`, `$TEMP` or any other place the walk cannot resolve are left out.
//!
//! An INI write with an empty key and value is the script deleting the section (`DeleteINISec`), as NSIS itself stores it.
//!
//! Sizes: a solid installer's are exact (the open reads the stream once); a non-solid one's are the stored size,
//! which for packed files is the packed size.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::{Entry, Fail, IniWrite, Source, Wanted};
use crate::job::Temp;
use container::Container;
use container::UNSUPPORTED;
use script::Header;

mod container;
mod script;
#[cfg(test)]
mod tests;

/// A file of the tree: its path and where its bytes are.
struct Item {
    path: Vec<String>,
    offset: u32,
}

/// A 7z the script unpacked: its files go below `into`.
struct Nested {
    into: Vec<String>,
    source: Box<dyn Source>,
}

pub struct Nsis {
    path: PathBuf,
    container: Container,
    items: Vec<Item>,
    nested: Vec<Nested>,
    entries: Vec<Entry>,
    ini: Vec<IniWrite>,
    /// Holds the nested archives' files.
    _temp: Temp,
}

fn write_to(dest: &Path, parts: &[String], reader: &mut dyn Read) -> Result<u64, Fail> {
    let path = parts.iter().fold(dest.to_owned(), |p, part| p.join(part));
    let error = |e: io::Error| format!("{}: {e}", parts.join("/"));
    fs::create_dir_all(path.parent().unwrap_or(dest)).map_err(error)?;
    let mut out = io::BufWriter::new(fs::File::create(&path).map_err(error)?);
    let n = io::copy(reader, &mut out).map_err(error)?;
    io::Write::flush(&mut out).map_err(error)?;
    Ok(n)
}

impl Nsis {
    fn open(path: &Path, container: Container) -> Result<Nsis, Fail> {
        let header = Header::new(&container.header)?;
        let script = script::walk(&header);
        // A path written twice keeps the later write's data, at the first one's place.
        let mut items: Vec<Item> = Vec::new();
        for file in &script.files {
            match items.iter_mut().find(|i| i.path == file.path) {
                Some(item) => item.offset = file.offset,
                None => items.push(Item { path: file.path.clone(), offset: file.offset }),
            }
        }
        // The archives the script unpacks with nsis7z leave the tree for their contents.
        let archives: Vec<(usize, &script::Extract)> =
            script.extracts.iter().filter_map(|e| Some((items.iter().position(|i| e.is(&i.path))?, e))).collect();
        let mut sizes: BTreeMap<u32, u64> = items.iter().map(|i| (i.offset, 0)).collect();
        let temp = Temp::create(&std::env::temp_dir(), "nsis")?;
        let packed: BTreeMap<u32, PathBuf> =
            archives.iter().enumerate().map(|(n, (i, _))| (items[*i].offset, temp.0.join(format!("{n}.7z")))).collect();
        let offsets: Vec<u32> = sizes.keys().copied().collect();
        container.files(&offsets, &mut |offset, reader| {
            let n = match packed.get(&offset) {
                Some(file) => {
                    let mut out = io::BufWriter::new(fs::File::create(file).map_err(|e| e.to_string())?);
                    io::copy(reader, &mut out).map_err(|e| e.to_string())?
                }
                // Reading a non-solid file's size out costs a whole unpack; its block's stored size does for it.
                None if !container.solid => 0,
                None => io::copy(reader, &mut io::sink()).map_err(|e| e.to_string())?,
            };
            sizes.insert(offset, n);
            Ok(())
        })?;
        if !container.solid {
            for (offset, size) in &mut sizes {
                *size = container.stored_size(*offset)?.unwrap_or(0);
            }
        }
        let mut nested = Vec::new();
        for (n, (i, extract)) in archives.iter().enumerate() {
            let file = temp.0.join(format!("{n}.7z"));
            let source = super::sevenz::open(&file)
                .and_then(Result::ok)
                .ok_or_else(|| format!("{UNSUPPORTED}: its archive {} can't be read", items[*i].path.join("/")))?;
            nested.push(Nested { into: extract.into.clone(), source });
        }
        let gone: Vec<usize> = archives.iter().map(|(i, _)| *i).collect();
        let kept: Vec<Item> =
            items.into_iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, i)| i).collect();
        let mut entries: Vec<Entry> = kept
            .iter()
            .map(|i| Entry { path: i.path.clone(), size: sizes.get(&i.offset).copied().unwrap_or(0) })
            .collect();
        for n in &nested {
            entries.extend(
                n.source
                    .entries()
                    .iter()
                    .map(|e| Entry { path: n.into.iter().chain(&e.path).cloned().collect(), size: e.size }),
            );
        }
        Ok(Nsis { path: path.to_owned(), container, items: kept, nested, entries, ini: script.ini, _temp: temp })
    }
}

impl Source for Nsis {
    fn path(&self) -> &Path {
        &self.path
    }

    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn ini_writes(&self) -> &[IniWrite] {
        &self.ini
    }

    fn extract(
        &self,
        wanted: &Wanted,
        dest: &Path,
        progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
    ) -> Result<(), Fail> {
        progress(0)?;
        let mut direct: BTreeMap<u32, Vec<&Vec<String>>> = BTreeMap::new();
        let mut inner: Vec<Vec<(Vec<String>, Vec<String>)>> = vec![Vec::new(); self.nested.len()];
        for (from, to) in wanted {
            if let Some(item) = self.items.iter().find(|i| i.path == *from) {
                direct.entry(item.offset).or_default().push(to);
            } else if let Some(n) = self.nested.iter().position(|n| from.starts_with(&n.into)) {
                inner[n].push((from[self.nested[n].into.len()..].to_vec(), to.clone()));
            } else {
                return Err(format!("{}: not in {}", from.join("/"), self.path.display()));
            }
        }
        let offsets: Vec<u32> = direct.keys().copied().collect();
        self.container.files(&offsets, &mut |offset, reader| {
            let targets = &direct[&offset];
            progress(write_to(dest, targets[0], reader)?)?;
            for to in &targets[1..] {
                let (from, to) = (join(dest, targets[0]), join(dest, to));
                fs::create_dir_all(to.parent().unwrap_or(dest)).map_err(|e| e.to_string())?;
                progress(fs::copy(from, to).map_err(|e| e.to_string())?)?;
            }
            Ok(())
        })?;
        for (nested, wanted) in self.nested.iter().zip(&inner) {
            if !wanted.is_empty() {
                nested.source.extract(wanted, dest, progress)?;
            }
        }
        Ok(())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, Fail> {
        if let Some(item) = self.items.iter().find(|i| i.path == entry.path) {
            let mut bytes = Vec::new();
            self.container.files(&[item.offset], &mut |_, reader| {
                reader.read_to_end(&mut bytes).map(drop).map_err(|e| e.to_string())
            })?;
            return Ok(bytes);
        }
        let n = self
            .nested
            .iter()
            .find(|n| entry.path.starts_with(&n.into))
            .ok_or_else(|| format!("{}: not in {}", entry.path.join("/"), self.path.display()))?;
        n.source.read(&Entry { path: entry.path[n.into.len()..].to_vec(), size: entry.size })
    }
}

fn join(dest: &Path, parts: &[String]) -> PathBuf {
    parts.iter().fold(dest.to_owned(), |p, part| p.join(part))
}

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    if path.is_dir() || !path.to_string_lossy().to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    match Container::open(path) {
        Ok(None) => None,
        Ok(Some(container)) => Some(Nsis::open(path, container).map(|n| Box::new(n) as Box<dyn Source>)),
        Err(e) => Some(Err(e)),
    }
}
