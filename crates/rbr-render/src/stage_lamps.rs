//! The stage's own lamps (renderer addition, ours; the game has no lights): street lamps, floodlights and flares that a
//! stage only paints. The night stages (126 Sardian Night, 401 Vinec-Skalsko NIGHT, 466 and 467 Odry snow Night) draw
//! a sprite of each lamp's light: a white texture whose alpha is the light's shape, a cone under a lamp head
//! (`bulbglo_long`, `spot_long`) or a star about a flare (`flare`). The game draws them alpha-tested, a hard white
//! shape. Here each sprite is a lamp ([`find`]): at the sprite's brightest point (its texture's peak alpha), pointing
//! from there along the sprite toward the rest of its light (down a cone; none for a star). At night the lamps light
//! the stage (`street_sum` in `night.wgsl`), the rain and the particles, and each shows as a bulb
//! (`stage_lamps_pass`); the sprites, the game's painted beams, are no longer drawn (`Renderer::set_stage_lamps`).

use crate::foliage::test_texels;
use crate::stage::{Anchor, Mesh, Stage, TextureRef};
use glam::{Vec2, Vec3};
use std::collections::HashMap;

/// A texel counts as part of a sprite's light from this alpha.
const SEEN: u8 = 16;
/// A light sprite's lit texels are this white at least: their mean luminance (0 to 1) ...
const WHITE: f32 = 0.8;
/// ... with their channels at most this far apart (mean of max - min, 0 to 1).
const GREY: f32 = 0.12;
/// A light sprite's alpha is soft: at least this share of its lit texels is neither clear nor opaque (below 240).
const SOFT: f32 = 0.6;
/// Most of a light sprite is clear: at most this share of its texels is lit.
const COVER: f32 = 0.6;
/// Sprite points closer than this, metres, are one lamp (the crossed quads of one cone).
const SAME_LAMP: f32 = 1.0;
/// A sprite whose light reaches less than this share of its size from its peak is a star, not a cone.
const CONE: f32 = 0.15;

/// The lamps' colour, linear (ours): a warm white, as the sprites are white and old street lamps warm.
pub const COLOUR: Vec3 = Vec3::new(1.0, 0.78, 0.55);
/// A street lamp's intensity down its axis, cd (ours: about 25 lux on the road under a lamp 9 m up, a few lux
/// midway to the next 25 m on).
const CANDELA: f32 = 2000.0;
/// The lamps' units per candela (`lights::UNITS_PER_CANDELA`: a headlight's 20000 cd are 3000).
const UNITS_PER_CANDELA: f32 = 3000.0 / 20000.0;
/// A sprite this tall, metres, or taller is a street lamp's; a smaller one's lamp is as much weaker as its sprite's
/// area is smaller (a lamp on a wall), down to [`DIMMEST`] of a street lamp.
const STREET_SPRITE: f32 = 8.0;
const DIMMEST: f32 = 0.02;

/// `lamp`'s size against a street lamp's, 0 to 1 (its sprite's against [`STREET_SPRITE`]).
pub fn scale(lamp: &StageLamp) -> f32 {
    (lamp.size / STREET_SPRITE).min(1.0)
}

/// `lamp`'s intensity down its axis in the lamps' units (`street_sum` in `night.wgsl`).
pub fn intensity(lamp: &StageLamp) -> f32 {
    let scale = scale(lamp);
    CANDELA * UNITS_PER_CANDELA * (scale * scale).max(DIMMEST)
}

/// One lamp of the stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageLamp {
    /// Where its light comes from, stage space.
    pub position: Vec3,
    /// The way its light falls (unit), or zero for a lamp lighting every way.
    pub axis: Vec3,
    /// Its sprite's size, metres (the tallest of its quads).
    pub size: f32,
    /// Its sprite's texture.
    pub texture: TextureRef,
}

/// A texture's light shape: the uv of its brightest point and the alpha-weighted centre of its light.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    peak: Vec2,
    centre: Vec2,
}

