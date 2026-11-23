# DiRT Rally conversion

DiRT Rally (2015, Codemasters' EGO engine) stages read from the player's own install and written as BTB stages, in
`crates/rbr-dirt-rally` (its readers stay in that crate). The sample install is the Feral Linux port (Steam,
`steamapps/common/DiRT Rally/share/data`): loose files, gzip and plain formats, nothing encrypted but the game's own
`game.bin` and car physics. Whether the Windows release lays its files out the same is not checked. Findings from the
investigation of 2026-11-23 (`/tmp/dr-investigation/*.md` at the time); each still to be confirmed by the readers' tests.

## Install and routes

- `tracks/locations/<location>/<area>/route_<n>`: 12 rally areas (two per location: Finland, France (Col de Turini),
  Germany, Greece, Sweden, Wales), each route_0..5; Pikes Peak (routes 0, 2, 4, 6), three rallycross circuits,
  `frontend` (no course). 94 route folders.
- Route roles, all 12 areas (start and finish positions, codriver `_fwd`/`_rvs` names): 0 long forward, 1 long reverse,
  2/3 first half forward/reverse, 4/5 second half forward/reverse. Stage names are not on disk (`game.bin` is opaque).
- An area is one world its routes share: `routesplit.pssg` and `terrain_lm_day.pssg` are byte identical across
  ghymno_rally_01's six routes; objects (`objects.ens`, `route_objects.pssg`), trees, collision and course differ.

## Formats

- PSSG (PhyreEngine): gzip around a big-endian node tree; schema of node and attribute names, then nodes
  `id, size, attribute size`, attributes, children or a payload. `route_objecttypes.pssg` and `objects.ens` are plain XML.
- Codemasters binary XML (`1a 22 52 72`, "Rr): string blob, offsets, element records `[name, text, attribute count,
  first, child count, first]`, attribute pairs; numbers are text. `progress_track.xml`, `ai_track.xml`, codriver files.
- JPAK (`track.jpk`): little-endian table of uncompressed entries, one `.vcqtc` collision quadtree cell each (1883 on
  ghymno_rally_01 route_0), named by its quadtree address. A triangle crossing cells is in each, quantised over each
  box: 815,180 stored, 710,212 distinct to 1 cm (Greece route_0); up to 21 copies on Col de Turini. Nine or more
  copies made the BTB collision split recurse without end, so `collision::read` keeps one.

## Frame

- Right handed, y up, metres, world space for terrain: gate crossings lie on the drawn terrain to 0.24 m (Greece, 838
  gates) and 0.31 m (Finland, 1014); a gate's `left` is up × forward. Converted as `(x, -z, y)` (`frame.rs`).

## Drawn world

- Greece: near terrain and road in `route_N/routesplit.pssg` (about 2.0 M drawn triangles), vista and all terrain
  textures in the area's `tracksplit.pssg`. Finland has no routesplit: all terrain (about 4.5 M) is in tracksplit.
- `*BATCH*` nodes are position-only copies of the drawn tiers (same triangle counts): skipped.
- Textures are DXT1 and DXT5. `objectstextures.pssg`, `route_objectstextures.pssg` and tracksplit's lightmaps are 4x4
  placeholders: the real ones are in `patchup_ot.pssg`, `route_patchup_ot.pssg` and `route_N/terrain_lm_<time>.pssg`.
- Props (`objects.rs`): `objects.ens` instances (row-major 4x4, translation last) name a `TEMPLATEENTITYREFERENCE`
  (`route_objecttypes.pssg#x.max`); each `TEMPLATESUBENTITY`'s renderable names a node of `objects.pssg`,
  `route_objects.pssg` or `trees.pssg`. Only the `default` renderable draws (the `damage` ones are the broken pieces).
  A `ROOTNODE` holds the same geometry three times: under `lod` nodes (the visible level 0, with the trees' billboard
  and shadow quads, shaders `*billboard*`, left out), under `default` and `damage` (physics): `mesh::types` takes the `lod`
  ones and indexes every node id under the root. Every instance of Greece and Finland route_0 resolves.
- Trees (`trees.rs`): `trees.bin` layout in the module's header (types with local box and name, items of 80 bytes: type,
  3x3 with scale, position). Placed on the collision surface within 9 cm median (97 % within 1.5 m on Greece, the rest
  slope). The `*_dist_*` types are 500 m tiles of far trees (~100 triangles each, centred in the air). `objects.ens`
  also lists the physical trees: every tree-model instance of Greece route_0 (11252) and all but 3 of Finland's (14166)
  lies within 1.5 m of a `trees.bin` item, so they are drawn once, by `trees.bin`. Finland's header counts 3 more items
  than its table holds; the table's own count is read.
- Drawn props (`import.rs`): placed with the instance matrix (a mirrored one keeps the winding), materials split by draw
  range (100 m to 1 km by size, less for dense models, `*_dist_*` tiles 3 km). Alpha only for `foliage*`/`facing_foliage`/
  `treesheet*` (not trunk) and `*Blend*` shaders on DXT5: `Object_Emis`, `Object_Norm` and `Object_Stones` have DXT5
  with varying alpha as a mask, not transparency. Greece route_0: 16592 props + 13394 trees = 7.4 M triangles
  (terrain 1.9 M); Finland route_0: 16387 + 22228 = 28.9 M (terrain 4.3 M), almost all within 40 m of the road, so the
  range does not thin them. Not done: the far billboard levels, grass (`grass.grs`), cloth, `ornaments.bin`; foliage
  is drawn one-sided as the BTB mesh is.

