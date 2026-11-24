//! The Controls page: the wheel, pedals and shifter first, then the wheel's other buttons and its force feedback, and
//! the keyboard's keys last, collapsed. The launcher opens on it when a wheel is connected and nothing drives from one
//! yet (ours).
//!
//! A binding is set by moving the input on a joystick (`rbr_render::devices`, evdev on Linux): the first button
//! pressed or axis moved from where it was when the capture began (the steering needs a tenth of the axis' range,
//! the rest a quarter). The steering takes an axis only: turning left binds both steering actions to it. Each input
//! keeps the device it came from. A pedal or the steering moving the other way from the game's (a pedal resting at
//! the other end) sets its channel's invert (`input.ini`'s, `Bindings::invert`).

use std::path::Path;
use std::time::{Duration, Instant};

use egui::{Align, ComboBox, DragValue, Layout, RichText, Ui};
use rbr_formats::car_pack::SLOT;
use rbr_formats::rsf::settings::{personal, Sources};
use rbr_formats::saves::action_map::Object;
use rbr_gameplay::poll::Invert;
use rbr_gameplay::race_options::DriverAids;
use rbr_render::bindings::{self, Action, Bindings, Device, PadInput};
use rbr_render::devices::{values, Joysticks};
use rbr_render::ffb;
use rbr_render::hud::ffb_page::SMOOTHING_HELP;
use rbr_render::soft_lock::SoftLock;
use rbr_render::wheel_rotation::{self, Wheel};
use winit::keyboard::KeyCode;

use crate::catalog::Car;
use crate::kit::{self, Rows};
use crate::settings_page::{label_cell, row_tip};
use crate::theme::{medium, mono, GOOD, INPUT, LINK, PROGRESS, TEXT, TEXT_2, TEXT_3, TEXT_LABEL, WARN};

/// A binding being set: its action, and for a device input every joystick's values when it began (`None` for an axis
/// that had not reported yet: it rests where it first does, [`Joysticks::first`]).
enum Capture {
    Key(Action),
    Pad(Action, Vec<Vec<(Object, Option<f32>)>>),
}

/// The wheel and pedals rows.
const WHEEL: [(Action, &str); 5] = [
    (Action::SteerLeft, "Steering"),
    (Action::Throttle, "Throttle"),
    (Action::Brake, "Brake"),
    (Action::Clutch, "Clutch"),
    (Action::Handbrake, "Handbrake"),
];

/// The shifter rows.
const SHIFTER: [(Action, &str); 9] = [
    (Action::GearUp, "Shift up"),
    (Action::GearDown, "Shift down"),
    (Action::Reverse, "Reverse"),
    (Action::Gear1, "1st"),
    (Action::Gear2, "2nd"),
    (Action::Gear3, "3rd"),
    (Action::Gear4, "4th"),
    (Action::Gear5, "5th"),
    (Action::Gear6, "6th"),
];

/// How often the joysticks are looked for again while nothing is being set (a wheel plugged in).
const RESCAN: Duration = Duration::from_secs(2);

/// The force feedback: on or off, the strength, the direction, the smoothing and NGP's sensitivity per surface. The
/// strength and the smoothing are written to the settings file at once, as the pause
/// menu's Force feedback page does while driving.
fn ffb_ui(rows: &mut Rows, ffb: &mut ffb::Options, settings: &Path, read_only: bool) {
    rows.switch_row("Force feedback", None, &mut ffb.settings.enabled);
    let on = ffb.settings.enabled;
    rows.row("Strength", |ui| {
        ui.add_enabled_ui(on, |ui| {
            let strength =
                ui.add(DragValue::new(&mut ffb.settings.strength).range(0.0..=1.0).speed(0.01).fixed_decimals(2));
            if strength.on_hover_text(STRENGTH_HELP).changed() && !read_only {
                write_ffb(settings, "strength", &ffb.settings.strength.to_string());
            }
        });
    });
    rows.disabled_if(!on, |rows| rows.switch_row("Inverse", None, &mut ffb.settings.inverse));
    rows.row("Smoothing", |ui| {
        ui.add_enabled_ui(on, |ui| {
            // "off" at 0, else the milliseconds; typed in with or without the unit.
            let smoothing = DragValue::new(&mut ffb.smoothing)
                .range(ffb::SMOOTHING_RANGE)
                .custom_formatter(|ms, _| if ms == 0.0 { "off".into() } else { format!("{ms} ms") })
                .custom_parser(|text| match text.trim().trim_end_matches("ms").trim() {
                    "off" => Some(0.0),
                    ms => ms.parse().ok(),
                });
            if ui.add(smoothing).on_hover_text(SMOOTHING_HELP).changed() && !read_only {
                write_ffb(settings, "smoothing", &ffb.smoothing.to_string());
            }
        });
    });
    rows.hint(STRENGTH_HELP);
    rows.full(|ui| {
        ui.add_enabled_ui(on, |ui| {
            ui.label(RichText::new("Sensitivity (lower = heavier)").color(TEXT_LABEL)).on_hover_text(SENSITIVITY_HELP);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (value, surface) in ffb.sensitivity.iter_mut().zip(ffb::SURFACES).rev() {
                    ui.add(DragValue::new(value).range(ffb::SENSITIVITY_RANGE).prefix(format!("{surface} ")))
                        .on_hover_text(SENSITIVITY_HELP);
                }
            });
        });
    });
}

/// Writes `key` (the strength, the smoothing) into the settings file's `[ffb]`, the rest of the file kept.
fn write_ffb(settings: &Path, key: &str, value: &str) {
    let file = std::fs::read_to_string(settings).unwrap_or_default();
    let text = rbr_formats::ini::set_value(&file, "ffb", key, value);
    if let Err(e) = std::fs::write(settings, text) {
        eprintln!("launcher: {}: {e}", settings.display());
    }
}

