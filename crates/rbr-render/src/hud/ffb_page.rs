//! The pause menu's Force feedback page, ours (`crate::hud::pause`): a panel at the bottom of the screen over the
//! paused view (`crate::ffb::Session`). Its rows: this car's NGP sensitivity on the drive's surface, the wheel's
//! strength, smoothing and linearity (every car's), "Use suggested" when there is a suggestion other than the
//! sensitivity, Reset ("Use the global sensitivity": the car's own dropped) and Done; a line under them says what the
//! selected one does. Under them a graph of the wheel's force (NGP's curve, as linear as shown: `crate::ffb::shape`)
//! at the sensitivity shown over the player's steering load with this car on this surface (`crate::ffb::load`), the
//! region where the wheel maxes out shaded, and one line of advice (`load::advice`). Up and Down pick a row, Left and
//! Right change the value by [`STEP`] (the smoothing by [`SMOOTHING_STEP`], the linearity by [`LINEARITY_STEP`]); the
//! select acts on Use suggested, Reset and Done, and the back returns to the menu's first page. A change takes effect
//! on the paused drive (`crate::ffb::reshape`, `Wheel::set_strength`, `Wheel::set_smoothing`,
//! `Wheel::set_linearity`) and is kept; nothing changes without the player.

use super::camera_page::Moved;
use super::pause::Nav;
use crate::ffb::load::{self, Advice, Load};
use crate::ffb::{Session, LINEARITY_RANGE, SENSITIVITY_RANGE, SMOOTHING_RANGE, SURFACES};
use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};
use std::ops::RangeInclusive;

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 220);
const TEXT: Color32 = Color32::from_gray(235);
const HINT: Color32 = Color32::from_gray(160);
const SELECTED: Color32 = Color32::from_rgb(200, 60, 40);
const BARS: Color32 = Color32::from_rgba_premultiplied(90, 110, 140, 200);
const MAXED: Color32 = Color32::from_rgba_premultiplied(70, 16, 12, 110);
const WIDTH: f32 = 460.0;
const ROW: f32 = 26.0;
/// The graph's height, pixels.
const GRAPH: f32 = 110.0;

/// One step of the sensitivity and the strength, percent.
pub const STEP: i32 = 5;
/// One step of the smoothing, ms.
pub const SMOOTHING_STEP: i32 = 2;
/// One step of the linearity, percent.
pub const LINEARITY_STEP: i32 = 10;
/// The strength's range, percent.
const STRENGTH: RangeInclusive<i32> = 0..=100;

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Sensitivity,
    Strength,
    Smoothing,
    /// How linear the force is in the steering torque.
    Linearity,
    /// Sets the sensitivity to the suggestion.
    UseSuggested,
    Reset,
    Done,
}

/// The page: the drive's surface (0 gravel, 1 tarmac, 2 snow), the car's sensitivity on it, whether the car has its
/// own, the one every car takes, the strength, the player's load with the car on the surface, and the selected row.
#[derive(Debug, Clone, PartialEq)]
pub struct FfbPage {
    pub surface: usize,
    pub value: i32,
    pub own: bool,
    global: i32,
    /// The wheel's strength, percent.
    pub strength: i32,
    /// The smoothing, ms (`crate::ffb::smooth`).
    pub smoothing: i32,
    /// How linear the force is, percent (`crate::ffb::shape`).
    pub linearity: i32,
    load: Load,
    row: Row,
}

impl FfbPage {
    /// A page at full strength with no smoothing, no linearity and no load tracked.
    pub fn new(surface: usize, value: i32, own: bool, global: i32) -> FfbPage {
        let (load, row) = (Load::default(), Row::Sensitivity);
        FfbPage { surface, value, own, global, strength: 100, smoothing: 0, linearity: 0, load, row }
    }

