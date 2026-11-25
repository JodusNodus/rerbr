//! Shorthands for the wgpu objects the passes make alike (ours): full-screen pipelines and passes, 2-D targets, the
//! linear clamped sampler, uniform buffers, and bind groups and their layouts numbered in order. Each makes what the
//! long form did; a pass that differs (depth, blending per target, vertex buffers) keeps the long form.

/// A uniform buffer binding, for [`texture_entry`](crate::renderer::texture_entry) and [`layout`].
pub(crate) const UNIFORM: wgpu::BindingType = wgpu::BindingType::Buffer {
    ty: wgpu::BufferBindingType::Uniform,
    has_dynamic_offset: false,
    min_binding_size: None,
};
/// A read-only storage buffer binding.
pub(crate) const STORAGE: wgpu::BindingType = wgpu::BindingType::Buffer {
    ty: wgpu::BufferBindingType::Storage { read_only: true },
    has_dynamic_offset: false,
    min_binding_size: None,
};
/// A filtering sampler binding.
pub(crate) const FILTERING: wgpu::BindingType = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);

/// A bind group layout of `types`, binding `i` the `i`th, all read by the fragment stage.
pub(crate) fn layout(device: &wgpu::Device, label: &str, types: &[wgpu::BindingType]) -> wgpu::BindGroupLayout {
    let entries: Vec<_> =
        types.iter().enumerate().map(|(i, &ty)| crate::renderer::texture_entry(i as u32, ty)).collect();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries })
}

/// A bind group of `resources` for `layout`, binding `i` the `i`th.
pub(crate) fn bind_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    resources: &[wgpu::BindingResource],
) -> wgpu::BindGroup {
    let entries: Vec<_> = resources
        .iter()
        .enumerate()
        .map(|(i, resource)| wgpu::BindGroupEntry { binding: i as u32, resource: resource.clone() })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries })
}

/// A pipeline drawing a full-screen triangle (no vertex buffers, no depth): `module`'s `vertex` and `fragment` entry
/// points over `layouts`, into `targets`, with the fragment stage's override `constants`.
pub(crate) fn full_screen_pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    (vertex, fragment): (&str, &str),
    layouts: &[&wgpu::BindGroupLayout],
    targets: &[Option<wgpu::ColorTargetState>],
    constants: &[(&str, f64)],
) -> wgpu::RenderPipeline {
    let layouts: Vec<_> = layouts.iter().copied().map(Some).collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &layouts,
            immediate_size: 0,
        })),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: wgpu::PipelineCompilationOptions { constants, ..Default::default() },
            targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// A WGSL shader module of `source`.
pub(crate) fn shader(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

/// A 2-D colour target of `size` (at least 1 by 1), one mip level and sample, in `format` for `usage`.
pub(crate) fn target(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// The sampler the full-screen passes read their inputs with: linear, clamped to the edge.
pub(crate) fn linear_clamp(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

/// A uniform buffer of `size` bytes the queue writes.
pub(crate) fn uniform_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A pass drawing into `view` alone, first loading it with `load`.
pub(crate) fn colour_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    })
}

/// Draws `pipeline`'s full-screen triangle with `groups` (group `i` the `i`th) into `view`, first loading it with
/// `load`.
pub(crate) fn full_screen_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    groups: &[&wgpu::BindGroup],
) {
    let mut pass = colour_pass(encoder, label, view, load);
    pass.set_pipeline(pipeline);
    for (index, group) in groups.iter().enumerate() {
        pass.set_bind_group(index as u32, *group, &[]);
    }
    pass.draw(0..3, 0..1);
}

/// Clears to black ([`full_screen_pass`], [`colour_pass`]).
pub(crate) const CLEAR: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
