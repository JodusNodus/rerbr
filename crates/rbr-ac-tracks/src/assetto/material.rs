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
    /// Per declared key: `FRICTION`, whether it dirties the tyres (`DIRT_ADDITIVE` over 0), and CSP's `_EXT_SURFACE_TYPE`.
    friction: BTreeMap<String, (f64, bool, Option<String>)>,
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
/// The most `FRICTION` a dirtying road can declare and still be gravel: the gravel rallies declare 0.82 to 0.89, tarmac
/// rallies that dirty the tyres 0.93 and more (Monte Carlo 0.93, St Laurent's gritty patches 0.96).
const GRAVEL_ROAD: f64 = 0.9;

impl Surfaces {
    pub fn read(path: &Path, overrides: BTreeMap<String, u8>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let sections = rbr_formats::ini::parse(&text).map_err(|e| e.to_string())?;
        let number = |s: &rbr_formats::ini::Section, k: &str| {
            // The number alone: some files follow it with a comment.
            let v = s.get(k)?.trim_start();
            v[..v.find(|c: char| !(c.is_ascii_digit() || ".-+".contains(c))).unwrap_or(v.len())].parse::<f64>().ok()
        };
        let friction: BTreeMap<String, (f64, bool, Option<String>)> = sections
            .iter()
            .filter(|s| s.name.to_uppercase().starts_with("SURFACE_"))
            .filter_map(|s| Some((s.get("KEY")?.to_uppercase(), s)))
            .map(|(key, s)| {
                let kind = s.get("_EXT_SURFACE_TYPE").map(|v| v.trim().to_uppercase());
                (key, (number(s, "FRICTION").unwrap_or(0.), number(s, "DIRT_ADDITIVE").unwrap_or(0.) > 0., kind))
            })
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
        let declaration = self.friction.get(key);
        let declared = declaration.filter(|d| d.0 > 0.).map(|d| ((d.0 / ASPHALT).min(1.), d.1));
        // Of a class, the id whose share of 37's grip is nearest; `or` where the track declares none.
        let nearest = |class: &[(u8, f64)], or: u8| {
            let off = |id: &&(u8, f64)| declared.map_or(0., |(share, _)| (id.1 / TARMAC[0].1 - share).abs());
            declared.and(class.iter().min_by(|a, b| off(a).total_cmp(&off(b)))).map_or(or, |id| id.0)
        };
        // CSP's own word for the surface comes first, then a road that dirties the tyres and grips like gravel.
        let kind = declaration.and_then(|d| d.2.as_deref());
        let gravel_road = declaration.is_some_and(|d| d.1 && d.0 > 0. && d.0 <= GRAVEL_ROAD);
        match key {
            _ if kind == Some("GRAVEL") => nearest(&GRAVEL, 1),
            _ if kind == Some("ICE") => 117,
            _ if kind == Some("GRASS") => nearest(&GRASS, 113),
            "ROAD" | "ASPHALT" | "TARMAC" | "PIT" if gravel_road => nearest(&GRAVEL, 1),
            _ if gravel_road && key.starts_with("GRAV") => nearest(&GRAVEL, 1),
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
    /// Sections written with a normal map and with a highlight.
    pub normal_mapped: usize,
    pub highlighted: usize,
    /// Of those, the normal maps read at a UV scale of their own, and the ones with a detail normal layer.
    pub uv_scaled_normals: usize,
    pub detail_normals: usize,
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
            normal_mapped: 0,
            highlighted: 0,
            uv_scaled_normals: 0,
            detail_normals: 0,
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
            if let Some(file) = self.slot(model, mat, "txDiffuse", true)? {
                return Ok(file);
            }
            self.missing.insert(format!("{}: {name}", mat.name));
        }
        self.put(textures::flat([0.5, 0.5, 0.5, 1.]), "dds")
    }
    /// The texture in `slot`, stored as it is where the renderer can upload it, else decoded and re-encoded (`alpha`
    /// keeps its alpha channel). `None` where the slot has no texture in the model, or the one it names was stored
    /// before under that name.
    fn slot(&mut self, model: &Model, mat: &Material, slot: &str, alpha: bool) -> Result<Option<String>, String> {
        let Some(name) = mat.textures.get(slot) else { return Ok(None) };
        let alias = if alpha { name.clone() } else { format!("opaque:{name}") };
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
                    let (w, h, rgba) = textures::decode(data).ok_or_else(|| format!("unsupported texture {name}"))?;
                    textures::encode(alpha, w, h, rgba)
                }
            } else {
                data.clone()
            };
            let file = self.put(bytes, extension)?;
            self.aliases.insert(alias, file.clone());
            return Ok(Some(file));
        }
        Ok(self.aliases.get(&alias).cloned())
    }
    /// What a material written per material takes besides its diffuse (see [`Extras`]).
    pub fn extras(&mut self, model: &Model, index: usize) -> Result<Extras, String> {
        let mat = &model.materials[index];
        if is_placeholder(model, mat) {
            return Ok(Extras::default());
        }
        let mut extras = Extras { highlight: highlight(mat), ..Extras::default() };
        let normal = mat.textures.get("txNormal").filter(|_| !mat.shader.contains("Multilayer"));
        if let Some(name) = normal {
            // A normal map that cannot be read is no normal map.
            match self.slot(model, mat, "txNormal", false) {
                Ok(Some(file)) => extras.normal = Some(file),
                Ok(None) => _ = self.missing.insert(format!("{}: {name}", mat.name)),
                Err(_) => _ = self.simplified.insert(format!("{}: {name}", mat.name)),
            }
        }
        if let Some([diffuse, normal]) = uv_multipliers(model, mat).filter(|_| extras.normal.is_some()) {
            extras.bump_scale = Some([normal / diffuse; 2]);
        }
        if mat.shader.contains("Multilayer") {
            self.detail_map(model, mat, &mut extras);
        } else if mat.shader.contains("NMDetail") {
            self.detail_normal(model, mat, &mut extras);
        }
        if let Some(name) = mat.textures.get("txMaps").filter(|_| extras.highlight.is_some()) {
            extras.specular = self.share(model, name)?;
        }
        self.count_extras(&extras);
        Ok(extras)
    }
    /// Counts a section written with `extras` again: a material's tiles share its [`Extras`].
    pub fn count_extras(&mut self, extras: &Extras) {
        self.normal_mapped += usize::from(extras.normal.is_some());
        self.highlighted += usize::from(extras.highlight.is_some());
        self.uv_scaled_normals += usize::from(extras.normal.is_some() && extras.bump_scale.is_some());
        self.detail_normals += usize::from(extras.detail.is_some());
    }
    /// `ksMultilayer_fresnel_nm`'s only normal map, `txDetailNM`, read at the source UV times the pair `detailNMMult`
    /// (the scalar is always 0): the normal map of `extras`, on the UV set 0 the material's bake takes.
    fn detail_map(&mut self, model: &Model, mat: &Material, extras: &mut Extras) {
        let Some(&scale) = mat.pairs.get("detailNMMult").filter(|p| p.iter().all(|c| c.is_finite() && *c >= 0.)) else {
            return;
        };
        let Some(name) = mat.textures.get("txDetailNM") else { return };
        if mat.shader != "ksMultilayer_fresnel_nm" || scale == [0.; 2] || is_one_pixel(model, name) {
            return;
        }
        match self.slot(model, mat, "txDetailNM", false) {
            Ok(Some(file)) => {
                extras.normal = Some(file);
                extras.bump_scale = Some(scale);
            }
            Ok(None) => _ = self.missing.insert(format!("{}: {name}", mat.name)),
            Err(_) => _ = self.simplified.insert(format!("{}: {name}", mat.name)),
        }
    }
    /// The detail normal layer of `ksPerPixelMultiMap_NMDetail` and `_AT_NMDetail` with `useDetail`: the normal map
    /// gains the layer's mask as its alpha (`1 - diffuse alpha` as the diffuse bake ([`detail`]) masks the colour
    /// detail; 1 for the `AT` one), and the detail texture follows at `detailUVMultiplier` times the UV.
    fn detail_normal(&mut self, model: &Model, mat: &Material, extras: &mut Extras) {
        let repeat = mat.property("detailUVMultiplier", 0.);
        let blend = mat.property("detailNormalBlend", 0.);
        let Some(name) = mat.textures.get("txNormalDetail") else { return };
        if mat.property("useDetail", 0.) <= 0. || repeat <= 0. || blend == 0. || is_one_pixel(model, name) {
            return;
        }
        let masked = !mat.shader.contains("_AT_");
        let Some(diffuse) = multilayer::texture(model, mat, "txDiffuse") else { return };
        let Some(layer) = self.slot(model, mat, "txNormalDetail", false).ok().flatten() else {
            self.missing.insert(format!("{}: {name}", mat.name));
            return;
        };
        // The normal map's own texels carry the mask; a missing, unreadable or one-pixel one is flat.
        let normal = multilayer::texture(model, mat, "txNormal").filter(|n| n.0 * n.1 > 1);
        let (width, height, mut rgba) = normal.unwrap_or_else(|| {
            let (w, h) = (diffuse.0.min(2048), diffuse.1.min(2048));
            (w, h, [128, 128, 255, 255].repeat((w * h) as usize))
        });
        mask_alpha(&mut rgba, (width, height), masked.then_some(&diffuse));
        match self.put(textures::encode(true, width, height, rgba), "dds") {
            Ok(file) => {
                extras.normal = Some(file);
                extras.detail = Some(Detail { texture: layer, scale: [repeat; 2], blend });
            }
            Err(_) => _ = self.simplified.insert(format!("{}: {name}", mat.name)),
        }
    }
    /// The highlight of a material written per tile: the texture is baked, so there is no spec map.
    pub fn tile_extras(&mut self, model: &Model, index: usize) -> Extras {
        let mat = &model.materials[index];
        let highlight = highlight(mat).filter(|_| !is_placeholder(model, mat));
        let mut extras = Extras { highlight, ..Extras::default() };
        if !is_placeholder(model, mat) {
            self.detail_map(model, mat, &mut extras);
            // The tile's UV set 0 is its own: the normal map reads the source UV on the mesh's second set.
            extras.bump_uv = extras.normal.is_some();
        }
        self.count_extras(&extras);
        extras
    }
    /// The red channel of a spec map (AC's specular share) as an opaque grey texture.
    fn share(&mut self, model: &Model, name: &str) -> Result<Option<String>, String> {
        let alias = format!("share:{name}");
        if let Some(file) = self.aliases.get(&alias) {
            return Ok(Some(file.clone()));
        }
        let Some((w, h, rgba)) = model.textures.get(name).and_then(|d| textures::decode(d)) else {
            return Ok(None);
        };
        let grey = rgba.chunks_exact(4).flat_map(|t| [t[0], t[0], t[0], 255]).collect();
        let file = self.put(textures::encode(false, w, h, grey), "dds")?;
        self.aliases.insert(alias, file.clone());
        Ok(Some(file))
    }
    pub fn finish(self) -> Result<(), String> {
        self.zip.finish().map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Whether the diffuse texture is a one-pixel PNG: some track exports strip their textures to those.
pub fn is_placeholder(model: &Model, mat: &Material) -> bool {
    mat.textures.get("txDiffuse").is_some_and(|n| is_one_pixel(model, n))
}

/// Whether the model's texture `name` is a one-pixel PNG.
fn is_one_pixel(model: &Model, name: &str) -> bool {
    model.textures.get(name).is_some_and(|d| {
        d.len() <= 512 && d.starts_with(b"\x89PNG") && textures::decode(d).is_some_and(|(w, h, _)| w * h == 1)
    })
}

/// The UV multipliers of `ksPerPixelNM_UVMult` (`1 + diffuseMult`, `1 + normalMult`): the diffuse is read at the UV
/// times the first, the normal map at the UV times the second. `None` for another shader, a multiplier that is not
/// positive and a placeholder diffuse.
pub fn uv_multipliers(model: &Model, mat: &Material) -> Option<[f32; 2]> {
    let mults = ["diffuseMult", "normalMult"].map(|key| 1. + mat.property(key, 0.));
    (mat.shader == "ksPerPixelNM_UVMult" && mults.iter().all(|&m| m > 0.) && !is_placeholder(model, mat))
        .then_some(mults)
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

/// What a section carries besides its diffuse, for ReRBR's renderer: files in the textures, and the highlight's
/// `(ksSpecular, ksSpecularEXP)`.
#[derive(Default, Clone)]
pub struct Extras {
    pub normal: Option<String>,
    pub specular: Option<String>,
    pub highlight: Option<(f32, f32)>,
    /// `ReRbrBumpScale`: the normal map is read at its UV times this.
    pub bump_scale: Option<[f32; 2]>,
    /// `ReRbrBumpUv`: the normal map reads the mesh's second UV set.
    pub bump_uv: bool,
    pub detail: Option<Detail>,
}

/// A detail normal layer: a file in the textures, `ReRbrDetailScale` and `ReRbrDetailBlend`.
#[derive(Clone)]
pub struct Detail {
    pub texture: String,
    pub scale: [f32; 2],
    pub blend: f32,
}

/// `rgba`'s alpha as the detail normal layer's mask: `1 - ` the alpha of `diffuse` read at the same UV (bilinear,
/// repeating, on `rgba`'s texel grid), or 255 without one.
fn mask_alpha(rgba: &mut [u8], (width, height): (u32, u32), diffuse: Option<&textures::Image>) {
    let Some(diffuse) = diffuse else {
        rgba.chunks_exact_mut(4).for_each(|t| t[3] = 255);
        return;
    };
    let row = width as usize * 4;
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let rows = (height as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (n, chunk) in rgba.chunks_mut(rows * row).enumerate() {
            scope.spawn(move || {
                for (r, line) in chunk.chunks_mut(row).enumerate() {
                    let v = ((n * rows + r) as f64 + 0.5) / f64::from(height);
                    for (x, t) in line.chunks_mut(4).enumerate() {
                        let a = multilayer::sample(diffuse, (x as f64 + 0.5) / f64::from(width), v, true)[3];
                        t[3] = (255. - a).clamp(0., 255.).round() as u8;
                    }
                }
            });
        }
    });
}

/// The sun highlight of an opaque, non-placeholder material that declares `ksSpecular`.
fn highlight(mat: &Material) -> Option<(f32, f32)> {
    let specular = mat.property("ksSpecular", 0.);
    (specular > 0. && !is_transparent(mat)).then(|| (specular, mat.property("ksSpecularEXP", 4.).max(1.)))
}

/// A float with a decimal point and no exponent: BTB reads a bare `1,1,1` as 1.1 and 1.
fn decimal(v: f32) -> String {
    let text = format!("{v:.6}");
    let text = text.trim_end_matches('0');
    if text.ends_with('.') {
        format!("{text}0")
    } else {
        text.to_owned()
    }
}

pub fn section(out: &mut String, name: &str, texture: &str, transparent: bool) {
    section_with(out, name, texture, transparent, &Extras::default());
}

pub fn section_with(out: &mut String, name: &str, texture: &str, transparent: bool, extras: &Extras) {
    let technique = if transparent { "Transparent" } else { "Default" };
    let (power, color) = match extras.highlight {
        Some((s, p)) => (decimal(p), format!("{0},{0},{0}", decimal(s))),
        None => ("4".to_owned(), "0,0,0".to_owned()),
    };
    let _ = write!(out, "[{name}]\neffectName=BTB_Diffuse\ntechniqueName={technique}\nambient=0.4,0.4,0.4,1.0\nSpecularPower={power}\nSpecularColor={color}\ndiffuseTexture={texture}\n");
    for (key, file) in [("normalTexture", &extras.normal), ("specularTexture", &extras.specular)] {
        if let Some(file) = file {
            let _ = writeln!(out, "{key}={file}");
        }
    }
    let effect = match (extras.normal.is_some(), extras.highlight.is_some()) {
        (true, true) => Some("BTB_DiffuseBumpSpecular"),
        (true, false) => Some("BTB_DiffuseBump"),
        (false, true) => Some("BTB_DiffuseSpecular"),
        (false, false) => None,
    };
    if let Some(effect) = effect {
        let _ = writeln!(out, "ReRbrEffect={effect}");
    }
    if extras.normal.is_some() {
        let pair = |v: [f32; 2]| format!("{},{}", decimal(v[0]), decimal(v[1]));
        if let Some(scale) = extras.bump_scale {
            let _ = writeln!(out, "ReRbrBumpScale={}", pair(scale));
        }
        if extras.bump_uv {
            let _ = writeln!(out, "ReRbrBumpUv=1");
        }
        if let Some(detail) = &extras.detail {
            let _ = write!(
                out,
                "ReRbrDetailNormal={}\nReRbrDetailScale={}\nReRbrDetailBlend={}\n",
                detail.texture,
                pair(detail.scale),
                decimal(detail.blend)
            );
        }
    }
    out.push('\n');
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
        surfaces.friction = declared.iter().map(|&(key, f, dirty)| (key.to_owned(), (f, dirty, None))).collect();
        surfaces.friction.insert("GRASS".into(), (0.85, true, None));
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
    #[test]
    fn a_dirtying_road_with_gravel_grip_is_gravel_and_csps_word_wins() {
        let mut surfaces = Surfaces::default();
        let declared = [
            ("ROAD", 0.89, true, None),
            ("TARMAC", 1.0, false, None),
            ("GRIT", 0.97, true, None),
            ("GRAVB", 0.82, true, None),
            ("GRAVA", 0.84, true, Some("GRAVEL")),
            ("GRASS", 0.85, true, Some("GRAVEL")),
            ("SNOW", 0.6, true, Some("ICE")),
        ];
        surfaces.friction = declared
            .iter()
            .map(|&(key, f, dirty, kind)| (key.to_owned(), (f, dirty, kind.map(str::to_owned))))
            .collect();
        assert_eq!(surfaces.id("ROAD", false), 17);
        assert_eq!(surfaces.id("TARMAC", false), 37);
        assert_eq!(surfaces.id("GRAVA", false), 17);
        assert_eq!(surfaces.id("GRAVB", false), 17);
        assert_eq!(surfaces.id("GRASS", false), 17);
        assert_eq!(surfaces.id("SNOW", false), 117);
        // Gritty tarmac that dirties the tyres is still tarmac.
        assert_eq!(surfaces.id("GRIT", false), 65);
        surfaces.friction.insert("ROAD".into(), (0.93, true, None));
        assert_eq!(surfaces.id("ROAD", false), 53);
    }
    fn extras_model() -> Model {
        let dds = |t: [u8; 4]| rbr_formats::dds::rgba8_file(2, 2, &t.repeat(4));
        Model {
            version: 6,
            textures: BTreeMap::from([
                ("d".to_string(), dds([200, 100, 50, 255])),
                ("n".to_string(), dds([128, 128, 255, 255])),
                ("m".to_string(), dds([90, 10, 20, 255])),
            ]),
            materials: Vec::new(),
            meshes: Vec::new(),
            markers: BTreeMap::new(),
            dropped_triangles: 0,
            repaired_uvs: 0,
            repaired_normals: 0,
        }
    }
    fn extras_of(shader: &str, transparent: bool, slots: &[&str]) -> (String, Extras, usize) {
        let mut model = extras_model();
        let mut mat = Material {
            name: "m".into(),
            shader: shader.into(),
            alpha_test: transparent,
            textures: [("txDiffuse", "d"), ("txNormal", "n"), ("txMaps", "m")]
                .into_iter()
                .filter(|(s, _)| *s == "txDiffuse" || slots.contains(s))
                .map(|(s, n)| (s.into(), n.into()))
                .collect(),
            ..Material::default()
        };
        mat.properties.insert("ksSpecular".into(), 0.5);
        mat.properties.insert("ksSpecularEXP".into(), 15.);
        model.materials.push(mat);
        let path = std::env::temp_dir().join(format!("ac-extras-{}-{shader}-{transparent}.zip", std::process::id()));
        let mut bank = TextureBank::new(&path).unwrap();
        let extras = bank.extras(&model, 0).unwrap();
        let count = bank.count;
        bank.finish().unwrap();
        let _ = std::fs::remove_file(path);
        let mut out = String::new();
        section_with(&mut out, "s", "ac_diffuse.dds", transparent, &extras);
        (out, extras, count)
    }
    #[test]
    fn a_normal_map_and_a_spec_map_with_a_highlight_give_the_full_section() {
        let (out, extras, count) = extras_of("ksPerPixelNM", false, &["txNormal", "txMaps"]);
        assert_eq!(count, 2, "a normal map and a share map, both new");
        assert_eq!(
            out,
            format!(
                "[s]\neffectName=BTB_Diffuse\ntechniqueName=Default\nambient=0.4,0.4,0.4,1.0\nSpecularPower=15.0\nSpecularColor=0.5,0.5,0.5\ndiffuseTexture=ac_diffuse.dds\nnormalTexture={}\nspecularTexture={}\nReRbrEffect=BTB_DiffuseBumpSpecular\n\n",
                extras.normal.unwrap(),
                extras.specular.unwrap()
            )
        );
    }
    #[test]
    fn a_material_with_neither_feature_keeps_todays_section() {
        let mut out = String::new();
        section_with(&mut out, "s", "t.dds", true, &Extras::default());
        let mut today = String::new();
        section(&mut today, "s", "t.dds", true);
        assert_eq!(out, today);
        assert_eq!(out, "[s]\neffectName=BTB_Diffuse\ntechniqueName=Transparent\nambient=0.4,0.4,0.4,1.0\nSpecularPower=4\nSpecularColor=0,0,0\ndiffuseTexture=t.dds\n\n");
    }
    #[test]
    fn multilayer_gets_no_normal_map_and_transparent_no_highlight() {
        let (out, extras, _) = extras_of("ksMultilayer_fresnel_nm", false, &["txNormal"]);
        assert!(extras.normal.is_none() && !out.contains("normalTexture"));
        assert!(out.contains("ReRbrEffect=BTB_DiffuseSpecular") && !out.contains("specularTexture"));
        let (out, extras, _) = extras_of("ksPerPixelAT_NM", true, &["txNormal", "txMaps"]);
        assert!(extras.highlight.is_none() && extras.specular.is_none());
        assert!(out.contains("normalTexture=") && out.contains("ReRbrEffect=BTB_DiffuseBump\n"));
        assert!(out.contains("SpecularPower=4\nSpecularColor=0,0,0\n"));
    }
    #[test]
    fn the_highlight_reads_back_through_btbs_decimal_floats() {
        for (specular, power) in [(1., 15.), (0.5, 4.), (0.004, 120.), (0.00001, 0.5)] {
            let mut out = String::new();
            let extras = Extras { highlight: Some((specular, power)), ..Extras::default() };
            section_with(&mut out, "s", "t.dds", false, &extras);
            let value = |key: &str| {
                let line = out.lines().find_map(|l| l.strip_prefix(key)).unwrap();
                rbr_btb::track::decimal_floats(line)
            };
            let colour = value("SpecularColor=");
            assert_eq!(colour.len(), 3, "{out}");
            assert!(colour.iter().all(|&c| (c - f64::from(specular)).abs() < 1e-6), "{colour:?}");
            assert_eq!(value("SpecularPower="), [f64::from(power)]);
        }
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\rIDATx\xdacd`\xf8_\x0f\0\x02\x87\x01\x80\xebG\xba\x92\0\0\0\0IEND\xaeB`\x82";

    /// A model with one material of `shader` on `slots` (`slot -> texture`), `properties` and `pairs`; the textures:
    /// `d` a 2x2 diffuse whose alpha is 100 on the left column and 0 on the right, `n` a 2x2 normal map, `t` a 4x4
    /// detail normal map, `p` a one-pixel PNG.
    fn detail_extras(
        shader: &str,
        slots: &[(&str, &str)],
        properties: &[(&str, f32)],
        pairs: &[(&str, [f32; 2])],
    ) -> (Extras, TextureBank, Model) {
        let dds = |w: u32, h: u32, t: [u8; 4]| rbr_formats::dds::rgba8_file(w, h, &t.repeat((w * h) as usize));
        let mut model = extras_model();
        model.textures.insert(
            "d".into(),
            rbr_formats::dds::rgba8_file(2, 2, &[[200, 100, 50, 100], [10, 10, 10, 0]].repeat(2).concat()),
        );
        model.textures.insert("t".into(), dds(4, 4, [120, 130, 250, 255]));
        model.textures.insert("p".into(), PNG.to_vec());
        model.materials.push(Material {
            name: "m".into(),
            shader: shader.into(),
            textures: slots.iter().map(|&(s, n)| (s.to_owned(), n.to_owned())).collect(),
            properties: properties.iter().map(|&(k, v)| (k.to_owned(), v)).collect(),
            pairs: pairs.iter().map(|&(k, v)| (k.to_owned(), v)).collect(),
            ..Material::default()
        });
        let path = std::env::temp_dir().join(format!("ac-detail-{}-{shader}-{}.zip", std::process::id(), slots.len()));
        let mut bank = TextureBank::new(&path).unwrap();
        let extras = bank.extras(&model, 0).unwrap();
        let _ = std::fs::remove_file(path);
        (extras, bank, model)
    }

    fn alphas(bank_bytes: &[u8]) -> Vec<u8> {
        textures::decode(bank_bytes).unwrap().2.chunks_exact(4).map(|t| t[3]).collect()
    }

    #[test]
    fn a_nm_detail_material_takes_the_inverse_diffuse_alpha_as_its_masks_normal_alpha() {
        let slots = [("txDiffuse", "d"), ("txNormal", "n"), ("txNormalDetail", "t")];
        let properties = [("useDetail", 1.), ("detailUVMultiplier", 8.), ("detailNormalBlend", -1.5)];
        let (extras, bank, _) = detail_extras("ksPerPixelMultiMap_NMDetail", &slots, &properties, &[]);
        let detail = extras.detail.as_ref().expect("a detail layer");
        assert_eq!((detail.scale, detail.blend), ([8.; 2], -1.5));
        assert_eq!((bank.normal_mapped, bank.detail_normals, bank.uv_scaled_normals), (1, 1, 0));
        let mut out = String::new();
        section_with(&mut out, "s", "d.dds", false, &extras);
        assert!(out.contains("ReRbrDetailScale=8.0,8.0\nReRbrDetailBlend=-1.5\n"), "{out}");
        assert!(out.contains(&format!("ReRbrDetailNormal={}\n", detail.texture)));
        // The 2x2 diffuse's alpha 100, 0 per row is the mask 155, 255; the normal map's own alpha is replaced.
        let (rgba, _) = (bank_bytes(&extras, &bank), ());
        assert_eq!(alphas(&rgba), [155, 255, 155, 255]);
        // `AT_NMDetail` is unmasked.
        let (extras, bank, _) = detail_extras("ksPerPixelMultiMap_AT_NMDetail", &slots, &properties, &[]);
        assert_eq!(alphas(&bank_bytes(&extras, &bank)), [255; 4]);
    }

    /// The bytes of the normal map `extras` names: the bank's zip is gone, so read the file back from `put`'s record.
    fn bank_bytes(extras: &Extras, bank: &TextureBank) -> Vec<u8> {
        let name = extras.normal.as_ref().unwrap();
        bank.seen.values().flatten().find(|(file, _)| file == name).unwrap().1.clone()
    }

    #[test]
    fn a_missing_normal_map_is_flat_and_a_placeholder_detail_or_zero_blend_gives_no_layer() {
        let properties = [("useDetail", 1.), ("detailUVMultiplier", 8.), ("detailNormalBlend", 2.)];
        let slots = [("txDiffuse", "d"), ("txNormalDetail", "t")];
        let (extras, bank, _) = detail_extras("ksPerPixelMultiMap_NMDetail", &slots, &properties, &[]);
        let flat = textures::decode(&bank_bytes(&extras, &bank)).unwrap();
        assert_eq!((flat.0, flat.1), (2, 2));
        assert_eq!(flat.2[..3], [128, 128, 255]);
        let with = |slots: &[(&str, &str)], properties: &[(&str, f32)]| {
            detail_extras("ksPerPixelMultiMap_NMDetail", slots, properties, &[]).0.detail.is_some()
        };
        assert!(with(&slots, &properties));
        assert!(!with(&[("txDiffuse", "d"), ("txNormalDetail", "p")], &properties));
        assert!(!with(&slots, &[("useDetail", 0.), ("detailUVMultiplier", 8.), ("detailNormalBlend", 2.)]));
        assert!(!with(&slots, &[("useDetail", 1.), ("detailUVMultiplier", 0.), ("detailNormalBlend", 2.)]));
        assert!(!with(&slots, &[("useDetail", 1.), ("detailUVMultiplier", 8.), ("detailNormalBlend", 0.)]));
        assert!(!with(&[("txDiffuse", "d")], &properties));
        // A one-pixel normal map is as good as none.
        let (extras, bank, _) = detail_extras(
            "ksPerPixelMultiMap_NMDetail",
            &[("txDiffuse", "d"), ("txNormal", "p"), ("txNormalDetail", "t")],
            &properties,
            &[],
        );
        assert_eq!(textures::decode(&bank_bytes(&extras, &bank)).unwrap().2[..3], [128, 128, 255]);
    }

    #[test]
    fn multilayers_detail_normal_map_is_its_normal_map_at_the_pair() {
        let slots = [("txDiffuse", "d"), ("txDetailNM", "n")];
        let (extras, bank, _) =
            detail_extras("ksMultilayer_fresnel_nm", &slots, &[], &[("detailNMMult", [2000., 4000.5])]);
        assert!(extras.normal.is_some() && !extras.bump_uv);
        assert_eq!((bank.normal_mapped, bank.uv_scaled_normals), (1, 1));
        let mut out = String::new();
        section_with(&mut out, "s", "d.dds", false, &extras);
        assert!(out.contains("ReRbrEffect=BTB_DiffuseBump\nReRbrBumpScale=2000.0,4000.5\n"), "{out}");
        assert!(!out.contains("ReRbrBumpUv"));
        for (shader, slots, pairs) in [
            ("ksMultilayer_fresnel_nm", &[("txDiffuse", "d"), ("txDetailNM", "p")][..], [2., 2.]),
            ("ksMultilayer_fresnel_nm", &slots[..], [0., 0.]),
            ("ksMultilayer_objsp_nm4", &slots[..], [2., 2.]),
        ] {
            assert!(
                detail_extras(shader, slots, &[], &[("detailNMMult", pairs)]).0.normal.is_none(),
                "{shader} {pairs:?}"
            );
        }
        assert!(detail_extras("ksMultilayer_fresnel_nm", &slots, &[], &[]).0.normal.is_none());
    }

    #[test]
    fn a_tile_reads_the_multilayer_normal_map_on_the_source_uv() {
        let slots = [("txDiffuse", "d"), ("txDetailNM", "n")];
        let (_, mut bank, model) =
            detail_extras("ksMultilayer_fresnel_nm", &slots, &[], &[("detailNMMult", [10., 40.])]);
        let extras = bank.tile_extras(&model, 0);
        assert!(extras.normal.is_some() && extras.bump_uv);
        let mut out = String::new();
        section_with(&mut out, "s", "d.dds", false, &extras);
        assert!(out.contains("ReRbrBumpScale=10.0,40.0\nReRbrBumpUv=1\n"), "{out}");
    }

    #[test]
    fn uv_mult_scales_the_normal_map_against_the_diffuse() {
        let slots = [("txDiffuse", "d"), ("txNormal", "n")];
        let properties = [("diffuseMult", 3.), ("normalMult", 7.)];
        let (extras, bank, model) = detail_extras("ksPerPixelNM_UVMult", &slots, &properties, &[]);
        assert_eq!(extras.bump_scale, Some([2.; 2]));
        assert_eq!(bank.uv_scaled_normals, 1);
        assert_eq!(uv_multipliers(&model, &model.materials[0]), Some([4., 8.]));
        let (extras, ..) = detail_extras("ksPerPixelNM", &slots, &properties, &[]);
        assert_eq!(extras.bump_scale, None);
        let (_, _, model) = detail_extras("ksPerPixelNM_UVMult", &[("txDiffuse", "p")], &properties, &[]);
        assert_eq!(uv_multipliers(&model, &model.materials[0]), None, "a placeholder diffuse keeps its UVs");
    }

    #[test]
    fn the_new_keys_read_back_through_btbs_decimal_floats() {
        for scale in [[1., 1.], [2000., 4000.], [0.5, 10.], [1.0 / 3.0, 50.]] {
            let extras = Extras { normal: Some("n.dds".into()), bump_scale: Some(scale), ..Extras::default() };
            let mut out = String::new();
            section_with(&mut out, "s", "t.dds", false, &extras);
            let line = out.lines().find_map(|l| l.strip_prefix("ReRbrBumpScale=")).unwrap();
            let read = rbr_btb::track::decimal_floats(line);
            assert!(read.iter().zip(scale).all(|(r, s)| (r - f64::from(s)).abs() < 1e-5), "{line}");
        }
    }
}
