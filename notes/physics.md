# Physics: the step and exact ports

What a porter needs to keep `crates/rbr-physics` bit-exact against the original under NGP 7.4.778 (ports are of the
1.02 exe's functions with NGP's behaviour; no 1.02-only arm remains). Modules and `ports.csv`:
[crates/rbr-physics/README.md](../crates/rbr-physics/README.md); checks: [verification.md](verification.md); NGP:
[ngp.md](ngp.md). Per-subsystem field tables and asm notes: rbr-recomp `notes/physics/*.md`.

The original's physics is `NPhysics::` in the exe: each component (`CCar`, `CWheel`, `CTyre`, `CEngine`, `CDrive`,
`CSpringDamper`, ...) splits into `CDef`, `CSetup`/`CSetupOptions`, `CState` and `CScratch`, with values set through
the `NTweak` key tables from the per-car files in `physics.rbz`.

## Step

`TicPhysicsRecording` (0x47f420) calls `CPhysics::StepForward` (vtable slot 2, 0x5273a0) once per substep with
`p1 = 1`. `dt = CDef+0x1608c` (`MaxTimeStep`, 0.007 s retail, not 0.01). `CDef = *(g_1659b70+0x24)`. Collision
window `4*dt` (`0x72d150`). Per substep, in order:

1. First call only (`counter == 0`): S48 over list450.
2. `counter++` (CState `+0x18`), `Telem60`; S20 over list450.
3. `Work5500(dt)` (0x525500): all S5C, then all S64, then S6C(dt)+S58(dt) per node.
4. S24 over list450.
5. Unless `SUB_STEPPING` set and `counter & 3 != 0`: if `CHECK_COLL`, `CollideWork(4*dt)`; if `CHECK_PENETR`,
   `PenCheck` and on a hit (`0x4fbca0`) `FBC20`, `FC590X`, `FCA20`, solver S08; then solver S08 again, S54 and S48
   over list450.
6. S98 over list480; `sim_time` (CState `+0x20`, f64) `+= dt`, the sum rounded to f32 at PC=24
   (`physics::advance_time`).

After the loop: player car's `CDrive::Set4510` (0x504510), `g_1659b74++`.

CDef `+0x160cc` flags (exe names, table `0x7cbc10`): 0x10 `CHECK_COLL`, 0x20 `CHECK_PENETR`, 0x1000 `SUB_STEPPING`,
0x8000 `ENABLE_SINGLESTEPPING` (CPhysics `+0x5c` delay, `+0x60` pending). Bit 0x10000 (ground probe) read by the
wheel, 0x80000 by damage and gear logic. Debug `+0x160e0` bit 0x20000 writes the CPhysics `+4/+8/+0xc` counters;
log `+0x160f4` bit 0x4000 (`WHEEL_FORCES`).

`CState::RegWall` (0x5263d0): `S8C != 0` to list480 (`+0x480`), else `S80 != 0` to list450, else list460
(`S88 == 0`) or list470. List nodes: `+0x10` next, `+0x14` prev, `+0x18` list, `+0x60` bit 0x40 skips `Visit470`.

| Slot | CSolid (vtable 0x73fc54) | CCar |
| --- | --- | --- |
| S20 | 0x529200, jmp to child walk 0x51a3a0 | `Vf20x` 0x4e72d0 |
| S24 | 0x519bc0, `ret 4` | `Vf24x` 0x4e58e0 |
| S54 | 0x52b6b0 | `Vf54x` 0x4e7f90 |
| S58 | 0x51a250, child walk | `Hook` 0x4e9930 |
| S5C / S60 | 0x529240 (`E43Vf5C`, then S60 = `Accum250` 0x529250) | `Vf5cx` 0x4e5c60 |
| S64 | `Vf25x` 0x52cb20 | `Vf64x` 0x4e9020 |
| S6C | `Vf27x` 0x52cc30 | `Vf6cx` 0x4ea300 |
| S98 | 0x51a7c0 (unlink, slot 21 on the def) | `Vf98x` 0x4e6dd0 |

`CollideWork` (0x526d20) runs its body once per call (no slices): `Work6a70` detects, `FC500X` takes the minimum
event time (`Get94`, event `+0x94`; starts at 2.0, frees events with flags 0x6000); above 1.1 nothing happens. Else
`fc = clamp(t, 0, 1)`, `Work5640` (ignores its argument) back-steps each contact body (flag 0x4) to its event time,
`FCA20` solves every event with `time - 0.1 <= min(progress A, progress B)`, `Work5760` advances the rest. The dead
`f*0.0` term stays (NaN for inf/NaN `f`).

