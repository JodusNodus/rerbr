//! A game mode's rules over the stage loop ([`crate::game::Game::frame_with`]): what a mode reads from the drive
//! and when it ends it. The rally school (`rbr-school`) is one; a quick race has none (`()`).
//!
//! The rules only observe: they never write the physics. The game calls them where the original's listeners run:
//! the control payload before each driving substep, the step's emits after it, and the frame after the marker keys.

use crate::control::CarControl;
use crate::game::StepSample;

/// What a driving substep gave the mode's listeners.
#[derive(Debug, Clone, Copy)]
pub struct StepView {
    /// The car's sample inside the step (its velocity, position, brake-wall emits and retire).
    pub sample: StepSample,
    /// The driveline parameter's float emit, when it is on the table.
    pub distance: Option<f32>,
    /// The drive line's point at [`ModeRules::drive_line_at`], when the mode asked and the host has it.
    pub drive_line: Option<[f32; 3]>,
    /// The application state the step ran in.
    pub app_state: u32,
}

/// How a mode ends the drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeEnd {
    Finished,
    Retired,
}

/// A game mode's listeners on the stage loop. Every hook does nothing by default.
pub trait ModeRules {
    /// The drive starts.
    fn start(&mut self) {}
    /// A driving substep's control payload, before the step.
    fn before_step(&mut self, _control: &CarControl) {}
    /// The distance the game samples the drive line at for [`StepView::drive_line`].
    fn drive_line_at(&self) -> Option<f32> {
        None
    }
    /// The step's emits.
    fn after_step(&mut self, _step: &StepView) {}
    /// A driving frame of `dt` after the marker keys, with the HUD's race time: whether the mode ends the drive.
    fn frame(&mut self, _dt: f32, _race_time: f32) -> Option<ModeEnd> {
        None
    }
}

impl ModeRules for () {}

/// A mode that may not be on (the host's `Option` of a mode): its hooks when it is.
impl<R: ModeRules> ModeRules for Option<R> {
    fn start(&mut self) {
        if let Some(rules) = self {
            rules.start();
        }
    }
    fn before_step(&mut self, control: &CarControl) {
        if let Some(rules) = self {
            rules.before_step(control);
        }
    }
    fn drive_line_at(&self) -> Option<f32> {
        self.as_ref()?.drive_line_at()
    }
    fn after_step(&mut self, step: &StepView) {
        if let Some(rules) = self {
            rules.after_step(step);
        }
    }
    fn frame(&mut self, dt: f32, race_time: f32) -> Option<ModeEnd> {
        self.as_mut()?.frame(dt, race_time)
    }
}
