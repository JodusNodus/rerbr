# BeamNG levels

`crates/rbr-beamng` converts the player's own copy of a BeamNG level mod and one of its time-trial missions to a BTB
stage folder (`beamng-stage`), which the BTB path plays (`notes/btb.md`). Nothing of a level is committed: level mod
licences forbid sharing modified versions. Tests over a level only with `RBR_BEAMNG` (none yet). Evidence: the
Gheo Nordschleife 2026 mod (2026-11-08).

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

## Conversion

- Frame `(x - ox, z, y - oy)`, `(ox, oy)` the course's plan centre (680, 264). Collision mesh within 60 m of the
  driveline as `Visible=0`, `CollisionOnly=1` objects (100 m tiles per surface), each surface a material whose `mat.ini`
  map is the RBR id in all cells; drawn shapes within 25 m (CLI) as `CollisionModel=-1`.
- Surfaces: ASPHALT 37, RUMBLE_STRIP/CONCRETE/flat METAL 65, COBBLESTONE 69, GRASS 105, GRAVEL/SAND 9, DIRT 141, no
  model: steep 64 (barriers), flat 37.
- Visual road against the collision: p95 0.004 m, max 0.02 m. Collision 0.70 m vertex spacing along and across.
- Driveline every 2.5 m (the physics takes at most 10000 entries: 2 m gave 10397 and the port failed), heights on
  the collision; a loop's starts 50 m before the start, finish 90 m before the start; notes start/splits/finish/end.
- Nordschleife: 860233 collision triangles, `col.col` 57 MB in 296 cells, largest 52020 vertices (no 16-bit wrap),
  built in 1.5 s; drawn 2.73 M triangles (road, grass, signs, additions; rails, trees, people, GP left out by
  `--skip`). Map 41's fences sit over 900 m from the driveline with nothing under them.
- Autopilot (Fiesta Rally2, NGP, 30 m/s cap): 10.4 km of 20.8 in a 10-minute run, at most 1.3 m off the driveline;
  the finish not yet reached in one run.
- Captures (`viewer --camera EYE,DIRECTION`, the second triple a direction): start, Flugplatz (2.4 km), the
  Karussell approach (11.9 km), Döttinger Höhe (18.1 km), all with 0 missing textures.
