// Screen-space reflections (`renderer::ScreenReflection`; after AMD FidelityFX SSSR's hierarchical march, MIT, and
// DiligentFX's write-up, Apache-2.0; ideas only, no code): every reflecting surface (water, flowing water, the wet
// roads' film and puddles; the additions' flags say which), its rays drawn from the GGX distribution of visible normals
// (Heitz 2018) and traversed through the frame's depth pyramid (`renderer::depth_pyramid`) coarse to fine. A hit is
// judged by a thickness growing with the distance and read from blurrier levels the further it lies on screen, with
// the edge and distance fades and the cube and the sky for the misses. In place of a linear march with a hand-spread
// cone (2026-11-05): wet 41M at 4K 7.7 to 4.0 ms, 1080p 1.6 to 1.05 ms; 45O chase within 0.3 ms.

// The last ray's hit (`march`): how far it went and how much it is trusted, 0 for a miss; and the rays' mean distance
// to their hits (`reflect_ray`), for the split trace's resolve.
var<private> hit: vec2<f32>;
var<private> hit_distance: f32;

// Nearest depth per cell, level 0 the frame's (reversed depth: the largest).
@group(2) @binding(2) var depth_pyramid: texture_2d<f32>;

// Cells visited a ray at most (`Reflections::rays`).
override SSR_ITERATIONS: i32 = 64;
// The pyramid's finest level the rays reach (0 the frame's pixels; 1 cost as much and missed thin things).
const SSR_FINEST: i32 = 0;
// The GGX lobe's alpha over the cone's length along the view (`roughness` times `stretch`): 0.5 matched its length
// and spread the lamps' streaks wider than the linear march's; 0.25 keeps them as crisp.
const SSR_ALPHA: f32 = 0.25;
// The cells a ray that passed behind something skips at first, doubling each time it is still behind.
const SSR_SKIP: f32 = 4.0;
// The pyramid's coarsest level (`depth_pyramid::LEVELS`).
const PYRAMID_TOP: i32 = 8;
// The visible normals' tail left out (the share of the lobe's samples furthest out, which only add noise).
const SSR_TAIL: f32 = 0.3;
// The reflected ray stops short of the eye's plane, metres in front of it.
const SSR_EYE_PLANE: f32 = 0.2;

@fragment
fn fs_reflect(v: VertexOut) -> Traced {
    if (hidden(v)) {
        discard;
    }
    return Traced(reflect_ray(reflector(v), v.clip.xy), v.clip.z);
}

// What reflects at a point, its cone by `kind`: the film on a loose road (0), on tarmac (1), a puddle (2) or water (3).
struct Reflector {
    world: vec3<f32>,
    normal: vec3<f32>,
    weight: f32,
    kind: f32,
}

const KIND_TARMAC: f32 = 1.0;
const KIND_PUDDLE: f32 = 2.0;
const KIND_WATER: f32 = 3.0;

// The reflecting surface at `v`: the water's or the wet road's normal and reflectance (`puddled`).
fn reflector(v: VertexOut) -> Reflector {
    let view = normalize(frame.eye.xyz - v.world);
    var r = Reflector(v.world, vec3<f32>(0.0, 1.0, 0.0), 0.0, select(0.0, KIND_WATER, mode.water != 0u));
    if (mode.water != 0u && frame.enhance.y != 0.0) {
        let flowing = flowing_normal(v);
        r.normal = flowing.xyz;
        r.weight = water_fresnel(r.normal, view) * flowing.w;
    } else if (mode.water != 0u) {
        r.normal = water_normal(v.world);
        r.weight = water_fresnel(r.normal, view);
    } else if (frame.enhance.z != 0.0 && dot(v.normal, v.normal) > 1e-8) {
        let film = puddled(v);
        r.normal = film.normal;
        r.weight = film.reflectance;
        r.kind = select(select(0.0, KIND_TARMAC, v.wet.x > TARMAC_SHARE.x), KIND_PUDDLE, film.puddle >= 0.5);
    }
    return r;
}

// The cone (gloss, roughness, stretch) of a surface of `kind`.
fn cone_of(kind: f32) -> vec3<f32> {
    let film = select(vec3<f32>(GLOSS.x, ROUGHNESS.x, STRETCH.x), TARMAC_CONE, kind == KIND_TARMAC);
    return select(select(vec3<f32>(GLOSS.y, ROUGHNESS.y, STRETCH.y), film, kind < KIND_PUDDLE), PUDDLE_CONE, kind == KIND_PUDDLE);
}

