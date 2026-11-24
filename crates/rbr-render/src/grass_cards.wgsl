// The grass made on the GPU drawn (`grass::made`): a view's cards (`grass_made.wgsl`), four corners each, expanded into
// the stage's vertices, so `vertex` grows, widens and sways them as the plants' tufts: `specular.x` is `GRASS_MARK`
// plus the card's brightness, `specular.z` its kind (1 ours; 2 a converted stage's cards, which `shader.wgsl` gives
// `FX_GRASS_ROOT` and `FX_GRASS_TIP`; 3 its group pieces, flowers and weeds, grown only past `FX_PIECE_NEAR` from the
// eye, as CSP spawns them only from its third generation pass on; 4 a cover-map stage's cards, drawn in the atlas's own colours), `wet.x` the share of height (`uv2.x` the top's
// height for the wind), `uv1` the way out across, `uv2.y` the card's rank (`GRASS_THIN`) and `wet.y` its half width
// (`grass_widen`).

@group(3) @binding(0) var<storage, read> grass_cards: array<GrassCard>;

// How deep a card's foot goes into the ground, metres (the ground curves away from the plane it stands on), and the
// share of height its foot is lit as (`own_light`'s occlusion and sun at the foot: dense grass shades its own foot).
const GRASS_SINK: f32 = 0.03;
const GRASS_FOOT: f32 = 0.0;

// Corner `index % 4` of card `index / 4`: left foot, right foot, right top, left top.
fn grass_corner(index: u32) -> VertexIn {
    let card = grass_cards[index / 4u];
    let corner = index % 4u;
    let side = select(-1.0, 1.0, corner == 1u || corner == 2u);
    let top = corner >= 2u;
    let x = card.across * side + select(vec2<f32>(0.0), unpack2x16float(card.lean), top);
    let n_xz = unpack2x16snorm(card.normal);
    let n = vec3<f32>(n_xz.x, sqrt(max(1.0 - dot(n_xz, n_xz), 0.0)), n_xz.y);
    // On the ground's plane through the foot.
    let ground = -(n.x * x.x + n.z * x.y) / max(n.y, 0.1);
    let kind = card.hue >> 24u;
    let bright_keep = unpack2x16float(card.bright_keep);
    let u = unpack2x16unorm(card.rect_u);
    let v = unpack2x16unorm(card.rect_v);
    var out: VertexIn;
    out.position = card.at + vec3<f32>(x.x, ground + select(-GRASS_SINK, card.height, top), x.y);
    out.color = vec4<f32>(unpack4x8unorm(card.hue).xyz, 1.0);
    out.uv0 = vec2<f32>(select(u.y, u.x, side < 0.0), select(v.y, v.x, top));
    out.uv1 = normalize(card.across) * side;
    out.uv2 = vec2<f32>(select(0.0, card.height, top), bright_keep.y);
    out.normal = n;
    out.specular = vec3<f32>(GRASS_MARK + bright_keep.x, 1.0, array<f32, 4>(1.0, 2.0, 3.0, 4.0)[min(kind, 3u)]);
    out.occlusion = card.occlusion;
    out.sky_dir = card.sky_dir;
    out.bounce = card.bounce;
    out.wet = vec2<f32>(select(GRASS_FOOT, 1.0, top), length(card.across));
    out.instance = 0u;
    return out;
}

@vertex
fn vs_grass(@builtin(vertex_index) index: u32) -> VertexOut {
    return vertex(grass_corner(index));
}

@vertex
fn vs_grass_clouded(@builtin(vertex_index) index: u32) -> CloudedOut {
    let o = vertex(grass_corner(index));
    return CloudedOut(
        o.clip, o.color, o.uv0, o.uv1, o.uv2, o.world, o.normal, o.specular, o.sky, o.bounce_sun, o.bounce_sky, o.wet,
        cloud_shadow(o.world),
    );
}
