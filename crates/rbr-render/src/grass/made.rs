//! The grass made on the GPU every frame (ours): the CPU bakes each tile's ground once into a field of cells, the same
//! for any distance ([`Field::bake`]), and a compute pass per view walks each tile's candidate clumps, takes those the
//! density grows at their distance and the field holds, and writes their cards (`grass_made.wgsl`), which one
//! indirect draw per view expands into the stage shader's grass cards (`vs_grass` in `grass_cards.wgsl`). The tests
//! (the ground's grassiness and support, [`Field::grassy`]), hues and light are the CPU's, baked into the cells; the
//! sizes, pictures and kinds are the shader's.

use super::{
    density, patches, seen, smoothstep, tile_of, Field, Level, Light, BAND, COVER, DENSITY, FAR_SHARE, FULL, TILE,
};
use crate::uploads::Uploads;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3, Vec3Swizzles, Vec4};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;

/// The cells a tile's side holds, and a cell's side, metres.
const CELLS: usize = 64;
const CELL: f32 = TILE / CELLS as f32;
/// The cells baked past each side of a tile, for the support of its edge cells.
const BORDER: usize = 5;
/// The light's cells a tile's side holds.
const LIGHTS: usize = 32;
/// The words of a tile in the pool: its cells', then its light's.
const TILE_WORDS: usize = (CELLS * CELLS + LIGHTS * LIGHTS) * 4;
/// The most clumps a square metre ([`super::Fx::density`]'s most): a tile's candidates are as many over its area.
const MOST_DENSITY: f32 = DENSITY * 1.2;
const CANDIDATES: u32 = (MOST_DENSITY * TILE * TILE) as u32;
/// The most a cell's support records, metres: past a converted stage's widest cards' footprint.
const MOST_SUPPORT: f32 = (BORDER as f32 + 0.5) * CELL;
/// How many tiles the pool holds at each level (80 KB each: Dense 126 MB, under the 128 MiB a storage binding takes),
/// and the most cards a view draws. Joux Verte's grass everywhere fills the Dense pool on its replay, 760 thousand cards
/// in its chase view: the farthest tiles in reach, thin and fading, go without.
const POOL: [usize; 2] = [768, 1536];
const MAIN_CARDS: u32 = 1 << 20;
const MIRROR_CARDS: u32 = 1 << 18;
/// The views that draw the grass: the main one, the centre mirror and the door mirrors (`View::Mirror`).
pub(crate) const VIEWS: usize = 3;
/// The tiles within this of the eye are baked on the frame itself, so the near field is there on the first frame.
const SYNC_DISTANCE: f32 = 24.0;
/// How many finished tiles a frame uploads.
const UPLOAD_TILES: usize = 64;
/// A tile farther than a waiting one by this gives up its place in the pool.
const LEAD: f32 = 16.0;
/// The most worker threads baking tiles: a quarter of the machine's threads up to this.
const WORKERS: usize = 4;
/// The compute pass's workgroup size (`grass_made.wgsl`).
const GROUP: u32 = 256;

/// The cells a side of the blocks the distance to the driveline is taken over ([`Field::bake`]).
const BLOCK: usize = 4;

/// How far the ground stays grassy ([`COVER`]) around each of `grassy`'s cells (`side` a row), metres, up to
/// [`MOST_SUPPORT`]: the distance to the nearest cell short of it less half a cell, by two chamfer passes (the
/// diagonal steps the square root of 2), 0 for those short of it.
fn support(grassy: &[f32], side: usize) -> Vec<f32> {
    let far = MOST_SUPPORT / CELL + 1.0;
    let mut d: Vec<f32> = grassy.iter().map(|&g| if g < COVER { 0.0 } else { far }).collect();
    let diagonal = std::f32::consts::SQRT_2;
    let steps = [(-1, 0, 1.0), (0, -1, 1.0), (-1, -1, diagonal), (1, -1, diagonal)];
    let at = |i: usize, j: usize, (di, dj, _): (isize, isize, f32)| {
        let (i, j) = (i as isize + di, j as isize + dj);
        (i >= 0 && j >= 0 && (i as usize) < side && (j as usize) < side).then(|| j as usize * side + i as usize)
    };
    for j in 0..side {
        for i in 0..side {
            for step in steps {
                if let Some(n) = at(i, j, step) {
                    d[j * side + i] = d[j * side + i].min(d[n] + step.2);
                }
            }
        }
    }
    for j in (0..side).rev() {
        for i in (0..side).rev() {
            for (di, dj, w) in steps {
                if let Some(n) = at(i, j, (-di, -dj, w)) {
                    d[j * side + i] = d[j * side + i].min(d[n] + w);
                }
            }
        }
    }
    d.iter().map(|&d| ((d - 0.5) * CELL).clamp(0.0, MOST_SUPPORT)).collect()
}

/// A tile's field: its cells and light, packed ([`Field::bake`]), and its ground's lowest and highest points.
struct Baked {
    words: Vec<u32>,
    low: f32,
    high: f32,
}

/// A tile baked: its field, or none where it grows no grass, and the milliseconds it took.
type Done = ((i32, i32), Option<Baked>, f32);

