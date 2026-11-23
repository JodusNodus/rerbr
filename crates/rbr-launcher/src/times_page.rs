//! Library > Times: your best time on each stage by car class or by car, on each road, under one physics
//! (`times`, the log in `times.tsv`), and the Rally page's "Your best" line.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use egui::RichText;
use rbr_recording::physics::PhysicsId;

use crate::browser;
use crate::kit;
use crate::pages::Page;
use crate::theme::{self, TEXT, TEXT_2, TEXT_3};
use crate::times::query::{self, Best, Group, Key};
use crate::times::{self, log, Road, Run};
use crate::Form;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sort {
    #[default]
    Recent,
    Stage,
    Time,
}

impl Sort {
    const ALL: [Sort; 3] = [Sort::Recent, Sort::Stage, Sort::Time];

    fn label(self) -> &'static str {
        match self {
            Sort::Recent => "Recent",
            Sort::Stage => "Stage",
            Sort::Time => "Time",
        }
    }
}

/// The Times page's state and the log in memory.
#[derive(Default)]
pub struct TimesPage {
    runs: Vec<Run>,
    /// The player's folder the runs were read from.
    loaded: Option<PathBuf>,
    /// The log being built from the replays on a thread (there was none): the player's folder and the runs once done.
    building: Option<(PathBuf, Built)>,
    /// The replay of the drive running and what it steps (`Form::times_drive_started`).
    pending: Option<(PathBuf, PhysicsId)>,
    /// The physics chosen; none: the one a drive steps now.
    physics: Option<PhysicsId>,
    per_car: bool,
    query: String,
    class: Option<String>,
    road: Option<Road>,
    sort: Sort,
    selected: Option<Key>,
    scroll: (f32, f32),
}

/// The runs a thread has built, once it is done.
type Built = Arc<Mutex<Option<Vec<Run>>>>;

/// The widths of the right-aligned columns of the rows, per class and per car.
const CLASS_COLUMNS: [f32; 5] = [80.0, 46.0, 76.0, 130.0, 76.0];
const CAR_COLUMNS: [f32; 6] = [130.0, 80.0, 46.0, 76.0, 32.0, 76.0];

fn clock(ms: u32) -> String {
    rbr_render::hud::clock_label(f64::from(ms) / 1000.0)
}

fn age(recorded: u64) -> String {
    crate::replay_page::age(UNIX_EPOCH + Duration::from_secs(recorded))
}

