//! One "Add" for everything a player downloads: files dropped on the launcher window, or picked from the Settings
//! page's Add content section. Each item is classified from its names ([`detect`]), added where it belongs, and the
//! result is a notice on every page ([`ui`]) saying whether it worked and where to find it, or why it did not.
//!
//! Where each kind goes (user decision 2026-11-16): BTB stages to `tracks/` ([`btb`]); co-drivers and pace notes
//! through `codriver_packs::import`; NGP car pieces (physics zip, model and sound-bank archives dropped together) as
//! one `car_packs::Download`; NGP's package to the kept `NGP 7.4.778/` folder, as the Settings download does; a disc
//! image opens "Set up from the disc…" with the disc filled in. RAR files, installers, classic-format stages and
//! anything unknown are refused with the reason. A model or banks archive without its physics zip is refused: it has
//! no car to go to.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::car_packs::{self, Download, MODEL};
use crate::job::{Progress, Report};
use detect::Kind;

pub mod btb;
pub mod detect;
mod drop;
pub mod recent;
mod ui;

pub use ui::Add;

/// Where an outcome's Open button goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Goto {
    /// The stage picker, on the stage folder.
    Stage(String),
    /// The Settings page.
    Settings,
}

/// What happened to one item (or one thing in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The dropped or picked item's name.
    pub source: String,
    pub kind: &'static str,
    /// What was added and where to find it, or why not.
    pub result: Result<String, String>,
    pub goto: Option<Goto>,
}

impl Outcome {
    fn added(source: &str, kind: &'static str, what: &str, place: &str, goto: Goto) -> Outcome {
        let text = format!("Added {what} ({kind}): {place}");
        Outcome { source: source.to_owned(), kind, result: Ok(text), goto: Some(goto) }
    }

    fn failed(source: &str, kind: &'static str, reason: impl Into<String>) -> Outcome {
        Outcome { source: source.to_owned(), kind, result: Err(reason.into()), goto: None }
    }

    /// A warning that belongs to no item.
    pub fn warning(text: &str) -> Outcome {
        Outcome::failed("", "", text)
    }

    pub fn ok(&self) -> bool {
        self.result.is_ok()
    }

    /// The notice line, without its tick.
    pub fn line(&self) -> String {
        match (&self.result, self.source.is_empty()) {
            (Ok(text), _) => text.clone(),
            (Err(reason), true) => reason.clone(),
            (Err(reason), false) => format!("Couldn't add {}: {reason}", self.source),
        }
    }
}

/// What a run touched, so the launcher reads those lists again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Touched {
    pub stages: bool,
    pub cars: bool,
    pub codriver: bool,
    pub ngp: bool,
}

/// A finished run.
#[derive(Debug, Default)]
pub struct Done {
    pub outcomes: Vec<Outcome>,
    /// A disc image to open the disc setup with.
    pub disc: Option<PathBuf>,
    pub touched: Touched,
}

/// One dropped or picked item and what it holds.
struct Item {
    path: PathBuf,
    name: String,
    kinds: Result<Vec<Kind>, String>,
    files: Vec<btb::File>,
}

impl Item {
    fn read(path: &Path) -> Item {
        let name =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        let dir = path.is_dir();
        let lower = name.to_ascii_lowercase();
        let listed = if !path.exists() {
            Err("it doesn't exist".to_owned())
        } else if dir {
            Ok(btb::folder_files(path))
        } else if lower.ends_with(".zip") || lower.ends_with(".7z") {
            btb::archive_files(path)
        } else {
            Ok(Vec::new())
        };
        match listed {
            Err(e) => Item { path: path.to_owned(), name, kinds: Err(e), files: Vec::new() },
            Ok(files) => {
                let entries: Vec<String> = files.iter().map(|(p, _)| p.join("/")).collect();
                let kinds = Ok(detect::classify(&name, dir, &entries));
                Item { path: path.to_owned(), name, kinds, files }
            }
        }
    }
}

