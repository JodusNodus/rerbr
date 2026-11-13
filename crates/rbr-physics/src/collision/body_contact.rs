//! The body-against-entity contacts: the point, edge and pair walks over the grids, generic over the store.

use super::grid_contact::{self as collide_big, ContactKind};
use super::layout::collide_hosts::{GridContactHost, PairContactHost};
use super::store::CollisionStore;

/// The scratch frame of the point/edge walk, addressed from its base `s`.
struct GridWalkFrame {
    s: u32,
}

impl GridWalkFrame {
    const MATERIAL: u32 = 0x1c;
    const HIT: u32 = 0x20;
    const DIR: u32 = 0x30;
    const NORMAL: u32 = 0x40;
    const DESC_A: u32 = 0x5c;
    const U: u32 = 0x64;
    const T: u32 = 0x6c;
    /// The point walk's second descriptor and the point contact's midpoint.
    const DESC_B: u32 = 0x70;
    const FROM: u32 = 0x80;
    /// The edge contact's midpoint.
    const EDGE_MID: u32 = 0x90;
    const HIT2: u32 = 0xa0;
    const BACK: u32 = 0xb0;
    const AT: u32 = 0xc0;
    const PROBE: u32 = 0xd0;
    const NORMAL2: u32 = 0xe0;
    const HIT_SPAN: u32 = 0xf0;
    const EDGE_SPAN: u32 = 0x100;
    /// This body's grid in the other entity's frame.
    const GRID: u32 = 0x110;
    /// The other entity's grid in its own frame.
    const OTHER_GRID: u32 = 0x7c0;

    fn at(&self, offset: u32) -> u32 {
        self.s + offset
    }
}

const WORLD_POINT_CONTACTS: u32 = 4;
const WORLD_EDGE_CONTACTS: u32 = 8;
/// The flag (0x40) that sends the point walk through the other entity's query slot; a negative flags byte takes
/// the init path instead.
const ENTITY_SLOT_QUERY: u8 = 0x40;

/// Contacts of the body `this` against the entity `other`: this body's grid goes into the other entity's frame.
/// With point contacts on and the other queryable through its slot (or gridless), each grid point is tested through
/// that slot; else the other's grid is copied and, with point contacts on, each grid point is paired with the grid's
/// centre, then each grid edge when no point hit and edge contacts are on. Appends the events to `list`; returns 1
/// when it appended an event.
pub fn grid_contacts<S: CollisionStore>(
    host: &mut impl GridContactHost,
    store: &mut S,
    this: u32,
    matrix: u32,
    other: u32,
    list: u32,
    esp: u32,
) -> u32 {
    let f = GridWalkFrame { s: ((esp - 4) & !0xf) - 0xe90 };
    let grid = f.at(GridWalkFrame::GRID);
    host.ray_seeds(f.s - 4, grid);
    if (store.entity_flags(other) as i8) < 0 {
        let eax = host.collide_init(f.s - 0x10, other, this, list);
        host.ray_clear(f.s - 4, grid);
        return eax;
    }
    if host.grid_active(f.s - 4, other) == 0 {
        host.ray_clear(f.s - 4, grid);
        return 0;
    }
    let transform = host.transform(f.s - 4, other);
    host.fetch_y(f.s - 0xc, matrix, grid, transform);
    copy_vec4(store, f.at(GridWalkFrame::FROM), grid + 0x20);
    if store.world_flags() & WORLD_POINT_CONTACTS != 0
        && (store.entity_flags(other) & ENTITY_SLOT_QUERY != 0 || host.grid(f.s - 4, other) == 0)
    {
        let result = slot_contacts(host, store, &f, this, other, list);
        host.ray_clear(f.s - 4, grid);
        return result;
    }
    // The pushes interleave with the calls: the first transform is the grid copy's last argument.
    let other_grid = f.at(GridWalkFrame::OTHER_GRID);
    let last = host.transform(f.s - 4, other);
    let middle = host.transform(f.s - 8, other);
    let src = host.grid(f.s - 0xc, other);
    host.grid_copy(f.s - 0x10, other_grid, src, middle, last);
    copy_vec4(store, f.at(GridWalkFrame::BACK), other_grid + 0x20);
    let away = host.active(f.s - 4, other) != 0 || host.active(f.s - 4, this) == 0;
    let mut result = 0;
    if store.world_flags() & WORLD_POINT_CONTACTS != 0 {
        let n = u32::from(store.grid_edge_count(grid));
        for i in 0..n {
            result |= point_contact(host, store, &f, this, other, list, i, away);
        }
        if result != 0 {
            host.ray_clear(f.s - 4, other_grid);
            host.ray_clear(f.s - 4, grid);
            return result;
        }
    }
    if store.world_flags() & WORLD_EDGE_CONTACTS != 0 {
        if store.grid_status_flag(other_grid) & 2 != 0
            || store.grid_edge_count(other_grid) == 0
            || store.grid_edge_count(grid) == 0
        {
            host.ray_clear(f.s - 4, other_grid);
            host.ray_clear(f.s - 4, grid);
            return 0;
        }
        let m = u32::from(store.grid_pair_count(grid));
        for i in 0..m {
            result |= edge_contact(host, store, &f, this, other, list, i, away);
        }
    }
    host.ray_clear(f.s - 4, other_grid);
    host.ray_clear(f.s - 4, grid);
    result
}

