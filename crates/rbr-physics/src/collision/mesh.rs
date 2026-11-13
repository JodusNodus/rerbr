//! The collision mesh queries: resetting a ray object, the separating-axis box tests, the walk over the face
//! tree, the segment and ray casts, and the face query, generic over the store.

use super::layout::solver_hosts::{CellQueryHost, RayHost, SampleHost};
use super::layout::{store_point_triangle, store_ray_triangle_result, sweep_scratch, SweepScratch, SystemFrame};
use super::mesh_hit::{
    self, channel_sum, face_samples, face_vertex, face_word, nibble_sum, CHANNEL_SCALE, FACE_BYTES, NIBBLE_SCALE,
};
use super::overlap::{self};
use super::store::{CollisionStore, RayHit, SurfaceRecord};
use super::triangle::{self, RayTriangleResult};
use crate::math::vec::Vec3;

/// Resets a ray-casting object for a new query: clears its seed bytes and words, then its two cached vectors
/// (host calls). Returns `object`.
pub fn ray_seeds<S: CollisionStore>(host: &mut impl RayHost, store: &mut S, object: u32) -> u32 {
    store.clear_ray_seeds(object);
    let (first, second) = store.ray_vectors(object);
    host.vec4_clear(first);
    host.vec4_clear(second);
    object
}

/// Clears the ray object's per-test fields.
pub fn ray_clear<S: CollisionStore>(store: &mut S, object: u32) {
    store.clear_ray(object);
}

/// True (1) when no world axis separates the two boxes: the stored box at `this`, and the box with centre
/// `other_center` and half extents `other_extent`.
pub fn world_axes_overlap<S: CollisionStore>(store: &S, this: u32, other_center: u32, other_extent: u32) -> u32 {
    u32::from(overlap::world_axes_overlap(store.sat_box(this), store.vec3(other_center), store.vec3(other_extent)))
}

/// True (1) when no world or cross axis separates the stored box at `this` from the other box.
pub fn box_sat_overlap<S: CollisionStore>(store: &S, this: u32, other_center: u32, other_extent: u32) -> u32 {
    u32::from(overlap::box_sat_overlap(store.sat_box(this), store.vec3(other_center), store.vec3(other_extent)))
}

/// A tree walk's pending nodes, the last pushed popped first: the first [`Pending::INLINE`] in place, the rest on the
/// heap.
struct Pending {
    inline: [u32; Pending::INLINE],
    len: usize,
    more: Vec<u32>,
}

impl Pending {
    const INLINE: usize = 64;

    fn new() -> Self {
        Pending { inline: [0; Pending::INLINE], len: 0, more: Vec::new() }
    }

    fn push(&mut self, node: u32) {
        match self.inline.get_mut(self.len) {
            Some(slot) => {
                *slot = node;
                self.len += 1;
            }
            None => self.more.push(node),
        }
    }

    fn pop(&mut self) -> Option<u32> {
        self.more.pop().or_else(|| {
            self.len = self.len.checked_sub(1)?;
            Some(self.inline[self.len])
        })
    }
}

/// Writes the leaves under `root` whose boxes overlap the query box at `query` (centre, then half extents) to
/// `out`, after the leaf count. A leaf is a node that holds faces; a branch node pushes the children whose flag
/// word is clear and whose boxes overlap the query; the last pushed is walked first. The walk reads only the tree and
/// the query, so the query is read once, at the first branch, and each leaf is written as it is found, the count last.
pub fn node_walk<S: CollisionStore>(store: &mut S, query: u32, root: u32, out: u32) {
    let mut pending = Pending::new();
    if root != 0 {
        pending.push(root);
    }
    let mut leaves = 0;
    let mut query_box = None;
    while let Some(node) = pending.pop() {
        let tree = store.tree_node(node);
        if tree.faces() != 0 {
            store.store_list_record(out, leaves + 1, node);
            leaves += 1;
            continue;
        }
        if tree.link == 0 {
            continue;
        }
        let (center, extent) =
            *query_box.get_or_insert_with(|| (store.vec3(query), store.vec3(store.query_box_half(query))));
        for (child, word) in store.child_pair(tree.link) {
            if word & 0x20_0000 == 0 && overlap::world_axes_overlap(store.sat_box(child), center, extent) {
                pending.push(child);
            }
        }
    }
    store.set_u32(out, leaves);
}

