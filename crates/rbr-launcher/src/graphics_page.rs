//! The Graphics & sound page: the window, the camera, the HUD, the sound and the renderer's quality.

use egui::{ComboBox, DragValue};
use rbr_render::camera_prefs;
use rbr_render::coords;
use rbr_render::hud::layout::Layout;
use rbr_render::hud::model::STACK_MAX;
use rbr_render::hud::{Part, Parts};
use rbr_render::mirrors::Rate;
use rbr_render::present::Present;
use rbr_render::renderer::{AntiAliasing, Detail, ReflectionScale, Reflections};

use crate::kit;
use crate::pages::{combo, Choice};
use crate::settings_page::{label_cell, row_tip};
use crate::Form;

/// The Quality section's choices: the renderer's additions (`viewer --enhance`), the wet roads (`--enhance wet` and
/// `--reflections`), the detail (`--detail`), the mirrors (`--mirrors`) and how often and how sharp the reflections
/// update (`--reflection-rate`, `--reflection-res`). Each is kept under its own key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Graphics {
    pub lighting: bool,
    /// Our rain, snow and particles (`particles`).
    pub particles: bool,
    pub grass: Grass,
    /// The windscreen as glass from inside, and the dash reflected in it (`windscreen`).
    pub windscreen: bool,
    /// The motion blur (`blur`), which no preset changes: a taste, off unless asked for.
    pub motion_blur: bool,
    pub wet_roads: WetRoads,
    pub detail: Detail,
    pub mirrors: bool,
    /// How often the dynamic reflections update (`--reflection-rate`).
    pub reflection_rate: Rate,
    /// Their resolution (`--reflection-res`).
    pub reflection_scale: ReflectionScale,
}

/// The additions always on, as they cost next to nothing: 16x anisotropic filtering, the soft decals, the flowing
/// water, the soft car shadow and our tyre marks.
pub const ALWAYS_ON: [&str; 5] = ["anisotropy", "contact", "decals", "marks", "water"];

/// A new install's choices: the Ultra preset.
pub const DEFAULT_GRAPHICS: Graphics = Quality::Ultra.graphics();

impl Default for Graphics {
    /// A new install's choices.
    fn default() -> Self {
        DEFAULT_GRAPHICS
    }
}

/// Our grass by the road (`--enhance grass-light` or `grass`, [`rbr_render::grass::Level`]): none, light (half the
/// clumps, to 100 m) or dense (all of them, to 300 m). Drawn only with the lighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grass {
    Off,
    Light,
    Dense,
}

impl Choice for Grass {
    const ALL: &[Self] = &[Grass::Dense, Grass::Light, Grass::Off];
    fn name(self) -> &'static str {
        match self {
            Grass::Off => "off",
            Grass::Light => "light",
            Grass::Dense => "dense",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Grass::Off => "Off",
            Grass::Light => "Light",
            Grass::Dense => "Dense",
        }
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
            Detail::High => "Off",
            Detail::Ultra => "Sharper shadows",
        }
    }
}

/// The Quality section's presets (their cost: `notes/renderer.md` "Cost on an M3 Pro").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// The game's look with the free additions.
    Low,
    /// Our lighting, light grass and every addition but the windscreen's glass; the wet roads without the scenery's
    /// reflections, the reflections updated every third frame at half size.
    Medium,
    High,
    /// High with the Ultra detail and dense grass.
    Ultra,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Low, Quality::Medium, Quality::High, Quality::Ultra];

    /// Its choices.
    pub const fn graphics(self) -> Graphics {
        let high = Graphics {
            lighting: true,
            particles: true,
            grass: Grass::Light,
            windscreen: true,
            motion_blur: false,
            wet_roads: WetRoads::High,
            detail: Detail::High,
            mirrors: true,
            reflection_rate: Rate::ALL[1],
            reflection_scale: ReflectionScale::ThreeQuarters,
        };
        match self {
            Quality::Low => Graphics {
                lighting: false,
                particles: false,
                grass: Grass::Off,
                windscreen: false,
                wet_roads: WetRoads::Off,
                mirrors: false,
                reflection_rate: Rate::ALL[2],
                reflection_scale: ReflectionScale::Half,
                ..high
            },
            Quality::Medium => Graphics {
                windscreen: false,
                wet_roads: WetRoads::Low,
                reflection_rate: Rate::ALL[2],
                reflection_scale: ReflectionScale::Half,
                ..high
            },
            Quality::High => high,
            Quality::Ultra => Graphics {
                detail: Detail::Ultra,
                grass: Grass::Dense,
                reflection_rate: Rate::ALL[0],
                reflection_scale: ReflectionScale::Full,
                ..high
            },
        }
    }

    /// Its choices over `kept`, whose motion blur stays.
    pub const fn over(self, kept: Graphics) -> Graphics {
        Graphics { motion_blur: kept.motion_blur, ..self.graphics() }
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

/// The render scale dropdown's choices: each scale with its anti-aliasing (`viewer --anti-aliasing`). Below 100
/// the temporal anti-aliasing must stay on to upscale; at 100 it either resolves the jittered frame or draws each
/// pixel as it is.
pub const RENDER_CHOICES: [(u32, AntiAliasing); 6] = [
    (100, AntiAliasing::Taa),
    (100, AntiAliasing::Off),
    (87, AntiAliasing::Taa),
    (75, AntiAliasing::Taa),
    (67, AntiAliasing::Taa),
    (50, AntiAliasing::Taa),
];

/// A render scale choice's label.
fn scale_label((percent, anti_aliasing): (u32, AntiAliasing)) -> String {
    match (percent, anti_aliasing) {
        (100, AntiAliasing::Taa) => "100% (TAA)".into(),
        (100, AntiAliasing::Off) => "100% (no TAA)".into(),
        (percent, _) => format!("{percent}%"),
    }
}

/// A reflection rate's label.
fn rate_label(rate: Rate) -> &'static str {
    match rate.every() {
        1 => "Every frame",
        2 => "Every 2nd frame",
        _ => "Every 3rd frame",
    }
}

