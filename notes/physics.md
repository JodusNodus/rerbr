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
  record holds `0x7fc00000`), `wheel_vectors` (`edge_force`, sign only). Recorded st0 NaNs carry no sign
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
TyreStore>`), never `dyn`, so the `Mem` path compiles as before.

| Trait | File | Mem impl |
| --- | --- | --- |
| `store::Raw` (supertrait: words, `mem()`) | `store/mod.rs` | same |
| `StrutStore` (handle `Strut`), `DamperBlocks` | `spring_damper/store.rs` | `StrutView` only; `DamperBlocks`: `spring_damper/layout.rs` |
| `VcuStore` (one unit, no handle), `DriveBlocks` | `vcu/store.rs` | `VcuView` only; `DriveBlocks`: `vcu/layout.rs` |
| `TyreStore` (handle `Tyre`) | `tyre/store.rs` | `TyreView` only; the callers' blocks: `tyre/layout/kernels.rs` |
| `WheelStore` (by `Slot`; the image's words as default methods) | `wheel/layout/store.rs` | `wheel/layout/fields.rs` (every field on the image, per call); `WheelView` (typed) |
| `CarStore` | `car_body/store.rs` | `car_body/layout/car.rs` |
| `EngineStore: DamperBlocks` | `engine_driveline/store.rs` | `engine_driveline/layout/mem_store.rs` |
| `RigidStore` | `rigid_body/store.rs` | `rigid_body/layout/mem_store.rs` |
| `CoreStore` | `physics_core/store.rs` | `physics_core/layout/mem_store.rs` |
| `HingeStore`, `EntityStore: HingeStore` | `entities/store.rs` | `entities/layout/hinge_state.rs`, `layout/store_mem.rs` |
| `StageStore` (the drive line, the material table and its sheets) | `stage/store.rs` | `stage/layout/queries.rs`; `StageView` (typed) |
| `TweakStore` | `ntweak/store.rs` | `ntweak/layout/mod.rs` |
| `CollisionStore` | `collision/store.rs` | `collision/layout/mem_store.rs` |

- A calls trait takes the store as a parameter defaulting to `Mem` (`Vf6cxCalls<S = Mem>`); clashing names use
  UFCS; what a trait does not name yet goes through `Raw`. Constructors and setup copies stay on `&mut Mem`.
- `object!` (`store/layout.rs`) tables an object once (struct, `Field` per row, `lift`/`lower`); tabled so far
  (`store::car_objects`): tyre scratch, strut, its state and fade pair, VCU scratch and state.
- Mem-direct dispatch (`--dispatch direct`, live default): a call within the `IN_PLACE` subsystems
  (`rbr-hybrid/src/pure/mod.rs`) to a leaf (`pure/leaves.rs`, from `scripts/pure_leaves.py`) runs as a plain call
  when it passes exactly the words the leaf pops; a non-leaf binding too (B1b, `Pure::call_plain`, `call_framed`:
  words written as the host writes them, nothing read back, own frame, image word for word the host path's;
  `args` holds the words it pops, `Pure::stack_words`). Watched emits and tyre steps always go through the host
  (`NOT_DIRECT`). `Host::call_direct` leaves the words unwritten where the callee reads only its arguments (ground
  probe's callees, `E45_M40C0`'s `LookupTwin`, CRT vector iterators). Bindings get `Direct` (`pure/mod.rs`);
  `Pure`'s own `Host` is the fact-check's (`--dispatch pure`). 176-step NGP bench: 739.8e9 to 678.3e9
  instructions.
- Checks: `round_trip` (`rbr-hybrid`), `rbr-standalone --twin mem|mem-direct|typed`, the `poison` feature.
- The generic sequences sit outside the layouts (`rigid_body::{solid_rows, solid_contacts, solid_window}`,
  `physics_core::{step, contact_loop, passenger_step, pose, emit, mounted, accumulate}`, `entities::hinge_step`,
  `ntweak::plan`, `collision::{solver_pass, entry, events, body_contact, grid_edge, mesh, wall, body}`), re-exported
  from `layout` under their old paths; object offsets and frame constants stay in the layout. Evidence: drive2
  attempt 5 physics PASS 84084/84084, the 4 hybrid snapshots identical.
- Stay on `Mem`: constructors and setup copies everywhere; `load` and stage's load code (image construction only);
  code writing at computed addresses in the original's stack frames (physics_core detector walks, `notify`,
  `prof_calls`; entities `collide_init`, detach events, `build_hinge_axes`); rigid_body wake, sleep, `Vf158` (via
  physics_core's Mem-typed `FilingHost`); NTweak apply and parse (host-bound); collision cell grid, links, pools,
  `col_grids`, `brake_wall_contact` (calls `car_body::light::sub_block_init`).
- Typed storage (B2), the strut first: `spring_damper::strut::Struts` (per wheel: the controller's mode, latch and
  signal, the three effective dampings, the fade pair; 12 words a strut) behind `StrutStore` keyed by `StrutId`; the
  image side is `layout::{StrutImage, CarStruts}` (`lift`/`lower` on the `object!` fields, `FadeObj` the fade pair).
  The definition, the headers and every pointer stay in the image, read per call. Per call (`layout::with_strut`):
  the emulator's bindings, the vectors, `Pure::reset_strut_dampers`, another car's strut. Over a step
  (`Pure::typed_struts`, the default): `Playback::step` lifts the car's struts before `StepForward` and lowers them
  after (a stop too), the bindings take them through `Host::struts`, so every reader between steps (telemetry, SND1,
  FRM1, the twin, a restart's `restore`) sees the image's words. NGP's strut words (`strut+0x30..0x38`, the owner
  wheel's `+0xe80..0xe8c`) and the step's out pointers stay the binding's, on the image. Evidence: drive2 attempt 5
  84084/84084 typed; `--twin typed` (against the per-call path) equal after each of 84084 steps; the `poison` build
  (`--features rbr-hybrid/poison`, the held words poisoned in the runs' memory) 84084/84084; `round_trip` 192 typed
  bytes equal on 43 snapshots; `tests/pure_struts.rs` both storages equal to the original and to each other.
  The hybrid gate's 1.02 snapshots run no strut call but the kernels and `GetName` in 30 steps (`Step` declines).
- One typed storage for the car: `store::typed::CarTyped` (the struts in a `RefCell`, their runs never nest; the VCU
  in a `Cell`, FlagDispatch calls the slip query, a VCU binding, through the host), lifted and lowered together by
  `Pure::typed` (was `typed_struts`) around `Playback::step`, reached by the bindings through `Host::typed`, which
  holds `store::typed::Typed` (the car's and the stage's).
- The VCU, second: `vcu::layout::Unit` (the scratch's pedals, fixed clutch, carried word, launch and slip fields; the
  state's lockout, launch clutch with its dirty word, pedal swap, gear timer and cooldown; 16 words) in `CarVcu`
  (`VcuImage`, `lift`/`lower` on `ScratchObj`/`StateObj`, `with_vcu` per call, `round_trip`). Definition, pointers and
  the owner car stay in the image, read per call. The drive's gear block, outputs (CReals, lock torques), gear count
  and ratios are the drive's (its step and setters read them in the same step), so `DriveBlocks` writes them on the
  image; `CDrive::M6840` stays a `Mem` function. `VcuCalls` keeps every callee a host call (drive, engine, car,
  lights untyped; the slip query because the original's call places its stack words). CopyDef's maps and ReadPacket's
  packet are read and written on the image, their scratch fields through the store; ReadPacket's run is lowered
  before FlagDispatch's, so the per-call slip query lifts it. Readers outside the bindings during a step: NGP's
  engine wrapper (the cooldown, `state+0x64`) and drive wrapper and curve term (the handbrake, `car+0x864`), through
  `rbr_hybrid::vcu::unit_of` (an unrouted read fails the poison run: `vcu gear timer (+0x4)` in `0x5273a0`).
  Evidence: drive2 attempt 5 84084/84084 typed; `--twin typed` equal after each of 84084 steps; poison 84084/84084;
  `round_trip` 64 typed bytes on 40 snapshots; `vcu_vectors` on the per-call store (records' unmapped unit bytes
  zero-filled; CheckSlip's drive+8 mapped to its window), and the chain test (8 entries per-call vs held, every byte,
  240 Step records); hybrid gate 13/13 runs Step and ReadPacket per call (drive2 has auto gears and clutch help off,
  so its gear logic and launch clutch return early); +0.14 % instructions per step.
- The tyres, third: `tyre::tyres::Tyres` (per wheel a `Tyre`: the scratch's contact, material copy, patch, forces,
  slips, noise signal, outputs, heat and soil words and the tread sectors' wear; the state's active flag, relaxation
  counter, start flag, deflections, radius, setup copies, inner air, engaged flags, hold direction, searches,
  temperatures and sector; 992 bytes a tyre) in `CarTyres` (`layout/typed.rs`: `TyreImage` lift/lower, `with_tyre` per
  call, `round_trip`). The setup (`m_def`), the pointers and the wheel's contact angle (`wheel+0x5ac`) stay in the
  image, read per call; a wear pointer of 0 holds no wear. The store loads and stores a whole `Frame`; the entries
  (`layout/entries.rs`) take the callers' blocks as values and return their outputs, which the bindings write after
  the tyre, so a pointer into the tyre's own words keeps its write (ComputeOutputs' record at `this+0x20` is the
  tyre's stiffnesses). Ten scratch scalars are words of the material block: lower writes the block then the scalars,
  and the view's store settles the block (`settle_material`), so the held state is lower∘lift's. Indices past a window
  (heat, sector, surface wear, the limits' blend) decline with `Outside`. EmitNoise lowers a held tyre around the emit
  (its connections read the block at its address); CopyFrom on a held tyre's `+0x1a0` goes to its store. Readers
  during a step, through `rbr_hybrid::tyre::read_tyre`: NGP's step wrapper (the speed), the thermal words (in contact,
  the boost `+0x1c8`), the ground probe's radius (`GroundCalls::effective_radius`), the pure host's tyre term; an
  unrouted read fails the poison run (`wheel0 tyre forces (+0x3c)` in `0x791fb660`, the wrapper's speed). Locate,
  TyreClamp, FloorS32, TyreFrictionCurve, Set414 and the getters stay on the image (the callers' words). Evidence:
  drive2 attempt 5 84084/84084 typed; `--twin typed` equal after each of 84084 steps (aids off and on); poison
  84084/84084; `round_trip` 3968 typed bytes on 43 snapshots; `tyre_vectors` on the per-call store (frame records'
  unmapped footprint zero-filled) and the chain test (20 entries per call vs held, every byte, 1020 WriteOutputs
  records); hybrid gate 13/13 (the 1.02 snapshots run the original Step, which calls the sub-entries per call);
  +0.54 % instructions per step.
- The stage, fourth, read-only: `stage::typed::Stage` (the drive line `DRIVE_LINE` names: its 10000 entries of 0x30
  bytes and the count, `line+0x10..+0x75314`; `MATERIAL_TABLE`'s 256 slots, slot address and count, `+0..+0x808`, and
  the 256-byte sheet of each slot with one), one per loaded stage, not per car: `Pure` lifts it at the first step
  (`OnceLock`) and puts an `Arc` of it in each step's `Typed`; nothing lowers it. `layout::StageView` is `StageStore`
  over it, the image for a read it does not hold (past the table, another address); without a step (emulator, vectors,
  between steps) the readers run on `Mem`'s. Readers: E12_SampleX, SampleBig, BlendB, LerpAt, Last, ClampMix,
  `Test7910`, `Vf98x`'s and Split17C's samples, NGP's sampler, `E44MatTable::M520240` and NGP's sheet lookup, through
  `rbr_hybrid::stage::on_stage`; `SetDriveLine` and `Build` (load) write it on the image. Unchanged by a step: the
  poison run (an unrouted E12_SampleX fails: `drive line (+0x75300)` in `0x475ea0`); `.col` image, sheets, table and
  drive line hash equal before and after drive2, while NGP's first collision-system copy changes at `+0x1014..+0x1024`
  (the queries' found list: collision state, not the stage's). The poison build checks each typed answer against the
  image's. Evidence: drive2 84084/84084 typed; `--twin typed` equal after each of 84084 steps (also aids on, and wet
  with realistic damage); poison 84084/84084 and on RSF 122/117 wet (60000 autopilot steps each); `round_trip`
  484876-491532 stage bytes and `differential` (every entry, slot and texel, 624196 answer bytes) on 40 snapshots and
  `load_stage`; perf -0.23 % instructions. The hybrid gate runs E12_SampleX, Thunk4F0 and SampleBig per call on the
  image; M520240 declines there (NGP's). Not typed: the collision system and its copies, the `.col` tree, the mesh
  grids and water meshes (the collision queries, heap graphs; their scratch is written per query); `CollisionQuery`
  is NGP's runtime copy (no image word) or the snapshot's operand.
- The wheels, fifth: `wheel::layout::Wheel` (`object!` with a `Slot` per field, `layout/typed.rs`; 444 bytes a wheel):
  the pose the refresh and the axes leave (deformed top mount, steering rod length, wishbone copy, wobbled and inclined
  axle, strut direction, tip, inverse arm and rack, drive axle direction, rod mount, steering rod, debug rows), the walk
  output, the probe's frame axis, contact distance, sector values, sweep residue, ground body and copies (`+0xdb0..
  +0xe30`, the two edge copies `Blend4`'s), the edge force's across vector, the step's strut velocity, spin, four forces
  and end spin, the damage-changed flag. `CarWheels` (lift, lower, owned, `round_trip`) is in `CarTyped`, lifted and
  lowered with the rest. The wheel's runs nest through the host (the step calls the refresh, the probe, the edge forces,
  the top mount, the hub torque and the spin cross; the probe the sector lerps, the contact pick, the tag blend), so
  the state is a `RefCell` borrowed one field at a time, and there is no per-call lift: `WheelView` holds the step's
  wheels typed and keeps any other wheel on the image, field by field at its address (the `Mem` store is the same,
  for the vectors), which is the per-call path (emulator, hybrid gate, between steps). The store keys a wheel by its
  address: per call that is the image's word, held the view finds the wheel. Stay in the image, read and written there:
  the definition, the pointers (car, def, damage, tyre setup, wheel index), the damage record (the car's damage object,
  `+0x3c04 + 0x38k`, its functors and `ResetDamage`), the solid's words (position, orientation, velocities; the refresh
  writes them), the reals' headers (`SetDirty`), and every word another subsystem reads or writes at its address: the
  wheel angle and strut length (the tyre's contact angle, `Snap9360`), the axle and knuckle axis (the car's transforms),
  the world hub, axle, probe direction, carcass edges and hub velocity (the car's walks, the CRT vector helpers, the
  collision casts), the car force and scratch (the car's accumulators), the blend ops and last tags (the tyre step's
  pointers, NGP's step wrapper), the ray blocks (the mesh queries' hit words, NGP's tag blend), the ground flag
  (`Vf6cx`), NGP's strut words `+0xe80..+0xe8c`. The calls into the tyre and the strut stay host calls: they take
  pointers into the step's frame and the image words above, NGP rewrites three of them (the tyre step, the spring
  step, the relaxation), and the host writes the original's stack words around each. Readers
  during a step, through `rbr_hybrid::wheel::read_wheel`: NGP's engine wrapper (the rim speeds, under the engine
  model) and the tyres' thermal words (the end spin); an unrouted read fails the poison run (`wheel0 end_spin_rate`
  in `0x5358c0`). Evidence: drive2 attempt 5 84084/84084 typed; `--twin typed` equal after each of 84084 steps (also
  aids on with realistic damage); poison 84084/84084 (both); `round_trip` 1776 typed bytes and `differential` (the
  host-free entries in a row per call against held, every byte of the wheels, damage records and outputs, 21856
  bytes) on 40 snapshots; `wheel_vectors` on the Mem store; the hybrid gate 13/13 (the 1.02 snapshots run the original
  step, which calls the refresh, probe, sector lerps, `Blend4`, the hub torque and the spin cross per call; `M3DD30`
  and `M3D460` decline there); +0.73 % instructions per step.
- Left: how a non-`Mem` store hosts the original's stack frames; `--twin` in the live run; typed state across steps.

## Physics core

`src/physics_core/` (model per quantity in the file headers; everything touching memory or calling the original in
`layout/`; logs in `src/game_log/`, table search in `src/math/table.rs`). Detector vtables `PenCheck`/`Work6a70`
take `0x73f5b4`/`0x73f7b0` (structure in `layout/detector_calls.rs`). Handler-stack base `+0xe848` (top `+0xe888`,
-1 empty). The row/column helpers are `mul_rows`/`transform_xyz`, not the exe's `XformRows`/`MulVec` names. The emit
guard's `int3` sits at entry + 0x3d. `game_log_vectors`: 4000 direct calls.
The step's node slots by when it calls them (names ours): S20 `PRE_STEP_SLOT`, S24 `POST_BODIES_SLOT`, S48
`INIT_PASS_SLOT`, S54 `POST_CONTACT_SLOT`, S98 `END_STEP_SLOT`; the solids' S80 `TAKES_PART_SLOT`. The mounted pull
subtracts the definition's `vecGravity`: its key at `+0x160a0`, the vector at `+0x10` of it, the `+0x160b0` the
accumulate reads (`CoreStore::gravity_key`). The contact loop's hit counts read `DbgFlags` (`+0x160e0`, bit 0x20000).

## Rigid body

`src/rigid_body/` (`layout/` offsets; contact record in `layout::contacts::load_contact`; behavior per function in
the file docs: `force.rs`, `integrate.rs`, `impulse.rs`, `contact.rs`, `contact_list.rs`, `window.rs`). The step's
S60/S64/S6C are slots 25/27 (code: `Vf25x`/`Vf27x`, no S-numbers). Flag 0x4 at `+0x60` is the parent-updates-child
bit, not a "contact body" name. Flag 0x8 there is `layout::CONTACT_LIST_FLAG` (named by use: the contact list step,
the slow-velocity clears, the pose step's drive-line sample, the contact sample hook, the placement's hook).

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

Layout, from the asm (`layout/fields.rs`): the three ray blocks (0x70 each, centre `+0xc20`, edges `+0xc90` minus and
`+0xd00` plus) hold the cast's hit words at `+0x10` (point, normal `+0x20`) and the offset across the tyre at `+0x50`;
the damage record at `+0x7f0` (top mount up and side, steering, hub, two sector tables); `CopyCF30`
(`layout::apply_setup`) copies the `CSetup` into the CDef and stores `StrutDimZ + StrutPlatformHeight` at `def+0x3fc`.

## Tyre

`src/tyre/` (stage order and name map in `entries.rs`, record layouts in `layout/`, the typed state in `tyres.rs`
and `layout/typed.rs`, see "Store seam"). 1.02 multiplies the static
friction (`F[0x7A]`, `+0x1e8`) by the temperature grip (`+0x4bc`) twice (NGP the kinetic one the second time); the
sticking load `normal_force*0.1` (`F[0xC5]`; NGP 0.025) rounds before `*sticking_weight` (`F[0xFC]`).

## Spring-damper

`src/spring_damper/` (lifecycle in `suspension.rs`, typed state and its updates in `strut.rs`, kernels in the model
docs, `Mem` edge in `layout.rs`; damper events via `Host::emit_damper`). `CSpringDamper`'s fade pointer `+0x10` is
read without a check; its writer is `CState::SetUnk10` (0x52da40, writes `this+0x10`), which `E51Wheel::UnkA860`
calls on the strut (record: ecx the controller, vtable 0x7401e8), not the state: `layout::set_fade_pointer`. The fade
pair is the damage handler's wheel damper pair (`+0x3ce4 + 8k`, `+0` 0.5 the test multiplier from
`E36DamageOps::Slot4`, `+4` the amount, cleared by `ResetDamage`'s `CU32Value::Clear`, `ntweak::layout`).

## Engine and driveline

`src/engine_driveline/` (model in `engine.rs`/`drive_line.rs`/`sim.rs`, drive-line table in
`layout/drive_line.rs`, offsets in `layout/`). `Set4570` (0x504570): a NaN lock takes the upper clamp and stays
NaN. `SimE010` sets a cut flag when `x <= limit` or unordered. `Sim8E20` (0x518e20) returns the clamped `b` in st0
(callers multiply by `0x73a2e0`). Clutch `M93A0` (0x5093a0): lock and slip with latch `+0x368`, slip `+0x60`.
`CDriveLine::Sample` (0x50b180): the first entry from 1 whose distance is above `d` (`comiss`+`ja`, so NaN finds
none) gives `BlendB` at `(d - d0) / (d1 - d0)`; otherwise the last entry's position (`this - 0x20` when empty).
The `test eax,eax` after a hit is dead (`k >= 1`, direct-call branch cover).
The drive's `+0x170` and the engine's `+0x48` (the option block) point at the damage handler's drive and engine
blocks (`+0x3a18`, `+0x3aac`: `layout::DriveDamage`, `EngineOptions`; rbr-recomp `notes/physics/damage.md` 1a).

## VCU

`src/vcu/` (`CScratchSim::Step` 0x537f50 sequence, tables and locks in the file docs; typed state in "Store seam"). RSF drive2 attempt 5 parted
at its first launch without PC53 rounding of the pedal and speed tables.

## Ground and surfaces

`src/entities/ground.rs` (queries to the host at `g_7c755c`; `VfE8` 0x51c6f0 record; `CollideInit` 0x51bd30 second
pass on bit 3 of `bodyA+0x60`).

## Collision and contacts

`src/collision/` (geometry in physical terms; game memory, calls and sequencing in `layout/`) and
`src/rigid_body/` contact files. The wall table is read eight floats at an 8-byte stride (original; keep).
`Impulse640` (0x529640): 4th argument a time (`dt`), 3rd unused. `E36SphereBox` sums `dz + dy + dx` in x87.
`BoxHit` sets only `AL`.
`E37GridOps::Get00`-`Get2C`, `T04_2`, `Valid5990` (0x4f5950-0x4f5a60) read a `CSigCollisionMesh` descriptor: the
offsets and widths of `load_mesh_desc` (`layout::grid_elems::desc_*`). `CColGrid` names its vertices (+0x40) `edge`,
its edges (+0x484) `pair` and its vertex count (+0x68d) `edge_count` (`layout::ColGrid`, the stores); `MeshGrid` and
`grid_elems` say vertices and edges.

## Entities

`src/entities/` (hooks, `E90SegPool`, walks in the file docs; fence cloth in `layout/fence.rs`). `E90SegPool::Init`
(0x601270) owns a `new[]` array with the count word before it. A group's block is `n` posts of `0x140 + 0x34 * layers *
rows` and `n - 1` spans of `0x78 + layers * (rows * (0x24 + 0x2c * (cols - 2)) + 0x24 * (cols - 4 + 4 * (rows - 1) *
(cols - 1)))` (segment 0x78, particle 0x28, spring 0x20, bend 0xc, pointer 4). Walks are pre-order over children whose
`S80() != 0`.

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
Each functor is named after the part whose handler word it changes: the word's `NRepair::C*Damage` reader (RTTI names,
rbr-recomp `RBR_E39_Damage*.cpp`: radiator `+0x3b00`, oil cooler `+0x3b0c`, oil pressure `+0x3b08`, clutch `+0x3a9c`,
water pump `+0x3af4`, hydraulics `+0x3aa8`, starter `+0x3a94`, brake circuit `+0x3a98`, gearbox factor `+0x3aa0`,
driveshafts `+0x3a30`, differentials `+0x3a1c`, windshield chips `+0x3dc4` and cracks `+0x3dd8`) or the light block's
lamp kind (`notes/renderer.md`). The reconstruction's own functor comments (gearbox, head gasket, suspension arm, ...)
disagree with those readers and are not used.

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
