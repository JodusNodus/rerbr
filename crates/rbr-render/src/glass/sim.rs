//! The windscreen's simulated water (ours, "Simulated" windscreen effects; `sim.wgsl` steps it, `sim_draw.wgsl` draws
//! it). The fine water on the glass, which the Standard level draws as a procedural film's beads, is a set of beads
//! (centre, radius, velocity) in the glass's chart, in a fixed array of [`CAPACITY`] slots on the GPU. Rain lands as
//! small beads at the rain's rate, beads that touch run together (volume kept), a bead past its pin runs under the felt
//! gravity and the airflow, gathers the beads in its path and sheds a few small ones, and the blades push what lies in
//! their way ahead of them. The model's own drops keep running and drawing as before; the ones that run cut through the
//! beads on their path.
//!
//! [`Queue`] (the CPU's half) turns each step the glass takes into fixed [`SUBSTEP`]s (their time kept from step to step,
//! the spawns seeded by the substep's number), so that the simulation advances by the step clock's time and a run
//! repeats to the bit; [`Gpu`] holds the buffers and records the substeps' passes.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use super::model::FILM_CELLS;
use crate::gpu_util;
use crate::uploads::Uploads;

pub(crate) const SHADER: &str = include_str!("sim.wgsl");
/// The bead slots (`sim.wgsl`'s `CAP`).
pub const CAPACITY: u32 = 1 << 16;
/// The simulation's fixed step, seconds.
pub const SUBSTEP: f32 = 1.0 / 120.0;
/// The grid's cell, metres (the glass mask's texel too).
pub const PITCH: f32 = 0.0015;
/// The most substeps kept for the next frame: a glass unseen for longer than 9 s starts from the later part.
const MOST_STEPS: usize = 1100;
/// The most running drops cutting through the beads at once.
const MOST_CUTTERS: usize = 1024;
/// The film's corners across a pane and up it (`sim_draw.wgsl`'s `SIM_COLUMNS` + 1 by `SIM_ROWS` + 1).
pub const CORNERS: usize = (FILM_CELLS[0] + 1) * (FILM_CELLS[1] + 1);
/// A step's block in the steps' buffer (the uniform's dynamic offset).
const STRIDE: u64 = 256;
/// The pass's thread groups of 64.
const GROUP: u32 = 64;

/// The rain's beads landing a second on a square metre of glass at full intensity, standing and more per m/s of speed,
/// and the share gusts add or take.
const RAIN_STILL: f32 = 8000.0;
const RAIN_PER_SPEED: f32 = 200.0;
const GUSTS: f32 = 0.35;

/// The grid of the glass's chart the simulation and its mask lie in: cells of [`PITCH`] from `lo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub lo: Vec2,
    pub hi: Vec2,
    pub columns: u32,
    pub rows: u32,
}

impl Grid {
    /// The grid over the chart's `bounds`.
    pub fn new([lo, hi]: [Vec2; 2]) -> Grid {
        let cells = ((hi - lo) / PITCH).ceil().max(Vec2::ONE).min(Vec2::splat(2048.0));
        Grid { lo, hi, columns: cells.x as u32, rows: cells.y as u32 }
    }

    /// The chart point at the middle of cell `column`, `row`.
    pub fn centre(&self, column: u32, row: u32) -> Vec2 {
        self.lo + Vec2::new(column as f32 + 0.5, row as f32 + 0.5) * PITCH
    }
}

/// One substep's block of the steps' buffer (`sim.wgsl`'s `Step`).
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct Step {
    lo: [f32; 2],
    hi: [f32; 2],
    gravity: [f32; 2],
    dt: f32,
    speed: f32,
    phase: [f32; 2],
    tick: u32,
    spawn: u32,
    grid: [u32; 4],
    pitch: [f32; 4],
    wipers: [[f32; 4]; 4],
    pad: [u32; 28],
}

/// A running drop of the model, cutting through the beads along `from` to `to` (chart metres) as wide as `radius`.
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct Cutter {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub radius: f32,
    pub pad: [f32; 3],
}

/// What the simulation needs once: its grid and the mask of the glass over it (255 on the glass), row by row.
#[derive(Debug, Clone)]
pub struct Setup {
    pub grid: Grid,
    pub mask: Vec<u8>,
}

/// How the beads sit on the glass seen from this frame's camera (stage space): the film's corners row by row, and the
/// pane's across and up.
#[derive(Debug, Clone)]
pub struct View {
    pub corners: Vec<[f32; 4]>,
    pub across: [f32; 3],
    pub up: [f32; 3],
}

