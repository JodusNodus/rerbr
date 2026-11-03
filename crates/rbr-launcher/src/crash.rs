//! When a drive, a replay or the launcher itself stops with an error: the run's log, a panel with its end, and a bug
//! report on GitHub filled in from it (the form's fields by their ids, `.github/ISSUE_TEMPLATE/bug_report.yml`).
//! Nothing is sent: the player reads the report on GitHub and submits it.
//!
//! A drive's viewer, and the `rbr-standalone` it starts, write to `<files>/logs/drive-<time>.log` (the last [`KEEP`]
//! kept), copied to the launcher's own stderr as it grows. A launcher panic goes to `<files>/logs/launcher-crash.log`
//! and is shown at the next start.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The bug report form.
const NEW_ISSUE: &str = "https://github.com/JodusNodus/rerbr/issues/new?template=bug_report.yml";
/// The drive logs kept.
const KEEP: usize = 10;
/// The end of a log shown and reported, bytes.
const TAIL: usize = 3000;
/// The longest bug report link (GitHub refuses links of about 8 KB).
const URL: usize = 7000;
/// A launcher panic, until the next start shows it.
const LAUNCHER_CRASH: &str = "launcher-crash.log";

/// The running viewer's log, and the report of the last run that failed.
#[derive(Default)]
pub struct Watch {
    /// The running viewer's log, its title (stage and car), and whether it is a replay.
    run: Option<(PathBuf, String, bool)>,
    /// Ends the copy of the log to the launcher's stderr.
    copying: Option<Arc<AtomicBool>>,
    /// Stop was pressed: the exit is not an error.
    pub stopped: bool,
    /// The report the panel shows.
    pub report: Option<Report>,
}

/// What stopped, how, and the end of its log.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    heading: String,
    /// The drive's title and the exit, a line each.
    context: String,
    tail: String,
    log: PathBuf,
}

impl Watch {
    /// A new drive log in `logs` for the run titled `title`, the oldest beyond [`KEEP`] removed: the file the viewer
    /// writes to, copied to the launcher's stderr as it grows.
    pub fn start(&mut self, logs: &Path, title: &str) -> io::Result<File> {
        fs::create_dir_all(logs)?;
        prune(logs, "drive-", KEEP - 1);
        let path = logs.join(format!("drive-{}.log", stamp()));
        let file = File::create(&path)?;
        self.copying = Some(copy_to_stderr(path.clone()));
        self.watch(path, title, false);
        Ok(file)
    }

    /// Watches the run writing `log`.
    pub fn watch(&mut self, log: PathBuf, title: &str, replay: bool) {
        self.run = Some((log, title.to_owned(), replay));
        self.stopped = false;
    }

    /// The run did not start: its log is no longer copied.
    pub fn cancel(&mut self) {
        if let Some(done) = self.copying.take() {
            done.store(true, Ordering::Relaxed);
        }
        self.run = None;
    }

    /// The run ended with `status`: a report unless it succeeded or Stop ended it.
    pub fn finished(&mut self, status: ExitStatus) {
        let run = self.run.take();
        self.cancel();
        let Some((log, title, replay)) = run else { return };
        if status.success() || std::mem::take(&mut self.stopped) {
            return;
        }
        let (crashed, exit) = exit_words(status);
        let heading = match (crashed, replay) {
            (true, _) => "ReRBR crashed",
            (false, false) => "The drive stopped with an error",
            (false, true) => "The replay stopped with an error",
        };
        let tail = fs::read(&log).map(|bytes| tail(&String::from_utf8_lossy(&bytes))).unwrap_or_default();
        self.report = Some(Report { heading: heading.into(), context: format!("{title}\n{exit}"), tail, log });
    }

    /// The panel of the report, over the page.
    pub fn ui(&mut self, ctx: &egui::Context) {
        let Some(report) = &self.report else { return };
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("crash report")).show(ctx, |ui| {
            ui.set_width(680.0);
            ui.heading(&report.heading);
            ui.label(&report.context);
            ui.add_space(6.0);
            ui.weak(format!("The end of {}:", report.log.display()));
            egui::ScrollArea::vertical().max_height(280.0).stick_to_bottom(true).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&report.tail).monospace().size(11.5)).wrap());
            });
            ui.add_space(6.0);
            ui.weak(
                "Reporting opens GitHub's bug form filled in with this; nothing is sent until you submit it there. \
                 The log shows your file paths, which may include your user name.",
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Report on GitHub…").clicked() {
                    open(OsStr::new(&report.issue_url()));
                }
                if ui.button("Copy report").clicked() {
                    ui.ctx().copy_text(format!("{}\n{}", report.text(&report.tail), version()));
                }
                if ui.button("Open the log folder").clicked() {
                    open(report.log.parent().unwrap_or(Path::new(".")).as_os_str());
                }
                close = ui.button("Close").clicked();
            });
        });
        if close || modal.should_close() {
            self.report = None;
        }
    }
}

