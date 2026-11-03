# BTB stages

BTB (Bob's Track Builder) stages as RallySimFans installs them (`RX_CONTENT/Tracks/<folder>/`) and its `rbr_rx.dll`
plugin plays them. Code: `crates/rbr-btb` (`track` the folder readers, `classic` the `trk`/`mat`/`dls`/`lbs`
builders, `ids` the RSF stage ids; test `crates/rbr-btb/tests/tracks.rs`). Facts with their evidence, one line each.

## Sources

- One folder per stage under `<root>/RX_CONTENT/Tracks/` (72 locally), opened by `track::scan` when it holds a
  `track.ini` (all do, exact lowercase names, no case variants needed).
- `track.ini` `[INFO]`: name, physics (`gravel` 40, `tarmac` 19, `snow` 12, one `Gravel`), author, version,
  splashscreen (69 folders), date (49), comment, length km; `countrycode`/`surface`/`particles` on a few.
- `spawn.ini` `[SPAWN]`: `m00..m11` identity, `m15` 0, `angleX`/`angleY` 0 in every folder; the cache carries only
  `m12/m13/m14` and `angleZ`.
- `driveline.ini` `[DRIVELINE]`: `count` and that many `K<i>` 8-float points; the eighth value is 0 in every folder.
- `settings.ini` `[SETTINGS]`: only `sunDirection` (period decimals, comma list); every file carries the same
  commented-out alternatives.
- `objectlist.ini` `[OBJECTS]` count and `[Object_<i>]` in order (`Visible` 0/1, `DrawInstanced` 0/1, `Moveable` 0,
  `ShadowCaster` 0, `CollisionModel` -1/0/2, `colBox` triple): 552 `CollisionModel=2` objects across the install.
- `materials.ini` `[MATERIALS]` count, `MAT_<i>` names, one section per material: `effectName` (12 `BTB_*`),
  `techniqueName` (`Default`, `DefaultUp`, `Transparent`, `TransparentUp`), `ambient` always `0.4,0.4,0.4,1.0`,
  `SpecularPower`/`SpecularColor` with decimal commas, texture paths (absolute author paths the game never reads).
- `mat.ini` `[MAPS]` count and `[MAP<i>]` `data0..data15` rows of 16 cells (values fit `u8`).
- `pacenotes.ini` `[PACENOTES]` count and `[P<i>]` type, distance, flag; five folders hold more `[P<i>]` than `count`.
- Numbers use the author's French-locale decimal comma (`LODOut=2679,576`, `SpecularColor=0,4235294,...`,
  `colBox=0,15, 1, 0,15` in either locale); parsed by `track::decimal_floats`.
- Text files are bytes decoded lossy (non-UTF-8 `track.ini` names exist), sections/keys first-match
  case-insensitive, headers only at line starts: `rbr_formats::ini` gives Windows-identical sections here (no
  indented headers or comments in any folder).
- `Objects.zip`/`Textures.zip` (the `.x` meshes) are another worker's scope; `CACHE/col.col`, `ksh.ksh` and the
  minimaps too.

## Built files

- The plugin builds `CACHE/trk.trk`, `mat.mat`, `dls.dls`, `lbs.lbs` (19 folders cached locally): the builders
  return bytes identical to every cached file (`tests/tracks.rs`).
- `.trk`: one `(3,0x14)` driveline chunk (first seven `K` values as `f32`, the `u16`s zero) plus one `(3,0x16)`
  chunk (a `u32` mesh count plus the `ksh.ksh` mesh bytes; four cached folders build it empty).
- One `(3,0x16)` mesh per `CollisionModel=2` object (mesh count equals their count in all 19 caches) and one
  placement per clone (counts equal the `cloneCount` sums); mesh faces identical, all named `.ksh`: geometry and
  placements come from the `.x` meshes, out of scope, so the builder takes the `ksh` bytes as input.
- `.mat`: nine entries (`dry`/`damp`/`wet` by `new`/`normal`/`worn`, 11 space-padded characters) with identical
  runs: `[MAPS]` count runs of 16x16 cells each, in `data0..data15` order, no tail.
- `.dls`: a fixed 299-byte `MINAATAD` template (version 18, constant trigger-data/channel/trigger/camera words,
  `Driveline\0` plus `299 300\0\0` name block) holding the first `count` notes as the `Driveline` set's marker keys
  (`tag`, `flag`, `f32` distance); only the note count and the offsets vary.
- The plugin always reads `pacenotes.ini`, never the `pacenotes1_*` or `*TeamSumppu.ini` copies some folders keep
  (`Pirttikulma snow`'s copy holds count 196 against the cache's 211; most copies are byte-identical).
- `.lbs`: 144 bytes, a `(2,11)` start chunk (identity matrix with the spawn translation, the position twice, `(0,
  angleZ, 0)` rotation), then empty `(2,10)` and `(1,0)` stubs the game never dispatches (the retail `lbs` parser
  reads only the first two chunks: the stub is not valid retail geometry).
- Missing-newline quirk glues `69[MAP1]` onto a `data15` line (`Kinamba Yala`, `Olocau Gatova`, four more folders):
  that `[MAPn]` is not a section, the trailing token still reads 69 (`atoi`), and the map reads zeros.
- Duplicate `[MAP<i>]` sections keep the first (`Sagen Sweden 2012`'s cache proves it: five `[MAP0]`s, the first wins).
- A missing map or row reads zeros (no oracle for the uncached folders with missing rows: `Pirttikulma` MAP14,
  `Sarvana`'s empty second MAP24, `Pirttikulma snow` data12 rows).

## Stage ids

- RSF stage ids (1000 and up) per folder, first evidence wins (`ids::match_ids`): a `<id>_*.png` splash picture in
  the folder, a `rallysimfans_personal.ini` `[stage<id>] name=<folder>` entry, else a `stages_data.json` name
  matching after dropping a trailing `-`, lowercasing and accent-folding.
- Older non-BTB stages reuse names, so ids below 1000 never count: `Makkola` is also 549, `Nechanice - Presin`
  also 560.
- 70 of 72 folders match; unmatched: `BikolTardos12` (the json's `Bikol Tardos`, 1002) and `Myttaala Gravel` (the
  json's `Myttäälä`, 1059).