    /// The page for the drive's car on `surface` (FFB1's: 0 gravel, 1 tarmac, 2 snow).
    pub fn of(session: &Session, surface: u32) -> FfbPage {
        let surface = surface.min(2) as usize;
        let page = FfbPage::new(surface, session.now()[surface], session.own(), session.global[surface]);
        let strength = (session.strength * 100.0).round() as i32;
        let (smoothing, linearity) = (session.smoothing, session.linearity);
        FfbPage { strength, smoothing, linearity, load: session.load_on(surface), ..page }
    }

    /// The selected row.
    pub fn row(&self) -> Row {
        self.row
    }

    /// The suggestion, when there is driving enough to give one.
    fn suggested(&self) -> Option<i32> {
        load::suggested(&self.load, self.linearity, SENSITIVITY_RANGE)
            .filter(|_| self.load.seconds() >= load::ADVICE_SECONDS)
    }

    /// The rows shown: Use suggested only with a suggestion other than the sensitivity.
    fn rows(&self) -> Vec<Row> {
        let suggest = self.suggested().is_some_and(|p| p != self.value);
        [Row::Sensitivity, Row::UseSuggested, Row::Strength, Row::Smoothing, Row::Linearity, Row::Reset, Row::Done]
            .into_iter()
            .filter(|&r| r != Row::UseSuggested || suggest)
            .collect()
    }

    /// Takes one move.
    pub fn nav(&mut self, nav: Nav) -> Option<Moved> {
        let rows = self.rows();
        let (n, at) = (rows.len(), rows.iter().position(|&r| r == self.row).unwrap_or(0));
        let by = if nav == Nav::Right { STEP } else { -STEP };
        match (nav, self.row) {
            (Nav::Up, _) => self.row = rows[(at + n - 1) % n],
            (Nav::Down, _) => self.row = rows[(at + 1) % n],
            (Nav::Back, _) | (Nav::Select, Row::Done) => return Some(Moved::Left),
            (Nav::Select, Row::Reset) => {
                let changed = self.own;
                (self.value, self.own) = (self.global, false);
                return changed.then_some(Moved::Changed);
            }
            (Nav::Select, Row::UseSuggested) => {
                let to = self.suggested()?;
                (self.value, self.own, self.row) = (to, true, Row::Sensitivity);
                return Some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Sensitivity) => {
                let value = (self.value + by).clamp(*SENSITIVITY_RANGE.start(), *SENSITIVITY_RANGE.end());
                if value == self.value {
                    return None;
                }
                (self.value, self.own) = (value, true);
                return Some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Strength) => {
                let value = (self.strength + by).clamp(*STRENGTH.start(), *STRENGTH.end());
                if value == self.strength {
                    return None;
                }
                self.strength = value;
                return Some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Smoothing) => {
                let value = (self.smoothing + by.signum() * SMOOTHING_STEP)
                    .clamp(*SMOOTHING_RANGE.start(), *SMOOTHING_RANGE.end());
                if value == self.smoothing {
                    return None;
                }
                self.smoothing = value;
                return Some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Linearity) => {
                let value = (self.linearity + by.signum() * LINEARITY_STEP)
                    .clamp(*LINEARITY_RANGE.start(), *LINEARITY_RANGE.end());
                if value == self.linearity {
                    return None;
                }
                self.linearity = value;
                return Some(Moved::Changed);
            }
            _ => {}
        }
        None
    }
}

fn surface(page: &FfbPage) -> &'static str {
    SURFACES[page.surface.min(2)]
}

fn label(page: &FfbPage, row: Row) -> String {
    match row {
        Row::Sensitivity => format!("Sensitivity, {}", surface(page)),
        Row::Strength => "Strength".into(),
        Row::Smoothing => "Smoothing".into(),
        Row::Linearity => "Linearity".into(),
        Row::UseSuggested => match page.suggested().unwrap_or(page.value) {
            p if p >= *SENSITIVITY_RANGE.end() => format!("Use suggested ({p} %, the most)"),
            p => format!("Use suggested ({p} %)"),
        },
        Row::Reset => "Use the global sensitivity".into(),
        Row::Done => "Done".into(),
    }
}

