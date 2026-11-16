// The stage's lamps' bulbs (`stage_lamps.rs`, `stage_lamps_pass.rs`, ours): a soft disc facing the eye at each lamp,
// added to the frame. The game's painted beams are not drawn: the lamps' light shows on the stage (`street_sum`) and
// in the rain, the mist and the particles (`lamps_at` in `effects.wgsl`).

struct View {
    view_projection: mat4x4<f32>,
    // The eye, stage space, and w the renderer's units per cd/m2 (`pre_exposure` in `shader.wgsl`).
    eye: vec4<f32>,
    // The lamps' colour times how lit they are (0 by day, 1 at night).
    colour: vec4<f32>,
    // The stage's linear fog: x its start, y its end, metres; z 1 when it is on.
    fog: vec4<f32>,
    // x the target's height, pixels.
    screen: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;

struct VertexIn {
    // xyz the bulb's centre, w its radius, metres.
    @location(0) bulb: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // The corner, -1 to 1 across the drawn disc.
    @location(0) at: vec2<f32>,
    // The bulb's share of the drawn disc's radius (below 1 where it is held at `LEAST_PIXELS`).
    @location(1) @interpolate(flat) share: f32,
    @location(2) @interpolate(flat) fogged: f32,
}

// A bulb's luminance, cd/m2 (a street lamp's bowl, past the tone curve's white so that it glares), and the fewest
// pixels its drawn radius takes, so that a far lamp stays a steady point.
const BULB: f32 = 4000.0;
const LEAST_PIXELS: f32 = 1.5;
const NEAR_EYE: f32 = 0.5;

@vertex
fn vs_bulb(@builtin(vertex_index) corner: u32, v: VertexIn) -> VertexOut {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[corner];
    // Drawn `NEAR_EYE` toward the eye (no farther than halfway), where it shows in the same place, so that the lamp's
    // housing about it does not cut the disc.
    let to_eye = view.eye.xyz - v.bulb.xyz;
    let pull = min(NEAR_EYE, 0.5 * length(to_eye));
    let centre = view.view_projection * vec4<f32>(v.bulb.xyz + normalize(to_eye) * pull, 1.0);
    // Clip units per metre across and up the view (the rows' lengths: the view's rotation is orthonormal).
    let m = view.view_projection;
    let focal = vec2<f32>(length(vec3<f32>(m[0][0], m[1][0], m[2][0])), length(vec3<f32>(m[0][1], m[1][1], m[2][1])));
    let least = LEAST_PIXELS * 2.0 * centre.w / (view.screen.x * focal.y);
    let radius = max(v.bulb.w, least);
    var out: VertexOut;
    out.clip = vec4<f32>(centre.xy + c * radius * focal, centre.zw);
    out.at = c;
    out.share = v.bulb.w / radius;
    let d = distance(v.bulb.xyz, view.eye.xyz);
    out.fogged = 1.0;
    if (view.fog.z != 0.0) {
        out.fogged = clamp((view.fog.y - d) / max(view.fog.y - view.fog.x, 1.0), 0.0, 1.0);
    }
    return out;
}

@fragment
fn fs_bulb(v: VertexOut) -> @location(0) vec4<f32> {
    // A soft disc whose light is the bulb's: spread over the drawn disc when that is larger.
    let r2 = dot(v.at, v.at);
    let disc = exp(-4.0 * r2) * (1.0 - smoothstep(0.8, 1.0, r2));
    let luminance = BULB * disc * v.share * v.share;
    return vec4<f32>(view.colour.rgb * luminance * view.eye.w * v.fogged, 0.0);
}
