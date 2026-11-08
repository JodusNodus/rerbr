# BeamNG levels

`crates/rbr-beamng` converts the player's own copy of a BeamNG level mod and one of its time-trial missions to a BTB
stage folder (`beamng-stage`), which the BTB path plays (`notes/btb.md`). Nothing of a level is committed: level mod
licences forbid sharing modified versions. Tests over a level only with `RBR_BEAMNG` (`tests/level.rs`: every `.cdae`,
the materials, a conversion into a temporary folder). The default output is `rbr_btb::converted::root()`, the
launcher's player folder plus `converted` (one function for both). Evidence: the Gheo Nordschleife 2026 mod
(2026-11-08).

## Formats

- `.cdae` v31: `u32` version (low byte 31, exporter above bit 16), `u32` header size, MessagePack header map
  (`compression`, `bodysize`), body zstd-compressed when `compression`; the body is Torque3D's `TSShape` as a
  MessagePack stream in BeamNG's documented order. `Point3F`/`Box3F` are MessagePack arrays, the vectors
  `{count, element size, bin}`. 55/55 files of the mod read (`cdae::parse`, ruzstd).
- Meshes are triangle soups (3 vertices per triangle, no sharing); vertices in the node's frame (y up), the node's
  default rotation (`Quat16`, Torque's transposed convention) turns them z up; a `TSStatic`'s position (whole-level
  shapes at `(255, 255, 255)`) places them. Collision is the detail levels of negative size (`Collision-1..12`).
- Faces wind clockwise seen from their front (collision ground faces: `(b-a)x(c-a)` z down); after the stage's axis
  swap the corner order stays (BTB ground faces: cross y up).
- `items.level.json`: one JSON object per line, `__parent` group names; `*.materials.json` and ground models are JSON
  with comments and trailing commas (`json`). Materials match meshes by `mapTo`; `groundType` names a ground model,
  a level's own (`groundmodels/*.json`) resolve through `collisiontype`.
- `race.race.json`: path nodes (`useAsSplit`), segments by `oldId`, start positions, `classification.closed`. The
  Nordschleife mission has 5 nodes; the centre line is the level's drivable `DecalRoad` nearest every node
  (`aiRoad`, 802 nodes, 20792 m as a centripetal Catmull-Rom spline, worst node 2.0 m off).
- Textures: colour maps DXT1/3/5, PNG, DX10 BC1 and BC7 (the road asphalt, graffiti), BC5 normal maps (unused).
  `rbr_formats::dds` reads DX10 BC1/2/3/7 headers, so the converter copies every `.dds` as it is; the renderer
  uploads BC7 natively (`Bc7RgbaUnorm`), CPU paths (foliage, upscale, start board, particles) leave BC7 out or flat,
  and BTB alpha takes BC7 as opaque.
- Opacity: a material's `opacityMap` (first stage) holds its alpha; 48 of 1160 materials have one, 45 of them
  `translucent` or `alphaTest` (trees, bushes, fences, graffiti, dirt edges, kerb lines, the racing groove, tyre
  walls). Opacity maps are BC5 (`BC5U`, the first channel is the opacity) or PNG; their colour maps BC7 (41), DXT5 or
  PNG (4), or none (3: a `baseColorFactor` all over). `cutout` puts the opacity into the colour's alpha and encodes
  DXT5 with mips (texpresso); BC7 decodes with `bcdec_rs` (MIT; the RBR_BEAMNG test decodes every
  `.dds` of the level).
- Forests: `*.forest4.json` (NDJSON `{type, pos, rotationMatrix, scale}`), `*.forest.json` v2 (`instances` by type,
  `pos, quat, scale` rows) and `art/forest/managedItemData.json` (`shapeFile` per type). The Nordschleife's 45 forest
  items name BeamNG's own tree shapes, which the zip does not hold: not drawn (its trees are the `Treez` and
  `Vegetation24` shapes); `--trees-from` leaves them out too (no shape, no height of their own). The level's terrain (`theTerrain.ter`) is not in the zip either; its own ground mesh ends
  about 100 m from the road (2.38 M of its 2.41 M triangles lie within 100 m), so there are no far hills to draw.

## Conversion

- Collada (`dae.rs`): a shape with no `.cdae`, or one not read (v26), reads its `.dae` (Blender's
  `<triangles>`/`<polylist>`, node matrices baked in, `v` flipped, wound clockwise; under `start01` a name's trailing
  number is the detail size, `Colmesh-1` collision). The 2K touge levels (Techeyy: Irohazaka, Akagi, Nagao) and Akina
  (gameproject) ship only `.dae`.
