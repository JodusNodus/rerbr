//! The Championship page's setup screen: the presets on the left and the builder on the right, a saved championship
//! as a Continue bar on top, the live summary and Start in the pinned footer.

use egui::{Align, Color32, DragValue, Layout, Margin, RichText, Sense, Slider, Stroke, Ui, Vec2};
use rbr_formats::saves::is_file_name_part;
use rbr_season::config::{Damage, Daylight, Length, Preset, Skill, Surface, Surfaces, Weather};
use rbr_season::team::team_offers;

use super::common::{panel_frame, row, BAD};
use super::view::*;
use super::*;
use crate::kit;
use crate::pages::combo;
use crate::theme::*;

/// The preset the setup opens on.
const FIRST_PRESET: Preset = Preset::Weekend;
/// The presets' column.
const PRESETS_WIDTH: f32 = 250.0;

impl ChampionshipPage {
    /// The setup screen: the presets, the builder, Continue for a saved championship, Start.
    pub(super) fn entry_ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
        let mut request = None;
        if self.config.is_none() {
            self.open_builder(FIRST_PRESET, env.cars, env.default_car, env.seed);
        } else if self.config == self.applied && self.car != Some(env.default_car) && env.default_car < env.cars.len() {
            // Untouched since its preset was applied: the builder keeps following the Drive page's car.
            self.car = None;
            let preset = self.builder.map_or(FIRST_PRESET, |b| b.preset);
            self.open_builder(preset, env.cars, env.default_car, env.seed);
        }
        let preset = self.builder.map_or(FIRST_PRESET, |b| b.preset);
        let car = self.car.unwrap_or(env.default_car);
        let stages = self.pool.as_mut().and_then(|p| p.stages().map(<[_]>::to_vec));
        let progress = self.pool.as_ref().and_then(Pool::progress);
        let saved = self.saved_event(env.here, env.profile);
        let mut edited = self.config.clone()?;
        let classes = classes(env.cars);
        let (driving, status) = (self.driving, self.status.clone());
        let teams = self.ai.as_ref().map_or_else(|_| Vec::new(), |(_, roster)| team_offers(roster));
        let ai_error = self.ai.as_ref().err().cloned();
        let mut left_out = std::mem::take(&mut self.left_out);
        let planned = (!self.plan.rallies.is_empty()).then(|| self.plan.rallies.clone());
        let plan_ready = self.plan.ready() || !self.plan.own();
        let mut start = false;

        let subtitle = "Build a rally or a season on any installed stage, against a field in your car's class.";
        kit::page_header(ui, "Championship", Some(subtitle), |ui| {
            let tip = "Your name for the saves: one championship is kept per profile, and your best laps on the \
                       Circuit page. Another name starts a separate set.";
            kit::hint(ui, "saves and best laps are kept under this name");
            let edit = egui::TextEdit::singleline(env.profile).desired_width(130.0).hint_text("Your name");
            ui.add_enabled(!driving, edit).on_hover_text(tip);
            env.profile.retain(|c| is_file_name_part(c.encode_utf8(&mut [0; 4])));
            ui.label(RichText::new("Profile").color(TEXT_2)).on_hover_text(tip);
        });

