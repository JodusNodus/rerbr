//! The glass as the viewer draws it: its panes, drops and film ([`Drawn`]).
use glam::{Mat4, Vec2, Vec3};
use rbr_formats::data::Data;

use super::model::{self, Conditions, Glass, Kind, Switch, Wiper, Wiping};
use super::mud::{hidden, pane_cells, through_at};
use super::{film_corners, gpu, CarGlass, Mud, Tints, Windscreen, WINDSCREEN};
use super::{STEP, WARM_UP};
use crate::dirt::Spray;
use crate::particles::Precipitation;
use crate::playback::Playback;
use crate::renderer::Renderer;

/// The car's acceleration the water feels (ours), smoothed over this many seconds (a frame's change of velocity
/// jitters over every bump) and at most [`FELT_MOST`] m/s² (a restart's jump is no crash).
const FELT_SMOOTHING: f32 = 0.25;
const FELT_MOST: f32 = 30.0;
/// Ours: drops sit this far outside the glass's surface, metres.
const LIFT: f32 = 0.001;
/// Ours: a sliding drop is drawn this much longer along its run per m/s.
const STRETCH: f32 = 6.0;
/// Ours: mud seen from inside shows the side stuck to the glass, in its own shade: its colour times this (the outside
/// view sees the sunlit side), and with the cabin's light the outside's light through its thin rim (`glass.wgsl`,
/// kind 5).
const MUD_INSIDE: f32 = 0.2;
/// Ours: thrown snow lets the light through, so seen from inside it keeps more of its colour.
const SNOW_INSIDE: f32 = 0.75;
/// Ours: a mud speck's quad spans this many times its radius, for the droplets around it (`SPLAT_REACH` in
/// `glass.wgsl`).
const SPLAT_REACH: f32 = 2.6;
/// The glass as the viewer draws it: the car's glass, the model and the drive's weather.
pub struct Drawn {
    car: CarGlass,
    glass: Glass,
    /// The windscreen's, then each side window's pane cells seen from inside, model space ([`pane_cells`]), and what
    /// its glass lets through over its film ([`Tints::film_tints`]).
    panes: Vec<(Vec<gpu::Drop>, Vec<f32>)>,
    precipitation: Option<Precipitation>,
    /// The wiper switch, and the schedule it gives (changed only while the blades are parked).
    pub switch: Switch,
    wiping: Wiping,
    /// The playback's interior parts of each wiper, and its exterior parts of each exterior wiper.
    parts: Vec<Vec<usize>>,
    exterior_parts: Vec<Vec<usize>>,
    /// The side windows' glass, in [`CarGlass::sides`]' order.
    sides: Vec<Glass>,
    /// The last frame followed, and its step.
    last: Option<(usize, u32)>,
    /// The car's acceleration the water feels, physics frame ([`felt_acceleration`]).
    felt: Vec3,
    /// How the mud specks look.
    mud: Mud,
    /// The last frame's drops, kept for their memory.
    instances: Vec<gpu::Drop>,
}

impl Drawn {
    /// The car's windscreen chart.
    pub fn windscreen(&self) -> &Windscreen {
        &self.car.windscreen
    }