/// Builds the query box from the centre `a` and the half extents `b` in the scratch `tmp`, and walks the object's
/// face tree over it with [`node_walk`]. The matches go to `out`, or to the object's own list when `out` is 0.
/// Returns 1 when the list has records, else 0.
pub fn query_box_faces<S: CollisionStore>(store: &mut S, this: u32, a: u32, b: u32, out: u32, tmp: u32) -> u32 {
    let lst = if out == 0 { store.own_list(this) } else { out };
    store.set_u32(lst, 0);
    let half_at = store.query_box_half(tmp);
    for k in 0..3 {
        let (centre, half) = (store.f32(a + 4 * k), store.f32(b + 4 * k));
        store.set_f32(tmp + 4 * k, centre);
        store.set_f32(half_at + 4 * k, half);
    }
    let root = store.tree_root(this);
    node_walk(store, tmp, root, lst);
    u32::from(store.u32(lst) != 0)
}

/// The half-width of the segment sweep's query box around the segment start.
const SWEEP_MARGIN: f32 = 0.1;

/// Tests the ray (`origin`, `direction`) against the object's mesh: [`triangle::mesh_ray_test`] over its stored
/// vertices and tags. On a hit the distance goes to `distance` and the four tags to `out`. Returns 1 on a hit.
pub fn mesh_test<S: CollisionStore>(
    store: &mut S,
    object: u32,
    origin: Vec3,
    direction: Vec3,
    out: u32,
    distance: u32,
) -> u32 {
    let block = store.mesh_block(object);
    let hit = {
        let view = &*store;
        triangle::mesh_ray_test(
            origin,
            direction,
            |i| view.mesh_point(block, i as u32),
            |i| view.mesh_tag(object, i as u32),
        )
    };
    let Some(hit) = hit else {
        return 0;
    };
    store.set_f32(distance, hit.distance);
    for (index, tag) in hit.tags.iter().enumerate() {
        store.set_u32(out + 4 * index as u32, *tag);
    }
    1
}

/// Queries the grid cells overlapping the start's box (widened by [`SWEEP_MARGIN`]), giving four list heads in the
/// scratch. Tests each flagged mesh object in those lists with [`mesh_test`] until one hits; the widened box stays
/// in the scratch for the query.
pub fn segment_sweep<S: CollisionStore>(
    host: &mut impl CellQueryHost<S>,
    store: &mut S,
    scratch: u32,
    start_at: u32,
    end_at: u32,
    out: u32,
    distance: u32,
) {
    let start = store.vec3(start_at);
    let direction = store.vec3(end_at) - start;
    let SweepScratch { lo, hi, cells } = sweep_scratch(scratch);
    let widen = |sign: f32| Vec3 {
        x: start.x + sign * SWEEP_MARGIN,
        y: start.y + sign * SWEEP_MARGIN,
        z: start.z + sign * SWEEP_MARGIN,
    };
    store.store_vec3(lo, widen(-1.0));
    store.store_vec3(hi, widen(1.0));
    host.query_cells(store, lo, hi, cells);

    let mut hit = 0;
    for slot in 0..4 {
        let head = store.u32(cells + 4 * slot);
        if head == 0 {
            continue;
        }
        let mut node = store.cell_first(head);
        while node != 0 && hit == 0 {
            let object = store.node_owner(node);
            if object != 0 && store.mesh_flags(object) & 0x200 != 0 {
                hit = mesh_test(store, object, start, direction, out, distance);
            }
            node = store.node_next(node);
        }
        if hit != 0 {
            break;
        }
    }
}

/// The ray/triangle intersection of `origin` and `direction` with the triangle `v0`, `v1`, `v2`, stored at
/// `output`. Every input is read before the output is written. Returns 1 on a hit.
pub fn ray_triangle<S: CollisionStore>(
    store: &mut S,
    origin: u32,
    direction: u32,
    v0: u32,
    v1: u32,
    v2: u32,
    output: u32,
) -> u32 {
    let result = triangle::ray_triangle(
        store.vec3(origin),
        store.vec3(direction),
        store.vec3(v0),
        store.vec3(v1),
        store.vec3(v2),
    );
    store_ray_triangle_result(store, output, &result);
    u32::from(result.hit)
}

/// The ray parameter the grid ray query starts its search from (2.0); `ONE` and `ZERO` are the hit bounds it
/// compares against.
const SAMPLE_START: f32 = 2.0;
const ONE: f32 = 1.0;
const ZERO: f32 = 0.0;

