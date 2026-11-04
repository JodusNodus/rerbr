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
- Keyboard (2026-11-03): Ctrl+1..7 (Cmd on macOS) open the tabs, Ctrl+Tab / Ctrl+Page Down the next, with Shift
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
  `Physics/<key>/Setups/` of `Car::setup_keys` (`rsf-<id>`, then 5; a retail car its number's), and their slot files
  `SavedGames/<key>slot<n>_*.lsp`. The championship's setup row before a stage is default, current or a saved setup (`SeasonSetup::Saved`).
- Install findings are dated data, not code: RSF install (2026-10-29, 525 stage folders) resolves LuppisV3 on 511
  stages (477 as the newest-file rule did, 33 where a newer `_default` won, 15 `_default`-only keeping their own
  notes, one stray file); the note-set rule itself is `codriver_packs::set_files`.
- Replays (ours): every live start records one (`--save-replay <files>/replays/<nanoseconds>-<stage>.rrp`,
  full history kept; `viewer --replay FILE [ARGS...]` watches it again). The file holds the viewer arguments that
  drove and the stepping process's frame log (`rbr_recording::replay`); watching runs the drive's frames again
  from its last start, so a replay needs the same install, setup and profile, else it stops with a digest error.
  While watching there is no device input, pause menu or ghost saving: P or Space pauses, R starts over, the
  camera key cycles the views, V resets the mouse look, Esc closes.
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

Form in `rbr-launcher/src/settings.rs`, folder and once-only migrate in `rbr-launcher/src/home.rs`.

- Quality keys (`graphics_page::Graphics`): `lighting`, `particles`, `cabin`, `contact`, `grass`, `windscreen`,
  `marks`, `wet_roads`, `detail`, `mirrors`; unset takes the Ultra preset. The preset box is not stored: it shows the preset the
  keys match, else Custom; presets leave `marks` and the render scale alone. A saved `surfaces` (the removed scanned surfaces) is not read.
- `perf_log` defaults off; on, the viewer logs with `viewer --perf-log <files folder>/logs` (`home::LOGS`, shown on the Graphics page).
- `present` (`viewer --present`) defaults to mailbox, vsync on macOS: Metal has vsync or immediate only, so the launcher
  offers those two there (`Present::OFFERED`) and shows a saved mailbox as vsync (2026-11-02). `max_fps` (`viewer
  --max-fps`, one of `FRAME_CAPS`, 0 none) is the Frame cap: uncapped held at the display's rate.
