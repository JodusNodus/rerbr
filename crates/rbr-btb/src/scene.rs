//! A BTB track as draw-ready data, with no renderer types: the scenery of one `RX_CONTENT/Tracks/<folder>/`.
//!
//! [`Scene::load`] reads the folder's `objectlist.ini`, `materials.ini`, `Objects.zip` meshes and placements, and
//! `spawn.ini`, `driveline.ini` and `settings.ini`, and hands them over as: [`Mesh`]es (one per distinct `.x` file:
//! positions, normals, the UV sets and the vertex streams its effect needs), [`Material`]s (the effect and technique
//! of `materials.ini`, the texture files found in `Textures.zip`) and [`Object`]s (a mesh, its placements and its
//! `objectlist.ini` flags). Textures are read on request ([`Scene::texture`]).
//!
//! # Frame
//!
//! The scene frame is the files' own: Direct3D, left-handed, y up, metres. It is the frame of the classic `.lbs`
//! geometry too (`notes/formats.md` "Stage geometry"), so a renderer takes positions as they are. The `.xform`
//! matrices place a mesh with `p * M` (row vectors); the effects light with the normal taken the same way
//! ([`place_point`], [`place_normal`]). The driveline, written in the physics frame (z up), becomes `(x, z, y)`;
//! `spawn.ini` is already in the scene frame. `notes/btb.md` "Drawing" has the evidence.
//!
//! # Effects
//!
//! What each `BTB_*` effect (the `.fx` sources in a track's `FX/`) computes is in [`Effect`] and [`Technique`].

use crate::track::{self, Track};
use crate::{xfile, xform};
use rbr_formats::{dds, rbz};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A `BTB_*` effect, as its flags: the extra inputs it samples beyond the diffuse texture. The pixel colour is
/// `texture * (ambient + max(0, N.L))` with `ambient` 0.4 and `L` towards the sun (`sunDirection` negated), where
/// `texture` is:
///
/// - `vert_lerp`: `mix(diffuse, second, blend)`, `blend` a per-vertex weight (`BLENDWEIGHT`), alpha from the first;
/// - `multiply_add`: `diffuse * multiplier + additive` (before the lerp when both);
/// - `bump`: `N` is the normal map's (red and green as x and y, the normal rebuilt from them) in the tangent frame;
/// - `specular`: plus `specularTexture * SpecularColor * pow(max(0, N.H), SpecularPower)`, `H` between the eye and the
///   sun.
///
/// `sky` is `BTB_Sky`: a sky dome material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effect {
    pub bump: bool,
    pub specular: bool,
    pub multiply_add: bool,
    pub vert_lerp: bool,
    pub sky: bool,
}

impl Effect {
    /// The effect `name` (`BTB_DiffuseBumpSpecularVertLerp`, ...); an unknown or missing name is `BTB_Diffuse`.
    pub fn parse(name: Option<&str>) -> Effect {
        let name = name.unwrap_or_default().to_ascii_lowercase();
        Effect {
            bump: name.contains("bump"),
            specular: name.contains("specular"),
            multiply_add: name.contains("multiplyadd"),
            vert_lerp: name.contains("vertlerp"),
            sky: name == "btb_sky",
        }
    }

    /// Which entry of [`Mesh::uvs`] each input reads: the UV sets after the diffuse one come in this order (the vertex
    /// input lists of the `.fx` files): bump, specular, multiplier, additive, lerp.
    pub fn uv_slots(self) -> UvSlots {
        let mut next = 1;
        let mut take = |wanted: bool| {
            wanted.then(|| {
                next += 1;
                next - 1
            })
        };
        let bump = take(self.bump);
        let specular = take(self.specular);
        let multiplier = take(self.multiply_add);
        let additive = take(self.multiply_add);
        let lerp = take(self.vert_lerp);
        UvSlots { bump, specular, multiplier, additive, lerp }
    }
}

/// The index into [`Mesh::uvs`] of each effect input's UV set; `None` where the effect has no such input. The
/// diffuse texture reads set 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UvSlots {
    pub bump: Option<usize>,
    pub specular: Option<usize>,
    pub multiplier: Option<usize>,
    pub additive: Option<usize>,
    pub lerp: Option<usize>,
}

/// A `techniqueName`: `Default`, `DefaultUp`, `Transparent`, `TransparentUp`. All cull nothing and never fog.
///
/// - `up`: the normal is `(0, 1, 0)` for every vertex (foliage and the like lit as if facing the sky);
/// - `transparent`: two passes, first alpha blended (`SRCALPHA`, `INVSRCALPHA`) without writing depth, then the
///   alpha test `alpha >= 200` writing depth. The default is one opaque pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Technique {
    pub up: bool,
    pub transparent: bool,
}

impl Technique {
    pub fn parse(name: Option<&str>) -> Technique {
        let name = name.unwrap_or_default().to_ascii_lowercase();
        Technique { up: name.ends_with("up"), transparent: name.starts_with("transparent") }
    }
}

