//! Part and material data of a car, from `<car>.ini` and `<car>_shaders.ini`.
//!
//! A part (`[e_body]`, `[e_wheel_tarmac_lf]`, ...) names one shader class and one material per face,
//! as `FaceBlock<N>_Shader` and `FaceBlock<N>_Material`, where `N` is the face's index in the
//! model's mesh (rbr-recomp `notes/formats/sgc.md`, section 8). `Switch = true` marks parts that the car shows
//! only in some states (lights, brake discs). A material is drawn with its diffuse bitmap, found
//! through the `<material>_diffuse_bitmap` section and the `TextureSet0` texture set
//! (`notes/formats.md` "Cars", `notes/formats.md` "Shaders and vertex layouts").

use crate::ini::Section;

/// One face's shader class and material, as the part section names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceBlock {
    /// Shader class, for example `CarPaint` or `CarTire`.
    pub shader: String,
    /// Material name, for example `external::carpaint`. Equal to the face's name in the model.
    pub material: String,
}

/// The section named `name`, ignoring ASCII case. The game matches section names this way.
pub fn section<'a>(sections: &'a [Section], name: &str) -> Option<&'a Section> {
    sections.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// The face blocks of a part, in face order: `FaceBlock0`, `FaceBlock1`, ... up to the first index
/// with no shader or material key.
pub fn face_blocks(part: &Section) -> Vec<FaceBlock> {
    (0..)
        .map_while(|n| {
            Some(FaceBlock {
                shader: part.get(&format!("FaceBlock{n}_Shader"))?.to_owned(),
                material: part.get(&format!("FaceBlock{n}_Material"))?.to_owned(),
            })
        })
        .collect()
}

/// Whether the part is shown only in some states (`Switch = true`: lights, brake discs, and so on).
pub fn is_switched(part: &Section) -> bool {
    part.get("Switch").is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// The DOS path (`cars\Xsara\textures\xsara_external.dds`) of a material's diffuse texture, or `None`
/// when the material has no bitmap. `shaders` is the sections of `<car>_shaders.ini`. As the game's car shaders
/// (`CShaderCarBase::Load`, HH 0x1007b3e0): the texture is the `[<material>_diffuse_bitmap]` section's `Filename`
/// whatever the material's own section says, or whether it has one (RSF's GR Yaris Rally2 has only the bitmap
/// sections for `internal::hud`).
pub fn diffuse_path<'a>(shaders: &'a [Section], material: &str) -> Option<&'a str> {
    bitmap_path(shaders, material, "diffuse")
}

/// The DOS path of a material's normal map, or `None` when it has none: ours, the `[<material>_bump_bitmap]` section's
/// `Filename` the way [`diffuse_path`] reads the diffuse one (no retail car has the section; a converted car's
/// materials can, `rbr-dirt-rally`).
pub fn bump_path<'a>(shaders: &'a [Section], material: &str) -> Option<&'a str> {
    bitmap_path(shaders, material, "bump")
}

/// The DOS path of the `[<material>_<kind>_bitmap]` section's `Filename`, through the texture set.
fn bitmap_path<'a>(shaders: &'a [Section], material: &str, kind: &str) -> Option<&'a str> {
    let file = section(shaders, &format!("{material}_{kind}_bitmap"))?.get("Filename").filter(|f| !f.is_empty())?;
    texture_set(shaders).find_map(|(name, path)| name.eq_ignore_ascii_case(file).then_some(path))
}

/// The `TextureSet0` texture set's entries (file name, DOS path). Ours: the entries of every section with the set's
/// name, as the Fiesta Rally3 (RSF) has an empty `[TSDefault]` header before its own.
fn texture_set(shaders: &[Section]) -> impl Iterator<Item = (&str, &str)> {
    let name = section(shaders, "TextureSets").and_then(|s| s.get("TextureSet0"));
    shaders
        .iter()
        .filter(move |s| name.is_some_and(|n| s.name.eq_ignore_ascii_case(n)))
        .flat_map(|s| s.entries.iter().map(|(file, path)| (file.as_str(), path.as_str())))
}

/// The DOS path of the texture whose `FriendlyName` is `name` (`Scrape`, `InternalScrape`, `Dirt`, ...), which the
/// car shader classes bind by that name (`GetFriendlyTexture`, `notes/formats.md` "Shaders and vertex layouts"): the file's own section
/// carries the name, and the `TextureSet0` texture set its path.
pub fn friendly_path<'a>(shaders: &'a [Section], name: &str) -> Option<&'a str> {
    texture_set(shaders)
        .find(|(file, _)| section(shaders, file).and_then(|s| s.get("FriendlyName")) == Some(name))
        .map(|(_, path)| path)
}

