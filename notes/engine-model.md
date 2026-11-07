# Engine model (ours, experimental)

`crates/rbr-engine`'s `realistic` model: a mean-value SI engine that replaces NGP's torque formula, calibrated per car
on its NGP WOT table. Not bit-exact by design; it never runs in the faithful physics. Formulas and constants are in
the doc comments. Each line gives its evidence: a source (list at the end), or "inferred" with the reason. Tags:
**generic** (textbook engine physics), **guess** (a number with no source; the gaps).

## Rules

- f64 inside, f32 at the interface; `libm` for every transcendental; no allocation, hashing, time or randomness per
  step (inferred: bit-identical on every platform). `tests/model.rs` checks two runs give the same bits and any input
  (NaN, infinities, 0 or negative dt and speed) gives finite torque.
- Calibration about 2 s per turbo car, 30 ms per NA car (release); a step about 2.4 µs (measured).

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
- Plate area 1 − cos θ/cos 7°, θ = 7° + 83°·lift [US 6591667].
- C_D 0.80 one throttle (inferred: fits are about 0.8 at WOT [C_D estimation]); 0.85 per cylinder (guess); at least
  45 mm per cylinder (guess).
- Manifold 0.8·V_d one throttle, 0.3·V_d per cylinder, 1.0·V_d turbo, boost pipe 2.0·V_d (guess: ITBs have no
  plenum [HPA Alpha-N]; Rally2 caps the restrictor-to-plate volume at 20 L [FIA 261]).
- W_cyl = η_v·p·V_d·n/(120RT) and the manifold filling dp/dt = RT/V·(W_in − W_cyl) (generic) [Andersson],
  [Hendricks]. Fully implicit Euler, root-solved per sub-step (inferred: the linearly implicit step overshot a
  per-cylinder tip-in to 2.4 bar).
- η_v: turbo 0.95 flat (guess); NA the WOT table's shape (inferred: the table is mostly breathing).
- No leak; idle air instead (inferred: the plate formula closes at 7°, and a sized leak idled every sample
  55–95 N·m fast).

## Throttle (`engine.rs`, `ecu.rs`)

- Cable: lift = pedal, pedal^1.6 per cylinder (guess: progressive linkages), no lag.
- DBW: plate aim = pedal^1.3, then the ETB (30 ms first order, 40 ms full travel) [Gorzelic]. Rally ECUs map pedal to
  throttle position (MoTeC, Link, Haltech, Life Racing on M-Sport R5, Marelli) [MoTeC DBW], [Link e-throttle],
  [Haltech DBW], [Life Racing], [Marelli]; torque requests are road (ME7) and F1 [Bosch ME7], [F1 torque map].
  The translation sits below 1:1, 40 % pedal → about 30 % throttle [HPA 138]; 1.3 is the exponent through that
  point (inferred).
- The idle air adds to the aim; anti-lag's kick overrides it [MoTeC DBW]. Blips and shift cuts come through the
  interface's throttle (NGP's control unit).
- Part throttle follows WOT to an rpm then falls at roughly constant power, as BeamNG's model [BeamNG throttle].
  Peak net torque at pedal 10/20/30/50 %: na 29/43/54/77 %, turbo 17/42/55/88 %, old 31/47/63/93 %, cable_turbo
  15/26/45/87 % of max (BeamNG 2023: 49/65/76/91 %).

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

## ECU (`ecu.rs`)

- Idle: PI on air, P on spark, 10° base retard with −10/+15° room [MoTeC idle], [HPA M1 idle], [US 7143741],
  [US 7418943]. Gains from the linearised idle and the car's inertia at 0.5 Hz (air) and 3 Hz (spark) (guess: no
  published gains [Yildiz]). Spark P-only (inferred: a spark integrator fights the air loop).
- The loop runs with a closed pedal below idle + 800 rpm, integrates only below idle + 100 rpm (inferred: no
  wind-down while the overrun air decays).
- Idle air ceiling 15 % lift [HPA 138] (Haltech DBW idle about 10 %), [Link idle] (3.5 %).
- DFCO (ECU cars): pedal < 0.02 and n > idle + 800 rpm; resumes below idle + 300 rpm or on the pedal [stealth316],
  [MSExtra], [Hondata].
