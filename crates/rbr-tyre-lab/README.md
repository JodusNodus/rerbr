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
- `brush`, `calibrate`: the brush-v1 tyre model and its calibration solve,
  re-exported from `crates/rbr-tyre` (the game's tyre model, in development,
  not released: `--physics experimental-brush` only).
- `examples/compare.rs`: compares brush-v1 against NGP's sweep and published
  targets. Writes CSVs plus `summary.md` into the output dir.

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
cargo run -q --release -p rbr-tyre-lab --example ngp_transient -- --image /tmp/tyre-lab/dyn.img --out /tmp/tyre-lab/ngp-dyn
cargo run -q --release -p rbr-tyre-lab --example materials -- "$RBR_NGP_DLL" [ID...]
```

No game data is committed: dumps, images and CSVs stay in `/tmp/tyre-lab`; `materials` prints only.
