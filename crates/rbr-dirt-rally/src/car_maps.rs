//! A DiRT car's other maps in the renderer's form.
//!
//! The renderer's cars read one diffuse texture whose alpha is the reflection and specular strength, and (ours, a
//! converted car's `[<material>_bump_bitmap]`) a normal map. DiRT keeps a body's specular in the RGB of
//! `<car>_specocc` and its baked ambient occlusion in the alpha, and the interior's in `int_<car>_sp` and
//! `int_<car>_occ`, all on the diffuse's own UVs. [`diffuse`] folds the occlusion into the colour and makes the
//! specular its alpha; [`normal`] repacks a normal map as the RGB the renderer's bump reads. Both write BC7: a re-encoded
//! 4096 livery keeps its colours and the normals their slopes (DXT1's 5:6:5 endpoints band both).
use crate::texture::{Format, Texture};
use rbr_ac_tracks::btb::textures::encode_bc7;

/// One channel of a map, read at another texture's texels.
pub struct Plane {
    values: Vec<u8>,
    width: usize,
    height: usize,
}

impl Plane {
    /// Channel `channel` (0 red to 3 alpha) of `texture`'s base level.
    pub fn of(texture: &Texture, channel: usize) -> Plane {
        let values = texture.rgba().chunks_exact(4).map(|p| p[channel]).collect();
        Plane { values, width: texture.width as usize, height: texture.height as usize }
    }

    /// A plane of `width` by `height` values.
    #[cfg(test)]
    pub fn from_values(values: Vec<u8>, width: usize, height: usize) -> Plane {
        Plane { values, width, height }
    }

    /// The value at UV (`u`, `v`), wrapping, nearest texel (rows run down with v, as the car's UVs need no flip).
    pub fn sample(&self, u: f32, v: f32) -> u8 {
        let texel = |t: f32, n: usize| ((t.rem_euclid(1.0) * n as f32) as usize).min(n - 1);
        self.values[texel(v, self.height) * self.width + texel(u, self.width)]
    }

    /// The value under texel (`x`, `y`) of a `width` by `height` texture (the nearest texel, for a map of another size).
    fn at(&self, x: usize, y: usize, width: usize, height: usize) -> u8 {
        self.values[y * self.height / height * self.width + x * self.width / width]
    }
}

/// How the renderer's light takes a baked occlusion `ao` (0 to 1) of strength `scale` (DiRT's `OcclusionMapScale`: 0 none,
/// 1 whole): the light the material keeps. DiRT applies it to the ambient light alone, which the renderer's own lights do
/// not separate, so the bake darkens the colour and takes this much of it, in linear light (the factor on the
/// colours as stored is its 2.2th root).
fn kept(ao: f32, scale: f32) -> f32 {
    (1.0 - scale * (1.0 - ao)).powf(1.0 / 2.2)
}

/// `diffuse` with `occlusion` baked into them and `specular` as its alpha: the plane over
/// `reference`, the specular of the material's plain finish, which keeps its alpha at 1 (a bare metal, brighter,
/// does too: the clear coat does not reflect more than it). Without a specular map the alpha is the texture's.
pub fn diffuse(diffuse: &Texture, occlusion: Option<&(Plane, f32)>, specular: Option<&(Plane, f32)>) -> Vec<u8> {
    let (width, height) = (diffuse.width as usize, diffuse.height as usize);
    let keeps =
        occlusion.map(|(plane, scale)| (plane, std::array::from_fn::<f32, 256, _>(|i| kept(i as f32 / 255.0, *scale))));
    let mut rgba = diffuse.rgba();
    for (i, p) in rgba.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i % width, i / width);
        let keep = keeps.map_or(1.0, |(plane, keeps)| keeps[usize::from(plane.at(x, y, width, height))]);
        for c in &mut p[..3] {
            *c = (f32::from(*c) * keep).round() as u8;
        }
        p[3] = specular.map_or(p[3], |(plane, reference)| {
            (f32::from(plane.at(x, y, width, height)) / reference).clamp(0.0, 255.0).round() as u8
        });
    }
    encode_bc7(specular.is_some() || diffuse.format == Format::Dxt5, diffuse.width, diffuse.height, rgba)
}

/// A normal map as the renderer's bump reads it ([`repacked`]), BC7.
pub fn normal(map: &Texture, crumpled: bool) -> Vec<u8> {
    encode_bc7(false, map.width, map.height, repacked(map, crumpled))
}

