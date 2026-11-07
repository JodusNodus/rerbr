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
- `rbr-standalone --tyre-image FILE [--tyre-image-step N]` (with `--autopilot`):
  a memory image of the four wheels' tyre footprints (scratch, state, setup,
  wear table, contact record, NGP's `.data`) plus a `txt` sidecar with the
  addresses and thermal scalars. Read-only, as the probe.
- `examples/ngp_transient.rs`: loads the image into a `Mem` and runs NGP's
  real per-wheel tyre step (`entries::step` + `entries::relax_count`, exactly
  the wheel step's sequence) under scripted kinematics: validation against the
  sweep, lateral/longitudinal steps, sinusoidal steer, low speed, heat.
- `src/probe.rs`: the probe-dump parser shared by the examples.
- `src/brush.rs`: the brush-v0 candidate tyre model (ours, experimental):
  steady-state brush with combined slip, Grosch-like sliding-speed friction,
  load/temperature/wear/pressure factors, a wet loss and a single-point
  transient, with unit tests. The nominal-point calibration lives in
  `Calibration`, built at run time; only the spec's own constants are
  hard-coded. Lab code only: nothing is wired into the game.
- `src/calibrate.rs`: solves the `Calibration` from measured peak, slide and
  slope targets per axis (nested fixed-point/Newton from round starts), fed
  values by the caller. No NGP-derived numbers in committed code.
- `examples/compare.rs`: compares brush-v0 against NGP's sweep and published
  targets. Writes CSVs plus `summary.md` into the output dir.

## Use

```sh
cargo build --release -p rbr-standalone
./target/release/rbr-standalone --autopilot OUT --boot --stage track-85_M \
    --steps 8000 --top-speed 60 --circle -260,120,40 \
    --rsf-car Ford_Fiesta_Rally2_ngp7 --tyre-probe /tmp/tyre-lab/x.probe \
    --data "$RBR_DATA" --ngp-physics "$RBR_NGP_PHYSICS"
cargo run -q -p rbr-tyre-lab --example sweep -- \
    --probe /tmp/tyre-lab/x.probe --out /tmp/tyre-lab/ngp
```

```sh
./target/release/rbr-standalone --autopilot OUT --boot --stage track-85_M \
    --steps 8000 --top-speed 60 --circle -260,120,40 \
    --rsf-car Ford_Fiesta_Rally2_ngp7 --tyre-image /tmp/tyre-lab/x.img \
    --tyre-image-step 8000 --data "$RBR_DATA" --ngp-physics "$RBR_NGP_PHYSICS"
cargo run -q -p rbr-tyre-lab --example ngp_transient -- \
    --image /tmp/tyre-lab/x.img --out /tmp/tyre-lab/ngp-dyn
```

No game data is committed: probe dumps and CSVs stay in `/tmp/tyre-lab`.
