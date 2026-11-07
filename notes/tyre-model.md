# Tyre model research (ours, experimental)

Whether a more current tyre model can improve on NGP's, tarmac first. Research and lab only: nothing here is wired
into the game, and NGP's tyre stays the bit-exact default. Tools: `rbr-standalone --tyre-probe FILE` (read-only frame
dumps on `--autopilot` runs) and `crates/rbr-tyre-lab` (`sweep`, `materials`; README). Probe dumps, images and CSVs
stay in `/tmp/tyre-lab`, never in git.

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
  (17) 0.90/0.756 over soil 0.855/0.664, 9 mm (SinkFactor 0: no soil solve). Wet on the dry tyre: 1.105 × wet slope
  0.925 = 0.79 of dry, at any speed.
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

Read in full unless marked. Sources are cited once per row.

| Effect | Target | Source |
| --- | --- | --- |
| Peak μ against load | μ ∝ Fz^-0.13 (1.30 at 2 kN to 1.05 at 10 kN, read from a figure) | Singh & Sivaramakrishnan (Goodyear), arXiv 2305.18422, Figs. 15-16 |
| Cα against load | ∝ Fz^~0.6 (read from a figure) | same |
| Cα: pressure / tread / heat | +10 % for +20 % pressure; +30 % at 60 % less tread; -20-25 % cold to hot (bulk temperature) | same, Table 3 |
| Peak μ: pressure / tread / heat | small; +10 % at 60 % less tread (dry); -10 % cold to hot (surface temperature) | same, Table 3 |
| Grip against temperature | passenger tyre 0.7 to 1.05 warming, optimum 88 °C, width 50 °C | Vera et al., Sensors 22:6380 (2022) |
| Which temperature | grip follows the tread core, stiffness the carcass | Farroni et al., Appl. Sci. 10:1604 (2020) (TRT); contradicts Goodyear's surface-for-μ; both bound NGP's 0.6/0.4 blend |
| Relaxation length | σ_y = -0.14 + 0.021 V + 1.9e-4 Fz - 1.6e-8 Fz² m: 0.3-0.9 m, linear in speed | Vera et al. (2022), passenger tyre, ≤2 Hz |
| Sliding over peak μ | 0.7-0.85 dry | textbook (Pacejka 2012), not re-read |
| Peak slip | 4-8°, 0.05-0.15 | textbook, not re-read |
| Trail at small slip | a/3 (half contact length a) | brush theory (Pacejka 2012) |
| Convection | Nu ∝ Re^0.8 (NGP 0.78) | West & Limebeer, VSD 60 (2022) |
| Wet against speed | μ falls with speed and film; steepest under ~1 mm | Wiese et al., Vehicles 3:6 (2021); Zhao et al. IJTST 2024 (abstract only) |
| Hydroplaning speed | v_p = 6.36 √p km/h (p in kPa) | NASA (Horne), checked in Zhang et al., Lubricants 13:76 (2025) |
| Rubber friction | no static/kinetic pair: μ(v, T) with flash temperature | Persson, arXiv 1007.2713, cond-mat/0605273; Grosch 1963 (secondary) |

No open data for rally tyres, gravel cut onto tarmac or damp tarmac: those stay calibrated on stage times, not papers.

## NGP against the targets

- Matches: peak-μ load sensitivity (Fz^0.878 against ~0.87), trail a/3, convection exponent, a temperature bell.
- Off: Cα load exponent 0.36 (0.5-0.65); stiffness blind to temperature, pressure apart, and tread depth; dry wear
  lowers grip (tread loss raises it); wet grip fixed per surface (no speed or film effect); static/kinetic pair;
  peak slip angle 9.5° at 4 kN (textbook 4-8°, secondary); lateral transient 0.04-0.10 m to 63 %, shrinking with
  speed, against 0.3-0.9 m growing with speed on a passenger tyre (a rally tyre's stiffer sidewall shortens it; no
  rally data); the sticking model's 360 Hz alternation; tread warm-up of ~8 K in 20 s of hard cornering.

## Candidate (brush-v0, lab only)

`rbr_tyre_lab::brush` (`calibrate`, `examples/compare.rs`). Pacejka's brush with combined slip (ch. 3) and
single-contact-point transient (ch. 7); stiffness ∝ Fz^0.6 times bulk temperature (-0.45 %/K), tread depth
(2.6/(d + 1.6)) and NGP's pressure factor; μ ∝ Fz^-0.13, falling with sliding speed (μ_k + (μ_s - μ_k)/(1 +
(Vs/5)²)); NGP's temperature grip kept; dry wear +10 % at 60 % worn, fading with wetness; wet loss
h/(h + 0.5 mm) · min(1, (V/v_p)²) · 0.6 with h = W · 1 mm and v_p from the hydroplaning speed times (0.6 + 0.4 d);
σ = C/k + 0.021 V. Calibrated at run time to NGP at 4 kN, 15 m/s (no NGP values in code). Below 2 m/s not modelled.
Ours, not from papers: the sliding-speed law and its 5 m/s, the wet film scale and weights, the tread term of v_p,
the transfer of the passenger tyre's σ speed slope to a rally tyre, the longitudinal load exponent.

Against NGP on the Fiesta Rally2 (`compare` summary):

- Meets by construction: Cα ∝ Fz^0.64; Cα -21 % cold to hot and +27 % at 60 % worn; dry peak μ +12 % at 60 % worn;
  peak Fy ∝ Fz^0.845; σ_y 0.31/0.41/0.62/0.83 m at 5/10/20/30 m/s (step steer 63 % at 0.36/0.56/0.75 m against NGP's
  ~0.08); slide over peak 0.80 at 30°.
- New behaviour: peak μ_y falls with speed (1.306/1.265/1.224/1.152 at 10/15/20/30 m/s, NGP flat); wet W = 1 peak
  0.98/0.76/0.60 at 10/20/30 m/s against NGP's flat 1.00 (magnitudes ours).
- Misses: peak slip ratio 0.18-0.22 (textbook 0.05-0.15, NGP 0.12-0.16): the brush peaks at 3μFz/C, so Cx
  calibrated to NGP's traction stiffness is too soft; braking peaks 0-17 % above NGP's (theoretical slip over |ωR|);
  locked over peak μ_x at 10 m/s 0.695 dry, 0.651 wet (NGP 0.78/0.75; Kebbati et al., Sustainability 15:6945 (2023):
  dry ≤ 0.885, wet 0.69): the sliding-speed drop is too steep; trail 0 past saturation (NGP 0.004 m).

## Open

- Whether brush-v0 drives better needs it on the car: a hook like the engine model's (`--engine-model`), user
  decision. The 4-8× longer lateral transient is the largest change a driver would feel, and its evidence is a
  passenger tyre.
- Before that: Cx from the peak slip ratio instead of NGP's traction stiffness; a gentler sliding-speed drop (the
  locked-wheel ratio); low speed (keep NGP's sticking model or a bristle state, which would also remove NGP's 360 Hz
  alternation).
