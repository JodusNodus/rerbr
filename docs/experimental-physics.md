# ReRBR experimental physics

ReRBR runs NGP 7's physics bit for bit by default. The launcher's Settings > Physics row (or `--physics experimental`)
offers a second choice, "ReRBR experimental (built on NGP 7)": NGP 7 with some of its systems replaced by ours. Today
it replaces one: a mean-value engine model takes the place of NGP's torque formula. The clutch, gearbox, driveline,
tyres and damage stay NGP's. A tyre model is in development and not part of it: see
[ReRBR experimental tyre model](tyre-model.md).

While it is experimental, a run with it saves no records, ghosts or championship results. Its replays carry the choice
and play back with it.

This page explains what NGP's formula does, what our model does in its place, the evidence behind each part, and how
both compare with BeamNG.drive's published part-throttle model. Every constant with its source is in
[notes/engine-model.md](../notes/engine-model.md), and the code is in `crates/rbr-engine`.

## Why replace NGP's engine formula

NGP's engine is one line of algebra. At a given engine speed it blends two curves by the throttle: the full-throttle
torque curve from the car's file, and a straight braking line. That makes the torque at a fixed rpm linear in the
pedal, with no time in it. A turbo car has no lag, a throttle body has no manifold to fill, and a light pedal at high
rpm behaves like a scaled-down full throttle.

Real engines don't work that way. A nearly closed throttle passes a fixed mass of air per second once the flow chokes,
so the power it allows is roughly fixed and torque falls as rpm rises. A turbo needs exhaust energy before it gives
boost. Engine braking comes from friction and from pumping against a closed throttle, and neither goes to zero at low
rpm. Our model computes these from the air path.

It keeps each car's own full-throttle curve. At full throttle in standard air it gives the car's NGP torque at every
point of the table, except where a turbo can't supply the air yet (see [Torque from air](#torque-from-air)). What
changes is part throttle, transients, idle, engine braking, the limiter and the turbo.

## What NGP's formula does

The engine wrapper in `NGP.dll` (`0x1004b8e0`, ported in `rbr-hybrid/src/ngp/engine.rs` and
`rbr-physics/src/ngp/engine.rs`) returns the crank torque each physics step, 720 times a second. Every RallySimFans
car uses its idle-model path:

```
T = ((1 − t)·B + t·V) / 1.05

B = −(min(ω, ω_limit) / ω_limit) · BrakeCoefficient · MaxTorque
V = damage · (ρ / 1.2041) · Tq(ω) · MaxTorque − friction
```

- `t` is the throttle the control unit passes on, `Tq(ω)` the car's torque table, `ρ` the air density at the car's
  height, `friction` the 1.02 engine sim's extra friction (oil starvation, overheating, damage).
- Above the rev limit `V = 0`. The limiter is a hard fuel cut with the braking line still acting.
- During the control unit's shift cooldown `V` is multiplied by `TorqueCut_NGP`.
- Below the idle speed `t` is ignored and `V` is capped at 5 % of MaxTorque below 0.95·idle. Idle is a limit cycle. On
  one booted car the torque alternates between −13.4 and +106.6 N·m while the engine hovers at 1000–1008 rpm.
- When NGP's 0.15 s speed history shows a stall, the torque is −0.1·MaxTorque.

What follows from it:

- **Part throttle is lazy.** The braking line pulls every part-throttle line down. At 10 % pedal the most torque the
  engine makes at any rpm is 0–2 % of its peak, and at 50 % pedal it is about 40 %.
- **Braking is a line through zero.** `B` is proportional to rpm, so a closed throttle barely brakes at low rpm. On
  turbo cars `BrakeCoefficient` is a share of the boosted MaxTorque, so they brake hard at the limit. Our turbo sample
  car, set up like a 1.6 L Rally2 car with 460 N·m and BrakeCoefficient 0.25, brakes with 115 N·m there, about 9 bar
  of mean effective pressure. The coast curve of a real 2.0 L engine in rFactor 2's engine file gives about 3 bar at
  8000 rpm.
- **No time.** Torque follows the pedal within the step. The 1.02 engine sim still runs underneath and spins its own
  turbo, but its torque is discarded; with our model the turbo sound follows the model's turbo instead.

## The realistic model

Our model is a mean-value engine model, the kind engine control engineers use to design ECUs (Hendricks; Eriksson and
Nielsen; Andersson 2017). It steps once per physics step. "Mean value" means it tracks pressures, flows and speeds
averaged over an engine cycle. It doesn't resolve single combustion events, except at the rev limiter. A four-cylinder
at 7500 rpm fires about every third step at 720 steps a second, so the limiter picks which steps fire. A step costs
about 2.4 µs. Building a car's model takes about 1.3 s for a turbo car and 30 ms for an NA car, once at load.

The model replaces only NGP's torque call. NGP's stall detection, damage factor and shift cooldown, the 1.02 sim's
friction and the air's pressure and temperature reach it each step, and its torque goes back through the same
`/ 1.05`.

### What it reads from each car

