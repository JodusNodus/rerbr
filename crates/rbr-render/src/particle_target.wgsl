// The soft particles drawn at half the frame's size (renderer addition, `particle_target.rs`): the frame's depth
// reduced to that size (`fs_depth`), then the particles' light and the frame's transmittance through them laid over
// the frame (`fs_composite`), each full-size pixel taking the reduced pixels about it by how near their depth is to its
// own, so a particle behind the car's edge does not bleed over it.

@group(0) @binding(0) var full_depth: texture_depth_2d;
@group(0) @binding(1) var half_depth: texture_depth_2d;
@group(0) @binding(2) var half_colour: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// The nearest of the 2x2 full-size depths (the depth is reversed: nearer is larger), so a particle behind a thin
// edge in the block is hidden there and the composite takes it from a neighbour of the edge's own depth.
@fragment
fn fs_depth(@builtin(position) clip: vec4<f32>) -> @builtin(frag_depth) f32 {
    let most = vec2<i32>(textureDimensions(full_depth)) - 1;
    let base = vec2<i32>(clip.xy) * 2;
    var nearest = 0.0;
    for (var k = 0; k < 4; k++) {
        let at = min(base + vec2<i32>(k & 1, k >> 1), most);
        nearest = max(nearest, textureLoad(full_depth, at, 0));
    }
    return nearest;
}

// The layer laid over the frame (`layer_at`). Where no particle reached, the frame stays as it is: no blend to pay
// for.
@fragment
fn fs_composite(@builtin(position) clip: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(clip.xy);
    let layer = layer_at(pixel, textureLoad(full_depth, pixel, 0), half_depth, half_colour);
    if (layer.empty) {
        discard;
    }
    return layer.over;
}
