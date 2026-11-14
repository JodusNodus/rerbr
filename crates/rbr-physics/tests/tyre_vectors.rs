//! Replays the original's tyre calls (`harness/vcu_vectors.py --set tyre`) through the ports in
//! `rbr_physics::tyre`. Needs record files: `RBR_TYRE_VECTORS=<file>[:<file>...]`. Without the variable
//! the tests are skipped.

mod common;

use common::{load, map_windows, After, Call, Window};
use rbr_physics::fpu::Precision;
use rbr_physics::math::vec::Vec4;
use rbr_physics::mem::Mem;
use rbr_physics::store::defs::{Defs, Held};
use rbr_physics::tyre::layout::entries;
use rbr_physics::tyre::store::TyreStore;
use rbr_physics::tyre::tyres::TyreId;
use rbr_physics::tyre::{forces, grip, layout, solve, state};

const VAR: &str = "RBR_TYRE_VECTORS";

/// Replays every recorded call of `addr` through `run`, on the memory `map` builds for the call. `run` returns st0
/// (for an x87 return), or `Outside` when the port declines the call: a declined call is counted and skipped, because
/// the binding runs the original for it. st0 is checked bit for bit, then every window must hold the original's bytes.
fn replay_run(
    addr: u32,
    name: &str,
    nan_tolerant: bool,
    map: fn(&Call) -> Mem,
    run: impl Fn(&mut Mem, &Call) -> Result<Option<f64>, layout::Outside>,
) {
    let Some(calls) = load(VAR, addr) else {
        eprintln!("skipping {name}: {VAR} not set");
        return;
    };
    if calls.is_empty() {
        eprintln!("{name}: no calls in the vector files");
        return;
    }
    let mut branches = std::collections::BTreeMap::<String, usize>::new();
    let mut declined = 0;
    for call in &calls {
        let mut mem = map(call);
        let st0 = match run(&mut mem, call) {
            Ok(st0) => st0,
            Err(out) => {
                // A decline is an index outside the table's window: below 0, or past its entries. The word below a
                // table (-1) is in its window (locate and the blend tables), so no decline reports -1.
                assert!(
                    out.index < 0 || out.index >= out.len as i32,
                    "{name} call {} ({}): declined at index {} of {} entries, inside the window",
                    call.index,
                    call.branch,
                    out.index,
                    out.len
                );
                declined += 1;
                continue;
            }
        };
        if let Some(st0) = st0 {
            let recorded = f64::from_bits(call.st0_bits);
            // The fixture retains the original st0 NaN sign, but this comparison intentionally ignores it:
            // a host may choose a different default-NaN sign. The windows are compared bit for bit here.
            if recorded.is_nan() {
                assert!(st0.is_nan(), "{name} call {} ({}): st0 {st0} vs NaN", call.index, call.branch);
            } else {
                assert_eq!(
                    st0.to_bits(),
                    recorded.to_bits(),
                    "{name} call {} ({}): st0 {st0} vs {recorded}",
                    call.index,
                    call.branch
                );
            }
        }
        for Window { index: k, at, len, before: _, after: post } in &call.windows {
            let now = mem.read(*at, *len);
            let same = if nan_tolerant { same_words(&now, post) } else { now == *post };
            assert!(same, "{name} call {} window {k}: {now:02x?} vs {post:02x?}", call.index);
        }
        *branches.entry(call.branch.clone()).or_default() += 1;
    }
    eprintln!("{name}: {} calls match, {declined} declined, by branch: {branches:?}", calls.len() - declined);
}

/// Replays every recorded call of `addr` through `port`, which maps the call's arguments onto the port and
/// writes the results into `mem` and returns st0 (for an x87 return). st0 is checked bit for bit, then every
/// window must hold the original's bytes after the call.
fn replay(addr: u32, name: &str, port: impl Fn(&mut Mem, &Call) -> Option<f64>) {
    replay_inner(addr, name, false, port);
}

/// `replay`, with the windows compared NaN-tolerantly when `nan_tolerant` (see `same_words`).
fn replay_inner(addr: u32, name: &str, nan_tolerant: bool, port: impl Fn(&mut Mem, &Call) -> Option<f64>) {
    replay_run(addr, name, nan_tolerant, map_windows, |mem, c| Ok(port(mem, c)));
}