/// Bakes `tile` ([`Field::bake`]), timed.
fn baked(field: &Field, tile: (i32, i32), light: &[[Light; 3]]) -> Done {
    let start = std::time::Instant::now();
    let baked = field.bake(tile, light);
    (tile, baked, start.elapsed().as_secs_f32() * 1000.0)
}

/// The ground on top at a cell's centre: its triangle, the centre's weights on it and its height.
#[derive(Clone, Copy)]
struct Top {
    triangle: u32,
    w: [f32; 3],
    y: f32,
}

impl Field {
    /// The field of `tile`, lit by `light` (each ground triangle's corners'; none: open sky): `None` where no cell
    /// grows grass. Each of its [`CELLS`] x [`CELLS`] cells takes the highest ground at its centre and holds four words:
    /// its height; its normal's x and z (snorm8), how likely a candidate clump grows there (its grassiness times the
    /// falloff from the driveline and its profile's density against [`MOST_DENSITY`], unorm8, 0 below [`COVER`]) and its
    /// support (how far the ground stays as grassy around it, against [`MOST_SUPPORT`], unorm8); its grass's hue (rgb8)
    /// and the patch's tallness (unorm8 of 0.55 to 1.5); its brightness ([`Field::look`], unorm16 of 0 to 4), the
    /// patch's dryness (unorm8) and its profile. Then its [`LIGHTS`] x [`LIGHTS`] light cells: [`Light::packed`].
    fn bake(&self, tile: (i32, i32), light: &[[Light; 3]]) -> Option<Baked> {
        let side = CELLS + 2 * BORDER;
        let origin = Vec2::new(tile.0 as f32, tile.1 as f32) * TILE - BORDER as f32 * CELL;
        let centre = |i: usize, j: usize| origin + (Vec2::new(i as f32, j as f32) + 0.5) * CELL;
        // The triangles over the tile and its border, from its own and its neighbours' lists.
        let mut triangles: Vec<u32> = (-1..=1)
            .flat_map(|x| (-1..=1).map(move |z| (tile.0 + x, tile.1 + z)))
            .filter_map(|t| self.tiles.get(&t))
            .flatten()
            .copied()
            .collect();
        triangles.sort_unstable();
        triangles.dedup();
        let mut top: Vec<Option<Top>> = vec![None; side * side];
        let to_cell = |x: f32| (x / CELL - 0.5).clamp(-1.0, side as f32);
        for &t in &triangles {
            let g = &self.triangles[t as usize];
            let (low, high) =
                (g.p[0].min(g.p[1]).min(g.p[2]).xz() - origin, g.p[0].max(g.p[1]).max(g.p[2]).xz() - origin);
            let (i0, i1) = (to_cell(low.x).ceil().max(0.0) as usize, to_cell(high.x).floor());
            let (j0, j1) = (to_cell(low.y).ceil().max(0.0) as usize, to_cell(high.y).floor());
            if i1 < 0.0 || j1 < 0.0 {
                continue;
            }
            let (i1, j1) = ((i1 as usize).min(side - 1), (j1 as usize).min(side - 1));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let c = centre(i, j);
                    let w = g.weights(Vec3::new(c.x, 0.0, c.y));
                    if w.iter().any(|&v| v < -1e-4) {
                        continue;
                    }
                    let y = g.p[0].y * w[0] + g.p[1].y * w[1] + g.p[2].y * w[2];
                    let cell = &mut top[j * side + i];
                    if cell.is_none_or(|c| y > c.y) {
                        *cell = Some(Top { triangle: t, w, y });
                    }
                }
            }
        }
        let ground = |top: &Top| &self.triangles[top.triangle as usize];
        let grassy: Vec<f32> = top.iter().map(|t| t.map_or(0.0, |t| self.grassy(ground(&t), t.w))).collect();
        let support = support(&grassy, side);
        // The distance across to the driveline, by blocks of [`BLOCK`] cells: the falloff it sets is metres wide.
        let blocks = CELLS / BLOCK;
        let across: Vec<f32> = (0..blocks * blocks)
            .map(|k| {
                let block = Vec2::new((k % blocks) as f32, (k / blocks) as f32) + 0.5;
                self.line.distance(origin + BORDER as f32 * CELL + block * BLOCK as f32 * CELL)
            })
            .collect();
        let mut words = vec![0u32; TILE_WORDS];
        let (mut low, mut high, mut any) = (f32::MAX, f32::MIN, false);
        let unorm8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
        let snorm8 = |x: f32| ((x.clamp(-1.0, 1.0) * 127.0).round() as i32 as u32) & 0xff;
        for j in 0..CELLS {
            for i in 0..CELLS {
                let (bi, bj) = (i + BORDER, j + BORDER);
                let Some(t) = top[bj * side + bi] else { continue };
                let g = ground(&t);
                let c = centre(bi, bj);
                (low, high) = (low.min(t.y), high.max(t.y));
                let vegetation = grassy[bj * side + bi];
                let support = support[bj * side + bi];
                let across = across[(j / BLOCK) * blocks + i / BLOCK];
                let near = 1.0 - (1.0 - FAR_SHARE) * smoothstep([FULL, BAND], across);
                let fade = 1.0 - smoothstep([BAND - 6.0, BAND], across);
                let share = self.fx(g).map_or(DENSITY, super::Fx::density) / MOST_DENSITY;
                let likely = if vegetation >= COVER { vegetation * near * fade * share } else { 0.0 };
                any |= likely > 0.0 && support > 0.0;
                let (tall, dry) = patches(c);
                let (hue, bright) = self.look(g, t.w, dry);
                let at = (j * CELLS + i) * 4;
                words[at] = t.y.to_bits();
                words[at + 1] = snorm8(g.normal.x)
                    | snorm8(g.normal.z) << 8
                    | unorm8(likely) << 16
                    | unorm8(support / MOST_SUPPORT) << 24;
                words[at + 2] = u32::from(hue[0])
                    | u32::from(hue[1]) << 8
                    | u32::from(hue[2]) << 16
                    | unorm8((tall - 0.55) / 0.95) << 24;
                words[at + 3] = ((bright / 4.0).clamp(0.0, 1.0) * 65535.0).round() as u32
                    | unorm8(dry) << 16
                    | u32::from(g.profile) << 24;
            }
        }
        if !any {
            return None;
        }
        // The light at each light cell's centre, of the ground on top just inside it.
        let lights = CELLS / LIGHTS;
        for j in 0..LIGHTS {
            for i in 0..LIGHTS {
                let (bi, bj) = (i * lights + lights / 2 + BORDER, j * lights + lights / 2 + BORDER);
                let Some(t) = top[bj * side + bi] else { continue };
                let c = origin + (Vec2::new(i as f32, j as f32) + 0.5) * CELL * lights as f32 + BORDER as f32 * CELL;
                let w = ground(&t).weights(Vec3::new(c.x, 0.0, c.y));
                let corners = light.get(t.triangle as usize).copied().unwrap_or_default();
                let (occlusion, sky, bounce) = Light::blend(&corners, w).packed();
                let at = (CELLS * CELLS + j * LIGHTS + i) * 4;
                words[at..at + 4].copy_from_slice(&[occlusion.to_bits(), sky, bounce[0], bounce[1]]);
            }
        }
        Some(Baked { words, low, high })
    }

    /// The converted stage's atlas pieces for the GPU (`grass_made.wgsl`): each profile's cell size, first piece, count
    /// and whether it has an atlas, then the pieces in the order [`super::Fx::piece`] picks them, each with the share
    /// of the picks up to and with it.
    fn pieces(&self) -> (Vec<Profile>, Vec<Piece>) {
        let (mut profiles, mut pieces) = (Vec::new(), Vec::new());
        for fx in &self.fx {
            let first = pieces.len() as u32;
            if let Some((atlas, size)) = &fx.atlas {
                let groups: Vec<_> = atlas.groups.iter().filter(|g| !g.pieces.is_empty()).collect();
                let total = atlas.base + groups.iter().map(|g| g.chance * super::FX_PIECE_SHARE).sum::<f32>();
                let mut bound = 0.0;
                let mut push = |share: f32, rect: [f32; 4], cells: [f32; 2], piece: u32| {
                    bound += share / total.max(1e-6);
                    pieces.push(Piece { rect, cells, bound, piece });
                };
                let columns = atlas.grid[0];
                for column in 1..=columns {
                    let rect = super::piece_rect(atlas.grid, [column, 0], [1, 1], *size);
                    push(atlas.base / columns as f32, rect, [1.0; 2], 0);
                }
                // A stage with a cover map (DiRT Rally's tufts) has no base grass for its pieces to come in beyond:
                // they are its grass, drawn from the eye in the atlas's own colours (kind 2: `grass_cards.wgsl`).
                let piece = if fx.cover.is_some() { 2 } else { 1 };
                for group in groups {
                    let sum = group.pieces.iter().map(|p| p.chance).sum::<f32>().max(1e-6);
                    for p in &group.pieces {
                        let rect = super::piece_rect(atlas.grid, p.cell, p.size, *size);
                        push(group.chance * super::FX_PIECE_SHARE * p.chance / sum, rect, p.mult, piece);
                    }
                }
                if let Some(last) = pieces.last_mut() {
                    last.bound = 1.0;
                }
            }
            let count = pieces.len() as u32 - first;
            profiles.push(Profile { cell: fx.cell(), first, count, atlas: u32::from(fx.atlas.is_some()) });
        }
        if profiles.is_empty() {
            profiles.push(Profile::zeroed());
        }
        if pieces.is_empty() {
            pieces.push(Piece::zeroed());
        }
        (profiles, pieces)
    }
}

