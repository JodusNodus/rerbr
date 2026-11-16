# Physics map

What happens in one physics step, and where the code for each part of the car lives, in physics terms. The porting
side (layouts, bindings, float rules, the store seam) is in [notes/physics.md](../notes/physics.md) and the
[rbr-physics README](../crates/rbr-physics/README.md). Paths without a crate name are in `rbr_physics`
(`crates/rbr-physics/src/`).

## What the physics is

A rigid car body on four wheels. Each wheel has a suspension strut (spring, damper, bump stops) on a wishbone and
steering geometry, and a tyre with a contact-patch model (slip, friction curve, soil, heat, wear). An engine,
clutch, gearbox, differentials and brakes drive the wheels, a control unit (VCU) handles automatic shifting, launch
clutch, rev cut and diff locks, and the body and loose props collide with the stage's collision meshes, its brake
walls and each other. Only NGP 7.4.778's behaviour runs: where NGP edits or replaces a 1.02 function, the port holds
NGP's version and no 1.02-only arm is left (`ngp` holds NGP's own functions). One step is 1/720 s, written into the
physics definition's `MaxTimeStep` by NGP's stage init (`ngp::layout::stage::stage_init`); retail 1.02 used 0.007 s
(`rbr_recording::STEP_SECONDS`). While driving, each step is its own call; the load's countdown is NGP's 1500 steps
(`rbr_gameplay::stage::NGP_LOAD_SUBSTEPS`).

Units are SI as the code documents them: m, s, m/s, rad/s (engine speed too), N, N/m, K, Pa; mass and inertia are
stored inverted (`InverseMass`, `vecLocalInverseInertiaDiagonal`). The physics world frame is z up, right-handed (the
renderer swaps y and z into the stage files' left-handed y-up frame: `crates/rbr-render/src/coords.rs`). The car's
own axes are not documented in the physics code; NGP's telemetry takes surge as -y, sway as -x and heave as z of the
car frame ([ngp.md](../notes/ngp.md), "UDP telemetry"). Floats: NGP's own functions are SSE f32; the 1.02 code's x87
chains run at 53-bit precision in the race and 24-bit in the load settle (`fpu::precision`, `fpu::round_x87`).

## One step, in order

How a step is reached: `rbr_gameplay::game::Game::frame_with` runs the frame's steps through the
`rbr_gameplay::game::PhysicsStep` trait; `rbr-standalone` implements it with `Runner` in
`crates/rbr-standalone/src/physics.rs`, which calls `rbr_step::runner::Playback::step`. From the step entry on, every
call goes through `rbr_step`'s host to the Rust port bound for it (the `BINDINGS` tables); a call with no port
stops the run. So the order below follows the original's calls, spread over the modules each item names, not
one loop.

1. **Controls in.** The step's control payload (throttle, brake, handbrake, clutch, steering, gear, flags:
   `rbr_gameplay::control::CarControl`) goes to the car: `car_body::control::apply_control`. While the car's
   override timer runs, a neutral block with the handbrake on is used instead.
2. **Step entry.** `physics_core::step::step_forward`: the single-step gate (`physics_core::pacing::single_step_count`)
   and, on the first step only, a setup hook on every body. Then, per step:
3. **Pre-step hooks.** Each active body's pre-step hook; the car refreshes its collision-grid edge
   (`car_body::edge::refresh_edge`).
