//! The glass pass (`glass.wgsl`): the windscreen's drops as instanced quads, drawn after the frame over the kept
//! colour and depth (depth tested, no depth write), reading the frame for the drops' lenses. When the renderer has no
//! frame of its own to read (the screen-space reflections' one), the pass copies the frame into [`Pass::scene`] first.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::atmosphere::Atmosphere;
use crate::gpu_util;
use crate::uploads::Uploads;

/// One drop, flake or bug on the glass: its centre and half axes in stage space, its kind (0 water, 1 snow, 2 bug,
/// 3 mud, 4 a cell of the wet film, 5 mud seen from inside, 6 a cell of the pane itself seen from inside), a seed for its ragged edge (the film's streak length),
/// its opacity and its colour (snow, bugs and mud; the film's corners), and the pane's outward normal (kind 6; a film
/// cell's: its pane's cell size, landing clock and number, for the beads). A pane
/// cell's `centre` is its low corner, `axis_u`, `axis_v` and `far` the other corners' offsets from it, `tint` what
/// its glass lets through at [`PANE_TINTS`] + 1 points a side, row by row from the low corner, 4 bits each from the
/// low ones up (`pane_tint` in `glass.wgsl`), and `edge` how far inside the glass's outline each corner lies (low,
/// right, top, far), over [`PANE_EDGE`].
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct Drop {
    pub centre: [f32; 3],
    pub kind: f32,
    pub axis_u: [f32; 3],
    pub seed: f32,
    pub axis_v: [f32; 3],
    pub alpha: f32,
    pub color: [f32; 4],
    pub normal: [f32; 4],
    pub far: [f32; 3],
    pub tint: [u32; 4],
    pub edge: [i8; 4],
}

/// The steps a pane cell's `tint` takes across the cell (`glass.wgsl`'s `PANE_TINTS`): at the cell's 2 cm the
/// interpolated corners let the sky's light onto the lower 2 cm of a sun strip, a pale band over the black.
pub const PANE_TINTS: usize = 4;

/// A pane cell's `edge` at 1, metres (`glass.wgsl`'s `PANE_EDGE`).
pub const PANE_EDGE: f32 = 0.05;

pub(crate) const SHADER: &str = include_str!("glass.wgsl");
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 11] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 28, shader_location: 3 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 32, shader_location: 4 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 44, shader_location: 5 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 6 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 64, shader_location: 7 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 80, shader_location: 8 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32x4, offset: 92, shader_location: 9 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Snorm8x4, offset: 108, shader_location: 10 },
];

#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct FrameUniform {
    view_projection: [[f32; 4]; 4],
    screen: [f32; 4],
    light: [f32; 4],
    /// At night (renderer addition, `Atmosphere::night`): rgb the headlights' glow on the glass, w 1.
    night: [f32; 4],
    /// Seen from outside (ours): x 1, the wet layer of the film (`glass.wgsl`); 0 inside. y how wet the glass has been,
    /// 0 to 1 (the dried water spots); z how wet the stage is (the mud seen from inside).
    outside: [f32; 4],
    /// The eye, stage space.
    eye: [f32; 4],
    /// The way to the sun, stage space, w 1 by day; and its light on the glass, rgb.
    sun: [f32; 4],
    sun_light: [f32; 4],
    /// The windscreen's wipers ([`Pass::set_wipers`]), in `glass.wgsl`'s `Frame` as four fields.
    wipers: Wipers,
    /// The sun's first cascade (`sun::Cascades`), and x 1 when there are cascades.
    sun_matrix: [[f32; 4]; 4],
    shadow: [f32; 4],
}

/// The windscreen's wipers for the shader (`glass.wgsl`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Pod, Zeroable)]
#[repr(C)]
pub struct Wipers {
    /// At most two, each two rows: its pivot in the chart (metres) and its reach, then its park angle, its signed
    /// sweep and 1 (0: no wiper).
    pub wipers: [[f32; 4]; 4],
    /// Their last strokes, newest first ([`super::model::Stroke`]): the angle fraction each went from and to, and the
    /// seconds since it began and since it ended (0 while under way); none where from and to are equal.
    pub strokes: [[f32; 4]; super::model::STROKES],
    /// x the newest stroke's number; y how fast the film builds per second and z the most it holds
    /// ([`super::model::Glass::wetting`]).
    pub wiping: [f32; 4],
    /// The windscreen's film cells: the low corner of the first and a cell's size, chart metres.
    pub film: [f32; 4],
}

