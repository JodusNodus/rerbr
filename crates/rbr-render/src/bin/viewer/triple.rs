//! Triple screens in the window (`--triple`, `rbr_render::triple`): one window split in three (a spanned display, or
//! windowed), or with `--triple-monitors` a borderless window on each monitor, the main one on the centre's. A curved
//! panel (`--curved`) is one window split in its three segments. The centre view is drawn first (the exposure follows
//! it) and carries the HUD.

use glam::Mat4;
use rbr_render::camera::Lens;
use rbr_render::present::Acquired;
use rbr_render::triple::Screens;
use std::sync::Arc;
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowAttributes};

/// The frame's views of `lens` on screens of `screen` pixels: one, or of `screens` the left, centre and right; with the
/// index of the one the HUD goes on.
pub fn views(lens: &Lens, screens: Option<&Screens>, screen: (u32, u32)) -> (Vec<(Mat4, Mat4)>, usize) {
    let views = lens.views(screens, screen.0 as f32 / screen.1.max(1) as f32);
    let centre = views.len() / 2;
    (views, centre)
}

/// The order the views of [`views`] are drawn in: the centre first, which draws what they share and sets the exposure.
pub fn order(count: usize) -> impl Iterator<Item = usize> {
    let centre = count / 2;
    std::iter::once(centre).chain((0..count).filter(move |&i| i != centre))
}

/// One screen's size in a window of `size` pixels: a third of it when the window holds the three (`spanned`).
pub fn screen(size: (u32, u32), spanned: bool) -> (u32, u32) {
    match spanned {
        true => ((size.0 / 3).max(1), size.1),
        false => size,
    }
}

/// Where view `index` of a spanned window's three goes in a window `width` wide, its top left corner in pixels: the
/// three centred, a width not of three leaving its spare columns at the outer edges.
pub fn spanned_at(index: usize, screen: (u32, u32), width: u32) -> (u32, u32) {
    (width.saturating_sub(screen.0 * 3) / 2 + screen.0 * index as u32, 0)
}

/// Where the HUD, the menus and the loading screen lay out in a window of `size` holding the three views (x, y, width
/// and height in pixels): a triple rig's centre screen, a curved panel's middle at most 16:9 wide.
pub fn hud_region(screens: &Screens, size: (u32, u32)) -> [u32; 4] {
    match screens {
        Screens::Triple(_) => {
            let screen = screen(size, true);
            let (x, y) = spanned_at(1, screen, size.0);
            [x, y, screen.0, screen.1]
        }
        Screens::Curved(_) => {
            let width = size.0.min(size.1 * 16 / 9);
            [(size.0 - width) / 2, 0, width, size.1]
        }
    }
}

/// The monitor named `name`, for a window to fill.
pub fn monitor(event_loop: &ActiveEventLoop, name: &str) -> Result<MonitorHandle, String> {
    let names: Vec<String> = event_loop.available_monitors().filter_map(|m| m.name()).collect();
    event_loop
        .available_monitors()
        .find(|monitor| monitor.name().as_deref() == Some(name))
        .ok_or_else(|| format!("--triple-monitors: no monitor {name} (monitors: {})", names.join(", ")))
}

/// A side screen's window (`--triple-monitors`): borderless on its monitor, presenting the side view.
pub struct Side {
    pub window: Arc<Window>,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    /// `config` is not yet the surface's ([`rbr_render::present::configure`]).
    pub stale: bool,
}

impl Side {
    /// A window like the main one's `attributes` filling `monitor`, presenting like `config` on `device`.
    pub fn open(
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
        monitor: MonitorHandle,
        (instance, device): (&wgpu::Instance, &wgpu::Device),
        config: &wgpu::SurfaceConfiguration,
    ) -> Result<Side, Box<dyn std::error::Error>> {
        let attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let surface = instance.create_surface(window.clone())?;
        let size = window.inner_size();
        let config =
            wgpu::SurfaceConfiguration { width: size.width.max(1), height: size.height.max(1), ..config.clone() };
        let stale = !rbr_render::present::configure(&surface, device, &config);
        Ok(Side { window, surface, config, stale })
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: winit::dpi::PhysicalSize<u32>) {
        (self.config.width, self.config.height) = (size.width.max(1), size.height.max(1));
        self.stale = !rbr_render::present::configure(&self.surface, device, &self.config);
    }

    /// The surface's texture for this frame, configuring it again when it went stale.
    pub fn acquire(&mut self, device: &wgpu::Device) -> Option<wgpu::SurfaceTexture> {
        if self.stale {
            self.stale = !rbr_render::present::configure(&self.surface, device, &self.config);
            if self.stale {
                return None;
            }
        }
        match rbr_render::present::acquire(&self.surface) {
            Acquired::Frame(frame) => Some(frame),
            Acquired::Hidden => None,
            Acquired::Failed => {
                self.stale = !rbr_render::present::configure(&self.surface, device, &self.config);
                None
            }
        }
    }

    /// Clears the window to black (the loading screen's frames).
    pub fn clear(&mut self, gpu: (&wgpu::Device, &wgpu::Queue)) {
        let Some(frame) = self.acquire(gpu.0) else { return };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = gpu.0.create_command_encoder(&Default::default());
        clear(&mut encoder, &view);
        gpu.1.submit([encoder.finish()]);
        self.window.pre_present_notify();
        gpu.1.present(frame);
    }
}

/// Clears `target` to black.
pub fn clear(encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_centre_view_is_drawn_first() {
        assert_eq!(order(1).collect::<Vec<_>>(), [0]);
        assert_eq!(order(3).collect::<Vec<_>>(), [1, 0, 2]);
    }

    #[test]
    fn a_spanned_window_holds_three_screens() {
        assert_eq!(screen((5760, 1080), true), (1920, 1080));
        assert_eq!(screen((1920, 1080), false), (1920, 1080));
        assert_eq!(spanned_at(2, (1920, 1080), 5760), (3840, 0));
    }

    #[test]
    fn a_width_not_of_three_leaves_its_spare_columns_at_the_edges() {
        let third = screen((3842, 1080), true);
        assert_eq!(third, (1280, 1080));
        assert_eq!([0, 1, 2].map(|i| spanned_at(i, third, 3842).0), [1, 1281, 2561]);
        assert_eq!(spanned_at(0, screen((3841, 1080), true), 3841).0, 0);
    }

    #[test]
    fn the_hud_goes_on_the_centre_screen_or_the_panels_middle() {
        let rig = Screens::Triple(rbr_render::triple::Rig { diagonal: 27.0, bezel: 10.0, distance: 70.0, angle: None });
        assert_eq!(hud_region(&rig, (5760, 1080)), [1920, 0, 1920, 1080]);
        let panel = Screens::Curved(rbr_render::triple::Curved { diagonal: 49.0, radius: 1800.0, distance: 70.0 });
        assert_eq!(hud_region(&panel, (5120, 1440)), [1280, 0, 2560, 1440]);
        assert_eq!(hud_region(&panel, (3440, 1440)), [440, 0, 2560, 1440]);
        // Narrower than 16:9, all of the width.
        assert_eq!(hud_region(&panel, (1280, 1024)), [0, 0, 1280, 1024]);
    }
}
