//! The placements (`CSigStuff`): `E46_Notify67d0`'s calls answered by the ports, the deferred state resets, the grid
//! sync and the frame walk.

use super::files::{NoParent, StageHost};
use super::meshes::{store_placement, PLACEMENT_SIZE};
use super::{WALL, WALL_ROOT};
use crate::collision::layout::body_hosts::BodyResetHost;
use crate::collision::layout::grid_hosts::AllocHost;
use crate::collision::layout::state::StateHost;
use crate::collision::layout::{cell_grid_ctor, state_unka44, sync_grid, CtorHost, GridSyncHost};
use crate::entities::frame::{entity_hook, Form, Hook, Owner};
use crate::entities::layout::{construct_entity, node_of, EntityArgs};
use crate::entities::store::EntityStore;
use crate::entities::transform::copy_blocks_and_walk_one_two;
use crate::entities::walk::{self, Node, Walk, Walker};
use crate::load::layout::body_event_listened;
use crate::mem::Mem;
use crate::physics_core::detect::Xy;
use crate::physics_core::layout::construct::Heap;
use crate::physics_core::layout::detect_calls::GridBuildHost;
use crate::physics_core::layout::Filing;
use crate::physics_core::layout::{self as physics, FilingHost, NotifyHost};
use crate::rigid_body::emit::{window_start, BodyEvent};
use crate::rigid_body::host::{HostUnavailable, SolidCtorHost};
use crate::rigid_body::layout::{
    construct_solid, new_solid, place_solid, refresh_and_reset, scale_add_rows, solid_filing, solid_sleep, solid_wake,
    Body, PlaceHost, RestHost,
};
use crate::rigid_body::rotation::{self, MulHost};
use crate::stage::Placement;

/// The cell grid object (`CCellGrid`, 0x14 bytes).
const GRID_SIZE: u32 = 0x14;

/// The calls the stage's objects make, answered by the ports. `E45_MD60`'s: `CBrakeWallManager::QueryWall` (0x4e4140)
/// on the manager, `operator new(0x14)` and `CCellGrid::CCellGrid` (0x4e0550).
pub(super) struct Calls<'a, H>(pub(super) &'a mut H);

impl<H: StageHost> GridBuildHost for Calls<'_, H> {
    fn query_wall(&mut self, mem: &mut Mem, manager: u32) -> Option<[Xy; 2]> {
        let wall = mem.u32(manager + WALL);
        (wall != 0).then(|| crate::stage::wall_corners(mem.f32s(wall + WALL_ROOT)))
    }

    fn alloc_grid(&mut self, mem: &mut Mem) -> u32 {
        self.0.alloc(mem, GRID_SIZE)
    }

    fn construct_grid(&mut self, mem: &mut Mem, object: u32, min: Xy, width: f32, height: f32) -> u32 {
        cell_grid_ctor(self.0, mem, object, min.x, min.y, width, height)
    }
}

/// `CPhysics::E46_Notify67d0` (0x5267d0, `physics_core::layout::notify_placement`) on a placement the stage load emits,
/// stored in the host's scratch: its object is a `CSolid` (`CSolid::CDef::Vf50`), placed (`CSolid::Vf44`), filed with
/// the state and put to sleep (`CSolid::E43Vf94`; a placement with flag 1 is woken, `CSolid::E43Vf90`). The objects'
/// virtual calls are answered for `CSolid` only; another class, a parent, a dynamic placement or a listener of the body
/// event that is not ported is `Err`.
pub fn place(mem: &mut Mem, host: &mut impl StageHost, placement: &Placement) -> Result<(), HostUnavailable> {
    let at = host.scratch(mem, PLACEMENT_SIZE);
    store_placement(mem, at, placement);
    physics::notify_placement(mem, &mut Calls(host), at)
}

