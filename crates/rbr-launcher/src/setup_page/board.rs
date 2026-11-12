//! The Car setup page's map of the car: the car from above with the parts each area sets (tyres, coil-overs,
//! anti-roll bars, brake discs, differentials, gearbox, steering wheel), and a chip per area beside it with its leading
//! values and how many lines differ from the comparison. The chips and the parts pick the area to edit. The parts
//! show the values (an anti-roll bar's weight its stiffness, a diff's fill its locking torque, the discs' glow the
//! brake balance) and are outlined where they differ from the comparison.

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui};
use rbr_gameplay::setup::{Area, Setup};

use super::charts::{DIFF, FRONT, REAR};
use super::values::{find, Change, Field, WHEEL_AREAS};

/// The board's size.
pub const SIZE: egui::Vec2 = vec2(372.0, 330.0);
const CHIP: egui::Vec2 = vec2(114.0, 46.0);
/// The car's half width and length, its axles' distance from the nose.
const HALF: f32 = 50.0;
const LENGTH: f32 = 306.0;
const AXLES: [f32; 2] = [64.0, 244.0];

/// What the board shows of a setup.
pub struct View {
    /// By [`Area::ALL`]: the chip's leading values and the lines differing from the comparison.
    summaries: [String; 8],
    counts: [usize; 8],
    /// The front and rear anti-roll bars' stiffness, 0 to 1 in their ranges.
    roll_bars: [f32; 2],
    /// The centre, front and rear diffs' locking torque, 0 to 1 of the largest any of them may have; `None` for a
    /// diff the car's options fix at 0 (none fitted).
    diffs: [Option<f32>; 3],
    /// The front brakes' share of the brake pressure.
    brake_front: f32,
    differs: Differs,
}

/// The parts whose values differ from the comparison.
#[derive(Debug, Default, PartialEq)]
pub struct Differs {
    /// By [`WHEEL_AREAS`] and wheel.
    corners: [[bool; 4]; 4],
    bars: [bool; 2],
    brakes: [bool; 2],
    steering: bool,
    /// Centre, front, rear: the torque, the maps and the controls that act on it.
    diffs: [bool; 3],
    gears: bool,
}

impl View {
    /// What the board shows of `setup` (the car's `fields`), whose `lines` and `differs` from the comparison are
    /// given; `gear` its drop gear as shown.
    pub fn of(fields: &[Field], setup: &Setup, lines: &[Change], differs: Differs, gear: Option<String>) -> View {
        let d = setup;
        let field = |key: &str| find(fields, key).map(|i| &fields[i]);
        let pair = |key: &str, front: f32, rear: f32| {
            field(key).map_or_else(String::new, |f| {
                let unit = f.spec.unit;
                let n = |v: f32| format!("{:.*}", unit.decimals, v * unit.scale);
                format!("{} · {} {}", n(front), n(rear), unit.suffix)
            })
        };
        let fraction = |key: &str, v: f32| {
            field(key).filter(|f| !f.fixed()).map_or(0.5, |f| (v - f.range.min) / (f.range.max - f.range.min))
        };
        let share = |front: f32, rear: f32| front / (front + rear);
        let torques = [d.center_diff_torque, d.front_diff_torque, d.rear_diff_torque];
        let fitted = ["CenterDiffMaxTorque", "FrontDiffMaxTorque", "RearDiffMaxTorque"]
            .map(|key| field(key).map(|f| f.range.max).filter(|&max| max > 0.0));
        let most = fitted.iter().flatten().fold(0.0_f32, |a, &b| a.max(b));
        let diffs = std::array::from_fn(|n| fitted[n].map(|_| torques[n] / most));
        let shown: Vec<String> =
            (0..3).filter(|&n| fitted[n].is_some()).map(|n| format!("{:.0}", torques[n])).collect();
        let c = &d.corners;
        let summaries = Area::ALL.map(|area| match area {
            Area::Tyres => pair("Pressure", c[0].tyre_pressure, c[2].tyre_pressure),
            Area::Springs => pair("SpringStiffness", c[0].spring_stiffness, c[2].spring_stiffness),
            Area::Dampers => pair("DampingBump", c[0].bump, c[2].bump),
            Area::Geometry => pair("WheelAxisInclination", c[0].axis_inclination, c[2].axis_inclination),
            Area::RollBars => {
                let bars = share(d.front_roll_bar, d.rear_roll_bar);
                format!("{:.0}° lock · {:.0} % front", d.steering_lock.to_degrees(), 100.0 * bars)
            }
            Area::Brakes => format!("{:.0} % front", 100.0 * share(d.front_brake_pressure, d.rear_brake_pressure)),
            Area::Differentials if shown.is_empty() => "none to set".to_owned(),
            Area::Differentials => format!("{} Nm", shown.join(" · ")),
            Area::Gears => gear.clone().map_or_else(String::new, |g| format!("drop gear {g}")),
        });
        View {
            summaries,
            counts: Area::ALL.map(|area| lines.iter().filter(|l| l.area == area).count()),
            roll_bars: [
                fraction("FrontRollBarStiffness", d.front_roll_bar),
                fraction("RearRollBarStiffness", d.rear_roll_bar),
            ],
            diffs,
            brake_front: share(d.front_brake_pressure, d.rear_brake_pressure),
            differs,
        }
    }
}

