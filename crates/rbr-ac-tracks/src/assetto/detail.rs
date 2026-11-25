//! AC's detail shaders (`ksPerPixelMultiMap_NMDetail` and `_AT_NMDetail`, `useDetail` 1): the diffuse times a detail
//! texture repeated `detailUVMultiplier` times over the diffuse's UVs, faded out by the diffuse's alpha,
//! `diffuse * lerp(detail, 1, diffuse.a)` on all four channels (the compiled `ksPerPixelMultiMap_NMDetail_ps`). The
//! diffuse's alpha is a detail mask there, not a transparency. A BTB material has one texture, so one repeat of the
//! diffuse is baked at the detail's resolution; the detail is periodic in it when the multiplier is a whole number.
use crate::assetto::kn5::Model;
use crate::assetto::multilayer::{fit, sample, texture};
use rbr_btb_writer::textures::Image;

/// The largest side of a detail bake: four times the usual 1024 px road diffuse, 8 MB as DXT1.
const LIMIT: u32 = 4096;

/// The baked diffuse of a detail material, or `None` when it uses no detail or a texture is missing.
pub fn bake(model: &Model, index: usize) -> Option<Image> {
    let mat = &model.materials[index];
    let repeat = f64::from(mat.property("detailUVMultiplier", 0.));
    if mat.property("useDetail", 0.) <= 0. || repeat <= 0. {
        return None;
    }
    let (diffuse, detail) = (texture(model, mat, "txDiffuse")?, texture(model, mat, "txDetail")?);
    let side = |base: u32, detail: u32| {
        let want = (f64::from(detail) * repeat).max(f64::from(base)).min(f64::from(LIMIT)) as u32;
        want.next_power_of_two().min(LIMIT).max(base.min(LIMIT))
    };
    let (width, height) = (side(diffuse.0, detail.0), side(diffuse.1, detail.1));
    let detail = fit(detail, [f64::from(width) / repeat, f64::from(height) / repeat]);
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let rows = (height as usize).div_ceil(threads);
    std::thread::scope(|scope| {
        for (n, chunk) in rgba.chunks_mut(rows * width as usize * 4).enumerate() {
            let (diffuse, detail) = (&diffuse, &detail);
            scope.spawn(move || {
                for (r, row) in chunk.chunks_mut(width as usize * 4).enumerate() {
                    let v = ((n * rows + r) as f64 + 0.5) / f64::from(height);
                    for (x, out) in row.chunks_mut(4).enumerate() {
                        let u = (x as f64 + 0.5) / f64::from(width);
                        let base = sample(diffuse, u, v, true);
                        let d = sample(detail, u * repeat, v * repeat, true);
                        let mask = base[3] / 255.;
                        for k in 0..4 {
                            out[k] = (base[k] * (d[k] / 255. * (1. - mask) + mask)).clamp(0., 255.).round() as u8;
                        }
                    }
                }
            });
        }
    });
    Some((width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assetto::kn5::Material;
    use std::collections::BTreeMap;

    fn model(diffuse: [u8; 4], detail: [u8; 4], repeat: f32) -> Model {
        let dds = |c: [u8; 4]| rbr_formats::dds::rgba8_file(2, 2, &c.repeat(4));
        let mut material = Material { shader: "ksPerPixelMultiMap_NMDetail".into(), ..Material::default() };
        material.textures = [("txDiffuse", "d"), ("txDetail", "t")].map(|(s, n)| (s.into(), n.into())).into();
        material.properties = [("useDetail", 1.), ("detailUVMultiplier", repeat)].map(|(k, v)| (k.into(), v)).into();
        Model {
            version: 6,
            textures: [("d", diffuse), ("t", detail)].map(|(n, c)| (n.to_string(), dds(c))).into(),
            materials: vec![material],
            meshes: Vec::new(),
            markers: BTreeMap::new(),
            dropped_triangles: 0,
            repaired_uvs: 0,
            repaired_normals: 0,
        }
    }

    #[test]
    fn the_detail_multiplies_where_the_diffuse_alpha_is_low() {
        // Alpha 0: the diffuse times the detail; the bake is the detail's size times the multiplier.
        let (w, h, rgba) = bake(&model([200, 100, 50, 0], [128, 255, 0, 255], 4.), 0).unwrap();
        assert_eq!((w, h), (8, 8));
        assert_eq!(&rgba[..3], &[100, 100, 0]);
        // Alpha 255: the detail does not show.
        let (_, _, rgba) = bake(&model([200, 100, 50, 255], [128, 255, 0, 255], 4.), 0).unwrap();
        assert_eq!(&rgba[..4], &[200, 100, 50, 255]);
    }

    #[test]
    fn without_use_detail_nothing_is_baked() {
        let mut m = model([200, 100, 50, 0], [128, 255, 0, 255], 4.);
        m.materials[0].properties.insert("useDetail".into(), 0.);
        assert!(bake(&m, 0).is_none());
    }
}
