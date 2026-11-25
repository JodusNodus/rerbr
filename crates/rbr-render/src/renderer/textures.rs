//! The draws' textures, samplers and bind groups, and the colour and depth targets.

use super::pipelines::cutout;
use super::uniforms::ModeUniform;
use super::DEPTH_FORMAT;
use crate::stage::{Address, DrawState, Highlight, Layers, Shade, Sheen, TextureRef, TextureSet, MIP_MAP_BIAS};
use bytemuck::bytes_of;
use rbr_formats::dds::{self, Format};
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

/// Texel used for a draw with no texture (vertex colour only).
pub(super) const WHITE: [u8; 4] = [255, 255, 255, 255];
/// Texel used for a missing texture that would otherwise cover what it should not: a cut-out or blended draw's (not
/// drawn), a second texture (adds nothing).
pub(super) const CLEAR: [u8; 4] = [255, 255, 255, 0];

/// Which view a texture slot samples: a loaded texture, the white placeholder (no texture, or a missing one: the draw
/// is plain, its vertex colour), or the clear one (a missing texture of a cut-out or blended draw: its cards would be
/// solid rectangles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Source {
    White,
    Clear,
    Texture(TextureRef),
}

/// The texture bindings and shader mode of a draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct BindKey {
    textures: [Source; 2],
    /// The shadow light map (`White` when the draw has none or it is missing: no shadow).
    shadow: Source,
    address: [Address; 2],
    second_texture: bool,
    pub(super) alpha_ref: Option<u8>,
    mip_bias: bool,
    /// The near ground ([`DrawState::ground`]): its terrain detail.
    ground: bool,
    pub(super) cutout: bool,
    pub(super) shade: Option<Shade>,
    /// The water stand-in's draws, which the shader draws as water.
    pub(super) water: bool,
    /// The specular map (`White` when the draw has none or it is missing: no highlight), or a lit draw's dirt map
    /// ([`Shade::dirt`]) in the same binding.
    pub(super) specular: Source,
    /// The first texture has an emissive mask ([`Views::glows`]).
    glow: bool,
    /// A BTB normal map ([`crate::stage::Surface::normal`]; `White` for none or a missing one) and whether it reads `uv1`.
    normal: Source,
    normal_uv1: bool,
    normal_doubled: bool,
    /// The normal map's uv scale's bits ([`crate::stage::NormalMap::scale`]).
    normal_scale: [u32; 2],
    /// A BTB highlight ([`crate::stage::Surface::highlight`]), its map in the specular binding.
    highlight: Option<Highlight>,
    /// The material's light factors' bits ([`crate::stage::Surface::light`]).
    light: [u32; 2],
    /// A converted stage's grass ground ([`DrawState::grass`]): its green texels drawn warmer.
    grass: bool,
    /// A BTB road's dry sheen ([`crate::stage::Surface::sheen`]).
    sheen: Option<Sheen>,
    /// A multilayer material summed per pixel ([`crate::stage::Surface::layers`]) with its diffuse's, mask's and details' textures;
    /// `None` when any of them did not load.
    layers: Option<(Layers, [Source; 6])>,
}

impl BindKey {
    /// What the draw's mode uniform says it is ([`ModeUniform`]): the water stand-in, a draw with a specular map (the
    /// roads' shine and wet film), a second texture, the near ground (its fine detail).
    pub(super) fn features(&self) -> DrawFeatures {
        DrawFeatures {
            water: self.water,
            specular: self.specular != Source::White && self.shade.is_none() && self.highlight.is_none(),
            second_texture: self.second_texture,
            detail: self.ground,
            bump: self.normal != Source::White,
            highlight: self.highlight.is_some(),
            sheen: self.sheen.is_some(),
            layers: self.layers.is_some(),
        }
    }
}

/// The parts of the stage shader a draw runs, by its mode ([`BindKey::features`]; `KNOWN` and `DRAW_*` in
/// `shader.wgsl`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct DrawFeatures {
    pub(super) water: bool,
    pub(super) specular: bool,
    pub(super) second_texture: bool,
    pub(super) detail: bool,
    pub(super) bump: bool,
    pub(super) highlight: bool,
    pub(super) sheen: bool,
    pub(super) layers: bool,
}

/// The nine samplers: one per (U mode, V mode) pair.
pub(super) struct Samplers([[wgpu::Sampler; 3]; 3]);

impl Samplers {
    pub(super) fn new(device: &wgpu::Device, anisotropy: u16) -> Samplers {
        let sampler = |u: Address, v: Address| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("stage"),
                address_mode_u: address_mode(u),
                address_mode_v: address_mode(v),
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                anisotropy_clamp: anisotropy,
                ..Default::default()
            })
        };
        let row = |u| [sampler(u, Address::Wrap), sampler(u, Address::Mirror), sampler(u, Address::Clamp)];
        Samplers([row(Address::Wrap), row(Address::Mirror), row(Address::Clamp)])
    }

    fn get(&self, u: Address, v: Address) -> &wgpu::Sampler {
        &self.0[address_index(u)][address_index(v)]
    }
}

