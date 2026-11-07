//! The Championship tab's entry (the presets, Continue first when there is a saved championship) and the builder
//! (its settings beside the live summary and Start).

use egui::{Color32, DragValue, RichText, Sense, Slider, Stroke, Ui};
use rbr_formats::saves::is_file_name_part;
use rbr_season::config::{Damage, Daylight, Length, Preset, Skill, StagePool, Surface, Surfaces, Weather};
use rbr_season::team::team_offers;

use super::common::{card, card_title, CARD};
use super::view::*;
use super::*;
use crate::browser::{DIM, GREY};
use crate::pages::{combo, fields, section, start_button, ACCENT, WARN};

impl ChampionshipPage {
    /// The presets, with the profile and Continue first when it has one.
    pub(super) fn entry_ui(&mut self, ui: &mut Ui, env: Env) {
        ui.label(RichText::new("Championship").size(24.0).strong());
        ui.colored_label(
            GREY,
            "Build a rally or a season on any installed stage, against a field in your car's class.",
        );
        ui.add_space(8.0);
        fields(ui, "championship-profile", |ui| {
            ui.label("Profile").on_hover_text("The championship is saved under this name, one per profile");
            ui.add_enabled(!self.driving, egui::TextEdit::singleline(env.profile).desired_width(160.0));
            env.profile.retain(|c| is_file_name_part(c.encode_utf8(&mut [0; 4])));
            ui.end_row();
        });
        let saved = self.saved_event(env.here, env.profile);
        if let Some(saved) = &saved {
            ui.add_space(8.0);
            card(ui, |ui| {
                card_title(ui, "Your championship");
                match saved {
                    Ok(line) => {
                        ui.label(RichText::new(line).size(16.0));
                        ui.add_space(8.0);
                        if ui.horizontal(|ui| ui.add(start_button("Continue"))).inner.clicked() {
                            self.continue_event(env.here, env.profile, env.renamed);
                        }
                    }
                    Err(e) => _ = ui.colored_label(WARN, format!("The saved championship does not read: {e}")),
                }
            });
        }
        if !self.status.is_empty() {
            ui.colored_label(WARN, &self.status);
        }
        section(ui, if saved.is_some() { "Or start a new one" } else { "Start a championship" });
        for row in Preset::ALL.chunks(2) {
            ui.columns(2, |cols| {
                for (ui, &preset) in cols.iter_mut().zip(row) {
                    let (title, text, detail) = preset_text(preset);
                    let clicked = button_card(ui, title, |ui| {
                        ui.label(RichText::new(title).size(18.0).strong());
                        ui.add_space(2.0);
                        ui.colored_label(GREY, text);
                        ui.add_space(4.0);
                        ui.colored_label(DIM, detail);
                    });
                    if clicked {
                        self.open_builder(preset, env.cars, env.default_car, env.seed);
                    }
                }
            });
            ui.add_space(8.0);
        }
    }

