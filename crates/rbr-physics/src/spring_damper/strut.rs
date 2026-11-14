//! The struts' typed state: what of a strut the step, the force update and the signal reset change (the controller's
//! mode, latch and signal, the effective damping and the fade), per wheel. The definition is read where the car keeps
//! it; the memory edge (`layout`) lifts this state from the image and lowers it back.

use super::suspension::Damping;
use super::{Controller, FadeState};

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