/// Ours: the glass takes the stage's ambient light and this much of its diffuse.
const DIFFUSE_SHARE: f32 = 0.5;
/// At night: the light the headlights' beams, scattered back off the rain and the road, throw on the glass.
const NIGHT_GLOW: [f32; 3] = [0.16, 0.155, 0.14];
const FIRST_CAPACITY: usize = 1024;

/// The fragment entry points of [`Pass`]'s pipelines.
const PIPELINES: [&str; 4] = ["fs_main", "fs_film", "fs_pane", "fs_film_half"];
/// [`PIPELINES`]' film cells, and their composite from the half-size target ([`HALF_FILM_FROM`]).
const FILM: usize = 1;
const FILM_HALF: usize = 3;

/// The frame height (pixels) from which the film's cells seen from inside are drawn at half size under
/// [`Detail::High`](crate::renderer::Detail::High), for their cost at 4K (ours): their beads then look as they do at half
/// the height. [`Detail::Ultra`](crate::renderer::Detail::Ultra) keeps them whole.
const HALF_FILM_FROM: u32 = 1800;

/// A colour target of `format` and `size` that the glass reads.
fn half_target(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> wgpu::TextureView {
    let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    gpu_util::target(device, "glass film, half size", size, format, usage)
}

/// Which of [`PIPELINES`] draws a drop of `kind`: the film's cells (4), the panes' cells (6), the rest.
fn pipeline_of(kind: f32) -> usize {
    match kind as u32 {
        4 => 1,
        6 => 2,
        _ => 0,
    }
}

/// The glass pipelines (`fs_main`, and `fs_film` and `fs_pane` for the film's and the panes' cells, apart so that the
/// other kinds don't run with their registers), the frame's drops and the resolved frame it reads when the renderer
/// has none.
pub struct Pass {
    pipelines: [wgpu::RenderPipeline; 4],
    /// The drops' runs, in order, and the pipeline of each.
    runs: Vec<(usize, std::ops::Range<u32>)>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The sun's cascades (`sun::SunShadow::view`, or a placeholder) and their comparison sampler.
    sun_map: wgpu::TextureView,
    sun_sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    capacity: usize,
    count: u32,
    scene: Option<((u32, u32), wgpu::TextureView)>,
    wipers: Wipers,
    /// The film's cells at half size ([`HALF_FILM_FROM`]): their pipeline (no depth), the frame's uniform for that
    /// size, the target of that size while used, and a texel standing in for it in the other draws.
    half_film: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
    half_uniform: wgpu::Buffer,
    half: Option<((u32, u32), wgpu::TextureView)>,
    no_half: wgpu::TextureView,
    /// Whether the film may be drawn at half size ([`HALF_FILM_FROM`]), and whether this frame does.
    may_halve: bool,
    halved: bool,
    /// The beads' squares (`glass.wgsl`'s `bead_square`): each pane's layers' [`Table`]s, the squares they list (made
    /// once a frame by `bead_squares`, [`Pass::draw_half`]), and how many groups of [`SQUARES_GROUP`] squares the
    /// largest table takes, over how many tables.
    tables: wgpu::Buffer,
    squares: wgpu::Buffer,
    squares_capacity: usize,
    bead_squares: wgpu::ComputePipeline,
    compute_read: wgpu::BindGroupLayout,
    compute_write: wgpu::BindGroupLayout,
    table_groups: (u32, u32),
    /// The film's noise over the glass (`bake_statics`), made in the first frame drawn.
    statics: wgpu::TextureView,
    bake_statics: wgpu::ComputePipeline,
    bake_write: wgpu::BindGroupLayout,
    baked: std::cell::Cell<bool>,
}

/// One layer of one pane's beads' squares, as `glass.wgsl`'s `Table`: the squares of the lattice that the pane's film
/// cells reach, row by row from the lowest, at `offset` in the squares.
#[derive(Debug, Clone, Copy, PartialEq, Default, Pod, Zeroable)]
#[repr(C)]
struct Table {
    origin: [i32; 2],
    dims: [u32; 2],
    offset: u32,
    pane: u32,
    side: u32,
    layer: u32,
    size: [f32; 2],
    landed: f32,
    pad: f32,
}

/// The panes the beads' tables hold, two layers each (a windscreen and four side windows at most).
const PANES: usize = 8;
const TABLES: usize = PANES * 2;
/// The squares a compute group makes (`@workgroup_size` of `bead_squares`), and how many the buffer first holds.
const SQUARES_GROUP: usize = 64;
/// The film's baked noise's texels to a film cell, across and up (`glass.wgsl`'s `STATIC_TEXELS`).
const STATIC_TEXELS: u32 = 16;
const FIRST_SQUARES: usize = 1 << 16;
/// The two layers' lattices (`glass.wgsl`'s `BEAD_PITCH` and `BEAD_TURN`): square size in metres, turn in radians.
const BEAD_LATTICES: [(f32, f32); 2] = [(0.0055, 0.53), (0.012, -0.97)];

fn squares_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glass bead squares"),
        size: (capacity * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    })
}

