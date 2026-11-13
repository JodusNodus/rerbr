# Gameplay sound

How 1.02 makes its in-race sound and how `crates/rbr-sound` reproduces it. Addresses are `RichardBurnsRally_SSE.exe`
1.02. File layouts (`audio.dat`, audio inis, `.eng`, FMOD banks): [formats.md](formats.md). The code's doc comments
carry formulas and constants. Detail: rbr-recomp `notes/sound.md`, `notes/marks-sound-ghost.md` section 2.

## Architecture

Layers in `lib.rs` (models, mixer, `device`, `Player`), the mixer's rules in `mixer/mod.rs` (queue, limit, master
gain, limiter, ours-divergences), distances in `world.rs`. The model is NGP 7.4.778's (`Inputs::ngp`); FMOD banks are
modelled only for cars that have one (`fmod`). The pipeline never writes physics or gameplay state.

- Sound limit evidence: tanner in the car plays 1431 of 1607 gravel hits at 32 against 604 at 16 (not in code).
- A gain is `SOUNDDEF` volume times amp times a player option; volumes are linear amplitude (`SetVol` 0x4d2740).
- The 30 ms sound thread never runs (not in code); the in-car flag is the viewer's bonnet and cockpit views.

- `rbr-sound --replay FILE.rrp` renders a saved replay's sound offline (its own stepping command, back to back): the
  way to hear a change on a drive the user made.
- Offline replay video uses `Player::offline` at 44100 Hz, sharing the live player's bank, custom co-driver and FMOD
  setup; `Player::render` fills stereo samples without an audio device, clocked by the replay's answered periods.

## Faithful and ours

Every source is faithful except the divergences documented as ours in each module (`mixer/mod.rs` lists the mixer's;
`engine.rs`, `car.rs`, `tyre.rs`, `impact.rs`, `menu.rs` their own). Decisions kept here:

- Frame clock, resampling, stereo balance, fades, pitch/gain variation, cabin low-pass: `mixer`.
- Realism (rally car): hotter in-car engine EQ, dog-box whine with coast whine, anti-lag crackle, sound limit 32.
- Engine grains, whine smoothing, blow-off once per arming, software tyre path, gear-click edge, wipers per blade
  change, impact spacing, hard reflections, late-join countdown skip, retail fallbacks, own bird timer, siren at the
  sphere, H-shifter grind: the source's module.
- No music (user decision).
- Limits of 1.02 lifted (user decision 2026-11-04): the engine follows the rpm to 20 000 (`engine::RPM_CEILING`; 1.02
  clamps at 9000, 0x731cd4), above the top bank only the firing period shortens; the co-driver speech queue is
  unbounded (`SpInsert` 0x4dd4d0 holds 10); Doppler on the car heard from outside, the siren and the helicopter
  (`world::doppler`, 343 m/s, the camera's velocity from its motion, the car's factor smoothed over 50 ms; inside the
  car exactly 1; the helicopter is taken at rest).
