//! A grass field over the off-road ground near the road: a renderer addition behind
//! [`crate::renderer::Enhance::grass`] (ours, a proof of concept, not the game's look).
//!
//! The ground within [`BAND`] of the driveline (the off-road ground, [`crate::puddles::mark_ground`], and the road mesh,
//! whose shoulders can be grass) gets clumps of two crossed cards of grass blades drawn here ([`texture`]), up to
//! [`DENSITY`] a square metre where its untinted textures are green or straw ([`green`]) across the clump's footprint,
//! or, off the road, look like vegetation at all, dry or green ([`vegetation`]), as far as the collision under the ground
//! and its texture's name and collision say grass ([`prior`]).
//! Small marks and cracks get none; samples across triangle edges take the neighbouring ground. Each card takes the ground's
//! hue, a little greener, and brightness under it ([`BRIGHT`]); noise patches make the field taller or shorter,
//! lusher or drier ([`patches`]), the cards lean a little and the shader sways their tops in the wind (`grass_sway`).
//!
//! [`add`] keeps the ground's triangles by [`TILE`] square tile (a [`Field`]) and adds one mesh standing for the grass
//! ([`Lod::Close`]); the GPU makes the cards every frame ([`made`]): the tiles about the eye are baked once into cells
//! and a compute pass per view grows their clumps, all of them near and a thinning share further out ([`density`]),
//! as far as its [`Level`] reaches. The shader shrinks each card into the ground by its rank against the density where
//! it stands (`grass_density`, `GRASS_MARGIN`), so the field thins and thickens smoothly rather than popping, and
//! widens the cards left as much as the field thins ([`Level::profile`]), so it covers the ground as near with fewer
//! cards. They are cut-out stage draws marked as grass tufts, so the plants' lighting takes them (`own_light` in
//! `shader.wgsl`) and they cast no sun shadow; they take the sky's occlusion and the bounce computed at load for the
//! ground under them ([`Light`]).

use crate::foliage::test_texels;
use crate::parallel::in_parallel;
use crate::particles::wheel_spray::ground::smoothstep;
use crate::puddles::{self, GROUND_MARK};
use crate::stage::{
    Address, Anchor, Cover, Draw, DrawState, GrassOccluder, Image, Layer, Lod, Mesh, Stage, StageTexture, TextureName,
    TextureRef, TextureSet, Vertex, OBJECT_ALPHA_REF,
};
use glam::{Vec2, Vec3, Vec3Swizzles, Vec4Swizzles};
use rbr_formats::dds;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

pub(crate) mod made;

/// The grass texture's file name in [`Stage::texture_names`] (not a game file: [`texture`] draws it).
pub const TEXTURE: &str = "rerbr-grass.dds";
/// How far from the driveline, metres, grass grows: the hillsides and meadows in view beyond the verges too.
const BAND: f32 = 40.0;
/// Clumps (two crossed cards) a square metre on fully grassy ground within [`FULL`] of the driveline, thinning to
/// [`FAR_SHARE`] of it at [`BAND`].
const DENSITY: f32 = 40.0;
/// A converted stage's atlas clumps a square metre times its cell's area ([`Fx::density`]): smaller cells, more clumps,
/// the same cover. Four times the cover CSP's pictures measure (0.8: 0.9, 1.6 and 1.1 times CSP's share of the frame on
/// Prunières, Sa Calobra and Gabiria), as a look (ours): at CSP's the clumps stand apart as tufts over the dark ground,
/// overlapping they read as a field (`notes/assetto.md`).
const FX_COVER: f32 = 3.2;
const FULL: f32 = 10.0;
const FAR_SHARE: f32 = 0.4;
/// The steepest ground that takes grass: the cosine of its slope.
const LEVEL: f32 = 0.45;
/// The side of the square tiles, metres.
const TILE: f32 = 8.0;
/// How much grass draws and how far (`viewer --enhance grass-light` or `grass`; the launcher's Light and Dense).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Light,
    Dense,
}

impl Level {
    /// The share of the clumps drawn near the eye, the metres from the eye it stays at that within, the most its cards
    /// widen past them, and the metres by which it has faded to none ([`density`], [`widen`]; `frame.grass` in
    /// `shader.wgsl`). Past the full distance the clumps thin as one over the distance and the cards left widen as
    /// much, so the field covers the ground as near and each card keeps its width on the screen, up to the most; the
    /// last [`FADE`] of the reach fades it out, and the ground past it takes the field's colour (`thatch`).
    /// A converted stage's (`converted`) stays full [`FX_FULL`] times farther out: its atlas pictures dither out
    /// rather than shrink (`grass_dithered` in `shader.wgsl`), and the thinning shows less farther off.
    const fn profile(self, converted: bool) -> [f32; 4] {
        let [share, full, most, reach] = match self {
            Level::Light => [0.5, 20.0, 2.0, 100.0],
            Level::Dense => [1.0, 40.0, 5.0, 300.0],
        };
        [share, if converted { full * FX_FULL } else { full }, most, reach]
    }

    /// How far from the eye grass draws, metres: the tiles with their middle within this of the eye stream in and
    /// draw (at most [`REACH`]).
    fn reach(self) -> f32 {
        self.profile(false)[3]
    }

    /// `frame.grass` in `shader.wgsl`: [`Level::profile`]; with no grass a profile of none whose edges still differ.
    pub(crate) fn uniform(level: Option<Level>, converted: bool) -> [f32; 4] {
        level.map_or([0.0, 1.0, 1.0, 1.0], |level| level.profile(converted))
    }
}

/// How much farther out a converted stage's grass stays full ([`Level::profile`]).
const FX_FULL: f32 = 1.5;
/// The farthest any [`Level`] draws grass, metres ([`CLOSE_DISTANCE`] covers a tile past it).
pub(crate) const REACH: f32 = Level::Dense.profile(false)[3];
/// The share of a level's reach past which its field fades out (`GRASS_FADE` in `shader.wgsl`).
const FADE: f32 = 0.65;
/// The most any [`Level`]'s cards widen ([`widen`]): the tiles' bounds take it.
const WIDEST: f32 = 5.0;
/// How much farther a tile behind the eye counts, by its distance along the view, when the pool picks the tiles to
/// hold ([`made::Made::update`]): the pool goes to the field in view (the mirrors show the grass behind the eye).
const BEHIND: f32 = 3.0;
/// The shader grows this much more of a tile's clumps than its density draws, and shrinks those past the density into
/// the ground (`GRASS_THIN` in `shader.wgsl`), so the field thins and thickens smoothly rather than popping.
const GROW_MARGIN: f32 = 1.5;
/// A card's height range, metres, before its patch and kind scale it, and its width against its height.
const HEIGHT: [f32; 2] = [0.07, 0.14];
const WIDTH: [f32; 2] = [1.0, 1.6];
/// Final size limits after the patch and kind scale a card: ordinary grass, seed heads, and width, metres.
const MAX_HEIGHT: [f32; 2] = [0.14, 0.20];
const MAX_WIDTH: f32 = 0.20;
/// A converted stage's atlas cards: the metres of a cell per square root of CSP's `SHAPE_SIZE`, and the size for a stage
/// that sets none: the square root, as CSP's grass stands about 0.3 to 0.4 m on Prunières (9.5) and Sa Calobra (6.5) and
/// about 0.25 m on Gabiria (3), at 0.15 (`notes/assetto.md`); smaller (ours), so that the denser clumps make a sward
/// rather than a hedge, with [`FX_WIDEN`] and [`CELL_VARY`] and the patches' tallness for the size.
const CELL_PER_SIZE: f32 = 0.09;
const DEFAULT_SIZE: f32 = 1.5;
/// CSP's `MASK_MAIN_THRESHOLD` against our measure of green ([`Fx::green`]): its own pictures cover about as much of
/// Prunières' (0.75) and Roure's (0.3) verges as ours does at this share of the threshold.
const FX_MASK_SCALE: f32 = 0.4;
/// A converted stage's group pieces (flowers, weeds) against the weights the stage gives them: CSP spawns them only in
/// its sparser far passes, so its pictures show them much rarer than the weights (Sa Calobra's 4 to 2 would be a third).
const FX_PIECE_SHARE: f32 = 0.25;
/// How much each clump of an atlas varies in size, either way, and the width of its cards against their cell's: wide
/// enough that neighbours overlap.
const CELL_VARY: [f32; 2] = [0.55, 1.35];
const FX_WIDEN: f32 = 1.8;
/// A tuft needs this much vegetation around its root, metres, even when its cards are smaller.
const PATCH_RADIUS: f32 = 0.12;
/// The least grassiness across the footprint: a fixed threshold, so a lucky draw cannot grow in a faint mark.
const COVER: f32 = 0.5;
/// Maximum wind displacement against height (`GRASS_SWAY` and the gust in `shader.wgsl`).
const SWAY: f32 = 0.26;
/// Ground samples may differ by this much in height, metres: mesh seams and collision/render differences.
const GROUND_REACH: f32 = 0.10;
/// How far a card's top leans, against its height.
const LEAN: f32 = 0.25;
/// The cards' linear brightness against the texture's mean, by which the shader scales their albedo (`GRASS_MARK` in
/// `shader.wgsl`): the ground's under them, as the tint takes only its hue.
const BRIGHT: [f32; 2] = [0.2, 2.5];
/// A converted stage's: its atlas is much brighter than its dark ground, so the ratio goes lower.
const FX_BRIGHT: [f32; 2] = [0.01, 2.5];
/// The cards' brightness against the ground's: lit as a dense field (`own_light` in `shader.wgsl`), they take less light
/// than the ground. With the ground between them thatched darker (`thatch` there) the field keeps about the ground's
/// brightness, the blades lighter than the gaps (RSF Helette 580, Wythop 532, Gabiria 450).
const SHADE: f32 = 0.9;
/// The ground's grassiness ([`green`]) where grass starts and where it is full, off the road and on the road mesh
/// (whose shoulders take only plainly green ground: a pale gravel road is near straw). `THATCH_GRASSY` in `shader.wgsl`
/// is the same.
const GRASSY: [[f32; 2]; 2] = [[0.02, 0.07], [0.05, 0.11]];
/// The texels each side the ground's texture is averaged over for the grass ([`blur`]).
const BLUR: usize = 3;
/// How far each side, metres, the ground's texture is averaged for [`vegetation`]: its patches, not its grains (a
/// meadow photo's straw and soil grains, 30 cm across on RSF Gabiria, speckled the field), nor its neighbours (a road
/// atlas's gravel lane, Bareyo's, took its fields' grass at a fixed blur); at most [`PATCH_BLUR`] texels.
const PATCH: f32 = 0.6;
const PATCH_BLUR: usize = 24;
/// The share of the ground's baked vertex tint its colour keeps (`GROUND_HUE` in `shader.wgsl`).
const GROUND_HUE: f32 = 0.3;
/// The grass's saturation against the ground's hue under it: a little less, as its lighting (the sun through the
/// blades, the ground's warm light from below) saturates it more than the ground's.
const SATURATION: f32 = 0.85;
/// The hue of lush grass (display, its largest channel 1), which the grass's hue leans towards by [`LUSH_SHARE`] away
/// from its dry patches: the ground's texture is greyed by its earth, the grass on it is green.
const LUSH: Vec3 = Vec3::new(0.72, 1.0, 0.42);
const LUSH_SHARE: f32 = 0.15;
/// A converted stage's green blades' hue: the ground's at [`FX_SATURATION`], leaned by [`FX_OLIVE_SHARE`] to
/// [`FX_OLIVE`] over green ground and to [`STRAW`] over warm or brown ground ([`FX_GREEN_GROUND`]; [`FX_ALBEDO`] their
/// brightness; `shader.wgsl` darkens their roots), and towards [`STRAW`] by [`FX_DRY_SHARE`] in the dry patches
/// ([`patches`]). Chosen by eye over the test set (ours: realism, not CSP's
/// numbers): the ground's hue at half saturation and a light lean read as grey-white on Sa Calobra and Prunières.
const FX_SATURATION: f32 = 0.8;
const FX_OLIVE: Vec3 = Vec3::new(0.85, 1.0, 0.35);
const FX_OLIVE_SHARE: f32 = 0.4;
const FX_DRY_SHARE: f32 = 0.6;
/// The ground's green over its red (its texture times its tint, display) from which the blades lean wholly to
/// [`FX_OLIVE`], from all [`STRAW`] at the first to all olive at the second: CSP's blades take the ground's colour, so
/// Sa Calobra's grey-brown slopes grow dry grass and Joux Verte's green banks green.
const FX_GREEN_GROUND: [f32; 2] = [0.95, 1.15];
/// A converted stage's green blades' mean linear albedo, whatever the ground's brightness: CSP takes the ground's colour
/// for them and the atlas's green for their brightness, and its pictures show them a light olive over Prunières' and
/// Sa Calobra's dark ground (`notes/assetto.md`); a little darker than the 0.07 that matched, as the dense field shows
/// far more of them.
const FX_ALBEDO: f32 = 0.055;
/// The tint of dry grass, on the ground's hue, in the driest patches.
const STRAW: Vec3 = Vec3::new(1.12, 1.0, 0.72);
/// The texture's pictures: [`COLUMNS`] by [`ROWS`], each [`PICTURE`] texels (about a card's shape). The first eight
/// are lawn, then four meadow, two dry grass and two grass with seed heads ([`Kind`]).
const COLUMNS: usize = 4;
const ROWS: usize = 4;
const PICTURE: (usize, usize) = (256, 256);

