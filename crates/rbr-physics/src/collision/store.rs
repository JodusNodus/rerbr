//! The collision subsystem's store (`crate::store`): the layout's load and store groups. `Mem` implements it in
//! `layout/mem_store.rs`.

use super::brake_wall::{EdgeCross, SegmentHit};
use super::contact_solver::{ContactMaterial, ContactRow, RowEvent};
use super::face_sweep::{Bounds, Face};
use super::obb::{ObbContact, ObbFrame, SphereContact};
use super::overlap::SatBox;
use crate::math::vec::{Vec3, Vec4};
use crate::store::Raw;

/// A collision event's two sides, named by the body they belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The event's body, with its material ids.
    Body,
    /// The event's partner, with its material ids.
    Partner,
}

/// A tree node's flag word (the face count and the table index) and its link (the children, or the first face).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub word: u32,
    pub link: u32,
}

impl TreeNode {
    pub fn faces(self) -> u32 {
        self.word & 0x1f_ffff
    }

    pub fn table(self) -> u32 {
        self.word >> 22
    }
}

/// The record a mesh query writes: the point, the normal, the plane offset, the two channel sums, the two surface
/// bytes, the ray parameter and the two face flags.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceRecord {
    pub point: Vec3,
    pub normal: Vec3,
    pub plane_offset: f32,
    pub channels: [f32; 2],
    pub surfaces: [u32; 2],
    pub t: f32,
    pub flags: [u32; 2],
}

/// The nearest face of a mesh ray: its normal, face, vertex table, ray parameter and barycentric coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub normal: Vec3,
    pub face: u32,
    pub table: u32,
    pub t: f32,
    pub u: f32,
    pub v: f32,
}

/// An event's id words: the first pair and the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventIds {
    pub first: [u32; 2],
    pub second: [u32; 2],
}

/// What `correct_penetrations` reads of an event: its flags, plane and point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventPlane {
    pub flags: u32,
    pub plane: Vec4,
    pub point: Vec3,
}

/// The solver, its event list, contact rows and offset rows, and the bodies and events they name. `solver`,
/// `event`, `body`, `rows` and `row` are object addresses.
///
/// Every method defaults to the image's ([`Mem`](crate::mem::Mem)'s, on [`Raw::mem`]): a store over typed state
/// overrides those of the words it holds. `Mem` implements each one itself (one it lacked would call itself).
pub trait CollisionStore: Raw {
    /// A vector at `at`, and its store.
    fn vec4(&self, at: u32) -> Vec4 {
        CollisionStore::vec4(self.mem(), at)
    }
    fn store_vec4(&mut self, at: u32, value: Vec4) {
        CollisionStore::store_vec4(self.mem_mut(), at, value)
    }
    /// The function at `object`'s virtual slot `slot`.
    fn vtable_slot(&self, object: u32, slot: u32) -> u32 {
        CollisionStore::vtable_slot(self.mem(), object, slot)
    }

    // The solver's event list.
    fn first_event(&self, solver: u32) -> u32 {
        CollisionStore::first_event(self.mem(), solver)
    }
    fn event_list_len(&self, solver: u32) -> u32 {
        CollisionStore::event_list_len(self.mem(), solver)
    }
    fn event_count(&self, solver: u32) -> u32 {
        CollisionStore::event_count(self.mem(), solver)
    }
    fn next_event(&self, event: u32) -> u32 {
        CollisionStore::next_event(self.mem(), event)
    }
    /// Whether the event releases to the pool, which the release and drain assume.
    fn event_releases_to_pool(&self, event: u32) -> bool {
        CollisionStore::event_releases_to_pool(self.mem(), event)
    }
    /// Drains the solver's event list with the pool flag.
    fn drain_event_list(&mut self, solver: u32) {
        CollisionStore::drain_event_list(self.mem_mut(), solver)
    }
    /// Releases the event: counts it off the solver's list and unlinks it when it is on it.
    fn release_event(&mut self, solver: u32, event: u32) {
        CollisionStore::release_event(self.mem_mut(), solver, event)
    }
    /// Returns the event to the free list with the pool flag.
    fn pool_event(&mut self, event: u32) {
        CollisionStore::pool_event(self.mem_mut(), event)
    }

