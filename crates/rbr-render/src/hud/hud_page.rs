//! The pause menu's HUD page, ours (`crate::hud::pause`): a panel in the middle of the screen over the paused view,
//! which sets the HUD's size, which parts show and where each stands ([`Layout`], [`Parts`]). Its rows: the size, the
//! part picked, whether it shows, then Reset (every part back in its place at 100 %; what shows stays) and Done. While
//! the page is open the HUD shows [`sample`], with something in every part, and the picked part is outlined; the
//! pointer picks a part and drags it to its place ([`HudPage::grab`], [`HudPage::drag`]). Up and Down pick a row, Left
//! and Right change it; the select acts on Reset and Done, and the back returns to the menu's first page.

use egui::{Align2, Color32, FontId, Pos2, Rect, Vec2};
use rbr_recording::splits::{Split, Splits};
use rbr_recording::tyre::Tyre;

use super::camera_page::Moved;
use super::layout::{Layout, SCALES};
use super::model::{CarStatus, HudView, Lamp, Message, Note};
use super::pause::Nav;
use super::telemetry::TracePoint;
use super::{pace, Hud, Part, Parts, Readout};
use crate::live::RaceHud;

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 220);
const TEXT: Color32 = Color32::from_gray(235);
const HINT: Color32 = Color32::from_gray(160);
const SELECTED: Color32 = Color32::from_rgb(200, 60, 40);
const PICKED: Color32 = Color32::from_rgb(255, 196, 0);
const WIDTH: f32 = 380.0;
const ROW: f32 = 26.0;
/// One move of the size, percent.
const SCALE_STEP: i16 = 5;

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Size,
    Part,
    Shown,
    Reset,
    Done,
}

const ROWS: [Row; 5] = [Row::Size, Row::Part, Row::Shown, Row::Reset, Row::Done];

/// A part held by the pointer: the part, where the pointer took it, and its bounds and the layout then.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grab {
    part: Part,
    from: Pos2,
    bounds: Rect,
    layout: Layout,
}

/// The page: the parts shown, the layout, the part picked and the selected row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudPage {
    pub parts: Parts,
    pub layout: Layout,
    part: usize,
    row: usize,
}

impl HudPage {
    pub fn new(parts: Parts, layout: Layout) -> HudPage {
        HudPage { parts, layout, part: 0, row: 0 }
    }

    /// The selected row and the picked part.
    pub fn row(&self) -> Row {
        ROWS[self.row]
    }

    pub fn part(&self) -> Part {
        Part::ALL[self.part]
    }

    /// Picks the part `hud` drew under `pointer` and holds it; `None` on the page's panel or off every part.
    pub fn grab(&mut self, hud: &dyn Hud, screen: Rect, pointer: Pos2) -> Option<Grab> {
        let held = |part: &Part| part.carrier() == *part && hud.bounds(*part).is_some_and(|b| b.contains(pointer));
        let part = Part::ALL.into_iter().find(held).filter(|_| !panel(screen).contains(pointer))?;
        self.part = part as usize;
        Some(Grab { part, from: pointer, bounds: hud.bounds(part)?, layout: self.layout })
    }

    /// Moves the held part with the `pointer`, kept on `screen`.
    pub fn drag(&mut self, grab: &Grab, screen: Rect, pointer: Pos2) {
        self.layout = grab.layout.moved(grab.part, grab.bounds, pointer - grab.from, screen);
    }

