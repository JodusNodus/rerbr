//! NGP's wrappers of the physics step's functions.

use crate::car_body::read_car;
use crate::stage::on_stage;
use rbr_physics::car_body::layout::slot as car_slot;
use rbr_physics::car_body::store::CarStore;
use rbr_physics::engine_driveline::layout::{
    curve_input_at, wheel_at, with_worker, BrakeBlock, CurveInput, DriveObjects,
};
use rbr_physics::engine_driveline::store::DriveStore;
use rbr_physics::mem::Mem;
use rbr_physics::ngp::atmosphere::atmosphere;
use rbr_physics::ngp::axle;
use rbr_physics::ngp::brake::{brake_step, Brake, BrakeIn};
use rbr_physics::ngp::diff;
use rbr_physics::ngp::downforce::{downforce, DownforceIn, Path};
use rbr_physics::ngp::drive;
use rbr_physics::ngp::layout::{drive_line, material, tag_blend};
use rbr_physics::ngp::outputs;
use rbr_physics::ngp::step_wrapper::{self, SurfaceHeatPercent};
use rbr_physics::store::Raw;
use rbr_physics::tyre::layout::{self, entries};

use crate::engine_driveline::{on_drive, on_drive_in};
use crate::rigid_body::{set_solid_word, solid_f32};
use crate::{Ctx, Decline, Host, Regs, Return};

use super::address::{at, FILE_BASE};
use super::{data_f32, f, SETTINGS, SPLIT};

/// The words `0x1004b5a0` stores, in [`rbr_physics::ngp::atmosphere::Atmosphere`]'s order.
const ATMOSPHERE_OUT: [u32; 6] = [0x1017_1adc, 0x1017_1ad8, 0x1017_1ae0, 0x1017_67c8, 0x1017_67cc, 0x1017_67d0];

/// `0x1004b5a0` (cdecl, the car): the atmosphere at the car's height (`+0x148`), into NGP's `.data`. Leaves the car
/// in eax (its first load).
pub(crate) fn atmosphere_update(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let car = r.stack[0];
    apply_atmosphere(ctx.host, &mut ctx.mem, car);
    Ok(Return::void(car, 0))
}

/// `0x1004b5a0`'s work for `car`. The height is the car solid's position as `host` holds it
/// ([`crate::rigid_body::solid_f32`]).
pub(crate) fn apply_atmosphere(host: &dyn Host, m: &mut Mem, car: u32) {
    let constants = rbr_physics::ngp::layout::atmosphere::constants(m, at(FILE_BASE).expect("NGP atmosphere"));
    let a = atmosphere(solid_f32(host, m, car + 0x148), &constants);
    let values = [a.temperature, a.pressure, a.density, a.inv_pressure, a.pressure_115, a.pressure_0987];
    for (addr, v) in ATMOSPHERE_OUT.into_iter().zip(values) {
        m.set_f32(at(addr).expect("hooked only with NGP"), v);
    }
}

/// `0x10047040` (cdecl, `p`; the car is `*(*(p+8)+0x580)`): the downforce into the car's sums, and the drag factor.
/// `eax` is what the original's path leaves: 8 (no downforce), the speed's direction (0, 1 or -1, no speed) or the
/// wheel at `car+0x474` (applied).
pub(crate) fn downforce_update(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    Ok(Return::void(apply_downforce(ctx.host, &mut ctx.mem, r.stack[0]), 0))
}