fn address_index(address: Address) -> usize {
    match address {
        Address::Wrap => 0,
        Address::Mirror => 1,
        Address::Clamp => 2,
    }
}

fn address_mode(address: Address) -> wgpu::AddressMode {
    match address {
        Address::Wrap => wgpu::AddressMode::Repeat,
        Address::Mirror => wgpu::AddressMode::MirrorRepeat,
        Address::Clamp => wgpu::AddressMode::ClampToEdge,
    }
}

/// The texture bindings of a draw. A first texture that did not load draws plain (white), or not at all in a cut-out
/// or blended draw (clear); a second one adds nothing (clear: it blends in by its alpha).
pub(super) fn bind_key(state: &DrawState, views: &Views) -> BindKey {
    let loaded = &views.loaded;
    let source = |texture: Option<TextureRef>, missing: Source| match texture {
        None => Source::White,
        Some(texture) if loaded.contains_key(&texture) => Source::Texture(texture),
        Some(_) => missing,
    };
    let first = if state.alpha_ref.is_some() || state.blend { Source::Clear } else { Source::White };
    let (normal, highlight, sheen) = state.surface.map_or((None, None, None), |s| (s.normal, s.highlight, s.sheen));
    // A missing light map means no shadow.
    let shadow = match state.shadow {
        Some(texture) if loaded.contains_key(&texture) => Source::Texture(texture),
        _ => Source::White,
    };
    BindKey {
        textures: [source(state.textures[0], first), source(state.textures[1], Source::Clear)],
        shadow,
        address: state.address,
        second_texture: state.second_texture,
        alpha_ref: state.alpha_ref,
        mip_bias: state.mip_bias,
        ground: state.ground,
        cutout: cutout(state),
        shade: state.shade,
        water: state.textures[0].is_some_and(|t| t.set == TextureSet::WaterStandIn),
        specular: match state.specular.or(state.shade.and_then(|s| s.dirt)).or(highlight.and_then(|h| h.map)) {
            Some(texture) if loaded.contains_key(&texture) => Source::Texture(texture),
            _ => Source::White,
        },
        glow: state.textures[0].is_some_and(|t| views.glows.contains_key(&t)),
        normal: source(normal.map(|n| n.texture), Source::White),
        normal_uv1: normal.is_some_and(|n| n.uv1),
        normal_doubled: normal.is_some_and(|n| n.doubled),
        normal_scale: normal.map_or([1.0f32; 2], |n| n.scale).map(f32::to_bits),
        highlight,
        light: state.surface.map_or([1.0f32; 2], |s| s.light.0).map(f32::to_bits),
        grass: state.grass.is_some(),
        sheen,
        layers: state.surface.and_then(|s| s.layers).and_then(|layers| {
            let found = |texture| loaded.contains_key(&texture).then_some(Source::Texture(texture));
            let [r, g, b, a] = layers.details.map(found);
            Some((layers, [found(layers.diffuse)?, found(layers.mask)?, r?, g?, b?, a?]))
        }),
    }
}

/// An emissive mask.
pub(super) struct Glow {
    pub(super) view: wgpu::TextureView,
    /// Its alpha counts (not DXT1).
    pub(super) alpha: bool,
    pub(super) kind: GlowKind,
}

/// What lights an emissive mask (`backlit` in `shader.wgsl`, `mode.glow.z`): always on, at its own luminance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GlowKind {
    /// A dial's backlight (`DIAL_LUMINANCE`): a needle gauge's, a car texture's own mask, the start board's digits.
    Dial = 0,
    /// The start board's lit lamps (`SIGNAL_LUMINANCE`).
    Signal = 1,
    /// A gauge with no needle, digits, bars and lamps (`DISPLAY_LUMINANCE`).
    Display = 2,
}

/// The texture views a draw can sample: the loaded stage textures and the two placeholders.
pub(super) struct Views {
    pub(super) loaded: BTreeMap<TextureRef, wgpu::TextureView>,
    pub(super) white: wgpu::TextureView,
    pub(super) clear: wgpu::TextureView,
    /// The emissive masks, by the texture they light (a dial's `Image::Dial`, the start board's faces).
    pub(super) glows: BTreeMap<TextureRef, Glow>,
    /// The near ground's detail windows ([`crate::terrain_detail::window`]).
    pub(super) detail: BTreeMap<TextureRef, [f32; 4]>,
    /// The near ground textures' bank shares ([`crate::terrain_detail::bank_shares`]).
    pub(super) banks: BTreeMap<TextureRef, f32>,
}