/// Each car's own force-feedback sensitivity (`[ffb_cars]`, `ffb::CarSensitivity`): one row per listed car (its
/// name when a car of `cars` has the key, else the key itself), the Drive page's selected car `car` added with the
/// global values, and RSF's own per-car sensitivities imported. Every edit is written to the settings file at once
/// (the pause menu's Force feedback page writes the same section while driving).
#[allow(clippy::too_many_arguments)]
fn ffb_cars_ui(
    ui: &mut Ui,
    on: bool,
    global: [i32; 3],
    linearity: i32,
    ffb_cars: &mut ffb::CarSensitivity,
    loads: &ffb::Loads,
    cars: &[Car],
    car: usize,
    root: &str,
    settings: &Path,
    read_only: bool,
    ffb_status: &mut Option<String>,
    search: &mut String,
) {
    ui.add_space(14.0);
    let title = RichText::new(format!("Force feedback per car ({})", ffb_cars.cars.len()))
        .size(12.0)
        .family(medium())
        .color(TEXT_2);
    egui::CollapsingHeader::new(title).default_open(false).show(ui, |ui| {
        ui.add_enabled_ui(on && !read_only, |ui| {
            kit::hint(ui, "Cars listed here use their own sensitivities instead of the ones above.");
            ui.add_space(4.0);
            let mut changed = false;
            kit::panel(ui, |rows| {
                rows.full(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut *search)
                            .hint_text("Search cars")
                            .desired_width(ui.available_width()),
                    );
                });
                let search = search.trim().to_lowercase();
                let mut remove: Option<String> = None;
                for (key, values) in ffb_cars.cars.iter_mut() {
                    let name = car_display(key, cars);
                    if !name.to_lowercase().contains(&search) {
                        continue;
                    }
                    rows.full(|ui| {
                        label_cell(ui, 150.0, &name);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                remove = Some(key.clone());
                            }
                            for (k, (value, surface)) in values.iter_mut().zip(ffb::SURFACES).enumerate().rev() {
                                let help = match maxed_out(loads, key, k, *value, linearity) {
                                    Some(n) => {
                                        format!("{SENSITIVITY_HELP}\n\nThe wheel maxed out {n} % of your driving here.")
                                    }
                                    None => SENSITIVITY_HELP.to_owned(),
                                };
                                changed |= ui
                                    .add(
                                        DragValue::new(value)
                                            .range(ffb::SENSITIVITY_RANGE)
                                            .prefix(format!("{surface} ")),
                                    )
                                    .on_hover_text(help)
                                    .changed();
                            }
                        });
                    });
                }
                if let Some(key) = remove {
                    ffb_cars.cars.remove(&key);
                    changed = true;
                }
                let selected = cars.get(car);
                let key = selected.map(|c| c.key.clone());
                let listed = key.as_ref().is_some_and(|k| ffb_cars.cars.contains_key(k));
                rows.full(|ui| {
                    let name = selected.map(|c| c.name.as_str()).unwrap_or("?");
                    if ui
                        .add_enabled(!listed, egui::Button::new(format!("Add the selected car ({name})")))
                        .on_hover_text("Lists this car with the sensitivities above")
                        .clicked()
                    {
                        if let Some(key) = key.clone() {
                            ffb_cars.cars.insert(key, global);
                            changed = true;
                        }
                    }
                    if ui
                        .add_enabled(has_rsf_settings(root), egui::Button::new("Import from RSF"))
                        .on_hover_text(IMPORT_HELP)
                        .clicked()
                    {
                        let n = import_ffb_cars(root, cars, global, ffb_cars);
                        *ffb_status = Some(match n {
                            1 => "Imported 1 car from RSF.".to_owned(),
                            n => format!("Imported {n} cars from RSF."),
                        });
                        changed |= n > 0;
                    }
                });
                rows.hint("Hover a value for how often the wheel maxed out; adjust in the pause menu while driving.");
                if let Some(status) = &*ffb_status {
                    rows.hint(status);
                }
            });
            if changed {
                write_ffb_cars(settings, ffb_cars);
            }
        });
    });
}

/// How often the wheel maxed out, percent, at `value` and `linearity`, over car `key`'s tracked driving on `surface`
/// (`ffb::load`), with enough of it.
fn maxed_out(loads: &ffb::Loads, key: &str, surface: usize, value: i32, linearity: i32) -> Option<i32> {
    let load = loads.of(key, surface).filter(|l| l.seconds() >= ffb::load::ADVICE_SECONDS)?;
    load.maxed_out(value, linearity).map(|share| (share * 100.0).round() as i32)
}

/// A per-car key as the player reads it: the car's name when a car of `cars` has the key, else the key itself.
fn car_display(key: &str, cars: &[Car]) -> String {
    cars.iter().find(|car| car.key == key).map(|car| car.name.clone()).unwrap_or_else(|| key.to_owned())
}

/// Writes `cars` into the settings file's `[ffb_cars]` section, the rest of the file kept.
fn write_ffb_cars(settings: &Path, cars: &ffb::CarSensitivity) {
    let file = std::fs::read_to_string(settings).unwrap_or_default();
    if let Err(e) = std::fs::write(settings, cars.replace_in(&file)) {
        eprintln!("launcher: {}: {e}", settings.display());
    }
}

/// What the import does.
const IMPORT_HELP: &str = "Lists every RSF car below with RSF's own sensitivity for it (rallysimfans_personal.ini's \
     forcefeedbacksensitivity per surface, a surface RSF leaves empty filled from the install's global), replacing \
     those cars' entries. Only cars that differ from the sensitivities above are added.";

/// Whether the game folder `root` holds RSF's files to import from (`rsfdata/`).
fn has_rsf_settings(root: &str) -> bool {
    Path::new(root.trim()).join("rsfdata").is_dir()
}

/// RSF's own per-car sensitivities for the RSF cars of `cars`, resolved as the removed import did (a surface RSF
/// leaves empty filled from the install's global). Adds the cars that differ from `global`, overwriting their
/// entries, and returns how many were added.
fn import_ffb_cars(root: &str, cars: &[Car], global: [i32; 3], ffb_cars: &mut ffb::CarSensitivity) -> usize {
    let root = Path::new(root.trim());
    let sources = Sources::install(root, 0);
    let personal_ini = std::fs::read(root.join("rallysimfans_personal.ini"))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let mut imported = 0;
    for car in cars {
        let crate::catalog::Physics::Pack(_) = car.physics else { continue };
        let Some(id) = car.rsf_id else { continue };
        let own = Sources { car: personal(&personal_ini, id), ..sources.clone() }.resolve(SLOT).sensitivity;
        if own != global {
            ffb_cars.cars.insert(car.key.clone(), own);
            imported += 1;
        }
    }
    imported
}

/// NGP's force-feedback sensitivity per surface (`ffb::Options::sensitivity`), in its readme's words.
const SENSITIVITY_HELP: &str = "How much of a car's steering force fits before the wheel maxes out. Higher: lighter, \
     more detail. Lower: heavier, maxes out sooner.";
const STRENGTH_HELP: &str = "Strength: how heavy the wheel feels, for every car.";
const DEVICE_HELP: &str = "The wheel the force goes to. First found tries each device in turn and keeps the first one \
     that takes the force; one picked by name does the same among the devices with that name. Some drivers list a \
     wheel more than once (Logitech Gaming Software lists a G29 three times), and only one of them is the wheel: the \
     line below says which one the last drive used.";
/// The rows' label column, the input button and the live meter's widths.
const ROW_LABEL: f32 = 104.0;
const BUTTON_WIDTH: f32 = 204.0;
const METER: f32 = 60.0;

/// The other actions a wheel button can drive.
const BUTTONS: [(Action, &str); 10] = [
    (Action::Pause, "Pause"),
    (Action::Restart, "Restart"),
    (Action::CallForHelp, "Call for help"),
    (Action::LookBack, "Look back"),
    (Action::Camera, "Change camera"),
    (Action::Wipers, "Wipers"),
    (Action::Headlights, "Headlights"),
    (Action::GaugeMode, "Gauge mode"),
    (Action::Ignition, "Ignition"),
    (Action::Neutral, "Neutral"),
];

