//! Our HUD's look (decision 2026-10-17: modern and simple, with every feature of the original; the layout after EA
//! WRC): the clock at the top left with a passed split under it and the stage's progress across the top (`top`,
//! `splits`), the view and frame rate at the top right, the pace note, result panels and the race's calls in the
//! middle (`centre`, `splits`, `countdown`), the dial at the bottom right (`dial`), the car's status at the bottom left
//! (`car_status`) and the telemetry the player asked for beside the dial and under the status (`telemetry`). Shapes
//! and the default font, not the minidash or digidash art; the pace notes are the game's sprites (`pace`).

use egui::{Align2, Color32, FontId, Pos2, Rect, Vec2};

use super::layout::Layout;
use super::{Hud, Part, Readout};

mod car_status;
mod centre;
mod countdown;
mod dial;
pub mod pace;
mod splits;
mod telemetry;
mod top;

/// Margin, padding and corner radius, points.
const MARGIN: f32 = 20.0;
const PAD: f32 = 12.0;
const RADIUS: f32 = 4.0;
/// The panels' fill, their accent stripe, the text and the captions.
const PANEL: Color32 = Color32::from_rgba_premultiplied(9, 10, 13, 190);
const ACCENT: Color32 = Color32::from_rgb(255, 196, 0);
const TEXT: Color32 = Color32::from_gray(240);
const DIM: Color32 = Color32::from_gray(150);
/// A lit lamp, the redline and the warnings; the messages and penalties.
const LAMP_ON: Color32 = Color32::from_rgb(235, 60, 50);
const ALERT: Color32 = Color32::from_rgb(240, 160, 60);

/// Our HUD, with the game's pace sprites ([`Stock::new`]; without them the notes are words).
#[derive(Default)]
pub struct Stock {
    sprites: pace::Sprites,
    /// Where each part was last drawn, by its index in [`Part::ALL`]; `None` for one that drew nothing.
    bounds: [Option<Rect>; Part::ALL.len()],
}

impl Stock {
    /// The HUD with `data`'s pace sprites.
    pub fn new(data: &rbr_formats::data::Data) -> Stock {
        Stock { sprites: pace::Sprites::load(data), ..Stock::default() }
    }

    /// Paints `r` with `painter` on `screen`, [`REFERENCE_HEIGHT`](super::REFERENCE_HEIGHT) points high, each part
    /// where its layout puts it ([`Readout::layout`]).
    fn paint_on(&mut self, painter: &egui::Painter, screen: Rect, r: &Readout) {
        self.sprites.upload(painter.ctx());
        self.bounds = [None; Part::ALL.len()];
        let (sprites, layout) = (&self.sprites, &r.layout);
        let mut bounds = [None; Part::ALL.len()];
        let mut part = |part: Part, paint: &mut dyn FnMut()| {
            bounds[part as usize] = placed(painter, screen, layout, part, paint);
        };

        // Top right: the rendered frames per second, when the viewer measures them, and the view. Ours.
        part(Part::View, &mut || {
            let mut at = Pos2::new(screen.max.x - MARGIN, screen.min.y + MARGIN);
            if let Some(fps) = r.fps {
                painter.text(at, Align2::RIGHT_TOP, format!("{fps:.0} fps"), FontId::proportional(12.0), DIM);
                at.y += 16.0;
            }
            if r.parts.shows(Part::View) {
                let keys = if r.view_keys.is_empty() { String::new() } else { format!(" · {}", r.view_keys) };
                painter.text(at, Align2::RIGHT_TOP, format!("{} view{keys}", r.view), FontId::proportional(12.0), DIM);
            }
        });

        let school = r.state.school.is_some();
        let page = r.splits.as_ref().filter(|record| splits::has_page(record, school));
        part(Part::Clock, &mut || {
            let under_clock = top::clock(painter, screen, r);
            if let Some(record) = r.splits.as_ref().filter(|_| page.is_none() && r.parts.shows(Part::Splits)) {
                splits::passed(painter, Pos2::new(screen.min.x + MARGIN, under_clock), record);
            }
        });
        if let Some(record) = page {
            splits::page(painter, screen, record, r.state.end_page);
        }
        // `HUD::Update` draws nothing while the rescue helicopter flies; the caption is ours.
        if r.state.helicopter && r.state.end_page.is_none() {
            let at = Pos2::new(screen.center().x, screen.min.y + MARGIN);
            painter.text(at, Align2::CENTER_TOP, "rescue helicopter called", FontId::proportional(15.0), DIM);
            self.bounds = bounds;
            return;
        }
        part(Part::Progress, &mut || top::progress(painter, screen, r));
        part(Part::Pace, &mut || centre::note(painter, screen, r, sprites));
        part(Part::Messages, &mut || centre::message(painter, screen, r));
        centre::calls(painter, screen, r, sprites);
        // The telemetry's cards stand beside the dial and the car's status over the trace before the layout moves
        // them, whichever of them show.
        let mut right = screen.max.x;
        part(Part::Dash, &mut || right = dial::paint(painter, screen, r));
        let dial = right;
        part(Part::Tyres, &mut || right = telemetry::tyres(painter, screen, dial, r));
        part(Part::Gforce, &mut || telemetry::g_force(painter, screen, right, r));
        let mut bottom = screen.max.y;
        part(Part::Trace, &mut || bottom = telemetry::trace(painter, screen, r));
        part(Part::Car, &mut || {
            if r.parts.shows(Part::Car) {
                let screen = Rect::from_min_max(screen.min, Pos2::new(screen.max.x, bottom));
                car_status::paint(painter, screen, &r.state);
            }
        });
        self.bounds = bounds;
    }
}