/// Replays `addr` through a tyre entry on the per-call view, as the binding runs it: `run` reads the call's blocks,
/// runs the entry on the tyre at `this` (`args[0]`, [`on_tyre`]) and writes its outputs through the call's pointers
/// after, on the memory of a frame call.
fn replay_tyre(
    addr: u32,
    name: &str,
    nan_tolerant: bool,
    run: impl Fn(&mut Mem, &Call) -> Result<Option<f64>, layout::Outside>,
) {
    replay_run(addr, name, nan_tolerant, map_frame, run);
}

/// Runs `run` on the tyre whose scratch is `args[0]`, lifted and lowered around it (`layout::with_tyre`).
fn on_tyre<T>(mem: &mut Mem, c: &Call, run: impl FnOnce(&mut layout::TyreView, TyreId) -> T) -> T {
    layout::with_tyre(mem, c.args[0], run)
}

/// Whether the call's windows hold the `len` bytes at `at`.
fn recorded(call: &Call, at: u32, len: u32) -> bool {
    call.windows.iter().any(|Window { at: w, len: wlen, .. }| *w <= at && at + len <= *w + *wlen as u32)
}

/// The memory of a frame call: the recorded windows, then zeros over the frame's footprint (`load_frame` reads
/// `this`, the def, the state, the record's contact angle and the wear table) wherever no window holds the word. A
/// window keeps its own bytes, and a word no window holds reads as zero.
fn map_frame(call: &Call) -> Mem {
    let mut mem = map_windows(call);
    let zero = |mem: &mut Mem, base: u32, len: u32| {
        for at in base..base + len {
            if !recorded(call, at, 1) {
                mem.map(at, &[0]);
            }
        }
    };
    let this = call.args[0];
    zero(&mut mem, this, 0x520);
    let (def, state) = (mem.u32(this + 0x10), mem.u32(this + 0x14));
    let (record, wear) = (mem.u32(this + 0x410), mem.u32(this + 0x414));
    zero(&mut mem, def, 0x320);
    zero(&mut mem, state, 0x140);
    zero(&mut mem, record + 0x5AC, 4);
    zero(&mut mem, wear, 0x20);
    mem
}

/// The argument word `i` as an f32.
fn arg_f32(call: &Call, i: usize) -> f32 {
    f32::from_bits(call.args[i])
}

#[test]
fn rolling_torque_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x531ba0, "rolling_torque", false, |mem, c| {
        Ok(Some(on_tyre(mem, c, |v, t| entries::rolling_torque(v, t))))
    });
}

#[test]
fn clamp_deflection_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x5322e0, "clamp_deflection", |mem, c| {
        entries::clamp_deflection(mem, arg_f32(c, 0), arg_f32(c, 1), arg_f32(c, 2), c.args[3]);
        None
    });
}

#[test]
fn compute_slip_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x532a30, "compute_slip", false, |mem, c| {
        let dir = c.args[1];
        let dir = [mem.f32(dir), mem.f32(dir + 4), mem.f32(dir + 8)];
        on_tyre(mem, c, |v, t| entries::compute_slip(v, t, dir, arg_f32(c, 2)));
        Ok(None)
    });
}

#[test]
fn slip_directions_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x533640, "slip_directions", false, |mem, c| {
        let velocities = [arg_f32(c, 1), arg_f32(c, 2), arg_f32(c, 3)];
        let out = on_tyre(mem, c, |v, t| entries::slip_directions(v, t, velocities));
        for (i, value) in out.iter().enumerate() {
            mem.set_f32(c.args[4 + i], *value);
        }
        Ok(None)
    });
}

#[test]
fn soil_drag_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x5338a0, "soil_drag", true, |mem, c| {
        on_tyre(mem, c, |v, t| entries::soil_drag(v, t));
        Ok(None)
    });
}

#[test]
fn store_material_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x535040, "store_material", |mem, c| {
        entries::copy_material(mem, c.args[0], c.args[1]);
        None
    });
}

#[test]
fn store_wear_table_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x532490, "store_wear_table", |mem, c| {
        layout::store_wear_table(mem, c.args[0], c.args[1]);
        None
    });
}

