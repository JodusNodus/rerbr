//! The window: its event loop, and each frame of the drive or the free-fly camera.

mod feed;
mod frame;
mod input;
mod results;
mod shell;

pub use shell::run_window;

use crate::args::{Args, QuitFrom};
use crate::latency::Latency;
use crate::pacer::Pacer;
use crate::scene::Scene;
use crate::shown::{self};
use feed::Feed;
use gilrs::ev::filter::Repeat;
use gilrs::Gilrs;
use input::shifter;
use rbr_formats::data::Data;
use rbr_render::bindings::{Action, Bindings};
use rbr_render::camera::FreeFly;
use rbr_render::ffb;
use rbr_render::hud::pause::{Nav, PauseMenu};
use rbr_render::hud::results::ResultsMenu;
use rbr_render::hud::Overlay;
use rbr_render::live::Controls;
use rbr_render::perf_log;
use rbr_render::playback::ChaseLook;
use rbr_render::renderer::{Gpu, Renderer};
use rbr_render::shifter::Shifter;
use rbr_render::views::{CarView, Shown};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// The loading screen's frame period.
const LOADING_FRAME: Duration = Duration::from_millis(16);
/// The least time between frames while the window is hidden ([`rbr_render::present::Acquired::Hidden`]).
const HIDDEN_FRAME: Duration = Duration::from_millis(16);
/// How long before a paced frame's start the event loop's timer wakes it; a sleep takes the rest.
const TIMER_SLACK: Duration = Duration::from_millis(2);
/// Free-fly speed, metres per second, and the multiplier while Shift is held.
const WALK_SPEED: f32 = 20.0;
const FAST_FACTOR: f32 = 5.0;
/// Radians of turn per mouse pixel.
const LOOK_SENSITIVITY: f32 = 0.003;
/// Longest frame step used for movement, seconds. Longer stalls do not jump the camera.
const MAX_FRAME_SECONDS: f32 = 0.1;
/// Orbit zoom rate: the distance changes by a factor of `e` per second at 1.0.
const ZOOM_RATE: f32 = 1.0;
/// Chase zoom per mouse-wheel line: each line changes the distance by a factor of `e^WHEEL_ZOOM`.
const WHEEL_ZOOM: f32 = 0.1;
/// Pixels of trackpad scroll counted as one wheel line.
const PIXELS_PER_LINE: f32 = 20.0;
/// A held gamepad button repeats its menu move after this and then this often, as a held key does.
const PAD_REPEAT: Repeat = Repeat { after: Duration::from_millis(400), every: Duration::from_millis(80) };

/// Rendered frames per second, averaged over about half a second so the shown number stays readable.
/// Ours: the original has no FPS counter.
#[derive(Debug, Clone, Copy, Default)]
struct FpsMeter {
    frames: u32,
    elapsed: f32,
    shown: Option<f32>,
}

impl FpsMeter {
    /// One more frame took `dt` seconds; the smoothed rate, once a full window has passed.
    fn update(&mut self, dt: f32) -> Option<f32> {
        self.frames += 1;
        self.elapsed += dt;
        if self.elapsed >= 0.5 {
            self.shown = Some(self.frames as f32 / self.elapsed.max(f32::EPSILON));
            (self.frames, self.elapsed) = (0, 0.0);
        }
        self.shown
    }
}

/// A school demo playing: the replay's time and whether it ended (then the brief shows).
struct DemoPlay {
    t: f32,
    finished: bool,
}

