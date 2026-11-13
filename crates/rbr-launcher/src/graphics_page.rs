//! The Graphics & sound page: the window, the camera, the HUD, the sound and the renderer's quality.

use egui::{Color32, ComboBox, DragValue};
use rbr_render::camera_prefs;
use rbr_render::coords;
use rbr_render::hud::layout::Layout;
use rbr_render::hud::model::STACK_MAX;
use rbr_render::hud::{Part, Parts};
use rbr_render::mirrors::Rate;
use rbr_render::present::Present;
use rbr_render::renderer::{Detail, Reflections};
use rbr_render::triple::Rig;

use crate::pages::{combo, fields, section, Choice, DRIVE_FIELDS, FIELD, WARN};
use crate::Form;

/// The Quality section's choices: the renderer's additions (`viewer --enhance`), the wet roads (`--enhance wet` and
/// `--reflections`), the detail (`--detail`) and the mirrors (`--mirrors`). Each is kept under its own key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Graphics {
    pub lighting: bool,
    /// Our rain, snow and particles (`particles`).
    pub particles: bool,
    pub cabin: bool,
    pub contact: bool,
    pub grass: bool,
    /// The windscreen as glass from inside, and the dash reflected in it (`windscreen`).
    pub windscreen: bool,
    /// Our tyre marks, which no preset changes: free, and off until reviewed in play.
    pub marks: bool,
    /// The motion blur (`blur`), which no preset changes: a taste, off unless asked for.
    pub motion_blur: bool,
    pub wet_roads: WetRoads,
    pub detail: Detail,
    pub mirrors: Rate,
}

/// The additions always on, as they cost nothing: 16x anisotropic filtering, the soft decals, the flowing water.
pub const ALWAYS_ON: [&str; 3] = ["anisotropy", "decals", "water"];

/// A new install's choices: the High preset.
pub const DEFAULT_GRAPHICS: Graphics = Quality::Ultra.graphics(false);

impl Default for Graphics {
    /// A new install's choices.
    fn default() -> Self {
        DEFAULT_GRAPHICS
    }
}

/// The width of the Quality section's boxes, beside the left column.
const QUALITY_FIELD: f32 = 170.0;

/// The wet roads: as in 1.02, or our film and puddles (`--enhance wet`) reflecting the baked stage and sky (low), and
/// the scenery on screen too, on the wet roads and the water (medium and high, `--reflections`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WetRoads {
    Off,
    Low,
    Medium,
    High,
}

impl WetRoads {
    /// The choices offered, in the launcher's order.
    pub const ALL: [WetRoads; 4] = [WetRoads::High, WetRoads::Medium, WetRoads::Low, WetRoads::Off];

    /// The viewer's screen-space reflections for it.
    pub fn reflections(self) -> Reflections {
        match self {
            WetRoads::Off | WetRoads::Low => Reflections::Off,
            WetRoads::Medium => Reflections::Medium,
            WetRoads::High => Reflections::High,
        }
    }
}

impl Choice for WetRoads {
    const ALL: &[Self] = &WetRoads::ALL;
    fn name(self) -> &'static str {
        match self {
            WetRoads::Off => "off",
            WetRoads::Low => "low",
            WetRoads::Medium => "medium",
            WetRoads::High => "high",
        }
    }
    fn label(self) -> &'static str {
        match self {
            WetRoads::Off => "Off",
            WetRoads::Low => "Low",
            WetRoads::Medium => "Medium",
            WetRoads::High => "High",
        }
    }
}

impl Choice for Detail {
    const ALL: &[Self] = &Detail::ALL;
    fn name(self) -> &'static str {
        Detail::name(self)
    }
    fn label(self) -> &'static str {
        match self {
            Detail::High => "High",
            Detail::Ultra => "Ultra",
        }
    }
}

