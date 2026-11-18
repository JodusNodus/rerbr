//! The Drive page: the quick race's car and stage, conditions, race options, the previews and Start.

use egui::{Align, Color32, ComboBox, CornerRadius, Frame, Key, Layout, Margin, Rect, RichText, Sense, Stroke, Ui};
use rbr_gameplay::race_options::{DamageLevel, Skill};
use rbr_gameplay::weather::{self, Preset, Weather};
use rbr_render::glass::model::Switch;
use rbr_render::shifter::{CarBox, Engagement};

use crate::browser::{self, STAR};
use crate::car_browser::{self, Target};
use crate::catalog::{self, TyreChoice};
use crate::ghosts::{self, GhostChoice};
use crate::kit::{self, Rows};
use crate::pages::{combo, Choice, Issue, Page};
use crate::settings::{Clouds, Fog, Precipitation, SEQUENTIAL};
use crate::stage_browser::{self, Action, StageRow};
use crate::theme::{self, ERROR, LINE, PANEL, TEXT, TEXT_2, TEXT_3, TEXT_LABEL, WARN};
use crate::time_of_day::{self, StartTime};
use crate::Form;

/// The gap between a page's columns.
const GAP: f32 = 24.0;
/// A dropdown's width in a row of a column.
const DROPDOWN: f32 = 150.0;

/// The Weather combo's choice: a game preset, or one season row of the map (its section).
#[derive(Debug, Clone, PartialEq, Eq)]
enum WeatherSel {
    Preset(Preset),
    Row(String),
}

/// A Custom weather look: our clouds, rain or snow and fog, and the road's wetness (0 dry, 1 damp, 2 wet).
#[derive(Clone, Copy)]
struct Look {
    name: &'static str,
    /// The name on a snow stage, where it snows.
    snowy: &'static str,
    clouds: Clouds,
    rain: Precipitation,
    fog: Fog,
    wetness: u32,
}

const fn look(names: [&'static str; 2], clouds: Clouds, rain: Precipitation, fog: Fog, wetness: u32) -> Look {
    Look { name: names[0], snowy: names[1], clouds, rain, fog, wetness }
}

impl Look {
    fn name(self, snowy: bool) -> &'static str {
        if snowy {
            self.snowy
        } else {
            self.name
        }
    }
}

/// The Custom weather's looks, the second ([`Form::apply_look`]) the one Custom starts on.
const LOOKS: [Look; 7] = [
    look(["Sunny"; 2], Clouds::Clear, Precipitation::None, Fog::Off, 0),
    look(["Fair"; 2], Clouds::Partly, Precipitation::None, Fog::Off, 0),
    look(["Cloudy"; 2], Clouds::Light, Precipitation::None, Fog::Off, 0),
    look(["Overcast"; 2], Clouds::Heavy, Precipitation::None, Fog::Off, 0),
    look(["Drizzle", "Flurries"], Clouds::Heavy, Precipitation::Light, Fog::Light, 1),
    look(["Rain", "Snow"], Clouds::Heavy, Precipitation::Heavy, Fog::Off, 2),
    look(["Foggy"; 2], Clouds::Light, Precipitation::None, Fog::Heavy, 1),
];

impl Form {
    /// Sets the Custom weather to `look` (the Stage weather's choice is kept for going back to it).
    fn apply_look(&mut self, look: Look) {
        self.clouds = Some(look.clouds);
        self.precipitation = Some(look.rain);
        self.fog = Some(look.fog);
        self.wetness = Some(look.wetness);
    }

    /// The Custom look the clouds, rain, fog and wetness make, `None` for an own mix.
    fn look(&self) -> Option<&'static str> {
        let now = (self.clouds, self.precipitation, self.fog, self.wetness);
        let look = LOOKS.iter().find(|l| now == (Some(l.clouds), Some(l.rain), Some(l.fog), Some(l.wetness)));
        look.map(|l| l.name(self.snowy()))
    }

    /// Whether what falls is snow: a snow stage's (the viewer's emitter, `Precipitation::chosen`).
    fn snowy(&self) -> bool {
        self.catalog.maps[self.map].surface() == 2
    }

    /// For the screenshots: the Custom weather on the Rain look at the default time of day, and with `adjust` its controls open on an own mix.
    pub(crate) fn screenshot_custom_weather(&mut self, adjust: bool) {
        self.apply_look(LOOKS[5]);
        self.start_time = StartTime::default();
        self.weather_adjust = adjust;
        if adjust {
            self.precipitation = Some(Precipitation::Light);
        }
    }
}

