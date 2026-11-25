//! A face's look: its shader class's lighting constants, dirt and scrape textures, render state, pass and draw list.

use crate::stage::{Address, DrawState, Light, NormalMap, Shade, Surface, TextureRef};
use rbr_formats::car_parts::{Finish, FinishKind};
use rbr_formats::ini::Section;

/// The lighting constants of `class` in the car's clear-weather shader settings (the overcast file is not used), or
/// `None` (unlit) when the class has no section.
pub(super) fn shade_for(settings: &[Section], class: &str) -> Option<Shade> {
    let section = settings.iter().find(|s| s.name == class)?;
    let number = |key: &str| section.get(key).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    Some(Shade {
        ka: number("Ka"),
        kd: number("Kd"),
        ks: number("CookTorranceKs"),
        m: number("CookTorranceM"),
        fresnel_min: number("FresnelMin"),
        fresnel_max: number("FresnelMax"),
        kf: number("Kf"),
        glass: class == "CarGlass",
        tint: class == "CarIntGlass",
        glow: None,
        glow_kind: 0,
        crack: false,
        dirt: None,
        grime: 0,
        spatter: false,
        window: false,
        finish: None,
    })
}

/// The friendly name of the dirt mask a car class blends in by the dirt level (`Shade::dirt`).
pub(super) fn dirt_texture(shader: &str) -> Option<&'static str> {
    match shader {
        "CarPaint" | "CarTexture" => Some("Dirt"),
        "CarTire" => Some("TireDirt"),
        _ => None,
    }
}

/// The finish a car class takes where its material has none (ours): the rim (`CarRim`) DiRT Rally's wheel
/// (`car_wheel.fx`, the constants of 105 of its 150 wheel materials: Fresnel 0.1 to 0.5, power 1, the cube untinted and
/// sharp, the metallic flake's power 10), so an RSF car's wheels reflect as a converted DiRT car's; its specular from the
/// texel's brightness (`WHEEL_SPECULAR` in `shader.wgsl`), at the gain DiRT's `sat(2 * spec)` takes.
pub(super) fn class_finish(shader: &str) -> Option<Finish> {
    (shader == "CarRim").then_some(Finish {
        kind: FinishKind::Exterior,
        fresnel: [0.1, 0.5, 1.0],
        environment: Some([1.0, 1.0, 1.0, 0.0]),
        phong: [10.0, 1.0, 1.0, 1.0, 1.0],
        noise: 10.0,
        carbon: [0.0, 1.0, 1.0],
        specular: 2.0,
    })
}

/// The friendly name of the scrape texture a car class blends in by the vertex's scrape amount: `Scrape` for
/// `CarPaint` (stage 3) and `InternalScrape` for `CarPaintInt` (stage 2). The other classes bind `Scrape` and do not
/// sample it (`notes/formats.md` "Shaders and vertex layouts").
pub(super) fn scrape_texture(shader: &str) -> Option<&'static str> {
    match shader {
        "CarPaint" => Some("Scrape"),
        "CarPaintInt" => Some("InternalScrape"),
        _ => None,
    }
}

/// The render state of a car face: lit by `shade`, depth tested, and the state `pass` gives. A `scrape` texture is
/// the second texture, blended in by its alpha and the vertex's scrape amount (`CarPaint1.pso`: `lrp` by
/// `sat(v0.w * t3.a)` towards `t3 * v0`). A `normal` map (ours, a converted car's) bends the normal the light and the
/// reflection take, as the stages' bump does, and makes the diffuse's alpha the specular strength.
pub(super) fn draw_state(
    texture: Option<TextureRef>,
    scrape: Option<TextureRef>,
    normal: Option<TextureRef>,
    pass: Pass,
    shade: Option<Shade>,
) -> DrawState {
    let blend = !matches!(pass, Pass::Opaque | Pass::TwoSided);
    DrawState {
        textures: [texture, scrape],
        shadow: None,
        address: [if pass == Pass::Crack { Address::Clamp } else { Address::Wrap }; 2],
        second_texture: scrape.is_some(),
        alpha_ref: (blend && pass != Pass::Crack).then_some(0),
        // `CShaderCarBase::Load` sets the diffuse sampler's bias for every class with a diffuse bitmap. Ours: not on a
        // converted car's faces with a normal map, whose textures are made for the plain filter (DiRT Rally's 2048 to
        // 4096 atlases, a carbon weave repeated 300 to 500 times): 8 times finer than the pixel they alias.
        mip_bias: texture.is_some() && normal.is_none(),
        ground: false,
        grass: None,
        cull_back: !matches!(pass, Pass::Plastic | Pass::InteriorGlass | Pass::TwoSided),
        blend,
        depth_test: true,
        depth_write: matches!(pass, Pass::Opaque | Pass::Plastic | Pass::TwoSided),
        shade,
        specular: None,
        surface: normal.map(|texture| Surface {
            normal: Some(NormalMap { texture, uv1: false, doubled: false, scale: [1.0; 2] }),
            highlight: None,
            sheen: None,
            light: Light::NONE,
            layers: None,
        }),
    }
}

