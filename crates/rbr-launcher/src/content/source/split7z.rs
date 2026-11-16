//! A 7-Zip archive split into `name.7z.001`, `.002`, ...: the parts, which are plain byte splits, read in order as
//! one archive. Opening any part opens them all. The archive is then read by the plain 7z source
//! ([`super::sevenz`]), from the parts joined into a temp file.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use super::{Entry, Fail, Source, Wanted};
use crate::job::Temp;

pub struct Split {
    path: PathBuf,
    inner: Box<dyn Source>,
    _temp: Temp,
}

/// `name.7z.001`'s `name.7z` and the part's number, or `None` when the file is no part.
fn part_of(path: &Path) -> Option<(PathBuf, u32)> {
    let name = path.file_name()?.to_str()?;
    let (stem, number) = name.rsplit_once('.')?;
    let ok =
        number.len() >= 3 && number.bytes().all(|b| b.is_ascii_digit()) && stem.to_ascii_lowercase().ends_with(".7z");
    ok.then(|| Some((path.with_file_name(stem), number.parse().ok()?)))?
}

/// The numbers of the parts of `stem` found beside it.
fn parts_of(stem: &Path) -> Vec<u32> {
    let (Some(dir), Some(name)) = (stem.parent(), stem.file_name().and_then(|n| n.to_str())) else { return Vec::new() };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    read.filter_map(|e| part_of(&e.ok()?.path()).filter(|(s, _)| s.file_name().and_then(|n| n.to_str()) == Some(name)))
        .map(|(_, n)| n)
        .collect()
}

impl Split {
    fn open(path: &Path, stem: &Path, width: usize) -> Result<Split, Fail> {
        let found = parts_of(stem);
        let last = found.iter().copied().max().unwrap_or(0);
        if let Some(missing) = (1..=last).find(|n| !found.contains(n)) {
            return Err(format!("part {missing} is missing"));
        }
        let temp = Temp::create(&std::env::temp_dir(), "split7z")?;
        let joined = temp.0.join("joined.7z");
        let mut out = io::BufWriter::new(File::create(&joined).map_err(|e| e.to_string())?);
        let name = stem.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        for n in 1..=last {
            let part = stem.with_file_name(format!("{name}.{n:0width$}"));
            let mut file = File::open(&part).map_err(|e| format!("{}: {e}", part.display()))?;
            io::copy(&mut file, &mut out).map_err(|e| format!("{}: {e}", part.display()))?;
        }
        io::Write::flush(&mut out).map_err(|e| e.to_string())?;
        drop(out);
        let inner = super::sevenz::open(&joined).ok_or("not recognised")??;
        Ok(Split { path: path.to_owned(), inner, _temp: temp })
    }
}

impl Source for Split {
    fn path(&self) -> &Path {
        &self.path
    }

    fn name(&self) -> String {
        part_of(&self.path)
            .and_then(|(stem, _)| stem.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default()
    }

    fn entries(&self) -> &[Entry] {
        self.inner.entries()
    }

    fn extract(
        &self,
        wanted: &Wanted,
        dest: &Path,
        progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
    ) -> Result<(), Fail> {
        self.inner.extract(wanted, dest, progress)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, Fail> {
        self.inner.read(entry)
    }
}

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    let (stem, _) = part_of(path)?;
    let width = path.extension()?.len();
    Some(Split::open(path, &stem, width).map(|s| Box::new(s) as Box<dyn Source>))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::source::open as open_source;

    /// A 7z of two files, cut into `parts` pieces `dir/mod.7z.001`, ...
    fn split(dir: &Path, parts: usize) -> Vec<PathBuf> {
        let whole = dir.join("whole.7z");
        let mut out = sevenz_rust2::ArchiveWriter::create(&whole).unwrap();
        let big: Vec<u8> = (0..5000u32).map(|n| (n.wrapping_mul(2654435761) >> 13) as u8).collect();
        for (name, bytes) in [("Maps/a.bin", big.as_slice()), ("b.txt", b"bee".as_slice())] {
            out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(bytes)).unwrap();
        }
        out.finish().unwrap();
        let bytes = fs::read(&whole).unwrap();
        fs::remove_file(whole).unwrap();
        let size = bytes.len().div_ceil(parts);
        (1..=parts)
            .zip(bytes.chunks(size))
            .map(|(n, chunk)| {
                let part = dir.join(format!("mod.7z.{n:03}"));
                fs::write(&part, chunk).unwrap();
                part
            })
            .collect()
    }

    #[test]
    fn any_part_opens_all_of_them_as_one_archive() {
        let dir = crate::test_dir("split7z");
        let parts = split(&dir, 3);
        assert_eq!(parts.len(), 3);
        for part in &parts {
            let source = open_source(part).unwrap();
            let mut names = crate::content::source::names(&*source);
            names.sort();
            assert_eq!(names, ["Maps/a.bin", "b.txt"], "{}", part.display());
            assert_eq!(source.name(), "mod.7z");
            let entry = source.entries().iter().find(|e| e.path == ["b.txt"]).unwrap();
            assert_eq!(source.read(entry).unwrap(), b"bee");
        }
        let out = dir.join("out");
        let source = open_source(&parts[1]).unwrap();
        source
            .extract(&[(vec!["Maps".into(), "a.bin".into()], vec!["x".into(), "a.bin".into()])], &out, &mut |_| Ok(()))
            .unwrap();
        assert_eq!(fs::read(out.join("x/a.bin")).unwrap().len(), 5000);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_part_is_named() {
        let dir = crate::test_dir("split7z-missing");
        let parts = split(&dir, 3);
        fs::remove_file(&parts[1]).unwrap();
        assert_eq!(open_source(&parts[0]).err().as_deref(), Some("part 2 is missing"));
        assert_eq!(open_source(&parts[2]).err().as_deref(), Some("part 2 is missing"));
        fs::remove_file(&parts[0]).unwrap();
        assert_eq!(open_source(&parts[2]).err().as_deref(), Some("part 1 is missing"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
