// Ground-truth ambient occlusion in place of the contact shading (`renderer::gtao`, `Enhance::gtao`, ours, a proof of
// concept), appended to `shader.wgsl`. After Jimenez et al. 2016, "Practical Real-Time Strategies for Accurate Indirect
// Occlusion", and Intel's XeGTAO (MIT) for the slice set-up: per half-size texel a few slices through the view vector, in
// each the highest horizon either way from the depth, integrated against the face's cosine. Never the sun's: the stage
// reads it on the sky's and bounce's light only (`contact_at`, `combine_ao`).
//
// Three passes over the 2x2 pixels' top left pixel: `fs_gtao` (raw, noisy: the slices turned and the steps jittered by
// the pixel and the frame), `fs_gtao_resolve` (3x3 depth-aware blur, then the previous frame's result reprojected,
// clamped to the blur's range, into the texture the stage reads: the visibility and the view distance it was worked
// out at, for the bilateral upsample in `contact_at`).

struct GtaoRun {
    // The previous frame's `frame.view_projection`.
    previous: mat4x4<f32>,
    // `x` the frame's number, `y` the radius in metres, `z` the power the visibility is raised to, `w` 1 where there is
    // no history (the first frame, a resize).
    run: vec4<f32>,
}

@group(1) @binding(14) var gtao_raw: texture_2d<f32>;
@group(1) @binding(15) var gtao_history: texture_2d<f32>;
@group(1) @binding(16) var<uniform> gtao: GtaoRun;

const GTAO_SLICES: i32 = 2;
const GTAO_STEPS: i32 = 5;
// The most the radius spans in pixels (the taps past it cost cache misses).
const GTAO_MOST_PIXELS: f32 = 64.0;
// Where the horizon's weight starts to fall off, as a share of the radius (XeGTAO's 0.615 of it ends at zero).
const GTAO_FALLOFF: f32 = 0.615;
const GTAO_PI: f32 = 3.14159265;

fn gtao_ign(p: vec2<i32>, phase: f32, shift: f32) -> f32 {
    let q = vec2<f32>(p) + 5.588238 * phase + shift;
    return fract(52.9829189 * fract(dot(q, vec2<f32>(0.06711056, 0.00583715))));
}

// The view distance at `world` (the clip w).
fn gtao_depth(world: vec3<f32>) -> f32 {
    let m = frame.view_projection;
    return m[0].w * world.x + m[1].w * world.y + m[2].w * world.z + m[3].w;
}

