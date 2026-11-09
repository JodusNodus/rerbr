//! The Car setup page's editor of one area: the area's diagram, then each value as a track over its range with the
//! comparison's tick and the stretch between them, its − and + steps (the game's), the number, and the difference.
//! The per-wheel areas edit an axle's two wheels together until they are set apart.

use egui::{vec2, Align2, Color32, CornerRadius, DragValue, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use rbr_gameplay::setup::{Area, Range, Setup, Spec, MAP_POINTS};

use super::charts::{self, Damper, Spring, BASE, DIFF, FRONT, REAR};
use super::values::{values, with_values, AXLES, MAP_NAMES, WHEEL_AREAS};
use super::SetupPage;
use crate::pages::combo;

/// A value that is not an axle's.
const NEUTRAL: Color32 = Color32::from_rgb(150, 205, 170);
/// A track's width and height.
const TRACK: egui::Vec2 = vec2(230.0, 18.0);
/// The width of the rows' names (an axle, a wheel).
const NAME: f32 = 84.0;

/// What the area sets, under its title.
fn about(area: Area) -> &'static str {
    match area {
        Area::Tyres => "Tyre pressures, wheel by wheel.",
        Area::Springs => "The main spring and the helper spring at each corner.",
        Area::Dampers => "How hard each damper resists the wheel's movement, by its speed.",
        Area::Geometry => "Wheel alignment and ride height.",
        Area::RollBars => "The anti-roll bars and the steering lock.",
        Area::Brakes => "Brake pressures and the handbrake.",
        Area::Differentials => {
            "Each diff's locking torque, and the control unit's lock maps: the lock is the largest of the throttle, \
             brake and speed curves."
        }
        Area::Gears => "The drop gear between the gearbox and the diffs; the gearbox's own ratios are fixed.",
    }
}

/// A value's rule of thumb (general, not measured on this car), or what it does where the physics says so.
fn hint(label: &str) -> &'static str {
    match label {
        "Pressure" => "Lower: more grip on loose ground, a lazier response. Higher: a sharper response.",
        "Spring length" => "The spring's free length: longer lifts the corner.",
        "Spring rate" => "Stiffer: a quicker response and less roll. Softer: more grip over bumps.",
        "Helper spring length" | "Helper spring rate" => {
            "A soft spring in series with the main one, working while the wheel hangs low."
        }
        "Bump (slow)" => "Resists the wheel rising slowly (roll, pitch). Stiffer: the weight moves quicker.",
        "Rebound" => "Resists the wheel dropping. Too stiff and the wheel leaves the ground after a bump.",
        "Bump (fast)" => "Resists sharp hits and landings, past the speed below. Softer soaks them up.",
        "Fast bump from" => "The wheel speed the fast bump rate takes over from.",
        "Top mount" => "The strut top's mounting slot.",
        "Toe (steering rod)" => "The steering rod's length sets the wheel's toe.",
        "Ride height (platform)" => "The spring platform's height: the corner's ride height.",
        "Camber (axis inclination)" => "Negative leans the wheel's top inward: more grip in corners.",
        "Steering lock" => "How far the front wheels turn: more for hairpins, less for a calmer wheel.",
        "Front anti-roll bar" => "Stiffer: less roll, more understeer.",
        "Rear anti-roll bar" => "Stiffer: less roll, more oversteer.",
        "Front brake pressure" | "Rear brake pressure" => {
            "Their balance: more front is steadier, more rear turns the car under braking."
        }
        "Handbrake centre-diff release" => "The centre diff opens while the handbrake is pulled this far or more.",
        "Centre diff torque" | "Front diff torque" | "Rear diff torque" => {
            "The torque the diff locks with at full lock: more traction, less turn-in."
        }
        "Left-foot brake threshold" => {
            "With both pedals past this the centre diff takes the left-foot brake map; 0 never."
        }
        _ => "",
    }
}

/// The difference of `now` from `base` as shown, `None` when they are the same: with the unit's decimals, or more
/// when those round it to nothing.
fn difference(spec: &Spec, now: f32, base: f32) -> Option<String> {
    (now != base).then(|| {
        let unit = spec.unit;
        format!("{} {}", signed((now - base) * unit.scale, unit.decimals), unit.suffix).trim_end().to_owned()
    })
}