impl Views {
    fn get(&self, source: Source) -> &wgpu::TextureView {
        match source {
            Source::White => &self.white,
            Source::Clear => &self.clear,
            Source::Texture(texture) => self.loaded.get(&texture).unwrap_or(&self.white),
        }
    }
}

/// A draw's lit class (`mode.lit`, `LIT` in `shader.wgsl`): 0 unlit, 1 lit, 2 glass, 3 a lit lamp's part, 4 a crack.
pub(super) fn lit_class(shade: Option<&Shade>) -> u32 {
    shade.map_or(0, |s| match (s.glow.is_some(), s.crack) {
        (true, _) => 3,
        (_, true) => 4,
        _ => 1 + u32::from(s.glass),
    })
}

/// A bind group for one set of texture bindings and shader mode.
pub(super) fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    key: BindKey,
    views: &Views,
    samplers: &Samplers,
) -> wgpu::BindGroup {
    let glow = match key.textures[0] {
        Source::Texture(texture) if key.glow => views.glows.get(&texture),
        _ => None,
    };
    let features = key.features();
    let mode = ModeUniform {
        second_texture: u32::from(features.second_texture),
        alpha_test: u32::from(key.alpha_ref.is_some()),
        alpha_ref: key.alpha_ref.map_or(0.0, |r| f32::from(r) / 255.0),
        mip_bias: if key.mip_bias { MIP_MAP_BIAS } else { 0.0 },
        cutout: u32::from(key.cutout),
        lit: lit_class(key.shade.as_ref()),
        water: u32::from(features.water),
        specular: u32::from(features.specular),
        light: key.shade.map_or([0.0; 4], |s| [s.ka, s.kd, s.ks, s.m]),
        // `w` 1: the dirt map is in the specular binding.
        fresnel: key.shade.map_or([0.0; 4], |s| {
            [s.fresnel_min, s.fresnel_max, s.kf, f32::from(u8::from(key.specular != Source::White))]
        }),
        glow: [
            f32::from(u8::from(glow.is_some())),
            f32::from(u8::from(glow.is_some_and(|g| g.alpha))),
            glow.map_or(0.0, |g| f32::from(g.kind as u8)),
            key.shade.map_or(0.0, |s| f32::from(s.glow_kind)),
        ],
        lamp: key.shade.and_then(|s| s.glow).unwrap_or_default(),
        grime: key.shade.map_or([0.0; 4], |s| {
            [
                f32::from(s.grime) / 10.0,
                f32::from(u8::from(s.tint)),
                f32::from(u8::from(s.spatter)),
                f32::from(u8::from(s.window)),
            ]
        }),
        detail: match key.textures[0] {
            Source::Texture(texture) if key.ground => views.detail.get(&texture).copied().unwrap_or_default(),
            _ => [0.0; 4],
        },
        bank: {
            let [a, b] = key.textures.map(|source| match source {
                Source::Texture(texture) if key.ground => views.banks.get(&texture).copied().unwrap_or_default(),
                _ => 0.0,
            });
            [a, b, f32::from(u8::from(key.ground)), 0.0]
        },
        bump: [
            if key.normal == Source::White { 0.0 } else { 1.0 + f32::from(u8::from(key.normal_uv1)) },
            if key.normal_doubled { 2.0 } else { 1.0 },
            f32::from_bits(key.normal_scale[0]),
            f32::from_bits(key.normal_scale[1]),
        ],
        highlight: key.highlight.map_or([0.0; 4], |h| [h.colour[0], h.colour[1], h.colour[2], h.power]),
        tint: [f32::from_bits(key.light[0]), f32::from_bits(key.light[1]), f32::from(u8::from(key.grass)), 0.0],
        sheen: {
            let finish = key.shade.and_then(|s| s.finish).map_or([0.0; 3], |f| [f.roughness, f.metal, f.flock]);
            [key.sheen.map_or(0.0, |s| s.roughness), finish[0], finish[1], finish[2]]
        },
        layer: key.layers.map_or([0.0; 4], |(l, _)| {
            [
                if l.uv {
                    3.0
                } else if l.world {
                    1.0
                } else {
                    2.0
                },
                l.magic,
                f32::from(u8::from(l.blend.is_some())),
                l.blend.unwrap_or(0.0),
            ]
        }),
        layer_scale: layer_pairs(key.layers.map(|(l, _)| l.scale)),
        layer_offset: layer_pairs(key.layers.map(|(l, _)| l.offset)),
    };
    // The diffuse, the mask and the four details, or the white texel for a draw without layers.
    let layer_views = key.layers.map_or([&views.white; 6], |(_, sources)| sources.map(|source| views.get(source)));
    let glow = glow.map_or(&views.white, |g| &g.view);
    let mode_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("draw mode"),
        contents: bytes_of(&mode),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("draw textures"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(views.get(key.textures[0])),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(views.get(key.textures[1])),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(samplers.get(key.address[0], key.address[1])),
            },
            wgpu::BindGroupEntry { binding: 3, resource: mode_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(views.get(key.shadow)) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(views.get(key.specular)) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(glow) },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(views.get(key.normal)) },
            wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(layer_views[1]) },
            wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(layer_views[2]) },
            wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(layer_views[3]) },
            wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::TextureView(layer_views[4]) },
            wgpu::BindGroupEntry { binding: 12, resource: wgpu::BindingResource::TextureView(layer_views[5]) },
            wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::TextureView(layer_views[0]) },
        ],
    })
}