/// The physics zips of `physics` with the model and the sound banks that go with each, by name; the model and banks
/// archives that go with none.
pub fn group_cars(physics: &[PathBuf], models: &[PathBuf], banks: &[PathBuf]) -> (Vec<Download>, Vec<PathBuf>) {
    let file = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut models: Vec<(String, &PathBuf)> = models.iter().map(|p| (detect::car_name(&file(p), MODEL), p)).collect();
    let mut banks: Vec<(String, &PathBuf)> =
        banks.iter().map(|p| (detect::car_name(&file(p), car_packs::BANKS), p)).collect();
    let take = |pool: &mut Vec<(String, &PathBuf)>, car: &str| {
        let names: Vec<&str> = pool.iter().map(|(n, _)| n.as_str()).collect();
        let at = car_packs::pick_model(&[car], &names)?;
        Some(pool.remove(at).1.clone())
    };
    let downloads = physics
        .iter()
        .map(|p| {
            let car = detect::physics_name(&file(p));
            Download {
                physics: p.clone(),
                model: take(&mut models, &car),
                banks: take(&mut banks, &car),
                group: None,
                drive_train: None,
                year: None,
            }
        })
        .collect();
    let left = models.into_iter().chain(banks).map(|(_, p)| p.clone()).collect();
    (downloads, left)
}

/// The outcome of a co-driver import's result line (`Co-driver <pack>: …`, `Stage notes <pack>: …`).
fn codriver_outcome(source: &str, line: &str) -> Outcome {
    let (kind, rest, place) = match (line.strip_prefix("Co-driver "), line.strip_prefix("Stage notes ")) {
        (Some(rest), _) => ("co-driver", rest, "pick it in Settings > Co-driver"),
        (_, Some(rest)) => ("pace notes", rest, "they play with a custom co-driver (Settings > Co-driver)"),
        _ => ("co-driver", line, "pick it in Settings > Co-driver"),
    };
    let pack = rest.split(':').next().unwrap_or(rest);
    Outcome::added(source, kind, pack, place, Goto::Settings)
}

fn cancelled(source: &str, kind: &'static str) -> Outcome {
    Outcome::failed(source, kind, "cancelled")
}

fn reason(e: String) -> String {
    match e.ends_with("cancelled") {
        true => "cancelled".into(),
        false => e,
    }
}

/// Adds every item of `paths` to the player's folder `home`, in turn, and says what happened to each.
pub fn run(home: &Path, paths: &[PathBuf], report: Report<'_>, cancel: &AtomicBool) -> Done {
    let tracks = home.join(rbr_btb::player::TRACKS);
    let items: Vec<Item> = paths.iter().map(|p| Item::read(p)).collect();
    let mut done = Done::default();
    let (mut physics, mut models, mut banks) = (Vec::new(), Vec::new(), Vec::new());
    let step = |step: &'static str| report(&Progress { step, done: 0, total: 0 });
    for item in &items {
        let kinds = match &item.kinds {
            Ok(kinds) => kinds.clone(),
            Err(e) => {
                done.outcomes.push(Outcome::failed(&item.name, "", e.clone()));
                continue;
            }
        };
        // A folder nothing else claims may still be a co-driver or notes folder `import` accepts.
        let kinds = match kinds[..] {
            [Kind::Refused(ref why)] if why == detect::UNKNOWN && item.path.is_dir() => {
                step("Adding co-driver");
                match crate::codriver_packs::import(home, &item.path, cancel) {
                    Ok(lines) => {
                        done.touched.codriver = true;
                        done.outcomes.extend(lines.iter().map(|l| codriver_outcome(&item.name, l)));
                        continue;
                    }
                    Err(_) => kinds,
                }
            }
            _ => kinds,
        };
        for kind in kinds {
            if cancel.load(Ordering::Relaxed) {
                done.outcomes.push(cancelled(&item.name, kind.label()));
                continue;
            }
            match kind {
                Kind::Refused(why) => done.outcomes.push(Outcome::failed(&item.name, kind_label_refused(), why)),
                Kind::Physics => physics.push(item.path.clone()),
                Kind::Model => models.push(item.path.clone()),
                Kind::Banks => banks.push(item.path.clone()),
                Kind::Stages(stages) => {
                    done.touched.stages = true;
                    step("Adding BTB stage");
                    for stage in &stages {
                        let added = btb::add_stage(&item.path, &item.files, stage, &tracks, report, cancel);
                        done.outcomes.push(match added {
                            Ok(a) => Outcome::added(
                                &item.name,
                                "BTB stage",
                                &a.name,
                                "it's in the stage picker",
                                Goto::Stage(a.folder),
                            ),
                            Err(e) => Outcome::failed(&stage.name, "BTB stage", reason(e)),
                        });
                    }
                }
                Kind::Codriver => {
                    done.touched.codriver = true;
                    step("Adding co-driver");
                    match crate::codriver_packs::import(home, &item.path, cancel) {
                        Ok(lines) => done.outcomes.extend(lines.iter().map(|l| codriver_outcome(&item.name, l))),
                        Err(e) => done.outcomes.push(Outcome::failed(&item.name, "co-driver", reason(e))),
                    }
                }
                Kind::Ngp => {
                    done.touched.ngp = true;
                    step("Adding NGP");
                    let to = home.join(crate::ngp_install::KEPT);
                    let result = std::fs::create_dir_all(&to)
                        .map_err(|e| format!("{}: {e}", to.display()))
                        .and_then(|()| crate::ngp_install::add_ngp_to(&item.path, &to, report, cancel));
                    done.outcomes.push(match result {
                        Ok(()) => {
                            Outcome::added(&item.name, "NGP", "NGP 7.4.778", "it's the NGP in use now", Goto::Settings)
                        }
                        Err(e) => Outcome::failed(&item.name, "NGP", reason(e)),
                    });
                }
                Kind::Disc => match done.disc {
                    None => {
                        done.disc = Some(item.path.clone());
                        let text =
                            format!("Disc image {}: choose where to set it up in Settings > Game folder", item.name);
                        let goto = Some(Goto::Settings);
                        done.outcomes.push(Outcome {
                            source: item.name.clone(),
                            kind: "disc image",
                            result: Ok(text),
                            goto,
                        });
                    }
                    Some(_) => done.outcomes.push(Outcome::failed(&item.name, "disc image", "one disc at a time")),
                },
            }
        }
    }
    add_cars(home, &physics, &models, &banks, &items, report, cancel, &mut done);
    done
}