/// The Quality section's presets (their cost: `notes/renderer.md` "Cost on an M3 Pro").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// The game's look with the free additions.
    Low,
    /// Every addition but our lighting, the costliest.
    Medium,
    High,
    /// High with the Ultra detail.
    Ultra,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Low, Quality::Medium, Quality::High, Quality::Ultra];

    /// Its choices, the tyre marks `marks`.
    pub const fn graphics(self, marks: bool) -> Graphics {
        let high = Graphics {
            lighting: true,
            particles: true,
            cabin: true,
            contact: true,
            grass: true,
            windscreen: true,
            marks,
            motion_blur: false,
            wet_roads: WetRoads::High,
            detail: Detail::High,
            mirrors: Rate::Every(1),
        };
        match self {
            Quality::Low => Graphics {
                lighting: false,
                particles: false,
                cabin: false,
                contact: false,
                grass: false,
                windscreen: false,
                wet_roads: WetRoads::Off,
                mirrors: Rate::Off,
                ..high
            },
            Quality::Medium => Graphics {
                lighting: false,
                grass: false,
                windscreen: false,
                wet_roads: WetRoads::Medium,
                mirrors: Rate::Every(3),
                ..high
            },
            Quality::High => high,
            Quality::Ultra => Graphics { detail: Detail::Ultra, ..high },
        }
    }

    /// Its choices over `kept`, whose tyre marks and motion blur stay.
    pub const fn over(self, kept: Graphics) -> Graphics {
        Graphics { motion_blur: kept.motion_blur, ..self.graphics(kept.marks) }
    }

    /// The preset `graphics` are, if any.
    pub fn of(graphics: Graphics) -> Option<Quality> {
        Quality::ALL.into_iter().find(|q| q.over(graphics) == graphics)
    }

    fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
            Quality::Ultra => "Ultra",
        }
    }
}

/// The render scales the Graphics & sound page offers, percent of the window's width and height (`viewer --render-scale`): below
/// 100 the frame is drawn smaller and the temporal anti-aliasing upscales it, for
/// high refresh rates at 1440p and above. 100, the first, is the default.
pub const RENDER_SCALES: [u32; 5] = [100, 87, 75, 67, 50];

/// A render scale's label.
fn scale_label(percent: u32) -> String {
    match percent {
        100 => "100% (native)".into(),
        percent => format!("{percent}%"),
    }
}

/// A detail choice's label.
/// A mirrors' rate's label.
fn mirrors_label(rate: Rate) -> &'static str {
    match rate {
        Rate::Off => "Off",
        Rate::Every(1) => "Every frame",
        Rate::Every(2) => "Every 2nd frame",
        Rate::Every(_) => "Every 3rd frame",
    }
}

/// A frame presentation's label and hover tip (`viewer --present`). macOS offers vsync and uncapped only
/// ([`Present::OFFERED`]), vsync the default.
fn present_label(present: Present) -> (&'static str, &'static str) {
    let mac = cfg!(target_os = "macos");
    match present {
        Present::Mailbox => (
            "Fast, no tearing (recommended)",
            "Shows the newest frame at each refresh: no tearing, and no wait for the refresh (where the system has no \
             such mode, this is vsync)",
        ),
        Present::Fifo if mac => (
            "Vsync (recommended)",
            "Each frame waits for the display's refresh: no tearing, and no more frames drawn than the display shows \
             (macOS has no tear-free mode without the wait)",
        ),
        Present::Fifo => {
            ("Vsync", "Each frame waits for the display's refresh: a frame just too slow for one refresh waits for two")
        }
        Present::Immediate if mac => (
            "Uncapped",
            "Shows each frame as soon as it is drawn: the lowest lag, with the GPU drawing as many frames as it can \
             (the frame cap holds it to the display's rate); fullscreen, it may tear",
        ),
        Present::Immediate => (
            "Uncapped (may tear)",
            "Shows each frame as soon as it is drawn: the lowest lag, but it tears where the compositor allows tearing \
             (Hyprland with an immediate window rule); elsewhere it shows like the fast mode",
        ),
    }
}

/// The frame caps offered, frames a second (`viewer --max-fps`); 0, the first, is none.
pub const FRAME_CAPS: [u32; 7] = [0, 30, 60, 90, 120, 144, 240];

/// Triple screens (ours, `viewer --triple`): three like screens side by side, the side ones turned towards the
/// player. Each screen's diagonal in inches (10 to 65, 27 by default), the frame around each screen's image in
/// millimetres (0 to 60, 10 by default), the eye to the centre screen in centimetres (30 to 200, 70 by default),
/// and the side screens' turn in degrees (`None`: facing the eye, the default; 0 to 90).
pub const TRIPLE_DIAGONAL_DEFAULT: f32 = 27.0;
pub const TRIPLE_DIAGONAL_RANGE: std::ops::RangeInclusive<f32> = 10.0..=65.0;
pub const TRIPLE_BEZEL_DEFAULT: f32 = 10.0;
pub const TRIPLE_BEZEL_RANGE: std::ops::RangeInclusive<f32> = 0.0..=60.0;
pub const TRIPLE_DISTANCE_DEFAULT: f32 = 70.0;
pub const TRIPLE_DISTANCE_RANGE: std::ops::RangeInclusive<f32> = 30.0..=200.0;
pub const TRIPLE_ANGLE_RANGE: std::ops::RangeInclusive<f32> = 0.0..=rbr_render::triple::MAX_ANGLE;