    // An event's fields.
    fn event_body(&self, event: u32) -> u32 {
        CollisionStore::event_body(self.mem(), event)
    }
    fn event_partner(&self, event: u32) -> u32 {
        CollisionStore::event_partner(self.mem(), event)
    }
    fn event_flags(&self, event: u32) -> u32 {
        CollisionStore::event_flags(self.mem(), event)
    }
    fn store_event_flags(&mut self, event: u32, flags: u32) {
        CollisionStore::store_event_flags(self.mem_mut(), event, flags)
    }
    fn event_suppressed(&self, event: u32) -> bool {
        CollisionStore::event_suppressed(self.mem(), event)
    }
    /// The event's time.
    fn event_time(&self, event: u32) -> f32 {
        CollisionStore::event_time(self.mem(), event)
    }
    fn event_plane(&self, event: u32) -> EventPlane {
        CollisionStore::event_plane(self.mem(), event)
    }
    /// The event's plane vector.
    fn event_normal(&self, event: u32) -> Vec4 {
        CollisionStore::event_normal(self.mem(), event)
    }
    /// The event's material refresh from its bodies' grid tags.
    fn refresh_event_materials(&mut self, event: u32) {
        CollisionStore::refresh_event_materials(self.mem_mut(), event)
    }
    /// Whether the material of the event's id on `side` has flag bit 3.
    fn side_suppresses(&self, event: u32, side: Side) -> bool {
        CollisionStore::side_suppresses(self.mem(), event, side)
    }
    /// What `contact_rows` reads of the event.
    fn row_event(&self, event: u32) -> RowEvent {
        CollisionStore::row_event(self.mem(), event)
    }
    /// The tag of the event's material and the reals of its entry.
    fn event_material(&self, event: u32) -> (u32, ContactMaterial) {
        CollisionStore::event_material(self.mem(), event)
    }
    /// The address of the event's contact point `contact_rows` composes for `side` (0 the partner's, else the body's).
    fn event_other_point(&self, event: u32, side: u32) -> u32 {
        CollisionStore::event_other_point(self.mem(), event, side)
    }

    // A body.
    /// Whether the body takes part in the solve.
    fn body_active(&self, body: u32) -> bool {
        CollisionStore::body_active(self.mem(), body)
    }
    fn body_progress(&self, body: u32) -> f32 {
        CollisionStore::body_progress(self.mem(), body)
    }
    fn body_position(&self, body: u32) -> Vec4 {
        CollisionStore::body_position(self.mem(), body)
    }
    fn store_body_position(&mut self, body: u32, value: Vec4) {
        CollisionStore::store_body_position(self.mem_mut(), body, value)
    }
    fn body_contact_rows(&self, body: u32) -> u32 {
        CollisionStore::body_contact_rows(self.mem(), body)
    }
    fn set_body_contact_rows(&mut self, body: u32, rows: u32) {
        CollisionStore::set_body_contact_rows(self.mem_mut(), body, rows)
    }
    fn body_offset_row(&self, body: u32) -> u32 {
        CollisionStore::body_offset_row(self.mem(), body)
    }
    fn set_body_offset_row(&mut self, body: u32, row: u32) {
        CollisionStore::set_body_offset_row(self.mem_mut(), body, row)
    }
    /// The root frame the body's transforms compose on.
    fn frame_root(&self) -> u32 {
        CollisionStore::frame_root(self.mem())
    }

