//! The pause menu's Camera page, ours (`crate::hud::pause`): a panel at the bottom of the screen over the paused view,
//! which sets the view and this car's nudges of it ([`crate::camera_prefs`]). Its rows: the view, its field of view
//! ([`Fov`]: the chase's, or the one of the cockpit, bonnet and bumper), then sideways, height, forward and tilt for
//! the cockpit, bonnet and bumper, or height and distance for the chase, for the cockpit its head ([`crate::head`]),
//! the driver's arms, the co-driver and the car's steering wheel each on or off ([`Cockpit`]), then Reset (the view
//! back to the car's own place; the field of view and the cockpit's choices stay) and Done.
//! Up and Down pick a row, Left and Right change it; the select acts on Reset and Done, and the back returns to the
//! menu's first page.

use super::pause::Nav;
use crate::camera_prefs::{Cockpit, Nudges, CHASE_STEP, FOV_STEP, REACH, STEP, TILT_REACH, TILT_STEP};
use crate::coords::FOV_RANGE_DEGREES;
use crate::game_camera::Settings;
use crate::views::{CarView, Nudge};
use egui::{Align2, Color32, FontId, Pos2, Rect, Vec2};

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 220);
const TEXT: Color32 = Color32::from_gray(235);
const HINT: Color32 = Color32::from_gray(160);
const SELECTED: Color32 = Color32::from_rgb(200, 60, 40);
const WIDTH: f32 = 380.0;
const ROW: f32 = 26.0;

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    View,
    /// The view's field of view.
    Fov,
    Sideways,
    Height,
    Forward,
    Tilt,
    /// The chase's.
    Distance,
    /// The cockpit's.
    Head,
    Driver,
    CoDriver,
    Wheel,
    Reset,
    Done,
}

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::View => "View",
            Row::Fov => "Field of view",
            Row::Sideways => "Sideways",
            Row::Height => "Height",
            Row::Forward => "Forward",
            Row::Tilt => "Tilt",
            Row::Distance => "Distance",
            Row::Head => "Head motion",
            Row::Driver => "Driver's arms",
            Row::CoDriver => "Co-driver",
            Row::Wheel => "Steering wheel",
            Row::Reset => "Reset",
            Row::Done => "Done",
        }
    }
}

/// The rows `view` has, top to bottom.
pub fn rows(view: CarView) -> &'static [Row] {
    match view {
        CarView::Chase => &[Row::View, Row::Fov, Row::Height, Row::Distance, Row::Reset, Row::Done],
        CarView::Cockpit => &[
            Row::View,
            Row::Fov,
            Row::Sideways,
            Row::Height,
            Row::Forward,
            Row::Tilt,
            Row::Head,
            Row::Driver,
            Row::CoDriver,
            Row::Wheel,
            Row::Reset,
            Row::Done,
        ],
        _ => &[Row::View, Row::Fov, Row::Sideways, Row::Height, Row::Forward, Row::Tilt, Row::Reset, Row::Done],
    }
}

/// The horizontal fields of view, radians: the cockpit's, bonnet's and bumper's (`viewer --fov`, which the free camera
/// takes too) and the chase's (`--chase-fov`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fov {
    pub views: f32,
    pub chase: f32,
}

impl Default for Fov {
    fn default() -> Fov {
        Fov { views: crate::coords::FOV_DEGREES.to_radians(), chase: crate::playback::CHASE_FOV_DEGREES.to_radians() }
    }
}

impl Fov {
    /// The one `view` is drawn with.
    pub fn of(self, view: CarView) -> f32 {
        match view {
            CarView::Chase => self.chase,
            _ => self.views,
        }
    }

    /// These with `view`'s set to `fov`.
    fn with(self, view: CarView, fov: f32) -> Fov {
        match view {
            CarView::Chase => Fov { chase: fov, ..self },
            _ => Fov { views: fov, ..self },
        }
    }
}

