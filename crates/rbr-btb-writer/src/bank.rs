//! A stage's `Textures.zip`: files stored once however often they are put, named in the order they arrive.
use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;

pub struct TextureZip {
    zip: zip::ZipWriter<std::fs::File>,
    /// Hash buckets retain output filenames; a hash match is checked against the written payload.
    seen: BTreeMap<(u64, usize), Vec<(String, Vec<u8>)>>,
    /// The files written.
    pub count: usize,
}

impl TextureZip {
    pub fn new(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        Ok(Self { zip: zip::ZipWriter::new(file), seen: BTreeMap::new(), count: 0 })
    }

    /// Stores `bytes` as `Textures/ac_<n>.<extension>` unless the same bytes are in already; the file's name.
    pub fn put(&mut self, bytes: Vec<u8>, extension: &str) -> Result<String, String> {
        let mut hash = DefaultHasher::new();
        bytes.hash(&mut hash);
        let key = (hash.finish(), bytes.len());
        if let Some((file, _)) = self.seen.get(&key).and_then(|bucket| bucket.iter().find(|(_, b)| *b == bytes)) {
            return Ok(file.clone());
        }
        let filename = format!("ac_{:05}.{extension}", self.count);
        self.zip
            .start_file(
                format!("Textures/{filename}"),
                zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .map_err(|e| e.to_string())?;
        self.zip.write_all(&bytes).map_err(|e| e.to_string())?;
        self.seen.entry(key).or_default().push((filename.clone(), bytes));
        self.count += 1;
        Ok(filename)
    }

    pub fn finish(self) -> Result<(), String> {
        self.zip.finish().map(|_| ()).map_err(|e| e.to_string())
    }
}
