//! Downloading the texture upscaler from the Graphics page ("Download upscaler…") into the player's folder
//! (`rbr_render::upscale::DIR`), where the viewer's `--upscale-textures` finds it: `realesrgan-ncnn-vulkan` (MIT) from
//! Real-ESRGAN's v0.2.5.0 release for this platform, and Real-ESRGAN's General WDN v3 model (BSD-3) in ncnn form from
//! Upscayl's custom models at a fixed commit. Each download is checked against its SHA-256 before anything is kept;
//! the files are written only once all have downloaded, and the cache beside them is left as it is.
//!
//! The cache of upscaled textures there (`upscale::CACHE`) is looked after while no viewer runs ([`Cache`]): the plain
//! files a build before 2026-11-13 left are packed, at the launcher's start and after each drive, and the Graphics
//! page shows its size with "Clear…".

use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use sha2::{Digest, Sha256};

use crate::gvrc;
use crate::job::{Counter, Job, Progress, Report, Temp};
use crate::pages::{GOOD, WARN};
use crate::Form;
use rbr_formats::rbz;
use rbr_render::upscale::{self, BINARY, MODEL};

type Fail = String;

/// A release zip: its URL, SHA-256, and the files kept from it.
struct Release {
    url: &'static str,
    sha256: &'static str,
    files: &'static [&'static str],
}

const RELEASE_URL: &str = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/";

/// This platform's release, if Real-ESRGAN builds one for it (none for ARM Linux or Windows).
const RELEASE: Option<Release> = if cfg!(target_os = "macos") {
    // A universal binary, x86_64 and arm64.
    Some(Release {
        url: "realesrgan-ncnn-vulkan-20220424-macos.zip",
        sha256: "e0ad05580abfeb25f8d8fb55aaf7bedf552c375b5b4d9bd3c8d59764d2cc333a",
        files: &[BINARY],
    })
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some(Release {
        url: "realesrgan-ncnn-vulkan-20220424-ubuntu.zip",
        sha256: "e5aa6eb131234b87c0c51f82b89390f5e3e642b7b70f2b9bbe95b6a285a40c96",
        files: &[BINARY],
    })
} else if cfg!(all(windows, target_arch = "x86_64")) {
    Some(Release {
        url: "realesrgan-ncnn-vulkan-20220424-windows.zip",
        sha256: "abc02804e17982a3be33675e4d471e91ea374e65b70167abc09e31acb412802d",
        files: &[BINARY, "vcomp140.dll"],
    })
} else {
    None
};

const MODEL_URL: &str =
    "https://raw.githubusercontent.com/upscayl/custom-models/500e6a0e8d60b1bf89c685391ef464f3b909b786/models/";

/// The model's files, by extension, with their SHA-256.
const MODEL_FILES: [(&str, &str); 2] = [
    ("param", "22174924330297357434ad21ed0af7f4b820008d2a502b492754d130d4142714"),
    ("bin", "74eae91a6230cbe5b931be6c49141faaff0034976933d4da9accb77c329c8a78"),
];

/// Whether this platform has an upscaler to download.
pub fn available() -> bool {
    RELEASE.is_some()
}

/// Checks the file `path` against `sha256`.
fn check(path: &Path, sha256: &str) -> Result<(), Fail> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let bytes = fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    let found = format!("{:x}", Sha256::digest(&bytes));
    (found == sha256).then_some(()).ok_or_else(|| format!("{name} is not the expected file (SHA-256 {found})"))
}