impl TimesPage {
    /// The best of each key as the filters and the sort leave them.
    fn shown<'a>(&self, runs: &'a [Run], physics: &PhysicsId) -> Vec<Best<'a>> {
        let all = if self.per_car { query::best_per_car(runs, physics) } else { query::best_per_class(runs, physics) };
        let search = self.query.trim().to_lowercase();
        let mut shown: Vec<Best> = all
            .into_iter()
            .filter(|b| self.class.as_ref().is_none_or(|c| b.run.class == *c))
            .filter(|b| self.road.is_none_or(|r| b.run.road == r))
            .filter(|b| {
                search.is_empty()
                    || b.run.stage_name.to_lowercase().contains(&search)
                    || b.run.car_name.to_lowercase().contains(&search)
            })
            .collect();
        match self.sort {
            Sort::Recent => shown.sort_by_key(|b| std::cmp::Reverse(b.run.recorded)),
            Sort::Stage => shown.sort_by(|a, b| {
                (a.run.stage_name.to_lowercase(), a.run.stage, a.run.road, &a.run.class, &a.run.car_name).cmp(&(
                    b.run.stage_name.to_lowercase(),
                    b.run.stage,
                    b.run.road,
                    &b.run.class,
                    &b.run.car_name,
                ))
            }),
            Sort::Time => shown.sort_by_key(|b| b.run.time),
        }
        shown
    }

    /// The physics the page shows, with the ones to choose from: those in the log and `current`.
    fn physics_options(&self, current: &PhysicsId) -> (PhysicsId, Vec<(PhysicsId, String)>) {
        let mut ids: Vec<PhysicsId> = self.runs.iter().map(|r| r.physics.clone()).collect();
        ids.push(current.clone());
        ids.sort();
        ids.dedup();
        let options = ids.into_iter().map(|id| (id.clone(), id.label())).collect();
        (self.physics.clone().unwrap_or_else(|| current.clone()), options)
    }

    fn filter_bar(&mut self, ui: &mut egui::Ui, physics: &PhysicsId, options: &[(PhysicsId, String)]) {
        let mut classes: Vec<String> = self.runs.iter().map(|r| r.class.clone()).collect();
        classes.sort();
        classes.dedup();
        let classes: Vec<(Option<String>, String)> = std::iter::once((None, "All classes".to_owned()))
            .chain(classes.into_iter().map(|c| (Some(c.clone()), c)))
            .collect();
        let roads: Vec<(Option<Road>, String)> = std::iter::once((None, "All roads".to_owned()))
            .chain(Road::ALL.map(|r| (Some(r), r.label().to_owned())))
            .collect();
        let sorts: Vec<(Sort, String)> = Sort::ALL.map(|s| (s, format!("Sort: {}", s.label()))).to_vec();
        let labels = ["Best per class", "Best per car"];
        const DROPS: [f32; 4] = [190.0, 112.0, 96.0, 112.0];
        let sizes = (browser::segmented_width(ui, &labels), DROPS.iter().sum::<f32>() + 4.0 * 8.0, (180.0, 300.0));
        let (physics_text, class_text) = (physics.label(), self.class.clone().unwrap_or_else(|| "All classes".into()));
        let road_text = self.road.map_or("All roads", Road::label).to_owned();
        let sort_text = format!("Sort: {}", self.sort.label());
        let chosen = std::cell::RefCell::new(physics.clone());
        let before = (self.per_car, self.class.clone(), self.road);
        browser::filter_bar(
            ui,
            self,
            sizes,
            |s, ui, width| {
                browser::search_box(ui, "Search stage or car", &mut s.query, &mut false, width);
                kit::segmented(ui, "times view", &mut s.per_car, &[(false, labels[0]), (true, labels[1])]);
            },
            |s, ui| {
                let physics = |_: &mut Self, ui: &mut egui::Ui| {
                    browser::dropdown(ui, "times physics", DROPS[0], &physics_text, &mut *chosen.borrow_mut(), options)
                };
                let class = |s: &mut Self, ui: &mut egui::Ui| {
                    browser::dropdown(ui, "times class", DROPS[1], &class_text, &mut s.class, &classes)
                };
                let road = |s: &mut Self, ui: &mut egui::Ui| {
                    browser::dropdown(ui, "times road", DROPS[2], &road_text, &mut s.road, &roads)
                };
                let sort = |s: &mut Self, ui: &mut egui::Ui| {
                    browser::dropdown(ui, "times sort", DROPS[3], &sort_text, &mut s.sort, &sorts)
                };
                browser::in_order(ui, s, &[&physics, &class, &road, &sort]);
            },
        );
        let chosen = chosen.into_inner();
        if chosen != *physics {
            self.physics = Some(chosen);
            self.selected = None;
        }
        if before != (self.per_car, self.class.clone(), self.road) {
            self.selected = None;
        }
    }
}

/// A row: the stage, then the columns right to left of `widths`.
fn draw_row(ui: &egui::Ui, rect: egui::Rect, best: &Best, per_car: bool, (selected, hovered): (bool, bool)) {
    browser::paint_selection(ui, rect, selected, hovered);
    let (run, p) = (best.run, ui.painter());
    let widths: &[f32] = if per_car { &CAR_COLUMNS } else { &CLASS_COLUMNS };
    let (lefts, name_right) = browser::columns(rect, widths);
    let font = egui::FontId::proportional(13.0);
    let small = egui::FontId::proportional(12.0);
    let cell = |k: usize, text: &str, font: &egui::FontId, colour| {
        browser::paint_cell(p, rect, lefts[k], lefts[k] + widths[k], text, font.clone(), colour)
    };
    browser::paint_cell(p, rect, browser::name_x(rect), name_right, &run.stage_name, font.clone(), theme::TEXT_LIST);
    let runs = best.runs.to_string();
    if per_car {
        cell(0, &run.car_name, &small, TEXT_2);
        cell(1, &run.class, &small, TEXT_2);
        cell(2, run.road.label(), &small, TEXT_2);
        cell(3, &clock(run.time), &theme::mono(12.0), TEXT);
        cell(4, &runs, &small, TEXT_3);
        cell(5, &age(run.recorded), &small, TEXT_3);
    } else {
        cell(0, &run.class, &small, TEXT_2);
        cell(1, run.road.label(), &small, TEXT_2);
        cell(2, &clock(run.time), &theme::mono(12.0), TEXT);
        cell(3, &run.car_name, &small, TEXT_2);
        cell(4, &age(run.recorded), &small, TEXT_3);
    }
}

