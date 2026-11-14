//! The tyre functions' sequences, one per original entry, generic over the store: each loads the tyre's frame, runs
//! its model and stores the frame (moved from the bindings in `rbr-hybrid/src/tyre.rs`, which decode the registers,
//! read the blocks the callers' pointers name, call these, and write the outputs these return through the pointers).
//! The blocks are the caller's: an output that a pointer into the tyre's own words takes is written after the tyre's
//! state (on the per-call path, after its lower), as the original leaves it.
//!
//! An `Outside` leaves the store as it was.

use super::construct::{SetupCopies, INITIAL_EFFECTIVE_RADIUS};
use super::frame_types::{Frame, Indentation};
use super::kernels::{load_material, load_segment, read_table, store_material, store_segment};
use super::{
    apply_heat, apply_inner_air_terms, apply_relax, apply_sector, apply_soil, coefficients_from_words, contact_input,
    count_relax, heat_input, hold_input, inner_air_terms, limits_input, patch_input, relax_input, rolling_input,
    sector_input, soil_input, Outside, StepArgs, StepOut,
};
use crate::math::vec::Vec4;
use crate::mem::Mem;
use crate::ngp::heat::TyreThermal;
use crate::tyre::friction_limits::{FrictionLimits, LimitsInput};
use crate::tyre::grip::GripInput;
use crate::tyre::model::Model;
use crate::tyre::output_block::OutputBlock;
use crate::tyre::patch_forces::PatchForces;
use crate::tyre::solve::Contact;
use crate::tyre::store::TyreStore;
use crate::tyre::surface::SurfaceFriction;
use crate::tyre::{
    contact_frame, contact_patch, forces, friction_curve as curve, friction_limits, grip, heat, hold, locate,
    noise_signal, output_block, patch_forces, relax, sliding, slip, soil, state, step, surface,
};

/// Runs a frame model on the tyre: the frame is loaded once, `run` edits it, and it is stored once when `run`
/// succeeds.
pub fn with_frame<S: TyreStore, T>(
    store: &mut S,
    tyre: S::Tyre,
    run: impl FnOnce(&mut Frame) -> Result<T, Outside>,
) -> Result<T, Outside> {
    let mut frame = store.load_frame(tyre);
    let out = run(&mut frame)?;
    store.store_frame(tyre, &frame);
    Ok(out)
}

/// [`with_frame`] for a model that cannot decline.
fn on_frame<S: TyreStore, T>(store: &mut S, tyre: S::Tyre, run: impl FnOnce(&mut Frame) -> T) -> T {
    let mut frame = store.load_frame(tyre);
    let out = run(&mut frame);
    store.store_frame(tyre, &frame);
    out
}

/// `CScratch::Sign`, st0.
pub fn rolling_torque<S: TyreStore>(store: &S, tyre: S::Tyre) -> f64 {
    forces::rolling_torque(rolling_input(&store.load_frame(tyre)))
}

/// `CScratch::Friction (dt)`, st0.
pub fn hold<S: TyreStore>(store: &mut S, tyre: S::Tyre, dt: f32) -> f64 {
    on_frame(store, tyre, |f| {
        let out = hold::hold(hold_input(f, dt));
        f.state.hold_direction = out.direction;
        out.torque
    })
}

/// `CScratch::Align (normal*, axle*)`: whether it aligned.
pub fn build_contact_frame<S: TyreStore>(store: &mut S, tyre: S::Tyre, normal: Vec4, axle: Vec4) -> bool {
    on_frame(store, tyre, |f| contact_frame::build(f, normal, axle))
}

