# Steering input (ours): sticks and keys

How a gamepad stick and the keys become the steering the car gets, so that both feel good on every surface without
changing the physics. Issue #6 (gamepads: triggers, a steering filter, vibration). Code: `rbr_gameplay::steering`
(the shaping and the cells), `rbr_gameplay::filter` (the hook), `rbr_render::devices` (the stick's gate and dead
zone), `rbr-standalone` `--stick-steering`/`--key-steering`, the launcher's Controls page. The 1.02 filter and poll are
in [gameplay.md](gameplay.md) "Input filter" and "Device poll".

## The 1.02 path as it is

- Every live control runs through the 1.02 filter grid (`filter.rs`): per channel a cell chosen by the poll, with a
  cubic curve, a rise and fall rate (per second, against the output, scaled by the fade), and a speed fade
  (`MaxSpeedFade` at `MaxSpeedFadeSpeed` m/s). Cells: 0 `STEER::AXIS`, 1 `STEER::STICK`, 3 `STEER::BUTTON_DIGITAL`.
- The poll selects only cell 0 (any axis) or cell 3 (keys, buttons) (`poll/mod.rs` `tic`): a stick is a wheel to it,
  and cell 1 is never selected.
- Live play takes cell 0 without its fade (`Grid::without_wheel_fade`) and no rates: a stick goes lock to lock in one
  step. That is #6's complaint.
- The retail `PC_InputFilter.ini`: keys rise 3.0/s, fall 5.0/s, fade 0.5 at 27 m/s; stick parabolic (the identity
  curve: `lin` 1, no terms), fade 0.5 at 27 m/s, no rates; axis inactive, fade 0.3 at 35 m/s. RSF renames the file, and
  the built-in grid's keys run at a tenth of those rates (0.3/s, 0.5/s): about 3 s to full lock.
- The fade's speed is `notify_a`'s: forward speed times `1 - min(|sideways| / sin 60, 1)` of the velocity's
  direction, so a slide counts as slower (0 at 60 degrees of slip). That is the original's only grip-like input.
- The recorded drives store the payload after the filter (FRM1), and `--boot` never runs it: nothing here moves the
  gate. A replay (`.rrp`) stores the viewer's payloads before the filter, and its header the viewer's arguments with the
  stepping command (`viewer/replay.rs`), so the filter's settings must be stepping arguments to replay exactly.

## What others do (researched 2026-11-22)

| Source | Mechanism |
| --- | --- |
| OpenCMR2 (GPL-3.0, `game/StageObjects.cpp` `StageObject_ShapeThumbstickSteering`, `src/input/input_sdl.cpp`) | Stick eased at the car's steering speed, as keys; response `x + (x^2 - x) k`, k 0 at low speed to 1 (a square) at top speed, fitted to the lock key drivers used per speed; full lock kept at the end; round gate read as square (`x * \|(x, y)\| / max(\|x\|, \|y\|)`); dead zone 2 %, saturation 95 % for pads |
| gt5-recomp | GT5's own steering; one small round dead zone for every stick; triggers scaled to the game's full range; fake DualSense reports through the real parser (`padsim.py`) |
| AC Advanced Gamepad Assist (adam10603, MIT) | Stick is a share of the front tyres' peak slip angle (0.95), minus the front axle's travel angle; countersteer from the rear's slip; self-steer and yaw damping; peak slip learned online; rate scaled by speed |
| CSP Gamepad Force | Stick is a force on a simulated rack against the game's aligning torque |
| BeamNG | Understeer limit from a grip-limited radius, off on gravel ("loose surfaces benefit from understeer"); oversteer help; speed-scaled rate and limit |
| ACC, AMS2, DR2, EA WRC | Dead zone, saturation, linearity, speed sensitivity (a cut of the most lock), filter or rate |

Ranked for low grip: slip-angle targeting > countersteer and self-steer > a rack against the aligning torque >
grip-radius limit > speed fade and rate > curve, dead zone, saturation. The first four read the car's state after a
step; the last two need only the input.

## Design (this batch)

Two layers, both ours, both on the input side:

