// The grass made on the GPU (`grass::made`): each workgroup walks one tile's candidate clumps, ranked by their index
// (the first of them an even subset, so the density takes a prefix), placed on a low-discrepancy sequence across the
// tile; a clump grows where the density at its distance reaches its rank, its cell's likelihood takes it and the
// ground stays grassy under its footprint (the cell's support), and writes its two crossed cards when they are in
// view. The constants before this are `grass`'s (`grass::made::source`).

struct Cull {
    // The view's planes, normalised (a degenerate one 0, 0, 0, 1).
    planes: array<vec4<f32>, 6>,
    eye: vec4<f32>,
    // The level's profile (`grass::Level::profile`, `frame.grass` in `shader.wgsl`).
    profile: vec4<f32>,
    // The tiles listed, the most cards, a converted stage (1) and the candidates a tile.
    info: vec4<u32>,
}

struct Tile {
    slot: u32,
    x: i32,
    z: i32,
    count: u32,
}

struct Profile {
    cell: f32,
    first: u32,
    count: u32,
    atlas: u32,
}

struct Piece {
    rect: vec4<f32>,
    cells: vec2<f32>,
    bound: f32,
    piece: u32,
}

// The cards made and, at `ARGS_OFFSET`, the indirect draw's arguments.
struct Counter {
    count: atomic<u32>,
    p0: u32,
    p1: u32,
    p2: u32,
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

@group(0) @binding(0) var<uniform> cull: Cull;
@group(0) @binding(1) var<storage, read> tiles: array<Tile>;
@group(0) @binding(2) var<storage, read> pool: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read> profiles: array<Profile>;
@group(0) @binding(4) var<storage, read> pieces: array<Piece>;
@group(0) @binding(5) var<storage, read_write> cards: array<GrassCard>;
@group(0) @binding(6) var<storage, read_write> counter: Counter;

const PI: f32 = 3.14159265;
const LAWN: u32 = 0u;
const MEADOW: u32 = 1u;
const DRY: u32 = 2u;
const SEEDS: u32 = 3u;

// PCG.
fn hash(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// 0 to 1.
fn next(state: ptr<function, u32>) -> f32 {
    *state = hash(*state);
    return f32(*state >> 8u) / 16777216.0;
}

// `grass::density`.
fn density(d: f32) -> f32 {
    let p = cull.profile;
    return p.x * min(p.y / max(d, 1e-3), 1.0) * (1.0 - smoothstep(FADE * p.w, p.w, d));
}

// `grass::Kind::pick`.
fn kind_of(pick: f32, tall: f32, dry: f32) -> u32 {
    let seeds = 0.04 + 0.1 * smoothstep(1.0, 1.4, tall);
    let dry_share = 0.04 + 0.4 * dry;
    let meadow = 0.2 + 0.3 * smoothstep(0.8, 1.3, tall);
    if (pick < seeds) {
        return SEEDS;
    }
    if (pick < seeds + dry_share) {
        return DRY;
    }
    if (pick < seeds + dry_share + meadow) {
        return MEADOW;
    }
    return LAWN;
}

// `grass::Kind::height`.
fn kind_height(kind: u32) -> f32 {
    return array<f32, 4>(1.0, 1.3, 1.15, 1.9)[kind];
}

// `grass::Kind::picture`.
fn picture_of(kind: u32, pick: f32) -> u32 {
    switch kind {
        case 0u: {
            return u32(fract(pick * 97.0) * 8.0) % 8u;
        }
        case 1u: {
            return 8u + u32(fract(pick * 89.0) * 4.0) % 4u;
        }
        case 2u: {
            return 12u + u32(fract(pick * 83.0) * 2.0) % 2u;
        }
        default: {
            return 14u + u32(fract(pick * 79.0) * 2.0) % 2u;
        }
    }
}

// `grass::picture_rect`.
fn picture_rect(picture: u32) -> vec4<f32> {
    let margin = 2.0 / (PICTURE * vec2<f32>(f32(COLUMNS), f32(ROWS)));
    let at = vec2<f32>(f32(picture % COLUMNS), f32(picture / COLUMNS));
    let size = vec2<f32>(f32(COLUMNS), f32(ROWS));
    return vec4<f32>(at.x / size.x + margin.x, (at.x + 1.0) / size.x - margin.x, at.y / size.y + margin.y,
        (at.y + 1.0) / size.y - margin.y);
}

// `grass::Fx::piece`: the profile's piece by `pick`.
fn piece_of(profile: Profile, pick: f32) -> Piece {
    var piece = pieces[profile.first];
    for (var k = 0u; k < profile.count; k++) {
        piece = pieces[profile.first + k];
        if (pick < piece.bound) {
            break;
        }
    }
    return piece;
}

struct Shape {
    rect: vec4<f32>,
    width: f32,
    height: f32,
    piece: bool,
}

// A card's picture and size: an atlas piece's cells at the profile's cell size, or
// our blades' at the meadow's height.
fn shape(pick: f32, kind: u32, meadow: f32, width: f32, height: f32, tall: f32, profile: Profile, atlas: bool) -> Shape {
    if (atlas) {
        let p = piece_of(profile, fract(pick * 131.7));
        let unit = profile.cell * mix(CELL_VARY.x, CELL_VARY.y, height) * tall * tall;
        return Shape(p.rect, unit * p.cells.x * FX_WIDEN, unit * p.cells.y, p.piece != 0u);
    }
    let w = min(meadow * mix(WIDTH.x, WIDTH.y, width), MAX_WIDTH);
    return Shape(picture_rect(picture_of(kind, pick)), w, meadow, false);
}

fn cell_index(base: u32, cell: vec2<i32>) -> u32 {
    let c = vec2<u32>(clamp(cell, vec2<i32>(0), vec2<i32>(i32(CELLS) - 1)));
    return base + c.y * CELLS + c.x;
}

// How far the ground stays grassy around `local` (metres in its tile), between the cells' centres.
fn support_at(base: u32, local: vec2<f32>) -> f32 {
    let f = local / (TILE / f32(CELLS)) - 0.5;
    let low = floor(f);
    let t = f - low;
    let a = vec2<i32>(low);
    let s = vec4<f32>(
        f32(pool[cell_index(base, a)].y >> 24u),
        f32(pool[cell_index(base, a + vec2<i32>(1, 0))].y >> 24u),
        f32(pool[cell_index(base, a + vec2<i32>(0, 1))].y >> 24u),
        f32(pool[cell_index(base, a + vec2<i32>(1, 1))].y >> 24u),
    ) * (MOST_SUPPORT / 255.0);
    return mix(mix(s.x, s.y, t.x), mix(s.z, s.w, t.x), t.y);
}

fn snorm8(b: u32) -> f32 {
    return max(f32(i32(b << 24u) >> 24u) / 127.0, -1.0);
}

fn candidate(tile: Tile, base: u32, offset: vec2<u32>, seed: u32, i: u32) {
    let keep = (f32(i) + 0.5) / f32(CANDIDATES);
    let origin = vec2<f32>(f32(tile.x), f32(tile.z)) * TILE;
    var random = hash(seed ^ hash(i));
    // The R2 sequence from the tile's offset, a little jittered.
    let r2 = offset + (i + 1u) * vec2<u32>(3242174889u, 2447445413u);
    let jitter = vec2<f32>(next(&random), next(&random)) - 0.5;
    let spacing = TILE / sqrt(f32(CANDIDATES));
    let local = clamp(vec2<f32>(r2) * (TILE / 4294967296.0) + jitter * spacing * 0.5, vec2<f32>(0.0),
        vec2<f32>(TILE - 1e-3));
    let world = origin + local;
    if (keep >= GROW_MARGIN * density(distance(cull.eye.xz, world))) {
        return;
    }
    let cell = min(vec2<u32>(local / (TILE / f32(CELLS))), vec2<u32>(CELLS - 1u));
    let w = pool[base + cell.y * CELLS + cell.x];
    let likely = f32((w.y >> 16u) & 255u) / 255.0;
    let chance = next(&random);
    if (chance >= likely) {
        return;
    }
    let turn = next(&random);
    let width = next(&random);
    let height = next(&random);
    let pick = next(&random);
    let lean = next(&random);
    let lean_turn = next(&random);
    // On the cell's ground plane.
    let n_xz = vec2<f32>(snorm8(w.y & 255u), snorm8((w.y >> 8u) & 255u));
    let n = vec3<f32>(n_xz.x, sqrt(max(1.0 - dot(n_xz, n_xz), 0.0)), n_xz.y);
    let off = local - (vec2<f32>(cell) + 0.5) * (TILE / f32(CELLS));
    let at = vec3<f32>(world.x, bitcast<f32>(w.x) - (n.x * off.x + n.z * off.y) / max(n.y, 0.1), world.y);
    let d = distance(cull.eye.xyz, at);
    if (keep >= GROW_MARGIN * density(d)) {
        return;
    }
    let tall = 0.55 + 0.95 * f32(w.z >> 24u) / 255.0;
    let dry = f32((w.w >> 16u) & 255u) / 255.0;
    let converted = cull.info.z != 0u;
    let p = w.w >> 24u;
    let profile = profiles[select(0u, p, p < arrayLength(&profiles))];
    let atlas = converted && profile.atlas != 0u;
    let kind = kind_of(pick, tall, dry);
    let meadow = min(mix(HEIGHT.x, HEIGHT.y, height) * tall * kind_height(kind),
        select(MAX_HEIGHT.x, MAX_HEIGHT.y, kind == SEEDS));
    let first = shape(pick, kind, meadow, width, height, tall, profile, atlas);
    let other = shape(fract(pick * 53.0), kind, meadow, width, height, tall, profile, atlas);
    let radius = select(max(first.width * 0.5 + first.height * (LEAN + SWAY), PATCH_RADIUS), first.width * 0.4,
        converted);
    if (support_at(base, local) < radius) {
        return;
    }
    // In view: a sphere about the clump at its widest.
    let widen = clamp(d / cull.profile.y, 1.0, cull.profile.z);
    let middle = at + vec3<f32>(0.0, first.height * 0.5, 0.0);
    let r = max(first.width, other.width) * 0.5 * widen + first.height * (0.5 + LEAN + SWAY) + 0.05;
    for (var k = 0u; k < 6u; k++) {
        let plane = cull.planes[k];
        if (dot(plane.xyz, middle) + plane.w < -r) {
            return;
        }
    }
    let bounds = select(BRIGHT, FX_BRIGHT, converted);
    let bright = clamp(f32(w.w & 65535u) / 65535.0 * 4.0 * (0.85 + 0.3 * fract(pick * 37.0)), bounds.x, bounds.y);
    let light_cell = min(vec2<u32>(local / (TILE / f32(LIGHTS))), vec2<u32>(LIGHTS - 1u));
    let l = pool[base + CELLS * CELLS + light_cell.y * LIGHTS + light_cell.x];
    let lean_angle = lean_turn * 2.0 * PI;
    let leaning = vec2<f32>(cos(lean_angle), sin(lean_angle)) * lean * LEAN * first.height;
    let angle = turn * PI;
    let across = vec2<f32>(cos(angle), sin(angle)) * first.width * 0.5;
    let slot = atomicAdd(&counter.count, 2u);
    if (slot >= cull.info.y) {
        return;
    }
    let hue = w.z & 0xffffffu;
    let fx = select(0u, 1u, converted);
    cards[slot] = GrassCard(at, first.height, across, pack2x16float(leaning), pack2x16snorm(n_xz),
        hue | select(fx, 2u, first.piece) << 24u, pack2x16float(vec2<f32>(bright, keep)), pack2x16unorm(first.rect.xy),
        pack2x16unorm(first.rect.zw), bitcast<f32>(l.x), l.y, l.zw);
    // The other card across it, a little shorter, with another picture of its kind.
    let crossed = normalize(vec2<f32>(-across.y, across.x)) * other.width * 0.5;
    cards[slot + 1u] = GrassCard(at, other.height * 0.85, crossed, pack2x16float(leaning), pack2x16snorm(n_xz),
        hue | select(fx, 2u, other.piece) << 24u, pack2x16float(vec2<f32>(bright, keep)), pack2x16unorm(other.rect.xy),
        pack2x16unorm(other.rect.zw), bitcast<f32>(l.x), l.y, l.zw);
}

@compute @workgroup_size(GROUP)
fn cs_generate(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    let tile = tiles[group.x];
    let seed = hash(bitcast<u32>(tile.x) * 73856093u ^ bitcast<u32>(tile.z) * 19349663u);
    let offset = vec2<u32>(hash(seed + 1u), hash(seed + 2u));
    let base = tile.slot * (TILE_WORDS / 4u);
    for (var i = local; i < tile.count; i += GROUP) {
        candidate(tile, base, offset, seed, i);
    }
}

// The draw's arguments: the cards made, at most the view's.
@compute @workgroup_size(1)
fn cs_finish() {
    let count = min(atomicLoad(&counter.count), cull.info.y);
    counter.index_count = count * 6u;
    counter.instance_count = 1u;
    counter.first_index = 0u;
    counter.base_vertex = 0;
    counter.first_instance = 0u;
}
