//! What an item is made of: a folder, a zip, a 7z or a plain file, each as one virtual file tree. A kind of content
//! ([`super::kind`]) reads the tree's names and asks the source to copy out what it wants. One module per container;
//! [`open`] tries each in [`SOURCES`], so a container is one module and one line there.

use std::path::{Path, PathBuf};

mod file;
mod folder;
mod nsis;
mod packed;
pub mod refused;
mod split7z;

type Fail = String;

/// A file of the tree: its path parts below the item's top and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: Vec<String>,
    pub size: u64,
}

/// A source's try at a path: `None` when the path is not its kind of container, else the opened tree or why it
/// cannot be opened.
type Try = fn(&Path) -> Option<Result<Box<dyn Source>, Fail>>;

/// The containers, in the order they are tried; the last takes any file.
const SOURCES: &[Try] = &[
    nsis::open,
    refused::open,
    folder::open,
    packed::open_zip,
    packed::open_seven,
    packed::open_rar,
    split7z::open,
    file::open,
];

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
    /// One file's bytes: by default copied out through [`Source::extract`] into a temp folder; a source that reads a
    /// file directly overrides it.
    #[allow(dead_code)]
    fn read(&self, entry: &Entry) -> Result<Vec<u8>, Fail> {
        let temp = crate::job::Temp::create(&std::env::temp_dir(), "read")?;
        let wanted = [(entry.path.clone(), entry.path.clone())];
        self.extract(&wanted, &temp.0, &mut |_| Ok(()))?;
        let file = entry.path.iter().fold(temp.0.clone(), |p, part| p.join(part));
        std::fs::read(&file).map_err(|e| format!("{}: {e}", entry.path.join("/")))
    }
    /// The INI writes the container makes besides its files (an installer's), in script order; none for plain
    /// containers.
    #[allow(dead_code)]
    fn ini_writes(&self) -> &[IniWrite] {
        &[]
    }
}

/// One INI write an installer makes: the file (path parts below the install folder, as ["Maps", "Tracks.ini"]), the
/// section, the key and the value, with the installer's variables resolved where they are constants; one it cannot
/// resolve stays as `${name}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct IniWrite {
    pub file: Vec<String>,
    pub section: String,
    pub key: String,
    pub value: String,
}

/// The paths of the entries, `/`-joined.
pub fn names(source: &dyn Source) -> Vec<String> {
    source.entries().iter().map(|e| e.path.join("/")).collect()
}

/// The item `path` belongs to: a split archive's part names its first part, so dropping every part adds it once.
pub fn item(path: &Path) -> PathBuf {
    split7z::first_part(path).filter(|first| first.exists()).unwrap_or_else(|| path.to_owned())
}

/// Opens `path` with the first of [`SOURCES`] that takes it, or says why it cannot be opened.
pub fn open(path: &Path) -> Result<Box<dyn Source>, Fail> {
    if !path.exists() {
        return Err("it doesn't exist".into());
    }
    SOURCES.iter().find_map(|source| source(path)).unwrap_or_else(|| Err("not recognised".into()))
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
        assert_eq!(open(&dir.join("none.7z")).err().as_deref(), Some("it doesn't exist"));
        write(&dir.join("x.rar"), b"Rar!");
        assert!(open(&dir.join("x.rar")).err().is_some_and(|r| r.starts_with("can't read it")));
        write(&dir.join("bad.7z"), b"not a 7z");
        assert!(open(&dir.join("bad.7z")).err().is_some_and(|r| r.starts_with("can't read it")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