1. The device (viewer, `rbr_render::devices`): a stick is an axis of a device SDL or evdev calls a gamepad (evdev:
   `BTN_SOUTH`; SDL: `SDL_IsGamepad` and its mapping's `leftx`/`lefty`, `rightx`/`righty`) with a partner axis. Its
   value is read as a square gate with the partner, then the dead zone and saturation (`[steering]`
   `stick_dead_zone`, `stick_saturation`, percent); the poll's cell becomes 1 (`CELL_STICK`). The payload carries the
   shaped value, so replays keep it.
2. The filter (stepping process): for cell 1, ours replaces the grid's: the speed curve `x + (x^2 - x) k(v)` on the
   target with `v` the fade's speed (a slide gets the linear end: quick countersteer), then the 1.02 `step` with our
   rise and fall and no fade. For cell 3, our rise and fall replace the grid's, the grid's fade kept. Given by
   `--stick-steering RISE,FALL,CURVE` and `--key-steering RISE,FALL` (the launcher's `[steering]`), so replays carry
   them; without the flags the grid is the 1.02's, so older replays run as they did.

- Full lock stays at the end of the stick's travel at every speed (unlike RBR's stick fade, which cuts it to half by
  27 m/s): hairpins and big slides at speed need it.
- The grid itself (a profile's, the INI's) is not changed: ours sits beside it in the filter, so the profile's words
  and `Grid::from_profile` stay the 1.02's.
- Defaults (ours, to tune by driving): stick dead zone 5 %, saturation 95 %, rise 3.0/s, fall 5.0/s (0.33 s to full
  lock, 0.2 s back), curve 100 % of the fitted `k(v)`; keys rise 2.5/s, fall 5.0/s. The launcher shows the rates as
  seconds across the lock (Controls, "Gamepad stick and keys"); `[steering]` in the settings file.
- Without the launcher's flags (a bare `rbr-standalone --live`), a stick still selects cell 1 and gets the grid's:
  RBR's own stick cell (its fade, no rates).

## The curve's fit

`crates/rbr-recording/examples/steer_speed.rs` (FRM1 to `step,steering,speed,faded_speed`) and `harness/steer_fit.py`
(per 2 m/s bin, the quantiles of |steering| fitted with `p + (p^2 - p) k`, normalised to full lock). Data: the wheel
drives (G923, one driver): drive1/1-2 tarmac 174k steps, drive2/1-6 snow 371k, test3/1-2 gravel 13k (setup file only,
too small and unlike the others: left out).

- k is 1 (a square) from 2-4 m/s on every surface and both speeds; with the fade's speed it leaves 0 at about 1.5
  m/s: `k = clamp((v - 1.5) * 0.5, 0, 1)` in code (`CURVE_FROM`, `CURVE_SLOPE`). Unlike OpenCMR2's key drivers, whose k
  ramps to 1 only at top speed: a wheel driver keeps most of the time near the centre.
- Above 24 m/s the used lock is steeper than a square (`p^a`, a 2.4-3.1); below 20 m/s a 1.7-2.2. Tarmac and snow
  agree at speed; snow uses more lock below 16 m/s.
- Median |steering| 0.12-0.29 at 0-16 m/s, 0.07-0.13 at 20-28, 0.03-0.06 above 30; p90 0.9 to 10 m/s, 0.4 at 20, 0.2 at
  30, 0.1 at 44.

## Verification

- Unit tests: the square gate, dead zone and saturation; the curve's ends (0 and full lock at every speed) and its k;
  time to full lock and back at the rates; cell 1 and 3 overrides, and the grid's path without the flags.
- `scripts/rust-check.sh`; the gate once (nothing in `--boot` changes).
- Not verified here: a real pad on any platform (no pad on this machine). evdev's gamepad mark and axis pairs follow
  the kernel's gamepad convention; SDL's come from its mapping (the SDL path compiles on Linux for its test only, and
  now starts SDL's gamepad subsystem with the joysticks').

## Open

- Tune on a pad: the rates, the dead zone, and whether the curve should go past a square at speed (the fit's power
  2.4-3.1 above 24 m/s; `shaped` stops at `x^2`).
- A gravel hand drive on a wheel for the fit.
- Later, each its own batch: triggers captured as axes (#6 item 1, unverified on alpha 5), rumble from the surface
  (OpenCMR2's model), DualSense trigger effects, and a slip-angle assist or a rack against NGP's torque.