/// The walk through the other entity's query slot: each grid point against the grid's centre. A hit's point goes
/// into this body's frame and becomes a point event.
fn slot_contacts<S: CollisionStore>(
    host: &mut impl GridContactHost,
    store: &mut S,
    f: &GridWalkFrame,
    this: u32,
    other: u32,
    list: u32,
) -> u32 {
    let grid = f.at(GridWalkFrame::GRID);
    let (desc_a, desc_b) = (f.at(GridWalkFrame::DESC_A), f.at(GridWalkFrame::DESC_B));
    let transform = host.transform(f.s - 4, other);
    host.combine(f.s - 0xc, desc_a, f.at(GridWalkFrame::FROM), transform);
    let n = u32::from(store.grid_edge_count(grid));
    let mut result = 0;
    for i in 0..n {
        let transform = host.transform(f.s - 4, other);
        host.combine_twin(f.s - 0xc, desc_b, store.grid_edge(grid, i), transform);
        let material = f.at(GridWalkFrame::MATERIAL);
        let args = [
            desc_a,
            desc_b,
            f.at(GridWalkFrame::HIT),
            f.at(GridWalkFrame::U),
            f.at(GridWalkFrame::NORMAL),
            f.at(GridWalkFrame::T),
            material,
            material,
        ];
        if host.contact_query(f.s - 0x24, other, args) == 0 {
            continue;
        }
        let transform = host.transform(f.s - 4, this);
        host.fetch_x(f.s - 0xc, desc_b, f.at(GridWalkFrame::AT), transform);
        let event = make_event(host, f, ContactKind::Point, this, other, f.at(GridWalkFrame::AT));
        let (first, second) = store.event_sample_slots(event);
        host.sample_b(f.s - 0x10, f.at(GridWalkFrame::GRID), i, first, second);
        host.append(f.s - 8, list, event);
        result = 1;
    }
    result
}

/// The point contact for grid point `i`: the edge test from the grid's centre to the point. When the reverse test
/// hits too, the contact moves to the midpoint of the two hits and is probed again along the flat direction; else
/// it stays at the point. Returns 1 for an event.
#[allow(clippy::too_many_arguments)]
fn point_contact<S: CollisionStore>(
    host: &mut impl GridContactHost,
    store: &mut S,
    f: &GridWalkFrame,
    this: u32,
    other: u32,
    list: u32,
    i: u32,
    away: bool,
) -> u32 {
    let point = store.grid_edge(f.at(GridWalkFrame::GRID), i);
    let (from, mid) = (f.at(GridWalkFrame::FROM), f.at(GridWalkFrame::DESC_B));
    let t = f.at(GridWalkFrame::T);
    if edge_test(host, f, from, point, GridWalkFrame::HIT, t, GridWalkFrame::NORMAL) == 0 {
        return 0;
    }
    let at = if edge_test(host, f, point, from, GridWalkFrame::HIT2, t, GridWalkFrame::NORMAL2) != 0 {
        if !probe(host, store, f, mid, t, away, ContactKind::Point) {
            return 0;
        }
        mid
    } else {
        point
    };
    let this_transform = host.transform(f.s - 8, this);
    let transform = host.transform(f.s - 0x10, other);
    host.compose_a(f.s - 0x10, transform, at, this_transform, f.at(GridWalkFrame::AT));
    to_world(host, store, f, other);
    let event = make_event(host, f, ContactKind::Point, this, other, f.at(GridWalkFrame::AT));
    let (first, second) = store.event_sample_slots(event);
    host.sample_b(f.s - 0x10, f.at(GridWalkFrame::GRID), i, first, second);
    host.append(f.s - 8, list, event);
    1
}