/// A converted stage's grass profile on the GPU ([`Field::pieces`]).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Profile {
    cell: f32,
    first: u32,
    count: u32,
    atlas: u32,
}

/// An atlas piece on the GPU: its rectangle (`u0, u1, v0, v1`), its size in cells, the share of the picks up to and
/// with it, and whether it is a group's piece.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Piece {
    rect: [f32; 4],
    cells: [f32; 2],
    bound: f32,
    piece: u32,
}

/// A view's culling (`Cull` in `grass_made.wgsl`): the view's planes (normalised, a degenerate one always passed), its
/// eye, the level's profile ([`Level::profile`]), and the tiles listed, the most cards, whether the stage is a converted
/// one and the candidates a tile.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Cull {
    planes: [[f32; 4]; 6],
    eye: [f32; 4],
    profile: [f32; 4],
    info: [u32; 4],
}

/// A tile a view walks (`Tile` in `grass_made.wgsl`): its place in the pool, its coordinates and how many of its
/// candidates.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Walk {
    slot: u32,
    x: i32,
    z: i32,
    count: u32,
}

/// A view's buffers: its culling, its tiles, its cards, and its count with the draw's arguments after it.
struct ViewBuffers {
    cull: wgpu::Buffer,
    tiles: wgpu::Buffer,
    counter: wgpu::Buffer,
    capacity: u32,
    make: wgpu::BindGroup,
    draw: wgpu::BindGroup,
}

