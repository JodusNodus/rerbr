
// The simulated beads (ours, `glass/sim.rs`; appended to `glass.wgsl`): one instance per slot of the simulation's bead
// array, a quad on the glass drawn as a water lens like a drop (`fs_main`, kind 0), at the screen's resolution. The
// slot's chart place is put on the curved glass by the film's corners, seen from the frame's camera (`SimView`).

struct SimBead {
    pos: vec2<f32>,
    vel: vec2<f32>,
    radius: f32,
    seed: u32,
    link: u32,
    shed: f32,
}

// The windscreen's film corners in stage space (`gpu::Wipers::film`'s grid, columns + 1 by rows + 1, row by row), the
// pane's across and up in stage space, and the chart's bounds.
struct SimView {
    across: vec4<f32>,
    up: vec4<f32>,
    lo: vec2<f32>,
    hi: vec2<f32>,
    corners: array<vec4<f32>, 1225>,
}

@group(0) @binding(10) var<storage, read> sim_beads: array<SimBead>;
@group(0) @binding(11) var<uniform> sim_view: SimView;
const SIM_COLUMNS: f32 = 48.0;
const SIM_ROWS: f32 = 24.0;
// A running bead is drawn this much longer along its run per m/s (`glass::drawn::STRETCH`).
const SIM_STRETCH: f32 = 6.0;
// A bead's quad lies this far off the glass, metres, above its film.
const SIM_LIFT: f32 = 0.0;

@vertex
fn vs_bead(@builtin(vertex_index) corner: u32, @builtin(instance_index) slot: u32) -> Out {
    let b = sim_beads[slot];
    var out: Out;
    if (b.radius <= 0.0) {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let f = (b.pos - sim_view.lo) / (sim_view.hi - sim_view.lo) * vec2<f32>(SIM_COLUMNS, SIM_ROWS);
    let i = clamp(floor(f), vec2<f32>(0.0), vec2<f32>(SIM_COLUMNS - 1.0, SIM_ROWS - 1.0));
    let t = f - i;
    let k = u32(i.y) * u32(SIM_COLUMNS + 1.0) + u32(i.x);
    let row = u32(SIM_COLUMNS) + 1u;
    let low = mix(sim_view.corners[k], sim_view.corners[k + 1u], t.x);
    let high = mix(sim_view.corners[k + row], sim_view.corners[k + row + 1u], t.x);
    let centre = mix(low, high, t.y).xyz;
    let run = length(b.vel);
    var across = sim_view.across.xyz;
    var up = sim_view.up.xyz;
    var u = across * b.radius;
    var v = up * b.radius;
    if (run > 0.0) {
        let along = b.vel / run;
        let side = vec2<f32>(-along.y, along.x);
        u = (across * side.x + up * side.y) * b.radius;
        v = (across * along.x + up * along.y) * b.radius * (1.0 + run * SIM_STRETCH);
    }
    var instance: Instance;
    instance.centre = centre;
    instance.kind = 0.0;
    instance.axis_u = u;
    instance.seed = f32(b.seed >> 8u) / 16777216.0;
    instance.axis_v = v;
    instance.alpha = 1.0;
    instance.color = vec4<f32>(1.0);
    instance.normal = vec4<f32>(0.0);
    instance.far = vec3<f32>(0.0);
    instance.tint = vec4<u32>(0u);
    instance.edge = vec4<f32>(0.0);
    return place_instance(corner, instance);
}

@fragment
fn fs_bead(in: Out) -> @location(0) vec4<f32> {
    let size = max(length(in.pixels.xy), 0.5);
    let rr = outline(in.local, in.seed, mix(DROP_RAGGED.x, DROP_RAGGED.y, smoothstep(2.0, 6.0, size)));
    if (rr > 1.0) {
        discard;
    }
    let q = in.local * (rr / max(length(in.local), 1e-6));
    let run = max(length(in.pixels.zw) / size, 1.0);
    let o = in.pixels.xy * q.x + in.pixels.zw * q.y;
    let look = in.pixels.xy * q.x + in.pixels.zw * (q.y / run);
    return lens(screen_up(o, rr), o, look, size, in.centre.xy, LENS_MAG, 0.6, 1.0, in.seed)
        * (1.0 - smoothstep(1.0 - 0.8 / size, 1.0, rr));
}
