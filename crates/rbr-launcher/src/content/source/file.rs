//! A plain file that is no archive (an `.iso`, a `.rar`, an installer): a tree of no files, known by its name.

use std::path::{Path, PathBuf};

use super::{Entry, Fail, Source, Wanted};

pub struct File {
    pub path: PathBuf,
}

impl Source for File {
    fn path(&self) -> &Path {
        &self.path
    }

    fn entries(&self) -> &[Entry] {
        &[]
    }

    fn extract(&self, _: &Wanted, _: &Path, _: &mut dyn FnMut(u64) -> Result<(), Fail>) -> Result<(), Fail> {
        Err(format!("{} holds no files to copy", self.path.display()))
    }
}

pub fn open(path: &Path) -> Option<Result<Box<dyn Source>, Fail>> {
    Some(Ok(Box::new(File { path: path.to_owned() })))
}
