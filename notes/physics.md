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
- Residue rule (decision, row 15j): a stack word the original reads without anything having written it in the step
  is `rbr_hybrid::RESIDUE` (0) on the pure host, held and per call, both dispatches (`residue(host)`; the twins
  compare every byte, so both sides take it); the emulator and the vectors keep the word their stack holds. 0 because
  a lane of 0.0 makes no NaN, infinity or denormal in lanewise arithmetic. This changes only words that carry
  garbage. `residue-audit` builds take a NaN (`0xffbadbad`) and log each site's first read: a compared word that
  depends on a residue word then differs in the trace. Decided words (drive2 attempt 5, aids off, on and `--damage
  realistic`, traces word by word against the build before and against the audit build):
  - `CPassenger::Step`'s walk point, lane 3 (`physics_core/passenger_step.rs`): `MB560` copies it to car `+0x8ec`
    (`STALE_CAR`), which is 0 in the recording too, so the word now compares equal; no other word changes.
  - `E38Ops::M_C1A0`'s relative velocity, lane 3 (`collision/solver.rs`; `PlaneDiff` writes three lanes; slot-0x94):
    the slip and the impulse carry it to car `+0x8c`, `+0x9c`, `+0x14c`, `+0x1cc`, `+0x22c`, `+0x33c`, `+0x34c`,
    `+0x161c` (`STALE_CAR`) and wheel `+0x6ec` (`HEAP_WHEEL`), nothing else. The pure run's word there was 0 already.
  - `E09Vec4::Sub`/`E21Vec3::Add` out.w, `E33MakeEvent`'s data[15] (it stays in the event pool), `M3DD30`'s frame
    lane `+0x6c`: reached every run, reach no car or wheel word (NaN against 0: traces equal). `Unk94d0`'s remainder
    lane: not reached on drive2.
  - The respawn countdown (`CCar::Vf98x`, `car_body/layout/car_calls.rs`, `run_countdown`): the drive-line sample, the
    probe and the vote's output (`Sampler54A0`'s `c`) are written in three lanes and read in four (the vote, `Normalize`,
    the hit's copy, slot 97 `Vf180`: the pose's w, car `+0x14c`); lane 3 of each is `RESIDUE`. This was the reader of
    CollideWork's 0x499e90 return slot (slot-0x74 of `StepForward`: the direct dispatch left that word, the frame's
    `0xfeed2000`, where the countdown read it, drive2 step 19211): the call is `skip_unwritten` now, both dispatches. The
    same stale words differ from the build before (the call word `0xfeed2000`/`2002`, then float lane arithmetic on it):
    car `+0x8c`, `+0x9c`, `+0x14c`, `+0x33c` and the wheels' `+0x6ec` (`STALE_CAR`, `HEAP_WHEEL`, not compared), from
    step 19210 on; no other word.
  - Payload and block words the original never writes, filled `RESIDUE` before the emit or call on the pure host
    (`fill_residue`): `Assist8DF0`'s seed block `+0`, `+0x1c`, the car step's emit payload word 0 (position, Mrr state),
    `Snap9360`'s first 16 bytes, `SimF950`'s and the body state's words 1..3, the mounted state's words 1..3,
    DampingCollision `+0x08`, `+0x40`, the contact hook's offset lane 3, the mounted pull's gap lane 3
    (`MountedPullHost::gap_lane`). Audit (drive2, aids off, on and `--damage realistic`, trace word by word): the build
    differs from the normal one only at the stale words above; the normal one equals the build before. `Snap9360`, the
    mounted pull and step and `Assist8DF0`'s absent-wheel pairs are not reached on drive2.
  - Still the stack's: the detach locals (`SimFD00`, `StepBig`: the event's unwritten words, the to-world outputs' lane 3;
    no detach on drive2), `CWheel::Vf28`'s payload (does not run in a pure step), `UpdateMrr`'s block word.

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

The standalone gameplay adapter samples a typed `CarState`; backend memory reads, gear lookup, tyre-noise attribution
and control encoding stay in `rbr-standalone/src/physics/layout.rs`. Live and recorded samples use the same conversion.

