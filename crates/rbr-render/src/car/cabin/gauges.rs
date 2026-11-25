//! The dial faces of the interior set (ours): where the RBRHUD gauges' dials are on the car's textures, and the
//! [`GAUGE`] mark on the vertices of the triangles over them.

use super::{GAUGE, INTERIOR};
use crate::stage::{Image, Mesh, Stage, TextureRef, TextureSet};
use glam::Vec2;
use rbr_formats::rbrhud::{self, Atlas, Kind};
use std::collections::BTreeMap;
use std::path::Path;

/// A dial face: a circle in a texture's uv, its centre and its radius along u and v.
pub type Face = (Vec2, Vec2);

/// The dial faces of the RBRHUD gauges of the car whose folder is `car_folder` (the ones [`crate::dials`] draws), under
/// the install `root`, by the texture of `stage` each is on. Ours: the faces are not in the data, the needles are,
/// so a face is the circle about a needle's pivot as far as the needle reaches, and a quarter more ([`FACE`]).
pub fn gauge_faces(root: &Path, car_folder: &str, stage: &Stage) -> BTreeMap<TextureRef, Vec<Face>> {
    let mut faces: BTreeMap<TextureRef, Vec<Face>> = BTreeMap::new();
    let Some((folder, configs)) = rbrhud::variant(root, car_folder) else { return faces };
    for config in configs {
        let Some(atlas) = rbr_formats::cars::resolve(&folder, &config.atlas)
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| Atlas::parse(&String::from_utf8_lossy(&bytes)))
        else {
            continue;
        };
        let needles: Vec<(Vec2, f32)> = config
            .elements
            .iter()
            .filter_map(|element| {
                let Kind::Rotating(rotating) = &element.kind else { return None };
                let [_, _, w, h] = atlas.sprite(element.texture.as_deref()?)?;
                let pivot = Vec2::from(rotating.pivot);
                let reach = [(0.0, 0.0), (w as f32, 0.0), (0.0, h as f32), (w as f32, h as f32)]
                    .map(|corner| (Vec2::from(corner) - pivot).length())
                    .into_iter()
                    .fold(0.0, f32::max);
                Some((Vec2::new(element.x, element.y), reach * element.scale * FACE))
            })
            .collect();
        for (&key, texture) in &stage.textures {
            let file = texture.request.rsplit(['\\', '/']).next().unwrap_or_default();
            let Some(Image::Dial { texels, .. }) = &texture.image else { continue };
            if key.set == TextureSet::Car && file.eq_ignore_ascii_case(&config.texture) {
                let size = Vec2::new(texels.0 as f32, texels.1 as f32);
                faces.entry(key).or_default().extend(needles.iter().map(|&(at, reach)| (at / size, reach / size)));
            }
        }
    }
    faces
}

/// A dial face's radius against its needle's reach.
const FACE: f32 = 1.25;

/// Marks [`GAUGE`] the baked vertices of `interior`'s triangles whose middle is on one of `faces` (in the uv of their
/// draw's first texture). The cabin keeps their light at least at the game's (`cabin.wgsl`): under the dash's hood they
/// would read darker than in the game, and until the exposure follows the cabin the eye cannot adapt to them. True when
/// any changed.
pub fn mark_gauges(mesh: &mut Mesh, faces: &BTreeMap<TextureRef, Vec<Face>>) -> bool {
    let Mesh { draws, indices, vertices, .. } = mesh;
    let mut marked = false;
    for draw in draws.iter() {
        let Some(faces) = draw.state.textures[0].and_then(|t| faces.get(&t)) else { continue };
        for triangle in indices[draw.indices.clone()].chunks_exact(3) {
            let middle = triangle.iter().map(|&i| Vec2::from(vertices[i as usize].uv0)).sum::<Vec2>() / 3.0;
            if faces.iter().any(|&(centre, radius)| ((middle - centre) / radius).length() <= 1.0) {
                for &i in triangle {
                    let vertex = &mut vertices[i as usize];
                    if vertex.specular[1].floor() == INTERIOR {
                        vertex.specular[1] += GAUGE - INTERIOR;
                        marked = true;
                    }
                }
            }
        }
    }
    marked
}
