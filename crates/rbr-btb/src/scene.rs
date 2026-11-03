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

    /// Whether the texture file has any texel that is not opaque (unreadable counts as yes): only then does a
    /// [`Technique::transparent`] material need its blended pass.
    pub fn texture_has_alpha(&self, file: &str) -> bool {
        self.textures.read(file).is_none_or(|bytes| has_alpha(&bytes))
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

    /// The texture file (a name from a [`Material`]) as a `.dds` file: the archive's own (plain-texel layouts converted to RGBA), or its `.jpg` or `.png`
    /// decoded to a `.dds` of plain texels (a third of the install's textures are JPEGs). `None` if it is missing
    /// or does not decode.
    pub fn read(&self, file: &str) -> Option<Vec<u8>> {
        let mut zip = rbz::open(&self.0).ok()?;
        let bytes = read_entry(&mut zip, &format!("Textures/{file}")).ok()?;
        match bytes.get(..4)? {
            b"DDS " => Some(dds::to_rgba8(&bytes).unwrap_or(bytes)),
            [0xff, 0xd8, ..] => {
                let options = zune_core::options::DecoderOptions::default()
                    .jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGBA);
                let mut decoder =
                    zune_jpeg::JpegDecoder::new_with_options(zune_core::bytestream::ZCursor::new(&bytes), options);
                let pixels = decoder.decode().ok()?;
                let (width, height) = decoder.dimensions()?;
                Some(dds::rgba8_file(width as u32, height as u32, &pixels))
            }
            [0x89, b'P', b'N', b'G'] => {
                let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
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
                    png::ColorType::Indexed => return None,
                };
                Some(dds::rgba8_file(info.width, info.height, &pixels))
            }
            _ => None,
        }
    }
}

/// Whether the `.dds` `bytes` has a texel with alpha under 255 in its first level; unreadable counts as yes.
pub fn has_alpha(bytes: &[u8]) -> bool {
    let Ok(dds) = dds::parse(bytes) else { return true };
    let Some(level) = dds.levels().into_iter().next() else { return true };
    match dds.format {
        dds::Format::Rgba8 => level.data.chunks_exact(4).any(|texel| texel[3] != 255),
        dds::Format::Dxt3 => level.data.chunks_exact(16).any(|block| block[..8].iter().any(|&b| b != 255)),
        // Alpha endpoints at 255 interpolate to 255.
        dds::Format::Dxt5 => level.data.chunks_exact(16).any(|block| block[0] != 255 || block[1] != 255),
        // A block with `c0 <= c1` and an index of 3 is transparent.
        dds::Format::Dxt1 => level.data.chunks_exact(8).any(|block| {
            let c0 = u16::from_le_bytes([block[0], block[1]]);
            let c1 = u16::from_le_bytes([block[2], block[3]]);
            c0 <= c1 && block[4..8].iter().any(|&b| (0..4).any(|i| (b >> (2 * i)) & 3 == 3))
        }),
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

fn read_entry<R: Read + std::io::Seek>(zip: &mut rbz::ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
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
