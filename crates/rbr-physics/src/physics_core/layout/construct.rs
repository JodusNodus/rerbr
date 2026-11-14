//! The objects of the physics core as their constructors make them (`src/rbr/RBR_E46_CDef.cpp`, `RBR_E46_Physics.cpp`).
//! A constructor writes only the words its body sets: the typed image below has those fields, and `layout` stores
//! them and nothing else, so the other bytes of the object keep what they had. Vtable and name words are the
//! addresses the original stores (`StateImage`'s own vtable and the NTweak ones); the names are the constants' meaning
//! where known and descriptive otherwise (see README "Unknown meaning").

use crate::mem::Mem;
pub use crate::ntweak::layout::{REAL_NAME, REAL_VTABLE, UNSIGNED_NAME, UNSIGNED_VTABLE};
use crate::ntweak::layout::{VECTOR_NAME, VECTOR_VTABLE};
use crate::physics_core::layout;

/// `NTweak::CBase` header, the first 0x0c bytes of a tweak object: vtable, name pointer, dirty flag.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TweakHeader {
    pub vtable: u32,
    pub name: u32,
    pub dirty: u32,
}

/// `NTweak::CUnsigned`: the header, then its value (+0x0c of the object).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TweakUnsigned {
    pub header: TweakHeader,
    pub value: u32,
}

/// `NTweak::CObjIdQueue` head words the `StateImage` constructor sets (+0x28, +0x2c, +0x30 of the state): its vtable, its
/// name and a third word (zero).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObjIdQueueHead {
    pub vtable: u32,
    pub name: u32,
    pub third_word: u32,
}

/// `E34Drain` list head: head node, tail node (`push_item` appends there), count, flag (0x10 bytes, all zero when a
/// state is made).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrainList {
    pub head: u32,
    pub tail: u32,
    pub count: u32,
    pub flag: u32,
}

/// The constructor's image of `CPhysics::StateImage` (0x4b8 bytes): the words `CState::StateImage` writes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateImage {
    /// `CPhysics::StateImage` vtable (`0x73f884`) and the `"Physics"` name, at +0x00.
    pub base: TweakHeader,
    /// Substep counter (`m_u0c`), +0x0c.
    pub substep_count: TweakUnsigned,
    /// Simulation time, seeded with 43185.0, +0x20.
    pub sim_time: f64,
    /// Object id queue, +0x28.
    pub queue: ObjIdQueueHead,
    /// `m_u440`, +0x440 (an unsigned tweak; meaning not known).
    pub first_unsigned: TweakUnsigned,
    /// The four walls lists, +0x450, +0x460, +0x470, +0x480.
    pub lists: [DrainList; 4],
    /// `m_r490`, a `CReal` header, +0x490. Its value (+0x49c) is not written by the constructor.
    pub real_header: TweakHeader,
    /// `m_u4a0`, +0x4a0 (an unsigned tweak; meaning not known).
    pub second_unsigned: TweakUnsigned,
    /// The constructor's argument, +0x4b0.
    pub extra: u32,
}

/// The `"Physics"` base name of `StateImage`'s `CBase`.
const PHYSICS_NAME: u32 = 0x0073_f7b8;
/// `StateImage`'s vtable.
const STATE_VTABLE: u32 = 0x0073_f884;
/// `CObjIdQueue`'s vtable and its second word (its name, as the CBase ones are).
const OBJ_ID_QUEUE_VTABLE: u32 = 0x0074_04dc;
const OBJ_ID_QUEUE_NAME: u32 = 0x0073_e920;
/// The simulation time's seed (`StateImage` +0x20), the double at 0x73f878.
const SIM_TIME_SEED: f64 = 43185.0;

/// `CPhysics::CState::StateImage(extra)` (0x5270c0): the image the constructor leaves.
pub fn new_state(extra: u32) -> StateImage {
    let unsigned =
        || TweakUnsigned { header: TweakHeader { vtable: UNSIGNED_VTABLE, name: UNSIGNED_NAME, dirty: 0 }, value: 0 };
    StateImage {
        base: TweakHeader { vtable: STATE_VTABLE, name: PHYSICS_NAME, dirty: 0 },
        substep_count: unsigned(),
        sim_time: SIM_TIME_SEED,
        queue: ObjIdQueueHead { vtable: OBJ_ID_QUEUE_VTABLE, name: OBJ_ID_QUEUE_NAME, third_word: 0 },
        first_unsigned: unsigned(),
        lists: [DrainList::default(); 4],
        real_header: TweakHeader { vtable: REAL_VTABLE, name: REAL_NAME, dirty: 0 },
        second_unsigned: unsigned(),
        extra,
    }
}

/// `CPhysics::CScratch`'s vtable, at +0x00.
const SCRATCH_VTABLE: u32 = 0x0073_f794;
/// The solver a scratch holds at +0x10: a `CCollisionSolver` of `SOLVER_SIZE` bytes, built by its constructor.
pub const SOLVER_AT: u32 = 0x10;
pub const SOLVER_SIZE: u32 = 0xc440;