/// The edge contact for grid edge record `i`: edge tests along the edge both ways, then the probe from the midpoint
/// of the hits. The event is at the midpoint, sampled at the hit's edge ratio. Returns 1 for an event.
#[allow(clippy::too_many_arguments)]
fn edge_contact<S: CollisionStore>(
    host: &mut impl GridContactHost,
    store: &mut S,
    f: &GridWalkFrame,
    this: u32,
    other: u32,
    list: u32,
    i: u32,
    away: bool,
) -> u32 {
    let grid = f.at(GridWalkFrame::GRID);
    let [first, second, ..] = store.grid_pair(grid, i);
    let (start, end) = (store.grid_edge(grid, u32::from(first)), store.grid_edge(grid, u32::from(second)));
    let (mid, u) = (f.at(GridWalkFrame::EDGE_MID), f.at(GridWalkFrame::U));
    if edge_test(host, f, start, end, GridWalkFrame::HIT, u, GridWalkFrame::NORMAL) == 0
        || edge_test(host, f, end, start, GridWalkFrame::HIT2, u, GridWalkFrame::NORMAL2) == 0
        || !probe(host, store, f, mid, u, away, ContactKind::Edge)
    {
        return 0;
    }
    let span = host.sub(f.s - 0xc, end, f.at(GridWalkFrame::EDGE_SPAN), start);
    let edge_length = store.vec3(span).length3();
    let span = host.sub(f.s - 0xc, f.at(GridWalkFrame::HIT), f.at(GridWalkFrame::HIT_SPAN), start);
    let hit_length = store.vec3(span).length3();
    let ratio = collide_big::edge_ratio(edge_length, hit_length);
    let transform = host.transform(f.s - 0x10, other);
    let hit = f.at(GridWalkFrame::HIT);
    host.compose_a(f.s - 0x10, transform, hit, store.frame_root(), hit);
    let this_transform = host.transform(f.s - 8, this);
    let transform = host.transform(f.s - 0x10, other);
    host.compose_a(f.s - 0x10, transform, mid, this_transform, mid);
    let normal = f.at(GridWalkFrame::NORMAL);
    let transform = host.transform(f.s - 0x10, other);
    host.compose_b(f.s - 0x10, transform, normal, store.frame_root(), normal);
    let event = make_event(host, f, ContactKind::Edge, this, other, mid);
    let (first, second) = store.event_sample_slots(event);
    host.sample_a(f.s - 0x14, f.at(GridWalkFrame::GRID), i, ratio, first, second);
    host.append(f.s - 8, list, event);
    1
}

/// One edge test on the other entity's grid: the hit and normal into the frame.
fn edge_test(
    host: &mut impl GridContactHost,
    f: &GridWalkFrame,
    from: u32,
    to: u32,
    hit: u32,
    t: u32,
    normal: u32,
) -> u32 {
    host.edge_edge(
        f.s - 0x1c,
        f.at(GridWalkFrame::OTHER_GRID),
        from,
        to,
        f.at(hit),
        t,
        f.at(normal),
        f.at(GridWalkFrame::MATERIAL),
    )
}

/// The midpoint of the two hits to `mid`, the normalised probe direction to the frame's direction, and the probe
/// edge test from `mid + dir` back to `mid`. On a hit, the normal is the direction with its plane offset through the
/// new hit. False when the probe misses.
fn probe<S: CollisionStore>(
    host: &mut impl GridContactHost,
    store: &mut S,
    f: &GridWalkFrame,
    mid: u32,
    t: u32,
    away: bool,
    kind: ContactKind,
) -> bool {
    let (hit, dir) = (f.at(GridWalkFrame::HIT), f.at(GridWalkFrame::DIR));
    let centre = collide_big::midpoint(store.vec3(hit), store.vec3(f.at(GridWalkFrame::HIT2)));
    store.store_vec3(mid, centre);
    let back = store.vec3(f.at(GridWalkFrame::BACK));
    let from = store.vec3(f.at(GridWalkFrame::FROM));
    store.store_vec3(dir, collide_big::probe_direction(centre, back, from, away, kind));
    host.normalize(f.s - 4, dir);
    let probe = host.add(f.s - 0x20, mid, f.at(GridWalkFrame::PROBE), dir);
    if edge_test(host, f, probe, mid, GridWalkFrame::HIT, t, GridWalkFrame::NORMAL) == 0 {
        return false;
    }
    let dir = store.vec3(dir);
    let normal = f.at(GridWalkFrame::NORMAL);
    store.store_vec3(normal, dir);
    store.set_f32(normal + 12, collide_big::plane_offset(dir, store.vec3(hit)));
    true
}

