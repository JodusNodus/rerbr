# Events (ours)

Rally events the player builds from presets, on any installed stage (retail, RSF, BTB), against an AI field in their
car class. Code: `crates/rbr-events` (`config`, `pace`, `field`, `event`); the faithful 1.02 season stays in
`rbr-championship`. Nothing gameplay- or physics-facing: the event layer only reads drivelines and hands the launcher
times, standings and saves.

## Model

- `pace`: a quasi-static speed plan along the driveline's Hermite spline (`driveline_points`: the game's own
  Hermite through the `.trk` records, tangents fitted as the autopilot fits them; `timed_slice` cuts the `.dls`
  start/finish). Samples at ~1.5 m; speed limit `sqrt(grip_eff / curvature)` capped at top speed; forward pass with
  acceleration falling linearly to 0 at top speed, backward pass with braking; trapezoid time integral.
- Curvature is the *signed* heading change, smoothed over ~19 m before taking the absolute: the records' lateral
  jitter weaves left and right and cancels out, while real corners keep their sign. Smoothing the absolute
  curvature instead counts every weave as a corner (found on map 61: 69 rad of smoothed-signed turning vs ~2.5 rad
  net heading change).
- `grip_eff` = class grip x surface (tarmac 1, gravel 0.75, snow 0.50) x (1 - 0.25 x wetness). Braking scales with
  the grip.
- `field`: fictional international crews (nothing from the game's roster, which stays out of the source). Skills
  spread below the best: best = 1 - 0.08 x (1 - difficulty), rest up to 0.09 below, rescaled so the best equals the
  reference at difficulty 1. Stage time = reference / skill with central noise, rarer 5-60 s losses and a small
  retire chance, wider spread when wet. All draws from `CrtRand`, seeded per (seed, rally, stage).
- `event`: per rally one surface (Mixed draws one per rally from the pool's own), one stage family preferred
  (retail `track-NN`, RSF/BTB the id before `-`), sampled without repeating unless the pool is too small. Rally
  names from the first stage minus its variant (`Autiovaara II` -> `Autiovaara Rally`). Clocks from 08:00 (21:00 at
  night; realistic puts the last leg at dusk/night), changeable weather per leg, ~20 min mid-rally services and
  45 min overnight in a weekend. Points use the modern 25-18-15-12-10-8-6-4-2-1 top-10 scale per rally; seasons sum
  rally points. Adaptive moves every entrant's skill by at most 0.003 a stage toward the player finishing 2nd-4th.
  Save is a versioned `|`-separated text format (`rbr-events 1` head), backslash-escaped, no serde.

## Calibration

`crates/rbr-events/examples/pace.rs` prints every installed stage's length and reference time per class. Retail AI
column runs the 1.02 championship AI's best driver (`rbr-championship` `ai::stage_time`, level 2, dry, mean of 8
seeds) over the dry `ai.ini` rows; it matches the `times.ini` developer records within a few seconds everywhere.

- WRC/top class vs the AI, totals: gravel 0.992 (39 stages), tarmac 1.009 (12), snow 0.934 (6); median stage 0.97.
- Other classes follow by power/weight/grip ratios (WRC 9.6/9.2/11.5/58 down to Group 2 6.8/4.5/8.0/43).
- Real-world check: fast gravel WRC ~109-126 km/h, twisty tarmac ~89-96, snow ~74-100; Rally2 ~7 % slower,
  Group 2 ~21 % slower.
- Residuals are the method's, not the constants': the driveline is a lossy record of the road (dense drivelines
  keep positioning weave the pars ignore; sparse ones cut corners the pars charge for), and the AI pars are
  hand-tuned per track. Outliers: maps 61/66 (fast per the AI, twisty per the driveline) and 54/56 (slow per the
  AI, cut per the driveline), each ~30-60 % off while the middle half of stages sits within ~10 %.

## Open items

- The launcher builds `StageRef`s and calls `reference_time` (parallel work); BTB stages have no drivelines yet,
  so they need reference times another way (length/surface estimate) until they do.
- Wetness comes from the itinerary's conditions; the launcher maps conditions to the stage's own weathers.
