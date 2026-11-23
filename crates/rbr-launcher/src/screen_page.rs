//! The Graphics & sound page's Screen section (ours): what the player drives on, one flat screen, one curved panel
//! (`viewer --curved`) or three screens (`viewer --triple`), its size and the eye's distance, and whether the in-car
//! views' field of view follows from them ([`Form::matched_fov`]) or is set by hand.
//!
//! As other sims do it (iRacing's monitor setup and its Compute, Le Mans Ultimate's triple screen tool locking the
//! slider, ACC's view settings), the field of view comes from what the player measures: the diagonal, the radius
//! printed on a curved panel (1800R), and the distance from the eyes to the middle of the screen sitting as they drive.

use egui::ComboBox;
use rbr_render::coords;
use rbr_render::triple::{Curved, Rig};

use crate::graphics_page::slider;
use crate::kit;
use crate::settings_page::note;
use crate::theme::WARN;
use crate::Form;

/// Triple screens (`viewer --triple`): three like screens side by side, the side ones turned towards the player.
/// Each screen's diagonal in inches (10 to 65, 27 by default), the frame around each screen's image in millimetres
/// (0 to 60, 10 by default), the eye to the centre screen in centimetres (30 to 200, 70 by default), and the side
/// screens' turn in degrees (`None`: facing the eye, the default; 0 to 90).
pub const TRIPLE_DIAGONAL_DEFAULT: f32 = 27.0;
pub const TRIPLE_DIAGONAL_RANGE: std::ops::RangeInclusive<f32> = 10.0..=65.0;
pub const TRIPLE_BEZEL_DEFAULT: f32 = 10.0;
pub const TRIPLE_BEZEL_RANGE: std::ops::RangeInclusive<f32> = 0.0..=60.0;
pub const TRIPLE_DISTANCE_DEFAULT: f32 = 70.0;
pub const TRIPLE_DISTANCE_RANGE: std::ops::RangeInclusive<f32> = 30.0..=200.0;
pub const TRIPLE_ANGLE_RANGE: std::ops::RangeInclusive<f32> = 0.0..=rbr_render::triple::MAX_ANGLE;

/// One flat or curved screen (`viewer --curved`): its diagonal in inches (24 to 65, 34 by default), a curved
/// panel's radius in millimetres (800 to 4000, 1800 by default: a 1800R) and the eye to its middle in centimetres
/// (30 to 200, 70 by default).
pub const SCREEN_DIAGONAL_DEFAULT: f32 = 34.0;
pub const SCREEN_DIAGONAL_RANGE: std::ops::RangeInclusive<f32> = 24.0..=65.0;
pub const SCREEN_RADIUS_DEFAULT: f32 = 1800.0;
pub const SCREEN_RADIUS_RANGE: std::ops::RangeInclusive<f32> = 800.0..=4000.0;
pub const SCREEN_DISTANCE_DEFAULT: f32 = 70.0;
pub const SCREEN_DISTANCE_RANGE: std::ops::RangeInclusive<f32> = 30.0..=200.0;

/// What the screens are: one flat one, one curved one (`viewer --curved`), or three (`viewer --triple`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screens {
    Single,
    Curved,
    Triple,
}

/// `names` once each monitor is picked: `viewer --triple-monitors` takes three.
fn picked(names: Option<&[String; 3]>) -> Option<&[String; 3]> {
    names.filter(|names| names.iter().all(|name| !name.is_empty()))
}

/// One screen's aspect on a display or window `whole` pixels: all of it with a window a monitor (`per_monitor`),
/// else a third of it.
fn screen_aspect(whole: (u32, u32), per_monitor: bool) -> f32 {
    let aspect = whole.0 as f32 / whole.1.max(1) as f32;
    match per_monitor {
        true => aspect,
        false => aspect / 3.0,
    }
}

/// `degrees` within the viewer's field of view range.
fn in_range(degrees: f32) -> f32 {
    degrees.clamp(*coords::FOV_RANGE_DEGREES.start(), *coords::FOV_RANGE_DEGREES.end())
}

impl Form {
    /// The screens in use: triple wins over curved when the file has both on.
    pub fn screens(&self) -> Screens {
        match (self.triple, self.curved) {
            (true, _) => Screens::Triple,
            (false, true) => Screens::Curved,
            (false, false) => Screens::Single,
        }
    }

    /// Picks `screens`; a curved panel or triple screens match the field of view to them.
    fn set_screens(&mut self, screens: Screens) {
        self.triple = screens == Screens::Triple;
        self.curved = screens == Screens::Curved;
        self.fov_from_screen |= screens != Screens::Single;
    }

    /// The curved panel, when curved and not triple (`viewer --curved`).
    pub fn curved_panel(&self) -> Option<Curved> {
        (self.screens() == Screens::Curved).then_some(Curved {
            diagonal: self.screen_diagonal,
            radius: self.screen_radius,
            distance: self.screen_distance,
        })
    }

    /// The triple-screen rig, when triple screens are on (`viewer --triple`).
    pub fn triple_rig(&self) -> Option<Rig> {
        self.triple.then(|| self.rig())
    }