/// The solve tests: the contact blocks are read from the record's `p`, `qb` and `r`, and the solved contact stored back
/// after the tyre. The windows are compared with `same_words`: a NaN the solve makes from finite operands (`inf - inf`,
/// a `sqrt` of a negative) may take a different default-NaN sign on the host. Every other word must match bit for bit.
fn replay_solve(addr: u32, name: &str, run: impl Fn(&mut rbr_physics::tyre::solve::Contact, &Call)) {
    replay_tyre(addr, name, true, |mem, c| {
        let (p, qb, r) = (c.args[1], c.args[2], c.args[3]);
        let blocks = layout::load_contact_blocks(mem, p, qb, r);
        let contact = on_tyre(mem, c, |v, t| entries::with_contact(v, t, blocks, |contact| run(contact, c)));
        layout::store_contact_blocks(mem, p, qb, &contact);
        Ok(None)
    });
}

/// Equal byte strings, with NaN equal to NaN word by word (`f32`, 4 bytes at a time; the tail is compared as bytes).
fn same_words(ours: &[u8], orig: &[u8]) -> bool {
    let word_ok = |a: &[u8], b: &[u8]| {
        a == b
            || (f32::from_le_bytes(a.try_into().unwrap()).is_nan()
                && f32::from_le_bytes(b.try_into().unwrap()).is_nan())
    };
    let whole = ours.len() / 4 * 4;
    ours.len() == orig.len()
        && (0..whole).step_by(4).all(|i| word_ok(&ours[i..i + 4], &orig[i..i + 4]))
        && ours[whole..] == orig[whole..]
}

#[test]
fn solve_tread_only_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_solve(0x533ae0, "solve_tread_only", |contact, c| solve::solve_tread_only(contact, c.args[4] as i32));
}

#[test]
fn solve_both_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_solve(0x533bf0, "solve_both", |contact, c| solve::solve_both(contact, c.args[4] as i32));
}

#[test]
fn solve_neither_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_solve(0x533d40, "solve_neither", |contact, c| solve::solve_neither(contact, c.args[4] as i32));
}

#[test]
fn solve_carcass_only_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_solve(0x533e80, "solve_carcass_only", |contact, c| solve::solve_carcass_only(contact, c.args[4] as i32));
}

#[test]
fn solve_contact_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_solve(0x5351a0, "solve_contact", |contact, c| {
        solve::solve_contact(contact, c.args[4] != 0, c.args[5] != 0, c.args[6] as i32)
    });
}

/// `friction` with NaN-tolerant windows: its `sqrt` of a negative takes the host's NaN sign (see `same_words`).
#[test]
fn hold_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x531c70, "hold", true, |mem, c| Ok(Some(on_tyre(mem, c, |v, t| entries::hold(v, t, arg_f32(c, 1))))));
}

/// `floor_s32` (FloorS32, stdcall: x, the table, its count). The table is a window read through `mem`.
#[test]
fn table_lerp_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x5325f0, "table_lerp", |mem, c| Some(entries::table_lerp(mem, arg_f32(c, 0), c.args[1], c.args[2])));
}

/// `friction_curve` (TyreFrictionCurve, stdcall with 13 words): args 0-8 are values, 9 and 10 the outputs,
/// 11 the table and 12 its count. NaN-tolerant windows: a NaN output takes the host's NaN sign (see `same_words`).
#[test]
fn friction_curve_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_inner(0x5334d0, "friction_curve", true, |mem, c| Some(entries::friction_curve(mem, &c.args)));
}

/// `split_slip` (SplitSlip, thiscall, no stack words): the slip speed at +0x360 splits into the weights at +0x3dc
/// and +0x3f0.
#[test]
fn weigh_models_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x5346f0, "weigh_models", false, |mem, c| {
        on_tyre(mem, c, |v, t| entries::weigh_models(v, t));
        Ok(None)
    });
}

/// `align` (Align, thiscall: the two vector pointers as stack words). Its eax is 1 when it aligned.
#[test]
fn build_contact_frame_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x531fc0, "build_contact_frame", true, |mem, c| {
        let (a, b) = (Vec4::load(mem, c.args[1]), Vec4::load(mem, c.args[2]));
        let aligned = on_tyre(mem, c, |v, t| entries::build_contact_frame(v, t, a, b));
        assert_eq!(u32::from(aligned), c.eax, "build_contact_frame call {}: eax", c.index);
        Ok(None)
    });
}

