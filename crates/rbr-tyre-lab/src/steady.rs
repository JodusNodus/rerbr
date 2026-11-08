//! Steady forces of a probed wheel under a profile's tyre model, as the game asks it: the model built by
//! `rbr_tyre::model` from the wheel's tables and asked at each slip until its force dynamics settle; where it gives
//! `None` (NGP's: `ngp`, or a surface the model hands back) NGP's own `patch_forces` answer, as in the game's seam.
//! Flat road, no sinking, no soil force. Shared by the `scorecard` and `envelope` examples.

use rbr_physics::tyre::patch_forces::patch_forces;
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
}

impl Tyre<'_> {
    /// Steady (Fx, Fy) at `fz` for the contact velocities and spin (NGP's signs).
    pub fn forces(&mut self, fz: f32, vlat: f32, vlong: f32, spin: f32) -> (f32, f32) {
        let (fx, fy, _) = self.contact(fz, vlat, vlong, spin);
        (fx, fy)
    }

    /// Steady (Fx, Fy, pneumatic trail) at `fz` for the contact velocities and spin (NGP's signs), the trail blended
    /// by the model's weight as the seam blends it.
    pub fn contact(&mut self, fz: f32, vlat: f32, vlong: f32, spin: f32) -> (f32, f32, f32) {
        let p = self.p;
        let patch = patch_at(self.cloud, fz, p.patch_length);
        let ngp = patch_forces(&patch_input(p, &blend_at(p, fz), fz, patch, vlat, vlong, spin));
        let ngp_out = (ngp.longitudinal_force, ngp.lateral_force, ngp.pneumatic_trail);
        let Some(model) = self.model.as_mut() else { return ngp_out };
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
            grip: p.static_friction,
            stiffness_scale: 1.0,
            stiffness_factor: p.stiffness_factor,
            tread_temperature: p.tread_temp,
            tread_left: 1.0 - p.wear_op,
            soil_thickness: self.soil,
            water_factor: self.water,
            lateral_stiffness: p.carcass_lat * p.stiffness_factor,
            torsional_stiffness: p.carcass_tor * p.stiffness_factor,
        };
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
                (
                    mix(ngp.longitudinal_force, o.longitudinal_force),
                    mix(ngp.lateral_force, o.lateral_force),
                    mix(ngp.pneumatic_trail, o.pneumatic_trail),
                )
            }
            None => ngp_out,
        }
    }

    /// The steady aligning moment the game applies about the contact normal, N·m, positive when it turns the wheel
    /// toward its velocity (reduces the slip angle), and the lateral force, N (NGP's sign): `solve_forces`' trail
    /// torque above 2 m/s, the trail times the lateral force less the carcass offset, the lateral deflection (the
    /// lateral force over the carcass's lateral stiffness, flat) times the longitudinal force.
    pub fn aligning(&mut self, fz: f32, vlat: f32, vlong: f32, spin: f32) -> (f32, f32) {
        let (fx, fy, trail) = self.contact(fz, vlat, vlong, spin);
        let deflection = -fy / (self.p.carcass_lat * self.p.stiffness_factor);
        let torque = vlong.signum() * trail * fy - deflection * fx;
        (-torque * vlong.signum() * vlat.signum(), fy)
    }

    /// The aligning moment (see [`Tyre::aligning`]) at the slip angle `deg` and the slip ratio `kappa` (0 free
    /// rolling, negative braking), the contact speed held.
    pub fn mz(&mut self, fz: f32, deg: f32, kappa: f32) -> f32 {
        let (v, a) = (self.speed, deg.to_radians());
        let (vlat, vlong) = (v * a.sin(), -v * a.cos());
        self.aligning(fz, vlat, vlong, (1.0 + kappa) * (-vlong) / self.p.effective_radius).0
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