    fn rig(&self) -> Rig {
        Rig {
            diagonal: self.triple_diagonal,
            bezel: self.triple_bezel,
            distance: self.triple_distance,
            angle: self.triple_angle,
        }
    }

    /// The three monitors' names once each is picked (`viewer --triple-monitors`).
    pub fn triple_monitors(&self) -> Option<&[String; 3]> {
        picked(self.triple_monitors.as_ref())
    }

    /// One screen's aspect as the viewer will draw it on the monitor (fullscreen) or the window; 16:9 before the
    /// monitor is known.
    fn screen_aspect(&self) -> f32 {
        match (self.fullscreen, self.monitor) {
            (true, Some(monitor)) => screen_aspect(monitor, self.triple_monitors().is_some()),
            (true, None) => 16.0 / 9.0,
            (false, _) => screen_aspect(self.size, false),
        }
    }

    /// The whole monitor's (fullscreen) or window's aspect, for a single or curved screen; 16:9 before the monitor
    /// is known.
    fn whole_aspect(&self) -> f32 {
        match (self.fullscreen, self.monitor) {
            (true, Some(monitor)) => screen_aspect(monitor, true),
            (true, None) => 16.0 / 9.0,
            (false, _) => screen_aspect(self.size, true),
        }
    }

    /// The field of view, degrees, that shows the world at its true size from the seat: the whole flat screen's, the
    /// whole curved panel's, or triple screens' centre one's (what `viewer --fov` means for each).
    pub fn screen_fov(&self) -> f32 {
        let fov = match self.screens() {
            Screens::Single => {
                let flat =
                    Rig { diagonal: self.screen_diagonal, bezel: 0.0, distance: self.screen_distance, angle: None };
                flat.true_fov(self.whole_aspect())
            }
            Screens::Curved => self.curved_panel().map_or(0.0, |panel| panel.true_fov(self.whole_aspect())),
            Screens::Triple => self.rig().true_fov(self.screen_aspect()),
        };
        fov.to_degrees()
    }

    /// The in-car views' field of view when it follows the screen ([`Form::screen_fov`] within the viewer's range,
    /// to a tenth of a degree), else `None`: the Camera section's slider.
    pub fn matched_fov(&self) -> Option<f32> {
        self.fov_from_screen.then(|| (in_range(self.screen_fov()) * 10.0).round() / 10.0)
    }

    /// The in-car views' field of view the drive starts with (`viewer --fov`).
    pub fn drive_fov(&self) -> f32 {
        self.matched_fov().unwrap_or(self.fov)
    }

    /// The Screen section: the kind of screen, its size and distance, and the field of view matched to it or set by
    /// hand.
    pub(crate) fn screen_fields(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        kit::group_label(ui, "Screen", None);
        let screens = self.screens();
        kit::panel(ui, |rows| {
            let tip = "Single: one flat screen, ultrawides too. Curved: one curved panel, drawn in three segments \
                       that follow its curve. Triple: three like screens side by side, the side ones turned towards you";
            let mut picked = screens;
            rows.row("Screens", |ui| {
                let options = [(Screens::Single, "Single"), (Screens::Curved, "Curved"), (Screens::Triple, "Triple")];
                if kit::segmented(ui, "screens", &mut picked, &options).on_hover_text(tip).changed() {
                    self.set_screens(picked);
                }
            });
            match screens {
                Screens::Single if self.fov_from_screen => self.flat_rows(rows, false),
                Screens::Single => {}
                Screens::Curved => self.flat_rows(rows, true),
                Screens::Triple => self.triple_rows(rows),
            }

            let tip = "Match my screen: the in-car views show the world at its true size, from your screen's size \
                       and your distance to it. Manual: the Camera section's slider";
            rows.row("Field of view", |ui| {
                let options = [(true, "Match my screen"), (false, "Manual")];
                kit::segmented(ui, "field of view from the screen", &mut self.fov_from_screen, &options)
                    .on_hover_text(tip);
            });
            rows.hint(&self.fov_text());
        });
        let hint = match screens {
            Screens::Single if self.fov_from_screen => {
                "Measure from your eyes to the middle of the screen, sitting as you drive."
            }
            Screens::Single => return,
            Screens::Curved => {
                "The field of view covers the whole panel. Measure from your eyes to its middle, sitting as you drive; \
                 the radius is on the box, 1800R being 1800 mm."
            }
            Screens::Triple => {
                "The field of view is the centre screen's. Measure from your eyes to the centre screen, sitting as you \
                 drive."
            }
        };
        ui.add_space(4.0);
        kit::hint(ui, hint);
    }

    /// What the field of view is, or would be matched to the screen.
    fn fov_text(&self) -> String {
        let fov = self.screen_fov();
        let limited =
            if in_range(fov) != fov { format!(" (the viewer takes {:.0}°)", in_range(fov)) } else { String::new() };
        let side = match (self.screens(), self.triple_angle) {
            (Screens::Triple, None) => {
                format!(", side screens turned {:.0}°", self.rig().side_angle(self.screen_aspect()).to_degrees())
            }
            _ => String::new(),
        };
        let unknown =
            if self.fullscreen && self.monitor.is_none() { ", taking a 16:9 monitor until it is known" } else { "" };
        match self.fov_from_screen {
            true => format!("In-car views at {fov:.0}° from your seat{limited}{side}{unknown}"),
            false if self.screens() == Screens::Single => {
                "Set on the Camera section below. Match my screen works it out from its size and your distance".into()
            }
            false => {
                format!("Your screen's true field of view is {fov:.0}°{side}{unknown}; Camera below sets it by hand")
            }
        }
    }

