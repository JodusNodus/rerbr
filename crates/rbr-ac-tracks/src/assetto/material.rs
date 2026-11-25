//! AC material/surface translation. A surface's name gives its RBR class; its declared friction picks the id.
use crate::assetto::kn5::{Material, Model};
use crate::assetto::{detail, multilayer};
use rbr_btb_writer::textures;
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
            _ => match (rbr_btb_writer::surfaces::surface(Some(key), steep), declared) {
                // A name of the track's own on level ground (Riudecanyes' BERM, Turini's BORDE): tarmac, or dirt
                // where it dirties the tyres and has less grip than any tarmac.
                (37, Some((share, true))) if share < 0.9 => nearest(&DIRT, 133),
                (37, Some(_)) => nearest(&TARMAC, 37),
                // Rock that dirties the tyres and grips like gravel is loose (Greece's and Onin's CROCK, 0.82 and 0.85).
                (69, _) if gravel_road => nearest(&GRAVEL, 1),
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
    /// The file a model's texture was stored as, by name, alpha and size: a material read twice stores nothing twice.
    stored: BTreeMap<(String, bool, usize), String>,
    /// The pixel size of textures [`TextureBank::layers`] has looked at, by name and byte size.
    sizes: BTreeMap<(String, usize), Option<(u32, u32)>>,
    pub missing: BTreeSet<String>,
    pub simplified: BTreeSet<String>,
    /// Multilayer materials baked into one texture, and materials whose textures are one-pixel placeholders.
    pub baked: BTreeSet<String>,
    pub placeholders: BTreeSet<String>,
    /// Sections written with a normal map and with a highlight.
    pub normal_mapped: usize,
    pub highlighted: usize,
    /// Sections written with a dry road sheen.
    pub sheened: usize,
    /// Sections written to be drawn as multilayer at render time ([`Layers`]).
    pub layered: usize,
    pub count: usize,
    /// The stage's origin `[x, z]` in AC's frame ([`Layers::world`]); the stage's `(x, z)` is AC's `(x - origin[0], -z - origin[1])`.
    pub origin: [f64; 2],
}