/// What a chip's leading values are.
fn legend(area: Area) -> &'static str {
    match area {
        Area::Tyres => "Pressure: front · rear (the left wheels)",
        Area::Springs => "Spring rate: front · rear (the left wheels)",
        Area::Dampers => "Slow bump: front · rear (the left wheels)",
        Area::Geometry => "Camber (axis inclination): front · rear (the left wheels)",
        Area::RollBars => "Steering lock · the front's share of the anti-roll stiffness",
        Area::Brakes => "The front's share of the brake pressure",
        Area::Differentials => "Locking torque of each diff the car has: centre, front, rear",
        Area::Gears => "The drop gear's teeth",
    }
}

impl Differs {
    /// The parts of the values (the car's `fields`) in which `now` differs from `base`.
    pub fn of(fields: &[Field], base: &[f32], now: &[f32]) -> Differs {
        let mut out = Differs::default();
        for spec in fields.iter().enumerate().filter(|(i, _)| base.get(*i) != now.get(*i)).map(|(_, f)| &f.spec) {
            if let (Some(a), Some(w)) = (WHEEL_AREAS.iter().position(|a| *a == spec.area), spec.index) {
                out.corners[a][w] = true;
                continue;
            }
            if let (Area::Differentials, Some(m)) = (spec.area, spec.index) {
                out.diffs[[0, 0, 1, 2][m.min(3)]] = true;
                continue;
            }
            match spec.key.as_str() {
                "FrontRollBarStiffness" => out.bars[0] = true,
                "RearRollBarStiffness" => out.bars[1] = true,
                "MaxSteeringLock" => out.steering = true,
                "MaxBrakePressureFront" => out.brakes[0] = true,
                "MaxBrakePressureRear" => out.brakes[1] = true,
                "CenterDiffMaxTorque" | "CenterDiffHandbrakeRelease" | "LeftFootBrakeThreshold" => out.diffs[0] = true,
                "FrontDiffMaxTorque" => out.diffs[1] = true,
                "RearDiffMaxTorque" => out.diffs[2] = true,
                "DropGearId" => out.gears = true,
                _ => {}
            }
        }
        out
    }

    fn part(&self, part: Part) -> bool {
        let corner = |area: usize, w: usize| self.corners[area][w];
        match part {
            Part::Wheel(w) => corner(0, w) || corner(3, w),
            Part::Coil(w) => corner(1, w),
            Part::Damper(w) => corner(2, w),
            Part::Disc(w) => self.brakes[w / 2],
            Part::Bar(axle) => self.bars[axle],
            Part::Steering => self.steering,
            Part::Diff(d) => self.diffs[d],
            Part::Gearbox => self.gears,
        }
    }
}

/// A part of the car the board draws, and the area that sets it.
#[derive(Clone, Copy)]
enum Part {
    Wheel(usize),
    Disc(usize),
    Coil(usize),
    Damper(usize),
    Bar(usize),
    /// Centre, front, rear.
    Diff(usize),
    Gearbox,
    Steering,
}

impl Part {
    fn area(self) -> Area {
        match self {
            Part::Wheel(_) => Area::Tyres,
            Part::Disc(_) => Area::Brakes,
            Part::Coil(_) => Area::Springs,
            Part::Damper(_) => Area::Dampers,
            Part::Bar(_) | Part::Steering => Area::RollBars,
            Part::Diff(_) => Area::Differentials,
            Part::Gearbox => Area::Gears,
        }
    }
}

/// The parts in the order they are hit (the small ones over the large).
const PARTS: [Part; 23] = [
    Part::Disc(0),
    Part::Disc(1),
    Part::Disc(2),
    Part::Disc(3),
    Part::Diff(0),
    Part::Diff(1),
    Part::Diff(2),
    Part::Gearbox,
    Part::Steering,
    Part::Damper(0),
    Part::Damper(1),
    Part::Damper(2),
    Part::Damper(3),
    Part::Coil(0),
    Part::Coil(1),
    Part::Coil(2),
    Part::Coil(3),
    Part::Bar(0),
    Part::Bar(1),
    Part::Wheel(0),
    Part::Wheel(1),
    Part::Wheel(2),
    Part::Wheel(3),
];