/// A `materials.ini` section, as the meshes name it.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// `effectName`, or `ReRbrEffect` (ReRBR's own key, which the plugin does not know: a converted stage's normal map
    /// and highlight under a plain `BTB_Diffuse`) where the section has it.
    pub effect: Effect,
    /// The effect is `ReRbrEffect`'s: every input reads the diffuse's UV set, and the mesh carries no tangent frame.
    pub first_uv: bool,
    pub technique: Technique,
    /// `ambient` is 0.4 in every folder, and the effects use that constant; the file's value is not carried.
    pub specular_power: f32,
    pub specular_color: [f32; 3],
    /// The `Textures.zip` file names (as the archive holds them, no directory) of the inputs; `None` where the
    /// material names none or the archive lacks it.
    pub diffuse: Option<String>,
    pub second: Option<String>,
    pub normal: Option<String>,
    pub specular: Option<String>,
    pub multiplier: Option<String>,
    pub additive: Option<String>,
    /// ReRBR's `ReRbrBumpScale=u,v`: the normal map is read at its UV set times this. `[1, 1]` where the section has
    /// none.
    pub bump_scale: [f32; 2],
    /// ReRBR's `ReRbrLight=ambient,diffuse`: factors on the sky and bounce light, and on the sun's. `[1, 1]` where the
    /// section has none.
    pub light: [f32; 2],
    /// ReRBR's `ReRbrBumpUv=1`: the normal map reads the mesh's second UV set ([`Mesh::uvs`] entry 1) whatever
    /// [`Material::first_uv`] says.
    pub bump_uv: bool,
    /// ReRBR's `ReRbrSheen=<roughness>`: the opaque road's dry sheen, a rough dielectric's sky reflection and sun
    /// highlight at this roughness. Only with `ReRbrEffect`.
    pub sheen: Option<f32>,
    /// ReRBR's `ReRbrGrass`: what the stage's grass ([`Scene::grass`]) does on this material (converted stages).
    pub grass: Grass,
    /// ReRBR's `ReRbrLayer*` keys: the material is a multilayer sum the renderer works out per pixel. Only with
    /// `ReRbrEffect`, and only when every texture is in the archive.
    pub layers: Option<Layers>,
}

/// ReRBR's `ReRbrGrass` (converted stages, CSP's GrassFX): `1` the grass grows on the material (`GRASS_MATERIALS`),
/// `hide` it grows nowhere under it (`OCCLUDING_MATERIALS`), `hide-alpha` nowhere under its opaque texels
/// (`OCCLUDING_MATERIALS_ALPHA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grass {
    #[default]
    None,
    /// At a profile: 0 the stage's, `n` its `[ReRbrGrass<n>]` (`ReRbrGrassProfile=<n>`; CSP's GrassFX adjustments).
    Grows(u8),
    Hides,
    HidesAlpha,
}

/// `diffuse * (R*mask.r + G*mask.g + B*mask.b + A*mask.a) * magic`, the sum of Assetto Corsa's `ksMultilayer` shaders
/// (the section's `diffuseTexture` is a bake of it, for what does not read these keys). The diffuse and the mask are
/// read at the mesh's UV set 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Layers {
    /// The `Textures.zip` file names of the diffuse, the mask and the details for the mask's R, G, B and A.
    pub diffuse: String,
    pub mask: String,
    pub details: [String; 4],
    /// Each detail's `(u, v)` multiplier.
    pub scale: [[f32; 2]; 4],
    pub magic: f32,
    /// `ReRbrLayerWorld=x,z`, the stage's origin in AC's frame: each detail is read at AC's `(x, z) * scale`, with
    /// `x = stage x + origin x` and `z = -(stage z + origin z)`. `None`: at the UV set 0 times `scale`.
    pub world: Option<[f64; 2]>,
    /// `ReRbrLayerUv=1`: the details read the mesh's UV set 1 times `scale`, and the weights are the vertex's, the second
    /// to fourth layers' in the UV sets 2 (`u`, `v`) and 3 (`u`), the first taking the rest. There is no mask then
    /// (`mask` is the diffuse's name, unread). DiRT Rally's terrain: its own layer coordinates beside the atlas of the
    /// diffuse.
    pub uv: bool,
    /// `ReRbrLayerBlend=S` (with `uv`): DiRT Rally's road blend with `DetailBlendScale` `S`. The second and third layers' textures
    /// carry their height maps in the alpha, and the weights R and G are roughened by them (`layered` in the shader).
    pub blend: Option<f32>,
}

impl Material {
    /// The UV set each of the effect's inputs reads: [`Effect::uv_slots`], or set 0 for all with `ReRbrEffect`.
    pub fn uv_slots(&self) -> UvSlots {
        let slots = self.effect.uv_slots();
        if !self.first_uv {
            return slots;
        }
        let first = |slot: Option<usize>| slot.map(|_| 0);
        UvSlots {
            bump: if self.bump_uv { slots.bump.map(|_| 1) } else { first(slots.bump) },
            specular: first(slots.specular),
            multiplier: first(slots.multiplier),
            additive: first(slots.additive),
            lerp: first(slots.lerp),
        }
    }
}