    /// The glass of car `name` in `precipitation`, with the wiper meshes found among `playback`'s interior parts and
    /// what its windows' faces let through, `tints`, its mud specks as `mud`. `None` (reported on stderr) when the car
    /// has no windscreen part.
    /// The wipers start at `switch`.
    pub fn load(
        data: &Data,
        name: &str,
        precipitation: Option<Precipitation>,
        playback: &mut Playback,
        switch: Switch,
        tints: &Tints,
        mud: Mud,
    ) -> Option<Drawn> {
        let car = CarGlass::load(data, name).map_err(|e| eprintln!("glass: {e}")).ok()?;
        let [lo, hi] = car.windscreen.bounds;
        let glass = Glass::new([lo, hi], car.windscreen.area, car.wipers.iter().map(|w| w.0).collect(), 0x6c_a55);
        let sides = (car.sides.iter().zip(1..))
            .map(|(side, k)| {
                let w = &side.inside;
                let back = Vec2::new(Vec3::Z.dot(w.axes[0]), Vec3::Z.dot(w.axes[1]));
                Glass::side(w.bounds, w.area, back, 0x6c_a55 + k)
            })
            .collect::<Vec<_>>();
        let pane = |part: &str, glass: &Glass, w: &Windscreen, seed: f32| {
            let tints = tints.film_tints(part, glass, w);
            (pane_cells(glass, w, &film_corners(glass, w), &tints, seed), tints)
        };
        let panes = std::iter::once(pane(WINDSCREEN, &glass, &car.windscreen, 0.0))
            .chain(car.sides.iter().zip(&sides).map(|(side, g)| pane(side.part, g, &side.inside, 1.0)))
            .collect();
        let parts: Vec<Vec<usize>> = car.wipers.iter().map(|(_, centre)| playback.parts_at(*centre, true)).collect();
        let exterior_parts: Vec<Vec<usize>> = car
            .exterior
            .iter()
            .flat_map(|(_, wipers)| wipers)
            .map(|(_, centre)| playback.parts_at(*centre, false))
            .collect();
        let wiping = switch.wiping(falling(precipitation), amounts(precipitation).1, 0);
        eprintln!(
            "glass: windscreen {:.2} m², {} side windows, {} wipers ({} meshes, {} outside), {:?}, wipers {} ({:?})",
            car.windscreen.area,
            car.sides.len(),
            car.wipers.len(),
            parts.iter().map(Vec::len).sum::<usize>(),
            exterior_parts.iter().map(Vec::len).sum::<usize>(),
            precipitation,
            switch.name(),
            wiping
        );
        let felt = Vec3::ZERO;
        Some(Drawn {
            car,
            glass,
            panes,
            precipitation,
            switch,
            wiping,
            parts,
            exterior_parts,
            sides,
            last: None,
            felt,
            mud,
            instances: Vec::new(),
        })
    }