/// Everything that exists once the window does.
pub(super) struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// `config` is not yet the surface's ([`rbr_render::present::configure`]).
    stale: bool,
    gpu: Gpu,
    renderer: Renderer,
    camera: FreeFly,
    /// The HUD, drawn with a drive.
    overlay: Overlay,
}
pub struct App {
    /// What the viewer shows.
    scene: Scene,
    /// The smoothed frame rate, for the HUD's FPS counter.
    meter: FpsMeter,
    /// The camera view of the drive (C cycles it).
    view: CarView,
    /// What the last frame showed: the view, or a camera the game forced over it.
    shown: Shown,
    /// Every camera's horizontal field of view, radians (`--fov`).
    fov: f32,
    /// `--max-fps`: the shortest time between drawn frames.
    frame_period: Option<Duration>,
    /// Frames start no faster than the GPU finishes them, and under fifo the screen takes them (`--pacing`).
    pacer: Option<Pacer>,
    /// When the frames went on screen, for the pacer and the latency, and the frames presented so far.
    feedback: Option<shown::Shown>,
    presented: u64,
    controls: Controls,
    /// The live mode's keys and device inputs (`--bindings`).
    bindings: Bindings,
    /// The gamepads, for the pause menu's navigation.
    gilrs: Option<Gilrs>,
    /// The wheel's force feedback (live mode), which the live process's step records set as they come.
    wheel: Option<Arc<ffb::Wheel>>,
    /// Each action's device input in the last frame, for the gear and restart presses.
    pads: [f32; Action::ALL.len()],
    /// The advanced H-shifter (`h-shifter = advanced`), in live mode.
    shifter: Option<Shifter>,
    /// The input thread (live mode).
    feed: Option<Feed>,
    /// Watching a replay (`--replay`): the stepping process serves its frames, and the window takes no device
    /// input, opens no menus and saves no ghost.
    replay: bool,
    /// The live drive whose last stage result is saved when the window closes.
    save_replay: Option<PathBuf>,
    /// A failed replay closes the window and reports failure to the launcher.
    replay_failure: Option<String>,
    running: Running,
    /// Keys currently held.
    held: HashSet<KeyCode>,
    /// Whether the left mouse button is held (mouse look).
    looking: bool,
    pointer: Option<PhysicalPosition<f64>>,
    /// The chase camera's turn around the car (`--drive`, `--live`): left drag turns, the wheel zooms, V resets.
    look: ChaseLook,
    last_frame: Instant,
    /// When the window opened (the renderer's time).
    started: Instant,
    /// The viewer time that runs the characters without a drive.
    clock: f64,
    /// The live drive's sound (`--no-sound` turns it off).
    sound: Option<rbr_render::sound::Player>,
    /// The rally school's speech feed, loading its samples from the game data on the first school frame.
    school_speech: rbr_render::sound::SchoolSpeech,
    /// Frames with a live drive, for the school end page's `sub` (never the game's `rand()` stream).
    school_frame: u32,
    quit_after: Option<(f32, QuitFrom)>,
    /// When the live race's go was first seen (`--quit-after go+S`; the performance log's summary counts from it).
    went: Option<Instant>,
    /// When the live race was first seen over (`--quit-after end+S`).
    ended: Option<Instant>,
    /// The live drive's pause menu, while it is open (the drive is paused).
    menu: Option<PauseMenu>,
    /// The school demo playing (a lesson's drive in the brief state): the replay's time and whether it ended.
    demo: Option<DemoPlay>,
    /// Actions alongside the stage result, independent of HUD visibility.
    results: Option<ResultsMenu>,
    /// When the open menu paused the drive, and the time paused before it (the renderer's time leaves it out).
    paused_at: Option<Instant>,
    paused: Duration,
    /// The menu's exit was chosen: the window closes.
    exit: bool,
    /// The live input's timing (`RBR_LATENCY`), printed when the app ends.
    latency: Option<Latency>,
}

