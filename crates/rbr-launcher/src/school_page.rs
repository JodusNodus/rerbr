//! The School page: the rally school's 17 lessons (flow mode 7, `rbr_school`), each on its map with the lesson's
//! tyre in the Drive page's car (`rbr-standalone --school N`). Ours: the original drives retail car 5 with the
//! lesson's setup; a car slot names no particular car outside a vanilla install, so any car drives a lesson.
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
use rbr_school::Lesson;

use crate::browser::{DIM, GREY, STAR};
use crate::catalog::car_label;
use crate::drive_page::{column, split_columns, Summary};
use crate::kit;
use crate::pages::{Issue, Page};
use crate::theme::{self, white, GOOD, LINE, PANEL, ROW_SELECTED, TEXT, TEXT_2, TEXT_3, TEXT_LIST};
use crate::Form;

/// The page's state: the English string table, the selected lesson, the lesson times and the player's results.
pub struct SchoolPage {
    strings: Option<Strings>,
    selected: u32,
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
            times: HashMap::new(),
            results: None,
        }
    }

    fn text(&self, key: &str) -> String {
        crate::championship_page::common::text_of(self.strings.as_ref(), key)
    }

    /// The lesson's times, read once per map from its `.dls` ([`lesson_times`]).
    fn times(&mut self, data: &Data, lesson: Lesson) -> Option<(f32, f32)> {
        let map = lesson.map();
        *self.times.entry(map).or_insert_with(|| lesson_times(data, lesson))
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
    let n = lesson.index() - group_start(lesson) + 1;
    let key = format!("RSM_LESSON_{}{n}", if lesson.index() < Lesson::FIRST_ADVANCED { "NRB" } else { "NRA" });
    strings
        .and_then(|s| s.get(&key))
        .map(|t| t.replace('\n', " ").trim().trim_end_matches('.').to_owned())
        .unwrap_or_else(|| format!("Lesson {n}"))
}

/// The first lesson of `lesson`'s group.
fn group_start(lesson: Lesson) -> u32 {
    if lesson.index() < Lesson::FIRST_ADVANCED {
        0
    } else {
        Lesson::FIRST_ADVANCED
    }
}

/// The lessons of `lesson`'s group.
fn group_lessons(lesson: Lesson) -> impl Iterator<Item = Lesson> {
    let first = group_start(lesson);
    Lesson::all().filter(move |l| group_start(*l) == first)
}

/// The player's best result: 0 is not passed, 1..3 passed with the grade, 4 perfect.
pub fn result_text(grade: u32) -> String {
    match grade {
        0 => "Not passed".to_owned(),
        4 => "Perfect".to_owned(),
        n @ 1..=3 => format!("Passed {n}/4"),
        n => format!("{n}/4"),
    }
}

/// The result's colour: gold for perfect, green for passed, grey for not passed.
fn result_colour(grade: u32) -> Color32 {
    match grade {
        0 => GREY,
        4 => STAR,
        _ => GOOD,
    }
}

/// A grade as four pips in `rect`, `grade` of them filled in its colour; nothing for a lesson not passed.
fn paint_pips(painter: &egui::Painter, rect: egui::Rect, grade: u32) {
    if grade > 0 {
        let colour = result_colour(grade);
        for k in 0..4u32 {
            let centre = egui::pos2(rect.left() + PIP_STEP * (k as f32 + 0.5), rect.center().y);
            if k < grade {
                painter.circle_filled(centre, PIP_RADIUS, colour);
            } else {
                painter.circle_stroke(centre, PIP_RADIUS, egui::Stroke::new(1.0, DIM));
            }
        }
    }
}

/// A grade as four pips; nothing for a lesson not passed.
fn grade_pips(ui: &mut egui::Ui, grade: u32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(4.0 * PIP_STEP, PIP_STEP), egui::Sense::hover());
    paint_pips(ui.painter(), rect, grade);
    response.on_hover_text(result_text(grade))
}

/// A lesson's row of the list: its number, title and grade; the selection is filled.
fn lesson_row(ui: &mut egui::Ui, number: usize, title: &str, grade: u32, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 29.0), egui::Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 6, ROW_SELECTED);
    } else if response.hovered() {
        painter.rect_filled(rect, 6, white(10));
    }
    let middle = rect.left_center();
    painter.text(
        middle + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        number.to_string(),
        theme::mono(11.5),
        TEXT_3,
    );
    let colour = if selected { Color32::WHITE } else { TEXT_LIST };
    let font = egui::FontId::proportional(13.0);
    painter.text(middle + egui::vec2(42.0, 0.0), egui::Align2::LEFT_CENTER, title, font, colour);
    let pips = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 10.0 - 4.0 * PIP_STEP, rect.center().y - PIP_STEP / 2.0),
        egui::vec2(4.0 * PIP_STEP, PIP_STEP),
    );
    paint_pips(painter, pips, grade);
    if grade > 0 {
        response.on_hover_text(result_text(grade))
    } else {
        response
    }
}

