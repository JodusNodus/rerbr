//! A folder: its files below it, links left out.

use std::fs;
use std::path::{Path, PathBuf};

use super::{Entry, Fail, Source, Wanted};
use crate::archive::join;

pub struct Folder {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Folder {
    fn open(path: PathBuf) -> Folder {
        let entries = crate::codriver_packs::walk(&path)
            .into_iter()
            .map(|(rel, file)| Entry {
                path: rel.split('/').map(str::to_owned).collect(),
                size: file.metadata().map_or(0, |m| m.len()),
            })
            .collect();
        Folder { path, entries }
    }
}

impl Source for Folder {
    fn path(&self) -> &Path {
        &self.path
    }

    fn is_dir(&self) -> bool {
        true
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
        for (from, to) in wanted {
            let source = join(&self.path, from);
            let target = join(dest, to);
            fs::create_dir_all(target.parent().unwrap_or(dest)).map_err(|e| format!("{}: {e}", target.display()))?;
            let n = fs::copy(&source, &target).map_err(|e| format!("{}: {e}", source.display()))?;
            progress(n)?;
        }
        Ok(())
    }
}

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    path.is_dir().then(|| Ok(Box::new(Folder::open(path.to_owned())) as Box<dyn Source>))
}
