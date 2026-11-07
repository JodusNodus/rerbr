# Engine model (ours, experimental)

`crates/rbr-engine`'s `realistic` model: a mean-value SI engine that replaces NGP's torque formula, calibrated per car
on its NGP WOT table. Not bit-exact by design; it never runs in the faithful physics. Formulas and constants are in
the doc comments. Each line gives its evidence: a source (list at the end), or "inferred" with the reason. Tags:
**generic** (textbook engine physics), **guess** (a number with no source; the gaps).

## Rules

- f64 inside, f32 at the interface; `libm` for every transcendental; no allocation, hashing, time or randomness per
  step (inferred: bit-identical on every platform). `tests/model.rs` checks two runs give the same bits and any input
  (NaN, infinities, 0 or negative dt and speed) gives finite torque.
- Calibration about 1.3 s per turbo car (its steady runs on threads, each independent, so the bits do not depend on
  them), 30 ms per NA car (release); a step about 2.4 µs (measured). Steady means the pressures and the turbo's speed
  move under 1e-4 of themselves over 0.25 s (inferred).

## Class (`class.rs`)

- Actuation, ALS, restrictor and boost cap by RSF group and year: the FIA articles per class [FIA 254], [FIA 254A],
  [FIA 255], [FIA 255A], [FIA 260], [FIA 261], [rallycars restrictors]; DBW from 1999 in WRC [Impreza WRC]; the year from `cars_data.json`, else
  the group's middle era (inferred: the RSF list has years for nearly every car).
- A throttle per cylinder for NA Group 4, Group B, Group A8, Super 2000, Group A7 from 1993 (inferred from the
  engines in [tech-racingcars 911], [tech-racingcars Golf] and [HPA Alpha-N]: carburettor chokes, slides, ITBs).
- An ECU idle loop from 1987, a fixed idle stop before (inferred: carburettors and mechanical injection).
- Facts by folder: cylinders (Audi A2 5 [tech-racingcars Audi A2]; 911 SC, SC RS 6 [tech-racingcars 911]; Fiesta
  Rally3, Rally3 Evo, Rally4, 208 Rally4, GR Yaris Rally2 3 [FIA 260], [GR Yaris]); Evo IX N4 cable
  [rallycars Group N]; Clio III R3 DBW [tech-racingcars Clio R3]; Golf II GTI 16V K-Jetronic, fixed stop
  [tech-racingcars Golf]; Fiesta R2 NA without anti-lag [Group R].
- WRC boost cap 2.5 bar abs from 2011 [FIA 255A]; 4.0 bar before (guess, from forum reports of 3–4 bar
  [NASIOC WRC], [Kosunen]).

## Air path (`gas.rs`, `engine.rs`)

- Orifice flow with Ψ choked below 0.528 (generic) [NASA], [Andersson]; Ψ linear from Π 0.98 to 1 (inferred:
  a finite slope near equal pressures).
- Plate area 1 − cos θ/cos 7°, θ = 7° + 83°·lift [US 6591667]. Its flow bench gives C_D 0.70–0.77 at 0–25° from
  closed and a leak of about 5 mm² (60 mm bore); C_D 0.8 and no leak about cancel at light pedal.
- Torque from MAP at part load checked against [EPA Ford 1.6]: the 2013 1.6 EcoBoost makes 27 % of its 100 kPa torque
  at 50 kPa and none at 24–26 kPa (2500 rpm); the na sample 28 % at 47 kPa.
- C_D 0.80 one throttle (inferred: fits are about 0.8 at WOT [C_D estimation]); 0.85 per cylinder (guess); at least
  45 mm per cylinder (guess).
- Manifold 0.8·V_d one throttle, 0.3·V_d per cylinder, 1.0·V_d turbo, boost pipe 2.0·V_d (guess: ITBs have no
  plenum [HPA Alpha-N]; Rally2 caps the restrictor-to-plate volume at 20 L [FIA 261]).
- W_cyl = η_v·η_res·p·V_d·n/(120RT) and the manifold filling dp/dt = RT/V·(W_in − W_cyl) (generic) [Andersson],
  [Hendricks]. Fully implicit Euler, root-solved per sub-step (inferred: the linearly implicit step overshot a
  per-cylinder tip-in to 2.4 bar).
- η_v: turbo 0.95 flat (guess); NA the WOT table's shape (inferred: the table is mostly breathing).
- Residual gas: η_res = (r_c − (p_em/p_im)^(1/γ))/(r_c − 1), the ideal cycle's (generic, Heywood; the pressure
  factor of [arXiv 2012.02859]'s η_v), r_c 11 NA, 9.5 turbo, γ 1.3 (guess: the residual is exhaust; 1.4 for air),
  p_em of the last step. 1 at p_em = p_im; at idle 0.71 (modern turbos) to 0.87. In W_cyl and the charge; K
  recalibrates. On turbos the identity runs once more last (after the hold, the gate's base and the idle gains),
  since the state now feeds back on the charge through p_em (0.16 % off the table otherwise).
- No leak; idle air instead (inferred: the plate formula closes at 7°, and a sized leak idled every sample
  55–95 N·m fast).

## Throttle (`engine.rs`, `ecu.rs`)

- Cable: lift = pedal, pedal^1.6 per cylinder (guess: progressive linkages), no lag.
- DBW: plate aim = pedal, then the ETB (30 ms first order, 40 ms full travel) [Gorzelic]. Race ECUs map pedal to
  throttle position (MoTeC, Link, Haltech, Life Racing on M-Sport R5, Marelli) [MoTeC DBW], [Link e-throttle],
  [Haltech DBW], [Life Racing], [Marelli]; torque requests are road (ME7) and F1 [Bosch ME7], [F1 torque map]. Their
  translation tables start at 1:1 (Link, Haltech); MoTeC M1 aims at a share of airflow through its throttle area
  table, which opens the plate more than 1:1 at small openings [HPA M1 DBW]. No rally map is public. HPA 138's 40 %
  pedal → 30 % throttle (pedal^1.3, used before) is a Haltech street 350Z tuned to hold a steady speed [HPA 138]; with
  it the DBW samples gave about half BeamNG's power at 10–20 % pedal.
- The idle air adds to the aim; anti-lag's kick overrides it [MoTeC DBW]. Blips and shift cuts come through the
  interface's throttle (NGP's control unit).