/// `names` once each monitor is picked: `viewer --triple-monitors` takes three.
fn picked(names: Option<&[String; 3]>) -> Option<&[String; 3]> {
    names.filter(|names| names.iter().all(|name| !name.is_empty()))
}

/// One screen's aspect on a display or window `whole` pixels: all of it with a window a monitor (`per_monitor`),
/// else a third of it.
fn screen_aspect(whole: (u32, u32), per_monitor: bool) -> f32 {
    let aspect = whole.0 as f32 / whole.1.max(1) as f32;
    match per_monitor {
        true => aspect,
        false => aspect / 3.0,
    }
}

/// A frame cap's label.
fn cap_label(fps: u32) -> String {
    match fps {
        0 => "Off".into(),
        fps => format!("{fps} fps"),
    }
}

/// The HUD's options (`viewer --no-hud`, `--mph`, `--fps`, `--pace-stack`, `--hud-hide`, `--hud-show`), and the
/// cockpit's working dials (`--no-dials`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudChoice {
    pub show: bool,
    pub mph: bool,
    pub fps: bool,
    /// The pace stack's notes, 0 to 8 (the game's `PN_STACK` stops at 4; 0, its default, hides it).
    pub stack: u8,
    /// The working dials of the RSF cars with RBRHUD gauges in the install (on by default).
    pub dials: bool,
    /// The HUD's parts shown (all but the telemetry by default).
    pub parts: Parts,
    /// Where they stand and how large (`--hud-layout`, `--hud-scale`), set on the pause menu's HUD page while
    /// driving and passed on from the file.
    pub layout: Layout,
}

/// Each HUD part's hover tip, by [`Part::ALL`].
const PART_TIPS: [&str; Part::ALL.len()] = [
    "The race time and its penalties",
    "The progress bar with the splits, the distance driven and the distance to go",
    "The symbol of each call at the top",
    "The time and difference as each split is passed, and the running gap to the ghost",
    "The rev dial, which flashes at the shift point, with the gear, speed, revs, water and turbo",
    "The pedal arcs inside the dial and the steering under it",
    "The lit warning lamps and the damaged parts at the bottom left, shown only when there are any",
    "The stand-still and car-problem messages",
    "Each tyre's temperature and wear beside the dial: green at its best, blue too cold for full grip, red too hot",
    "A running chart of the pedals and the steering over the last 10 seconds",
];

impl Default for HudChoice {
    fn default() -> Self {
        HudChoice {
            show: true,
            mph: false,
            fps: false,
            stack: 0,
            dials: true,
            parts: Parts::default(),
            layout: Layout::default(),
        }
    }
}

/// The loudest sound volume the viewer takes (`viewer --volume`).
pub const MAX_VOLUME: f32 = 4.0;

/// The loudest each sound group takes (`viewer --volumes`).
pub const MAX_GROUP_VOLUME: f32 = 2.0;

