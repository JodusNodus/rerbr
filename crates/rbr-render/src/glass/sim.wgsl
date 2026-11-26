// The simulated water on the windscreen (ours, `glass/sim.rs`): beads of water in the glass's chart, stepped at a fixed
// 1/120 s by compute passes. A bead is a circle (centre, radius, velocity) in a slot of a fixed array; a grid of the
// chart's 1.5 mm cells holds the largest bead whose centre lies in each, so that a bead finds the ones it touches among
// the cells around it. A step is, in order:
//   clear_grid, integrate (the pull, the pin, the run, the blades, the shed beads, the grid), target (the touching
//   bead that beats it, by volume then slot), merge (follows the targets to their root, adds its volume and momentum to
//   the root's accumulator with integer atomics, so that the sum does not depend on the order, and dies), apply (the
//   root takes the sums), claim and create (the rain's new beads and the beads running ones shed take free slots: each
//   candidate picks a slot by hash and the highest claim wins, again independent of the order).
// Every number a slot ends with is a function of the step's inputs and the slots before it, so two runs agree to the bit.

const CAP: u32 = 65536u;
const NONE: u32 = 0xffffffffu;
const TAU: f32 = 6.2831853;

struct Step {
    // The chart's bounds (metres).
    lo: vec2<f32>,
    hi: vec2<f32>,
    // The pull on the water, m/s^2 in the chart (gravity less the car's acceleration), and the car's speed.
    gravity: vec2<f32>,
    dt: f32,
    speed: f32,
    // The blades' angle fraction before and after this step; the step's number and how many raindrops land in it.
    phase: vec2<f32>,
    tick: u32,
    spawn: u32,
    // The grid's columns and rows, and the cutters; the cell's size (metres).
    grid: vec4<u32>,
    pitch: vec4<f32>,
    // The wipers, as `gpu::Wipers::wipers`.
    wipers: array<vec4<f32>, 4>,
}

struct Bead {
    pos: vec2<f32>,
    vel: vec2<f32>,
    radius: f32,
    seed: u32,
    // The bead that absorbs this one (`NONE` for a root).
    link: u32,
    // The way it has run since it shed a bead.
    shed: f32,
}

struct Shed {
    pos: vec2<f32>,
    radius: f32,
    seed: u32,
}

struct Acc {
    volume: atomic<u32>,
    weight: atomic<u32>,
    mx: atomic<i32>,
    my: atomic<i32>,
    vx: atomic<i32>,
    vy: atomic<i32>,
    pad0: u32,
    pad1: u32,
}

struct Cutter {
    start: vec2<f32>,
    end: vec2<f32>,
    radius: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}

@group(0) @binding(0) var<uniform> step: Step;
@group(0) @binding(1) var<storage, read_write> beads: array<Bead>;
@group(0) @binding(2) var<storage, read_write> grid: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> acc: array<Acc>;
@group(0) @binding(4) var<storage, read_write> claims: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> sheds: array<Shed>;
@group(0) @binding(6) var mask: texture_2d<f32>;
@group(0) @binding(7) var<storage, read> cutters: array<Cutter>;

// The beads. The largest a bead grows (what it gathers beyond runs off), the smallest that stays, and how near two
// beads' edges must be to run together (of the sum of their radii).
const RMAX: f32 = 0.0055;
const RMIN: f32 = 0.0002;
const TOUCH: f32 = 0.78;

// What lands: the radius range (metres, the cube of a uniform number across it), and what skids on.
const RAIN_RADIUS: vec2<f32> = vec2<f32>(0.0004, 0.0022);
const SKID: f32 = 0.002;
const SKID_MOST: f32 = 0.2;

