//! The entity vtable slots (byte offsets in the vtable) the collision model calls through `CollisionStore::vtable_slot`.

/// Slot 12 (0x30, `CEntity::Vf30`): the receiver of the entity's transforms, its own pointer in the entity tree.
pub const TRANSFORM_SLOT: u32 = 0x30;
/// Slot 0x118 (`CBody::Vf118` for a body): the entity's collision grid.
pub const GRID_SLOT: u32 = 0x118;
/// Slot 0x128: the eight-word impulse query on an event's body.
pub const IMPULSE_SLOT: u32 = 0x128;
/// Slot 0x138: the six-word query over an event's composed frames.
pub const DOT_PRODUCTS_SLOT: u32 = 0x138;
/// Slot 0x13c (`E47Solid::Vf79x`): the body's inverse mass.
pub const INVERSE_MASS_SLOT: u32 = 0x13c;
/// Slot 0x140: the velocity of the body at a point.
pub const POINT_VELOCITY_SLOT: u32 = 0x140;
/// Slots 0x80 (`CSolid::E43Vf80`, the active word) and 0x8c (`CSolid::E43Vf8C`, the very-soft word), the solver's two
/// tests before it wakes a body (`rigid_body::layout::rest`).
pub const ACTIVE_SLOT: u32 = 0x80;
pub const VERY_SOFT_SLOT: u32 = 0x8c;
/// Slot 0x90 (`E47Solid::Vf36x`): the body's wake.
pub const WAKE_SLOT: u32 = 0x90;
