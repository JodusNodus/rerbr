//! The School page: the rally school's 17 lessons (flow mode 7, `rbr_school`), each on its map with the lesson's
//! tyre and car 5 with the lesson's setup (`rbr-standalone --school N`), or the Drive page's car on its own setup
//! (ours: the original drives car 5 only).
//!
//! Every lesson is open (user decision: no unlocks). Each row shows the lesson number (`RSM_LESSON_NRB1`..`NRB6`
//! for the basic lessons, `RSM_LESSON_NRA1`..`NRA11` for the advanced) and its title (`Lesson::title_key`), with
//! the player's best result. The detail shows the instruction (`brief_keys().0`), the cone guide
//! (`brief_keys().1`), the surface (from the lesson's setup file), the pass and perfect times, and Drive lesson.
//!
//! The times come from the lesson map's `.dls`: the `Driveline` animation set's replay-stream header (words 4 and
//! 5, floats, the perfect and pass times; both 0 is untimed), the same record `rbr_formats::markers` reads the
//! stage markers from. The results come from the profile's school array (`Profile::school`, the original's
//! `E69Profile +0x424`): 0 not passed, 1..3 passed with the grade, 4 perfect.

use std::collections::HashMap;
use std::time::SystemTime;

use egui::{Color32, RichText};

use rbr_formats::data::Data;
use rbr_formats::saves::{Kind, SaveDir};
use rbr_formats::strings::Strings;
use rbr_school::lesson::CAR;
use rbr_school::Lesson;

use crate::catalog::{car_label, Car, Catalog};
use crate::pages::{section, Fix, Issue, ACCENT, DRIVE_FIELDS, WARN};
use crate::Form;

/// The school's car (retail car 5, `IMPREZA03`) for the viewer and `rbr-standalone` (`--school` drives it).
pub(crate) fn school_car(catalog: &Catalog) -> Option<Car> {
    catalog.all_cars.iter().find(|c| c.rsf.is_none() && c.index == CAR).cloned()
}

/// The page's state: the English string table, the selected lesson and car, the lesson times and the player's
/// results.
pub struct SchoolPage {
    strings: Option<Strings>,
    selected: u32,
    /// The lesson is driven in the Drive page's car instead of its own.
    drive_car: bool,
    /// The lesson maps' times, by map: the perfect and pass times, or `None` when untimed.
    times: HashMap<u32, Option<(f32, f32)>>,
    results: Option<CachedResults>,
}

/// The profile's school results, re-read when the profile or its file changes.
struct CachedResults {
    profile: String,
    mtime: Option<SystemTime>,
    grades: [u32; 17],
}

impl SchoolPage {
    pub fn new(data: &Data) -> SchoolPage {
        SchoolPage {
            strings: rbr_formats::strings::english(data).ok(),
            selected: 0,
            drive_car: false,
            times: HashMap::new(),
            results: None,
        }
    }

    fn text(&self, key: &str) -> String {
        self.strings.as_ref().and_then(|s| s.get(key)).unwrap_or(key).to_owned()
    }

    /// The lesson's times, read once per map from its `.dls` ([`lesson_times`]).
    fn times(&mut self, data: &Data, lesson: Lesson) -> Option<(f32, f32)> {
        let map = lesson.map();
        if let Some(cached) = self.times.get(&map) {
            return *cached;
        }
        let times = lesson_times(data, lesson);
        self.times.insert(map, times);
        times
    }

    /// The profile's school grades, re-read when the profile or its file changes.
    fn results(&mut self, saves: &SaveDir, profile: &str) -> [u32; 17] {
        let mtime =
            std::fs::metadata(saves.path.join(Kind::Profile.file_name(profile))).and_then(|m| m.modified()).ok();
        if let Some(cached) = &self.results {
            if cached.profile == profile && cached.mtime == mtime {
                return cached.grades;
            }
        }
        let grades = school_grades(saves, profile);
        self.results = Some(CachedResults { profile: profile.to_owned(), mtime, grades });
        grades
    }
}

/// The lesson's group: 0-5 basic (`RSCD_BASIC`), 6-16 advanced (`RSCD_ADV`). The keys hold the courses'
/// description sentences, so the headers stay plain "Basic"/"Advanced" with the sentences under them.
fn group_of(lesson: Lesson) -> (&'static str, &'static str) {
    if lesson.index() < Lesson::FIRST_ADVANCED {
        ("Basic", "RSCD_BASIC")
    } else {
        ("Advanced", "RSCD_ADV")
    }
}

