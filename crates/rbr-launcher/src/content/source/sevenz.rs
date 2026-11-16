//! A .7z: its files from its listing, copied out by `crate::archive`.

use std::path::{Path, PathBuf};

use super::{Entry, Fail, Source, Wanted};
use crate::archive::{extract_seven, seven_entries};

pub struct Seven {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Seven {
    fn open(path: PathBuf) -> Result<Seven, Fail> {
        let listed = seven_entries(&path).map_err(|e| format!("can't read it: {e}"))?;
        let entries = listed
            .into_iter()
            .filter(|e| !e.dir)
            .filter_map(|e| Some(Entry { path: e.parts?, size: e.size }))
            .collect();
        Ok(Seven { path, entries })
    }
}

impl Source for Seven {
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
        extract_seven(&self.path, dest, wanted, progress)
    }
}

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    let named = path.to_string_lossy().to_ascii_lowercase().ends_with(".7z");
    named.then(|| Ok(Box::new(Seven::open(path.to_owned())?) as Box<dyn Source>))
}
