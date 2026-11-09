//! The Car setup page's diagrams, drawn with egui's painter: what the values do, with the comparison's values
//! (`base`) ghosted behind them. The diff maps' curves are edited by dragging their knots.

use egui::emath::Rot2;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use rbr_gameplay::setup::{DiffMap, Range, MAP_POINTS};

/// The front axle's colour.
pub const FRONT: Color32 = Color32::from_rgb(95, 175, 235);
/// The rear axle's colour.
pub const REAR: Color32 = Color32::from_rgb(200, 140, 235);
/// What a value is compared with: drawn dashed or hollow behind it.
pub const BASE: Color32 = Color32::from_gray(125);
/// A value that differs from the comparison.
pub const DIFF: Color32 = Color32::from_rgb(240, 165, 70);

/// The throttle curve's colour on the diff maps.
const THROTTLE: Color32 = Color32::from_rgb(110, 200, 120);
/// The brake curve's colour on the diff maps.
const BRAKE: Color32 = Color32::from_rgb(235, 110, 110);

/// One damper's values as the setup holds them (SI: Ns/m and m/s).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Damper {
    pub bump: f32,
    pub rebound: f32,
    pub fast_bump: f32,
    /// The wheel speed the fast bump rate takes over from (`BumpHighSpeedBreak`).
    pub fast_from: f32,
}

/// One coil-over's springs as the setup holds them (SI: m and N/m).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spring {
    pub length: f32,
    pub rate: f32,
    pub helper_length: f32,
    pub helper_rate: f32,
}

/// Small axis and legend type.
fn small() -> FontId {
    FontId::proportional(11.0)
}

fn label_colour() -> Color32 {
    Color32::from_gray(170)
}

fn grid_stroke() -> Stroke {
    Stroke::new(1.0, Color32::from_gray(60))
}

fn zero_stroke() -> Stroke {
    Stroke::new(1.0, Color32::from_gray(105))
}

