# Engine model (ours, experimental)

`crates/rbr-engine`'s `realistic` model: a mean-value SI engine (Hendricks; Eriksson–Nielsen as in Andersson 2017)
that replaces NGP's torque formula, calibrated per car on its NGP WOT table. Not bit-exact by design; it never runs in
the faithful physics. Sources: "report" is the engine research report (`/tmp/engine-research-2/report.md`), "R1" the
throttle research (`/tmp/throttle-research/report.md`, `cars.md`); formulas and constants are in the doc comments.

## Rules

- f64 inside, f32 at the interface; `libm` for every transcendental; no allocation, hashing, time or randomness per
  step (the brief). Two runs give the same bits (`tests/model.rs`).
- Any input is sanitised (`engine::Step::from_inputs`): NaN, infinities, 0 or negative dt and speed give finite torque
  (`tests/model.rs`).
- Calibration: about 2 s per turbo car, 30 ms per NA car (release); a step about 2.4 µs.

## Class (`class.rs`)

- Report §6 rules 1–9 on the RSF group names; the year from `cars_data.json`, else the group's middle era.
- Per-cylinder throttles for NA Group 4, Group B, Group A8, Super 2000 and Group A7 from 1993; ECU idle from 1987, a
  fixed stop before (the brief; inferred).
- Facts by folder (R1 `cars.md`): cylinders (Audi A2 5; 911 SC and SC RS 6; Fiesta Rally3, Rally3 Evo, Rally4, 208
  Rally4, GR Yaris Rally2 3); Evo IX N4 cable (24); Clio III R3 DBW (33); Golf II GTI 16V fixed idle stop, K-Jetronic
  (40); Fiesta R2 no anti-lag (12).
- WRC boost cap 2.5 bar abs from 2011 (FIA 255A), 4.0 before (report §5.5; 4.0 inferred).

## Air path (`gas.rs`, `engine.rs`)

- Orifice Ψ choked below 0.528, linear from Π 0.98 to 0 (R1 §1.3); plate area 1 − cos θ/cos 7°, θ = 7° + 83°·lift.
- C_D 0.80 one throttle, 0.85 per cylinder with at least 45 mm per cylinder; manifold 0.8·V_d, 0.3·V_d per cylinder,
  1.0·V_d turbo (the brief).
- Fully implicit Euler (Illinois root) per sub-step: the brief's linearly implicit step overshot a per-cylinder tip-in
  to 2.4 bar.
- Leak 0. The brief's leak (p_im 0.2·p_a at 3000 rpm, fuel cut) idles every sample 55–95 N·m too fast. Instead an
  overrun air, sized to that 0.2·p_a point, opens only while the fuel is cut (inferred; parent informed).
- DBW lift: plate area for p_req minus the closed point's area, so the idle air is not counted twice (inferred).

## Torque and friction (`friction.rs`, `calibrate.rs`)

- K(n) from the identity K = 4π(T_wot + friction + pumping)/charge on a 250 rpm grid and every knot, each from a
  cold tip-in steady state (inferred: turbo steady states are path dependent near the threshold).
- s_f from closed plate, fuel cut, steady at RPMLimit = −BrakeCoefficient·MaxTorque, clamped 0.6–2.0 (report §1.4).
- Every installed turbo car clamps: raw 2.3–4.9, as the brake target scales with the boosted MaxTorque. NA cars are
  1.30–1.83 (`tests/rsf_cars.rs`, 42 cars).
- η_m = 1, losses explicit; friction fades to 0 within 5 rad/s of standstill (inferred).

## ECU (`ecu.rs`)

- Idle: PI on air, P on spark (a spark integrator fights the air loop; inferred). Gains come from the linearised
  idle at 0.5/3 Hz with the car's inertia. The loops run only with a closed pedal below idle + 800 rpm (inferred).
- DFCO for every car class (inferred). Limiter: ignition cut over 200 rpm, then fuel (report §4). The events use
  error diffusion.
- Anti-lag: report §5.6 values. The 15 % kick is an area (lift 0.31): a 15 % lift opens 5 % of the area, the
  manifold falls to 0.3 bar and the turbo runs down (re-apply 0.35 s). BOV shut while active (inferred).

## Turbo (`turbo.rs`)

- Speed line with fan-law scaling ν = (ω/ω*)², a continuous surge band 0.05, and an efficiency ellipse in flow only:
  the Π axis made the steady state bistable (inferred).
- Restrictor solved with the compressor by root finding (a two-step oscillation otherwise). T_em is 700 °C at zero
  flow, 950 °C at rated flow, capped at 1150 °C (700 inferred).
- A_t: bisection for the largest nozzle whose cold spool reaches 0.9·π_max at the spool knot; the wastegate area
  equals A_t (inferred, replaces the brief's shut-gate steady boost).
- Wastegate band 0.5 bar, Ti 0.5 s (a limit cycle at narrower settings), τ 125 ms. The spring gate is P-only on gauge
  boost. The electronic gate is PI on absolute boost, so it makes up altitude (inferred).

## Open

- The cable turbo sample (Group A8 1993, spring gate) re-applies after a 1 s lift in 0.17 s (brief: under 0.15 s).
  Its turbo runs down to 1.46 bar under a 15 % area kick; a 20 % kick holds 2.2 bar and passes. The test allows
  0.2 s for cable cars.
- Turbo steady part-pedal curves dip near the boost threshold (path dependent). The threshold is sharp, about
  1.4 bar at 3000 rpm and at target by 3250–3500 rpm, and K jumps there (about −30 % over 200 rpm).
- At a fixed speed below idle, the closed pedal gives up to +200 N·m because the idle loop saturates (bench artefact
  unless a car is driven there).
- The spark loop saturates (gain about 2°/rpm). Limiter torque swings each step (mean value per step). The
  wastegate overshoots about 5 %.