/// A point contact's hit and normal through the other entity's transform into the world.
fn to_world<S: CollisionStore>(host: &mut impl GridContactHost, store: &mut S, f: &GridWalkFrame, other: u32) {
    let (hit, normal) = (f.at(GridWalkFrame::HIT), f.at(GridWalkFrame::NORMAL));
    let transform = host.transform(f.s - 0x10, other);
    host.compose_a(f.s - 0x10, transform, hit, store.frame_root(), hit);
    let transform = host.transform(f.s - 0x10, other);
    host.compose_b(f.s - 0x10, transform, normal, store.frame_root(), normal);
}

fn make_event(
    host: &mut impl GridContactHost,
    f: &GridWalkFrame,
    kind: ContactKind,
    this: u32,
    other: u32,
    at: u32,
) -> u32 {
    let (hit, normal) = (f.at(GridWalkFrame::HIT), f.at(GridWalkFrame::NORMAL));
    host.make_event(f.s - 0x28, kind, hit, this, other, at, normal)
}

/// The scratch frame of the pair walk, addressed from its base `s`.
struct PairWalkFrame {
    s: u32,
}

impl PairWalkFrame {
    const SIZE: u32 = 0x14e0;
    /// The pair test's `t` and the word after it, which the test reads but does not write.
    const T: u32 = 0x14;
    const SCRATCH: u32 = 0x18;
    /// The overlap box: its corners, centre and half extent.
    const HI: u32 = 0x20;
    const LO: u32 = 0x30;
    const CENTRE: u32 = 0x40;
    const HIT: u32 = 0x50;
    const HALF: u32 = 0x60;
    const NORMAL: u32 = 0x70;
    /// The two fetched grids' boxes, joined.
    const UNION_LO: u32 = 0x80;
    const UNION_HI: u32 = 0x90;
    const AT: u32 = 0xa0;
    /// This body's grid fetched through `b` and through `a`, and the other entity's copied grid.
    const GRID_B: u32 = 0xb0;
    const GRID_A: u32 = 0x760;
    const OTHER_GRID: u32 = 0xe10;

    fn at(&self, offset: u32) -> u32 {
        self.s + offset
    }
}

/// Contacts of the body `this` against the entity `other`: this body's grid is fetched twice (through `a` and
/// through `b`) and the other's grid is copied. Each of this body's edge pairs whose edge meets the box where the
/// grids overlap is pair-tested. Appends the events to `list`; returns 1 when it appended an event.
#[allow(clippy::too_many_arguments)]
pub fn pair_contacts<S: CollisionStore>(
    host: &mut impl PairContactHost,
    store: &mut S,
    this: u32,
    a: u32,
    b: u32,
    other: u32,
    list: u32,
    esp: u32,
) -> u32 {
    let f = PairWalkFrame { s: ((esp - 4) & !0xf) - PairWalkFrame::SIZE };
    let (grid_a, grid_b) = (f.at(PairWalkFrame::GRID_A), f.at(PairWalkFrame::GRID_B));
    host.ray_seeds(f.s - 4, grid_a);
    host.ray_seeds(f.s - 4, grid_b);
    let src = host.grid(f.s - 4, other);
    let transform = host.second_transform(f.s - 4, other);
    host.fetch_y(f.s - 0xc, a, grid_a, transform);
    let transform = host.transform(f.s - 4, other);
    host.fetch_y(f.s - 0xc, b, grid_b, transform);
    if store.body_grid(this) == 0 || src == 0 || store.grid_edge_count(src) == 0 || store.grid_edge_count(grid_a) == 0 {
        host.ray_clear(f.s - 4, grid_b);
        host.ray_clear(f.s - 4, grid_a);
        return 0;
    }
    // The pushes interleave with the calls: the first transform is the grid copy's last argument.
    let other_grid = f.at(PairWalkFrame::OTHER_GRID);
    let last = host.transform(f.s - 4, other);
    let middle = host.transform(f.s - 8, other);
    host.grid_copy(f.s - 0x10, other_grid, src, middle, last);
    overlap_box(store, &f);
    let grid = store.body_grid(this);
    let pairs = store.grid_pair_count(grid);
    let mut result = 0;
    for i in 0..u32::from(pairs) {
        result |= pair_contact(host, store, &f, this, other, list, i);
    }
    host.ray_clear(f.s - 4, other_grid);
    host.ray_clear(f.s - 4, grid_b);
    host.ray_clear(f.s - 4, grid_a);
    result
}