- Part throttle follows WOT to an rpm then falls at roughly constant power, as BeamNG's model [BeamNG throttle].
  Most net power from IdleRPM up at pedal 10/20/30/50 %, % of the WOT's: na 9/23/43/80, turbo 8/20/47/87, old
  5/18/43/91, cable_turbo 3/11/21/75 (BeamNG 2023, its power plot: 9/25/43/69). Torque, % of MaxTorque: na
  40/53/65/86, turbo 27/47/63/91, old 26/46/63/93, cable_turbo 13/25/37/87 (BeamNG 49/65/76/91).
- BeamNG's curve is not measured: [Heusinkveld], whom the post credits, measured sims, and his proposed shape is a
  chart that "might also be completely fictional". Its lines are most likely before friction and engine braking,
  which its engine code applies apart [BeamNG friction] (inferred); its power plot's 0 % line repeats the torque
  plot's values on the kW axis.

## Torque (`calibrate.rs`, `ecu.rs`)

- T_ig = K(n)·m_charge/(4π)·η_ign·(1 − cut)·damage (generic: Willans, fuel-to-torque at fixed λ [Moretto], [Guzzella]).
- K from the identity K = 4π(T_wot + friction + pumping)/charge, so steady WOT reproduces the table exactly
  (inferred). K is calibrated on a 250 rpm grid and every knot, from a fresh engine's tip-in (inferred: a turbo's
  steady boost depends on the path near its threshold).
- η_ign = max(0, 1 − 2.8e-4·Δθ²): about 1 % at 6° from MBT [MIT 2.61], quadratic [US 6363317].
- λ 0.88 at full load [x-engineer]; η_ig 0.38 for the boost target (weak) [Chegg]; LHV 43.4 MJ/kg, AFR 14.7
  (generic).

## Friction and pumping (`friction.rs`)

- Sandoval–Heywood FMEP terms, DOHC buckets, the default bearing ratios [Sandoval], [ESA.NET]; the worked example is
  reproduced within 4 % (test).
- Oil: φ = √(μ(T)/μ(363 K)), Vogel μ = 0.00862·exp(2040/(T − 89.6)), fitted to 10W-40 [Anton Paar]; cold friction
  up to 2.5× warm [cold friction].
- Pumping (p_em − p_im) + 0.2 bar·(p_im/p_a)·(n/n_lim)² (generic form [TAMU], [Moretto]; 0.2 bar guess in the
  report's 0.1–0.3). NA p_em = p_a + 0.15 bar·(W/W_rated)² (guess).
- s_f: closed plate, fuel cut, steady at RPMLimit gives −BrakeCoefficient·MaxTorque, clamped 0.6–2.0 (inferred:
  NGP's one braking point; friction models need a scale [ecu-lab]).
- BrakeCoefficient is a share of MaxTorque, so it overstates braking on boosted engines: every installed turbo car
  clamps (raw 2.3–4.9), NA cars are 1.30–1.87 (`tests/rsf_cars.rs`).
- The rF2 SkipBarber's coast line, −21 to −53 N·m at 2000–8000 rpm [rF2 engine.ini], is near the NA samples'
  (`examples/plot.py`).
- η_m = 1, losses explicit; friction fades to 0 within 5 rad/s of standstill (inferred: no stiction at rest).
- The 1.02 sim's friction torque (`Inputs::friction`: oil starvation, overheating, damage) opposes the motion with the
  same fade, as NGP applies it (`direction * friction_torque`, rbr-physics `ngp/engine.rs`). Unsigned, a stalled
  engine dragged backwards in gear ran away to the −1e4 N·m clamp and drove the car backwards under full brake
  (found by the creep measurement; `the_1_02_friction_opposes_the_motion`).

## ECU (`ecu.rs`)

- Idle: PI on air, P on spark, 27° base retard (η_ign 0.80, a torque reserve: an MPC idle holds spark efficiency
  0.75 [arXiv 2012.02859], idle control uses at most about 30 % of the reserve [US 7418943]) with −10/+15° room
  [MoTeC idle], [HPA M1 idle], [US 7143741]. Gains from the linearised idle and the car's inertia at 0.5 Hz (air) and 3 Hz (spark) (guess: no
  published gains [Yildiz]). Spark P-only (inferred: a spark integrator fights the air loop).
- The loop runs with a closed pedal below idle + 800 rpm, integrates only below idle + 100 rpm (inferred: no
  wind-down while the overrun air decays).
- Idle air ceiling: the plate's area at 10 % position, every ECU car [HPA 138] (Haltech's DBW idle maximum starts at
  about 10 %); [Link idle] idles at 3.5 %.