@fragment
fn fs_gtao(v: SkyOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(v.clip.xy) * 2;
    let depth = textureLoad(scene_depth, p, 0);
    if (depth == 0.0) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }
    let world = mask_world(p, depth);
    let at = gtao_depth(world);
    // The cabin's creases keep their own, smaller shading (`contact_light`).
    if (in_car() && length(world - frame.eye.xyz) < CABIN_CONTACT_REACH) {
        return vec4<f32>(contact_light(p, world, CABIN_CONTACT_RADIUS, CABIN_CONTACT_STRENGTH), at, 0.0, 1.0);
    }
    let size = vec2<i32>(textureDimensions(scene_depth));
    let m = frame.view_projection;
    let radius = gtao.run.y;
    let pixels = radius * 0.5 * f32(size.y) * length(vec3<f32>(m[0].y, m[1].y, m[2].y)) / max(at, 1e-3);
    if (pixels < 2.0) {
        return vec4<f32>(1.0, at, 0.0, 1.0);
    }
    let reach = min(pixels, GTAO_MOST_PIXELS);

    // The face's normal from the depth (as `contact_light`), toward the eye.
    var normal = cross(depth_step(world, p, vec2<i32>(0, 1)), depth_step(world, p, vec2<i32>(1, 0)));
    normal = select(vec3<f32>(0.0, 1.0, 0.0), normalize(normal), dot(normal, normal) > 1e-12);
    let to_eye = normalize(frame.eye.xyz - world);
    normal *= select(-1.0, 1.0, dot(normal, to_eye) >= 0.0);
    // What a pixel step right and down is in the world at this depth.
    let right = mask_world(p + vec2<i32>(1, 0), depth) - world;
    let down = mask_world(p + vec2<i32>(0, 1), depth) - world;

    let falloff_range = GTAO_FALLOFF * radius;
    let falloff_mul = -1.0 / falloff_range;
    let falloff_add = (radius - falloff_range) / falloff_range + 1.0;

    let phase = gtao.run.x;
    let slice_noise = gtao_ign(p, phase, 0.0);
    let step_noise = gtao_ign(p, phase, 37.0);
    var visibility = 0.0;
    for (var k = 0; k < GTAO_SLICES; k++) {
        let phi = (f32(k) + slice_noise) * GTAO_PI / f32(GTAO_SLICES);
        let along = vec2<f32>(cos(phi), sin(phi));
        let direction = normalize(right * along.x + down * along.y);
        let ortho = direction - dot(direction, to_eye) * to_eye;
        let axis = normalize(cross(ortho, to_eye));
        let projected = normal - axis * dot(normal, axis);
        let projected_length = max(length(projected), 1e-4);
        let cos_n = clamp(dot(projected, to_eye) / projected_length, 0.0, 1.0);
        let n = sign(dot(ortho, projected)) * acos(cos_n);
        // The horizons' cosines start at the face's own plane.
        var horizon_plus = cos(n + 0.5 * GTAO_PI);
        var horizon_minus = cos(n - 0.5 * GTAO_PI);
        let low_plus = horizon_plus;
        let low_minus = horizon_minus;
        for (var s = 0; s < GTAO_STEPS; s++) {
            let t = (f32(s) + step_noise) / f32(GTAO_STEPS);
            let offset = vec2<i32>(round(along * (1.5 + t * t * reach)));
            for (var side = 0; side < 2; side++) {
                let q = select(p - offset, p + offset, side == 0);
                if (any(q < vec2<i32>(0)) || any(q >= size)) {
                    continue;
                }
                let d = textureLoad(scene_depth, q, 0);
                if (d == 0.0) {
                    continue;
                }
                let delta = mask_world(q, d) - world;
                let length_delta = length(delta);
                if (length_delta < 1e-4) {
                    continue;
                }
                let weight = saturate(length_delta * falloff_mul + falloff_add);
                if (side == 0) {
                    horizon_plus = max(horizon_plus, mix(low_plus, dot(delta / length_delta, to_eye), weight));
                } else {
                    horizon_minus = max(horizon_minus, mix(low_minus, dot(delta / length_delta, to_eye), weight));
                }
            }
        }
        var h_minus = -acos(clamp(horizon_minus, -1.0, 1.0));
        var h_plus = acos(clamp(horizon_plus, -1.0, 1.0));
        h_minus = n + clamp(h_minus - n, -0.5 * GTAO_PI, 0.5 * GTAO_PI);
        h_plus = n + clamp(h_plus - n, -0.5 * GTAO_PI, 0.5 * GTAO_PI);
        let arc_minus = (cos_n + 2.0 * h_minus * sin(n) - cos(2.0 * h_minus - n)) * 0.25;
        let arc_plus = (cos_n + 2.0 * h_plus * sin(n) - cos(2.0 * h_plus - n)) * 0.25;
        visibility += projected_length * (arc_minus + arc_plus);
    }
    visibility = saturate(visibility / f32(GTAO_SLICES));
    return vec4<f32>(pow(visibility, gtao.run.z), at, 0.0, 1.0);
}

// How much of `other` to take next to a texel at view distance `at`, by how near their distances are.
fn gtao_like(other: f32, at: f32) -> f32 {
    return exp(-abs(other - at) / (0.06 * at + 0.05));
}