/// What the simulation takes of a frame ([`Queue::take`]).
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// The beads begin again from none (a seek, a restart).
    pub reset: bool,
    pub setup: Option<Setup>,
    pub steps: Vec<Step>,
    pub cutters: Vec<Cutter>,
    /// The camera's view of the glass; none: the beads are not drawn this frame.
    pub view: Option<View>,
}

/// One step of the glass for the simulation: its time, the rain (the shower's share), the car's speed, the pull in the
/// chart (gravity less the car's acceleration) and the blades' angle fraction before and after.
#[derive(Debug, Clone, Copy)]
pub struct Input {
    pub dt: f32,
    pub rain: f32,
    pub speed: f32,
    pub gravity: Vec2,
    pub phases: [f32; 2],
}

/// The CPU's half: the glass's steps cut into [`SUBSTEP`]s.
#[derive(Debug, Clone)]
pub struct Queue {
    grid: Grid,
    area: f32,
    setup: Option<Setup>,
    /// Time not yet stepped, the substeps counted, their time, and the raindrops not yet landed.
    carry_time: f32,
    tick: u32,
    clock: f32,
    carry_rain: f32,
    steps: Vec<Step>,
    reset: bool,
}

impl Queue {
    /// A queue over `grid` for glass of `area` square metres, whose mask (`Grid::columns` by `rows`) is `mask`.
    pub fn new(grid: Grid, mask: Vec<u8>, area: f32) -> Queue {
        Queue {
            grid,
            area,
            setup: Some(Setup { grid, mask }),
            carry_time: 0.0,
            tick: 0,
            clock: 0.0,
            carry_rain: 0.0,
            steps: Vec::new(),
            reset: true,
        }
    }

    /// The beads begin again from none, with this frame's steps.
    pub fn clear(&mut self) {
        self.carry_time = 0.0;
        self.tick = 0;
        self.clock = 0.0;
        self.carry_rain = 0.0;
        self.steps.clear();
        self.reset = true;
    }

    /// The gusts' share of the rain at `clock` seconds.
    fn gust(clock: f32) -> f32 {
        1.0 + GUSTS * (0.65 * (clock * 0.7).sin() + 0.35 * (clock * 1.9 + 1.3).sin())
    }

    /// The substeps of a glass step `input`.
    pub fn advance(&mut self, input: Input) {
        self.carry_time += input.dt;
        let n = (self.carry_time / SUBSTEP).floor().max(0.0) as u32;
        self.carry_time -= n as f32 * SUBSTEP;
        let phase = |k: u32| input.phases[0] + (input.phases[1] - input.phases[0]) * (k as f32 / n.max(1) as f32);
        for k in 0..n {
            self.clock += SUBSTEP;
            let rate = input.rain * (RAIN_STILL + RAIN_PER_SPEED * input.speed.max(0.0)) * Self::gust(self.clock);
            self.carry_rain += rate * self.area * SUBSTEP;
            let spawn = self.carry_rain.floor().max(0.0);
            self.carry_rain -= spawn;
            self.tick = self.tick.wrapping_add(1);
            self.steps.push(Step {
                lo: self.grid.lo.to_array(),
                hi: self.grid.hi.to_array(),
                gravity: input.gravity.to_array(),
                dt: SUBSTEP,
                speed: input.speed,
                phase: [phase(k), phase(k + 1)],
                tick: self.tick,
                spawn: spawn as u32,
                grid: [self.grid.columns, self.grid.rows, 0, 0],
                pitch: [PITCH, 0.0, 0.0, 0.0],
                wipers: [[0.0; 4]; 4],
                pad: [0; 28],
            });
        }
        if self.steps.len() > MOST_STEPS {
            self.steps.drain(..self.steps.len() - MOST_STEPS);
        }
    }

    /// The steps since the last call, and whether the beads begin again and the grid and mask, the first time.
    pub fn take(&mut self, cutters: Vec<Cutter>, view: Option<View>) -> Frame {
        Frame {
            reset: std::mem::take(&mut self.reset),
            setup: self.setup.take(),
            steps: std::mem::take(&mut self.steps),
            cutters,
            view,
        }
    }
}

/// The passes of one substep, in order, with the number of threads each wants (0: the bead slots, 1: the grid's cells,
/// 2: the slots and the substep's raindrops).
const PASSES: [(&str, u8); 7] =
    [("clear_grid", 1), ("integrate", 0), ("find_target", 0), ("merge", 0), ("apply", 0), ("claim", 2), ("create", 2)];