// The pull on a bead as `model.rs` has it (`PIN`, `AIR`, `MOBILITY`, `MOST_SPEED`, `SLIDE_RESPONSE`, ...): a bead holds
// until the pull passes `PIN` (2 mm / r)^2 times its grip, runs by `MOBILITY` of the excess at most `MOST_SPEED`, and
// holds again under `HOLD_SPEED`; the airflow pushes up and out by `AIR` speed^2 / (r in mm).
const PIN: f32 = 16.0;
const PIN_RADIUS: f32 = 0.002;
const PATCHY: f32 = 0.6;
const PATCH: f32 = 0.004;
const PATCH_TURN: f32 = 0.6;
const GRIP: vec2<f32> = vec2<f32>(0.7, 1.5);
const RUNNING_PIN: f32 = 0.85;
const AIR: f32 = 0.05;
const AIR_SIDEWAYS: f32 = 0.35;
const MOBILITY: f32 = 0.004;
const MOST_SPEED: f32 = 0.2;
const SLIDE_RESPONSE: f32 = 0.15;
const HOLD_SPEED: f32 = 0.004;

// A running bead sheds a bead of this share of its radius every this many of its radii (from, to) of run, and only
// from `SHED_FROM`.
const SHED_RADIUS: vec2<f32> = vec2<f32>(0.1, 0.25);
const SHED_EVERY: vec2<f32> = vec2<f32>(2.5, 7.0);
const SHED_FROM: f32 = 0.0018;

// The blade pushes what lies within this of its line to its leading edge, metres.
const BLADE_PAD: f32 = 0.003;

fn pcg(n: u32) -> u32 {
    let v = n * 747796405u + 2891336453u;
    let w = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (w >> 22u) ^ w;
}

fn unit(n: u32) -> f32 {
    return f32(pcg(n) >> 8u) / 16777216.0;
}

// The `noise` of `model.rs`: a value noise, -1 to 1.
fn vnoise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = p - cell;
    let s = f * f * (3.0 - 2.0 * f);
    let c = vec2<i32>(cell);
    var v = vec4<f32>(0.0);
    for (var k = 0u; k < 4u; k++) {
        let x = u32(c.x + i32(k & 1u));
        let y = u32(c.y + i32(k >> 1u));
        var h = (x * 0x8da6b343u) ^ (y * 0xd8163841u);
        h ^= h >> 15u;
        h = h * 0x2c1b3c6du;
        h ^= h >> 12u;
        v[k] = f32(h >> 8u) / 16777216.0;
    }
    let low = mix(v.x, v.y, s.x);
    let high = mix(v.z, v.w, s.x);
    return mix(low, high, s.y) * 2.0 - 1.0;
}

// A bead's volume as a whole number (its radius in tenths of a millimetre, cubed): what the atomics add.
fn volume(r: f32) -> u32 {
    let x = r * 10000.0;
    return u32(x * x * x + 0.5);
}

fn radius_of(v: f32) -> f32 {
    return pow(max(v, 0.0), 1.0 / 3.0) * 0.0001;
}

fn cell_of(pos: vec2<f32>) -> vec2<i32> {
    let c = vec2<i32>(floor((pos - step.lo) / step.pitch.x));
    return clamp(c, vec2<i32>(0), vec2<i32>(i32(step.grid.x) - 1, i32(step.grid.y) - 1));
}

fn on_glass(pos: vec2<f32>) -> bool {
    let f = floor((pos - step.lo) / step.pitch.x);
    if (f.x < 0.0 || f.y < 0.0 || f.x >= f32(step.grid.x) || f.y >= f32(step.grid.y)) {
        return false;
    }
    return textureLoad(mask, vec2<i32>(f), 0).r > 0.5;
}

@compute @workgroup_size(64)
fn clear_grid(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < step.grid.x * step.grid.y) {
        atomicStore(&grid[i], 0u);
    }
}