## x87 and float rules

1.02 physics runs the x87 at PC=24 (control word 0x007f on every captured call; `CreateDevice` at 0x40c418 omits
`FPU_PRESERVE`). 64-bit precision never occurs. Only `E09Timer` (0x465be0, 0x465c90) and two D3D `_controlfp`
scopes switch to 53 bits and restore. Under NGP the race runs at PC=53 (0x27f), the load at PC=24:
`fpu::precision()` is the thread's, set by the host (`fpu::set_precision`, `Precision::scope`).

| Class | Asm | Rust |
| --- | --- | --- |
| S | SSE scalar on f32 | `f32`, same operation order |
| M | one x87 op, then `fst dword` | `f32` at PC=24 (f64 then `as f32` at PC=53) |
| C | x87 chain, st0 carried | f64 with `fpu::round24` (or `round_x87(v)`) after each op |
| R | float returned in st0 | return the chain's type; caller stores or keeps it as the asm does |
| Q | `fsqrt` | correctly rounded at the precision (`f32::sqrt` at PC=24) |
| T | `fsin`/`fcos` (`__CIsin` 0x53f3b0, `__CIcos` 0x53f3a0) | `math::trig` (`X87Ext80`: 64 significand bits, 66-bit Pi reduction, `|x| >= 2^63` unchanged) |

- Precision control rounds arithmetic only: `fld`/`fst` of a double is exact. The x87 exponent range exceeds f64's;
  `round24` stays unbounded within f64, so check any new chain whose values can leave about 1e±308.
- Follow the asm's operation order, not the C++ expression (MSVC reassociates). An SSE result stored to f32 before
  an x87 part is a rounding point.
- SSE NaN and infinity: `math::sse::{add, sub, mul, div}` return the quieted NaN operand (right one first) and
  `0xffc00000` for invalid operations. Use them where an operand can be NaN or infinite.
- `rsqrtss`/`rcpss` are 12-bit table approximations: `math::rsqrt`, `math::rcp` (tables measured on the reference CPU).
- Comparisons copy the asm's branch sense so NaN goes the same way: `!(a > b)` is not `a <= b`; `comiss` + `jbe`
  is taken when unordered (`math::le_or_unordered`). No `f32::clamp`, `min`, `max` on values that can be NaN.
  `math::abs_by_subtraction`, `math::sign` copy the asm forms.
- Float to int: never `as`. `fpu::trunc_i32` (`cvttss2si`), `fpu::round_i32` (`cvtss2si`, `fistp`) give
  `INT_INDEFINITE` (`i32::MIN`) for NaN and out of range; `math::ftol::trunc_i64` is `_ftol2` (0x70c2c4, low word of
  the int64).
- Constants keep the exe's bit patterns (some pools differ from the literal, e.g. 0x3d4cccd0 for 0.05). A punned
  word (float bits in an int) stays `u32`; build floats with `f32::from_bits`.
