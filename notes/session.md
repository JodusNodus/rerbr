# Sessions (ours, planned)

The layer above the 1.02 stage loop that lets a point-to-point rally stage, a hotlap, a lap race and a school lesson be
the same kind of thing. Code: `crates/rbr-session` (`course`, `end`, `event`, `lap`), on top of `rbr-gameplay`, which keeps the 1.02 loop
bit-exact and is the `RallyStage` format's engine. `rbr-session` depends on `rbr-gameplay`, not the reverse: the
stage loop calls the `Format` trait, so the trait lives in `rbr-gameplay` and the formats of `rbr-session` implement it.

## Words

| Term | Meaning | Examples |
| --- | --- | --- |
| Course | The geometry: the drive line, its gates (start, sectors, finish) and whether it is open or closed. | Gabiria (open), Nordschleife (closed) |
| Format | The rules of a session: how it is timed and ends, what is recorded, what the HUD shows. A trait. | `RallyStage`, `Hotlap`, `Practice`, `Laps(n)`, `Lesson` |
| Session | One running instance: a course, a format, a car and conditions. It produces a result. | one stage run, one hotlap session |
| Mode | The player-facing product that strings sessions together. | Quick rally, Championship, Rally school, Time trial |

- `rbr_gameplay::flow::FlowMode` (was `Mode`) is the original's flow word (`*0x165fa90`), not our mode. It stays as
  the compatibility word the `RallyStage` format sets for the recordings; new code never says "mode" for it.
- A rally stage is the `RallyStage` format on an open course: today's `TimeKeeper`, `MarkerTicker`, `FinishZone` and
  `RaceFlow`, bit-exact, special-cased nowhere. A hotlap is `Hotlap` on a closed course (or one run on an open one).
- The championship (`rbr-season`) is a Mode running a sequence of `RallyStage` sessions.
- The `Format` trait (was `ModeRules`, `rbr-gameplay/src/format.rs`; `FormatEnd` was `ModeEnd`, `EndCause::Format`
  was `Mode`) stays in `rbr-gameplay` because the stage loop calls it. Its hooks observe the drive and may end it;
  `rbr-school`'s `SchoolRun` implements it, the circuit's will from `rbr-session`.

## Why

- `FlowMode(u32)` is tested by `matches!` (`finish_to_menu`, `camera_go`, `confirm_arms`): our modes would have to
  take numbers in the original's table.
- `ModeRules` (now `Format`) could end a drive but not own timing, finish conditions, HUD readouts or what is saved.
- Timing, finish, `RaceFlow` (three splits), `StageResult`, `record.rs` (keyed by stage id) and the ghost are all one
  start, N splits, one finish.
- Progress is the car's distance along an open drive line (car `+0x17c`, searched in
  `rbr-physics/src/ngp/drive_line.rs`); the line holds at most 10000 entries, so unrolling laps cannot work (the
  Nordschleife is about 8300 at 2.5 m).
