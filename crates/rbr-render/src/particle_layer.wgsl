// The particles' half-size layer (`particle_target.rs`) at a full-size pixel, appended to `particle_target.wgsl` and
// `taa.wgsl`: the particles' light (rgb, premultiplied) and the frame's transmittance through them (a), so that the
// frame times a plus rgb is the frame with them drawn over it, and whether no particle reached it (`empty`: the frame
// stays as it is). The pixel takes the reduced pixels about it by how near their depth is to its own (`depth`), so a
// particle behind the car's edge does not bleed over it.

// A view distance's stand-in for a reversed depth (its inverse, up to the near plane's scale); the sky very far.
fn far_of(depth: f32) -> f32 {
    return 1.0 / max(depth, 1e-7);
}

// How sharply the reduced pixels' depths are told apart from the pixel's: their relative difference at which the
// weight halves.
const DEPTH_TOLERANCE: f32 = 0.05;

struct Layer {
    over: vec4<f32>,
    empty: bool,
}

fn layer_at(pixel: vec2<i32>, depth: f32, half_depth: texture_depth_2d, half_colour: texture_2d<f32>) -> Layer {
    let far = far_of(depth);
    let most = vec2<i32>(textureDimensions(half_depth)) - 1;
    let at = vec2<f32>(pixel) * 0.5 - 0.25;
    let base = vec2<i32>(floor(at));
    let f = at - floor(at);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    var empty = true;
    for (var k = 0; k < 4; k++) {
        let corner = vec2<i32>(k & 1, k >> 1);
        let texel = clamp(base + corner, vec2<i32>(0), most);
        let bilinear = select(1.0 - f.x, f.x, corner.x == 1) * select(1.0 - f.y, f.y, corner.y == 1);
        let apart = abs(far_of(textureLoad(half_depth, texel, 0)) - far) / far / DEPTH_TOLERANCE;
        let w = (bilinear + 1e-4) / (1.0 + apart * apart);
        let drawn = textureLoad(half_colour, texel, 0);
        empty = empty && all(drawn == vec4<f32>(0.0, 0.0, 0.0, 1.0));
        sum += drawn * w;
        weight += w;
    }
    return Layer(sum / weight, empty);
}