/// `fov` (radians) one [`FOV_STEP`] to `sign`'s side, in whole degrees within [`FOV_RANGE_DEGREES`].
fn stepped(fov: f32, sign: f32) -> f32 {
    let (lo, hi) = FOV_RANGE_DEGREES.into_inner();
    (fov.to_degrees() + sign * FOV_STEP).round().clamp(lo, hi).to_radians()
}

/// What a move on the page did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// The view or a nudge changed: show and keep it.
    Changed,
    /// Done or the back: the menu's first page.
    Left,
}

/// The page: the view shown, the car's nudges, the cockpit's choices, the fields of view and the selected row.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraPage {
    pub view: CarView,
    pub nudges: Nudges,
    pub cockpit: Cockpit,
    pub fov: Fov,
    row: usize,
}

impl CameraPage {
    pub fn new(view: CarView, nudges: Nudges, cockpit: Cockpit, fov: Fov) -> CameraPage {
        CameraPage { view, nudges, cockpit, fov, row: 0 }
    }

    /// The cockpit's choice `row` sets.
    fn choice(&mut self, row: Row) -> Option<&mut bool> {
        match row {
            Row::Head => Some(&mut self.cockpit.head),
            Row::Driver => Some(&mut self.cockpit.driver),
            Row::CoDriver => Some(&mut self.cockpit.codriver),
            Row::Wheel => Some(&mut self.cockpit.wheel),
            _ => None,
        }
    }

    /// The selected row.
    pub fn row(&self) -> Row {
        rows(self.view)[self.row]
    }

    /// Takes one move.
    pub fn nav(&mut self, nav: Nav) -> Option<Moved> {
        let n = rows(self.view).len();
        match (nav, self.row()) {
            (Nav::Up, _) => self.row = (self.row + n - 1) % n,
            (Nav::Down, _) => self.row = (self.row + 1) % n,
            (Nav::Back, _) | (Nav::Select, Row::Done) => return Some(Moved::Left),
            (Nav::Select, Row::Reset) => return self.set(Nudge::default()),
            (Nav::Left | Nav::Right, Row::View) => {
                let i = self.view.index() + if nav == Nav::Right { 1 } else { CarView::ALL.len() - 1 };
                self.view = CarView::ALL[i % CarView::ALL.len()];
                return Some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Head | Row::Driver | Row::CoDriver | Row::Wheel) => {
                let on = self.choice(self.row())?;
                *on = !*on;
                return Some(Moved::Changed);
            }
            (Nav::Left | Nav::Right, Row::Fov) => {
                let sign = if nav == Nav::Right { 1.0 } else { -1.0 };
                let fov = self.fov.with(self.view, stepped(self.fov.of(self.view), sign));
                return (fov != self.fov).then(|| {
                    self.fov = fov;
                    Moved::Changed
                });
            }
            (Nav::Left | Nav::Right, row) => {
                let sign = if nav == Nav::Right { 1.0 } else { -1.0 };
                return self.set(changed(self.view, self.nudges[self.view.index()], row, sign));
            }
            (Nav::Select, _) => {}
        }
        None
    }

    /// Sets the view's nudge; changed when it differs.
    fn set(&mut self, nudge: Nudge) -> Option<Moved> {
        let at = &mut self.nudges[self.view.index()];
        (*at != nudge).then(|| {
            *at = nudge;
            Moved::Changed
        })
    }
}