/// The nearest face found so far: its ray parameter, barycentric `u` and `v`, the unnormalised normal, and the
/// face and grid block it came from.
#[derive(Clone, Copy)]
struct NearestFace {
    t: f32,
    u: f32,
    v: f32,
    normal: Vec3,
    face: u32,
    base: u32,
}

/// The low and high nibble sums (scaled by 1/15) of the three face bytes.
fn sample_sums(bytes: [u8; 3], w: f32, u: f32, v: f32) -> (f32, f32) {
    let low = nibble_sum(bytes, 0, w, u, v) * NIBBLE_SCALE;
    (low, nibble_sum(bytes, 4, w, u, v) * NIBBLE_SCALE)
}

/// The nearest face of the records in `list` (the grid's own list when `list` is 0) that the ray from `start` to
/// `end` hits, with distance at most 1. The hit record goes to `out`. The query box is the ray's centre box: the
/// start with half the direction as its extent. Returns 1 on a hit, else 0.
pub fn mesh_ray_query<S: CollisionStore>(
    host: &mut impl SampleHost,
    store: &mut S,
    grid: u32,
    start_at: u32,
    end_at: u32,
    out: u32,
    list: u32,
) -> u32 {
    let start = store.vec3(start_at);
    let direction = store.vec3(end_at) - start;
    let half = direction.scale(0.5);
    let list = if list == 0 { store.own_list(grid) } else { list };
    let mut nearest =
        NearestFace { t: SAMPLE_START, u: 0.0, v: 0.0, normal: Vec3 { x: 0.0, y: 0.0, z: 0.0 }, face: 0, base: 0 };
    for index in 1..=store.list_len(list) {
        let record = store.list_record(list, index);
        let node = store.tree_node(record);
        let base = store.mesh_table(grid, node.table());
        if base == 0 || !overlap::box_sat_overlap(store.sat_box(record), start, half) {
            continue;
        }
        for face_index in 0..node.faces() {
            let face = store.tree_face(node.link, face_index);
            let bytes = store.face_bytes(face);
            let vertex = |slot: usize| store.mesh_vertex(base, face_vertex(&bytes, slot));
            let hit = triangle::ray_triangle(start, direction, vertex(0), vertex(1), vertex(2));
            if let RayTriangleResult { hit: true, distance: Some(t), u: Some(u), v: Some(v), normal: Some(normal) } =
                hit
            {
                if t >= ZERO && t < nearest.t {
                    nearest = NearestFace { t, u, v, normal, face, base };
                }
            }
        }
    }
    if nearest.t > ONE {
        return 0;
    }

    let NearestFace { t, u, v, face, base, .. } = nearest;
    let bytes = store.face_bytes(face);
    let normal = nearest.normal.normalize_sse();
    let w = ONE - (v + u);
    let corner = store.mesh_vertex(base, face_vertex(&bytes, 0));
    let (w6, w8) = (face_word(&bytes, 6), face_word(&bytes, 8));
    let (low, high) = sample_sums(face_samples(&bytes, 12), w, u, v);
    let first = host.sample(u32::from(bytes[10]), low, high) & 0xff;
    let (low, high) = sample_sums(face_samples(&bytes, 15), w, u, v);
    let second = host.sample(u32::from(bytes[11]), low, high) & 0xff;
    store.store_surface_record(
        out,
        &SurfaceRecord {
            point: direction.scale(t) + start,
            normal,
            plane_offset: -corner.dot(normal),
            channels: [channel_sum(w8, v, u, w) * CHANNEL_SCALE, channel_sum(w6, v, u, w) * CHANNEL_SCALE],
            surfaces: [first, second],
            t,
            flags: [u32::from(w6 >> 15), u32::from(w8 >> 15)],
        },
    );
    1
}

/// The ray parameter the segment query pre-seeds its hit record with.
const SEGMENT_START: f32 = 1.1;