/// A reflection resolution's label.
fn reflection_scale_label(scale: ReflectionScale) -> &'static str {
    match scale {
        ReflectionScale::Half => "50 %",
        ReflectionScale::ThreeQuarters => "75 %",
        ReflectionScale::Full => "100 %",
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

/// A whole-number slider with its unit `suffix` and hover `tip`.
pub(crate) fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    tip: &str,
) {
    ui.spacing_mut().slider_width = 180.0;
    ui.add(egui::Slider::new(value, range).step_by(1.0).fixed_decimals(0).suffix(suffix)).on_hover_text(tip);
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

            let tip = "Draws the 3D view at this share of the window's size, which the temporal anti-aliasing \
                       upscales to it over the frames: faster, for high refresh rates at 1440p and above; the lower, \
                       the softer, and fast-moving edges grow rougher. 100% (no TAA) draws each pixel as it is: the \
                       sharpest stills and cheapest frames, but jagged edges, shimmering cut-outs and no motion blur";
            let mut choice = (self.render_scale, self.anti_aliasing);
            rows.row("Render scale", |ui| {
                combo(ui, "render scale", None, &mut choice, &RENDER_CHOICES, scale_label).on_hover_text(tip);
            });
            (self.render_scale, self.anti_aliasing) = choice;

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
                rows.disabled_if(true, |rows| {
                    rows.switch_row(
                        "Upscaled textures",
                        Some("Not available on this platform"),
                        &mut self.upscale_textures,
                    )
                    .on_disabled_hover_text(tip)
                });
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

        self.screen_fields(ui);

        ui.add_space(14.0);
        kit::group_label(ui, "Camera", None);
        kit::panel(ui, |rows| {
            let tip = "The horizontal field of view of the in-car views and the free camera; the chase view has its \
                       own below. The pause menu's Camera page sets it too";
            match self.matched_fov() {
                Some(fov) => {
                    let tip = "Matched to your screen (Screen above): pick Manual there to set it here. Changing it \
                               on the pause menu's Camera page also switches to Manual";
                    let mut shown = fov.round();
                    rows.row("Field of view", |ui| {
                        ui.add_enabled_ui(false, |ui| slider(ui, &mut shown, coords::FOV_RANGE_DEGREES, "°", tip));
                    });
                }
                None => rows.row("Field of view", |ui| slider(ui, &mut self.fov, coords::FOV_RANGE_DEGREES, "°", tip)),
            }

            let tip = "The horizontal field of view of the chase view (65 by default). The pause menu's Camera page \
                       sets it too";
            rows.row("Chase field of view", |ui| slider(ui, &mut self.chase_fov, coords::FOV_RANGE_DEGREES, "°", tip));

            let mut cockpit = self.cockpit;
            let tip = "The in-car view's head: it keeps the horizon steadier over bumps. Off, the view is fixed to \
                       the car. The pause menu's Camera page sets it too";
            let read_only = self.read_only;
            rows.disabled_if(read_only, |rows| {
                rows.switch_row("Head motion in the car", None, &mut cockpit.head).on_hover_text(tip)
            });

            let tip = "What the cockpit view shows: the driver's arms on the wheel, the co-driver beside you, and the \
                       car's steering wheel (leave it out when driving with a wheel of your own). The pause menu's \
                       Camera page sets them too";
            rows.disabled_if(read_only, |rows| {
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

    /// The Quality section: the presets, then each choice with its tip on hover.
    fn quality(&mut self, ui: &mut egui::Ui) {
        kit::group_label(ui, "Quality", None);
        let g = &mut self.graphics;
        kit::panel(ui, |rows| {
            let tip = "Sets the choices below. Low: the game's look. Medium: all but Windscreen, with light grass, \
                       wet roads at low, the reflections every third frame at 50 %. High: all but Sharper shadows, \
                       with light grass, the reflections every 2nd frame at 75 %. Ultra: all, with dense grass, \
                       the reflections every frame at 100 %. The render scale and motion blur \
                       stay as they are";
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
                       worked out from the stage when it loads, and reflection probes. In the cockpit, the sun only through \
                       the glass, the roof, pillars and dash shading each other, darker footwells and corners, a sheen \
                       on the dash, and the headlights' glow at night. The costliest choice: up to \
                       half the frame time on a dry day, and a few seconds longer to load";
            rows.switch_row("Sky, sun, shadows and cockpit light", None, &mut g.lighting).on_hover_text(tip);

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

            let tip = "In the cockpit: the windscreen as real glass, a faint dust film and specks the sun lights up, \
                       wiper scratches and dried water spots, and the dash reflected in it. About 1.5 to 2 ms a frame \
                       (the reflection draws the cabin and the stage behind it once more)";
            rows.switch_row("Windscreen glass effects", None, &mut g.windscreen).on_hover_text(tip);

            let tip = "A field of grass blades over the grassy ground within 40 m of the road, swaying in the wind, \
                       made on the graphics card every frame. Light: half the blades, as thick to about 40 m of the \
                       camera and fading out by 100 m; about 0.2 GB of video memory. Dense: all of them, as thick to \
                       about 200 m and fading out by 300 m; about 0.25 GB. Needs Sky, sun and shadows. Costly, Dense \
                       the most";
            let lit = g.lighting;
            rows.row("Grass by the road", |ui| {
                ui.add_enabled_ui(lit, |ui| combo(ui, "grass", None, &mut g.grass, Grass::ALL, Grass::label)).inner
            })
            .on_hover_text(tip)
            .on_disabled_hover_text(tip);

            let tip = "For strong GPUs: sun shadows at twice the texels (sharper shadows of posts, cables and \
                       trunks). Needs Sky, sun and shadows. Some more frame time, and video memory on long stages";
            let mut sharp = g.detail == Detail::Ultra;
            rows.switch_row("Sharper shadows", None, &mut sharp).on_hover_text(tip);
            g.detail = if sharp { Detail::Ultra } else { Detail::High };

            let tip = "In the cockpit: the rear-view and door mirrors show the stage, the car and the cars behind. \
                       Costly in the rain, which they draw too. Off: they only reflect the sky. How often and how \
                       sharp: the two choices below";
            rows.switch_row("Mirrors", None, &mut g.mirrors).on_hover_text(tip);

            let tip = "How often the reflections update: the mirrors (each in turn), the windscreen's (at most every \
                       2nd frame) and the surroundings the car's paint and glass reflect. Every frame is the \
                       smoothest and costs the most";
            rows.row("Reflection updates", |ui| {
                combo(ui, "reflection updates", None, &mut g.reflection_rate, &Rate::ALL, rate_label)
                    .on_hover_text(tip);
            });

            let tip = "How sharp the reflections are: the mirrors' and the windscreen's images, the surroundings in \
                       the car's paint and glass (a cube of 128, 192 or 256 texels), and at 100 % the wet roads' \
                       reflections at full size (50 and 75 %: half). At 100 % the cockpit's bonnet and chrome mirror \
                       the surroundings as the outside view does. Costs frame time and video memory";
            rows.row("Reflection resolution", |ui| {
                combo(
                    ui,
                    "reflection resolution",
                    None,
                    &mut g.reflection_scale,
                    &ReflectionScale::ALL,
                    reflection_scale_label,
                )
                .on_hover_text(tip);
            });

            let tip = "Blurs what moves across the screen over a 1/120 s shutter: the road and the verges at speed, \
                       the scenery in a fast turn of the view. The car and the cockpit stay sharp. About 0.1 ms a \
                       frame at 1080p, 0.4 ms at 4K. The presets leave it as it is";
            let taa = self.anti_aliasing == AntiAliasing::Taa;
            rows.disabled_if(!taa, |rows| {
                rows.switch_row("Motion blur at speed", None, &mut g.motion_blur).on_hover_text(tip)
            })
            .on_disabled_hover_text(
                "Needs the temporal anti-aliasing (render scale 100% (TAA)): without its \
                                       history there is nothing to gather",
            );
        });
        ui.add_space(4.0);
        kit::hint(
            ui,
            "Additions beyond the game's look. The stage always draws at its highest quality; sharp distant textures, \
             soft decals, flowing water, the soft car shadow and realistic tyre marks are always on.",
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
