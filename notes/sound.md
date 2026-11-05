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

## Exhaust and car effects

Mechanics in `exhaust.rs` (pops, bangs, runs, engine fire, anti-lag) and `effects.rs` (ignition, dog rings, cooler,
wipers).

- Only the wipers fire in the drives tried (check coverage, not in code).

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
- The splash is 0 in every drive tried, so no wetness voice is checked. Lancer leaving the road at 69.9 s, in the
  car: -22 to -17 dB over 200 ms now (evidence for the gain law, not in code).

## Suspension and driveshaft

Mechanics in `suspension.rs` (squeaks, knocks, bottoming, damage call 7, frame folding) and `driveshaft.rs` (clunks).

## Impacts

Mechanics in `impact.rs` (payload change rule, ranged set, scream).

- A scrape changes the payload every step: Lancer 75-90 s, 371 impacts to 22 (evidence for the spacing rule, not in
  code).

## Co-driver

Mechanics in `codriver.rs` (phrase, queue, damage calls, countdown). Word ids: `rbr_gameplay::markers`.

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
- Aston's `event:/wind 2` (its only wind) is not played: no daemon slot addresses it (inferred NGP-silent too).
- `suspension_damage` (56 banks) is never set by the daemon; ours stays 0 (undamaged), which holds 306_maxi_B's
  one wheel bus-vol at -80 dB (inferred).

## Open

- Which mixer (DX, Prologic, EAX) runs; which resampler mode 2 is.
- Whether 1.02 on current Windows takes the tyres' software path.
- The sound limit's count: 9 against the live list's 11.
- HUD countdown against the gameplay start; why a `--boot` drive starts below 0.
- What writes the splash (`+0x98`); ignition, dog rings, cooler and `EngineNoise p[5]`/`p[7]` never seen live.
- `E31Speech +0x95` as the idle flag (inferred).
- Wine's constant-power 3D panning against 2D speech.
- FMOD: region end rule, async and loop flags, parameter defaults, 3D attributes, an A/B against NGP.
