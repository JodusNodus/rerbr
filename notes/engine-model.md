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
  [FIA 255], [FIA 255A], [FIA 260], [FIA 261] and Appendix J 1985–2001 [Appendix J]; DBW from 1999 in WRC
  [Impreza WRC]; the year from `cars_data.json`, else the group's middle era (inferred: the RSF list has years for
  nearly every car).
- Appendix J 1985–2001 names no boost limit for Groups N and A ("this pressure is free" in 1986; from 1987 the
  wastegate may be made adjustable), no anti-lag and no ban on drive-by-wire (fly-by-wire is banned from 1995 only in
  cross-country, trucks and GT); Group N may fit a homologated mechanical throttle kit to a road car's motorised
  throttle from 2001 [Appendix J]. So anti-lag from 1992 on Group A and from 1985 on the Audi rest on the cars'
  history, not on a rule: Toyota says its ST205 (late 1994) "pioneered the use of anti-lag boost technology"
  [ST205 anti-lag], and Sainz's 1994 works Escort Cosworth has one now [Bonhams Escort]; the 1992 start is a guess.
- A throttle per cylinder for NA Group 4, Group B, Group A8, Super 2000, Group A7 from 1993 (inferred: race NA
  engines of those classes ran twin-choke carburettors, mechanical-injection slides or ITBs, which tuners run on
  alpha-N [HPA Alpha-N]). The 911 SC RS (Group B) has Kugelfischer mechanical injection; the road 911 SC, whose
  intake the Group 4 car's page gives, is K-Jetronic, one throttle [tech-racingcars 911] (open).
- An ECU idle loop from 1987, a fixed idle stop before (inferred: carburettors and mechanical injection).
- Facts by folder: cylinders (Audi A2 5 [tech-racingcars Audi A2]; 911 SC, SC RS 6 [tech-racingcars 911]; Fiesta
  Rally3, Rally3 Evo, Rally4, 208 Rally4, GR Yaris Rally2 3 [Fiesta Rally3], [Fiesta Rally4], [208 Rally4],
  [GR Yaris]); Evo IX N4 cable (Group N keeps the series accelerator cable [FIA 254] 6.1; the road Evo VIII and IX
  share cable MN100169 [Evo cable]); Clio III R3 DBW (inferred: the road Clio 3 RS has a motorised throttle
  [Clio 3 RS throttle]; nothing public on the R3); Golf II GTI 16V K-Jetronic, fixed stop (inferred from the
  mechanical injection) [tech-racingcars Golf]; Fiesta R2 NA [Group R], so no anti-lag.
- Rally2 boost 2.5 bar abs with the FIA pop-off valve [FIA 261] ("The maximum boost pressure is 2.5 barA"). Rally3
  and Rally4 have a restrictor (31 and 30 mm) and no boost limit [FIA 260] art. 304, Group N a 33 mm restrictor and
  none [FIA 254] (read 2026-11-10): a class without a rule has no cap, and the calibration's bound for it is 3.5 bar
  (`PI_CAP`, guess; no installed car reaches it). Every installed turbo's K over
  the fuel–air cycle's (from twice idle up), where its boost is held: Fiesta Rally4 0.98–1.00 (boost 3.27 bar),
  Fiesta Rally3 0.98–1.01 (2.67), Clio Rally3 1.01–1.06 (3.12), Impreza N14 N4 1.02–1.07 (2.83), Evo IX N4 0.98–1.02
  (2.85), Evo X R4 1.04–1.10 (3.03). Unchanged, a rule holding them: R5 and Rally2 at 2.5 bar 1.10–1.23, WRC 1.6 at
  2.5 bar 1.17–1.26, WRC 2.0 2006 at their restrictor 1.06–1.13; Group A 0.84–1.01, the Audi 1.01–1.08, Fiesta R2
  0.96–0.98, Alpine RGT 0.99–1.00. So what is left over the ceiling sits only where a real rule limits the air. WOT
  torque as before (the identity), scorecard 60/92 with no row moved. Fiesta Rally4: closed → WOT to 90 % at
  4000 rpm 1.16 s, WOT again after a 1 s lift at 4500 rpm with anti-lag 0.30 s (its 1.2 bar aim is further from the
  WOT boost), without 1.04 s. R1–R3 turbos and RGT keep 2.5 bar (their articles not read; neither installed car is
  over the ceiling).
- WRC boost cap 2.5 bar abs from 2011 [FIA 255A]; 4.0 bar before (guess: forum reports of 3–4 bar [NASIOC WRC],
  [passionford], both unreachable at the 2026-11 audit).
- Compression ratio: NA 11 (guess); a turbo's by group (`compression`). R5/Rally2 10.5 and WRC 1.6 12.5, their rules'
  maximum (FIA 261, 255A; a user decision 2026-11-10, the engines' own unpublished): Fiesta Rally2 part-load and
  spool keys move 1-4 %, K/K_max 1.13. Group B 7.0: road ur-quattro 2.1 10V 7.0
  [audiurquattro], [auto-data], road Sport quattro 8.0 and 205 T16 6.5 [ultimatespecs], Delta S4 Stradale 7.5
  [ultimatecarpage], S1 E2 7.5 [tech-racingcars S1]. Group A and N/R4 8.5, their road engines': Cosworth YB 8.0
  [FordWiki Escort], Integrale Evo2 8.0 [ultimatecarpage], Delta HF 4WD Group N 8.0 [tech-racingcars Delta N], 4G63T
  8.5 (Evo I–II), 9.0 (III), 8.8 (IV–IX) and 4B11T 9.0 [motorreviewer 4G63], [motorreviewer 4B11], EJ20 turbos 8.0
  and EJ257 8.2 [motorreviewer EJ20], [motorreviewer EJ25], 3S-GTE 8.5, 8.8, 8.5 [motorreviewer 3S-GTE],
  [8020 3S-GTE]. The rest 9.5 (guess): no WRC, R5/Rally2 or Rally3/4 race engine's is published; the rules cap it at 10.5 for Rally2 (art. 310-1)
  [FIA 261] and 12.5 for WRC 1.6 [FIA 255A], maxima, not the engines'; the road bases run
  10.0–10.1 (1.6 EcoBoost) [Wikipedia EcoBoost], 10.5 (G16E-GTS) [GR Yaris spec] and 10.5 (1.2 PureTech)
  [ultimatespecs]. It sets the residual gas, the rings' gas loading and K's ceiling (Torque).

## Air path (`gas.rs`, `engine.rs`)

- Orifice flow with Ψ choked below (2/(γ+1))^(γ/(γ−1)) = 0.528 at γ 1.4 (generic; [NASA] gives the choked flow,
  [Andersson] the Ψ); Ψ linear from Π 0.98 to 1 (inferred:
  a finite slope near equal pressures).
- Plate area 1 − cos θ/cos 7°, θ = 7° + 83°·lift [US 6591667]. Its flow bench gives C_D 0.70–0.77 at 0–25° from
  closed and a leak of about 5 mm² (60 mm bore); C_D 0.8 and no leak about cancel at light pedal. [Guzzella] App. B's
  2.8 L V6 uses the same form (α0 7.9°, 58.7 mm) with a 5.3 mm² leak.
- Torque from MAP at part load checked against [EPA Ford 1.6]: the 2013 1.6 EcoBoost makes 27 % of its 100 kPa torque
  at 50 kPa and none at 24–26 kPa (2500 rpm); the na sample 28 % at 47 kPa.
- C_D 0.80 one throttle (inferred: fits are about 0.8 at WOT [C_D estimation]); 0.85 per cylinder (guess); at least
  45 mm per cylinder (guess).
- Manifold 0.8·V_d one throttle, 0.3·V_d per cylinder, 1.0·V_d turbo, boost pipe 2.0·V_d (guess: ITBs have only
  their runners after the plate (inferred); Rally2 caps the restrictor-to-plate volume at 20 L [FIA 261]). [Guzzella]
  App. B's 2.8 L V6 has 5.8 L (2.1·V_d) and a measured τ of about 0.45 s at 1000 rpm, about 2.5× ours (Open).
- W_cyl = η_v·η_res·p·V_d·n/(120RT) and the manifold filling dp/dt = RT/V·(W_in − W_cyl) (generic) [Andersson],
  [Hendricks]. Fully implicit Euler, root-solved per sub-step (inferred: the linearly implicit step overshot a
  per-cylinder tip-in to 2.4 bar).
- η_v: turbo 0.95 flat (four-valve heads peak at 0.95–0.99 [Garrett turbo selection]; on the manifold's density at
  full load, air from fuel flow × λ × 14.1, EPA's road DI turbos: Ford 1.6 0.84–0.91, Mazda 2.5T 0.88–0.99, Ford 2.7 0.92–1.00
  at 1500–3500 rpm [EPA]; none over 1, so a value over 1 has no support); NA the WOT table's shape, peak 1
  (inferred: the table is mostly breathing), then raised where K would top the fuel–air cycle (Torque). Road NA engines
  peak at 0.87–0.88 [Heywood] fig. 6-8; the installed NA cars now peak at 1.12–1.19, the two S2000s at 1.25–1.27.
- Residual gas: η_res = (r_c − (p_em/p_im)^(1/γ))/(r_c − 1), the ideal cycle's (generic, Heywood; the pressure
  factor of [arXiv 2012.02859]'s η_v), r_c by class (Class), γ 1.3 (guess: the residual is exhaust; 1.4 for air),
  p_em of the last step. 1 at p_em = p_im; at idle 0.71 (modern turbos) to 0.87. In W_cyl and the charge; K
  recalibrates. On turbos the identity runs once more last (after the hold, the gate's base and the idle gains),
  since the state now feeds back on the charge through p_em (0.16 % off the table otherwise).
- No leak; idle air instead (inferred: the plate formula closes at 7°, and a sized leak idled every sample
  55–95 N·m fast).

## Throttle (`engine.rs`, `ecu.rs`)

- Cable: lift = pedal, pedal^1.6 per cylinder (guess: progressive linkages), no lag.
- DBW: plate aim = the pedal table (`ecu::dbw_plate`: the pedal itself to 20 %, two thirds of its slope from there to 60
  % of the plate at 80 % of the pedal, the rest over the last fifth), then the ETB, 30 ms first order and 40 ms full
  travel (guess; [Gorzelic] model the throttle as a 40 ms first-order lag, "taken at 40 msec", an assumption). Race ECUs
  set the plate from the pedal through a table: Link over pedal and rpm [Link e-throttle], Haltech [Haltech DBW];
  Marelli's SRG-48x drives a DBW motor [Marelli]; [MoTeC DBW] and [Life Racing] (M-Sport R5) were unreachable at the
  2026-11 audit. Torque requests are road (ME7) and F1 [Bosch ME7], [F1 torque map]. The table is fitted through the
  model's steady torque to the 2017-21 World Rally Cars' pedal map (Open; 2026-11-11, `engine/pedal-map`): the i20 Coupe
  WRC 2020 gives 0.08 / 0.23 / 0.41 / 0.55 / 0.71 / 0.82 / 0.90 / 0.96 / 0.99 of the range from the shut to the open
  throttle at 10 to 90 % pedal at 6000 rpm (0.34-0.40 / 0.50-0.56 / 0.65-0.72 / 0.77-0.82 / 0.86-0.91 at 30 to 70 % from
  6500 down to 5500 rpm), the real cars 0.31 / 0.49 / 0.67 / 0.81 / 0.92 there (± 0.03-0.04, the tarmac and the gravel
  rows pooled by their errors); the 1:1 plate before it (a guess: neither source shows a default) gave 0.49 / 0.71 /
  0.86 / 0.96 / 0.99. No other class is measured. WOT, the closed pedal and the Scorecard do not move (its rows solve
  for the pedal), nor the dump start (i20 3.23 / 4.03 s to 100 km/h on the tarmac and gravel pads, Fiesta Rally2 3.89 /
  4.49; the slip starts within 0.01 s). MoTeC M1 aims at a share of airflow through its throttle area table [HPA M1
  DBW], which opens the plate more than 1:1 at small openings (inferred). No rally map is public. HPA 138's 40 % pedal →
  30 % throttle (pedal^1.3, used before) is a Haltech street 350Z tuned to hold a steady speed [HPA 138]; with it the
  DBW samples gave about half BeamNG's power at 10–20 % pedal.
- The idle air adds to the aim; anti-lag's kick overrides it (inferred; [MoTeC DBW] unreachable). Blips and shift cuts
  come through the interface's throttle (NGP's control unit).
- Part throttle follows WOT to an rpm then falls at roughly constant power, as BeamNG's model [BeamNG throttle]. Most
  net power from IdleRPM up at pedal 10/20/30/50 %, % of the WOT's: na 10/23/35/61, turbo 8/21/41/73 (10/23/42/77 and
  8/21/49/88 on the 1:1 plate), old 5/19/44/91, cable_turbo 3/11/22/75 (BeamNG 2023, its power plot: 9/25/43/69).
  Torque, % of MaxTorque: na 40/53/61/75, turbo 27/49/58/79 (40/53/65/84 and 27/49/64/91 on the 1:1 plate), old
  26/47/63/93, cable_turbo 12/25/38/88 (BeamNG 49/65/76/91).