/// A non-finite setup value reads as 0, so no input can panic or poison a scale.
fn clean(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

/// A legend row of swatch-plus-label entries across the top of `plot`.
fn legend(painter: &egui::Painter, plot: Rect, entries: &[(&str, Color32, bool)]) {
    let mut x = plot.left() + 4.0;
    let y = plot.top() + 7.0;
    for (text, colour, dashed) in entries {
        let from = Pos2::new(x, y);
        let to = Pos2::new(x + 16.0, y);
        let stroke = Stroke::new(2.0, *colour);
        if *dashed {
            painter.extend(Shape::dashed_line(&[from, to], stroke, 4.0, 3.0));
        } else {
            painter.line_segment([from, to], stroke);
        }
        let placed = painter.text(Pos2::new(x + 20.0, y), Align2::LEFT_CENTER, *text, small(), label_colour());
        x = placed.right() + 12.0;
    }
}

/// The dampers' force over the wheel's speed: bump left of 0 (the slow rate up to `fast_from`, the fast rate past
/// it), rebound right of 0; front and rear (`[front, rear]`), the comparison's dashed.
pub fn dampers(ui: &mut Ui, now: [Damper; 2], base: [Damper; 2], size: Vec2) {
    let clean_damper = |d: &Damper| Damper {
        bump: clean(d.bump).max(0.0),
        rebound: clean(d.rebound).max(0.0),
        fast_bump: clean(d.fast_bump).max(0.0),
        fast_from: clean(d.fast_from).max(0.0),
    };
    let now = [clean_damper(&now[0]), clean_damper(&now[1])];
    let base = [clean_damper(&base[0]), clean_damper(&base[1])];

    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 42.0, rect.top() + 20.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - 18.0),
    );
    if plot.width() <= 0.0 || plot.height() <= 0.0 {
        return;
    }
    legend(
        &painter,
        Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), plot.top())),
        &[("Front", FRONT, false), ("Rear", REAR, false), ("Compared", BASE, true)],
    );

    let mut xmax: f32 = 0.5;
    for d in now.iter().chain(base.iter()) {
        xmax = xmax.max(2.0 * d.fast_from);
    }
    if !xmax.is_finite() || xmax <= 0.0 {
        xmax = 0.5;
    }
    let mut ymax: f32 = 1.0;
    for d in now.iter().chain(base.iter()) {
        ymax = ymax.max(force(d, xmax).abs()).max(force(d, -xmax).abs());
    }
    if !ymax.is_finite() || ymax <= 0.0 {
        ymax = 1.0;
    }
    ymax *= 1.1;

    let to_px = |v: f32, f: f32| {
        Pos2::new(
            plot.left() + (v + xmax) / (2.0 * xmax) * plot.width(),
            plot.center().y - f / ymax * (plot.height() / 2.0),
        )
    };
    for fx in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let x = plot.left() + (fx + 1.0) / 2.0 * plot.width();
        painter.vline(x, plot.y_range(), if fx == 0.0 { zero_stroke() } else { grid_stroke() });
    }
    for fy in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let y = plot.center().y - fy * (plot.height() / 2.0);
        painter.hline(plot.x_range(), y, if fy == 0.0 { zero_stroke() } else { grid_stroke() });
    }
    let kilo = ymax >= 1500.0;
    let fmt_f = |f: f32| {
        if kilo {
            format!("{:.1}k", f / 1000.0)
        } else {
            format!("{:.0}", f)
        }
    };
    painter.text(
        Pos2::new(plot.left() - 4.0, plot.top()),
        Align2::RIGHT_CENTER,
        fmt_f(ymax / 1.1),
        small(),
        label_colour(),
    );
    painter.text(Pos2::new(plot.left() - 4.0, plot.center().y), Align2::RIGHT_CENTER, "0", small(), label_colour());
    painter.text(
        Pos2::new(plot.left() - 4.0, plot.bottom()),
        Align2::RIGHT_CENTER,
        fmt_f(-ymax / 1.1),
        small(),
        label_colour(),
    );
    painter.text(
        Pos2::new(plot.left(), plot.bottom() + 4.0),
        Align2::LEFT_TOP,
        format!("-{xmax:.2}"),
        small(),
        label_colour(),
    );
    painter.text(Pos2::new(plot.center().x, plot.bottom() + 4.0), Align2::CENTER_TOP, "0", small(), label_colour());
    painter.text(
        Pos2::new(plot.right(), plot.bottom() + 4.0),
        Align2::RIGHT_TOP,
        format!("{xmax:.2} m/s"),
        small(),
        label_colour(),
    );
    painter.text(Pos2::new(plot.left() + 2.0, plot.top() + 2.0), Align2::LEFT_TOP, "bump", small(), label_colour());
    painter.text(
        Pos2::new(plot.right() - 2.0, plot.top() + 2.0),
        Align2::RIGHT_TOP,
        "rebound",
        small(),
        label_colour(),
    );

    let curve = |d: &Damper| {
        (0..=64)
            .map(|k| {
                let v = -xmax + 2.0 * xmax * k as f32 / 64.0;
                to_px(v, force(d, v))
            })
            .collect::<Vec<_>>()
    };
    for d in &base {
        painter.extend(Shape::dashed_line(&curve(d), Stroke::new(1.25, BASE), 5.0, 4.0));
    }
    for (d, colour) in [&now[0], &now[1]].into_iter().zip([FRONT, REAR]) {
        painter.add(Shape::line(curve(d), Stroke::new(2.0, colour)));
    }
}

/// A damper's force at wheel speed `v` (bump `v < 0`, rebound above): the slow bump rate to `fast_from`, then the
/// fast rate continuing from the break point; the slow rate throughout when `fast_from` is 0.
fn force(d: &Damper, v: f32) -> f32 {
    if v >= 0.0 {
        return d.rebound * v;
    }
    let s = -v;
    if d.fast_from <= 0.0 || s <= d.fast_from {
        return -d.bump * s;
    }
    -(d.bump * d.fast_from + d.fast_bump * (s - d.fast_from))
}

