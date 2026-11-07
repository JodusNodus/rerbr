//! The tyre model (ours, experimental, `rbr_tyre`) of the runs on this thread: with one set, NGP's tyre step takes the
//! contact patch's forces from it ([`crate::tyre`]); with none (the default) the step is NGP's.

use std::cell::RefCell;

use rbr_physics::tyre::model::Model as Forces;
use rbr_tyre::Model;

thread_local! {
    static MODEL: RefCell<Option<Box<dyn Model>>> = const { RefCell::new(None) };
}

/// The tyre model the runs on this thread step from now on (`None`: NGP's sliding model); returns the one it replaces.
/// The host sets a fresh one at each start and restart: its state is the model's own, which snapshots don't carry.
pub fn set_tyre_model(model: Option<Box<dyn Model>>) -> Option<Box<dyn Model>> {
    MODEL.replace(model)
}

/// The set model back to a new one's state (the stage's restart), its car's calibration kept.
pub fn restart_tyre_model() {
    MODEL.with_borrow_mut(|m| {
        if let Some(m) = m {
            m.restart();
        }
    });
}

/// `run` with the set model's forces for the tyre at `tyre` (`None` without one).
pub(crate) fn with_tyre_model<T>(tyre: u32, run: impl FnOnce(Option<Forces>) -> T) -> T {
    MODEL.with_borrow_mut(|m| match m {
        Some(m) => run(Some(&mut |x| m.forces(tyre, x))),
        None => run(None),
    })
}
