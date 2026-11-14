//! The tyre functions' sequences, one per original entry, generic over the store: each loads what its model reads,
//! runs the model and stores what it wrote (moved from the bindings in `rbr-hybrid/src/tyre.rs`, which decode the
//! registers and call these). Pointer arguments are the caller's stack or blocks, read and written through `Raw`.
//!
//! The frame entries (`with_frame`) load the scratch once, run on it, and store it once. An `Outside` leaves memory
//! as it was.

use super::frame_types::Frame;
use super::{Outside, StepArgs};
use crate::math::vec::Vec4;
use crate::ngp::heat::TyreThermal;
use crate::tyre::grip::GripInput;
use crate::tyre::model::Model;
use crate::tyre::solve::Contact;
use crate::tyre::store::TyreStore;
use crate::tyre::{
    contact_frame, contact_patch, forces, friction_curve as curve, friction_limits, grip, heat, hold, locate,
    noise_signal, output_block, patch_forces, relax, sliding, slip, soil, state, step, surface,
};

/// Runs a frame model on the scratch at `this`: the frame is loaded once, `run` edits it, and it is stored once when
/// `run` succeeds.
pub fn with_frame<S: TyreStore, T>(
    mem: &mut S,
    this: u32,
    run: impl FnOnce(&mut Frame) -> Result<T, Outside>,
) -> Result<T, Outside> {
    let mut frame = mem.load_frame(this);
    let out = run(&mut frame)?;
    mem.store_frame(this, &frame);
    Ok(out)
}

/// `Locate (table, x, n, out)`: the segment at `out` is the hint, and the result is stored back there.
pub fn locate<S: TyreStore>(mem: &mut S, table: u32, x: f32, n: u32, out: u32) -> Result<(), Outside> {
    let table = mem.read_table(table, n as usize);
    let seg = locate::locate(mem.load_segment(out), &table, x)?;
    mem.store_segment(out, &seg);
    Ok(())
}

/// `CScratch::Sign`, st0.
pub fn rolling_torque<S: TyreStore>(mem: &S, this: u32) -> f64 {
    forces::rolling_torque(mem.load_rolling_input(this))
}

/// `CScratch::Friction (dt)`, st0.
pub fn hold<S: TyreStore>(mem: &mut S, this: u32, dt: f32) -> f64 {
    let out = hold::hold(mem.load_hold_input(this, dt));
    mem.store_hold_direction(this, out.direction);
    out.torque
}

/// `CScratch::Align (normal*, axle*)`: whether it aligned.
pub fn build_contact_frame<S: TyreStore>(mem: &mut S, this: u32, normal: u32, axle: u32) -> Result<bool, Outside> {
    let normal = Vec4::load(mem, normal);
    let axle = Vec4::load(mem, axle);
    with_frame(mem, this, |frame| Ok(contact_frame::build(frame, normal, axle)))
}

/// `CScratch::ComputeOutputs`: the 17 stack words, scales at 1, 2, 4 and 5, `src` at 6, the output pointers at 7 to 16.
pub fn friction_limits<S: TyreStore>(mem: &mut S, this: u32, s: &[u32]) {
    let f = f32::from_bits;
    let input = mem.load_friction_limits(this, s[6], [f(s[1]), f(s[2])], [f(s[4]), f(s[5])]);
    let out = friction_limits::friction_limits(&input);
    mem.store_friction_limits(std::array::from_fn(|i| s[7 + i]), &out);
}

/// `TyreClamp (friction_force, stiffness, deflection, out*)`.
pub fn clamp_deflection<S: TyreStore>(mem: &mut S, friction_force: f32, stiffness: f32, deflection: f32, out: u32) {
    mem.set_f32(out, grip::clamp_deflection(friction_force, stiffness, deflection));
}

/// `FloorS32 (x, table, count)`, st0.
pub fn table_lerp<S: TyreStore>(mem: &S, x: f32, table: u32, count: u32) -> f64 {
    curve::table_lerp(x, count as i32, |i| mem.f32(table + 4 * i))
}

/// `CScratch::ComputeSlip (velocity*, spin)`.
pub fn compute_slip<S: TyreStore>(mem: &mut S, this: u32, velocity: u32, spin: f32) {
    let velocity = [mem.f32(velocity), mem.f32(velocity + 4), mem.f32(velocity + 8)];
    let radius = mem.load_effective_radius(this);
    let axes = mem.load_contact_axes(this);
    let out = slip::compute_slip(&axes, radius, velocity, spin);
    mem.store_slip(this, &out);
}

