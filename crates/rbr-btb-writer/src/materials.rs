//! The `materials.ini` sections a converter writes: a diffuse texture and what ReRBR's renderer reads besides it.
use std::fmt::Write as _;

/// What a section carries besides its diffuse, for ReRBR's renderer: files in the textures, and the highlight's
/// `(ksSpecular, ksSpecularEXP)`.
#[derive(Default)]
pub struct Extras {
    pub normal: Option<String>,
    pub specular: Option<String>,
    pub highlight: Option<(f32, f32)>,
    /// `ReRbrBumpScale`: the normal map is read at its UV times this (AC's `detailNMMult`).
    pub bump_scale: Option<[f32; 2]>,
    /// `ReRbrBumpUv`: the normal map reads the mesh's second UV set.
    pub bump_uv: bool,
    /// `ReRbrSheen`: the dry road sheen's roughness .
    pub sheen: Option<f32>,
    /// `(ksAmbient, ksDiffuse)` as the material gives them; `normalise_light` turns them into `ReRbrLight`.
    pub light: Option<[f32; 2]>,
    /// `ksAmbient + ksDiffuse` of a foliage shader's material ; `normalise_light` turns it into
    /// `ReRbrLight` against the track's foliage.
    pub foliage: Option<f32>,
    /// `ReRbrGrass`: what CSP's GrassFX does on this material .
    pub grass: rbr_btb::scene::Grass,
    /// `ReRbrLayer*`: the renderer sums a multilayer material per pixel.
    pub layers: Option<Layers>,
    /// The gain its diffuse was stored at ([`Extras::stored_at`]): a third `ReRbrLight` value that
    /// `normalise_light` divides out.
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
/// + B*mask.b + A*mask.a) * magicMult` ; the section's `diffuseTexture` stays the bake of the same sum.
/// The diffuse and the mask are read at the mesh's UV; each detail at `uv * scale` (`Layers::world` `None`, the
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


/// A float with a decimal point and no exponent: BTB reads a bare `1,1,1` as 1.1 and 1.
pub fn decimal(v: f32) -> String {
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