/// The grass's ground by tile ([`add`]), from which a tile's field is baked when it is in reach ([`made`]).
pub struct Field {
    /// How much grass it draws and how far.
    pub level: Level,
    triangles: Vec<Ground>,
    tiles: HashMap<(i32, i32), Vec<u32>>,
    textures: Vec<Texels>,
    /// The same, averaged over their patches ([`PATCH_BLUR`]), for [`vegetation`].
    patches: Vec<Texels>,
    placement: Vec<Texels>,
    line: Line,
    /// The grass texture's mean colour (display).
    mean: Vec3,
    /// A converted stage's grass, placed on its marked materials, by profile (the stage's first; empty elsewhere).
    fx: Vec<Fx>,
    /// Its ground textures (surveys).
    pub report: Vec<TextureReport>,
    /// The stage texture of each of [`Field::textures`].
    texture_refs: Vec<TextureRef>,
}

/// A converted stage's GrassFX (Assetto Corsa's `[GRASS_FX]`, [`Stage::grass_fx`]) as the field takes it: grass grows
/// on the marked materials' ground, where its luminance is in range, in place of the choice by colour.
struct Fx {
    luminance: [f32; 2],
    /// CSP's colour mask, `MASK_MAIN_THRESHOLD` and `MASK_RED_THRESHOLD` ([`Fx::green`]); `None`: luminance only.
    mask: Option<[f32; 2]>,
    /// The atlas the track ships, its pieces and its size in texels; `None`: our blades.
    atlas: Option<(rbr_btb::track::GrassFx, Vec2)>,
    /// The cells ([`HIDE_STEP`] square) under its occluders ([`hidden`]), shared by the profiles.
    hidden: Arc<HashSet<(i32, i32)>>,
    /// The stage's cover map, where it ships one: no grass where it reads 0 (the profiles share it).
    cover: Option<Arc<Cover>>,
}

impl Fx {
    /// The stage's grass and its profiles ([`GrassFx::profiles`](rbr_btb::track::GrassFx::profiles)), with the atlas
    /// of `size` texels or our blades.
    fn profiles(
        settings: &rbr_btb::track::GrassFx,
        size: Option<Vec2>,
        hidden: Arc<HashSet<(i32, i32)>>,
        cover: Option<Arc<Cover>>,
    ) -> Vec<Fx> {
        std::iter::once(settings)
            .chain(&settings.profiles)
            .map(|p| Fx {
                luminance: p.luminance,
                mask: p.mask,
                atlas: size.map(|size| (p.clone(), size)),
                hidden: hidden.clone(),
                cover: cover.clone(),
            })
            .collect()
    }

    /// Whether ground of colour `rgb` (0 to 1) takes grass, its patches' colour `patch` (the blurred texels, as CSP's
    /// `MASK_BLUR` blurs): 1 or 0.
    fn mask(&self, rgb: Vec3, patch: Vec3) -> f32 {
        let l = luminance(rgb);
        let green = self.mask.is_none_or(|[main, red]| Fx::green(patch, red) > main * FX_MASK_SCALE);
        f32::from(u8::from(l >= self.luminance[0] && l <= self.luminance[1] && green))
    }

    /// How green ground of colour `rgb` (display, 0 to 1) is, as CSP's `MASK_MAIN_THRESHOLD` takes it (ours: CSP
    /// documents no formula, "how saturated with green, not real saturation, could be more than 1"): in linear light,
    /// green over the larger of blue and red less `red` (`MASK_RED_THRESHOLD`, leaning the target to yellow and red),
    /// over the mean. Grey is 0; Sa Calobra's dry verges 0.3, Roure's 0.4 (thresholds 0.0001 and 0.3).
    fn green(rgb: Vec3, red: f32) -> f32 {
        let c = rgb.powf(2.2);
        (c.y - (c.x - red).max(c.z)) / ((c.x + c.y + c.z) / 3.0).max(1e-4)
    }

    /// The metres of one atlas cell: [`CELL_PER_SIZE`] times the square root of CSP's `SHAPE_SIZE` (unset:
    /// [`DEFAULT_SIZE`]).
    fn cell(&self) -> f32 {
        let size = self.atlas.as_ref().map_or(0.0, |(a, _)| a.size);
        CELL_PER_SIZE * if size > 0.0 { size } else { DEFAULT_SIZE }.sqrt()
    }

    /// Clumps a square metre: [`FX_COVER`] cells' worth with an atlas (at most a fifth over [`DENSITY`], which
    /// [`CAPACITY`] holds), else ours.
    fn density(&self) -> f32 {
        match self.atlas {
            Some(_) => (FX_COVER / self.cell().powi(2)).min(DENSITY * 1.2),
            None => DENSITY,
        }
    }

    /// A card's picture's rectangle (`u0, u1, v0, v1`), its width and height in cells (the piece's `SIZE_MULT`, else
    /// its size) and whether it is a group's piece rather than the base grass, by `pick`: a piece of the atlas by the
    /// weights of its base (the top row's cells) and groups, then its pieces: as the GPU picks it (`piece_of` in
    /// `grass_made.wgsl`, over [`made`]'s table of the pieces).
    #[cfg(test)]
    fn piece(&self, pick: f32) -> Option<([f32; 4], [f32; 2], bool)> {
        let (atlas, size) = self.atlas.as_ref()?;
        let columns = atlas.grid[0] as f32;
        let groups = atlas.groups.iter().filter(|g| !g.pieces.is_empty());
        let mut at = pick * (atlas.base + groups.clone().map(|g| g.chance * FX_PIECE_SHARE).sum::<f32>());
        let cell = |[x, y]: [u32; 2], [w, h]: [u32; 2], cells: [f32; 2], piece: bool| {
            Some((piece_rect(atlas.grid, [x, y], [w, h], *size), cells, piece))
        };
        if at < atlas.base {
            let column = 1 + ((at / atlas.base * columns) as u32).min(atlas.grid[0] - 1);
            return cell([column, 0], [1, 1], [1.0; 2], false);
        }
        at -= atlas.base;
        for group in groups {
            let chance = group.chance * FX_PIECE_SHARE;
            if at < chance {
                let mut at = at / chance * group.pieces.iter().map(|p| p.chance).sum::<f32>();
                let piece = group.pieces.iter().find(|p| {
                    at -= p.chance;
                    at < 0.0
                });
                let piece = piece.unwrap_or(&group.pieces[0]);
                return cell(piece.cell, piece.size, piece.mult, true);
            }
            at -= chance;
        }
        cell([1, 0], [1, 1], [1.0; 2], false)
    }
}

impl Field {
    /// How many ground triangles it holds.
    pub fn triangles(&self) -> usize {
        self.triangles.len()
    }

    /// Each ground triangle's stage mesh index (once the grass's mesh is in) and its corners' vertices there, whose
    /// [`Light`] the renderer reads back.
    pub(crate) fn corners(&self) -> impl Iterator<Item = (usize, [u32; 3])> + '_ {
        self.triangles.iter().map(|g| g.corners)
    }

    /// The field's highest ground at `xz` seen from above (surveys: `examples/grass_strip.rs`).
    pub fn probe(&self, xz: Vec2) -> Option<Probe> {
        let q = Vec3::new(xz.x, 0.0, xz.y);
        let (g, w, height) = self
            .tiles
            .get(&tile_of(q))?
            .iter()
            .filter_map(|&i| {
                let g = &self.triangles[i as usize];
                let w = g.weights(q);
                w.iter().all(|&v| v >= -1e-4).then(|| (g, w, g.p[0].y * w[0] + g.p[1].y * w[1] + g.p[2].y * w[2]))
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))?;
        let grassy = self.grassy(g, w);
        let grows = grassy >= COVER && self.supported(g, Vec3::new(xz.x, height, xz.y), PATCH_RADIUS);
        Some(Probe { height, grassy, grows, prior: g.prior, texture: self.texture_refs.get(g.first as usize).copied() })
    }
}