- Default NaN sign: an arm64 host makes `0x7fc00000` where the original wrote `0xffc00000`. Tests accept "NaN for
  NaN" only at named sites: `tyre_vectors` (`same_words`: solvers, `patch_forces`, `sliding`, `contact_patch`,
  `solve_forces`, `hold`, `heat_step`, `relax_sector`, `relax`, `friction_curve`, `write_output_block`),
  `car_body_vectors` (`same_word`: `to_quat`, `steering_ffb`), `entities_solve` (`+0x624`), `entities_slip`,
  `spring_damper_vectors` (st0 returns, `check_spring_windows`), `engine_driveline_vectors` (`check_st0`,
  `check_windows_nan`, `sim_e660` for `torque`, which a NaN `load_d` makes NaN: x86_64 writes `0xffc00000` where the
  record holds `0x7fc00000`), `wheel_vectors` (`split_on_sub`, sign only). Recorded st0 NaNs carry no sign
  (`harness/vcu_vectors.py`). `harness/rust_step.py` excludes words NaN in both runs that differ only in bit 31. A
  new site gets its own entry, never a blanket tolerance. The vector targets pass on arm64 (CI's macOS job) and on
  x86_64.
- Stack residue is behaviour. Callees that leave arguments on the stack feed the next call: the slot-12 identity
  getter (0x519c00, `mov eax, ecx; ret`) keeps its pushes for the following `ComposeA`; `MB560` (0x51b560) and
  `ComposeA` (0x51b4c0) are thiscall `ret 0xc` with three arguments, some callers reuse earlier pushes, and
  `CSolid::E43Vf6C` leaves three words for 0x51b4c0. Unwritten lanes (a w lane after a three-float write, then a
  four-lane `CopyFrom`) hold stack garbage; tests exclude those words by name. Audit (drive2 attempt 5, 84084
  steps, pure dispatch; branch `audit/stack-residue`, feature `residue-audit`): the only argument-word reuse is
  `E43Vf6C`'s call of `ComposeA` (0x51b4c0) with the three words the slot-12 getter (0x519c00) left (4 per
  step); filling the dead stack changes only `STALE_CAR` and `HEAP_WHEEL` words, all from the unwritten word at
  `StepForward`'s slot-0x94 (esp-60 of `E38Ops::M_C1A0`, 0x4fc1a0) and slot-0x74. Not audited: `--dispatch
  direct`, other drives.

## Memory and porting rules

- Ports are pure functions over typed values; raw offsets and `Mem` only in the subsystem's `layout.rs` or
  `layout/`. `Mem` panics on an unmapped byte; a skipped read or a write outside the recorded windows is caught only
  by reading the asm.
- Block copies use `Mem::copy` (a `memmove`, read then write) or `Mem::copy_words` (forward `rep movsd`);
  halfwords `Mem::u16`. On pages they are one range check and a word loop (tanner boot: 7 % fewer instructions).
  The page marks stay: with unmapped reads as zero only 1178 of the tanner boot's 13000 steps are equal.
- Grep `src/` for the address before porting: a ported function is called, not copied (pure math in `src/math/`).
- Another subsystem's callee is called through its typed function when it has a `ports.csv` row, else it is a host
  method (one per callee, address in the doc comment). A subsystem's own functions are ported, not hosted.
- The `ports.csv` row goes in the port's commit. No rows (no records): 0x51c810, 0x51be56, 0x51bf3c.
- Pure-Rust gaps: tyre `Step`, `Locate`, `HeatStep` (0x534090) read tables at indices outside the recorded window
  (the binding returns `Outside` and runs the original); pure Rust needs the def's raw words there.
- The asm decides, not the C++: where the reconstruction has a bug (uninitialised arrays in the E38 ray worklist,
  indeterminate scales in `E36Collect`), port what the original does.
- Dispatch slots and signal emits go through a host trait in the original's call order; tests check that order.

## Standalone dispatch

The boot maps the 1.02 exe's `.rdata` and `.data` as zeros and over them the words a run reads before writing them
(`rbr_formats::exe_data` with the season table, the renderer's and the tyre sound's tables), no `.text` or
patch array; NGP callees and boot settings are in [ngp.md](ngp.md) (no exe code mapped; the load budget is NGP's 1500).
The words come from `exe-reads` builds (`cargo build --release -p rbr-standalone --features rbr-physics/exe-reads`, a
run with `RBR_EXE_READS=FILE` writes them; `scripts/gen-exe-data.py --reads FILE --write` merges them into
`data/exe-words.txt`): drive2 attempt 5 and 140 autopilot boots (8 retail cars, 16 stages, weathers, damage, a
championship stage). A port that reads a word not listed reads 0: rerun the sweep after porting new exe reads. A run reads
only the virtual-table slots it calls, so the generator takes every virtual table whole (`whole_vtables`): the 937 the
code installs (their addresses are `.text` operands) and each one a listed word lies in, 13522 words in all.
CMountedRigidBody's table (0x73e62c) had 9 of its 95 slots and its scratch's (0x73fb6c) none: a mounted part coming off
(`StepBig`, 0x522100) stopped the physics with `unbound 0x0`, then, with the first table whole, with `E47_Vf54Helper`
(0x52b6b0) declining the scratch's zero slot 1 (El Cubilete, live drives, 2026-11-03). drive2 attempt 5 stays physics
PASS 84084/84084.
Instruction operands are decoded only for captured-memory validation.

## Store seam

The standalone gameplay adapter samples a typed `CarState`; backend memory reads, gear lookup, tyre-noise
attribution and control encoding stay in `rbr-standalone/src/physics/layout.rs`. Live and recorded samples use the
same conversion.

One physics code over two storages (Phase B). A subsystem's store trait names its layout's load and store groups;
`Mem` implements each with the layout's bodies. Sequence functions are generic over the trait (`fn step<S:
SpringStore>`), never `dyn`, so the `Mem` path compiles as before.

| Trait | File | Mem impl |
| --- | --- | --- |
| `store::Raw` (supertrait: words, `mem()`) | `store/mod.rs` | same |
| `SpringStore` | `spring_damper/store.rs` | `spring_damper/layout.rs` |
| `VcuStore` | `vcu/store.rs` | `vcu/layout.rs` |
| `TyreStore` | `tyre/store.rs` | `tyre/layout/kernels.rs` |
| `WheelStore` | `wheel/layout/store.rs` | `wheel/layout/fields.rs` |
| `CarStore` | `car_body/store.rs` | `car_body/layout/car.rs` |
| `EngineStore: SpringStore` | `engine_driveline/store.rs` | `engine_driveline/layout/mem_store.rs` |
| `RigidStore` | `rigid_body/store.rs` | `rigid_body/layout/mem_store.rs` |
| `CoreStore` | `physics_core/store.rs` | `physics_core/layout/mem_store.rs` |
| `HingeStore`, `EntityStore: HingeStore` | `entities/store.rs` | `entities/layout/hinge_state.rs`, `layout/store_mem.rs` |
| `StageStore` | `stage/store.rs` | `stage/layout/queries.rs` |
| `TweakStore` | `ntweak/store.rs` | `ntweak/layout/mod.rs` |
| `CollisionStore` | `collision/store.rs` | `collision/layout/mem_store.rs` |

- A calls trait takes the store as a parameter defaulting to `Mem` (`Vf6cxCalls<S = Mem>`); clashing names use
  UFCS; what a trait does not name yet goes through `Raw`. Constructors and setup copies stay on `&mut Mem`.
- `object!` (`store/layout.rs`) tables an object once (struct, `Field` per row, `lift`/`lower`); tabled so far
  (`store::car_objects`): tyre scratch, strut and state, VCU scratch and state.
- Mem-direct dispatch (`--dispatch direct`, live default): a call within the `IN_PLACE` subsystems
  (`rbr-hybrid/src/pure/mod.rs`) to a leaf (`pure/leaves.rs`, from `scripts/pure_leaves.py`) runs as a plain call
  when it passes exactly the words the leaf pops; a non-leaf binding too (B1b, `Pure::call_plain`, `call_framed`:
  words written as the host writes them, nothing read back, own frame, image word for word the host path's;
  `args` holds the words it pops, `Pure::stack_words`). Watched emits and tyre steps always go through the host
  (`NOT_DIRECT`). `Host::call_direct` leaves the words unwritten where the callee reads only its arguments (ground
  probe's callees, `E45_M40C0`'s `LookupTwin`, CRT vector iterators). Bindings get `Direct` (`pure/mod.rs`);
  `Pure`'s own `Host` is the fact-check's (`--dispatch pure`). 176-step NGP bench: 739.8e9 to 678.3e9
  instructions.
- Checks: `round_trip` (`rbr-hybrid`), `rbr-standalone --twin mem|mem-direct`, the `poison` feature.
- The generic sequences sit outside the layouts (`rigid_body::{solid_rows, solid_contacts, solid_window}`,
  `physics_core::{step, contact_loop, passenger_step, pose, emit, mounted, accumulate}`, `entities::hinge_step`,
  `ntweak::plan`, `collision::{solver_pass, e36, events, body_contact, grid_edge, mesh, wall, body}`), re-exported
  from `layout` under their old paths; object offsets and frame constants stay in the layout. Evidence: drive2
  attempt 5 physics PASS 84084/84084, the 4 hybrid snapshots identical.
- Stay on `Mem`: constructors and setup copies everywhere; `load` and stage's load code (image construction only);
  code writing at computed addresses in the original's stack frames (physics_core detector walks, `notify`,
  `prof_calls`; entities `collide_init`, detach events, `vf_e070`); rigid_body wake, sleep, `Vf158` (via
  physics_core's Mem-typed `FilingHost`); NTweak apply and parse (host-bound); collision cell grid, links, pools,
  `col_grids`, `brake_wall_vf_f0` (calls `car_body::light::sub_block_init`).
- Left: how a non-`Mem` store hosts the original's stack frames; `--twin` in the live run.

## Physics core

`src/physics_core/` (model per quantity in the file headers; everything touching memory or calling the original in
`layout/`; logs in `src/game_log/`, table search in `src/math/table.rs`). Detector vtables `PenCheck`/`Work6a70`
take `0x73f5b4`/`0x73f7b0` (structure in `layout/detector_calls.rs`). Handler-stack base `+0xe848` (top `+0xe888`,
-1 empty). The row/column helpers are `mul_rows`/`transform_xyz`, not the exe's `XformRows`/`MulVec` names. The emit
guard's `int3` sits at entry + 0x3d. `game_log_vectors`: 4000 direct calls.

## Rigid body

`src/rigid_body/` (`layout/` offsets; contact record in `layout::contacts::load_contact`; behavior per function in
the file docs: `force.rs`, `integrate.rs`, `impulse.rs`, `contact.rs`, `contact_list.rs`, `window.rs`). The step's
S60/S64/S6C are slots 25/27 (code: `Vf25x`/`Vf27x`, no S-numbers). Flag 0x4 at `+0x60` is the parent-updates-child
bit, not a "contact body" name.

## Car body

`src/car_body/` (module per function in the file headers; offsets and callee addresses in `layout/`; `ports.csv`
maps each address). `Xform6870` calls `ComposeA` with no pushes of its own.

## Jointed bodies

Hinged model per quantity in `src/entities/hinge_*.rs`, offsets in `layout/`; mounted and passenger in
`src/physics_core/`. All sizes, offsets and rules in the file docs.

## Wheel

`src/wheel/` (quantities in the model modules, memory and calls out in `layout/`; CDef-key reals in `refresh.rs`
and `refresh_pose.rs`, NGP damage keys in `data/ngp-lsp-keys.md`). The asm has no int conversions. Unicorn picks
between two NaNs the x87 way, so its records differ from `math::sse` in NaN sign only.

## Tyre

`src/tyre/` (stage order and name map in `entries.rs`, record layouts in `layout/`). `F[0x7A]` is multiplied by `e3`
twice; `F[0xC5]*0.1` rounds before `*F[0xFC]`.

## Spring-damper

`src/spring_damper/` (lifecycle in `suspension.rs`, kernels in the model docs, `Mem` edge in `layout.rs`; damper
events via `Host::emit_damper`). `CSpringDamper`'s fade pointer `+0x10` is read without a check; its writer is not
found.

## Engine and driveline

`src/engine_driveline/` (model in `engine.rs`/`drive_line.rs`/`sim.rs`, drive-line table in
`layout/drive_line.rs`, offsets in `layout/`). `Set4570` (0x504570): a NaN lock takes the upper clamp and stays
NaN. `SimE010` sets a cut flag when `x <= limit` or unordered. `Sim8E20` (0x518e20) returns the clamped `b` in st0
(callers multiply by `0x73a2e0`). Clutch `M93A0` (0x5093a0): lock and slip with latch `+0x368`, slip `+0x60`.
`CDriveLine::Sample` (0x50b180): the first entry from 1 whose distance is above `d` (`comiss`+`ja`, so NaN finds
none) gives `BlendB` at `(d - d0) / (d1 - d0)`; otherwise the last entry's position (`this - 0x20` when empty).
The `test eax,eax` after a hit is dead (`k >= 1`, direct-call branch cover).

## VCU

`src/vcu/` (`CScratchSim::Step` 0x537f50 sequence, tables and locks in the file docs). RSF drive2 attempt 5 parted
at its first launch without PC53 rounding of the pedal and speed tables.

## Ground and surfaces

`src/entities/ground.rs` (queries to the host at `g_7c755c`; `VfE8` 0x51c6f0 record; `CollideInit` 0x51bd30 second
pass on bit 3 of `bodyA+0x60`).

## Collision and contacts

`src/collision/` (geometry in physical terms; game memory, calls and sequencing in `layout/`) and
`src/rigid_body/` contact files. The wall table is read eight floats at an 8-byte stride (original; keep).
`Impulse640` (0x529640): 4th argument a time (`dt`), 3rd unused. `E36SphereBox` sums `dz + dy + dx` in x87.
`BoxHit` sets only `AL`.

## Entities

`src/entities/` (hooks, `E90SegPool`, walks in the file docs). `E90SegPool::Init` (0x601270) owns a `new[]` array
with the count word before it. Walks are pre-order over children whose `S80() != 0`.

## Scenery

Scenery collision comes from the `.trk` collision meshes (kind `0x16`, loader 0x46e300), not `.col`, emitted at
load as hard collision. Placements to solids in `stage/layout/placement.rs` (`E46_Notify67d0` 0x5267d0,
`E39_EntfCreate` 0x502070; index = `physics.lsp` `DefManager` order: 1 `GENERIC`, 13 `STREET_SIGN`, 21
`BANNER_FENCE`, 24 `PIG`, 25 `TRAFFIC_CONE`, 28 `LOG`, 30 `LOG_POLE`, 31/32 wooden fence bar/pole). `GENERIC`
has inverse mass 0 and never wakes; props sleep in list460 until a contact wakes them (`E38_004FCA20` 0x4fca20),
then emit `CSigSolidGeneralState` from `CSolid::Vf28` (0x52c620), id = handle `0x9c40` + the `.lbs` (1,6) record
index. A contact's surface is the mesh material byte (`0x79` Bendable_Tree, tested by `E37Flag4AC0` 0x4f4ac0).
Surface wetness: the `.mat` entry named with the race's wear and wetness words maps cells to material tags (wet tarmac
37/38/39 dry/damp/wet); `CWheel::Unk9900` passes each tag's class (`entry+0x148`) and `WaterFactor` (`entry+0xe8`, 0
dry to 1 wet) to `CScratch::BlendTable` and blends the same `+0xe8` into the tyre's copy (`scratch+0x288`), so there is
no second wetness. RSF 122 / 117 wet: tags 39 / 71, WaterFactor 1. 12 RSF stages (457 460 468 479 480 553 554 564 568
569 584 585) have no wet entry: wet builds no table and every cell is tag 0 (μs 0.95, class 0, WaterFactor 0: on the tarmac tyre, class base factor 0.90,
a static 0.83 after the temperature grip) (`--tyre-probe`, `materials` example, 2026-11-08).
Fences (`.fnc`) are cloth and never touch the car.

## Damage

`src/car_body/damage*.rs` (offsets, functor tables, levels and clamp in the file docs). Four table values at
`+0x2a88..+0x2b90` are 0.1f: `load_defs::damage_ops_tables_match_lcb1_load_dump`.

## Tweak keys

The `.lsp` keys per class: [data/physics-lsp-fields.md](../data/physics-lsp-fields.md); the exe's NTweak key tables
come from rbr-recomp's `scripts/tweak-keys.py`.

`CBase::Parse` (0x52f0c0) runs at load (setup 0x48d940, tyres 0x48d9f0, `E46_InitPhysics` 0x5266d0, file emits of
`CMultiSignal1<char *>` 0x47e840), in the season (0x5d6310, 0x5d6450) and in the tweak menus (bit-field menu
0x530233). The load builds from the reader's items (`ntweak::plan`); the hybrid binding walks the token tables
(`ntweak::layout::parse`): each group, and each inner or member group again, is cut in place by `Tokenize` into a
0x4000-byte `operator new` table freed after it; its name and items go to the token heap (`E11_StackAlloc`,
which logs and traps when full). The class is the object's vtable (RTTI; `CLASSES` in `rbr-hybrid` `ntweak.rs`); the
season's two classes have no key table and decline. Five lp-snap0b emit dumps and the h1 setup dump are identical
with no declines (253 calls, 2026-10-20).

## Coverage gaps

The reference drive (tanner-corolla, damage 1) reaches 792 of 2,651 physics functions; unreached: damage and crash
paths (wall penetration, retire, detach, `Impulse640`), VCU aids (off on that car; `harness/vcu_vectors.py --fuzz`),
load and teardown (`tests/load_*.rs`), snow and tarmac data, replay and ghost paths, NTweak menus, about 170
functions with no callers. Branches no drive reaches get fuzzed direct-call records (`harness/vector_specs/`), else
the port is `partial` in `ports.csv`. A binding no drive reaches is `identical` in `bound.csv` once its records take
every conditional jump both ways (`harness/branch_cover.py`), apart from the original's traps: a signal index at or
past 0x20 (`CMultiSignal1::Emit`, int3) and a drive-line index at or past the count (`CDriveLine::LerpAt`, int3;
`Split17C`, `Vf98x`, `Test7910` decline there). Callees a recipe must steer (the vote's miss, slot 36 clearing
`+0x274` in `Vf180`) return at once without running in a third of the calls (`proofs_respawn.py`).
