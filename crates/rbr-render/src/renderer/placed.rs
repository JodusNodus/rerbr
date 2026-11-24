//! The instanced BTB meshes on the GPU ([`crate::stage::placed`]): each model's vertices once, in packs of their own after
//! the stage's (their vertices are in the model's space, so the bakes of the sky's occlusion and the bounce leave
//! them out: [`MeshBuffers::baked`]), the placements in one instance buffer, and a [`GpuMesh`] per group that draws its
//! model once for each placement in its range.
//!
//! The light of the sky and the bounce reach a model's vertices as the bake gives them to a few of its placements
//! ([`Ambient`]): each vertex keeps the mean share of its placement's probe that it sees, the mean direction it sees the
//! sky in and its mean bounce, which `instanced` in `shader.wgsl` puts together with each placement's probe.

use super::meshes::{GpuDraw, GpuMesh, MeshBuffers, MeshSlot, Packing};
use crate::stage::placed::{Group, Instance, Placed};
use crate::stage::{Anchor, Draw, Lod, Mesh, Vertex};
use bytemuck::Zeroable;
use glam::{Mat3, Vec2, Vec3};
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
    /// Each model's vertices as uploaded.
    vertices: Vec<Vec<Vertex>>,
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
        let vertices: Vec<Vec<Vertex>> =
            meshes.iter().zip(prepared).map(|(mesh, prepared)| prepared.unwrap_or_else(|| mesh.vertices.clone())).collect();
        let at = meshes.iter().zip(&vertices).map(|(mesh, v)| packing.add(device, v, &mesh.indices, false)).collect();
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("instances"),
            contents: bytemuck::cast_slice(&placed.instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Models { packing, at, vertices, instances, probes: stage.add(device, &probes(placed), &[], false).0 }
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
    /// `bakes` (the sky's occlusion and the bounce to come), the packs of [`Ambient`]'s samples after them.
    pub(super) fn finish(
        self,
        device: &wgpu::Device,
        placed: &Placed,
        buffers: &mut Vec<MeshBuffers>,
        meshes: &mut [GpuMesh],
        bakes: bool,
    ) -> Option<Ambient> {
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
        if !bakes {
            return None;
        }
        let slots = self.at.iter().map(|(slot, _)| slot.shifted(first)).collect();
        let samples = samples(placed);
        let mut packing = Packing::default();
        for &(model, k) in &samples {
            let instance = &placed.instances[k as usize];
            let placed: Vec<Vertex> = self.vertices[model].iter().map(|v| instance.place(v)).collect();
            packing.add(device, &placed, &[], false);
        }
        let packs = buffers.len()..buffers.len();
        buffers.extend(packing.finish(device));
        Some(Ambient { vertices: self.vertices, slots, samples, probes: self.probes, packs: packs.start..buffers.len() })
    }
}

/// The most placements of a model whose bake its vertices take the mean of ([`samples`]): with every placement the
/// shares' error against the per-vertex bake was 0.069 (Pirttikulma) and 0.093 (Kaihuavaara 2022) rms, with 16 0.071
/// and 0.096.
const SAMPLES: usize = 16;

/// Up to [`SAMPLES`] placements of each model, spread over its placements in order: each model and instance.
fn samples(placed: &Placed) -> Vec<(usize, u32)> {
    let mut of_model = vec![Vec::new(); placed.models.len()];
    let mut seen = vec![false; placed.instances.len()];
    for group in &placed.groups {
        for k in group.instances.clone().filter(|&k| !std::mem::replace(&mut seen[k as usize], true)) {
            of_model[group.model].push(k);
        }
    }
    of_model
        .iter()
        .enumerate()
        .flat_map(|(model, ks)| ks.iter().step_by(ks.len().div_ceil(SAMPLES).max(1)).map(move |&k| (model, k)))
        .collect()
}

/// The ambient of the instanced models, to be baked (ours): a few placements of each model ([`samples`]) expanded into
/// world vertices in packs of their own, which the bakes of the sky's occlusion and the bounce write into as into the
/// stage's. [`Ambient::apply`] reads them back and gives each model vertex the mean over them ([`model_ambient`]).
pub(super) struct Ambient {
    vertices: Vec<Vec<Vertex>>,
    /// Each model's slot in the model packs.
    slots: Vec<MeshSlot>,
    samples: Vec<(usize, u32)>,
    probes: MeshSlot,
    /// The samples' packs in the renderer's buffers, the last.
    packs: Range<usize>,
}

impl Ambient {
    /// Writes the models' ambient into their vertices and drops the samples' packs from `buffers`; leaves the models
    /// as they were when a read fails.
    pub(super) fn apply(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        placed: &Placed,
        buffers: &mut Vec<MeshBuffers>,
    ) {
        let read = |buffer: &wgpu::Buffer, first: u64, count: u64| read_vertices(device, queue, buffer, first, count);
        let baked: Option<Vec<Vec<Vertex>>> =
            buffers.drain(self.packs).map(|b| read(&b.vertices, 0, u64::from(b.vertex_count))).collect();
        let probes = &buffers[self.probes.buffer()].vertices;
        let count = self.samples.iter().map(|&(_, k)| u64::from(k) + 1).max().unwrap_or(0);
        let probes = read(probes, self.probes.base_vertex() as u64, count);
        let (Some(baked), Some(probes)) = (baked, probes) else { return };
        let samples: Vec<Sample> = self
            .samples
            .iter()
            .map(|&(model, k)| Sample::of(model, &placed.instances[k as usize], &probes[k as usize]))
            .collect();
        for (model, vertices) in model_ambient(&self.vertices, &samples, &baked.concat()) {
            let slot = self.slots[model];
            let at = slot.base_vertex() as u64 * size_of::<Vertex>() as u64;
            queue.write_buffer(&buffers[slot.buffer()].vertices, at, bytemuck::cast_slice(&vertices));
        }
    }
}

