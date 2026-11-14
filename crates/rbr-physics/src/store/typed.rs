//! The typed storage a step holds (B2, `notes/physics.md` "Store seam"): [`Typed`], the car's typed subsystems
//! ([`CarTyped`]) and the stage they run on. The car's are lifted from the image together and lowered together. The
//! struts and the tyres are borrowed by each of their runs (their runs do not nest); the VCU's state is in a cell and
//! the drive's and the engine's are borrowed per access, since their runs call each other through the host; the wheels' fields are
//! borrowed one at a time, since the wheel's runs nest. The stage is lifted once per loaded stage and never lowered (a
//! step only reads it).

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use crate::engine_driveline::layout::{CarDrive, CarEngine};
use crate::mem::Mem;
use crate::spring_damper::layout::CarStruts;
use crate::stage::typed::Stage;
use crate::tyre::layout::CarTyres;
use crate::vcu::layout::CarVcu;
use crate::wheel::layout::CarWheels;

/// What a step holds in typed storage: the car's state and the stage's.
#[derive(Clone, Debug, PartialEq)]
pub struct Typed {
    pub car: CarTyped,
    pub stage: Arc<Stage>,
}

impl Typed {
    /// Every owned byte range, named: the car's and the stage's.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut owned = self.car.owned();
        owned.extend(self.stage.owned());
        owned
    }
}

/// The typed state of the car at one address.
#[derive(Clone, Debug, PartialEq)]
pub struct CarTyped {
    pub struts: RefCell<CarStruts>,
    pub vcu: CarVcu,
    pub drive: CarDrive,
    pub engine: CarEngine,
    pub tyres: RefCell<CarTyres>,
    pub wheels: CarWheels,
}

impl CarTyped {
    /// The car at `car`'s typed state, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        CarTyped {
            struts: RefCell::new(CarStruts::lift(mem, car)),
            vcu: CarVcu::lift(mem, car),
            drive: CarDrive::lift(mem, car),
            engine: CarEngine::lift(mem, car),
            tyres: RefCell::new(CarTyres::lift(mem, car)),
            wheels: CarWheels::lift(mem, car),
        }
    }

    /// Writes every typed subsystem's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        self.struts.borrow().lower(mem);
        self.vcu.lower(mem);
        self.drive.lower(mem);
        self.engine.lower(mem);
        self.tyres.borrow().lower(mem);
        self.wheels.lower(mem);
    }

    /// Every owned byte range, named.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut owned = self.struts.borrow().owned();
        owned.extend(self.vcu.owned());
        owned.extend(self.drive.owned());
        owned.extend(self.engine.owned());
        owned.extend(self.tyres.borrow().owned());
        owned.extend(self.wheels.owned());
        owned
    }
}

/// `lower(lift(image))` of the car's typed state into an empty image, compared with `mem` on every owned byte, and the
/// stage's typed bytes against the image's: their count, or the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    Ok(crate::spring_damper::layout::round_trip(mem, car)?
        + crate::vcu::layout::round_trip(mem, car)?
        + crate::engine_driveline::layout::drive_round_trip(mem, car)?
        + crate::engine_driveline::layout::engine_round_trip(mem, car)?
        + crate::tyre::layout::round_trip(mem, car)?
        + crate::wheel::layout::round_trip(mem, car)?
        + crate::stage::layout::round_trip(mem)?)
}
