//! The struts' typed state: what of a strut the step, the force update and the signal reset change (the controller's
//! mode, latch and signal, the effective damping and the fade), per wheel, and the three updates on a [`StrutStore`].
//! The definition is read where the car keeps it; the memory edge (`layout`) lifts this state from the image and
//! lowers it back.

use super::store::StrutStore;
use super::suspension::{Damping, Suspension};
use super::{Controller, DamperSignal, FadeState, NgpStep, StepInput, StepOutput};

/// A strut's changing state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Strut {
    pub controller: Controller,
    pub damping: Damping,
    pub fade: FadeState,
}

/// A strut by its slot in [`Struts`]: the index of its wheel on the car.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrutId(pub usize);

/// A car's struts, by wheel; `None` for a wheel without one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Struts(pub [Option<Strut>; 4]);

impl Struts {
    pub fn get(&self, id: StrutId) -> &Strut {
        self.0[id.0].as_ref().expect("a strut in its slot")
    }

    pub fn get_mut(&mut self, id: StrutId) -> &mut Strut {
        self.0[id.0].as_mut().expect("a strut in its slot")
    }
}

/// Recomputes the strut's three effective damping values from its definition and fade.
pub fn update_forces<S: StrutStore>(store: &mut S, strut: S::Strut) {
    let mut suspension = Suspension {
        definition: store.definition(strut),
        fade: store.fade(strut),
        damping: store.damping(strut),
        ..Default::default()
    };
    suspension.update_damping();
    store.store_damping(strut, suspension.damping);
}

/// Runs one spring-damper step on the strut, stores its changed state and returns its spring and stop force,
/// breakpoint offset and damping coefficient.
pub fn step<S: StrutStore>(store: &mut S, strut: S::Strut, input: StepInput, ngp: NgpStep) -> StepOutput {
    let mut controller = store.controller(strut);
    let mut fade = store.fade(strut);
    let mut state = store.damping(strut).state();
    let output = super::step(&mut controller, &mut fade, &store.definition(strut), &mut state, input, ngp);
    store.store_fade(strut, fade);
    store.store_damping(strut, Damping::from_state(state));
    store.store_step_state(strut, controller);
    output
}

/// Resets the strut's damper signal to its owner and returns the payload it held (the emit is the caller's).
pub fn reset_outputs<S: StrutStore>(store: &mut S, strut: S::Strut) -> DamperSignal {
    let mut suspension = Suspension { controller: store.controller(strut), ..Default::default() };
    let emitted = suspension.reset_signal(store.owner_id(strut));
    super::emit_signal(emitted);
    store.store_signal(strut, suspension.controller.signal);
    emitted
}