- BeamNG's curve is not measured: [Heusinkveld], whom the post credits, read the part-throttle curves of 12 sims (his
  video's chapter "How I Got Data and Disclaimer") and proposes a shape; no dyno data. Its lines are most likely before
  friction and engine braking, which its engine code applies apart [BeamNG friction] (inferred); its power plot's 0 %
  line repeats the torque plot's values on the kW axis.

## Torque (`calibrate.rs`, `ecu.rs`)

- T_ig = K(n)·m_charge/(4π)·η_ign·(1 − cut)·damage (generic: Willans, fuel-to-torque at fixed λ [Guzzella]; [Moretto]
  fits it on a diesel).
- K from the identity K = 4π(T_wot + friction + pumping)/charge, so steady WOT reproduces the table exactly
  (inferred). K is calibrated on a 250 rpm grid and every knot, from a fresh engine's tip-in (inferred: a turbo's
  steady boost depends on the path near its threshold).
- An NA engine's K is at most the fuel–air cycle's work per kg of air: 0.344 at r_c 6, 0.383 at 8, 0.400 at 9.5 and
  0.419 at 11 at φ 1.136 [Heywood] fig. 5-9 (6, 8 read off its right panel at φ 1 and 1.2), × LHV/(AFR·λ), 1.34 MJ/kg
  at 9.5 and 1.41 at 11. A richer mixture does not raise it: η·φ, the work per kg of air, is flat from φ 1.1 to 1.4
  (r_c 12: 0.459 at 1.136, 0.462 at 1.2, 0.455 at 1.4). Real engines reach about 0.8 of the cycle (pp. 194–196),
  the road turbos' maps 0.32–0.39 on the fuel at λ 0.88 [EPA], [Guzzella] figs. 2.32–2.33 about 0.34 at 4000 rpm.
  Where the identity gives more, η_v takes the rest (`breathe`; the na sample's 0.46 at its rated speed was over the
  ideal cycle). A turbo's K stays, one value over rpm from the retarget: on the installed cars 0.97–1.01 of the
  ceiling on Group A, 1.05–1.08 on the Group B Audi, 1.04–1.26 on Group N/R4 and the modern classes (Open).
- η_ign = max(0, 1 − 2.8e-4·Δθ²): about 1 % at 6° from MBT [MIT 2.61], quadratic [US 6363317]. [Guzzella] eq. 2.116
  and fig. 2.34: 1 − k_ζ·Δζ², k_ζ 2.4–3.3e-4 within ±15°; [Heywood] fig. 15-3b, 1/bsfc at constant load: 0.990,
  0.973, 0.945 and 0.917 at 5, 10, 15 and 20° (ours 0.993, 0.972, 0.937, 0.888). Untested past 20°.
- λ 0.88 (guess, used at every load): [x-engineer] gives λ 0.86 for maximum power; the road 1.6 EcoBoost runs λ
  0.95–1.00 at full load and 1.00 at part load [EPA Ford 1.6]; tuners run boosted engines at 0.80–0.85 [HPA lambda],
  [EFI Mastery lambda], and Bosch's MED17 enriches to protect the exhaust [Nefmoto LAMBTS], from about 850–950 °C
  [Schiller]. It enters the boost target and the exhaust's flow and heat, not the torque (K absorbs it). η_ig 0.38 for
  the boost target (weak) [Chegg]; LHV 43.4 MJ/kg, AFR 14.7 (generic).

## Friction and pumping (`friction.rs`)

- Sandoval–Heywood FMEP, App. B [Sandoval]: DOHC buckets (Table 3), the default bearing ratios [ESA.NET], ring tension
  and roughness ratios 1, auxiliaries eq. 14, 8.32 + 1.86e-3·N + 7.45e-7·N² kPa (refitted to SI engines; Patton's
  diesel-fitted form was here before). The worked
  example is reproduced within 4 % (test). Against its data the paper's model runs about 15 kPa over at every speed
  (fig. 8, 3.0 L V6). Its reference oil is a 10W-30 at 10.6 cSt and 90 °C; ours is a 10W-40 (about 18 cSt) with φ 1
  at 363 K, which leaves the model's level as published.
- Oil: φ = √(μ(T)/μ(363 K)), Vogel μ = 0.00862·exp(2040/(T − 89.6)) mPa·s, fitted to 10W-40 [Anton Paar]. With 20 °C oil
  the model's FMEP is 1.6× warm at 1000 rpm to 2.4× at 7000 (φ 3.6 on the hydrodynamic terms); [cold friction]: up
  to 2.5× at about 20 °C (after Will et al.), and FMEP ∝ μ^n, n 0.19–0.24 for gasoline engines (1.6–1.9× for this
  oil). In game the 1.02 sim's engine runs at 358–363 K, so this rarely acts.
- Pumping (p_em − p_im) plus the valves' flow losses, 3.0e-3 kPa·s²/m²·S_p²/(n_v²r⁴) per set of two at an NA engine's
  WOT [Sandoval] App. B, r from [Patton]'s four-valve survey (D_iv = 6.11 + 0.313B, D_ev = 4.14 + 0.286B mm); the
  intake's scaled by p_im/p_a, the exhaust's by (p_im/p_a)²·p_a/p_em (inferred from Patton's Δp ∝ ṁ²/(ρA²) with the
  density where the gas passes). About 38 kPa at 7000 rpm on a 2.0 L four; the guess before, 0.2 bar·(p_im/p_a)·
  (n/n_lim)², gave 20. NA p_em = p_a + 0.15 bar·(W/W_rated)² (guess, a free-flowing system; [Patton]'s production
  system, 40 kPa at 15 m/s, would give 60–100 kPa at the rally NA cars' rated speeds). At a closed pedal the vacuum
  dominates: [Guzzella] eq. 2.107 gives 0.9 bar for a motored SI engine at closed throttle, ours 0.82–0.85 at
  20 kPa. WOT mechanical efficiency on the NA samples 0.89 at 2000 rpm and 0.79–0.81 at rated ([Heywood] p. 49:
  typically 0.90 and 0.75).
- s_f: closed plate, fuel cut, steady at RPMLimit gives −BrakeCoefficient·MaxTorque, clamped 0.6–2.0, a turbo's at
  most 1 (below) (inferred:
  NGP's one braking point; friction models need a scale [ecu-lab]). The scale is 1 + (s_f − 1)·(n/n_lim)², so s_f at
  the limit and Sandoval's own FMEP at low speed (inferred: Sandoval at s_f 1 meets the road engines' rubbing
  friction from their fuel-cut points, 0.5–0.7 bar at 2000 rpm [EPA], and NGP's NA braking line gives −1.09 bar at
  2000 on the na sample; the share over Sandoval growing with n² is ours). With one flat scale the ECU samples braked
  1.4–1.7× the road engines below 5000 rpm. Friction from 2000 to 6000 rpm grows 2.15× and 2.66× on Sandoval's
  3.0 L (figs. 8, 6), 2.5–2.7× with WOT pumping motored ([Heywood] eq. 13.6, [Patton] figs. 18–19): the NA samples
  2.15–2.3, the turbos at s_f 1 1.96–2.06 (Sandoval's model on a 1.6–2.0 L four grows less than his 3.0 L V6; 3.0–3.1
  at the old clamp of 2). [Guzzella] eq. 2.112's friction over Sandoval's grows as
  n^2.1–2.3, the shape the n² scale takes (its level, 1.3× at 2000 rpm and 2.5× at 6000, is no road engine's here).
- BrakeCoefficient is a share of MaxTorque, so it overstates braking on boosted engines: every installed turbo car's
  fit is 2.07–4.39, the NA cars' 1.12–1.58 (`tests/rsf_cars.rs`). A turbo's s_f is at most 1, Sandoval's engine (was
  the 2.0 clamp, a guess): the restrictor bounds it, the Rally2's 214 kW at 6000 rpm through 0.183 kg/s (32 mm
  choked, C_D 0.95) at the fuel–air cycle's 1.34 MJ/kg leaving at most 3.8 bar for friction and pumping (at 2:
  3.25 + 1.85; at 1: 1.98 + 1.84). At 1 the turbo samples' closed pedal is −1.62, −2.06 and −2.34 bar at 2000, 4000
  and 5000 rpm (at 2: −1.67, −2.42, −3.00), against EPA's road engines' −1.08..−1.48, −1.74..−2.00 and −2.16..−2.50
  (two turbos among them), and 35–44 % of NGP's braking at the limit (60–76 % at 2).
- The rF2 SkipBarber's coast line, −21 to −53 N·m at 2000–8000 rpm [rF2 engine.ini], is near the NA samples'
  (`examples/plot.py`).
- η_m = 1, losses explicit; friction fades to 0 within 5 rad/s of standstill (inferred: no stiction at rest).
- The 1.02 sim's friction torque (`Inputs::friction`: oil starvation, overheating, damage) opposes the motion with the
  same fade, as NGP applies it (`direction * friction_torque`, rbr-physics `ngp/engine.rs`). Unsigned, a stalled
  engine dragged backwards in gear ran away to the −1e4 N·m clamp and drove the car backwards under full brake
  (found by the creep measurement; `the_1_02_friction_opposes_the_motion`).

## ECU (`ecu.rs`)