/// How a material's faces reflect (ours, a converted DiRT Rally car's interior and exterior trim: `rbr-dirt-rally` writes
/// its shaders' constants as the material section's `Finish*` keys; no retail car has them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finish {
    pub kind: FinishKind,
    /// `FinishFresnel`: the reflection's strength over the specular texel, `min` facing the eye to `max` at grazing,
    /// along `(1 - n.v)^power`.
    pub fresnel: [f32; 3],
    /// `FinishEnvironment`: the reflection's colour and its blur (0 sharp, 1 soft); `None` for a material that reflects
    /// nothing.
    pub environment: Option<[f32; 4]>,
    /// `FinishPhong`: the sun's highlight, `pow(R.L, power)` times `scale` and the colour.
    pub phong: [f32; 5],
    /// `FinishNoise`: a flock's fibres' noise, its size over the uv.
    pub noise: f32,
    /// `FinishCarbon`: a carbon weave's fibre highlight, its strength, power and falloff power.
    pub carbon: [f32; 3],
    /// `FinishSpecular`: the gain on the specular texel, the product at most 1 (the exterior's `sat(2 * spec)` over the
    /// reference the converter wrote the specular with); 1 without the key.
    pub specular: f32,
}

/// What shader a [`Finish`] stands for: DiRT Rally's `interior.fx`, `interior_flock.fx` (suede), `interior_carbon.fx`;
/// the exterior's `carpaint_metallic.fx` (Shiny Metal), `car_wheel.fx`, `car_matt.fx`, and `car_light_pod.fx`, whose
/// reflection the lit colour's brightness gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishKind {
    Plain,
    Flock,
    Carbon,
    Exterior,
    Gated,
}

/// A material's [`Finish`], or `None` without a known `Finish` key. Keys missing or short take the converter's defaults
/// (Fresnel 0 1 1, Phong 10 1 1 1 1, noise 10, carbon 0 1 1, specular 1).
pub fn finish(shaders: &[Section], material: &str) -> Option<Finish> {
    let section = section(shaders, material)?;
    let kind = match section.get("Finish")?.trim().to_ascii_lowercase().as_str() {
        "plain" => FinishKind::Plain,
        "flock" => FinishKind::Flock,
        "carbon" => FinishKind::Carbon,
        "exterior" => FinishKind::Exterior,
        "gated" => FinishKind::Gated,
        _ => return None,
    };
    let numbers = |key: &str| -> Option<Vec<f32>> {
        section.get(key).map(|v| v.split_whitespace().filter_map(|n| n.parse().ok()).collect())
    };
    fn filled<const N: usize>(values: Option<Vec<f32>>, default: [f32; N]) -> [f32; N] {
        let values = values.unwrap_or_default();
        std::array::from_fn(|i| values.get(i).copied().unwrap_or(default[i]))
    }
    Some(Finish {
        kind,
        fresnel: filled(numbers("FinishFresnel"), [0.0, 1.0, 1.0]),
        environment: numbers("FinishEnvironment").map(|v| filled(Some(v), [1.0, 1.0, 1.0, 0.0])),
        phong: filled(numbers("FinishPhong"), [10.0, 1.0, 1.0, 1.0, 1.0]),
        noise: filled(numbers("FinishNoise"), [10.0])[0],
        carbon: filled(numbers("FinishCarbon"), [0.0, 1.0, 1.0]),
        specular: filled(numbers("FinishSpecular"), [1.0])[0],
    })
}

