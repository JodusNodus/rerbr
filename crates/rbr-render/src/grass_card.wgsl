// A grass card made on the GPU (`grass_made.wgsl`, drawn by `vs_grass` in `grass_cards.wgsl`): what `grass::Card` holds,
// packed. Its foot's middle and its height; half its width across (x and z) and its top's lean (two halves); the
// ground's normal's x and z (snorm16); its hue (rgb8) and kind (0 ours, 1 a converted stage's, 2 one of its pieces);
// its brightness and rank (two halves); its picture's rectangle (`u0, u1` and `v0, v1`, unorm16); the ground's light
// as a vertex holds it (`occlusion`, `sky_dir`, `bounce`).
struct GrassCard {
    at: vec3<f32>,
    height: f32,
    across: vec2<f32>,
    lean: u32,
    normal: u32,
    hue: u32,
    bright_keep: u32,
    rect_u: u32,
    rect_v: u32,
    occlusion: f32,
    sky_dir: u32,
    bounce: vec2<u32>,
}
