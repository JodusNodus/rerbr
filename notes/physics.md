# Physics: the step and exact ports

What a porter needs to keep `crates/rbr-physics` bit-exact against the original under NGP 7.4.778 (ports are of the
1.02 exe's functions with NGP's behaviour; no 1.02-only arm remains). Modules and `ports.csv`:
[crates/rbr-physics/README.md](../crates/rbr-physics/README.md); checks: [verification.md](verification.md); NGP:
[ngp.md](ngp.md). Per-subsystem field tables and asm notes: rbr-recomp `notes/physics/*.md`.

The original's physics is `NPhysics::` in the exe: each component (`CCar`, `CWheel`, `CTyre`, `CEngine`, `CDrive`,
`CSpringDamper`, ...) splits into `CDef`, `CSetup`/`CSetupOptions`, `CState` and `CScratch`, with values set through
the `NTweak` key tables from the per-car files in `physics.rbz`. `rbr-step` binds the original's functions by address
on the ports; a binding gets the call's registers and stack words, runs the port, calls the callees its port leaves
to its caller through the `Host` (the `Runner`) and says what `ret` leaves (eax, x87 result, popped bytes). A call to
an address with no binding stops the run (`Stop`).

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
- Float to int: never `as`. `fpu::trunc_i32` (`cvttss2si`) gives `INT_INDEFINITE` (`i32::MIN`) for NaN and out of
  range; `math::ftol::trunc_i64` is `_ftol2` (0x70c2c4, low word of the int64).
- Constants keep the exe's bit patterns (some pools differ from the literal, e.g. 0x3d4cccd0 for 0.05). A punned
  word (float bits in an int) stays `u32`; build floats with `f32::from_bits`.
- Default NaN sign: an arm64 host makes `0x7fc00000` where the original wrote `0xffc00000`; a comparison accepts
  "NaN for NaN" only at a named site, never as a blanket tolerance.
