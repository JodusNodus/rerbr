//! The Car setup page's editor of one area: the area's diagram, then each value as a track over its range with the
//! comparison's tick and the stretch between them, its − and + steps (the game's), the number, and the difference.
//! The per-wheel areas edit an axle's two wheels together until they are set apart.

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, DragValue, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
};
use rbr_gameplay::setup::{Area, Bounds, Range, Setup, Unit, DIFF_MAPS, MAP_POINTS};

use super::charts::{self, Damper, Spring, BASE, DIFF, FRONT, REAR};
use super::values::{edit, find, values, wheel, Field, AXLES, MAP_NAMES, WHEEL_AREAS};
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
fn difference(unit: Unit, now: f32, base: f32) -> Option<String> {
    (now != base)
        .then(|| format!("{} {}", signed((now - base) * unit.scale, unit.decimals), unit.suffix).trim_end().to_owned())
}

/// `value` with a sign and `decimals` decimals, or up to 3 more when those show 0.
fn signed(value: f32, decimals: usize) -> String {
    let places =
        (decimals..=decimals + 3).find(|&d| (value * 10f32.powi(d as i32)).abs() >= 0.5).unwrap_or(decimals + 3);
    format!("{value:+.places$}")
}

/// The track: the range, the value's fill and knob, the comparison's tick and the stretch between them. A drag or
/// click takes the game's steps toward the pointer (`range` is not fixed).
fn track(ui: &mut Ui, value: &mut f32, base: f32, range: Range, color: Color32) {
    let (rect, response) = ui.allocate_exact_size(TRACK, Sense::click_and_drag());
    let rail = rect.shrink2(vec2(7.0, 0.0));
    let span = range.max - range.min;
    let x_of = |v: f32| rail.left() + rail.width() * ((v - range.min) / span).clamp(0.0, 1.0);
    if let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
        *value = range.toward(*value, range.min + span * ((p.x - rail.left()) / rail.width()).clamp(0.0, 1.0));
    }
    let painter = ui.painter_at(rect.expand(2.0));
    let y = rect.center().y;
    let bar = Rect::from_min_max(pos2(rail.left(), y - 3.0), pos2(rail.right(), y + 3.0));
    painter.rect_filled(bar, CornerRadius::same(3), ui.visuals().extreme_bg_color);
    let (at, was) = (x_of(*value), x_of(base));
    let fill = Rect::from_min_max(bar.left_top(), pos2(at, bar.bottom()));
    painter.rect_filled(fill, CornerRadius::same(3), color.gamma_multiply(0.45));
    let differs = base != *value;
    if differs {
        let stretch = Rect::from_min_max(pos2(at.min(was), y - 1.5), pos2(at.max(was), y + 1.5));
        painter.rect_filled(stretch, CornerRadius::ZERO, DIFF);
        painter.line_segment([pos2(was, y - 7.0), pos2(was, y + 7.0)], Stroke::new(2.0, BASE));
    }
    let knob = if response.hovered() || response.dragged() { 7.5 } else { 6.5 };
    let ring = if differs { DIFF } else { ui.visuals().widgets.inactive.fg_stroke.color };
    painter.circle(pos2(at, y), knob, color, Stroke::new(1.5, ring));
}

/// A value's controls: − track + number difference; a value the car's options fix is only shown. Whether it
/// changed.
fn controls(ui: &mut Ui, field: &Field, value: &mut f32, base: f32, color: Color32) -> bool {
    let (unit, range) = (field.spec.unit, field.range);
    if field.fixed() {
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
    let text = RichText::new(difference(unit, *value, base).unwrap_or_default()).color(DIFF).size(12.0);
    ui.add_sized([86.0, 20.0], egui::Label::new(text)).on_hover_text(format!("compared: {}", unit.show(base)));
    *value != before
}

/// A value's title line: its label (its range on hover) and its hint.
fn title(ui: &mut Ui, field: &Field) {
    let (spec, range) = (&field.spec, field.range);
    let (unit, wrap) = (spec.unit, if range.wrap { ", wraps" } else { "" });
    let span = format!("{} to {}, step {}{wrap}", unit.show(range.min), unit.show(range.max), unit.show(range.step));
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(spec.label).strong()).on_hover_text(span);
        ui.label(RichText::new(hint(spec.label)).weak().size(12.0));
    });
}