/// `nudge` of `view` after one step of `row` to its `sign`'s side: the fixed views' places within [`REACH`] and tilts
/// within [`TILT_REACH`], the chase's within what moves it ([`Settings::RADIUS_RANGE`], [`Settings::ANGLE_RANGE`]).
pub fn changed(view: CarView, nudge: Nudge, row: Row, sign: f32) -> Nudge {
    let step = |value: f32, step: f32, reach: f32| snap((value + sign * step).clamp(-reach, reach), step);
    let mut out = nudge;
    let shift = &mut out.shift;
    match (view, row) {
        (CarView::Chase, Row::Height) => shift.y = snap(shift.y + sign * CHASE_STEP, CHASE_STEP),
        (CarView::Chase, Row::Distance) => shift.z = snap(shift.z - sign * CHASE_STEP, CHASE_STEP),
        (CarView::Chase, _) => {}
        (_, Row::Sideways) => shift.x = step(shift.x, STEP, REACH),
        (_, Row::Height) => shift.y = step(shift.y, STEP, REACH),
        (_, Row::Forward) => shift.z = step(shift.z, STEP, REACH),
        (_, Row::Tilt) => out.tilt = step(out.tilt, TILT_STEP, TILT_REACH),
        _ => {}
    }
    let chase = |n: Nudge| Settings::default().nudged(n.shift);
    match view == CarView::Chase && chase(out) == chase(nudge) {
        true => nudge,
        false => out,
    }
}

/// `value` on the grid of `step`, written short in the file (0.06, not 0.060000002).
fn snap(value: f32, step: f32) -> f32 {
    let per = (1.0 / step).round();
    (value * per).round() / per
}

/// A value as the page shows it: signed, with a minus sign, and 0 without one.
fn signed(value: f32, decimals: usize, unit: &str) -> String {
    let text = format!("{value:+.decimals$}");
    match text.trim_start_matches(['+', '-']).trim_matches(['0', '.']).is_empty() {
        true => format!("0{unit}"),
        false => format!("{}{unit}", text.replace('-', "\u{2212}")),
    }
}

/// The text of `row`'s value, relative to the car's own place.
fn value(page: &CameraPage, row: Row) -> String {
    let Nudge { shift, tilt } = page.nudges[page.view.index()];
    let chase = page.view == CarView::Chase;
    match row {
        Row::View => {
            let name = page.view.name();
            name[..1].to_ascii_uppercase() + &name[1..]
        }
        Row::Height if chase => signed(shift.y, 1, " m"),
        Row::Distance => signed(-shift.z, 1, " m"),
        Row::Sideways => signed(shift.x * 100.0, 0, " cm"),
        Row::Height => signed(shift.y * 100.0, 0, " cm"),
        Row::Forward => signed(shift.z * 100.0, 0, " cm"),
        Row::Tilt => signed(tilt, 1, "\u{b0}"),
        Row::Fov => format!("{:.0}\u{b0}", page.fov.of(page.view).to_degrees()),
        Row::Head | Row::Driver | Row::CoDriver | Row::Wheel => {
            let on = page.clone().choice(row).is_some_and(|on| *on);
            if on { "On" } else { "Off" }.into()
        }
        Row::Reset | Row::Done => String::new(),
    }
}