/// `value` with a sign and `decimals` decimals, or up to 3 more when those show 0.
fn signed(value: f32, decimals: usize) -> String {
    let places =
        (decimals..=decimals + 3).find(|&d| (value * 10f32.powi(d as i32)).abs() >= 0.5).unwrap_or(decimals + 3);
    format!("{value:+.places$}")
}

/// The track: the range, the value's fill and knob, the comparison's tick and the stretch between them. A drag or
/// click takes the steps toward the pointer. Whether the value changed.
fn track(ui: &mut Ui, value: &mut f32, base: f32, range: Range, color: Color32) -> bool {
    let fixed = range.max <= range.min || range.step <= 0.0;
    let sense = if fixed { Sense::hover() } else { Sense::click_and_drag() };
    let (rect, response) = ui.allocate_exact_size(TRACK, sense);
    let rail = rect.shrink2(vec2(7.0, 0.0));
    let span = range.max - range.min;
    let x_of =
        |v: f32| rail.left() + rail.width() * if span > 0.0 { ((v - range.min) / span).clamp(0.0, 1.0) } else { 0.5 };
    let mut changed = false;
    if let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
        let target = range.min + span * ((p.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
        let next = range.toward(*value, target);
        changed = next != *value;
        *value = next;
    }
    let painter = ui.painter_at(rect.expand(2.0));
    let visuals = ui.visuals();
    let y = rect.center().y;
    let bar = Rect::from_min_max(egui::pos2(rail.left(), y - 3.0), egui::pos2(rail.right(), y + 3.0));
    painter.rect_filled(bar, CornerRadius::same(3), visuals.extreme_bg_color);
    let (at, was) = (x_of(*value), x_of(base));
    let tone = if fixed { visuals.weak_text_color() } else { color };
    painter.rect_filled(
        Rect::from_min_max(bar.left_top(), egui::pos2(at, bar.bottom())),
        CornerRadius::same(3),
        tone.gamma_multiply(0.45),
    );
    if base != *value {
        let stretch = Rect::from_min_max(egui::pos2(at.min(was), y - 1.5), egui::pos2(at.max(was), y + 1.5));
        painter.rect_filled(stretch, CornerRadius::ZERO, DIFF);
        painter.line_segment([egui::pos2(was, y - 7.0), egui::pos2(was, y + 7.0)], Stroke::new(2.0, BASE));
    }
    let knob = if response.hovered() || response.dragged() { 7.5 } else { 6.5 };
    let ring = if base != *value { DIFF } else { visuals.widgets.inactive.fg_stroke.color };
    painter.circle(egui::pos2(at, y), knob, tone, Stroke::new(1.5, ring));
    changed
}

/// One value's controls: − track + number difference; a value the car's options fix is only shown. Whether it
/// changed.
fn controls(ui: &mut Ui, spec: &Spec, range: Range, value: &mut f32, base: f32, color: Color32) -> bool {
    let unit = spec.unit;
    if range.max <= range.min || range.step <= 0.0 {
        ui.label(RichText::new(format!("{}, fixed for this car", unit.show(*value))).weak());
        return false;
    }
    let before = *value;
    if ui.small_button("−").clicked() {
        *value = range.down(*value);
    }
    track(ui, value, base, range, color);
    if ui.small_button("+").clicked() {
        *value = range.up(*value);
    }
    let mut shown = *value * unit.scale;
    let field = DragValue::new(&mut shown)
        .range(range.min * unit.scale..=range.max * unit.scale)
        .speed(range.step * unit.scale)
        .fixed_decimals(unit.decimals)
        .suffix(format!(" {}", unit.suffix).trim_end().to_owned());
    if ui.add_sized([96.0, 20.0], field).changed() {
        *value = range.toward(*value, shown / unit.scale);
    }
    let text = difference(spec, *value, base).unwrap_or_default();
    let compared = format!("compared: {}", unit.show(base));
    ui.add_sized([86.0, 20.0], egui::Label::new(RichText::new(text).color(DIFF).size(12.0))).on_hover_text(compared);
    *value != before
}

/// A value's title line: its label, its rule of thumb, its range on hover.
fn title(ui: &mut Ui, spec: &Spec, range: &Range) {
    let unit = spec.unit;
    let wrap = if range.wrap { ", wraps" } else { "" };
    let span = format!("{} to {}, step {}{wrap}", unit.show(range.min), unit.show(range.max), unit.show(range.step));
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(spec.label).strong()).on_hover_text(span);
        let hint = hint(spec.label);
        if !hint.is_empty() {
            ui.label(RichText::new(hint).weak().size(12.0));
        }
    });
}

