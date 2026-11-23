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

- Look back (2026-11-16, issue #7): the key's swing (`Follow::look_back`, 0 forward to pi behind) also turns the cockpit,
  bonnet and bumper views about their eye (`Playback::lens`); until then it turned the chase only, so in the cockpit
  the key did nothing. The bonnet and bumper eyes also move to the model's back (`Mounts::rear`, `BUMPER_AHEAD` past
  it; the move follows the swing, half way at a quarter turn) so they look down the road from the rear instead of
  at the car's own body (the bumper's was its number plate).
- Whether RSF itself applies `<car>_personal.ini` `[Cam_*]` overrides is not traced (the loader is
  `rbr_render::car::camera_sections`). One looking more than 30 degrees to the side is not taken (the Porsche 911 GT3
  RS (2010) RGT pack ships a `Cam_internal` 60 degrees left; the 43 installed others are within 5, 2026-11-01).
- The X-key adjust mode is gone; an old `camera adjust` line is ignored (not in code).
- Field of view on the pause menu's Camera page (ours, 2026-11-08): the shown view's, in 1-degree steps over 30 to 120;
  the cockpit, bonnet and bumper share one (`--fov`), the chase has its own (`--chase-fov`). Written at once to the
  launcher's `[launcher] fov` / `chase_fov` (one setting with the Graphics page's sliders; the launcher rereads it
  when the viewer exits). Reset leaves it.