/// A row of a panel with a hover text on its label.
pub(crate) fn labelled<R>(rows: &mut Rows, label: &str, tip: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    rows.full(|ui| {
        ui.label(RichText::new(label).color(TEXT_LABEL)).on_hover_text(tip);
        ui.with_layout(Layout::right_to_left(Align::Center), control).inner
    })
}

/// `rect` split into `n` columns side by side.
pub(crate) fn split_columns(rect: Rect, n: usize) -> Vec<Rect> {
    // Whole points, so the columns come out equally wide on screen and their cards' pictures equally tall.
    let width = ((rect.width() - GAP * (n - 1) as f32) / n as f32).floor();
    (0..n)
        .map(|k| {
            let left = (rect.left() + k as f32 * (width + GAP)).round();
            Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(left + width, rect.bottom()))
        })
        .collect()
}

/// A column of `rect`, scrolling on its own when it overflows.
pub(crate) fn column(ui: &mut Ui, rect: Rect, id: &str, content: impl FnOnce(&mut Ui)) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        content(ui)
    });
}

/// The text under a card's picture.
#[derive(Default)]
pub(crate) struct CardText {
    pub meta: String,
    pub notes: Vec<(String, Color32)>,
}

/// A card: the picture (given its width), then the name and a line of facts under a divider; whether it was clicked.
pub(crate) fn card(
    ui: &mut Ui,
    favourite: bool,
    name: &str,
    picture: impl FnOnce(&mut Ui, f32, CornerRadius) -> CardText,
) -> bool {
    let shown = Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(10).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        ui.set_width(width);
        let text = picture(ui, width, CornerRadius { nw: 9, ne: 9, sw: 0, se: 0 });
        let top = ui.available_rect_before_wrap().top();
        ui.painter().hline(ui.max_rect().x_range(), top, Stroke::new(1.0, LINE));
        Frame::new().inner_margin(Margin::symmetric(14, 12)).show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.spacing_mut().item_spacing.y = 3.0;
            let mut job = egui::text::LayoutJob::default();
            let font = egui::FontSelection::FontId(egui::FontId::new(15.0, theme::semibold()));
            if favourite {
                RichText::new("★ ").color(STAR).append_to(&mut job, ui.style(), font.clone(), Align::Min);
            }
            RichText::new(name).color(TEXT).append_to(&mut job, ui.style(), font, Align::Min);
            ui.add(egui::Label::new(job).truncate());
            ui.add(egui::Label::new(RichText::new(&text.meta).size(12.5).color(TEXT_2)).truncate());
            for (note, colour) in &text.notes {
                ui.add(egui::Label::new(RichText::new(note).size(12.0).color(*colour)).wrap());
            }
        });
    });
    let id = ui.scope_id().with(("card", name));
    ui.interact(shown.response.rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// What the footer shows left of the actions.
pub(crate) struct Summary {
    pub title: String,
    pub sub: String,
    /// Why there is nothing to drive, in place of `sub`.
    pub problem: Option<String>,
    /// The title's hover text.
    pub hover: Option<String>,
}

/// The experimental physics note in a footer (added right to left: the link first); whether Change was clicked.
fn experimental_note(ui: &mut Ui, text: &str) -> bool {
    let change = kit::link(ui, "Change").on_hover_text(crate::physics::NO_RECORDS).clicked();
    ui.label(RichText::new(text).size(12.0).color(TEXT_LABEL)).on_hover_text(crate::physics::NO_RECORDS);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 16.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, WARN);
    change
}