// The reflection at `r` and its weight, the mean of `SSR_RAYS` rays, `pixel` the traced target's pixel (its noise).
// Each is a sample of the GGX lobe of `alpha` (`SSR_ALPHA` of the surface's cone, `GLOSS`, `ROUGHNESS` and `STRETCH`,
// or a puddle's or tarmac film's): at a grazing view the lobe stretches toward the eye by itself.
fn reflect_ray(r: Reflector, pixel: vec2<f32>) -> vec4<f32> {
    let view = normalize(frame.eye.xyz - r.world);
    let n = r.normal;
    let weight = r.weight;
    let water = r.kind == KIND_WATER;
    let still = f32(r.kind >= KIND_PUDDLE);
    let mirrored = reflect(-view, n);
    if (weight <= 0.0 || mirrored.y <= 0.0) {
        return vec4<f32>(0.0);
    }
    var miss = vec4<f32>(0.0);
    if (frame.reflection.x != 0.0 && water) {
        let near = 1.0 - smoothstep(SSR_CUBE_REACH.x, SSR_CUBE_REACH.y, distance(r.world, frame.eye.xyz));
        miss = vec4<f32>(surrounding(mirrored, 1.0, r.world), weight * near);
    }
    let cone = cone_of(r.kind);
    let alpha = SSR_ALPHA * cone.y * cone.z;
    // Two noises a pixel (interleaved gradient noise, Jimenez 2014), each ray a step of the R2 sequence on.
    let noise = vec2<f32>(
        fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715)))),
        fract(52.9829189 * fract(dot(pixel + vec2<f32>(47.0, 17.0), vec2<f32>(0.06711056, 0.00583715)))),
    );
    // The surface's frame: z the normal.
    let tx = normalize(cross(vec3<f32>(0.0, 0.0, 1.0), n) + vec3<f32>(1e-4, 0.0, 0.0));
    let ty = cross(n, tx);
    let local = vec3<f32>(dot(view, tx), dot(view, ty), dot(view, n));
    var sum = vec4<f32>(0.0);
    var reached = vec2<f32>(0.0);
    for (var k = 0; k < SSR_RAYS; k++) {
        let u = fract(noise + f32(k) * vec2<f32>(0.7548777, 0.5698403));
        let h = visible_normal(local, alpha, vec2<f32>(u.x * (1.0 - SSR_TAIL), u.y));
        let micro = h.x * tx + h.y * ty + h.z * n;
        let ray = reflect(-view, micro);
        let found = march(r.world, ray, weight, miss, still, cone.x);
        sum += vec4<f32>(found.rgb * found.a, found.a);
        reached += vec2<f32>(hit.x, 1.0) * hit.y;
    }
    hit_distance = reached.x / max(reached.y, 1e-6);
    sum /= f32(SSR_RAYS);
    return vec4<f32>(sum.rgb / max(sum.a, 1e-4), sum.a);
}

// Split trace (`Detail::Ultra` above `ULTRA_FULL_HEIGHT` rows, `renderer::ScreenReflection`): the reflecting draws at
// the frame's size write their surface (`fs_surface`: normal x and z, kind, weight; the weight blended over the layers
// below, the rest the top layer's), the rays are traced from one pixel of each 2x2 (`fs_trace`, the one that reflects
// most) and `fs_resolve` lays them over the frame by each pixel's own weight.
@group(2) @binding(3) var surface: texture_2d<f32>;

@fragment
fn fs_surface(v: VertexOut) -> @location(0) vec4<f32> {
    if (hidden(v)) {
        discard;
    }
    let r = reflector(v);
    if (r.weight <= 0.0 || reflect(normalize(v.world - frame.eye.xyz), r.normal).y <= 0.0) {
        discard;
    }
    return vec4<f32>(r.normal.x, r.normal.z, r.kind, r.weight);
}

// One triangle over the target.
@vertex
fn vs_screen(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

struct Split {
    @location(0) colour: vec4<f32>,
    @location(1) depth: f32,
    @location(2) normal: vec4<f32>,
}

// The reflection and its share of the surface's weight (the rays' hits), the frame's depth and the surface's normal (x
// and z) at the pixel of the 2x2 under `position` whose surface reflects most.
@fragment
fn fs_trace(@builtin(position) position: vec4<f32>) -> Split {
    let size = vec2<i32>(textureDimensions(surface));
    let quad = vec2<i32>(position.xy) * 2;
    var at = quad;
    var s = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let p = min(quad + vec2<i32>(k & 1, k >> 1u), size - 1);
        let here = textureLoad(surface, p, 0);
        if (here.w > s.w) {
            at = p;
            s = here;
        }
    }
    if (s.w <= 0.0) {
        return Split(vec4<f32>(0.0), 0.0, vec4<f32>(0.0));
    }
    let depth = textureLoad(scene_depth, at, 0);
    let uv = (vec2<f32>(at) + 0.5) / vec2<f32>(size);
    let q = frame.inverse_view_projection * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let normal = vec3<f32>(s.x, sqrt(max(1.0 - s.x * s.x - s.y * s.y, 0.0)), s.y);
    let found = reflect_ray(Reflector(q.xyz / q.w, normal, s.w, s.z), position.xy);
    return Split(vec4<f32>(found.rgb, found.a / s.w), depth, vec4<f32>(s.xy, hit_distance, 0.0));
}

