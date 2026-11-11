//! Experimental tuning of the brush tyre in development (Settings page, under Physics, while the physics is
//! `experimental-brush`): a few of the model's values to drive and feel (`rbr_tyre::Tuning`), saved as
//! `[launcher] tyre_tuning` and passed as `rbr-standalone --tyre-tuning` at each stage start (so a replay's header
//! keeps them). Championship stages and trials run untuned.

use egui::{Slider, Ui};
use rbr_tyre::tuning::{self, Tuning};

use crate::pages::{fields, section, WARN};
use crate::physics::PhysicsProfile;

/// Under the section: where the values apply.
const WHERE: &str = "Applies at the next stage start, never during a drive. Championship stages and trials always \
                     run the model as it is. No records, ghosts or championship results are saved.";

/// What the drive page says while a value is off the model's.
pub const ACTIVE: &str = "Experimental tuning active";

/// `rbr-standalone`'s arguments for `tuning` under `physics`: `--tyre-tuning` with the values off the model's, only
/// under the brush tyre; none at the model's.
pub fn args(physics: PhysicsProfile, tuning: &Tuning) -> Vec<String> {
    match physics == PhysicsProfile::ExperimentalBrush && !tuning.is_default() {
        true => vec!["--tyre-tuning".to_owned(), tuning.to_string()],
        false => Vec::new(),
    }
}

/// The section: a slider per value, each starting at the model's, and Reset to model. Shown only under the brush tyre.
pub fn tuning_section(ui: &mut Ui, physics: PhysicsProfile, tuning: &mut Tuning) {
    if physics != PhysicsProfile::ExperimentalBrush {
        return;
    }
    section(ui, "Experimental tuning");
    fields(ui, "tyre tuning", |ui| {
        ui.label("Slide grip on hard surfaces");
        let mut slide = tuning.slide_grip.unwrap_or(tuning::SLIDE_GRIP_MODEL);
        let (lo, hi) = tuning::SLIDE_GRIP;
        let tip = "Side grip left at 30° of slip over the peak (dry tarmac, 4 kN, 54 km/h): lower slides away \
                   sooner past the limit. The model's 0.83; road tyres 0.73-0.83, racing tyres 0.80-0.90.";
        if ui.add(Slider::new(&mut slide, lo..=hi).step_by(0.001).fixed_decimals(3)).on_hover_text(tip).changed() {
            let slide = (slide * 1000.0).round() / 1000.0;
            tuning.slide_grip = (slide != tuning::SLIDE_GRIP_MODEL).then_some(slide);
        }
        ui.end_row();
        let factor = |ui: &mut Ui, label: &str, value: &mut f64, (lo, hi): (f64, f64), tip: &str| {
            ui.label(label);
            if ui
                .add(Slider::new(value, lo..=hi).step_by(0.05).fixed_decimals(2).suffix("×"))
                .on_hover_text(tip)
                .changed()
            {
                *value = (*value * 100.0).round() / 100.0;
            }
            ui.end_row();
        };
        let grip = "Times the grip of this surface (the model's level). Gravel's covers every loose surface but snow.";
        factor(ui, "Grip on tarmac", &mut tuning.tarmac, tuning::GRIP, grip);
        factor(ui, "Grip on gravel", &mut tuning.gravel, tuning::GRIP, grip);
        factor(ui, "Grip on snow", &mut tuning.snow, tuning::GRIP, grip);
        factor(ui, "Grip on ice", &mut tuning.ice, tuning::GRIP, grip);
        let stiffness = "How fast the side and braking forces build with slip; the peak grip stays.";
        factor(ui, "Cornering stiffness", &mut tuning.stiffness, tuning::STIFFNESS, stiffness);
        let response = "How far the tyre rolls before its force follows a steering change (the relaxation length): \
                        higher is lazier.";
        factor(ui, "Response", &mut tuning.response, tuning::RESPONSE, response);
        ui.label("");
        if ui.add_enabled(!tuning.is_default(), egui::Button::new("Reset to model")).clicked() {
            *tuning = Tuning::default();
        }
        ui.end_row();
    });
    ui.weak(WHERE);
}

/// The drive page's line while a value is off the model's under the brush tyre, the values on hover.
pub fn active_note(ui: &mut Ui, physics: PhysicsProfile, tuning: &Tuning) {
    if let [_, values] = args(physics, tuning).as_slice() {
        ui.colored_label(WARN, ACTIVE).on_hover_text(values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_brush_tyre_takes_the_values_off_the_model_s() {
        let tuned = Tuning { gravel: 0.9, slide_grip: Some(0.8), ..Tuning::default() };
        let brush = PhysicsProfile::ExperimentalBrush;
        assert_eq!(args(brush, &tuned), ["--tyre-tuning", "slide-grip=0.8,gravel=0.9"]);
        assert!(args(brush, &Tuning::default()).is_empty());
        assert!(args(PhysicsProfile::Experimental, &tuned).is_empty());
        assert!(args(PhysicsProfile::Ngp, &tuned).is_empty());
    }

    #[test]
    fn drawing_the_section_leaves_the_values_as_they_are() {
        // A slider snaps its value to its step when drawn: the model's slide grip must lie on it.
        for before in [Tuning::default(), Tuning { slide_grip: Some(0.82), gravel: 0.9, ..Tuning::default() }] {
            let mut t = before;
            egui::Context::default()
                .run_ui(egui::RawInput::default(), |ui| tuning_section(ui, PhysicsProfile::ExperimentalBrush, &mut t))
                .textures_delta
                .clear();
            assert_eq!(t, before);
        }
    }
}