/// A balance between front and rear: the bar split at the front's share, the comparison's tick.
fn balance(ui: &mut Ui, what: &str, front: f32, base: f32) {
    let width = 360.0_f32.min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(3.0));
    let share = if front.is_finite() { front.clamp(0.0, 1.0) } else { 0.5 };
    let split = rect.left() + rect.width() * share;
    let radius = CornerRadius::same(4);
    painter.rect_filled(rect, radius, REAR.gamma_multiply(0.55));
    painter.rect_filled(
        Rect::from_min_max(rect.left_top(), egui::pos2(split, rect.bottom())),
        radius,
        FRONT.gamma_multiply(0.7),
    );
    painter.rect_stroke(rect, radius, ui.visuals().widgets.noninteractive.bg_stroke, StrokeKind::Inside);
    if base.is_finite() && base != share {
        let x = rect.left() + rect.width() * base.clamp(0.0, 1.0);
        painter.line_segment(
            [egui::pos2(x, rect.top() - 3.0), egui::pos2(x, rect.bottom() + 3.0)],
            Stroke::new(2.0, DIFF),
        );
    }
    let font = FontId::proportional(12.5);
    let text = Color32::from_gray(235);
    painter.text(
        rect.left_center() + vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        format!("{:.0} % front", 100.0 * share),
        font.clone(),
        text,
    );
    painter.text(
        rect.right_center() - vec2(8.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("{:.0} % rear", 100.0 * (1.0 - share)),
        font,
        text,
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new(what).weak().size(12.0));
        if base.is_finite() && base != share {
            let delta = format!("{} points front (compared: {:.0} %)", signed(100.0 * (share - base), 1), 100.0 * base);
            ui.label(RichText::new(delta).color(DIFF).size(12.0));
        }
    });
}

/// The front's share of `front + rear`.
fn share(front: f32, rear: f32) -> f32 {
    front / (front + rear)
}

impl SetupPage {
    /// The car's values with their ranges, in [`values`] order.
    pub(super) fn specs(&self) -> Vec<(Spec, Range)> {
        let ranges = self.car.as_ref().map_or(&[][..], |c| &c.ranges[..]);
        Setup::specs().into_iter().zip(ranges.iter().copied()).collect()
    }

    /// Whether each axle's left and right values of `area` are all the same.
    fn alike(setup: &Setup, specs: &[(Spec, Range)], area: Area) -> [bool; 2] {
        let list = values(setup);
        let mut out = [true; 2];
        for (i, (spec, _)) in specs.iter().enumerate().filter(|(_, (s, _))| s.area == area) {
            let Some(w) = spec.index.filter(|w| w % 2 == 0) else { continue };
            let pair =
                specs.iter().position(|(s, _)| s.label == spec.label && s.area == area && s.index == Some(w + 1));
            if pair.is_some_and(|j| list[j] != list[i]) {
                out[w / 2] = false;
            }
        }
        out
    }

