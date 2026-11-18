//! The Graphics & sound page: the window, the camera, the HUD, the sound and the renderer's quality.

use egui::{ComboBox, DragValue};
use rbr_render::camera_prefs;
use rbr_render::coords;
use rbr_render::hud::layout::Layout;
use rbr_render::hud::model::STACK_MAX;
use rbr_render::hud::{Part, Parts};
use rbr_render::mirrors::Rate;
use rbr_render::present::Present;
use rbr_render::renderer::{Detail, Reflections};
use rbr_render::triple::Rig;

use crate::kit;
use crate::pages::{combo, Choice};
use crate::settings_page::{label_cell, note, row_tip};
use crate::theme::WARN;
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
    /// The Graphics & sound page: the window, the camera and the HUD on the left; the renderer's quality and the sound
    /// on the right.
    pub fn graphics_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("graphics").auto_shrink([false, false]).show(ui, |ui| {
            kit::body(ui, |ui| {
                ui.columns(2, |columns| {
                    self.view_fields(&mut columns[0]);
                    self.quality(&mut columns[1]);
                    columns[1].add_space(14.0);
                    self.sound(&mut columns[1]);
                });
            })
        });
    }

    fn view_fields(&mut self, ui: &mut egui::Ui) {
        kit::group_label(ui, "Display", None);
        let logs = self.logs_dir().display().to_string();
        let can_upscale = crate::upscaler::available();
        kit::panel(ui, |rows| {
            let tip =
                "Fills the monitor at its own resolution, borderless: the compositor can show the frames directly";
            rows.switch_row("Fullscreen at the monitor's resolution", None, &mut self.fullscreen).on_hover_text(tip);

            let fullscreen = self.fullscreen;
            row_tip(rows, "Resolution", "The driving window's size when not fullscreen", |ui| {
                ui.add_enabled_ui(!fullscreen, |ui| {
                    ComboBox::from_id_salt("size").height(320.0).selected_text(size_label(self.size)).show_ui(
                        ui,
                        |ui| {
                            for &size in &self.sizes {
                                ui.selectable_value(&mut self.size, size, size_label(size));
                            }
                        },
                    );
                });
            });

            let tip = "Three like screens side by side, the side ones turned towards you, each drawing its own \
                       view from your seat. The field-of-view sliders set the centre screen's view";
            rows.switch_row("Triple screens", None, &mut self.triple).on_hover_text(tip);

            let tip = "Draws the 3D view at this share of the window's size, which the temporal anti-aliasing \
                       upscales to it over the frames: faster, for high refresh rates at 1440p and above; the lower, \
                       the softer, and fast-moving edges grow rougher";
            rows.row("Render scale", |ui| {
                combo(ui, "render scale", None, &mut self.render_scale, &RENDER_SCALES, scale_label).on_hover_text(tip);
            });

            let tip = if self.upscaler {
                "Upscales the car's and the stage's textures 4x (to at most 4096 texels a side), so the bonnet, the \
                 livery, the cockpit, the gravel and the signs stay sharp up close. The first drive with \
                 a car or on a stage waits while they are made; then they are kept and load at once"
            } else if can_upscale {
                "Needs the upscaler, about 50 MB: Download upscaler… gets it"
            } else {
                "Needs the upscaler, which Real-ESRGAN does not build for this platform"
            };
            if self.upscaler {
                rows.switch_row("Upscaled textures", None, &mut self.upscale_textures).on_hover_text(tip);
                if self.upscaler_cache_shown() {
                    rows.full(|ui| ui.horizontal_wrapped(|ui| self.upscaler_cache_ui(ui)).inner);
                }
            } else if can_upscale {
                rows.full(|ui| {
                    label_cell(ui, 130.0, "Upscaled textures").on_hover_text(tip);
                    ui.vertical(|ui| self.upscaler_download_ui(ui));
                });
            } else {
                let mut off = false;
                rows.switch_row("Upscaled textures", Some("Not available on this platform"), &mut off)
                    .on_hover_text(tip);
            }

            let (label, tip) = present_label(self.present);
            row_tip(rows, "Frame rate", "How frames reach the display, and the most frames a second drawn", |ui| {
                let tip_cap = "The most frames a second the driving window draws. Uncapped with the display's refresh \
                               rate here keeps the lowest lag without the GPU drawing frames the display never shows";
                combo(ui, "frame cap", None, &mut self.max_fps, &FRAME_CAPS, |fps| match fps {
                    0 => "No cap".into(),
                    fps => cap_label(fps),
                })
                .on_hover_text(tip_cap);
                ComboBox::from_id_salt("present")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for &present in Present::OFFERED {
                            let (label, tip) = present_label(present);
                            ui.selectable_value(&mut self.present, present, label).on_hover_text(tip);
                        }
                    })
                    .response
                    .on_hover_text(tip);
            });

            if rbr_render::present::HDR_OFFERED {
                let tip = "On an HDR display (a Mac's extended dynamic range) the sun, the lamps and their glints \
                           show brighter than its white, as far as it reaches at its brightness; the rest of the \
                           frame and the HUD as without. Elsewhere the window draws as without it";
                rows.switch_row("HDR", Some("Highlights past the display's white"), &mut self.hdr).on_hover_text(tip);
            }

            rows.switch_row("FPS counter, top right", None, &mut self.hud.fps);

            let tip =
                "Writes how long each frame, each stage on the CPU and each pass on the GPU takes, once a second \
                       (a .log to paste in a bug report) and the last 600 frames at the end (a .trace.json for \
                       ui.perfetto.dev). Costs well under 0.2 ms a frame";
            let hint = self.perf_log.then(|| format!("In {logs}"));
            rows.switch_row("Performance log for each drive", hint.as_deref(), &mut self.perf_log).on_hover_text(tip);
        });

        self.triple_fields(ui);

        ui.add_space(14.0);
        kit::group_label(ui, "Camera", None);
        kit::panel(ui, |rows| {
            let tip = "The horizontal field of view of the in-car views and the free camera; the chase view has its \
                       own below. The pause menu's Camera page sets it too";
            rows.row("Field of view", |ui| {
                ui.spacing_mut().slider_width = 180.0;
                ui.add(
                    egui::Slider::new(&mut self.fov, coords::FOV_RANGE_DEGREES)
                        .step_by(1.0)
                        .fixed_decimals(0)
                        .suffix("°"),
                )
                .on_hover_text(tip);
            });

            let tip = "The horizontal field of view of the chase view (65 by default). The pause menu's Camera page \
                       sets it too";
            rows.row("Chase field of view", |ui| {
                ui.spacing_mut().slider_width = 180.0;
                ui.add(
                    egui::Slider::new(&mut self.chase_fov, coords::FOV_RANGE_DEGREES)
                        .step_by(1.0)
                        .fixed_decimals(0)
                        .suffix("°"),
                )
                .on_hover_text(tip);
            });

            let mut cockpit = self.cockpit;
            let tip = "The in-car view's head: it keeps the horizon steadier over bumps. Off, the view is fixed to \
                       the car. The pause menu's Camera page sets it too";
            rows.switch_row("Head motion in the car", None, &mut cockpit.head).on_hover_text(tip);

            let tip = "What the cockpit view shows: the driver's arms on the wheel, the co-driver beside you, and the \
                       car's steering wheel (leave it out when driving with a wheel of your own). The pause menu's \
                       Camera page sets them too";
            rows.full(|ui| {
                label_cell(ui, 110.0, "Cockpit shows").on_hover_text(tip);
                ui.horizontal_wrapped(|ui| {
                    let choices = [
                        (&mut cockpit.driver, "Driver's arms"),
                        (&mut cockpit.codriver, "Co-driver"),
                        (&mut cockpit.wheel, "Steering wheel"),
                    ];
                    for (on, label) in choices {
                        ui.checkbox(on, label).on_hover_text(tip);
                    }
                });
            });
            if self.read_only {
                cockpit = self.cockpit;
            }
            if cockpit != self.cockpit {
                self.cockpit = cockpit;
                if let Err(e) = self.edit_camera(|prefs| prefs.cockpit = cockpit) {
                    self.status = e;
                }
            }

            rows.row("Cameras", |ui| self.camera_row(ui));
        });

        ui.add_space(14.0);
        kit::group_label(ui, "HUD", None);
        kit::panel(ui, |rows| {
            rows.switch_row("HUD", Some("Shown while driving"), &mut self.hud.show);

            let tip = "The countdown, false start, wrong way and the finish and result pages always show with the HUD";
            let show = self.hud.show;
            rows.full(|ui| {
                label_cell(ui, 84.0, "Parts").on_hover_text(tip);
                ui.add_enabled_ui(show, |ui| {
                    ui.vertical(|ui| {
                        let parts: Vec<_> = Part::ALL.into_iter().zip(PART_TIPS).collect();
                        for (row, parts) in self.hud.parts.0.chunks_mut(2).zip(parts.chunks(2)) {
                            ui.horizontal(|ui| {
                                for (shown, (part, tip)) in row.iter_mut().zip(parts) {
                                    let cell = egui::Layout::left_to_right(egui::Align::Center);
                                    ui.allocate_ui_with_layout(egui::vec2(150.0, 24.0), cell, |ui| {
                                        ui.set_min_width(150.0);
                                        ui.checkbox(shown, part.label()).on_hover_text(*tip);
                                    });
                                }
                            });
                        }
                    });
                });
            });

            let tip = "Needles, lights and displays that follow the car in the cockpit, from the RBRHUD gauges in the \
                       RSF install";
            rows.switch_row("Working dials", Some("RSF cars"), &mut self.hud.dials).on_hover_text(tip);

            rows.row("Speed unit", |ui| {
                kit::segmented(ui, "speed unit", &mut self.hud.mph, &[(false, "km/h"), (true, "mph")]);
            });

            let tip = "The next notes stacked at the right of the screen (0: none)";
            rows.row("Shown notes", |ui| {
                ui.add(DragValue::new(&mut self.hud.stack).range(0..=STACK_MAX).prefix("stack ")).on_hover_text(tip);
            });
        });
    }

    /// The Sound group: the master volume and each group's, 1 being the game's.
    fn sound(&mut self, ui: &mut egui::Ui) {
        kit::group_label(ui, "Sound", None);
        kit::panel(ui, |rows| {
            let list = [
                ("Volume", "1 is the game's volume", &mut self.volume, MAX_VOLUME),
                (
                    "Engine",
                    "The engine, the gearbox whine, and an FMOD car's whole sound",
                    &mut self.engine_volume,
                    MAX_GROUP_VOLUME,
                ),
                (
                    "Co-driver",
                    "The pace notes, the countdown and the damage calls",
                    &mut self.codriver_volume,
                    MAX_GROUP_VOLUME,
                ),
                (
                    "Effects",
                    "The turbo, pops, impacts, weather and the rest",
                    &mut self.effects_volume,
                    MAX_GROUP_VOLUME,
                ),
                ("Tyres", "Rolling, rumble and gravel", &mut self.tyres_volume, MAX_GROUP_VOLUME),
            ];
            for (label, tip, value, max) in list {
                row_tip(rows, label, tip, |ui| {
                    ui.spacing_mut().slider_width = 220.0;
                    ui.add(egui::Slider::new(value, 0.0..=max).max_decimals(2)).on_hover_text(tip);
                });
            }
        });
        ui.add_space(4.0);
        kit::hint(ui, "1 is the game's volume. Volume goes up to 4, each group up to 2.");
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

    /// The Triple screens section, shown while they are on (the switch is in Display): the rig's geometry, the
    /// monitors, and the true field of view at the seat.
    fn triple_fields(&mut self, ui: &mut egui::Ui) {
        if !self.triple {
            return;
        }
        ui.add_space(14.0);
        kit::group_label(ui, "Triple screens", None);
        let aspect = self.screen_aspect();
        let rig = self.rig();
        let fov = rig.true_fov(aspect).to_degrees();
        let auto_degrees = Rig { angle: None, ..rig }.side_angle(aspect).to_degrees();
        let mut text = format!("{fov:.0}° at this distance");
        if self.triple_angle.is_none() {
            text += &format!(", side screens at {:.0}°", rig.side_angle(aspect).to_degrees());
        }
        let names = self.monitors.clone();
        kit::panel(ui, |rows| {
            let slider = |ui: &mut egui::Ui, value: &mut f32, range, suffix: &str, tip: &str| {
                ui.spacing_mut().slider_width = 180.0;
                ui.add(egui::Slider::new(value, range).step_by(1.0).fixed_decimals(0).suffix(suffix))
                    .on_hover_text(tip);
            };
            rows.row("Screen size", |ui| {
                slider(ui, &mut self.triple_diagonal, TRIPLE_DIAGONAL_RANGE, " in", "Each screen's diagonal")
            });
            rows.row("Bezel", |ui| {
                let tip = "The frame around each screen's image: two of them part neighbouring images";
                slider(ui, &mut self.triple_bezel, TRIPLE_BEZEL_RANGE, " mm", tip)
            });
            rows.row("Distance", |ui| {
                let tip = "From your eyes to the centre screen";
                slider(ui, &mut self.triple_distance, TRIPLE_DISTANCE_RANGE, " cm", tip)
            });

            let tip = "How far each side screen is turned towards you from the centre screen's plane. Auto faces \
                       their middles at your eyes at this distance";
            let mut auto = self.triple_angle.is_none();
            let mut degrees = self.triple_angle.unwrap_or(auto_degrees);
            rows.row("Side angle", |ui| {
                ui.add_enabled_ui(!auto, |ui| {
                    slider(ui, &mut degrees, TRIPLE_ANGLE_RANGE, "°", tip);
                });
                if ui.checkbox(&mut auto, "Auto").on_hover_text(tip).changed() {
                    self.triple_angle = if auto { None } else { Some(degrees) };
                }
            });
            if !auto {
                self.triple_angle = Some(degrees);
            }

            let tip = "One borderless fullscreen window a monitor, or one window split in thirds on a spanned \
                       display such as NVIDIA Surround or AMD Eyefinity";
            let mut three = self.triple_monitors.is_some();
            rows.row("Displays", |ui| {
                let options = [(false, "One wide display (Surround / Eyefinity)"), (true, "Three monitors")];
                if kit::segmented(ui, "triple displays", &mut three, &options).on_hover_text(tip).changed() {
                    self.triple_monitors = three.then(|| {
                        [
                            names.first().cloned().unwrap_or_default(),
                            names.get(1).cloned().unwrap_or_default(),
                            names.get(2).cloned().unwrap_or_default(),
                        ]
                    });
                }
            });
            if let Some(monitors) = &mut self.triple_monitors {
                for (slot, side) in monitors.iter_mut().zip(["Left", "Centre", "Right"]) {
                    let mut options = names.clone();
                    if !slot.is_empty() && !options.contains(slot) {
                        options.push(slot.clone());
                    }
                    rows.row(side, |ui| {
                        ComboBox::from_id_salt(("triple monitor", side))
                            .selected_text(if slot.is_empty() { "Pick one" } else { slot.as_str() })
                            .show_ui(ui, |ui| {
                                for name in &options {
                                    ui.selectable_value(slot, name.clone(), name);
                                }
                            });
                    });
                }
                if names.len() < 3 {
                    note(rows, WARN, &format!("{} of 3 monitors connected", names.len()));
                }
            }

            let tip = "The centre screen's horizontal field of view that shows the world at its true size at \
                       this distance";
            row_tip(rows, "True field of view", tip, |ui| {
                let tip = "Sets the in-car views' field of view below to this, rounded to whole degrees";
                if ui.button("Use for in-car views").on_hover_text(tip).clicked() {
                    self.fov = fov.clamp(*coords::FOV_RANGE_DEGREES.start(), *coords::FOV_RANGE_DEGREES.end()).round();
                }
            });
            rows.hint(&text);
        });
        ui.add_space(4.0);
        kit::hint(ui, "The field-of-view sliders set the centre screen's field of view in triple mode.");
    }

    /// The Quality section: the presets, then each choice with its tip on hover.
    fn quality(&mut self, ui: &mut egui::Ui) {
        kit::group_label(ui, "Quality", None);
        let g = &mut self.graphics;
        kit::panel(ui, |rows| {
            let tip = "Sets the choices below. Low: the game's look. Medium: all but Lighting, the costliest, Dense \
                       grass, which needs it, and Windscreen. High: all but Ultra detail. Ultra: all. The render scale, tyre marks \
                       and motion blur stay as they are";
            rows.row("Preset", |ui| {
                ComboBox::from_id_salt("preset")
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
            });

            let tip = "A physically based sky and light for the time of day, with moving volumetric clouds, haze and \
                       mist, sun shafts, real-time sun shadows and sunlit foliage, ambient occlusion and bounced light \
                       worked out from the stage when it loads, and reflection probes. The costliest choice: up to \
                       half the frame time on a dry day, and a few seconds longer to load";
            rows.switch_row("Sky, sun and shadows", None, &mut g.lighting).on_hover_text(tip);

            let tip = "GPU rain and snow around the car, rings on wet roads, sunlit and soft particles, wheel spray \
                       and smoke, and the car's dirt. Costly in heavy rain (the mirrors draw it too), next to nothing \
                       in the dry";
            rows.switch_row("Rain, snow and particles", None, &mut g.particles).on_hover_text(tip);

            let tip = "In wet weather, a film of water and puddles on the roads, the ground about them soaked. Low: \
                       they reflect the stage's baked reflections and the sky. Medium and High: also the scenery on \
                       screen, on the wet roads and on rivers and lakes, Medium grainier. Off: the game's wet roads. \
                       Each step up costs more frame time in the rain";
            rows.row("Wet roads", |ui| {
                combo(ui, "wet roads", None, &mut g.wet_roads, &WetRoads::ALL, WetRoads::label).on_hover_text(tip);
            });

            let tip = "In the cockpit: the sun only through the glass, with the roof, pillars and dash shading each \
                       other, darker footwells and corners, a sheen on the dash, and the headlights' glow at night";
            rows.switch_row("Cockpit light", None, &mut g.cabin).on_hover_text(tip);

            let tip = "In the cockpit: the windscreen as real glass, a faint dust film and specks the sun lights up, \
                       wiper scratches and dried water spots, and the dash reflected in it. About 1.5 to 2 ms a frame \
                       (the reflection draws the cabin and the stage behind it once more)";
            rows.switch_row("Windscreen glass effects", None, &mut g.windscreen).on_hover_text(tip);

            let tip = "The car's own shape as a soft shadow on the ground and fences under it, in place of the game's \
                       shadow decal";
            rows.switch_row("Soft car shadow", None, &mut g.contact).on_hover_text(tip);

            let tip = "A field of grass blades over the grassy ground within 20 m of the road, swaying in the wind, \
                       drawn within 50 m of the camera. Needs Lighting. Costly (the sun's shadows draw it too), and \
                       about 150 MB of video memory";
            rows.switch_row("Dense grass by the road", None, &mut g.grass).on_hover_text(tip);

            let tip = "Ultra, for strong GPUs: sun shadows at twice the texels (sharper shadows of posts, cables and \
                       trunks) and sharper reflections in the car's paint and the wet roads. 15 to 40 % more frame \
                       time, and 0.5 GB more video memory on long stages";
            rows.row("Detail", |ui| {
                combo(ui, "detail", None, &mut g.detail, &Detail::ALL, Detail::label).on_hover_text(tip);
            });

            let tip = "In the cockpit: the rear-view and door mirrors show the stage, the car and the cars behind, \
                       redrawn this often (each mirror in turn). Costly in the rain, which they draw too. Off: they \
                       only reflect the sky";
            rows.row("Mirrors", |ui| {
                combo(ui, "mirrors", None, &mut g.mirrors, &Rate::ALL, mirrors_label).on_hover_text(tip);
            });

            let tip = "Tyre marks as wide as the tread and swept wider in a slide: ruts with berms in gravel, rubber \
                       on tarmac only when sliding, grooves in snow, flattened grass, longer trails. The presets leave \
                       them as they are";
            rows.switch_row("Realistic tyre marks", None, &mut g.marks).on_hover_text(tip);

            let tip = "Blurs what moves across the screen over a 1/120 s shutter: the road and the verges at speed, \
                       the scenery in a fast turn of the view. The car and the cockpit stay sharp. About 0.1 ms a \
                       frame at 1080p, 0.4 ms at 4K. The presets leave it as it is";
            rows.switch_row("Motion blur at speed", None, &mut g.motion_blur).on_hover_text(tip);
        });
        ui.add_space(4.0);
        kit::hint(
            ui,
            "Additions beyond the game's look. The stage always draws at its highest quality; sharp distant textures, \
             soft decals and flowing water are always on.",
        );
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