@fragment
fn fs_gtao_resolve(v: SkyOut) -> @location(0) vec4<f32> {
    let h = vec2<i32>(v.clip.xy);
    let size = vec2<i32>(textureDimensions(gtao_raw));
    let centre = textureLoad(gtao_raw, h, 0).xy;
    if (centre.y == 0.0) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }
    // The 3x3, by distance: the slices' noise averaged, with the range the history is kept within.
    var sum = 0.0;
    var total = 0.0;
    var sum_squares = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let s = textureLoad(gtao_raw, clamp(h + vec2<i32>(x, y), vec2<i32>(0), size - 1), 0).xy;
            if (s.y == 0.0) {
                continue;
            }
            let weight = gtao_like(s.y, centre.y);
            sum += weight * s.x;
            sum_squares += weight * s.x * s.x;
            total += weight;
        }
    }
    let mean = sum / total;
    var visibility = mean;
    if (gtao.run.w == 0.0) {
        let spread = sqrt(max(sum_squares / total - mean * mean, 0.0));
        let p = h * 2;
        let world = mask_world(p, textureLoad(scene_depth, p, 0));
        let before = gtao.previous * vec4<f32>(world, 1.0);
        if (before.w > 1e-3) {
            let uv = vec2<f32>(before.x, -before.y) / before.w * 0.5 + 0.5;
            // The texel `at` stands at its 2x2's top left pixel, `2 at`, whose centre is `2 at + 0.5`.
            let at = (uv * vec2<f32>(textureDimensions(scene_depth)) - 0.5) / 2.0;
            let base = vec2<i32>(floor(at));
            let f = at - floor(at);
            var past = 0.0;
            var past_total = 0.0;
            for (var k = 0; k < 4; k++) {
                let o = vec2<i32>(k & 1, k >> 1);
                let q = base + o;
                if (any(q < vec2<i32>(0)) || any(q >= size)) {
                    continue;
                }
                let s = textureLoad(gtao_history, q, 0).xy;
                if (s.y == 0.0) {
                    continue;
                }
                let bilinear = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1);
                let weight = bilinear * gtao_like(s.y, before.w);
                past += weight * s.x;
                past_total += weight;
            }
            if (past_total > 0.25) {
                // Within the blur's range (twice its spread, and a little more): what moved is not smeared.
                let kept = clamp(past / past_total, mean - 2.0 * spread - 0.03, mean + 2.0 * spread + 0.03);
                visibility = mix(mean, kept, 0.9 * saturate(past_total));
            }
        }
    }
    return vec4<f32>(visibility, centre.y, 0.0, 1.0);
}

// The stage's contact shading at the pixel `p` at `world`: the half-size texel's, or with `Enhance::gtao`
// (`frame.sun_shadow.w`) the four nearest texels' by how near their view distance is to the pixel's (a silhouette's
// far side takes nothing of its near side's).
fn contact_at(p: vec2<i32>, world: vec3<f32>) -> f32 {
    if (frame.sun_shadow.w == 0.0) {
        return textureLoad(sun_contact, p / 2, 0).r;
    }
    let at = gtao_depth(world);
    let last = vec2<i32>(textureDimensions(sun_contact)) - 1;
    let u = vec2<f32>(p) / 2.0;
    let base = vec2<i32>(floor(u));
    let f = u - floor(u);
    var sum = 0.0;
    var total = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1);
        let s = textureLoad(sun_contact, clamp(base + o, vec2<i32>(0), last), 0).xy;
        let weight = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1) * gtao_like(s.y, at) + 1e-5;
        sum += weight * s.x;
        total += weight;
    }
    return sum / total;
}

// The baked occlusion with the screen's: they see the same stage, so they are not multiplied under `Enhance::gtao`
// (the darker of the two stands: the bake's metres, the screen's centimetres at the car, the crowd and the ground's
// detail); else as before.
fn combine_ao(baked: f32, screen: f32) -> f32 {
    return select(baked * screen, min(baked, screen), frame.sun_shadow.w != 0.0);
}