impl Form {
    /// The pinned footer of Rally, Circuit and School: the summary, what blocks the action with its fix, the
    /// experimental note, and the primary action (with a key `hint`: Enter too, and Stop while the viewer runs). True when the action was asked for.
    pub(crate) fn launch_footer(
        &mut self,
        ui: &mut Ui,
        id: &str,
        summary: Summary,
        blocker: Option<Issue>,
        label: &str,
        hint: Option<&str>,
    ) -> bool {
        let note = !self.drive_physics().keeps_records();
        let mut pressed = false;
        kit::footer(ui, id, |ui| {
            // What blocks the action, with its fix; the last event (`status`) is on the sidebar's status line.
            if let Some(issue) = blocker.as_ref().filter(|_| self.child.is_none() || hint.is_none()) {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(WARN, &issue.text);
                    if let Some(fix) = issue.fix {
                        if ui.button(fix.label()).clicked() {
                            self.apply(fix);
                        }
                    }
                });
                ui.add_space(8.0);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let button = ui.add_enabled(blocker.is_none(), kit::primary_button(label, hint));
                let button = if hint.is_some() { button.on_hover_text("Enter starts too") } else { button };
                // Enter on a focused control presses it instead.
                let enter =
                    hint.is_some() && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(Key::Enter));
                pressed = blocker.is_none() && (button.clicked() || enter);
                let stop = egui::Button::new(RichText::new("Stop").size(14.0)).min_size(egui::vec2(80.0, 38.0));
                if hint.is_some() && self.child.is_some() && ui.add(stop).clicked() {
                    {
                        // Our own child: the viewer, which ends its stepping process with it.
                        if let Some(child) = &mut self.child {
                            let _ = child.kill();
                        }
                        self.crash.stopped = true;
                    }
                }
                ui.add_space(12.0);
                if note && experimental_note(ui, "Experimental physics: results aren't saved") {
                    self.page = Page::Settings;
                }
                ui.add_space(12.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let title = ui.add(
                            egui::Label::new(RichText::new(&summary.title).family(theme::medium()).color(TEXT))
                                .truncate(),
                        );
                        if let Some(hover) = &summary.hover {
                            title.on_hover_text(hover);
                        }
                        match &summary.problem {
                            Some(problem) => {
                                _ = ui.add(egui::Label::new(RichText::new(problem).size(12.0).color(WARN)).truncate())
                            }
                            None => {
                                _ = ui.add(
                                    egui::Label::new(RichText::new(&summary.sub).size(12.0).color(TEXT_2)).truncate(),
                                )
                            }
                        };
                    });
                });
            });
        });
        pressed
    }

    /// A page's body of `n` columns, each given its rect.
    pub(crate) fn browser_frame(&mut self, ui: &mut Ui, page: fn(&mut Form, &mut Ui)) {
        // The browsers keep the narrower padding of the pages not yet restyled.
        ui.add_space(14.0);
        Frame::new().inner_margin(Margin { left: 14, right: 4, top: 0, bottom: 8 }).show(ui, |ui| page(self, ui));
    }

    /// The Rally page: the car, the stage and the conditions in three columns, Start pinned below.
    pub fn drive_page(&mut self, ui: &mut Ui) {
        if self.stage_browser.open && self.stage_browser.target == Target::Drive {
            return self.browser_frame(ui, Form::stage_browser_page);
        }
        if self.car_browser.open && self.car_browser.target == Target::Drive {
            return self.browser_frame(ui, Form::car_browser_page);
        }
        kit::page_header(ui, "Rally", Some("Drive one stage in any car."), |_| {});
        let summary = self.rally_summary();
        let blocker = self.blocker();
        if self.launch_footer(ui, "rally footer", summary, blocker, "Start", Some("↵")) {
            self.start();
        }
        kit::body(ui, |ui| {
            let rects = split_columns(ui.available_rect_before_wrap(), 3);
            column(ui, rects[0], "rally car", |ui| {
                self.car_column(ui, Target::Drive);
                self.car_rows(ui);
            });
            column(ui, rects[1], "rally stage", |ui| self.stage_column(ui));
            column(ui, rects[2], "rally conditions", |ui| {
                self.conditions_column(ui, "Stage's own", false);
            });
        });
    }

    /// The summary of the drive: car and stage, then the stage's facts and the conditions.
    fn rally_summary(&self) -> Summary {
        let map = &self.catalog.maps[self.map];
        let car = self.car_browser.row(self.car).map(|r| r.title.clone());
        let title = match car {
            Some(car) => format!("{car} · {}", map.name()),
            None => map.name().to_owned(),
        };
        match self.selection() {
            Ok((stage, weather)) => {
                let mut parts = Vec::new();
                if let Some(row) = self.stage_browser.row(self.map) {
                    parts.extend(row.km.map(|km| format!("{km:.1} km")));
                    parts.push(stage_browser::SURFACES[row.surface as usize].to_owned());
                }
                parts.push(self.conditions_summary(&weather));
                Summary { title, sub: parts.join(" · "), problem: None, hover: Some(stage.name.clone()) }
            }
            Err(reason) => Summary { title, sub: String::new(), problem: Some(reason), hover: None },
        }
    }

    /// The conditions in a few words: the weather, the time of day when it is ours, the road.
    pub(crate) fn conditions_summary(&self, weather: &Weather) -> String {
        let wear = catalog::WEARS[weather.wear.min(2) as usize];
        let mut parts = vec![self.weather_name()];
        if self.custom() {
            parts.push(self.start_time.label().to_owned());
        }
        parts.push(weather.mat_words().1.to_owned());
        parts.push(format!("{wear} wear"));
        parts.join(" · ")
    }

    /// The Car column: its card, the favourites and, with the H-shifter, the gearbox.
    pub(crate) fn car_column(&mut self, ui: &mut Ui, target: Target) {
        kit::group_label(ui, "Car", None);
        if self.catalog.cars.is_empty() {
            ui.colored_label(WARN, "No cars yet: add some on Library > Cars with Get more cars…");
            return;
        }
        let Some(selected) = self.car_browser.row(self.car) else { return };
        let (title, detail) = (selected.title.clone(), selected.detail());
        let favourite = self.favourite_cars.contains(&selected.name);
        let root = self.game_root();
        let open = card(ui, favourite, &title, |ui, width, radius| {
            let car = &self.catalog.cars[self.car];
            let error = self.previews.car_picture(ui, car, self.skins.choice(car), &root, width, radius);
            let mut notes = Vec::new();
            notes.extend(error.map(|e| (format!("No car model: {e}"), ERROR)));
            if let Some(stand_in) = car.model.as_ref().ok().and_then(|m| m.fallback.as_ref()) {
                notes.push((format!("Stand-in model: {stand_in}"), TEXT_3));
            }
            CardText { meta: detail, notes }
        });
        if open {
            self.car_browser.open_on(target, self.car, &self.favourite_cars, None);
        }
    }

    /// The gearbox and setup rows of the car.
    pub(crate) fn car_rows(&mut self, ui: &mut Ui) {
        if self.car().is_none() {
            return;
        }
        self.select_setup();
        let mut edit = false;
        kit::panel(ui, |rows| {
            if self.bindings.shifter.advanced {
                self.gearbox_row(rows);
            }
            let tip = "Any saved setup of the car, the stage surface's first; the pick is kept for the car and the \
                       surface. Edit… makes and changes them";
            labelled(rows, "Setup", tip, |ui| {
                let width = (ui.available_width() - 60.0).clamp(120.0, 230.0);
                ui.allocate_ui_with_layout(egui::vec2(width, 26.0), Layout::left_to_right(Align::Center), |ui| {
                    edit = self.setup.choice_ui(ui, width);
                });
            });
        });
        if edit {
            self.open_setup();
        }
    }

    /// The advanced H-shifter's gearbox of the car, kept for the car.
    fn gearbox_row(&mut self, rows: &mut Rows) {
        let Some(car) = self.car().map(|c| c.name.clone()) else { return };
        let tip = "The car's gearbox for the advanced H-shifter, kept for this car. Sequential: the gear up and down \
                   actions, with the game's gearbox aids. H-pattern: the shifter's gears need the clutch or matched \
                   revs, without Clutch Help and the automatic gearbox; dog gears go in without the clutch within a \
                   wider range of revs than synchro ones";
        let mut gearbox = self.gearboxes.of(&car);
        labelled(rows, "Gearbox", tip, |ui| {
            _ = combo(ui, "gearbox", DROPDOWN, &mut gearbox, &GEARBOXES, gearbox_label)
        });
        if gearbox != self.gearboxes.of(&car) {
            self.gearboxes.set(&car, gearbox);
        }
    }

    /// The Stage column: its card, the favourites, and the race's ghost and damage.
    fn stage_column(&mut self, ui: &mut Ui) {
        kit::group_label(ui, "Stage", None);
        let Some(selected) = self.stage_browser.row(self.map) else { return };
        let (name, favourite) = (selected.name.clone(), self.favourite_maps.contains(&selected.number));
        let open = card(ui, favourite, &name, |ui, width, radius| {
            let map = &self.catalog.maps[self.map];
            match self.previews.stage_picture(ui, map, width, radius) {
                Some((meta, problem)) => CardText {
                    meta,
                    notes: problem.map(|p| (format!("Cannot load: {p}"), ERROR)).into_iter().collect(),
                },
                None => CardText::default(),
            }
        });
        if open {
            self.stage_browser.open_on(self.map, &self.favourite_maps, None);
        }
        kit::group_label(ui, "Race", None);
        kit::panel(ui, |rows| {
            self.ghost_row(rows);
            self.damage_rows(rows);
        });
    }

    /// The Conditions column: the weather and the time of day, then Road and tyres. `fold` folds Road and tyres
    /// into one row (the Circuit page).
    pub(crate) fn conditions_column(&mut self, ui: &mut Ui, own: &str, fold: bool) {
        kit::group_label(ui, "Conditions", None);
        kit::panel(ui, |rows| {
            self.conditions_rows(rows, own);
            if fold {
                self.folded_road_rows(rows);
            }
        });
        if !fold {
            kit::group_label(ui, "Road and tyres", None);
            kit::panel(ui, |rows| self.road_rows(rows));
        }
    }

    /// Road and tyres as one row that opens the real controls (the Circuit page).
    fn folded_road_rows(&mut self, rows: &mut Rows) {
        let summary = match self.selection() {
            Ok((_, weather)) => {
                let wear = catalog::WEARS[weather.wear.min(2) as usize];
                let tyre = match self.tyre {
                    TyreChoice::Auto => "Auto".to_owned(),
                    TyreChoice::Fixed(t) => catalog::tyre_label(t).to_owned(),
                };
                sentence(&format!("{} · {} · {tyre}", weather.mat_words().1, sentence(wear)))
            }
            Err(_) => String::new(),
        };
        let open = self.circuit.road_open;
        let arrow = if open { "▼" } else { "▶" };
        let clicked = rows.full(|ui| {
            let label =
                ui.add(egui::Label::new(RichText::new("Road and tyres").color(TEXT_LABEL)).sense(Sense::click()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let arrow =
                    ui.add(egui::Label::new(RichText::new(arrow).size(10.0).color(TEXT_3)).sense(Sense::click()));
                let text = ui.add(egui::Label::new(RichText::new(&summary).color(TEXT_2)).sense(Sense::click()));
                label.clicked() || arrow.clicked() || text.clicked()
            })
            .inner
        });
        if clicked {
            self.circuit.road_open = !open;
        }
        if open {
            self.road_rows(rows);
        }
    }

    /// The weather and the time of day.
    fn conditions_rows(&mut self, rows: &mut Rows, own: &str) {
        // A saved ghost brings its own weather (`E18_ConnGhostMemory::Dispatch`): the wetness and the wear
        // follow it, while the time of day and our sky stay editable.
        let ghost = self.ghost_weather();
        let tip = "Stage weather: the game's presets (good, bad, or random as the game draws it from the stage's \
                   season), or one of the stage's own season rows, exactly as authored. Custom: our sky over the \
                   stage's good weather, from a few looks or set by hand (needs the Sky and Clouds additions)";
        let mut custom = self.clouds.is_some();
        labelled(rows, "Weather", tip, |ui| {
            _ = kit::segmented(ui, "weather mode", &mut custom, &[(false, own), (true, "Custom")])
        });
        if custom != self.clouds.is_some() {
            // Custom's other choices stay for going back to it (the Stage weather does not read them).
            if custom {
                self.apply_look(LOOKS[1]);
            } else {
                self.clouds = None;
            }
        }

        if custom {
            let base = weather::name(self.custom_base());
            let tip = format!(
                "Sets the clouds, the rain or snow, the fog and the road's wetness, over the stage's {base} weather: \
                 its baked light and road; the bad weather's on a damp or wet road"
            );
            let snowy = self.snowy();
            let looks = self.look();
            labelled(rows, "Sky", &tip, |ui| {
                ui.toggle_value(&mut self.weather_adjust, "Adjust…");
                let mut picked = None;
                ComboBox::from_id_salt("sky look").width(110.0).selected_text(looks.unwrap_or("Own mix")).show_ui(
                    ui,
                    |ui| {
                        for look in LOOKS {
                            let name = look.name(snowy);
                            if ui.selectable_label(looks == Some(name), name).clicked() {
                                picked = Some(look);
                            }
                        }
                    },
                );
                if let Some(look) = picked {
                    self.apply_look(look);
                }
            });
            rows.hint(&format!("over the stage's {base} weather"));
            if self.weather_adjust {
                labelled(rows, "Clouds", "", |ui| {
                    combo(ui, "clouds", DROPDOWN, &mut self.clouds, &Clouds::ALL.map(Some), |clouds| {
                        clouds.map_or("Clear", Clouds::label)
                    });
                });
                let (falls, tip) = if snowy {
                    ("Snow", "What falls and is heard")
                } else {
                    ("Rain", "What falls and is heard; snow on a snow stage")
                };
                labelled(rows, falls, tip, |ui| {
                    combo(
                        ui,
                        "precipitation",
                        DROPDOWN,
                        &mut self.precipitation,
                        &Precipitation::ALL.map(Some),
                        |rain| rain.unwrap_or(Precipitation::None).label(),
                    );
                });
                let tip = "Ours, over any weather (needs the Sky addition in Graphics), not the game's: off, light, \
                           moderate or heavy";
                labelled(rows, "Fog", tip, |ui| {
                    combo(ui, "fog", DROPDOWN, &mut self.fog, &Fog::ALL.map(Some), |fog| {
                        fog.unwrap_or(Fog::Off).label()
                    });
                });
            }
        } else {
            labelled(rows, "Stage weather", "", |ui| {
                if let Some((_, weather)) = &ghost {
                    ui.label(format!("from the ghost: {}", weather_label(weather)));
                    return;
                }
                let map = &self.catalog.maps[self.map];
                let season = map.season(&self.season);
                let season_rows = catalog::environment_rows(&season, map.number);
                let mut selected = match self.weather_row.clone() {
                    Some(section) if season_rows.iter().any(|(_, s, _)| s == &section) => WeatherSel::Row(section),
                    _ => WeatherSel::Preset(self.options.weather),
                };
                let selected_text = match &selected {
                    WeatherSel::Preset(preset) => weather::name(*preset).to_owned(),
                    WeatherSel::Row(section) => season_rows
                        .iter()
                        .find(|(_, s, _)| s == section)
                        .map(|(_, _, label)| label.clone())
                        .unwrap_or_else(|| weather::name(self.options.weather).to_owned()),
                };
                ComboBox::from_id_salt("weather").width(DROPDOWN).selected_text(selected_text).show_ui(ui, |ui| {
                    for preset in [Preset::Good, Preset::Random, Preset::Bad] {
                        ui.selectable_value(&mut selected, WeatherSel::Preset(preset), weather::name(preset));
                    }
                    for (_, section, label) in &season_rows {
                        ui.selectable_value(&mut selected, WeatherSel::Row(section.clone()), label);
                    }
                });
                match selected {
                    WeatherSel::Preset(preset) => {
                        self.options.weather = preset;
                        self.weather_row = None;
                    }
                    WeatherSel::Row(section) => self.weather_row = Some(section),
                }
            });
        }

        if custom {
            let tip = "Ours: relights the stage at a moment of its rally's day, solved from where the sun stands on \
                       that date: the sun, its light and shadows, and dusk and night lit by the car's lamps. As baked \
                       is the sun the stage's light was baked with";
            labelled(rows, "Time of day", tip, |ui| _ = self.time_of_day_box(ui).on_hover_text(tip));
            let site = time_of_day::site(&self.catalog.maps[self.map]);
            let start = self.start_time.minutes(site);
            rows.full(|ui| {
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    time_of_day::sun_arc(ui, ui.available_width(), site, start);
                    ui.add_space(8.0);
                });
            });
        }
    }

    /// The road, the tyre and the wipers.
    fn road_rows(&mut self, rows: &mut Rows) {
        // The Stage weather's wetness, and the good and random weather's wear, are the variant's: not shown.
        let ghost = self.ghost_weather();
        let tip = "The road's wetness over the weather's: its grip, the viewer's road, the auto tyre and \
                   rbr-standalone follow it";
        if let Some((_, weather)) = &ghost {
            labelled(rows, "Wetness", tip, |ui| {
                _ = ui.label(format!("following the ghost: {}", weather.mat_words().1))
            });
        } else if self.custom() {
            let mut wetness = self.wetness.unwrap_or(0).min(2);
            labelled(rows, "Wetness", tip, |ui| {
                kit::segmented(ui, "wetness", &mut wetness, &[(0, "Dry"), (1, "Damp"), (2, "Wet")]);
            });
            if Some(wetness) != self.wetness {
                self.wetness = Some(wetness);
            }
        }

        // A specific row, the bad weather and the Custom weather take the chosen wear.
        let wear_tip = "The road surface: new, normal or worn (the stage's grip table and textures)";
        if let Some((_, weather)) = &ghost {
            labelled(rows, "Wear", wear_tip, |ui| {
                _ = ui.label(format!("following the ghost: {}", weather.mat_words().0))
            });
        } else if self.custom() || self.weather_row.is_some() || self.options.weather == Preset::Bad {
            let names = catalog::WEARS.map(sentence);
            let options: Vec<(u32, &str)> = (0..3).map(|i| (i, names[i as usize].as_str())).collect();
            labelled(rows, "Wear", wear_tip, |ui| _ = kit::segmented(ui, "wear", &mut self.wear, &options));
        }

        let default_tyre = self.selection().ok().and_then(|(_, weather)| self.tyre_id(&weather).ok());
        labelled(rows, "Tyres", "Auto takes the game's tyre for the surface and the road's wetness", |ui| {
            combo(ui, "tyre", 170.0, &mut self.tyre, &TYRES, |tyre| match tyre {
                TyreChoice::Auto => default_tyre
                    .map_or_else(|| "Auto".to_owned(), |tyre| format!("Auto — {}", catalog::tyre_label(tyre))),
                TyreChoice::Fixed(fixed) => catalog::tyre_label(fixed).to_owned(),
            });
        });

        let tip = "The wiper switch at the start; auto follows the rain and the mud on the glass. I steps it while \
                   driving";
        labelled(rows, "Wipers", tip, |ui| {
            combo(ui, "wipers", DROPDOWN, &mut self.wipers, &Switch::ALL, |switch| sentence(switch.name()));
        });
    }

    /// The Car damage row, with what calling for help costs.
    pub(crate) fn damage_rows(&mut self, rows: &mut Rows) {
        let penalty = self.options.assist_settings().penalty();
        let tip = format!("Off, Safe, Reduced or Realistic. Calling for help costs {penalty} s at this level");
        labelled(rows, "Car damage", &tip, |ui| {
            combo(ui, "damage", 120.0, &mut self.options.damage, &DamageLevel::ALL, DamageLevel::label)
                .on_hover_text(&tip);
        });
        rows.hint(&format!("Calling for help costs {penalty} s at this level"));
    }

    /// Makes car `index` of the catalog the car, and the most recent pick.
    pub(crate) fn choose_car(&mut self, index: usize) {
        self.car = index;
        browser::remember(&mut self.recent_cars, self.catalog.cars[index].name.clone());
    }

    /// The car browser in place of the page's body: the Drive page's car, or the car a new season drives.
    pub(crate) fn car_browser_page(&mut self, ui: &mut egui::Ui) {
        let root = self.game_root();
        let env = car_browser::Env {
            cars: &self.catalog.cars,
            favourites: &self.favourite_cars,
            recent: &self.recent_cars,
            previews: &mut self.previews,
            skins: &mut self.skins,
            root: &root,
            heading: String::new(),
        };
        let target = self.car_browser.target;
        match self.car_browser.ui(ui, env) {
            Some(car_browser::Action::Choose(index)) => {
                match target {
                    Target::Drive => self.choose_car(index),
                    Target::Championship => self.championship.pick_car(index, &self.catalog.cars),
                    Target::Circuit => self.choose_car(index),
                }
                self.car_browser.open = false;
            }
            Some(car_browser::Action::Favourite(name)) => match self.favourite_cars.iter().position(|n| *n == name) {
                Some(at) => drop(self.favourite_cars.remove(at)),
                None => self.favourite_cars.push(name),
            },
            Some(car_browser::Action::Back) => self.car_browser.open = false,
            // The Cars page's.
            Some(
                car_browser::Action::Remove(_)
                | car_browser::Action::GetMore
                | car_browser::Action::GetMoreSkins
                | car_browser::Action::Setups(_),
            )
            | None => {}
        }
    }

    /// Makes map `index` of the catalog the stage, and the most recent pick.
    pub(crate) fn choose_stage(&mut self, index: usize) {
        self.map = index;
        browser::remember(&mut self.recent_maps, self.catalog.maps[index].number);
        self.validate_weather_row();
    }

    /// Drops the specific weather row when it is not the new stage's: the choice falls back to Good.
    pub(crate) fn validate_weather_row(&mut self) {
        let map = &self.catalog.maps[self.map];
        let valid = self
            .weather_row
            .as_deref()
            .is_some_and(|section| catalog::row_by_section(&map.season(&self.season), map.number, section).is_some());
        if !valid && self.weather_row.is_some() {
            self.weather_row = None;
            self.options.weather = Preset::Good;
        }
    }

    /// The Weather choice as the summary shows it: the specific row's label, else the preset's name (a saved
    /// ghost brings its own weather, as the row below the fields says).
    fn weather_name(&self) -> String {
        if self.ghost_weather().is_some() {
            return weather::name(self.options.weather).to_owned();
        }
        if self.clouds.is_some() {
            return self.look().unwrap_or("custom").to_owned();
        }
        if let Some(section) = &self.weather_row {
            let map = &self.catalog.maps[self.map];
            let season = map.season(&self.season);
            if let Some(k) = catalog::row_by_section(&season, map.number, section) {
                return catalog::row_label(&season.rows[k]);
            }
        }
        weather::name(self.options.weather).to_owned()
    }

    /// The stage browser in place of the page's body.
    pub(crate) fn stage_browser_page(&mut self, ui: &mut egui::Ui) {
        let records = self.records();
        let (catalog, season) = (&self.catalog, &self.season);
        let facts = |row: &StageRow| {
            let weather = |preset| {
                let found = catalog.stage_weather(season, row.number, preset, weather::Draws::default());
                found.map_or("none".to_owned(), |(_, w)| w.mat_words().1.to_owned())
            };
            let (good, bad) = (weather(Preset::Good), weather(Preset::Bad));
            let weathers = if good == bad { good } else { format!("good {good} · bad {bad}") };
            let ghost =
                if ghosts::ghost_files(&records, row.number).is_empty() { "none yet" } else { "kept from your drives" };
            vec![("Weather", weathers), ("Ghost", ghost.to_owned())]
        };
        let env = stage_browser::Env {
            maps: &self.catalog.maps,
            favourites: &self.favourite_maps,
            recent: &self.recent_maps,
            previews: &mut self.previews,
            facts: &facts,
        };
        match self.stage_browser.ui(ui, env) {
            Some(Action::Choose(index)) => {
                match self.stage_browser.target {
                    Target::Drive | Target::Circuit => self.choose_stage(index),
                    Target::Championship => self.championship.pick_stage(self.catalog.maps[index].number),
                }
                self.stage_browser.open = false;
            }
            Some(Action::Favourite(number)) => match self.favourite_maps.iter().position(|&n| n == number) {
                Some(at) => drop(self.favourite_maps.remove(at)),
                None => self.favourite_maps.push(number),
            },
            Some(Action::Back) => self.stage_browser.open = false,
            None => {}
        }
    }

    /// The Ghost row: the best one and the saved ones of the map.
    fn ghost_row(&mut self, rows: &mut Rows) {
        let map = self.catalog.maps[self.map].number;
        let records = self.records();
        let map_season = self.catalog.maps[self.map].season(&self.season).into_owned();
        // A saved ghost with the weather its words bring on this map's season (`Season::ghost_weather`).
        let ghost_label = |g: GhostChoice| match g {
            GhostChoice::Off => "Off".to_owned(),
            GhostChoice::Best => "Best (kept per stage)".to_owned(),
            GhostChoice::File(f) => {
                let weather = ghosts::ghost_words(&records, &f).and_then(|w| map_season.ghost_weather(map, &w));
                let name = f.trim_end_matches(".ghc");
                weather.map_or_else(|| name.to_owned(), |w| format!("{name} — {}", weather_label(&w)))
            }
        };
        let tip = "Best: race the fastest finished drive on this stage, replaced when you beat it";
        let choices: Vec<GhostChoice> = [GhostChoice::Off, GhostChoice::Best]
            .into_iter()
            .chain(ghosts::ghost_files(&self.records(), map).into_iter().map(GhostChoice::File))
            .collect();
        labelled(rows, "Ghost", tip, |ui| {
            combo(ui, "ghost", DROPDOWN, &mut self.ghost, &choices, ghost_label).on_hover_text(tip);
        });
    }
}

