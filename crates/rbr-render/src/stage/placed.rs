//! A BTB stage's instanced placements (`ReRbrInstanced=1`, [`super::btb`]): each mesh's vertices stored once, in its own
//! space, and a transform per placement ([`Instance`]) that the vertex shader applies (`vs_main_placed`), where the
//! other placements are expanded into world-space vertices.

use super::mesh::{Bounds, DrawState, Layer, Lod, Vertex};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::collections::HashMap;
use std::ops::Range;

/// The instanced placements of a stage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Placed {
    pub models: Vec<Model>,
    /// Drawn like meshes: by layer, cell and band.
    pub groups: Vec<Group>,
    /// Every group's placements, each group's in a range of its own ([`Group::instances`]).
    pub instances: Vec<Instance>,
}

/// One mesh in its own space.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// As the mesh's placements would be expanded, but untransformed: positions and normals local, colour's alpha the
    /// lerp weight, the rest of the colour left to the instance's light ([`Instance::shade`]).
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// A model's placements of one cell and band, drawn in one layer with one state.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub model: usize,
    pub layer: Layer,
    pub state: DrawState,
    pub lod: Lod,
    /// Casts into the sun's near cascades ([`super::Mesh::sun_cascades`]).
    pub sun_cascades: bool,
    /// Into [`Placed::instances`].
    pub instances: Range<u32>,
    /// The union of the boxes of all placements of its layer, cell, band and cascade flag, instanced or not
    /// ([`Builder::cover`]), which the views cull and band by, and the visibility cell: the expanded mesh's.
    pub bounds: Bounds,
    pub cell: Option<Bounds>,
}

impl Placed {
    /// Triangles over all groups, each once per placement.
    pub fn triangle_count(&self) -> usize {
        self.groups.iter().map(|g| self.models[g.model].indices.len() / 3 * g.instances.len()).sum()
    }
}

/// [`Instance::flags`] bit: an `Up` technique, which lights with `N = (0, 1, 0)`.
pub const UP: u32 = 1;

/// A placement: the matrix's three output columns as rows, so that `dot(rows[c], (p, 1))` is the placed `p`'s
/// component `c` (`scene::place_point`), and what the effects' light needs.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct Instance {
    pub rows: [[f32; 4]; 3],
    /// Toward the sun.
    pub to_sun: [f32; 3],
    /// [`UP`] bits, as a float (the vertex attribute is a vec4).
    pub flags: f32,
}

/// The `ambient` of every BTB material (`materials.ini`, 0.4 in all folders): the light's floor.
pub const AMBIENT: f32 = 0.4;

impl Instance {
    pub fn new(placement: &[f32; 16], to_sun: Vec3, up: bool) -> Instance {
        let m = placement;
        Instance {
            rows: [0, 1, 2].map(|c| [m[c], m[4 + c], m[8 + c], m[12 + c]]),
            to_sun: to_sun.into(),
            flags: if up { UP as f32 } else { 0.0 },
        }
    }

    pub fn up(&self) -> bool {
        self.flags as u32 & UP != 0
    }

    /// `v` placed, as `vs_main_placed` does it: position and normal by the rows (`scene::place_normal`'s, normalised),
    /// the colour `0.4 + N.L` the effects light with, to 1/255.
    pub fn place(&self, v: &Vertex) -> Vertex {
        let [x, y, z] = v.position;
        let n = v.normal;
        let at = |row: &[f32; 4]| row[0] * x + row[1] * y + row[2] * z + row[3];
        let turned = Vec3::from(self.rows.map(|r| r[0] * n[0] + r[1] * n[1] + r[2] * n[2]));
        let normal = if self.up() {
            Vec3::Y
        } else if turned.length() > 0.0 {
            turned / turned.length()
        } else {
            turned
        };
        let light = (AMBIENT + normal.dot(Vec3::from(self.to_sun)).max(0.0)).min(1.0);
        let shade = (light * 255.0).round() as u8;
        Vertex {
            position: self.rows.map(|r| at(&r)),
            color: [shade, shade, shade, v.color[3]],
            normal: normal.into(),
            ..*v
        }
    }
}