/// `clamp_grip` (ClampGrip, thiscall, seven stack words: five floats, then the out pointers).
#[test]
fn limit_to_friction_circle_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    // The outputs are pointers that may sit in the tyre: they are written after it, as the binding does.
    replay_tyre(0x533760, "limit_to_friction_circle", false, |mem, c| {
        let g = grip::GripInput {
            normal_force: arg_f32(c, 1),
            static_friction: arg_f32(c, 2),
            kinetic_friction: arg_f32(c, 3),
            lateral_stiffness: arg_f32(c, 4),
            torsional_stiffness: arg_f32(c, 5),
        };
        let (deflection, torsion) = on_tyre(mem, c, |v, t| entries::limit_to_friction_circle(v, t, g));
        mem.set_f32(c.args[6], deflection);
        mem.set_f32(c.args[7], torsion);
        Ok(None)
    });
}

/// `solve_pair` (SolvePair, thiscall, no stack words).
#[test]
fn limit_sticking_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x5352a0, "limit_sticking", false, |mem, c| {
        on_tyre(mem, c, |v, t| entries::limit_sticking(v, t));
        Ok(None)
    });
}

/// `solve_forces` (SolveForces, thiscall: the friction's `k` and the two output vectors as stack words).
/// NaN-tolerant: a NaN lane of an output takes the other default NaN sign (the x87 default is negative), see notes.
#[test]
fn solve_forces_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    // The outputs are pointers that may sit in the tyre: they are written after it, as the binding does.
    replay_tyre(0x532b60, "solve_forces", true, |mem, c| {
        let torque_in = Vec4::load(mem, c.args[3]);
        let (force, torque) = on_tyre(mem, c, |v, t| {
            // A tyre model at full weight (ours): no damping, the trail unscaled, whatever the sticking weight.
            let frame = v.load_frame(t);
            let mut model = frame;
            forces::solve_forces(&mut model, arg_f32(c, 1), torque_in, 1.0);
            let s = &frame.scratch;
            let lateral = 0.0 - frame.state.lateral_deflection * s.lateral_stiffness * s.inclination_cos;
            let torsion = 0.0 - frame.state.torsion_angle * s.torsional_stiffness;
            let same = |a: f32, b: f32| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan());
            // (A NaN weight or spin slip, which only fuzzed records hold, makes the factors NaN either way.)
            if [s.sliding_weight, s.sticking_weight, s.spin_slip].iter().all(|x| x.is_finite()) {
                assert!(same(model.scratch.lateral_force, lateral), "{} {lateral}", model.scratch.lateral_force);
                assert!(same(model.scratch.torsion_torque, torsion), "{} {torsion}", model.scratch.torsion_torque);
                assert!(same(model.scratch.pneumatic_trail, s.pneumatic_trail));
            }
            entries::solve_forces(v, t, arg_f32(c, 1), torque_in)
        });
        force.store(mem, c.args[2]);
        torque.store(mem, c.args[3]);
        Ok(None)
    });
}

/// `fill_outputs` (FillOutputs, thiscall: the `src` vector pointer, the `pair` pointer and the divisor as stack words).
#[test]
fn fill_noise_signal_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x532f10, "fill_noise_signal", false, |mem, c| {
        let velocity = Vec4::load(mem, c.args[1]);
        let debug_words: [u32; 3] = std::array::from_fn(|k| mem.u32(c.args[2] + 4 * k as u32));
        on_tyre(mem, c, |v, t| entries::fill_noise_signal(v, t, velocity, debug_words, arg_f32(c, 3)));
        Ok(None)
    });
}

/// `write_outputs` (WriteOutputs, thiscall: the output struct pointer as its stack word).
#[test]
fn write_output_block_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    // Fuzz call 3858 differs only in the sign of six default NaNs (see rbr-recomp notes/rust-physics.md section 8).
    // The outputs are pointers that may sit in the tyre: they are written after it, as the binding does.
    replay_tyre(0x533080, "write_output_block", true, |mem, c| {
        let block = on_tyre(mem, c, |v, t| entries::write_output_block(v, t));
        layout::store_output_block(mem, c.args[1], &block);
        Ok(None)
    });
}

