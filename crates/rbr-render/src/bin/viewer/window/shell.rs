//! Opening the window and its loading screen ([`run_window`]).

use crate::args::Args;
use crate::draw::start_camera;
use crate::loading::{Loaded, Loading};
use crate::screenshot::SIZE;
use crate::triple::{self, Side};
use rbr_formats::data::Data;
use rbr_render::hud::stock::Stock;
use rbr_render::hud::Overlay;
use rbr_render::icon;
use rbr_render::present::Acquired;
use rbr_render::renderer::{self, Gpu};
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use super::{App, Running, LOADING_FRAME};

/// Opens the window, shows the loading screen while a thread loads what `args` name, then runs the drive in it.
pub fn run_window(args: Args, data: Data, fmod_bank: Option<PathBuf>) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let mut viewer = Viewer { args: Arc::new(args), data, fmod_bank, phase: Phase::Opening, failed: None };
    event_loop.run_app(&mut viewer).map_err(|e| e.to_string())?;
    viewer.failed.map_or(Ok(()), Err)
}

/// The window's handler: the loading screen, then the drive.
struct Viewer {
    args: Arc<Args>,
    data: Data,
    /// The community car's FMOD bank, for the sound the load starts.
    fmod_bank: Option<PathBuf>,
    phase: Phase,
    /// Why the window or the load failed.
    failed: Option<String>,
}

enum Phase {
    Opening,
    Loading(Box<(Shell, Loading)>),
    Running(Box<App>),
}

impl Viewer {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        self.failed = Some(error);
        event_loop.exit();
    }

    /// The loaded scene's drive, in the loading screen's window.
    fn run(&self, shell: Shell, Loaded { scene, renderer, sound }: Loaded) -> App {
        let args = &*self.args;
        let Shell { window, surface, config, stale, gpu, overlay, sides, .. } = shell;
        let camera = start_camera(&scene.stage, scene.chase);
        let rig = args.triple;
        let running = Running { window, surface, config, stale, gpu, renderer, camera, overlay, rig, sides };
        App::new(scene, args, sound, running, self.data.clone())
    }
}

impl ApplicationHandler for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !matches!(self.phase, Phase::Opening) {
            return;
        }
        match Shell::open(event_loop, &self.args, &self.data) {
            Ok(shell) => {
                let (gpu, config) = (shell.gpu.clone(), &shell.config);
                let (args, data, fmod_bank) = (self.args.clone(), self.data.clone(), self.fmod_bank.clone());
                // The first frame warms up at one screen's size (a third of a window holding a triple screen's three).
                let screen = triple::screen((config.width, config.height), shell.spanned);
                let loading = Loading::start(args, data, fmod_bank, gpu, config.format, screen);
                self.phase = Phase::Loading(Box::new((shell, loading)));
            }
            Err(error) => self.fail(event_loop, error.to_string()),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let (shell, loading) = match &mut self.phase {
            Phase::Opening => return,
            Phase::Loading(loading) => &mut **loading,
            Phase::Running(app) => return app.window_event(event_loop, id, event),
        };
        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event: KeyEvent { physical_key: PhysicalKey::Code(KeyCode::Escape), state: ElementState::Pressed, .. },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => match shell.sides.iter_mut().find(|side| side.window.id() == id) {
                Some(side) => side.resize(&shell.gpu.device, size),
                None => shell.resize(size),
            },
            WindowEvent::RedrawRequested => shell.draw_loading(loading),
            _ => {}
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        if let Phase::Running(app) = &mut self.phase {
            app.device_event(event_loop, id, event);
        }
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Phase::Running(app) = &mut self.phase {
            self.failed = app.replay_failure.take();
            app.exiting(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let (shell, loading) = match &mut self.phase {
            Phase::Opening => return,
            Phase::Loading(loading) => &mut **loading,
            Phase::Running(app) => return app.about_to_wait(event_loop),
        };
        let Some(loaded) = loading.poll() else {
            shell.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + LOADING_FRAME));
            return;
        };
        let Phase::Loading(loading) = std::mem::replace(&mut self.phase, Phase::Opening) else { unreachable!() };
        match loaded {
            Ok(loaded) => self.phase = Phase::Running(Box::new(self.run(loading.0, loaded))),
            Err(error) => self.fail(event_loop, error),
        }
    }
}

/// The window with its surface, GPU and overlay: what the loading screen draws in, and the drive after it.
struct Shell {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// `config` is not yet the surface's ([`rbr_render::present::configure`]).
    stale: bool,
    gpu: Gpu,
    overlay: Overlay,
    /// The side screens' windows (`--triple-monitors`), left and right.
    sides: Vec<Side>,
    /// The window holds a triple screen's three views (`--triple` without `--triple-monitors`).
    spanned: bool,
}

