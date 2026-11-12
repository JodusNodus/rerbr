//! The telemetry cards along the bottom, each only when its part is shown (`crate::hud::telemetry`), clear of the
//! chase view's car in the middle: from the dial leftwards the tyres' temperatures as the car from above and the
//! g-force as a dot in a 2 g circle (it goes where the car accelerates: up on the throttle, down on the brakes, to the
//! side it turns to), and at the bottom left the trace of the pedals and the steering, the newest at the right. Ours.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use super::dial::{BRAKE, CLUTCH, HANDBRAKE, THROTTLE};
use super::{caption, DIM, MARGIN, PANEL, RADIUS, TEXT};
use crate::hud::telemetry::{tyre_view, TracePoint, TRACE_SECONDS};
use crate::hud::{Part, Readout};

/// The cards' height and the gap between them, points.
const HEIGHT: f32 = 96.0;
const GAP: f32 = 8.0;
/// The space under a card's caption.
const TOP: f32 = 20.0;
const TRACE_WIDTH: f32 = 300.0;
/// The g-force at the circle's rim.
const G_RIM: f32 = 2.0;
/// A tyre at its best temperature, too cold and too hot.
const BEST: Color32 = Color32::from_rgb(60, 170, 90);
const COLD: Color32 = Color32::from_rgb(60, 120, 220);
const HOT: Color32 = Color32::from_rgb(215, 55, 45);

/// Paints the shown cards, the right ones leftwards from `right`, the left edge of what stands at the bottom right.
/// The bottom of what is left of the screen's left side.
pub fn paint(painter: &Painter, screen: Rect, right: f32, r: &Readout) -> f32 {
    let mut right = right.min(screen.max.x - MARGIN + GAP) - GAP;
    let (top, bottom) = (screen.max.y - MARGIN - HEIGHT, screen.max.y - MARGIN);
    let card = |rect: Rect, name: &str| {
        painter.rect_filled(rect, RADIUS, PANEL);
        caption(painter, rect.min + Vec2::new(8.0, 6.0), Align2::LEFT_TOP, name);
        Rect::from_min_max(rect.min + Vec2::new(8.0, TOP), rect.max - Vec2::splat(8.0))
    };
    if let Some(tyres) = r.state.tyres.filter(|_| r.parts.shows(Part::Tyres)) {
        let inner = card(Rect::from_min_max(Pos2::new(right - 92.0, top), Pos2::new(right, bottom)), "tyres °C");
        right -= 92.0 + GAP;
        let size = (inner.size() - Vec2::splat(4.0)) / 2.0;
        for (w, tyre) in tyres.iter().enumerate() {
            let at = inner.min + Vec2::new((w % 2) as f32, (w / 2) as f32) * (size + Vec2::splat(4.0));
            let tile = Rect::from_min_size(at, size);
            let Some(view) = tyre_view(tyre) else {
                painter.rect_filled(tile, 3.0, Color32::from_white_alpha(20));
                continue;
            };
            let to = if view.heat < 0.0 { COLD } else { HOT };
            painter.rect_filled(tile, 3.0, BEST.lerp_to_gamma(to, view.heat.abs()));
            let text = format!("{:.0}", view.celsius);
            painter.text(tile.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), TEXT);
        }
    }
    if let Some([side, forward]) = r.g_force.filter(|_| r.parts.shows(Part::Gforce)) {
        let inner = card(Rect::from_min_max(Pos2::new(right - HEIGHT, top), Pos2::new(right, bottom)), "g-force");
        let (centre, radius) = (inner.center(), inner.height() / 2.0);
        let ring = Stroke::new(1.0, Color32::from_white_alpha(45));
        painter.circle_stroke(centre, radius, ring);
        painter.circle_stroke(centre, radius / G_RIM, ring);
        painter.line_segment([centre - Vec2::new(radius, 0.0), centre + Vec2::new(radius, 0.0)], ring);
        painter.line_segment([centre - Vec2::new(0.0, radius), centre + Vec2::new(0.0, radius)], ring);
        let g = Vec2::new(side, -forward);
        let dot = centre + g / g.length().max(G_RIM) * radius;
        painter.circle_filled(dot, 4.0, TEXT);
        let at = Pos2::new(inner.max.x, inner.min.y - 14.0);
        painter.text(at, Align2::RIGHT_TOP, format!("{:.1}", g.length()), FontId::proportional(11.0), TEXT);
    }
    if !r.parts.shows(Part::Trace) || r.trace.len() < 2 {
        return screen.max.y;
    }
    let left = Pos2::new(screen.min.x + MARGIN, top);
    trace(painter, card(Rect::from_min_size(left, Vec2::new(TRACE_WIDTH, HEIGHT)), "inputs"), &r.trace);
    top + MARGIN - GAP
}

/// The trace in `rect`: the pedals from its bottom to its top, the steering about its middle.
fn trace(painter: &Painter, rect: Rect, points: &[TracePoint]) {
    painter.line_segment([rect.left_center(), rect.right_center()], (1.0, Color32::from_white_alpha(30)));
    let line = |lane: usize, colour: Color32, width: f32| {
        let at = |p: &TracePoint| {
            let x = rect.max.x - rect.width() * p.age / TRACE_SECONDS as f32;
            let value = p.control[lane];
            let y = if lane == 4 { 0.5 - 0.5 * value.clamp(-1.0, 1.0) } else { 1.0 - value.clamp(0.0, 1.0) };
            Pos2::new(x, rect.min.y + rect.height() * y)
        };
        painter.add(egui::Shape::line(points.iter().map(at).collect(), (width, colour)));
    };
    line(4, DIM, 1.0);
    line(3, CLUTCH, 1.5);
    line(2, HANDBRAKE, 1.5);
    line(1, BRAKE, 1.5);
    line(0, THROTTLE, 1.5);
}