/// The chips (left column top to bottom, then the right), and the part each one points at.
const CHIPS: [(Area, Part); 8] = [
    (Area::Tyres, Part::Wheel(0)),
    (Area::Springs, Part::Coil(0)),
    (Area::Dampers, Part::Damper(2)),
    (Area::Geometry, Part::Wheel(2)),
    (Area::RollBars, Part::Bar(0)),
    (Area::Gears, Part::Gearbox),
    (Area::Differentials, Part::Diff(0)),
    (Area::Brakes, Part::Disc(3)),
];

/// How the chips name the areas: shorter than [`Area::name`].
fn short(area: Area) -> &'static str {
    match area {
        Area::RollBars => "Roll bars",
        _ => area.name(),
    }
}

fn index(area: Area) -> usize {
    Area::ALL.iter().position(|a| *a == area).unwrap_or(0)
}

/// Where the parts are: `at(x, y)` is `x` px right of the car's centre line and `y` px behind its nose.
struct Frame {
    origin: Pos2,
}

impl Frame {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y)
    }

    fn rect(&self, part: Part) -> Rect {
        let around = |x: f32, y: f32, w: f32, h: f32| Rect::from_center_size(self.at(x, y), vec2(w, h));
        let wheel = |w: usize| (if w.is_multiple_of(2) { -1.0 } else { 1.0 }, AXLES[w / 2]);
        match part {
            Part::Wheel(w) => {
                let (side, y) = wheel(w);
                around(side * HALF, y, 18.0, 46.0)
            }
            Part::Disc(w) => {
                let (side, y) = wheel(w);
                around(side * (HALF - 13.0), y, 7.0, 26.0)
            }
            Part::Coil(w) => {
                let (side, y) = wheel(w);
                around(side * 24.0, y - 11.0, 12.0, 18.0)
            }
            Part::Damper(w) => {
                let (side, y) = wheel(w);
                around(side * 24.0, y + 11.0, 12.0, 16.0)
            }
            Part::Bar(axle) => around(0.0, AXLES[axle] + if axle == 0 { 24.0 } else { -24.0 }, 44.0, 10.0),
            Part::Diff(0) => around(0.0, 162.0, 18.0, 18.0),
            Part::Diff(d) => around(0.0, AXLES[d - 1], 18.0, 18.0),
            Part::Gearbox => around(0.0, 108.0, 20.0, 26.0),
            Part::Steering => around(-20.0, 138.0, 20.0, 20.0),
        }
    }
}

/// Draws the board; the area picked by a click on its chip or one of its parts.
pub fn ui(ui: &mut Ui, selected: Area, view: &View) -> Option<Area> {
    let (rect, response) = ui.allocate_exact_size(SIZE, Sense::click());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals().clone();
    let frame = Frame { origin: pos2(rect.center().x, rect.top() + (SIZE.y - LENGTH) / 2.0) };
    let chip_rect = |n: usize| {
        let x = if n < 4 { rect.left() } else { rect.right() - CHIP.x };
        let gap = (SIZE.y - 4.0 * CHIP.y) / 4.0;
        Rect::from_min_size(pos2(x, rect.top() + gap / 2.0 + (n % 4) as f32 * (CHIP.y + gap)), CHIP)
    };

    let hovered = response.hover_pos().and_then(|p| {
        let chip = (0..CHIPS.len()).find(|&n| chip_rect(n).contains(p)).map(|n| CHIPS[n].0);
        chip.or_else(|| PARTS.iter().find(|&&part| frame.rect(part).expand(2.0).contains(p)).map(|p| p.area()))
    });
    // An area's colour while it is picked or under the pointer.
    let lit = |area: Area| {
        if area == selected {
            Some(visuals.selection.stroke.color)
        } else if Some(area) == hovered {
            Some(visuals.widgets.hovered.fg_stroke.color)
        } else {
            None
        }
    };

    // The leaders under everything.
    for (n, (area, part)) in CHIPS.iter().enumerate() {
        let chip = chip_rect(n);
        let from = if n < 4 { chip.right_center() } else { chip.left_center() };
        let to = frame.rect(*part).center();
        let color = lit(*area).unwrap_or(visuals.weak_text_color().gamma_multiply(0.45));
        let knee = pos2(from.x + if n < 4 { 8.0 } else { -8.0 }, from.y);
        painter.add(Shape::line(vec![from, knee, to], Stroke::new(1.0, color)));
        painter.circle_filled(to, 2.5, color);
    }

    car(&painter, &frame, &visuals, view, &lit);

    for (n, (area, _)) in CHIPS.iter().enumerate() {
        chip(&painter, chip_rect(n), *area, &visuals, view, *area == selected, Some(*area) == hovered);
    }
    let picked = response.clicked().then_some(hovered).flatten();
    if let Some(area) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let count = view.counts[index(area)];
        let differs = match count {
            0 => String::new(),
            1 => "\n1 difference from the compared setup".to_owned(),
            n => format!("\n{n} differences from the compared setup"),
        };
        response.on_hover_text(format!("{}\n{}{differs}", area.name(), legend(area)));
    }
    picked
}

