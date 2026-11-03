# BTB stages

BTB (Bob's Track Builder) stages as RallySimFans installs them (`RX_CONTENT/Tracks/<folder>/`) and its `rbr_rx.dll`
plugin plays them. Code: `crates/rbr-btb`. Facts with their evidence, one line each.

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