/// Casts the mesh ray from the point at `origin_at` along the direction at `direction_at` over the tree at `root`.
/// The nearest face hit so far goes to the record at `out` (starting from the caller's pre-seeded ray parameter).
/// The worklist is LIFO with the root first: a branch node pushes the children the ray's box (the origin, half the
/// direction) overlaps; a node with faces tests them against the system's mesh table. A hit replaces the record only
/// when `t > 0` and `t` is nearer (strictly).
pub fn mesh_ray_worklist<S: CollisionStore>(
    store: &mut S,
    system: u32,
    origin_at: u32,
    direction_at: u32,
    root: u32,
    out: u32,
) {
    let half = store.vec3(direction_at).scale(0.5);
    let mut pending = Pending::new();
    pending.push(root);
    while let Some(node) = pending.pop() {
        let tree = store.tree_node(node);
        let faces = tree.faces();
        if faces == 0 {
            if tree.link == 0 {
                continue;
            }
            let center = store.vec3(origin_at);
            for (child, word) in store.child_pair(tree.link) {
                if word & 0x20_0000 == 0 && overlap::box_sat_overlap(store.sat_box(child), center, half) {
                    pending.push(child);
                }
            }
            continue;
        }
        let table = store.mesh_table(system, tree.table());
        if table == 0 {
            continue;
        }
        for face in 0..faces {
            let at = store.tree_face(tree.link, face);
            let bytes = store.face_bytes(at);
            let vertex = |slot: usize| store.mesh_vertex(table, face_vertex(&bytes, slot));
            let hit = triangle::ray_triangle(
                store.vec3(origin_at),
                store.vec3(direction_at),
                vertex(0),
                vertex(1),
                vertex(2),
            );
            if let RayTriangleResult { hit: true, distance: Some(t), u: Some(u), v: Some(v), normal: Some(normal) } =
                hit
            {
                if t > ZERO && store.ray_hit_t(out) > t {
                    store.store_ray_hit(out, &RayHit { normal, face: at, table, t, u, v });
                }
            }
        }
    }
}

/// The collision system's segment query from the point at `origin_at` to the point at `end_at`: casts the mesh ray
/// on the root ([`mesh_ray_worklist`]), then on a hit (`t` at most 1) writes the surface record to `out` (see
/// [`mesh_hit::segment_record`]). Returns 1 on a hit, else 0; a miss writes nothing but the frame.
pub fn segment_query<S: CollisionStore>(
    host: &mut impl SampleHost,
    store: &mut S,
    system: u32,
    origin_at: u32,
    end_at: u32,
    out: u32,
    frame: SystemFrame,
) -> u32 {
    let root = store.tree_root(system);
    if root == 0 {
        return 0;
    }
    let origin = store.vec3(origin_at);
    let direction = store.vec3(end_at) - origin;
    store.store_vec3(frame.origin, origin);
    store.store_vec3(frame.direction, direction);
    store.set_ray_hit_t(frame.hit, SEGMENT_START);
    mesh_ray_worklist(store, system, frame.origin, frame.direction, root, frame.hit);
    let t = store.ray_hit_t(frame.hit);
    if !(t <= ONE) {
        return 0;
    }
    let nearest = store.ray_hit(frame.hit);
    let normal = nearest.normal.normalize_sse();
    store.store_ray_hit_normal(frame.hit, normal);
    let face: [u8; FACE_BYTES] = store.face_bytes(nearest.face);
    let len = store.mesh_vertex(nearest.table, face_vertex(&face, 0)).dot(normal);
    let hit = mesh_hit::SurfaceHit { t, normal, u: nearest.u, v: nearest.v, vertex_dot_normal: len, face };
    let record = mesh_hit::segment_record(origin, direction, hit);
    let mut surfaces = [0; 2];
    for (surface, lookup) in surfaces.iter_mut().zip(record.lookups) {
        *surface = host.sample(lookup.byte, lookup.low, lookup.high) & 0xff;
    }
    store.store_surface_record(
        out,
        &SurfaceRecord {
            point: record.point,
            normal: record.normal,
            plane_offset: record.plane_offset,
            channels: record.channels,
            surfaces,
            t: record.t,
            flags: record.flags,
        },
    );
    1
}

/// Builds the query box (the centre `a`, then the half extents `b`, six floats at `query`), walks the tree and
/// returns whether the walk found anything. A null `out` selects the object's own list.
pub fn seg_test<S: CollisionStore>(store: &mut S, this: u32, a: u32, b: u32, out: u32, query: u32) -> u32 {
    let out = if out == 0 { store.own_list(this) } else { out };
    store.set_u32(out, 0);
    let half_at = store.query_box_half(query);
    for lane in 0..3 {
        let (centre, half) = (store.u32(a + 4 * lane), store.u32(b + 4 * lane));
        store.set_u32(query + 4 * lane, centre);
        store.set_u32(half_at + 4 * lane, half);
    }
    let root = store.tree_root(this);
    node_walk(store, query, root, out);
    (store.u32(out) != 0) as u32
}

