# Gameplay

The 1.02 gameplay around the physics step: input, the frame loop, race flow, options, weather, scripted events,
championship and saves. Code: `crates/rbr-gameplay/src/` (module map:
[crates/rbr-gameplay/README.md](../crates/rbr-gameplay/README.md)); each section names the module that states the
detail. File layouts are in [formats.md](formats.md); cameras and HUD in
[cameras-hud-launcher.md](cameras-hud-launcher.md). Addresses are `RichardBurnsRally_SSE.exe` 1.02. Detail dropped
here: rbr-recomp `notes/gameplay-loop.md`, `notes/scripted-season.md`, `notes/weather.md`.

## Frame and fixed step

Mechanics in `tick.rs` (clock, clamp, whole steps, `facc`, settle counts) and `game.rs` (frame order, per-substep
work); 7 ms steps and the 160 settle steps in `rbr_recording::clock`. Check: `tests/schedule.rs` (all four drives).

- The step is 0.007 s from retail `Root/physics.rbz` (the CHELALIC `physics.dat` copy's 0.01 is not loaded).
- The frame's physics call is the physics' `Vf0C` (0x526a10).

## Control payload

Words, pack bytes, flag map and the NGP handler in `control.rs` (`QuantizedControl::pack`, `car_control`,
`FLAG_MAP`, `CarControl::ngp`); the car side in `rbr-physics/src/vcu/` (`layout.rs` `read_packet`); FRM1 in
`tests/recordings.rs`. Words: throttle, brake, handbrake, clutch, steering (float), flags, gear, aux.

- Measured: throttle 1.0 speeds the car up; brake 0 to 1 (`control.rs` cites this note for both).
- Steering comes back out through `Unpack` (0x48c9c0): `byte * 2 / 255`.

### Device poll

Mechanics in `poll/mod.rs` (devices, actions, `GetActionValue` sets, `Changed`, `Invert*Axis`, steering cells, emit)
and `poll/wine.rs` (Proton path, G923 lattice, rest values); NGP additions in `control.rs` and `notes/ngp.md`. Check:
`rbr_render::bin/input_check` (RSF sessions, "Input chain check").

- Ours, not the game's: a pedal's `Invert*Axis` applies only when its action is bound to an axis that has moved twice.
  The game flips a settling axis's 0 to 1 (pedal pinned until touched), and a released button's or the combined
  axis's part too. Settled axes are unchanged. Tests: `an_inverted_pedal_reads_zero_until_it_has_moved_twice`,
  `invert_leaves_a_button_pedal_alone`.
- NGP version addresses (code states 778's only): 779 polls at 0x10006e70, its direct gears at 0x1003c4d0. The strand
  the 1000 Hz thread posts to is 0x10098270 -> 0x10094200 (not in code).
- RBRControls (the install's) sets properties and FFB on device instances of its own (MOD1: PRP1 9, DEV1 5, no POL1); the
  chain replays exactly without it.
- Live: checked on cachy (autopilot, `rec/input-own`): G923 and T500 RS found, read at rest (0, gears bound). The
  keyboard state is the frame's held keys: a sub-frame tap is missed (unverified).
- No import of the install's controls or settings (user decision: the player sets them again); removed with
  `rbr-launcher/src/rsf_import.rs`, `rbr_formats::wine`.

### Input filter

Mechanics in `filter.rs` (grid, curve, `Step`, `Pack`, `notify_a`, `Reset`, profile grid, `without_wheel_fade`,
`is_made_up`) and `ini.rs`; the NGP-path-only live rule in `rbr-standalone/src/live/mod.rs` (`Grid::from_profile`,
`new_game`). Checks: `tests/filter_peek.rs` (grid and zero-input path per step), `tests/filter_vectors.rs` with
`harness/vector_specs/gameplay_filter.py` (curve and `Pack` fuzzed). `game.rs` takes every live control through
NGP's handler (`CarControl::ngp`); a replay entry still goes through the quantizer's `NotifyFilt`. The 1.02
`ControlPath`, the single-step gate and the unread event copies (`generated_control`, `checkpoint`, `marker_fired`,
`time_card`, `elapsed`, `clock`) are removed.

- RSF renames the install's file to `PC_InputFilter.ini.bak`, leaving the built-in grid, whose digital steering runs at a tenth of the retail file's rates (not in code).
- Live play takes the profile grid from `--saves`/`--profile` on the NGP path only; FRM1 holds the payload after the filter, so `--boot` never runs it.

## Stage states

Machine in `stage.rs`, the go in `camera_go.rs`, the live accept in `rbr-standalone/src/live/mod.rs`
(`ACCEPT_MESSAGE`). Check: `tests/stage_peek.rs` (every step of `peek/g7-tanner-corolla`).

- The mode rule (`M8840` 0x4888bf) and the 2.0 s arm are asm only: no recording starts a championship stage.

## Timing and markers

Mechanics in `markers.rs` and `timing.rs` (keys, `CTimeKeeper`, `Record`, f32 clock). Checks: `tests/timing.rs`
(tanner-corolla steps), `tests/timing_peek.rs` (every field, every step, two drives).

- Between the six fired-bit checkpoints (tarmac-lancer, arctic-snow, tanner-damage) unchecked: the snapshots' key
  list is not read. The old `tests/game_loop.rs` assertion is removed with the test (it needed 1.02's f32 clock).

## Race clock and start

Mechanics in `race_clock.rs` (countdown, go, jump start, penalties, outside assistance, HELPCALL). Check:
`tests/race_clock_peek.rs` (three peeked assist drives, bit for bit).

## Race end

Mechanics in `flow.rs` (flow words, finish, replay limit, retire codes, end pages, restart), `result.rs` and
`pause_menu.rs` (page 1 has RESTART, page 2 none; no OPTIONS item is ours). Checks: `tests/` drive expectations,
`live_restart_answers_each_frame_again` in `rbr-standalone/src/live/tests.rs` (500 autopilot frames answer byte for
byte again).

- tanner-corolla runs 874 steps past the finish with the clock stopped (evidence for the 20 s timer rule).
- tanner-corolla: finish 233.67441 s, splits 69.84889 and 143.56372 s, the go frame getting no time; boot and live give it a step (233.68141, 69.855896, 143.57072 s).
- Full replay buffer (`ReplayRuns`), not in code: `E19ClsScan` (0x499ff0) keeps runs of equal nine-byte controls
  (max 0xffff steps each); at run index 0x20f56 `E19QccFire` (0x49af10) emits no control, the step still runs, then
  `TicPhysicsRecording` (0x47f4fb) emits `CSigEndRace` with 1 and zero times (a control changing every step fills
  it after 945 s). NGP replaces both calls (0x47f4c3, 0x47f4ce) with a growing buffer. Unreached by any recording:
  unverified.
- Ours: a race left early (`RaceFlow::end_now`) ends with cause `Left`.
- Ours (user decision, 2026-11-04): no time limit. The game retires the race at 1800 s (`0x746828`), which cut off
  long community stages; a ghost records to 4.5 h (its 24-bit stamps), not the game's 0x4b000-byte buffer (about
  1024 s). A retired race still offers 1800 s (`record::RETIRED_TIME`).
- Not compared with an original ghost (outside-assistance penalty in the ghost clock).

## Scripted events

Gameplay effects in `stage.rs`/`game.rs` (HELPCALL path, `CSigCarProblem`, stuck test) and
`rbr_render::characters::scripted` (flip spectators, helicopter, bird hit, marshals); randomness in the race clock
path (`--seed` is the viewer's). Evidence drives: `drive-rcv-help-corolla-34`, `-stuck-206-64`,
`-retire-impreza00-45` (checks in `rbr-gameplay/tests/race_clock_peek.rs`,
`rbr-render/tests/scripted_data.rs`).

- A call for help while moving (the motion probe: faster than 1.5 m/s) emits `CSigCarProblem` kind 4 (MSG_CARSTOP)
  instead of the assist. Its emit (0x4e8a70) is bound for the pure step; no recording reaches it (live boot check:
  `a_call_for_help_while_moving_asks_to_stop_and_the_drive_goes_on`, 14.7 m/s on track-11_O).
- The flip spectators are five `load_car_flipN.lua`, cosmetic; `run_car_flip.lua` never runs.
- Bird strike feeds only the sound. Crash-site scripts are unused in retail.

## Rally school

Mechanics in the `rbr-school` crate (lessons, the `.dls` `Driveline` filter bank, its listeners, `EvalFilters`,
the awards), a mode on the stage loop: `SchoolRun` is a `rbr_gameplay::mode::ModeRules` the host passes to
`Game::frame_with` (the start, the control before each substep, the step's emits, `Tic570` each frame). The host
owns it, and the live hook and the profile write are in `rbr-standalone` (`--school N`, `live::race_end`). Checks:
`rbr-school/tests/vectors.rs` (direct-call vectors of the bank's methods, records in `gameplay_school/`) and an
autopilot drive of lesson 2 (stops in the box, verdict 3, profile written). No original school drive is recorded
yet: the oracle run is open.

- Car (ours, user decision 2026-11-04): the original drives car 5 with the lesson's setup (`Lesson::setup_file`).
  `--school` takes any `--car` or `--rsf-car`, car 5 by default; like the original the lesson's setup goes over
  slot 5 without a pack, whatever car it holds (`Race::school_setup`), any other car keeps its default setup on the
  lesson's tyre. Lesson 2 autopilot over the 8 retail slots and 84 RSF packs: every drive ends by the school
  (verdicts 0, 3 or 4 by the autopilot's time; car 2 at 10.10 s fails).

- Ported and vector-checked but with no caller in ours, removed in f9eb0e60 (restore from its parent): the sub
  primitives the listeners use (`Classify`, `Category`, `Pick`, `Over`/`Over2`/`Over3`, `InRange`, `Init`, `Set7`,
  `SetThresh`, `SetLim`, `MaxStore`), `InitState`, `Publish`, `RallySchoolSound` and the latch accessors. The
  failure page needs only `FlagMap` and `Get20`/`Get24`/`Get28` (restored, vector-covered): its draw calls nothing
  else (verified against the disassembly below).
- Failure reasons (`rbr_school::explain::failure_reasons`, listed under the HUD's Failed): `Vf0C` 0x542330 over the
  bank, verified against the 1.02 disassembly where the reconstruction (`RBR_E52_X87.cpp`, 26% matched) diverges:
  title `FAILURE1`, advice `SCOM_FAIL1-4` by `rand & 3` (`Vf04` 0x542aa0; neither picked from the result); per sub
  `FlagMap` 0/1/2 reads `lim0`/`lim1`/`lim2` (other values skip: `dec; jne`), `q >= 0` names the lesson's own
  string-table key at that offset (`BlockAddB` 0x475c90, block base 0 in ours), `q == -1` skips, else the sub's tag
  (`m_00`) 1..8/1..5/1..5 names a built-in (`m_str8[9..16]` `BRKFAIL`..`NOHBFAIL`, `[17..21]` `BRKEARLY`..`HALTEARLY`,
  `[22..26]` `BRKLATE`..`HALTLATE`; the reconstruction's `m_str8[6..13]` and its switch on the limit are wrong);
  each tripped check (`state` +-1) with `code > 0` names its own key; results de-duplicated, at most 7; when timed
  (all three ladder words ordered-nonzero, `IsZeroAny` 0x4981a0) a race time over the limit names `TIMEFAIL`, else
  over the pass `TIMEBAD`, appended without a scan. `FAILHELP` and `OOBFAIL`/`OOBYAFAIL`/`OOBSTFAIL`/`OOB_TTFAIL`/
  `CRASHFAIL` are set but never resolved here (the out-of-bounds ones feed the fade's warning instead).
- The race time the time pair compares (`DAT_0165fc68+0x140`) is inferred to be the HUD time; ours passes the
  end's race time. Every retail lesson's limits and check codes resolve to keys in the English table, as does
  every built-in but `HALTEARLY` (absent in 1.02 English, skipped at runtime like the original's null `Resolve`).
- `SCH1` carries the failed end's reason keys after the speech ids (old bodies still read); the standalone fills
  them once the verdict is known and failed, the viewer resolves them through `bstrings.eng` (the `HudModel`
  keeps the table a school drive hands it) and the verdict panel lists each text under Failed.
- Lessons 0 and 10 (free practice, "yellow rectangle: end lesson") end standing in the box within 2 m (each axis)
  of `CDriveLine::Sample(m_00)` on the object `0x1659184` points to (fixed 2026-11-04: read as the table it gave
  the origin and they never ended). Both start facing the box, before the start on a two-record driveline; lesson
  10's map has no finish and its card a finish time from the first step.
- Lesson sweep (autopilot, car 5, an NGP car 2/4/6 and an RSF pack): every lesson ends, by the school, a retire
  latch or the finish marker; 0, 1, 2, 10 and 11 pass, the others fail on the autopilot's time or lines. Slots with
  no NGP car (here 0, 1, 3, 7) start the free lessons with the engine off; the launcher does not offer them. Lesson
  1 passes since the autopilot rides the playground fence's edge, holds its 90 km/h band and brakes hard for its
  800 m kink; lesson 8's 15 s and 85-115 km/h band stay out of the test pilot's reach (ends with a recorded fail).
- `SchoolIncident` reads its lead through the sub's words whatever it points at: a check lead is armed by its
  bounds (`IsArmed` on +0x14/+0x18). Inferred: no record has an untripped check lead with a zero bound.

- Flow mode 7: the lesson is the flow's +0xca4, its step counter +0xca8 (2 in a timed lesson's load, 4 while
  driving); +0xcb4 is set by the untimed lessons' (0, 4, 10, 13, 14, 16) load only (`M5693f0` cases 2 and 4).
- No countdown: in state 10 mode 7 arms neither the camera orbit (`M8840`, modes 1 and 2) nor the confirm (3 to 5);
  the confirm drives at once (`Stage::school_start`).
- Listeners: the control payload's words 0 to 2 (`CbF30`) before each substep; in the step the brake-wall emits
  (`CbF10`, only segments with index bit 14; flag bit 2 is the crossing), the general state (`OnDrive`), the
  driveline float (`Cb930`, not in state 10); `Tic570` runs `EndRace` each frame and emits a retire's
  `CSigEndRace` once a trip latch is set. A retire sets latch B (`M5662c0`).
- Verdict (`EvalFilters`): a latch gives 0, a no-go sub (flags 4) done gives 0, then the time ladder
  (`perfect_time`, `pass_time`, `time_limit`), the speed checks, then `SeverityMax` over the 1.02 repair objects;
  lessons 4 and 16 leave the damage out.
- Awards (case 8): a fail keeps the results; a pass gives every earlier lesson without a result the same grade and
  raises the lesson's own (profile +0x424, `Profile::school`); the ride (lesson 5) gives 4. A school drive writes
  no stage records (inferred).
- Demonstration and ride (`rse%d.arp`, `ghosts.rbz`) play viewer-side (`rbr_render::school_demo`): a timed
  lesson's demonstration draws the player's car at the replay's pose from the chase camera until the replay ends or
  Enter skips it, then the brief; lesson 5's ride plays from the in-car camera and sends the brief's confirm at its
  end, ending the drive on its first frame (`SchoolRun::frame`) with the award's 4, unjudged.
- Speech ([sound.md](sound.md#school-speech)): the briefing, the `Trigger` ids and the end pages on the school
  voice; the results-page `RallySchoolSound` emit left out.
- Judged replay (case 6, ours): watching a replay (`viewer --replay`) of a school lesson races Richard's ghost
  (`Ghosts\rse<lesson>.gho` in `Root/ghosts.rbz`) as the ghost car, on the HUD race clock (verified on a lesson 2
  replay: the ghost loads with 218 samples and drives in step with the race clock; lessons 4, 10, 14 and 16 have
  no `.gho` and race without one). A live lesson drive stays vanilla. Inferred: the ghost's race-clock timing
  (the original's case 6 is not traced); unverified against the original's judged replay.
- Unverified: whether physics steps during the brief, the untimed lessons' start, `EndRace`'s order against the
  HUD tick and its `dt`, NGP leaving the bank's repair handler alone.

## Race options

Mechanics in `race_options.rs` (menu words, damage level, aids, penalties, gate). Checks: level 1 with default aids
(tanner-corolla 13000/13000), level 3 (tanner-damage 12120/12120), championship Rookie and Champion on
tanner-corolla.

- The aids go into the control unit's setup through `InitializeCarSetting` (0x547530).

- Not checked: levels 0 and 2, non-default aids, auto gears.

## Weather

Mechanics in `weather.rs` (daemon, season-table draw, grip path); ours in `rbr-standalone/src/args.rs` (`--weather`,
`--season-row`, `--wear`, `--wetness`) and the launcher. Sky, fog, precipitation, windscreen and wipers are
presentation: [renderer.md](renderer.md), [sound.md](sound.md).

- Any season-table row starts like Random with that row (`--weather 1 --season-row K`); good rows come from `misc/nice_w.ini`, bad rows from `bad_w.ini`.
- Map 68 has season-table row 267 (dry crisp clear) but is in neither list: the original's good and bad weather stop at the int3 there; ours (`Season::unlisted`) takes the map's first table row, so 68 starts on all three weathers. Maps with listed rows are unchanged; a map with no table row still stops.
- Ours: the launcher's rally page has a Weather choice per rally (Season's weather by default: the drawn row, as the game; Good or Bad: the stage's own, via `Catalog::stage_weather`), applied to each championship stage but not the trials. The season's wear still applies; results and records save as for any stage.
- Wet surfaces are separate physics records with lower friction (wet/dry static 0.71-0.88, not in code).

## Championship and service

Mechanics in the `rbr-season` crate: `rally` (the `--result-out` file `StageReport`, the tyre choices), `ai` (the
1.02 AI engine), `service/` (the park), `event` (our championship); the setup in `rbr_gameplay::setup`. The classic
1.02 season (`rbr-championship`: season, events queue, byte-exact save) is deleted, in git history. The `rand()` order is the asm's (`ai`); the driver classes' draws are ours (not in code).
- The launcher owns the built events' rally turns (`championship_page`); each stage is a `rbr-standalone --championship` run
  whose stage rules are flow mode 3's tests in `rbr-gameplay` (`Mode::CHAMPIONSHIP`), so it needs no
  `rbr_gameplay::mode::ModeRules` hook.
- `rbr_gameplay::car_damage::Rules` (1.02 or NGP) picks the damage objects for the HUD and the service park.

## Damage

Mechanics in `car_damage.rs` (carried damage, part severity, repair, service refresh). Physics internals:
[physics.md](physics.md).

## Saves

Mechanics in `record.rs` (submit, personal best, `times.ini`) and the launcher's saves; layouts in [formats.md](formats.md).
The viewer compares against `misc/times.ini` without `--saves` (`rbr_render::hud::splits`).

- The submit rule is not checked against the original.
