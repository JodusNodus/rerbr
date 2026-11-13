//! The collision mesh's memory: the mesh queries are `collision::mesh`, generic over `CollisionStore`; here are the
//! views and small copies they and the other layout files use.

use crate::math::vec::Vec3;
use crate::mem::Mem;

use super::vec_ops::vec4_copy_from;
use crate::collision::contact_solver::MaterialPair;
use crate::collision::overlap::SatBox;
use crate::collision::store::CollisionStore;
use crate::collision::triangle::{MeshHit, PointTriangle, RayTriangleResult, SegmentTriangle};

pub use crate::collision::mesh::{
    box_sat_overlap, face_query, mesh_ray_query, mesh_ray_worklist, mesh_test, node_walk, point_triangle,
    query_box_faces, ray_clear, ray_seeds, ray_triangle, seg_test, segment_query, segment_sweep, segment_test,
    world_axes_overlap,
};

/// The scratch frame the segment query (`CCollisionSystem::Vf00`) threads through its calls: the origin and direction
/// it casts, the hit record, and the stack pointer the lookups are entered at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemFrame {
    pub origin: u32,
    pub direction: u32,
    pub hit: u32,
    pub sample_sp: u32,
}

/// The segment query's frame below its entry stack pointer `sp`.
pub fn system_frame(sp: u32) -> SystemFrame {
    let base = sp - 0x4c;
    SystemFrame { origin: base + 0x14, direction: base + 0x8, hit: base + 0x20, sample_sp: base - 0x18 }
}

/// `E38SegmentSweep`'s scratch: the widened box's low and high corners, and the four cell list heads the query
/// writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweepScratch {
    pub lo: u32,
    pub hi: u32,
    pub cells: u32,
}

pub fn sweep_scratch(scratch: u32) -> SweepScratch {
    SweepScratch { lo: scratch, hi: scratch + 0x10, cells: scratch + 0x20 }
}

/// Stores the ray/triangle result's fields: the normal (+12), `distance` (+32), `u` (+36) and `v` (+40). A miss
/// leaves the fields it never reached untouched.
pub fn store_ray_triangle_result<S: CollisionStore>(store: &mut S, at: u32, result: &RayTriangleResult) {
    if let Some(normal) = result.normal {
        store.store_vec3(at + 12, normal);
    }
    if let Some(distance) = result.distance {
        store.set_f32(at + 32, distance);
    }
    if let Some(u) = result.u {
        store.set_f32(at + 36, u);
    }
    if let Some(v) = result.v {
        store.set_f32(at + 40, v);
    }
}

/// Stores the segment/triangle record's fields: the normal (+0), `t` (+0x14), `u` (+0x18) and `v` (+0x1c). A miss
/// leaves the fields it never reached untouched.
pub fn store_segment_triangle<S: CollisionStore>(store: &mut S, at: u32, result: &SegmentTriangle) {
    if let Some(normal) = result.normal {
        store.store_vec3(at, normal);
    }
    if let Some(t) = result.t {
        store.set_f32(at + 0x14, t);
    }
    if let Some(u) = result.u {
        store.set_f32(at + 0x18, u);
    }
    if let Some(v) = result.v {
        store.set_f32(at + 0x1c, v);
    }
}

/// Stores the point/triangle result's fields: the normal (+0xc), `t` (+0x24) and `u` (+0x28), and on a hit 1.0
/// (+0x20) and the foot (+0).
pub fn store_point_triangle<S: CollisionStore>(store: &mut S, at: u32, result: &PointTriangle) {
    store.store_vec3(at + 0xc, result.normal);
    if let Some(t) = result.t {
        store.set_f32(at + 0x24, t);
    }
    if let Some(u) = result.u {
        store.set_f32(at + 0x28, u);
    }
    if let Some(foot) = result.foot {
        store.set_f32(at + 0x20, 1.0);
        store.store_vec3(at, foot);
    }
}

/// Loads a separating-axis box: the centre at +0 and the half extents at +12.
pub fn load_sat_box(mem: &Mem, at: u32) -> SatBox {
    let [x, y, z, hx, hy, hz] = mem.f32s(at);
    SatBox { center: Vec3 { x, y, z }, half_extent: Vec3 { x: hx, y: hy, z: hz } }
}

/// Loads the two tags and the weight that E38_004FB720 picks from. The table is at +0xac, or at +0x80 when bit 0x800 of
/// the flags word at +0xe4 is set.
pub fn load_material_pair(mem: &Mem, object: u32) -> MaterialPair {
    let table = if mem.u32(object + 0xe4) & 0x800 == 0 { 0xac } else { 0x80 };
    MaterialPair {
        low: mem.u32(object + table),
        high: mem.u32(object + table + 4),
        weight: mem.f32(object + table + 8),
    }
}

/// The material manager that `Tag520940` is called on: the application's +0x24 pointer, plus 0x18.
pub fn material_manager(mem: &Mem) -> u32 {
    let application = mem.u32(0x0165_9b70);
    mem.u32(application + 0x24) + 0x18
}

/// The vertex base of a collision mesh (+0xa0). Vertex `i` is four floats at `base + 16 * i`.
pub fn mesh_vertices(mem: &Mem, object: u32) -> u32 {
    mem.u32(object + 0xa0)
}

/// Mesh tag `index` (+0xb0 + 4 * index).
pub fn mesh_tag(mem: &Mem, object: u32, index: usize) -> u32 {
    mem.u32(object + 0xb0 + index as u32 * 4)
}

/// Stores the four tags of a mesh hit at `at`.
pub fn store_mesh_hit_tags(mem: &mut Mem, at: u32, hit: &MeshHit) {
    for (index, tag) in hit.tags.iter().enumerate() {
        mem.set_u32(at + index as u32 * 4, *tag);
    }
}

/// Zeroes the 0x804-byte collision mesh scratch block at `object` (E38_004FAD10, 0x4fad10).
pub fn store_zero_mesh_scratch(mem: &mut Mem, object: u32) {
    for offset in (0..0x804).step_by(4) {
        mem.set_u32(object + offset, 0);
    }
}

/// CCollisionMeshOpen::CopyBounds (0x4fb070), thiscall on the mesh with `(a, b)`: its lower bound (+0x70) is copied to
/// `a` and its upper bound (+0x80) to `b`, each by `vec4_copy_from`. Returns the second copy's eax, `b`.
pub fn mesh_copy_bounds(mem: &mut Mem, mesh: u32, a: u32, b: u32) -> u32 {
    vec4_copy_from(mem, a, mesh + 0x70);
    vec4_copy_from(mem, b, mesh + 0x80)
}