    /// Follows the drive to its current frame (each frame one physics step, with its tyre `spray`), turns the wiper
    /// meshes in `playback` (call before [`Playback::apply`]) and gives `renderer` the drops: on the windscreen inside
    /// for the `interior` views, else on the outside one (ours: the same drops, mud and wiped arcs seen from outside),
    /// or none for a car without one. A frame past the spray's known ones takes the last known frame's.
    pub fn draw(&mut self, renderer: &mut Renderer, playback: &mut Playback, interior: bool, spray: Option<&Spray>) {
        let samples = playback.samples();
        let index = playback.index();
        let first = match self.last {
            Some((last, step)) if last <= index && index - last <= WARM_UP && samples[last].step == step => last + 1,
            _ => {
                self.glass.clear();
                self.felt = Vec3::ZERO;
                let first = index.saturating_sub(WARM_UP);
                let before = spray.map_or_else(Default::default, |s| s.before(first));
                let windscreen = &self.car.windscreen;
                self.glass.spatter_all(before, |at| windscreen.inside(at));
                for (side, glass) in self.car.sides.iter().zip(&mut self.sides) {
                    glass.clear();
                    glass.spatter_all(before, |at| side.inside.inside(at));
                }
                first
            }
        };
        let (rain, snow) = amounts(self.precipitation);
        // The wipers read the raw fall, not the shower's share: drizzle wipes now and then, heavier rain always.
        let falling = falling(self.precipitation);
        // The wipers run on the game's clock: the drive's steps (NGP's 1/720 s in a live NGP run).
        let step = playback.step_clock().step;
        let mut pending: Option<Pending> = None;
        for (i, sample) in samples.iter().enumerate().take(index + 1).skip(first) {
            // The frame's time on the same clock, so that the water keeps pace with the blades.
            let dt = i.checked_sub(1).map_or(STEP, |p| sample.step.saturating_sub(samples[p].step) as f32 * step);
            if let Some(p) = i.checked_sub(1).filter(|_| dt > 0.0) {
                let change = Vec3::from(sample.car.velocity) - Vec3::from(samples[p].car.velocity);
                self.felt = felt_acceleration(self.felt, change, dt);
            }
            let to_model = playback.model_to_physics(sample).inverse();
            let gravity = to_model.transform_vector3(Vec3::new(0.0, 0.0, -9.81) - self.felt);
            let axes = self.car.windscreen.axes;
            let conditions = Conditions {
                rain,
                snow,
                speed: Vec3::from(sample.car.velocity).length(),
                gravity: Vec2::new(gravity.dot(axes[0]), gravity.dot(axes[1])),
                spray: spray.map_or_else(Default::default, |s| s.thrown(i.min(s.len().saturating_sub(1)))),
            };
            let time = sample.step as f32 * step;
            // The switch takes effect at the park, so the blades never jump.
            if self.wiping.phase(time) == 0.0 {
                self.wiping = self.switch.wiping(falling, snow, self.glass.mud());
            }
            let phase = self.wiping.phase(time);
            // A step sweeps one way: it ends where the blades turn.
            if let Some(p) = pending.take_if(|p| p.turns(phase)) {
                self.step_glass(p);
            }
            let p = match pending.take() {
                Some(p) => p.then(dt, conditions, gravity, phase),
                None => Pending { dt, conditions, gravity, phase, way: (phase - self.glass.phase).signum() },
            };
            if p.dt >= GLASS_STEP || i == index {
                self.step_glass(p);
            } else {
                pending = Some(p);
            }
        }
        self.last = Some((index, samples[index].step));
        let phase = self.glass.phase;
        let turn =
            |windscreen: &Windscreen, wipers: &[(Wiper, Vec3)], parts: &[Vec<usize>], playback: &mut Playback| {
                for ((wiper, _), parts) in wipers.iter().zip(parts) {
                    let pivot = windscreen.point(wiper.pivot, 0.0);
                    let motion = Mat4::from_translation(pivot)
                        * Mat4::from_axis_angle(windscreen.turning_axis(wiper), wiper.turn * wiper.sweep * phase)
                        * Mat4::from_translation(-pivot);
                    for &part in parts {
                        playback.set_motion(part, motion);
                    }
                }
            };
        let windscreen = &self.car.windscreen;
        turn(windscreen, &self.car.wipers, &self.parts, playback);
        if let Some((outside, wipers)) = &self.car.exterior {
            turn(outside, wipers, &self.exterior_parts, playback);
        }
        let lit = renderer.enhance().cabin;
        let effects = renderer.enhance().windscreen;
        let mut drops = std::mem::take(&mut self.instances);
        drops.clear();
        match (interior, &self.car.exterior) {
            (true, _) => self.drops(&mut drops, playback, None, lit, effects),
            (false, Some((outside, _))) => self.drops(&mut drops, playback, Some(outside), lit, effects),
            (false, None) => {}
        }
        // The windscreen's wipers: the pane's swept areas (kind 6), and the strokes that cleared the film (kind 4).
        let g = &self.glass;
        let (corner, size) = g.film_cell(0);
        let mut wipers = gpu::Wipers {
            strokes: g.strokes.map(|s| [s.from, s.to, g.clock - s.began, g.clock - s.ended]),
            wiping: [g.stroke_count as f32, g.wetting[0], g.wetting[1], 0.0],
            film: [corner.x - size.x * 0.5, corner.y - size.y * 0.5, size.x, size.y],
            ..Default::default()
        };
        for ((w, _), rows) in self.car.wipers.iter().zip(wipers.wipers.chunks_exact_mut(2)) {
            rows[0] = [w.pivot.x, w.pivot.y, w.reach[0], w.reach[1]];
            rows[1] = [w.park, w.turn * w.sweep, 1.0, 0.0];
        }
        renderer.set_glass(&drops, wipers);
        self.instances = drops;
        // The cabin reflected in the windscreen's pane.
        let to_stage = playback.body();
        renderer.set_windscreen_mirror((interior && effects).then(|| self.car.windscreen.mirror(to_stage)));
    }