impl App {
    fn new(scene: Scene, args: &Args, sound: Option<rbr_render::sound::Player>, running: Running, data: Data) -> App {
        let view = scene.cameras.start_view(args.view);
        let ffb = args.ffb.clone();
        let replay = args.replay.is_some();
        let shifter = shifter(args, scene.live.is_some() && !replay);
        // Gamepads navigate the menu in live mode only.
        let gilrs = scene.live.as_ref().filter(|_| !replay).and_then(|_| match Gilrs::new() {
            Ok(gilrs) => Some(gilrs),
            Err(e) => {
                eprintln!("viewer: no gamepads: {e}");
                None
            }
        });
        // The wheel's rotation, in live mode when set (again here: a wheel plugged in again has its own).
        if let Some(degrees) = ffb.rotation.filter(|_| scene.live.is_some() && !replay) {
            match rbr_render::wheel_rotation::set(degrees) {
                Ok(wheels) => wheels.iter().for_each(|w| eprintln!("viewer: {} turns {} degrees", w.name, w.degrees)),
                Err(e) => eprintln!("viewer: wheel rotation not set: {e}"),
            }
        }
        // The wheel's force, in live mode with force feedback on.
        let wheel =
            scene.live.as_ref().filter(|_| !replay && ffb.settings.enabled).and_then(|_| match ffb::Wheel::open(ffb) {
                Ok(wheel) => {
                    eprintln!("viewer: force feedback on {}", wheel.name);
                    Some(Arc::new(wheel))
                }
                Err(e) => {
                    eprintln!("viewer: no force feedback: {e}");
                    None
                }
            });
        let latency = scene.live.as_ref().and_then(|_| Latency::from_env());
        // The refresh pacing: Linux only, as a ProMotion display moves its refreshes to follow the frames (macOS
        // measures the feedback for `RBR_LATENCY` alone, so the Metal layer subclass never runs in normal play).
        let pace_refresh = args.pacing && !cfg!(target_os = "macos");
        let feedback = (pace_refresh || latency.is_some())
            .then(|| shown::Shown::new(&running.window, &running.surface))
            .and_then(|shown| shown.map_err(|e| eprintln!("viewer: no presentation feedback: {e}")).ok());
        // The refresh pacing under fifo, with the feedback: the display's refresh, ms.
        let refresh = (pace_refresh && running.config.present_mode == wgpu::PresentMode::Fifo && feedback.is_some())
            .then(|| running.window.current_monitor()?.refresh_rate_millihertz())
            .flatten()
            .map(|millihertz| 1e6 / f64::from(millihertz));
        if let Some(refresh) = refresh {
            eprintln!("viewer: pacing a refresh-bound loop to the display's {:.1} Hz", 1000.0 / refresh);
        }
        let feed = scene.live.as_ref().filter(|_| !replay).map(|_| {
            let script = latency.as_ref().and_then(Latency::script);
            Feed::start(args.bindings.clone(), script, latency.is_some())
        });
        let mut app = App {
            scene,
            clock: 0.0,
            meter: FpsMeter::default(),
            view,
            shown: Shown::Player(view),
            fov: args.fov,
            frame_period: args.max_fps.map(|fps| Duration::from_secs_f32(1.0 / fps)),
            pacer: args.pacing.then(|| Pacer::new(&running.gpu.device, refresh)),
            feedback,
            presented: 0,
            controls: Controls::default(),
            bindings: args.bindings.clone(),
            gilrs,
            wheel,
            pads: [0.0; Action::ALL.len()],
            shifter,
            feed,
            replay,
            save_replay: args.save_replay.clone().filter(|_| !replay),
            replay_failure: None,
            running,
            held: HashSet::new(),
            looking: false,
            pointer: None,
            look: ChaseLook::default(),
            last_frame: Instant::now(),
            started: Instant::now(),
            sound,
            school_speech: rbr_render::sound::SchoolSpeech::new(data),
            school_frame: 0,
            quit_after: args.quit_after,
            went: None,
            ended: None,
            menu: None,
            demo: None,
            results: None,
            paused_at: None,
            paused: Duration::ZERO,
            exit: false,
            latency,
        };
        app.attach_live();
        if perf_log::enabled() {
            let running = &mut app.running;
            let (config, asked) = (&running.config, args.present.name());
            let present = format!("{:?} (asked for {asked})", config.present_mode);
            crate::perf::begin(
                args,
                &running.gpu.adapter,
                (config.width, config.height),
                present,
                &mut running.renderer,
            );
        }
        app
    }

    /// Points the input thread and the wheel at the live process (started, or started over), whose step records then
    /// set the wheel's force and the dead zone's steering as they come.
    fn attach_live(&mut self) {
        let Some(process) = &self.scene.live else { return };
        if let Some(feed) = &self.feed {
            feed.attach(process.sender());
        }
        if let Some(wheel) = &self.wheel {
            let wheel = Arc::clone(wheel);
            process.on_step(move |step| wheel.set(step.force, step.steering()));
        }
        if self.latency.is_some() {
            process.keep_steps();
        }
    }
}

impl ApplicationHandler for App {
    /// The window is open before the app is made ([`Viewer`]).
    fn resumed(&mut self, _: &ActiveEventLoop) {}