- DFCO (ECU cars): pedal < 0.02 and n > idle + 800 rpm; resumes below idle + 300 rpm or on the pedal [stealth316],
  [MSExtra], [Hondata].
- Overrun air while the fuel is cut: real ECUs open an idle valve or dashpot on decel [MoTeC FK8R]. Sized so the
  manifold holds 0.2·p_a (decel vacuum 17–23 kPa [Autosport decel]), at every speed (inferred: area in proportion
  to the cylinders' flow, as a throttle-aim-by-rpm table). The s_f point uses it.
- When the fuel returns, the overrun air decays to the idle feed-forward with τ 0.5 s (guess), so no air step.
- Fixed-stop cars have no DFCO (inferred: carbs and mechanical injection keep fuelling through the idle circuit;
  the decel cut-off solenoids were for road emissions).
- Limiter, Link's convention: ignition cut 0→100 % from RPMLimit − 200 rpm to RPMLimit, fuel cut from RPMLimit +
  100 [HPA 084], [HPA 171] (Link G4+: 200 rpm control range settling 100–150 rpm under the limit, hard cut +50 to
  +200), so RPMLimit is the cap as NGP and the class rules read it (MoTeC's ramp above the limit [HPA M1 limits] let
  every car run 2–3 % over: Fiesta Rally3 805.6 rad/s on a 785.4 limit in game). Neutral WOT holds the samples 4–99
  rpm under the limit (up to 18 over on the turbo sample); tested within −200..+50. The calibration's WOT runs
  leave the limiter out (the table runs to RPMLimit); the tests compare the table with the limiter in only up to
  RPMLimit − 200.
  The events are picked by deterministic error diffusion (inferred), which carries the fractional events from step
  to step as ECUs rotate the cut cylinders; a step's torque is its events' fired share, held over steps without an
  event, so near the limit it swings with the firing rhythm. Bench (720 Hz, the HUD's rpm sampled every 12 steps):
  in neutral with the car's inertia the rpm moves 12–25 rpm a step and 30–110 rpm a frame (the bounce under the
  limit); with 1.5 kg·m² (a low gear, guess) at most 2 rpm a step and 8 a frame. A mean-value cut (1 − cut every
  step) holds the rpm still on the limit, which a real limiter does not (measured). In game (drive2 attempt 5's
  payload, Focus WRC 2006, `--trace-out`): held at the limit before the start the rpm bounces 7434–7480 (frame steps
  up to 79 rpm; NGP sits at 7515 and steps up to 146); near the limit in gear the steering force changes per step no
  more than elsewhere (p99 0.038 against 0.070 at 4000–6900 rpm; NGP 0.025 and 0.027), so FFB does not show it.
- Shift cut keeps the share TorqueCut, airflow kept [FIA 254A] (S2000 cuts), [CSP powertrain].
- No combustion below 150 rpm or when stalled [turboford] (crank speeds 150–250 rpm).
- Anti-lag: armed above 0.45·RPMLimit, active with pedal < 0.1 for at most 3 s, re-armed above 0.3 [MoTeC ALS],
  [Link ALS timeout]. Kick 20 % of the throttle area [Link e-throttle ALS] (about 20 % kick, 25 % in the closed-pedal
  cells). As an area (inferred: a 15 % lift opens 5 % of the area, and the turbo ran down). Retard 60° [Link retard] (−25 to −50°; 60
  guess). Fuel +20 %, cut 30 % [Link e-throttle ALS], [HPA ALS]. Boost aim 1.2 bar gauge (guess). 60 % of the
  unburnt fuel burns in the manifold (guess). The BOV stays shut (inferred: ALS needs the charge).

## Turbo (`turbo.rs`)

- Compressor, turbine, shaft and wastegate equations (generic) [Andersson], [Eriksson turbo].
- Normalised maps from a design point at peak power [Brunel], [US 12056425] (generic). Speed line scaled by
  ν = (ω/ω*)², the fan law (inferred). ω* 15 000 rad/s (guess).
- Continuous surge band 0.05 (guess). Efficiency ellipse in flow only, η_c 0.75 peak, 0.4 floor (guess). The Π axis
  made the steady state bistable (inferred).
- Restrictor choked flow (generic) [NASA], C_D 0.95, 20 % pressure loss (guess), solved with the compressor by root
  finding (inferred: a two-step oscillation otherwise). Sizes by group and year [rallycars restrictors], [FIA 261],
  [FIA 255A], [FIA 254].
- Turbine nozzle with Ψ, η_t 0.65 (guess) [Andersson].
- A_t is fitted so K is as flat as the turbo allows (inferred: K is the fuel's energy times the combustion's
  efficiency per kg of air, nearly constant over rpm, so the boost should carry the table's shape). It minimises the
  squared deviations of ln K at the knots and every 500 rpm from idle up, each from a fresh engine's tip-in: a scan on
  ln A_t over 5e-5–1e-2 m², then golden section.
- The exhaust manifold may reach at most 2× the intake's at steady WOT (inferred, tuner practice: without it the fit took a 7e-5 m²
  nozzle with 7–9 bar of exhaust pressure for 1.5 bar at 1000 rpm; matched turbos run about 1–2).
- Then each knot's boost target is scaled by K_knot/K_median (the charge one K needs) and limited, and A_t refitted;
  twice (inferred: the first target's η_ig leaves out the turbine's back pressure). The boost scaled is the one the
  steady WOT made where that is under the target, since the charge and so K came from it (measured: scaling the
  unreachable target took the turbo sample's 1000 rpm target to 1.90 bar for 1.06 made, the targets interpolated up
  to 2500 asked for too much charge, and K dipped to 1.32e6 at 1750 against 1.58e6 on boost elsewhere; now 1.58e6
  from 1750 up, A_t and the held points unchanged).
- The table is an upper bound everywhere and exact wherever the air path can make it. On a turbo car, at a grid
  point where the steady WOT boost misses 98 % of its target (`ON_BOOST`, this worker's choice), K is held at the median K of the on-boost knots, so the
  steady torque there falls short of the table. Where the identity gives less than the median (Group A, Rally4 at
  low rpm: tables weaker than physics), K stays. NA cars unchanged. Tested: steady WOT ≤ table + 0.1 % everywhere,
  = table within 0.5 % at every knot not held.
- Held, 250 rpm steps (with the residual gas): turbo sample 500–1250 rpm (worst 9 % short); R5, Rally2 and WRC
  2014–2020 250–1250 (8.5–16 %); Xsara WRC 2006 250–1250 (20 %); Focus WRC 2006 250–1250 (27 %; 250–1500 before the retarget fix); Alpine
  A110 RGT 1750 (12 %); Fiesta Rally4 2500 (2.8 %), 208 Rally4 2250 (1.5 %), Fiesta R2 2250 (2.0 %), Delta HF 2750
  (0.4 %). No other car is held.
- The wastegate port is 3 nozzles (inferred: 1 could not hold the target with the smaller turbine, 8 gave a limit cycle).
- ω* does not change K (10 000–20 000 rad/s moved the spread in the third decimal); it sets the spool time through
  ½Jω² (measured). Kept at 15 000.
- Shaft inertia 1e-5 kg·m² [Chalmers] (their T45: 1.16e-5), [GT1548]. Losses 0.002ω + 2.5e-6ω² + 100 W [rF2 engine.ini].
- Exhaust temperature affine in flow, 700 °C at 0 (guess), 950 °C rated, capped at 1150 °C, τ 0.3 s (guess).
  Intercooler effectiveness 0.7 (guess).
- Boost target π_wot(n), first from the table with η_ig 0.38, η_v 0.95, capped (class, else 2.5 abs) and by the
  restrictor [FIA 261], [FIA 255A], then made consistent with one K (above).
- Wastegate aim. Electronic (DBW): p_a + (π_wot(n)·p_a − p_a)·min(1, θ/θ_open(n)), θ the throttle's aim position,
  as race ECUs' boost aim tables over rpm × throttle [HPA M1 boost], [HPA Link boost] (inferred: those tables reach
  full aim before full throttle; we place that point where the plate stops limiting). θ_open(n): at calibration, the
  lift whose area passes the steady WOT flow at p_im/p_b = 0.95 at that boost (turbo sample 0.24 at 1000 rpm, 0.46
  at 3000, 0.61 at 5000, 0.74 at 7000). Linear in θ below it (inferred). The aim position rather than the plate, so
  a re-apply after anti-lag does not dump the boost while the plate opens. A linear aim to θ = 1 flattened mid
  pedal (40 % 54, 50 % 60 % of max). Spring (cable):
  π_wot(n)·p_a whatever the pedal, so a cable turbo kicks at part throttle.
- Electronic gate: a base opening by rpm (the steady WOT one) plus PID, band 1 bar, Ti 0.5 s, Td 0.08 s on the boost
  (not the error), conditional integration (inferred: boost controllers run a base duty table with closed-loop trim).
  Td: tip-ins within 5 % overshoot and settled in 1 s (tuning practice, inferred); 0.05–0.12 s all pass. Electronic
  gate τ 125 ms [Gorzelic] (an electronically commanded gate). Spring: P-only, band 0.5 bar, so it overboosts a little.
  Spring τ 10 ms (inferred: the diaphragm feels the boost through its hose, which carries it within 10 ms over 3 m of
  6 mm [Thomasson 2014]; a 60 cm³ dome through a 3 mm port lags a 7 bar/s rise about 3 ms; spring and valve ring at
  about 60 Hz). With the 125 ms of the electronic gate it spiked 15–20 kPa (2.2–2.9 psi) at 5000–6500 rpm, against
  about 1 psi for a split second on spring gates [enginebasics]; a gate that opens late spikes [Turbosmart spikes].
  Bands and Ti guess (a limit cycle at narrower settings; none at 10 ms, every 250 rpm at 30/60/100 % pedal).
- DBW turbo sample, WOT tip-in at a fixed speed, to 90 % / overshoot / settled within 2 %: 2500 0.44 s / 1.8 % /
  0.54 s, 3000 0.41 / 2.1 % / 0.79, 3500 0.40 / 2.2 % / 0.82, 4500 0.37 / 2.2 % / 0.83, 5000 0.34 / 2.4 % / 0.87,
  6500 0.27 / 3.7 % / 1.10 (was 7 % and 17 % with a 0.5 s ring before the derivative). cable_turbo (spring): 3000
  0.83 / 0.3 % / 1.07, 5000 0.27 / 3.1 % / 0.37, 6500 0.15 / 5.0 % / 0.26 (boost +0.3, +2.5, +3.4 kPa; the torque
  overshoots more than the boost as the opening gate lowers the exhaust pressure). Tested: the boost spikes at most
  0.1 bar at 3000, 5000 and 6500 rpm. In game (autopilot, Celica ST185, track-11_O, 600 s, experimental, anti-lag
  steps left out): at pedal ≥ 0.5 in gear the boost ran over the steady WOT boost at that rpm by more than 10 kPa for
  15.9 s of 171 s with 125 ms (most 34 kPa), 0.3 s with 10 ms (most 17 kPa); split 2 438.02 against 437.82 s. Tested: steady torque at any pedal the same
  within 2 % + 1 N·m from closed or from WOT, every 250 rpm from idle and every 10 % (worst 1.7 %, near zero torque).
  Tested too: the most torque over rpm (idle to RPMLimit − 200) rises with every 10 % of pedal on every sample.
- Blow-off: opens at 0.3 bar over the manifold, full at 0.4 bar, empties the pipe in about 0.1 s (guess).
- Telemetry for the sound (`Telemetry`, never read back): the blow-off valve's opening (0 in anti-lag, stays open
  while the manifold is under vacuum, as a real one does; tested on a lift at 5000 rpm) and the step's air pressure.
  `rbr-standalone` `live/picture.rs` `model_turbo` maps it into SND1's turbo words (`notes/sound.md` "Engine").