        // What blocks Start, first reason first.
        let fit = stages.as_deref().map(|s| eligible(&edited, s).len());
        let blocked = if let Some(e) = &ai_error {
            Some(format!("The AI tables do not read: {e}"))
        } else if let Some((done, total)) = progress {
            Some(format!("Reading the stages' lengths… {done} of {total}"))
        } else if fit == Some(0) {
            Some("No installed stage fits: choose other stages or another surface.".to_owned())
        } else if !plan_ready {
            Some("Each rally's legs need a stage.".to_owned())
        } else if !is_file_name_part(env.profile) {
            Some("Give the profile a name.".to_owned())
        } else if driving {
            Some("A stage is being driven.".to_owned())
        } else if fit.is_none() {
            Some("Reading the installed stages…".to_owned())
        } else {
            None
        };
        let all = class_cars(env.cars, &edited.class);
        let field = all.iter().filter(|k| !left_out.contains(k)).count().max(1).min(all.len());
        kit::footer(ui, "championship-setup-footer", |ui| {
            let width = ui.available_width();
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(width * 0.55, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new(preset_text(preset).0).family(semibold()).color(TEXT));
                    let line = match &planned {
                        Some(rallies) => plan_summary(&edited, rallies),
                        None => summary(&edited, stages.as_deref()),
                    };
                    ui.label(RichText::new(line).size(12.0).color(TEXT_2));
                    let class = label_class(&edited.class);
                    let drivers = match field {
                        0 | 1 => format!("The field drives your car: no other {class} car is installed."),
                        n => format!("{n} {class} cars fill the field."),
                    };
                    let fits = fit.filter(|&n| n > 0).map_or(String::new(), |n| format!(" {n} installed stages fit."));
                    ui.label(RichText::new(format!("{drivers}{fits}")).size(12.0).color(TEXT_3));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    start = ui.add_enabled(blocked.is_none(), kit::primary_button("Start", None)).clicked();
                    ui.vertical(|ui| {
                        if let Some(reason) = &blocked {
                            if progress.is_some() {
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                            }
                            kit::status_dot(ui, WARN, reason);
                        }
                        if saved.is_some() {
                            kit::status_dot(ui, WARN, &format!("Replaces {}'s saved championship", env.profile));
                        }
                        if !status.is_empty() {
                            kit::status_dot(ui, WARN, &status);
                        }
                    });
                });
            });
        });

        let mut abandon = false;
        let mut picked = None;
        kit::body(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("championship").auto_shrink([false, false]).show(ui, |ui| {
                if let Some(saved) = &saved {
                    self.continue_bar(ui, saved, &env, &mut abandon);
                    ui.add_space(18.0);
                }
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(PRESETS_WIDTH, 0.0), Layout::top_down(Align::Min), |ui| {
                        kit::group_label(
                            ui,
                            if saved.is_some() { "Or start a new one" } else { "New championship" },
                            None,
                        );
                        picked = preset_list(ui, preset);
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 4.0);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        ui.label(RichText::new(preset_text(preset).0).size(15.0).family(semibold()).color(TEXT));
                        ui.label(RichText::new(preset_text(preset).1).color(TEXT_2));
                        ui.add_space(8.0);
                        if self.settings_ui(ui, &env, car, &classes, &teams, &mut edited, &mut left_out) {
                            request = Some(Request::BrowseCar);
                        }
                        ui.add_space(8.0);
                        if let Some(stages) = &stages {
                            self.plan.follow(&edited, stages);
                            let slot = itinerary_editor(ui, &mut self.plan, &edited, stages, plan_ready);
                            if let Some(slot) = slot {
                                self.picking = Some(slot);
                                request = Some(Request::BrowseStage);
                            }
                        }
                    });
                });
            });
        });
        // The footer was drawn before the body changed the settings: draw once more for it.
        if self.config.as_ref() != Some(&edited) || self.left_out != left_out {
            ui.ctx().request_repaint();
        }
        if self.abandoning {
            match abandon_modal(ui.ctx(), env.profile) {
                Some(true) => {
                    self.abandoning = false;
                    self.leave_event(env.here, env.profile);
                    return None;
                }
                Some(false) => self.abandoning = false,
                None => {}
            }
        }
        self.config = Some(edited);
        self.left_out = left_out;
        if abandon {
            self.abandoning = true;
        }
        if let Some(preset) = picked {
            self.open_builder(preset, env.cars, env.default_car, env.seed);
        }
        if let Some(stages) = stages.filter(|_| start) {
            self.start_event(env.here, env.profile, env.cars, &stages, env.archive);
        }
        request
    }
}