    // The solver's contact rows.
    fn solver_contact_count(&self, solver: u32) -> u32 {
        CollisionStore::solver_contact_count(self.mem(), solver)
    }
    fn set_solver_contact_count(&mut self, solver: u32, count: u32) {
        CollisionStore::set_solver_contact_count(self.mem_mut(), solver, count)
    }
    /// The body of the solver's contact row `index`.
    fn solver_contact_body(&self, solver: u32, index: u32) -> u32 {
        CollisionStore::solver_contact_body(self.mem(), solver, index)
    }
    /// The address of the solver's contact row `index`.
    fn solver_contact_row(&self, solver: u32, index: u32) -> u32 {
        CollisionStore::solver_contact_row(self.mem(), solver, index)
    }
    /// Starts a contact row for `body` at `rows`: its body, an empty count, and the body's link to it.
    fn open_contact_row(&mut self, rows: u32, body: u32) {
        CollisionStore::open_contact_row(self.mem_mut(), rows, body)
    }
    fn contact_row_count(&self, rows: u32) -> u32 {
        CollisionStore::contact_row_count(self.mem(), rows)
    }
    fn set_contact_row_count(&mut self, rows: u32, count: u32) {
        CollisionStore::set_contact_row_count(self.mem_mut(), rows, count)
    }
    fn store_contact_row(&mut self, rows: u32, index: u32, row: &ContactRow) {
        CollisionStore::store_contact_row(self.mem_mut(), rows, index, row)
    }

    // The solver's offset rows.
    fn solver_offset_count(&self, solver: u32) -> u32 {
        CollisionStore::solver_offset_count(self.mem(), solver)
    }
    fn set_solver_offset_count(&mut self, solver: u32, count: u32) {
        CollisionStore::set_solver_offset_count(self.mem_mut(), solver, count)
    }
    /// The address of the solver's offset row `index`.
    fn solver_offset_row(&self, solver: u32, index: u32) -> u32 {
        CollisionStore::solver_offset_row(self.mem(), solver, index)
    }
    fn offset_row_body(&self, row: u32) -> u32 {
        CollisionStore::offset_row_body(self.mem(), row)
    }
    fn offset_row_count(&self, row: u32) -> u32 {
        CollisionStore::offset_row_count(self.mem(), row)
    }
    fn set_offset_row_count(&mut self, row: u32, count: u32) {
        CollisionStore::set_offset_row_count(self.mem_mut(), row, count)
    }
    fn offset(&self, row: u32, index: u32) -> Vec4 {
        CollisionStore::offset(self.mem(), row, index)
    }
    fn store_offset(&mut self, row: u32, index: u32, value: Vec4) {
        CollisionStore::store_offset(self.mem_mut(), row, index, value)
    }
    /// Starts an offset row for `body` with its one offset in place: the body, the body's link and the count of 1.
    fn open_offset_row(&mut self, row: u32, body: u32) {
        CollisionStore::open_offset_row(self.mem_mut(), row, body)
    }

    /// The body's bounds: the lower and upper vectors.
    fn body_bounds(&self, body: u32) -> (Vec4, Vec4) {
        CollisionStore::body_bounds(self.mem(), body)
    }
    /// Whether the grid is active.
    fn grid_active(&self, grid: u32) -> u8 {
        CollisionStore::grid_active(self.mem(), grid)
    }