    /// Steps the glass, and the side windows', by the physics steps `p`.
    fn step_glass(&mut self, p: Pending) {
        let windscreen = &self.car.windscreen;
        self.glass.step(p.dt, p.conditions, p.phase, |at| windscreen.inside(at));
        for (side, glass) in self.car.sides.iter().zip(&mut self.sides) {
            let [across, up, _] = side.inside.axes;
            let gravity = Vec2::new(p.gravity.dot(across), p.gravity.dot(up));
            glass.step(p.dt, Conditions { gravity, ..p.conditions }, 0.0, |at| side.inside.inside(at));
        }
    }

    /// Adds to `drops` the drops at the current frame, stage space, on the windscreen inside, or on `outside` (the
    /// outside one) at the points over the inside one's. `lit` with [`Enhance::cabin`](crate::renderer::Enhance::cabin)
    /// (`pane_drops`), the panes themselves seen from inside with
    /// [`Enhance::windscreen`](crate::renderer::Enhance::windscreen) (`effects`).
    fn drops(
        &self,
        drops: &mut Vec<gpu::Drop>,
        playback: &Playback,
        outside: Option<&Windscreen>,
        lit: bool,
        effects: bool,
    ) {
        let to_stage = playback.body();
        let w = &self.car.windscreen;
        let place = |at: Vec2, lift: f32| match outside {
            Some(o) => o.point(o.chart(w.point(at, 0.0)), lift),
            None => w.point(at, lift),
        };
        // The film's streaks along the airflow, longer with speed: up the windscreen, back along the side windows.
        let streak = 1.0 + (Vec3::from(playback.sample().car.velocity).length() / FILM_STREAK_SPEED).min(2.0);
        let [across, up, _] = outside.unwrap_or(w).axes.map(|a| to_stage.transform_vector3(a));
        // Seen from inside, the mud behind a sun strip or the black band printed on the glass is hidden.
        let inside = |k: usize| outside.is_none().then(|| &self.panes[k].1[..]);
        pane_drops(drops, &self.glass, w, outside, to_stage, streak, lit, self.mud, inside(0), 0);
        for (k, (side, glass)) in self.car.sides.iter().zip(&self.sides).enumerate() {
            // Outside, on the exterior pane over it, when the car has one.
            let on = match outside {
                Some(_) => side.outside.as_ref().map(Some),
                None => Some(None),
            };
            if let Some(on) = on {
                let pane = k as u32 + 1;
                pane_drops(drops, glass, &side.inside, on, to_stage, -streak, lit, self.mud, inside(k + 1), pane);
            }
        }
        // The snow each blade carries, along its edge at its angle.
        let phase = self.glass.phase;
        for (wiper, &load) in self.glass.wipers.iter().zip(&self.glass.blade_snow) {
            let angle = wiper.park + wiper.turn * wiper.sweep * phase;
            let along = Vec2::new(angle.cos(), angle.sin());
            let ahead = Vec2::new(-along.y, along.x) * wiper.turn;
            for k in 0..load as usize {
                let (a, b) = (hash(k as u32 * 2 + 1), hash(k as u32 * 2 + 2));
                let r = wiper.reach[0] + (wiper.reach[1] - wiper.reach[0]) * a;
                let at = wiper.pivot + along * r + ahead * (b - 0.3) * BLADE_DEPTH;
                let radius = BLADE_FLAKE[0] + (BLADE_FLAKE[1] - BLADE_FLAKE[0]) * b;
                drops.push(gpu::Drop {
                    centre: to_stage.transform_point3(place(at, BLADE_LIFT)).to_array(),
                    kind: 1.0,
                    axis_u: (across * radius).to_array(),
                    seed: a,
                    axis_v: (up * radius).to_array(),
                    alpha: 1.0,
                    color: [0.95, 0.96, 1.0, 1.0],
                    normal: [0.0; 4],
                    far: [0.0; 3],
                    tint: [0; 4],
                    edge: [0; 4],
                });
            }
        }
        // The panes themselves seen from inside, over all that lies on them: their inner face's reflection and loss
        // are nearer the eye than the water on the outer one.
        if outside.is_none() && effects {
            let out = |v: [f32; 3]| to_stage.transform_vector3(Vec3::from(v));
            drops.extend(self.panes.iter().flat_map(|(cells, _)| cells).map(|c| gpu::Drop {
                centre: to_stage.transform_point3(Vec3::from(c.centre)).to_array(),
                axis_u: out(c.axis_u).to_array(),
                axis_v: out(c.axis_v).to_array(),
                far: out(c.far).to_array(),
                normal: out([c.normal[0], c.normal[1], c.normal[2]]).normalize().extend(0.0).to_array(),
                ..*c
            }));
        }
    }
}

