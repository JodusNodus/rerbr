//! Steady forces of a probed wheel under a profile's tyre model, as the game asks it: the model built by
//! `rbr_tyre::model` from the wheel's tables and asked at each slip until its force dynamics settle; where it gives
//! `None` (NGP's: `ngp`, or a surface the model hands back) NGP's own `patch_forces` answer, as in the game's seam.
//! Flat road, no sinking, no soil force. The inclination (`camber`, its sine in the wheel's frame, NGP's sign) reaches
//! both as the tyre step passes it: NGP's patch takes it off the lateral slip, and its table scale
//! (`ngp::inclination::grip_scale`) scales NGP's cornering friction table and the model's grip. Shared by the `scorecard`,
//! `envelope` and `camber_pressure` examples.

use rbr_physics::ngp::inclination;
use rbr_physics::tyre::patch_forces::{patch_forces, PatchForces};
use rbr_tyre::{ForceInput, Model};

use crate::ngp::{blend_at, patch_input};
use crate::probe::{patch_at, Params};

const DT: f32 = 1.0 / 720.0;
/// Steps the model's force dynamics settle at each slip (2 s).
const SETTLE: usize = 1440;

/// The tyre under test: the probed wheel and the profile's model.
pub struct Tyre<'a> {
    pub p: &'a Params,
    pub cloud: &'a [(f32, f32)],
    pub soil: f32,
    pub model: Option<Box<dyn Model>>,
    pub speed: f32,
    pub water: f32,
    /// The sine of the inclination (0: upright).
    pub camber: f32,
}