/// `contact_patch` (ContactPatch, thiscall with one stack word, the `out` float). Its `eax` is 1 when solved. The
/// windows are NaN-tolerant: a `sqrt` of a negative `s8` takes the other default NaN sign (fuzz call 233).
#[test]
fn contact_patch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    // The outputs are pointers that may sit in the tyre: they are written after it, as the binding does.
    replay_tyre(0x5358f0, "contact_patch", true, |mem, c| {
        let out = on_tyre(mem, c, |v, t| entries::contact_patch(v, t));
        assert_eq!(u32::from(out.is_some()), c.eax, "contact_patch call {}: eax", c.index);
        if let Some(value) = out {
            mem.set_f32(c.args[1], value);
        }
        Ok(None)
    });
}

/// `locate` (0x528980, cdecl): the table pointer, `x`, the count and the out struct as stack words 0 to 3.
#[test]
fn locate_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    // NaN-tolerant: a NaN fraction's sign is the host's default, not the original's x87 one.
    replay_run(0x528980, "locate", true, map_locate, |mem, c| {
        entries::locate(mem, c.args[0], arg_f32(c, 1), c.args[2], c.args[3])?;
        Ok(None)
    });
}

/// The memory of a `locate` call: the recorded windows, then zero on the word below the table where no window holds it
/// (the drive records `d-*` predate that window). The search reads that word only from a hint of -1, or from a NaN `x`
/// with a hint of 0, which gives a NaN fraction whatever the word. No store record has a hint of -1, so the zero is
/// never read into a result; a record that does would show it as a mismatch.
fn map_locate(call: &Call) -> Mem {
    let mut mem = map_windows(call);
    let below = call.args[0] - 4;
    if !recorded(call, below, 4) {
        mem.map(below, &[0; 4]);
    }
    mem
}

/// `set_state_field` (SetStateField, thiscall with the flag as its stack word).
#[test]
fn set_active_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x532470, "set_active", false, |mem, c| {
        on_tyre(mem, c, |v, t| entries::set_active(v, t, c.args[1]));
        Ok(None)
    });
}

/// `set_state48` (SetState48, thiscall with the float as its stack word). The windows include the def (`+0x7c`, `+0x8c`).
#[test]
fn set_penetration_depth_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    // The original reads the def's `+0x7c` and `+0x8c`. A record without a window over them (`d-s*`) cannot show the
    // result, so the call is counted as declined.
    replay_tyre(0x533490, "set_penetration_depth", false, |mem, c| {
        let def = mem.u32(c.args[0] + 0x10);
        if !recorded(c, def + 0x7C, 0x14) {
            // Not an index: no window holds the def. The empty window (index 0 of 0 entries) is outside it.
            return Err(layout::Outside { index: 0, len: 0 });
        }
        on_tyre(mem, c, |v, t| entries::set_penetration_depth(v, t, arg_f32(c, 1)));
        Ok(None)
    });
}

/// `clear_bias` (ClearBias, thiscall, no stack words). Its `DamperBiasSub` calls write nothing, so the windows are
/// unchanged.
#[test]
fn clear_bias_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x534000, "clear_bias", |_mem, _c| {
        state::clear_bias();
        None
    });
}

/// `blend_table` (BlendTable, thiscall with six stack words; eax is 1). The outputs go through the out pointers in
/// the asm's order.
#[test]
fn surface_friction_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x534e70, "surface_friction", false, |mem, c| {
        let (classes, wetness) = ([c.args[1], c.args[2]], [arg_f32(c, 3), arg_f32(c, 4)]);
        let out = on_tyre(mem, c, |v, t| entries::surface_friction(v, t, classes, wetness))?;
        layout::store_surface_friction(mem, c.args[5], c.args[6], out);
        assert_eq!(c.eax, 1, "surface_friction call {}: eax", c.index);
        Ok(None)
    });
}

/// `TyreTail_Get10` (thiscall, no stack words): the scratch's setup pointer (`+0x10`), returned in eax.
#[test]
fn def_pointer_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x5326b0, "def_pointer", |mem, c| {
        assert_eq!(layout::load_def_pointer(mem, c.args[0]), c.eax, "def_pointer call {}: eax", c.index);
        None
    });
}

