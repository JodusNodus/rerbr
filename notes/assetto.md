# Assetto Corsa conversion

- Input is the player's installed track folder. KN5 v5/v6 static meshes, AI v7, point-to-point timing gates;
  output is the shared `RX_CONTENT/Tracks/<name>` BTB format.
- KN5 format references: [AC_to_SVJ](https://github.com/RFloEng/AC_to_SVJ/blob/main/kn5_reader.py),
  [AcTools reader](https://github.com/gro-ove/actools/blob/master/AcTools/Kn5File/Kn5Basic.cs),
  [AcTools node fields](https://github.com/gro-ove/actools/blob/master/AcTools/Kn5File/Kn5Node.cs).
  The reader here is original code. Materials store blend and alpha-test as separate bytes; textures carry a
  length after the active flag even when inactive. The car exporter discards mesh flags; this reader retains them.
- AC's frame is right handed, BTB's mesh frame left handed (Direct3D). KN5 `(x,y,z)` becomes mesh `(x,y,-z)` and
  driveline/collision `(x,-z,y)`, a rotation about x, with the triangle order reversed ([Handedness](#handedness-2026-11-16)).
  Composed row-vector transforms are baked, normals use the inverse transpose, and a mirrored node transform reverses
  triangle winding.
  CréQuy's road cross products face up in the KN5 frame, matching the BTB mesh frame.
- AI points: 16-byte header, then 20-byte `(x,y,z,distance,id)` records. Distances are recomputed from positions.
  Timing gate projection uses plan coordinates because the gate transforms can be below the road.
- CréQuy: 8537/10031 AI points in 2024/2025. Timing gates give approximately 12.314/14.991 km; the AI polyline
  includes run-up and run-off. Each layout names 8 KN5s; all 11 root files have ordinary v5/v6 structures.
- Physical mesh names have a nonzero integer followed by a surface token: `1ROAD230`, `1GRASS0`, `1WALL...`.
  Year-prefixed scenery such as `2025_OBJ43` does not qualify. Missing surface definitions use the token's
  explicit fallback class; RBR surface ids come from `../data/physics-lsp-fields.md`.
- CréQuy's `objects.kn5` has X8R8G8B8 DDS textures without an alpha mask, such as `Blank_20_20_22.dds`.
  Convert these from their channel masks to RGBA8; an unused fourth byte is not alpha.
- Base diffuse and alpha are retained. `ksMultilayer*` materials are baked into one texture
  ([Multilayer](#multilayer-and-placeholder-textures-2026-11-16)), drawn per pixel by ReRBR's renderer where it reads `ReRbrLayer*`
  ([Multilayer at render time](#multilayer-at-render-time-2026-11-19)), else world-space ones per drawn tile, and `useDetail`
  materials with their detail ([Fidelity](#fidelity-2026-11-17)). Normal and reflection shading, lighting factors, CSP
  scripts and runtime model replacements remain unsupported. Conversion reports list simplified shader kinds and missing
  textures.

## Verified on CréQuy 2025, 2026-11-12

- 13,773,449 drawn triangles, 1,196,193 collision triangles, 2907 textures, no missing textures;
  all 6034 driveline samples have ground. Collision has 487 cells; largest 50,595 of 65,536 allowed vertices.
- `road-probe`, source collision: no missing surface on either wheel track; stacked 5.05/4.31%, median normal
  change 0.02°, p99 height step 0.5 mm. Smoothing remains optional; the source road completes the drive.
- Headless autopilot, NGP slot 2, top speed 25 m/s: `finished=1`, `drivable=1`, `cause=Marker`, 784.793 s;
  both splits recorded, maximum 1.3 m off the driveline. `/tmp/ac-stage-2025-full-drive/` holds the run.
- ReRBR render with Ford Fiesta Rally2: zero missing textures and bad triangles; start area, road, scenery and
  car placement inspected. Capture `/tmp/crequy-2025.png`, log `/tmp/ac-stage-render.log`.
- Format, clippy and 13 tests pass; the optional source test reads all 11 root KN5s and both layouts.

## CréQuy 2024

- 11,715,330 drawn triangles, 1,004,027 collision triangles, 2827 textures, no missing textures;
  all 4963 driveline samples have ground. Collision has 393 cells; largest 60,798 of 65,536 allowed vertices.
- `road-probe`, source collision: no missing surface on either wheel track; stacked 5.31/5.28%, median normal
  change 0.02°, p99 height step 0.5 mm.
- Full timed route, NGP slot 2, top speed 25 m/s: `finished=1`, `drivable=1`, 668.9072 s, both splits recorded,
  maximum 1.7 m off the driveline. `/tmp/ac-stage-2024-full-drive/` holds the run.
- The initial result marker 30 m after the finish lay beyond the autopilot's braking stop. Match the BeamNG converter's
  15 m run-off for both layouts. A targeted drive over 2024's final 300 m gives `finished=1`, `cause=Marker`, end
  page `0x10` (`/tmp/ac-stage-2024-finish-drive/`). Timed gates and geometry are unchanged.
- Installed layouts: `Crequy_2024` and `Crequy_2025` under the player's converted BTB root, with launcher ids
  1976965 and 1199346. Source track files and generated assets remain outside this repository.

## Other installed JCRR stages

- Single-layout tracks use `models.ini` or their same-named KN5; source filenames match case insensitively.
  Joux Verte's layout names use `SS` while some model lists use `ss`. Inactive `AC_AB_*` helpers still define
  timing gates, despite their hidden debug meshes. Puech Arnal has zero-sized sector placeholders.
- The requested batch excludes layouts without their own AI file: Mandagout–Peyregrosse and Peyregrosse freeroam.
  Forward Peyregrosse–Mandagout's AI begins about 20 m past the gate; a short aligned extension covers it,
  with every added point checked against physical road geometry.
- Mont Ventoux has 12 faces touching invalid positions and 14,867 invalid normals; Puech Arnal has 20 invalid
  faces, 72 invalid UV pairs and two invalid normals. Omit only faces touching invalid positions, replace bad UV
  components with zero and rebuild bad normals from valid faces. Reports record each repair; synthetic tests
  cover these cases.
- Joux Verte SS7 2020 and Thizy exceed the BTB collision-cell index limit with source collision over 75 m.
  The existing road re-mesher and a 25 m collision corridor produce cells within the limit.
- Puech Arnal's final AI record runs 13 m backwards over the preceding road. Retaining it selects the return
  leg as the finish and makes the controller attempt a false U-turn. Omit only a terminal return over 150°
  whose point falls within 0.5 m of the preceding 30 m in 3D. The first-pass finish gives 3.474 km; no other
  batch layout triggers this repair. A synthetic test preserves a nearby separate hairpin.

### Batch verification and installation, 2026-11-12

All nine layouts have ground at every driveline sample, no missing wheel-track surface in `road-probe`, and
no missing textures. Start-area renders of all nine were inspected; the renderer reports zero missing textures
and bad triangles. Shots are under `/tmp/ac-jcrr-batch/screenshots/`.

Full headless checks use NGP slot 2 and a 25 m/s top speed, with the sweep's selected weather/tyres:

| Layout | Timed km | Complete | Time, s | Maximum offset, m |
| --- | ---: | --- | ---: | ---: |
| Joux Verte SS1 2024 | 12.517 | yes | 778.6317 | 1.4 |
| Joux Verte SS7 2020 | 20.208 | yes | 1479.139 | 1.5 |
| Joux Verte SS7 2024 | 22.428 | yes | 1339.163 | 1.5 |
| Mont Ventoux | 9.987 | yes | 532.085 | 1.4 |
| Muracciole–Vezzani | 10.504 | yes | 709.2274 | 2.3 |
| Porto–Piana | 9.520 | no | — | 16.7 |
| Puech Arnal | 3.474 | yes | 304.52487 | 1.3 |
| Peyregrosse–Mandagout | 13.267 | no | — | 21.0 |
| Thizy | 18.737 | yes | 1033.6686 | 1.3 |

Porto–Piana and Peyregrosse–Mandagout leave the road near driveline 8.34 km and 11.17 km respectively. Isolated
section tests at 15 and 8 m/s in dry conditions also fail there; smoothing collision does not resolve it. Their
source collision is retained, and they are marked as needing manual verification. Clear road probes alone do
not establish that the controller or a human driver can complete those stages.

The nine layouts (120.641 km total) are installed under the player's `converted/RX_CONTENT/Tracks/`, with
`AC_` folder prefixes. ReRBR lists all nine as startable. A receipt with source AI paths, launcher ids and drive
results is at `~/.local/share/rerbr/converted/jcrr-conversion-2026-11-12.json`. No source assets were modified.

A fresh CréQuy 2025 conversion retains its draw/collision counts and clear road probes. Its regression drive
finishes with `cause=Marker`, 784.7832 s and maximum offset 1.3 m; the render is clean. Neither CréQuy layout
triggers terminal AI trimming. Format, clippy and 13 tests pass, including all 11 local KN5s and both layouts.

## Combined converter and start positions, 2026-11-12

- The source reader now lives at `crates/rbr-ac-tracks/src/assetto`; geometry, texture decoding, mesh writing, road smoothing,
  pacenotes and stage preparation are shared in `crates/rbr-ac-tracks/src/btb`. The command is `rbr-ac-tracks ac`.
- The earlier start captures were stationary: the live log shows 0.0 m/s. The converter had placed the car's
  centre at the AB timing gate. Use `AC_START_0` when it lies 2–50 m behind the gate and within 5 m of the line,
  with height sampled from physical road collision. Otherwise sample the driveline five metres before the gate,
  extrapolating its tangent if the source recording has no lead-in. Reject an ungrounded spawn.
- Of the 11 installed AC layouts, nine use their grid helper and two use the fallback. CréQuy 2024/2025 start
  2.23/5.60 m behind the gate. Start positions and provenance were updated in the installed folders; geometry,
  surface maps, driveline and timing files were preserved. Backups are under the player's `backups` folder.
- All 11 pass departure checks with no false start. CréQuy 2025 completes its full drive in 784.9348 s with
  `cause=Marker`, maximum offset 1.3 m and `false_start=0`. Its probes still have no missing wheel-track surface.
- A fresh CréQuy 2025 export matches the previous converter byte for byte except `spawn.ini` and the conversion
  report. Captures `/tmp/crequy-2025-start-fixed.png` and `/tmp/crequy-2025-driving.png` show the corrected grid
  position and the car further down the stage; both render with zero missing textures and bad triangles.
- The combined check suite has 50 tests, including the local CréQuy source test, CLI routes, the native grid helper
  and a missing-helper fallback. The two late-route autopilot failures documented above remain unverified.

## Riudecanyes and Turini Gallego, 2026-11-12

- Converted only `rallycatalunya_riudecanyes_2/riudecanyes` and the root layout of `afm_turini_gallego`, each
  using its own fast-lane AI. The full Riudecanyes timed route is 16.190728 km; Turini Gallego is 3.684917 km.
- Riudecanyes uses zero-padded collision names such as `01TARMAC` and `01 WALL gr`. Accept nonzero numeric
  prefixes with leading zeros and optional spaces, while retaining the exclusion of year-prefixed scenery with
  an underscore. The synthetic stage fixture now exercises a zero-padded physical mesh.
- Both recordings end at their finish gates. Extend the final direction over checked physical ground to leave
  30 m of run-off, with the result marker 15 m after the finish. Reject insufficient grounded run-off. A unit
  test checks that timing distances remain unchanged and that missing ground prevents extension.
- Source collision is retained without remeshing. Riudecanyes has 4,697,671 drawn triangles, 902,066 collision
  triangles and 41 textures. Turini has 344,679 drawn triangles, 152,924 collision triangles and 39 textures.
  Both have ground at every driveline sample, no missing wheel-track surface and no missing textures.
- Full dry headless drives, NGP slot 2 with a 25 m/s top speed, reach `finished=1`, `cause=Marker` and
  `false_start=0`. Riudecanyes completes in 1103.6553 s with maximum offset 1.5 m; Turini in 259.87253 s with
  maximum offset 1.3 m. Both use the fallback grid position about five metres behind the timing gate.
- Start and driving captures were inspected, with zero missing textures and bad triangles. Persistent shots
  are in `~/.local/share/rerbr/converted/screenshots/riudecanyes-turini-2026-11-12/`. AC detail shaders still
  use their base diffuse texture.
- Installed folders are `AC_Riudecanyes_2022` and `AC_Turini_Gallego`, with launcher ids 1475091 and 1452715.
  Both were favourited, preserving all 55 existing favourites for 57 total. The conversion and validation
  receipt is `~/.local/share/rerbr/converted/ac-riudecanyes-turini-2026-11-12.json`.
- Format, clippy with warnings denied and the release test suite pass. ReRBR and source assets were unchanged.

## Surfaces by declared friction, 2026-11-12

- Before: a surface's name alone chose its RBR id, so every road was 37 (NGP static 1.30) whatever `surfaces.ini`
  declared (CréQuy 0.92/0.94, Mont Ventoux 0.95, Turini 0.98), and names of a track's own fell back to 37
  (Riudecanyes' BERM, CUT and STUCK at 0.60 with `DIRT_ADDITIVE`: 139k verge faces as grippy as the road).
- Now (`material::Surfaces::id`): the name gives the class, and `FRICTION / 0.98` (clean racing asphalt, capped at 1)
  picks the id of the class whose NGP 7.4.778 static friction over 37's is nearest (`rbr-tyre-lab` `materials`):
  tarmac 37 1, 65 0.981, 45 0.975, 69 0.962, 73 0.956, 53 0.950, 77 0.938, 81 0.932, 85 0.913; gravel 17/9/1
  0.69-0.67; dirt 133/141/149 0.65-0.58; grass 113/105/97 0.54-0.50. No RBR surface lies between 0.91 and 0.69.
- A name of the track's own on level ground is tarmac, or dirt where it has `DIRT_ADDITIVE` and under 0.9 of
  asphalt. A key without a `FRICTION` keeps its class's former id; walls (64), ICE (117) and `--surface` are as before.
- Reconverted and installed 2026-11-12 (collision and surface files only; drivelines, spawns and notes kept; the
  former files under the player's `backups/ac-surface-friction-20261112`): roads CréQuy 2024 77, 2025 69, Mont
  Ventoux and the three Joux Verte 45, Riudecanyes TARMAC 65, TARMAD 53, BERM/CUT/STUCK 141, Turini ROAD 37, LISA 45,
  TURBIA 65, BORDE 85; Thizy, Porto-Piana and Muracciole stay 37. Peyregrosse-Mandagout was left as installed: a
  fresh conversion differs from it in driveline and collision. Riudecanyes' `KRB` (363k faces) has no entry in
  `surfaces.ini` and stays 37.
- Headless dry drives, slot 2 at 25 m/s, all `finished=1`, `cause=Marker`, `false_start=0`: CréQuy 2025 786.1 s
  (784.9 before), CréQuy 2024 669.8 s, Mont Ventoux 533.2 s (532.1), Riudecanyes 1104.1 s (1103.7), Turini 259.9 s
  (259.9). Thizy and Joux Verte SS7 2020 need `--remesh --collision-corridor 25`. Not driven: the other six.

## Invisible walls, 2026-11-13

- AC hides an inactive mesh (`Active=0`) but still collides with it. JCRR keeps most of its invisible walls that
  way: Créquy 2025 has 53 active `1WALL` meshes and 2868 inactive ones (12,554 triangles, two to four each, around
  buildings and trees); Thizy 13 and 3287; Porto–Piana 104 and 30. Skipping inactive meshes left the car free to
  drive through that scenery. Physical names now collide whatever their active flag; drawing still needs it.
  Meshes under `AC_` helpers stay excluded.
- Scenery without a physical name has no collision in AC either, and gets none here.
- RBR tests a collision face from its front only (`rbr-physics` `collision/triangle.rs`: the segment test needs a
  positive determinant, the point test a point behind the face by at most 0.5 m). AC wall sheets are wound either
  way. `walls::face_road` turns a steep face within 20 m of the driveline that shows the road its back, unless a
  front face stands before it (the far side of a box, a twin sheet) or it lies wholly below the road (an
  embankment side). A doubled sheet is not an option: the point test of the copy would hold a car that is within
  0.5 m in front of the original.
- Créquy 2025: 1,207,205 collision triangles (was 1,196,193; walls 18,245 to 29,257), 357 faces turned. 487
  cells, largest 51,957 vertices. `road-probe` as before: no missing surface, stacked 5.05/4.31 %.
- Twelve installed layouts were reconverted with this change merged with the unmerged `ac-surface-friction`
  branch, which the installed stages already came from. Drawn meshes, textures and `materials.ini` are byte
  identical to the installed ones, so no new render was taken. Only `Objects.zip`, `objectlist.ini`, `mat.ini`
  and the counts in `ac-conversion.json` were replaced; drivelines, pacenotes, spawns and `track.ini` stay.
  Backups are in the player's `backups/ac-walls-20261113`.

  | Layout | Wall triangles | Faces turned | Drive, s |
  | --- | ---: | ---: | ---: |
  | Créquy 2024 | 17,608 to 27,882 | 312 | 670.784 |
  | Créquy 2025 | 18,245 to 29,257 | 357 | 787.007 |
  | Joux Verte SS1 2024 | 7600 to 22,448 | 113 | 778.876 |
  | Joux Verte SS7 2020 | 7882 to 27,611 | 137 | 1479.778 |
  | Joux Verte SS7 2024 | 8604 to 31,216 | 112 | 1339.244 |
  | Mont Ventoux | 5050 to 5074 | 476 | 533.208 |
  | Muracciole–Vezzani | 2750 to 9530 | 131 | 709.425 |
  | Porto–Piana | 40,682 to 45,054 | 0 | leaves at 8.36 km, as before |
  | Puech Arnal | 87,225 | 64 | 250.105 |
  | Riudecanyes 2022 | 13,769 | 0 | 1365.030 |
  | Thizy | 3810 to 14,742 | 840 | 1034.858 |
  | Turini Gallego | 3542 | 11 | 259.830 |

  The eleven that finished before finish again (NGP slot 2, 25 m/s), at most 2.3 m off the driveline, no false
  start: no added wall stands on the road. No car has been driven into a wall headless; that check is the
  player's.
- Peyregrosse–Mandagout was left as installed: its files predate the friction update and a reconversion does
  not reproduce them (collision 828,881 against 1,144,195 triangles, other surface ids). Its 154,452 wall
  triangles are all active; 69 faces would turn.

## Catalunya, Mulholland and Ebisu, 2026-11-16

- Converted from the local install, each with its own fast-lane AI and no remeshing: Catalunya's `riuold`,
  `duesaigues`, `duesold`, `collpower` and `collshake` (Riudecanyes 2022 was already installed), and Mulholland
  `east` and `west`. Mulholland `freeroam` has no `AC_AB_*` markers and is skipped. Installed names are
  `AC_Riudecanyes_Classic`, `AC_Duesaigues_2016`, `AC_Duesaigues_Classic`, `AC_Coll_de_la_Teixeta_2011/2022`,
  `AC_Mulholland_East` and `AC_Mulholland_West`.
- All seven have no missing textures, collision cells within the limit (largest 64,299 of 65,536, Riudecanyes
  Classic), no missing wheel-track surface in `probe`, and finish a full dry NGP slot 2 drive at 25 m/s with
  `false_start=0`; maximum offset from the driveline is 1.4-3.8 m (Mulholland east worst). Collshake stops 0.3 m
  (dry) to 1.7 m (wet) short of the result marker, so its end cause is `Left` with the finish recorded; the other six reach
  `Marker`. Start and mid-stage renders show zero missing textures and bad triangles. Banners seen from behind show
  mirrored text; it is the back face and also appears in the earlier Riudecanyes 2022 stage. (Wrong: the whole stage was
  mirrored, see [Handedness](#handedness-2026-11-16).)
- Ebisu Circuit Touge (`ebisu_circuit_touge_course`) is a closed 0.91 km loop with no timing, grid or sector
  markers, and its KN5 ends with 47 MB of CSP vertex-AO records. The reader no longer rejects data after the node
  tree. `--circuit` makes the AI loop a ReRBR circuit (see `docs/assetto.md`; first converted with the earlier `--lap`, which invented gates), installed as `AC_Ebisu_Touge`.
- Ebisu's KN5 holds 124 one-pixel PNG stubs in place of textures, so its scenery renders as flat blue. Its road
  collision is rough (`probe`: crease median 2.7 degrees, p99 step 60 mm; `--remesh` gives 0.3 degrees but
  keeps 36-40 mm steps). The drive finishes the lap in 79 s but stalls in the hairpin just before the line and
  never reaches the result marker (`cause=Left`). Treat it as a geometry-only conversion.
- Gunma Cycle Sports Center (`pk_gunma_cycle_sports_center`, layout `gcsc_full_attack`) has AB gates 4.4 km apart, but its
  AI line is a closed 5.9 km loop. Converted with `--circuit` as `AC_Gunma_Gunsai_Touge`: a ReRBR circuit whose lap line and
  spawn lie at `AC_AB_START`. The layout has no `data/surfaces.ini`; the one from `gcsc_hot_version_attack` (same physical
  meshes) was copied into a symlinked temporary track folder. `scripts/btb-autopilot.py --laps 2` drives two laps, 318.4 s and
  316.8 s, through the seam (`cause=Format`, max 1.4 m off the driveline, no false start). The layout's own gates and
  countdown placement were not compared.
- Gunma's grass (`ksMultilayer`, 47,639 triangles) first rendered as flat white-grey patches on the verges although the report
  listed no missing textures: its mask is an uncompressed A8R8G8B8 DDS, `textures::decode` could not read it, the bake
  returned `None` and the near-white diffuse was kept (`base_diffuse_only`). `decode` now falls back to `legacy_dds`
  (moved from `material.rs` to `btb/textures.rs`); both multilayer shaders bake and the verges are grass. Earlier
  stages whose multilayer masks are legacy DDS fell back the same way and are worth reconverting; the report's
  `base_diffuse_only` lists them. Reconverted with the fix (2026-11-16, same options, identical triangle counts, timing and
  spawn; old folders in `~/.local/share/rerbr/backups/pre-multilayer-decode-fix-2026-11-16/`): Coll de la Teixeta 2011 and
  2022, Duesaigues 2016 and Classic, Riudecanyes 2022 and Classic. Thizy, the three Joux Verte, Mont Ventoux, Puech
  Arnal, both CréQuy and Peyregrosse–Mandagout still keep the base diffuse for a multilayer ground: its diffuse is
  8192 px, over the 4096 px bake limit.

## Glava Zete, Roure–Beuil, Sa Calobra, St Jean–St Laurent, La Bollène, 2026-11-16

- Nine layouts converted with their own AI and no remeshing: `glava-zete/glavazete`, `ss_roure/rbcl` and `rbdo`,
  `ss_sacalobra/down` and `up`, `ss_stlaurent/sjsl` and `slsj`, `ss_bollene_1.1/original` and `reverse`. Sa Calobra
  `freeroam` has no timing gates and is skipped. Installed as `AC_Glava_Zete`, `AC_Roure_Beuil_Uphill/Downhill`,
  `AC_Sa_Calobra_Downhill/Hillclimb`, `AC_St_Jean_St_Laurent`, `AC_St_Laurent_St_Jean` and
  `AC_La_Bollene_Original/Reverse`.
- No missing textures; largest collision cell 65,475 of 65,536 (La Bollène); no missing wheel-track surface in `probe`.
  Full dry drives (NGP slot 2, 25 m/s) finish with `false_start=0` and at most 2.7 m off the driveline. Seven
  end at the result marker; Roure–Beuil uphill and Sa Calobra hillclimb stop just short of it (`cause=Left`, finish recorded).

## Handedness, 2026-11-16

Every AC conversion until now was a mirror image. The converter turned KN5 `(x,y,z)` into `(x,z,y)`, a swap, which
changes handedness; AC's frame is right handed with z pointing south. Four independent checks:

- Official tracks' AI lines run anticlockwise in the `(x,z)` plane for the clockwise circuits (Monza, Silverstone,
  Nordschleife, Mugello, Red Bull Ring, Zandvoort, Barcelona, Vallelunga) and clockwise for Imola.
- `AC_START_0`'s x axis lies 90 degrees clockwise of its z (forward) axis on Imola, Monza and Mont Ventoux. A car's
  x is its left: `DRIVEREYES` x is positive for left-hand drive cars (`dthwsh_mazda_rx7_fd_bn_lhd`) and negative for
  right-hand drive ones, with front at +z. That frame is right handed.
- Sa Calobra Downhill's driveline against OpenStreetMap's MA-2141: shape residual 375 m before, 78 m after. Mont
  Ventoux's start-to-summit vector had its north component flipped.
- Text on banners and signs read mirrored (Mulholland's START gantry, WRC and SuperStage boards, Roure's "alpes") and
  read correctly after the fix.

`ai::world` now maps `(x,y,z)` to `(x,-z,y)`, a proper rotation (north is +y), the corners of each triangle are
taken in reverse order and the normals follow. A reflected scene drawn left handed with that winding is exactly AC's
own picture. `Triangle::normal` still points away from the viewer, so `walls::face_road` and the road re-mesher
are unchanged. The synthetic fixture now faces -z in the stage (`angleZ` = pi). Pacenote sides come from the corrected
driveline. Glava Zete's feather banners still read mirrored; their orientation is the source's (not otherwise checked).

Verified on a fresh export of Sa Calobra Downhill/Hillclimb and Glava Zete: `probe` clear, headless drives finish
(`cause=Marker`, `false_start=0`; 589.1 s, 757.8 s and 384.6 s, at most 2.4 m off). Installed stages made before this
date are still mirrored until they are converted again.

## Multilayer and placeholder textures, 2026-11-16

- `ksMultilayer`, `_fresnel_nm`, `_objsp` and `_objsp_nm4` shade `diffuse * (R*mask.r + G*mask.g + B*mask.b +
  A*mask.a) * magicMult` (read from the compiled `system/shaders/win/*_ps.fxo`). The diffuse is a macro tint, near white
  on Glava Zete's terrain (`teren.dds` averages 0.89), so using it alone gave white ground. Detail texels sit at
  `uv * mult` for `_objsp` (a per-axis pair, stored after the property's scalar) and at the world `(x,z) * mult` for the
  others. `multilayer::bake` writes the sum at the larger of the diffuse and mask sizes, as DXT1. World-space detail is
  baked with the mesh's average metres per UV unit (right scale, not exact position), and detail finer than the
  baked pixel becomes its average. Materials whose textures exceed 4096 px (Mont Ventoux's, CréQuy's 109 MB
  `BackgroundMap`) keep the base diffuse.
- Ebisu's KN5 textures are all the same 70-byte one-pixel PNG, a translucent blue (0,0,255,127), so its scenery
  rendered as overlapping blue glass. A one-pixel PNG diffuse is now a placeholder: an opaque colour from the material
  name (road, grass, soil/cliff, tyre, glass, else grey), and materials that AC draws with alpha (leaves, decals, glass) are
  not drawn, as their texture holds no alpha to cut with. Collision is unchanged. Its road mesh itself is rough.

### Reinstall, 2026-11-16

All 30 installed AC layouts were converted again with the handedness and multilayer changes (options from each
`ac-conversion.json`: `--remesh --collision-corridor 25` for Thizy and Joux Verte SS7 2020, `--circuit` for Ebisu). Drawn and
collision counts match the earlier installs, and spawn sources are unchanged. Headless drives (NGP slot 2, 25 m/s,
dry) finish with `false_start=0`, at most 3.8 m off (Mulholland East), at times within seconds of the earlier ones.
Porto–Piana and Peyregrosse–Mandagout still leave the road (22.5 m and 19.9 m off, `finished=0`), as before. La Bollène
Original now stops 16 m short of the result marker (`cause=Left`, finish recorded) and takes 932 s against 750 s; the
mirrored road is otherwise identical, and the cause was not investigated. Peyregrosse's reconversion now gives
1,144,195 collision triangles, the figure the earlier note expected. Previous folders are in the player's
`backups/ac-handedness-20261116`.

## Import interface, 2026-11-16

- `import::discover`/`layouts` list a `content/tracks` folder's layouts from file names alone (`models_<layout>.ini`,
  else a single layout `.`); a layout lacking `ai/fast_lane.ai` or `data/surfaces.ini` is `Unsupported` (299 layouts
  listed on the local install, nothing read). `import::import` converts one layout and reports through `btb::control::Control`
  (log lines, a cancel flag checked per model and per mesh; a cancelled import leaves no stage). Choices made for the player,
  recorded in `Applied`: a missing `AC_AB_*` gate with a closed AI loop becomes a circuit; a collision cell over the limit
  retries once with `--remesh` and a 25 m collision corridor.
- Thizy through the CLI (`rbr-ac-tracks import <tracks> --track thizy`): first attempt overflows a cell, the retry gives
  509 cells, largest 34,446 vertices, 881,912 collision and 11,158,896 drawn triangles, 18.737 km, spawn from `AC_START_0`;
  73 s in all including the failed attempt. The stage was not driven or rendered.
- `Input::read_with_ai` still prints its two source warnings (a trimmed AI point, an ignored sector) to stderr.
- `make check` (the old repository's) with `--locked` failed before any of this: `Cargo.lock` lacks `windows 0.62.2`, which ReRBR's
  `rbr-school` now needs. The checks above ran with `--offline`.

### Import speed, 2026-11-16

- Mont Ventoux (4 KN5s, 16.1 M drawn triangles) spent about half its 25 s writing `Objects.zip`: each mesh was serialised and
  deflated one after another. `Objects::meshes` now builds and deflates a model's meshes on every core and copies the finished
  one-entry archives into `Objects.zip` (`raw_copy_file`), in the same order. Mont Ventoux 26 to 13 s, Thizy 56 to 36 s
  (both including Thizy's failed first attempt); `Objects.zip` entries, every other file and the size are identical to the
  previous build (compared by entry name and bytes; `ac-conversion.json` not compared). CPU time is unchanged.
- Left: a layout that overflows a collision cell is converted twice (Thizy about half of its 36 s), and the remaining serial
  time is the first model's multilayer bake (3 s on Mont Ventoux), the triangle pass (2 s for the largest KN5) and ReRBR's
  cell build (3 s). Timings on this machine vary with other load; the figures are the quietest of two runs.

### Pop-in near the camera, 2026-11-17

- ReRBR draws a BTB mesh by the eye's distance to the centre of the box of everything it merged into it (placements whose box
  centres share a 128 m cell and a `LODIn`/`LODOut` band; a box over 150 m half extent is alone and ReRBR adds its radius).
  AC measures from each object. The converter's tiles were 100 m on the stage's absolute coordinates, so ReRBR merged up to
  four of them and a fence at the edge of a merged box sat well over its source range from the box centre: it appeared only
  inside that circle, close to the car. Riudecanyes 2022 as installed: of 1154 meshes with a source range, 210 were guaranteed
  under 100 m (40 under 50 m, the worst 28 m) although the shortest source range is 150 m.
- Drawn tiles are now ReRBR's 128 m cells from the stage origin (collision tiles are unchanged), and a mesh whose source gives a
  range has `LODOut` raised, `LODIn` lowered, by the half diagonal of the box ReRBR will merge it into (`lod_padding`).
  Meshes without a source range keep the 1000 m cap unpadded. Reconverted: 926 meshes with a range, guaranteed range at least
  151 m, none under 100 m; visible objects 5094 to 3609 (fewer, fuller tiles), same drawn and collision triangles, `Objects.zip`
  the same size. Measured by emulating ReRBR's grouping over the converted stage's scene; not seen in a render or driven.
- Installed stages keep the old tiles until converted again. Meshes of different KN5s merge in ReRBR but are padded
  separately, so a rare overlap can be a little short.

## Fidelity, 2026-11-17

ReRBR draws a BTB material's diffuse only, so AC detail has to be in that texture. Views came from ReRBR's viewer at
driveline points (`scripts/btb-shots.py`), against the previous build's stage.

- `useDetail` 1 in `ksPerPixelMultiMap` and `_NMDetail` shades `diffuse * lerp(detail(uv * detailUVMultiplier), 1,
  diffuse.a)` on all four channels (the compiled `_ps.fxo` of both). `detail::bake` writes one repeat of the diffuse at the
  detail's resolution, at most 4096 px. The diffuse's alpha is the detail mask there: an opaque DXT1 encode turned texels
  under half alpha into transparent black (Glava Zete's guardrails and walls were black), so `textures::encode` now
  ignores alpha for DXT1.
- World-space multilayer ground baked once per material had a texel of metres on Riudecanyes' road: a grey smear. It is
  now baked per drawn tile (`multilayer::bake_tile`): each texel takes the source UV for the diffuse and mask and the world
  position for the details, at `0.03 + 0.003 d` metres (`d` the mean distance from the driveline; first `0.04 + 0.008 d`,
  which left verges 15 m off flat pastel where the screen resolves 1.5 cm), within 4 M texels and 8192 px a tile. A
  tile keeps the material's single bake when that is already as fine (coarser texel side of `metres_per_uv` over the
  diffuse and mask size): road diffuses repeated along the road at 4-17 mm a texel (La Bollène, Sa Calobra, Glava Zete)
  had turned blurry and blotchy at 4 cm.
- Laid out in the source UVs the road still streaked toward the horizon: Riudecanyes' road UVs are sheared (the world
  directions of u and v at cos -0.87) and islands differ threefold in scale, so a square UV texel is a long sliver on the
  road. Each UV island is now mapped to metres by the affine map that makes its average texel square (faces whose UVs
  nearly collapse are left out of the average; an island laid out at the wrong area takes the plane of its faces), turned
  to its principal axis, cut while its faces fill under 40 % of its box, and shelf-packed into one texture per tile. Seam
  vertices are duplicated. The road is isotropic at 1.2 and 10.3 km, and the speckled grain matches AC's.
- Riudecanyes `Textures.zip` 62 to 416 MB and import 5.4 to 10.7 s; Mont Ventoux 890 MB to 1.1 GB, 14.4 to 15.3 s; Glava
  Zete 104 to 293 MB; Sa Calobra down 221 to 197 MB. Riudecanyes probes as before and autopilot finishes (1103.6 s,
  `false_start=0`). Sa Calobra, Glava Zete and Mont Ventoux were rendered, not driven.
- Left as AC has them: cliff faces stretched at their base by their own UVs, and world `(x,z)` multilayer detail smeared
  down steep verges. Per-material `ksAmbient`/`ksDiffuse` are the `ReRbrLight` factors (below).
- Reinstall, 2026-11-18: the 30 installed AC stages other than Ebisu were converted again with the options from their
  `ac-conversion.json` (`--remesh --collision-corridor 25` where remeshed, `--circuit` for Gunma); previous folders are in
  the player's `backups/pre-fidelity-20261118`. Installed AC stages 16 to 25 GB. Ebisu was left: its install predates
  `--circuit` and has no timing gates, and its textures are stubs. Glava Zete (384.6 s) and Sa Calobra downhill (725.3 s)
  finish headless with `false_start=0`, at most 2.3 m off the driveline; the others were not driven.
- Checked, 2026-11-18 (the `1c8b92b` install): every installed AC stage but Ebisu rendered at three driveline points, and driven
  headless (NGP slot 2, 25 m/s, dry): 28 runs without a false start; 25 finish. Porto–Piana (16.2 m off) and
  Peyregrosse–Mandagout (5.0 m) leave the road as before; Gunma, driven three laps rather than the two of its earlier check,
  sticks at the lap seam on the third. La Bollène Original now finishes (750.1 s). Porto–Piana's colourful
  rocks are its `Background` multilayer, whose detail B is the 32 MB satellite photo of its diffuse repeated every 5 m, and
  the stretched cliffs (Porto–Piana, Puech Arnal, La Bollène's tunnel) are the source's UVs; both are unchanged from the
  first conversions. Porto–Piana's trees and road carry `ksAmbient`/`ksDiffuse` near 0.12 against 0.2 elsewhere: AC draws
  them darker; `ReRbrLight` now carries the ratio to the stage's reference.
- Reinstalled with `56366a8`: the same 30 stages, 27 GB (previous install not kept; the pre-fidelity folders remain in the
  backup). Riudecanyes 2022 835 MB and 15 s, La Bollène Original 588 MB, Sa Calobra downhill 602 MB, CréQuy 2025 3.3 GB and
  54 s. La Bollène Original (750.1 s) and Turini Gallego (259.9 s) finish headless with `false_start=0`.
- The `56366a8` reinstall came from `main`, which lacked the surfaces by declared friction: every road was 37 again and
  Riudecanyes' verges full-grip tarmac. Merged into `main` (`00f0e0e`) and the 30 stages converted again; only the files
  that differ were replaced (collision in `Objects.zip` and `objectlist.ini`, `mat.ini`, the surface entries of
  `materials.ini` and `Textures.zip`, `ac-conversion.json`; Joux Verte SS7 2020's `driveline.ini` moved under 1 mm), the
  former ones in the player's `backups/pre-surfaces-20261118`. Gunma and both Sa Calobra layouts declare no friction
  and are unchanged. Riudecanyes 2022 `probe`: no missing surface on either wheel track, stacked 0.55/0.21 %.
  Headless dry drives, slot 2 at 25 m/s, `finished=1`, `cause=Marker`, `false_start=0`: Riudecanyes 2022 1104.1 s
  (1104.1 with these surfaces on 2026-11-12), Turini Gallego 259.9 s.

## Normal-map alpha and never-drawn materials, 2026-11-18

- Compared against each track's `ui/preview.png` (elevated renders over the tightest hairpins, `--clouds clear`). Roure's
  pines were solid green slabs: `pines`, `pines_distant` and `scots_pine_leaves` (`ksPerPixelMultiMap_AT`) have DXT1
  diffuses without alpha and their cut-out in the BC7 normal map's alpha (71-86 % under half). An alpha-tested material
  whose diffuse is opaque and whose normal map's alpha cuts now takes that alpha (`material::cut_by_normal`). Of the
  122 source KN5s of the installed tracks only Roure's do this.
- An alpha-tested material with `ksAlphaRef` over 1 (CSP's alpha reference; 1.0 is the common default) is never drawn
  and is left out (`material::is_never_drawn`): Roure's `pinesTT` (10000, the pines' shadow caster, drawn with the
  normal map as its colour), its vehicles' `_glExt` glass (2) and Glava Zete's `SenkaKola` car-shadow decals (2).
- Roure downhill 3,899,278 to 3,857,798 drawn triangles, uphill 3,905,602 to 3,864,122, Glava Zete 1,929,464 to
  1,928,632; collision unchanged.
- Puech Arnal still shows dark flat sheets along some verges, lit and unlit; not back faces (culling them in a scratch
  ReRBR build kept the sheets and dropped the spectators). Not yet traced to a mesh.
- With the viewer's default clouds a stage can sit in cloud shadow and read cold and dark against AC's sunny previews;
  `--clouds clear` matches them. The asphalt reads darker than AC's under `lighting`, as drawn, not from the textures.
- Installed both Roure layouts and Glava Zete (changed files only; the former ones in the player's
  `backups/pre-roure-trees-20261118`). Roure downhill finishes headless, slot 2 at 25 m/s: 733.8 s, `cause=Marker`,
  `false_start=0`, at most 1.4 m off the driveline.

## Far scenery, CSP trees, 2026-11-18

- Counting what the conversion leaves out (a scratch build, since removed): most of the triangles missing from drawn
  against source are collision-only meshes AC never draws (Sa Calobra 2.8 M of 4.7 M, La Bollène 1.5 M of 2.5 M) and
  the other layouts' share of a combined file (Teixeta). The real losses were the horizon past the 300 m draw corridor
  (Sa Calobra 130 k triangles of terrain and sea, La Bollène 67 k of peaks, St Laurent 225 k) and CSP's trees.
- Far scenery: outside the corridor a triangle is kept where its longest side is at least 1/50 of its distance from the
  course (`Far`), to 10 km, grouped by 1 km tile and drawn to 10 km. 49 k to 183 k triangles (2 to 5 %) a stage.
  The size test alone cut holes in mountain surfaces (it drops the small triangles inside and keeps the large ones, sky
  showed on ridges), so a dropped triangle in the same mesh that shares a vertex with a kept far triangle is kept too,
  `FAR_RINGS` (3) times over (`Far::connected`). Sa Calobra down: far 49 k to 103 k triangles, `Objects.zip` 95.5 to
  96.8 MB, drawn 1.859 M to 1.912 M; Roure rbcl 177 k to 219 k, 114.3 to 115.1 MB.
- Far scenery, whole surfaces (2026-11-19): three rings still left floating crags over Sa Calobra's ridges (rock view,
  7.34 km), so `Far::connected` now keeps the whole surface joined to a kept far triangle through shared vertices (flood
  fill), within 10 km. Sa Calobra down far 103 k to 130 k, `Objects.zip` 96.7 to 97.5 MB; Roure rbcl 219 k to 221 k;
  conversion times unchanged (17 s, 25 s).
- Corridor meshes without a source range were all cut at `--lod` (1000 m): the valley's terrain and road below the
  4.8 km wall view dropped out in 128 m cells, a staircase-edged hole showing sky under the far mountain. An opaque
  corridor mesh is now drawn to its largest box side / `FAR_SHARE` (between `--lod` and 10 km; a full 128 m cell to
  6.4 km); cut-outs (grass, trees) keep `--lod`. Scene pass at two Sa Calobra views 2.31/1.81 ms to 2.33/1.81 ms.
- CSP trees: `extension/*.ini` `[TREES]` `LIST_<n>` names a file in `extension/trees` of `tree:` lines (`pos`, `size`,
  `width`; `configure: size variance`/`angle variance` set ranges for those that follow) of `.bin` files, each a zip of
  `tree.ini` (`HEIGHT`, `WIDTH`, `[SHADING] BRIGHTNESS`, LOD meshes), `.mesh` files (format in `crates/rbr-ac-tracks/src/assetto/trees.rs`,
  unit mesh `x, z` in -1..1, `y` 0..1) and textures. Three of the 15 source tracks have lists: Mulholland (16,961 trees
  of 3,000 to 4,500 triangles, bushes 700), Roure (4,132 trees, oaks of 10,000 triangles) and St Laurent (1,593 conifers
  of 8,000). ReRBR expands every placement (no instancing), so a stage cannot take them as meshes. Each tree within the
  draw corridor is four crossed cards textured with its LOD 0 rendered from four sides (`trees::bake`, 256 px each,
  alpha from the albedo or, as Roure's pines, the normal map), 8 triangles: Mulholland 16,647 trees 133 k triangles,
  Roure 3,399 and 27 k, St Laurent 1,593 and 13 k. Size is `HEIGHT` and `WIDTH` over the unit mesh's box times `size`;
  that `WIDTH` is a full width fits the oak (35 m for 41 m) and the conifer (16 m for 31 m) but is not confirmed
  against AC. `BRIGHTNESS` over 0.3 scales the albedo (oak 0.95 for a leaf albedo of 0.07; the others 0.25 to 0.4);
  unverified against AC.
- `[MODEL_REPLACEMENT]` `FILE`/`HIDE` in a track with a tree list: Mulholland draws `hedges.kn5` and
  `mulholland_foliage.kn5` as scenery and CSP hides their trees and hedges for its list. Those meshes are now left out
  (Mulholland East 3.37 M to 3.06 M drawn triangles, cards included). Crequy's and Joux Verte's replacements (windmills
  from a script, one object at night) have no tree list and are unchanged.
- Roure's `pines_distant` (the forest on far slopes) kept solid green slabs: its diffuse has alpha under 255 (shading)
  and no texel under 128, so `cut_by_normal` rejected it; it now needs only no texel under half alpha, and runs before
  the `useDetail` bake.
- Other CSP features in the source tracks' `extension` folders, not used: `[MATERIAL_ADJUSTMENT]` (24, day/night and weather conditions), `[SHADER_REPLACEMENT]`/`[MESH_ADJUSTMENT]`
  (water, `IS_RENDERABLE=0` hiding meshes on Thizy and Joux Verte, blend modes), `[WRAP_MESHES]` and `[ANIMATED]`
  (windmills and signs, Teixeta, Duesaigues, Riudecanyes), `[SPECTATORS]`, `[LIGHT_SERIES]`, `[PARTICLES_FX]`,
  `[RAIN_FX]`, `[TRACK_MAP]`, scripts (Crequy's turbines).
- All 29 installed stages converted again with far scenery, the tree cards and the `pines_distant` fix (Gunma's source
  was an unpacked copy in `/tmp`, gone, so it keeps its earlier conversion); changed files only, the former ones in the
  player's `backups/pre-trees-20261118` (23 GB). Mulholland East and West 16,647 trees each, Roure 3,399 and 3,370,
  St Jean - St Laurent and back 1,593. Headless drives, slot 2 at 25 m/s, `finished=1`, `false_start=0`: Roure uphill
  782.6 s (`cause=Left` at the line's end, 1.3 m off), Mulholland East 1131.3 s (`Marker`, 3.8 m off), St Jean - St
  Laurent 767.0 s (`Marker`, 2.2 m off; the run was the reverse layout, `slsj`).

## CSP mesh adjustments, Puech sheets, grass, 2026-11-18

- `[MESH_ADJUSTMENT_<n>]`/`[MESH_ADJUSTEMENT_<n>]` with `IS_RENDERABLE = 0` and no `CONDITION` (or `ALWAYS_ON`), active,
  now leaves its `MESHES` out of the drawing (`csp::unrendered`; names lowercase with `?` and `*`, `shader:` entries
  ignored; collision unchanged). Only Thizy has any (`OBJ3630`, `OBJ3651`: 40 triangles of the `vitrenuit` night window
  glow, Thizy 11,301,851 to 11,301,811); Joux Verte's name no mesh of its layouts. Not installed on its own: the stages
  carry it from the next conversion. `[MODEL_REPLACEMENT]` handling moved to `csp::replaced`.
- Puech Arnal's dark flat sheets (bisected by leaving meshes out and rendering the same hairpin): not `text_trans`
  (a fully clear texture on 178 triangles of 3,500 m²; leaving it out changes nothing), not the `blank_*` materials, not
  `Background3`, not the `obj_*` props. They are verge strips, `1wall_wall<n>` meshes of `ksPerPixelAT` `randon5bord*`
  textures: a flat strip of grass texture with one cut edge (alpha only along the top), over terrain of another colour,
  so its hard sides show. Whether AC shows the same is unknown (the preview is too small); not changed.
- `[GRASS_FX]` (26 stages) grows grass at run time on `GRASS_MATERIALS`; the converter now carries it (2026-11-19,
  `csp::grass_fx`): the listed materials (wildcards as `MESH_ADJUSTMENT`) get `ReRbrGrass=1` in `materials.ini`, and
  `settings.ini` gets `[ReRbrGrass]`: `texture` (`TEXTURE` where it names a `.dds` of the track's own `extension`
  folder, copied into `Textures.zip`; no key or a CSP-bundled file like `grass_fx/highlands.dds`: none, ReRBR's
  blades), `grid` (`TEXTURE_GRID`), `base` (`TEXTURE_BASE_CHANCE`), `group<n>` (`TEXTURE_GROUP_<n>_CHANCE`, default 0)
  with `group<n>_piece<m>=col,row,w,h,chance[,mw,mh]` (`PIECE_<m>`: cells from 1, row 1 at the bottom of the picture, `PIECE_<m>_CHANCE` default 1, `PIECE_<m>_SIZE_MULT` read as width, height multipliers), `size` (`SHAPE_SIZE`),
  and `luminance=min,max` (`MASK_MIN/MAX_LUMINANCE`). Dense grass (`--enhance grass`) then grows on those materials
  instead of by colour (`grass::add`): where the ground texel's linear luminance is in range, on any facing the
  ground's normal allows, within the usual 20 m band. Not used: `MASK_MAIN_THRESHOLD`, `MASK_RED_THRESHOLD`,
  `SHAPE_TIDY/CUT`, `OCCLUDING_MATERIALS`, `[GRASS_FX_CONFIGURATION_*]`/`ADJUSTMENT`
  (Highlands). Row order checked on Sa Calobra's atlas (`3,1,1,2` the heather, `4,1,2,2` the reeds, the top row the
  plain grass, which `base` takes one cell each; test `an_atlas_piece_counts_its_rows_from_the_bottom`). Size: a cell is
  `SHAPE_SIZE` x 0.08 m (6.5: 0.52 m, calibrated by eye on the AC rock shot's boulders: tussocks 0.4-0.8 m), times the
  piece's cells and `SIZE_MULT`, 0.8-1.2 per clump; unset `SHAPE_SIZE` takes 1.5. Colour: the ground's hue at
  `FX_SATURATION` 0.3 (no lush/straw lean) and `FX_SHADE` 1.2 with the brightness ratio clamp lowered (`FX_BRIGHT`):
  Sa Calobra dawn blade mean 39/34/26 against AC's 41/34/30 (ground 50/44/30). The support ring is 0.4 of a card's
  width (the meadow's ring halved the cards). Unverified: `SIZE_MULT` as width,height (it may be a min,max scale);
  the atlas grey pass skipped compressed (DXT) atlases until 2026-11-19, so Sa Calobra's had used our blades.
  Sa Calobra down, rock view 07:35 (`/tmp/grassfx/rock.png`, before | after): grey terrain went from a patch of
  colour-picked grass to olive tufts over the dark ground, none on the white boulders (luminance over 0.25), as the AC
  shot. Riudecanyes (no `TEXTURE`): our blades on `srfdtl` and `cliffs3`. The atlas is drawn grey (luminance of its
  texels, mips and cut-out coverage rebuilt) so the ground's hue and brightness colour it, as ours.

## Launcher import (2026-11-18)

- The converter lives in `crates/rbr-ac-tracks` (the readers `assetto`, the writer `btb` and the interface `import`
  the launcher calls, `cli` for the `rbr-ac-tracks` executable: `ac`, `import`, `prepare`, `probe`). It needs only
  `rbr-btb` and `rbr-formats`, not the renderer. The standalone `btb-converter` repository (the AC reader's history, and
  BeamNG) is no longer used; its AC code, tests and the two stage-checking scripts (`scripts/btb-autopilot.py`,
  `scripts/btb-shots.py`) moved here on 2026-11-18, BeamNG did not.
- The launcher's Add (`crates/rbr-launcher/src/content/kind/ac.rs`) takes a folder, a zip or a 7z with an AC track:
  a folder with a KN5 of its own and `data/surfaces.ini` (its own or in a layout folder, names ignoring case; a car
  has no surfaces). It unpacks it without programs and sound banks into a hidden folder in `tracks/`, converts each layout
  by `import::import` (circuit when the AI line is a closed loop without gates, smoothed road when a collision cell
  overflows) into `AC_<track>` or `AC_<track>_<layout>`, and removes the hidden folder; a cancel stops the conversion
  and leaves nothing behind. A layout the converter cannot take (no AI line, no surfaces) is reported with its reason.
- A stage with `ac-conversion.json` lists in Stages as "Added by you · Assetto Corsa import" (`Btb::ac`); the search
  matches "assetto" and "ac import".
- Get more stages links AC Rally Central's rally list (each track has its author's download, RBR conversions too) and
  assettocorsa.club's track mods. Neither is checked for what its archives hold beyond that; protected KN5s and tracks
  that need CSP to look right convert as far as the readers go.
- The launcher reads `.rar` too (`rars`, pure Rust, Apache-2.0, read only: RAR 1.3 to 7, one volume, no encryption;
  a young crate, its README calls it an experiment, but it extracted all six archives below as `unrar` does). Its
  decoding runs in file order, so progress and Cancel move per file.
- Checked 2026-11-18, each from the archive through the launcher's Add (`RBR_ASSETTO=<archive or folder> cargo test -p
  rbr-launcher a_real_track -- --nocapture`), the four last also rendered (no drive): `asw-steppe-rally.zip` (tundra),
  `pineridgerallyv1_by_ultranewb.7z`, `AC_Monte_Carlo-Stage3-V1_04-TTM.rar`, `Tramo_Ermita_Sant_Antoni_v1.01.rar`
  (6.1 km) and `simtraxx_pribram_0.7` `.rar` (the `race` layout, imported as a circuit; its three other layouts have no
  AI line) and `Gunma Cycle Sports Center v1.2.rar` (two of three layouts; `gcsc_full_attack` has no
  `data/surfaces.ini`). Not converted, with the reason shown: `simtraxx_el_cubilete_0.6.rar` (only `ai/_fast_lane.ai`,
  AC ignores it) and `zelezniki_forward.rar` (empty `ai` folder). A track without an AI line cannot give a driveline.
- Also checked: the synthetic track as a zip (listed, marked, re-add refused, cancel clean). Not checked by hand in
  the launcher window, and neither site's own download links (mega.nz, overtake.gg) were followed from the launcher.

## Gravel and `physics=`, 2026-11-18

- Before: every stage was `physics=tarmac`, and a gravel rally's `ROAD` (Pine Ridge, Steppe: `FRICTION` 0.89 with
  `DIRT_ADDITIVE`) became tarmac 85.
- Now: CSP's `_EXT_SURFACE_TYPE` in `surfaces.ini` comes first (`GRAVEL` gravel, `ICE` 117, `GRASS` grass; only
  `dr_pikes/pikes_gravel` of the 2 205 sections in 244 local layouts sets it; `dwrite.dll` also names `UNKNOWN`). Then a
  `ROAD`, `ASPHALT`, `TARMAC`, `PIT` or `GRAV*` key that dirties the tyres and declares at most 0.90 is gravel (17).
  Tarmac that dirties tyres declares more (Monte Carlo 0.93, St Laurent's `GRIT` 0.96-0.98) and stays tarmac. The edge
  is thin: 0.89 and 0.93 are the nearest values seen. Turini's verge `BORDE` (0.89, dirty) stays 85, being no road key.
- `physics=` is gravel when over half of the driveline's length lies on gravel or dirt ids (1, 9, 17, 133, 141, 149),
  else tarmac; `ac-conversion.json` records it. Snow is not guessed. One tyre model serves the whole stage, so a mixed
  stage's minority is driven on the wrong one.
- Checked: reconverted St Laurent, Sa Calobra, Roure, Riudecanyes: surfaces identical, tarmac. Peyregrosse's `GRAV` 133
  becomes 17, tarmac. Pine Ridge (`ROAD` gravel to 5.7 km, then `TARMAC`) gravel; Steppe gravel, finished (10.4 km);
  Pikes gravel finished (19.3 km); Pikes mixed (`GRAVA` 69 %) gravel, finished. `--surface gravel` reaches the physics.
  Pine Ridge's autopilot stops at 5.98 km in a barrier chicane the straight driveline runs into, with tarmac too.

## Converting the whole local install, 2026-11-18

- A full pass over 221 ready layouts of one install (no heavy 70-100 km ones) found these failures, fixed here:
  a collision mesh gap of a few centimetres under a driveline point (`ek_*` touge: 25 to 36 points of about 2 500 each
  with ground within 0.3 m; now the nearest ground within 0.5 m stands in, and a gap under the spawn too); an AI line
  recorded finish to start (Akina uphill, Tsuchisaka outbound: reversed when that makes the gates follow, but not a
  closed loop, which imports as a circuit: Nordschleife `touristenfahrten`).
- Not fixed: `ek_hakone_turnpike` (collision cells over the 1024 slots even at 25 m and with `--remesh`); Pico Veleta
  (a collision cell of 595 000 vertices: its road mesh has 15 cm triangles, 100 per m2, and `--remesh` writes 3 300
  faces where 215 000 level faces stay in the densest 200 m cell, cause not found); layouts with no start gate or a gate
  49 m off the AI line (`ek_usui_circuit`, `haruna`, `ks_drag`, `santa_monica_mtns`, `simtraxx_drazgose`); a damaged
  install (`la_canyons`, `pacific_coast_hwy`). The `ek_*` tracks of that install carry one-pixel textures, so they
  render grey.


## SuperStage tracks, 2026-11-19

- The `ss_*` mods of the local install: Roure, Sa Calobra, St Laurent and La Bollène were already in; added through
  `rbr-ac-tracks import` (own AI, `--name` each): Cabanette `down`/`up` (`AC_La_Cabanette_Col_de_Braus_Down/Up`,
  collision cell overflow, so the importer retried with `--remesh --collision-corridor 25`), Gabiria `gl`/`lg`
  (`AC_Gabiria_Legazpi_2004`, `AC_Legazpi_Gabiria_2004`) and Prunières `pe`/`per` (`AC_Prunieres_Embrun`,
  `AC_Embrun_Prunieres`). Sa Calobra `freeroam` stays skipped (no timing gates). Without `--name` a multi-layout track
  is `AC_<layout>`, which for `down` met an existing `AC_down` (Sa Calobra): name them.
- Gabiria and Prunières carry skinned meshes (`persAnim*`, animated spectators; KN5 node type 3) that stopped the
  reader. It now reads past them (bone count, bones of name and 64-byte matrix, vertices of 76 bytes, indices, material,
  12 bytes of layer and level of detail; the node tree of `persSteJus.kn5` ends exactly at its last byte) and draws and
  collides nothing of them.
- No missing textures or ground; largest collision cell 65,439 of 65,536 (Cabanette down). Headless dry drives (Toyota GR Yaris Rally2
  of the sweep, 25 m/s), all `finished=1`, `false_start=0`, at most 2.4 m off the driveline: Cabanette up
  552.4 s (`Marker`), down 520.7 s, Gabiria-Legazpi 435.5 s (`Marker`), Legazpi-Gabiria 430.4 s, Prunières-Embrun 1140.5 s, Embrun-Prunières 1159.4 s (these
  four stop short of the result marker, `cause=Left`, finish recorded). Not rendered.

## Normal maps and highlights, 2026-11-19

- `materials.ini` sections keep `effectName=BTB_Diffuse` and the diffuse (other BTB readers draw it unchanged) and add `normalTexture`
  (`txNormal`, stored like a diffuse, re-encoded as opaque DXT1 where needed), `specularTexture` (`txMaps` red as grey DXT1), `SpecularPower`
  (`ksSpecularEXP`, at least 1, 4 if missing) and `SpecularColor` (`ksSpecular` three times), and `ReRbrEffect` =
  `BTB_DiffuseBump`, `BTB_DiffuseSpecular` or `BTB_DiffuseBumpSpecular` (ReRBR's own key, [btb.md](btb.md#drawing)). All inputs are read on the diffuse's UV set 0; the meshes are unchanged.
- Normal map: per-material sections only, not placeholders, not `Multilayer` (but `fresnel_nm`'s `txDetailNM`, below), not tile bakes, not tree cards or generated sections; cut-outs may take
  one. Highlight: opaque non-placeholder materials with `ksSpecular > 0`, per-material and tile bakes; the spec map only per material. Without
  either the section is today's byte for byte. `ac-conversion.json` counts `normal_mapped_materials` and `highlight_materials`.
- Sa Calobra down: 18 sections with a normal map, 19 with a highlight (4 both), 0 with a spec map (no highlighted material has `txMaps`);
  Textures.zip 518.2 MB to 553.8 MB (+5 textures), import 21 s as before, `Objects.zip` byte identical. Rendered at four
  driveline points (`--enhance all`, 11:00): the rocks (`extrarocks`, 811 k triangles), walls and rails gain their relief
  (RMSE 4 to 14 of 255 against main); the roads' highlight (`ksSpecular` 1, EXP 15) is a faint sheen, none blown out facing
  the sun. The new files on main's viewer render byte identical to the old conversion.

## UV multipliers; detail normals and CSP vertex AO tried, 2026-11-19

Evidence: the compiled AC shaders (`system/shaders/win/*_ps.fxo`, disassembled with vkd3d-compiler).

- `ksPerPixelNM_UVMult` (19 installed tracks, 2.0 M triangles) reads its diffuse at uv * (1 + `diffuseMult`) and `txNormal` at
  uv * (1 + `normalMult`). The triangles' UVs carry the first (`stage.rs`, `material::uv_multipliers`) and the section
  `ReRbrBumpScale` the ratio ([btb.md](btb.md)). Not for a placeholder diffuse.
- `ReRbrLight=<ambient>,<diffuse>` (2026-11-19): `ksAmbient`/`ksDiffuse` relative to the reference, the
  area-weighted (m²) median of each over the drawn opaque materials (`material::normalise_light`, after the last
  section), clamped to 0.25..3, omitted within 2% of 1 on both; none for placeholders or materials lacking either.
  Trees as CSP cards keep their own brightness (no key). Sa Calobra down: reference 0.62/0.30 (terrain); road 1.77/1.67, cliff and rocks 1.08/1.07, boulders 0.37/0.25, walls 0.44/0.25.
  With the sheen (`SHEEN_STRENGTH` 0.25/0.35/0.5, Sa Calobra wall view 07:35) the road reads 43/48/55 against AC's 57–59;
  toward the sun the road is 0.8–1.0x its away-from-sun level. That 0.5 made up for a sun too high: AC's shots have the
  sun on the horizon (06:45 here), the foreground in shade. There, with `[ReRbrAir]` `mist=0`, 0.5/0.2/0.1 give rock
  near road 106/85/65 and wall road 73/64/55 (AC 59/59), the pillar 24 → 38 (AC 41); 0.5 washed the road's texture out
  toward the sky, so `SHEEN_STRENGTH` 0.1 (2026-11-19). Not re-rendered on Roure.
- Tried and taken out again (Roure, renders at four driveline points, midday clear, 08:15 low sun and 11:00 overcast; mean change
  of 255 levels): `ksMultilayer_fresnel_nm`'s `txDetailNM` (at the source uv * the pair `detailNMMult`) and the
  `ksPerPixelMultiMap_(AT_)NMDetail` detail layer (`txNormalDetail` at uv * `detailUVMultiplier`, `N + (1 - diffuse.a) *
  detailNormalBlend * (D - N)`) drawn at runtime: 0.1 to 4.5, the road's millimetre detail flat at chase distance (in AC
  it feeds the sheen and fresnel), for +100 MB of Roure textures. CSP's vertex AO (`.vao-patch`: zip, records of name, kind,
  raw vertex 0, count, a byte per vertex in v4/v5 (v5 `(b/255)^2`), halves in the legacy `Patch.data`; matched 99 % of Roure's
  and Sa Calobra's drawn triangles) as the greater occlusion with the computed sky occlusion: 0.1 to 1.9, as it mostly agrees
  with it, for +4 % `Objects.zip`. KN5-embedded AO (tangent lengths) is used by none of 589 installed KN5s.

## Dry road sheen and the road normal map, 2026-11-19

- `ReRbrSheen=<roughness>` ([btb.md](btb.md)) on every opaque, non-placeholder, non-foliage material (tile sections too): GGX
  `alpha = sqrt(2 / (n + 2))` from `ksSpecularEXP`, roughness `sqrt(alpha)` clamped to 0.45..0.85, 0.75 where `ksSpecular` <= 0
  or n < 1. AC's fresnelC/EXP/max are not read (inconsistent author tuning: C=0 EXP=0 max=0.7 is a constant 70% mirror).
  Which part is road is the renderer's (`Vertex::wet`, tarmac collision surface under the vertex; the physical meshes live in
  `phys.kn5`, apart from the visible ones, so no name or material join works in the converter). Roure 395 sections, Sa Calobra 606.
- `ksMultilayer_fresnel_nm`'s `txDetailNM` is back as the section's normal map at the source uv times `detailNMMult` (`ReRbrBumpScale=u,v`);
  tile bakes carry the source uv as a second UV set (`ReRbrBumpUv=1`, `FVFData`). Not back: the NMDetail layer, the mask re-encode, CSP AO.
  Textures.zip Roure 499 to 544 MB, Sa Calobra 529 to 553 MB; 365 and 579 sections gain a normal map.
- Roure and Sa Calobra at four driveline points each, `--enhance all`, 1600x900, main against this (mean absolute grey change of 255 / share of
  pixels over 8): midday roure 15.9 / 76 %, sacalobra 9.4 / 36 %; overcast 20.7 / 86 %, 9.6 / 35 %; low sun facing it (17:00) 39.2 / 94 %,
  21.9 / 81 %. About half of it is the auto exposure: a brighter road darkens the frame (top third 5 to 30 % darker). The normal maps add
  1.5 to 5 on top of the sheen alone. Full physical strength read as wet or icy blue on the black converted roads, so the sheen is
  weighted `SHEEN_STRENGTH` (0.1 after the low-sun calibration). Known: a patch decal lying on the road (Roure 6.5 km) takes no sheen.
- Road material the collision calls gravel or dirt within 8 m of tarmac takes the tarmac's sheen (`puddles::mark_sheen`): Roure's shoulders showed as a dark band. Not fixed, accepted: Roure's visual-only road pad (6.5 km, over grass collision) and kerb strip (5 km, over wall collision), which carry no tarmac under them; cut-outs and blended decals take no sheen.
- Into-sun glare (Sa Calobra 07:35, rock and wall recreations): the glare came from the Blinn-Phong highlight (`ksSpecular` 1, EXP 10) staying at 1 - weight under the sheen, not from the retail `with_specular` (which never runs on converted draws: they have a highlight). The sheen now replaces the highlight on tarmac. Road medians (grey of 255; diffuse alone / 0.35 / 0.5 / 1.0): rock near 7 / 88 / 118 / 191, mid 20 / 87 / 114 / 183 (mid/near 0.96 to 0.99, AC's about 1.1); wall near 19 / 42 / 51 / 81 (AC 59). `SHEEN_STRENGTH` 0.5, later 0.1 (above).
- Haze (ours, 2026-11-19): every import writes `settings.ini` `[ReRbrAir]` `fog_color=0.74, 0.68, 0.62` and
  `aerosol=0.2` and `mist=0` (`SETTINGS_INI`; see `notes/renderer.md`), so the sky and distance read warm grey instead of map 41's
  clean blue. Roure rbcl 13:00 stays mild (sky blue in clear gaps, pale only near the horizon). Sa Calobra 07:35 at 0.2:
  wall sky (201,206,211) top, (208,213,218) horizon (0.3: (200,206,212), (210,214,215); AC (187,196,211), (208,208,210)).

## Multilayer at render time, 2026-11-19

- `ksMultilayer*` materials whose diffuse and mask fit `multilayer::LIMIT` are drawn by the renderer: `TextureBank::layers` stores the
  diffuse, the mask and the four details under their own names (`slot`; shared details once, by name) and `section_with` writes
  `ReRbrLayerDiffuse`, `ReRbrLayerMask`, `ReRbrLayerDetail=a,b,c,d`, `ReRbrLayerScale` (eight numbers: each detail's `(u, v)`; the
  scalar `multX` twice for world space, the `_objsp` pair otherwise), `ReRbrLayerMagic` and, for world space, `ReRbrLayerWorld=ox,oz`
  (the converter's `origin`, six decimals; absent for `_objsp`: details at `uv * pair`). Keys in [btb.md](btb.md); they need `ReRbrEffect`
  (`BTB_Diffuse` is written for them). `diffuseTexture` is still the material's bake, the fallback for a reader without the keys.
- Such materials get no tile bakes (`tile_bakes` takes the set), so meshes keep their source UVs: the normal map is `txDetailNM` at the
  source uv times `detailNMMult` as for any per-material section (no `ReRbrBumpUv`, no `uvs2`). `ac-conversion.json`:
  `multilayer_per_pixel_materials`.
- Sa Calobra down (`/tmp` conversions, release binary): `Textures.zip` 581.1 to 422.4 MB (603 to 62 textures: no tile textures),
  `Objects.zip` 97.5 to 96.7 MB (no chart copies of vertices), import 22.5 s to 17.0 s. Roure rbcl: `Textures.zip` 572.4 to 698.5 MB (354 tile
  textures gone, the raw 22 MB diffuses, masks and details added), 25 s as before, 15 materials per pixel.
- Rendered (Sa Calobra 06:45, `--enhance all,grass`, 1600x900, `scripts`-style views over the driveline at 7340 m and 4800 m): near road grey 64.1 / 8.0
  (mean / standard deviation) to 63.8 / 8.2 and 46.1 / 21.3 to 46.7 / 20.6 (AC's 59 / 11): the grain is now the detail textures' structure
  (cracks, patches) at any distance; the large-scale colour matches the tile bakes (blurred-grey correlation 0.95 to 0.999), so the world
  offsets are AC's. Roure rbcl 12:00 at 750 m and 3750 m along: no visible regression.
