//! Work running on a thread with a progress line and Cancel: the disc setup, adding car packs, the co-driver
//! import, and the downloads (`Job`, its `Progress` and its `Counter`, in a `Temp` folder beside the work that is
//! removed however the work ends), and the platform's file dialog.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// A failed or cancelled job's message.
type Fail = String;

/// An error's message led by the path it is about.
pub(crate) fn with_path<E: std::fmt::Display>(path: &Path) -> impl Fn(E) -> String + Copy + '_ {
    move |e| format!("{}: {e}", path.display())
}

/// A job's progress report: the step and the bytes written of all.
pub type Report<'a> = &'a (dyn Fn(&Progress) + Sync);

/// What the work is doing: a step, what it is on (a stage's name, empty when it is on no one thing) and the bytes
/// written of all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    pub step: &'static str,
    pub subject: String,
    pub done: u64,
    pub total: u64,
}

impl Progress {
    pub fn line(&self) -> String {
        let subject = if self.subject.is_empty() { String::new() } else { format!(" {}", self.subject) };
        match self.total {
            0 => format!("{}{subject}…", self.step),
            total => format!("{}{subject}… {} of {} MB", self.step, self.done >> 20, total >> 20),
        }
    }
}

/// Work running on its thread with a progress line and Cancel.
pub struct Job<T> {
    pub(crate) progress: Arc<Mutex<Progress>>,
    pub(crate) cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<T, Fail>>>,
}

impl<T: Send + 'static> Job<T> {
    /// Runs `work` with its progress report and its cancel flag.
    pub fn start(work: impl FnOnce(Report<'_>, &AtomicBool) -> Result<T, Fail> + Send + 'static) -> Job<T> {
        let progress = Arc::new(Mutex::new(Progress::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared, stop) = (progress.clone(), cancel.clone());
        let thread = std::thread::spawn(move || work(&|p| *shared.lock().unwrap() = p.clone(), &stop));
        Job { progress, cancel, thread: Some(thread) }
    }

    /// The work's result, once it has ended (for work shown without a progress line).
    pub fn result(&mut self) -> Option<Result<T, Fail>> {
        if !self.thread.as_ref()?.is_finished() {
            return None;
        }
        Some(self.thread.take()?.join().unwrap_or_else(|_| Err("it stopped unexpectedly".into())))
    }

    /// The spinner, the progress line and Cancel, for work polled elsewhere ([`Job::result`]).
    pub fn progress_ui(&self, ui: &mut egui::Ui) {
        ui.spinner();
        ui.label(self.progress.lock().unwrap().line());
        if ui.button("Cancel").clicked() {
            self.cancel.store(true, Ordering::Relaxed);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }

    /// The progress ([`Job::progress_ui`]); the result once the work has ended.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Result<T, Fail>> {
        self.progress_ui(ui);
        self.result()
    }
}

impl<T> Drop for Job<T> {
    /// Closing the window cancels the work and waits for it to remove its temp folder.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A temp folder beside the work, removed however the work ends.
pub struct Temp(pub PathBuf);

impl Temp {
    /// Makes `parent/.<prefix>-<pid>-<nanos>`; removed on drop.
    pub fn create(parent: &Path, prefix: &str) -> Result<Temp, Fail> {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let temp = Temp(parent.join(format!(".{prefix}-{}-{stamp}", std::process::id())));
        fs::create_dir_all(&temp.0).map_err(|e| format!("{}: {e}", temp.0.display()))?;
        Ok(temp)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Counts the bytes written of all, reports them, and stops when cancelled.
pub struct Counter<'a> {
    pub step: &'static str,
    pub done: u64,
    pub total: u64,
    pub report: Report<'a>,
    pub cancel: &'a AtomicBool,
}

impl Counter<'_> {
    pub fn add(&mut self, n: u64) -> Result<(), Fail> {
        self.done += n;
        (self.report)(&Progress { step: self.step, done: self.done, total: self.total, ..Progress::default() });
        match self.cancel.load(Ordering::Relaxed) {
            true => Err("cancelled".into()),
            false => Ok(()),
        }
    }
}

/// The platform's file dialog with `prompt` (`osascript` on macOS, PowerShell on Windows, `zenity` or `kdialog`
/// elsewhere); `None` when it is cancelled or there is none.
pub(crate) fn pick_file(prompt: &str) -> Option<String> {
    dialog(prompt, Pick::File).into_iter().next()
}

/// The platform's file dialog with several files to choose; none when it is cancelled or there is none.
pub(crate) fn pick_files(prompt: &str) -> Vec<String> {
    dialog(prompt, Pick::Files)
}

/// The platform's folder dialog with `prompt`; `None` when it is cancelled or there is none.
pub fn pick_folder(prompt: &str) -> Option<String> {
    dialog(prompt, Pick::Folder).into_iter().next()
}

#[derive(Clone, Copy, PartialEq)]
enum Pick {
    File,
    Files,
    Folder,
}

/// The platform's dialog: the paths chosen, one per line of its output.
fn dialog(prompt: &str, pick: Pick) -> Vec<String> {
    let run = |program: &str, args: &[&str]| -> Vec<String> {
        let Ok(out) = rbr_render::console::no_window(&mut Command::new(program)).args(args).output() else {
            return Vec::new();
        };
        let text = String::from_utf8(out.stdout).unwrap_or_default();
        let lines = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned);
        if out.status.success() {
            lines.collect()
        } else {
            Vec::new()
        }
    };
    let first = |paths: Vec<String>, other: &dyn Fn() -> Vec<String>| if paths.is_empty() { other() } else { paths };
    if cfg!(target_os = "macos") {
        let script = match pick {
            Pick::Folder => format!("POSIX path of (choose folder with prompt \"{prompt}\")"),
            Pick::File => format!("POSIX path of (choose file with prompt \"{prompt}\")"),
            Pick::Files => format!(
                "set picked to choose file with prompt \"{prompt}\" with multiple selections allowed\n\
                 set out to \"\"\nrepeat with f in picked\nset out to out & POSIX path of f & linefeed\n\
                 end repeat\nout"
            ),
        };
        run("osascript", &["-e", &script])
    } else if cfg!(windows) {
        let script = match pick {
            Pick::Folder => {
                "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
                 if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }"
            }
            Pick::File => {
                "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; \
                 if ($d.ShowDialog() -eq 'OK') { $d.FileName }"
            }
            Pick::Files => {
                "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; \
                 $d.Multiselect = $true; if ($d.ShowDialog() -eq 'OK') { $d.FileNames }"
            }
        };
        run("powershell", &["-NoProfile", "-Command", script])
    } else {
        let title = format!("--title={prompt}");
        match pick {
            Pick::Folder => first(run("zenity", &["--file-selection", "--directory", &title]), &|| {
                run("kdialog", &["--getexistingdirectory"])
            }),
            Pick::File => {
                first(run("zenity", &["--file-selection", &title]), &|| run("kdialog", &["--getopenfilename"]))
            }
            Pick::Files => first(run("zenity", &["--file-selection", "--multiple", "--separator=\n", &title]), &|| {
                run("kdialog", &["--getopenfilename", "--multiple", "--separate-output"])
            }),
        }
    }
}