impl Form {
    /// Reads the times log for the current player folder. With none it is built from the replays on a thread (reading
    /// hundreds of them takes seconds from a cold disk): the runs are none until it is done.
    pub(crate) fn times_load(&mut self, ctx: &egui::Context) {
        let home = self.here();
        if let Some((folder, job)) = &self.times.building {
            let done = job.lock().unwrap_or_else(|e| e.into_inner()).take();
            match done {
                None => return,
                Some(runs) => {
                    self.times.loaded = Some(folder.clone());
                    self.times.runs = runs;
                    self.times.building = None;
                }
            }
        }
        if self.times.loaded.as_ref() == Some(&home) {
            return;
        }
        self.times.runs.clear();
        if home.join(log::FILE).exists() {
            self.times.runs = log::read(&home);
            self.times.loaded = Some(home);
            return;
        }
        let (job, names, ctx) = (Arc::new(Mutex::new(None)), log::Names::of(&self.catalog), ctx.clone());
        self.times.building = Some((home.clone(), job.clone()));
        std::thread::spawn(move || {
            let runs = log::backfill(&home, &names, None).unwrap_or_default();
            *job.lock().unwrap_or_else(|e| e.into_inner()) = Some(runs);
            ctx.request_repaint();
        });
    }

    /// A drive's start, with its arguments: its replay and the physics it steps, to log at its exit.
    pub(crate) fn times_drive_started(&mut self, args: &[String]) {
        let replay = args.windows(2).find(|pair| pair[0] == "--save-replay").map(|pair| PathBuf::from(&pair[1]));
        self.times.pending = replay.map(|path| (path, self.drive_physics().id()));
    }

    /// The viewer's exit: logs the drive's run when it finished.
    pub(crate) fn times_drive_closed(&mut self) {
        let Some((path, physics)) = self.times.pending.take().filter(|_| self.times.building.is_none()) else {
            return;
        };
        let names = log::Names::of(&self.catalog);
        let Some(run) = Run::from_replay(&path, &names, Some(&physics)) else { return };
        let home = self.here();
        match log::append(&home, &names, &run) {
            // Read again when the page is next used if the log was just built from the replays.
            Ok(()) if self.times.loaded.as_ref() == Some(&home) => self.times.runs.push(run),
            Ok(()) => {}
            Err(e) => self.status = format!("Cannot save the time: {e}"),
        }
    }

    /// "Your best: …" for the selected stage and the car's class under the physics a drive steps: on the weather's road
    /// when it is known, else the best on any road.
    pub(crate) fn best_line(&mut self, ctx: &egui::Context) -> Option<String> {
        self.times_load(ctx);
        let stage = self.catalog.maps.get(self.map)?.number;
        let class = times::class_of(self.catalog.cars.get(self.car)?.group.as_deref());
        let known = self.options.weather != rbr_gameplay::weather::Preset::Random
            || self.weather_row.is_some()
            || self.clouds.is_some();
        let road =
            known.then(|| self.selection().ok()).flatten().and_then(|(_, weather)| Road::parse(weather.mat_words().1));
        let best = query::best_for(&self.times.runs, &self.drive_physics().id(), stage, &class, road)?;
        Some(format!("Your best: {} · {} · {}", clock(best.time), best.class, best.road.label()))
    }