impl ChampionshipPage {
    /// A saved championship: where it stands, Abandon… and Continue.
    fn continue_bar(&mut self, ui: &mut Ui, saved: &Result<String, String>, env: &Env, abandon: &mut bool) {
        kit::group_label(ui, "Your championship", None);
        panel_frame().inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    match saved {
                        Ok(line) => {
                            ui.label(RichText::new(line).family(medium()).color(TEXT));
                            ui.label(RichText::new(format!("Saved under {}", env.profile)).size(12.0).color(TEXT_2));
                        }
                        Err(e) => _ = ui.colored_label(WARN, format!("The saved championship does not read: {e}")),
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if saved.is_ok() && ui.add(kit::primary_button("Continue", None)).clicked() {
                        self.continue_event(env.here, env.profile, env.renamed);
                    }
                    let go = ui.add_enabled(!self.driving, egui::Button::new("Abandon…"));
                    *abandon |= go.on_hover_text("Delete this championship's save").clicked();
                });
            });
        });
    }

    /// The setting panels: the event and the rules side by side, then the length and the seed. Whether Browse was
    /// clicked on the car.
    #[allow(clippy::too_many_arguments)]
    fn settings_ui(
        &mut self,
        ui: &mut Ui,
        env: &Env,
        car: usize,
        classes: &[String],
        teams: &[rbr_season::team::TeamOffer],
        config: &mut EventConfig,
        left_out: &mut Vec<String>,
    ) -> bool {
        let mut browse = false;
        let width = 150.0;
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            kit::group_label(ui, "Event", None);
            kit::panel(ui, |rows| {
                let info = (env.car_info)(car);
                row(rows, "Car", "", |ui| {
                    browse = kit::link(ui, "Change").clicked();
                    let (name, _, favourite) =
                        info.clone().unwrap_or_else(|| ("no car".to_owned(), String::new(), false));
                    let mut job = egui::text::LayoutJob::default();
                    let font = egui::FontSelection::Default;
                    if favourite {
                        RichText::new("★ ").color(crate::browser::STAR).append_to(
                            &mut job,
                            ui.style(),
                            font.clone(),
                            Align::Min,
                        );
                    }
                    RichText::new(name).color(TEXT).append_to(&mut job, ui.style(), font, Align::Min);
                    ui.add(egui::Label::new(job).truncate());
                });
                if let Some((_, detail, _)) = &info {
                    rows.hint(detail);
                }
                row(rows, "Class", "Only cars of this class fill the field", |ui| {
                    let label = |c: String| if c.is_empty() { "No class".to_owned() } else { c };
                    combo(ui, "championship-class", width, &mut config.class, classes, label);
                });
                row(rows, "Field cars", "The models the AI crews drive; team-mates share one", |ui| {
                    models_menu(ui, env.cars, &config.class, left_out)
                });
                row(rows, "Surface", "", |ui| {
                    let surfaces = [Surface::Gravel, Surface::Tarmac, Surface::Snow].map(Surfaces::Only);
                    let all = [surfaces[0], surfaces[1], surfaces[2], Surfaces::Mixed];
                    combo(ui, "championship-surface", width, &mut config.surfaces, &all, |s| match s {
                        Surfaces::Only(Surface::Gravel) => "Gravel",
                        Surfaces::Only(Surface::Tarmac) => "Tarmac",
                        Surfaces::Only(Surface::Snow) => "Snow",
                        Surfaces::Mixed => "Mixed, by rally",
                    });
                });
                row(rows, "Time of day", "", |ui| {
                    let all = [Daylight::Day, Daylight::Realistic, Daylight::Night];
                    combo(ui, "championship-daylight", width, &mut config.daylight, &all, |d| match d {
                        Daylight::Day => "Day",
                        Daylight::Realistic => "Day, the first day ends at dusk",
                        Daylight::Night => "Night",
                    });
                });
                row(
                    rows,
                    "Weather",
                    "Changeable: each leg's road follows the stage's own weathers, so the tyre choice matters",
                    |ui| {
                        let all = [Weather::Dry, Weather::Changeable, Weather::Wet];
                        combo(ui, "championship-weather", width, &mut config.weather, &all, |w| match w {
                            Weather::Dry => "Dry",
                            Weather::Changeable => "Changeable",
                            Weather::Wet => "Wet",
                        });
                    },
                );
            });

            let ui = &mut cols[1];
            kit::group_label(ui, "Rules", None);
            kit::panel(ui, |rows| {
                row(rows, "Difficulty", "", |ui| {
                    ui.label(RichText::new(difficulty_word(config.difficulty)).family(medium()).color(TEXT));
                    ui.spacing_mut().slider_width = 110.0;
                    ui.add(Slider::new(&mut config.difficulty, 0.0..=1.0).show_value(false));
                });
                row(rows, "Field", "", |ui| {
                    ui.spacing_mut().slider_width = 110.0;
                    ui.add(Slider::new(&mut config.field_size, 1..=30).suffix(" drivers"));
                });
                row(rows, "Damage", "", |ui| {
                    let all = [Damage::Off, Damage::Reduced, Damage::Realistic];
                    combo(ui, "championship-damage", width, &mut config.damage, &all, |d| match d {
                        Damage::Off => "Off",
                        Damage::Reduced => "Reduced",
                        Damage::Realistic => "Realistic",
                    });
                });
                row(rows, "Skill level", "How hard the field drives and how much help the car gets when stuck", |ui| {
                    let all = [Skill::Rookie, Skill::Professional, Skill::Champion];
                    combo(ui, "championship-skill", width, &mut config.skill, &all, skill_word);
                });
                rows.hint(&format!("Help when stuck: {}", assistance_note(config.skill)));
                if !teams.is_empty() {
                    let mut team = config.team as usize;
                    row(rows, "Team", "The service crew that repairs your car", |ui| {
                        let name = |i: usize| {
                            teams.iter().find(|t| t.index as usize == i).map_or(String::new(), |t| t.name.clone())
                        };
                        let indexes: Vec<usize> = teams.iter().map(|t| t.index as usize).collect();
                        combo(ui, "championship-team", width, &mut team, &indexes, name);
                    });
                    config.team = team as u8;
                    if let Some(offer) = teams.iter().find(|t| t.index as usize == team) {
                        rows.hint(&offer.crew);
                    }
                }
                let restart =
                    "Retire and you restart the next leg (rallies of several legs); each stage not completed \
                               counts as its fastest time plus 10:00";
                row(rows, "Super Rally", restart, |ui| _ = kit::switch(ui, &mut config.restart));
                let power = "The rally's last stage pays its five fastest 5-4-3-2-1 bonus points";
                row(rows, "Power stage", power, |ui| _ = kit::switch(ui, &mut config.power_stage));
                let adaptive = "The difficulty moves with your stage results so the fight stays close";
                row(rows, "Adaptive", adaptive, |ui| _ = kit::switch(ui, &mut config.adaptive));
                rows.hint("Adaptive: the field follows your results.");
            });
        });
        ui.add_space(14.0);
        kit::group_label(ui, "Length", None);
        kit::panel(ui, |rows| {
            row(rows, "Length", "", |ui| length_row(ui, &mut config.length));
            row(rows, "Seed", "The same seed and stages build the same championship", |ui| {
                if kit::link(ui, "New seed").clicked() {
                    config.seed = config.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                }
                ui.add(DragValue::new(&mut config.seed));
            });
        });
        browse
    }
}

