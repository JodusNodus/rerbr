//! Car skins: the ones installed for a car, the one the player chose, and importing a downloaded skin (a `.zip` or
//! `.7z` from RBR Skins Center and the like).
//!
//! A skin is a folder of files that replace the model folder's by path: `textures/<name>.dds` and, for some, shader
//! settings (`rbr_formats::data::Data::with_skin`). A car's skins are the ones imported into the player's folder
//! (`skins/<car key>/<skin>/`) and, for a community car with its own model, the ones in the model's `skins` folder
//! (RSF's). Choosing one is ours and saved per car key (`car_skins` in the settings); RSF's `.rsfconfig` selection
//! (`car_pack::selected_skin`) stays the default until one is chosen, and is never written.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rbr_formats::{car_pack, cars};

use crate::archive::{self, Entry};
use crate::catalog::{Car, Physics};
use crate::job::{self, Counter, Job, Report, Temp};
use crate::pages::{combo, GOOD, WARN};

/// The folder in the player's folder with the imported skins, `skins/<car key>/<skin>/`.
pub const DIR: &str = "skins";
/// The choice that draws the model's own textures.
pub const ORIGINAL: &str = "none";

type Fail = String;

/// An installed skin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin {
    pub name: String,
    pub dir: PathBuf,
}

/// The folder in the model's own folder with RSF's skins for a community car with its own model.
fn model_skins(car: &Car) -> Option<PathBuf> {
    let (Physics::Pack(_), Ok(model)) = (&car.physics, &car.model) else { return None };
    let root = car_pack::root_of(car.model_path.as_deref()?).ok()?;
    let folder = model.entry.file_name.split(['\\', '/']).rev().nth(1)?;
    cars::resolve(&root, &format!("Cars\\{folder}\\skins"))
}

/// The folders in `dir`, by name.
fn skins_in(dir: &Path) -> Vec<Skin> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| Some(Skin { name: archive::safe_folder(e.file_name().to_str()?)?, dir: e.path() }))
        .collect()
}

/// Whether skins can dress the car: a stand-in model is another car's, so its skins are not offered.
pub fn dressable(car: &Car) -> bool {
    match &car.model {
        Ok(model) => model.fallback.is_none(),
        Err(_) => false,
    }
}

/// The skins installed for `car` under the player's folder `home`, by name: its imported ones, then the model's own
/// folder's, which a name already imported hides.
pub fn installed(home: &Path, car: &Car) -> Vec<Skin> {
    if !dressable(car) {
        return Vec::new();
    }
    let mut skins = skins_in(&home.join(DIR).join(&car.key));
    for skin in model_skins(car).map(|dir| skins_in(&dir)).unwrap_or_default() {
        if !skins.iter().any(|s| s.name.eq_ignore_ascii_case(&skin.name)) {
            skins.push(skin);
        }
    }
    skins.retain(|s| !s.name.eq_ignore_ascii_case(ORIGINAL));
    skins.sort_by_key(|s| s.name.to_lowercase());
    skins
}

/// The skin `chosen` (a saved choice) is for `car`, as the viewer's `--car-skin`: none when nothing is chosen or the
/// chosen skin is gone, which leaves RSF's selection.
pub fn choice(home: &Path, car: &Car, chosen: Option<&str>) -> Option<car_pack::Skin> {
    match chosen? {
        ORIGINAL => Some(car_pack::Skin::Original),
        name => installed(home, car).into_iter().find(|s| s.name == name).map(|s| car_pack::Skin::Dir(s.dir)),
    }
}

/// The skins the player chose, the player's folder they are imported into, and the import running or its result.
#[derive(Default)]
pub struct Skins {
    /// The player's folder.
    home: PathBuf,
    /// The skin chosen for each car by its key (`ORIGINAL` is the model's own textures).
    pub chosen: BTreeMap<String, String>,
    /// The import running: the car it is for and its skins' names.
    job: Option<(String, Job<Vec<String>>)>,
    status: Option<Result<String, String>>,
}

impl Skins {
    pub fn new(home: PathBuf, chosen: BTreeMap<String, String>) -> Skins {
        Skins { home, chosen, ..Skins::default() }
    }

    /// The skin `car` is drawn with, as the viewer's `--car-skin`: none leaves RSF's selection.
    pub fn choice(&self, car: &Car) -> Option<car_pack::Skin> {
        choice(&self.home, car, self.chosen.get(&car.key).map(String::as_str))
    }

