//! The Car setup page's diagrams, drawn with egui's painter: what the values do, with the comparison's values
//! (`base`) ghosted behind them. The diff maps' curves are edited by dragging their knots.

use egui::emath::Rot2;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use rbr_gameplay::setup::{DiffMap, Range, MAP_POINTS};

/// The front axle's colour.
pub const FRONT: Color32 = Color32::from_rgb(95, 175, 235);
/// The rear axle's colour.
pub const REAR: Color32 = Color32::from_rgb(200, 140, 235);
/// What a value is compared with: drawn dashed or hollow behind it.
pub const BASE: Color32 = Color32::from_gray(125);
/// A value that differs from the comparison.
pub const DIFF: Color32 = Color32::from_rgb(240, 165, 70);

/// The diff maps' throttle and brake curves.
const THROTTLE: Color32 = Color32::from_rgb(110, 200, 120);
const BRAKE: Color32 = Color32::from_rgb(235, 110, 110);
/// The cards' background, labels, grid and zero lines.
const CARD: Color32 = Color32::from_gray(28);
const LABEL: Color32 = Color32::from_gray(170);
const GRID: Color32 = Color32::from_gray(60);
const ZERO: Color32 = Color32::from_gray(105);

/// One damper's values as the setup holds them (SI: Ns/m and m/s).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Damper {
    pub bump: f32,
    pub rebound: f32,
    pub fast_bump: f32,
    /// How fast the wheel must rise for the fast bump rate to take over (`BumpHighSpeedBreak`).
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

fn small() -> FontId {
    FontId::proportional(11.0)
}

/// A setup value as a diagram takes it: a non-finite one as 0, so nothing can poison a scale.
fn clean(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

/// A legend entry: its text, colour and whether its line is dashed.
type Entry<'a> = (&'a str, Color32, bool);

const AXLES_LEGEND: [Entry; 3] = [("Front", FRONT, false), ("Rear", REAR, false), ("Compared", BASE, true)];

/// A diagram's card of `size`: its background and legend, and the plot inside the margins for the labels (`left` of
/// it, `bottom` under it); `None` without room for a plot.
fn card(ui: &mut Ui, size: Vec2, legend: &[Entry], left: f32, bottom: f32) -> Option<(Painter, Rect)> {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, CARD);
    let mut x = rect.left() + 8.0;
    let y = rect.top() + 11.0;
    for (text, colour, dashed) in legend {
        let (from, to) = (Pos2::new(x, y), Pos2::new(x + 16.0, y));
        let stroke = Stroke::new(2.0, *colour);
        if *dashed {
            painter.extend(Shape::dashed_line(&[from, to], stroke, 4.0, 3.0));
        } else {
            painter.line_segment([from, to], stroke);
        }
        x = painter.text(Pos2::new(x + 20.0, y), Align2::LEFT_CENTER, *text, small(), LABEL).right() + 12.0;
    }
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + left, rect.top() + 24.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - bottom),
    );
    (plot.width() > 0.0 && plot.height() > 0.0).then_some((painter, plot))
}

/// The grid at the plot's edges and middle, the zero lines at the fractions `zero` across and up, and the labels:
/// `xs` under the plot's left, middle and right, `ys` beside its bottom, middle and top.
fn axes(painter: &Painter, plot: Rect, zero: Vec2, xs: [String; 3], ys: [String; 3]) {
    for (n, (x, y)) in xs.into_iter().zip(ys).enumerate() {
        let f = n as f32 / 2.0;
        let (at_x, at_y) = (plot.left() + f * plot.width(), plot.bottom() - f * plot.height());
        painter.vline(at_x, plot.y_range(), Stroke::new(1.0, if f == zero.x { ZERO } else { GRID }));
        painter.hline(plot.x_range(), at_y, Stroke::new(1.0, if f == zero.y { ZERO } else { GRID }));
        let anchor = [Align2::LEFT_TOP, Align2::CENTER_TOP, Align2::RIGHT_TOP][n];
        painter.text(Pos2::new(at_x, plot.bottom() + 4.0), anchor, x, small(), LABEL);
        painter.text(Pos2::new(plot.left() - 4.0, at_y), Align2::RIGHT_CENTER, y, small(), LABEL);
    }
}