/// The collision classes a ground texture's triangles lie on ([`surface_class`]).
pub const SURFACE_CLASSES: [&str; 7] = ["grass", "rough", "gravel", "dirt", "tarmac", "other", "none"];

/// The class of collision surface `id` in [`SURFACE_CLASSES`]: none where no collision lies under the ground.
fn surface_class(id: Option<u32>) -> usize {
    use crate::particles::wheel_spray::ground::{material, Kind};
    let Some(id) = id else { return 6 };
    if matches!(id, 0x81 | 0x82 | 0x89 | 0x8a) {
        return 1;
    }
    match material(id).kind {
        Kind::Grass => 0,
        Kind::Gravel => 2,
        Kind::Dirt => 3,
        Kind::Tarmac => 4,
        _ => 5,
    }
}

/// A ground texture's area in the field's band and its share on each collision class.
#[derive(Default)]
struct Evidence {
    area: f32,
    surfaces: [f32; 7],
}

/// What a ground texture looks like, over its texels ([`Texels::sharp`]).
#[derive(Clone, Copy, Debug)]
pub struct Looks {
    /// Mean display colour.
    pub mean: [f32; 3],
    /// Share of texels green or straw enough for grass ([`green`] at [`COVER`]).
    pub green: f32,
    /// Share of nearly grey texels.
    pub grey: f32,
}

impl Looks {
    fn of(t: &Texels) -> Looks {
        let n = (t.width * t.height).max(1);
        let rgb =
            |i: usize| Vec3::new(t.rgba[i * 4].into(), t.rgba[i * 4 + 1].into(), t.rgba[i * 4 + 2].into()) / 255.0;
        let (mut mean, mut green_n, mut grey_n) = (Vec3::ZERO, 0, 0);
        for i in 0..n {
            let c = rgb(i);
            mean += c;
            green_n += usize::from(self::green(c, false) >= COVER);
            grey_n += usize::from(c.max_element() - c.min_element() < 0.06);
        }
        let n = n as f32;
        Looks { mean: (mean / n).into(), green: green_n as f32 / n, grey: grey_n as f32 / n }
    }
}

/// A ground texture in the field's band (surveys): its file name, its area, its share on each of
/// [`SURFACE_CLASSES`] and its looks (none where it did not load).
#[derive(Clone, Debug)]
pub struct TextureReport {
    pub texture: TextureRef,
    pub name: String,
    pub area: f32,
    pub surfaces: [f32; 7],
    pub looks: Option<Looks>,
    /// A texel's side on the ground, metres (0: unknown).
    pub texel: f32,
}

/// What the field holds at a point ([`Field::probe`]).
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    /// The ground's height, metres.
    pub height: f32,
    /// How much grass the ground takes there, 0 to 1 ([`green`], or the converted stage's mask).
    pub grassy: f32,
    /// Whether a tuft grows there: [`COVER`] at the point and all round its smallest footprint ([`PATCH_RADIUS`]).
    pub grows: bool,
    /// How likely the ground is grass ([`prior`]; 0 on the road mesh and a converted stage's GrassFX ground).
    pub prior: f32,
    /// The ground's first texture.
    pub texture: Option<TextureRef>,
}

/// A ground triangle: its corners' positions, texture coordinates, vertex alphas and tints ([`tint`]), whether it is
/// the road mesh's, its textures' indices in [`Field::textures`] (the second blended by `uv1` and the alpha), its
/// upward normal, and its stage mesh's index and corners' vertices there.
struct Ground {
    p: [Vec3; 3],
    uv0: [Vec2; 3],
    uv1: [Vec2; 3],
    alpha: [f32; 3],
    hue: [Vec3; 3],
    road: bool,
    /// The grass profile of its material ([`Field::fx`]).
    profile: u8,
    /// How likely its ground is grass by the collision under it and its texture's name and collision ([`prior`]): the
    /// share of [`vegetation`] it takes besides the plainly green.
    prior: f32,
    first: u32,
    second: Option<u32>,
    normal: Vec3,
    corners: (usize, [u32; 3]),
}

impl Ground {
    /// The weights of `q` projected onto this triangle, including beyond its edges.
    fn weights(&self, q: Vec3) -> [f32; 3] {
        let [p, a, b] = self.p;
        let (e1, e2, d) = ((a - p).xz(), (b - p).xz(), (q - p).xz());
        let det = e1.perp_dot(e2);
        let (a, b) = (d.perp_dot(e2) / det, e1.perp_dot(d) / det);
        [1.0 - a - b, a, b]
    }

    fn colour(&self, textures: &[Texels], w: [f32; 3]) -> Vec3 {
        let blend = |c: &[Vec2; 3]| c[0] * w[0] + c[1] * w[1] + c[2] * w[2];
        let mut colour = textures[self.first as usize].at(blend(&self.uv0));
        if let Some(second) = self.second {
            let alpha = (self.alpha[0] * w[0] + self.alpha[1] * w[1] + self.alpha[2] * w[2]).clamp(0.0, 1.0);
            let over = textures[second as usize].at(blend(&self.uv1));
            colour = colour.lerp(over.xyz().extend(1.0), over.w * alpha);
        }
        colour.xyz()
    }
}

/// The light the load computes for a ground vertex, unpacked: how little of the sky it sees
/// ([`Vertex::occlusion`]), the octahedral pair of the direction it sees it in ([`Vertex::sky_dir`], 0 for none) and
/// what it gets off the stage per unit of the sun's and of the sky's light ([`Vertex::bounce`]). A card takes the
/// ground's at its foot, blended across the triangle as the GPU blends it; all 0 (open sky) when the renderer has
/// none.
#[derive(Clone, Copy, Default)]
pub(crate) struct Light {
    occlusion: f32,
    sky: Vec2,
    bounce: [Vec3; 2],
}

/// The range of the bounce's 10-bit shares (`RANGE` in `bounce.wgsl`).
const BOUNCE_RANGE: f32 = 2.0;

impl Light {
    pub(crate) fn of(v: &Vertex) -> Light {
        let snorm = |w: u32| (f32::from(w as u16 as i16) / 32767.0).max(-1.0);
        let shares = |w: u32| {
            Vec3::new((w & 1023) as f32, (w >> 10 & 1023) as f32, (w >> 20 & 1023) as f32) * (BOUNCE_RANGE / 1023.0)
        };
        Light {
            occlusion: v.occlusion,
            sky: Vec2::new(snorm(v.sky_dir), snorm(v.sky_dir >> 16)),
            bounce: v.bounce.map(shares),
        }
    }

    /// The light at barycentric weights `w` of a triangle whose corners have `corners`.
    fn blend(corners: &[Light; 3], w: [f32; 3]) -> Light {
        let [a, b, c] = corners;
        Light {
            occlusion: a.occlusion * w[0] + b.occlusion * w[1] + c.occlusion * w[2],
            sky: a.sky * w[0] + b.sky * w[1] + c.sky * w[2],
            bounce: [0, 1].map(|k| a.bounce[k] * w[0] + b.bounce[k] * w[1] + c.bounce[k] * w[2]),
        }
    }

    /// Packed as a vertex holds it: its `occlusion`, `sky_dir` and `bounce`.
    fn packed(self) -> (f32, u32, [u32; 2]) {
        let snorm = |x: f32| u32::from((x.clamp(-1.0, 1.0) * 32767.0).round() as i16 as u16);
        let sky = if self.sky == Vec2::ZERO { 0 } else { (snorm(self.sky.x) | snorm(self.sky.y) << 16).max(1) };
        let shares = |c: Vec3| {
            let q = (c / BOUNCE_RANGE).clamp(Vec3::ZERO, Vec3::ONE) * 1023.0;
            q.x.round() as u32 | (q.y.round() as u32) << 10 | (q.z.round() as u32) << 20
        };
        (self.occlusion, sky, self.bounce.map(shares))
    }
}

