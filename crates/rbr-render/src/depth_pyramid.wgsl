// The frame's depth pyramid (`renderer::depth_pyramid`) for the screen-space reflections (`ssr.wgsl`; after AMD
// FidelityFX SSSR's hierarchical depth, MIT): level 0 the frame's depth, each further level the nearest of the 2x2
// texels below it (reversed depth: the largest). Level 0 is padded to a multiple of the coarsest cell; the padding is
// the sky (0).

@group(0) @binding(0) var below: texture_2d<f32>;
@group(0) @binding(1) var level: texture_storage_2d<r32float, write>;
@group(0) @binding(2) var frame_depth: texture_depth_2d;

@group(0) @binding(3) var level_1: texture_storage_2d<r32float, write>;

// Levels 0 and 1: `id` a texel of level 1, over 2x2 of the frame's depth.
@compute @workgroup_size(8, 8)
fn cs_copy(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= textureDimensions(level_1))) {
        return;
    }
    var nearest = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let at = id.xy * 2u + vec2<u32>(k & 1u, k >> 1u);
        var depth = 0.0;
        if (all(at < textureDimensions(frame_depth))) {
            depth = textureLoad(frame_depth, at, 0);
        }
        textureStore(level, at, vec4<f32>(depth, 0.0, 0.0, 0.0));
        nearest = max(nearest, depth);
    }
    textureStore(level_1, id.xy, vec4<f32>(nearest, 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn cs_halve(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= textureDimensions(level))) {
        return;
    }
    let at = id.xy * 2u;
    let nearest = max(
        max(textureLoad(below, at, 0).r, textureLoad(below, at + vec2<u32>(1u, 0u), 0).r),
        max(textureLoad(below, at + vec2<u32>(0u, 1u), 0).r, textureLoad(below, at + vec2<u32>(1u, 1u), 0).r),
    );
    textureStore(level, id.xy, vec4<f32>(nearest, 0.0, 0.0, 0.0));
}