    pub(super) fn area_ui(&mut self, ui: &mut Ui) {
        let area = self.area;
        ui.label(RichText::new(area.name()).heading().strong());
        ui.label(RichText::new(about(area)).weak());
        ui.add_space(6.0);
        let (draft, base) = (self.draft, self.base);
        let wide = ui.available_width() >= 760.0;
        match area {
            Area::Tyres => {}
            Area::Springs => {
                let spring = |s: &Setup, w: usize| {
                    let c = &s.corners[w];
                    Spring {
                        length: c.spring_length,
                        rate: c.spring_stiffness,
                        helper_length: c.helper_spring_length,
                        helper_rate: c.helper_spring_stiffness,
                    }
                };
                ui.horizontal_top(|ui| {
                    charts::springs(
                        ui,
                        [spring(&draft, 0), spring(&draft, 2)],
                        [spring(&base, 0), spring(&base, 2)],
                        vec2(260.0, 200.0),
                    );
                    ui.vertical(|ui| {
                        ui.add_space(30.0);
                        let rate = |s: &Setup| {
                            share(
                                s.corners[0].spring_stiffness + s.corners[1].spring_stiffness,
                                s.corners[2].spring_stiffness + s.corners[3].spring_stiffness,
                            )
                        };
                        balance(ui, "Spring rate, front to rear", rate(&draft), rate(&base));
                    });
                });
            }
            Area::Dampers => {
                let damper = |s: &Setup, w: usize| {
                    let c = &s.corners[w];
                    Damper {
                        bump: c.bump,
                        rebound: c.rebound,
                        fast_bump: c.fast_bump,
                        fast_from: c.fast_bump_threshold,
                    }
                };
                let size = vec2(ui.available_width().min(520.0), 230.0);
                charts::dampers(ui, [damper(&draft, 0), damper(&draft, 2)], [damper(&base, 0), damper(&base, 2)], size);
            }
            Area::Geometry => {
                let camber = |s: &Setup| s.corners.map(|c| c.axis_inclination);
                charts::camber(ui, camber(&draft), camber(&base), vec2(260.0, 200.0));
            }
            Area::RollBars => {
                let bars = |s: &Setup| share(s.front_roll_bar, s.rear_roll_bar);
                balance(ui, "Anti-roll stiffness, front to rear", bars(&draft), bars(&base));
            }
            Area::Brakes => {
                let brakes = |s: &Setup| share(s.front_brake_pressure, s.rear_brake_pressure);
                balance(ui, "Brake balance", brakes(&draft), brakes(&base));
            }
            Area::Differentials | Area::Gears => {}
        }
        if area == Area::Gears {
            self.gears_ui(ui);
            return;
        }
        ui.add_space(8.0);
        if WHEEL_AREAS.contains(&area) {
            self.wheel_rows(ui, area);
        } else {
            self.rows(ui, area);
        }
        if area == Area::Differentials {
            ui.add_space(10.0);
            self.maps_ui(ui, wide);
        }
    }

    /// The values of a page without wheels (the diff maps' knots left out), one row each.
    fn rows(&mut self, ui: &mut Ui, area: Area) {
        let specs = self.specs();
        let (mut now, base) = (values(&self.draft), values(&self.base));
        for (i, (spec, range)) in specs.iter().enumerate() {
            if spec.area != area
                || spec.index.is_some()
                || !matches!(spec.bounds, rbr_gameplay::setup::Bounds::Triple(_))
            {
                continue;
            }
            title(ui, spec, range);
            ui.horizontal(|ui| {
                ui.add_space(NAME);
                let color = match spec.label.split(' ').next() {
                    Some("Front") => FRONT,
                    Some("Rear") => REAR,
                    _ => NEUTRAL,
                };
                controls(ui, spec, *range, &mut now[i], base[i], color);
            });
            ui.add_space(6.0);
        }
        self.draft = with_values(&self.draft, &now);
    }