/// A coil spring's zigzag from `bottom` up `len_px`, `coils` turns `half_w` half as wide.
fn coil_points(cx: f32, bottom: f32, len_px: f32, coils: usize, half_w: f32) -> Vec<Pos2> {
    let steps = (coils * 2).max(2);
    let mut points = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let x = if k == 0 || k == steps {
            cx
        } else if k % 2 == 1 {
            cx - half_w
        } else {
            cx + half_w
        };
        points.push(Pos2::new(x, bottom - len_px * k as f32 / steps as f32));
    }
    points
}

/// The front and rear coil-overs side by side, to one scale: the main spring under the helper spring, each coil's
/// length its free length, the wire's weight its rate; the comparison's outline behind them.
pub fn springs(ui: &mut Ui, now: [Spring; 2], base: [Spring; 2], size: Vec2) {
    let clean_spring = |s: &Spring| Spring {
        length: clean(s.length).max(0.0),
        rate: clean(s.rate).max(0.0),
        helper_length: clean(s.helper_length).max(0.0),
        helper_rate: clean(s.helper_rate).max(0.0),
    };
    let now = now.map(|s| clean_spring(&s));
    let base = base.map(|s| clean_spring(&s));

    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top() + 24.0),
        Pos2::new(rect.right() - 8.0, rect.bottom() - 36.0),
    );
    if plot.width() <= 0.0 || plot.height() <= 0.0 {
        return;
    }
    legend(
        &painter,
        Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), plot.top())),
        &[("Front", FRONT, false), ("Rear", REAR, false), ("Compared", BASE, true)],
    );

    let total = now.iter().chain(&base).map(|s| s.length + s.helper_length).fold(0.1_f32, f32::max);
    let scale = plot.height() / total;
    let max_rate = now.iter().chain(&base).map(|s| s.rate.max(s.helper_rate)).fold(1.0_f32, f32::max);
    let ground = plot.bottom();
    let half_w = (plot.width() * 0.09).clamp(10.0, 20.0);
    for (axle, (current, compared)) in now.iter().zip(&base).enumerate() {
        let colour = [FRONT, REAR][axle];
        let cx = plot.left() + plot.width() * (0.25 + 0.5 * axle as f32);
        // The comparison: the outline of its main spring and of its helper above it.
        if compared != current {
            let main_top = ground - compared.length * scale;
            let helper_top = main_top - compared.helper_length * scale;
            let outline = |top: f32, bottom: f32, w: f32| {
                let r = Rect::from_min_max(Pos2::new(cx - w, top), Pos2::new(cx + w, bottom));
                let corners = [r.left_bottom(), r.left_top(), r.right_top(), r.right_bottom()];
                painter.extend(Shape::dashed_line(&corners, Stroke::new(1.0, BASE), 4.0, 3.0));
            };
            outline(main_top, ground, half_w + 4.0);
            outline(helper_top, main_top, half_w * 0.7 + 4.0);
        }
        let main_top = ground - current.length * scale;
        let helper_top = main_top - current.helper_length * scale;
        let weight = |rate: f32| 1.0 + 2.5 * rate / max_rate;
        painter.add(Shape::line(
            coil_points(cx, ground, current.length * scale, 6, half_w),
            Stroke::new(weight(current.rate), colour),
        ));
        painter.add(Shape::line(
            coil_points(cx, main_top, current.helper_length * scale, 3, half_w * 0.7),
            Stroke::new(weight(current.helper_rate), colour.gamma_multiply(0.7)),
        ));
        // The perch between the springs, the top mount and the ground.
        let plate =
            |y: f32, w: f32, stroke: Stroke| painter.line_segment([Pos2::new(cx - w, y), Pos2::new(cx + w, y)], stroke);
        plate(main_top, half_w + 2.0, Stroke::new(2.0, label_colour()));
        plate(helper_top, half_w * 0.7 + 2.0, Stroke::new(1.5, label_colour()));
        plate(ground, half_w + 6.0, zero_stroke());

        let text = |dy: f32, text: String, colour: Color32| {
            painter.text(Pos2::new(cx, rect.bottom() - dy), Align2::CENTER_TOP, text, small(), colour);
        };
        text(32.0, format!("{:.0} mm · {:.0} N/mm", current.length * 1e3, current.rate * 1e-3), colour);
        text(
            18.0,
            format!("helper {:.0} mm · {:.0} N/mm", current.helper_length * 1e3, current.helper_rate * 1e-3),
            label_colour(),
        );
    }
}

