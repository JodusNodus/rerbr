//! AC material/surface translation. A surface's name gives its RBR class; its declared friction picks the id.
use crate::assetto::kn5::{Material, Model};
use crate::assetto::{detail, multilayer};
use crate::btb::textures;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::Path;

#[derive(Default)]
pub struct Surfaces {
    pub overrides: BTreeMap<String, u8>,
    pub declared: BTreeSet<String>,
    /// Per declared key: `FRICTION`, and whether it dirties the tyres (`DIRT_ADDITIVE` over 0).
    friction: BTreeMap<String, (f64, bool)>,
}

/// AC's `FRICTION` of clean racing asphalt, which RBR's grippiest tarmac (37) stands for.
const ASPHALT: f64 = 0.98;
/// RBR's dry surfaces by class with NGP 7.4.778's static friction (`rbr-tyre-lab` `materials`, 2026-11-12).
const TARMAC: [(u8, f64); 9] = [
    (37, 1.3),
    (65, 1.275),
    (45, 1.2675),
    (69, 1.25),
    (73, 1.2431),
    (53, 1.235),
    (77, 1.2188),
    (81, 1.2112),
    (85, 1.1875),
];
const GRAVEL: [(u8, f64); 3] = [(17, 0.9), (9, 0.885), (1, 0.8725)];
const DIRT: [(u8, f64); 3] = [(133, 0.84), (141, 0.8), (149, 0.76)];
const GRASS: [(u8, f64); 3] = [(113, 0.7), (105, 0.675), (97, 0.65)];

impl Surfaces {
    pub fn read(path: &Path, overrides: BTreeMap<String, u8>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let sections = rbr_formats::ini::parse(&text).map_err(|e| e.to_string())?;
        let number = |s: &rbr_formats::ini::Section, k: &str| {
            // The number alone: some files follow it with a comment.
            let v = s.get(k)?.trim_start();
            v[..v.find(|c: char| !(c.is_ascii_digit() || ".-+".contains(c))).unwrap_or(v.len())].parse::<f64>().ok()
        };
        let friction: BTreeMap<String, (f64, bool)> = sections
            .iter()
            .filter(|s| s.name.to_uppercase().starts_with("SURFACE_"))
            .filter_map(|s| Some((s.get("KEY")?.to_uppercase(), s)))
            .map(|(key, s)| (key, (number(s, "FRICTION").unwrap_or(0.), number(s, "DIRT_ADDITIVE").unwrap_or(0.) > 0.)))
            .collect();
        Ok(Self { overrides, declared: friction.keys().cloned().collect(), friction })
    }
    /// Physical nodes have a nonzero integer prefix, sometimes zero-padded or followed by spaces.
    /// Year-prefixed scenery such as `2025_OBJ43` has an underscore and does not qualify.
    pub fn key(name: &str) -> Option<String> {
        let rest = name.trim_start_matches(|c: char| c.is_ascii_digit());
        let prefix = &name.as_bytes()[..name.len() - rest.len()];
        if !prefix.iter().any(|b| matches!(b, b'1'..=b'9')) {
            return None;
        }
        let rest = rest.trim_start();
        if !rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return None;
        }
        Some(rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_uppercase())
    }
    pub fn id(&self, key: &str, steep: bool) -> u8 {
        if let Some(&id) = self.overrides.get(key) {
            return id;
        }
        // The share of clean asphalt's grip the track declares for it, where it declares one.
        let declared = self.friction.get(key).filter(|f| f.0 > 0.).map(|&(f, dirty)| ((f / ASPHALT).min(1.), dirty));
        // Of a class, the id whose share of 37's grip is nearest; `or` where the track declares none.
        let nearest = |class: &[(u8, f64)], or: u8| {
            let off = |id: &&(u8, f64)| declared.map_or(0., |(share, _)| (id.1 / TARMAC[0].1 - share).abs());
            declared.and(class.iter().min_by(|a, b| off(a).total_cmp(&off(b)))).map_or(or, |id| id.0)
        };
        match key {
            "ROAD" | "ASPHALT" | "TARMAC" | "PIT" => nearest(&TARMAC, 37),
            "KERB" | "CONCRETE" => nearest(&TARMAC, 65),
            "GRASS" => nearest(&GRASS, 113),
            "SAND" | "GRAVEL" => nearest(&GRAVEL, 1),
            "DIRT" => nearest(&DIRT, 133),
            "MUD" => nearest(&DIRT, 149),
            "WALL" | "GUARDRAIL" => 64,
            "ICE" => 117, // RBR snow on ice, shallow. Approximation; override for another class.
            _ => match (crate::btb::surfaces::surface(Some(key), steep), declared) {
                // A name of the track's own on level ground (Riudecanyes' BERM, Turini's BORDE): tarmac, or dirt
                // where it dirties the tyres and has less grip than any tarmac.
                (37, Some((share, true))) if share < 0.9 => nearest(&DIRT, 133),
                (37, Some(_)) => nearest(&TARMAC, 37),
                (id, _) => id,
            },
        }
    }
}