// Where the blades have gone in this step, a bead in their way is pushed ahead of the blade: to its leading edge, a
// hair in front of it, which the next step pushes on, and which a stroke's turn leaves lying.
fn push(pos: vec2<f32>) -> vec3<f32> {
    let p0 = step.phase.x;
    let p1 = step.phase.y;
    if (abs(p1 - p0) < 1e-6) {
        return vec3<f32>(pos, 0.0);
    }
    for (var i = 0; i < 2; i++) {
        let w = step.wipers[i * 2];
        let s = step.wipers[i * 2 + 1];
        if (s.z == 0.0) {
            continue;
        }
        let d = pos - w.xy;
        let r = length(d);
        if (r < w.z || r > w.w) {
            continue;
        }
        let a = atan2(d.y, d.x) - s.x;
        let share = (a - TAU * round(a / TAU)) * sign(s.y) / max(abs(s.y), 1e-3);
        let pad = BLADE_PAD / (r * abs(s.y));
        if (share >= min(p0, p1) - pad && share <= max(p0, p1) + pad) {
            let edge = p1 + sign(p1 - p0) * pad * 1.05;
            let angle = s.x + edge * s.y;
            return vec3<f32>(w.xy + r * vec2<f32>(cos(angle), sin(angle)), 1.0);
        }
    }
    return vec3<f32>(pos, 0.0);
}

@compute @workgroup_size(64)
fn integrate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= CAP) {
        return;
    }
    atomicStore(&claims[i], 0u);
    sheds[i].radius = 0.0;
    var b = beads[i];
    if (b.radius <= 0.0) {
        return;
    }
    if (!on_glass(b.pos) || b.radius < RMIN) {
        beads[i].radius = 0.0;
        return;
    }
    let dt = step.dt;
    let r = b.radius;
    // The airflow pushes up and out from the middle of the glass.
    let half_width = max((step.hi.x - step.lo.x) * 0.5, 1e-3);
    let outward = clamp((b.pos.x - (step.lo.x + step.hi.x) * 0.5) / half_width, -1.0, 1.0);
    let air = AIR * step.speed * step.speed / (r * 1000.0);
    let pull = step.gravity + vec2<f32>(outward * AIR_SIDEWAYS, 1.0) * air;
    let grip = mix(GRIP.x, GRIP.y, f32(b.seed & 255u) / 256.0)
        * (1.0 + PATCHY * vnoise(vec2<f32>(cos(PATCH_TURN) * b.pos.x - sin(PATCH_TURN) * b.pos.y,
            sin(PATCH_TURN) * b.pos.x + cos(PATCH_TURN) * b.pos.y) / PATCH));
    let pin = PIN * (PIN_RADIUS / r) * (PIN_RADIUS / r) * grip;
    let holding = b.vel.x == 0.0 && b.vel.y == 0.0;
    let beyond = length(pull) - pin * select(RUNNING_PIN, 1.0, holding);
    let mobility = MOBILITY * (r * 500.0) * (r * 500.0);
    var towards = vec2<f32>(0.0);
    if (beyond > 0.0) {
        towards = normalize(pull) * min(beyond * mobility, MOST_SPEED);
    }
    b.vel += (towards - b.vel) * (1.0 - exp(-dt / SLIDE_RESPONSE));
    if (beyond <= 0.0 && length(b.vel) < HOLD_SPEED) {
        b.vel = vec2<f32>(0.0);
    }
    let moved = b.vel * dt;
    b.pos += moved;
    // The blades.
    let pushed = push(b.pos);
    if (pushed.z != 0.0) {
        b.pos = pushed.xy;
        b.vel = vec2<f32>(0.0);
        b.shed = 0.0;
    }
    // A runner sheds small beads behind it.
    let way = length(moved);
    if (way > 0.0) {
        b.shed += way;
        let h = unit(b.seed ^ (step.tick * 0x9e3779b1u));
        let every = r * mix(SHED_EVERY.x, SHED_EVERY.y, f32((b.seed >> 8u) & 255u) / 256.0);
        if (b.shed >= every && r >= SHED_FROM) {
            let s = r * mix(SHED_RADIUS.x, SHED_RADIUS.y, h);
            let along = moved / way;
            let side = vec2<f32>(-along.y, along.x) * (unit(b.seed ^ 0x51ed270bu ^ step.tick) - 0.5) * r * 1.2;
            sheds[i] = Shed(b.pos - along * r * 1.1 + side, s, pcg(b.seed + step.tick));
            b.radius = pow(max(r * r * r - s * s * s, 0.0), 1.0 / 3.0);
            b.shed = 0.0;
        }
    }
    b.link = NONE;
    beads[i] = b;
    let c = cell_of(b.pos);
    let rq = min(u32(b.radius * 100000.0) + 1u, 65535u);
    atomicMax(&grid[u32(c.y) * step.grid.x + u32(c.x)], (rq << 16u) | i);
}