/// The Gearbox row's choices.
const GEARBOXES: [CarBox; 3] = [
    SEQUENTIAL,
    CarBox { engagement: Engagement::Synchro, sequential: false },
    CarBox { engagement: Engagement::Dog, sequential: false },
];

impl Choice for DamageLevel {
    const ALL: &[Self] = &DamageLevel::ALL;
    fn name(self) -> &'static str {
        DamageLevel::name(self)
    }
    fn label(self) -> &'static str {
        match self {
            DamageLevel::Off => "Off",
            DamageLevel::Safe => "Safe",
            DamageLevel::Reduced => "Reduced",
            DamageLevel::Realistic => "Realistic",
        }
    }
}

impl Choice for Skill {
    const ALL: &[Self] = &Skill::ALL;
    fn name(self) -> &'static str {
        Skill::name(self)
    }
    fn label(self) -> &'static str {
        Skill::label(self)
    }
}

fn gearbox_label(gearbox: CarBox) -> &'static str {
    match gearbox {
        CarBox { sequential: true, .. } => "Sequential",
        CarBox { engagement: Engagement::Synchro, .. } => "H-pattern, synchro",
        CarBox { engagement: Engagement::Dog, .. } => "H-pattern, dog",
    }
}

/// The Tyre row's choices: the game's tyre, or one of 0 through 6.
const TYRES: [TyreChoice; 8] = [
    TyreChoice::Auto,
    TyreChoice::Fixed(0),
    TyreChoice::Fixed(1),
    TyreChoice::Fixed(2),
    TyreChoice::Fixed(3),
    TyreChoice::Fixed(4),
    TyreChoice::Fixed(5),
    TyreChoice::Fixed(6),
];

/// A race weather in a few words: the stage letter, the surface wetness and the tyre wear.
fn weather_label(weather: &Weather) -> String {
    let (wear, wetness) = weather.mat_words();
    format!("{}, {wetness}, {wear} wear", weather.stage())
}

impl Form {
    /// The Time of day box: the moments, then the bake's sun.
    fn time_of_day_box(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ComboBox::from_id_salt("time of day")
            .width(DROPDOWN)
            .selected_text(self.start_time.label())
            .show_ui(ui, |ui| {
                for start in StartTime::all() {
                    ui.selectable_value(&mut self.start_time, start, start.label());
                }
            })
            .response
    }
}

/// Our own dropdown words in sentence case: "new" as "New".
fn sentence(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}