/// Adds to `drops` the drops, film and mud of one pane's `glass` at the current frame, stage space (`to_stage` from the
/// car's model), on its chart `w` inside, or on `outside` at the points over `w`'s. `streak` is the film's streak
/// length, up the pane, or along it when negative. `lit`: the mud seen from inside lets the outside's light through
/// (kind 5). The mud looks as `mud`; `tints`, inside, what the pane's glass lets through over its film
/// ([`Tints::film_tints`]), which hides the mud behind its printed bands. `pane` numbers the pane for its beads' layout
/// (0 the windscreen).
#[allow(clippy::too_many_arguments)]
fn pane_drops(
    drops: &mut Vec<gpu::Drop>,
    glass: &Glass,
    w: &Windscreen,
    outside: Option<&Windscreen>,
    to_stage: Mat4,
    streak: f32,
    lit: bool,
    mud: Mud,
    tints: Option<&[f32]>,
    pane: u32,
) {
    let place = |at: Vec2, lift: f32| match outside {
        Some(o) => o.point(o.chart(w.point(at, 0.0)), lift),
        None => w.point(at, lift),
    };
    let [across, up, _] = outside.unwrap_or(w).axes.map(|a| to_stage.transform_vector3(a));
    // The wet film first, under the drops: a patch per cell, its corners' film (the mean of the cells around each
    // corner, none off the glass) in `color`, so that the shader blends them into one smooth sheet that fades out
    // at the glass's edge.
    let [columns, rows] = model::FILM_CELLS;
    let (film, size) = (&glass.film, glass.film_cell(0).1);
    // Each corner's film and its place on the curved glass, so that neighbouring cells meet without seams; none while
    // no cell shows.
    let corner = |(column, row): (usize, usize)| {
        let at = glass.film_corner(column, row);
        let place = to_stage.transform_point3(place(at, LIFT * 0.5));
        if !w.inside(at) {
            return (0.0, place);
        }
        let (mut sum, mut count) = (0.0, 0.0);
        for r in row.saturating_sub(1)..(row + 1).min(rows) {
            for c in column.saturating_sub(1)..(column + 1).min(columns) {
                sum += film[r * columns + c];
                count += 1.0;
            }
        }
        (sum / count, place)
    };
    let shown = film.iter().any(|&f| f > FILM_SHOWN);
    let corners: Vec<(f32, Vec3)> = match shown {
        true => (0..=rows).flat_map(|row| (0..=columns).map(move |column| (column, row))).map(corner).collect(),
        false => Vec::new(),
    };
    let cells = if shown { film.len() } else { 0 };
    for index in 0..cells {
        let (column, row) = (index % columns, index / columns);
        let at = |c: usize, r: usize| corners[(row + r) * (columns + 1) + column + c];
        let [low, right, top, far] = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
        let color = [low.0, right.0, top.0, far.0];
        if color.iter().all(|&f| f <= FILM_SHOWN) {
            continue;
        }
        let [low, right, top, far] = [low.1, right.1, top.1, far.1];
        drops.push(gpu::Drop {
            centre: ((low + right + top + far) * 0.25).to_array(),
            kind: 4.0,
            axis_u: ((right + far - low - top) * 0.25).to_array(),
            seed: streak,
            axis_v: ((top + far - low - right) * 0.25).to_array(),
            // The cell, and the pane's cell size, landing clock and number, for its beads (`glass.wgsl`).
            alpha: index as f32,
            color,
            normal: [size.x, size.y, glass.landed, pane as f32],
            far: [0.0; 3],
            tint: [0; 4],
            edge: [0; 4],
        });
    }
    drops.extend(glass.drops.iter().map(|d| {
        let centre = to_stage.transform_point3(place(d.at, LIFT));
        let run = d.velocity.length();
        // A ribbon: its middle, half as long as its span (and its ends' roundness more), as wide as its radius.
        if d.kind == Kind::Trail && d.span != Vec2::ZERO {
            let centre = to_stage.transform_point3(place(d.at + d.span * 0.5, LIFT));
            let to = |c: Vec2| across * c.x + up * c.y;
            let along = d.span.normalize_or_zero();
            return gpu::Drop {
                centre: centre.to_array(),
                kind: 7.0,
                axis_u: (to(Vec2::new(-along.y, along.x)) * d.radius).to_array(),
                seed: d.seed,
                axis_v: (to(along) * (d.span.length() * 0.5 + d.radius)).to_array(),
                alpha: d.left.min(1.0),
                color: [1.0; 4],
                normal: [0.0; 4],
                far: [0.0; 3],
                tint: [0; 4],
                edge: [0; 4],
            };
        }
        let (u, v) = if run > 0.0 {
            let along = d.velocity / run;
            let side = Vec2::new(-along.y, along.x);
            let to = |c: Vec2| across * c.x + up * c.y;
            (to(side) * d.radius, to(along) * d.radius * (1.0 + run * STRETCH))
        } else if d.kind == Kind::Mud {
            // The speck's droplets around it (`splat` in `glass.wgsl`).
            let reach = d.radius * SPLAT_REACH * if model::is_snow(d.left) { 1.0 } else { mud.size };
            (across * reach, up * reach)
        } else {
            (across * d.radius, up * d.radius)
        };
        let (kind, alpha, color) = match d.kind {
            Kind::Water => (0.0, 1.0, [1.0; 4]),
            Kind::Trail => (0.0, d.left.min(1.0), [1.0; 4]),
            Kind::Mud => {
                let (kind, shade) = match (outside, model::is_snow(d.left)) {
                    (Some(_), _) => (3.0, 1.0),
                    (None, false) => (if lit { 5.0 } else { 3.0 }, MUD_INSIDE),
                    (None, true) => (3.0, SNOW_INSIDE),
                };
                let [r, g, b] = mud.color(d.left).map(|c| c * shade);
                (kind, tints.map_or(1.0, |t| hidden(through_at(glass, t, d.at))), [r, g, b, 1.0])
            }
            Kind::Snow => (1.0, 0.5 + 0.5 * d.left, [0.95, 0.96, 1.0, 1.0]),
            Kind::Bug => {
                let fade = ((model::BUG_LIFE + model::BUG_FADE - d.age) / model::BUG_FADE).clamp(0.0, 1.0);
                let [r, g, b] = model::BUG_COLORS[d.left as usize % 4];
                (2.0, fade, [r, g, b, 1.0])
            }
        };
        gpu::Drop {
            centre: centre.to_array(),
            kind,
            axis_u: u.to_array(),
            seed: d.seed,
            axis_v: v.to_array(),
            alpha,
            color,
            normal: [0.0; 4],
            far: [0.0; 3],
            tint: [0; 4],
            edge: [0; 4],
        }
    }));
}
/// The film below this is not drawn.
const FILM_SHOWN: f32 = 0.02;
/// The least time the glass steps at once (ours), for a frame shown over many of the drive's (a seek's warm-up, a
/// catch-up): a step each took 0.3 ms in heavy rain. A step ends early where the wipers turn and at the frame shown.
const GLASS_STEP: f32 = 1.0 / 60.0;

