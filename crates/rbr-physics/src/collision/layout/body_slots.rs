//! CBody::VfD8 (0x4e3070): the body's pass over the two ray buffers, testing each edge pair against the other body's
//! `VfE8` and recording the hits. The pass does no arithmetic of its own: it sequences host calls (rays, matrices, the
//! other body's virtuals, events) over the frame addresses that `layout` gives. CCar::VfD8x (0x4eaf10) is a tail jump
//! to it.

/// The frame of CBody::VfD8: the two matrix operands and the scratch the calls write (addresses in the emulator).
/// The ray buffers are 0x6b0 bytes; `sub_block` is the payload's sub-block (`payload + 0x20`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeHitsFrame {
    pub a: u32,
    pub b: u32,
    pub ray_a: u32,
    pub ray_b: u32,
    pub payload: u32,
    pub sub_block: u32,
    pub aux: u32,
    pub desc: u32,
    pub twin: u32,
}

/// The stride of the ray buffers' edge records (`rayX + 0x40 + 0x10 * i`).
const EDGE_STRIDE: u32 = 0x10;
/// Where the ray buffers' edge records start.
const EDGE_BASE: u32 = 0x40;

/// The calls VfD8 makes, in the original's order. The body and the other body are the implementor's; `worker` is the
/// matrix the other body returns from its slot 0x30 or 0x34.
pub trait EdgeHitsCalls {
    /// `E38RaySeeds` (0x4f6550) on a ray buffer.
    fn ray_seeds(&mut self, ray: u32);
    /// `E33Mat34::FetchY` (0x51b780) on the matrix `matrix`, writing `out`, with the other body's matrix `worker`.
    fn fetch_y(&mut self, matrix: u32, out: u32, worker: u32);
    /// The other body's slot 0x34 matrix (`Vf34`).
    fn other_second_transform(&mut self) -> u32;
    /// The other body's slot 0x30 matrix (`Vf30`).
    fn other_transform(&mut self) -> u32;
    /// The body's own slot 0x30 matrix (`Vf30`).
    fn this_transform(&mut self) -> u32;
    /// `E33Mat34::Combine` (0x51ab90) on `out`, from `src` and `worker`.
    fn combine(&mut self, out: u32, src: u32, worker: u32);
    /// `NTweak::CE36SubBlock::Init` (0x4f3e80).
    fn sub_block_init(&mut self, block: u32);
    /// The grid's edge count (`m_unk68D`).
    fn grid_count(&mut self) -> u32;
    /// The other body's slot 0xe8 (`VfE8`) on `twin` and `desc` with the payload; true when the pair hit.
    fn grid_hit(&mut self, twin: u32, desc: u32, payload: u32) -> bool;
    /// `E33Mat34::ComposeA` (0x51b4c0) on the identity object: `out` from `payload` and `worker`.
    fn compose(&mut self, out: u32, payload: u32, worker: u32);
    /// `E36AllocEvent(0xf0)` (0x4f4f10); 0 when no event is allocated.
    fn alloc_event(&mut self) -> u32;
    /// `CCollisionEvent::InitFull` (0x4f50e0) on `event`, with the body, `aux` and `payload`; returns the event.
    fn init_full(&mut self, event: u32, aux: u32, payload: u32) -> u32;
    /// `CColGrid::SampleB` (0x4e0950) on the grid for edge `index`, writing the event's two words (`+0xa4`, `+0xa8`).
    fn vertex_sample(&mut self, index: u32, event: u32);
    /// The event list's slot 1 (`Append`).
    fn append(&mut self, event: u32);
    /// `E38RayClear` (0x4f6740) on a ray buffer.
    fn ray_clear(&mut self, ray: u32);
}

/// CBody::VfD8 (0x4e3070) (name ours): the hit flag (1 when any edge pair hit, else 0).
pub fn record_edge_hits(calls: &mut impl EdgeHitsCalls, frame: &EdgeHitsFrame) -> u32 {
    calls.ray_seeds(frame.ray_a);
    calls.ray_seeds(frame.ray_b);
    let worker = calls.other_second_transform();
    calls.fetch_y(frame.a, frame.ray_a, worker);
    let worker = calls.other_transform();
    calls.fetch_y(frame.b, frame.ray_b, worker);
    calls.sub_block_init(frame.sub_block);
    let count = calls.grid_count();
    let mut hit_any = 0;
    for index in 0..count {
        let edge = EDGE_BASE + EDGE_STRIDE * index;
        let worker = calls.other_transform();
        calls.combine(frame.desc, frame.ray_b + edge, worker);
        let worker = calls.other_second_transform();
        calls.combine(frame.twin, frame.ray_a + edge, worker);
        if calls.grid_hit(frame.twin, frame.desc, frame.payload) {
            hit_any = 1;
            let worker = calls.this_transform();
            calls.compose(frame.aux, frame.payload, worker);
            let event = match calls.alloc_event() {
                0 => 0,
                allocated => calls.init_full(allocated, frame.aux, frame.payload),
            };
            calls.vertex_sample(index, event);
            calls.append(event);
        }
    }
    calls.ray_clear(frame.ray_b);
    calls.ray_clear(frame.ray_a);
    hit_any
}