## Course

- `progress_track.xml`: 12 m gates every 12 m (`distance`, `left`, `right`, `crossing`), splits naming gates (start,
  times, finish). Stage length is finish minus start (ghymno_rally_01 route_0: 9666.5 m); the gates run on past both.
- `ai_track.xml`: road sections, `position` on the left edge, `normal` across, `right_track_limit` the width (inferred
  from the gates; to be confirmed on the mesh).
- The start grid (`grids.pssg`) lies on the start gate's crossing within 0.54 m on 79 of 84 rally routes; its matrix
  is rotated on eight, so the heading comes from the gates.
- Codriver `_data.xml`: calls at `distanceAlongTrack` from the start line with subcalls (type 1 left, 2 right
  probable, `angle` the corner's turn in degrees); other types undecoded.

## Surfaces

- Collision cells name up to eight four-character codes (`DRT+`, `GVO+`, `SWD+`; `+` driven); `audio/materials.xml`'s
  closing comment lists them, `wet_materials_remap.xml` their wet codes. No grip value is readable, so codes map by
  kind (`surface.rs`): rock (`ROK`) is coarse gravel 49, never tarmac on a gravel stage; `DEFA` takes the stage's road.
- Snow, water and rough ids are depths, not weather: their `mat.ini` damp and wet rows repeat the dry id.
- `audio/materials.xml` gives each code a sound and a physics class: `CBW+` (Hafren's ford) is water over mud, 157.

## Conversion sweep (2026-11-23)

- All 72 rally routes convert (before props and shading, 9 to 34 s each). Physics by the codes under the course:
  Finland, Greece, UK gravel; Germany and Col de Turini 01 routes 0 to 3 and 02 tarmac; Sweden and Turini 01 routes
  4 and 5 snow. Panzerplatte 01 route_0 passes a cell's vertex limit (65538) at a 75 m collision corridor: the import
  retries once at half.
- Driven headless (btb-autopilot): Ghymno 1 long ran 5.75 of 9.75 km through both splits, off at a gravel bank (GBK).

## Cross-checks

- Look: the Steam store page's official screenshots (app 310560, kept out of Git) show Finland in autumn (yellow
  birches, pines), Greece warm and dry (golden grass, olive scrub, hazy blue mountains), Sweden with snow-laden
  spruce over brown-grey icy gravel, Wales dark wet conifer plantations.

- Five AC ports (GabenHood's `onin`, `dr_greece` ladders and snakes, `wales_dr`, `dr_sweed`) are translations of
  `*_rally_01` route_0 (no rotation or scale, heights equal, about 1 m along the road). They are someone's rips of
  unknown method: a hint where ours differ, not a reference.

## Terrain shading (`shade.rs`), 2026-11-23

- The shaders (`terrain_road`, `_rockbank`, `_rock_d4`, `_edge_nm`, `_infield_nm`) sum up to four diffuse layers (vertex `Color`
  R, G, B the second to fourth weights, the first the rest; first-layer detail in `Map<N>UVScaleAndOffset`, 1 on every material read)
  times `TColourMap` and `TAmbientOcclusion`. Both maps lie on the second `ST` stream, a unique unwrap of a section's ground
  (15 sections and two specials on Greece, one atlas per lightmap name, shared by its materials); the layers lie on the first's `xy`
  (metres per unit 4 to 8 on roads and banks, tiling), `zw` unidentified. Compiled DXBC only (`shaderpack.pssg`): the blend is read
  from the data, not the code.
- Drawn by ReRBR's `ReRbrLayer*` (no renderer change): per section a diffuse (colour map over its covered texels' mean, hue kept at half,
  times the lightmap over its bright texels' mean, half strength: its tree-shadow dots show as streaks without the trees) and a 2048
  mask of the vertex weights rasterised in the atlas, both transposed; uv0 is that atlas; each layer's own texture is a detail at
  `uv0 * scale`, per island of the mesh (triangles joined by a vertex) at the repeat that gives the layer its metres (atlas rate per metre
  over the layer's), classed within 20% as a material of its own. Roads' ruts run along the atlas's u, hence the transpose.
  Islands whose layer runs at no one angle to the atlas, or whose atlas shears over 1.6, read the details at the world's (x, z) instead.
  Triangles whose atlas jump is 8 times the mesh's median rate (they join islands) keep the old single-texture draw.
- `TBlendMap2/3` (a height map the game roughens a weight's border with) is not used: finer than the mask. Colour-map hue and
  lightmap strength are chosen by eye (no game screenshot to compare): Greece's section means run pink to blue.
- Greece route_0: import 8.6 to 25 s, `Textures.zip` 99 to 291 MB; Finland route_0: 21 to 53 s, 109 to 349 MB. Seen at 0.1, 1.5 and 4 km: roads
  carry their ruts and Finland's grass verges, crisp detail at any distance; Greece's far hills (the mountain atlas, flattened in big
  islands) show streaks and blotches of the second layer.
