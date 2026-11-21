//! The typed storage a step holds (B2, `notes/physics.md` "Store seam"): [`Typed`], the car's typed subsystems
//! ([`CarTyped`]), the physics core's (one per world, lifted and lowered with the car's) and the stage they run on. The car's are lifted from the image together and lowered together. The
//! struts and the tyres are borrowed by each of their runs (their runs do not nest); the VCU's state is in a cell and
//! the drive's and the engine's are borrowed per access, since their runs call each other through the host; the wheels'
//! fields and the car's own are borrowed one at a time, since their runs nest. The stage is lifted once per loaded stage
//! and never lowered (a step only reads it).

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use crate::car_body::layout::CarOwn;
use crate::collision::layout::CarCollision;
use crate::engine_driveline::layout::{CarDrive, CarEngine};
use crate::mem::Mem;
use crate::physics_core::layout::Core;
use crate::rigid_body::layout::CarSolids;
use crate::spring_damper::layout::CarStruts;
use crate::stage::typed::Stage;
use crate::store::defs::Defs;
use crate::tyre::layout::CarTyres;
use crate::vcu::layout::CarVcu;
use crate::wheel::layout::CarWheels;

/// What a step holds in typed storage: the car's state, the physics core's, the stage's and the definitions the car's
/// subsystems read (lifted with the car's, read only: `defs`).
#[derive(Clone, Debug, PartialEq)]
pub struct Typed {
    pub car: CarTyped,
    pub core: Core,
    pub stage: Arc<Stage>,
    pub defs: Defs,
    /// The car's solids: every reader of a solid's words at its address runs on the solids' store (`SolidWords`).
    pub solids: CarSolids,
}

impl Typed {
    /// The car at `car`'s, its solids', the physics core's and the definitions' typed state, lifted from the image, on
    /// `stage`.
    pub fn lift(mem: &Mem, car: u32, stage: Arc<Stage>) -> Self {
        Typed {
            car: CarTyped::lift(mem, car),
            core: Core::lift(mem),
            stage,
            defs: Defs::lift(mem, car),
            solids: CarSolids::lift(mem, car),
        }
    }

    /// Writes the car's and the core's state back to the image (the stage's is never written).
    pub fn lower(&self, mem: &mut Mem) {
        self.car.lower(mem);
        self.core.lower(mem);
        self.solids.lower(mem);
    }

    /// Every owned byte range, named: the car's, the core's and the stage's.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut owned = self.car.owned();
        owned.extend(self.core.owned());
        owned.extend(self.solids.owned());
        owned.extend(self.stage.owned());
        owned
    }

    /// The byte ranges a step must not write, named: the held definitions'.
    pub fn watched(&self) -> Vec<(Range<u32>, String)> {
        self.defs.watched()
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
    pub own: CarOwn,
    pub collision: CarCollision,
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
            own: CarOwn::lift(mem, car),
            collision: CarCollision::lift(mem),
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
        self.own.lower(mem);
        self.collision.lower(mem);
    }

    /// Every owned byte range, named.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let mut owned = self.struts.borrow().owned();
        owned.extend(self.vcu.owned());
        owned.extend(self.drive.owned());
        owned.extend(self.engine.owned());
        owned.extend(self.tyres.borrow().owned());
        owned.extend(self.wheels.owned());
        owned.extend(self.own.owned());
        owned.extend(self.collision.owned());
        owned
    }
}
