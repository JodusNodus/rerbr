// Motion blur along each pixel's screen motion (`motion_blur.rs`, ours, experimental): one full-screen pass at the
// output's size that gathers the resolved frame (`scene`: the temporal anti-aliasing wrote it here instead of the
// post pass's scene) along each pixel's blur vector into the post pass's scene. The blur vector is the pixel's
// velocity in uv per frame times the exposure's scale over the frame time (`blur.x`), centred on the pixel (half
// before, half after) and capped at `MOST` of the output height. The velocity is what `previous_uv` in `taa.wgsl`
// finds: where the motion target's z is over 0.5 its xy, else the point from the reversed depth carried by the
// resolve's `view_to_prev`. The motion target and the depth are at the render's size, the scene at the output's:
// they are read with `textureLoad` at the pixel scaled to their size. Eleven taps along the vector (the centre plus
// 5 each side), their positions offset by the pixel's interleaved gradient noise (as `volume_jitter` in
// `cloud_volume.wgsl`) advanced each frame. The centre tap has weight 1; each other tap's weight is its blur vector's
// length over the centre's, so a pixel that does not move on screen (the car in the chase view, the cabin in the
// cockpit view) neither blurs nor bleeds into the moving road beside it. A blur vector shorter than half an output
// pixel returns the scene unchanged. Taps outside the screen are clamped to its edge.

struct Taa {
    // The current frame's clip space (unjittered) to the previous frame's, for a point that stays put.
    view_to_prev: mat4x4<f32>,
    // The frame's size: w, h, 1/w, 1/h.
    size: vec4<f32>,
    // The output's (the scene's): w, h, 1/w, 1/h.
    output: vec4<f32>,
    // xy the frame's jitter, its pixels; z, w the upscale's.
    jitter: vec4<f32>,
    // x the new frame's base share, y the clip box's half size, z 1 = reset, w the debug view.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> t: Taa;
// The resolved frame.
@group(0) @binding(1) var scene: texture_2d<f32>;
// The frame's depth (reversed).
@group(0) @binding(2) var depth: texture_depth_2d;
// The motion pass's (`renderer/motion.rs`): xy the uv the point was at less the uv it is at, z 0 where nothing moving
// was drawn, 1 where something was and 2 where the car's meshes were.
@group(0) @binding(3) var motion: texture_2d<f32>;
@group(0) @binding(4) var linear_sampler: sampler;
// x the velocity's scale into the blur vector (the exposure over the frame time, 0 without strength), y the frame's
// number.
@group(0) @binding(5) var<uniform> blur: vec4<f32>;

// The longest blur vector, of the output height.
const MOST: f32 = 0.05;

struct Out {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) index: u32) -> Out {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Out(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0));
}

// The render-size pixel `uv` falls in, clamped to the target.
fn render_pixel(uv: vec2<f32>) -> vec2<i32> {
    let last = vec2<i32>(t.size.xy) - 1;
    return clamp(vec2<i32>(uv * t.size.xy), vec2<i32>(0), last);
}

// The pixel's velocity in uv per frame: the motion target's where something moving was drawn, else the point from the
// depth carried by the previous frame's unjittered view-projection, as `previous_uv` in `taa.wgsl` finds it.
fn velocity_at(pixel: vec2<i32>, uv: vec2<f32>) -> vec2<f32> {
    let moved = textureLoad(motion, pixel, 0).xyz;
    if (moved.z > 0.5) {
        return moved.xy;
    }
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let here = vec4<f32>(ndc, textureLoad(depth, pixel, 0), 1.0);
    let clip = t.view_to_prev * here;
    let then = clip.xy / max(clip.w, 1.0e-6);
    return vec2<f32>(then.x * 0.5 + 0.5, 0.5 - then.y * 0.5) - uv;
}

// Capped at MOST of the output height.
fn capped(v: vec2<f32>) -> vec2<f32> {
    let length = length(v);
    return select(v, v * (MOST / max(length, 1.0e-6)), length > MOST);
}

// Interleaved gradient noise (Jimenez 2014), as `volume_jitter` in `cloud_volume.wgsl`.
fn noise(p: vec2<f32>) -> f32 {
    let q = p + 5.588238 * (blur.y % 64.0);
    return fract(52.9829189 * fract(dot(q, vec2<f32>(0.06711056, 0.00583715))));
}

fn load_scene(pixel: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(scene)) - 1;
    // The float target holds what the passes wrote, NaN included: kept to 0..64.
    let c = textureLoad(scene, clamp(pixel, vec2<i32>(0), last), 0).rgb;
    return clamp(select(c, vec3<f32>(0.0), c != c), vec3<f32>(0.0), vec3<f32>(64.0));
}

@fragment
fn fs_blur(in: Out) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(in.position.xy);
    let uv = in.position.xy * t.output.zw;
    let centre = load_scene(pixel);
    var v = capped(velocity_at(render_pixel(uv), uv) * blur.x);
    // No strength on a frame whose history reset (a camera cut), without a frame time, or with a blur vector
    // shorter than half an output pixel: the scene unchanged.
    if (t.params.z > 0.5 || blur.x == 0.0 || length(v) < 0.5 * t.output.w) {
        return vec4<f32>(centre, textureLoad(scene, pixel, 0).a);
    }
    let shift = noise(in.position.xy) - 0.5;
    let vp = max(length(v), 1.0e-6);
    var sum = centre;
    var weight = 1.0;
    for (var i = -5; i <= 5; i++) {
        if (i == 0) {
            continue;
        }
        // Taps outside the screen are clamped to its edge.
        let tap = clamp(uv + v * ((f32(i) + shift) / 10.0), vec2<f32>(0.0), vec2<f32>(1.0));
        let vt = capped(velocity_at(render_pixel(tap), tap) * blur.x);
        let w = clamp(length(vt) / vp, 0.0, 1.0);
        // The float target holds what the passes wrote, NaN included: kept to 0..64.
        let c = textureSampleLevel(scene, linear_sampler, tap, 0.0).rgb;
        sum += w * clamp(select(c, vec3<f32>(0.0), c != c), vec3<f32>(0.0), vec3<f32>(64.0));
        weight += w;
    }
    return vec4<f32>(sum / weight, textureLoad(scene, pixel, 0).a);
}