type TextureBucket = Vec<(String, Vec<u8>)>;

pub struct TextureBank {
    zip: zip::ZipWriter<std::fs::File>,
    /// Hash buckets retain output filenames; a hash match is checked against the written payload.
    seen: BTreeMap<(u64, usize), TextureBucket>,
    aliases: BTreeMap<String, String>,
    pub missing: BTreeSet<String>,
    pub simplified: BTreeSet<String>,
    /// Multilayer materials baked into one texture, and materials whose textures are one-pixel placeholders.
    pub baked: BTreeSet<String>,
    pub placeholders: BTreeSet<String>,
    pub count: usize,
}

impl TextureBank {
    pub fn new(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        Ok(Self {
            zip: zip::ZipWriter::new(file),
            seen: BTreeMap::new(),
            aliases: BTreeMap::new(),
            missing: BTreeSet::new(),
            simplified: BTreeSet::new(),
            baked: BTreeSet::new(),
            placeholders: BTreeSet::new(),
            count: 0,
        })
    }
    pub fn put(&mut self, bytes: Vec<u8>, extension: &str) -> Result<String, String> {
        let mut hash = DefaultHasher::new();
        bytes.hash(&mut hash);
        let key = (hash.finish(), bytes.len());
        if let Some((file, _)) = self.seen.get(&key).and_then(|bucket| bucket.iter().find(|(_, b)| *b == bytes)) {
            return Ok(file.clone());
        }
        let filename = format!("ac_{:05}.{extension}", self.count);
        self.zip
            .start_file(
                format!("Textures/{filename}"),
                zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .map_err(|e| e.to_string())?;
        self.zip.write_all(&bytes).map_err(|e| e.to_string())?;
        self.seen.entry(key).or_default().push((filename.clone(), bytes));
        self.count += 1;
        Ok(filename)
    }
    /// The output texture of material `index`. Multilayer materials are baked ([`multilayer`]); a texture that is only
    /// a one-pixel placeholder (some track exports strip theirs) becomes an opaque colour guessed from the material
    /// name, and `opaque` says so. Anything else keeps its diffuse map.
    pub fn diffuse(&mut self, model: &Model, index: usize) -> Result<(String, bool), String> {
        let mat = &model.materials[index];
        if let Some(file) = self.stub(model, mat)? {
            return Ok((file, true));
        }
        // Before the detail bake, which would keep the diffuse's alpha: Roure's `pines_distant` has both.
        let multilayer = mat.shader.contains("Multilayer");
        if let Some(bytes) = cut_by_normal(model, mat).filter(|_| !multilayer) {
            return Ok((self.put(bytes, "dds")?, false));
        }
        if multilayer {
            let metres = if mat.shader.contains("objsp") { [1.; 2] } else { multilayer::metres_per_uv(model, index) };
            if let Some((w, h, rgba)) = multilayer::bake(model, index, metres) {
                self.baked.insert(mat.shader.clone());
                return Ok((self.put(textures::encode(false, w, h, rgba), "dds")?, false));
            }
            self.simplified.insert(mat.shader.clone());
        } else if mat.property("useDetail", 0.) > 0. {
            if let Some((w, h, rgba)) = detail::bake(model, index) {
                self.baked.insert(mat.shader.clone());
                return Ok((self.put(textures::encode(is_transparent(mat), w, h, rgba), "dds")?, false));
            }
            self.simplified.insert(mat.shader.clone());
        }
        Ok((self.base_diffuse(model, mat)?, false))
    }
    fn stub(&mut self, model: &Model, mat: &Material) -> Result<Option<String>, String> {
        if !is_placeholder(model, mat) {
            return Ok(None);
        }
        self.placeholders.insert(mat.name.clone());
        Ok(Some(self.put(textures::flat(placeholder_colour(&mat.name)), "dds")?))
    }
    fn base_diffuse(&mut self, model: &Model, mat: &Material) -> Result<String, String> {
        if let Some(name) = mat.textures.get("txDiffuse") {
            if let Some(data) = model.textures.get(name).filter(|d| !d.is_empty()) {
                let extension = if data.starts_with(b"DDS ") {
                    "dds"
                } else if data.starts_with(b"\x89PNG") {
                    "png"
                } else if data.starts_with(&[0xff, 0xd8]) {
                    "jpg"
                } else {
                    "dds"
                };
                // Validate DDS support; decode/re-encode formats the renderer cannot upload directly.
                let bytes = if extension == "dds" && rbr_formats::dds::parse(data).is_err() {
                    if let Some((w, h, rgba)) = textures::legacy_dds(data) {
                        rbr_formats::dds::rgba8_file(w, h, &rgba)
                    } else {
                        let (w, h, rgba) =
                            textures::decode(data).ok_or_else(|| format!("unsupported texture {name}"))?;
                        textures::encode(true, w, h, rgba)
                    }
                } else {
                    data.clone()
                };
                let file = self.put(bytes, extension)?;
                self.aliases.insert(name.clone(), file.clone());
                return Ok(file);
            }
            if let Some(file) = self.aliases.get(name) {
                return Ok(file.clone());
            }
            self.missing.insert(format!("{}: {name}", mat.name));
        }
        self.put(textures::flat([0.5, 0.5, 0.5, 1.]), "dds")
    }
    pub fn finish(self) -> Result<(), String> {
        self.zip.finish().map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Whether the diffuse texture is a one-pixel PNG: some track exports strip their textures to those.
pub fn is_placeholder(model: &Model, mat: &Material) -> bool {
    mat.textures.get("txDiffuse").and_then(|n| model.textures.get(n)).is_some_and(|d| {
        d.len() <= 512 && d.starts_with(b"\x89PNG") && textures::decode(d).is_some_and(|(w, h, _)| w * h == 1)
    })
}

/// A material AC draws with alpha: a cut-out or blended one. A placeholder has no alpha to cut with.
pub fn is_transparent(mat: &Material) -> bool {
    mat.alpha_test || mat.blend != 0 || mat.shader.contains("AT") || mat.shader == "ksTree"
}

/// An alpha-tested material AC never draws: its `ksAlphaRef` (CSP's alpha test reference, 0 to 1) is above every
/// texel's alpha. Roure's `pinesTT` (10000) only casts its trees' shadows.
pub fn is_never_drawn(mat: &Material) -> bool {
    is_transparent(mat) && mat.property("ksAlphaRef", 0.) > 1.
}

/// The diffuse of an alpha-tested material with the normal map's alpha, as a DXT5 `.dds`, where no diffuse texel is
/// under half alpha and the normal map's are: Roure's pines keep their cut-out there. `None` otherwise.
fn cut_by_normal(model: &Model, mat: &Material) -> Option<Vec<u8>> {
    if !is_transparent(mat) {
        return None;
    }
    let decode = |slot: &str| textures::decode(model.textures.get(mat.textures.get(slot)?)?);
    let (w, h, normal) = decode("txNormal")?;
    if normal.chunks_exact(4).all(|t| t[3] >= 128) {
        return None;
    }
    let diffuse = decode("txDiffuse")?;
    if diffuse.2.chunks_exact(4).any(|t| t[3] < 128) {
        return None;
    }
    let opacity = normal.chunks_exact(4).flat_map(|t| [t[3]; 4]).collect();
    Some(textures::merge(Some(diffuse), [1.; 4], &(w, h, opacity)))
}

/// A plausible opaque colour for a material whose texture is a placeholder, from its name.
fn placeholder_colour(name: &str) -> [f32; 4] {
    let name = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    let rgb = if has(&["road", "asphalt", "tarmac"]) {
        [0.30, 0.30, 0.31]
    } else if has(&["grass", "lawn", "field", "leaf", "tree"]) {
        [0.27, 0.38, 0.17]
    } else if has(&["soil", "cliff", "dirt", "rock", "stone", "earth"]) {
        [0.38, 0.31, 0.24]
    } else if has(&["tyre", "tire", "rubber"]) {
        [0.08, 0.08, 0.08]
    } else if has(&["window", "glass", "mirror"]) {
        [0.35, 0.42, 0.50]
    } else {
        [0.55, 0.55, 0.53]
    };
    [rgb[0], rgb[1], rgb[2], 1.]
}

pub fn section(out: &mut String, name: &str, texture: &str, transparent: bool) {
    let technique = if transparent { "Transparent" } else { "Default" };
    let _ = writeln!(out,"[{name}]\neffectName=BTB_Diffuse\ntechniqueName={technique}\nambient=0.4,0.4,0.4,1.0\nSpecularPower=4\nSpecularColor=0,0,0\ndiffuseTexture={texture}\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_opaque_diffuse_takes_the_normal_maps_cut_out() {
        // A 2x2 diffuse with no cut-out (its alpha is shading); the normal map cuts its left column.
        let dds = |texels: [[u8; 4]; 4]| rbr_formats::dds::rgba8_file(2, 2, &texels.concat());
        let normal = dds([[128, 128, 255, 0], [128, 128, 255, 255], [128, 128, 255, 0], [128, 128, 255, 255]]);
        let model = Model {
            version: 6,
            textures: BTreeMap::from([
                ("d".to_string(), dds([[200, 100, 50, 200]; 4])),
                ("n".to_string(), normal),
                ("solid".to_string(), dds([[128, 128, 255, 255]; 4])),
            ]),
            materials: Vec::new(),
            meshes: Vec::new(),
            markers: BTreeMap::new(),
            dropped_triangles: 0,
            repaired_uvs: 0,
            repaired_normals: 0,
        };
        let on = |shader: &str, normal: &str| Material {
            shader: shader.into(),
            textures: [("txDiffuse", "d"), ("txNormal", normal)].map(|(s, n)| (s.into(), n.into())).into(),
            ..Material::default()
        };
        let bytes = cut_by_normal(&model, &on("ksPerPixelMultiMap_AT", "n")).unwrap();
        let (_, _, rgba) = textures::decode(&bytes).unwrap();
        assert_eq!(rgba.chunks_exact(4).map(|t| t[3]).collect::<Vec<_>>(), [0, 255, 0, 255]);
        assert!(cut_by_normal(&model, &on("ksPerPixelMultiMap", "n")).is_none(), "not alpha tested");
        assert!(cut_by_normal(&model, &on("ksPerPixelMultiMap_AT", "solid")).is_none(), "the normal map cuts nothing");
        let mut never = on("ksTree", "n");
        never.properties.insert("ksAlphaRef".into(), 10000.);
        assert!(is_never_drawn(&never));
        never.properties.insert("ksAlphaRef".into(), 1.);
        assert!(!is_never_drawn(&never));
    }
    #[test]
    fn a_one_pixel_png_is_a_placeholder_and_a_dds_is_not() {
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\rIDATx\xdacd`\xf8_\x0f\0\x02\x87\x01\x80\xebG\xba\x92\0\0\0\0IEND\xaeB`\x82";
        let mut model = Model {
            version: 6,
            textures: BTreeMap::from([
                ("a.dds".to_string(), png.to_vec()),
                ("b.dds".to_string(), textures::flat([1.; 4])),
            ]),
            materials: Vec::new(),
            meshes: Vec::new(),
            markers: BTreeMap::new(),
            dropped_triangles: 0,
            repaired_uvs: 0,
            repaired_normals: 0,
        };
        let on = |name: &str| Material {
            textures: BTreeMap::from([("txDiffuse".to_string(), name.to_string())]),
            ..Material::default()
        };
        assert!(is_placeholder(&model, &on("a.dds")));
        assert!(!is_placeholder(&model, &on("b.dds")));
        assert!(!is_placeholder(&model, &on("missing.dds")));
        model.textures.clear();
        assert!(!is_placeholder(&model, &on("a.dds")));
        assert_eq!(placeholder_colour("ROAD2")[..3], [0.30, 0.30, 0.31]);
        assert_eq!(placeholder_colour("PL_TREES1")[3], 1.);
    }
    #[test]
    fn collision_names_are_separate_from_scenery_and_surface_overrides_win() {
        assert_eq!(Surfaces::key("1ROAD230").as_deref(), Some("ROAD"));
        assert_eq!(Surfaces::key("01TARMAC").as_deref(), Some("TARMAC"));
        assert_eq!(Surfaces::key("01 WALL gr").as_deref(), Some("WALL"));
        assert_eq!(Surfaces::key("01 Concrete bridge").as_deref(), Some("CONCRETE"));
        assert_eq!(Surfaces::key("2025_OBJ43"), None);
        assert_eq!(Surfaces::key("OBJ1"), None);
        assert_eq!(Surfaces::key("0ROAD"), None);
        assert_eq!(Surfaces::key("000ROAD"), None);
        assert_eq!(Surfaces::key("01"), None);
        let mut surfaces = Surfaces::default();
        surfaces.overrides.insert("ROAD".into(), 65);
        assert_eq!(surfaces.id("ROAD", false), 65);
        assert_eq!(surfaces.id("WALL", true), 64);
        assert_eq!(surfaces.id("GRASS", false), 113);
    }
    #[test]
    fn declared_friction_picks_the_surface_within_its_class() {
        let mut surfaces = Surfaces::default();
        let declared = [
            ("ROAD", 0.98, false),
            ("TARMAD", 0.93, false),
            ("KERB", 0.92, false),
            ("BERM", 0.6, true),
            ("BORDE", 0.89, true),
        ];
        surfaces.friction = declared.iter().map(|&(key, f, dirty)| (key.to_owned(), (f, dirty))).collect();
        surfaces.friction.insert("GRASS".into(), (0.85, true));
        assert_eq!(surfaces.id("ROAD", false), 37);
        assert_eq!(surfaces.id("TARMAD", false), 53);
        assert_eq!(surfaces.id("KERB", false), 77);
        assert_eq!(surfaces.id("BERM", false), 141);
        assert_eq!(surfaces.id("BERM", true), 64);
        assert_eq!(surfaces.id("BORDE", false), 85);
        assert_eq!(surfaces.id("GRASS", false), 113);
        // Not declared: the class's usual id.
        assert_eq!(surfaces.id("CONCRETE", false), 65);
        assert_eq!(surfaces.id("KRB", false), 37);
    }
}
