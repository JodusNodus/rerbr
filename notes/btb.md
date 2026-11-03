# BTB stages

BTB (Bob's Track Builder) stages as RallySimFans installs them (`RX_CONTENT/Tracks/<folder>/`) and its `rbr_rx.dll`
plugin plays them. Code: `crates/rbr-btb` (`track` the folder readers, `classic` the `trk`/`mat`/`dls`/`lbs`
builders, `ids` the RSF stage ids, `xfile`/`xform` the meshes; tests `crates/rbr-btb/tests/`). Facts with their evidence, one line each.

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
- `Objects.zip`/`Textures.zip`: the `.x` meshes, `.xform` placements and textures ([Meshes](#meshes)).

## Built files

- The plugin builds `CACHE/trk.trk`, `mat.mat`, `dls.dls`, `lbs.lbs` (19 folders cached locally): the builders
  return bytes identical to every cached file (`tests/tracks.rs`).
- `.trk`: one `(3,0x14)` driveline chunk (first seven `K` values as `f32`, the `u16`s zero) plus one `(3,0x16)`
  chunk (a `u32` mesh count plus the `ksh.ksh` mesh bytes; four cached folders build it empty).
- One `(3,0x16)` mesh per `CollisionModel=2` object (mesh count equals their count in all 19 caches) and one
  placement per clone (counts equal the `cloneCount` sums); mesh faces identical, all named `.ksh`: geometry and
  placements come from the `.x` meshes; until that is built, the builder takes the `ksh` bytes as input.
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
## Meshes

BTB ("Bob's Track Builder") stages draw from `RX_CONTENT/Tracks/<folder>/Objects.zip` (`Objects/*.x` meshes,
`Objects/*.xform` placements), placed by `objectlist.ini` `[Object_<i>]` entries (`file=<mesh>` with no extension,
`xform=<placements>`); readers `rbr_btb::xfile`/`rbr_btb::xform`, test `crates/rbr-btb/tests/meshes.rs` (`RBR_RSF`).

- Every `.x` of the install (60973 files in 72 tracks) starts `xof 0303bin 0032`: binary, 32-bit floats; no
  `txt `, `bzip`/`tzip` or `0064` file exists, and the reader does not decode those (they report `Unsupported`).
- Each file declares its templates inline and holds one unnamed top-level `Mesh`; no named objects, references or
  `Frame`s occur.
- Declared templates: always `Vector`, `MeshFace`, `Mesh`, `MeshNormals`, `ColorRGBA`, `ColorRGB`, `Material`,
  `MeshMaterialList`, `TextureFilename`, `VertexDuplicationIndices`; `Coords2d`/`MeshTextureCoords` in 60452 files
  (521 meshes have no UVs); `VertexElement`/`DeclData` in 18170, `FVFData` in 1035 (disjoint).
- Every polygon is a triangle (14,302,088 faces, all size 3); `Mesh::triangles` fans them.
- `MeshTextureCoords` holds one UV per vertex whenever present; no file has two UV sets inline and no
  `MeshVertexColors` template occurs anywhere (no vertex colours).
- Each `MeshMaterialList` has one material and all face indexes are 0; every `Material`'s face colour, power,
  specular and emissive are 0, so the material is just its `TextureFilename`.
- A `.x` texture name is a `materials.ini` section (`texturashierbapop\Materials\a2a23561` is
  `[texturashierbapop\Materials\a2a23561]`, giving the BTB effect, technique and `diffuseTexture`); the only
  exceptions are Cauterets' `...\vieuxpylone\mat*` names and one La Serra name, which have no section.
- `materials.ini` `[MATERIALS]` maps `MAT_<i>=<section>[-blend<N>]` (e.g. Pirttikulma's
  `MAT_0=Tiet\Materials\Leveasora_tumma6.dds-blend24`); the `.x` names match the section part, never a `-blend`
  variant. Some `MAT_` values dangle (30 tracks, e.g. Bareyo's `WP\Materials\piedra-blend2` with no `WP\Materials\piedra`
  section): what the game falls back to is unexplained.
- `DeclData` holds 7 element patterns of BTB `VertexElement {Type, Method, Usage, UsageIndex}` (always `Method` 0),
  e.g. `{(2,0,6,0),(2,0,7,0),(1,0,5,1)}` in 10324 files; the payload length always equals the `D3DDECLTYPE` widths
  times the vertex count, consistent with tangent/binormal/extra-UV/blend streams (`Usage` 5 with `UsageIndex` 1..3
  reads as further UV sets), but BTB's own `Type`/`Usage` numbering is not confirmed against its source.
- `FVFData` codes are 258 (926 files) and 514 (109); the payload is always 2 and 4 DWORDs per vertex respectively,
  i.e. one and two further 2-float UV sets next to the FVF texture-count bits.
- A `.xform` is raw 4x4 row-major `f32` matrices (Direct3D order: translation in elements 12..14); every length is a
  multiple of 64 and every matrix ends in 1.0 (4,671,721 placements in 141,350 files); `xform::parse` errors otherwise.
- All 140,785 `objectlist.ini` `file=`/`xform=` names resolve to `Objects/<name>.x`/`.xform` case-insensitively.