/// `0x10047040`'s work for `p`; returns its eax. With an aero model set (`super::aero_model`, ours), the file's
/// downforce and the drag factor take its factors: for the car's sideslip, from the speed along its axis over its
/// speed (+0x15fc), and for the file's downforce over its `Cd * A`. The car's speeds are its typed state's, as `host`
/// holds it ([`crate::car_body::read_car`]); the sums (`+0x378`, the force accumulator's z, and `+0x310`, the solid's
/// scratch) its solid's ([`crate::rigid_body::solid_f32`]).
fn apply_downforce(host: &dyn Host, m: &mut Mem, p: u32) -> u32 {
    let car = m.u32(m.u32(p + 8) + 0x580);
    let def = m.u32(car + 0x34);
    let dmg = m.u32(car + 0x9f4);
    let mut i = DownforceIn {
        damage: (dmg != 0).then(|| [m.u32(dmg + 0x3db4), m.u32(dmg + 0x3dbc)]),
        front: m.f32(def + 0x1fc),
        rear: m.f32(def + 0x20c),
        density: m.f32(at(0x1017_1ae0).expect("hooked only with NGP")),
        speed: read_car(host, m, car, car_slot::forward_speed),
        wheel_arms: [0x470, 0x474, 0x478, 0x47c].map(|o| m.f32(m.u32(car + o) + 0x8b4)),
        force_sum: solid_f32(host, m, car + 0x378),
        moment_sum: solid_f32(host, m, car + 0x310),
    };
    let model = super::aero_model::model().map(|model| {
        let cd_area = m.drag_coefficient(car) * m.frontal_area(car);
        model.factors(i.speed, read_car(host, m, car, car_slot::speed), i.front + i.rear, cd_area)
    });
    if let Some((front, rear, _)) = model {
        i.front *= front;
        i.rear *= rear;
    }
    let mut o = downforce(&i);
    if let Some((.., drag)) = model {
        o.drag_factor *= drag;
    }
    m.set_f32(car + 0x4a4, o.speed_ratio_sq);
    m.set_f32(car + 0x4a8, o.front_force);
    m.set_f32(car + 0x4ac, o.rear_force);
    set_solid_word(host, m, car + 0x378, o.force_sum.to_bits());
    set_solid_word(host, m, car + 0x310, o.moment_sum.to_bits());
    m.set_f32(at(0x1017_147c).expect("hooked only with NGP"), o.drag_factor);
    match o.path {
        Path::NoDownforce => 8,
        Path::NoSpeed(direction) => direction as u32,
        Path::Applied => m.u32(car + 0x474),
    }
}

/// `0x10048d50` (thiscall on the drive, `dt`, `ret 4`): NGP's wrapper of `M9BC0` (`rbr_physics::ngp::diff`). The
/// downforce and the gain's loss, the diffs' lock torques for NGP's drivetrain types, then the wheel worker: NGP's axle
/// worker `0x100487f0` for types 4 and 5 (ported), else `M9BC0` in the original through NGP's trampoline `0x10045df0`.
pub(crate) fn drive_wrapper(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let drive = r.ecx;
    let m = &ctx.mem;
    let objects = DriveObjects::at(m, drive);
    let (def, state, obj) = (objects.def, objects.state, objects.damage);
    let word = m.u32(def + 0x640);
    let kind = diff::drive_type(word);
    let settings = m.u32(m.u32(at(SETTINGS).expect("hooked only with NGP")));
    if settings == 0 {
        return Err(Decline::Unsupported("NGP's settings object is not made yet"));
    }
    if (4..8).contains(&kind) && (0..4).any(|k| m.u32(def + 0x158 + 0xf0 * k) > 3) {
        return Err(Decline::Unsupported("a wheel's pack index is past the axles"));
    }
    if matches!(kind, 4 | 5) && !axle_fits(m, def, kind - 3) {
        return Err(Decline::Unsupported("the axle's blocks or packs are out of range"));
    }
    apply_downforce(ctx.host, &mut ctx.mem, drive);
    let loss = super::drive_loss::loss(kind, ctx.mem.u32(settings + 0xe88));
    let tuning = (4..8).contains(&kind).then(|| {
        let m = &ctx.mem;
        let car = m.u32(state + 0x580);
        let locks = m.u32(car + 0x850);
        diff::DiffTuning {
            locks: [[0x1b8, 0x268], [0x738, 0x7e8], [0x9f8, 0xaa8]].map(|p| p.map(|o| m.f32(locks + o))),
            handbrake: crate::vcu::unit_of(ctx.host, m, car).scratch.pedals.handbrake,
            handbrake_release: m.f32(locks + 0x198),
            max_torque: [0, 1, 2].map(|g| m.f32(def + 0x58 + 0x60 * g)),
            enabled: [4, 0xc, 0x14].map(|o| m.u32(obj + o)),
            split: data_f32(ctx, 0x1017_1a90),
            viscous_gain: data_f32(ctx, 0x1017_1aa0),
        }
    });
    // The gain after the loss (the state's `+0x37c`), then the diffs' lock torques (the lock channels' scales) from
    // the worker as the gain leaves it.
    on_drive(ctx, drive, |d| {
        with_worker(d, |w| {
            w.state.drive_gain = diff::after_loss(w.state.drive_gain, loss);
            if let Some(tuning) = &tuning {
                let scales = diff::lock_torques(w, diff::diff_types(word), tuning);
                for (term, v) in w.state.terms.iter_mut().zip(scales) {
                    term.scale = v;
                }
            }
        })
    });
    let dt = r.stack[0];
    if matches!(kind, 4 | 5) {
        let g = (kind - 3) as usize;
        on_drive(ctx, drive, |d| with_worker(d, |w| axle::axle_worker(w, f(dt), g)));
        return Ok(Return::void(0, 4));
    }
    let called = ctx.host.call(at(WHEEL_WORKER).unwrap(), drive, &[dt]);
    Ok(Return::void(called.eax, 4))
}