// The split trace's resolve over the frame (`Trace::Split`): each pixel takes the traced texels about it by a tent and
// by how near their depth is to its own (coverage), their reflections mostly from those whose normal is like its own;
// then the hit is rebuilt at the texels' mean distance to theirs along the mirrored direction of the pixel's own normal
// and of the texels' mean normal, and the difference of the frame read at the two is added, so that the ripples'
// distortion follows each pixel's normal (where the frame's depth agrees at both). Reading at the pixel's own direction
// alone came out darker and patchier than the full-size trace: its rays' mean direction is not the mirrored one.
@group(2) @binding(4) var traced_colour: texture_2d<f32>;
@group(2) @binding(5) var traced_depth: texture_2d<f32>;
@group(2) @binding(6) var traced_hit: texture_2d<f32>;

// How sharply a pixel prefers the texels traced from normals like its own (one over the square of the normals' spread
// in radians): 1000 to 10000 brought out the rain's ripples in the car's mirrored image (Luceram 417 fly-by at 4K).
const NORMAL_MATCH: f32 = 3000.0;
const DEPTH_MATCH: f32 = 100.0;
// The rays' lobe's width at a rebuilt hit over its alpha, read as a blur there (1 and 2 alike on Luceram's fly-by).
const LOBE: f32 = 1.0;
// The darkest a rebuilt hit takes a pixel, over the texels' reflection (lone dark pixels below it).
const DARKEST: f32 = 0.5;

fn unpacked(xz: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(xz.x, sqrt(max(1.0 - dot(xz, xz), 0.0)), xz.y);
}

// The frame read where a ray from `world` along `ray` lies after `travelled` metres, blurred as the trace's rays' lobe
// spreads (`LOBE`) and as far on screen as it lies from `pixel`; alpha 0 where it is off screen or the frame's depth
// is not there.
fn rebuilt(world: vec3<f32>, ray: vec3<f32>, travelled: f32, pixel: vec2<f32>, cone: vec3<f32>) -> vec4<f32> {
    let full = vec2<f32>(textureDimensions(scene_depth));
    let point = world + ray * travelled;
    let c = frame.view_projection * vec4<f32>(point, 1.0);
    let at = vec2<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
    if (c.w <= 0.0 || any(at < vec2<f32>(0.0)) || any(at > vec2<f32>(1.0))) {
        return vec4<f32>(0.0);
    }
    let behind = distance(point, frame.eye.xyz) - scene_distance(at, scene_depth_at(at));
    if (abs(behind) > 0.15 + 0.01 * distance(world, frame.eye.xyz) + 0.02 * travelled) {
        return vec4<f32>(0.0);
    }
    let across = travelled * SSR_ALPHA * cone.y * cone.z * LOBE;
    let e = frame.view_projection * vec4<f32>(point + normalize(cross(ray, vec3<f32>(0.0, 1.0, 0.0))) * across, 1.0);
    let spread = length((e.xy / e.w - c.xy / c.w) * vec2<f32>(0.5, -0.5) * full);
    let pixels = length(at * full - pixel);
    let lod = clamp(log2(max(max(pixels * cone.x, spread), 1.0)), 0.0, f32(textureNumLevels(scene_colour) - 1u));
    return vec4<f32>(textureSampleLevel(scene_colour, reflection_sampler, at, lod).rgb, 1.0);
}