/// The light sprite textures' shapes; `None` for a texture that is not one.
fn shape(width: usize, rgba: &[u8]) -> Option<Shape> {
    let height = rgba.len() / 4 / width.max(1);
    if width == 0 || height == 0 {
        return None;
    }
    let (mut lit, mut soft, mut light, mut grey, mut weight) = (0usize, 0usize, 0.0, 0.0, 0.0);
    let mut centre = Vec2::ZERO;
    let mut best = 0u8;
    for (i, texel) in rgba.chunks_exact(4).enumerate() {
        let a = texel[3];
        if a < SEEN {
            continue;
        }
        lit += 1;
        soft += usize::from(a < 240);
        let [r, g, b] = [texel[0], texel[1], texel[2]].map(|c| f32::from(c) / 255.0);
        light += 0.2126 * r + 0.7152 * g + 0.0722 * b;
        grey += r.max(g).max(b) - r.min(g).min(b);
        let w = f32::from(a);
        centre += w * uv_of(i, width, height);
        weight += w;
        best = best.max(a);
    }
    let n = lit as f32;
    let white = lit > 0
        && light / n >= WHITE
        && grey / n <= GREY
        && soft as f32 >= SOFT * n
        && n <= COVER * (width * height) as f32;
    if !white {
        return None;
    }
    // The peak: the centre of the texels within 90% of the most opaque.
    let floor = (u16::from(best) * 9 / 10) as u8;
    let (mut peak, mut count) = (Vec2::ZERO, 0.0);
    for (i, texel) in rgba.chunks_exact(4).enumerate() {
        if texel[3] >= floor {
            peak += uv_of(i, width, height);
            count += 1.0;
        }
    }
    Some(Shape { peak: peak / count, centre: centre / weight })
}

fn uv_of(i: usize, width: usize, height: usize) -> Vec2 {
    Vec2::new(((i % width) as f32 + 0.5) / width as f32, ((i / width) as f32 + 0.5) / height as f32)
}

/// Words one of which a light sprite texture's file name holds (upper case): `bulbglo_long`, `spot_long`,
/// `bulbglowneo`, `flare`. White soft cut-outs are other things too: clouds (307's `Clloud`), wire fences
/// (`FenceW`), road signs, the spectators' camera flashes (`flash`), snowfall.
const NAMED: [&str; 6] = ["GLO", "SPOT", "BULB", "LIGHT", "LAMP", "FLARE"];

/// Finds the stage's lamps ([`StageLamp`]: one per light sprite, its crossed quads one lamp) and takes their sprites'
/// draws out of the stage's meshes. The textures must be loaded (`Stage::load_textures`).
pub fn take(stage: &mut Stage) -> Vec<StageLamp> {
    let mut shapes: HashMap<TextureRef, Option<Shape>> = HashMap::new();
    let mut found: Vec<StageLamp> = Vec::new();
    for m in 0..stage.meshes.len() {
        if stage.meshes[m].anchor != Anchor::World {
            continue;
        }
        let mut kept = Vec::with_capacity(stage.meshes[m].draws.len());
        for draw in std::mem::take(&mut stage.meshes[m].draws) {
            let texture = draw.state.textures[0].filter(|_| is_sprite_state(&draw.state));
            let shape = texture.and_then(|t| *shapes.entry(t).or_insert_with(|| sprite_shape(stage, t)));
            let (Some(texture), Some(shape)) = (texture, shape) else {
                kept.push(draw);
                continue;
            };
            let mesh = &stage.meshes[m];
            for t in mesh.indices[draw.indices.clone()].chunks_exact(3) {
                let Some(lamp) = triangle_lamp(mesh, [t[0], t[1], t[2]], shape, texture) else { continue };
                match found.iter_mut().find(|l| l.position.distance(lamp.position) < SAME_LAMP) {
                    Some(same) => {
                        same.size = same.size.max(lamp.size);
                        if same.axis == Vec3::ZERO {
                            same.axis = lamp.axis;
                        }
                    }
                    None => found.push(lamp),
                }
            }
        }
        stage.meshes[m].draws = kept;
    }
    found
}

/// A light sprite texture's shape: named as one ([`NAMED`]) and white with a soft alpha ([`shape`]).
fn sprite_shape(stage: &Stage, texture: TextureRef) -> Option<Shape> {
    let name = stage.texture_names.get(&texture)?.file_name.to_uppercase();
    if !NAMED.iter().any(|word| name.contains(word)) {
        return None;
    }
    test_texels(stage, texture).and_then(|(width, rgba)| shape(width, &rgba))
}

/// Whether a draw's state is a light sprite's: an object's alpha-tested draw with one texture (not the ground's).
fn is_sprite_state(state: &crate::stage::DrawState) -> bool {
    state.alpha_ref.is_some() && state.textures[1].is_none() && state.shade.is_none() && state.shadow.is_none()
}