// The beads each bead can touch lie within `TOUCH` of the sum of their radii, among the grid cells within that reach of
// it (up to the largest bead's).
@compute @workgroup_size(64)
fn find_target(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= CAP) {
        return;
    }
    let b = beads[i];
    if (b.radius <= 0.0) {
        return;
    }
    let mine = volume(b.radius);
    let reach = min(i32(ceil(TOUCH * (b.radius + RMAX) / step.pitch.x)), 8);
    let c = cell_of(b.pos);
    var best = NONE;
    var best_volume = 0u;
    for (var dy = -reach; dy <= reach; dy++) {
        let y = c.y + dy;
        if (y < 0 || y >= i32(step.grid.y)) {
            continue;
        }
        for (var dx = -reach; dx <= reach; dx++) {
            let x = c.x + dx;
            if (x < 0 || x >= i32(step.grid.x)) {
                continue;
            }
            let word = atomicLoad(&grid[u32(y) * step.grid.x + u32(x)]);
            let j = word & 0xffffu;
            if (word == 0u || j == i) {
                continue;
            }
            let o = beads[j];
            if (o.radius <= 0.0) {
                continue;
            }
            let theirs = volume(o.radius);
            if (!(theirs > mine || (theirs == mine && j > i))) {
                continue;
            }
            if (distance(b.pos, o.pos) < TOUCH * (b.radius + o.radius)
                && (theirs > best_volume || (theirs == best_volume && j > best))) {
                best = j;
                best_volume = theirs;
            }
        }
    }
    beads[i].link = best;
}

// A bead with a target follows the targets to the root and adds itself to the root's sums; it is gone.
@compute @workgroup_size(64)
fn merge(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= CAP) {
        return;
    }
    let b = beads[i];
    if (b.radius <= 0.0 || b.link == NONE) {
        return;
    }
    var root = b.link;
    for (var hop = 0; hop < 6; hop++) {
        let next = beads[root].link;
        if (next == NONE) {
            break;
        }
        root = next;
    }
    let v = volume(b.radius);
    let w = max(v / 16u, 1u);
    let off = vec2<i32>(round((b.pos - beads[root].pos) * 16000.0));
    let vel = vec2<i32>(round(b.vel * 1000.0));
    atomicAdd(&acc[root].volume, v);
    atomicAdd(&acc[root].weight, w);
    atomicAdd(&acc[root].mx, i32(w) * off.x);
    atomicAdd(&acc[root].my, i32(w) * off.y);
    atomicAdd(&acc[root].vx, i32(w) * vel.x);
    atomicAdd(&acc[root].vy, i32(w) * vel.y);
    beads[i].radius = 0.0;
}

@compute @workgroup_size(64)
fn apply(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= CAP) {
        return;
    }
    let extra = atomicLoad(&acc[i].volume);
    if (extra == 0u) {
        return;
    }
    var b = beads[i];
    let own = max(volume(b.radius) / 16u, 1u);
    let weight = f32(own + atomicLoad(&acc[i].weight));
    b.pos += vec2<f32>(f32(atomicLoad(&acc[i].mx)), f32(atomicLoad(&acc[i].my))) / (16000.0 * weight);
    b.vel = (b.vel * 1000.0 * f32(own) + vec2<f32>(f32(atomicLoad(&acc[i].vx)), f32(atomicLoad(&acc[i].vy)))) / (weight * 1000.0);
    b.radius = min(radius_of(f32(volume(b.radius) + extra)), RMAX);
    b.shed = 0.0;
    beads[i] = b;
    atomicStore(&acc[i].volume, 0u);
    atomicStore(&acc[i].weight, 0u);
    atomicStore(&acc[i].mx, 0);
    atomicStore(&acc[i].my, 0);
    atomicStore(&acc[i].vx, 0);
    atomicStore(&acc[i].vy, 0);
}