/// Draws the page at the bottom of the screen, over the undimmed view.
pub fn paint(ctx: &egui::Context, page: &CameraPage) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("camera page")));
    let screen = ctx.content_rect();
    let rows = rows(page.view);
    let size = Vec2::new(WIDTH, 62.0 + ROW * rows.len() as f32);
    let panel = Rect::from_min_size(Pos2::new(screen.center().x - WIDTH / 2.0, screen.max.y - size.y - 16.0), size);
    painter.rect_filled(panel, 8.0, PANEL);
    let mut at = Pos2::new(panel.min.x + 20.0, panel.min.y + 10.0);
    painter.text(Pos2::new(panel.center().x, at.y), Align2::CENTER_TOP, "Camera", FontId::proportional(18.0), TEXT);
    at.y += 28.0;
    let font = FontId::proportional(16.0);
    for &row in rows {
        let line = Rect::from_min_size(at, Vec2::new(WIDTH - 40.0, ROW - 4.0));
        let selected = row == page.row();
        if selected {
            painter.rect_filled(line, 4.0, SELECTED);
        }
        painter.text(line.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, row.label(), font.clone(), TEXT);
        let value = value(page, row);
        if !value.is_empty() {
            let value = if selected { format!("<  {value}  >") } else { value };
            painter.text(line.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, value, font.clone(), TEXT);
        }
        at.y += ROW;
    }
    painter.text(
        Pos2::new(panel.center().x, panel.max.y - 8.0),
        Align2::CENTER_BOTTOM,
        "Up/Down choose    Left/Right change    Esc back",
        FontId::proportional(12.0),
        HINT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn page(view: CarView) -> CameraPage {
        CameraPage::new(view, Nudges::default(), Cockpit::default(), Fov::default())
    }

    /// Moves `page` to `row`.
    fn to(page: &mut CameraPage, row: Row) {
        while page.row() != row {
            page.nav(Nav::Down);
        }
    }

    #[test]
    fn each_view_has_its_rows_and_the_view_row_cycles() {
        let mut p = page(CarView::Cockpit);
        let names = |p: &CameraPage| rows(p.view).iter().map(|&r| value(p, r)).collect::<Vec<_>>();
        let on = ["On", "Off", "On", "On"];
        assert_eq!(
            names(&p),
            [&["Cockpit", "50\u{b0}", "0 cm", "0 cm", "0 cm", "0\u{b0}"][..], &on, &["", ""]].concat()
        );
        to(&mut p, Row::Head);
        assert_eq!(p.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!(value(&p, Row::Head), "Off");
        to(&mut p, Row::Reset);
        to(&mut p, Row::Wheel);
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        to(&mut p, Row::Reset);
        assert_eq!(p.nav(Nav::Select), None, "Reset leaves the cockpit's choices");
        assert_eq!(p.cockpit, Cockpit { head: false, wheel: false, ..Cockpit::default() });
        to(&mut p, Row::View);
        assert_eq!(p.nav(Nav::Left), Some(Moved::Changed));
        assert_eq!(p.view, CarView::Chase);
        assert_eq!(rows(p.view), [Row::View, Row::Fov, Row::Height, Row::Distance, Row::Reset, Row::Done]);
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!(p.view, CarView::Bonnet);
        // Up from the top wraps to Done.
        assert_eq!(p.nav(Nav::Up), None);
        assert_eq!(p.row(), Row::Done);
        assert_eq!(p.nav(Nav::Select), Some(Moved::Left));
        assert_eq!(page(CarView::Bumper).nav(Nav::Back), Some(Moved::Left));
        // The select does nothing on a value.
        assert_eq!(page(CarView::Bumper).nav(Nav::Select), None);
    }

    #[test]
    fn the_fixed_views_step_within_their_reach() {
        let mut p = page(CarView::Cockpit);
        to(&mut p, Row::Height);
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        p.nav(Nav::Right);
        to(&mut p, Row::Forward);
        p.nav(Nav::Left);
        to(&mut p, Row::Tilt);
        for _ in 0..4 {
            p.nav(Nav::Left);
        }
        let n = p.nudges[CarView::Cockpit.index()];
        assert_eq!(n, Nudge { shift: Vec3::new(0.0, 0.04, -0.02), tilt: -2.0 });
        assert_eq!(value(&p, Row::Height), "+4 cm");
        assert_eq!(value(&p, Row::Forward), "\u{2212}2 cm");
        assert_eq!(value(&p, Row::Tilt), "\u{2212}2.0\u{b0}");
        // The other views are left as they were.
        assert_eq!(p.nudges[CarView::Bonnet.index()], Nudge::default());
        // At the reach a step does nothing.
        for _ in 0..40 {
            p.nav(Nav::Left);
        }
        assert_eq!(p.nudges[CarView::Cockpit.index()].tilt, -TILT_REACH);
        assert_eq!(p.nav(Nav::Left), None);
        to(&mut p, Row::Sideways);
        for _ in 0..60 {
            p.nav(Nav::Right);
        }
        assert_eq!(p.nudges[CarView::Cockpit.index()].shift.x, REACH);
        assert_eq!(p.nav(Nav::Right), None);
        // Reset puts this view back.
        to(&mut p, Row::Reset);
        assert_eq!(p.nav(Nav::Select), Some(Moved::Changed));
        assert_eq!(p.nudges[CarView::Cockpit.index()], Nudge::default());
        assert_eq!(p.nav(Nav::Select), None, "already the car's own");
    }

    #[test]
    fn the_chase_steps_its_height_and_distance_while_they_move_it() {
        let mut p = page(CarView::Chase);
        to(&mut p, Row::Height);
        p.nav(Nav::Right);
        p.nav(Nav::Right);
        p.nav(Nav::Right);
        to(&mut p, Row::Distance);
        p.nav(Nav::Right);
        let n = p.nudges[CarView::Chase.index()];
        assert_eq!(n, Nudge { shift: Vec3::new(0.0, 0.3, -0.1), tilt: 0.0 });
        assert_eq!((value(&p, Row::Height), value(&p, Row::Distance)), ("+0.3 m".into(), "+0.1 m".into()));
        // Nearer until the radius's range stops it; the steps past it are not taken.
        let max = (0..200).filter(|_| p.nav(Nav::Left).is_some()).count();
        assert!(max < 100, "{max}");
        let near = Settings::default().nudged(p.nudges[CarView::Chase.index()].shift).radius;
        assert!((near - Settings::RADIUS_RANGE.start()).abs() < CHASE_STEP, "{near}");
    }

    #[test]
    fn the_field_of_view_steps_the_view_s_own_in_whole_degrees_within_the_range() {
        let mut p = page(CarView::Bonnet);
        to(&mut p, Row::Fov);
        assert_eq!(p.nav(Nav::Right), Some(Moved::Changed));
        assert_eq!(value(&p, Row::Fov), "51\u{b0}");
        assert_eq!(p.fov, Fov { views: 51.0f32.to_radians(), ..Fov::default() });
        // The cockpit and bumper share it; the chase has its own.
        to(&mut p, Row::View);
        p.nav(Nav::Left);
        assert_eq!((p.view, value(&p, Row::Fov)), (CarView::Cockpit, "51\u{b0}".into()));
        p.nav(Nav::Left);
        to(&mut p, Row::Fov);
        assert_eq!((p.view, value(&p, Row::Fov)), (CarView::Chase, "65\u{b0}".into()));
        let steps = (0..100).filter(|_| p.nav(Nav::Right).is_some()).count();
        assert_eq!((steps, p.fov.chase), (55, 120.0f32.to_radians()), "stops at the range's end");
        assert_eq!(p.fov.views, 51.0f32.to_radians());
        // Reset leaves it.
        to(&mut p, Row::Reset);
        assert_eq!(p.nav(Nav::Select), None);
        assert_eq!(p.fov.chase, 120.0f32.to_radians());
        // A field of view off the grid (the file's 72.5) goes onto it with the first step; unmoved, it stays as given.
        let mut p = CameraPage::new(
            CarView::Cockpit,
            Nudges::default(),
            Cockpit::default(),
            Fov { views: 72.5f32.to_radians(), ..Fov::default() },
        );
        assert_eq!(p.fov.views, 72.5f32.to_radians());
        to(&mut p, Row::Fov);
        p.nav(Nav::Left);
        assert_eq!(p.fov.views, 72.0f32.to_radians());
    }

    #[test]
    fn values_show_signed_and_short() {
        assert_eq!(signed(0.0, 0, " cm"), "0 cm");
        assert_eq!(signed(-0.0001, 1, " m"), "0 m");
        assert_eq!(signed(0.3, 1, " m"), "+0.3 m");
        assert_eq!(snap(0.02 * 3.0, STEP).to_string(), "0.06");
        assert_eq!(snap(-0.5 * 3.0, TILT_STEP).to_string(), "-1.5");
    }
}