/// The calls `CPhysics::CScratch` makes into code outside this port. The caller runs them against the game memory
/// (the solver's bytes are theirs to write, so the port neither writes nor checks them).
pub trait ScratchHost {
    /// `CCollisionSolver::CtorSolver(def)` (E38, 0x4fc930) on the solver at `solver`.
    fn ctor_solver(&mut self, mem: &mut Mem, solver: u32, def: u32);
    /// The solver's slot 2 (0x4fc0b0, the `S08` of `E45S08`), with the solver as `this`.
    fn solver_post(&mut self, mem: &mut Mem, solver: u32);
}

/// `CPhysics::CScratch::CScratch(solverDef)` (0x524e80): the vtable, the solver's constructor, then `Vf04`.
pub fn construct_scratch(mem: &mut Mem, host: &mut impl ScratchHost, this: u32, solver_def: u32) {
    layout::set_scratch_vtable(mem, this, SCRATCH_VTABLE);
    host.ctor_solver(mem, this + SOLVER_AT, solver_def);
    reset_scratch(mem, host, this);
}

/// `CPhysics::CScratch::Vf04` (0x523c50): the solver's `S08`, then the block at +0xe840 is cleared.
pub fn reset_scratch(mem: &mut Mem, host: &mut impl ScratchHost, this: u32) {
    host.solver_post(mem, this + SOLVER_AT);
    layout::clear_scratch_block(mem, this);
}

/// `CPhysics::CScratch`'s size, the block `CDef::Vf54` allocates.
pub const SCRATCH_SIZE: u32 = 0xe890;
/// The solver definition a `CDef` holds at +0x16024 (`m_solverDef`), the argument of the scratch's solver.
const SOLVER_DEF_AT: u32 = 0x0001_6024;

/// The allocation `CPhysics::CDef::Vf54` makes (the CRT's `operator new`): the caller returns the block, or 0 when
/// there is none.
pub trait Heap {
    fn alloc(&mut self, mem: &mut Mem, size: u32) -> u32;
}

/// `CPhysics::CDef::Vf54(owner)` (0x526760): a scratch of `SCRATCH_SIZE` bytes, built on the solver definition at
/// `def + SOLVER_DEF_AT`. Returns the scratch, or 0 when the allocation fails. The original does not use `owner`.
pub fn def_new_scratch(mem: &mut Mem, host: &mut (impl Heap + ScratchHost), def: u32) -> u32 {
    let at = host.alloc(mem, SCRATCH_SIZE);
    if at != 0 {
        construct_scratch(mem, host, at, def + SOLVER_DEF_AT);
    }
    at
}

/// `CPhysics::StateImage`'s size, the block `CDef::Vf50` allocates.
pub const STATE_SIZE: u32 = 0x4b8;

/// `CPhysics::CDef::Vf50(owner, name)` (0x5278d0): a state of `STATE_SIZE` bytes with `extra` as its argument. Returns
/// the state, or 0 when the allocation fails. The name is not used by the state.
pub fn def_new_state(mem: &mut Mem, host: &mut impl Heap, extra: u32) -> u32 {
    let at = host.alloc(mem, STATE_SIZE);
    if at != 0 {
        layout::store_state(mem, at, &new_state(extra));
    }
    at
}

/// `CPhysics::CDef`'s vtable, at +0x00.
const DEF_VTABLE: u32 = 0x0073_f72c;
/// The `CFlags` and `CDbgFlags` storages' vtables, and the `CLogFlags` one (`LOG_FLAGS_VTABLE`).
pub const FLAGS_VTABLE: u32 = 0x0073_f60c;
pub const DBG_FLAGS_VTABLE: u32 = 0x0073_f66c;
pub const LOG_FLAGS_VTABLE: u32 = 0x0073_f6cc;
/// The name of the flag storages (0x14 bytes each: a vtable, a name, then zeros).
pub const FLAGS_NAME: u32 = 0x0074_04ac;

/// `NTweak::CReal`: the header, then its value (+0x0c of the object).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TweakReal {
    pub header: TweakHeader,
    pub value: f32,
}

/// `NTweak::CVector`: the header, then four floats at +0x10 (the word at +0x0c is not written by the constructor).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TweakVector {
    pub header: TweakHeader,
    pub value: [f32; 4],
}

/// A flag storage of 0x14 bytes (`CFlags`, `CDbgFlags`, `CLogFlags`): its vtable and name, then three zero words.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Storage {
    pub vtable: u32,
    pub name: u32,
}

