//! The `realistic` model: the calibrated mean-value engine behind the [`Model`] interface (`notes/engine-model.md`).

use crate::calibrate::{self, Calibration};
use crate::engine::{self, Out, Spec, State, Step};
use crate::{Inputs, Model, Params, Telemetry};

/// One car's engine and its state.
#[derive(Clone, Debug)]
pub struct Realistic {
    spec: Spec,
    calibration: Calibration,
    state: State,
    last: Out,
}

impl Realistic {
    /// Calibrates the engine for `params` (a fraction of a second) and starts it at rest in the reference air.
    pub fn new(params: &Params) -> Self {
        let (spec, calibration) = calibrate::calibrate(params);
        Self { spec, calibration, state: State::new(), last: Out::default() }
    }

    pub fn calibration(&self) -> &Calibration {
        &self.calibration
    }

    pub fn spec(&self) -> &Spec {
        &self.spec
    }

    /// The engine's constants, to change for a comparison (anti-lag off and the like).
    pub fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// The last step's torque and its parts.
    pub fn out(&self) -> &Out {
        &self.last
    }
}

impl Model for Realistic {
    fn torque(&mut self, input: &Inputs) -> f32 {
        self.last = engine::step(&self.spec, &mut self.state, &Step::from_inputs(input));
        self.last.torque as f32
    }

    fn restart(&mut self) {
        self.state = State::new();
        self.last = Out::default();
    }

    fn telemetry(&self) -> Telemetry {
        let s = &self.state;
        Telemetry {
            manifold_pressure: s.p_im as f32,
            boost_pressure: s.p_boost as f32,
            turbo_speed: s.omega_t as f32,
            plate: s.plate as f32,
            cut: self.last.cut as f32,
            anti_lag: self.last.anti_lag,
            blow_off: self.last.blow_off as f32,
            air_pressure: self.last.p_a as f32,
        }
    }
}
