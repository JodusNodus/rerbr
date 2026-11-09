//! A frame of several views from one eye (ours: a triple screen's, `crate::triple`). Each view keeps what carries
//! from frame to frame (the temporal anti-aliasing's history, the motion's last view, the clouds' march), parked while
//! another is drawn; the frame's first view draws what they share (the sun's maps, the reflection cubes, the mirrors,
//! the grass about the eye). Like-sized views share the rest of the targets: each view's commands are submitted before
//! the next is drawn ([`Renderer::render_view`]).

use super::cloud_volume::CloudHistory;
use super::Renderer;
use glam::Mat4;

/// One of a frame's views for [`Renderer::render_view`].
#[derive(Debug, Clone, Copy)]
pub struct FrameView<'a> {
    /// Every view of the frame, each its view-projection and the sky's (the camera-anchored meshes').
    pub views: &'a [(Mat4, Mat4)],
    /// The view drawn, of `views`; each keeps its own history.
    pub index: usize,
    /// Whether it is the frame's first view drawn, which draws what the views share for all of them.
    pub first: bool,
    /// Where the view goes in the target, its top left corner in pixels (a spanned display's third), keeping the rest;
    /// `None`: it fills the target.
    pub at: Option<(u32, u32)>,
}

impl FrameView<'_> {
    /// The view drawn: its view-projection and the sky's.
    pub fn matrices(&self) -> (Mat4, Mat4) {
        self.views[self.index]
    }

    /// Every view's view-projection.
    pub(super) fn view_projections(&self) -> Vec<Mat4> {
        self.views.iter().map(|&(view_projection, _)| view_projection).collect()
    }
}

/// What a view keeps while another is drawn.
pub(super) struct Parked {
    taa: Option<crate::taa::Taa>,
    motion: Option<(Mat4, Mat4)>,
    clouds: Option<CloudHistory>,
}

impl Renderer {
    /// Makes view `index` the drawn one: parks the drawn view's history and takes `index`'s (fresh the first time).
    pub(super) fn switch_view(&mut self, index: usize) {
        if index == self.view {
            return;
        }
        if self.parked.len() <= index.max(self.view) {
            self.parked.resize_with(index.max(self.view) + 1, || None);
        }
        let incoming = match self.parked[index].take() {
            Some(parked) => parked,
            None => Parked { taa: self.fresh_taa(), motion: None, clouds: None },
        };
        let size = (&self.depth_view, self.depth_size);
        let outgoing = Parked {
            taa: std::mem::replace(&mut self.taa, incoming.taa),
            motion: self.motion.as_mut().and_then(|motion| motion.exchange(incoming.motion)),
            clouds: self.cloud_volume.exchange(&self.device, incoming.clouds, size),
        };
        self.parked[self.view] = Some(outgoing);
        self.view = index;
    }

    /// A new view's temporal anti-aliasing for the frame's targets, when it is on.
    fn fresh_taa(&self) -> Option<crate::taa::Taa> {
        let (_, motion) = (self.taa.as_ref()?, self.motion.as_ref()?);
        let mut taa =
            crate::taa::Taa::new(&self.device, self.depth_size, self.output_size, &self.depth_view, motion.view());
        taa.set_layer(&self.device, self.particle_target.as_ref().map(|target| target.layer_views()));
        Some(taa)
    }

    /// Drops the parked views' history, made for targets of another size: they start again when next drawn.
    pub(super) fn drop_parked(&mut self) {
        self.parked.iter_mut().for_each(|parked| *parked = None);
    }

    /// Starts every view's temporal anti-aliasing history again.
    pub(super) fn reset_parked(&mut self) {
        for taa in self.parked.iter_mut().flatten().filter_map(|parked| parked.taa.as_mut()) {
            taa.reset();
        }
    }
}