    /// Takes one move.
    pub fn nav(&mut self, nav: Nav) -> Option<Moved> {
        let n = ROWS.len();
        let before = *self;
        let sign: i16 = if nav == Nav::Right { 1 } else { -1 };
        match (nav, self.row()) {
            (Nav::Up, _) => self.row = (self.row + n - 1) % n,
            (Nav::Down, _) => self.row = (self.row + 1) % n,
            (Nav::Back, _) | (Nav::Select, Row::Done) => return Some(Moved::Left),
            (Nav::Select, Row::Reset) => self.layout = Layout::default(),
            (Nav::Select, Row::Shown) | (Nav::Left | Nav::Right, Row::Shown) => {
                self.parts.0[self.part] = !self.parts.0[self.part];
            }
            (Nav::Left | Nav::Right, Row::Size) => {
                let scale = self.layout.scale.saturating_add_signed(sign * SCALE_STEP);
                self.layout.scale = scale.clamp(*SCALES.start(), *SCALES.end());
            }
            (Nav::Left | Nav::Right, Row::Part) => {
                let n = Part::ALL.len();
                self.part = (self.part + if nav == Nav::Right { 1 } else { n - 1 }) % n;
            }
            _ => {}
        }
        // Moving between rows and parts changes nothing to show or keep.
        ((before.parts, before.layout) != (self.parts, self.layout) || before.part != self.part)
            .then_some(Moved::Changed)
    }
}

/// The text of `row`'s value.
fn value(page: &HudPage, row: Row) -> String {
    match row {
        Row::Size => format!("{} %", page.layout.scale),
        Row::Part => page.part().label().into(),
        Row::Shown => if page.parts.shows(page.part()) { "On" } else { "Off" }.into(),
        Row::Reset | Row::Done => String::new(),
    }
}

/// `base` with something in every part, for the open page: a note called, a split just passed, a gap to a ghost, a
/// message, a damaged car, warm and cold tyres, a trace.
pub fn sample(base: Readout) -> Readout {
    let tyre = |celsius: f32, grip| {
        let kelvin = celsius + 273.15;
        Tyre { surface: kelvin, tread: kelvin, grip, optimum: 348.15, wear: 0.006 }
    };
    let trace = (0..300).map(|i| {
        let t = i as f32 / 30.0;
        let pedal = (t * 1.3).sin();
        TracePoint {
            age: 10.0 - t,
            control: [pedal.max(0.0), (-pedal).max(0.0) * 0.8, 0.0, 0.0, (t * 0.9).sin() * 0.6],
        }
    });
    let state = HudView {
        note: pace::symbol(2).map(|symbol| (Note { symbol, flags: 0, called: 60.0, at: None }, 1.0)),
        note_distance: Some(40.0),
        lamps: Some([Lamp::On, Lamp::Off, Lamp::Off, Lamp::Off, Lamp::Off]),
        flash_on: true,
        message: Some((Message::Stuck, 1.0)),
        progress: Some((0.4, vec![0.33, 0.66])),
        boost: Some(96_000.0),
        water: Some(92.0),
        odometer: Some((1700.0, Some(2500.0))),
        car: Some(CarStatus { systems: [1, 0, 2, 0, 0, 1], suspension: [0, 2, 0, 0], tyres: [0; 4] }),
        rev_limit: base.state.rev_limit,
        upshift: base.state.upshift,
        tyres: Some([tyre(52.0, 0.93), tyre(55.0, 0.95), tyre(74.0, 1.0), tyre(112.0, 0.92)]),
        ..HudView::default()
    };
    Readout {
        race: Some(RaceHud { time: 62.4, countdown: -62.4, penalties: 0.0, running: true, jump_start: false }),
        splits: Some(Splits {
            elapsed: 62.4,
            race_time: 62.4,
            splits: vec![Split { time: Some(61.0), reference: Some(61.8) }, Split::default()],
            ..Splits::default()
        }),
        state,
        ghost_gap: Some(-0.42),
        trace: trace.collect(),
        ..base
    }
}

