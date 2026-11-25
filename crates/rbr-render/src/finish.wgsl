
// A converted car's material's finish (ours, `car::cabin::finish_uniform`, `mode.finish`), the same in every view: DiRT
// Rally's shaders as their disassembly reads (notes `renderer.md`). Inside, `interior.fx`, `interior_flock.fx`,
// `interior_carbon.fx`; outside, `carpaint_metallic.fx` (its Shiny Metal; the paint keeps the clear coat),
// `car_wheel.fx`, `car_light_pod.fx` (its reflection gated by the lit colour's brightness) and `car_matt.fx` (a
// highlight alone). The reflection's strength is the specular texel (`gloss`) times the specular's gain, at most 1
// (outside DiRT's `sat(2 * spec)`), times the Fresnel from `min` facing the eye to `max` at grazing along
// `(1 - n.v)^power` (the interior's; the exterior shaders' sigmoid takes constants packed in a way not read); a carbon's
// lacquer its `min` alone; a flock's times its fuzz (`finish_fuzz`). It mirrors the surroundings (`paint_surroundings`:
// the cube drawn at the car, else the probes), white or in the material's colour, sharp (`FINISH_BLUR.x` of the cube's
// levels) or soft (`.y`), over the lit texel in linear light; in the cabin only where it sees out of the windows
// (`cabin_open`). The sun's highlight is the Phong lobe `pow(R.L, power)` at the same strength in the Phong colour (in
// the cabin `CABIN_PHONG` of the sun, as DiRT halves its interior's diffuse sun), with a flock's fuzz lit white and a
// carbon's fibres' highlight.
const FINISH_BLUR: vec2<f32> = vec2<f32>(0.5, 3.5);
const CABIN_PHONG: f32 = 2.0;
const EXTERIOR_PHONG: f32 = 0.2;
// A flock's fibres (`interior_flock.fx`'s noise map, ours a value noise): cells per uv over its `NoiseSize`.
const FLOCK_CELLS: f32 = 256.0;
// `car_light_pod.fx`: the reflection times the lit colour (linear) this many times over, at most 1.
const POD_GATE: f32 = 1.8;

// The finish's kind (`car::cabin::finish_uniform`): 0 none, the interior's 1 plain, 2 flock, 3 carbon, the exterior's
// 4 gated, 5 exterior.
fn finish_kind() -> u32 {
    return u32(mode.finish[0].x + 0.5);
}

fn finished() -> bool {
    return finish_kind() != 0u;
}

// An exterior finish, which takes the outside view's light; an interior one only the cabin's (`cabin_face`): its
// outside copy (`e_internal`) has no cabin light, and the sky it mirrored there (a pillar's inside) was not seen.
fn exterior_finish() -> bool {
    return finish_kind() >= 4u;
}

fn finish_carbon() -> bool {
    return finish_kind() == 3u;
}

// A flock's fuzz (`interior_flock.fx`): its fibres seen edge on, `(1 - (n.v)^0.8)^2`, in its noise, less on the faces
// turned down.
fn finish_fuzz(v: VertexOut, n: vec3<f32>, view: vec3<f32>) -> f32 {
    if (finish_kind() != 2u) {
        return 0.0;
    }
    let edge = 1.0 - min(pow(clamp(dot(n, view), 0.0, 1.0), 0.8), 1.0);
    let noise = value_noise(vec3<f32>(v.uv0 * mode.finish[3].w * FLOCK_CELLS, 0.0));
    return edge * edge * noise * (1.0 - clamp(-n.y, 0.0, 1.0));
}

// The finish's reflection strength at `n`, `view`.
fn finish_strength(v: VertexOut, n: vec3<f32>, view: vec3<f32>, gloss: f32) -> f32 {
    let f = mode.finish[0];
    if (finish_carbon()) {
        return f.y;
    }
    let grazing = min(pow(max(1.0 - dot(n, view), 0.0), f.w), 1.0);
    let s = clamp(gloss * mode.finish[3].x, 0.0, 1.0) * mix(f.y, f.z, grazing);
    return select(s, s * finish_fuzz(v, n, view), finish_kind() == 2u);
}