@fragment
fn fs_resolve(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let under = textureLoad(scene_colour, pixel, 0);
    let here = textureLoad(surface, pixel, 0);
    if (here.w <= 0.0) {
        return under;
    }
    let normal = unpacked(here.xy);
    let depth = textureLoad(scene_depth, pixel, 0);
    let size = vec2<i32>(textureDimensions(traced_colour));
    let p = position.xy * 0.5 - 0.5;
    let centre = vec2<i32>(round(p));
    var colour = vec4<f32>(0.0);
    var reached = vec2<f32>(0.0);
    var normals = vec3<f32>(0.0);
    var cover = 0.0;
    var total = 0.0;
    var low = vec3<f32>(1e30);
    var high = vec3<f32>(0.0);
    for (var k = 0; k < 9; k++) {
        let at = centre + vec2<i32>(k % 3 - 1, k / 3 - 1);
        let far = abs(vec2<f32>(at) - p);
        let weight = max(1.5 - far.x, 0.0) * max(1.5 - far.y, 0.0);
        let texel = clamp(at, vec2<i32>(0), size - 1);
        let d = textureLoad(traced_depth, texel, 0).r;
        let off = abs(d - depth) / max(max(d, depth), 1e-6);
        let found = textureLoad(traced_colour, texel, 0);
        let traced = textureLoad(traced_hit, texel, 0);
        let near = weight * exp(-off * off * DEPTH_MATCH) * found.a;
        let like = (exp(-(1.0 - dot(normal, unpacked(traced.xy))) * NORMAL_MATCH) + 0.01) * near;
        colour += vec4<f32>(found.rgb, 1.0) * like;
        reached += vec2<f32>(traced.z, 1.0) * like * f32(traced.z > 0.0);
        normals += unpacked(traced.xy) * like;
        if (near > 1e-4) {
            low = min(low, found.rgb);
            high = max(high, found.rgb);
        }
        cover += near;
        total += weight;
    }
    var reflection = colour.rgb / max(colour.a, 1e-6);
    let travelled = reached.x / max(reached.y, 1e-6);
    if (reached.y > 0.0 && travelled > 0.0 && travelled <= SSR_REACH) {
        let uv = position.xy / vec2<f32>(textureDimensions(scene_depth));
        let q = frame.inverse_view_projection * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
        let world = q.xyz / q.w;
        let view = normalize(world - frame.eye.xyz);
        let cone = cone_of(here.z);
        let own = rebuilt(world, reflect(view, normal), travelled, position.xy, cone);
        let theirs = rebuilt(world, reflect(view, normalize(normals)), travelled, position.xy, cone);
        if (own.a > 0.0 && theirs.a > 0.0) {
            // The brightness alone, within the texels' own range: a rebuilt hit on something they did not see (a dark
            // gap, a lamp) otherwise showed as a lone dark or coloured pixel.
            let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
            let was = dot(reflection, luma);
            let now = clamp(was + dot(own.rgb - theirs.rgb, luma), max(dot(low, luma), was * DARKEST), dot(high, luma));
            reflection *= now / max(was, 1e-4);
        }
    }
    return vec4<f32>(mix(under.rgb, reflection, here.w * cover / total), under.a);
}

// A normal of the GGX lobe of `alpha` seen from `view` (the surface's frame, z the normal), from `u` (Heitz 2018,
// "Sampling the GGX Distribution of Visible Normals").
fn visible_normal(view: vec3<f32>, alpha: f32, u: vec2<f32>) -> vec3<f32> {
    let vh = normalize(vec3<f32>(alpha * view.xy, view.z));
    let length_squared = dot(vh.xy, vh.xy);
    let t1 = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-vh.y, vh.x, 0.0) * inverseSqrt(max(length_squared, 1e-12)), length_squared > 0.0);
    let t2 = cross(vh, t1);
    let r = sqrt(u.x);
    let phi = 6.2831853 * u.y;
    let p1 = r * cos(phi);
    let s = 0.5 * (1.0 + vh.z);
    let p2 = (1.0 - s) * sqrt(1.0 - p1 * p1) + s * r * sin(phi);
    let nh = p1 * t1 + p2 * t2 + sqrt(max(0.0, 1.0 - p1 * p1 - p2 * p2)) * vh;
    return normalize(vec3<f32>(alpha * nh.xy, max(nh.z, 1e-6)));
}