    /// A single screen's or a curved panel's size and distance, and the panel's radius.
    fn flat_rows(&mut self, rows: &mut kit::Rows<'_>, curved: bool) {
        rows.row("Screen size", |ui| {
            slider(ui, &mut self.screen_diagonal, SCREEN_DIAGONAL_RANGE, " in", "The screen's diagonal")
        });
        if curved {
            rows.row("Curve radius", |ui| {
                let tip = "The curve's radius, on the box or in the manual: a 1800R panel is 1800 mm";
                slider(ui, &mut self.screen_radius, SCREEN_RADIUS_RANGE, " mm", tip)
            });
        }
        rows.row("Distance", |ui| {
            let tip = "From your eyes to the middle of the screen, sitting as you drive";
            slider(ui, &mut self.screen_distance, SCREEN_DISTANCE_RANGE, " cm", tip)
        });
    }

    /// Triple screens' size, bezel, distance and side angle, and the monitors they are on.
    fn triple_rows(&mut self, rows: &mut kit::Rows<'_>) {
        let auto_degrees = Rig { angle: None, ..self.rig() }.side_angle(self.screen_aspect()).to_degrees();
        let names = self.monitors.clone();
        rows.row("Screen size", |ui| {
            slider(ui, &mut self.triple_diagonal, TRIPLE_DIAGONAL_RANGE, " in", "Each screen's diagonal")
        });
        rows.row("Bezel", |ui| {
            let tip = "The frame around each screen's image: two of them part neighbouring images";
            slider(ui, &mut self.triple_bezel, TRIPLE_BEZEL_RANGE, " mm", tip)
        });
        rows.row("Distance", |ui| {
            let tip = "From your eyes to the centre screen, sitting as you drive";
            slider(ui, &mut self.triple_distance, TRIPLE_DISTANCE_RANGE, " cm", tip)
        });

        let tip = "How far each side screen is turned towards you from the centre screen's plane. Auto faces their \
                   middles at your eyes at this distance";
        let mut auto = self.triple_angle.is_none();
        let mut degrees = self.triple_angle.unwrap_or(auto_degrees);
        rows.row("Side angle", |ui| {
            ui.add_enabled_ui(!auto, |ui| {
                slider(ui, &mut degrees, TRIPLE_ANGLE_RANGE, "°", tip);
            });
            if ui.checkbox(&mut auto, "Auto").on_hover_text(tip).changed() {
                self.triple_angle = if auto { None } else { Some(degrees) };
            }
        });
        if !auto {
            self.triple_angle = Some(degrees);
        }

        let tip = "One borderless fullscreen window a monitor, or one window split in thirds on a spanned display \
                   such as NVIDIA Surround or AMD Eyefinity";
        let mut three = self.triple_monitors.is_some();
        rows.row("Displays", |ui| {
            let options = [(false, "One wide display (Surround / Eyefinity)"), (true, "Three monitors")];
            if kit::segmented(ui, "triple displays", &mut three, &options).on_hover_text(tip).changed() {
                self.triple_monitors = three.then(|| {
                    [
                        names.first().cloned().unwrap_or_default(),
                        names.get(1).cloned().unwrap_or_default(),
                        names.get(2).cloned().unwrap_or_default(),
                    ]
                });
            }
        });
        if let Some(monitors) = &mut self.triple_monitors {
            for (slot, side) in monitors.iter_mut().zip(["Left", "Centre", "Right"]) {
                let mut options = names.clone();
                if !slot.is_empty() && !options.contains(slot) {
                    options.push(slot.clone());
                }
                rows.row(side, |ui| {
                    ComboBox::from_id_salt(("triple monitor", side))
                        .selected_text(if slot.is_empty() { "Pick one" } else { slot.as_str() })
                        .show_ui(ui, |ui| {
                            for name in &options {
                                ui.selectable_value(slot, name.clone(), name);
                            }
                        });
                });
            }
            if names.len() < 3 {
                note(rows, WARN, &format!("{} of 3 monitors connected", names.len()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_monitors_go_once_all_three_are_picked() {
        let names = ["DP-1".to_owned(), String::new(), "DP-2".to_owned()];
        assert_eq!(picked(Some(&names)), None);
        let names = ["DP-1".to_owned(), "DP-3".to_owned(), "DP-2".to_owned()];
        assert_eq!(picked(Some(&names)), Some(&names));
    }

    #[test]
    fn a_triple_screen_is_a_third_of_a_spanned_display() {
        let wide = 16.0 / 9.0;
        assert!((screen_aspect((5760, 1080), false) - wide).abs() < 1e-6);
        assert!((screen_aspect((1920, 1080), true) - wide).abs() < 1e-6);
    }
}