/// The dampers' force over the wheel's speed: bump left of 0 (the slow rate up to `fast_from`, the fast rate past
/// it), rebound right of 0; front and rear (`[front, rear]`), the comparison's dashed.
pub fn dampers(ui: &mut Ui, now: [Damper; 2], base: [Damper; 2], size: Vec2) {
    let positive = |d: Damper| Damper {
        bump: clean(d.bump).max(0.0),
        rebound: clean(d.rebound).max(0.0),
        fast_bump: clean(d.fast_bump).max(0.0),
        fast_from: clean(d.fast_from).max(0.0),
    };
    let (now, base) = (now.map(positive), base.map(positive));
    let Some((painter, plot)) = card(ui, size, &AXLES_LEGEND, 42.0, 18.0) else { return };
    let all = || now.iter().chain(&base);
    let xmax = all().map(|d| 2.0 * d.fast_from).fold(0.5_f32, f32::max);
    // A tenth over the largest force, so no curve runs along the edge.
    let ymax = 1.1 * all().map(|d| force(d, xmax).abs().max(force(d, -xmax).abs())).fold(1.0_f32, f32::max);
    let shown = |f: f32| if ymax >= 1500.0 { format!("{:.1}k", f / 1000.0) } else { format!("{f:.0}") };
    let xs = [format!("-{xmax:.2}"), "0".to_owned(), format!("{xmax:.2} m/s")];
    axes(&painter, plot, Vec2::splat(0.5), xs, [shown(-ymax), "0".to_owned(), shown(ymax)]);
    painter.text(plot.left_top() + Vec2::splat(3.0), Align2::LEFT_TOP, "bump", small(), LABEL);
    painter.text(plot.right_top() + Vec2::new(-3.0, 3.0), Align2::RIGHT_TOP, "rebound", small(), LABEL);

    let curve = |d: &Damper| -> Vec<Pos2> {
        (0..=64)
            .map(|k| {
                let v = xmax * (k as f32 / 32.0 - 1.0);
                plot.center() + Vec2::new(v / xmax * plot.width(), -force(d, v) / ymax * plot.height()) / 2.0
            })
            .collect()
    };
    for d in &base {
        painter.extend(Shape::dashed_line(&curve(d), Stroke::new(1.25, BASE), 5.0, 4.0));
    }
    for (d, colour) in now.iter().zip([FRONT, REAR]) {
        painter.add(Shape::line(curve(d), Stroke::new(2.0, colour)));
    }
}

/// A damper's force at wheel speed `v` (bump below 0, drawn negative; rebound above), as
/// `rbr_physics::spring_damper` gives it: the slow bump rate to `fast_from`, then the fast rate on from the break,
/// the slow rate throughout when `fast_from` is 0.
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

/// A coil's zigzag up from `bottom` over `length` px, `coils` turns `half` px either side of `x`.
fn coil(x: f32, bottom: f32, length: f32, coils: usize, half: f32) -> Vec<Pos2> {
    let steps = (coils * 2).max(2);
    (0..=steps)
        .map(|k| {
            let side = if k == 0 || k == steps {
                0.0
            } else if k % 2 == 1 {
                -half
            } else {
                half
            };
            Pos2::new(x + side, bottom - length * k as f32 / steps as f32)
        })
        .collect()
}

