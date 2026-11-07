//! NGP's pure tyre functions over a probed wheel's parameters ([`Params`]): the load blend of its tables and the inputs
//! of `patch_forces` and `friction_limits` as the tyre step builds them, on a flat road (no inclination, no sinking).

use rbr_physics::ngp::outputs;
use rbr_physics::tyre::friction_limits::{BlendPair, FrictionLimits, LimitsInput};
use rbr_physics::tyre::patch_forces::PatchInput;
use rbr_physics::tyre::slip::slip_directions;

use crate::probe::Params;

/// The load tables at one normal force: the blend weight between the two load entries around it, the friction and
/// peak-slip pairs (`CrnMyFct`, `TrctMyFct`, `SlpPkCrn`, `SlpPkTrct`) and the blended stiffnesses times the pressure
/// factor.
pub struct LoadBlend {
    pub weight: f32,
    pub tables: [BlendPair; 4],
    pub cornering_stiffness: f32,
    pub traction_stiffness: f32,
}

/// The load blend at `fz`: the segment of the load axis holding it, clamped at both ends as `locate` clamps.
pub fn blend_at(p: &Params, fz: f32) -> LoadBlend {
    let ax = &p.load_axis;
    let n = ax.len();
    let (lo, hi, frac) = if fz <= ax[0] {
        (0, 0, 0.0)
    } else if fz >= ax[n - 1] {
        (n - 1, n - 1, 0.0)
    } else {
        let lo = (0..n - 1).find(|&k| fz <= ax[k + 1]).unwrap_or(0);
        (lo, lo + 1, (fz - ax[lo]) / (ax[lo + 1] - ax[lo]))
    };
    let pair = |t: &[f32]| BlendPair { at_a: t[lo], at_b: t[hi] };
    let lerp = |t: &[f32]| ((1.0 - frac) * t[lo] + t[hi] * frac) * p.stiffness_factor;
    LoadBlend {
        weight: frac,
        tables: [pair(&p.crn_my), pair(&p.trc_my), pair(&p.crn_pk), pair(&p.trc_pk)],
        cornering_stiffness: lerp(&p.crn_stf),
        traction_stiffness: lerp(&p.trc_stf),
    }
}

/// `patch_forces`' input for the contact velocities `vlat`, `vlong` (m/s) and the spin (rad/s) at `fz` with the patch
/// length `patch`, with the probed surface friction (override `static_friction`/`kinetic_friction` for another).
pub fn patch_input(p: &Params, b: &LoadBlend, fz: f32, patch: f32, vlat: f32, vlong: f32, spin: f32) -> PatchInput {
    PatchInput {
        speed: (vlat * vlat + vlong * vlong).sqrt(),
        lateral_velocity: vlat,
        longitudinal_velocity: vlong,
        spin,
        effective_radius: p.effective_radius,
        static_friction: p.static_friction,
        kinetic_friction: p.kinetic_friction,
        reference_friction: p.reference_friction,
        normal_force: fz,
        cornering_stiffness: b.cornering_stiffness,
        traction_stiffness: b.traction_stiffness,
        inclination_sin: 0.0,
        patch_length: patch,
        sink_depth: 0.0,
        weight: b.weight,
        tables: b.tables,
    }
}

/// NGP's friction limits for the same slip (`ngp::outputs::friction_limits`, as `patch_forces` takes them).
pub fn limits_at(p: &Params, b: &LoadBlend, vlat: f32, vlong: f32, spin: f32) -> FrictionLimits {
    let [lateral_share, longitudinal_share, _] = slip_directions(spin, vlat, vlong, p.effective_radius);
    outputs::friction_limits(&LimitsInput {
        weight: b.weight,
        cornering_friction: b.tables[0],
        traction_friction: b.tables[1],
        cornering_peak_slip: b.tables[2],
        traction_peak_slip: b.tables[3],
        lateral_share,
        longitudinal_share,
        static_scale: p.static_friction * p.reference_friction,
        kinetic_scale: p.kinetic_friction * p.reference_friction,
        cornering_stiffness: b.cornering_stiffness,
        traction_stiffness: b.traction_stiffness,
    })
}
