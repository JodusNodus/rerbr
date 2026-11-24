//! The instanced BTB meshes on the GPU ([`crate::stage::placed`]): each model's vertices once, in packs of their own after
//! the stage's (their vertices are in the model's space, so the bakes of the sky's occlusion and the bounce leave
//! them out: [`MeshBuffers::baked`]), the placements in one instance buffer, and a [`GpuMesh`] per group that draws its
//! model once for each placement in its range.
//!
//! Each placed vertex keeps the ambient the bakes give it as if it were expanded into the world (ours, [`Ambient`]):
//! every placement is expanded into packs of its own for the bakes, and after them the four words of each vertex's
//! occlusion, sky direction and bounce go into one storage buffer ([`Ambient::records`], 16 bytes a placed vertex
//! against the expanded vertex's 88), which `instanced` in `shader.wgsl` reads by the placement's [`Header`].

use super::meshes::{GpuDraw, GpuMesh, MeshBuffers, MeshSlot, Packing};
use crate::relight::Ground;
use crate::foliage;
use crate::stage::placed::{Group, Instance, Placed};
use crate::stage::{Anchor, Draw, Lod, Mesh, Vertex};
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use std::ops::Range;
use wgpu::util::DeviceExt;

/// The models of `placed` as meshes, for the vertex preparations the stage's meshes get (the sun's normals, the
/// plants): their draws are those of the groups that draw them.
pub(super) fn model_meshes(placed: &Placed) -> Vec<Mesh> {
    placed
        .models
        .iter()
        .enumerate()
        .map(|(model, m)| {
            let mut draws: Vec<Draw> = Vec::new();
            for g in placed.groups.iter().filter(|g| g.model == model) {
                let draw = Draw { indices: 0..m.indices.len(), state: g.state, lod: Lod::Always };
                if !draws.iter().any(|d| d.state == draw.state) {
                    draws.push(draw);
                }
            }
            Mesh {
                layer: crate::stage::Layer::Solid,
                anchor: Anchor::World,
                bounds: None,
                cell: None,
                vertices: m.vertices.clone(),
                indices: m.indices.clone(),
                draws,
                sun_cascades: true,
            }
        })
        .collect()
}

/// A model vertex's [`Vertex::occlusion`] when it takes the ground's normal under its placement, as
/// [`Ground::on_foliage`] gives an expanded plant's (the cut-out draws'); [`ON_CROWN`] on a crown's, whose normal is
/// the way out of its crown, to be blended with the ground's as [`foliage::with_plants`] does. The model's own ambient
/// fields are otherwise unused: the placed vertex's come from [`Ambient::records`].
pub(super) const ON_GROUND: f32 = 1.0;
pub(super) const ON_CROWN: f32 = 2.0;

/// Each model's placements' middle scale (the cube root of the turn's determinant): a model's plants are told apart and
/// shaped at that size ([`prepared_model`]), as the expanded placements are in the world.
pub(super) fn model_scales(placed: &Placed) -> Vec<f32> {
    let mut scales = vec![Vec::new(); placed.models.len()];
    for (instance, model) in placed.instances.iter().zip(instance_models(placed)) {
        let r = instance.rows;
        let turn = glam::Mat3::from_cols_array(&[r[0][0], r[1][0], r[2][0], r[0][1], r[1][1], r[2][1], r[0][2], r[1][2], r[2][2]]);
        scales[model].push(turn.determinant().abs().cbrt());
    }
    scales
        .into_iter()
        .map(|mut s| {
            s.sort_by(f32::total_cmp);
            s.get(s.len() / 2).copied().filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0)
        })
        .collect()
}

