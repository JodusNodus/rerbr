//! The telemetry cards along the bottom, each only when its part is shown (`crate::hud::telemetry`), clear of the
//! chase view's car in the middle: beside the dial the tyres' temperatures, with the tread worn under each, as the car
//! from above, and at the bottom left the trace of the pedals and the steering, the newest at the right. Ours.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Vec2};

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
const TYRES_WIDTH: f32 = 92.0;
/// A tyre at its best temperature, too cold and too hot.
const BEST: Color32 = Color32::from_rgb(60, 170, 90);
const COLD: Color32 = Color32::from_rgb(60, 120, 220);
const HOT: Color32 = Color32::from_rgb(215, 55, 45);

/// A card of the bottom row, its right edge at `right`: its fill and `name`; the space inside it.
fn card(painter: &Painter, screen: Rect, right: f32, width: f32, name: &str) -> Rect {
    let bottom = screen.max.y - MARGIN;
    let rect = Rect::from_min_max(Pos2::new(right - width, bottom - HEIGHT), Pos2::new(right, bottom));
    painter.rect_filled(rect, RADIUS, PANEL);
    caption(painter, rect.min + Vec2::new(8.0, 6.0), Align2::LEFT_TOP, name);
    Rect::from_min_max(rect.min + Vec2::new(8.0, TOP), rect.max - Vec2::splat(8.0))
}

/// The tyres' card ([`Part::Tyres`]) left of `dial`, the left edge of what stands at the bottom right.
pub fn tyres(painter: &Painter, screen: Rect, dial: f32, r: &Readout) {
    let Some(tyres) = r.state.tyres.filter(|_| r.parts.shows(Part::Tyres)) else { return };
    let right = dial.min(screen.max.x - MARGIN + GAP) - GAP;
    let inner = card(painter, screen, right, TYRES_WIDTH, "tyres °C");
    let small = Color32::from_white_alpha(190);
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
        let (at, text) = (tile.center() - Vec2::new(0.0, 5.0), format!("{:.0}", view.celsius));
        painter.text(at, Align2::CENTER_CENTER, text, FontId::proportional(14.0), TEXT);
        let (at, text) = (tile.center() + Vec2::new(0.0, 9.0), format!("{:.1}%", view.worn));
        painter.text(at, Align2::CENTER_CENTER, text, FontId::proportional(9.0), small);
    }
}

/// The trace's card at the bottom left ([`Part::Trace`]); the bottom of what is left of the screen's left side.
pub fn trace(painter: &Painter, screen: Rect, r: &Readout) -> f32 {
    if !r.parts.shows(Part::Trace) || r.trace.len() < 2 {
        return screen.max.y;
    }
    let inner = card(painter, screen, screen.min.x + MARGIN + TRACE_WIDTH, TRACE_WIDTH, "inputs");
    lines(painter, inner, &r.trace);
    inner.min.y - TOP - GAP + MARGIN
}

/// The trace in `rect`: the pedals from its bottom to its top, the steering about its middle.
fn lines(painter: &Painter, rect: Rect, points: &[TracePoint]) {
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