/// One `.x` file's geometry, vertices unique per position-and-normal pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// The `.x` file's name (no extension), as `objectlist.ini` gives it.
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// The tangent frame of the bump effects, empty when the mesh carries none.
    pub tangents: Vec<[f32; 3]>,
    pub binormals: Vec<[f32; 3]>,
    /// The UV sets: the diffuse one, then those the effect names ([`Effect::uv_slots`]); empty if the mesh has no UVs.
    pub uvs: Vec<Vec<[f32; 2]>>,
    /// The `VertLerp` weight per vertex, empty when the mesh carries none.
    pub blend: Vec<f32>,
    /// Triangle list.
    pub indices: Vec<u32>,
    /// Index into [`Scene::materials`]; `None` when the mesh's material has no `materials.ini` section.
    pub material: Option<usize>,
}

/// One `objectlist.ini` entry: a mesh and where it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    /// Index into [`Scene::meshes`].
    pub mesh: usize,
    /// The `.xform` placements, row-major Direct3D matrices ([`place_point`]).
    pub placements: Vec<[f32; 16]>,
    /// Drawn only from this far, metres from the eye (`LODIn`).
    pub lod_in: f32,
    /// Drawn only up to this far (`LODOut`); infinite when the file gives none.
    pub lod_out: f32,
    pub visible: bool,
    /// Its faces are the collision ground's (`CollisionModel=0`, drawn or `CollisionOnly`, [`crate::col`]): the
    /// stage's ground, its walls and its buildings.
    pub collides: bool,
    pub shadow_caster: bool,
    /// Casts into the sun's near cascades (`SunCascades`, ReRBR's own key).
    pub sun_cascades: bool,
    /// Drawn as one mesh and a transform per placement (`ReRbrInstanced`, ReRBR's own key).
    pub instanced: bool,
}

/// Where the car starts (`spawn.ini`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    pub position: [f32; 3],
    /// The heading, radians from +z towards +x: the car faces `(sin yaw, 0, cos yaw)`.
    pub yaw: f32,
}

/// A track's scenery, see the module docs.
#[derive(Debug)]
pub struct Scene {
    pub name: String,
    pub spawn: Spawn,
    /// The driveline's points in the scene frame.
    pub driveline: Vec<[f32; 3]>,
    /// `settings.ini` `sunDirection`, the direction the sunlight travels (pointing down).
    pub sun_direction: Option<[f32; 3]>,
    /// `settings.ini` `[ReRbrGrass]`.
    pub grass: Option<track::GrassFx>,
    /// `settings.ini` `[ReRbrAir]`.
    pub air: Option<track::Air>,
    pub materials: Vec<Material>,
    pub meshes: Vec<Mesh>,
    pub objects: Vec<Object>,
    pub textures: TextureFiles,
}

impl Scene {
    /// Reads the track folder.
    pub fn load(folder: &Path) -> Result<Scene, String> {
        let track = Track::open(folder)?;
        let spawn = track.spawn()?;
        let driveline = track.driveline()?;
        let settings = track.settings()?;
        let (sun, grass, air) = (settings.sun_direction, settings.grass, settings.air);
        crate::phases::mark("btb: track, spawn, driveline, settings");
        let objects = track.objects()?;
        crate::phases::mark("btb: object list");
        let files = TextureFiles::open(folder)?;
        let textures = Textures::list(&files.0)?;
        crate::phases::mark("btb: Textures.zip list, materials");

        let sections = track.materials()?.sections;
        let materials: Vec<Material> = sections.iter().map(|m| material(m, &textures)).collect();
        let by_name: HashMap<String, usize> =
            materials.iter().enumerate().map(|(i, m)| (m.name.to_lowercase(), i)).collect();

        let objects_zip = rbr_formats::stage_files::in_dir(folder, "Objects.zip")
            .ok_or_else(|| format!("{}: no Objects.zip", folder.display()))?;
        let open = || rbz::open(&objects_zip).map_err(|e| format!("{}: {e}", objects_zip.display()));
        let mut zip = open()?;
        let mut mesh_files: Vec<&str> = Vec::new();
        let mut mesh_of: HashMap<String, usize> = HashMap::new();
        let mut placed = Vec::new();
        for object in &objects {
            // The plugin skips an object whose mesh file the archive lacks (`Anavasi Portarias`'s `obj_242_0`).
            if !has_entry(&mut zip, &format!("Objects/{}.x", object.file)) {
                continue;
            }
            let mesh = *mesh_of.entry(object.file.to_lowercase()).or_insert_with(|| {
                mesh_files.push(&object.file);
                mesh_files.len() - 1
            });
            let bytes = read_entry(&mut zip, &format!("Objects/{}.xform", object.xform))?;
            let placements = xform::parse(&bytes).map_err(|e| format!("{}.xform: {e}", object.xform))?;
            placed.push(Object {
                mesh,
                placements,
                lod_in: object.lod_in.unwrap_or(0.0) as f32,
                lod_out: object.lod_out.map_or(f32::INFINITY, |d| d as f32),
                visible: object.visible.unwrap_or(true),
                collides: object.collision_model == Some(0) && (object.visible != Some(false) || object.collision_only),
                shadow_caster: object.shadow_caster.unwrap_or(false),
                sun_cascades: object.sun_cascades,
                instanced: object.instanced,
            });
        }
        crate::phases::mark("btb: Objects.zip entries + .xform parse");
        let meshes = parse_meshes(&mesh_files, open, &by_name)?;
        crate::phases::mark("btb: .x meshes (read + parse)");

        let m = spawn.matrix;
        Ok(Scene {
            name: track.name,
            spawn: Spawn { position: [m[12] as f32, m[13] as f32, m[14] as f32], yaw: spawn.angles[2] as f32 },
            driveline: driveline
                .points
                .iter()
                .map(|p| [p.position[0] as f32, p.position[2] as f32, p.position[1] as f32])
                .collect(),
            sun_direction: sun.map(|s| s.map(|v| v as f32)),
            grass,
            air,
            materials,
            meshes,
            objects: placed,
            textures: files,
        })
    }