/// The overlap box: the join of the two fetched grids' boxes, met with the other grid's box, to the frame's
/// corners, centre and half extent.
fn overlap_box<S: CollisionStore>(store: &mut S, f: &PairWalkFrame) {
    let (a, b) = (f.at(PairWalkFrame::GRID_A), f.at(PairWalkFrame::GRID_B));
    let other = f.at(PairWalkFrame::OTHER_GRID);
    let union_lo = store.vec4(a).min_lanes(store.vec4(b));
    store.store_vec4(f.at(PairWalkFrame::UNION_LO), union_lo);
    let union_hi = store.vec4(a + 0x10).max_lanes(store.vec4(b + 0x10));
    store.store_vec4(f.at(PairWalkFrame::UNION_HI), union_hi);
    let lo = store.vec4(other).max_lanes(union_lo);
    store.store_vec4(f.at(PairWalkFrame::LO), lo);
    let hi = store.vec4(other + 0x10).min_lanes(union_hi);
    store.store_vec4(f.at(PairWalkFrame::HI), hi);
    let (centre, half) = collide_big::overlap_centre_half(
        lo,
        hi,
        store.f32(f.at(PairWalkFrame::CENTRE) + 12),
        store.f32(f.at(PairWalkFrame::HALF) + 12),
    );
    store.store_vec4(f.at(PairWalkFrame::CENTRE), centre);
    store.store_vec4(f.at(PairWalkFrame::HALF), half);
}

/// Edge pair `i`: its edge against the overlap box, then the pair test on the other grid with the edge through `a`
/// and through `b`. A hit and its normal go into the world through the other's transform; the contact point into this
/// body's frame (`AT`). The event takes the grid's two flag bytes, which are cleared. Returns 1 for an event.
fn pair_contact<S: CollisionStore>(
    host: &mut impl PairContactHost,
    store: &mut S,
    f: &PairWalkFrame,
    this: u32,
    other: u32,
    list: u32,
    i: u32,
) -> u32 {
    let (grid_a, grid_b) = (f.at(PairWalkFrame::GRID_A), f.at(PairWalkFrame::GRID_B));
    let [b0, b1, ..] = store.grid_pair(grid_b, i);
    let (from, to) = (store.grid_edge(grid_b, u32::from(b0)), store.grid_edge(grid_b, u32::from(b1)));
    if !crate::collision::overlap::segment_overlaps_box(
        store.vec4(from),
        store.vec4(to),
        store.vec4(f.at(PairWalkFrame::CENTRE)),
        store.vec4(f.at(PairWalkFrame::HALF)),
    ) {
        return 0;
    }
    let [a0, a1, ..] = store.grid_pair(grid_a, i);
    let other_grid = f.at(PairWalkFrame::OTHER_GRID);
    let (hit, normal, at) = (f.at(PairWalkFrame::HIT), f.at(PairWalkFrame::NORMAL), f.at(PairWalkFrame::AT));
    let args = [
        store.grid_edge(grid_a, u32::from(a0)),
        store.grid_edge(grid_a, u32::from(a1)),
        from,
        to,
        hit,
        f.at(PairWalkFrame::T),
        normal,
        f.at(PairWalkFrame::SCRATCH),
        other_grid,
        0,
    ];
    if host.edge_pair(f.s - 0x2c, other_grid, args) == 0 {
        return 0;
    }
    let identity = store.frame_root();
    let transform = host.transform(f.s - 0x10, other);
    host.compose_a(f.s - 0x10, transform, hit, identity, hit);
    let identity = store.frame_root();
    let transform = host.transform(f.s - 0x10, other);
    host.compose_b(f.s - 0x10, transform, normal, identity, normal);
    let this_transform = host.transform(f.s - 8, this);
    host.compose_a(f.s - 0x10, store.frame_root(), hit, this_transform, at);
    let t = store.u32(f.at(PairWalkFrame::T));
    let event = host.pair_event(f.s - 0x28, hit, t, this, other, at, normal);
    let (flag_low, flag_high) = store.take_body_grid_flags(this);
    store.store_event_samples(event, flag_high, flag_low);
    host.append(f.s - 8, list, event);
    1
}

/// Copies the sixteen bytes at `src` to `dst`, read whole before the store.
fn copy_vec4<S: CollisionStore>(store: &mut S, dst: u32, src: u32) {
    let value = store.vec4(src);
    store.store_vec4(dst, value);
}