    // The body contact walks: the other entity, the world flags and the grids.
    fn entity_flags(&self, entity: u32) -> u8 {
        CollisionStore::entity_flags(self.mem(), entity)
    }
    /// The world settings' contact flags (bit 2 point contacts, bit 3 edge contacts).
    fn world_flags(&self) -> u32 {
        CollisionStore::world_flags(self.mem())
    }
    fn body_grid(&self, body: u32) -> u32 {
        CollisionStore::body_grid(self.mem(), body)
    }
    fn grid_edge_count(&self, grid: u32) -> u8 {
        CollisionStore::grid_edge_count(self.mem(), grid)
    }
    fn grid_pair_count(&self, grid: u32) -> u8 {
        CollisionStore::grid_pair_count(self.mem(), grid)
    }
    fn grid_status_flag(&self, grid: u32) -> u8 {
        CollisionStore::grid_status_flag(self.mem(), grid)
    }
    /// The address of the grid's edge vector `index`.
    fn grid_edge(&self, grid: u32, index: u32) -> u32 {
        CollisionStore::grid_edge(self.mem(), grid, index)
    }
    /// The addresses of the grid's box corners, low and high.
    fn grid_corners(&self, grid: u32) -> (u32, u32) {
        CollisionStore::grid_corners(self.mem(), grid)
    }
    /// The address of the grid's centre.
    fn grid_centre(&self, grid: u32) -> u32 {
        CollisionStore::grid_centre(self.mem(), grid)
    }
    /// The six bytes of the grid's edge record `index`: the two vertices and the sample indices.
    fn grid_pair(&self, grid: u32, index: u32) -> [u8; 6] {
        CollisionStore::grid_pair(self.mem(), grid, index)
    }
    /// The body's grid flag bytes, read and cleared.
    fn take_body_grid_flags(&mut self, body: u32) -> (u32, u32) {
        CollisionStore::take_body_grid_flags(self.mem_mut(), body)
    }
    /// The addresses of the event's two sample words.
    fn event_sample_slots(&self, event: u32) -> (u32, u32) {
        CollisionStore::event_sample_slots(self.mem(), event)
    }
    fn store_event_samples(&mut self, event: u32, first: u32, second: u32) {
        CollisionStore::store_event_samples(self.mem_mut(), event, first, second)
    }

    // The brake wall: its quadtree, the leaf collector and the segment table.
    /// The quadtree's root node.
    fn wall_root(&self, wall: u32) -> u32 {
        CollisionStore::wall_root(self.mem(), wall)
    }
    /// A quadtree node's item count and link (the children, or the leaf's data).
    fn wall_node(&self, node: u32) -> (u32, u32) {
        CollisionStore::wall_node(self.mem(), node)
    }
    /// The two children a node's link names.
    fn wall_children(&self, link: u32) -> [u32; 2] {
        CollisionStore::wall_children(self.mem(), link)
    }
    /// A node's box: the centre x and y, then the half extents.
    fn wall_box(&self, node: u32) -> [f32; 4] {
        CollisionStore::wall_box(self.mem(), node)
    }
    fn clear_collector(&mut self, collector: u32) {
        CollisionStore::clear_collector(self.mem_mut(), collector)
    }
    fn collector_count(&self, collector: u32) -> u32 {
        CollisionStore::collector_count(self.mem(), collector)
    }
    fn collector_push(&mut self, collector: u32, node: u32) {
        CollisionStore::collector_push(self.mem_mut(), collector, node)
    }
    fn collector_leaf(&self, collector: u32, index: u32) -> u32 {
        CollisionStore::collector_leaf(self.mem(), collector, index)
    }
    fn wall_latch(&self, wall: u32) -> u32 {
        CollisionStore::wall_latch(self.mem(), wall)
    }
    fn set_wall_latch(&mut self, wall: u32, value: u32) {
        CollisionStore::set_wall_latch(self.mem_mut(), wall, value)
    }
    /// The collector the box query fills (the wall's leaf list).
    fn wall_leaves(&self, wall: u32) -> u32 {
        CollisionStore::wall_leaves(self.mem(), wall)
    }
    fn wall_mesh(&self, wall: u32) -> u32 {
        CollisionStore::wall_mesh(self.mem(), wall)
    }
    /// The wall manager's wall (0 without one).
    fn manager_wall(&self, manager: u32) -> u32 {
        CollisionStore::manager_wall(self.mem(), manager)
    }
    /// The probe's point buffer at `at`: the point's x and y words.
    fn store_probe_point(&mut self, at: u32, xy: [u32; 2]) {
        CollisionStore::store_probe_point(self.mem_mut(), at, xy)
    }
    /// The four vertices of segment record `index`.
    fn wall_segment(&self, mesh: u32, index: u32) -> [[f32; 2]; 4] {
        CollisionStore::wall_segment(self.mem(), mesh, index)
    }
    /// A leaf's segment count, and the index word of its segment `j`.
    fn leaf_count(&self, leaf: u32) -> u32 {
        CollisionStore::leaf_count(self.mem(), leaf)
    }
    fn leaf_word(&self, leaf: u32, j: u32) -> u32 {
        CollisionStore::leaf_word(self.mem(), leaf, j)
    }
    /// The penetration signal's emit object.
    fn penetration_emitter(&self) -> u32 {
        CollisionStore::penetration_emitter(self.mem())
    }
    /// The probe record at `out`: the normal, penetration, signed distance and flags.
    fn store_wall_probe(&mut self, out: u32, hit: &SegmentHit, flags: u32) {
        CollisionStore::store_wall_probe(self.mem_mut(), out, hit, flags)
    }
    fn store_wall_flags(&mut self, out: u32, flags: u32) {
        CollisionStore::store_wall_flags(self.mem_mut(), out, flags)
    }
    /// The crossing record at `out`: the point, the normal, the offset, the parameter and the wall.
    fn store_wall_crossing(&mut self, out: u32, wall: u32, hit: &EdgeCross) {
        CollisionStore::store_wall_crossing(self.mem_mut(), out, wall, hit)
    }

