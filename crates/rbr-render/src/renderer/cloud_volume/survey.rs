//! Offline full-sky cloud coverage measurement, using the production noise, weather and volume shader. Each pixel
//! covers the same solid angle; RGB are low-cloud, cirrus and combined opacity. No game files are needed.

use super::{layer, noise, texture, Params, DETAIL_SIZE, SHAPE_SIZE, WEATHER_FORMAT, WEATHER_REACH, WEATHER_SIZE};
use crate::atmosphere::Atmosphere;
use crate::renderer::uniforms::FrameUniform;
use crate::renderer::{Gpu, ReadbackError, SHADER};
use bytemuck::Zeroable;
use glam::{Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

/// A reusable GPU survey of the entire upper hemisphere. See `examples/cloud_survey.rs`.
pub struct CloudSurvey {
    gpu: Gpu,
    weather: wgpu::RenderPipeline,
    march: wgpu::RenderPipeline,
    weather_groups: [wgpu::BindGroup; 3],
    march_groups: [wgpu::BindGroup; 3],
    frame: wgpu::Buffer,
    params: wgpu::Buffer,
    weather_view: wgpu::TextureView,
}

impl CloudSurvey {
    /// Builds only the clouds' resources and two diagnostic pipelines. This does not construct a stage renderer.
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cloud survey"),
            source: wgpu::ShaderSource::Wgsl(format!("{SHADER}\n{}", include_str!("survey.wgsl")).into()),
        });
        let pipeline = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_sky"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let weather = pipeline("fs_cloud_weather", WEATHER_FORMAT);
        let march = pipeline("fs_cloud_survey", wgpu::TextureFormat::Rgba32Float);
        let frame = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cloud survey frame"),
            contents: bytemuck::bytes_of(&FrameUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cloud survey parameters"),
            contents: bytemuck::bytes_of(&Params::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shape = noise(device, &gpu.queue, SHAPE_SIZE, 0);
        let detail = noise(device, &gpu.queue, DETAIL_SIZE, 1);
        let weather_view = texture(device, WEATHER_FORMAT, (WEATHER_SIZE, WEATHER_SIZE));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = |pipeline: &wgpu::RenderPipeline, index, entries: &[wgpu::BindGroupEntry<'_>]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cloud survey"),
                layout: &pipeline.get_bind_group_layout(index),
                entries,
            })
        };
        fn buffer(binding: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource: b.as_entire_binding() }
        }
        fn view(binding: u32, v: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(v) }
        }
        let weather_groups = [
            group(&weather, 0, &[buffer(0, &frame)]),
            group(&weather, 1, &[]),
            group(&weather, 2, &[buffer(9, &params)]),
        ];
        let march_groups = [
            group(
                &march,
                0,
                &[
                    buffer(0, &frame),
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&sampler) },
                    // Neutral lighting is immaterial to alpha. The shape volume supplies the unused aerial colour.
                    view(8, &shape),
                ],
            ),
            group(&march, 1, &[]),
            group(
                &march,
                2,
                &[
                    view(5, &shape),
                    view(6, &detail),
                    view(7, &weather_view),
                    buffer(9, &params),
                    wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            ),
        ];
        Self { gpu: gpu.clone(), weather, march, weather_groups, march_groups, frame, params, weather_view }
    }

    /// Measures a fixed field of `atmosphere` at `eye` and `time`. `control` overrides only the weather input, for
    /// fitting the preset mapping. `width` sets the sample count, width * width/2. Returned RGB are opacity by layer.
    pub fn measure(
        &self,
        atmosphere: &Atmosphere,
        eye: Vec3,
        time: f32,
        control: Option<f32>,
        width: u32,
    ) -> Result<Vec<[f32; 4]>, ReadbackError> {
        assert!(width >= 16 && width.is_multiple_of(16), "survey width must be a positive multiple of 16");
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let frame =
            FrameUniform::cloud_survey(atmosphere, eye, time, control.unwrap_or_else(|| atmosphere.cloud_layer()));
        queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(&frame));
        let texel = WEATHER_REACH / WEATHER_SIZE as f32;
        let corner = ((Vec2::new(eye.x, eye.z) - WEATHER_REACH * 0.5) / texel).floor() * texel;
        let (layer, shape) = layer(atmosphere);
        let params = Params {
            previous: Mat4::IDENTITY.to_cols_array_2d(),
            state: [0.0; 4],
            weather: [corner.x, corner.y, WEATHER_REACH, 0.0],
            layer,
            shape,
            shadow: [0.0; 4],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        let height = width / 2;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cloud survey opacity"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cloud survey readback"),
            size: u64::from(width) * u64::from(height) * 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        for (pipeline, groups, view) in
            [(&self.weather, &self.weather_groups, &self.weather_view), (&self.march, &self.march_groups, &target_view)]
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cloud survey"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            for (i, group) in groups.iter().enumerate() {
                pass.set_bind_group(i as u32, group, &[]);
            }
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 16),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(ReadbackError::Poll)?;
        receiver.recv().expect("map callback runs before poll returns").map_err(ReadbackError::Map)?;
        let mapped = readback.slice(..).get_mapped_range().expect("mapped above");
        let samples = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        readback.unmap();
        Ok(samples)
    }
}