/// `TyreTail_Get14` (thiscall, no stack words): the scratch's state pointer (`+0x14`), returned in eax.
#[test]
fn state_pointer_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x5326c0, "state_pointer", |mem, c| {
        assert_eq!(layout::load_state_pointer(mem, c.args[0]), c.eax, "state_pointer call {}: eax", c.index);
        None
    });
}

/// `NTweak::CBase::GetName` (thiscall, no stack words): the name object at `this+4`, returned in eax.
#[test]
fn name_object_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay(0x52db10, "name_object", |mem, c| {
        assert_eq!(layout::load_name_object(mem, c.args[0]), c.eax, "name_object call {}: eax", c.index);
        None
    });
}

/// `CTyre::CScratch::CopyDefFrom` (thiscall, one stack word, the setup): the setup copied into the def, the state's
/// copies, and `DamperUpdateB` on the state's terms. The copied words are bit copies; the update may make NaNs.
#[test]
fn copy_def_from_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_tyre(0x534760, "copy_def_from", true, |mem, c| {
        layout::copy_def_from(mem, c.args[0], c.args[1]);
        Ok(None)
    });
}

/// `CTyre::CState::CState` (thiscall, three stack words the constructor does not store, `ret 0xC`): the defaults.
/// It returns `this`.
#[test]
fn construct_state_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x531500, "construct_state", false, map_windows, |mem, c| {
        layout::construct_state(mem, c.args[0]);
        assert_eq!(c.eax, c.args[0], "construct_state call {}: eax", c.index);
        Ok(None)
    });
}

/// `CTyre::CScratch::Init` (thiscall, no stack words): the scratch's defaults, on the scratch the call is made on.
#[test]
fn init_scratch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x5327d0, "init_scratch", false, map_windows, |mem, c| {
        layout::init_scratch(mem, c.args[0]);
        Ok(None)
    });
}

/// `CTyre::CScratch::CScratch(def, state, record)` (thiscall, three stack words, `ret 0xC`): `Init` on the sub-block,
/// the scratch's defaults, and the three pointers. It returns `this`.
#[test]
fn construct_scratch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x5328d0, "construct_scratch", false, map_windows, |mem, c| {
        layout::construct_scratch(mem, c.args[0], c.args[1], c.args[2], c.args[3]);
        assert_eq!(c.eax, c.args[0], "construct_scratch call {}: eax", c.index);
        Ok(None)
    });
}

/// The memory of `CreateScratch`: the recorded windows, then the block it allocated, zeroed as a fresh allocation.
fn map_scratch_block(call: &Call) -> Mem {
    let mut mem = map_windows(call);
    for After { at, len, .. } in &call.after {
        mem.map(*at, &vec![0; *len]);
    }
    mem
}

/// `CTyre::CDef::CreateScratch` (thiscall): the block `operator new` returned (`eax`) holds the scratch after `Init`.
#[test]
fn create_scratch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x535220, "create_scratch", false, map_scratch_block, |mem, c| {
        assert_eq!(layout::create_scratch(mem, c.eax), c.eax, "create_scratch call {}: eax", c.index);
        for After { at, len, bytes } in &c.after {
            assert_eq!(&mem.read(*at, *len), bytes, "create_scratch call {}: block", c.index);
        }
        Ok(None)
    });
}

/// `FUN_005324a0` (fastcall, no stack words): 17 `NTweak::CBase` headers at every 0x10 bytes of the block.
#[test]
fn reset_tweak_headers_17_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x5324a0, "reset_tweak_headers_17", false, map_windows, |mem, c| {
        layout::reset_tweak_headers(mem, c.args[0], 17);
        Ok(None)
    });
}

/// `FUN_00532550` (fastcall, no stack words): 3 `NTweak::CBase` headers at every 0x10 bytes of the block.
#[test]
fn reset_tweak_headers_3_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x532550, "reset_tweak_headers_3", false, map_windows, |mem, c| {
        layout::reset_tweak_headers(mem, c.args[0], 3);
        Ok(None)
    });
}

/// `CTyre::CScratch::CScratch()` (0x532570, the destructor body, thiscall): the defaults on the scratch.
#[test]
fn destroy_scratch_body_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x532570, "destroy_scratch_body", false, map_windows, |mem, c| {
        layout::destroy_scratch_body(mem, c.args[0]);
        Ok(None)
    });
}