impl Tyre<'_> {
    /// NGP's patch forces and the model's input at `fz` for the contact velocities and spin (NGP's signs).
    fn inputs(&self, fz: f32, vlat: f32, vlong: f32, spin: f32) -> (PatchForces, ForceInput) {
        let p = self.p;
        let patch = patch_at(self.cloud, fz, p.patch_length);
        let (k, _) = inclination::grip_scale(inclination::degrees(self.camber));
        let mut input = patch_input(p, &blend_at(p, fz), fz, patch, vlat, vlong, spin);
        input.inclination_sin = self.camber;
        // The tyre step scales the cornering friction table only.
        let lateral = &mut input.tables[0];
        (lateral.at_a, lateral.at_b) = (lateral.at_a * k, lateral.at_b * k);
        let x = ForceInput {
            dt: DT,
            touching: true,
            speed: (vlat * vlat + vlong * vlong).sqrt(),
            lateral_velocity: vlat,
            longitudinal_velocity: vlong,
            spin,
            effective_radius: p.effective_radius,
            normal_force: fz,
            patch_length: patch,
            grip: p.static_friction * k,
            stiffness_scale: 1.0,
            stiffness_factor: p.stiffness_factor,
            tread_temperature: p.tread_temp,
            tread_left: 1.0 - p.wear_op,
            soil_thickness: self.soil,
            water_factor: self.water,
            inclination_sin: self.camber,
            lateral_stiffness: p.carcass_lat * p.stiffness_factor,
            torsional_stiffness: p.carcass_tor * p.stiffness_factor,
        };
        (patch_forces(&input), x)
    }

    /// Steady (Fx, Fy) at `fz` for the contact velocities and spin (NGP's signs).
    pub fn forces(&mut self, fz: f32, vlat: f32, vlong: f32, spin: f32) -> (f32, f32) {
        let (ngp, x) = self.inputs(fz, vlat, vlong, spin);
        let Some(model) = self.model.as_mut() else { return (ngp.longitudinal_force, ngp.lateral_force) };
        model.restart();
        let mut last = None;
        for _ in 0..SETTLE {
            last = model.forces(0, &x);
            if last.is_none() {
                break;
            }
        }
        match last {
            Some(o) => {
                let mix = |n: f32, m: f32| n + (m - n) * o.weight;
                (mix(ngp.longitudinal_force, o.longitudinal_force), mix(ngp.lateral_force, o.lateral_force))
            }
            None => (ngp.longitudinal_force, ngp.lateral_force),
        }
    }

    /// The distance rolled, m, to 63 % of the steady side force after a step to the slip angle `deg` from rest
    /// (free rolling): the lateral relaxation length's measure. NGP's sliding model grows the deflection by the lateral
    /// velocity times dt until the patch force clamps it, so its force ramps linearly; a model is stepped from its
    /// restart.
    pub fn x63_y(&mut self, fz: f32, deg: f32) -> f32 {
        let (v, a) = (self.speed, deg.to_radians());
        let (vlat, vlong) = (v * a.sin(), -v * a.cos());
        let (ngp, x) = self.inputs(fz, vlat, vlong, -vlong / self.p.effective_radius);
        let Some(model) = self.model.as_mut() else {
            return 0.63 * ngp.lateral_force.abs() / (x.lateral_stiffness * vlat.abs()) * v;
        };
        model.restart();
        let mut fy = Vec::with_capacity(SETTLE);
        for _ in 0..SETTLE {
            match model.forces(0, &x) {
                Some(o) => fy.push(o.lateral_force),
                None => return f32::NAN,
            }
        }
        // The crossing, linear between the steps (the force after step i at (i + 1) dt, 0 at the start).
        let end = 0.63 * fy[SETTLE - 1].abs();
        let n = fy.iter().position(|f| f.abs() >= end).unwrap_or(SETTLE - 1);
        let before = if n > 0 { fy[n - 1].abs() } else { 0.0 };
        let frac = (end - before) / (fy[n].abs() - before);
        (n as f32 + frac) * DT * v
    }

    /// |Fy|/Fz free rolling at the slip angle `deg`.
    pub fn mu_y(&mut self, fz: f32, deg: f32) -> f32 {
        let (v, a) = (self.speed, deg.to_radians());
        let vlong = -v * a.cos();
        self.forces(fz, v * a.sin(), vlong, -vlong / self.p.effective_radius).1.abs() / fz
    }

    /// (|Fx|, |Fy|)/Fz locked (no spin) at the slip angle `deg`.
    pub fn locked(&mut self, fz: f32, deg: f32) -> (f32, f32) {
        let (v, a) = (self.speed, deg.to_radians());
        let (fx, fy) = self.forces(fz, v * a.sin(), -v * a.cos(), 0.0);
        (fx.abs() / fz, fy.abs() / fz)
    }

    /// |Fx|/Fz straight ahead at the slip ratio `kappa` (negative braking, -1 locked).
    pub fn mu_x(&mut self, fz: f32, kappa: f32) -> f32 {
        let v = self.speed;
        self.forces(fz, 0.0, -v, (1.0 + kappa) * v / self.p.effective_radius).0.abs() / fz
    }

    /// |Fy(κ)|/|Fy(0)| at the slip angle `deg`, the contact speed held (combined slip).
    pub fn combined(&mut self, fz: f32, deg: f32, kappa: f32) -> f32 {
        let (v, a) = (self.speed, deg.to_radians());
        let (vlat, vlong) = (v * a.sin(), -v * a.cos());
        let r = self.p.effective_radius;
        let fy = |t: &mut Self, k: f32| t.forces(fz, vlat, vlong, (1.0 + k) * (-vlong) / r).1.abs();
        fy(self, kappa) / fy(self, 0.0)
    }
    /// The cornering stiffness over the load, per rad: the least-squares slope of Fy/Fz over 0.25-2° (as the
    /// scorecard's `calpha_per_fz`).
    pub fn calpha_per_fz(&mut self, fz: f32) -> f32 {
        let angles: Vec<f32> = grid(0.25, 2.0, 0.25).map(f32::to_radians).collect();
        slope(&angles, |a| self.mu_y(fz, a.to_degrees()))
    }

    /// The longitudinal slip stiffness over the load: the slope of |Fx|/Fz at κ ±0.02 and ±0.04, driving and braking
    /// averaged (as the scorecard's `kx_per_fz`).
    pub fn kx_per_fz(&mut self, fz: f32) -> f32 {
        slope(&[0.02, 0.04], |k| (self.mu_x(fz, k) + self.mu_x(fz, -k)) / 2.0)
    }

    /// The camber stiffness over the load, per rad of the inclination's sine: the change of Fy/Fz between the
    /// inclinations ±1° at a slip angle of 1° (both inside the linear range; NGP's in-game sliding model holds no
    /// force where its offset slip and the sliding disagree, so 0° is not used). Positive: NGP's direction, thrust
    /// towards the top's lean (inwards for negative camber).
    pub fn camber_per_fz(&mut self, fz: f32) -> f32 {
        let keep = self.camber;
        let s = 1.0f32.to_radians().sin();
        let (v, a) = (self.speed, 1.0f32.to_radians());
        let (vlat, vlong) = (v * a.sin(), -v * a.cos());
        let spin = -vlong / self.p.effective_radius;
        let fy = |t: &mut Self, c: f32| {
            t.camber = c;
            t.forces(fz, vlat, vlong, spin).1
        };
        let k = (fy(self, s) - fy(self, -s)) / (2.0 * s) / fz;
        self.camber = keep;
        k
    }
}

/// Least-squares slope through the origin of `f` over `xs`.
pub fn slope(xs: &[f32], mut f: impl FnMut(f32) -> f32) -> f32 {
    let (sxy, sxx) = xs.iter().fold((0.0, 0.0), |(sxy, sxx), &x| (sxy + x * f(x), sxx + x * x));
    sxy / sxx
}

/// The grid's peak (value, at) of `f` over `xs`.
pub fn peak(xs: impl Iterator<Item = f32>, mut f: impl FnMut(f32) -> f32) -> (f32, f32) {
    xs.map(|x| (f(x), x)).fold((0.0, 0.0), |best, (y, x)| if y > best.0 { (y, x) } else { best })
}

/// `from` to `to` in steps of `step`.
pub fn grid(from: f32, to: f32, step: f32) -> impl Iterator<Item = f32> {
    let n = ((to - from) / step).round() as i32;
    (0..=n).map(move |k| from + k as f32 * step)
}