4. **Body pass**, `physics_core::step::step_bodies`: three walks over the active bodies (the car, woken props).
   1. Force input: the children's force input (`entities::tree::walk_gated`), then gravity from the physics
      definition's `vecGravity` (`physics_core::accumulate::accumulate_forces`, `physics_core::math::gravity_force`).
   2. Force to acceleration. For the car, `car_body::forces::apply_forces`: aero drag and spin damping
      (`car_body::drag::apply_drag`, with NGP's drag factor), brake-wall friction
      (`car_body::wall_friction::wall_friction`), the gyroscopic torque, then `rigid_body::force::force_pass`
      (contact vote, force times inverse mass, torque times local inverse inertia, accumulators cleared).
   3. Integrate, per body. For the car, `car_body::step::step_car`: the passenger
      (`physics_core::passenger_step::passenger_step`), the rigid body (`rigid_body::integrate::integrate`:
      velocities, pose, sleep), the brake-wall raycast and its impulse, hinged and mounted parts
      (`entities::hinge_step`, `physics_core::mounted::mounted_position_step`; their own words typed with their solids,
      `entities::layout::Hinged`, `physics_core::layout::mounted_body::Mounted`), the timekeeper and the retire check
      (`car_body::retire::check_retire`).
   4. Child walk, right after each body's integrate. For the car, `car_body::controls::step_controls`, in order:
      steering slew and rack; the VCU (`vcu::step::step`); the drive (step 5); the wheels (step 6); the anti-roll
      bars (`car_body::roll_bars::update_roll_bars`); the force-feedback torque (`car_body::layout::update_ffb_torque`).
5. **Engine and driveline**, `engine_driveline::layout::step_drive`, in order: the engine step through NGP's wrapper
   (wear and death in `ngp::engine`, air from `ngp::atmosphere::atmosphere`, the engine sim
   `engine_driveline::layout::engine_step`, NGP's torque `ngp::engine::torque`); the starter
   (`ngp::starter::starter`); each wheel's brake (`ngp::brake::brake_step`); gear trim and clutch
   (`engine_driveline::drive_worker::clutch_torque`); then the wheel worker: NGP's downforce
   (`ngp::downforce::downforce`), drivetrain loss and diff lock torques (`ngp::diff::after_loss`,
   `ngp::diff::lock_torques`), and the driven-wheel solve (`ngp::axle::axle_worker` for drivetrain types 4 and 5,
   else `engine_driveline::drive_worker::wheel_worker`). With `DriveType` 3 the drive takes
   `engine_driveline::drive_worker::pair_worker` in place of that whole wheel worker.
6. **Wheels and tyres.** The car's child walk (`entities::tree::walk_gated`) reaches each wheel's step,
   `wheel::layout::wheel_step`: suspension geometry for the strut's travel (`wheel::layout::refresh`, model in
   `wheel::refresh`), the ground probe (`wheel::layout::ground_probe`: three rays, surface tags blended by
   `ngp::tag_blend::blend`), the tyre (`tyre::step::step`, inside NGP's speed and surface scaling
   `ngp::step_wrapper`), then the strut (`spring_damper::layout::step`, with NGP's bump stop and rebound
   `ngp::spring`) and the tyre's temperature relaxation.
7. **Collisions and contacts**, every step unless sub-stepping is set (`physics_core::pacing::contact_due`; off on
   every snapshot): `physics_core::contact_loop::collide_window` over a window of 20 steps (detect with
   `physics_core::layout::run_collision_detectors`, take the earliest event, back-step the bodies to it, solve,
   advance the rest); the penetration check (`physics_core::layout::penetration_check`); the solver
   (`collision::solver_pass`: `clear_contact_rows`, `event_count`, `refresh_events`, `correct_penetrations`,
   `solve_due_events`, arithmetic in `collision::contact_solver`); then each body's post-contact hooks (the car
   resets its per-step scratch: `car_body::scratch_dispatch::dispatch_scratch_after_reset`).
8. **End-of-step hooks and clock.** The end-of-step hook of each body that has one (the car's respawn countdown:
   `car_body::layout::run_countdown`), then the clock (`physics_core::pacing::advance_time`).
9. **Outputs.** Nothing is returned: the step leaves the state in memory. Gameplay samples the car after each step
   (`car_state` in `crates/rbr-standalone/src/physics/layout.rs`, into `rbr_gameplay::game::StepSample`), and
   the renderer, sound and force feedback read memory without writing it.

By the order of the walks, the forces the wheels, tyres and struts add in step 4.4 reach the body's velocity in the
next step's body pass (derived from the call order, not traced word by word).

## Where to find