/// The meshes' bounds and visibility cell for a group of placements spanning `low` to `high` and shown over `lod`: a
/// mesh whose own range reaches past the game's cells (`visibility::DRAW_REACH`) is culled by that range alone (a
/// converted stage's far scenery, kept to the far plane).
pub fn bounds_and_cell(low: Vec3, high: Vec3, lod: Lod) -> (Bounds, Option<Bounds>) {
    let bounds = Bounds { center: (low + high) / 2.0, half_extent: (high - low) / 2.0 };
    let far_scenery = matches!(lod, Lod::Band { far, .. } if f32::from(far) > crate::visibility::DRAW_REACH);
    (bounds, (!far_scenery).then_some(bounds))
}

/// Collects the placements into [`Placed`]: one group per model, cell, band and cascade flag. A model is a mesh drawn in one
/// layer ([`Builder::model`]): a translucent mesh's cut-out and blended draws are two, as the expanded placements are two
/// meshes, whose plants' normals differ (`Ground::on_foliage` takes the cut-out draws alone).
#[derive(Default)]
pub struct Builder {
    models: Vec<Model>,
    by_mesh: HashMap<(usize, Layer), usize>,
    by_key: HashMap<(usize, (i32, i32), Lod, bool), usize>,
    buckets: Vec<Bucket>,
    /// The box of every placement, instanced or not, by the expanded meshes' key (layer, cell, band, cascade flag): a
    /// group takes its key's, so that it shows and turns its band where the expanded mesh would.
    cells: HashMap<CellKey, (Vec3, Vec3)>,
}

type CellKey = (Layer, (i32, i32), Lod, bool);

struct Bucket {
    model: usize,
    cell: (i32, i32),
    lod: Lod,
    sun_cascades: bool,
    state: (Layer, DrawState),
    instances: Vec<Instance>,
    low: Vec3,
    high: Vec3,
}

impl Builder {
    /// The model of source mesh `mesh` drawn in `layer`, made by `make` the first time.
    pub fn model(&mut self, mesh: usize, layer: Layer, make: impl FnOnce() -> Model) -> usize {
        *self.by_mesh.entry((mesh, layer)).or_insert_with(|| {
            self.models.push(make());
            self.models.len() - 1
        })
    }

    /// Counts a placement's box from `low` to `high` into the expanded meshes' key `key` ([`Builder::cells`]).
    pub fn cover(&mut self, key: CellKey, (low, high): (Vec3, Vec3)) {
        let cell = self.cells.entry(key).or_insert((low, high));
        *cell = (cell.0.min(low), cell.1.max(high));
    }

    /// Places `model` as `instance` in `cell`, drawn over `lod` with `state`, its box from `low` to `high`.
    pub fn add(
        &mut self,
        (model, cell, lod, sun_cascades): (usize, (i32, i32), Lod, bool),
        state: (Layer, DrawState),
        instance: Instance,
        (low, high): (Vec3, Vec3),
    ) {
        self.cover((state.0, cell, lod, sun_cascades), (low, high));
        let at = *self.by_key.entry((model, cell, lod, sun_cascades)).or_insert_with(|| {
            self.buckets.push(Bucket {
                model,
                cell,
                lod,
                sun_cascades,
                state,
                instances: Vec::new(),
                low: Vec3::splat(f32::MAX),
                high: Vec3::splat(f32::MIN),
            });
            self.buckets.len() - 1
        });
        let bucket = &mut self.buckets[at];
        bucket.instances.push(instance);
        bucket.low = bucket.low.min(low);
        bucket.high = bucket.high.max(high);
    }

    pub fn finish(self) -> Placed {
        let mut instances = Vec::new();
        let mut groups = Vec::new();
        for bucket in self.buckets {
            let first = instances.len() as u32;
            instances.extend(bucket.instances);
            let (layer, state) = bucket.state;
            let key = (layer, bucket.cell, bucket.lod, bucket.sun_cascades);
            let (low, high) = self.cells.get(&key).copied().unwrap_or((bucket.low, bucket.high));
            let (bounds, cell) = bounds_and_cell(low, high, bucket.lod);
            groups.push(Group {
                model: bucket.model,
                layer,
                state,
                lod: bucket.lod,
                sun_cascades: bucket.sun_cascades,
                instances: first..instances.len() as u32,
                bounds,
                cell,
            });
        }
        // Solid before blended, as the meshes are drawn.
        groups.sort_by_key(|g| g.layer);
        Placed { models: self.models, groups, instances }
    }
}