/// An input that moved: which, and whether its value went up.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Moved {
    input: PadInput,
    up: bool,
}

/// The channel an action's axis drives, as `invert axes` names it.
fn invert_flag(invert: &mut Invert, action: Action) -> Option<&mut bool> {
    Some(match action {
        Action::SteerLeft => &mut invert.steering,
        Action::Throttle => &mut invert.throttle,
        Action::Brake => &mut invert.brake,
        Action::Handbrake => &mut invert.handbrake,
        Action::Clutch => &mut invert.clutch,
        _ => return None,
    })
}

/// Sets `action`'s device input; turning left sets both steering actions to the axis. An axis that went up as
/// the input was pressed or turned left is inverted: the game's steering is negative to the left, and its pedals
/// read `(1 - v) / 2`. A button is not.
fn bind(bindings: &mut Bindings, action: Action, moved: Moved) {
    bindings.actions[action as usize].pad = Some(moved.input);
    if action == Action::SteerLeft {
        bindings.actions[Action::SteerRight as usize].pad = Some(moved.input);
    }
    if let Some(flag) = invert_flag(&mut bindings.invert, action) {
        *flag = moved.input.is_axis() && moved.up;
    }
}

/// The action's live value from the joysticks' values, as the drive reads it: the steering -1..1, the rest 0..1.
fn live_value(bindings: &Bindings, action: Action, value: f32) -> f32 {
    if !bindings.get(action).pad.is_some_and(PadInput::is_axis) {
        return value;
    }
    let mut invert = bindings.invert;
    let inverted = invert_flag(&mut invert, action).is_some_and(|f| *f);
    match (action, inverted) {
        (Action::SteerLeft, false) => value,
        (Action::SteerLeft, true) => -value,
        (_, true) => (1.0 + value) / 2.0,
        (_, false) => (1.0 - value) / 2.0,
    }
}

/// Whether nothing drives from a wheel yet: no steering or pedal input bound.
pub fn unbound(bindings: &Bindings) -> bool {
    [Action::SteerLeft, Action::SteerRight, Action::Throttle, Action::Brake]
        .iter()
        .all(|&a| bindings.get(a).pad.is_none())
}

pub struct ControlsPage {
    capture: Option<Capture>,
    /// The page was shown at the first start, for a wheel nothing drives from yet.
    offered: bool,
    /// The joysticks connected, and when they were last looked for. Opened at the first [`ControlsPage::poll`], on the
    /// launcher's main thread (`rbr_render::sdl`): not at the page's making, which tests do on parallel threads.
    joysticks: Option<Joysticks>,
    scanned: Instant,
    /// The wheels whose rotation can be set, as last read, or why the last set failed.
    wheels: Result<Vec<Wheel>, String>,
    /// The last "Import from RSF"'s summary.
    ffb_status: Option<String>,
    /// The per-car force feedback list's search text.
    ffb_search: String,
    /// The force-feedback devices connected when the device list was last opened.
    ffb_devices: Vec<String>,
    /// The last drive's force feedback: the device it drove, or why there was none ([`last_ffb`]).
    ffb_last: Option<Result<String, String>>,
}

impl ControlsPage {
    pub fn new() -> ControlsPage {
        ControlsPage {
            capture: None,
            offered: false,
            joysticks: None,
            scanned: Instant::now(),
            wheels: Ok(wheel_rotation::wheels()),
            ffb_status: None,
            ffb_search: String::new(),
            ffb_devices: Vec::new(),
            ffb_last: None,
        }
    }

    /// Reads the force feedback of the last drive that had it from the drive logs in `logs`.
    pub fn read_last_drive(&mut self, logs: &Path) {
        if let Some(last) = last_ffb(logs) {
            self.ffb_last = Some(last);
        }
    }

    /// Whether a binding is being set: the keys go to it.
    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Whether a joystick is connected.
    pub fn has_pads(&self) -> bool {
        self.joysticks.as_ref().is_some_and(|j| !j.is_empty())
    }

    /// Once: whether to show the page, as a joystick is connected and nothing drives from one yet.
    pub fn offer(&mut self, bindings: &Bindings) -> bool {
        let offer = !self.offered && self.has_pads() && unbound(bindings);
        self.offered |= offer;
        offer
    }

    /// A key pressed while a binding is being set: Escape cancels it, any other bindable key sets a
    /// key binding. Whether the bindings changed.
    pub fn key(&mut self, bindings: &mut Bindings, key: KeyCode) -> bool {
        match self.capture {
            _ if key == KeyCode::Escape => {
                self.capture = None;
                false
            }
            Some(Capture::Key(action)) if bindings::bindable(key) => {
                bindings.actions[action as usize].key = Some(key);
                self.capture = None;
                true
            }
            _ => false,
        }
    }

    /// Closes the joysticks for the viewer, which has them to itself while it runs: Windows' DirectInput lets one
    /// open of a force-feedback wheel take it exclusively (`rbr_render::ffb`). [`ControlsPage::poll`] opens them again
    /// after.
    pub fn release(&mut self) {
        if let Some(joysticks) = &mut self.joysticks {
            joysticks.close();
        }
    }

    /// Reads the joysticks, and looks for new ones; while a device binding is being set, the first button pressed or
    /// axis moved sets it. While the viewer is `running` they stay closed ([`ControlsPage::release`]).
    pub fn poll(&mut self, bindings: &mut Bindings, running: bool) {
        if running {
            self.release();
            return;
        }
        let joysticks = self.joysticks.get_or_insert_with(Joysticks::connected);
        if self.capture.is_none() && self.scanned.elapsed() >= RESCAN {
            joysticks.rescan();
            self.scanned = Instant::now();
        }
        joysticks.read();
        let Some(Capture::Pad(action, rest)) = &self.capture else { return };
        let action = *action;
        let steering = action == Action::SteerLeft;
        let threshold = if steering { bindings::STEER_MOVE } else { bindings::PRESS };
        let mut moved = None;
        for (k, rest) in rest.iter().enumerate().take(joysticks.len()) {
            let (axes, buttons) = joysticks.objects(k);
            let now = values(joysticks.state(k), axes, buttons);
            let device = joysticks.device(k);
            // The steering takes an axis only.
            let found = now.zip(rest).find_map(|((object, v), &(_, r))| {
                let r = r.or_else(|| if let Object::Axis(axis) = object { joysticks.first(k, axis) } else { None })?;
                ((!steering || matches!(object, Object::Axis(_))) && (v - r).abs() > threshold)
                    .then_some((object, v > r))
            });
            if let Some((object, up)) = found {
                moved = Some(Moved { input: PadInput { device, object }, up });
                break;
            }
        }
        if let Some(moved) = moved {
            bind(bindings, action, moved);
            self.capture = None;
        }
    }