- Spool, the [Chalmers] case (GT-Power, road 1.5 L four, T45): at a fixed speed from the steady pedal for 5 bar BMEP
  to 90 % of the steady WOT torque, 0.54 s at 1000 rpm (1.0 s on a three, 0.68 s with half the inertia). Tested
  0.4–1.2 s at the first 500 rpm step with 1.5 bar steady WOT boost: turbo sample 0.45 s at 2000 (0.64 before the
  retarget fix lowered its 2000 rpm target from 1.93 to 1.70 bar), cable_turbo 0.77 s at 3000 (with the residual gas the turbo sample's 1500 rpm WOT boost fell under 1.5 bar). At 1000 rpm both make
  almost no boost, so 5 bar BMEP is already near WOT (0.05 and 0.01 s); tested ≤ 1.2 s only.
- The retarget fix changed spool only where the target fell (1750–2250 rpm on the modern turbos; nothing at 1500 or
  from 2500 up, nothing on Group A). Same turbine, same steady WOT torque; the engine now needs less boost for it at
  one K, and the boost that 90 % of it needs comes sooner. WOT tip-in from closed to 90 % / settled within 2 %, and 5
  bar BMEP to 90 %, before → after (s): Fiesta Rally2 1750 1.05/1.40/1.53 → 0.42/0.59/0.65, 2000 0.55/0.67/0.74 →
  0.37/0.48/0.50, 2250 0.59/0.70/0.48 → 0.51/0.63/0.41 (target 2.09/2.06/2.03 → 1.63/1.76/1.88 bar); Fiesta WRC
  2019 and i20 WRC 2020 1750 0.88/1.27/1.46 → 0.31/0.46/0.52, 2000 0.47/0.59/0.68 → 0.30/0.40–0.63/0.42; Polo R5
  1750 0.60/0.85/0.98 → 0.39/0.56/0.63; turbo sample 2000 0.46/0.57/0.64 → 0.33/0.43/0.45; Focus WRC 2006 1500
  0.62/2.13/1.55 → 0.68/2.38/1.79 (1500 no longer held, so it now reaches the table), 1750 0.67/0.86/1.03 →
  0.55/0.71/0.86. Celica ST185, cable_turbo unchanged. Before, 1750 was twice as slow as 1500 and 2000 on the
  Rally2 and WRCs (the inflated target); now tip-in falls smoothly with rpm. No public rally time-to-torque exists
  to say which is closer to real; the road 1.6 THP holds 20 bar BMEP from 1750 rpm [SAE 2018-01-1423], so a slow
  spot at 1750 has no support (inferred).

## In game

Numbers in `docs/experimental-physics.md` ("In game"), main `96c85981`, experimental = realistic + brush; the brush
is off on gravel, so gravel numbers compare engines only.

- track-11_O (gravel) 600 s, four cars: unchanged by the brush but the Fiesta's split 2 (436.20 → 436.10 s); tools
  `run-stage-compare.sh` and `cmp2.py` in the evidence archive.
- Fiesta Rally2 0-100 km/h, plain flat-out start (`run-standing-start.sh`, `harness/car_pace.py` from 0.5 m/s): RSF
  504 tarmac +6.7 % NGP 4.52 / exp 4.34 s (4.44 with NGP's tyre); track-64 gravel flat 4.66 / 4.62; RSF 345 gravel
  −4 % 4.50 / 4.45.
- Autopilot `--launch 7200,0.2` (hold, slip throttle, torque-crossing upshifts): 4.59 / 4.48, 4.63 / 4.62, 4.47 /
  4.45: no gain. Both tyres pull as hard at 9 m/s of slip as near the peak at launch speeds (504: 10 m/s² while the
  wheels spin 9 m/s over the car); a slip throttle cut bogged the turbo to 130 rad/s off boost (hence
  `LAUNCH_FLOOR`). Launch 3500–7200 rpm, slip 10–40 %: no faster. Torque-crossing shifts 6386–6587 rpm against
  `SHIFT_UP` 6494.
- Sourced figure: Volkswagen's claim 4.1 s for the Polo GTI R5 (Rally2 class, 1230 kg); no measured Rally2 figure
  found. Slope-corrected 504 (constant acceleration, −0.66 m/s²) ≈ 3.9 s (inferred).

## Open

- The rally turbos make almost no boost at 1000 rpm, so the [Chalmers] 1000 rpm spool case does not apply to them.
- Modern turbos idle at 20–22 kPa, left so (below). Idle MAP with the 27° reserve and the residual gas, against the usual SI idle 25–45 kPa [Gears], [MOTOR], [Tuning
  School]: samples na 33.2, old 27.1, cable_turbo 30.0, turbo 20.3 kPa; RSF modern turbos (Rally2, R5, WRC) 20–22,
  Group N and R4 22–23, Rally3/4 and R1 25–31, Group A, B, 4 and S1600/S2000/R2/R3 23–34. K dominates on the modern
  turbos: 1.6–1.7e6 J/kg at idle (η_ig ≈ 0.57 of the stoichiometric 2.93e6) against 0.9–1.4e6 elsewhere; friction
  (17 N·m, s_f clamped to 2.0) and pumping (10 N·m) are like the others'. At η_ig 0.38 they would idle near 28–30 kPa
  (estimate).
  The retarget fix leaves it (turbo sample 20.3 kPa before and after: idle sits in the held range, whose K is the
  on-boost median). A part-load K that keeps the table: K(n, x) = K_wot(n)·f(x), x = m_charge / m_charge,wot(n) at
  the same rpm, f(1) = 1 (so the identity at WOT, and the hold, are untouched), f falling to about 0.65 at idle
  load (η_ig 0.57 → 0.38, guess). The idle air, the idle gains, the closed-pedal brake and the DBW part-pedal tests are
  calibrated after K, so they would follow; aim_open and gate_base read WOT runs only. Not done (nothing reads idle
  MAP; it would lower part-pedal torque everywhere, a feel change).
- At a fixed speed below idle the closed pedal gives up to +166 N·m (turbo sample, 500 rpm; na 65 N·m) with the
  idle air at its 10 % ceiling. In gear at low speed this pushes the car.
- Fixed: the turbo sample's part-pedal bump at 1500 and dip at 1750 rpm. The cause was the boost target, not the
  hold: K went 1.68e6 (held) → 1.61e6 (1500) → 1.32e6 (1750) → 1.59e6 (2500) because the targets between the held
  1000 rpm knot and 2500 were inflated (retarget, above). Now 1.68 → 1.61 → 1.58e6, and the 20 % pedal line at
  1000–2250 rpm reads 212 216 212 217 210 185 N·m (was 212 216 212 177 195 174). No blend of K across the boundary
  was needed: the remaining step from the held median to the on-boost K (4–6 %) spreads over the 1500 point.
- Low-rpm data. No Rally2/R5/WRC dyno sheet, homologation curve or onboard MAP log below 2000 rpm is public (searched
  M-Sport, Škoda, VW, Hyundai, forums; makers give peak figures only [Škoda Rally2 engine]). The nearest measured
  data is a road base engine: PSA EP6CDTX 1.6 THP (base of the 208 and C3 Rally2 engines), full-load BMEP about 8 bar
  at 1000 rpm, 12.5 at 1250, 16–17 at 1500, 20 from 1750 (read off the figure, ±0.5 bar) [SAE 2018-01-1423 fig. 23],
  i.e. 102, 159, 210 and 254 N·m. Road 1.6 turbos reach their plateau at 1500–1750 rpm [Ford 1.6 EcoBoost],
  [Hyundai i20 N]. So at 1000 rpm a road engine with a small twin-scroll turbo makes 40 % of its peak; the turbo
  sample's table asks for 253 N·m (18 bar BMEP, 55 %) and the model gives 234 (18.4 bar at 1.06 bar boost, K at η_ig
  ≈ 0.57). The data does not support more low-rpm torque: a rally turbo with a 32 mm restrictor spools later than
  the road one, and an NA 1.6 makes about 11–13 bar. If anything the held points are too strong, through K (the same
  K as the idle MAP item). A turbine refit for more low-rpm boost was not tried.
- The spark loop saturates (gain about 2°/rpm).
- No rally pedal map or measured part-throttle map of a race engine is public; the DBW 1:1 is the race ECUs' start.

## Sources

- Volkswagen, Polo GTI R5 0-100 km/h 4.1 s, 1230 kg (maker's claim): https://www.volkswagen-newsroom.com/en/the-new-polo-gti-driving-presentation-2512/the-new-polo-gti-r5-gti-goes-rallying-2567
- SAE 2018-01-1423 (EPA/SwRI, PSA EP6CDTX 1.6 THP full load, public domain): https://www.epa.gov/sites/default/files/2018-10/documents/sae-paper-2018-01-1423.pdf
- Ford 1.6 EcoBoost, ratings: https://fordauthority.com/fmc/ford-motor-company-engines/ford-ecoboost-family/ford-1-6l-ecoboost-engine/
- Hyundai i20 N, spec sheet: https://www.hyundai.com/content/dam/hyundai/au/en/models/i20-n/2021/images/i20N_Spec-Table_Oct_2021.pdf
- Škoda Rally2 engine (Fabia RS Rally2): https://cdn.skoda-storyboard.com/2022/06/FABIA_RS_Rally2_Engine.pdf

- Andersson, LiU MSc 2017 (MVEM, orifice, turbo): https://www.diva-portal.org/smash/get/diva2:1138994/FULLTEXT01.pdf
- Hendricks & Sorenson MVEM: https://orbit.dtu.dk/en/publications/mean-value-si-engine-model-for-control-studies
- Eriksson, turbo modelling: https://www.researchgate.net/publication/244520704_Turbocharger_Modeling_for_Automotive_Control_Applications
- NASA, choked flow: https://www.grc.nasa.gov/www/k-12/airplane/mflchk.html
- US 6591667, plate area: https://patents.google.com/patent/US6591667
- C_D estimation: https://www.researchgate.net/publication/224181168_Estimation_of_automotive_engine_parameters_Part_I_Discharge_coefficient_of_throttle_body
- HPA Alpha-N (ITBs): https://www.hpacademy.com/forum/understanding-afr/show/alpha-n-20/
- Sandoval (SAE 2003-01-0725): https://saemobilus.sae.org/papers/improved-friction-model-spark-ignition-engines-2003-01-0725
- ESA.NET FrictionModel.cs (Sandoval eqs. 7.b–11): https://github.com/pangtuwi/ESA.NET/pull/223
- ecu-lab, friction scale: https://github.com/DNiev/ecu-lab/pull/138
- Anton Paar, oil viscosity: https://wiki.anton-paar.com/us-en/engine-oil/
- cold friction: https://www.sciencedirect.com/science/article/pii/S0196890414001939
- TAMU, pumping: https://oaktrust.library.tamu.edu/server/api/core/bitstreams/161c947f-2eff-48dd-8bf4-80912a8bec75/content
- Moretto 2024, Willans: https://www.dora.lib4ri.ch/empa/dload/empa:36100/PDF/Moretto-2024-A_method_to_quantify_the-(published_version).pdf
- Guzzella & Onder: https://link.springer.com/chapter/10.1007/978-3-642-10775-7_2
- MIT 2.61, spark: https://web.mit.edu/2.61/www/Lecture%20notes/Lec.%2009%20SI%20engine%20combustion%20I.pdf
- US 6363317, spark quadratic: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/6363317
- x-engineer, λ: https://x-engineer.org/air-fuel-ratio/
- Chegg, η_ig 0.38: https://www.chegg.com/homework-help/questions-and-answers/18-gross-indicated-fuel-conversion-efficiency-4-stroke-s-engine-nf-g-038-equivalence-ratio-q73413709
- rF2 engine.ini: http://tirewall.net/mh-rf2/vehicle/physics/engine_ini.html
- MoTeC idle: https://motec.zendesk.com/hc/en-gb/articles/14575739507727-Checklist-Idle-Setup
- HPA M1 idle: https://www.hpacademy.com/courses/motec-m1-software-tutorial/additional-functions-idle-speed-control/
- US 7143741, idle reserve: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7143741
- arXiv 2012.02859, idle spark-efficiency setpoint: https://arxiv.org/abs/2012.02859
- US 7418943, idle reserve use: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7418943
- Yildiz, idle plant: https://web.mit.edu/aaclab/pdfs/Yildiray%20Yildiz%20TCST.pdf
- Link idle: https://forums.linkecu.com/topic/17853-e-throttle-idle-problems/
- stealth316, DFCO: https://www.stealth316.com/2-rpmfuelcut.htm
- MSExtra, DFCO: https://www.msextra.com/forums/viewtopic.php?t=33238
- Hondata, DFCO: https://www.hondata.com/help/kmanager/advancedparameters.htm
- MoTeC FK8R, overrun throttle aim: https://www.milspecwiring.com/assets/images/11401_honda_civic_fk8r_kit.pdf
- Autosport, decel vacuum: https://forums.autosport.com/topic/89377-f1-engines-and-engine-braking/
- HPA M1 limits: https://www.hpacademy.com/courses/motec-m1-software-tutorial/setting-up-m1-for-your-engine-engine-speed-limits/
- HPA 084, Link limiter: https://www.hpacademy.com/previous-webinars/084-link-g4-rev-limit-configuration/
- HPA 171, limiter: https://www.hpacademy.com/previous-webinars/171-configuring-the-ultimate-rpm-limiter/
- Haltech limiter: https://support.haltech.com/portal/en/kb/articles/main-limiter
- CSP powertrain, cuts keep airflow: https://github.com/AssettoCN/ac-docs/blob/main/docs/en/car/physics/powertrain.md
- turboford, cranking: https://www.turboford.org/thread-cranking-rpm
- MoTeC ALS: https://motec.zendesk.com/hc/en-gb/articles/14565159804687-Checklist-Anti-Lag-Setup
- Link ALS timeout: https://forums.linkecu.com/topic/6254-anti-lag-settings/
- Link e-throttle ALS: https://forums.linkecu.com/topic/14055-e-throttle-antilag-setup/
- Link retard table: https://www.akao.co.uk/manuals/Link%20G4+%20Manual/antilagretardtable.htm
- HPA ALS: https://www.hpacademy.com/forum/motec-m1-software-tutorial/show/als-anti-lag-on-link-ecus/
- MoTeC DBW: https://motec.zendesk.com/hc/en-gb/articles/15859133831823-Hundred-series-ECU-DBW-setup-information
- Link e-throttle: https://www.hpacademy.com/courses/link-g4-plus-software-tutorial/advanced-fuctions-e-throttle/
- Haltech DBW: https://support.haltech.com/portal/en/kb/articles/drive-by-wire-elite
- Life Racing (M-Sport R5): https://autosporttech.co.nz/life-racing/
- Marelli SRG-48x: https://aviorace.com/wp-content/uploads/2020/12/SRG-48X-p118.pdf
- Bosch ME7 torque structure: https://www.rbracing-rsr.com/turbo/indian_chief/Bosch_ME7_Torque.pdf
- F1 torque map: https://www.formula1-dictionary.net/engine_torque_map.html
- HPA M1 boost aim (rpm × throttle): https://www.hpacademy.com/courses/boost-control/motec-m1-closed-loop-boost-control-tuning/
- HPA Link boost target (rpm × throttle): https://www.hpacademy.com/courses/link-g4-plus-software-tutorial/advanced-fuctions-closed-loop-boost-control/
- HPA 138, DBW webinar: https://www.hpacademy.com/previous-webinars/138-tuning-drive-by-wire-throttle-control/
- BeamNG throttle curves: https://beamng.com/game/news/blog/throttle-torque-curves/
- Heusinkveld, sims' throttle models: https://www.youtube.com/watch?v=0gW-cbSX3Uc
- BeamNG friction (forum reading of its engine code): https://www.beamng.com/threads/engine-braking-calculations-wrong.94673/
- HPA M1 DBW (throttle aim as airflow): https://www.hpacademy.com/courses/motec-m1-software-tutorial/additional-functions-dbw-throttle/
- EPA Ford 1.6 (steady map): https://www.epa.gov/vehicle-and-fuel-emissions-testing/benchmarking-advanced-low-emission-light-duty-vehicle-technology
- Gorzelic 2012, throttle and wastegate: https://public.websites.umich.edu/~annastef/papers_hcci/Gorzelic2012ccdc.pdf
- Thomasson 2014, pressure delay in hoses (Linköping diss. 1590, sec. 3.3.1): https://www.diva-portal.org/smash/get/diva2:709572/FULLTEXT01.pdf
- enginebasics, spring gate spike about 1 psi: https://enginebasics.com/Advanced%20Engine%20Tuning/Boost%20Spiking.html
- Turbosmart spikes (MAP Performance): https://www.maperformance.com/blogs/tech/39579651-boost-spikes-what-are-they-how-do-they-come-about-and-how-to-avoid-them
- Gears Magazine, idle MAP 31 kPa: https://gearsmagazine.com/magazine/pressure-sensors/
- MOTOR, idle 27 kPa: https://www.motor.com/magazine-summary/fuel-injection-diagnosis-july-2008/
- Tuning School, idle 30–50 kPa: https://thetuningschool.com/blogs/news/what-data-is-good-data
- Chalmers, tip-in and T45 inertia: https://publications.lib.chalmers.se/records/fulltext/255844/255844.pdf
- GT1548 inertia: https://doi.org/10.3390/app10217452
- Brunel, normalised maps: https://bura.brunel.ac.uk/bitstream/2438/14007/1/IMechE%20Turbo%20Conf%202012%20-%20TURBOCHARGER%20MATCHING%20METHODOLOGY.pdf
- US 12056425, map scaling: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/12056425
- passionford, WRC wastegate: https://passionford.com/forum/general-car-related-discussion/391631-wrc-rally-car-turbo-s-and-power.html
- NASIOC, WRC boost: https://forums.nasioc.com/forums/showthread.php?t=781478
- Kosunen, WRC boost: https://www.kosunenracing.com/ega.html
- rallycars, restrictors: https://rallycars.com/technical-stuff/fia-turbo-restrictors/
- rallycars, Group N: https://rallycars.com/technical-stuff/fia-group-n/
- Impreza WRC (DBW 1999): https://en.wikipedia.org/wiki/Subaru_Impreza_WRC
- Group R: https://en.wikipedia.org/wiki/Group_R
- FIA 254: https://www.fia.com/sites/default/files/254_2019_wmsc_2018.12.05.pdf
- FIA 254A (S2000): https://www.fia.com/sites/default/files/regulation/file/254A%20(2013).pdf
- FIA 255 (Group A, S1600): https://www.fia.com/sites/default/files/255_2019_wmsc_2018_12.05.pdf
- FIA 255A (WRC 2011–16): https://www.fia.com/sites/default/files/255a_2016-corrected_16.12.2015.pdf
- FIA 260 (R1–R3, Rally3–5): https://www.fia.com/sites/default/files/260_2021_wmsc_2020.12.16.pdf
- FIA 261 (Rally2): https://www.fia.com/sites/default/files/261_2023.pdf
- tech-racingcars Audi A2: http://tech-racingcars.wikidot.com/audi-quattro-a2
- tech-racingcars 911: http://tech-racingcars.wikidot.com/porsche-911-sc
- tech-racingcars Clio R3: https://tech-racingcars.wikidot.com/renault-clio-iii-r3
- tech-racingcars Golf: http://tech-racingcars.wikidot.com/vw-golf-mk-ii-gti-16v
- GR Yaris Rally2: https://en.wikipedia.org/wiki/Toyota_GR_Yaris_Rally2