/// The front and rear coil-overs side by side, to one scale: the main spring under the helper spring, each coil's
/// length its free length, the wire's weight its rate; the comparison's outline behind them.
pub fn springs(ui: &mut Ui, now: [Spring; 2], base: [Spring; 2], size: Vec2) {
    let positive = |s: Spring| Spring {
        length: clean(s.length).max(0.0),
        rate: clean(s.rate).max(0.0),
        helper_length: clean(s.helper_length).max(0.0),
        helper_rate: clean(s.helper_rate).max(0.0),
    };
    let (now, base) = (now.map(positive), base.map(positive));
    let Some((painter, plot)) = card(ui, size, &AXLES_LEGEND, 10.0, 36.0) else { return };
    let all = || now.iter().chain(&base);
    let scale = plot.height() / all().map(|s| s.length + s.helper_length).fold(0.1_f32, f32::max);
    let most = all().map(|s| s.rate.max(s.helper_rate)).fold(1.0_f32, f32::max);
    let weight = |rate: f32| 1.0 + 2.5 * rate / most;
    let ground = plot.bottom();
    let half = (plot.width() * 0.09).clamp(10.0, 20.0);
    let plate = |x: f32, y: f32, w: f32, stroke: Stroke| painter.hline(x - w..=x + w, y, stroke);
    for (axle, (current, compared)) in now.iter().zip(&base).enumerate() {
        let colour = [FRONT, REAR][axle];
        let x = plot.left() + plot.width() * (0.25 + 0.5 * axle as f32);
        // The comparison: the outlines of its main spring and of its helper above it.
        if compared != current {
            let main_top = ground - compared.length * scale;
            let helper_top = main_top - compared.helper_length * scale;
            for (top, bottom, w) in [(main_top, ground, half + 4.0), (helper_top, main_top, half * 0.7 + 4.0)] {
                let r = Rect::from_min_max(Pos2::new(x - w, top), Pos2::new(x + w, bottom));
                let outline = [r.left_bottom(), r.left_top(), r.right_top(), r.right_bottom()];
                painter.extend(Shape::dashed_line(&outline, Stroke::new(1.0, BASE), 4.0, 3.0));
            }
        }
        let main_top = ground - current.length * scale;
        let helper_top = main_top - current.helper_length * scale;
        let main = coil(x, ground, current.length * scale, 6, half);
        painter.add(Shape::line(main, Stroke::new(weight(current.rate), colour)));
        let helper = coil(x, main_top, current.helper_length * scale, 3, half * 0.7);
        painter.add(Shape::line(helper, Stroke::new(weight(current.helper_rate), colour.gamma_multiply(0.7))));
        // The perch between the springs, the top mount and the ground.
        plate(x, main_top, half + 2.0, Stroke::new(2.0, LABEL));
        plate(x, helper_top, half * 0.7 + 2.0, Stroke::new(1.5, LABEL));
        plate(x, ground, half + 6.0, Stroke::new(1.0, ZERO));
        let text = |dy: f32, text: String, colour: Color32| {
            painter.text(Pos2::new(x, plot.bottom() + dy), Align2::CENTER_TOP, text, small(), colour);
        };
        let (mm, n_mm) = (|m: f32| m * 1e3, |rate: f32| rate * 1e-3);
        text(4.0, format!("{:.0} mm · {:.0} N/mm", mm(current.length), n_mm(current.rate)), colour);
        let helper = format!("helper {:.0} mm · {:.0} N/mm", mm(current.helper_length), n_mm(current.helper_rate));
        text(18.0, helper, LABEL);
    }
}

/// The four wheels from the front (`[LF, RF, LB, RB]` axis inclinations, radians; the front pair over the rear),
/// each leaning from the comparison's upright outline by what its value differs, exaggerated so a tenth of a degree
/// shows. The value is the axle's tilt on its strut (`rbr_physics::wheel::axes::inclined_axle`), not the camber: the
/// strut leans in itself (the i20's fronts stand at -4.9° with 0 and at -0.9° with its default -4.19°), and a degree
/// more of the value leans the wheel's top a degree further in (0.96-1.16° at rest on the pad; six cars the same way).
pub fn camber(ui: &mut Ui, now: [f32; 4], base: [f32; 4], size: Vec2) {
    /// How many times over a wheel leans, so small changes show.
    const EXAGGERATE: f32 = 4.0;
    const NAMES: [&str; 4] = ["LF", "RF", "LB", "RB"];
    let Some((painter, plot)) = card(ui, size, &AXLES_LEGEND, 8.0, 8.0) else { return };
    let cell = plot.size() / 2.0;
    let (w, h) = ((cell.x * 0.30).clamp(14.0, 30.0), (cell.y - 26.0).max(20.0));
    for (i, name) in NAMES.iter().enumerate() {
        let (row, col) = (i / 2, i % 2);
        let origin = plot.left_top() + Vec2::new(col as f32 * cell.x, row as f32 * cell.y);
        let contact = origin + Vec2::new(cell.x / 2.0, cell.y - 16.0);
        let colour = [FRONT, REAR][row];
        // A higher value leans the wheel's top inward: right on the left wheels, left on the right ones.
        let side = if col == 0 { 1.0 } else { -1.0 };
        let wheel = |lean_in: f32| {
            let rotation = Rot2::from_angle((lean_in * EXAGGERATE * side).clamp(-0.5, 0.5));
            [(-w / 2.0, 0.0), (w / 2.0, 0.0), (w / 2.0, -h), (-w / 2.0, -h)]
                .map(|(x, y)| contact + rotation * Vec2::new(x, y))
                .to_vec()
        };
        painter.hline(contact.x - 22.0..=contact.x + 22.0, contact.y + 3.0, Stroke::new(1.0, ZERO));
        painter.add(Shape::convex_polygon(wheel(0.0), Color32::TRANSPARENT, Stroke::new(1.0, BASE)));
        let lean_in = clean(now[i]) - clean(base[i]);
        painter.add(Shape::convex_polygon(wheel(lean_in), Color32::from_gray(45), Stroke::new(2.0, colour)));
        painter.circle_filled(contact, 2.0, colour);
        let label = format!("{name} {:.2}°", clean(now[i]).to_degrees());
        painter.text(Pos2::new(contact.x, origin.y + cell.y - 11.0), Align2::CENTER_TOP, label, small(), LABEL);
    }
}