    /// The texture file's [`Alpha`] (unreadable counts as translucent).
    pub fn texture_alpha(&self, file: &str) -> Alpha {
        self.textures.read(file).map_or(Alpha::Translucent, |bytes| alpha(&bytes))
    }
}

/// A track folder's `Textures.zip`.
#[derive(Debug, Clone)]
pub struct TextureFiles(PathBuf);

impl TextureFiles {
    /// The archive of `folder`.
    pub fn open(folder: &Path) -> Result<TextureFiles, String> {
        rbr_formats::stage_files::in_dir(folder, "Textures.zip")
            .map(TextureFiles)
            .ok_or_else(|| format!("{}: no Textures.zip", folder.display()))
    }

    /// The archive's bytes of `file`, as stored.
    pub fn bytes(&self, file: &str) -> Option<Vec<u8>> {
        read_entry(&mut rbz::open(&self.0).ok()?, &format!("Textures/{file}")).ok()
    }

    /// The texture file (a name from a [`Material`]) as a `.dds` file: the archive's own (plain-texel layouts converted
    /// to RGBA), or its `.jpg`, `.png` or `.bmp` decoded to a `.dds` of plain texels (a third of the install's textures
    /// are JPEGs; Crastazza's ground is a BMP). `None` if it is missing or does not decode.
    pub fn read(&self, file: &str) -> Option<Vec<u8>> {
        let mut zip = rbz::open(&self.0).ok()?;
        let bytes = read_entry(&mut zip, &format!("Textures/{file}")).ok()?;
        if bytes.starts_with(b"DDS ") {
            return Some(dds::to_rgba8(&bytes).unwrap_or(bytes));
        }
        let (width, height, pixels) = decode_image(&bytes)?;
        Some(dds::rgba8_file(width, height, &pixels))
    }
}

/// A `.jpg`, `.png` or `.bmp` file's width, height and RGBA texels; `None` for another file or one that does not decode.
pub fn decode_image(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    match bytes.get(..4)? {
        [0xff, 0xd8, ..] => {
            let options = zune_core::options::DecoderOptions::default()
                .jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGBA);
            let mut decoder =
                zune_jpeg::JpegDecoder::new_with_options(zune_core::bytestream::ZCursor::new(bytes), options);
            let pixels = decoder.decode().ok()?;
            let (width, height) = decoder.dimensions()?;
            Some((width as u32, height as u32, pixels))
        }
        [0x89, b'P', b'N', b'G'] => decode_png(bytes).ok(),
        [b'B', b'M', ..] => bmp(bytes),
        _ => None,
    }
}

/// A PNG's width, height and RGBA texels; an image without alpha reads as opaque.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), Box<dyn std::error::Error>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buffer)?;
    let pixels: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buffer[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            buffer[..info.buffer_size()].chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
        }
        png::ColorType::GrayscaleAlpha => {
            buffer[..info.buffer_size()].chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        png::ColorType::Grayscale => buffer[..info.buffer_size()].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        // `normalize_to_color8` expands palettes.
        png::ColorType::Indexed => return Err("indexed PNG after expansion".into()),
    };
    Ok((info.width, info.height, pixels))
}

/// An uncompressed 24- or 32-bit Windows bitmap's width, height and RGBA texels; `None` for the other layouts.
fn bmp(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let (offset, width, height) = (u32_at(10)? as usize, u32_at(18)? as i32, u32_at(22)? as i32);
    let (bits, compression) = (u16_at(28)?, u32_at(30)?);
    if width <= 0 || height == 0 || compression != 0 || !matches!(bits, 24 | 32) {
        return None;
    }
    let (width, rows) = (width as usize, height.unsigned_abs() as usize);
    let texel = usize::from(bits / 8);
    let stride = (width * texel).div_ceil(4) * 4;
    let mut pixels = Vec::with_capacity(width * rows * 4);
    for y in 0..rows {
        // Bottom-up unless the height is negative.
        let row = if height > 0 { rows - 1 - y } else { y };
        let start = offset + row * stride;
        let line = bytes.get(start..start + width * texel)?;
        pixels.extend(line.chunks_exact(texel).flat_map(|p| [p[2], p[1], p[0], 255]));
    }
    Some((width as u32, rows as u32, pixels))
}

/// How a texture's alpha covers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alpha {
    /// Every texel opaque.
    Opaque,
    /// Opaque and clear texels with a thin fringe between them: leaves, grass, fences.
    Cutout,
    /// Mostly part-transparent texels: glass, smoke, soft decals.
    Translucent,
}