/// The squares of a layer's lattice that a pane's film cells (`size` metres a cell) reach: the lowest square's number
/// and how many across and up. A pixel looks at the 2 x 2 squares from `floor(p - 0.5)` (`bead_layer`), the margin
/// takes up the angles' rounding.
fn lattice_squares(size: [f32; 2], (pitch, turn): (f32, f32)) -> ([i32; 2], [u32; 2]) {
    let [columns, rows] = super::model::FILM_CELLS;
    let (width, height) = (columns as f32 * size[0], rows as f32 * size[1]);
    let (c, s) = (turn.cos(), turn.sin());
    let corners = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
        .map(|(x, y): (f32, f32)| [(c * x + s * y) / pitch, (c * y - s * x) / pitch]);
    let low = corners.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    let high = corners.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
    let origin = low.map(|v| (v - 0.5).floor() as i32 - 1);
    let top = high.map(|v| (v - 0.5).floor() as i32 + 2);
    (origin, [(top[0] - origin[0] + 1) as u32, (top[1] - origin[1] + 1) as u32])
}

impl Pass {
    /// The pass for colour targets of `format` and a `depth` attachment, reading the sun's cascades `sun_map`; the film
    /// is drawn at half size on tall frames if `may_halve` ([`HALF_FILM_FROM`]).
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        sun_map: &wgpu::TextureView,
        may_halve: bool,
    ) -> Pass {
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass"),
            entries: &[
                entry(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(2, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(
                    3,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    4,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
                entry(
                    5,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    6,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    7,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(8, wgpu::ShaderStages::FRAGMENT, storage(true)),
                entry(9, wgpu::ShaderStages::FRAGMENT, storage(true)),
                entry(
                    10,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass scene"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sun_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass sun"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let uniform = gpu_util::uniform_buffer(device, "glass frame", size_of::<FrameUniform>());
        let shader = gpu_util::shader(device, "glass", SHADER);
        let premultiplied = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point, depth: Option<wgpu::TextureFormat>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Drop>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &INSTANCE_ATTRIBUTES,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState { color: premultiplied, alpha: premultiplied }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: depth.map(|format| wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = PIPELINES.map(|entry_point| pipeline(entry_point, Some(depth)));
        // The film's cells at half size are drawn whole, under what hides them; `fs_film_half` lays them over the
        // frame through their quads at full size, depth tested.
        let half_film = pipeline("fs_film", None);
        let half_uniform = gpu_util::uniform_buffer(device, "glass frame, half size", size_of::<FrameUniform>());
        let tables = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass bead tables"),
            size: (TABLES * size_of::<Table>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let compute_read = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass bead squares, read"),
            entries: &[
                entry(
                    0,
                    wgpu::ShaderStages::COMPUTE,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(8, wgpu::ShaderStages::COMPUTE, storage(true)),
            ],
        });
        let compute_write = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass bead squares, write"),
            entries: &[entry(0, wgpu::ShaderStages::COMPUTE, storage(false))],
        });
        let compute_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass bead squares"),
            bind_group_layouts: &[Some(&compute_read), Some(&compute_write)],
            immediate_size: 0,
        });
        let bead_squares = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("glass bead squares"),
            layout: Some(&compute_layout),
            module: &shader,
            entry_point: Some("bead_squares"),
            compilation_options: Default::default(),
            cache: None,
        });
        let squares = squares_buffer(device, FIRST_SQUARES);
        let [columns, rows] = super::model::FILM_CELLS.map(|n| n as u32 * STATIC_TEXELS);
        let statics = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("glass film noise"),
                size: wgpu::Extent3d { width: columns, height: rows, depth_or_array_layers: 2 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
        let bake_write = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass film noise, write"),
            entries: &[entry(
                1,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba16Float,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                },
            )],
        });
        let bake_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass film noise"),
            bind_group_layouts: &[None, Some(&bake_write)],
            immediate_size: 0,
        });
        let bake_statics = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("glass film noise"),
            layout: Some(&bake_layout),
            module: &shader,
            entry_point: Some("bake_statics"),
            compilation_options: Default::default(),
            cache: None,
        });
        let no_half = half_target(device, format, (1, 1));
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass drops"),
            size: (FIRST_CAPACITY * std::mem::size_of::<Drop>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Pass {
            pipelines,
            runs: Vec::new(),
            layout,
            sampler,
            sun_map: sun_map.clone(),
            sun_sampler,
            uniform,
            instances,
            capacity: FIRST_CAPACITY,
            count: 0,
            scene: None,
            wipers: Wipers::default(),
            half_film,
            half_uniform,
            half: None,
            no_half,
            may_halve,
            halved: false,
            format,
            tables,
            squares,
            squares_capacity: FIRST_SQUARES,
            bead_squares,
            compute_read,
            compute_write,
            table_groups: (0, 0),
            statics,
            bake_statics,
            bake_write,
            baked: std::cell::Cell::new(false),
        }
    }

    /// The drops to draw.
    pub(crate) fn set(&mut self, device: &wgpu::Device, uploads: &mut Uploads, drops: &[Drop]) {
        if drops.len() > self.capacity {
            self.capacity = drops.len().next_power_of_two();
            let mut all = drops.to_vec();
            all.resize(self.capacity, Drop::zeroed());
            self.instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glass drops"),
                contents: bytemuck::cast_slice(&all),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
        } else if !drops.is_empty() {
            uploads.write(&self.instances, 0, bytemuck::cast_slice(drops));
        }
        self.count = drops.len() as u32;
        self.set_tables(device, uploads, drops);
        self.runs.clear();
        for (i, drop) in drops.iter().enumerate() {
            let (pipeline, i) = (pipeline_of(drop.kind), i as u32);
            match self.runs.last_mut() {
                Some((last, run)) if *last == pipeline => run.end = i + 1,
                _ => self.runs.push((pipeline, i..i + 1)),
            }
        }
    }

    /// The beads' tables for the panes whose film cells `drops` hold (`pane_drops`: a film cell carries its pane's cell
    /// size, landing clock and number in `normal`, and a side window's streak is negative).
    fn set_tables(&mut self, device: &wgpu::Device, uploads: &mut Uploads, drops: &[Drop]) {
        let mut tables = [Table::default(); TABLES];
        let mut seen = [false; PANES];
        let mut offset = 0;
        for drop in drops.iter().filter(|d| d.kind == 4.0) {
            let [width, height, landed, pane] = drop.normal;
            let pane = pane as usize;
            if pane >= PANES || std::mem::replace(&mut seen[pane], true) {
                continue;
            }
            for (layer, lattice) in BEAD_LATTICES.into_iter().enumerate() {
                let (origin, dims) = lattice_squares([width, height], lattice);
                tables[pane * 2 + layer] = Table {
                    origin,
                    dims,
                    offset,
                    pane: pane as u32,
                    side: u32::from(drop.seed < 0.0),
                    layer: layer as u32,
                    size: [width, height],
                    landed,
                    pad: 0.0,
                };
                offset += dims[0] * dims[1];
            }
        }
        let used = tables.iter().rposition(|t| t.dims != [0; 2]).map_or(0, |last| last + 1);
        let longest = tables.iter().map(|t| (t.dims[0] * t.dims[1]) as usize).max().unwrap_or(0);
        self.table_groups = (longest.div_ceil(SQUARES_GROUP) as u32, used as u32);
        if used > 0 {
            uploads.write(&self.tables, 0, bytemuck::cast_slice(&tables));
        }
        if offset as usize > self.squares_capacity {
            self.squares_capacity = (offset as usize).next_power_of_two();
            self.squares = squares_buffer(device, self.squares_capacity);
        }
    }

    /// The windscreen's wipers: the pane's swept areas seen from inside (kind 6), and where and when the blades last
    /// passed over the film (kind 4).
    pub fn set_wipers(&mut self, wipers: Wipers) {
        self.wipers = wipers;
    }

    /// Whether there is anything to draw.
    pub fn active(&self) -> bool {
        self.count > 0
    }

    /// The frame's camera, size and light; `wet` how wet the stage is (0 dry, 1 wet).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_projection: Mat4,
        (width, height): (u32, u32),
        a: &Atmosphere,
        outside: bool,
        wet: f32,
        cascades: Option<&crate::sun::Cascades>,
    ) {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let light = (a.ambient + a.diffuse * DIFFUSE_SHARE).min(glam::Vec3::splat(1.2));
        let uniform = FrameUniform {
            view_projection: view_projection.to_cols_array_2d(),
            screen: [w, h, 1.0 / w, 1.0 / h],
            light: light.extend(1.0).to_array(),
            night: if a.night { [NIGHT_GLOW[0], NIGHT_GLOW[1], NIGHT_GLOW[2], 1.0] } else { [0.0; 4] },
            outside: [f32::from(u8::from(outside)), wet.max(a.precipitation()).clamp(0.0, 1.0), wet, 0.0],
            eye: crate::camera::eye(view_projection).extend(1.0).to_array(),
            sun: (-a.sun_direction.normalize_or_zero()).extend(if a.night { 0.0 } else { 1.0 }).to_array(),
            sun_light: a.diffuse.extend(1.0).to_array(),
            wipers: self.wipers,
            sun_matrix: cascades.map_or(Mat4::IDENTITY, |c| c.matrices[0]).to_cols_array_2d(),
            shadow: [f32::from(u8::from(cascades.is_some())), 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.halved =
            self.may_halve && !outside && height >= HALF_FILM_FROM && self.runs.iter().any(|(p, _)| *p == FILM);
        if self.halved {
            let size = (width.div_ceil(2), height.div_ceil(2));
            if self.half.as_ref().is_none_or(|(s, _)| *s != size) {
                self.half = Some((size, half_target(device, self.format, size)));
            }
            let (w, h) = (size.0 as f32, size.1 as f32);
            let half = FrameUniform { screen: [w, h, 1.0 / w, 1.0 / h], ..uniform };
            queue.write_buffer(&self.half_uniform, 0, bytemuck::bytes_of(&half));
        }
    }

    /// The pass's own resolved frame of `size` (made or remade as needed), for a renderer without one.
    pub fn scene(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> &wgpu::TextureView {
        if self.scene.as_ref().is_none_or(|(s, _)| *s != size) {
            // Copied from when the frame has one sample (`Renderer::over_attachment`).
            let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC;
            self.scene = Some((size, gpu_util::target(device, "glass resolved frame", size, format, usage)));
        }
        &self.scene.as_ref().expect("made above").1
    }

    /// The pass's own resolved frame, after [`Pass::scene`].
    pub fn own_scene(&self) -> Option<&wgpu::TextureView> {
        self.scene.as_ref().map(|(_, view)| view)
    }

    /// The pass's bind group over `uniform`, reading the resolved frame `scene`, the mirrors' images and views
    /// `mirrors` and the film at half size `half`.
    fn group(
        &self,
        device: &wgpu::Device,
        uniform: &wgpu::Buffer,
        scene: &wgpu::TextureView,
        (images, views): (&wgpu::TextureView, &wgpu::Buffer),
        half: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        use wgpu::BindingResource::{Sampler, TextureView};
        let resources = [
            uniform.as_entire_binding(),
            TextureView(scene),
            Sampler(&self.sampler),
            TextureView(&self.sun_map),
            Sampler(&self.sun_sampler),
            TextureView(images),
            views.as_entire_binding(),
            TextureView(half),
            self.tables.as_entire_binding(),
            self.squares.as_entire_binding(),
            TextureView(&self.statics),
        ];
        gpu_util::bind_group(device, "glass", &self.layout, &resources)
    }

    /// Makes the beads' squares (`bead_squares`) from the frame's wipers, before the film's cells read them.
    fn make_squares(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        let (groups, tables) = self.table_groups;
        if groups == 0 {
            return;
        }
        if !self.baked.replace(true) {
            let write = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass film noise, write"),
                layout: &self.bake_write,
                entries: &[wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.statics),
                }],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("glass film noise"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.bake_statics);
            pass.set_bind_group(1, &write, &[]);
            let [columns, rows] = super::model::FILM_CELLS.map(|n| (n as u32 * STATIC_TEXELS).div_ceil(8));
            pass.dispatch_workgroups(columns, rows, 2);
        }
        fn at(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
        }
        let read = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass bead squares, read"),
            layout: &self.compute_read,
            entries: &[at(0, &self.uniform), at(8, &self.tables)],
        });
        let write = gpu_util::bind_group(
            device,
            "glass bead squares, write",
            &self.compute_write,
            &[self.squares.as_entire_binding()],
        );
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("glass bead squares"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.bead_squares);
        pass.set_bind_group(0, &read, &[]);
        pass.set_bind_group(1, &write, &[]);
        pass.dispatch_workgroups(groups, tables, 1);
    }

    /// Draws the film's cells at half size ([`HALF_FILM_FROM`]) when the frame does, before [`Pass::draw`] lays them
    /// over it; reading the resolved frame `scene` and the mirrors' `mirrors`.
    pub fn draw_half(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        mirrors: (&wgpu::TextureView, &wgpu::Buffer),
    ) {
        self.make_squares(device, encoder);
        let Some((_, target)) = self.half.as_ref().filter(|_| self.halved && self.count > 0) else {
            return;
        };
        let load = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let mut pass = gpu_util::colour_pass(encoder, "glass film, half size", target, load);
        pass.set_pipeline(&self.half_film);
        pass.set_bind_group(0, &self.group(device, &self.half_uniform, scene, mirrors, &self.no_half), &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        for (_, run) in self.runs.iter().filter(|(p, _)| *p == FILM) {
            pass.draw(0..4, run.clone());
        }
    }

    /// Draws the drops into `pass`, reading the resolved frame `scene` and the mirrors' images and views `mirrors`
    /// (the windscreen's reflection, `renderer::mirrors`).
    pub fn draw(
        &self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        scene: &wgpu::TextureView,
        mirrors: (&wgpu::TextureView, &wgpu::Buffer),
    ) {
        if self.count == 0 {
            return;
        }
        let half = self.half.as_ref().filter(|_| self.halved);
        let group = self.group(device, &self.uniform, scene, mirrors, half.map_or(&self.no_half, |(_, v)| v));
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        for (pipeline, run) in &self.runs {
            let pipeline = if *pipeline == FILM && half.is_some() { FILM_HALF } else { *pipeline };
            pass.set_pipeline(&self.pipelines[pipeline]);
            pass.draw(0..4, run.clone());
        }
    }
}

