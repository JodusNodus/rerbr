//! The vista's ground colour (`terrain_track_vista_d4.fx`), baked into one texture per material.
//!
//! The shader draws the distant hills as the near terrain's three layers (`TDiffuseSpecMap1..3`: earth, bush, rock) blended
//! by `TlargeBlendMap` (a 1024 map on the large map's uv: red the first layer, blue the second, green the third, by eye against the reference) and
//! multiplied by `TlargeColourMap` (times 2, like `ColourMapScale`). The large colour map alone is a grey-blue tint, not
//! the ground's colour. Seen from the road the layers' own detail is far below a pixel, so each layer is taken at its mean
//! colour and the result keeps the colour map's light and dark.
use crate::material::Material;
use crate::texture::Library;
use rbr_ac_tracks::btb::textures;

/// The mean colour of an image's RGB, 0 to 1.
fn mean(px: &[u8]) -> [f32; 3] {
    let mut sum = [0f64; 3];
    for p in px.chunks_exact(4) {
        (0..3).for_each(|k| sum[k] += f64::from(p[k]));
    }
    let n = (px.len() / 4).max(1) as f64;
    std::array::from_fn(|k| (sum[k] / n / 255.) as f32)
}

/// The vista's baked ground as a DDS the size of its large colour map; `None` unless every map it needs is real.
pub fn diffuse(m: &Material, library: &mut Library) -> Option<Vec<u8>> {
    let mut get = |input: &str| {
        let t = library.get(m.textures.get(input)?).filter(|t| !t.is_placeholder())?;
        Some((t.width as usize, t.height as usize, t.rgba()))
    };
    let (w, h, colour) = get("TlargeColourMap")?;
    let (mw, mh, blend) = get("TlargeBlendMap")?;
    let layers: [[f32; 3]; 3] = [
        mean(&get("TDiffuseSpecMap1")?.2),
        mean(&get("TDiffuseSpecMap2")?.2),
        mean(&get("TDiffuseSpecMap3")?.2),
    ];
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let b = &blend[((y * mh / h) * mw + x * mw / w) * 4..];
            // Red is the first layer, blue the second, green the third.
            let weights = [f32::from(b[0]), f32::from(b[2]), f32::from(b[1])];
            let total = weights.iter().sum::<f32>().max(1.);
            let c = &colour[(y * w + x) * 4..];
            for k in 0..3 {
                let ground: f32 = (0..3).map(|l| weights[l] / total * layers[l][k]).sum();
                out.push(((ground * f32::from(c[k]) / 255. * 2.).clamp(0., 1.) * 255.).round() as u8);
            }
            out.push(255);
        }
    }
    Some(textures::encode_bc7(false, w as u32, h as u32, out))
}
