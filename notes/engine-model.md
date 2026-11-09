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
- WRC boost cap 2.5 bar abs from 2011 [FIA 255A]; 4.0 bar before (guess: forum reports of 3–4 bar [NASIOC WRC],
  [passionford], both unreachable at the 2026-11 audit).

## Air path (`gas.rs`, `engine.rs`)

- Orifice flow with Ψ choked below (2/(γ+1))^(γ/(γ−1)) = 0.528 at γ 1.4 (generic; [NASA] gives the choked flow,
  [Andersson] the Ψ); Ψ linear from Π 0.98 to 1 (inferred:
  a finite slope near equal pressures).
- Plate area 1 − cos θ/cos 7°, θ = 7° + 83°·lift [US 6591667]. Its flow bench gives C_D 0.70–0.77 at 0–25° from
  closed and a leak of about 5 mm² (60 mm bore); C_D 0.8 and no leak about cancel at light pedal.
- Torque from MAP at part load checked against [EPA Ford 1.6]: the 2013 1.6 EcoBoost makes 27 % of its 100 kPa torque
  at 50 kPa and none at 24–26 kPa (2500 rpm); the na sample 28 % at 47 kPa.
- C_D 0.80 one throttle (inferred: fits are about 0.8 at WOT [C_D estimation]); 0.85 per cylinder (guess); at least
  45 mm per cylinder (guess).
- Manifold 0.8·V_d one throttle, 0.3·V_d per cylinder, 1.0·V_d turbo, boost pipe 2.0·V_d (guess: ITBs have only
  their runners after the plate (inferred); Rally2 caps the restrictor-to-plate volume at 20 L [FIA 261]).
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
- DBW: plate aim = pedal, then the ETB, 30 ms first order and 40 ms full travel (guess; [Gorzelic] model the throttle
  as a 40 ms first-order lag, "taken at 40 msec", an assumption). Race ECUs set the plate
  from the pedal through a table: Link over pedal and rpm [Link e-throttle], Haltech [Haltech DBW]; Marelli's
  SRG-48x drives a DBW motor [Marelli]; [MoTeC DBW] and [Life Racing] (M-Sport R5) were unreachable at the 2026-11
  audit. Torque requests are road (ME7) and F1 [Bosch ME7], [F1 torque map]. The 1:1 start is ours (guess: neither
  source shows a default). MoTeC M1 aims at a share of airflow through its throttle area table [HPA M1 DBW], which
  opens the plate more than 1:1 at small openings (inferred). No rally map is public. HPA 138's 40 %
  pedal → 30 % throttle (pedal^1.3, used before) is a Haltech street 350Z tuned to hold a steady speed [HPA 138]; with
  it the DBW samples gave about half BeamNG's power at 10–20 % pedal.
- The idle air adds to the aim; anti-lag's kick overrides it (inferred; [MoTeC DBW] unreachable). Blips and shift cuts
  come through the interface's throttle (NGP's control unit).
- Part throttle follows WOT to an rpm then falls at roughly constant power, as BeamNG's model [BeamNG throttle].
  Most net power from IdleRPM up at pedal 10/20/30/50 %, % of the WOT's: na 9/23/43/80, turbo 8/20/47/87, old
  5/18/43/91, cable_turbo 3/11/21/75 (BeamNG 2023, its power plot: 9/25/43/69). Torque, % of MaxTorque: na
  40/53/65/86, turbo 27/47/63/91, old 26/46/63/93, cable_turbo 13/25/37/87 (BeamNG 49/65/76/91).
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
- η_ign = max(0, 1 − 2.8e-4·Δθ²): about 1 % at 6° from MBT [MIT 2.61], quadratic [US 6363317].
- λ 0.88 (guess, used at every load): [x-engineer] gives λ 0.86 for maximum power; the road 1.6 EcoBoost runs
  λ 0.95–1.00 at full load and 1.00 at part load [EPA Ford 1.6]. It enters the boost target and the exhaust's flow
  and heat, not the torque (K absorbs it). η_ig 0.38 for the boost target (weak) [Chegg]; LHV 43.4 MJ/kg, AFR 14.7
  (generic).

