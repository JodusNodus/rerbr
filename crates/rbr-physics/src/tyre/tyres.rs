//! The tyres' typed state: what of a tyre its functions change between their calls (the contact, the material copy,
//! the patch, the forces, the slips, the heat and the wear, the noise signal and the outputs), per wheel. The setup and
//! the wheel's contact angle are read where the car keeps them; the memory edge (`layout`) lifts this state from the
//! image and lowers it back.

use super::layout::{Scratch, StateWords};

/// A tyre's changing state: its scratch words (with the wear of its tread sectors) and its state words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tyre {
    pub scratch: Scratch,
    pub state: StateWords,
}

/// A tyre by its slot in [`Tyres`]: the index of its wheel on the car.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TyreId(pub usize);

/// A car's tyres, by wheel; `None` for a wheel without one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tyres(pub [Option<Tyre>; 4]);

impl Tyres {
    pub fn get(&self, id: TyreId) -> &Tyre {
        self.0[id.0].as_ref().expect("a tyre in its slot")
    }

    pub fn get_mut(&mut self, id: TyreId) -> &mut Tyre {
        self.0[id.0].as_mut().expect("a tyre in its slot")
    }
}