/// Physics steps the glass takes as one ([`Drawn::draw`]): their time, the last one's conditions with all their spray,
/// its gravity (model space), and the wipers' angle fraction after it and the way they went.
#[derive(Debug, Clone, Copy)]
struct Pending {
    dt: f32,
    conditions: Conditions,
    gravity: Vec3,
    phase: f32,
    way: f32,
}

impl Pending {
    /// Whether the wipers, at `phase` next, turn after these steps.
    fn turns(&self, phase: f32) -> bool {
        let way = (phase - self.phase).signum();
        phase != self.phase && self.way != 0.0 && way != self.way
    }

    /// These steps and one more of `dt` seconds.
    fn then(self, dt: f32, conditions: Conditions, gravity: Vec3, phase: f32) -> Pending {
        let way = if phase == self.phase { self.way } else { (phase - self.phase).signum() };
        let spray = self.conditions.spray.plus(conditions.spray);
        Pending { dt: self.dt + dt, conditions: Conditions { spray, ..conditions }, gravity, phase, way }
    }
}

/// Ours: the film's streaks grow by their length per this many m/s, up to three times it.
const FILM_STREAK_SPEED: f32 = 12.0;
/// Snow on a blade (ours): the flakes' radius, metres, how far they spread across the blade, and their height over
/// the glass (on top of the blade).
const BLADE_FLAKE: [f32; 2] = [0.004, 0.008];
const BLADE_DEPTH: f32 = 0.02;
const BLADE_LIFT: f32 = 0.012;

