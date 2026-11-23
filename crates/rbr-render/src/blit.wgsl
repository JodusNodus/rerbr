// Halves a texture: each target texel is the bilinear sample at the shared corner of four source texels, their mean.
// Builds the car reflection cube's mip levels (`renderer::Reflection`) and the frame's for the screen-space reflections
// (`fs_blit_capped`).

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

struct BlitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle over the target.
@vertex
fn vs_blit(@builtin(vertex_index) index: u32) -> BlitOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return BlitOut(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

@fragment
fn fs_blit(v: BlitOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, source_sampler, v.uv, 0.0);
}

// The frame's levels for the screen-space reflections (`renderer::ScreenReflection`): each halving at most
// `REFLECTED_CAP` in its strongest channel, keeping its hue (`REFLECTED_CAP` in `ssr.wgsl`).
override REFLECTED_CAP: f32 = 256.0;

@fragment
fn fs_blit_capped(v: BlitOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(source, source_sampler, v.uv, 0.0);
    return vec4<f32>(c.rgb * min(1.0, REFLECTED_CAP / max(max(c.r, max(c.g, c.b)), 1e-6)), c.a);
}