/// The presets as a list of radio rows; the one clicked.
fn preset_list(ui: &mut Ui, selected: Preset) -> Option<Preset> {
    let mut clicked = None;
    panel_frame().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.set_width(ui.available_width());
        let last = Preset::ALL.len() - 1;
        for (k, &preset) in Preset::ALL.iter().enumerate() {
            let (title, _, detail) = preset_text(preset);
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());
            let radius = |on: bool| if on { 9 } else { 0 };
            let corners = egui::CornerRadius {
                nw: radius(k == 0),
                ne: radius(k == 0),
                sw: radius(k == last),
                se: radius(k == last),
            };
            let on = preset == selected;
            let fill = match (on, response.hovered()) {
                (true, _) => white(15),
                (false, true) => white(8),
                _ => Color32::TRANSPARENT,
            };
            let painter = ui.painter();
            painter.rect_filled(rect.shrink(1.0), corners, fill);
            if k > 0 {
                painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, LINE));
            }
            let centre = egui::pos2(rect.left() + 21.0, rect.center().y);
            painter.circle_stroke(centre, 7.0, Stroke::new(1.5, if on { ACCENT } else { CHECK_BORDER }));
            if on {
                painter.circle_filled(centre, 3.5, ACCENT);
            }
            let name = painter.layout_no_wrap(title.to_owned(), egui::FontId::new(13.0, medium()), TEXT);
            let sub = painter.layout_no_wrap(
                detail.to_owned(),
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
                TEXT_2,
            );
            let top = rect.center().y - (name.size().y + sub.size().y + 3.0) / 2.0;
            let x = rect.left() + 38.0;
            let below = top + name.size().y + 3.0;
            painter.galley(egui::pos2(x, top), name, TEXT);
            painter.galley(egui::pos2(x, below), sub, TEXT_2);
            if response.has_focus() {
                painter.rect_stroke(rect.shrink(1.0), corners, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
            }
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                clicked = Some(preset);
            }
        }
    });
    clicked
}

