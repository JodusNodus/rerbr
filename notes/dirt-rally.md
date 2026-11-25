# DiRT Rally conversion

DiRT Rally (2015, Codemasters' EGO engine) stages read from the player's own install and written as BTB stages, in
`crates/rbr-dirt-rally` (its readers stay in that crate). The sample install is the Feral Linux port (Steam,
`steamapps/common/DiRT Rally/share/data`): loose files, gzip and plain formats, nothing encrypted but the game's own
`game.bin` and car physics. Whether the Windows release lays its files out the same is not checked. Findings from the
investigation of 2026-11-23 (`/tmp/dr-investigation/*.md` at the time); each still to be confirmed by the readers' tests.

## Install and routes

- `tracks/locations/<location>/<area>/route_<n>`: 12 rally areas (two per location: Finland, France (Col de Turini),
  Germany, Greece, Sweden, Wales), each route_0..5; Pikes Peak (routes 0, 2, 4, 6), three rallycross venues (Höljes and Lydden Hill routes 0 to 2, Hell 0 and 1; see Rallycross below),
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
- Drawn props (`import.rs`): placed with the instance matrix, drawn instanced (below). Materials split by draw range
  (100 m to 1 km by size, less for dense models, `*_dist_*` tiles 3 km). Alpha only for `foliage*`/`facing_foliage`/
  `treesheet*` (not trunk) and `*Blend*` shaders on DXT5: `Object_Emis`, `Object_Norm` and `Object_Stones` have DXT5
  with varying alpha as a mask, not transparency. Trees hold no intermediate level: a root has `_x0` (2 to 3.9 k
  triangles for the big conifers, bare trees and birches), `_x2` (the far billboard, `foliage_billboard.fx`, 2
  triangles: its vertices all sit at one centre and `uv2` is each corner's offset, right and up, in metres; the atlas
  holds 8 views of the tree, one per 512 px column) and `_xs` (shadow quad). Only `objects.pssg` and
  `route_objects.pssg` have `LODRENDERINSTANCES`, and their props are small.
- Instanced (`import::Placer::finish`, `stage.rs`): a prop or tree type is written once per material, range and
  mirroring in its own space (`dr_<n>.x`), its placements in one `dr_x<n>.xform` per type and range, the objects
  `ReRbrInstanced=1`, `CollisionModel=-1`, `Visible=1`, `LODIn=0`, `LODOut=range`. The matrix is DiRT Rally's `M` as
  `F M F` with `F = diag(1, 1, -1)` (`import::xform`; the mesh's axes are `(x, y, -z)` of DiRT Rally's), translation
  `F t` less the stage origin; a mirrored placement (negative determinant) is its own model wound the other way (the
  renderer does not flip the winding per instance). A test places a model and compares it with `bake` (1 mm, a turned,
  scaled, mirrored matrix). A range's single placement stays baked, as do the `*_dist_*` tiles. The range is
  `min(size * 100, 60000 / sqrt(triangles))` rounded up to `PROP_RANGES`; on Finland every tree lies within 340 m of the
  road, so raising the cost term to 150000 changes nothing (the same 67003 placements), and a frame costs the same as
  with the billboards (20 ms read back at 1280x720, everything on, with 32 M instanced triangles in the stage).
  The far level is the billboard, baked a pair per tree within 1 km of the road (4 triangles, so no instancing: it
  faces the road per placement; it needs `LODIn` = the model's range, `LODOut` 3 km: `Input::instanced.lod_in`).
  Leaf cards (`is_cards`) cannot turn to the road per placement in a shared model: each stored card is two cards
  crossed at 90 degrees about the type's vertical, each drawn from both sides (`import::model`); the shots show birch and
  olive crowns as full as the road-facing ones (`/tmp/dr-inst-look/*-after`). Baked singles keep the road-facing cards
  (`import::cards`, `facing`). Each type's `_x2` samples its own atlas (`<tree>_lod_d.tga`) and the near model its
  own `*_fo_d`/`*_tr_d`: both are autumn on Finland. The near leaves had been missing because `facing_foliage.fx` cards
  keep all four vertices at the card's centre with the corner offsets (right, up, metres) in the second `ST` stream
  (the billboards too) and the game's shader turns them to the camera: drawn as stored they were zero-area. The
  `*_facing` types (`treesheet_foliage_norm`, a whole tree or bush on one card) are the same trick: any mesh whose
  triangles all collapse to a point with offsets in the second stream is cards (`import::is_cards`).
  Finland route_0, before (baked models to 8 m, billboards past) and after: written 9.31 M triangles -> 4.70 M (terrain
  4.3 M; 348 models, 67003 placements, 32 M triangles if all were in view); stage folder 760 -> 644 MB; import 91 -> 72 s;
  viewer load 12 to 16 -> 8 to 11.5 s, peak RSS 2.9 to 3.1 -> 1.9 to 2.1 GB. Greece 4.86 M -> 2.40 M (14.6 M instanced),
  499 -> 440 MB, 33 -> 31 s, load 5.0 -> 3.1 s, 2.0 -> 1.3 GB. Germany panzerplatte 9.01 M -> 3.31 M (25.5 M instanced),
  662 -> 529 MB, 110 -> 88 s, load 7.0 -> 4.7 s, 3.7 -> 1.6 GB. Grass is done (below); not done: cloth, `ornaments.bin`;
  foliage is drawn one-sided as the BTB mesh is.

## Collision of props and trees, 2026-11-24

- Source: `route_objecttypes.pssg` gives each entity its physics hulls (`TEMPLATERIGIDBODYSHAPE` -> `TEMPLATESHAPEBOX/CYLINDER/CAPSULE/SPHERE/CONE`, its transform, a material `props="physparams.xml#sSolidTreeWood"`, and for some a `TEMPLATEBEHAVIOUR` with an impulse class). A `TEMPLATESUBENTITY` with `infMass="1"` drawn by a non-`damage` renderable is the intact prop; `infMass="0"` and `damage` ones are the pieces that fly off (`objects::shapes`).
- Solid (`objects::Shape::solid`): a hull with no impulse class (trees, `sStone` rocks and walls, houses, stumps, logs, fences, `core_barr_fence`, scaffolding, parked vehicles marked `sMetalSolid`) or `bArmco` (rails), unless its material is `*Vegetation` (bushes). Not solid: any hull with `bPhantom` (cones, tape poles), `bLightest`/`bLight`/`bMedium`/`bHeavy`/`bDefault` (signs, small fences, telegraph poles, movable rocks `sSpawnable*`, cars `core_estate`), and vegetation. Telegraph poles and `fin_sign_*` are `bHeavy`: they break in DR, so they stay non-solid here (one `solid()` line to change if wanted).
- Written (`solid::triangles`, `import::solid_props`): the hull at `shape matrix * instance matrix` as plain collision faces in the same tiles as the ground (no new objects, no BTB change): box 12 triangles, round hulls a prism (6 sides under 0.4 m radius, else 8; apex for cones; no foot cap on an upright one). Hulls whose top is under 0.3 m above the prop's origin (half-buried scrub rocks, 7200 of 8200 within 15 m on Greece) are left out. Trees come from `objects.ens` (all physical trees are listed there), not `trees.bin`. The 75 m collision corridor clips them as the ground.
- Collision triangles (before -> after): Greece 679,601 -> 744,165; Finland 1,557,326 -> 1,714,242; Hafren 1,253,583 -> 1,369,544; Col de Turini 887,446 -> 971,150. Largest cell 53,964 -> 47,016 (Greece), 59,331 -> 63,960 (Finland), 60,594 -> 64,293 (Hafren), 60,744 -> 63,645 (Turini) of 65,536: Finland, Hafren and Turini sit near the limit (the existing half-corridor retry covers a breach).
- Ray check (horizontal 2 m segment through the hull at 0.5 m above ground, within 15 m of the road, built col.col): Greece trunks 673/674 hit (268 hit before: slopes), rails 57/58 (1 before); Finland trunks 3825/3835 (60 before), rails 26/26 (0 before). Hull centres lie within 1 m of their drawn prop's bounds on all checked hulls (`solid_hulls_stand_on_their_drawn_props`).
- Autopilot (btb-autopilot, car 2) on Ghymno route_0: before and after both reach 5769 m of 9750 through both splits and leave the road at the same spot (gravel bank); no tree or rail hit before it.
- Not done: `barrierlines.cqtc` / `boundarylines.cqtc` (empty on Greece and Finland; non-empty on Turini, Panzerplatte, rallycross), cone and capsule axes unverified (few solid ones), vegetation stays walk-through.

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
- Wet: routes carry weather variants of their baked light (`terrain_lm_{day,day_alt,night,wet}`, `buildings_lm_*`,
  `sky_*`, `shadow_map_*`, `ambient_sh_*`; areas `lighting_*.xml`, `ambientmaps_*`, `envmap_*`, `waterdefs_*`); Greece
  has no wet set. Road materials' `TWetMask` is `default_wetmap.tga` or, on Hafren, a 512² tiling noise of blobs (wet
  patches), not a placement map. ReRBR's own wet road (film, puddles, rain) already runs on the converted stages.
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
- Drawn by ReRBR's `ReRbrLayer*` with `ReRbrLayerUv=1` (2026-11-24, `shade.rs`, `notes/btb.md`): per section one BC7 diffuse (colour map over its covered texels' mean, 30% of its hue
  kept, times the lightmap over its bright texels' mean, 80% strength; DXT1 turned the smooth, nearly grey diffuse into magenta and green blotches on Greece's roads) at the atlas (UV set 0, the second `ST`
  stream, untransposed); each layer's texture is a detail at the first stream's `xy` (UV set 1, `Map<N>UVScaleAndOffset` of the first layer applied, the others relative to it); the weights are the
  vertex colour per pixel (R, G, B the second to fourth layers; sets 2 and 3), summed as the chain `mix(mix(mix(L1, L2, R), L3, G), L4, B)`, layers a material lacks weighing 0. The renderer's
  near-ground detail and steep-bank replacement are off on such draws. This replaced a mask (the weights rasterised in the atlas) with the details at `atlas * repeat` per island of the mesh,
  repeat classes, a world-space fallback, `collapsed` triangles, and `Layers::bake` (the steep-face first texture): all gone, a material is one draw. Still kept: `bridges` (triangles joining distant atlas
  islands keep the unshaded first-layer draw). A tile's layer uv is shifted by whole units that keep every layer's tiling (`shade::period`), `f32` at kilometres. The normal map is the dominant layer's at the layer
  uv (`ReRbrBumpUv`, `ReRbrBumpScale` the layer's relative repeat).
  Measured (release, 8 cores, shared machine; `Textures.zip` MB before -> after, import s where measured alone): Turini 506 -> 422, Sweden 542 -> 381 (58 -> 27 s), Greece 554 -> 382, Finland 757 -> 565, Wales 660 -> 557,
  Germany 590 -> 452. Turini 6 km scene pass 2.84 -> 1.91 ms, 2099 -> 1267 draws (1280x720, `--enhance all,grass`); first frame 5.9 -> 5.0 s. Turini's wall at 6 and 8 km is continuous (no blocks, seams or flat sheets), the 1 km barcode bands
  stay gone, Sweden's banks have no facets, Finland's grass grows. All 37 reference shots (`scripts/render-shots.py`, AC BTB stages among them) RMSE 0 against the build before. Sheets: `/tmp/luv/sheet-*.png`.
  Hafren's roads (G the skid layer, 0.1 to 0.5 over the road) went near white with a linear sum `1 - R - G - B, R, G, B` (it passes 1 where R + G does); the chain keeps the sum at 1 and they read as before.
  The white dashes on Hafren's road at 4 km (a grey paved stretch by road signs) in the earlier mask-bake render are in no layer texture, the atlas or any weight order (layer 1 alone shows none). `TBlendMap2/3` are greyscale height maps of their layer (pebbles, ruts, noise; Hafren 4 km: `set1_03_b`, `skd_01_b`), no markings; `BlendMaskUVScale` is 0 on every material read. Likely the old mask's rectangular cells over the set4 details' bright flecks (untested).
  Road blend (2026-11-24, `ReRbrLayerBlend`, `shade.rs`, `notes/btb.md`): `terrain_road` materials with `DetailBlendScale` and zero `Map2/3UVScaleAndOffset` offsets draw the DXBC's weights: `a = sat(1-R)`,
  `t = sat((h2-a)/(sat(a+S)-a))`, `R' = t²(3-2t)`, `G' = h3*G`, then the chain. h2 and h3 (`TBlendMap2/3`, red, resampled wrapped to the larger size, at the layer's own uv: same scale as the layer) ride in the alpha of
  layer 2's and layer 3's textures, re-encoded BC7 per (layer, map) pair (the layers' own alpha, the specular, is unread by the renderer). Every road material has both maps (`default_black` is real: the skid weight is 0 there);
  other shaders (`wsm_edge_nm`, `edge_nm`) and roads with a map offset keep the plain weights (formula unread). Greece 3.5 km: the black tyre stripes in the bends are gone (rmse 11 against before, 22% of pixels over 8/255).
  Finland, Hafren and Germany change by ruts and pebble-scale borders only (rmse 2 to 8); the roads still read grey and smooth next to DR's light dusty ones, which this blend does not explain (layer textures and the colour-map and
  lightmap diffuse are the same before and after).
  The other shaders take the chain mix of the weights, which is what the data gives there. Vertex colours are used as bytes over 255 (no gamma): snow, rock and road transitions read right. Greece's verges differ from the mask's (a grass layer on banks, less at some road edges): the weights are per pixel
  now; against the game itself unverified.
- (History, superseded by the above) the mask path:
- Drawn by ReRBR's `ReRbrLayer*` (no renderer change): per section a diffuse (colour map over its covered texels' mean, 30% of its hue kept,
  times the lightmap over its bright texels' mean, 80% strength), written as BC7 (DXT1 turned the smooth, nearly grey diffuse into
  the magenta and green blotches on Greece's roads: 5:6:5 endpoints per 4x4 block), and a 2048
  mask of the vertex weights rasterised in the atlas, both transposed; uv0 is that atlas; each layer's own texture is a detail at
  `uv0 * scale`, per island of the mesh (triangles joined by a vertex) at the repeat that gives the layer its metres (atlas rate per metre
  over the layer's), classed within 20% as a material of its own. Roads' ruts run along the atlas's u, hence the transpose.
  Islands whose layer runs at no one angle to the atlas, or whose atlas shears over 1.6, read the details at the world's (x, z) instead.
  Triangles whose atlas jump is 8 times the mesh's median rate (they join islands) keep the old single-texture draw.
- `TBlendMap2/3` (a height map the game roughens a weight's border with): used for roads since `ReRbrLayerBlend` (above); not by the mask path. Colour-map hue and
  lightmap strength are chosen by eye against the Steam store shots: Greece's section means run pink to blue, `ColourMapScale` is 2 on
  every material (so the game's neutral is a mid blue-grey, not grey). The renderer multiplies the textures as display values, so no
  gamma applies to our diffuse.
- Banks (Sweden's snow, Turini's rock, 2026-11-24): the torn triangle patches and colour stripes were not in the colour map or the lightmap (sampled at the second
  `ST` stream they are smooth: a render of colour times lightmap at the stream over the same view has none; the first stream's `zw` is the second layer's
  coordinates, 0 to 96 over a road mesh, not a map's; no material constant maps world or `zw` into the maps; vertex-shared uv2 is continuous, 0.3% of
  positions hold two uv2). They are the renderer's terrain detail: on faces steeper than about 30 degrees (`terrain_steep`, 0.6 to 0.85 of the face's up
  share) it replaces the layered colour by the draw's first texture (`terrain_even`, `terrain_bank`), which was the first layer alone (no macro colour,
  no lightmap), so each triangle's flatness switched it between the two looks (facets; with `--no-relight` and without `--enhance` the same). Proof on
  Sweden: setting the bank materials' magic to 0.3 blackened exactly the patch triangles and left the bank's body unchanged; flat diffuse or flat colour
  map showed it was the first texture on the body and the layered one on the patches. Fixed in `shade.rs`/`stage.rs`: the first texture of a material
  whose triangles are at least 3% steep (face up share under 0.85) is `Layers::bake`, the atlas's diffuse times the layers' mean colours by the mask
  (BC7, section-wide channel means); flat ground keeps its first layer (the grass reads it: a bake on Finland's verges lost the grass). Sweden `Textures.zip`
  439 -> 472 MB, import 39 s before and after; Turini's green, pink and yellow stripes are gone (grey rock), Greece and Finland unchanged apart from richer rock.
- Turini's rock wall at 6 km (`rck6-leaf` `terrain_rockbank.fx`, `rck6-snow`/`leaf-snow` `terrain_road.fx`; 2026-11-24), the blocks with seams and pulled streaks: not the bake, the world
  projection or the repeat classes alone (bake off, steep islands never world: same picture; classes at 0.1 instead of 0.4: fewer seams, same sheets). The wall is the
  layered draw (diffuse x mask x details) with the details at the per-island atlas repeat; the same wall drawn with its first layer at the mesh's own uv
  (`DBG`-skipped shading) is continuous, so the blocks are the islands' separate repeats and the flat grey-white sheets with straight edges are the snow layer's vertex weights
  (inf_05 is a flat white texture) without the game's height roughening (`TBlendMap2/3`, `DetailBlendPower` 4, `DetailBlendScale` 0.6, compiled shader: the formula is unread).
  The game's own layer uv is stretched on the overhangs too (the raw-uv draw shows the same smear). Steep rock islands: atlas to layer scale off by over 1.6 on 7% of the area
  (per-triangle repeats: 2%, but every triangle a seam: worse, tried). Found and fixed on the way: `Map<N>UVScaleAndOffset` of the second and later layers was ignored
  (mesh uv carries layer 1's only): Map2 is 2 on 109 Turini materials (snow, leaf), `[1, 2]` on 44; the layers' scales are now repeat times the layer's over the first's.
- Germany's road at 0.3 km (2026-11-24) is light concrete, not a converter defect: sections 5 to 8 and 14 are `Concblock` (`terrain_road`, layers `ger_pan_ral_set4_01_d` twice and a skid), and
  `set4_01_d` is the brightest road texture of the area (mean 145/139/130; `Tarmac_main` `set1_01_d` 126/124/122, `Cobbles` `set2_01_d` 136/132/123). In the shots the sunlit tarmac at 2 km reads
  about 150 (texture 126, renderer gain 1.2) and the concrete at 0.3 km about 171 (145 x 1.2): the same gain. Nothing wrong in the colour map, lightmap or mask (diffuse means 128 on every section).
- Greece route_0: import 8.6 to 25 s, `Textures.zip` 99 to 291 MB, with the BC7 diffuse about 45 s and 385 MB (plain RGBA: 458 MB); Finland route_0: 21 to 53 s, 109 to 349 MB. Seen at 0.1, 1.5 and 4 km: roads
  carry their ruts and Finland's grass verges, crisp detail at any distance; Greece's far hills (the mountain atlas, flattened in big
  islands) show streaks and blotches of the second layer.
- Vista (`vista.rs`), 2026-11-24: the far hills and valley (`NONLOD`, `terrain_track_vista_d4.fx`) were drawn with `TlargeColourMap` alone, a
  grey-blue macro tint (means 112/110/99), hence the pale blue-grey valley. The shader blends the earth, bush and rock layers (`TDiffuseSpecMap1..3`,
  means 145/124/97, 98/95/69, 114/109/90) by `TlargeBlendMap` (1024, same uv as the colour map; red the first layer, blue the second, green the
  third: the first mapping tried, blue first, gave olive green, this one the tan of the reference, chosen by eye) times the colour map times 2.
  Now baked per vista material at the colour map's size (layer means, BC7): Greece's valley is tan with olive scrub as in `greece-cockpit`. Import 31 -> 32 s,
  `Textures.zip` 403 -> 426 MB.
- Rock walls, Greece: not picked by slope; the game assigns materials per mesh (`terrain_rockbank` `fea_0N` layers, `terrain_rock_d4` `rck_a_d`/`rck_b_d` on
  `rck_c_d`'s atlas). Their layer uv is already metric (steep triangles, median 4.4 to 8 m per unit, 0.5 to 2 cm per texel), and `rck_a_d` is near
  neutral (102/97/93), `fea_06_d` 124/112/100. The blue-grey is the renderer's lighting: the same stage shot with `--no-relight` is warm and crisp (walls and valley),
  `--fog off` and `--sun midday` change little. Dropping the colour-map hue (`TINT` 0) or the lightmap's shadow hue did not change the walls. Not a converter defect.

- Turini's barcode bands on the road at 1 km (2026-11-24): `monte_a` (`terrain_road`) quads whose second-`ST` corners lie on one line of the lightmap (u constant, 192 triangles in
  `monte_a!3`, atlas area zero; found by skipping materials in the draw until the bands went, then dumping the triangles' `uv`/`uv2`). The details lie at `atlas * repeat`, so a degenerate atlas
  triangle drew one line of each detail across the road (dense stripes along it). `shade::collapsed` (atlas area per square metre under 2% of the mesh's median) keeps them in the shaded draw
  with the details read at the world and the layer's own repeat (`layer_world`): the atlas's maps along that line are smooth, as in the game. Drawing them unshaded instead (first layer alone) left
  light strips. Sweden 0.3 km differs slightly in its ruts (mean 3/255), Finland and Greece unchanged.
- Turini's near-black rock face at 3 km is not a converter defect: the section's diffuse at those triangles is 150/255 (x2 magic, layers `rck_06_d` mean 107, bright and detailed texture), normals
  agree with the faces, and with the bake off it is as dark. The same stage shot at 09:00 or 17:00 lights the wall fully (`--clock`): at 13:00 the wall faces away from the renderer's sun (azimuth not the
  game's, see "Sun and place"), so it is in shadow with little ambient. Fix belongs with the sun direction from `lighting_day.xml` (open) or the renderer's ambient on steep faces.

## Decals and birch trunks (Finland route_0), 2026-11-24

- The pale strips on the gravel are the terrain's `DECAL` tier: 73 materials, all `decal_ao_vc.fx` (`TDiffuseAlphaMap` `fin_ral_decal_01..04_d` DXT5, AO map, no
  `ST` second stream), `set*_mud` (tyre strips, ruts) and `set*_splats` (gravel patches). The shader's alpha is the texture's times the vertex `Color` alpha:
  mud's runs 0 to 255, mean 0.33, splats' mean 0.95 (histograms over all decal vertices). Drawn with the texture's alpha alone the strips were 3 times too
  opaque and flat grey. `import::decal_levels` now splits each decal mesh by its triangles' mean vertex alpha (4 levels, clear ones dropped), each level its
  own copy of the texture with the alpha scaled (`faded`). Shots `/tmp/dr-decals/after/shots/000.300.png`: the strips are faint tyre lines. The AO map and vertex
  RGB (mean 0.9 to 0.95, grey) are not applied.
- Birch trunks (`large_tree_tr`, `foliage_trunk.fx`, `fin_birch_full_01_tr_d`, 512x1024 DXT1, opaque, real, not a placeholder): the converter draws the right
  texture. It is pale grey (mean 166, 161, 153) with dark lichen and bark lines, vertex `Color` a grey AO (mean 0.9). So the bright white of the game's shots
  is its exposure and lighting (a sunlit trunk of 0.63 albedo), not the converter's pick: the trunks in shade read grey and dark. Nothing in the material (one
  texture, no tint constant: only `ScaleX_solo`, `ScaleY_solo`) to correct; left as is.

## Sponsor boards, 2026-11-24

- Boards, cloth and flags (`board.tga`, `cloth.tga`, `teardrop.tga`) name `sponsor_pack_<a-e>.pssg|sponsor_fallback.pssg#<name>` (Höljes has `e`). The packs are filled by the game at run time and are not on disk;
  `tracks/sponsors/sponsor_fallback.pssg` is a debug texture (flat green with "FALLBACK"), and the library only looked in the route and area folders, so the grey rectangle on Col de Turini
  route_0 (km 0.3, `route_objects.pssg` `Material #1`, `Object_Dynamic.fx`) had no texture at all. `texture::Library` now also reads `tracks/sponsors` and takes a pack's letter to one of the
  installed `sponsor_*.pssg` (91 files, each with real `board`, `cloth`, `flag`, `teardrop`, `arch`, `drape`), a quarter of the sorted list apart and wrapping: a real board (alpinestars) instead of grey or the fallback.
  The game's own pick per pack is unknown; the tint files (`sponsor_*_tint.xml`) are not applied. Other stages' shots at 0.3, 2, 4 km are unchanged (mean difference 0).

## Sun and place, 2026-11-24

- Light: each area's `lighting_day.xml` (binary XML, `param` elements) carries `rotationangle` and `heightangle` (Greece 01 30/45, 02 61/44, `day_alt` -91/15; Finland -90/45; Sweden -90/40; Wales 01 -161/48,
  02 43/48; Germany -134/35; Turini -105/31-34), the sun colour (`specular_colour`, Greece 1.0/0.93/0.84) and fog. The converter still writes AC's constant `sunDirection` (30 degrees up): the
  azimuth convention could not be verified. Tried, on Greece 01/02, Finland 01, Wales 02, Germany 01 and Turini: the day lightmap (`terrain_lm_day`, texel at each terrain triangle's `uv2`) against
  `max(0, n.s)` and a heightfield shadow march over the azimuth scan, then day-night, day_alt-night and day-day_alt per texel against the xml suns over rotation sign and offset: r 0.05-0.3, broad
  humps, the peaks disagree between day, day_alt and areas (Greece 02 340 against xml 61; day-alt 270 or 60 by sign). The lightmaps are ambient occlusion, not the sun. `sky_day.pssg` holds no sun disc.
  Open: decode `shadow_map_day.clm`, or a DR shot with known camera and sun.
- Place: `track.ini` carries `latitude` and `longitude` where the renderer's `countrycode` default is far off (Turini, Baumholder, Pikes Peak, Hell, Holjes, Lydden Hill: `route::place`).

## Hafren and Pikes Peak survey, 2026-11-24

- Pikes Peak route_0 imports (`import` allows area `pikes_peak`; `is_rally` stays for naming): 19.5 km, tarmac, 7.7 M drawn triangles; Hafren route_0 3.1 M. Shots at 0.3, 1, 2, 3, 4, 5, 6, 8 km, everything on.
  All 5200 (Hafren) and 15295 (Pikes) material texture references resolve to real textures (no placeholders, no missing); the black road signs at Pikes 4 to 8 km are the signs'
  backs (`Object.fx`, real `pke_rd_sign_*` textures), not a missing texture.
- Hafren's light towers (`core_event_lights_a_night`, 3 solids: `Object_Emis`, a shader with no diffuse `volumetrics.fx` of 448 triangles): the volumetrics are the light cones, additive in the game and
  untextured, so they drew as a dark blob wider than the road over the trailer (6 and 8 km). `mesh::types` now leaves `volumetrics*` out with the billboards.
- `Object_Cov` and `Object_2SidedCov` are cut-outs, not opaque (DXT5 alpha is the wire mesh of `wal_wirefnc_mid_a_1_d` and `pke_fence_02_d`, the branches of `wal_gateway_c_2_d`, the cable): `blended` took only
  `*blend*` shaders, so fences drew as solid slabs. `blended` now includes `cov.fx`.
- Shade read blue and violet (Wales, Pikes Peak, Turini): the bakes carry DiRT Rally's own ambient and the renderer's clear-sky ambient came on top. `settings.ini` writes `sky_tint=0.2` (`[ReRbrAir]`, `notes/renderer.md`): 0 left Wales' roads warm brown, 0.3 and 0.5 left Pikes' asphalt blue, 0.2 reads near neutral on all five routes.

## Rallycross circuits, 2026-11-24

- Venues `sweden/holjes_rallycross` (routes 0 to 2), `norway/hell_rallycross` (0, 1), `uk/lydden_hill_rallycross` (0 to 2) import as ReRBR circuits (`circuit=1`, a closed driveline ring), through the same stage writer
  as the rally stages (`stage::Input::circuit`; the AC converter's circuit conventions: last ring point repeats the first, start, finish and end keys at `rbr_btb::circuit::FAR`, no pacenotes). The route files are
  the same set as a rally route's. `progress_track.xml` says `track type=circuit total_distance=<lap>`; routes are layouts of one venue (not directions): the
  lap is 1207, 937, 683 m (Höljes 0, 1, 2), 1036, 986 m (Hell), 1349, 990, 575 m (Lydden Hill); the gates run once round and the last lies one gate step from the first (8 to 15 m).
- Progress route id 0 is the lap: splits `finish` (the lap line; Lydden 0 and 1 name a late gate, the rest gate 0), `first_corner` and `time` ones (1 to 3). Route id 1 is the joker lap: `joker_lap_start`
  and `joker_lap_finish` gates and `jokerGates` (the detour's own 13 to 17 gates). The joker lap is not imported.
- `ai_track.xml` holds the gates of every layout of the venue (Höljes 182 to 219, with unlinked ones), `links` (`from_gate` to `to_gate`) and `fork_sets` (`decision_type=joker`: a fork's branch used for `race` is the
  lap, the `joker` one the detour; Hell has a second fork on the detour's own gates). The lap is the links walked from the gate nearest the lap line (among gates that have a link out), the race branch at each fork,
  until it returns (`route::lap_road`): its length is the file's `total_distance` within 3 % on all eight routes. Gates are 8 to 30 m apart (Höljes 2's 20 to 30 m), the line is cut at the 2.5 m
  resample like a rally route's.
- `grids.pssg` holds a standing grid (`grid_start_standing_*`, slot_000..004, rows `grid_start_staggered_*`), but its pole lies 20 to 34 m off the road and 3 to 5 m high on all three venues (the
  progress gates and the AI road agree on the road, the grid is not on the lap), and the grid's matrix has the track going -z (slots at +z are behind): not used. The car stands 3.7 m behind the lap line
  as on a rally route, and the lap is timed from 1 m behind that, the ring's seam: a car on the seam itself could come out with progress just behind it and the next step counted a lap (Lydden Hill 0: Lap at 0.15 s
  with the seam at the spawn, 109.6 s with it 1 m behind; Höljes 2 had been lucky). The AC converter's circuit has the car on the seam: the same risk there.
- Titles: `Höljes`, `Hell`, `Lydden Hill` for route 0, `<venue>, layout N` for the others. Physics by the majority rule: tarmac on all eight (Höljes and Hell are part gravel, which keeps its surface ids but
  drives on the tarmac tyre model). Rallycross has no codriver files: no pacenotes are written.
- Imports (release, 8 cores): 28 to 47 s; 442 k to 601 k drawn triangles (Hell 0 5.8 M instanced), 83 k to 380 k collision triangles; Hell 0 and Lydden Hill 2 reach 65.4 k of a cell's 65536 vertices.
- Driven headless (btb-autopilot, car 2, `--laps 1`, `rbr-standalone` Started, Sector x3, Lap, Finished): all eight lap and finish, 0.7 to 1.2 m off the driveline at most; lap times 135.9, 95.0, 71.5 s
  (Höljes 0 to 2), 95.2, 91.6 s (Hell), 109.7, 90.7, 53.8 s (Lydden Hill). Shots (everything on, 4 points a venue, route 0): no converter defect specific to rallycross seen; Höljes' and Hell's gravel
  loops draw as dirt under the tarmac tyre model.
- Not done: the joker lap (the detour and its `jokerGates`; a circuit has one line), the start grid's real place, the venue's own start (the grid is the heat start; the lap line is the finish line).

## Normal maps and gloss, 2026-11-24

- Encoding (every `TNormalMap*` of props, terrain and trees alike): DXT5nm, alpha is x, green y, red 1, blue 0. Green points up the image: integrating 44 terrain maps to heights (Poisson solve over the four
  sign choices), 35 correlate best with their diffuse (cracks and mortar low) with `dh/du = -x/z`, `dh/dv = +y/z` (v down the image), the other three choices 0 to 5 votes. `Texture::normal_dds` writes red x, green
  `255 - y` (along v, as the renderer's `bumped` reads it), opaque DXT1, as the AC conversions' normal maps are stored.
- Gloss: props carry a separate `TSpecularMap` RGB (rock 4 to 15 of 255, painted cars R about 90, G 120 to 216, B about 20; channel meaning unread, there is no spec constant in any material). Written as the red
  channel in grey (`specular_dds`), `SpecularColor` 0.5, `SpecularPower` 40, ReRBR's `BTB_DiffuseBumpSpecular`. At 1.0 and 16 painted car bodies read white in the sun.
- Props (`import::Placer::slot`): solid materials (not `blended`) with a `TNormalMap` and/or `TSpecularMap` write both; leaves and wire keep the card. The tree sheets' (`treesheet_foliage_norm`, billboards) normals are
  not used.
- Terrain (`shade::Layers::normal`): the ground's renderer path is `ReRbrLayer*` plus one `normalTexture` read at `uv0 * ReRbrBumpScale`, the same uv the details use, so no renderer change: a material
  takes the normal map of the layer that weighs most over its triangles (vertex weights), at that layer's repeat (`scale` of its mask channel). Not for the islands read in the world's (x, z): the bump has no
  world mode. One normal per pixel for all four layers (a per-pixel sum by the mask) would need the shader to sample four normal maps.
- Result (Turini, Greece, Sweden, Finland, Wales at 0.3, 2, 4 (6) km, `--enhance all`): RMSE against the diffuse-only conversion 1 to 6 of 255 (Turini's rock wall at 6 km 9): grain on rock, gravel and snow, car paint with a soft
  highlight, no sheen elsewhere. `Textures.zip` +30 % Greece 450 -> 554 MB, +33 % Turini 397 -> 506, +20 % Sweden 452 -> 542, +31 % Finland 577 -> 757, +22 % Wales 539 -> 660 (DXT1 normals, 5:6:5). Import under load
  (average 15 to 20): Greece 59 -> 65 s, Turini 51 -> 54 s. Viewer frame at Turini 6 km (1280x720, all on, `--frames 60`): 14.9 to 15.4 ms before, 13.7 to 14.4 after (noise; nothing measurable).

## Light, haze and weather (2026-11-24)

- What the game ships per area: `lighting_{day,day_alt,night[,night_alt]}.xml` (binary XML, one attribute per `param`; wet variants on Hafren, Turini, Finland, Germany, Sweden, none on Greece), `ambientmaps_*`,
  `envmap_*`, `skymap_*.clm`; per route `sky_*.pssg`, `route_skymap_*.clm`, `ambient_sh_*.bin`, `pfx_atmospheric.xml` (`weatherType` dry/wet times `lightingType` day, day_alt, night). `postprocess/effects.xml` has one effect per
  variant (`t~<location>~<area>~<variant>`: `ColourBalance.colour` (saturation: Greece day 0.75, Hafren day 0.71, Turini 1.0), `ToneMap` target 1.0 to 1.1 with a min/max luminance clamp, `Vignette`, bloom, `Palette`
  `dd_<area>_<variant>`); the palettes are 32x16x16 RGBA8 colour-grade LUTs in `palettes.pssg` (gzip PSSG, axes G, B, R; Greece day lifts blacks to (0,0,6) and cools mid-grey to 136/142/153). Not read: the LUTs, the skymaps, the
  sky meshes, the ambient SH. Which variant an event runs is not on disk (only thumbnail names `..._day_alt_dry.tga`).
- `lighting_day.xml` values (Greece 01 / Hafren 01 / Turini 01): sun `rotationangle`/`heightangle` 30/45, -161/48, -105/31; `specular_colour` (sun) 1/.93/.84, .90/.92/1, .90/.93/.93; `fogHazeDistance` 12000, 4000, 3000/6000;
  `fogHazeColour` (.51,.63,.74), grey .44, (.77,.88,.93); sun-side `fogHazeColour2` x `fogHazeBrightness2` (1,.95,.87)x1.37, white x1, (.59,.79,1)x1.3; `HDRRange` 2.2, 0.7, 2.2 (Hafren's day is already overcast); `vfogEnable` false
  everywhere; `skyGlobalColour` .69, .60, (.72,.80,.84).
- Importer, `lighting.rs`, from the area's `lighting_day.xml` and `t~..~day` post effect, written to `settings.ini` `[ReRbrAir]` (`notes/btb.md`): `aerosol` = 1.2 / `fogHazeDistance` (km, the nearer of the two), clamped 0.05 to 0.2 (Greece 0.10,
  Hafren and Turini 0.2); `fog_color` = the haze's colour half-way to its sun-side one (each scaled to its brightest channel); `saturation` = `ColourBalance.colour`; `exposure` = `ToneMap.targetLuminance` (the min/max clamp only
  bounds DiRT Rally's own adaptation, which our meter does; Hafren's 0.315 to 0.555 pins it, not reproduced); `sun_tint` = `specular_colour` over its luminance.
- Calibration (Greece 9 km, Wales 3.5 km, Turini 3 km against `/tmp/dr-gp` frames): on a clear row the aerosol and haze hue are weak (aerosol 0.05 to 0.16 and a warm or grey hue move the far hills a few levels; saturation 0.75
  and the tint are subtle), the pale horizon and hills of the frames come from the row: hazy (`41M_hazy_clear`) for Greece and Turini, overcast for Wales. The converted stage's `aerosol` and hue apply only on clear rows
  (`skylight::sky_uniform`), not on hazy, overcast or rain.
- Brightness against the gameplay frames (2026-11-24, `sun_stops`): the converted Finland and Germany roads read 100 to 110 of 255 against DiRT Rally's 150 to 200 because the renderer exposes a sunlit clear day 1.5 stops down (`post::SUN_EXPOSURE`, a photo's look; scaled by the sun's share and the clouds'), while DiRT Rally's sunlit gravel is not held down; Greece (hazy) and Hafren (overcast) were already level with the frames since their share is small. Measured (Finland 12 km road/grass, Germany 9 km road, Greece 6 km, Hafren 3.5 km road): `sun_stops` 1.5 -> 0 gave 108/98/86 -> 160/145/128 (grass 80 -> 121), 116/112/103 -> 169/163/151, 104/92/82 -> 108/96/86, 125/124/123 -> 127/126/125 (DR frames: Finland 169/154/145 shade, 198/175/159 sun; Germany sun 152/133/105; its road is more orange, hue not addressed). Importer writes `sun_stops=0` always. Ruled out: `exposure` alone (1.8 -> x1.26, 3 -> road 171 but Greece and Hafren go pale grey, 184/178/166 on Germany), the diffuse bake (its section mean is 0.5 x `magic` 2: the layer's own albedo, `ColourMapScale` is already honoured), `HDRRange`, `gi_*` and `deferred*` fields (no pattern against the needed factors 1.8 Finland, 1.3 Germany, 1.0 Greece and Hafren).
- Colour grade (2026-11-24, `grade.rs`, `[ReRbrAir]` `grade`): the palette named by the day effect's `Palette` (`dd_<area>_day`, `palettes.pssg`, 32 x 16 x 16 `u8` BGRA, texel x = G, y = B*16 + R) is written as a 32 x 256 RGBA PNG in `Textures.zip`; the post pass applies it last. Input space: display-referred (sRGB-coded, after tone curve and saturation). Measured offline on Germany's frames (3 patches at the one aligned location, DR 5:24 against our 9 km: sunlit road, shaded road, sunlit grass): the LUT on the linear light is out (sunlit road 37/27/18 against DR 139/116/89, rms 56 against 33 ungraded); on the coded colour its hue lands on DR's (road R/B 1.60 against DR 1.56, ungraded 1.12). The gamma 0.9 before or after the table, pure 2.2 against sRGB, are not separable on these frames (rms 24 to 26 for all: our scenes and DR's differ in content): not modelled. Germany day's `tintAmount` is 0, so `tintColour` is unused; `gamma` 0.9, `colour` 0.95 (the existing `saturation`) and `hdrWhiteout` 1 are left out. Finland's day palette is the identity, Hafren's and Turini's close to it, Greece's lifts mid-grey 128 to 154 (luma) and warms. The table brightens, so the importer takes its lift at a sunlit road's grey (0.6) back out of `exposure` (`Grade::lift`, `Lighting::graded`: exposure divided by (luma out / in)^2.2). Renders (1280x720, default enhancements, road box bottom centre, before = no grade, after = grade and exposure divided by the lift): Germany 9 km 155/155/154 -> 168/152/128 with the compensation alone, DR 139/116/89 (sunlit gravel 152/133/105), so Germany's exposure is trimmed by 0.76 (`grade::trim`): 145/130/106, the road and grass turn orange and golden as in the frames; Finland 130/122/116 -> 129/120/114 (identity table); Hafren 138/136/133 -> 123/128/126 and Turini 141/130/117 -> 138/131/124 (near-identity tables); Greece 108/97/87 -> 125/95/88: level in brightness but the table, which cools and pinks mid-grey (128 -> 164/149/148), turns our grey-brown road pink where DR's golden road (194/169/144) is warm before the table: Greece's look wants a warmer pre-grade (sun and haze), not shown to match. `sun_stops` has no effect in the plain viewer (the sunlit offset needs `--enhance lighting`), so the retune is by `exposure`. Finland's road reads 130 here against 160 at the same settings in the earlier measurement (main moved since), DR 177/156/144.
- Own weather (`track.ini` `weather_good`, `weather_bad`, `Season::with_own`): `weather_good` = `41O_M_norain_lightcloud` when the day's `HDRRange` is under 1.5 (Hafren), else `41M_hazy_clear` when `fogHazeBrightness2` >= 1.3 or the
  haze reaches 6 km or less (Greece, Turini, Sweden), else none (Finland, Germany stay on the retail good row); `weather_bad` = `41O_M_lightrain_heavycloud` where the area has a wet variant. Wales' own good weather has no damp
  road (no map 41 row has a damp, heavy-cloud, fog-free section); `lightfog_heavycloud` was too thick against the frames. Open: the clock and the sun's azimuth (see "Sun and place").


## Grass, 2026-11-24

- `grass.grs` (per area; all 18 retail files read, `grass.rs` tests): `u32 7, T (8 types, Lydden Hill 6), N, 0`; words 4 to 908 the tuft types'
  parameters and names (`t<type>_<model>`, `type_0T`: not read); word 909 on N ascending byte offsets, the cells after. A cell: `u16 vertices, u16
  triangles`, 3 zero words, box `[xmin ymin zmin xmax ymax zmax]` in DiRT Rally's world, vertices of 8 bytes (`u16` x and `u16` z over the box, `u8` y over its
  height, 3 bytes `11 10 84` but for 7% of them: RGB565 grey 0x8410 with `ff` before it where it differs; a tint, not read), triangles of 4 bytes (three
  indices, a flag). Flag bits 4 and 5 are a class 0..3 (0 a 3 to 8 m band at the road's shoulders, 1 the main field, 2 and 3 other zones: Greece's south
  and Wales' tarmac sections and forest; their meaning is not known), bit 3 and the rest not read.
- It is the terrain surface where grass grows, the road cut out, 4 m triangles away from the road: vertex heights lie on the gates' crossings to 0.25 m (Greece, 839 gates, median
  0.03 m), vertices on an exact 4.00 m lattice, holes where nothing grows (4 m squares in Wales' verges). Greece 362 k vertices, 439 k triangles; Wales 718 k and 847 k; the area's
  routes share it. Grass lies within 8 m of at least 96 % of every rally route's gates (Pikes Peak 55 to 99 %: rock).
- `ground_cover.pssg`: one quad a tuft (`RENDERNODE t<type>_<model>`, 53 on Greece; position extents are its size in metres, `uv` its rectangle in `gc_billboard_d`, a
  2048 x 1024 DXT5 atlas: Greece's is dry golden grass, thistles and scrub, Wales' green grass, rushes and brambles). `ground_cover.xml`: draw distance 150 m, fade 120 + 40.
- Import (`grass.rs`, `stage.rs`): the triangles are rasterised to a 2 m cover map (class + 1, centres within 0.4 m of a triangle) over the drawn rectangle, as
  `Textures.zip`'s PNG with `cover=` and `cover_rect=` (`notes/btb.md`); the atlas goes in as it is, `grid` one texel a cell, `base=0`, one group holding every model with its
  rectangle and metre size as `SIZE_MULT` (`size` makes a cell 1 m: 3.2 clumps a m2, `Fx::density`), a model taller or wider than 0.8 m rarer by the square of the ratio. The
  terrain materials (not decals) carry `ReRbrGrass=1`; the map alone decides where. Every class takes the same pieces (the class is stored for later).
- Result (all enhancements, `--enhance all,grass`, 0.3, 2, 4 km, `/tmp/grs/cmp-*.png`): Greece's verges grow golden tufts and scrub where they were bare, Wales' green tufts line the road in the forest, Finland's tufts replace the old lawn, all in the atlas's own colours
  and sizes (first render: olive and 5 m bushes, from the base cards' ground hue and size spread; see `notes/btb.md`). Viewer load (1280x720, `--screenshot`, best of 2): Greece 6.7 s both, 1.78 to 1.84 GB;
  Wales 9.35 to 9.05 s, 2.67 to 2.68 GB. Greece `Textures.zip` +0.25 MB (400.22 to 400.48 MB); import time unmeasured (the machine was shared). The 37 reference shots: RMSE 0 against the build before.
- Open: the class's meaning (which tufts each zone grows), the types' parameters (counts, scales), the tint bytes, tufts' densities against the game's (ours is a guess from the reference shots).

- Cost (2026-11-24, RX 9060 XT, 3840x2160, Ultra, dense grass, autopilot): Finland's cover-map atlas has 4149 pieces and `piece_of` scanned them linearly in the grass compute (grass pass 9.8 ms; GPU frame 25.1 -> 15.1 ms with grass off); a binary search makes it 0.12 ms (17.5 ms frame incl. the grass draws), 37 shots and 6 DR shots RMSE 0. Turini's `scene` 10.4 ms: instanced draws 5.2 (alpha-tested foliage 3.9, opaque 1.0), baked opaque terrain 2.2; lighting off 5.6 (shadow sampling 4.8); grass draws 0.25; `RBR_BTB_INSTANCE=off` 15.9. Instanced groups beyond 250 m of their centre are most of the instanced cost (about 4 of 5 ms; machine shared, noisy): 3 to 4 k-triangle trees run to ~1 km before the billboard. Open: a nearer far range for dense models (a quality choice).

## Cars (`car.rs`, `car_pack.rs`), 2026-11-24

First car: the Escort Mk II (`cars/models/cme`), drawn by the viewer with NGP physics on a retail stage, on the live path
(`viewer ... --car-pack <physics pack> --car-model <out>/Cars/<name>/<name>.sgc --live rbr-standalone --live ...`).
`rbr-dirt-rally car CAR --class CLASS --template <car folder or .sgc> --out DIR [--livery N] [--name NAME]` (the livery is the car's first
without `--livery`; the class is required: it is in the game's database, not in the car's files).

- Liveries (2026-11-25): the model's own textures are the chosen livery's; each other livery (`livery_NN`, 1 to 11 a car) is a skin of the model,
  `Cars/<name>/skins/Livery NN/textures/`, which the launcher's Skin box lists for the added car (`car_skins`, a model's own `skins/`). A skin
  holds only the files a livery's file gives (paint atlases with their baked maps, wheels, glass, lights, their normal maps) that differ from
  the model's; a reconversion drops the `Livery *` folders first. Every livery of a car holds the same texture names and sizes (17 cars
  surveyed). Livery names are not in the readable files (the front end's database is `game.bin`, encrypted): the skins are numbered. Cost on
  the Escort: 10 skins of about 77 MB, 68 s more. Checked: chase shots of the Escort base, 17, 33 and 44 with `--car-skin`.

- Added cars (`added_car.rs`, 2026-11-24): `rbr-dirt-rally add-car CAR [--livery N] [--cars DIR]` adds a DiRT car to the launcher's
  added cars (`<player folder>/cars`, `rbr-launcher`'s `car_packs`) as a car of its own, on the NGP physics of the gvrc.de car of its model,
  which must be added first: a copy of that car's pack as `rsfdata/cars/<pack> (DiRT Rally)/`, the model as `Cars/DiRT_<car>/` (the gvrc model as
  the template), and a `cars.ini` section copying its group, drive, year, revision and bank, with a new id (kept on a re-add). The launcher lists and
  removes it as any added car. DiRT's own physics is not usable: `*.ctf` and `game.bin` are encrypted (the `.ctf` header names the car in
  plain text, `Ford Escort RS1800 MK2 - [Tarmac] RWD - v2`); the readable files are AI tables, audio and damage. `MATCHES` pairs 17 of the
  46 cars with a gvrc car of the same body (cme, 131, str, m3r, mta, 20b, ers, 555, 306, imp, for, c4r, mer, sti, mcr, ffr, pol; some a year or
  evolution apart: 20b on the 205 Evo 1, imp on the S9, pol and ffr a season later); the others have no gvrc car of their body. The classes in the
  table are by era, not read. All 17 add, list as startable (`rerbr --list` with `RERBR_HOME` on a copy) and drive; side shots at Gabiria show the
  wheels in their arches and the crew in the seats. Fit, DiRT's wheelbase against the template's rest wheels: within 30 mm for 11 cars,
  +44 to +109 mm for ers, 131, ffr, mcr and sti, -144 mm for cme (the gvrc Escort is drawn large: tracks 1.55 against DiRT's 1.43); the body is
  centred between the axles, so half of it shows at each arch. Tracks differ by up to 11 cm (131, imp, mer front, for rear). The Mini (`mcr`) shows two grey triangles of its interior door frames
  (`int_door_fl/fr` `Matt`, up to the roof) through the rear of the front side windows from outside.
- Template wheels: the renderer's rest places (`rbr-render` `car::wheels`), the first class (tarmac, gravel, snow) whose four wheel nodes are
  more than 0.2 m apart. RSF's Polo WRC 2016, i20s, DS3 WRC and others stack their tarmac wheels at one point: with the tarmac nodes, the Polo's
  body sat 1.25 m behind the physics and the crew's heads showed on the bonnet.

- All cars (survey of the 46 under `cars/models`, 2026-11-24): the exterior materials are named alike on every car (`Car Body Paint`, `Matt`,
  `Shiny Metal`, `Cabin`, `Window Glass`, `Lights Glass/Pod/Satin`, `Discs`, `Suspension`, the class wheels and tyres; some cars add `Grills`,
  `Mud Flaps`, `Aerials`, `Caliper`, `Carbon Fiber` and `Spare <class> ...`), but each car and material has its own `OcclusionMapScale` (the paint 0
  to 1), read from the file. The interior's materials bind their textures (`TDiffuseAlphaMap`, `TNormalMap`, `TOcclusionMap`, `TSpecularMap`,
  `TCarbonDiffuseMap`; names vary: `int_<car>_nm` or `int_<car>_main_nm`, `.TGA` on some), so `interior_style` reads them, by shader
  (`interior`, `_flock`, `_alpha`, `_netting`, `_carbon`, `motec`, `lcd_text`, `mirror`, `interior_windscreen`). Five cars (`i20`, `306`, `6rb`,
  `fow`, `pol`) index their largest sources with `uint`; some have no `livery_00`. All 46 convert (each with the Escort as template, class
  `historic`); the i20 and the 208 RX (`20x`: own brake lens, mud flaps, calipers, window net) look right in the Gabiria doc shot from the chase
  and the cockpit.

- Files per car under `cars/models/<car>/`: `<car>_highlod.pssg` and `_lowlod.pssg` (gzip PSSG, the stages' container),
  `livery_NN/textures_high/<car>_tex[_tm|_gr|_sn|_wt]_high_NN.pssg`, class textures in `cars/generics/<class>/car_gen_*_high.pssg`,
  the cockpit in `cars/interiors/models/<car>/` (`int_<car>.pssg`, `int_<car>.xml`). Physics (`*.ctf`, encrypted), engine bundles (`.bdl`) and banks are not read. The car's name
  is only in the `.ctf` header (plain text, some with stray leading bytes); the folder code and the engine bundle in `audio_car_setup.xml`
  (`4cyl_na_dat_escort_mkii_dd`) also say which car it is.
- Scene: `LOD0_` over one `MATRIXPALETTEJOINTNODE` per part (`x0_door_fl`), each holding `MATRIXPALETTEJOINTRENDERINSTANCE`s: a range of a shared
  `RENDERDATASOURCE` (`streamOffset`, `elementCountFromOffset`; `indexOffset`, `indicesCountFromOffset`; indices count from the range's first
  vertex), the material (`shader="#Car Body Paint"`) and a joint. Vertices are in the part's own space (the part's node `TRANSFORM` places it:
  wheels at their hubs, vertices about the hub); `primitive` is on the index source, `SkinIndices` is a scalar `float` stream. Frame: x left,
  y up, z forward, right handed (the Escort: wheels at z +1.316 and -1.057, x 0.714 and 0.724, hub height 0.398, tyre radius 0.32).
- Materials hold constants only; textures bind by name: `cme_main` (4096 DXT1, paint and every `Matt` part; one written texture per
  `OcclusionMapScale`, key `main<percent>`), `cme_cabin`, `cme_glass`, `cme_lights`, `disc`, `caliper`, `cme_wheel` (per class file; the rim and the
  sidewall share it, key `wheel_<class>`; the spare is drawn once, as the tarmac class's), `tread` (class generics), `suspension` and `grills` (class
  generics). A normal map is written once per source (`dr_<name>[_<class>].dds`). The livery files' formats are
  `dxt1_srgb`/`dxt5_srgb` (`texture::read_car`; stage reading is unchanged). UVs need no flip.
- Into the renderer's car format: z mirrored about the wheels' centre, wheels laid on the template's (wheelbase 2.373 against the RSF Escort's 2.518:
  the body is 7 cm short at each axle against the physics' hubs), triangle order flipped (the mirror; unflipped, the outer skins were culled).
  Parts become `e_body` (everything outside, split at 60000 vertices), `e_window_*`, `e_wheel_{tarmac,gravel,snow}_<side>` (rim, tyre and disc, node at
  the hub), `e_internal` and `i_internal` (the cockpit, below) and `i_body`; the template's collision hulls, helper nodes, points, cameras, lamp and
  shader settings are kept, so the physics pack's frame holds.
- Cockpit (`int_cme.pssg`, `mesh::scene`, `car::interior`): no `LOD0_`; a `ROOTNODE` tree of `NODE`/`RENDERNODE`s with `RENDERSTREAMINSTANCE`s (the
  stages' reader), one node per piece (`int_main` dash and lining, `int_seats`, `int_rollcage`, `int_door_fl/fr`, `int_window_f/fl/fr` glass,
  `int_dials`, `int_needle_tach`, `int_digi_speed`, `int_indicator_gear/limit/shift_1`, pedals, `int_handbrake`, `int_gearstick`,
  `int_steering_wheel`, `int_wiper_*` with `_pos_01/02` animation keys, `int_reflection_m/r` mirrors) at its rest pose, already in the exterior's
  frame (every node's own transform is identity at the car). Not read: the animation files and the environment map.
  - Materials, by what they bind (key: the texture's name without the car's): the dash, doors, seats, cage, levers and wheel share the atlas
    `int_cme_main` whatever their shader (`CarTextureNoDirt`, key `int_main`, with every map any of its materials gives it; the car's own maps only,
    the shared `int_gen_sp` is a 32 texel constant); the gauge faces `int_cme_dials` (`int_dials`), the shift and limit lights `int_indicator`
    (`int_indicator`, unlit); the alpha and netting shaders' alpha is their opacity (`_alpha` keys). `carbon fibre`'s UVs lie on the atlas's
    occlusion layout (they showed knobs and decals on the dash top): it draws the weave `int_carbon_diff` with the UVs times its `CarbonUVRepeat`
    (the Escort 500, most cars 300). Not drawn: `lcd_text` by its font (the working displays below give
    those quads a texture of their own). `interior glass windows` (`interior_windscreen.fx`) is `CarIntGlass` with `TDiffuseAlphaMap` (the frit
    border, the tint band, smears in the alpha) and `TDustMap`'s red laid over it, 0.25 of the way in a grey brown (`car_maps::windscreen`; DiRT
    raises the dust as the car runs, ours is fixed); the outside copy leaves it out, the body's glass is the windows there. The two mirrors (`int_reflection_m`, `_r`) are parts named `i_mirrorglass_m` and `_r` in the glass class with a flat texel,
    which `rbr-render`'s mirrors pass takes as glass: `--mirrors 1` shows the road behind in the centre mirror and the flank in the wing mirror.
  - Sets: `i_internal` and `e_internal` are both this scene (the exterior file's `Cabin` material is DiRT's low-detail copy and is used only for a car
    without an interior file); the outside copy drops the triangles above the windscreen's top or beyond the side windows, 1 cm on (`clip`, `within_body`; the
    Escort 1.40 and 0.68, found by eye first; the rear window's top, 1.41, would keep the bands), the cage's roof tubes and the lining
    that lie in the paint and showed as grey bands along the roof. The steering wheel is the scene's own, taken into the exterior wheel's node, in
    `i_steeringwheel` and `e_steeringwheel`. `i_body` is the exterior parts `int_cme.xml` keeps active in the cockpit (`active_nodes`: bonnet, wings,
    doors, the driver's mirror, the engine and under-bonnet, the light rigs) with their exterior materials; the renderer draws only the `i_*` set in the
    cockpit, so these are what shows of the outside there (not looked at one by one: the bonnet is under the dash line from the head).
  - The renderer moves the cockpit eye back `COCKPIT_BACK` 0.5 m from `Cam_internal` (`views::cockpit_back`); DiRT's `head-cam` is at the head, among
    the headrests, so unchanged the eye sat behind them and the seats' top edge crossed the windscreen as a dark bar (found by dropping nodes one at
    a time; seats moved 0.5 m back, bar gone). `Cam_internal`'s `Pos` is put 0.5 m ahead of the head (`CAMERAS`), the eye lands on it (or 0.05 m
    behind for the wheel's `WHEEL_BEHIND`).
- Cockpit against DiRT (2026-11-24, cme, a DiRT head-cam video frame): at the same field of view (the video's is DiRT's widest, 70
  degrees up and down, `--fov 102` across) the framing, dash, cage, door bars, wheel and mirrors match. The eye ends 0.65 m behind the wheel
  (`WHEEL_BEHIND`) against DiRT's head-cam's 0.61 m (`position` z -0.32, wheel 0.29). In sun (`--weather dry --clouds clear`) the dash, roof and
  wheel are as dark as DiRT's. In Gabiria's haze the cabin is greyer, from the ambient light.
- Cockpits against DiRT screenshots in sun (2026-11-25, ffr, m3r, ers, 15:00): textures match. The interior occlusion is baked at full strength
  (`INTERIOR_OCCLUSION` 1.0, was 0.5): the footwell and under the dash darken; the lower third's mean grey went 56 to 53 (ffr, DiRT 34), 53 to 48
  (m3r, DiRT 57), 47 to 43 (ers, DiRT 31). The rest is the renderer's cabin exposure (`CABIN_STOPS`, shared with every car, unchanged) and no sun
  patches inside the cabin. Not DiRT's: the driver's arms (ours with `driver = on`, `camera_prefs`), nothing more on the dash (the digits, bars and lamps work, `gauges.rs`).
- Steering wheel: `x0_steering_wheel` becomes `i_steeringwheel` and `e_steeringwheel`, node at the hub, local axes from the part's rows (x, y and the
  z column negated for the mirrored local vertices; the same sense as the template's matrix, so the renderer's `wheel_turn` turns it the right way).
  Cockpit, bonnet and bumper cameras: DiRT's `cameras.xml` (BXML, `rbr-dirt-rally xml FILE`) views `head-cam` (`position`), `bumper` and `bonnet`
  (`offset`) place the template's `Cam_internal`, `Cam_bonnet` and `Cam_bonnet2` (look direction and length kept). The Escort is right-hand drive and
  the RSF one left (the wheel hub x -0.343 against +0.299): when the wheel is on the other side from `D_driver_use`, the crew nodes (both seats, the
  gear and handbrake hands) are mirrored in x, so the driver sits at the DiRT wheel; the game's driver clip shifts with the right hand, so the right
  hand reaches across to the lever on its left.
- Lamps: `Lights Glass` is the lens, its UVs on the `cme_lights` atlas (red and amber at the tail, white at the reverse, the clear circle at the head;
  the atlas's alpha is the lens's own, so it is `CarGlass` with that texture, `dr_lens.dds`). The lenses `light_glass_f{l,r}`, `light_combi_glass_b{l,r}`
  and `light_rev_glass_b{l,r}` are each an unlit part (`Predicate = off`) and a lit one (`_lit`, `on`) on the switch boxes `light_lf`, `brake_lb`,
  `reverse_lb` and so on, named as the renderer names lamps (`e_light_lf`, `e_light_brake_lb`, `e_light_reverse_lb`), so the headlight key, the brake
  pedal and the reverse gear light them (the renderer glows a lit lens by its area). DiRT's combined tail and brake lamp is both: the tail's
  part (`e_light_lb`, `_lit`) always on, so it glows at night, and a brake `_lit` over it, same faces, with no unlit brake part to cover the tail's
  lit one. The second lamp sets and the bumper's rally lights (`light_2_glass_*`, `light_rig_glass_*`, `light_2_brake_glass_*`) are lamp parts
  on the same boxes (`e_light_rig_lf` on `light_lf`), so the headlight key lights them; their pods (`light_rig_pod_*`) and the small lenses on the
  bumpers and wings stay in the body. Beams: the model gets the template's night option as its own `light_on/` (`night_lights`: the template's
  `rsf::night_options` overlaid in order on its `.ini` and `_settings.ini`, the last value of a key kept; the `point_*` sections and the whole
  settings written), as the gvrc Mini WRC's lamp points are only in its option folder and the model had no beams at night. The gvrc Escort's
  `light_ramp_on` sets `[e_lightglass_rf]` twice and disagrees with itself, so the renderer's checked merge is not used.
  The lit parts draw the lens as `lens_lit` with the livery's `<car>_lights_on` (the lit bulbs and reflectors on the lights atlas's layout,
  black elsewhere) as its glow map, and `rbr-render`'s `lamp_shown` takes a lit part's glow map in place of its texture for where and in
  what colour it shines (`lit_styles`): the combined lamp's lit tail shines on its red segment, not over the amber one (cme, ers at 22:30).