fn chip(painter: &egui::Painter, rect: Rect, area: Area, visuals: &egui::Visuals, view: &View, on: bool, hover: bool) {
    let fill = if on {
        visuals.selection.bg_fill
    } else if hover {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.faint_bg_color
    };
    let stroke = if hover && !on { visuals.widgets.hovered.bg_stroke } else { Stroke::NONE };
    painter.rect(rect, CornerRadius::same(6), fill, stroke, StrokeKind::Inside);
    let text = if on { Color32::WHITE } else { visuals.strong_text_color() };
    let left = rect.left_top() + vec2(8.0, 6.0);
    painter.text(left, Align2::LEFT_TOP, short(area), FontId::proportional(13.5), text);
    let weak = if on { Color32::from_gray(215) } else { visuals.weak_text_color() };
    let summary = &view.summaries[index(area)];
    painter.text(left + vec2(0.0, 19.0), Align2::LEFT_TOP, summary, FontId::proportional(11.0), weak);
    let count = view.counts[index(area)];
    if count > 0 {
        let at = rect.right_top() + vec2(-11.0, 11.0);
        painter.circle_filled(at, 8.0, DIFF);
        painter.text(at, Align2::CENTER_CENTER, count.to_string(), FontId::proportional(11.0), Color32::BLACK);
    }
}