## Friction and pumping (`friction.rs`)

- Sandoval–Heywood FMEP terms, DOHC buckets, the default bearing ratios [Sandoval], [ESA.NET]; the worked example is
  reproduced within 4 % (test).
- Oil: φ = √(μ(T)/μ(363 K)), Vogel μ = 0.00862·exp(2040/(T − 89.6)) mPa·s, fitted to 10W-40 [Anton Paar]. With 20 °C oil
  the model's FMEP is 1.6× warm at 1000 rpm to 2.4× at 7000 (φ 3.6 on the hydrodynamic terms); [cold friction]: up
  to 2.5× at about 20 °C (after Will et al.), and FMEP ∝ μ^n, n 0.19–0.24 for gasoline engines (1.6–1.9× for this
  oil). In game the 1.02 sim's engine runs at 358–363 K, so this rarely acts.
- Pumping (p_em − p_im) + 0.2 bar·(p_im/p_a)·(n/n_lim)² (generic form [TAMU], [Moretto]; 0.2 bar guess in the
  report's 0.1–0.3). NA p_em = p_a + 0.15 bar·(W/W_rated)² (guess).
- s_f: closed plate, fuel cut, steady at RPMLimit gives −BrakeCoefficient·MaxTorque, clamped 0.6–2.0 (inferred:
  NGP's one braking point; friction models need a scale [ecu-lab]). The scale is 1 + (s_f − 1)·(n/n_lim)², so s_f at
  the limit and Sandoval's own FMEP at low speed (inferred: Sandoval at s_f 1 meets the road engines' rubbing
  friction from their fuel-cut points, 0.5–0.7 bar at 2000 rpm [EPA], and NGP's NA braking line gives −1.09 bar at
  2000 on the na sample; the share over Sandoval growing with n² is ours). With one flat scale the ECU samples braked
  1.4–1.7× the road engines below 5000 rpm.
- BrakeCoefficient is a share of MaxTorque, so it overstates braking on boosted engines: every installed turbo car
  clamps (raw 2.3–4.9), NA cars are 1.32–1.87 (`tests/rsf_cars.rs`). The clamp's 2.0 is a guess and sets the friction
  of every installed turbo car; through the identity it also raises their K.
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
  held over idle: before, the Fiesta Rally2 pulled away at a closed pedal crept at 1262–1397 rpm, idle 1200, now 1205).
  Tested: after 3 s at 80 % of idle, 10 s at idle + 150 rpm brings the idle air under its feed-forward.
- Over idle the loop closes the air down to a floor, the air that holds 0.1·p_a at that speed (sized as the overrun
  air), not to 0 (road engines' lowest fired loads 9.2–34 kPa [EPA]; race ECUs hold a throttle aim minimum on overrun
  [MoTeC FK8R]; 0.1 is ours, under every sample's idle need). Before, the manifold fell to 4.5–8 kPa at idle + 50 to
  + 700 rpm with the fuel on; now 12.6–13.1 kPa at the lowest (Scorecard). A blip's fall back within 50 rpm of idle
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
- Anti-lag: armed above 0.45·RPMLimit, active with pedal < 0.1 for at most 3 s, re-armed above 0.3 (guess). Link
  enables it above an rpm and a throttle and holds it for a deactivation time after both fall under them
  [Link ALS manual], 5 s typical [HPA Link ALS]; [MoTeC ALS] was unreachable at the 2026-11 audit. Kick 20 % of the
  throttle area: a Link user's about 20 % [Link ALS forum]; [Link e-throttle ALS] (about 20 %, 25 % in the
  closed-pedal cells) was unreachable at the audit. As an area (inferred: a 15 % lift opens 5 % of the area, and the
  turbo ran down). Retard 60° (guess): Link's retard table is an offset, −25 to −50° typical [Link retard]; the same
  user ran about −50°. Fuel +20 %, cut 30 % (guess): that user ran a cut of about 60 and fuel 0–2 % [Link ALS forum];
  others keep up to 40–80 % of the fuel limit on the lowest throttle row [HPA ALS]. Boost aim 1.2 bar gauge (guess).
  60 % of the cut firings' fuel burns in the manifold (guess), and the retard's lost work heats the exhaust as at idle
  (Turbo); the exhaust sits at the 1150 °C cap within 1 s. The BOV stays shut (inferred: ALS needs the charge).

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
  32 mm the same years [Appendix J], 33 mm from 2010 [Impreza N2010], [FIA 254]; WRC 1.6 33 mm, 36 mm from 2017
  [FIA 255A], [World Rally Car]; Rally2 32 mm [FIA 261]. [rallycars restrictors]' 38/36/34 mm (1992/1994/1995) mixes
  Group A's 38 and 34 with Group N's 36.
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
- Shaft inertia 1e-5 kg·m² [Chalmers] (their T45: 1.16e-5), [GT1548]. Losses 0.002ω + 2.5e-6ω² + 100 W, our reading
  of rF2's `TurboFriction=(0.002,2.5)`, "Torque (Nm, constant at any speed), power (W, ramps up linearly with speed)",
  and `TurboStaticFrictionWatts=100`, which "prevents numerical problems at low spool speeds" [rF2 engine.ini]. The
  static 100 W stops the shaft at idle and at a closed pedal (ω_t 0 on both turbo samples).
