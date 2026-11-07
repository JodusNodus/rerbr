# Championship (ours)

Rallies and seasons the player builds from presets, on any installed stage (retail, RSF, BTB), against an AI field in
their car class: the one championship mode, a better 1.02 season. Code: `crates/rbr-season` holds everything a season
needs: `config`, `pace`, `track`, `field`, `event/` (`build`, `run`, `standings`, `save`), `team`, and the 1.02 pieces
it reuses (`ai`: the AI engine, `service`: the service park and `Crew`, `rally`: `StageReport`, `tyre_choices`; `road`,
`seed`). The classic 1.02 season (`rbr-championship`) is deleted, in git history. Nothing gameplay- or physics-facing:
the layer only reads drivelines and hands the launcher times, standings and saves.

## Model

- `pace`: a quasi-static speed plan along the driveline's Hermite spline (`driveline_points`: the game's own
  Hermite through the `.trk` records, tangents fitted as the autopilot fits them; `timed_road` cuts it at the `.dls`
  start and finish and keeps the splits from the start, the one road the launcher, the examples and the tests use). Samples at ~1.5 m; speed limit `sqrt(grip_eff / curvature)` capped at top speed, the grip growing with the square of the speed on the anchor's aero; forward pass with
  acceleration falling linearly to 0 at top speed and the backward pass with braking each within what the corner leaves of the grip (the friction circle); trapezoid time integral.
- Curvature is the *signed* heading change in the ground plane (x, y; z is up in the `.trk`), smoothed over ~37 m
  (`SMOOTH` 12, the hot laps' best) before taking the absolute. Until the hot-laps check the plan took x and z, the
  height for the second axis, so it read the road's climbs and noise as corners (the stage times 13-17 % off the
  boards per stage, now 6.1-6.5 %).
- `grip_eff` = class grip x surface (tarmac 1, gravel 0.86, snow 0.85, the hot laps' stage times) x the class's `loose` on gravel and snow x
  (1 - 0.25 x wetness). Braking scales with the grip. The class table is `pace::CLASSES`, matched in order.
- `track`: a stage's `ai_ini::Track` for the 1.02 engine, from its driveline. Sections are the stage's `.dls` splits
  (the retail `ai.ini` cuts there), thirds without. A section's par is its share of the speed plan's time
  (`PAR_SCALE` 1: the engine's par draws and mistakes net out for the field's best, whose mean lands within 0.2 % of the
  reference, sd 0.002 over the retail stages). Each check describes one half: a straights check (type 3) when the half
  is 70 %+ long straights (top speed over 150 m+) or looser than tightness 0.235 (28 % of the retail halves are
  straights; this cut takes 45 % under the faster plan, two fifths of them retail straights), else curves (type 0); the value is 3 +/- how tight
  the corners hold the plan against the retail mean of its side (0.18 straights, 0.33 curves); the second half of a snow section looser than the first takes rough (type 1),
  a much looser gravel one obstacles (type 2). Material from the surface, accident table 0 (every retail row uses it).
  `StageProfile::flat` gives a stage without a driveline a reference and a middling track from its length. A BTB
  stage is timed on the `.trk` and `.dls` the game drives it with (`rbr_btb::classic::trk`/`dls` from `driveline.ini`
  and `pacenotes.ini`): over the 38 on the boards (30+ entries) its length is the board's within 0.4 % (median; the
  whole line was 6.7 % long) and the WRC 2.0 reference over the board sd 5.9/4.4/6.2 % tarmac/gravel/snow, median
  1.07/1.08/1.06 (the raw points without the spline or the trim: gravel 1.18, sd 9.7 %) (2026-11-07).