/// The virtual slots the placements' objects are checked for, with the function each must hold: the new solid
/// (`CSolid::CDef::Vf50`, 0x52d640) on the definition; on the object the place (`CSolid::Vf44`), the refresh
/// (`CSolid::Vf40`), the frame slot 0x30 (`node_of`), the filing slots (`solid_filing`), the wake (`CSolid::E43Vf90`)
/// with its window start (`CSolid::S18`, 0x529dc0, named by use: its port ends in `emit::window_start`), and the sleep
/// (`CSolid::E43Vf94`); an empty function (`CBase::Unk44`, `ret`) in its state's (`+0x38`) slot 0x44; the owner's hook
/// One (`CSolid::E43Vf9C`, 0x52ac80, `scale_add_rows`).
const SOLID_DEF_NEW: (u32, u32) = (0x50, 0x0052_D640);
const SOLID_PLACE: (u32, u32) = (0x44, 0x0052_C700);
const SOLID_REFRESH: (u32, u32) = (0x40, 0x0052_B6D0);
const NODE_OF: (u32, u32) = (0x30, 0x0051_9C00);
const SOLID_WAKE: (u32, u32) = (0x90, 0x0052_BF30);
const SOLID_WINDOW_START: (u32, u32) = (0x48, 0x0052_9DC0);
const SOLID_SLEEP: (u32, u32) = (0x94, 0x0052_C130);
const STATE_EMPTY_SLOT: (u32, u32) = (0x44, 0x0052_F480);
const SOLID_HOOK_ONE: (u32, u32) = (0x9C, 0x0052_AC80);
/// `CEntityFrame::Vf04` (0x519530), the first of the six frame hooks (slots 1 to 6, 0x30 bytes apart).
pub(crate) const FRAME_HOOKS: u32 = 0x0051_9530;
/// A solid's state and its first child.
const ENTITY_STATE: u32 = 0x38;
const FIRST_CHILD: u32 = 0x48;

/// Whether slot `slot` of `object`'s vtable holds `function`.
fn holds(mem: &Mem, object: u32, (slot, function): (u32, u32)) -> bool {
    mem.u32(mem.u32(object) + slot) == function
}

/// `Err` unless `ok`: the object is not one whose calls are ported here.
fn check(ok: bool) -> Result<(), HostUnavailable> {
    ok.then_some(()).ok_or(HostUnavailable)
}

impl<H: StageHost> Heap for Calls<'_, H> {
    fn alloc(&mut self, mem: &mut Mem, size: u32) -> u32 {
        self.0.alloc(mem, size)
    }
}

impl<H: StageHost> AllocHost for Calls<'_, H> {
    fn allocate(&mut self, bytes: u32) -> u32 {
        self.0.allocate(bytes)
    }
}

impl<H: StageHost> NotifyHost for Calls<'_, H> {
    /// A stage object has no parent; its definition must be a `CSolid::CDef`.
    fn def_new(&mut self, mem: &mut Mem, def: u32, a: u32, b: u32) -> Result<u32, HostUnavailable> {
        check(b == 0 && holds(mem, def, SOLID_DEF_NEW))?;
        new_solid(mem, def, a, b, self)
    }

    fn place(&mut self, mem: &mut Mem, object: u32, stuff: u32) -> Result<(), HostUnavailable> {
        check(holds(mem, object, SOLID_PLACE))?;
        place_solid(mem, self, object, stuff)
    }

    fn adopt(&mut self, _: &mut Mem, _: u32, _: u32) -> Result<(), HostUnavailable> {
        Err(HostUnavailable)
    }

    fn wake(&mut self, mem: &mut Mem, object: u32) -> Result<(), HostUnavailable> {
        check(holds(mem, object, SOLID_WAKE))?;
        solid_wake(mem, self, object)
    }

    fn sleep(&mut self, mem: &mut Mem, object: u32) -> Result<(), HostUnavailable> {
        check(holds(mem, object, SOLID_SLEEP))?;
        solid_sleep(mem, self, object)
    }
}

impl<H> FilingHost for Calls<'_, H> {
    fn filing(&mut self, mem: &Mem, object: u32) -> Result<Filing, HostUnavailable> {
        solid_filing(mem, object)
    }
}