- Overrun air while the fuel is cut: real ECUs open an idle valve or dashpot on decel [MoTeC FK8R]. Sized so the
  manifold holds 0.2·p_a (decel vacuum 17–23 kPa [Autosport decel]), at every speed (inferred: area in proportion
  to the cylinders' flow, as a throttle-aim-by-rpm table). The s_f point uses it.
- When the fuel returns, the overrun air decays to the idle feed-forward with τ 0.5 s (guess), so no air step.
- Fixed-stop cars have no DFCO (inferred: carbs and mechanical injection keep fuelling through the idle circuit;
  the decel cut-off solenoids were for road emissions).
- Limiter: ignition cut 0→100 % over RPMLimit + 200 rpm, then fuel [HPA M1 limits], [HPA 171], [Haltech limiter].
  The events are picked by deterministic error diffusion (inferred).
- Shift cut keeps the share TorqueCut, airflow kept [FIA 254A] (S2000 cuts), [CSP powertrain].
- No combustion below 150 rpm or when stalled [turboford] (crank speeds 150–250 rpm).
- Anti-lag: armed above 0.45·RPMLimit, active with pedal < 0.1 for at most 3 s, re-armed above 0.3 [MoTeC ALS],
  [Link ALS timeout]. Kick 15 % of the throttle area (lift 0.31) [Link e-throttle ALS] (kicks 10–25 %). As an area
  (inferred: a 15 % lift opens 5 % of the area, the turbo ran down). Retard 60° [Link retard] (−25 to −50°; 60
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
- Turbine nozzle with Ψ, η_t 0.65 (guess) [Andersson]. A_t: the largest nozzle whose cold spool reaches 0.9·π_max
  at the spool knot (inferred). The wastegate area equals A_t (guess).
- Shaft inertia 1e-5 kg·m² [Chalmers] (T45 1.16e-5), [GT1548]. Losses 0.002ω + 2.5e-6ω² + 100 W [rF2 engine.ini].
- Exhaust temperature affine in flow, 700 °C at 0 (guess), 950 °C rated, capped at 1150 °C, τ 0.3 s (guess).
  Intercooler effectiveness 0.7 (guess).
- Boost target π_wot(n) from the table with η_ig 0.38, η_v 0.95, capped (class, else 2.5 abs) and by the restrictor
  [FIA 261], [FIA 255A].
- Wastegate: both aim at π_wot(n)·p_a whatever the pedal (inferred: rally ECUs map pedal to position, and the
  turbo of a pedal-mapped car is held at its WOT boost [passionford]). Electronic (DBW): PI, band 0.5 bar, Ti 0.5 s.
  Spring (cable): P-only. τ 125 ms [Gorzelic]. Band and Ti guess (a limit cycle at narrower settings).
- Blow-off: opens at 0.3 bar over the manifold, full at 0.4 bar, empties the pipe in about 0.1 s (guess).
- Spool from closed to WOT at 4000 rpm reaches 90 % in about 0.65 s [Chalmers] (GT-Power 0.54–1.0 s).

## Open

- The cable turbo sample (Group A8 1993) re-applies after a 1 s anti-lag lift in 0.17 s (the brief: under 0.15 s).
  Under the 15 % area kick its turbo runs down to 1.46 bar; a 20 % kick holds 2.2 bar and passes. The test allows
  cable cars 0.2 s.
- Turbo part-pedal steady curves dip near the boost threshold, which is sharp (about 1.4 bar at 3000 rpm, at target
  by 3250–3500 rpm). K jumps about −30 % across it.
- At a fixed speed below idle, the closed pedal gives up to +220 N·m (turbo sample, 750 rpm), because the idle loop
  opens to its 15 % ceiling. In gear at low speed this pushes the car.
- The turbo sample idles at a 10 kPa manifold: the high low-rpm K needs little air.
- The spark loop saturates (gain about 2°/rpm). The limiter's torque swings each step (mean value per step). The
  wastegate overshoots about 5 %.
- DBW part pedal gives less torque than BeamNG's 2023 curves (table above).

## Sources

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
- US 7418943, idle reserve use: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7418943
- Yildiz, idle plant: https://web.mit.edu/aaclab/pdfs/Yildiray%20Yildiz%20TCST.pdf
- Link idle: https://forums.linkecu.com/topic/17853-e-throttle-idle-problems/
- stealth316, DFCO: https://www.stealth316.com/2-rpmfuelcut.htm
- MSExtra, DFCO: https://www.msextra.com/forums/viewtopic.php?t=33238
- Hondata, DFCO: https://www.hondata.com/help/kmanager/advancedparameters.htm
- MoTeC FK8R, overrun throttle aim: https://www.milspecwiring.com/assets/images/11401_honda_civic_fk8r_kit.pdf
- Autosport, decel vacuum: https://forums.autosport.com/topic/89377-f1-engines-and-engine-braking/
- HPA M1 limits: https://www.hpacademy.com/courses/motec-m1-software-tutorial/setting-up-m1-for-your-engine-engine-speed-limits/
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
- HPA 138, DBW webinar: https://www.hpacademy.com/previous-webinars/138-tuning-drive-by-wire-throttle-control/
- BeamNG throttle curves: https://beamng.com/game/news/blog/throttle-torque-curves/
- Gorzelic 2012, throttle and wastegate: https://public.websites.umich.edu/~annastef/papers_hcci/Gorzelic2012ccdc.pdf
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
