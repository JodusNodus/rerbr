//! E37 event sorting, the event time and the constant getter leaves.

use crate::mem::Mem;

/// CCollisionEvent::Get94 (0x4f3f80) (name ours): the event's time (+0x94).
pub fn event_time(mem: &Mem, event: u32) -> f32 {
    mem.f32(event + 0x94)
}

/// E37Zero4090 (0x4f4090) (name ours): 0.
pub const fn zero() -> u32 {
    0
}

/// E37Nop40A0 (0x4f40a0) (name ours): an empty function.
pub const fn nop() {}

/// E37Zero40B0 (0x4f40b0) (name ours): 0, as `zero`.
pub const fn zero_twin() -> u32 {
    0
}

/// E37Get795E14a (0x4f40c0) (name ours): the float at 0x795e14, 1.0.
pub const fn one() -> f32 {
    1.0
}

/// E37Get795E14b (0x4f40d0) (name ours): 1.0, as `one`.
pub const fn one_twin() -> f32 {
    1.0
}

/// E37Get72AFCC (0x4f40e0) (name ours): the float at 0x72afcc, 0.3.
pub const fn three_tenths() -> f32 {
    0.3
}