- `field`: fictional international crews (nothing from the game's roster, which stays out of the source) drive the
  1.02 AI engine (`ai::stage_time`: sections, checks, mistakes, accidents with car-part damage and injuries,
  retirements, the inspired roll, false starts 10/60/180 s then out, temperaments; wet is the engine's +9 %). Drivers
  are drawn by difficulty with means rising with it (technique 2.9 + 1.1 d, skills 2.6 + d, speed 2 + 1.4 d,
  endurance 3 + 0.8 d, spreads from the retail `aidrv.ini`'s fourteen drivers) and the retail temperament mix (mostly 3,
  then 1, 0, 4). Entrant `i` takes team `i / 2` and that team's car, as the season seats two drivers to a team; team-mates drive
  the same catalog model, the teams taking the class's models in the seed's order (the AI's skill comes from the
  roster car, its pace on each stage also from the model, `car_pace`). The
  difficulty also sets the field's `pace` over the pars (1 at 1, 1.087 at 0); adaptive moves it 0.003 a stage.
  `Entrant.skill`/`consistency` are display summaries of the driver record.
- `car_pace`: each field model's pace within the class from its NGP scripts (`rbr_formats::car_specs`: mass, drag
  area, peak power and rpm up to the limiter, drivetrain loss, driven wheels, downforce at 150 km/h). A standing start
  in a few lines of physics (drive force the lesser of traction and 0.75 of the wheel power over the speed, less drag
  and rolling) fitted to the plan's `a(v) = accel (1 - v / top)`; the corners and braking add 0.86 of the downforce as
  grip (`ClassPace::aero`, grip per (m/s)^2). `for_car` scales the class's accel and top by the car's over the field's
  mean car, so the class table stays the anchor. A stage's factor for a model is the speed plan's dry time with its
  pace over the mean car's, times the model's handling over the mean car's (`CarPace::handling`, by surface: rear
  drive, front drive, downforce per kg, fitted to the hot laps); `ItineraryStage::car_factors[e]` scales entrant
  `e`'s section pars before the engine (the engine itself unchanged) and goes in the save's S line (old saves read
  as 1). The launcher reads a pack's `common.lsp`/`r_tarmac.lsp` or the archive slot's in `physics.rbz`; a model
  whose specs do not read runs at 1.
- AI service: after a stage with a service the cars get `ai::service` (the manager's order, repairs at the team's cost
  within 20/40 minutes) and `service_penalty` (10 s per minute late) into `Event.penalties`. Cars are repaired and
  false starts reset at each rally's first stage. A driver who retires is out for the rally.
