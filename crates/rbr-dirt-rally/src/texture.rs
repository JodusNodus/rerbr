//! Textures of a PSSG (`TEXTURE` nodes: DXT1 or DXT5 mip chains) and the library that finds the real one of a material's
//! reference among the files of a route.
//!
//! The files hold stubs (4x4 DXT1) for the textures that ship elsewhere: terrain lightmaps (`x_LM.tga` in `tracksplit`,
//! real as `x_LM_r0_day.tga` in `terrain_lm_<time>`) and object textures (`objectstextures`, real in `patchup_ot`).
use crate::install::Route;
use crate::pssg::{Node, Pssg};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Dxt1,
    Dxt5,
    /// One byte a texel (the cars' bar graph masks, `u8`).
    U8,
}

impl Format {
    /// Bytes of one mip level.
    fn level_bytes(self, width: u32, height: u32) -> usize {
        let blocks = (width as usize).div_ceil(4).max(1) * (height as usize).div_ceil(4).max(1);
        match self {
            Format::Dxt1 => blocks * 8,
            Format::Dxt5 => blocks * 16,
            Format::U8 => width as usize * height as usize,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// As written in the file, `gre_ghy_ral_set1_06_d.tga`.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    /// Mip levels held in `data`, largest first, each `max(1, w >> i)` by `max(1, h >> i)` in 4x4 blocks.
    pub levels: u32,
    pub data: Vec<u8>,
}

impl Texture {
    /// A `TEXTURE` node; `None` for formats other than DXT1 and DXT5 or damaged sizes.
    ///
    /// Non-square textures carry some bytes after the standard chain (127 blocks for 1024x512, 255 for 2048x1024: not a
    /// level, the levels before them decode as the downsampled base); they are left out.
    fn from_node(node: Node) -> Option<Texture> {
        Texture::from_node_in(node, false)
    }

    /// [`Texture::from_node`], also taking the car files' formats when `car`: the `_srgb` ones (the liveries: the same
    /// blocks, tagged for sampling as sRGB) and `u8` (the interiors' bar graph masks).
    fn from_node_in(node: Node, car: bool) -> Option<Texture> {
        let format = match node.str("texelFormat")? {
            "dxt1" => Format::Dxt1,
            "dxt5" => Format::Dxt5,
            "dxt1_srgb" if car => Format::Dxt1,
            "dxt5_srgb" if car => Format::Dxt5,
            "u8" if car => Format::U8,
            _ => return None,
        };
        let (width, height) = (node.u32("width")?, node.u32("height")?);
        let stored = node.child("TEXTUREIMAGEBLOCK")?.child_payload("TEXTUREIMAGEBLOCKDATA");
        let (mut levels, mut size) = (0, 0);
        for i in 0..=node.u32("numberMipMapLevels")? {
            let next = size + format.level_bytes((width >> i).max(1), (height >> i).max(1));
            if next > stored.len() {
                break;
            }
            (levels, size) = (i + 1, next);
        }
        let name = node.id()?.to_owned();
        (levels > 0).then(|| Texture { name, width, height, format, levels, data: stored[..size].to_vec() })
    }

    /// The 4x4 stub that stands in for a texture shipped in another file.
    pub fn is_placeholder(&self) -> bool {
        self.width <= 4 && self.height <= 4
    }

    /// A DDS file of every level held.
    pub fn dds(&self) -> Vec<u8> {
        if self.format == Format::U8 {
            return rbr_formats::dds::rgba8_file(self.width, self.height, &self.rgba());
        }
        let mut out = b"DDS ".to_vec();
        let mips = self.levels > 1;
        let fourcc: &[u8; 4] = if self.format == Format::Dxt1 { b"DXT1" } else { b"DXT5" };
        let flags = 0x1 | 0x2 | 0x4 | 0x1000 | 0x8_0000 | if mips { 0x2_0000 } else { 0 };
        let caps = 0x1000 | if mips { 0x8 | 0x40_0000 } else { 0 };
        let head: [u32; 31] = {
            let mut h = [0; 31];
            h[0] = 124;
            h[1] = flags;
            h[2] = self.height;
            h[3] = self.width;
            h[4] = self.format.level_bytes(self.width, self.height) as u32;
            h[6] = self.levels;
            h[18] = 32;
            h[19] = 0x4;
            h[20] = u32::from_le_bytes(*fourcc);
            h[26] = caps;
            h
        };
        out.extend(head.iter().flat_map(|v| v.to_le_bytes()));
        out.extend(&self.data);
        out
    }

    /// The base level as RGBA8 (a `u8` texture's byte in every channel).
    pub fn rgba(&self) -> Vec<u8> {
        let format = match self.format {
            Format::Dxt1 => texpresso::Format::Bc1,
            Format::Dxt5 => texpresso::Format::Bc3,
            Format::U8 => {
                let texels = &self.data[..self.format.level_bytes(self.width, self.height)];
                return texels.iter().flat_map(|&v| [v; 4]).collect();
            }
        };
        let (w, h) = (self.width as usize, self.height as usize);
        let mut out = vec![0; w * h * 4];
        format.decompress(&self.data[..self.format.level_bytes(self.width, self.height)], w, h, &mut out);
        out
    }

    /// A DXT5nm normal map (`TNormalMap`: alpha is x, green y up the image, red 1, blue 0) as the opaque `.dds` the
    /// renderer's bump reads: red x, green y along v (down the image), z rebuilt. The sign: integrating 44 terrain
    /// maps to heights, 35 correlate best with their diffuse (mortar and cracks low) with green up the image.
    pub fn normal_dds(&self) -> Vec<u8> {
        let mut rgba = self.rgba();
        rgba.chunks_exact_mut(4).for_each(|p| (p[0], p[1], p[2], p[3]) = (p[3], 255 - p[1], 255, 255));
        rbr_ac_tracks::btb::textures::encode(false, self.width, self.height, rgba)
    }

    /// A specular map (`TSpecularMap`: red the intensity) as a grey `.dds`, which the renderer's highlight takes
    /// per channel.
    pub fn specular_dds(&self) -> Vec<u8> {
        let mut rgba = self.rgba();
        rgba.chunks_exact_mut(4).for_each(|p| (p[1], p[2], p[3]) = (p[0], p[0], 255));
        rbr_ac_tracks::btb::textures::encode(false, self.width, self.height, rgba)
    }
}

/// The textures of a car's file: [`read`] with the `_srgb` formats.
pub fn read_car(pssg: &Pssg) -> Vec<Texture> {
    pssg.find("TEXTURE").filter_map(|n| Texture::from_node_in(n, true)).collect()
}

/// Every texture of the file (copies its data; a [`Library`] decodes one at a time).
pub fn read(pssg: &Pssg) -> Vec<Texture> {
    pssg.find("TEXTURE").filter_map(Texture::from_node).collect()
}

struct File {
    pssg: Pssg,
    /// Lower-cased texture name -> name as written.
    names: HashMap<String, String>,
}

/// Finds the real texture of a material's reference for a route: loaded lazily, names matched ignoring case.
pub struct Library {
    area: PathBuf,
    route: PathBuf,
    index: u32,
    time: String,
    files: HashMap<PathBuf, Option<File>>,
    cache: HashMap<String, Option<Rc<Texture>>>,
}

impl Library {
    /// `time` is the lightmap set of the route files: `day`, `day_alt`, `night`, `night_alt` (Finland also `wet`).
    pub fn new(route: &Route, time: &str) -> Library {
        Library {
            area: route.area_dir().to_owned(),
            route: route.dir.clone(),
            index: route.index,
            time: time.to_owned(),
            files: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    pub fn day(route: &Route) -> Library {
        Library::new(route, "day")
    }

    fn sponsors(&self) -> PathBuf {
        self.area.ancestors().nth(3).map(|tracks| tracks.join("sponsors")).unwrap_or_default()
    }

    /// The sponsor file standing for a pack (`sponsor_pack_a.pssg`, filled by the game at run time and not on disk):
    /// the pack's letter picks one of the installed sponsors, a quarter of the list apart and wrapping (Höljes has a
    /// pack `e`), so a stage shows real boards instead of `sponsor_fallback.pssg`'s green "FALLBACK".
    fn sponsor(&self, pack: &str) -> Option<String> {
        let letter = pack.strip_prefix("sponsor_pack_")?.chars().next()?;
        let mut names: Vec<String> = std::fs::read_dir(self.sponsors())
            .ok()?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".pssg") && n.starts_with("sponsor_") && !n.starts_with("sponsor_pack_"))
            .filter(|n| n != "sponsor_fallback.pssg")
            .collect();
        names.sort();
        let pack = (letter as usize).checked_sub('a' as usize)?;
        let at = (pack * (names.len() / 4 + 1)).checked_rem(names.len())?;
        names.get(at).cloned()
    }

    fn file(&mut self, name: &str) -> Option<&File> {
        let sponsors = self.sponsors();
        let path = [&self.route, &self.area, &sponsors].map(|d| d.join(name)).into_iter().find(|p| p.is_file())?;
        self.files
            .entry(path.clone())
            .or_insert_with(|| {
                let pssg = Pssg::read(&path).ok()?;
                let names =
                    pssg.find("TEXTURE").filter_map(|t| t.id()).map(|n| (n.to_lowercase(), n.to_owned())).collect();
                Some(File { pssg, names })
            })
            .as_ref()
    }

    /// The texture a reference (`tracksplit.pssg#x.tga`, `#x.tga` or `x.tga`) draws: the real one wherever it is, else
    /// the placeholder, else `None`.
    pub fn get(&mut self, reference: &str) -> Option<Rc<Texture>> {
        let key = reference.to_lowercase();
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let found = self.resolve(&key);
        self.cache.insert(key, found.clone());
        found
    }

    fn resolve(&mut self, reference: &str) -> Option<Rc<Texture>> {
        let (listed, name) = reference.split_once('#').unwrap_or(("", reference));
        let stem = name.strip_suffix(".tga").unwrap_or(name);
        let lightmap = stem.ends_with("_lm");
        let lm = [format!("terrain_lm_{}.pssg", self.time), format!("buildings_lm_{}.pssg", self.time)];
        let mut files: Vec<String> = if lightmap { lm.to_vec() } else { Vec::new() };
        // A sponsor reference lists its pack and the fallback: `sponsor_pack_a.pssg|sponsor_fallback.pssg#board.tga`.
        for file in listed.split('|').filter(|f| !f.is_empty()) {
            files.push(self.sponsor(file).unwrap_or_else(|| file.to_owned()));
        }
        files.extend(
            [
                "route_patchup_ot.pssg",
                "patchup_ot.pssg",
                "tracksplit.pssg",
                "treestextures.pssg",
                "route_objectstextures.pssg",
                "objectstextures.pssg",
            ]
            .map(String::from),
        );
        // The route's lightmap files also hold the `low_*` colour textures that are stubs in `tracksplit`.
        files.extend(lm.into_iter().filter(|_| !lightmap));
        let mut placeholder = None;
        for file in files {
            let wanted = {
                let (time, index) = (self.time.clone(), self.index);
                let Some(f) = self.file(&file) else { continue };
                let written = if lightmap && file.contains("_lm_") {
                    // `x_LM.tga` is `x_LM_r<n>_<time>.tga`, `n` the route's lightmap set (0 in every shipped route).
                    let (prefix, suffix) = (format!("{stem}_r"), format!("_{time}.tga"));
                    let mut named: Vec<&String> =
                        f.names.keys().filter(|k| k.starts_with(&prefix) && k.ends_with(&suffix)).collect();
                    named.sort_by_key(|k| !k.starts_with(&format!("{stem}_r{index}_")));
                    named.first().and_then(|k| f.names.get(*k))
                } else {
                    f.names.get(name)
                };
                written.and_then(|w| f.pssg.by_id(w)).and_then(Texture::from_node)
            };
            match wanted {
                Some(t) if !t.is_placeholder() => return Some(Rc::new(t)),
                Some(t) => placeholder = placeholder.or(Some(Rc::new(t))),
                None => {}
            }
        }
        placeholder
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::Install;
    use crate::material;

    fn texture(width: u32, height: u32, format: Format, levels: u32) -> Texture {
        let size: usize = (0..levels).map(|i| format.level_bytes((width >> i).max(1), (height >> i).max(1))).sum();
        Texture { name: "t.tga".into(), width, height, format, levels, data: vec![7; size] }
    }

    #[test]
    fn dds_header_and_chain() {
        let t = texture(8, 4, Format::Dxt5, 4);
        assert_eq!(t.data.len(), 2 * 16 + 16 + 16 + 16);
        let dds = t.dds();
        assert_eq!(&dds[..4], b"DDS ");
        let word = |i: usize| u32::from_le_bytes(dds[i..i + 4].try_into().unwrap());
        assert_eq!((word(12), word(16), word(20), word(28), word(84)), (4, 8, 32, 4, u32::from_le_bytes(*b"DXT5")));
        assert_eq!(dds.len(), 128 + t.data.len());
        assert!(!t.is_placeholder() && texture(4, 4, Format::Dxt1, 1).is_placeholder());
    }

    #[test]
    fn a_dxt5nm_normal_map_becomes_red_x_green_down() {
        // x in alpha (60), y up the image in green (200): red 60, green 255 - 200, opaque DXT1.
        let mut block = vec![0; 16];
        texpresso::Format::Bc3.compress(&[255, 200, 0, 60].repeat(16), 4, 4, texpresso::Params::default(), &mut block);
        let t = Texture { name: "n".into(), width: 4, height: 4, format: Format::Dxt5, levels: 1, data: block };
        let dds = t.normal_dds();
        assert_eq!(&dds[84..88], b"DXT1");
        let mut px = [0u8; 64];
        texpresso::Format::Bc1.decompress(&dds[128..136], 4, 4, &mut px);
        assert!(px[0].abs_diff(60) < 9 && px[1].abs_diff(55) < 9 && px[2] > 247 && px[3] == 255, "{:?}", &px[..4]);
        let grey = t.specular_dds();
        texpresso::Format::Bc1.decompress(&grey[128..136], 4, 4, &mut px);
        assert!(px[0].abs_diff(255) < 9 && px[0].abs_diff(px[1]) < 9 && px[0].abs_diff(px[2]) < 9);
    }

    #[test]
    fn rgba_decodes_a_block() {
        // One DXT1 block: both endpoints pure red (0xf800), all indices 0.
        let t = Texture {
            name: "r".into(),
            width: 4,
            height: 4,
            format: Format::Dxt1,
            levels: 1,
            data: vec![0, 0xf8, 0, 0xf8, 0, 0, 0, 0],
        };
        let px = t.rgba();
        assert_eq!(px.len(), 64);
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn every_texture_of_the_sample_routes_resolves() {
        let Some(game) = std::env::var_os("RBR_DIRT_RALLY") else { return };
        let install = Install::at(std::path::Path::new(&game)).unwrap();
        for id in ["greece/ghymno_rally_01/route_0", "finland/ouninpohja_rally_01/route_0"] {
            let started = std::time::Instant::now();
            let route = install.route(id).unwrap();
            let mut library = Library::day(&route);
            let (mut total, mut placeholders, mut missing) = (0, Vec::new(), Vec::new());
            let mut files = vec![
                route.area_dir().join("tracksplit.pssg"),
                route.area_dir().join("objects.pssg"),
                route.area_dir().join("trees.pssg"),
                route.dir.join("route_objects.pssg"),
            ];
            files.push(route.dir.join("routesplit.pssg"));
            for file in files.into_iter().filter(|f| f.is_file()) {
                for m in material::read(&Pssg::read(&file).unwrap()).values() {
                    for reference in m.textures.values() {
                        total += 1;
                        match library.get(reference) {
                            None => missing.push(reference.clone()),
                            Some(t) if t.is_placeholder() => placeholders.push(reference.clone()),
                            Some(_) => {}
                        }
                    }
                }
            }
            missing.sort();
            missing.dedup();
            placeholders.sort();
            placeholders.dedup();
            eprintln!(
                "{id}: {total} references, {} distinct placeholders {placeholders:?}, {} missing {missing:?}, {} ms",
                placeholders.len(),
                missing.len(),
                started.elapsed().as_millis()
            );
            // Sponsor packs are swapped in by the game at run time; the cubemap is not a `.tga` texture.
            missing.retain(|m| !m.starts_with("sponsor_") && !m.ends_with("default_cubemap.dds"));
            assert!(missing.is_empty() && placeholders.is_empty(), "{missing:?} {placeholders:?}");
        }
    }
}