| Input | From |
| --- | --- |
| Full-throttle torque curve | `Tq_xx_NGP`, `Tq_xx_RPM_NGP`, `MaxTorque_NGP` |
| Rev limit, idle speed | the control unit's `RPMLimit`, `IdleRPM_NGP` |
| Engine braking at the limit | `BrakeCoefficient_NGP` |
| Displacement | 4 × `CylinderVolume_NGP` (NGP's convention on every car, also the 3-, 5- and 6-cylinder ones) |
| Throttle area | `InletPipeArea_NGP` |
| Turbo or not | `EngineType_NGP` bit 0 |
| Shift cut share | `TorqueCut_NGP` |
| Crank inertia in neutral | `EngineInertia` + `GearBoxInertiaIn` |
| Class and era | RallySimFans' group and year (`cars.json`, `cars_data.json`) |
| Each step | throttle, engine speed, air pressure and temperature, engine temperature, damage, the shift cooldown, NGP's stall answer |

No car file has turbo, throttle actuation, anti-lag, restrictor or cylinder count keys. All 42 installed RallySimFans
cars leave NGP's `Turbo*` keys at their defaults. What the data can't give, the [class rules](#classes) and a short
table of sourced facts give.

### From pedal to torque

```
pedal ──► ECU ──► throttle plate ──► intake manifold ──► cylinders ──► indicated torque
           │            ▲                   ▲                │
           │ idle air,  │                   │ boost          ▼
           │ anti-lag,  │             compressor ◄── shaft ◄── turbine ◄── exhaust
           │ cuts       │
           ▼                                net torque = indicated − friction − pumping
        spark, cut
```

### Throttle

A cable car's plate follows the pedal. A car with a throttle per cylinder (carburettor chokes, slides, individual
throttle bodies) has a progressive linkage, plate = pedal^1.6.

A drive-by-wire car's plate also follows the pedal, through a throttle motor with a 30 ms lag that needs at least
40 ms for full travel. Race ECUs work by position. Life Racing (M-Sport's R5 and Rally2 cars), MoTeC, Link, Haltech
and Marelli all map pedal to throttle position through a table. Link's and Haltech's tables start at one to one.
MoTeC M1 aims at a share of the airflow, which opens the plate more than the pedal at small openings. No rally team's
map is public, so ours is the one to one start. Torque-request pedals belong to road cars' Bosch ME7 and to F1.

The ECU's idle air adds to the plate's aim and anti-lag's kick overrides it. Blips and shift cuts still come from
NGP's control unit through the throttle it passes on.

The plate's open area grows as `1 − cos θ / cos 7°`, from 7° closed to 90° open (US 6591667). At 10 % of its travel a
plate opens about 3 % of its area, at 30 % about 14 %, at 50 % about 33 %. The same patent's flow bench gives a
discharge coefficient of 0.70–0.77 at small openings, a few percent under our 0.8, and a leak of about 5 mm² that we
leave out. At light pedal the two about cancel.

### Manifold

Air flows through the plate as through an orifice, choked below a pressure ratio of 0.528. The cylinders draw it out
at

```
W_cyl = η_v · η_res · p_im · V_d · n / (120 · R · T)
```

and the manifold's pressure follows from the balance, with the manifold's volume as the lag: 0.8·V_d behind one
throttle, 0.3·V_d with a throttle per cylinder, V_d on a turbo plus 2·V_d of boost pipe. The volumetric efficiency
`η_v` follows the torque table's shape on an NA engine (its table is mostly breathing) and is 0.95 on a turbo. On an
NA engine it rises over 1 where the table asks for more work per kilogram of air than any engine can give (next
section).
`η_res` is the share of the cylinder left for fresh air after the residual exhaust, from the ideal cycle:

```
η_res = (r_c − (p_em / p_im)^(1/γ)) / (r_c − 1)        r_c 11 NA; turbo by group (Classes); γ 1.3
```

It is 1 at full load and 0.71–0.87 at idle.

From this alone come the effects NGP lacks. A small opening passes a fixed mass of air once the flow chokes, so its
torque falls with rpm. A tip-in fills the manifold over a few revolutions. A throttle per cylinder has almost no
manifold and answers at once.

### Torque from air

Indicated torque is proportional to the air in the cylinders (the Willans line), scaled by spark timing and cuts:

```
T_ind = K(n) · m_charge / (4π) · η_ign(Δθ) · (1 − cut) · damage
η_ign = 1 − 2.8e-4 · Δθ²        Δθ: spark retard from best timing, degrees
```

`m_charge` is the air the cylinders take in one engine cycle of two revolutions. `K(n)` is the work per kilogram of
that air: the fuel's energy, the mixture, the combustion's efficiency. The model doesn't guess it. At load it runs its
own engine to a steady state at full throttle every 250 rpm and at every knot of the torque table, and solves

```
K = 4π · (T_table + friction + pumping) / m_charge
```

so steady full throttle gives the table back. On an NA engine `η_v` already carries the table's shape, so `K` comes
out nearly flat.

`K` has a ceiling. No engine turns more of its fuel into work than the fuel–air cycle: at our λ 0.88, 34 % at
compression ratio 6, 38 % at 8, 40 % at 9.5 and 42 % at 11 (Heywood, fig. 5-9), and real engines reach about 0.8 of
that. EPA's road turbos come out at 32–39 %. Running richer doesn't raise it: the same figure's work per kilogram of
air is flat from λ 0.88 to 0.7. Where an NA car's table asks for more, `K` stays at the ceiling and `η_v` takes the
rest: the installed NA cars then breathe 1.12–1.19 at their peak, as race engines with tuned intakes do, and the two
S2000 cars 1.25–1.27. A turbo car's `K` stays where the identity puts it, one value over its rpm: 0.97–1.01 times the
ceiling on the Group A turbos, 1.05–1.08 on the Group B Audi, up to 1.19 on the modern cars (Known limits).

The table is an upper bound. On a turbo car below its boost threshold this identity would raise `K` to make up for
the missing air, which gives full torque with no boost and erases the lag. Where the steady full-throttle boost misses
98 % of its target, the model holds `K` at the median of the on-boost points instead, and the torque falls short of
the table until the turbo delivers. On the installed cars:

| Car | Held, rpm | Most below the table |
| --- | --- | --- |
| Fiesta R5 | 250–1250 | 4.2 % |
| WRC 2014–2020, Polo R5, GR Yaris | 250–1250 (WRC 2019–20 to 1500) | 6.9–9.5 % |
| Fiesta Rally2 | 250–1250 | 11.2 % |
| Xsara WRC 2006 | 250–1250 | 16.3 % |
| Focus WRC 2006 | 250–1500 | 20.5 % |
| Alpine A110 RGT | 1750 | 3.2 % |
| Audi quattro A2 | 2500–2750 | 3.3 % |
| Fiesta Rally4 | 2250 | 3.0 % |
| Clio Rally3 | 2000 | 0.6 % |
| Lancer Evo X R4 | 1250–1500 | 0.5 % |

No other car is held, and every car's steady full throttle stays within 0.1 % of its table or below it.

### Friction

Sandoval and Heywood's component model from MIT (SAE 2003-01-0725) is the standard published one for spark-ignition
engines. It sums crankshaft bearings and seals, pistons and rings, valvetrain and auxiliaries, each a function of bore,
stroke and rpm, with the viscous terms scaled by the oil's viscosity. The auxiliaries (oil pump, water pump, alternator)
are the paper's refit to spark-ignition engines, 8.3 + 1.86e-3·N + 7.45e-7·N² kPa. Bore and stroke come from the
displacement as a square engine. The viscosity follows the 1.02 engine sim's engine temperature through a fit to 10W-40
data, so a cold engine at 20 °C has 1.6–2.4 times the friction of a warm one (a review of cold-start studies, Roberts et
al. 2014, gives up to 2.5 times). In game the 1.02 sim's engine runs at 85–90 °C, so this rarely acts.

On an NA car one factor scales the curve at the rev limit. It is set so that a closed throttle there, with the fuel
cut, brakes with the car's own `BrakeCoefficient · MaxTorque`, the one braking point NGP gives, clamped to 0.6–2. The
installed NA cars land at 1.12–1.58. Below the limit the factor falls toward 1 with the square of the speed, so at low
rpm the friction is Sandoval's own, which matches what EPA's road engines lose with the fuel cut at 2000 rpm. With one
factor everywhere the samples braked 1.4–1.7 times as hard as those engines below 5000 rpm.

A turbo car's friction is Sandoval's own at every speed. NGP's figure is a share of the boosted torque, and every
installed turbo car would need 2.1–4.4 times Sandoval's friction to meet it. Measured engines rule that out. From 2000
to 6000 rpm friction grows 2.2–2.7 times on Sandoval's 3.0 L V6, and 2.5–2.7 times with the pumping on the motored
engines of Heywood and Patton; at a factor of 2 the turbo cars grew 3.0–3.1 times (now 2.0–2.1, the NA cars
2.2–2.3). A Rally2 car's 32 mm restrictor: even at the fuel–air cycle, its 214 kW at 6000 rpm leaves at most 3.8 bar
for friction and pumping. At a factor of 2 the model had 5.1, now 3.8. Their closed throttle now sits at or near
EPA's road engines (two turbos among them) at 2000–5000 rpm, and their engine braking at the limit is 35–44 % of
NGP's.

### Pumping

The work of pulling air through a nearly closed throttle and pushing it out through the exhaust, as a mean effective
pressure:

```
PMEP = (p_em − p_im) + c · S_p² / (n_v² r_i⁴) · (p_im / p_a) + c · S_p² / (n_v² r_e⁴) · (p_im / p_a)² · p_a / p_em
```

with Sandoval's constant c = 3.0e-3 kPa·s²/m² for the flow through each set of two valves, `S_p` the mean piston speed
and `r` a valve's diameter over the bore from Patton's survey of four-valve heads (SAE 890836). At an NA engine's full
throttle this is the published formula, about 0.4 bar at 7000 rpm on a 2.0 L four. The densities are ours: the
intake valve's loss scales with the manifold's density and the exhaust valve's with the exhaust's, so a turbo's boost
counts at its own density.

On overrun, ECU cars cut the fuel and hold the manifold near 0.2 bar with extra air, as real ECUs do (tuners read
17–23 kPa of decel manifold pressure; EPA's road engines sit at 16–43 kPa with the fuel cut). Just over idle, with the
fuel still on, the idle loop closes its air down to what holds 0.1 bar, not shut; road engines never run under
9 kPa fired. Friction and pumping together are the engine braking.

### ECU

The parts of a race ECU that change torque:

- **Idle.** An air loop (proportional and integral) and a fast spark loop (proportional), with gains from the car's
  inertia at 0.5 Hz and 3 Hz. The spark sits 27° retarded at idle (η_ign 0.80), a torque reserve the spark loop can
  use either way, as production idle control does. The idle air is capped at the plate's area at 10 % position, down
  to 300 rpm under idle; further down the cap falls to the idle air's base at half idle (anti-stall: enough to catch a
  dip, not to push the car when the clutch drags the engine down). Above idle + 100 rpm the air loop only gives back
  what it gained, so the air it wound up while a clutch bit doesn't hold the engine over idle. Cars from before 1987, on carburettors or mechanical injection, have a fixed idle stop and find their idle speed from the
  balance of air and friction.
- **Overrun fuel cut.** On ECU cars, with the pedal under 2 % above idle + 800 rpm. The fuel returns below idle +
  300 rpm or on the pedal. While the fuel is cut, overrun air holds the manifold at 0.2 bar. When the fuel returns that
  air decays to the idle air over 0.5 s. Fixed-stop cars keep fuelling through the idle circuit.
- **Rev limiter.** It follows Link's convention. The ignition cut ramps from 0 to 100 % between `RPMLimit − 200` and
  `RPMLimit`, and the fuel is cut from `RPMLimit + 100`. The model picks which firings to cut by deterministic error
  diffusion, so the engine bounces just under the limit as a real one does.
- **Shift cut.** The car's `TorqueCut` share as an ignition cut. Air keeps flowing, so the turbo keeps spinning.
- **Stall.** No combustion while NGP reports a stall or below 150 rpm, a starter's cranking speed.

### Turbo

A compressor, a turbine and the shaft between them, with a wastegate, a blow-off valve and an intercooler of
effectiveness 0.7. No rally turbo's map is public, so the compressor and turbine follow generic normalised maps, with
a design speed of 15 000 rad/s and a shaft inertia of 1.7e-5 kg·m², a measured passenger-car rotor (Perge et al.
2017; Chalmers' T45 has 1.16e-5), for a turbo of 0.15 kg/s at a pressure ratio of 2. Each car's rotor is that one
scaled by similarity to its own design flow and pressure ratio: a bigger compressor turns slower and its inertia grows
with the fifth power of its diameter, so a large turbo takes longer to spool and a small one runs faster. The model
sizes them per car at load:

1. The boost target at each rpm is the boost the table's torque needs, capped by the class where its rules have a limit
   (2.5 bar absolute on R5, Rally2, WRC 1.6, RGT and R1–R3, 4 bar on WRC 2.0) and by the restrictor's choked flow.
2. The turbine's nozzle area is fitted so `K` comes out as flat over rpm as the turbo allows. `K` is the fuel's energy
   per kilogram of air and nearly constant, so the boost should carry the table's shape. The exhaust pressure may reach
   at most twice the intake's.
3. Points where the turbo can't make its target are held, as above.

Spool time then comes from the shaft's inertia and the exhaust's energy. The shaft's bearings take 0.002·ω + 2.5e-6·ω²
W, rFactor 2's friction figures without its 100 W static loss, which is there against numerical trouble and stopped the
shaft at idle; the samples' shafts now idle at 4400–4800 rpm (Guzzella: at idle it "can fall below 10 000 rpm").

On drive-by-wire cars an electronic wastegate (a base opening by rpm plus a PID loop) aims at a boost that rises with
throttle position:

```
p_aim = p_a + (π_wot(n) · p_a − p_a) · min(1, θ / θ_open(n))
```

`θ_open` is the plate position past which the plate stops limiting the air at full boost: 0.24 at 1000 rpm, 0.46 at
3000, 0.61 at 5000 and 0.74 at 7000 on the turbo sample. Race ECUs' boost aim tables over rpm and throttle (MoTeC M1,
Link G4+) reach full aim before full throttle in the same way. On cable cars a spring wastegate aims at full boost
whatever the pedal, so they kick at part throttle once the turbo lights. That kick is the step in the cable_turbo
figure's 40 % line at 3250 rpm. The spring gate's diaphragm feels the boost through a short hose, so it opens within
about 10 ms, where the electronic gate's actuator takes 125 ms. A tip-in at a fixed 3000 or 5000 rpm spikes the boost
0.2–1.8 kPa (a quarter of a psi at most) over its steady level; tuners report about 1 psi for a split second on spring
gates (enginebasics). On a stage the Celica, with the 125 ms, ran more than 10 kPa over its steady boost for 16 s of the
171 s it spent at half pedal or more; with the 10 ms, for 0.3 s. A blow-off valve opens at 0.3 bar over the manifold.

### Anti-lag

On cars whose class ran it, anti-lag arms above 45 % of `RPMLimit`. With the pedal under 10 %, for at most 3 s, the
ECU opens the throttle to 20 % of its area, retards the spark 60°, adds 20 % fuel and cuts 30 % of the firings. 60 % of
the unburnt fuel burns in the exhaust manifold, which keeps the turbine spinning toward a boost aim of 1.2 bar gauge.
The blow-off valve stays shut.

## Classes

The class rules come from the FIA articles for each class and RallySimFans' group and year. Without a year a car gets
its group's middle era.

| RallySimFans group | Throttle | Intake | Idle | Anti-lag | Restrictor, boost cap |
| --- | --- | --- | --- | --- | --- |
| Group 4, Group B (NA) | cable | per cylinder | fixed stop | no | none |
| Group B (turbo) | cable | one | fixed stop | Audi from 1985 | none |
| Group A7 (NA) | cable | one, per cylinder from 1993 | ECU from 1987 | no | none |
| Group A8 (NA) | cable | per cylinder | ECU from 1987 | no | none |
| Group A7, A8 (turbo) | cable | one | ECU from 1987 | from 1992 | none to 1989, 40 mm 1990–91, 38 mm 1992–94, 34 mm from 1995 |
| Super 1600 | cable | one | ECU | no | none |
| Super 2000 | cable | per cylinder | ECU | no | none |
| R1, R2, R3, Rally 4 (NA) | drive-by-wire from 2008 | one | ECU | no | none |
| R1, R2, R3 (turbo) | drive-by-wire | one | ECU | yes | 2.5 bar |
| Group N4, R4 | drive-by-wire from 2008 | one | ECU | yes | none to 1989, 40 mm 1990–91, 36 mm 1992–94, 32 mm 1995–2009, 33 mm from 2010 |
| WRC 2.0 | drive-by-wire from 1999 | one | ECU | yes | 34 mm, 4 bar (a guess) |
| WRC 1.6 | drive-by-wire | one | ECU | yes | 33 mm, 36 mm from 2017; 2.5 bar |
| R5, Rally 2 | drive-by-wire | one | ECU | yes | 32 mm, 2.5 bar |
| Rally 3 | drive-by-wire | one | ECU | yes | 30 mm, 31 mm from 2022; no boost limit |
| Rally 4 (turbo) | drive-by-wire | one | ECU | yes | 30 mm; no boost limit |
| RGT | drive-by-wire | one | ECU | yes | 2.5 bar |
| no metadata | cable | one | ECU | no | none |

A turbo's compression ratio follows its group: 7.0 on Group B (the road ur-quattro's 2.1 has 7.0, the 205 T16 6.5,
the S1 E2 and the Delta S4 7.5), 8.5 on Group A, N and R4, whose road engines run 8.0–9.0 (Cosworth YB, EJ20 and the
Integrale's 8.0, 3S-GTE 8.5–8.8, 4G63 8.5–9.0, EJ257 8.2, 4B11T 9.0), 10.5 on R5 and Rally2 and 12.5 on WRC 1.6, the
highest their rules allow (FIA articles 261 and 255A), and 9.5 on the rest. No modern rally engine's is published;
their road bases run 10.0–10.5 (the 1.6 EcoBoost, the GR Yaris's G16E-GTS, the 1.2 PureTech). NA engines keep 11.

Group N, R4, Rally3 and Rally4 have a restrictor and no boost limit in their rules, so on those cars the restrictor
and the torque table set the boost.

A short table of facts adds what no rule knows. The Audi quattro A2 has 5 cylinders, the two Porsche 911s 6, and the
Fiesta Rally3, Rally3 Evo and Rally4, the 208 Rally4 and the GR Yaris Rally2 3. The Lancer Evo IX N4 keeps its road
car's cable throttle, the Clio III R3 its road car's drive-by-wire. The Golf II GTI 16V's K-Jetronic has a fixed idle
stop, and the Fiesta R2, an NA car NGP flags as turbo, has no anti-lag.

## Evidence

How firm each part is:

| Part | Basis | Strength |
| --- | --- | --- |
| Full-throttle torque | the car's table, reproduced wherever the air path can make it | the car's data |
| Orifice flow, manifold, turbo shaft and flows | Hendricks' mean-value model; Andersson 2017; Eriksson; NASA's choked flow | textbook |
| Residual gas | the ideal cycle (Heywood) | textbook formula; the old turbos' compression ratios their road engines', R5, Rally2 and WRC 1.6 at their rules' maximum, the rest guessed |
| Volumetric efficiency | the NA table's shape, over 1 where `K` meets its ceiling; 0.95 on turbos | inferred |
| Work per kilogram of air | the table through the identity; at most the fuel–air cycle on NA engines (Heywood fig. 5-9); EPA's road turbos 32–39 % | the car's data under a textbook ceiling; modern turbo cars over it, as are their makers' figures |
| Friction components | Sandoval–Heywood (SAE 2003-01-0725, Appendix B), with the paper's own refit of the auxiliaries; its worked example reproduced within 4 % | published model; about 15 kPa over its own 3.0 L engine |
| Friction level | NA: one point per car, NGP's braking at the limit, clamped to 0.6–2; Sandoval's own at low rpm. Turbo: Sandoval's own | the car's data, partly; low rpm against EPA's fuel-cut braking; the growth with rpm against Sandoval, Heywood and Patton; the turbos' against a Rally2's restrictor |
| Oil viscosity | a Vogel fit through Anton Paar's 10W-40 data | measured data |
| Pumping | the manifolds' difference plus Sandoval's valve losses, valve sizes from Patton's survey (SAE 890836) | published model; its density scaling ours; the NA exhaust's back pressure guessed |
| Overrun manifold pressure 0.2 bar | tuners' decel figures, 17–23 kPa; EPA's road engines 16–43 kPa with the fuel cut | practitioner, measured road engines |
| Spark efficiency | 1 % loss at 6° retard (MIT 2.61), quadratic (US 6363317); Guzzella's 2.4–3.3e-4 per degree² within 15°, Heywood's to 20° | textbook curves to 20°; the idle's 27° and anti-lag's 60° beyond them |
| Idle loops | HP Academy's MoTeC M1 and Link idle setup; the reserve from an idle control paper (arXiv 2012.02859) and US 7418943 | tuner docs and papers; the spark room and loop gains ours |
| Overrun fuel cut | cut and resume speeds with hysteresis (MSExtra, Hondata, stealth316) | tuner docs; our thresholds guessed |
| Rev limiter | Link G4+ limiter (HP Academy webinars 084 and 171) | tuner docs |
| Pedal map | Link and Haltech set the throttle from the pedal through a table, MoTeC M1 by airflow | tuner docs; the one to one start is ours, no rally map is public |
| Plate flow at small openings | US 6591667's flow bench: C_D 0.70–0.77, a 5 mm² leak | measured, one 60 mm throttle |
| Torque at part load | EPA's steady maps of nine road engines: zero torque at 18.5–27.2 kPa (ours 16.0–21.8), the turbos 27 % of their 100 kPa torque at 50 kPa (Honda 42 %; ours 39–43 %) | measured, road engines |
| Torque per charge on turbos | the table: a Rally2 makes 33.5 bar BMEP at no more than 2.5 bar abs; the makers: M-Sport's Fiesta R5 475 N·m at 4000 rpm, Škoda's Fabia R5 evo 425, VW's Polo R WRC 425 at 5000, Hyundai's i20 N Rally2 350 | the car's data, within its makers'; on the turbo sample 1.4–1.8 times the road turbos' torque at 100 kPa |
| Exhaust temperature | a line through EPA's four road turbos, 520 °C at no flow; 300 °C where nothing burns; half the retard's lost work (Guzzella's k_e ≈ 0.5); 950 °C at rated flow | measured road engines; the rated point guessed |
| Turbine efficiency | 0.65; Marelli's turbine 0.62 averaged over the pulse, 0.78 steady; Guzzella's peaks 0.65–0.75 | measured, one turbine |
| Boost aim by throttle | MoTeC M1 and Link boost aim tables (HP Academy courses) | tuner docs; where full aim starts is ours |
| Class rules | FIA articles 254, 254A, 255, 255A, 260, 261; Appendix J 1985–2001 for the restrictor history (Group N 40, 36, 32 mm, 33 from 2010) | regulations; anti-lag and drive-by-wire years from the cars' history |
| Turbo maps | generic normalised shapes, sized per car | inferred: no rally turbo map is public |
| Shaft inertia, spool | Chalmers' GT-Power study: a 2.0 L four 0.54 s to 90 % at 1000 rpm, a 1.5 L three with a T45 1.00 s, 0.62–1.02 s at 2100–2400 rpm; measured rotors 1.7e-5 and 2.7e-5 kg·m² (ours 1.7e-5); bearing loss 95–500 W at 80 000 rpm on two rigs (ours 192 W) | published simulation and measurements |
| Anti-lag | Link settings: a kick of about 20 %, retard of 25–50° from the base, a hold of about 5 s after a lift | settings documented; our retard, fuel, cut, 3 s, boost aim and burn share guessed |
| Drive-by-wire on modern rally cars | the era and the road engines; homologated ECU actuators | inferred: no team says it outright |

No measured engine-braking curve of a race engine is public, so an NA car's friction level at the limit rests on NGP's
own figure, checked against a published rFactor 2 coast curve of a real 2.0 L engine. EPA's road engines brake −1.1 to
−1.5 bar BMEP at 2000 rpm and −1.7 to −2.0 at 4000 with the fuel cut; the NA sample brakes −1.64 and −2.19, the turbo
samples −1.59 to −1.62 and −2.05 to −2.06. Turbo maps and spool rates are generic. Anti-lag's real torque on overrun is
unpublished.

`harness/engine_scorecard.py` scores the model against these published figures, row by row, from
`data/engine-targets.csv` (`notes/engine-model.md`, "Scorecard").

## Compared with NGP and BeamNG

BeamNG.drive described its part-throttle model in 2023 ([Modelling part-throttle torque curves in
BeamNG.drive](https://beamng.com/game/news/blog/throttle-torque-curves/)). Like NGP it authors only the full-throttle
curve per car and computes part throttle from it. Its 2013 model scaled the curve by the throttle. The post says this
made cars hard to drive: "A driver may need to apply excessive throttle inputs to achieve a specific torque output".
Its 2016 "constant power" model lets the pedal set power, so each pedal line follows the full-throttle curve up to
some rpm and then falls as 1/rpm, and the 2023 version rounds those corners following Niels Heusinkveld's research.
Turbo, damage and altitude come on top, and the post lists engine braking as work to do. The post calls a
physics-based engine the ideal it can't afford. Ours is one at the mean-value level, at 2.4 µs a step.

The figures below come from four synthetic sample cars in `crates/rbr-engine/src/samples.rs`, so no game data is in
this repository:

| Sample | Set up like | Engine |
| --- | --- | --- |
| old | Fiat 131 Abarth, Group 4, 1978 | 2.0 L NA, 226 N·m, cable, a throttle per cylinder, fixed idle stop |
| na | 208 R2, 2012 | 1.6 L NA, 190 N·m, drive-by-wire |
| turbo | Fiesta Rally2, 2023 | 1.6 L turbo, 460 N·m, drive-by-wire, anti-lag, 32 mm restrictor |
| cable_turbo | Celica ST185, Group A, 1993 | 2.0 L turbo, 460 N·m, cable, spring wastegate, anti-lag, 38 mm restrictor |

The NGP lines are NGP's formula at a fixed speed, without friction and air density. `cargo run --release -p
rbr-engine --example curves -- DIR` writes the data and `python3 -I crates/rbr-engine/examples/plot.py --figures DIR
docs/images/engine-model` draws them.

### Part throttle

![Steady torque by pedal, turbo sample](images/engine-model/torque-by-pedal-turbo.png)

![Steady torque by pedal, NA sample](images/engine-model/torque-by-pedal-na.png)

![Steady torque by pedal, old sample](images/engine-model/torque-by-pedal-old.png)

![Steady torque by pedal, cable turbo sample](images/engine-model/torque-by-pedal-cable_turbo.png)

NGP's lines are its full-throttle curve squeezed toward the braking line. Ours follow full throttle up to an rpm and
then fall, the shape BeamNG's 2016 and 2023 models are built to give. In ours it comes from choked flow through the
plate. On the old sample the power above the closed-throttle line stays at 9.6–9.8 kW from 3000 to 8000 rpm at
10 % pedal, and at 35–37 kW at 20 %.

![Most torque and power at each pedal](images/engine-model/max-torque-by-pedal.png)

The most net power each pedal position gives at any rpm from idle up, as a share of the full pedal's:

| Pedal | NGP | old | na | turbo | cable_turbo | BeamNG 2023 |
| --- | --- | --- | --- | --- | --- | --- |
| 10 % | 0–1 % | 5 % | 10 % | 8 % | 3 % | 9 % |
| 20 % | 2–8 % | 19 % | 23 % | 21 % | 11 % | 25 % |
| 30 % | 12–19 % | 44 % | 42 % | 49 % | 22 % | 43 % |
| 40 % | 23–31 % | 74 % | 61 % | 73 % | 44 % | 57 % |
| 50 % | 36–42 % | 91 % | 77 % | 88 % | 75 % | 69 % |
| 60 % | 48–53 % | 97 % | 87 % | 97 % | 88 % | 80 % |
| 70 % | 61–65 % | 99 % | 93 % | 98 % | 94 % | 89 % |
| 80 % | 74–77 % | 99 % | 97 % | 99 % | 97 % | 94 % |

And the most net torque, as a share of peak torque:

| Pedal | NGP | old | na | turbo | cable_turbo | BeamNG 2023 |
| --- | --- | --- | --- | --- | --- | --- |
| 10 % | 0–2 % | 26 % | 40 % | 27 % | 12 % | 49 % |
| 20 % | 6–10 % | 47 % | 53 % | 49 % | 25 % | 65 % |
| 30 % | 14–21 % | 63 % | 65 % | 64 % | 38 % | 76 % |
| 40 % | 26–32 % | 81 % | 75 % | 79 % | 62 % | 84 % |
| 50 % | 38–43 % | 93 % | 84 % | 91 % | 88 % | 91 % |
| 60 % | 51–55 % | 97 % | 91 % | 97 % | 93 % | 95 % |
| 70 % | 63–66 % | 98 % | 95 % | 98 % | 97 % | 98 % |
| 80 % | 75–77 % | 98 % | 98 % | 99 % | 98 % | 99 % |

BeamNG's values are read from its published plots, and the NGP columns span the four samples. Power compares better:
at light pedal the torque share mostly says how strong a car's full-throttle curve is near idle, where every light
pedal line meets it.

On power, the drive-by-wire samples follow BeamNG up to 30 % pedal and give more from 40 % up. Two samples give less at
light pedal, for reasons in their hardware. The old car's throttles per cylinder sit on a progressive linkage. The
cable turbo makes its full power on boost, and at light pedal it is an NA engine until the turbo spins. BeamNG scales
power with the pedal whatever the turbo does.

An earlier version mapped the drive-by-wire pedal as pedal^1.3, from HP Academy's drive-by-wire webinar (40 % pedal,
about 30 % throttle), and gave about half BeamNG's power at 10–20 % pedal. That calibration is a Haltech on a street
Nissan 350Z, shaped to hold a steady speed on the road. The rest of the chain checks out against measurements: the
plate flow above, and EPA's steady map of the 2013 Ford 1.6 EcoBoost, which makes 27 % of its 100 kPa torque at
50 kPa of manifold pressure where ours makes 28 % at 47 kPa.

BeamNG's curve isn't measured data either. Niels Heusinkveld, whose research the post credits, read the part-throttle
curves of 12 sims, not cars, and proposed a shape from them. Forum readings of BeamNG's engine code have it apply
friction and engine braking apart from these curves, so the plotted lines are most likely torque before friction. Net,
its 10 % line would brake at 8500 rpm, as ours does, since the air through a small opening sets the indicated power and
friction and pumping take their share first. Its power plot's 0 % line repeats the torque plot's values on the kW axis.

### Engine braking

![Closed-throttle torque](images/engine-model/engine-braking.png)

Closed pedal, N·m, ours / NGP / the rFactor 2 2.0 L coast curve scaled by displacement:

| Sample | 2000 rpm | 4000 rpm | 6000 rpm | 8000 rpm |
| --- | --- | --- | --- | --- |
| old | −14.4 / −15.8 / −20.8 | −28.7 / −31.5 / −29.8 | −42.3 / −47.3 / −40.6 | −58.8 / −63.1 / −52.5 |
| na | −20.8 / −13.9 / −16.6 | −27.8 / −27.8 / −23.9 | −38.8 / −41.7 / −32.6 | −55.0 / −55.6 / −42.1 |
| turbo | −14.6 / −30.7 / −16.6 | −26.3 / −61.3 / −23.9 | −33.6 / −92.0 / −32.6 | −42.2 / −115.0 / −42.1 |
| cable_turbo | −25.3 / −30.7 / −20.8 | −32.7 / −61.3 / −29.9 | −42.1 / −92.0 / −40.7 | −53.4 / −115.0 / −52.5 |

Each figure is a fresh engine held at that speed. The turbo sample's 2000 rpm is exactly its idle + 800 rpm, where
the overrun cut has not started, so it keeps its fuel there; come down from higher it brakes with −20.6 N·m.

NGP's braking line goes through zero at 0 rpm. Ours has a large part that doesn't fall with rpm, friction and pumping
at a closed throttle, and so does BeamNG's 0 % line, which runs almost flat from about −70 N·m at 500 rpm to −90 N·m
at 8500 rpm on an engine of about 600 N·m. At the rev limit ours matches NGP on NA cars, since that is where the
friction is calibrated, and at 2000–4000 rpm the NA sample sits near NGP and the rFactor 2 curve. On turbo cars ours
brakes a third to half as hard as NGP and lies on the rFactor 2 curve from 4000 rpm up: their friction is Sandoval's
(Friction).

Below idle the figure's curves rise. With the pedal closed the ECU's idle air opens to its ceiling to stop a stall,
down to 300 rpm under idle, then less: the turbo sample gives +118 N·m at 1000 rpm, +93 at 750 and +35 at 500 (was
+166 at 500 with the whole ceiling at any speed).

In gear with the pedal closed a car creeps at its idle speed in first, as a real one does: on the flat the Fiesta
Rally2 creeps at 10.8 km/h (idle 1200 rpm; NGP 11.0 km/h), the Fiesta R2 at 9.5 and the Golf II GTI 16V, on its fixed
idle stop, at 5.5. Braked in gear, it stalls: with the brake rising to full over 16 s the Rally2 stalls at 20 % brake
with at most 90 N·m from the engine (152 before; NGP at 45 %), about where a car with no engine would stop; the R2 at
18 % (46 N·m), the Golf at 9 % (23 N·m). The ECU cars still pull away from rest at a closed pedal, with clutch help on
or off; the Golf needs a touch of throttle. The creep and stall runs predate the friction and pumping refit
(2026-11) and were not re-run.

### Response

![Tip-in at fixed rpm](images/engine-model/tip-in.png)

NGP's torque follows the pedal within the step and its turbo cars have no lag. In ours, from a closed pedal to full
pedal at a fixed speed, the drive-by-wire NA sample reaches 90 % of its torque in 25–28 ms, the throttle motor's lag
and the manifold's filling. The old sample, with a throttle per cylinder on a cable, gets there within the step. The
turbo sample's torque jumps to its off-boost level and then follows the boost, reaching 90 % in 0.74 s at 3000 rpm
and 0.54 s at 5000. It overshoots 1.1–1.4 % from 2500 to 5000 rpm and settles within 2 % in 0.7–1.0 s.

From a steady 5 bar of mean effective pressure to 90 % of full torque, the turbo sample takes 0.85 s at 2000 rpm and the
cable_turbo sample, with its larger and heavier rotor, 1.51 s at 3000. Chalmers' GT-Power study gives, for the same
test, 0.54 s for Volvo's 2.0 L turbo four and 1.00 s for a 1.5 L three with a T45 at 1000 rpm (0.68 s with less than
half its inertia), and 0.62–1.02 s on those engines at 2100–2400 rpm. Our rally turbos make almost no boost at 1000 rpm,
so that point doesn't apply to them. With the earlier rotor of 1e-5 kg·m² the samples took 0.46 s at 2000 rpm, and with
one 1.7e-5 rotor for every car 0.69 s and 1.18 s.

![Anti-lag](images/engine-model/anti-lag.png)

Lift for a second at 4500 rpm and re-apply. Without anti-lag the boost falls to ambient and the torque takes 0.78 s
to come back to 90 %. With it the boost stays above 2.2 bar and the torque is back to 90 % in 0.13 s. Lifted, the
engine brakes a little harder with anti-lag than without (−33 against −28 N·m).

### Idle and the rev limiter

![Idle and free rev](images/engine-model/idle-and-rev.png)

The figure runs each sample in neutral with the car's inertia: idle, a 300 rpm dip at 3 s, then full pedal from 6.0 to
6.3 s. NGP's lines come from a replica of its formula. The ECU cars pull the dip back to within 20 rpm of idle in
0.29–0.34 s. The old car, with its fixed idle stop, takes 1.4 s to come within 50 rpm, longer than NGP. After the
blip the NA car falls faster than NGP, from its stronger braking at low rpm. The turbo car reaches 5240 rpm in the
0.3 s blip where NGP reaches the limiter, because its turbo hasn't spooled.

![Rev limiter](images/engine-model/limiter.png)

At full pedal in neutral NGP sits on the limit, alternating full torque and braking. Ours cuts firings over the
200 rpm under the limit and bounces 11–84 rpm under it on the NA sample. The turbo sample bounces from 79 rpm under
the limit to 23 over it.

### In game

Every number here was measured at `cdadc164`, the physics that ships: "NGP 7" is NGP 7's physics, "experimental" is
the realistic engine with NGP's tyre (`{ engine: "realistic", tyre: "ngp" }`), so every gap is the engine's.

The autopilot drove each of four RallySimFans cars for 600 s on a gravel stage, track-11_O (Mustaselka), and on a
tarmac one, track-41 (Cote D'Arbroz, its M variant, as the season's weather loads it), once with NGP 7's physics and
once with the experimental physics. Three of them are the cars the samples are modelled on. Only the Fiesta has
drive-by-wire, and its experimental run is with the one to one pedal map. "Over the limit" is the time the engine
spent above `RPMLimit`. "Slip" is the time in gear, above 3 m/s, with the engine more than 3 % above the speed the
gear gives with the clutch engaged, out of about 590 s in gear on gravel and 305–325 s on tarmac (track-41 finishes
in about 5 minutes).

Gravel, track-11_O:

| Car | Physics | Split 2 | Highest rpm over the limit | Over the limit | Slip |
| --- | --- | --- | --- | --- | --- |
| Fiesta Rally2 | NGP 7 | 442.63 s | +34 | 5.0 s | 3.3 s |
| | experimental | 434.90 s | +34 | 0.6 s | 4.5 s |
| Celica ST185 | NGP 7 | 445.47 s | +25 | 5.4 s | 4.6 s |
| | experimental | 436.42 s | +6 | 0.01 s | 5.6 s |
| Golf II GTI 16V | NGP 7 | 466.95 s | +12 | 5.7 s | 6.0 s |
| | experimental | 446.00 s | −7 | 0 s | 8.9 s |
| Fiat 131 Abarth | NGP 7 | 463.65 s | +13 | 4.5 s | 7.9 s |
| | experimental | 447.55 s | −25 | 0 s | 12.4 s |

Tarmac, track-41:

| Car | Physics | Split 2 | Finish | Highest rpm over the limit | Over the limit | Slip |
| --- | --- | --- | --- | --- | --- | --- |
| Fiesta Rally2 | NGP 7 | 210.71 s | 309.01 s | +25 | 5.0 s | 3.6 s |
| | experimental | 207.66 s | 304.76 s | +34 | 0.5 s | 3.4 s |
| Celica ST185 | NGP 7 | 212.97 s | 310.91 s | +25 | 5.2 s | 3.5 s |
| | experimental | 207.91 s | 304.71 s | +6 | 0.01 s | 4.4 s |
| Golf II GTI 16V | NGP 7 | 222.28 s | 324.24 s | +21 | 4.7 s | 4.5 s |
| | experimental | 213.82 s | 314.03 s | −17 | 0 s | 5.2 s |
| Fiat 131 Abarth | NGP 7 | 221.31 s | 323.35 s | +13 | 4.2 s | 5.9 s |
| | experimental | 214.69 s | 315.10 s | −25 | 0 s | 6.2 s |

Every run finished or ran its 600 s and was still drivable at the end, and no engine stalled: the slowest any turned
was 82 rad/s, the experimental Golf braking at full on track-41's run-out, where NGP's Golf kept 89 rad/s (an
earlier build stalled there, see
[The clutch under the experimental engine](#the-clutch-under-the-experimental-engine)). The experimental limiter
keeps every car at its limit in gear: the Fiesta's and the Celica's time over is their start burnout in neutral,
and the Fiesta's drop into first from it. With the experimental physics the turbo cars reached split 2 1.7–2.0 %
sooner on gravel and 1.4–2.4 % on tarmac, the NA cars 3.5–4.5 % sooner on gravel and 3.0–3.8 % on tarmac. The
autopilot's caution sets most of its pace, and this is one stage and one run per car, so these gaps are not lap
times a driver would see. On gravel the clutch slips 1.0–4.5 s longer per run, on tarmac between 0.2 s shorter and
0.9 s longer, and the engine's mean distance from the clutch-engaged speed stays at most 2.6 rad/s.

#### 0 to 100 km/h

The autopilot's standing starts with the Fiesta Rally2 at full throttle, timed from the first move over 0.5 m/s.
The start revs in neutral on the handbrake and drops into first at full throttle; NGP's clutch help then closes the
clutch. The flat pad (`--stage pad-tarmac|pad-gravel`, a generated level square of one material, `rbr_btb::pad`) is level to
2 cm over these runs, and the car holds its heading on it:

| Flat pad | NGP 7 | Experimental |
| --- | --- | --- |
| Tarmac (material 37) | 4.00 s | 3.98 s |
| Gravel (material 17) | 4.62 s | 4.57 s |

The nearest sourced figure is a maker's claim for another Rally2 car: Volkswagen gives 4.1 s for the Polo GTI R5
(1230 kg, 272 PS). No measured figure (GPS onboard, WRC2 telemetry, magazine test) for any Rally2 car was found, and
M-Sport publishes none for the Fiesta.

The same start on stages (`--flat-out`, `harness/car_pace.py`), where the driver follows the road and waits for the
wheelspin to end before it shifts up. No installed tarmac stage starts flat: the retail French stages climb 2–15 %
over their first 100 m, and RSF 504 climbs 6.7 %. Retail track-64 (Hualapai Nation, gravel) is flat and straight
for 100 m.

| Stage | NGP 7 | Experimental |
| --- | --- | --- |
| RSF 504, tarmac, 6.7 % uphill | 4.52 s | 4.47 s |
| track-64, gravel, flat | 4.66 s | 4.67 s |
| RSF 345, gravel, 4 % downhill | 4.50 s | 4.49 s |

A launch control (revs held below the limiter, then the throttle holding the wheel slip), tried on an earlier build,
was no faster on any of the three stages: the tyres pull as hard at about 9 m/s of wheelspin as near their peak at these
speeds, and cutting the throttle loses boost (it bogged the engine to 130 rad/s). The gap to a 3.5 s target is not
the start.

### The clutch under the experimental engine

The original's clutch decides which way it slips from the sign of the engine's torque, and locks again whenever the
engine's speed is on the other side of the wheels' from that. NGP's engine never pulls near idle, so the rule holds for
it. Ours does for a moment after an overrun, while the manifold fills. With the driven wheels locked under braking the
clutch help opened the clutch in time, but the rule then read the falling wheel speed as the slip reversing and tied
the engine to the wheels every step, down to a stall, with the clutch open. With the experimental engine the clutch
now locks again only if it has the torque to bring the engine to the wheels' speed in that step. Otherwise it keeps
slipping. The Golf that stalled now keeps 82 rad/s through the same stop. A shift or a lift with the engine above
the wheels' speed slips at the clutch's torque for a few steps where it used to snap: of six cars checked, two reach
50 km/h 0.04–0.05 s sooner from a standing start and four are unchanged to the millisecond. NGP's and the 1.02
physics do not take this path.

## Known limits

- The turbo sound follows the model: the whine follows its shaft speed and boost, the blow-off its valve, and the
  anti-lag pops play one short burst (0.3-0.7 s) each time its anti-lag starts, and only then. Real rally anti-lag
  bangs about 7 times a second for about 0.5 s per lift, louder still (counted in onboard and roadside videos); ours
  are sparser and quieter, for listening comfort. The whine's pitch is the model's shaft speed times 0.78, to
  stay in the range the 1.02 sim's turbo gave the sound. The blow-off's loudness takes the Fiesta Rally2's valve size
  for every car. A sound bank without backfire sounds, the Fiesta Rally2's among them, plays the pops on the game's
  own pop sounds, which NGP mutes for it, unless its engine recordings already crackle off the throttle. Until the
  model's anti-lag first runs, and on cars without anti-lag, the sound's own crackle still plays on a throttle lift.
- The clutch's capacity is NGP's, sized for its torque. The model's peak torque is the same, so this matters only for
  transient overshoot.
- Damage, overheating and oil starvation reach the model through NGP's damage factor and the 1.02 sim's friction, as
  they reach NGP's formula.
- Modern turbo cars fall short of their table below about 1500 rpm (the held points above). No rally engine's
  low-rpm curve is public. The one measured curve of a base engine we found, PSA's road 1.6 THP (the base of the 208
  and C3 Rally2 engines), makes about 40 % of its peak torque at 1000 rpm and 80 % at 1500 (SAE 2018-01-1423). The
  tables ask for more than that at 1000 rpm, and the model still gives more, so the shortfall is not a reason to add
  low-rpm boost.
- Modern turbo cars idle at 15.1–17.5 kPa of manifold pressure and the turbo sample makes no torque at 15.6 kPa at 2500
  rpm; EPA's warm road engines idle at 22–29 kPa and make none at 18.5–27.2. The rally tables ask for about 13 bar of
  BMEP per bar of boost (a Rally2's 33.5 bar at the 2.5 bar cap), well over road turbos', so little air turns the
  friction over. Before the friction followed rpm they idled at 20–22 kPa, from twice Sandoval's friction at idle.
- Closed-pedal braking still runs up to 0.2 bar of BMEP over EPA's road engines at 2000–4000 rpm.
- A modern turbo car's work per kilogram of air sits over the fuel–air cycle, up to 1.19 times it (the Rally2 47.5 % at
  4000 rpm against 41 %), so as to keep NGP's torque. At the 2.5 bar cap the tables need 13–15 bar of BMEP per bar of
  boost; the makers claim as much (M-Sport's Fiesta R5 475 N·m at 4000 rpm). Nothing sourced closes it: running richer
  adds nothing (Heywood), turbo η_v peaks at 0.95–0.99 (Garrett), a road intercooler measured 0.57 and 0.91 with water
  spray (one paper), and the R5, Rally2 and WRC 1.6 engines already run their rules' highest compression ratio. Three
  ways down were tried and dropped: more charge as on NA cars ran away once the restrictor held the air; boost carrying
  `K` to the ceiling wherever it could asked the Rally2's turbo for 2.05 bar at 2000 rpm, at its limit, so it never
  reached 90 % in 5 s; and the restrictor's full flow at high rpm brought `K` there only from 1.16 to 1.10 (its pressure
  drop and the back pressure take the rest), raised it at the cap and spooled the turbo sample under GT-Power's band.
- The turbo cars' exhaust runs about 660 °C at idle, against 296–430 °C at road engines' ports and up to 657 °C on a
  road turbo retarded at its lightest loads, and about 565 °C at light part load against 585–732 °C on road turbos at
  the same BMEP. One line over the share of the rated flow cannot hold both.
- Between idle − 300 rpm and idle a closed pedal still gets the whole idle air ceiling, up to +118 N·m on the turbo
  sample at 1000 rpm. How far below idle a rally ECU's anti-stall reaches is not published; the 300 rpm and the fade to
  half idle are guesses.
- On the limiter the torque swings from step to step with the firing rhythm: the model cuts whole firings, rotating
  the cut as ECUs do. In neutral the HUD's rpm bounces 30–110 rpm from frame to frame under the limit; in gear a few
  rpm. The steering force doesn't show it.

## Sources

The full list, with the line each one supports, is in [notes/engine-model.md](../notes/engine-model.md#sources).

- SAE 2018-01-1423, full load of PSA's 1.6 THP: https://www.epa.gov/sites/default/files/2018-10/documents/sae-paper-2018-01-1423.pdf

- Andersson, mean-value engine and turbo model, Linköping MSc 2017: https://www.diva-portal.org/smash/get/diva2:1138994/FULLTEXT01.pdf
- Hendricks and Sorenson, mean-value SI engine model: https://orbit.dtu.dk/en/publications/mean-value-si-engine-model-for-control-studies
- Moraal & Kolmanovsky, turbocharger modelling (SAE 1999-01-0908): https://www.researchgate.net/publication/244520704_Turbocharger_Modeling_for_Automotive_Control_Applications
- Boost spikes on spring wastegates, enginebasics: https://enginebasics.com/Advanced%20Engine%20Tuning/Boost%20Spiking.html
- Pressure delay in hoses, Thomasson 2014 (Linköping dissertation 1590): https://www.diva-portal.org/smash/get/diva2:709572/FULLTEXT01.pdf
- NASA, choked mass flow: https://www.grc.nasa.gov/www/k-12/airplane/mflchk.html
- Throttle plate area, US 6591667: https://patents.google.com/patent/US6591667
- Sandoval & Heywood, An improved friction model for spark-ignition engines (SAE 2003-01-0725):
  https://saemobilus.sae.org/papers/improved-friction-model-spark-ignition-engines-2003-01-0725
- Sandoval–Heywood friction model, as transcribed in ESA.NET: https://github.com/pangtuwi/ESA.NET/pull/223
- Patton, Nitschke & Heywood, Development and evaluation of a friction model for spark-ignition engines (SAE 890836):
  https://doi.org/10.4271/890836
- Heywood, Internal Combustion Engine Fundamentals (McGraw-Hill 1988): the fuel–air cycle, friction, mechanical
  efficiency
- Guzzella & Onder, Introduction to Modeling and Control of Internal Combustion Engine Systems (Springer 2010):
  spark efficiency, exhaust heat, turbine efficiency, throttle and manifold
- Marelli & Capobianco 2011, steady and pulsating flow efficiency of a waste-gated turbine (Energy 36, 459):
  https://doi.org/10.1016/j.energy.2010.10.019
- Makers' figures: M-Sport, Ford Fiesta R5 2019 technical specification (PDF); Škoda Motorsport, Fabia R5 evo
  technical specification: https://cdn.skoda-storyboard.com/2019/12/MOTORSPORT_Technical_specification.pdf; Hyundai
  Motorsport, i20 N Rally2 technical specifications: https://motorsport.hyundai.com/customer-programmes/rally2;
  Volkswagen Motorsport, Polo R WRC (2012):
  https://web.archive.org/web/20121212190829/http://volkswagen-motorsport.com/index.php?id=339&L=1
- Compression ratios: Toyota UK, GR Yaris technical specifications (2024):
  https://media.toyota.co.uk/wp-content/uploads/sites/5/pdf/240621-GR-Yaris-Tech-Specs.pdf; the other road engines'
  as listed in `notes/engine-model.md` (tech-racingcars, ultimatespecs, ultimatecarpage, motorreviewer, FordWiki,
  Wikipedia)
- Garrett, How to select a turbo, part 2 (η_v of four-valve heads):
  https://www.garrettmotion.com/news/newsroom/article/how-to-select-a-turbo-part-2-understanding-calculations-to-turbo-any-engine/
- Engine oil viscosity, Anton Paar: https://wiki.anton-paar.com/us-en/engine-oil/
- Willans line, Moretto 2024: https://www.dora.lib4ri.ch/empa/dload/empa:36100/PDF/Moretto-2024-A_method_to_quantify_the-(published_version).pdf
- Spark retard and torque, MIT 2.61: https://web.mit.edu/2.61/www/Lecture%20notes/Lec.%2009%20SI%20engine%20combustion%20I.pdf
- Spark efficiency curve, US 6363317: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/6363317
- Idle spark reserve, arXiv 2012.02859: https://arxiv.org/abs/2012.02859
- Idle reserve use, US 7418943: https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7418943
- MoTeC M1 idle: https://motec.zendesk.com/hc/en-gb/articles/14575739507727-Checklist-Idle-Setup
- Link G4+ idle, anti-stall gain: https://www.hpacademy.com/courses/link-g4-plus-software-tutorial/advanced-fuctions-idle-speed-control/
- Overrun fuel cut, MSExtra: https://www.msextra.com/forums/viewtopic.php?t=33238
- Decel manifold pressure: https://forums.autosport.com/topic/89377-f1-engines-and-engine-braking/
- rFactor 2 engine file, coast torque: http://tirewall.net/mh-rf2/vehicle/physics/engine_ini.html
- Link G4+ limiter, HP Academy webinar 084: https://www.hpacademy.com/previous-webinars/084-link-g4-rev-limit-configuration/
- Limiters, HP Academy webinar 171: https://www.hpacademy.com/previous-webinars/171-configuring-the-ultimate-rpm-limiter/
- Drive-by-wire tuning, HP Academy webinar 138: https://www.hpacademy.com/previous-webinars/138-tuning-drive-by-wire-throttle-control/
- MoTeC Hundred series drive-by-wire: https://motec.zendesk.com/hc/en-gb/articles/15859133831823-Hundred-series-ECU-DBW-setup-information
- Life Racing on M-Sport's R5: https://autosporttech.co.nz/life-racing/
- Bosch ME7 torque structure: https://www.rbracing-rsr.com/turbo/indian_chief/Bosch_ME7_Torque.pdf
- MoTeC M1 boost aim: https://www.hpacademy.com/courses/boost-control/motec-m1-closed-loop-boost-control-tuning/
- Link G4+ boost target: https://www.hpacademy.com/courses/link-g4-plus-software-tutorial/advanced-fuctions-closed-loop-boost-control/
- Turbo tip-in times and inertia, Chalmers: https://publications.lib.chalmers.se/records/fulltext/255844/255844.pdf
- Turbo rotor inertia and bearing friction, Perge et al. 2017 (Lubricants 5(3) 29): https://doi.org/10.3390/lubricants5030029
- Turbo bearing friction, Chung et al. 2017 (KAIS 18(1) 585): https://doi.org/10.5762/kais.2017.18.1.585
- Turbo rotor inertia of a 2.0 L engine, Chung & Lee 2016 (Trans. KSAE 24(1) 33): https://doi.org/10.7467/KSAE.2016.24.1.033
- MoTeC anti-lag: https://motec.zendesk.com/hc/en-gb/articles/14565159804687-Checklist-Anti-Lag-Setup
- Link e-throttle anti-lag: https://forums.linkecu.com/topic/14055-e-throttle-antilag-setup/
- Ignition cuts and exhaust flow, CSP's powertrain notes: https://github.com/AssettoCN/ac-docs/blob/main/docs/en/car/physics/powertrain.md
- FIA turbo restrictors, rallycars.com: https://rallycars.com/technical-stuff/fia-turbo-restrictors/
- FIA Article 254A (2013), Super 2000: https://www.fia.com/sites/default/files/regulation/file/254A%20(2013).pdf
- FIA Article 255 (2019), Group A and Super 1600: https://www.fia.com/sites/default/files/255_2019_wmsc_2018_12.05.pdf
- FIA Article 255A (2016), WRC: https://www.fia.com/sites/default/files/255a_2016-corrected_16.12.2015.pdf
- FIA Article 260 (2021), R1–R3, Rally3–5: https://www.fia.com/sites/default/files/260_2021_wmsc_2020.12.16.pdf
- FIA Article 261 (2023), Rally2: https://www.fia.com/sites/default/files/261_2023.pdf
- MoTeC M1 drive-by-wire, throttle aim as airflow (HP Academy): https://www.hpacademy.com/courses/motec-m1-software-tutorial/additional-functions-dbw-throttle/
- EPA engine benchmarking, steady maps of nine road engines (2013 Ford 1.6 L EcoBoost and others): https://www.epa.gov/vehicle-and-fuel-emissions-testing/benchmarking-advanced-low-emission-light-duty-vehicle-technology
- BeamNG.drive, modelling part-throttle torque curves: https://beamng.com/game/news/blog/throttle-torque-curves/
- Niels Heusinkveld, throttle models in sims: https://www.youtube.com/watch?v=0gW-cbSX3Uc
- BeamNG engine braking and friction, a forum reading of its engine code: https://www.beamng.com/threads/engine-braking-calculations-wrong.94673/
- Volkswagen, Polo GTI R5 0-100 km/h 4.1 s (maker's claim): https://www.volkswagen-newsroom.com/en/the-new-polo-gti-driving-presentation-2512/the-new-polo-gti-r5-gti-goes-rallying-2567