    /// Each joystick's values now.
    fn pad_rest(&self) -> Vec<Vec<(Object, Option<f32>)>> {
        let Some(joysticks) = &self.joysticks else { return Vec::new() };
        (0..joysticks.len())
            .map(|k| {
                let (axes, buttons) = joysticks.objects(k);
                values(joysticks.state(k), axes, buttons)
                    .map(|(object, v)| match object {
                        Object::Axis(axis) => (object, joysticks.first(k, axis).map(|_| v)),
                        _ => (object, Some(v)),
                    })
                    .collect()
            })
            .collect()
    }

    /// The value of a device input now, -1..1 for an axis, 0 or 1 for a button.
    fn read(&self, pad: PadInput) -> Option<f32> {
        let joysticks = self.joysticks.as_ref()?;
        let k = (0..joysticks.len()).find(|&k| joysticks.device(k) == pad.device)?;
        let (axes, buttons) = joysticks.objects(k);
        values(joysticks.state(k), axes, buttons).find(|(o, _)| *o == pad.object).map(|(_, v)| v)
    }

    /// The joysticks connected and the names their bindings show ([`short_name`]).
    fn connected(&self) -> Vec<(Device, String)> {
        let Some(joysticks) = &self.joysticks else { return Vec::new() };
        (0..joysticks.len()).map(|k| (joysticks.device(k), short_name(joysticks.name(k)))).collect()
    }

    /// The joysticks connected: name and id.
    fn pads(&self) -> Vec<String> {
        let Some(joysticks) = &self.joysticks else { return Vec::new() };
        (0..joysticks.len()).map(|k| format!("{} ({})", joysticks.name(k), joysticks.device(k).format())).collect()
    }

    /// The joysticks connected, by their short names ([`short_name`]).
    pub fn pad_names(&self) -> Vec<String> {
        self.connected().into_iter().map(|(_, name)| name).collect()
    }