/// `CScratch::ComputeOutputs`'s inputs for the caller's slip `shares` and friction `scales`. The stiffnesses pass
/// through from the caller's record (`src+0x36c`, `+0x370`): `stiffnesses` when it is the caller's, `None` when it is
/// the scratch's own words (`src` is `this+0x20`, so they are the cornering and traction stiffnesses).
pub fn friction_limits_input<S: TyreStore>(
    store: &S,
    tyre: S::Tyre,
    stiffnesses: Option<[f32; 2]>,
    shares: [f32; 2],
    scales: [f32; 2],
) -> Result<LimitsInput, Outside> {
    let f = store.load_frame(tyre);
    let stiffnesses = stiffnesses.unwrap_or([f.scratch.cornering_stiffness, f.scratch.traction_stiffness]);
    limits_input(&f, stiffnesses, shares, scales)
}

/// `CScratch::ComputeOutputs`: the friction limits ([`friction_limits_input`]), which the caller stores through its ten
/// pointers.
pub fn friction_limits<S: TyreStore>(
    store: &S,
    tyre: S::Tyre,
    stiffnesses: Option<[f32; 2]>,
    shares: [f32; 2],
    scales: [f32; 2],
) -> Result<FrictionLimits, Outside> {
    Ok(friction_limits::friction_limits(&friction_limits_input(store, tyre, stiffnesses, shares, scales)?))
}

/// `CScratch::ComputeSlip (velocity*, spin)`.
pub fn compute_slip<S: TyreStore>(store: &mut S, tyre: S::Tyre, velocity: [f32; 3], spin: f32) {
    on_frame(store, tyre, |f| slip::update_slip_at(f, velocity, spin))
}

/// `CScratch::SolveForces (dt, force*, torque*)`: the force and the torque, which adds to the caller's `torque_in`.
pub fn solve_forces<S: TyreStore>(store: &mut S, tyre: S::Tyre, dt: f32, torque_in: Vec4) -> (Vec4, Vec4) {
    on_frame(store, tyre, |f| forces::solve_forces(f, dt, torque_in, 0.0))
}

/// `CScratch::FillOutputs (velocity*, debug_words*, divisor)`.
pub fn fill_noise_signal<S: TyreStore>(
    store: &mut S,
    tyre: S::Tyre,
    velocity: Vec4,
    debug_words: [u32; 3],
    divisor: f32,
) {
    on_frame(store, tyre, |f| noise_signal::fill_noise_signal(f, velocity, debug_words, divisor))
}

/// `CScratch::WriteOutputs (out*)`: the output block, which the caller stores.
pub fn write_output_block<S: TyreStore>(store: &mut S, tyre: S::Tyre) -> OutputBlock {
    on_frame(store, tyre, output_block::write_output_block)
}

/// `CScratch::SolveDirs (spin, lateral, longitudinal, out0*, out1*, out2*)`: the three slip directions.
pub fn slip_directions<S: TyreStore>(store: &S, tyre: S::Tyre, velocities: [f32; 3]) -> [f32; 3] {
    let [spin, lateral, longitudinal] = velocities;
    slip::slip_directions(spin, lateral, longitudinal, store.load_frame(tyre).state.effective_radius)
}

/// `CScratch::ClampGrip (normal_force, static_friction, kinetic_friction, lateral_stiffness, torsional_stiffness,
/// deflection*, torsion*)`: the deflection and the torsion, which the caller stores.
pub fn limit_to_friction_circle<S: TyreStore>(store: &mut S, tyre: S::Tyre, g: GripInput) -> (f32, f32) {
    on_frame(store, tyre, |f| {
        let out = grip::limit_to_friction_circle(&f.state, g);
        f.state.grip_clamped = out.clamped;
        f.scratch.stick_phase_a = out.phase_a;
        f.scratch.stick_phase_b = out.phase_b;
        (out.deflection, out.torsion)
    })
}

/// `CScratch::Accumulate`.
pub fn soil_drag<S: TyreStore>(store: &mut S, tyre: S::Tyre) {
    on_frame(store, tyre, |f| {
        let out = soil::soil_drag(soil_input(f));
        apply_soil(f, &out);
    })
}