impl Report {
    /// What happened with `tail`, the end of the log: the bug form's first field, and with [`version`] what Copy
    /// report copies.
    fn text(&self, tail: &str) -> String {
        let name = self.log.file_name().unwrap_or_default().to_string_lossy();
        format!(
            "{}\n{}\n\nThe end of `{name}`:\n```text\n{tail}\n```\n\nWhat I was doing:\n",
            self.heading, self.context
        )
    }

    /// The bug form filled in: the title, what happened with the end of the log, and the version and system.
    fn issue_url(&self) -> String {
        let first = self.context.lines().next().unwrap_or_default();
        let title = if first.is_empty() { self.heading.clone() } else { format!("{}: {first}", self.heading) };
        // The log's first lines go until the link fits.
        let mut tail = self.tail.as_str();
        loop {
            let (what, version) = (self.text(tail), version());
            let url =
                format!("{NEW_ISSUE}&title={}&what={}&version={}", encode(&title), encode(&what), encode(&version));
            match tail.split_once('\n') {
                Some((_, rest)) if url.len() > URL => tail = rest,
                _ => return url,
            }
        }
    }
}

/// The version and system, for the form's second field.
fn version() -> String {
    format!("ReRBR {}, {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH)
}

/// Whether the exit is a crash (a panic, a signal or a Windows exception) rather than an error the program reported,
/// and how it ended.
fn exit_words(status: ExitStatus) -> (bool, String) {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return (true, format!("Ended by signal {signal}"));
    }
    match status.code() {
        // Rust's exit code after a panic.
        Some(101) => (true, "Exit code 101 (a panic)".into()),
        Some(code) if cfg!(windows) && code as u32 >= 0xC000_0000 => (true, format!("Exception 0x{:08X}", code as u32)),
        Some(code) => (false, format!("Exit code {code}")),
        None => (true, status.to_string()),
    }
}

/// The last [`TAIL`] bytes of `text`, from a line's start.
fn tail(text: &str) -> String {
    let text = text.trim_end();
    let mut start = text.len().saturating_sub(TAIL);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let end = &text[start..];
    match (start, end.find('\n')) {
        (0, _) | (_, None) => end.to_owned(),
        (_, Some(n)) => end[n + 1..].to_owned(),
    }
}

/// `text` for a link's query: unreserved characters as they are, every other byte as `%XX`.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Removes the oldest `<prefix>*.log` of `dir` beyond `keep`.
fn prune(dir: &Path, prefix: &str, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(OsStr::to_str).is_some_and(|n| n.starts_with(prefix) && n.ends_with(".log")))
        .collect();
    // The names carry the time.
    logs.sort();
    for old in logs.iter().rev().skip(keep) {
        let _ = fs::remove_file(old);
    }
}

fn stamp() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// Copies what is written to `log` to the launcher's stderr until the returned flag is set (the terminal, or
/// `rerbr.log` on Windows). The viewer writes to the file itself, so it never writes to a closed pipe when the
/// launcher closes first.
fn copy_to_stderr(log: PathBuf) -> Arc<AtomicBool> {
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    std::thread::spawn(move || {
        let Ok(mut file) = File::open(&log) else { return };
        let mut buffer = Vec::new();
        loop {
            let last = flag.load(Ordering::Relaxed);
            buffer.clear();
            if file.read_to_end(&mut buffer).is_ok() && !buffer.is_empty() {
                let _ = io::stderr().write_all(&buffer);
            }
            if last {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
    done
}

/// Opens a link or a folder with the system's handler.
fn open(target: &OsStr) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut command = Command::new(program);
    rbr_render::console::no_window(&mut command).arg(target).stdout(Stdio::null()).stderr(Stdio::null());
    match command.spawn() {
        // Reaped on its own thread.
        Ok(mut child) => _ = std::thread::spawn(move || child.wait()),
        Err(e) => eprintln!("launcher: {program}: {e}"),
    }
}

/// Also writes a launcher panic, with its backtrace, to `logs`, for the next start to show.
pub fn hook(logs: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let text = format!("{info}\n\n{}", std::backtrace::Backtrace::force_capture());
        let _ = fs::create_dir_all(&logs).and_then(|()| fs::write(logs.join(LAUNCHER_CRASH), text));
    }));
}