/// `CScratch::SolveForces (dt, force*, torque*)`: `torque` is read (the asm adds into it), and both are written.
pub fn solve_forces<S: TyreStore>(mem: &mut S, this: u32, dt: f32, force: u32, torque: u32) -> Result<(), Outside> {
    let torque_in = Vec4::load(mem, torque);
    let (f, t) = with_frame(mem, this, |frame| Ok(forces::solve_forces(frame, dt, torque_in, 0.0)))?;
    f.store(mem, force);
    t.store(mem, torque);
    Ok(())
}

/// `CScratch::FillOutputs (velocity*, debug_words*, divisor)`.
pub fn fill_noise_signal<S: TyreStore>(
    mem: &mut S,
    this: u32,
    velocity: u32,
    debug_words: u32,
    divisor: f32,
) -> Result<(), Outside> {
    let velocity = Vec4::load(mem, velocity);
    let debug_words: [u32; 3] = std::array::from_fn(|k| mem.u32(debug_words + 4 * k as u32));
    with_frame(mem, this, |frame| {
        noise_signal::fill_noise_signal(frame, velocity, debug_words, divisor);
        Ok(())
    })
}

/// `CScratch::WriteOutputs (out*)`.
pub fn write_output_block<S: TyreStore>(mem: &mut S, this: u32, out: u32) -> Result<(), Outside> {
    let block = with_frame(mem, this, |frame| Ok(output_block::write_output_block(frame)))?;
    mem.store_output_block(out, &block);
    Ok(())
}

/// `TyreFrictionCurve`: 13 stack words, values 0 to 8, out pointers 9 and 10, the table and its count; st0.
pub fn friction_curve<S: TyreStore>(mem: &mut S, s: &[u32]) -> f64 {
    let (table, count) = (s[11], s[12] as i32);
    let f = f32::from_bits;
    let input = curve::CurveInput {
        slip: f(s[0]),
        peak_slip: f(s[1]),
        end_slip: f(s[2]),
        peak_friction: f(s[3]),
        slide_friction: f(s[4]),
        stiffness: f(s[5]),
        load: f(s[6]),
        trail_linear: f(s[7]),
        trail_sliding: f(s[8]),
        count,
    };
    let out = curve::friction_curve(&input, |i| mem.f32(table + 4 * i));
    mem.set_f32(s[9], out.trail_fraction);
    mem.set_f32(s[10], out.phase);
    out.friction
}

/// `CScratch::SolveDirs (spin, lateral, longitudinal, out0*, out1*, out2*)`.
pub fn slip_directions<S: TyreStore>(mem: &mut S, this: u32, velocities: [f32; 3], outs: [u32; 3]) {
    let radius = mem.load_effective_radius(this);
    let [spin, lateral, longitudinal] = velocities;
    let out = slip::slip_directions(spin, lateral, longitudinal, radius);
    for (at, value) in outs.into_iter().zip(out) {
        mem.set_f32(at, value);
    }
}

/// `CScratch::ClampGrip (normal_force, static_friction, kinetic_friction, lateral_stiffness, torsional_stiffness,
/// deflection*, torsion*)`.
pub fn limit_to_friction_circle<S: TyreStore>(
    mem: &mut S,
    this: u32,
    g: GripInput,
    deflection: u32,
    torsion: u32,
) -> Result<(), Outside> {
    let out = with_frame(mem, this, |frame| {
        let out = grip::limit_to_friction_circle(&frame.state, g);
        frame.state.grip_clamped = out.clamped;
        frame.scratch.stick_phase_a = out.phase_a;
        frame.scratch.stick_phase_b = out.phase_b;
        Ok(out)
    })?;
    mem.set_f32(deflection, out.deflection);
    mem.set_f32(torsion, out.torsion);
    Ok(())
}

/// `CScratch::Accumulate`.
pub fn soil_drag<S: TyreStore>(mem: &mut S, this: u32) {
    let out = soil::soil_drag(mem.load_soil_input(this));
    mem.store_soil_drag(this, &out);
}

/// The contact solvers `(indentation*, forces*, coefficients*, ...)`: the contact loaded from and stored back to the
/// three blocks (the coefficients are read only).
pub fn with_contact<S: TyreStore>(
    mem: &mut S,
    this: u32,
    indentation: u32,
    forces: u32,
    coefficients: u32,
    run: impl FnOnce(&mut Contact),
) {
    let mut contact = mem.load_contact(this, indentation, forces, coefficients);
    run(&mut contact);
    mem.store_contact(this, indentation, forces, &contact);
}

/// `CScratch::HeatStep (lateral_force, longitudinal_force, dt)`.
pub fn heat_step<S: TyreStore>(
    mem: &mut S,
    this: u32,
    lateral_force: f32,
    longitudinal_force: f32,
    dt: f32,
    ngp: &TyreThermal,
) {
    let input = mem.load_heat(this, lateral_force, longitudinal_force, dt);
    mem.store_heat(this, &heat::heat_step(input, ngp));
}

