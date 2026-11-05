//! The frame's depth pyramid for the stochastic screen-space reflections (`viewer --ssr stochastic`,
//! `depth_pyramid.wgsl`): the frame's depth, then each level the nearest depth of the 2x2 texels below it, which the
//! rays traverse coarse to fine (`ssr.wgsl`).

/// The pyramid's levels past the frame's depth: at 4K the coarsest cell is 128 pixels.
pub(super) const LEVELS: u32 = 8;
/// The depth shader.
pub(crate) const SHADER: &str = include_str!("../depth_pyramid.wgsl");

/// A level's bind group and size.
type Level = (wgpu::BindGroup, (u32, u32));

pub(super) struct DepthPyramid {
    copy: wgpu::ComputePipeline,
    halve: wgpu::ComputePipeline,
    /// All the levels, for the trace.
    pub(super) view: wgpu::TextureView,
    /// Each level's bind group and size: level 0 from the frame's depth, the others from the level below.
    groups: Vec<Level>,
}

impl DepthPyramid {
    pub(super) fn new(device: &wgpu::Device, size: (u32, u32), depth: &wgpu::TextureView) -> DepthPyramid {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth pyramid"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let [copy, halve] = ["cs_copy", "cs_halve"].map(|entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("depth pyramid"),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let (view, groups) = Self::targets(device, [&copy, &halve], size, depth);
        DepthPyramid { copy, halve, view, groups }
    }

    pub(super) fn resize(&mut self, device: &wgpu::Device, size: (u32, u32), depth: &wgpu::TextureView) {
        (self.view, self.groups) = Self::targets(device, [&self.copy, &self.halve], size, depth);
    }

    /// The pyramid for a frame of `size`, padded to a multiple of its coarsest cell so that every level halves exactly.
    fn targets(
        device: &wgpu::Device,
        [copy, halve]: [&wgpu::ComputePipeline; 2],
        (width, height): (u32, u32),
        depth: &wgpu::TextureView,
    ) -> (wgpu::TextureView, Vec<Level>) {
        let cell = 1 << LEVELS;
        let (width, height) = (width.max(1).next_multiple_of(cell), height.max(1).next_multiple_of(cell));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth pyramid"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: LEVELS + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let level = |mip| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let texture_entry =
            |binding, view| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) };
        let levels: Vec<_> = (0..=LEVELS).map(level).collect();
        let groups = (0..=LEVELS)
            .map(|mip| {
                let at = mip as usize;
                let (pipeline, entries) = match mip {
                    0 => (copy, [texture_entry(1, &levels[0]), texture_entry(2, depth)]),
                    _ => (halve, [texture_entry(0, &levels[at - 1]), texture_entry(1, &levels[at])]),
                };
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("depth pyramid"),
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
                (group, (width >> mip, height >> mip))
            })
            .collect();
        (texture.create_view(&Default::default()), groups)
    }

    /// Fills the pyramid from the frame's depth as it now is.
    pub(super) fn build(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("depth pyramid"), timestamp_writes: None });
        for (mip, (group, (width, height))) in self.groups.iter().enumerate() {
            pass.set_pipeline(if mip == 0 { &self.copy } else { &self.halve });
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
    }
}