    /// The page: the connected devices, the wheel, the shifter, the aids and the force feedback in two columns, the
    /// keyboard and "Reset all" under them.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        bindings: &mut Bindings,
        ffb: &mut ffb::Options,
        aids: &mut DriverAids,
        head_gasket: &mut i32,
        ffb_cars: &mut ffb::CarSensitivity,
        ffb_load: &ffb::Loads,
        cars: &[Car],
        car: usize,
        car_lock: Option<u16>,
        root: &str,
        settings: &Path,
        read_only: bool,
    ) {
        let pads = self.pads();
        if pads.is_empty() {
            ui.label(
                RichText::new(
                    "No wheel or gamepad found: connect it and it shows here. Until then the keyboard drives: its keys \
                     are under Keyboard below.",
                )
                .color(TEXT_2),
            );
        } else {
            kit::status_dot(ui, GOOD, &pads.join(" · "));
        }
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let left = &mut columns[0];
            kit::group_label(left, "Wheel and pedals", None);
            kit::panel(left, |rows| {
                self.pad_rows(rows, bindings, WHEEL);
                invert_ui(rows, bindings);
                self.rotation_ui(rows, ffb, cars.get(car).map(|c| (c.name.as_str(), car_lock)));
                rows.switch_row("Rev lights", Some(REV_LIGHTS_HELP), &mut ffb.rev_lights);
            });
            left.add_space(14.0);
            kit::group_label(left, "Wheel buttons", None);
            kit::panel(left, |rows| self.pad_rows(rows, bindings, BUTTONS));

            let right = &mut columns[1];
            kit::group_label(right, "Shifter", None);
            kit::panel(right, |rows| {
                self.pad_rows(rows, bindings, SHIFTER);
                shifter_ui(rows, bindings);
            });
            right.add_space(4.0);
            kit::hint(right, "With gears bound and Neutral not, letting go of the H-shifter selects neutral.");
            right.add_space(14.0);
            if kit::group_label(right, "Driver aids", Some("Game defaults")) {
                *aids = DriverAids::default();
            }
            kit::panel(right, |rows| aids_ui(rows, aids));
            right.add_space(14.0);
            kit::group_label(right, "Car", None);
            kit::panel(right, |rows| head_gasket_ui(rows, head_gasket));
            right.add_space(14.0);
            kit::group_label(right, "Force feedback", None);
            kit::panel(right, |rows| {
                ffb_ui(rows, ffb, settings, read_only);
                self.ffb_device_ui(rows, ffb);
            });
            let (global, linearity) = (ffb.sensitivity, ffb.linearity());
            let on = ffb.settings.enabled;
            let (status, search) = (&mut self.ffb_status, &mut self.ffb_search);
            ffb_cars_ui(
                right, on, global, linearity, ffb_cars, ffb_load, cars, car, root, settings, read_only, status, search,
            );
        });
        ui.add_space(14.0);
        egui::CollapsingHeader::new(RichText::new("Keyboard").size(12.0).family(medium()).color(TEXT_2))
            .default_open(false)
            .show(ui, |ui| self.keys_ui(ui, bindings));
        ui.add_space(12.0);
        if ui.button("Reset all controls to defaults").clicked() {
            *bindings = Bindings::default();
            self.capture = None;
        }
    }

    /// A section's device rows.
    fn pad_rows<const N: usize>(&mut self, rows: &mut Rows, bindings: &mut Bindings, list: [(Action, &str); N]) {
        let connected = self.connected();
        for (action, label) in list {
            self.pad_row(rows, bindings, action, label, &connected);
        }
    }

    /// An action's device row: its input (click it to set it; named by `connected`) or "Not set", its live value
    /// and Clear or Bind.
    fn pad_row(
        &mut self,
        rows: &mut Rows,
        bindings: &mut Bindings,
        action: Action,
        label: &str,
        connected: &[(Device, String)],
    ) {
        let binding = bindings.get(action);
        let steering = action == Action::SteerLeft;
        let capturing = matches!(&self.capture, Some(Capture::Pad(a, _)) if *a == action);
        let text = if capturing {
            Some(if steering { "turn the wheel left…" } else { "move it…" }.to_owned())
        } else if steering {
            Some(steering_label(bindings, connected)).filter(|t| t != "-")
        } else {
            binding.pad.map(|p| pad_label(p, connected))
        };
        let bound = text.is_some();
        let live = binding.pad.and_then(|p| self.read(p)).map(|v| live_value(bindings, action, v));
        let (set, clear) =
            binding_row(rows, ROW_LABEL, label, text.as_deref(), capturing, |ui| meter(ui, live, steering, bound));
        if set {
            self.capture = Some(Capture::Pad(action, self.pad_rest()));
        }
        if clear {
            bindings.actions[action as usize].pad = None;
            if steering {
                bindings.actions[Action::SteerRight as usize].pad = None;
            }
        }
    }

    /// Each action's key, and a clear.
    fn keys_ui(&mut self, ui: &mut Ui, bindings: &mut Bindings) {
        ui.columns(2, |columns| {
            for (n, column) in columns.iter_mut().enumerate() {
                kit::panel(column, |rows| {
                    for (k, action) in Action::ALL.into_iter().enumerate() {
                        if k * 2 / Action::ALL.len() != n {
                            continue;
                        }
                        let key = bindings.get(action).key;
                        let capturing = matches!(self.capture, Some(Capture::Key(a)) if a == action);
                        let text =
                            if capturing { Some("press a key (Esc cancels)") } else { key.map(bindings::key_name) };
                        let (set, clear) = binding_row(rows, 150.0, action.name(), text, capturing, |_| {});
                        if set {
                            self.capture = Some(Capture::Key(action));
                        }
                        if clear {
                            bindings.actions[action as usize].key = None;
                        }
                    }
                });
            }
        });
    }

    /// The wheel's lock-to-lock rotation: as it is, the same for every car, or each car's own up to the degrees; how
    /// far the player's wheel turns; and what the selected car, `car` (its name and lock), gets from them. Set on the
    /// wheel when changed, and by the viewer when a drive starts.
    fn rotation_ui(&mut self, rows: &mut Rows, ffb: &mut ffb::Options, car: Option<(&str, Option<u16>)>) {
        let mut commit = false;
        row_tip(rows, "Steering rotation", ROTATION_HELP, |ui| {
            const MODES: [(Option<bool>, &str); 3] =
                [(None, "Wheel as it is"), (Some(false), "Same for every car"), (Some(true), "Each car's own, up to")];
            let mut mode = ffb.rotation.map(|_| ffb.rotation_per_car);
            let mut degrees = ffb.rotation.unwrap_or(900);
            if let Some(per_car) = mode {
                commit |= degrees_ui(ui, &mut degrees);
                ffb.rotation_per_car = per_car;
            }
            let name = MODES.iter().find(|(m, _)| *m == mode).map_or("", |(_, n)| n);
            ComboBox::from_id_salt("rotation").selected_text(name).show_ui(ui, |ui| {
                for (m, n) in MODES {
                    commit |= ui.selectable_value(&mut mode, m, n).changed();
                }
            });
            ffb.rotation = mode.map(|_| degrees);
        });
        row_tip(rows, "Your wheel turns", WHEEL_ROTATION_HELP, |ui| {
            let mut known = ffb.wheel_rotation.is_some();
            let mut degrees = ffb.wheel_rotation.unwrap_or(900);
            if known {
                commit |= degrees_ui(ui, &mut degrees);
            }
            let name = if known { "I set it to" } else { "ReRBR sets it" };
            ComboBox::from_id_salt("wheel-rotation").selected_text(name).show_ui(ui, |ui| {
                commit |= ui.selectable_value(&mut known, false, "ReRBR sets it").changed();
                commit |= ui.selectable_value(&mut known, true, "I set it to").changed();
            });
            ffb.wheel_rotation = known.then_some(degrees);
        });
        // On the wheel as the drive will: the wheel's own rotation when given, else the rotation.
        if let Some(degrees) = ffb.rotation.filter(|_| commit).map(|r| ffb.wheel_rotation.unwrap_or(r)) {
            self.wheels = wheel_rotation::set(degrees);
        }
        let settable = matches!(&self.wheels, Ok(wheels) if !wheels.is_empty());
        let status = match &self.wheels {
            Ok(wheels) if wheels.is_empty() => "ReRBR cannot set a wheel's rotation here.".to_owned(),
            Ok(wheels) => {
                wheels.iter().map(|w| format!("{} turns {}°", w.name, w.degrees)).collect::<Vec<_>>().join(", ")
            }
            Err(e) => e.clone(),
        };
        rows.hint(&status);
        rows.hint(&rotation_summary(ffb, car, settable));
    }

    /// Which device gets the force (`ffb::Options::device`): the first found or one by name. The devices are looked
    /// for when the list is opened, not on the page's rescan: SDL takes a tenth of a second over it.
    fn ffb_device_ui(&mut self, rows: &mut Rows, ffb: &mut ffb::Options) {
        let on = ffb.settings.enabled;
        row_tip(rows, "Device", DEVICE_HELP, |ui| {
            ui.add_enabled_ui(on, |ui| {
                let selected = ffb.device.as_deref().unwrap_or("first found").to_owned();
                let list = ComboBox::from_id_salt("ffb-device").selected_text(selected).show_ui(ui, |ui| {
                    ui.selectable_value(&mut ffb.device, None, "first found");
                    for name in &self.ffb_devices {
                        ui.selectable_value(&mut ffb.device, Some(name.clone()), name);
                    }
                    // The one set that is not connected stays in the list.
                    let set = ffb.device.as_deref().filter(|d| !self.ffb_devices.iter().any(|n| ffb::picks(d, n)));
                    if let Some(set) = set {
                        let _ = ui.selectable_label(true, format!("{set} (not connected)"));
                    }
                });
                if list.response.clicked() {
                    self.ffb_devices = ffb::devices().unwrap_or_else(|e| {
                        eprintln!("launcher: {e}");
                        Vec::new()
                    });
                }
            });
        });
        // Greyed while the force feedback is off, as the device list above.
        let last = match &self.ffb_last {
            Some(Ok(device)) => Some((format!("Last drive: on {device}"), TEXT_3, None)),
            Some(Err(why)) => {
                let text = "Last drive: no force feedback. If your wheel is listed more than once in Device, pick \
                            another of its entries.";
                Some((text.to_owned(), WARN, Some(why.as_str())))
            }
            None => None,
        };
        if let Some((text, color, why)) = last {
            rows.full(|ui| {
                ui.add_enabled_ui(on, |ui| {
                    let label = ui.add(egui::Label::new(RichText::new(text).size(12.0).color(color)).wrap());
                    if let Some(why) = why {
                        label.on_hover_text(why);
                    }
                });
            });
        }
    }
}

/// A binding's input as a button (click it to set the binding): mono on the input fill, accent while being set.
fn binding_button(ui: &mut Ui, text: &str, capturing: bool) -> egui::Response {
    let color = if capturing { LINK } else { TEXT };
    let button = egui::Button::new(RichText::new(text).font(mono(11.5)).color(color))
        .fill(INPUT)
        .truncate()
        .min_size(egui::vec2(BUTTON_WIDTH, 24.0));
    ui.add_sized([BUTTON_WIDTH, 24.0], button).on_hover_text(text)
}

/// A binding's row: `label`, then `text` as the input button (or "Not set") with `after` beside it unless being set,
/// and Clear or Bind at the right. Returns whether the binding is to be set and whether to be cleared.
fn binding_row(
    rows: &mut Rows,
    width: f32,
    label: &str,
    text: Option<&str>,
    capturing: bool,
    after: impl FnOnce(&mut Ui),
) -> (bool, bool) {
    let bound = text.is_some();
    let (mut set, mut clear) = (false, false);
    rows.full(|ui| {
        label_cell(ui, width, label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            clear = bound && !capturing && kit::link(ui, "Clear").clicked();
            if !bound {
                set = kit::link(ui, "Bind").clicked();
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| match text {
                Some(text) => {
                    set |= binding_button(ui, text, capturing).clicked();
                    if !capturing {
                        after(ui);
                    }
                }
                None => _ = ui.label(RichText::new("Not set").color(TEXT_3)),
            });
        });
    });
    (set, clear)
}