- Idle: PI on air, P on spark, 27° base retard (η_ign 0.80, a torque reserve: an MPC idle holds spark efficiency 0.75
  [arXiv 2012.02859], idle control uses at most about 30 % of the reserve [US 7418943], a reserve by retard
  [US 7143741]) with −10/+15° room (guess). MoTeC M1's idle timing target is 10–15° BTDC, under the main table, with
  a minimum of 0–5° and the main table as the maximum [HPA M1 idle]; road engines' warm idles run −2 to 9° BTDC
  (Mazda 2.5 −1.9 to 4.7°, Toyota 2.5 8.9° [EPA]). Gains from the linearised idle and the car's inertia at 0.5 Hz (air) and 3 Hz
  (spark) (guess: no published gains [Yildiz]). Spark P-only (inferred: a spark integrator fights the air loop).
- The loop runs with a closed pedal below idle + 800 rpm and integrates below idle + 100 rpm; above, a positive
  integral only bleeds to 0 (inferred: no wind-down while the overrun air decays, and no wind-up from a clutch's bite
  held over idle: the Fiesta Rally2 pulled away at a closed pedal crept at 1205 rpm (was 1262–1397, idle 1200).
  Tested: after 3 s at 80 % of idle, 10 s at idle + 150 rpm brings the idle air under its feed-forward.
- Over idle the loop closes the air down to a floor, the air that holds 0.1·p_a at that speed (sized as the overrun
  air), not to 0 (road engines' lowest fired loads 9.2–34 kPa [EPA]; race ECUs hold a throttle aim minimum on overrun
  [MoTeC FK8R]; 0.1 is ours, under every sample's idle need). The manifold holds 12.6–13.1 kPa at the lowest (was
  4.5–8; Scorecard). A blip's fall back within 50 rpm of idle
  in neutral is the same within 0.15 s.
- Idle air ceiling: the plate's area at 10 % position, every ECU car [HPA 138] (Haltech's DBW idle maximum starts at
  about 10 %); [Link idle] idles at 3.5 %. Anti-stall: the ceiling's reach above the feed-forward is whole down to
  idle − 300 rpm and falls linearly to none at half idle (`ecu::anti_stall`). Link's anti-stall gain adds idle valve
  opening from 150 rpm under the target, within the valve's clamp [HPA Link idle]; the fade lower is inferred (an
  engine dragged far under idle is loaded, not dipping; braked in gear a real one stalls), 300 rpm and the half are
  guess. A lower ceiling (7 % position) left the Escort Cosworth, Delta HF and Celica ST185 with their idle air at it;
  a cap on the idle loop's net torque is not how ECUs control idle and stopped clutch help's pull-away (measured).
  Tested: every ECU sample at 0.3 and 0.5 of idle gets only the feed-forward, at idle − 300 the whole ceiling; every
  RSF car idles under the ceiling.