impl Hud for Stock {
    fn paint(&mut self, ctx: &egui::Context, r: &Readout) {
        self.paint_on(&ctx.layer_painter(egui::LayerId::background()), ctx.content_rect(), r);
    }

    fn bounds(&self, part: Part) -> Option<Rect> {
        self.bounds[part as usize]
    }
}

/// Paints a part and puts what it drew where `layout` has it ([`Layout::place`]); where that is, `None` when it drew
/// nothing.
fn placed(painter: &egui::Painter, screen: Rect, layout: &Layout, part: Part, paint: &mut dyn FnMut()) -> Option<Rect> {
    let start = painter.add(egui::Shape::Noop);
    paint();
    let end = painter.add(egui::Shape::Noop);
    painter.ctx().graphics_mut(|graphics| {
        let list = graphics.entry(painter.layer_id());
        let drawn = list.all_entries().take(end.0).skip(start.0 + 1);
        let bounds = drawn.map(|s| s.shape.visual_bounding_rect()).filter(Rect::is_positive).reduce(Rect::union)?;
        let place = layout.place(part, bounds, screen);
        if place != egui::emath::TSTransform::IDENTITY {
            list.transform_range(start, end, place);
        }
        Some(place * bounds)
    })
}

/// A card: the panel's fill at `alpha` with a stripe of `accent` down its left edge.
fn panel(painter: &egui::Painter, rect: Rect, accent: Color32, alpha: f32) {
    painter.rect_filled(rect, RADIUS, PANEL.gamma_multiply(alpha));
    let corners = egui::CornerRadius { nw: RADIUS as u8, sw: RADIUS as u8, ne: 0, se: 0 };
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
        corners,
        accent.gamma_multiply(alpha),
    );
}

/// A small upper-case label.
fn caption(painter: &egui::Painter, at: Pos2, align: Align2, text: &str) {
    painter.text(at, align, text.to_uppercase(), FontId::proportional(11.0), DIM);
}

/// `text` aligned at `at`, its digits in cells of one width so that it does not jitter as they change, and drawn
/// four times a fraction of a point apart for a heavier face than the default font's (it has no bold).
fn digits(painter: &egui::Painter, at: Pos2, align: Align2, text: &str, font: FontId, colour: Color32) {
    let cell = painter.layout_no_wrap("0".to_owned(), font.clone(), colour).size().x;
    let glyphs: Vec<_> = text
        .chars()
        .map(|c| {
            let galley = painter.layout_no_wrap(c.to_string(), font.clone(), colour);
            let width = if c.is_ascii_digit() { cell } else { galley.size().x };
            (galley, width)
        })
        .collect();
    let width: f32 = glyphs.iter().map(|(_, w)| w).sum();
    let height = glyphs.first().map_or(0.0, |(g, _)| g.size().y);
    let mut x = at.x - align.x().to_factor() * width;
    let y = at.y - align.y().to_factor() * height;
    for (galley, w) in glyphs {
        let pos = Pos2::new(x + (w - galley.size().x) / 2.0, y);
        for dx in [0.0, 0.4, 0.8, 1.2] {
            painter.galley(pos + Vec2::new(dx, 0.0), galley.clone(), colour);
        }
        x += w;
    }
}