/// `map`'s texels as the renderer's bump reads them: red x, green y along v (down the image), blue and alpha 255 (z is
/// rebuilt). DiRT's maps are DXT5 with the x in the alpha and the y in the green (`cme_main_nm`: red and blue copy the
/// green), or plain RGB (`int_cme_nm`, DXT1); the green runs up the image (`Texture::normal_dds`: 35 of 44 terrain maps
/// integrate to heights that follow their diffuse with it so, and so do the Escort's tread, disc, suspension and grille
/// maps). The slopes are taken about the map's own flat value, its most common one. With `crumpled`, the map's panels
/// are flattened first ([`flattened`]).
fn repacked(map: &Texture, crumpled: bool) -> Vec<u8> {
    let mut rgba = map.rgba();
    let slopes = |channel: usize| -> Vec<f32> {
        let mut count = [0u32; 256];
        rgba.chunks_exact(4).for_each(|p| count[usize::from(p[channel])] += 1);
        let flat = (0..256).max_by_key(|&v| count[v]).unwrap_or(128) as f32;
        rgba.chunks_exact(4).map(|p| (f32::from(p[channel]) - flat) / 127.5).collect()
    };
    let (mut x, mut y) = (slopes(if map.format == Format::Dxt5 { 3 } else { 0 }), slopes(1));
    if crumpled {
        flattened(&mut x, &mut y, map.width as usize, map.height as usize);
    }
    let byte = |slope: f32| ((slope * 127.5 + 127.5).round().clamp(0.0, 255.0)) as u8;
    for (i, p) in rgba.chunks_exact_mut(4).enumerate() {
        p.copy_from_slice(&[byte(x[i]), byte(-y[i]), 255, 255]);
    }
    rgba
}

