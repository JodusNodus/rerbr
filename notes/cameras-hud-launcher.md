# Cameras, HUD and launcher

Presentation around the race: none of it writes physics. The game's behaviour is from the 1.02 exe; the look is ours
unless stated. Gameplay timing: [gameplay.md](gameplay.md). Detail dropped here: rbr-recomp `notes/cameras.md`.

## Cameras

Chase builder, settings and start orbit in `rbr_render::game_camera`; car views in `rbr_render::views` (modes 1, 3,
4, 5, 8, 10, 13, CH_CAM, seat shake); helicopter and flip cameras in `rbr_render::characters::scripted`; the go in
[gameplay.md](gameplay.md). Mode numbers are the game's.

- One camera host (`E15CamHost`, app +0x70): mode +0x00, matrix +0xcc, FOV in degrees +0x2c0, near +0x290, far +0x294 (300.0).
- `CCamera::Tic` runs the mode's builder, then `M7320` (0x487320) emits `CSigCamera` (matrix, velocity, FOV in radians, near, far, two flags); a mode change emits `CSigCameraModeChange`.
- Unported builders: mode 0 (0x4849c0), mode 4 (`M5120` 0x485120), modes 6 and 9 (0x486290, 0x485090).
- `CH_CAM` (action 9, `M487840` 0x487840) cycles 1, 3, 4, 5 in a race, 7 too in playback; blocked while the helicopter flies and in mode 13.

- Track cameras (mode 7, `.dls` table 3, builder 0x489f70; formats parsing in `rbr_formats::markers`): look-at,
  FOV, flag, mode word, znear; look-at -15/-16 give mode 4, -14 mode 1 (not in render code).
- Mode 11 (`M8690` 0x488690) is the fly-in; no code sets it, so 1.02 has no fly-in. Modes 2, 6, 9 are replay payload
  only. Mode 0 covers states 1 (with `m_unk10`) and 7. The mode-13 flip camera uses FOV 75 (not in render code).
- The car-flip camera port is not checked against a capture.

### Ours: the viewer's views