#[cfg(test)]
crate::shader_check::wgsl_layout!(
    the_glass_frame_keeps_the_shaders_layout,
    SHADER,
    "Frame",
    FrameUniform { view_projection, screen, light, night, outside, eye, sun, sun_light, wipers.wipers, wipers.strokes, wipers.wiping, wipers.film, sun_matrix, shadow }
);

#[cfg(test)]
crate::shader_check::wgsl_layout!(
    the_glass_bead_table_keeps_the_shaders_layout,
    SHADER,
    "Table",
    Table { origin, dims, offset, pane, side, layer, size, landed, pad }
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers of `const name: vec2<f32> = vec2<f32>(x, y);` in the shader.
    fn pair(name: &str) -> [f32; 2] {
        let line = SHADER.lines().find(|l| l.starts_with(&format!("const {name}:"))).expect("the constant");
        let inner = line.split("vec2<f32>(").nth(1).and_then(|rest| rest.split(')').next()).expect("its values");
        let mut numbers = inner.split(',').map(|n| n.trim().parse::<f32>().expect("a number"));
        [numbers.next().unwrap(), numbers.next().unwrap()]
    }

    #[test]
    fn the_beads_lattices_are_the_shaders() {
        let (pitch, turn) = (pair("BEAD_PITCH"), pair("BEAD_TURN"));
        assert_eq!(BEAD_LATTICES, [(pitch[0], turn[0]), (pitch[1], turn[1])]);
    }

    #[test]
    fn a_lattice_covers_the_pane_and_the_squares_a_pixel_looks_at() {
        let size = [0.026, 0.021];
        let (width, height) = (48.0 * size[0], 24.0 * size[1]);
        for (pitch, turn) in BEAD_LATTICES {
            let (origin, dims) = lattice_squares(size, (pitch, turn));
            let (c, s) = (turn.cos(), turn.sin());
            for (x, y) in [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height), (width * 0.5, height * 0.5)] {
                let p = [(c * x + s * y) / pitch, (c * y - s * x) / pitch];
                for k in 0..4 {
                    let square = [(p[0] - 0.5).floor() as i32 + (k & 1), (p[1] - 0.5).floor() as i32 + (k >> 1)];
                    assert!((0..2).all(|a| square[a] >= origin[a] && square[a] < origin[a] + dims[a] as i32));
                }
            }
        }
    }
}
