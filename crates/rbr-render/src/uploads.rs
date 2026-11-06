//! The frame's large buffer writes (the car's moving parts, the grass, the windscreen's drops, the particles, the
//! marks, the motion, the dials), through staging memory kept from frame to frame ([`wgpu::util::StagingBelt`]):
//! `Queue::write_buffer` makes a staging buffer for each write and fills fresh pages. They are made before the frame's
//! first reads, which [`crate::renderer::Renderer::render`] submits them ahead of; the writes after (the views' uniforms)
//! stay on the queue.

/// The staging chunks' size: the frame's largest write (the windscreen's drops, 1.8 MB in rain) fits one.
const CHUNK: u64 = 4 << 20;

/// Writes copied into their buffers by the commands [`Uploads::take`] gives.
pub(crate) struct Uploads {
    device: wgpu::Device,
    belt: wgpu::util::StagingBelt,
    encoder: Option<wgpu::CommandEncoder>,
}

impl Uploads {
    pub(crate) fn new(device: &wgpu::Device) -> Uploads {
        Uploads { device: device.clone(), belt: wgpu::util::StagingBelt::new(device.clone(), CHUNK), encoder: None }
    }

    /// Writes `data` into `buffer` at `offset`, both multiples of 4 bytes, as `Queue::write_buffer` does.
    pub(crate) fn write(&mut self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        let Some(size) = wgpu::BufferSize::new(data.len() as u64) else { return };
        let encoder = (self.encoder).get_or_insert_with(|| {
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("uploads") })
        });
        self.belt.write_buffer(encoder, buffer, offset, size).copy_from_slice(data);
    }

    /// Copies all of `source`, filled elsewhere, into `buffer` at `offset`, with the writes.
    pub(crate) fn copy(&mut self, source: &wgpu::Buffer, buffer: &wgpu::Buffer, offset: u64) {
        let encoder = (self.encoder).get_or_insert_with(|| {
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("uploads") })
        });
        encoder.copy_buffer_to_buffer(source, 0, buffer, offset, source.size());
    }

    /// The copies of the writes since the last call, to submit before the commands that read them.
    pub(crate) fn take(&mut self) -> Option<wgpu::CommandBuffer> {
        let encoder = self.encoder.take()?;
        self.belt.finish_and_recall_on_submit(&encoder);
        Some(encoder.finish())
    }
}
