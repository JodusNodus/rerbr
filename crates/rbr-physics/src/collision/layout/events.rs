//! The collision event pool and `MakeEvent`'s payload. The calls on the event and its bodies (`E37EvOps`, `PlaneDiff`,
//! `InitFull`, `E33MakeEvent`) are `collision::events`, generic over `CollisionStore`.

use crate::mem::Mem;

use super::event_pool::FREE_LIST_HEAD;

pub use crate::collision::events::{
    apply_event_impulse, init_full, make_event_at, query_event_bodies, relative_point_velocity, EventBodyHost,
};

/// `E37EvOps`'s body query (`query_event_bodies`, entered at `sp`): the two walks' outputs and the composed point in
/// its frame, and the entry stack pointers of the transforms, of the pair walks and the compose, and of the body's
/// six-word query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventQueryFrame {
    pub walk_a: u32,
    pub walk_b: u32,
    pub composed: u32,
    pub transform_sp: u32,
    pub walk_sp: u32,
    pub query_sp: u32,
}

pub fn event_query_frame(sp: u32) -> EventQueryFrame {
    let base = super::aligned(sp);
    EventQueryFrame {
        walk_a: base - 0x20,
        walk_b: base - 0x30,
        composed: base - 0x10,
        transform_sp: base - 0x48,
        walk_sp: base - 0x50,
        query_sp: base - 0x5c,
    }
}

/// `E37EvOps`'s impulse (`apply_event_impulse`, entered at `sp`): the partner's negated `d` and `c` in its frame, and
/// the entry stack pointer of the bodies' impulse calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventImpulseFrame {
    pub negated_d: u32,
    pub negated_c: u32,
    pub impulse_sp: u32,
}

pub fn event_impulse_frame(sp: u32) -> EventImpulseFrame {
    let base = super::aligned(sp);
    EventImpulseFrame { negated_d: base - 0x20, negated_c: base - 0x10, impulse_sp: base - 0x54 }
}

/// `PlaneDiff` (`relative_point_velocity`, entered at `sp`): the two sides' point velocities in its frame, and the
/// entry stack pointer of their queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointVelocityFrame {
    pub body: u32,
    pub partner: u32,
    pub velocity_sp: u32,
}

pub fn point_velocity_frame(sp: u32) -> PointVelocityFrame {
    let base = super::aligned(sp) - 0x30;
    PointVelocityFrame { body: base + 0x10, partner: base + 0x20, velocity_sp: base - 0xc }
}

/// The entry stack pointer of `InitFull`'s calls (`init_full`, entered at `sp`).
pub fn init_event_sp(sp: u32) -> u32 {
    super::aligned(sp) - 0x90
}

/// `E33MakeEvent`'s frame aligned from its entry stack pointer `sp`: the payload the initializer copies starts after
/// the header words, and its last word is never written.
pub fn make_event_frame(sp: u32) -> u32 {
    super::aligned(sp) - 0x60
}

/// The payload's address within the builder frame `s`.
pub fn make_event_payload(s: u32) -> u32 {
    s + 0x10
}

/// The payload's last word in the builder frame of the builder entered at `sp`.
pub fn make_event_tail(sp: u32) -> u32 {
    make_event_payload(make_event_frame(sp)) + 0x3c
}

/// Writes MakeEvent's payload words at `at` before InitFull copies them.
pub fn store_payload(mem: &mut Mem, at: u32, data: &[u32; 16]) {
    for (k, word) in (0..).zip(data) {
        mem.set_u32(at + 4 * k, *word);
    }
}

/// The pool's events: 0x80 `CCollisionEvent`s (0xf0 bytes each) in `.data` from 0x8ef810, which `E37Flush5010`
/// (0x4f5010) keeps (it deletes only events outside them).
const POOL_EVENTS: u32 = 0x008e_f810;
const POOL_EVENT_COUNT: u32 = 0x80;
/// The size of a `CCollisionEvent` (`E36AllocEvent`'s argument).
pub const EVENT_SIZE: u32 = 0xf0;

/// E37Prefill5340 (0x4f5340): `E37Flush5010`, then the pool's events pushed at the free list's head in address order
/// (`list::insert`'s stores; the asm keeps the head in a register and stores it last). The load (`CPhysics::CPhysics`)
/// finds the free list empty, where the flush does nothing; flushing a non-empty list is not ported.
pub fn prefill_pool(mem: &mut Mem) {
    assert_eq!(mem.u32(FREE_LIST_HEAD), 0, "E37Flush5010 on a non-empty event pool is not ported");
    for k in 0..POOL_EVENT_COUNT {
        super::list::insert(mem, FREE_LIST_HEAD, POOL_EVENTS + EVENT_SIZE * k);
    }
}