// The candidates for a free slot: the step's raindrops, then the beads shed (one per slot of the bead that shed it).
// Each picks a slot by hash, the second try if the first is taken, and the highest candidate number claims it.
fn slot_of(k: u32) -> u32 {
    let h = pcg(step.tick * 2654435761u + k * 40503u + 17u);
    var slot = h & 0xffffu;
    if (beads[slot].radius > 0.0) {
        slot = (pcg(h) >> 3u) & 0xffffu;
        if (beads[slot].radius > 0.0) {
            return NONE;
        }
    }
    return slot;
}

// Candidate `k`: the bead it would make (radius 0 for none).
fn candidate(k: u32) -> Shed {
    if (k < step.spawn) {
        let n = step.tick * 0x9e3779b1u + k * 0x85ebca6bu;
        let u = unit(n);
        let at = step.lo + (step.hi - step.lo) * vec2<f32>(unit(n ^ 0x68e31da4u), unit(n ^ 0xb5297a4du));
        if (!on_glass(at)) {
            return Shed(at, 0.0, 0u);
        }
        return Shed(at, mix(RAIN_RADIUS.x, RAIN_RADIUS.y, u * u * u), pcg(n ^ 0x1b873593u));
    }
    return sheds[k - step.spawn];
}

@compute @workgroup_size(64)
fn claim(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if (k >= step.spawn + CAP) {
        return;
    }
    if (candidate(k).radius <= 0.0) {
        return;
    }
    let slot = slot_of(k);
    if (slot != NONE) {
        atomicMax(&claims[slot], k + 1u);
    }
}

@compute @workgroup_size(64)
fn create(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if (k >= step.spawn + CAP) {
        return;
    }
    let c = candidate(k);
    if (c.radius <= 0.0) {
        return;
    }
    let slot = slot_of(k);
    if (slot == NONE || atomicLoad(&claims[slot]) != k + 1u) {
        return;
    }
    // A raindrop skids on along the airflow at the car's speed; a shed bead stays.
    var vel = vec2<f32>(0.0);
    if (k < step.spawn) {
        let half_width = max((step.hi.x - step.lo.x) * 0.5, 1e-3);
        let outward = clamp((c.pos.x - (step.lo.x + step.hi.x) * 0.5) / half_width, -1.0, 1.0);
        vel = normalize(vec2<f32>(outward * AIR_SIDEWAYS, 1.0)) * min(SKID * step.speed, SKID_MOST);
    }
    beads[slot] = Bead(c.pos, vel, c.radius, c.seed, NONE, 0.0);
}

// The running drops of the model cut through the beads on their path: a thread each, over the cells their stretch
// covers (the grid of the last step).
@compute @workgroup_size(64)
fn cut(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if (k >= step.grid.z) {
        return;
    }
    let cutter = cutters[k];
    let lo = cell_of(min(cutter.start, cutter.end) - vec2<f32>(cutter.radius + 0.004));
    let hi = cell_of(max(cutter.start, cutter.end) + vec2<f32>(cutter.radius + 0.004));
    let line = cutter.end - cutter.start;
    let along = max(dot(line, line), 1e-12);
    for (var y = lo.y; y <= hi.y; y++) {
        for (var x = lo.x; x <= hi.x; x++) {
            let word = atomicLoad(&grid[u32(y) * step.grid.x + u32(x)]);
            let j = word & 0xffffu;
            if (word == 0u) {
                continue;
            }
            let b = beads[j];
            if (b.radius <= 0.0) {
                continue;
            }
            let t = clamp(dot(b.pos - cutter.start, line) / along, 0.0, 1.0);
            if (distance(b.pos, cutter.start + line * t) < cutter.radius + b.radius * 0.5) {
                beads[j].radius = 0.0;
            }
        }
    }
}