- Triple screens (ours, 2026-11-08, [renderer.md](renderer.md#triple-screens-ours-2026-11-08)): `--fov` and
  `--chase-fov` set the centre screen's field of view; every camera, the start orbit and the helicam included, draws
  the three; the HUD and the menus go on the centre.
- In-car head (ours, `rbr_render::head`): over the game's seat shake, the view keeps the horizon steadier, on the
  physics clock (same at any frame rate, in replays and videos); a look into corners and slides was dropped (user,
  2026-11-01: motion sickness). On by default;
  `[camera] head = off`, from the pause menu's Camera page (cockpit) or the launcher's Graphics page, gives the
  car-fixed view. Tuning constants are game feel, not measured.
- Cockpit choices (ours, `camera_prefs::Cockpit`): besides the head, `[camera] driver = on` draws the driver's arms in
  the cockpit view (off by default, user's call 2026-11-10), `codriver = off` and `wheel = off` leave the co-driver and
  the car's steering wheel out of it (each on by default), from the same two pages. The launcher's `hide_wheel` of before is read as `wheel = off` and no longer
  written; `viewer --hide-wheel` still hides the wheel whatever the file says.

## HUD

Model and rules in `rbr_render::hud::model`, sprites in `hud::pace`, layout in `hud::stock`, parts in `hud::mod`
(`viewer --hud-hide`, `--hud-show`); split references in `hud::splits`. A recorded drive shows only speed, gear, rpm and controls.

- The game's 3D notes (`AID_MARKER`) are not drawn (dropped 2026-10-27).
- The minidash and digidash art is not drawn (not in code).
- Telemetry parts (ours, 2026-11-12; `hud::telemetry`, drawn by `hud::stock::telemetry`), off unless shown: `viewer
  --hud-show tyres,trace`, the launcher's HUD parts, kept as `[launcher] hud_show` beside `hud_hide`.
  - `tyres` (live drives): each tyre's 0.6 surface + 0.4 tread temperature, the tyre step's weights, green at full
    grip, towards blue below and red above the tyre's best `RubMyFct` temperature as its temperature grip falls to
    0.85, with the tread worn under it (the mean of its eight sectors' wear, percent). From TYR1
    (`rbr_recording::tyre`), which `rbr-standalone` reads from the tyre frames after the step. Corolla, gravel,
    track-54_E: 56 °C at the start line, 0.3 % worn by the launch and still 24 s in. Not tried under
    `experimental-brush`.
  - `trace`: the pedals and the steering over the last 10 s, at the bottom left (the car's status moves over it), from
    the drive's frames, so a recorded drive and a replay have it too.
  - A g-force part was dropped the day it was added (user decision 2026-11-12: not useful); `g` in a kept `hud_show`
    or `hud_layout` names nothing (`Part::RETIRED`).
- Layout and size (ours, 2026-11-12; `hud::layout`): each movable part (its own `Part::carrier`: clock with the passed
  split and the ghost gap, progress, pace note, dial with its pedals, car state, messages, tyres, trace) has an
  offset in points of the 720-point screen from where the look puts it, and the HUD a scale of 50 to 200 %; a part
  scales about the corner or edge it stands at (`Part::pivot`) and is kept on the screen. `viewer --hud-layout
  PART:X,Y;...`, `--hud-scale PERCENT`; `[launcher] hud_layout`, `hud_scale`. `Stock` draws a part, then moves and
  scales its shapes (`placed`); with no layout the frame is unchanged (RMSE 0 against the capture before). The race's
  calls and pages, the pace stack and the frame rate neither move nor scale.
- The clock is its name over 28-point digits with one 22-point row under it for each penalty, the ghost gap and a
  passed split (number, time, difference): 96 points high with both rows, 196 before (user request 2026-11-12).
- Ghost gap (ours, 2026-11-12; `ghost::passed`, `Session::gap`): with a ghost, a row under the clock has the drive's
  race time less the time the ghost was where the car is, on the nearest segment of the ghost's samples (looked for
  about the last one found, then along the whole path). Shown with `splits`, from the go to the finish, within 30 m of
  the ghost's path. Autopilot against `drive-tanner-corolla.bin`'s ghost on track-54_E: +1.87 s at 12.6 s, the ghost
  about 28 m up the road at 53 km/h.
- The dial's rim flashes amber at 6 Hz and the gear is lit from the upshift point (SHF1's for the gear, else 5500
  rpm), in place of the lamp dot over the dial (ours, 2026-11-12). The frame rate (`--fps`) is a pill at the top
  right like the distances'. The camera's name is not shown (`Part::View` dropped 2026-11-12, retired like `g`).
- The pause menu's HUD page (`hud::hud_page`, after Camera; user decision 2026-11-12: set while driving, not in the
  launcher): Size, Part (all ten), Shown, Reset (places and size; what shows stays) and Done. The pointer picks the
  part under it and drags it to its place, kept on the screen (user decision 2026-11-12, in place of Across and Down
  rows; the split times move with the clock, the pedals with the dial; not tried with a mouse in a window, only
  `hud_page`'s tests). While it is open the HUD shows a sample with something in every part and the picked part
  outlined. Each change, and a drag when the button is let go, is written at once to `[launcher] hud_hide`,
  `hud_show`, `hud_scale` and `hud_layout`; the launcher rereads them when the viewer exits and passes them on, and
  has no control of its own for the layout or the size. `viewer --screenshot --pause-menu dde` draws it.

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
  ReRBR experimental physics keeps no stage records, personal bests or championship results (user decision):
  `rbr-standalone` makes `--saves` dry (`Args::keeps_records`) and championship stages refuse to start (trials still
  run); `.rrp` replays work. Its times and best ghost are kept apart under its version: the quick race's Best ghost is
  one file per physics id, car class and road, `<player folder>/ghosts/<physics id>/<class>/<road>/Best_<map>.ghc`
  (`ghosts::best_ghost`), passed as `--best-ghost` for every physics. The old `GhostCars/Best_<map>.ghc` is no longer
  written; it lists as "Best (before classes)".
- The launcher offers no tuning of the brush tyre (user decision 2026-11-13; its Experimental tuning section, the
  `[launcher] tyre_tuning` key and the Drive page's note are gone; an older file's key is not read). `rbr-standalone
  --tyre-tuning` stays for the tyre research ([tyre-model.md](tyre-model.md#experimental-tuning)).

- The game's quality settings and music are not offered (user decisions).
- One game folder (user decision 2026-11-04): everything is read from the game folder in use (`Paths::game_root`:
  `RBR_RSF` when set, else the game data folder), its plugins and community content with it. A saved `rsf_root`
  is ignored and dropped on save.
- The co-driver is set up on the Co-drivers page, not per drive (user decision 2026-10-29); the drive pages don't show
  or link it (user, 2026-11-18).
- The player picks the co-driver as they think of one: Co-driver (off, the original, or a language), Calls (how the
  corners are said), then Voice (who says them); Said as (`"6 Left"` or `"Left 6"`) and Grades (every grade or
  Simple) only when the calls come both ways (user decision 2026-11-22, over picking the person first;
  `codriver_packs::{choices, catalog}`). Each call style is a V3 install of its own (a pack, or RSF's plugin): its
  texts name the preview's 8 corners tightest to fastest (`choices::Ladder`, `HP` read as Hairpin), grouped as
  Numbers (one word at most among the inner six, as Square or 90), Numbers and words, or Words; numbers in words
  count (French `un, deux`). `catalog` (Luppis's V3 list, 18 packs) names a folder's person and language, and lists
  each pack's calls as read from its downloads (13 packs; Czech, Paolo Lombardi, Hrenovalke and SpaCry unread): what
  no source has is listed in each dropdown as "Not added yet", opening Get more with its packs first. A folder is
  checked for the corners in its install's style: missing some lists as incomplete, none is left out (RSF's
  `english`, `german`, `JanneLaahanen`). A pick keeps as much of the rest as the new choice has (`choices::closest`).
  The choices are scanned with the packs (load, import, remove: 0.47 s with Janne's 42 English installs, release
  build); switching re-reads only the chosen pack (`reselect_codriver`, 30 ms).
- The stage notes are automatic (user decision 2026-11-16, `codriver_packs::pick_notes`): with a custom voice, over
  every owned pack and the RSF MyPacenotes, the set covering the most stages (ties: owned pack, then name), in it the
  highest `_<n>` version,
  then owned over RSF, then the newest file; none: the stage's own notes. The range set is the plugin's `Rbr.ini`
  (else its first). `notes_pack`, `notes_set`, `my_pacenotes` and `codriver_ranges` are no longer read or saved.
  The summary ("On Tanner: <voice> calls LuppisV3 (182 notes)") shows under the voice.
- Dropping files works on X11, Windows and macOS only: winit 0.30 has no drag and drop on native Wayland, so there Import
  says "Add files or folders" and does not offer it (`content::drop::no_drop`; user decision 2026-11-17, over forcing XWayland).
- One Add for downloads (user decision 2026-11-16; `content/`): files dropped on the window, or "Choose files…"/"Choose
  folder…" on Import, are opened as a file tree (`content/source/`: folder, zip, 7z, split 7z, NSIS
  installer, plain file, tried in `SOURCES`) and each kind (`content/kind/`, one module each, one line in
  `content::KINDS`) says whether it is in it; added in one job with its result on Import (✔ where to find it, ⚠ why
  not; a toast on other pages). BTB stages (`track.ini`, `objectlist.ini`, `Objects.zip`, `Textures.zip`, several per archive) and classic
  stages (below) both go to `<files>/tracks/` through a hidden staging folder, never over an existing stage;
  co-drivers and pace notes through `codriver_packs::import` (a source holding several plugin installs, as Janne
  Laahanen's 42 call styles, imports each as its own pack `<source> - <install folder>`: 42 packs, 633 MB, 9.8 s
  from the folder; an archive is read once, 12.7 s from a solid 7z of it (one read per install took 8 minutes);
  more than 4 give one summary line, a failing install is a `Failed co-driver …` line and the rest still import).
  A solid 7z's entries are slices of one stream: `archive::extract_seven` reads through the ones it leaves out, or
  every later one fails its CRC (sevenz-rust2 0.24 does not skip them itself);
  NGP car pieces (physics, model and banks dropped together) as one `car_packs::Download`; NGP's package to
  `NGP 7.4.778/`; a disc opens the disc setup filled in; a car skin to its car (`content/kind/skin.rs`, below).
  Refused with the reason: `.rar`, non-NSIS installers, encrypted stages, a lone model or banks, anything else. The
  Downloads folder is not scanned (user, 2026-11-18): only what is dropped or chosen is added. Links: RSF's stage download, BTBfin, RBR Czech's
  stage list (its installers are read, never run).
- Classic stages (`content/kind/classic/`): RSF-shaped folders (`Tracks<id>.ini`) or loose `track-<n>_<L>.*` files
  with their `[Map<n>]` and weathers from the tree's inis or an installer's INI writes (empty-key writes, the
  installers' section deletes, ignored), several per source. Each is written once into
  `<files>/tracks/<number>-<name>/`, an RSF stage folder (`rsf::scan_maps`, listed by `Catalog::add_classic_stages`
  beside the BTB stages): renamed files, `Tracks<n>.ini` rebuilt (`Particles`, `SplashScreen` as
  `tracks\<folder>\…`, resolved from the player folder as RSF's are from its root), `TrackSettings<n>.ini` (own
  weathers, else a retail stage's of the same surface), the remap's `track-<n>_` and the `.rbz` entry names (raw
  copy), licence/readme and splash (Open licence in the notice). A variant missing a required file is left out and
  said. Own number kept when free, else the first free from 10 000 (RSF ids reach 1301, BTB's start at 1 000 000).
  Refused: RSF `01 a0` headers, a `Key` in `[Map<n>]`, a `.col` not `OC7R`, no complete variant, the same name and
  copied sizes again. Evidence: 120 RSF stage folders added with forced numbers list with their variants, weathers,
  surface, author and length, and load and drive 20 000 steps under `stage-sweep.sh` (24 match the originals' rows);
  RBR Czech installers: Peklo, Pribram (O variant left out) and Sturec added, Manukau refused as encrypted.
- NGP: only 7.4.778 is read (`notes/ngp.md`). Settings' "Download NGP 7.4.778…" keeps its `NGP.dll` and
  `physics.rbz` in `<files>/NGP 7.4.778/` and writes no game folder, RSF's 7.5.779 or a plain 1.02 alike; the
  sidebar's status area offers the download from every page, and with both NGP paths unset a kept copy is used
  (`ngp_install::KEPT`; user decision 2026-11-03); `--download-ngp` too. No player path writes an existing game or
  RSF folder (audit 2026-11-03; the disc setup makes a new one), and the Game folder row says so; with no `HOME`/
  `APPDATA` the player's folder falls back to the launcher's own folder (`home::dir`).
- Shell (user decision 2026-11-18, the redesign; `pages.rs`, `sidebar.rs`, `theme.rs`, `kit.rs`): a 220 px sidebar
  in place of the tab rows and the status bar. Drive (Rally, Circuit, Championship, School), Library
  (Cars and Stages with their counts, Co-drivers, Replays (user, 2026-11-18)), a status area (game folder found, Experimental physics, the
  readiness issue with its fix button, the status text, the update link), then Import and Settings. Settings' General, Controls and Graphics & sound are a segmented
  control under the page title; the sidebar item opens the last of them (`Form::last_pages`, indexed by
  `pages::Section`). Car setup is in no group: `Form::open_setup` records the opener (`setup_from`) for its back
  link; the Cars inspector's Setups › Manage… opens it for that car (`Form::open_car_setup`, `setup_car`) without
  changing the Rally car (user, 2026-11-18). "Get more" is a modal (`Form::get_more`, `GetMore`, `get_page.rs`) over
  Cars (Get more cars… and Get more skins…: RBR Skins Center, user 2026-11-18), Stages or Co-drivers: gvrc.de's
  list (search, class, tick table of the cars not added: added ones are always hidden, no Select all shown,
  user 2026-11-18; `gvrc.rs`) or the sites' rows,
  each pointing to Import. A car download is polled in `Form::content_notice` (drawn every frame), not by the modal:
  closing the modal leaves a progress line above the page bottom and a toast with the result. Import (`import_page`:
  drop zone with Choose files… / Choose folder…, Last import with an Open link per outcome, Clear)
  takes every drop; on another page the result is a toast (`Add::toast`, `kit::toast`) linking to Import. Open in
  Stages shows the stage in Library › Stages (`StagesPage::show_stage`); a disc opens Settings › General's disc setup.
  Geist and Geist Mono are bundled (`assets/fonts`, OFL), egui's fonts stay as fallbacks for
  ★ ⚠ ▼ ↵. Default window 1280×760.
- Rally, Circuit, School (redesign, `drive_page.rs`, `circuit_page.rs`, `school_page.rs`): page header, a pinned footer
  (`Form::launch_footer`: summary, blocker with its fix, experimental note with Change to Settings, Stop while driving,
  Start with the Enter hint), and columns that scroll on their own (`split_columns`, whole points wide, `column`).
  The car and stage cards' pictures share the car's 16:9 shape (`preview::picture_height`; a stage's picture is
  cropped at its sides), so the cards line up (user, 2026-11-18). The
  group labels carry no links: a card opens its browser, the Weather segment goes back to the stage's own (user, 2026-11-18). RSF is named only where it tells something (the
  inspectors' Source, encrypted stages, RSF's settings and setups), not on cards, labels or the search hint (user,
  2026-11-18). Rally: Car (card, Gearbox,
  Setup with Edit…), Stage (card, Ghost, Car damage; no favourites' chips under the cards, user 2026-11-18), Conditions (Weather Stage's own / Custom, Sky look with Adjust…, Time of day with the sun arc, then Road and
  tyres). Circuit: Session (Length, Pace notes, Damage) and Conditions with Road and tyres folded into one row; the best lap
  panel keeps sectors and car. School: both courses in one list (grades as pips, double-click drives), the lesson's brief,
  cone guide, times and Car on the right. The browsers still draw in the old padding (`Form::browser_frame`).
- Settings and Co-drivers pages (2026-11-18, the redesign; `settings_page.rs`, `controls_page.rs`, `graphics_page.rs`,
  `codriver_page.rs`): grouped row panels (`kit::panel`, `Rows::row` / `switch_row`), two columns under the Settings
  tabs, each page scrolling (`kit::body`). General: status rows (✔, or ⚠ in the warning colour with its fix button) for
  the folder, NGP, cars and wheel (named from `ControlsPage::pad_names`); the Experimental group is main's three switches
  (force feedback from the tyres, engine, brush tyre), each with its help and "What is this?" link; Advanced
  collapses; the files folder and its Open link are the pinned footer. Controls: every action a row ("Not set" + Bind,
  or the input as a button that rebinds, a live meter, Clear), gears each their own row, aids/head gasket/force
  feedback as switch rows. Graphics & sound: Display, Camera and HUD on the left, Quality and Sound on the right.
  Co-drivers: voice (grouped by source) with Preview, Call ahead with −/+ (0.25 s) beside the 0.05–15 s drag value,
  the notes summary, installed packs with Remove, and the Get more modal link.
- Keyboard (2026-11-03, groups 2026-11-18): Ctrl+1..3 (Cmd on macOS) open the Drive, Library and Settings groups at
  the page each was left on, Ctrl+Tab / Ctrl+Page Down the group's next page, with Shift or Ctrl+Page Up the one
  before (`Page::shortcut`; nothing from Import or Car setup); Tab, Shift+Tab and the arrows move egui's focus, Enter or Space
  presses. The browser lists and the Replays list are one keyboard list each (rows are not Tab stops): the arrows,
  Page keys and Enter work while nothing else or the search box is focused, Esc on another control only leaves it
  (`browser::Keys::take`); under a modal the list has no keys, so Esc closes the modal (`browser::list_has_keys`). A dropdown closes on a keyboard pick and gives the focus back (`Form::ui`; egui closes
  them only on a click). Enter starts a drive only with nothing focused, else it presses the focused control.
- Errors and crashes (`crash.rs`, user decision 2026-11-03: every error, not only crashes): a drive's viewer and
  `rbr-standalone` write `<files>/logs/drive-<ns>.log` (last 10 kept), copied to the launcher's stderr; the viewer
  gets `RUST_BACKTRACE=1`. A viewer exit other than success (not after Stop), drive or replay, opens a panel with the
  log's end, "Report on GitHub…" (the bug form filled in through its link: `title`, `what`, `version`; nothing sent),
  Copy report and the log folder. A launcher panic is written to `logs/launcher-crash.log` and shown at the next
  start. The system clipboard is egui-winit's `clipboard` feature (smithay-clipboard on Wayland, arboard elsewhere;
  arboard brings the `image` crate, egui-winit asks for its image support); before 2026-11-03 paste from other
  programs did not work.
- Idle and while the viewer runs (2026-11-05): the joysticks are looked for every 2 s in one context
  (`Joysticks::rescan`: their ids, and they are opened again only when those changed). From the viewer's start to
  its exit the launcher's joysticks are closed (2026-11-16, issue #7: Windows' DirectInput gives a force-feedback wheel
  to one exclusive open, `ngp.md` "Windows"), and opened again by the next rescan. While the viewer runs the
  launcher draws only at 4 Hz (`POLL`: its exit), with no rescan and the car turntable still; otherwise the Drive
  page's turntable redraws at 10 Hz (one turn in 6 s; 20 Hz cost about 6% of a core).
- Joysticks off Linux (2026-11-12, issue #6; not run on macOS or Windows, the library type-checks for Windows): SDL3's
  joysticks (`devices::sdl`, `rbr_render::sdl`) in place of gilrs-core, whose macOS layer found no DualSense on macOS
  27, wired or over Bluetooth (cause not found; it drops a device without a word when a property read fails). The
  axes and buttons are SDL's, in its order, the hats left out as on Linux; polled, SDL's events off. Bindings saved
  on macOS or Windows before name gilrs's axes and buttons and are set again. On Linux the same code reads what
  evdev reads: the devices' ids, the axis and button counts, the buttons and every moved axis's value (the ignored
  test `sdl_reads_what_evdev_reads`: a G923, a T500 RS shifter, a uinput Xbox 360 pad; an axis not moved since the
  open reads SDL's 0 there, and SDL names the pads it knows itself). macOS: SDL's IOKit driver hears of devices
  plugged in on the thread that first started its joysticks, so the viewer opens the menus' gamepads on the main
  thread before the wheel and the input thread. The viewer looks for a bound joystick missing at the open every
  second and opens everything again in the same context when it shows up (`Input::find_missing`).
- The menus' gamepad (`rbr_render::gamepads`, 2026-11-12): SDL3's gamepads on every platform in place of gilrs: the
  d-pad, South and East of each, polled once a frame (18 µs on Linux), a held d-pad repeating after 400 ms every
  80 ms (a uinput pad: Down at the press and again from 400 ms, Select and Back once a press). A wheel SDL has no
  gamepad mapping for moves nothing.
- Update check (`update.rs`, 2026-11-05): one request at the window's start to the GitHub releases API (published
  releases only; `release.yml` makes drafts), the highest `v*` tag above `CARGO_PKG_VERSION` (semver order) shown as
  a link to its page in the sidebar's status area. No download, no setting; a failed check shows nothing.
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
- Library layout (redesign, 2026-11-18; `browser.rs`): Cars, Stages and Replays are one pattern: `kit::page_header` (count subtitle;
  Random, Get more…), one filter bar (search, the scope segments All / Favourites / Recent / Circuits /
  Cannot start (no "Added by you": user, 2026-11-18; an older save's `added` opens on All), shown only when they have rows, then class, drive, sort for cars and surface, country, length, sort for
  stages; they wrap to a second row under ~1060 px), the list (header row, 33 px rows, a hint bar of the keys that work)
  and a 380 px inspector (`browser::inspector`: scrolling facts, pinned Favourite and primary button). A BTB stage is
  marked in the inspector's Source only, not in the list, and not called experimental there (user, 2026-11-18). The class and
  country are filters beside the scope (`Filter::class`, `Filter::country`, saved as `class` in `browser_state`); an older
  save's `class:`/`country:` collection restores as the filter. The pickers (Drive, Circuit, Championship) are the same
  browsers with Back, "Choose car/stage" and no Remove. Replays: rows of stage, time, car, conditions, age; the inspector
  has Stage time, Conditions, Recorded (with the stage start), Penalties, Camera (cockpit, bonnet, bumper, chase), Watch, Delete (asks),
  and the export with its MP4 filename.
- Content > Stages (`stages_page.rs`): the stage browser as a page (`StageBrowser::start_page`: no Back or Random, Esc
  does nothing) over every map, circuits too, with the collection "Circuits"; the subtitle counts what's added (folders in
  `<files>/tracks/`); the detail has kind, folder (Open folder), weathers and a circuit's best lap (rally records are in the
  game's saves, not shown). "Drive this stage" sets the Rally (or Circuit) stage and opens that page. Remove… deletes
  only a folder directly in `tracks/` (`remove_added`), then drops the map and relists; records (`lap-records.ini`,
  `GhostCars/*_<map>.ghc`, saves) stay. With the textures upscaled, a BTB stage over the upscaler's budget says
  "Textures: kept as they are: too large to upscale (N GB)" (`stages_page::Sizes`, renderer note).
- Car skins (`car_skins.rs`, 2026-11-17): the Skin box and Import skin… sit under the turntable on Content > Cars and
  in the car picker's detail panel. A skin is a folder
  of files over the model folder's by path (`textures/*.dds`, shader settings: `Data::with_skin(model, dir)`, retail cars
  too). Listed: the player's `skins/<car key>/<skin>/` (imported) and a community car's own `Cars/<model>/skins/*`
  (RSF's, read only); none for a stand-in model. The pick is ours, per car key (`car_skins = key:skin|...`, `none` =
  the model's own textures); unset leaves RSF's `.rsfconfig` selection (never written) and retail's own textures. It goes
  to the viewer as `--car-skin DIR|none` (`car_pack::Flags::skin`, wins over `.rsfconfig`; not to `rbr-standalone`) and to
  the turntable preview. Import takes a `.zip`, `.7z` or folder (RBR Skins Center's, RSF's exports; the Add's sources):
  each shallowest folder holding `textures/` becomes a skin, named for the archive (several in one archive: for their
  folders); same name replaces. The Add (2026-11-18, user decision) takes a skin too, when no other kind does (a model
  archive and stages hold textures): its car is the one whose model folder its path names (`Cars/<model>/`), else the
  one whose model `Textures` share the most `.dds` names with it (at least half; names are model-prefixed: Fiesta
  Rally3's `Zasada_Rally3` goes to the Rally3, not the Rally3 Evo), into every car of that model; several models or
  none: the notice's Open Cars, for the car's own Import skin…. The skin added is chosen, and Open shows its car.
  Checked with a channel-swapped retail ZR skin (picker, preview, `viewer --screenshot`) and RSF's Fiesta Rally3
  `Zasada_Rally3`; no real Skins Center archive yet (links are Mega/Drive, not fetchable here).
  The cars added from gvrc.de keep their model in the files folder's `Cars/<model>/`; of 99 only one has a `skins/`
  folder (a stray `.rar`, no skin folder), so they offer imported skins only.
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
- Car setup page layout (`setup_page/`, redesign 2026-11-18, ours): three columns under the title (back link to the
  opener, car · stage) and the setup header (picker, compare-free buttons Save / Save as… / Revert / Default values /
  Delete…, prompts as before). Left (240): a small overview of the car from above (`board::overview`, parts pick the
  area; bar weight = anti-roll stiffness, diff fill = locking torque, disc glow = brake share, outlined orange where
  their own values differ) over the category list (`board::list`: name, leading values, count badge, tooltip on what
  the values mean). Centre: the selected area's editor, scrolling. Right (260): "Compared with" (default, as opened or
  a saved setup; `Compare`) and the lines that differ from it, each with ↺ and a click to jump to its area (an axle
  changed alike is one line, a diff map one line). Each track carries the comparison's tick and the stretch to it, the
  number its signed difference, an area's "Reset to compared" takes all of it back (`values.rs`, a `Field` table
  per car: spec, range, the wheel's pair). Per-wheel areas edit an axle's two wheels together while they agree (a
  difference always shows both). Diagrams per area (`charts.rs`): spring coil-overs and their front share, the dampers'
  force over wheel speed, each wheel's lean from the front, brake and anti-roll balance bars, the diff maps' lock curves
  dragged by the game's steps. Hints under each value are general rules of thumb, except the handbrake release and
  left-foot threshold, which say what `rbr_physics::vcu::locks` does. `--screenshot` also writes
  `setup-changed-<n>.png` with some values stepped.
- The setup page's hints against the physics (2026-11-12; the code, and the i20 on the pads with one value changed,
  `notes/tyre-model.md` "Setup's reach"). Camber: the value is the axle's tilt on its strut and runs against the camber
  (a degree more of it leans the top 0.96-1.16° further in; the i20, Fiesta Rally2, quattro A2, Escort Mk II and the
  slot cars 0 and 5 alike), so the hint says so and the diagram draws each wheel's lean against the compared setup's
  upright outline: the camber itself needs the strut's lean at the ride height, which the launcher does not have.
  Pressure: NGP's factor scales the tyre's stiffness, and 1.8-2.6 bar move the limits 1.1 % at most, so the hint no
  longer promises grip on loose ground. Ride height: +10 mm of spring length or platform lift the car about 10 mm, the
  helper's length 0.4 mm per mm (closed at rest: its rate at 5 or 20 N/mm gives one height), and the helper's two hints
  are apart. Brakes: 55-70 % of front share move the i20's slide under braking by under 1° while its centre diff locks
  under braking, which the hint adds. Roll bars: the hints hold on the steady circle (sideslip at the limit 13.1° to
  10.7° with the front bar doubled, 13.5° with the rear's) and turn round under braking in the turn (33° and 15° against
  24°), so they say "in a steady corner". The fast bump takes over past its speed and never at 0, the helper works under
  its closing force, the centre opens at the handbrake's release (`spring_damper::step`, `vcu::locks`). Not checked: the
  diffs' "less turn-in" (mixed under braking), the response claims, the top mount (the i20's front: 0.17° more camber a
  slot).
- Car keys (2026-11-07): a pack car's key (camera nudges, `[ffb_cars]`, `--car-key`, championship entries) is
  `catalog::pack_key`, `pack-` and its pack folder's letters and digits without RSF's `_ngp<N>`, so RSF's and the
  added copy of a car are one key (all 102 of gvrc.de's list match RSF's packs this way). Before, it was
  `rsf-<number>`: RSF's car number, or the added cars' own count, which collided with RSF's (13 cars here).
  `Catalog::renamed` maps the old keys (RSF numbers first, an added count only where RSF has no such number); the
  settings' sections move at load and at each save, a continued championship's car keys at its load. A car both
  sources have stays listed twice (user decision 2026-11-07), one model of a championship field
  (`class_cars`); the detail panel's Source row says which copy it is.
- Force feedback (2026-11-08): the Controls page says what Strength does ("how heavy the wheel feels, for every
  car"), the global row reads "Sensitivity for cars without their own", and a listed car's sensitivity says on hover
  how often the wheel maxed out at it over the car's tracked load on that surface (20 s or more, `[ffb_load]`,
  `ngp.md` "Force feedback"), with a caption to adjust in the pause menu while driving. The strength is written to the
  file on change; the save carries `[ffb] strength`, `[ffb_cars]` and `[ffb_load]` over from the file, and the form
  reloads them when the viewer exits. `--ffb-sensitivity` is unchanged. Smoothing (2026-11-12; off, or 1-10 ms,
  `ngp.md` "Force feedback") sits next to the strength and is written and carried over the same way (the linearity
  control went 2026-11-13: it follows the force-feedback model, `ngp.md` "Linearity"). Device (2026-11-12; `[ffb] device`,
  `ffb::devices`): "first found" (the first with a constant force) or one of SDL's haptic devices by name, listed
  unopened when the list is opened (SDL's init takes 0.11 s with a G923, so not on the page's rescan); one without a
  constant force is listed too, each name once (2026-11-17). Not checked by a click: the page's capture draws the list
  closed. Under it (2026-11-17, issue #7): "Last drive: on NAME", or "Last drive: no force feedback. If your wheel is
  listed more than once in Device, pick another of its entries." with the reason on hover, from the newest drive log
  with `ffb::LOG_ON`/`LOG_OFF` (read at the form's load and after each drive). The viewer tries every device a choice
  fits until one takes the force (`ngp.md` "Windows").
- The pause menu's Force feedback page (`hud::ffb_page`, with a wheel, once FFB1 says the surface): Sensitivity (the
  car's percentage), "Use suggested (X %)" (sets Sensitivity on the player's select; shown with 20 s of load and a
  suggestion other than the sensitivity; "the most" at 500 %), Strength (every car, 0-100 % by 5, live through
  `Wheel::set_strength`, `[ffb] strength`), Smoothing (every car, off to 10 ms by 2, live through
  `Wheel::set_smoothing`, `[ffb] smoothing`), "Use the global sensitivity" (the car's own dropped)
  and Done, with a line saying what the selected row does. Under them a 420×110 graph: the force at the percentage
  and the model's linearity (wheel force over steering load), the car's stored load on the surface plus this drive's as bars (square-root heights), the
  maxed-out region (force > 95 %) shaded. Then one status line (`load::advice`, in order: under 20 s drive more;
  maxed out 5 % or more, raise to the suggestion; below 0.7× it, raise (the usual cornering sits high in the range);
  above 1.4× it, lower or raise Strength; else good: the usual cornering at 0.53-0.78 of the force) and a
  guide: set Strength once, then Sensitivity per car until the wheel rarely maxes out. Nothing changes without the
  player. `viewer --screenshot --pause-menu ddde` draws it (no wheel needed).
- Install findings are dated data, not code: RSF install (2026-10-29, 525 stage folders) resolves LuppisV3 on 511
  stages (477 as the newest-file rule did, 33 where a newer `_default` won, 15 `_default`-only keeping their own
  notes, one stray file); the note-set rule itself is `codriver_packs::set_files`.
- Replays (ours): every live start records one (`--save-replay <files>/replays/<nanoseconds>-<stage>.rrp`,
  full history kept; `viewer --replay FILE [ARGS...]` watches it again). The file holds the viewer arguments that
  drove and the stepping process's frame log (`rbr_recording::replay`); watching runs the drive's frames again
  from its last start, so a replay needs the same install, setup and profile, else it stops with a digest error.
  The setup is kept beside the replay (`.rrp.setup`, copied by `rbr-standalone --replay-out` from its `--setup`,
  read by `--replay` in its place): the header's `--setup` is the launcher's `setup-current.lsp`, which the next
  drive overwrites, so five of seven drives of 2026-11-15 differed at frame 0 until their setups were rebuilt.
  Replays from before have no copy and run on `--setup`'s file.
  While watching there is no device input, pause menu or ghost saving: P or Space pauses, R starts over, the
  camera key cycles the views, V resets the mouse look, Esc closes. A replay screenshot (`--screenshot PNG
  --at N`) draws the Nth answer headless, the way a live screenshot draws the Nth payload: the replay
  runs back to back (`--replay-pace free`), not in lockstep (`--frames -` takes no `--replay` beside it);
  `--at-drive` takes no `--replay`. `rbr-standalone --live ... --replay F --replay-pace free --replay-trace OUT`
  writes each stepped frame's car and tyres as text (research, read-only; `replay_trace.rs`; with `--tyre-probe F
  [--tyre-probe-every N]` the tyre frames as the autopilot's probe, `harness/replay_gg.py`). `--replay-tyre STEP:MODEL` (research) runs the replay checked to
  STEP, then fits the tyre model MODEL (`ngp`, `brush`) afresh and runs the drive's inputs on uncompared.
- Championship layout (redesign, 2026-11-18): one setup screen (presets list left, the builder's Event / Rules / Length panels right,
  the itinerary editor in a collapsible section, a saved championship as a Continue bar with Abandon… behind a confirmation, the live
  summary and Start with its blocking reason in the pinned footer; no separate builder view, the first preset is selected on open).
  A championship under way: header (back link, rally, step, place), a scrolling itinerary strip (tiles per stage and service, your
  place per stage, click a driven stage for its times), the step's card left (next stage with Position, Start order, Tyres, Setup
  and Edit… only where a choice is allowed; welcome; service park; Super Rally restart; rally result; championship result),
  bulletins and calendar under it, the standings right with an Overall / stage-times toggle (`table.rs`, scrolling) and the season
  points; the step's primary button is the footer's (Drive disabled with the reason when the launch is blocked, the stage is driven
  or the physics is experimental, which refuses championship stages).
- Championship tab (`championship_page/`, on `rbr_season::event::Event`; 2026-11-05): the one mode of the tab; the classic 1.02 season
  page (`rally_page`) is gone from the launcher, and its saves in `SavedGames/pf<profile>.rbr` and the settings'
  `championship_*` keys and `[season]` section are no longer read (the files are left alone; the settings file drops the
  keys at its next save). The tab opens on the preset cards (Quick rally, Rally weekend, Short championship, Season; no Random, user 2026-11-18; the builder sets rallies, legs and stages), Continue first when
  `SavedGames/championship-<profile>.txt` exists (carried damage in `.dmg`, the service's late penalty and the tyre in `.ini`:
  `Event` keeps neither). A preset opens the builder (car through the car browser, `Target::Championship`, defaulting to the
  Drive page's; class = the car's group, only that class's cars fill the field, Field cars leaving models out; length, surface, time of day,
  weather, damage, skill level, team, difficulty Gentle/Club/National/Pro, adaptive, field, seed) beside a live summary
  and Start, the itinerary below (`plan`): the seed's plan, drawn again as the settings change, editable (change a
  stage through the stage browser, `StageBrowser::target` `Championship`, move, remove, add stages, legs and rallies,
  rename) and then the player's own until Draw again; Start builds it with `Event::build_planned`. The skill level is the game's three (`SSKILL`) under `rbr_season` names: Rookie 10 s, Professional 20 s,
  Champion 35 s and a stuck car is not helped (`--championship rookie|professional|champion`); the help text shows the
  penalty. The Team row (service crew) lists `rbr_season::team::team_offers` over the roster of `ai.ini`/`aidrv.ini`
  (`EventConfig::team`; `Event::crew()` sets the service park's repair times). The AI tables (`ai::load_ai`) are read
  once with the page; when they do not read the builder says so and Start stays off. The pool is every listed,
  startable map (`pool::items`; always every source, no Stages from row, user 2026-11-18: `StagePool::All`,
  the season crate keeps the others for saves), its length from the driveline (`.trk` kind 0x14, a BTB folder's `driveline.ini`),
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
  flags, `view.rs` the screen switch, `builder.rs` the setup screen, `running.rs` the championship's screens,
  `cards.rs` its cards, `table.rs` the standings' table, `results.rs` what they read from the results, and `common.rs` the shared cards, places, times and the service park. `--screenshot` draws them on an event the real `Event::build` makes from `championship_page::fixture`'s stages and AI stand-in
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
- Times, after Replays in Library (`times_page.rs`, log in `times/`; user decisions, 2026-11-23): your best time per stage by car
  class or by car, per road (dry, damp, wet), under one physics id (`rbr_recording::physics`; a dropdown, default the physics a drive steps
  now), so NGP and ReRBR runs are never ranked together. Browser pattern: Best per class | Best per car, search, class, road, sort;
  inspector with the splits, "Your runs" (20), Watch and Drive again (the Rally page's car and stage). The Rally page's stage column
  says "Your best: time · class · road" for the stage and class under the current physics. Log: `times.tsv` (`gameplay.md` Saves).
- Replays, last in Library (`pages::Section::pages`, `replay_page.rs`): cached, newest-first headers from
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

- Quality keys (`graphics_page::Graphics`): `lighting`, `particles`, `grass`, `windscreen`,
  `motion_blur` (`--enhance blur`, user decision 2026-11-11: an option, off by default), `wet_roads`, `detail` (`high`, or `ultra` for the Graphics page's "Sharper shadows and reflections" switch), `mirrors`; unset takes the Ultra preset. The preset box is not stored: it shows the preset the
  keys match, else Custom; presets leave `motion_blur` and the render scale alone (`Quality::over`). `contact` and `marks` are always on (`ALWAYS_ON`, user decision 2026-11-19); their saved keys are not read. `cabin` (the cockpit light) follows `lighting` (one switch, "Sky, sun, shadows and cockpit light", user decision 2026-11-19; the saved key is not read). A saved `surfaces` (the removed scanned surfaces) is not read. `grass` is `off`, `light`
  or `dense` (`graphics_page::Grass`; the earlier `on`/`off` switch's `on` is not read, so it takes the default) and draws
  only with `lighting` (`viewer`: the grass level and lighting): "Grass by the road" is greyed out while lighting is off,
  its saved value kept.
- `perf_log` defaults off; on, the viewer logs with `viewer --perf-log <files folder>/logs` (`home::LOGS`, shown on the Graphics page).
- `present` (`viewer --present`) defaults to mailbox, vsync on macOS: Metal has vsync or immediate only, so the launcher
  offers those two there (`Present::OFFERED`) and shows a saved mailbox as vsync (2026-11-02). `max_fps` (`viewer
  --max-fps`, one of `FRAME_CAPS`, 0 none) is the Frame cap: uncapped held at the display's rate.
- `hdr` (`viewer --hdr`, Graphics > Display > HDR) defaults off: the highlights past an HDR display's SDR white where the
  surface has extended-range sRGB (macOS EDR), else the viewer draws SDR (2026-11-05; renderer note, `hdr` row). Offered
  and passed on macOS only (`present::HDR_OFFERED`): DX12 has no such space, Windows' Vulkan drivers scRGB and PQ only.
- Triple screens (`viewer --triple`, Graphics > Triple screens): `triple` (off), `triple_diagonal` (27 in, 10-65),
  `triple_bezel` (10 mm, 0-60), `triple_distance` (70 cm, 30-200), `triple_angle` (`auto` or 0-90, the side screens'
  turn), `triple_monitors` (left,centre,right names: with fullscreen, `--triple-monitors`, a window a monitor;
  unset or windowed: one window split in thirds). The FOV sliders set the centre screen's field of view; the true
  field of view readout takes a third of a spanned display's or the window's aspect.
- `[telemetry]` `udp = ADDR:PORT` (Settings page, Telemetry's UDP address; unset off): `rbr-standalone
  --telemetry-udp`, NGP's UDP packet ([ngp.md](ngp.md#udp-telemetry)). A value without a port after its last `:` is
  not read, sent or saved; the field warns while it holds one.
- Circuit page (`circuit_page.rs`, `notes/session.md`): the Drive page's twin for hotlapping, tab 2. It lists the BTB
  stages with `circuit = 1` (`Btb::circuit`, circuits only: the Drive page lists them as ones that cannot start, never
  takes one as its default, and its Start on one offers "Open Circuit") and the stages a scan on a thread finds to end
  where they start (`rbr_btb::circuit`), which stay stages on the Drive page too. Its Circuit field is the stage
  field's: the selection, starred, Browse… into a second `StageBrowser` over those maps ("Choose a circuit", its facts
  the best lap and the weather). It takes the Drive page's car
  (its own browser `Target::Circuit`), setup, weather, time of day, road and tyre (the page runs them with `Form::map`
  set to the circuit while it draws, then puts the Drive page's stage back) and adds the session's length (free
  practice or 1, 3, 5, 10, 20 laps; saved as `circuit`, `circuit_laps`). The best lap card reads `lap-records.ini` in the
  save folder every two seconds (the drive is another process): the best lap, its sectors' own times and the car. Start
  (`Form::page == Page::Circuit`) passes `--laps`, no ghost arguments, and prepares a candidate as a ring.