/// The contact solvers `(indentation*, forces*, coefficients*, ...)`: the contact from the three blocks (read by the
/// caller) and the tyre's soil thickness and engaged flags; the flags are stored, the solved contact returned for the
/// caller to store its indentation and forces.
pub fn with_contact<S: TyreStore>(
    store: &mut S,
    tyre: S::Tyre,
    blocks: (Indentation, [f32; 3], [f32; 8]),
    run: impl FnOnce(&mut Contact),
) -> Contact {
    let (indentation, [normal_force, carcass_force, tread_force], coefficients) = blocks;
    on_frame(store, tyre, |f| {
        let mut contact = Contact {
            indentation,
            normal_force,
            carcass_force,
            tread_force,
            ..contact_input(f, coefficients_from_words(coefficients))
        };
        run(&mut contact);
        f.state.tread_engaged = contact.tread_engaged;
        f.state.carcass_engaged = contact.carcass_engaged;
        contact
    })
}

/// `CScratch::HeatStep (lateral_force, longitudinal_force, dt)`.
pub fn heat_step<S: TyreStore>(
    store: &mut S,
    tyre: S::Tyre,
    lateral_force: f32,
    longitudinal_force: f32,
    dt: f32,
    ngp: &TyreThermal,
) -> Result<(), Outside> {
    with_frame(store, tyre, |f| {
        let out = heat::heat_step(heat_input(f, lateral_force, longitudinal_force, dt)?, ngp);
        apply_heat(f, &out);
        Ok(())
    })
}

/// `CScratch::TableStep (sector, elapsed)`.
pub fn relax_sector<S: TyreStore>(
    store: &mut S,
    tyre: S::Tyre,
    sector: u32,
    elapsed: f32,
    ngp: &TyreThermal,
) -> Result<(), Outside> {
    with_frame(store, tyre, |f| {
        let out = relax::relax_sector(&sector_input(f, sector)?, elapsed, ngp);
        apply_sector(f, sector, &out);
        Ok(())
    })
}

/// `CScratch::SetStateField (flag)`: the active flag.
pub fn set_active<S: TyreStore>(store: &mut S, tyre: S::Tyre, flag: u32) {
    on_frame(store, tyre, |f| state::set_active(f, flag))
}

/// `CScratch::SetState48 (height)`.
pub fn set_penetration_depth<S: TyreStore>(store: &mut S, tyre: S::Tyre, height: f32) {
    on_frame(store, tyre, |f| state::set_penetration_depth(f, height))
}

/// `CScratch::BlendTable (first_class, second_class, first_wetness, second_wetness, first*, second*)`: the two
/// frictions, which the caller stores.
pub fn surface_friction<S: TyreStore>(
    store: &S,
    tyre: S::Tyre,
    classes: [u32; 2],
    wetness: [f32; 2],
) -> Result<SurfaceFriction, Outside> {
    Ok(surface::surface_friction(&store.load_surface_input(tyre, classes, wetness)?))
}

/// `CScratch::RelaxCount (dt)`: `RelaxStep`, then the relaxation counter.
pub fn relax_and_count<S: TyreStore>(store: &mut S, tyre: S::Tyre, dt: f32, ngp: &TyreThermal) {
    on_frame(store, tyre, |f| {
        let out = relax::relax(&relax_input(f, dt), ngp);
        apply_relax(f, &out);
        count_relax(f);
    })
}

/// `CScratch::RelaxStep (dt)`.
pub fn relax<S: TyreStore>(store: &mut S, tyre: S::Tyre, dt: f32, ngp: &TyreThermal) {
    on_frame(store, tyre, |f| {
        let out = relax::relax(&relax_input(f, dt), ngp);
        apply_relax(f, &out);
    })
}

/// `CScratch::SplitSlip`.
pub fn weigh_models<S: TyreStore>(store: &mut S, tyre: S::Tyre) {
    on_frame(store, tyre, slip::weigh_models)
}

/// `CScratch::SolvePair`.
pub fn limit_sticking<S: TyreStore>(store: &mut S, tyre: S::Tyre) {
    on_frame(store, tyre, grip::limit_sticking)
}

