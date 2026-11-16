//! What the drivers read from the game image: the car, the application state, the time keeper and the physics clock.

use rbr_gameplay::timing::TimeKeeper;
use rbr_physics::mem::Mem;

use crate::timekeeper::layout as keeper_layout;
use rbr_physics::car_body::store::CarStore;

/// `CCar *` of the player's car.
pub const CAR_PTR: u32 = 0x8ef660;
/// The car block a recording frame holds.
pub const CAR_BYTES: usize = 0x1700;
/// The application root's (`g_1659b70`) time keeper, the one `CCar::Vf6cx` records into.
const TIME_KEEPER: u32 = 0x40;

/// `CApplication *` (`DAT_007eac48`), and its state (+0x728): the word `SetAppState` (0x47f370) stores and every
/// `CMultiSignal1` selects its connection slot by (the signal manager is built on its address).
const APPLICATION: u32 = 0x7eac48;
const APP_STATE: u32 = 0x728;
/// The application state of the load (`M004801d0` case 0xd), whose frames step the physics 40 substeps at a time.
pub(super) const LOAD_STATE: u32 = 0xd;

/// The application state in `mem`.
pub fn app_state(mem: &Mem) -> u32 {
    mem.u32(mem.u32(APPLICATION) + APP_STATE)
}

/// `SetAppState`'s store of `state` in `mem` (its `CSigAppState` emit reaches only the car sound, which is not run).
pub fn set_app_state(mem: &mut Mem, state: u32) {
    let at = mem.u32(APPLICATION) + APP_STATE;
    mem.set_u32(at, state);
}

/// The bound `CTimeKeeper` in `mem` (the keeper `Record` and `Update` run on), as the typed port reads it.
pub fn time_keeper(mem: &Mem) -> TimeKeeper {
    let keeper = mem.u32(mem.u32(keeper_layout::APP) + TIME_KEEPER);
    keeper_layout::load_keeper(mem, keeper)
}

/// The physics clock in `mem`: the state's double, which `Record` is given and `StepForward` advances.
pub fn clock(mem: &Mem) -> f64 {
    mem.timer_double()
}
