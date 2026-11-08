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
- Inclination (`ngp::inclination::grip_scale`): 1.0 at 0°, 0.95 at +5°, 0.8875 at +10°, 1.05 from -5° to -10°, on the
  cornering friction table only. Camber thrust: `patch_forces` takes 0.1 of the inclination's sine off the lateral
  slip, so C_γ is 0.094-0.098 of Cα, C_γ/Fz 2.61/1.71/1.34 per rad at 2/4/6 kN (lab `camber_pressure`, 1° slip ±1°);
  the step's clamp (`grip::clamp_deflection`) holds no force while the offset slip and the sliding disagree (|α| <
  0.1 |γ|). The wheel's frame has its lateral axis outwards on either side and the inclination negative with the top
  in (Fiesta Rally2 at rest -1.6 to -2.2°, map 85 circle probe), so negative camber pushes inwards.
- Pressure: the stiffness factor (0.5/0.9/1.1/1.2 at 0/1.5/3/5 bar, one table for every tyre) scales Cα, Kx and
  the lateral and torsional carcass alike, and the air pressure the vertical stiffness (patch length). +0.5 bar from
  2.0 at 4 kN: Cα +5.6 %, Kx +4.4 %, σ_y -0.8 % (the ramp's 63 % distance), peak μ 0 (patch length held).
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
| Aligning moment, pure slip | at 4 kN: t0/a 0.42 / 0.51 / 0.36 (t0 at ±0.5°, a the MF-Swift contact length from Q_RA1/2); t0 ∝ Fz^0.93 / 0.93 / 1.01 over 2-6 kN, a ∝ Fz^0.60 / 0.64 / 0.59; Mz peak ∝ Fz^1.64 / 1.90 / 1.90, peak Mz/(μ_y Fz a) 0.19 / 0.26 / 0.20 (brush theory 27/256 = 0.105); Mz peak at 0.30 / 0.37 / 0.35 of the Fy peak's slip angle, Mz back to 0 at 0.78 / 1.73 / 1.19 of it, slightly negative or 0 beyond. Measured Goodyear Wrangler P225/75R15 (48 km/h, ~6 kN): Mz peak 3.7° against the Fy peak at 9-11°, 0 at 13-15°, never negative | TNO 205/60R15 / Continental 225/50R18 (MF-Tool 6.2) / MF6.1, evaluated with MF 6.1's Mz (MF/model; the KT_MF_Tool 245/60R16 template copies TNO's and is left out); Svendenius, PhD Lund 2007, Fig 5.4 (NHTSA data, fig ±0.25°) |
| Aligning moment, combined | Mz(κ)/Mz(0) at 4°, κ ∓0.05, 4 kN: braking 0.62 / 1.29 / 0.40, driving -0.07 / -0.63 / 0.44 (MF 6.1's combined Mz has one fitted term, s·Fx; MF6.1's is 0). The fitted s puts braking above driving; Wong (Goodyear data) says driving raises and braking lowers Mz, as a carcass lateral deflection under Fx does. Conflict: not scored. Low μ: the trail falls to 0 at the full-slide angle, ∝ μ (Hsu, Laws & Gerdes, IEEE TCST 18:4, 2010, on dry gravel; model, no Mz numbers); no open Mz data on snow, ice or gravel | same sets (MF/model); Wong, Theory of Ground Vehicles ch. 1 (secondary, not read) |
| Contact patch, braking | static ink print 205/55R16 at 4821 N, 240 kPa: 147 mm long, 161 mm wide (NGP's tarmac patch at 4.8 kN, 200 kPa: ~0.165 m, 12 % longer, as the lower pressure suggests; the brush's a is half of it); ABS car 100-0 km/h on ten such tyres 36.3-41.7 m, 0.94-1.08 g mean (v²/2s, pedal rise included: a lower bound for a rally tarmac tyre) | Liang et al., Adv. Mech. Eng. 12(12) (2020), doi 10.1177/1687814020981173 (table; g inferred) |

| Convection | Nu ∝ Re^0.8 (NGP 0.78) | West & Limebeer, VSD 60 (2022) |
| Hydroplaning speed | v_p = 6.36 √p km/h (p in kPa) for water deeper than the tread grooves; thin films raise it (Huebner: 26.04 WD^-0.259 mph for WD < 0.1 in); 2 of 8 mm tread costs 17-22 % of v_p; μ ≤ 0.05 at total hydroplaning | Horne & Dreher, NASA TN D-2056 (1963), metric form in Zhang et al., Lubricants 13:76 (2025); Gallaway-Huebner via Ketabdari et al., PLOS ONE (2022); ADAC worn-tyre test (2026) |
| Wet films and speed | road films 0.18 mm in light rain, 0.56-0.85 mm in heavy (drained highways); damp braking μ only slightly above wet; a new tyre brakes from 100 km/h on a watered track at the mean μ it has from 80 (0.98/0.97), at 1.6 mm it keeps 0.69 / 0.52 of new; ABS 80-0 km/h on wet asphalt, EcoContact 6 225/45 R18 at 2.6/2.3 bar, μ* 0.689 new, 0.615 at 3 mm, 0.554 at 1.6 mm (0.89, 0.80 of new; film unstated) | PLOS ONE (2025), Table 2; TN D-1376; Continental Contidrom via AUTO BILD (quoted); Continental test report 2021_100 (2021, table) |
| Wet against slip speed | F(S) = F60 exp((60 - S)/S_p), S_p = 14.2 + 89.7 MPD km/h (smooth test tyre); on the one road with both, S_p 67 predicted against ~220 km/h in locked-wheel data, so not used | ASTM E1960 via two Virginia Tech theses; Rajapakshe et al., Dyna 176 (2012) |
| Low speed | lumped/distributed LuGre: σ_0 181.54 1/m, σ_2 0.0018 s/m, σ_1 ~1 (dynamic fit); MF fades forces below 1 m/s (VXLOW), no standstill | Canudas de Wit et al. (2003), Tables 1, 3; MFeval.jl |
| Longitudinal carcass | c_x at 4 kN (MF-Swift overall stiffness, σ = K/c): TNO 205/60R15 382 kN/m (σx 0.23 m), MF6.1 358 (σx 0.32 m; MFeval 4.0's fit 399), IIT Madras 245/60R16 288; measured 225/50R17 214-234 kN/m (1.9-3.0 bar), P245/70R15 static 163-188 kN/m; c_x/c_y 1.3-2.75 in every set; +5-9 % from 4 to 6 kN | Besselink, Schmeitz & Pacejka, VSD 48 S1 (2010), eqs. 17-20, on the sets; op het Veld, MSc TU/e (2008), Flatplank, Table 4.2; Gent & Walter, The Pneumatic Tire, NHTSA DOT HS 810 561 (2006), Table 9.1 |
| Lateral carcass | road sets σy 0.34-0.75 m at 4 kN (c_y 109-273 kN/m); the two 50-series tyres relax longest, so low profile alone does not shorten σy | same sets; op het Veld; Gent & Walter ch. 8 |
| Carcass damping | MF-Swift residual d_x 690-1060, d_y 300-1000 N·s/m, from the template's 0.002 s on the stiffness (not measured) | same sets |
| Longitudinal past the peak | road MF sets 0.75-0.87 / 0.69-0.82 at κ 0.5 / 1 (4 kN); racing radial MF 6.2 fit 0.68-0.71 / 0.63-0.66 (Dallara AV-21 front, fit range unstated); locked over peak: next row | TNO, Continental, MFeval and IIT sets; JyNing04/Pacejka-tire-model (GitHub) |
| Braked peak and locked against speed | car tyre 195/55R15 on a towed ASTM E274 trailer, braked at constant speed to lock, 32/64/100 km/h, hot rolled (HRA) and stone mastic (SMA) asphalt: dry peak 0.93/0.98/1.12 HRA, 1.13/1.16/1.10 SMA; dry locked 0.77/0.74/0.75 HRA, 0.83/0.77/0.66 SMA; wet (~1 mm water) peak 1.00/1.02/1.00 HRA, 0.99/0.93/0.76 SMA; wet locked 0.64/0.58/0.52 HRA, 0.66/0.48/0.37 SMA. So locked over peak dry 0.73-0.82 / 0.66-0.76 / 0.59-0.67, wet 0.64-0.67 / 0.52-0.57 / 0.49-0.52; locked 64/32 dry 0.93-0.96, 100/32 dry 0.78-0.97, wet 0.56-0.81; peak 100/32 dry 0.97-1.20. Three sport motorcycle tyres within ~0.1 of it except the dry HRA peak (+0.2). A car with ABS from 64 km/h on the dry SMA: 1.11 g (MFDD), PFT peak 1.13, locked 0.68 | Lambourn & Wesley, TRL PPR496 (2010), Figs 6-13 (fig ±0.02), text, Table 1 (table); read in full |
| Carcass against pressure | +0.5 bar: σy -12 %, CFy +9.5 %, CFx +3.5 %; the sets' 2.2-2.6 bar cover rally hot pressures (2.0-2.5 bar) | op het Veld (2008) |
| Camber | camber stiffness Fz (-1.001 + 0.175 dfz)(1 - 0.81 dpi) per rad: 1.07 Fz at 4 kN, 2.6 bar (1.27 at 2.0 bar), 0.047-0.052 of Kyα at 2-6.75 kN; MF 5.2 truck 0.061; brush ε a/(3R) with the camber reduction ε 0.4-0.7 for car tyres (0.031-0.055 at the Fiesta's a/(3R) 0.078) | MF6.1 / MF5.2 sets; Romano et al., Meccanica (2021) (quoted) |
| Pressure, MF 6.1 at 4 kN | +0.5 bar from 2.0-2.6 bar: Kyα -11 to -14 %, Kx -14 to -17 %, c_y +13-16 %, c_x +4.2-4.4 %, σ_y -21 to -26 %, peak μ +1.5-2.5 %; Goodyear: Cα falls with pressure at light loads and rises at heavy ones, peak μ no clear trend | MF6.1 (PPY*, PPX*, PCFY3, PCFX3); Goodyear arXiv 2305.18422 Figs 9-10 |
| Racing slick (rig) | WTCC Yokohama A005 250/660R18: Fy peak at 5-6°, 0.93-0.97 of peak at 10°, 0.89-0.94 at 14°; Cα/Fz 27 /rad new, +6 % at 0.75 mm wear; 1.9 → 2.1 bar: Cα +2.3 %, vertical stiffness 239 → 262 kN/m; grip optimum ≈40 K above track temperature on rig and car; degradation 0.1-0.2 s/lap. Its simulator test plan (Appendix C: baseline, one parameter per stint, fixed lap budget, cockpit A/B toggle) is the template for brush against NGP | Richardson, Chalmers MSc 2017:82 (Cyan Racing), Figs 17, 54, 55, 57 (fig; load not stated), §5.2.4 (quoted) |
| Rubber friction | no static/kinetic pair: μ(v, T) with flash temperature | Persson, arXiv 1007.2713, cond-mat/0605273; Grosch 1963 (secondary) |

Not found in open sources: rally tyre data, racing-radial data beyond the slick row (TTC data is licence-gated), the
full Gallaway 1979 tyre terms, road-sensor damp/wet film thresholds, gravel cut onto tarmac, damp tarmac grip. Those
stay calibrated on stage times.

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
(-0.45 %/K), tread depth (2.6/(d + 1.6)) and NGP's pressure factor, and on a hard surface below NGP's grip 0.55
times grip/0.55 (ice, [Ice](#ice-material-116)); peak Fy ∝ Fz^0.87, Fx ∝ Fz^0.93; friction
against sliding speed g(Vs) = (μ_C + (μ_S - μ_C) exp(-(Vs/7 m/s)³)) exp(-0.008 Vs), μ_C/μ_S 0.8, μ_C lowered
0.25 (1 - exp(-Vs/7 m/s)) at W = 1 (below); NGP's temperature grip kept; dry wear
+10 % at 60 % worn, fading with wetness; partial hydroplaning (below); force dynamics F = c u + d u', (v d + f) u' = f v_N s - v c u per axis (Rill,
Vehicle Dynamics lecture notes, Hochschule Regensburg 2009, 3.4.2 and 4.2.2; f = F/s the steady secant, s made with
v_N = max(v, 0.5 m/s), v itself the transport speed
 as in Rill's eqs. 4.20-4.21, so a locked wheel's deflection
follows the sliding and a stopped one's holds; c_y NGP's carcass and c_x 450 kN/m, times NGP's pressure factor over
its calibration value to the powers 0.7 and 0.6 (c_y as NGP's to the power 1.7; below), at every load (road sets 288-399 kN/m at 4 kN, measured 163-234, c_x/c_y 1.3-2.75); d the mean of
Rill's passenger values, d_x 500/1500 and d_y 250/600 N/(m/s)), combined force and the deflections capped at μ Fz as
Rill caps them; relaxation length (f + v d)/c, the stretched string's C/c at small slip, no other speed term; camber
thrust 0.05 of Cy per unit inclination sine (below). Calibrated
at run time to NGP's lateral peak and slope and longitudinal peak at 4 kN, 15 m/s (no NGP values in code). The dynamics
reach standstill, and the game runs the brush down to it.

Combined slip (2026-11-07): the adhering part of the patch gives -C_i s_i (1 - ψ)² along the stiffness-weighted
slip, the sliding part μ Fz ψ² (3 - 2ψ) against the sliding velocity, μ Fz from ψ = 1 (Svendenius, Tire Modeling and
Friction Estimation, PhD, Lund 2007, eqs. 4.14, 4.17, 4.18-4.20, 4.26-4.28; read in full); the directional μ is the
ellipse along the sliding velocity, his collinear slide force. The aligning moment is his M'_z (eqs. 4.33-4.35):
μ Fz a (1 - ψ)² (θ_y s_y (1 - 4ψ) + 3ψ² s_y/|s|); the deformation torque M''_z, ∝ (C_y - C_x), stays out. Its a is
NGP's patch half length times (Fz/Fz0)^0.35 (2026-11-08, `brush::trail_half_length`): the MF sets' t0/a grows with load
(above), brush theory's a/3 holds at the design load only; forces unchanged (a enters Mz alone). Pure
slip is Pacejka's cubic either way, so the curves, the per-wheel fit and the calibration are unchanged (lab CSVs
identical). Before, the whole force pointed along (C_x s_x, C_y s_y), so at full slide the side share was 1/1.4 of
the sliding velocity's. Pacejka's own ch. 3 text (isotropic bristles, where both directions coincide) was not
reread here: the book is not open.

Friction against sliding speed (2026-11-08): the generalised Stribeck curve (Olsson, Åström, Canudas de Wit et al.,
Eur. J. Control 4, 1998) times Burckhardt's speed factor exp(-c4 Vs) (Kiencke & Nielsen's c4 0.02-0.04 s/m), its four
values fitted to TRL PPR496's car tyre (Published targets): the braked peak holds with speed while the locked
friction falls. Canudas de Wit's own fit (exponent 1/2, v_s 6.57 m/s, μ_C/μ_S 0.52) falls fastest over the peak's
1-3 m/s of sliding, so the braked peak fell 9 % from 32 to 100 km/h and the locked wheel kept 0.76 of it at 100.
Wet (2026-11-08): the wetness W (NGP's `WaterFactor`, at the seam) lowers μ_C by W · 0.25 (1 - exp(-Vs/7 m/s)): water
lubricates the sliding rubber the more the faster it slides (0.18 at 32 km/h, 0.23 at 64, 0.25 at 100), fitted to
TRL's wet locked over peak; the peak barely moves (Vs at the peak is in the curve's flat part). Partial hydroplaning
scales the whole friction by 1 - 0.95 λ: the grooves (d · 8 mm) and the road's texture (MPD 0.7 mm, within VTI
748A's 0.4-2 mm, ours) drain the film (h = W · 0.7 mm) below v_on = 2.1 m/s · drained depth / h, and the lifted share
λ grows linearly from v_on to 5 v_on or Horne's v_p at 200 kPa (90 km/h), the higher (0.05 left, Horne's μ ≤ 0.05).
At W = 1 a new tyre loses nothing to 94 km/h (1.5 % at 100, 14 % at 150); a bald one keeps its friction at rest,
loses from 8 km/h, keeps 0.51 at 50 and is lifted whole at 90. The mean μ of an ABS stop against new, lab: 1.6 mm
from 80 / 100 km/h 0.685 / 0.52 (Contidrom via AUTO BILD 0.69 / 0.52, quoted; Continental's report 2021_100,
EcoContact 6 225/45 R18, 80-0 km/h, 0.80, table), 3 mm from 80 0.90 (report 0.89). The two sources disagree at
1.6 mm. Rejected: grooves alone (2.3 m/s, lifted at 7 v_on) lifted a bald tyre's patch at standstill; a faster
drainage (3 m/s, lifted at 5 v_on: new nil to 123 km/h) put 3 mm at 0.93. Horne's v_p (water deeper than the grooves,
ADAC's 17-22 % lost at 2 of 8 mm) is not the regime of the game's films and is not used; the seam carries no
pressure. The game's tread starts new (`--wear` is the road's), so in game the term only acts above 94 km/h.

Camber and pressure (2026-11-08): the inclination's sine (`ForceInput::inclination_sin`, read-only at the seam) adds
-0.05 sin γ |ωR| to the lateral slip velocity, MF's horizontal shift as an equivalent slip: C_γ = 0.05 Cy at small
slip, gone once the patch slides whole, none at standstill or locked (MF 6.1 0.047-0.052, MF 5.2 0.061, brush ε
a/(3R) 0.031-0.055; NGP 0.1). The carcass stiffens with pressure faster than the slip stiffness laterally and slower
longitudinally: per +0.5 bar c_y +9.5 % (op het Veld) / +13-16 % (MF 6.1), c_x +3.5 % / +4.2-4.4 %; NGP's factor rises
6.9 % from 2.0 bar, so c_y and c_x go as its powers 1.7 and 0.6 (the means), and σ_y = Cy/c_y shortens with pressure.
The slip stiffnesses keep NGP's factor: at 4 kN the sources disagree on Cα's sign (MF 6.1 -11 to -14 %, op het
Veld's σ_y and CFy -4 %, Goodyear +10 % per +20 % near nominal load), so σ_y shortens 5 %, not the sources' 12-26 %.
Kx keeps NGP's +4.4 % against MF 6.1's -15 % (one set). At the calibration pressure (factor 1) nothing moves; at
the Fiesta's 2.0 bar (0.967) c_y is 2.3 % below NGP's and c_x 441 kN/m (435 before).

Ours, not from papers: the friction curve's four values and the wet Coulomb loss and its speed scale (fits to one
tyre's trailer data, two surfaces); the partial hydroplaning's drainage speed 2.1 m/s, its linear ramp to 5 v_on, the
texture depth 0.7 mm within the published range and the 8 mm new groove depth (fits to two braking tests, three tread depths, film unknown: taken as the game's
0.7 mm), and the wear gain's fade with W; leaving out σ's speed term (2026-11-07, user: the measured slope is a passenger tyre's, with its
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
- 2026-11-08, friction against sliding speed fitted to TRL PPR496 (lab `compare`, "braked peak and locked"; Canudas
  de Wit's curve in brackets, NGP after): braked peak at 32/64/100 km/h 1.313/1.298/1.274 (1.374/1.301/1.249; NGP
  flat 1.297), 100/32 0.97 (0.91; TRL 0.97-1.20); locked over peak dry 0.78/0.71/0.67 (0.79/0.77/0.76; NGP 0.78; TRL
  0.73-0.82/0.66-0.76/0.59-0.67); locked 64/32 0.90, 100/32 0.83 (0.91, 0.87; TRL car 0.93-0.96, 0.78-0.97; all four
  tyres 0.86-0.99 at 64). Wet (NGP's wet surface 39, W = 1) 0.64/0.56/0.52 (0.77/0.74/0.73; NGP 0.75; TRL
  0.64-0.67/0.52-0.57/0.49-0.52), the wet peak within 0.5 % of the dry curve's on the same surface. A first fit
  without the speed factor (exponent 2, v_s 10 m/s, μ_C/μ_S 0.65) met the three ratios but took locked 64/32 to 0.82:
  rejected. Elsewhere: peak μ_y falls 6 % from 10 to 30 m/s (10 %); μ_y at 30° over peak 0.833 (0.876; sets
  0.73-0.83); longitudinal at κ 0.5 / 1 0.827/0.726 (0.861/0.786; road sets 0.75-0.87/0.69-0.82); peak slip angle
  9.25° (8.75°; sets 6.5-8.5°, NGP 9.5°), peak κ 0.14 drive / 0.12 brake (0.12/0.10; sets 0.094-0.139); combined
  Fy(κ 0.25)/Fy(0) 0.292/0.347/0.478 at 2/4/8° (0.283/0.333/0.465; Svendenius 0.2-0.3, 0.29-0.34, fig ±0.05);
  lateral over peak at 17° 0.951 (0.937; racing 0.93-0.97). The flatter curve near rest puts the peak later: the
  peak slip angle moves 0.5° further from the sets. Rejected (lab, 2026-11-08): d_x scaled with c_x from Rill's pair
  (2650 N·s/m at 435 kN/m) damps the wheel-tyre mode (settles in 0.02-0.16 s against 0.04-0.38, 5-33 crossings
  against 27-222) and holds on the grade without creep, but shortens the κ-step's 63 % distance at 25 m/s from 0.139
  to 0.104 m, further below the sets' σ_x/R 1.19-1.44 (0.37-0.45 m); MF-Swift's d/c of 0.002 s gives d_x 900, the
  present 1000.
- Meets: combined slip at 4 kN, 15 m/s, Fy(κ ±0.25)/Fy(0) 0.283 at 2°, 0.333 at 4°, 0.465 at 8° (Svendenius's
  measured road tyre 0.2-0.3 and 0.29-0.34; NGP 0.40/0.46/0.61; before 0.20/0.24/0.35), Fy/|F| equal to the sliding
  velocity's share once sliding (0.269 at 4°; NGP 0.35); peak κ 0.12 drive / 0.10 brake at 4 kN (0.18-0.22); dry
  locked over peak at 10 m/s 0.80 (0.70); braking (this bullet before the 2026-11-08 friction curve, above)
  peaks within 4 % of NGP's (17 %); Kx/Fz 22 by the |κ| ≤ 0.04 fit, Kx ∝ Fz^1.14; peak Fx ∝ Fz^0.96; Cα ∝ Fz^0.64;
  Cα -21 % cold to hot and +27 % at 60 % worn; dry peak μ +12 % at 60 % worn; peak Fy ∝ Fz^0.85; σ_y 0.20 m at 4 kN
  (σ/R 0.63; NGP 0.17 m; Pacejka 0.12-0.45 m); with the damping 0.20-0.23 m at 5-30 m/s, 63 % of a 2° step in
  0.17-0.18 m at 10-30 m/s (0.18-0.21 m before).
- New behaviour: peak μ_y falls 6 % from 10 to 30 m/s (1.280/1.264/1.245/1.200, NGP flat; Kiencke & Nielsen's
  c4 0.02-0.04 s/m gives 4-8 % at 10 % slip and 20 m/s; the braked peak 3 % from 32 to 100 km/h).
- Outside the published values (before 2026-11-08 unless given): slide over peak 0.88 lateral at 30° (now 0.833; sets
  0.73-0.83; NGP 0.87); wet locked over peak 0.77 (now 0.64-0.67 at 32 km/h, the wet Coulomb loss); peak slip angle 8.75° (6.5-8.5°); trail
  0 past saturation (NGP 0.004 m).

## In game

- Seam: `rbr_physics::tyre::model` (`ForceInput`, `ForceOutput`, `apply`), asked in `tyre::step` after NGP's sliding
  solve and the slip phase; `rbr_hybrid::ngp::set_tyre_model` per thread, as the engine's. The model's forces become the
  deflection and torsion `solve_forces` turns back into them (in-game probe: `lateral_force` equals the model's to the
  f32). `solve_forces` takes the model's weight w as an argument: below 2 m/s (NGP's sticking weight > 0) its two
  damping factors and the sliding weight's scale of the trail and of the carcass offset torque become `1 - (1 -
  factor)(1 - w)`, so at w = 1 the model's force passes undamped (vector replay: 740 recorded calls with a sticking
  weight, equal to `-deflection · k` to the bit). Left NGP's: the hold torque (below), the rolling resistance torque (×
  sliding weight), the sticking load the drive line takes as each wheel's latch bound (`0.025 Fz` × sticking weight;
  1.02's `CWheel` passes it to `CDrive::SetTwo`, read as `wheel_bias`), NGP's sticking solve itself (still run, then
  replaced by `apply`). With no model set, or w = 0, the code is NGP's: rsf_compare drive2/5 physics PASS 84084/84084,
  exit 0.
- Hold torque (NGP's `hold.rs`, kept): a yaw stiction of the patch about the normal axis, `-900 N·m` times the
  lateral component of a stored in-plane direction, the component capped at `min(0.3, Fz μk / 45000)` × sticking
  weight (the direction slips past it), relaxed at (v - 0.3 m/s) dt; it acts only when the sticking weight is above 0
  (contact speed < 2 m/s). In game 7-13 N·m at the start hold, 10-33 N·m after a stop, 67-90 N·m peak per run. The
  brush has no bore torque, so it adds what the brush lacks rather than fighting it: scaled by (1 - w) (gone), every
  stop time within 0.01 s, reversal share within 0.01, rock-back within 10 mm, four-car finishes within 0.04 s
  (2026-11-07).
- Scope: hard surfaces (no soil) at every speed, down to standstill, weight 1 (2026-11-07, user). On soil loose-v1
  (2026-11-08, [Loose surfaces: loose-v1](#loose-surfaces-loose-v1)).
- Grip: NGP's static friction (surface, wet or dry, times the temperature grip) times the inclination's table scale
  (NGP's acts on its cornering table only, the brush's on both axes: ±1 % per degree to 5°) scales μ_S and μ_C both, so the fitted μ_C/μ_S holds on every surface; NGP's kinetic friction is unused. NGP's
  wet surface values carry the wet level; the surface's `WaterFactor` (`ForceInput::water_factor`, 2026-11-08) lowers
  μ_C (more at faster sliding), fades the dry wear gain and sets the film of the partial hydroplaning (NGP's tread
  remaining its drainage). Retail wet tarmac
  carries it (track-41_M wet 1.0, track-85_M 0.92-0.98), and so do RSF wet stages with a wet `.mat` entry (122, 117:
  1.0); `BlendTable`'s wetness is the same word ([physics.md](physics.md), Scenery). RSF 568 has no wet entry: wet
  there is the fallback material 0 (μs 0.95, class 0, WaterFactor 0; the tarmac tyre's class-0 base factor 0.90 and
  the temperature grip ~0.97 give the tyre's static 0.83), not a wet surface. The dry wear gain and the
  stiffness's temperature, wear and pressure factors are the brush's.
- Calibration per wheel at fit (`rbr_tyre::fit`): NGP's `patch_forces` swept from the wheel's own tables at
  `load_axis[3]` (the 4 kN design load times the car's mass ratio), 15 m/s, unit surface, stiffness factor 1; t_cal is
  the temperature of `RubMyFct`'s best friction. Nothing NGP-derived is committed. The level away from the design
  load is the brush's published load sensitivity (lab `envelope`, Fiesta Rally2 map 41 probe, grip 1.27, 15 m/s:
  1.5-8 kN μ_y +0.1 to +5.7 % over NGP's, μ_x driving -4.2 to +3.3 %; mean over the probe's frames μ_y +1.0 %). NGP's
  level at each load was tried and rejected (2026-11-08): pace -0.01 to +0.06 s, peak Fx load exponent 0.982 against
  the published 0.90-0.95.
- Dynamics: Rill's carcass deflections per wheel, the model's state (snapshots don't carry it; restart and
  touch-down zero it). c_y is NGP's lateral carcass, c_x 450 kN/m, each moved with the pressure factor by its power
  (2026-11-08, [Candidate](#candidate-brush-v1); before, c_x 450 kN/m times the pressure factor over its calibration
  value: 2026-11-07; Golf II 447 kN/m, Fiesta Rally2 435 kN/m, c_x/c_y 1.37/1.07; NGP's torsional over R² 640/796
  kN/m, a tenth of it below 10 m/s), the same at every speed and load;
  trail and phase are the steady brush's.
- Runs below are `experimental-brush` (realistic engine + brush), NGP 7 in brackets; debug traces of the wheel speeds
  (not committed), 2026-11-07, each the last measurement before the 2026-11-08 friction curve:
  - Golf II GTI 16V, track-41_M climb (gear 2, 4-5 m/s, throttle 0.15-0.5, 10-13 s), both fronts over car speed
    mean/max 0.96/1.19 (0.95/1.07), no stall (the brush before the force dynamics: 1.54/3.04 with a stall at 11.72 s).
    Fiesta Rally2 8-20 s 0.97/1.47, Fiat 131 rears 1.08/3.03.
  - track-41_M 3-8 m/s on throttle: share of time the driven wheel runs over 1.1 times the car speed Golf / Fiesta /
    Fiat 0.128/0.102/0.114 (0.099/0.141/0.067); wheel-speed reversals 0.247/0.307/0.215 of steps (0.14/0.17/0.11);
    180-360 Hz power 7.8/60/11.6 (5.4/22/3.1); no instability. c_x 450 kN/m against Cx(Fz0)/(1.44 R) there: |Fx -
    steady Fx|/|steady Fx| 0.20/0.28/0.16 against 0.33/0.36/0.25.
  - Brake from 10 m/s (track-41_M, `--brake-from`), stop after full / 0.35 pedal: Golf 1.04/2.19 s (0.99/2.31),
    Fiesta 0.83/1.63 (0.89/2.30), Fiat 1.05/2.33 (1.17/2.38); full pedal locks in both. NGP's 360 Hz sticking
    alternation is gone: wheel-speed reversals in the second around the stop 0.001-0.047 (NGP up to 0.088). Rock-back
    after the stop 0.12-0.42 m/s peak, 10-30 mm in 2 s (0.16-0.29 m/s, 30-60 mm), ringing on the carcass longer (peaks
    0.012-0.077 m/s at 0.75-1 s against 0.005-0.015, all under 0.02 m/s by 1.5 s; no drift in the last second). Held
    at the start (handbrake, 785-898 rad/s, 7 s): no drift at the trace's 10 mm; the Fiat rocks at about 5 Hz, ±80 N
    a wheel, under 0.01 m/s, gone by 6 s (NGP 0).
  - Four-car 600 s: no NaN, the Golf stalls only after the finish (full brake in fifth from 24 m/s locks its fronts in
    both physics; the realistic engine stalls with the clutch in, `engine-model.md`); finish times in Open. Fiesta
    0-100 on RSF 504 4.38 s (4.52). Full brake from 15 m/s on map 41: 11.79 m (12.03 m; experimental 12.10 m).
    No measurable step cost (100000 steps 31-33 s either way).
- Brake test (2026-11-08, `harness/brake_test.py`, Fiesta Rally2, declutched, full pedal against `--slip
  0.1,0.15,0.2,0.25,0.3`; g through 32 and 64 km/h from the runs from 80 km/h). RSF 568 dry: locked 1.085/0.995 g
  (1.059/1.083), best threshold 1.375/1.421 (1.324/1.404), locked over best 0.79/0.70 (0.80/0.77; TRL car tyre
  0.73-0.82/0.66-0.76), locked 64/32 0.92 (1.02; TRL 0.93-0.96). Whole stop, locked over best distance from
  30/50/80 km/h 1.07/1.17/1.27 (1.20/1.19/1.20). From 100 km/h no locked run is valid in either physics: the locked
  fronts cannot steer and the car leaves the line (3.5 m off on 568, "hit something" on 568, 320, 482). Wet, retail
  track-41_M (WaterFactor 1) from 50 km/h: locked at 32 km/h 0.730 g against best 1.07, 0.68 (NGP 0.817/1.024, 0.80;
  TRL wet 0.64-0.67); whole locked stop from 30 km/h 0.822 g. RSF 568 wet is the fallback material 0 (no wet `.mat`
  entry, above): locked over best 0.79/0.73 at 32/64 km/h. RSF wet tarmac with WaterFactor 1, from 50 km/h, locked
  over best threshold at 32 km/h, dry → wet: 122 0.76 → 0.65 (NGP 0.78 → 0.79), 117 0.79 → 0.67 (NGP 0.79 → 0.77);
  TRL dry 0.73-0.82, wet 0.64-0.67. Wet law of 2026-11-08 (Coulomb loss growing with Vs, partial hydroplaning),
  before → after, locked g at 32 / 64 km/h from 80 km/h: track-41_M 0.767 / 0.575 (locked stop 0.662 → 0.631 g),
  117 0.675 → 0.673 / 0.618 → 0.581, 122 0.608 / 0.599 → locked run hit the edge; from 50 km/h locked 0.740 → 0.727
  (41_M), 0.654 → 0.644 (122), 0.690 → 0.682 (117) g; best threshold within 0.02 g (new tyres, ≤ 80 km/h). NGP from
  80 km/h locked at 64: 0.775 (122), 0.796 (117). Autopilot pace on wet track-41_M: Fiesta Rally2 371.74 → 371.44 s
  (NGP 381.43), Celica ST185 375.93 → 375.93 s (NGP 385.34).
- The Fiesta's 0.35-pedal stop (1.63 s against NGP's 2.30) is the engine's, not the tyre's: realistic engine with
  NGP's tyre (`experimental`) 1.65 s; under NGP the car holds 2.93-2.96 m/s for 0.75 s in second gear at 123-125 rad/s
  until the autopilot shifts down, the realistic engine falls to 107 rad/s and keeps braking (2026-11-07).
- Ramped 40 m circle (map 85, `--circle -260,120,40 --circle-ramp 0.25`): NGP and the brush alone hold the same
  steering to the same limit, 8.6 m/s², then both run wide at full lock; the autopilot's steering steps (0.008) hide
  any linear-range difference. Steady state matches by the fit; the differences are in transients, combined slip,
  speed, load and temperature (lab tables above).

### A/B protocol (a driver, tarmac)

After Richardson's simulator plan (Chalmers 2017, Appendix C: baseline stint, one parameter per stint, a fixed lap
budget, feedback logged per stint), for `experimental` (NGP's tyre) against `experimental-brush`. Both run the
realistic engine, so only the tyre differs; against `ngp` the engine differs too. No in-game toggle: two command
lines.

1. Setup: run `rerbr` from a terminal, Settings physics "ReRBR experimental", start the stage once; stderr prints
   `launcher: VIEWER ARGS`. Save that line twice, one with `--physics experimental` and one with `--physics
   experimental-brush`, each with `--result-out FILE` appended (experimental keeps no records). A second person names
   the two X and Y at random and keeps the key until the end.
2. Fixed: car Ford Fiesta Rally2 (`Ford_Fiesta_Rally2_ngp7`; a second session on a rear-drive car, the Fiat 131),
   setup, tyre, weather and wetness; stages RSF 568 SS Graciosa I (dry), retail track-41_M dry, then track-41_M with
   `--wetness wet` (wet tarmac carries `WaterFactor`, so the brush's wet term acts there; RSF 568 has no wet
   surface, so use a stage with one, e.g. RSF 122).
3. Session per stage: one familiarisation run on X; stints X, Y, X, Y of 3 timed runs each (the repeat catches
   learning); the same run budget for every stint; nothing else changes between stints.
4. Log per run: stage time and splits (`--result-out`), the replay the viewer records, and the driver's 1-5 marks and
   one line each on turn-in, mid-corner balance, braking and lock-up, trail braking, traction out of slow corners,
   and how predictable the limit is. Unblind after the last stint.
5. Alongside, the tarmac scorecard rows (`harness/tyre_scorecard.py`): braked peak and locked against speed dry and
   wet (TRL PPR496), combined slip at 2-8° (Svendenius), peak slip angle and κ, over peak at 30° and κ 0.5/1, σ_y.
   A preference for one physics where the rows differ points at the property behind it.

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
85 as above (step 8000, wheel 1). Ice (material 116): map 85 iced whole (`--material-as 116`, [Ice](#ice-material-116)),
the circle's wheel 1 at step 8000, the tarmac tyre at 0.0949/0.0902. "Sweep": pure `patch_forces` at 15 m/s; "step": `ngp_transient
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
| Peak μ_y | gravel ≲ 0.67 WRC wet loose (CW, quoted; μ inferred), 0.55 wet crushed (TR1011, quoted); packed snow 0.35-0.43, loose 0.28-0.33 (TR0713, table); ice 0.15-0.17 (TR0713, table) | 0.828 sweep, 0.854 step | 0.851, 0.857 | 0.094 | 1.262, 1.266 |
| Over tarmac | gravel 50-60 % (CW, quoted) | 0.66 | 0.67 | 0.08 | 1 |
| Peak slip angle | gravel ~12° (CW, quoted), 12.1° (Velenis, MF), dirt 16° (ChassisSim, model input); packed snow 9-17°, loose 16-18° (TR0713, table); ice 4.5-5.1° (TR0713, table) | 13.5° (95 % at 9°); step 14° | 11° (95 % at 7.75°); step 11.25° | 0.75° (95 % at 0.5°) | 9.5° (6.75°) |
| μ_y at 30° over peak | gravel 0.63 (CW, quoted; ratio inferred), 0.85 (Velenis, MF); snow no fall to ~17° (TR0713, quoted); ice 0.59 at 20° (TR0713, fig ±0.005) | 0.956; step 0.985, at 25 m/s rising to 30° (0.897) | 0.955; step 0.997, at 25 m/s rising (0.926) | 0.966 at 20°, 0.950 | 0.865 |
| Peak κ | gravel none open ("0.4-0.7", snippet); snow still rising at 0.30 (Surkutwar, fig ±0.01); ice 0.05 (Ružinskas, quoted) | 0.24 (2 kN 0.16) | 0.22 (0.12-0.30 at 2-6 kN) | 0.02 | 0.16 |
| μ_x at κ 1 over peak | gravel 0.75 (Velenis, MF); snow no fall to 0.30 (Surkutwar, fig); ice ~0.5 at 0.45 (Ružinskas, fig ±0.005) | 0.82 sweep and step | 0.85 | 0.92 | 0.78 |
| Ploughing: locked against peak | gravel: locked stops 25-30 % shorter than ABS, decel 1.25-1.30× (SAE 1999-01-1287, table; ratio inferred); car braking 0.50-0.60 wet crushed (TR1011, table) | locked 0.82 of peak (step 0.82, 0.79 at 25 m/s); game, brake test on 345 declutched from 100 / 50 km/h: locked 0.77 / 0.63 g, 10 / 17 % longer than the best threshold braking (K 0.5, 0.85 / 0.73 g) | 0.85 | 0.93 | 0.78; RSF 568 locked 1.09 / 1.06 g, 21 / 19 % longer than threshold at K 0.3 (1.31 / 1.26 g) |
| Peak μ_y at 2/4/6 kN (Fy ∝ Fz^n) | snow no clear loss at 4.6-11 kN (TR0713, inferred, weak); soil lateral coefficient falls slightly at 1-4 kN (El-Razaz review, quoted); gravel none | 0.889/0.828/0.774 (0.875); step 0.913/0.854/0.797 | 0.902/0.851/0.788 (0.881); step 0.928 (rising)/0.857/0.795 | 0.099/0.094/0.087 (0.891) | 1.353/1.262/1.179 (0.877) |
| Peak μ_x at 2/4/6 kN (Fx ∝ Fz^n) | none | 0.864/0.849/0.845 (0.982) | 0.877/0.870/0.858 (0.979) | 0.101/0.100/0.101 (snow tyre, earlier sweep); n 0.993 on the tarmac tyre | 1.309/1.292/1.286 (0.984) |
| Cα/Fz at 4 kN; Cα ∝ Fz^m | initial slope packed snow ~10/rad, ice ~4.6/rad (findings, inferred from figs); gravel none | 9.1/rad; 0.325 | 10.5/rad; 0.338 | 3.7/rad (0-2° fit, past the peak: the tarmac tyre's 17.6 on 0.095); 0.876 | 17.6/rad; 0.359 |

Where NGP sits:

- Level: gravel and snow are one surface to NGP, 0.83-0.86 at 4 kN (0.66-0.67 of tarmac): gravel 1.3-1.5× the
  published peaks, snow about twice. Ice (116) is 0.094 on the tarmac tyre, below the measured 0.15-0.17.
- Shape: the peak slip angles of gravel (13.5°) and snow (11°) fall inside the published ranges, because the gravel
  and snow kinds are softer (Cα/Fz 9-10.5 against 17.6); ice peaks at 0.75° against 4-5°, its stiffness being the
  tarmac tyre's (NGP's stiffness is the tyre's on every surface). Past the peak every loose curve is flat (0.96 of peak at 30° in the sweep) and the soil force makes it
  flatter or rising, more with speed: like the snow data, flatter than the gravel data (0.63-0.85), and ice does not
  fall (0.97 against 0.59).
- Ploughing: none longitudinally (the soil's longitudinal force is zero); a locked wheel keeps 0.82 of the peak on
  gravel, so it stops longer than one at peak slip (10-17 % in the declutched brake test below), where the published
  locked wheel stops 25-30 % shorter.
- Brake test (2026-11-07, `harness/brake_test.py`, Fiesta Rally2, `--flat-out --top-speed 80`, full pedal against
  `--brake-slip` K 0.1-0.7; ratio = full-pedal distance / best threshold distance). Declutched (`--brake-declutch
  --clutch-help off`, no shifts): the full pedal locks (mean rim speed over 3 m/s 0-0.47 m/s, slip 0.99-1.00), the
  engine runs free of the gear in every locked run. In gear (clutch help on, the plain `--brake-from`), the autopilot
  downshifts and on gravel the idling engine (124-128 rad/s, clutch engaged) turns the braked wheels at ~3 m/s, so
  that full pedal is brake against engine, not locked. On gravel experimental-brush was NGP's tyre with the realistic
  engine (the brush was off on soil before loose-v1); declutched the two agree within 1 %. The force rises with
  slip to K 0.5 on gravel, 0.2-0.3 on tarmac, and no run stops shorter locked.

  | Surface | Physics | From | Locked (declutched) | Best threshold (declutched) | Ratio | Ratio in gear |
  | --- | --- | --- | --- | --- | --- | --- |
  | gravel 345 | ngp | 100 km/h | 51.1 m, 0.77 g | K 0.5: 46.5 m, 0.85 g | 1.10 | 1.04 |
  | gravel 345 | ngp | 50 km/h | 15.7 m, 0.63 g | K 0.5: 13.5 m, 0.73 g | 1.17 | 1.06 |
  | gravel 345 | experimental-brush | 100 km/h | 51.2 m, 0.77 g | K 0.5: 46.7 m, 0.84 g | 1.10 | 1.08 |
  | gravel 345 | experimental-brush | 50 km/h | 15.6 m, 0.63 g | K 0.5: 13.4 m, 0.74 g | 1.17 | 1.14 |
  | tarmac 568 | ngp | 100 km/h | 36.2 m, 1.09 g | K 0.3: 30.0 m, 1.31 g | 1.21 | 1.17 |
  | tarmac 568 | ngp | 50 km/h | 9.3 m, 1.06 g | K 0.3: 7.8 m, 1.26 g | 1.19 | 1.05 |
  | tarmac 568 | experimental-brush (before 2026-11-08) | 100 km/h | 40.8 m, 0.97 g | K 0.15-0.2: 31.0 m, 1.27 g | 1.32 | 1.29 |
  | tarmac 568 | experimental-brush (before 2026-11-08) | 50 km/h | 8.8 m, 1.12 g | K 0.2-0.3: 7.8 m, 1.26 g | 1.13 | 1.09 |

  Published gravel: 0.70-0.75. In gear the full-pedal stops are 0-9 % shorter (the engine keeps the wheels
  turning) and the best threshold stops 0-9 % longer than declutched. Earlier run, full brake from 100 km/h on 345:
  0.72 g, setup not compared.

- Load: the tarmac sensitivity on every surface (same tables): Fy ∝ Fz^0.875-0.886, Fx ∝ Fz^0.98, Cα ∝ Fz^0.33-0.36.

### Ice (material 116)

- Where: the RSF stages 565/614 (Legazpi-Gabiria winter, tarmac), 3 % of each `.mat` entry's cells, long stretches
  of the second half (614: all four wheels on 116 at steps 137000-140000 of an autopilot run); 563 Toivonvaara's
  damp-new entry blends it 0.77 into soil 103 (snow, NGP's soil path). 124/156's wet entries list it, but 60000
  autopilot steps on them met none. Scan: `.mat` class bytes; runs: `--tyre-probe` `block_tags` (2026-11-08).
- Tyre and factor: 565/614 run the tarmac tyre (type 0, the stage's surface). 116's surface class is 5 (`materials`
  example's `class` column, `+0x148`), whose base factor is 0.95 on the tarmac tyre and 1.0 on the gravel and snow
  tyres (probe `tyre_classes`). In the run, static 0.0896 = 0.10 · 0.95 · temperature grip 0.946 (cold tyre, 316 K).
  The scorecard's "factor taken as 1" is replaced by this run.
- Tooling: `rbr-standalone --material-as 116` (autopilot, test only) gives every material 116's tyre values
  (`ngp::layout::stage::materials_as`: friction, soil, bump, water, rolling resistance and the class; collision values
  and flags stay each material's), from the start or with `--brake-from` from the brake onset: no 565/614 stretch is
  flat or straight enough to stop on from 50 km/h (~60-100 m), and the iced launch spins on the brush. Probe: map 85
  circle iced; brake: RSF 504's start straight, the run-up on its tarmac. On 568 a locked car drifts 2 m off the line
  and the game resets it (outside assistance, code 2); 504 rises 6.6 % over the stop (345 falls 2.4 %), so the decel
  is taken on the flat (car_pace `decel_flat`, less g · rise / distance): ngp 0.096 g at K 0.1 on 504 (raw 0.162);
  by the same correction 0.091 on 568 and 0.079 on 345.
- The brush on ice: below NGP's grip 0.55 its slip stiffnesses scale with grip/0.55 (`brush::low_mu_stiffness_factor`;
  no tarmac material reaches it: the lowest hard surface, 0.68 class 2, is 0.57 on a cold tarmac tyre; a snow tyre on
  wet low-grip tarmac can reach 0.43). Evidence: Svendenius Table 7.1 brush fits at 4 kN, winter tyre C0x 25-27.6 on
  asphalt (μ 1.0-1.2; wet stiffer than dry), 16 on wet basalt (μ 0.27), 6.25 on ice (μ 0.078): knee 0.55 with an
  exponent 0.76 through all three; summer tyre 0.09 of asphalt's on ice (exponent 1.2); 1 is ours, between. Before,
  the tarmac tyre's stiffness (Cα/Fz 17.6) at μ 0.095 slid whole at 0.75-1°; the peak level stays NGP's (0.0948
  against 0.0938 at 4 kN).
- Trade-off: with NGP's level (0.094, the data 0.15-0.18) the peak slip angle and the initial slope cannot both
  match (the brush slides whole at tan α = 3 μ/(C/Fz)): exponent 0.76 gives 2.75° and a 0-2° slope 3.2 /rad, 1.0 gives
  4.25-4.5° and 2.4-2.5 /rad, 1.2 gives 5.75° and 1.9 /rad (lab, the probed wheel, 4 kN, on the friction curve before the TRL refit; 1.0 gives 4.5° and 2.43 /rad on the
  current one). The peak angle was kept; the
  model's own Cx/Fz at zero slip is about 4.3 (Svendenius 3.6-6.7), the scorecard's κ 0.02-0.04 fit (2.5) lies past the
  linear part on ice.

## Loose surfaces: loose-v1

`rbr_tyre::loose` (in development, `--physics experimental-brush` only): the car model (`car.rs`) takes it on every
soil wheel (`soil_thickness > 0`: gravel and snow) in place of NGP's tyre; NGP's soil lateral force (∝ v_lat²) stays on
top (`model::apply`), the carcass dynamics (`brush::step_dynamic`) are the brush's and keep their state across a change
between soil and hard surface. Lab: `loose` example (a probe frame on soil, NGP's sweep against it).

- Model: the brush's patch (`brush::patch`, combined slip unchanged) with a flat friction (no fall with sliding speed;
  on gravel the shear law below in place of the brush's cubic),
  plus a berm: from a braking slip of `BERM_FROM` 0.8 (or the longitudinal full slide, |sx| = 3 μ_x Fz/Cx, if later)
  to lock the braking force rises by a smoothstep to `PLOUGH` 1.3 times the free rolling peak, against the sliding
  velocity (with the friction); between the full slide and 0.8 it stays at the peak. The braking slip takes the speed
  floored at 0.5 m/s, so the berm fades out at standstill. No berm when driving.
- Evidence: lateral saturation without a fall to 38° (Coutermarsh & Shoop, SAE 2010-01-0374, Fig. 15); locked over
  ABS decel 1.25 laden / 1.30 light (NHTSA DOT HS 808 875, table), the berm "ahead of a locked tyre" (quoted); the
  locked tyre's side force PLOUGH μ sin α against Coutermarsh's 0.72 sin α over a free rolling 0.55 (1.31 sin α, Fig.
  16); their locked μ_x 0.85 is 1.5 times the lateral max, above PLOUGH.
- Gravel's shape (2026-11-08, `loose::shear`): on NGP's gravel materials (surface class 1, `Gravel_*` ids 1-51 and 153-199, by
  their share in the wheel's two-material blend; the class reaches the model through `ForceInput::surface_classes`,
  read by the host from the wheel's last `blend_tags`, `wheel::layout::surface_classes`) the patch's force is μ Fz (1 -
  exp(-u)) along the theoretical slip, u = |(sx/s0x, sy/s0)|, s0 = tan 6.3°, s0x keeping the brush's Cx/Cy 1.4 at the
  axes' levels: Janosi-Hanamoto's soil shear law with the shear displacement the slip times a fixed length (form via
  Senatore & Sandu, J. Terramech. 48 (2011); Wong's K values not read). α0 from Coutermarsh & Shoop Fig. 15 (fig
  ±0.004): pea stone 6.28° (rms 0.0012 over 1-38°; the brush's cubic 0.0165, the law integrated over a uniform patch
  0.0098), wet gravel 9.7° (0.0015); in K terms, about the half patch times tan α0 (~11 mm at a 0.2 m patch,
  inferred). One α0 for every gravel (dry, damp, wet: wet gravel's 9.7° is a different soil, silty sand, not NGP's
  wetness). The slope follows the level (μ Fz/tan α0: 6.0 /rad at 6.7 kN, NGP 6.6); the level is approached, never
  peaked (0.995 at 30°). Snow (class 3: SnowOnGravel/SnowOnIce 101-103, 117-119, snowwalls), grass (5), rock (4) and
  rough (0) keep the flat brush: packed snow's own α0 is 3.6° (Fig. 8, rms 0.0042), which at NGP's snow level doubles
  NGP's slope (snow-calpha-per-fz 8-12 would miss), while the brush at NGP's slope passes α90 (9.0 in 7-9°).
- Fit: NGP's free rolling peaks μ_y, μ_x (driving) at each load of the wheel's axis on the unit surface
  (`fit::loose`), linear between, times NGP's grip; the flat brush's peak is μ Fz, so the level needs no solve. The
  cornering stiffness is re-solved under the flat friction to NGP's slope (0-2°, 4 kN); Cx = 1.4 Cy as the brush's.
- Ours: the smoothstep and its start at 0.8 (no intermediate slips are published; chosen on the trade-off below: a
  berm from the full slide holds the wheels off lock at the Fiesta's full-pedal brake torque), no berm when driving
  (the data are braked; a spinning tyre's sinkage resistance is unmeasured), PLOUGH independent of the soil's
  thickness (data on 5 cm layers, NGP's 9-30 mm) and of speed (no trend at 40-80 km/h), NGP's soil lateral force kept
  (NGP's lateral ploughing: 3-4 % of the peak at 30° and 15 m/s, ~10 % at 25 m/s, out of reach of Coutermarsh's 1-1.5
  m/s).
- Carcass at the limit (`brush::step_dynamic`, both surfaces): sliding at the limit, the deflections take the steady
  force's direction. Before, a locked wheel's deflection integrated the sliding velocity weighted by c_x/c_y (450 kN/m
  against NGP's lateral carcass), so its side force at 20° was 0.197 against the steady 0.37; in game it moved RSF 111
  by 0.02 s and the gravel locked decel by 0.001 g.

Lab, RSF 345 frame (Fiesta Rally2, wheel 1, gravel 17, static 0.847 after the temperature grip, 4 kN, 15 m/s; NGP /
loose-v1; NGP's soil force left out of both):

| Property | Published | NGP | loose-v1 |
| --- | --- | --- | --- |
| μ_y/peak at 5/10/20/30° | pea stone 0.56/0.81/0.96/1.00, wet gravel 0.40/0.64/0.87/0.96 | 0.74/0.98/0.99/0.96 | 0.73/0.98/1.00/1.00 |
| μ_y at 40° over peak | ~1 (no fall to 38°) | 0.913 | 1.000 |
| Peak μ_y, slip angle | 0.54-0.55 (road AT tyre) | 0.829 at 13.75° | 0.829 at 16.5° |
| μ_x at κ 0.05 / driving peak | none | 0.335 / 0.850 (κ 0.24) | 0.504 / 0.850 (κ ~0.3) |
| Braking μ_x over peak at κ 0.3/0.5/0.7/0.9/1 | locked 1.25-1.30 (car) | 0.99/0.95/0.88/0.83/0.82 | 1.00/1.00/1.00/1.15/1.30 |
| Locked μ_y over free rolling peak at 5/10/20/35° | 1.31 sin α: 0.11/0.23/0.45/0.75 | 0.18/0.33/0.46/0.54 | 0.12/0.23/0.46/0.76 |
| Peaks μ_y / μ_x at 2/4/6 kN | | 0.889/0.864, 0.829/0.850, 0.774/0.846 | 0.888/0.882, 0.829/0.850, 0.774/0.846 |

The table is before gravel's shear law (2026-11-08): it gives μ_y/peak 0.55/0.80/0.96/0.995 at 5/10/20/30° and 1 - 5e-4
at 40° (scorecard probe, wheel 0). The 2 kN μ_x is 2 % above NGP's: the fit sweeps at stiffness factor 1, the frame's pressure moves NGP's light-load
longitudinal peak. Snow (map 10, SnowOnGravel_Shallow 101, wheel 1): the same shape, peaks 0.834/0.854 against NGP's
0.844/0.864 (1 % low, the same cause), not tuned.

In game (2026-11-08, Fiesta Rally2, declutched `harness/brake_test.py`, slip targets 0.2-0.7; ngp traces
byte-identical with and without loose-v1; experimental-brush before loose-v1 was NGP's tyre on gravel, within 1 % of
ngp declutched; RSF 111 287.42 s, 112 437.28 s, circle 0.725 g). Trade-off for the berm (local builds, the committed
default marked; "best" is the best single-pedal slip target; wheel slips FL/FR/RL/RR of the full-pedal run above 3
m/s):

| Variant | 50 km/h: full / best g, full ÷ best, ÷ K 0.2 | wheel slips | 100 km/h: full / best g, full ÷ best, ÷ K 0.2 | wheel slips | snow full g | circle g | RSF 111 / 112 s |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ngp | 0.628 / 0.732 (K 0.5), 0.86, 1.18 | 1.00 all | 0.771 / 0.847 (K 0.5), 0.91, 1.30 | 0.99 | 0.791 | 0.571 | 290.32 / 441.72 |
| PLOUGH 1.0 (flat) | 0.773 / 0.767 (K 0.7), 1.01, 1.19 | 1.00 all | 0.914 / 0.883 (K 0.7), 1.04, 1.24 | 0.91-0.93 | 0.924 | 0.609 | 286.45 / 435.50 |
| PLOUGH 1.15 from full slide | 0.873 / 0.829, 1.05, 1.34 | 0.93-0.95 | 0.976 / 0.926, 1.05, 1.32 | 0.87/0.62/0.86/0.61 | 1.017 | 0.670 | 286.42 / 434.96 |
| PLOUGH 1.3 from full slide | 0.911 / 0.876, 1.04, 1.40 | 0.70-0.73 | 0.995 / 0.961, 1.04, 1.34 | 0.80/0.49/0.79/0.47 | 1.072 | 0.605 | 286.38 / 435.33 |
| **PLOUGH 1.3 from 0.8** | 0.892 / 0.768, 1.16, 1.37 | 0.93-0.94 | 0.979 / 0.884, 1.11, 1.32 | 0.90/0.81/0.89/0.80 | 1.059 | 0.609 | 286.43 / 435.45 |
| peaks × 0.75, PLOUGH 1.3 from full slide | 0.742 / 0.684, 1.08, 1.42 | 1.00 all | 0.872 / 0.805, 1.08, 1.42 | 0.99 | 0.874 | 0.547 | 288.00 / 441.69 |
| peaks × 0.75, PLOUGH 1.3 from 0.8 | 0.734 / 0.570 (K 0.5), 1.29, 1.40 | 1.00 all | 0.860 / 0.696, 1.24, 1.40 | 1.00 all | 0.866 | no latch | 288.10 / 479.29 |
| peaks × 0.75, flat | 0.554 / 0.570 (K 0.5), 0.97, 1.06 | 1.00 all | 0.696 / 0.695, 1.00, 1.13 | 1.00 all | 0.691 | no latch | 288.11 / 481.93 |

Published: locked 0.47-0.66 g (gravel), 0.31-0.35 g (snow); locked over ABS 1.12-1.59 per car; circle 0.43-0.7 g.

- Default: PLOUGH 1.3 from 0.8. It keeps the published ratio's 1.3 and is the only NGP-level variant whose full pedal
  locks the wheels (the left/right difference at 100 km/h is the brake equilibrium, 0.80-0.90); against the best
  single-pedal target it gives 1.16 / 1.11, against K 0.2 1.37 / 1.32. From the full slide the berm's force past
  the peak exceeds the brake torque: the wheels settle at slip 0.5-0.8, one side locks, the autopilot steers against
  the yaw (full lock below 7 m/s at 100 km/h), and the deepest target is the best (1.04).
- K 0.2 is not an ABS: the one pedal holds the most-slipping wheel at 0.2 and under-brakes the rest, so NGP, with no
  berm, also scores 1.18 / 1.30 locked over K 0.2. The scorecard's locked-over-ABS rows now take the per-wheel ABS
  at K 0.15 ([A per-wheel ABS in the brake test](#a-per-wheel-abs-in-the-brake-test)); K 0.2 and the best target
  stay as context.
- Level: every NGP-level variant brakes 0.77-1.0 g on gravel and 0.92-1.07 g on snow, against 0.47-0.66 / 0.31-0.35 g:
  NGP's loose peak is already 1.3-1.5 times the published gravel peaks. Peaks × 0.75 (gravel peak ≈ 0.62, inside
  0.54-0.67) with the berm brakes 0.73-0.87 g and locks every wheel, the ratio 1.24-1.29 against the best target; flat
  at 0.75 brakes 0.55-0.70 g, inside the band, with no ploughing. The cost: the circle at 0.547 g or no latch (the
  autopilot never settles on the circle), RSF 111 +0.6 %, RSF 112 +1.4 % (berm from the full slide) to +10 % (berm
  from 0.8 or flat: splits 107.7 / 292.0 against 105.9 / 273.8, time lost in the second section; the autopilot is
  not tuned to the grip). The grip level is the user's decision (2026-11-08: stays NGP's for now; the gravel
  braking and road-tyre rows are lower bounds for a rally tyre, [Car-level targets](#car-level-targets-on-loose-surfaces)).
- Standstill (berm from 0.8, Fiesta Rally2, RSF 345 gravel): no creep, no movement after 1 s. After a stop from 50
  km/h, the peak speed per half second is 0.292/0.015/0.006/0 m/s at full pedal (NGP 0.293/0.014/0.001/0) and
  0.296/0.029/0.003/0 at a 0.35 pedal (NGP 0.296/0.043/0.001). From the full slide it rang to 0.061/0.019.
- Snow (lab, map 10 wheel 1, static 0.852, 4 kN, 15 m/s): μ_y over peak at 5/10/20/30/40° is NGP
  0.800/0.998/0.980/0.955/0.922 and loose-v1 0.721/0.972/1.000/1.000/1.000. Peak μ_y: NGP 0.844 at 11°, loose-v1
  0.834 at 17.5°. Braking over the driving peak at κ -0.5/-0.9/-1: loose-v1 1.000/1.150/1.300, NGP 0.959/0.854/0.844.
  Locked side force over peak at 5/10/20°: loose-v1 0.116/0.231/0.454, NGP 0.152/0.288/0.442.
- Soil↔hard in game (RSF 469/473, tarmac and gravel mixed, soil 0-30 mm, Fiesta Rally2, the autopilot): there are
  9-22 changes per run above 2 m/s, most at road edges or bumps, where Fz jumps. Where Fz is steady (< 20 %), μ steps
  by -0.057 to +0.070 at the change: loose-v1's flat friction against the brush's fall with sliding speed, plus the
  surface's grip. At the changes |ΔF|/Fz stays within the run's spread (p99 0.18 elsewhere). No oscillation: 0-1
  reversals over 0.01 in 40 steps, except in sideways crashes at 10-18 kN, which come from the terrain. Stage times:
  469 eb 274.26 s against ngp 285.09 s. On 473 eb finished in 294.43 s; ngp ended on its roof at 257 m (the autopilot).
- Robustness (2026-11-08, the autopilot to the finish, 5 cars × 5 stages, ngp and eb): Golf II (FWD), 131 Abarth and
  M3 E30 (RWD), Fiesta Rally2 and Celica ST185 (AWD), on RSF 104 and 111 (gravel), track-11_O (snow under NGP 7:
  SnowOnGravel 101-103, probe 2026-11-08), track-10_O (snow) and RSF 389 (snow). 24/25 finish under both. No NaN. Recoveries are alike (track-10_O: eb 6, ngp 5). eb is
  faster everywhere: 1.1-2.3 % on RSF and 1.8-4.3 % on retail. Split on track-10/11_O with `experimental` (NGP's
  tyre, ReRBR's engine), M3 / 131 / Golf: ngp → experimental -3.2/-3.0, -3.6/-3.0, -3.0/-4.1 %, the engine model;
  experimental → eb -0.5/-0.2, +1.9/-0.2, -1.0/-0.2 %, loose-v1. The gap is the engine's, as on tarmac.
- The one DNF is the Golf on RSF 104 under eb, stuck at 4488 m. It brakes at a 0.8 pedal at full lock (4450 m, 12
  m/s). The fronts creep at 0.4-0.9 m/s (slip 0.93-0.96) and the engine in fourth follows them down to 0 and stalls.
  The autopilot has no restart. ngp and experimental lock both fronts at the same corner to 0 m/s at about 113 rad/s.
  The engine separates, idles at 82-89 rad/s, and both finish (282.12 / 277.27 s). Traced with a debug build
  (2026-11-08):
  - The clutch help works as ported: `vcu::step::step_basic` sends slip 0 down to 99.5 rad/s, then ramps to 1 (open)
    at 89 rad/s.
  - Even so, the engine speed stays equal to the gear's speed of the creeping fronts down to about 5 rad/s. NGP's
    driveline stays clutch-locked, its engine speed following the drive's, while the driven wheels turn. With the
    wheels at 0 it lets go.
  - Under ReRBR's engine the dragged engine gives -15 to -0.3 N·m. NGP's stall history (`stall_speed`) then marks it
    stalled, and the model's torque stays 0.
  - The creep is loose-v1's flat friction, not the berm: no fall from the peak to sliding, so a brake torque near the
    peak does not run the wheel into lock (NGP's force falls past its peak). A berm built only at slip ≥ 0.99, none
    at the corner, creeps the same (0.6-0.7 m/s, engine 24-27 rad/s) and DNFs at the same step.
  - Neither the port nor the engine model is wrong. The fix belongs to the autopilot: declutch, or go to neutral,
    when the driven wheels lock under braking, and restart a stalled engine. A driver declutches.
  - Fixed by the autopilot (2026-11-08): in a race under NGP it selects neutral while the braked wheels drag the
    engine under 89 rad/s for 4 steps (the clutch pedal does nothing while the clutch help sets the clutch) and
    restarts a stalled engine on the ignition. The Golf now finishes RSF 104 under eb in 275.34 s. In the sweep, all
    27 ngp runs keep their times exactly. eb moves on the Golf track-10_O (+1.21 s, +0.31 %, neutral at a front lock in
    first) and the Celica RSF 104 (+0.06 s); the other 25 runs are unchanged.
- Not adopted (2026-11-08, prototype): a berm built over sliding distance (per-wheel state growing over the patch
  length 2a of sliding past the ramp's start, washing out over 5 cm of rolling). Its steady state s / (s + 3.6(1 - s))
  (0.79 at slip 0.93 against the ramp's 0.72) behaves like the ramp: the Golf on RSF 104 DNFs at the same step (washout
  5 cm, 1 cm, or building only from slip 0.99); Fiesta 345 from 50 km/h, full pedal 0.870-0.889 g against the ramp's
  0.892 g (locked over best 1.13-1.16), no variant locks every wheel; Golf and Celica stage times on RSF 104/111/389
  within 0.05 s. It fixes nothing and adds a state.
- Scorecard (experimental-brush, [Scorecard](#scorecard), 2026-11-08 after gravel's shear law; main 4bb4c042 52/103,
  this 61/103): passes the pea-stone μ_y over max at 2.5/5/10/15° 0.327/0.547/0.798/0.912 (main 0.40/0.68/0.95/1.00),
  the rally ratios at 5/10/20° 0.547/0.798/0.964 (0.68/0.95/1.00), α90 14.25° (8.5), the slope at 6.7 kN 6.04 /rad
  (7.37), gravel peak κ 0.58 (0.23), circle 0.664 g (0.620), locked over ABS K 0.15 1.53 / 1.34 (1.38 / 1.20), locked
  over lateral max 1.458, locked over traction 1.305, gravel over tarmac braking 0.748, the snow rows (unchanged);
  newly misses gravel-peak-slip-angle (11-16°, Croft-White's ~12° saturation, inferred): the curve has no peak, so the
  0-40° argmax is 40°, against the pea-stone curve still rising at 38° (that row and pea-peak-alpha ≥ 15 conflict).
  Still misses locked decel 0.882 / 0.908 g (0.47-0.66), locked side force at 30° (0.33-0.39), μ_y(30°) over peak 0.995
  (rally 0.63-0.85, against the pea stone's ≥ 0.97), snow peak κ, and every absolute gravel and snow level (NGP's).
- Gravel's shear law in game (2026-11-08, main 4bb4c042 → this, experimental-brush; ngp unchanged by construction):
  Fiesta Rally2 RSF 345 from 50 km/h declutched, full pedal 0.892 → 0.882 g, wheel slip 0.943 → 0.946 (the wheels still
  do not lock fully, as before); ABS K 0.15 0.645 → 0.576 g (its wheels' mean slip 0.08-0.13: below the slower rise's
  plateau), locked over ABS 1.38 → 1.53. r40 circle 0.620 → 0.664 g. Standstill after the stop, peak speed per half
  second: full pedal 0.345/0.055/0.016/0.006/0.002/0 → 0.347/0.031/0.008/0 m/s, 0.35 pedal 0.346/0.029/0.008/0 →
  0.347/0.031/0.008/0; no drift after 1 s either. Soil↔hard RSF 469 / 473 (Fiesta): 274.52 → 274.59 s; 473 main
  rolled at 22 s (autopilot), now finishes 274.40 s; wheel-speed reversals (> 0.01 m/s, over 2 m/s) 0.023-0.034 of
  steps on 469 both, 0.022-0.037 on 473 (main 0.05-0.12 in its crash). Stage times, ngp / main eb / this eb (s):

  | Car | RSF 104 | RSF 111 | RSF 389 (snow) | track-11_O (snow) |
  | --- | --- | --- | --- | --- |
  | Fiesta Rally2 | 268.55 / 263.82 / 265.68 | 290.32 / 286.61 / 287.06 | 178.68 / 176.75 / 176.75 | 648.88 / 636.60 / 636.60 |
  | Golf II GTI 16V | 282.12 / DNF / DNF | 302.62 / 297.04 / 298.19 | 187.14 / 183.42 / 183.42 | 681.71 / 652.44 / 652.44 |
  | Celica ST185 | 267.54 / 263.27 / 264.82 | 289.08 / 285.66 / 286.38 | 179.41 / 177.40 / 177.40 | 652.11 / 640.27 / 640.27 |

  On gravel the law costs 0.45-1.85 s (0.2-0.7 %), eb stays 1.0-1.3 % ahead of ngp; snow is byte-for-byte main's. The
  Golf's RSF 104 DNF is the autopilot's (stuck at 4488 m in both, above). These runs used α0 6.28°; 6.3° moves the
  scorecard rows by ≤ 0.002.
- Not adopted (2026-11-08): the exponential approach on the brush's own stiffness (NGP's slope, u = 3ψ, the brush's
  adhesion/sliding split): pea stone 0.38/0.61/0.85/0.95 at 6.7 kN and 0.98 at 20°, all four pea rows still miss; one
  isotropic s0 on both axes (the soil's K alike, Cx/Cy = μx/μy): locked over ABS K 0.15 1.67 at 50 km/h (band ≤ 1.59),
  1.44 at 80, everything else as adopted.

### The soil's depth: NGP's materials and the surface across the road

2026-11-08. NGP's soil materials (lab `materials`, NGP 7.4.778's stage init, dry; damp / wet take μ × 0.925 / 0.85,
WaterFactor 0.25 / 1 and SoilDensity +100 / +200; names from `lsp_keys.rs`):

| Ids (dry) | Materials | μs / μk | soil μs / μk | Thickness | SinkFactor | Class |
| --- | --- | --- | --- | --- | --- | --- |
| 17, 21, 49 | Gravel Fine / Medium / Coarse Shallow | 0.900 / 0.895 / 0.8925, μk 0.75 | 0.855 / 0.664 (17) | 9 / 9 / 15 mm | 1.5-2e-7 | 1 |
| 9, 13, 41 | Gravel Fine / Medium / Coarse Deep | 0.885 / 0.880 / 0.8775 | 0.832 / 0.646 (9) | 17.5 / 17.5 / 22.5 mm | 2e-7 | 1 |
| 1, 5, 33 | Gravel Fine / Medium / Coarse Deeper | 0.8725 / 0.8675 / 0.865 | 0.811 / 0.631 (1) | 25 / 25 / 30 mm | 2.5e-7 | 1 |
| 153, 177-199 | `Gravel_Fine_Shallow_Dry_Clone`, `US2_` (fine) and `BR2_` (coarse) Shallow / Deep / Deeper | within 0.015 of the base names | | the base names' (US2 9/17.5/25, BR2 15/22.5/30 mm) | 1.5-2.5e-7 | 1 |
| 133, 141, 149, 161 | Dirt Hard / Medium / Soft, Dirt_Hard clone | 0.84 / 0.80 / 0.76 | | 9 (damp 12, wet 15) / 15 / 20 mm | 2-3e-7 | 4 |
| 56, 97-115 | Rock_Small; Grass Hard / Medium / Soft | 0.795; 0.65 / 0.675 / 0.70 | | 20; 20 / 25 / 30 mm | 3-8e-7 | 4; 5 |
| 101-103, 117-119, 62, 88 | SnowOnGravel / SnowOnIce Shallow / Medium / Deep; Snowwall, its bottom | 0.86-0.84 / 0.91-0.87; 0.87 | | 13 / 20 / 30 mm; 30 mm | 1.5-2e-7 | 3 |
| 129-139 | Rough, VeryRough | 0.65-0.70 | | 30 mm | 2-4e-7 | 0 |

Gravel's level falls 3 % from shallow to deeper and its RollingResistance rises 0.060 / 0.065 / 0.070. Class 1 is
every gravel above (1-51, 153-155, 177-199), so the shear law covers RSF 104's BR2 gravels. In NGP the thickness
already acts: thin layers are sunk through (the hard values), 30 mm layers blend toward the soil friction, and the
soil's lateral force grows with the sink ([Loose surfaces: NGP](#loose-surfaces-ngp-against-published-data)).

Under the path (Fiesta Rally2 autopilot under `ngp`, `.mat` new dry; lab `surface_across` every 2 m at 0, ±1, ±2,
±3 m from the car's path, thickness the blend's, linear in its weight as the probe's `surface:` line shows to 5e-7;
the ±1 m mean is within 0.4 mm of the probe's wheel mean on every stage; RSF 345 its first 10.2 km of 19.5):

| Stage | Under the line (share) | Line mean, p10-p90 | Gravel only ±2 / ±3 m | 100 m means along |
| --- | --- | --- | --- | --- |
| RSF 104 | BR2 Deeper 34 %, Deep 27 %, Coarse Shallow 18 %, BR2 Shallow 13 % | 22.5 mm, 15-29.5 | 23.1-23.4 / 24.4-24.5 mm (gravel 20-23 % of ±3 m, the rest grass 30 mm) | 17.7-28.6 mm |
| RSF 111 | Fine Shallow 86 %, Medium Shallow 14 % | 9.0 mm, 9-9 | 9.0 / 9.8-9.9 mm | 9.0-11.0 mm |
| RSF 345 | Fine Deep 63 %, Fine Shallow 37 % | 14.3 mm, 9-17.5 | 17.8-19.7 / 21.6-22.4 mm (Fine Deeper; gravel 35-47 % of ±3 m) | 9.0-17.5 mm |
| RSF 389 (snow) | SnowOnIce Shallow 100 % | 13 mm | 13 / 13 mm (all materials) | 13 mm |
| track-11_O (snow) | SnowOnGravel Medium 76 %, Shallow 21 % | 18.6 mm, 14.5-20 | 19.5-24.1 / 23.4-28.8 mm (all materials; Snowwall_Bottom 30 mm) | 13.3-21.7 mm |

- Across: the stages make the verge deeper than the line (gravel at ±3 m 2-8 mm deeper on 104/345, 1 mm on 111),
  and RSF 345 changes the depth along the road (Fine Deep and Fine Shallow alternate, 9 against 17.5 mm).
- Passes: nothing changes in a run. NGP's sheet lookup reads the `.mat` cells loaded at the start
  (`ngp::layout::material::sample_slot`); the only variant is the entry the wear word picks at the load (new, normal,
  worn; a season draws normal or worn, `rbr_gameplay::weather`). Worn moves the line on 104 22.5 → 19.4 mm (wheel
  tracks ±1 m 17.5 mm, centre 19.5, verge 24), on 111 9.0 → 13.6 mm (deeper), on track-11_O 18.6 → 14.6 mm; 345's
  entries are one map. The direction is each stage author's, not a scraped line.
- Published depth dependence (earlier research, TR-10-11, SAE 2010-01-0374, a 2026-11-08 search): no depth law for the
  shape (α0, initial slope) or the berm on gravel. What exists is the level: pea stone traction 0.58-0.68 "depending
  upon the depth of stone", higher where little stone (Coutermarsh & Shoop, quoted, depth not measured); braking
  0.50 → 0.57 → 0.60 as the ruts deepen 1.9 → 3.4 cm (TR-10-11 Table 3, table); the scraped line 0.68-0.74 against
  the windrow 0.56-0.59 (Fig 13, quoted; ratio 0.80 inferred); deep pea stone over 20 cm rolling resistance 0.16
  (quoted). A rigid wheel in dense sand over a rigid base: drawbar pull at high slip non-monotonic in the layer's
  thickness (J. Terramech. 1994, "Travelling performance of a wheel on a finite thickness ground", abstract only;
  longitudinal, not a tyre). Snow's 0.5 cm of fresh snow halving the initial slope (SAE 2010-01-0374) is snow.
- Decision (2026-11-08): loose-v1 stays independent of the thickness. The stages do put 9-30 mm under the wheels, but the
  data give only a level, and the level is NGP's. A windrow less grippy than the line (published 0.80; NGP's deeper
  against shallow 0.97) would be a grip-level change: the user's decision.

## Car-level targets on loose surfaces

Measured whole-car behaviour (and per-tyre forces where open) on gravel, snow and ice, to judge a loose-surface tyre
model against data rather than feel. Research only (2026-11-07, second round); the full findings, sources, digitised
traces and scripts are kept outside the repository (not in git). Labels as above; g from a stop is v²/2s (inferred, a
lower bound: it includes the pedal rise). Every braking series is road cars on road tyres; none is a rally tyre.
The scorecard takes those gravel levels as lower bounds for a rally tyre (decision 2026-11-08): a raced line alone
grips about 1.2 times the loose windrow (TR-10-11 Table 3), and a WRC car's 0.6-0.7 g lateral (CW) times the measured
locked/lateral 1.3-1.5 gives 0.8-1.0 g locked (inferred). CW's WRC tyre μ_y ≲ 0.67, the one rally-tyre level, stays an
upper bound. NGP gives a car's tyre no era: `Performance_NGP` is 1.0 on 38 of the 42 RSF cars (Group B and Group 4
alike), 0.95 on the two R1 cars, 0.985 front / 1.015 rear on both 911s.

| Target | Value and conditions | Source (precision) | How ReRBR reproduces it (run, car, metric) | Tooling missing |
| --- | --- | --- | --- | --- |
| Locked-wheel braking on gravel | 0.47-0.59 g, 9 light vehicles, 56 km/h, 5 cm #617 crushed limestone, shortest of 3 stops; 0.59-0.66 g, 7 SUVs, pickups and cars, 50 km/h, freshly graded silty sandy gravel; 0.53-0.64, Polo, 40-80 km/h compacted; no speed trend over 40-80 km/h. NGP (declutched brake test, RSF 345): 0.63 g from 50 km/h, 0.77 g from 100 km/h | NHTSA DOT HS 808 875 Tables A1/A2 (table; g inferred); Macnabb et al. ESV 98-S2-W-36 Table 4 (table, accelerometer); Proctor-Parker SATC 2019 Table 1 (table; g inferred) | `harness/brake_test.py` on RSF 345 from 40-80 km/h, Fiesta Rally2; the locked run's mean decel | none |
| Locked over ABS on gravel | decel ratio 1.25 laden / 1.30 light, per car 1.12-1.42; 1.21-1.59 per car. Same method on wet Jennite (hard, SN 30/10): 0.54-0.62. NGP: tyre locked 0.82 of peak; brake test locked over best threshold decel 0.86 (50 km/h) / 0.91 (100 km/h) | NHTSA Tables A1/A2 (table; ratio inferred, reproduces SAE 1999-01-1287 Table 2); Macnabb Table 4 (table; ratio inferred) | `harness/brake_test.py`: declutched locked against `--brake-abs` K 0.15 (per-wheel ABS) on the same stretch; ratio of mean decel | none |
| Gravel against tarmac, braking | same car locked gravel / ABS asphalt 0.69-0.84; tarmac reference by the same method: ABS wet asphalt (SN 85/65) 0.69-0.84 g from 80 km/h, dry concrete 0.69-0.88 g from 97 km/h. NGP, declutched, same speeds: locked gravel (345) / locked tarmac (568) 0.71 at 100 km/h, 0.59 at 50; locked gravel / best threshold tarmac 0.59 / 0.50 (rally tyres, the published pairs road tyres) | Macnabb Tables 3-4 (table; ratio inferred); NHTSA Tables A1/A2 (table; g inferred) | `harness/brake_test.py` at one speed on 345 and 568, same car; ratio | none |
| Car lateral g on gravel | 0.43-0.54 g, CIV, rally drivers, hard turn at ~48 km/h, pea stone (CIV lateral coefficient 0.35-0.41 on the same passes); Explorer 0.405 / 0.536 g at 48 / 64 km/h entry; WRC 0.6-0.7 g (CW; the WRC+ telemetry's 1 s p90 is 0.73-0.92 g, row below). ReRBR, ramp 0.4, ngp / experimental-brush: Fiesta Rally2 0.571 / 0.664 g, i20 Coupe WRC 2020 0.587 / 0.739, Fiesta WRC 2019 0.749 / 0.423 (single runs; ramp 0.25 gives 0.36-0.47 g, an inward weave of the autopilot, [Real stage telemetry](#real-stage-telemetry-wrc-gravel)) | TR-10-11 Fig 12 (fig ±0.01), Table 6 (table); Croft-White (quoted) | `--circle 1051,-166,40 --circle-ramp 0.4` on 345, Fiesta Rally2; steady lateral g before it runs wide | none |
| Trail-brake by an expert on gravel | Group A Impreza STI (AWD, 340 bhp), track gravel, 80-90° corner: 77 → 23 km/h over 1.05-4.05 s (0.51 g mean, 0.66 g peak over 0.5 s), rear-axle slip angle 42° at 4 s, back to 0 by 5 s; pendulum turn −29° → +63°, 0.59 g peak | Velenis et al. MED 2007 Figs 3, 5 (fig ±1 km/h, ±1°; digitised by pixel; inputs normalised); no data release found | RSF Impreza GC8 555 GrpA on a gravel 80-90° corner, inputs of the same shape; speed and rear-axle slip angle traces | a payload writer from an input CSV, assumed steering, brake and throttle scales, a matching corner; shape check only |
| Snow braking | road winter tyres 0.33-0.45 g (50-0, 35-5, 25-5 km/h, ABS); locked 0.31-0.35 g; trailer μx,max 0.46-0.57, μy,max 0.47-0.56 at 30 km/h, no clear fall past the peak. NGP: tyre peak 0.85; car not measured | Auto Bild 2025, Vi Bilägare 2024-25 (table, secondary); VTI R875 Tabell 55, 60, 62 (table); Sokolovskij 2007 (table), SAE 960652 (quoted) | `--brake-from 13.9` on map 10 (`track-10_O`), Fiesta Rally2; mean decel | none (rally snow tyres are studded: the level is a lower bound) |
| Ice braking and cornering | car 50-0 km/h: studded 0.23-0.30 g, studless 0.18-0.22 g; best three at −2/−7/−13 °C 0.24/0.28/0.31 g studded, 0.17/0.22/0.26 g studless. Rig, smooth ice −3 °C, 30 km/h, 4.2 kN: μx peak/locked 0.25/0.22 studded, 0.21/0.17 Nordic; μy peak 0.22 at ~5° / 0.19 at 2-3°, at 20° 0.19 / 0.13. NGP: material 116 μ 0.10 (0.095 on the tarmac tyre), tyre peak at 0.75°; car from 50 km/h on the flat 0.096 g best threshold, 0.095 g locked (ratio 0.99); experimental-brush 0.100 / 0.081 g (0.81) | Tekniikan Maailma 18/2025 (table, secondary via tyrereviews); VTI R875 Tabell 46, 48, Figur 3 (table; means inferred; fig ±0.01) | `brake_test.py` on RSF 504 with `-- --material-as 116` (iced from the brake onset; `decel_flat`); map 85 circle iced (`--circle -260,120,40 --material-as 116`) | a flat iced straight (504 rises 6.6 %); no car lateral g target on ice |
| Rally braking and lateral g, real stage | WRC 2017-21 cars, 18 drives on two gravel stages (medians, range): 16 longest braking events 0.60 g (0.41-0.90) from the speed, peak 0.5 s 0.83-1.06 g; lateral 1 s p50 / p90 / p99 0.40 / 0.89 / 1.25 g Finland, 0.30 / 0.74 / 1.02 g Wales; g-g p95 of \|lat\| about 1.0 g from 30 to 150 km/h. ReRBR autopilot, RSF 524 Hassi, 3 cars × ngp / experimental-brush: braking 0.40-0.44 g, lateral p50 / p90 / p99 0.24 / 0.38 / 0.45 g, the autopilot's plan (4 / 5 m/s²) | WRC+ merged telemetry via RallyDataJunkie, Rally Finland 2021 SS7, Wales Rally GB 2017 SS10 (10 Hz; units and axes inferred, lateral checked against GPS) | `harness/rally_gg.py run` and `stats` (`data/rally-telemetry-gravel.csv`) | a driver at the car's limit: the autopilot plans 0.41 g braking and 0.51 g lateral, so the stage runs measure it, not the tyre (weak) |
| Surface change under traffic | braking coefficient 0.50 → 0.57 → 0.60 over three passes in one track; scraped line 0.68-0.74, loose windrow 0.56-0.59 | TR-10-11 Table 3 (table), Fig 13 (quoted) | none | a surface that changes with passes: NGP has none (only the `.mat` wear entry at the load, [the soil's depth](#the-soils-depth-ngps-materials-and-the-surface-across-the-road)) |
| Per-tyre lateral curve on gravel | road AT tyre 235/75R15, 6.7 kN, 1-1.5 m/s, towed lock-to-lock sweep. Free rolling, averaged curve μ_y at 5/10/20/30°: pea stone 0.30/0.44/0.52/0.54, wet gravel 0.22/0.35/0.48/0.53, still rising at 38-39° (max 0.54-0.55; per run 0.57 ±0.01 / 0.60 ±0.02); 90 % of max at 14.5° / 21°, initial slope 3-5 /rad. Front locked (pea stone): μx 0.85 at 0°, 1.5× the lateral max and 1.25-1.47× the traction (0.58-0.68); side force 0.72 sin α (0.23 at 20°, 0.36 at 30°). Circle breakout at 8.5 m/s: front LC 0.55 / 0.43 | Coutermarsh & Shoop SAE 2010-01-0374 Figs 14-17 (fig ±0.004 μ, locked ±0.02; ratios inferred) | sweep on kind 4: shape ratios and Cα/Fz (NGP: 95 % at 9°, 9.1 /rad, locked 0.82 of peak) | none |
| Per-tyre lateral curves on snow and ice | same rig: max μ_y packed snow 0.40 ±0.03, + 0.5 cm fresh 0.375, wet 0.31, remixed 0.24, fresh 0.22, ice 0.18 ±0.015. Shapes: packed snow 90 % by 7-9°, flat to 26-33° (0.91-0.99 of max); fresh snow (5/15/22 cm) rising to the curve end at 15-19°, deeper lower; remixed snow linear to 33°, no peak; wet snow peaks at 10° and falls to 0.72 at 31°; ice peaks at 4-5°, 0.85 at 16°. Initial slope packed 6-9 /rad, halved by 0.5 cm fresh. Longitudinal (VTI truck, 50 km/h, brush fit): winter tyre on snow μ 0.40-0.41 at 2-6 kN, Cx/Fz 9-14 (24-28 on asphalt), rising to 40 % slip; studded 0.51; ice μ 0.08-0.10, studded 0.16, Cx/Fz 6-7 | SAE 2010-01-0374 Figs 7-8 (fig ±0.006); Svendenius, PhD Lund 2007, Table 7.1 (table) | sweep on kind 6 and 116 | none |
| Rally studs on ice | protrusion 5.5-7 mm, 18-20 studs in contact; no μ or g open; road-stud fits end at 1.8 mm | press (quoted); VTI meddelande 923 (quoted) | none | data |

Ranking:

1. Strong and testable now: locked over ABS on gravel (1.25-1.30, every car, two programmes; the opposite sign on
   wet Jennite by the same method), where NGP's brake test gives 0.86-0.91, the clearest miss; the locked-wheel level
   on gravel (three measured series give 0.47-0.66 g at 40-80 km/h; NGP 0.63 g from 50 km/h, at the top, on a rally
   tyre); the gravel/tarmac braking ratio; and the gravel circle (0.43-0.7 g). Snow braking (0.33-0.45 g) also runs
   now, as a lower bound.
2. Ice (VTI rig and magazine data are solid): runs on material 116 now (`--material-as 116`); the level is NGP's
   (0.10 against 0.18-0.30 g), so the shape and ratio rows carry the comparison.
3. Weak or shape only: the Velenis trail-brake (normalised inputs, one corner), the WRC telemetry statistics (18
   drives, inferred units; the autopilot drives under its own plan, not at the limit).
4. Out of reach: per-tyre gravel curves at rally speed or load (only 1-1.5 m/s, one load), rally studs (no data),
   surface change with passes.

## Real stage telemetry (WRC, gravel)

Research (2026-11-08); raw files outside the repository, derived figures per drive in `data/rally-telemetry-gravel.csv`,
`harness/rally_gg.py` (definitions in its docstring).

- Data: WRC+ merged telemetry as RallyDataJunkie republished it (the code MIT, the data with no stated licence; no
  feed after 2021). `df_telemetrymergeddata_SS<n>_<driver>.csv`: 10 Hz (15 % of steps 0.2 s), rows newest first;
  `speed` km/h (integrates to the GPS path within 0.3 %), `accx` lateral +left and `accy` longitudinal
  +deceleration, both read as 0.01 g (inferred), `brk`/`throttle` 0-100, `rpm`, `gear`, `kms` 0.1 km, GPS. Single
  samples reach 10 g (a 3-sample median takes them out). Used: Rally Finland 2021 SS7 Kakaristo-Hassi 1 (18.17 km;
  Yaris WRC ×5, i20 Coupe WRC, Fiesta WRC ×2) and Wales Rally GB 2017 SS10 Gartheiniog 1 (12.61 km; C3, Fiesta, Yaris,
  i20 WRC, 10 drives); cars from the entry lists (memory, not in the data). Monte-Carlo 2018 exists (tarmac/snow);
  no open Rally2/R5 file with acceleration (WRC2's are 6.4 s GPS points).
- Unit checks over 1 s: `accx` against v times the GPS course's rate, slope 0.77-0.85 (Finland) / 0.59-0.75 (Wales),
  r 0.85-0.92: the 0.01 g lateral unit holds within the GPS's smoothing. `-accy` against the speed's rate, slope
  0.49-0.74, and it lags the speed by about 0.5 s: the longitudinal channel reads low or filtered (inferred), so
  braking is taken from the speed.
- Figures (medians over the drives, range): 16 longest braking events 0.60 g from the speed (0.41-0.73 Finland,
  0.44-0.90 Wales), 0.54 / 0.53 g from `accy`, peak 0.5 s mean 1.00 / 0.83 g. Lateral 1 s p50 / p90 / p99 0.40 /
  0.89 / 1.25 g Finland (mean 125 km/h), 0.30 / 0.74 / 1.02 g Wales (97 km/h). g-g p95 over 0.5 s means, \|lat\|
  / decel, per 30-60/60-90/90-120/120-150/150+ km/h: Finland 0.96/1.04/1.07/1.14/1.04 and 0.78/1.11/1.10/1.00/0.47;
  Wales 0.90/0.95/0.94/0.90/0.73 and 0.85/0.93/0.89/0.64/0.45. The lateral level is flat with speed from 30 km/h
  (where aero is small) to 150 km/h, so most of it is not downforce; specific force includes road camber and roll.
- ReRBR (`rally_gg.py run`, RSF 524 Hassi, 7.78 km, Finnish gravel; trace columns `glong glat`, the specific force on
  the body axes): i20 Coupe WRC 2020, Fiesta WRC 2019, Fiesta Rally2, ngp / experimental-brush: 346-353 s, mean 80-82
  km/h, top 108-111 (the autopilot's 30 m/s), braking 16 longest 0.40-0.44 g, lateral p50 / p90 / p99 0.23-0.25 /
  0.37-0.40 / 0.43-0.48 g, g-g p95 \|lat\| 0.44-0.51 / 0.40-0.44 / 0.32-0.35 g. These are the autopilot's plan
  (`BRAKING` 4 m/s², `LATERAL` 5 m/s²), equal across cars and physics: the stage runs cannot rank the tyre until a
  driver runs at the limit. Against the telemetry the real WRC drives brake 1.4 times and corner 1.9-2.3 times (p90)
  harder.
- The r40 circle (Fiesta Rally2, RSF 345, ramp 0.4, `--tyre-probe-every 10`, 1 s means; experimental-brush, ngp the
  same pattern): through the hold the front axle works harder (front α 7-19°, Fy/Fz 0.47-0.62; rear α 3-9°, 0.28-0.51):
  the car runs wide on the front. At the break (24-26 s) both axles go to 21-27° with throttle 0.3-0.4, |F|/Fz 0.69-0.86
  per wheel. Loads: inner 1.8-2.1 kN, outer 5.0-5.3 kN on both axles (static 3.8 front, 3.1 rear). The four wheels'
  combined Σ\|F\|/ΣFz is 0.75 against the loads' own peaks Σμ(Fz)Fz/ΣFz ≈ 0.82 (peaks 0.89/0.83/0.77 at 2/4/6 kN):
  the load spread costs about 0.02 (0.84 at the mean 3.5 kN), the tyres reach about 0.9 of their peak, and the driving force takes the rest, so
  Σ\|Fy\|/ΣFz is 0.65-0.66 and `glat` 0.71-0.75 g. A 0.83 tyre at 4 kN is consistent with 0.663 g at the car. At
  ramp 0.25 the "slide" is the autopilot weaving inward (4 m inside, steering crossing over) at 0.44 g, not grip: the
  circle row reads the car and the controller together.

## Rolling resistance

- What NGP applies: a torque on the wheel, `-(patch_length · RollingResistance / 2) · Fz · sliding_weight · sgn(ω)`
  (`tyre::forces::rolling_torque`; kept under the brush), the material's `RollingResistance`: tarmac fine (37) 0.04,
  gravel (17 / 9 / 1) 0.060 / 0.065 / 0.070, snow (101) 0.08, ice (116) 0.02 (`--tyre-probe` `surface:`). Over the
  radius that is 0.0089 Fz on tarmac (patch 0.14-0.16 m, R 0.315 m), 0.020-0.028 on gravel (the patch lengthens as
  the tyre sinks 9-25 mm), 0.027 on snow, 0.0044 on ice. The soil's drag (`soil_op`) is 0 rolling straight.
- Car level (`harness/coast_test.py`: the brake test's declutched run with a pedal rounding to 0, the coast from 60
  to 10 km/h cut into 10 m pieces, each piece's energy loss less the grade, corners over 0.5 m/s² left out, the drag
  ρ Cd A / 2m of the car file held, the median), Fiesta Rally2, ngp / experimental-brush (2026-11-08): tarmac RSF 568
  0.0066 / 0.0065, gravel RSF 345 0.0180 / 0.0179, snow map 10 0.024 / 0.023, ice (568 on 116) 0.0025 / 0.0022;
  spread of the pieces 0.0004-0.0018. Less the drag, tarmac stays flat 0.006-0.0075 from 5 to 26 m/s, and a free
  a + b v² fit from 100 km/h gives b 0.30e-3 /m against the car file's 0.32e-3 (body mass, ρ at 90 m): the drag is
  the file's. The car's level sits 0.002-0.003 g under the torque's Fz share on every surface (0.74 of it on
  tarmac, 0.57 on ice); not traced.
- Published (`data/tyre-targets.csv`, `coast:SURFACE:crr`): road cars coasting on Swedish roads Cr = 0.00912 +
  2.1e-5 IRI v + 0.00172 MPD (VTI 748A 2012), 0.010-0.0135 smooth to rough; EU label C1 class A ≤ 0.0065 on the
  ISO 28580 drum; gravel 0.016 (Team O'Neil dry) and 0.018 (MPG, ≤ 10 cm pea stone), 0.16 in pea stone over 20 cm
  (ERDC/CRREL TR-10-11, text), rolled gravel 0.02 (Bosch); fresh snow 11-22 cm, snow alone, leading tyre
  0.024-0.127, trailing 0.017 (CRREL 95-7 Table 3); hard-packed snow near the hard surface's (VTI 971A); no car
  coast-down on ice found (the ice band is the VTI model with no texture, inferred). Gravel and snow PASS, tarmac
  0.0066 just under the inferred 0.007 floor, ice 0.0025 against 0.007-0.013 (weak data: NGP's 116 halves tarmac's
  RollingResistance).

## Scorecard

`harness/tyre_scorecard.py` scores the physics against `data/tyre-targets.csv` (one row per target from the tables
above, `id,surface,level,metric,conditions,low,high,unit,source,precision,how`; inclusive bands, an empty bound open;
both bounds empty: context, printed CTX and not scored; `how` `none` where nothing measures it). One scored row per
measured quantity: where sources disagree on the conditions the run has, the other row is context (the studded-tyre ice
decel: the Fiesta's tarmac tyre on 116 is unstudded; fresh and remixed snow: map 10 is SnowOnGravel_Shallow 101, a
shallow layer on a firm base, the rig's packed snow; Velenis's MF locked over peak, a model's value, against the
measured locked over traction). Fiesta Rally2 throughout.

Run (release builds of `rbr-standalone` and the lab's `scorecard` example first):
`.venv/bin/python -I -B harness/tyre_scorecard.py [--physics ngp,experimental-brush] [--quick] [--only REGEX] [--jobs
N] [--work DIR] [--json OUT]`; the full run takes 1.5 min at 16 jobs on 32 threads, `--quick` (tyre rows and the
50 km/h gravel and tarmac brake rows) less. `--compare A.json B.json` prints the rows that moved, each file scored on
the bands it stored, a changed band flagged `(band)`. The `how` keys:

- `tyre:SURFACE:KEY`: the lab's `scorecard` example on a probed wheel (tarmac map 85 circle wheel 1 at step 8000;
  gravel RSF 345 circle wheel 0 at step 6000; snow map 10 wheel 0 at step 6000; ice map 85's circle iced whole with
  `--material-as 116`, wheel 1 at step 8000, the tarmac tyre as on RSF 565/614). It builds the profile's
  `rbr_tyre::model` from the wheel's tables as the game does (the brush, and loose-v1 on soil, under
  experimental-brush) and lets its dynamics settle 2 s at each slip; where the model hands back (`ngp`) NGP's
  `patch_forces` answer. A new model name needs its line in `TYRE_MODEL`. Steady forces at 15 m/s, flat, no sinking
  and no soil force (the sweep, not `ngp_transient`'s whole step); the pea-stone rows (measured at 1-1.5 m/s) are
  taken at 15 m/s and 6.7 kN. Braking peaks (`peak_mu_x_brake_*`, `peak_kappa_*`, `mu_x_rel_k*_brake_*`,
  `mu_x_locked_rel_*`, the longitudinal load exponent) are the maximum over a braking slip of 0 to loose-v1's
  `BERM_FROM` 0.8, the free rolling side's peak before the berm; locked is κ -1. NGP's and the brush's braking peaks
  lie below 0.8 on all four probes (their keys equal the 0-1 maximum's, lab on the same probes).
- `tyre-ratio:A/B:KEY`: surface A's key over B's.
- `brake:SURFACE:KMH:locked_g|best_g|ratio|ratio_kK|ratio_absK`: `brake_test.py`'s declutched runs (gravel RSF 345,
  tarmac RSF 568, snow map 10, ice RSF 504 iced from the brake onset, its decel on the flat, slip targets 0.05-0.3),
  full pedal against slip targets 0.1-0.7; ratio = locked decel / best threshold decel, ratio_kK locked over the
  target K's (one pedal), ratio_absK locked over the per-wheel ABS at K (`--brake-abs`, a run per K the rows name).
  The locked-over-ABS rows (gravel 50/80, tarmac 50) take ABS K 0.15; the K 0.2 and best-target ratios are context.
- `coast:SURFACE:crr|crr_free`: `coast_test.py` from 60 km/h (tarmac RSF 568, gravel RSF 345, snow map 10, ice RSF
  568 on 116 from the lift; [Rolling resistance](#rolling-resistance)).
- `brake-ratio:A/B:KMH:X/Y`, `circle:gravel:lat_g` (`car_pace.analyse_circle` on the gravel probe run, 40000 steps,
  ramp 0.4). The circle's slide speed also depends on the engine holding the ramp: read that row as car, not tyre.

Counts (2026-11-08, `tyre/review-fixes`, full run; 112 rows, 97 scored): ngp 31/97, experimental-brush 48/97 PASS. It
reproduces the tables above: gravel 345 ngp locked 0.628 g from 50 km/h, circle 0.571 g (loose-v1 0.663); NGP gravel
peak μ_y 0.828 (13.75° on the 0.25° grid against the sweep's 13.5°, a flat top), μ_y(30°)/peak 0.956, μ_x(κ 1)/peak
0.82; tarmac brush peak κ 0.11, Fy(κ 0.25)/Fy(0) 0.296/0.349 at 2°/4°, Kx/Fz 21.2.

Tarmac, ngp / experimental-brush against the band (`tarmac-*`):

- TRL PPR496 (`tarmac-KMH` / `tarmac-wet-KMH`: the tarmac wheel at 32/64/100 km/h, `--speed`, the wet ones with
  `--water 1`, WaterFactor 1 on the dry friction level: the rows are ratios): locked over peak 0.780 / 0.776, 0.709,
  0.668 at 32/64/100 (0.73-0.82, 0.66-0.76, 0.59-0.67); peak 100/32 1.000 / 0.968 (0.97-1.20); locked 64/32 1.000 /
  0.902 (0.86-0.98); wet locked over peak 0.780 / 0.659, 0.546, 0.505 (0.64-0.67, 0.52-0.57, 0.49-0.52; 0.583, 0.552 with the
  constant Coulomb loss before 2026-11-08); wet locked 100/32 1.000 / 0.730 (0.56-0.81); wet peak 100/32 1.000 /
  0.951 (0.76-1.00). Contidrom (`tarmac-wet-80`, `tarmac-wet-KMH-worn`: `--tread 0.2`, key `stop_mu_x_4k`, the mean
  braked peak of a stop; `tarmac-wet-80-3mm`: `--tread 0.375`): new 100 over 80 1.000 / 0.991 (0.97-1.0), 1.6 mm
  over new from 80 / 100 1.000 / 0.685, 0.521 (0.66-0.83, 0.49-0.55), 3 mm from 80 1.000 / 0.898 (0.87-0.91; the lab's
  NGP side holds the probed friction, so its tread does not move it). The car row from 100 km/h is not
  scored (`how` none): its locked run leaves the line (In game).
- Shape: peak slip angle 9.5° both (5.6-8.5); combined Fy(κ 0.25)/Fy(0) at 4° 0.463 / 0.349 (0.29-0.34, fig ±0.05);
  Kx/Fz 14.0 / 21.2 (18.4-32.9); peak Fx ∝ Fz^0.985 / 0.935 (0.90-0.95).
- Aligning moment (the lab's `Tyre::aligning`: the torque `solve_forces` applies above 2 m/s, trail × Fy less the
  carcass offset Fy/k_y × Fx, flat): t0/a 0.319 / 0.302 (0.36-0.51; brush theory a/3 at the design load); t0 ∝
  Fz^0.307 / 0.694 (0.90-1.05); patch half length ∝ Fz^0.30 both (NGP's contact geometry; 0.55-0.65); Mz peak ∝
  Fz^0.975 / 1.535 (1.60-1.95); Mz peak at 0.421 / 0.289 of the Fy peak's angle (0.29-0.41; the brush's 2.75° on the
  0.25° grid); Mz to 0 never / at 1.13 (0.78-1.73: NGP's trail keeps 0.004 m at 10°, Mz 0.30 of its peak there); peak
  Mz/(μ Fz a) 0.163 / 0.107 (0.19-0.26). Combined at 4°, κ ∓0.05 (context): ngp 0.60 / 1.23, brush -0.15 / 1.41,
  both lower under braking, as the carcass offset makes them (NGP's trail barely falls with κ, 0.92 of pure at 0.05;
  the brush's 0.61-0.71, the MF sets' αeq 0.26-0.42). Ice: Mz peak at 0.25° / 1.25° (brush theory: a quarter of the
  full-slide angle); gravel and snow Mz peak at 0.42 / 0.25 and 0.50 / 0.25 of the Fy peak's angle.
- In game (`--trace-speed`'s `ffb` column, the filtered steering torque before NGP's curve; r40 circle ramp, map 85):
  mean torque per 1 m/s bin, ngp / brush, 147 / 152 at 0.35 g, 190 / 176 at 0.54 g, 224 / 203 at 0.70 g (the peak),
  186 / 176 at 0.78 g and 120 / 154 at 0.85 g as the fronts saturate: both go light before the limit, the brush by
  24 % against NGP's 46 %; the trail's load exponent moves it under 1 %.

Ice (16 scored, the probe and brake rows on 116; ngp / experimental-brush): peak slip angle 0.75° / 4.5° (4.5-5.1),
5.0° at 6.7 kN for the brush (4-5); peak κ 0.02 / 0.06 (0.04-0.06); Cα/Fz 0-2° fit 3.72 / 2.43 (3.7-5.5); Kx/Fz 2.86
/ 2.52 (3.6-6.7); μ_y(20°)/peak 0.97 / 0.91 (0.59-0.86); locked/peak 0.92 / 0.72 (0.81-0.88); car locked over
threshold 0.990 / 0.809 (0.81-0.88); car decels 0.096 / 0.100 g threshold, 0.095 / 0.081 g locked (studless
0.18-0.22): the level is NGP's. The brush's stiffness on ice trades the peak angle against the initial slope ([Ice
(material 116)](#ice-material-116)).

Gravel and snow under loose-v1: [Loose surfaces: loose-v1](#loose-surfaces-loose-v1).

Camber and pressure rows (2026-11-08; lab keys `camber_per_fz_4k`, `camber_over_calpha_4k`, `*_dp05_4k`: +0.5 bar on
the probe's 2.0 bar through NGP's stiffness factor, patch length held; σ_y as the 63 % distance of a 1° step):
experimental-brush 49 → 52/99 PASS (ngp 33/99), before → camber thrust → carcass pressure law. MISS → PASS C_γ/Fz
0.017 → 0.842 (0.55-1.27; ngp 1.706), C_γ/Cα 0.001 → 0.049 (0.031-0.061; ngp 0.097), ice locked over threshold
0.809 → 0.811 (0.81-0.88, at the edge); still MISS σ_y -0.6 → -5.1 % (-26 to -12; ngp -0.8), Kx +4.9 % (MF 6.1 -17 to
-14; ngp +4.4); PASS Cα +5.8 % (-14 to +12.5, sources disagree; ngp +5.6), peak μ_y +0.3 % (±3). Car rows within
0.005 except the gravel circle (car, not tyre) 0.609 → 0.667 → 0.620 g. Map 85 ramped circle with the setup's camber
at 0°, as set (-3.9°/-1.2°) and -4° (`--setup`): slide at 0.880/0.875/0.876 g brush (0.880/0.875/0.875 before),
ngp 0.890/0.890/0.889; steering against lateral acceleration within the autopilot's noise. With main's aligning rows
merged: 52/106 (ngp 33/106), the ice row back at 0.807.

Wet rows (2026-11-08, `tyre/wet-shape`, full run against 4bb4c042): experimental-brush 52/103 → 59/108 PASS (ngp
33/103 → 35/108); MISS → PASS TRL wet locked over peak at 64 / 100 km/h 0.583 / 0.552 → 0.546 / 0.505 (with the
road texture's drainage; the wet tyre rows rerun); the five new
wet rows (above) pass under the brush, ngp misses the three tread rows; every dry row and the brake, circle and coast
rows unchanged.

### A per-wheel ABS in the brake test

- Hook: `rbr_hybrid::ngp::set_brake_scale` (thread-local, `None` by default) multiplies each wheel's pedal pressure
  (`x + 0x60`) at `brake_step`'s one call site (`rbr-hybrid/src/ngp/steps.rs`). Unset, the read is NGP's: rsf_compare
  drive2/5 physics 84084/84084 and the merge gate equal. Only the autopilot sets it, for one drive (NGP cars only).
- Controller (`--brake-abs K`, `autopilot::Brake::Abs`): a 0.2 s declutched lift first, whose second half gives each
  wheel's free-rolling |ωR| / v (an ABS learns its rolling radii); then the full pedal, and each step (720 Hz) a
  wheel more than 0.02 over K from free rolling has its pressure scale dumped at 20 /s, one more than 0.02 under
  reapplied at 5 /s, in between held. The reference matters: declutched coasts read 1 - |ωR| / v 0.054-0.061 on 345
  gravel and 0.022-0.028 on 568 tarmac at every speed over 4 m/s; the raw ratio at K 0.15 is free-rolling 0.09 on
  gravel and gave NGP 1.67-1.80.
- K 0.15: the middle of the 0.1-0.2 slip a 1990s ABS regulates about (Burckhardt 1993), as the NHTSA and Macnabb
  fleets ran.
- Measured (Fiesta Rally2, declutched; locked g, ABS K 0.15 g, locked / ABS; best single-pedal target's ratio):

| | gravel 50 | gravel 80 | tarmac 50 | tarmac 80 |
| --- | --- | --- | --- | --- |
| ngp | 0.628, 0.573, **1.095**; 0.858 | 0.708, 0.658, **1.076**; 0.857 | 1.062, 1.232, **0.862**; 0.844 | 1.086, 1.295, 0.839; 0.834 |
| experimental-brush | 0.892, 0.644, **1.385**; 1.162 | 0.926, 0.772, **1.199**; 1.077 | 1.093, 1.316, **0.831**; 0.849 | 1.041, 1.330, 0.783; 0.792 |

  Targets: gravel 1.12-1.59 (ngp MISS by 0.03 / 0.04, brush PASS), tarmac 0.58-0.885 (both PASS; wet Jennite by the
  gravel method 0.54-0.62). At K 0.1 / 0.2 gravel gives ngp 1.33 / 0.99 (50) and the brush 1.57 / 1.34: the ratio
  follows K steeply on NGP's gravel curve, so the band call for ngp rests on K.
- Wheels under ABS K 0.15 (slip from free rolling, mean; cycle rate, upward crossings of K + 0.02): fronts 0.12-0.14,
  3-13 Hz (ngp gravel 3-8, tarmac 7-10; brush gravel 6-10, tarmac 8-13); rears 0.09-0.13, at full pressure most of
  the stop (the brake balance leaves them under K), 0-4 Hz. The law dumps at K + 0.02, so the fronts hold 0.01-0.02
  under K. Unit check: `the_abs_holds_each_wheels_slip_from_free_rolling` (toy wheel, rim read 6 % low).

## Open

- Driving it: combined slip (0.72 of NGP's side force at 4° and 25 % slip, inside the measured road data; see
  [Racing and rally targets](#racing-and-rally-targets)) is the largest change a driver would feel. The
  deformation torque M''_z (∝ C_y - C_x) is left out of the aligning moment. Combined Mz: the MF sets' fitted s·Fx and
  Wong's Goodyear data disagree on whether braking or driving raises it, so neither M''_z (bristle, Svendenius eqs.
  4.37-4.39, braking lowers Mz with C_x > C_y) nor the longitudinal half of the carcass torque Fx Fy (1/c_x - 1/c_y)
  (his eq. 4.72, which would cancel NGP's offset to within 10 % at c_x 450, c_y 405 kN/m) was added (2026-11-08).
- Aligning moment level and patch: t0/a 0.30 against the sets' 0.36-0.51 and peak Mz/(μ Fz a) 0.107 against 0.19-0.26
  would make the steering 1.2-2.4 times heavier near the Mz peak than NGP's: a force-feedback level decision, left. The
  trail's load exponent stays short (0.69 against 0.90-1.05) because NGP's patch grows ∝ Fz^0.30 (MF-Swift 0.6); the
  brush cannot take its own contact length without NGP's patch at the design load (not in `Wheel`).
  Autopilot pace, map 41, 600 s, finish ngp / experimental / experimental-brush (2026-11-08): Fiesta Rally2 309.01 /
  304.61 / 304.06 s, Celica ST185 310.91 / 305.17 / 304.48, Golf II 324.24 / 314.57 / 313.77, 131 Abarth 323.35 /
  315.49 / 315.15: the engine 89-96 % of the gap (1.4-3.0 %), the tyre 0.1-0.25 %. The autopilot plans 5 m/s² lateral
  and 4 m/s² braking, under half the grip. Of the tyre's share, Kx (shape, data-backed: NGP 14 Fz, published 18.4-32.9,
  brush 21) 0.18-0.50 s, a build with NGP's Kx: the brush's driven wheels slip a third less on throttle. Ruled out
  (builds, not committed): NGP's Cα load exponent (within 0.04 s); no carcass dynamics above 3 m/s and a friction flat
  with sliding speed are faster still. 0.1-0.2 % unattributed.
- σ for a rally tarmac tyre against speed and pressure is unmeasured. Ours shortens 5 % per +0.5 bar (2026-11-08;
  op het Veld -12 %, MF 6.1 -21 to -26 %, racing bicycle tyres twice as long at 3.5 as at 7.5 bar, Meccanica 2023):
  meeting the sources needs Cα to fall with pressure, as theirs does at 4 kN while Goodyear's rises near nominal
  load and NGP's rises. Kx against pressure: NGP's +4.4 % per +0.5 bar against MF 6.1's -15 % (one set).
- Wet: the partial hydroplaning's onset and ramp rest on two ABS braking tests that disagree at 1.6 mm (AUTO BILD
  0.69, Continental's report 0.80 of new from 80 km/h; films unstated) and on an 8 mm new groove depth; a rally
  tarmac tyre's (FIA: at least 17 % grooves of 5.5 mm) would set the onset lower (v_on 19 m/s new at 5.5 mm). No open data on a new tyre's wet peak above 100 km/h, nor on the
  film's depth in a braking test. NGP's tread cannot be set in a test run (`--wear` is the road's), so the tread rows
  are lab only. Wet on the 12 RSF stages without a wet `.mat` entry (568 among them) falls back to material 0 (μs 0.95, dry; the tarmac tyre's static 0.83, In game):
  whether the original does the same, and whether the launcher should offer wet there, is open.
- The brake test from 100 km/h: locked fronts cannot steer and the car leaves the line on every stage tried (568,
  320, 482); the TRL 100 km/h point is checked in the lab only. A straight, flat stretch or a held heading would
  fix it.

- A rally tyre's own longitudinal carcass stiffness and damping are unpublished: c_x 450 kN/m is ours (road sets
  288-399 kN/m, measured 163-234: strong that c_x ≥ 300 kN/m and c_x > c_y, weak on the rally value, range 350-550),
  d_x Rill's passenger tyres' (MF-Swift's residual damping is a template default). The lateral carcass stays NGP's
  (σ_y/R 0.65 against the sets' 1.01-1.84).
- Low speed: the brush's carcass rings after a stop longer than NGP's sticking model settles (ζ 0.05-0.07, lab; in
  game under 0.02 m/s by 1.5 s) and the Fiat rocks at rest while revving; a low-speed carcass damping is unmeasured.
  The hold torque and the drive line's sticking load stay NGP's. LuGre with Canudas de Wit's σ_0 181.54 1/m (726 kN/m
  at 4 kN, 1.6 times c_x) and σ_1 ~1 would be a second bristle over the same contact; Rill's restored dynamics already
  give the stick-slip at standstill LuGre adds, so it was not prototyped.
- Ice: NGP's level (0.094) against the measured 0.15-0.18 keeps the brush's peak slip angle and initial slope from both
  matching; a flat straight to stop on (504's 6.6 % rise is taken out by energy); the brush's locked/peak 0.72 against
  0.81-0.88 (its sliding-speed fall, fitted on tarmac, applies on ice too); rally studs (no data).
- Loose surfaces unmeasured: combined slip on gravel and snow, and the soil's lateral force above 25 m/s through the whole car. NGP has no ploughing (the declutched brake
  test stops 10-17 % longer locked). loose-v1's berm (1.3 from a braking slip of 0.8) gives 1.11-1.16 over the best
  single-pedal target. At NGP's grip level the locked braking is 0.89-0.98 g, above the road cars' 0.47-0.66 g (a
  lower bound for a rally tyre) and inside the 0.8-1.0 g inferred for a WRC car; the tyre's lateral peak 0.83 is above
  CW's ≲ 0.67 for a WRC tyre while the car's 0.663 g is inside CW's 0.6-0.7 g. A lower loose level is a product
  decision (the trade-off table in [Loose surfaces: loose-v1](#loose-surfaces-loose-v1)). Also open: the berm's shape
  against slip (unpublished), an
  ABS's own μ-slip on gravel (the scorecard's ABS is ours, K 0.15 from free rolling), a spinning
  tyre's sinkage resistance, the berm against the layer's thickness, and snow (same model, not tuned; 1.06 g full
  pedal against 0.31-0.35 g).
- loose-v1 open items:
  - Eb's 1-4 % over ngp on soil is the engine model's (3-4 %); loose-v1 adds -1.0 to +1.9 %.