/// A live meter of an input (the steering from the middle, the rest from the left); an empty track while it has no
/// reading, nothing on an unbound row.
fn meter(ui: &mut Ui, value: Option<f32>, steering: bool, bound: bool) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(METER, 24.0), egui::Sense::hover());
    if !bound {
        return;
    }
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
    ui.painter().rect_filled(track, 2, INPUT);
    let Some(value) = value else { return };
    if steering {
        let x = track.center().x + value.clamp(-1.0, 1.0) * track.width() / 2.0;
        let marker = egui::Rect::from_center_size(egui::pos2(x, track.center().y), egui::vec2(4.0, 10.0));
        ui.painter().rect_filled(marker, 1, PROGRESS);
        response.on_hover_text(format!("{value:+.2}"));
    } else {
        let mut fill = track;
        fill.set_right(track.left() + track.width() * value.clamp(0.0, 1.0));
        ui.painter().rect_filled(fill, 2, PROGRESS);
        response.on_hover_text(format!("{value:.2}"));
    }
}

/// The pedals' and the steering's inverts (`input.ini`'s `Invert*Axis`), which the set-up sets.
fn invert_ui(rows: &mut Rows, bindings: &mut Bindings) {
    rows.full(|ui| {
        label_cell(ui, ROW_LABEL, "Invert");
        ui.horizontal_wrapped(|ui| {
            for (action, label) in WHEEL {
                if let Some(flag) = invert_flag(&mut bindings.invert, action) {
                    ui.checkbox(flag, label).on_hover_text(INVERT_HELP);
                }
            }
        });
    })
}

/// What the inverts do.
const INVERT_HELP: &str = "Flips an axis, as RBR's input.ini does: for a pedal that rests at the other end, or a \
     wheel that turns the other way. Binding an axis sets it.";

/// The steering's inputs: the axis once when both are on it, else both ([`pad_label`]).
fn steering_label(bindings: &Bindings, connected: &[(Device, String)]) -> String {
    let (left, right) = (bindings.get(Action::SteerLeft).pad, bindings.get(Action::SteerRight).pad);
    let f = |p: Option<PadInput>| p.map_or("-".to_owned(), |p| pad_label(p, connected));
    match (left, right) {
        (None, None) => "-".to_owned(),
        (Some(l), Some(r)) if r == l => f(Some(l)),
        (l, r) => format!("{} / {}", f(l), f(r)),
    }
}

/// A device input as the player reads it: the axis or button first, so a cut label keeps it, then its device's name
/// while connected (`connected`), else its id (`axis 0 · G923 Racing Wheel`).
fn pad_label(pad: PadInput, connected: &[(Device, String)]) -> String {
    let id = pad.device.format();
    let device = connected.iter().find(|(d, _)| *d == pad.device).map_or(id.as_str(), |(_, name)| name);
    let input = pad.format();
    format!("{} · {device}", input.strip_prefix(&id).unwrap_or(&input).trim())
}

/// A device's name without its maker and what it is for: `Logitech G923 Racing Wheel for PlayStation 4 and PC` is
/// `G923 Racing Wheel`, `Thrustmaster T500 RS Gear Shift` is `T500 RS Gear Shift`.
fn short_name(name: &str) -> String {
    let name = name.split(" for ").next().unwrap_or(name).trim();
    // The T500's shifter calls its maker "Thustmaster".
    let makers = [
        "Logitech",
        "Thrustmaster",
        "Thustmaster",
        "Fanatec",
        "Moza",
        "Simucube",
        "Simagic",
        "Cammus",
        "Microsoft",
        "Sony",
    ];
    let rest = makers.iter().find_map(|m| name.strip_prefix(m).and_then(|r| r.strip_prefix(' ')));
    rest.filter(|r| !r.is_empty()).unwrap_or(name).to_owned()
}

/// What the rotation setting does.
const REV_LIGHTS_HELP: &str = "The wheel's LEDs fill up to the car's shift point and flash past it. Linux: wheels \
     whose driver gives them RPM lights (Logitech G27, G29, G923 for PlayStation). macOS and Windows: those Logitech \
     wheels, untested.";

/// A rotation's degrees; whether a change is done (typed, or the drag let go), for the wheel's `range`.
fn degrees_ui(ui: &mut Ui, degrees: &mut u16) -> bool {
    let r = ui.add(DragValue::new(degrees).range(wheel_rotation::DEGREES).speed(5).suffix("°"));
    (r.changed() && !r.dragged()) || r.drag_stopped()
}

/// What the selected car, `car` (its name and lock, degrees, when its setup loaded), gets from the rotation settings,
/// worked out as the drive does (`ffb::Options::rotation_for`, `SoftLock::new`); `settable` when a wheel's driver
/// takes the rotation.
fn rotation_summary(ffb: &ffb::Options, car: Option<(&str, Option<u16>)>, settable: bool) -> String {
    let (name, lock) = car.map_or(("This car", None), |(n, l)| (n, l));
    let Some(rotation) = ffb.rotation_for(lock) else {
        return "Full lock at your wheel's own end, whatever it is set to.".to_owned();
    };
    let at = match ffb.wheel_rotation {
        Some(wheel) => rotation.min(wheel),
        None if settable => rotation,
        None => return format!("Full lock at your wheel's own end: set Your wheel turns to get {rotation}°."),
    };
    let car_lock = match lock {
        Some(l) => format!("{name}: car lock {l}°. "),
        None => format!("{name}: car lock unknown (no setup). "),
    };
    let feel = lock.map_or(String::new(), |l| {
        let ratio = f32::from(l) / f32::from(at);
        match ratio {
            r if (r - 1.0).abs() < 0.01 => ", true to the car".to_owned(),
            r if r > 1.0 => format!(", steering {r:.2}× quicker than the real car"),
            r => format!(", steering {:.2}× slower than the real car", r),
        }
    });
    let end = match SoftLock::new(Some(rotation), ffb.wheel_rotation) {
        Some(soft) => format!(" Soft lock at {}° each way.", f32::from(soft.lock) / 2.0),
        None if ffb.wheel_rotation.is_some() => " Full lock at your wheel's end.".to_owned(),
        None => " The wheel stops there (as far as it turns).".to_owned(),
    };
    format!("{car_lock}Full lock at {at}°{feel}.{end}")
}

const ROTATION_HELP: &str = "How far you turn the wheel, lock to lock, for the car's full lock. Same for every car: \
     those degrees for each, so a car with more lock steers quicker and one with less slower. Each car's own: the \
     car's real lock, so it steers as the real car does, up to the degrees; a car with more lock than that steers \
     quicker. Wheel as it is: full lock at your wheel's own end, nothing changed.";