    /// The live input's timing, while the Wayland connection its feedback shares is still open.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let (Some(path), Some(splits)) =
            (&self.save_replay, self.scene.live.as_ref().and_then(|live| live.splits.as_ref()))
        {
            if let Err(e) = rbr_recording::replay::save_result(path, splits) {
                eprintln!("viewer: replay result: {e}");
            }
        }
        if let Some(latency) = self.latency.take() {
            eprintln!("viewer: {}", latency.summary());
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let events = perf_log::span("window events");
        match event {
            WindowEvent::CloseRequested => {
                self.finish_ghost();
                event_loop.exit()
            }
            WindowEvent::Resized(size) => {
                let running = &mut self.running;
                running.config.width = size.width.max(1);
                running.config.height = size.height.max(1);
                running.stale = !rbr_render::present::configure(&running.surface, &running.gpu.device, &running.config);
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { physical_key: PhysicalKey::Code(code), state, repeat, .. },
                ..
            } => match state {
                ElementState::Pressed if self.results.is_some() => {
                    self.results_key(code, repeat);
                }
                ElementState::Pressed
                    if matches!(code, KeyCode::Enter | KeyCode::NumpadEnter) && self.accept_message() => {}
                ElementState::Pressed if self.replay => {
                    self.replay_key(code, event_loop);
                }
                ElementState::Pressed if self.scene.live.is_some() && code == KeyCode::Escape => {
                    self.press(Action::Pause);
                }
                ElementState::Pressed if self.menu.is_some() => {
                    match self.bindings.action_for_key(code) {
                        Some(Action::Pause) => self.press(Action::Pause),
                        _ => Nav::of_key(code).into_iter().for_each(|nav| self.menu_nav(nav)),
                    }
                    self.held.insert(code);
                }
                ElementState::Pressed => {
                    if code == KeyCode::Escape {
                        self.finish_ghost();
                        event_loop.exit();
                    }
                    if code == KeyCode::KeyV {
                        self.look = ChaseLook::default();
                    }
                    if self.scene.live.is_some() {
                        if let Some(action) = self.bindings.action_for_key(code) {
                            self.press(action);
                        }
                    } else if let Some(
                        action @ (Action::Wipers | Action::Headlights | Action::Camera | Action::GaugeMode),
                    ) = self.bindings.action_for_key(code)
                    {
                        self.press(action);
                    } else if let Some(playback) = &mut self.scene.playback {
                        match code {
                            KeyCode::KeyP => playback.paused = !playback.paused,
                            KeyCode::ArrowRight => playback.step(1),
                            KeyCode::ArrowLeft => playback.step(-1),
                            KeyCode::ArrowUp => playback.rate = (playback.rate * 2.0).min(16.0),
                            KeyCode::ArrowDown => playback.rate = (playback.rate / 2.0).max(1.0 / 16.0),
                            KeyCode::KeyR => {
                                playback.restart();
                                self.running.renderer.reset_history();
                            }
                            // Ours: calls the rescue helicopter, as the game flow does on a retire.
                            KeyCode::KeyK => {
                                if let Some(characters) = &mut self.scene.characters {
                                    characters.call_rescue();
                                }
                            }
                            _ => {}
                        }
                    }
                    self.held.insert(code);
                }
                ElementState::Released => {
                    self.held.remove(&code);
                }
            },
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Some(position);
                self.results_hover();
            }
            WindowEvent::CursorLeft { .. } => self.pointer = None,
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } if self.results.is_some() => {
                if state == ElementState::Pressed {
                    self.results_click();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.looking = state == ElementState::Pressed && !self.accept_message()
            }
            WindowEvent::MouseWheel { delta, .. } if self.scene.playback.is_some() => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / PIXELS_PER_LINE,
                };
                // Scrolling up (positive) moves the eye in.
                self.look.zoom_by((-lines * WHEEL_ZOOM).exp());
            }
            WindowEvent::RedrawRequested => {
                drop(events);
                self.frame()
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        let _events = perf_log::span("window events");
        if let (true, DeviceEvent::MouseMotion { delta: (dx, dy) }) = (self.looking, &event) {
            let (yaw, pitch) = (
                *dx as f32 * LOOK_SENSITIVITY,
                // Mouse up (negative dy) looks up.
                -*dy as f32 * LOOK_SENSITIVITY,
            );
            match (&mut self.scene.orbit, &self.scene.playback) {
                (Some(orbit), _) => orbit.turn(yaw, pitch),
                (None, Some(_)) => self.look.turn(yaw, pitch),
                (None, None) => self.running.camera.turn(yaw, pitch),
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let since = |(_, from): (f32, QuitFrom)| match from {
            QuitFrom::Open => Some(self.started),
            QuitFrom::Go => self.went,
            QuitFrom::End => self.ended,
        };
        let quit = self.quit_after.is_some_and(|q| since(q).is_some_and(|t| t.elapsed().as_secs_f32() > q.0));
        if quit || self.exit {
            self.finish_ghost();
            event_loop.exit();
        }
        // Under `--max-fps` and the pacer the next frame waits for its time: on the event loop's timer (a ms
        // coarse) until shortly before it, then in a sleep, as the frame's start reads the input.
        let period = self.frame_period.into_iter().chain(self.pacer.as_mut().map(Pacer::period)).max();
        let now = Instant::now();
        let wait = period.and_then(|period| (self.last_frame + period).checked_duration_since(now));
        match wait {
            Some(wait) if wait > TIMER_SLACK => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + wait - TIMER_SLACK));
                return;
            }
            Some(wait) => std::thread::sleep(wait),
            None => {}
        }
        // The next frame is drawn now, not on `RedrawRequested`: Wayland compositors send that only after they took
        // the last frame, which waits for the GPU, so the CPU's half of a frame never overlapped the GPU's (49 fps
        // against 68 in a cockpit on cachy's Hyprland). Acquiring the surface's texture holds the loop back instead.
        // macOS keeps the redraw request.
        if cfg!(not(target_os = "macos")) {
            event_loop.set_control_flow(ControlFlow::Poll);
            self.frame();
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
            self.running.window.request_redraw();
        }
    }
}