/// The [`Alpha`] of the `.dds` `bytes`' first level, from its texels' alpha: translucent where the part-transparent
/// ones (6 to 249) outnumber the clear ones. Unreadable counts as translucent.
pub fn alpha(bytes: &[u8]) -> Alpha {
    let Ok(dds) = dds::parse(bytes) else { return Alpha::Translucent };
    let Some(level) = dds.levels().into_iter().next() else { return Alpha::Translucent };
    let (mut clear, mut part) = (0usize, 0usize);
    let mut count = |a: u8| match a {
        0..=5 => clear += 1,
        250..=255 => {}
        _ => part += 1,
    };
    match dds.format {
        dds::Format::Rgba8 => level.data.chunks_exact(4).for_each(|texel| count(texel[3])),
        dds::Format::Dxt3 => {
            for block in level.data.chunks_exact(16) {
                block[..8].iter().flat_map(|&b| [b & 15, b >> 4]).for_each(|a| count(a * 17));
            }
        }
        dds::Format::Dxt5 => {
            for block in level.data.chunks_exact(16) {
                let (a0, a1) = (u16::from(block[0]), u16::from(block[1]));
                let palette: [u8; 8] = std::array::from_fn(|i| {
                    let i = i as u16;
                    (match i {
                        0 => a0,
                        1 => a1,
                        _ if a0 > a1 => ((8 - i) * a0 + (i - 1) * a1) / 7,
                        6 => 0,
                        7 => 255,
                        _ => ((6 - i) * a0 + (i - 1) * a1) / 5,
                    }) as u8
                });
                let bits = block[2..8].iter().rev().fold(0u64, |bits, &b| bits << 8 | u64::from(b));
                (0..16).for_each(|t| count(palette[(bits >> (3 * t) & 7) as usize]));
            }
        }
        // BC7's alpha is not decoded on the CPU: taken as opaque.
        dds::Format::Bc7 => {}
        // A block with `c0 <= c1` and an index of 3 is transparent.
        dds::Format::Dxt1 => {
            for block in level.data.chunks_exact(8) {
                let c0 = u16::from_le_bytes([block[0], block[1]]);
                let c1 = u16::from_le_bytes([block[2], block[3]]);
                if c0 <= c1 {
                    clear +=
                        block[4..8].iter().map(|&b| (0..4).filter(|i| (b >> (2 * i)) & 3 == 3).count()).sum::<usize>();
                }
            }
        }
    }
    if clear + part == 0 {
        Alpha::Opaque
    } else if part > clear {
        Alpha::Translucent
    } else {
        Alpha::Cutout
    }
}

/// `p * M` for a row-major Direct3D matrix.
pub fn place_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|c| p[0] * m[c] + p[1] * m[4 + c] + p[2] * m[8 + c] + m[12 + c])
}

/// `n * M` without the translation, normalised: the effects' `normalize(mul(N, (float3x4)world))`.
pub fn place_normal(m: &[f32; 16], n: [f32; 3]) -> [f32; 3] {
    let v = [0, 1, 2].map(|c| n[0] * m[c] + n[1] * m[4 + c] + n[2] * m[8 + c]);
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length > 0.0 {
        v.map(|c| c / length)
    } else {
        v
    }
}

/// Whether the archive holds the entry.
pub fn has_entry<R: Read + std::io::Seek>(zip: &mut rbz::ZipArchive<R>, name: &str) -> bool {
    rbz::by_name(zip, name).is_ok()
}

pub fn read_entry<R: Read + std::io::Seek>(zip: &mut rbz::ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    rbz::by_name(zip, name)
        .map_err(|e| format!("{name}: {e}"))?
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(bytes)
}

/// The names `Textures.zip` holds, by lower-case file name.
struct Textures(HashMap<String, String>);

impl Textures {
    fn list(path: &Path) -> Result<Textures, String> {
        let zip = rbz::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut files = HashMap::new();
        for name in zip.file_names().flatten() {
            let file = name.rsplit(['/', '\\']).next().unwrap_or(&name);
            if !file.is_empty() {
                files.insert(file.to_lowercase(), file.to_owned());
            }
        }
        Ok(Textures(files))
    }

    /// The archive's file for an author's path (`C:\...\Textures\ground.dds`): matched by file name, ignoring case,
    /// and by stem where the archive holds the `.dds` of another extension (`SKY_01.JPG`).
    fn find(&self, path: &str) -> Option<String> {
        let file = path.rsplit(['/', '\\']).next()?.to_lowercase();
        let stem = file.rsplit_once('.').map_or(file.as_str(), |(stem, _)| stem);
        self.0.get(&file).or_else(|| self.0.get(&format!("{stem}.dds"))).cloned()
    }
}