/// How a face is drawn. Every car class's `Load` runs `CShaderCarBase::Load` (0x1007b3e0), whose
/// `SShaderMaterial::SetupRenderState` (0x1003f4b0) turns on, for a material with an opacity map (or a transparency
/// above 0, which no retail car has), alpha blending (`SRCALPHA`, `INVSRCALPHA`), the alpha test `> 0` and no z
/// write; the render state's default culls back faces (`D3DCULL_CCW`, 0x10091b50). `CarPlastic`'s `Load` (0x1007dab0)
/// then turns z write on and culling off when it is transparent, and `CarIntGlass`'s turns culling off. The class
/// does not matter otherwise: the capture's `CarTexture` draw of `internal transparent::tire` (an opacity material, in
/// the opaque list) is blended with test `> 0` and no z write, as its `CarGlass` draws are (`notes/formats.md` "Shaders and vertex layouts").
/// The pixel shaders' alpha is the diffuse texture's (`CarTextureNoDirt.pso`: `mov r0.w, t0.w`); `CarGlass` adds a
/// Fresnel term, which is not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    Opaque,
    /// Blended, alpha test `> 0`, no z write, back faces culled.
    Blended,
    /// `CarPlastic` with an opacity map: blended, alpha test `> 0`, z write, no culling.
    Plastic,
    /// `CarIntGlass` with an opacity map: blended, alpha test `> 0`, no z write, no culling.
    InteriorGlass,
    /// `CarIntGlass` without one: opaque, no culling (the Porsche 911 GT3 RS (2010) RGT pack's inner door panels).
    TwoSided,
    /// A window's crack overlay (`m_damageRes`, `InitDamage`): blended, no alpha test, back faces culled, clamped uvs.
    /// The game writes z (with `LESSEQUAL`); the overlay does not, so the second half's overlay on the same pane is
    /// not hidden by the first under the renderer's `Less`.
    Crack,
}

/// The pass of a face with shader class `shader`, whose material has an opacity map when `opacity`.
pub(super) fn pass_for(shader: &str, opacity: bool) -> Pass {
    match (shader, opacity) {
        ("CarIntGlass", true) => Pass::InteriorGlass,
        ("CarIntGlass", false) => Pass::TwoSided,
        ("CarPlastic", true) => Pass::Plastic,
        (_, true) => Pass::Blended,
        (_, false) => Pass::Opaque,
    }
}

/// The car list a face's shader is drawn from (`FUN_1001e430`, rbr-recomp `notes/formats/sgc.md`, section 9). The world render
/// (0x1006ada0) draws the wheel list (`CarRim`, `CarTire`) and then `CHH_Car::Render`, which draws the opaque list
/// and then the transparent one. A shader goes to the transparent list when its class's `IsTransparent` is true:
/// `CarIntGlass` always, `CarGlass`, `CarPlastic`, `CarRim` and `CarLight` with an opacity map; the other classes
/// return false, so a blended `CarTextureNoDirt` face (the `pc_e_window_*` panes) is drawn in the opaque list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum List {
    Wheels,
    Opaque,
    Transparent,
}

pub(super) fn list_for(shader: &str, opacity: bool) -> List {
    match shader {
        "CarRim" | "CarTire" => List::Wheels,
        "CarIntGlass" => List::Transparent,
        "CarGlass" | "CarPlastic" | "CarLight" if opacity => List::Transparent,
        _ => List::Opaque,
    }
}

/// When a face is drawn: its list, then its shader's rank in the order the loader created the shaders. Within a
/// shader the parts keep the model's order.
pub(super) type Order = (List, usize);

/// The diffuse, scrape and normal textures, the pass and the draw order of one face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FaceStyle {
    pub(super) texture: Option<TextureRef>,
    pub(super) scrape: Option<TextureRef>,
    pub(super) normal: Option<TextureRef>,
    pub(super) shade: Option<Shade>,
    pub(super) pass: Pass,
    pub(super) order: Order,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_rim_class_takes_a_finish() {
        let rim = class_finish("CarRim").unwrap();
        assert_eq!((rim.kind, rim.fresnel, rim.specular), (FinishKind::Exterior, [0.1, 0.5, 1.0], 2.0));
        assert!(rim.environment.is_some());
        for class in ["CarPaint", "CarTexture", "CarTire", "CarPlastic", "CarGlass"] {
            assert_eq!(class_finish(class), None);
        }
    }
}