fn value(page: &FfbPage, row: Row) -> String {
    match row {
        Row::Sensitivity => format!("{} %{}", page.value, if page.own { "" } else { " (global)" }),
        Row::Strength => format!("{} %", page.strength),
        Row::Smoothing if page.smoothing == 0 => "off".into(),
        Row::Smoothing => format!("{} ms", page.smoothing),
        Row::Linearity if page.linearity == 0 => "off".into(),
        Row::Linearity => format!("{} %", page.linearity),
        Row::UseSuggested | Row::Reset | Row::Done => String::new(),
    }
}

/// What the selected row does, in one line.
fn help(page: &FfbPage) -> String {
    match page.row {
        Row::Sensitivity => "How much of this car's steering force fits before the wheel maxes out. Higher: lighter, \
                             more detail. Lower: heavier, maxes out sooner."
            .into(),
        Row::Strength => "How heavy the wheel feels, for every car.".into(),
        Row::Smoothing => SMOOTHING_HELP.into(),
        Row::Linearity => LINEARITY_HELP.into(),
        Row::UseSuggested => "Sets Sensitivity to the suggestion. You can change it again.".into(),
        Row::Reset => "Drops this car's own sensitivity: it takes the global one.".into(),
        Row::Done => "Back to the menu.".into(),
    }
}

/// What the smoothing does, here and on the launcher's Controls page.
pub const SMOOTHING_HELP: &str = "Takes the rattle out of the wheel, for every car. Higher: quieter, and the force \
                                  follows the car that many ms later. Off: NGP's force as it is.";

/// What the linearity does, here and on the launcher's Controls page.
pub const LINEARITY_HELP: &str = "How directly the force follows the steering, for every car. Higher: the wheel goes \
                                  light more clearly when the front tyres let go, but is lighter on small loads and \
                                  reaches full force sooner. Off: NGP's force as it is.";

/// The advice at the sensitivity shown (`load::advice`).
fn status(page: &FfbPage) -> String {
    let percent = |share: f32| (share * 100.0).round() as i32;
    match load::advice(&page.load, page.value, page.linearity, SENSITIVITY_RANGE) {
        Advice::DriveMore(seconds) => format!(
            "Drive a bit more on {} to see your steering load ({}/{} s)",
            surface(page),
            seconds.floor() as i32,
            load::ADVICE_SECONDS as i32
        ),
        Advice::Raise(maxed, to) => format!(
            "The wheel maxes out often ({} % of your driving): raise Sensitivity to about {to} %",
            percent(maxed)
        ),
        Advice::High(maxed, to) => format!(
            "Your usual cornering sits high in the wheel's range (maxed out {} %): raise Sensitivity to about {to} %",
            percent(maxed)
        ),
        Advice::Lower(to) => {
            format!(
                "Your cornering uses little of the wheel's range: lower Sensitivity to about {to} %, or raise Strength"
            )
        }
        Advice::Good(maxed) => format!("Good: your usual cornering sits mid-range (maxed out {} %)", percent(maxed)),
    }
}

/// The page's guide at the bottom.
const GUIDE: &str = "Set Strength once for your wheel. Then for each car, adjust Sensitivity until the wheel rarely \
                     maxes out.";

/// The graph's load range: past where the wheel maxes out at the sensitivity shown, and past the load's last bin.
fn x_range(page: &FfbPage) -> f32 {
    let maxed = load::maxed_from(page.value, page.linearity);
    let last = page.load.tenths.iter().rposition(|t| *t > 0.0).map_or(0.0, |k| (k + 1) as f32 * load::BIN);
    (maxed * 1.4).max(last * 1.05).clamp(20.0, load::BINS as f32 * load::BIN)
}