/// A model's `vertices` (of `mesh`, with the sun's normals) prepared as a stage mesh's: with the `ground`'s normals
/// its cut-out plants marked [`ON_GROUND`] (the placement's ground goes in in `instanced`), then the plants
/// ([`foliage::with_plants`]) found at `scale` (grass, crowns and panels are told apart by their size), a crown's
/// vertices marked [`ON_CROWN`] with the way out of the crown alone as their normal.
pub(super) fn prepared_model<'a>(
    mut vertices: Vec<Vertex>,
    mesh: &Mesh,
    scale: f32,
    ground: bool,
    plant: impl Fn(&Draw) -> Option<&'a foliage::Cut> + Copy,
) -> Vec<Vertex> {
    if ground {
        let plants = mesh.draws.iter().filter(|d| d.state.shade.is_none() && d.state.alpha_ref.is_some());
        for &i in plants.flat_map(|d| &mesh.indices[d.indices.clone()]) {
            let v = &mut vertices[i as usize];
            (v.normal, v.occlusion) = ([0.0, 1.0, 0.0], ON_GROUND);
        }
    }
    let scaled = |vertices: Vec<Vertex>, by: f32| -> Vec<Vertex> {
        vertices.into_iter().map(|v| Vertex { position: (Vec3::from(v.position) * by).to_array(), ..v }).collect()
    };
    let mut vertices = scaled(foliage::with_plants(scaled(vertices, scale), mesh, plant), 1.0 / scale);
    for v in vertices.iter_mut().filter(|v| v.occlusion == ON_GROUND && v.specular[0] == foliage::TREE_CARD) {
        // `with_plants` gave `normalize(out (1 - CROWN_UP) + up CROWN_UP)` with CROWN_UP a half: `out` is `up` mirrored
        // about it.
        let n = Vec3::from(v.normal);
        (v.normal, v.occlusion) = ((2.0 * n.y * n - Vec3::Y).normalize_or(Vec3::Y).to_array(), ON_CROWN);
    }
    vertices
}

/// `v` (a model vertex) placed by `instance` with the `ground`'s normal under it (zero: none), as `instanced` in
/// `shader.wgsl` places it.
fn placed_vertex(v: &Vertex, instance: &Instance, ground: Vec3) -> Vertex {
    let mut placed = instance.place(v);
    if ground != Vec3::ZERO && v.occlusion == ON_GROUND {
        placed.normal = ground.to_array();
    } else if ground != Vec3::ZERO && v.occlusion == ON_CROWN {
        let out = Vec3::from(placed.normal);
        placed.normal = (out * (1.0 - foliage::CROWN_UP) + ground * foliage::CROWN_UP).normalize_or(Vec3::Y).to_array();
    }
    Vertex { occlusion: 0.0, ..placed }
}

/// A placement's word for `instanced` (`ambient` in `InstanceIn`, location 15, read once per instance): where its
/// vertices' ambient records start less its model's first vertex (wrapping, so that `x + vertex_index` is the record);
/// the ground's normal under it, packed as the sky's directions are (0: none); whether the records are baked.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Header {
    pub(crate) record: u32,
    pub(crate) ground: u32,
    pub(crate) unused: u32,
    pub(crate) baked: u32,
}

/// Each instance's model, in instance order.
pub(super) fn instance_models(placed: &Placed) -> Vec<usize> {
    let mut model = vec![0; placed.instances.len()];
    for group in &placed.groups {
        for k in group.instances.clone() {
            model[k as usize] = group.model;
        }
    }
    model
}

/// The ambient records of all placed vertices: each instance's model's vertex count, in instance order.
pub(super) fn record_count(placed: &Placed) -> usize {
    instance_models(placed).iter().map(|&m| placed.models[m].vertices.len()).sum()
}

/// The models uploaded, before their packs are placed after the stage's.
pub(super) struct Models {
    packing: Packing,
    /// Each model's slot and first index.
    at: Vec<(MeshSlot, u32)>,
    /// Each model's vertices as uploaded.
    vertices: Vec<Vec<Vertex>>,
    instances: wgpu::Buffer,
    headers: wgpu::Buffer,
    /// Each instance's ground normal (zero: none).
    grounds: Vec<Vec3>,
}