/// `CSolid::CSolid` (0x52d480) under `CSolid::CDef::Vf50`.
impl<H> SolidCtorHost for Calls<'_, H> {
    fn solid_ctor(
        &mut self,
        mem: &mut Mem,
        solid: u32,
        def: u32,
        a: u32,
        b: u32,
        flags: u32,
    ) -> Result<(), HostUnavailable> {
        construct_solid(mem, self, solid, def, a, b, flags);
        Ok(())
    }
}

/// `CEntity::CEntity` (0x51a640) under `CBody::CBody`, for an entity with no parent.
impl<H> CtorHost for Calls<'_, H> {
    fn entity_ctor(&mut self, mem: &mut Mem, body: u32, definition: u32, frame: u32, parent: u32, flags: u32) -> u32 {
        let args = EntityArgs { definition, sort_key: frame, parent, options: flags };
        construct_entity(mem, body, &args, &mut NoParent)
    }
}

impl<H: StageHost> PlaceHost for Calls<'_, H> {
    /// `CSolid::Vf40` (0x52b6d0, `refresh_and_reset`): `CBody::Vf40`'s two calls end it, and nothing reads what they write
    /// before them, so `Deferred` keeps them and they are made here after it, in its order: `CBody::CState::Unk44`
    /// (`state_unka44`, with its last call, `CColGrid::SyncGrid` on the body's grid, made after it the same way), then
    /// `CEntity::Vf40` (0x51a4b0): the state's slot 0x44 (`CBase::Unk44`, nothing) and the walk of the children (a placed
    /// solid has none).
    fn refresh(&mut self, mem: &mut Mem, this: u32) -> Result<(), HostUnavailable> {
        check(holds(mem, this, SOLID_REFRESH))?;
        let mut deferred = Deferred::default();
        refresh_and_reset(mem, this, &mut deferred)?;
        let (Some(state), Some(body)) = (deferred.state, deferred.body) else {
            return Err(HostUnavailable);
        };
        check(holds(mem, this, NODE_OF))?;
        let mut sync = Deferred::default();
        state_unka44(&mut sync, mem, state);
        if let Some((grid, a, low, high)) = sync.sync {
            let scratch = self.0.scratch(mem, 16 * u32::from(mem.u8(grid + GRID_EDGES)));
            let mut walk = GridSync { failed: false };
            sync_grid(&mut walk, mem, grid, a, low, high, scratch);
            check(!walk.failed)?;
        }
        check(holds(mem, body + ENTITY_STATE, STATE_EMPTY_SLOT))?;
        check(mem.u32(body + FIRST_CHILD) == 0)
    }
}

/// A collision grid's edge-vector count (`+0x68d`, the vectors from `+0x40`).
const GRID_EDGES: u32 = 0x68D;

/// The calls `refresh_and_reset` and `state_unka44` end with, kept to be made after them: `CBody::Vf40`'s state and body, and
/// `SyncGrid`'s grid and arguments.
#[derive(Default)]
struct Deferred {
    state: Option<u32>,
    body: Option<u32>,
    sync: Option<(u32, u32, u32, u32)>,
}

/// `E46Mat::Mul` (0x528c20, `Mat::mul_rows`): the rows of `b` times `m`. A placed solid's state gates it with the
/// extent's fourth word, which `CSolid::Vf44` sets unless the extent is (1, 1, 1).
impl MulHost for Deferred {
    fn mul(&mut self, m: [f32; 9], b: [f32; 9]) -> Result<[f32; 9], HostUnavailable> {
        Ok(rotation::mul_rows(m, b))
    }
}

impl BodyResetHost for Deferred {
    fn reset_state(&mut self, state: u32) -> u32 {
        self.state = Some(state);
        0
    }

    fn reset_entity(&mut self, body: u32) -> u32 {
        self.body = Some(body);
        0
    }
}