impl TextureBank {
    pub fn new(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        Ok(Self {
            zip: zip::ZipWriter::new(file),
            seen: BTreeMap::new(),
            aliases: BTreeMap::new(),
            stored: BTreeMap::new(),
            sizes: BTreeMap::new(),
            missing: BTreeSet::new(),
            simplified: BTreeSet::new(),
            baked: BTreeSet::new(),
            placeholders: BTreeSet::new(),
            normal_mapped: 0,
            highlighted: 0,
            sheened: 0,
            layered: 0,
            count: 0,
            origin: [0.; 2],
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
    /// name, and `opaque` says so. Anything else keeps its diffuse map. The gain is a dark bake's
    /// ([`multilayer::bake`]), which [`Extras::stored_at`] takes back; 1 for the rest.
    pub fn diffuse(&mut self, model: &Model, index: usize) -> Result<(String, bool, f32), String> {
        let mat = &model.materials[index];
        if let Some(file) = self.stub(model, mat)? {
            return Ok((file, true, 1.));
        }
        let multilayer = mat.shader.contains("Multilayer");
        if alpha_from_normal(mat) {
            // The detail baked first, under the diffuse's alpha (Roure's `pines_distant` has both).
            let baked = detail::bake(model, index);
            if let Some(bytes) = cut_by_normal(model, mat, baked.clone()) {
                if baked.is_some() {
                    self.baked.insert(mat.shader.clone());
                }
                return Ok((self.put(bytes, "dds")?, false, 1.));
            }
        }
        if multilayer {
            let metres = if mat.shader.contains("objsp") { [1.; 2] } else { multilayer::metres_per_uv(model, index) };
            // Only where `ReRbrLight` can take the gain back.
            if let Some(((w, h, rgba), gain)) = multilayer::bake(model, index, metres, light(mat).is_some()) {
                self.baked.insert(mat.shader.clone());
                return Ok((self.put(textures::encode_bc7(false, w, h, rgba), "dds")?, false, gain));
            }
            self.simplified.insert(mat.shader.clone());
        } else if mat.property("useDetail", 0.) > 0. {
            if let Some((w, h, rgba)) = detail::bake(model, index) {
                self.baked.insert(mat.shader.clone());
                return Ok((self.put(textures::encode_bc7(is_transparent(mat), w, h, rgba), "dds")?, false, 1.));
            }
            self.simplified.insert(mat.shader.clone());
        }
        Ok((self.base_diffuse(model, mat)?, false, 1.))
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
            let key = (name.clone(), alpha, data.len());
            if let Some(file) = self.stored.get(&key) {
                return Ok(Some(file.clone()));
            }
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
            self.stored.insert(key, file.clone());
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
        let mut extras = Extras {
            highlight: highlight(mat),
            sheen: sheen(mat),
            light: light(mat),
            foliage: foliage_light(mat),
            ..Extras::default()
        };
        let normal = mat.textures.get("txNormal").filter(|_| !mat.shader.contains("Multilayer"));
        if let Some(name) = normal {
            // A normal map that cannot be read is no normal map.
            match self.slot(model, mat, "txNormal", false) {
                Ok(Some(file)) => extras.normal = Some(file),
                Ok(None) => _ = self.missing.insert(format!("{}: {name}", mat.name)),
                Err(_) => _ = self.simplified.insert(format!("{}: {name}", mat.name)),
            }
        }
        // `ksPerPixelNM_UVMult`: the UVs carry the diffuse's multiplier, the map reads them at the ratio of the two.
        if let Some([diffuse, normal]) = uv_multipliers(model, mat).filter(|_| extras.normal.is_some()) {
            extras.bump_scale = Some([normal / diffuse; 2]);
        }
        self.detail_map(model, mat, &mut extras);
        if let Some(name) = mat.textures.get("txMaps").filter(|_| extras.highlight.is_some()) {
            extras.specular = self.share(model, name)?;
        }
        extras.layers = self.layers(model, mat);
        self.count(&extras);
        Ok(extras)
    }
    /// The multilayer material's own textures and numbers, for the renderer to sum per pixel ([`Layers`]); `None` for
    /// another shader, a placeholder diffuse, a missing or unreadable texture, and one past [`multilayer::LIMIT`] (which
    /// [`multilayer::bake`] leaves out too). The textures are stored under their own names, so materials sharing a
    /// detail texture share its file.
    pub fn layers(&mut self, model: &Model, mat: &Material) -> Option<Layers> {
        if !mat.shader.contains("Multilayer") || is_placeholder(model, mat) {
            return None;
        }
        for slot in ["txDiffuse", "txMask"] {
            let name = mat.textures.get(slot)?;
            let data = model.textures.get(name).filter(|d| !d.is_empty())?;
            let (w, h) =
                (*self.sizes.entry((name.clone(), data.len())).or_insert_with(|| {
                    match rbr_formats::dds::parse(data) {
                        Ok(dds) => Some((dds.width, dds.height)),
                        Err(_) => textures::decode(data).map(|(w, h, _)| (w, h)),
                    }
                }))?;
            if w > multilayer::LIMIT || h > multilayer::LIMIT {
                return None;
            }
        }
        let objsp = mat.shader.contains("objsp");
        let scale: [[f32; 2]; 4] = std::array::from_fn(|i| {
            let key = multilayer::MULTIPLIERS[i];
            let pair = if objsp { mat.pairs.get(key).copied().unwrap_or([0.; 2]) } else { [mat.property(key, 0.); 2] };
            pair.map(|m| if m.is_finite() && m >= 0. { m } else { 0. })
        });
        // The mask's alpha is the fourth layer's weight.
        let slots = [
            ("txDiffuse", false),
            ("txMask", true),
            ("txDetailR", false),
            ("txDetailG", false),
            ("txDetailB", false),
            ("txDetailA", false),
        ];
        let mut files = Vec::new();
        for (slot, alpha) in slots {
            files.push(self.slot(model, mat, slot, alpha).ok()??);
        }
        let mut files = files.into_iter();
        let mut next = || files.next().unwrap();
        let (diffuse, mask) = (next(), next());
        Some(Layers {
            diffuse,
            mask,
            details: std::array::from_fn(|_| next()),
            scale,
            magic: mat.property("magicMult", 1.),
            world: (!objsp).then_some(self.origin),
            uv: false,
            blend: None,
        })
    }
    fn count(&mut self, extras: &Extras) {
        self.normal_mapped += usize::from(extras.normal.is_some());
        self.highlighted += usize::from(extras.highlight.is_some());
        self.sheened += usize::from(extras.sheen.is_some());
        self.layered += usize::from(extras.layers.is_some());
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
    /// What a material written per tile takes: its highlight and sheen, and `txDetailNM`; the texture is baked, so
    /// there is no spec map, and the tile's UV set 0 is its own, so the normal map reads the source UV on the
    /// mesh's second set.
    pub fn tile_extras(&mut self, model: &Model, index: usize) -> Extras {
        let mat = &model.materials[index];
        let mut extras = Extras::default();
        if !is_placeholder(model, mat) {
            extras = Extras { highlight: highlight(mat), sheen: sheen(mat), light: light(mat), ..extras };
            self.detail_map(model, mat, &mut extras);
            extras.bump_uv = extras.normal.is_some();
        }
        self.count(&extras);
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

/// The UV multipliers of `ksPerPixelNM_UVMult` (`1 + diffuseMult`, `1 + normalMult`; its compiled `_ps.fxo`): the
/// diffuse is read at the UV times the first, the normal map at the UV times the second. `None` for another shader, a
/// multiplier that is not positive and a placeholder diffuse.
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

/// Whether AC's shader takes the material's alpha from its normal map: `ksPerPixelMultiMap_AT` and `_AT_NMDetail`
/// write `txNormal.a` (their disassembly; the diffuse's alpha only masks the detail), `ksPerPixelAT` and `_AT_NM` the
/// diffuse's.
fn alpha_from_normal(mat: &Material) -> bool {
    mat.shader.starts_with("ksPerPixelMultiMap_AT")
}

/// The diffuse (`colour`, else the material's own) of a material whose alpha AC takes from its normal map
/// ([`alpha_from_normal`]) with that alpha, as a DXT5 `.dds`: Roure's pines and Prunières' road edges (diffuse alpha
/// 0) keep their cut-out. `None` for other materials or without a normal map.
fn cut_by_normal(model: &Model, mat: &Material, colour: Option<textures::Image>) -> Option<Vec<u8>> {
    if !alpha_from_normal(mat) {
        return None;
    }
    let decode = |slot: &str| textures::decode(model.textures.get(mat.textures.get(slot)?)?);
    let (w, h, normal) = decode("txNormal")?;
    let diffuse = match colour {
        Some(colour) => colour,
        None => decode("txDiffuse")?,
    };
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
#[derive(Default)]
pub struct Extras {
    pub normal: Option<String>,
    pub specular: Option<String>,
    pub highlight: Option<(f32, f32)>,
    /// `ReRbrBumpScale`: the normal map is read at its UV times this ([`uv_multipliers`], `detailNMMult`).
    pub bump_scale: Option<[f32; 2]>,
    /// `ReRbrBumpUv`: the normal map reads the mesh's second UV set.
    pub bump_uv: bool,
    /// `ReRbrSheen`: the dry road sheen's roughness ([`sheen`]).
    pub sheen: Option<f32>,
    /// `(ksAmbient, ksDiffuse)` as the material gives them; [`normalise_light`] turns them into `ReRbrLight`.
    pub light: Option<[f32; 2]>,
    /// `ksAmbient + ksDiffuse` of a foliage shader's material ([`foliage_light`]); [`normalise_light`] turns it into
    /// `ReRbrLight` against the track's foliage.
    pub foliage: Option<f32>,
    /// `ReRbrGrass`: what CSP's GrassFX does on this material ([`super::csp::GrassFx::role`]).
    pub grass: rbr_btb::scene::Grass,
    /// `ReRbrLayer*`: the renderer sums a multilayer material per pixel.
    pub layers: Option<Layers>,
    /// The gain its diffuse was stored at ([`Extras::stored_at`]): a third `ReRbrLight` value that
    /// [`normalise_light`] divides out.
    pub stored_gain: Option<f32>,
}

impl Extras {
    /// These extras for a diffuse stored `gain` times brighter (a dark bake, [`multilayer::bake`]): the per-pixel sum
    /// takes it too (`magicMult`), the light its inverse.
    pub fn stored_at(mut self, gain: f32) -> Self {
        if gain != 1. && self.light.is_some() {
            if let Some(layers) = &mut self.layers {
                layers.magic *= gain;
            }
            self.stored_gain = Some(gain);
        }
        self
    }
}

/// A multilayer material for the renderer to draw per pixel (`ReRbrLayer*` keys), as `diffuse * (R*mask.r + G*mask.g
/// + B*mask.b + A*mask.a) * magicMult` ([`multilayer`]); the section's `diffuseTexture` stays the bake of the same sum.
/// The diffuse and the mask are read at the mesh's UV; each detail at `uv * scale` ([`Layers::world`] `None`, the
/// `_objsp` shaders) or at AC's world `(x, z) * scale`.
#[derive(Clone, Debug, PartialEq)]
pub struct Layers {
    /// The stored names of the diffuse, the mask and the details R, G, B and A.
    pub diffuse: String,
    pub mask: String,
    pub details: [String; 4],
    /// Each detail's `(u, v)` multiplier, the same on both axes for world space.
    pub scale: [[f32; 2]; 4],
    pub magic: f32,
    /// The stage's origin `[x, z]`: the details are read at world space, AC's `(x, z) = (stage x + origin[0], -(stage z + origin[1]))`.
    pub world: Option<[f64; 2]>,
    /// `ReRbrLayerUv=1`: the details read the mesh's UV set 1 and the weights are its vertices' (UV sets 2 and 3), no mask
    /// (`mask` is unused); DiRT Rally's terrain.
    pub uv: bool,
    /// `ReRbrLayerBlend=S` (with `uv`): DiRT Rally's road blend, `DetailBlendScale` `S`, the height maps in the alpha of the
    /// second and third details.
    pub blend: Option<f32>,
}

/// AC's foliage shaders. They light by `ks` as the others do (`ksTree_ps.fxo`: `tex * (ksAmbient * ambient * hemi +
/// ksDiffuse * sun * NdotL)`), but against the ground's reference St Jean - St Laurent's distant forest (`treeDistant`,
/// 0.29/0.055 against a ground of about 0.65/0.34) went from green pines to a dark smear, its crowns (`treeTop`, 0/0.8)
/// to ambient 0.05 and sun 2. So the foliage keeps its own brightness ([`foliage_light`]).
fn foliage(mat: &Material) -> bool {
    matches!(mat.shader.as_str(), "ksTree" | "ksGrass")
}

/// `(ksAmbient, ksDiffuse)` of a material that declares both, but foliage ([`foliage`]).
fn light(mat: &Material) -> Option<[f32; 2]> {
    if foliage(mat) {
        return None;
    }
    let both = [mat.properties.get("ksAmbient")?, mat.properties.get("ksDiffuse")?];
    Some(both.map(|v| *v).map(|v| v.max(0.)))
}

/// `ksAmbient + ksDiffuse` of a foliage material ([`foliage`]). Authors darken a pale part of a shared tree atlas by
/// its `ks`: Prunières–Embrun's `bushGreen` (0.065/0.05, the trees about 0.25/0.15 on the same `treeA_albedo`) drew
/// pale yellow without it. [`normalise_light`] darkens such a material against the track's foliage, never brightens
/// one (St Jean's `treeTop`).
fn foliage_light(mat: &Material) -> Option<f32> {
    if !foliage(mat) {
        return None;
    }
    Some(mat.properties.get("ksAmbient")?.max(0.) + mat.properties.get("ksDiffuse")?.max(0.))
}

/// AC's shaders light in display (gamma) values, ReRBR's in linear light: a `ksAmbient`/`ksDiffuse` ratio `r` is
/// `r^LIGHT_GAMMA` of ReRBR's light. Without it Sa Calobra's pale boulders (`extrarocks`, ratio 0.37/0.2 on a texture
/// 2.8 times the terrain's) read cream where AC draws them as grey as the ground.
const LIGHT_GAMMA: f32 = 2.2;
/// Factors below this or above that are the most `ReRbrLight` moves a material, in linear light. The top is low: Sa
/// Calobra's road (1.77/1.67, 3.5/3.1 linear) read 1.5 times AC's against its ground; at 2 it is 1.1 (AC's road 59,
/// ours 65 and 53 at the rock and wall views, 06:45).
const LIGHT_RANGE: (f32, f32) = (0.05, 2.0);
/// The highest `(ksAmbient, ksDiffuse)` reference of the installed tracks drawn as AC draws them: St Jean - St
/// Laurent's ambient 0.65 (its 0.65/0.34, Sa Calobra's 0.62/0.30, the same author's dark textures under high `ks`) and
/// Gabiria's and the Nordschleife's diffuse 0.35 (40 stages, 2026-11-20). AC lights `ks` absolutely, so a track whose
/// reference is higher (Prunières-Embrun's 1.6/1.2: its terrain 0.03 of display white before the light) is lit that
/// much brighter overall: its factors and range take `(reference / most)^LIGHT_GAMMA`, 7.2/15 there, about what Sa
/// Calobra's terrain gives (8/21). Below it the reference stays the track's own.
const REFERENCE_MOST: [f32; 2] = [0.65, 0.35];

/// The weighted median of `(value, weight)` pairs.
fn weighted_median(mut values: Vec<(f32, f64)>) -> Option<f32> {
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let half = values.iter().map(|v| v.1).sum::<f64>() / 2.0;
    let mut seen = 0.0;
    values
        .into_iter()
        .find(|v| {
            seen += v.1;
            seen >= half
        })
        .map(|v| v.0)
}

/// Rewrites the `ReRbrLight=<ksAmbient>,<ksDiffuse>` lines [`section_with`] wrote into factors relative to the
/// track's reference: the median of each over the drawn opaque materials, by the `areas` (m²) each section draws, to
/// [`LIGHT_GAMMA`] in linear light, times the gain of a reference above [`REFERENCE_MOST`], over a third value's
/// ([`Extras::stored_gain`]) to the same power. The `ReRbrFoliage=<sum>` lines become `ReRbrLight` the same way
/// against the median of the foliage's, at most 1. A factor within 2% of 1 for both drops the line.
pub fn normalise_light(materials: &str, areas: &BTreeMap<String, f64>) -> String {
    /// A section's light lines as [`section_with`] wrote them.
    #[derive(Default)]
    struct Lit<'a> {
        name: &'a str,
        opaque: bool,
        light: Option<[f32; 2]>,
        stored: Option<f32>,
        foliage: Option<f32>,
    }
    let mut sections = Vec::<Lit>::new();
    for line in materials.lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push(Lit { name, ..Lit::default() });
        } else if let Some(last) = sections.last_mut() {
            last.opaque |= line == "techniqueName=Default";
            if let Some(v) = line.strip_prefix("ReRbrLight=") {
                let v: Vec<f32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                (last.light, last.stored) = match v[..] {
                    [a, d] => (Some([a, d]), None),
                    [a, d, stored] => (Some([a, d]), Some(stored)),
                    _ => (None, None),
                };
            }
            if let Some(v) = line.strip_prefix("ReRbrFoliage=") {
                last.foliage = v.parse().ok();
            }
        }
    }
    let median = |value: &dyn Fn(&Lit) -> Option<f32>| {
        weighted_median(sections.iter().filter_map(|s| Some((value(s)?, *areas.get(s.name)?))).collect())
            .filter(|m| *m > 0.)
    };
    let opaque = |axis: usize| median(&|s| s.light.filter(|_| s.opaque).map(|light| light[axis]));
    let reference = opaque(0).zip(opaque(1)).map(|(ambient, diffuse)| [ambient, diffuse]);
    let foliage = median(&|s| s.foliage);
    let mut out = String::new();
    let mut at = 0;
    for line in materials.lines() {
        if line.starts_with('[') {
            at += 1;
        }
        let factors: Option<[f32; 2]> = if line.starts_with("ReRbrLight=") {
            let lit = &sections[at - 1];
            reference.map(|reference| {
                let stored = lit.stored.unwrap_or(1.).powf(LIGHT_GAMMA);
                let raw = lit.light.unwrap_or(reference);
                std::array::from_fn(|i| {
                    let gain = (reference[i] / REFERENCE_MOST[i]).max(1.).powf(LIGHT_GAMMA);
                    ((raw[i] / reference[i]).powf(LIGHT_GAMMA) * gain).clamp(LIGHT_RANGE.0, LIGHT_RANGE.1 * gain)
                        / stored
                })
            })
        } else if line.starts_with("ReRbrFoliage=") {
            let sum = sections[at - 1].foliage;
            sum.zip(foliage).map(|(sum, foliage)| [(sum / foliage).powf(LIGHT_GAMMA).clamp(LIGHT_RANGE.0, 1.); 2])
        } else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        if let Some(f) = factors.filter(|f| f.iter().any(|v| (v - 1.).abs() > 0.02)) {
            let _ = writeln!(out, "ReRbrLight={},{}", decimal(f[0]), decimal(f[1]));
        }
    }
    out
}

/// The roughness of the dry sheen of a material, `None` for a cut-out, blended or foliage one. The renderer draws it
/// only where the collision surface under the vertex is tarmac. From AC's Blinn-Phong exponent `n`: GGX `alpha = sqrt(2 / (n + 2))`, roughness `sqrt(alpha)` in
/// 0.45 to 0.85; a material without `ksSpecular` or with `n < 1` takes [`ROAD_ROUGHNESS`]. AC's own fresnel values are
/// author tuning and not read.
pub fn sheen(mat: &Material) -> Option<f32> {
    if is_transparent(mat) || mat.shader.contains("Grass") {
        return None;
    }
    let (specular, n) = (mat.property("ksSpecular", 0.), mat.property("ksSpecularEXP", 0.));
    if specular <= 0. || n < 1. {
        return Some(ROAD_ROUGHNESS);
    }
    Some((2. / (n + 2.)).sqrt().sqrt().clamp(0.45, 0.85))
}

/// The sheen roughness of a road without a usable `ksSpecularEXP`.
const ROAD_ROUGHNESS: f32 = 0.75;

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
        (false, false) => (extras.sheen.is_some() || extras.layers.is_some()).then_some("BTB_Diffuse"),
    };
    if let Some(effect) = effect {
        let _ = writeln!(out, "ReRbrEffect={effect}");
    }
    if extras.normal.is_some() {
        if let Some([u, v]) = extras.bump_scale {
            let _ = writeln!(out, "ReRbrBumpScale={},{}", decimal(u), decimal(v));
        }
        if extras.bump_uv {
            let _ = writeln!(out, "ReRbrBumpUv=1");
        }
    }
    if let Some(roughness) = extras.sheen {
        let _ = writeln!(out, "ReRbrSheen={}", decimal(roughness));
    }
    if let Some([ambient, diffuse]) = extras.light {
        let gain = extras.stored_gain.map(|g| format!(",{}", decimal(g))).unwrap_or_default();
        let _ = writeln!(out, "ReRbrLight={},{}{gain}", decimal(ambient), decimal(diffuse));
    }
    if let Some(sum) = extras.foliage {
        let _ = writeln!(out, "ReRbrFoliage={}", decimal(sum));
    }
    match extras.grass {
        rbr_btb::scene::Grass::None => {}
        rbr_btb::scene::Grass::Grows(0) => out.push_str("ReRbrGrass=1\n"),
        rbr_btb::scene::Grass::Grows(p) => {
            let _ = writeln!(out, "ReRbrGrass=1\nReRbrGrassProfile={p}");
        }
        rbr_btb::scene::Grass::Hides => out.push_str("ReRbrGrass=hide\n"),
        rbr_btb::scene::Grass::HidesAlpha => out.push_str("ReRbrGrass=hide-alpha\n"),
    }
    if let Some(layers) = &extras.layers {
        let list = |values: &mut dyn Iterator<Item = String>| values.collect::<Vec<_>>().join(",");
        let _ = writeln!(out, "ReRbrLayerDiffuse={}", layers.diffuse);
        if layers.uv {
            out.push_str("ReRbrLayerUv=1\n");
        } else {
            let _ = writeln!(out, "ReRbrLayerMask={}", layers.mask);
        }
        if let Some(scale) = layers.blend {
            let _ = writeln!(out, "ReRbrLayerBlend={}", decimal(scale));
        }
        let _ = writeln!(out, "ReRbrLayerDetail={}", layers.details.join(","));
        let _ = writeln!(out, "ReRbrLayerScale={}", list(&mut layers.scale.iter().flatten().map(|m| decimal(*m))));
        let _ = writeln!(out, "ReRbrLayerMagic={}", decimal(layers.magic));
        if let Some([x, z]) = layers.world {
            let _ = writeln!(out, "ReRbrLayerWorld={x:.6},{z:.6}");
        }
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_grass_material_is_marked() {
        let mut out = String::new();
        section_with(
            &mut out,
            "m",
            "t.dds",
            false,
            &Extras { grass: rbr_btb::scene::Grass::Grows(0), ..Extras::default() },
        );
        assert!(out.ends_with("ReRbrGrass=1\n\n"));
        let mut profiled = String::new();
        section_with(
            &mut profiled,
            "m",
            "t.dds",
            false,
            &Extras { grass: rbr_btb::scene::Grass::Grows(2), ..Extras::default() },
        );
        assert!(profiled.ends_with("ReRbrGrass=1\nReRbrGrassProfile=2\n\n"));
        section(&mut out, "n", "t.dds", false);
        assert_eq!(out.matches("ReRbrGrass").count(), 1);
    }

    #[test]
    fn a_multimap_at_material_takes_its_alpha_from_its_normal_map() {
        // A 2x2 diffuse (its alpha masks the detail); the normal map cuts its left column.
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
        let alpha =
            |bytes: Vec<u8>| textures::decode(&bytes).unwrap().2.chunks_exact(4).map(|t| t[3]).collect::<Vec<_>>();
        assert_eq!(alpha(cut_by_normal(&model, &on("ksPerPixelMultiMap_AT", "n"), None).unwrap()), [0, 255, 0, 255]);
        // Prunières' road edges: the diffuse's alpha is 0 throughout, the normal map's still cuts.
        let clear = Some((2, 2, [200, 100, 50, 0].repeat(4)));
        assert_eq!(alpha(cut_by_normal(&model, &on("ksPerPixelMultiMap_AT", "n"), clear).unwrap()), [0, 255, 0, 255]);
        assert_eq!(alpha(cut_by_normal(&model, &on("ksPerPixelMultiMap_AT", "solid"), None).unwrap()), [255; 4]);
        assert!(cut_by_normal(&model, &on("ksPerPixelMultiMap", "n"), None).is_none(), "not alpha tested");
        assert!(cut_by_normal(&model, &on("ksPerPixelAT_NM", "n"), None).is_none(), "the diffuse's alpha");
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
            ("CROCK", 0.82, true, None),
            ("COBBLE", 0.96, true, None),
            ("STONE", 0.85, false, None),
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
        // Loose rock is gravel; cobbles that grip like tarmac and clean stone stay rock.
        assert_eq!(surfaces.id("CROCK", false), 17);
        assert_eq!(surfaces.id("COBBLE", false), 69);
        assert_eq!(surfaces.id("STONE", false), 69);
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
                "[s]\neffectName=BTB_Diffuse\ntechniqueName=Default\nambient=0.4,0.4,0.4,1.0\nSpecularPower=15.0\nSpecularColor=0.5,0.5,0.5\ndiffuseTexture=ac_diffuse.dds\nnormalTexture={}\nspecularTexture={}\nReRbrEffect=BTB_DiffuseBumpSpecular\nReRbrSheen=0.58566\n\n",
                extras.normal.unwrap(),
                extras.specular.unwrap()
            )
        );
    }
    #[test]
    fn uv_mult_scales_the_normal_map_against_the_diffuse() {
        let mut model = extras_model();
        let textures = [("txDiffuse", "d"), ("txNormal", "n")].map(|(s, n)| (s.to_owned(), n.to_owned()));
        let properties = [("diffuseMult", 3.), ("normalMult", 0.5)].map(|(k, v)| (k.to_owned(), v));
        let mat = |shader: &str| Material {
            name: "m".into(),
            shader: shader.into(),
            textures: textures.clone().into(),
            properties: properties.clone().into(),
            ..Material::default()
        };
        model.materials.extend([mat("ksPerPixelNM_UVMult"), mat("ksPerPixelNM")]);
        assert_eq!(uv_multipliers(&model, &model.materials[0]), Some([4., 1.5]));
        assert_eq!(uv_multipliers(&model, &model.materials[1]), None);
        let path = std::env::temp_dir().join(format!("ac-uvmult-{}.zip", std::process::id()));
        let mut bank = TextureBank::new(&path).unwrap();
        let (scaled, plain) = (bank.extras(&model, 0).unwrap(), bank.extras(&model, 1).unwrap());
        bank.finish().unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!((scaled.bump_scale, plain.bump_scale), (Some([0.375; 2]), None));
        let mut out = String::new();
        section_with(&mut out, "s", "d.dds", false, &scaled);
        let line = out.lines().find_map(|l| l.strip_prefix("ReRbrBumpScale=")).unwrap();
        assert_eq!(rbr_btb::track::decimal_floats(line), vec![0.375, 0.375]);
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
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\rIDATx\xdacd`\xf8_\x0f\0\x02\x87\x01\x80\xebG\xba\x92\0\0\0\0IEND\xaeB`\x82";
    fn bank_for(name: &str) -> (TextureBank, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("ac-{name}-{}.zip", std::process::id()));
        (TextureBank::new(&path).unwrap(), path)
    }
    fn multilayer_nm(pair: [f32; 2], detail: &str) -> Model {
        let mut model = extras_model();
        model.textures.insert("p".into(), PNG.to_vec());
        let mat = Material {
            name: "m".into(),
            shader: "ksMultilayer_fresnel_nm".into(),
            textures: [("txDiffuse", "d"), ("txDetailNM", detail)].map(|(s, n)| (s.into(), n.into())).into(),
            pairs: BTreeMap::from([("detailNMMult".to_owned(), pair)]),
            ..Material::default()
        };
        model.materials.push(mat);
        model
    }
    fn layered(shader: &str) -> (String, Extras) {
        let mut model = extras_model();
        for (name, texel) in [("a", [1, 2, 3, 255]), ("b", [4, 5, 6, 255])] {
            model.textures.insert(name.into(), rbr_formats::dds::rgba8_file(2, 2, &texel.repeat(4)));
        }
        // Detail R and B share a texture, as Sa Calobra's layers do.
        let textures = [
            ("txDiffuse", "d"),
            ("txMask", "m"),
            ("txDetailR", "a"),
            ("txDetailG", "b"),
            ("txDetailB", "a"),
            ("txDetailA", "b"),
        ];
        let mut mat = Material {
            name: "m".into(),
            shader: shader.into(),
            textures: textures.map(|(s, n)| (s.into(), n.into())).into(),
            ..Material::default()
        };
        mat.properties.insert("magicMult".into(), 1.5);
        for (i, key) in multilayer::MULTIPLIERS.iter().enumerate() {
            mat.properties.insert((*key).into(), 0.5 + i as f32);
            mat.pairs.insert((*key).into(), [10. * (i + 1) as f32, 0.25]);
        }
        model.materials.push(mat);
        let (mut bank, path) = bank_for(&format!("layers-{shader}"));
        bank.origin = [-1234.5, 20.25];
        let extras = bank.extras(&model, 0).unwrap();
        let (layered, count) = (bank.layered, bank.count);
        bank.finish().unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!((layered, count), (1, 4), "diffuse, mask and two shared details are stored once");
        let mut out = String::new();
        section_with(&mut out, "s", "bake.dds", false, &extras);
        (out, extras)
    }
    #[test]
    fn world_space_multilayer_writes_its_textures_scales_and_origin() {
        let (out, extras) = layered("ksMultilayer_fresnel_nm");
        let layers = extras.layers.unwrap();
        assert_eq!(layers.details[0], layers.details[2]);
        assert_eq!(layers.scale, [[0.5; 2], [1.5; 2], [2.5; 2], [3.5; 2]]);
        let value = |key: &str| out.lines().find_map(|l| l.strip_prefix(key)).unwrap().to_owned();
        assert!(out.contains("diffuseTexture=bake.dds\nReRbrEffect=BTB_Diffuse\n"), "{out}");
        assert_eq!(value("ReRbrLayerMask="), layers.mask);
        assert_eq!(value("ReRbrLayerDetail="), layers.details.join(","));
        assert_eq!(
            rbr_btb::track::decimal_floats(&value("ReRbrLayerScale=")),
            [0.5, 0.5, 1.5, 1.5, 2.5, 2.5, 3.5, 3.5]
        );
        assert_eq!(rbr_btb::track::decimal_floats(&value("ReRbrLayerMagic=")), [1.5]);
        assert_eq!(rbr_btb::track::decimal_floats(&value("ReRbrLayerWorld=")), [-1234.5, 20.25]);
    }
    #[test]
    fn an_objsp_multilayer_reads_its_pairs_at_the_uv_and_has_no_world() {
        let (out, extras) = layered("ksMultilayer_objsp");
        assert_eq!(extras.layers.unwrap().scale[1], [20., 0.25]);
        assert!(
            out.contains("ReRbrLayerScale=10.0,0.25,20.0,0.25,30.0,0.25,40.0,0.25\n")
                && !out.contains("ReRbrLayerWorld")
        );
    }
    #[test]
    fn multilayers_detail_normal_map_is_its_normal_map_at_the_pair() {
        let model = multilayer_nm([2000., 4000.5], "n");
        let (mut bank, path) = bank_for("detailnm");
        let extras = bank.extras(&model, 0).unwrap();
        assert!(extras.normal.is_some() && !extras.bump_uv);
        let mut out = String::new();
        section_with(&mut out, "s", "d.dds", false, &extras);
        assert!(out.contains("ReRbrEffect=BTB_DiffuseBump\nReRbrBumpScale=2000.0,4000.5\n"), "{out}");
        assert!(!out.contains("ReRbrBumpUv"));
        // A tile reads it on the source UV.
        let tile = bank.tile_extras(&model, 0);
        let mut out = String::new();
        section_with(&mut out, "s", "d.dds", false, &tile);
        assert!(out.contains("ReRbrBumpScale=2000.0,4000.5\nReRbrBumpUv=1\n"), "{out}");
        // A one-pixel map and a (0, 0) pair give none.
        for (pair, name) in [([2., 2.], "p"), ([0., 0.], "n")] {
            let model = multilayer_nm(pair, name);
            assert!(bank.extras(&model, 0).unwrap().normal.is_none(), "{pair:?} {name}");
        }
        bank.finish().unwrap();
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn a_materials_sheen_comes_from_its_exponent() {
        let mat = |shader: &str, specular: f32, exp: f32, alpha: bool| {
            let mut m = Material { name: "m".into(), shader: shader.into(), alpha_test: alpha, ..Material::default() };
            m.properties.insert("ksSpecular".into(), specular);
            m.properties.insert("ksSpecularEXP".into(), exp);
            m
        };
        let rough = |n: f32| sheen(&mat("ksPerPixel", 1., n, false));
        // n 15: alpha 0.343, roughness 0.586; a low n clamps to 0.85, a high one to 0.45.
        assert!((rough(15.).unwrap() - (2f32 / 17.).sqrt().sqrt()).abs() < 1e-6);
        assert_eq!((rough(1.5), rough(5000.)), (Some(0.85), Some(0.45)));
        // No ksSpecular or a tiny exponent: the default.
        assert_eq!(sheen(&mat("ksPerPixel", 0., 50., false)), Some(ROAD_ROUGHNESS));
        assert_eq!(rough(0.5), Some(ROAD_ROUGHNESS));
        // A cut-out, foliage.
        assert_eq!(sheen(&mat("ksPerPixelAT", 1., 15., true)), None);
        assert_eq!(sheen(&mat("ksTree", 1., 15., false)), None);
        assert_eq!(sheen(&mat("ksGrass", 1., 15., false)), None);
    }
    #[test]
    fn sheen_is_written_and_read_back_for_opaque_materials_and_not_placeholders() {
        let mut model = extras_model();
        model.textures.insert("p".into(), PNG.to_vec());
        let mat = |diffuse: &str, shader: &str, alpha: bool| Material {
            name: "m".into(),
            shader: shader.into(),
            alpha_test: alpha,
            textures: BTreeMap::from([("txDiffuse".to_owned(), diffuse.to_owned())]),
            ..Material::default()
        };
        model.materials = vec![
            mat("d", "ksPerPixel", false),
            mat("p", "ksPerPixel", false),
            mat("d", "ksPerPixelAT", true),
            mat("d", "ksGrass", false),
        ];
        let (mut bank, path) = bank_for("sheen");
        let marked: Vec<_> = (0..4).map(|i| bank.extras(&model, i).unwrap().sheen).collect();
        assert_eq!(marked, [Some(ROAD_ROUGHNESS), None, None, None]);
        assert_eq!(bank.tile_extras(&model, 0).sheen, Some(ROAD_ROUGHNESS));
        assert_eq!(bank.sheened, 2);
        let extras = bank.extras(&model, 0).unwrap();
        let mut out = String::new();
        section_with(&mut out, "s", "d.dds", false, &extras);
        assert!(out.contains("ReRbrEffect=BTB_Diffuse\nReRbrSheen=0.75\n"), "{out}");
        let line = out.lines().find_map(|l| l.strip_prefix("ReRbrSheen=")).unwrap();
        assert_eq!(rbr_btb::track::decimal_floats(line), vec![0.75]);
        bank.finish().unwrap();
        let _ = std::fs::remove_file(path);
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

    #[test]
    fn foliage_shaders_take_their_light_against_the_foliage() {
        let mat = |shader: &str| Material {
            shader: shader.into(),
            properties: [("ksAmbient".to_owned(), 0.0), ("ksDiffuse".to_owned(), 0.8)].into(),
            ..Material::default()
        };
        assert_eq!(
            [mat("ksTree"), mat("ksGrass"), mat("ksPerPixel")].map(|m| (light(&m), foliage_light(&m))),
            [(None, Some(0.8)), (None, Some(0.8)), (Some([0.0, 0.8]), None)]
        );
        let section = |name: &str, light: &str| {
            format!("[{name}]\neffectName=BTB_Diffuse\ntechniqueName=Transparent\n{light}diffuseTexture=t.dds\n\n")
        };
        let materials = [
            section("tree", "ReRbrFoliage=0.4\n"),
            section("bush", "ReRbrFoliage=0.1\n"),
            section("crown", "ReRbrFoliage=0.8\n"),
        ]
        .concat();
        let areas = BTreeMap::from([("tree", 50.), ("bush", 10.), ("crown", 10.)].map(|(k, v)| (k.to_owned(), v)));
        let out = normalise_light(&materials, &areas);
        assert!(!out.contains("ReRbrFoliage"));
        // Against the trees' 0.4, with no opaque reference: the bush at 0.25^2.2, the crown not brightened.
        assert_eq!(out.matches("ReRbrLight=").count(), 1);
        assert!(out.contains("[bush]\neffectName=BTB_Diffuse\ntechniqueName=Transparent\nReRbrLight=0.05,0.05\n"));
    }

    #[test]
    fn light_is_written_relative_to_the_weighted_median_and_clamped() {
        let section = |name: &str, technique: &str, light: &str| {
            format!("[{name}]\neffectName=BTB_Diffuse\ntechniqueName={technique}\n{light}diffuseTexture=t.dds\n\n")
        };
        let materials = [
            section("road", "Default", "ReRbrLight=0.55,0.25\n"),
            section("rock", "Default", "ReRbrLight=0.11,0.03\n"),
            section("same", "Default", "ReRbrLight=0.55,0.25\n"),
            section("leaf", "Transparent", "ReRbrLight=0.15,0.05\n"),
            section("plain", "Default", ""),
        ]
        .concat();
        let triangles = BTreeMap::from(
            [("road", 60.), ("rock", 10.), ("same", 10.), ("leaf", 1000.), ("plain", 5.)]
                .map(|(k, v)| (k.to_owned(), v)),
        );
        let out = normalise_light(&materials, &triangles);
        let light = |name: &str| {
            let at = out.find(&format!("[{name}]")).unwrap();
            out[at..]
                .lines()
                .take_while(|l| !l.is_empty())
                .find_map(|l| l.strip_prefix("ReRbrLight="))
                .map(str::to_owned)
        };
        // The median is the road's 0.55/0.25 (70 of 80 opaque triangles; the transparent leaf does not count).
        assert_eq!(light("road"), None);
        assert_eq!(light("same"), None);
        // Ratios to 2.2 (linear light), clamped: rock 0.2/0.12 and leaf 0.27/0.2 to the floor but the leaf's ambient.
        assert_eq!(light("rock").as_deref(), Some("0.05,0.05"));
        assert_eq!(light("leaf").as_deref(), Some("0.057359,0.05"));
        assert_eq!(light("plain"), None);
        // Without any opaque material to reference nothing is written.
        let none = normalise_light(&section("leaf", "Transparent", "ReRbrLight=0.15,0.05\n"), &triangles);
        assert!(!none.contains("ReRbrLight"));
        // A reference above `REFERENCE_MOST` (1.3/0.7, twice it) lights the whole track 2^2.2 brighter, its range too:
        // the road at the gain, the rock (0.0016/0.0007 of it) at the floor, a ratio of 2 at 4 times the top.
        let bright = [
            section("road", "Default", "ReRbrLight=1.3,0.7\n"),
            section("rock", "Default", "ReRbrLight=0.07,0.05\n"),
            section("same", "Default", "ReRbrLight=2.6,1.4\n"),
            section("stored", "Default", "ReRbrLight=1.3,0.7,2.0\n"),
        ]
        .concat();
        let out = normalise_light(&bright, &triangles);
        let light = |name: &str| {
            let at = out.find(&format!("[{name}]")).unwrap();
            out[at..]
                .lines()
                .take_while(|l| !l.is_empty())
                .find_map(|l| l.strip_prefix("ReRbrLight="))
                .map(str::to_owned)
        };
        assert_eq!(light("road").as_deref(), Some("4.594794,4.594794"));
        assert_eq!(light("rock").as_deref(), Some("0.05,0.05"));
        assert_eq!(light("same").as_deref(), Some("9.189588,9.189588"));
        // A diffuse stored twice as bright takes 2^2.2 of it back: the road's gain over it is 1.
        assert_eq!(light("stored"), None);
    }
}