    /// The skin picker in the car browser: the skin of `car`, an Import button for a skin archive, and the import's
    /// progress or result. Nothing for a car without its own model.
    pub fn ui(&mut self, ui: &mut egui::Ui, car: &Car) {
        if !dressable(car) {
            return;
        }
        let skins = installed(&self.home, car);
        let community = matches!(car.physics, Physics::Pack(_));
        let mut picked =
            self.chosen.get(&car.key).filter(|n| *n == ORIGINAL || skins.iter().any(|s| &s.name == *n)).cloned();
        let mut options: Vec<Option<String>> = vec![None];
        options.extend(community.then(|| Some(ORIGINAL.to_owned())));
        options.extend(skins.iter().map(|s| Some(s.name.clone())));
        let label = |name: Option<String>| match name.as_deref() {
            None if community => "Default".to_owned(),
            None => "Original".to_owned(),
            Some(ORIGINAL) => "Original textures".to_owned(),
            Some(name) => name.to_owned(),
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Skin").color(crate::browser::GREY));
            combo(ui, "car-skin", 190.0, &mut picked, &options, label);
            let idle = self.job.is_none();
            let import = ui.add_enabled(idle, egui::Button::new("Import skin…"));
            if import.on_hover_text("A .zip or .7z skin, as RBR Skins Center's").clicked() {
                if let Some(path) = job::pick_file("Choose a skin archive (.zip or .7z)") {
                    self.start(PathBuf::from(path), &car.key);
                }
            }
        });
        match picked {
            Some(name) => self.chosen.insert(car.key.clone(), name),
            None => self.chosen.remove(&car.key),
        };
        let finished = self.job.as_mut().and_then(|(key, job)| {
            let result = ui.horizontal(|ui| job.ui(ui)).inner?;
            Some((key.clone(), result))
        });
        if let Some((key, result)) = finished {
            self.job = None;
            self.status = Some(result.map(|names| {
                let text = format!("Imported {}", names.join(", "));
                self.chosen.extend(names.into_iter().next().map(|first| (key, first)));
                text
            }));
        }
        match &self.status {
            Some(Ok(text)) => _ = ui.colored_label(GOOD, format!("✔ {text}")),
            Some(Err(text)) => _ = ui.colored_label(WARN, format!("⚠ {text}")),
            None => {}
        }
    }

    /// Imports `archive` into the car `key`'s skins on a thread of its own.
    fn start(&mut self, archive: PathBuf, key: &str) {
        let dest = self.home.join(DIR).join(key);
        self.status = None;
        self.job = Some((key.to_owned(), Job::start(move |report, cancel| import(&archive, &dest, report, cancel))));
    }
}