/// Draws the graph into `rect`: the load's bars, the maxed-out region and the wheel's force (as linear as shown) at
/// the sensitivity shown; its legend under it.
fn graph(painter: &egui::Painter, rect: Rect, page: &FfbPage) {
    let x_max = x_range(page);
    let at =
        |x: f32, f: f32| Pos2::new(rect.min.x + rect.width() * (x / x_max).min(1.0), rect.max.y - rect.height() * f);
    let small = FontId::proportional(11.0);
    let maxed = load::maxed_from(page.value, page.linearity);
    if maxed < x_max {
        painter.rect_filled(Rect::from_min_max(at(maxed, 1.0), rect.max), 0.0, MAXED);
        painter.text(at(maxed, 1.0) + Vec2::new(4.0, 14.0), Align2::LEFT_TOP, "maxed out", small.clone(), TEXT);
    }
    // Square-root heights: the cornering's tail shows next to the straights' near-zero load.
    let top = page.load.tenths.iter().copied().fold(0.0, f32::max).sqrt();
    for (k, t) in page.load.tenths.iter().enumerate().filter(|(_, t)| **t > 0.0) {
        let x0 = k as f32 * load::BIN;
        if x0 >= x_max {
            break;
        }
        let bar = Rect::from_min_max(at(x0, 0.85 * t.sqrt() / top), at(x0 + load::BIN, 0.0));
        painter.rect_filled(bar.shrink2(Vec2::new(0.5, 0.0)), 0.0, BARS);
    }
    let curve: Vec<Pos2> =
        (0..=80).map(|k| k as f32 * x_max / 80.0).map(|x| at(x, load::shaped(x, page.value, page.linearity))).collect();
    painter.add(Shape::line(curve, Stroke::new(2.0, TEXT)));
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, HINT));
    painter.line_segment([rect.left_bottom(), rect.left_top()], Stroke::new(1.0, HINT));
    painter.text(rect.left_top() + Vec2::new(4.0, 0.0), Align2::LEFT_TOP, "wheel force", small.clone(), HINT);
    painter.text(rect.right_bottom() + Vec2::new(0.0, 2.0), Align2::RIGHT_TOP, "steering load", small.clone(), HINT);
    // The legend: the bars and the line.
    let mut x = rect.min.x;
    let y = rect.max.y + 22.0;
    for (bars, text) in [(true, "bars: your driving"), (false, "line: wheel force")] {
        if bars {
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y - 4.0), Vec2::new(10.0, 8.0)), 0.0, BARS);
        } else {
            painter.line_segment([Pos2::new(x, y), Pos2::new(x + 10.0, y)], Stroke::new(2.0, TEXT));
        }
        let r = painter.text(Pos2::new(x + 14.0, y), Align2::LEFT_CENTER, text, small.clone(), HINT);
        x = r.max.x + 14.0;
    }
}

/// Lays `text` out wrapped to the panel's inner width.
fn wrapped(painter: &egui::Painter, text: String, size: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    painter.layout(text, FontId::proportional(size), color, WIDTH - 40.0)
}

