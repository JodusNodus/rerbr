# ReRBR experimental tyre model

A tyre model is in development and not released: the launcher's Settings page does not offer it, and `--physics
experimental-brush` runs it on top of [ReRBR experimental physics](experimental-physics.md), with its engine model,
aero and drivetrain loss (`scripts/launcher.sh --physics experimental-brush`, or `rerbr --physics experimental-brush`,
starts the launcher with it for that session; nothing saves it).

It is a brush model, the textbook physical tyre (Pacejka, *Tyre and Vehicle Dynamics*): the tread is a row of bristles
that stick to the road at the front of the contact patch and slide at the rear. It takes the place of NGP's model of the
contact patch on hard surfaces at every speed, fitted at load to give NGP's peak forces and 1.3 times its cornering
stiffness, with grip that falls as the tyre slides faster and one friction limit shared by braking and cornering. On
gravel and snow the same tyre keeps its grip as it slides, and a locked wheel builds a berm of loose material ahead
of it that brakes it harder than rolling, as measured on gravel. On snow the peak grip is NGP's. On gravel it is
NGP's sideways and 0.9 of it along the wheel, both lower on deep loose gravel, set on the 2017-21 World Rally Cars'
telemetry.

The model, its evidence, its numbers in game and the open questions are in
[notes/tyre-model.md](../notes/tyre-model.md); the code is in `crates/rbr-tyre`.

## Force curves

Each figure shows the steady force of one tyre, NGP 7's on the left (`ngp`) and the brush on the right
(`experimental-brush`), on the same wheel: a Fiesta Rally2 front wheel with NGP's car data, at 4 kN load and 15 m/s (54
km/h), dry. A positive slip angle gives a positive side force; a negative slip ratio brakes, -1 is locked. The peaks
come from NGP's grip level by design (within 1 % at 4 kN), except gravel's along the wheel; the shapes are where the
two differ. Every number below is a
row of the tyre scorecard, with its source, in [notes/tyre-model.md](../notes/tyre-model.md#scorecard). In a drive the
force builds over the tyre's relaxation length; these are the values it settles to. `harness/tyre_curves.py` redraws
them.

### Tarmac

![Side force against slip angle on dry tarmac](images/tyre-model/lat-tarmac.svg)

- Side force builds 1.3 times as fast with slip angle as NGP's: a cornering stiffness of 22.9 per rad of the load
  against 17.6. A measured semi-slick has 21.9-22.3 and a slick 27.2; no rally tyre's is published, and the estimate
  for a tarmac rally tyre is 22 to about 30.
- Side force peaks at a slip angle of 6.5°; measured road and racing tyres peak at 4.5-8.5°, NGP's at 9.5°.
- Past the peak the brush has lost 3 % by 10.5°, the gentlest of a measured racing tyre's 3-12 % there; NGP has lost
  0.3 %. No rally tyre has been measured: it is taken as no sharper than the slick.
- Fully sideways at 30° the brush keeps 0.84 of its peak, the level chosen by driving, and NGP 0.87. Road tyres keep
  0.73-0.83 and racing tyres 0.80-0.90: the brush sits where the two meet, NGP at the forgiving end.
- Braking at a slip ratio of 0.25 while cornering at 4°, the brush keeps 0.29 of its side force, above the measured
  0.23-0.27; NGP keeps 0.46.
- Braking at half its peak braking force while cornering at 2°, the brush keeps 0.97 of its side force and NGP 0.79;
  a measured road tyre keeps 0.99. At three quarters they keep 0.91, 0.63 and 0.93. At 5° and 8° the brush stays
  within 0.03 of that tyre at half, and its hardest braking there is 3-7 % over that tyre's. In a deep slide the
  brush's force points along the sliding, as that tyre's does; NGP's keeps about 1.4 times as much side force.

![Braking and driving force against slip ratio on dry tarmac](images/tyre-model/long-tarmac.svg)

- Braking and driving force peak at a slip ratio of 0.09 (NGP 0.15); measured 0.08-0.18.
- Locked, the brush keeps 0.72 of its peak braking force and NGP 0.78; measured car tyres 0.58-0.885.
- The damp tarmac tyre is the best choice on a damp road, where NGP's only ties the dry tyre: it gains 8 % over the dry
  tyre there, what NGP's wet tyre gains over the dry one on a wet road, and loses 2 % to it on a dry road. Tread
  compounds alone rate 95-120 % of a control on wet roads. From 50 km/h on damp tarmac the Fiesta Rally2 brakes
  locked at 1.07 of the dry tyre (NGP 1.00).

### Gravel

![Side force against slip angle on dry gravel](images/tyre-model/lat-gravel.svg)

- The brush builds side force more slowly the deeper the loose layer under it, and keeps it: 90 % of its maximum at
  8° on NGP's shallow gravel (9 mm, the figure), 11° on its deep gravel (17.5 mm) and 14° from 25 mm, with no fall
  to 40°. Measured: a WRC car on a stage road about 12°, a tyre on loose pea stone over asphalt 14.5° (at walking pace,
  level to 38°), a car on a firm gravel lot 3°. NGP's reaches 90 % at 7.5° on every depth and falls to 0.91 of its peak
  by 40°.
- At 10° the brush has 0.95 of its maximum on 9 mm, 0.88 on 17.5 mm and 0.80 from 25 mm (the pea stone tyre 0.80),
  NGP's 0.98.
- NGP adds the force of the soil a sliding tyre ploughs sideways, growing with the sideways speed: 0.3 g more at 30°
  of sideslip at 140 km/h. On gravel the brush holds it from 6 m/s of sideways speed, 15° at 80 km/h. The real cars'
  whole g in Finland is level from 15° to 40° of sideslip (its top tenth 1.04-1.06 at 60-100 km/h), where the game's
  median rose from 0.87 at 20-30° to 1.00 at 40-60°; with the force held it stays at 0.84-0.85.

![Braking and driving force against slip ratio on dry gravel](images/tyre-model/long-gravel.svg)

- Along the wheel the brush has 0.9 of NGP's grip. With NGP's the game's i20 Coupe WRC brakes 5-25 % harder on
  gravel than the 2017-21 World Rally Cars at their hardest in Finland, the grippiest of six gravel rallies; at 0.9
  a driver stays under the real crews. Sideways the level is NGP's, where the same drives reach the real cars' apex
  speeds.
- Deep loose gravel grips less, along the wheel and sideways: NGP's level to 15 mm of loose layer, falling to 0.96 of
  it at 30 mm. NGP's loosest gravel is then 10 % under its firmest (6 % in NGP), where the six rallies' hardest
  braking spreads 5-14 %.
- Braking force levels off from a slip ratio of about 0.15 (0.25 on deep layers) and holds to 0.8. Past that the wheel
  pushes a berm of gravel ahead of it, larger the deeper the loose layer: locked it brakes 1.05 times its rolling
  peak on 9 mm (the figure), 1.18 times on 30 mm and 1.3 times from 5 cm, the depth of the published stops. Measured
  cars on gravel stop 1.12-1.59 times harder locked than with ABS; the Fiesta Rally2 under the brush 1.28-1.37,
  under NGP 1.08-1.10.
- Driving force levels off and does not fall at full spin, where NGP's falls to 0.82 of its peak. No measured curve on
  gravel reaches full spin, so this part is open.
- Damp gravel grips as dry gravel, where NGP's loses 7.5 %: the one gravel road measured dry and sprinkled skidded as
  hard or harder wetted, and no measurement shows a damp loss. Wet gravel keeps NGP's 0.85 of dry, inside the 0.76-0.90
  measured on gravel roads. From 50 km/h the Fiesta Rally2 brakes locked on damp gravel at 1.02 of dry (NGP 0.93).

### Snow

![Side force against slip angle on snow](images/tyre-model/lat-snow.svg)

![Braking and driving force against slip ratio on snow](images/tyre-model/long-snow.svg)

- Side force levels off from about 12° and does not fall; its maximum is at 14.75° (NGP 11°), measured on packed and
  loose snow 9-18°.
- The berm on snow is small: locked, the wheel brakes 1.05 times its rolling peak. Studded tyres on compacted snow
  lock at 1.02-1.10 times their braking at 15-20 % slip; the Fiesta Rally2 under the brush 1.06-1.10.

## Beyond the curves

- **Temperature.** On hard surfaces the brush runs its own tyre temperatures: half of the sliding power heats a thin
  surface layer, which loses heat to the air, the road and the tread under it, and the grip follows NGP's rubber curve
  at those temperatures. The half is an assumption inside the published 0.49-0.68. Cornering steadily at the limit
  the grip settles within 1 % of NGP's; a long slide heats the surface and makes the car harder to catch. On gravel
  and snow NGP's heat stays.
- **Pressure.** More pressure makes the brush build force more slowly at light load, 5.7 % per 0.5 bar at 4 kN, and
  not at all from 6.3 kN up, as measured tyres do up to about one and a half times their rated load. NGP's builds
  force faster with pressure at every load. Every car's default is the 2.0 bar both models agree at, so this shows
  only with a changed setup.
- **Ice.** NGP's snow tyre is studded. On bare ice the brush gives it the grip it has on the stages' snow (0.85, an
  assumption: road studs measure 0.21-0.30, rally studs are not published) and leaves every other tyre at NGP's 0.10,
  where measured road tyres are at 0 °C (0.09-0.12). NGP gives every tyre the 0.10.