const WHEEL_ROTATION_HELP: &str = "ReRBR sets it: the steering rotation is set on the wheel, which then stops there. \
     This works on Linux with the wheel drivers that allow it (Logitech, Fanatec), not on Windows or macOS. I set it \
     to: the degrees your wheel turns as its own software sets it. A steering rotation below that gets a soft lock: \
     full lock comes early and past it the wheel pushes back. A steering rotation above it, say a car of 1050 on a \
     900 wheel, reaches full lock at the wheel's end.";

fn shifter_ui(rows: &mut Rows, bindings: &mut Bindings) {
    let shifter = &mut bindings.shifter;
    rows.switch_row("Advanced H-shifter", Some("A gear needs the clutch or matched revs"), &mut shifter.advanced)
        .on_hover_text(
            "With gears 1-6 and reverse bound to the shifter. Off: the gears select at once, as NGP has them. \
             The Drive page's Gearbox picks each car's: an H-pattern car drives without Clutch Help and the \
             automatic gearbox, a sequential one (every car until picked) keeps both and shifts as before. The \
             grind plays RBRControls' Shifter_Error.wav from the RSF folder.",
        );
    if !shifter.advanced {
        return;
    }
    rows.row("Clutch threshold", |ui| {
        ui.add(egui::Slider::new(&mut shifter.clutch_threshold, 0.0..=0.95))
            .on_hover_text("How far the clutch pedal must be pressed to count as in");
    });
}

/// NGP's `headGasketDurability` with RSF's "Engine head gasket durability fix" on, as its install has it.
const HEAD_GASKET_FIX: i32 = 10;

/// What the head gasket fix does.
const HEAD_GASKET_HELP: &str = "RSF's fix: the head gasket wears 10 times slower, so realistic damage no longer kills \
     the engine unexpectedly. Off is NGP's default.";

/// The game's gearbox aids (its English labels, `bstrings.eng`); "Game defaults" is the group's link. A car with an
/// H-pattern gearbox under the advanced H-shifter drives without the automatic gearbox and clutch help.
fn aids_ui(rows: &mut Rows, aids: &mut DriverAids) {
    const H_PATTERN: &str = "; off for a car whose gearbox is H-pattern under the advanced H-shifter";
    let list = [
        ("Automatic gearbox", &mut aids.auto_gears, format!("Shifts up and down for you{H_PATTERN}")),
        ("Gear Protection", &mut aids.gear_guard, "Stops a shift into a gear that would over-rev".into()),
        ("Neutral Lock", &mut aids.neutral_lock, "Keeps the gearbox from slipping into neutral".into()),
        ("Clutch Help", &mut aids.clutch_help, format!("Works the clutch for you when you shift{H_PATTERN}")),
    ];
    for (label, value, tip) in list {
        rows.switch_row(label, None, value).on_hover_text(tip);
    }
}

/// The head gasket fix (NGP's `headGasketDurability`, 1 off).
fn head_gasket_ui(rows: &mut Rows, head_gasket: &mut i32) {
    let mut fix = *head_gasket > 1;
    if rows.switch_row("Head gasket durability fix", None, &mut fix).on_hover_text(HEAD_GASKET_HELP).changed() {
        *head_gasket = if fix { HEAD_GASKET_FIX } else { 1 };
    }
}

/// The force feedback of the newest drive log in `logs` that has it ([`ffb_outcome`]).
fn last_ffb(logs: &Path) -> Option<Result<String, String>> {
    let mut drives: Vec<_> = std::fs::read_dir(logs)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("drive-") && n.ends_with(".log"))
        })
        .collect();
    // The names carry the time (`crash::Watch::start`).
    drives.sort();
    drives.iter().rev().find_map(|log| ffb_outcome(&String::from_utf8_lossy(&std::fs::read(log).ok()?)))
}

/// The device a drive log's force feedback went to, or why there was none (`ffb::LOG_ON`, `ffb::LOG_OFF`).
fn ffb_outcome(log: &str) -> Option<Result<String, String>> {
    log.lines().rev().find_map(|line| {
        line.strip_prefix(ffb::LOG_ON)
            .map(|device| Ok(device.to_owned()))
            .or_else(|| line.strip_prefix(ffb::LOG_OFF).map(|why| Err(why.to_owned())))
    })
}