// The ray from `v` along `ray` traversed through the depth pyramid (FidelityFX SSSR's hierarchical march, in the
// frame's pixels): a cell nearer than the ray is crossed and the next is taken a level coarser; one the ray is
// behind is taken a level finer. At the finest level the ray has reached the frame's surface: a hit if it lies
// within the thickness behind it, else the ray passed behind something in front and goes on.
fn march(world: vec3<f32>, ray: vec3<f32>, weight: f32, miss: vec4<f32>, still: f32, gloss: f32) -> vec4<f32> {
    hit = vec2<f32>(0.0);
    if (ray.y <= 0.0) {
        return miss;
    }
    let size = vec2<f32>(textureDimensions(scene_depth));
    let start = frame.view_projection * vec4<f32>(world, 1.0);
    let along = frame.view_projection * vec4<f32>(ray, 0.0);
    var reach = SSR_REACH;
    if (along.w < 0.0) {
        reach = min(reach, (SSR_EYE_PLANE - start.w) / along.w);
    }
    if (reach <= 0.0) {
        return miss;
    }
    let end = start + along * reach;
    let o = vec3<f32>((start.x / start.w * 0.5 + 0.5) * size.x, (0.5 - start.y / start.w * 0.5) * size.y, start.z / start.w);
    let e = vec3<f32>((end.x / end.w * 0.5 + 0.5) * size.x, (0.5 - end.y / end.w * 0.5) * size.y, end.z / end.w);
    let d = e - o;
    let inverse = vec3<f32>(
        select(1e30, 1.0 / d.x, d.x != 0.0),
        select(1e30, 1.0 / d.y, d.y != 0.0),
        select(1e30, 1.0 / d.z, d.z != 0.0),
    );
    // Where the ray leaves the screen.
    let wall = select(vec2<f32>(0.0), size, d.xy > vec2<f32>(0.0));
    let exits = select(vec2<f32>(1e30), (wall - o.xy) * inverse.xy, d.xy != vec2<f32>(0.0));
    let last = min(1.0, min(exits.x, exits.y));
    // The far side of a cell the ray crosses, and a nudge into the next.
    let ahead = select(vec2<f32>(0.0), vec2<f32>(1.0), d.xy >= vec2<f32>(0.0));
    let nudge = select(vec2<f32>(-0.005), vec2<f32>(0.005), d.xy >= vec2<f32>(0.0)) * exp2(f32(SSR_FINEST));
    let thickness = 0.15 + 0.01 * distance(world, frame.eye.xyz);
    let film = still < 0.5;
    var level = SSR_FINEST;
    // The cells a ray behind something skips at once, doubling each time.
    var skipping = SSR_SKIP;
    let first = exp2(f32(level));
    let planes = ((floor(o.xy / first) + ahead) * first + nudge - o.xy) * inverse.xy;
    var t = min(planes.x, planes.y);
    var p = o + d * t;
    for (var i = 0; i < SSR_ITERATIONS; i++) {
        if (t > last) {
            return miss;
        }
        let cell_size = exp2(f32(level));
        let cell = floor(p.xy / cell_size);
        let surface = textureLoad(depth_pyramid, vec2<i32>(cell), level).r;
        let crossing = ((cell + ahead) * cell_size + nudge - o.xy) * inverse.xy;
        let across = min(crossing.x, crossing.y);
        // The cell's surface plane, only while the ray goes away from the eye.
        let down = select(1e30, (surface - o.z) * inverse.z, d.z < 0.0);
        let above = surface < p.z;
        if (above) {
            t = min(across, down);
            p = o + d * t;
        }
        if (above && across <= down) {
            level = min(level + 1, PYRAMID_TOP);
            skipping = SSR_SKIP;
        } else if (level > SSR_FINEST) {
            level -= 1;
        } else {
            let uv = p.xy / size;
            let depth = scene_depth_at(uv);
            let q = frame.inverse_view_projection * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, p.z, 1.0);
            let at = q.xyz / q.w;
            let travelled = distance(at, world);
            let behind = distance(at, frame.eye.xyz) - scene_distance(uv, depth);
            let room = thickness + 0.02 * travelled;
            if (depth > 0.0 && travelled > 0.05 && behind <= room) {
                let pixels = length(p.xy - o.xy);
                let lod = clamp(log2(max(pixels * gloss, 1.0)), 0.0, f32(textureNumLevels(scene_colour) - 1u));
                let colour = textureSampleLevel(scene_colour, reflection_sampler, uv, lod).rgb;
                let edge = smoothstep(0.0, select(0.15, FILM_EDGE, film), min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y)));
                let fade = 1.0 - smoothstep(0.6, 1.0, travelled / SSR_REACH);
                let trust = edge * fade * (1.0 - smoothstep(0.3 * room, room, max(behind, 0.0)));
                hit = vec2<f32>(travelled, trust);
                return vec4<f32>(mix(miss.rgb, colour, trust), mix(miss.a, weight, trust));
            }
            // Behind something in front (or the surface it left): on past it, `skipping` cells at once.
            let skip = cell_size * skipping;
            skipping *= 2.0;
            let beyond = ((floor(p.xy / skip) + ahead) * skip + nudge - o.xy) * inverse.xy;
            t = max(t, min(beyond.x, beyond.y));
            p = o + d * t;
        }
    }
    return miss;
}
