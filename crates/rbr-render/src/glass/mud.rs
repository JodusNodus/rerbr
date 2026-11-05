//! The mud on the glass: its look ([`Mud`]) and each pane's mud cells ([`pane_cells`]).
use super::model::{self, Glass};
use super::{gpu, Corner, Windscreen};
use glam::Vec2;

/// Ours: a mud speck's size on a dry stage and on a wet one, of its radius (dry dust spatters in fine dots, wet mud in
/// blobs).
const MUD_SIZE: [f32; 2] = [0.7, 1.2];
/// How the mud specks look on a stage (ours).
#[derive(Debug, Clone, Copy)]
pub struct Mud {
    /// The colour of mud's specks against [`model::speck_color`]'s.
    tint: [f32; 3],
    /// Their size against their radius ([`MUD_SIZE`]).
    pub(super) size: f32,
}

impl Mud {
    /// The specks of a stage of `dust` colour and `wetness` (0 dry, 1 wet): the dust against the neutral dust,
    /// darkened with the wetness from the specks' drying mud to the car's wet dirt ([`crate::dirt::COLOUR`] against
    /// mud's mean), as the car's dirt goes ([`crate::dirt::CarDirt::look`]); finer dry, bigger wet.
    pub fn of(dust: [f32; 3], wetness: f32) -> Mud {
        let (neutral, mean) = (crate::particles::wheel_spray::NEUTRAL_DUST, model::mud_mean());
        Mud {
            tint: std::array::from_fn(|k| {
                dust[k] / neutral[k] * (1.0 + (crate::dirt::COLOUR[k] / mean[k] - 1.0) * wetness)
            }),
            size: MUD_SIZE[0] + (MUD_SIZE[1] - MUD_SIZE[0]) * wetness,
        }
    }

    /// A speck's colour by its colour index ([`model::speck_color`]): mud's tinted, snow's as it is.
    pub(super) fn color(self, index: f32) -> [f32; 3] {
        let colour = model::speck_color(index);
        if model::is_snow(index) {
            return colour;
        }
        std::array::from_fn(|k| (colour[k] * self.tint[k]).min(1.0))
    }
}

/// How much of a speck shows inside behind glass that lets `through` through: all of it over the clear glass, none
/// behind the printed bands.
pub(super) fn hidden(through: f32) -> f32 {
    let t = ((through - 0.1) / 0.3).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What a pane's glass lets through at chart point `at`: the nearest of its film's `tints` ([`Tints::film_tints`]).
pub(super) fn through_at(glass: &Glass, tints: &[f32], at: Vec2) -> f32 {
    let [columns, rows] = model::FILM_CELLS.map(|n| n * gpu::PANE_TINTS);
    let (lo, hi) = (glass.film_corner(0, 0), glass.film_corner(model::FILM_CELLS[0], model::FILM_CELLS[1]));
    let t = ((at - lo) / (hi - lo)).clamp(Vec2::ZERO, Vec2::ONE);
    let (column, row) = ((t.x * columns as f32).round() as usize, (t.y * rows as f32).round() as usize);
    tints.get(row * (columns + 1) + column).copied().unwrap_or(1.0)
}

/// The cells of one pane's `glass` (its film cells) on its chart `w`, model space, for the pane seen from inside (kind
/// 6, `glass.wgsl`): each with its middle and size in the chart, metres, in `color`, and from its film `corners`
/// ([`Corner`]) how far inside the outline they lie in `edge` (over [`gpu::PANE_EDGE`]), and `tints`
/// ([`Tints::film_tints`]) what the glass lets through across each in `tint`; `seed` 0 the windscreen, 1 a side window.
/// Its corners as [`gpu::Drop`] says, exact: the glass's corners need not lie in a plane. Only the cells with a corner
/// on the glass. The same each frame but for the car's place, so made once ([`Drawn::load`]).
pub(super) fn pane_cells(
    glass: &Glass,
    w: &Windscreen,
    corners: &[Corner],
    tints: &[f32],
    seed: f32,
) -> Vec<gpu::Drop> {
    let steps = gpu::PANE_TINTS;
    let across = model::FILM_CELLS[0] * steps + 1;
    (0..glass.film.len())
        .filter_map(|index| {
            let (at, size) = glass.film_cell(index);
            // Neighbours take a shared corner's point, so its height, from the same sum.
            let (column, row) = (index % model::FILM_CELLS[0], index / model::FILM_CELLS[0]);
            let cell = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .map(|(i, j)| corners[(row + j) * (model::FILM_CELLS[0] + 1) + column + i]);
            if cell.iter().all(|c| c.edge <= 0.0) {
                return None;
            }
            let [low, right, top, far] = cell.map(|c| c.point);
            Some(gpu::Drop {
                centre: low.to_array(),
                kind: 6.0,
                axis_u: (right - low).to_array(),
                seed,
                axis_v: (top - low).to_array(),
                alpha: 1.0,
                color: [at.x, at.y, size.x, size.y],
                // The chart's mean plane's: the grid's heights step from cell to cell.
                normal: w.axes[2].extend(0.0).to_array(),
                far: (far - low).to_array(),
                tint: (0..(steps + 1) * (steps + 1)).fold([0; 4], |mut tint, k| {
                    let through = tints[(row * steps + k / (steps + 1)) * across + column * steps + k % (steps + 1)];
                    tint[k / 8] |= ((through.clamp(0.0, 1.0) * 15.0).round() as u32) << (4 * (k % 8));
                    tint
                }),
                edge: cell.map(|c| ((c.edge / gpu::PANE_EDGE).clamp(-1.0, 1.0) * 127.0).round() as i8),
            })
        })
        .collect()
}