/// Downloads the upscaler into the folder `dir`.
pub fn download(dir: &Path, report: Report<'_>, cancel: &AtomicBool) -> Result<(), Fail> {
    let release = RELEASE.ok_or("Real-ESRGAN has no upscaler build for this platform")?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let temp = Temp::create(dir, "downloading")?;
    let agent = gvrc::agent();
    let zip_url = format!("{RELEASE_URL}{}", release.url);
    let model_urls = MODEL_FILES.map(|(ext, _)| format!("{MODEL_URL}{MODEL}.{ext}"));
    let total = std::iter::once(&zip_url).chain(&model_urls).map(|url| gvrc::size_of(&agent, url)).sum();
    let mut count = Counter { step: "Downloading the upscaler", done: 0, total, report, cancel };
    count.add(0)?;

    let zip_path = temp.0.join(release.url);
    gvrc::fetch(&agent, &zip_url, &zip_path, &mut count)?;
    check(&zip_path, release.sha256)?;
    let mut zip = rbz::open(&zip_path).map_err(|e| format!("{}: {e}", release.url))?;
    let staged = temp.0.join("files");
    fs::create_dir_all(staged.join("models")).map_err(|e| e.to_string())?;
    for name in release.files {
        let mut bytes = Vec::new();
        rbz::by_name(&mut zip, name)
            .and_then(|mut file| Ok(file.read_to_end(&mut bytes)?))
            .map_err(|e| format!("{name} in {}: {e}", release.url))?;
        fs::write(staged.join(name), bytes).map_err(|e| format!("{name}: {e}"))?;
    }
    for (url, (ext, sha256)) in model_urls.iter().zip(MODEL_FILES) {
        let path = staged.join("models").join(format!("{MODEL}.{ext}"));
        gvrc::fetch(&agent, url, &path, &mut count)?;
        check(&path, sha256)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(staged.join(BINARY), fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }

    report(&Progress { step: "Installing the upscaler", ..Progress::default() });
    fs::create_dir_all(dir.join("models")).map_err(|e| e.to_string())?;
    let models = MODEL_FILES.map(|(ext, _)| format!("models/{MODEL}.{ext}"));
    for name in release.files.iter().copied().chain(models.iter().map(String::as_str)) {
        fs::rename(staged.join(name), dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

/// The download's confirmation, job and result line.
#[derive(Default)]
pub struct Download {
    confirm: bool,
    job: Option<Job<()>>,
    status: Option<Result<String, String>>,
}

/// The upscaled textures' folder: the job that packs it and measures it, or clears it, its size, and "Clear…"'s
/// confirmation.
#[derive(Default)]
pub struct Cache {
    job: Option<Job<u64>>,
    /// The folder's size in bytes as the last job left it: none while the viewer, which adds to it, runs.
    size: Option<u64>,
    confirm: bool,
}

impl Cache {
    /// Each frame: packs and measures `cache` when its size is not known, and stops that while the viewer runs.
    pub fn tick(&mut self, cache: &Path, running: bool) {
        if let Some(job) = &mut self.job {
            if running {
                job.cancel.store(true, Ordering::Relaxed);
            }
            if let Some(result) = job.result() {
                (self.job, self.size) = (None, result.ok());
            }
        }
        if running {
            (self.size, self.confirm) = (None, false);
        } else if self.size.is_none() && self.job.is_none() {
            let cache = cache.to_path_buf();
            self.job = Some(Job::start(move |report, cancel| pack(&cache, report, cancel)));
        }
    }
}

/// Packs `cache`'s plain files (`upscale::pack`) over half the machine's threads, which leaves the launcher its
/// frames, and returns the cache's size in bytes. A file that fails stays for the next run.
fn pack(cache: &Path, report: Report<'_>, cancel: &AtomicBool) -> Result<u64, Fail> {
    upscale::tidy(cache);
    let plain = upscale::plain_files(cache);
    let total = plain.iter().map(|(_, size)| size).sum();
    let (next, done) = (AtomicUsize::new(0), AtomicU64::new(0));
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get() / 2).max(1);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while !cancel.load(Ordering::Relaxed) {
                    let Some((path, size)) = plain.get(next.fetch_add(1, Ordering::Relaxed)) else { break };
                    let _ = upscale::pack(path);
                    let done = done.fetch_add(*size, Ordering::Relaxed) + size;
                    report(&Progress { step: "Packing the texture cache", done, total });
                }
            });
        }
    });
    match cancel.load(Ordering::Relaxed) {
        true => Err("cancelled".into()),
        false => Ok(upscale::cache_size(cache)),
    }
}

/// A size in bytes as shown: `1.5 GB`, under 1 GB `340 MB`.
fn size_text(bytes: u64) -> String {
    match bytes >> 20 {
        mb if mb >= 1024 => format!("{:.1} GB", mb as f64 / 1024.0),
        mb => format!("{mb} MB"),
    }
}

impl Form {
    /// The upscaler's folder in the player's folder.
    pub fn upscaler_dir(&self) -> std::path::PathBuf {
        self.here().join(upscale::DIR)
    }

    /// Each frame: looks after the upscaled textures' folder ([`Cache::tick`]); not when nothing is written.
    pub fn upscaler_cache_tick(&mut self) {
        if !self.read_only {
            self.upscaler_cache.tick(&self.upscaler_dir().join(upscale::CACHE), self.child.is_some());
        }
    }