/// Four `(u, v)` pairs as the two vectors the mode uniform holds (zero for none).
fn layer_pairs(pairs: Option<[[f32; 2]; 4]>) -> [[f32; 4]; 2] {
    let [a, b, c, d] = pairs.unwrap_or_default();
    [[a[0], a[1], b[0], b[1]], [c[0], c[1], d[0], d[1]]]
}

/// A 1x1 texture of one colour.
pub(super) fn solid_texture(device: &wgpu::Device, queue: &wgpu::Queue, texel: [u8; 4]) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("solid"),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &texel,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
        wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
    );
    texture
}

/// A single-channel light map (`R8Unorm`): one level of `width * height` texels.
pub(super) fn luminance_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: Some(height) },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    texture
}

/// A texture with its whole mip chain, uploaded level by level, from the first level the device can hold (a GPU
/// under 484's 16384-texel backdrop takes it from its next level). A block-compressed texture whose size is not
/// whole blocks (a 2x2 one on a community stage), which the GPU cannot hold compressed, is decoded to one RGBA level.
pub(crate) fn dds_texture(device: &wgpu::Device, queue: &wgpu::Queue, dds: &dds::Dds) -> wgpu::Texture {
    let edge = dds.format.block_edge();
    if !dds.width.is_multiple_of(edge) || !dds.height.is_multiple_of(edge) {
        let (width, height) = (dds.width as usize, dds.height as usize);
        let bc = match dds.format {
            Format::Dxt1 => Some(texpresso::Format::Bc1),
            Format::Dxt3 => Some(texpresso::Format::Bc2),
            Format::Dxt5 => Some(texpresso::Format::Bc3),
            Format::Rgba8 => unreachable!("plain texels are whole blocks"),
            Format::Bc7 => None,
        };
        let mut rgba = vec![128u8; width * height * 4];
        match bc {
            Some(bc) => bc.decompress(dds.levels()[0].data, width, height, &mut rgba),
            None => eprintln!("dds: a {}x{} BC7 texture is not whole blocks, drawn flat", dds.width, dds.height),
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dds rgba"),
            size: wgpu::Extent3d { width: dds.width, height: dds.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * dds.width),
                rows_per_image: Some(dds.height),
            },
            wgpu::Extent3d { width: dds.width, height: dds.height, depth_or_array_layers: 1 },
        );
        return texture;
    }
    let format = match dds.format {
        Format::Dxt1 => wgpu::TextureFormat::Bc1RgbaUnorm,
        Format::Dxt3 => wgpu::TextureFormat::Bc2RgbaUnorm,
        Format::Dxt5 => wgpu::TextureFormat::Bc3RgbaUnorm,
        Format::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
        Format::Bc7 => wgpu::TextureFormat::Bc7RgbaUnorm,
    };
    let levels = dds.levels();
    let most = device.limits().max_texture_dimension_2d;
    let fits =
        |l: &dds::Level| l.width.max(l.height) <= most && l.width.is_multiple_of(edge) && l.height.is_multiple_of(edge);
    let first = levels.iter().position(fits).unwrap_or(0);
    let levels = &levels[first..];
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dds"),
        size: wgpu::Extent3d { width: levels[0].width, height: levels[0].height, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip_level, level) in levels.iter().enumerate() {
        // Copies cover whole blocks: a level under 4 texels is still one block across.
        let blocks_x = level.width.div_ceil(edge);
        let blocks_y = level.height.div_ceil(edge);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip_level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            level.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(blocks_x * dds.format.block_len() as u32),
                rows_per_image: Some(blocks_y),
            },
            // Block formats copy whole blocks: a mip smaller than 4 texels still takes one block.
            wgpu::Extent3d { width: blocks_x * edge, height: blocks_y * edge, depth_or_array_layers: 1 },
        );
    }
    texture
}

pub(super) fn depth_view(device: &wgpu::Device, (width, height): (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            // Read by the reflections, the sun's mask, the effects and the post pass.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