/// The four wheels from the front (`[LF, RF, LB, RB]` axis inclinations, radians; the front pair over the rear),
/// each tilted by its camber, exaggerated so a tenth of a degree shows; the comparison's outline behind them.
pub fn camber(ui: &mut Ui, now: [f32; 4], base: [f32; 4], size: Vec2) {
    /// How many times over a wheel leans, so small cambers show.
    const EXAGGERATE: f32 = 4.0;
    const NAMES: [&str; 4] = ["LF", "RF", "LB", "RB"];

    let now: [f32; 4] = now.map(clean);
    let base: [f32; 4] = base.map(clean);

    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top() + 20.0),
        Pos2::new(rect.right() - 8.0, rect.bottom() - 8.0),
    );
    if plot.width() <= 0.0 || plot.height() <= 0.0 {
        return;
    }
    legend(
        &painter,
        Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), plot.top())),
        &[("Front", FRONT, false), ("Rear", REAR, false), ("Compared", BASE, true)],
    );

    let cell = Vec2::new(plot.width() / 2.0, plot.height() / 2.0);
    for (i, name) in NAMES.iter().enumerate() {
        let row = i / 2;
        let col = i % 2;
        let origin = Pos2::new(plot.left() + col as f32 * cell.x, plot.top() + row as f32 * cell.y);
        let contact = Pos2::new(origin.x + cell.x / 2.0, origin.y + cell.y - 16.0);
        let colour = if row == 0 { FRONT } else { REAR };
        // Left wheels lean right at the top for negative camber, right wheels left: the rotation's sign flips.
        let side = if col == 0 { 1.0 } else { -1.0 };
        let wheel = |angle: f32| {
            let tilt = (-angle * EXAGGERATE * side).clamp(-0.5, 0.5);
            let (w, h) = ((cell.x * 0.30).clamp(14.0, 30.0), (cell.y - 26.0).max(20.0));
            let rotation = Rot2::from_angle(tilt);
            let corner = |x: f32, y: f32| contact + rotation * Vec2::new(x, y);
            [corner(-w / 2.0, 0.0), corner(w / 2.0, 0.0), corner(w / 2.0, -h), corner(-w / 2.0, -h)]
        };
        let ground_y = contact.y + 3.0;
        painter.line_segment(
            [Pos2::new(contact.x - 22.0, ground_y), Pos2::new(contact.x + 22.0, ground_y)],
            zero_stroke(),
        );
        painter.add(Shape::convex_polygon(wheel(base[i]).to_vec(), Color32::TRANSPARENT, Stroke::new(1.0, BASE)));
        painter.add(Shape::convex_polygon(wheel(now[i]).to_vec(), Color32::from_gray(45), Stroke::new(2.0, colour)));
        painter.circle_filled(contact, 2.0, colour);
        let degrees = now[i] * 180.0 / std::f32::consts::PI;
        painter.text(
            Pos2::new(contact.x, origin.y + cell.y - 11.0),
            Align2::CENTER_TOP,
            format!("{name} {degrees:.2}°"),
            small(),
            label_colour(),
        );
    }
}

