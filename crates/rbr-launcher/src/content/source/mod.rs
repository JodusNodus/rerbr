//! What an item is made of: a folder, a zip, a 7z or a plain file, each as one virtual file tree. A kind of content
//! ([`super::kind`]) reads the tree's names and asks the source to copy out what it wants. One module per container;
//! [`open`] picks it.

use std::path::{Path, PathBuf};

mod file;
mod folder;
mod sevenz;
mod zip;

type Fail = String;

/// A file of the tree: its path parts below the item's top and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: Vec<String>,
    pub size: u64,
}

/// Why an item cannot be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

/// Paths to copy: the path in the tree, and the path below the destination.
pub type Wanted = [(Vec<String>, Vec<String>)];

/// One dropped or picked item as a file tree.
pub trait Source {
    /// The item's real path.
    fn path(&self) -> &Path;
    /// The item's file or folder name.
    fn name(&self) -> String {
        self.path().file_name().map_or_else(|| self.path().display().to_string(), |n| n.to_string_lossy().into_owned())
    }
    /// Whether the item is a folder.
    fn is_dir(&self) -> bool {
        false
    }
    /// The files, with safe paths.
    fn entries(&self) -> &[Entry];
    /// Copies `wanted` into `dest`; `progress` gets each file's bytes and stops the copy by failing.
    fn extract(
        &self,
        wanted: &Wanted,
        dest: &Path,
        progress: &mut dyn FnMut(u64) -> Result<(), Fail>,
    ) -> Result<(), Fail>;
    /// Facts the container carries beside its files (an installer's writes): none yet.
    #[allow(dead_code)]
    fn facts(&self) -> Option<&[(String, String)]> {
        None
    }
}

/// The paths of the entries, `/`-joined.
pub fn names(source: &dyn Source) -> Vec<String> {
    source.entries().iter().map(|e| e.path.join("/")).collect()
}

/// Opens `path` as the container its kind says: a folder, a `.zip`, a `.7z`; any other file as a tree of no files.
pub fn open(path: &Path) -> Result<Box<dyn Source>, Refusal> {
    if !path.exists() {
        return Err(Refusal("it doesn't exist".into()));
    }
    let lower = path.to_string_lossy().to_ascii_lowercase();
    let path: PathBuf = path.to_owned();
    if path.is_dir() {
        Ok(Box::new(folder::Folder::open(path)))
    } else if lower.ends_with(".zip") {
        Ok(Box::new(zip::Zip::open(path).map_err(Refusal)?))
    } else if lower.ends_with(".7z") {
        Ok(Box::new(sevenz::Seven::open(path).map_err(Refusal)?))
    } else {
        Ok(Box::new(file::File { path }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::kind::btb::tests::write;

    #[test]
    fn each_container_lists_its_files_and_a_plain_file_has_none() {
        let dir = crate::test_dir("content-source");
        write(&dir.join("f/a/b.txt"), b"abc");
        let folder = open(&dir.join("f")).unwrap();
        assert!(folder.is_dir());
        assert_eq!(folder.entries(), [Entry { path: vec!["a".into(), "b.txt".into()], size: 3 }]);
        let zip = dir.join("z.zip");
        let mut w = ::zip::ZipWriter::new(std::fs::File::create(&zip).unwrap());
        w.start_file("x/y.txt", ::zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut w, b"hi").unwrap();
        w.finish().unwrap();
        assert_eq!(open(&zip).unwrap().entries()[0].path, ["x", "y.txt"]);
        write(&dir.join("RBR.iso"), b"iso");
        assert!(open(&dir.join("RBR.iso")).unwrap().entries().is_empty());
        assert_eq!(open(&dir.join("none.7z")).err(), Some(Refusal("it doesn't exist".into())));
        write(&dir.join("bad.7z"), b"not a 7z");
        assert!(open(&dir.join("bad.7z")).err().is_some_and(|r| r.0.starts_with("can't read it")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// A tree of names only, for the kinds' tests.
#[cfg(test)]
pub struct Fake {
    pub path: PathBuf,
    pub dir: bool,
    pub entries: Vec<Entry>,
}

#[cfg(test)]
impl Fake {
    /// An item called `name` (a folder when `dir`) holding the files `files`.
    pub fn new(name: &str, dir: bool, files: &[&str]) -> Fake {
        let entries =
            files.iter().map(|f| Entry { path: f.split(['/', '\\']).map(str::to_owned).collect(), size: 0 }).collect();
        Fake { path: PathBuf::from(name), dir, entries }
    }
}

#[cfg(test)]
impl Source for Fake {
    fn path(&self) -> &Path {
        &self.path
    }

    fn is_dir(&self) -> bool {
        self.dir
    }

    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn extract(&self, _: &Wanted, _: &Path, _: &mut dyn FnMut(u64) -> Result<(), Fail>) -> Result<(), Fail> {
        Err("a fake".into())
    }
}