/// The state's owner slot 0x30 is `node_of` (checked by the caller).
impl StateHost for Deferred {
    fn owner_vf30(&mut self, owner: u32) -> u32 {
        node_of(owner)
    }

    fn wall_sync(&mut self, wall: u32, body: u32, vec_a: u32, vec_b: u32) -> u32 {
        self.sync = Some((wall, body, vec_a, vec_b));
        0
    }
}

/// `SyncGrid`'s calls: `E37E43::M_B460` (`copy_blocks_and_walk_one_two`) with the walk on game memory, and the vector copy.
struct GridSync {
    failed: bool,
}

impl GridSyncHost for GridSync {
    fn copy_transform(&mut self, mem: &mut Mem, a: u32, edges: u32, worker: u32, out: u32, count: u32) {
        let mut walk = FrameWalk { mem, failed: false };
        copy_blocks_and_walk_one_two(&mut walk, a, worker, edges, out, count);
        self.failed |= walk.failed;
    }

    fn copy_vec4(&mut self, mem: &mut Mem, dst: u32, src: u32) {
        mem.copy(dst, src, 0x10);
    }
}

/// The pair walk on game memory, for a solid's frame under the root: the frames' hooks are `CEntityFrame`'s, which call
/// the owner's; a solid's hook One is `scale_add_rows`. Any other hook sets `failed`.
struct FrameWalk<'m> {
    mem: &'m mut Mem,
    failed: bool,
}

impl Walk for FrameWalk<'_> {
    fn node(&self, at: u32) -> Node {
        EntityStore::load_node(self.mem, at)
    }

    fn hook(&mut self, at: u32, hook: Hook, a: u32, b: u32) {
        let index = hook as u32;
        if !holds(self.mem, at, (4 * (index + 1), FRAME_HOOKS + 0x30 * index)) {
            self.failed = true;
            return;
        }
        let frame = EntityStore::load_entity_frame(self.mem, at);
        entity_hook(&frame, hook, a, b, self);
    }

    fn walk(&mut self, which: Walker, receiver: u32, o2: u32, a: u32, b: u32) -> u32 {
        walk::run(self, which, receiver, o2, a, b)
    }

    fn grid_count(&self, grid: u32) -> u32 {
        EntityStore::grid_count(self.mem, grid)
    }

    fn copy16(&mut self, to: u32, from: u32) {
        self.mem.copy(to, from, 0x10);
    }
}

/// The owner's side of a frame hook.
impl Owner for FrameWalk<'_> {
    fn hook(&mut self, owner: u32, hook: Hook, form: Form, a: u32, b: u32) -> u32 {
        if hook == Hook::One && form == Form::X && holds(self.mem, owner, SOLID_HOOK_ONE) {
            scale_add_rows(self.mem, owner, a, b);
        } else {
            self.failed = true;
        }
        a
    }
    fn gate(&mut self, _: u32) -> u32 {
        unreachable!("an entity frame's hook asks no gate")
    }
    fn frame_of(&mut self, _: u32) -> u32 {
        unreachable!("a frame hook asks no entity for its frame")
    }
    fn old_frame_of(&mut self, _: u32) -> u32 {
        unreachable!("a frame hook asks no entity for its old frame")
    }
}

/// `CSolid::E43Vf90`'s and `E43Vf94`'s calls: the body's slot 18 and the body event.
impl<H> RestHost for Calls<'_, H> {
    /// `CSolid::S18` (0x529dc0): the walk of the children (a placed solid has none), then `emit::window_start`.
    fn start_window(&mut self, mem: &mut Mem, this: u32) -> Result<(), HostUnavailable> {
        check(holds(mem, this, SOLID_WINDOW_START) && mem.u32(this + FIRST_CHILD) == 0)?;
        let mut body = Body::load(mem, this);
        window_start(&mut body);
        body.store(mem, this);
        Ok(())
    }

    fn emit(&mut self, mem: &Mem, _: &BodyEvent) -> Result<(), HostUnavailable> {
        check(!body_event_listened(mem))
    }
}