impl Shell {
    fn open(event_loop: &ActiveEventLoop, args: &Args, data: &Data) -> Result<Shell, Box<dyn Error>> {
        let attributes = Window::default_attributes().with_title("ReRBR").with_window_icon(icon::window_icon());
        let attributes = match args.size {
            Some((width, height)) => attributes.with_inner_size(PhysicalSize::new(width, height)),
            None => attributes.with_inner_size(LogicalSize::new(SIZE.0, SIZE.1)),
        };
        // The Wayland app id and the X11 class (one field in winit), which compositor rules can match.
        #[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
        let attributes =
            winit::platform::wayland::WindowAttributesExtWayland::with_name(attributes, icon::APP_ID, icon::APP_ID);
        // A triple screen's own monitors (`--triple-monitors`): the main window on the centre one, a side's on each other.
        let monitors = match (&args.triple, &args.triple_monitors) {
            (Some(_), Some(names)) => {
                Some(names.iter().map(|name| triple::monitor(event_loop, name)).collect::<Result<Vec<_>, _>>()?)
            }
            _ => None,
        };
        // Borderless on the current monitor: the surface follows the monitor's size (`WindowEvent::Resized`), and a
        // Wayland compositor can scan it out directly.
        let fullscreen = match &monitors {
            Some(monitors) => Some(Fullscreen::Borderless(Some(monitors[1].clone()))),
            None => args.fullscreen.then_some(Fullscreen::Borderless(None)),
        };
        let window = Arc::new(event_loop.create_window(attributes.clone().with_fullscreen(fullscreen))?);
        let instance = renderer::instance();
        // Kept to the process's end: dropped with the window, the Vulkan loader unloaded the driver, its layers and
        // itself while the exit handlers they had registered (MangoHud's, set by `MANGOHUD=1`) were still to run, and
        // the exit after every drive crashed (SIGSEGV or `free(): invalid pointer`, 2026-11-03).
        std::mem::forget(instance.clone());
        let surface = instance.create_surface(window.clone())?;
        let gpu = renderer::request_gpu(&instance, Some(&surface))?;
        let capabilities = surface.get_capabilities(&gpu.adapter);
        let (format, color_space) = rbr_render::present::surface_colour(&capabilities, args.hdr);
        let size = window.inner_size();
        let present_mode = args.present.pick(&capabilities.present_modes);
        eprintln!("viewer: presenting with {present_mode:?} (asked for {})", args.present.name());
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            // The frame's alpha is not shown, as with the game's X8R8G8B8 back buffer (see `write_png`).
            alpha_mode: if capabilities.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
                wgpu::CompositeAlphaMode::Opaque
            } else {
                capabilities.alpha_modes[0]
            },
            view_formats: vec![],
            color_space,
            // Two frames queued: the frame waits for its texture before it reads the input ([`App::frame`]), so the
            // second adds no lag, and with one the frame missed every other refresh once the wait came first (27
            // frames a second against 60 at 4K on cachy, `RBR_LATENCY`).
            desired_maximum_frame_latency: 2,
        };
        let stale = !rbr_render::present::configure(&surface, &gpu.device, &config);
        if args.hdr {
            if color_space.is_hdr() {
                let headroom = surface.display_hdr_info(&gpu.adapter).headroom;
                eprintln!("viewer: HDR in {color_space:?}, display headroom {headroom:?}");
            } else {
                eprintln!("viewer: HDR asked for, but the display has no extended-range sRGB: drawing SDR");
            }
        }
        let overlay = Overlay::with_hud(&gpu.device, format, Box::new(Stock::new(data)));
        let sides = match monitors {
            Some(monitors) => [&monitors[0], &monitors[2]]
                .into_iter()
                .map(|monitor| {
                    Side::open(event_loop, attributes.clone(), monitor.clone(), (&instance, &gpu.device), &config)
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        let spanned = args.triple.is_some() && sides.is_empty();
        Ok(Shell { window, surface, config, stale, gpu, overlay, sides, spanned })
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.config.width = size.width.max(1);
        self.config.height = size.height.max(1);
        self.stale = !rbr_render::present::configure(&self.surface, &self.gpu.device, &self.config);
    }
}

/// The centre screen of a window split in three: where the HUD and the menus go.
pub(super) fn spanned_region(size: (u32, u32)) -> [u32; 4] {
    let screen = triple::screen(size, true);
    let (x, y) = triple::spanned_at(1, screen);
    [x, y, screen.0, screen.1]
}

impl Shell {
    /// A frame of the loading screen: black under it.
    fn draw_loading(&mut self, loading: &mut Loading) {
        if self.stale {
            self.stale = !rbr_render::present::configure(&self.surface, &self.gpu.device, &self.config);
            if self.stale {
                return;
            }
        }
        let frame = match rbr_render::present::acquire(&self.surface) {
            Acquired::Frame(frame) => frame,
            // The loading screen's frames are paced by `about_to_wait`.
            Acquired::Hidden => return,
            Acquired::Failed => {
                self.stale = !rbr_render::present::configure(&self.surface, &self.gpu.device, &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("loading"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        let size = (self.config.width, self.config.height);
        // Spanned, the loading screen goes on the centre screen.
        self.overlay.set_region(self.spanned.then(|| spanned_region(size)));
        self.overlay
            .draw_with(&self.gpu.device, &self.gpu.queue, &mut encoder, &view, size, |ctx, _| loading.paint(ctx));
        self.gpu.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
        for side in &mut self.sides {
            side.clear((&self.gpu.device, &self.gpu.queue));
        }
    }
}