/// Draws the page at the bottom of the screen, over the undimmed view.
pub fn paint(ctx: &egui::Context, page: &FfbPage) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("ffb page")));
    let screen = ctx.content_rect();
    let rows = page.rows();
    let help = wrapped(&painter, help(page), 13.0, HINT);
    let status = wrapped(&painter, status(page), 14.0, TEXT);
    let guide = wrapped(&painter, GUIDE.into(), 12.0, HINT);
    let nav = "Up/Down choose    Left/Right change    Enter select    Esc back";
    let height = 38.0
        + ROW * rows.len() as f32
        + help.size().y
        + 10.0
        + GRAPH
        + 36.0
        + status.size().y
        + 10.0
        + guide.size().y
        + 30.0;
    let size = Vec2::new(WIDTH, height);
    let panel = Rect::from_min_size(Pos2::new(screen.center().x - WIDTH / 2.0, screen.max.y - size.y - 16.0), size);
    painter.rect_filled(panel, 8.0, PANEL);
    let mut at = Pos2::new(panel.min.x + 20.0, panel.min.y + 10.0);
    let title = "Force feedback";
    painter.text(Pos2::new(panel.center().x, at.y), Align2::CENTER_TOP, title, FontId::proportional(18.0), TEXT);
    at.y += 28.0;
    let font = FontId::proportional(16.0);
    for row in rows {
        let line = Rect::from_min_size(at, Vec2::new(WIDTH - 40.0, ROW - 4.0));
        let selected = row == page.row();
        if selected {
            painter.rect_filled(line, 4.0, SELECTED);
        }
        let left = line.left_center() + Vec2::new(8.0, 0.0);
        painter.text(left, Align2::LEFT_CENTER, label(page, row), font.clone(), TEXT);
        let value = value(page, row);
        if !value.is_empty() {
            let value = if selected { format!("<  {value}  >") } else { value };
            painter.text(line.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, value, font.clone(), TEXT);
        }
        at.y += ROW;
    }
    let help_height = help.size().y;
    painter.galley(at, help, HINT);
    at.y += help_height + 10.0;
    graph(&painter, Rect::from_min_size(at, Vec2::new(WIDTH - 40.0, GRAPH)), page);
    at.y += GRAPH + 36.0;
    let status_height = status.size().y;
    painter.galley(at, status, TEXT);
    at.y += status_height + 10.0;
    painter.galley(at, guide, HINT);
    let bottom = Pos2::new(panel.center().x, panel.max.y - 8.0);
    painter.text(bottom, Align2::CENTER_BOTTOM, nav, FontId::proportional(12.0), HINT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffb::Options;

    #[test]
    fn the_sensitivity_steps_within_its_range_and_resets() {
        let mut p = FfbPage::new(0, 280, false, 280);
        assert_eq!(
            (label(&p, Row::Sensitivity), value(&p, Row::Sensitivity)),
            ("Sensitivity, gravel".into(), "280 % (global)".into())
        );
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!((p.value, p.own), (285, true));
        for _ in 0..100 {
            p.nav(Nav::Right);
        }
        assert_eq!(p.value, *SENSITIVITY_RANGE.end());
        assert_eq!(p.nav(Nav::Right), None);
        // Reset: every car's again, once.
        for _ in 0..4 {
            p.nav(Nav::Down);
        }
        assert_eq!(p.nav(Nav::Select), Some(Moved::Changed));
        assert_eq!((p.value, p.own), (280, false));
        assert_eq!(p.nav(Nav::Select), None);
        // Done and the back leave; Up from the top wraps to Done.
        assert_eq!(p.nav(Nav::Down), None);
        assert_eq!(p.nav(Nav::Select), Some(Moved::Left));
        let mut p = FfbPage::new(2, 15, true, 100);
        assert_eq!(p.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!(p.nav(Nav::Left), None, "at the range's start");
        assert_eq!(p.nav(Nav::Up), None);
        assert_eq!(p.row(), Row::Done);
        assert_eq!(p.nav(Nav::Back), Some(Moved::Left));
    }

    #[test]
    fn the_strength_steps_from_none_to_full() {
        let mut p = FfbPage::new(0, 280, false, 280);
        p.nav(Nav::Down);
        assert_eq!((p.row(), value(&p, Row::Strength)), (Row::Strength, "100 %".into()));
        assert_eq!(p.nav(Nav::Right), None);
        assert_eq!(p.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!((p.strength, p.value, p.own), (95, 280, false));
        for _ in 0..30 {
            p.nav(Nav::Left);
        }
        assert_eq!(p.strength, 0);
    }

    #[test]
    fn the_smoothing_steps_from_off_to_its_most() {
        let mut p = FfbPage::new(0, 280, false, 280);
        p.nav(Nav::Down);
        p.nav(Nav::Down);
        assert_eq!((p.row(), value(&p, Row::Smoothing)), (Row::Smoothing, "off".into()));
        assert_eq!(p.nav(Nav::Left), None);
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!((p.smoothing, value(&p, Row::Smoothing), p.strength, p.own), (2, "2 ms".into(), 100, false));
        for _ in 0..30 {
            p.nav(Nav::Right);
        }
        assert_eq!(p.smoothing, *SMOOTHING_RANGE.end());
    }

    #[test]
    fn the_linearity_steps_from_off_to_a_line() {
        let mut p = FfbPage::new(0, 280, false, 280);
        for _ in 0..3 {
            p.nav(Nav::Down);
        }
        assert_eq!(
            (p.row(), label(&p, Row::Linearity), value(&p, Row::Linearity)),
            (Row::Linearity, "Linearity".into(), "off".into())
        );
        assert_eq!(p.nav(Nav::Left), None);
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!(
            (p.linearity, value(&p, Row::Linearity), p.strength, p.smoothing, p.own),
            (10, "10 %".into(), 100, 0, false)
        );
        for _ in 0..30 {
            p.nav(Nav::Right);
        }
        assert_eq!(p.linearity, *LINEARITY_RANGE.end());
        // The more linear, the sooner the wheel maxes out: the graph's shaded region starts earlier.
        assert!(load::maxed_from(p.value, p.linearity) < load::maxed_from(p.value, 0));
    }

    /// A page on tarmac at `value` over a load of `seconds` at x 42.5 (suggested 100 %).
    fn page(value: i32, seconds: f32) -> FfbPage {
        let mut session = Session::load(None, Some("rsf-81".into()), &Options::default(), [100; 3]);
        let mut stored = Load::default();
        stored.add(40.0, seconds);
        session.loads.cars.insert(("rsf-81".into(), 1), stored);
        FfbPage { value, own: true, ..FfbPage::of(&session, 1) }
    }

    #[test]
    fn the_status_says_what_to_do() {
        assert_eq!(status(&page(100, 12.5)), "Drive a bit more on tarmac to see your steering load (12/20 s)");
        assert_eq!(
            status(&page(30, 30.0)),
            "The wheel maxes out often (100 % of your driving): raise Sensitivity to about 100 %"
        );
        assert_eq!(
            status(&page(65, 30.0)),
            "Your usual cornering sits high in the wheel's range (maxed out 0 %): raise Sensitivity to about 100 %"
        );
        assert_eq!(
            status(&page(145, 30.0)),
            "Your cornering uses little of the wheel's range: lower Sensitivity to about 100 %, or raise Strength"
        );
        // Near the suggestion, a little below or above it, is good.
        for value in [70, 100, 140] {
            assert_eq!(status(&page(value, 30.0)), "Good: your usual cornering sits mid-range (maxed out 0 %)");
        }
        // The graph reaches past where the wheel maxes out.
        assert!(x_range(&page(200, 30.0)) > load::ratio_at(load::MAXED, 200));
    }

    #[test]
    fn use_suggested_is_one_select_away() {
        // Not with too little driving, nor at the suggestion already.
        assert!(!page(100, 12.5).rows().contains(&Row::UseSuggested));
        assert!(!page(100, 30.0).rows().contains(&Row::UseSuggested));
        let mut p = page(155, 30.0);
        assert_eq!(
            p.rows(),
            [Row::Sensitivity, Row::UseSuggested, Row::Strength, Row::Smoothing, Row::Linearity, Row::Reset, Row::Done]
        );
        p.nav(Nav::Down);
        assert_eq!((p.row(), label(&p, Row::UseSuggested)), (Row::UseSuggested, "Use suggested (100 %)".into()));
        assert_eq!(p.nav(Nav::Select), Some(Moved::Changed));
        assert_eq!((p.value, p.own, p.row()), (100, true, Row::Sensitivity));
        assert!(!p.rows().contains(&Row::UseSuggested));
        assert_eq!(
            help(&p),
            "How much of this car's steering force fits before the wheel maxes out. Higher: lighter, more detail. \
             Lower: heavier, maxes out sooner."
        );
        p.nav(Nav::Down);
        assert_eq!((p.row(), help(&p)), (Row::Strength, "How heavy the wheel feels, for every car.".into()));
        // At the cap the label says it is the most.
        let mut heavy = Load::default();
        heavy.add(300.0, 30.0);
        let p = FfbPage { load: heavy, ..page(100, 30.0) };
        assert_eq!(label(&p, Row::UseSuggested), "Use suggested (500 %, the most)");
    }
}