    /// The builder: the settings beside the summary and Start.
    pub(super) fn builder_ui(&mut self, ui: &mut Ui, env: Env) -> Option<Request> {
        let mut request = None;
        let preset = self.builder.map_or(Preset::QuickRally, |b| b.preset);
        if ui.link("‹ Championship").clicked() {
            self.view = View::Entry;
            return None;
        }
        ui.label(RichText::new(preset_text(preset).0).size(24.0).strong());
        ui.colored_label(GREY, preset_text(preset).1);
        ui.add_space(8.0);
        let car = self.car.unwrap_or(env.default_car);
        let stages = self.pool.as_mut().and_then(|p| p.stages().map(<[_]>::to_vec));
        let progress = self.pool.as_ref().and_then(Pool::progress);
        let saved = self.saved_event(env.here, env.profile).is_some();
        let mut edited = self.config.clone()?;
        let config = &mut edited;
        let classes = classes(env.cars);
        let (driving, status) = (self.driving, self.status.clone());
        let mut start = false;
        let teams = self.ai.as_ref().map_or_else(|_| Vec::new(), |(_, roster)| team_offers(roster));
        let (team, mut picked_team) = (config.team as usize, config.team as usize);
        let ai_error = self.ai.as_ref().err().cloned();
        let mut left_out = std::mem::take(&mut self.left_out);
        let planned = (!self.plan.rallies.is_empty()).then(|| self.plan.rallies.clone());
        let plan_ready = self.plan.ready() || !self.plan.own();
        ui.columns(2, |cols| {
            fields(&mut cols[0], "championship-builder", |ui| {
                ui.label("Car");
                if (env.car_field)(ui, car) {
                    request = Some(Request::BrowseCar);
                }
                ui.end_row();
                ui.label("Class").on_hover_text("Only cars of this class fill the field");
                let label = |c: String| if c.is_empty() { "No class".to_owned() } else { c };
                combo(ui, "championship-class", 200.0, &mut config.class, &classes, label);
                ui.end_row();
                ui.label("Field cars").on_hover_text("The models the AI crews drive; team-mates share one");
                models_menu(ui, env.cars, &config.class, &mut left_out);
                ui.end_row();
                ui.label("Length");
                length_row(ui, &mut config.length);
                ui.end_row();
                ui.label("Stages from");
                combo(
                    ui,
                    "championship-pool",
                    200.0,
                    &mut config.pool,
                    &[StagePool::All, StagePool::Retail, StagePool::Community],
                    |p| match p {
                        StagePool::All => "Every installed stage",
                        StagePool::Retail => "The game's stages",
                        StagePool::Community => "Community stages (RSF, BTB)",
                    },
                );
                ui.end_row();
                ui.label("Surface");
                let surfaces = [Surface::Gravel, Surface::Tarmac, Surface::Snow].map(Surfaces::Only);
                combo(
                    ui,
                    "championship-surface",
                    200.0,
                    &mut config.surfaces,
                    &[surfaces[0], surfaces[1], surfaces[2], Surfaces::Mixed],
                    |s| match s {
                        Surfaces::Only(Surface::Gravel) => "Gravel",
                        Surfaces::Only(Surface::Tarmac) => "Tarmac",
                        Surfaces::Only(Surface::Snow) => "Snow",
                        Surfaces::Mixed => "Mixed, by rally",
                    },
                );
                ui.end_row();
                ui.label("Time of day");
                combo(
                    ui,
                    "championship-daylight",
                    200.0,
                    &mut config.daylight,
                    &[Daylight::Day, Daylight::Realistic, Daylight::Night],
                    |d| match d {
                        Daylight::Day => "Day",
                        Daylight::Realistic => "Day, the first day ends at dusk",
                        Daylight::Night => "Night",
                    },
                );
                ui.end_row();
                ui.label("Weather");
                combo(
                    ui,
                    "championship-weather",
                    200.0,
                    &mut config.weather,
                    &[Weather::Dry, Weather::Changeable, Weather::Wet],
                    |w| match w {
                        Weather::Dry => "Dry",
                        Weather::Changeable => "Changeable",
                        Weather::Wet => "Wet",
                    },
                )
                .on_hover_text(
                    "Changeable: each leg's road follows the stage's own weathers, so the tyre choice matters",
                );
                ui.end_row();
                ui.label("Damage");
                combo(
                    ui,
                    "championship-damage",
                    200.0,
                    &mut config.damage,
                    &[Damage::Off, Damage::Reduced, Damage::Realistic],
                    |d| match d {
                        Damage::Off => "Off",
                        Damage::Reduced => "Reduced",
                        Damage::Realistic => "Realistic",
                    },
                );
                ui.end_row();
                ui.label("Skill level");
                ui.horizontal(|ui| {
                    combo(
                        ui,
                        "championship-skill",
                        160.0,
                        &mut config.skill,
                        &[Skill::Rookie, Skill::Professional, Skill::Champion],
                        skill_word,
                    );
                    ui.colored_label(GREY, format!("help when stuck: {}", assistance_note(config.skill)));
                });
                ui.end_row();
                ui.label("Rules");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut config.restart, "Super Rally").on_hover_text(
                        "Retire and you restart the next leg (rallies of several legs); each stage not completed counts as its \
                         fastest time plus 10:00",
                    );
                    ui.checkbox(&mut config.power_stage, "Power stage")
                        .on_hover_text("The rally's last stage pays its five fastest 5-4-3-2-1 bonus points");
                });
                ui.end_row();
                if !teams.is_empty() {
                    ui.label("Team").on_hover_text("The service crew that repairs your car");
                    let (mut team, offers) = (team, &teams);
                    ui.horizontal(|ui| {
                        let name = |i: usize| {
                            offers.iter().find(|t| t.index as usize == i).map_or(String::new(), |t| t.name.clone())
                        };
                        let indexes: Vec<usize> = offers.iter().map(|t| t.index as usize).collect();
                        combo(ui, "championship-team", 200.0, &mut team, &indexes, name);
                        if let Some(offer) = offers.iter().find(|t| t.index as usize == team) {
                            ui.colored_label(GREY, &offer.crew);
                        }
                    });
                    picked_team = team;
                    ui.end_row();
                }
                ui.label("Difficulty");
                ui.horizontal(|ui| {
                    ui.add(Slider::new(&mut config.difficulty, 0.0..=1.0).show_value(false));
                    ui.label(RichText::new(difficulty_word(config.difficulty)).strong());
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut config.adaptive, "Adaptive: the field follows your results")
                    .on_hover_text("The difficulty moves with your stage results so the fight stays close");
                ui.end_row();
                ui.label("Field");
                ui.add(Slider::new(&mut config.field_size, 1..=30).suffix(" drivers"));
                ui.end_row();
                ui.label("Seed").on_hover_text("The same seed and stages build the same championship");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut config.seed));
                    if ui.button("New seed").clicked() {
                        config.seed = config.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    }
                });
                ui.end_row();
            });
            let ui = &mut cols[1];
            card(ui, |ui| {
                card_title(ui, "Summary");
                let line = match &planned {
                    Some(rallies) => plan_summary(config, rallies),
                    None => summary(config, stages.as_deref()),
                };
                ui.label(RichText::new(line).size(16.0).color(Color32::WHITE));
                ui.add_space(4.0);
                let all = class_cars(env.cars, &config.class);
                let field = all.iter().filter(|k| !left_out.contains(k)).count().max(1).min(all.len());
                ui.colored_label(
                    GREY,
                    match field {
                        0 | 1 => format!(
                            "The field drives your car: no other {} car is installed.",
                            label_class(&config.class)
                        ),
                        n => format!("{n} {} cars fill the field.", label_class(&config.class)),
                    },
                );
                let fit = stages.as_deref().map(|s| eligible(config, s).len());
                match (progress, fit) {
                    (Some((done, total)), _) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.colored_label(GREY, format!("Reading the stages' lengths… {done} of {total}"));
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    }
                    (None, Some(0)) => {
                        _ = ui.colored_label(WARN, "No installed stage fits: choose other stages or another surface.")
                    }
                    (None, Some(n)) => _ = ui.colored_label(GREY, format!("{n} installed stages fit.")),
                    (None, None) => {}
                }
                ui.add_space(10.0);
                if !plan_ready {
                    ui.colored_label(WARN, "Each rally's legs need a stage.");
                }
                let ready = fit.is_some_and(|n| n > 0)
                    && plan_ready
                    && is_file_name_part(env.profile)
                    && !driving
                    && ai_error.is_none();
                let go = ui.horizontal(|ui| ui.add_enabled(ready, start_button("Start"))).inner;
                if saved {
                    ui.colored_label(GREY, format!("Replaces {}'s saved championship.", env.profile));
                }
                start = go.clicked();
                if let Some(e) = &ai_error {
                    ui.colored_label(WARN, format!("The AI tables do not read: {e}"));
                }
                if !status.is_empty() {
                    ui.colored_label(WARN, &status);
                }
            });
        });
        edited.team = picked_team as u8;
        self.left_out = left_out;
        if let Some(stages) = &stages {
            self.plan.follow(&edited, stages);
            ui.add_space(12.0);
            section(ui, "Itinerary");
            if let Some(slot) = super::plan::plan_ui(ui, &mut self.plan, &edited, stages) {
                self.picking = Some(slot);
                request = Some(Request::BrowseStage);
            }
        }
        self.config = Some(edited);
        if let Some(stages) = stages.filter(|_| start) {
            self.start_event(env.here, env.profile, env.cars, &stages, env.archive);
        }
        request
    }
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
        Preset::Random => {
            ("Random", "An event drawn from a random seed: its length, stages and weather.", "a surprise")
        }
    }
}

/// A card that is one button: its frame lit on hover and focus. Whether it was clicked.
fn button_card(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) -> bool {
    let inner = egui::Frame::new().fill(CARD).corner_radius(6.0).inner_margin(12.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(92.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), add);
    });
    let response = ui.interact(inner.response.rect, ui.scope_id().with(id), Sense::click());
    if response.hovered() || response.has_focus() {
        let stroke = Stroke::new(1.5, ACCENT);
        ui.painter().rect_stroke(response.rect, 6.0, stroke, egui::StrokeKind::Inside);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
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