impl crate::Form {
    /// The Controls page, under the Settings tabs.
    pub fn controls_page(&mut self, ui: &mut Ui) {
        let root = self.game_root();
        let settings = self.paths.settings.clone();
        let (read_only, car) = (self.read_only, self.car);
        let car_lock = self.setup.steering_lock().map(wheel_rotation::car_degrees);
        egui::ScrollArea::vertical().id_salt("controls").auto_shrink([false, false]).show(ui, |ui| {
            kit::body(ui, |ui| {
                self.controls.ui(
                    ui,
                    &mut self.bindings,
                    &mut self.ffb,
                    &mut self.options.aids,
                    &mut self.head_gasket,
                    &mut self.ffb_cars,
                    &self.ffb_load,
                    &self.catalog.cars,
                    car,
                    car_lock,
                    &root,
                    &settings,
                    read_only,
                )
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rotation_summary_says_what_the_car_gets() {
        let options = |rotation, per_car, wheel| ffb::Options {
            rotation,
            rotation_per_car: per_car,
            wheel_rotation: wheel,
            ..ffb::Options::default()
        };
        let say = |o: &ffb::Options, lock: u16, settable| rotation_summary(o, Some(("Fiesta", Some(lock))), settable);
        // Each car's own up to 540, on a wheel set to 900 in its software.
        let own = options(Some(540), true, Some(900));
        assert_eq!(
            say(&own, 450, false),
            "Fiesta: car lock 450°. Full lock at 450°, true to the car. Soft lock at 225° each way."
        );
        assert_eq!(
            say(&own, 720, false),
            "Fiesta: car lock 720°. Full lock at 540°, steering 1.33× quicker than the real car. Soft lock at 270° \
             each way."
        );
        // The same 540 for every car.
        let same = options(Some(540), false, Some(900));
        assert_eq!(
            say(&same, 450, false),
            "Fiesta: car lock 450°. Full lock at 540°, steering 0.83× slower than the real car. Soft lock at 270° \
             each way."
        );
        // A car of 1050 on a 900 wheel: the wheel's end.
        assert_eq!(
            say(&options(Some(1080), true, Some(900)), 1050, false),
            "Fiesta: car lock 1050°. Full lock at 900°, steering 1.17× quicker than the real car. Full lock at your \
             wheel's end."
        );
        // ReRBR sets it: on a driver that takes it, else nothing to go on.
        let set = options(Some(540), true, None);
        assert_eq!(
            say(&set, 450, true),
            "Fiesta: car lock 450°. Full lock at 450°, true to the car. The wheel stops there (as far as it turns)."
        );
        assert_eq!(say(&set, 450, false), "Full lock at your wheel's own end: set Your wheel turns to get 450°.");
        assert_eq!(
            say(&options(None, false, Some(900)), 450, true),
            "Full lock at your wheel's own end, whatever it is set to."
        );
    }
    use rbr_render::bindings::Device;

    const G923: Device = Device { vendor: 0x046d, product: 0xc266, index: 0 };

    /// The per-car hover: each surface with 20 s of driving, at the car's own sensitivity there.
    #[test]
    fn the_per_car_hover_says_how_often_the_wheel_maxed_out() {
        let loads = ffb::Loads::parse(
            "[ffb_load]\nrsf-81 gravel = 0,0,0,0,0,0,0,0,200\nrsf-81 tarmac = 0,300\nrsf-81 snow = 10\n",
        );
        // x 42.5 maxes out at 30 %, not at 100 %; snow has 1 s only.
        assert_eq!(
            (maxed_out(&loads, "rsf-81", 0, 30, 0), maxed_out(&loads, "rsf-81", 1, 100, 0)),
            (Some(100), Some(0))
        );
        assert_eq!((maxed_out(&loads, "rsf-81", 2, 100, 0), maxed_out(&loads, "rsf-1", 0, 100, 0)), (None, None));
    }

    fn moved(object: Object, up: bool) -> Moved {
        Moved { input: PadInput { device: G923, object }, up }
    }

    /// The G923 on Linux: the wheel axis 0, the pedals axes 1 and 2 (going down as pressed), the handbrake a button.
    #[test]
    fn binding_sets_the_inverts_and_the_bars_read_as_the_drive() {
        let mut bindings = Bindings::default();
        assert!(unbound(&bindings));
        bind(&mut bindings, Action::SteerLeft, moved(Object::Axis(0), false));
        assert_eq!(bindings.get(Action::SteerRight).pad, Some(moved(Object::Axis(0), false).input));
        bind(&mut bindings, Action::Throttle, moved(Object::Axis(2), false));
        // A brake pedal that rises as pressed is inverted; a button sets no invert.
        bind(&mut bindings, Action::Brake, moved(Object::Axis(1), true));
        bind(&mut bindings, Action::Handbrake, moved(Object::Button(7), true));
        let i = bindings.invert;
        assert_eq!((i.steering, i.throttle, i.brake, i.handbrake), (false, false, true, false));
        // A button in place of an inverted pedal clears its invert.
        bindings.invert.handbrake = true;
        bind(&mut bindings, Action::Handbrake, moved(Object::Button(7), true));
        assert!(!bindings.invert.handbrake);
        assert!(!unbound(&bindings));
        // The keys stay.
        assert_eq!(bindings.get(Action::Throttle).key, Some(KeyCode::ArrowUp));
        // The bars read as the drive does: the throttle at rest (axis +1) is 0, pressed (-1) is 1.
        assert_eq!(
            (live_value(&bindings, Action::Throttle, 1.0), live_value(&bindings, Action::Throttle, -1.0)),
            (0.0, 1.0)
        );
        assert_eq!(live_value(&bindings, Action::Brake, -1.0), 0.0);
    }

    #[test]
    fn the_steering_shows_once_for_one_axis() {
        let mut bindings = Bindings::default();
        assert_eq!(steering_label(&bindings, &[]), "-");
        bind(&mut bindings, Action::SteerLeft, moved(Object::Axis(0), true));
        assert_eq!(steering_label(&bindings, &[]), "axis 0 · 046d:c266/0");
        let wheel = bindings.get(Action::SteerLeft).pad.unwrap().device;
        let connected = [(wheel, short_name("Logitech G923 Racing Wheel for PlayStation 4 and PC"))];
        assert_eq!(steering_label(&bindings, &connected), "axis 0 · G923 Racing Wheel");
        assert!(bindings.invert.steering);
        bindings.actions[Action::SteerRight as usize].pad = None;
        assert_eq!(steering_label(&bindings, &[]), "axis 0 · 046d:c266/0 / -");
    }

    /// "Import from RSF" takes RSF's per-car sensitivities, filling a surface RSF leaves empty from the install's
    /// global, and only the cars that differ from the launcher's global ones (overwriting their entries).
    #[test]
    fn import_takes_rsf_s_per_car_sensitivities() {
        let root = std::env::temp_dir().join(format!("ffb-import-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("RichardBurnsRally.ini"),
            b"[NGP]\nForceFeedbackSensitivityGravel=280\nForceFeedbackSensitivityTarmac=250\n\
             ForceFeedbackSensitivitySnow=280\n",
        )
        .unwrap();
        std::fs::write(
            root.join("rallysimfans_personal.ini"),
            b"[car88]\nforcefeedbacksensitivitygravel=300\nlogin=secret\n[car87]\n\
             forcefeedbacksensitivitygravel=280\nforcefeedbacksensitivitytarmac=250\nforcefeedbacksensitivitysnow=280\n",
        )
        .unwrap();
        let pack = |id, name: &str| Car {
            slot: 5,
            name: name.into(),
            physics: crate::catalog::Physics::Pack(root.join("pack")),
            key: format!("pack-{}", name.to_lowercase()),
            rsf_id: Some(id),
            ..Car::default()
        };
        let cars = vec![pack(88, "Evo"), pack(87, "Same")];
        let global = [280, 250, 280];
        let mut ffb_cars = ffb::CarSensitivity { cars: [("pack-evo".to_owned(), [1, 2, 3])].into() };
        let n = import_ffb_cars(&root.display().to_string(), &cars, global, &mut ffb_cars);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(n, 1);
        // Car 88's gravel is RSF's own, its other surfaces the install's global; car 87 matches the launcher's
        // global and is left out, and the stale entry is overwritten.
        assert_eq!(ffb_cars.cars, [("pack-evo".to_owned(), [300, 250, 280])].into());
        assert_eq!(car_display("pack-evo", &cars), "Evo");
        assert_eq!(car_display("rsf-1", &cars), "rsf-1");
    }

    #[test]
    fn device_names_drop_the_maker_and_what_they_are_for() {
        assert_eq!(short_name("Logitech G923 Racing Wheel for PlayStation 4 and PC"), "G923 Racing Wheel");
        assert_eq!(short_name("Thrustmaster T500 RS Gear Shift"), "T500 RS Gear Shift");
        assert_eq!(short_name("Thustmaster T500 RS Gear Shift"), "T500 RS Gear Shift");
        assert_eq!(short_name("Logitech"), "Logitech");
        assert_eq!(short_name("Generic USB Joystick"), "Generic USB Joystick");
    }

    #[test]
    fn the_last_drive_names_the_force_feedbacks_device_or_why_none() {
        use super::ffb_outcome;
        let on = "viewer: pack car x\nffb: steering axis: created (effect 0) and running\n\
                  viewer: force feedback on Logitech G29 Driving Force Racing Wheel USB\nstep 1520\n";
        assert_eq!(ffb_outcome(on), Some(Ok("Logitech G29 Driving Force Racing Wheel USB".into())));
        let off = "viewer: no force feedback: no force-feedback device fits: G29 (no constant force)\n";
        assert_eq!(ffb_outcome(off), Some(Err("no force-feedback device fits: G29 (no constant force)".into())));
        assert_eq!(ffb_outcome("viewer: live: 770 drawn frames\n"), None);
    }
}