- Wind and birds (1.02: a replay's, `Scene.replay`, wind outside the car, birds with the car over 50 m away): a replay
  and the video export set `replay`; while driving they play from every camera outside the car (birds without the
  50 m rule); the birds never from the in-car views.
- In-car wind while driving (`world::cabin_wind`; 1.02's `Vf08` 0x4aaf50 in-car branch, unreached in a race:
  `a = |v| * 0.1`, gain `Wind amp * clamp(a, 0.1, 1)`, pitch `a * 0.25`): 1.02's pitch; the gain ours, 0 to 10 km/h,
  linear to twice the `Wind` amp at 160 km/h (1.02's is full from 36 km/h), through the cabin low-pass; an FMOD bank
  with `event:/wind` mutes it as before. Speed ramp at full throttle (Corolla, scratch render): wind 21 dB under the
  engine at 60 km/h, 14 at 100, 11-13 from 150 (1.02's gain: 16 from 36 km/h; not in code).

## Engine

Mechanics in `engine.rs` (inputs, bank, slew, crossfade, grains, `MixScale`, output filter), reflections in
`reflections.rs`, whine/turbo/blow-off in `car.rs`.

- All 8 cars use `subaru.eng`; four of the other `.eng` files cannot load in 1.02 (not in code).
- Output is `E30Engine::Vf08` (0x4cebc0; in-car filter at 0x4ced95, `mixer::INSIDE_CUTOFF`): in the car the 400 Hz low-pass at gain 2 (`InsideCutFrequency` is not read); outside the dry block plus its low-passed self above 2500 rpm.
- Roadside reflections (ours, realism; tuning is game feel): with a stage loaded the two reflection taps follow the
  roadside instead of 1.02's fixed 1.6/1.4 m (`rbr_render::roadside` probes it after each frame's step, `Scene.roadside`
  carries it, `reflections::side_gain`/`side_cutoff`/`side_delay` map it; `viewer --no-roadside` and a missing probe
  keep 1.02's). Probe: the `.trk` scenery placements as upright cylinders (surface id sets the hardness; bushes,
  `Bendable_Tree`, spectators and cones left out; the index keeps the first two for the verge) summed in a 120° lobe
  each side to 35 m, plus 4 `.col` rays a side (square of the face's cosine, so the road and gentle slopes count for
  nothing) and a ceiling segment. Delay is the round trip, gain `4.5 * size * hardness * min(1, 5 / d)` capped at 2,
  cutoff 1.2-6 kHz by hardness halved by 40 m, under 750 Hz in the car; glides 30 ms (delay, slew 0.16 sample/sample)
  and 25 ms (gain), a jump over 1.5 m of path crossfades in 20 ms. Outside cameras: the same from the car, sides as the
  camera faces.
- Roadside evidence (scratch, not in code): probe 7-11 us mean, 13-23 us p99, under 0.1 ms worst a call over 32 000
  poses (retail, release). Along the retail drivelines at 22 m/s the in-car mean side gain is about 0.9 on forest stages
  (35_N, 22_N, 4x up to 1.2 on Mont Blanc's walls), 0.1 on 66_E (80 % open) and 0.4-0.8 elsewhere, against 1.02's 1;
  every retail stage but map 69 has scenery.

- The engine payload's turbo words (1.02 `Sim7D30` 0x517d30): `p[2]` the shaft speed (rad/s), `p[3]` the charge
  over the air, `p[6]` the blow-off flow `(charge - air) * lift * 0.05` and `p[7]` the blow-off valve's lift (state
  `+0x48`, up to def `+0x68` `TurboBlowOffCoefficient_NGP`, from def `+0x48`/`+0x58` the min/max blow-off pressures;
  NGP keys at `+0x3c`/`+0x4c`/`+0x5c`, values `+0xc` on). Nothing reads `p[7]`; rbr-recomp names it the wastegate,
  but the wastegate is def `+0x78`-`+0x98` and does not reach the payload.
- With an engine model set (ReRBR experimental physics) the live picture writes those words from the model's
  telemetry (`rbr-standalone` `live/picture.rs` `model_turbo`; the 1.02 sim's turbo runs beside the model unaware of
  its boost): `p[2]` its shaft speed times 0.78 (the 1.02 sim's over the model's, 0.79 at the median and 0.78 at the
  99th percentile, Fiesta Rally2 on track-11_O, 90 s of autopilot, both read from the same run), `p[3]` its boost
  over the air (the two already agree: medians 1.54 and 1.62), `p[7]` its valve opening times 1e-4 (the Fiesta
  Rally2's lift, its `p[7]` with the valve open) and `p[6]` the 1.02 formula on those; `p[4]`
  `rbr_recording::sound::MODEL_ANTI_LAG` (2.0) while the model's anti-lag runs, the 1.02 sim's misfire word
  otherwise. The 1.02 sim's misfire level is `min(rate * 50, 1)` (`misfire_roll`), so above 1 is only ever the model;
  the NGP drive below gives at most 0.09. The NGP profile's SND1 is unchanged (tested). Same drive, 90 s: blow-offs 16
  against 7 from the 1.02 sim. The headless trace carries only `p[0]`, so it needs no change.
- Anti-lag under the model (ours, realism, 2026-11-07; `exhaust::AntiLag`, `fmod::FmodCar::model_pop`): one source of
  pops. From the first `MODEL_ANTI_LAG` word on, the crackle plays one burst of 0.3-0.7 s (`MODEL_BURST`) each time
  the model's anti-lag starts, and only then (a lift no longer fires it), 0.1-0.35 s apart (`MODEL_GAP`), at the
  misfire pops' gain (`MODEL_GAIN` 1; user 2026-11-07: the real rate and level, 0.04-0.25 s for as long as anti-lag
  ran at 2.5 times, were too much to listen to); the
  word is no misfire (no 1.02 pop, bang or run from it) and ends a misfire's pop run; an FMOD bank's backfire plays
  each of those pops (`exhaustpop` 0.95 on its step, 0 between). A bank whose backfire event has no instruments
  leaves those pops unmuted on the 1.02 voices (user decision 2026-11-07; NGP's mute stands for everything else),
  unless its off-throttle engine recordings crackle of their own (`fmod::CRACKLE_SPREAD`: 5 ms level at p95 over
  the median above 6 dB): of the 57 RSF banks, 48 have an empty backfire event; Focus Mk II WRC, i20 WRC 2021,
  Impreza WRC S9, Escort Cosworth, 208 Rally4 and BMW M1 crackle (6.2-21 dB, bursts of 4-9 peaks/s 9 dB over the
  recording's median), the Fiesta Rally2's overrun loops are steady (2.8-4.5 dB, 0-1.5 peaks/s), as are the Fiesta
  R5's (the same recordings), Fabia R5's and 208 R2's. Before the first word (the run's first lift) and on cars
  without anti-lag the crackle still fires on lifts (on a bank that mutes it, unheard). NGP's sound is byte-identical (rbr-sound render of an NGP
  drive). Cars without anti-lag under the model: byte-identical (Golf II GTI 16V).
- Real anti-lag (evidence for `MODEL_GAP`, `MODEL_GAIN`; onsets of 2-8 kHz energy in 5 ms frames 9 dB (roadside) or
  5 dB (on board) over the median of the 0.5 s around, grouped into bursts at gaps under 0.4 s; not in code): a VW
  Polo GTI R5 from the roadside (youtube.com/watch?v=Uv0AmwP-gSA, 523 s) gives 36 bursts of 7.3 bangs/s median (5.8
  to 9.6 at p25-p75), 0.48 s long median (1.39 max), the bangs 11 dB over the median (16 at p90); a Hyundai i20
  Rally2 helmet camera on a stage (youtube.com/watch?v=azDl6hZIk3g, 176 s) 30 bursts of 5.8/s (4.5 to 6.9), 0.55 s
  (1.49 max), 5.9 dB over. Gaps within bursts: p10 0.045 s, median 0.095 (roadside) and 0.165 s (on board), p90
  0.31-0.34 s. Published ALS guides (MoTeC, Link, rallycars.com, Wikipedia) give no pop rate or level. Whether the
  rate follows rpm, and how long bangs last on a lift longer than the videos' (about 1.5 s), is not known.
- Anti-lag, Fiesta Rally2 under the model, autopilot on track-11_O (90 s; `rbr-sound --log` counts): with its FMOD
  bank nothing was heard before (its backfire events are empty, so NGP's mute took every 1.02 pop); now its pops in
  bursts of 5.8-7.2/s at 76-88 s, 6.7-8.8 dB over the 2-8 kHz median in the car, 6.7-9.4 dB in the chase view. On
  the 1.02 voices (`--no-fmod`, inside) the lift at 56-66 s went from 7.4 pop steps/s (bursts of 39-66/s, 2.9 bangs/s)
  to 2.3/s, the stage at 76-88 s from 15.5/s (bursts of 14-33/s, 9.8 bangs/s) to 3.2/s (bursts of 5.9-8.0/s, 0.2
  bangs/s); 6.7-8.5 dB over in the car, 9.6-9.8 dB in the chase view (p90 15.7). Focus Mk II RS WRC 2006 (backfire on
  its `_ext` event), chase: its backfires in bursts of 6.8-9.9/s. NGP drive (FMOD and 1.02 voices) and the Golf II GTI
  16V under the model: byte-identical renders. Renders `/tmp/als-sound-renders/` (not kept).
- Open: other cars' blow-off valves take the Fiesta Rally2's lift; whether the crackle should stay on lifts of cars
  without anti-lag under the model; on banks with a backfire event and crackling overrun loops (Focus Mk II WRC
  outside) the model's backfires play beside the loops' own crackle.

## Exhaust and car effects

Mechanics in `exhaust.rs` (pops, bangs, runs, engine fire, anti-lag) and `effects.rs` (ignition, dog rings, cooler,
wipers).

- Only the wipers fire in the drives tried (check coverage, not in code).
- Wipers and splats from the glass (ours, realism, 2026-11-05; `effects::Glass`, `rbr_render::sound::glass`,
  `Player::set_glass`): with the renderer's glass model the wipers play as its blades move, in the car only, instead of
  1.02's 0.4 s timer under precipitation (kept without a glass: `rbr-sound`, a car without a windscreen chart). The
  motor loops while the blades are off the park; each stroke the glass begins (`Glass::stroke_count`) plays one stroke,
  wet by the water under the sweep (the film's mean over the swept cells plus the share the water, snow and trails
  cover; fully wet from 0.08) and dry for the rest, by equal power, so the mud-driven auto wipers on dry glass scrape;
  each puddle splat the glass lands (`Glass::splatted`, its big drop's size) slaps the windscreen once, 0.25 to 0.5,
  through the cabin low-pass, 30 ms apart within a frame (at most 6 waiting). Splats warmed up after a seek are dropped.
  Recordings embedded (`data::GlassBank`, `sounds/CREDITS.md`): in-car wipers (strokes and the motor from between them),
  a dry-glass scrape, a bucket of water hitting a car; the retail `CAR::WIPER` samples stay for 1.02's path. Levels:
  the motor 0.5 of the `WiperEngine` amp, a stroke 1.6 of the `Wiper` amp (the recordings' loudest 50 ms at -15 and
  -18 dBFS). Autopilot on wet 52O, GC8, in the car (`/tmp/wipers-rain/`, not kept): with no rain and the wipers on
  continuous each stroke scrapes about -29 dBFS (20 ms), as loud as the whole mix at idle and 15 dB under it at speed.

## Tyres and surfaces

Mechanics in `tyre.rs` (loops, rumble, gravel hits, rattles, software path, low-pass; the exe's surface table from
`rbr_formats::exe_data`); NGP's road list in `rbr_formats::ngp`.

- In the car the road noise already follows speed and surface: the level's speed term `min(speed * 0.05 + slip *
  0.1, 1)` (full by 72 km/h) on the surface's own loops (gravel, tarmac, snow; table pitch, cabin low-pass), and the
  rumble off tarmac (full by 36 km/h, pitch 0.8-1.2) (`tyre.rs`; checked 2026-11-05).
- Ours (game feel, `tyre::roll_pitch`): in the car the rolling surface loops (not the skid loops) rise in pitch from
  the table's at 72 km/h (wheel speed) linearly to +20 % at 180 km/h, held above; level, rumble, stones and outside
  cameras unchanged. Speed ramp (tyre layer alone, gravel and tarmac): commanded gain identical, output level -0.2 to
  -0.3 dB from 140 km/h (the cabin low-pass), spectral centroid 717 to 823-853 Hz (scratch render, not in code).
- Water (ours, 2026-11-05, `tyre::Water`): 1.02's splash is 0 in every drive tried, so the renderer's water per
  wheel plays instead (`wheel_spray::water`, the spray's own: puddles with `--enhance wet`, the wet tarmac film, the
  water surface; `rbr_render::sound::water`, `Player::set_water`; without it, 1.02's splash alone). The retail data
  has one water sample, `Surfaces/Normal/water.wav` (NormalWater and SkidWater, a steady swish), and no wet surface
  loops, so two CC0 Freesound recordings are embedded (`crates/rbr-sound/sounds/CREDITS.md`): three runs of a car
  through water (`Shot::Splash`) and a wet-road pass's peak as a loop (`WheelLoop::Hiss`). Puddle depth is the spray's
  plume depth (`wheel_spray::puddle`, standing water 0.15 to 0.6; a rut's 0.08 film is 0); wet tarmac's film
  (`Water::film`, standing 0.5 under `--enhance wet` in wet weather) swishes but never splashes (user decision). Running in: a splash once
  the depth stops rising, if it rose by 0.15 or more times the speed term (2 to 15 m/s), picked in turn (no random
  numbers); in it, the swish on `water.wav`; on a wet road the loop by wetness squared over 2 to 25 m/s, 0.6 of the
  level and 0.78 of the pitch off tarmac. The gush (the rise, decaying as 1.02's wetness) overrides the swish and the
  swish the hiss, at most 1 between them. Both loops take the tyres' distance gain (1.02's wetness: flat) and in the
  car the cabin low-pass. The bottoming thud under the splash was dropped: the recordings carry the impact.
  Autopilot drive on wet 54O, Corolla (`/tmp/water-sound/r6/`, not kept): the right-side puddle +2 to +4 dB on the
  chase mix, +1 in the car; the wet gravel hiss 10-13 dB under the tyres; without water, output identical.
- Tarmac skid loop (ours, 2026-11-12, on trial): `TIRE::SURFACES SkidTarmac` is a 0.98 s loop at a constant level
  (50 ms envelope std 0.18 dB, a 1335 Hz tone, -7.9 dBFS RMS); in its place six seconds of a real car's squeal
  (embedded, CC0, `CREDITS.md`: 930-1100 Hz with its octave coming and going, envelope std 1.3 dB, -12 dBFS RMS, so
  4 dB lower). The trigger, level and pitch rules are 1.02's; gravel and snow keep theirs. No open recording of a
  low sideways scrub without a squeal on top was found. Stones tried and dropped the same day (user: keep the
  retail `IMPACT::GRUS`; pebbles on a metal box, branch `sound/stones`). Not judged by ear yet.
- Lancer leaving the road at 69.9 s, in the
  car: -22 to -17 dB over 200 ms now (evidence for the gain law, not in code).
- Verge (ours, 2026-11-05, `verge.rs`): 1.02 has no grass, bush or branch sounds. The renderer's `rbr_render::verge`
  gives each frame each wheel's share on grass (the tyre payload's two surfaces' `Grass_*` blend) and the bushes and
  bendable trees the body is in (`rbr_render::sound::verge`, `Player::set_verge`; without it nothing plays): the
  roadside index keeps their `.trk` placements (`Growth`, left out of the echoes), the body a 4 x 1.8 m box from 0.35 m
  below to 0.9 m above the car's centre, each placement widened by 0.5 m of foliage (the meshes are cores and trunks,
  0.1-0.4 m across). Grass loop `2 * sqrt(share / 4)` over 1.5-20 m/s, brush loop `1.4 * min(sum(density * depth /
  0.5 m), 1)` over 0.5-12 m/s, both pitched 0.8-1.2 and 0.85-1.15 to 30 m/s, falling with 150 ms; a hit on each new
  bendable tree (bush: half times its density) over 2-20 m/s, at most 2 a frame, four samples in turn (no random
  numbers, no mixer variation). Loops through the cabin low-pass, hits through the stones' 2.5 kHz shell filter (the
  cabin's 1 kHz left them 30 dB under the mix), all at the tyres' distance gain. Stones: no CC0/CC-BY recording of
  stones on an underbody or arch was clean enough (the interior and tyre-mic gravel drives carry the road's roar under
  every tick), so the retail `IMPACT::GRUS` stay. Away from the verge the output is byte-identical to main (rbr-sound
  binary and the verge path with an empty verge, 61_M autopilot, chase and cockpit); the hits count under the sound
  limit, so in a crash that saturates it they change which stones play. Probe 0.7-1.2 us mean, 1.3-3.5 us p99 a frame
  (retail, release); the echoes' probe unchanged (15.8 vs 16.0 us on 42_M). Retail roadside vegetation is sparse near
  the drivelines (42_M: 5 placements within 6 m of the autopilot's first 60 s), so most corners cut over grass alone
  (not in code).
- Verge, tall vegetation only (ours, 2026-11-12, on trial; user: no sound for short grass, the patter is better
  than the swish): grass under the wheels plays nothing now (the stages mark grass by firmness, not height). The
  patter loop (`Loop::Grass`: the four stick-on-tall-grass hits laid 45 times a second into 6 s, `CREDITS.md`, in
  place of foley of a person in high grass) plays with the brush on the body's contacts with bushes and bendable
  trees (level 2 by the contacts, 1.5-20 m/s), and a wheel on a bush or bendable-tree surface counts as a contact
  (`Verge::brush`, two wheels full). In the car both loops take the stones' 2.5 kHz shell filter at 2.5 times the
  level (`mixer` `VERGE_GAIN`): through the cabin's 1 kHz filter the grass loop (85 % of its energy above 4 kHz)
  lay 21 to 29 dB under the mix. Sherwood Forest I (RSF 367, Lancer Evo II, the user's drive, in the car): six
  encounters in 10 min, 3.6 s in contact, 9 branch hits; the added sound 6 to 18 dB under the mix (one at walking
  pace silent). `rbr-sound --verge CSV` plays the contacts `rbr-render`'s `verge_probe` example finds along
  `--poses`; a replay whose stepping program is gone steps with `--standalone`. No open recording of a vehicle
  through grass or brush was found. Not judged by ear yet.
- What the stages give the verge (2026-11-12, `rbr-render` example `verge_survey`: the ground's surface every 5 m
  along the driveline at 4, 6 and 9 m to each side, and the `.trk`'s bush and bendable-tree placements within 8 m;
  one variant a map). 479 of the 501 installed classic maps read (19 `.col` encrypted): grass is 20 % or more of
  the 6 m verge on 70 % of them, rough on 10 %, neither reaches 5 % on 21 % (tarmac, snow and gravel verges); no
  ground sample is a bush surface; no vegetation placement within 8 m on 33 % of the retail and 53 % of the modded
  maps, 5 or more a km on 52 % and 37 %. The 97 installed BTB stages: grass 20 % or more on 68 %, and not one
  vegetation placement (their objects carry no bush or bendable-tree surface), so there the brush and the branch
  hits never play.

## Suspension and driveshaft

Mechanics in `suspension.rs` (squeaks, knocks, bottoming, damage call 7, frame folding) and `driveshaft.rs` (clunks).

- Bump-stop knocks (ours, 2026-11-12, user: unrealistic on bumpy tarmac): 1.02 plays every bump word above 0.01 as
  an `IMPACT::RANGED` crash sample at full level, and a strut riding its stop changes the word every step. Now the
  knock is a thud on a car's body (8 embedded CC BY recordings in three strengths, `CREDITS.md`, in place of
  `CAR::SUSPENSION`'s two clipped samples: soft below 0.2 of full, hard from 0.6) at `2 * bump / 0.8` (full from
  1.02's bottoming threshold, where the ranged impact still joins it; the thuds are 6 dB under 1.02's samples),
  one a wheel per 9 frames unless 1.5 times harder; the frame keeps its hardest bump. The
  user's replay (Alpine A110 RGT, Mont Ventoux BTB, 330 s, in the car; `rbr-sound --replay`): 1979 new bump words,
  median 0.037, a tenth above 0.13; commanded ranged impacts 852 to 34, knocks 272 at a median gain of 0.07, 7 at
  full (not in code). The recorded drives never touch the stops (62 drives, 39 min: no bump word changes). A knock
  takes a place at the sound limit as the ranged impacts do (the newest one-shot stops): on gravel the stones held
  the limit and 50 of 139 knocks were refused (the Hassi replay, Lancer Evo II, 305 s; none now, not in code).

## Impacts

Mechanics in `impact.rs` (payload change rule, ranged set, scream).

- A scrape changes the payload every step: Lancer 75-90 s, 371 impacts to 22 (evidence for the spacing rule, not in
  code).
- Level (ours, 2026-11-12): a ranged impact plays at `power / 1000`, full from there (1.02: full from 100); the
  sample follows the power as before. A body scrape holds about 765 (the Ventoux replay, not in code).
- Scrape (ours, 2026-11-12): a contact whose payload keeps changing (held 4 steps past the last change) scrapes
  from its second step: a loop of metal dragged over concrete (embedded, CC BY, `CREDITS.md`; no open recording on
  tarmac or gravel was found) at `2 * power / 1000` over 1-15 m/s, pitch 0.8-1.2, up in 30 ms and down in 150, at
  the impacts' distance gain and in the car through the stones' 2.5 kHz shell filter. During the contact only an
  impact 1.5 times harder than the last plays; the 0.15 s gap applies between contacts. The payload gives the
  contact's point on the body ([0xc..]) but not what it touched, so the road and a bank scrape alike. The Ventoux
  replay's contacts are a low front corner for 50 ms at 30 m/s (the nose on a compression) and two longer ones;
  ranged impacts made 261 to 23 (not in code). Levels are set by measurement, not by ear.
- Body slide (ours, 2026-11-12, on trial; user: a flipped car scraping along is not believable): with no wheel on
  the ground (`Tyres::slide`) the contact holds 30 steps, the scrape is twice as loud at 0.7 of its pitch, and on
  loose ground (the wheels' last surface not tarmac-first) the gravel skid loop plays under it (`Loop::Slide`, level
  2 by the speed, pitch 0.7-1.0; `impact.rs` `SLIDE_*`). Fiat 131 on its roof from 110 to 70 km/h (RSF 1075, in the
  car, the user's replay): before, the mix held its driving level (-12.2 to -14.6 dBFS over 0.28 s windows, the
  engine leading, the scrape dipping when the payload paused); after -10.9 to -11.6 dBFS, centroid 430-730 to
  570-770 Hz. No open recording of a heavy body dragged over a road was found (drum, sheet and tool drags of under
  1 s only). Not judged by ear yet.

## Co-driver

Mechanics in `codriver.rs` (phrase, queue, damage calls, countdown). Word ids: `rbr_gameplay::markers`.

- Damage call 8 (`gearbox_broke`) comes from the step's `CSigCarDamageSpeech` emits (SND1 `speech`, a bit per code,
  counted by the pure watch at 0x4ae0d0), not the handler's speech ring: about 24 damage functors step the ring
  (`E35Ring`, rbr-recomp `RBR_E35_DamageB.cpp`), only `RingStore` (the gear actuator, `+0x3a90` cleared) emits.
  Before 2026-11-13 any first damage said "gearbox broke" while the HUD showed the gearbox fine.

## Custom co-driver

Mechanics in `custom_codriver.rs` (window, spoken call, sticky rules, preview in `preview.rs`, HUD integration);
pack layout in `rbr_formats::pacenote`.

- Test install (2026-10-26): all 17 styles and both range sets load the same packages, so the voice decides the words
  (`Janne Laahanen EnNu A`: Descriptive ids on numbered takes, `six_left` for EASYLEFT); the launcher hides the style
  and range rows when the sets say the same (data observation, not in code).
- Owned copies: Janne V3 with Tanner LuppisV3 loads 1,154 samples without a game install configured; a 40 s constant-speed
  preview gives the same 47 calls and byte-identical WAV as the source pack (local copied-pack review, 2026-10-22).

## Countdown

Mechanics in `codriver.rs` (HUD codes, `CSoundFX`, late-join skip). No finish sound.

## School speech

Mechanics in `speech` (`Speech::School`), `data::SchoolBank`, the mixer's queue and `rbr_render::sound::SchoolSpeech`.

- Handler `E25School::OnSignal` (0x4b8200, `RBR_E25_School.cpp`): `{0,0,0,raw 0}` stops the playing word and flushes the queue (`S10` + `SpClearAll`); a nonzero raw runs `E25School::M80c0x` (0x4b80c0) over the NUL-terminated digit string (4-character slots, each `atoi`'d, `digits[3] == 0` ending it), which flushes the pending queue and queues each id at priority 1; kinds 4/5/6 queue one id at priority 0 from `sub`-indexed tables (4: 499, 242, 236, 500; 5: 468, 235, 495, 496; 6: 317, 467, 497, 498); kind 1 queues `M8160[bank]` at priority 0; kinds 0/2/3 without raw are silent.
- Senders: every school-flow step emits the stop; `CReplayManager::Trigger` (0x4992a0, `RBR_E19_School.cpp`) emits `{2, i, 0, pair.code}` per tripped pair, the code the digit string's offset in the lesson `.dls`'s table-7 string block (`ReadStream` 0x498840, `E12_AnimSet::BlockAddB` 0x475c90); the end pages emit kind 4 on failure (`CFailedSchoolExplanationUI::Vf04` 0x542aa0) or by grade, 0 to 4, 1..3 to 5, 4 to 6 (`CRSEndTrialUI::Vf04` 0x5a2510, `CSchoolAwardUI::Vf04` 0x5a26a0), with `sub = rand & 3`.
- Rust (`rbr_school::speech`): `speech_ids` follows `M80c0x` exactly (C `atoi` per slot), the `.dls` string block rides with the run and the pair code is its offset (block base 0); SCH1 carries the frame's stop and `Trigger`'s ids; `Speech::School` plays `Audio\RS\ENG\<id>.ogg` (English only), priorities 0/1 through `SpInsert`'s `speech_slot`, `FlushSpeech` for the pending-only clear; the co-driver switch does not mute it and the co-driver volume applies.
- Inferred: kind 1 plays once per brief (`BRIEFING[lesson]`, no sender reconstructed); each SCH1 state change stands in for the flow steps' stops; the results page's `RallySchoolSound` `{2,1,0,lesson}` emit is left out (its payload contradicts the handler).

## World and ambient

Mechanics in `world.rs` (wind, birds, siren) and `mixer/rain.rs`. The siren plays only in `rbr-sound --camera
trackside` (the binary has no replay cameras); the wind in every camera, the birds outside the car.

- Rain in the car (ours, realism, 2026-11-05; `world::cabin_rain`, `Command::CabinRain`): two recordings from inside
  parked cars (light rain from the driver's seat, heavy rain on a roof; `sounds/CREDITS.md`) replace 1.02's
  synthesised stream there; outside the car the synthesised one stays (a 300 Hz rumble, but no outside recording
  was clearly better, and outside the car the wind and tyres cover it). The drops hitting the car go as the glass
  model's, `amount / 100 * (1 + speed / 28)` (light standing 0.5, heavy 1); heavy takes over from light over 0.5 to 1
  by equal power, the level is its square root (at most 3) times 1.6 and the `Rain` amp, the pitch +12 % by 30 m/s.
  No cabin low-pass (recorded inside). Snow plays none, as before. Same drive, heavy rain: at idle the mix +1.5 dB
  (+5 dB at 200-600 Hz) over 1.02's stream, at speed +0.2 to 0.5 dB (the engine and tyres cover it); 1.02's light and
  heavy streams measure the same.

## Scripted and service park

Mechanics in `scripted.rs` (helicopter, bird hit, incident) and `menu.rs` (service park, music).

## FMOD cars (ours)

Mechanics in `fmod.rs` (block, parameters, pauses, bank levels, `LOUDNESS`), `mixer/fmod_mix.rs`, `mixer/dsp.rs`; bank
layout in `rbr_formats::fmod`. Cars without a bank use slot 5's retail sound (Impreza 2003); the rest of the sounds
come from the retail `audio.dat` (`audio.dat.disabled` in RSF installs).

- NGP's FMOD output sits 6.8 dB (car) and 8.8 dB (outside) above ours (not in code).
- Checked: gvrc's C2 S1600 bank loads (not in code).
- Checked: all 63 banks on this machine (57 RSF, 6 gvrc) carry `event:/wheel` with 2-4 layers over `brake`
  and `wheel_speed` (2026-11-04); its four instances play with per-wheel speeds and brake from FMB1, the car
  speed when the block is missing. Renders of pack cars with banks (Focus Mk II RS WRC 2006 on drive2/5,
  Fiesta rally3 evo and DS3 R1 on the tanner drive, Evo IX on rsf-test1/1; `/tmp/car-pack-sound/`, not kept)
  command the wheel loops every driving step (`--log`'s `fmod_wheel`) and the solo-`fmod` mix matches the full
  mix, engine-dominated as Sound-C measured the original's.
- Checked: `skid_int`, `skid_ext` and `gear_grind` are empty in all 63 banks (their events reference no
  sheets; the daemon loads neither), so there is nothing to play (2026-11-04, scan, not in code).
- Checked: the `gear`/`state` parameters (2026-11-04). Every bank tiles `gear` [-1, 8] per gear with `state`
  [0.5, 1] for upshifts and [~0.085, ~0.49] for downshifts, which is what the shift logic sends (0.6-1.0 up,
  0.1-0.5 down, else 0); a down `state` of 0.487-0.497 lands in neither, as the daemon's does (inferred from
  the bank ranges and the event structure). A tanner-drive render fires a launch shift's gear shot end to end.
- Loop count and trigger chance (2026-11-09; `INST` +24, +33, `rbr_formats::fmod`): -1 on every long loop of the
  57 RSF banks, 0 on every gear, ignition and blow-off instrument; 71 instruments of the engine, turbo and
  transmission events have 0 (bangs, blow-offs, shift fluctuations), the i20 WRC 2021's `pop B` 2, and only the
  engine events have chances under 100 % (22.5-93; the GC8 555's on-load bang 39.5). Those instruments play once as
  their regions are entered (`Slot::shot`), each draw on the chance; a looping multi instrument plays its playlist,
  another entry at random at each end (ours: modes 1 and 2 not told apart, weights not read). Before, every
  loop-event instrument looped its first recording: the GC8 555 (the bank with the most, 7 one-shots and 6
  playlists) looped 0.3-0.7 s bangs and blow-offs and repeated one 0.55 s pop off the throttle. GC8 on
  `drive-v6-tarmac-impreza00-45` 55-85 s (`/tmp/gc8-sound/`, not kept), lifts over 2650 rpm: in the car the 2-8 kHz
  envelope's correlation at the pop's 0.55 s period 0.53 before, -0.02 now, its level -0.5 dB, the mix -0.6 dB.
  Byte-identical: retail sound and the 14 banks without such instruments (DS3 R1 rendered). Byte 28 (1 on the loops
  and most engine one-shots, 0 on gear, ignition and some backfires; inferred FMOD's sync flag) is not read: a
  one-shot plays to its end.
- Aston's `event:/wind 2` (its only wind) is not played: no daemon slot addresses it (inferred NGP-silent too).
- `suspension_damage` (56 banks) is never set by the daemon; ours stays 0 (undamaged), which holds 306_maxi_B's
  one wheel bus-vol at -80 dB (inferred).

## Open

- Which mixer (DX, Prologic, EAX) runs; which resampler mode 2 is.
- Whether 1.02 on current Windows takes the tyres' software path.
- The sound limit's count: 9 against the live list's 11.
- HUD countdown against the gameplay start; why a `--boot` drive starts below 0.
- What writes the splash (`+0x98`); water against the floor through deep fords (the swish at full depth stands in); ignition, dog rings, cooler and `EngineNoise p[5]`/`p[7]` never seen live.
- `E31Speech +0x95` as the idle flag (inferred).
- Wine's constant-power 3D panning against 2D speech.
- FMOD: region end rule, the async flag (INST byte 28), the playlist modes, parameter defaults, 3D attributes, an A/B against NGP.