    // The collision mesh: the node tree, the face lists and the hit records.
    fn sat_box(&self, at: u32) -> SatBox {
        CollisionStore::sat_box(self.mem(), at)
    }
    fn tree_node(&self, node: u32) -> TreeNode {
        CollisionStore::tree_node(self.mem(), node)
    }
    /// The two children a node's link names: each one's address and flag word.
    fn child_pair(&self, link: u32) -> [(u32, u32); 2] {
        CollisionStore::child_pair(self.mem(), link)
    }
    /// The address of face `index` of a leaf whose link is `link`.
    fn tree_face(&self, link: u32, index: u32) -> u32 {
        CollisionStore::tree_face(self.mem(), link, index)
    }
    /// The tree root of a collider or system.
    fn tree_root(&self, owner: u32) -> u32 {
        CollisionStore::tree_root(self.mem(), owner)
    }
    /// The collider's own list of records, used when the caller gives none.
    fn own_list(&self, owner: u32) -> u32 {
        CollisionStore::own_list(self.mem(), owner)
    }
    fn list_len(&self, list: u32) -> u32 {
        CollisionStore::list_len(self.mem(), list)
    }
    fn list_record(&self, list: u32, index: u32) -> u32 {
        CollisionStore::list_record(self.mem(), list, index)
    }
    fn store_list_record(&mut self, list: u32, index: u32, record: u32) {
        CollisionStore::store_list_record(self.mem_mut(), list, index, record)
    }
    /// The address of a query box's half extents (its centre is at the box's address).
    fn query_box_half(&self, query: u32) -> u32 {
        CollisionStore::query_box_half(self.mem(), query)
    }
    /// The vertex table `index` of a collider or system.
    fn mesh_table(&self, owner: u32, index: u32) -> u32 {
        CollisionStore::mesh_table(self.mem(), owner, index)
    }
    /// The vertex in slot `slot` of a vertex table.
    fn mesh_vertex(&self, table: u32, slot: u16) -> Vec3 {
        CollisionStore::mesh_vertex(self.mem(), table, slot)
    }
    /// The face record at `at`.
    fn face_bytes(&self, at: u32) -> [u8; 0x12] {
        CollisionStore::face_bytes(self.mem(), at)
    }
    fn mesh_flags(&self, object: u32) -> u32 {
        CollisionStore::mesh_flags(self.mem(), object)
    }
    /// A mesh's vertex block and its tags.
    fn mesh_block(&self, object: u32) -> u32 {
        CollisionStore::mesh_block(self.mem(), object)
    }
    fn mesh_point(&self, block: u32, index: u32) -> Vec3 {
        CollisionStore::mesh_point(self.mem(), block, index)
    }
    fn mesh_tag(&self, object: u32, index: u32) -> u32 {
        CollisionStore::mesh_tag(self.mem(), object, index)
    }
    /// A ray object's seeds cleared, and the addresses of its two vectors.
    fn clear_ray_seeds(&mut self, object: u32) {
        CollisionStore::clear_ray_seeds(self.mem_mut(), object)
    }
    fn ray_vectors(&self, object: u32) -> (u32, u32) {
        CollisionStore::ray_vectors(self.mem(), object)
    }
    fn clear_ray(&mut self, object: u32) {
        CollisionStore::clear_ray(self.mem_mut(), object)
    }
    /// The surface record a mesh query returns.
    fn store_surface_record(&mut self, out: u32, record: &SurfaceRecord) {
        CollisionStore::store_surface_record(self.mem_mut(), out, record)
    }
    /// The nearest-face record the node worklist keeps.
    fn ray_hit(&self, hit: u32) -> RayHit {
        CollisionStore::ray_hit(self.mem(), hit)
    }
    fn store_ray_hit(&mut self, hit: u32, value: &RayHit) {
        CollisionStore::store_ray_hit(self.mem_mut(), hit, value)
    }
    fn ray_hit_t(&self, hit: u32) -> f32 {
        CollisionStore::ray_hit_t(self.mem(), hit)
    }
    fn set_ray_hit_t(&mut self, hit: u32, t: f32) {
        CollisionStore::set_ray_hit_t(self.mem_mut(), hit, t)
    }
    fn store_ray_hit_normal(&mut self, hit: u32, normal: Vec3) {
        CollisionStore::store_ray_hit_normal(self.mem_mut(), hit, normal)
    }

