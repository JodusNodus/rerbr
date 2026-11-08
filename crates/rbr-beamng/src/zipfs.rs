//! A level zip as a file system: entries found by their path ignoring case and a leading slash, as BeamNG resolves
//! `/levels/...` names.

use rbr_formats::rbz;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub struct Archive {
    zip: rbz::ZipArchive<BufReader<File>>,
    /// Lower-case path to the entry's name.
    names: HashMap<String, String>,
}

fn key(path: &str) -> String {
    path.trim_start_matches('/').replace('\\', "/").to_lowercase()
}

impl Archive {
    pub fn open(path: &Path) -> Result<Archive, String> {
        let zip = rbz::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let names = zip.file_names().flatten().map(|n| (key(&n), n.into_owned())).collect();
        Ok(Archive { zip, names })
    }

    /// Every entry name, in archive order.
    pub fn names(&self) -> Vec<String> {
        self.zip.file_names().flatten().map(|n| n.into_owned()).collect()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.names.contains_key(&key(path))
    }

    /// The entry's archive name for a level path.
    pub fn find(&self, path: &str) -> Option<&str> {
        self.names.get(&key(path)).map(String::as_str)
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let name = self.find(path).ok_or_else(|| format!("{path}: not in the level zip"))?.to_owned();
        let mut out = Vec::new();
        rbz::by_name(&mut self.zip, &name)
            .map_err(|e| format!("{name}: {e}"))?
            .read_to_end(&mut out)
            .map_err(|e| format!("{name}: {e}"))?;
        Ok(out)
    }

    pub fn read_text(&mut self, path: &str) -> Result<String, String> {
        Ok(String::from_utf8_lossy(&self.read(path)?).into_owned())
    }
}