/// Whether a material has an opacity channel (`OpacityChannel = true`). The glass classes are alpha
/// blended only when it does (`CarGlass` in the 1.02 shader, `notes/formats.md` "Shaders and vertex layouts").
pub fn opacity_channel(shaders: &[Section], material: &str) -> bool {
    section(shaders, material).and_then(|s| s.get("OpacityChannel")).is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ini;

    const PART: &str = "[e_body]\r\n\
        FaceBlock0_Material = external::carpaint\r\n\
        FaceBlock0_Shader = CarPaint\r\n\
        FaceBlock1_Material = external::tire\r\n\
        FaceBlock1_Shader = CarTexture\r\n\
        Predicate = NotUsed\r\n\
        Switch = false\r\n\
        [e_light_lf]\r\n\
        FaceBlock0_Material = external dynamic::tire\r\n\
        FaceBlock0_Shader = CarLight\r\n\
        Switch = TRUE\r\n";

    const SHADERS: &str = "[external::carpaint]\r\n\
        DiffuseChannel = true\r\n\
        [external::carpaint_diffuse_bitmap]\r\n\
        Filename = xsara_external.dds\r\n\
        [external::tire]\r\n\
        DiffuseChannel = false\r\n\
        [external transparent::carpaint]\r\n\
        OpacityChannel = true\r\n\
        [external::tire_diffuse_bitmap]\r\n\
        Filename = xsara_external.dds\r\n\
        [TextureSets]\r\n\
        TextureSet0 = TSDefault\r\n\
        [TSDefault]\r\n\
        xsara_external.dds = cars\\Xsara\\textures\\xsara_external.dds\r\n\
        xsara_external_scrape.dds = cars\\Xsara\\textures\\xsara_external_scrape.dds\r\n\
        [xsara_external.dds]\r\n\
        FriendlyName = \r\n\
        [xsara_external_scrape.dds]\r\n\
        FriendlyName = Scrape\r\n";

    fn sections(text: &str) -> Vec<Section> {
        ini::parse(text).unwrap()
    }

    #[test]
    fn reads_face_blocks_in_order() {
        let parts = sections(PART);
        let body = section(&parts, "E_BODY").unwrap();
        assert_eq!(
            face_blocks(body),
            vec![
                FaceBlock { shader: "CarPaint".to_owned(), material: "external::carpaint".to_owned() },
                FaceBlock { shader: "CarTexture".to_owned(), material: "external::tire".to_owned() },
            ]
        );
        assert!(!is_switched(body));
        assert!(is_switched(section(&parts, "e_light_lf").unwrap()));
    }

    #[test]
    fn part_without_face_blocks_has_none() {
        let parts = sections("[k_window_f]\r\nSwitch = false\r\n");
        assert!(face_blocks(&parts[0]).is_empty());
    }

    #[test]
    fn diffuse_path_follows_bitmap_and_texture_set() {
        let shaders = sections(SHADERS);
        assert_eq!(diffuse_path(&shaders, "external::carpaint"), Some("cars\\Xsara\\textures\\xsara_external.dds"));
        // The bitmap counts whatever the material's section says, as in the game's car shaders.
        assert_eq!(diffuse_path(&shaders, "external::tire"), Some("cars\\Xsara\\textures\\xsara_external.dds"));
        let only_bitmap = sections(&SHADERS.replace("[external::carpaint]\r\nDiffuseChannel = true\r\n", ""));
        assert_eq!(diffuse_path(&only_bitmap, "external::carpaint"), Some("cars\\Xsara\\textures\\xsara_external.dds"));
        assert_eq!(diffuse_path(&shaders, "external::missing"), None);
    }

    #[test]
    fn an_empty_texture_set_header_before_the_set_is_skipped() {
        let shaders = sections(&SHADERS.replace("[TSDefault]\r\n", "[TSDefault]\r\n[TSDefault]\r\n"));
        assert_eq!(diffuse_path(&shaders, "external::carpaint"), Some("cars\\Xsara\\textures\\xsara_external.dds"));
        assert_eq!(friendly_path(&shaders, "Scrape"), Some("cars\\Xsara\\textures\\xsara_external_scrape.dds"));
    }

    #[test]
    fn a_normal_map_is_the_bump_bitmaps_file() {
        let shaders =
            sections(&format!("{SHADERS}[external::carpaint_bump_bitmap]\r\nFilename = xsara_external_scrape.dds\r\n"));
        assert_eq!(bump_path(&shaders, "external::carpaint"), Some("cars\\Xsara\\textures\\xsara_external_scrape.dds"));
        // No retail car has the section, and the diffuse bitmap is not it.
        assert_eq!(bump_path(&sections(SHADERS), "external::carpaint"), None);
        assert_eq!(bump_path(&shaders, "external::tire"), None);
    }

    #[test]
    fn a_finish_is_the_materials_finish_keys() {
        let keys = "[external::tire]\r\nFinish = flock\r\nFinishFresnel = 0 1 2.5\r\nFinishEnvironment = 1 1 1 1\r\n\
            FinishPhong = 15 1 1 0.5 1\r\nFinishNoise = 10\r\n";
        let shaders = sections(&SHADERS.replace("[external::tire]\r\n", keys));
        let f = finish(&shaders, "external::tire").unwrap();
        assert_eq!(
            (f.kind, f.fresnel, f.environment),
            (FinishKind::Flock, [0.0, 1.0, 2.5], Some([1.0, 1.0, 1.0, 1.0]))
        );
        assert_eq!((f.phong, f.noise, f.carbon, f.specular), ([15.0, 1.0, 1.0, 0.5, 1.0], 10.0, [0.0, 1.0, 1.0], 1.0));
        assert_eq!(finish(&shaders, "external::carpaint"), None);
        let gated = sections(
            &SHADERS.replace("[external::tire]\r\n", "[external::tire]\r\nFinish = gated\r\nFinishSpecular = 1.8\r\n"),
        );
        assert_eq!(finish(&gated, "external::tire").map(|f| (f.kind, f.specular)), Some((FinishKind::Gated, 1.8)));
        let exterior = sections(&SHADERS.replace("[external::tire]\r\n", "[external::tire]\r\nFinish = exterior\r\n"));
        assert_eq!(finish(&exterior, "external::tire").map(|f| f.kind), Some(FinishKind::Exterior));
        // No environment key: the material reflects nothing.
        let plain = sections(&SHADERS.replace("[external::tire]\r\n", "[external::tire]\r\nFinish = plain\r\n"));
        assert_eq!(finish(&plain, "external::tire").map(|f| f.environment), Some(None));
    }

    #[test]
    fn opacity_channel_is_read_per_material() {
        let shaders = sections(SHADERS);
        assert!(opacity_channel(&shaders, "external transparent::carpaint"));
        assert!(!opacity_channel(&shaders, "external::carpaint"));
        assert!(!opacity_channel(&shaders, "missing"));
    }

    #[test]
    fn friendly_path_finds_the_named_texture_in_the_set() {
        let shaders = sections(SHADERS);
        assert_eq!(friendly_path(&shaders, "Scrape"), Some("cars\\Xsara\\textures\\xsara_external_scrape.dds"));
        assert_eq!(friendly_path(&shaders, "InternalScrape"), None);
    }
}