/// `CTyre::CScratch::~CScratch` (0x5329a0, thiscall): the scratch's words, its def and state cleared, and the body on
/// the sub-block. The release of the def is a no-op in the original.
#[test]
fn destroy_scratch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x5329a0, "destroy_scratch", false, map_windows, |mem, c| {
        layout::destroy_scratch(mem, c.args[0]);
        Ok(None)
    });
}

/// `CTyre::CScratch::DeleteScratch(flags)` (0x5328b0, thiscall, one stack word): the body on the scratch, returning
/// `this`. The block is freed when `flags & 1`, by the caller; the original's call to free is not compared.
#[test]
fn delete_scratch_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x5328b0, "delete_scratch", false, map_windows, |mem, c| {
        let free = layout::delete_scratch(mem, c.args[0], c.args[1]);
        assert_eq!(free, c.args[1] & 1 != 0, "delete_scratch call {}: flags", c.index);
        Ok(None)
    });
}

/// `CTyre::CDef::DeleteContainer(flags)` (0x535280, thiscall, one stack word): `~CScratch` on the scratch, then the
/// free when `flags & 1`, as `DeleteScratch`.
#[test]
fn delete_container_matches_original_calls() {
    let _pc24 = Precision::Pc24.scope();
    replay_run(0x535280, "delete_container", false, map_windows, |mem, c| {
        let free = layout::delete_container(mem, c.args[0], c.args[1]);
        assert_eq!(free, c.args[1] & 1 != 0, "delete_container call {}: flags", c.index);
        Ok(None)
    });
}

/// `CTyre::CScratch::EmitNoise` (0x535890) with no connections on the tyre-noise signal: nothing is invoked, and the
/// scratch's signal block keeps its first word and loses its second.
#[test]
fn emit_noise_clears_the_signal_block() {
    let _pc24 = Precision::Pc24.scope();
    struct NoConnections;
    impl rbr_physics::physics_core::layout::SignalHost for NoConnections {
        fn invoke(&mut self, _mem: &mut Mem, _conn: u32, _sig: u32) {
            unreachable!("the signal has no connections")
        }
        fn log_recursive(&mut self) {
            unreachable!("the signal is not emitting")
        }
        fn log_range(&mut self, _count: u32) {
            unreachable!("the count is in range")
        }
    }
    let (manager, scratch) = (0x8000, 0x9000);
    let mut mem = Mem::new();
    mem.set_u32(0x0089_3600, manager); // g_signalManager
    let signal = manager + 0x6268;
    mem.set_u32(signal, 0); // the guard, clear
    mem.set_u32(signal + 4, 0x7000); // the count pointer, count 0
    mem.set_u32(0x7000, 0);
    mem.set_u32(signal + 8, 0); // slot 0 has no connections
    mem.set_u32(signal + 0x208, 0); // nor the overflow list
    mem.set_u32(scratch + 0x420, 0x1111);
    mem.set_u32(scratch + 0x424, 0x2222);
    rbr_physics::tyre::layout::emit_noise(&mut mem, &mut NoConnections, scratch);
    assert_eq!(mem.u32(scratch + 0x420), 0x1111);
    assert_eq!(mem.u32(scratch + 0x424), 0);
}

/// A tyre entry on its store.
type Entry<'a> = dyn Fn(&mut layout::TyreView, TyreId) + 'a;