/// Whether `0x100487f0` on axle `g` stays in the drive's records: its two blocks are wheels and their packs index
/// the active flags (at most 2).
fn axle_fits(m: &Mem, def: u32, g: u32) -> bool {
    [0x28, 0x38].into_iter().all(|o| {
        let k = m.u32(def + 0x60 * g + o);
        k < 4 && m.u32(def + 0x158 + 0xf0 * k) <= 2
    })
}

/// `0x100487f0` (thiscall on the drive, `(dt, g)`, `ret 8`, void): NGP's axle worker (`rbr_physics::ngp::axle`).
pub(crate) fn axle_worker(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [dt, g, ..] = r.stack;
    let def = DriveObjects::at(&ctx.mem, r.ecx).def;
    if !matches!(g, 1 | 2) || !axle_fits(&ctx.mem, def, g) {
        return Err(Decline::Unsupported("the axle's blocks or packs are out of range"));
    }
    on_drive(ctx, r.ecx, |d| with_worker(d, |w| axle::axle_worker(w, f(dt), g as usize)));
    Ok(Return::void(0, 8))
}

/// `0x10015fc0` (thiscall on the drive line, `(pos, x)`, `ret 8`, st0): NGP's `SampleBig` in doubles
/// (`rbr_physics::ngp::drive_line`).
pub(crate) fn drive_line_sample(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let point = ctx.mem.f32s(r.stack[0]);
    let v = on_stage(ctx.host, &ctx.mem, |line| drive_line::position_of(line, r.ecx, point, f(r.stack[1])));
    Ok(Return::float(f64::from(v), 8))
}

/// NGP's rewrite of `E39_Hook4160`, which `Step1E0` calls at `0x50a2a8`.
pub(crate) const CURVE_TERM: u32 = 0x1004_6880;

/// `0x10046880` (thiscall on the drive, `(a1, c, n, x, dt)`, `ret 0x14`, st0): NGP's curve term
/// (`rbr_physics::ngp::brake`), with 1.02's `Worker4060` ported.
pub(crate) fn curve_term_binding(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a1, c, n, x, dt, ..] = r.stack;
    let v = curve_term(&mut ctx.mem, ctx.host, r.ecx, [a1, c, n, x, dt])?;
    Ok(Return::float(f64::from(v), 0x14))
}