- Races: `--mission` takes a `quickrace/*.race.json` (the 2K levels: `DH`, `UH`), or `road` / `road:reverse`: the
  longest drivable AI road (joined pieces included) end to end (Akina has no race; its one road runs 8.2 km down from
  1130 m). Open roads whose ends lie within 15 m are also tried joined (`course::chained`): Irohazaka's 4 pieces.
  A race without splits gets two at a third and two thirds.
- `Line::stretch` wrapped a run ending exactly at an open road's end back to its start (finish 6709 m on a 5081 m
  Irohazaka course); fixed (2026-11-08).
- Materials: some levels write `"class": "material"` (Irohazaka; its slopes had no texture and drove as tarmac); the
  colour map is the first stage's that has one (Akina's ground: a detail map only in stage 0).
- Akina sets `isRenderEnabled` false on all 1005 statics it draws (no scripts in the mod): `--draw-disabled` draws
  them; `--skip Skybox --skip satview --skip collsision`.
- Forest (`forest.rs`, `*.forest4.json`, `managedItemData.json` `shapeFile`): one `.xform` per kind, detail and band.
  Plants under 3 m only within 30 m, their least detail, to 150 m; trees within `--forest-corridor` (80 m), least
  detail to 500 m, full detail up to 150 m from the eye for those within 15 m. `--forest-budget` (6 M triangles): by
  distance over height, least detail for 60 %, full detail for those by the road, then more least detail. Placed of
  those in the corridor: Irohazaka all 2842, Akagi 7664 of 16358, Nagao 6795 of 26482, Akina 2478 of 43906.
- Autopilot (NGP car 2, `--top-speed 25`, 2026-11-08), at most 1.5 m off the driveline: Irohazaka 1.1 km, Akina
  3.6 km and split 1, Akagi 2.2 km and split 1, Nagao 1.8 km. Frame rate not measured.