Ours views, nudges and FOV rules in `rbr_render::views`; the chase's field of view is 65 degrees unless set
(`viewer --chase-fov`, launcher Camera page, 30 to 120 like `--fov`; `Playback::chase_fov`); the pace stack shows
`PN_STACK` notes ahead (the original's loop stops one short, so its 4 showed 3), up to 8 in the viewer and launcher
(the game's slider stops at 4); the launcher's co-driver "Call ahead" goes to 15 s (the page stopped at 8); choices in `rbr_render::camera_prefs` (`[camera]`,
`viewer --cameras`).

- Whether RSF itself applies `<car>_personal.ini` `[Cam_*]` overrides is not traced (the loader is
  `rbr_render::car::camera_sections`). One looking more than 30 degrees to the side is not taken (the Porsche 911 GT3
  RS (2010) RGT pack ships a `Cam_internal` 60 degrees left; the 43 installed others are within 5, 2026-11-01).
- The X-key adjust mode is gone; an old `camera adjust` line is ignored (not in code).
- In-car head (ours, `rbr_render::head`): over the game's seat shake, the view keeps the horizon steadier, on the
  physics clock (same at any frame rate, in replays and videos); a look into corners and slides was dropped (user,
  2026-11-01: motion sickness). On by default;
  `[camera] head = off`, from the pause menu's Camera page (cockpit) or the launcher's Graphics page, gives the
  car-fixed view. Tuning constants are game feel, not measured.

## HUD

Model and rules in `rbr_render::hud::model`, sprites in `hud::pace`, layout in `hud::stock`, parts in `hud::mod`
(`viewer --hud-hide`); split references in `hud::splits`. A recorded drive shows only speed, gear, rpm and controls.

- The game's 3D notes (`AID_MARKER`) are not drawn (dropped 2026-10-27).
- The minidash and digidash art is not drawn (not in code).

### RBRHUD gauges

Data in `rbr_formats::rbrhud`, drawing in `rbr_render::dials` (toggle: `dials`); check
`rbr-formats/tests/rsf_data.rs` (every sprite and font the configs name is in its atlas).

- A car copied into ReRBR's own folder has no `Plugins/RBRHUD` beside it: its gauges come from the game install's,
  read in place (`load_dials` in the viewer's `scene.rs`; the M3 E36 GrpA drew none before, 2026-11-01).

## Launcher

Pages and policy in `crates/rbr-launcher/src/` (`main.rs` doc: command line, weather rules, volumes, co-driver
sources). Start runs the viewer with `rbr-standalone --live --boot` as child processes (`command.rs`).

- Settings page, Physics (after the co-driver): NGP 7 (original) or ReRBR experimental (built on NGP 7), with a "What
  is this?" link to `docs/experimental-physics.md` on GitHub (`physics::PhysicsProfile`, `[launcher] physics = ngp |
  experimental`), passed after `--live` as `--physics experimental` to quick races, championship stages and lessons,
  with the car's `--car-group` and `--car-year` where the catalog has them (an RSF car both, an added
  car its group); NGP's adds nothing. A replay's header keeps it (the stepping command). `rerbr --physics NAME`
  (`scripts/launcher.sh` passes it) drives `ngp`, `experimental` or `experimental-brush` (the brush tyre, not offered
  on the page) for the session over the saved choice, never saved; the Settings page says so under the choice.
  ReRBR experimental physics keeps no records (user decision): `rbr-standalone` makes `--saves`
  dry (stage record, personal best, lesson verdict; `Args::keeps_records`), the quick race races the best ghost as
  `--ghost` and never `--best-ghost`, and championship stages refuse to start (trials still run); `.rrp` replays work.

- The game's quality settings and music are not offered (user decisions).
- One game folder (user decision 2026-11-04): everything is read from the game folder in use (`Paths::game_root`:
  `RBR_RSF` when set, else the game data folder), its plugins and community content with it. A saved `rsf_root`
  is ignored and dropped on save.
- The co-driver is set up on the Settings page, not per drive (user decision 2026-10-29).
- NGP: only 7.4.778 is read (`notes/ngp.md`). Settings' "Download NGP 7.4.778…" keeps its `NGP.dll` and
  `physics.rbz` in `<files>/NGP 7.4.778/` and writes no game folder, RSF's 7.5.779 or a plain 1.02 alike; the
  status line offers the download from every page, and with both NGP paths unset a kept copy is used
  (`ngp_install::KEPT`; user decision 2026-11-03); `--download-ngp` too. No player path writes an existing game or
  RSF folder (audit 2026-11-03; the disc setup makes a new one), and the Game folder row says so; with no `HOME`/
  `APPDATA` the player's folder falls back to the launcher's own folder (`home::dir`).
- Keyboard (2026-11-03): Ctrl+1..8 (Cmd on macOS) open the tabs, Ctrl+Tab / Ctrl+Page Down the next, with Shift
  or Ctrl+Page Up the one before (`Page::shortcut`); Tab, Shift+Tab and the arrows move egui's focus, Enter or Space
  presses. The browser lists and the Replays list are one keyboard list each (rows are not Tab stops): the arrows,
  Page keys and Enter work while nothing else or the search box is focused, Esc on another control only leaves it
  (`browser::Keys::take`). A dropdown closes on a keyboard pick and gives the focus back (`Form::ui`; egui closes
  them only on a click). Enter starts a drive only with nothing focused, else it presses the focused control.
- Errors and crashes (`crash.rs`, user decision 2026-11-03: every error, not only crashes): a drive's viewer and
  `rbr-standalone` write `<files>/logs/drive-<ns>.log` (last 10 kept), copied to the launcher's stderr; the viewer
  gets `RUST_BACKTRACE=1`. A viewer exit other than success (not after Stop), drive or replay, opens a panel with the
  log's end, "Report on GitHub…" (the bug form filled in through its link: `title`, `what`, `version`; nothing sent),
  Copy report and the log folder. A launcher panic is written to `logs/launcher-crash.log` and shown at the next
  start. The system clipboard is egui-winit's `clipboard` feature (smithay-clipboard on Wayland, arboard elsewhere;
  arboard brings the `image` crate, egui-winit asks for its image support); before 2026-11-03 paste from other
  programs did not work.
- Idle and while the viewer runs (2026-11-05): the joysticks are looked for every 2 s in one gilrs context
  (`Joysticks::rescan`); a new context each time leaked a `gilrs` thread and its HID manager on macOS (gilrs-core
  0.6.8 never stops it; 27 to 79 threads in 4 min). While the viewer runs the launcher draws only at 4 Hz (`POLL`:
  its exit, the gamepad events queued meanwhile), with no rescan and the car turntable still; otherwise the Drive
  page's turntable redraws at 10 Hz (one turn in 6 s; 20 Hz cost about 6% of a core).
- Joysticks on macOS (2026-11-05, from gilrs-core 0.6.8's code, not tried with a wheel): a new context lists no
  pad until its connection events are read, so `imp::Context::new` reads them until 100 ms pass without one (500 ms
  at most); before, the viewer opened no bound device there. The viewer looks for a bound joystick missing at the
  open every second and opens everything again in the same context when it shows up (`Input::find_missing`).
- Update check (`update.rs`, 2026-11-05): one request at the window's start to the GitHub releases API (published
  releases only; `release.yml` makes drafts), the highest `v*` tag above `CARGO_PKG_VERSION` (semver order) shown as
  a link to its page at the status bar's right. No download, no setting; a failed check shows nothing.
- Stage pictures (Drive page, viewer loading screen; `rbr_render::stage_picture`, 2026-11-03): retail stages show
  the menu bank's (all 384 x 184); community stages show `rsfdata/images/stage_images/<id>.png` (what the RallySimFans menus show,
  1024 x 768), else their `<id>-*.dds` loading poster, the middle of either cropped to 384:184 so both kinds look
  alike. Three of RSF's 658 (588, 629, 630) are JPEGs named `.png` and show the poster (no JPEG decoder).
- BTB stages (`btb.rs`, `notes/btb.md` "Playing"): the RSF install's `RX_CONTENT/Tracks` folders are listed with its stages (number = RSF id, else a synthetic one), Start builds the stage's files into `<files>/btb/<folder>/`; its weathers are map 41's rows under its number (`Season::of_btb`), its records and ghosts are kept under that number.
- Stage browser: “Newest” sorts by map number, highest first (RSF numbers maps as it adds them); no hand-kept
  release dates (user decision 2026-11-03).
- Added cars (gvrc.de packs in the player's `cars/`, listed in `cars/cars.ini`) keep their group and drive
  train from the list's `cat`/`trans` (`RallyN` as `Rally N`, bare `A5`..`A8` as `Group A5`..;
  `car_packs::backfill`), so the browser classes and drive-filters them like the other pack cars.
- gvrc.de's list (6.175) gives 102 NGP cars, all 22 classes, ~1.5 GiB; all 102 add with their own model and drive
  headless (20000 steps on track-10_O) from retail data and gvrc's NGP alone, no RSF (2026-11-07). A physics zip's
  pack is the folder holding `common.lsp`; folders beside it are dropped (the Fiesta Rally2 and i20 N Rally2 zips
  carry a stray `<pack> - rev1/`). The original eight (`cat=Original`) are the 2004 retail physics, not NGP's.
- Stage and car pickers retain collection, search, filters, sort, highlight and scroll on reopening and restart
  (`[stage_browser]` / `[car_browser]` in the player's settings, `browser_state`); highlights use map numbers
  and car names across rescans; Favourites shortcuts still open that collection explicitly (2026-11-01).
- Setups (`setup_page.rs`, 2026-11-04): the Drive page's Setup field picks the default or any saved setup of the car,
  the stage surface's first, then the other surfaces' (labelled), then those of no known surface (user decision
  2026-11-04); the pick is kept per car and surface (`[setup]`, `<surface> <car> = <name>`, `<other surface>/<name>`
  for another surface's; `/` is not allowed in a name). Values load over the stage surface's defaults. The Car setup
  tab is the manager (open, edit, save, delete; saving keeps it for the stage's surface and drives with it). Ours, every car: `<files>/setups/<car>/<surface>_<name>.lsp`
  (`<car>`: the pack's folder name, a retail car's physics folder such as `c_xsara`; user decision 2026-11-04), RSF's
  naming, so a file copies into the install's `SavedGames/<pack>/` as it is. Read only: a pack car's install `SavedGames/<pack>/`
  and `rsfdata/setups/<pack>/` (RSF's `<id>_d_<surface>.lsp` copies of the defaults skipped; RSF install 2026-11-04:
  85 shipped, 214 saved files, none named otherwise). The 1.02 install's `Physics/<car>/Setups/` are not read (1.02's
  car ranges, not NGP's; user decision 2026-11-04).
  The older `[launcher] setup` (one name for every car) is no longer read. No save slots (user decision
  2026-11-04). Earlier versions' files are listed read only: a pack car's `<files>/SavedGames/<pack>/`, the
  `Physics/<key>/Setups/` of `Car::setup_keys` (its key, an RSF car's `rsf-<id>`, then 5; a retail car its number's), and their slot files
  `SavedGames/<key>slot<n>_*.lsp`. The championship's setup row before a stage is default, current or a saved setup (`SeasonSetup::Saved`).
- Car keys (2026-11-07): a pack car's key (camera nudges, `[ffb_cars]`, `--car-key`, championship entries) is
  `catalog::pack_key`, `pack-` and its pack folder's letters and digits without RSF's `_ngp<N>`, so RSF's and the
  added copy of a car are one key (all 102 of gvrc.de's list match RSF's packs this way). Before, it was
  `rsf-<number>`: RSF's car number, or the added cars' own count, which collided with RSF's (13 cars here).
  `Catalog::renamed` maps the old keys (RSF numbers first, an added count only where RSF has no such number); the
  settings' sections move at load and at each save, a continued championship's car keys at its load. A car both
  sources have stays listed twice (user decision 2026-11-07), one model of a championship field
  (`class_cars`); the detail panel's Source row says which copy it is.
- Force feedback (2026-11-08): the Controls page says what Strength does ("how heavy the wheel feels. All cars."),
  the global row reads "Sensitivity for cars without their own", and each listed car shows "maxed out: <surface>
  N %" at its own sensitivity for each surface with 20 s of tracked load (`[ffb_load]`, `ngp.md` "Force feedback";
  hover: adjust in the pause menu while driving). The strength is written to the file on change; the save carries
  `[ffb] strength`, `[ffb_cars]` and `[ffb_load]` over from the file, and the form reloads them when the viewer exits.
  `--ffb-sensitivity` is unchanged.
- The pause menu's Force feedback page (`hud::ffb_page`, with a wheel, once FFB1 says the surface): Sensitivity (the
  car's percentage), Strength (every car, 0-100 % by 5, live through `Wheel::set_strength`, `[ffb] strength`), "Use
  suggested (X %)" (sets Sensitivity on the player's select; shown with 20 s of load and a suggestion other than the
  sensitivity), Reset (every car's percentage) and Done, with a line saying what the selected row does. Under them a
  380×110 graph: NGP's curve at the percentage shown (wheel force over steering load), the car's stored load on the
  surface plus this drive's as bars, the maxed-out region (force > 95 %) shaded, and a legend. Then one advice line
  from one criterion, the sensitivity against the suggestion (`load::advice`: under 20 s "drive a bit more"; below
  0.85× raise, worded "maxes out often" only from 5 % maxed out; above 1.15× lower or raise Strength; else good),
  each with the maxed-out share, and a guide: set Strength once, then Sensitivity per car until the wheel rarely maxes
  out. Nothing changes without the player. `viewer --screenshot --pause-menu dde` draws it (no wheel needed).
- Install findings are dated data, not code: RSF install (2026-10-29, 525 stage folders) resolves LuppisV3 on 511
  stages (477 as the newest-file rule did, 33 where a newer `_default` won, 15 `_default`-only keeping their own
  notes, one stray file); the note-set rule itself is `codriver_packs::set_files`.
- Replays (ours): every live start records one (`--save-replay <files>/replays/<nanoseconds>-<stage>.rrp`,
  full history kept; `viewer --replay FILE [ARGS...]` watches it again). The file holds the viewer arguments that
  drove and the stepping process's frame log (`rbr_recording::replay`); watching runs the drive's frames again
  from its last start, so a replay needs the same install, setup and profile, else it stops with a digest error.
  While watching there is no device input, pause menu or ghost saving: P or Space pauses, R starts over, the
  camera key cycles the views, V resets the mouse look, Esc closes. A replay screenshot (`--screenshot PNG
  --at N`) draws the Nth answer headless, the way a live screenshot draws the Nth payload: the replay
  runs back to back (`--replay-pace free`), not in lockstep (`--frames -` takes no `--replay` beside it);
  `--at-drive` takes no `--replay`.
- Championship tab (`championship_page/`, on `rbr_season::event::Event`; 2026-11-05): the one mode of the tab; the classic 1.02 season
  page (`rally_page`) is gone from the launcher, and its saves in `SavedGames/pf<profile>.rbr` and the settings'
  `championship_*` keys and `[season]` section are no longer read (the files are left alone; the settings file drops the
  keys at its next save). The tab opens on the preset cards (Quick rally, Rally weekend, Short championship, Season, Random; the builder sets rallies, legs and stages), Continue first when
  `SavedGames/championship-<profile>.txt` exists (carried damage in `.dmg`, the service's late penalty and the tyre in `.ini`:
  `Event` keeps neither). A preset opens the builder (car through the car browser, `Target::Championship`, defaulting to the
  Drive page's; class = the car's group, only that class's cars fill the field, Field cars leaving models out; length, stage pool, surface, time of day,
  weather, damage, skill level, team, difficulty Gentle/Club/National/Pro, adaptive, field, seed) beside a live summary
  and Start, the itinerary below (`plan`): the seed's plan, drawn again as the settings change, editable (change a
  stage through the stage browser, `StageBrowser::target` `Championship`, move, remove, add stages, legs and rallies,
  rename) and then the player's own until Draw again; Start builds it with `Event::build_planned`. The skill level is the game's three (`SSKILL`) under `rbr_season` names: Rookie 10 s, Professional 20 s,
  Champion 35 s and a stuck car is not helped (`--championship rookie|professional|champion`); the help text shows the
  penalty. The Team row (service crew) lists `rbr_season::team::team_offers` over the roster of `ai.ini`/`aidrv.ini`
  (`EventConfig::team`; `Event::crew()` sets the service park's repair times). The AI tables (`ai::load_ai`) are read
  once with the page; when they do not read the builder says so and Start stays off. The pool is every listed,
  startable map (`pool::items`), its length from the driveline (`.trk` kind 0x14, a BTB folder's `driveline.ini`),
  read on a thread and kept in `<files>/cache/drivelines/<map>.line` (path, size, mtime checked); an unreadable
  driveline drops the stage (RSF install 2026-11-05: 687 `.trk`, none encrypted, 209 MB). The event's stage profile
  (`pool::profile`) is `track::profile` over the kept points, `StageProfile::flat` without them. A stage launches like the Drive page's on the map's weather row of the
  itinerary's road (nearest row plus `--wetness` when none matches), `--clock`/`--start-time` from its clock, as a
  championship stage (`--championship` on the event's skill, `--championship-damage` the event's), with `--damage-in/-out`
  and `--result-out` in `<files>/championship/`. Screens: welcome, stage card (tyres and setup after a service or at the
  start, else kept; a Start order line "14th on the road · used road" from `Event::start_order`/`road_wear`: fresh, used or worn; a Rally line, the place and the gaps to the drivers either side), a stage's result (its top five and the
  player's row with gaps, the place on it and the places moved overall, the power stage's bonus; the last stage
  driven, or one picked by its name on the itinerary with "Latest" back), service (`common::park_card`, the itinerary's
  minutes; at the end of a leg after a retirement under Super Rally a restart card instead, no park: the car rebuilt, no `--damage-in`), rally over with points,
  the stage wins, the podium, the season's title fight ("3rd in the season · 12 pts behind ... · 60 pts still to win")
  and, when a rally follows, "Practice first (not recorded)": a yard trial (map 72) and a stage trial (the rally's first
  stage, its surface, road and clock), in the event's car and damage on the road's tyre, with no
  championship flags, no `--saves` and no files; its end changes nothing (`ChampionshipPage::trial`, `practice`), and
  event over (champion, the player's record: rally wins, podiums, stage wins, retirements). The standings show the
  places the last stage moved (⬆/⬇ from NotoEmoji: the default fonts have no ▲), the car on the name's hover, the last
  stage's column until the rally is over and Pts (and PS) after; a season adds per-rally points columns and a calendar
  (surface, length, the player's finish, the winner). `results.rs` reads them (stage times and winners, place changes,
  rivals, finishes, record). The builder's Rules row: Super Rally and Power stage. A bulletins card ("Since your last stage", `Event::bulletins`, the last 8 lines, "Show all" for the rest) sits
  above the standings. The page holds the real `Event` (no seam): `mod.rs` its turns, `words.rs` the settings' words and
  flags, `view.rs` the screen switch, `builder.rs` the entry and builder, `running.rs` the championship's screens,
  `cards.rs` its cards, `results.rs` what they read from the results, and `common.rs` the shared cards, places, times and the service park. `--screenshot` draws them on an event the real `Event::build` makes from `championship_page::fixture`'s stages and AI stand-in
  (`championship*.png`, `championship-super-rally.png` a weekend's restart; `--screenshot-damage FILE` adds `championship-service-damaged.png`). The built-event test (`a_quick_rally_on_the_real_stages_and_ai_plays_through`, `RBR_DATA`) loads the real `ai.ini`,
  builds a Quick rally on the installed stages and plays every stage at a fixed pace, saved and read back before SS4 to end as the run never saved.
- School tab, after Championship (`pages::Page::TABS`, `school_page.rs`), no longer tagged experimental (user decision
  2026-11-07): the 17 rally-school lessons in their Basic/Advanced groups with each group's passed count (all open, user decision), each row its number in
  the group, title and grade pips; the detail its label (`RSM_LESSON_NRB/A`, the table's period dropped), title,
  instruction, cone guide with coloured markers (`brief_keys`), surface (from the lesson's setup file), the
  pass/perfect times from the map's `.dls` `Driveline` stream header (both 0: Untimed), the best result, the Drive
  page's car with a link there, what comes first (`Lesson::demonstrated`, the ride) and Drive lesson (Ride along
  for lesson 6; a double-click on a row starts it too). Results are the profile's school array (`Profile::school`,
  0 not passed, 1..3 Passed n/4, 4 Perfect), re-read on change. Drive
  lesson starts like the Drive page's Start on good weather (`command::start_school`, `rbr-standalone --school N`
  with `--saves`/`--profile`; `--tyre`/`--setup`/`--season-row` left out) in the Drive page's car, retail or RSF
  (user decision 2026-11-04: a car slot names no particular car outside a vanilla install). It is blocked while
  the viewer runs, without readiness, or without a Drive page car. In the viewer a
  lesson opens on its brief panel (the number, title, instruction and cone guide from `bstrings.eng`, the
  pass/perfect times when timed, Enter to start), driving shows a short Too slow/Too fast toast for the bank's
  incident codes (ours: no HUD consumer of them was found in 1.02), and the end its verdict panel (Perfect, Passed
  with the grade as pips, Failed; the time and the tier reached) in place of the stage result, over the results
  menu's Retry lesson and Quit (`hud::school`, from the live SCH1 record: the lesson, its state, the kept verdict,
  the times and the answer's last incident code). No countdown plaque shows in a lesson. The brief's confirm ends
  the start orbit, as the countdown's end does. A timed lesson's brief first plays its demonstration from
  `Ghosts\rse%d.arp` (viewer-only, `rbr_render::school_demo`: the player's car at the replay's pose, wheels at rest,
  from the chase camera, the HUD and the start lamps hidden under a "Demonstration · Enter or A: skip" hint, Enter
  or the pad's A skipping without confirming). Lesson 5's ride plays `rse5.arp` the same way from the in-car camera,
  and its end or skip sends the brief's confirm, ending the lesson with its award and the verdict panel.
- Replays tab, after Car setup (`pages::Page::TABS`, `replay_page.rs`): cached, newest-first headers from
  `<files>/replays`, searched by title or filename or conditions;
  malformed and unfinished files stay visible with their error. Rows show the stage time, how long ago it was recorded and
  saved road/cloud/precipitation/fog overrides; details include the stage start time and included penalties. A live
  window saves its last SPL1 beside the replay as `.rrp.splits`, distinguishing a finish, DNF and an interrupted drive;
  older replays without it show Time unavailable. Watch uses the saved presentation with the selected
  camera; Export offers 720p–2160p, 30/60 fps, an MP4 filename (default `<files>/videos/<title>.mp4`, numbered
  when taken; the replay's file name is on the title's hover), audio and progress from the viewer's log (ffmpeg on
  PATH). Replay exit never advances the championship. Files are kept until removed by the player.

### `launcher-settings.ini`

Form in `rbr-launcher/src/settings.rs`, folder in `rbr-launcher/src/home.rs`.

- Quality keys (`graphics_page::Graphics`): `lighting`, `particles`, `cabin`, `contact`, `grass`, `windscreen`,
  `marks`, `wet_roads`, `detail`, `mirrors`; unset takes the Ultra preset. The preset box is not stored: it shows the preset the
  keys match, else Custom; presets leave `marks` and the render scale alone. A saved `surfaces` (the removed scanned surfaces) is not read.
- `perf_log` defaults off; on, the viewer logs with `viewer --perf-log <files folder>/logs` (`home::LOGS`, shown on the Graphics page).
- `present` (`viewer --present`) defaults to mailbox, vsync on macOS: Metal has vsync or immediate only, so the launcher
  offers those two there (`Present::OFFERED`) and shows a saved mailbox as vsync (2026-11-02). `max_fps` (`viewer
  --max-fps`, one of `FRAME_CAPS`, 0 none) is the Frame cap: uncapped held at the display's rate.
- `hdr` (`viewer --hdr`, Graphics > Display > HDR) defaults off: the highlights past an HDR display's SDR white where the
  surface has extended-range sRGB (macOS EDR), else the viewer draws SDR (2026-11-05; renderer note, `hdr` row). Offered
  and passed on macOS only (`present::HDR_OFFERED`): DX12 has no such space, Windows' Vulkan drivers scRGB and PQ only.