/// A diff map's lock (0 to 1) over the throttle and the brake pedal (0 to 100 %, a knot each 10 %): two curves, the
/// comparison's dashed. Each knot drags up and down by `lock`'s steps (`Range::toward`); its tooltip gives the lock
/// in % and in Nm of the diff's `torque`. Whether a value changed.
pub fn pedal_locks(ui: &mut Ui, map: &mut DiffMap, base: &DiffMap, lock: Range, torque: f32, size: Vec2) -> bool {
    let torque = clean(torque);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 42.0, rect.top() + 20.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - 18.0),
    );
    if plot.width() <= 0.0 || plot.height() <= 0.0 {
        return false;
    }
    legend(
        &painter,
        Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), plot.top())),
        &[("Throttle", THROTTLE, false), ("Brake", BRAKE, false), ("Compared", BASE, true)],
    );

    for pct in [0.0, 0.5, 1.0] {
        let x = plot.left() + pct * plot.width();
        painter.vline(x, plot.y_range(), if pct == 0.0 { zero_stroke() } else { grid_stroke() });
        let y = plot.bottom() - pct * plot.height();
        painter.hline(plot.x_range(), y, if pct == 0.0 { zero_stroke() } else { grid_stroke() });
    }
    for (pct, text) in [(0.0, "0"), (0.5, "50"), (1.0, "100 %")] {
        painter.text(
            Pos2::new(plot.left() - 4.0, plot.bottom() - pct * plot.height()),
            Align2::RIGHT_CENTER,
            text,
            small(),
            label_colour(),
        );
    }
    for (i, (pct, text)) in [(0.0, "0"), (0.5, "50"), (1.0, "100 % pedal")].into_iter().enumerate() {
        let anchor = if i == 0 {
            Align2::LEFT_TOP
        } else if i == 2 {
            Align2::RIGHT_TOP
        } else {
            Align2::CENTER_TOP
        };
        painter.text(
            Pos2::new(plot.left() + pct * plot.width(), plot.bottom() + 4.0),
            anchor,
            text,
            small(),
            label_colour(),
        );
    }

    let to_px = |pedal: f32, value: f32| {
        Pos2::new(
            plot.left() + pedal.clamp(0.0, 1.0) * plot.width(),
            plot.bottom() - clean(value).clamp(0.0, 1.0) * plot.height(),
        )
    };
    let pedal_at = |n: usize| n as f32 / (MAP_POINTS as f32 - 1.0);
    let curves = [("throttle", THROTTLE, map.throttle, base.throttle), ("brake", BRAKE, map.brake, base.brake)];
    for (_, _, _, compared) in &curves {
        let points: Vec<Pos2> = (0..MAP_POINTS).map(|n| to_px(pedal_at(n), compared[n])).collect();
        painter.extend(Shape::dashed_line(&points, Stroke::new(1.25, BASE), 5.0, 4.0));
    }
    for (_, colour, values, _) in &curves {
        let points: Vec<Pos2> = (0..MAP_POINTS).map(|n| to_px(pedal_at(n), values[n])).collect();
        painter.add(Shape::line(points, Stroke::new(2.0, *colour)));
    }

    let usable = lock.max.is_finite() && lock.min.is_finite() && lock.max > lock.min && lock.step > 0.0;
    if !usable {
        return false;
    }
    let mut changed = false;
    for (curve, (name, colour, _, compared)) in curves.iter().enumerate() {
        let values = if curve == 0 { &mut map.throttle } else { &mut map.brake };
        for n in 0..MAP_POINTS {
            let value = clean(values[n]);
            let at = to_px(pedal_at(n), value);
            let id = ui.scope_id().with(("pedal", curve, n));
            let hit = Rect::from_center_size(at, Vec2::splat(14.0));
            let response = ui.interact(hit, id, Sense::drag());
            let hovered = response.hovered() || response.dragged();
            if response.dragged() {
                let pointer = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_hover_pos());
                if let Some(pointer) = pointer {
                    let target = ((plot.bottom() - pointer.y) / plot.height()).clamp(0.0, 1.0);
                    let next = lock.toward(value, target);
                    if next != value {
                        values[n] = next;
                        changed = true;
                    }
                }
            }
            let fill = if (value - clean(compared[n])).abs() > 1e-6 { DIFF } else { *colour };
            painter.circle(at, if hovered { 5.5 } else { 3.5 }, fill, Stroke::new(1.0, Color32::BLACK));
            response.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text(format!(
                "{name} {:.0} %: lock {:.0} % ({:.0} Nm)",
                n as f32 * 10.0,
                value * 100.0,
                value * torque
            ));
        }
    }
    changed
}

