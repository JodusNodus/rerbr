//! The top left clock, with its rows under it, and the stage's progress across the top.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Vec2};

use super::{caption, digits, ALERT, DIM, MARGIN, PANEL, TEXT};
use crate::hud::{Part, Readout};

/// The clock's width, a row's height under it and the gap between them, points.
const WIDTH: f32 = 210.0;
const ROW: f32 = 22.0;
const GAP: f32 = 3.0;

/// The clock ([`Readout::clock`]): its name over the time in large digits, and a row for each note. The top of the
/// space under it.
pub fn clock(painter: &Painter, screen: Rect, r: &Readout) -> f32 {
    if !r.parts.shows(Part::Clock) {
        return screen.min.y + MARGIN;
    }
    let clock = r.clock();
    let time = Rect::from_min_size(screen.min + Vec2::splat(MARGIN), Vec2::new(WIDTH, 46.0));
    fade(painter, time);
    caption(painter, time.min + Vec2::new(8.0, 4.0), Align2::LEFT_TOP, clock.title);
    let at = time.left_bottom() + Vec2::new(8.0, -1.0);
    digits(painter, at, Align2::LEFT_BOTTOM, &clock.time, FontId::proportional(28.0), TEXT);
    let mut at = Pos2::new(time.min.x, time.max.y + GAP);
    for (name, amount) in clock.notes {
        at.y = row(painter, at, name, &[(amount, ALERT)]);
    }
    at.y
}

/// A row under the clock, its top left at `at`: `name` at its left and `texts` from its right edge leftwards, each in
/// its colour, the first's the stripe's too. The top of the space under it.
pub fn row(painter: &Painter, at: Pos2, name: &str, texts: &[(String, Color32)]) -> f32 {
    let row = Rect::from_min_size(at, Vec2::new(WIDTH, ROW));
    super::panel(painter, row, texts.first().map_or(DIM, |text| text.1), 1.0);
    caption(painter, row.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, name);
    let mut right = row.max.x - 8.0;
    for (text, colour) in texts {
        let at = Pos2::new(right, row.center().y);
        right = painter.text(at, Align2::RIGHT_CENTER, text, FontId::monospace(15.0), *colour).min.x - 10.0;
    }
    row.max.y + GAP
}

/// The stage's progress across the top: the distance driven at the left, the stage's length at the right, the
/// line with the start, the splits and the car under it.
pub fn progress(painter: &Painter, screen: Rect, r: &Readout) {
    let s = &r.state;
    if !r.parts.shows(Part::Progress) || s.helicopter {
        return;
    }
    let Some((car, splits)) = &s.progress else { return };
    let line = Rect::from_center_size(
        Pos2::new(screen.center().x, screen.min.y + MARGIN + 10.0),
        Vec2::new(screen.width() * 0.4, 3.0),
    );
    painter.rect_filled(line.expand(1.0), 2.0, Color32::from_black_alpha(120));
    painter.rect_filled(line, 1.5, Color32::from_white_alpha(60));
    let x = line.min.x + line.width() * car.clamp(0.0, 1.0);
    painter.rect_filled(Rect::from_x_y_ranges(line.min.x..=x, line.y_range()), 1.5, Color32::from_white_alpha(150));
    painter.rect_filled(Rect::from_center_size(line.left_center(), Vec2::splat(6.0)), 0.0, TEXT);
    for split in splits {
        let x = line.min.x + line.width() * split;
        painter.line_segment([Pos2::new(x, line.min.y - 5.0), Pos2::new(x, line.max.y + 5.0)], (2.0, TEXT));
    }
    let tip = Pos2::new(x, line.max.y + 3.0);
    let diamond = [tip, tip + Vec2::new(4.0, 4.5), tip + Vec2::new(0.0, 9.0), tip + Vec2::new(-4.0, 4.5)];
    painter.add(egui::Shape::convex_polygon(diamond.to_vec(), TEXT, (1.0, Color32::from_black_alpha(120))));
    if let Some((driven, to_go)) = s.odometer {
        distance(painter, line.left_center() - Vec2::new(12.0, 0.0), Align2::RIGHT_CENTER, driven, r.mph);
        if let Some(to_go) = to_go {
            let length = driven + to_go.max(0.0);
            distance(painter, line.right_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, length, r.mph);
        }
    }
}

/// A distance in km (or miles with `mph`, the speed's unit; ours) to one decimal.
fn distance(painter: &Painter, at: Pos2, align: Align2, metres: f32, mph: bool) {
    let (distance, unit) = if mph { (metres / 1609.344, "mi") } else { (metres / 1000.0, "km") };
    value(painter, at, align, format!("{distance:.1}"), unit);
}

/// `number` with its unit small after it.
pub fn value(painter: &Painter, at: Pos2, align: Align2, number: String, unit: &str) {
    let number = painter.layout_no_wrap(number, FontId::proportional(17.0), TEXT);
    let unit = painter.layout_no_wrap(unit.to_owned(), FontId::proportional(10.0), DIM);
    let left = at.x - align.x().to_factor() * (number.size().x + 3.0 + unit.size().x);
    let number_at = Pos2::new(left, at.y - number.size().y / 2.0);
    let unit_at = Pos2::new(left + number.size().x + 3.0, at.y + number.size().y * 0.25 - unit.size().y);
    // On a dark pill, readable on a bright sky.
    let pill = Rect::from_min_size(number_at, number.size()).union(Rect::from_min_size(unit_at, unit.size()));
    painter.rect_filled(pill.expand2(Vec2::new(5.0, 1.0)), 3.0, Color32::from_black_alpha(110));
    painter.galley(number_at, number, TEXT);
    painter.galley(unit_at, unit, DIM);
}

/// The panel's fill across `rect`, fading out to the right.
fn fade(painter: &Painter, rect: Rect) {
    let mut mesh = egui::Mesh::default();
    for (corner, colour) in [
        (rect.left_top(), PANEL),
        (rect.right_top(), Color32::TRANSPARENT),
        (rect.right_bottom(), Color32::TRANSPARENT),
        (rect.left_bottom(), PANEL),
    ] {
        mesh.colored_vertex(corner, colour);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}
