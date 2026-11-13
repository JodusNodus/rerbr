//! `--ffb-tyres MM` (ours, a trial; `notes/ngp.md` "Force feedback"): the wheel's force rebuilt from the front tyres,
//! as AC EVO's is (its `finalFF` fits `a Mz + b Fy` of the front tyres, `b / a` 36-45 mm, linear, no lag), in place
//! of NGP's steering torque sum and its filter. Each front tyre on the ground gives its aligning moment (the
//! pneumatic trail times the lateral force) plus the lateral force times a fixed mechanical trail of `MM`, about the
//! contact's normal, then the wheel's steering gain (`inv_arm * inv_rack`) and the car's rack ratio put the sum in the
//! units NGP's curve takes, so the sensitivity, the linearity (100 %: a line, as AC's) and the steering load read it
//! as NGP's torque. Reads the tyres' scratch after the step, never writes the memory.

use rbr_hybrid::pure::CAR_PTR;
use rbr_physics::car_body::store::CarStore;
use rbr_physics::mem::Mem;
use rbr_physics::ngp::sign;
use rbr_physics::tyre::store::TyreStore;
use rbr_physics::wheel::layout::{INV_ARM, INV_RACK, ON_GROUND, TYRE_SCRATCH};

/// The mechanical trail, m, that gives AC EVO's torque by front slip angle on the r40 circle (`notes/ngp.md`):
/// shorter than its fit's 36-45 mm, as our tyres' pneumatic trail is shorter. `--trace-wheels` takes it.
pub const SUGGESTED_TRAIL: f32 = 0.025;

/// The wheel pointers in the car (`car+0x470`): the front pair first.
const WHEELS: u32 = 0x470;

/// The steering torque from the front tyres with the mechanical trail `trail` (m), in the units of the car's
/// filtered torque (`CarStore::ffb_torque`). The trail torque has the tyre step's sign, `sgn(v_long) * trail * F_lat`
/// about the contact's normal, which holds for the mirrored right-hand contact frame; its world vertical part sums.
/// The gain and the rack ratio count by size: a pack's Alpine A110 RGT has both negative, the Fiesta Rally2 both
/// positive, and NGP's torque has the same sign for the same turn in either.
pub fn torque(mem: &Mem, trail: f32) -> f32 {
    let car = mem.u32(CAR_PTR);
    let sum: f32 = (0..2)
        .map(|k| mem.u32(car + WHEELS + 4 * k))
        .filter(|&wheel| mem.u32(wheel + ON_GROUND) != 0)
        .map(|wheel| {
            let s = mem.load_frame(wheel + TYRE_SCRATCH).scratch;
            let moment = sign(s.contact_velocity[1]) as f32 * (s.pneumatic_trail + trail) * s.lateral_force;
            moment * s.contact_frame.normal.z * (mem.f32(wheel + INV_ARM) * mem.f32(wheel + INV_RACK)).abs()
        })
        .sum();
    -mem.steering_rack_ratio(car).abs() * sum
}

/// The force the wheel gets, in [-1, 1] before the player's settings: [`torque`] through NGP's curve at the car's
/// sensitivity, 0 while the car's force feedback is muted or its override timer runs (as NGP's filter holds it).
pub fn force(mem: &Mem, trail: f32) -> f32 {
    let car = mem.u32(CAR_PTR);
    let torque = if mem.override_timer(car) > 0.0 { 0.0 } else { torque(mem, trail) };
    rbr_physics::ngp::ffb::curve(torque, mem.ffb_muted(car), mem.max_ff_torque(car))
}
