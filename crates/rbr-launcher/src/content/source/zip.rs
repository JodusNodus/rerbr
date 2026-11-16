//! A .zip: its files from its listing, copied out by `crate::archive`.

use std::path::{Path, PathBuf};

use super::{Entry, Fail, Source, Wanted};
use crate::archive::{extract_zip, zip_entries};

pub struct Zip {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Zip {
    fn open(path: PathBuf) -> Result<Zip, Fail> {
        let listed = zip_entries(&path).map_err(|e| format!("can't read it: {e}"))?;
        let entries = listed
            .into_iter()
            .filter(|e| !e.dir)
            .filter_map(|e| Some(Entry { path: e.parts?, size: e.size }))
            .collect();
        Ok(Zip { path, entries })
    }
}

impl Source for Zip {
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
        extract_zip(&self.path, dest, wanted, progress)
    }
}

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    let named = path.to_string_lossy().to_ascii_lowercase().ends_with(".zip");
    named.then(|| Ok(Box::new(Zip::open(path.to_owned())?) as Box<dyn Source>))
}