/// As [`seg_test`], with the query box of the segment from `a` to `b` ([`overlap::segment_box`]).
pub fn segment_test<S: CollisionStore>(store: &mut S, this: u32, a: u32, b: u32, out: u32, query: u32) -> u32 {
    let segment = overlap::segment_box(store.vec3(a), store.vec3(b));
    let out = if out == 0 { store.own_list(this) } else { out };
    store.store_vec3(query, segment.center);
    store.store_vec3(store.query_box_half(query), segment.half_extent);
    store.set_u32(out, 0);
    let root = store.tree_root(this);
    node_walk(store, query, root, out);
    (store.u32(out) != 0) as u32
}

/// `triangle::point_triangle` of the point and the face's vertices, stored to `out`. Returns 1 on a hit.
pub fn point_triangle<S: CollisionStore>(store: &mut S, point: u32, v0: u32, v1: u32, v2: u32, out: u32) -> u32 {
    let [point, v0, v1, v2] = [point, v0, v1, v2].map(|at| store.vec3(at));
    let result = triangle::point_triangle(point, v0, v1, v2);
    store_point_triangle(store, out, &result);
    u32::from(result.foot.is_some())
}

/// The low (or high, with `shift` 4) nibble sum of the face bytes, weighted by `t`, `u` and `w` (in order) and scaled
/// by 1/15.
fn face_nibble_sum(bytes: [u8; 3], shift: u32, t: f32, u: f32, w: f32) -> f32 {
    let nibble = |byte: u8| f32::from((byte >> shift) & 0xf);
    ((nibble(bytes[1]) * t + nibble(bytes[2]) * u) + nibble(bytes[0]) * w) * NIBBLE_SCALE
}

/// The two nibble sums of the face bytes for the material table.
fn face_sample_sums(bytes: [u8; 3], t: f32, u: f32, w: f32) -> (f32, f32) {
    (face_nibble_sum(bytes, 0, t, u, w), face_nibble_sum(bytes, 4, t, u, w))
}

/// The first face of the records in `list` (the object's own list when 0) that `point_triangle` hits, stopping at
/// the first hit. Its record goes to `out`: the foot, the normal and its plane distance, the 5-bit channel sums of
/// the face words, the two material table samples, and the two flag bits. Returns 1 on a hit, else 0 with `out`
/// untouched.
pub fn face_query<S: CollisionStore>(
    host: &mut impl SampleHost,
    store: &mut S,
    this: u32,
    point_at: u32,
    out: u32,
    list: u32,
) -> u32 {
    let point = store.vec3(point_at);
    let list = if list == 0 { store.own_list(this) } else { list };
    let mut found = None;
    'records: for index in 1..=store.list_len(list) {
        let record = store.list_record(list, index);
        let node = store.tree_node(record);
        let base = store.mesh_table(this, node.table());
        if base == 0 {
            continue;
        }
        for face_index in 0..node.faces() {
            let face = store.tree_face(node.link, face_index);
            let bytes = store.face_bytes(face);
            let vertex = |slot: usize| store.mesh_vertex(base, face_vertex(&bytes, slot));
            let result = triangle::point_triangle(point, vertex(0), vertex(1), vertex(2));
            if let (Some(foot), Some(t), Some(u)) = (result.foot, result.t, result.u) {
                found = Some((foot, result.normal, t, u, face, base));
                break 'records;
            }
        }
    }
    let Some((foot, normal, t, u, face, base)) = found else {
        return 0;
    };
    let bytes = store.face_bytes(face);
    let corner = store.mesh_vertex(base, face_vertex(&bytes, 0));
    let (w6, w8) = (face_word(&bytes, 6), face_word(&bytes, 8));
    let w = ONE - (u + t);
    let (low, high) = face_sample_sums(face_samples(&bytes, 12), t, u, w);
    let first = host.sample(u32::from(bytes[10]), low, high) & 0xff;
    let (low, high) = face_sample_sums(face_samples(&bytes, 15), t, u, w);
    let second = host.sample(u32::from(bytes[11]), low, high) & 0xff;
    store.store_surface_record(
        out,
        &SurfaceRecord {
            point: foot,
            normal,
            plane_offset: -corner.dot(normal),
            channels: [channel_sum(w8, u, t, w) * CHANNEL_SCALE, channel_sum(w6, u, t, w) * CHANNEL_SCALE],
            surfaces: [first, second],
            t: ZERO,
            flags: [u32::from(w6 >> 15), u32::from(w8 >> 15)],
        },
    );
    1
}