/// The pips' spacing and radius (points).
const PIP_STEP: f32 = 11.0;
const PIP_RADIUS: f32 = 3.5;

/// A cone guide line's colour, by the cone colour it starts with (`Red cones - Lift off throttle`).
fn cone_colour(line: &str) -> Color32 {
    let word = line.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    match word.as_str() {
        "red" => Color32::from_rgb(220, 70, 60),
        "green" => Color32::from_rgb(80, 190, 90),
        "yellow" => Color32::from_rgb(235, 200, 60),
        "blue" => Color32::from_rgb(80, 140, 230),
        "white" => Color32::from_gray(225),
        _ => GREY,
    }
}

/// The cone guide, one line a cone colour with its marker.
fn cone_guide(ui: &mut egui::Ui, text: &str) {
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
            let (top, base) = (rect.center_top(), rect.bottom());
            let points = vec![top, egui::pos2(rect.right(), base), egui::pos2(rect.left(), base)];
            ui.painter().add(egui::Shape::convex_polygon(points, cone_colour(line), egui::Stroke::NONE));
            ui.label(line);
        });
    }
}

/// What the lesson plays before the drive, or instead of it.
fn lesson_flow(lesson: Lesson) -> &'static str {
    if lesson.ride() {
        "Ride along while Richard drives the stage (Enter skips). The ride counts as perfect."
    } else if lesson.demonstrated() {
        "Richard shows the lesson first (Enter skips it), then you drive it."
    } else {
        "No demonstration: you drive the lesson straight from the brief."
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
        .unwrap_or_default()
}