    /// The values of a page per wheel: a row per axle while its wheels are set together, else per wheel.
    fn wheel_rows(&mut self, ui: &mut Ui, area: Area) {
        let specs = self.specs();
        let slot = wheel_area(area);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Left and right set together:").weak());
            for (axle, name) in AXLES.into_iter().enumerate() {
                let was = self.linked[slot][axle];
                let toggle = ui.selectable_label(was, capital(name)).on_hover_text(if was {
                    "Set this axle's left and right wheels apart"
                } else {
                    "Set this axle's wheels together: the right wheel takes the left's values"
                });
                if toggle.clicked() {
                    self.linked[slot][axle] = !was;
                    if !was {
                        self.copy_left_to_right(&specs, area, axle);
                    }
                }
            }
        });
        ui.add_space(4.0);
        let (mut now, base) = (values(&self.draft), values(&self.base));
        let mut labels: Vec<&'static str> = Vec::new();
        for (spec, _) in specs.iter().filter(|(s, _)| s.area == area) {
            if !labels.contains(&spec.label) {
                labels.push(spec.label);
            }
        }
        let find = |label: &str, w: usize| {
            specs.iter().position(|(s, _)| s.area == area && s.label == label && s.index == Some(w))
        };
        for label in labels {
            let Some(first) = find(label, 0) else { continue };
            title(ui, &specs[first].0, &specs[first].1);
            for (axle, axle_name) in AXLES.into_iter().enumerate() {
                let color = if axle == 0 { FRONT } else { REAR };
                // One row for the axle while it is set together and its wheels agree (a difference never hides).
                let pair = (find(label, axle * 2), find(label, axle * 2 + 1));
                let agree = matches!(pair, (Some(l), Some(r)) if now[l] == now[r]);
                let wheels: Vec<(String, Vec<usize>)> = if self.linked[slot][axle] && agree {
                    vec![(capital(axle_name), [pair.0, pair.1].into_iter().flatten().collect())]
                } else {
                    (0..2)
                        .filter_map(|side| {
                            let w = axle * 2 + side;
                            Some((format!("{} {}", capital(axle_name), ["left", "right"][side]), vec![find(label, w)?]))
                        })
                        .collect()
                };
                for (name, indices) in wheels {
                    let Some(&i) = indices.first() else { continue };
                    ui.horizontal(|ui| {
                        let layout = egui::Layout::left_to_right(egui::Align::Center);
                        ui.allocate_ui_with_layout(vec2(NAME, 18.0), layout, |ui| {
                            ui.set_min_width(NAME);
                            ui.label(RichText::new(name).color(color));
                        });
                        let (spec, range) = &specs[i];
                        if controls(ui, spec, *range, &mut now[i], base[i], color) {
                            for &j in &indices[1..] {
                                now[j] = now[i];
                            }
                        }
                    });
                }
            }
            ui.add_space(6.0);
        }
        self.draft = with_values(&self.draft, &now);
    }

    /// Gives the right wheel of `axle` the left's values of `area`.
    fn copy_left_to_right(&mut self, specs: &[(Spec, Range)], area: Area, axle: usize) {
        let mut now = values(&self.draft);
        for (i, (spec, _)) in specs.iter().enumerate().filter(|(_, (s, _))| s.area == area && s.index == Some(axle * 2))
        {
            if let Some(j) =
                specs.iter().position(|(s, _)| s.area == area && s.label == spec.label && s.index == Some(axle * 2 + 1))
            {
                now[j] = now[i];
            }
        }
        self.draft = with_values(&self.draft, &now);
    }

    /// The diff maps: one at a time, its pedal and speed curves dragged, its knots as numbers below.
    fn maps_ui(&mut self, ui: &mut Ui, wide: bool) {
        let specs = self.specs();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Lock map").strong());
            for (m, name) in MAP_NAMES.into_iter().enumerate() {
                let differs = self.draft.diff_maps[m] != self.base.diff_maps[m];
                let text = if differs { RichText::new(format!("{name} •")).color(DIFF) } else { RichText::new(name) };
                if ui.selectable_label(self.map == m, text).clicked() {
                    self.map = m;
                }
            }
        });
        let m = self.map;
        let range_of = |key: &str| {
            let prefix = rbr_gameplay::setup::DIFF_MAPS[m];
            specs.iter().find(|(s, _)| s.key.starts_with(&format!("{prefix}{key}"))).map(|(_, r)| *r)
        };
        let fixed = Range { min: 0.0, max: 0.0, step: 0.0, wrap: false };
        let lock = range_of("DiffThrottle_").unwrap_or(fixed);
        let factor = range_of("SpeedMapFactor_").unwrap_or(fixed);
        let torque = match m {
            0 | 1 => self.draft.center_diff_torque,
            2 => self.draft.front_diff_torque,
            _ => self.draft.rear_diff_torque,
        };
        let base = self.base.diff_maps[m];
        let width = ui.available_width();
        let size = if wide { vec2((width - 12.0) / 2.0, 220.0) } else { vec2(width.min(520.0), 220.0) };
        let map = &mut self.draft.diff_maps[m];
        let layout =
            if wide { egui::Layout::left_to_right(egui::Align::Min) } else { egui::Layout::top_down(egui::Align::Min) };
        ui.with_layout(layout, |ui| {
            charts::pedal_locks(ui, map, &base, lock, torque, size);
            charts::speed_locks(ui, map, &base, factor, size);
        });
        egui::CollapsingHeader::new("The map's knots as numbers").id_salt("setup-map-knots").show(ui, |ui| {
            self.knots_ui(ui, &specs);
        });
    }

    fn knots_ui(&mut self, ui: &mut Ui, specs: &[(Spec, Range)]) {
        let (mut now, base) = (values(&self.draft), values(&self.base));
        let map: Vec<usize> = (0..specs.len())
            .filter(|&i| specs[i].0.area == Area::Differentials && specs[i].0.index == Some(self.map))
            .collect();
        egui::Grid::new("setup-map").num_columns(MAP_POINTS + 1).spacing([6.0, 6.0]).show(ui, |ui| {
            ui.label("point");
            for n in 0..MAP_POINTS {
                ui.label(format!("{}", n * 10));
            }
            ui.end_row();
            for (row, name) in map.chunks(MAP_POINTS).zip(["throttle lock", "brake lock", "speed", "speed lock factor"])
            {
                let unit = specs[row[0]].0.unit;
                ui.label(format!("{name} ({})", unit.suffix));
                for &i in row {
                    let (spec, range) = &specs[i];
                    let mut shown = now[i] * unit.scale;
                    let fixed = range.max <= range.min || range.step <= 0.0;
                    let field = DragValue::new(&mut shown)
                        .range(range.min * unit.scale..=range.max * unit.scale)
                        .speed(range.step * unit.scale)
                        .fixed_decimals(spec.unit.decimals);
                    let response = ui.add_enabled(!fixed, field);
                    if now[i] != base[i] {
                        ui.painter().rect_stroke(
                            response.rect.expand(1.0),
                            CornerRadius::same(3),
                            Stroke::new(1.0, DIFF),
                            StrokeKind::Outside,
                        );
                    }
                    if response.changed() {
                        now[i] = range.toward(now[i], shown / unit.scale);
                    }
                }
                ui.end_row();
            }
        });
        self.draft = with_values(&self.draft, &now);
    }

    /// The drop gear: a card per option with its teeth and ratio, the ratio's bar against the longest.
    fn gears_ui(&mut self, ui: &mut Ui) {
        let specs = self.specs();
        let Some((_, range)) = specs.iter().find(|(s, _)| s.area == Area::Gears) else { return };
        let gears = self.car.as_ref().map_or(Vec::new(), |c| c.gears.clone());
        let ids: Vec<u32> = (range.min as u32..=range.max as u32).collect();
        let ratio = |id: u32| gears.iter().find(|g| g.0 == id).filter(|g| g.2 > 0.0).map(|&(_, p, g)| p / g);
        let teeth = |id: u32| gears.iter().find(|g| g.0 == id).map_or(format!("{id}"), |&(_, p, g)| format!("{p}:{g}"));
        let longest = ids.iter().filter_map(|&id| ratio(id)).fold(0.0_f32, f32::max);
        if ids.len() > 12 || longest <= 0.0 {
            // Without the teeth (or with very many options) the plain list.
            ui.horizontal(|ui| {
                ui.label("Drop gear");
                ui.add_enabled_ui(range.max > range.min, |ui| {
                    combo(ui, "drop-gear", None, &mut self.draft.drop_gear, &ids, teeth)
                });
            });
            return;
        }
        ui.label(RichText::new("Drop gear").strong());
        ui.add_space(4.0);
        for id in ids {
            let on = self.draft.drop_gear == id;
            let was = self.base.drop_gear == id;
            let (rect, response) = ui.allocate_exact_size(vec2(420.0, 30.0), Sense::click());
            let visuals = ui.visuals();
            let fill = if on {
                visuals.selection.bg_fill
            } else if response.hovered() {
                visuals.widgets.hovered.weak_bg_fill
            } else {
                visuals.faint_bg_color
            };
            let painter = ui.painter();
            painter.rect(
                rect,
                CornerRadius::same(5),
                fill,
                if was && !on { Stroke::new(1.5, DIFF) } else { Stroke::NONE },
                StrokeKind::Inside,
            );
            let r = ratio(id).unwrap_or(0.0);
            let bar = Rect::from_min_size(rect.left_top() + vec2(150.0, 11.0), vec2(250.0 * r / longest, 8.0));
            painter.rect_filled(bar, CornerRadius::same(2), FRONT.gamma_multiply(if on { 1.0 } else { 0.5 }));
            let text = if on { Color32::WHITE } else { visuals.text_color() };
            painter.text(
                rect.left_center() + vec2(10.0, 0.0),
                Align2::LEFT_CENTER,
                teeth(id),
                FontId::proportional(14.0),
                text,
            );
            painter.text(
                rect.left_center() + vec2(70.0, 0.0),
                Align2::LEFT_CENTER,
                format!("{r:.2}:1"),
                FontId::proportional(12.0),
                text,
            );
            if was && !on {
                painter.text(
                    rect.right_center() + vec2(8.0, 0.0),
                    Align2::LEFT_CENTER,
                    "compared",
                    FontId::proportional(11.0),
                    DIFF,
                );
            }
            if response.clicked() && range.max > range.min {
                self.draft.drop_gear = id;
            }
            ui.add_space(2.0);
        }
    }
}

