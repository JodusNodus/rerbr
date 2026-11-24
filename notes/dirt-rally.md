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
  662 -> 529 MB, 110 -> 88 s, load 7.0 -> 4.7 s, 3.7 -> 1.6 GB. Not done: grass (`grass.grs`), cloth, `ornaments.bin`;
  foliage is drawn one-sided as the BTB mesh is.

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
  The blend formula of the road shaders (below) is not applied: it needs `TBlendMap2/3` bound as textures per draw (fragment textures are 20, past the default 16), and was never seen to improve a view; the chain mix of the weights is what
  the data gives. Vertex colours are used as bytes over 255 (no gamma): snow, rock and road transitions read right. Greece's verges differ from the mask's (a grass layer on banks, less at some road edges): the weights are per pixel
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
- `TBlendMap2/3` (a height map the game roughens a weight's border with) is not used: finer than the mask. Colour-map hue and
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