/// The byte offset of the draw's arguments in a view's counter buffer.
const ARGS_OFFSET: u64 = 16;

/// The grass made on the GPU about the eye ([`Made::update`], [`Made::make`], [`Made::draw`]).
pub(crate) struct Made {
    field: Arc<Field>,
    light: Arc<Vec<[Light; 3]>>,
    /// The renderer's mesh holding the grass's draw (its pipelines and texture, [`is_made`]).
    pub(crate) mesh: usize,
    jobs: Option<SyncSender<(i32, i32)>>,
    done: Receiver<Done>,
    workers: Vec<std::thread::JoinHandle<()>>,
    in_flight: HashSet<(i32, i32)>,
    ready: Vec<((i32, i32), Baked)>,
    /// The tiles in the pool: their place and their ground's height range.
    resident: HashMap<(i32, i32), (usize, f32, f32)>,
    slots: Vec<Option<(i32, i32)>>,
    /// The tiles baked with no grass.
    bare: HashSet<(i32, i32)>,
    /// The tiles in reach this frame, resident.
    drawn: Vec<(i32, i32)>,
    /// The tiles in reach not yet resident.
    missing: usize,
    /// The tiles baked and the milliseconds they took ([`Made::stats`]).
    baked: (u32, f32),
    /// The frame waits for every tile in reach (a screenshot).
    pub(crate) waits: bool,
    pool: wgpu::Buffer,
    index: wgpu::Buffer,
    views: Vec<ViewBuffers>,
    generate: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
    /// `RERBR_GRASS_STATS`: the main view's cards read back every [`STATS_EVERY`] frames and printed, with the frames
    /// made.
    stats: Option<(wgpu::Device, wgpu::Buffer, std::cell::Cell<u32>)>,
}

/// How often the stats are printed, in frames.
const STATS_EVERY: u32 = 30;

impl Drop for Made {
    fn drop(&mut self) {
        self.jobs.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// The bind group layout of a view's cards, as the grass's draw reads them (`@group(3)` in `grass_cards.wgsl`).
pub(crate) fn cards_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("grass cards"),
        entries: &[storage_entry(0, wgpu::ShaderStages::VERTEX, true)],
    })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
}

