//! Load-time setup of the physics core (`src/rbr/RBR_E46_Small.cpp`, group D of the README). These run once, not per step.

use super::construct::{Heap, PhysicsDefHost};
use crate::mem::Mem;
use crate::physics_core::layout;

/// `CPhysics::CDef`'s size, the block `E46_CreateDef` allocates.
pub const DEF_SIZE: u32 = 0x16160;

/// The calls `E46_CreateDef` makes into code outside this port, after the def's own constructor.
pub trait CreateHost: Heap + PhysicsDefHost {
    /// The def's slot 2 (0x52f0c0), the `StepForward` the original casts the def to. It initialises the def's
    /// sub-objects, so the caller runs it.
    fn step_forward(&mut self, mem: &mut Mem, def: u32, arg: u32);
    /// `E45_PhysUnk34` (0x523a20) on the def; returns its result.
    fn def_result(&mut self, mem: &mut Mem, def: u32) -> u32;
}

/// The calls `E46_InitPhysics` makes into the script reader (`NTweak`), which the caller runs.
pub trait TokenHost {
    /// `NTweak::StripComments` (0x52eba0): blanks the comments of `script`.
    fn strip_comments(&mut self, mem: &mut Mem, script: u32);
    /// `NTweak::CTokenCursor::Tokenize` (0x52ec80) on the cursor at `cursor`, over `script`.
    fn tokenize(&mut self, mem: &mut Mem, cursor: u32, script: u32);
    /// `NTweak::CTokenCursor::FreeTokens` (0x52ee10) on the cursor at `cursor`.
    fn free_tokens(&mut self, mem: &mut Mem, cursor: u32);
}

/// The calls `E46_AppInit` makes into other subsystems, which the caller runs.
pub trait AppHost {
    /// `CBrakeWallManager::ClearWall` (0x4e40d0) on the wall manager at `wall_manager`.
    fn clear_walls(&mut self, mem: &mut Mem, wall_manager: u32);
    /// `E38_004FAD10` (0x4fad10) with the value of the collision global.
    fn collision_clear(&mut self, mem: &mut Mem, value: u32);
    /// `E39WaterTable::Reset` (0x4ffba0) on the water table at `table`.
    fn water_reset(&mut self, mem: &mut Mem, table: u32);
    /// `E09Vec4::Clear` (0x465d80) on the vector at `at`.
    fn vec4_clear(&mut self, mem: &mut Mem, at: u32);
    /// `NTweak::InitTokenScratch` (0x52eb20).
    fn init_token_scratch(&mut self, mem: &mut Mem);
    /// `E37SigInit800` (0x4fa800), the last call (a tail call in the original).
    fn sig_init(&mut self, mem: &mut Mem);
}

/// The calls `E43HostRel::WallRel` makes into other code, which the caller runs.
pub trait WallHost {
    /// `E34Link10::UnlinkA` (0x4e41a0) on the relay's link at `link`.
    fn unlink(&mut self, mem: &mut Mem, link: u32);
    /// `CPhysics::CState::RegWall` (0x5263d0) on the state at `state`, with the relay as the wall.
    fn register(&mut self, mem: &mut Mem, state: u32, wall: u32);
}

/// `E43HostRel::WallRel` (0x5276a0): with a target list, lowers its count, unlinks the relay and registers the relay
/// as a wall with the physics' state. `physics` is the host (`this`), `relay` the wall (the argument).
pub fn relink_wall(mem: &mut Mem, host: &mut impl WallHost, physics: u32, relay: u32) {
    let target = layout::relay_target(mem, relay);
    if target == 0 {
        return;
    }
    layout::release_target(mem, target);
    host.unlink(mem, layout::relay_link(relay));
    let state = layout::state_of(mem, physics);
    host.register(mem, state, relay);
}