- Road order: stage 1 starts by season points (the seed's shuffle for rally 1 and ties), later stages by the rally
  standings, leader first, the retired last (`Event::start_order`); the player's place gives `road_wear` with the 1.02
  rule (front three new, 4-11 normal, rest worn). 1.02's AI comparison has no road-position effect on time, nor do we.
- Team crew: `EventConfig.team` is the roster team whose engineer and mechanics scale the player's repair times
  (`Event::crew`, `team::team_offers` describes them in words). Player's car is separate.
- Bulletins: `Event::bulletins(rally)` renders the engine's `Bulletin` and `AccidentRecord` entries in order, "SS2: name
  ...", without the manager orders (`run::news`): a driver's accidents on a stage summed into one line, totals under
  5 s left out, the first "shaken" per driver; "an uneventful stage" only from a stage's log with nothing to tell, never
  a service's. They are stored as text in the save.
- `event`: per rally one surface (Mixed as a calendar: every surface of the pool once where the rallies allow, the
  rest drawn by each surface's number of locations, shuffled) and one location (`StageRef::family`: an RSF stage's
  country, a retail map's rally `retail-<map / 10>`, a BTB stage's name), one not yet visited in the event first,
  then one with stages for every loop, the seed drawing among the best. Each leg is a loop of new stages run twice
  with the midday service between the passes (A B C, service, A B C; an odd leg's second pass a stage short), as
  real rallies run; the surface's other stages fill a small location, and a pool too small runs stages again. That
  draw is `Event::plan` (a `RallyPlan` per rally: name, each leg's stages); `Event::build_planned` builds from any plan,
  the player's own included (clocks, conditions and services by the settings, the conditions from `seed ^ 0x3EA7` so
  an edited plan keeps the weather; the config's length becomes the plan's rallies, most legs and most stages). Rally
  names from the first stage minus its variant (`Autiovaara II` -> `Autiovaara Rally`). `Length { rallies, legs, stages }`: one
  rally is an event of its own, more a championship; each rally's stages split over its legs (a leg is a day, the
  earlier legs the longer). Presets after real formats: Quick rally 1x1 leg x6 (a national one-day rally), Rally
  weekend 1x3x12 (a WRC round), Short championship 4x1x6 (a national series), Season 6x2x6; the save's old `rally:n`,
  `weekend:n`, `season:r:n` read as one, two and one legs. Each leg starts at 08:00 (21:00 at night; realistic puts the
  first day's last two stages at dusk/night), changeable weather per leg, ~20 min mid-leg services and 45 min overnight
  after each leg but the last. Points use the modern 25-18-15-12-10-8-6-4-2-1 top-10 scale per rally; seasons sum
  rally points. Adaptive moves every entrant's skill by at most 0.003 a stage toward the player finishing 2nd-4th.
  A retired player ends the rally (`restart` off): the field's remaining stages are simulated, the turn goes to RallyOver,
  no points. Super Rally (`EventConfig.restart`, on in every preset), as the WRC runs it (2005 regulations on: restart
  at the next leg; current penalty 10 min, after 5 and 7): a crew that retires restarts at the next leg, after the
  service that ends the leg (`ItineraryStage::ends_leg`, `Event::rejoin_after`), and each stage it did not complete,
  the retirement's included, counts as that stage's fastest time plus 10:00 (`Event::charged`, `RESTART_CHARGE`; the
  reference when no one finished). Only rallies of several legs have a next leg; a retirement in the last leg is final. The player's
  car is rebuilt (no park at that service); the AI crews restart the same way (`Event::sits_out`, their cars rebuilt
  at the leg's end, "restarts under Super Rally" in the news), except the hurt and those excluded for false starts
  (`AiResult::Out`, "X" in the save). Power stage
  (`EventConfig.power_stage`, the WRC's 5-4-3-2-1 since 2017, on in the presets with several legs): the complete rally's last stage pays its five fastest classified
  5-4-3-2-1 (`POWER_POINTS`, `Standing.bonus`, in `points`; a tie goes to the better placed). Points
  go to finishers only. Save is a versioned `|`-separated text format (`rbr-season 4` head, `3` read with both rules
  off: config with the team and the two rules,
  pace, itinerary with each stage's family and track, the roster's teams and costs, each entrant's driver/car state and false
  starts, results, AI penalties, bulletins), backslash-escaped, no serde.

## Calibration

`crates/rbr-season/examples/pace.rs` prints every installed stage's length and reference time per class. Retail AI
column runs the 1.02 championship AI's best driver (`rbr-season` `ai::stage_time`, level 2, dry, mean of 8
seeds) over the dry `ai.ini` rows; it matches the `times.ini` developer records within a few seconds everywhere.

- The anchor row vs the 1.02 AI's best driver, totals: gravel 0.995 (39 stages), tarmac 1.066 (12), snow 0.890 (6),
  0.996 over all; per map against the retail pars (`examples/track.rs`) gravel 1.034 sd 0.037, tarmac 1.070 sd
  0.021, snow 0.973 sd 0.045. The surfaces apart follow the hot laps, not 1.02's pars: the players in NGP hold snow
  and gravel at 0.85-0.86 of tarmac's grip (the old 0.75 and 0.50 put gravel 5 % and snow 21 % slow against tarmac).
- Stages: the boards' stage effect (Fiesta WRC 2019, the hundredth-best driver of those with 50+ times) against the
  anchor's reference over 244 installed stages: sd 6.3 % tarmac, 6.1 % gravel, 6.5 % snow; the reference sits ~10 %
  behind that driver on every surface. The acceleration and the braking share one grip budget with the cornering
  (the friction circle: no new parameters, -0.2 % held-out), and the anchor carries an effective aero of 2.0e-4
  (downforce plus the fast amateurs' commitment in the fast corners, fitted once: -0.3 % more held-out, consistent
  across surfaces, seeds and countries). Tried and left out, one line each: a pace-note limit on top of the
  curvature (the `.dls` calls, severity 0-5) gained 0.3 %; grade in the longitudinal budget no gain (grades are small
  and climb/descent cancel); standing start (0 m/s) neutral; smoothing 16 better on tarmac/snow but worse on
  gravel; a braking margin below the hardware slower on gravel/snow only and unphysical on tarmac; a crest cap from
  the vertical curvature never binds at physical scales; abs-smoothed curvature for chicanes far worse (it reads
  the records' jitter as corners); MIN_CORNER and top-speed variants dead once coupled; the road-length labels'
  noise (up to +7 % on some stages) correlates but is the labels', not the road's.
- The anchor is the `WRC 2.0` row (2003's cars, as 1.02's; plain `WRC` too). The others are fitted to the
  RallySimFans hot laps (557k players' best times on 519 stages, exported by the user, not committed): each car's
  effect from `log t = stage + driver + car` per surface over each driver's times within 10 % of that car's best on
  the stage (drivers with 5+ times; residual 2.9-3.3 %), a class's effect the mean of its cars'. `examples/class_fit.rs`
  takes each class's shape from its cars in the NGP sweep (grip, braking, the `car_pace` straight; geometric means
  over the 102 added packs since 2026-11-07, every class measured: A5 and Group 2 top out at ~0.70 of WRC 2.0, where
  the A7 and Group 4 shapes they borrowed before said 1.09 and 0.98), scales it to the tarmac effect and sets `loose`
  to the gravel and snow ones, each within 1.1 %. The anchor keeps its fitted aero; the other rows keep 0 and take
  their cars' own downforce through `car_pace`. Effects over WRC 2.0, tarmac/gravel: WRC 1.6 -3.9/-4.4 %, Rally 2 -2.5/-3.4, R5
  +0.3/-0.3, R4 +1.6/+1.5, S2000 +1.2, Rally 3 +2.7/+2.3, A8 +4.4/+4.5, N4 +6.1, Rally 4 +5.7/+6.5, R3 +6.1/+8.4,
  Group B +6.6/+7.6, RGT +6.6/+12.6, Group 4 +8.2/+8.7, R2 +8.3/+9.3, A6 +10, R1 +16/+18, A5 +18, Group 2 +21/+20
  (the old table had Rally 2 7.9 % and Group B 3.7 % behind the top class).
- Level: on the retail stages the WRC 2.0 reference is ~14 % behind the boards' best time and ~8 % behind the tenth,
  and 43 % of the entries in the class beat it (the 1.02 AI's best ~9 % behind the best); the other classes sit
  11-23 % behind their best (median over the retail stages with 100+ entries).
- Measured straight-line pace (`scripts/car-pace-sweep.sh`, analysed by `harness/car_pace.py` into
  `/tmp/car-pace/cars.csv`, not committed): every installed car (the 8 archive slots, every RSF pack) from a
  standing boot start on RSF 504's start straight (tarmac, dry), `--flat-out` holding full throttle without braking
  (`rbr-standalone --trace-speed` logs time, speed, pedals, gear and position per step), game-default manual gears
  with the autopilot shifting and the clutch help on. Columns are the 0-60/100/140/160 km/h times, the speeds at
  10/20/30 s, the top speed and the distance, all inside the flat-out window (first motion to first brake, slide
  or corner; the straight ends at the kink ~600 m out, so the 20/30 s columns stay empty).
- Measured steady-state grip and braking (same script/CSV): a radius-40 m circle on the map-85 school tarmac
  with the target speed ramping up (`--circle`), taking the fastest held speed of the first latched hold and
  v^2/R; and flat-out standstill starts on RSF 568's kilometre start straight (tarmac, dry) with full braking
  from 100 and 150 km/h (`--brake-from`), taking stopping distance and mean deceleration, crash-stops excluded.
  No radius-100 m column: no surveyed venue has a 200 m clear flat paved disc (map 85 is fenced into smaller
  compounds; other discs sit hundreds of metres from spawn through obstacles; Sosnova's files are encrypted).
  504 cannot host the 150 km/h stop (its kink and roadside trees arrive ~330 m past launch).
- Measured the same circle and the 100 km/h braking on dry gravel (`*_gravel` columns, RSF 345 Azov II, same
  approach): the circle on 345's open start area (the 113/390 school compounds beach the car in deep loose
  gravel), the braking on its 970 m start straight. A hold needs a clean approach (no beach-and-relaunch, no
  hot rejoin), angular travel on the circle and no crawl or bog-down, else its columns stay empty with the reason;
  the controller also downshifts when lugging in the measurement modes (loose ground stalls tall gears).
  `--circle-ramp RATE` overrides the circle target's 0.5 m/s-per-s rise for repeatability runs at other ramps.
  The gravel circle runs at ramp 0.4: the repeatability check re-ran 7 cars at ramps 0.4/0.5/0.6 on two gravel
  spots, and the gentle ramp is the reproducible quasi-steady one (all 7 within 4 km/h on both spots), while the
  default 0.5 is a knife-edge on gravel (neutral cars blow through to 65+ pushing, loose ones saw at 52-56).
- No-RSF cars: the same script now sweeps the added-cars folder's 102 packs (`--car-pack`/`--car-model` under NGP
  7.4.778, no archive slots, no RSF cache) on the unchanged stages (RSF 504/568/345, retail map 85) into
  `/tmp/car-pace-all/cars.csv`. Retail stages cannot host the runs (longest tarmac flat-out window ~185 m on map 45;
  no school compound like 345; no start-point option), so the RSF install stays a stage source. Over the 42 shared
  packs (same revisions) every numeric column is identical, traces byte-identical. Three light FWD cars (500 R3T,
  Fiesta R2 Mk VIII, Clio IV R3T) bog at 504's start and leave the pace columns empty.
- `car_pace` against the NGP physics' own runs (the sweep above, 50 cars): 0-60/100/140 km/h within 7.6 % RMS over
  29 cars (high-revving cars left out: the autopilot shifts near 7000 rpm, below their power peak; late launches
  too); grip on the 40 m circle 0.89-0.99 g, within 1.6 % RMS on the downforce alone (`TyrePerformancePct` and the
  mass showed no effect, so neither is in the model); braking from 100 km/h follows the brakes' hardware (5.5 % RMS),
  not modelled per car. So in the NGP physics a Group B car holds about as much at low speed as a modern one; the
  modern car's edge is its aero in the fast corners. On gravel the circle holds 0.62 of the tarmac grip on average,
  four- and two-wheel drive alike; the cars' spread there does not follow their specs (the Fiesta WRC 2019 and the
  i20 2020, alike in mass, power and downforce, 12 km/h apart; the holds fall in two groups by how long they last),
  so gravel takes no term of its own. Gravel braking 0.61-0.81 g, four-wheel drive ~6 % better, not modelled.
  `examples/car_pace.rs` prints the factors per field on the retail roads (handling included, gravel/tarmac): WRC
  field -2.3/-2.2 % (2019-20 cars) to +2.4/+2.3 % (2006); Rally2 within 0.7 %; Group B quattro -6.7/-5.5 %, Porsche
  +1.2/+0.6 %, Lada +7.1/+6.7 %; a mixed field WRC 2019 -3.7/-3.5 %, Escort Cosworth +3.3/+3.1 %.
- `car_pace` against the hot laps' car effects (42 installed cars on the boards, over the Fiesta WRC 2019; the
  `vs,` lines of `examples/car_pace.rs`): the speed plan alone 2.7/3.5/3.9 % RMS on tarmac/gravel/snow past the
  shared offset, the players losing far more in rear-driven cars (Porsche, Escort Mk II, M3, Alpine 10-17 %) and in
  old four-wheel drive than the plan. `CarPace::handling` fits log time on rear drive +4.6/+6.6/+8.3 %, front drive
  +0.3/+2.5/+1.7 % and -1.1/-1.1/-0.7 % per N/kg of downforce at 150 km/h: 1.5/2.0/2.3 % RMS left out one at a
  time. So Group B's edge over a modern car is lost on the loose (Audi S1 +4.1 % on tarmac, +5.7 % on gravel over the
  Fiesta WRC 2019).
- Retail AI checks: straights 28 % of the halves (a half under tightness 0.235 or 70 % long straights), 118/216 check
  types match the retail rows (132/216 before the faster plan; the rows are hand-set and barely follow the geometry).

## Open items

- A damp road is a flat 4.5 % on the field's pace (half the engine's wet +9 %), not a wet track of its own.
- The teams' budgets and costs stay as the roster has them for the whole season.
- The generated check types follow the geometry, not the retail ones (below).

## Generated tracks vs the retail `ai.ini`

`examples/track.rs` (reads the install's `ai.ini` at run time, nothing kept) builds each retail map's track for the WRC
class and compares (36 maps, dry rows, `timed_road`): material 108/108; section share of the stage |generated - retail|
0.010; total par generated / retail gravel 1.034 (sd 0.037), snow 0.973 (0.045), tarmac 1.070 (0.021), the same
residuals as the pace model's; `tests/retail_tracks.rs` keeps the materials and each surface's mean within 0.95-1.05; check types 118/216 and type with value within 1 114/216. The retail checks are hand-set and barely follow
the geometry (type 0 vs 3 halves have near-equal tightness and straight share), so a constant (0, 3) would score about
as well; the generator keeps the rules above for their plausibility, not their match. `examples/season.rs` runs a
three-rally season on the retail stages with a fixed player time and prints standings, bulletins and retirements.
