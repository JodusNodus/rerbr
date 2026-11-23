// The sun's shadow cascades (renderer addition, `sun.rs`): stage draws drawn from the sun, depth only. The alpha-tested draws
// drop their cut-out texels at the alpha reference, so foliage casts its leaves, not its cards.

struct Cascade {
    matrix: mat4x4<f32>,
    // How far the backdrop's draws (instance 1) are moved, world metres (`sun::backdrop_drop`).
    drop: vec4<f32>,
}

// The head of `Mode` in `shader.wgsl`.
struct Mode {
    second_texture: u32,
    alpha_test: u32,
    alpha_ref: f32,
    mip_bias: f32,
    cutout: u32,
}

@group(0) @binding(0) var<uniform> cascade: Cascade;
@group(1) @binding(0) var texture0: texture_2d<f32>;
@group(1) @binding(2) var texture_sampler: sampler;
@group(1) @binding(3) var<uniform> mode: Mode;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    // y 1 on grass (`foliage.rs`).
    @location(6) specular: vec3<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv0: vec2<f32>,
}

@vertex
fn vs_sun(v: VertexIn, @builtin(instance_index) instance: u32) -> Out {
    var out: Out;
    // Grass casts no shadow: outside the clip volume.
    if (mode.cutout != 0u && v.specular.y != 0.0) {
        out.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    let backdrop = instance == 1u;
    out.clip = cascade.matrix * vec4<f32>(v.position + select(vec3<f32>(0.0), cascade.drop.xyz, backdrop), 1.0);
    // Casters further toward the sun than the near plane stay on it rather than being clipped (pancaking). Past the far
    // plane they are clipped, the backdrop's too: what lies there shades nothing in the map, and a vertex held on that
    // plane bent its triangle toward the sun, so the backdrop's large triangles shaded the road a low sun lit.
    out.clip.z = max(out.clip.z, 0.0);
    out.uv0 = v.uv0;
    return out;
}

// A placement of an instanced BTB mesh (`stage::placed`, `InstanceIn` in `shader.wgsl`): the matrix's rows.
struct InstanceIn {
    @location(11) row0: vec4<f32>,
    @location(12) row1: vec4<f32>,
    @location(13) row2: vec4<f32>,
}

// `vs_sun` for an instanced mesh: no backdrop among them.
@vertex
fn vs_sun_placed(v: VertexIn, i: InstanceIn) -> Out {
    var out: Out;
    if (mode.cutout != 0u && v.specular.y != 0.0) {
        out.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    let p = vec4<f32>(v.position, 1.0);
    out.clip = cascade.matrix * vec4<f32>(dot(i.row0, p), dot(i.row1, p), dot(i.row2, p), 1.0);
    out.clip.z = max(out.clip.z, 0.0);
    out.uv0 = v.uv0;
    return out;
}

@fragment
fn fs_sun(v: Out) {
    let cut = mode.alpha_test != 0u || mode.cutout != 0u;
    if (cut && textureSampleLevel(texture0, texture_sampler, v.uv0, 0.0).a <= mode.alpha_ref) {
        discard;
    }
}

// On `Detail::Ultra` the third cascade is drawn at half the layer's texels (`sun::COARSE`) and copied up into its layer,
// each drawn texel to 2x2, so its readers see the layer they always did.
@group(0) @binding(1) var coarse: texture_depth_2d;

// One triangle over the layer.
@vertex
fn vs_coarse(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_coarse(@builtin(position) at: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(coarse, vec2<i32>(at.xy) / 2, 0);
}