// What the finish mirrors of the surroundings, `open` of it seen, over the lit colour `shaded` (a gated finish's
// reflection is as bright as that), and its share of the face (`a`).
fn finish_mirror(v: VertexOut, n: vec3<f32>, view: vec3<f32>, gloss: f32, open: f32, shaded: vec3<f32>) -> vec4<f32> {
    let environment = mode.finish[1];
    if (environment.w < 0.0) {
        return vec4<f32>(0.0);
    }
    let r = reflect(-view, n);
    let seen = paint_surroundings(r, mix(FINISH_BLUR.x, FINISH_BLUR.y, environment.w));
    let grey = vec3<f32>(dot(seen, vec3<f32>(0.2126, 0.7152, 0.0722)));
    var mirror = mix(seen, grey, CABIN_NEUTRAL * environment.w) * environment.rgb;
    if (finish_kind() == 4u) {
        // DiRT gates in linear light, as the lit colour is before its tone curve.
        let gate = clamp(POD_GATE * pow(shaded, vec3<f32>(2.2)), vec3<f32>(0.0), vec3<f32>(1.0));
        mirror = mirror * pow(gate, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(mirror, clamp(finish_strength(v, n, view, gloss) * open, 0.0, 1.0));
}

// `shaded` with the finish's reflection over it, in linear light.
fn finish_over(shaded: vec3<f32>, mirror: vec4<f32>) -> vec3<f32> {
    return pow(mix(pow(shaded, vec3<f32>(2.2)), pow(mirror.rgb, vec3<f32>(2.2)), mirror.a), vec3<f32>(1.0 / 2.2));
}

// The finish's light off the sun, per unit of the sun's light and of its share past the shadows: the Phong lobe at
// `weight` of the sun, a flock's fuzz lit white (half the sun, as DiRT's diffuse takes it), a carbon's fibres along the
// weave's direction `fibre` (Kajiya-Kay, `interior_carbon.fx`), on the side the sun lights.
fn finish_sun(v: VertexOut, n: vec3<f32>, view: vec3<f32>, gloss: f32, fibre: vec3<f32>, weight: f32) -> vec3<f32> {
    let to_sun = -frame.sun.xyz;
    let nl = dot(n, to_sun);
    if (nl <= 0.0) {
        return vec3<f32>(0.0);
    }
    let phong = mode.finish[2];
    let lobe = pow(clamp(dot(reflect(-view, n), to_sun), 0.0, 1.0), phong.x);
    // An interior finish's lobe takes its reflection's strength. Outside, one that mirrors nothing (`car_matt.fx`) takes
    // the specular texel alone; one that mirrors (`carpaint_metallic.fx`, `car_wheel.fx`, `car_light_pod.fx`) has a
    // metallic flake's in place of it, only where its specular is past a half (`sat((spec - 0.5) * 8)`).
    let raw = clamp(gloss * mode.finish[3].x, 0.0, 1.0);
    var strength = finish_strength(v, n, view, gloss);
    if (finish_carbon()) {
        strength = 1.0;
    } else if (exterior_finish() && mode.finish[1].w < 0.0) {
        strength = raw;
    } else if (exterior_finish()) {
        // The texel as DiRT's map holds it: the gain is twice its reference.
        let spec = clamp(0.5 * gloss * mode.finish[3].x, 0.0, 1.0);
        strength = spec * clamp((spec - 0.5) * 8.0, 0.0, 1.0);
    }
    var light = phong.yzw * lobe * strength * weight;
    light += vec3<f32>(0.5 * finish_fuzz(v, n, view) * nl);
    if (finish_carbon() && dot(fibre, fibre) > 0.0) {
        let carbon = mode.finish[3];
        let tl = dot(fibre, to_sun);
        let tv = dot(fibre, view);
        let along = sqrt(max(1.0 - tl * tl, 0.0)) * sqrt(max(1.0 - tv * tv, 0.0)) - tl * tv;
        let falloff = min(pow(clamp(dot(reflect(-view, n), to_sun), 0.0, 1.0), carbon.z), 1.0);
        light += vec3<f32>(carbon.x * 0.5 * pow(max(along, 0.0), carbon.y) * nl * mix(1.0, falloff, 0.5));
    }
    return light;
}

// A carbon weave's fibre direction (`interior_carbon.fx`'s `TCarbonVectorMap`, in the normal map's slot: red and green
// along u and v), in the world, across `n`; zero for none. Read as `bumped` reads a normal map, before any branch.
fn carbon_fibre(v: VertexOut, n: vec3<f32>) -> vec3<f32> {
    let uv = v.uv0 * mode.bump.zw;
    let dir = textureSample(normal_texture, texture_sampler, uv).rg * 2.0 - 1.0;
    let dp1 = dpdx(v.world);
    let dp2 = dpdy(v.world);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let side2 = cross(dp2, n);
    let side1 = cross(n, dp1);
    let t = side2 * duv1.x + side1 * duv2.x;
    let b = side2 * duv1.y + side1 * duv2.y;
    let along = t * dir.x + b * dir.y;
    let across = along - n * dot(along, n);
    return select(vec3<f32>(0.0), normalize(across), dot(across, across) > 1e-20);
}