/// The lesson's number label: `RSM_LESSON_NRB1`..`NRB6` for the basic lessons, `RSM_LESSON_NRA1`..`NRA11` for the
/// advanced (the tables have no `A7`, like [`Lesson::brief_keys`]).
pub fn lesson_label(strings: Option<&Strings>, lesson: Lesson) -> String {
    let key = if lesson.index() < Lesson::FIRST_ADVANCED {
        format!("RSM_LESSON_NRB{}", lesson.index() + 1)
    } else {
        format!("RSM_LESSON_NRA{}", lesson.index() - Lesson::FIRST_ADVANCED + 1)
    };
    strings
        .and_then(|s| s.get(&key))
        .map(|t| t.replace('\n', " "))
        .unwrap_or_else(|| format!("Lesson {}", lesson.index() + 1))
}

/// The player's best result: 0 is not passed, 1..3 passed with the grade, 4 perfect.
pub fn result_text(grade: u32) -> String {
    match grade {
        0 => "-".to_owned(),
        4 => "Perfect".to_owned(),
        n @ 1..=3 => format!("Passed {n}/4"),
        n => format!("{n}/4"),
    }
}

/// The lesson's surface, from its setup file (tarmac on the tarmac lessons, gravel on the others).
pub fn surface_of(lesson: Lesson) -> &'static str {
    if lesson.setup_file().ends_with("tarmac.lsp") {
        "Tarmac"
    } else {
        "Gravel"
    }
}

/// A time in seconds, as the game's school texts write it (`RSM_TIMELIMITA6` is "Pass Time: 10s").
fn seconds(time: f32) -> String {
    format!("{} s", time as u32)
}

/// The lesson's perfect and pass times from its map's `.dls`: the `Driveline` animation set's replay-stream
/// header words 4 and 5 (floats); both 0 is untimed (`None`).
pub fn lesson_times(data: &Data, lesson: Lesson) -> Option<(f32, f32)> {
    let path = data.resolve(&format!("Maps/track-{}_M.dls", lesson.map()))?;
    let bytes = std::fs::read(&path).ok()?;
    let file = rbr_formats::dls::parse(&bytes).ok()?;
    let header = driveline_replay(&file)?;
    let (perfect, pass) = (f32::from_bits(header[4]), f32::from_bits(header[5]));
    (perfect != 0.0 || pass != 0.0).then_some((perfect, pass))
}

/// The `Driveline` animation set's replay-stream header. Like `rbr_formats::markers`' entry lookup: `fields[0]` is
/// the set name's offset into the string table, compared case-insensitively, the first match taken.
fn driveline_replay(file: &rbr_formats::dls::DlsFile<'_>) -> Option<[u32; 7]> {
    file.animation_entries.iter().find_map(|entry| {
        let name = file.string_bytes.get(entry.fields[0] as usize..)?;
        let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        name[..end].eq_ignore_ascii_case(b"Driveline").then_some(entry.replay.header)
    })
}

/// The profile's school grades (`Profile::school`, index = lesson), all 0 without a profile.
fn school_grades(saves: &SaveDir, profile: &str) -> [u32; 17] {
    saves
        .load_profile(profile)
        .ok()
        .and_then(|file| file.current().cloned().or_else(|| file.profiles.list.first().cloned()))
        .map(|entry| entry.school)
        .unwrap_or([0; 17])
}