/// Keeps the grass's ground of `stage` (after its textures are loaded and its ground and roads marked) at `level`, and
/// adds its texture and the mesh standing for the grass the GPU makes ([`made_mesh`], before the blended ones) for the
/// renderer ([`crate::renderer::Renderer::set_grass`]). `None` on a snow stage or without a driveline.
pub fn add(stage: &mut Stage, level: Level) -> Option<Field> {
    if stage.snow_ground || stage.driveline.len() < 2 {
        return None;
    }
    // A converted stage's marked materials: the grass grows on them alone, drawn from the atlas it ships if it loaded.
    let marked = |m: &Mesh, d: &Draw| {
        m.anchor == Anchor::World && m.layer == Layer::Solid && d.state.grass.is_some() && d.state.alpha_ref.is_none()
    };
    let fx_stage = stage.grass_fx.is_some() && stage.meshes.iter().any(|m| m.draws.iter().any(|d| marked(m, d)));
    let grows = |m: &Mesh, d: &Draw| if fx_stage { marked(m, d) } else { puddles::ground(m, d) };
    let line = Line::new(&stage.driveline);
    let hidden = stage
        .grass_fx
        .as_ref()
        .filter(|_| fx_stage)
        .map(|fx| hidden(&fx.occluders, &line, |t| Texels::sharp(stage, t)));
    let atlas = stage.grass_fx.as_ref().filter(|_| fx_stage).and_then(|fx| {
        let texture = fx.texture?;
        let Some(Some(Image::Dds(bytes))) = stage.textures.get(&texture).map(|t| &t.image) else { return None };
        let (bytes, mean, size) = track_atlas(bytes)?;
        Some((texture, bytes, mean, fx.settings.clone(), size, fx.cover.clone()))
    });
    let index = stage.texture_names.keys().chain(stage.textures.keys()).filter(|t| t.set == TextureSet::Stage);
    let texture = TextureRef { set: TextureSet::Stage, index: index.map(|t| t.index + 1).max().unwrap_or(0) };
    let (bytes, mean) = if atlas.is_some() { (Vec::new(), Vec3::ZERO) } else { self::texture() };
    // The ground draws' textures' texels, made over the load's threads (one after another they took 0.7 s on 580),
    // numbered as the draws below first use them.
    let ground = stage.meshes.iter().flat_map(|m| m.draws.iter().filter(|d| d.lod != Lod::Far && grows(m, d)));
    let wanted: BTreeSet<TextureRef> = ground
        .flat_map(|d| [d.state.textures[0], d.state.textures[1].filter(|_| d.state.second_texture)])
        .flatten()
        .collect();
    let wanted: Vec<TextureRef> = wanted.into_iter().collect();
    let spans = spans(stage, grows);
    let made = in_parallel(&wanted, |&t| {
        let sharp = Texels::sharp(stage, t)?;
        // The patches' blur in texels: [`PATCH`] metres at the texture's size on the ground.
        let texel = spans.get(&t).map_or(0.0, |&(world, uv)| texel_size(world, uv, &sharp));
        let radius = if texel > 0.0 { (PATCH / texel).round().clamp(1.0, PATCH_BLUR as f32) as usize } else { BLUR };
        Some((sharp.blurred(BLUR), sharp.blurred(radius), sharp))
    });
    let mut made: HashMap<TextureRef, (Texels, Texels, Texels)> =
        wanted.into_iter().zip(made).filter_map(|(t, texels)| Some((t, texels?))).collect();
    let mut textures = Vec::new();
    let mut placement = Vec::new();
    let mut patches = Vec::new();
    let mut texture_refs = Vec::new();
    let mut texture_index: HashMap<TextureRef, Option<u32>> = HashMap::new();
    let mut texels = |t: Option<TextureRef>| {
        *texture_index.entry(t?).or_insert_with(|| {
            let (colour, patch, sharp) = made.remove(&t?)?;
            textures.push(colour);
            patches.push(patch);
            placement.push(sharp);
            texture_refs.push(t?);
            Some(textures.len() as u32 - 1)
        })
    };
    let mut triangles = Vec::new();
    let mut evidence: HashMap<TextureRef, Evidence> = HashMap::new();
    let mut sources = Vec::new();
    let mut tiles: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    // The grass's mesh goes before the first blended mesh, which moves the meshes from there on by one.
    let blended = stage.meshes.iter().position(|m| m.layer == Layer::Blended).unwrap_or(stage.meshes.len());
    let meshes = stage.meshes.iter().enumerate().filter(|(_, m)| m.anchor == Anchor::World && m.layer == Layer::Solid);
    for (index, mesh) in meshes {
        let (own, index) = (index, if index < blended { index } else { index + 1 });
        for draw in mesh.draws.iter().filter(|d| d.lod != Lod::Far && grows(mesh, d)) {
            let Some(first) = texels(draw.state.textures[0]) else { continue };
            let second = if draw.state.second_texture { texels(draw.state.textures[1]) } else { None };
            for t in mesh.indices[draw.indices.clone()].chunks_exact(3) {
                let corners = [t[0], t[1], t[2]];
                let v = corners.map(|i| &mesh.vertices[i as usize]);
                // The off-road ground, or the road mesh, whose shoulders can be grass: its texture says where.
                if !fx_stage && v.iter().any(|v| v.edge > GROUND_MARK && v.wet <= 0.0) {
                    continue;
                }
                // A converted stage's grass never grows on the road it drives, even where its grass materials take
                // the road's (Gabiria lists them): CSP's pictures show none there.
                if fx_stage && v.iter().any(|v| v.wet > 0.0) {
                    continue;
                }
                let p = v.map(|v| Vec3::from(v.position));
                let cross = (p[1] - p[0]).cross(p[2] - p[0]);
                let normal = cross.normalize_or_zero() * cross.y.signum();
                let (low, high) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
                let near = line.distance(((low + high) * 0.5).xz()) - (high - low).xz().length() * 0.5;
                if normal.y < LEVEL || cross.y.abs() < 1e-6 || near > BAND {
                    continue;
                }
                let centre = (p[0] + p[1] + p[2]) / 3.0;
                let hit = stage.collision.as_ref().map(|ground| ground.below([centre.x, centre.z, centre.y]));
                // Over collision ground (none: past a converted stage's collision corridor), not a prop above it.
                if !fx_stage && hit.flatten().is_some_and(|hit| (centre.y - hit.z).abs() > GROUND_REACH) {
                    continue;
                }
                let alpha = v.iter().map(|v| f32::from(v.color[3])).sum::<f32>() / (3.0 * 255.0);
                let look = match draw.state.textures {
                    [_, Some(over)] if draw.state.second_texture && alpha > 0.5 => Some(over),
                    [first, _] => first,
                };
                let class = surface_class(hit.flatten().map(|h| h.surface));
                if let Some(look) = look {
                    let e = evidence.entry(look).or_default();
                    e.area += cross.length() * 0.5;
                    e.surfaces[class] += cross.length() * 0.5;
                }
                sources.push((class, look, own, corners));
                let triangle = triangles.len() as u32;
                triangles.push(Ground {
                    p,
                    uv0: v.map(|v| Vec2::from(v.uv0)),
                    uv1: v.map(|v| Vec2::from(v.uv1)),
                    alpha: v.map(|v| f32::from(v.color[3]) / 255.0),
                    hue: v.map(|v| tint(v.color)),
                    road: v.iter().any(|v| v.wet > 0.0),
                    profile: draw.state.grass.unwrap_or(0),
                    prior: 0.0,
                    first,
                    second,
                    normal,
                    corners: (index, corners),
                });
                let (from, to) = (tile_of(low), tile_of(high));
                for x in from.0..=to.0 {
                    for z in from.1..=to.1 {
                        tiles.entry((x, z)).or_default().push(triangle);
                    }
                }
            }
        }
    }
    // The atlas is the stage's own texture; ours is made here.
    let (texture, mean, fx) = match atlas {
        Some((texture, bytes, mean, settings, size, cover)) => {
            stage
                .textures
                .insert(texture, StageTexture { request: TEXTURE.to_owned(), image: Some(Image::Dds(bytes)) });
            (texture, mean, Fx::profiles(&settings, Some(size), Arc::new(hidden.unwrap_or_default()), cover))
        }
        None => {
            stage.texture_names.insert(
                texture,
                TextureName { track: stage.name.clone(), file_name: TEXTURE.to_owned(), ground: false },
            );
            stage
                .textures
                .insert(texture, StageTexture { request: TEXTURE.to_owned(), image: Some(Image::Dds(bytes)) });
            let fx = stage.grass_fx.as_ref().filter(|_| fx_stage);
            let fx =
                fx.map(|fx| Fx::profiles(&fx.settings, None, Arc::new(hidden.unwrap_or_default()), fx.cover.clone()));
            (texture, mean, fx.unwrap_or_default())
        }
    };
    let name = |t: TextureRef| stage.texture_names.get(&t).map_or("", |n| n.file_name.as_str());
    let by_texture: HashMap<TextureRef, f32> = evidence.iter().map(|(&t, e)| (t, texture_prior(e, name(t)))).collect();
    for (g, (class, look, mesh, corners)) in triangles.iter_mut().zip(sources) {
        // The road mesh takes only the plainly green, as before (Kinamba Yala's savanna texture is its road's too).
        g.prior = if g.road { 0.0 } else { prior(class, look.and_then(|t| by_texture.get(&t).copied())) };
        // The ground shader's thatch takes the same test (`thatch` in `shader.wgsl`): its vertices carry the prior in
        // their mark, the most of their triangles'.
        if !fx_stage && !g.road {
            for i in corners {
                let v = &mut stage.meshes[mesh].vertices[i as usize];
                v.edge = v.edge.min(GROUND_MARK - g.prior);
            }
        }
    }
    let report = evidence
        .into_iter()
        .map(|(t, e)| {
            let index = texture_index.get(&t).copied().flatten();
            let looks = index.map(|i| Looks::of(&placement[i as usize]));
            let texel =
                index.zip(spans.get(&t)).map_or(0.0, |(i, &(world, uv))| texel_size(world, uv, &placement[i as usize]));
            let name = stage.texture_names.get(&t).map_or_else(String::new, |n| n.file_name.clone());
            TextureReport {
                texture: t,
                name,
                area: e.area,
                surfaces: e.surfaces.map(|s| s / e.area.max(1e-6)),
                looks,
                texel,
            }
        })
        .collect();
    stage.meshes.insert(blended, made_mesh(texture));
    Some(Field { level, triangles, tiles, textures, patches, placement, line, mean, fx, report, texture_refs })
}

impl Field {
    /// The converted stage's grass on ground `g`, at its profile.
    fn fx(&self, g: &Ground) -> Option<&Fx> {
        self.fx.get(usize::from(g.profile)).or(self.fx.first())
    }

    /// Untinted vegetation on the actual ground at `q`; try the root's triangle before its neighbours.
    fn vegetation(&self, root: &Ground, q: Vec3) -> f32 {
        let sample = |g: &Ground| {
            let w = g.weights(q);
            let y = g.p[0].y * w[0] + g.p[1].y * w[1] + g.p[2].y * w[2];
            (w.iter().all(|&v| v >= -1e-4) && (q.y - y).abs() <= GROUND_REACH).then(|| self.grassy(g, w))
        };
        sample(root).unwrap_or_else(|| {
            self.tiles
                .get(&tile_of(q))
                .into_iter()
                .flatten()
                .find_map(|&i| sample(&self.triangles[i as usize]))
                .unwrap_or(0.0)
        })
    }

    /// How much grass ground `g` takes at weights `w` ([`green`], or the converted stage's [`Fx::mask`]).
    fn grassy(&self, g: &Ground, w: [f32; 3]) -> f32 {
        let rgb = g.colour(&self.placement, w);
        let at = (g.p[0] * w[0] + g.p[1] * w[1] + g.p[2] * w[2]).xz();
        match self.fx(g) {
            Some(fx) if fx.hidden.contains(&hide_cell(at)) => 0.0,
            Some(fx) if fx.cover.as_ref().is_some_and(|c| c.at(at) == 0) => 0.0,
            Some(fx) => fx.mask(rgb, g.colour(&self.textures, w)),
            None if g.prior > 0.0 => green(rgb, g.road).max(g.prior * vegetation(g.colour(&self.patches, w))),
            None => green(rgb, g.road),
        }
    }

    /// A small ring over the supporting ground, covering both cards, their lean and their wind movement ([`Field::probe`];
    /// the GPU takes each cell's support, [`made`]).
    fn supported(&self, g: &Ground, at: Vec3, radius: f32) -> bool {
        let d = std::f32::consts::FRAC_1_SQRT_2;
        [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (d, d), (d, -d), (-d, d), (-d, -d)].into_iter().all(
            |(x, z)| {
                let (x, z) = (x * radius, z * radius);
                let y = -(g.normal.x * x + g.normal.z * z) / g.normal.y;
                self.vegetation(g, at + Vec3::new(x, y, z)) >= COVER
            },
        )
    }

