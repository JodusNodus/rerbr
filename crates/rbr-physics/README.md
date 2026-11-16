# rbr-physics

Bit-exact ports of the 1.02 physics functions, plus NGP 7.4.778's own (precision: `src/fpu.rs`). Behaviour, precision
and porting rules: [notes/physics.md](../../notes/physics.md). Checks against the recordings:
[notes/verification.md](../../notes/verification.md). The step in physics terms and where each quantity lives:
[docs/physics-map.md](../../docs/physics-map.md).

## Code style

The crate is physics code in its own terms that produces the original's bits, not a transcription of the
decompile. [`ngp/heat.rs`](src/ngp/heat.rs) is the reference.

- Model code (a subsystem's modules outside `layout`) names physical quantities and roles: `carcass_radius`, `spin`,
  `slip_ratio`, `heat_flow_to_air`, `TyreShape`, with units in the docs. Inputs that belong together are a type.
- No addresses, offsets, original function or class names, calling conventions, registers or `st0` in model code or
  its docs. They live in `layout`/`layout/` (memory), the `rbr-step` bindings (named after the original's
  functions), `ports.csv` (address to Rust item), the tests and the notes.
- A name says what the evidence shows: a tweak key, the formula's role, units and values. Where the meaning is not
  known, a neutral role name (`blend_weight`) documented as named by use, never `f_1e8` or `w7c`, and no invented
  physics.
- Constants are plain literals or `std` constants (`TAU`, `293.14`, `0.78_f32 as f64`), not bit patterns, where the
  value is exactly the same.
- Exactness first: every kept computation keeps its operation order and rounding (`fpu` helpers for the x87 chains,
  comments for unordered compares like `maxss`). A rename never changes a result; the recorded drives prove it.

## Modules

Raw offsets live only in each module's layout code. Tests are in `tests/`.

| Module | Ports | Layout |
| --- | --- | --- |
| `vcu` | CVehicleControlUnit | `layout.rs` |
| `spring_damper` | CSpringDamper, E48 | `layout.rs` |
| `tyre` | CTyre and the CScratch step | `layout/` |
| `wheel` | CWheel, E51Wheel | `layout/` |
| `car_body` | CCar, CCarBody, damage, lights, aero | `layout/` |
| `engine_driveline` | CDrive, drive line, CEngine, E39-E43 | `layout/` |
| `rigid_body` | CSolid, E47 | `layout/` |
| `collision` | E33 bodies, E34 brake walls, E36-E38 | `layout/` |
| `entities` | CEntity, frames, CGround, CHingedRigidBody | `layout/` |
| `physics_core` | CPhysics step, detectors, mounted bodies, passenger, E45/E46 | `layout/` |
| `ngp` | NGP.dll's own functions | - |
| `load`, `ntweak` | car definitions and setups from `.lsp`, the NTweak apply walk | `layout/`, `ntweak/layout.rs` |
| `stage` | the stage's physics world from `.col` and `.mat` | `layout/` |
| `math`, `fpu` | rsqrt/rcp tables (`data/`), ftol, quaternions, x87 rounding | - |
| `mem`, `snapshot`, `store` | game memory, snapshot reader, the storage seam | `store/layout.rs` |

The per-module READMEs (port plans, working names, open questions) were removed; comments that cite them say
`<module>/README.md` (git 9c28c0b8), readable with `git show 9c28c0b8:rust/rbr-physics/src/<module>/README.md`.

The `poison` feature makes accesses to bytes another store owns panic (check builds only).

## ports.csv

One row per ported function: `addr,module,rust_item`. Add the row in the commit that adds the port and keep rows
sorted by address. `tests/ports_manifest.rs` checks that each module and item exists.

## Related crates

- `crates/rbr-step`: the game's physics step on the ports and the typed state.
- `crates/rbr-standalone`: runs the pure Rust step against a recording (`--boot`).