- Road re-mesh (`remesh.rs`, on unless `--no-remesh`, 2026-11-08). The touge roads crease under NGP's wheel contact
  (probe of the prepared `.col` 0.75 m off the driveline: median normal change at a triangle crossing, creases over
  1° per km): retail 41/44 0.25°, 54-72; Irohazaka 0.64°, 280; Akagi 0.39°, 84; Nagao 0.40°, 114; Akina 0.15°, 36.
  Irohazaka's slope changes alternate (lag-1 correlation -0.67): a sawtooth of its ~2 m strips, not the road's shape;
  its vertices moved onto a smooth surface still gave 0.58°. The band, up to 7 m either side (the levels' AI road
  widths, 2.5-5 m, are a lane's) until the surface breaks across (15 % plus 5 cm per column), is sampled every 0.25 m
  on 15 columns, each column fitted along (local linear, sigma 2 m), and written as strips (sections within 2 mm and
  5 cm of their chords, at most 3 m apart); level faces under it go, those across its edge keep their corners there
  moved onto it. After: Irohazaka 0.17°, 104; Akagi 0.12°, 8; Nagao 0.16°, 19; Akina 0.06°, 2. Collision triangles
  352k/194k/171k/281k to 262k/127k/135k/233k.
- A collision cell holds at most 21845 faces (its corners are indexed in 16 bits); `beamng-stage` builds the cells
  after converting and warns past that. Irohazaka's fullest (stacked switchbacks, 118 m of height in 128 m) 16688
  faces, 20692 with the re-mesh (21201 with 17 columns).
- Joined road pieces overlap (Irohazaka's by 2-3 m at 2100 m and 3870 m, Nagao's at 1238, 2555 and 4113 m): the line
  turned back on itself 175-180° and the autopilot stalled there; `course::join` leaves out the overlap (2026-11-08).
- Frame `(x - ox, z, y - oy)`, `(ox, oy)` the course's plan centre (680, 264). Collision mesh within 60 m of the
  driveline as `Visible=0`, `CollisionOnly=1` objects (100 m tiles per surface), each surface a material whose `mat.ini`
  map is the RBR id in all cells; drawn shapes as `CollisionModel=-1`, one object per 100 m tile, material and
  `LODOut`: within `--draw-corridor` of the driveline, or a shape's own `--reach TEXT=M`, up to its `--lod TEXT=M` from
  the eye (else `LODOut` 400 m for objects of small triangles). No `.xform` instancing: the level's shapes are
  whole-level triangle soups already placed, and the BTB scene expands every clone anyway (`notes/btb.md` "LOD and
  size"), so clones would shrink only the files.
- Drawn corners weld within 1 mm, 1/256 of a normal and 1/4096 of a texture (the meshes repeat each corner per
  triangle: 18.0 M corners to 8.5 M vertices for 6.0 M triangles); collision corners stay exact.
- Surfaces, by static friction against BeamNG's asphalt (BeamNG's `art/groundmodels.json`: ASPHALT 0.98) and NGP's
  against tarmac fine clean (1.30; NGP's stage materials, `rbr-tyre-lab` `materials`):

  | BeamNG ground model (μs, depth) | ratio | RBR id (NGP μs, soil depth) | ratio |
  | --- | --- | --- | --- |
  | ASPHALT 0.98 | 1 | 37 Tarmac_Fine_Clean 1.30 | 1 |
  | RUMBLE_STRIP 0.96 (kerbs) | 0.98 | 65 Tarmac_Medium_Clean 1.275 | 0.98 |
  | COBBLESTONE 0.93 (Karussell concrete, setts) | 0.95 | 69 Tarmac_Coarse_Clean 1.25 | 0.96 |
  | METAL_TREAD 1.0 (flat) | 1.02 | 37 | 1 |
  | GRASS 0.61, 5 cm; the level's TRACKGRASS 0.61, 3 cm | 0.62 | 113 Grass_Soft 0.70, 3 cm | 0.54 |
  | GRAVEL 0.69; the level's TRACKGRAVEL 0.60, 10 cm; SAND 0.60, 10 cm | 0.61-0.70 | 1 Gravel_Fine_Deeper 0.8725, 2.5 cm | 0.67 |
  | DIRT 0.70, DIRT_DUSTY 0.68 | 0.69-0.71 | 133 Dirt_Hard 0.84 | 0.65 |
  | MUD 0.55, 15 cm | 0.56 | 149 Dirt_Soft 0.76 | 0.58 |
  | none or another model, steep (walls) | - | 64 Metal_Barrier | - |
  | none, flat | - | 37 | - |

  NGP's grass (0.65-0.70) and gravel (0.865-0.90) are its closest of each kind; 53 Tarmac_Fine_Covered (1.235) is a
  hair closer to cobbles by number but is a gravel-strewn surface. Changed from the first version: grass 105 to 113,
  gravel 9 to 1, dirt 141 to 133, a flat metal tread 65 to 37, a steep face of an unknown model (the level's
  `SOFT_COLLISION_GENERAL`) 37 to 64.
- Visual road against the collision: p95 0.004 m, max 0.02 m. Collision 0.70 m vertex spacing along and across.
- Driveline every 2.5 m (the physics takes at most 10000 entries: 2 m gave 10397 and the port failed), heights on
  the collision; a loop's starts 50 m before the start, finish 90 m before the start; notes start/splits/finish/end
  and the pace notes below.
- Nordschleife command (all scenery but the animated shapes and the Ferris wheel): `--skip Ferris --skip _ANIM
  --draw-corridor 60 --reach Nordschleife.cdae=150 --reach /GP=300 --lod RailBack=350 --lod Decals24=250 --lod
  nords_groove=250 --lod Instances24=400 --lod Tyers24=400 --lod Others24=500 --lod Park24=500 --lod Vegetation24=1200
  --lod Treez=1200 --lod GP_Tyres=400 --lod GP_Grass=500 --lod Peoplez=150`: 7.8 M drawn triangles, 28 s; in view at
  the start/Flugplatz/Karussell/Döttinger Höhe 0.70/0.56/0.22/0.84 M (without the `--lod`s 1.24/1.09/0.28/1.62 M;
  the first version 0.40/0.45/0.08/0.46 M), VRAM 9.7 GB at 4K with upscaled textures; spectators (`Peoplez`, 150 m)
  add 2 %.
- Frame rate (`scripts/bench.py --stage 1654149 --car "Ford Fiesta Rally2"`, 4K, every option at its most, the 90 s
  from the start, RX 9060 XT, 2026-11-08): 1 % low 80.9 fps before, 72.6-74.2 after; the bench's own scene 62.4 in the
  same session. Other workers' load put 0.3-2.4 s hitches into both scenes' runs that session.
- Nordschleife: 860233 collision triangles, `col.col` 57 MB in 296 cells, largest 52020 vertices (no 16-bit wrap),
  built in 1.5 s; drawn 2.73 M triangles (road, grass, signs, additions; rails, trees, people, GP left out by
  `--skip`). Map 41's fences sit over 900 m from the driveline with nothing under them.
- Autopilot (Fiesta Rally2, NGP, `--top-speed 50`, `--result-out`): start to finish in 913.8 s race time (3 min 14 s
  of wall time), at most 3.7 m off the driveline; split 1 313.3 s (7017 m), split 2 511.2 s (11584 m).
- Splits: two, the mission's `useAsSplit` nodes nearest a third and two thirds of the stage (`convert::two_splits`;
  all when two or fewer), as every retail stage has: 1.02's race flow times three on the HUD (`RaceFlow` `splits:
  [f32; 3]`) but ends the race with two (`EndRace`), so of the Nordschleife's four the third showed and the fourth
  was never timed.
- Open: the grass ground looks pale and flat; the bench covers only the first 1.3 km (Döttinger Höhe has the most in
  view).
- Retail trees (`--trees-from STAGE --trees TEXT`, `trees.rs`; without `--trees-from` the output is unchanged): the
  retail stage's plants as the lighting finds them (`rbr_render::foliage::plants`: cut-out pieces grouped into trees,
  crowns), those 5 m or taller with stage textures, grouped by textures, triangles and height into kinds (the 16 most
  placed); spacing the median nearest-neighbour distance of its trees. In the `--trees` shapes: an object up to 30 m
  across holding see-through triangles is one tree (`Treez`: trunk, limbs and leaf cards that need not touch), wider
  objects split into connected pieces; a piece 40 m or longer is a tree-line card, retail trees along its foot (corners
  with a corner 1 m or more above within 1 m) at the spacing, as tall as the card there; pieces lower than the
  shortest kind less 20 % stay (bushes), as does everything not see-through (water). Each tree takes a kind within
  20 % of its height (by a seed of its foot), else the nearest, scaled within 20 % and turned by the seed; one `.x`
  per kind and texture, one `.xform` per kind and `LODOut`. Retail 3 to 5 m plants (saplings, a third of 44's) left
  out: as the lowest kind they took the level's bushes for trees.
- Nordschleife with `--trees Treez --trees Vegetation24` (2026-11-08): 44 (Joux Plane) 16 kinds 5.0-16.8 m of its 7366
  trees, spacing 3.9 m, 19916 placed (3712 along 52 cards), 121848 triangles, 7.41 M drawn in all (7.33 M of the
  level's before); 21 (Falstone) 5.2-30.5 m of 3880, 4.4 m, 18742 placed, 125076 triangles. In view at
  start/Flugplatz/Karussell/Döttinger Höhe 0.70/0.54/0.20/0.81 M (without: 0.70/0.56/0.22/0.84 M). 44 is the
  Eifel's mix of conifers and broadleaves, 21 conifers only. Conversion 27 s, 4.2 GB peak; `beamng-stage` 2.8 MB to
  5.1 MB with `rbr-render` linked, and it runs without a display (no GPU or window opened). Frame rate not measured.
- Captures (`viewer --camera EYE,DIRECTION`, the second triple a direction): start, Flugplatz (2.4 km), the
  Karussell approach (11.9 km), Döttinger Höhe (18.1 km), all with 0 missing textures.

## Pace notes

`pacenotes` reads notes from the driveline (2.5 m points with collision heights), between the start and the finish,
in the note types 1.02's co-driver voices (`rbr_gameplay::markers::slot_word_id`); `pacenotes.ini` keeps them with
the start, split, finish and end keys, sorted by distance.

- Corners: runs where the plan heading's rate over ±10 m is at least 1/400 m⁻¹ on one side, turning at least 15°; the
  note sits where the run starts, graded by its tightest radius (left/right tag):

  | Radius, m (at most) | 15 | 25 | 40 | 70 | 120 | 200 | 400 |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | Note | hairpin (0/11) | 90 (1/10) | K (2/9) | medium (3/8) | fast (4/7) | easy (5/6) | flat (26/25) |

- Flags: `long` (0x400) past the 90s for a corner turning 100° or more, or 200 m long; `tightens` (4) when the
  tightest point lies in the last third and the first third's tightest radius is 1.6 times it or more.
- Crests and jumps: convex runs of the height profile (slopes over ±15 m) tighter than 500 m, runs closer than 30 m
  one crest, at its tightest point: over crest (16), a jump (20) at 400 m or less (a car leaves the ground over a
  radius `R` above `sqrt(g R)`: 63 m/s for 400 m). Dips and bumps are not called.
- A distance key (13) 0.1 m after a note with 30 m or more to the next one: a co-driver mod says the distance to the
  next call from it (`rbr_sound::custom_codriver`); 1.02's co-driver ignores it (our payload's distance is 0).
- Nordschleife: 95 notes (85 corners, 8 crests, 2 jumps near 15.7 and 16.9 km; no hairpins) and 92 distance keys; the
  Karussell (11.9 km) is a K left, long. Heard: the replay of the 90 s bench drive with the user's co-driver mod
  (Janne Laahanen) rendered to video (`viewer --video`), the co-driver alone (`--volumes 0,1,0,0`): the countdown,
  then 10 calls about 150 m apart over the first 1.4 km, each with a distance.