    /// The hue (display, its largest channel 255) and the brightness against the texture's ([`BRIGHT`], before
    /// each clump's own and the bounds) of the grass on ground `g` at weights `w` in a patch of dryness `dry`: the
    /// ground's under it ([`SATURATION`], [`LUSH`], [`STRAW`]), or a converted stage's ([`FX_SATURATION`]).
    fn look(&self, g: &Ground, w: [f32; 3], dry: f32) -> ([u8; 3], f32) {
        // A cover-map stage's tufts keep the atlas's own colours (kind 2: `shader.wgsl`): a white hue, no brightness.
        if self.fx.first().is_some_and(|fx| fx.cover.is_some()) {
            return ([255; 3], 1.0);
        }
        let colour = g.colour(&self.textures, w) * (g.hue[0] * w[0] + g.hue[1] * w[1] + g.hue[2] * w[2]);
        let hue = colour / self.mean;
        let grey = Vec3::splat(hue.element_sum() / 3.0);
        let converted = !self.fx.is_empty();
        let hue = grey.lerp(hue, if converted { FX_SATURATION } else { SATURATION }).max(Vec3::splat(0.01));
        let hue = if converted {
            let straw = STRAW / STRAW.max_element();
            let lean = straw.lerp(FX_OLIVE, smoothstep(FX_GREEN_GROUND, colour.y / colour.x.max(1e-4)));
            (hue / hue.max_element()).lerp(lean, FX_OLIVE_SHARE).lerp(straw, dry * FX_DRY_SHARE)
        } else {
            let hue = (hue / hue.max_element()).lerp(LUSH, LUSH_SHARE * (1.0 - dry));
            hue.lerp(hue * STRAW, dry * 0.35)
        };
        let hue = (hue / hue.max_element() * 255.0).round().as_uvec3();
        let lit = if converted { FX_ALBEDO } else { SHADE * luminance(colour) };
        ([hue.x as u8, hue.y as u8, hue.z as u8], lit / luminance(self.mean))
    }
}

/// The field's patches at `p` (stage x and z): how tall the grass is against its mean (0.55 to 1.5) and how dry (0 to
/// 1), from value noise at a few metres and under a metre.
fn patches(p: Vec2) -> (f32, f32) {
    let tall = noise(p / 3.5, 1) * 0.7 + noise(p / 0.9, 2) * 0.3;
    let dry = smoothstep([0.6, 0.9], noise(p / 7.0, 3) * 0.8 + noise(p / 1.6, 4) * 0.2);
    (0.55 + 0.95 * tall, dry)
}

/// Smooth value noise, 0 to 1, of lattice `seed`.
fn noise(p: Vec2, seed: u64) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let at = |x: f32, y: f32| {
        let mut random = Random((cell.x + x) as i64 as u64 ^ ((cell.y + y) as i64 as u64).rotate_left(32) ^ seed << 56);
        random.next()
    };
    let s = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * s.x;
    let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * s.x;
    top + (bottom - top) * s.y
}

fn tile_of(p: Vec3) -> (i32, i32) {
    ((p.x / TILE).floor() as i32, (p.z / TILE).floor() as i32)
}

/// The density of the grass at `level` `distance` metres from the eye: its share within its full distance, then as
/// one over the distance, faded out over the last [`FADE`] of its reach ([`Level::profile`]; `grass_density` in
/// `shader.wgsl` is the same).
fn density([share, full, _, reach]: [f32; 4], distance: f32) -> f32 {
    share * (full / distance.max(1e-3)).min(1.0) * (1.0 - smoothstep([FADE * reach, reach], distance))
}

/// How much wider than its own a card of `profile` draws `distance` metres from the eye: as much as its clumps thin
/// past the full distance, up to the level's most (`grass_widen` in `shader.wgsl`).
#[cfg(test)]
fn widen([_, full, most, _]: [f32; 4], distance: f32) -> f32 {
    (distance / full).clamp(1.0, most)
}

/// The mesh standing for the grass the GPU makes ([`made::is_made`]): one cut-out draw of the grass's `texture`, which
/// holds its pipelines, with no vertices, indices or bounds (the load's bakes over the stage's extent leave it out).
fn made_mesh(texture: TextureRef) -> Mesh {
    let state = DrawState {
        textures: [Some(texture), None],
        shadow: None,
        address: [Address::Clamp; 2],
        second_texture: false,
        alpha_ref: Some(OBJECT_ALPHA_REF),
        mip_bias: false,
        ground: false,
        grass: None,
        cull_back: false,
        blend: false,
        depth_test: true,
        depth_write: true,
        shade: None,
        specular: None,
        surface: None,
    };
    Mesh {
        layer: Layer::Solid,
        anchor: Anchor::World,
        bounds: None,
        cell: None,
        draws: vec![Draw { indices: 0..0, state, lod: Lod::Close }],
        vertices: Vec::new(),
        indices: Vec::new(),
        sun_cascades: true,
    }
}

/// How far a point `offset` from the eye (stage x and z) counts for the eye looking along `forward` (unit, or zero for
/// none): its distance, with its distance along the view [`BEHIND`] times as far when it lies behind the eye.
fn seen(offset: Vec2, forward: Vec2) -> f32 {
    let along = offset.dot(forward);
    if along >= 0.0 {
        return offset.length();
    }
    let across = offset - forward * along;
    across.length().hypot(along * BEHIND)
}

/// How much grass ground of colour `rgb` (0 to 1) takes, off the road or on the road mesh, by how green or straw its
/// chromaticity is ([`GRASSY`]).
fn green(rgb: Vec3, road: bool) -> f32 {
    let c = rgb / rgb.element_sum().max(1e-3);
    // Green or straw (green over blue) and not earth (red over green) nor stone (grey).
    let [low, high] = GRASSY[usize::from(road)];
    smoothstep([low, high], c.y - c.z - 2.0 * (c.x - c.y - 0.02).max(0.0))
}

/// A texel's side on the ground, metres, of `texels` spread `world` square metres over `uv` of its coordinates'
/// squares ([`spans`]); 0 where it is not spread.
fn texel_size(world: f32, uv: f32, texels: &Texels) -> f32 {
    if uv <= 0.0 {
        0.0
    } else {
        (world / (uv * (texels.width * texels.height) as f32)).sqrt()
    }
}

