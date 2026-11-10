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
- Heat step (`tyre::heat::heat_step`, after the forces; `relax` after each tyre step), Fiesta Rally2 front: 8 sector
  surfaces of 38.4 J/K each (0.0192 kg × 2000 J/(kg·K), ~0.5 mm), tread 5836 J/K, carcass 16165 J/K, tread
  conductivity 1 W/(m·K), carcass-tread 2 W/(m·K) at `def+0x124` (613 W/K over the band), contact conductance
  0.5-1.2 over 50-200 kPa, air 293.1 K, road air + 9.86 K. Of the slip power ~19 % reaches the surface, 23.6 % the
  tread, 12 % the carcass; the rest is dropped by `(1 - 0.6 hot) · wet damping` (1/3 dry), so the surface runs a few
  K from the tread (30° slide at 20 m/s: 331 → 346 K in 2 s). Map 41 autopilot: from 337 K the tread falls to
  318-319 K over the stage.
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
| Peak μ against load | lateral μ ∝ Fz^-0.08 over 2-6 kN (peak Fy ∝ Fz^0.92; Fz^0.88 over 2-10 kN) (figure); longitudinal falls at 0.73 / 0.39 of the lateral rate | Singh & Sivaramakrishnan (Goodyear), arXiv 2305.18422, Fig. 15b; MF6.1 / PB |
| Stiffness against load | over 2-6 kN: Cα ∝ Fz^0.83-0.85 (figure; ~0.6-0.7 only over 2-10 kN), sets 0.91 MF6.1 / 0.69 PB / 0.56 TNO / 0.73 Continental; Kx (|κ| ≤ 0.04 fit) ∝ Fz^1.13 / 1.08 / 1.16 / 1.14 | Goodyear Figs. 9, 15a; MF6.1 / PB / TNO / Continental sets |
| Longitudinal stiffness | Kx/Fz 32.9 / 18.4, Kx/Ky 1.62 / 1.15 at Fz0 | MF6.1 / PB |
| Peak slip | 6.5° / 8.5°, κ 0.094 / 0.139 at Fz0 | MF6.1 / PB |
| Cα: pressure / tread / heat | +10 % for +20 % pressure (load not stated; Fig. 9 falls with pressure up to nominal load, -12 % at 100 % and -19.5 % at 67 % for 33 → 41 psi, -18 % per +0.5 bar: `tarmac-calpha-pressure`'s low end); +30 % at 60 % less tread; -20-25 % cold to hot (bulk temperature) | Goodyear, Table 3 |
| Peak μ: pressure / tread / heat | small; +10 % at 60 % less tread (dry); -10 % cold to hot (surface temperature) | same |
| Grip against temperature | passenger tyre 0.7 to 1.05 warming, optimum 88 °C, width 50 °C | Alcázar Vargas et al., Sensors 22:6380 (2022) |
| Which temperature | grip follows the tread core, stiffness the carcass | Farroni et al., Appl. Sci. 10:1604 (2020) (TRT); contradicts Goodyear's surface-for-μ; both bound NGP's 0.6/0.4 blend |
| Friction against sliding speed | g(Vs) = μ_C + (μ_S - μ_C) exp(-√(Vs/v_s)), μ_C 0.8, μ_S 1.55, v_s 6.57 m/s | Canudas de Wit et al., VSD 39 (2003) 189-226, eqs. 21-22, Table 1: their distributed LuGre (brush) fitted to straight braking on a dry test track (author preprint HAL-00949740; v_s held fixed in the fit) |
| Sliding over peak | lateral 0.83 full slide (PB), 0.73 asymptote (MF6.1); longitudinal 0.71 at κ 0.67 (PB), MF6.1 at 4 kN 0.80 / 0.73 at κ 0.5 / 1 driving, 0.85 / 0.77 braking, asymptote sin(C π/2) 0.65 (at Fz0 0.78-0.83 / 0.72-0.75; the shifts move them 0.01-0.02); locked over peak dry 0.65 to 0.885, wet 0.64 / 0.69 | sets; Burckhardt 1993 dry/wet asphalt via three reproductions (test speed unstated); Lorenčič, Sustainability 15:6945 (2023), ABS over locked at 30-40 km/h (secondary read) |
| Dry friction against speed | peak "relatively unchanged" with speed on dry runways, fast drop wet; Kiencke & Nielsen's factor exp(-c4 Vs), c4 0.02-0.04 s/m | Horne & Leland, NASA TN D-1376 (1962), aircraft tyres; Sova et al., IOP Conf. Ser. 832:012011 (2020) (book not read) |
| Relaxation length | 0.12-0.45 m, longer at higher speed and load (Wikipedia, citing Cossalter: motorcycle tyres), of the order of the wheel radius at Fz0 (Pacejka); car tyre 205/65 R15 0.3-0.9 m at 2-6 kN, 30-70 km/h; grows with speed: σ_y = -0.14 + 0.021 V + 1.9e-4 Fz - 1.6e-8 Fz² m (car tyre bench, ≤2 Hz; also with frequency, <0.5 m near 0 Hz to ~1.0 m at 1 Hz); motorcycle tyres 0.1-0.4 m, "higher ... at higher speeds"; MF-Swift: independent of V, set by load, slip and pressure; σ/R0 at Fz0 lateral 1.01 / 1.84, longitudinal 1.19 / 1.44 | Wikipedia "Relaxation length" (Cossalter; Pacejka for the wheel radius); Alcázar Vargas et al., Sensors (2022) p. 15; Cossalter, Lot & Massaro, Motorcycle Dynamics, ch. 1 (Wiley excerpt); Schmeitz, TNO MF-Tyre/MF-Swift lecture (2013); MF 5.2 truck / MF6.1 sets |
| Trail at small slip | a/3 (half contact length a) | brush theory (Pacejka 2012) |
| Aligning moment, pure slip | at 4 kN: t0/a 0.42 / 0.51 / 0.36 (t0 at ±0.5°, a the MF-Swift contact length from Q_RA1/2); t0 ∝ Fz^0.93 / 0.93 / 1.01 over 2-6 kN, a ∝ Fz^0.60 / 0.64 / 0.59; Mz peak ∝ Fz^1.64 / 1.90 / 1.90, peak Mz/(μ_y Fz a) 0.19 / 0.26 / 0.20 (brush theory 27/256 = 0.105); Mz peak at 0.30 / 0.37 / 0.35 of the Fy peak's slip angle, Mz back to 0 at 0.78 / 1.73 / 1.18 of it (Continental's crossing is flat: 1.80-1.85 without s·Fx), then down to -30 / -4 / -13 Nm (0.65 / 0.06 / 0.29 of the peak). Measured Goodyear Wrangler P225/75R15 (48 km/h, ~6 kN): Mz peak 3.7° against the Fy peak at 9-11°, 0 at 13-15°, never negative | TNO 205/60R15 / Continental 225/50R18 (MF-Tool 6.2) / MF6.1, evaluated with MF 6.1's Mz (MF/model; the KT_MF_Tool 245/60R16 template copies TNO's and is left out); Svendenius, PhD Lund 2007, Fig 5.4 (NHTSA data, fig ±0.25°) |
| Aligning moment, combined | Mz(κ)/Mz(0) at 4°, κ ∓0.05, 4 kN: braking 0.62 / 1.29 / 0.40, driving -0.08 / -0.63 / 0.44, at α +4° (at -4° braking 0.99 / 0.71 / 0.43, driving -0.34 / -0.01 / 0.48) (MF 6.1's combined Mz has one fitted term, s·Fx; MF6.1's is 0). The fitted s puts braking above driving; Wong (no data or reference) says driving raises and braking lowers Mz, as a carcass lateral deflection under Fx does. Conflict: not scored. Low μ: the trail falls to 0 at the full-slide angle, ∝ μ (Hsu, Laws & Gerdes, IEEE TCST 18:4, 2010, on dry gravel; model, no Mz numbers); no open Mz data on snow, ice or gravel | same sets (MF/model); Wong, Theory of Ground Vehicles 3rd ed. §1.4.2 (read 2026-11-09: one qualitative sentence, no data or reference) |
| Contact patch, braking | static ink print 205/55R16 at 4821 N, 240 kPa: 147 mm long, 161 mm wide (NGP's tarmac patch at 4.8 kN, 200 kPa: ~0.165 m, 12 % longer, as the lower pressure suggests; the brush's a is half of it); ABS car 100-0 km/h on ten such tyres 36.3-41.7 m, 0.94-1.08 g mean (v²/2s, pedal rise included: a lower bound for a rally tarmac tyre) | Liang et al., Adv. Mech. Eng. 12(12) (2020), doi 10.1177/1687814020981173 (table; g inferred) |

| Convection | Nu ∝ Re^0.8 (NGP 0.78) | West & Limebeer, VSD 60 (2022) |
| Hydroplaning speed | v_p = 6.35 √p km/h (p in kPa; 9 √p kt, p in psi, converted; 6.36 in Deng et al.) for water deeper than the tread grooves; thin films raise it (Huebner: 26.04 WD^-0.259 mph for WD < 0.1 in); 2 of 8 mm tread costs 17-22 % of v_p; μ ≤ 0.05 at total hydroplaning | Horne & Dreher, NASA TN D-2056 (1963), metric form in Deng et al., Lubricants 13:76 (2025); Gallaway-Huebner via Salvi & Kumar, PLOS ONE 17:e0272993 (2022), eq. 2A; ADAC worn-tyre test (2026) |
| Wet films and speed | road films 0.18 mm in light rain, 0.56-0.85 mm in heavy (drained highways); damp braking μ only slightly above wet; a new tyre brakes from 100 km/h on a watered track at the mean μ it has from 80 (0.98/0.97), at 1.6 mm it keeps 0.69 / 0.52 of new; ABS 80-0 km/h on wet asphalt, EcoContact 6 225/45 R18 at 2.6/2.3 bar, μ* 0.689 new, 0.615 at 3 mm, 0.554 at 1.6 mm (0.89, 0.80 of new; film unstated) | PLOS ONE (2025), Table 2; TN D-1376; Continental Contidrom via AUTO BILD (quoted); Continental test report 2021_100 (2021, table) |
| Wet against dry, car braking | ABS stops on the same tyre and speeds, μ = v²/2gd (means, not peaks): UHP road wet/dry 0.66-0.79 at 100-0 km/h (14 tyre-car pairs, middle ~0.71; SportContact 7 0.71/0.78, Hankook S1 evo Z 0.79); semi-slicks 0.54-0.66 (Cup 2, A052 0.66; Trofeo RS, Cup 2 R 0.54; slick wet 0.35); wet 80-0 against dry 100-0: road 0.73-0.80, semis 0.48-0.59 (several mm water). Water depth unstated (R117 test 1.0 ± 0.5 mm). Band for a performance tarmac tyre 0.65-0.80; a treaded rally tyre ~0.65-0.75 (inferred). NGP's 0.79, at any speed, is the band's top | AUTO BILD sportscars 2024 (AMG GT), 2025 (Emira) (table); Track Day 2026 (M2) via Tyre Reviews (quoted, search extract); sport auto 2018 (quoted; wet speed inferred); ADAC 2024 (quoted) |
| Damp against dry and wet | μmax on a macro- and microrough road, film < 10 µm: 0.8/0.7 of dry at 60/100 km/h; 1.5 mm 0.6/0.4; 5 mm 0.5/0.1. MXV3A dry 1-1.3, damp draining mix 0.7, 2 mm wet asphalt 0.6; on a damp draining mix a competition compound 0.8 against car 0.6-0.7. Rubber slider (DFT, 40 km/h) keeps ~0.82 of dry to ~0.3 mm, then 0.28 by 0.5 mm; critical depth 0.34/0.31/0.15 mm at 20/40/60 km/h. NGP damp 0.92 is above all of them | Michelin, "The tyre: Grip" (2001), pp. 29, 59 (table, rounded); Beautru et al., SURF 2012, Figs 10a, 13 (fig ±0.03) |
| Wet against slip speed | F(S) = F60 exp((60 - S)/S_p), S_p = 14.2 + 89.7 MPD km/h (smooth test tyre); on the one road with both, S_p 67 predicted against ~220 km/h in locked-wheel data, so not used | ASTM E1960 via two Virginia Tech theses; Rajapakshe et al., Dyna 176 (2012) |
| Wet peak against speed and film | car tyre peak μ ("Haftreibungszahl"), all films ~0.755 at 60 km/h; at 100 / 140 km/h: 0.2 mm 0.745 / 0.73, 0.5 mm 0.71 / 0.655, 1 mm 0.675 / 0.59, 2 mm 0.60 / 0.375, 3 mm 0.41 / 0.14 (so 100/60: 0.99, 0.94, 0.89, 0.79; 140/60: 0.97, 0.87, 0.78). Wet peak independent of film and speed to ~50 km/h (Veith). Summer 195/65 R15, 3-5 mm tread, film 0-1 mm, MPD 0.6: wet/dry peak ~0.86 at 30-80 km/h, each falling 0.03-0.06 over it. Same size at 2 mm tread, 1 mm film over MPD 0.58, 5 kN: 0.98/0.88/0.80/0.58 at 20/50/60/80 km/h | Reimpell & Sponagel, Fahrwerktechnik: Reifen und Räder (1986), via Müller et al., FAT-Schriftenreihe 299, Abb. 21b (fig ±0.02; its caption credits Reimpell & Sponagel 1986, whose figure is probably Gengenbach 1968); Veith, ASTM STP 793 (1983) via Beautru 2012 (quoted); Klempau, diss. TU Darmstadt (2004), Abb. 71, 72 (fig ±0.02) |
| Rain to film, damp | 2.5 % cross-fall, 3.5-7 m flow path, texture 0.5 mm: film over the aggregate tops (Gallaway) ~0 at 2-10 mm/h, 0.24-0.50 mm at 25 mm/h, 0.62-1.0 mm at 50 mm/h; over the mean profile (Ross & Russam) 0.09-0.12 / 0.19-0.26 / 0.29-0.40 mm at 2 / 10 / 25 mm/h. So ≥ 0.5 mm is heavy rain, ruts or puddles. Road sensors: damp from 10 µm, wet from 0.1 mm (Lufft MARWIS defaults); a road looking black without a shiny film read 0.08 mm | Gallaway et al., TTI 138-5 (1979), metric form as TII/Arup give it (inferred: computed); Ross & Russam, RRL (1968) via PMC8253438 (inferred: computed, the slope's unit unconfirmed); MARWIS manual (table); Sandberg et al., rolling resistance in wet (quoted) |
| Damp against dry (more) | damp asphalt: peak 0.79 at κ ~0.07 against dry 1.13 at ~0.15 (0.70), locked 0.58 against 0.86 (0.67), speed unstated. Peak against a reflection sensor's wetness, 195/65 R15: asphalt concrete 1.15 dry, ~0.97 damp, 0.95 wet (0.84, 0.83), other surfaces ~1.1, ~0.83, ~0.78 (0.75, 0.71): most of the loss comes at damp. Locked, damp 0.70 flat from 20 to 100 km/h against dry 0.90 (0.78), wet 0.61 → 0.33. So damp ≈ 0.70-0.85 of dry at the peak, 1.0-1.15 of wet at low speed, without a speed effect | Bachmann, diss. TU Darmstadt (1998), via FAT 299 Abb. 19 (fig ±0.02) and Klempau (2004) Abb. 16 (fig ±0.03); Reimpell & Sponagel (1986) via FAT 299 Abb. 20b (fig ±0.02) |
| Wet peak against speed and film, drums | Karlsruhe inside drum (fine-grained coating, no macrotexture). 205/70 R14, 2 bar, 3 kN, peak μ at 60/80/100/120/140 km/h: 0.2 mm 0.671/0.659/0.643/0.640/0.618, 0.5 mm 0.652/0.643/0.620/0.588/0.553, 1 mm 0.611/0.578/0.528/0.473/0.435, 2 mm 0.602/0.525/0.420/0.318/0.168, 3 mm 0.602/0.482/0.224/0.092/0.076; "limit of hydroplaning" (μ 0.25) at 3 mm ~97 km/h. At 0.7 mm (interpolated) of 60 km/h: 0.970/0.917/0.852/0.796; linear from 70 km/h at 0.0104 per m/s (residual ≤ 0.005), through 60 km/h 0.0087 (≤ 0.019). 155 SR13 new (~7 mm), 1.8 bar: braking peak 0.60 at 60 km/h for 0.2-1 mm (2 mm 0.54), flat to 80; 0.2/0.5/1 mm at 100 0.595/0.595/0.57, 140 0.567/0.551/0.523 (0.7 mm of 60: 0.98 / 0.90 at 100 / 140); lateral peak 0.2/0.5/1 mm 0.60/0.595/0.585 at 60, 0.575/0.567/0.522 at 100, 0.545/0.518/0.47 at 140 (0.7 mm: 0.925 / 0.84) | Liedl & Köhler, ASTM STP 793 (1983) pp. 232-249, Fig. 9 (fig ±0.01; drum of Gnadler's institute, so possibly Reimpell's source); Williams & Evans, ASTM STP 793 pp. 41-64, Fig. 2, Table 2 (fig ±0.01) |
| Wet peak against speed, void and texture | wet peak, slide and cornering coefficients fall "in an approximately linear manner" with speed; level independent of depth at 30-50 km/h, strongly dependent at ≥ 96 km/h; cornering vs log depth slope 0.01/0.05/0.20 per decade at 32/48/96 km/h. E 274 trailer, GR78-15, water 1.5-3 mm (centreline average): straight braking peak 96/32 km/h 0.93 for a tyre of fractional groove volume φV 0.41 (groove area share × depth / 10 mm, Table 4: not an area void) on coarse (S) and bleeding (H) asphalt, 0.38 for φV 0.18; slide 0.62-0.67 / ~0.27; cornering (max to 9°) 0.85-0.90 / 0.40-0.52. Cornering 96/48 km/h, lines from 24 km/h: rough surface 0.86 rib, 0.93 block; smooth 0.57 rib, 0.83 block. Wet against dry at 48 km/h, 0.5 mm, textured belt: peak 0.84 both, locked 0.51 / 0.73; lateral peak (H78-15, concrete) wet/dry 0.83 at 32 km/h, wet 64/32 0.86, 96/32 0.75 | Veith, ASTM STP 793 pp. 3-40: text (quoted), Table 1 (table), Figs. 2, 9, 15-20 (fig ±0.02), Table 6 (table); Bergman, TRR 621 (1977) and Beauregard & McNall, SAE 730147 (1973), both via Veith Figs. 7, 8 (fig ±0.02) |
| Damp (thin film) against wet | at 60 km/h: drum 0.2 mm over 0.7 mm 1.055 (Liedl), 1.00 braking, 1.01 lateral (Williams); cornering 0.12 over 0.88 mm (trailer, low-texture pad) 1.01 / 1.09 / 1.38 at 32 / 48 / 96 km/h. Speed loss at 0.2 mm, 100 / 140 over 60 km/h: 0.958 / 0.921 (Liedl), 0.99 / 0.945 braking, 0.958 / 0.908 lateral (Williams). No measured damp against dry found | same papers (fig ±0.02) |
| Worn tread against film and speed, drum | 155 SR13 worn evenly to 0.6-1.6 mm (centre ~1.55), braking peak over new at 60/80/100/120 km/h: 0.2 mm 1.23/1.20/1.21/1.00 (lateral 1.38 at 60), 0.5 mm 0.96/0.68/0.34/0.18, 0.7 mm (interpolated) 0.85/0.49/0.24/0.13, 1 mm 0.67/0.20/0.07/0.06; half lost at ~90/80/67 km/h on 0.5/0.7/1 mm (∝ h^-0.43, inferred). At 2 mm water over new at 80/100/120: 3 mm tread 0.27/0.24/0.27, 2 mm 0.20/0.165/0.19, ~1 mm 0.10/0.07/0.07 | Williams & Evans, ASTM STP 793, Figs. 2, 4, 6, Table 1 (fig ±0.02) |
| Hydroplaning against groove pattern | CFD, locked G78-15 on 5 mm water, smooth road, 186 kPa (Horne 87 km/h): longitudinal / transverse / block grooves 98 / 127 / 145 km/h at 5 mm groove width, depth and 16.8 mm spacing; per mm +1.6 / +1.6 / +1.8 km/h deeper, +6.8 / +5.5 / +7.2 wider, +1.8 / +2.5 / +2.3 closer; 5.1 → 8.5 mm depth +6 km/h (~6 %). Groove volume φV = void × depth / 10 mm: at 96 km/h slide and cornering μ rise from ~0.1 at φV 0.1 to a plateau by ~0.3; no effect at 48 km/h | Kumar, Kumar & Fwa, J. EASTS 8 (2010), Tables 2-4 (table; a model, verified on smooth tyres only); Veith, Figs. 13, 21 (fig ±0.02) |
| Wet against film, speed and tread, roads | Dutch multifactor trailer, 4 car tyres (Uniroyal Rallye 180, Michelin ZX, Vredestein Sprint, Goodyear G 800) × 4 roads (sand patch TD 0.3/0.6/0.7/0.8 mm) × 50/100 km/h × sprayed water 0.3/0.6 mm (volume over wetted area, so mostly in the texture; 0.3 the least that wets the whole track) × tread new (7-8 mm)/2 mm, 250/400 kgf. Means: peak 0.906/0.781 at 50/100 (new 0.900/0.844, 100/50 0.938; 2 mm 0.912/0.718, 0.787); locked 0.602/0.410 (locked/peak 0.66/0.525; new 0.619/0.464, 2 mm 0.584/0.355); lateral 0.830/0.745 (new 0.800/0.755, 0.944; 2 mm 0.860/0.735, 0.855). Water 0.6 over 0.3 mm: peak new 1.005, 2 mm 0.914; locked 0.98; lateral 1.00 at 50, 0.97 at 100. Road 100/50 peak 0.83-0.925 (TD 0.3 lowest). No dry or damp level | SWOV R-73-08 (Voorburg 1973), Bijlage 8 pp. 1-4 to 1-21, 2-14 (table), Bijlage 5 Tabel 1 (table) |
| Wet against dry, lateral, trailer | free-rolling max lateral coefficient (sweep to 20°), 1510 lb, 28 psi, concrete: OE bias-belted, dry 0.763 at 32 km/h (6 tyres), wet 0.687/0.648/0.645 at 32/64/96 km/h (2 tyres; wet/dry 0.90; 64/32 0.94, 96/32 0.94), water from speed-proportional nozzles, depth unstated. Fig. 18 (another run): H78-15 bias wet/dry 0.83, 64/32 0.87, 96/32 0.74; HR78-15 radial wet/dry 0.755, 64/32 0.88, 96/32 0.83; wet painted asphalt 0.41-0.43 (dry 0.835 per text). Dry braking peak 1.05 at 12 % slip, slide 0.85 (H78-15, 32 km/h) | Beauregard & McNall, SAE 730147 (1973), table p. 8 (table), Figs. 7, 18 (fig ±0.02) |
| Wet trailer peak against speed, 1.5 mm | 195/65 R14, wet asphalt, 1.5 mm water, 4 tyres, peak at 40/60/80 km/h 0.99/0.98/0.93, 0.90/0.86/0.81, 0.95/0.93/0.91, 0.98/0.94/0.91 (80/40 0.90-0.96, 80/60 0.94-0.98), locked 0.69/0.63/0.54, 0.58/0.52/0.45, 0.71/0.62/0.53, 0.76/0.66/0.58 (locked/peak 0.64-0.78 at 40, 0.56-0.64 at 80). 205/60 R15 μ-slip, surface unstated (peak 1.41 at 50 km/h, so probably dry): peak 1.41/1.37/1.30/1.27/1.245/1.21 at 50/60/80/90/100/110 km/h (100/50 0.88) | Oh & Lee, Adv. Mech. Eng. 2014:260428, Table 3 (table), Fig. 6 (fig ±0.02) |
| Wet against dry, rubber sealing | low-speed tyre-road friction on wet roads 20-30 % below dry (wet/dry 0.70-0.80), explained by rubber sealing water pools in the texture (less hysteresis), not by a film: present as soon as the texture holds water | Persson et al., Nature Materials 3 (2004) 882, abstract (quoted, not read in full) |
| Worn tread against speed and road | UK (RRL, cross-ply 5.60-13, 3.5 kN, worn in traffic; peak from car deceleration, two tyres averaged, so low), 8 → 2 → 1 → 0 mm tread: asphalt concrete, 0.5 mm over the aggregate, peak 50 km/h 0.80 → 0.78 → 0.78 → 0.77, 80 0.775 → 0.75 → 0.72 → 0.62, 130 0.77 → 0.64 → 0.60 → ~0.40; locked 0.52 → 0.50 flat, 0.43 → 0.36 → 0.30, 0.22 → 0.16 → 0.13. Mastic asphalt (smooth), 1.1 mm: peak 50 0.73 → 0.71 → 0.60 → 0.17, 80 0.53 → 0.31 → 0.22 → 0.10, 130 0.33 → 0.07 linear; locked 50 0.46 → 0.38 → 0.19. Germany, lateral (> 8°), 1.5 mm calculated water, 2.35 kN: 155 SR 15 radial 40/60/80 km/h 1.07/0.95/0.82 new, 2 mm 0.99/0.94/0.90 of new, bald 0.96/0.86/0.73; cross-ply 5.60-15 0.86/0.76/0.70, 2 mm 0.92/0.86/0.74, bald 0.77/0.63/0.44; "no particular point beyond which μy drops". The Dutch trailer results (4 tyres, 165 SR 13) are on pages missing from the file | Dijks, TRR 621 (1977) pp. 136-140, Figs. 2, 3 (fig ±0.02); the file ends at p. 140 (its pages 6-26 are Horne's runway paper, TRR 621 pp. 101-121) |
| Wet-weather tread against dry, same compound | GR78-15 trailer, identical construction and compound, 7-rib 20 % groove area ("closed") against block 40 % ("open"), wet cornering coefficient, water held constant (1.4-1.8 mm on the paper's other surfaces): open over closed at 32 / 48 / 64 / 80 / 96 km/h on a smooth (low macrotexture) road 0.94 / 0.96 / 1.02 / 1.10 / 1.35, on a rough one 0.89 / 0.92 / 0.97 / 1.01 / 1.01; speed gradient per mph closed / open -0.0069 / -0.0023 smooth, -0.0023 / -0.0012 rough. Locked slide μ against φV (Surface J, SN 24, 1.4 mm): 32 km/h flat 0.39 from φV 0.08; 64 km/h 0.21 / 0.27 / 0.29 at φV 0.07 / 0.20 / 0.32; 96 km/h 0.07 / 0.19 / 0.235 / 0.25 at 0.07 / 0.20 / 0.31 / 0.40. Cornering against φV (Surface A, SN 16): 2 km/h 0.68 → 0.51 over φV 0.15 → 0.32 ("the same wet or dry in a relative sense"), 48 km/h flat 0.41, 96 km/h 0.15 / 0.24 / 0.28 / 0.31 at 0.15 / 0.21 / 0.28 / 0.32 | Veith, ASTM STP 793 (1983) Fig. 9 p. 17, Figs 13, 21 (fig ±0.02), text (quoted); Veith, TRR 621 (1977) pp. 113-125 (surfaces, water depths: text) |
| Tread depth against film, thin films | cornering coefficient, half against full skid depth, Surface I: 96 km/h 0.86 at 0.12 mm water, ~0.65 at 0.7-0.9 mm, ~0.6 at ~6 mm; 48 km/h 0.95-1.0; 32 km/h 0.97-0.98 (full depth 0.49 → 0.30 from 32 to 96 km/h at ~0.7 mm on that low-texture surface) | Veith, STP 793 Fig. 2 (fig ±0.03; water depths read off a log axis ±15 %) |
| Void and siping at 1 mm, drum | KIT inner drum, 245/45 R18, 4875 N, 2.1 bar, 7.5 mm tread, one compound (wet level ~0.7 asphalt, ~1.0 Safety-Walk), μmax braking at 1 mm water (1 l/m²), over small blocks (SB, 25.0 % void) at 80 km/h: asphalt (MTD 1 mm) SB 1.00 / 0.945, small blocks 35.3 % (SBv) 1.09 / 1.035, big blocks 24.8 % (BB) 0.955 / 0.885 at 80 / 120 km/h (SBv/SB 1.09-1.10, flat); Safety-Walk (MTD 0.2 mm) SB 1.00 / 0.905 / 0.83 / 0.73, SBv 0.94 / 0.915 / 0.895 / 0.83, BB 0.765 / 0.63 / 0.525 / 0.385 at 80 / 100 / 120 / 140 (SBv/SB 0.94 / 1.01 / 1.08 / 1.14). Big blocks at 2 against 1 mm: asphalt 0.965 / 0.93 / 0.835 against 1.00 / 0.935 / 0.93, Safety-Walk 1.00 / 0.665 / 0.42 against 1.00 / 0.82 / 0.68 at 80 / 100 / 120. Sipes, same big blocks, asphalt 1 mm: μmax BB 0.665 / 0.635 / 0.59, siped BBs 0.77 / 0.77 / 0.78, SB 0.65 / 0.63 / 0.61 at 80 / 100 / 120 km/h (BBs/BB 1.16 / 1.21 / 1.32; the authors call BBs' rise with speed not plausible); damp against 1 mm at 120 km/h: BB 0.59 / 0.59, BBs 0.72 / 0.78, SB 0.575 / 0.61 (damp at or below 1 mm, sipes +22 % on damp) | Löwer, Wagner, Unrau, Bederna & Gauterin, Tribol. Lett. 68:25 (2020), doi 10.1007/s11249-019-1264-6, Figs 10, 11, App. A (fig ±0.02; table); Löwer, Wagner, Unrau, Wies & Gauterin, Vehicles 3 (2021) 84-110, doi 10.3390/vehicles3010006, Figs 19, 20 (fig ±0.02) |
| Compound and wet grip | among tread rubbers, wet skid index (concrete, locked) BR 76, 6:4 SBR:BR 94, SBR 1500 100, solution SBR 102, 40 % styrene SBR 111 (wear 240 → 76); oil Tg -52 / -33 / -2 °C: 100 / 104 / 108; peak / slide braking at 64 km/h NR 0.63 / 0.45, synthetic 0.77 / 0.51, high styrene 0.82 / 0.50 (resilience affects the peak more than the slide); wet stopping distance 130-180 ft over five rubbers (BR to butyl, μ 1.38×); "tread composition plays a relatively minor role" (against pattern, speed, road). Same tyre (MXT 175/70 R14) on a damp draining mix: competition compound 0.8, car compounds 0.7 / 0.6, truck 0.55. Higher hysteresis (tan δ at 0 °C) is the industry's wet-grip indicator | Eckert, TRR 621 (1977) pp. 107-112, Tables 1, 2, 4, 5 (table), text (quoted); Michelin, The tyre: Grip (2001) p. 29 (table); Polymers 15:4288 (2023), NHTSA DOT HS 811 270 (2010) (quoted, search extract) |
| Tread temperature, wet | calculated kinetic μ of a "standard tread compound" on asphalt: dry 60 °C peak ~1.07; wet 30 / 40 / 50 / 60 °C ~0.80 / 0.76 / 0.72 / 0.66 (a tyre runs ~30 °C on a wet road, ~60 °C on dry; wet at 60 °C 0.62 of dry); measured μ-slip, wet/dry peak ~0.79 (Fig. 1); dry/wet differ little below 1 m/s sliding | Persson, Tartaglino, Albohr & Tosatti, Phys. Rev. B 71 (2005) 035428 (arXiv cond-mat/0502495), Figs 1, 5 (fig ±0.03), text (quoted) |
| Compound and wet traction, lab against road | LAT 100 wetted disc (75 N, 15° slip, 0.06-6 km/h, water 2-60 °C), side-force rating of three passenger tread compounds against a control (100 %): B 75-85, C 78-116 (115 at 0-5 °C, 88 at 60 °C, log v 1), D 70-107; road wet grip ratings of many test programmes against the lab's 95-120 % (most within 96-108 %), lab ≈ road (slope ~1). No damp and no dry rating of the same compounds | Heinz & Grosch, Rubber Chem. Technol. 80 (2007) 580, doi 10.5254/1.3548182, Table V p. 592 (table), Fig. 9 p. 594 (fig ±1 %) |
| Road tyres' wet grip spread | EU label, C1: wet grip index G (against the SRTT, UNECE R117 Annex 5) class A ≥ 1.55, B 1.40-1.54, C 1.25-1.39, D 1.10-1.24, E ≤ 1.09 (A over E ≥ 1.42); A against E up to 18 m shorter from 80 km/h, up to 30 % | Regulation (EU) 2020/740 Annex I Part B (table); DEKRA, Kumho, Nokian pages (quoted, secondary) |
| Rally tarmac tread rules and tyres | FIA (2006): moulded, ≥ 17 % of a 170 × 140 mm control area in grooves ≥ 5.5 mm deep and ≥ 2 mm wide, groove widths ≥ 4 mm circumferentially and ≥ 16 mm radially; the rule covers tyres grooved < 25 %; ≥ 1.6 mm over three quarters of the pattern. WRC 2013: asphalt tyres ≥ 21 % grooves, two radial grooves. FIA autocross (2020): < 25 % grooving "dry weather", > 25 % "wet weather" (design free), ≥ 5.5 mm new. Michelin H5/S5 (WRC dry asphalt) 6 mm grooves, 202 mm tread; FW2 (full wet, 2016) 7 mm, sea > 27.5 % ("dictated by the regulations"), 175 mm, 215/45 R18, tested on 3 mm water at 90 km/h, 2.1 bar. So φV (area × depth / 10 mm) ≈ 0.10-0.15 dry, ≥ 0.19 wet (inferred) | FIA Appendix IV (2006) arts 2-4 (quoted); Crash.net, WRC 2013 tyre regulations (quoted); FIA Appendix J Art. 279A (2020) (quoted); Tyrepress (2016) (quoted) |
| Low speed | lumped/distributed LuGre: σ_0 181.54 1/m, σ_2 0.0018 s/m, σ_1 ~1 (dynamic fit); MF fades forces below 1 m/s (VXLOW), no standstill | Canudas de Wit et al. (2003), Tables 1, 3; MFeval.jl |
| Longitudinal carcass | c_x at 4 kN (MF-Swift overall stiffness, σ = K/c): TNO 205/60R15 382 kN/m (σx 0.23 m), MF6.1 358 (σx 0.32 m; MFeval 4.0's fit 399), IIT Madras 245/60R16 288; measured 225/50R17 214-234 kN/m (1.9-3.0 bar), P245/70R15 static 163-188 kN/m; c_x/c_y 1.3-2.75 in every set; +5-9 % from 4 to 6 kN | Besselink, Schmeitz & Pacejka, VSD 48 S1 (2010), eqs. 17-20, on the sets; op het Veld, MSc TU/e (2008), Flatplank, Table 4.2; Gent & Walter, The Pneumatic Tire, NHTSA DOT HS 810 561 (2006), Table 9.1 |
| Lateral carcass | road sets σy 0.34-0.75 m at 4 kN (c_y 109-273 kN/m); the two 50-series tyres relax longest, so low profile alone does not shorten σy | same sets; op het Veld; Gent & Walter ch. 8 |
| Carcass damping | MF-Swift residual d_x 690-1060, d_y 300-1000 N·s/m, from the template's 0.002 s on the stiffness (not measured) | same sets |
| Longitudinal past the peak | road MF sets 0.75-0.87 / 0.69-0.82 at κ 0.5 / 1 (4 kN); racing radial MF 6.2 fit 0.68-0.71 / 0.63-0.66 (Dallara AV-21 front, fit range unstated); locked over peak: next row | TNO, Continental, MFeval and IIT sets; JyNing04/Pacejka-tire-model (GitHub) |
| Braked peak and locked against speed | car tyre 195/55R15 on a towed ASTM E274 trailer, braked at constant speed to lock, 32/64/100 km/h, hot rolled (HRA) and stone mastic (SMA) asphalt: dry peak 0.93/0.98/1.12 HRA, 1.13/1.16/1.10 SMA; dry locked 0.77/0.74/0.75 HRA, 0.83/0.77/0.66 SMA; wet (~1 mm water) peak 1.00/1.02/1.00 HRA, 0.99/0.93/0.76 SMA; wet locked 0.64/0.58/0.52 HRA, 0.66/0.48/0.37 SMA. So locked over peak dry 0.73-0.82 / 0.66-0.76 / 0.59-0.67, wet 0.64-0.67 / 0.52-0.57 / 0.49-0.52; locked 64/32 dry 0.93-0.96, 100/32 dry 0.78-0.97, wet 0.56-0.81; peak 100/32 dry 0.97-1.20. Three sport motorcycle tyres within ~0.1 of it except the dry HRA peak (+0.2). A car with ABS from 64 km/h on the dry SMA: 1.11 g (MFDD), PFT peak 1.13, locked 0.68 | Lambourn & Wesley, TRL PPR496 (2010), Figs 6-13 (fig ±0.02), text, Table 1 (table); read in full |
| Wet against dry past the peak, sideways to 90° | Calspan flat belt, J78-15 bias-belted, 6.2 kN, 48 km/h, honed Safety-Walk, 0.5 mm water (wet SN30) against dry (SN85): μ_y wet peak 0.75 at ~13°, 0.645 at 32°, 0.59 at 48°, 0.50 at 90° (= locked); dry 0.875 at ~30°, 0.80 at 60-72°, 0.825 at 90°. So 30°/peak wet 0.87, dry 1.0 (wet over dry 0.87). Braking, same tyre: peak 0.83 / 0.84, locked 0.52 / 0.74 (locked/peak 0.63 / 0.88, text: 37 % / 12 %); at κ 0.5 (the sliding speed of 30° rolling, 6.7 m/s) 0.84 / 0.95 of peak: sideways at 30° keeps about what braking keeps at the same sliding speed, wet and dry (+0.03-0.05) | Bergman, TRR 621 (1976) pp. 8-18, Figs 4, 5 (fig ±0.02), text (quoted) |
| Wet past the peak, shaved radial, belt | Calspan TIRF, Goodyear Eagle RSA P225/60R16 shaved to 4/32 in (3.2 mm), 34 psi, 0.46-2.3 klb, belt of 3M 80-grit sandpaper, 1.27 mm water, sweeps ±20° at 3°/s: dry 48 km/h μ_y(20°)/peak 0.74-0.86 (peak at 5-11°, the fall largest at light load); wet 48 and 72 km/h flat from 10-15° to 20° (~1.0); 97 / 121 km/h near hydroplaning (μ 0.1-0.35), flat or rising. Wet peak/dry at 48 km/h ~0.86 at 6.1 kN; wet over dry at 20° 1.10 (wet above dry). Sandpaper: sharp microtexture, no macrotexture (the authors: not a road) | Salaani, Heydinger & Grygier, SAE 2006-01-0559 (NHTSA copy), Figs 14-18 (fig ±0.03) |
| Sideways at 20° against locked, smooth tyre | SCRIM: smooth test tyre at 20° yaw, 2 kN, wet; the same tyres on a braked trailer (PFT) locked: the sideways readings continue the locked friction against sliding speed V sin 20° on 5 of 6 MIRA wet-grip surfaces (R² 0.88-0.99; e.g. ISO 2 asphalt SR 52 / 48.5 / 44 at sliding 10 / 17 / 27 km/h, locked Fn 41.5 / 36.5 / 36 at 30 / 50 / 80 km/h, fig ±1); the highest-friction surface was below its critical angle. Bird & Miller (1937, sidecar wheel): wet concrete at 32 km/h 0.745 at 16-19°, 0.71 at 21° (0.95); wet rock asphalt, 14 km/h, 0.38 → 0.33-0.35 | Sanders, TRL PPR980 (2021), Figs 7-2, 8-1 to 8-6, text; Sanders & Browne, TRL PPR957 (2020), Fig 8-4 (replot of Bird & Miller) (fig ±0.02) |
| Carcass against pressure | +0.5 bar: σy -12 %, CFy +9.5 %, CFx +3.5 %; the sets' 2.2-2.6 bar cover rally hot pressures (2.0-2.5 bar). Race tyre 325/660R13, manufacturer's static deflection at ~1.2/1.55/1.9 bar: d ln k / d ln p lateral 0.36, longitudinal 0.08, torsional 0.41, so +8 % / +2 % / +10 % over a rally tyre's +25 % (2.0 to 2.5 bar) | op het Veld (2008); Stocco, Biral & Bertolazzi, Math. Comput. Simul. 223 (2024) 654-676, doi 10.1016/j.matcom.2024.04.020, Fig. 10 (normalised at 2.25 bar; fig ±0.01, pressures read ±5 kPa) |
| Camber | camber stiffness Fz (-1.001 + 0.175 dfz)(1 - 0.81 dpi) per rad: 1.07 Fz at 4 kN, 2.6 bar (1.27 at 2.0 bar), 0.047-0.052 of Kyα at 2-6.75 kN (all at α = 0; 1.00-1.14 Fz at 1°); MF 5.2 truck 0.061 at its Fz0 9.4 kN (0.027 at 4 kN); brush (1-ε) a/(3R) with the camber reduction ε 0.4-0.7 for car tyres (0.023-0.047 at the Fiesta's a/(3R) 0.078; spin (1-ε) sin γ/R, Romano eq. 3a, Pacejka 4.76) | MF6.1 / MF5.2 sets; Romano et al., Meccanica 57 (2022) (quoted) |
| Pressure, MF 6.1 at 4 kN | +0.5 bar from 2.0-2.6 bar: Kyα -11 to -14 %, Kx -14 to -17 %, c_y +13-16 %, c_x +4.2-4.4 %, σ_y -21 to -26 %, peak μ +1.5-2.5 %; Goodyear: Cα falls with pressure at light loads and rises at heavy ones, peak μ no clear trend | MF6.1 (PPY*, PPX*, PCFY3, PCFX3); Goodyear arXiv 2305.18422 Figs 9-10 |
| Racing slick (rig) | WTCC Yokohama A005 250/660R18: Fy peak at 5-6°, 0.93-0.97 of peak at 10°, 0.89-0.94 at 14°; Cα/Fz 27 /rad new, +6 % at 0.75 mm wear; 1.9 → 2.1 bar: Cα +2.3 %, vertical stiffness 239 → 262 kN/m; grip optimum ≈40 K above track temperature on rig and car; degradation 0.1-0.2 s/lap. Its simulator test plan (Appendix C: baseline, one parameter per stint, fixed lap budget, cockpit A/B toggle) is the template for brush against NGP | Richardson, Chalmers MSc 2017:82 (Cyan Racing), Figs 17, 54, 55, 57 (fig; load not stated), §5.2.4 (quoted) |
| Racing slick on a car: the data | A2RL EAV24 (Dallara SF23 base, Yokohama Advan slicks, SF sizes 270/620R13 F, 360/620R13 R), Yas Marina, 2025-11-15 19:13 local (after dusk), one conservative autonomous lap (81.3 s, 50 Hz CSV decimated from the ~100 Hz state estimate). Wheel loads in daN or kgf (unstated; ±2 %): static sum 725, front share 0.42 against lr/L 0.437 from the estimator's own slip-angle geometry (lf 1.74, lr 1.35 m, L 3.09); downforce 0.246 v² (ClA ~4.1 m²); lateral and longitudinal transfer both give a CoG height 0.36-0.38 m at 740 kg (track 1.55 m assumed). The `ay_mps2` channel lags vx·r by 0.62 s (filtered; peaks 1.25 against 1.43-1.55 g kinematic), so lateral force uses vx·r + v̇y; Kistler accelerations are noise. Reach: car μ (m·a over the measured loads) 99th pct 1.12 combined, 0.91 lateral, 1.09 braking; α ≤ 2° front, 1.4° rear; κ ≤ 0.04 braking, ~0.04 traction (planner, ABS/TC). Tyres cold: surface 30-48 °C, TPMS air 44-47 °C, 2.21-2.28 bar absolute (~1.2 bar gauge). The fast laps (1.88 g lateral, 1.99 g braking from the lagged channel, so lower bounds) exist only as summaries; the .mcap files are not public | BoxBoxAI (GitHub azimsh3r/BoxBoxAI), `web/public/session_summary_good.csv`, `session_summary_fast.json`, `driving-data/README.md` and msg definitions (no licence, no data terms stated); arXiv 2602.08571 §1.2 (car, tyre make); Wikipedia SF19/SF23, Yokohama 2019 release (sizes; SF23's assumed the same) (inferred: reductions) |
| Racing slick on a car: lateral | axle Cα/Fz (secant of Fy/Fz on α, α ≤ 2°, forward and inverse fits within 5 %): front 30-32 /rad at 1.6-3.5 kN per tyre (flat with load: Cα ∝ Fz^~0.9, load confounded with speed), rear 53-56 /rad (falls with load, ∝ Fz^~0.3; rear α ~1° depends on lr, ±20 %; includes the limited-slip diff). Linear to μ_y 0.8 at 1.4° (front) with no measured fall: peak slip angle, the shape past it and peak-μ load sensitivity are out of reach. Brush full-slide angle 3μ/(Cα/Fz) with μ ≥ 1.1 gives front ≥ 6° (agrees with the slick rig's 5-6°). Against the brush: 22.9 at 4 kN (≈27 at 2.5 kN by its Fz^0.64): the slick is 1.15× stiffer at the same load, the rig slick (same Advan A005 family) 27; a treaded rally tyre sits between, so 22.9 stands. Force-on-slip lag: rear ≤ 0.02 s (one sample), front 0.08 s (steering compliance, not tyre relaxation) | same (inferred) |
| Racing slick on a car: braking, traction, combined | braking, front κ from the free-rolling fronts' radius (0.302 m + 1.2e-6 v²): car μ_x 0.29 / 0.45 / 0.60 / 0.78 / 0.90 at κ 0.0075 / 0.0125 / 0.0175 / 0.025 / 0.04 (drag 0.6-1.0 N/(m/s)² moves it ≤ 0.1 above 45 m/s, < 0.03 below 35), so Kx/Fz ≥ 29-36 (brush 26.8); κ held at 0.02-0.04 at constant pressure while μ_x rose with falling downforce: no peak reached (brush peak κ 0.09: not tested). Traction: rear κ against the fronts too scattered (±0.2-0.3 μ, κ = 0 at μ_x 0.4) to use. Combined: front Cα/Fz while braking at car μ_x 0.48 / 0.71 / 0.88 (κ 0.02-0.03) is 26-28 / 32-34 / 32-36 against 30-32 free rolling, no loss resolved (±15 %); an isotropic brush at μ 1.27-1.5 would keep 0.71-0.77 of it at μ_x 0.88: either the slick's peak μ is ~1.8 or more, or its lateral stiffness holds better under braking | same (inferred) |
| Racing slick on a car: temperature | nothing: one lap, surface temperatures within ~10 K and cold (track not measured, ~28-30 °C after dusk estimated, so 5-15 K above it against the rig's optimum ≈ 40 K above track); the fast laps' summaries (+3-6 K, 1.9-2.0 g) mix temperature with the planner's speed | same (inferred) |
| Gravel wet against dry (same road) | untreated gravel county road (Iowa), towed trailer, 19×4.75 bias tyres at 830 lb gross, surface sprinkled just ahead of the wheel: straight (locked) skid at 30 mph wet 0.68 smooth / 0.71 new tread against dry 0.64 / 0.60 (1.06 / 1.18), rising with speed both wet and dry; side skid (rolling wheel at ~15°) wet 0.59 / 0.62 against dry 0.61 / 0.63 (0.97 / 0.98); text: gravel and cinders "about the same wet or dry", the only surfaces so; the locked wheel ploughs and grips more than a braked rolling one, wet and dry | Moyer, HRB Proc. 13 (1933) pp. 123-168: synopsis (quoted), Figs 10b, 11a, 12a, 13a pp. 139-144 (fig ±0.02), p. 149 (text) |
| Gravel saturated against dry (one site) | rally-school hard-pack (angular crushed gravel, silty sand SM): CRREL vehicle's driving traction dry 0.86, wet 0.60 (0.70) after 3.3 cm rain, surface saturated, slurry pushed ahead; the wet day's locked-front braking coefficient 0.50 / 0.57 / 0.60 over three passes in one track. One car stopped on wet loose gravel among eight on dry (NHTSA, laden, 56 km/h): locked 25.5 m against 21.8-26.4 m (0.95 of their mean decel), ABS 34.1 against 26.9-33.3 m | ERDC/CRREL TR-10-11 p. 6 (quoted), Table 3 (table); NHTSA DOT HS 808 875 Table A2, Fig. A12 note (table; ratios inferred, one car, weak) |
| Reconstruction drag factors, gravel | Baker / Northwestern table: loose gravel dry 0.40-0.70, wet 0.45-0.75 (both speed bands); packed, oiled dry 0.55-0.85 / 0.50-0.80 (< / > 30 mph), wet 0.40-0.80 / 0.40-0.60; test basis unknown | Baker 1975, Fricke 1990 (book, not read: secondary reproductions, table); mfes.com on its basis (quoted) |
| Reconstruction tables, gravel (1983 print) | Baker's table as printed in 1983 (Ref. 1 p. 210, no basis given): gravel loose dry 0.40-0.70, wet 0.45-0.75 (< and > 30 mph; wet/dry of the band ends 1.07-1.13), packed, oiled dry 0.55-0.85 / 0.50-0.80, wet 0.40-0.80 / 0.40-0.60 (< / > 30 mph); Collins 1979 (Ref. 3 pp. 171, 187, car tyre, single values): gravel or sand 0.55, dry dirt 0.65, mud 0.40-0.50; Limpert's μ = μ0 - νV (dry peak 0.95, sliding 0.85, wet peak 0.75, sliding 0.70; pavement); the authors' own values are body-on-ground only (car on its top on gravel 0.5-0.7); no tests of their own on gravel, wet or dry; text: moisture may matter much on unpaved ground (for sliding bodies) | Warner, Smith, James & Germane, SAE 830612 (1983) App. 1 p. 12, App. 2 p. 13 (table, cited compilations), p. 10 (text) |
| Gravel-type unpaved roads wet against normal, braking | MgCl-treated unpaved roads (northern Chile, 30 sections), light truck braked as hard as possible from 40 km/h, accelerometer mean decel (drag factor; locked or ABS not stated), 3-10 replicates, CV 5.4 % mean over 222 tests; sprinkled ("water", surface soaked, slurry film) over normal on the same section: well-graded 0.68-0.93 (6 sections, mean 0.78), loose aggregate on a hard base 0.71-0.84 (4 tests, 0.77), sealed (fines-rich, smooth) 0.59-0.61 (3, 0.60); Table 4 means normalised to 30 % RH 0.567/0.707 = 0.80, 0.510/0.675 = 0.76, 0.458/0.745 = 0.61; against air humidity (fit, R² 0.5, normal surfaces): flat to ~35 % RH (0.71-0.73), 0.90 of the 30 % value at 50 %, 0.81 at 60 %, 0.69 at 70 % (the salt takes water from the air above 32 %); one untreated granular road 0.585 dry (one test, no wet); review: Paige-Green's gravel roads (South Africa, 50 km/h) dry 5.9 m/s² mean (4.1-8.0, 45 sections) against wet 5.3 (4.1-6.2, 6 sections): 0.90 (different section sets); Lea & Jones (California unpaved) 0.40-0.85, no wet split | González, Paniagua & Chamorro, TRR 2473 (2015) Tables 2-4 (table; ratios inferred), Fig. 3 (fit as printed), pp. 156-157 (quoted review) |
| Unpaved wearing course and water | slipperiness when wet comes from the fines: > 20 % passing 0.075 mm (silt, clay) dusty dry and slippery wet, < 10 % washboards and ravels; well-graded gravel with a little clay binds a hard tight surface; 12 % of 109 sections (Transvaal, Namibia) were periodically slippery after a number of passes in prolonged rain, all safe above 80 km/h dry; loose material after blading made it worse | Jones, UCPRC-GL-2017-03 App. A (text); Paige-Green & Netterberg, TRR 1106 (1987) pp. 208-214 (quoted, visual ratings) |
| Soil moisture and traction | tractor net traction, ~850 curves (Steinkampf): optimum at 15-20 % moisture, lower drier and wetter; fit κmax = 0.54 + 0.12 k - 0.1 k² over 5-30 % (k 0-1): 0.54 dry, 0.576 at 20 %, 0.56 wet (±7 %). Granular road base (non-plastic, OMC 9.6 %): c 80 kPa, φ 39° at optimum against 35 kPa, 31° saturated (shear strength 0.64 at 200 kPa, inferred); suction cohesion of unsaturated granular soil peaks at intermediate saturation | Schreiber & Kutzbach, Res. Agr. Eng. 54 (2008) 43-49, Fig. 4, eq. 20 (quoted, fit); Molina Gómez et al., Tecnura 20(49) (2016) 75-85 (text); search abstracts (not read) |
| Rubber friction | no static/kinetic pair: μ(v, T) with flash temperature | Persson, arXiv 1007.2713, cond-mat/0605273; Grosch 1963 (secondary) |
| Tyre heat: friction share, one node | half the friction power into the tyre (assumed); one-node τ 6.4 s; μ = 1.070 - 3.967e-3 T(°C) (-0.4 %/K at the surface); IR surface 25-80 °C on a drifting car | Kobayashi, Weber & Gerdes, arXiv 2407.12989 (text, fit) |
| Tyre heat: racing model | share fitted 0.49-0.68; road conductance 12 kW/(m²·K) over the non-sliding contact (from Kelly & Sharp); convection Nu = 0.0239 Re^0.805, L = 1/(1/D + 1/W); tread 0.5 kg × 2.4 kJ/(kg·K); tread 100-150 °C | West & Limebeer, VSD 2022 (text) |
| Rubber thermal properties | conductivity 0.2-0.3 W/(m·K); a passenger tyre warms over > 30 min; k 0.33, ρ 1190, c 2000; D ≈ 1e-7 m²/s | Gent & Walter, NHTSA DOT HS 810 561; Fina, PhD Surrey 2016; Persson, cond-mat/0605273 (text) |
| Rally tarmac tyre windows | RA9 20-60, RA7+ 30-70, RA7 50-90, RA5 70-110 °C | Pirelli RA range (maker's text) |
| Wet road cooling | tyres 6-7 °C cooler on a wet road | Ejsmont et al., J. Civ. Eng. Archit. 9 (2015) 1302 (text) |

Not found in open sources: rally tyre data, racing-radial data beyond the slick row (TTC data is licence-gated), the
full Gallaway 1979 tyre terms, gravel cut onto tarmac, a rally tyre's wet or damp grip, a measured surface
temperature rise and decay over one slide, Kelly & Sharp's own thermal parameters. Those
stay calibrated on stage times.

## NGP against the targets

- Matches: peak-μ load sensitivity (Fz^0.878 against 0.80-0.94), trail a/3, convection exponent, a temperature bell.
- Off: Cα load exponent 0.36 (0.56-0.91); Kx/Fz 14 at 4 kN (18.4-32.9), Kx ∝ Fz^0.35 (1.08-1.16); stiffness blind to
  temperature, pressure apart, and tread depth; dry wear lowers grip (tread loss raises it); wet grip fixed per
  surface (no speed or film effect); static/kinetic pair; peak slip angle 9.5° at 4 kN (6.5-8.5°); lateral transient
  0.04-0.10 m to 63 %, shrinking with speed (σ/R0 1.0-1.8 in the sets); the sticking model's 360 Hz alternation;
  tread warm-up of ~8 K in 20 s of hard cornering; a slide moves the surface a few K (19 % of the slip power reaches
  it; published models put half into the tyre).

## Candidate (brush-v1)

`rbr_tyre::brush` (`calibrate`; the lab's `examples/compare.rs`). Pacejka's brush with combined slip (ch. 3)
split as Svendenius splits it (adhesion along the stiffness-weighted slip, sliding against the sliding velocity;
below) and
Rill's first-order force dynamics (below). Cy ∝ Fz^0.6 at 1.3 times NGP's 0-2° slope (`brush::CORNERING_OVER_NGP`,
below), Cx = 1.4 Cy at Fz0 (`KX_OVER_KY`, so also 1.3 times that of NGP's slope) and ∝ Fz^1.2, both times bulk temperature
(-0.45 %/K), tread depth (2.6/(d + 1.6)) and NGP's pressure factor, and on a hard surface below NGP's grip 0.55
times grip/0.55 on NGP's slope's Cy and Cx (ice, [Ice](#ice-material-116); the 1.3 fades out from grip ~0.75 down to 0.55); peak Fy ∝ Fz^0.87, Fx ∝ Fz^0.93; friction
against sliding speed g(Vs) = (μ_C + (μ_S - μ_C) exp(-(Vs/7 m/s)³)) exp(-0.008 Vs), μ_C/μ_S 0.8, μ_C lowered
0.25 (1 - exp(-Vs/7 m/s)) at W = 1 (below); NGP's temperature grip kept; dry wear
+10 % at 60 % worn, fading with wetness; the wet peak's thin-film loss with speed and partial hydroplaning (below); force dynamics F = c u + d u', (v d + f) u' = f v_N s - v c u per axis (Rill,
Vehicle Dynamics lecture notes, Hochschule Regensburg 2009, 3.4.2 and 4.2.2; f = F/s the steady secant, s made with
v_N = max(v, 0.5 m/s), v itself the transport speed
 as in Rill's eqs. 4.20-4.21, so a locked wheel's deflection
follows the sliding and a stopped one's holds; c_y NGP's carcass and c_x 450 kN/m, times NGP's pressure factor over
its calibration value to the powers 0.7 and 0.6 (c_y as NGP's to the power 1.7; below), at every load (road sets 288-399 kN/m at 4 kN, measured 163-234, c_x/c_y 1.3-2.75); d the mean of
Rill's passenger values, d_x 500/1500 and d_y 250/600 N/(m/s)), combined force and the deflections capped at μ Fz as
Rill caps them; relaxation length (f + v d)/c, the stretched string's C/c at small slip, no other speed term; camber
thrust 0.05 of Cy per unit inclination sine (below). Calibrated
at run time to NGP's lateral peak and 1.3 times its slope, the longitudinal side on NGP's own slope, and its
longitudinal peak at 4 kN, 15 m/s (no NGP values in code); loose-v1 starts from the calibration on NGP's slope. The dynamics
reach standstill, and the game runs the brush down to it.

Combined slip (2026-11-07): the adhering part of the patch gives -C_i s_i (1 - ψ)² along the stiffness-weighted
slip, the sliding part μ Fz ψ² (3 - 2ψ) against the sliding velocity, μ Fz from ψ = 1 (Svendenius, Tire Modeling and
Friction Estimation, PhD, Lund 2007, eqs. 4.14, 4.17, 4.18-4.20, 4.26-4.28; read in full); the directional μ is the
ellipse along the sliding velocity, his collinear slide force. The aligning moment is his M'_z (eqs. 4.33-4.35):
μ Fz a (1 - ψ)² (θ_y s_y (1 - 4ψ) + 3ψ² s_y/|s|); the deformation torque M''_z, ∝ (C_y - C_x), stays out. Its a is
NGP's patch half length times (Fz/Fz0)^0.35 (2026-11-08, `brush::trail_half_length`): the MF sets' t0/a grows with load
(above), brush theory's a/3 holds at the design load only; forces unchanged (a enters Mz alone). On a hard surface
the trail is Hsu & Gerdes's affine one in place of M'_z's (2026-11-08, `brush::affine_trail`): t = a/3 (1 - ψ), Mz =
-t Fy, the same t0 and zero at full slide, Mz peak 0.158 μ Fz a at ψ 0.37 (M'_z: 27/256 at 0.25). They found the
parabolic brush's trail less accurate in practice than its force model and validated the affine one on their car's
steering torque to the limit (Y.-H. J. Hsu, PhD Stanford 2009, eq. 2.13 and ch. 4; IEEE TCST 18:4, 2010). Loose
surfaces keep M'_z. Pure
slip is Pacejka's cubic either way, so the curves, the per-wheel fit and the calibration are unchanged (lab CSVs
identical). Pacejka's own ch. 3 text (isotropic bristles, where both directions coincide) was not
reread here: the book is not open.

Friction against sliding speed (2026-11-08): the generalised Stribeck curve (Olsson, Åström, Canudas de Wit et al.,
Eur. J. Control 4, 1998) times Burckhardt's speed factor exp(-c4 Vs) (Kiencke & Nielsen's c4 0.02-0.04 s/m), its four
values fitted to TRL PPR496's car tyre (Published targets): the braked peak holds with speed while the locked
friction falls. Canudas de Wit's own fit (exponent 1/2, v_s 6.57 m/s, μ_C/μ_S 0.52) falls fastest over the peak's
1-3 m/s of sliding, so the braked peak fell 9 % from 32 to 100 km/h and the locked wheel kept 0.76 of it at 100.
Wet (2026-11-08): the wetness W (NGP's `WaterFactor`, at the seam) lowers μ_C by W · 0.25 (1 - exp(-Vs/7 m/s)): water
lubricates the sliding rubber the more the faster it slides (0.18 at 32 km/h, 0.23 at 64, 0.25 at 100), fitted to
TRL's wet locked over peak; the peak barely moves (Vs at the peak is in the curve's flat part). Partial hydroplaning
(2026-11-09, `tyre/wet-damp-worn`, `brush::partial_lift`) scales the whole friction by 1 - 0.95 λ: the grooves (d · 8
mm) and the road's texture (MPD 0.7 mm, within VTI 748A's 0.4-2 mm, ours) drain the film (h = W · 0.7 mm) below v_on
= 6.3 m/s · (drained depth / 0.7 mm) · (0.7 mm / h)^0.43, and the lifted share λ grows linearly from v_on to 2.2 v_on
plus 50 m/s · (texture / 0.7 mm) · (0.7 mm / h)^0.43 (the texture's asperities keep a lifted patch in contact), or
Horne's v_p at 200 kPa (90 km/h), the higher (0.05 left, Horne's μ ≤ 0.05). Fitted (least squares on worn over new,
each set at its own film and texture) to the drum (Williams & Evans, no texture, ~1.5 mm, 0.2-1 mm films), SWOV (2 mm,
0.3 / 0.6 mm sprayed) and Dijks (UK 2 / 1 / 0 mm on 0.5 mm, German 2 / 0 mm on 1.5 mm); the film exponent is the
drum's half-loss speed (h^-0.43). Residuals ≤ 0.08 but for SWOV's 0.6 mm cell at 100 km/h (0.97 against 0.82,
inferred from its means) and Dijks UK 2 mm at 130 km/h (0.92 against 0.83). On the game's road at W 1: a new tread
loses nothing to 280 km/h; worn over new at 2 mm 0.96 / 0.86 at 100 / 130 km/h, 1.6 mm 0.98 / 0.91 / 0.80 at 80 /
100 / 130, bald 0.92 / 0.83 / 0.74 / 0.65 at 40 / 60 / 80 / 100 (was v_on 2.1 m/s · drained depth / h, lifted at
5 v_on, fitted to ABS stops of unstated film: lost earlier and more than all three sets). Horne's v_p (water deeper than the grooves, ADAC's 17-22 % lost at 2 of 8
mm) is not the regime of the game's films and is not used; the seam carries no pressure. Thin film (2026-11-08, `tyre/wet-speed`): the whole friction falls by 0.0074 W per m/s over 60 km/h
(`brush::thin_film`, never below 0.05), fitted to Reimpell & Sponagel's wet peak at the game's 0.7 mm (0.5 / 1 mm
interpolated: 0.922 / 0.833 of 60 km/h at 100 / 140 km/h; the fit 0.918 / 0.836); v is the speed over the road, the
speed the patch meets the film at, locked or rolling (the sliding speed at the peak, 1-3 m/s, does not grow with
it). It takes over the new tread's own partial hydroplaning: the factor is thin_film × wet_loss(d) / wet_loss(new)
(`brush::wet_speed_factor`), so a new tyre takes the thin film alone (no 1.5 % at 100 km/h on top) and a worn one its
hydroplaning against new on top, as its fit is (worn over new). The game's tread starts new (`--wear` is the
road's) and wears little (map 41 wet, Fiesta, whole stage: sector wear 0.006-0.009, one sector per wheel 0.04-0.05
at the end; tread left ≥ 0.95), so in game the wet speed loss is the thin film's. Damp (2026-11-09, `tyre/wet-damp-worn`, a user decision):
`brush::damp_level` scales the friction by 1 - 0.098 at W 0.25, 1 at W 0 and 1, linear in W between (a blend of two
materials blends W and NGP's friction alike), so the damp peak is 0.82 of dry (NGP's values alone 0.909) and 1.04
of wet; wet keeps NGP's 0.79. Evidence: the loss from dry comes with the first water in the texture (Persson 2004;
SWOV 0.3 against 0.6 mm 0.995; drums 0.2 against 0.7 mm 1.00-1.055), inside Bachmann's 0.70-0.85.

Camber and pressure (2026-11-08): the inclination's sine (`ForceInput::inclination_sin`, read-only at the seam) adds
-0.05 sin γ |ωR| to the lateral slip velocity, MF's horizontal shift as an equivalent slip: C_γ = 0.05 Cy at small slip,
gone once the patch slides whole, none at standstill or locked (MF 6.1 0.047-0.052, MF 5.2 0.061 at its truck Fz0 of 9.4
kN, 0.027 at 4 kN, brush (1-ε) a/(3R) 0.023-0.047; NGP 0.1). The carcass stiffens with pressure faster than the slip
stiffness laterally and slower longitudinally: per +0.5 bar c_y +9.5 % (op het Veld) / +13-16 % (MF 6.1), c_x +3.5 % /
+4.2-4.4 %; NGP's factor rises 6.9 % from 2.0 bar, so c_y and c_x go as its powers 1.7 and 0.6 (the means), and σ_y =
Cy/c_y shortens with pressure.
A race tyre's static carcass (Stocco et al. 2024, Fig. 10) gives +8 % / +2 % over the same +25 %: the same split, at
the low end; the powers stay.
The slip stiffnesses keep NGP's factor: at 4 kN the sources disagree on Cα's sign (MF 6.1 -11 to -14 %, op het
Veld's σ_y and CFy -4 %, Goodyear +10 % per +20 % near nominal load), so σ_y shortens 5 %, not the sources' 12-26 %.
Kx keeps NGP's +4.4 % against MF 6.1's -15 % (one set). At the calibration pressure (factor 1) nothing moves; at
the Fiesta's 2.0 bar (0.967) c_y is 2.3 % below NGP's and c_x 441 kN/m (was 435).

Ours, not from papers: the friction curve's four values and the wet Coulomb loss and its speed scale (fits to one
tyre's trailer data, two surfaces); the texture depth 0.7 mm and the 8 mm new groove depth (both within the
published range), the wear gain's fade with W, the thin film's linearity in W (Reimpell's 0.2 mm keeps 0.99 / 0.97 at 100 /
140 km/h, W 0.25's 0.175 mm 0.979 / 0.959: within the figure's ±0.02, his loss grows faster than linear below 0.5 mm)
and its product with the worn tread's hydroplaning against new; leaving out σ's speed term (2026-11-07, user: the measured slope is a passenger tyre's, with its
excitation frequency mixed in, and puts a rally tyre above the cited 0.12-0.45 m, a motorcycle range);
taking the longitudinal exponents
and Kx/Ky as the mean of two road-tyre sets; the carcass damping as the mean of Rill's two passenger values; the
longitudinal carcass's 450 kN/m (above the road sets; c_x/c_y 1.07-1.37 against NGP's lateral carcass); NGP's
lateral carcass stiffness and temperature grip.

Against NGP and the targets on the Fiesta Rally2 (`compare` summary; brush-v0 in brackets where it changed):

- 2026-11-07, low speed (Golf II GTI 16V, track-41_M climb, realistic engine): the earlier transient (a lag on the
  slip, σ = C/c, rate V_r/σ) almost froze as the wheel locked (V_r → 0.5 m/s): after a spin-to-brake reversal the
  force kept the wrong sign 0.21-0.33 s (lab, fronts at 2.4-3.5 kN, 4.5 m/s, wheel 2.5 → 0.5 times the car) and drove a wheel-diff oscillation (FL +11 to -6 m/s, steady
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
  measured road tyre 0.2-0.3 and 0.29-0.34; NGP 0.40/0.46/0.61; was 0.20/0.24/0.35), Fy/|F| equal to the sliding
  velocity's share once sliding (0.269 at 4°; NGP 0.35); peak κ 0.12 drive / 0.10 brake at 4 kN (was 0.18-0.22); dry
  locked over peak at 10 m/s 0.80 (was 0.70); braking peaks within 4 % of NGP's (was 17 %); Kx/Fz 22 by the |κ| ≤ 0.04 fit, Kx ∝ Fz^1.14; peak Fx ∝ Fz^0.96; Cα ∝ Fz^0.64;
  Cα -21 % cold to hot and +27 % at 60 % worn; dry peak μ +12 % at 60 % worn; peak Fy ∝ Fz^0.85; σ_y 0.20 m at 4 kN
  (σ/R 0.63; NGP 0.17 m; Pacejka 0.12-0.45 m); with the damping 0.20-0.23 m at 5-30 m/s, 63 % of a 2° step in
  0.17-0.18 m at 10-30 m/s (was 0.18-0.21 m).
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
  brush has no bore torque, so it adds what the brush lacks rather than fighting it (the (1 - w) scaling is gone): every
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
  (2026-11-08; Golf II 447 kN/m, Fiesta Rally2 435 kN/m at their pressures), the same at every speed and load;
  trail and phase are the steady brush's.
- Runs below are `experimental-brush` (realistic engine + brush), NGP 7 in brackets; debug traces of the wheel speeds
  (not committed), 2026-11-07, each the last measurement before the 2026-11-08 friction curve:
  - Golf II GTI 16V, track-41_M climb (gear 2, 4-5 m/s, throttle 0.15-0.5, 10-13 s), both fronts over car speed
    mean/max 0.96/1.19 (0.95/1.07), no stall. Fiesta Rally2 8-20 s 0.97/1.47, Fiat 131 rears 1.08/3.03.
  - track-41_M 3-8 m/s on throttle: share of time the driven wheel runs over 1.1 times the car speed Golf / Fiesta /
    Fiat 0.128/0.102/0.114 (0.099/0.141/0.067); wheel-speed reversals 0.247/0.307/0.215 of steps (0.14/0.17/0.11);
    180-360 Hz power 7.8/60/11.6 (5.4/22/3.1); no instability.
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
  TRL dry 0.73-0.82, wet 0.64-0.67.
  Wet law of 2026-11-08 (Coulomb loss growing with Vs, partial hydroplaning), locked g at 32 / 64 km/h from
  80 km/h: track-41_M 0.767 / 0.575 (locked stop 0.631 g, was 0.662), 117 0.673 / 0.581 (was 0.675 / 0.618),
  122 0.608 / 0.599 → locked run hit the edge; from 50 km/h locked 0.727 (41_M, was 0.740), 0.644 (122, was 0.654),
  0.682 (117, was 0.690) g; best threshold within 0.02 g (new tyres, ≤ 80 km/h). NGP from 80 km/h locked at 64:
  0.775 (122), 0.796 (117). Autopilot pace on wet track-41_M: Fiesta Rally2 371.44 s (was 371.74; NGP 381.43),
  Celica ST185 375.93 s (NGP 385.34).
- Wet and damp against the research (2026-11-08, `tyre/wet-research`, main fa211dfb, Fiesta Rally2, no model change).
  `--wetness damp` on track-41_M puts the road on tarmac fine clean damp (38: μs 1.2025, WaterFactor 0.25), wet on 39
  (1.105, 1); RSF 117 the same way (65/69 → 66/70 → 67/71, the same 0.925 / 0.85 and 0.25 / 1). In game, with the
  tarmac tyre's own wet slope, the best of ABS K 0.15 and threshold braking from 50 km/h (41_M) over dry: ngp damp 0.915,
  wet 0.80; eb 0.915, 0.78; from 100 km/h (117, threshold K 0.2 / ABS): ngp 0.91 / 0.92, 0.77 / 0.75; eb 0.91, 0.78 /
  0.74. Against damp 0.70-0.85 of dry: MISS (both; NGP's surface values); wet 0.65-0.80: PASS. Wet over dry no lower
  from 100 than from 50 km/h (hydroplaning only past 94 km/h on new tread): against the wet peak's 0.89-0.94 at 100 of
  its 60 km/h value (0.5-1 mm films): MISS (lab: eb peak 100/60 0.985, 140/60 0.89, band 0.78-0.87; ngp 1.00 at any
  speed). Damp flat with speed: PASS (eb peak 0.99 to 50 m/s). Locked damp over dry, 50 km/h: ngp 0.90, eb 0.88
  (0.67-0.78: MISS). Worn, lab, eb at 2 mm tread over 20 km/h: 0.83 / 0.75 / 0.58 at 50 / 60 / 80 (Klempau 1 mm
  film, smooth road: 0.90 / 0.82 / 0.59). Map 41 autopilot pace (s, dry / damp / wet): ngp 309.01 / 338.51 / 381.43,
  eb 303.66 / 330.92 / 371.30 (wet +23 %, damp +9 %, both physics; the speed plan's, no rally reference).
- Thin film (2026-11-08, `tyre/wet-speed`, against main 0fb71ed8, Fiesta Rally2). Lab (tarmac wheel, 4 kN, new): eb
  braked peak at 100 / 140 km/h over 60 wet 0.918 / 0.837 (was 0.984 / 0.884; Reimpell 0.89-0.94 / 0.78-0.87:
  MISS → PASS), damp 0.978 / 0.958 (was 0.999; 0.2 mm 0.99 / 0.97), 180 km/h wet 0.755 (was 0.784); dry bit-equal; worn (2 mm) wet
  over new unchanged below 94 km/h, so 2 mm at 50 / 60 / 80 km/h over 20 0.83 / 0.75 / 0.56 (was 0.58 at 80;
  computed: the thin film's 0.959 at 80; Klempau 0.90 / 0.82 / 0.59). In game, best of threshold and ABS over dry: from 50 km/h (41_M) unchanged (damp
  0.915, wet 0.78: no loss below 60 km/h); from 100 km/h (117, threshold / ABS) damp 0.91 / 0.91 → 0.90 / 0.90, wet
  0.78 / 0.74 → 0.75 / 0.72, so wet from 100 is 0.96 of wet from 50 (was 1.00). Map 41 autopilot pace (s, damp /
  wet): Fiesta 330.92 / 371.30 → 330.90 / 371.32, Golf II 340.79 / 381.92 → 341.15 / 381.95 (Golf dry 313.78; ngp
  324.24 / 353.52 / 398.24): 41_M wet tops out at 88 km/h and the autopilot brakes above 60 km/h 4-6 % of the time.
- Damp level and worn refit (2026-11-09, `tyre/wet-damp-worn`, against main d903fe5a, Fiesta Rally2, experimental-brush).
  Lab (tarmac wheel, 4 kN, braked peak over dry new at 10 m/s, at 10 / 30 / 50 m/s): damp new 0.908 / 0.880 / 0.847 →
  0.820 / 0.795 / 0.765; wet new 0.788 / 0.707 / 0.592 unchanged; wet 3 mm 0.788 / 0.439 / 0.128 → 0.788 / 0.709 /
  0.489, 2 mm 0.745 / 0.264 / 0.038 → 0.789 / 0.661 / 0.411, 1.6 mm 0.705 / 0.151 / 0.038 → 0.789 / 0.626 / 0.373; dry
  bit-equal. Scorecard: damp over dry 0.909 → 0.821 (0.70-0.85 MISS → PASS), damp over wet 1.153 → 1.041 (1.00-1.06
  MISS → PASS); the worn stop rows (now context) 0.680 / 0.894 / 0.510 → 1.001 / 1.003 / 0.981; nothing else moved.
  In game, map 41 from 50 km/h, best of ABS K 0.15 and threshold over dry: damp 0.915 → 0.837, wet 0.783 unchanged
  (traces of dry and wet bit-equal); locked damp over dry 0.884 → 0.807 (0.67-0.78: still MISS). Pace, map 41 damp:
  330.90 → 331.32 s (wet 371.32 unchanged).
- The Fiesta's 0.35-pedal stop (1.63 s against NGP's 2.30) is the engine's, not the tyre's: realistic engine with
  NGP's tyre (`experimental`) 1.65 s; under NGP the car holds 2.93-2.96 m/s for 0.75 s in second gear at 123-125 rad/s
  until the autopilot shifts down, the realistic engine falls to 107 rad/s and keeps braking (2026-11-07).
- Ramped 40 m circle (map 85, `--circle -260,120,40 --circle-ramp 0.25`): NGP and the brush alone hold the same
  steering to the same limit, 8.6 m/s², then both run wide at full lock; the autopilot's steering steps (0.008) hide
  any linear-range difference. Steady state matches by the fit; the differences are in transients, combined slip,
  speed, load and temperature (lab tables above).

- Past the limit (2026-11-08, `harness/limit_recovery.py`: map 85 r40 circle, every material as tarmac 37
  (the former `--material-as 37`: the circle crosses loose gravel 189/181 at its south-west, μs 0.875, soil 6.5 mm,
  which loose-v1 takes under the brush), ramped at 0.5 m/s per s until the car first runs 1 m wide, then `--circle-past
  T` holds throttle T with the pursuit steering on; RSF 568's start straight resets the car 2.4 m off its line, so no
  open-loop step steer at speed). Limit 77-81 km/h, 1.19-1.28 g in every physics. After a lift (T 0): lateral g kept
  (lowest 0.25 s mean in 1.5 s over the limit's) / widest m / back within 0.5 m s / peak sideslip °, ngp; experimental
  (NGP's tyre); brush before V_SHAPE; brush with it (5fff12c2): Fiesta Rally2 0.89/1.6/1.7/5.6; 0.89/2.0/2.0/4.7;
  0.80/4.2/3.1/2.9; 0.79/2.8/2.6/3.3. Golf II 0.79/2.0/2.3/3.3; 0.75/3.0/2.8/3.3; 0.72/6.4/4.1/3.6; 0.66/6.0/4.0/3.4.
  Escort Mk II: experimental 0.79/2.2/2.4/52 (ngp never ran wide on the ramp); brush spins (180°) before and after. Full
  throttle (T 1): the Golf spins under the brush before (180°) and after (99°), not under NGP's tyre (17-29°); the
  Fiesta spins under ngp and the brush. A scratch build with the lateral friction flat with sliding speed (bound, not
  committed: μ_y(30°)/peak at 15 m/s leaves the band) gives 0.91/1.7/1.7 (Fiesta), 0.87/2.9/2.3 (Golf), the Escort back
  in 1.8 s without spinning, and a limit 4-7 % above NGP's: on clean tarmac at 80 km/h the friction's fall past the peak
  is what makes the brush less forgiving; V_SHAPE takes the speed out of it and leaves the fall with slip the 15 m/s
  curve has. Not the cause there: carcass dynamics, camber thrust, Cα's load exponent, Kx/Ky, μ's load exponent, trail
  (scratch builds, each within 0.7° of sideslip on the stage's own surface). On the stage's own surface (with the
  gravel) the brush's car slid rear first (onset sideslip 11-13° against 1-2°): that is loose-v1 against NGP's soil on
  the patch, not tarmac.
- Open-loop handling on a flat pad (2026-11-10, `harness/handling.py --out DIR`, release `rbr-standalone`; 9 cases,
  596 runs incl. probes, 15 min at `--jobs 8`): `--stage pad-tarmac` (a generated 2 km square of level faces,
  `rbr_btb::pad`; every material takes tarmac 37's values), `--manoeuvre KIND,KMH,AMP[,EXTRA]` (`autopilot/pad.rs`) holds the start heading, brings the car to
  the entry speed in the gear the shift points give, and after 2 s of steady speed runs the schedule open loop with the
  exact float steering (the controller's 8-bit steps are 0.008). Metrics and their definitions: the script's docstring.
  Ramp steer at 80 km/h (0.015 per s of lock, 0.08-0.14 g/s): peak g ngp / experimental / brush: Fiesta 1.248 / 1.249 /
  1.253, Golf 1.186 / 1.186 / 1.223, Escort 1.193 / 1.185 / 1.242; steer at 0.9 of the peak 0.248 / 0.248 / 0.213, 0.188
  / 0.188 / 0.183, 0.127 / 0.126 / 0.128; sideslip at the peak 3.5 / 3.5 / 1.7°, 3.8 / 3.8 / 2.0°, 10.8 / 10.6 / 5.7°;
  limit: Fiesta and Golf hold it understeering in every physics, the Escort's rear lets go (spin) in every physics. Step
  steer to 0.9 of the peak (yaw-rate t90 s / overshoot %): Fiesta 0.17 / 12 – 0.17 / 12 – 0.15 / 8.5, Golf 0.18 / 8 – 0.18 /
  10 – 0.17 / 8, Escort 0.44 / 7 – 0.51 / 6 – 0.19 / 9. Sine dwell, lowest steer (fraction of lock) that spins at 80 /
  110 km/h: Fiesta none / 0.55, none / 0.55, none / none; Golf none / 0.55, none / 0.55, none / 0.3; Escort 0.3 / 0.3, 0.3
  / 0.3, 0.2 / 0.15 (ngp, experimental, brush); the outcome flips between the ±2 km/h neighbours in places (Golf ngp
  110 km/h at 0.4: 1 of 3 spin; Escort ngp 80 km/h at 0.4: 2 of 3). Lift in turn (yaw-rate change at 1 s, °/s; circle at 0.9
  of the peak, the Escort at 0.6, the highest share it holds in ngp and experimental, g 0.75): Fiesta 7.9 / 4.9 / 1.8,
  Golf 1.8 / 1.6 / 3.1, Escort 0.0 / -0.2 / -0.5. Power in turn (0.8 of the peak, the Escort 0.6): Fiesta -2.1 / -2.0 /
  -0.3, Golf -4.8 / -3.9 / -2.5, Escort spins 3 of 3 in all three. Brake in turn (0.3 / 0.6 / 0.9 of the straight-line
  limit pedal; yaw-rate ratio at 1 s, light / medium / hard): Fiesta ngp 1.84 / 0.79 / 0.47 (no spin), experimental 1.86
  (spins 3 of 3) / 0.90 / 0.43, brush 2.07 (spins 3 of 3) / 1.24 / 0.34; Golf ngp 0.97 / 0.51 / 0.12, experimental 0.98
  / 0.54 / 0.20, brush 1.60 / 0.38 / -0.09; Escort 0.89 / 0.62 / 0.29, 0.86 / 0.61 / 0.28, 1.00 / 0.68 / 0.43 (none
  spin).
- The pad against the brush's tuning (2026-11-10, `handling.py --only ramp,sine --extra="--tyre-tuning KEY=V"`): sine
  dwell spins out of 24 runs (8 amplitudes, entry speed and ±2 km/h), Fiesta at 110 km/h / Golf at 110 / Escort at 80
  / Escort at 110: ngp 9 / 4 / 11 / 16; brush 0 / 9 / 14 / 17; slide grip 0.75: 0 / 7 / 12 / 14; 0.90: 7 / 12 / 12 /
  20; 0.95: 9 / 15 / 15 / 17; stiffness 0.77 (NGP's slope): 8 / 12 / 12 / 18; response 2.0: 3 / 9 / 15 / 16; no car
  spins at 80 km/h but the Escort. More slide grip spins the cars more here, not less: the front keeps turning the
  car in. The brush against NGP is not one-sided (Fiesta fewer, Escort about the same, Golf more at 110 km/h), and
  no tuning value brings the Golf to NGP's 4: its cause is not the slide grip, the stiffness or the response.
  The slide grip stays the user's 0.83. Open: the Golf at 110 km/h, throttle released (front axle under engine
  braking and side force together); the Fiesta's spin under a light brake in a turn with the realistic engine and
  NGP's tyre (3 of 3, ngp none), not looked into.
- The pad after the temperatures and the pressure slope (2026-11-10): sine dwell
  spins unchanged (Fiesta 0, Golf 9 at 110 km/h), ramp peak g within 0.2 %, step and circle manoeuvres within the
  runs' ranges; the temperatures show only in slides of seconds (`limit_recovery`).
- Gravel pad (2026-11-10, `handling.py --material 17 --extra "--surface gravel"`, ngp / brush): ramp peak Fiesta
  0.913 / 0.931 g, Golf 0.891 / 0.919, Escort 0.882 / 0.961; sine dwell spins of 24 at 80 km/h Fiesta 5 / 4, Golf 0
  / 7, Escort 12 / 12, at 110 km/h 9 / 9, 12 / 12, 6 / 9. Brake in turn (0.9 of the peak, 80 km/h): the Fiesta spins
  under the brush at the medium and hard pedal (yaw ratio at 1 s 1.95 and 1.47, ngp 1.39 and 0.93, no spin); the Golf
  holds a sideslip of 14° under the brush against 5-9°, and its step steer at 0.9 of the peak 18.5° against 4.3°. The
  brush's rear is the looser one on gravel as on tarmac, the Golf's most. Open, with the Golf on tarmac.
- The Golf under the brush, traced (2026-11-10, per-axle probes, `TYRE_SHARE` 0.5 / 0.25 / 0 builds):
  - Sine dwell at 110 km/h: 9 of 24 spins at every heat share (ngp 4), and still from amplitude 0.3 with stiffness
    0.77, slide grip 0.95, response 2.0, the grip matched to NGP's peak g or the load exponent 0.80: none of them is
    the cause. The brush car yaws through the reversal 0.07-0.1 s sooner and holds 57-58 deg/s in the dwell where
    ngp decays; both axles run 2.5° further past a peak that comes at 7° (ngp 10°) and falls 5-9 % by 20-30°. The
    rear axle sits on one wheel (inside rear 13-49 N). The trait is not isolated.
  - Full throttle past the limit (`limit_recovery`): the front ploughs at 29-31° of slip (full lock held by the
    pursuit), the rear at 2-4.5°. Front Fy/Fz ngp 0.88-0.90, brush 0.70-0.74, without heat 0.76-0.79: the brush's
    curve at 30° under drive takes about 12 %, the heat (370 → 398 K in 4 s) about 8 %. Widest 8.3 m ngp, 23-27 m
    brush at any share; never back under either. The spin at the end follows the plough.
  - `ice-car-coast-crr` 0.002 → 0.022 is the measurement: the autopilot's coast on ice goes into a lateral slide at
    56 km/h on a chaotic edge (ice=1.01 → 0.059, 1.02 → 0.0024); the temperatures stay 325-337 K. `coast_test`
    should drop sliding pieces.
- The sine dwell against NHTSA's (2026-11-10; DOT HS 809 974, Forkenbrock et al. 2006, read): 50 mph, throttle
  released, amplitudes from 1.5 A up to the greater of 6.5 A and 270° (A the handwheel angle of 0.3 g), criteria yaw
  rate ≤ 35 % of its peak 1.0 s and ≤ 20 % 1.75 s after the steering ends. With ESC fully disabled every car but one
  (a Toyota Tacoma) of the 58-vehicle set broke them, FWD saloons and minivans included; three heavy pickups passed
  in one steer direction only (§6.3, quoted). Ours at 80 km/h on tarmac in A (0.75 of the steer at 0.4 g), first
  amplitude breaking the criteria, ngp / brush: Escort 5.3 A / 5.2 A, Fiesta Rally2 6.8 A / none to full lock (18
  A), Golf none / 8.7 A only. Road cars on 0.85 g tyres are over-driven 2.3× at 6.5 A, ours on 1.25 g tyres 1.6×
  (inferred): the same over-drive is 9.5 A. So a car that never breaks them is the unusual one: the Golf under ngp
  and the Fiesta under the brush. No test at 110 km/h in the report.
- The same paper as the pressure slope's (arXiv 2305.18422, read): Cα falls with pressure at low loads and rises at
  high loads (text at Fig. 9); up to the nominal load it falls (-19.5 % at 67 %, -12 % at 100 %, the scorecard
  row), and through zero at about 150 % (-3.6 % at 133 %, +4.0 % at 167 %). `SLIP_PRESSURE` is the power at 4 kN
  and fades with the load to none at 6.3 kN (`SLIP_PRESSURE_ZERO_LOAD`, 1.5 times the 20/65-18's 430 kg, for every
  tyre: guess), none above (2026-11-10; the rise there unmeasured on a rally tyre). Cα per +0.5 bar: -5.7 % at 4
  kN, 0 at 6.3 kN (`calpha_dp05_6k3`, row `tarmac-calpha-pressure-high-load`). Cx takes the same fade, with no
  source for it. Every car runs the reference 2.0 bar, so only a changed setup shows it.
- The Fiesta Rally2 never breaking the criteria under the brush (2026-11-10, per-axle probes, 0.4 of lock at 80
  km/h): the rear holds its full force at 5.5-8° of slip (Fy/Fz 1.20-1.24) where NGP's needs 9-18°, so the sideslip
  peaks at 5° against 15° and the yaw rate is gone 0.2 s after the steering ends (ngp 1.4 s). That is the cornering
  stiffness at 1.3 times NGP's slope (sourced and driven, below): with `stiffness=0.77` the Fiesta spins 8 of 24 at
  110 km/h, ngp 9. Not a fault.
- Brake in turn, Fiesta Rally2, traced (2026-11-10, per-wheel probes; the centre is rigid, the axles' mean speeds
  equal, the inside wheels lock first and the outside ones carry on at κ -0.1 to -0.2):
  - Tarmac, light pedal (0.5 g; brush spins, ngp reaches 47°): no wheel locks. The outside rear carries the rear axle
    (inside 100-900 N) and runs at 21-29° of slip, where the brush's curve has fallen 6-9 %: Fy/Fz 1.05 and 0.99
    against 1.15 and 1.11 at 1.1-1.5 s, about +720 N·m of yaw moment. Slide grip 0.90-0.95 and stiffness 0.77 leave
    it. Medium and hard do not spin: the front (79.7 % of the brake torque against 64-70 % of the load) goes first.
  - Gravel, medium pedal (brush spins, yaw ratio 2.07 against 1.39): the front's lateral force under the brake, +10
    % (6641 against 6044 N), the rear equal. At κ -0.15 and 20° loose-v1 gives Fy/Fz 0.71, Fx/Fz 0.29, NGP 0.66 and
    0.39: the same |F| 0.77, loose-v1's along the slip velocity, NGP's 9° nearer the wheel's heading. Soft soil data
    (Krick via El-Razaz, α 25°, 35 % slip: Fy 0.67 of the lightly braked, |F| 0.87): along the slip velocity gives
    0.70 (inferred, one point, a tractor tyre).
  - Gravel, hard pedal (2 of 3 spins): the berm and the held longitudinal peak on the locked wheels; `PLOUGH` 1.0
    gives 0 of 3 (yaw ratio 1.35), the peak falling to 0.82 gives 1.02. At the light and medium pedal they change
    nothing.
  - Brake balance: tarmac 79.7 / 20.3 % front / rear, gravel 71.1 / 28.9 %; front load share 0.64-0.74. No measured
    yaw ratios of real cars found (ISO 7975 gives the method only).
- Rolling speed and the friction (2026-11-08, `brush::V_SHAPE`): above 16.7 m/s rolling (MF 6.1's LONGVL) the
  friction's sliding speed is the theoretical slip times 16.7 m/s, so a rolling tyre's curve against slip keeps its
  15 m/s shape at speed; a locked wheel keeps TRL's fall. Scorecard 84/112 (was 83; TRL peak 100/32 0.986, was 0.968,
  MISS → PASS; locked over peak at 100 km/h 0.656, wet 0.495, wet peak 100/32 0.970, all in band); map 41 600 s
  Fiesta Rally2 unchanged at 304.03 s, Golf II 314.09 (was 314.11; ngp 309.01, 324.24).
- Lateral past-peak shape (2026-11-08, `brush::LATERAL_FALL` 0.65, user decision): the lateral axis takes 0.65 of the
  friction's fall with sliding speed, the longitudinal axis all of it, so a locked wheel braking straight keeps the TRL
  fit. μ_y(30°)/peak at 4 kN, 15 m/s 0.893 (was 0.833),
  chosen at the band's upper end (0.73-0.90) on weaker sources: TRR
  621 (bias tyre, flat belt, 48 km/h) ~1.0 at 30°, Persson 2011 (model) 0.97 at 25°, NGP 0.865, against the road MF
  sets' full slide 0.73-0.83. Scorecard 85/112 (was 83): μ_y(30°)/peak +0.060 (PASS), peak slip angle 9.75°
  (was 9.50; MISS, band 5.6-8.5), Mz to 0 at 1.10 of the Fy peak's angle (was 1.13), Mz peak at 0.282 of it
  (was 0.289; MISS), peak μ_y per +0.5 bar 0.18 % (was 0.27), ice μ_y(20°)/peak 0.940 and end over max 0.963
  (was 0.907 / 0.943; both MISS before; the ice tyre is the tarmac brush), ice locked over threshold 0.812
  (was 0.807; MISS → PASS); TRL locked rows
  and the brake-test car rows within 0.005 of 5fff12c2. Clean-tarmac recovery after a lift (kept g / widest m / back
  s): Fiesta 0.87/2.1/2.1 (5fff12c2 0.79/2.8/2.6, ngp 0.89/1.6/1.7), Golf 0.74/4.0/3.2 (0.66/6.0/4.0; ngp
  0.79/2.0/2.3), Escort 0.77/4.1/never, peak sideslip 97° (spun 180° before; experimental 0.79/2.2/2.4, 52°); full
  throttle: Golf 38° and back on line never (99° spin before; ngp 17°), Fiesta 39° and back in 4.8 s (54°, never),
  Escort still spins. Limit 82.8/78.7/76.4 km/h, 1.29/1.25/1.19 g (ngp Fiesta 1.27, Golf 1.20). Map 41 600 s: Fiesta
  304.00 s, Golf 314.09.
- The stiffness level against the library (2026-11-10, `build/tyre-night/sources`; Cα/Fz per rad at 4 kN, MF sets
  computed from PKY1, PKY2, PKY4 at their nominal pressure): 1970s 78-series road tyres 9.2-12.6 (Gent & Walter Figs
  8.68, 8.70, measured, 0.16-0.22 per deg); 205/60R15 11.5 (Pacejka 2nd ed. Table A3.1) and 13.3 (`tno-205-60r15`);
  225/50R18 20.4 (`continental-225-50r18`); 255/55R20 22.2 (`mf61-example`; 23.1 / 20.9 at 2 / 6 kN); semi-slick
  R888R 195/50R16 21.9-22.3 (`megaride-2024`, measured); A005 slick 27.2 (`chalmers-richardson-2017` Fig 54); a
  racing radial 34 (MF/model); FS slicks 31-39 at 0.6-1.1 kN. NGP 17.6, brush 22.9. The stiffness rises as the
  profile falls: the brush is at a modern 50-series road tyre and the semi-slick, NGP between a 60-series and a
  50-series road tyre. No rally tarmac tyre is measured; an 18 in 40-series one may be stiffer still.
- A rally tarmac tyre's stiffness, estimated (2026-11-10): the Rally2 tyre is 20/65-18 (Michelin Pilot Sport A:
  200 mm tread, 650 mm across, so a 96 mm sidewall; road legal in Europe, "reinforced casing"), the R888R
  195/50R16 has 97 mm and the A005 slick 190/580-15 100 mm (size from a forum post, inferred to be Richardson's).
  At equal sidewall and width the slick's 27.2 over the semi-slick's 22.1 is +23 %, and Richardson's +6 % per 0.75
  mm of wear makes that 2.9 mm of tread: the tread's thickness accounts for the gap, the carcass for none
  (inferred). A moulded rally tyre (grooves 5.5 mm) new is then at the semi-slick's 22-24, rising about 8 % per mm
  worn: the brush's 22.9 stays. Sensitivity on the pad, stiffness 1.0 / 1.1 / 1.2 (22.9 / 25.2 / 27.5): peak g
  equal, steer at 0.4 g Fiesta 0.072 / 0.070 / 0.068, yaw t90 0.16 / 0.15 / 0.14 s, sine dwell spins at 110 km/h
  Fiesta 0 / 0 / 0, Golf 9 / 9 / 6, Escort 17 / 17 / 17.
- Rally tarmac tyres against semi-slicks and slicks, searched (2026-11-10, seven languages; part indexes
  `INDEX-rally-grip-1.md`, `-2.md` in the source library): no measured same-car test includes a rally tyre, and no
  logged g of a tarmac rally car is open. Drivers put a rally tyre at a Toyo R888 (Michelin N20) to clearly above
  it (Dunlop moulded slick), a circuit slick above both (subjective). Measured, not a rally tyre: i30 N laps 98.02
  / 94.81 / 91.79 s on a road tyre / Trofeo R / slick (`tyrereviews-i30n-slick-wayback`). Quoted, page unread: a
  2005 World Rally Car on Michelin's asphalt tyre "up to 1.5 g" in fast corners, 1.8 g braking. Makers' data: 20/65-18
  max load 430 kg, hot 2.0-2.3 bar, camber under 2° (`michelin-pressure-2024`); Pirelli RA 235/40R18 tread 210 mm;
  Hankook Z215 210/650R18 tread 218 mm, depth 5.5 mm; no stiffness or friction published. NGP's 1.25 g and the
  brush's 22.9 stay: nothing measured moves them.
- Two racing-tyre papers read (2026-11-10; library `kasprzak-lewis-milliken-2006`, `pottinger-pelz-falciola-1998`):
  - SAE 2006-01-3607, a full-size radial racing tyre on Calspan's belt, 200-1800 lb at 20-38 psi (Figs 15-18, fig
    ±10 %, the stiffness surface ±20 %): lateral peak at 4.5-6° at 4.4-8 kN; 3-12 % under the peak at 10.5°; μ about
    2.0 / 1.5 / 1.35 at 0.9 / 4.4 / 8 kN (belt); Cα/Fz about 37 per rad at 4.4 kN, 25-32 at 8 kN. At 8 kN more
    pressure gives more force and slope (peak +17 % from 20 to 38 psi), on the FSAE tyre at 0.7-1.6 kN less: the
    sign turns with the load as Goodyear's. A rally car's outside wheel (5.8-6.4 kN) is over the 20/65-18's 430 kg,
    so `SLIP_PRESSURE`'s fall had the wrong sign there: it now fades to none at 6.3 kN (see the pressure slope).
  - SAE 982747, a 350/710-18 IMSA radial, Fy against Fx at 2° and 4° (Figs 20-21, normalised, fig ±0.1): at 2° the
    side force holds to about 0.8 of the peak braking force, at 4° about 0.8-0.85 of it is left at half. Lab at 4
    kN, Fy/Fy(0) at 0.5 / 0.75 of the peak braking force: 2° brush 0.96 / 0.88, ngp 0.82 / 0.64; 4° brush 0.93 /
    0.80, ngp 0.86 / 0.71. The brush follows the 2° curve; the 4° reading does not separate them.
  - For the brush: the peak angle (7.0° against 9.75°) and a fall past it have a measured racing tyre behind them,
    which falls more than the brush did (1.2 % at 10.5°); the stiffness estimate's range opens upward (22 to about
    30, never under the semi-slick).
- The fall past the peak sets in sooner (2026-11-10, a user decision: a rally tyre no sharper than the slick, so its
  gentlest 3 % at 10.5°): the lateral axis takes the Stribeck exponent 1.25 (`brush::LATERAL_STRIBECK_EXP`) in place
  of the locked wheel's 3, the slide grip solved as before. Lab at 4 kN: μ_y at 10.5° over the peak 0.969
  (was 0.988; row `tarmac-mu-y10.5-over-peak`), peak angle 6.5° (was 7.0°), peak μ_y 1.263 (was 1.276; ngp 1.268; at
  6 kN 1.191, was 1.208; ngp 1.185), μ_y at 30° 1.064 both (0.843 of the lower peak, was 0.834), at 40° 1.018
  (was 0.999). Braking, gravel and
  combined slip's rows as before. Pad, tarmac 80 km/h, ramp peak g Fiesta Rally2 / Golf / Escort: 1.224 / 1.195 /
  1.228 (was 1.253 / 1.221 / 1.242; ngp 1.248 / 1.186 / 1.193); sine dwell spins of 48: 0 / 9 / 28 (was 0 / 9 / 31),
  the Fiesta's peak sideslip at 110 km/h 28° (was 48°); lift, power and brake in turn spin as before.
  The step steer's low
  overshoot flips between about 2.5 and 10 % from run to run (before this too): not a measure.
- Combined slip against a measured road tyre (2026-11-10; library `xia-lu-tong-2020`, SAE 2020-01-0896: a 195/55R16
  at 225 kPa and 60 km/h on an MTS Flat-Trac, slip ratio to ±30 % at 2°, 5°, 8°, Figs 4-6, whose axes are Fx across
  and Fy up whatever the labels say; fig ±0.03). Fy/Fy(0) at 0.5 / 0.75 of the pure peak braking force, and the
  largest Fx over that peak, at 4.8 kN (lab: `force_sweep --long --fz 4800 --speed 16.7 --alphas 0,2,5,8`):

  | | 2° | 5° | 8° |
  | --- | --- | --- | --- |
  | measured | 0.99 / 0.93 | 0.89 / 0.70, 0.86 | 0.83 / 0.52, 0.77 |
  | brush | 0.97 / 0.91 | 0.90 / 0.68, 0.88 | 0.84 / 0.59, 0.81 |
  | ngp | 0.78 / 0.62 | 0.82 / 0.62, 0.91 | 0.84 / 0.65, 0.84 |

  At 30 % slip the measured Fy/Fx is 0.11 / 0.27 / 0.44 (0.125 / 0.29 / 0.45 at 2.4 kN) against the slip velocity's
  tan α / κ = 0.116 / 0.292 / 0.468, which the brush gives exactly; ngp 0.169 / 0.415 / 0.653. So the sliding force
  along the slip velocity is measured, and NGP gives side force away too early at small angles and keeps too much of
  it in a deep slide. The tyre itself: Cα/Fz (2° secant) 17.9 / 15.4 per rad at 2.4 / 4.8 kN, μ_y 1.13 / 0.97 /
  0.89 and μ_x 1.17 / 1.18 / 1.14 at 2.4 / 4.8 / 7.2 kN. A road tyre on a belt: the shape carries, the levels do not.
  Scored (`data/tyre-targets.csv`, rows `tarmac-brake-in-turn-*`, `tarmac-slide-direction-*`, bands the reading ±0.03):
  brush 8 of 10, ngp 1 of 10. The brush's two misses: the largest braking force at 5° and 8° over the pure peak, 0.89
  / 0.83 against 0.86 / 0.77 (it brakes 4-7 % too well in a turn; the measured tyre heats and wears in those sweeps).
- An analytical combined-slip model with a flexible carcass, and its test data (2026-11-10; library
  `xu-guo-zhang-karimi-2014`, Math. Probl. Eng. 2014, 397538; Fig 14 a P245/65R17 at 4 kN, 2°, 4°, 8°, fig ±0.05):
  - Measured at 2°: Fy/Fy(0) about 0.98 / 0.85 at 0.5 / 0.75 of the peak Fx, as Xia's tyre and the brush, not NGP;
    Cα/Fz about 15.8 per rad; the curves end near Fy = 0 at lock.
  - The force's direction passes from the adhesion direction (stiffer along than across, so turned toward Fx) to the
    slip direction as the patch slides (Fig 11). Lab at 4.8 kN, (Fy/Fx) over (tan α / κ) at 2°, κ 0.02 / 0.05 / 0.10:
    brush 0.75 / 0.89 / 1.00, ngp 1.28 / 1.22 / 1.00. The brush has it; NGP turns the other way.
  - Not in the brush: the carcass's twist raises the cornering stiffness under braking and lowers it under drive
    (model Fig 5: +11 % at κ -0.05, -8 % at +0.05), so a little brake gives more side force than none at 2°; the data
    tilt by about ±10 % at 2° and not at 4° or 8° (Xia's the same at 7.2 kN, flat at 2.4 and 4.8 kN). The brush
    keeps more under braking than drive (0.885 against 0.839 at κ ∓0.05) and never over the free-rolling force.
    Second order, left.
- Cornering stiffness 1.3 times NGP's 0-2° slope (2026-11-08, `brush::CORNERING_OVER_NGP`, user's Nordschleife drive
  "very believable"; Cx, μ_x and loose-v1 as before): semi-slick R888R at NGP's grip Cα/Fz 21.9-22.3 per rad, 6-7° peak
  ([Racing and rally targets](#racing-and-rally-targets)). Probe at 4 kN: Cα/Fz 22.9 (was 17.2), peak slip angle 7.0°
  (was 9.75°; 6.7 kN 8°, was 11°), peak μ_y 1.269 (was 1.266), μ_y(30°)/peak 0.886 (was 0.893), peak κ 0.11
  unchanged. Scorecard 87/112 (was 85; MISS → PASS: peak slip angle, combined Fy(κ 0.25)/Fy(0) at 4° 0.293
  (was 0.350), Mz peak at 0.393 of the Fy peak's angle (was 0.410); PASS → MISS: Kx/Ky 0.925 (was 1.231),
  band 1.15-1.62; at 2/6 kN by the load exponents 0.67/1.12 (was 0.88/1.50)); combined at 2° 0.227 (was 0.296),
  camber stiffness 1.11 per rad (was 0.84; 0.55-1.27); every gravel and snow value bit-equal, ice within 0.5 %
  (`brush::low_mu_slip_stiffness`: below grip 0.55 the Cy of NGP's slope). Limit recovery (r40, uniform 37, kept g
  / widest m / back s, lift / full throttle): Fiesta 0.84/2.3/2.2 (was 0.85/2.0/2.1), full throttle spun (150°,
  was 42°); Golf 0.75/4.0/3.2 (was 0.74/3.9/3.1), 45° (was 38°); Escort spins in all four runs, the lift run
  11.6 m wide (was 4.3). r40 ramp mean |ffb| per 0.1 g within 6 % at 0.3-0.7 g, +8-10 % on the Fiesta at 0.7-1.0 g
  and the Golf's +15 % at 0.9-1.0 g, peak 268 / 103 / 68 (was 256 / 103 / 69; Fiesta / Golf / Escort); map 41 600 s
  mean |ffb| at 0.3-0.7 g -1 to -6 %, pace Fiesta 303.85 s (was 304.03), Golf 314.09 (unchanged).
- Longitudinal stiffness 1.3 times NGP's fit as well (2026-11-08, user-approved; `fit::calibration` returns the stiff
  calibration whole: Cx = `KX_OVER_KY` Cy, μ_x solved for NGP's longitudinal peak; below grip 0.55 Cx of NGP's slope,
  `brush::low_mu_slip_stiffness`). Scorecard 89/112 (was 87; MISS → PASS: Kx/Ky 1.168 (was 0.925),
  band 1.15-1.62; Kx/Fz 26.8 (was 21.2) against road c0x 23.8-42.8); peak κ 0.090 at 4 kN (was 0.110;
  R888R 0.08-0.10); Kx load exponent 1.10 (was 1.13); TRL locked/peak, wet and worn stops within 0.006;
  tarmac car locked/ABS 50 km/h 0.841 (was 0.832), ABS 0.15 1.293 g (was 1.314), threshold κ 0.1 from 100 km/h
  1.171 g (was 1.057), locked 0.970 g (was 0.974); combined Mz at 4° braking (context) -0.120 (was 0.098);
  gravel, snow and ice brake rows bit-equal, ice c0x 2.514 (was 2.523); ice coast crr 0.048 (was 0.002; the run-up
  on tarmac launches differently and the autopilot leaves the iced road; both MISS). Traction, RSF 568 flat out,
  0-100 km/h: Fiesta 4.082 s (was 4.117), Golf 6.778 s (was 6.794). Limit recovery (r40, uniform 37): Fiesta full
  throttle 45° and not back (was a 150° spin), lift 2.3 m wide (unchanged); Golf lift 4.5 m wide (was 4.0),
  full throttle 43° (was 45°); Escort spins in both as before. Map 41 600 s: pace Fiesta 303.67 s (was 303.85),
  Golf 313.78 (was 314.09); mean |ffb| at 0.3-0.7 g +0.5 to -2.8 % (Fiesta), -0.3 to -3.1 % (Golf).
- Rolling sideways against locked, data (2026-11-08): TRR 621 (1976, Calspan flat belt, J78-15 bias-belted, 6.2 kN,
  48 km/h only, dry SN85; fig ±0.02): μ_y 0.84 at 16°, ~0.87 near 30°, ~0.80 at 60-72°, ~0.82 at 90° (= the locked
  wheel's sideways value), braking peak 0.84 / locked 0.74 quoted; so 30°/peak ~1.0 against the brush's 0.80 at 54
  km/h and NGP's 0.865, and at one speed the rolling slide sits at the locked value of a similar sliding speed.
  Persson, J. Phys. Condens. Matter 23 (2011) 015003 (model, flash temperature, tread at 60-80 °C; fig ±0.02): μ_x at
  25 % slip 1.38/1.37/1.40 at 10/30/40 m/s, μ_y 25°/peak 0.97 at 27 m/s; the cold-rubber benefit of a fresh element
  lasts 0.2-2 mm of sliding, so contact time alone does not hold grip at 10-30°. Gent & Walter, NHTSA DOT HS 810 561
  (2006) ch. 8: dry peak lateral "not likely to change much with speed", the back slope expected steeper with speed
  (a sketch from locked-wheel data, not measured). Every published MF set read (27 `.tir` on GitHub, MFeval, Chrono)
  has LMUV 0 or none. Cabrera et al., Sensors 18:896 (2018), locked 205/65R15 on dry asphalt: μ = 0.640 + 0.262
  exp(-0.081 v km/h), flat above ~40 km/h (table), unlike TRL's. Pottinger, Marshall & Arnold, SAE 760029 (1976,
  GR70-15, 10-121 km/h, read in full): F_y at 1° +8-9 % per tenfold speed, the speed effect insignificant beyond about
  4° (quoted; no larger angles tested). Pacejka, Tyre and Vehicle Dynamics 2nd ed. (2006) eq. 3.116: μ = μ0/(1 + a_μ
  V_s), a_μ 0.03 s/m in his tread-simulation example (a model, "typical especially for wet roads"; the peak "will
  decrease when the speed of travel is increased", not shown); MF's λ_μV (eq. 4.E7, μ/(1 + λ_μV V_s/V_0)) is 0 unless
  a slippery surface is meant. The brush's curve is close to that example: at V_s 7.5 m/s 0.81 of μ_S against 0.82.
  Selig et al., Tire Sci. Technol. 42 (2014) 216 (summer tyre on P80 corundum, flat-bed, 16.6 m/s only): cornering
  heats the tread surface from 36 to 75 °C at 10° and 9 kN and the measured μ_y falls below the braking curve's level,
  faster with slip than flash heating alone predicts (fig). Nakanishi et al., VSD 62 (2024) 2401 (slick, drum, 100
  km/h, 0-4° only): the fitted friction scale falls with slip angle, attributed to heat. No open two-speed measurement
  of a rolling tyre at 10-30° on dry tarmac was found.
- Wet slide past the peak against the data (2026-11-09, T56, `research/wet-slide`, no model change; lab at 4 kN, 50
  km/h, NGP's wet tarmac level with WaterFactor 1 against dry). μ_y(30°)/peak dry / wet: brush 0.852 / 0.808 (wet over
  dry 0.95), NGP 0.865 / 0.860 (0.99); Bergman 1.0 / 0.87 (0.87); μ_y(20°)/peak wet brush 0.916, NGP 0.943, Beauregard
  0.955 (32-64 km/h), Bird & Miller 0.95, Salaani ~1.0. μ_x(κ 0.5)/peak dry / wet: brush 0.837 / 0.760, NGP 0.919 /
  0.919, Bergman 0.95 / 0.84; locked/peak brush 0.724 / 0.566, NGP 0.78 / 0.78, Bergman 0.88 / 0.63 (TRL interpolated
  to 50 km/h 0.57-0.61). Speed, wet μ_y(20°)/peak 100 over 32 km/h: brush 0.88, NGP 1.00, Beauregard 0.95 (a bias
  tyre peaking at 13-16°). What the data say: (1) a rolling tyre at a large angle in the wet slides at about the
  friction a braked or locked tyre has at the same sliding speed (Bergman 30° against κ 0.5, wet 0.87 / 0.84, dry
  1.0 / 0.95; SCRIM's smooth tyre at 20° on the locked line, PPR980), so the brush's mechanism (the locked-wheel law
  read at the patch's sliding speed) is supported; (2) how much of the wet loss reaches the side axis is not settled:
  Bergman's wet lateral loses 0.13 of its peak more than dry against braking's 0.11 at the same sliding speed (all of
  it, the brush passes 0.57 of it: 0.044 against 0.077), on a honed surface whose wet braking loss (locked/peak 0.63) is
  TRL's; Salaani's shaved tyre on sandpaper loses nothing wet past the peak while dry falls 15-25 % (heat or abrasion
  on a sharp microtexture: the wet side force at 20° 1.10 of the dry); Beauregard falls 0.045 wet, 0 dry, by 20°. NGP
  (no wet fall) fits neither Bergman nor Beauregard, and Salaani's wet stays above its own dry. So the 0.65 share is untested in the wet: one source
  says the whole loss, one none; the brush's wet over dry 0.95 sits between Bergman's 0.87 and Beauregard's ~0.955 at
  20°. If Bergman were taken alone, the wet Coulomb loss would reach the side axis whole (the dry fall keeping
  `LATERAL_FALL`): wet 30°/peak at 50 km/h ~0.77-0.78 (estimate from the κ 0.5 loss, not run), wet over dry ~0.91. Not
  done: one bias tyre on one low-friction surface at one speed. No measurement of a radial or rally tyre past 20° on
  wet road asphalt, or of the wet slide's speed dependence past 20°, was found (Rompe, FAT Hefte 234/249, 1973/1975,
  sweeps to 90° on wet roads per Bergman, not available). Rows: `tarmac-bergman-wet-locked-over-peak-48` (scored:
  brush 0.566 MISS -0.034, NGP 0.78 MISS), six context rows.

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

### A player's laps: Nordschleife, Alpine A110 Rally RGT (RWD)

Two laps by the user (2026-11-08, main 5af50f40, BTB `track-1654149_M`, 20.76 km dry, sequential, no aids): brush
559.80 s with 10 s of penalties, `experimental` (NGP's tyre) 522.41 s. Their words: "NGP definitely stepped out more,
had to do more corrections". States: `rbr-standalone --live ... --replay F --replay-pace free --replay-trace OUT`
(each frame's car and tyres; both replays all frames equal, finish times as in `.rrp.splits`); slip angle
`atan(-vlat/vlong)`, κ `-vslip/vlong` from the scratch's contact velocities; lab `drive_force` (|Fy|/Fz at a given drive
force) on the car's probed rear wheel. Step-out: rear axle α > 6° and > front + 1°, at > 36 km/h.

- Counts (brush / NGP): 58 / 69 events (2.8 / 3.3 per km); rear α > 8° for 29 / 58 s; counter-steer > 3° of road
  wheel for 16 / 39 s; steering reversals of ≥ 4° of wheel 104 / 147. Mean steady-throttle event 0.57 / 1.13 s, rear
  peak 9.5 / 12.6°, counter-steer 3.4 / 6.3°; power 0.77 / 0.89 s, 13.3 / 12.4°, 4.1 / 7.4°; lift-off 1 / 4 events.
  At 13 corners driven alike (speed within 6 %, throttle within 0.15) NGP's rear went 2-11° further in 11 of them
  (e.g. 8.1 km, 96/92 km/h, throttle 0.6: rear 6.7 / 17.0°, counter-steer 0.5 / 13.6°).
- Onset (part throttle, 80-180 km/h): the cornering stiffness's load exponent (brush 0.64, NGP 0.36, data 0.47-0.69).
  Rear over front axle force at 2° at the laps' loads 1.29-1.47 / 1.16-1.39 against a load ratio of 1.5-1.7, so the
  rear runs 1.12 / 1.28 of the front's slip angle (laps' medians 1.55/1.38°, 1.80/1.41°). In game the loaded outer
  rear (4.5-6 kN, driving) gives |Fy|/Fz 0.48/0.82/1.03 at 2/4/6° against 0.45/0.75/0.94, as the lab (5.5 kN).
- Under power: Kx (brush 27 with Cx 1.3 times NGP's slope's since 2026-11-08, was 21; NGP 14 per Fz; data 18.4-32.9)
  halves the rear's slip ratio at the same drive force
  (lap medians 0.022 / 0.042 at Fx/Fz 0.3-0.6); at a given drive force the lateral force below the peak is within 3 %
  at 4 kN. Past 15° under drive the brush keeps less (Fx/Fz 0.45, 4 kN: 1.130/1.082/0.987 against 1.157/1.134/1.060 at
  15/20/30°) at 1.6-3× NGP's κ.
- The brush's one spin (15.7 km): 11 km/h faster than the NGP lap (165-177 km/h), full throttle at rear α 9.6° and κ
  0.15, then the S-bend's change of direction took the rear from 2° to 46° in 1 s through a lift and 8° of
  counter-steer; ~17 s lost. Not a like-for-like comparison; it fits the known fall past the peak ("Past the limit on
  tarmac", Open).
- The spin from the drive's own state (2026-11-08, T26; `--replay-tyre STEP:MODEL` on the brush replay, the user's
  inputs run on open loop from STEP with a fresh tyre model, same engine): from nine entry points 418.0-421.6 s
  (2.2-0.1 s before the change of direction) NGP's tyre spins from 6, the brush from 8 (the fresh brush reproduces the
  drive's spin; from 420.5 s it stays at 41°). The outcome flips between neighbouring entry points under either tyre:
  open-loop inputs made for one slide do not separate the tyres there. Where they part (switch at 421.1 s, NGP
  recovers): in 421.5-421.85 s, throttle 0.4-0.8, rear α 7-10°, κ 0.02-0.14, the brush's rear axle holds μ_y
  1.07-1.09 against NGP's 0.98-1.03 (light inner rear at 1-2 kN: |Fy|/Fz 1.09-1.13 against 0.95, the brush's
  μ ∝ Fz^-0.13) and its front axle drops to 0.26 against 0.64, so the yaw rate swings from +9 to -69 °/s against -52;
  the rear then sits at its peak (μ 1.04-1.12, α 10-46°) and cannot stop the yaw. A pendulum on the catch, not the
  rear letting go under drive. Variants switched in the same way all spin 9/9 (lateral fall 0.55, μ_y(30°)/peak about
  0.91; fall 1.0, 0.833; the sliding force turned toward the lateral axis, direction (s_x, 1.3 s_y)). The r40 limit
  test (`limit_recovery.py` on map 85 as tarmac 37, kept g / widest m / back s / peak β, lift; full throttle) brush now:
  Fiesta 0.85/2.0/2.05/4.0°; 0.82/14.3/never/41.6° (ngp spins, 93°); Golf 0.74/3.9/3.1/3.1°; 0.81/17.1/never/37.5°
  (ngp 16.5°); Escort: ngp never past the limit, the brush spins. Fall 0.55: Fiesta full throttle back in 4.9 s at 37°, Golf 33.7°, Escort lift
  81° (100°), within a few % elsewhere; direction 1.3: Fiesta full throttle spins (77°). Not committed: no variant
  turns the snap and the data do not single one out. The autopilot's own speed plan at 10-14 m/s² (`--plan`, first
  2.3 km, Alpine, 170-180 km/h) slides no car past 10° of sideslip in any physics (peak brush 5.9°, ngp 7.4°), so it
  is no closed-loop test here. Back to back on the drive: `rbr-standalone ARGS --replay F --replay-pace free
  --replay-tyre 304712:ngp` against `:brush` (ARGS the program arguments the replay stores after `--live PROGRAM`; step 304712 is 421.1 s).

## Tyre temperatures

`rbr_tyre::thermal` (experimental-brush on hard surfaces only, 2026-11-10, `tyre/thermal`). NGP's heat step sends 81 %
of the slip power past the surface (above), so a slide barely moves the grip. Per wheel two nodes, explicit at 1/720 s:

- Surface layer: NGP's 8 sectors' capacity as one (307 J/K, ~0.5 mm). In: `TYRE_SHARE` 0.5 of the patch's friction
  power |Fx (vx + ω R)| + |Fy vy| (Kobayashi assumed 0.5; West & Limebeer fitted 0.49-0.68). Out: conduction into
  the bulk across the layer's thickness, k 0.25 W/(m·K) (Gent & Walter 0.2-0.3, Fina 0.33), 198 W/K; convection
  Nu = 0.0239 Re^0.805, L = 1/(1/D + 1/W) (West & Limebeer), 44 / 75 / 104 W/(m²·K) at 10 / 20 / 30 m/s over the
  tread's surface (2πRW); conduction to the road at air + 9.86 K, NGP's term kept: contact factor × (1500 + 4500 hot)
  × (1 + 3 W) × contact area (hot 0 at 40 °C to 1 at 120 °C). West & Limebeer's 12 kW/(m²·K) over the adhering share
  alone left a tyre at the limit 375 K after 10 min (the brush's patch slides whole at its peak): not taken. Air
  properties standard (ours).
- Bulk (tread): NGP's 5836 J/K. In: the conduction from the surface and NGP's rolling resistance power (0.5 l_p rr Fz
  × sliding weight × |ω|). Out: NGP's carcass conductance (613 W/K) to the carcass.
- Grip: NGP's `RubMyFct` 0.6 surface + 0.4 bulk, scaled against NGP's own blend (the static friction already holds
  it); the brush's stiffness (-0.45 %/K) reads the bulk. Steady state at the skidpad point (12 kW, 21.7 m/s, 900 s, a
  stand-in carcass): grip within 1 % of NGP's 0.997 (unit test).
- Seam: `tyre/step.rs:206` passes NGP's rubber grip and thermal state to `model::input` (`ThermalInput`), the model
  answers `ForceOutput::temperatures`, and after NGP's heat step `model::apply_temperatures` (`step.rs:229`,
  `model.rs:189`) writes the surface into all 8 sectors and the bulk into the tread, and puts the carcass back to its
  value before the heat step. NGP's wear and power words stay, `relax` still conducts tread → carcass (the same flow
  the bulk loses), so pressure, wear and the HUD read the new values. State in `car::Tyre`, reset off contact and at
  restart, adopted from NGP's slots when the model takes over. ngp, experimental and 1.02 never set `temperatures`.
- Soil: NGP's heat step as it is (no published share or soil conduction; gravel probe surfaces reached 387 K with
  the tarmac terms).

Lab (`ngp_transient --model brush`, section (f), map 85 circle image, front right; NGP / brush, surface mean K, grip):
6° at 20 m/s for 20 s 335.3 / 340.9, tread 339.4 / 339.1, 5 s later 330.6 / 324.8; 30° slide 2 s from 331 K 346.4 /
374.8 (grip 1.000 / 0.980), back to 332.3 / 327.3 at 5 s; locked at 100 km/h 2 s 346.5 (one sector 390) / 420.5 (grip
0.941 / 0.910); 60 s at the limit then straight: tread 345.3 / 342.0, 30 s later 337.1 / 335.8. Keys
(`--thermal-keys`): surface rise in 1 s of 30° 12.2 / 35.2 K, decay τ after it 0.17 / 0.53 s, tread rise 6° 20 s
2.19 / 2.30 K, tread after 600 s at the limit 356.1 / 346.4 K (Pirelli RA7 50-90 °C), wet over dry cooling at 80 km/h
3.75 / 3.68 K (Ejsmont 6-7: MISS both).

Past the limit (`limit_recovery.py` on map 85 as tarmac 37, brush before / after; widest m, back s, peak β °): Fiesta
lift 2.90 / 3.11, 2.59 / 2.75; full throttle 15.96 / 17.19, 5.18 / 4.88, β 47 / 65; Golf lift 6.14 / 6.48, 4.00 / 4.23;
full throttle 22.94 / 26.74, never back, β 51 / 160 (spun); Escort lift 12.42 / 13.44 (spun both), full throttle 8.29 /
11.68, 5.42 / never. ngp unchanged. The hot surface costs grip in the slide, so recovering is harder.

Stage (map 41, Fiesta Rally2, autopilot): finish 303.64 s (was 303.61; ngp 309.01). Surface mean front 314.6-393.9 K
(mean 320.9), rear 314.3-359.1; tread 319.4-334.4 (mean 326). With NGP's heat step under the brush (was): surface
316-331, tread 318-334; ngp surface 316.5-331.8, tread 319-334.

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
  6° (model inputs, "in the ball park"); a road racing tyre loses 10-20 % past the peak; its "rally tyre" (Fig 5b, an
  input curve, peak 6°) keeps 0.97/0.94/0.90/0.87 at 8.6/11.4/17.1/20°, its "road course tyre" (Fig 5a) 0.73 at 8.4°;
  its only measured data are Croft-White's (Fig 2). Pirelli P Zero RA datasheet:
  tread windows 20-60 °C (RA9) to 70-110 °C (RA5), hot 2.0-2.5 bar. Michelin 2023-24 pressures: asphalt rally hot
  2.0-2.3 bar, cold at least 1.6. FIA tread rule (2006 copy): at least 17 % grooves of 5.5 mm. Track cars on
  semi-slicks (Darkside Developments, VBOX, single-session peaks): 1.32-1.53 g lateral, 1.17-1.47 g braking.
- Drifting: Hindiyeh, "Dynamics and control of drifting in automobiles", PhD Stanford 2013 (Gerdes lab): P1 (RWD,
  1724 kg) on gravel over asphalt at 8 m/s, lumped-axle Fiala brush fitted to ramp steers (Table 2.2, Figs 2.13-2.14):
  µ 0.55 with no discernible peak over sliding, Cα 120/175 kN/rad front/rear (Cα/Fz ~15/19 per rad at static loads,
  inferred). Combined slip is modelled (friction circle), not measured; the drift equilibrium (β -20.4°, r 0.6 rad/s,
  FxR 2293 N; Table 4.3) fits a rear µ 0.53, a derate of 0.96 where the friction circle at that drive gives 0.89
  (inferred; lumped, includes load transfer). Drift equilibria are open-loop unstable; the rear drive force holds the
  sideslip (Ch. 3-5). Not a tarmac source.
- Not used: Racecar Engineering, "Tyre dynamics" (Campbell-Brennan, 2020), an introduction: its "around six degrees"
  peak is an illustrative curve from Balkwill, Performance Vehicle Dynamics (2017; tyre unstated); its longitudinal
  peak at slip ratio "0.3-0.4" contradicts every set above (0.08-0.18).
- Semi-slick on tarmac: MegaRide with Calspan, "Tire Performance Evaluation: Comparing Indoor Test Bench and Outdoor
  Model-Based Vehicle Testing Methodologies" (megaride.eu, 2024), Toyo Proxes R888R 195/50 R16 on a Fiat 124 Spider,
  MF 5.2 fits from track data (Circuito del Sele, 75 °C, 2.2 bar) and from Calspan's flat track (sandpaper), Fig 12
  (fig ±5 %): μ_y 1.36 / 1.50 at 1 kN, 1.20 / 1.29 at 4 kN, 1.13 / 1.19 at 5.5 kN (outdoor / indoor; μ ∝ Fz^-0.11 /
  -0.14); K_y at 4 kN ~1560 / 1530 N/deg, Cα/Fz 22.3 / 21.9 per rad (NGP 17.6, 1.27 / 1.24 times), K_y ∝ Fz^~0.8 /
  ~0.6 over 2-5 kN; lateral peak near 6-7° at 4 kN (fig ±1°), at 20° 0.86-0.90 of it at 4 kN, 0.80 indoor at
  5.5 kN; longitudinal peak near κ 0.08-0.10 (Fig 8). A track tyre at NGP's grip level, measured on asphalt: the
  closest data to a tarmac rally tyre found.
- Not used: Edy's "Pacejka '94 parameters explained" (edy.es), a game-tuning guide: its "typical ranges" are not
  measurements.
- Rally tarmac in the wet: no measured grip found. Michelin Pilot Sport FW2 (WRC rain, 2016): tread 175 mm against
  202 (H5/S5), grooves 7 against 6 mm, sea > 27.5 %, developed on 3 mm water at 90 km/h (Tyrepress, quoted); Pirelli
  Cinturato RWB (WRC wet), P Zero RA soft "mixed dry and damp"; Hankook Z210 wet, Z215 "dry or damp" (makers' text).

Rally on tarmac: no Rally1/Rally2/R5 g data, logger trace or g-g diagram is open (searched telemetry overlays,
WRC+ graphics, team and engineer publications); the only rally numbers are the ChassisSim model inputs. Combined
slip on tarmac rally is described only qualitatively (trail braking: brake hard and ease off on turn-in, Team O'Neil).

Comparison: NGP and brush-v1 on the Fiesta Rally2 at 4 kN (`racing` example; 15 m/s unless stated):

| Property | Racing / rally | Road targets | NGP | brush-v1 | Racing moves the target |
| --- | --- | --- | --- | --- | --- |
| Peak μ (car) | semi-slick track cars 1.32-1.53 g lateral, 1.17-1.47 g braking; rally car ~1 g (estimate) | | 1.265; brake 1.16 g mean from 15 m/s | 1.265 (calibrated); 1.28 g | no: no rally level, NGP sets it |
| Peak Fy ∝ Fz^n | 0.70 C, 0.73 A (fig ±0.05), 0.80 E, 0.82 D (4-8 kN), 0.84 B | 0.92 (2-6 kN; 0.88 over 2-10 kN) | 0.878 (2-6 kN), 0.787 (4-8 kN) | 0.846, 0.845 (4-8 kN) | down, to ~0.80 |
| Peak slip angle | A flat from 4-5° to 17° (fig ±1°); B 9-12° (fig ±1°); D 5.6-6.7°; E 8-9°; rally 6° (estimate) | 6.5-8.5° | 9.5° (95 % at 6.75°) | 8.75° (95 % at 6.5°); 7.0° at 1.3 times NGP's slope (2026-11-08) | down only by D and the estimate, both weak |
| Peak κ | A 0.08-0.12 (fig); B 0.10-0.12 drive, 0.13-0.18 brake (fig); E 0.10-0.11 | 0.094-0.139 | 0.16 | 0.12 drive / 0.10 brake | no |
| Cα ∝ Fz^m | A 0.87 (MF); B ~0.56 (fig ±30 %) | 0.56-0.91 | 0.36 | 0.64 | no (spread) |
| Cα/Fz, Kx/Fz per rad | A 31-34, 41-47 (MF); B 35-39 at 664 N (fig ±30 %) | Kx/Fz 18.4-32.9 | 17.6, 14.1 | 17.6, 22.0 | no: at 0.6-1.1 kN; scaled to 4 kN by Fz^-0.4 (inferred) ~19 |
| Lateral over peak | A 0.95 at 17° (fig ±0.03); B 0.96 at 12° (fig); ChassisSim rally input curve 0.90 at 17° | 0.73-0.83 full slide | 0.992 at 12°, 0.968 at 17° | 0.974 / 0.937 (11.2 m/s: 0.979 / 0.945); 0.876 at 30° | up: brush-v1 is inside the racing range to 17° |
| Longitudinal over peak | A 0.90-0.95 at κ 0.5-1 (fig ±0.05); B 0.93 at κ 0.2 (fig) | 0.71-0.85 at κ 0.5-1 (asymptote 0.65) | 0.920 at κ 0.5, 0.781 at 1 | 0.861 / 0.786 (11.2 m/s: 0.873 / 0.798) | up: brush-v1 0.05-0.15 below racing |
| Combined: Fy(25 % slip)/Fy(0) | none open for racing; G (road, measured points) 0.14-0.25 at 2°, 0.23-0.27 at 4° (fig ±0.05; the model lines 0.25-0.31); Fy/\|F\| at 4° ~0.26 | | 0.40 at 2°, 0.46 at 4°; Fy/\|F\| 0.35 | 0.28, 0.33; Fy/\|F\| 0.269 (before the split 0.20, 0.24; 0.196) | none from racing |
| Relaxation length | Hoosier R25B 0.49-0.50 m (Cal Poly senior project, search excerpt only, its 0.55 s does not fit 40 km/h): not used | car tyre 0.3-0.9 m (rig); motorcycle 0.10-0.4 m | 0.17 m | 0.20 m | unknown |
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
4. Load sensitivity, optional: N_MU_Y 0.87 to ~0.82 (racing 0.70-0.84; NGP itself 0.79 above 4 kN); costs 2 % peak μ at
   6 kN. Low priority. Not taken: tried in the lab, 0.82 holds the 4 kN peak (1.265; the fit is at the design load) and
   moves the 4-8 kN exponent from 0.845 to 0.800 (NGP 0.787), 6 kN μ 1.188 → 1.166 (NGP 1.181). The evidence for it is
   weak: the FS exponents (0.70-0.84) are at a tenth of a rally car's loads, where the exponent depends on the load
   range; D (0.82) is the only figure at rally loads, second-hand, tyre unnamed; E is a model. The one measured road
   sweep (Goodyear, 0.92 over 2-6 kN, 0.88 over 2-10 kN) and NGP over the 2-6 kN a rally car runs (0.878) sit at or
   above 0.87, and 0.82 would take the light (inside, 2 kN) wheel's peak μ further above NGP's (1.407 → ~1.46, NGP
   1.356). Revisit with a rally or racing tyre's load sweep at 2-8 kN.
5. Leave peak κ, the stiffness exponents, σ and the temperature grip: racing data do not move them, or are unverified.

Missing: any rally tyre curve; measured combined slip for any racing tyre (TTC combined runs are licence-gated);
rally g data on tarmac; a verified racing relaxation length; absolute μ against temperature for a slick.

## Loose surfaces: NGP against published data

Runs (2026-11-07, `ngp` physics, Fiesta Rally2): gravel on RSF 345 Azov II (`--circle 1051,-166,40 --circle-ramp
0.4`, step 6000, Gravel_Fine_Shallow_Dry 17, kind 4); snow on retail map 10 (`track-10_O`, autopilot, step 6000,
SnowOnGravel_Shallow 101, kind 6), SnowOnIce_Shallow (117, RSF 162 Karlstad, step 21000) within 0.05 of it; tarmac map
85 as above (step 8000, wheel 1). Ice (material 116): the pad of 116 on the tarmac tyre (`--stage pad-116 --surface
tarmac`, [Ice](#ice-material-116)), the circle's wheel 1 at step 8000, the tarmac tyre at 0.0949/0.0902. "Sweep": pure
`patch_forces` at 15 m/s; "step": `ngp_transient --ramps-only`, NGP's whole per-wheel step on the run's `--tyre-image`,
15 m/s unless stated; 4 kN unless stated. Published values, sources and precision labels from the 2026-11-07
loose-surface research (its files are not in git).

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
| Peak slip angle | gravel ~12° (CW, quoted), 12.1° (Velenis, MF), dirt 16° (ChassisSim, model input); packed snow 9-17°, loose 13-18° (TR0713, table); ice 4.5-5.1° (TR0713, table) | 13.5° (95 % at 9°); step 14° | 11° (95 % at 7.75°); step 11.25° | 0.75° (95 % at 0.5°) | 9.5° (6.75°) |
| μ_y at 30° over peak | gravel 0.63 (CW's text; his figures digitised 0.96-1.06, fig ±0.05, inferred: [Loose surfaces: loose-v1](#loose-surfaces-loose-v1)), 0.85 (Velenis, MF); snow near its max from ~15° (TR0713, quoted; one packed curve ~0.92 of its peak at 15°, Fig. 13); ice 0.59 at 20° (TR0713, fig ±0.005) | 0.956; step 0.985, at 25 m/s rising to 30° (0.897) | 0.955; step 0.997, at 25 m/s rising (0.926) | 0.966 at 20°, 0.950 | 0.865 |
| Peak κ | gravel none open ("0.4-0.7", snippet); snow flat from ~0.26 to 0.30 (Surkutwar, driving traction, fig ±0.01); ice 0.05 (Ružinskas, quoted) | 0.24 (2 kN 0.16) | 0.22 (0.12-0.30 at 2-6 kN) | 0.02 | 0.16 |
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
  flatter or rising, more with speed: like the snow data and the gravel data (CW's figures digitised, Coutermarsh), and ice does not
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
  turning) and the best threshold stops 0-9 % longer than declutched.

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
- Tooling: `--stage pad-116 --surface tarmac` (a pad of material 116 on the tarmac tyre and set-up) for the probe
  circle, the brake (45000 steps: the run-up on ice takes 27 s to 50 km/h) and the coast. Before the pad every
  material took 116's values (`--material-as 116`, removed): map 85 for the probe, RSF 504 from the brake onset
  (rising 6.6 %, decel taken on the flat: ngp 0.096 g at K 0.1, raw 0.162), RSF 568 for the coast. The pad's best
  threshold is 0.095 g.
- The brush on ice: below NGP's grip 0.55 its slip stiffnesses, on the Cy and Cx of NGP's own slope, scale with
  grip/0.55 (`brush::low_mu_stiffness_factor`, `low_mu_slip_stiffness`; the 1.3 over NGP's slope fades out from ~0.75;
  no tarmac material reaches it: the lowest hard surface, 0.68 class 2, is 0.57 on a cold tarmac tyre; a snow tyre on
  wet low-grip tarmac can reach 0.43). Evidence: Svendenius Table 7.1 brush fits at 4 kN, winter tyre C0x 25-27.6 on
  asphalt (μ 1.0-1.2; wet stiffer than dry), 16 on wet basalt (μ 0.27), 6.25 on ice (μ 0.078): knee 0.55 with an
  exponent 0.76 through all three; summer tyre 0.09 of asphalt's on ice (exponent 1.2); 1 is ours, between (was the
  tarmac tyre's stiffness, sliding whole at 0.75-1°); the peak level stays NGP's (0.0948
  against 0.0938 at 4 kN).
- Trade-off: with NGP's level (0.094, the data 0.15-0.18) the peak slip angle and the initial slope cannot both
  match (the brush slides whole at tan α = 3 μ/(C/Fz)): exponent 0.76 gives 2.75° and a 0-2° slope 3.2 /rad, 1.0 gives
  4.25-4.5° and 2.4-2.5 /rad, 1.2 gives 5.75° and 1.9 /rad (lab, the probed wheel, 4 kN, on the friction curve before the TRL refit; 1.0 gives 4.5° and 2.43 /rad on the
  current one). The peak angle was kept; the
  model's own Cx/Fz at zero slip is about 4.3 (Svendenius 3.6-6.7), the scorecard's κ 0.02-0.04 fit (2.5) lies past the
  linear part on ice.
- By tyre (2026-11-10, a user decision): ice is class 5 with grass, and NGP gives 116 the same 0.10 on every tyre
  (class factor 0.95 tarmac, 1.0 gravel and snow: probe `tyre_classes`), studs or not. The unstudded tyres keep it
  (road tyres at 0 °C 0.09-0.12). The brush gives the studded tyre (`Wheel::is_studded`, kind 6: the only one better
  on snow than on tarmac) `brush::STUDDED_ICE` 0.85 on the hard ice share, its own level on snow (guess: rally studs
  "at or above a gravel tyre on gravel", no μ open; road studs 0.21-0.30 are the floor). Iced pad, Fiesta Rally2 ramp
  steer at 40 km/h: tarmac tyre 0.082 g ngp / 0.095 brush, snow tyre 0.091 / 0.767. Not covered: 116 blended into
  snow soil (563 Toivonvaara damp-new, the soil path, where class 5 is also grass). Snow needs no such split: NGP's
  class-3 factors (tarmac 0.525-0.575, gravel 0.60-0.65, snow tyre 1.0) match Ichihara's rib / snow / studded
  traction peaks (0.28-0.32 / 0.34-0.41 / 0.57-0.60: 0.5 / 0.65 / 1).

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
  floored at 0.5 m/s, so the berm fades out at standstill. No berm when driving. On snow (surface class 3, by its
  share in the blend, `loose::class_share`) the berm is `SNOW_PLOUGH` 1.05.
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
  inferred). α0 follows the surface's WaterFactor (`ForceInput::water_factor`; NGP's wet gravel materials carry 1,
  damp 0.25, dry 0: `materials` example, ids 2-51), linear from 1 to 9.7/6.3 times the dry α0
  (`loose::gravel_alpha0_deg`; ours: the form, and Fig. 15's wet gravel is another soil, a silty sand, standing in for
  wet gravel). The dry α0 follows the loose layer's depth (2026-11-09,
  [below](#the-soils-depth-ngps-materials-and-the-surface-across-the-road)): 6.3° is pea stone's and NGP's deeper
  gravel's (25-30 mm). The slope follows the level
  (μ Fz/tan α0: 6.0 /rad at 6.7 kN, NGP 6.6); the level is approached, never peaked (0.995 at 30°, 0.973 wet). Snow (class 3: SnowOnGravel/SnowOnIce 101-103, 117-119, snowwalls), grass (5), rock (4) and
  rough (0) keep the flat brush: packed snow's own α0 is 3.6° (Fig. 8, rms 0.0042), which at NGP's snow level doubles
  NGP's slope (snow-calpha-per-fz 8-12 would miss), while the brush at NGP's slope passes α90 (9.0 in 7-9°).
- Firm gravel (context, 2026-11-09): Hindiyeh & Gerdes 2014 (JDSMC 136 051015, §2; Stanford's P1, RWD, 1724 kg) fit
  Fiala per axle from steady cornering on a gravel lot, μ 0.55, highly variable: C_α/Fz 15.4 front / 19.2 rear per
  rad, full slide at 6.1 / 4.9°, 90 % at 3.3 / 2.6° (an exponential law's α0 ~1.1-1.4°; axle-lumped, so a tyre's own
  is stiffer). Ours at 4 kN, 15 m/s, dry (`gravel-firm-*`, context, the probe's 9 mm): ngp 9.1 per rad, α90 7.5°;
  brush 11.2, 8.0° (was 6.6, 14.25°).
  Gravel's shape spans about 5x between a firm lot and loose pea stone (α0 6.3°); the brush sits at the loose end,
  NGP between. A firm stage (packed base, thin loose top) may build force faster than the brush does. Candidate,
  done 2026-11-09: α0 by NGP's soil depth (shallow `Gravel_*` firmer). Their drift held β 20° at 8 m/s with 12° of
  countersteer for 30 s within 3-5° (the controller's equilibrium: rear drive 0.46 of the rear's μ Fz, the front
  near saturation). Velenis, Tsiotras & Lu (ECC 2007) is simulation only: its MF (Table I: B 7, C 1.6, D 0.52) is
  the "Velenis MF" model input of `gravel-peak-slip-angle` and `gravel-mu-y30-over-peak`.
- Metz's gravel (context, 2026-11-09; Metz SAE 930765 pp. 69-71, via Allen, Rosenthal & Chrstos SAE 970559 Table 2
  and eq. 17): an exponential fit Fy/Fz = A (1 - exp(-B α)), B falling with load (`gravel-metz-*`): gravel μ 0.52,
  α90 12.1° at the rated load and 7.5° at half of it, C_α/Fz 5.7 / 9.1 per rad. Weak for a rally tyre: the
  off-highway rows are a Terra Tire 12.5-18 (low-pressure flotation tyre) on undriven, angled wheels, scaled from
  Schwanghart & Rott's plots (ISTVS 1984), the load exponent 0.14 borrowed from a 17.5-25 loader tyre, whose
  manufacturer's data are the pavement row (so no gravel-over-pavement ratio on one tyre). The fit's B =
  (C (F_ZT/Fz)^m - D)/A (the printed +D/A gives none of the table's B; -D/A gives all five). With Croft-White's WRC on
  a gravel road (~12°, p75 9-11°) a gravel road sits at α90 ~8-12°: NGP (7.5°) a little stiff, the brush (14.25°) a
  little soft, the firm lot (2.6-3.3°) and loose pea stone (14.5-21°) either side. Allen's reading of Christoffersen
  et al. (SAE 950139, towed at 0.2 m/s, Fig. 7): on ploughing soils the free-rolling side force at 90° exceeds the
  locked wheel's (sand 1.41 against 1.0), coastal grass did not plough: context for the berm (`PLOUGH`). The
  rally-road evidence for α90 is Croft-White alone; Metz's flotation tyre and the firm lot bracket it.
- Fit: NGP's free rolling peaks μ_y, μ_x (driving) at each load of the wheel's axis on the unit surface
  (`fit::loose`), linear between, times NGP's grip; the flat brush's peak is μ Fz, so the level needs no solve. The
  cornering stiffness is re-solved under the flat friction to NGP's slope (0-2°, 4 kN); Cx = 1.4 Cy as the brush's.
- Snow's berm (2026-11-08): studded snow tyres on normal compacted snow, 20 km/h (Ichihara & Mizoguchi, HRB SR 115,
  Fig. 3, fig ±0.02): 0.36-0.42 at 15-20 % slip, peak 0.42-0.445 at 40-60 %, locked 0.40-0.43, so locked over the 15-20 %
  value 1.02-1.08 and over the peak 0.95-0.97 (snow tyres 0.74-0.95 of the 20 % value); 1.05 is the studded middle.
  In game (Fiesta Rally2 / Escort, map 10, declutched, locked over ABS K 0.15): 50 km/h 1.05 / 1.28 (was 1.17 /
  1.42), 80 km/h 1.09 / 1.07 (was 1.27 / 1.13; ngp 0.98-1.06); Rally2 locked 0.95 g (50; was 1.06), 0.92 g (80;
  was 1.08); ABS,
  threshold and the snow circle (0.554 g) unchanged. The Escort's 50 km/h ABS run is its weakest (0.68 g, best single
  target 0.71). Scorecard: only `snow-car-locked-decel-50` moves (0.952, was 1.059, PASS); every gravel, tarmac, ice and
  ngp row bit-equal. Pace, autopilot to the finish (Fiesta Rally2): track-10_O 385.48 s (was 385.36; ngp 393.53), RSF 111
  (gravel) 287.08 s unchanged.
- Ours: the smoothstep and its start at 0.8 (no intermediate slips are published; chosen on the trade-off below: a
  berm from the full slide holds the wheels off lock at the Fiesta's full-pedal brake torque), no berm when driving
  (the data are braked; a spinning tyre's sinkage resistance is unmeasured), PLOUGH independent of the soil's
  thickness (data on 5 cm layers, NGP's 9-30 mm) and of speed (no trend at 40-80 km/h), NGP's soil lateral force kept
  (NGP's lateral ploughing: 3-4 % of the peak at 30° and 15 m/s, ~10 % at 25 m/s, out of reach of Coutermarsh's 1-1.5
  m/s).
- Carcass at the limit (`brush::step_dynamic`, both surfaces): sliding at the limit, the deflections take the steady
  force's direction (was: a locked wheel's deflection integrated the sliding velocity weighted by c_x/c_y, so its
  side force at 20° was 0.197 against the steady 0.37; in game it moved RSF 111 by 0.02 s and the gravel locked
  decel by 0.001 g).

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
- The one DNF is the Golf on RSF 104 under eb, stuck at 4488 m: braking at a 0.8 pedal at full lock (4450 m,
  12 m/s), the fronts creep at 0.4-0.9 m/s (slip 0.93-0.96) and the engine in fourth follows them down to 0 and
  stalls; the autopilot has no restart. ngp and experimental lock both fronts at the same corner and finish
  (282.12 / 277.27 s). The creep is loose-v1's flat friction, not the berm: no fall from the peak to sliding, so a
  brake torque near the peak does not run the wheel into lock (NGP's force falls past its peak; a berm from slip
  0.99 creeps the same and DNFs at the same step). Fixed in the autopilot (2026-11-08): neutral while the braked
  wheels drag the engine under 89 rad/s for 4 steps, plus restarting a stalled engine on the ignition. The Golf now
  finishes RSF 104 under eb in 275.34 s; all 27 ngp sweep runs keep their times, eb moves only on the Golf
  track-10_O (+1.21 s, neutral at a front lock in first) and the Celica RSF 104 (+0.06 s).
- Not adopted (2026-11-08, prototype): a berm built over sliding distance (per-wheel state growing over the patch
  length 2a of sliding past the ramp's start, washing out over 5 cm of rolling). Its steady state s / (s + 3.6(1 - s))
  (0.79 at slip 0.93 against the ramp's 0.72) behaves like the ramp: the Golf on RSF 104 DNFs at the same step (washout
  5 cm, 1 cm, or building only from slip 0.99); Fiesta 345 from 50 km/h, full pedal 0.870-0.889 g against the ramp's
  0.892 g (locked over best 1.13-1.16), no variant locks every wheel; Golf and Celica stage times on RSF 104/111/389
  within 0.05 s. It fixes nothing and adds a state.
- Scorecard (experimental-brush, [Scorecard](#scorecard), 2026-11-08 after gravel's shear law; main 4bb4c042 52/103,
  this 61/103): passes the pea-stone μ_y over max at 2.5/5/10/15° 0.327/0.547/0.798/0.912 (was 0.40/0.68/0.95/1.00),
  the rally ratios at 5/10/20° 0.547/0.798/0.964 (was 0.68/0.95/1.00), α90 14.25° (was 8.5), the slope at 6.7 kN
  6.04 /rad (was 7.37), gravel peak κ 0.58 (was 0.23), circle 0.664 g (was 0.620), locked over ABS K 0.15
  1.53 / 1.34 (was 1.38 / 1.20), locked
  over lateral max 1.458, locked over traction 1.305, gravel over tarmac braking 0.748, the snow rows (unchanged);
  newly misses gravel-peak-slip-angle (11-16°, Croft-White's ~12° saturation, inferred): the curve has no peak, so the
  0-40° argmax is 40°, against the pea-stone curve still rising at 38° (that row and pea-peak-alpha ≥ 15 conflict).
  Still misses locked decel 0.882 / 0.908 g (0.47-0.66), locked side force at 30° (0.33-0.39), μ_y(30°) over peak 0.995
  (rally 0.63-0.85, against the pea stone's ≥ 0.97), snow peak κ, and every absolute gravel and snow level (NGP's).
  Both conflicting rows were split by speed on 2026-11-08 (Gravel at stage speed, below): ngp / eb now pass
  gravel-cw-mu-y30-over-peak 0.956 / 0.973, gravel-cw-alpha90 7.5 (MISS) / 21.0°.
- Gravel's shear law in game (2026-11-08, main 4bb4c042 → this, experimental-brush; ngp unchanged by construction):
  Fiesta Rally2 RSF 345 from 50 km/h declutched, full pedal 0.882 g (was 0.892), wheel slip 0.946 (was 0.943;
  the wheels still do not lock fully, as before); ABS K 0.15 0.576 g (was 0.645; its wheels' mean slip 0.08-0.13:
  below the slower rise's plateau), locked over ABS 1.53 (was 1.38). r40 circle 0.664 g (was 0.620). Standstill after
  the stop, peak speed per half second: full pedal 0.347/0.031/0.008/0 m/s (was 0.345/0.055/0.016/0.006/0.002/0),
  0.35 pedal 0.347/0.031/0.008/0 (was 0.346/0.029/0.008/0); no drift after 1 s either. Soil↔hard RSF 469 / 473
  (Fiesta): 274.59 s (was 274.52); 473 now finishes 274.40 s (main rolled at 22 s, autopilot); wheel-speed reversals (> 0.01 m/s, over 2 m/s) 0.023-0.034 of
  steps on 469 both, 0.022-0.037 on 473 (main 0.05-0.12 in its crash). Stage times, ngp / main eb / this eb (s):

  | Car | RSF 104 | RSF 111 | RSF 389 (snow) | track-11_O (snow) |
  | --- | --- | --- | --- | --- |
  | Fiesta Rally2 | 268.55 / 263.82 / 265.68 | 290.32 / 286.61 / 287.06 | 178.68 / 176.75 / 176.75 | 648.88 / 636.60 / 636.60 |
  | Golf II GTI 16V | 282.12 / DNF / DNF | 302.62 / 297.04 / 298.19 | 187.14 / 183.42 / 183.42 | 681.71 / 652.44 / 652.44 |
  | Celica ST185 | 267.54 / 263.27 / 264.82 | 289.08 / 285.66 / 286.38 | 179.41 / 177.40 / 177.40 | 652.11 / 640.27 / 640.27 |

  On gravel the law costs 0.45-1.85 s (0.2-0.7 %), eb stays 1.0-1.3 % ahead of ngp; snow is byte-for-byte main's. The
  Golf's RSF 104 DNF is the autopilot's (stuck at 4488 m in both, above). These runs used α0 6.28°; 6.3° moves the
  scorecard rows by ≤ 0.002.
- Gravel at stage speed (2026-11-08, `tyre/gravel-speed`): Croft-White's WRC curves (PhD, Cranfield 2006, Figs
  4.29-4.33; rear tyre, static Fz 3 kN, full stage speed, wet loose gravel and dry tarmac), digitised by pixel, the
  scatter folded by sign, percentiles per 2° bin (fig ±0.05, inferred). Gravel, 28-32° over the 10-24° maximum,
  p50/p75/p90: 0.96/1.06/0.97 and 1.05/1.06/1.05 (two runs); 90 % reached at 9-11° (p75) to 15-21° (median). Tarmac by
  the same method: Correvit (Fig. 4.29) 0.76/0.77/0.74 at 28-32°, β sensor (Fig. 4.30) 0.71/0.83/0.82 at 26-28°,
  against the bench data's 0.73-0.90 (tarmac-mu-y30-over-peak; NGP 0.865, the brush 0.833): the method does not
  overstate the fall. The text's "1250 N at 30°" (0.63) is outside the figures' envelope (a few sparse traces). So the
  stage-speed rows (`gravel-cw-*`, at 20 m/s, 4 kN, WaterFactor 1) are 0.93-1.00 and 9-22°, the argmax and Velenis
  rows context. The saturation angle is the first slip angle at 90 % of the maximum (lab `alpha90`, maximum over
  0-40°; figures, of the 10-32° plateau). CW's median rise fits the shear law with α0 8-12° at stage speed, as
  Coutermarsh's wet gravel does (9.7°) at 1-1.5 m/s: the rise shows little speed dependence, so α0 follows the water
  and not the speed. The gravel shape rows are taken at their rig's 1.5 m/s (`gravel-1.5`, `gravel-wet-1.5`). No other
  gravel tyre force against speed was found (two searches; Krueger, Auburn MSc, an ATV tyre on a towed gravel rig, not
  read).
- Not adopted (2026-11-08, `tyre/gravel-speed`): a peak with a fall toward stage speed: CW's digitised p75/p90
  envelope stays flat from 12° to 32°, and his text's 0.63 lies outside it; the shear law passes both ends. Wet α0 on the lateral axis only
  (longitudinal kept dry): wet 345 locked over ABS K 0.15 1.651 against 1.894 both axes (main 1.648); no wet-gravel
  braking data choose between them, both axes keep the law's one K.
- Wet gravel (2026-11-08, `tyre/gravel-speed`, main 8e65298c → wet α0, experimental-brush; dry gravel and snow
  byte-identical, traces compared on RSF 104 for all three cars, ngp identical): scorecard 80/112 (was 76; ngp 37/112),
  the four wet-gravel rows MISS → PASS (0.227/0.404/0.648/0.797 at 2.5/5/10/15°, was 0.327/0.547/0.798/0.912),
  CW rows 0.973 (was 0.995) and 21.0° (was 14.25°). Fiesta Rally2 RSF 345 `--wetness wet` from 50 km/h declutched:
  locked 0.815 g (was 0.820), ABS K 0.15 0.430 g (was 0.497; locked over ABS 1.894, was 1.648), K 0.2 0.388 g
  (was 0.479); r40 circle wet 0.415 g (main: no latch; dry 0.664 both). Standstill after the stop, peak speed per
  half second, wet: full pedal 0.068/0.011/0.003/0 m/s (was 0.112/0.026/0.008/0.003/0), 0.35 pedal
  0.094/0.024/0/0 (was 0.097/0.020/0.010/0.005/0.004/0.011/0.087/0.012). Stage times (s), wet: ngp / main eb / eb; dry eb as main (RSF 111 287.08/286.37/298.19, 389
  176.75/177.40/183.42, track-11_O 636.60/640.27/652.44 for Fiesta/Celica/Golf):

  | Car | RSF 104 wet | RSF 111 wet |
  | --- | --- | --- |
  | Fiesta Rally2 | 326.73 / 323.70 / 326.46 | 358.89 / 352.46 / 354.62 |
  | Celica ST185 | 326.72 / 324.40 / 328.15 | 360.57 / 355.63 / 357.91 |
  | Golf II GTI 16V | 341.95 / 336.43 / 339.42 | 375.50 / 366.33 / 368.72 |

  Wet α0 costs eb 2.2-3.7 s (0.6-1.2 %); eb stays 0.1-1.8 % ahead of ngp on wet, except the Celica on 104 (0.4 % behind).
- Dry, damp, wet gravel in game (2026-11-09, main 6258bd51, no model change; RSF 345 `--wetness`, from 50 km/h
  declutched, `harness/brake_test.py`, raw decel g; "level" = the best threshold run; circle `--circle 1051,-166,40
  --circle-ramp 0.4`, 40000 steps, the car_pace first-hold read and the best 1 s steady hold, |off| < 3 m, before the
  first stop; ngp / eb):

  | Car, wetness | Locked | Level (best K) | ABS K 0.15 | ABS K 0.2 / 0.3 / 0.5 | Locked / ABS 0.15 | Locked wheel slip | Circle first-hold / best steady |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | Rally2 dry | 0.628 / 0.882 | 0.732 (0.5) / 0.754 (0.7) | 0.573 / 0.577 | 0.637 / 0.683 / 0.720; 0.635 / 0.711 / 0.766 | 1.095 / 1.529 | 1.00 / 0.946 | 0.571 / 0.664; 0.706 / 0.692 |
  | Rally2 damp | 0.582 / 0.880 | 0.692 (0.5) / 0.729 (0.7) | 0.553 / 0.523 | 0.602 / 0.636 / 0.679; 0.579 / 0.655 / 0.733 | 1.054 / 1.684 | 1.00 / 0.966 | 0.580 / 0.689; 0.709 / 0.704 |
  | Rally2 wet | 0.494 / 0.815 | 0.601 (0.5) / 0.645 (0.7) | 0.504 / 0.430 | 0.540 / 0.580 / 0.575; 0.496 / 0.565 / 0.625 | 0.980 / 1.894 | 1.00 / 0.992 | late latch / 0.415; 0.743 / 0.632 |
  | Escort dry | 0.612 / 0.763 | 0.653 (0.5) / 0.694 (0.7) | 0.605 / 0.611 | -, 0.717, 0.734; -, 0.725, 0.757 | 1.011 / 1.249 | 1.00 / 0.889 | no latch both (weak, as before) |
  | Escort damp | 0.552 / 0.761 | 0.610 / 0.675 | 0.480 / 0.544 | -, 0.661, 0.692; -, 0.660, 0.731 | 1.150 / 1.399 | 1.00 / 0.934 | |
  | Escort wet | 0.474 / 0.723 | 0.550 / 0.603 | 0.291 / 0.432 | -, 0.337, 0.602; -, 0.583, 0.632 | 1.629 / 1.675 | 1.00 / 0.982 | |

  The Escort ngp wet ABS K 0.15 / 0.3 rows are the rig's: its rears sit at slip 0.33 without cycling. eb's level wet
  over dry 0.855 (ngp 0.82), damp 0.97 (ngp 0.95); the shear law has no peak below lock (95 % of the level at a
  braking slip of 0.20 dry, 0.22 damp, 0.27 wet; s0x 0.081 / 0.092 / 0.125), the force's maximum is the berm at lock;
  ngp peaks at K 0.5 and locked is 0.82 of it.
- Why wet locked over ABS is 1.894 (eb, Rally2; the two factors measured): locked / ABS = (locked / level) × (level /
  ABS K 0.15) = dry 1.170 × 1.307 = 1.529, damp 1.207 × 1.394 = 1.684, wet 1.264 × 1.500 = 1.894. (a) The fixed slip
  target on the slower rise: the ABS wheels hold a mean slip of 0.12-0.14 from free rolling at K 0.15, where the law
  gives 0.84 of the level dry and 0.70 wet (measured ABS over level 0.765 / 0.667, the cycling below the steady
  curve); the same fraction on wet needs K ≈ 0.23 (0.15 × s0x wet / dry), which measures ~1.55 (K 0.2 1.642, K 0.3
  1.443), dry's value. (b) The berm reached in full: the wet wheels lock (slip 0.992 against dry 0.946: the lower wet
  force no longer holds them off lock against the full-pedal torque), so the locked run reaches PLOUGH's 1.3 (1.26
  over the level) where dry stops at 1.17. Neither is a defect of the law; (a) is the test's fixed K, (b) the berm's
  dependence on whether the wheel locks. The published 1.12-1.59 are dry; no wet gravel locked/ABS data exist.
- Why the wet circle is 0.63 of dry (eb, Rally2): the first-hold read stops at the first slide, and on wet that slide
  comes at 0.40-0.46 g (launch +12 to +15 s): under the hold's throttle 0.3-0.5 the wheels turn at 1.13-2.0 times the
  car's speed (dry 0.9-1.2 outside short spikes), the rear slip angles pass the fronts' (18-27° against 10-15°), and
  the car spins (both axles past 40° by +14.5 s): power oversteer, the rear axle saturating first under combined slip.
  The same run then holds 0.63 g steady (0.91 of dry's 0.69 by the same read). The wet tyre's cornering slope is 0.65 of dry (tan 6.3° / tan 9.7°) and its driven slip for a given force 1.55
  times dry's, so the autopilot's line holding wanders (off -3 to +4 m in the first hold against ±1 m dry): the 0.63
  is the read's first slide and the autopilot, not the wet lateral level (0.855 of dry). ngp's wet first hold latches
  late; its best steady hold 0.743 is above dry's 0.706 (single runs, the autopilot's wander; weak).
- Moisture against the data (2026-11-09, T57, `research/gravel-moisture`, no model change; sources in [Published
  targets](#published-targets)). The one road measured both ways (Moyer's Iowa gravel, a sprinkled surface) skids locked
  at 1.06-1.18 of dry and sideways at 0.97-0.98: no loss; the reconstruction table agrees (loose gravel wet 0.45-0.75
  against dry 0.40-0.70, basis unknown) and NHTSA's one wet-gravel car stops locked at 0.95 of the dry fleet (weak). The
  loss comes with a saturated, fines-rich road: Team O'Neil's silty-sand hard-pack after 3.3 cm of rain drives at 0.70
  of dry (one site, two days), and road authorities rate gravel slippery when wet only with excess fines or loose
  material on top. Soil data put the moisture optimum in the middle (traction ±7 %, cohesion from suction), so a damp
  bound road can match or pass a dry loose one; nothing measures damp gravel itself, and nothing ties moisture to the
  loose layer's depth (only the fines). Against the game, locked wet over dry from 50 km/h (RSF 345, Fiesta Rally2,
  `gravel-car-wet-over-dry-locked-50`, context: Moyer's 1.06-1.18 against CRREL's 0.70): ngp 0.787 (0.494 / 0.628 g),
  brush 0.922 (0.822 / 0.892 g); damp ngp 0.928, brush 0.968; best threshold wet / damp over dry ngp 0.821 / 0.944,
  brush 0.848 / 0.931 (context). NGP's × 0.925 / × 0.85 fit a saturated silty road (0.70) more than a wetted well-graded
  one (~1.0); both are real gravels, and the data cannot pick one level for "wet". The brush's wet α0 (9.7°) compounds
  the level at the side: by the law, μ_y at 15° wet over dry is 0.81 / 0.78 / 0.74 at 9 / 17.5 / 25 mm (shape 0.95 /
  0.92 / 0.87 × 0.85; inferred), against Moyer's 0.97 at 15° on a sprinkled road; the slower rise rests on Coutermarsh's
  wet gravel (another site and soil) and Croft-White's wet stage (α0 8-12°). Proposal (the user's decision): the
  evidence supports a smaller wet loss on well-graded gravel, damp × 1.0 and wet × 0.95 (Moyer's locked and sideways
  means, NHTSA's 0.95), keeping × 0.85 only if "wet" means a saturated silty road; and, if wet keeps × 0.85, the brush's
  wet α0 could go to dry's. No change made. Two more sources (T58, 2026-11-09): Warner 1983 prints Baker's table
  first-hand (Ref. 1 p. 210, basis still unknown; loose wet ≥ dry) and has no gravel tests of its own. González 2015
  sprinkles MgCl-treated roads: wet over normal 0.78 well-graded, 0.77 loose on a hard base, 0.60 sealed (fines crust;
  same section, inferred), and their review gives Paige-Green's gravel roads 0.90 wet over dry (different sections).
  Both keep T57's picture: no loss on clean wetted gravel (Moyer), ~0.9 on ordinary gravel roads tested wet
  (Paige-Green), 0.6-0.8 where water makes a slurry of the fines (CRREL, MgCl); damp × 1.0 is unaffected (no damp test;
  the MgCl humidity loss is the salt's), and wet × 0.85 sits inside the 0.76-0.90 of the two gravel-road data sets (×
  0.95 would sit above both).
- Damp gravel at the dry level (2026-11-09, T59, `tyre/gravel-damp`, a user decision, experimental-brush only):
  `loose::gravel_damp_level` scales the loose level by 1 + (1/0.925 - 1) times the damp tent (`brush::damp_weight`: 1
  at W 0.25, 0 dry and wet, linear between) times the gravel class's share; grass, rock, snow, tarmac, the tyre's own
  wet slope and the wet α0 unchanged. Materials (`materials` example, every class-1 id: 1-51, 153-155, 177-199): damp
  is the dry static and soil static × 0.9250 (±0.0001, the print's rounding), wet × 0.850 fine, 0.845 medium, 0.8525
  coarse. Evidence: Moyer's sprinkled gravel locked 1.06-1.18 of dry, no damp loss measured, wet roads 0.76-0.90 of dry
  (above). Fiesta Rally2, RSF 345 from 50 km/h declutched, brush, damp at the dry level (was: locked 0.863 g,
  best threshold (K 0.7) 0.714, ABS K 0.15 0.560; dry 0.892 / 0.767 / 0.624): locked 0.897 g, best threshold (K 0.7)
  0.784, ABS K 0.15 0.618; ngp, brush dry and wet, `experimental` damp traces bit-equal. Flat gravel circle
  (map 85 under 17 / 18, tyre 3, r 40, ramp 0.4), kin g damp 0.979 (was 0.888; dry 0.987). Scorecard:
  `gravel-car-damp-over-dry-locked-50` 1.006 (was 0.968), `-best-50` 1.022 (was 0.931; context; the threshold's best K
  is a single run each); no other row moves.
- Not adopted (2026-11-08): the exponential approach on the brush's own stiffness (NGP's slope, u = 3ψ, the brush's
  adhesion/sliding split): pea stone 0.38/0.61/0.85/0.95 at 6.7 kN and 0.98 at 20°, all four pea rows still miss; one
  isotropic s0 on both axes (the soil's K alike, Cx/Cy = μx/μy): locked over ABS K 0.15 1.67 at 50 km/h (band ≤ 1.59),
  1.44 at 80, everything else as adopted.

### The soil's depth: NGP's materials and the surface across the road

2026-11-08. NGP's soil materials (lab `materials`, NGP 7.4.778's stage init, dry; damp / wet take μ × 0.925 / 0.845-0.8525,
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
- Decision (2026-11-08; the shape went by depth on 2026-11-09, below): the level and the berm stay independent of the
  thickness; a windrow less grippy than the line (published 0.80; NGP's deeper against shallow 0.97) would be a
  grip-level change: the user's decision.

Gravel's shape by the depth (2026-11-09, `loose::GRAVEL_DEPTH_ALPHA0_DEG`). Evidence for the build-up against the
loose layer (labels as above):

- No source measures C_α/Fz or α90 against a gravel layer's depth. The direction, firmer builds faster: loam bin at
  cone index 150-900 kPa, initial slope B·LFCmax = 2.18 + 0.38 EMOB (significant at 2 %, ~2.3 → 10 /rad), LFCmax no
  significant trend (Gee-Clough & Sommer 1981 via El-Razaz, Leeds PhD 1988, Fig 2.5; quoted, fig ±0.3); McAllister's
  38 field soils the same sign (ibid., quoted); Metz's B gravel 0.19, meadow 0.15, ploughed 0.07 per deg (table);
  pea stone α0 ~6° against loose wet gravel ~11° (Coutermarsh Fig. 15, confounded with moisture and site).
- The layers: TR-10-11's pea stone (Coutermarsh's curve) is 0.64 cm stone on recycled asphalt, its layer depth not
  stated; repeated hard turns built it up to 10 cm (slalom spots 4-9.5 cm), over 20 cm in places; Clegg CBR 4-15 loose
  against 67-188 for the exposed competent layer (p. 18, Table A2; quoted, table). Hindiyeh's lot is "a layer of gravel
  with asphalt underneath", swept bare in places (quoted; α90 2.6-3.3° per axle). Krueger's ATV tyre on 25-50 mm loose
  limestone over a very stiff base: C_α/Fz 4.6 / ~2.8 / 2.4 /rad at 50 / 185 / 320 lbf (Fig 3.32, p. 101, fig ±0.5
  lbf/deg). El-Razaz's Schwanghart (7.50-16 tractor tyre) on gravel: 0.33-0.50 peak at 15-25°, ~3-3.5 /rad (Fig 2.7, fig
  ±0.05). Holloway 1989 (via McIntyre Fig 52): an ATV tyre on clay/gravel still rising at 15° (fig ±0.03). Croft-White,
  Roodt, SATC, McIntyre's clay, CRREL 95-1, ERDC 3-666: no depth or firmness link to the build-up (Roodt: more and
  coarser loose chips on a sealed road give a lower locked μ, level only). Macnabb (ESV 98, ABS on a recently graded
  loose gravel road, silty sand with gravel): braking only, no lateral curve and no layer depth (its depth column is the
  tread's).
- Model: dry α0 3.4° at 9 mm, 4.8° at 17.5 mm, 6.3° from 25 mm, linear between, the ends held, on NGP's blended
  `SoilThickness` (`ForceInput::soil_thickness`, scratch `+0x258`, already in the model's input: no new layout), times
  the wet ratio 9.7/6.3 by the WaterFactor. Anchors (ours, the direction from the evidence above): 9 mm α90 8° between
  the firm lot (3°) and Croft-White's stage road (~12°, very wet: ~8° dry by the wet ratio), 17.5 mm 11° that road,
  25 mm pea stone (its layer is deeper than NGP's deepest). Fitted with `force_sweep --soil` on the scorecard's gravel
  probe (4 kN, 15 m/s): α90 / C_α/Fz dry 8.0° / 11.2, 10.25° / 9.1 (15 mm), 11.0° / 8.4, 13.25° / 7.2 (22.5 mm),
  14.25° / 6.6; wet (WaterFactor 1) 12.0° / 7.8, 15.25° / 6.2, 16.75° / 5.8, 19.75° / 4.8, 21.0° / 4.5; ngp 7.5° / 9.1
  on every depth. Caveat: Croft-White's ~12° is a wet road, ~8° dry by our weak wet/dry ratio, so a dry gravel
  road's α90 is 8-12° and the anchors inside that span are ours. A gravel edge blended with grass (30 mm) takes the
  blend's thickness: a softer verge.
- Depth shares along the driveline (lab `surface_across DATA STAGE driveline`, every 2 m, the blend's weights, `.mat`
  new dry; the driveline gives RSF 345 40/60 % shallow/deep, 14.1 mm against the autopilot line's 37/63 %, 14.3 mm),
  shallow 9-15 / deep 17.5-22.5 / deeper 25-30 mm of the gravel: retail 24 gravel stages (133 km) 33 / 48 / 19 %
  (UK 20-25 mostly deeper, means 19-25 mm; Australia 31-36 deep, 16-18 mm; Japan 51-56 mixed, 17-18 mm; USA 61-66
  shallow, 9-14 mm); RSF 146 installed gravel stages (802 km) 46 / 35 / 19 %, 67 stages mostly shallow, 52 deep, 27
  deeper (10 over half non-gravel). So under the line two thirds of the gravel driven is firmer than pea stone.
- In game (Fiesta Rally2, experimental-brush, single runs): scorecard (gravel, `--jobs 6`) 33/35 PASS both, ngp
  rows bit-equal; flat r40 circle (map 85) under 17 (9 mm) 0.994 g (was 0.955), under 9 (17.5 mm) 0.986 (was 0.978),
  under 1 (25 mm) 0.972 (unchanged); RSF 345 circle 0.667 (was 0.664); locked over ABS K 0.15 on 345 50 / 80 km/h
  1.442 / 1.292 (was 1.529 / 1.340); `gravel-peak-kappa` 0.42 (was 0.58; band 0.4-0.7, the 9 mm probe);
  `gravel-cw-alpha90` 12° (was 21). Brake test from 50 km/h, RSF 111 (9 mm): ABS 0.845 g (was 0.760), locked
  0.955 (was 0.948), locked over ABS 1.130 (was 1.247; band 1.12-1.59); RSF 105 (17.5 mm): ABS 0.677 (was 0.703),
  locked 0.944 (was 0.957), locked over ABS 1.394 (was 1.361; its K 0.3 run 0.64 g, was 0.80, ends on grass 97:
  the approach, not the law, which is at its level by κ 0.3).

## Car-level targets on loose surfaces

Measured whole-car behaviour (and per-tyre forces where open) on gravel, snow and ice, to judge a loose-surface tyre
model against data rather than feel. Research only (2026-11-07, second round); the full findings, sources, digitised
traces and scripts are kept outside the repository (not in git). Labels as above; g from a stop is v²/2s (inferred, a
lower bound: it includes the pedal rise). Every braking series is road cars on road tyres; none is a rally tyre.
The scorecard takes those gravel levels as lower bounds for a rally tyre (decision 2026-11-08): a raced line alone
grips about 1.2 times the loose windrow (TR-10-11 Table 3), and a WRC car's 0.6-0.7 g lateral (CW) times the measured
locked/lateral 1.3-1.5 gives 0.8-1.0 g locked (inferred). CW's WRC tyre μ_y ≲ 0.67 stayed an upper bound until
2026-11-09 (a very wet, loose road: lower bound only, the level against real Rally2). NGP gives a car's tyre no era:
`Performance_NGP` is 1.0 on 38 of the 42 RSF cars (Group B and Group 4 alike), 0.95 on the two R1 cars, 0.985 front /
1.015 rear on both 911s.

| Target | Value and conditions | Source (precision) | How ReRBR reproduces it (run, car, metric) | Tooling missing |
| --- | --- | --- | --- | --- |
| Locked-wheel braking on gravel | 0.47-0.59 g, 9 light vehicles, 56 km/h, 5 cm #617 crushed limestone, shortest of 3 stops; 0.59-0.66 g, 7 SUVs, pickups and cars, 50 km/h, freshly graded silty sandy gravel; 0.53-0.64, Polo, 40-80 km/h compacted; no speed trend over 40-80 km/h. NGP (declutched brake test, RSF 345): 0.63 g from 50 km/h, 0.77 g from 100 km/h | NHTSA DOT HS 808 875 Tables A1/A2 (table; g inferred); Macnabb et al. ESV 98-S2-W-36 Table 4 (table, accelerometer); Proctor-Parker SATC 2019 Table 1 (table; g inferred) | `harness/brake_test.py` on RSF 345 from 40-80 km/h, Fiesta Rally2; the locked run's mean decel | none |
| Locked over ABS on gravel | decel ratio 1.25 laden / 1.30 light, per car 1.12-1.42; 1.21-1.59 per car. Same method on wet Jennite (hard, SN 30/10): 0.54-0.62. NGP: tyre locked 0.82 of peak; brake test locked over best threshold decel 0.86 (50 km/h) / 0.91 (100 km/h) | NHTSA Tables A1/A2 (table; ratio inferred, reproduces SAE 1999-01-1287 Table 2); Macnabb Table 4 (table; ratio inferred) | `harness/brake_test.py`: declutched locked against `--brake-abs` K 0.15 (per-wheel ABS) on the same stretch; ratio of mean decel | none |
| Gravel against tarmac, braking | same car locked gravel / ABS dry pavement, both from ~50 km/h, 0.69-0.84 (Macnabb); NHTSA cross-speed pairs (gravel locked from 56 km/h over the references below) 0.56-0.82; tarmac reference by the same method: ABS wet asphalt (SN 85/65) 0.69-0.84 g from 80 km/h, dry concrete 0.69-0.88 g from 97 km/h. NGP, declutched, same speeds: locked gravel (345) / locked tarmac (568) 0.71 at 100 km/h, 0.59 at 50; locked gravel / best threshold tarmac 0.59 / 0.50 (rally tyres, the published pairs road tyres) | Macnabb Tables 3-4 (table; ratio inferred); NHTSA Tables A1/A2 (table; g and ratios inferred, context) | `harness/brake_test.py` at one speed on 345 and 568, same car; ratio | none |
| Car lateral g on gravel | 0.43-0.54 g, CIV, rally drivers, hard turn at ~48 km/h, pea stone (CIV lateral coefficient 0.35-0.41 on the same passes); Explorer 0.405 / 0.536 g at 48 / 64 km/h entry; WRC 0.6-0.7 g (CW: a peak body lateral on wet loose gravel, not a steady dry limit; the WRC+ telemetry's 1 s p90 is 0.73-0.92 g, row below). ReRBR on RSF 345's sloped circle (terrain, not grip; flat circle 0.94-0.99 g, [Car-level grip in the same terms](#car-level-grip-in-the-same-terms)), ramp 0.4, ngp / experimental-brush: Fiesta Rally2 0.571 / 0.664 g, i20 Coupe WRC 2020 0.587 / 0.739, Fiesta WRC 2019 0.749 / 0.423 (single runs; ramp 0.25 gives 0.36-0.47 g, an inward weave of the autopilot, [Real stage telemetry](#real-stage-telemetry-wrc-gravel)) | TR-10-11 Fig 12 (fig ±0.01), Table 6 (table); Croft-White (quoted) | `--circle 1051,-166,40 --circle-ramp 0.4` on 345, Fiesta Rally2; steady lateral g before it runs wide | none |
| Trail-brake by an expert on gravel | Group A Impreza STI (AWD, 340 bhp), track gravel, 80-90° corner: 77 → 23 km/h over 1.05-4.05 s (0.51 g mean, 0.66 g peak over 0.5 s), rear-axle slip angle 42° at 4 s, back to 0 by 5 s; pendulum turn −29° → +63°, 0.59 g peak | Velenis et al. MED 2007 Figs 3, 5 (fig ±1 km/h, ±1°; digitised by pixel; inputs normalised); no data release found | RSF Impreza GC8 555 GrpA on a gravel 80-90° corner, inputs of the same shape; speed and rear-axle slip angle traces | a payload writer from an input CSV, assumed steering, brake and throttle scales, a matching corner; shape check only |
| Snow braking | road winter tyres 0.33-0.45 g (50-0, 35-5, 25-5 km/h, ABS); locked 0.31-0.35 g; trailer μx,max 0.43-0.57 (new winter tyres), μy,max 0.47-0.56 at 30 km/h, no clear fall past the peak. NGP: tyre peak 0.85; car not measured | Auto Bild 2025, Vi Bilägare 2026 studded (distances, secondary; g inferred); VTI R875 p. 45 (quoted), Tabell 55, 60, 62-63 (table); Sokolovskij 2007 (table), SAE 960652 (quoted) | `--brake-from 13.9` on map 10 (`track-10_O`), Fiesta Rally2; mean decel | none (rally snow tyres are studded: the level is a lower bound) |
| Ice braking and cornering | car 50-0 km/h: studded 0.23-0.30 g, studless 0.18-0.22 g; best three at −2/−7/−13 °C 0.24/0.28/0.31 g studded, 0.17/0.22/0.26 g studless. Rig, smooth ice −3 °C, 30 km/h, 4.2 kN: μx peak/locked 0.25/0.22 studded, 0.21/0.165 Nordic (locked over peak 0.88 / 0.77); μy peak 0.22 at ~5° / 0.19 at 2-3°, at 20° 0.19 / 0.13. NGP: material 116 μ 0.10 (0.095 on the tarmac tyre), tyre peak at 0.75°; car from 50 km/h on the flat 0.096 g best threshold, 0.095 g locked (ratio 0.99); experimental-brush 0.100 / 0.081 g (0.81) | Tekniikan Maailma 18/2025 (table, secondary via tyrereviews); VTI R875 Tabell 46, 48, Figur 3 (table; means inferred; fig ±0.01) | `brake_test.py` on RSF 504 with `-- --material-as 116` (iced from the brake onset; `decel_flat`); map 85 circle iced (`--circle -260,120,40 --material-as 116`) | a flat iced straight (504 rises 6.6 %); no car lateral g target on ice |
| Rally braking and lateral g, real stage | WRC 2017-21 cars, 18 drives on two gravel stages (medians, range): 16 longest braking events 0.60 g (0.41-0.90) from the speed, peak 0.5 s 0.83-1.06 g; lateral 1 s p50 / p90 / p99 0.40 / 0.89 / 1.25 g Finland, 0.30 / 0.74 / 1.02 g Wales; g-g p95 of \|lat\| about 1.0 g from 30 to 150 km/h. ReRBR autopilot, RSF 524 Hassi (SS7's last 7.7 km), 3 cars × ngp / experimental-brush: braking 0.40-0.44 g, lateral p50 / p90 / p99 0.24 / 0.38 / 0.45 g, the autopilot's plan (4 / 5 m/s²); at the car's limits (`--plan`) 0.58-0.62 g, 0.28-0.34 / 0.49-0.57 / 0.64-0.74 g, 292-315 s against the real 207-216 s | WRC+ merged telemetry via RallyDataJunkie, Rally Finland 2021 SS7, Wales Rally GB 2017 SS10 (10 Hz; units and axes inferred, lateral checked against GPS) | `harness/rally_gg.py run` and `stats` (`data/rally-telemetry-gravel.csv`), `harness/limit_pace.py` | the driver: at its limits (`--plan`) the autopilot brakes as the real drives but holds 0.55-0.64 of their lateral p90 on the driveline, with single runs 5-15 s apart, so the times do not rank the tyre (weak) |
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
2. Ice (VTI rig and magazine data are solid): runs on material 116 now (`--stage pad-116`); the level is NGP's
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
- Full throttle on straights (2026-11-10; `harness/rally_wot.py`, the samples and the fit in its docstring; the
  2017-21 World Rally Cars from the WRC's archive: Rally de España 2021, tarmac, 104 drives; Rallye Deutschland 2019,
  tarmac, 140; Rally Finland 2021, gravel, 112). Median level acceleration, g, España / Finland / the game's i20
  Coupe WRC 2020 flat out on pad-tarmac under ngp / experimental: 4th 110-120 km/h 0.384 / 0.334 / 0.457 / 0.492; 5th
  130-140 0.296 / 0.263 / 0.385 / 0.424; 6th 150-160 0.214 / 0.182 / 0.314 / 0.342; 170-180 0.143 / 0.108 / 0.243 /
  0.267; 180-190 0.072 / 0.053 / 0.210 / 0.231; 190-200 0.056 / 0.014 / 0.178 / 0.199 (pad-gravel, experimental, 6th:
  0.323 / 0.244 / 0.212 / 0.098 at 150 / 170 / 180 / 190). The real models agree (tarmac, 4th 110-120: Yaris 0.385,
  Fiesta 0.384, i20 0.384); the game's cars pull 1.2-1.3 times as hard at 115 km/h and 3 times at 185.
  - Fit (1390 kg, Crr 0.015, ρ 1.17): España CdA 1.76 m² (5-95 % 1.69-1.82), wheel power 219 / 224 / 231 / 219 / 196
    kW at 6500 / 7000 / 7500 / 8000 / 8500 rpm; Deutschland 1.71 (1.67-1.77), 230 / 233 / 229 / 232 / 196; per model
    1.55-1.83. España with 1350 kg, ρ 1.12, Crr 0.010 or only above 130 km/h: CdA 1.72-1.85, power 213-235. Finland
    (Crr 0.018, ρ 1.24): 1.56 (1.44-1.69), 205 kW at 7000, its drag not separated from its rolling loss.
    Deutschland 2016 (Polo R, i20, Fiesta RS WRC; no altitude, 107 drives): 0.83 (0.63-1.06), weak.
  - The game by the same fit: ngp CdA 0.79 m² (the file's `Cd * A` 0.9725 is 0.81 m² in SI: NGP's drag factor is
    0.5 ρ / 1.204), wheel power 225-237 kW; experimental 0.88, 249-258 kW.
  - So the 2017-21 cars' drag area is 2.1 times their files'; their wheel power is ngp's (the file's 380 PS less 12 %
    over 1.05 = 234 kW), and the experimental engine's whole torque is about 10 % over the telemetry's (at 1390 kg;
    the power read scales with the mass). Gears 2-5 hold within 2 % (rpm per km/h, real i20 88.5 / 72.6 / 60.1 /
    50.7 / 44.5, file 88.8 / 71.8 / 58.8 / 49.1 / 41.7), 6th is 5 % tall. Top speed 196-202 km/h in both: the real
    cars against their drag (0.056 g left at 195 km/h, 8350 rpm), the game's on the limiter with 0.2 g in hand.
  - Every file's `Cd * A` / 1.204 is 0.60-1.0 m² whatever its downforce (Golf II GTI 0.68, Escort Cosworth 0.78,
    Fiesta Rally2 0.65 with 3100 N, the WRC 2019 cars 0.81 with 3700 N: downforce over drag 3.8 and 3.6; 2.0 with the
    measured drag). Dominy & Richardson's 2004 model: 0.85 m². No Rally2 / R5 data (WRC2's are 6.4 s GPS points).
- At the car's limits (`harness/limit_pace.py`, `rbr-standalone --autopilot --plan LAT,BRAKE --top-speed 60`;
  2026-11-08, after T18's loose-v1 merge, which leaves these dry runs byte-identical). Limits on RSF 345, r40 circle
  lateral g / best threshold braking g from 80 km/h (slip 0.5, the top target tried, best on every car) / locked g:
  ngp Fiesta Rally2 0.571/0.826/0.708, i20 0.587/0.819/0.709, Fiesta WRC 0.749/0.820/0.709; experimental-brush
  0.664/0.840/0.908, 0.739/0.838/0.909, 0.423/0.837/0.908 (the brush stops shorter locked than at slip 0.5; the brush
  Fiesta WRC circle reads low, so its search held braking at 0.9 and scaled the lateral alone). Plan = factor ×
  limits, factors 0.8-1.2 by 0.1 and 0.05 next to the edge, single runs; clean = finished, no recovery, the trace's
  `off` under 3 m all the way (the default runs keep under 1 m). Fastest clean (factor, plan lat/brake m/s², time s;
  default autopilot): ngp Rally2 1.0, 5.60/8.10, 314.9 (353.5); i20 1.1, 6.33/8.84, 301.0 (350.1); Fiesta WRC 0.95,
  6.98/7.64, 291.9 (350.1). Brush Rally2 0.9, 5.86/7.41, 310.4 (349.1); i20 0.85, 6.16/6.99, 304.5 (346.4); Fiesta WRC
  1.45 (braking 0.9), 6.01/7.39, 304.7 (346.4). The next factor up runs 3-6 m off the line (a slide wide or a spin);
  neighbouring factors differ by 5-15 s, so these times do not rank ngp against the brush. Under ngp the plan holds
  1.0-1.1 of the circle, under the brush 0.85-0.9.
- At-limit statistics (`rally_gg.py stats`, the six chosen runs; telemetry Finland medians in brackets): top 145-156
  km/h (196-202), mean 90-97 (125); braking 16 longest 0.58-0.62 g (0.60), peak 0.5 s 0.74-0.79 (1.00); lateral 1 s
  p50 / p90 / p99 0.28-0.34 / 0.49-0.57 / 0.64-0.74 g (0.39 / 0.89 / 1.25); g-g p95 \|lat\| per 30-60 / 60-90 /
  90-120 / 120-150 km/h 0.65-0.86 / 0.63-0.74 / 0.45-0.56 / 0.35-0.45 g (0.96 / 1.04 / 1.08 / 1.14), decel 0.59-0.86
  / 0.71-0.76 / 0.74-0.79 / 0.68-0.78 g (0.78 / 1.11 / 1.10 / 1.01). Braking now matches the real mean; lateral p90
  is 0.55-0.64 of the real one and falls with speed, where the real level is flat.
- RSF 524 is SS7's road from 10.24 km to the finish: the run's path fits the 2021 GPS track (rigid fit, no scale or
  mirror) at 8-9 m RMS per driver (the best fit anywhere else 160 m); the first ~70 m of RSF 524 are off the GPS
  track (a start spur), the finish within 3-7 m; the speed profiles line up corner by corner (the 25 km/h hairpin at
  4.95 km in both). Real drivers over that part (from the spur's junction, flying, entry 40-100 km/h): 206.7-216.0 s
  (Evans 206.7, Breen 206.9, Rovanperä 207.7, Ogier 209.1, Lappi 209.2, Katsuta 211.6, Greensmith 215.4, Fourmaux
  216.0); the at-limit runs about 287-310 s over the same part (race time less ~5 s to 70 m), 1.4 times slower; the
  default autopilot about 1.65 times.
- What still limits the comparison: the line (the autopilot follows the driveline, the road's centre, where drivers
  cut and widen corners: the same lateral g at a larger radius is a higher speed); the speed plan (one lateral and one
  braking value, separate, no friction circle: braking runs to the corner's limit speed, and a downshift under
  braking slips the driven wheels and breaks traction in the turn, where a driver blips); its steering (pure pursuit
  at a fixed gain, no counter-steer, so a slide ends the run: the next factor up runs wide or spins); the cars' circle
  limits (0.57-0.75 g against 0.9-1.1 g real: the physics, not the driver); aero (the real 150-200 km/h sections are
  not reached here); and the driver, a WRC driver on a recce'd stage against a controller.
- The r40 circle (Fiesta Rally2, RSF 345, ramp 0.4, `--tyre-probe-every 10`, 1 s means; experimental-brush, ngp the
  same pattern; superseded: the circle lies on an 11.9° slope and its hold ends running downhill — see the flat
  circle in [Car-level grip in the same terms](#car-level-grip-in-the-same-terms), which saturates all four tyres.
  At
  ramp 0.25 the "slide" is the autopilot weaving inward (4 m inside, steering crossing over) at 0.44 g, not grip: the
  circle row reads the car and the controller together.

## Loose levels: rally data against the game (2026-11-08)

Research (2026-11-08, third round; downloads outside the repository) and a check on main 0fb71ed8. Labels as above.

| Quantity | Published | Source (precision) |
| --- | --- | --- |
| Rally-tyre level on gravel | none quantified: no tyre-maker, team or test number for a rally tyre against a road or AT tyre; AT tyres on gravel, ABS, 0.52-0.56 g from 40 km/h, 0.66-0.73 g from 80 km/h | tyre magazines (secondary, g inferred from distances) |
| Locked over ABS on gravel | 1.25 (locked 0.58 g, 30 mph, "peak friction at 100 percent slip", locked fronts "plow aside the looser material"); 1.18-1.40 over 9 cases (one Polo, 40-80 km/h, new / 38 % / smooth tyres), locked 0.53-0.64 g (new tyres 0.61-0.64) | Clemett & Moules, HRR 477 (1973) Table 1 (table, g inferred); Proctor-Parker & Stopforth, SATC 2019 Table 1 (table; g and ratios inferred) |
| Studded snow tyres on compacted snow (0 °C) | peak traction rib 0.28-0.32, snow tyre 0.34-0.41, studded 0.57-0.60; spinning 0.12-0.16 / 0.13-0.15 / 0.28-0.43; braking peaks at 40-60 % slip on snow and studded tyres, "the differences between these maximum values and ... 100 percent slip are small"; locked/peak rib 0.82, chained rising to 1.47 at lock | Ichihara & Mizoguchi, HRB SR 115 (1969-70) Tables 4-5, Fig 2 (table, quoted, fig ±0.02) |
| Snow, road winter tyres | ABS 0.405 g mean (25-5 km/h, ≥ 20 runs), FWD traction 0.227 g; studded and Nordic studless equal on snow (-0.07 m/s², not significant); car on 7.7-15.4 m circles 0.25-0.40 g on snow, 0.1-0.2 g on ice | VTI R875 §3 (table); Lu 1994 via WSDOT T2695 (secondary) |
| Ice, studded against not (road studs 1.1-1.8 mm) | 0 °C smooth ice: optimum slip 0.09-0.11 / 0.14-0.21, locked 0.10-0.12 / 0.12-0.19, cornering 0.12 / 0.16-0.17; -1 to -14 °C: locked 0.10-0.14 / 0.12-0.19, cornering 0.21-0.25 / 0.21-0.26; skid trailer, ice above 30 °F locked 0.10 / 0.14, below 20 °F 0.21 / 0.29; studs under 0.6-0.9 mm protrusion act as none | Nordström & Samuelsson 1991 via WSDOT T2695 Table 4 (table, secondary); Burnett & Kearney, HRR 136 (1966) (table); VTI meddelande 923 (quoted) |
| Rally studs | FIA: ≤ 20 studs per dm of circumference, length ≤ 20 mm, ≤ 4 g, protrusion free (WRC: 5.5-7 mm, 384 per tyre, 145 mm tread); no μ or g. A WRC engineer: studded "more lateral grip and stopping power on ice than ... on gravel", traction lower; drivers: "similar or slightly more than a gravel tyre on gravel" | Rally Sweden 2025 SR Art. 7.5 (quoted); RallySport Magazine 2007 (quoted, secondary); Pirelli 2021 (snippet) |
| Combined slip on soft soil | α 6°: braking at 0.57 of the pure side force costs 16 % of it (light braking raises it), driving at 0.71 costs 24 %; α 25°, 35 % driving slip: Fy 0.67 of the lightly braked, Fx/Fz ≈ 0.27, \|F\| 0.87 of the pure lateral (inside a circle); max Fx > max Fy (an ellipse) | Janosi, Kamm & Wray 1981 (truck tyre, scarified sandy soil) and Krick 1973 (tractor tyre, sandy loam), both via El-Razaz PhD Leeds 1988 Figs 6.16, 6.40 (fig ±30 lbf / ±5 kp, secondary) |
| Driven tyre, traction against slip | dense sand: peak 0.63 at ~35 %, 0.87 of it at 80 %; shallow snow 0.40 at 15 %, 0.63 of it at 80 %; loose sand rising to 100 %; sand rig: pull peak at 15-25 % slip, 0.63-0.78 of it at 48-60 %; gravel: none | CRREL 95-1 Fig 18 (schematic, fig ±0.02); WES TR 3-666 Plate 5 (fig ±4 lb), §34 (quoted) |
| Driven tyre on gravel, traction against slip (2026-11-09) | dry 10-14 mm sub-round gravel bin, 37x12.5R16.5 off-road tyre, 10.2 kN, 0.05 m/s, driving slip (ωR - V)/ωR (the brush's theoretical slip): gross traction (T/R) 0.120 / 0.226 / 0.295 Fz at 10 / 20 / 30 %, still rising near-linearly (0.41 / 0.77 of the 30 % value), no peak in range; sinkage 61-83 mm (6-9× NGP's layers); sand 0.115 / 0.216 / 0.261; sand at 10 % and 7.7 / 10.2 / 12.7 kN 0.130 / 0.115 / 0.131 (no load trend); dry only, no full spin | Zeng, Xu & Zang, J. Terramech. 104 (2022) 49-58, Figs 8, 14 (fig ±50 N) |
| Driven tyre on sand, self-propelled | 205/60R16, 0.6-1.0 kN, 0.5 m/s, dry sand: one operating point per load, slip 47-52 %, gross thrust (motor torque) 0.041-0.047 Fz: with no drawbar the thrust is the motion resistance; no curve | Choi et al. (Pusan), preprint, Table 11 (table) |
| ATV tyre on gravel, lateral (towed, free rolling) | Dunlop KT 821, 5-10 psi, 50-320 lbf, ~10 mph: Fy = Fz A (1 - exp(-B α)), R² 0.72-0.97; A = B = c Fz^m, gravel 10 psi: 0.48 at 95 lbf, 0.24 at 320 lbf (α0 = 1/B 2.1-4.2°); rubber-on-gravel shear K 0.06 in (1.5 mm); longitudinal only as the free rolling drag against slip angle, no traction-slip | Krueger, Auburn MSc 2007, eq. 2.5, Table 3.6, Table 3.7 (fit; A = B by the author's fit form, weak) |
| ATV tyre on clay, traction and braking against slip | towed rig with a driven test wheel, remolded over-consolidated clay (φ 9°, c 9.8 kPa), 5 psi, 800 N added load: traction rises to a saturation near 0.3-0.5 slip (2nd-order fits; slip scatter ±0.1, no data past 0.65); braking (1000 N, 7 psi) peaks at 0.2-0.25 slip, 0.83 of it at 0.35 (fit); the free rolling wheel reads -50 lbf (net, axle force) | McIntyre, Auburn MSc 2015, Figs 63-65 (fig ±10 lbf; clay, not gravel) |
| Baja buggy (UNLV MIDAS): car, sensors, place (2026-11-09) | SAE Baja car, 10 hp Briggs & Stratton (README), CVT and a fixed-ratio transaxle (ratios not given), rear spool, knobby ATV tyres; no mass, weight split, CG, wheel or gear data anywhere in the repository (the MATLAB code only imports, cleans and plots; `KeyValues.txt` maps channel names). Overall ratio engine/axle 24 at 10 km/h to 8.2 at 46 km/h; the CVT holds the engine at ~2800 rpm while accelerating, 3600 governed at the top speed 46.6 km/h; r 0.278 m (axle rpm against GPS). Channels: `EngineRPM` and `AxleRpm` = README's two Hall sensors (engine, driven wheel: ≥ 2000 errors, 10 rpm steps, held between pulses, 0 and spikes of 570-1500 below ~150 rpm ≈ 15 km/h); `RPM` always 0 (unused); `Analog1` 3.0-3.7 V with no step at braking or throttle, drifting 3.16 → 3.42 V over the session: no pedal (pedal, governor and suspension pots and the temperatures are listed in the README, not logged); `AccelX/Y/Z` 25 Hz (+X forward, +Y with +yaw), `Yaw/Pitch/Roll` 25 Hz in °/s (+pitch nose-down), GPS 10 Hz with float32 lat/lon (0.4 m N, 0.7 m E). One spot by UNLV, Las Vegas (36.113 N, 115.146 W, ~620 m), 2017-07-25 19:57-20:43 PDT (UTC stamps; TopSpeed first, then Acceleration, then Braking); surface not named (the user: gravel) | UNLV SAE Baja, MIDAS repository, `sample-input-files` (MIT; README, code and logs read) |
| Baja buggy, hard stops (2026-11-09) | Six stops from 45 km/h: 0.86-0.92 g mean 0.9 → 0.1 v0 (0.4 s peak 0.90-1.02), onset to rest 1.6-1.7 s, by AccelX scaled (×0.87-1.00) so that it takes v0 to 0 where the deceleration ends; flat within ±0.06 g from 35 to 5 km/h (0.87-0.95 by 5 km/h bins), no rise or fall resolvable above ~5 km/h (25 Hz, 0.1-0.2 s stop transient). The rear axle stops 0.55-0.89 s in, at 20-30 km/h, and stays at 0 (the last ~0.7-0.9 s, ~50 % of the stop); deceleration before / after the rear lock 0.82-0.89 / 0.77-0.93 g: no change at lock (fronts not logged). Pitch by the gyro 3-14° nose-down; the scale falls as pitch rises (1.00 at 3°, 0.87-0.90 at 10-14°), so AccelX over-reads by g·sin θ. GPS runs 0.1-0.55 s late against the accelerometer (fit per stop; 0.1 s over the session) and its speed reaches 0 0.0-1.3 s after the positions stop: the stop distances from positions (15.8-20.8 m) exceed the accelerometer's (15.9-16.6 m) by -0.3 to 4.2 m, in the order of v0 × that lag. The first pass's 0.54-0.93 g from GPS speed was low for this reason | as above, `rc_Braking00/02/04/05.log` (inferred: our reduction, scripts outside the repository) |
| Baja buggy, braking slip of the rear (2026-11-09) | All logs, > 12 km/h, slip (ωr - v)/v of the rear against car deceleration (GPS 5-point, 0.1 g bins, n 1442 → 8): -0.025 / -0.045 / -0.07 / -0.10 / -0.18 / -0.22 / -0.25 / -0.30 at 0.05 / 0.15 / 0.25 / 0.35 / 0.45 / 0.55 / 0.65 / 0.75 g; at 0.8-1.0 g the rear is at 0 in 25-50 % of samples, -0.38 when rolling. Deceleration keeps rising with slip to lock: no fall past a peak. The brake split is unknown, so this is the car's deceleration, not the rear tyre's μ; GPS lag and the held axle value bias the slip by up to ~±0.05 at 0.5 g | as above, all 18 logs (inferred) |
| Baja buggy, traction (2026-11-09) | Above 10 km/h the car is power-limited, not grip-limited: a·v constant 1.06-1.28 g·m/s from 10 to 35 km/h with the engine held at ~2800 rpm; driving slip (ωr - v)/ωr 0.16 / 0.12 / 0.09 / 0.06 / 0.04 at 0.30 / 0.26 / 0.21 / 0.15 / 0.12 g and 10-15 / 15-20 / 20-25 / 25-30 / 30-35 km/h (~0.3-0.5 slip per g, linear). Launch (11 runs): 0.35-0.47 g peak (0.60 once) over 0.2 s, below 13 km/h, engine 1800-2400 (clutch engaging); the axle sensor gives no usable slip below ~12 km/h, so whether the launch is grip- or clutch-limited cannot be told, and no traction peak is in the data. Replaces the first pass's "0.35-0.38 g at slip 0.8-0.9": those were launch samples with axle spikes | as above, `rc_Acceleration*.log` (inferred) |
| Baja buggy, cornering (2026-11-09) | Lateral by v·r (GPS speed × yaw gyro): largest steady 1 s (yaw sd < 3 °/s) 0.68 g at 36 km/h on a 15 m radius, 0.57 g at 22 km/h on 6.6 m, 0.55 g at 17 km/h on 4.4 m; AccelY reads 1.11-1.15 × v·r at every level 0.1-0.7 g (roll, ~7°/g if all of it). No steering angle (README: not installed), so whether 0.68 g is the limit cannot be told | as above, mostly `rc_TopSpeed00.log` (inferred) |
| Baja buggy, coasting (2026-11-09) | Engine near idle, v > 3 m/s, straight, ≥ 1 s: lowest decelerations 0.034 / 0.064 g at 19 km/h on opposite headings (mean 0.049), 0.05-0.10 g at 16-23 km/h otherwise. The engine stays coupled (engine/axle 8-10, so engine braking), aero ~0.006 g, light brake not excluded (no pedal channel): an upper bound 0.03-0.07 for the rolling resistance, consistent with NGP's gravel 0.020-0.028 Fz; no a + b v² (coasts only at 16-23 km/h, top speed 46 km/h). `rc_TopSpeed00Complete.txt` is the same log densified | as above (inferred) |

No open WRC telemetry for Rally Sweden or the Arctic (RallyDataJunkie's scraper broke before Sweden 2022); the
WRC+ Finland 2021 files' first samples are time-compressed (0-60 km/h in 0.5 s), so they give no launch.

Game (Fiesta Rally2 4WD and Escort Mk II Grp4 RWD, ngp / experimental-brush; decel on the flat, `decel_flat`;
gravel RSF 345, snow map 10 for braking and RSF 345 as material 101 with `--tyre 6` for the circle, ice RSF 568 or map
85 as material 116 (the former `--material-as`; on the pads since, [Scorecard](#scorecard)); launch: `--flat-out` from
the boot, 0-40 km/h; scorecard rows as in [Scorecard](#scorecard)):

| Surface | Quantity | ngp | experimental-brush | Band | Moves it |
| --- | --- | --- | --- | --- | --- |
| gravel | tyre peak μ_y | 0.828 | 0.828 | ≲ 0.67 (CW, one rally tyre): MISS both | level |
| gravel | circle, Rally2 / Escort | 0.571 / ~0.48 | 0.664 / ~0.54 | 0.43-0.7 road and CW: PASS; WRC telemetry 1 s p90 0.74-0.89 above both | level and shape (Escort: no latched hold, a v²/R read, weak) |
| gravel | locked 50 / 80 / 100 km/h, Rally2 | 0.69 / 0.75 / 0.78 | 0.93 / 0.95 / 0.99 | road 0.47-0.66 (lower bound); WRC 0.8-1.0 (inferred), telemetry 0.5 s peak 0.83-1.06: brush PASS, ngp low | level and shape (berm) |
| gravel | locked 50 / 80, Escort | 0.67 / 0.76 | 0.82 / 0.85 | as above | as above |
| gravel | locked over ABS K 0.15, Rally2 50 / 80; Escort | 1.10 / 1.08; 1.01 / 1.07 | 1.53 / 1.34; 1.25 / 1.20 | 1.12-1.59 (+ 1.18-1.40, 1.25 new): brush PASS, ngp MISS | shape |
| gravel | traction 0-40 km/h, Rally2 / Escort | 0.73 / 0.39 g | 0.90 / 0.51 g | none for a car; gravel over tarmac 0.66 / 0.62 ngp, 0.86 / 0.88 brush | shape: loose-v1 holds the peak at full spin, NGP falls to 0.82 |
| snow | tyre peak μ_y / μ_x | 0.85 / 0.87 | 0.84 / 0.86 | road 0.28-0.57: MISS; studded rally ≈ gravel (quoted): about NGP's | level |
| snow | circle, Rally2 | 0.48 | 0.55 | road 0.25-0.40: above | level |
| snow | locked 50 / 80, Rally2; Escort | 0.79 / 0.78; 0.73 / 0.77 | 1.07 / 1.08; 0.95 / 0.94 | road 0.31-0.35 (lower bound) | level and shape (berm) |
| snow | locked over ABS, Rally2 50 / 80; Escort | 0.98 / 1.06; 1.05 / 1.00 | 1.17 / 1.27; 1.42 / 1.13 | road winter 0.8-0.95 (locked 0.31-0.35 over ABS 0.33-0.45, inferred); studded "small" difference; chains 1.47: brush above | shape (berm on snow, ours) |
| snow | traction 0-40, Rally2 / Escort | 0.74 / 0.42 | 0.88 / 0.49 | road FWD 0.23; studded peak 0.57-0.60, spinning 0.28-0.43 | level and shape |
| ice | tyre peak μ_y (tarmac tyre, unstudded) | 0.094 | 0.094 | -3 °C 0.15-0.18: MISS; 0 °C smooth ice unstudded cornering 0.12, optimum 0.09-0.11: about NGP's | level (the temperature) |
| ice | car best / locked 50 km/h, Rally2 | 0.096 / 0.095 | 0.100 / 0.081 | studless car 0.18-0.22 (-2 to -13 °C); locked 0.10-0.12 at 0 °C | level |
| ice | launch 0-20 km/h, Rally2; Escort | 0.08 g; does not move | 0.06 g; does not move | | NGP's rest clear and free-wheel latch drag (below) |

- Gravel: no case to lower the level. The one rally-tyre level (CW ≲ 0.67) sits under the WRC telemetry's lateral
  and braking, and the published car braking is road cars on road tyres. The berm is in band against four
  programmes now (1.18-1.40, 1.25, 1.25-1.30, 1.12-1.59).
- Snow: NGP gives snow the gravel level, and the only rally-stud statements put a studded tyre on snow and ice at or
  above a gravel tyre on gravel. The road-tyre bands are lower bounds. The berm on snow is ours and not supported:
  winter and studded tyres peak at 40-60 % slip with locked about equal, so loose-v1's snow locked over ABS of
  1.13-1.42 sits above the road data (≤ 1). Locked, snow out-brakes gravel by 14 % under both physics (map 10 against 345's deeper soil).
- Ice: material 116 runs the tarmac tyre (RSF 565/614, unstudded). Its 0.094 is the 0 °C smooth-ice value
  (Nordström, unstudded); the -3 °C band is a colder ice. The RWD Escort on 568 iced does not leave the line under
  either physics: rears spinning at 20-49 m/s in gears 2-6 for 13 s at 0.000 m/s, the 4WD Fiesta does. Cause
  (2026-11-09, env-gated probe builds, not committed): NGP's own, shared by both physics, not the tyre or tooling.
  (1) Rest clear: the integrator zeroes a flagged body's velocity under |v|² 4e-7 (`rigid_body::integrate`
  `CLEAR_SPEED_SQ`), so from rest at 720 Hz a net push under 0.455 m/s² (0.046 g) never starts the car. The
  Escort's rears give 2 × 265 N (NGP: static μ at full spin, the sticking weight follows the contact speed), less
  568's start grade (241 N): 0.25 m/s². With the free-wheel drag off, μ × 1.2 (0.33 m/s²) stays, μ × 1.5 (0.47)
  moves, and with the clear off too μ × 1.2 moves; the Fiesta's ~0.7 clears it. (2) Free-wheel drag: under 2 m/s each wheel's latch bound is the
  sticking load 0.025 Fz N·m (`tyre::step` `OUTPUT_SCALE`, `free_wheel_step`), so a free front drags 0.025 Fz / R
  ≈ 0.086 Fz, the ice μ itself (247 N at Fz 2891 N, R 0.29, 0.9 m/s; 16 N at 4 m/s): iced from 1 m/s the Escort
  slows at 0.017 g to a stop, from 4 m/s it gains 0.027 g (brush 0.017). Both off: 0.025 g from rest. Tooling
  checked: 116's values copy (μ 0.10/0.095, rolling 0.02, class 5; its collision values change nothing), throttle
  0.3 stays put too. BMW M3 E30 frozen, both Porsche 911 SC creep 0.02 m/s and stop. A real RWD car would crawl
  (free-rolling drag ~0.013-0.0145 Fz at walking pace: radial car tyres, smooth road, Wong Fig. 1.3, eq. 1.1, [Wong](#wong-theory-of-ground-vehicles-against-the-open-items-2026-11-09)). Either change is the integrator's or the drive line's,
  not the brush's: open as a decision.
- Combined slip: the brush already loses more side force driving than braking at the same slip (α 6°, κ ±0.05: 0.98
  / 1.03; NGP symmetric 0.96). Its |F| stays on the circle (α 25°, κ 0.3: 1.02 of the pure lateral, NGP 1.02), where
  soft soil falls inside it (0.87). Soil is not gravel, so no change.

Proposal (no implementation; evidence first; 1 done 2026-11-08, `SNOW_PLOUGH` 1.05, [Loose surfaces:
loose-v1](#loose-surfaces-loose-v1); 2 measured in a drive-test build, below):

1. Snow shape (brush only, class 3): a berm of 1.0-1.1 in place of 1.3. Evidence: SR 115 (peak at 40-60 %, locked
   about equal), road winter locked/ABS ≤ 1. Expected (the PLOUGH 1.0 row of the loose-v1 trade-off): snow full
   pedal 1.06 → ~0.92 g, locked/ABS ~1.0, gravel unchanged. Driver test first: a snow hairpin braked locked against
   threshold.
2. Gravel shape (brush only, weak): a driven fall past the peak to ~0.85 of it at full spin (dense sand 0.87 at 80 %,
   sand rig 0.63-0.78 at 48-60 %, none on gravel). Expected (estimated, × 0.85-0.9 of the spinning wheels' share): the Rally2's 0-40 km/h 0.90 → ~0.80 g, Escort
   0.51 → ~0.46. Driver test: power slides out of slow gravel corners and a standing start.
   Drive-test build (2026-11-08, branch `tyre/loose-shape`'s WIP commit, not merged; `RERBR_TEST_GRAVEL_SPIN=F`):
   on gravel's share the driven force falls by a smoothstep from a driving slip of 0.3 (dense sand's ~35 % peak; the
   shear law at 0.97 of its level) to F at full spin (0.88 of the way at 80 %: dense sand 0.87 there at F 0.85).
   F 0.85: Rally2 0-40 km/h 0.89 → 0.83 g (0-20 0.92 → 0.81), Escort 0.51 → 0.45 g (ngp 0.73 / 0.40); circle 0.664
   → 0.669 g; scorecard `gravel-traction` 0.842 → 0.829, locked over ABS 50 / 80 1.529 / 1.340 → 1.556 / 1.351 (the
   approach runs differ), all PASS; RSF 111 287.08 → 287.16 s.
   Papers (2026-11-09, table above): the one gravel traction-slip curve (Zeng 2022) stops at 30 % slip, still rising,
   on a 6-9 cm deep bed at 0.05 m/s; no open curve on gravel reaches full spin, and none is wet or damp. It supports
   only "no fall before 30 %": the WIP's (`tyre/loose-shape`, SPIN_FROM 0.3, smoothstep to F 0.85) driven force is
   1.000 / 0.998 / 0.992 / 0.97 / 0.88 / 0.85 of the level at 0.3 / 0.35 / 0.4 / 0.5 / 0.8 / 1, so its effective peak
   already lies at 0.35-0.4 (dense sand ~35 %, clay 0.3-0.5). No evidence to change SPIN_FROM, F or the smoothstep;
   F stays sand-based (0.63-0.87 of the peak at 48-100 %). The rise does not test the shear law: Zeng's deep-bed rise
   (0.41 / 0.77 of the 30 % value at 10 / 20 %) is 5-10 times slower than the law's (s0x 0.081: 0.71 / 0.92 / 0.98 of
   the level at 0.1 / 0.2 / 0.3), a bulldozing bed (sinkage 61-83 mm) and not a road layer; Krueger's gravel takes
   the law's exponential form laterally (R² 0.72-0.97) with α0 2-4° on a soft ATV tyre. Which curve to compare: the
   brush's Fx is the patch force at the axle, NGP's `RollingResistance` a separate wheel torque (`tyre::forces::
   rolling_torque`, 0.018-0.028 Fz on gravel), so Fx is the net (drawbar) traction and Fx + RR Fz the gross (T/R)
   one; Zeng's and Choi's curves are gross, McIntyre's net.
3. Level: none recommended. Gravel and snow stay NGP's (the rally data do not sit below them). Ice 116 at 0.094 fits
   0 °C ice for an unstudded tyre. A colder ice level, or studded rally tyres on 116, would be a level decision.
4. Scorecard (done 2026-11-09, [Scorecard](#scorecard)): the snow level rows from road, winter and AT tyres are lower
   bounds (`snow-peak-mu-y` 0.35, packed; `snow-peak-mu-x` 0.46) or context where they repeat a scored quantity
   (`snow-lat-max-packed`, `snow-long-mu-winter`); new `snow-car-locked-over-abs-50/80` 1.00-1.10 (studded, SR 115
   Fig. 3, 1.02-1.08 ± 0.02; road winter ≤ 1.0 below); gravel's 1.18-1.40 (SATC) is context
   (`gravel-car-locked-over-abs-satc-80`): it lies inside the scored 1.12-1.59 on the same run.

## Implied grip from real stage times

Research (2026-11-09; downloads outside the repository): what grip real Rally2 cars use per surface, from official
times on the official routes, put in the same terms as the game's car-level g. `harness/implied_grip.py`
(definitions in its docstring), per stage `data/implied-grip-stages.csv`, calibration `data/implied-grip-calibration.csv`.

- Data: WRC live timing (stage, split times and split distances, classes RC2/Rally2 and RC1/Rally1, 2018-2026; the
  Red Bull host `p-p.redbull.com/rb-wrccom-lintegration-yv-prod/api/`, api.wrc.com no longer resolves) and
  RallyDataJunkie's routes (`geodir/<event>_<year>.json`, 2022-2026, no elevation; Finland 2021 and Monte-Carlo 2018
  with their WRC+ telemetry). Times: Completed, not assessed. 879 stages with a route: 553 usable (length within 1 %,
  ≥ 3 RC2 times), 122 at 1-2 % (left out of the figures), 173 longer, 31 without RC2 times; no route coarse (median
  vertex spacing 8-34 m). Unusable for length: Monte-Carlo 2023/2024/2026 (+2-8 %), Central European 2023/2024
  (+2-5 %; 2025 one stage), Japan 2022/2025 (+2-12 %), Croatia 2022/2024 (+2-17 %, 2024 SS1/8 drawn as "Part 1"
  only), Kenya 2022/2023, New Zealand 2022, Greece 2023 (three routes 1.7-2.2×). No route: Monte-Carlo and Sweden
  2022, every 2026 event but Monte-Carlo and Sweden, Croatia 2025. Road conditions (wet, snow cover, road position)
  are not in the timing data: not reported.
- Model: point mass on the route's curvature (σ 15 m), corner speed √(μ_y g_eff/κ), traction/power forward and
  braking backward passes, friction ellipse, μ_x = μ_y; Rally2 1430 kg, 170 kW at the wheels (214 kW × 0.85 × 0.92
  over the shift band), CdA 0.85, ClA 0.2, 190 km/h; WRC 2017-21 1360 kg, 218 kW, CdA 0.95, ClA 1.0. Implied μ: the
  μ_y that gives the class's best and top-5 median time. Car parameters move it under 1 % each (m 1390-1480 kg,
  P 150-195 kW, CdA 0.7-1.0, ClA 0-0.5, rolling 0.6-1.5×); μ_x/μ_y 0.8-1.2 ±3 %; per stage `mu_rss` median 0.035.
  The level depends on the smoothing (SS7 1.40 / 0.89 / 0.64 at σ 5 / 15 / 30 m) and the ellipse (off: -12 %); the
  calibration uses the same choices, so they cancel. σ 15: SS7's μ within 1.5 % with every 2nd-4th vertex dropped
  (spacing 20 → 103 m), σ 30 falls 12 %. Grade (telemetry altitude): +0.2 % (Finland) / +0.3 % (Monte, climbs to 940
  m), so the routes' missing elevation costs under 1.5 %. A line inside ±2-4 m of the route lowers μ 11-19 % with no
  better fit: not used; the calibration takes the line in.
- Calibration (WRC 2017-21 cars, 5-8 merged 10 Hz drives a stage; `rally_gg.py` definitions; stages over 5 km):
  Finland 2021 (17 stages, gravel): route μ / lateral 1 s p90 1.02 (0.94-1.11), / g-g p95 |lat| 0.83 (0.73-0.90),
  / braking 0.5 s peak 0.92; Monte-Carlo 2018 (15 stages, tarmac with snow and ice; SS9 lat p90 0.41 g): 0.86
  (0.73-0.95), 0.74 (0.60-0.86), 0.81. On the drivers' own GPS line both agree: μ / p90 0.847 and 0.843, / g-g p95
  0.69 and 0.71. The route-based difference is the line: GPS μ / route μ 0.84 in Finland (wide roads, cut corners),
  0.97 at Monte (narrow). So the model maps time to measured g alike on both surfaces, and the transfer to another
  event lies between the two (wide roads: Finland's 0.83, narrow: Monte's 0.74 per g-g p95). The model's speed
  against the telemetry by position: r 0.75 (Finland) / 0.84 (Monte), SS7 p10/50/90 93/133/171 against 91/134/171
  km/h. Rally2 on the same stages: μ 0.93 of the WRC cars' in both rallies (Finland 2021 RC2 top-5 0.84).
- Game in the same terms: on SS7's part that is RSF 524 (10.31 km to the finish), the at-limit autopilot's times
  ([Real stage telemetry](#real-stage-telemetry-wrc-gravel)) give μ 0.46 (ngp Rally2, plan 0.571 g: 0.80 of it) /
  0.47 (brush, plan 0.598 g: 0.79), the real drivers 0.98-1.09. Model μ over the car's limit is 0.79-0.80 in the game
  and 0.83 (route μ / g-g p95) for the real cars: the g-g p95 stands for the car's lateral limit, and the game's
  circle row is that limit.

| Surface | Events (stages) | Rally2 top-5 μ, event medians | RC1 μ | Car-level lateral, μ / 0.83 (/ 0.74) | Game Rally2: circle / threshold braking, ngp; brush |
| --- | --- | --- | --- | --- | --- |
| gravel, Finland 2022-2025 | 4 (68) | 0.838 / 0.850 / 0.833 / 0.889 | 0.89-0.92 | 1.02 g (0.93-1.15 over the stages' ratios; 1.13) | 0.571 / 0.826 (80 km/h); 0.664 / 0.840 |
| gravel, all | 32 (388) | 0.721 (Kenya 0.42-0.48, Sardegna 0.57-0.61, Greece 0.66-0.74, Portugal 0.72-0.74, Estonia 0.68-0.75, Chile 0.76-0.79, Latvia 0.79) | 0.47-0.92 | 0.87 g | as above |
| snow, Sweden 2023-2026 | 4 (64) | 0.704 / 0.627 / 0.620 / 0.605 | 0.67-0.74 | 0.75 g (0.73-0.85 over the years, 0.69-0.85 over the ratios; 0.84) | 0.48 / 0.875 (50 km/h); 0.55 / 0.922 |
| tarmac | 11 (87) | 0.880 (Croatia 0.74-0.88, Japan 0.79-0.91, Belgium 0.81, CER 2025 0.98, Spain 1.09, Canarias 1.16) | 0.79-1.19 | narrow (Croatia, Japan, Belgium) / 0.74: 1.0-1.2 g; wide (Spain, Canarias) / 0.83: 1.3-1.4 g | 1.19-1.28 (r40, map 85, every physics) / 1.31 (100 km/h); 1.27 |
| mixed, Monte-Carlo 2025 | 1 (14) | 0.748 (0.49-0.83) | 0.81 | 1.01 g (/ 0.74) | |

Per stage (CSV): best and top-5 times, μ for both, `split_mu` (sections between splits: p10-p90 within an event
13-40 % either side of its median, so the sections mix road character with grip and do not separate them), the sensitivity
moves, the direction, the RC1 μ beside.

- Answer (Rally2, car level, uncertainty from the calibration's transfer ±10 % and the stages' spread): gravel on
  Finnish roads about 1.0 g (0.93-1.15), snow (Sweden) about 0.75 g (0.69-0.85), tarmac 1.0-1.4 g by road. The game
  side of this table (gravel and snow circles 0.571 / 0.664 and 0.48 / 0.55) is RSF 345's sloped circle, terrain and
  not grip: in the same terms on a flat circle the game is 0.86-0.90 of the real on gravel, above it on snow, in
  range on tarmac ([Car-level grip in the same terms](#car-level-grip-in-the-same-terms)). Real ratios inside one
  calibration: snow over gravel 0.74 (0.68-0.85 by year), tarmac over gravel 1.0-1.4. Braking through the 0.5 s peak ratio (0.92 / 0.81): gravel 0.92 g, snow 0.68 g, tarmac 1.09 g real, against the
  game's threshold rows above (the model's μ_x = μ_y makes this weaker than the lateral).
- What moves it most: the line (road width): Finland's against Monte's ratio is 10 %, the stages' ratios ±10 %;
  then μ_x/μ_y ±3 %; the car's parameters under 1 % each. Not covered: the real lateral g includes road camber and
  ruts the cars lean on (the game's flat circle has none; RSF 345's lies on a slope, [Car-level grip in the same
  terms](#car-level-grip-in-the-same-terms)); one calibration rally per surface, both with WRC cars, not
  Rally2; no snow telemetry (Sweden takes Finland's ratio, Monte's gives +11 %).
- Level: no recommendation; the gaps in the same terms are in [Car-level grip in the same
  terms](#car-level-grip-in-the-same-terms). The level is the user's decision.

## Car-level grip in the same terms

Research (2026-11-09, main 9158d147): the game's Fiesta Rally2 against real Rally2 in accelerometer terms. Tools:
`harness/car_grip.py` (circles: kinematic v²/R, `glat` over the same 1 s, path g, sideslip), `limit_pace.py --surface`,
`rally_gg.py stats --speed-lag`, the brush's grip × F (now `--tyre-tuning SURFACE=F`, "Experimental tuning"; at 1,
traces byte-identical). Runs and scripts outside the repository. ngp / experimental-brush.

- RSF 345's r40 circle (the old gravel row, the snow circle under 101) lies on an 11.9° (21 %) slope: z 9.6-24.5 m
  over the lap (plane fit, rms 0.5 m); the hold covers 30-180° of the lap and ends running downhill at ~13.5 m/s
  whatever the surface or level (material 17: 0.47 / 0.55 g; brush grip × 0.8-1.2: no latch, 0.69, 0.66, 0.56, 0.53;
  tyre 3 in place of 345's damp tyre 4: 0.57 / 0.57). Its 0.571 / 0.664 g is terrain and controller, not grip. Flat
  (`limit_pace` `gravel-flat`, `snow`, `tarmac`): now the pads; the figures here are map 85's (z 3.4 m all round) as
  gravel 17 with the dry gravel tyre, snow 101 with the snow tyre and tarmac 37, on the tarmac set-up; 345's damp tyre 4
  there reads 3 % under tyre 3 (0.936 / 0.935).
- Flat r40 circles, Fiesta Rally2, kinematic / glat / glat ÷ kinematic: gravel 0.971 / 0.920 / 0.95 and 0.955 / 0.877
  / 0.92; snow 0.952 / 0.893 / 0.94 and 0.982 / 0.900 / 0.92; tarmac 1.282 / 1.181 / 0.92 and 1.327 / 1.186 / 0.89
  (before T55's limit hold; after it 1.334 / 1.254 and 1.333 / 1.234: "Why the flat r40 tarmac circle
  lost it"). glat ≈ the path's own v²/r (0.98-1.01): the row overstates an accelerometer by 5-11 % because the car
  runs 1-3 m wide of the commanded radius; roll and sideslip (β 7-9° ngp, 11-14° brush) cancel. r25 / r40 and ramps 0.2-0.8
  move it ±3 % (gravel kinematic 0.95-1.00, glat 0.86-0.92; tarmac 1.22-1.37 / 1.14-1.21); r60 does not fit the pad.
- At the flat gravel limit all four tyres run at 0.93-1.02 of NGP's own peak at their load (`limit_frames`): ngp
  fronts 1.01-1.02, rears 0.96-1.02, Σ|F|/ΣFz 0.872 against Σμ(Fz)Fz/ΣFz 0.873; brush fronts 0.99-1.02, rears
  0.93-0.95 (0.844: 3 % left at the rear). The earlier "front-limited, 0.9 of the peak" was the slope. A test holding
  a rear slip angle is not needed: the steady circle already saturates the car.
- At-limit runs (plan = factor × the RSF 345 limits; factor `fLAT bBRAKE`; clean = finished, no recovery, off < 3
  m): 524 gravel ngp f1.3 b0.8 (7.28 / 6.48 m/s²) 296.3 s, faster than f1.0's 314.9 by trading braking for lateral
  (f1.5 b0.8 287.1 s at 4.0 m off); brush f0.9 310.4 s, none faster clean (f1.1-1.2 b0.8 299-300 s at 3.9-4.3 m off).
  562 tarmac (tarmac limits 1.28 / 1.33 g circle, 1.30 / 1.32 g braking) ngp and brush f0.75 b0.6 719.9 / 705.9 s
  (f0.8 up: recoveries or rolls). 422 snow f1.0-1.6 b0.8 all clean and still faster at the top (ngp 621 → 564 s,
  brush 581 → 508 s) with lateral p90 0.28 / 0.39 g: the autopilot's other guards set the pace there, not the plan
  (not diagnosed; 531 not run).
- At-limit statistics (`rally_gg.py stats`; Finland 2021 SS7 telemetry medians in brackets): 524 braking 16 longest
  0.58 / 0.58 g (0.60), peak 0.5 s 0.71 / 0.76 (1.00), lateral 1 s p50 / p90 / p99 0.34 / 0.57 / 0.69 and 0.31 /
  0.52 / 0.68 (0.40 / 0.89 / 1.25), g-g p95 |lat| 60-90 / 90-120 / 120-150 km/h 0.70 / 0.56 / 0.45 and 0.64 / 0.49 /
  0.39 (1.04 / 1.07 / 1.14). 562: braking 0.80 / 0.82, peak 1.04 / 1.08, lateral 0.37 / 0.87 / 1.09 both, g-g |lat|
  30-60 / 60-90 / 90-120 1.10-1.14 / 0.93-0.99 / 0.74-0.76. 422: braking 0.61 / 0.63, lateral p90 0.28 / 0.39.
- On 524's hardest moments (top 5 % of 0.5 s |glat|, 0.57-0.61 g) the tyres work at 0.74-0.80 of their peak (a wheel
  ≥ 0.95 in 21-53 % of them), 0.55-0.58 at the median: the stage pace leaves ~25 % at the corners and the whole step
  from 0.57 to the circle's 0.92 g, the driver's (line, plan, steering), not the tyre's.
- The speed channel's bias: a first-order lag on the game's speed lowers the 16 longest events' decel by 17 / 31 %
  (gravel, τ 0.3 / 0.5 s), 10 / 21 % (tarmac), 13 / 26 % (snow); the 0.5 s peak (accelerometer) is unchanged. The
  WRC+ speed's own lag is unknown (it integrates to the GPS path; `accy` trails it by 0.5 s), so the real 0.60 g is a
  lower bound by up to that much.
- Times in implied μ (`implied_grip.py` on the same routes, Rally2 car): 524 = SS7 from 10.31 km: Evans 206.7 s μ
  1.089 (WRC), a Rally2 at × 0.93 μ 1.013 = 220.9 s; game (race time less 5 s for the spur) ngp 291.3 s μ 0.52,
  brush 305.4 s 0.47, default autopilot 0.36-0.37. 562 = Canarias SS14 (as drawn, direction unchecked): Rally2 543.1 /
  547.6 / 548.0 s μ 1.11 / 1.09 / 1.09; game 719.9 / 705.9 s μ 0.58 / 0.61. Snow: no route for Sweden 2018 or the
  Arctic 2021. Stage μ over the flat circle's glat: game 0.49-0.57 against the real 0.83 (Finland) / 0.74 (Monte).
- Class (flat circles, kinematic g, ngp / brush): gravel Fiesta Rally2 0.971 / 0.955, GR Yaris Rally2 0.973 / 0.978,
  Fiesta R5 0.958 / 0.943, Polo GTI R5 0.957 / 0.940, Fiesta WRC 2019 0.990 / 0.978, i20 Coupe WRC 2020 0.991 / 0.978;
  tarmac 1.24-1.28 / 1.29-1.33 Rally2/R5, 1.27 / 1.31-1.33 WRC; snow within 3 % of gravel. Rally2 over WRC 0.97-0.98
  (ngp) and 0.96-1.00 (brush) on gravel, 0.97-1.01 on tarmac, against the real 0.93. The wide NGP spread (Fiesta WRC
  0.749, i20 0.587) and the brush's Fiesta WRC 0.423 (a 3.3 s hold) are RSF 345's slope (hold lengths 3-19 s); flat,
  the six cars lie within 5 %, so no tyre, setup or aero value is needed to explain it; at the circle's 70 km/h the
  WRC cars' extra downforce over the Rally2's is ~1 % of the weight and their power does not act. Fix: the scored row
  reads the flat circle (done).
- Aero (2026-11-09): NGP's downforce (`rbr_physics::ngp::downforce`) is the car's `FrontDownforce_NGP` /
  `RearDownforce_NGP` at 150 km/h on the square of the speed along the car's axis (+0x15f8), so cos²β with sideslip
  (-7 % at 15°), the split fixed; the drag (`car_body::drag`) is Cd·A on the velocity, yaw-free. Fiesta Rally2 (RSF
  `common.lsp`): 1395 + 1705 N (ClA ≈ 3.0 m², 45 % front), Cd·A 0.78 m², 1431 kg: 22 % of the weight at 150 km/h,
  4.8 % at the gravel circle's 70 km/h and 6.4 % at tarmac's 81 (Fiesta WRC 2019: 1800 + 1900 N, 27 % at 150). Two
  hand drives of 524 (`replay_gg`'s traces, 1 s means) carry ΣFz +2.4 / +3.8 kN from 12-20 to 32-40 m/s (the
  downforce alone +1.9; the rest the road's compressions).
  So ~4 % of the gravel circle's g and ~5 % of tarmac's is downforce (estimated: the load × car_pace's `AERO_GRIP`
  0.86); the real Rally2 figures carry little at those speeds, and `implied_grip.py`'s Rally2 (ClA 0.2, swept 0-0.5)
  read on game times credits the game's downforce to its μ. No published Rally2 downforce found. Dominy & Richardson
  (SAE 2004-01-3508, Durham; 20 % generic 2004 WRC model, moving ground, rear wing): datum ClA 0.75 m², Cd·A 0.85 m²
  (Table 2: −2050 / 2333 N at 67 m/s, 1300 kg), a quarter of NGP's Rally2 downforce. With yaw (Fig. 10, 14, read ±10
  %, moving ground, clearances 8-14 mm model scale): at 15° drag +35-60 %, total downforce −20-40 %, the front's
  −80-100 % (at 14 mm gone), the rear's about unchanged: the centre of pressure moves back; a fixed ground shows much
  less. Their lap simulation (Table 3's 2.08 km stage, 78.2 s loose, loose μ = half of tarmac's by assumption): zero-yaw
  aero makes the time 0.2 s fast at 9° body slip, up to 0.4 s (tarmac) / 0.3 s (loose) at 15° (Fig. 18), steady state
  only. Not in ReRBR (any physics): a sliding car keeps its front downforce and its drag.
- Aero with sideslip, a prototype (2026-11-10, `--aero yaw` with an experimental profile, off by default;
  `rbr_hybrid::ngp::aero_model` on NGP's downforce update): my read of D&R's Figs 10 and 14 (model N at 0 / 15°, 8 /
  11 / 14 mm): front lift -1.30 / -0.25, -1.05 / -0.15, -0.85 / +0.05, about linear; rear -1.4, -1.7, -2.0 to -1.8;
  drag 2.85, 3.45, 3.8 to 4.4-4.6 (+54, +30, +21 %). So the front downforce × max(0, 1 - β/17°), the rear NGP's, the
  drag × (1 + 0.35 β/15°) held past 15° (no data there; a guess). `handling.py`, experimental-brush, 140 km/h,
  without / with: gravel pad ramp steer peak g Fiesta Rally2 1.126 / 1.073, Fiesta WRC 2019 1.145 / 1.155 (sideslip
  at the peak 20 / 39°), sine dwell g -2 to -10 % from 0.3 of lock up, lowest spinning amplitude WRC 0.75 / 1.0,
  Rally2 0.55 / 0.55; tarmac ramp 1.461 / 1.437 and 1.424 / 1.406 (sideslip 3.5°), lowest spinning amplitude WRC
  0.4 / 0.3, Rally2 0.3 / 0.3; at 110 km/h on gravel within 3 %. Not decided: one model test of a 2004 car at tarmac
  clearances, on files with four times its downforce.
- The same pads without it (experimental-brush): ramp steer peak g at 140 km/h over the r40 circle's (70 / 82
  km/h): gravel 1.145 / 0.974 (WRC), 1.126 / 0.967 (Rally2) = 1.17; tarmac 1.46 / 1.33, 1.42 / 1.34 = 1.06-1.10.
  The real telemetry's g-g p95 |lat| at 120-150 over 30-60 km/h (`data/rally-telemetry-gravel.csv`, medians):
  Finland 2021 1.24 (0.96-1.43 over 8 drives), over 60-90 km/h 1.15; Wales 2017 1.01 and 0.93. A bin's p95 is a
  lower bound of the limit and Finland's fast corners are cambered, so the files' downforce is not contradicted;
  D&R's level (a quarter) would give +5 %. A CFD of the 2008 Impreza S14 WRC (DynaRace blog, k-ε, no cooling flow;
  weak) gives 1948 N at 160 km/h, ~1.7 kN at 150: the files' 2006 WRC cars carry 1.61-1.95 kN.
- The devices' drag, a prototype (2026-11-10, `--aero wings` or `wings+yaw` with an experimental profile, off by
  default; `aero_model::wing_drag_factor`): the drag area gains 0.074 × (the file's downforce area)², the form of
  induced drag, set on the telemetry's 1.74 m² for the 2017-21 cars ("Full throttle on straights": file 0.81 m²,
  downforce area 3.54 m²). The rule on the other files: WRC 2006 0.82 and 1.08 m² (D&R's 2004 model 0.85), Fiesta
  Rally2 1.30, Polo R5 1.04, S2000 0.90, Group N +0.03, Fiesta RS WRC 2014 1.22 (Deutschland 2016's weak fit 0.83,
  0.63-1.06: over), nothing without downforce. i20 Coupe WRC 2020, experimental, without / with (real): pad-tarmac 4th
  110-120 km/h 0.492 / 0.461 (0.384), 6th 150-160 0.342 / 0.270 (0.214), 170-180 0.267 / 0.173 (0.143), 180-190
  0.231 / 0.128 (0.072); pad-gravel 6th 150-160 0.323 / 0.247 (0.182), 170-180 0.244 / 0.150 (0.108); `rally_wot.py
  fit` reads 1.82 m² on tarmac, 1.64 on gravel; what is left is the experimental engine's 10 %. Top speed without /
  with, tarmac and gravel: i20 202.2 / 199.4 and 200.6 / 196.8 km/h (real 196-202), Fiesta Rally2 177.5 / 175.8 and
  192.7 / 189.6, Polo R5 182.6 / 181.6 and 181.6 / 180.2. RSF 524 with the autopilot (80 km/h mean): i20 344.9 /
  346.6 s, Rally2 347.2 / 348.4. Not decided: one class measured, Rally2 and R5 by the rule alone; damaged aero
  parts keep this drag.
- Braking between cars (2026-11-10; `brake_test.py`, pad-tarmac, 100-0 km/h declutched, mean g, best threshold / ABS
  at κ 0.15): Alpine A110 Rally (500 N at 150 km/h) ngp 1.20 / 1.23 (32.7 / 31.9 m), brush 1.21 / 1.25; Fiesta Rally2
  (3100 N) ngp 1.32 / 1.32 (30.0 / 29.8 m), brush 1.31 / 1.33. NGP's tyre tables are the same for every car and the
  file's tyre width does nothing (Alpine fronts 205 to 235 mm: no change); at 32 km/h in the ABS runs both cars hold
  1.26-1.27 g, so the 7-9 % between them is the file's downforce. With `--tyre-tuning tarmac=1.1` the Fiesta's brush
  gives 1.42 / 1.46 g and 1.45 g on the r40 circle (1.31 at 1.0).
- Steady drift in a model, no measurement (Chaichaowarat & Wannasuphoprasit, SAE 2013-01-0001; RWD single-track
  model, no load transfer, 1250 kg, Table 1; MF fits of a P225/60R16 from Nicolas-Comstock 1972, Table 2, which give
  μ_y peak 0.98 at 9°, μ_y(30°)/peak 0.85-0.86, locked/peak 0.81, peak κ 0.15: NGP's shape, inside the tarmac band):
  the speed is nearly flat in sideslip (Fig. 6, read ±0.3 km/h): r10 m 34.0 km/h at β 4°, ~33 at 20°; r22 m 50.2 at
  β 15° (counter-steer, the best), 49.8-50.2 over 5-20°; ~0.90 g both; counter-steer from β 13° / 11.5° (Fig. 5),
  rear κ 0.2-0.33 at 15-20° (Fig. 7). With a tyre that keeps ~0.85 at 30°, a steady slide costs ≤ 3 % of speed.
- Brush grip × F on the flat circles (glat at F 0.8 / 0.9 / 1 / 1.1 / 1.2): gravel 0.709 / 0.783 / 0.877 / 0.960 /
  1.038, snow 0.711 / 0.814 / 0.900 / 0.978 / 1.029, tarmac 0.940 / 1.069 / 1.186 / 1.329 / 1.452: 0.82 g (gravel,
  snow) and 1.3 g (tarmac) per unit F, near proportional.
- Answer (accelerometer g at the car's limit; real from [Implied grip](#implied-grip-from-real-stage-times), its
  ±10 % transfer and the stages' spread): gravel real 1.02 g (0.93-1.15) against 0.92 (ngp) / 0.88 (brush): 0.90 /
  0.86 of the real (0.76-0.99). Snow 0.75 (0.69-0.85) against 0.89 / 0.90: 1.19 / 1.20 (1.05-1.30), the game above.
  Tarmac narrow 1.0-1.2, wide 1.3-1.4 against 1.18 / 1.19: in range. The stage pace (μ 0.47-0.52 against 1.01 on
  524) is the autopilot's, not the grip's. The factor on the brush's μ that reaches the real Rally2 (circle as driven):
  gravel 1.17 (1.07-1.33), snow 0.82 (0.75-0.94), tarmac 0.86-1.01 narrow, 1.09-1.17 wide; driven well (the rear's
  3 % used) gravel 1.14, snow 0.80. ngp from the brush's slope (estimated): gravel 1.12. Measurements, not a level
  change: the level is the user's decision.
- Croft-White (PhD, Cranfield 2006, read from the CORE copy): the 0.6-0.7 g is "peak lateral accelerations" from team
  telemetry, a body-mounted accelerometer with no roll compensation, Ford Focus WRC test runs on a very wet, loose
  gravel road near Malaga (February 2005, 14 runs, 60-140 km/h, two drivers' runs plotted), quoted in §4.5.2 as a
  check on the bicycle-model tyre forces; tarmac (dry, Barcelona 2006) ~1.4 g. Not a steady dry limit:
  `gravel-car-lateral-g` keeps the road cars' 0.43 as its lower bound, no upper bound, on the flat circle; the RSF 345
  run is context (`gravel-car-lateral-g-rsf345`). Context rows (not scored): `tarmac-car-lateral-rally2-narrow` /
  `-wide` (flat circle glat; the game's 1.23-1.25 g lies between the two event groups), the braking rows
  (`*-car-braking-rally2`, weak: μ_x = μ_y and a 0.5 s peak against a stop's mean), `car-class-ratio-rally2-wrc`
  (none: two cars).
- Scored against real Rally2 (2026-11-09): `gravel-car-lateral-rally2` (0.93-1.15 g) and `snow-car-lateral-rally2`
  (0.69-0.85 g) on the flat circle's glat; the level's reference for a rally tyre. CW's ≲0.67 and 50-60 % come from a
  very wet, loose road, so `gravel-peak-mu-y` and `gravel-over-tarmac-peak-mu-y` keep only their lower bounds (road
  and AT tyres, CW). After T55's hold, ngp / brush: gravel 0.916 / 0.923 g (MISS, 0.90 of the real), snow 0.885 /
  0.889 g (MISS, 1.18 of it), tarmac 1.254 / 1.234 g; gravel over tarmac on the circles 0.73 / 0.75 against 0.72-0.82
  (implied μ, Finland over Spain and Canarias).
- A player's drive in these terms (2026-11-09): `harness/replay_gg.py REPLAY.rrp... [AUTOPILOT.trace...] --out DIR`
  (a copy of the launcher's `.rrp`; the header names the physics, car and stage) runs the replay with
  `--replay-trace` and `--tyre-probe` (every 144 steps) and prints time, km, outside assistance, the largest distance
  off the driveline (the car's parameter on the `.trk` spline), rally_gg's braking and lateral figures (glat and
  kinematic) and each wheel's |F|/Fz over `limit_frames`' peak at its load (hardest 5 % by Σ|F|/ΣFz / median). A
  replay stays exact on any build whose physics matches the drive's (621 brush of 2026-11-08 22:28 on main 5f4c3209:
  all 346920 frames equal); otherwise it stops at the first differing frame (a Falstone brush drive of 13:37, before
  that day's brush changes: frame 22931) and the figures cover the frames before it. Autopilot 562 at f0.75 b0.6, ngp
  / brush: wheels 0.98/0.98/0.93/0.92 and 0.98/0.98/0.94/0.93 of their peak in the hardest 5 %, 0.53-0.60 at the median.

## Race autopilot

Test tooling (2026-11-09, branch autopilot/race-line): `rbr-standalone --autopilot OUT --race LAT,BRAKE,SLIP`
(`crates/rbr-standalone/src/autopilot/race.rs`) drives at the car's measured limits so stage times can be read against
real ones. LAT, BRAKE: the flat circle's lateral and the threshold stop's braking acceleration (m/s²), SLIP the stop's
best slip (`limit_pace.py limits --surface`; Fiesta Rally2 ngp / brush: tarmac 1.282 / 1.327 g, 1.303 / 1.315 g, slip
0.3 / 0.2; gravel-flat 0.971 / 0.955, 0.826 / 0.840, 0.5; flat snow 0.952 / 0.982, 0.825 / 0.845, 0.5), scaled by
one factor f. The line is a minimum-curvature line within the road's edges less 1.4 m: the road comes from the `.col`
triangles under NGP's material sheets, scanned across the driveline every 2 m (road p10/p50 width 562 4.6/5.2 m, 524
4.2/5.3, 422 3.8/4.8). The speed plan runs on that line with a friction ellipse in braking, held 0.15 s ahead.
Braking is under the `--plan` threshold controller at SLIP. On tarmac the default's heading law steers to the line. On
a loose surface (SLIP over 0.4) a curvature tracker steers it and the throttle eases past 0.06 rad of sideslip. The
default's law countersteers all sideslip, and in gravel's steady drift that held the steering at 0.2-0.4 while the
car ran wide. The tracker cost 11 s on 562 under ngp and spun the brush car. Gears, traction, recovery: the
default's. Without the flag every output is byte for byte the same (traces and `.out` of 562, 524, RSF 345's circle
and a brake stop).

- Split, Fiesta Rally2 ngp. Point mass at the measured limits on 562: on the default's driven path 603.8 s, on the
  line 539.0 s (−10.7 %), with unlimited power on the path 555.2 s. The default drive takes 903.5 s, so the speed plan
  and the default's guards cost ~300 s, the line ~65 s and power ~50 s. 524 at 0.92/0.83 g: path 248.6 s, line
  218.3 s (−12.2 %), default 353.5 s.
- Inside the race run on 562: the plan on the line at f0.9 without acceleration limits is 519.3 s, and the run takes
  589.2 s. With the run's own full-throttle acceleration (p90 by speed) the plan gives 603 s against the run's 606 s,
  so the controller is within 0.5 % and the remaining ~70 s is power: ~110 W/kg flat from 4300 to 6200 rpm, and the
  stage climbs 250 m. Traction and shifting sit inside that envelope and are not separated.
- Settings tried on 562: a tracking guard at 1.2 m costs 4 s; anticipation of 0.1 / 0.15 / 0.25 s gives 592 / 589 /
  594 s, and none gives 611 s with recoveries; a margin of 1.0-1.4 m drives alike and 2.2 m costs 30 s.
- Tarmac calibration, 562 at f0.9, clean (finished, no recovery, within 4.1 m of the driveline): ngp 589.2 s, brush
  587.7 s. Real Rally2 is 543.1 / 547.6 / 548.0 s, so ngp is 7.5-8.5 % over and the brush 7.2-8.2 % over. Implied μ
  (`implied_grip.py`, Canarias SS14 as drawn): ngp 0.911, brush 0.916, real 1.09-1.11. f0.92 gives 585.4 s; f0.95
  and f1 leave the line (path glat 1.18 g against the circle's 1.28).
- Tyre use on 562 in the hardest 5 % of moments (glat 1.03 / 1.06 g; `limit_frames`, NGP's peak): front 0.88 / 0.91,
  rear 0.84 / 0.87, car 0.84 / 0.88, a wheel ≥ 0.95 in 73 % of them; 0.65-0.67 at the median. The car falls short of
  0.9 because tracking runs out above f0.92, and the straights are power-limited.
- Loose, same parameters, f the largest clean of 0.4-0.6.
  - 524 ngp: f0.55 gives 278.4 s (f0.6: 4 recoveries), against a real Rally2 equivalent of 220.9 s (+26 %). Implied
    μ 0.602 (273.4 s less the 5 s spur) against the real 1.013. Tyre use 0.77 / 0.82, car 0.77 at 0.68 g.
  - 524 brush: no clean f from 0.4 to 0.6. The car swings into a sideslip pendulum of ±0.5-0.7 rad on part throttle
    at ~19 m/s; at f0.4 near 2220 m it runs 6 m wide and stops. The default drive is clean, 349.2 s.
  - Law per run: the slip picks it, so ngp and the brush drive the same law on each stage (562 the heading law, slip
    0.3 / 0.2; 524 and 422 the tracker, 0.5 both): the times compare like for like.
- The 524 brush pendulum is the tracker on the brush, not either alone (f0.4, the law forced by a test switch, not
  committed): brush + tracker 456.7 s with 6 recoveries, 27 swings over 0.3 rad (half period p50 1.1 s, peak
  |sideslip| p50 0.56); brush + heading law 316.4 s clean, ngp + tracker 315.9 s clean, ngp + heading 315.6 s clean,
  all three |sideslip| p99 0.07-0.09, max 0.21-0.22. The first swing (74.4-77.0 s, 20-27 m/s, gear 5) grows out of a
  weave the tracker starts at 30-35 m/s on a near straight from 69 s: steering ±0.1, glat ±0.5 g, period ~2.2 s, where
  the other three runs hold the steering at -0.01 to -0.1 and glat 0.1-0.4 g. Over the swing the throttle is 0 (the
  plan is under the speed) and the brake is off past 0.18 rad; the rear slip angle reaches 19-20°, the rear axle's
  Σ|Fy|/ΣFz 0.90 (p50 0.73), the front's 0.74 (p50 0.48), the front's slip angle falling to ~0 as the countersteer
  catches up a quarter cycle late. At the same place the other runs have the rear at 2.6-4.1° and Σ|Fy|/ΣFz ≤ 0.41.
  Steering to glat over the first 68 s (speed over 20 m/s, 2 s high-pass, 0.05 s steps): the brush answers 0.05-0.1 s
  later than ngp (lag 0.45 / 0.40 s against 0.35 / 0.40 s, tracker / heading) and with less glat per unit steering
  (3.1 / 3.8 g against 4.1 / 4.8 g); the tracker's loop, tuned on ngp, loses its margin on that. A controller-tyre
  interaction: the brush car is not unstable by itself on 524 under the default's law.
  - 422 ngp: f0.55 gives 428.5 s (f0.6 rolls), against Paddon's 383.8 s (WRC) (+11.6 %). Tyre use 0.81 / 0.82, car
    0.80. The default drive takes 650.8 s.
  - 422 brush: f0.45 gives 453.6 s; f0.5 rolls and f0.55 has 3 recoveries. Tyre use 0.72 / 0.71. The default drive
    takes 639.9 s.
  - There is no route for 422, so no μ.
- What limits 422 (T44): the drive braked at slip 0.2, tarmac's best stop, which is weak on snow, so it arrived 1.5-2
  m/s over the plan. The heading law then countersteered the drift to full lock, and the full-lock guard (5 m/s) and
  the tracking guard slowed it hard while it slid wide on the 4.8 m road. The race mode brakes at the stop's slip
  (0.5) and steers loose surfaces with the tracker; it counts full lock on its own steering. At f0.55 the guards held
  1660 steps (full lock) and 0 (tracking). `--plan`'s fixed `PLAN_SLIP` 0.2 is the same limit on loose surfaces. It
  is research tooling, not the default drive, so it is left unchanged.
- Power against grip, from the plan on each run's line with the run's own full-throttle acceleration (p90 by speed;
  the whole driveline, so times run a little over the race's): share of time accelerating (power-limited) / braking /
  at the curve limit, and what power costs against unlimited power at the same grip. 562 f0.9 ngp and brush: 0.64 /
  0.33 / 0.03, power 84.5 / 82.7 s (14 %). 524 ngp f0.55: 0.51 / 0.47 / 0.02, 28.2 s (9 %); at f1 0.66 / 0.33, 13 %.
  422 ngp f0.55: 0.48 / 0.48 / 0.04, 33.2 s (8 %); brush f0.45: 0.46 / 0.50 / 0.04, 34.5 s (7 %); at f1 11 %. The
  game car's power to weight, from its full-throttle acceleration on 562 (v·a, p90 per rpm band): ~110 W/kg from 4300
  to 6200 rpm, net at the road; a real Rally2 is ~214 kW at the crank on 1230 kg plus ~160 kg crew, ~154 W/kg at the
  crank (~130 at the wheels at an assumed 85 %). With 25 % more acceleration everywhere the 562 plan gains 12 s (2 %),
  524's 3 s (1 %): the tarmac gap is not mostly power. Of 562's ~8 %, f0.9 against f1 is ~4 % (plan 604.0 against
  581.9 s), power ~2 %, the rest line and controller. RSF's best human Fiesta Rally2 on 562 under ngp, 549.1 s, is
  about the real time: the car and physics reach it, the autopilot does not.
- Reading: on tarmac the car-level grip carries the car to within ~8 % of the real time at 0.9 of the measured
  limits. On gravel and snow the controller's clean limit (0.55 of the circle's g, tyres at 0.77-0.82 of
  their peak) sets the time, so the 26 % / 12 % gaps bound the grip deficit from above and do not measure it. A point
  mass at the full measured 0.92 g on 524's line gives 218.3 s, the real 220.9 s. An autopilot is not a driver.
- Not run: a second car on 562, which needs its own tarmac limits.

## Wong, Theory of Ground Vehicles, against the open items (2026-11-09)

J. Y. Wong, Theory of Ground Vehicles, 3rd ed. (Wiley), a scan read page by page (`research/wong`, notes only); book
page numbers. Labels as above; "inferred (ours)" marks our own integration of his equations.

| Quantity | Published | Source (precision) |
| --- | --- | --- |
| Shear deformation modulus K, Janosi-Hanamoto τ = τmax (1 - e^(-j/K)) (soils "type A") | firm sandy terrain 10 mm, loose sand 25 mm, clay at maximum compaction 6 mm, undisturbed fresh snow 25-50 mm; "may be a function of normal pressure" (bevameter plates and rings, 3-70 kPa); worked example on snow (n 1.6, φ 19.7°) K 50 mm; no gravel | Wong pp. 145-148, eq. 2.56, Example 2.3 (text values) |
| Shear with a hump ("type C": compact sand, silt, loam, frozen snow), eq. 2.61 | displacement at the peak Kω 27-71 mm and residual over peak Kr 0.38-0.72 (firm mineral terrain); firm snow Kω 22 mm, Kr 0.66 | Wong p. 149 (text values) |
| K fitted at vehicle scale | rubber-padded track on a paved road, μ 0.9, K 75 mm (deduced from a turning test) | Wong p. 412 (text; a whole-vehicle fit, lumps the running gear's compliance) |
| Braking effort against skid, loose gravel, 64 km/h | bias-ply 7.75-14 car tyre: 0.36 / 0.42 / 0.45 / 0.455 / 0.47 / 0.51 at 10 / 20 / 40 / 60 / 80 / 100 %, no peak below lock; over the 40-60 % plateau 0.80 / 0.93 / 1 / 1.04 / 1.13; locked over plateau 1.13 (band 1.0-1.29 from the band's width), the rise steep only past ~85 % | Wong Fig. 1.18 (SAE, ref. 1.17; fig ±0.02, band width 0.05) |
| Braking effort against skid, dry bitumen 64 km/h / glare ice 32 km/h | dry peak 0.80-0.85 at 15-20 %, locked 0.76-0.81 (0.95 of peak); ice peak 0.10-0.12 at 10-15 %, 0.08-0.10 at 50 %, locked 0.08-0.10 (0.8-0.9 of peak) | same figure (fig ±0.02) |
| Peak / sliding road adhesion (average) | asphalt and concrete dry 0.8-0.9 / 0.75; asphalt wet 0.5-0.7 / 0.45-0.6; concrete wet 0.8 / 0.7; gravel 0.6 / 0.55; earth road dry 0.68 / 0.65, wet 0.55 / 0.4-0.5; snow hard-packed 0.2 / 0.15; ice 0.1 / 0.07 | Wong Table 1.2 (table, secondary: his ref. 1.12; tyre and speed unstated) |
| Rolling resistance against speed, hard surface | radial car tyres, rated load and pressure, smooth road: f_r = 0.0136 + 0.40e-7 V² (V km/h, to 150), band 0.013-0.0145 at 0-50 km/h; bias 0.0169 + 0.19e-6 V²; Table 1.1: concrete/asphalt 0.013, rolled gravel 0.02, tarmacadam 0.025, unpaved road 0.05, field 0.1-0.35; a car tyre's at -10 °C shoulder temperature ~2.3 times its 60 °C value | Wong Fig. 1.3, eqs. 1.1-1.2 (Bosch, fit; fig ±0.0005), Table 1.1 (Bosch 1996, table), p. 14 / Fig. 1.12 (text) |
| Tractor drawbar pull against slip, agricultural soil | 2WD and 4WD tractors, dry loam stubble and wet clayey loam: slip rises slowly, then the curves turn vertical at the maximum pull and run to 60-80 % slip with no visible fall in pull (net traction) | Wong Fig. 4.6 (IMechE, ref. 4.6; fig ±0.5 kN; a drawbar-controlled test cannot hold a falling branch, weak) |
| Driven tyre on type-C soil, traction against slip | inferred (ours: eq. 2.61 over a uniform 0.2 m patch, j = i x, Wong's track method eqs. 2.69-2.71): peak 0.83-0.85 of τmax A at slip 0.21-0.32 (Kω 27 mm) to 0.56-0.84 (Kω 71 mm); full slip over the peak 0.65-0.92 (Kω 27) and 0.91-1.0 (Kω 71); firm snow peak at 0.23, full slip 0.86 of it | inferred from Wong p. 149 values |
| Combined slip, hard surface | radial car tyre (165-15, 2.45 kN, 40 km/h): cornering force at a given α about the same braking and driving; bias-ply: higher braking than driving; envelope a friction ellipse | Wong Fig. 1.39, p. 52 (NBS Monograph 122; figure, qualitative) |
| Aligning moment, combined | "the effect of a driving torque is to increase the aligning torque for a given slip angle, while a braking torque has the opposite effect"; no figure, no numbers, no reference on the sentence | Wong p. 40 (quoted) |
| Wet peak against speed, tread pattern (water depth unstated) | wet gravel ribbed 0.37 → 0.33, ribbed-siped 0.45 → 0.43, smooth 0.37 → 0.24 from 48 to 100 km/h; wet asphalt (panel has no tick labels; its ticks match the quartzite panel's 0-1.2) ribbed ~1.0 → 0.63, siped 0.87 → 0.62, smooth 0.92 → ~0.07 | Wong Fig. 1.47 (NBS Monograph 122; fig ±0.03) |
| Wet side force over dry against speed, 1.02 mm water | 4-groove tread, textured concrete: ~0.99 at 16, ~0.80 at 64, ~0.62 at 100, ~0.45 at 135 km/h; smooth concrete 0.85 at 16, ~0.1 at 105; smooth tread lower. 7.62 mm water: both treads at 0 by ~90 km/h (Horne's speed) | Wong Fig. 1.51 (Horne & Joyner, SAE, ref. 1.34; fig ±3 %) |
| Wet peak, tread depth (car tyres) | 7 mm tread 0.92 at 50 → 0.90 at 100 km/h; 0 mm 0.85 → 0.61; sliding 7 mm 0.62 → 0.45, 0 mm 0.52 → 0.31 | Wong Fig. 1.52 (ref. 1.8; fig ±0.02; water depth unstated) |
| Hydroplaning speed | V_p = 6.34 √p km/h (p kPa), 88 km/h at 193 kPa; Horne & Joyner's aircraft-to-car data on the curve; with water shallower than the grooves "a generally accepted theory has yet to be evolved" | Wong eq. 1.90, Fig. 1.50, p. 69 (formula, quoted) |

What it means for the model:

1. K (gravel α0): support. With the law integrated over a uniform patch (Wong eq. 2.71) the initial slope is τmax A
   L/(2K), so loose-v1's s0 = tan α0 maps to K = a tan α0: gravel 6.3° → 11 mm, wet 9.7° → 17 mm at a = 0.1 m, inside
   firm sand (10) to loose sand (25 mm); no gravel K is given. Packed snow's 3.6° → 6.3 mm sits far below fresh snow's
   25-50 mm but near firm snow's type-C rise (Kω 22 mm, so a type-A fit to the rise ≈ Kω/3 ≈ 7 mm, inferred). Plate K
   is measured at 3-70 kPa, under a tyre's pressure, and Wong's paved-road K of 75 mm shows fitted K lumps the running
   gear's compliance: order of magnitude only, nothing to change (snow stays on the flat brush).
2. Driven tyre: no measured tyre curve on soil. Tractor data show no fall in net pull to 60-80 % slip (weak); type-C
   soils give an inferred fall at full slip to 0.65-1.0 of the peak, firm snow 0.86, with the peak at slip 0.21-0.32 on
   Kω 27 mm terrain. The drive-test WIP (`tyre/loose-shape`, fall from 0.3 to F 0.85) lies inside both; nothing to
   score, nothing to change. Sinkage grows continuously with shear displacement (slip sinkage, Fig. 2.49, plates).
3. Combined slip on deformable terrain: nothing (Wong treats wheels on soil in pure slip; skid-steering of tracks uses
   one isotropic K on the resultant displacement, eqs. 6.36, 6.43). On hard surfaces Fig. 1.39 matches the brush's
   mild asymmetry for a bias-ply tyre and NGP's symmetry for a radial: context.
4. Aligning moment, combined: the "Wong" side of the conflict is one qualitative sentence with no data or reference
   in the 3rd edition; it cannot score the row. The row stays unscored; the MF sets' fitted s·Fx remains the only
   numbers, and they are one fitted term.
5. Hard surfaces: Table 1.2's gravel 0.6, packed snow 0.2 and ice 0.1 are road-tyre averages; NGP's gravel and snow
   0.83-0.85 stay above them as before (level stays NGP's), ice 0.094 meets 0.1. Fig. 1.18's loose gravel supports
   loose-v1's shape on two counts: the rise (0.80 / 0.93 of the plateau at 10 / 20 % against the shear law's 0.71 /
   0.92 at s0x 0.081) and a berm rising late, past ~85 % slip, to 1.13 (1.0-1.29) at lock at 64 km/h. loose-v1's
   PLOUGH 1.3 is at the band's top; the car-level locked/ABS rows stay the scored ones. A tyre-level row
   `gravel-locked-over-plateau` 1.0-1.29 (bias-ply road tyre, one figure) could be scored from the lab sweep (loose-v1
   1.30 at κ 1 over κ 0.5: on the edge; NGP 0.82/0.95 = 0.86: MISS). Ice sliding over peak: Table 1.2 0.70 (the brush's
   0.72) against Fig. 1.18 0.8-0.9 (the Open item's 0.81-0.88): split, no change. Rolling resistance: the walking-pace
   value of a free radial car tyre on a smooth road is 0.013-0.0145 Fz (Fig. 1.3), sourcing the RWD-on-ice item's
   0.01-0.015: NGP's 0.086 Fz free-wheel drag under 2 m/s is 6 times it. Tarmac 0.013 (Bosch) is above the car's
   coast-down 0.0066, as VTI's 0.010-0.0135 already said; nothing on ice.
6. Wet: Horne's v_p (6.34 √p) agrees with the model's 6.36 √p within rounding. Fig. 1.51 at 1.02 mm (wet over dry 0.80
   at 64, 0.62 at 100 km/h, a 1960s 4-groove tyre) falls earlier and further than the thin film at 0.7 mm (0.918 at 100
   over 60): consistent with a deeper film and an older tread, no contradiction. Fig. 1.52 (7 mm tread: 0.98 at 100
   over 50; 0 mm 0.72) agrees with a new tread losing little on thin water. Wet gravel (Fig. 1.47) loses only 4-11 %
   from 48 to 100 km/h with a patterned tyre: support for loose-v1 having no wet speed fade (`loose.rs`,
   `hydroplaning: false`). Nothing changes.

## Stage times: real against RSF and the game

Research (2026-11-09, main d903fe5a). Real times are from juwra.com, fia.com, Wikipedia and rally-maps.com. RSF ids and
lengths are from `stages_data.json`. Game: the default autopilot to the finish, dry, Fiesta Rally2 (ngp7) and i20 Coupe
WRC 2020 (ngp6, the WRC-class NGP car installed). Single runs, no recoveries. Ratios eb/ngp are Rally2 / i20, with
eb/experimental (the tyre alone) in brackets. RSF's best needs a login ("Please login." to visitors); the user's
saved rank pages are read below.

| RSF id, length | Real stage | Winner, time, car | Match | eb/ngp (eb/exp) |
| --- | --- | --- | --- | --- |
| 424 / 425, 7524 / 7881 m (tarmac) | Barum Rally Zlín 2004 SS2 Biskupice, 7.57 km, run once, Friday 17:06 | not found (ewrc-results returns 402). Jean-Joseph, 8th in a Clio S1600, 4:11.3, so the winner was under 251.3 s | 424 −0.6 %, length only. 425 is not this route | not run: the RSF track is encrypted |
| 562, 14884 m (tarmac) | Rally Islas Canarias, Agüimes–Santa Lucía: 2025 WRC SS14/17, 14.97 km (Sunday). 2024 ERC SS2/5, 14.83 km | 2025: Rovanperä 530.1 s (GR Yaris Rally1), Ogier 530.6. 2024: Bonato 543.1 / 543.9 (C3 Rally2), Armstrong 547.6 best of two (Fiesta Rally2) | −0.6 % / +0.4 %, good on length. Direction not checked (2026 runs it reversed) | 0.986 / 0.986 (0.998 / 0.997) |
| 501, 15594 m (gravel) | Rally Finland 2023 SS4/8 Myhinpää, 15.51 km (Friday) | Rovanperä 415.7 s, Neuville 419.6 (Rally1) | +0.5 %, good on length; direction not checked | 0.989 / 0.990 (1.001 / 1.002) |
| 524, 7777 m (gravel) | Rally Finland 2021 SS7 Kakaristo–Hassi, its last 7.7 km | Evans 206.7 s over that part, flying (Yaris WRC) | strong: GPS fit ([Real stage telemetry](#real-stage-telemetry-wrc-gravel)) | 0.988 / 0.989 (1.001 / 1.001) |
| 611, 7400 m (gravel) | 1000 Lakes 1986 SS5/37 Ouninpohja, 7.38 km (24.5 km in 1983-85, 16.9 km in 1987) | Alén 232 s (Delta S4), Blomqvist 232 s (Group B) | +0.3 %, good on length. 1124 is the same route, not installed | 0.986 / 0.989 (1.000 / 1.001) |
| 433, 2316 m (gravel) | Rally Finland 2019 SS1/11 Harju, 2.31 km (3.48 km in 2023) | Neuville 107.3 / 108.3 s (i20 Coupe WRC) | +0.3 %, good on length | 0.979 / 0.979 (0.995 / 0.995) |
| 137 / 121, 5100 / 5150 m (gravel) | Wales Rally GB 2009 SS2/5 Sweet Lamb, 5.13 km (December). 3.3-4.3 km in 2008-17 | Loeb 203.7 / 204.1 s (C4 WRC) | −0.6 / +0.4 %, weak: length only, the RSF version's year is unknown | 0.974 / 0.983 (1.003 / 1.003); 0.975 / 0.988 (1.003 / 1.004) |
| 531, 22463 m (snow) | Arctic Rally Finland 2021 SS9/10 Aittajärvi, 22.47 km | Evans 607.5 s, Rovanperä 602.4 (power stage) (Yaris WRC) | 0.0 %, good | 1.009 / 0.992 (1.019 / 0.998) |
| 615, 19957 m (snow) | Arctic Rally Finland 2021 SS4/7 Kaihuavaara, 19.91 km (27.93 km in the January Arctic Lapland Rally) | Evans 546.6 s, Rovanperä 548.8 (Yaris WRC) | +0.2 %, good | not used: the autopilot crawls (24-94 s under 5 m/s, spinning at a climb at 4.1 km); raw 1.182 / 0.953 |
| 422, 9742 m (snow) | Rally Sweden 2018 SS8/19 Torsby, 9.56 km | Paddon 383.8 s (Friday 16:56), Lappi 361.2 (Sunday, power stage) | +1.9 %, fair | 0.983 / 0.985 (0.999 / 0.998) |

- The user's Hassi drives by hand (2026-11-09, experimental-brush, dry; Fiesta Rally2 with 4 recoveries, stopped at
  7.3 km; GR Yaris Rally2 clean to 5.9 km, splits 89.8 / 177.5 s against RSF's Rally2 median 84.3 / 165.7 under
  ngp): lateral 1 s p90 / p99 0.83 / 1.05 g (glat) in both, g-g p95 |lat| 0.92-1.03 g at 60-150 km/h (Finland 2021
  WRC 0.89 / 1.25 and 1.04-1.14; a Rally2 at × 0.93 about 0.83 and 0.97-1.06; the at-limit autopilot 0.52-0.57 and
  0.39-0.70); tyres in the hardest 5 %: car 0.99-1.03 of the peak, Yaris front / rear 0.98 / 0.98, Fiesta 1.05 / 0.94;
  braking 16 longest 0.64-0.65 g (WRC 0.60), 0.5 s peak 0.69-0.79 (WRC 1.00). A driver reaches the tyres' peak and a
  real Rally2's cornering g on the brush's gravel level, the rear's reserve included (`replay_gg.py`).
- The eb/ngp shift is the engine model's, not the tyre's: 0.974-0.992 on every clean stage, with eb/experimental at
  0.995-1.004. The exception is 531 Rally2 at 1.019, a single run. The default autopilot drives a fixed plan (4 / 5
  m/s²), well under either tyre's limit, so these runs cannot show a grip difference. That needs at-limit runs
  (`limit_pace.py`), whose single-run scatter (5-15 s on 7.8 km) is larger than the effect.
- Real against RSF (2026-11-09; the user's saved NGP7 hotlap rank pages, read for car and finish time only, no names
  kept; NGP7 is `ngp` physics). Best / top-10 median per class, s, and over the real reference:

  | Stage (entries) | Real reference | RSF WRC 2017-21 | RSF Rally2/R5 | Fiesta Rally2 |
  | --- | --- | --- | --- | --- |
  | 562 Aguimes-Santa Lucia 2025, tarmac (1452) | WRC2 2025 SS14 548.0 (top 5 551.6), Rally1 530.1 | 520.7 / 532.4 (0.982 / 1.004 of Rally1) | 526.6 / 534.5 (0.961 / 0.975) | 549.1 / 559.3 (n 66) |
  | 524 Hassi, gravel (1796) | Evans 206.7 flying (Yaris WRC); Rally2 220.9 (implied μ × 0.93, estimate) | 197.4 / 202.3 (0.955 / 0.979) | 205.8 / 208.7 (0.932 / 0.945) | 212.4 / 219.7 (n 55) |
  | 422 Torsby 2018, snow (206; route +1.9 %) | Lappi 361.2 Sunday (Paddon 383.8 Friday) | 308.7 / 318.6 (0.855 / 0.882) | 320.3 / 331.3 | 343.2 (n 6) |
  | 531 Aittajarvi, snow (496) | Rovanperä 602.4, Evans 607.5 | 550.2 / 571.7 (0.913 / 0.949) | 552.0 / 563.1 | 581.5 (n 17) |

  Tarmac, where the car-level grip matches ([Car-level grip in the same terms](#car-level-grip-in-the-same-terms)),
  is the control: the sim's best Rally2 runs 2.5-4 % under the real WRC2 times. Gravel sits 2-3 % beyond that
  control (against the Rally2 estimate; the WRC row the same once Evans's flying start is allowed for, RSF's runs
  start standing): inside the ±5 % below, so no sign that NGP's gravel grip is low. Snow sits 5-12 % beyond it
  (Torsby on a longer route, Aittajarvi less): with f 0.6-0.8 corner-limited that is roughly 10-30 % more μ than the
  real cars used, the same sign as the flat circle's 1.2 (inferred; real snow varies with fresh snow, ice and stud
  wear, and a sim leans on snowbanks without risk). The Fiesta Rally2 has few entries (top drivers pick the Fabia RS
  Rally2), so its rows are not a car comparison.
- Reading a gap: if a share f of the stage time is corner-limited, Δt/t ≈ −f/2 · Δμ/μ. So a 5 % time gap is 10 % μ
  at f = 1 and 20 % at f = 0.5 (fast Finnish gravel averages 134 km/h). The time gap also contains the driver, the car
  and the route version. A sim's best knows the stage from hundreds of runs, takes no risk and runs on a fixed road;
  real drivers have two recce passes and a road that changes. Rally1 hybrid (2022+) and 2017-21 WRC differ from RSF's
  cars, and the route versions here are matched on length only. Together these spread the time by about ±5 %, so a
  gap below ~10 % says nothing about μ.

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
- Published (`data/tyre-targets.csv`, `coast:SURFACE:crr`): road cars coasting on Swedish roads Cr = 0.00912 + 2.1e-5
  IRI v + 0.00172 MPD at 8 °C (VTI 748A 2012 p. 60), 0.0101-0.0135 smooth to rough at 15 m/s; EU label C1 class A ≤
  0.0065 on the ISO 28580 drum; gravel 0.016 (Team O'Neil dry) and 0.018 (MPG, 0.64 cm pea stone on recycled asphalt),
  0.16 in pea stone over 20 cm (ERDC/CRREL TR-10-11, text), rolled gravel 0.02 (Bosch); fresh snow 11-22 cm, snow alone,
  leading tyre 0.024-0.127, trailing 0.017 (CRREL 95-7 Table 3); hard-packed snow near the hard surface's (VTI 971A); no
  car coast-down on ice found (the ice band is the VTI model with no texture, inferred). Gravel and snow PASS, tarmac
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
  gravel RSF 345 circle wheel 0 at step 6000; snow map 10 wheel 0 at step 6000; ice pad-116's circle with
  `--surface tarmac`, wheel 1 at step 8000, the tarmac tyre as on RSF 565/614). It builds the profile's
  `rbr_tyre::model` from the wheel's tables as the game does (the brush, and loose-v1 on soil, under
  experimental-brush) and lets its dynamics settle 2 s at each slip; where the model hands back (`ngp`) NGP's
  `patch_forces` answer. A new model name needs its line in `TYRE_MODEL`. Steady forces at 15 m/s, flat, no sinking
  and no soil force (the sweep, not `ngp_transient`'s whole step); the gravel shape rows (Coutermarsh, measured at 1-1.5
  m/s) are taken at 1.5 m/s (`gravel-1.5`, wet `gravel-wet-1.5` with WaterFactor 1) and 6.7 kN on a 25 mm layer
  (`--soil`; pea stone's unstated), Croft-White's at 20 m/s (`gravel-wet-20`) on the probe's 9 mm; `gravel-deep` /
  `gravel-deeper` the probe on 17.5 / 25 mm (`gravel-depth-alpha90-*`, context). Braking peaks (`peak_mu_x_brake_*`,
  `peak_kappa_*`, `mu_x_rel_k*_brake_*`, `mu_x_locked_rel_*`, the longitudinal load exponent) are the maximum over a
  braking slip of 0 to loose-v1's `BERM_FROM` 0.8, the free rolling side's peak before the berm; locked is κ -1. NGP's
  and the brush's braking peaks lie below 0.8 on all four probes (their keys equal the 0-1 maximum's, lab on the same
  probes).
  `tarmac-50`, `tarmac-damp-level-50` and `tarmac-wet-level-50`: the tarmac wheel at 50 km/h, dry and on NGP's
  damp / wet tarmac level (`--friction-scale` 0.925 / 0.85 times the tyre's class wet slope 1 - 0.075 W) with
  WaterFactor 0.25 / 1.
- `tyre-ratio:A/B:KEY`: surface A's key over B's.
- `brake:SURFACE:KMH:locked_g|best_g|ratio|ratio_kK|ratio_absK`: `brake_test.py`'s declutched runs (gravel RSF 345,
  tarmac RSF 568, snow map 10, ice pad-116 with the run-up on ice too, 45000 steps, slip targets 0.05-0.3),
  full pedal against slip targets 0.1-0.7; ratio = locked decel / best threshold decel, ratio_kK locked over the
  target K's (one pedal), ratio_absK locked over the per-wheel ABS at K (`--brake-abs`, a run per K the rows name).
  The locked-over-ABS rows (gravel 50/80, snow 50/80, tarmac 50) take ABS K 0.15; the K 0.2 and best-target ratios are
  context. No row reads wet gravel's locked over ABS (its 1.89 is the fixed K on the slower rise, no wet data).
- `coast:SURFACE:crr|crr_free`: `coast_test.py` from 60 km/h (tarmac RSF 568, gravel RSF 345, snow map 10, ice
  pad-116; [Rolling resistance](#rolling-resistance)).
- `brake-ratio:A/B:KMH:X/Y`, `circle:gravel:lat_g` (`car_pace.analyse_circle` on the gravel probe run, 40000 steps,
  ramp 0.4; on an 11.9° slope, context since 2026-11-09). The circle's slide speed also depends on the engine holding
  the ramp: read that row as car, not tyre.
- `circle:SURFACE:KEY` (`car_grip.py`'s flat r40 circle: `gravel-flat`, `snow` on pad-gravel / pad-snow with the
  gravel / snow tyre, `tarmac` on pad-tarmac; `kin_g`, `glat_g`, `path_g`, `glat_p95`, `beta_deg`;
  [Car-level grip in the same terms](#car-level-grip-in-the-same-terms)).
- On the pads (2026-11-10; `--material-as` is removed; a pad is one real material and the car runs the set-up of
  the pad's surface; Fiesta Rally2, ngp / brush, the old set-up in brackets): `tyre:ice:*` rows identical; ice
  braking locked 0.090 / 0.082 g (0.095 / 0.082), locked over best 0.949 / 0.813 (0.990 / 0.824); tarmac circle 1.333
  / 1.317 g (1.334 / 1.317); gravel-flat 0.964 / 0.936 (0.987 / 0.987 on map 85's tarmac set-up: pad-gravel with
  `--surface tarmac` reads 0.988 ngp); snow 0.929 / 0.939 (flat map 85 0.962 / 0.989; RSF 345's slope 0.478 /
  0.556); ice coast Crr 0.0035 / 0.0033 (0.0025 / 0.0024 on RSF 568 iced), read over 17.5-9.7 m/s: 60 to 10 km/h on
  level ice needs 1880 m and the pad has 1500. No verdict changes.
- Past the limit on pad-tarmac (`limit_recovery.py`, 2026-11-10; ngp / brush): limits within 0.4 km/h of map 85's
  and the lift runs match (Fiesta widest 1.64 / 3.35 m, back in 1.67 / 2.84 s). At full throttle the Fiesta and the
  Golf run wide without turning round (Fiesta widest 25 / 57 m, β 4.9° / 2.1°; Golf 10.9 / 38.8 m, β 3.3° / 0.6°;
  map 85 gave β 93° / 101° and 16.5° / 180°): map 85's spins came from its bank 11 m outside the circle (Fiesta ngp
  51 m from the centre: 92 to 76 km/h in 0.8 s, z +0.34 m). The Escort passes the limit under ngp (73.9 km/h, β
  26.9°, back in 1.2 s on a lift) and still spins under the brush.

Counts (2026-11-09, main b9bbe63d run as 5f4c3209, main plus tooling; full run `--jobs 4`; 162 rows, 141 measured,
112 scored, 29 context): ngp 41/112, experimental-brush 93/112 PASS (2026-11-08 `tyre/review-fixes`: 31/97, 48/97).
Since then:

- New rows (19 scored): ngp 6 PASS (`gravel-cw-mu-y30-over-peak`, `snow-car-locked-over-abs-80`,
  `tarmac-calpha-pressure`, `tarmac-peak-mu-y-pressure`, `tarmac-trl-wet-peak-100-over-32`, `tarmac-wet-speed-loss`),
  13 MISS; brush 17 PASS, MISS `tarmac-kx-pressure`, `tarmac-sigma-y-pressure`. 10 new context rows (the Rally2 car
  rows, `gravel-car-lateral-g-rsf345`, `-wrc-stage`, `gravel-car-locked-over-abs-satc-80`).
- Moved to context (both physics): `gravel-mu-y30-over-peak`, `gravel-peak-slip-angle` (gravel shape rows at their
  sources' speeds), `snow-lat-max-packed`, `snow-long-mu-winter` (road-tyre snow levels).
- Band changes (no model change): road-tyre gravel and snow levels became lower bounds: MISS → PASS
  `gravel-car-locked-decel-80` ngp, `gravel-car-locked-decel-50/80` brush, `gravel-locked-lat-20deg` both,
  `gravel-locked-lat-30deg` brush, `gravel-locked-mux` brush, `gravel-traction` both, `snow-peak-mu-x` both,
  `snow-peak-mu-y` both; `gravel-lat-end-over-max` 0.63-1 → 0.97-1: ngp PASS → MISS (0.960);
  `gravel-car-lateral-g` reads the flat circle (0.971 / 0.955, PASS as before).
- Model changes (brush, MISS → PASS): loose-v1's shear law on gravel (`gravel-pea-mu-y-a2.5/5/10/15`,
  `gravel-lat-ratio-5/10/20deg`, `gravel-lat-slope`, `gravel-lat-alpha90`, `gravel-peak-kappa`); the wet law
  (`tarmac-trl-wet-locked-over-peak-64/100`); `V_SHAPE` (`tarmac-trl-peak-100-over-32` 0.968 → 0.992);
  `CORNERING_OVER_NGP` and Cx 1.3 (`tarmac-peak-slip-angle` 9.5 → 7.0°, `tarmac-combined-a4` 0.349 → 0.292,
  `tarmac-long-c0x-road` 21.2 → 26.8); the ice knee scaled with it (`ice-car-locked-over-threshold-50` 0.809 →
  0.812). Lab: the aligning patch over the front axle's frames (`tarmac-mz-half-length-load-exponent` both, brush
  `tarmac-mz-peak-load-exponent`, `-trail0-load-exponent`, `-peak-alpha-over-fy`). No PASS → MISS from a model.
- Citation audit (T52, 2026-11-09; bands from the source pages, no model change): brush PASS → MISS
  `tarmac-combined-a4` and `tarmac-peak-fx-load-exponent`; no ngp verdict moved. (Both misses stand in Open.)
- Tyre temperatures (2026-11-10, `tyre/thermal`, full run against main 7d3c7312): six
  `tarmac-thermal-*` rows, one scored (`-wet-cooling-80`, Ejsmont 5-8 K: ngp 3.75, brush 3.68, MISS both), five
  context; `thermal:KEY` runs `ngp_transient --thermal-keys` on the tarmac probe's image (~5 min per physics). Moved
  (brush, all within their verdicts): ice locked over threshold 0.811 → 0.826, ice coast Crr 0.002 → 0.022, tarmac
  locked over ABS 0.833 → 0.830, the rest ≤ 0.012. Now ngp 43/115, experimental-brush 92/115 PASS.

It reproduces the tables above: gravel 345 ngp locked 0.628 g from 50 km/h, the 345 circle 0.571 g (brush 0.664); NGP
gravel peak μ_y 0.828 (13.75° on the 0.25° grid against the sweep's 13.5°, a flat top), μ_y(30°)/peak 0.956, μ_x(κ
1)/peak 0.82; tarmac brush peak κ 0.09, Fy(κ 0.25)/Fy(0) 0.226/0.292 at 2°/4°, Kx/Fz 26.8.

Tarmac, ngp / experimental-brush against the band (`tarmac-*`):

- TRL PPR496 (`tarmac-KMH` / `tarmac-wet-KMH`: the tarmac wheel at 32/64/100 km/h, `--speed`, the wet ones with
  `--water 1`, WaterFactor 1 on the dry friction level: the rows are ratios): locked over peak at 32/64/100 km/h:
  ngp 0.780 flat; brush 0.776 / 0.709 / 0.656 (was 0.668 at 100; 0.73-0.82, 0.66-0.76, 0.59-0.67); peak 100/32
  1.000 / 0.986 (was 0.968; 0.97-1.20); locked 64/32 1.000 / 0.902 (0.86-0.98); wet locked over peak 0.780 /
  0.659, 0.546, 0.505 (0.64-0.67, 0.52-0.57, 0.49-0.52; 0.583, 0.552 with the constant Coulomb loss before
  2026-11-08); wet locked 100/32 1.000 / 0.680 (was 0.730; 0.56-0.81); wet peak 100/32 1.000 /
  0.911 (was 0.951; 0.76-1.00).
  Contidrom (`tarmac-wet-80`, `tarmac-wet-KMH-worn`: `--tread 0.2`, key `stop_mu_x_4k`, the mean
  braked peak of a stop; `tarmac-wet-80-3mm`: `--tread 0.375`): new 100 over 80 1.000 / 0.979 (was 0.991; 0.97-1.0),
  1.6 mm over new from 80 / 100 1.000 / 0.680, 0.510 (was 0.685, 0.521; 0.66-0.83, 0.49-0.55), 3 mm from 80
  1.000 / 0.894 (was 0.898; 0.87-0.91; the lab's NGP side holds the probed friction, so its tread does not move it).
  The car row from 100 km/h is not
  scored (`how` none): its locked run leaves the line (In game).
- Shape (ngp; the brush's peak angle, combined slip and stiffnesses moved with the model, see In game and Open
  `tarmac-combined-a4`): peak slip angle 9.5° (5.6-8.5); combined Fy(κ 0.25)/Fy(0) at 4° 0.463 (0.29-0.34,
  fig ±0.05); Kx/Fz 14.0 (18.4-32.9); peak Fx ∝ Fz^0.985 (0.90-0.95).
- Aligning moment (the lab's `Tyre::aligning`: the torque `solve_forces` applies above 2 m/s, trail × Fy less the
  carcass offset Fy/k_y × Fx, flat): t0/a 0.319 / 0.316 (0.36-0.51; brush theory a/3 at the design load); t0 ∝
  Fz^0.585 / 0.949 (0.90-1.05); patch half length ∝ Fz^0.57 both (NGP's contact geometry, the front axle's clean
  frames at 1.6-5.5 kN, local 0.55-0.58; 0.55-0.65); Mz peak ∝ Fz^1.25 / 1.79 (1.60-1.95) (2026-11-08: the lab took
  the patch from wheel 0's frames alone, 2.3-3.9 kN, and held the end length beyond, which read as Fz^0.30 and put t0
  at Fz^0.31 / 0.69 and the Mz peak at Fz^0.98 / 1.54; the game was never short); Mz peak at 0.421 / 0.410 of the Fy peak's angle (0.29-0.41; the brush's 4.0° against 9.75° on the
  0.25° grid, scored MISS by 0.0003; 0.282 with M'_z); Mz to 0 never / at 1.18 (0.78-1.73: NGP's trail keeps 0.004 m at 10°, Mz 0.30 of its peak there); peak
  Mz/(μ Fz a) 0.163 / 0.159 (0.19-0.26; 0.106 with M'_z). Combined at 4°, κ ∓0.05 (context): ngp 0.60 / 1.23, brush 0.26 /
  1.13 (-0.11 / 1.38 with M'_z), both lower under braking, as the carcass offset makes them (NGP's trail barely falls with κ, 0.92 of pure at 0.05;
  the brush's 0.61-0.71 with M'_z, the MF sets' αeq 0.26-0.42). Ice: Mz peak at 0.25° / 1.75° (1.25° with M'_z; brush theory: a quarter of the
  full-slide angle); gravel and snow Mz peak at 0.42 / 0.25 and 0.50 / 0.25 of the Fy peak's angle.
- In game (`--trace-speed`'s `ffb` column, the filtered steering torque before NGP's curve; 2026-11-08, r40 circle
  ramp on map 85 made uniform dry tarmac (the former `--material-as 37`), limit ~1.2 g; the plain circle crosses a patch
  of peak μ_y ~0.8, which the earlier "brush goes light 24 % against NGP's 46 %" mixed in): a scratch build with the
  tyre's normal-axis torque zeroed keeps 60-75 % of the column on all cars (the suspension's caster and jacking through
  Fy and Fz, NGP's car data), so the tyre's Mz is a quarter to a third of what the wheel feels. Peak torque ngp / brush
  before / brush now (Fiesta Rally2 4WD, Golf II FWD, Escort Mk II RWD): 279 / 241 / 284 at 1.10-1.15 g, 107 / 89 / 105
  at 0.87-0.92 g, 65 / 54 / 69 at 0.9-1.07 g; at the limit 206-212 / 178-195 / 176-200 (-25 / -24 / -35 %), 56-62 /
  49-53 / 49-54 (-45 / -43 / -53 %), 62 / 46 / 62 (-5 / -15 / -10 %). Before, the brush's Mz share peaked at 0.6-0.7 of
  the limit and fell 70 % (NGP's at 0.85, -50 %); its fall was never smaller than NGP's, its level 10-20 % lower at
  0.4-1.0 g. With the affine trail the brush is within 4 % of NGP's at 0.4-1.0 g on all three and falls further at the
  limit. Real car (Hsu, PhD 2009, Figs 2.12-2.14: P1, steer by wire, 10 m/s ramp steer on pavement, front axle load-cell
  torque incl. mechanical trail 0.026 m against t0 0.023 m; fig ±20 N m, ±0.05 g): torque peaks ~280 N m at 0.70-0.80 g
  (0.75-0.85 of the 0.95 g limit, α_f 4-5° against the Fy peak's ~8-10°), 225 at 0.90 g (-20 %), 180-200 when the limit
  is first reached (-30 %), 100 well past (α_f 17°, -64 %). The game's mechanical share is larger and its torque peaks
  later (0.79-0.91 of the limit, both models).

Ice (16 scored, the probe and brake rows on 116; ngp / experimental-brush): peak slip angle 0.75° / 4.5° (4.5-5.1),
5.0° at 6.7 kN for the brush (4-5); peak κ 0.02 / 0.06 (0.04-0.06); Cα/Fz 0-2° fit 3.72 / 2.43 (3.7-5.5); Kx/Fz 2.86
/ 2.52 (3.6-6.7); μ_y(20°)/peak 0.97 / 0.915 (0.59-0.86; was 0.91); locked/peak 0.92 / 0.72 (0.81-0.88); car locked over
threshold 0.990 / 0.826 (0.81-0.88; was 0.809); car decels 0.096 / 0.100 g threshold, 0.095 / 0.081 g locked (studless
0.18-0.22): the level is NGP's. The brush's stiffness on ice trades the peak angle against the initial slope ([Ice
(material 116)](#ice-material-116)).

Gravel and snow under loose-v1: [Loose surfaces: loose-v1](#loose-surfaces-loose-v1).

Camber and pressure rows (2026-11-08; lab keys `camber_per_fz_4k`, `camber_over_calpha_4k`, `*_dp05_4k`: +0.5 bar on
the probe's 2.0 bar through NGP's stiffness factor, patch length held; σ_y as the 63 % distance of a 1° step):
MISS → PASS with the camber thrust: C_γ/Fz 0.842 (was 0.017; 0.55-1.27; ngp 1.706), C_γ/Cα 0.049 (was 0.001;
0.031-0.061; ngp 0.097). With the pressure law (2026-11-10): σ_y -16.3 % (was -5.1; PASS, -26 to -12; ngp -0.8),
Kx -4.5 % (was +4.9; MISS, MF 6.1 -17 to -14; ngp +4.4); PASS Cα -5.7 % (was +5.8; -14 to +12.5, sources disagree;
ngp +5.6), peak μ_y -0.16 % (was +0.3; ±3). Ice locked over threshold now 0.826 (thermal rows). Car rows within
0.005 except the gravel circle (car, not tyre) 0.620 g (was 0.609, 0.667). Map 85 ramped circle with the setup's camber
at 0°, as set (-3.9°/-1.2°) and -4° (`--setup`): slide at 0.880/0.875/0.876 g brush (0.880/0.875/0.875 before),
ngp 0.890/0.890/0.889; steering against lateral acceleration within the autopilot's noise. With main's aligning rows
merged: 52/106 (ngp 33/106), the ice row back at 0.807.

Wet rows (2026-11-08, `tyre/wet-shape`, full run against 4bb4c042): MISS → PASS TRL wet locked over peak at
64 / 100 km/h (0.546 / 0.505, was 0.583 / 0.552; with the road texture's drainage; the wet tyre rows rerun); the five new
wet rows (above) pass under the brush, ngp misses the three tread rows; every dry row and the brake, circle and coast
rows unchanged.

Snow levels (2026-11-09, `tyre/scorecard-loose`, full run against main d903fe5a; bands only, no model change):
A rally car on snow runs studs (5.5-7 mm), at or above a road
tyre (SR 115: studded 0.57-0.60 against snow tyres 0.34-0.41), so the road-tyre levels are lower bounds: MISS → PASS
`snow-peak-mu-y` 0.852 / 0.842 (0.35..), `snow-peak-mu-x` 0.871 / 0.863 (0.46..); MISS → CTX `snow-lat-max-packed`,
`snow-long-mu-winter` (same quantity as those). New: snow locked over ABS K 0.15 at 50 / 80 km/h ngp 0.982 MISS /
1.059, brush 1.054 / 1.087 (1.00-1.10); gravel SATC context 1.076 / 1.340. Every other row unchanged.

Thin film (2026-11-08, `tyre/wet-speed`, full run against main 0fb71ed8): seven wet rows move, all in band: TRL wet peak 100/32 0.977 → 0.911 (0.76-1.00), wet locked 100/32 0.730 → 0.680
(0.56-0.81), wet locked over peak at 100 0.490 (was 0.491; 0.49-0.52, at the edge); Contidrom new 100 over 80
0.997 → 0.979 (0.97-1.0), 1.6 mm from 80 / 100 0.682 / 0.516 → 0.680 / 0.510, 3 mm 0.895 → 0.894. Every other row
bit-equal.

Pressure law (2026-11-10, `tyre/brush-misses`, full run against main 75bdb7b3, 114 scored rows): MISS → PASS `tarmac-sigma-y-pressure` -5.4 → -16.3 % (-26 to -12); `tarmac-kx-pressure`
+4.0 → -4.5 % (MISS, -17 to -14), `tarmac-calpha-pressure` +5.3 → -5.7 % (PASS), `tarmac-peak-mu-y-pressure` +0.12 → -0.16 %
(PASS); every other row within 1 % (the scorecard's tarmac circle, Fiesta Rally2 r40 ramp 0.5 on map 85: lateral
1.234 → 1.236 g). The aligning rows (`tarmac-mz-trail0-over-a` 0.310, `tarmac-mz-peak-per-mu-fz-a` 0.158)
are unchanged and left (Open).

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

## Experimental tuning

Controls a driver sets per drive to feel the brush (`rbr_tyre::Tuning`, `rbr-standalone --tyre-tuning KEY=VALUE,...`,
experimental-brush only, refused under the other physics; the launcher's Settings > Experimental tuning). At the model's
values every force is the untuned brush's, bit for bit (factors exact at 1, the slide grip unset): main's and the
branch's autopilot traces cmp equal on RSF 562, 524, map 85's flat gravel circle and an RSF 568 brake test, ngp and
experimental-brush (2026-11-09). The replay's header keeps the values (`replay_gg.py` prints them): a tuned drive
replays all 42617 frames equal; without the flag it differs at the go. No records either way (the experimental
physics keeps none); championship stages and trials run untuned.

| Key | Range | Maps to | Also moves |
| --- | --- | --- | --- |
| `slide-grip` | 0.70-0.95, model 0.83 (solved untuned too) | μ_y(30°)/peak at 4 kN, 15 m/s, free rolling, on the calibration point: the lateral Coulomb level over the static one (`MU_C_OVER_S` 0.8 before the solve) solved per wheel by bisection over 0.05-1.0, the peak and slope solved again (`fit::calibration`) | the locked side force past the peak (`locked_mu_y_a30_4k` 0.432 / 0.477 / 0.486 at 0.70 / model / 0.95); peak angle 7.0° to 7.25° from 0.92. Braking unchanged (only the lateral axis's level moves): peak μ_x brake 1.2921, locked over peak 0.718, Cα and Kx within 0.1 % |
| `tarmac`, `gravel`, `snow`, `ice` | ×0.70-1.30 | NGP's grip handed to the brush (static friction) by the wheel's surface: hard surfaces of class 5 (material 116) ice, other hard tarmac; soil of class 3 snow, other soil (gravel, grass, rock) gravel; a two-material blend by its class shares | the slopes that follow the level: gravel's shear law and ice's low-μ stiffness ×F; tarmac Cα/Fz 22.9 → 21.7 / 23.8 at 0.8 / 1.2 (the brush's 0-2° slope bends with μ). NGP's soil force on top is not scaled |
| `stiffness` | ×0.70-1.30 | the slip stiffnesses: the brush's cornering slope target (`CORNERING_OVER_NGP` × F, Cx = `KX_OVER_KY` Cy), NGP's slope ice falls back to, loose-v1's slope and gravel's shear law (s0 / F); the peaks solved again | the peak slip (7.0° → 10.75° / 4.75° at 0.7 / 1.3) and the relaxation length σ = C/k with it (0.249 → 0.164 / 0.353 m) |
| `response` | ×0.5-2.0 | the relaxation lengths: both carcass stiffnesses over F (the damping's V d/c with them) | nothing steady |

Each control does what it says (lab `scorecard --tuning` on the scorecard's probes, Fiesta Rally2, 4 kN, 15 m/s; the
circles `car_grip.py` r40 on map 85, flat):

| Control | Model | Set | Reads |
| --- | --- | --- | --- |
| slide grip (`mu_y_rel_a30_4k`) | 0.8865 | 0.70 / 0.75 / 0.82 / 0.886 / 0.92 / 0.95 | 0.7047 / 0.7545 / 0.8240 / 0.8895 / 0.9231 / 0.9528 |
| tarmac grip, peak μ_y 4 kN | 1.271 | ×0.8 / ×1.2 | 1.020 / 1.520 (×0.803 / ×1.196) |
| gravel grip, peak μ_y | 0.828 | ×0.8 / ×1.2 | 0.663 / 0.994 |
| snow grip, peak μ_y | 0.842 | ×0.8 / ×1.2 | 0.674 / 1.010 |
| ice grip, peak μ_y | 0.0941 | ×0.8 / ×1.2 | 0.0753 / 0.1129 |
| circle kin g, gravel / snow / tarmac flat | 0.955 / 0.982 / 1.327 | ×0.8 | 0.767 / 0.766 / 1.065 (×0.80 / 0.78 / 0.80) |
| same | | ×1.2 | 1.130 / 1.124 / 1.584 (×1.18 / 1.14 / 1.19) |
| stiffness, Cα/Fz tarmac / gravel / snow / ice | 22.94 / 6.64 / 8.63 / 2.42 | ×0.7 / ×1.3 | 15.58 / 4.82 / 6.05 / - and 31.12 / 8.33 / 11.22 / 2.82 |
| response, σ_y (63 % of a 1° step) tarmac / gravel | 0.249 / 0.104 m | ×0.5 / ×2 | 0.1245 / 0.053 and 0.498 / 0.208 m |

The slide grip is solved on the calibration point (unit surface, best temperature, pressure factor 1) and read on the
probed wheel; the two agree within 0.005. The lab's peak μ per wheel for `replay_gg.py`'s tyre use (`limit_frames`)
stays the untuned brush's. The table's model column is the brush before the model took 0.83.

- The model's slide grip is 0.83 (2026-11-09, a user decision after driving 0.83 on the panel): the solve runs
  untuned too (`tuning::SLIDE_GRIP_MODEL`), in place of `LATERAL_FALL`'s own 0.886. It is road tyres' top (0.73-0.83),
  racing tyres' lower part (0.80-0.90) and the top of Croft-White's WRC tarmac 0.71-0.83. Scorecard, experimental-brush
  (f36c3a34): 90/111 both; `tarmac-mu-y30-over-peak` 0.834 (was 0.886, read on the probe); ice
  μ_y(20°)/peak 0.915 (was 0.940) and its end over max 0.949 (was 0.963; both still MISS, toward the band);
  ice coast crr
  0.002 (was 0.033; MISS both); braking within 0.4 %. The flat r40 tarmac circle 1.143 g (was 1.187; ngp 1.181; real
  narrow-tarmac Rally2 1.0-1.2 g, context). The solve costs about 0.1 s per wheel when a brush is built (lab, two
  builds: 3.0 s, was 2.8).
- Why the flat r40 tarmac circle lost it (T54, 2026-11-09; `car_grip.py`'s hold, probes every 36 steps,
  `limit_frames` per wheel, |F|/Fz over the model's own peak μ_y at the wheel's load; brush 0.83 / brush
  `slide-grip=0.886` / ngp): the circle is front-limited and the autopilot holds the fronts far past their peak. Over
  the fastest second the Fiesta Rally2's fronts run at 22-24° slip angle (3.0-3.8 times their peak angle, 6-7.6°),
  sliding whole (ψ 3-4), with 0.09-0.15 drive slip; tyre use 0.87 / 0.90-0.92 / 0.94-0.96. The rears stay below the
  peak (3.5-4.8°, 0.5-0.7 of it; use 0.84-0.91 / 0.87-0.92 / 0.76-0.97). glat 1.143 / 1.195 / 1.181 g. The tracker
  winds the steering on (0.25 → 0.77) as the speed ramps once the fronts reach their peak (~6° at 21.5 m/s), and the
  fastest second falls in that slide, so the row reads μ_y near 22°, which the slide grip sets, not the peak. The
  Escort Mk II (RWD) holds a power drift instead (β -17°): fronts at 5.7-6.2° (0.77 of the peak, use 0.96-0.99), rears
  at 20-22° with 0.16-0.66 drive slip (use 0.79-0.97); glat 1.090 / 1.117 / 1.123 g (-2.4 %). Since T55
  (2026-11-09) the circle holds the steering once more steering stops buying lateral acceleration
  (`autopilot/limit.rs`: over 2 s the inward steering up ≥ 0.05 of lock while the lateral g rose under a quarter of
  its relative rise, within 0.95 of the run's best g, for 0.5 s; the same for every model, no tyre constant), and the
  ramp runs the car wide. The circle now measures the car at its front limit: tarmac, fastest second (ngp / brush),
  steering 0.38 / 0.31, outer front 10.8° / 7.2° (1.05 / 0.95 of its peak angle, use 1.00 / 0.98), inner front 11.1° /
  7.3° (1.9 / 1.2, use 0.97), rears 0.5-0.7 of their peak (use 0.83-0.97); glat 1.254 / 1.234 (was 1.181 / 1.143),
  kinematic 1.334 / 1.333 (was 1.282 / 1.321); gravel-flat and flat snow within 1.6 % (glat 0.916 / 0.923, 0.885 /
  0.889). The Escort never triggers it (its steering unwinds into countersteer as the rears saturate): traces
  byte-identical. RSF 345's sloped circle unchanged (its swings sit below 0.95 of the best g).

## Open

- Tyre temperatures ([Tyre temperatures](#tyre-temperatures)): the hot surface makes recovery past the limit harder
  (Golf and Escort full throttle now spin); a driver's test decides. Unmeasured: a slide's surface rise and decay
  (the 35 K in 1 s and τ 0.5 s are the model's alone), the wet road's conductance (wet cooling 3.7 K against 6-7 K,
  NGP's alike), the road term (NGP's, kept), soil (NGP's heat step). Ice coast Crr moved 0.002 → 0.022 (MISS
  both, band 0.007-0.013), not traced.
- Kx/Ky 1.17 at 4 kN with both stiffnesses 1.3 times NGP's slope's, near the band's foot (1.15-1.62); Kx's
  pressure dependence is -4.5 % per +0.5 bar (2026-11-10, `brush::SLIP_PRESSURE`: the slip stiffnesses' power of NGP's
  stiffness factor, -1.1, the sources' mean Cα -7.2 %, Cx the same by brush theory's a² ∝ 1/p; hard surfaces, NGP's level
  kept at 2.0 bar) against MF 6.1's -15 %, the only source on Kx (one fitted road set): its sign is right, its size
  MISS; a steeper Cx power would be that set alone. Cold class-2 surfaces (grip
  0.57-0.75) get part of the 1.3 only.
- Driving it: combined slip (0.72 of NGP's side force at 4° and 25 % slip, inside the measured road data; see
  [Racing and rally targets](#racing-and-rally-targets)) is the largest change a driver would feel. The
  deformation torque M''_z (∝ C_y - C_x) is left out of the aligning moment. Combined Mz: the MF sets' fitted s·Fx and
  Wong's Goodyear data disagree on whether braking or driving raises it, so neither M''_z (bristle, Svendenius eqs.
  4.37-4.39, braking lowers Mz with C_x > C_y) nor the longitudinal half of the carcass torque Fx Fy (1/c_x - 1/c_y)
  (his eq. 4.72, which would cancel NGP's offset to within 10 % at c_x 450, c_y 405 kN/m) was added (2026-11-08).
- `tarmac-combined-a4` 0.292 against 0.23-0.27 (T54, 2026-11-09; no model change). At κ -0.25, α 4° the patch slides
  whole (θ_x s_x ≈ 2.3), so Fy/Fy0 is the sliding velocity's lateral share (0.269) times |F|/Fy0 (1.086); Kx/Ky,
  the adhesion split and the carcass do not enter, and the axis frictions there are isotropic (μ_x(κ -0.25) 1.232,
  μ_y(15°) 1.220, the same sliding speed). Svendenius's points (Fig 5.5 redigitised: ±4° at 22.5 % 0.25 / 0.26,
  Fig 5.7: Fy/|F| 0.18-0.24 at 25 %) point the force 10-30 % more longitudinally than their sliding velocity, while
  their |F|/Fy0 is 1.15 (higher than ours); his own models through those data give 0.31. Lab, the alternatives costed
  (a4 / a2 / locked μ_y at 30°, 4 kN): now 0.292 / 0.225 / 0.467; MDR anisotropic sliding friction (his eq. 4.22)
  0.289 / 0.223 / 0.486 (the brush's frictions are isotropic, so it barely moves); the whole sliding force along the
  stiffness-weighted slip (Dugoff's direction, the brush before 2026-11-07) 0.215 / 0.164 / 0.360 (below the band,
  and the locked wheel's force no longer opposes its velocity, locked μ_y -23 % at 4 kN, -41 % at 6.7 kN). The
  published MF sets' fitted combined weighting G_yκ at 4 kN, α 4°, κ -0.25 gives 0.34-0.50 (MF 6.1 example 0.40,
  TNO 205/60R15 0.41, Continental 225/50R18 0.34, MF 5.2 example 0.50; their μ_x/μ_y 1.07-1.21), above the brush. So
  the one measured tyre is below every physical model and the MF sets above the brush; only a fitted direction
  factor would put it in the band.
- Hard-surface misses left on purpose (2026-11-10, then scorecard 93/114):
  (1) `tarmac-peak-fx-load-exponent` 0.933 against
  0.94-0.97: the band is the sets' longitudinal-over-lateral rate (0.73 / 0.39) on Goodyear's lateral 0.92, the brush's
  lateral is NGP's 0.878 (the load sensitivity of the peak is grip, kept), and 1 - 0.55 (1 - 0.87) gives 0.93 as built;
  within the figure's ±0.02. (2) `tarmac-bergman-wet-locked-over-peak-48` 0.566 against 0.60-0.66: the wetness term
  alone gives 0.592 at 50 km/h (TRL's 32 and 64 km/h rows 0.657 / 0.542 interpolated, inside the band's edge), and NGP's
  own wet tarmac level (μk/μs 0.770 against dry 0.800) takes the 0.9625 more (0.570, lab 0.566): the wet level is NGP's.
  (3) `tarmac-car-coast-crr` 0.0066 against 0.007-0.0135, `ice-car-coast-crr` 0.0022 against 0.007-0.013: the torque is
  NGP's surface `RollingResistance` applied by `rbr-physics` outside the tyre model (`ForceOutput` carries no rolling
  term), ngp and the brush read the same; a hysteresis term independent of the surface (VTI's Cr0 0.0091) needs a new
  seam output and moves every car's top speed, and the car's level sits 0.002-0.003 under the torque's share (not
  traced). (4) `tarmac-combined-a4` 0.292: unchanged (the entry above).
- Aligning moment level and patch: t0/a 0.32 against the sets' 0.36-0.51 stays brush theory's a/3; peak Mz/(μ Fz a)
  0.159 with the affine trail (2026-11-08) against 0.19-0.26 (0.107 with M'_z). In game (`ffb` column, map 41, 600 s
  autopilot, mean |ffb| at 0.3-0.4 / 0.4-0.5 / 0.5-0.6 / 0.6-0.7 g, ngp / brush with M'_z / affine): Fiesta Rally2
  138 / 126 / 132, 169 / 154 / 165, 209 / 182 / 199, 236 / 213 / 238; Golf II 55 / 49 / 52, 70 / 64 / 69, 85 / 73 /
  81, 108 / 86 / 97: +7-8 % over the band's steps against M'_z (+5 % at 0.3 g, +12 % at 0.65 g), within 6 % of NGP's.
  NGP's curve output above 0.95 on 21 / 19 / 21 % of the Fiesta's steps at the default S 105, under 1 % at S 262.5;
  the 10000 clamp is never reached. Autopilot pace unchanged (Fiesta 304.00 → 304.03 s, Golf 314.09 → 314.09).
  The load dependence is in band: NGP's patch grows ∝ Fz^0.57 (MF-Swift 0.59-0.64), so with the trail's own
  (Fz/Fz0)^0.35 t0 ∝ Fz^0.95.
  The level through physics (2026-11-08, numeric brush, 4 kN, a 0.074 m, Cy 70.4 kN/rad, μ 1.26; prototype, not
  built): the bristles on a stretched-string carcass (Svendenius eq. 4.62, Pacejka's string), tread stiffness from
  Cx = 2a²c_p (isotropic rubber, his eq. 4.70) and the string's foundation set so Cy stays NGP's, give t0/a
  0.355-0.369 for a string length 0.05-0.4 m but peak Mz/(μ Fz a) only 0.109-0.113; a flatter pressure than the
  parabola raises the peak (1-u⁴ 0.132, 1-u⁸ 0.155, uniform 0.187, with the string and 1-u⁴ 0.139) but moves the Fy
  peak to 14-20° and the Mz peak to 0.11-0.19 of it. No sourced mechanism reaches 0.19-0.26 with the force curves
  kept, so the level stays brush theory's.
  Rows and level (2026-11-10; no model change): both rows divide by NGP's patch half length a, the sets' by
  MF-Swift's contact length, so they compare two different a's; in metres the brush's t0 is 0.023 m at 4 kN
  (0.31 · 0.074), the P1 car's measured pneumatic trail (Hsu, PhD Stanford 2009, Figs 2.12-2.14) 0.023 m, and the
  in-game steering torque is within 4 % of NGP's over 0.4-1.0 g. The string carcass would lift t0/a only to
  0.355-0.369 (prototype above), the peak not at all (0.109-0.113), so no row would pass; a trail factor to the band
  is a fit. Mz does reach the driver: `ForceOutput::pneumatic_trail` (`car.rs`) is NGP's trail, times Fy it is the
  normal-axis torque of `rbr-physics` `tyre/forces.rs` (the car's yaw moment and, summed over the steered wheels,
  the force-feedback torque). Stocco et al. 2024 (flexible-carcass brush with a quartic pressure, eq. 7)
  give no route either: in the rigid brush, pure side slip, their convexity λ 4.05 (a literature estimate, not
  fitted; λ 12 is the parabola, 0.105) gives 0.123, and λ 0 (1-u⁴) gives 0.132; moving the pressure centre 0.02 of
  the patch forward, as rolling resistance does, gives 0.108, rearward 0.138; their flexible carcass lowers Mz at
  high load (Fig. 9); and their Mz fits are their weakest, with the race tyre's axes hidden.
  Autopilot pace, map 41, 600 s, finish ngp / experimental / experimental-brush (2026-11-08): Fiesta Rally2 309.01 /
  304.61 / 304.06 s, Celica ST185 310.91 / 305.17 / 304.48, Golf II 324.24 / 314.57 / 313.77, 131 Abarth 323.35 /
  315.49 / 315.15: the engine 89-96 % of the gap (1.4-3.0 %), the tyre 0.1-0.25 %. The autopilot plans 5 m/s² lateral
  and 4 m/s² braking, under half the grip. Of the tyre's share, Kx (shape, data-backed: NGP 14 Fz, published 18.4-32.9,
  brush 21) 0.18-0.50 s, a build with NGP's Kx: the brush's driven wheels slip a third less on throttle. Ruled out
  (builds, not committed): NGP's Cα load exponent (within 0.04 s); no carcass dynamics above 3 m/s and a friction flat
  with sliding speed are faster still. 0.1-0.2 % unattributed.
- Past the limit on tarmac (user, 2026-11-08: "the slip was very immediate, and then it's pretty much impossible to
  get grip back"): with V_SHAPE and LATERAL_FALL (μ_y(30°)/peak 0.89, the band's top) the Fiesta runs 2.1 m wide
  after a lift against NGP's 1.6 m, the Golf 4.0 against 2.0, the Escort spins less (97°, not 180°). A driver's test
  is next; a rolling tyre's measured past-peak shape at speed would settle the value.
- σ for a rally tarmac tyre against speed and pressure is unmeasured. Ours shortens 16 % per +0.5 bar (2026-11-10,
  PASS; was 5 %; op het Veld -12 %, MF 6.1 -21 to -26 %, racing bicycle tyres twice as long at 3.5 as at 7.5 bar,
  Meccanica 2023): Cα now falls with pressure (-5.7 %, band -18 to +5.7; it rose
  5.3 % with NGP's factor), as MF 6.1, op het Veld and Goodyear (4 kN) have it; the A005 slick's rises. Kx against
  pressure: -4.5 % against MF 6.1's -15 % (one set; Open, first entry).
- Wet and damp (research and check above, 2026-11-08): (a), the thin film, is in (`tyre/wet-speed`, above). (b), the
  level, is a drive-test switch only (`RERBR_TEST_WET_LEVEL`, In game): μ × (1 - a min(1, W/0.25)), a 0.08 puts
  damp at 0.81-0.85 and wet at 0.66-0.67 of dry in game (bands 0.70-0.85, 0.65-0.80; the lab's 0.84 / 0.72 estimate
  was the peak's); it moves the grip level off NGP's on damp and wet, the user's decision after driving it. The thin
  film's form is linear in W: Reimpell's loss grows faster than linear below 0.5 mm (Beautru's critical depth
  0.15-0.34 mm), so W 0.25 may lose 1 % too much. Wanted: Gengenbach (ATZ 70, 1968), Bachmann
  (VDI 12, 1998), Veith (ASTM STP 793), Oh & Lee (Adv. Mech. Eng. 2014, doi:10.1155/2014/260428).
- Wet papers (2026-11-09, `tyre/papers-wet`, notes only; rows "drums", "void and texture", "Damp (thin film)", "Worn
  tread", "groove pattern" above). (1) Speed fade: two more drums support `tyre/wet-speed`'s thin film at 0.7 mm,
  100 / 140 km/h over 60: Liedl 0.917 / 0.796, Williams braking 0.98 / 0.90, lateral 0.925 / 0.84, Reimpell 0.92 /
  0.83; the branch's 0.918 / 0.836 sits in the middle; keep the form and 0.0074. Linear within ±0.02; Liedl's best
  line starts at 70 km/h (0.0104 per m/s: 0.912 / 0.797), one tyre, not a reason to move. Veith's trailer losses start
  from 24-32 km/h, but on 1.5-3 mm water and low-void or smooth-pad cases (96/32 peak 0.93 for a high-void tyre).
  W 0.25 (0.175 mm) at 0.979 / 0.959 lies between the 0.2 mm curves (0.958-0.99 / 0.908-0.97): the linear W stays.
  (2) Damp: no new damp-against-dry number; thin film over wet at ≤ 60 km/h 1.00-1.06, so damp sits at or just above wet
  at low speed, and wet against dry at 32-48 km/h spans 0.83 (lateral, concrete) to 1.0 (peak, textured belt). The
  0.70-0.85 band stays Bachmann's alone (secondary). (3) Hydroplaning: on 0.7 mm a new tread shows no hydroplaning to
  140 km/h (drums), so the thin film taking over the new tread's term is right. The worn drum tyre (0.6-1.6 mm) loses
  later and faster than `wet_loss`: at 1.5 mm (0.7 mm film) the model gives 0.64 / 0.44 / 0.24 / 0.05 of new at 60 / 80 /
  100 / 120 km/h against 0.85 / 0.49 / 0.24 / 0.13, and that with the model's 0.7 mm road texture added (the drum has none). The
  film dependence is weaker than the model's v_on ∝ 1/h (half loss ∝ h^-0.43; Huebner -0.26). The Contidrom fit
  (film unknown) and the game's new tread keep it a lab-only question; a refit (later onset, ramp to ~2 v_on) waits for
  a worn-tread wet test with a known film. Groove pattern moves the deep-water hydroplaning speed far more than depth
  (block +48 % over longitudinal, depth 5 → 8.5 mm +6 %): a rally tyre's block tread is not penalised by v_p. Gruber
  2000 has no damp or wet numbers (sensor signals only); Oh & Lee could not be fetched (Cloudflare blocks every host).
  Wanted: Gengenbach (diss. TH Karlsruhe 1967; ATZ 70, 1968), Bergman (TRR 621, 1977, pp. 8-18), Beauregard & McNall
  (SAE 730147, 1973), Speyer & Kelley (TRR 621, 1977, p. 99), Sabey, Williams & Lupton (SAE-FISITA 1970, cited as SAE 70036).
- Wet papers 2 (2026-11-09, `tyre/papers-wet2`, notes only; rows "roads", "lateral, trailer", "1.5 mm", "rubber
  sealing" above). (1) Damp: no tyre measurement of damp against dry beyond Bachmann. SWOV's thinnest water that wets the
  track (0.3 mm sprayed, mostly in a 0.3-0.8 mm texture) grips like 0.6 mm on a new tread (peak 1.005, lateral 1.00 at
  50 km/h, locked 0.98), and Persson puts the loss from dry (20-30 %) in the sealing of water-filled texture, which a damp
  road already has. So the loss comes at damp and damp ≈ wet at low speed: damp 0.79-0.84 of dry (1.00-1.06 of wet),
  inside Bachmann's 0.70-0.85; best number ~0.82 (moderate: one direct damp source, the rest inferred). (2) Wet against
  dry at the peak: trailer lateral 0.755-0.90 at 32 km/h (Beauregard, 3 tyres), rubber 0.70-0.80 (Persson), with Veith
  0.83-0.84 and Klempau 0.86; NGP's 0.79 sits in the band, its lower half. (3) Speed fade: SWOV new tread peak 100/50
  0.938, lateral 0.944 (thin films over texture) against the branch's 0.918 at 100 over 60: consistent. Oh & Lee
  (1.5 mm, thicker than 0.7) lose 2-5 % already from 40 to 60 km/h, 80/60 0.94-0.98 against the fade's 0.96; Beauregard's
  table is flat above 64 km/h (0.94), his Fig. 18 falls from 32 km/h (96/32 0.74-0.83, depth unstated, 1973 tyres): no
  source on a known thin film contradicts the onset at 60. Wet locked/peak 0.66 / 0.525 at 50 / 100 km/h (SWOV) meets
  TRL's 0.64-0.67 / 0.49-0.52. Oh & Lee's Fig. 6 (surface unstated, peak 1.41) falls 12 % from 50 to 100 km/h; if dry,
  it is the one source against the dry peak holding with speed (TRL 100/32 0.97-1.20). (4) Worn tread: SWOV 2 mm over
  new, peak 1.01 / 0.85 at 50 / 100 km/h, 0.98 / 0.91 at 0.3 / 0.6 mm water; `wet_loss` at 2 mm (drained 2.7 mm, h
  0.3 / 0.6 mm) gives 0.94 / 0.71 over both speeds against SWOV's 0.98 / 0.89 (inferred: sprayed depth taken as h): the
  model loses earlier, as the drums said; still lab only. Reul 2011 has dry braking only; the UMTRI bibliography names
  sources, no data. Wanted: Kulakowski & Harwood, ASTM STP 1031 (1990) pp. 50-60, doi:10.1520/STP1031-EB (locked μ against
  thin films, critical depth 0.025-0.23 mm); Cerezo, Do, Prevost & Bouteldja, Proc. IMechE J 228 (2014) 1285,
  doi:10.1177/1350650114544533 (locked, dry to wet in situ); Gengenbach, The effect of wet pavement on the performance of
  automobile tires (Karlsruhe 1967, English, UMTRI-43837; Deep Blue behind Cloudflare); Holmes & Stone, RRL LR 254 (1969);
  Wild, UM-HSRI-PF-73-31 (1973); Sakai, Kanaya & Okayama, JARI (1973).
  Dijks (TRR 621, 1977; row "Worn tread against speed and road"): worn over new at 2 mm tread, UK peak 0.99 / 0.97 / 0.83
  at 50 / 80 / 130 km/h on rough asphalt (0.5 mm), 0.97 / 0.58 at 50 / 80 on smooth mastic (1.1 mm); German lateral (1.5 mm)
  radial 0.99 / 0.94 / 0.90, cross-ply 0.92 / 0.86 / 0.74 at 40 / 60 / 80. `wet_loss` at 2 mm (inferred, the film as h,
  the model's 0.7 mm texture, none on mastic): 0.95 / 0.77 / 0.48 (0.5 mm), 0.54 / 0.19 (mastic), at h 0.7 mm 0.91 / 0.75 /
  0.59 (40 / 60 / 80): every set loses later and less than the model, so the onset v_on ∝ drained depth / h is too low for a
  worn tread (third independent set after the drums and SWOV). New tread against speed: rough asphalt peak 130/50 0.96,
  smooth mastic 0.45 (1.1 mm), German radial lateral 80/40 0.77 (1.5 mm): the fade depends on texture and film; the
  branch's 0.856 at 130 over 60 lies between. Nothing for damp or wet against dry. Wanted: the rest of Dijks (TRR 621
  pp. 141 on, the Delft trailer data); the RRL cross-ply study on six surfaces (Dijks ref. 6, not in the file).
- Wet: the partial hydroplaning rests on three worn-tread sets at known films (drum, SWOV, Dijks; 2026-11-09) and on an
  8 mm new groove depth; a rally tarmac tyre's (FIA: at least 17 % grooves of 5.5 mm) would set the onset lower (v_on
  56 m/s new at 5.5 mm, nothing lost in game). The roads lose less than the fit at 100-130 km/h (SWOV 0.6 mm, Dijks
  UK), the drum more at 100-120 km/h: no one form takes both within 0.05. The ABS stops of unstated film (Contidrom,
  Continental 2021_100) are context. No open data on a new tyre's wet peak above 100 km/h, nor on the film's depth in
  a braking test. NGP's tread cannot be set in a test run (`--wear` is the road's), so the tread rows
  are lab only. Wet on the 12 RSF stages without a wet `.mat` entry (568 among them) falls back to material 0 (μs 0.95, dry; the tarmac tyre's static 0.83, In game):
  whether the original does the same, and whether the launcher should offer wet there, is open.
- Wet-weather tyre against dry tyre (2026-11-09, `research/wet-tyre`, notes only; rows "Wet-weather tread", "Tread
  depth against film", "Void and siping", "Compound", "Tread temperature", "Road tyres' wet grip spread", "Rally tarmac
  tread rules" above). No measurement of a rally (or race) wet tyre against the dry one exists in open sources; the
  same-tyre comparisons are road and test tyres at 1-2 mm. (1) On a wet road at ≤ 50 km/h more groove void costs
  grip (Veith, same compound: 40 % against 20 % area 0.89-0.96 at 32-48 km/h; walking pace 0.75 for φV doubled) and
  depth is neutral (0.95-1.0); what a wet tyre gains there comes from siping (+16-32 % at 1 mm, +22 % damp, Löwer) and
  a higher-hysteresis, colder-working compound (+4-11 % among practical treads, Eckert; 0.6 → 0.8 on damp, Michelin;
  +21 % wet μ from 60 to 30 °C, Persson). NGP's flat +8 % sits inside that, its source unstated. (2) With speed the
  open pattern's gain grows where the road drains badly or the water is deep: 1.10 / 1.35 at 80 / 96 km/h on smooth
  concrete (Veith, ~1.5 mm), 1.08 / 1.14 at 120 / 140 on Safety-Walk at 1 mm (Löwer, 35 against 25 % void); on
  textured asphalt it is flat, 1.09-1.10 at 80-120 km/h at 1 mm (Löwer), 1.0 at 80-96 (Veith rough). Half the
  tread depth at 96 km/h keeps 0.86 of full on 0.12 mm and ~0.65 on 0.7-0.9 mm water (Veith Fig. 2, low texture). On
  the game's road (MPD 0.7 mm, 0.7 mm film) the data put the wet tyre's speed gain between none and the smooth
  roads': a few % at 100 km/h, about 5-10 % at 150 (inferred). NGP's +8 % flat with speed is the textured-asphalt
  case. (3) On a dry road no open test compares a rain tyre with the dry one of the same maker; Veith's walking-pace
  cornering (0.75 for φV doubled, "the same wet or dry in a relative sense") and the softer siped blocks (Michelin)
  bound NGP's -7.5 % (0.925) from below, not above: the loss could be larger, unmeasured. (4) Damp: damp grips at or
  below 1 mm on every pattern (0.92-1.00 at 120 km/h, Löwer), siping helps on damp (+22 %), the compound spread is
  measured on damp (Michelin); a thin film's drainage term is small (W 0.25). So the data do not support NGP's damp
  tyre gaining nothing on damp, but what it should gain is a level (compound, siping: the grip level, a decision),
  not drainage. Proposal (not implemented): give the tyre type a groove volume φV (area × depth / 10 mm) that NGP's
  tyre types do not carry, by NGP's design wetness: dry 0.126 (21 % at 6 mm: FIA 17-21 %, Michelin H5/S5 6 mm), wet
  0.21 (30 % at 7 mm: FW2 > 27.5 %, 7 mm; FIA autocross > 25 % is "wet"), damp between (0.16, ours); scale the thin
  film's loss (`THIN_FILM_LOSS`, fitted to Reimpell's road tyre, kept for the dry tyre) by (φV_dry / φV)^k, k 0.5
  (between Löwer's textured asphalt, k ≈ 0, and Veith's rough road, 0.94; Veith smooth 1.6, Safety-Walk 2.4; ours).
  `partial_lift` cannot carry it: with the tyre type's groove depth (6 / 7 mm instead of 8) a new tread's onset on
  the game's 0.7 mm film is 217 / 250 km/h, nothing below 150 km/h (only a worn tread would differ). Numbers by the
  model's formulas (`thin_film` at W 1 on NGP's levels, over the dry tyre on a dry road; new tread; not the lab):
  dry tyre 0.790 / 0.725 / 0.644, wet tyre 0.850 / 0.796 / 0.728 at 50 / 100 / 150 km/h, wet over dry 1.076 /
  1.098 / 1.131 (k 1: 1.076 / 1.115 / 1.174; today 1.076 flat). The damp tyre on damp gains ≤ 0.5 % by it at 150
  km/h. Calibrating instead to Reimpell's road tyre as φV_ref (~0.24 at 30 % and 8 mm, assumed) would make today's
  dry tyre lose more (0.887 / 0.745 of 60 km/h at 100 / 150 against 0.918 / 0.815) without rally data: not proposed.
  Weak: φV per tyre type is from rules and one maker's text, k from two drum and trailer tests on other roads.
  Wanted: a wet against dry rally or race tyre test at a known film; Veith & Pottinger, "Tire wet traction:
  operational severity and its influence on performance", The Physics of Tire Traction (Plenum, 1974), doi
  10.1007/978-1-4757-1370-1_2 (paywalled: open against closed patterns at more speeds and depths). Heinz & Grosch
  (Rubber Chem. Technol. 80 (2007) 580-607) since read: T61's entry below.
- Damp tarmac tyre (2026-11-09, T61, `tyre/damp-tyre`, a user decision, experimental-brush only): NGP's damp tarmac
  tyre (kind 1, base 1, slope -0.075, design wetness 0.5 on class 2) ties the dry tyre on damp (both 0.98125) and is
  never the best choice. On the tarmac class's share its factor is now the dry tyre's times R(W)
  (`brush::damp_tyre_ratio`, linear through 0.98125 dry, 1/0.925 damp, 0.94375/0.925 wet; `damp_tyre_level` over NGP's
  factor); the tyre type's surface words come on `rbr_tyre::Wheel` (`class_friction`, `class_wet_slope`,
  `design_wetness`, read in `rbr-standalone`'s `physics/layout.rs`), the tyre told by design wetness 0.5 with tarmac its
  best class (kind 4, the damp gravel tyre, has gravel best). Lab (`scorecard` on the track-41_M probe, `--tyre 1`,
  wheel 0, 50 km/h, peak μ_y with against without): dry 1.019, damp 1.080, wet 0.981 (factors 1.0195 / 1.0811 /
  0.9805): against the dry tyre on a dry road dry 0.981, damp 0.886 (0.82), wet 0.802 (0.818). Brake test, Fiesta
  Rally2, track-41_M from 50 km/h declutched, `--tyre 1`, locked / best threshold g: dry 1.058 / 1.230 (was 1.041 / 1.212),
  damp 0.935 / 1.134 (was 0.874 / 1.071),
  wet 0.758 / 1.040 (was 0.773 / 1.061) (`--tyre 0` damp 0.872 /
  1.066, `--tyre 2` 0.843 / 1.036: the damp tyre now best on damp). RSF 568 dry 1.063 / 1.264 (was 1.042 / 1.242); its
  damp and wet are the class-0 fallback (no change). ngp, experimental, brush `--tyre 0` / `2` traces bit-equal,
  scorecard unchanged. Heinz & Grosch (row above) give compound alone 95-120 % on wet roads, most within ±8 %: the
  +8 % is inside, though their data are wet traction, not damp.
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
  0.77-0.88 (its sliding-speed fall, fitted on tarmac, applies on ice too); rally studs (no data).
- Loose levels (2026-11-08, [Loose levels](#loose-levels-rally-data-against-the-game-2026-11-08)): gravel and snow stay NGP's; snow's berm is 1.05 (done); a driven fall on gravel is in a drive test (no open gravel curve reaches full spin, 2026-11-09); the RWD launch on 116 is NGP's (velocity clear under 0.046 g from rest, free-wheel latch drag 0.086 Fz under 2 m/s; not the tyre). Wet gravel's locked/ABS 1.89 and circle 0.63 are the test's fixed K, the berm's full lock and the circle read's first slide (Dry, damp, wet gravel in game); wanted: wet or damp gravel braking data, a gravel traction curve to full spin.
- Loose surfaces unmeasured: combined slip on gravel and snow (soft soil only, ibid.), and the soil's lateral force above 25 m/s through the whole car. NGP has no ploughing (the declutched brake
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