/// A curve: its knots' points, the values they hold (0 to 1 up the plot), the comparison's, the steps a drag takes,
/// its colour and a name for the knots' ids.
struct Curve<'a> {
    points: &'a [Pos2],
    values: &'a mut [f32],
    compared: &'a [f32],
    range: Range,
    colour: Color32,
    salt: &'static str,
}

/// The curve's knots, each dragged up and down by its range's steps (`Range::toward`) and filled [`DIFF`] where it
/// differs from the comparison; `tip` describes knot `n` at a value. Whether a value changed.
fn knots(ui: &mut Ui, painter: &Painter, plot: Rect, curve: Curve, tip: impl Fn(usize, f32) -> String) -> bool {
    let range = curve.range;
    let usable = range.max.is_finite() && range.min.is_finite() && range.max > range.min && range.step > 0.0;
    let mut changed = false;
    for (n, &at) in curve.points.iter().enumerate() {
        let value = clean(curve.values[n]);
        let id = ui.scope_id().with((curve.salt, n));
        let response = ui.interact(Rect::from_center_size(at, Vec2::splat(14.0)), id, Sense::drag());
        if let Some(pointer) = response.interact_pointer_pos().filter(|_| usable && response.dragged()) {
            let next = range.toward(value, ((plot.bottom() - pointer.y) / plot.height()).clamp(0.0, 1.0));
            changed |= next != value;
            curve.values[n] = next;
        }
        let fill = if value != clean(curve.compared[n]) { DIFF } else { curve.colour };
        let radius = if usable && (response.hovered() || response.dragged()) { 5.5 } else { 3.5 };
        painter.circle(at, radius, fill, Stroke::new(1.0, Color32::BLACK));
        let response = if usable { response.on_hover_cursor(egui::CursorIcon::ResizeVertical) } else { response };
        response.on_hover_text(tip(n, value));
    }
    changed
}

/// The point of a value (0 to 1) at the fraction `x` across the plot.
fn point(plot: Rect, x: f32, value: f32) -> Pos2 {
    let (x, y) = (x.clamp(0.0, 1.0), clean(value).clamp(0.0, 1.0));
    Pos2::new(plot.left() + x * plot.width(), plot.bottom() - y * plot.height())
}

