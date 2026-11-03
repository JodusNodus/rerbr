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
    pub effect: Effect,
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
    pub shadow_caster: bool,
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
        let sun = track.settings()?.sun_direction;
        let objects = track.objects()?;
        let files = TextureFiles::open(folder)?;
        let textures = Textures::list(&files.0)?;

        let sections = track.materials()?.sections;
        let materials: Vec<Material> = sections.iter().map(|m| material(m, &textures)).collect();
        let by_name: HashMap<String, usize> =
            materials.iter().enumerate().map(|(i, m)| (m.name.to_lowercase(), i)).collect();

        let objects_zip = rbr_formats::stage_files::in_dir(folder, "Objects.zip")
            .ok_or_else(|| format!("{}: no Objects.zip", folder.display()))?;
        let mut zip = rbz::open(&objects_zip).map_err(|e| format!("{}: {e}", objects_zip.display()))?;
        let mut meshes: Vec<Mesh> = Vec::new();
        let mut mesh_of: HashMap<String, usize> = HashMap::new();
        let mut placed = Vec::new();
        for object in &objects {
            let key = object.file.to_lowercase();
            let mesh = match mesh_of.get(&key) {
                Some(&mesh) => mesh,
                None => {
                    let bytes = read_entry(&mut zip, &format!("Objects/{}.x", object.file))?;
                    meshes.push(mesh_of_x(&object.file, &bytes, &by_name)?);
                    mesh_of.insert(key, meshes.len() - 1);
                    meshes.len() - 1
                }
            };
            let bytes = read_entry(&mut zip, &format!("Objects/{}.xform", object.xform))?;
            let placements = xform::parse(&bytes).map_err(|e| format!("{}.xform: {e}", object.xform))?;
            placed.push(Object {
                mesh,
                placements,
                lod_in: object.lod_in.unwrap_or(0.0) as f32,
                lod_out: object.lod_out.map_or(f32::INFINITY, |d| d as f32),
                visible: object.visible.unwrap_or(true),
                shadow_caster: object.shadow_caster.unwrap_or(false),
            });
        }

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
        [0x89, b'P', b'N', b'G'] => {
            let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
            decoder.set_transformations(png::Transformations::normalize_to_color8());
            let mut reader = decoder.read_info().ok()?;
            let mut buffer = vec![0; reader.output_buffer_size()?];
            let info = reader.next_frame(&mut buffer).ok()?;
            let pixels: Vec<u8> = match info.color_type {
                png::ColorType::Rgba => buffer[..info.buffer_size()].to_vec(),
                png::ColorType::Rgb => {
                    buffer[..info.buffer_size()].chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
                }
                png::ColorType::GrayscaleAlpha => {
                    buffer[..info.buffer_size()].chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
                }
                png::ColorType::Grayscale => {
                    buffer[..info.buffer_size()].iter().flat_map(|&g| [g, g, g, 255]).collect()
                }
                // `normalize_to_color8` expands palettes.
                png::ColorType::Indexed => return None,
            };
            Some((info.width, info.height, pixels))
        }
        [b'B', b'M', ..] => bmp(bytes),
        _ => None,
    }
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
    Material {
        name: m.name.clone(),
        effect: Effect::parse(m.effect.as_deref()),
        technique: Technique::parse(m.technique.as_deref()),
        specular_power: m.specular_power.unwrap_or(4.0) as f32,
        specular_color: [color(0), color(1), color(2)],
        diffuse: find("diffuseTexture"),
        second: find("secondDiffuseTexture"),
        normal: find("normalTexture"),
        specular: find("specularTexture"),
        multiplier: find("multiplierTexture"),
        additive: find("additiveTexture"),
    }
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