- Look (2026-11-24; the launcher's default is Ultra: `--enhance` lighting, cabin, particles and the rest, which is what these checks use; the renders
  without them hide the first two):
  - The cockpit's textures are `CarTextureNoDirt` like the template's `e_internal` (as `CarPlastic`, Ka and Kd 2, it glared teal), at the
    atlas's own colours. A 4x tone curve on the DXT blocks (for a dash that came out black) washed every car's cockpit grey; side by side with
    DiRT's head-cam the untouched atlas matches (2026-11-24). Shader constants of the material (`Ambient`, `Diffuse`) change nothing.
  - Night: an interior material's `TEmissiveMap` (`int_<car>_dials_on`, `int_indicator_on`) is written as `dr_<key>_glow.dds` beside its texture,
    the renderer's car texture glow mask (`notes/renderer.md`), so the dials and lamps are backlit at night on every car, not only cme's RBRHUD pack.
  - The paint keeps the template's `Kf` (0.21), so the renderer's clear coat (sky reflection and a broad sheen, on where `Kf` and `FresnelMax` are
    above 0) is on, as on the stock Escort. Earlier it was forced to 0 because the coat washed the livery's saturated red (168, 7, 10) to pink; with
    it off the car had no reflection at all ("doesn't look reflective"). On: the boot lid, roof and spoiler mirror the sky, and the red does go
    pink at grazing angles in hazy light, the yellow decals pale. The diffuse alpha (the specular map over `FULL_SPECULAR` 0.9) scales the coat:
    paint about 0.5, bare metal and chrome 1, plastics and rubber under 0.25; alpha 0.5 against 0.75 and 1 looked alike (the Fresnel term at a grazing
    view dominates). `Shiny Metal` is `CarPaint` too (DiRT gives it `carpaint_metallic.fx`; as `CarTextureNoDirt` it had no reflection).
  - Maps (`car_maps.rs`, 2026-11-24): the renderer's cars read one diffuse (alpha = reflection mask) and, new, a normal map per material
    (`[<material>_bump_bitmap]` in `_shaders.ini`, `car_parts::bump_path`; `DrawState::surface` with `NormalMap`, the stages' screen-space `bumped()`, so no
    tangents; `shade_car` takes the bent normal, and the diffuse alpha, in an unblended bump draw, scales the sun highlight, the clear coat's sheen and
    the cabin's sheen: `gloss`). No retail car has the section: four stock-car shots (day, night, rain, cockpit) render with RMSE 0 against the build
    without it. Converter side: `cme_specocc` is specular in RGB (the paint 0.43) and baked occlusion in the
    alpha, on `cme_main`'s UVs; the occlusion goes into the colour in linear light with each material's `OcclusionMapScale` (paint 0.4, `Matt` 0.8,
    `Shiny Metal` 0.5: keys `main40`, `main80`, `main50`; the interior atlas 0.5 of its 1, `INTERIOR_OCCLUSION`, as the cabin light already shades it), the
    specular becomes the alpha, and both are written BC7 (a re-encoded 4096 keeps its colours). Normal maps (written BC7, x red, green flipped to run down
    the image; tread, disc, suspension and grille maps integrate to heights that follow their diffuse with that sign, the wheel's does not tell): body
    (`cme_main_nm`, DXT5 with x in the alpha), lights and lens, rim and sidewall, tread, disc, suspension, grille, and the interior atlas (`int_cme_nm`, RGB).
    `cme_main_nm`'s panels (doors, bonnet, roof, wings) hold a foil-like crumple, the damage layer DiRT blends in as the car is hit; it creased the whole
    door, so `car_maps::flattened` sends the slopes to flat where a 6 texel blur of them is large (the hardware's bolts, vents and edges stay). The
    damage state itself is not drawn. Also fixed: the tyre's sidewall (`... Tyre Wall`, UVs over all 0..1) is on the wheel's atlas (`wheel_<class>`, with the rim; it
    had the tread texture, blocks all over the sidewall) and now shows `255/50-16 RADIAL`; the right-hand tyres' lettering reads mirrored.
  - Needles (`needles.rs`, 2026-11-24): every car's `int_<car>.xml` `<displays type="analog">` names its needle meshes (`value` tach, speed or boost,
    `node`, `min_value`/`max_value` in the game's unit times `factor`, `long_way`). The interior scene holds each at rest (the range's bottom) and a node
    `<needle>_max` at the top; the turn between them (axis and angle, the long way round, angle - 360, where `long_way` is 1) is the sweep, about the
    rest node's place. `car` writes them, mirrored into the template's frame (sweep negated), as `<name>_gauges.ini` (`rbr_formats::car_gauges`) and
    each needle as a part of its own (`i_needle_tach`); the renderer turns it (`rbr_render::needles`). cme's tach sweeps 341.7 degrees (338.9 by
    hand on its ticks). Checked in a live drive: cme 5.5 thousand rpm in first at 51 km/h; 555 speed, tach and boost at the HUD's 46 km/h, 5225 rpm
    and 0.8 bar over the air (the HUD's 1.81 is absolute; DiRT's 555 gauge runs -1 to 2.5, so relative). Of the 17 added cars, 11 have needles
    (306, imp, for, c4r, ffr, pol name none with a mesh); `analog_dummy` (jitter) needles stay at rest.
  - Working displays (`gauges.rs`, 2026-11-25): every display of the config's `digital`, `motec` and `indicator_light` lists whose node
    is in the scene (most configs list digital displays the car lacks) gets a texture of its own (`dr_gauge_<node>.dds`; its faces' material
    is renamed to it), which a config of an RBRHUD pack (`rbr_render::dials`) beside `Cars/` redraws (`<out>/Plugins/RBRHUD/gauges/<name>/`,
    one `config<N>.ini` per display, one sprite sheet; the viewer reads it from the root of the `--car-model` path). Digits: DiRT's quad UVs
    span the whole font strip (64 x 512, ten cells: the digits, or the gear's N, 1 to 8 and R) and the game scrolls them, so the texture is
    `num_chars` cells side by side, each a font cell stretched to 64 x 64, u across the characters and v down one cell: `SpeedKPH` or `RPM`
    right-justified (`Number`), the gear a sprite per drive gear (0 reverse, the font's last cell; 1 neutral; 2 first). The font is the node's
    material's `TEmissiveMap`, whatever its shader (`lcd_text`, or `interior` on the cars whose gear display is a rounded panel): an LCD's
    (DXT5, black digits in the alpha) on a clear texture, the faces blended over the face behind (`OpacityType=Bitmap`), no glow; an LED's
    (DXT1, a dim red on black) at its hue's full strength with its brightness as the alpha, on black, glowing at night. Bar graphs (`motec`,
    306, Impreza, Polo): the face (`TDiffuseAlphaMap`) with each bar of `TDiffuseAlphaMap_2` (the texels of one value of the `u8`
    `TAlphaMaskMap`, 198 to 234 for 30 bars, 255 elsewhere) lit past its engine speed, the mask's `min_alpha` to `max_alpha` spanning
    `min_value` to `max_value` (thousands of rpm times `factor` over rad/s); the face's `_on` map is the night glow. Lamps: the lamp's
    `int_indicator` copy with the lit half (diffuse plus emissive) over the unlit half its UVs show past `cond_min`, a dimmed unlit half (0.4)
    under it; night glow the lit map at 0.6 on both halves. Checked on 306, Impreza, Polo and Escort in a moving start at 14:00 and 22:30:
    306 `51` km/h and the bars at about 4.5 thousand rpm on the LCD, `2` on the gear LED; Polo `46` and gear `1`; the Escort's speed on its
    own LCD quad beside the dials atlas's printed `MPH` (the game's factor is 3.6, km/h); Mini `46` and `4582` rpm; C4 `46` and gear `1`. The
    display faces go last in the cabin's pieces (`Model::cabin`): the renderer draws shaders in the order of their first faces, and the
    Mini's dial face (`interior_alpha`, blended) drawn after its LCD quads covered the digits. The oil LCD and the trip computer stay as printed.
  - The gear display sits at the instrument pod's left edge, behind the wheel's rim from the head, and by day the pod's hood shades it dark (the cabin
    light keeps only the needle dials' faces at the game's light, `car::cabin::gauge_faces`); it shows at night. A frame of a moving car: needle at
    its rpm, the digits at its km/h, the shift light lit past 7900 (night, 8.5 thousand rpm), the limit light at 8900 not yet.
  - The rear bulkhead (`int_main`) shows from outside through the rear quarter glass as a flat dark panel with sharp edges; it is DiRT's geometry,
    which DiRT never draws from outside.