/// The simulation on the GPU: the beads' buffers and the passes that step them.
pub struct Gpu {
    beads: wgpu::Buffer,
    acc: wgpu::Buffer,
    claims: wgpu::Buffer,
    sheds: wgpu::Buffer,
    cutters: wgpu::Buffer,
    view: wgpu::Buffer,
    steps: wgpu::Buffer,
    steps_capacity: u64,
    layout: wgpu::BindGroupLayout,
    pipelines: Vec<wgpu::ComputePipeline>,
    cut: wgpu::ComputePipeline,
    /// The grid's buffer and mask, once set up, and the bind group over them.
    grid: Option<(Grid, wgpu::Buffer, wgpu::TextureView)>,
    group: Option<wgpu::BindGroup>,
    /// The substeps not yet recorded (a frame that draws no glass leaves them for the next), the frame's cutters,
    /// whether to begin from none, and whether the beads are drawn.
    pending: Vec<Step>,
    cutting: u32,
    reset: bool,
    drawable: bool,
}

fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn buffer(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
}

impl Gpu {
    pub fn new(device: &wgpu::Device) -> Gpu {
        let slots = u64::from(CAPACITY);
        let rw = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass sim"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                ),
                entry(1, storage(false)),
                entry(2, storage(false)),
                entry(3, storage(false)),
                entry(4, storage(false)),
                entry(5, storage(false)),
                entry(
                    6,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(7, storage(true)),
            ],
        });
        let module = gpu_util::shader(device, "glass sim", SHADER);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass sim"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Gpu {
            beads: buffer(device, "glass beads", slots * 32, rw),
            acc: buffer(device, "glass beads' sums", slots * 32, rw),
            claims: buffer(device, "glass beads' claims", slots * 4, rw),
            sheds: buffer(device, "glass beads shed", slots * 16, rw),
            cutters: buffer(device, "glass cutters", (MOST_CUTTERS * size_of::<Cutter>()) as u64, rw),
            view: gpu_util::uniform_buffer(device, "glass beads' view", size_of::<ViewBlock>()),
            steps: buffer(
                device,
                "glass sim steps",
                STRIDE * 64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            steps_capacity: 64,
            pipelines: PASSES.iter().map(|(name, _)| pipeline(name)).collect(),
            cut: pipeline("cut"),
            layout,
            grid: None,
            group: None,
            pending: Vec::new(),
            cutting: 0,
            reset: false,
            drawable: false,
        }
    }

    /// The beads' buffer and the view's, for the glass pass's bind group.
    pub fn resources(&self) -> (wgpu::Buffer, wgpu::Buffer) {
        (self.beads.clone(), self.view.clone())
    }

    /// Whether the beads are drawn this frame.
    pub fn drawable(&self) -> bool {
        self.drawable && self.grid.is_some()
    }

    /// Takes the frame's work: the setup, the view, the cutters and the substeps (each with the wipers' `wipers`).
    pub(crate) fn set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uploads: &mut Uploads,
        mut frame: Frame,
        wipers: [[f32; 4]; 4],
    ) {
        if let Some(Setup { grid, mask }) = frame.setup {
            let size = wgpu::Extent3d { width: grid.columns, height: grid.rows, depth_or_array_layers: 1 };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("glass mask"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &mask,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(grid.columns), rows_per_image: None },
                size,
            );
            let cells = u64::from(grid.columns) * u64::from(grid.rows);
            let cells = buffer(device, "glass beads' grid", cells * 4, wgpu::BufferUsages::STORAGE);
            self.grid = Some((grid, cells, texture.create_view(&Default::default())));
            self.group = None;
        }
        if frame.reset {
            self.pending.clear();
        }
        self.reset |= frame.reset;
        self.drawable = frame.view.is_some();
        if let Some(view) = &frame.view {
            let mut block = ViewBlock::zeroed();
            block.across = [view.across[0], view.across[1], view.across[2], 0.0];
            block.up = [view.up[0], view.up[1], view.up[2], 0.0];
            if let Some((grid, _, _)) = &self.grid {
                block.lo = grid.lo.to_array();
                block.hi = grid.hi.to_array();
            }
            for (to, from) in block.corners.iter_mut().zip(&view.corners) {
                *to = *from;
            }
            uploads.write(&self.view, 0, bytemuck::bytes_of(&block));
        }
        frame.cutters.truncate(MOST_CUTTERS);
        self.cutting = frame.cutters.len() as u32;
        if !frame.cutters.is_empty() {
            uploads.write(&self.cutters, 0, bytemuck::cast_slice(&frame.cutters));
        }
        self.pending.append(&mut frame.steps);
        if self.pending.len() > MOST_STEPS {
            self.pending.drain(..self.pending.len() - MOST_STEPS);
        }
        for step in &mut self.pending {
            step.wipers = wipers;
            step.grid[2] = self.cutting;
        }
        if (self.pending.len() as u64) > self.steps_capacity {
            self.steps_capacity = (self.pending.len() as u64).next_power_of_two();
            self.steps = buffer(
                device,
                "glass sim steps",
                STRIDE * self.steps_capacity,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            self.group = None;
        }
        if !self.pending.is_empty() {
            uploads.write(&self.steps, 0, bytemuck::cast_slice(&self.pending));
        }
        if self.group.is_none() {
            if let Some((_, cells, mask)) = &self.grid {
                use wgpu::BindingResource::{Buffer, TextureView};
                let steps =
                    Buffer(wgpu::BufferBinding { buffer: &self.steps, offset: 0, size: NonZeroU64::new(STRIDE) });
                self.group = Some(gpu_util::bind_group(
                    device,
                    "glass sim",
                    &self.layout,
                    &[
                        steps,
                        self.beads.as_entire_binding(),
                        cells.as_entire_binding(),
                        self.acc.as_entire_binding(),
                        self.claims.as_entire_binding(),
                        self.sheds.as_entire_binding(),
                        TextureView(mask),
                        self.cutters.as_entire_binding(),
                    ],
                ));
            }
        }
    }

    /// Records the frame's substeps' passes, then the cutters'.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let (Some((grid, _, _)), Some(group)) = (&self.grid, &self.group) else { return };
        if std::mem::take(&mut self.reset) {
            encoder.clear_buffer(&self.beads, 0, None);
            encoder.clear_buffer(&self.acc, 0, None);
        }
        if self.pending.is_empty() {
            return;
        }
        let cells = grid.columns * grid.rows;
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("glass sim"), timestamp_writes: None });
        for (k, step) in self.pending.iter().enumerate() {
            pass.set_bind_group(0, group, &[(k as u64 * STRIDE) as u32]);
            for ((_, threads), pipeline) in PASSES.iter().zip(&self.pipelines) {
                let wanted = match threads {
                    0 => CAPACITY,
                    1 => cells,
                    _ => CAPACITY + step.spawn,
                };
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(wanted.div_ceil(GROUP), 1, 1);
            }
        }
        if self.cutting > 0 {
            // After the last substep, over its grid (its block's `grid.z` counts the cutters).
            pass.set_bind_group(0, group, &[((self.pending.len() - 1) as u64 * STRIDE) as u32]);
            pass.set_pipeline(&self.cut);
            pass.dispatch_workgroups(self.cutting.div_ceil(GROUP), 1, 1);
        }
        self.pending.clear();
        self.cutting = 0;
    }
}