/// The calls CBody::VfE8 (0x4e2f50) makes: the body's slot 0xf4 (`VfF4`, eight arguments) on this body, then the grid's
/// two flag bytes (+0x688, +0x689), which the original reads, clears, and stores in the record on a hit.
pub trait GridHitCalls {
    /// The body's `VfF4` (slot 0xf4) on `a`, `b` and `record` (the rest of its arguments are the layout's); its eax.
    fn box_edge_hit(&mut self, a: u32, b: u32, record: u32) -> u32;
    /// The grid's flag bytes (+0x688, +0x689), read and then cleared.
    fn take_grid_flags(&mut self) -> (u32, u32);
    /// The record's words on a hit: +0x2c and +0x30 (the two flags) and +0x38 (this body).
    fn store_hit(&mut self, record: u32, flags: (u32, u32));
}

/// CBody::VfE8 (0x4e2f50): the hit flag. Nothing is tested but the record and the last argument (both must be
/// nonzero), so a miss returns 0 without touching the grid.
pub fn grid_hit(calls: &mut impl GridHitCalls, a: u32, b: u32, record: u32, last: u32) -> u32 {
    if last == 0 || record == 0 {
        return 0;
    }
    if calls.box_edge_hit(a, b, record) == 0 {
        return 0;
    }
    let flags = calls.take_grid_flags();
    calls.store_hit(record, flags);
    1
}

/// The frame of CBody::VfF4 (`box_edge_hit`): its eight arguments in the original's order (its `a` to `h`), and the
/// two box temporaries the FetchX calls write (`from` for `first`'s box, `to` for `second`'s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxEdgeHitFrame {
    pub first: u32,
    pub second: u32,
    /// The EdgeEdge outputs: the hit point, `t` and the normal.
    pub hit: u32,
    pub t: u32,
    pub normal: u32,
    /// The word a hit clears to 0.0, the EdgeEdge material and the word a hit copies it to.
    pub cleared: u32,
    pub material: u32,
    pub material_out: u32,
    pub from: u32,
    pub to: u32,
}

/// The calls CBody::VfF4 (0x4e2e90) makes, in the original's order. A hit copies the material to `material_out`,
/// clears `cleared`, and composes the hit point and the normal on the body's slot-0x30 matrix.
pub trait BoxEdgeHitCalls {
    /// The body's slot 0x34 matrix (`Vf34`).
    fn second_transform(&mut self) -> u32;
    /// The body's slot 0x30 matrix (`Vf30`).
    fn transform(&mut self) -> u32;
    /// `E33Mat34::FetchX` (0x51b6c0) on `matrix`, writing `out`, with the transform `t`.
    fn fetch_x(&mut self, matrix: u32, out: u32, t: u32);
    /// `E37GridOps::EdgeEdge` (0x4f8280) on the body's grid with `(from, to, hit, t, normal, material)`; true on a hit.
    fn edge_edge(&mut self, from: u32, to: u32, hit: u32, t: u32, normal: u32, material: u32) -> bool;
    /// The hit's record words: `material_out` takes `material`'s word, `cleared` is cleared to 0.0.
    fn store_hit(&mut self, cleared: u32, material: u32, material_out: u32);
    /// `E33Mat34::ComposeA` (0x51b4c0), thiscall on `transform` with `(point, the identity worker, out)`.
    fn compose(&mut self, transform: u32, point: u32, out: u32);
    /// `E33Mat34::ComposeB` (0x51b600), thiscall on `transform` with `(point, the identity worker, out)`; name ours, by
    /// use (the callers pass normals).
    fn compose_normal(&mut self, transform: u32, point: u32, out: u32);
}

/// CBody::VfF4 (0x4e2e90) (name ours): the hit flag. Both boxes are fetched through the body's matrices before the edge
/// test.
pub fn box_edge_hit(calls: &mut impl BoxEdgeHitCalls, frame: &BoxEdgeHitFrame) -> u32 {
    let matrix_a = calls.second_transform();
    calls.fetch_x(frame.first, frame.from, matrix_a);
    let matrix_b = calls.transform();
    calls.fetch_x(frame.second, frame.to, matrix_b);
    if !calls.edge_edge(frame.from, frame.to, frame.hit, frame.t, frame.normal, frame.material) {
        return 0;
    }
    calls.store_hit(frame.cleared, frame.material, frame.material_out);
    let transform = calls.transform();
    calls.compose(transform, frame.hit, frame.hit);
    let transform = calls.transform();
    calls.compose_normal(transform, frame.normal, frame.normal);
    1
}