- Stack residue is behaviour. Callees that leave arguments on the stack feed the next call: the slot-12 identity
  getter (0x519c00, `mov eax, ecx; ret`) keeps its pushes for the following `ComposeA`; `MB560` (0x51b560) and
  `ComposeA` (0x51b4c0) are thiscall `ret 0xc` with three arguments, some callers reuse earlier pushes, and
  `CSolid::E43Vf6C` leaves three words for 0x51b4c0 (the only argument-word reuse on drive2 attempt 5: 4 a step).
  Unwritten lanes (a w lane after a three-float write, then a four-lane `CopyFrom`) hold stack garbage; the
  recording compare excludes those words by name ([verification.md](verification.md#masks)).
- Residue rule: a stack word the original reads without anything having written it in the step is
  `rbr_step::RESIDUE` (0), held and per call. 0 because a lane of 0.0 makes no NaN, infinity or denormal in lanewise
  arithmetic; it changes only words that carry garbage. Evidence for each site: a build that filled the word with a
  NaN changed nothing but the words below on drive2 attempt 5 (aids off, on, `--damage realistic`). Sites:
  - `CPassenger::Step`'s walk point, lane 3 (`physics_core/passenger_step.rs`): `MB560` copies it to car `+0x8ec`
    (`STALE_CAR`), which is 0 in the recording too.
  - `E38Ops::M_C1A0`'s relative velocity, lane 3 (`collision/solver.rs`; `PlaneDiff` writes three lanes): the slip and
    the impulse carry it to car `+0x8c`, `+0x9c`, `+0x14c`, `+0x1cc`, `+0x22c`, `+0x33c`, `+0x34c`, `+0x161c`
    (`STALE_CAR`) and wheel `+0x6ec` (`HEAP_WHEEL`), nothing else.
  - `E09Vec4::Sub`/`E21Vec3::Add` out.w, `E33MakeEvent`'s data[15] (stays in the event pool), `M3DD30`'s frame lane
    `+0x6c`: reached every run, reach no car or wheel word.
  - The respawn countdown (`CCar::Vf98x`, `car_body/layout/car_calls.rs`, `run_countdown`): the drive-line sample, the
    probe and the vote's output are written in three lanes and read in four (the vote, `Normalize`, the hit's copy,
    slot 97 `Vf180`: car `+0x14c`); lane 3 of each is `RESIDUE`. The countdown also read `CollideWork`'s 0x499e90
    return slot (the word at `StepForward`'s slot-0x74): that call leaves it unwritten (drive2 step 19211 differed
    when the frame's `0xfeed2000` stood there).
  - Payload and block words the original never writes are filled `RESIDUE` before the emit or call on the runner
    (`fill_residue`): `Assist8DF0`'s seed block `+0`, `+0x1c`, the car step's emit payload word 0, `Snap9360`'s first
    16 bytes, `SimF950`'s and the body state's words 1..3, the mounted state's words 1..3, DampingCollision `+0x08`,
    `+0x40`, the contact hook's offset lane 3, the mounted pull's gap lane 3 (`MountedPullHost::gap_lane`).
  - Still the stack's: the detach locals (`SimFD00`, `StepBig`: the event's unwritten words, the to-world outputs'
    lane 3; no detach on drive2), `CWheel::Vf28`'s payload (does not run in a step), `UpdateMrr`'s block word,
    `Unk94d0`'s remainder lane (not reached on drive2).

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
- The asm decides, not the C++: where the reconstruction has a bug (uninitialised arrays in the E38 ray worklist,
  indeterminate scales in `E36Collect`), port what the original does.
- Dispatch slots and signal emits go through a host trait in the original's call order; tests check that order.

## Exe data

The boot maps the 1.02 exe's `.rdata` and `.data` as zeros and over them the words a run reads before writing them
(`rbr_formats::exe_data` with the season table, the renderer's and the tyre sound's tables), no `.text` or patch
array; NGP callees and boot settings are in [ngp.md](ngp.md) (no exe code mapped; the load budget is NGP's 1500).
The words come from `exe-reads` builds (`cargo build --release -p rbr-standalone --features rbr-physics/exe-reads`, a
run with `RBR_EXE_READS=FILE` writes them; `scripts/gen-exe-data.py --reads FILE --write` merges them into
`data/exe-words.txt`): drive2 attempt 5 and 140 autopilot boots (8 retail cars, 16 stages, weathers, damage, a
championship stage). A port that reads a word not listed reads 0: rerun the sweep after porting new exe reads.
- A run reads only the virtual-table slots it calls, so the generator takes every virtual table whole
  (`whole_vtables`): the 937 the code installs (their addresses are `.text` operands) and each one a listed word lies
  in. A part coming off (`StepBig`, 0x522100) stopped the physics with `unbound 0x0` while `CMountedRigidBody`'s table
  (0x73e62c) had 9 of 95 slots and its scratch's (0x73fb6c) none (El Cubilete).
- A replay is a sweep over its drive: an El Condor drive (RSF Audi quattro A2, `--tyre 3`) differed at frame 0 on the
  tables alone until merged with `--reads` (`viewer --replay` on an `exe-reads` build): the tyre class of tyre 3
  (`g_e09CloudTable` 0x7c12f0) and the fence types 2 and 5 (`g_fenceTypes` rows, 0x7d81fc, 0x7d8288) had not been read.
- Instruction operands are decoded only for captured-memory validation.

## Store seam

The standalone gameplay adapter samples a typed `CarState`; backend memory reads, gear lookup, tyre-noise attribution
and control encoding stay in `rbr-standalone/src/physics/layout.rs`.

One physics code over two storages. A subsystem's store trait names its layout's load and store groups; `Mem`
implements the ones that need it with the layout's bodies, a view over the typed state is the path a step runs.
Sequence functions are generic over the trait (`fn step<S: TyreStore>`), never `dyn`.

| Trait | File | Implementors |
| --- | --- | --- |
| `store::Raw` (supertrait: words, `mem()`) | `store/mod.rs` | `Mem`, the views |
| `StrutStore` (handle `Strut`), `DamperBlocks` | `spring_damper/store.rs` | `StrutView`; `DamperBlocks` on every `Raw` (`spring_damper/layout.rs`) |
| `VcuStore` (one unit, no handle), `DriveBlocks` | `vcu/store.rs` | `VcuView`; `DriveBlocks`: the drive's typed state (`DriveRef`) |
| `TyreStore` (handle `Tyre`) | `tyre/store.rs` | `TyreView`; the callers' blocks: `tyre/layout/kernels.rs` |
| `WheelStore` (by `Slot`; the image's words as default methods) | `wheel/layout/store.rs` | `Mem` (`wheel/layout/fields.rs`), `WheelView` |
| `CarStore` | `car_body/store.rs` | every `CarFields` (`car_body/layout/car.rs`): `Mem`, `CarView` |
| `DriveStore` (one drive, no handle; `word`/`set_word` for the index arithmetic) | `engine_driveline/store.rs` | `DriveView` (`layout/drive_typed.rs`) |
| `EngineStore: DamperBlocks + EngineTables` (the engine's records, by address), `EngineTables` | `engine_driveline/store.rs` | `Mem` (`layout/mem_store.rs`; `EngineTables` on every `Raw`), `EngineView` (`layout/engine_typed.rs`) |
| `RigidStore` | `rigid_body/store.rs` | every `SolidWords` (`rigid_body/layout/mem_store.rs`): `Mem`, `SolidView` |
| `CoreStore` | `physics_core/store.rs` | every `CoreFields` (`physics_core/layout/mem_store.rs`, `layout/typed.rs`): `Mem`, `CoreView` |
| `HingeStore`, `EntityStore: HingeStore` | `entities/store.rs` | `entities/layout/hinge_state.rs` (every `SolidWords`), `layout/store_mem.rs` (`Mem`) |
| `StageStore` (the drive line, the material table and its sheets) | `stage/store.rs` | `Mem` (`stage/layout/queries.rs`), `StageView` |
| `TweakStore` | `ntweak/store.rs` | `Mem` (`ntweak/layout/mod.rs`) |
| `CollisionStore` (by address; every method defaults to the image's) | `collision/store.rs` | `Mem` (`collision/layout/mem_store.rs`), `CollisionView` (`layout/typed.rs`, the held words only) |

- A calls trait takes the store as a parameter defaulting to `Mem` (`ForcesCalls<S = Mem>`); clashing names use UFCS;
  what a trait does not name yet goes through `Raw`. Generic sequences sit outside the layouts
  (`rigid_body::{solid_rows, solid_contacts, solid_window}`, `physics_core::{step, contact_loop, passenger_step, pose,
  emit, mounted, accumulate}`, `entities::hinge_step`, `ntweak::plan`, `collision::{solver_pass, entry, events,
  body_contact, grid_edge, mesh, wall, body}`). `object!` (`store/layout.rs`) tables an object once (struct, `Field`
  per row, `lift`/`lower`, `word_of`); `store::car_objects` lists the car's.
- Stay on `Mem` (the image): constructors, setup copies, `load` and the stage's load (image construction); code
  writing at computed addresses in the original's stack frames (detector walks, `notify`, `prof_calls`,
  `collide_init`, detach events, `build_hinge_axes`); rigid_body wake, sleep, `Vf158` (`FilingHost`); NTweak apply and
  parse; the collision event pool and free list, grid construction, `brake_wall_contact`; the entity walks' marks
  (`+0x4`/`+0x20`, serials), tree links (`+0x48` first child, `+0x10` sibling) and exclusion sets (`[count,
  members...]` arrays from `E43_ExclA000`; a member is removed by `exclusion::remove_exclusion_at` when a hinge
  breaks); the entity lists (the sleeping list holds 3172-8217 nodes); the `.col` tree, mesh grids and water meshes;
  props.
- Typed allocations go through the stand-in allocator (`runner/heap.rs`: placement and log).

### Typed storage

- `store::typed::Typed`: `car: CarTyped` (`struts`, `vcu`, `drive`, `engine`, `tyres`, `wheels`, `own`, `collision`),
  `core` (one per world), `stage` (an `Arc`, one per loaded stage), `defs` (read only) and `solids` (with the hinged
  and mounted bodies' own words).
- The bindings always run on it: a step lifts it when nothing is held, runs on it and leaves it held (stale); they
  reach it through `Host::typed()`. The stage is lifted once (`OnceLock`), the definitions every step (setup, respawn
  and restart write them between steps); neither is lowered.
- Between steps: `Runner::image()` lowers a stale state (it stays held), `image_mut()` lowers and drops it (the next
  step lifts the writer's words), `snapshot` lowers, `restore` drops; the runs between steps (`notify_a_vector`,
  `Playback::emit`, `reset_strut_dampers`) run on the held state and mark it stale. A held solid's image copy follows
  each lower (else a word back at its lifted value stays unlowered: drive2 frame 920). Every reader and writer
  between steps goes through the gate (`Runner`'s image is private); with `poison` every image clone panics at a held
  word while stale. Per-step and per-frame readers (`layout::{car_state, accumulate_tyre_terms, drive_gear}`, live
  `poll`, the wheel force, the picture's strut pointers) read `Runner::with_held` or `Runner::unheld_image` and do not
  lower; live lowers once per answer (picture, dash, sound stay on `image()`).
- The boot's `CCar::UpdateA720` builds what the lift reads (fade pairs, setups, frames), so it runs under
  `Runner::construct`: each binding frame that asks lifts the typed state and lowers it on the frame's exit
  (`lift_on_demand`, `release_scoped`).
- Views compose: `with_defs`, `with_core` (the car's clock, `CarFields: CoreFields`), `with_solids` (car, wheel,
  collision, core); an address a view does not hold (another car, a prop) is the image's.
- Readers outside the bindings during a step (NGP's wrappers, the hinged and mounted bodies, `CCar::ToWorld`) use
  `rbr_step`'s accessors: `vcu::unit_of`, `tyre::read_tyre`, `wheel::read_wheel`, `engine_driveline::on_drive_in`,
  `on_engine_in`, `car_body::{on_car, read_car}`, `physics_core::{on_core, read_core}`, `stage::on_stage`,
  `collision::on_grids`, `rigid_body::{on_solids, with_solids, solid_word, set_solid_word}`.

### Calls between bindings

- `Runner::call_at` runs a callee as a plain call (`Runner::call_plain`) when its binding is a leaf (`runner/leaves.rs`,
  from `scripts/leaves.py`: no host call, nothing of the stack but its arguments) or passes exactly the words the
  callee pops (`Runner::stack_words`, learned per binding); watched emits and tyre steps always take the host path
  (`NOT_DIRECT`); `Host::call_direct` skips the call's words where the callee reads only its arguments.
- Callers on held state skip the call's words and sometimes the call itself where no later reader reads them (the
  residue rule above; evidence: the recordings stay equal with the words filled with a NaN, and the gate): `crate::enter`
  (call's words written, run counted), `crate::enter_leaf` (not written), `Host::call_in_place` (the binding in a frame of
  its own, none of the call's words), `Host::with_image` (a Rust port with values on the image), `crate::trivial` (the
  slot-12 getter 0x519c00 (eax `this`), the slot-13 getter 0x519c10 (`this + 0x1c`), the empty hook 0x52f480, the `ret n`
  stand-ins whose eax no caller reads).
- Where the callees are direct: the VCU's calls of the drive, engine and car (`vcu::VcuCallees`), its light setters and
  slip query; the car's `Get4540` and `SetDirty`; the wheel's calls of the tyre's setters, relaxation and tail getters,
  the strut (NGP's spring step, `GetName`) and the tyre step (NGP's wrapper, `ngp::tyre_step`; its term through
  `Host::tyre_stepped`); the wheel binding's own calls (`wheel::call_wheel`); the wheel step's `Fwd6DB0`, `Fwd6DD0`,
  `Fwd43C0`, `GetA48`, `SetA1C`, `AddADC` (`car_body::on_held_car`); a solid's gate `CSolid::E43Vf80`
  (`rigid_body::solid_gate_direct`); `E43Vf70`/`E43Vf74` from the wheel, the car's drag and the gravity pass while the
  flush gate is clear (`rigid_body::accumulate_direct`); `Vf81x` from the force pass and the hinge solve at the call's
  esp (`contact_vote_direct`); `M_D760X`'s material samples on the held stage; the hinged body's own bindings from
  `SimF950` etc. (`entities/hinge.rs`, `enter_hinge`); `SetDirty` (0x52f450: `m_dirty = 1`) from `M3DD30`, `M3B900` (the
  wheel's four reals' dirty words held: `CarWheels::set_dirty`) and `SimF950`; leaf math (`collision::{copy_vec4,
  length}`). Host calls remain for `CWheel::E43Vf20`/`E43Vf6C`'s jumps, `WorldCompose`, `ToWorld` (untyped walks whose
  calls in place would land elsewhere on the stack), `CopyCF30`, `M3D030` (boot's first step only, before the car's
  lift), `E45_M1980`, `ResetOutputs`/`UpdateAndReset` (`CWheel::Vf28` never runs in a step; `Runner::reset_strut_dampers`
  is the held reset), `VfE070` (writes the definition) and the hinge's walks and detaches.
- Entities' matrix and pair-walk ops (map in "Entities"): `entities::{op, Op}` (`Op::{Mb560, ComposeA, ComposeB, FetchX,
  FetchY, Combine, CopyBlocks}`, `rbr-step/src/entities/matrices.rs`) carry the stack words as values with the receiver;
  on held state `op` runs them on the image `Host::with_image` lends, no call, no frame. The bindings (`mb560_run`, ...)
  are the per-call adapter over the same `run`. The tree walks run as Rust calls at the call's entry esp
  (`entities::walk_at`, `EntityWalk::{Gated, Integrate, PreOrder, Name}`: `CEntity::Vf40`, `E43_Walk1F0`, `E43Vf20`,
  `Vf48`, `Vf58`, `Vf5C`, `Vf64`, `Vf6C`), `CBody::CState::Unk44` (`collision::reset_state_at`),
  `E38RaySeeds`/`E38RayClear` (`collision::{ray_seeds_at, ray_clear_at}`) and `CollideInit` (`entities::collide_init`).
  Back out to the walk in place (same result): a frame hook, gate or owner slot not in `entities::layout::passage`
  (an unknown owner; a car body's scatter with its gate set), an output on the pose words the owner's loop reads,
  overlapping rows, more than 16 rows (14 in the contact loops), a grid of more than 30 boxes.
- The cell grid: `E45_M40C0` reads `LookupTwin`'s link from the held `CellTable` (`collision::held_cell_link`; a cell
  with no link block yet goes through the binding, which allocates through the stand-in and fills the held slot);
  `E45_M40C0`, `E45_M46A0`, `M_D760X`, `CColGrid::SyncGrid`, `E37GridOps::M6610` and `CLink::Unlink` (0x47d160) run in
  place on held state (`collision::call_grid`; `SyncGrid` declines only over 127 edges, `M6610` only a one-point
  source). `CellLinks` (`collision/layout/cell_links.rs`, beside `CellTable`) holds the cells' lists and bodies' cell
  links as records of three words (next or first, previous or last, the cell or the count) from their first write,
  keyed by address, lowered where they differ from the image; the list code is `car_body::links::Links` (`HeldLinks`)
  in the original's write order (`CBodyLink::Unlink`/`CLink::Unlink`, `MoveToCell`, `CCar::AppendA980`). Registration
  (`LookupTwin` + `AppendA980`) is `collision::register_cell_direct`; `E45_M40C0` returns its four cells as values
  (`collision::box_cells_direct`), `E45_M46A0`'s best hit is a value (`sweep_edges_of`) but its out-pointer result
  object stays (the wheel probe reads it). Not typed: a link's fourth word, a head's spare word, the registration
  zones (no site in the step).

### Per subsystem

Evidence for every row: drive2 attempt 5 held (84084/84084), the poison build, and `round_trip` on the snapshots.

| Typed state (storage) | What is typed | On the image, why |
| --- | --- | --- |
| Struts, `spring_damper::strut::Struts` (`RefCell`, per run) | controller mode, latch, signal; effective dampings; fade pair | definition, headers, pointers; NGP's strut words `strut+0x30..0x38`, wheel `+0xe80..+0xe8c` and the out pointers (the binding writes them) |
| VCU, `vcu::layout::Unit` (`Cell`) | pedals, clutch, launch, slip, lockout, gear timer, cooldown; the drive's gear words through `DriveBlocks` | definition, pointers, owner car; CopyDef's maps, ReadPacket's packet |
| Tyres, `tyre::tyres::Tyres` (`RefCell`, per run) | contact, patch, forces, slips, outputs, heat, soil, wear; relaxation, deflections, temperatures | setup, pointers, the wheel's contact angle; `Locate`, `TyreClamp`, the friction curve, getters (callers' words) |
| Stage, `stage::typed::Stage` (read only, `Arc`) | drive line, material table, sheets | collision system and copies, `.col` tree, mesh grids, water meshes (heap graphs, per-query scratch) |
| Wheels, `wheel::layout::Wheel` (field at a time) | refresh and axes pose, walk, probe, sectors, ground copies, spin, forces, flags, four reals' dirty words | pointers, damage record, reals' other header words, every word another subsystem reads at its address (angle, axle, hub, car force, ray blocks, NGP's strut words) |
| Drive, `engine_driveline::layout::DriveTyped` (`RefCell`, per access) | lanes, pair, pack, bias, gear steps; locks, gains, ring, clutch and pair matrices, gear fields, wheel blocks | definition, damage block (its functors), pointers, state's car `+0x580` (NGP reads it raw), worker matrices (`E39_MatVec`) |
| Engine, `engine_driveline::layout::EngineTyped` (`RefCell`, per word; a word store via `word_of`) | crank, pedal, heat, stall, misfire; radiator, coolant, turbo, boost, dampers, intake, cut flags, torque terms | option block (damage handler's, NGP's wrapper writes it), roll table and seed, `EngineNoise` payload (listeners read it at its address) |
| Car's own, `car_body::layout::CarState` (field at a time) | control update and kept controls, contact times, value cells (`CarStore::cell`), speeds, respawn, overload, assistance | pointers, parts, damage handler, cell headers (`SetDirty`), `+0xaa0`/`+0xac0`, `+0x1600`/`+0x1610` (hinged bodies read them at their address) |
| Core, `physics_core::layout::Core` (`Cell`) | hit and pass counters, single stepper, substep and step counters, clock, step globals | pointers, the counter's NTweak header, the solver and its event list, entity lists |
| Collision, `collision::layout::CarCollision` | solver counts, contact and offset rows (per block on first touch, `Touched`), car grids' (ids 1-9) hit bytes, the cell grid's header and slots (`CellTable`: a slot on its first lookup, only filled slots lowered), `CellLinks` | rows' composed point `+0x40` (read at its address), body link, grids' other words, props' grids, events, pool, free list |
| Definitions, `store::defs::Defs` (read only, watched) | `WheelDef`, strut `Definition`, `TyreDef`, `CarDef`, `vcu::layout::Def`, `DriveDefinition`, `EngineDefinition`, flags word | step-written words (wheel defs `+0x880..+0x900`, tyre setups `+0x204..+0x284`), exhaust and turbo tables (lift cost), hinged and mounted defs (mount point `+0x220`, block `+0x200`), physics def but flags |
| Hinged and mounted bodies' own words, `entities::layout::Hinged`, `physics_core::layout::mounted_body::Mounted` (held with their solid, `CarSolids`: one lookup, `OwnTable`) | limit springs' positions, angle, rate, pose matrices, hinge rotation, refresh copies, pivot outputs, solve value; the mounted body's event-sent flag | owner, link, definition and enable pointers; NTweak headers and dirty words (`SetDirty`); the walks' outputs at their addresses (`Xform8130`'s `+0x5c0`, `MB560`'s `+0x5f0`, `+0x610`); exclusion sets; the mounted state and scratch objects |
| Solids, `rigid_body::layout::Solid` (per solid, held by address) | pose, velocities, acceleration estimates, sleep and window state, scratch, accumulators: car, wheels, hinged and mounted bodies | pointers, flags byte, NTweak headers, scale, state's diagonal and gate, other scratch words |

### Rules

- Borrow per access, never over a host call, where runs re-enter through the host (drive, engine, wheel, car, core);
  struts and tyres borrow per run. No lift or lower inside a step: image round trips around nested runs broke
  orderings (the force pass after `MB560`'s copy, `E43Vf70` after `E43Vf60`).
- Lazy for large state: lift on touch (collision rows) or leave on the image (the exhaust table's lift cost +0.76 % a
  step, an eager event pool +23.9 %).
- Values over pointers: entries take the callers' blocks as values and return outputs the binding writes after
  (`RigidStore::contact_input` takes a `Contact`); a pointer into the subsystem's own words keeps its write.
- Readers at an address (NGP's wrappers, hinged bodies, `CopyFrom`) show only under poison, rarely in the drive: each
  needs an accessor. A run in place on held words calls `ran_direct` (the call memo sees only the image).
- Per-word cost: a held word read was a span search, a borrow and a field lookup where the image read was one load
  (+12 % a step for the solids). Held solids keep their span as one cell array and a copy as lifted; lookups try the
  bounds, the last solid, the last gap, then a binary search; lower writes only the changed runs (`Mem::set_u32s`),
  which is sound because poison forbids image writes to held words in a step (+3.4 % against holding off).
- Definitions are watched, not poisoned: reads go through and are checked against the image (`defs::checked`), a
  write panics (`Mem::watch_writes`).

### Checks

- `scripts/gate.sh`: drive2 attempt 5, the poison build (`rbr-step/poison`: held words poisoned, definitions watched,
  held across steps) and the drive with every aid on ([verification.md](verification.md#drives-and-the-gate-matrix)).
- `round_trip` (`rbr-step` binary, snapshots): lower∘lift of each object, stage and definition answers against the
  image.

## Physics core

`src/physics_core/` (model per quantity in the file headers; everything touching memory or calling the original in
`layout/`; logs in `src/game_log/`, table search in `src/math/table.rs`). Detector vtables `PenCheck`/`Work6a70`
take `0x73f5b4`/`0x73f7b0` (structure in `layout/detector_calls.rs`). Handler-stack base `+0xe848` (top `+0xe888`,
-1 empty). The row/column helpers are `mul_rows`/`transform_xyz`, not the exe's `XformRows`/`MulVec` names. The emit
guard's `int3` sits at entry + 0x3d.
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
and `refresh_pose.rs`, NGP damage keys in `data/ngp-lsp-keys.md`). The asm has no int conversions.

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

`src/vcu/` (`CScratchSim::Step` 0x537f50 sequence, tables and locks in the file docs; typed state in "Store seam"). RSF drive2 attempt 5 parted at its first launch without PC53 rounding of the pedal and speed tables.

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

Matrix and pair-walk ops (`walk.rs`, `transform.rs`; bindings `rbr-step/src/entities/{walks,matrices,frame}.rs`):
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
  at the step's start); a walk's `b` is its row count (the shallower walkers pass 1). A scatter (`CCarBody::Vf9Cx`/
  `VfA0x`: base subtracted/added, gate clear) is `Passage::Points`; its hook call's words are written as the walk in
  place writes them (the tyre noise payload's `+0x46c` reads that dead stack).
- Residue: the child walks' words reached `CPassenger::Step`'s `MB560` source, lane 3 unwritten, copied into car
  `+0x8ec`; that lane is `RESIDUE` on the runner and the words are not written. No reader of the hooks', the
  identity walks' or the ops' own call words.

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
a static 0.83 after the temperature grip) (`--tyre-probe`, `materials` example).
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
0x530233). The load builds from the reader's items (`ntweak::plan`); the binding walks the token tables
(`ntweak::layout::parse`): each group, and each inner or member group again, is cut in place by `Tokenize` into a
0x4000-byte `operator new` table freed after it; its name and items go to the token heap (`E11_StackAlloc`,
which logs and traps when full). The class is the object's vtable (RTTI; `CLASSES` in `rbr-step` `ntweak.rs`); the
season's two classes have no key table and decline.

## Coverage gaps

No recorded drive reaches, so nothing checks: damage and crash paths (wall penetration, retire, detach, `Impulse640`),
VCU aids with the gate's aids-off drive (the aids-on drive reaches them without a recording), load and teardown
(`tests/load_*.rs` compare the load dumps), snow and tarmac data, replay and ghost paths, NTweak menus. The original's
traps: a signal index at or past 0x20 (`CMultiSignal1::Emit`, int3) and a drive-line index at or past the count
(`CDriveLine::LerpAt`, int3; `Split17C`, `Vf98x`, `Test7910` decline there).