    /// Beside "upscaled": the packing's progress, else the upscaled textures' size and "Clear…" with its
    /// confirmation.
    pub fn upscaler_cache_ui(&mut self, ui: &mut egui::Ui) {
        let dir = self.upscaler_dir().join(upscale::CACHE);
        let cache = &mut self.upscaler_cache;
        if let Some(job) = &cache.job {
            let progress = job.progress.lock().unwrap().clone();
            if progress.total > 0 {
                ui.spinner();
                ui.weak(progress.line());
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        let Some(size) = cache.size.filter(|&size| size > 0).map(size_text) else { return };
        if !cache.confirm {
            ui.weak(format!("{size} kept"));
            let tip = "Deletes the upscaled textures kept for the cars and stages driven so far. The first drive with \
                       each car and on each stage makes them again";
            cache.confirm = ui.button("Clear…").on_hover_text(tip).clicked();
            return;
        }
        ui.label(format!("Delete {size}?"));
        if ui.button("Delete").clicked() {
            (cache.confirm, cache.size) = (false, None);
            cache.job = Some(Job::start(move |_, _| match fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", dir.display())),
                _ => Ok(0),
            }));
        }
        if ui.button("Cancel").clicked() {
            cache.confirm = false;
        }
    }

    /// "Download upscaler…", its confirmation, progress and result line, while the upscaler is not there.
    pub fn upscaler_download_ui(&mut self, ui: &mut egui::Ui) {
        let dir = self.upscaler_dir();
        let download = &mut self.upscaler_download;
        if download.job.is_none() && !download.confirm && ui.button("Download upscaler…").clicked() {
            (download.confirm, download.status) = (true, None);
        }
        if download.confirm {
            ui.label(format!(
                "Download the texture upscaler (about 50 MB) into {}? It is Real-ESRGAN's realesrgan-ncnn-vulkan (MIT \
                 licence, github.com/xinntao/Real-ESRGAN) and its General WDN v3 model (BSD-3 licence), from \
                 github.com/upscayl/custom-models.",
                dir.display()
            ));
            ui.horizontal(|ui| {
                if ui.button("Download").clicked() {
                    download.confirm = false;
                    let to = dir.clone();
                    download.job = Some(Job::start(move |report, cancel| self::download(&to, report, cancel)));
                }
                if ui.button("Cancel").clicked() {
                    download.confirm = false;
                }
            });
        }
        if let Some(job) = &mut download.job {
            if let Some(result) = ui.horizontal_wrapped(|ui| job.ui(ui)).inner {
                download.job = None;
                download.status = Some(match result {
                    Err(e) if e.ends_with("cancelled") => Err("Cancelled; nothing was kept.".into()),
                    Err(e) => Err(e),
                    Ok(()) => Ok("The upscaler is installed.".into()),
                });
            }
        }
        match &download.status {
            Some(Ok(text)) => _ = ui.colored_label(GOOD, format!("✔ {text}")),
            Some(Err(text)) => _ = ui.colored_label(WARN, format!("⚠ {text}")),
            None => {}
        }
        if matches!(download.status, Some(Ok(_))) {
            self.upscaler = upscale::Upscaler::find(&dir).is_some();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_is_packed_and_measured_while_no_viewer_runs() {
        let cache = crate::test_dir("upscaler-cache");
        let folder = cache.join("model-4096");
        fs::create_dir_all(&folder).unwrap();
        let texture = upscale::encode(rbr_formats::dds::Format::Dxt1, 64, 64, vec![128; 64 * 64 * 4]);
        fs::write(folder.join("a.dds"), &texture).unwrap();
        let mut state = Cache::default();
        let wait = |state: &mut Cache, running| {
            state.tick(&cache, running);
            while state.job.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(5));
                state.tick(&cache, running);
            }
        };
        // A running viewer: nothing is started.
        wait(&mut state, true);
        assert!(state.size.is_none() && folder.join("a.dds").exists());
        wait(&mut state, false);
        let packed = fs::metadata(folder.join("a.dds.gz")).unwrap().len();
        assert!(!folder.join("a.dds").exists() && packed < texture.len() as u64);
        assert_eq!(state.size, Some(packed));
        assert_eq!((size_text(340 << 20), size_text(1536 << 20)), ("340 MB".into(), "1.5 GB".into()));
        fs::remove_dir_all(&cache).unwrap();
    }

    /// Downloads the real upscaler (network, about 50 MB): `cargo test -p rbr-launcher upscaler -- --ignored`.
    #[test]
    #[ignore]
    fn the_download_is_found() {
        let dir = crate::test_dir("upscaler-download");
        download(&dir, &|_| {}, &AtomicBool::new(false)).unwrap();
        assert!(upscale::Upscaler::find(&dir).is_some());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1 + RELEASE.unwrap().files.len(), "no temp folder left");
        fs::remove_dir_all(&dir).unwrap();
    }
}
