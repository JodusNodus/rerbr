//! A .rar: its files from its listing, copied out by `crate::archive` (RAR 1.3 to 7, one volume, not encrypted).

use std::path::{Path, PathBuf};

use super::{Entry, Fail, Source, Wanted};
use crate::archive::{extract_rar, rar_entries};

pub struct Rar {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Rar {
    fn open(path: PathBuf) -> Result<Rar, Fail> {
        let listed = rar_entries(&path).map_err(|e| format!("can't read it: {e}"))?;
        let entries = listed
            .into_iter()
            .filter(|e| !e.dir)
            .filter_map(|e| Some(Entry { path: e.parts?, size: e.size }))
            .collect();
        Ok(Rar { path, entries })
    }
}

impl Source for Rar {
    fn path(&self) -> &Path {
        &self.path
    }

    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn extract(
        &self,
        wanted: &Wanted,
        dest: &Path,
        progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
    ) -> Result<(), Fail> {
        extract_rar(&self.path, dest, wanted, progress)
    }
}

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    let named = !path.is_dir() && path.to_string_lossy().to_ascii_lowercase().ends_with(".rar");
    named.then(|| Ok(Box::new(Rar::open(path.to_owned())?) as Box<dyn Source>))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solid_rar_lists_its_files_and_extracts_only_the_wanted_ones() {
        let dir = crate::test_dir("content-rar");
        let mut builder = rars::Builder::new(rars::ArchiveVersion::Rar50).solid(true);
        let text = "track data ".repeat(5000).into_bytes();
        for (name, bytes) in [("T/a.kn5", &text[..]), ("T/data/surfaces.ini", b"[SURFACE_0]"), ("T/skip.bin", b"no")] {
            builder.add_bytes(name.as_bytes().to_vec(), bytes.to_vec(), None, None).unwrap();
        }
        std::fs::write(dir.join("t.rar"), builder.to_bytes().unwrap()).unwrap();
        let source = open(&dir.join("t.rar")).unwrap().unwrap();
        let names: Vec<_> = source.entries().iter().map(|e| e.path.join("/")).collect();
        assert_eq!(names, ["T/a.kn5", "T/data/surfaces.ini", "T/skip.bin"]);
        let wanted = [
            (vec!["T".into(), "a.kn5".into()], vec!["a.kn5".into()]),
            (vec!["T".into(), "data".into(), "surfaces.ini".into()], vec!["data".into(), "surfaces.ini".into()]),
        ];
        let mut bytes = 0;
        source
            .extract(&wanted, &dir.join("out"), &mut |n| {
                bytes += n;
                Ok(())
            })
            .unwrap();
        assert_eq!(std::fs::read(dir.join("out/a.kn5")).unwrap(), text);
        assert_eq!(std::fs::read(dir.join("out/data/surfaces.ini")).unwrap(), b"[SURFACE_0]");
        assert!(!dir.join("out/skip.bin").exists());
        assert_eq!(bytes, (text.len() + 11) as u64);
        // A cancel stops it.
        let stopped = source.extract(&wanted, &dir.join("out2"), &mut |_| Err("cancelled".into()));
        assert_eq!(stopped, Err("cancelled".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