/// The words `CPhysics::CDef::CDef` writes on its own (its base, its tweaks and flag storages). The def manager at
/// +0x0c, the material manager at +0x18 and the solver's def at +0x16024 are built by their own constructors. The
/// fields are named after the tweak keys at their offsets (`data/physics-lsp-fields.md`: `MaxExpirableCount` at
/// +0x16070 to `SingleStepperDelay` at +0x16150; `flags`, `dbg` and `log` are `Flags`, `DbgFlags` and `LogFlags`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsDefImage {
    pub base: TweakHeader,
    pub max_expirable_count: TweakUnsigned,
    pub max_time_step: TweakReal,
    pub min_time_delta: TweakReal,
    pub gravity: TweakVector,
    pub flags: Storage,
    pub dbg: Storage,
    pub log: Storage,
    pub brake_wall_buffer: TweakReal,
    pub brake_wall_factor: TweakReal,
    pub brake_wall_damping: TweakReal,
    pub damage_zone: TweakUnsigned,
    pub force_material: TweakUnsigned,
    pub single_stepper_delay: TweakUnsigned,
}

/// The `CDef` image the constructor leaves on its own (every tweak at its default, value zero).
pub fn physics_def_image() -> PhysicsDefImage {
    let real = || TweakReal { header: TweakHeader { vtable: REAL_VTABLE, name: REAL_NAME, dirty: 0 }, value: 0.0 };
    let unsigned =
        || TweakUnsigned { header: TweakHeader { vtable: UNSIGNED_VTABLE, name: UNSIGNED_NAME, dirty: 0 }, value: 0 };
    let storage = |vtable| Storage { vtable, name: FLAGS_NAME };
    PhysicsDefImage {
        base: TweakHeader { vtable: DEF_VTABLE, name: PHYSICS_NAME, dirty: 0 },
        max_expirable_count: unsigned(),
        max_time_step: real(),
        min_time_delta: real(),
        gravity: TweakVector {
            header: TweakHeader { vtable: VECTOR_VTABLE, name: VECTOR_NAME, dirty: 0 },
            value: [0.0; 4],
        },
        flags: storage(FLAGS_VTABLE),
        dbg: storage(DBG_FLAGS_VTABLE),
        log: storage(LOG_FLAGS_VTABLE),
        brake_wall_buffer: real(),
        brake_wall_factor: real(),
        brake_wall_damping: real(),
        damage_zone: unsigned(),
        force_material: unsigned(),
        single_stepper_delay: unsigned(),
    }
}

/// The calls `CPhysics::CDef::CDef` makes into code outside this port (its members' constructors in the body). Each
/// gets the address of the object it builds.
pub trait PhysicsDefHost {
    /// `E39_DefMgrCtor` (0x501d90) on the def manager at `at` (+0x0c).
    fn def_manager(&mut self, mem: &mut Mem, at: u32);
    /// `CMaterialManager::CMaterialManager` (0x520b30) on the material manager at `at` (+0x18).
    fn materials(&mut self, mem: &mut Mem, at: u32);
    /// `CCollisionSolver::CDef::CDef` (0x4fb350) on the solver's def at `at` (+0x16024).
    fn solver_def(&mut self, mem: &mut Mem, at: u32);
}

/// `CPhysics::CDef::CDef` (0x524c70) on the object at `this`: its own words first, then the three built objects, in
/// the original's order.
pub fn construct_physics_def(mem: &mut Mem, host: &mut impl PhysicsDefHost, this: u32) {
    layout::store_physics_def(mem, this, &physics_def_image());
    host.def_manager(mem, this + 0xc);
    host.materials(mem, this + 0x18);
    host.solver_def(mem, this + 0x16024);
}

/// `CDefManager`'s vtable (0x739e44) and name (`"DefManager"`, 0x739e90).
const DEF_MANAGER_VTABLE: u32 = 0x0073_9e44;
const DEF_MANAGER_NAME: u32 = 0x0073_9e90;
/// `CCollisionSolver::CDef`'s vtable (0x7398f4) and name (`"CCollisionSolver"`, 0x739940).
const SOLVER_DEF_VTABLE: u32 = 0x0073_98f4;
const SOLVER_DEF_NAME: u32 = 0x0073_9940;

/// The words `CCollisionSolver::CDef::CDef` writes: its header, `Restitution`, `HyperRestitution` and
/// `MinInvEffRedMass`, then `nSteps`, every value zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolverDefImage {
    pub base: TweakHeader,
    pub reals: [TweakReal; 3],
    pub steps: TweakUnsigned,
}

/// The header `E39_DefMgrCtor` (0x501d90) writes.
pub fn def_manager_image() -> TweakHeader {
    TweakHeader { vtable: DEF_MANAGER_VTABLE, name: DEF_MANAGER_NAME, dirty: 0 }
}

/// The image `CCollisionSolver::CDef::CDef` (0x4fb350) writes.
pub fn solver_def_image() -> SolverDefImage {
    let real = TweakReal { header: TweakHeader { vtable: REAL_VTABLE, name: REAL_NAME, dirty: 0 }, value: 0.0 };
    SolverDefImage {
        base: TweakHeader { vtable: SOLVER_DEF_VTABLE, name: SOLVER_DEF_NAME, dirty: 0 },
        reals: [real; 3],
        steps: TweakUnsigned {
            header: TweakHeader { vtable: UNSIGNED_VTABLE, name: UNSIGNED_NAME, dirty: 0 },
            value: 0,
        },
    }
}