/// `CScratch::StepInner (rec*, forces*, zeroed*, trail*, lateral_phase*, longitudinal_phase*)`, the record the
/// scratch's own words: the patch forces, which the caller stores.
pub fn patch_forces<S: TyreStore>(store: &S, tyre: S::Tyre) -> Result<PatchForces, Outside> {
    Ok(patch_forces::patch_forces(&patch_input(&store.load_frame(tyre))?))
}

/// `CScratch::ContactPatch (out*)`: the value the model returns when it solved, which the caller stores.
pub fn contact_patch<S: TyreStore>(store: &mut S, tyre: S::Tyre) -> Option<f32> {
    on_frame(store, tyre, contact_patch::contact_patch)
}

/// `CScratch::SolvePatch`.
pub fn solve_sliding<S: TyreStore>(store: &mut S, tyre: S::Tyre) -> Result<(), Outside> {
    with_frame(store, tyre, sliding::solve_sliding)
}

/// `CScratch::Step`: `Step` on the frame with its stack words' values `args`; the outputs, which the caller stores.
/// `tyre_model` (ours, experimental) takes the contact patch's forces ([`step::step`]).
pub fn step<S: TyreStore>(
    store: &mut S,
    tyre: S::Tyre,
    args: &StepArgs,
    ngp: &TyreThermal,
    tyre_model: Option<Model>,
) -> Result<StepOut, Outside> {
    with_frame(store, tyre, |f| step::step(f, args, ngp, tyre_model))
}

/// `CScratch::CopyDefFrom`'s state copies after the def's ([`super::copy_setup_into_def`]): the setup's copies, the
/// initial effective radius, then the inner air's mass from its gas state (`DamperUpdateB` with dry air's R).
pub fn adopt_setup<S: TyreStore>(store: &mut S, tyre: S::Tyre, copies: SetupCopies) {
    on_frame(store, tyre, |f| {
        let st = &mut f.state;
        st.vertical_stiffness = copies.vertical_stiffness;
        st.tread_elasticity = copies.tread_elasticity;
        st.tread_coverage = copies.tread_coverage;
        st.air_pressure = copies.air_pressure;
        st.effective_radius = INITIAL_EFFECTIVE_RADIUS;
        let mut terms = inner_air_terms(st);
        crate::spring_damper::solve_scale(&mut terms, 287.0);
        apply_inner_air_terms(st, &terms);
    })
}

// The entries on the caller's words only (no tyre's state): the image's, per call.

/// `Locate (table, x, n, out)`: the segment at `out` is the hint, and the result is stored back there.
pub fn locate(mem: &mut Mem, table: u32, x: f32, n: u32, out: u32) -> Result<(), Outside> {
    let table = read_table(mem, table, n as usize);
    let seg = locate::locate(load_segment(mem, out), &table, x)?;
    store_segment(mem, out, &seg);
    Ok(())
}

/// `TyreClamp (friction_force, stiffness, deflection, out*)`.
pub fn clamp_deflection(mem: &mut Mem, friction_force: f32, stiffness: f32, deflection: f32, out: u32) {
    mem.set_f32(out, grip::clamp_deflection(friction_force, stiffness, deflection));
}

/// `FloorS32 (x, table, count)`, st0.
pub fn table_lerp(mem: &Mem, x: f32, table: u32, count: u32) -> f64 {
    curve::table_lerp(x, count as i32, |i| mem.f32(table + 4 * i))
}

/// `TyreFrictionCurve`: 13 stack words, values 0 to 8, out pointers 9 and 10, the table and its count; st0.
pub fn friction_curve(mem: &mut Mem, s: &[u32]) -> f64 {
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

/// `CTyreMatData::CopyFrom (src*)`: the material words copied from the block at `src` to the block at `this`.
pub fn copy_material(mem: &mut Mem, this: u32, src: u32) {
    let src = load_material(mem, src);
    store_material(mem, this, &src);
}