/// `CScratch::TableStep (sector, elapsed)`.
pub fn relax_sector<S: TyreStore>(mem: &mut S, this: u32, sector: u32, elapsed: f32, ngp: &TyreThermal) {
    let input = mem.load_sector_input(this, sector);
    let out = relax::relax_sector(&input, elapsed, ngp);
    mem.store_sector(this, sector, &out);
}

/// `CScratch::SetStateField (flag)`: the active flag.
pub fn set_active<S: TyreStore>(mem: &mut S, this: u32, flag: u32) -> Result<(), Outside> {
    with_frame(mem, this, |frame| {
        state::set_active(frame, flag);
        Ok(())
    })
}

/// `CScratch::SetState48 (height)`.
pub fn set_penetration_depth<S: TyreStore>(mem: &mut S, this: u32, height: f32) -> Result<(), Outside> {
    with_frame(mem, this, |frame| {
        state::set_penetration_depth(frame, height);
        Ok(())
    })
}

/// `CScratch::BlendTable (first_class, second_class, first_wetness, second_wetness, first*, second*)`.
pub fn surface_friction<S: TyreStore>(
    mem: &mut S,
    this: u32,
    classes: [u32; 2],
    wetness: [f32; 2],
    first: u32,
    second: u32,
) {
    let input = mem.load_surface_input(this, classes, wetness);
    mem.store_surface_friction(first, second, surface::surface_friction(&input));
}

/// `CScratch::RelaxCount (dt)`: `RelaxStep`, then the relaxation counter (`m_state+0x1c`).
pub fn relax_and_count<S: TyreStore>(mem: &mut S, this: u32, dt: f32, ngp: &TyreThermal) {
    let input = mem.load_relax(this, dt);
    let out = relax::relax(&input, ngp);
    mem.store_relax_count(this, &out);
}

/// `CScratch::RelaxStep (dt)`.
pub fn relax<S: TyreStore>(mem: &mut S, this: u32, dt: f32, ngp: &TyreThermal) {
    let input = mem.load_relax(this, dt);
    let out = relax::relax(&input, ngp);
    mem.store_relax(this, &out);
}

/// `CScratch::SplitSlip`.
pub fn weigh_models<S: TyreStore>(mem: &mut S, this: u32) -> Result<(), Outside> {
    with_frame(mem, this, |frame| {
        slip::weigh_models(frame);
        Ok(())
    })
}

/// `CTyreMatData::CopyFrom (src*)`: the material words copied from the block at `src`.
pub fn copy_material<S: TyreStore>(mem: &mut S, this: u32, src: u32) {
    let src = mem.load_material(src);
    mem.store_material(this, &src);
}

/// `CScratch::SolvePair`.
pub fn limit_sticking<S: TyreStore>(mem: &mut S, this: u32) -> Result<(), Outside> {
    with_frame(mem, this, |frame| {
        grip::limit_sticking(frame);
        Ok(())
    })
}

/// `CScratch::StepInner (rec*, forces*, zeroed*, trail*, lateral_phase*, longitudinal_phase*)`; the record is the
/// scratch's own words.
pub fn patch_forces<S: TyreStore>(mem: &mut S, this: u32, outs: [u32; 5]) {
    let input = mem.load_patch_input(this);
    let out = patch_forces::patch_forces(&input);
    mem.store_patch_forces(outs, &out);
}

/// `CScratch::ContactPatch (out*)`: whether it solved; the value the model returns is stored at `out`.
pub fn contact_patch<S: TyreStore>(mem: &mut S, this: u32, out: u32) -> Result<bool, Outside> {
    let solved = with_frame(mem, this, |frame| Ok(contact_patch::contact_patch(frame)))?;
    if let Some(value) = solved {
        mem.set_f32(out, value);
    }
    Ok(solved.is_some())
}

/// `CScratch::SolvePatch`.
pub fn solve_sliding<S: TyreStore>(mem: &mut S, this: u32) -> Result<(), Outside> {
    with_frame(mem, this, sliding::solve_sliding)
}

/// `CScratch::Step`: its 16 stack words are loaded once, `Step` runs on the frame, and its outputs are stored after
/// the frame. `tyre_model` (ours, experimental) takes the contact patch's forces ([`step::step`]).
pub fn step<S: TyreStore>(
    mem: &mut S,
    this: u32,
    words: &[u32],
    ngp: &TyreThermal,
    tyre_model: Option<Model>,
) -> Result<(), Outside> {
    let (args, ptrs): (StepArgs, _) = mem.load_step(&words[..16]);
    let out = with_frame(mem, this, |frame| step::step(frame, &args, ngp, tyre_model))?;
    mem.store_step(&ptrs, &out);
    Ok(())
}