/// A number in [0, 1) from `k` (a fixed scatter for the blade's flakes).
fn hash(k: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Rain and snow intensity of a precipitation: the rain's shower share, so drizzle wets the glass slowly and
/// heavy rain as it is; snow unchanged ([`Precipitation::shower`]).
fn amounts(p: Option<Precipitation>) -> (f32, f32) {
    match p {
        Some(p) if p.snow => (0.0, p.intensity),
        Some(p) => (p.shower(), 0.0),
        None => (0.0, 0.0),
    }
}

/// The car's acceleration the water feels after a frame of `dt` seconds whose velocity changed by `change` (m/s), from
/// `felt` before: smoothed over [`FELT_SMOOTHING`], at most [`FELT_MOST`].
fn felt_acceleration(felt: Vec3, change: Vec3, dt: f32) -> Vec3 {
    let now = (change / dt).clamp_length_max(FELT_MOST);
    felt + (now - felt) * (1.0 - (-dt / FELT_SMOOTHING).exp())
}

/// The raw rain of a precipitation (no snow), for the wipers' switch: drizzle wipes now and then.
fn falling(p: Option<Precipitation>) -> f32 {
    p.filter(|p| !p.snow).map_or(0.0, |p| p.intensity)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_water_feels_the_car_brake_but_not_a_restart() {
        // Braking at 1 g, a 1/60 s frame at a time: in a second and a half the water feels nearly all of it.
        let dt = 1.0 / 60.0;
        let mut felt = Vec3::ZERO;
        for _ in 0..90 {
            felt = felt_acceleration(felt, Vec3::new(-9.81 * dt, 0.0, 0.0), dt);
        }
        assert!((felt.x + 9.81).abs() < 0.1 && felt.y == 0.0, "{felt}");
        // A restart's jump from 30 m/s to a stand in one frame: a nudge, not 1800 m/s².
        let jump = felt_acceleration(Vec3::ZERO, Vec3::new(-30.0, 0.0, 0.0), dt);
        assert!(jump.length() < FELT_MOST * 0.2, "{jump}");
    }
}