| Quantity or concept | Module or file | Main type or fn |
| --- | --- | --- |
| Tyre slip | `tyre/slip.rs` | `tyre::slip::compute_slip`, `weigh_models` (sticking against sliding), `tyre::tyres::Tyres` (the tyres' typed state) |
| Tyre contact patch, normal force | `tyre/contact_patch.rs`, `tyre/solve.rs`, `tyre/forces.rs` | `tyre::contact_patch::contact_patch`, `tyre::forces::solve_forces` |
| Tyre heat and wear | `tyre/heat.rs`, `tyre/relax.rs`, `ngp/heat.rs` | `tyre::heat::heat_step`, `tyre::relax::relax_sector`, `ngp::heat::TyreThermal` |
| Grip, friction curve | `tyre/friction_curve.rs`, `tyre/friction_limits.rs`, `tyre/grip.rs`, `ngp/outputs.rs` | `tyre::friction_curve::friction_curve`, `tyre::friction_limits::friction_limits`, `tyre::grip::limit_to_friction_circle`, `ngp::outputs::friction_limits` |
| Tyre on surface, wetness, inclination | `tyre/surface.rs`, `tyre/soil.rs`, `ngp/inclination.rs` | `tyre::surface::surface_friction`, `tyre::soil::soil_drag`, `ngp::inclination::grip_scale` |
| Suspension geometry | `wheel/refresh.rs`, `wheel/axes.rs` | `wheel::layout::refresh`, `wheel::layout::Wheel` (the wheels' typed state) |
| Spring, bump stop, damper | `spring_damper/mod.rs`, `spring_damper/suspension.rs`, `spring_damper/strut.rs`, `ngp/spring.rs` | `spring_damper::step`, `spring_damper::suspension::Suspension`, `spring_damper::strut::Struts` (the typed state), `ngp::spring::bump_stop`, `ngp::spring::latch` |
| Anti-roll bars | `car_body/roll_bars.rs` | `car_body::roll_bars::update_roll_bars` |
| Engine torque | `engine_driveline/engine.rs`, `engine_driveline/layout/engine_sim.rs`, `ngp/engine.rs` | `engine_driveline::engine::torque_curve`, `engine_driveline::layout::engine_step`, `ngp::engine::torque`, `engine_driveline::layout::EngineTyped` (the engine's typed state), `EngineDefinition` (its definition, held read only) |
| Starter | `ngp/starter.rs` | `ngp::starter::starter` |
| Clutch | `engine_driveline/drive_worker.rs`, `ngp/drive.rs`, `vcu/clutch.rs` | `engine_driveline::drive_worker::clutch_torque`, `ngp::drive::clutch_state`, `vcu::clutch::launch_clutch` |
| Differentials, torque split | `ngp/diff.rs`, `ngp/axle.rs`, `ngp/drive.rs`, `vcu/locks.rs` | `ngp::diff::lock_torques`, `ngp::axle::axle_worker`, `ngp::drive::split_factor`, `vcu::locks::diff_locks` |
| Gearbox | `engine_driveline/drive.rs`, `engine_driveline/drive_worker.rs` | `engine_driveline::drive::gear_ratio`, `request_gear`, `engine_driveline::drive_worker::gear_logic`, `engine_driveline::layout::DriveTyped` (the drive's typed state) |
| VCU (auto shift, rev cut) | `vcu/` | `vcu::step::step`, `vcu::gear::update`, `vcu::cut::rev_limit_cut`, `vcu::layout::Unit` (the typed state) |
| Aero drag | `car_body/drag.rs` | `car_body::drag::apply_drag` |
| Downforce, air density | `ngp/downforce.rs`, `ngp/atmosphere.rs` | `ngp::downforce::downforce`, `ngp::atmosphere::atmosphere` |
| Brakes | `ngp/brake.rs`, `engine_driveline/drive.rs` | `ngp::brake::brake_step` (torque, disc temperatures), `engine_driveline::drive::set_brake_in` |
| Steering | `car_body/controls.rs` | `car_body::controls::step_controls`, `car_body::layout::CarState` (the car's own typed state) |
| Mass, inertia, integration | `rigid_body/force.rs`, `rigid_body/integrate.rs` | `rigid_body::force::force_pass`, `rigid_body::integrate::integrate`, `rigid_body::layout::Solid` (the solids' typed state, held over the step; their readers on its store, `rbr_step::rigid_body::on_solids`) |
| Gravity | `physics_core/accumulate.rs`, `physics_core/math.rs` | `physics_core::accumulate::accumulate_forces`, `physics_core::math::gravity_force` |
| Step pacing, physics clock | `physics_core/step.rs`, `physics_core/pacing.rs`, `physics_core/contact_loop.rs` | `physics_core::step::step_forward`, `physics_core::pacing::advance_time`, `physics_core::layout::Core` (the core's typed state: clock, substep counter, pass and hit counters) |
| Ground surface, materials | `entities/ground.rs`, `entities/material_table.rs`, `ngp/material.rs`, `ngp/tag_blend.rs`, `stage/` | `entities::ground::box_probe`, `entities::material_table::MaterialTable`, `ngp::material::cell`, `ngp::tag_blend::blend`, `wheel::layout::ground_probe` |
| Stage world (`.col`, `.mat`, `.trk`) | `stage/`, `engine_driveline/drive_line.rs`, `ngp/drive_line.rs` | `stage::layout::load_col`, `ngp::drive_line::project` (drive line), `stage::typed::Stage` (what a step reads, typed) |
| Collision grid | `collision/cells.rs`, `collision/mesh_grid.rs`, `physics_core/detect.rs`, `load/car_grids.rs` | `collision::mesh_grid::MeshGrid`, `collision::cells::point_cell`, `physics_core::detect::box_cells`, `collision::layout::{CellTable, CellLinks}` (the cell grid's header, slots, lists and cell links, typed) |
| Collision response | `collision/contact_solver.rs`, `collision/solver_pass.rs`, `collision/obb.rs`, `collision/wall.rs` | `collision::contact_solver::normal_impulse`, `friction_impulse`, `collision::obb::obb_contact`, `collision::wall::wall_raycast`, `collision::layout::CarCollision` (the solver's rows and counts and the car's grids' hit bytes, typed) |
| Damage | `car_body/damage.rs`, `car_body/damage_ops.rs`, `car_body/damage_functors.rs`, `wheel/axes.rs` | `car_body::damage::set_level`, `car_body::damage_ops::level_fractions`, `ngp::engine::damage_factor` |
| Force feedback | `car_body/steering_ffb.rs`, `ngp/ffb.rs` | `car_body::steering_ffb::filtered_torque`, `ngp::ffb::curve` |
| Car and setup values from `.lsp` | `load/`, `ntweak/`, `store/defs.rs` | `load::physics_file`, `load::setup_file`, `ntweak::plan::plan`, `store::defs::Defs` (the definitions a step reads, typed) |

The `.lsp` keys each object reads are in [data/physics-lsp-fields.md](../data/physics-lsp-fields.md) and
[data/ngp-lsp-keys.md](../data/ngp-lsp-keys.md).

## Changing a physics value safely

The rule: physics stays bit-exact against the original's recordings, so a change to anything in `rbr-physics` or the
bindings must not change one bit of any step. A change that makes the car behave differently is not a fix here; it
goes in experimental physics (below).

| Check | Proves |
| --- | --- |
| `scripts/rust-check.sh` | fmt, clippy and the tests; must print PASS |
| `scripts/gate.sh` | drive2 attempt 5 through `rbr-standalone`: every one of 84084 NGP steps equal to the recording; the drive with the typed state's words poisoned on the image ([physics.md](../notes/physics.md), "Store seam"); the drive with every aid on runs without a stop |

The recorded drives are the only oracle: there is no per-call check against the original, so a branch no recorded
drive reaches is not checked ([verification.md](../notes/verification.md)). A new recording is the way to cover one.

Experimental physics, which may differ from NGP, goes outside `rbr-physics`: engine models in `crates/rbr-engine`,
tyre models in `crates/rbr-tyre`, hooked in through `rbr_step::ngp::set_engine_model`, `set_tyre_model`,
`set_aero_model` and `set_four_wheel_drive_loss`, and chosen by `--physics` profiles
(`crates/rbr-standalone/src/profile.rs`). With no model set the code is NGP's. See
[experimental-physics.md](experimental-physics.md) and [tyre-model.md](tyre-model.md).
