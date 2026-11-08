# rbr-tyre-lab

Research tooling (ours, experimental): characterise NGP's tyre model as it
actually runs, as a quantitative baseline for a later modern-tyre comparison.
Nothing here changes what the game computes.

## Parts

- `rbr-standalone --tyre-probe FILE` (with `--autopilot`): after every 1000th
  step and the final step, dumps each wheel's tyre `Frame` (setup tables and
  operating point) as `name: values` lines. Read-only: it reads physics memory
  after the step, never writes it. See `crates/rbr-standalone/src/tyre_probe.rs`.
- `examples/sweep.rs`: takes a probe dump and sweeps NGP's pure tyre functions
  (`patch_forces`, `friction_limits`, through `rbr-physics`, never copied)
  over slip and load. Writes CSVs plus `summary.md` into the output dir.
  `--friction S,K` puts a surface's static and kinetic friction in place of
  the probed one, for a material no run reaches.
- `rbr-standalone --tyre-image FILE [--tyre-image-step N]` (with `--autopilot`):
  a memory image of the four wheels' tyre footprints (scratch, state, setup,
  wear table, contact record, NGP's `.data`) plus a `txt` sidecar with the
  addresses and thermal scalars. Read-only, as the probe.
- `examples/ngp_transient.rs`: loads the image into a `Mem` and runs NGP's
  real per-wheel tyre step (`entries::step` + `entries::relax_count`, exactly
  the wheel step's sequence) under scripted kinematics: validation against the
  sweep, lateral/longitudinal steps, sinusoidal steer, low speed, heat.
  `--fz N`, `--speed M/S` and `--ramps-only` give the whole step's slip-angle
  (0-30°) and slip-ratio (locked to 1) ramps at another load and speed, which
  include the soil's lateral force the pure sweep leaves out.
- `src/probe.rs`: the probe-dump parser shared by the examples.
- `brush`, `calibrate`: the brush-v1 tyre model and its calibration solve,
  re-exported from `crates/rbr-tyre` (the game's tyre model, in development,
  not released: `--physics experimental-brush` only).
- `examples/compare.rs`: compares brush-v1 against NGP's sweep and published
  targets. Writes CSVs plus `summary.md` into the output dir.
- `examples/low_speed.rs`: the brush's force dynamics on one wheel, a quarter
  car stepped explicitly at 720 Hz, for NGP's speed-scaled torsional stiffness,
  Rill's carcass and the game's longitudinal carcass stiffness (450 kN/m at
  the calibration pressure), down to standstill: slip steps, the wheel-tyre
  mode at 0.5-10 m/s, pull-away, braking to a stop and holding on a grade.
  Prints only.
- `examples/loose.rs`: loose-v1 (the game's tyre on soil) against NGP's sweep
  on a probe frame on gravel or snow and the published loose-surface curves:
  μ_y(α), μ_x(κ) driving and braking, a locked wheel's side force, the peaks
  against load. Prints only.
- `examples/scorecard.rs`: the tyre-level numbers of `harness/tyre_scorecard.py`: steady forces of the tyre model
  a profile runs (`--model ngp|brush`, NGP's where it hands back) on a probed wheel, as `key value` lines. Prints
  only.
- `examples/envelope.rs`: a profile's tyre model against NGP's on a probed wheel: peak μ_y and μ_x (driving,
  braking) against load, speed and tread temperature, small slips, the friction circle, and with `--frames` the
  peaks at the loads and speeds of every frame in the probe dump. Prints only. Both examples ask the model through
  `src/steady.rs`, as the game's seam does.
- `examples/drive_force.rs`: |Fy|/Fz and the slip ratio at a given drive or braking force Fx/Fz, against slip angle
  and load, NGP's tyre against a profile's model on a probed wheel (the throttle sets the force, not the slip).
  Prints only.
- `examples/racing.rs`: brush-v1 and NGP at the conditions of the racing-tyre
  and combined-slip data (`notes/tyre-model.md`, "Racing and rally targets").
  Prints only.

## Use

In bash, with the paths from `scripts/dev-env.sh` (`--rsf-car` needs `RBR_RSF`; set `RBR_NGP_DLL` to NGP
7.4.778's `NGP.dll` when neither the data nor the RSF install holds one). One run gives both captures:

```sh
. scripts/dev-env.sh
cargo build --release -p rbr-standalone
./target/release/rbr-standalone --autopilot /tmp/tyre-lab/x.out --boot --stage track-85_M \
    --steps 8000 --top-speed 60 --circle -260,120,40 --rsf-car Ford_Fiesta_Rally2_ngp7 \
    --tyre-probe /tmp/tyre-lab/x.probe --tyre-image /tmp/tyre-lab/dyn.img --tyre-image-step 8000 \
    --data "$RBR_DATA" --ngp-physics "$RBR_NGP_PHYSICS"
cargo run -q --release -p rbr-tyre-lab --example sweep -- --probe /tmp/tyre-lab/x.probe --out /tmp/tyre-lab/ngp
cargo run -q --release -p rbr-tyre-lab --example compare -- --probe /tmp/tyre-lab/x.probe --out /tmp/tyre-lab/cand
cargo run -q --release -p rbr-tyre-lab --example racing -- --probe /tmp/tyre-lab/x.probe
cargo run -q --release -p rbr-tyre-lab --example loose -- --probe /tmp/tyre-lab/gravel.probe --wheel 1
cargo run -q --release -p rbr-tyre-lab --example low_speed -- --probe /tmp/tyre-lab/x.probe
cargo run -q --release -p rbr-tyre-lab --example ngp_transient -- --image /tmp/tyre-lab/dyn.img --out /tmp/tyre-lab/ngp-dyn
cargo run -q --release -p rbr-tyre-lab --example ngp_transient -- --image /tmp/tyre-lab/dyn.img --sweep /tmp/tyre-lab/ngp \
    --fz 2000 --speed 25 --ramps-only --out /tmp/tyre-lab/ngp-ramps
cargo run -q --release -p rbr-tyre-lab --example materials -- "$RBR_NGP_DLL" [ID...]
cargo run -q --release -p rbr-tyre-lab --example surface_across -- "$RBR_DATA" STAGE x.trace [WEAR WETNESS] [EVERY_M] [OFFSETS]
```

`materials` prints each id's friction, soil and water values and its surface class (the tyre's per-class factor).
`surface_across` prints the two materials and the blend weight under a `--trace-speed` path and at lateral offsets
(default ±1/2/3 m), from the stage's `.col` and `.mat` as NGP's sheet lookup reads them.
An ice probe: the autopilot line above with `--material-as 116` (every material takes NGP's ice values).

No game data is committed: dumps, images and CSVs stay in `/tmp/tyre-lab`; `materials` and `surface_across` print only.
