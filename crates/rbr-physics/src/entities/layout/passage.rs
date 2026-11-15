//! What a frame's hook does to rows carried through it (`entities::carry`), read from the frame's and its owner's
//! vtables: the `CEntityFrame` and `CEntityOldFrame` hooks forward to the owner's slots 0x9c..0xc8, `CSolid`'s row
//! loops and `CEntity`'s echoes.

use std::ops::Range;

use crate::entities::carry::Passage;
use crate::entities::frame::{Form, Hook};
use crate::entities::store::EntityStore;
use crate::physics_core::layout::{takes_part_word, vtable_target};
use crate::rigid_body::layout::{SolidWords, START_ORIENTATION, START_POSITION};
use crate::rigid_body::rows::RowKind;
use crate::rigid_body::solid_rows::row_map;
use crate::rigid_body::store::RigidStore;

/// `CEntityFrame::Vf04..Vf14` (hooks One to Five) and `CEntityOldFrame::Vf04..Vf10` (One to Four).
const FRAME_HOOKS: [u32; 5] = [0x519530, 0x519560, 0x519590, 0x5195c0, 0x5195f0];
const OLD_FRAME_HOOKS: [u32; 4] = [0x5196d0, 0x519720, 0x519770, 0x5197c0];
/// `E43PairWalk::MAB10..MAB40`, the world frame's hooks: they return their first argument.
const ROOT_HOOKS: Range<u32> = 0x51ab10..0x51ab41;
/// The owner's gate (slot 0x80): `CSolid::E43Vf80` (its takes-part word) and `CEntity::E43Vf80` (0).
const GATE_SLOT: u32 = 0x80;
const SOLID_GATE: u32 = 0x5293d0;
const ENTITY_GATE: u32 = 0x519a20;
/// `CEntity::E43Vf9C`, `E43VfA0`, `E43VfAC` and `E43VfB0`: they return their first argument.
const ENTITY_ECHOES: [u32; 4] = [0x51a700, 0x51a710, 0x51a740, 0x51a750];

/// The owner's slot a frame hook calls (`X`), or an old frame's when its owner's gate is set (`Y`).
pub fn frame_slot(hook: Hook, form: Form) -> u32 {
    let pair = match hook {
        Hook::One | Hook::Two => 0x9c,
        Hook::Three | Hook::Four => 0xac,
        Hook::Five | Hook::Six => 0xbc,
    };
    let second = matches!(hook, Hook::Two | Hook::Four | Hook::Six);
    pair + 4 * u32::from(second) + 8 * u32::from(form == Form::Y)
}

/// `CSolid`'s row loop at `target` (slots 0x9c..0xb8).
fn solid_rows(target: u32) -> Option<RowKind> {
    Some(match target {
        0x52ac80 => RowKind::PointsOut,
        0x52ad10 => RowKind::PointsIn,
        0x52ad40 => RowKind::Rows,
        0x52add0 => RowKind::Rows3,
        0x52b110 => RowKind::StartPointsOut,
        0x52b1f0 => RowKind::StartPointsIn,
        0x52b2e0 => RowKind::StartRows,
        0x52b380 => RowKind::StartRows3,
        _ => return None,
    })
}

/// The words of a solid its row loops read: the matrix and position (`+0x110..+0x150`) and the pose at the step's
/// start (the orientation to the position's end).
const POSE: [Range<u32>; 2] = [0x110..0x150, START_ORIENTATION..START_POSITION + 0x10];

/// What slot `1 + hook` of the frame at `at` does to rows that end at `out`: `None` for a hook, a gate or an owner slot
/// not listed above, and for a body whose pose `out` reaches (its loop would read the rows it stored). The frame's
/// and vtables' words are the image's, the owner's pose `store`'s.
pub fn passage<S: RigidStore + SolidWords>(store: &S, at: u32, hook: Hook, out: &Range<u32>) -> Option<Passage> {
    let mem = store.mem();
    let slot = 4 * (1 + hook as u32);
    let target = vtable_target(mem, at, slot);
    if ROOT_HOOKS.contains(&target) {
        return Some(Passage::Unchanged);
    }
    let old = OLD_FRAME_HOOKS.get(hook as usize) == Some(&target);
    if !old && FRAME_HOOKS.get(hook as usize) != Some(&target) {
        return None;
    }
    let frame = mem.load_entity_frame(at);
    if frame.depth == 0 {
        return Some(Passage::Unchanged);
    }
    let owner = frame.owner;
    let form = match old.then(|| vtable_target(mem, owner, GATE_SLOT)) {
        None | Some(ENTITY_GATE) => Form::X,
        Some(SOLID_GATE) if takes_part_word(store, owner) == 0 => Form::X,
        Some(SOLID_GATE) => Form::Y,
        Some(_) => return None,
    };
    let target = vtable_target(mem, owner, frame_slot(hook, form));
    if ENTITY_ECHOES.contains(&target) {
        return Some(Passage::Unchanged);
    }
    let kind = solid_rows(target)?;
    let reached = POSE.iter().any(|pose| out.start < owner + pose.end && owner + pose.start < out.end);
    (!reached).then(|| Passage::Body(row_map(store, owner, kind)))
}