/// The entries in a row on the image between calls (each lifts and lowers the tyre) and on the typed storage over the
/// whole row (lifted before it, lowered after) leave every byte of the record's windows and the tyre's scratch, state
/// and wear equal, on every `WriteOutputs` record.
#[test]
fn the_typed_storage_over_a_chain_keeps_the_per_call_bytes() {
    use rbr_physics::ngp::heat::{Airflow, TyreShape, TyreThermal};
    let _pc24 = Precision::Pc24.scope();
    let Some(calls) = load(VAR, 0x533080) else {
        eprintln!("skipping the typed storage's chain: {VAR} not set");
        return;
    };
    let thermal = TyreThermal {
        air_temperature: 288.0,
        relax_rate: 100.0,
        shape: TyreShape {
            carcass_radius: 0.3,
            tread_depth: 0.01,
            carcass_thickness: 0.005,
            tread_width: 0.2,
            sidewall_height: 0.09,
        },
        airflow: Airflow { kind: 0, spin: 40.0, car_speed: 12.0, boost: None, factors: [6.0, 6.0] },
    };
    let g = grip::GripInput {
        normal_force: 3000.0,
        static_friction: 1.1,
        kinetic_friction: 0.9,
        lateral_stiffness: 40000.0,
        torsional_stiffness: 900.0,
    };
    let copies = layout::SetupCopies {
        vertical_stiffness: 200000.0,
        tread_elasticity: 0.5,
        tread_coverage: 0.7,
        air_pressure: 2.0,
    };
    let x = Vec4 { x: 1.0, y: 0.0, z: 0.0, w: 0.0 };
    let y = Vec4 { x: 0.0, y: 1.0, z: 0.0, w: 0.0 };
    let blocks = (layout::Indentation::from_words([0.01; 7]), [3000.0, 1000.0, 2000.0], [0.5; 8]);
    let dt = 0.004;
    let mut chained = 0;
    for call in &calls {
        let scratch = call.args[0];
        let base = map_frame(call);
        // The entries in a row, as a step and its callers make their tyre calls.
        let entries: [&Entry; 20] = [
            &|v, t| entries::set_active(v, t, 1),
            &|v, t| entries::adopt_setup(v, t, copies),
            &|v, t| entries::set_penetration_depth(v, t, 0.02),
            &|v, t| _ = entries::build_contact_frame(v, t, y, x),
            &|v, t| entries::compute_slip(v, t, [1.0, 0.5, 2.0], 3.0),
            &|v, t| entries::weigh_models(v, t),
            &|v, t| _ = entries::with_contact(v, t, blocks, |c| solve::solve_both(c, 0)),
            &|v, t| entries::soil_drag(v, t),
            &|v, t| _ = entries::hold(v, t, dt),
            &|v, t| _ = entries::limit_to_friction_circle(v, t, g),
            &|v, t| entries::limit_sticking(v, t),
            &|v, t| _ = entries::solve_sliding(v, t),
            &|v, t| _ = entries::contact_patch(v, t),
            &|v, t| _ = entries::solve_forces(v, t, dt, x),
            &|v, t| _ = entries::write_output_block(v, t),
            &|v, t| entries::fill_noise_signal(v, t, x, [1, 2, 3], 2.0),
            &|v, t| _ = entries::heat_step(v, t, 100.0, 200.0, dt, &thermal),
            &|v, t| _ = entries::relax_sector(v, t, 0, dt, &thermal),
            &|v, t| entries::relax(v, t, dt, &thermal),
            &|v, t| entries::relax_and_count(v, t, dt, &thermal),
        ];
        let mut per_call = base.clone();
        for entry in entries {
            layout::with_tyre(&mut per_call, scratch, entry);
        }
        let mut typed = base.clone();
        let mut held = layout::CarTyres::lift_scratches(&typed, [Some(scratch), None, None, None]);
        // The setup held too, as a step's typed definitions hold it.
        let def = layout::TyreImage::of(&typed, scratch).def;
        let mut defs = Defs::default();
        defs.tyres[0] = Some(Held { at: def, def: layout::TyreDef::lift(&typed, def) });
        let mut view = held.view(&typed).with_defs(Some(&defs));
        for entry in entries {
            entry(&mut view, TyreId(0));
        }
        held.lower(&mut typed);
        let image = layout::TyreImage::of(&base, scratch);
        let objects = [(scratch, 0x520), (image.state, 0x140), (image.wear, 0x20)];
        for (at, len) in call.windows.iter().map(|w| (w.at, w.len)).chain(objects) {
            let (a, b) = (per_call.read(at, len), typed.read(at, len));
            let k = a.iter().zip(&b).position(|(x, y)| x != y);
            assert!(k.is_none(), "call {}: {at:#x}+{:#x} differs between the storages", call.index, k.unwrap_or(0));
        }
        assert_ne!(per_call.read(image.state, 0x140), base.read(image.state, 0x140), "call {}: the state", call.index);
        chained += 1;
    }
    assert!(chained > 0, "no WriteOutputs record");
    eprintln!("the typed storage's chain: {chained} WriteOutputs records, every byte equal to the per-call path's");
}
