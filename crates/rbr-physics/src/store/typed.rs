//! The car's typed storage (B2, `notes/physics.md` "Store seam"): the subsystems whose state is typed, lifted from the
//! image together and lowered together. The struts and the tyres are borrowed by each of their runs (their runs do not
//! nest); the VCU's state is in a cell, since its runs call each other through the host.

use std::cell::RefCell;
use std::ops::Range;

use crate::mem::Mem;
use crate::spring_damper::layout::CarStruts;
use crate::tyre::layout::CarTyres;
use crate::vcu::layout::CarVcu;

/// The typed state of the car at one address.
#[derive(Clone, Debug, PartialEq)]
pub struct CarTyped {
    pub struts: RefCell<CarStruts>,
    pub vcu: CarVcu,
    pub tyres: RefCell<CarTyres>,
}

impl CarTyped {
    /// The car at `car`'s typed state, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        CarTyped {
            struts: RefCell::new(CarStruts::lift(mem, car)),
            vcu: CarVcu::lift(mem, car),
            tyres: RefCell::new(CarTyres::lift(mem, car)),
        }
    }

    /// Writes every typed subsystem's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        self.struts.borrow().lower(mem);
        self.vcu.lower(mem);
        self.tyres.borrow().lower(mem);
    }

    /// Every owned byte range, named.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut owned = self.struts.borrow().owned();
        owned.extend(self.vcu.owned());
        owned.extend(self.tyres.borrow().owned());
        owned
    }
}

/// `lower(lift(image))` of the car's typed state into an empty image, compared with `mem` on every owned byte: their
/// count, or the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    Ok(crate::spring_damper::layout::round_trip(mem, car)?
        + crate::vcu::layout::round_trip(mem, car)?
        + crate::tyre::layout::round_trip(mem, car)?)
}