- A single lap already runs as a stage: the converter starts a loop 50 m before the start line and finishes 90 m before
  it (`beamng-rbrbtb` notes; the autopilot's Nordschleife run is 913.8 s).

## Plan

Each step keeps the recordings bit-exact (`scripts/rust-check.sh`, `rsf_compare` physics PASS 84084/84084); the
existing types are built beside, not rewritten, since `FlowMode`/timing/ghost types reach 5 crates and 42 `rbr-render` files.

1. Skeleton, no behaviour change (done): `Course`, `TimingEvent`, `EndRule`, `LapTiming`; the timekeeper's
   checkpoints reach the host as events. `SessionKey { stage, kind }` comes with the records (step 3).
2. `ModeRules` becomes `Format` (done: a rename; the school was its only user and fits unchanged).
3. Records and ghost keyed by `SessionKey`; today's rows are `kind = stage`.
4. Circuit: lap timing, `EndRule` for N laps and free practice, best lap, sectors, a per-lap ghost, the HUD panel, the
   launcher entry.
5. Closed drive line (done): NGP's search wraps at the seam on a line marked closed in its data, below. The only
   step in physics code; the audit is under "Progress value".
6. Converters (`beamng-rbrbtb`, `assetto-rbrbtb`) write the course key, gates and sectors, the Nordschleife first.

## Decisions

- Scope: the full session layer (not the timing seam alone), in a new crate `rbr-session`.
- A circuit is marked by an explicit key in the stage (`track.ini` / course key: kind, laps, gates), not detected from
  the line (converters write it).

## Progress value (audit for step 5)

- Written only by the physics' step, from its previous value as the hint: `engine_driveline::layout::project_point`
  (1.02, `CDriveLine::SampleBig` 0x50b180) and `ngp::drive_line::project` (NGP), each a window of entries around the
  hint, clamped to 0 and `n - 1`. The seam handling (modular window, wrap) goes in both.
- Read (as a distance through `driveline::sample_x`, or `dot150`, the speed along the line): the timekeeper
  (`Game::record_step`), the marker ticker (`Game::tick_markers`), the HUD/telemetry/picture (`rbr-standalone`
  `live`), the autopilot (its inverse), replay playback, the school's lessons (`Format::drive_line_at`) and the
  scripted characters (`rbr_render::characters`).
- Nothing found that reads it in the help-call respawn path (`CSigRespawnRequest` is the physics' reposition); not
  traced to the end, unverified.
- On a wrapping line everything keyed by distance and fired once needs re-arming each lap: the marker keys' fired
  bits (pace notes), the scripted characters' events. The timekeeper is not used for a circuit (`LapTiming`).

## State

- Done: `flow::Mode` renamed `FlowMode`; `ModeRules` renamed `Format` (step 2). `rbr-session` has `Course`,
  `EndRule`, `TimingEvent` (a 1.02 `Checkpoint` converts to one; `SubstepEvent::checkpoint` and `elapsed` carry what the
  timekeeper saw out of `Game`; `StepView::elapsed` hands the time to formats), `LapTiming` (laps, sectors, one-shot,
  N-lap or timed end; pass times interpolated between steps) and `LapRules`, the circuit's `Format`. A test runs
  `Game` + `LapRules` for two laps on a synthetic closed line (distance wrapping at the seam, as NGP's search now makes it,
  from a line marked closed): `Started`, a sector and a lap each lap, `Finished`, the drive ended with `EndCause::Format`.
- Step 5, the closed line: a `.trk` drive line whose first record's two words are `trk::CLOSED_LINE_WORDS` ("LOOP",
  the entry's tag word, which nothing in the original reads) is closed: its last record repeats the first (position and
  tangent) and `ngp::drive_line::project_closed` runs over its `count - 1` segments, the window and the segment before
  the car wrapping at the seam (`Line::closed`, `StageStore::line_closed`, the tag read from the image and the typed
  stage). The progress value then runs `0..count - 1` and comes in at 0 again; the distance (`sample_x`) drops by the
  lap length at the line. NGP's search only: 1.02's `SampleBig` (`project_point`) is not changed, a closed line under
  it behaves as an open one. Open lines take the old path untouched. Checks: unit tests on a ring (`ngp::drive_line`),
  `tests/ngp_closed_line.rs` on the image, `scripts/gate.sh --hybrid --typed` PASS (84084/84084, 13 of 13 snapshots
  equal, the typed twins equal), vectors ok. No original drive of a closed line exists: its behaviour is ours, checked
  against the ring only.
- A circuit plays (headless): a BTB stage with `circuit = 1` in its `track.ini` (`TrackInfo::circuit`; `classic::trk`
  writes the closed words on the first record) whose `driveline.ini` is a closed ring (the last point the first's copy)
  is a circuit. `rbr-standalone --boot ... --laps N|open` builds `LapRules` from `Course::load(markers, line)` and
  runs it beside the school's rules (`Format` for `(A, B)` and `&mut R`; `Live::take_lap_events`). The lap line is the
  spawn: the line starts where the car stands, a standing start (`LapTiming::start_now`, from `Format::start`), the
  seam is the lap line. The stage's own start, finish and end keys lie far past the lap (40000 m) so the 1.02
  timekeeper and marker ticker never fire them (the start line's jump start would, with the car past the key).
  The autopilot follows an unrolled copy (`StageData::line`, 3 laps) and maps the wrapping progress onto it
  (`Driver::with_period`).
- Nordschleife (local conversion, `~/.local/share/rerbr/converted/.../Nordschleife`, originals in
  `~/.local/share/rerbr/backups/nordschleife-pre-circuit/`): the open 8318-point line closed by 4 points over its 11 m
  gap and rotated to start at the spawn (K20, 50 m in), 8322 points, 20803.22 m; the pace notes shifted by 50 m;
  start, finish and end moved to 40000 m. Autopilot (NGP car 2, `--top-speed 50`, `--laps 1`): Started at step 1520,
  sectors at 323.39 s (6966.9 m) and 524.55 s (11533.8 m), lap 944.4474 s (step time), then `Finished` and the race
  ended with cause `Format` (race clock 944.7107 s, the HUD's 1.02 splits 316.98 and 520.30 s are the race clock at the
  ticker's split events, not the laps' sectors: they disagree by seconds, unexplained; the laps' times follow the
  step time and the distances). Two laps: see below.
- Not yet: records and ghost by `SessionKey` (step 3); the HUD panel, the per-lap ghost and the launcher entry (step 4's
  rest); the converters write the closed ring (step 6).
- Open: the season's pace plan on a closed line (it reads the progress as an open one); per-lap re-arming of the marker keys and the scripted events; a circuit's start and end in the 1.02 loop (its timekeeper's start and finish
  keys: the finish must lie past the line so the stage never finishes by itself; the stage's end page and result for a
  `Format` end are the race clock's).