/// `count` vertices of `buffer` from vertex `first`, read back; `None` when the read fails.
fn read_vertices(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer, first: u64, count: u64) -> Option<Vec<Vertex>> {
    let size = size_of::<Vertex>() as u64;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instanced ambient"),
        size: count.max(1) * size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, first * size, &readback, 0, count * size);
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

/// The least share of the sky a probe is taken to see, against which a vertex's own share is weighed.
const PROBE_FLOOR: f32 = 0.05;

/// A baked sample of a model ([`model_ambient`]): the model, the turn from the world into the model's space, and the
/// share of the sky its probe sees.
struct Sample {
    model: usize,
    to_model: Mat3,
    probe_seen: f32,
}

impl Sample {
    fn of(model: usize, instance: &Instance, probe: &Vertex) -> Sample {
        let r = instance.rows;
        let turn = Mat3::from_cols_array(&[r[0][0], r[1][0], r[2][0], r[0][1], r[1][1], r[2][1], r[0][2], r[1][2], r[2][2]]);
        let to_model = if turn.determinant().abs() > 1e-12 { turn.inverse() } else { Mat3::IDENTITY };
        Sample { model, to_model, probe_seen: (1.0 - probe.occlusion).max(PROBE_FLOOR) }
    }
}

/// The models' vertices (`vertices`) with their ambient, from the `samples`' baked vertices (`baked`, each sample's
/// after the one before): over its samples a vertex takes the mean of the share of the sky it sees over its probe's,
/// stored as `1 - share` in its occlusion (0: as the probe; `instanced` in `shader.wgsl` takes `(1 - occlusion) * (1 -
/// probe's)`); the mean direction it sees the sky in, in the model's space; and its mean bounce. Only the models
/// sampled, with their index.
fn model_ambient(vertices: &[Vec<Vertex>], samples: &[Sample], baked: &[Vertex]) -> Vec<(usize, Vec<Vertex>)> {
    #[derive(Clone, Copy, Default)]
    struct Sums {
        share: f32,
        direction: Vec3,
        bounce: [Vec3; 2],
    }
    let mut sums: Vec<Option<(f32, Vec<Sums>)>> = vec![None; vertices.len()];
    let mut first = 0;
    for sample in samples {
        let n = vertices[sample.model].len();
        let (count, model_sums) = sums[sample.model].get_or_insert_with(|| (0.0, vec![Sums::default(); n]));
        *count += 1.0;
        for (sum, v) in model_sums.iter_mut().zip(&baked[first..first + n]) {
            sum.share += (1.0 - v.occlusion) / sample.probe_seen;
            if v.sky_dir != 0 {
                sum.direction += (sample.to_model * unpacked_direction(v.sky_dir)).normalize_or_zero();
            }
            for (s, w) in sum.bounce.iter_mut().zip(v.bounce) {
                *s += unpacked_light(w);
            }
        }
        first += n;
    }
    sums.into_iter()
        .enumerate()
        .filter_map(|(model, s)| Some((model, s?)))
        .map(|(model, (count, sums))| {
            let vertices = vertices[model]
                .iter()
                .zip(sums)
                .map(|(v, s)| Vertex {
                    occlusion: 1.0 - s.share / count,
                    sky_dir: if s.direction == Vec3::ZERO { 0 } else { packed_direction(s.direction.normalize()) },
                    bounce: s.bounce.map(|b| packed_light(b / count)),
                    ..*v
                })
                .collect();
            (model, vertices)
        })
        .collect()
}

/// The direction `sky_occlusion.wgsl` packs octahedrally about y in two snorms (`sky_direction` in `shader.wgsl`).
fn unpacked_direction(w: u32) -> Vec3 {
    let snorm = |x: u32| ((x & 0xffff) as u16 as i16 as f32 / 32767.0).max(-1.0);
    let p = Vec2::new(snorm(w), snorm(w >> 16));
    let mut d = Vec3::new(p.x, 1.0 - p.x.abs() - p.y.abs(), p.y);
    if d.y < 0.0 {
        d = Vec3::new((1.0 - p.y.abs()) * p.x.signum(), d.y, (1.0 - p.x.abs()) * p.y.signum());
    }
    d.normalize_or_zero()
}

/// `d` packed as `sky_occlusion.wgsl`'s `packed_direction`: never 0 (none).
fn packed_direction(d: Vec3) -> u32 {
    let mut p = Vec2::new(d.x, d.z) / (d.x.abs() + d.y.abs() + d.z.abs());
    if d.y < 0.0 {
        let sign = Vec2::new(if p.x >= 0.0 { 1.0 } else { -1.0 }, if p.y >= 0.0 { 1.0 } else { -1.0 });
        p = (Vec2::ONE - Vec2::new(p.y.abs(), p.x.abs())) * sign;
    }
    let snorm = |x: f32| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16 as u16 as u32;
    (snorm(p.x) | (snorm(p.y) << 16)).max(1)
}

/// The bounce's packed light (`bounce_light` in `shader.wgsl`, `packed` in `bounce.wgsl`): three 10-bit shares of 2.
fn unpacked_light(w: u32) -> Vec3 {
    Vec3::new((w & 1023) as f32, ((w >> 10) & 1023) as f32, ((w >> 20) & 1023) as f32) * (2.0 / 1023.0)
}

fn packed_light(c: Vec3) -> u32 {
    let q = (c / 2.0).clamp(Vec3::ZERO, Vec3::ONE) * 1023.0;
    let q = q.round();
    q.x as u32 | (q.y as u32) << 10 | (q.z as u32) << 20
}