/// The itinerary editor in a collapsible section; the slot to pick a stage for, if asked.
fn itinerary_editor(
    ui: &mut Ui,
    plan: &mut super::plan::Plan,
    config: &EventConfig,
    stages: &[StageRef],
    ready: bool,
) -> Option<super::plan::Slot> {
    let rallies = plan.rallies.len();
    let count: usize = plan.rallies.iter().flat_map(|r| r.legs.iter()).map(Vec::len).sum();
    let title = RichText::new(format!(
        "Itinerary · {rallies} rall{} · {count} stage{}{}",
        if rallies == 1 { "y" } else { "ies" },
        if count == 1 { "" } else { "s" },
        if ready { "" } else { " · needs a stage" }
    ))
    .size(12.0)
    .family(medium())
    .color(if ready { TEXT_2 } else { WARN });
    let mut slot = None;
    egui::CollapsingHeader::new(title).id_salt("championship-itinerary-editor").default_open(false).show(ui, |ui| {
        ui.add_space(4.0);
        slot = super::plan::plan_ui(ui, plan, config, stages);
    });
    slot
}

/// The confirmation to abandon the saved championship: `Some(true)` to abandon, `Some(false)` to keep it.
fn abandon_modal(ctx: &egui::Context, profile: &str) -> Option<bool> {
    let mut choice = None;
    let closed = kit::modal(
        ctx,
        "championship-abandon",
        "Abandon this championship?",
        Some("Its stage results, standings and carried damage are deleted."),
        |ui| {
            ui.label(
                RichText::new(format!("{profile}'s saved championship is removed. This cannot be undone."))
                    .color(TEXT_LABEL),
            );
        },
        |ui| {
            if ui.add(egui::Button::new(RichText::new("Abandon").color(BAD))).clicked() {
                choice = Some(true);
            }
            if ui.add(egui::Button::new("Keep it")).clicked() {
                choice = Some(false);
            }
        },
    );
    choice.or(closed.closed().then_some(false))
}

/// A preset's card: its name, what it is and its shape.
fn preset_text(preset: Preset) -> (&'static str, &'static str, &'static str) {
    match preset {
        Preset::QuickRally => {
            ("Quick rally", "A one-day rally on gravel, as most national rallies run.", "6 stages · one leg")
        }
        Preset::Weekend => (
            "Rally weekend",
            "A WRC-style round: three days, an overnight service after each, the power stage last.",
            "12 stages · three legs",
        ),
        Preset::Championship => (
            "Short championship",
            "Four one-day rallies on mixed surfaces, points after each, as a national series.",
            "4 rallies · 24 stages",
        ),
        Preset::Season => (
            "Season",
            "Six two-day rallies on mixed surfaces with power stages, points after each.",
            "6 rallies · 36 stages",
        ),
    }
}

/// The rallies, each rally's legs (days) and its stages.
fn length_row(ui: &mut Ui, length: &mut Length) {
    let Length { rallies, legs, stages } = length;
    ui.horizontal(|ui| {
        let plural = |n: u8, one: &'static str, more: &'static str| if n == 1 { one } else { more };
        let suffix = plural(*rallies, " rally", " rallies");
        ui.add(DragValue::new(rallies).range(1..=12).suffix(suffix))
            .on_hover_text("One rally, or a championship with points after each");
        let suffix = plural(*legs, " leg", " legs");
        ui.add(DragValue::new(legs).range(1..=3).suffix(suffix))
            .on_hover_text("A leg is a day; an overnight service ends it");
        let suffix = if *rallies > 1 { " stages each" } else { " stages" };
        ui.add(DragValue::new(stages).range(2..=20).suffix(suffix));
    });
    *stages = (*stages).max(*legs);
}

/// The field's models: a menu of the class's, each to keep in or leave out; the last one stays.
fn models_menu(ui: &mut Ui, cars: &[crate::catalog::Car], class: &str, left_out: &mut Vec<String>) {
    let all = class_cars(cars, class);
    let kept = all.iter().filter(|k| !left_out.contains(k)).count();
    let label = match (kept, all.len()) {
        (_, 0) => "Your car".to_owned(),
        (k, n) if k == n || k == 0 => format!("All {n} model{}", if n == 1 { "" } else { "s" }),
        (k, n) => format!("{k} of {n} models"),
    };
    ui.add_enabled_ui(all.len() > 1, |ui| {
        ui.menu_button(label, |ui| {
            for key in &all {
                let mut keep = !left_out.contains(key);
                let last = keep && kept <= 1;
                let name = super::view::car_name(cars, key);
                if ui.add_enabled(!last, egui::Checkbox::new(&mut keep, name)).changed() {
                    match keep {
                        true => left_out.retain(|k| k != key),
                        false => left_out.push(key.clone()),
                    }
                }
            }
        });
    });
}