/// The launcher's panic at an earlier start, once: its file is renamed `launcher-crash-<time>.log`.
pub fn last_crash(logs: &Path) -> Option<Report> {
    let path = logs.join(LAUNCHER_CRASH);
    let text = fs::read_to_string(&path).ok()?;
    let kept = logs.join(format!("launcher-crash-{}.log", stamp()));
    fs::rename(&path, &kept).ok()?;
    prune(logs, "launcher-crash-", KEEP);
    Some(Report {
        heading: "ReRBR crashed the last time it ran".into(),
        context: "The launcher hit an error.".into(),
        tail: tail(&text),
        log: kept,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_is_the_end_of_the_log_from_a_line() {
        assert_eq!(tail("a\nb\n"), "a\nb");
        let long = format!("{}\nlast line", "x".repeat(TAIL * 2));
        assert_eq!(tail(&long), "last line");
        let lines: String = (0..1000).map(|n| format!("line {n}\n")).collect();
        let end = tail(&lines);
        assert!(end.len() <= TAIL && end.starts_with("line ") && end.ends_with("line 999"), "{end}");
    }

    #[test]
    fn the_form_is_filled_in_through_the_link() {
        assert_eq!(encode("a b&c=ü\n"), "a%20b%26c%3D%C3%BC%0A");
        let report = Report {
            heading: "The drive stopped with an error".into(),
            context: "Torsby 2018 · Fiesta\nExit code 1".into(),
            tail: "viewer: the physics stopped".into(),
            log: PathBuf::from("/files/logs/drive-1.log"),
        };
        let url = report.issue_url();
        assert!(url.starts_with(NEW_ISSUE), "{url}");
        assert!(url.contains("&title=The%20drive%20stopped%20with%20an%20error%3A%20Torsby%202018%20%C2%B7%20Fiesta&"));
        assert!(url.contains("drive-1.log") && url.contains("the%20physics%20stopped"), "{url}");
        assert!(url.contains(&format!("&version={}", encode(&version()))), "{url}");
        // A log of symbols, three bytes each in the link, is cut from its start to fit.
        let lines: String = (0..200).map(|n| format!("{n} ~ <> ()\n")).collect();
        let url = Report { tail: tail(&lines), ..report }.issue_url();
        assert!(url.len() <= URL && url.contains("199%20"), "{}", url.len());
    }

    #[cfg(unix)]
    #[test]
    fn a_run_that_fails_gets_a_report_and_a_stopped_one_does_not() {
        use std::os::unix::process::ExitStatusExt;
        let dir = crate::test_dir("crash-watch");
        let mut watch = Watch::default();
        let mut file = watch.start(&dir, "Torsby 2018 · Fiesta").unwrap();
        writeln!(file, "viewer: the physics stopped").unwrap();
        watch.finished(ExitStatus::from_raw(1 << 8));
        let report = watch.report.take().expect("a report");
        assert_eq!(report.heading, "The drive stopped with an error");
        assert_eq!(report.context, "Torsby 2018 · Fiesta\nExit code 1");
        assert_eq!(report.tail, "viewer: the physics stopped");

        watch.start(&dir, "Torsby 2018 · Fiesta").unwrap();
        watch.finished(ExitStatus::from_raw(101 << 8));
        assert_eq!(watch.report.take().unwrap().heading, "ReRBR crashed");
        watch.start(&dir, "Torsby 2018 · Fiesta").unwrap();
        watch.stopped = true;
        watch.finished(ExitStatus::from_raw(9));
        assert_eq!(watch.report, None);
        watch.start(&dir, "Torsby 2018 · Fiesta").unwrap();
        watch.finished(ExitStatus::from_raw(0));
        assert_eq!(watch.report, None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_logs_go() {
        let dir = crate::test_dir("crash-prune");
        fs::create_dir_all(&dir).unwrap();
        for n in 10..25 {
            fs::write(dir.join(format!("drive-{n}.log")), "").unwrap();
        }
        fs::write(dir.join("replay-1.log"), "").unwrap();
        prune(&dir, "drive-", 3);
        let mut left: Vec<String> =
            fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["drive-22.log", "drive-23.log", "drive-24.log", "replay-1.log"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_launcher_crash_shows_once() {
        let dir = crate::test_dir("crash-launcher");
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(last_crash(&dir), None);
        fs::write(dir.join(LAUNCHER_CRASH), "panicked at src/pages.rs:1:1:\nboom").unwrap();
        let report = last_crash(&dir).expect("the crash");
        assert!(report.tail.ends_with("boom") && report.log.exists(), "{report:?}");
        assert_eq!(last_crash(&dir), None);
        fs::remove_dir_all(dir).unwrap();
    }
}
