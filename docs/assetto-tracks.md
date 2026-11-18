# Assetto Corsa tracks

ReRBR converts the Assetto Corsa tracks you own into stages (experimental). Conversion reads the track's files in
place; nothing of the game is copied into the repository, and converted stages are for your own use. Do not
redistribute source or converted tracks without their authors' permission.

## In the launcher

Add the track on Import: drop a track folder, a `.zip` or a `.7z` on the launcher, or pick it in Settings. Tracks
from the sites linked in Stages > Get more stages (AC Rally Central, Assetto Corsa Club) come as archives. Each layout
of the track becomes a stage in `tracks/` (`AC_<track>`, or `AC_<track>_<layout>`), listed in Stages as "Added by you ·
Assetto Corsa import". A track with a closed AI loop and no timing gates becomes a circuit. Conversion takes seconds for a small
track and longer for a large one; Cancel leaves nothing behind.

## On the command line

```sh
cargo build --release -p rbr-ac-tracks
# Assetto Corsa's content/tracks folder: list every layout and whether it can be imported
target/release/rbr-ac-tracks import /path/to/assettocorsa/content/tracks
# Import one, with the options chosen for you (circuit, road smoothing, collision width)
target/release/rbr-ac-tracks import /path/to/assettocorsa/content/tracks --track thizy
# Fixed options: omit --layout for a track with one root layout
target/release/rbr-ac-tracks ac --track /path/to/crequy --layout crequy_2025 --name Crequy_2025
```

Output defaults to the player's `tracks` folder (`$RERBR_HOME`, else `~/.local/share/rerbr`); `--out DIR` writes
`DIR/<name>`. `ac --help` lists the options. `--replace` only replaces a folder that carries the conversion report
`ac-conversion.json`.

## What it needs and leaves out

- KN5 v5/v6 models, a v7 `ai/fast_lane.ai`, `data/surfaces.ini` and, unless it is a circuit, `AC_AB_START` and
  `AC_AB_FINISH` markers. Protected KN5s and skinned track meshes are not supported.
- Diffuse textures keep their colour and alpha, with multilayer and detail textures baked in; normal and reflection
  maps are not drawn. CSP tree lists are drawn as crossed cards; other CSP effects, scripts and animated scenery are
  not.
- Surfaces map to RBR classes, so grip and bumps approximate AC's. The AI line is the driveline; pacenotes are
  generated from the route and should be checked by driving the stage.

Details and the verified stages are in [notes/assetto.md](../notes/assetto.md).

## Checking a stage

```sh
target/release/rbr-ac-tracks prepare --track /path/to/tracks/MyStage --out /tmp/my-stage-prepared
target/release/rbr-ac-tracks probe /tmp/my-stage-prepared
python3 scripts/btb-autopilot.py MyStage --tracks /path/to/tracks --require-finish
python3 scripts/btb-shots.py /path/to/tracks/MyStage --count 6
```

`prepare` reads `$RBR_DATA/Maps` without `--maps`. `probe` samples both wheel tracks for missing surface, steps and
creases. The autopilot and the shots need release builds of `rerbr`, `rbr-standalone`, `viewer` and `rbr-ac-tracks`
and the game data from `dev.env`. `RBR_ASSETTO=/path/to/track cargo test -p rbr-ac-tracks` also reads every KN5 of a
local track.
