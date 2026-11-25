//! A stage's `Objects.zip` and its `objectlist.ini`.
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

/// The `.xform` of an object that is not moved or instanced (a 4×4 matrix).
const IDENTITY: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

pub struct Objects {
    zip: zip::ZipWriter<std::fs::File>,
    ini: String,
    count: usize,
    /// Starts the meshes' file names: `Objects/<prefix>_<n>.x`.
    prefix: &'static str,
}

impl Objects {
    /// A new archive at `path`, holding the `identity` transform.
    pub fn new(path: &Path, prefix: &'static str) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut out = Self { zip: zip::ZipWriter::new(file), ini: String::new(), count: 0, prefix };
        out.put("Objects/identity.xform", &IDENTITY.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>())?;
        Ok(out)
    }

    pub fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.zip.start_file(name, deflated()).map_err(|e| e.to_string())?;
        self.zip.write_all(bytes).map_err(|e| e.to_string())
    }

    /// One mesh `.x`, placed by the `.xform` `xform` (already [`put`](Self::put); `identity` is always there), with the
    /// `objectlist.ini` keys `entries`.
    pub fn add(&mut self, mesh: &[u8], xform: &str, entries: &str) -> Result<(), String> {
        let i = self.count;
        self.put(&format!("Objects/{}_{i}.x", self.prefix), mesh)?;
        self.list(i, xform, entries);
        self.count += 1;
        Ok(())
    }

    /// Adds the meshes `mesh` makes of `batch`, as `(its .x bytes, its objectlist keys)`, in order. The bytes are built
    /// and deflated on every core, each as a one-entry archive that is copied in without recompressing: deflate is
    /// most of an import's time.
    pub fn add_all<T: Sync>(
        &mut self,
        batch: &[T],
        xform: &str,
        mesh: impl Fn(&T) -> (Vec<u8>, String) + Sync,
    ) -> Result<(), String> {
        let (first, prefix) = (self.count, self.prefix);
        let threads = std::thread::available_parallelism().map_or(4, usize::from).min(batch.len().max(1));
        let made = std::thread::scope(|scope| {
            let mesh = &mesh;
            let workers: Vec<_> = (0..threads)
                .map(|t| {
                    scope.spawn(move || {
                        (t..batch.len())
                            .step_by(threads)
                            .map(|i| {
                                let (bytes, entries) = mesh(&batch[i]);
                                (i, archive(&format!("Objects/{prefix}_{}.x", first + i), &bytes), entries)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut all: Vec<_> = workers.into_iter().flat_map(|w| w.join().unwrap()).collect();
            all.sort_by_key(|(i, ..)| *i);
            all
        });
        for (i, archive, entries) in made {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive?)).map_err(|e| e.to_string())?;
            self.zip.raw_copy_file(archive.by_index_raw(0).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            self.list(first + i, xform, &entries);
        }
        self.count += batch.len();
        Ok(())
    }

    fn list(&mut self, i: usize, xform: &str, entries: &str) {
        let _ = writeln!(
            self.ini,
            "\n[Object_{i}]\nfile={}_{i}\nxform={xform}\ncloneCount=1\nDrawInstanced=0\nMoveable=0\n{entries}",
            self.prefix
        );
    }

    /// Closes the archive; the `objectlist.ini` text.
    pub fn finish(self) -> Result<String, String> {
        self.zip.finish().map_err(|e| e.to_string())?;
        Ok(format!("[OBJECTS]\ncount={}\n{}", self.count, self.ini))
    }
}

fn deflated() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

/// `bytes` as the only entry of a zip held in memory.
fn archive(name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(name, deflated()).map_err(|e| e.to_string())?;
    zip.write_all(bytes).map_err(|e| e.to_string())?;
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}