/// The slopes of a body's normal map without the crumpled panels it carries for damage: the doors, bonnet, roof and
/// wings hold a foil-like crumple (it shows as creases over the whole door in the renderer) that DiRT blends in as the
/// car is hit. They are broad: a blur wider than the hardware's features (bolts, vents, edges, a pixel or a few wide)
/// keeps them and loses it, so where the blurred slope is large the slopes go to flat, a texel or four past it, softly.
fn flattened(x: &mut [f32], y: &mut [f32], width: usize, height: usize) {
    let (mut lx, mut ly) = (x.to_vec(), y.to_vec());
    for plane in [&mut lx, &mut ly] {
        blur(plane, width, height, CRUMPLE_BLUR, 3);
    }
    let mut broad: Vec<f32> = lx.iter().zip(&ly).map(|(a, b)| a.hypot(*b)).collect();
    blur(&mut broad, width, height, CRUMPLE_BLUR / 2, 3);
    let mut crumple: Vec<f32> = broad
        .iter()
        .map(|&m| {
            let t = ((m - CRUMPLE_SLOPE.0) / (CRUMPLE_SLOPE.1 - CRUMPLE_SLOPE.0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        })
        .collect();
    dilate(&mut crumple, width, height, CRUMPLE_BLUR * 2 / 3);
    blur(&mut crumple, width, height, 1, 2);
    for (i, keep) in crumple.iter().enumerate() {
        x[i] *= 1.0 - keep;
        y[i] *= 1.0 - keep;
    }
}

/// The blur's radius in texels, and the blurred slope (its length) where a crumple starts and is certain (the maps of a
/// 2048 body: the hardware's own blur stays under 0.05, the panels' is 0.1 to 0.5).
const CRUMPLE_BLUR: usize = 6;
const CRUMPLE_SLOPE: (f32, f32) = (0.05, 0.12);

/// `values` blurred `passes` times by a box of `radius` texels either side (clamped at the edges), rows then columns:
/// three passes come close to a Gaussian.
fn blur(values: &mut [f32], width: usize, height: usize, radius: usize, passes: usize) {
    let line = |get: &dyn Fn(usize) -> usize, len: usize, values: &mut [f32]| {
        let mut source: Vec<f32> = (0..len).map(|i| values[get(i)]).collect();
        for _ in 0..passes {
            let mut sums = vec![0.0f32; len + 1];
            for i in 0..len {
                sums[i + 1] = sums[i] + source[i];
            }
            source = (0..len)
                .map(|i| {
                    let (lo, hi) = (i.saturating_sub(radius), (i + radius + 1).min(len));
                    (sums[hi] - sums[lo]) / (hi - lo) as f32
                })
                .collect();
        }
        source.iter().enumerate().for_each(|(i, v)| values[get(i)] = *v);
    };
    for row in 0..height {
        line(&|i| row * width + i, width, values);
    }
    for column in 0..width {
        line(&|i| i * width + column, height, values);
    }
}

/// `values` with each texel the greatest within `radius` texels either side along rows then columns.
fn dilate(values: &mut [f32], width: usize, height: usize, radius: usize) {
    let line = |get: &dyn Fn(usize) -> usize, len: usize, values: &mut [f32]| {
        let source: Vec<f32> = (0..len).map(|i| values[get(i)]).collect();
        for i in 0..len {
            let (lo, hi) = (i.saturating_sub(radius), (i + radius + 1).min(len));
            values[get(i)] = source[lo..hi].iter().copied().fold(0.0, f32::max);
        }
    };
    for row in 0..height {
        line(&|i| row * width + i, width, values);
    }
    for column in 0..width {
        line(&|i| i * width + column, height, values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxt1_of(width: u32, height: u32, colour: [u8; 3]) -> Texture {
        let pack = |c: [u8; 3]| (u16::from(c[0] >> 3) << 11) | (u16::from(c[1] >> 2) << 5) | u16::from(c[2] >> 3);
        let block: Vec<u8> = [pack(colour).to_le_bytes(), pack(colour).to_le_bytes(), [0; 2], [0; 2]].concat();
        let blocks = (width / 4 * height / 4) as usize;
        Texture { name: "t".into(), width, height, format: Format::Dxt1, levels: 1, data: block.repeat(blocks) }
    }

    #[test]
    fn full_occlusion_scale_keeps_the_light_the_map_says() {
        assert!((kept(1.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((kept(0.0, 0.0) - 1.0).abs() < 1e-6);
        // In linear light the share is the map's, over the 2.2 power the colours as stored have.
        assert!((kept(0.5, 1.0).powf(2.2) - 0.5).abs() < 1e-5);
        assert!((kept(0.0, 0.4).powf(2.2) - 0.6).abs() < 1e-5);
    }

    #[test]
    fn a_plane_is_read_at_another_textures_texels() {
        let texture = dxt1_of(8, 8, [255, 255, 255]);
        let plane = Plane::of(&texture, 0);
        assert_eq!(plane.at(7, 7, 16, 16), 255);
        assert_eq!(plane.at(15, 0, 16, 16), plane.at(7, 0, 8, 8));
    }

    #[test]
    fn the_baked_diffuse_is_a_bc7_with_its_mips() {
        let texture = dxt1_of(16, 16, [200, 100, 50]);
        let dark = (Plane::of(&dxt1_of(4, 4, [0, 0, 0]), 0), 1.0);
        let dds = diffuse(&texture, Some(&dark), Some(&dark));
        assert_eq!(&dds[84..88], b"DX10");
        let file = rbr_formats::dds::parse(&dds).unwrap();
        assert_eq!((file.width, file.height), (16, 16));
        assert!(file.levels().len() > 1);
    }

    #[test]
    fn a_normal_map_takes_alpha_as_x_and_flips_green() {
        // A DXT5 map flat but for its first block, whose x (alpha) is 255 and y (green) 0: a slope of +1 in x and -1 in
        // y about the flat (128, 128), so red 255 and green, flipped, 255.
        let flat = dxt1_of(8, 8, [128, 128, 128]);
        let mut data = Vec::new();
        for (i, colour) in flat.data.chunks_exact(8).enumerate() {
            let (alpha, colour) = if i == 0 { (255, &[0; 8][..]) } else { (128, colour) };
            data.extend([alpha, alpha, 0, 0, 0, 0, 0, 0].iter().chain(colour));
        }
        let map = Texture { format: Format::Dxt5, data, ..flat };
        let texels = repacked(&map, false);
        let (first, flat) = (&texels[..4], &texels[(5 * 8 + 5) * 4..][..4]);
        assert!(first[0] >= 254 && first[1] == 255 && first[2..] == [255, 255], "{first:?}");
        assert!(flat[0].abs_diff(128) <= 1 && flat[1].abs_diff(128) <= 1, "{flat:?}");
        assert_eq!(&normal(&map, false)[84..88], b"DX10");
    }
}