fn material(m: &track::Material, textures: &Textures) -> Material {
    let find = |key: &str| m.texture(key).and_then(|path| textures.find(path));
    let color = |i: usize| m.specular_color.get(i).copied().unwrap_or(0.0) as f32;
    let own = m.entry("ReRbrEffect");
    // `u,v` or one number for both; anything else is the default.
    let bump_scale = match m.entry("ReRbrBumpScale").map(track::decimal_floats).as_deref() {
        Some(&[u, v]) => [u as f32, v as f32],
        Some(&[s]) => [s as f32; 2],
        _ => [1.0; 2],
    };
    let light = match m.entry("ReRbrLight").map(track::decimal_floats).as_deref() {
        Some(&[a, d]) => [a as f32, d as f32],
        _ => [1.0; 2],
    };
    Material {
        layers: own.and_then(|_| layers(m, textures)),
        name: m.name.clone(),
        effect: Effect::parse(own.or(m.effect.as_deref())),
        first_uv: own.is_some(),
        technique: Technique::parse(m.technique.as_deref()),
        specular_power: m.specular_power.unwrap_or(4.0) as f32,
        specular_color: [color(0), color(1), color(2)],
        diffuse: find("diffuseTexture"),
        second: find("secondDiffuseTexture"),
        normal: find("normalTexture"),
        specular: find("specularTexture"),
        multiplier: find("multiplierTexture"),
        additive: find("additiveTexture"),
        bump_scale,
        light,
        bump_uv: own.is_some() && m.entry("ReRbrBumpUv").and_then(track::decimal_float) == Some(1.0),
        sheen: own
            .and_then(|_| m.entry("ReRbrSheen").and_then(track::decimal_float))
            .map(|r| r as f32)
            .filter(|r| r.is_finite() && *r > 0.0),
        grass: match m.entry("ReRbrGrass") {
            Some("1") => Grass::Grows(m.entry("ReRbrGrassProfile").and_then(|p| p.trim().parse().ok()).unwrap_or(0)),
            Some("hide") => Grass::Hides,
            Some("hide-alpha") => Grass::HidesAlpha,
            _ => Grass::None,
        },
    }
}

/// The `ReRbrLayer*` keys of `m`: `None` unless the diffuse, the mask and the four details are in the archive and the
/// scales are eight finite numbers, none negative.
fn layers(m: &track::Material, textures: &Textures) -> Option<Layers> {
    let file = |name: &str| textures.find(m.entry(name)?);
    let details: Vec<String> =
        m.entry("ReRbrLayerDetail")?.split(',').map(|n| textures.find(n.trim())).collect::<Option<_>>()?;
    let scale = track::decimal_floats(m.entry("ReRbrLayerScale")?);
    if scale.len() != 8 || scale.iter().any(|s| !s.is_finite() || *s < 0.0) {
        return None;
    }
    let world = m.entry("ReRbrLayerWorld").map(track::decimal_floats);
    let uv = m.entry("ReRbrLayerUv").and_then(track::decimal_float) == Some(1.0);
    let diffuse = file("ReRbrLayerDiffuse")?;
    Some(Layers {
        mask: if uv { diffuse.clone() } else { file("ReRbrLayerMask")? },
        diffuse,
        uv,
        details: details.try_into().ok()?,
        scale: std::array::from_fn(|i| [scale[2 * i] as f32, scale[2 * i + 1] as f32]),
        blend: m
            .entry("ReRbrLayerBlend")
            .and_then(track::decimal_float)
            .filter(|s| uv && s.is_finite() && *s >= 0.0)
            .map(|s| s as f32),
        magic: m.entry("ReRbrLayerMagic").and_then(track::decimal_float).map_or(1.0, |v| v as f32),
        world: match world.as_deref() {
            None => None,
            Some(&[x, z]) => Some([x, z]),
            Some(_) => return None,
        },
    })
}

/// The meshes of `files` (`Objects/<file>.x`), in order, read and parsed on every core, each worker with its own handle
/// on the archive from `open`: most of a large stage's load (Mont Ventoux's 16,494 files, 1 GB, take 22 s on one).
/// The error of the first file that fails.
fn parse_meshes<R: Read + std::io::Seek>(
    files: &[&str],
    open: impl Fn() -> Result<rbz::ZipArchive<R>, String> + Sync,
    materials: &HashMap<String, usize>,
) -> Result<Vec<Mesh>, String> {
    let threads = std::thread::available_parallelism().map_or(4, usize::from).min(files.len()).max(1);
    let parse = |zip: &mut rbz::ZipArchive<R>, file: &str| {
        mesh_of_x(file, &read_entry(zip, &format!("Objects/{file}.x"))?, materials)
    };
    let mut parsed: Vec<(usize, Result<Mesh, String>)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|t| {
                let (open, parse) = (&open, &parse);
                scope.spawn(move || match open() {
                    Ok(mut zip) => (t..files.len()).step_by(threads).map(|i| (i, parse(&mut zip, files[i]))).collect(),
                    Err(e) => vec![(t, Err(e))],
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().expect("mesh worker panicked")).collect()
    });
    parsed.sort_by_key(|(i, _)| *i);
    parsed.into_iter().map(|(_, mesh)| mesh).collect()
}