/// Each ground texture's ground area and texture area (in its coordinates' squares) under the draws `grows` takes, by
/// which it is drawn ([`PATCH`]).
fn spans(stage: &Stage, grows: impl Fn(&Mesh, &Draw) -> bool) -> HashMap<TextureRef, (f32, f32)> {
    let mut spans: HashMap<TextureRef, (f32, f32)> = HashMap::new();
    for mesh in &stage.meshes {
        for draw in mesh.draws.iter().filter(|d| d.lod != Lod::Far && grows(mesh, d)) {
            let layers =
                [(draw.state.textures[0], 0), (draw.state.textures[1].filter(|_| draw.state.second_texture), 1)];
            for t in mesh.indices[draw.indices.clone()].chunks_exact(3) {
                let v = [0, 1, 2].map(|k| &mesh.vertices[t[k] as usize]);
                let p = v.map(|v| Vec3::from(v.position));
                let world = (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
                for (texture, layer) in layers {
                    let Some(texture) = texture else { continue };
                    let uv = v.map(|v| Vec2::from(if layer == 0 { v.uv0 } else { v.uv1 }));
                    let span = spans.entry(texture).or_default();
                    span.0 += world;
                    span.1 += (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs() * 0.5;
                }
            }
        }
    }
    spans
}

/// How much ground of colour `rgb` (display, 0 to 1, blurred: its patches) looks like vegetation, green or dry (ours):
/// its hue from orange-yellow to green, some saturation, not dark. Straw and olive pass, which [`green`] leaves out;
/// soil (redder), rock and gravel (grey) do not. Only ground likely grass by what lies under it takes it ([`prior`]).
fn vegetation(rgb: Vec3) -> f32 {
    let (most, least) = (rgb.max_element(), rgb.min_element());
    let chroma = most - least;
    if chroma < 1e-3 || rgb.z >= rgb.y {
        return 0.0;
    }
    // Hue in degrees, red 0, yellow 60, green 120: blue under green, so 0 to 180.
    let hue = if rgb.x >= rgb.y { 60.0 * (rgb.y - rgb.z) / chroma } else { 60.0 * (2.0 + (rgb.z - rgb.x) / chroma) };
    smoothstep(VEGETATION_HUE, hue) * smoothstep(VEGETATION_SATURATION, chroma / most) * smoothstep([0.08, 0.16], most)
}

/// The hue (degrees) and saturation where [`vegetation`] starts and is full: RSF Gabiria's meadow (`forest-brown`,
/// 40 degrees, 0.34) and Bareyo's grass (37, 0.36) in, their soil (34 and 29) mostly out, gravel (grey) out.
const VEGETATION_HUE: [f32; 2] = [32.0, 38.0];
const VEGETATION_SATURATION: [f32; 2] = [0.10, 0.18];

/// How likely ground on a collision class ([`SURFACE_CLASSES`]) is grass: the grass's own and rough (RBR's `Rough_`,
/// meadows and heath on RSF stages) most, dirt a little (verges marked loose under grass), objects, rocks, bushes and
/// ground without collision half; gravel and tarmac none (a road atlas's grassy shoulders still take the plainly
/// green, [`green`]).
const CLASS_PRIOR: [f32; 7] = [1.0, 0.85, 0.0, 0.3, 0.0, 0.5, 0.5];

/// Words in a texture's file name that say grass, and those that say it is not (lower case).
const GRASS_WORDS: [&str; 14] = [
    "grass", "gras", "herbe", "champ", "meadow", "lawn", "prato", "prado", "hierba", "cesped", "wiese", "rasen",
    "nurmi", "heath",
];
const BARE_WORDS: [&str; 18] = [
    "road", "route", "asphalt", "tarmac", "gravel", "gravier", "grava", "rock", "roche", "stone", "sand", "dirt",
    "mud", "soil", "concrete", "beton", "snow", "kerb",
];

/// How likely a ground texture is grass, 0 to 1, by the collision under all its ground ([`CLASS_PRIOR`] by area) and
/// its file name: a grass word lifts it, a word for bare ground lowers it.
fn texture_prior(e: &Evidence, name: &str) -> f32 {
    let collision = e.surfaces.iter().zip(CLASS_PRIOR).map(|(s, p)| s * p).sum::<f32>() / e.area.max(1e-6);
    let name = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    let named = if has(&GRASS_WORDS) {
        1.0
    } else if has(&BARE_WORDS) {
        0.0
    } else {
        collision
    };
    (collision + named) * 0.5
}

/// A ground triangle's [`Ground::prior`]: its own collision class's and its texture's ([`texture_prior`]), half each.
fn prior(class: usize, texture: Option<f32>) -> f32 {
    let own = CLASS_PRIOR[class];
    texture.map_or(own, |t| (own + t) * 0.5)
}

/// The rectangle (`u0, u1, v0, v1`, a texel in from its edges) of an atlas piece: `cell` (column from 1, row from 0 at
/// the top, the base grass's, as CSP counts: "columns index start at 1, but rows index start at 0") and `size` in
/// cells, of a `grid` of columns and rows over a texture of `texels`.
fn piece_rect(grid: [u32; 2], cell: [u32; 2], size: [u32; 2], texels: Vec2) -> [f32; 4] {
    let [columns, rows] = grid.map(|n| n as f32);
    let (u0, v0) = ((cell[0] - 1) as f32 / columns, cell[1] as f32 / rows);
    let (u1, v1) = (((cell[0] - 1 + size[0]) as f32 / columns).min(1.0), ((cell[1] + size[1]) as f32 / rows).min(1.0));
    let margin = Vec2::new(2.0, 2.0) / texels;
    [u0 + margin.x, u1 - margin.x, v0 + margin.y, v1 - margin.y]
}

/// A track's grass atlas (`dds`: plain 24- or 32-bit, DXT1, DXT3, DXT5 or BC7) as drawn: its colours as they are (CSP's
/// bright green blades, red and blue nought, take the ground's colour in `shader.wgsl`, their green the brightness;
/// the rest keep theirs), with its mip chain and each level's alpha scaled to keep the top level's cut-out coverage
/// ([`keep_coverage`]). Its bytes, the green blades' mean brightness (display, weighted by alpha) and its size in
/// texels.
fn track_atlas(dds: &[u8]) -> Option<(Vec<u8>, Vec3, Vec2)> {
    let plain = dds::to_rgba8(dds);
    let file = dds::parse(plain.as_deref().unwrap_or(dds)).ok()?;
    let level = file.levels().into_iter().next()?;
    let (width, height) = (level.width as usize, level.height as usize);
    let pixels = crate::upscale::decompress(file.format, level.data, level.width, level.height);
    let (mut sum, mut weight) = (0.0, 0.0);
    for t in pixels.chunks_exact(4) {
        let marker = t[0].max(t[2]) as f32 <= 0.1 * f32::from(t[1]);
        let alpha = f32::from(t[3]) / 255.0 * f32::from(u8::from(marker));
        (sum, weight) = (sum + f32::from(t[1]) / 255.0 * alpha, weight + alpha);
    }
    let bytes = dds::with_mips(&dds::rgba8_file(width as u32, height as u32, &pixels))?;
    Some((
        keep_coverage(bytes, width, height),
        Vec3::splat(if weight >= 1.0 { sum / weight } else { 0.7 }),
        Vec2::new(width as f32, height as f32),
    ))
}

/// The metres of a cell of a converted stage's [`hidden`] ground: CSP's GrassFX `MAP_STEP`.
const HIDE_STEP: f32 = 0.25;

/// The [`HIDE_STEP`] cell holding `xz`.
fn hide_cell(xz: Vec2) -> (i32, i32) {
    let cell = (xz / HIDE_STEP).floor();
    (cell.x as i32, cell.y as i32)
}

/// The cells a converted stage's grass grows nowhere in, as CSP's GrassFX hides it under `OCCLUDING_MATERIALS` (seen
/// from above, whatever their height): those whose centre an occluder near the road covers, and where it reads its
/// texture (`OCCLUDING_MATERIALS_ALPHA`, Prunières' road edges, its `texels`) only under texels at least half opaque.
fn hidden(
    occluders: &[GrassOccluder],
    line: &Line,
    mut texels_of: impl FnMut(TextureRef) -> Option<Texels>,
) -> HashSet<(i32, i32)> {
    let mut texels: HashMap<TextureRef, Option<Texels>> = HashMap::new();
    let mut cells = HashSet::new();
    for o in occluders {
        let (low, high) = (o.p[0].min(o.p[1]).min(o.p[2]), o.p[0].max(o.p[1]).max(o.p[2]));
        if line.distance(((low + high) * 0.5).xz()) - (high - low).xz().length() * 0.5 > BAND {
            continue;
        }
        let alpha = o.texture.and_then(|t| texels.entry(t).or_insert_with(|| texels_of(t)).as_ref());
        let [a, b, c] = o.p.map(|p| p.xz());
        let area = (b - a).perp_dot(c - a);
        if area.abs() < 1e-8 {
            continue;
        }
        let ((x0, z0), (x1, z1)) = (hide_cell(low.xz()), hide_cell(high.xz()));
        for x in x0..=x1 {
            for z in z0..=z1 {
                let q = (Vec2::new(x as f32, z as f32) + 0.5) * HIDE_STEP;
                let (wb, wc) = ((q - a).perp_dot(c - a) / area, (b - a).perp_dot(q - a) / area);
                let w = [1.0 - wb - wc, wb, wc];
                if w.iter().any(|&w| w < 0.0) {
                    continue;
                }
                let opaque = alpha.is_none_or(|texels| {
                    let uv = o.uv.map(Vec2::from);
                    texels.at(uv[0] * w[0] + uv[1] * w[1] + uv[2] * w[2]).w >= 0.5
                });
                if opaque {
                    cells.insert((x, z));
                }
            }
        }
    }
    cells
}

/// The linear luminance of a display colour (0 to 1).
fn luminance(c: Vec3) -> f32 {
    c.powf(2.2).dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// The tint of a ground vertex of colour `rgb`, as `own_light` in `shader.wgsl` takes it on the near ground: its hue,
/// [`GROUND_HUE`] of it.
pub(crate) fn tint([r, g, b, _]: [u8; 4]) -> Vec3 {
    let c = Vec3::new(r.into(), g.into(), b.into()) / 255.0;
    let most = c.max_element();
    let hue = Vec3::ONE.lerp(c / most.max(1e-3), smoothstep([0.02, 0.08], most));
    Vec3::ONE.lerp(hue, GROUND_HUE)
}

/// A ground texture's texels at its test level (`foliage::test_texels`), sampled nearest and repeated.
pub(crate) struct Texels {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl Texels {
    fn blurred(&self, radius: usize) -> Texels {
        let (width, height) = (self.width, self.height);
        let rgba = blur(&blur(&self.rgba, width, height, (1, 0), radius), width, height, (0, 1), radius);
        Texels { width, height, rgba }
    }

    /// The unblurred test level, for placement and the dust's colour.
    pub(crate) fn sharp(stage: &Stage, texture: TextureRef) -> Option<Texels> {
        let (width, rgba) = test_texels(stage, texture)?;
        let height = rgba.len() / 4 / width.max(1);
        (height > 0).then_some(Texels { width, height, rgba })
    }

    /// RGBA, 0 to 1.
    pub(crate) fn at(&self, uv: Vec2) -> glam::Vec4 {
        let x = (uv.x.rem_euclid(1.0) * self.width as f32) as usize % self.width;
        let y = (uv.y.rem_euclid(1.0) * self.height as f32) as usize % self.height;
        let i = (y * self.width + x) * 4;
        glam::Vec4::from_array(std::array::from_fn(|c| f32::from(self.rgba[i + c]) / 255.0))
    }
}

/// `rgba` (`width` x `height`, repeated) averaged over `radius` texels each side along `step` ([`BLUR`],
/// [`PATCH_BLUR`]): the grass follows the ground's patches, not its texels.
fn blur(rgba: &[u8], width: usize, height: usize, step: (usize, usize), radius: usize) -> Vec<u8> {
    let at = |x: isize, y: isize, c: usize| {
        let (x, y) = (x.rem_euclid(width as isize) as usize, y.rem_euclid(height as isize) as usize);
        u32::from(rgba[(y * width + x) * 4 + c])
    };
    let mut out = vec![0; rgba.len()];
    for y in 0..height {
        for x in 0..width {
            for c in 0..4 {
                let sum: u32 = (-(radius as isize)..=radius as isize)
                    .map(|k| at(x as isize + step.0 as isize * k, y as isize + step.1 as isize * k, c))
                    .sum();
                out[(y * width + x) * 4 + c] = (sum / (2 * radius as u32 + 1)) as u8;
            }
        }
    }
    out
}

/// The driveline's segments in a grid, for the distance across to it.
pub(crate) struct Line {
    grid: HashMap<(i32, i32), Vec<(Vec2, Vec2)>>,
}

impl Line {
    pub(crate) fn new(points: &[Vec3]) -> Line {
        let mut grid: HashMap<(i32, i32), Vec<(Vec2, Vec2)>> = HashMap::new();
        for pair in points.windows(2) {
            let (a, b) = (pair[0].xz(), pair[1].xz());
            let (low, high) = (a.min(b) - BAND, a.max(b) + BAND);
            for x in Line::cell(low.x)..=Line::cell(high.x) {
                for z in Line::cell(low.y)..=Line::cell(high.y) {
                    grid.entry((x, z)).or_default().push((a, b));
                }
            }
        }
        Line { grid }
    }

    fn cell(c: f32) -> i32 {
        (c / BAND).floor() as i32
    }

    /// The distance across from `p` to the driveline, up to just past [`BAND`].
    pub(crate) fn distance(&self, p: Vec2) -> f32 {
        let segments = self.grid.get(&(Line::cell(p.x), Line::cell(p.y)));
        segments.into_iter().flatten().fold(BAND + 1.0, |near, &(a, b)| {
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            near.min(p.distance(a + ab * t))
        })
    }
}

/// A small hash-seeded generator (SplitMix64), for the patches' noise.
struct Random(u64);

impl Random {
    /// 0 to 1.
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The texture being drawn: colour (display) and coverage per texel.
struct Canvas {
    width: usize,
    colour: Vec<Vec3>,
    alpha: Vec<f32>,
}

impl Canvas {
    /// One blade in the picture whose box is `left`, `top` (texels) and [`PICTURE`]: rooted at `root` across its bottom,
    /// rising `tall` with its tip `lean` across, bending over by `droop` (0 straight, 1 its tip back at half its height),
    /// `base` wide at the root and tapering to a point, shaded from `foot` to `tip`.
    #[allow(clippy::too_many_arguments)]
    fn blade(
        &mut self,
        (left, top): (usize, usize),
        root: f32,
        tall: f32,
        lean: f32,
        base: f32,
        droop: f32,
        foot: Vec3,
        tip: Vec3,
    ) {
        let (w, h) = (PICTURE.0 as f32, PICTURE.1 as f32);
        // A quadratic curve: up from the root, over towards the tip.
        let from = Vec2::new(root, h - 1.0);
        let bend = Vec2::new(root + lean * 0.35, h - 1.0 - tall);
        let to = Vec2::new(root + lean, h - 1.0 - tall * (1.0 - 0.5 * droop));
        let steps = ((tall + lean.abs()) * 2.0) as usize + 2;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let at = from * (1.0 - t) * (1.0 - t) + bend * 2.0 * t * (1.0 - t) + to * t * t;
            let half = base * 0.5 * (1.0 - t).powf(0.7) + 0.25;
            let colour = foot.lerp(tip, t.powf(0.7));
            let (x0, x1) = ((at.x - half - 1.0).floor() as i64, (at.x + half + 1.0).ceil() as i64);
            let (y0, y1) = ((at.y - half - 1.0).floor() as i64, (at.y + half + 1.0).ceil() as i64);
            for y in y0.max(0)..=y1.min(h as i64 - 1) {
                for x in x0.max(2)..=x1.min(w as i64 - 3) {
                    let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(at);
                    let cover = (half + 0.5 - d).clamp(0.0, 1.0);
                    if cover > 0.0 {
                        let i = (y as usize + top) * self.width + left + x as usize;
                        self.colour[i] = self.colour[i].lerp(colour, cover);
                        self.alpha[i] = self.alpha[i].max(cover);
                    }
                }
            }
        }
    }

    /// A seed head: an upright ellipse `long` by `thick` texels about `centre` in the picture at `left`, `top`.
    fn seeds(&mut self, (left, top): (usize, usize), centre: Vec2, long: f32, thick: f32, colour: Vec3) {
        let (w, h) = (PICTURE.0 as i64, PICTURE.1 as i64);
        for y in (centre.y - long) as i64..=(centre.y + long) as i64 {
            for x in (centre.x - thick) as i64..=(centre.x + thick) as i64 {
                let d = Vec2::new((x as f32 - centre.x) / thick, (y as f32 - centre.y) / long).length();
                let cover = ((1.0 - d) * thick).clamp(0.0, 1.0);
                if cover > 0.0 && (2..w - 2).contains(&x) && (0..h).contains(&y) {
                    let i = (y as usize + top) * self.width + left + x as usize;
                    self.colour[i] = self.colour[i].lerp(colour, cover);
                    self.alpha[i] = self.alpha[i].max(cover);
                }
            }
        }
    }
}

/// The grass texture: [`COLUMNS`] by [`ROWS`] pictures of blades ([`Kind`]), as an RGBA `.dds` with its mip chain, each
/// level's alpha scaled to keep the top level's cut-out coverage (so far grass does not thin away), and the blades'
/// mean colour (0 to 1, display).
fn texture() -> (Vec<u8>, Vec3) {
    let (width, height) = (PICTURE.0 * COLUMNS, PICTURE.1 * ROWS);
    let mut canvas = Canvas { width, colour: vec![Vec3::ZERO; width * height], alpha: vec![0.0; width * height] };
    let mut random = Random(0x5eed);
    let (w, h) = (PICTURE.0 as f32, PICTURE.1 as f32);
    // Lush and dry blades' foot and tip colours: near grey, as the ground's hue colours them, the foot in the shade of
    // the blades about it.
    let lush = (Vec3::new(0.2, 0.22, 0.17), Vec3::new(0.62, 0.66, 0.55));
    let straw = (Vec3::new(0.27, 0.25, 0.2), Vec3::new(0.76, 0.71, 0.56));
    for picture in 0..COLUMNS * ROWS {
        let corner = ((picture % COLUMNS) * PICTURE.0, (picture / COLUMNS) * PICTURE.1);
        let (count, dry, broad, droops) = match picture {
            0..=7 => (240, 0.06, 0.0, 0.3),
            8..=11 => (180, 0.1, 0.25, 0.55),
            12 | 13 => (180, 0.8, 0.0, 0.45),
            _ => (140, 0.2, 0.0, 0.3),
        };
        for _ in 0..count {
            // Rooted across the card, the outer blades shorter and leaning out; many short blades under a few tall.
            let out = random.next() * 2.0 - 1.0;
            let root = w * 0.5 + w * 0.42 * out;
            let tall = h * (0.18 + 0.8 * random.next().powf(1.7)) * (1.0 - 0.4 * out * out);
            let base = if random.next() < broad { 5.0 + 2.5 * random.next() } else { 2.0 + 2.0 * random.next() };
            let droop = if random.next() < droops { 0.2 + 0.8 * random.next() } else { 0.0 };
            let side = if random.next() < 0.5 { -1.0 } else { 1.0 };
            let lean = w * (0.08 * out + 0.05 * (random.next() - 0.5)) + side * droop * tall * 0.5;
            let dryness = if random.next() < dry { 0.6 + 0.4 * random.next() } else { 0.15 * random.next() };
            // Each blade a little lighter or darker, yellower or bluer.
            let shade = 0.75 + 0.35 * random.next();
            let k = random.next() - 0.5;
            let tint = Vec3::new(1.0 + 0.12 * k, 1.0, 1.0 - 0.16 * k) * shade;
            let foot = lush.0.lerp(straw.0, dryness) * tint;
            let tip = lush.1.lerp(straw.1, dryness) * tint;
            canvas.blade(corner, root, tall, lean, base, droop, foot, tip);
        }
        if picture >= 14 {
            // Seed heads on thin stalks over the grass.
            for _ in 0..5 {
                let root = w * (0.15 + 0.7 * random.next());
                let tall = h * (0.78 + 0.18 * random.next());
                let lean = w * 0.06 * (random.next() - 0.5);
                canvas.blade(corner, root, tall, lean, 1.4, 0.0, straw.0, straw.1 * 0.9);
                let tip = Vec2::new(root + lean, h - 1.0 - tall + 8.0);
                canvas.seeds(corner, tip, 8.0, 2.2, Vec3::new(0.62, 0.55, 0.36));
            }
        }
    }
    let Canvas { colour, alpha, .. } = canvas;
    // The cut texels take the blades' mean colour, so filtering darkens no edge.
    let (sum, weight) =
        colour.iter().zip(&alpha).fold((Vec3::ZERO, 0.0), |(sum, weight), (c, a)| (sum + *c * *a, weight + a));
    let mean = sum / weight.max(1.0);
    let pixels: Vec<u8> = colour
        .iter()
        .zip(&alpha)
        .flat_map(|(c, &a)| {
            let c = c.lerp(mean, (1.0 - a).clamp(0.0, 1.0));
            [c.x, c.y, c.z, a].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        })
        .collect();
    let bytes = dds::with_mips(&dds::rgba8_file(width as u32, height as u32, &pixels)).expect("an RGBA file");
    (keep_coverage(bytes, width, height), mean)
}

/// `bytes` (an RGBA `.dds` of `width` x `height` with its mip chain) with each lower level's alpha scaled so that as
/// many of its texels pass the cut-out test as of the top level's.
fn keep_coverage(mut bytes: Vec<u8>, width: usize, height: usize) -> Vec<u8> {
    let sizes: Vec<usize> = (0..)
        .map(|l| (width >> l).max(1) * (height >> l).max(1) * 4)
        .take_while({
            let mut done = false;
            move |&s| !std::mem::replace(&mut done, s == 4)
        })
        .collect();
    let mut at = bytes.len() - sizes.iter().sum::<usize>();
    let passing = |level: &[u8], scale: f32| {
        let cut = f32::from(OBJECT_ALPHA_REF);
        level.chunks_exact(4).filter(|t| f32::from(t[3]) * scale > cut).count() as f32 / (level.len() / 4) as f32
    };
    let top = passing(&bytes[at..at + sizes[0]], 1.0);
    for &size in &sizes {
        let level = &mut bytes[at..at + size];
        let (mut low, mut high) = (0.1f32, 8.0f32);
        for _ in 0..20 {
            let mid = (low + high) * 0.5;
            if passing(level, mid) < top {
                low = mid;
            } else {
                high = mid;
            }
        }
        for texel in level.chunks_exact_mut(4) {
            texel[3] = (f32::from(texel[3]) * high).min(255.0) as u8;
        }
        at += size;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two neighbouring triangles of road mesh: collision labels may cover grassy shoulders too.
    pub(super) fn field(paint: impl Fn(f32, f32) -> [u8; 3]) -> Field {
        let mut rgba = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                let [r, g, b] = paint(x as f32 / 64.0, y as f32 / 64.0);
                rgba.extend([r, g, b, 255]);
            }
        }
        let sharp = Texels { width: 64, height: 64, rgba };
        let (colour, patches) = (sharp.blurred(BLUR), sharp.blurred(PATCH_BLUR));
        let triangle = |p: [Vec3; 3]| Ground {
            uv0: p.map(|p| p.xz() / 4.0),
            uv1: [Vec2::ZERO; 3],
            alpha: [1.0; 3],
            hue: [Vec3::ONE; 3],
            road: true,
            profile: 0,
            prior: 0.0,
            first: 0,
            second: None,
            normal: Vec3::Y,
            corners: (0, [0; 3]),
            p,
        };
        let [a, b, c, d] = [Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 4.0), Vec3::new(4.0, 0.0, 4.0)];
        Field {
            level: Level::Dense,
            triangles: vec![triangle([a, b, c]), triangle([b, d, c])],
            tiles: HashMap::from([((0, 0), vec![0, 1])]),
            textures: vec![colour],
            patches: vec![patches],
            placement: vec![sharp],
            line: Line::new(&[a, d]),
            mean: Vec3::splat(0.5),
            fx: Vec::new(),
            report: Vec::new(),
            texture_refs: Vec::new(),
        }
    }

    #[test]
    fn a_converted_stages_grass_takes_ground_by_luminance_and_pieces_by_weight() {
        use rbr_btb::track::{GrassFx, GrassGroup, GrassPiece};
        let mut settings = GrassFx {
            texture: None,
            grid: [8, 3],
            base: 1.0,
            size: 6.5,
            groups: vec![GrassGroup {
                chance: 1.0,
                pieces: vec![GrassPiece { cell: [4, 1], size: [2, 2], chance: 1.0, mult: [0.6, 1.5] }],
            }],
            luminance: [0.005, 0.25],
            mask: Some([0.3, 0.01]),
            cover: None,
            profiles: Vec::new(),
        };
        settings.profiles = vec![GrassFx { base: 0.0, mask: Some([-0.05, 0.01]), ..settings.clone() }];
        let profiles = Fx::profiles(&settings, Some(Vec2::new(2048.0, 768.0)), Arc::default(), None);
        let fx = &profiles[0];
        // A profile takes its own mask and weights: Joux Verte's configuration grows grass on dull ground too.
        let dull = Vec3::new(0.46, 0.47, 0.42);
        assert_eq!((profiles.len(), fx.mask(dull, dull), profiles[1].mask(dull, dull)), (2, 0.0, 1.0));
        assert!(profiles[1].piece(0.0).is_some_and(|(_, _, piece)| piece));
        // Green enough and in range takes grass; grey asphalt, too bright, black or yellow-brown dirt does not.
        let (grass, grey, dirt) = (Vec3::new(0.4, 0.5, 0.2), Vec3::splat(0.5), Vec3::new(0.57, 0.51, 0.38));
        let mask = |c: Vec3| fx.mask(c, c);
        assert_eq!([grass, grey, Vec3::splat(0.9), Vec3::ZERO, dirt].map(mask), [1.0, 0.0, 0.0, 0.0, 0.0]);
        // Roure's dry verge passes its threshold; Sa Calobra's its own.
        assert!(Fx::green(Vec3::new(0.20, 0.20, 0.15), 0.01) > 0.3);
        assert!(Fx::green(Vec3::new(0.15, 0.14, 0.12), 0.02) > 0.0001);
        // The base's weight is the top row's cells, the group's (at [`FX_PIECE_SHARE`]) its piece.
        let (rect, cells, piece) = fx.piece(0.0).unwrap();
        assert!(rect[0] < 1.0 / 8.0 && rect[3] < 1.0 / 3.0 && cells == [1.0, 1.0] && !piece);
        let (rect, cells, piece) = fx.piece(0.9).unwrap();
        assert!(rect[0] > 3.0 / 8.0 && rect[1] < 5.0 / 8.0 && rect[2] > 0.3 && cells == [0.6, 1.5] && piece);
        assert!((fx.cell() - CELL_PER_SIZE * 6.5f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn a_compressed_atlas_keeps_its_colours_and_its_coverage() {
        // Left half: CSP's green marker blades; right half: a straw blade, which keeps its colour and is no part of
        // the mean.
        let rgba: Vec<u8> =
            (0..16 * 16).flat_map(|i| if i % 16 < 8 { [0, 200, 0, 255] } else { [180, 160, 90, 255] }).collect();
        let file = crate::upscale::encode(dds::Format::Dxt5, 16, 16, rgba);
        let (bytes, mean, size) = track_atlas(&file).unwrap();
        let file = dds::parse(&bytes).unwrap();
        let level = file.levels()[0];
        let texels = crate::upscale::decompress(file.format, level.data, level.width, level.height);
        let (left, right) = (&texels[4 * 16 * 8..][..4], &texels[(16 * 8 + 12) * 4..][..4]);
        assert!(left[0] < 10 && left[1] > 180 && left[2] < 10 && right[0] > 160 && right[2] < 110);
        assert!((mean.x - 200.0 / 255.0).abs() < 0.03 && size == Vec2::new(16.0, 16.0));
    }

    #[test]
    fn occluders_hide_the_ground_under_them_and_by_alpha_under_their_opaque_texels() {
        let line = Line::new(&[Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0)]);
        // A 1 m square of two triangles at x 2-3, z 0-1, its texture's left half opaque.
        let square = |texture| {
            let ([a, b, c, d], uv) = (
                [[2.0, 0.0], [3.0, 0.0], [3.0, 1.0], [2.0, 1.0]].map(|[x, z]| Vec3::new(x, 0.5, z)),
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            );
            [
                GrassOccluder { p: [a, b, c], uv: [uv[0], uv[1], uv[2]], texture },
                GrassOccluder { p: [a, c, d], uv: [uv[0], uv[2], uv[3]], texture },
            ]
        };
        let half = || {
            let rgba = (0..4).flat_map(|x| [0, 0, 0, if x < 2 { 255 } else { 0 }]).collect();
            Some(Texels { width: 4, height: 1, rgba })
        };
        let solid = hidden(&square(None), &line, |_| None);
        assert_eq!(solid.len(), 16);
        assert!(solid.contains(&hide_cell(Vec2::new(2.1, 0.1))) && !solid.contains(&hide_cell(Vec2::new(1.9, 0.1))));
        let alpha = hidden(&square(Some(TextureRef { set: TextureSet::Stage, index: 0 })), &line, |_| half());
        assert_eq!(alpha.len(), 8);
        assert!(alpha.contains(&hide_cell(Vec2::new(2.1, 0.5))) && !alpha.contains(&hide_cell(Vec2::new(2.9, 0.5))));
    }

    #[test]
    fn an_atlas_piece_counts_its_rows_from_the_top_from_0() {
        // Sa Calobra's atlas (2048 by 768, 8 by 3 cells of 256): the base grass is the top row (row 0), the daffodil
        // tuft `1,1` the middle row's first cell, the flowered clump `1,2,2,1` the bottom row's first two cells, the
        // heather `3,1,1,2` column 3's lower two rows and the red flowers `8,2` the bottom row's last cell.
        let texels = Vec2::new(2048.0, 768.0);
        let pixels = |r: [f32; 4]| [r[0] * 2048.0, r[1] * 2048.0, r[2] * 768.0, r[3] * 768.0];
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3);
        assert!(near(pixels(piece_rect([8, 3], [1, 0], [1, 1], texels)), [2.0, 254.0, 2.0, 254.0]));
        assert!(near(pixels(piece_rect([8, 3], [1, 1], [1, 1], texels)), [2.0, 254.0, 258.0, 510.0]));
        assert!(near(pixels(piece_rect([8, 3], [1, 2], [2, 1], texels)), [2.0, 510.0, 514.0, 766.0]));
        assert!(near(pixels(piece_rect([8, 3], [3, 1], [1, 2], texels)), [514.0, 766.0, 258.0, 766.0]));
        assert!(near(pixels(piece_rect([8, 3], [8, 2], [1, 1], texels)), [1794.0, 2046.0, 514.0, 766.0]));
    }

    #[test]
    fn vegetation_takes_straw_and_green_but_not_soil_gravel_or_shade() {
        // RSF Gabiria's meadow and Bareyo's grass (texture means), a lawn; Bareyo's soil, gravel, black.
        for c in [[0.62, 0.55, 0.41], [0.44, 0.38, 0.28], [0.36, 0.39, 0.19]] {
            assert!(vegetation(Vec3::from(c)) > 0.5, "{c:?}");
        }
        for c in [[0.65, 0.53, 0.42], [0.53, 0.51, 0.51], [0.05, 0.06, 0.03]] {
            assert!(vegetation(Vec3::from(c)) < 0.1, "{c:?}");
        }
    }

    #[test]
    fn the_prior_takes_the_collision_and_the_name() {
        let on = |class: usize| {
            let mut e = Evidence { area: 10.0, ..Evidence::default() };
            e.surfaces[class] = 10.0;
            e
        };
        // A hashed name on grass collision is grass; a grass name on tarmac is half; a road name on tarmac is not.
        assert_eq!(texture_prior(&on(0), "bdf34259.dds"), 1.0);
        assert_eq!(texture_prior(&on(4), "Herbe1_T.dds"), 0.5);
        assert_eq!(texture_prior(&on(4), "Route_O.dds"), 0.0);
        // A triangle takes its own collision half: Helette's grass texture on a rock's collision is still likely grass.
        assert!(prior(5, Some(texture_prior(&on(0), "Herbe1_T.dds"))) > 0.7);
        assert_eq!(prior(2, None), 0.0);
    }

    #[test]
    fn a_wide_blur_wraps_a_small_texture() {
        let t = Texels { width: 4, height: 4, rgba: (0..64).map(|i| (i * 4) as u8).collect() };
        assert_eq!(t.blurred(PATCH_BLUR).rgba.len(), 64);
    }

    #[test]
    fn the_texture_keeps_its_coverage_down_the_mips() {
        let (bytes, mean) = texture();
        let dds = dds::parse(&bytes).unwrap();
        assert_eq!((dds.width as usize, dds.height as usize), (PICTURE.0 * COLUMNS, PICTURE.1 * ROWS));
        let cover = |data: &[u8]| {
            data.chunks_exact(4).filter(|t| t[3] > OBJECT_ALPHA_REF).count() as f32 / (data.len() / 4) as f32
        };
        let levels = dds.levels();
        let top = cover(levels[0].data);
        assert!((0.05..0.7).contains(&top), "{top}");
        for level in &levels[1..6] {
            assert!((cover(level.data) - top).abs() < 0.03, "{} {}", level.width, cover(level.data));
        }
        assert!(mean.max_element() - mean.min_element() < 0.12, "near grey: {mean}");
    }

    #[test]
    fn grass_grows_on_green_and_straw_ground_only() {
        // Green, then a straw meadow (Gabiria-Legazpi's), earth, grey stone and pale gravel.
        assert_eq!(green(Vec3::new(0.4, 0.5, 0.2), true), 1.0);
        assert!(green(Vec3::new(0.57, 0.53, 0.37), false) > 0.9);
        assert_eq!(green(Vec3::new(0.5, 0.4, 0.3), false), 0.0);
        assert_eq!(green(Vec3::splat(0.5), false), 0.0);
        assert_eq!(green(Vec3::new(0.6, 0.57, 0.5), true), 0.0);
    }

    #[test]
    fn density_is_full_to_its_level_then_thins_as_its_cards_widen() {
        let dense = |d| density(Level::Dense.profile(false), d);
        assert_eq!((dense(0.0), dense(40.0)), (1.0, 1.0));
        assert!((dense(80.0) - 0.5).abs() < 1e-6, "{}", dense(80.0));
        assert!(dense(90.0) > dense(100.0) && dense(100.0) > dense(200.0));
        assert_eq!((dense(300.0), dense(340.0)), (0.0, 0.0));
        let light = |d| density(Level::Light.profile(false), d);
        assert_eq!((light(0.0), light(20.0), light(120.0)), (0.5, 0.5, 0.0));
        assert!((light(40.0) - 0.25).abs() < 1e-6, "{}", light(40.0));
        // A converted stage's stays full farther out.
        assert_eq!(density(Level::Dense.profile(true), 40.0 * FX_FULL), 1.0);
        // The field covers the ground as near while its cards widen (to 5 x 40 m and 2 x 20 m) and before it fades,
        // less past that.
        for profile in [Level::Light, Level::Dense].into_iter().flat_map(|l| [l.profile(false), l.profile(true)]) {
            let [share, full, most, reach] = profile;
            let cover = |d| density(profile, d) * widen(profile, d) / share;
            for d in [0.0, full, full * 2.0, (full * most).min(FADE * reach)] {
                assert!((cover(d) - 1.0).abs() < 1e-5, "{profile:?} {d} {}", cover(d));
            }
            assert!(cover(full * most + 10.0) < 1.0 && cover(reach) == 0.0);
        }
    }

    #[test]
    fn tiles_behind_the_eye_count_farther() {
        let forward = Vec2::new(0.0, 1.0);
        assert_eq!(seen(Vec2::new(3.0, 4.0), forward), 5.0);
        assert_eq!(seen(Vec2::new(0.0, -10.0), forward), 10.0 * BEHIND);
        assert_eq!(seen(Vec2::new(10.0, 0.0), forward), 10.0);
        // With no view, as far as it is.
        assert_eq!(seen(Vec2::new(0.0, -10.0), Vec2::ZERO), 10.0);
    }

    #[test]
    fn patches_stay_in_range() {
        for k in 0..500 {
            let p = Vec2::new(k as f32 * 0.37 - 90.0, k as f32 * 1.13 - 300.0);
            let (tall, dry) = patches(p);
            assert!((0.55..=1.5).contains(&tall) && (0.0..=1.0).contains(&dry), "{tall} {dry}");
        }
    }
}
