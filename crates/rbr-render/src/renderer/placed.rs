//! The instanced BTB meshes on the GPU ([`crate::stage::placed`]): each model's vertices once, in packs of their own after
//! the stage's (their vertices are in the model's space, so the bakes of the sky's occlusion and the bounce leave
//! them out: [`MeshBuffers::baked`]), the placements in one instance buffer, and a [`GpuMesh`] per group that draws its
//! model once for each placement in its range.

use super::meshes::{GpuDraw, GpuMesh, MeshBuffers, MeshSlot, Packing};
use crate::stage::placed::{Group, Placed};
use crate::stage::{Anchor, Draw, Lod, Mesh, Vertex};
use bytemuck::Zeroable;
use glam::Vec3;
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

/// One vertex per instance, at the middle of its placed box and facing up: the point that the bakes of the sky's occlusion
/// and the bounce find the light of the whole placement at (`probe` in `InstanceIn`), in the order of the instances.
fn probes(placed: &Placed) -> Vec<Vertex> {
    let mut probes = vec![Vertex::zeroed(); placed.instances.len()];
    for group in &placed.groups {
        let model = &placed.models[group.model];
        let (low, high) = model.vertices.iter().fold((Vec3::MAX, Vec3::MIN), |(low, high), v| {
            (low.min(Vec3::from(v.position)), high.max(Vec3::from(v.position)))
        });
        for k in group.instances.clone() {
            let rows = placed.instances[k as usize].rows;
            // The placed box (over its eight corners): the probe sits at its middle.
            let at = |c: usize, corner: Vec3| {
                rows[c][0] * corner.x + rows[c][1] * corner.y + rows[c][2] * corner.z + rows[c][3]
            };
            let corners = (0..8).map(|i| Vec3::select(glam::BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0), high, low));
            let (mut min, mut max) = (Vec3::MAX, Vec3::MIN);
            for corner in corners {
                let p = Vec3::new(at(0, corner), at(1, corner), at(2, corner));
                (min, max) = (min.min(p), max.max(p));
            }
            probes[k as usize] = Vertex {
                position: [(min.x + max.x) / 2.0, (min.y + max.y) / 2.0, (min.z + max.z) / 2.0],
                normal: [0.0, 1.0, 0.0],
                ..Vertex::zeroed()
            };
        }
    }
    probes
}

/// The models uploaded, before their packs are placed after the stage's.
pub(super) struct Models {
    packing: Packing,
    /// Each model's slot and first index.
    at: Vec<(MeshSlot, u32)>,
    instances: wgpu::Buffer,
    /// Where the probes lie in the stage's packing, one per instance.
    probes: MeshSlot,
}

impl Models {
    /// Uploads `meshes` ([`model_meshes`]) with the vertices `prepared` where they differ from their own, and the
    /// instances; their probes go into `stage`, the stage's own packing, where the bakes of the sky's occlusion and the
    /// bounce write into them.
    pub(super) fn new(
        device: &wgpu::Device,
        stage: &mut Packing,
        placed: &Placed,
        meshes: &[Mesh],
        prepared: Vec<Option<Vec<Vertex>>>,
    ) -> Models {
        let mut packing = Packing::default();
        let at = meshes
            .iter()
            .zip(prepared)
            .map(|(mesh, prepared)| {
                let vertices = prepared.as_deref().unwrap_or(&mesh.vertices);
                packing.add(device, vertices, &mesh.indices, false)
            })
            .collect();
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("instances"),
            contents: bytemuck::cast_slice(&placed.instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Models { packing, at, instances, probes: stage.add(device, &probes(placed), &[], false).0 }
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

    /// Puts the model packs after the stage's `buffers`, moving the slots of the instanced `meshes` to them.
    pub(super) fn finish(self, device: &wgpu::Device, buffers: &mut Vec<MeshBuffers>, meshes: &mut [GpuMesh]) {
        let first = buffers.len();
        let probes = &buffers[self.probes.buffer()];
        let probes = (probes.vertices.clone(), self.probes.base_vertex() as u64 * size_of::<Vertex>() as u64);
        for mesh in meshes.iter_mut().filter(|m| m.instances.is_some()) {
            mesh.slot = mesh.slot.shifted(first);
        }
        buffers.extend(self.packing.finish(device).into_iter().map(|pack| MeshBuffers {
            baked: false,
            instances: Some(self.instances.clone()),
            probes: Some(probes.clone()),
            ..pack
        }));
    }
}
