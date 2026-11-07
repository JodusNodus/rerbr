//! The engine model (ours, experimental, `rbr_engine`) of the runs on this thread: with one set, NGP's engine wrapper
//! takes its torque in place of NGP's formula ([`super::engine`]); with none (the default) the wrapper is NGP's.

use std::cell::RefCell;

use rbr_engine::{Inputs, Model};

thread_local! {
    static MODEL: RefCell<Option<Box<dyn Model>>> = const { RefCell::new(None) };
}

/// The engine model the runs on this thread step from now on (`None`: NGP's formula); returns the one it replaces.
/// The host sets a fresh one at each start and restart: its state is the model's own, which snapshots don't carry.
pub fn set_engine_model(model: Option<Box<dyn Model>>) -> Option<Box<dyn Model>> {
    MODEL.replace(model)
}

/// The model's telemetry after its last step, when one is set.
pub fn engine_model_telemetry() -> Option<rbr_engine::Telemetry> {
    MODEL.with_borrow(|m| m.as_ref().map(|m| m.telemetry()))
}

/// Whether a model is set on this thread.
pub(super) fn is_set() -> bool {
    MODEL.with_borrow(Option::is_some)
}

/// The set model's torque for `input` (`None` without one).
pub(super) fn torque(input: &Inputs) -> Option<f32> {
    MODEL.with_borrow_mut(|m| m.as_mut().map(|m| m.torque(input)))
}