impl Models {
    /// Uploads `meshes` ([`model_meshes`]) with the vertices `prepared` where they differ from their own, the instances
    /// and their headers; with `baked`, the records are to be baked ([`Models::finish`]). A plant's vertices take the
    /// `ground`'s normal under their placement.
    pub(super) fn new(
        device: &wgpu::Device,
        placed: &Placed,
        meshes: &[Mesh],
        prepared: Vec<Option<Vec<Vertex>>>,
        ground: Option<&Ground>,
        baked: bool,
    ) -> Models {
        let mut packing = Packing::default();
        let vertices: Vec<Vec<Vertex>> =
            meshes.iter().zip(prepared).map(|(mesh, prepared)| prepared.unwrap_or_else(|| mesh.vertices.clone())).collect();
        let at: Vec<(MeshSlot, u32)> =
            meshes.iter().zip(&vertices).map(|(mesh, v)| packing.add(device, v, &mesh.indices, false)).collect();
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("instances"),
            contents: bytemuck::cast_slice(&placed.instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grounds: Vec<Vec3> = placed
            .instances
            .iter()
            .map(|i| ground.map_or(Vec3::ZERO, |g| g.normal_at(Vec3::new(i.rows[0][3], i.rows[1][3], i.rows[2][3]))))
            .collect();
        let mut record = 0u32;
        let headers: Vec<Header> = instance_models(placed)
            .iter()
            .zip(&grounds)
            .map(|(&model, &ground)| {
                let header = Header {
                    record: record.wrapping_sub(at[model].0.base_vertex() as u32),
                    ground: if ground == Vec3::ZERO { 0 } else { packed_direction(ground) },
                    unused: 0,
                    baked: u32::from(baked),
                };
                record += vertices[model].len() as u32;
                header
            })
            .collect();
        let headers = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("instance headers"),
            contents: bytemuck::cast_slice(&headers),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Models { packing, at, vertices, instances, headers, grounds }
    }

    /// The first index of `model` in its pack.
    pub(super) fn first_index(&self, model: usize) -> u32 {
        self.at[model].1
    }

    /// The mesh of `group` with its `draws` (of its model's indices at [`Models::first_index`]): its slot is in the
    /// model packs, to be moved on by [`Models::finish`].
    pub(super) fn mesh(&self, group: &Group, draws: Vec<GpuDraw>) -> GpuMesh {
        let b = |b: crate::stage::Bounds| (b.center, b.half_extent);
        GpuMesh {
            bounds: Some(b(group.bounds)),
            cell: group.cell.map(b),
            visible: true,
            outside: None,
            reflected: false,
            anchor: Anchor::World,
            layer: group.layer,
            slot: self.at[group.model].0,
            draws,
            interactive: None,
            in_car: None,
            car: false,
            sky_dome: false,
            backdrop_bounds: None,
            sun_cascades: group.sun_cascades,
            instances: Some(group.instances.clone()),
        }
    }

    /// Puts the model packs after the stage's `buffers`, moving the slots of the instanced `meshes` to them; with
    /// `records` (to be baked), every placement expanded into packs after them, for [`Ambient::apply`].
    pub(super) fn finish(
        self,
        device: &wgpu::Device,
        placed: &Placed,
        buffers: &mut Vec<MeshBuffers>,
        meshes: &mut [GpuMesh],
        records: Option<&wgpu::Buffer>,
    ) -> Option<Ambient> {
        let first = buffers.len();
        for mesh in meshes.iter_mut().filter(|m| m.instances.is_some()) {
            mesh.slot = mesh.slot.shifted(first);
        }
        buffers.extend(self.packing.finish(device).into_iter().map(|pack| MeshBuffers {
            baked: false,
            instances: Some((self.instances.clone(), self.headers.clone())),
            ..pack
        }));
        let records = records?.clone();
        let start = buffers.len();
        let mut packing = Packing::default();
        for ((instance, model), ground) in placed.instances.iter().zip(instance_models(placed)).zip(&self.grounds) {
            let expanded: Vec<Vertex> = self.vertices[model]
                .iter()
                .map(|v| placed_vertex(v, instance, *ground))
                .collect();
            packing.add(device, &expanded, &[], false);
        }
        buffers.extend(packing.finish(device));
        Some(Ambient { packs: start..buffers.len(), records })
    }
}

/// The placed vertices' ambient, to be baked: every placement expanded into world vertices in packs of their own
/// (in instance order, as the records lie), which the bakes of the sky's occlusion and the bounce write into as into the
/// stage's. [`Ambient::apply`] keeps their four words in [`Ambient::records`] and drops the packs.
pub(super) struct Ambient {
    /// The expanded packs in the renderer's buffers, the last.
    packs: Range<usize>,
    /// One `[u32; 4]` per placed vertex: its occlusion, sky direction and bounce as the bakes leave them in a
    /// [`Vertex`].
    records: wgpu::Buffer,
}

impl Ambient {
    /// Copies the baked words into the records and drops the expanded packs from `buffers`; leaves the records zero
    /// (no occlusion, no bounce) where a read fails.
    pub(super) fn apply(self, device: &wgpu::Device, queue: &wgpu::Queue, buffers: &mut Vec<MeshBuffers>) {
        let mut at = 0u64;
        for pack in buffers.drain(self.packs) {
            let Some(vertices) = read_vertices(device, queue, &pack.vertices, u64::from(pack.vertex_count)) else {
                return;
            };
            let words: Vec<[u32; 4]> = vertices.iter().map(record).collect();
            eprintln!("DEBUG ambient pack {} vertices, mean occlusion {}", vertices.len(), vertices.iter().map(|v| v.occlusion).sum::<f32>() / vertices.len() as f32);
            queue.write_buffer(&self.records, at, bytemuck::cast_slice(&words));
            at += (words.len() * size_of::<[u32; 4]>()) as u64;
        }
    }
}

/// A baked vertex's ambient record (`placed_ambient` in `shader.wgsl`).
fn record(v: &Vertex) -> [u32; 4] {
    [v.occlusion.to_bits(), v.sky_dir, v.bounce[0], v.bounce[1]]
}

/// The first `count` vertices of `buffer`, read back; `None` when the read fails.
fn read_vertices(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer, count: u64) -> Option<Vec<Vertex>> {
    let size = count.max(1) * size_of::<Vertex>() as u64;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instanced ambient"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, size);
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok().and_then(|_| receiver.recv().ok()?.ok())?;
    let vertices = bytemuck::pod_collect_to_vec(&readback.slice(..).get_mapped_range().ok()?);
    readback.unmap();
    Some(vertices)
}

/// `d` packed as `sky_occlusion.wgsl`'s `packed_direction` (octahedral about y, two snorms; never 0), which
/// `sky_direction` in `shader.wgsl` unpacks.
fn packed_direction(d: Vec3) -> u32 {
    let mut p = Vec2::new(d.x, d.z) / (d.x.abs() + d.y.abs() + d.z.abs());
    if d.y < 0.0 {
        let sign = Vec2::new(if p.x >= 0.0 { 1.0 } else { -1.0 }, if p.y >= 0.0 { 1.0 } else { -1.0 });
        p = (Vec2::ONE - Vec2::new(p.y.abs(), p.x.abs())) * sign;
    }
    let snorm = |x: f32| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16 as u16 as u32;
    (snorm(p.x) | (snorm(p.y) << 16)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packed_direction_unpacks_as_the_shader_does() {
        for d in [Vec3::Y, Vec3::new(0.3, 0.9, -0.2), Vec3::new(-0.6, 0.1, 0.7), Vec3::new(0.2, -0.5, 0.4)] {
            let d = d.normalize();
            let w = packed_direction(d);
            let snorm = |x: u32| ((x & 0xffff) as u16 as i16 as f32 / 32767.0).max(-1.0);
            let p = Vec2::new(snorm(w), snorm(w >> 16));
            let mut u = Vec3::new(p.x, 1.0 - p.x.abs() - p.y.abs(), p.y);
            if u.y < 0.0 {
                u = Vec3::new((1.0 - p.y.abs()) * p.x.signum(), u.y, (1.0 - p.x.abs()) * p.y.signum());
            }
            assert!(u.normalize().dot(d) > 0.9999, "{d} {u}");
        }
    }
}
