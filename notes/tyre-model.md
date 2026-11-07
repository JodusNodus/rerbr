# Tyre model research (ours, experimental)

Whether a more current tyre model can improve on NGP's, tarmac first. The candidate, brush-v1, runs in the game
behind a hidden profile, `--physics experimental-brush` (the engine model with the brush; `crates/rbr-tyre`,
[In game](#in-game)). It is in development and not released: `--physics experimental` and the launcher run NGP's
tyre (user 2026-11-07). NGP's tyre stays the bit-exact default. Tools: `rbr-standalone --tyre-probe FILE`
(read-only frame dumps on `--autopilot` runs) and `crates/rbr-tyre-lab` (`sweep`, `materials`; README). Probe dumps,
images and CSVs stay in `/tmp/tyre-lab`, never in git.

## NGP's tyre as it runs (7.4.778)

Measured on the Ford Fiesta Rally2 (`--rsf-car Ford_Fiesta_Rally2_ngp7`), map 85 tarmac, `--circle -260,120,40`,
step 8000; the sweep calls NGP's own `patch_forces`/`friction_limits` with the probed tables.

- Grip chain: NGP's `Physics/physics.lsp` gives every surface myStatic 0.3 / myKinetic 0.225, but NGP's stage init
  (`ngp::layout::stage::stage_init`, table `stage[0]` for revisions 778/779) overwrites the materials: tarmac fine
  clean dry (37) 1.30/1.04, soil 0. The car init (`ngp/layout/car_init.rs` `tyre()`) writes the kind's tyre preset
  (`Tables::tyres`, kind 0 tarmac dry) and `Performance_NGP` (the old `ReferenceFriction`) 1; load axis and
  stiffness tables scale with the car's mass ratio. Scratch static 1.29, kinetic 1.03 (temperature grip ~0.997);
  lateral peak `CrnMyFct(Fz) · 1.29` = 1.26 at 4 kN.
- Surface values (`materials` example): tarmac fine clean dry/damp/wet (37/38/39) μs 1.30/1.2025/1.105, μk
  1.04/0.944/0.851, WaterFactor 0/0.25/1; sprinkled and covered 2.5 % and 5 % below clean; gravel fine shallow
  (17) 0.90/0.756 over soil 0.855/0.664, 9 mm, SinkFactor 2e-7 (the soil solve runs:
  [Loose surfaces](#loose-surfaces-ngp-against-published-data)). Wet on the dry tyre: 1.105 × wet slope 0.925 = 0.79
  of dry, at any speed.
- The tyre type's friction tables are the same for all 7 kinds; tarmac against gravel comes from the surface values,
  the kind's stiffness and peak slips, and soil (sweep peak μ_y at 4 kN 0.846 / 1.265 = 0.67; skidpad 0.62).
- Sweep, dry tarmac, 15 m/s: peak μ_y 1.356/1.304/1.265/1.224/1.181 at 2/3/4/5/6 kN (peak Fy ∝ Fz^0.878), peak slip
  angle 5.75/8.5/9.5/10/10.5°; Cα 55.4/63.5/70.4/76.6/82.2 kN/rad (∝ Fz^0.36); peak μ_x 1.31-1.29 (Fz^0.98), peak
  slip ratio 0.12-0.16; μ at 30° over peak 0.865; trail 0.018 m at small slip, 0.004 m at 10°; carcass lateral
  stiffness ~405-419 kN/m, so Cα/k_y = 0.14-0.20 m.
- Temperature: `RubMyFct` 0.838 at 20 °C, 1.0 near 65-85 °C, 0.85 at 130 °C, on 0.6 sector surface + 0.4 tread
  temperature; tread 337 K on the skidpad. Stiffness does not depend on temperature.
- Wear lowers grip (`WearFtr` 0.95-0.975 times the sector's wear); stiffness does not depend on tread depth.
- Inclination (`ngp::inclination::grip_scale`): 1.0 at 0°, 0.95 at +5°, 0.8875 at +10°, 1.05 from -5° to -10°.
- Dynamics, through NGP's own step (`ngp_transient`, `--tyre-image` capture, front right at 4 kN; the quasi-static
  ramp matches the sweep within 0.0013 μ): a lateral slip step ramps the deflection linearly until the friction
  curve's force clamps it (63 % after 0.10 m at 2° falling to 0.08 m at 30 m/s, 0.06-0.04 m at 6°; 90/63 ratio 1.4,
  not an exponential's 2.3), so 2° steer at 20 m/s has no phase lag up to 2 Hz. Longitudinal steps settle in 0.35-0.44
  m at 10 m/s and 0.03 m at 20 m/s: the torsional stiffness runs at 0.1 at rest to 1 from 20 m/s (`SPEED_KNEE`).
- Low speed: the sticking model (below 1 m/s, blended to 2 m/s) alternates every step, ±10-40 N at 360 Hz; at
  standstill a push holds -4 kN with ±14 N.
- Heat: 6° at 20 m/s for 20 s takes the tread from 293 to 300.7 K (τ 19 s) and grip from 1.017 to 1.059; from the
  skidpad's 337 K it barely moves (τ 12 s). The tread temperature follows a slow level, not the slip power.
- Mass: 40 % more mass costs ~4 % lateral μ (1.4^-0.122) and ~0 longitudinally, inside the skidpad spread, which is
  why the season sweep saw no mass term (`season.md`).

## Published targets

Read in full unless marked; numbers only, no parameter files copied. Two Magic Formula sets recur: MF6.1, the MF 6.1
example set (255/55R20, Fz0 6752 N, 260 kPa) shipped with MFeval.jl (MFeval 4.0 ships another fit of the same size,
PKX1 50.4 against 32.9, longitudinal asymptote 0.75), and PB, the passenger set Chrono's TMeasy
`GuessPassCar70Par` normalises from Pacejka's book data (code read, provenance secondary). Downloads stay in
`/tmp/tyre-research`.

| Effect | Target | Source |
| --- | --- | --- |
| Peak μ against load | lateral μ ∝ Fz^-0.13 (figure); longitudinal falls at 0.72 / 0.39 of the lateral rate | Singh & Sivaramakrishnan (Goodyear), arXiv 2305.18422, Figs. 15-16; MF6.1 / PB |
| Stiffness against load | Cα ∝ Fz^~0.6 (figure; sets 0.47 / 0.69); Kx ∝ Fz^1.31 / 1.08 | Goodyear; MF6.1 / PB |
| Longitudinal stiffness | Kx/Fz 32.9 / 18.4, Kx/Ky 1.62 / 1.15 at Fz0 | MF6.1 / PB |
| Peak slip | 6.5° / 8.5°, κ 0.094 / 0.139 at Fz0 | MF6.1 / PB |
| Cα: pressure / tread / heat | +10 % for +20 % pressure; +30 % at 60 % less tread; -20-25 % cold to hot (bulk temperature) | Goodyear, Table 3 |
| Peak μ: pressure / tread / heat | small; +10 % at 60 % less tread (dry); -10 % cold to hot (surface temperature) | same |
| Grip against temperature | passenger tyre 0.7 to 1.05 warming, optimum 88 °C, width 50 °C | Vera et al., Sensors 22:6380 (2022) |
| Which temperature | grip follows the tread core, stiffness the carcass | Farroni et al., Appl. Sci. 10:1604 (2020) (TRT); contradicts Goodyear's surface-for-μ; both bound NGP's 0.6/0.4 blend |
| Friction against sliding speed | g(Vs) = μ_C + (μ_S - μ_C) exp(-√(Vs/v_s)), μ_C 0.8, μ_S 1.55, v_s 6.57 m/s | Canudas de Wit et al., VSD 39 (2003) 189-226, eqs. 21-22, Table 1: their distributed LuGre (brush) fitted to straight braking on a dry test track (author preprint HAL-00949740; v_s held fixed in the fit) |
| Sliding over peak | lateral 0.83 full slide (PB), 0.73 asymptote (MF6.1); longitudinal 0.71 at κ 0.67 (PB), MF6.1 at 4 kN 0.80 / 0.73 at κ 0.5 / 1 driving, 0.85 / 0.77 braking, asymptote sin(C π/2) 0.65 (at Fz0 0.78-0.83 / 0.72-0.75; the shifts move them 0.01-0.02); locked over peak dry 0.65 to 0.885, wet 0.64 / 0.69 | sets; Burckhardt 1993 dry/wet asphalt via three reproductions (test speed unstated); Kebbati et al., Sustainability 15:6945 (2023), ABS over locked at 30-40 km/h (secondary read) |
| Dry friction against speed | peak "relatively unchanged" with speed on dry runways, fast drop wet; Kiencke & Nielsen's factor exp(-c4 Vs), c4 0.02-0.04 s/m | Horne & Leland, NASA TN D-1376 (1962), aircraft tyres; Shadrin et al., IOP Conf. Ser. 832:012011 (2020) (book not read) |
| Relaxation length | 0.12-0.45 m, longer at higher speed and load, of the order of the wheel radius at Fz0 (Pacejka); grows with speed: σ_y = -0.14 + 0.021 V + 1.9e-4 Fz - 1.6e-8 Fz² m (car tyre bench, ≤2 Hz; also with frequency, <0.5 m near 0 Hz to ~1.0 m at 1 Hz); motorcycle tyres 0.1-0.4 m, "higher ... at higher speeds"; MF-Swift: independent of V, set by load, slip and pressure; σ/R0 at Fz0 lateral 1.01 / 1.84, longitudinal 1.19 / 1.44 | Pacejka via Wikipedia "Relaxation length"; Vera et al. (2022); Cossalter, Lot & Massaro, Motorcycle Dynamics, ch. 1 (Wiley excerpt); Schmeitz, TNO MF-Tyre/MF-Swift lecture (2013); MF 5.2 truck / MF6.1 sets |
| Trail at small slip | a/3 (half contact length a) | brush theory (Pacejka 2012) |
| Convection | Nu ∝ Re^0.8 (NGP 0.78) | West & Limebeer, VSD 60 (2022) |
| Hydroplaning speed | v_p = 6.36 √p km/h (p in kPa) for water deeper than the tread grooves; thin films raise it (Huebner: 26.04 WD^-0.259 mph for WD < 0.1 in); 2 of 8 mm tread costs 17-22 % of v_p; μ ≤ 0.05 at total hydroplaning | Horne & Dreher, NASA TN D-2056 (1963), metric form in Zhang et al., Lubricants 13:76 (2025); Gallaway-Huebner via Ketabdari et al., PLOS ONE (2022); ADAC worn-tyre test (2026) |
| Wet films and speed | road films 0.18 mm in light rain, 0.56-0.85 mm in heavy (drained highways); damp braking μ only slightly above wet; a new tyre brakes from 100 km/h on a watered track at the mean μ it has from 80 (0.98/0.97), at 1.6 mm it keeps 0.69 / 0.52 of new | PLOS ONE (2025), Table 2; TN D-1376; Continental Contidrom via AUTO BILD |
| Wet against slip speed | F(S) = F60 exp((60 - S)/S_p), S_p = 14.2 + 89.7 MPD km/h (smooth test tyre); on the one road with both, S_p 67 predicted against ~220 km/h in locked-wheel data, so not used | ASTM E1960 via two Virginia Tech theses; Rajapakshe et al., Dyna 176 (2012) |
| Low speed | lumped/distributed LuGre: σ_0 181.54 1/m, σ_2 0.0018 s/m, σ_1 ~1 (dynamic fit); MF fades forces below 1 m/s (VXLOW), no standstill | Canudas de Wit et al. (2003), Tables 1, 3; MFeval.jl |
| Longitudinal carcass | c_x at 4 kN (MF-Swift overall stiffness, σ = K/c): TNO 205/60R15 382 kN/m (σx 0.23 m), MF6.1 358 (σx 0.32 m; MFeval 4.0's fit 399), IIT Madras 245/60R16 288; measured 225/50R17 214-234 kN/m (1.9-3.0 bar), P245/70R15 static 163-188 kN/m; c_x/c_y 1.3-2.75 in every set; +5-9 % from 4 to 6 kN | Besselink, Schmeitz & Pacejka, VSD 48 S1 (2010), eqs. 17-20, on the sets; op het Veld, MSc TU/e (2008), Flatplank, Table 4.2; Gent & Walter, The Pneumatic Tire, NHTSA DOT HS 810 561 (2006), Table 9.1 |
| Lateral carcass | road sets σy 0.34-0.75 m at 4 kN (c_y 109-273 kN/m); the two 50-series tyres relax longest, so low profile alone does not shorten σy | same sets; op het Veld; Gent & Walter ch. 8 |
| Carcass damping | MF-Swift residual d_x 690-1060, d_y 300-1000 N·s/m, from the template's 0.002 s on the stiffness (not measured) | same sets |
| Longitudinal past the peak | road MF sets 0.75-0.87 / 0.69-0.82 at κ 0.5 / 1 (4 kN); racing radial MF 6.2 fit 0.68-0.71 / 0.63-0.66 (Dallara AV-21 front, fit range unstated); locked over peak dry 0.58-0.75 (32-100 km/h) | TNO, Continental, MFeval and IIT sets; JyNing04/Pacejka-tire-model (GitHub); Lambourn & Wesley, TRL PPR496 (2010), abstract |
| Carcass against pressure | +0.5 bar: σy -12 %, CFy +9.5 %, CFx +3.5 %; the sets' 2.2-2.6 bar cover rally hot pressures (2.0-2.5 bar) | op het Veld (2008) |
| Camber | camber stiffness Fz (-1.001 + 0.175 dfz) per rad, ~5 % of Ky; car-tyre camber reduction 0.4-0.7 | MF6.1; Romano et al., Meccanica (2021) |
| Rubber friction | no static/kinetic pair: μ(v, T) with flash temperature | Persson, arXiv 1007.2713, cond-mat/0605273; Grosch 1963 (secondary) |

Not found in open sources: rally or racing-radial tyre data (TTC data is licence-gated), the full Gallaway 1979 tyre
terms, road-sensor damp/wet film thresholds, gravel cut onto tarmac, damp tarmac grip. Those stay calibrated on stage
times.

## NGP against the targets

- Matches: peak-μ load sensitivity (Fz^0.878 against ~0.87), trail a/3, convection exponent, a temperature bell.
- Off: Cα load exponent 0.36 (0.47-0.69); Kx/Fz 14 at 4 kN (18.4-32.9), Kx ∝ Fz^0.35 (1.08-1.31); stiffness blind to
  temperature, pressure apart, and tread depth; dry wear lowers grip (tread loss raises it); wet grip fixed per
  surface (no speed or film effect); static/kinetic pair; peak slip angle 9.5° at 4 kN (6.5-8.5°); lateral transient
  0.04-0.10 m to 63 %, shrinking with speed (σ/R0 1.0-1.8 in the sets); the sticking model's 360 Hz alternation;
  tread warm-up of ~8 K in 20 s of hard cornering.

## Candidate (brush-v1)

`rbr_tyre::brush` (`calibrate`; the lab's `examples/compare.rs`). Pacejka's brush with combined slip (ch. 3)
split as Svendenius splits it (adhesion along the stiffness-weighted slip, sliding against the sliding velocity;
below) and
Rill's first-order force dynamics (below). Cy ∝ Fz^0.6, Cx = 1.4 Cy at Fz0 and ∝ Fz^1.2, both times bulk temperature
(-0.45 %/K), tread depth (2.6/(d + 1.6)) and NGP's pressure factor; peak Fy ∝ Fz^0.87, Fx ∝ Fz^0.93; friction
against sliding speed by Canudas de Wit's g(Vs) with their v_s and μ_C/μ_S; NGP's temperature grip kept; dry wear
+10 % at 60 % worn, fading with wetness; wet loss h/(h + 0.5 mm) · min(1, (V/v_p)²) · 0.6 with h = W · 0.7 mm and
v_p = 6.36 √p (0.75 + 0.25 d); force dynamics F = c u + d u', (v d + f) u' = f v_N s - v c u per axis (Rill,
Vehicle Dynamics lecture notes, Hochschule Regensburg 2009, 3.4.2 and 4.2.2; f = F/s the steady secant, s made with
v_N = max(v, 0.5 m/s), v itself the transport speed as in Rill's eqs. 4.20-4.21, so a locked wheel's deflection
follows the sliding and a stopped one's holds; c_y NGP's carcass, c_x 450 kN/m times the pressure factor over its
calibration value, at every load (road sets 288-399 kN/m at 4 kN, measured 163-234, c_x/c_y 1.3-2.75); d the mean of
Rill's passenger values, d_x 500/1500 and d_y 250/600 N/(m/s)), combined force and the deflections capped at μ Fz as
Rill caps them; relaxation length (f + v d)/c, the stretched string's C/c at small slip, no other speed term. Calibrated
at run time to NGP's lateral peak and slope and longitudinal peak at 4 kN, 15 m/s (no NGP values in code). The dynamics
reach standstill; the game runs the brush from 2 m/s.

Combined slip (2026-11-07): the adhering part of the patch gives -C_i s_i (1 - ψ)² along the stiffness-weighted
slip, the sliding part μ Fz ψ² (3 - 2ψ) against the sliding velocity, μ Fz from ψ = 1 (Svendenius, Tire Modeling and
Friction Estimation, PhD, Lund 2007, eqs. 4.14, 4.17, 4.18-4.20, 4.26-4.28; read in full); the directional μ is the
ellipse along the sliding velocity, his collinear slide force. The aligning moment is his M'_z (eqs. 4.33-4.35):
μ Fz a (1 - ψ)² (θ_y s_y (1 - 4ψ) + 3ψ² s_y/|s|); the deformation torque M''_z, ∝ (C_y - C_x), stays out. Pure
slip is Pacejka's cubic either way, so the curves, the per-wheel fit and the calibration are unchanged (lab CSVs
identical). Before, the whole force pointed along (C_x s_x, C_y s_y), so at full slide the side share was 1/1.4 of
the sliding velocity's. Pacejka's own ch. 3 text (isotropic bristles, where both directions coincide) was not
reread here: the book is not open.

Ours, not from papers: the wet loss's film ramp, its 0.5 mm, the quadratic and the 0.6 weight, and the wear gain's
fade with W; leaving out σ's speed term (2026-11-07, user: the measured slope is a passenger tyre's, with its
excitation frequency mixed in, and puts a rally tyre above Pacejka's 0.12-0.45 m); taking the longitudinal exponents
and Kx/Ky as the mean of two road-tyre sets; the carcass damping as the mean of Rill's two passenger values; the
longitudinal carcass's 450 kN/m (above the road sets; c_x/c_y 1.07-1.37 against NGP's lateral carcass); NGP's
lateral carcass stiffness and temperature grip.

Against NGP and the targets on the Fiesta Rally2 (`compare` summary; brush-v0 in brackets where it changed):

- 2026-11-07, low speed (Golf II GTI 16V, track-41_M climb, realistic engine): the earlier transient (a lag on the
  slip, σ = C/c, rate V_r/σ) ran σ_x 1-2.8 m at 4-7 m/s on NGP's low-speed carcass and almost froze as the wheel
  locked (V_r → 0.5 m/s): after a spin-to-brake reversal the force kept the wrong sign 0.21-0.33 s (lab, fronts at
  2.4-3.5 kN, 4.5 m/s, wheel 2.5 → 0.5 times the car) and drove a wheel-diff oscillation (FL +11 to -6 m/s, steady
  slip +0.35 against transient -0.12). Steady grip was not short: 1.38 μ peak at κ 0.15, 1.10 at the spin's κ 1.5
  (sliding 6.75 m/s). Rill's dynamics turn it in 3-15 ms; σ_x 1.0-1.6 m at κ 0.02 to 0.25-0.3 m sliding; Rill: σ_x
  passenger 0.7 m, review 0.3-0.8 m; measured σ_y 0.6 m at 0° to 0.1 m at 10° (Rill Fig. 3.43).
- 2026-11-07, longitudinal carcass (`low_speed` example; quarter car stepped explicitly at 720 Hz, inertia 0.6-4 kg
  m²): 63 % of a κ 0.05 step after 0.90-0.93 m (Golf) / 0.82-0.85 m (Fiesta) below 10 m/s on NGP's tenth, 0.27-0.30 m
  with c_x = Cx/(1.44 R) at 1-25 m/s (NGP unscaled 0.09-0.10 m, Rill's 170 kN/m 0.35-0.41 m). From 0.5 to 10 m/s a
  drive-torque step settles with no step-to-step alternation for every choice (NGP's reason for the tenth, its
  explicit wheel step, does not return with Rill's damping): from free rolling the wheel slip peaks at 0.30-0.71 m/s
  against 0.47-0.95 on the tenth and settles within 0.05-0.41 s (0.07-0.51 s), 18-97 crossings of the settled slip in
  2 s; NGP unscaled rings longer (25-420). Down to standstill with the transport speed restored: pull-away, braking to
  a stop and holding on a 10 % grade stay stable with no creep beyond the rocking; after an abrupt stop the car rocks
  on the carcass at 3.5-4 Hz with ζ 0.05-0.07 (Rill's Fig. 4.4: 3.4 Hz, "oscillates some time"), 2-5 mm in the 2 s
  after.
- 2026-11-07, c_x 450 kN/m (`low_speed`, Fiesta Rally2 / Golf II wheel 0 at 3.9 / 3.4 kN; before, Cx/(1.44 R)
  248 / 200 kN/m, in brackets): 63 % of a κ 0.05 step after 0.13-0.16 m at 1-25 m/s (0.24-0.31 m). From 0.5 to 10 m/s a
  drive-torque step settles with no step-to-step alternation, within 0.04-0.37 s (0.05-0.41 s), slip peak 0.22-0.60
  m/s (0.31-0.71), with more and quicker crossings, 26-146 in 2 s (18-95; most at 5-10 m/s, inertia 0.6). Pull-away,
  braking to a stop and holding on the 10 % grade stay stable: the rock after the stop moves 6-13 mm in the first
  second (10-19 mm) and 0.2-0.9 mm in the next two (1.1-5.1 mm).
- Meets: combined slip at 4 kN, 15 m/s, Fy(κ ±0.25)/Fy(0) 0.283 at 2°, 0.333 at 4°, 0.465 at 8° (Svendenius's
  measured road tyre 0.2-0.3 and 0.29-0.34; NGP 0.40/0.46/0.61; before 0.20/0.24/0.35), Fy/|F| equal to the sliding
  velocity's share once sliding (0.269 at 4°; NGP 0.35); peak κ 0.12 drive / 0.10 brake at 4 kN (0.18-0.22); dry
  locked over peak at 10 m/s 0.80 (0.70); braking
  peaks within 4 % of NGP's (17 %); Kx/Fz 22 by the |κ| ≤ 0.04 fit, Kx ∝ Fz^1.14; peak Fx ∝ Fz^0.96; Cα ∝ Fz^0.64;
  Cα -21 % cold to hot and +27 % at 60 % worn; dry peak μ +12 % at 60 % worn; peak Fy ∝ Fz^0.85; σ_y 0.20 m at 4 kN
  (σ/R 0.63; NGP 0.17 m; Pacejka 0.12-0.45 m); with the damping 0.20-0.23 m at 5-30 m/s, 63 % of a 2° step in
  0.17-0.18 m at 10-30 m/s (0.18-0.21 m before).
- New behaviour: peak μ_y falls 10 % from 10 to 30 m/s (1.310/1.264/1.231/1.181, NGP flat; Kiencke & Nielsen's
  c4 0.02-0.04 s/m gives 4-8 % at 10 % slip and 20 m/s); wet W = 1 peak 0.99/0.78/0.64 at 10/20/30 m/s against
  NGP's flat 1.00.
- Outside the published values: slide over peak 0.88 lateral at 30° (sets 0.73-0.83; NGP 0.87): the published g falls
  less after the peak than the sets (longitudinal 0.86 at κ 0.5 is inside the road sets' 0.75-0.87; NGP 0.92); wet
  locked over peak 0.77 (0.64-0.69): the wet adds no sliding-speed loss; the wet loss is the wrong shape: the new tyre
  loses 18 % from 20 to 30 m/s (Contidrom: nothing to 100 km/h) and the worn one barely more than new at 30 m/s
  (Contidrom: 0.52 of new from 100 km/h); peak slip angle 8.75° (6.5-8.5°); trail 0 past saturation (NGP 0.004 m).

## In game

- Seam: `rbr_physics::tyre::model` (`ForceInput`, `ForceOutput`, `apply`), asked in `tyre::step` after NGP's sliding
  solve and the slip phase; `rbr_hybrid::ngp::set_tyre_model` per thread, as the engine's. The model's forces become
  the deflection and torsion `solve_forces` turns back into them (in-game probe: `lateral_force` equals the model's to
  the f32). With no model set the code is NGP's: rsf_compare drive2/5 physics PASS 84084/84084, exit 0.
- Scope: contact speed ≥ 2 m/s (NGP's sliding model alone there) and no soil; the weight rises from 0 at 2 m/s to 1
  at 3 m/s. Below, and on soil, NGP's.
- Grip: NGP's static friction (surface, wet or dry, times the temperature grip) times the inclination's table scale
  scales μ_S and μ_C both, so the paper's μ_C/μ_S holds on every surface; NGP's kinetic friction is unused. The wet
  loss and the hydroplaning speed stay off (NGP's wet surface values carry the wet); the dry wear gain and the
  stiffness's temperature, wear and pressure factors are the brush's.
- Calibration per wheel at fit (`rbr_tyre::fit`): NGP's `patch_forces` swept from the wheel's own tables at
  `load_axis[3]` (the 4 kN design load times the car's mass ratio), 15 m/s, unit surface, stiffness factor 1; t_cal is
  the temperature of `RubMyFct`'s best friction. Nothing NGP-derived is committed.
- Dynamics: Rill's carcass deflections per wheel, the model's state (snapshots don't carry it; restart and
  touch-down zero it). c_y is NGP's lateral carcass; c_x 450 kN/m times the pressure factor over its calibration
  value (2026-11-07; Golf II 447 kN/m, Fiesta Rally2 435 kN/m, c_x/c_y 1.37/1.07; NGP's torsional over R² 640/796
  kN/m, a tenth of it below 10 m/s), the same at every speed and load;
  trail and phase are the steady brush's.
- Runs below are `experimental-brush` (realistic engine + brush; `--physics experimental` was this pairing before the
  brush flag split). Golf II GTI 16V, track-41_M climb (gear 2, 4-5 m/s, throttle 0.15-0.5, 10-13 s), front wheel over
  car speed mean/max: NGP 0.98/1.02, realistic + NGP tyre 1.02/1.09; before 1.54/3.04 with a stall at 11.72 s, brush
  alone 1.26/2.51; now 1.03/1.23, brush alone 0.98/1.19, no stall. Fiesta Rally2 8-20 s driven/car mean/max NGP
  0.99/1.16, before 1.26/2.82, now 1.01/1.38; Fiat 131 rears 1.14/3.10, 1.14/2.98, 1.19/3.03.
- track-41_M four-car 600 s, experimental-brush split 2 / finish before → now (NGP 7 210.71/309.01, 212.97/310.91,
  222.28/324.24, 221.31/323.35): Fiesta 207.80/304.88 → 207.97/305.08, Celica 208.15/305.22 → 208.48/305.53, Golf
  stalled at 161 m → 214.21/314.48, Fiat 214.54/314.89 → 216.44/316.91; slip > 3 % 6.7/4.2/-/5.0 → 3.0/3.5/4.8/5.2 s;
  over the limit and rpm over it unchanged. Fiesta 0-100 RSF 504 4.33 → 4.35 s (NGP 4.52). After the finish (full
  brake in fifth from 24 m/s) both physics lock the Golf's fronts; the realistic engine stalls with the clutch in,
  NGP's idles (engine side, `engine-model.md`).
- Combined slip along the sliding velocity (2026-11-07), experimental-brush before → after: four-car split 2 /
  finish Fiesta 207.97/305.08 → 207.60/304.67, Celica 208.48/305.53 → 208.31/305.33, Golf 214.21/314.48 →
  213.99/314.25, Fiat 216.44/316.91 → 215.63/316.06 (NGP unchanged, 210.71/309.01 and the rest as above); slip > 3 %
  3.2/3.6/4.8/5.2 s, no NaN; the Golf's 1.5 s under 30 rad/s is the post-finish stall above (321-323 s). Debug trace
  of the wheel speeds (not committed): Golf climb 10-13 s fronts over car mean/max 0.97/1.26 before and after (NGP
  0.95/1.07), after in gear 2 below 6 m/s 0.99/1.37 (NGP 0.98/1.11), engine at least 141 rad/s, no stall; Fiesta 8-20 s 0.97/1.46 →
  0.97/1.47, Fiat rears 1.09/3.03 → 1.08/3.03. Full brake from 15 m/s on map 41 (`--brake-from 15`, path length
  to the test's end at 0.3 m/s): 11.77 → 11.79 m, 10.7 m/s² mean, against NGP 12.03 m and experimental 12.10 m
  (10.2 m/s²); all lock. The straight-line numbers barely move, as expected: the change is in combined slip only.
- 2026-11-07, c_x from MF 6.1's relaxation length, measured before the combined slip change (track-41_M, 3-8 m/s on
  throttle, driven wheel over 1.1 times the car speed, share of the time, before → now, NGP 7): Golf 0.29 → 0.16 (0.10),
  Fiesta 0.18 → 0.12 (0.14), Fiat 0.19 → 0.13 (0.07); |Fx - steady Fx|/|steady Fx| 0.48/0.53/0.44 → 0.35/0.37/0.25.
  Step-to-step Fx ripple there 18-27 → 20-34 N on 1.7-2.0 kN; the wheel-speed 180-360 Hz content is 1.3-1.9 times NGP's
  in amplitude (1.1-1.4 before), no instability. Golf climb (10-13 s, both fronts, car above 1 m/s) mean/max 0.96/1.23
  (before 0.97/1.26, NGP 0.95/1.07), no stall. Four-car 600 s split 2 / finish: Fiesta 207.56/304.58, Celica
  207.72/304.75, Golf 213.86/314.12, Fiat 215.56/316.02; slip > 3 % 1.9/3.6/4.6/5.5 s; over the limit 0.2/0/0/0 s; the
  Golf stalls only after the finish (engine side). Fiesta 0-100 RSF 504 4.33 s (NGP 4.52). Brake tests from 10 m/s
  (track-41_M, `--brake-from`; full pedal locks in both physics): stopping times within 0.12 s of NGP's except the
  Fiesta at 0.35 pedal (1.63 against 2.30 s; the same with Rill's 170 kN/m, the brush before this change not run).
- 2026-11-07, c_x 450 kN/m in place of Cx(Fz0)/(1.44 R) (debug trace; before → after, NGP 7 in brackets; the
  baseline rerun on main after the combined slip change): track-41_M 3-8 m/s on throttle, driven wheel over 1.1 times
  the car speed Golf 0.145 → 0.128 (0.099), Fiesta 0.122 → 0.107 (0.141), Fiat 0.123 → 0.113 (0.067); |Fx - steady
  Fx|/|steady Fx| 0.33/0.36/0.25 → 0.20/0.28/0.16; step-to-step Fx ripple 20/34/30 → 25/40/33 N on 1.6-2.0 kN;
  wheel-speed reversals 0.24/0.31/0.21 → 0.24/0.31/0.22 of steps (0.14/0.17/0.11); wheel-speed 180-360 Hz power
  8.2/51/11.4 → 7.6/61/11.6 (5.4/22/3.1). Golf climb 10-13 s, both fronts, mean/max 0.96/1.21 → 0.96/1.19
  (0.95/1.07). Four-car 600 s split 2 / finish: Fiesta 207.33/304.34 → 207.09/304.08, Celica 207.73/304.77 →
  207.53/304.51, Golf 213.83/314.06 → 213.68/313.86, Fiat 214.97/315.41 → 215.28/315.69 (NGP unchanged); slip > 3 %
  1.9/3.6/4.8/5.5 → 2.0/3.5/4.5/5.1 s, no NaN, the Golf stalls only after the finish. Brake from 10 m/s, stop after
  full / 0.35 pedal: Golf 1.00/2.18 → 0.99/2.18 s (0.99/2.31), Fiesta 0.90/1.63 → 0.88/1.63 (0.89/2.30), Fiat
  1.09/2.38 → 1.07/2.36 (1.17/2.38); reversals around the stop within 0.008 of before. Fiesta 0-100 RSF 504 4.32 →
  4.33 s (4.52). No instability; the Fiesta's 180-360 Hz content, already 2.3 times NGP's, rises a fifth.
- 2026-11-07, the brush to standstill (prototype, not committed: weight 1 at every speed on hard surfaces): no
  instability. Brake tests stop in the same times as with the 2-3 m/s handover; at 0.35 pedal the wheel-speed
  reversals in the second around the stop fall from NGP's 0.06-0.09 of steps to 0.006-0.03 (NGP's 360 Hz sticking
  alternation gone) and the rock-back after the stop from 0.16-0.24 to 0.10-0.14 m/s; at full pedal reversals
  0.004-0.10 against NGP's 0.001-0.07. Held at the start (handbrake, revving) with no drift; track-41_M runs and the
  Golf climb within 0.1 s and 0.01 of the handover's. Below 2 m/s NGP's `solve_forces` still scales the model's force
  by its sticking-weight damping and adds its hold torque (the seam), so the brush would not be alone there.
- Fiesta Rally2, map 41 tarmac, autopilot, before the force dynamics (`ngp` / tyre only / full `experimental`, σ
  without the speed term, engine with the light pedal): 150000 steps reach 3007 / 3017 / 3066 m, split 1 118.82 /
  118.39 / 116.95 s, no NaN; full brake from 15 m/s stops in 9.86 / 9.40 / 8.96 m (11.4 / 12.0 / 12.6 m/s² mean). No
  measurable step cost (100000 steps 31-33 s either way).
- Ramped 40 m circle (map 85, `--circle -260,120,40 --circle-ramp 0.25`): NGP and the brush alone hold the same
  steering to the same limit, 8.6 m/s², then both run wide at full lock; the autopilot's steering steps (0.008) hide
  any linear-range difference. Steady state matches by the fit; the differences are in transients, combined slip,
  speed, load and temperature (lab tables above).

## Racing and rally targets

No open rally-tyre force data exists, and no open measured racing data reaches a rally car's loads except one
Milliken figure. The racing curves below are Formula Student slicks at 0.2-1.1 kN; their shapes carry over, their
levels and stiffnesses per newton do not. TTC data itself (licence-gated) was not used; two sources plot it.
Downloads stay in `/tmp/racing-tyre-research`. Precision: "table" exact; "fig ±x" read off a figure; "MF" computed
from a published fit.

Sources:

- A: Harsh & Shyrokau, Appl. Sci. 9:5328 (2019), Figs. 3-6: Apollo Vredestein FS slick (and a Hoosier, 5 % more
  lateral), trailer on a real road, 600/1000 N, speed unstated, μ "normalised" (scaling unstated); Appendix A MF 6.1
  fit (its combined-slip terms are estimated, not measured).
- B: Cantrell, UEN Pressbooks (CC BY), Figs. 20, 23: Hoosier R20 18.0x6.0-10, TTC round 9 (Calspan belt, 40 km/h,
  abrasive belt so μ 2-2.8), 219/664/1112 N with camber and pressure changing with load, slips to 12° and κ ±0.2.
- C: Ozerem & Morrey, Oxford Brookes RADAR, Figs. 6-7: Avon 6.2/20.0-13 FSAE, TTC belt, loads inferred from the
  sweep order (worker read, not rechecked).
- D: Milliken & Milliken, RCVD Fig. 2.9 via Wikipedia "Tire load sensitivity" (table, second-hand): peak Fy/Fz
  1.10/1.08/0.97 at 4.0/6.0/8.0 kN, at 5.6/6.0/6.7°; the tyre is not named.
- E: West, MEng thesis, Pretoria (2020), Table B.3: F1 tyre parameters from Tremlett & Limebeer (VSD 2016), a model,
  not a measurement.
- F: Farroni et al., TRT EVO, Proc. IMechE D 233 (2019), Fig. 2: GT racing tyre, on-track, normalised.
- G: Svendenius, Tire Modeling and Friction Estimation, PhD, Lund (2007), Figs. 5.2, 5.5, 5.7, ch. 4: measured
  combined slip of a Goodyear Wrangler RT/S P225/75R15 (NHTSA data, 48 km/h, 2.4 bar, 5.3-5.5 kN). A road
  all-terrain tyre: the only measured combined-slip data found.
- Rally: ChassisSim, "The vehicle dynamics of rallying" (Nowlan, blog PDF): a 1300 kg AWD car at 1 g, tarmac peak slip
  6° (model inputs, "in the ball park"); a road racing tyre loses 10-20 % past the peak. Pirelli P Zero RA datasheet:
  tread windows 20-60 °C (RA9) to 70-110 °C (RA5), hot 2.0-2.5 bar. Michelin 2023-24 pressures: asphalt rally hot
  2.0-2.3 bar, cold at least 1.6. FIA tread rule (2006 copy): at least 17 % grooves of 5.5 mm. Track cars on
  semi-slicks (Darkside Developments, VBOX, single-session peaks): 1.32-1.53 g lateral, 1.17-1.47 g braking.

Rally on tarmac: no Rally1/Rally2/R5 g data, logger trace or g-g diagram is open (searched telemetry overlays,
WRC+ graphics, team and engineer publications); the only rally numbers are the ChassisSim model inputs. Combined
slip on tarmac rally is described only qualitatively (trail braking: brake hard and ease off on turn-in, Team O'Neil).

Comparison: NGP and brush-v1 on the Fiesta Rally2 at 4 kN (`racing` example; 15 m/s unless stated):

| Property | Racing / rally | Road targets | NGP | brush-v1 | Racing moves the target |
| --- | --- | --- | --- | --- | --- |
| Peak μ (car) | semi-slick track cars 1.32-1.53 g lateral, 1.17-1.47 g braking; rally car ~1 g (estimate) | | 1.265; brake 1.16 g mean from 15 m/s | 1.265 (calibrated); 1.28 g | no: no rally level, NGP sets it |
| Peak Fy ∝ Fz^n | 0.70 C, 0.73 A (fig ±0.05), 0.80 E, 0.82 D (4-8 kN), 0.84 B | 0.87 | 0.878 (2-6 kN), 0.787 (4-8 kN) | 0.846, 0.845 (4-8 kN) | down, to ~0.80 |
| Peak slip angle | A flat from 4-5° to 17° (fig ±1°); B 9-12° (fig ±1°); D 5.6-6.7°; E 8-9°; rally 6° (estimate) | 6.5-8.5° | 9.5° (95 % at 6.75°) | 8.75° (95 % at 6.5°) | down only by D and the estimate, both weak |
| Peak κ | A 0.08-0.12 (fig); B 0.10-0.12 drive, 0.13-0.18 brake (fig); E 0.10-0.11 | 0.094-0.139 | 0.16 | 0.12 drive / 0.10 brake | no |
| Cα ∝ Fz^m | A 0.87 (MF); B ~0.56 (fig ±30 %) | 0.47-0.69 | 0.36 | 0.64 | no (spread) |
| Cα/Fz, Kx/Fz per rad | A 31-34, 41-47 (MF); B 35-39 at 664 N (fig ±30 %) | Kx/Fz 18.4-32.9 | 17.6, 14.1 | 17.6, 22.0 | no: at 0.6-1.1 kN; scaled to 4 kN by Fz^-0.4 (inferred) ~19 |
| Lateral over peak | A 0.95 at 17° (fig ±0.03); B 0.96 at 12° (fig); ChassisSim 0.8-0.9 (quoted) | 0.73-0.83 full slide | 0.992 at 12°, 0.968 at 17° | 0.974 / 0.937 (11.2 m/s: 0.979 / 0.945); 0.876 at 30° | up: brush-v1 is inside the racing range to 17° |
| Longitudinal over peak | A 0.90-0.95 at κ 0.5-1 (fig ±0.05); B 0.93 at κ 0.2 (fig) | 0.71-0.85 at κ 0.5-1 (asymptote 0.65) | 0.920 at κ 0.5, 0.781 at 1 | 0.861 / 0.786 (11.2 m/s: 0.873 / 0.798) | up: brush-v1 0.05-0.15 below racing |
| Combined: Fy(25 % slip)/Fy(0) | none open for racing; G (road) 0.2-0.3 at 2°, 0.29-0.34 at 4° (fig ±0.05); Fy/\|F\| at 4° ~0.26 | | 0.40 at 2°, 0.46 at 4°; Fy/\|F\| 0.35 | 0.28, 0.33; Fy/\|F\| 0.269 (before the split 0.20, 0.24; 0.196) | none from racing; brush-v1 now inside G |
| Relaxation length | Hoosier R25B 0.49-0.50 m (Cal Poly senior project, search excerpt only, its 0.55 s does not fit 40 km/h): not used | 0.12-0.45 m | 0.17 m | 0.20 m | unknown |
| Grip against temperature | F: GT optimum 95-103 °C core, ~0.6 at 60-65 °C, ~0.75 at 75, ~0.85 above 105 (fig ±0.1); Pirelli RA: 40 K window per compound | passenger optimum 88 °C, width 50 °C | `RubMyFct` 0.838 at 20 °C, 1.0 near 65-85, 0.85 at 130 | NGP's | no: a slick's bell is steeper; rally compounds move the window |

The combined-slip gap had a cause in the model, not in the data (fixed 2026-11-07, recommendation 1): at full slide
brush-v1 pointed the force along the
stiffness-weighted slip (Cx sx, Cy sy), so with Cx = 1.4 Cy the side share is 1/1.4 of the sliding velocity's
(0.196 against 0.269 at 4°, 25 %). Friction opposes the sliding velocity (Svendenius ch. 4, eqs. 4.18-4.22; G's
measured share 0.26 matches the sliding direction).

Recommendation (1 done, 4 evaluated and not taken, 2026-11-07; 2, 3 and 5 stand):

1. Combined slip: direct the sliding part of the force along the sliding velocity, keeping the stiffness-weighted
   direction for the adhering part (the BPL / Svendenius construction). At 4° and 25 % that takes the side force
   from 947 N to about 1300 N (inferred: total force unchanged times the sliding share), 0.33 of pure against G's
   0.29-0.34 and NGP's 0.46. The open item "half NGP's side force" becomes about 0.73 of NGP's, and that remainder is
   backed by measured data. Done: 1302 N, 0.333 of pure, 0.72 of NGP's ([Candidate](#candidate-brush-v1)).
2. Sliding over peak: keep. Racing slicks keep 0.93-0.97 of the peak to 12-17° and 0.90-0.95 to κ 1, above the road
   sets; brush-v1 sits between, inside the racing lateral range. A tarmac rally tyre is treaded (at least 17 %
   grooves), between road and slick (inferred). The longitudinal fall is the steeper side; locked-wheel road data
   (0.65-0.885) still brackets it, so no change.
3. Peak slip angle 8.75°: keep. The FS data are flat-topped (no single peak), and the only data at rally loads (D,
   5.6-6.7°) names no tyre; brush-v1's 95 % point, 6.5°, already sits there. Lowering the peak means a cornering
   stiffness above NGP's (peak slip scales with μFz/Cα), giving up the fit to NGP's slope: a product decision.
4. Load sensitivity, optional: N_MU_Y 0.87 to ~0.82 (racing 0.70-0.84; NGP itself 0.79 above 4 kN); costs 2 % peak
   μ at 6 kN. Low priority. Not taken: tried in the lab, 0.82 holds the 4 kN peak (1.265; the fit is at the design
   load) and moves the 4-8 kN exponent from 0.845 to 0.800 (NGP 0.787), 6 kN μ 1.188 → 1.166 (NGP 1.181). The
   evidence for it is weak: the FS exponents (0.70-0.84) are at a tenth of a rally car's loads, where the
   exponent depends on the load range; D (0.82) is the only figure at rally loads, second-hand, tyre unnamed; E is a
   model. The one measured road sweep (Goodyear, 0.87) and NGP over the 2-6 kN a rally car runs (0.878) agree with
   0.87, and 0.82 would take the light (inside, 2 kN) wheel's peak μ further above NGP's (1.407 → ~1.46, NGP
   1.356). Revisit with a rally or racing tyre's load sweep at 2-8 kN.
5. Leave peak κ, the stiffness exponents, σ and the temperature grip: racing data do not move them, or are unverified.

Missing: any rally tyre curve; measured combined slip for any racing tyre (TTC combined runs are licence-gated);
rally g data on tarmac; a verified racing relaxation length; absolute μ against temperature for a slick.

## Loose surfaces: NGP against published data

Runs (2026-11-07, `ngp` physics, Fiesta Rally2): gravel on RSF 345 Azov II (`--circle 1051,-166,40 --circle-ramp
0.4`, step 6000, Gravel_Fine_Shallow_Dry 17, kind 4); snow on retail map 10 (`track-10_O`, autopilot, step 6000,
SnowOnGravel_Shallow 101, kind 6), SnowOnIce_Shallow (117, RSF 162 Karlstad, step 21000) within 0.05 of it; tarmac map
85 as above (step 8000, wheel 1). Ice: material 116 (no name in 1.02's surface table; μ 0.10/0.095, no soil, on the
RSF 565/614 winter tarmac and in the wet entries of 124/156) met no run, so `sweep --friction 0.1,0.095` on the snow
frame (its tyre-class factor taken as 1, unverified). "Sweep": pure `patch_forces` at 15 m/s; "step": `ngp_transient
--ramps-only`, NGP's whole per-wheel step on the run's `--tyre-image`, 15 m/s unless stated; 4 kN unless stated.
Published values, sources and precision labels from the 2026-11-07 loose-surface research (its files are not in git).

- Tyre kind: `--tyre N`, else `boot::default_tyre` of the stage's surface (retail `boot::surface(map)`; an RSF stage's
  own): tarmac 0, gravel 4, snow 6. Materials: the stage's `.mat` entry for the wear and wetness gives each
  triangle's surface id; NGP's stage init writes those ids' values (`materials`).
- Soil enters on every loose material: SinkFactor 1.5-5e-7, not 0. At car loads the tyre sinks through 17's 9 mm
  and 101's 13 mm, so the friction is the hard value; the soil adds a lateral force ∝ density · patch length · sink ·
  v_lat² (ρ 1300 / 1100): at 30°, 127 / 161 N at 15 m/s, 350 / 444 N at 25 m/s. Its longitudinal force is multiplied
  by 0 (`tyre::soil`). 30 mm layers (103) sink partly and blend toward the soil friction. Tarmac and 116: SinkFactor
  0. The step stays within 0.03 μ of the sweep to 12° on gravel (0.013 snow, 0.005 tarmac).

| Property | Published | NGP gravel | NGP snow | NGP ice (116) | Tarmac reference |
| --- | --- | --- | --- | --- | --- |
| Peak μ_y | gravel ≲ 0.67 WRC wet loose (CW, quoted; μ inferred), 0.55 wet crushed (TR1011, quoted); packed snow 0.35-0.43, loose 0.28-0.33 (TR0713, table); ice 0.15-0.17 (TR0713, table) | 0.828 sweep, 0.854 step | 0.851, 0.857 | 0.099 | 1.262, 1.266 |
| Over tarmac | gravel 50-60 % (CW, quoted) | 0.66 | 0.67 | 0.08 | 1 |
| Peak slip angle | gravel ~12° (CW, quoted), 12.1° (Velenis, MF), dirt 16° (ChassisSim, model input); packed snow 9-17°, loose 16-18° (TR0713, table); ice 4.5-5.1° (TR0713, table) | 13.5° (95 % at 9°); step 14° | 11° (95 % at 7.75°); step 11.25° | 1.25° (95 % at 1°) | 9.5° (6.75°) |
| μ_y at 30° over peak | gravel 0.63 (CW, quoted; ratio inferred), 0.85 (Velenis, MF); snow no fall to ~17° (TR0713, quoted); ice 0.59 at 20° (TR0713, fig ±0.005) | 0.956; step 0.985, at 25 m/s rising to 30° (0.897) | 0.955; step 0.997, at 25 m/s rising (0.926) | 0.976 at 20°, 0.965 | 0.865 |
| Peak κ | gravel none open ("0.4-0.7", snippet); snow still rising at 0.30 (Surkutwar, fig ±0.01); ice 0.05 (Ružinskas, quoted) | 0.24 (2 kN 0.16) | 0.22 (0.12-0.30 at 2-6 kN) | 0.04-0.06 | 0.16 |
| μ_x at κ 1 over peak | gravel 0.75 (Velenis, MF); snow no fall to 0.30 (Surkutwar, fig); ice ~0.5 at 0.45 (Ružinskas, fig ±0.005) | 0.82 sweep and step | 0.85 | 0.93 | 0.78 |
| Ploughing: locked against peak | gravel: locked stops 25-30 % shorter than ABS, decel 1.25-1.30× (SAE 1999-01-1287, table; ratio inferred); car braking 0.50-0.60 wet crushed (TR1011, table) | locked 0.82 of peak (step 0.82, 0.79 at 25 m/s); game, brake test on 345 from 100 / 50 km/h: full pedal 0.80 / 0.63 g, 4 / 6 % longer than the best threshold braking (K 0.5, 0.83 / 0.67 g); the full pedal leaves the most-slipping wheel at slip 0.83 / 0.57, the rim speeds near 3 m/s | 0.85 | 0.93 | 0.78; RSF 568 full pedal 1.11 / 1.16 g, 17 / 5 % longer than threshold at K 0.3 (1.30 / 1.22 g) |
| Peak μ_y at 2/4/6 kN (Fy ∝ Fz^n) | snow no clear loss at 4.6-11 kN (TR0713, inferred, weak); soil lateral coefficient falls slightly at 1-4 kN (El-Razaz review, quoted); gravel none | 0.889/0.828/0.774 (0.875); step 0.913/0.854/0.797 | 0.902/0.851/0.788 (0.881); step 0.928 (rising)/0.857/0.795 | 0.105/0.099/0.092 (0.886) | 1.353/1.262/1.179 (0.877) |
| Peak μ_x at 2/4/6 kN (Fx ∝ Fz^n) | none | 0.864/0.849/0.845 (0.982) | 0.877/0.870/0.858 (0.979) | 0.101/0.100/0.101 | 1.309/1.292/1.286 (0.984) |
| Cα/Fz at 4 kN; Cα ∝ Fz^m | initial slope packed snow ~10/rad, ice ~4.6/rad (findings, inferred from figs); gravel none | 9.1/rad; 0.325 | 10.5/rad; 0.338 | the snow tyre's (μ alone changed) | 17.6/rad; 0.359 |

Where NGP sits:

- Level: gravel and snow are one surface to NGP, 0.83-0.86 at 4 kN (0.66-0.67 of tarmac): gravel 1.3-1.5× the
  published peaks, snow about twice. Ice (116) is 0.10, below the measured 0.15-0.17.
- Shape: the peak slip angles of gravel (13.5°) and snow (11°) fall inside the published ranges, because the gravel
  and snow kinds are softer (Cα/Fz 9-10.5 against 17.6); ice peaks at 1.25° against 4-5°, its stiffness being the
  snow tyre's. Past the peak every loose curve is flat (0.96 of peak at 30° in the sweep) and the soil force makes it
  flatter or rising, more with speed: like the snow data, flatter than the gravel data (0.63-0.85), and ice does not
  fall (0.97 against 0.59).
- Ploughing: none longitudinally (the soil's longitudinal force is zero); a locked wheel keeps 0.82 of the peak on
  gravel, so it stops longer than one at peak slip, where the published locked wheel stops 25-30 % shorter.
- Brake test (2026-11-07, `harness/brake_test.py`, Fiesta Rally2, `--flat-out --top-speed 80`, full pedal against
  `--brake-slip` K 0.1-0.7; ratio = full-pedal distance / best threshold distance): on gravel the force rises with slip
  to K 0.5, and no run stops shorter on the full pedal.

  | Surface | Physics | From | Full pedal | Best threshold | Ratio |
  | --- | --- | --- | --- | --- | --- |
  | gravel 345 | ngp | 100 km/h | 49.4 m, 0.80 g | K 0.5: 47.6 m, 0.83 g | 1.04 |
  | gravel 345 | ngp | 50 km/h | 15.6 m, 0.63 g | K 0.5: 14.7 m, 0.67 g | 1.06 |
  | gravel 345 | experimental-brush | 100 km/h | 50.8 m, 0.78 g | K 0.5: 46.9 m, 0.84 g | 1.08 |
  | gravel 345 | experimental-brush | 50 km/h | 15.6 m, 0.63 g | K 0.5: 13.6 m, 0.72 g | 1.14 |
  | tarmac 568 | ngp | 100 km/h | 35.6 m, 1.11 g | K 0.3: 30.4 m, 1.30 g | 1.17 |
  | tarmac 568 | ngp | 50 km/h | 8.5 m, 1.16 g | K 0.3: 8.1 m, 1.22 g | 1.05 |
  | tarmac 568 | experimental-brush | 100 km/h | 40.1 m, 0.98 g | K 0.15-0.2: 31.2 m, 1.26 g | 1.29 |
  | tarmac 568 | experimental-brush | 50 km/h | 8.7 m, 1.14 g | K 0.2: 7.9 m, 1.24 g | 1.09 |

  Published gravel: 0.70-0.75. Mean slip of the most-slipping wheel on the full pedal, from 100 / 50 km/h: NGP gravel
  0.83 / 0.57 (rim speeds held near 3 m/s, not locked), tarmac 0.99 / 0.54; brush gravel 0.98 / 0.96, tarmac 1.00 /
  0.90.
- Load: the tarmac sensitivity on every surface (same tables): Fy ∝ Fz^0.875-0.886, Fx ∝ Fz^0.98, Cα ∝ Fz^0.33-0.36.

## Open

- Driving it: combined slip (0.72 of NGP's side force at 4° and 25 % slip, inside the measured road data; see
  [Racing and rally targets](#racing-and-rally-targets)) is the largest change a driver would feel. The
  deformation torque M''_z (∝ C_y - C_x) is left out of the aligning moment; no measured combined Mz was checked.
  The autopilot's pace is NGP's within 1 %; a driver's view is the next check.
- σ for a rally tarmac tyre against speed and pressure is unmeasured. Ours does not shorten with pressure (NGP's
  stiffness factor scales C and k alike); racing bicycle tyres relax over almost twice the distance at 3.5 bar as at
  7.5 bar (Meccanica 2023).
- Wet: a published partial-hydroplaning shape (onset near v_p, not quadratic from rest), the film's effect on v_p,
  and a sliding-speed loss for wet roads that fits locked-wheel data.
- A rally tyre's own longitudinal carcass stiffness and damping are unpublished: c_x 450 kN/m is ours (road sets
  288-399 kN/m, measured 163-234: strong that c_x ≥ 300 kN/m and c_x > c_y, weak on the rally value, range 350-550),
  d_x Rill's passenger tyres' (MF-Swift's residual damping is a template default). The lateral carcass stays NGP's
  (σ_y/R 0.65 against the sets' 1.01-1.84).
- Low speed (decision): the brush could run to standstill (Rill's restored transport speed, measured above) in place
  of the 2-3 m/s handover; NGP's sticking-weight damping and hold torque would still act below 2 m/s. LuGre with
  Canudas de Wit's σ_0 181.54 1/m (726 kN/m at 4 kN, 1.6 times c_x) and σ_1 ~1 would be a second bristle over the same
  contact; Rill's restored dynamics already give the stick-slip at standstill LuGre adds, so it was not prototyped.
  Camber thrust from MF6.1's camber stiffness.
- Loose surfaces unmeasured: material 116 in a run (its tyre-class factor, taken as 1), combined slip on gravel and
  snow, and the soil's lateral force above 25 m/s through the whole car. Ploughing (locked 25-30 % shorter on
  gravel) is missing in both physics: the brake test stops 4-14 % longer locked.