/// What the glass pass binds for the beads while there is no simulation: one dead slot and an empty view.
pub fn placeholders(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::Buffer) {
    (
        buffer(device, "glass beads, none", 32, wgpu::BufferUsages::STORAGE),
        gpu_util::uniform_buffer(device, "glass beads' view, none", size_of::<ViewBlock>()),
    )
}

/// `sim_draw.wgsl`'s `SimView`.
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct ViewBlock {
    across: [f32; 4],
    up: [f32; 4],
    lo: [f32; 2],
    hi: [f32; 2],
    corners: [[f32; 4]; CORNERS],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_block_is_one_stride() {
        assert_eq!(size_of::<Step>() as u64, STRIDE);
    }

    #[test]
    fn the_queue_cuts_steps_into_fixed_substeps_and_keeps_the_time() {
        let grid = Grid::new([Vec2::ZERO, Vec2::new(1.0, 0.5)]);
        let mut queue = Queue::new(grid, vec![255; (grid.columns * grid.rows) as usize], 0.5);
        let input = Input { dt: 1.0 / 60.0, rain: 1.0, speed: 0.0, gravity: Vec2::ZERO, phases: [0.0, 0.1] };
        let frame_of = |q: &mut Queue| q.take(Vec::new(), None);
        for _ in 0..60 {
            queue.advance(input);
        }
        let frame = frame_of(&mut queue);
        assert!(frame.reset && frame.setup.is_some());
        assert!((119..=121).contains(&frame.steps.len()), "a second is 120 substeps: {}", frame.steps.len());
        assert_eq!(frame.steps[0].tick, 1);
        let spawned: u32 = frame.steps.iter().map(|s| s.spawn).sum();
        assert!(spawned > 1000, "heavy rain on half a square metre lands a few thousand beads: {spawned}");
        // The blades' phase runs through the substeps of a step.
        assert_eq!(frame.steps[0].phase[0], 0.0);
        assert!((frame.steps[1].phase[0] - frame.steps[0].phase[1]).abs() < 1e-6);
        // The same inputs give the same substeps.
        let mut again = Queue::new(grid, Vec::new(), 0.5);
        for _ in 0..60 {
            again.advance(input);
        }
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&frame.steps),
            bytemuck::cast_slice::<_, u8>(&frame_of(&mut again).steps)
        );
        assert!(!frame_of(&mut queue).reset, "taken once");
    }
}