impl Form {
    /// The Graphics & sound page: the window, the camera, the HUD and the sound on the left; the renderer's quality on
    /// the right.
    pub fn graphics_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("graphics").show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(DRIVE_FIELDS);
                    self.view_fields(ui);
                });
                ui.add_space(20.0);
                ui.vertical(|ui| self.quality(ui));
            });
        });
    }

    fn view_fields(&mut self, ui: &mut egui::Ui) {
        section(ui, "Display");
        fields(ui, "display", |ui| {
            let tip =
                "Fills the monitor at its own resolution, borderless: the compositor can show the frames directly";
            ui.label("Fullscreen").on_hover_text(tip);
            ui.checkbox(&mut self.fullscreen, "at the monitor's resolution").on_hover_text(tip);
            ui.end_row();

            ui.label("Resolution").on_hover_text("The driving window's size when not fullscreen");
            ui.add_enabled_ui(!self.fullscreen, |ui| {
                ComboBox::from_id_salt("size").width(FIELD).height(320.0).selected_text(size_label(self.size)).show_ui(
                    ui,
                    |ui| {
                        for &size in &self.sizes {
                            ui.selectable_value(&mut self.size, size, size_label(size));
                        }
                    },
                );
            });
            ui.end_row();

            let tip = "Draws the 3D view at this share of the window's size, which the temporal anti-aliasing \
                       upscales to it over the frames: faster, for high refresh rates at 1440p and above; the lower, \
                       the softer, and fast-moving edges grow rougher";
            ui.label("Render scale").on_hover_text(tip);
            combo(ui, "render scale", QUALITY_FIELD, &mut self.render_scale, &RENDER_SCALES, scale_label)
                .on_hover_text(tip);
            ui.end_row();

            let tip = if self.upscaler {
                "Upscales the car's and the stage's textures 4x (to at most 4096 texels a side), so the bonnet, the \
                 livery, the cockpit, the gravel and the signs stay sharp up close. The first drive with \
                 a car or on a stage waits while they are made; then they are kept and load at once"
            } else if crate::upscaler::available() {
                "Needs the upscaler, about 50 MB: Download upscaler… gets it"
            } else {
                "Needs the upscaler, which Real-ESRGAN does not build for this platform"
            };
            ui.label("Textures").on_hover_text(tip);
            if self.upscaler {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.upscale_textures, "upscaled").on_hover_text(tip);
                    self.upscaler_cache_ui(ui);
                });
            } else if crate::upscaler::available() {
                ui.vertical(|ui| self.upscaler_download_ui(ui));
            } else {
                ui.add_enabled_ui(false, |ui| ui.checkbox(&mut self.upscale_textures, "upscaled"))
                    .response
                    .on_disabled_hover_text(tip);
            }
            ui.end_row();

            let (label, tip) = present_label(self.present);
            ui.label("Frame presentation").on_hover_text(tip);
            ComboBox::from_id_salt("present")
                .width(FIELD)
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for &present in Present::OFFERED {
                        let (label, tip) = present_label(present);
                        ui.selectable_value(&mut self.present, present, label).on_hover_text(tip);
                    }
                })
                .response
                .on_hover_text(tip);
            ui.end_row();

            let tip = "The most frames a second the driving window draws. Uncapped with the display's refresh rate \
                       here keeps the lowest lag without the GPU drawing frames the display never shows";
            ui.label("Frame cap").on_hover_text(tip);
            combo(ui, "frame cap", QUALITY_FIELD, &mut self.max_fps, &FRAME_CAPS, cap_label).on_hover_text(tip);
            ui.end_row();

            if rbr_render::present::HDR_OFFERED {
                let tip = "On an HDR display (a Mac's extended dynamic range) the sun, the lamps and their glints \
                           show brighter than its white, as far as it reaches at its brightness; the rest of the \
                           frame and the HUD as without. Elsewhere the window draws as without it";
                ui.label("HDR").on_hover_text(tip);
                ui.checkbox(&mut self.hdr, "highlights past the display's white").on_hover_text(tip);
                ui.end_row();
            }

            ui.label("FPS counter");
            ui.checkbox(&mut self.hud.fps, "top-right corner");
            ui.end_row();

            let tip =
                "Writes how long each frame, each stage on the CPU and each pass on the GPU takes, once a second \
                       (a .log to paste in a bug report) and the last 600 frames at the end (a .trace.json for \
                       ui.perfetto.dev). Costs well under 0.2 ms a frame";
            ui.label("Performance log").on_hover_text(tip);
            ui.vertical(|ui| {
                ui.checkbox(&mut self.perf_log, "write one for each drive").on_hover_text(tip);
                if self.perf_log {
                    ui.weak(format!("In {}", self.logs_dir().display()));
                }
            });
            ui.end_row();
        });

        self.triple_fields(ui);

        section(ui, "Camera");
        fields(ui, "camera", |ui| {
            let tip = "The horizontal field of view of the in-car views and the free camera; the chase view has its \
                       own below. The pause menu's Camera page sets it too";
            ui.label("Field of view").on_hover_text(tip);
            ui.add(egui::Slider::new(&mut self.fov, coords::FOV_RANGE_DEGREES).step_by(1.0).suffix("°"))
                .on_hover_text(tip);
            ui.end_row();

            let tip = "The horizontal field of view of the chase view (65 by default). The pause menu's Camera page \
                       sets it too";
            ui.label("Chase field of view").on_hover_text(tip);
            ui.add(egui::Slider::new(&mut self.chase_fov, coords::FOV_RANGE_DEGREES).step_by(1.0).suffix("°"))
                .on_hover_text(tip);
            ui.end_row();

            let tip = "The in-car view's head: it keeps the horizon steadier over bumps. Off, the view is fixed to \
                       the car. The pause menu's Camera page sets it too";
            ui.label("Head motion").on_hover_text(tip);
            let mut cockpit = self.cockpit;
            ui.add_enabled(!self.read_only, egui::Checkbox::new(&mut cockpit.head, "in-car view")).on_hover_text(tip);
            ui.end_row();

            let tip = "What the cockpit view shows: the driver's arms on the wheel, the co-driver beside you, and the \
                       car's steering wheel (leave it out when driving with a wheel of your own). The pause menu's \
                       Camera page sets them too";
            ui.label("Cockpit").on_hover_text(tip);
            ui.horizontal(|ui| {
                let choices = [
                    (&mut cockpit.driver, "Driver's arms"),
                    (&mut cockpit.codriver, "Co-driver"),
                    (&mut cockpit.wheel, "Steering wheel"),
                ];
                for (on, label) in choices {
                    ui.add_enabled(!self.read_only, egui::Checkbox::new(on, label)).on_hover_text(tip);
                }
            });
            if cockpit != self.cockpit {
                self.cockpit = cockpit;
                if let Err(e) = self.edit_camera(|prefs| prefs.cockpit = cockpit) {
                    self.status = e;
                }
            }
            ui.end_row();

            ui.label("Cameras");
            self.camera_row(ui);
            ui.end_row();
        });

        section(ui, "HUD");
        fields(ui, "hud", |ui| {
            ui.label("HUD");
            ui.checkbox(&mut self.hud.show, "shown");
            ui.end_row();

            let tip = "The countdown, false start, wrong way and the finish and result pages always show with the HUD";
            ui.label("HUD parts").on_hover_text(tip);
            ui.add_enabled_ui(self.hud.show, |ui| {
                ui.vertical(|ui| {
                    let parts: Vec<_> = Part::ALL.into_iter().zip(PART_TIPS).collect();
                    for (row, parts) in self.hud.parts.0.chunks_mut(3).zip(parts.chunks(3)) {
                        ui.horizontal(|ui| {
                            for (shown, (part, tip)) in row.iter_mut().zip(parts) {
                                let cell = egui::Layout::left_to_right(egui::Align::Center);
                                ui.allocate_ui_with_layout(egui::vec2(140.0, 20.0), cell, |ui| {
                                    ui.set_min_width(140.0);
                                    ui.checkbox(shown, part.label()).on_hover_text(*tip);
                                });
                            }
                        });
                    }
                });
            });
            ui.end_row();

            let tip = "Needles, lights and displays that follow the car in the cockpit, from the RBRHUD gauges in the \
                       RSF install";
            ui.label("Working dials").on_hover_text(tip);
            ui.checkbox(&mut self.hud.dials, "RSF cars").on_hover_text(tip);
            ui.end_row();

            ui.label("Speed unit");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.hud.mph, false, "km/h");
                ui.radio_value(&mut self.hud.mph, true, "mph");
            });
            ui.end_row();

            let tip = "The next notes stacked at the right of the screen (0: none)";
            ui.label("Shown notes").on_hover_text(tip);
            ui.add(DragValue::new(&mut self.hud.stack).range(0..=STACK_MAX).prefix("stack ")).on_hover_text(tip);
            ui.end_row();
        });

        section(ui, "Sound");
        fields(ui, "sound", |ui| {
            ui.label("Volume").on_hover_text("1 is the game's volume");
            ui.add(egui::Slider::new(&mut self.volume, 0.0..=MAX_VOLUME).max_decimals(2));
            ui.end_row();
            ui.label("Engine").on_hover_text("The engine, the gearbox whine, and an FMOD car's whole sound");
            ui.add(egui::Slider::new(&mut self.engine_volume, 0.0..=MAX_GROUP_VOLUME).max_decimals(2));
            ui.end_row();
            ui.label("Co-driver").on_hover_text("The pace notes, the countdown and the damage calls");
            ui.add(egui::Slider::new(&mut self.codriver_volume, 0.0..=MAX_GROUP_VOLUME).max_decimals(2));
            ui.end_row();
            ui.label("Effects").on_hover_text("The turbo, pops, impacts, weather and the rest");
            ui.add(egui::Slider::new(&mut self.effects_volume, 0.0..=MAX_GROUP_VOLUME).max_decimals(2));
            ui.end_row();
            ui.label("Tyres").on_hover_text("Rolling, rumble and gravel");
            ui.add(egui::Slider::new(&mut self.tyres_volume, 0.0..=MAX_GROUP_VOLUME).max_decimals(2));
            ui.end_row();
        });
    }

    /// The triple-screen rig from the Triple screens section, when triple screens are on (`viewer --triple`).
    pub fn triple_rig(&self) -> Option<Rig> {
        self.triple.then(|| self.rig())
    }

    /// The Triple screens section's rig.
    fn rig(&self) -> Rig {
        Rig {
            diagonal: self.triple_diagonal,
            bezel: self.triple_bezel,
            distance: self.triple_distance,
            angle: self.triple_angle,
        }
    }

    /// The three monitors' names once each is picked (`viewer --triple-monitors`).
    pub fn triple_monitors(&self) -> Option<&[String; 3]> {
        picked(self.triple_monitors.as_ref())
    }

    /// One screen's aspect as the viewer will draw it on the monitor (fullscreen) or the window; 16:9 before the
    /// monitor is known.
    fn screen_aspect(&self) -> f32 {
        match (self.fullscreen, self.monitor) {
            (true, Some(monitor)) => screen_aspect(monitor, self.triple_monitors().is_some()),
            (true, None) => 16.0 / 9.0,
            (false, _) => screen_aspect(self.size, false),
        }
    }

    /// The Triple screens section: the rig's geometry, the monitors, and the true field of view at the seat.
    fn triple_fields(&mut self, ui: &mut egui::Ui) {
        section(ui, "Triple screens");
        fields(ui, "triple", |ui| {
            let tip = "Three like screens side by side, the side ones turned towards you, each drawing its own \
                       view from your seat. The field-of-view sliders set the centre screen's view";
            ui.label("Triple screens").on_hover_text(tip);
            ui.checkbox(&mut self.triple, "three screens").on_hover_text(tip);
            ui.end_row();
            if !self.triple {
                return;
            }
            let tip = "Each screen's diagonal";
            ui.label("Screen size").on_hover_text(tip);
            ui.add(
                egui::Slider::new(&mut self.triple_diagonal, TRIPLE_DIAGONAL_RANGE)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" in"),
            )
            .on_hover_text(tip);
            ui.end_row();

            let tip = "The frame around each screen's image: two of them part neighbouring images";
            ui.label("Bezel").on_hover_text(tip);
            ui.add(
                egui::Slider::new(&mut self.triple_bezel, TRIPLE_BEZEL_RANGE)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" mm"),
            )
            .on_hover_text(tip);
            ui.end_row();

            let tip = "From your eyes to the centre screen";
            ui.label("Distance").on_hover_text(tip);
            ui.add(
                egui::Slider::new(&mut self.triple_distance, TRIPLE_DISTANCE_RANGE)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" cm"),
            )
            .on_hover_text(tip);
            ui.end_row();

            let tip = "How far each side screen is turned towards you from the centre screen's plane. Auto faces \
                       their middles at your eyes at this distance";
            ui.label("Side angle").on_hover_text(tip);
            let aspect = self.screen_aspect();
            let auto_degrees = Rig { angle: None, ..self.rig() }.side_angle(aspect).to_degrees();
            let mut auto = self.triple_angle.is_none();
            let mut degrees = self.triple_angle.unwrap_or(auto_degrees);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut auto, "Auto").on_hover_text(tip).changed() {
                    self.triple_angle = if auto { None } else { Some(degrees) };
                }
                ui.add_enabled_ui(!auto, |ui| {
                    ui.add(egui::Slider::new(&mut degrees, TRIPLE_ANGLE_RANGE).step_by(1.0).suffix("°"));
                });
            });
            if !auto {
                self.triple_angle = Some(degrees);
            }
            ui.end_row();

            let tip = "One borderless fullscreen window a monitor, or one window split in thirds on a spanned \
                       display such as NVIDIA Surround or AMD Eyefinity";
            ui.label("Displays").on_hover_text(tip);
            let names = self.monitors.clone();
            let three = self.triple_monitors.is_some();
            ui.vertical(|ui| {
                if ui.radio(!three, "One wide display (Surround / Eyefinity)").on_hover_text(tip).clicked() {
                    self.triple_monitors = None;
                }
                if ui.radio(three, "Three monitors").on_hover_text(tip).clicked() && self.triple_monitors.is_none() {
                    self.triple_monitors = Some([
                        names.first().cloned().unwrap_or_default(),
                        names.get(1).cloned().unwrap_or_default(),
                        names.get(2).cloned().unwrap_or_default(),
                    ]);
                }
                if let Some(monitors) = &mut self.triple_monitors {
                    egui::Grid::new("triple monitors").show(ui, |ui| {
                        for (slot, side) in monitors.iter_mut().zip(["Left", "Centre", "Right"]) {
                            let mut options = names.clone();
                            if !slot.is_empty() && !options.contains(slot) {
                                options.push(slot.clone());
                            }
                            ui.label(side);
                            ComboBox::from_id_salt(("triple monitor", side))
                                .width(QUALITY_FIELD)
                                .selected_text(if slot.is_empty() { "Pick one" } else { slot.as_str() })
                                .show_ui(ui, |ui| {
                                    for name in &options {
                                        ui.selectable_value(slot, name.clone(), name);
                                    }
                                });
                            ui.end_row();
                        }
                    });
                    if names.len() < 3 {
                        ui.colored_label(WARN, format!("{} of 3 monitors connected", names.len()));
                    }
                }
            });
            ui.end_row();

            let rig = self.rig();
            let tip = "The centre screen's horizontal field of view that shows the world at its true size at \
                       this distance";
            let fov = rig.true_fov(aspect).to_degrees();
            ui.label("True field of view").on_hover_text(tip);
            ui.horizontal(|ui| {
                let mut text = format!("{fov:.0}° at this distance");
                if self.triple_angle.is_none() {
                    let side = rig.side_angle(aspect).to_degrees();
                    text += &format!(", side screens at {side:.0}°");
                }
                ui.label(text).on_hover_text(tip);
                let tip = "Sets the in-car views' field of view below to this, rounded to whole degrees";
                if ui.button("Use for in-car views").on_hover_text(tip).clicked() {
                    self.fov = fov.clamp(*coords::FOV_RANGE_DEGREES.start(), *coords::FOV_RANGE_DEGREES.end()).round();
                }
            });
            ui.end_row();
        });
        if self.triple {
            ui.colored_label(
                Color32::GRAY,
                "The field-of-view sliders set the centre screen's field of view in triple mode.",
            );
        }
    }

    /// The Quality section: the presets, then each choice with its tip on hover.
    fn quality(&mut self, ui: &mut egui::Ui) {
        section(ui, "Quality");
        ui.colored_label(
            Color32::GRAY,
            "Additions beyond the game's look. The stage always draws at its highest quality; sharp distant textures, \
             soft decals and flowing water are always on.",
        );
        ui.add_space(4.0);
        let g = &mut self.graphics;
        fields(ui, "quality", |ui| {
            let tip = "Sets the choices below. Low: the game's look. Medium: all but Lighting, the costliest, Dense \
                       grass, which needs it, and Windscreen. High: all but Ultra detail. Ultra: all. The render scale, tyre marks \
                       and motion blur stay as they are";
            ui.label("Preset").on_hover_text(tip);
            ComboBox::from_id_salt("preset")
                .width(QUALITY_FIELD)
                .selected_text(Quality::of(*g).map_or("Custom", Quality::label))
                .show_ui(ui, |ui| {
                    for preset in Quality::ALL {
                        if ui.selectable_label(Quality::of(*g) == Some(preset), preset.label()).clicked() {
                            *g = preset.over(*g);
                        }
                    }
                })
                .response
                .on_hover_text(tip);
            ui.end_row();

            let tip = "A physically based sky and light for the time of day, with moving volumetric clouds, haze and \
                       mist, sun shafts, real-time sun shadows and sunlit foliage, ambient occlusion and bounced light \
                       worked out from the stage when it loads, and reflection probes. The costliest choice: up to \
                       half the frame time on a dry day, and a few seconds longer to load";
            ui.label("Lighting").on_hover_text(tip);
            ui.checkbox(&mut g.lighting, "sky, sun and shadows").on_hover_text(tip);
            ui.end_row();

            let tip = "GPU rain and snow around the car, rings on wet roads, sunlit and soft particles, wheel spray \
                       and smoke, and the car's dirt. Costly in heavy rain (the mirrors draw it too), next to nothing \
                       in the dry";
            ui.label("Weather effects").on_hover_text(tip);
            ui.checkbox(&mut g.particles, "rain, snow and particles").on_hover_text(tip);
            ui.end_row();

            let tip = "In wet weather, a film of water and puddles on the roads, the ground about them soaked. Low: \
                       they reflect the stage's baked reflections and the sky. Medium and High: also the scenery on \
                       screen, on the wet roads and on rivers and lakes, Medium grainier. Off: the game's wet roads. \
                       Each step up costs more frame time in the rain";
            ui.label("Wet roads").on_hover_text(tip);
            combo(ui, "wet roads", QUALITY_FIELD, &mut g.wet_roads, &WetRoads::ALL, WetRoads::label).on_hover_text(tip);
            ui.end_row();

            let tip = "In the cockpit: the sun only through the glass, with the roof, pillars and dash shading each \
                       other, darker footwells and corners, a sheen on the dash, and the headlights' glow at night";
            ui.label("Cabin light").on_hover_text(tip);
            ui.checkbox(&mut g.cabin, "in the cockpit").on_hover_text(tip);
            ui.end_row();

            let tip = "In the cockpit: the windscreen as real glass, a faint dust film and specks the sun lights up, \
                       wiper scratches and dried water spots, and the dash reflected in it. About 1.5 to 2 ms a frame \
                       (the reflection draws the cabin and the stage behind it once more)";
            ui.label("Windscreen").on_hover_text(tip);
            ui.checkbox(&mut g.windscreen, "glass effects").on_hover_text(tip);
            ui.end_row();

            let tip = "The car's own shape as a soft shadow on the ground and fences under it, in place of the game's \
                       shadow decal";
            ui.label("Car shadow").on_hover_text(tip);
            ui.checkbox(&mut g.contact, "soft").on_hover_text(tip);
            ui.end_row();

            let tip = "A field of grass blades over the grassy ground within 20 m of the road, swaying in the wind, \
                       drawn within 50 m of the camera. Needs Lighting. Costly (the sun's shadows draw it too), and \
                       about 150 MB of video memory";
            ui.label("Dense grass").on_hover_text(tip);
            ui.checkbox(&mut g.grass, "by the road").on_hover_text(tip);
            ui.end_row();

            let tip = "Ultra, for strong GPUs: sun shadows at twice the texels (sharper shadows of posts, cables and \
                       trunks) and sharper reflections in the car's paint and the wet roads. 15 to 40 % more frame \
                       time, and 0.5 GB more video memory on long stages";
            ui.label("Detail").on_hover_text(tip);
            combo(ui, "detail", QUALITY_FIELD, &mut g.detail, &Detail::ALL, Detail::label).on_hover_text(tip);
            ui.end_row();

            let tip = "In the cockpit: the rear-view and door mirrors show the stage, the car and the cars behind, \
                       redrawn this often (each mirror in turn). Costly in the rain, which they draw too. Off: they \
                       only reflect the sky";
            ui.label("Mirrors").on_hover_text(tip);
            combo(ui, "mirrors", QUALITY_FIELD, &mut g.mirrors, &Rate::ALL, mirrors_label).on_hover_text(tip);
            ui.end_row();

            let tip = "Tyre marks as wide as the tread and swept wider in a slide: ruts with berms in gravel, rubber \
                       on tarmac only when sliding, grooves in snow, flattened grass, longer trails. The presets leave \
                       them as they are";
            ui.label("Tyre marks").on_hover_text(tip);
            ui.checkbox(&mut g.marks, "realistic").on_hover_text(tip);
            ui.end_row();

            let tip = "Blurs what moves across the screen over a 1/120 s shutter: the road and the verges at speed, \
                       the scenery in a fast turn of the view. The car and the cockpit stay sharp. About 0.1 ms a \
                       frame at 1080p, 0.4 ms at 4K. The presets leave it as it is";
            ui.label("Motion blur").on_hover_text(tip);
            ui.checkbox(&mut g.motion_blur, "at speed").on_hover_text(tip);
            ui.end_row();
        });
    }

    /// The Cameras row: the driving window's camera keys, and "Reset this car's cameras" (its nudges in the
    /// settings file's `[camera]` section, `rbr_render::camera_prefs`).
    fn camera_row(&mut self, ui: &mut egui::Ui) {
        let Some(car) = self.car() else { return };
        let key = car.key.clone();
        let reset = ui
            .add_enabled(!self.read_only, egui::Button::new("Reset this car's cameras"))
            .on_hover_text("While driving: C changes the view (kept for the next drive); the pause menu's Camera moves it for this car");
        if reset.clicked() {
            let mut had = false;
            let reset = self.edit_camera(|prefs| {
                had = prefs.cars.contains_key(&key);
                prefs.set_nudges(&key, [Default::default(); 4]);
            });
            self.status = match reset {
                Ok(()) if had => format!("{}: cameras reset", car.name),
                Ok(()) => format!("{}: cameras not adjusted", car.name),
                Err(e) => e,
            };
        }
    }

    /// Changes the settings file's `[camera]` section by `change`, the rest of the file kept.
    fn edit_camera(&self, change: impl FnOnce(&mut camera_prefs::CameraPrefs)) -> Result<(), String> {
        let file = std::fs::read_to_string(&self.paths.settings).unwrap_or_default();
        let mut prefs = camera_prefs::CameraPrefs::parse(&file);
        change(&mut prefs);
        std::fs::write(&self.paths.settings, prefs.replace_in(&file))
            .map_err(|e| format!("{}: {e}", self.paths.settings.display()))
    }
}

fn size_label((w, h): (u32, u32)) -> String {
    format!("{w} x {h}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_monitors_go_once_all_three_are_picked() {
        let names = ["DP-1".to_owned(), String::new(), "DP-2".to_owned()];
        assert_eq!(picked(Some(&names)), None);
        let names = ["DP-1".to_owned(), "DP-3".to_owned(), "DP-2".to_owned()];
        assert_eq!(picked(Some(&names)), Some(&names));
    }

    #[test]
    fn a_triple_screen_is_a_third_of_a_spanned_display() {
        let wide = 16.0 / 9.0;
        assert!((screen_aspect((5760, 1080), false) - wide).abs() < 1e-6);
        assert!((screen_aspect((1920, 1080), true) - wide).abs() < 1e-6);
    }
}