/// The car from above, nose up. A part is outlined in its area's colour while lit (`lit`), else in [`DIFF`] where
/// it differs from the comparison.
fn car(
    painter: &egui::Painter,
    frame: &Frame,
    visuals: &egui::Visuals,
    view: &View,
    lit: &dyn Fn(Area) -> Option<Color32>,
) {
    let body = Color32::from_rgb(54, 58, 66);
    let glass = Color32::from_rgb(34, 40, 50);
    let metal = Color32::from_rgb(92, 96, 104);
    let at = |x, y| frame.at(x, y);
    let mark = |part: Part| lit(part.area()).or(view.differs.part(part).then_some(DIFF));
    let outline = |part: Part| mark(part).map_or(Stroke::NONE, |c| Stroke::new(2.0, c));

    // The body: a nose rounder than the tail, the glasshouse.
    let hull = Rect::from_min_max(at(-HALF + 6.0, 0.0), at(HALF - 6.0, LENGTH));
    let corners = CornerRadius { nw: 24, ne: 24, sw: 14, se: 14 };
    painter.rect(hull, corners, body, visuals.widgets.noninteractive.bg_stroke, StrokeKind::Inside);
    let screen = vec![at(-34.0, 116.0), at(34.0, 116.0), at(30.0, 140.0), at(-30.0, 140.0)];
    painter.add(Shape::convex_polygon(screen, glass, Stroke::NONE));
    painter.rect_filled(
        Rect::from_min_max(at(-30.0, 142.0), at(30.0, 216.0)),
        CornerRadius::same(3),
        body.gamma_multiply(1.15),
    );
    let rear = vec![at(-30.0, 218.0), at(30.0, 218.0), at(32.0, 232.0), at(-32.0, 232.0)];
    painter.add(Shape::convex_polygon(rear, glass, Stroke::NONE));

    // The driveline: the prop shaft and the half shafts, then the gearbox and the diffs over them.
    let shaft = Stroke::new(2.0, metal);
    painter.line_segment([frame.rect(Part::Diff(1)).center(), frame.rect(Part::Diff(2)).center()], shaft);
    for y in AXLES {
        painter.line_segment([at(-HALF, y), at(HALF, y)], shaft);
    }
    painter.rect(frame.rect(Part::Gearbox), CornerRadius::same(3), metal, outline(Part::Gearbox), StrokeKind::Outside);
    for d in 0..3 {
        let r = frame.rect(Part::Diff(d));
        let (fill, stroke) = match view.diffs[d] {
            Some(lock) => (lerp(metal, FRONT, lock.clamp(0.0, 1.0)), outline(Part::Diff(d))),
            None => (body, Stroke::new(1.0, metal)),
        };
        painter.circle(r.center(), r.width() / 2.0, fill, stroke);
    }

    // The anti-roll bars, as heavy as they are stiff, linked to the struts.
    for axle in 0..2 {
        let r = frame.rect(Part::Bar(axle));
        let color = mark(Part::Bar(axle)).unwrap_or(Color32::from_gray(150));
        let ends = [r.left_center(), r.right_center()];
        painter.line_segment(ends, Stroke::new(1.5 + 4.5 * view.roll_bars[axle].clamp(0.0, 1.0), color));
        for (w, end) in (axle * 2..axle * 2 + 2).zip(ends) {
            painter.line_segment([end, frame.rect(Part::Coil(w)).center()], Stroke::new(1.5, color));
        }
    }

    // The steering wheel.
    let steer = frame.rect(Part::Steering).center();
    let color = mark(Part::Steering).unwrap_or(Color32::from_gray(150));
    painter.circle_stroke(steer, 8.0, Stroke::new(2.5, color));
    painter.line_segment([steer - vec2(8.0, 0.0), steer + vec2(8.0, 0.0)], Stroke::new(1.5, color));

    // The corners: tyre, disc, coil over damper.
    for w in 0..4 {
        let tyre = frame.rect(Part::Wheel(w));
        let edge = lit(Area::Geometry).or(mark(Part::Wheel(w)));
        let stroke = edge.map_or(Stroke::new(1.0, Color32::from_gray(80)), |c| Stroke::new(2.0, c));
        painter.rect(tyre, CornerRadius::same(4), Color32::from_rgb(28, 28, 30), stroke, StrokeKind::Inside);
        for k in 1..6 {
            let y = tyre.top() + tyre.height() * k as f32 / 6.0;
            let tread = [pos2(tyre.left() + 3.0, y), pos2(tyre.right() - 3.0, y)];
            painter.line_segment(tread, Stroke::new(1.0, Color32::from_gray(48)));
        }
        if let Some(color) = lit(Area::Geometry) {
            // The wheel's plane, for the geometry page.
            let plane = [tyre.center_top() - vec2(0.0, 6.0), tyre.center_bottom() + vec2(0.0, 6.0)];
            painter.line_segment(plane, Stroke::new(1.0, color));
        }

        let share = if w < 2 { view.brake_front } else { 1.0 - view.brake_front };
        let glow = lerp(metal, Color32::from_rgb(225, 60, 70), share.clamp(0.0, 1.0));
        painter.rect(
            frame.rect(Part::Disc(w)),
            CornerRadius::same(2),
            glow,
            outline(Part::Disc(w)),
            StrokeKind::Outside,
        );

        let damper = frame.rect(Part::Damper(w));
        let cylinder = Rect::from_center_size(damper.center(), vec2(6.0, damper.height()));
        painter.rect(cylinder, CornerRadius::same(2), metal, outline(Part::Damper(w)), StrokeKind::Outside);
        let coil = frame.rect(Part::Coil(w));
        let color = mark(Part::Coil(w)).unwrap_or(if w < 2 { FRONT } else { REAR }.gamma_multiply(0.8));
        let turns = 5;
        let points: Vec<Pos2> = (0..=turns * 2)
            .map(|k| {
                let x = if k % 2 == 0 { coil.left() } else { coil.right() };
                pos2(x, coil.top() + coil.height() * k as f32 / (turns * 2) as f32)
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(if mark(Part::Coil(w)).is_some() { 2.0 } else { 1.5 }, color)));
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_page::values::tests::any_car;
    use crate::setup_page::values::values;

    /// A part is marked where its own values differ: one tyre, an axle's bar, the diff a map acts on.
    #[test]
    fn the_parts_that_differ_are_the_changed_values_own() {
        let fields = any_car();
        let mut now = Setup::default();
        let base = values(&now);
        now.corners[3].tyre_pressure = 2.1e5;
        now.rear_roll_bar = 9000.0;
        now.diff_maps[1].brake[2] = 0.3;
        let differs = Differs::of(&fields, &base, &values(&now));
        let wheels: Vec<bool> = (0..4).map(|w| differs.part(Part::Wheel(w))).collect();
        assert_eq!(wheels, [false, false, false, true]);
        assert!(!differs.part(Part::Bar(0)) && differs.part(Part::Bar(1)));
        assert!(differs.part(Part::Diff(0)) && !differs.part(Part::Diff(1)) && !differs.part(Part::Diff(2)));
        assert!(!differs.part(Part::Coil(3)) && !differs.part(Part::Gearbox));
    }
}