/// A diff map's speed lock: its factor (0 to 1) at each knot's speed (m/s, shown in km/h), the comparison's dashed.
/// Each knot's factor drags up and down by `factor`'s steps; the speeds are not dragged. Whether a value changed.
pub fn speed_locks(ui: &mut Ui, map: &mut DiffMap, base: &DiffMap, factor: Range, size: Vec2) -> bool {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 42.0, rect.top() + 20.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - 18.0),
    );
    if plot.width() <= 0.0 || plot.height() <= 0.0 {
        return false;
    }
    legend(
        &painter,
        Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), plot.top())),
        &[("Lock factor", THROTTLE, false), ("Compared", BASE, true)],
    );

    let speed_of = |v: f32| clean(v).max(0.0);
    let mut lo: f32 = f32::INFINITY;
    let mut hi: f32 = f32::NEG_INFINITY;
    for v in map.speed.iter().chain(base.speed.iter()) {
        lo = lo.min(speed_of(*v));
        hi = hi.max(speed_of(*v));
    }
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        lo = lo.min(0.0);
        hi = lo + 1.0;
        if hi <= lo {
            (lo, hi) = (0.0, 1.0);
        }
    }

    for fx in [0.0, 0.5, 1.0] {
        let x = plot.left() + fx * plot.width();
        painter.vline(x, plot.y_range(), if fx == 0.0 { zero_stroke() } else { grid_stroke() });
        let y = plot.bottom() - fx * plot.height();
        painter.hline(plot.x_range(), y, if fx == 0.0 { zero_stroke() } else { grid_stroke() });
    }
    for (pct, text) in [(0.0, "0"), (0.5, "50"), (1.0, "100 %")] {
        painter.text(
            Pos2::new(plot.left() - 4.0, plot.bottom() - pct * plot.height()),
            Align2::RIGHT_CENTER,
            text,
            small(),
            label_colour(),
        );
    }
    for (i, fx) in [0.0, 0.5, 1.0].into_iter().enumerate() {
        let anchor = if i == 0 {
            Align2::LEFT_TOP
        } else if i == 2 {
            Align2::RIGHT_TOP
        } else {
            Align2::CENTER_TOP
        };
        painter.text(
            Pos2::new(plot.left() + fx * plot.width(), plot.bottom() + 4.0),
            anchor,
            format!("{:.0} km/h", (lo + fx * (hi - lo)) * 3.6),
            small(),
            label_colour(),
        );
    }

    let to_px = |speed: f32, value: f32| {
        let fx = if hi > lo { (speed_of(speed) - lo) / (hi - lo) } else { 0.5 };
        Pos2::new(
            plot.left() + fx.clamp(0.0, 1.0) * plot.width(),
            plot.bottom() - clean(value).clamp(0.0, 1.0) * plot.height(),
        )
    };
    // In index order, even when the speeds do not increase: the knots still join up as the map lists them.
    let base_points: Vec<Pos2> = (0..MAP_POINTS).map(|n| to_px(base.speed[n], base.speed_factor[n])).collect();
    painter.extend(Shape::dashed_line(&base_points, Stroke::new(1.25, BASE), 5.0, 4.0));
    let points: Vec<Pos2> = (0..MAP_POINTS).map(|n| to_px(map.speed[n], map.speed_factor[n])).collect();
    painter.add(Shape::line(points, Stroke::new(2.0, THROTTLE)));

    let usable = factor.max.is_finite() && factor.min.is_finite() && factor.max > factor.min && factor.step > 0.0;
    if !usable {
        return false;
    }
    let mut changed = false;
    for n in 0..MAP_POINTS {
        let value = clean(map.speed_factor[n]);
        let at = to_px(map.speed[n], value);
        let id = ui.scope_id().with(("speed", n));
        let hit = Rect::from_center_size(at, Vec2::splat(14.0));
        let response = ui.interact(hit, id, Sense::drag());
        let hovered = response.hovered() || response.dragged();
        if response.dragged() {
            let pointer = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_hover_pos());
            if let Some(pointer) = pointer {
                let target = ((plot.bottom() - pointer.y) / plot.height()).clamp(0.0, 1.0);
                let next = factor.toward(value, target);
                if next != value {
                    map.speed_factor[n] = next;
                    changed = true;
                }
            }
        }
        let fill = if (value - clean(base.speed_factor[n])).abs() > 1e-6 { DIFF } else { THROTTLE };
        painter.circle(at, if hovered { 5.5 } else { 3.5 }, fill, Stroke::new(1.0, Color32::BLACK));
        response.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text(format!(
            "{:.0} km/h: {:.0} %",
            speed_of(map.speed[n]) * 3.6,
            value * 100.0
        ));
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws all five diagrams for a look: `cargo test -p rbr-launcher charts_png -- --ignored --nocapture`,
    /// then open the PNG in the temporary folder.
    #[test]
    #[ignore]
    fn charts_png() {
        let throttle = std::array::from_fn(|n| 0.6 * n as f32 / (MAP_POINTS as f32 - 1.0));
        let brake = std::array::from_fn(|n| 0.3 * n as f32 / (MAP_POINTS as f32 - 1.0));
        let speed = std::array::from_fn(|n| 55.6 * n as f32 / (MAP_POINTS as f32 - 1.0));
        let falling = std::array::from_fn(|n| 0.5 - 0.4 * n as f32 / (MAP_POINTS as f32 - 1.0));
        let flat = |v: f32| [v; MAP_POINTS];
        let map = DiffMap { throttle, brake, speed, speed_factor: falling };
        let base = DiffMap { throttle: flat(0.25), brake: flat(0.15), speed, speed_factor: flat(0.3) };
        let lock = Range { min: 0.0, max: 1.0, step: 0.01, wrap: false };
        let dampers_now = [
            Damper { bump: 4000.0, rebound: 6000.0, fast_bump: 1800.0, fast_from: 0.1 },
            Damper { bump: 3500.0, rebound: 5500.0, fast_bump: 1600.0, fast_from: 0.12 },
        ];
        let dampers_base = [
            Damper { bump: 3800.0, rebound: 5800.0, fast_bump: 1700.0, fast_from: 0.09 },
            Damper { bump: 3400.0, rebound: 5300.0, fast_bump: 1500.0, fast_from: 0.11 },
        ];
        let springs_now = [
            Spring { length: 0.38, rate: 25000.0, helper_length: 0.14, helper_rate: 19000.0 },
            Spring { length: 0.385, rate: 20000.0, helper_length: 0.14, helper_rate: 15000.0 },
        ];
        let springs_base = [
            Spring { length: 0.37, rate: 23000.0, helper_length: 0.13, helper_rate: 17000.0 },
            Spring { length: 0.375, rate: 19000.0, helper_length: 0.13, helper_rate: 14000.0 },
        ];
        let degree = std::f32::consts::PI / 180.0;
        let camber_now = [-2.52 * degree, -2.52 * degree, -0.58 * degree, -0.58 * degree];
        let camber_base = [-2.0 * degree, -2.0 * degree, -0.4 * degree, -0.4 * degree];
        let path = std::env::temp_dir().join("setup-charts.png");
        crate::capture::shot(&path, |ctx| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Dampers");
                            dampers(ui, dampers_now, dampers_base, egui::vec2(420.0, 220.0));
                            ui.label("Pedal locks");
                            pedal_locks(ui, &mut map.clone(), &base, lock, 400.0, egui::vec2(420.0, 220.0));
                            ui.label("Speed locks");
                            speed_locks(ui, &mut map.clone(), &base, lock, egui::vec2(420.0, 220.0));
                        });
                        ui.vertical(|ui| {
                            ui.label("Springs");
                            springs(ui, springs_now, springs_base, egui::vec2(260.0, 200.0));
                            ui.label("Camber");
                            camber(ui, camber_now, camber_base, egui::vec2(260.0, 200.0));
                        });
                    });
                });
            });
        })
        .unwrap();
    }
}