    // The collision grid's face and edge-pair tables.
    fn grid_bounds(&self, grid: u32) -> Bounds {
        CollisionStore::grid_bounds(self.mem(), grid)
    }
    fn grid_face_count(&self, grid: u32) -> u32 {
        CollisionStore::grid_face_count(self.mem(), grid)
    }
    /// Whether the face's material is sampled (not the skipped value).
    fn grid_face_is_valid(&self, grid: u32, index: u32) -> bool {
        CollisionStore::grid_face_is_valid(self.mem(), grid, index)
    }
    fn grid_face(&self, grid: u32, index: u32) -> Face {
        CollisionStore::grid_face(self.mem(), grid, index)
    }
    fn grid_face_samples(&self, grid: u32, index: u32) -> [u8; 3] {
        CollisionStore::grid_face_samples(self.mem(), grid, index)
    }
    /// The triangle vertices of a face's three vertex indices.
    fn grid_face_vertices(&self, grid: u32, indices: [u8; 3]) -> [Vec3; 3] {
        CollisionStore::grid_face_vertices(self.mem(), grid, indices)
    }
    /// The grid's material tag, which a hit copies.
    fn grid_material(&self, grid: u32) -> u32 {
        CollisionStore::grid_material(self.mem(), grid)
    }
    /// The sample atlas' width and height, `None` for a grid without one.
    fn grid_sample_dimensions(&self, grid: u32) -> Option<(u16, u16)> {
        CollisionStore::grid_sample_dimensions(self.mem(), grid)
    }
    fn grid_vertex_samples(&self, grid: u32, indices: [u8; 3]) -> [[u8; 2]; 3] {
        CollisionStore::grid_vertex_samples(self.mem(), grid, indices)
    }
    fn grid_sample_bytes(&self, grid: u32, first: i32, second: i32) -> [u8; 2] {
        CollisionStore::grid_sample_bytes(self.mem(), grid, first, second)
    }
    /// The grid's sample result bytes.
    fn store_grid_sample_bytes(&mut self, grid: u32, bytes: [u8; 2]) {
        CollisionStore::store_grid_sample_bytes(self.mem_mut(), grid, bytes)
    }
    /// The vertex in slot `slot` of the grid's vertex table.
    fn grid_vertex(&self, grid: u32, slot: u8) -> Vec3 {
        CollisionStore::grid_vertex(self.mem(), grid, slot)
    }
    /// The two vertex slots of edge-pair row `index`.
    fn grid_pair_row(&self, grid: u32, index: u32) -> [u8; 2] {
        CollisionStore::grid_pair_row(self.mem(), grid, index)
    }
    fn grid_sample_slot(&self, grid: u32, slot: u8) -> u8 {
        CollisionStore::grid_sample_slot(self.mem(), grid, slot)
    }
    fn grid_sample_pair(&self, grid: u32, sample: u8) -> [u8; 2] {
        CollisionStore::grid_sample_pair(self.mem(), grid, sample)
    }
    /// Makes `grid` a copy of `src`.
    fn copy_grid(&mut self, grid: u32, src: u32) {
        CollisionStore::copy_grid(self.mem_mut(), grid, src)
    }
    /// The addresses of the grid's two point blocks.
    fn grid_blocks(&self, grid: u32) -> (u32, u32) {
        CollisionStore::grid_blocks(self.mem(), grid)
    }
    /// The scale the half extent takes.
    fn grid_half_scale(&self, grid: u32) -> f32 {
        CollisionStore::grid_half_scale(self.mem(), grid)
    }
    /// The grid's bounds: low, high, centre and half extent.
    fn store_grid_bounds(&mut self, grid: u32, low: Vec4, high: Vec4, centre: Vec4, half: Vec4) {
        CollisionStore::store_grid_bounds(self.mem_mut(), grid, low, high, centre, half)
    }

