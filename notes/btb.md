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


## Drawing

`rbr_btb::scene` (`Scene::load`, no renderer types) turns a folder into meshes, materials and placements; the renderer's
`stage/btb.rs` builds a `Stage` from it (`stage::load` takes it when the stage's folder passes
`track::is_track_folder`, i.e. `<RBR_RSF>/RX_CONTENT/Tracks/<folder>/track-41_M`; `load_textures` likewise). Physics and
launcher are not wired.

- Frame: `.x` vertices, `.xform` matrices and `spawn.ini` are already in the renderer's stage frame (Direct3D, left-handed,
  y up, placed `p * M`); no transform. The driveline (`z` up) is `(x, z, y)`, as the classic `.trk`'s. Evidence: the
  placement translations of Pirttikulma span x -384..3578, y 0..165, z -2920..2482 and its driveline `K0`/last point
  (463.8, -2918.6, 123.7)/(3545.4, 2459.1, 145.0) sit at stage (463.8, 123.7, -2918.6)/(3545.4, 145.0, 2459.1), inside
  that box; with the driveline points as camera positions the road is under the eye in every shot; the spawn lies ahead of
  `K0` along its tangent (Ulu Karagita 71 m, Bareyo 100 m, Pirttikulma 610 m) and `angleZ` is the yaw, car facing
  `(sin yaw, 0, cos yaw)`, the way the driveline runs.
- Effects (the `FX/*.fx` sources; every `.fx` is `texture * (0.4 + max(0, N.L))`, `L` toward the sun = `-sunDirection`,
  cull none, no fog): `VertLerp` blends `diffuse` and `secondDiffuse` by the mesh's `BLENDWEIGHT` (the `DeclData` usage 1
  stream), the second texture on the last UV set; `MultiplyAdd` is `diffuse * multiplier + additive` (UV sets 1 and 2);
  `Bump` takes the normal from `normalTexture` in the `DeclData` tangent frame; `Specular` adds
  `specularTexture * SpecularColor * pow(N.H, SpecularPower)`. UV set order after the diffuse: bump, specular, multiplier,
  additive, lerp (`Effect::uv_slots`; the `DeclData`/`FVFData` patterns agree for every mesh). `Up` techniques light with
  `N = (0, 1, 0)`. `Transparent`: pass 0 alpha blended without depth write, pass 1 alpha test `>= 200` with depth write.
- Textures: `Textures.zip` holds `.dds` (some uncompressed `X8R8G8B8`), and 1310 `.jpg` and 16 `.png` files that materials
  name (`tarmac4.jpg`): `scene::TextureFiles::read` decodes them to a plain-texel `.dds`. Materials name textures by author
  paths; the file name matches the archive ignoring case (or the stem with `.dds`).
- Drawn: diffuse, the lerp (vertex alpha is the weight; the renderer's second texture also multiplies it by the second
  texture's alpha, which BTB's does not), `Up`, `Transparent` as the two passes, but only for a diffuse texture with a
  translucent texel (`scene::has_alpha`; otherwise pass 0 and 1 together are one opaque draw), cull none, the vertex
  colour holding `(0.4 + N.L)` for the unenhanced draw; the renderer's own light, shadows and sky otherwise.
  Approximated: `MultiplyAdd` draws its diffuse (the multiplier/additive UV sets cannot share a draw's two texture slots),
  `Specular` has no highlight, `Bump` has no normal map (the renderer's stage draws have neither path). Not drawn:
  `BTB_Sky` meshes (7 in two tracks; the renderer's sky stays), the 139 meshes whose texture name has no
  `materials.ini` section (Cauterets' `vieuxpylone`, one La Serra), invisible objects. Pirttikulma's own sky
  (`Taivaita\Materials\sky16`, `BTB_Diffuse`) is an ordinary object, drawn as one. No weather, wear, light maps,
  characters, fences, grass or collision for a BTB stage (their files are absent there and load as none); the viewer's
  fences and characters report "not loaded".
- LOD and size: no instancing. Each placement is expanded into world-space vertices (88-byte renderer vertices), grouped by
  128 m cell (a placement over 150 m half extent is a mesh of its own: the terrain tiles) and by `LODIn`/`LODOut` in 50 m
  steps (`Lod::Band`, the draw's distance from its mesh's bounds centre; a solo placement's `LODOut` counts from its
  radius too). `LODOut` is under 500 m for 64775 of 140785 objects across the install, so the band matters. Worst
  expanded tracks (Transparent drawn twice where the texture has alpha): Kaihuavaara 2022 3.38 M vertices, 2.60 M
  triangles in 7070 meshes (peak 0.85 GB in a viewer screenshot); Stoccareddo II 3.79 M vertices, 1.99 M triangles, 1270
  meshes (peak 1.3 GB); Pirttikulma 1.18 M vertices, 0.74 M triangles. Load to the first frame 5 to 9 s on those. A view
  draws 60 to 350 k triangles.
- Open: no tangent-space bump or specular; `MultiplyAdd` terrain looks flat; the `CollisionModel=2` meshes are not drawn
  specially; the semantics of `LODOut` for large tiles (centre or box) is ours.