/// A diff map's lock (0 to 1) over the throttle and the brake pedal (0 to 100 %, a knot each 10 %): two curves, the
/// comparison's dashed. Each knot drags up and down by `lock`'s steps (`Range::toward`); its tooltip gives the lock
/// in % and in Nm of the diff's `torque`. Whether a value changed.
pub fn pedal_locks(ui: &mut Ui, map: &mut DiffMap, base: &DiffMap, lock: Range, torque: f32, size: Vec2) -> bool {
    let legend = [("Throttle", THROTTLE, false), ("Brake", BRAKE, false), ("Compared", BASE, true)];
    let Some((painter, plot)) = card(ui, size, &legend, 42.0, 18.0) else { return false };
    let labels = |last: &str| ["0".to_owned(), "50".to_owned(), last.to_owned()];
    axes(&painter, plot, Vec2::ZERO, labels("100 % pedal"), labels("100 %"));
    let points = |curve: &[f32; MAP_POINTS]| -> Vec<Pos2> {
        (0..MAP_POINTS).map(|n| point(plot, n as f32 / (MAP_POINTS - 1) as f32, curve[n])).collect()
    };
    for compared in [&base.throttle, &base.brake] {
        painter.extend(Shape::dashed_line(&points(compared), Stroke::new(1.25, BASE), 5.0, 4.0));
    }
    let torque = clean(torque);
    let mut changed = false;
    let curves =
        [("throttle", THROTTLE, &mut map.throttle, &base.throttle), ("brake", BRAKE, &mut map.brake, &base.brake)];
    for (salt, colour, values, compared) in curves {
        let line = points(values);
        painter.add(Shape::line(line.clone(), Stroke::new(2.0, colour)));
        let curve = Curve { points: &line, values, compared, range: lock, colour, salt };
        changed |= knots(ui, &painter, plot, curve, |n, v| {
            format!("{salt} {} %: lock {:.0} % ({:.0} Nm)", n * 10, v * 100.0, v * torque)
        });
    }
    changed
}

/// A diff map's speed lock: its factor (0 to 1) at each knot's speed (m/s, shown in km/h), the comparison's dashed.
/// Each knot's factor drags up and down by `factor`'s steps; the speeds are not dragged. Whether a value changed.
pub fn speed_locks(ui: &mut Ui, map: &mut DiffMap, base: &DiffMap, factor: Range, size: Vec2) -> bool {
    let legend = [("Lock factor", THROTTLE, false), ("Compared", BASE, true)];
    let Some((painter, plot)) = card(ui, size, &legend, 42.0, 18.0) else { return false };
    let speed = |v: f32| clean(v).max(0.0);
    let all = || map.speed.iter().chain(&base.speed).map(|&v| speed(v));
    let (lo, hi) = (all().fold(f32::INFINITY, f32::min), all().fold(0.0_f32, f32::max));
    let (lo, hi) = if hi > lo { (lo, hi) } else { (0.0, lo.max(0.0) + 1.0) };
    let kmh = |f: f32| format!("{:.0} km/h", (lo + f * (hi - lo)) * 3.6);
    let percent = ["0".to_owned(), "50".to_owned(), "100 %".to_owned()];
    axes(&painter, plot, Vec2::ZERO, [kmh(0.0), kmh(0.5), kmh(1.0)], percent);
    // In the map's order, even where its speeds do not increase.
    let points = |m: &DiffMap| -> Vec<Pos2> {
        (0..MAP_POINTS).map(|n| point(plot, (speed(m.speed[n]) - lo) / (hi - lo), m.speed_factor[n])).collect()
    };
    painter.extend(Shape::dashed_line(&points(base), Stroke::new(1.25, BASE), 5.0, 4.0));
    let line = points(map);
    painter.add(Shape::line(line.clone(), Stroke::new(2.0, THROTTLE)));
    let speeds = map.speed;
    let curve = Curve {
        points: &line,
        values: &mut map.speed_factor,
        compared: &base.speed_factor,
        range: factor,
        colour: THROTTLE,
        salt: "speed",
    };
    knots(ui, &painter, plot, curve, |n, v| format!("{:.0} km/h: {:.0} %", speed(speeds[n]) * 3.6, v * 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bump force is continuous at the break and grows at the fast rate past it; rebound at its own rate.
    #[test]
    fn a_dampers_force_bends_at_the_fast_bump_speed() {
        let d = Damper { bump: 4000.0, rebound: 6000.0, fast_bump: 1800.0, fast_from: 0.1 };
        assert_eq!(force(&d, 0.5), 3000.0);
        assert_eq!(force(&d, -0.1), -400.0);
        assert!((force(&d, -0.3) - (-400.0 - 1800.0 * 0.2)).abs() < 1e-3);
        assert_eq!(force(&Damper { fast_from: 0.0, ..d }, -0.3), -1200.0);
    }
}