One physics code over two storages. A subsystem's store trait names its layout's load and store groups; `Mem`
implements each with the layout's bodies (the per-call path), a view over the typed state the held path. Sequence
functions are generic over the trait (`fn step<S: TyreStore>`), never `dyn`.

| Trait | File | Mem impl |
| --- | --- | --- |
| `store::Raw` (supertrait: words, `mem()`) | `store/mod.rs` | same |
| `StrutStore` (handle `Strut`), `DamperBlocks` | `spring_damper/store.rs` | `StrutView` only; `DamperBlocks`: every word store (`spring_damper/layout.rs`) |
| `VcuStore` (one unit, no handle), `DriveBlocks` | `vcu/store.rs` | `VcuView` only; `DriveBlocks`: the drive's typed state (`DriveRef`) |
| `TyreStore` (handle `Tyre`) | `tyre/store.rs` | `TyreView` only; the callers' blocks: `tyre/layout/kernels.rs` |
| `WheelStore` (by `Slot`; the image's words as default methods) | `wheel/layout/store.rs` | `wheel/layout/fields.rs` (every field on the image, per call); `WheelView` (typed) |
| `CarStore` | `car_body/store.rs` | `car_body/layout/car.rs`, over `CarFields`: `Mem` per call, `CarView` (typed) |
| `DriveStore` (one drive, no handle; `word`/`set_word` for the index arithmetic) | `engine_driveline/store.rs` | `DriveView` only (`layout/drive_typed.rs`) |
| `EngineStore: DamperBlocks + EngineTables` (the engine's records, by address), `EngineTables` (tables, drive line, callers' records) | `engine_driveline/store.rs` | `layout/mem_store.rs` (`EngineTables` on every word store); `EngineView` (`layout/engine_typed.rs`) |
| `RigidStore` | `rigid_body/store.rs` | `rigid_body/layout/mem_store.rs`, on every `SolidWords` (the solids' words): `Mem` per call, `SolidView` (typed) |
| `CoreStore` | `physics_core/store.rs` | `physics_core/layout/mem_store.rs`, over `CoreFields` (`layout/typed.rs`): `Mem` per call, `CoreView` (typed) |
| `HingeStore`, `EntityStore: HingeStore` | `entities/store.rs` | `entities/layout/hinge_state.rs`, `layout/store_mem.rs` |
| `StageStore` (the drive line, the material table and its sheets) | `stage/store.rs` | `stage/layout/queries.rs`; `StageView` (typed) |
| `TweakStore` | `ntweak/store.rs` | `ntweak/layout/mod.rs` |
| `CollisionStore` (by address; every method defaults to the image's) | `collision/store.rs` | `collision/layout/mem_store.rs`; `CollisionView` (`layout/typed.rs`, the held words only) |

- A calls trait takes the store as a parameter defaulting to `Mem` (`ForcesCalls<S = Mem>`); clashing names use UFCS;
  what a trait does not name yet goes through `Raw`. Generic sequences sit outside the layouts
  (`rigid_body::{solid_rows, solid_contacts, solid_window}`, `physics_core::{step, contact_loop, passenger_step, pose,
  emit, mounted, accumulate}`, `entities::hinge_step`, `ntweak::plan`, `collision::{solver_pass, entry, events,
  body_contact, grid_edge, mesh, wall, body}`). `object!` (`store/layout.rs`) tables an object once (struct, `Field`
  per row, `lift`/`lower`, `word_of`); `store::car_objects` lists the car's.
- Stay on `Mem`: constructors, setup copies, `load` and the stage's load (image construction); code writing at
  computed addresses in the original's stack frames (detector walks, `notify`, `prof_calls`, `collide_init`, detach
  events, `build_hinge_axes`); rigid_body wake, sleep, `Vf158` (`FilingHost`); NTweak apply and parse; the collision
  cells' link blocks and links, event pool and free list, grid construction, `brake_wall_contact`.
- Mem-direct dispatch (`--dispatch direct`, the live default; `pure` is the fact-check's host): a call within the
  `IN_PLACE` subsystems to a leaf (`pure/leaves.rs`, from `scripts/pure_leaves.py`) or a binding (`Pure::call_plain`)
  runs as a plain call when it passes exactly the words the callee pops (`Pure::stack_words`); watched emits and tyre
  steps stay host calls (`NOT_DIRECT`); `Host::call_direct` skips the words where the callee reads only its arguments.

### Typed storage

- `store::typed::Typed`: `car: CarTyped` (`struts`, `vcu`, `drive`, `engine`, `tyres`, `wheels`, `own`, `collision`),
  `core` (one per world), `stage` (an `Arc`, one per loaded stage), `defs` (read only) and `solids` (with the hinged
  and mounted bodies' own words).
- A step holds it (`Pure::typed`, `Pure::typed_solids`, both on by default); the bindings reach it through
  `Host::typed()`. The stage is lifted once (`OnceLock`), the
  definitions every step (setup, respawn and restart write them between steps); neither is lowered.
- Across steps (`Pure::typed_across_steps`, on by default; off, a step lifts before and lowers after): a step lifts only when
  nothing is held and leaves it held (stale); `Pure::image()` lowers a stale state (it stays held), `image_mut()`
  lowers and drops it (the next step lifts the writer's words), `snapshot` lowers, `restore` drops; the runs between
  steps (`notify_a_vector`, `Playback::emit`, `reset_strut_dampers`) run on the held state and mark it stale. A held
  solid's image copy follows each lower (else a word back at its lifted value stays unlowered: drive2 frame 920).
  Every reader and writer between steps goes through the gate (`Pure`'s image is private); with `poison` every image
  clone panics at a held word while stale. drive2: instructions -0.9 % a step, allocations 26498 -> 9221 bytes a step.
  Per-step and per-frame readers (`layout::{car_state, accumulate_tyre_terms, drive_gear}`, live `poll`, the wheel force,
  the picture's strut pointers) read `Pure::with_held` or `Pure::unheld_image` and do not lower: drive2 lowers in the
  step 2 -> 0, instructions -5.6 %, 9221 -> 2951 bytes; live one lower per answer (picture, dash, sound stay on `image()`).
- Per call (the hybrid check's emulator, the vectors, `typed` off) the views run on the image: the struts, VCU, tyres
  and drive lift and lower around the call (`with_strut`, `with_vcu`, `with_tyre`, `with_drive`), the rest read each
  field at its address. Views compose: `with_defs`, `with_core` (the car's clock, `CarFields: CoreFields`),
  `with_solids` (car, wheel, collision, core); an address a view does not hold (another car, a prop) is the image's.
- Readers outside the bindings during a step (NGP's wrappers, the hinged and mounted bodies, `CCar::ToWorld`) use
  `rbr_hybrid`'s accessors: `vcu::unit_of`, `tyre::read_tyre`, `wheel::read_wheel`, `engine_driveline::on_drive_in`,
  `on_engine_in`, `car_body::{on_car, read_car}`, `physics_core::{on_core, read_core}`, `stage::on_stage`,
  `collision::on_grids`, `rigid_body::{on_solids, with_solids, solid_word, set_solid_word}`.
- Direct calls between held subsystems: the `*Calls` trait stays the seam; with `Host::typed()` the caller runs the
  callee on the held state with values, else the host. Direct: the VCU's calls of the drive, engine and car
  (`vcu::VcuCallees`), its light setters (on the image: the light block is no typed state's) and its slip query (a
  second view of the held unit's `Cell`); the car's `Get4540` (step) and `SetDirty` (assist: its one write, the cell
  header's dirty word, on the image); the wheel's of the tyre's setters, relaxation and tail getters, of the strut
  (NGP's spring step, `GetName`) and of the tyre step (NGP's wrapper, `ngp::tyre_step`; its term through
  `Host::tyre_stepped`). Host: `ResetOutputs` and `UpdateAndReset` (no held caller: `CWheel::Vf28` never runs in a
  pure step, `Pure::reset_strut_dampers` is its held reset; `M3D030` runs only in the boot's first step, before the
  car's lift). Residue audit: the words each skipped host call would write, filled with `0xffbadbad`, change nothing
  on drive2 (aids off and on; the tyre step's 17 also with damage on).
- Trivial callees without a call or its words on the pure host (`rbr_hybrid::trivial`; the emulator keeps the calls):
  the slot-12 getter 0x519c00 (eax `this`; `E43Vf6C` passes `ComposeA` its three words), the empty hook 0x52f480, the
  `ret n` stand-ins whose eax no caller reads, `CE36SubBlock::Init` (its port), the counter (`Host::query_counter`).
  drive2: host calls 1062.8 -> 938.6 a step, instructions -3.2 %. Residue audit (words filled with `0xffbadbad`):
  traces unchanged aids off and on and damage realistic, `--twin mem-direct` equal; CollideWork's 0x499e90 is skipped
  unwritten too (its slot was read by the respawn countdown: "Stack residue is behaviour").
- Held getters and setters direct (drive2 1062.6 -> 911.5 host calls a step): a solid's gate `CSolid::E43Vf80` from the
  walks and old frames (`rigid_body::solid_gate_direct`); the wheel step's `Fwd6DB0`, `Fwd6DD0`, `Fwd43C0` (held drive),
  `GetA48`, `SetA1C`, `AddADC` (`car_body::on_held_car`); `E43Vf70`/`E43Vf74` from the wheel, the car's drag and the
  gravity pass while the flush gate is clear (`rigid_body::accumulate_direct`); `Vf81x` from the force pass and the
  hinge solve at the call's esp (`contact_vote_direct`); `M_D760X`'s material samples on the held stage (NGP; no slots:
  host). Host: `CWheel::E43Vf20`/`E43Vf6C`'s jumps, `WorldCompose`, `ToWorld` (untyped walks and ops below, whose calls
  in place would land elsewhere on the stack). Audit as above, also `--damage realistic` and `--twin mem-direct`
  (`--dispatch direct` is live only).
- The wheel's own calls in place (drive2 787.3 -> 670.7 host calls a step, instructions -0.85 %): a call of a wheel
  binding on a held wheel from the wheel's code or the entity walks (`M3B5B0`, `M3D460`, `M3D520`, `M3B900`, `M3AFB0`,
  `M3AC50`, `Blend4`, `Unk97a0`, `Unk96e0`, `Unk9290`, `Unk9900`, `E50BlendOps::Blend`, Hook's `M3DD30`, `CWheel::E43Vf*`)
  runs in place (`wheel::call_wheel`, `Host::call_in_place`: its own frame at the call's esp, so the calls it leaves the
  placement of land as through the host, but none of the call's words); the frame blocks they take stay in the callers'
  frames on the image. Leaf math direct with values on the pure host (`collision::{copy_vec4, length}`): `Length` from
  `E36Collect`, `CopyFrom` from `SyncGrid`, `CBody::Vf11C`, the body state, the L2 detector, `SimCFF0`. Audit (the
  skipped words `0xffbadbad`): drive2 PASS, traces equal aids off, on, damage realistic; twin typed, mem-direct equal.
- `SetDirty` (0x52f450: `m_dirty = 1`, nothing else; its readers `IsDirty`/`ClrDirty` are the tweak UI's, not run)
  direct from `M3DD30`, `M3B900` (the wheel's four reals' dirty words held: `CarWheels::set_dirty`) and the hinge's
  `SimF950` (its headers on the image, `set_solid_word`): drive2 787.5 -> 761.3 host calls a step. Host: `CopyCF30`,
  `M3D030`, `E45_M1980` and the boot step's first four (no held state there). Audit as above; poison with one
  site back on the host stops at the held `strut_length_dirty`.
- The entities' matrix and pair-walk ops (map in "Entities"; drive2 644.6 -> 514.9 host calls, 760.1 -> 559.5 skipped
  calls a step, instructions -3.2 %): callers on held state run `MB560`, `ComposeA`, `ComposeB`, `FetchX`, `FetchY`,
  `WB410`, `Combine`, `M51B460`, `M51B500` in place (`entities::call_op`); `MFc10` is trivial (`this + 0x1c`); the
  walks' frame, solid and CEntity echo hooks, identity walks and `FetchX`'s direct run skip without their words
  (`skip_unwritten`; feature `residue-audit` fills them with `0xffba0000 | line`). The child walks' words (~260 a
  step) fed only the passenger walk point's lane 3, now `RESIDUE` ("Stack residue is behaviour"), and are skipped the
  same way. Audit: traces equal aids off, on, damage realistic; twin typed (the three) and mem-direct (aids off, on)
  equal.

- The frame carries as values (row 15d part 2; drive2 508.9 -> 494.1 host calls a step, in-place runs 711.7 -> 673.7,
  instructions -1.20 %): on held state `MB560`, `ComposeA`, `ComposeB`, `FetchX`, `M51B460` and `M51B500` read their
  rows once (a held solid's through its view), walk a local copy (`entities::carry` over `walk::run`; each frame's
  owner map a `rigid_body::rows::RowMap` with the pose read once, `entities::layout::passage` from the frame's and
  owner's vtables) and store the result where the original wrote it; no hook, gate or owner slot is called (the old
  frames' through the host before). Back out to the walk in place: a hook, gate or owner slot not in `passage`
  (`CCarBody::Vf9Cx`, 2.0 a step from `ComposeA`: its gated name getter is the car body's; `CSolid::E43VfBC`), an
  output on the owner's pose words, overlapping rows, more than 16 rows. Still the bindings' entry (`call_op` runs
  them in place: its 60 callers pass no memory, 20 leave the placement to the host); the child walks' words are
  written as in place (`HeldFrames::descend`, to go with `PairWalk::walk`'s). A carry counts as a run only when it
  touched held words (else the call memo of the E36 collect's transforms stops keeping them: +0.5 % instead).
  `FetchY`/`WB410` (grids' box tables, not rows) and `Combine` (a pair store) stay in place. The notify vector's
  `MB560` between steps (`Playback::notify_a_vector`) in place, 1.02 host calls a step. Audit (`residue-audit`, the
  hooks', gates' and owner slots' words filled): reports equal to the base aids off, on, damage realistic; twin typed
  and mem-direct equal aids off and on.
- The hinged body's calls of its own bindings run in place on a held body (`entities::hinge::call_hinge`): the limit
  update, `SimD160`, `SimC960`, `SimE1D0`, `SimCFF0`, `SimEA80`, `SimD860` from `SimF950`, `SimEAD0`, `SimD160` and
  `SimEFE0`; the limit update's `SetDirty` direct (`hinge::set_dirty`, the rate tweak's dirty word on the image).
  Host: `VfE070` (it writes the definition), the walks and ops (`MB560`, `Xform8130`, `BodyCompose`, `ToQuat`,
  `Scale`), `E43Vf6C`, the emits, the detaches' callees. Audit (the skipped words `0xffbadbad`): drive2 PASS, twin
  typed aids off, on and damage realistic, mem-direct equal.

- The cell grid (drive2 508.9 -> 389.8 host calls a step, instructions -1.7 %): `E45_M40C0` reads `LookupTwin`'s
  link from the held `CellTable` without the call or its words (`collision::held_cell_link`; a cell with no link
  block yet goes through the binding, which allocates through the stand-in and fills the held slot: 2 in drive2);
  callers on held state run `E45_M40C0`, `E45_M46A0`, `M_D760X`, `CColGrid::SyncGrid`, `E37GridOps::M6610` and the
  sample's `CLink::Unlink` (0x47d160) in place (`collision::call_grid`). `SyncGrid` declines only over 127 edges and
  `M6610` only a one-point source, in place as through the host. Host: `LookupTwin` from the pose step (`E43Vf6C`,
  1.0 a step: its third word feeds `CCar::AppendA980`). Audit (`residue-audit` fills an in-place call's words with
  `0xffbd0000 | callee`): drive2 PASS, twin typed aids off, on and damage realistic, mem-direct equal. Poison with
  `LookupTwin` back on the image stops at step 0 (`cell grid +0x0`). RSF 122 and 104 (autopilot, 30000 steps): poison
  runs through, output equal to the base's.

### Per subsystem

Each passes drive2 attempt 5 held (84084/84084), `--twin typed` (aids off and on), poison, `round_trip` on the 40-43
hybrid snapshots and the hybrid gate 13/13 (which runs them per call); "Evidence" adds the rest.

| Typed state (storage) | What is typed | On the image, why | Evidence |
| --- | --- | --- | --- |
| Struts, `spring_damper::strut::Struts` (`RefCell`, per run) | controller mode, latch, signal; effective dampings; fade pair | definition, headers, pointers; NGP's strut words `strut+0x30..0x38`, wheel `+0xe80..+0xe8c` and the out pointers (the binding writes them) | `tests/pure_struts.rs` |
| VCU, `vcu::layout::Unit` (`Cell`) | pedals, clutch, launch, slip, lockout, gear timer, cooldown; the drive's gear words through `DriveBlocks` | definition, pointers, owner car; CopyDef's maps, ReadPacket's packet | `vcu_vectors` per call, chain test |
| Tyres, `tyre::tyres::Tyres` (`RefCell`, per run) | contact, patch, forces, slips, outputs, heat, soil, wear; relaxation, deflections, temperatures | setup, pointers, the wheel's contact angle; `Locate`, `TyreClamp`, the friction curve, getters (callers' words) | `tyre_vectors` per call, chain test |
| Stage, `stage::typed::Stage` (read only, `Arc`) | drive line, material table, sheets | collision system and copies, `.col` tree, mesh grids, water meshes (heap graphs, per-query scratch) | `differential` (every entry, slot, texel); poison on RSF 122, 117 |
| Wheels, `wheel::layout::Wheel` (field at a time) | refresh and axes pose, walk, probe, sectors, ground copies, spin, forces, flags, four reals' dirty words | pointers, damage record, reals' other header words, every word another subsystem reads at its address (angle, axle, hub, car force, ray blocks, NGP's strut words) | `differential`; `wheel_vectors` on `Mem` |
| Drive, `engine_driveline::layout::DriveTyped` (`RefCell`, per access) | lanes, pair, pack, bias, gear steps; locks, gains, ring, clutch and pair matrices, gear fields, wheel blocks | definition, damage block (its functors), pointers, state's car `+0x580` (NGP reads it raw), worker matrices (`E39_MatVec`) | `engine_driveline_vectors`, chain test |
| Engine, `engine_driveline::layout::EngineTyped` (`RefCell`, per word; a word store via `word_of`) | crank, pedal, heat, stall, misfire; radiator, coolant, turbo, boost, dampers, intake, cut flags, torque terms | option block (damage handler's, NGP's wrapper writes it), roll table and seed, `EngineNoise` payload (listeners read it at its address) | `engine_differential`, chain test |
| Car's own, `car_body::layout::CarState` (field at a time) | control update and kept controls, contact times, value cells (`CarStore::cell`), speeds, respawn, overload, assistance | pointers, parts, damage handler, cell headers (`SetDirty`), `+0xaa0`/`+0xac0`, `+0x1600`/`+0x1610` (hinged bodies read them at their address) | `differential`; `car_body` vectors on `Mem` |
| Core, `physics_core::layout::Core` (`Cell`) | hit and pass counters, single stepper, substep and step counters, clock, step globals | pointers, the counter's NTweak header, the solver and its event list, entity lists | `differential` |
| Collision, `collision::layout::CarCollision` | solver counts, contact and offset rows (per block on first touch, `Touched`), car grids' (ids 1-9) hit bytes, the cell grid's header and slots (`CellTable`: a slot on its first lookup, only filled slots lowered) | rows' composed point `+0x40` (read at its address), body link, grids' other words, props' grids, events, pool, free list, the cells' link blocks and the bodies' cell links | `differential`; `collision_vectors` per call |
| Definitions, `store::defs::Defs` (read only, watched) | `WheelDef`, strut `Definition`, `TyreDef`, `CarDef`, `vcu::layout::Def`, `DriveDefinition`, `EngineDefinition`, flags word | step-written words (wheel defs `+0x880..+0x900`, tyre setups `+0x204..+0x284`), exhaust and turbo tables (lift cost), hinged and mounted defs (mount point `+0x220`, block `+0x200`), physics def but flags | `store::defs::differential`; a bit flipped in the held engine def fails drive2 |
| Hinged and mounted bodies' own words, `entities::layout::Hinged`, `physics_core::layout::mounted_body::Mounted` (held with their solid, `CarSolids`: one lookup, `OwnTable`) | limit springs' positions, angle, rate, pose matrices, hinge rotation, refresh copies, pivot outputs, solve value; the mounted body's event-sent flag | owner, link, definition and enable pointers; NTweak headers and dirty words (`SetDirty`); the walks' outputs at their addresses (`Xform8130`'s `+0x5c0`, `MB560`'s `+0x5f0`, `+0x610`: row 15d/e); exclusion sets; the mounted state and scratch objects | `rigid_body::layout::differential` (the hinge's host-free entries); twin over the own words; aids on reaches `StepBig` held once on drive2 |
| Solids, `rigid_body::layout::Solid` (per solid, held by address) | pose, velocities, acceleration estimates, sleep and window state, scratch, accumulators: car, wheels, hinged and mounted bodies | pointers, flags byte, NTweak headers, scale, state's diagonal and gate, other scratch words | `differential`; twin over every solid held, detached too; shadow survey |

### Rules

- Borrow per access, never over a host call, where runs re-enter through the host (drive, engine, wheel, car, core);
  struts and tyres borrow per run. No lift or lower inside a step: image round trips around nested runs broke
  orderings (the force pass after `MB560`'s copy, `E43Vf70` after `E43Vf60`).
- Lazy for large state: lift on touch (collision rows) or leave on the image (the exhaust table's lift cost +0.76 % a
  step, an eager event pool +23.9 %).
- Values over pointers: entries take the callers' blocks as values and return outputs the binding writes after
  (`RigidStore::contact_input` takes a `Contact`); a pointer into the subsystem's own words keeps its write.
- Readers at an address (NGP's wrappers, hinged bodies, `CopyFrom`) show only in the twin or poison, rarely in the
  drive: each needs an accessor. A run in place on held words calls `ran_direct` (the call memo sees only the image).
- Per-word cost: a held word read was a span search, a borrow and a field lookup where the image read was one load
  (+12 % a step for the solids). Held solids keep their span as one cell array and a copy as lifted; lookups try the
  bounds, the last solid, the last gap, then a binary search; lower writes only the changed runs (`Mem::set_u32s`),
  which is sound because poison forbids image writes to held words in a step (+3.4 % against holding off).
- Definitions are watched, not poisoned: reads go through and are checked against the image (`defs::checked`), a
  write panics (`Mem::watch_writes`).

### Checks

- `scripts/gate.sh --typed`: twin-typed and twin-aids (drive2 with `--twin typed`, aids off and on: held against per
  call after each of 84084 steps, round trips and stage queries equal) and poison (`rbr-hybrid/poison`: held words
  poisoned, definitions watched, held across steps); twin-across (`--twin across`: held over each step against held
  across steps); twin-direct (`--twin mem-direct`: the direct dispatch, where calls skip their stack words, against
  the pure dispatch; residue readers show there first). `--twin mem` checks the `Mem` path the same way. The twin
  and the recording compare read the images through `Pure::image`.
- `round_trip` (`rbr-hybrid` binary, snapshots): lower∘lift of each object, stage and definition answers against the
  image, the differentials (host-free entries in a row per call against held, every byte); the chain tests
  (`the_typed_*_over_a_chain_keeps_the_per_call_bytes`) over vectors.
- Shadow survey (instrumented build, not in the tree): held words written through and read from the image, every
  access to one on the poisoned image logged with its bindings, so all unrouted readers at once.

### Left

- The entities' per-entity state: the exclusion sets (`[count, members...]` arrays from `E43_ExclA000`, a member
  removed by `exclusion::remove_exclusion_at` when a hinge breaks; the test reads both sides', props' too) need an
  arena keyed by entity, lifted on first touch with image fallback; the walker marks (`+0x4`/`+0x20` and serials) and
  the tree links (`+0x48` first child, `+0x10` sibling) need link maps before the entity walks (row 15e) run on typed
  nodes.
- The event pool as a lazy arena (its readers at an address, `MB560`, `collide_init`, detach and the contact loop,
  need stores first); collision arenas: the cells' link blocks (head, tail, count) and the bodies' cell links as a
  link map (writers `unlink`, `move_to_cell`, `CCar::AppendA980`; readers `NotifyFiltered`, the detectors' cell
  walks, `E36Collect`, detach, the sample lists of `prof_calls`: each needs a store before the map can hold them),
  the registration zones' lists, the entity lists as link maps (the sleeping list holds 3172-8217 nodes: lazy,
  image fallback); `E45_M40C0`/`E45_M46A0` with their outs as values (they run in place on out pointers now).
- The `.col` tree, mesh grids and water meshes; props (lift on wake, lower on sleep).
- State typed across steps on by default (after the standalone and twin read through the gate); `--twin` in the live
  run; more direct calls (the host calls above).
- The pure step without its stack-frame image; definitions built from parsed files; the image path removed from the
  game (per call stays for the hybrid check and the vectors).

Open decisions, with the defaults taken until decided:

- Typed allocations go through the stand-in allocator (`pure/heap.rs`: placement and log) for now.
- State becomes typed across steps (every presentation reader then reads through a lower).
- The pure run drops its stack-frame image once direct edges pass the residue audit.
- Definitions: A now (lifted read only, step-written words moved into state, writes watched), B later (built from
  parsed files, lowered by a placement plan); whether B changes what milestone L is proved by is open.

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
`src/physics_core/`. All sizes, offsets and rules in the file docs. The bodies' own words a step writes are typed with
their solids ("Store seam"); the angle's and the rate's NTweak headers sit inside the end stops' window (`+0x3f4..`)
and the refresh's angle block, so a store of either writes two header words on the image and the value held.

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

Matrix and pair-walk ops (`walk.rs`, `transform.rs`; bindings `rbr-hybrid/src/entities/{walks,matrices,frame}.rs`):
- A node is a frame: the entity itself (`CEntityFrame` 0x73cb64: `+4` parent, `+8` depth, `+0xc` owner) or its old
  frame (`+0x1c`, `CEntityOldFrame` 0x73cb84, slot 13 `MFc10` 0x519c10). The root is the world frame (`*0x7cbb00`,
  `WALKER`). A walker climbs the deeper node's parents to equal depth, then both until they meet, calling the frame
  hooks (slots 1-6) on the way up (One, Three, Five) and down (Two, Four, Six); a frame forwards to its owner's slot
  0x9c..0xc0 (an old frame picks 0xa4..0xc8 by the owner's gate, slot 0x80). CSolid's: 0x9c `scale_add_rows`, 0xac
  `transform_body_rows`, 0xb0 `transform_body_rows3` on the rows in place; CEntity's 0x9c/0xa0/0xb0 return `a`.
- `MB560` (0x51b560, `(a, b, c)`): `a` copied to `c`, carried from `this`'s frame to `b`'s (Three-Four). `ComposeA`
  (0x51b4c0, `(src, o2, out)`) and `FetchX` (0x51b6c0, on a `{matrix, node}`): One-Two; `ComposeB` (0x51b600):
  Five-Six; `M51B460`/`M51B500`: `n` blocks, One-Two/Three-Four; `FetchY` (0x51b780): a grid copy and `WB410`'s grid
  hooks over its box tables; `Combine` (0x51ab90, twin 0x51ab70): the pair `(a, b)` at `this`. eax the output.
- Inputs: the frames' words and the owners' vtables (image, set at construction), the owners' poses (held solids'
  through `SolidView`), the rows (`a`, `out`: mostly the caller's frame). No typed state of their own.
- As values on held state (`entities::carry`): the rows a local copy, each frame a `Passage` (unchanged, or the owner
  body's `RowMap`: 0x9c points out, 0xa0 points in, 0xac rows, 0xb0 three-lane rows, 0xa4..0xb8 the same with the pose
  at the step's start); a walk's `b` is its row count (the shallower walkers pass 1).
- Residue (audit, the skipped words filled per site): the child walks' words reached `CPassenger::Step`'s `MB560`
  source, lane 3 unwritten, copied into car `+0x8ec`; that lane is `RESIDUE` on the pure host and the words are not
  written. No reader of the hooks', the identity walks' or the ops' own call words.

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