/// The lamp of one sprite triangle: the world point at the texture's peak uv when the triangle holds it.
fn triangle_lamp(mesh: &Mesh, triangle: [u32; 3], shape: Shape, texture: TextureRef) -> Option<StageLamp> {
    let vertex = |i: u32| &mesh.vertices[i as usize];
    let [a, b, c] = triangle.map(|i| Vec3::from(vertex(i).position));
    let [ta, tb, tc] = triangle.map(|i| Vec2::from(vertex(i).uv0));
    let at = |uv: Vec2| -> Option<Vec3> {
        let (w, inside) = barycentric(uv, ta, tb, tc)?;
        inside.then_some(a * w.x + b * w.y + c * w.z)
    };
    let position = at(shape.peak)?;
    // The centre of the light on the sprite's plane (the barycentric weights extend past the triangle).
    let (w, _) = barycentric(shape.centre, ta, tb, tc)?;
    let centre = a * w.x + b * w.y + c * w.z;
    let size = a.distance(b).max(b.distance(c)).max(c.distance(a));
    let reach = centre - position;
    let axis = if reach.length() > CONE * size { reach.normalize() } else { Vec3::ZERO };
    Some(StageLamp { position, axis, size, texture })
}

/// The barycentric weights of `p` in the triangle `a b c` (texture space), and whether `p` lies inside it (with a
/// little slack for the shared edge of a quad); `None` for a degenerate triangle.
fn barycentric(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec3, bool)> {
    let (v0, v1, v2) = (b - a, c - a, p - a);
    let den = v0.perp_dot(v1);
    if den.abs() < 1e-9 {
        return None;
    }
    let v = v2.perp_dot(v1) / den;
    let w = v0.perp_dot(v2) / den;
    let u = 1.0 - v - w;
    let slack = -1e-3;
    Some((Vec3::new(u, v, w), u >= slack && v >= slack && w >= slack))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16 by 32 texture: white, its alpha a cone from the top centre down, clear elsewhere.
    fn cone() -> Vec<u8> {
        let (w, h) = (16, 32);
        let mut rgba = vec![255; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let across = (x as f32 + 0.5 - w as f32 / 2.0).abs() / (1.0 + y as f32 * 0.25);
                let a = if across < 1.0 { 255.0 * (1.0 - y as f32 / h as f32) } else { 0.0 };
                rgba[(y * w + x) * 4 + 3] = a as u8;
            }
        }
        rgba
    }

    #[test]
    fn a_cone_sprite_peaks_at_its_top_and_falls_down() {
        let shape = shape(16, &cone()).expect("a light sprite");
        assert!(shape.peak.y < 0.1 && (shape.peak.x - 0.5).abs() < 0.05, "{shape:?}");
        assert!(shape.centre.y > shape.peak.y + 0.2, "{shape:?}");
    }

    #[test]
    fn coloured_or_opaque_textures_are_not_sprites() {
        let mut red = cone();
        red.chunks_exact_mut(4).for_each(|t| t[1..3].fill(0));
        assert_eq!(shape(16, &red), None);
        let opaque = vec![255; 16 * 32 * 4];
        assert_eq!(shape(16, &opaque), None);
    }

    #[test]
    fn the_peak_maps_to_the_quads_top() {
        let shape = Shape { peak: Vec2::new(0.5, 0.0), centre: Vec2::new(0.5, 0.6) };
        // A 10 m upright quad, uv v down from its top at y 10.
        let corner = |x: f32, y: f32, u: f32, v: f32| crate::stage::Vertex {
            position: [x, y, 0.0],
            uv0: [u, v],
            ..bytemuck::Zeroable::zeroed()
        };
        let mesh = Mesh {
            layer: crate::stage::Layer::Solid,
            anchor: crate::stage::Anchor::World,
            bounds: None,
            cell: None,
            vertices: vec![corner(-5.0, 10.0, 0.0, 0.0), corner(5.0, 10.0, 1.0, 0.0), corner(5.0, 0.0, 1.0, 1.0)],
            indices: vec![0, 1, 2],
            draws: Vec::new(),
            sun_cascades: true,
        };
        let lamp =
            triangle_lamp(&mesh, [0, 1, 2], shape, TextureRef { set: crate::stage::TextureSet::Stage, index: 0 })
                .expect("holds the peak");
        assert!(lamp.position.distance(Vec3::new(0.0, 10.0, 0.0)) < 1e-4, "{lamp:?}");
        assert!(lamp.axis.distance(Vec3::NEG_Y) < 1e-4, "{lamp:?}");
    }
}