- Not done: doors and hoods as their own hinged or damageable parts (no dents), the interior's wipers and the glass's cracks (its dust is fixed), the oil and trip LCDs (static), the damage
  crumple normals (below), `pc_e_window_*` cracks, the lower LOD; for the other cars: a template of their own model
  (the physics), the class from the game's database.
- Checked: loads with 0 missing textures and 0 bad triangles, 18 unit tests (the written mesh parses back, a helper node mirrors, material to class
  for both files, lamp parts' switch boxes, a settings key set in its section, the template's night option written, the config's active nodes, a part taken into
  its own space, the outside clip, the vertex split, the cockpit eye ahead of the head, the displays read from the scene's nodes, their pack reading as an RBRHUD pack with every sprite on its sheet, a bar as one mask value's texels, an LED font at full hue, the sheet's rows, images cropping and stretching), the whole crate's 100 tests (with the needles' turn between two placements and the analog displays read) (the added car's section and the match table, with the maps below: occlusion factor, plane lookup, BC7 output, normal repack and its swizzle, the materials' maps from their constants, the interior's from its bindings (the windscreen's with its dust) and an atlas's maps merged, a separate brake lens, the clip from the glass, the shaders file's bump bitmaps); stills of the car alone, on `track-20_N` at the start and in a
  live autopilot drive (cockpit with the DiRT dash, wheel, gauges, cage, door card, mirrors and eye, bonnet, bumper and chase; side and front from outside; day
  and night, brake lamps lit and unlit; Ultra settings and `--mirrors 1`). Developer commands: `pssg-tree FILE [DEPTH]`, `car-texture PSSG NAME OUT.dds`,
  `car-info` on `int_<car>.pssg`. Gauge stills: the needle at idle, 7.3 and 8.8 thousand rpm, the speed digits 0, 18, 31, 45, 60 km/h, day and night (`--clock 22:30`). Not compared with the game: DiRT's shaders (`carpaint_metallic.fx`) are not ported, the paint is RBR's `CarPaint`.
- Mud (`car_dirt.rs`, 2026-11-24): the class's `body_mud` (generic main file, alpha) is on a layout of its own that every body mesh carries as its second UV set (the first `ST` stream's `zw` where half4, else the second stream); baked per car onto `<car>_main`'s UVs (1024, `dr_dirt.dds`, rasterised per triangle, 2 texels of bleed, 0 where no triangle) and named `Dirt` in `_shaders.ini`, so the renderer's dirt level shows mud at arches, sills and the lower body (cme after 140 s of gravel: `/tmp/car-dirt/after.png`). The vertex `Color` is not used as a weight (unconfirmed). `Matt`, `Mud Flaps`, `Aerials` and `Carbon Fiber` are `CarTexture` (lit as `CarTextureNoDirt` but for the dirt layer) so they take it too.