/// The per-wheel area's slot in [`SetupPage::linked`].
fn wheel_area(area: Area) -> usize {
    WHEEL_AREAS.iter().position(|a| *a == area).unwrap_or(0)
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Every per-wheel area's axles set together where the setup's left and right are alike.
pub(super) fn linked_for(setup: &Setup, specs: &[(Spec, Range)]) -> [[bool; 2]; 4] {
    WHEEL_AREAS.map(|area| SetupPage::alike(setup, specs, area))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `ui` in a fixed area at the top left for each pass's pointer events.
    fn passes(passes: &[Vec<egui::Event>], mut draw: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        for events in passes {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 100.0))),
                events: events.clone(),
                ..Default::default()
            };
            let mut output = ctx.run_pass(input, |ctx| {
                egui::Area::new(egui::Id::new("track")).fixed_pos(egui::Pos2::ZERO).show(ctx, |ui| draw(ui));
            });
            output.textures_delta.clear();
        }
    }

    fn click(x: f32) -> Vec<Vec<egui::Event>> {
        let pos = egui::pos2(x, TRACK.y / 2.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![vec![], vec![egui::Event::PointerMoved(pos)], vec![button(true)], vec![button(false)], vec![]]
    }

    /// A click on the track takes the game's steps toward it: three quarters along 0..1 by 0.1 is 0.7 or 0.8, never
    /// a value between the steps; a fixed range takes no clicks.
    #[test]
    fn a_click_on_the_track_steps_toward_it() {
        let range = Range { min: 0.0, max: 1.0, step: 0.1, wrap: false };
        let mut value = 0.2;
        let x = 7.0 + (TRACK.x - 14.0) * 0.75;
        passes(&click(x), |ui| {
            track(ui, &mut value, 0.2, range, FRONT);
        });
        let steps = (value - 0.2) / 0.1;
        assert!((steps - steps.round()).abs() < 1e-4 && (0.69..=0.81).contains(&value), "{value}");
        assert_eq!(value, range.toward(0.2, 0.75));
    }

    #[test]
    fn a_difference_shows_more_decimals_rather_than_none() {
        assert_eq!(signed(0.6, 0), "+1");
        assert_eq!(signed(0.46, 0), "+0.5");
        assert_eq!(signed(-0.02, 1), "-0.02");
        assert_eq!(signed(0.0001, 0), "+0.000");
    }
}