- DFCO (ECU cars): pedal < 0.02 and n > idle + 800 rpm; resumes below idle + 300 rpm or on the pedal (thresholds
  guess; cut and resume speeds with hysteresis [stealth316], [MSExtra], Hondata's cut by MAP over rpm [Hondata]).
- Overrun air while the fuel is cut: real ECUs open an idle valve or dashpot on decel [MoTeC FK8R]. Sized so the
  manifold holds 0.2·p_a (decel vacuum 17–23 kPa [Autosport decel]; EPA's fuel-cut points: Mazda 2.5T 17–26 kPa,
  Honda 1.5T 25–43 kPa at 1000–5000 rpm [EPA]), at every speed (inferred: area in proportion
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
- Shift cut keeps the share TorqueCut, airflow kept: S2000 cuts ignition or injection only [FIA 254A], and the
  throttle stays open on such cuts [CSP powertrain].
- No combustion below 150 rpm or when stalled [turboford] (crank speeds 150–250 rpm).
- Anti-lag: armed above 0.45·RPMLimit, active with pedal < 0.1 for at most 3 s, re-armed above 0.3 (guess). Link enables
  it above an rpm and a throttle and holds it for a deactivation time after both fall under them [Link ALS manual], 5 s
  typical [HPA Link ALS]; [MoTeC ALS] was unreachable at the 2026-11 audit. Kick 20 % of the throttle area: a Link
  user's about 20 % [Link ALS forum]; [Link e-throttle ALS] (about 20 %, 25 % in the closed-pedal cells) was unreachable
  at the audit. As an area (inferred: a 15 % lift opens 5 % of the area, and the turbo ran down). Retard 60° (guess):
  Link's retard table is an offset, −25 to −50° typical [Link retard]; the same user ran about −50°. Fuel +20 %, cut 30
  % (guess): that user ran a cut of about 60 and fuel 0–2 % [Link ALS forum]; others keep up to 40–80 % of the fuel
  limit on the lowest throttle row [HPA ALS]. Boost aim 1.2 bar gauge (guess). 60 % of the cut firings' fuel burns in
  the manifold (guess), and the retard's lost work heats the exhaust as at idle (Turbo); the exhaust sits at the 1150 °C
  cap within 1 s. The BOV stays shut (inferred: ALS needs the charge). As it ends the retard comes off over 0.1 s
  (`ALS_FADE`, 2026-11-11, `engine/antilag-exit`; inferred: Link's retard table runs over the throttle and the rpm with
  values to about 30 % throttle and none from half [Link retard], so it blends out, and the 0.1 s is ours). Switched off
  at once, the manifold the kick kept at 1.8 bar gave 0.73 of the full torque at 10 % pedal and 0.60 / 0.46 / 0.40 over
  the next 0.06 s (i20 Coupe WRC 2020, 6000 rpm, a 0.4 s pedal ramp after a 1.5 s lift); with the fade -0.08 / 0.16 /
  0.28 / 0.33 / 0.37, rising with the pedal. The 2017-21 World Rally Cars read 0.03 / 0.03 / 0.11 / 0.25 of their steady
  full-throttle g with the throttle at 8 / 14 / 25 / 43 % on the way up (655 openings, Open). From 90 % pedal on the
  ramp's torque is the same (0.75-0.80, then 0.85-0.89 0.1 s later); WOT at once after a 1 s lift at 4500 rpm reaches
  half the torque 0.04-0.05 s later than before and 90 % within 0.02 s of before, 0.06 later on the i20 (Celica ST185
  0.07 s, Focus WRC 2006 0.19, Fiesta Rally2 0.10, Fiesta Rally4 0.29, i20 0.08). Scorecard unmoved.

## Turbo (`turbo.rs`)

- Compressor, turbine, shaft and wastegate equations (generic) [Andersson], [Moraal turbo].
- Normalised maps scaled to the engine (generic): maps scaled by the engine's power ratio [US 12056425], maps normalised
  by their peak efficiency [Brunel]; the design point at peak power is ours (inferred). Speed lines by the fan law, flow
  ∝ ν and pressure rise ∝ ν², ν = ω/ω* (inferred). ω* 15 000 rad/s (guess).
- Continuous surge band 0.05 (guess). Efficiency ellipse in flow only, η_c 0.75 peak, 0.4 floor (guess). The Π axis
  made the steady state bistable (inferred).
- Restrictor choked flow (generic) [NASA], C_D 0.95, 20 % pressure loss (guess), solved with the compressor by root
  finding (inferred: a two-step oscillation otherwise). Sizes by group and year: none before 1990 and none on Group B;
  Group A 40 mm 1990–91, 38 mm 1992–94, 34 mm from 1995 (WRC from 1997 the same, as Group A cars); Group N 40, 36 and
  32 mm the same years [Appendix J] (1990–91 "must be 40 mm" in every event; from 1992 "compulsory in rallies", allowed
  elsewhere), 33 mm from 2010 [Impreza N2010], [FIA 254]; WRC 1.6 33 mm, 36 mm from 2017
  [FIA 255A], [World Rally Car]; Rally2 32 mm [FIA 261]. [rallycars restrictors]' 38/36/34 mm (1992/1994/1995) mixes
  Group A's 38 and 34 with Group N's 36.
- Turbine nozzle with Ψ [Andersson], η_t 0.65 with the bearing losses apart: [Marelli 2011]'s 33 mm wastegated turbine
  averaged 0.62 over the pulse at 100 Hz with the gate closed, 0.78 in steady flow (Table 1); [Guzzella] eq. 2.52
  gives peaks of 0.65–0.75; [Heywood] fig. 6-56b 0.82 at the best blade speed ratio, 0.5 at 0.32 and 1.0.
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
  2014–2020 250–1250 (8.5–16 %); Xsara WRC 2006 250–1250 (20 %); Focus WRC 2006 250–1250 (27 %); Alpine
  A110 RGT 1750 (12 %); Fiesta Rally4 2500 (2.8 %), 208 Rally4 2250 (1.5 %), Fiesta R2 2250 (2.0 %), Delta HF 2750
  (0.4 %). No other car is held.
- The wastegate port is 3 nozzles (inferred: 1 could not hold the target with the smaller turbine, 8 gave a limit cycle).
- ω* does not change K (10 000–20 000 rad/s moved the spread in the third decimal); it sets the spool time through
  ½Jω² (measured). Kept at 15 000.
- Shaft inertia 1.7e-5 kg·m², a measured passenger-car rotor with a 52 mm compressor [Perge 2017] (a 2.0 L engine's
  2.72e-5 [Chung 2016]; [Chalmers]' T45 1.16e-5 and [GT1548] 5.5e-6 for smaller engines). With 1e-5 the samples
  spooled at 2000 rpm in 0.46–0.53 s, against [Chalmers]' 0.62–1.02 s at 2100–2400 rpm; now 0.64–0.74 s.
- The rotor by size (`turbo::rotor`, 2026-11-10): the 1.7e-5 rotor at ω* 15 000 is the reference, for a design
  flow of 0.15 kg/s at Π 2 (guess: flow ∝ D², [Garrett] 60 mm GT2860RS sold for 250–360 hp, 82 mm GT3582R for
  400–600 hp; its
  52 mm compressor at 15 000 rad/s has a 390 m/s tip, a work coefficient of 0.55 at η_c 0.75). Another turbo's by
  similarity from its design flow W* and ratio Π*: tip speed u ∝ √(Π*^((κ−1)/κ) − 1), diameter d ∝ √(W*/u), inertia
  ∝ d⁵ ([Frontiers 2016] after Watson 1981; the turbine wheel is 70 % of it), ω* ∝ u/d, so ½Jω² ∝ W*^1.5·u^0.5.
  The bearing losses are the reference's at the same share of the design speed (guess), so the steady state, K and
  the turbine's nozzle do not depend on the size. Before every car ran the reference rotor. Inertia kg·m² / design
  shaft rpm: turbo sample 1.41e-5 / 186 000, cable_turbo 2.55e-5 / 142 000, Fiesta Rally2 1.63e-5 / 171 000. Fixed
  speed, closed → WOT to 90 % at 4000 rpm, s (peak shaft speed, 1000 rpm): Audi quattro A2 1.20 (122), Delta HF
  0.83 (105), Celica ST185 1.02 (163), Focus WRC 2006 0.69 (202), Fiesta Rally2 0.66 (191), Fiesta Rally4 0.93
  (213). WOT again after a 1 s lift at 4500 rpm, anti-lag off: 0.69, 0.55, 0.69, 0.75, 0.79, 0.95; with
  the class's anti-lag Celica 0.05, Focus 0.21, Rally2 0.11, Rally4 0.10 (`curves` example).
  Scorecard 60/92 as before; spool rows 0.84 (turbo, 2000 rpm), 1.51 (cable_turbo, 3000), 0.93
  (Rally2, 2000), band 0.54–1.60. On a lift at 5000 rpm the turbo sample keeps 0.19 bar after 0.5 s (was 0.09;
  tested under 0.25). Sized on the volume behind a restrictor instead (the same mass is more volume) the cable_turbo
  row left the band at 1.71 s: not used. No era term: no data for 1980s wheels' flow per diameter or their mass.
  The whine's pitch follows the shaft (`live/picture.rs`), so it moves with ω* by car. RSF 524, plain autopilot,
  `experimental`, now: Audi quattro A2 350.11 s, Fiesta Rally2 347.73, Fiesta Rally4
  353.06 (`rally_gg.py run`).
- Losses 0.002ω + 2.5e-6ω², our reading of rF2's `TurboFriction=(0.002,2.5)`, "Torque (Nm, constant at any speed),
  power (W, ramps up linearly with speed)" [rF2 engine.ini]. Its `TurboStaticFrictionWatts=100`, which "prevents
  numerical problems at low spool speeds", is left out: a constant power is a torque without bound as the shaft
  slows, and it stopped the shaft at idle and at a closed pedal. Without it the turbo samples' shaft idles at
  3300–3500 rpm ([Guzzella] p. 50: at idle the speed "can fall below 10 000 rpm"; no measured idle speed found). At
  80 000 rpm the losses take 192 W (292 with the 100 W), between the two rigs' 95 W [Chung 2017] and about 350–500 W
  [Perge 2017] (oil 90 °C and 50–90 °C). The slow shaft at idle took the calibration's steady test longest, so it
  stops at 10 rad/s of shaft speed per 0.25 s (was 1; results move about 1e-4, the idle shaft 80 rpm).
- Exhaust temperature affine in flow for the firings that burn, 520 °C at 0 and 950 °C rated, capped at 1150 °C
  (MoTeC's anti-lag limits; [MoTeC ALS] unreachable at the audit), τ 0.3 s (guess). The 520 °C is the intercept of a
  line through EPA's four road turbos' 849 fired points (940 °C at the most flow, rms 85 K; turbine inlet against
  exhaust flow over the engine's most) [EPA]; 950 °C rated is a guess. Firings that do not burn (the fuel cut, cut
  events) leave the air at 300 °C (EPA's fuel-cut points at the turbine 199–422 °C), their fuel burning in the
  manifold at anti-lag's 60 %. Half the work the retard takes heats the exhaust: (1 − η_ign)·K·W_cyl·0.5 (inferred:
  EPA's Ford 1.6 and 2.7 retarded 27–44° at their lightest loads, 1000–1750 rpm, ran 70–142 K over their neighbours
  near MBT, 0.23–0.63 of that work; [Guzzella] eq. 2.153: k_e ≈ 0.5 of the heat the retard frees reaches the
  exhaust, the rest the cooling water, a first-law approximation). Before (700 °C at 0, the anti-lag term on the
  retard, no motored temperature) the turbo samples idle at 598–661 °C and run 301 under the fuel cut (was 1035 / 707).
  With 400 °C an older
  model (no residual gas, before the retarget) gave the steady WOT boost two branches; 520 °C passes the path test.
  The shaft's spin-down after a lift without anti-lag (4500 rpm fixed, from WOT: stopped within 0.8 s) is set by the
  compressor's work and the losses, the same before and after; no measured spin-down was found.
  Intercooler effectiveness 0.7 (guess; a road top-mount measured 0.57, 0.91 with water spray [Ma'arof 2020]).
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
  6500 0.27 / 3.7 % / 1.10 (was 7 % / 17 % before the derivative). cable_turbo (spring): 3000
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
- Spool, the [Chalmers] case (GT-Power): at a fixed speed from the steady pedal for 5 bar BMEP to 90 % of the steady
  WOT torque, at 1000 rpm Volvo's 2.0 L four (its turbo not given) takes 0.54 s, their 1.5 L three with a T45
  (1.157e-5 kg·m²) 1.00 s, and 0.68 s at 0.050 kg·cm² (Table 17); 0.87–1.60 s at 1500–1800 rpm and 0.62–1.02 s at
  2100–2400 on the same engines (Tables 15–16). Tested with the old 1e-5 rotor, 0.4–1.2 s at the first 500 rpm
  step with 1.5 bar steady WOT boost: turbo sample 0.45 s at 2000, cable_turbo 0.77 s at 3000 (with the residual
  gas the turbo sample's 1500 rpm WOT boost fell under 1.5 bar). At 1000 rpm both make
  almost no boost, so 5 bar BMEP is already near WOT (0.05 and 0.01 s); tested ≤ 1.2 s only.
- The retarget fix changed spool only where the target fell (1750–2250 rpm on the modern turbos; nothing at 1500 or
  from 2500 up, nothing on Group A). Same turbine, same steady WOT torque; the engine now needs less boost for it at
  one K, and the boost that 90 % of it needs comes sooner. WOT tip-in from closed to 90 % / settled within 2 %, and 5
  bar BMEP to 90 % (s): Fiesta Rally2 1750 0.42/0.59/0.65, 2000 0.37/0.48/0.50, 2250 0.51/0.63/0.41 (target
  1.63/1.76/1.88 bar); Fiesta WRC 2019 and i20 WRC 2020 1750 0.31/0.46/0.52, 2000 0.30/0.40–0.63/0.42; Polo R5
  1750 0.39/0.56/0.63; turbo sample 2000 0.33/0.43/0.45; Focus WRC 2006 1500 0.68/2.38/1.79 (1500 no longer held,
  so it now reaches the table), 1750 0.55/0.71/0.86. Celica ST185, cable_turbo unchanged. Tip-in now falls smoothly
  with rpm (before, 1750 was twice as slow as 1500 and 2000 on the Rally2 and WRCs: the inflated target). No public
  rally time-to-torque exists to say which is closer to real; the road 1.6 THP holds 20 bar BMEP from 1750 rpm
  [SAE 2018-01-1423], so a slow spot at 1750 has no support (inferred).

## Against measured engines

EPA's steady maps of nine road engines [EPA] (2026-11; the Scorecard below scores them, with the samples and the
Fiesta Rally2), warm, steady at a fixed speed. Road engines run λ 1, EGR and VVT and are not race engines; they bound
the physics, not the calibration. "Was" is before this round (`a5ea4790`).

- BMEP at 100 kPa MAP, 1500 / 2500 rpm: road turbos 8.7–11.0 / 7.6–9.3 bar (Ford 1.6 8.68 / 9.26, Ford 2.7 8.92 /
  7.62, Honda 1.5 10.95 / 8.07, Mazda 2.5T 9.20 / 8.88); turbo sample 16.5 / 14.9 (was 15.5 / 14.0), Fiesta Rally2
  14.9 at 2500, cable_turbo 9.8 / 12.7. At 90 kPa, 2500 rpm: road NA 7.8–9.3 (Chevy 2.5 7.79, Mazda 2.0 9.12, Mazda
  2.5 9.32); na 8.2, old 7.8. The table sets the torque per bar of boost: a Rally2's 432 N·m from 1620 cc is 33.5 bar
  BMEP, at least 13.4 bar per bar at the 2.5 bar abs cap [Škoda Rally2 engine], [FIA 261]; road turbos make about
  9–10. K as an efficiency on the fuel at λ 0.88 (K·AFR·λ/LHV) at 4000 rpm: road 0.32–0.39 (gross indicated, from
  their fuel-cut friction and turbine pressure: K 1.07–1.30 MJ/kg at λ 1; the band read 0.36–0.44 before, EPA's
  efficiency at λ 1 against a key at λ 0.88), the fuel–air cycle 0.40 at r_c 9.5 and 0.42 at 11 [Heywood]; turbo
  sample 0.48, Fiesta Rally2 0.48, na 0.42 (0.44 before the ceiling), old 0.41, cable_turbo 0.39.
- Share of the 100 kPa torque at 50 kPa, 2500 rpm: road turbos 27 % (Honda 42 %); turbo sample 43 %, Rally2 43 %,
  cable_turbo 39 %. Zero torque at 18.5–27.2 kPa on the road engines; na 21.6, old 22.0, cable_turbo 18.5, turbo and
  Rally2 16.2–16.3 (was 20.4–25.2).
- Warm idle MAP: Toyota 2.5 22.3 kPa (777 rpm), Mazda 2.5 23.4–25.6 (670–700), Mazda 2.0 28.5 (767); na 28.4, old
  24.6, cable_turbo 24.4, turbo and Rally2 16.3 (was 33.2, 27.1, 30.0, 20.3 and 20.6). Idle spark −1.9 to 8.9° BTDC
  on those three.
- Closed pedal with the fuel cut, BMEP at 2000 / 4000 / 5000 rpm: road −1.08 to −1.48 / −1.74 to −2.00 / −2.16 to
  −2.50 bar at 16–43 kPa MAP (Chevy 2.5, Honda 1.5T, Mazda 2.5, Mazda 2.5T, Toyota 2.5); na −1.66 / −2.22 / −2.60,
  turbo −1.68 / −2.37 / −2.88, cable_turbo −1.66 / −2.36 / −2.88 (was −2.17 to −2.43 / −2.84 to −3.23 / −3.21 to
  −3.67); old (fixed stop, fuelled) −0.92 / −1.83 / −2.24. EPA's points stop at 5000–5760 rpm, NGP's is at the limit.
  The manifold on the overrun cut at 2000 rpm: road 15.8–26.5 kPa, ours 20.3.
- Exhaust (turbine inlet on turbos, ports on NA): warm idle 296–430 °C at the NA ports; the turbos' lightest loads at
  1000–1300 rpm 340–518 °C near MBT and 593–657 °C on the Ford 1.6 retarded to −10 to −11.5°; under the fuel cut
  199–266 °C (Honda) and 329–422 °C (Mazda 2.5T); at 12.7–13.2 bar BMEP, 1500 rpm 585–732 °C; at the most flow
  828–954 °C. Turbo sample: idle 661 °C, fuel cut 300, pedal 0.15 at 1500 rpm (12.8 bar) 564, rated 899
  (was 1035, 708, 727, 924); cable_turbo idle 598, rated 968.
- Full-load λ: road turbos 0.91–1.01, NA 0.73–1.06 (ours 0.88).

## Scorecard

`data/engine-targets.csv` holds the published targets (columns `id,system,metric,conditions,low,high,unit,source,
precision,how`; bands inclusive, an empty bound open, both empty a context row). `harness/engine_scorecard.py` runs
the `engine_scorecard` example (`crates/rbr-engine/examples/engine_scorecard.rs`, about 5 s) and prints each row's
value, band and PASS / MISS / CTX; `--only REGEX`, `--json OUT`, `--compare A.json B.json` (as the tyre scorecard).
`how` is `SAMPLE:KEY` (the synthetic samples or an RSF car's folder, which needs RBR_RSF), `model:KEY` or
`class:GROUP:YEAR:restrictor_mm`; the script's docstring lists the keys. Rows: closed-pedal braking and its manifold
pressure, cold friction, part-load torque at a manifold pressure and the zero-torque pressure, K as an efficiency,
idle and overrun manifold pressures, exhaust temperatures, spool, bearing loss and rotor inertia, λ, restrictors by
group and year; friction's growth from 2000 to 6000 rpm, the motored WOT friction and pumping at 4000 rpm and its
growth, WOT mechanical efficiency and WOT over idle air (NA: a motored turbo builds boost), the turbo shaft at idle
(context). The Rally2 rows are the Fiesta Rally2 (`Ford_Fiesta_Rally2_ngp7`).

Rounds, each on its own bands, from 36/77 (`a5ea4790`) to 60/92 now: the friction by rpm², the idle floor, the exhaust
heat, the rotor and the restrictors, Sandoval's auxiliaries and the valves' pumping, the new rows and the η_ig band
on λ 0.88, an NA engine's K under the fuel–air cycle, a turbo's friction at Sandoval's scale, no static bearing
loss, a turbo's compression ratio by group.
By system: braking 7/17, friction 8/14, part load 6/15, torque 4/6, idle 9/11, exhaust 6/9, turbo 5/5, class 15/15.

## In game

Numbers in `docs/experimental-physics.md` ("In game"), re-run on `a334ec41` (2026-11-10; NGP's runs equal those on
`cdadc164` to the 0.01 s), experimental = realistic + NGP's tyre (the brush is not released, user 2026-11-07), so
every gap is the engine's. Tools `run-stage-compare.sh`, `cmp2.py` and `run-standing-start.sh` in
the evidence archive; 0-100 by `harness/car_pace.py` from 0.5 m/s. The same runs with the
brush (main `0ef55141`) are in `notes/tyre-model.md` ("In game").

- The model's torque goes to the drivetrain whole (2026-11-10, user decision; `rbr-hybrid` `engine_wrapper`): NGP's
  wrapper divides its formula's by 1.05, and the car files carry the makers' crank figures (file = sheet on the
  Fiesta Rally2, Rally3, Rally3 Evo, Rally4 and WRC, Clio Rally3, 208 Rally4; Polo GTI R5's file 5 % over
  Volkswagen's 272 PS / 400 N·m; Alpine A110 Rally 440 N·m against Signatech's 500; sources in the doc's "Power
  delivered"). So exp is 5 % over NGP at WOT. Masses are the class minimum + 200 kg on the tyres (static loads by
  `--tyre-probe`; FIA's crew is 160 kg; wheel masses add nothing), the wheelbase (CG over the static front share)
  within 1 cm of the sheet's on seven cars.
  Scorecard unchanged (60/92, no row moves: fixed-speed rows).
- track-11_O (gravel) 600 s, four cars: exp 1.9–4.7 % sooner to split 2 (split 2 exp
  Fiesta 434.15, Celica 435.01, Golf 444.98, Fiat 446.19 s; with the torque over 1.05 they were 434.90, 436.42,
  446.00, 447.55: the autopilot's caution sets the pace).
- track-41_M (tarmac): exp 1.8–4.4 % sooner to split 2, finishes Fiesta 309.01 / 303.76, Celica
  310.91 / 303.48, Golf 324.24 / 312.39, Fiat 323.35 / 313.42 s. No stall: the Golf II GTI's full brake on the run-out
  bottoms at 82 rad/s with the clutch open (NGP's Golf 89; was a stall with wheels locked at 16 m/s, engine
  464 → 0 rad/s in 0.8 s, fixed by the relock rule, see Open).
- Limiter: no experimental car over `RPMLimit` beyond one whole-rad/s sample, in gear or on the line (the start mode
  holds the turbo cars' countdown revs; before it the Fiesta's start burnout was 0.5–0.6 s over, +34 rpm).
- 0-100 km/h on the flat pad (`harness/pad_start.py`: `--stage pad-tarmac|pad-gravel --flat-out`, timed from
  0.5 m/s), NGP / exp, the clutch dump (the plain autopilot's start) and the best start (`--drive-slip K
  --clutch-help off`, the fastest of K 0.05-0.3; `autopilot::start`). Fiesta Rally2 tarmac dump 4.05 / 3.89, best
  4.03 / 3.85; gravel 4.62 / 4.49, best 4.45 / 4.29. Polo GTI R5 tarmac 4.11 / 3.93 both starts (Volkswagen:
  "approx. 4.1 s"; its file is 5 % over the sheet), gravel 4.79 / 4.59, best 4.60 / 4.41.
- The start driver (`--flat-out`): upshifts where the next gear's thrust at the ground speed wins (the car's torque
  knots and ratios; limit less 300 rpm at the latest). The best start passes the countdown in first with the clutch
  in at the power peak's revs, slips the clutch to hold the most-slipping wheel at K (opening under the torque
  peak, or under 0.9 of the highest revs since the go where a start limiter holds the engine below it), then holds K
  by throttle. With clutch help on `--drive-slip` is throttle only, above the torque peak.
- What the best start gains (tarmac NGP, dump / best): 4WD little (Fiesta Rally2 in first: engine 1.15 g, tyres
  about 1.3 g; i20 WRC 3.32 / 3.28, quattro A2 3.23 / 3.15); 2WD 0.3-1.0 s (M3 4.67 / 4.20, Escort 5.92 / 4.90,
  Golf 5.91 / 5.56, C2 S1600 5.64 / 5.30, 911 SC RS 4.23 / 3.83); gravel 0.1-0.2 s on 4WD, up to 0.95 s on 2WD
  (M3 6.72 / 5.76). Lada VFTS and 208 R2 gain nothing on tarmac (Lada: 0.61 g spinning against 0.67 g on the
  clutch). All cars pull under their later g in the first 0.2 s whatever the slip allowed (C2 0.48 g to 5 km/h):
  not looked into.
- Start mode (anti-lag classes): limiter at 0.6 of the limit with 60° retard on the fired events while it cuts,
  over 90 % pedal with every wheel under 0.5 m/s and the body under 1.0 m/s (Link G4+ launch control for the shape;
  the four values guesses); the cut fuel burns by the anti-lag path. A handbrake turn with the clutch in stays out
  (the body moves); spinning without moving leaves it (a wheel turns). The autopilot holds full throttle through
  the countdown, so the boost is built at the go. Tarmac dump 0-100 NGP / exp: Fiesta Rally3 5.18 / 4.99, Rally3 Evo
  4.73 / 4.60, Clio Rally3 4.43 / 4.35; the mode took 0.22, 0.47 and 0.37 s off them (measured with the torque over
  1.05: 5.45 → 5.23, 5.25 → 4.78, 4.88 → 4.51); Polo, Rally2, i20 WRC gained nothing from it. Bench standing 3 s, boost with/without (bar abs): Rally3 2.37/1.35, Polo
  2.53/1.64, Celica 2.31/1.61. The plain autopilot selects first when the revs pass `LAUNCH` or set no new maximum
  for 0.1 s (the start limiter sits under `LAUNCH`); under NGP its traces are byte-identical to before (Fiesta
  Rally2, Escort, 208 R2 on track-11_O). Telemetry untouched (no sound change).
- The same on stages (Fiesta Rally2, dump / best of K 0.15 and 0.2): RSF 504 tarmac +6.7 % NGP 4.57 / 4.56, exp
  4.37 / 4.37; track-64_E gravel flat 4.66 / 4.54, 4.52 / 4.39; RSF 345 gravel −4 % 4.54 / 4.37, 4.35 / 4.18.
- Published figures against tarmac dump / best, NGP then exp: Polo GTI R5 about 4.1 s (Volkswagen) 4.11 / 4.11,
  3.93 / 3.93; i20 Coupe WRC under 4 s (Hyundai) 3.32 / 3.28, 3.23 / 3.15; Fiesta Rally3 about 5 s to 62 mph
  (M-Sport, 1210 kg; ours 1410 kg) 5.18 / 5.18, 4.99 / 4.99; enthusiast sites: M3 Group A touring car about 4.3 s
  to 60 mph (330 bhp, 940 kg; ours 300 bhp, 1225 kg) 4.67 / 4.20, 4.46 / 4.01; 911 SC RS 5.3 s to 60 mph 4.23 /
  3.83, 4.53 / 3.72; quattro A2 about 4.5 s 3.23 / 3.15, 3.10 / 3.10. No timed figure found for
  the other 36 cars; no measured Rally2 figure.

## Open

- A wheel locked under braking with the clutch in stalled the realistic engine (the Golf II GTI after track-41_M's
  finish, full brake in fifth from 24 m/s; engine 464 → 0 rad/s in 0.8 s). The clutch help did open (launch clutch 0 →
  1 between 98.5 and 87.9 rad/s on the Golf's 89.0-99.5 band). Cause: `drive_worker::clutch_torque` takes the sign
  word from the sign of the engine's net torque and relocks (engine speed = clutch-engaged speed) whenever
  `sign(speed - locked) != word`, at any clutch torque; with the clutch open, net torque -6 N·m (overrun manifold at
  15.8 kPa, the Golf's fixed idle stop refilling it at about 37 kPa/s, zero crossing at 85 rad/s) and the engine above
  the wheels' speed, the engine rode the wheels to 0. NGP's idle model keeps that torque at or above 0, so its Golf
  idles at 89. Rule, with a model set only (`GearHost::clutch_holds_only_with_capacity`, set from
  `rbr_hybrid::ngp::engine_model_is_set`): relock only if `|(locked - old) * slip_gain_a / d - a| <= b`, else keep slipping with the word set to the real slip
  direction (`tests/clutch_relock.rs`). Golf now idles 85-89 rad/s through the stop and 106 at a standstill. Shifts
  with a closed pedal (engine above the new gear's speed) now slip at the clutch's capacity instead of snapping: the
  Escort Mk II and Fiesta R2 reach 50 km/h 0.05 and 0.04 s sooner, the Rally2, Celica, Fiat and Golf the same to the
  ms (Escort Mk II, locked braking, 28.9 m, was 26.6).
- The clutch help's band is NGP's per car (the Golf 89.0-99.5 rad/s, set for NGP's 89 rad/s idle); our idle is
  `IdleRPM` (104.7), above the band's top, so the clutch is fully closed at our idle and a car braked to a stop on
  locked wheels with the clutch in gear closes it on them once the engine recovers (not exercised: the autopilot's
  first gear holds the clutch open).
- Wheel power against the 2017-21 World Rally Cars' telemetry (2026-11-10; `notes/tyre-model.md`, "Real stage
  telemetry": `harness/rally_wot.py fit` on seven tarmac events): 221-234 kW at 6500-7500 rpm at 1390 kg; the i20 Coupe
  WRC 2020 by the same fit 225-239 under ngp and 249-258 with the whole torque (9-16 % over). From the sheet's 380 PS
  the real cars lose 17-20 % to the road, the file's driveline 12 %. Not changed under `experimental`: the whole torque
  is the user's decision (In game), the read scales with the mass assumed, and no other class has telemetry; under
  `experimental-brush` a four-wheel-drive car loses 17 % (next item). From the same data a full-throttle upshift costs
  what the real ones do (0.08-0.09 s of drive against 0.05-0.11; ngp 0.02-0.05), and the deceleration with the throttle
  lifted in gear is 30 % under the real cars' with the file's drag (the measured drag area is twice the file's; a
  prototype of the devices' drag is on `aero/wing-drag`, not merged).
- Full-throttle acceleration by speed with the aero prototype (2026-11-11, main `dbd36a4b`; `rally_wot.py table`, España
  2021 against the i20 Coupe WRC 2020 on pad-tarmac): the real cars 0.50 / 0.38 / 0.30 / 0.21 / 0.14 g at 90-100 /
  110-120 / 130-140 / 150-160 / 170-180 km/h; experimental-brush with `wings+downforce` 0.56 / 0.47 / 0.38 / 0.27 / 0.18
  (11-28 % over), with the files' aero 0.57 / 0.49 / 0.42 / 0.35 / 0.27, as experimental on NGP's tyre (up to 90 %
  over). With the real drag in both the thrust left over is 11-17 %: the wheel power's reading above. The gap is the
  road loss, 17-20 % real against the file's 12 %: a four-wheel-drive car under `experimental-brush` loses 17 % in place
  of its file's smaller `DriveTrainLoss_NGP` (2026-11-11, user decision; `Profile::drive_loss`,
  `rbr_hybrid::ngp::drive_loss`: NGP's loss, on the positive drive gain only; the installed 4WD files all carry 12, the
  2WD ones 8-10 and keep them; `experimental` keeps the file's, its flat-out trace byte-equal). With it 0.55 / 0.45 /
  0.35 / 0.25 / 0.16 g at those speeds (11-17 % over; with the real drag in both the thrust 5-14 %) and the fit's wheel
  power 242-250 kW at 6000-7500 rpm (258-267 at 12 % on this profile; the real 221-234). The start on pad-tarmac passes
  10 / 25 / 50 / 100 m at 1.44 / 2.30 / 3.34 / 4.98 s (1.44 / 2.26 / 3.27 / 4.87 at 12 %) against España's 1.39 / 2.22 /
  3.26 / 4.90 (94 starts, `rally_wot.py launch`): 25 to 100 m in the real cars' 2.68 s; material 49's 10 to 50 m stay
  2.05 s. The deceleration with the throttle lifted in gear (`rally_wot.py lift`) is 0.109 / 0.090 g in 6th at 100-120 /
  80-100 km/h and 0.078 / 0.076 in 5th / 4th at 60-80, against España's 0.145 / 0.101 / 0.085 / 0.089 and Deutschland
  2019's 0.090 in 6th at 80-100 and 0.085 / 0.094: 8-25 % under at like engine speeds (was 30 %; the tyres' rolling
  resistance, 0.0066, is part of the rest; the drivetrain's loss does not act with the throttle shut). Material 49 adds
  0.014 g, inside Finland's and Estonia's 0.085-0.115.
- The 2017-21 World Rally Cars' pedal map and their response to the pedal, from the same telemetry (2026-11-11;
  `harness/rally_wot.py pedal`: the accelerometer's g between the same gear's with the throttle shut and open, 3rd to
  5th gear above 4000 rpm, the throttle steady for 0.6 s; the channel taken as the pedal). The share of that range at 10
  to 90 % throttle: 0.14 / 0.23 / 0.26 / 0.49 / 0.51 / 0.65 / 0.79 / 0.95 / 0.90 on nine tarmac rallies (± 0.06-0.11,
  the last ± 0.01) and 0.06 / 0.13 / 0.34 / 0.49 / 0.72 / 0.84 / 0.97 / 1.00 / 0.96 on eight gravel ones (± 0.03-0.10),
  alike in each of the three gears. The i20 Coupe WRC 2020's 1:1 plate gives 0.08 / 0.23 / 0.49 / 0.71 / 0.86 / 0.96 /
  0.99 / 0.99 / 1.00 at 6000 rpm (the same ratio of its steady torques at a fixed speed; at 30 % 0.59 at 4500 rpm and
  0.44 at 7000): the real cars' share at 10-20 %, 0.12-0.35 over it at 30-60 %. NGP's blend is the pedal itself,
  0.01-0.27 under the real cars at 40-80 %. The pedal table (Throttle) is fitted to these. After a lift (the throttle
  from under 10 % to 90 % within 0.55 s, 3rd gear up, mostly lifts under 1 s above 5500 rpm; scratch) the real g is at
  that gear's and rpm's steady full-throttle level by the sample after the one that first shows 90 % (0.78-0.87 then
  1.0-1.1 of it on gravel, 293 openings; 1.2 then 0.9 on tarmac, 60), and 839 throttle closings put the accelerometer
  within 0.1 s of the pedal channels. The model on the same ramp at 6000 rpm makes 0.75-0.79 of its steady torque as the
  pedal passes 90 %, 0.84-0.89 0.1 s later and 0.95 after 0.2-0.3 s (lifts of 0.3-1.5 s; with the pedal table 0.66-0.71,
  0.77-0.83 and 0.94-0.98 after 0.3 s); with the anti-lag off 0.50-0.59, 0.60-0.71 and 0.85-0.92 after 0.3 s. The
  anti-lag holds; its last 15 % may come 0.1 s late, inside the 10 Hz data's resolution. With the lateral limit at 0.8 g
  (655 openings, 17 rallies) the real share is 0.03 / 0.03 / 0.11 / 0.25 / 0.47 / 0.93 / 1.02 from 0.5 s before the
  first sample at 90 % to 0.1 s after it, the throttle at 8 / 14 / 25 / 43 / 70 / 98 / 98 %: under the steady map on the
  way up, no burst as the pedal leaves the stop. The model's anti-lag ended at 10 % pedal with 0.73 of the full torque
  for 0.06 s; its retard now fades (ECU, Anti-lag). Lifts of 2 s and more are under 15 openings: the 3 s limit of the
  anti-lag is not tested.
- The 2017-21 World Rally Cars' idle and start line, from the same telemetry (2026-11-10, scratch; the drives begin at
  the start and end at the stop line). At rest in neutral with the throttle shut (España 2021, Deutschland 2019, Finland
  2021: 24-49k samples each) the engines turn at 2250-2750 rpm on 71-79 % of the samples (median 2470-2500) and at
  1750-2000 on 17-25 %, the Fords (1810-1940 on each of 18 rallies of 2017-21; Citroën 2440-2570, Hyundai 2320-2780,
  Toyota 2260-2700; the drivers' makes from memory, not checked); the i20 Coupe WRC 2020 idles at 1200 (`IdleRPM`; on
  the pad 1155 experimental, 1203 ngp). Braked with the wheels half or more under the car's speed, the engine stays in
  gear while the drivetrain turns at 2000 rpm or more (1-8 % of the samples with the engine 15 % over it) and sits at
  2050-2500 with the clutch slipping or out below that (47-94 %); the drivetrain is at 2000 rpm or more in 93 % of those
  samples (Finland, 1077 of 1159): the real idle keeps braked wheels turning, as NGP's does at 1200 rpm
  (`tyre-model.md`, "The 17.5 mm gravel pad"). At a full pedal from 187 km/h the i20's idle holds them at 22 km/h for
  only 0.6 s on the gravel pads under ngp (0.09 s under the brush, none on tarmac) before they lock with the clutch help
  open and the engine at 1020-1040 rpm. Not modelled: an idle of that height belongs with a stage mode, a design
  decision. At the start line the engine waits at 6140-6570 rpm with the throttle open (the medians of six rallies of
  2019-21, 80-126 starts each; quartiles 5800-7000), 0.72-0.77 of the i20's 8547 rpm limit, and dips to 4200-5000 rpm
  0.3-0.6 s after the car moves (wheels at 26-28 km/h on tarmac, 29-44 on gravel). By make and rally the medians are
  5920-6600 on the Fords of 2018-21, 5900-6490 on the Toyotas of 2019-21, 4920-7890 on the Hyundais (the median of their
  18 rallies 6660) and 5590-7740 on the Citroëns, with no split by surface. The start mode holds 4930 rpm on the i20
  (`START_RPM` 0.6, a guess; the cut holds the engine 4 % under the cap), NGP waits on the rev limiter (8547), and the
  autopilot's dump takes the wheels to 42 km/h in 0.08 s. A cap at 0.75 holds 6200 rpm and leaves the dump's 0-100 km/h
  within 0.02 s on six cars: on `engine/start-rpm`, not merged.
  - Other years, the same reading: the start line's median is 6780 rpm at Deutschland 2017, 6260 in 2018 and 6010 on
    Wales 2019's wet gravel (98-99 starts each), nine rallies at 6010-6780; the cars' top rpm at full throttle is 8290
    (p99.5 a drive, Deutschland 2019; 8000 on the 2016 cars). The 2014-16 cars (Deutschland 2016) idle at 2000-2500 rpm
    on 86 % of the samples at rest.
- The rally turbos make almost no boost at 1000 rpm, so the [Chalmers] 1000 rpm spool case does not apply to them.
- A modern turbo's K over the fuel–air cycle (1.04–1.26 of it; turbo sample and Rally2 0.47–0.48 at 4000 rpm
  against 0.40), kept so the tables hold (decided 2026-11: NGP's torque over the ceiling). At the 2.5 bar cap the
  tables ask 13–15 bar of BMEP per bar of boost; the makers claim as much: M-Sport's Fiesta R5 2019 475 N·m at
  4000 rpm and 290 bhp at 6000 [M-Sport Fiesta R5], Škoda's Fabia R5 evo 212 kW and 425 N·m [Škoda Fabia R5 evo],
  VW's Polo R WRC (33 mm) 232 kW at 6250 and 425 N·m at 5000 [VW Polo R WRC], Hyundai's i20 N Rally2 300 hp and
  350 N·m [Hyundai i20 N Rally2] (the Rally2 table 460). Sourced constants close a few % at most: λ none (above),
  η_v 0.95–0.99, an intercooler 0.57–0.91, race compression ratios unpublished (Class). Tried and dropped: η_v
  carrying the excess, as on NA (one value, by the on-boost median over K_max): the restrictor holds the air from
  4400 rpm, so η_v ran 0.95 → 1.17 → 1.25 with K still over; the retarget aiming at min(median, K_max) (boost up to
  the ceiling wherever free): the Rally2 then asked 2.05 bar at 2000 rpm, at its turbo's limit, and never reached
  90 % in 5 s from 5 bar BMEP (0.75 s before); restricted knots raised to the restrictor's full flow (the limit with
  the run's residual gas, which the 0.95 margin left at 86–88 % of the choke): K there 1.10 at 91 %, the
  restrictor's drop and a drive ratio near 2 taking the rest, K at the cap 1.20 and the turbo sample's spool
  0.52 s (under Chalmers' 0.54).
  Charge-side constants re-examined (2026-11-10, nothing changed): η_v 0.95 sits inside EPA's road DI turbos' 0.84–1.00
  on manifold density (Class, Air path); the intercooler 0.7 has no cited measurement (EPA's lab charge-air cooler holds
  the outlet near 38 °C, so no effectiveness; tuner pages give 0.6–0.9, not cited); the restrictor's C_D 0.95 has no
  source (the articles fix the bore over 3 mm of length and the outside, not the profile [FIA 255A]); the rules' compression
  caps are 10.5 (Rally2) and 12.5 (WRC 1.6), the engines' unpublished. Compression ratio 10.0–10.5 moves K/K_max of the
  Fiesta Rally2 1.16 → 1.15–1.13 (K 1.55 → 1.56e6 against a ceiling 1.36–1.38e6). All of an unsourced 10.5, intercooler
  0.85, C_D 0.98 and η_v 1.0 together: Rally2 K median 1.47e6 (1.06 of the ceiling), Rally4 1.39e6, Focus WRC 2006 1.41e6.
  So no sourced value moves K materially, and the tables' torque per bar of boost stays unexplained by the charge.
- Manifold volume: one throttle 0.8·V_d against [Guzzella]'s V6 at 2.1·V_d (measured τ about 2.5× ours), aftermarket
  race plenums for the 4G63 at 2.3–2.5·V_d [Magnus plenum], [Plazmaman plenum] and a design rule of 2–2.5× the
  cylinder volume per cylinder [IJERT plenum]; no rally engine's and no textbook figure found. Not changed.
- Modern turbos idle at 16.3–17.5 kPa (Rally2, R5, WRC 2006–2020 on RSF),
  under warm road idles of 22.3–28.5 kPa [EPA] and tuners' 27, 31 and 30–50 kPa [MOTOR], [Gears], [Tuning School];
  every RSF car 16.1–31.5 kPa, the NA ones 21.5–31.5 (`tests/rsf_cars.rs`). The turbo sample and the Fiesta Rally2
  make no torque at 16.0–16.2 kPa at 2500 rpm (road 18.5–27.2). Cause: the table's torque per bar of boost (K at
  4000 rpm 0.48 as an efficiency, road 0.32–0.39) over Sandoval's friction at low speed. EPA's road K is flat with
  load (Willans), so a part-load K under the WOT one has no support (proposed before, not done); race cams' overlap,
  which raises tuners' idles to 30–50 kPa, is not modelled. Not changed.
- Closed pedal below idle at a fixed speed, N·m, after the anti-stall fade: turbo sample 51 at 500
  rpm, 93 at 750, 110 at 1000; na 27 at 500; Fiesta Rally2 51, 91, 107; Fiesta R2 22 at
  500. Between idle − 300 and idle the whole ceiling stays.
- Creep, measured with a test-only autopilot mode (not kept; Telemetry with the torque and idle air): 20 s in
  neutral, first gear, the clutch let out holding 80 % of its speed, 20 s at a closed pedal, then the brake ramped to
  full over 16 s; the track-85_M circle (-260,120,40; flat), clutch help off, experimental with the brush tyre (now
  `experimental-brush`). The creep on the flat is the gearing's: Fiesta Rally2 10.8 km/h at 1193 rpm (NGP 11.0 at
  1215), 10.9 at 1205 from rest
  (was 11.4 at 1262), 11.0 with clutch help on (was 12.6 at 1397); Fiesta R2 9.5 at 995, from rest 9.6 (was 11.3 at
  1178); Golf II GTI 16V (fixed stop) 5.5 at 655, not from rest without throttle. Stall under the brake: Rally2 at
  brake 0.20 with at most 90 N·m (was 0.23 and 152; NGP 0.45), R2 0.18 and 46 (was 54), Golf 0.09 and 23; with no
  engine a car stops at about 0.19 from that speed (inferred from the ramp). Real creep is the idle speed in first
  (inferred: the load on the flat is about 10 N·m); no rally figure is published.
- The turbo sample's part-pedal bump at 1500 and dip at 1750 rpm (fixed): the boost target, not the
  hold: the targets between the held 1000 rpm knot and 2500 were inflated (retarget, above). K now 1.68 → 1.61 →
  1.58e6, and the 20 % pedal line at 1000–2250 rpm reads 212 216 212 217 210 185 N·m. No blend of K across the
  boundary was needed: the remaining step from the held median to the on-boost K (4–6 %) spreads over the 1500 point.
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
- The spark loop saturates (gain 0.49–0.65°/rpm on the samples, `spark_kp` over IdleRPM).
- No rally pedal map or measured part-throttle map of a race engine is public; the DBW 1:1 is a guess. A
  translation table that makes the steady torque even with the pedal at each speed was built and not adopted
  (2026-11-10, branch `engine/pedal-turbo`: Fiesta Rally2 at 2000 rpm, 30 % pedal, 86 % → 31 % of WOT torque; it
  needs the plate at about a sixth of the pedal there). The published position tables are mild: 40 % pedal → 30 %
  throttle [HPA 138], a "substantial reduction" on an LS3 [HPA M1 area forum]; torque-proportional pedals are F1's
  and road cars' torque structures ([F1 torque map]: constant torque, constant power or both by region), and NGP's
  own lines are that shape. Realism does not pick between them without a rally table.
- Exhaust (Scorecard): the turbo samples idle at 661 °C against road idles of 296–430 °C at the ports and the Ford
  1.6's 593–657 °C retarded at its lightest loads, and run 564 °C at pedal 0.15, 1500 rpm against 585–732 °C at the
  same BMEP on four road turbos. One line over the flow share cannot hold both: a rally engine's rated flow is about
  2.5 bar's, so a given load is a smaller share of it than on a road engine (inferred).
- Closed-pedal braking at 2000–4000 rpm still runs up to 0.2 bar over the road engines' with the fuel cut (Scorecard):
  the pumping at the 0.2·p_a overrun air is 0.8 bar, and Sandoval's FMEP for a 1.6 L four is over the road engines'
  0.5–0.7 bar at 2000 rpm. An NA car's limit point is NGP's, which no measured race engine checks.

## Sources

- Volkswagen, Polo GTI R5 0-100 km/h 4.1 s, 1230 kg (maker's claim): https://www.volkswagen-newsroom.com/en/the-new-polo-gti-driving-presentation-2512/the-new-polo-gti-r5-gti-goes-rallying-2567
- SAE 2018-01-1423 (EPA/SwRI, PSA EP6CDTX 1.6 THP full load, public domain): https://www.epa.gov/sites/default/files/2018-10/documents/sae-paper-2018-01-1423.pdf
- Ford 1.6 EcoBoost, ratings: https://fordauthority.com/fmc/ford-motor-company-engines/ford-ecoboost-family/ford-1-6l-ecoboost-engine/
- Hyundai i20 N, spec sheet: https://www.hyundai.com/content/dam/hyundai/au/en/models/i20-n/2021/images/i20N_Spec-Table_Oct_2021.pdf
- Škoda Rally2 engine (Fabia RS Rally2): https://cdn.skoda-storyboard.com/2022/06/FABIA_RS_Rally2_Engine.pdf
- M-Sport Fiesta R5 2019, "FORD FIESTA R5 2019 TECHNICAL SPECIFICATION" (M-Sport PDF; 290 BHP @ 6000 RPM, 475 Nm @
  4000 RPM, 32 mm)
- Škoda Fabia R5 evo, technical specification (212 kW, 425 Nm): https://cdn.skoda-storyboard.com/2019/12/MOTORSPORT_Technical_specification.pdf
- VW Polo R WRC, technical specifications (2012; 232 kW at 6250, 425 Nm at 5000, 33 mm): https://web.archive.org/web/20121212190829/http://volkswagen-motorsport.com/index.php?id=339&L=1
- Hyundai i20 N Rally2, technical specifications (300 HP, 350 Nm, 32 mm): https://motorsport.hyundai.com/customer-programmes/rally2
- GR Yaris spec, Toyota UK 2024 (G16E-GTS 10.5:1): https://media.toyota.co.uk/wp-content/uploads/sites/5/pdf/240621-GR-Yaris-Tech-Specs.pdf
- Wikipedia EcoBoost (1.6 L 10.0–10.1:1): https://en.wikipedia.org/wiki/Ford_EcoBoost_engine
- tech-racingcars S1 (S1 E2 7.5:1): http://tech-racingcars.wikidot.com/audi-sport-quattro-s1
- tech-racingcars Delta N (Delta HF 4WD Group N 8.0:1): http://tech-racingcars.wikidot.com/lancia-delta-hf-4wd-gr-n
- audiurquattro (2144 cc 7.0) and auto-data (Typ 85 2.1 Turbo 7:1): https://www.audiurquattro.fr, https://www.auto-data.net
- ultimatespecs (Sport quattro 8.0, 205 Turbo 16 6.5, 208 1.2 PureTech 130 10.5): https://www.ultimatespecs.com/car-specs/Audi/222/Audi-Quattro-Sport.html
- ultimatecarpage (Delta S4 Stradale 7.5:1, spec 2980; Delta HF Integrale Evo2 8.0:1, spec 7322): https://www.ultimatecarpage.com/spec/2980/Lancia-Delta-S4-Stradale.html
- FordWiki Escort (YBT 8:1): https://www.fordwiki.co.uk/index.php/Buyers_Guide_-_Escort_RS_Cosworth_(Escort_Mk5)
- motorreviewer 4G63, 4B11, EJ20, EJ25, 3S-GTE (engine_id 123, 124, 56, 53, 152): https://www.motorreviewer.com
- 8020 3S-GTE (gen 2 8.8:1): https://www.8020automotive.com
- Garrett turbo selection (η_v 0.95–0.99 on four-valve heads): https://www.garrettmotion.com/news/newsroom/article/how-to-select-a-turbo-part-2-understanding-calculations-to-turbo-any-engine/
- Ma'arof 2020, intercooler spray (IOP Conf. Ser.: Mater. Sci. Eng.; 56.52 → 90.72 %), abstract via OpenAlex
- HPA lambda (boosted maximum power λ 0.82–0.85): https://www.hpacademy.com/forum/efi-tuning/show/tuning-richer-than-0-90-lambda/
- EFI Mastery lambda (0.80–0.85 at high boost): https://efimastery.com/tech-articles/lambda-target-selection/
- Nefmoto LAMBTS (Bosch MED17 component protection): https://nefariousmotorsports.com/wiki/LAMBTS_2.120_(Lambda_for_Component_Protection)
- Schiller (ME17/MED17 protection from 850–950 °C): https://schiller-tuning.com/ecu-tuning-course/bosch-advanced-winols/lambda-component-protection
- Magnus plenum (4G63, 5.0 L), Plazmaman plenum (4.6 L): https://www.magnusmotorsports.com, https://www.plazmaman.com
- IJERT plenum (design rule): https://www.ijert.org/research/IJERTV9IS110031.pdf

- Andersson, LiU MSc 2017 (MVEM, orifice, turbo): https://www.diva-portal.org/smash/get/diva2:1138994/FULLTEXT01.pdf
- Hendricks & Sorenson MVEM: https://orbit.dtu.dk/en/publications/mean-value-si-engine-model-for-control-studies
- Moraal & Kolmanovsky, turbo modelling (SAE 1999-01-0908): https://www.researchgate.net/publication/244520704_Turbocharger_Modeling_for_Automotive_Control_Applications
- NASA, choked flow: https://www.grc.nasa.gov/www/k-12/airplane/mflchk.html
- US 6591667, plate area: https://patents.google.com/patent/US6591667
- C_D estimation: https://www.researchgate.net/publication/224181168_Estimation_of_automotive_engine_parameters_Part_I_Discharge_coefficient_of_throttle_body
- HPA Alpha-N (ITBs): https://www.hpacademy.com/forum/understanding-afr/show/alpha-n-20/
- Sandoval & Heywood, An improved friction model for spark-ignition engines (SAE 2003-01-0725; App. B, eq. 14, figs. 6,
  8, 9, 11): https://saemobilus.sae.org/papers/improved-friction-model-spark-ignition-engines-2003-01-0725
- Patton, Nitschke & Heywood, Development and evaluation of a friction model for spark-ignition engines (SAE 890836;
  p. 10 pumping and valve survey, figs. 15–19): https://doi.org/10.4271/890836
- Heywood, Internal Combustion Engine Fundamentals (McGraw-Hill 1988; p. 49, p. 287, figs. 5-9, 6-8, 6-56b, 15-3b,
  eq. 13.6).
- Marelli & Capobianco 2011, steady and pulsating flow efficiency of a waste-gated turbine (Energy 36, 459–465,
  Table 1): https://doi.org/10.1016/j.energy.2010.10.019
- ESA.NET FrictionModel.cs (Sandoval eqs. 7.b–11): https://github.com/pangtuwi/ESA.NET/pull/223
- ecu-lab, friction scale: https://github.com/DNiev/ecu-lab/pull/138
- Anton Paar, oil viscosity: https://wiki.anton-paar.com/us-en/engine-oil/
- cold friction: Roberts, Brooks & Shipway 2014, Internal combustion engine cold-start efficiency: a review, Energy
  Conversion and Management 82, doi:10.1016/j.enconman.2014.03.002 (open access):
  https://www.sciencedirect.com/science/article/pii/S0196890414001939
- Moretto 2024, Willans: https://www.dora.lib4ri.ch/empa/dload/empa:36100/PDF/Moretto-2024-A_method_to_quantify_the-(published_version).pdf
- Guzzella & Onder, Introduction to Modeling and Control of IC Engine Systems (Springer 2010, 2nd ed.; eqs. 2.52,
  2.107, 2.112, 2.116, 2.153, figs. 2.32–2.34, App. B): https://link.springer.com/chapter/10.1007/978-3-642-10775-7_2
- MIT 2.61, spark: https://web.mit.edu/2.61/www/Lecture%20notes/Lec.%2009%20SI%20engine%20combustion%20I.pdf
- US 6363317, spark quadratic: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/6363317
- x-engineer, λ: https://x-engineer.org/air-fuel-ratio/
- Chegg, η_ig 0.38: https://www.chegg.com/homework-help/questions-and-answers/18-gross-indicated-fuel-conversion-efficiency-4-stroke-s-engine-nf-g-038-equivalence-ratio-q73413709
- rF2 engine.ini: http://tirewall.net/mh-rf2/vehicle/physics/engine_ini.html
- HPA M1 idle: https://www.hpacademy.com/courses/motec-m1-software-tutorial/additional-functions-idle-speed-control/
- US 7143741, idle reserve: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7143741
- arXiv 2012.02859, idle spark-efficiency setpoint: https://arxiv.org/abs/2012.02859
- US 7418943, idle reserve use: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7418943
- Yildiz, idle plant: https://web.mit.edu/aaclab/pdfs/Yildiray%20Yildiz%20TCST.pdf
- Link idle: https://forums.linkecu.com/topic/17853-e-throttle-idle-problems/
- HPA Link idle, anti-stall gain: https://www.hpacademy.com/courses/link-g4-plus-software-tutorial/advanced-fuctions-idle-speed-control/
- stealth316, DFCO: https://www.stealth316.com/2-rpmfuelcut.htm
- MSExtra, DFCO: https://www.msextra.com/forums/viewtopic.php?t=33238
- Hondata, DFCO: https://www.hondata.com/help/kmanager/advancedparameters.htm
- MoTeC FK8R, overrun throttle aim: https://www.milspecwiring.com/assets/images/11401_honda_civic_fk8r_kit.pdf
- Autosport, decel vacuum: https://forums.autosport.com/topic/89377-f1-engines-and-engine-braking/
- HPA M1 limits: https://www.hpacademy.com/courses/motec-m1-software-tutorial/setting-up-m1-for-your-engine-engine-speed-limits/
- HPA 084, Link limiter: https://www.hpacademy.com/previous-webinars/084-link-g4-rev-limit-configuration/
- HPA 171, limiter: https://www.hpacademy.com/previous-webinars/171-configuring-the-ultimate-rpm-limiter/
- CSP powertrain, cuts keep airflow: https://github.com/AssettoCN/ac-docs/blob/main/docs/en/car/physics/powertrain.md
- turboford, cranking: https://www.turboford.org/thread-cranking-rpm
- MoTeC ALS: https://motec.zendesk.com/hc/en-gb/articles/14565159804687-Checklist-Anti-Lag-Setup
- Link ALS forum (a Link user's settings, 2016): https://forums.linkecu.com/topic/6254-anti-lag-settings/
- Link ALS manual (G4+ help, anti-lag settings): https://www.akao.co.uk/manuals/Link%20G4+%20Manual/antilag_settings.htm
- Link launch control (G4+ help: launch RPM limit for stationary starts, ign retard and fuel trim while active,
  trim dropped when a non-driven wheel turns; the RPM left to testing): https://www.akao.co.uk/manuals/Link%20G4+%20Manual/launchcontrol.htm
- HPA Link ALS (G4+ tutorial, deactivation time): https://www.hpacademy.com/courses/link-g4-plus-software-tutorial/advanced-fuctions-anti-lag/
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
- EPA, engine test data packages on the same page (steady maps, idle and fuel-cut points): 2013 Chevrolet 2.5
  LCV, 2013 Ford 1.6 EcoBoost, 2015 Ford 2.7 EcoBoost, 2016 Honda 1.5 L15B7, 2014 Mazda 2.0 and 2018 2.5
  Skyactiv-G, 2016 Mazda 2.5 Turbo, 2018 Toyota 2.5 A25A-FKS
- Gorzelic 2012, throttle and wastegate: https://public.websites.umich.edu/~annastef/papers_hcci/Gorzelic2012ccdc.pdf
- Thomasson 2014, pressure delay in hoses (Linköping diss. 1590, sec. 3.3.1): https://www.diva-portal.org/smash/get/diva2:709572/FULLTEXT01.pdf
- enginebasics, spring gate spike about 1 psi: https://enginebasics.com/Advanced%20Engine%20Tuning/Boost%20Spiking.html
- Turbosmart spikes (MAP Performance): https://www.maperformance.com/blogs/tech/39579651-boost-spikes-what-are-they-how-do-they-come-about-and-how-to-avoid-them
- Gears Magazine, idle MAP 31 kPa: https://gearsmagazine.com/magazine/pressure-sensors/
- MOTOR, idle 27 kPa: https://www.motor.com/magazine-summary/fuel-injection-diagnosis-july-2008/
- Tuning School, idle 30–50 kPa: https://thetuningschool.com/blogs/news/what-data-is-good-data
- Chalmers, tip-in and T45 inertia: https://publications.lib.chalmers.se/records/fulltext/255844/255844.pdf
- GT1548 inertia: https://doi.org/10.3390/app10217452
- Chung 2017, turbo bearing friction 30 000–90 000 rpm (KAIS 18(1) 585–591, Table 6): https://doi.org/10.5762/kais.2017.18.1.585
- Perge 2017, turbo bearing friction and rotor inertia (Lubricants 5(3) 29, Fig. 9b, Table 1): https://doi.org/10.3390/lubricants5030029
- Frontiers 2016, turbo lag review (rotor inertia ∝ D⁵ after Watson 1981, turbine wheel ≥ 70 % of it): https://www.frontiersin.org/journals/mechanical-engineering/articles/10.3389/fmech.2016.00004/full
- Garrett, wheel sizes (GT2860RS 47/60 mm compressor, GT3582R 61/82 mm): https://www.garrettmotion.com/racing-and-performance/performance-catalog/turbo/gt2860rs/ , https://www.garrettmotion.com/racing-and-performance/performance-catalog/turbo/gt3582r/ ; power ranges from a reseller's listing (search result titles, pages not read): https://www.extremepsi.com/Garrett-GT3582R-Turbocharger-400-600-HP.html
- HPA M1 area forum, throttle area against pedal translation: https://www.hpacademy.com/forum/general-tuning-discussion/show/m1tune-throttle-area-table
- Chung 2016, turbo rotor inertia of a 2.0 L SI engine's turbo (Trans. KSAE 24(1) 33–38, Table 4): https://doi.org/10.7467/KSAE.2016.24.1.033
- Brunel, normalised maps: https://bura.brunel.ac.uk/bitstream/2438/14007/1/IMechE%20Turbo%20Conf%202012%20-%20TURBOCHARGER%20MATCHING%20METHODOLOGY.pdf
- US 12056425, map scaling: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/12056425
- passionford, WRC wastegate: https://passionford.com/forum/general-car-related-discussion/391631-wrc-rally-car-turbo-s-and-power.html
- NASIOC, WRC boost: https://forums.nasioc.com/forums/showthread.php?t=781478
- rallycars, restrictors: https://rallycars.com/technical-stuff/fia-turbo-restrictors/
- Appendix J 1985–2001 (FIA Historic Database, English, e.g. 1992 Art. 254 6.1 and 255 5.1.8.3):
  https://historicdb.fia.com/regulations/period-appendix-j (1992:
  https://historicdb.fia.com/sites/default/files/regulations/1440578063/appendix_j_1992.pdf; 1995:
  https://historicdb.fia.com/sites/default/files/regulations/1440582215/appendix_j_1995_low.pdf)
- ST205 anti-lag (Toyota UK): https://mag.toyota.co.uk/tales-from-the-trophy-cabinet-toyotas-top-14-rally-cars/
- Bonhams Escort (ex-Sainz 1994 works Escort Cosworth, "An ALS (Anti Lag System) is fitted"): https://cars.bonhams.com/auction/21735/lot/235/ex-carlos-sainz1994-ford-escort-cosworth-works-rally-car-chassis-no-wfobxxgkabrl-92667-engine-no-rl-92667/
- Impreza N2010, Group N 32 → 33 mm (RallySport Magazine, 2010-01-19): https://rallysportmag.com/prodrive-launch-subaru-impreza-n2010/
- World Rally Car, 33 → 36 mm in 2017: https://en.wikipedia.org/wiki/World_Rally_Car
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
- tech-racingcars Golf: http://tech-racingcars.wikidot.com/vw-golf-mk-ii-gti-16v
- GR Yaris Rally2: https://en.wikipedia.org/wiki/Toyota_GR_Yaris_Rally2
- Fiesta Rally3 (1.5 EcoBoost, inline 3): https://en.wikipedia.org/wiki/Ford_Fiesta_Rally3
- Fiesta Rally4 (1.0 EcoBoost, 3 cylinders): https://en.wikipedia.org/wiki/Ford_Fiesta_Rally4
- 208 Rally4 (PSA EB, 3 cylinders): https://en.wikipedia.org/wiki/Peugeot_208_Rally4
- Evo cable (Mitsubishi MN100169, Evo VIII and IX): https://www.extremepsi.com/Mitsubishi-OEM-Accelerator-Throttle-Cable-Mitsubishi-Lancer-EVO-VIII-and-IX.html
- Clio 3 RS throttle ("boîtier papillon motorisé", Renault 82 00 110 998): https://saboulautosport.com/pieces-d-occasion-rs/1044-papillon-electrique-clio-3-rs-phase-1-phase-2-clio-2-rs-phase-2-phase-3-0000000003377.html