- Exhaust temperature affine in flow for the firings that burn, 520 °C at 0 and 950 °C rated, capped at 1150 °C
  (MoTeC's anti-lag limits; [MoTeC ALS] unreachable at the audit), τ 0.3 s (guess). The 520 °C is the intercept of a
  line through EPA's four road turbos' 849 fired points (940 °C at the most flow, rms 85 K; turbine inlet against
  exhaust flow over the engine's most) [EPA]; 950 °C rated is a guess. Firings that do not burn (the fuel cut, cut
  events) leave the air at 300 °C (EPA's fuel-cut points at the turbine 199–422 °C), their fuel burning in the
  manifold at anti-lag's 60 %. Half the work the retard takes heats the exhaust: (1 − η_ign)·K·W_cyl·0.5 (inferred:
  EPA's Ford 1.6 and 2.7 retarded 27–44° at their lightest loads, 1000–1750 rpm, ran 70–142 K over their neighbours
  near MBT, 0.23–0.63 of that work). Before (700 °C at 0, the anti-lag term on the retard, no motored temperature)
  the turbo samples idled at 1035 °C and ran 707 °C under the fuel cut; now 598–661 and 301. With 400 °C an older
  model (no residual gas, before the retarget) gave the steady WOT boost two branches; 520 °C passes the path test.
  The shaft's spin-down after a lift without anti-lag (4500 rpm fixed, from WOT: stopped within 0.8 s) is set by the
  compressor's work and the losses, the same before and after; no measured spin-down was found.
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
- Spool, the [Chalmers] case (GT-Power): at a fixed speed from the steady pedal for 5 bar BMEP to 90 % of the steady
  WOT torque, at 1000 rpm Volvo's 2.0 L four (its turbo not given) takes 0.54 s, their 1.5 L three with a T45
  (1.157e-5 kg·m²) 1.00 s, and 0.68 s at 0.050 kg·cm² (Table 17). Tested
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

## Against measured engines

EPA's steady maps of nine road engines [EPA] (2026-11, `/tmp` analysis, not kept) against the samples, warm, steady at
a fixed speed (a probe over `rbr_engine`, not kept). Road engines run λ 1 and EGR and are not race engines; they
bound the physics, not the calibration.

- BMEP at 100 kPa MAP, 1500 / 2500 rpm: road turbos 8.7–11.0 / 7.6–9.3 bar (Ford 1.6 8.68 / 9.26, Ford 2.7 8.92 /
  7.62, Honda 1.5 10.95 / 8.07, Mazda 2.5T 9.20 / 8.88); turbo sample 15.5 / 14.0, cable_turbo 9.8 / 12.2. At
  90 kPa, 2500 rpm: road NA 7.8–9.3 (Chevy 2.5 7.79, Mazda 2.0 9.12, Mazda 2.5 9.32); na sample 8.1. The turbo
  sample's K (η_ig ≈ 0.57) gives 1.4–1.8× the road turbos' torque at 100 kPa. The table asks for a high K: a Rally2's
  432 N·m from 1620 cc is 33.5 bar BMEP, at least 13.4 bar per bar at the 2.5 bar abs cap [Škoda Rally2 engine],
  [FIA 261].
- Share of the 100 kPa torque at 50 kPa, 2500 rpm: road turbos 27 % (Honda 42 %); turbo sample 39 %, na 33 %. Zero
  torque at 18.5–27.2 kPa on the road engines; turbo sample 20.6, na 25.2 kPa.
- Warm idle MAP: Toyota 2.5 22.3 kPa (777 rpm), Mazda 2.5 23.4–25.6 (670–700), Mazda 2.0 28.5 (767); samples na
  33.2, old 27.1, cable_turbo 30.0, turbo 20.3; RSF modern turbos 20–22. Idle spark −1.9 to 8.9° BTDC on those
  three.
- Closed pedal with the fuel cut, BMEP at 2000 / 4000 / 5000 rpm: road −1.1 to −1.5 / −1.7 to −2.0 / −2.2 to −2.5
  bar at 16–43 kPa MAP (Chevy 2.5, Honda 1.5T, Mazda 2.5, Mazda 2.5T, Toyota 2.5); ECU samples na −2.17 / −2.84,
  turbo −2.43 / −3.23 / −3.67, cable_turbo −2.39 / −3.22, so 1.4–1.7× (s_f, from NGP's braking at the limit,
  1.3–2.0). old (fixed stop, fuelled) −1.11 / −2.17. EPA's points stop at 5000–5760 rpm, NGP's is at the limit.
- Exhaust (turbine inlet on turbos, ports on NA): idle 317–430 °C (NA), 470–515 °C at the Ford 1.6's lightest loads
  at 1000 rpm, 199–266 °C (Honda) and 329–422 °C (Mazda 2.5T) under the fuel cut; the turbo samples 1035–1037 °C at
  idle and 705–715 °C under the fuel cut. Part load matches: turbo sample 727 °C at pedal 0.15, 1500 rpm (163 N·m),
  the Ford 1.6 732–783 °C near that load. Retard heats the exhaust: the Ford 1.6 at 2500 rpm light load
  620–680 °C, retarded 720–854 °C (about +180 K for 27°). The model's unburnt term (anti-lag's, which takes the idle
  retard too) adds about +330 K at idle for 27°, on a 700 °C intercept.
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
group and year. The Rally2 rows are the Fiesta Rally2 (`Ford_Fiesta_Rally2_ngp7`).

