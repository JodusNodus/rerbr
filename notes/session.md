# Sessions (ours, planned)

The layer above the 1.02 stage loop that lets a point-to-point rally stage, a hotlap, a lap race and a school lesson be
the same kind of thing. Code: `crates/rbr-session` (`course`, `end`, `event`, `lap`), on top of `rbr-gameplay`, which keeps the 1.02 loop
bit-exact and is the `RallyStage` format's engine. `rbr-session` depends on `rbr-gameplay`, not the reverse: the
`Format` trait's hooks see the step's types (`StepView`, `CarControl`), so it lives in the upper crate.

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
- `ModeRules` (`rbr-gameplay/src/mode.rs`, observe-only, one user: `rbr-school`) becomes the `Format` trait and goes
  once the school has moved over.

## Why

- `FlowMode(u32)` is tested by `matches!` (`finish_to_menu`, `camera_go`, `confirm_arms`): our modes would have to
  take numbers in the original's table.
- `ModeRules` can end a drive but not own timing, finish conditions, HUD readouts or what is saved.
- Timing, finish, `RaceFlow` (three splits), `StageResult`, `record.rs` (keyed by stage id) and the ghost are all one
  start, N splits, one finish.
- Progress is the car's distance along an open drive line (car `+0x17c`, searched in
  `rbr-physics/src/ngp/drive_line.rs`); the line holds at most 10000 entries, so unrolling laps cannot work (the
  Nordschleife is about 8300 at 2.5 m).
- A single lap already runs as a stage: the converter starts a loop 50 m before the start line and finishes 90 m before
  it (`beamng-rbrbtb` notes; the autopilot's Nordschleife run is 913.8 s).

## Plan

Each step keeps the recordings bit-exact (`scripts/rust-check.sh`, `rsf_compare` physics PASS 84084/84084); the
existing types are built beside, not rewritten, since `Mode`/timing/ghost types reach 5 crates and 42 `rbr-render` files.

1. Skeleton, no behaviour change (done, but for the `Format` trait): `Course`, `TimingEvent`, `EndRule`,
   `LapTiming`; the timekeeper's checkpoints reach the host as events. `SessionKey { stage, kind }` comes with the
   records (step 3), the `Format` trait with the school (step 2).
2. The school onto `Format` (the only `ModeRules` user: the test that the interface is enough).
3. Records and ghost keyed by `SessionKey`; today's rows are `kind = stage`.
4. Circuit: lap timing, `EndRule` for N laps and free practice, best lap, sectors, a per-lap ghost, the HUD panel, the
   launcher entry.
5. Closed drive line: an opt-in wrap of the search at the seam (`ngp::drive_line::Line` is the seam), on the explicit
   key below. The only step in physics code: hybrid check. First audit what else reads the progress value (help-call
   respawn, outside assistance, pace notes, `dot150`), unchecked so far.
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
  `live`), the autopilot (its inverse), replay playback, the school's lessons (`ModeRules::drive_line_at`) and the
  scripted characters (`rbr_render::characters`).
- Nothing found that reads it in the help-call respawn path (`CSigRespawnRequest` is the physics' reposition); not
  traced to the end, unverified.
- On a wrapping line everything keyed by distance and fired once needs re-arming each lap: the marker keys' fired
  bits (pace notes), the scripted characters' events. The timekeeper is not used for a circuit (`LapTiming`).

## State

- Done: `flow::Mode` renamed `FlowMode`. Step 1's data and timing: `rbr-session` has `Course`, `EndRule`,
  `TimingEvent` (a 1.02 `Checkpoint` converts to one; `SubstepEvent::checkpoint` now carries it out of `Game`) and
  `LapTiming` (laps, sectors and a one-shot or N-lap or timed end, tested on synthetic drives; pass times are
  interpolated between steps).
- Open: the course key's name and file; the `Format` trait's hooks (step 2, with the school); a circuit's start and
  end in the 1.02 loop (its timekeeper's finish key must lie past the line so the stage never finishes by itself, and
  the session ends the drive through the mode's end).
