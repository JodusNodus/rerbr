# rbr-engine (ours, experimental)

Engine models behind one interface (`Model`, `model(name, params)`). The `realistic` model is a mean-value SI engine
(air path, Sandoval–Heywood friction and pumping, an ECU with idle, overrun cut, limiter and anti-lag, and a turbo
with wastegate and restrictor). Each car's hardware class comes from its group and year, and the model is
calibrated so steady WOT reproduces the car's NGP torque table. The tests use synthetic cars (`samples`), plus every
installed RSF car when `RBR_RSF` is set. `examples/curves.rs` writes steady and transient CSVs, and `examples/plot.py`
plots them. Facts and sources: `notes/engine-model.md`.
