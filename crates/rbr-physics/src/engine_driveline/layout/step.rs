//! The drive's step (`CDrive::Step1E0`, 0x50a1e0).

use super::curve::{brake_block_at, curve_input_at, with_gear, with_worker};
use super::drive_typed::wheel_at;
use crate::engine_driveline::drive_step::{self, DriveStepHost};
use crate::engine_driveline::drive_worker;
use crate::engine_driveline::store::DriveStore;

/// `CDrive::Step1E0` (0x50a1e0, thiscall, `dt` on the stack): the drive's step, in the original's order. The car
/// forwarder is a host call; the wheel, pair and gear workers run through their typed ports, each on a fresh load of the
/// worker, as their bindings do. The speed term, the curve terms and the wheel worker are NGP's rewrites (host calls),
/// and the clutch input takes NGP's `clutch_scale`. Each host call finds the words before it stored.
pub fn step_drive<S: DriveStore>(host: &mut impl DriveStepHost, store: &mut S, dt: f32, clutch_scale: f32) {
    let this = store.at();
    let objects = store.objects();
    let (def, state) = (objects.def, objects.state);

    let (car, speed) = with_worker(store, |w| {
        let count = &mut w.state.step_count;
        count.set(count.value + 1.0);
        (w.state.car, w.state.drive.engine_speed)
    });
    let fr = drive_step::ratio_input(host.car_forwarder(car, dt, speed));
    let mut ring = store.ratio_ring();
    let ratio = drive_worker::ring_average(&mut ring, fr);
    store.store_ratio_ring(&ring);
    with_worker(store, |w| w.own.ring_ratio = ratio);

    let sum = drive_step::clutch_sum(host.ngp_speed_term(dt), ratio);
    let bias = store.worker().own.wheel_bias;
    for (k, wheel_bias) in bias.iter().enumerate() {
        let k = k as u32;
        let term = host.ngp_curve_term([wheel_at(state, k), brake_block_at(def, k), def, curve_input_at(this)], dt);
        store.store_latch_bound(k, drive_step::wheel_latch(term, *wheel_bias));
    }

    with_gear(store, |w, g| drive_worker::gear_trim(w, g, host, dt));
    let w = store.worker();
    let k2 = drive_step::clutch_ratio(store.gear_work().band.ratio, w.def.ratio_gain);

    let slip = store.lanes().clutch_slip.value;
    let b = drive_step::clutch_input(slip, w.damage.torque_gain, clutch_scale);
    let written = with_gear(store, |w, g| drive_worker::clutch_torque(w, g, host, sum, [b, k2, dt]));
    let torque = written.unwrap_or(0.0);

    with_gear(store, |w, g| {
        w.state.drive_gain = drive_step::drive_gain(w, g, torque, k2);
        if drive_step::trips(w) {
            drive_step::trip(w, g);
        }
    });

    if drive_step::takes_alternate_worker(w.def.mode) {
        with_worker(store, |w| drive_worker::pair_worker(w, dt));
    } else {
        host.ngp_wheel_worker(dt);
    }

    with_worker(store, |w| {
        w.own.speed_copy = drive_step::speed_copy(w, k2);
        w.own.clutch_magnitude = drive_step::abs_magnitude(torque);
    });
}

/// The def's and the damage block's words the worker reads (both on the image).
pub(super) const DEF_RATIO_GAIN: u32 = 0xA54;
pub(super) const DEF_MODE: u32 = 0x640;
pub(super) const DAMAGE_TORQUE_GAIN: u32 = 0x84;