/// `0x10046880`'s work; declines (before any writes) without NGP's settings or for a state that isn't a wheel's. The
/// handbrake is the VCU's, as `host` holds it ([`crate::vcu::unit_of`]). The wheel's words (`a1`) and the drive's input
/// block (`x`, the drive's `+0x10`) are the drive's typed state ([`on_drive_in`]); an `x` elsewhere is read on the image.
pub(crate) fn curve_term(m: &mut Mem, host: &dyn Host, drive: u32, args: [u32; 5]) -> Result<f32, Decline> {
    let [a1, c, n, x, dt] = args;
    let state = DriveObjects::at(m, drive).state;
    let car = m.u32(state + 0x580);
    let u = a1.wrapping_sub(state).wrapping_sub(0x48) / 200;
    if u > 3 || wheel_at(state, u) != a1 {
        return Err(Decline::Unsupported("the curve state is not a wheel's"));
    }
    let settings = m.u32(m.u32(at(SETTINGS).expect("hooked only with NGP")));
    if settings == 0 {
        return Err(Decline::Unsupported("NGP's settings object is not made yet"));
    }
    let front = u < 2;
    let handbrake = crate::vcu::unit_of(host, m, car).scratch.pedals.handbrake;
    let rear_split =
        diff::diff_types(m.u32(n + 0x640))[0] == 3 && !front && m.f32(m.u32(car + 0x850) + 0x198) >= handbrake;
    // The hook block's words are NGP's per-wheel brake keys (`Brake*_NGP`): ratio +0xec, piston area +0xcc, effective
    // radius +0x6c, plate area +0x9c, internal convection +0xbc, heat absorption +0x8c, specific heat +0x7c, mass
    // +0x3c, convection factor +0x4c, layer mass share +0x5c, vent area +0xac.
    let block = BrakeBlock::load(m, c);
    let brake = Brake {
        ratio: block.ratio,
        piston_area: block.piston_area,
        effective_radius: block.effective_radius,
        plate_area: block.plate_area,
        internal_convection: block.internal_convection,
        heat_absorption: block.heat_absorption,
        specific_heat: block.specific_heat,
        mass: block.mass,
        convection_factor: m.f32(c + 0x4c),
        layer_mass_share: m.f32(c + 0x5c),
        vent_area: m.f32(c + 0xac),
    };
    let handbrake_factor = m.f32(at(0x1017_1a94).unwrap());
    let released = front && m.u32(DriveObjects::at(m, drive).damage + 0x80) == 0;
    let speed_fade = m.f32(settings + if front { 0xfe8 } else { 0x1168 });
    let car_speed = read_car(host, m, car, car_slot::forward_speed);
    let air_temperature = m.f32(at(0x1017_1adc).unwrap());
    let own_input = x == curve_input_at(drive);
    let image_input = (!own_input).then(|| CurveInput::load(m, x));
    let o = on_drive_in(host, m, drive, |d| {
        let wheel = d.curve_state(u);
        let input = image_input.unwrap_or_else(|| d.curve_input());
        let i = BrakeIn {
            brake,
            wheel_speed: wheel.slope,
            layer_temperature: wheel.pos.value as u32,
            core_temperature: wheel.lane.value as u32,
            pedal_pressure: super::scaled_pedal(u, input.brake_torque),
            handbrake_pressure: input.handbrake_torque,
            handbrake_gain: wheel.term_gain,
            handbrake_factor,
            released,
            speed_fade,
            car_speed,
            air_temperature,
            dt: f(dt),
            split_handbrake: rear_split,
        };
        let o = brake_step(&i, |force| {
            let mut wheel = d.curve_state(u);
            let v = crate::engine_driveline::row_value_of(d.mem(), &mut wheel, force, c, n, f(dt));
            d.store_curve_state(u, &wheel);
            v as f32
        });
        let mut wheel = d.curve_state(u);
        wheel.pos.value = o.layer_temperature as i32;
        wheel.lane.value = o.core_temperature as i32;
        d.store_curve_state(u, &wheel);
        o
    });
    let row = at(0x1017_662c).unwrap() + 0x3c * u;
    for (k, v) in o.telemetry.into_iter().enumerate() {
        m.set_f32(row + 4 * k as u32, v);
    }
    if let Some(t) = o.transfer {
        // Into the latch bound of the wheel two before (the front wheel on this side); none before the front wheels.
        if u >= 2 {
            on_drive_in(host, m, drive, |d| d.store_latch_bound(u - 2, d.latch_bound(u - 2) + t));
        } else {
            let slot = state + 200 * u - 0x98;
            m.set_f32(slot, m.f32(slot) + t);
        }
    }
    Ok(o.torque)
}

const WHEEL_WORKER: u32 = 0x1004_5df0;