    // Event creation and the events' calls on their bodies.
    /// A three-lane vector at `at`, and its store (the fourth lane is not written).
    fn vec3(&self, at: u32) -> Vec3 {
        CollisionStore::vec3(self.mem(), at)
    }
    fn store_vec3(&mut self, at: u32, value: Vec3) {
        CollisionStore::store_vec3(self.mem_mut(), at, value)
    }
    /// The address of the event's contact point, the vector its bodies' calls take.
    fn event_point(&self, event: u32) -> u32 {
        CollisionStore::event_point(self.mem(), event)
    }
    /// The address of the event's material ids on `side`.
    fn event_materials_at(&self, event: u32, side: Side) -> u32 {
        CollisionStore::event_materials_at(self.mem(), event, side)
    }
    fn event_ids(&self, event: u32) -> EventIds {
        CollisionStore::event_ids(self.mem(), event)
    }
    /// Where the full initialization has the body's transforms write the event's two contact points.
    fn event_contact_outputs(&self, event: u32) -> (u32, u32) {
        CollisionStore::event_contact_outputs(self.mem(), event)
    }
    /// The full initialization's field writes, in order, before its calls: the links cleared, the handle on the root
    /// identity, the base table, the vectors and ids cleared, the tag, the 16 payload words at `payload`, the body
    /// and the kind in the flags.
    fn init_event(&mut self, event: u32, kind: u32, body: u32, tag: u32, payload: u32) {
        CollisionStore::init_event(self.mem_mut(), event, kind, body, tag, payload)
    }
    /// The global event count, one more.
    fn count_event(&mut self) {
        CollisionStore::count_event(self.mem_mut())
    }
    /// The head of the free list of events.
    fn free_event(&self) -> u32 {
        CollisionStore::free_event(self.mem())
    }
    /// Takes a free event off the list when it is on it: the count one less, the event unlinked.
    fn take_free_event(&mut self, event: u32) {
        CollisionStore::take_free_event(self.mem_mut(), event)
    }
    /// A grid's two hit flag bytes, which are cleared after the read.
    fn take_grid_hit_flags(&mut self, grid: u32) -> [u32; 2] {
        CollisionStore::take_grid_hit_flags(self.mem_mut(), grid)
    }
    fn grid_tag(&self, grid: u32) -> u32 {
        CollisionStore::grid_tag(self.mem(), grid)
    }
    /// Sets all three of the event's body-side material words to `tag` (the last cleared).
    fn store_event_tag(&mut self, event: u32, tag: u32) {
        CollisionStore::store_event_tag(self.mem_mut(), event, tag)
    }