/// The `.x` file as a [`Mesh`].
fn mesh_of_x(name: &str, bytes: &[u8], materials: &HashMap<String, usize>) -> Result<Mesh, String> {
    let fail = |e: xfile::Error| format!("{name}.x: {e}");
    let file = xfile::parse(bytes).map_err(fail)?;
    let root = *file.objects_of_type("Mesh").first().ok_or_else(|| format!("{name}.x: no Mesh"))?;
    let mesh = xfile::Mesh::decode(root, &file).map_err(fail)?;
    let count = mesh.positions.len();

    // Vertices are a position and a normal index: split where the file indexes them apart.
    let mut vertices: Vec<(u32, Option<u32>)> = Vec::new();
    let mut seen: HashMap<(u32, Option<u32>), u32> = HashMap::new();
    let mut indices = Vec::new();
    for (face, polygon) in mesh.faces.iter().enumerate() {
        let normal_face = mesh.normals.as_ref().and_then(|n| n.faces.get(face));
        for triangle in polygon.triangles() {
            for corner in triangle {
                let slot = polygon.0.iter().position(|&p| p == corner).unwrap_or(0);
                let normal = normal_face.and_then(|f| f.0.get(slot)).copied();
                let id = *seen.entry((corner, normal)).or_insert_with(|| {
                    vertices.push((corner, normal));
                    vertices.len() as u32 - 1
                });
                indices.push(id);
            }
        }
    }
    let by_position = |data: &Vec<[f32; 2]>| -> Vec<[f32; 2]> {
        vertices.iter().map(|&(p, _)| data.get(p as usize).copied().unwrap_or([0.0; 2])).collect()
    };
    let positions =
        vertices.iter().map(|&(p, _)| mesh.positions.get(p as usize).copied().unwrap_or([0.0; 3])).collect();
    let normals = vertices
        .iter()
        .map(|&(p, n)| match (&mesh.normals, n) {
            (Some(normals), Some(n)) => normals.normals.get(n as usize).copied().unwrap_or([0.0, 1.0, 0.0]),
            (Some(normals), None) => normals.normals.get(p as usize).copied().unwrap_or([0.0, 1.0, 0.0]),
            (None, _) => [0.0, 1.0, 0.0],
        })
        .collect();
    let mut uvs: Vec<Vec<[f32; 2]>> = mesh.uv_sets.iter().map(by_position).collect();

    // The extra vertex streams: `DeclData` elements (tangent 6, binormal 7, blend weight 1, texcoord 5 with its
    // index) and `FVFData` UV sets, one block of dwords per vertex.
    let mut tangents = Vec::new();
    let mut binormals = Vec::new();
    let mut blend = Vec::new();
    let mut extra_uvs: Vec<(u32, Vec<[f32; 2]>)> = Vec::new();
    for decl in &mesh.decl {
        let Some(stride) = decl.stride_dwords().filter(|&s| s > 0 && decl.data.len() == s * count) else { continue };
        let mut offset = 0;
        for element in &decl.elements {
            let width = element.width_dwords().unwrap_or(0);
            let at = |v: u32, i: usize| f32::from_bits(decl.data[v as usize * stride + offset + i]);
            let per_vertex =
                |f: &dyn Fn(u32) -> Vec<f32>| -> Vec<Vec<f32>> { vertices.iter().map(|&(p, _)| f(p)).collect() };
            let read = |n: usize| per_vertex(&|v| (0..n).map(|i| at(v, i)).collect());
            match (element.usage, element.ty) {
                (6, 2) => tangents = read(3).iter().map(|v| [v[0], v[1], v[2]]).collect(),
                (7, 2) => binormals = read(3).iter().map(|v| [v[0], v[1], v[2]]).collect(),
                (1, 0) => blend = read(1).iter().map(|v| v[0]).collect(),
                (5, 1) => extra_uvs.push((element.usage_index, read(2).iter().map(|v| [v[0], v[1]]).collect())),
                _ => {}
            }
            offset += width;
        }
    }
    for fvf in &mesh.fvf {
        let sets = fvf.uv_set_count();
        if sets == 0 || fvf.data.len() != sets * 2 * count {
            continue;
        }
        for set in 0..sets {
            let data: Vec<[f32; 2]> = (0..count)
                .map(|v| {
                    let at = (v * sets + set) * 2;
                    [f32::from_bits(fvf.data[at]), f32::from_bits(fvf.data[at + 1])]
                })
                .collect();
            extra_uvs.push((set as u32 + 1, by_position(&data)));
        }
    }
    extra_uvs.sort_by_key(|(index, _)| *index);
    uvs.extend(extra_uvs.into_iter().map(|(_, uv)| uv));

    let texture = mesh.materials.as_ref().and_then(|l| l.materials.first()).and_then(|m| m.texture.as_deref());
    Ok(Mesh {
        name: name.to_owned(),
        positions,
        normals,
        tangents,
        binormals,
        uvs,
        blend,
        indices,
        material: texture.and_then(|t| materials.get(&t.to_lowercase()).copied()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn has_entry_tells_a_missing_mesh_file_from_a_present_one() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut bytes);
        writer.start_file("Objects/Here.x", zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(b"x").unwrap();
        writer.finish().unwrap();
        let mut zip = rbz::ZipArchive::new(bytes).unwrap();
        assert!(has_entry(&mut zip, "Objects/here.x"));
        assert!(!has_entry(&mut zip, "Objects/obj_242_0.x"));
    }

    #[test]
    fn the_bump_scale_reads_a_pair_one_number_or_its_default() {
        let section = |entries: &[(&str, &str)]| track::Material {
            name: "m".into(),
            effect: Some("BTB_Diffuse".into()),
            technique: None,
            ambient: Vec::new(),
            specular_power: None,
            specular_color: Vec::new(),
            entries: entries.iter().map(|&(k, v)| (k.to_owned(), v.to_owned())).collect(),
        };
        let textures = Textures(Default::default());
        let scale = |entries: &[(&str, &str)]| material(&section(entries), &textures).bump_scale;
        assert_eq!(scale(&[]), [1.0; 2]);
        assert_eq!(scale(&[("ReRbrBumpScale", "2000.0,4000.5")]), [2000.0, 4000.5]);
        assert_eq!(scale(&[("ReRbrBumpScale", "3.5")]), [3.5; 2]);
    }

    #[test]
    fn the_light_factors_read_a_pair_or_default_to_one() {
        let section = |entries: &[(&str, &str)]| track::Material {
            name: "m".into(),
            effect: Some("BTB_Diffuse".into()),
            technique: None,
            ambient: Vec::new(),
            specular_power: None,
            specular_color: Vec::new(),
            entries: entries.iter().map(|&(k, v)| (k.to_owned(), v.to_owned())).collect(),
        };
        let textures = Textures(Default::default());
        let light = |entries: &[(&str, &str)]| material(&section(entries), &textures).light;
        assert_eq!(light(&[]), [1.0; 2]);
        assert_eq!(light(&[("ReRbrLight", "0.5,1.5")]), [0.5, 1.5]);
        assert_eq!(light(&[("ReRbrLight", "0.5")]), [1.0; 2]);
    }

    #[test]
    fn the_sheen_and_bump_uv_keys_need_the_rerbr_effect() {
        let section = |own: bool, entries: &[(&str, &str)]| {
            let mut entries: Vec<_> = entries.iter().map(|&(k, v)| (k.to_owned(), v.to_owned())).collect();
            if own {
                entries.push(("ReRbrEffect".into(), "BTB_Diffuse".into()));
            }
            track::Material {
                name: "m".into(),
                effect: Some("BTB_Diffuse".into()),
                technique: None,
                ambient: Vec::new(),
                specular_power: None,
                specular_color: Vec::new(),
                entries,
            }
        };
        let textures = Textures(Default::default());
        let read = |own, entries: &[(&str, &str)]| {
            let m = material(&section(own, entries), &textures);
            (m.sheen, m.bump_uv)
        };
        assert_eq!(read(true, &[]), (None, false));
        assert_eq!(read(true, &[("ReRbrSheen", "0.75"), ("ReRbrBumpUv", "1")]), (Some(0.75), true));
        assert_eq!(read(false, &[("ReRbrSheen", "0.75"), ("ReRbrBumpUv", "1")]), (None, false));
        assert_eq!(read(true, &[("ReRbrSheen", "0.0")]).0, None);
    }

    #[test]
    fn the_layer_keys_need_the_rerbr_effect_and_every_texture() {
        let section = |own: bool, extra: &[(&str, &str)]| {
            let mut entries: Vec<(String, String)> = [
                ("ReRbrLayerDiffuse", "d.dds"),
                ("ReRbrLayerMask", "M.dds"),
                ("ReRbrLayerDetail", "a.dds,b.dds,a.dds,c.dds"),
                ("ReRbrLayerScale", "1.0,1.0,2.5,2.5,0.0,0.0,4.0,8.0"),
                ("ReRbrLayerMagic", "1.5"),
            ]
            .iter()
            .chain(extra)
            .map(|&(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
            if own {
                entries.push(("ReRbrEffect".into(), "BTB_Diffuse".into()));
            }
            track::Material {
                name: "m".into(),
                effect: Some("BTB_Diffuse".into()),
                technique: None,
                ambient: Vec::new(),
                specular_power: None,
                specular_color: Vec::new(),
                entries,
            }
        };
        let names = ["d.dds", "m.dds", "a.dds", "b.dds", "c.dds"];
        let textures = Textures(names.iter().map(|n| (n.to_string(), n.to_string())).collect());
        let uv = material(&section(true, &[]), &textures).layers.unwrap();
        assert_eq!((uv.mask.as_str(), uv.details[2].as_str(), uv.magic, uv.world), ("m.dds", "a.dds", 1.5, None));
        assert_eq!(uv.scale, [[1.0, 1.0], [2.5, 2.5], [0.0, 0.0], [4.0, 8.0]]);
        let world = material(&section(true, &[("ReRbrLayerWorld", "-1234.5,20.25")]), &textures).layers.unwrap();
        assert_eq!(world.world, Some([-1234.5, 20.25]));
        assert!(material(&section(false, &[]), &textures).layers.is_none(), "needs ReRbrEffect");
        let missing = Textures(names[..4].iter().map(|n| (n.to_string(), n.to_string())).collect());
        assert!(material(&section(true, &[]), &missing).layers.is_none(), "a detail is not in the archive");
        assert!(material(&section(true, &[("ReRbrLayerWorld", "1.0")]), &textures).layers.is_none());
        // On their own coordinates the layers need no mask file.
        let no_mask =
            Textures(["d.dds", "a.dds", "b.dds", "c.dds"].iter().map(|n| (n.to_string(), n.to_string())).collect());
        let mut own = section(true, &[("ReRbrLayerUv", "1")]);
        own.entries.retain(|(k, _)| k != "ReRbrLayerMask");
        let layers = material(&own, &no_mask).layers.unwrap();
        assert!(layers.uv && layers.mask == layers.diffuse);
        assert!(!uv.uv);
    }
}
