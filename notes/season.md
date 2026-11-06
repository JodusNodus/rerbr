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
  start and finish and keeps the splits from the start, the one road the launcher, the examples and the tests use). Samples at ~1.5 m; speed limit `sqrt(grip_eff / curvature)` capped at top speed; forward pass with
  acceleration falling linearly to 0 at top speed, backward pass with braking; trapezoid time integral.
- Curvature is the *signed* heading change, smoothed over ~19 m before taking the absolute: the records' lateral
  jitter weaves left and right and cancels out, while real corners keep their sign. Smoothing the absolute
  curvature instead counts every weave as a corner (found on map 61: 69 rad of smoothed-signed turning vs ~2.5 rad
  net heading change).
- `grip_eff` = class grip x surface (tarmac 1, gravel 0.75, snow 0.50) x (1 - 0.25 x wetness). Braking scales with
  the grip.
- `track`: a stage's `ai_ini::Track` for the 1.02 engine, from its driveline. Sections are the stage's `.dls` splits
  (the retail `ai.ini` cuts there), thirds without. A section's par is its share of the speed plan's time
  (`PAR_SCALE` 1: the engine's par draws and mistakes net out for the field's best, whose mean lands within 0.2 % of the
  reference, sd 0.002 over the retail stages). Each check describes one half: a straights check (type 3) when the half
  is 70 %+ long straights (top speed over 150 m+), else curves (type 0, value 3 +/- how tight the corners hold the plan;
  tightness 0.15 is the retail mean); the second half of a snow section looser than the first takes rough (type 1),
  a much looser gravel one obstacles (type 2). Material from the surface, accident table 0 (every retail row uses it).
  `StageProfile::flat` gives a stage without a driveline (BTB) a reference and a middling track from its length.
- `field`: fictional international crews (nothing from the game's roster, which stays out of the source) drive the
  1.02 AI engine (`ai::stage_time`: sections, checks, mistakes, accidents with car-part damage and injuries,
  retirements, the inspired roll, false starts 10/60/180 s then out, temperaments; wet is the engine's +9 %). Drivers
  are drawn by difficulty with means rising with it (technique 2.9 + 1.1 d, skills 2.6 + d, speed 2 + 1.4 d,
  endurance 3 + 0.8 d, spreads from the retail `aidrv.ini`'s fourteen drivers) and the retail temperament mix (mostly 3,
  then 1, 0, 4). Entrant `i` takes team `i / 2` and that team's car, as the season seats two drivers to a team; team-mates drive
  the same catalog model, the teams taking the class's models in the seed's order (the AI's pace comes from the
  roster car, the model is the entry's label). The
  difficulty also sets the field's `pace` over the pars (1 at 1, 1.087 at 0); adaptive moves it 0.003 a stage.
  `Entrant.skill`/`consistency` are display summaries of the driver record.
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

- WRC/top class vs the AI, totals: gravel 0.992 (39 stages), tarmac 1.009 (12), snow 0.934 (6); median stage 0.97.
- Other classes follow by power/weight/grip ratios (WRC 9.6/9.2/11.5/58 down to Group 2 6.8/4.5/8.0/43).
- Real-world check: fast gravel WRC ~109-126 km/h, twisty tarmac ~89-96, snow ~74-100; Rally2 ~7 % slower,
  Group 2 ~21 % slower.
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
- Residuals are the method's, not the constants': the driveline is a lossy record of the road (dense drivelines
  keep positioning weave the pars ignore; sparse ones cut corners the pars charge for), and the AI pars are
  hand-tuned per track. Outliers: maps 61/66 (fast per the AI, twisty per the driveline) and 54/56 (slow per the
  AI, cut per the driveline), each ~30-60 % off while the middle half of stages sits within ~10 %.

## Open items

- A damp road is a flat 4.5 % on the field's pace (half the engine's wet +9 %), not a wet track of its own.
- The teams' budgets and costs stay as the roster has them for the whole season.
- BTB stages have no driveline: `StageProfile::flat` from the length, so their pace is an estimate.
- The generated check types follow the geometry, not the retail ones (below).

## Generated tracks vs the retail `ai.ini`

`examples/track.rs` (reads the install's `ai.ini` at run time, nothing kept) builds each retail map's track for the WRC
class and compares (36 maps, dry rows, `timed_road`): material 108/108; section share of the stage |generated - retail|
0.036; total par generated / retail gravel 1.038 (sd 0.230), snow 1.043 (0.154), tarmac 1.018 (0.091), the same
residuals as the pace model's; `tests/retail_tracks.rs` keeps the materials and each surface's mean within 0.95-1.05; check types 130/216 and type with value within 1 117/216. The retail checks are hand-set and barely follow
the geometry (type 0 vs 3 halves have near-equal tightness and straight share), so a constant (0, 3) would score about
as well; the generator keeps the rules above for their plausibility, not their match. `examples/season.rs` runs a
three-rally season on the retail stages with a fixed player time and prints standings, bulletins and retirements.
