// The stage's lamps' sprites (`stage_lamps.rs`, `stage_lamps_pass.rs`, ours): the light in the air under each lamp,
// added to the frame. A sprite's alpha is the light's shape; its brightest texels are the lamp itself.

struct View {
    view_projection: mat4x4<f32>,
    // The eye, stage space, and w the renderer's units per cd/m2 (`pre_exposure` in `shader.wgsl`).
    eye: vec4<f32>,
    // The lamps' colour times how lit they are (0 by day, 1 at night).
    colour: vec4<f32>,
    // The stage's linear fog: x its start, y its end, metres; z 1 when it is on.
    fog: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var sprite_texture: texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) world: vec3<f32>,
}

@vertex
fn vs_sprite(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = view.view_projection * vec4<f32>(v.pos, 1.0);
    out.uv = v.uv;
    out.world = v.pos;
    return out;
}

// The light in the air, cd/m2, where the sprite is opaque: a lamp's glow lit in the haze about it (`HAZE`), and the
// lamp itself where the alpha nears its peak (`BULB`, a street lamp's bowl, past the tone curve's white so that it
// glares).
const HAZE: f32 = 6.0;
const BULB: f32 = 4000.0;

@fragment
fn fs_sprite(v: VertexOut) -> @location(0) vec4<f32> {
    let a = textureSample(sprite_texture, sprite_sampler, v.uv).a;
    let d = distance(v.world, view.eye.xyz);
    var fogged = 1.0;
    if (view.fog.z != 0.0) {
        fogged = clamp((view.fog.y - d) / max(view.fog.y - view.fog.x, 1.0), 0.0, 1.0);
    }
    let a4 = a * a * a * a;
    let luminance = HAZE * a + BULB * a4 * a4;
    return vec4<f32>(view.colour.rgb * luminance * view.eye.w * fogged, 0.0);
}