    // The collision entry (`object`): a box frame and four contact records, filled from the bodies in the cells.
    fn obb_frame(&self, object: u32) -> ObbFrame {
        CollisionStore::obb_frame(self.mem(), object)
    }
    /// The sphere query at `at`: its centre and radius.
    fn sphere_input(&self, at: u32) -> (Vec4, f32) {
        CollisionStore::sphere_input(self.mem(), at)
    }
    /// The five vectors an OBB query takes at `at`: the centre, the half extents and the three axes.
    fn obb_input(&self, at: u32) -> [Vec4; 5] {
        CollisionStore::obb_input(self.mem(), at)
    }
    /// The w lane of the slot's previous normal, which the contact's normalisation scales.
    fn contact_normal_w(&self, object: u32, index: u32) -> f32 {
        CollisionStore::contact_normal_w(self.mem(), object, index)
    }
    /// Writes contact `index` of the entry; the OBB contact keeps the position's w lane.
    fn store_sphere_contact(&mut self, object: u32, index: u32, input: Vec4, contact: &SphereContact) {
        CollisionStore::store_sphere_contact(self.mem_mut(), object, index, input, contact)
    }
    fn store_obb_contact(&mut self, object: u32, index: u32, input: Vec4, contact: &ObbContact) {
        CollisionStore::store_obb_contact(self.mem_mut(), object, index, input, contact)
    }
    /// The surface tag byte of contact `index`.
    fn store_contact_tag(&mut self, object: u32, index: u32, tag: u8) {
        CollisionStore::store_contact_tag(self.mem_mut(), object, index, tag)
    }
    /// Where the entry's frame goes in and out of the bodies' transforms.
    fn entry_frame(&self, object: u32) -> EntryFrame {
        CollisionStore::entry_frame(self.mem(), object)
    }
    /// The centre the transform last wrote.
    fn entry_centre(&self, object: u32) -> [f32; 3] {
        CollisionStore::entry_centre(self.mem(), object)
    }
    /// The address the entry's contact records are listed at.
    fn contact_list(&self, object: u32) -> u32 {
        CollisionStore::contact_list(self.mem(), object)
    }
    /// The matrix address the entry composes with.
    fn collide_matrix(&self) -> u32 {
        CollisionStore::collide_matrix(self.mem())
    }
    /// A body's collision entry: its active shape, whether that shape is a sphere, and its tag.
    fn collision_shape(&self, collision: u32) -> u32 {
        CollisionStore::collision_shape(self.mem(), collision)
    }
    fn collision_is_sphere(&self, collision: u32) -> bool {
        CollisionStore::collision_is_sphere(self.mem(), collision)
    }
    fn collision_tag(&self, collision: u32) -> u8 {
        CollisionStore::collision_tag(self.mem(), collision)
    }
    /// A shape's OBB axis `axis` (0 to 2) and half-length.
    fn shape_axis(&self, shape: u32, axis: u32) -> Vec4 {
        CollisionStore::shape_axis(self.mem(), shape, axis)
    }
    fn shape_extent(&self, shape: u32, axis: u32) -> f32 {
        CollisionStore::shape_extent(self.mem(), shape, axis)
    }
    /// A cell list's first node, a node's next and its owning body.
    fn cell_first(&self, cell: u32) -> u32 {
        CollisionStore::cell_first(self.mem(), cell)
    }
    fn node_next(&self, node: u32) -> u32 {
        CollisionStore::node_next(self.mem(), node)
    }
    fn node_owner(&self, node: u32) -> u32 {
        CollisionStore::node_owner(self.mem(), node)
    }
}

/// The addresses of a collision entry's frame: the point and axes the bodies' transforms read (`*_in`) and the places
/// they write (`*_out`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntryFrame {
    pub centre_in: u32,
    pub centre_out: u32,
    pub axes_in: [u32; 3],
    pub axes_out: [u32; 3],
}
