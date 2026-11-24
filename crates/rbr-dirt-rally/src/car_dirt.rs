//! The mud a DiRT car takes, in the renderer's form.
//!
//! The renderer blends a face toward the dirt colour by the alpha of a mask on the face's first UVs (the friendly name
//! `Dirt` of `CarPaint` and `CarTexture`). DiRT's mud is the class's `body_mud` (alpha: arches, sills, the lower body),
//! on a layout of its own that every body mesh carries as its second UV set. [`bake`] lays that mask onto the body
//! atlas: each triangle is rasterised in the atlas's UVs and each texel takes the mud under the same point of the
//! triangle in the mud layout.
use crate::car_maps::Plane;

/// The mask's size, texels a side.
pub const SIZE: usize = 1024;

/// How far past a triangle's edge its texels are filled, so that the sampling's filter finds no gap at the seams.
const BLEED: usize = 2;

/// A triangle's corners in the atlas's UVs and in the mud's.
pub type Triangle = ([[f32; 2]; 3], [[f32; 2]; 3]);

/// The mud mask on the atlas's layout, `size` by `size` alpha bytes; texels no triangle covers are 0.
pub fn bake(triangles: impl IntoIterator<Item = Triangle>, mud: &Plane, size: usize) -> Vec<u8> {
    let mut mask = vec![0u8; size * size];
    let mut covered = vec![false; size * size];
    let s = size as f32;
    for (atlas, layout) in triangles {
        let p = atlas.map(|[u, v]| [u * s, v * s]);
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-6 || !area.is_finite() {
            continue;
        }
        let range = |k: usize| {
            let (lo, hi) = p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| (lo.min(c[k]), hi.max(c[k])));
            (lo.floor().max(0.0) as usize, (hi.ceil().max(0.0) as usize).min(size))
        };
        let ((x0, x1), (y0, y1)) = (range(0), range(1));
        for y in y0..y1 {
            for x in x0..x1 {
                let c = [x as f32 + 0.5, y as f32 + 0.5];
                let edge =
                    |a: [f32; 2], b: [f32; 2]| ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / area;
                let w = [edge(p[1], p[2]), edge(p[2], p[0]), edge(p[0], p[1])];
                if w.iter().any(|&w| w < 0.0) {
                    continue;
                }
                let at = |k: usize| (0..3).map(|i| w[i] * layout[i][k]).sum::<f32>();
                mask[y * size + x] = mud.sample(at(0), at(1));
                covered[y * size + x] = true;
            }
        }
    }
    for _ in 0..BLEED {
        let before = covered.clone();
        for i in (0..size * size).filter(|&i| !before[i]) {
            let (x, y) = (i % size, i / size);
            let near = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
            if let Some(j) =
                near.iter().filter(|&&(x, y)| x < size && y < size).map(|&(x, y)| y * size + x).find(|&j| before[j])
            {
                (mask[i], covered[i]) = (mask[j], true);
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_takes_the_mud_under_the_same_point_of_its_layout() {
        // The mud is 0 on its left half and 200 on its right; the triangle covers the atlas's lower left half and lies on
        // the mud's right half.
        let mud = Plane::from_values((0..16).map(|i| if i % 4 < 2 { 0 } else { 200 }).collect(), 4, 4);
        let mask = bake([([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], [[0.6, 0.1], [0.9, 0.1], [0.6, 0.9]])], &mud, 8);
        assert_eq!(mask[0], 200);
        assert_eq!(mask[8 * 3 + 3], 200);
        // The far corner is outside the triangle, and past the bleed.
        assert_eq!(mask[8 * 7 + 7], 0);
    }

    #[test]
    fn an_uncovered_texel_is_clear() {
        let mud = Plane::from_values(vec![255; 4], 2, 2);
        let mask = bake([([[0.0, 0.0], [0.25, 0.0], [0.0, 0.25]], [[0.0; 2]; 3])], &mud, 16);
        assert_eq!(mask[0], 255);
        assert_eq!(mask[16 * 15 + 15], 0);
    }
}