fn storage_entry(binding: u32, visibility: wgpu::ShaderStages, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Whether `mesh` is the grass's draw for the GPU ([`super::add`]): a grass draw with no vertices.
pub(crate) fn is_made(mesh: &crate::stage::Mesh) -> bool {
    mesh.vertices.is_empty() && !mesh.draws.is_empty() && mesh.draws.iter().all(|d| d.lod == crate::stage::Lod::Close)
}

/// The compute shader with the constants it shares with [`super`].
fn source() -> String {
    let constants = format!(
        "const TILE: f32 = {TILE:?};\nconst CELLS: u32 = {CELLS}u;\nconst LIGHTS: u32 = {LIGHTS}u;\n\
         const TILE_WORDS: u32 = {TILE_WORDS}u;\nconst CANDIDATES: u32 = {CANDIDATES}u;\n\
         const MOST_SUPPORT: f32 = {MOST_SUPPORT:?};\nconst GROW_MARGIN: f32 = {:?};\nconst FADE: f32 = {:?};\n\
         const WIDEST: f32 = {:?};\nconst HEIGHT: vec2<f32> = vec2<f32>({:?}, {:?});\n\
         const WIDTH: vec2<f32> = vec2<f32>({:?}, {:?});\nconst MAX_HEIGHT: vec2<f32> = vec2<f32>({:?}, {:?});\n\
         const MAX_WIDTH: f32 = {:?};\nconst CELL_VARY: vec2<f32> = vec2<f32>({:?}, {:?});\n\
         const FX_WIDEN: f32 = {:?};\nconst PATCH_RADIUS: f32 = {:?};\nconst SWAY: f32 = {:?};\n\
         const LEAN: f32 = {:?};\nconst BRIGHT: vec2<f32> = vec2<f32>({:?}, {:?});\n\
         const FX_BRIGHT: vec2<f32> = vec2<f32>({:?}, {:?});\nconst GROUP: u32 = {GROUP}u;\n\
         const COLUMNS: u32 = {}u;\nconst ROWS: u32 = {}u;\nconst PICTURE: vec2<f32> = vec2<f32>({:?}, {:?});\n",
        super::GROW_MARGIN,
        super::FADE,
        super::WIDEST,
        super::HEIGHT[0],
        super::HEIGHT[1],
        super::WIDTH[0],
        super::WIDTH[1],
        super::MAX_HEIGHT[0],
        super::MAX_HEIGHT[1],
        super::MAX_WIDTH,
        super::CELL_VARY[0],
        super::CELL_VARY[1],
        super::FX_WIDEN,
        super::PATCH_RADIUS,
        super::SWAY,
        super::LEAN,
        super::BRIGHT[0],
        super::BRIGHT[1],
        super::FX_BRIGHT[0],
        super::FX_BRIGHT[1],
        super::COLUMNS,
        super::ROWS,
        super::PICTURE.0 as f32,
        super::PICTURE.1 as f32,
    );
    constants + include_str!("../grass_card.wgsl") + include_str!("../grass_made.wgsl")
}

/// The worker threads baking tiles: the tiles in, the tiles baked out, and their handles.
type Workers = (SyncSender<(i32, i32)>, Receiver<Done>, Vec<std::thread::JoinHandle<()>>);

/// The worker threads baking tiles ([`Field::bake`]).
fn spawn_workers(field: &Arc<Field>, light: &Arc<Vec<[Light; 3]>>) -> Workers {
    let (jobs, receive) = std::sync::mpsc::sync_channel::<(i32, i32)>(64);
    let receive = Arc::new(std::sync::Mutex::new(receive));
    let (send, done) = std::sync::mpsc::channel();
    let count = std::thread::available_parallelism().map_or(1, |n| (n.get() / 4).clamp(1, WORKERS));
    let workers = (0..count)
        .filter_map(|i| {
            let (receive, send, field, light) = (receive.clone(), send.clone(), field.clone(), light.clone());
            let next = move || receive.lock().ok()?.recv().ok();
            std::thread::Builder::new()
                .name(format!("grass {i}"))
                .spawn(move || {
                    while let Some(tile) = next() {
                        if send.send(baked(&field, tile, &light)).is_err() {
                            break;
                        }
                    }
                })
                .ok()
        })
        .collect();
    (jobs, done, workers)
}

impl Made {
    /// The grass of `field`, lit by `light` (each ground triangle's corners'), drawn with mesh `mesh`'s draw, its views'
    /// cards bound by `drawn` ([`cards_layout`]).
    pub(crate) fn new(
        device: &wgpu::Device,
        field: Arc<Field>,
        light: Vec<[Light; 3]>,
        mesh: usize,
        drawn: &wgpu::BindGroupLayout,
    ) -> Made {
        let light = Arc::new(light);
        let (jobs, done, workers) = spawn_workers(&field, &light);
        let tiles = POOL[usize::from(field.level == Level::Dense)];
        let pool = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grass pool"),
            size: (tiles * TILE_WORDS * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (profiles, pieces) = field.pieces();
        let init = |label, contents: &[u8]| {
            wgpu::util::DeviceExt::create_buffer_init(
                device,
                &wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage: wgpu::BufferUsages::STORAGE },
            )
        };
        let profiles = init("grass profiles", bytemuck::cast_slice(&profiles));
        let pieces = init("grass pieces", bytemuck::cast_slice(&pieces));
        let most = MAIN_CARDS;
        let indices: Vec<u32> = (0..most).flat_map(|c| [0, 1, 2, 0, 2, 3].map(|k| c * 4 + k)).collect();
        let index = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("grass indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            },
        );
        let compute = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grass made"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: compute,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_entry(1, compute, true),
                storage_entry(2, compute, true),
                storage_entry(3, compute, true),
                storage_entry(4, compute, true),
                storage_entry(5, compute, false),
                storage_entry(6, compute, false),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grass made"),
            source: wgpu::ShaderSource::Wgsl(source().into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grass made"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("grass made"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (generate, finish) = (pipeline("cs_generate"), pipeline("cs_finish"));
        let views = (0..VIEWS)
            .map(|view| {
                let capacity = if view == 0 { MAIN_CARDS } else { MIRROR_CARDS };
                let buffer = |label, size: u64, usage| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(label),
                        size,
                        usage,
                        mapped_at_creation: false,
                    })
                };
                let copy = wgpu::BufferUsages::COPY_DST;
                let cull = buffer("grass cull", size_of::<Cull>() as u64, wgpu::BufferUsages::UNIFORM | copy);
                let tiles =
                    buffer("grass tiles", (tiles * size_of::<Walk>()) as u64, wgpu::BufferUsages::STORAGE | copy);
                let cards = buffer("grass cards", u64::from(capacity) * 64, wgpu::BufferUsages::STORAGE);
                let counter = buffer(
                    "grass counter",
                    64,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT | copy | wgpu::BufferUsages::COPY_SRC,
                );
                let make = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("grass made"),
                    layout: &layout,
                    entries: &[
                        entry(0, &cull),
                        entry(1, &tiles),
                        entry(2, &pool),
                        entry(3, &profiles),
                        entry(4, &pieces),
                        entry(5, &cards),
                        entry(6, &counter),
                    ],
                });
                let draw = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("grass cards"),
                    layout: drawn,
                    entries: &[entry(0, &cards)],
                });
                ViewBuffers { cull, tiles, counter, capacity, make, draw }
            })
            .collect();
        Made {
            field,
            light,
            mesh,
            jobs: Some(jobs),
            done,
            workers,
            in_flight: HashSet::new(),
            ready: Vec::new(),
            resident: HashMap::new(),
            slots: vec![None; tiles],
            bare: HashSet::new(),
            drawn: Vec::new(),
            missing: 0,
            baked: (0, 0.0),
            waits: false,
            pool,
            index,
            views,
            generate,
            finish,
            stats: std::env::var_os("RERBR_GRASS_STATS").map(|_| {
                let usage = wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST;
                let read = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("grass stats"),
                    size: 4,
                    usage,
                    mapped_at_creation: false,
                });
                (device.clone(), read, std::cell::Cell::new(0))
            }),
        }
    }

    /// With [`Made::stats`], reads the main view's cards back a frame after copying them, and prints them with its
    /// tiles and candidates walked.
    fn stats(&self, encoder: &mut wgpu::CommandEncoder, view: usize, walks: &[Walk]) {
        let Some((device, read, frame)) = self.stats.as_ref().filter(|_| view == 0) else { return };
        let n = frame.get();
        frame.set(n + 1);
        if n % STATS_EVERY == 1 {
            read.slice(..).map_async(wgpu::MapMode::Read, |_| ());
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            let cards = read.slice(..).get_mapped_range().map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            read.unmap();
            let candidates: u32 = walks.iter().map(|w| w.count).sum();
            let resident = self.resident.len();
            eprintln!(
                "grass made: {} cards of {}, {} tiles walked, {candidates} candidates, {resident} tiles resident, {} missing, \
                 {} baked at {:.2} ms",
                cards.unwrap_or(0),
                self.views[0].capacity,
                walks.len(),
                self.missing,
                self.baked.0,
                self.baked.1 / self.baked.0.max(1) as f32,
            );
        } else if n % STATS_EVERY == 0 {
            encoder.copy_buffer_to_buffer(&self.views[0].counter, 0, read, 0, 4);
        }
    }

    /// Whether a frame that waits for its tiles ([`Made::waits`]) still misses some.
    pub(crate) fn waiting(&self) -> bool {
        self.waits && self.missing > 0
    }

    /// Bakes and uploads the tiles in reach of the eye at `eye` looking along `forward` (distances behind it count
    /// [`super::BEHIND`] times along the view, [`seen`]), nearest first, as many as the pool holds: those within
    /// [`SYNC_DISTANCE`] on the frame, the rest on the workers. A finished tile takes a free place in the pool, or the
    /// farthest tile's more than a [`LEAD`] farther; a frame uploads up to [`UPLOAD_TILES`].
    pub(crate) fn update(&mut self, eye: Vec3, forward: Vec3, uploads: &mut Uploads) {
        if self.workers.iter().any(std::thread::JoinHandle::is_finished) {
            self.in_flight.clear();
            let (jobs, done, workers) = spawn_workers(&self.field, &self.light);
            (self.jobs, self.done, self.workers) = (Some(jobs), done, workers);
        }
        let forward = forward.xz().normalize_or_zero();
        let centre = |(x, z): (i32, i32)| Vec2::new((x as f32 + 0.5) * TILE, (z as f32 + 0.5) * TILE);
        let away = |tile: (i32, i32)| seen(centre(tile) - eye.xz(), forward);
        let reach = self.field.level.reach() + TILE;
        let (cx, cz) = tile_of(eye);
        let r = (reach / TILE).ceil() as i32;
        let mut wanted: Vec<((i32, i32), f32)> = (cx - r..=cx + r)
            .flat_map(|x| (cz - r..=cz + r).map(move |z| (x, z)))
            .filter(|tile| self.field.tiles.contains_key(tile) && !self.bare.contains(tile))
            .map(|tile| (tile, away(tile)))
            .filter(|&(_, d)| d < reach)
            .collect();
        wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
        wanted.truncate(self.slots.len());
        let order: HashMap<(i32, i32), f32> = wanted.iter().copied().collect();
        for &(tile, d) in &wanted {
            if self.resident.contains_key(&tile)
                || self.in_flight.contains(&tile)
                || self.ready.iter().any(|(t, _)| *t == tile)
            {
                continue;
            }
            if d < SYNC_DISTANCE {
                self.take(baked(&self.field, tile, &self.light));
            } else if self.jobs.as_ref().is_some_and(|jobs| jobs.try_send(tile).is_ok()) {
                self.in_flight.insert(tile);
            } else {
                break;
            }
        }
        while let Ok(done) = self.done.try_recv() {
            self.in_flight.remove(&done.0);
            self.take(done);
        }
        let distance = |tile: &(i32, i32)| order.get(tile).copied().unwrap_or(f32::MAX);
        self.ready.retain(|(tile, _)| order.contains_key(tile));
        self.ready.sort_by(|a, b| distance(&a.0).total_cmp(&distance(&b.0)));
        let mut uploaded = 0;
        for (tile, baked) in std::mem::take(&mut self.ready) {
            let free = self.slots.iter().position(Option::is_none);
            let near = distance(&tile) + LEAD;
            let farthest = || {
                let far = self.slots.iter().enumerate().filter_map(|(i, s)| Some((i, distance(&(*s)?))));
                far.filter(|&(_, d)| d > near).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
            };
            match free.or_else(farthest) {
                Some(slot) if uploaded < UPLOAD_TILES => {
                    if let Some(old) = self.slots[slot].take() {
                        self.resident.remove(&old);
                    }
                    let offset = (slot * TILE_WORDS * 4) as u64;
                    uploads.write(&self.pool, offset, bytemuck::cast_slice(&baked.words));
                    self.slots[slot] = Some(tile);
                    self.resident.insert(tile, (slot, baked.low, baked.high));
                    uploaded += 1;
                }
                _ => self.ready.push((tile, baked)),
            }
        }
        self.drawn = wanted.iter().map(|&(tile, _)| tile).filter(|tile| self.resident.contains_key(tile)).collect();
        self.missing = wanted.len() - self.drawn.len();
    }

    /// Takes a tile baked: ready to upload, or bare.
    fn take(&mut self, (tile, baked, took): Done) {
        self.baked = (self.baked.0 + 1, self.baked.1 + took);
        match baked {
            Some(baked) => self.ready.push((tile, baked)),
            None => _ = self.bare.insert(tile),
        }
    }

    /// Records the compute pass making view `view`'s cards (0 the main view, 1 and 2 the mirrors, [`VIEWS`]) for
    /// `view_projection` into `encoder`: each resident tile in reach and in view, with as many of its candidates as the
    /// density grows at its nearest point ([`density`]).
    pub(crate) fn make(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        view: usize,
        view_projection: Mat4,
    ) {
        let Some(buffers) = self.views.get(view) else { return };
        let eye = crate::camera::eye(view_projection);
        let level = self.field.level;
        let frustum = crate::camera::Frustum::from_view_projection(view_projection);
        // The cards widen and sway: a tile's box takes the widest and tallest.
        let pad = Vec3::new(1.0, 2.0, 1.0);
        let walks: Vec<Walk> = self
            .drawn
            .iter()
            .filter_map(|&tile| {
                let &(slot, low, high) = self.resident.get(&tile)?;
                let (x, z) = (tile.0 as f32 * TILE, tile.1 as f32 * TILE);
                let (min, max) = (Vec3::new(x, low, z) - pad, Vec3::new(x + TILE, high, z + TILE) + pad);
                let nearest = (eye.clamp(min, max) - eye).length();
                let share = (super::GROW_MARGIN * density(level, nearest)).min(1.0);
                let count = (share * CANDIDATES as f32).ceil() as u32;
                let shown = frustum.intersects_box((min + max) * 0.5, (max - min) * 0.5);
                (count > 0 && shown).then_some(Walk { slot: slot as u32, x: tile.0, z: tile.1, count })
            })
            .collect();
        let rows = [0, 1, 2, 3].map(|i| view_projection.row(i));
        let planes =
            [rows[3] + rows[0], rows[3] - rows[0], rows[3] + rows[1], rows[3] - rows[1], rows[2], rows[3] - rows[2]]
                .map(|p: Vec4| {
                    let length = p.truncate().length();
                    if length > 1e-6 {
                        (p / length).to_array()
                    } else {
                        [0.0, 0.0, 0.0, 1.0]
                    }
                });
        let cull = Cull {
            planes,
            eye: eye.extend(0.0).to_array(),
            profile: Level::uniform(Some(level)),
            info: [walks.len() as u32, buffers.capacity, u32::from(!self.field.fx.is_empty()), CANDIDATES],
        };
        queue.write_buffer(&buffers.cull, 0, bytemuck::bytes_of(&cull));
        if !walks.is_empty() {
            queue.write_buffer(&buffers.tiles, 0, bytemuck::cast_slice(&walks));
        }
        self.stats(encoder, view, &walks);
        encoder.clear_buffer(&buffers.counter, 0, Some(ARGS_OFFSET));
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("grass made"), timestamp_writes: None });
        pass.set_bind_group(0, &buffers.make, &[]);
        if !walks.is_empty() {
            pass.set_pipeline(&self.generate);
            pass.dispatch_workgroups(walks.len() as u32, 1, 1);
        }
        pass.set_pipeline(&self.finish);
        pass.dispatch_workgroups(1, 1, 1);
    }

    /// Draws view `view`'s cards ([`Made::make`]) with the pipeline and groups 0 to 2 set: binds its cards (group 3)
    /// and the grass's index buffer, which the caller's mesh draws bind again ([`crate::renderer`]'s `Bound`).
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, view: usize) {
        let Some(buffers) = self.views.get(view) else { return };
        pass.set_bind_group(3, &buffers.draw, &[]);
        pass.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed_indirect(&buffers.counter, ARGS_OFFSET);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tiles_field_holds_its_grassy_ground_and_how_far_it_stays_grassy() {
        // A 4 m square of ground, green where x < 2 m and grey past it; none past 4 m.
        let green = |u: f32, _| if u < 0.5 { [60, 140, 40] } else { [128, 128, 128] };
        let baked = super::super::tests::field(green).bake((0, 0), &[]).expect("grass");
        let cell = |x: f32, z: f32| {
            let at = ((z / CELL) as usize * CELLS + (x / CELL) as usize) * 4;
            let w = &baked.words[at..at + 4];
            (f32::from_bits(w[0]), (w[1] >> 16 & 255) as f32 / 255.0, (w[1] >> 24) as f32 / 255.0 * MOST_SUPPORT)
        };
        let ((height, likely, deep), (_, edge_likely, edge), (_, grey, _), (_, none, _)) =
            (cell(1.0, 2.0), cell(1.9, 2.0), cell(3.0, 2.0), cell(6.0, 2.0));
        assert!(height.abs() < 1e-6 && likely > 0.5 && edge_likely > 0.5, "{likely} {edge_likely}");
        assert!(grey == 0.0 && none == 0.0);
        // A metre in, the ground stays grassy past any card's footprint; a cell from the grey, a cell's.
        assert!(deep >= MOST_SUPPORT - 0.01 && edge < 0.2, "{deep} {edge}");
        assert!((baked.low, baked.high) == (0.0, 0.0));
        // Grey all over: no grass.
        assert!(super::super::tests::field(|_, _| [128, 128, 128]).bake((0, 0), &[]).is_none());
    }

    /// Each cell of tile (0, 0) of `field`: how likely a clump grows there and how far its ground stays grassy (none
    /// where the tile grows no grass).
    fn cells(field: &Field) -> Vec<(f32, f32)> {
        let Some(baked) = field.bake((0, 0), &[]) else { return Vec::new() };
        let cell = |w: &[u32]| ((w[1] >> 16 & 255) as f32 / 255.0, (w[1] >> 24) as f32 / 255.0 * MOST_SUPPORT);
        baked.words[..CELLS * CELLS * 4].chunks_exact(4).map(cell).collect()
    }

    /// How many cells of tile (0, 0) of `field` hold our narrowest clump's footprint ([`super::super::PATCH_RADIUS`]).
    fn grown(field: &Field) -> usize {
        cells(field).iter().filter(|&&(likely, support)| likely > 0.0 && support >= super::super::PATCH_RADIUS).count()
    }

    #[test]
    fn vertex_tint_cannot_grow_grass_on_grey_pavement() {
        let mut field = super::super::tests::field(|_, _| [100; 3]);
        for g in &mut field.triangles {
            g.hue = [Vec3::new(0.7, 1.0, 0.7); 3];
        }
        assert!(field.bake((0, 0), &[]).is_none());
    }

    #[test]
    fn a_narrow_green_crack_cannot_support_a_tuft() {
        let field = super::super::tests::field(|u, _| if (u - 0.5).abs() < 0.02 { [75, 125, 45] } else { [100; 3] });
        assert_eq!(grown(&field), 0);
    }

    #[test]
    fn straw_grows_only_where_the_ground_is_likely_grass_and_off_the_road() {
        let mut field = super::super::tests::field(|_, _| [158, 140, 105]);
        assert_eq!(grown(&field), 0, "straw is not plainly green");
        for g in &mut field.triangles {
            (g.road, g.prior) = (false, 0.9);
        }
        // Most of the 4 m square's 1024 cells.
        assert!(grown(&field) > 600, "{}", grown(&field));
        for g in &mut field.triangles {
            g.prior = 0.3;
        }
        assert_eq!(grown(&field), 0, "dirt's prior is too low");
    }

    #[test]
    fn footprint_crosses_mesh_seams_but_stops_at_neighbouring_pavement() {
        // A cell by the seam of the square's two triangles (x + z = 4 m).
        let at = |field: &Field| cells(field)[(1.9 / CELL) as usize * CELLS + (1.9 / CELL) as usize].1;
        let mut field = super::super::tests::field(|_, _| [75, 125, 45]);
        assert!(at(&field) > 0.2, "{}", at(&field));
        for texels in [&mut field.placement, &mut field.textures, &mut field.patches] {
            texels.push(super::super::Texels { width: 1, height: 1, rgba: vec![100, 100, 100, 255] });
        }
        field.triangles[1].first = 1;
        assert!(at(&field) < 0.2, "{}", at(&field));
    }

    #[test]
    fn the_gpu_structs_keep_the_shaders_layout() {
        assert_eq!(size_of::<Cull>(), 144);
        assert_eq!(size_of::<Walk>(), 16);
        assert_eq!(size_of::<Piece>(), 32);
        assert_eq!(size_of::<Profile>(), 16);
    }
}