/// `0x1005b660` (thiscall on the scratch, 16 words, `ret 0x40`): NGP's wrapper of `CScratch::Step`
/// (`rbr_physics::ngp::step_wrapper`) around the ported `Step`.
pub(crate) fn step_wrapper(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let m = &ctx.mem;
    let settings = m.u32(m.u32(at(SETTINGS).expect("hooked only with NGP")));
    if settings == 0 {
        return Err(Decline::Unsupported("NGP's settings object is not made yet"));
    }
    let def = m.u32(r.ecx + 0x10);
    let saved: [u32; 0x20] = m.u32s(def + 0x204);
    let p = r.stack[4];
    let k = step_wrapper::speed_factor(crate::tyre::read_tyre(ctx.host, m, r.ecx, |t| t.scratch.speed));
    let percent = SurfaceHeatPercent {
        class_2: m.u32(settings + 0xe98),
        class_1: m.u32(settings + 0xea8),
        class_3: m.u32(settings + 0xeb8),
    };
    let scale = step_wrapper::surface_scale(m.u32(p + 0x148), m.u32(def + 0xc), percent);
    let words = [0x48, 0x58, 0x78].map(|o| m.u32(p + o));
    for o in [0x48, 0x58, 0x78] {
        ctx.mem.set_f32(p + o, k * ctx.mem.f32(p + o));
    }
    if let Some(s) = scale {
        for o in (0x204..0x244).step_by(4) {
            ctx.mem.set_f32(def + o, ctx.mem.f32(def + o) * s);
        }
    }
    let ret = crate::tyre::step(ctx, r);
    for (o, w) in (0x204..0x284).step_by(4).zip(saved) {
        ctx.mem.set_u32(def + o, w);
    }
    if ret.is_err() {
        // `Step` declined before writing: undo the speed factor too, so the original runs on the entry state.
        for (o, w) in [0x48, 0x58, 0x78].into_iter().zip(words) {
            ctx.mem.set_u32(p + o, w);
        }
    }
    ret
}

/// `0x1005b4a0` (thiscall, `CScratch::ComputeOutputs`'s 17 stack words): the 1.02 function through NGP's call of the
/// original, then its edits of the lateral and longitudinal end slips.
pub(crate) fn compute_outputs(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let s = &r.stack;
    let stiffnesses = crate::tyre::limit_stiffnesses(ctx, r.ecx, s[6]);
    let (shares, scales) = ([f(s[1]), f(s[2])], [f(s[4]), f(s[5])]);
    let input =
        crate::tyre::on_tyre(ctx, r.ecx, |v, t| entries::friction_limits_input(v, t, stiffnesses, shares, scales))
            .map_err(|_| Decline::Unsupported("index outside the frame"))?;
    let out = outputs::friction_limits(&input);
    layout::store_friction_limits(&mut ctx.mem, std::array::from_fn(|i| s[7 + i]), &out);
    Ok(Return::void(0, 0x44))
}

/// `0x1005e8a0` (stdcall `(blk)`): NGP's rewrite of `M3D230`. Its eax (the CRT's leftovers) isn't modelled.
pub(crate) fn tag_blend(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    tag_blend::update(&mut ctx.mem, r.stack[0]);
    Ok(Return::void(0, 4))
}

/// `0x100458c0` (thiscall `(x)`): NGP's `M5FE0`.
pub(crate) fn wheel_slip_value(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let split = data_f32(ctx, SPLIT);
    let x = r.stack[0] as usize;
    on_drive(ctx, r.ecx, |d| with_worker(d, |w| drive::wheel_slip_value(w, x, split)));
    Ok(Return::void(0, 4))
}

/// `0x10045b80` (thiscall `(x)`): NGP's `M6260`.
pub(crate) fn wheel_latch(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let split = data_f32(ctx, SPLIT);
    let x = r.stack[0] as usize;
    on_drive(ctx, r.ecx, |d| with_worker(d, |w| drive::wheel_latch(w, x, split)));
    Ok(Return::void(0, 4))
}

/// `0x1004df40` (thiscall `(a, b)` on the slot): NGP's 16 by 16 sheet lookup.
pub(crate) fn sample_slot(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let [a, b, ..] = r.stack;
    Ok(Return::void(on_stage(ctx.host, &ctx.mem, |stage| material::sample_slot(stage, r.ecx, f(a), f(b))), 8))
}