impl Form {
    /// The School page: the lessons in their two groups with the best results, and the selected lesson's detail
    /// with Drive lesson.
    pub fn school_page(&mut self, ui: &mut egui::Ui) {
        let data = self.data.clone();
        let saves = SaveDir::new(&self.records());
        let profile = self.championship.profile.clone();
        let blocker = self.school_blocker();
        let car = self.car().map_or_else(|| "none".to_owned(), car_label);
        let (mut drive, mut to_drive_page) = (None, false);
        let grades = self.school.results(&saves, &profile);
        let grade = |lesson: Lesson| grades[lesson.index() as usize];
        let selected = Lesson::new(self.school.selected).unwrap_or(Lesson::new(0).unwrap());
        kit::page_header(ui, "School", Some("Richard teaches the rally driving skills, one lesson at a time."), |_| {});
        let summary = {
            let school = &self.school;
            let sub = format!(
                "{} · {} · {}",
                lesson_label(school.strings.as_ref(), selected),
                group_of(selected).0,
                surface_of(selected)
            );
            Summary { title: school.text(selected.title_key()), sub, problem: None, hover: None }
        };
        let label = if selected.ride() { "Ride along" } else { "Drive lesson" };
        if self.launch_footer(ui, "school footer", summary, blocker.clone(), label, None) {
            drive = Some(selected);
        }
        let school = &mut self.school;
        kit::body(ui, |ui| {
            let rects = split_columns(ui.available_rect_before_wrap(), 2);
            column(ui, rects[0], "school list", |ui| {
                for first in [Lesson::new(0).unwrap(), Lesson::new(Lesson::FIRST_ADVANCED).unwrap()] {
                    let (group, key) = group_of(first);
                    let passed = group_lessons(first).filter(|l| grade(*l) > 0).count();
                    let total = group_lessons(first).count();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(group).size(12.0).family(theme::medium()).color(TEXT_2));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("{passed} of {total} passed")).size(12.0).color(TEXT_3));
                        });
                    });
                    kit::hint(ui, &school.text(key));
                    egui::Frame::new()
                        .fill(PANEL)
                        .stroke(egui::Stroke::new(1.0, LINE))
                        .corner_radius(10)
                        .inner_margin(4)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for lesson in group_lessons(first) {
                                let i = lesson.index();
                                let title = school.text(lesson.title_key());
                                let number = (i - group_start(lesson) + 1) as usize;
                                let row = lesson_row(ui, number, &title, grade(lesson), school.selected == i);
                                if row.clicked() {
                                    school.selected = i;
                                }
                                if row.double_clicked() && blocker.is_none() {
                                    drive = Some(lesson);
                                }
                            }
                        });
                }
            });
            column(ui, rects[1], "school detail", |ui| {
                let lesson = selected;
                let (instruction, cones) = lesson.brief_keys();
                let (group, _) = group_of(lesson);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(
                        RichText::new(format!("{} · {group}", lesson_label(school.strings.as_ref(), lesson)))
                            .size(12.0)
                            .color(TEXT_2),
                    );
                    ui.label(
                        RichText::new(school.text(lesson.title_key())).size(17.0).family(theme::semibold()).color(TEXT),
                    );
                });
                ui.label(school.text(&instruction));
                cone_guide(ui, &school.text(&cones));
                kit::panel(ui, |rows| {
                    rows.row("Surface", |ui| _ = ui.label(surface_of(lesson)));
                    let times = school.times(&data, lesson).map_or_else(
                        || "Untimed".to_owned(),
                        |(perfect, pass)| format!("pass {}  ·  perfect {}", seconds(pass), seconds(perfect)),
                    );
                    rows.row("Times", |ui| _ = ui.label(times));
                    rows.row("Best", |ui| {
                        ui.colored_label(result_colour(grade(lesson)), result_text(grade(lesson)));
                        if grade(lesson) > 0 {
                            grade_pips(ui, grade(lesson));
                        }
                    });
                    rows.row("Car", |ui| {
                        to_drive_page = kit::link(ui, "Change").clicked();
                        ui.add(egui::Label::new(RichText::new(&car).color(TEXT)).truncate())
                            .on_hover_text("The Rally page's car, on its default setup.");
                    });
                });
                kit::hint(ui, lesson_flow(lesson));
            });
        });
        if to_drive_page {
            self.page = Page::Drive;
        }
        if let Some(lesson) = drive {
            self.start_school(lesson);
        }
    }

    /// What blocks Drive lesson: a running viewer, what the Drive page's Start checks, or no car on the Drive page.
    fn school_blocker(&self) -> Option<Issue> {
        if self.child.is_some() {
            return Some(Issue { text: "the viewer is running".into(), fix: None });
        }
        if let Some(issue) = self.readiness() {
            return Some(issue);
        }
        self.car().is_none().then(|| Issue { text: "no car on the Drive page".into(), fix: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
        // Without the string tables, each group counts from 1 as the tables' `RSM_LESSON_NRB`/`NRA` keys do.
        assert_eq!(lesson_label(None, Lesson::new(0).unwrap()), "Lesson 1");
        assert_eq!(lesson_label(None, Lesson::new(5).unwrap()), "Lesson 6");
        assert_eq!(lesson_label(None, Lesson::new(6).unwrap()), "Lesson 1");
        assert_eq!(lesson_label(None, Lesson::new(16).unwrap()), "Lesson 11");
        let counts: Vec<usize> =
            [0, Lesson::FIRST_ADVANCED].map(|i| group_lessons(Lesson::new(i).unwrap()).count()).into();
        assert_eq!(counts, [6, 11]);
    }

    /// The table's "Lesson 1." loses its period (`RBR_DATA`), so the label reads cleanly before the group name.
    #[test]
    fn lesson_labels_drop_the_tables_period() {
        let Some(data) = Data::from_env() else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        let strings = rbr_formats::strings::english(&data).unwrap();
        for lesson in Lesson::all() {
            let label = lesson_label(Some(&strings), lesson);
            assert!(label.starts_with("Lesson ") && !label.ends_with('.'), "{lesson:?}: {label:?}");
        }
    }

    #[test]
    fn cone_lines_take_their_cones_colour() {
        assert_eq!(cone_colour("Red cones - Lift off throttle"), Color32::from_rgb(220, 70, 60));
        assert_eq!(cone_colour("green cones - Brake"), Color32::from_rgb(80, 190, 90));
        assert_eq!(cone_colour("Yellow rectangle - End lesson"), Color32::from_rgb(235, 200, 60));
        assert_eq!(cone_colour(""), GREY);
    }

    /// The detail says what comes before the drive: the ride, a demonstration, or neither.
    #[test]
    fn the_lesson_flow_names_the_demonstration_and_the_ride() {
        assert!(lesson_flow(Lesson::new(5).unwrap()).starts_with("Ride along"));
        assert!(lesson_flow(Lesson::new(2).unwrap()).starts_with("Richard shows"));
        assert!(lesson_flow(Lesson::new(0).unwrap()).starts_with("No demonstration"));
    }

    #[test]
    fn grades_come_from_the_profiles_school_array() {
        let saves = SaveDir::new(&crate::test_dir("school-grades"));
        assert_eq!(school_grades(&saves, "nobody"), [0; 17]);
        let mut file = rbr_formats::saves::profile::ProfileFile::fresh("driver");
        file.profiles.list[0].school[2] = 3;
        file.profiles.list[0].school[7] = 4;
        saves.save_profile("driver", &file).unwrap();
        let grades = school_grades(&saves, "driver");
        assert_eq!((grades[2], grades[7], grades[0]), (3, 4, 0));
        assert_eq!([0, 1, 3, 4].map(result_text), ["Not passed", "Passed 1/4", "Passed 3/4", "Perfect"]);
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
        // As the launcher loads it (`settings::form`): the school maps are not listed.
        let mut catalog = crate::catalog::Catalog::scan(&install).unwrap();
        catalog.hide_unlisted();
        for lesson in Lesson::all() {
            let map = lesson.map();
            assert!(catalog.maps.iter().all(|m| m.number != map), "map {map} listed");
            let draws = Draws { row: None, wear: 0 };
            let (stage, weather) = catalog.stage_weather(&season, map, Preset::Good, draws).expect("good weather");
            assert_eq!(stage.name, format!("track-{map}_M"), "{lesson:?}");
            assert_eq!(weather.letter_char(), 'M', "{lesson:?}");
        }
    }
}
