# BTB stages

BTB (Bob's Track Builder) stages in the `RX_CONTENT/Tracks/<folder>/` folders of a RallySimFans install, which its `rbr_rx.dll`
plugin plays. Code: `crates/rbr-btb` (`track` the folder readers, `classic` the `trk`/`mat`/`dls`/`lbs`
builders, `ksh` the collision meshes, `ids` the RSF stage ids, `xfile`/`xform` the meshes; tests `crates/rbr-btb/tests/`). Facts with their evidence, one line each.

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
- Numbers are written with the author's locale (`LODOut=2679,576`, `SpecularColor=0,4235294,...`), but the plugin
  reads them in the C locale (`FUN_10008bc0`): `colBox` and `sunDirection` with `sscanf("%f, %f, %f")`, `LODIn`,
  `LODOut` and `distance` with `sscanf("%f")`, so commas separate: `colBox=0,15, 1, 0,15` is (0, 15, 1)
  (`track::scan_col_box`) and `LODOut=2679,576` is 2679 (`track::scan_float`). How the effect loader reads the
  `materials.ini` colours is not known; `track::decimal_floats` reads them with decimal commas.
- Text files are bytes decoded lossy (non-UTF-8 `track.ini` names exist), sections/keys first-match
  case-insensitive, headers only at line starts: `rbr_formats::ini` gives Windows-identical sections here (no
  indented headers or comments in any folder).
- `Objects.zip`/`Textures.zip`: the `.x` meshes, `.xform` placements and textures ([Meshes](#meshes)).

## Built files

- The plugin builds `CACHE/trk.trk`, `mat.mat`, `dls.dls`, `lbs.lbs` (19 folders cached locally): the builders
  return bytes identical to every cached file (`tests/tracks.rs`).
- `.trk`: one `(3,0x14)` driveline chunk (first seven `K` values as `f32`, the `u16`s zero) plus one `(3,0x16)`
  chunk (a `u32` mesh count plus the `ksh.ksh` mesh bytes; four cached folders build it empty).
- `(3,0x16)` is `ksh.ksh` verbatim after the mesh count ([Collision meshes](#collision-meshes-ksh)); `classic::trk_from` builds the whole `.trk` from the track alone.
- `.mat`: nine entries (`dry`/`damp`/`wet` by `new`/`normal`/`worn`, 11 space-padded characters) with identical
  runs: `[MAPS]` count runs of 16x16 cells each, in `data0..data15` order, no tail. ReRBR's own `damp<j>`/`wet<j>`
  rows in `[MAP<i>]` (converted stages; a missing row the `data<j>` one) fill the damp and wet entries, so wet
  weather reaches the physics (each `_Dry` material's `_Damp`/`_Wet` are the next two ids, 26 of 26 in 1.02's
  table); the 19 caches stay byte-identical.
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

## Collision

`rbr_btb::col::build(track)` writes `CACHE/col.col` byte for byte (19 of 19 cached folders, `tests/col.rs`; 0.2 to 1.9 s per
stage, Sagen Sweden 2012 the slowest). `rbr_rx.dll` addresses are RVAs from the base 0x10000000; the pipeline is
`ColSplit` 0x10022c10 (`col.cpp`) -> gather 0x10022770 -> `Split` 0x10025e80 -> `DumpCol` 0x100243a0.
- `CollisionOnly=1` (ReRBR's key, unknown to the plugin) puts a `Visible=0` object's faces in `col.col` too: an undrawn collision mesh (converted stages).
- `SunCascades=0` (ReRBR's key) keeps an object out of the sun's three near cascades; the far layer, the lamps' map and
  the bakes still take it. Absent, it casts, as every object did before: `ShadowCaster` stays unread, since all 72
  folders write 0 on every object (140785 of 140857; none 1) and still need their shadows. Converted stages write it on
  their forest back from the road (`beamng-rbrbtb`): Sadamine's 6 M-triangle forest at its start (1250x2054 Ultra, wet,
  16:58) cascades 0.40/2.37/1.38 to 0.28/0.74/0.49 ms, the bench 93/62 to 120/71 fps (avg/1 % low); the road and the
  trees by it keep their shadows, the slopes back from it lose their dapple. 36 reference shots byte-identical
  (2026-11-09).
- `converted`: the player's folder (the launcher's and converters', one function) and its `converted` BTB root;
  BeamNG levels convert there with `beamng-stage`, in its own repository (`beamng-rbrbtb`) since 2026-11-09.
- Faces: objects with `CollisionModel=0` and `Visible` not 0, in `objectlist.ini` order, clone by clone (gather 0x10022770); the
  clone count is the `.xform` size / 64 (`ini` `cloneCount` ignored: Crastazza `obj_124_2` has 22 matrices, `cloneCount=5`).
  `Visible=0` objects are absent (Le barrage des gorges `wall39_*`, bounds differ at the root without this).
- A mesh whose `VertexDuplicationIndices` point forward or at a non-original vertex (`indices[i] > i` or `indices[indices[i]]
  != indices[i]`) never loads, so its objects give no faces (Crastazza `VWPassat` cars and `Fence_marshall`; the same files hold
  `FreeLander`s with valid indices that load). Faces with two equal vertex indices are dropped (Myllylahde, 12 faces in one cell).
- Vertices (GetMeshFaces 0x10021fa0): record order `(v0, v2, v1)`; UVs likewise, a mesh without UVs reads 0. Windows D3DX hands out every
  non-zero position one ulp smaller (bits minus one, 34883/34883 on Dylanda, all 18 Windows caches); UVs are exact. An exactly
  identity matrix (x87 compare, 0x10023340) skips the transform, else `out = (x*A + y*B) + (z*C + D)` over the file matrix
  columns `A=m[j], B=m[4+j], C=m[8+j], D=m[12+j]` in `f32` (D3DXVec3Transform), then y and z swap.
- Wine's D3DX (`Kutemajarvi 1994`, cache of 2026-10-15; the other 18 date from 2025 and March 2026): positions are the file's floats,
  the sum is `((x*A + y*B) + z*C) + D`, and no triangle gets a map index (`D3dx::Wine`).
- Nodes: bounds are `(centre, half extent)` over the vertices, `f32`, `(min+max)*0.5` and `(max-min)*0.5` (0x10025250). A node with
  fewer than 9 faces is a leaf. `SortFacesRightLeft` (0x10025650) takes the AABB of the face centroids (`((a+b)+c)/3`), the
  longest axis (ties lowest) and puts a face right when `centre > centroid`, else left; a side left empty retries axes 0, 1, 2
  with the same centre.
- Regions (0x10025b60, 0x10025e00): a split node is a region node when both half extents x and y are below 100, or it holds
  fewer than 600 faces, or a child is a leaf; its two children are the cells, in preorder. A leaf child has no cell: its
  faces are not in the file.
- Main tree (0x100241c0, 0x10025c60): root, then per non-leaf node a child pair `{bounds, count 0, pointer}` in preorder
  (pointers from the tree root, 0x40 per pair); under a region node the pair are placeholders `{bounds, 0x200000, 0}`.
  Header `{OC7R, 0x10, cell count, table offset}`, the 0x30 root block `{0x304, 0, 0, 0x30, 0x30, 0, 0, 0x30, 0, 0x30, 0, 0}`,
  the tree, the cell offset table, the cells.
- Cell (0x10026860, 0x10026470): `{0x104, graft path, vertex count, 0x14, tree rel}`, three vertices per face (no dedup) in
  leaf preorder, the root `{bounds, 0x200000, 0x20}`, then each node's child pair followed by the children's subtrees; a
  leaf holds its 18-byte triangles padded to 4. The graft path is the depth in the top byte and, LSB first, one bit per step
  from the root (right child 1).
- Triangle: three `u16` indices of the first table entry with the same position and UV (exact `f32` equality, kept to 16
  bits: a cell over 65535 vertices wraps), in the order `(rec1, rec0, rec2)`; `0x422f` twice; the map index twice (0 for
  none); then `nibble[k]` for those entries twice, `nibble = (u8)(int)(u*15) | ((int)(v*15) & 0xf) << 4` (truncating; the
  low byte of `u` is not masked, so `u*15 >= 16` reaches the high nibble; the entry named by the wrapped 16-bit index
  supplies the UV). The caches of Le barrage des gorges and Ulu Karagita hold such wrapping cells.
- A converted stage (any `CollisionOnly` object) writes a cell that would wrap with each distinct (position, UV) once,
  in first-occurrence order, up to 65536 (about six times the faces of a shared mesh); the loader uses only the
  vertices' address. `col.rs` unit test; the 19 caches stay byte-identical.
- Map index (0x10021fa0): the mesh's first material name -> its `materials.ini` section -> the base name of `diffuseTexture`
  -> the first `mat.ini` `[MAP<i>]` whose `texture=` base name matches (case-insensitive). The material has no texture
  when `Textures.zip` lacks the file (Kuri Bush 1 `OBJ_RALLY`, Mattila `Blank_255_255_2555`) or it is a `.png` (Olocau
  Gatova `texture0.png`, `texture01.png`): then the index is 0.
- Not reproduced (no case in the 19 caches): the plugin drops a region whose subtree face count differs from the node's
  (`ColSplit` loop), a split that leaves a side empty, a root that is a leaf or a region.

## Stage ids

- RSF stage ids (1000 and up) per folder, first evidence wins (`ids::match_ids`): a `<id>_*.png` splash picture in
  the folder, a `rallysimfans_personal.ini` `[stage<id>] name=<folder>` entry, else a `stages_data.json` name
  matching after dropping a trailing `-`, lowercasing and accent-folding.
- Older non-BTB stages reuse names, so ids below 1000 never count: `Makkola` is also 549, `Nechanice - Presin`
  also 560.
- 70 of 72 folders match; unmatched: `BikolTardos12` (the json's `Bikol Tardos`, 1002) and `Myttaala Gravel` (the
  json's `Myttäälä`, 1059).
## Collision meshes (ksh)

`rbr_btb::ksh::build(track, D3dx)` rebuilds `CACHE/ksh.ksh` (`WriteKSH` 0x1001dee0 of `rbr_rx.dll`), the `(3,0x16)` mesh bytes of
`trk.trk`; `classic::trk_from` wraps it. `tests/ksh.rs`: 19 of 19 cached folders identical in ksh and trk (placement key words masked).

- One mesh per `CollisionModel` 1 or 2 object, in `objectlist.ini` order (counts equal in all 19 caches; no `CollisionModel=1` object
  exists locally, it only sets the first flag byte); `CollisionModel` 0 and -1 objects go to `col.col`, never here.
- Mesh: `.ksh\0`, six flag bytes (`CollisionModel == 1`, then zeros), `u32` 0, `u32` 8, 8 corners, `u32` 12, 12 faces (tag byte 0, then
  `a, c, b` of the fixed box triangle table `FACES`), `u32` clone count, placements `{key, pos, scale, quat}` of 44 bytes.
- Clones = the `xform` matrix count (file size >> 6, as `ReadClones` 0x1001dc40); a missing xform file is one identity clone. The ini
  `cloneCount` is not used (equal in all 19 caches anyway).
- Placement key: the address of the matrix in the plugin's heap (`0x78B5D350`, +64 per clone, other base per run): not reproducible,
  written as 0, the tests mask it. Nothing reads it (`rbr_formats::trk` skips it); the physics uses
  a mesh's key only as `host.interactive_handle(object.interactive_id)` for kinds other than 0 and 5 (`stage::wants_handle`), and handles
  come from the `.lbs` `(1,6)` records, which a BTB `lbs.lbs` lacks, so any key resolves to no handle. The object kind is the first flag
  byte: 0 (static) for `CollisionModel=2`, 1 (cone) for `CollisionModel=1`: `wants_handle` is false for all local meshes.
- `colBox` is read by `sscanf("%f, %f, %f")` (`FUN_10008bc0`): `0,1, 3, 0,1` is (0, 1, 3), the commas are separators, not decimal
  commas (`track::scan_col_box`). Evidence: first corners `(-0, -1.5, -0.05)` of that box.
- Box corners `(a, b, c)` = colBox: `ex = a*0.5`, `ey = c*0.5`, `ez = b*0.5` (f32), corner bits x:1 y:2 z:4, `x = ∓ex`, `y = ∓ey`,
  `z = -ez * 0.1f` (corners 0-3) or `ez + ez` (4-7); `-0.0` stays for a zero extent.
- Without `colBox` (31 objects, La Serra): the `.x` bounding box, positions after the D3DX load shift (nonzero float bits - 1), min/max
  and `c = 0.5 * (min + max)` in the file frame, corner `i` = `(min|max) - c` by bits x:1 y:2 z:4, written `(x, z, y)`.
- Placement: position `(m12, m14, m13)`, scale `(|row0|, |row2|, |row1|)` with `|v| = sqrtf((x*x + y*y) + z*z)` in f32 steps (x87
  at 24-bit precision under Direct3D), quaternion `D3DXQuaternionRotationMatrix` of the raw scaled matrix then
  `D3DXQuaternionNormalize`, written `(x, z, y, -w)`.
- Float rules: all arithmetic is f32, no fused ops. Native reproduces all 41804 placements of the other 18 caches, Wine all 3490 of Kutemajarvi.
- Two D3DX builds produce the caches (cache file times: Windows' 100 ns stamps vs Linux nanoseconds): `D3dx::Windows`
  (14 caches, `d3dx9_30.dll` from a Steam prefix, 2388176 bytes) and `D3dx::Wine` (Kutemajarvi 1994, built 2026-10-15 under Wine's
  builtin DLL). The tests use whichever reproduces the cache.
- Native `D3DXQuaternionRotationMatrix` (x87, 0x443258; the CPU variants do not replace it): `trace = (m11 + m00) + m22`; if `trace > 0`:
  `r = sqrt(trace + 1) * 0.5`, `w = r`, `inv = 0.25 / r`, `x = (m12 - m21) * inv`, `y = (m20 - m02) * inv`, `z = (m01 - m10) * inv`; else
  with `d = [m00 - m11 - m22, m11 - m00 - m22, m22 - (m11 + m00)]`, `i` = first largest (`d0 < d1` picks 1, then `d[i] < d2` picks 2),
  `(j, k) = (i+1, i+2) mod 3`: `r = sqrt(d[i] + 1) * 0.5`, `q[i] = r`, `inv = 0.25 / r`, `q[j] = (m_ji + m_ij) * inv`,
  `q[k] = (m_ki + m_ik) * inv`, `w = (m_jk - m_kj) * inv`.
- Native `D3DXQuaternionNormalize` (SSE2 variant 0x6117fd, selected when the CPU reports SSE2): `l = (x*x + y*y) + (z*z + w*w)`,
  `r0 = rsqrtss(l)`, `s = (3 - l*(r0*r0)) * 0.5 * r0`, each component `* s` (the x87 generic one is not what the caches used:
  18% match). `ksh::rsqrtss` reproduces Intel's instruction: table over exponent parity and the top 10 mantissa bits, entry =
  `1/sqrt` of the interval centre rounded to 12 mantissa bits (0 differences over all 2048 entries and 200000 random inputs on an
  i9-13900HX); AMD's table differs, an AMD-built cache would not match.
- Wine: `trace = m00 + m11 + m22 + 1`; if `trace > 1`: `s = 2*sqrt(trace)`, `x = (m12 - m21)/s`, `y = (m20 - m02)/s`,
  `z = (m01 - m10)/s`, `w = 0.25*s`, else `D3dx::Wine`'s `math.c` branches on the largest diagonal; normalise by dividing by
  `sqrtf(((x*x + y*y) + z*z) + w*w)`. Unverified: the D3DX load shift under Wine (Kutemajarvi has no mesh-bounded box).
- Open: the sscanf float conversion is assumed correctly rounded to f32 (MSVC's may round via double); `colBox` with fewer than three
  numbers falls back to the mesh box (no local case); a mesh with several materials or unusual `.x` positions was not seen.

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


## Playing

Evidence: the reverse-engineering report of `rbr_rx.dll` (Q2). Code: `rbr_btb::stage`, the launcher's `btb.rs`.

- The plugin makes the game load retail map 41 variant M and replaces only `Maps\track-41_M.{lbs,dls,col,trk,mat}` (hooks on the file opener `0x401c28`, `0x46a481`, `0x49f433`); it forces map number 41 at the track start (`10003c40(0x1660804, 0x29)`). Everything else stays retail map 41's there: `track-41_M.fnc` (fences, physical), `.ini`, `_textures.rbz`, `Tracks.ini [Map41]` (particles), `TrackSettings41.ini`, the season rows, ghosts and times by map number.
- ReRBR has no file opener to hook, so a BTB stage is a stage of its own (2026-11-04), a folder stage like a community one: the number is its RSF id (`ids`), else `1_000_000 + FNV(folder) % 1_000_000` (`btb::synthetic`), the name `<files>/btb/<folder>/track-<number>_M` (`stage::stage_name`; one variant, the files do not depend on the weather). The number is a `u32` everywhere it is stored (stage name, the ghost header's map word and `_NN.ghc` names, `community-records.ini` sections, the `pfresult.rst` row id).
- The surface is `track.ini` `physics=` (`E14_4a1330`, 0x4a1330: 0 gravel, 1 tarmac, 2 snow; gravel when `physics` is missing), passed as `--surface`.
- `rbr_btb::stage::prepare(track, number, out)` writes `track-<number>_M.{lbs,dls,mat,trk,col}` (`classic` `lbs`/`dls`/`mat`, `trk_from`, the `col`) and map 41's `.fnc` and `.ini` copied from the install and `btb.ini` (`[BTB] folder=`, `stamp=`) into `out`, `<files>/btb/<folder>/`. The plugin rebuilds them on every load; we keep them while `btb.ini`'s `stamp=` (the build number and the track folder's file names, sizes and times) matches, which saves 0.5 to 2 s a Start. `stage::source(folder)` reads `btb.ini` back (the renderer's and the particles' hook). ReRBR never writes the game folder.
- Map 41's fences, `.ini` and characters stay, as under RSF (user decision 2026-11-04, so a BTB stage looks and collides as under RSF): `prepare` copies the install's `track-41_M.fnc` and `.ini` (`stage::RETAIL`, one place to drop them) next to the generated files, and the characters' script folder, smoke and the physics' `objects_before_car` go by `SKY_MAP` (`--btb 41` sets `Race.map`). `rbr_formats::stage_files::file` has no fall back to `Maps\` (nothing else used it). The `.lbs` has no characters of its own.
- Weather: the stage offers all of map 41's rows (every variant, good and bad) as a season of its own (`Season::of_btb`): the rows keep their words under the stage's number, the weather's letter is M (the one stage file) and the sky section is the row's own words in map 41's `TrackSettings.ini` (`Weather::section(season)`, `Season::sky`). Physics inputs (wear, wetness, wet meshes, the `.mat` entry) come from the row as for any stage. `rbr-standalone --btb 41` makes the season from the retail table.
- Presentation borrows map 41's through one constant, `stage::SKY_MAP` (and `SKY_STAGE`, `track-41_M`): the sky sections, the wind (`TrackWeather`), the particle file when the stage names none (`Tracks.ini [Map41]`, tarmac), the time of day's site (`relight::site`, `rbr_render::stage::sky_map`). 
- Records and ghosts are the stage's own number's, in the player's normal records folder, like a community stage's (`community-records.ini`, `rb_<number>.gho`, `_NN.ghc`); it has no `times.ini` reference time (a map 41 stage did inherit its 168.00 s). The jump start is armed whatever the number (`Game::with_forced_jump_start`, `rbr-standalone --btb`): a retail or community stage arms it when `number % 10 < 8`, and the plugin's map 41 always was.
- `.col` is `rbr_btb::col::build`; every folder is offered (a build error shows at Start).
- `lbs::parse` accepts the stub `(1, 0)` chunk (a zero count, no sky word) so the boot reads the start location; `rbr-standalone --surface gravel|tarmac|snow` is the surface override.
- Launcher: Start passes `--surface`, `--btb 41` and the usual `--saves`/`--profile`.
- Own-number stages (2026-11-04, Yaris Rally2): Hokkara Gravel (1055) and Pirttikulma (1103) finish on map 41's good row 98 and its bad row 102 (wet), with the record under `[1055]`/`[1103]` of `community-records.ini` and no inherited time; 24 fence posts (map 41's), 0 missing textures.
- Autopilot drives (`--autopilot --boot`, NGP car 2, row 98) finish Hokkara Gravel and Pirttikulma snow with their splits (2026-11-03); on Ulu Karagita the autopilot leaves the faint savanna track at 3262 m at some speeds (12, 14, 24 m/s; 18 passes): the collision there is smooth and the `.col` the cache's.
- Particles (ours, 2026-11-03): map 41's `PS_MontBlanc` handles only tarmac and grass, so a gravel BTB stage threw no
  dust and the car stayed clean (Kuohu's surfaces 13, 17, 21, 41, 49 have no handler or alias in it). The particle
  file is the `track.ini` `particles=` (2 folders: `PS_Arctic`, `PS_USA`), else `PS_British` on gravel, `PS_BTBSnow` on snow and map 41's on tarmac (`particles::btb_particles`; all 72 folders read and handle their
  gravel, test `btb_stages_throw_their_surfaces`). RSF's `PS_BTBTarmac` does not read: 112 material aliases declared,
  116 listed (4 tab-separated lines added by hand), and nothing in the install names it or `PS_BTBSnow`, so how RSF
  picks a BTB stage's particles is not known. Until then the skid table was looked up again from the stage folder
  (`skidmarks::for_stage`, removed), which failed on a BTB folder (`no Tracks<id>.ini`) and dropped every particle
  of a BTB stage; it is now the chosen file's.

## Drawing

`rbr_btb::scene` (`Scene::load`, no renderer types) turns a folder into meshes, materials and placements; the renderer's
`stage/btb.rs` builds a `Stage` from it (`stage::load` takes it when the stage's folder passes
`track::is_track_folder`, i.e. `<RBR_RSF>/RX_CONTENT/Tracks/<folder>/track-<n>_<L>`, or a prepared folder naming its source: [Playing](#playing); `load_textures` likewise).

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
- Textures: `Textures.zip` holds `.dds` (some uncompressed `X8R8G8B8`), and 1310 `.jpg`, 16 `.png` and one `.bmp`
  (Crastazza's ground, 24-bit) that materials name (`tarmac4.jpg`): `scene::TextureFiles::read` decodes them to a
  plain-texel `.dds`. Materials name textures by author
  paths; the file name matches the archive ignoring case (or the stem with `.dds`).
- Drawn: diffuse, the lerp (vertex alpha is the weight; the renderer's second texture also multiplies it by the second
  texture's alpha, which BTB's does not), `Up`, cull none, the vertex colour holding `(0.4 + N.L)` for the unenhanced
  draw; the renderer's own light, shadows and sky otherwise. `Transparent` by the diffuse texture's alpha
  (`scene::alpha`): translucent (part-transparent texels outnumber the clear ones: glass, flags, wire mesh, skid marks)
  as the two passes; cut out (leaves, grass) as one alpha-tested draw, kept over 127 (ours: blended, the fringe wrote
  no depth and the temporal anti-aliasing carried it by what lay behind, so plants ghosted; alpha tested, they take the
  renderer's foliage light); opaque as one solid draw.
- Sky domes: 43 of the 72 folders' skies are ordinary objects, a box 10 km across with a sky texture (Crastazza, Cueva
  Santa, Savo, Pirttikulma's `sky16`). A solo placement with faces 50 m over five driveline points is drawn in the sky layer;
  the renderer's dome test (`sky_dome`, here passing over the driveline: the terrain runs past the dome's edge) then
  leaves it out of the shadows, the sky's occlusion and, under `Enhance::lighting`, the frame. Drawn as the stage, it
  roofed it: the ground went black under `lighting`.
  Approximated: `MultiplyAdd` draws its diffuse (the multiplier/additive UV sets cannot share a draw's two texture slots),
  `Specular` has no highlight, `Bump` has no normal map (the renderer's stage draws have neither path). Not drawn:
  `BTB_Sky` meshes (7 in two tracks; the renderer's sky stays), the 139 meshes whose texture name has no
  `materials.ini` section (Cauterets' `vieuxpylone`, one La Serra), invisible objects. Pirttikulma's own sky
  (`Taivaita\Materials\sky16`, `BTB_Diffuse`) is an ordinary object, a sky dome (below). No wear, light maps,
  characters, fences or grass for a BTB stage (the stage draws its own objects; map 41's `.fnc` fences are drawn); the weather is the sky map's sections.
- LOD and size: no instancing. Each placement is expanded into world-space vertices (88-byte renderer vertices), grouped by
  128 m cell (a placement over 150 m half extent is a mesh of its own: the terrain tiles) and by `LODIn`/`LODOut` in 50 m
  steps (`Lod::Band`, the draw's distance from its mesh's bounds centre; a solo placement's `LODOut` counts from its
  radius too). `LODOut` is under 500 m for 64775 of 140785 objects across the install, so the band matters. Worst
  expanded tracks (Transparent drawn twice where the texture has alpha): Kaihuavaara 2022 3.38 M vertices, 2.60 M
  triangles in 7070 meshes (peak 0.85 GB in a viewer screenshot); Stoccareddo II 3.79 M vertices, 1.99 M triangles, 1270
  meshes (peak 1.3 GB); Pirttikulma 1.18 M vertices, 0.74 M triangles. Load to the first frame 5 to 9 s on those. A view
  draws 60 to 350 k triangles.
- Load: the sky's occlusion and the ground's bounce bake 1 km tiles; a BTB stage's terrain runs kilometres past the road
  (Cueva Santa 221 tiles, a retail stage about 10), so both bake only the tiles within 1 km of the driveline
  (`sky_occlusion::REACH`; Cueva Santa 31, Crastazza 54 from 121 with its dome out). On a live start at 4K with the
  textures upscaled the bakes took 10 to 22 s before (2026-11-03). The first load of a stage also upscales its textures
  once (`upscale`, cached).
- Pictures: `stage_picture::btb_picture`, RSF's `rsfdata/images/stage_images/<id>.png` for the stage's id, else
  `track.ini` `splashscreen`, else the folder's first `.jpg`/`.png` (every one of the 72 folders shows one); the
  launcher's preview, else `driveline.ini` from above; the viewer's loading screen.
- Open: no tangent-space bump or specular; `MultiplyAdd` terrain looks flat; the `CollisionModel=2` meshes are not drawn
  specially; the semantics of `LODOut` for large tiles (centre or box) is ours.

## The flat pad

- A generated BTB stage (`rbr_btb::pad`, `BUILD 1`): a level 2 km square of one material, written as an ordinary BTB
  track folder (`track/`) and prepared (`stage/`) under `<player>/pad/<name>-b<BUILD>/`, never listed by the launcher.
- Geometry: 100x100 squares of 20 m at height 0, UVs tiling every 10 m; start on the centreline 500 m in from one edge
  facing the far edge; driveline straight from 50 m behind the start to 1450 m ahead; pace notes start, splits at 400
  and 800 m, finish at 1300 m, end 50 m after.
- One pad per material: `pad-tarmac` 37, `pad-gravel` 17, `pad-snow` 101 (`SnowOnGravel_Shallow`, the tyre work's snow),
  `pad-<id>` any other id 1-255 (`pad-116` is NGP's ice, unnamed in 1.02's table); stage number `2_000_000 + id`; `mat.ini` one map, all cells the id, with
  `damp`/`wet` rows of id+1/id+2 for a `_Dry` id; `track.ini` physics `tarmac`/`snow`/`gravel` from the key prefix (gravel when unnamed).
- One `Visible=1`, `CollisionModel=0` ground object, so the mesh is both drawn (`BTB_Diffuse`, one generated grid
  texture) and in `col.col` (every face on `.mat` run 0); no `ksh` meshes, so the `(3,0x16)` chunk is empty.
- Preparation builds under a temporary sibling name (pid, call counter) and renames into place; an existing final
  directory is reused and never written in place, so eight parallel cold starts share one build.
- `rbr-standalone --stage pad-<name>` prepares the pad and runs as `--stage <prepared> --btb 41 --surface <pad's>`
  (explicit `--surface` wins); `viewer pad-<name>` prepares it and draws it as the BTB stage (the live command's
  `--stage pad-<name>` resolves in the physics program); `--manoeuvre` needs a pad stage.