/// A row's name (an axle, a wheel) in its colour, in a column of its own.
fn name(ui: &mut Ui, text: String, color: Color32) {
    ui.allocate_ui_with_layout(vec2(NAME, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(NAME);
        ui.label(RichText::new(text).color(color));
    });
}

/// A balance between front and rear: the bar split at the front's share, the comparison's tick.
fn balance(ui: &mut Ui, what: &str, front: f32, base: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(360.0_f32.min(ui.available_width()), 26.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(3.0));
    let share = if front.is_finite() { front.clamp(0.0, 1.0) } else { 0.5 };
    let radius = CornerRadius::same(4);
    painter.rect_filled(rect, radius, REAR.gamma_multiply(0.55));
    let split = Rect::from_min_max(rect.left_top(), pos2(rect.left() + rect.width() * share, rect.bottom()));
    painter.rect_filled(split, radius, FRONT.gamma_multiply(0.7));
    painter.rect_stroke(rect, radius, ui.visuals().widgets.noninteractive.bg_stroke, StrokeKind::Inside);
    let differs = base.is_finite() && base != share;
    if differs {
        let x = rect.left() + rect.width() * base.clamp(0.0, 1.0);
        painter.line_segment([pos2(x, rect.top() - 3.0), pos2(x, rect.bottom() + 3.0)], Stroke::new(2.0, DIFF));
    }
    let (font, text) = (FontId::proportional(12.5), Color32::from_gray(235));
    let front_text = format!("{:.0} % front", 100.0 * share);
    painter.text(rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, front_text, font.clone(), text);
    let rear_text = format!("{:.0} % rear", 100.0 * (1.0 - share));
    painter.text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, rear_text, font, text);
    ui.horizontal(|ui| {
        ui.label(RichText::new(what).weak().size(12.0));
        if differs {
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
    pub(super) fn area_ui(&mut self, ui: &mut Ui) {
        let area = self.area;
        let (now, base) = (values(&self.draft), values(&self.base));
        let inside: Vec<usize> = (0..self.fields.len()).filter(|&i| self.fields[i].spec.area == area).collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(area.name()).heading().strong());
            if inside.iter().any(|&i| now[i] != base[i]) {
                let back = ui.button("↺ All back").on_hover_text("This area's values back to the compared setup's");
                if back.clicked() {
                    edit(&mut self.draft, &self.base, |now, base| inside.iter().for_each(|&i| now[i] = base[i]));
                }
            }
        });
        ui.label(RichText::new(about(area)).weak());
        ui.add_space(6.0);
        self.diagram(ui, area);
        ui.add_space(8.0);
        match area {
            Area::Gears => self.gears_ui(ui),
            _ if WHEEL_AREAS.contains(&area) => self.wheel_rows(ui, area),
            _ => self.rows(ui, area),
        }
        if area == Area::Differentials {
            ui.add_space(10.0);
            self.maps_ui(ui);
        }
    }

    /// The area's diagram, above its values.
    fn diagram(&self, ui: &mut Ui, area: Area) {
        let (draft, base) = (&self.draft, &self.base);
        match area {
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
                let rate = |s: &Setup| {
                    let c = &s.corners;
                    share(c[0].spring_stiffness + c[1].spring_stiffness, c[2].spring_stiffness + c[3].spring_stiffness)
                };
                ui.horizontal_top(|ui| {
                    let (now, was) = ([spring(draft, 0), spring(draft, 2)], [spring(base, 0), spring(base, 2)]);
                    charts::springs(ui, now, was, vec2(320.0, 220.0));
                    ui.vertical(|ui| {
                        ui.add_space(30.0);
                        balance(ui, "Spring rate, front to rear", rate(draft), rate(base));
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
                let (now, was) = ([damper(draft, 0), damper(draft, 2)], [damper(base, 0), damper(base, 2)]);
                charts::dampers(ui, now, was, vec2(ui.available_width().min(520.0), 230.0));
            }
            Area::Geometry => {
                let camber = |s: &Setup| s.corners.map(|c| c.axis_inclination);
                charts::camber(ui, camber(draft), camber(base), vec2(320.0, 220.0));
            }
            Area::RollBars => {
                let bars = |s: &Setup| share(s.front_roll_bar, s.rear_roll_bar);
                balance(ui, "Anti-roll stiffness, front to rear", bars(draft), bars(base));
            }
            Area::Brakes => {
                let brakes = |s: &Setup| share(s.front_brake_pressure, s.rear_brake_pressure);
                balance(ui, "Brake balance", brakes(draft), brakes(base));
            }
            Area::Tyres | Area::Differentials | Area::Gears => {}
        }
    }

    /// The values of an area without wheels (the diff maps' knots left out), one row each.
    fn rows(&mut self, ui: &mut Ui, area: Area) {
        let fields = &self.fields;
        edit(&mut self.draft, &self.base, |now, base| {
            for (i, field) in fields.iter().enumerate() {
                let spec = &field.spec;
                if spec.area != area || spec.index.is_some() || !matches!(spec.bounds, Bounds::Triple(_)) {
                    continue;
                }
                title(ui, field);
                let color = match spec.label.split(' ').next() {
                    Some("Front") => FRONT,
                    Some("Rear") => REAR,
                    _ => NEUTRAL,
                };
                ui.horizontal(|ui| {
                    ui.add_space(NAME);
                    controls(ui, field, &mut now[i], base[i], color);
                });
                ui.add_space(6.0);
            }
        });
    }

    /// The values of an area per wheel: a row per axle while its wheels are set together and agree, else per wheel.
    fn wheel_rows(&mut self, ui: &mut Ui, area: Area) {
        let fields = &self.fields;
        let linked = &mut self.linked[WHEEL_AREAS.iter().position(|a| *a == area).unwrap_or(0)];
        let mut join = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Left and right set together:").weak());
            for (axle, axle_name) in AXLES.into_iter().enumerate() {
                let on = linked[axle];
                let tip = if on {
                    "Set this axle's left and right wheels apart"
                } else {
                    "Set this axle's wheels together: the right wheel takes the left's values"
                };
                if ui.selectable_label(on, capital(axle_name)).on_hover_text(tip).clicked() {
                    linked[axle] = !on;
                    join = (!on).then_some(axle);
                }
            }
        });
        ui.add_space(4.0);
        let labels: Vec<&str> =
            fields.iter().filter(|f| f.spec.area == area && f.spec.index == Some(0)).map(|f| f.spec.label).collect();
        edit(&mut self.draft, &self.base, |now, base| {
            if let Some(axle) = join {
                for label in &labels {
                    if let Some((i, j)) = wheel(fields, area, label, axle * 2).and_then(|i| Some((i, fields[i].pair?)))
                    {
                        now[j] = now[i];
                    }
                }
            }
            for label in &labels {
                let Some(first) = wheel(fields, area, label, 0) else { continue };
                title(ui, &fields[first]);
                for (axle, axle_name) in AXLES.into_iter().enumerate() {
                    let Some(left) = wheel(fields, area, label, axle * 2) else { continue };
                    let color = [FRONT, REAR][axle];
                    let axle_name = capital(axle_name);
                    let rows: Vec<(String, usize, Option<usize>)> = match fields[left].pair {
                        Some(right) if linked[axle] && now[left] == now[right] => vec![(axle_name, left, Some(right))],
                        right => [Some(left), right]
                            .into_iter()
                            .zip(["left", "right"])
                            .filter_map(|(i, side)| Some((format!("{axle_name} {side}"), i?, None)))
                            .collect(),
                    };
                    for (text, i, also) in rows {
                        ui.horizontal(|ui| {
                            name(ui, text, color);
                            if controls(ui, &fields[i], &mut now[i], base[i], color) {
                                also.into_iter().for_each(|j| now[j] = now[i]);
                            }
                        });
                    }
                }
                ui.add_space(6.0);
            }
        });
    }

    /// The diff maps: one at a time, its pedal and speed curves dragged, its knots as numbers below.
    fn maps_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Lock map").strong());
            for (m, map_name) in MAP_NAMES.into_iter().enumerate() {
                let differs = self.draft.diff_maps[m] != self.base.diff_maps[m];
                let text = if differs {
                    RichText::new(format!("{map_name} •")).color(DIFF)
                } else {
                    RichText::new(map_name)
                };
                if ui.selectable_label(self.map == m, text).clicked() {
                    self.map = m;
                }
            }
        });
        let m = self.map;
        let range_of = |key: &str| {
            let at = find(&self.fields, &format!("{}{key}_00", DIFF_MAPS[m]));
            at.map_or(Range { min: 0.0, max: 0.0, step: 0.0, wrap: false }, |i| self.fields[i].range)
        };
        let (lock, factor) = (range_of("DiffThrottle"), range_of("SpeedMapFactor"));
        let d = &self.draft;
        let torque = [d.center_diff_torque, d.center_diff_torque, d.front_diff_torque, d.rear_diff_torque][m];
        let width = ui.available_width();
        let wide = width >= 760.0;
        let size = if wide { vec2((width - 12.0) / 2.0, 220.0) } else { vec2(width.min(520.0), 220.0) };
        let (map, base) = (&mut self.draft.diff_maps[m], &self.base.diff_maps[m]);
        let layout =
            if wide { egui::Layout::left_to_right(egui::Align::Min) } else { egui::Layout::top_down(egui::Align::Min) };
        ui.with_layout(layout, |ui| {
            charts::pedal_locks(ui, map, base, lock, torque, size);
            charts::speed_locks(ui, map, base, factor, size);
        });
        egui::CollapsingHeader::new("The map's knots as numbers")
            .id_salt("setup-map-knots")
            .show(ui, |ui| self.knots_ui(ui));
    }

    fn knots_ui(&mut self, ui: &mut Ui) {
        let fields = &self.fields;
        let map: Vec<usize> = (0..fields.len())
            .filter(|&i| fields[i].spec.area == Area::Differentials && fields[i].spec.index == Some(self.map))
            .collect();
        edit(&mut self.draft, &self.base, |now, base| {
            egui::Grid::new("setup-map").num_columns(MAP_POINTS + 1).spacing([6.0, 6.0]).show(ui, |ui| {
                ui.label("pedal %");
                for n in 0..MAP_POINTS {
                    ui.label(format!("{}", n * 10));
                }
                ui.end_row();
                for (row, row_name) in
                    map.chunks(MAP_POINTS).zip(["throttle lock", "brake lock", "speed", "speed lock factor"])
                {
                    ui.label(format!("{row_name} ({})", fields[row[0]].spec.unit.suffix));
                    for &i in row {
                        let (unit, range) = (fields[i].spec.unit, fields[i].range);
                        let mut shown = now[i] * unit.scale;
                        let cell = DragValue::new(&mut shown)
                            .range(range.min * unit.scale..=range.max * unit.scale)
                            .speed(range.step * unit.scale)
                            .fixed_decimals(unit.decimals);
                        let response = ui.add_enabled(!fields[i].fixed(), cell);
                        if now[i] != base[i] {
                            let mark = Stroke::new(1.0, DIFF);
                            ui.painter().rect_stroke(
                                response.rect.expand(1.0),
                                CornerRadius::same(3),
                                mark,
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
        });
    }

    /// The drop gear: a card per option with its teeth and ratio, the ratio's bar against the longest.
    fn gears_ui(&mut self, ui: &mut Ui) {
        let Some(field) = find(&self.fields, "DropGearId").map(|i| &self.fields[i]) else { return };
        let range = field.range;
        let gears = self.car.as_ref().map_or(&[][..], |c| &c.gears[..]);
        let ids: Vec<u32> = (range.min as u32..=range.max as u32).collect();
        let ratio = |id: u32| gears.iter().find(|g| g.0 == id).filter(|g| g.2 > 0.0).map(|&(_, p, g)| p / g);
        let teeth = |id: u32| gears.iter().find(|g| g.0 == id).map_or(format!("{id}"), |&(_, p, g)| format!("{p}:{g}"));
        let longest = ids.iter().filter_map(|&id| ratio(id)).fold(0.0_f32, f32::max);
        if ids.len() > 12 || longest <= 0.0 {
            // Without the teeth, or with very many options: the plain list.
            ui.horizontal(|ui| {
                ui.label("Drop gear");
                ui.add_enabled_ui(!field.fixed(), |ui| {
                    combo(ui, "drop-gear", None, &mut self.draft.drop_gear, &ids, teeth)
                });
            });
            return;
        }
        ui.label(RichText::new("Drop gear").strong());
        if field.fixed() {
            ui.label(RichText::new("fixed for this car").weak());
        }
        ui.add_space(4.0);
        let mut pick = None;
        for id in ids {
            let (on, was) = (self.draft.drop_gear == id, self.base.drop_gear == id);
            let (rect, response) = ui.allocate_exact_size(vec2(440.0, 30.0), Sense::click());
            let visuals = ui.visuals();
            let fill = match (on, response.hovered() && !field.fixed()) {
                (true, _) => visuals.selection.bg_fill,
                (false, true) => visuals.widgets.hovered.weak_bg_fill,
                (false, false) => visuals.faint_bg_color,
            };
            let ring = if was && !on { Stroke::new(1.5, DIFF) } else { Stroke::NONE };
            let painter = ui.painter();
            painter.rect(rect, CornerRadius::same(5), fill, ring, StrokeKind::Inside);
            let r = ratio(id).unwrap_or(0.0);
            let bar = Rect::from_min_size(rect.left_top() + vec2(130.0, 11.0), vec2(220.0 * r / longest, 8.0));
            painter.rect_filled(bar, CornerRadius::same(2), FRONT.gamma_multiply(if on { 1.0 } else { 0.5 }));
            let text = if on { Color32::WHITE } else { visuals.text_color() };
            let left = rect.left_center();
            painter.text(left + vec2(10.0, 0.0), Align2::LEFT_CENTER, teeth(id), FontId::proportional(14.0), text);
            painter.text(
                left + vec2(64.0, 0.0),
                Align2::LEFT_CENTER,
                format!("{r:.2}:1"),
                FontId::proportional(12.0),
                text,
            );
            if was && !on {
                let at = rect.right_center() - vec2(10.0, 0.0);
                painter.text(at, Align2::RIGHT_CENTER, "compared", FontId::proportional(11.0), DIFF);
            }
            if response.clicked() && !field.fixed() {
                pick = Some(id);
            }
            ui.add_space(2.0);
        }
        if let Some(id) = pick {
            self.draft.drop_gear = id;
        }
    }
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Each per-wheel area's axles ([`WHEEL_AREAS`]) set together where the setup's left and right all agree.
pub(super) fn linked_for(setup: &Setup, fields: &[Field]) -> [[bool; 2]; 4] {
    let list = values(setup);
    WHEEL_AREAS.map(|area| {
        let apart = |axle: usize| {
            fields.iter().enumerate().any(|(i, f)| {
                f.spec.area == area && f.spec.index == Some(axle * 2) && f.pair.is_some_and(|j| list[j] != list[i])
            })
        };
        [!apart(0), !apart(1)]
    })
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
                egui::Area::new(egui::Id::unique("track")).fixed_pos(egui::Pos2::ZERO).show(ctx, |ui| draw(ui));
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

    /// A click on the track takes the game's steps toward it: three quarters along 0..1 by 0.1 from 0.2 is 0.7 or
    /// 0.8, never a value between the steps.
    #[test]
    fn a_click_on_the_track_steps_toward_it() {
        let range = Range { min: 0.0, max: 1.0, step: 0.1, wrap: false };
        let mut value = 0.2;
        let x = 7.0 + (TRACK.x - 14.0) * 0.75;
        passes(&click(x), |ui| track(ui, &mut value, 0.2, range, FRONT));
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