impl Form {
    /// The School page: the lessons in their two groups with the best results, and the selected lesson's detail
    /// with Drive lesson.
    pub fn school_page(&mut self, ui: &mut egui::Ui) {
        let data = self.data.clone();
        let saves = SaveDir::new(&self.records());
        let profile = self.rally.profile.clone();
        let blocker = self.school_blocker();
        let lesson_car = school_car(&self.catalog).map_or_else(|| "car 5, not installed".to_owned(), |c| car_label(&c));
        let drive_car = self.car().map_or_else(|| "none".to_owned(), car_label);
        let mut drive = None;
        {
            let school = &mut self.school;
            let grades = school.results(&saves, &profile);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(DRIVE_FIELDS);
                    egui::ScrollArea::vertical().id_salt("school").show(ui, |ui| {
                        for (group, key) in [group_of(Lesson::new(0).unwrap()), group_of(Lesson::new(16).unwrap())] {
                            section(ui, group);
                            ui.colored_label(Color32::GRAY, school.text(key));
                            for i in 0..Lesson::COUNT {
                                let lesson = Lesson::new(i).unwrap();
                                if group_of(lesson).0 != group {
                                    continue;
                                }
                                let title = school.text(lesson.title_key());
                                let row = format!("{} · {title}", lesson_label(school.strings.as_ref(), lesson));
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut school.selected, i, row);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(result_text(grades[i as usize]));
                                    });
                                });
                            }
                        }
                    });
                });
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    let lesson = Lesson::new(school.selected).unwrap_or(Lesson::new(0).unwrap());
                    let title = school.text(lesson.title_key());
                    let (instruction, cones) = lesson.brief_keys();
                    ui.label(RichText::new(&title).size(16.5).strong());
                    ui.colored_label(Color32::GRAY, lesson_label(school.strings.as_ref(), lesson));
                    ui.add_space(4.0);
                    ui.label(school.text(&instruction));
                    ui.add_space(4.0);
                    ui.label(format!("Cones: {}", school.text(&cones)));
                    ui.add_space(8.0);
                    crate::pages::fields(ui, "school-detail", |ui| {
                        ui.label("Surface");
                        ui.label(surface_of(lesson));
                        ui.end_row();
                        let times = school.times(&data, lesson);
                        ui.label("Pass time");
                        ui.label(times.map_or_else(|| "None".to_owned(), |(_, pass)| seconds(pass)));
                        ui.end_row();
                        ui.label("Perfect time");
                        ui.label(times.map_or_else(|| "None".to_owned(), |(perfect, _)| seconds(perfect)));
                        ui.end_row();
                        ui.label("Best");
                        ui.label(result_text(grades[lesson.index() as usize]));
                        ui.end_row();
                        ui.label("Car");
                        ui.vertical(|ui| {
                            ui.radio_value(&mut school.drive_car, false, format!("The lesson's: {lesson_car}"));
                            ui.radio_value(&mut school.drive_car, true, format!("The Drive page's: {drive_car}"))
                                .on_hover_text(
                                    "Ours: the original drives the lesson's car. Yours runs on its own setup.",
                                );
                        });
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    let button =
                        egui::Button::new(RichText::new("Drive lesson").size(20.0).strong().color(Color32::WHITE))
                            .fill(ACCENT)
                            .min_size(egui::vec2(180.0, 44.0));
                    if ui.add_enabled(blocker.is_none(), button).clicked() {
                        drive = Some(lesson);
                    }
                    if let Some(issue) = &blocker {
                        ui.colored_label(WARN, &issue.text);
                    }
                });
            });
        }
        if let Some(lesson) = drive {
            self.start_school(lesson);
        }
    }

    /// The car a lesson is driven in: the Drive page's when chosen, else the lesson's (car 5).
    pub(crate) fn school_drive_car(&self) -> Option<Car> {
        if self.school.drive_car {
            self.car().cloned()
        } else {
            school_car(&self.catalog)
        }
    }

    /// What blocks Drive lesson: a running viewer, what the Drive page's Start checks, or the lesson's car 5 without
    /// an NGP car.
    fn school_blocker(&self) -> Option<Issue> {
        if self.child.is_some() {
            return Some(Issue { text: "the viewer is running".into(), fix: None });
        }
        if let Some(issue) = self.readiness() {
            return Some(issue);
        }
        if self.school.drive_car {
            return self.car().is_none().then(|| Issue { text: "no car on the Drive page".into(), fix: None });
        }
        (!self.ngp_cars.contains(&CAR)).then(|| Issue {
            text: "the rally school drives car 5, which has no NGP car in this archive".into(),
            fix: Some(Fix::Settings),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The player's folder's records, for the tests.
    fn test_records() -> (PathBuf, SaveDir) {
        let dir = crate::test_dir("school-grades");
        let saves = SaveDir::new(&dir);
        (dir, saves)
    }

    #[test]
    fn groups_split_at_the_first_advanced_lesson() {
        assert_eq!(Lesson::FIRST_ADVANCED, 6);
        assert_eq!(group_of(Lesson::new(0).unwrap()).0, "Basic");
        assert_eq!(group_of(Lesson::new(5).unwrap()).0, "Basic");
        assert_eq!(group_of(Lesson::new(6).unwrap()).0, "Advanced");
        assert_eq!(group_of(Lesson::new(16).unwrap()).0, "Advanced");
        assert_eq!(Lesson::new(0).unwrap().brief_keys(), ("B1SI".into(), "B1SA".into()));
        assert_eq!(Lesson::new(16).unwrap().brief_keys(), ("A12SI".into(), "A12SA".into()));
    }

    #[test]
    fn result_text_names_the_grades() {
        assert_eq!(result_text(0), "-");
        assert_eq!(result_text(1), "Passed 1/4");
        assert_eq!(result_text(3), "Passed 3/4");
        assert_eq!(result_text(4), "Perfect");
    }

    #[test]
    fn surfaces_follow_the_setup_files() {
        for lesson in Lesson::all() {
            let tarmac = lesson.setup_file().ends_with("tarmac.lsp");
            assert_eq!(surface_of(lesson), if tarmac { "Tarmac" } else { "Gravel" }, "{lesson:?}");
        }
        assert_eq!(surface_of(Lesson::new(10).unwrap()), "Tarmac");
        assert_eq!(surface_of(Lesson::new(2).unwrap()), "Gravel");
    }

    #[test]
    fn lesson_labels_number_the_groups() {
        assert_eq!(lesson_label(None, Lesson::new(0).unwrap()), "Lesson 1");
        assert_eq!(lesson_label(None, Lesson::new(5).unwrap()), "Lesson 6");
        assert_eq!(lesson_label(None, Lesson::new(6).unwrap()), "Lesson 7");
        assert_eq!(lesson_label(None, Lesson::new(16).unwrap()), "Lesson 17");
    }

    #[test]
    fn grades_come_from_the_profiles_school_array() {
        let (_dir, saves) = test_records();
        assert_eq!(school_grades(&saves, "nobody"), [0; 17]);
        let mut file = rbr_formats::saves::profile::ProfileFile::fresh("driver");
        file.profiles.list[0].school[2] = 3;
        file.profiles.list[0].school[7] = 4;
        saves.save_profile("driver", &file).unwrap();
        let grades = school_grades(&saves, "driver");
        assert_eq!((grades[2], grades[7], grades[0]), (3, 4, 0));
        assert_eq!((result_text(grades[2]).as_str(), result_text(grades[7]).as_str()), ("Passed 3/4", "Perfect"));
    }

    /// The lesson maps' times from their `.dls` files (`RBR_DATA`): lesson 2 (map 83) 9/10 s, lesson 0 (map 81)
    /// untimed, lesson 16 (map 74) 80/84 s.
    #[test]
    fn lesson_times_come_from_the_maps_driveline_streams() {
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        assert_eq!(lesson_times(&data, Lesson::new(2).unwrap()), Some((9.0, 10.0)));
        assert_eq!(lesson_times(&data, Lesson::new(0).unwrap()), None);
        assert_eq!(lesson_times(&data, Lesson::new(16).unwrap()), Some((80.0, 84.0)));
        for lesson in Lesson::all() {
            let path = data.resolve(&format!("Maps/track-{}_M.dls", lesson.map())).expect("a school map's .dls");
            assert!(path.is_file(), "{}", path.display());
        }
    }

    /// Every lesson runs on good weather on its map's `M` variant (`RBR_DATA`): what `start_school` resolves.
    #[test]
    fn every_lesson_has_its_stage_on_good_weather() {
        use rbr_gameplay::weather::{Draws, Preset};
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        let install = std::env::var_os("RBR_DATA").map(PathBuf::from).unwrap();
        let season = rbr_gameplay::weather::Season::load(&data).unwrap();
        let catalog = Catalog::scan(&install).unwrap();
        for lesson in Lesson::all() {
            let map = lesson.map();
            let draws = Draws { row: None, wear: 0 };
            let (stage, weather) = catalog.stage_weather(&season, map, Preset::Good, draws).expect("good weather");
            assert_eq!(stage.name, format!("track-{map}_M"), "{lesson:?}");
            assert_eq!(weather.letter_char(), 'M', "{lesson:?}");
        }
    }
}