    pub fn times_page(&mut self, ui: &mut egui::Ui) {
        self.times_load(ui.ctx());
        let home = self.here();
        let (physics, options) = self.times.physics_options(&self.drive_physics().id());
        let runs = std::mem::take(&mut self.times.runs);
        kit::page_header(ui, "Times", Some("Your best times, kept separately for each physics."), |_| {});
        self.times.filter_bar(ui, &physics, &options);
        let shown = self.times.shown(&runs, &physics);
        let building = self.times.building.is_some();
        let per_car = self.times.per_car;
        let keys = browser::Keys::take(ui);
        let at = shown.iter().position(|b| Some(&b.key) == self.times.selected.as_ref());
        let moved = (keys.movement() != 0 && !shown.is_empty()).then(|| {
            let to = at.map_or(0, |a| (a as isize + keys.movement()).clamp(0, shown.len() as isize - 1) as usize);
            self.times.selected = Some(shown[to].key.clone());
            to
        });
        if at.is_none() && moved.is_none() {
            self.times.selected = shown.first().map(|b| b.key.clone());
        }
        let selected = shown.iter().find(|b| Some(&b.key) == self.times.selected.as_ref());
        let ready = self.child.is_none() && self.readiness().is_none();
        let (mut watch, mut drive) = (None, None);
        browser::inspector(
            ui,
            |ui| {
                let Some(best) = selected else { return };
                let history = query::runs_of(&runs, &physics, &best.key);
                self.times_inspector(ui, best, &history, &physics);
            },
            |ui| {
                let Some(best) = selected else { return };
                let replay = home.join("replays").join(&best.run.replay);
                let can_drive = self.drive_target(best.run).is_some();
                let tip = if can_drive { "Enter" } else { "The car or the stage is not installed" };
                if browser::drive_button(ui, "Drive again", can_drive, tip).clicked() || (keys.enter && can_drive) {
                    drive = Some(best.run.clone());
                }
                if !best.run.replay.is_empty() && replay.exists() {
                    ui.add_space(6.0);
                    let width = ui.available_width();
                    let button = egui::Button::new("Watch replay").min_size(egui::vec2(width, 30.0));
                    if ui.add_enabled(ready, button).clicked() {
                        watch = Some(replay);
                    }
                }
            },
        );
        let text = format!("{} shown", shown.len());
        // The same profile's runs on another version: where an empty view's times went.
        let earlier = runs.iter().filter(|r| r.physics.profile == physics.profile && r.physics != physics);
        let earlier = earlier
            .max_by_key(|r| r.recorded)
            .map(|r| (r.physics.label(), runs.iter().filter(|o| o.physics == r.physics).count()));
        let mut clicked = None;
        browser::hint_bar(ui, &text, &[("↑↓", "Move"), ("↵", "Drive again")], |ui| {
            if shown.is_empty() {
                ui.add_space(10.0);
                let empty = if building {
                    "Reading your saved drives…".to_owned()
                } else {
                    let none = format!("No times under {} yet", physics.label());
                    match &earlier {
                        Some((label, count)) => format!("{none}.\n{count} earlier runs are under {label}."),
                        None => none,
                    }
                };
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new(empty).color(TEXT_2));
                });
                return;
            }
            if per_car {
                let titles = [
                    ("Car", false),
                    ("Class", false),
                    ("Road", false),
                    ("Time", false),
                    ("Runs", false),
                    ("Age", false),
                ];
                browser::list_header(ui, "Stage", &CAR_COLUMNS, &titles);
            } else {
                let titles = [("Class", false), ("Road", false), ("Time", false), ("Car", false), ("Age", false)];
                browser::list_header(ui, "Stage", &CLASS_COLUMNS, &titles);
            }
            let (offset, visible) = self.times.scroll;
            let mut area = egui::ScrollArea::vertical().id_salt("times-list").auto_shrink([false, false]);
            if let Some(to) = moved {
                let top = to as f32 * browser::ROW;
                let want = if top < offset {
                    top
                } else if top + browser::ROW > offset + visible {
                    top + browser::ROW - visible
                } else {
                    offset
                };
                area = area.vertical_scroll_offset(want);
            }
            let output = area.show_rows(ui, browser::ROW, shown.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for best in &shown[range] {
                    let size = egui::vec2(ui.available_width(), browser::ROW);
                    let (line, response) = ui.allocate_exact_size(size, egui::Sense::CLICK);
                    let rect = line.shrink2(egui::vec2(12.0, 1.0));
                    let on = self.times.selected.as_ref() == Some(&best.key);
                    draw_row(ui, rect, best, per_car, (on, response.hovered()));
                    if response.clicked() {
                        clicked = Some(best.key.clone());
                    }
                }
            });
            self.times.scroll = (output.state.clamped_offset().y, output.inner_rect.height());
        });
        if clicked.is_some() {
            self.times.selected = clicked;
        }
        self.times.runs = runs;
        if let Some(path) = watch {
            self.watch_replay(path);
        }
        if let Some(run) = drive {
            self.drive_again(&run);
        }
    }

    /// The catalog's car and map of `run`, when both are installed.
    fn drive_target(&self, run: &Run) -> Option<(usize, usize)> {
        let car = self.catalog.cars.iter().position(|c| c.key == run.car_key)?;
        let map = self.catalog.maps.iter().position(|m| m.number == run.stage)?;
        Some((car, map))
    }

    /// Sets the Rally page's car and stage to those of `run` and opens it.
    fn drive_again(&mut self, run: &Run) {
        if let Some((car, map)) = self.drive_target(run) {
            self.choose_car(car);
            self.choose_stage(map);
            self.page = Page::Drive;
        }
    }

    fn times_inspector(&self, ui: &mut egui::Ui, best: &Best, history: &[&Run], physics: &PhysicsId) {
        let run = best.run;
        let line = match &best.key.group {
            Group::Class(class) => format!("Best in {class}, by {}", run.car_name),
            Group::Car(_) => format!("{} · {}", run.car_name, run.class),
        };
        browser::title(ui, &run.stage_name, &line);
        ui.add_space(14.0);
        kit::panel(ui, |rows| {
            rows.row("Stage", |ui| _ = ui.add(egui::Label::new(&run.stage_name).truncate()));
            rows.row("Car", |ui| _ = ui.add(egui::Label::new(&run.car_name).truncate()));
            rows.row("Class", |ui| _ = ui.label(&run.class));
            rows.row("Road", |ui| _ = ui.label(run.road.label()));
            rows.row("Physics", |ui| _ = ui.label(physics.label()));
            rows.row("Time", |ui| _ = ui.label(RichText::new(clock(run.time)).font(theme::mono(12.0)).color(TEXT)));
            rows.row("Penalties", |ui| {
                ui.label(if run.penalties == 0 {
                    "None".to_owned()
                } else {
                    format!("{} included", clock(run.penalties))
                })
            });
            rows.row("Splits", |ui| {
                let splits: Vec<String> = run.splits.iter().map(|&s| clock(s)).collect();
                let text = if splits.is_empty() { "None".to_owned() } else { splits.join("  ") };
                ui.add(egui::Label::new(RichText::new(text).font(theme::mono(11.0))).truncate())
            });
        });
        ui.add_space(14.0);
        kit::group_label(ui, "Your runs", None);
        kit::panel(ui, |rows| {
            for run in history.iter().take(20) {
                rows.row(&times::date(run.recorded), |ui| {
                    ui.label(RichText::new(clock(run.time)).font(theme::mono(12.0)).color(TEXT))
                });
            }
        });
        if history.len() > 20 {
            kit::hint(ui, &format!("and {} slower", history.len() - 20));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(recorded: u64, stage: &str, car: &str, class: &str, road: Road, time: u32) -> Run {
        Run {
            recorded,
            stage: 1,
            stage_name: stage.into(),
            car_key: car.into(),
            car_name: car.into(),
            class: class.into(),
            road,
            physics: PhysicsId::parse("ngp@7.4.778").unwrap(),
            time,
            penalties: 0,
            splits: vec![],
            replay: String::new(),
        }
    }

    #[test]
    fn the_filters_and_the_sort_pick_and_order_the_bests() {
        let runs = vec![
            run(1, "Torsby", "Evo", "Group A", Road::Dry, 300_000),
            run(2, "Aalto", "Evo", "Group A", Road::Wet, 200_000),
            run(3, "Aalto", "Fiesta", "R5", Road::Dry, 250_000),
        ];
        let ngp = PhysicsId::parse("ngp@7.4.778").unwrap();
        let names = |page: &TimesPage| -> Vec<String> {
            page.shown(&runs, &ngp).iter().map(|b| format!("{}/{}", b.run.stage_name, b.run.car_name)).collect()
        };
        let mut page = TimesPage::default();
        assert_eq!(names(&page), ["Aalto/Fiesta", "Aalto/Evo", "Torsby/Evo"]);
        page.sort = Sort::Stage;
        assert_eq!(names(&page), ["Aalto/Fiesta", "Aalto/Evo", "Torsby/Evo"]);
        page.sort = Sort::Time;
        assert_eq!(names(&page), ["Aalto/Evo", "Aalto/Fiesta", "Torsby/Evo"]);
        page.road = Some(Road::Dry);
        assert_eq!(names(&page), ["Aalto/Fiesta", "Torsby/Evo"]);
        page.class = Some("R5".into());
        page.query = " fiesta".into();
        assert_eq!(names(&page), ["Aalto/Fiesta"]);
        page.query = "evo".into();
        assert!(names(&page).is_empty());
        let other = PhysicsId::parse("experimental@0.1.0").unwrap();
        assert!(TimesPage::default().shown(&runs, &other).is_empty());
        let (shown, options) = TimesPage { runs, ..Default::default() }.physics_options(&other);
        assert_eq!(shown, other);
        assert_eq!(options.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(), ["ReRBR engine 0.1.0", "NGP 7.4.778"]);
    }
}