/// Draws the page in the middle of the screen, over the undimmed view, and outlines the picked part where `hud` drew
/// it.
pub fn paint(ctx: &egui::Context, hud: &dyn Hud, page: &HudPage) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("hud page")));
    let screen = ctx.content_rect();
    if let Some(bounds) = hud.bounds(page.part().carrier()) {
        painter.rect_stroke(bounds.expand(3.0), 4.0, (2.0, PICKED), egui::StrokeKind::Outside);
    }
    let panel = panel(screen);
    painter.rect_filled(panel, 8.0, PANEL);
    let mut at = Pos2::new(panel.min.x + 20.0, panel.min.y + 10.0);
    painter.text(Pos2::new(panel.center().x, at.y), Align2::CENTER_TOP, "HUD", FontId::proportional(18.0), TEXT);
    at.y += 28.0;
    let font = FontId::proportional(16.0);
    for row in ROWS {
        let line = Rect::from_min_size(at, Vec2::new(WIDTH - 40.0, ROW - 4.0));
        let selected = row == page.row();
        if selected {
            painter.rect_filled(line, 4.0, SELECTED);
        }
        let label = match row {
            Row::Size => "Size",
            Row::Part => "Part",
            Row::Shown => "Shown",
            Row::Reset => "Reset",
            Row::Done => "Done",
        };
        painter.text(line.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, label, font.clone(), TEXT);
        let value = value(page, row);
        if !value.is_empty() {
            let value = if selected { format!("<  {value}  >") } else { value };
            painter.text(line.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, value, font.clone(), TEXT);
        }
        at.y += ROW;
    }
    let hints = ["Drag a part to move it", "Up/Down choose    Left/Right change    Esc back"];
    for (hint, up) in hints.into_iter().zip([24.0, 8.0]) {
        let at = Pos2::new(panel.center().x, panel.max.y - up);
        painter.text(at, Align2::CENTER_BOTTOM, hint, FontId::proportional(12.0), HINT);
    }
}

/// The page's panel in the middle of `screen`.
fn panel(screen: Rect) -> Rect {
    Rect::from_center_size(screen.center(), Vec2::new(WIDTH, 78.0 + ROW * ROWS.len() as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to(page: &mut HudPage, row: Row) {
        while page.row() != row {
            page.nav(Nav::Down);
        }
    }

    #[test]
    fn the_rows_size_pick_and_show_a_part() {
        let mut page = HudPage::new(Parts::default(), Layout::default());
        assert_eq!(page.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!((page.layout.scale, value(&page, Row::Size)), (95, "95 %".to_owned()));
        page.layout.scale = 200;
        assert_eq!(page.nav(Nav::Right), None);
        // Left from the first part wraps to the last, which is off until shown.
        to(&mut page, Row::Part);
        assert_eq!(page.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!((page.part(), value(&page, Row::Shown)), (Part::Trace, "Off".to_owned()));
        to(&mut page, Row::Shown);
        page.nav(Nav::Select);
        assert!(page.parts.shows(Part::Trace));
        page.layout.offsets[Part::Trace as usize] = [-10, 10];
        // Reset puts every part back at 100 % and leaves what shows; Done and the back leave.
        to(&mut page, Row::Reset);
        assert_eq!(page.nav(Nav::Select), Some(Moved::Changed));
        assert!(page.layout == Layout::default() && page.parts.shows(Part::Trace));
        assert_eq!(page.nav(Nav::Select), None);
        assert_eq!(page.nav(Nav::Back), Some(Moved::Left));
    }

    /// A look that drew the dial only.
    struct Drawn(Rect);

    impl Hud for Drawn {
        fn paint(&mut self, _: &egui::Context, _: &Readout) {}

        fn bounds(&self, part: Part) -> Option<Rect> {
            (part == Part::Dash).then_some(self.0)
        }
    }

    #[test]
    fn the_pointer_picks_the_part_under_it_and_drags_it() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let hud = Drawn(Rect::from_min_max(Pos2::new(1076.0, 516.0), Pos2::new(1260.0, 700.0)));
        let mut page = HudPage::new(Parts::default(), Layout::default());
        assert_eq!(page.grab(&hud, screen, Pos2::new(100.0, 100.0)), None);
        // Not through the page's panel.
        assert_eq!(page.grab(&Drawn(screen), screen, screen.center()), None);
        let grab = page.grab(&hud, screen, Pos2::new(1100.0, 600.0)).unwrap();
        assert_eq!(page.part(), Part::Dash);
        page.drag(&grab, screen, Pos2::new(1000.0, 590.0));
        assert_eq!(page.layout.offsets[Part::Dash as usize], [-100, -10]);
    }
}