fn kind_label_refused() -> &'static str {
    "not supported"
}

/// Adds the car pieces as downloads, and refuses the model and sound archives that have no physics zip.
#[allow(clippy::too_many_arguments)]
fn add_cars(
    home: &Path,
    physics: &[PathBuf],
    models: &[PathBuf],
    banks: &[PathBuf],
    items: &[Item],
    report: Report<'_>,
    cancel: &AtomicBool,
    done: &mut Done,
) {
    let name = |p: &PathBuf| items.iter().find(|i| &i.path == p).map(|i| i.name.clone()).unwrap_or_default();
    let (downloads, left) = group_cars(physics, models, banks);
    for path in left {
        let why = "this is a car model/sound; add it together with its NGP Physics zip";
        done.outcomes.push(Outcome::failed(&name(&path), "NGP car", why));
    }
    if downloads.is_empty() {
        return;
    }
    done.touched.cars = true;
    report(&Progress { step: "Adding cars", done: 0, total: 0 });
    match car_packs::add_downloads(&home.join(car_packs::DIR), &downloads, report, cancel) {
        Ok(added) => {
            for a in added {
                done.outcomes.push(match a.result {
                    Ok((car, model)) => {
                        let place = match model {
                            Some(_) => "it's in the car picker",
                            None => "it's in the car picker, drawn with a stand-in model until its model is added",
                        };
                        Outcome::added(&a.download, "NGP car", &car, place, Goto::Settings)
                    }
                    Err(e) => Outcome::failed(&a.download, "NGP car", reason(e)),
                });
            }
        }
        Err(e) => {
            for d in &downloads {
                done.outcomes.push(Outcome::failed(&name(&d.physics), "NGP car", reason(e.clone())));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn run_all(home: &Path, paths: &[PathBuf]) -> Done {
        run(home, paths, &|_| {}, &AtomicBool::new(false))
    }

    fn lines(done: &Done) -> Vec<String> {
        done.outcomes.iter().map(|o| format!("{} {}", if o.ok() { "ok" } else { "no" }, o.line())).collect()
    }

    #[test]
    fn cars_are_grouped_by_their_names() {
        let p = |s: &str| PathBuf::from(s);
        let physics = [p("/d/Ford Fiesta R5 - NGP Physics.zip"), p("/d/Audi quattro - NGP Physics.zip")];
        let models = [p("/d/Ford Fiesta - Model.7z"), p("/d/Skoda Fabia - Model.7z")];
        let banks = [p("/d/Audi quattro - banks.7z")];
        let (downloads, left) = group_cars(&physics, &models, &banks);
        assert_eq!(downloads[0].model, Some(p("/d/Ford Fiesta - Model.7z")));
        assert_eq!(downloads[0].banks, None);
        assert_eq!(
            (downloads[1].model.clone(), downloads[1].banks.clone()),
            (None, Some(p("/d/Audi quattro - banks.7z")))
        );
        assert_eq!(left, [p("/d/Skoda Fabia - Model.7z")]);
    }

    #[test]
    fn a_run_adds_each_item_and_says_why_the_others_were_not() {
        let dir = crate::test_dir("content-run");
        let home = dir.join("home");
        let stage = dir.join("drop/Autio I Gravel");
        btb::tests::synthetic_stage(&stage, "Autio I Gravel");
        let voice = dir.join("drop/Voice");
        btb::tests::write(&voice.join("PaceNote.ini"), b"[SETTINGS]\nsounds=english/steve\n");
        btb::tests::write(&voice.join("config/pacenotes/Rbr.ini"), b"[PACKAGE::RBR]\nfile=calls.ini\n");
        btb::tests::write(
            &voice.join("config/pacenotes/calls.ini"),
            b"[PACENOTE::ONE_LEFT]\nid=0\nSounds=1\nSnd0=one_left.ogg\n",
        );
        btb::tests::write(&voice.join("config/ranges/Rbr.ini"), b"[RANGE::50]\nSounds=1\nSnd0=50_1.ogg\n");
        btb::tests::write(&voice.join("sounds/english/steve/one_left.ogg"), b"OggS fake");
        btb::tests::write(&dir.join("drop/stage.rar"), b"Rar!");
        btb::tests::write(&dir.join("drop/Setup.exe"), b"MZ");
        btb::tests::write(&dir.join("drop/readme.txt"), b"hi");
        btb::tests::write(&dir.join("drop/RBR.iso"), b"iso");
        let paths: Vec<PathBuf> =
            ["Autio I Gravel", "Voice", "stage.rar", "Setup.exe", "readme.txt", "RBR.iso", "gone.zip"]
                .iter()
                .map(|n| dir.join("drop").join(n))
                .collect();
        let done = run_all(&home, &paths);
        let got = lines(&done);
        assert_eq!(got[0], "ok Added Autio I Gravel (BTB stage): it's in the stage picker");
        assert!(got[1].starts_with("ok Added Voice (co-driver): pick it in Settings"), "{got:?}");
        assert!(got[2].starts_with("no Couldn't add stage.rar: RAR archives can't be read"), "{got:?}");
        assert!(got[3].contains("installers can't be read: extract it with 7-Zip"), "{got:?}");
        assert!(got[4].contains("readme.txt: not recognised: ReRBR adds BTB stages"), "{got:?}");
        assert!(got[5].starts_with("ok Disc image RBR.iso"), "{got:?}");
        assert!(got[6].contains("gone.zip: it doesn't exist"), "{got:?}");
        assert_eq!(done.disc, Some(dir.join("drop/RBR.iso")));
        assert!(done.touched.stages && done.touched.codriver && !done.touched.cars);
        assert!(home.join("tracks/Autio I Gravel/track.ini").is_file());
        assert!(home.join(crate::codriver_packs::VOICES_DIR).join("Voice").is_dir());
        // The same stage again.
        let again = run_all(&home, &paths[..1]);
        assert_eq!(lines(&again), ["no Couldn't add Autio I Gravel: already added"]);
        // A cancelled run adds nothing.
        let stopped = run(&home, &paths[..1], &|_| {}, &AtomicBool::new(true));
        assert!(!stopped.outcomes[0].ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_model_without_its_physics_zip_is_refused() {
        let dir = crate::test_dir("content-model");
        let model = dir.join("Ford Fiesta - Model.7z");
        let mut out = sevenz_rust2::ArchiveWriter::create(&model).unwrap();
        out.push_archive_entry(
            sevenz_rust2::ArchiveEntry::new_file("Cars/Ford Fiesta/Ford Fiesta.sgc"),
            Some(&b"x"[..]),
        )
        .unwrap();
        out.finish().unwrap();
        let done = run_all(&dir.join("home"), &[model]);
        assert_eq!(
            lines(&done),
            ["no Couldn't add Ford Fiesta - Model.7z: this is a car model/sound; add it together with its NGP Physics zip"]
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