## In game

Numbers in `docs/experimental-physics.md` ("In game"), `b97695d86`, experimental = realistic + NGP's tyre (the brush
is not released, user 2026-11-07), so every gap is the engine's. Tools `run-stage-compare.sh`, `cmp2.py` and
`run-standing-start.sh` in the evidence archive; 0-100 by `harness/car_pace.py` from 0.5 m/s. The same runs with the
brush (main `0ef55141`) are in `notes/tyre-model.md` ("In game").

- track-11_O (gravel) 600 s, four cars: the same as on `0ef55141` to the 0.01 s (the brush is off on gravel). Exp
  1.6–4.2 % sooner to split 2.
- track-41_M (tarmac): exp 1.6–3.7 % sooner to split 2, finishes Fiesta 309.01 / 304.57, Celica 310.91 / 305.17, Golf
  324.24 / 314.57, Fiat 323.35 / 315.49 s. The Golf II GTI no longer stalls on the opening climb (that stall was the
  brush's, since fixed). It stalls after the finish: full brake in fifth on the run-out, wheels locked at 16 m/s, the
  clutch-engaged speed tracking the engine 464 → 0 rad/s in 0.8 s (trace column `locked`); NGP's Golf in the same
  stop keeps 89 rad/s with the clutch open. Open.
- Fiesta Rally2 0-100 km/h: RSF 504 tarmac +6.7 % NGP 4.52 / exp 4.46 s (4.44 at `96c85981`; brush 4.33, 4.35
  after its fix); track-64 gravel flat 4.66 / 4.63; RSF 345 gravel −4 % 4.50 / 4.44. Slope-corrected 504 (constant
  acceleration, −0.66 m/s²) ≈ 4.0 s (inferred).
- A launch control in the autopilot (hold, slip-held throttle, torque-crossing upshifts at 6386–6587 rpm against
  `SHIFT_UP` 6494; removed) was no faster: 504 4.59 / 4.48, track-64 4.63 / 4.62, 345 4.47 / 4.45 (on `96c85981`).
  The tyres pull as hard at 9 m/s of slip as near the peak at launch speeds (504: 10 m/s² while the wheels spin
  9 m/s over the car); cutting throttle on slip bogged the turbo to 130 rad/s off boost.
- Sourced figure: Volkswagen's claim 4.1 s for the Polo GTI R5 (Rally2 class, 1230 kg); no measured Rally2 figure
  found.

## Open

- A wheel locked under braking with the clutch in stalls the realistic engine: the Golf II GTI after track-41_M's
  finish, full brake in fifth from 24 m/s, fronts locked at 16-20 m/s, engine 464 → 0 rad/s in 0.8 s with the
  clutch-engaged speed tracking it (with NGP's tyre at `b97695d86` and with the brush on main; NGP's physics locks
  them too and idles at 89 rad/s with the clutch open); NGP's clutch help opens at 65 rad/s, too late. Not traced why
  it never opens with the model's engine. The Golf's stall on the climb was the brush's (fixed, `tyre-model.md`).
- The rally turbos make almost no boost at 1000 rpm, so the [Chalmers] 1000 rpm spool case does not apply to them.
- Modern turbos idle at 20–22 kPa, left so (below). Idle MAP with the 27° reserve and the residual gas, against
  warm road idles of 22.3–28.5 kPa [EPA] and tuners' 27, 31 and 30–50 kPa [MOTOR], [Gears], [Tuning School]: samples
  na 33.2, old 27.1, cable_turbo 30.0, turbo 20.3 kPa; RSF modern turbos (Rally2, R5, WRC) 20–22,
  Group N and R4 22–23, Rally3/4 and R1 25–31, Group A, B, 4 and S1600/S2000/R2/R3 23–34. K dominates on the modern
  turbos: 1.6–1.7e6 J/kg at idle (η_ig ≈ 0.57 of the stoichiometric 2.95e6) against 0.9–1.4e6 elsewhere; friction
  (17 N·m, s_f clamped to 2.0) and pumping (10 N·m) are like the others'. At η_ig 0.38 they would idle near 28–30 kPa
  (estimate).
  The retarget fix leaves it (turbo sample 20.3 kPa before and after: idle sits in the held range, whose K is the
  on-boost median). A part-load K that keeps the table: K(n, x) = K_wot(n)·f(x), x = m_charge / m_charge,wot(n) at
  the same rpm, f(1) = 1 (so the identity at WOT, and the hold, are untouched), f falling to about 0.65 at idle
  load (η_ig 0.57 → 0.38, guess). The idle air, the idle gains, the closed-pedal brake and the DBW part-pedal tests are
  calibrated after K, so they would follow; aim_open and gate_base read WOT runs only. Not done (nothing reads idle
  MAP; it would lower part-pedal torque everywhere, a feel change).
- Closed pedal below idle at a fixed speed, N·m, before → after the anti-stall fade: turbo sample 168 → 51 at 500
  rpm, 139 → 93 at 750, 110 at 1000; na 65 → 27 at 500; Fiesta Rally2 165 → 51, 136 → 91, 107; Fiesta R2 53 → 22 at
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
- The spark loop saturates (gain 0.49–0.65°/rpm on the samples, `spark_kp` over IdleRPM).
- No rally pedal map or measured part-throttle map of a race engine is public; the DBW 1:1 is a guess.
- Exhaust (Scorecard): the turbo samples idle at 661 °C against road idles of 296–430 °C at the ports and the Ford
  1.6's 593–657 °C retarded at its lightest loads, and run 564 °C at pedal 0.15, 1500 rpm against 585–732 °C at the
  same BMEP on four road turbos. One line over the flow share cannot hold both: a rally engine's rated flow is about
  2.5 bar's, so a given load is a smaller share of it than on a road engine (inferred).
- Closed-pedal braking at 2000–5000 rpm still runs 0.2–0.4 bar over the road engines' with the fuel cut (Scorecard):
  the pumping at the 0.2·p_a overrun air is 0.8 bar, and Sandoval's FMEP for a 1.6 L four is over the road engines'
  0.5–0.7 bar at 2000 rpm. The limit point is NGP's, which no measured race engine checks.

## Sources

- Volkswagen, Polo GTI R5 0-100 km/h 4.1 s, 1230 kg (maker's claim): https://www.volkswagen-newsroom.com/en/the-new-polo-gti-driving-presentation-2512/the-new-polo-gti-r5-gti-goes-rallying-2567
- SAE 2018-01-1423 (EPA/SwRI, PSA EP6CDTX 1.6 THP full load, public domain): https://www.epa.gov/sites/default/files/2018-10/documents/sae-paper-2018-01-1423.pdf
- Ford 1.6 EcoBoost, ratings: https://fordauthority.com/fmc/ford-motor-company-engines/ford-ecoboost-family/ford-1-6l-ecoboost-engine/
- Hyundai i20 N, spec sheet: https://www.hyundai.com/content/dam/hyundai/au/en/models/i20-n/2021/images/i20N_Spec-Table_Oct_2021.pdf
- Škoda Rally2 engine (Fabia RS Rally2): https://cdn.skoda-storyboard.com/2022/06/FABIA_RS_Rally2_Engine.pdf

- Andersson, LiU MSc 2017 (MVEM, orifice, turbo): https://www.diva-portal.org/smash/get/diva2:1138994/FULLTEXT01.pdf
- Hendricks & Sorenson MVEM: https://orbit.dtu.dk/en/publications/mean-value-si-engine-model-for-control-studies
- Moraal & Kolmanovsky, turbo modelling (SAE 1999-01-0908): https://www.researchgate.net/publication/244520704_Turbocharger_Modeling_for_Automotive_Control_Applications
- NASA, choked flow: https://www.grc.nasa.gov/www/k-12/airplane/mflchk.html
- US 6591667, plate area: https://patents.google.com/patent/US6591667
- C_D estimation: https://www.researchgate.net/publication/224181168_Estimation_of_automotive_engine_parameters_Part_I_Discharge_coefficient_of_throttle_body
- HPA Alpha-N (ITBs): https://www.hpacademy.com/forum/understanding-afr/show/alpha-n-20/
- Sandoval (SAE 2003-01-0725): https://saemobilus.sae.org/papers/improved-friction-model-spark-ignition-engines-2003-01-0725
- ESA.NET FrictionModel.cs (Sandoval eqs. 7.b–11): https://github.com/pangtuwi/ESA.NET/pull/223
- ecu-lab, friction scale: https://github.com/DNiev/ecu-lab/pull/138
- Anton Paar, oil viscosity: https://wiki.anton-paar.com/us-en/engine-oil/
- cold friction: Roberts, Brooks & Shipway 2014, Internal combustion engine cold-start efficiency: a review, Energy
  Conversion and Management 82, doi:10.1016/j.enconman.2014.03.002 (open access):
  https://www.sciencedirect.com/science/article/pii/S0196890414001939
- TAMU, pumping: https://oaktrust.library.tamu.edu/server/api/core/bitstreams/161c947f-2eff-48dd-8bf4-80912a8bec75/content
- Moretto 2024, Willans: https://www.dora.lib4ri.ch/empa/dload/empa:36100/PDF/Moretto-2024-A_method_to_quantify_the-(published_version).pdf
- Guzzella & Onder: https://link.springer.com/chapter/10.1007/978-3-642-10775-7_2
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
