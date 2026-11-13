# Status

Every 1.02 gameplay system and where it stands in Rust. G: gameplay-critical (changes times, penalties, results or
the car's motion). States: done, partial, out (the user's decision), user (the user's own work). "Original run": a run
of the original game is still wanted to compare. Checks: [verification.md](verification.md). Detail per row:
rbr-recomp `notes/progress/systems.md`; history up to the split: rbr-recomp `LOG.md`, `notes/archive/`.

## Current state (2026-10-19)

- **Physics:** about 930 ported functions (`crates/rbr-physics/ports.csv`), about 660 bindings
  (`crates/rbr-hybrid/bound.csv`). The pure step runs NGP only (`rbr-standalone --boot` needs `--ngp-physics`; 1.02
  snapshots are refused).
- **Load path:** `rbr-standalone --boot` builds the physics state from the game files, NGP's `physics.rbz` and
  immutable tables read from the player's verified NGP 7.4.778 DLL; no executable code section or patch array is
  mapped.
- **Gameplay loop:** done on the recorded drives (control payload, stage states, timing, time card), plus the
  championship, service park and saves.
- **Championship (ours):** `crates/rbr-season` builds rallies and seasons from presets on any installed stage, with a
  curvature pace model calibrated to the 1.02 AI's winning times, the classes and each field car model to the
  RallySimFans hot laps, a fictional AI field, standings, points and a text save ([season.md](season.md)).
- **Gate matrix:** the NGP gate is drive2 attempt 5 (snow, Ford Focus RS WRC 06, RSF car 55), physics PASS
  84084/84084 (`harness/rsf_compare.py`), and `harness/matrix.py` (NGP playback-made pairs on NGP's four car slots).
  The 1.02 drives (4 hand-recorded, 35 playback-made) feed only the hybrid check and playback
  ([verification.md](verification.md#drives-and-the-gate-matrix)).
- **NGP 7.4.778:** physics, load path, force feedback and the H-shifter in Rust ([ngp.md](ngp.md)).
- **Presentation:** renderer, sound, HUD, cameras and launcher in live play; leftovers in the tables below.

## Driving and stage flow

| System | State | Open items |
| --- | --- | --- |
| G Physics step (all subsystems) | done: `rbr-physics`, `rbr-hybrid` pure step (NGP only) | NGP gate matrix to every car and surface |
| G Surfaces, grip, weather tables, tyres | done | - |
| G Damage model, physical detach | done | - |
| G Damage level option | done | original runs for levels 0 and 2 |
| G Brake walls, stuck detection, recovery | done | - |
| G Outside-assistance penalty | done | 20/35 s not compared with the original |
| G Retire and end pages | done | original run: a championship retire and a wrecked finish, a stop in the finish zone, a full replay buffer (945 s) |
| G Physics clock and frame scheduling | done | - |
| G Stage and car load | done | map 68 (a home base map, not listed by the launcher) has table row 267 but is in neither `nice_w.ini` nor `bad_w.ini`, so the original's good and bad weather stop at the int3; ours gives it row 267 (dry crisp clear, `68M_crisp_clear`) for all three weathers (`Season::unlisted`) |
| G Community stages | partial | textures from other variants; encrypted stages not read; full-install sweep done ([verification.md](verification.md#stage-sweep)), no original run |
| G BTB stages (`RX_CONTENT`, experimental) | partial | bump, specular and multiply-add drawing; own number, records, ghosts and map 41's weather rows (converted stages: damp and wet surfaces, shared col corners); no original run ([btb.md](btb.md)) |
| G Community cars (NGP car packs) | partial | option folders |
| G Load settle, start states, the go | done | original run: the go's frame count, a championship start (confirm, 2 s) |
| G Countdown, race clock, jump start | done | moving start checked only on playback |
| G Time keeper, splits, finish | done | speed start and split 1 have no peek; a hand drive held 20 s past the finish |
| G Marker ticker, pace-note callouts | done | fired counts checked at six snapshot checkpoints only |
| G Stage result, records, personal best | done | original run: a drive that beats a record |
| G Restart | done | the viewer's key for RTS1 and its display after a restart |
| Start and finish set pieces | done | original run: the start marshal |

## Modes, setup, options

| System | State | Open items |
| --- | --- | --- |
| G Quick rally | done | - |
| G Game flow state machine | done | - |
| G Rally, service park, tyre and setup choice | done | original run: time-out message, NGP tyre rows; recce/shakedown |
| G Championship season, AI, forecast, offers, trials | done | original run: a season golden, the original reading our save |
| G Rally school | partial: all 17 lessons, demonstration, ride, speech, failure reasons, replay with Richard's ghost (ours) | an original school drive to check the judging, the ghost's timing and the brief's camera |
| G Unlocks, RB Challenge, hot seat | out | - |
| Profiles and save files | done | which profile options the launcher shows (user) |
| G Default setup, option selection | done | - |
| G Player-edited and saved setups | done | original run: a drive with a changed setup; a golden slot file (the format; the launcher offers named setups, not slots) |
| G Driver aids | done | original run: a drive per changed aid; no auto-gears recording |
| G Season difficulty, weather draw, pace-note settings | done | - |
| Particle and render quality options | out | always highest |

## Presentation

| System | State | Open items |
| --- | --- | --- |
| Co-driver phrases, countdown, damage calls | done | not listened to against the original; option gate inferred |
| Co-driver mods (Pacenote plugin voices, styles, MyPacenotes) | done (ours) | not compared by ear with the plugin; speed not `Dot150` |
| Locally owned co-driver and stage-note packs (`codrivers/`, `notes/`, `--import-codriver`) | done (ours) | discovery lists what a pack ships, not full note coverage |
| Engine, transmission, turbo, pops | done | harness hook for `CEngine +0x101c` |
| Tyres, suspension, impacts | done | run the frame finish read-only |
| Wind, rain, ambient, scripted sounds | done | trace `E25Rain::Render90`; listen live |
| Music | out | - |
| Stage, sky, car rendering | done | - |
| HUD, messages, dash lamps, pace-note symbols, units | done | result page from a real finish |
| RBRHUD working dials | done | oil, tyre health fixed; `Damage` unverified |
| Particles | partial | soft edges; rates against a capture |
| Skid marks | done | - |
| Car lights | partial | recordings without live data infer brake only (the gear and lamp words are heap; a replay's sidecar covers reverse and damage) |
| Deformation, cracks, detached parts, dirt | partial | door-detach crash drive (the renderer hangs/detaches doors by DOR1/DET1; no natural detach found yet) |
| Interactive scenery, fences | done | - |
| Stage characters, in-car crew, car-flip marshals | done | original run: game captures |
| Rescue helicopter, bird strike | done | original run: clip orientation |
| Rain and snow particles | done | - |
| Windscreen, wipers | partial | crack normal map, camera dirt |
| Night mode, render enhancements | done (ours) | - |
| Cockpit mirrors | done (ours) | set for the driver's eye, not as modelled; the Fiesta Rally2: none found; images not anti-aliased |
| In-car and chase cameras | done | mode 3 ground probe; chase capture |
| Start orbit, helicam, seat shake | done | - |
| ReRBR replays (`.rrp`), history and video export | done (ours) | playback re-simulates logged frames; requires the drive's build, install, setup and profile; `viewer --replay FILE --screenshot PNG --at N` draws the Nth answer headless (the replay runs `--replay-pace free`, not lockstep's `--frames -`) |
| Original replays (`.rpl`), replay cameras and filters | out | - |
| Ghost car (1.02; RSF ghosts not supported) | done | best-ghost store; original run: a ghost with a penalty |
| NGP UDP telemetry (`--telemetry-udp`, [ngp.md](ngp.md#udp-telemetry)) | done | NGP's own packets not captured for a field-by-field compare; per frame after the step (NGP: per substep before it); no mph |

## Input

| System | State | Open items |
| --- | --- | --- |
| G Input filter, quantisation | done | - |
| Devices, bindings, pause menu | done | the original's default map is not reproduced |
| Force feedback | user | - |
| G NGP physics | user | - |

## Next

1. Runs of the original: damage levels 0 and 2, each driver aid, a changed setup, the go's timing, a season
   golden, a retire, a record-beating drive.
2. Extend the NGP gate matrix ([verification.md](verification.md#drives-and-the-gate-matrix)) to whole stages for
   every car and surface class, with damage and collisions.
3. Community stages and cars: the FMOD leftovers, encrypted stages.
4. Presentation leftovers: particle edges, door detach, windscreen cracks, game captures to compare.

## Decisions

- 2026-11-07 (user): an opt-in experimental physics, "ReRBR experimental (built on NGP 7)" (ours; the launcher's
  Settings > Physics, `--physics experimental`): NGP 7's physics with some systems replaced by ours, one crate and one
  seam each. The first is the engine model (`rbr-engine`): a mean-value engine from each car's data in place of NGP's
  torque formula, in NGP's engine wrapper only ([engine-model.md](engine-model.md); explained in
  [docs/experimental-physics.md](../docs/experimental-physics.md), which the launcher links). Since 2026-11-11 (user)
  the profile also carries the aero fitted on the 2017-21 World Rally Cars' telemetry (`wings+downforce`: the downforce
  devices' drag, half the files' downforce) and a four-wheel-drive drivetrain loss of 17 % in place of the files' 12
  ([engine-model.md](engine-model.md), "Wheel power"; [tyre-model.md](tyre-model.md), "The experimental profiles'
  aero"). NGP 7 stays the default and bit-exact. While experimental, a run with it saves no records, ghosts or
  championship results; its replays carry the choice. It replaces the input-side throttle model, which is not merged.
  The tyre model (`rbr-tyre`): brush-v1 in place of NGP's model of the contact patch on hard surfaces down to
  standstill, loose-v1 on soil, NGP's grip level (gravel's 0.9 of it along the wheel and 1.10 sideways, falling with
  the loose layer's depth, with NGP's soil force held from 10 m/s of sideways speed, and a wet road's loss with speed
  half the drums': user 2026-11-11, the 1.10 the user's of 2026-11-12, the 10 m/s ours on the user's drives; snow's
  0.745 of it along the wheel and 0.836 sideways with the same hold: user 2026-11-13 (closer to the Arctic's stages, 5 %
  above them at most, then 1.02 of that after driving the snow pad by hand), the values ours on the 2017-21 World Rally
  Cars' snow telemetry; snow's force
  along the wheel in two stages and no berm, ours on VTI's trailer curves of packed snow and the crews' braking by
  slip; driving 0.85 of its braking level, ours on the Arctic's crews' thrust; [tyre-model.md](tyre-model.md)); in
  development, not released
  (user 2026-11-07): only the hidden `--physics experimental-brush` runs it; the launcher's Settings page does not offer
  it, `scripts/launcher.sh --physics experimental-brush` drives it for a session (unsaved).
- 2026-11-03 (user): ReRBR needs no 1.02 patch: the 1.02 exe's data words it reads are committed, generated
  (`rbr_formats::exe_data`, `scripts/gen-exe-data.py`, [physics.md](physics.md#standalone-dispatch)); the patch's
  `audio.ini` amps and its one script change are built in. The disc setup asks for the disc and NGP only.
- 2026-11-01 (user): development runs on one Linux x86_64 PC: no test machine, no ssh; the harness runs the original
  locally under Wine, its folders in `dev.env` ([verification.md](verification.md#dev-setup)).
- 2026-11-01 (user): dawn and golden hour keep part of the low sun's warmth: the eye's white balance adapts less to
  warm light ([renderer.md](renderer.md#time-of-day-and-night)).
- 2026-10-21 (user): ReRBR runs either on an existing RBR/RSF install, read-only, or without RSF (retail, patch,
  NGP and locally installed content). Downloaded packs are copied into ReRBR's user content directories; existing
  RSF content is read in place; player settings, saves and edits always belong to ReRBR.
- 2026-10-21 (user): ReRBR steps NGP physics only; the viewer and launcher run through `rbr-standalone --boot
  --ngp-physics`, the oracle is drive2 attempt 5. The 1.02 branches in `rbr-physics`/`rbr-hybrid` and the `Variant`
  type are gone: the x87 precision is the thread's (`fpu::precision()`), and the `ngp` flags of gameplay, render,
  recording and standalone went with it (the 1.02 quantizer path is gone with `ControlPath`; a replay entry still
  goes through `NotifyFilt`, as NGP calls the original in replay mode).
- 2026-10-20 (user): the porting workbench (the exe's function map, worklist, progress tracking, rename tables) moves
  to rbr-recomp's `rerbr/`. ReRBR keeps the Rust, the layouts and addresses that key the vectors and bindings, and
  the checks.
- 2026-10-19 (user): the repository becomes ReRBR, Rust only; the C++ reconstruction moves to rbr-recomp.
- 2026-10-19: the repository holds no private paths or hosts: the harness reaches the test machine through
  `RBR_REMOTE_*` variables (replaced 2026-11-01 by `dev.env`), and the test data comes from the
  player's disc and patch (`rerbr --extract-test-data`).
- 2026-10-19: the documentation is cut to one note per area; detail as of the split stays in rbr-recomp under the
  same paths.
- 2026-10-17 (user): the goal is RBR with NGP 7 physics and its NGP cars, on the original and the modded stages, all
  in live play ([rust-plan.md](rust-plan.md)). NGP and force feedback are the user's code.
- 2026-10-17 (user): HUD modern and simple with every feature of the original; sound and visuals may be better than
  the original; macOS and Linux first, Windows later; English only.
- 2026-10-16 (user): particles aim to look good, not to match 1.02; the windscreen gets a modern water-on-glass
  system ([renderer.md](renderer.md)).
- 2026-10-16 (user): a new basic launcher starts the Rust game; it is not the 1.02 frontend.
- 2026-10-16: the headline metric is how far `rbr-standalone` runs pure and equal on every drive, and its stop list.
- 2026-10-15: the C++ physics is frozen; Rust is ported a group at a time, bound whole and checked against the
  original directly ([rust-plan.md](rust-plan.md#order)). Rust matches every word the gate compares.
- 2026-10-15: the gameplay loop is written directly in Rust and checked against recordings; original `.rpl` playback
  deferred. ReRBR's own `.rrp` replays are supported.
- 2026-10-13: no matching or coverage work for UI, menus, rendering code, HedgeHog3D or LocoMotion.
- The reference is the official 1.02 binaries, not the RSF build. Never modify a player's RSF install; runs of the
  original use a separate vanilla 1.02 install.
- Existing crates over hand-written parsers.
- Contributors work on a branch or worktree each and merge through `integration/handoff`.