/// `chosen`'s skin saved by key: `key:skin|key:skin`'s pairs.
pub fn parse(names: Vec<String>) -> BTreeMap<String, String> {
    names.iter().filter_map(|n| n.split_once(':')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
}

/// The saved choices as the settings' `|` list, when any.
pub fn join(chosen: &BTreeMap<String, String>) -> Option<String> {
    (!chosen.is_empty()).then(|| chosen.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join("|"))
}

/// A skin's folder name from text that is not ours: no separators or the characters the settings and the platforms
/// keep for themselves.
fn clean(name: &str) -> String {
    let name: String = name.chars().map(|c| if "|:*?\"<>\\/".contains(c) { '_' } else { c }).collect();
    match archive::safe_folder(&name) {
        Some(name) if !name.eq_ignore_ascii_case(ORIGINAL) => name,
        _ => "skin".into(),
    }
}

/// The folders of an archive's file paths that hold a `textures` folder with files, the shallowest only: the folder
/// of a skin, whether the archive is the skin's own (`textures/`), a folder with it (`Skin/Textures/`) or a car's
/// (`Cars/<car>/Textures/`). Each is its path parts below the top.
fn skin_roots(files: &[&[String]]) -> Vec<Vec<String>> {
    let mut roots: Vec<Vec<String>> = files
        .iter()
        .filter_map(|parts| {
            let at = parts.iter().position(|p| p.eq_ignore_ascii_case("textures"))?;
            (at + 1 < parts.len()).then(|| parts[..at].to_vec())
        })
        .collect();
    let shallowest = roots.iter().map(Vec::len).min().unwrap_or(0);
    roots.retain(|r| r.len() == shallowest);
    roots.sort();
    roots.dedup();
    roots
}

/// Imports the skin archive `archive` into `dest` (a car's folder in the skins folder): its skins' names. One skin
/// takes the archive's name; an archive of several takes each folder's.
pub fn import(
    archive: &Path,
    dest: &Path,
    report: Report<'_>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<String>, Fail> {
    let extension = archive.extension().map(|e| e.to_string_lossy().to_lowercase());
    let seven = extension.as_deref() == Some("7z");
    let entries: Vec<Entry> = match extension.as_deref() {
        Some("zip") => archive::zip_entries(archive)?,
        Some("7z") => archive::seven_entries(archive)?,
        _ => return Err(format!("{}: not a .zip or .7z", archive.display())),
    };
    let files: Vec<&Entry> = entries.iter().filter(|e| !e.dir && e.parts.is_some()).collect();
    let parts: Vec<&[String]> = files.iter().filter_map(|e| e.parts.as_deref()).collect();
    let roots = skin_roots(&parts);
    if roots.is_empty() {
        return Err(format!("{}: no textures folder in it", archive.display()));
    }
    let stem = archive.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name_of = |root: &[String]| match (roots.len(), root.last()) {
        (1, _) | (_, None) => clean(&stem),
        (_, Some(last)) => clean(last),
    };
    let mut names: Vec<String> = roots.iter().map(|r| name_of(r)).collect();
    let total = files
        .iter()
        .filter(|e| e.parts.as_deref().is_some_and(|p| roots.iter().any(|r| p.starts_with(r))))
        .map(|e| e.size)
        .sum();
    let mut count = Counter { step: "Importing skin", done: 0, total, report, cancel };
    count.add(0)?;
    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    for (root, name) in roots.iter().zip(&names) {
        let wanted: Vec<(Vec<String>, Vec<String>)> = files
            .iter()
            .filter_map(|e| e.parts.clone())
            .filter(|p| p.starts_with(root))
            .map(|p| (p.clone(), p[root.len()..].to_vec()))
            .collect();
        let temp = Temp::create(dest, "importing")?;
        let progress = &mut |n| count.add(n);
        match seven {
            true => archive::extract_seven(archive, &temp.0, &wanted, progress)?,
            false => archive::extract_zip(archive, &temp.0, &wanted, progress)?,
        }
        let skin = dest.join(name);
        if skin.exists() {
            fs::remove_dir_all(&skin).map_err(|e| format!("{}: {e}", skin.display()))?;
        }
        fs::rename(&temp.0, &skin).map_err(|e| format!("{}: {e}", skin.display()))?;
        // Renamed away: the temp folder's drop finds nothing to remove.
        drop(temp);
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;

    fn zip(path: &Path, names: &[&str]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for name in names {
            zip.start_file(*name, options).unwrap();
            zip.write_all(name.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn import_into(archive: &Path, dest: &Path) -> Result<Vec<String>, Fail> {
        import(archive, dest, &|_| {}, &AtomicBool::new(false))
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = skins_in(dir).into_iter().map(|s| s.name).collect();
        names.sort();
        names
    }

    #[test]
    fn a_skin_archive_takes_the_archives_name_in_any_layout() {
        let root = crate::test_dir("skin-import");
        let dest = root.join("skins").join("IMPREZA03");
        for (file, inside) in [
            ("Red Bull.zip", ["Textures/body.dds", "Textures/glass.dds", "readme.txt"]),
            ("Folder.zip", ["Folder/textures/body.dds", "Folder/textures/glass.dds", "Folder/readme.txt"]),
            ("Car.zip", ["Cars/IMPREZA03/Textures/body.dds", "Cars/IMPREZA03/Textures/glass.dds", "x/y.txt"]),
        ] {
            zip(&root.join(file), &inside);
            let skin = file.trim_end_matches(".zip");
            assert_eq!(import_into(&root.join(file), &dest).unwrap(), [skin]);
            assert!(
                dest.join(skin).join("Textures").join("body.dds").is_file()
                    || dest.join(skin).join("textures").join("body.dds").is_file()
            );
        }
        assert_eq!(names(&dest), ["Car", "Folder", "Red Bull"]);
        // Importing again replaces the skin.
        zip(&root.join("Red Bull.zip"), &["textures/new.dds"]);
        import_into(&root.join("Red Bull.zip"), &dest).unwrap();
        assert_eq!(fs::read_dir(dest.join("Red Bull")).unwrap().count(), 1);
        assert_eq!(names(&dest).len(), 3);
        assert!(fs::read_dir(&dest).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_archive_of_several_skins_takes_each_folders_name() {
        let root = crate::test_dir("skin-several");
        zip(&root.join("Pack.zip"), &["Pack/Blue/textures/a.dds", "Pack/Red/Textures/a.dds", "Pack/readme.txt"]);
        assert_eq!(import_into(&root.join("Pack.zip"), &root.join("out")).unwrap(), ["Blue", "Red"]);
        let mut seven = sevenz_rust2::ArchiveWriter::create(root.join("Seven.7z")).unwrap();
        seven
            .push_archive_entry(sevenz_rust2::ArchiveEntry::new_file("textures/a.dds"), Some(b"dds".as_slice()))
            .unwrap();
        seven.finish().unwrap();
        assert_eq!(import_into(&root.join("Seven.7z"), &root.join("out")).unwrap(), ["Seven"]);
        assert_eq!(fs::read(root.join("out/Seven/textures/a.dds")).unwrap(), b"dds");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_is_not_a_skin_is_refused_and_leaves_nothing() {
        let root = crate::test_dir("skin-refused");
        zip(&root.join("None.zip"), &["readme.txt", "../textures/a.dds"]);
        let dest = root.join("out");
        assert!(import_into(&root.join("None.zip"), &dest).unwrap_err().contains("no textures folder"));
        assert!(import_into(&root.join("a.rar"), &dest).unwrap_err().contains("not a .zip or .7z"));
        assert!(!dest.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn names_are_clean_and_the_choice_saves() {
        assert_eq!(clean("a|b:c"), "a_b_c");
        assert_eq!(clean(".."), "skin");
        assert_eq!(clean("None"), "skin");
        let chosen = parse(vec!["pack-evo:Red".into(), "IMPREZA03:none".into(), "bad".into()]);
        assert_eq!(join(&chosen).as_deref(), Some("IMPREZA03:none|pack-evo:Red"));
        assert_eq!(join(&BTreeMap::new()), None);
    }

    #[test]
    fn a_community_models_own_skins_are_offered_after_the_imported_ones() {
        let home = crate::test_dir("skin-model");
        let root = crate::test_dir("skin-model-root");
        for dir in
            [home.join(DIR).join("pack-x").join("Red"), root.join("Cars/x/skins/red"), root.join("Cars/x/skins/Blue")]
        {
            fs::create_dir_all(dir).unwrap();
        }
        let car = Car {
            key: "pack-x".into(),
            physics: Physics::Pack(PathBuf::from("/pack")),
            model: Ok(car_pack::Model { entry: car_pack::model_entry("x", "x"), fallback: None }),
            model_path: Some(root.join("Cars/x/x.sgc")),
            ..Car::default()
        };
        let skins = installed(&home, &car);
        assert_eq!(skins.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Blue", "Red"]);
        assert_eq!(skins[1].dir, home.join(DIR).join("pack-x").join("Red"));
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_stand_in_model_offers_no_skins() {
        let home = crate::test_dir("skin-stand-in");
        fs::create_dir_all(home.join(DIR).join("pack-x").join("Red")).unwrap();
        let mut car = Car { key: "pack-x".into(), ..Car::default() };
        assert!(installed(&home, &car).is_empty());
        car.model = Ok(car_pack::Model { entry: car_pack::model_entry("x", "x"), fallback: None });
        assert_eq!(installed(&home, &car).len(), 1);
        assert_eq!(
            choice(&home, &car, Some("Red")),
            Some(car_pack::Skin::Dir(home.join(DIR).join("pack-x").join("Red")))
        );
        assert_eq!(choice(&home, &car, Some(ORIGINAL)), Some(car_pack::Skin::Original));
        assert_eq!(choice(&home, &car, Some("Gone")), None);
        assert_eq!(choice(&home, &car, None), None);
        car.model = Ok(car_pack::Model { entry: car_pack::model_entry("x", "x"), fallback: Some("why".into()) });
        assert!(installed(&home, &car).is_empty());
        fs::remove_dir_all(&home).unwrap();
    }
}
