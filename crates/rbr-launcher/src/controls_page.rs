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

use egui::{Color32, ComboBox, DragValue, ProgressBar, RichText, Ui};
use rbr_formats::car_pack::SLOT;
use rbr_formats::rsf::settings::{personal, Sources};
use rbr_formats::saves::action_map::Object;
use rbr_gameplay::poll::Invert;
use rbr_gameplay::race_options::DriverAids;
use rbr_render::bindings::{self, Action, Bindings, Device, PadInput};
use rbr_render::devices::{values, Joysticks};
use rbr_render::ffb;
use rbr_render::wheel_rotation::{self, Wheel};
use winit::keyboard::KeyCode;

use crate::catalog::Car;

/// A binding being set: its action, and for a device input every joystick's values when it began.
enum Capture {
    Key(Action),
    Pad(Action, Vec<Vec<(Object, f32)>>),
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

/// The force feedback: on or off, the strength, the direction and NGP's sensitivity per surface. The strength is
/// written to the settings file at once, as the pause menu's Force feedback page does while driving.
fn ffb_ui(ui: &mut Ui, ffb: &mut ffb::Options, settings: &Path, read_only: bool) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut ffb.settings.enabled, "on");
        ui.add_enabled_ui(ffb.settings.enabled, |ui| {
            let strength =
                ui.add(DragValue::new(&mut ffb.settings.strength).range(0.0..=1.0).speed(0.01).prefix("strength "));
            if strength.on_hover_text(STRENGTH_HELP).changed() && !read_only {
                write_strength(settings, ffb.settings.strength);
            }
            ui.checkbox(&mut ffb.settings.inverse, "inverse");
        });
    });
    ui.label(RichText::new(STRENGTH_HELP).color(Color32::GRAY));
    ui.add_enabled_ui(ffb.settings.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Sensitivity for cars without their own").on_hover_text(SENSITIVITY_HELP);
            for (value, surface) in ffb.sensitivity.iter_mut().zip(ffb::SURFACES) {
                ui.add(DragValue::new(value).range(ffb::SENSITIVITY_RANGE).prefix(format!("{surface} ")))
                    .on_hover_text(SENSITIVITY_HELP);
            }
        });
    });
}

/// Writes the strength into the settings file's `[ffb]`, the rest of the file kept.
fn write_strength(settings: &Path, strength: f32) {
    let file = std::fs::read_to_string(settings).unwrap_or_default();
    let text = rbr_formats::ini::set_value(&file, "ffb", "strength", &strength.to_string());
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
    ffb_cars: &mut ffb::CarSensitivity,
    loads: &ffb::Loads,
    cars: &[Car],
    car: usize,
    root: &str,
    settings: &Path,
    read_only: bool,
    ffb_status: &mut Option<String>,
) {
    heading(ui, "Force feedback per car");
    ui.add_enabled_ui(on && !read_only, |ui| {
        ui.label(
            RichText::new(
                "Cars listed here use their own sensitivities instead of the ones above, and the pause menu's Force \
                 feedback page sets them while driving.",
            )
            .color(Color32::GRAY),
        );
        let mut changed = false;
        let mut remove: Option<String> = None;
        for (key, values) in ffb_cars.cars.iter_mut() {
            ui.horizontal(|ui| {
                ui.label(car_display(key, cars));
                for (k, (value, surface)) in values.iter_mut().zip(ffb::SURFACES).enumerate() {
                    let help = match maxed_out(loads, key, k, *value) {
                        Some(n) => format!("{SENSITIVITY_HELP}\n\nThe wheel maxed out {n} % of your driving here."),
                        None => SENSITIVITY_HELP.to_owned(),
                    };
                    changed |= ui
                        .add(DragValue::new(value).range(ffb::SENSITIVITY_RANGE).prefix(format!("{surface} ")))
                        .on_hover_text(help)
                        .changed();
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(key.clone());
                }
            });
        }
        if let Some(key) = remove {
            ffb_cars.cars.remove(&key);
            changed = true;
        }
        let selected = cars.get(car);
        let key = selected.map(|c| c.key.clone());
        let listed = key.as_ref().is_some_and(|k| ffb_cars.cars.contains_key(k));
        ui.horizontal(|ui| {
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
        ui.label(
            RichText::new("Hover a value for how often the wheel maxed out; adjust in the pause menu while driving.")
                .color(Color32::GRAY),
        );
        if let Some(status) = &*ffb_status {
            ui.label(RichText::new(status).color(Color32::GRAY));
        }
        if changed {
            write_ffb_cars(settings, ffb_cars);
        }
    });
}

/// How often the wheel maxed out, percent, at `value` over car `key`'s tracked driving on `surface` (`ffb::load`),
/// with enough of it.
fn maxed_out(loads: &ffb::Loads, key: &str, surface: usize, value: i32) -> Option<i32> {
    let load = loads.of(key, surface).filter(|l| l.seconds() >= ffb::load::ADVICE_SECONDS)?;
    load.maxed_out(value).map(|share| (share * 100.0).round() as i32)
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
/// The rows' label column and the live bars' width.
const ROW_LABEL: f32 = 110.0;
const BAR: f32 = 110.0;
/// The width of each of the two columns.
const COLUMN: f32 = 500.0;

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
    /// The joysticks connected, and when they were last looked for.
    joysticks: Joysticks,
    scanned: Instant,
    /// The wheels whose rotation can be set, as last read, or why the last set failed.
    wheels: Result<Vec<Wheel>, String>,
    /// The last "Import from RSF"'s summary.
    ffb_status: Option<String>,
}

impl ControlsPage {
    pub fn new() -> ControlsPage {
        ControlsPage {
            capture: None,
            offered: false,
            joysticks: Joysticks::connected(),
            scanned: Instant::now(),
            wheels: Ok(wheel_rotation::wheels()),
            ffb_status: None,
        }
    }

    /// Whether a binding is being set: the keys go to it.
    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Whether a joystick is connected.
    pub fn has_pads(&self) -> bool {
        !self.joysticks.is_empty()
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

    /// Reads the joysticks, and looks for new ones unless the viewer is `running`; while a device binding is being
    /// set, the first button pressed or axis moved sets it.
    pub fn poll(&mut self, bindings: &mut Bindings, running: bool) {
        if self.capture.is_none() && !running && self.scanned.elapsed() >= RESCAN {
            self.joysticks.rescan();
            self.scanned = Instant::now();
        }
        self.joysticks.read();
        let Some(Capture::Pad(action, rest)) = &self.capture else { return };
        let action = *action;
        let steering = action == Action::SteerLeft;
        let threshold = if steering { bindings::STEER_MOVE } else { bindings::PRESS };
        let mut moved = None;
        for (k, rest) in rest.iter().enumerate().take(self.joysticks.len()) {
            let (axes, buttons) = self.joysticks.objects(k);
            let now = values(self.joysticks.state(k), axes, buttons);
            let device = self.joysticks.device(k);
            // The steering takes an axis only.
            let found = now.zip(rest).find(|((object, v), (_, r))| {
                (!steering || matches!(object, Object::Axis(_))) && (v - r).abs() > threshold
            });
            if let Some(((object, v), (_, r))) = found {
                moved = Some(Moved { input: PadInput { device, object }, up: v > *r });
                break;
            }
        }
        if let Some(moved) = moved {
            bind(bindings, action, moved);
            self.capture = None;
        }
    }

    /// Each joystick's values now.
    fn pad_rest(&self) -> Vec<Vec<(Object, f32)>> {
        (0..self.joysticks.len())
            .map(|k| {
                let (axes, buttons) = self.joysticks.objects(k);
                values(self.joysticks.state(k), axes, buttons).collect()
            })
            .collect()
    }

    /// The value of a device input now, -1..1 for an axis, 0 or 1 for a button.
    fn read(&self, pad: PadInput) -> Option<f32> {
        let k = (0..self.joysticks.len()).find(|&k| self.joysticks.device(k) == pad.device)?;
        let (axes, buttons) = self.joysticks.objects(k);
        values(self.joysticks.state(k), axes, buttons).find(|(o, _)| *o == pad.object).map(|(_, v)| v)
    }

    /// The joysticks connected and the names their bindings show ([`short_name`]).
    fn connected(&self) -> Vec<(Device, String)> {
        (0..self.joysticks.len()).map(|k| (self.joysticks.device(k), short_name(self.joysticks.name(k)))).collect()
    }

    /// The joysticks connected: name and id.
    fn pads(&self) -> Vec<String> {
        (0..self.joysticks.len())
            .map(|k| format!("{} ({})", self.joysticks.name(k), self.joysticks.device(k).format()))
            .collect()
    }

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
        root: &str,
        settings: &Path,
        read_only: bool,
    ) {
        let pads = self.pads();
        if pads.is_empty() {
            ui.label(
                "No wheel or gamepad found: connect it and it shows here. Until then the keyboard drives: its keys are \
                 under Keyboard below.",
            );
        } else {
            ui.label(format!("Connected: {}", pads.join(", ")));
        }
        ui.add_space(10.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(COLUMN);
                    heading(ui, "Wheel and pedals");
                    self.pad_rows(ui, bindings, "wheel", WHEEL);
                    invert_ui(ui, bindings);
                    self.rotation_ui(ui, ffb);
                    heading(ui, "Wheel buttons");
                    self.pad_rows(ui, bindings, "buttons", BUTTONS);
                });
                ui.add_space(24.0);
                ui.vertical(|ui| {
                    ui.set_width(COLUMN);
                    heading(ui, "Shifter");
                    self.pad_rows(ui, bindings, "shifter-rows", SHIFTER);
                    ui.label(
                        RichText::new("With gears bound and Neutral not, letting go of the H-shifter selects neutral.")
                            .color(Color32::GRAY),
                    );
                    shifter_ui(ui, bindings);
                    heading(ui, "Driver aids");
                    aids_ui(ui, aids, head_gasket);
                    heading(ui, "Force feedback");
                    ffb_ui(ui, ffb, settings, read_only);
                    let global = ffb.sensitivity;
                    let on = ffb.settings.enabled;
                    let status = &mut self.ffb_status;
                    ffb_cars_ui(ui, on, global, ffb_cars, ffb_load, cars, car, root, settings, read_only, status);
                });
            });
            ui.add_space(8.0);
            egui::CollapsingHeader::new(RichText::new("Keyboard").size(16.5).strong())
                .default_open(false)
                .show(ui, |ui| self.keys_ui(ui, bindings));
            ui.add_space(8.0);
            if ui.button("Reset all controls to defaults").clicked() {
                *bindings = Bindings::default();
                self.capture = None;
            }
        });
    }

    /// A section's device rows.
    fn pad_rows<const N: usize>(&mut self, ui: &mut Ui, bindings: &mut Bindings, id: &str, rows: [(Action, &str); N]) {
        let connected = self.connected();
        egui::Grid::new(id).num_columns(4).min_col_width(ROW_LABEL).spacing([12.0, 4.0]).show(ui, |ui| {
            for (action, label) in rows {
                self.pad_row(ui, bindings, action, label, &connected);
            }
        });
    }

    /// An action's device row: its input (click to set it; named by `connected`), its live value and a clear.
    fn pad_row(
        &mut self,
        ui: &mut Ui,
        bindings: &mut Bindings,
        action: Action,
        label: &str,
        connected: &[(Device, String)],
    ) {
        ui.label(label);
        let binding = bindings.get(action);
        let text = match &self.capture {
            Some(Capture::Pad(a, _)) if *a == action => {
                if action == Action::SteerLeft { "turn the wheel left…" } else { "move it…" }.to_owned()
            }
            _ if action == Action::SteerLeft => steering_label(bindings, connected),
            _ => binding.pad.map_or("-".to_owned(), |p| pad_label(p, connected)),
        };
        let button = egui::Button::new(text.clone()).truncate().min_size(egui::vec2(190.0, 0.0));
        if ui.add_sized([190.0, 18.0], button).on_hover_text(text).clicked() {
            self.capture = Some(Capture::Pad(action, self.pad_rest()));
        }
        match binding.pad.and_then(|p| self.read(p)) {
            Some(value) => {
                let value = live_value(bindings, action, value);
                let bar = match action {
                    Action::SteerLeft => ProgressBar::new((value + 1.0) / 2.0).text(format!("{value:+.2}")),
                    _ => ProgressBar::new(value),
                };
                ui.add(bar.desired_width(BAR));
            }
            None => {
                ui.add_sized([BAR, 0.0], egui::Label::new(""));
            }
        }
        if ui.small_button("clear").clicked() {
            bindings.actions[action as usize].pad = None;
            if action == Action::SteerLeft {
                bindings.actions[Action::SteerRight as usize].pad = None;
            }
        }
        ui.end_row();
    }

    /// Each action's key, and a clear.
    fn keys_ui(&mut self, ui: &mut Ui, bindings: &mut Bindings) {
        egui::Grid::new("keys").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
            for action in Action::ALL {
                ui.label(action.name());
                let text = match self.capture {
                    Some(Capture::Key(a)) if a == action => "press a key (Esc cancels)",
                    _ => bindings.get(action).key.map_or("-", bindings::key_name),
                };
                if ui.button(text).clicked() {
                    self.capture = Some(Capture::Key(action));
                }
                if ui.small_button("clear").clicked() {
                    bindings.actions[action as usize].key = None;
                }
                ui.end_row();
            }
        });
    }

    /// The wheel's lock-to-lock rotation: off, fixed, or per car (each car's own up to the degrees). Set on the wheel
    /// when changed, and by the viewer when a drive starts.
    fn rotation_ui(&mut self, ui: &mut Ui, ffb: &mut ffb::Options) {
        ui.horizontal(|ui| {
            ui.add_sized([ROW_LABEL, 18.0], egui::Label::new("Rotation")).on_hover_text(ROTATION_HELP);
            const MODES: [(Option<bool>, &str); 3] = [(None, "off"), (Some(false), "fixed"), (Some(true), "per car")];
            let mut mode = ffb.rotation.map(|_| ffb.rotation_per_car);
            let name = MODES.iter().find(|(m, _)| *m == mode).map_or("", |(_, n)| n);
            let mut changed = false;
            ComboBox::from_id_salt("rotation").selected_text(name).show_ui(ui, |ui| {
                for (m, n) in MODES {
                    changed |= ui.selectable_value(&mut mode, m, n).changed();
                }
            });
            let mut degrees = ffb.rotation.unwrap_or(900);
            if let Some(per_car) = mode {
                ui.label(if per_car { "up to" } else { "lock to lock" });
                let r = ui.add(DragValue::new(&mut degrees).range(wheel_rotation::DEGREES).speed(5).suffix("°"));
                changed |= (r.changed() && !r.dragged()) || r.drag_stopped();
                ffb.rotation_per_car = per_car;
            }
            ffb.rotation = mode.map(|_| degrees);
            if let Some(degrees) = ffb.rotation.filter(|_| changed) {
                self.wheels = wheel_rotation::set(degrees);
            }
        });
        let status = match &self.wheels {
            Ok(wheels) if wheels.is_empty() => "No wheel whose rotation can be set.".to_owned(),
            Ok(wheels) => wheels.iter().map(|w| format!("{}: {}°", w.name, w.degrees)).collect::<Vec<_>>().join(", "),
            Err(e) => e.clone(),
        };
        ui.label(RichText::new(status).color(Color32::GRAY));
    }
}

/// The pedals' and the steering's inverts (`input.ini`'s `Invert*Axis`), which the set-up sets.
fn invert_ui(ui: &mut Ui, bindings: &mut Bindings) {
    ui.horizontal(|ui| {
        ui.add_sized([ROW_LABEL, 18.0], egui::Label::new("Invert")).on_hover_text(INVERT_HELP);
        for (action, label) in [
            (Action::SteerLeft, "steering"),
            (Action::Throttle, "throttle"),
            (Action::Brake, "brake"),
            (Action::Clutch, "clutch"),
            (Action::Handbrake, "handbrake"),
        ] {
            if let Some(flag) = invert_flag(&mut bindings.invert, action) {
                ui.checkbox(flag, label).on_hover_text(INVERT_HELP);
            }
        }
    });
}

/// What the inverts do.
const INVERT_HELP: &str = "Flips an axis, as RBR's input.ini does: for a pedal that rests at the other end, or a \
     wheel that turns the other way. Binding an axis sets it.";

/// A section's heading, as the other pages have them.
fn heading(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).size(16.5).strong());
    ui.add_space(2.0);
}

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
const ROTATION_HELP: &str = "How far the wheel turns from lock to lock. The wheel stops there and full lock is \
     reached there. Fixed: the degrees for every car. Per car: each car's steering lock (its setup's MaxSteeringLock \
     x 720), up to the degrees. Off leaves the wheel as it is. Linux wheel drivers only (the wheel tools' udev rule \
     must allow it).";

fn shifter_ui(ui: &mut Ui, bindings: &mut Bindings) {
    let shifter = &mut bindings.shifter;
    ui.checkbox(&mut shifter.advanced, "Advanced H-shifter: a gear needs the clutch or matched revs").on_hover_text(
        "With gears 1-6 and reverse bound to the shifter. Off: the gears select at once, as NGP has them. \
         The Drive page's Gearbox picks each car's: an H-pattern car drives without Clutch Help and the automatic \
         gearbox, a sequential one (every car until picked) keeps both and shifts as before. The grind plays \
         RBRControls' Shifter_Error.wav from the RSF folder.",
    );
    if !shifter.advanced {
        return;
    }
    egui::Grid::new("shifter").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label("Clutch threshold");
        ui.add(egui::Slider::new(&mut shifter.clutch_threshold, 0.0..=0.95))
            .on_hover_text("How far the clutch pedal must be pressed to count as in");
        ui.end_row();
    });
}

/// NGP's `headGasketDurability` with RSF's "Engine head gasket durability fix" on, as its install has it.
const HEAD_GASKET_FIX: i32 = 10;

/// What the head gasket fix does.
const HEAD_GASKET_HELP: &str = "RSF's fix: the head gasket wears 10 times slower, so realistic damage no longer kills \
     the engine unexpectedly. Off is NGP's default.";

/// The game's gearbox aids (its English labels, `bstrings.eng`) and "Game defaults", then the head gasket fix (NGP's
/// `headGasketDurability`, 1 off). A car with an H-pattern gearbox under the advanced H-shifter drives without the
/// automatic gearbox and clutch help.
fn aids_ui(ui: &mut Ui, aids: &mut DriverAids, head_gasket: &mut i32) {
    const H_PATTERN: &str = "; off for a car whose gearbox is H-pattern under the advanced H-shifter";
    egui::Grid::new("aids").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
        let rows = [
            ("Automatic gearbox", &mut aids.auto_gears, format!("Shifts up and down for you{H_PATTERN}")),
            ("Gear Protection", &mut aids.gear_guard, "Stops a shift into a gear that would over-rev".into()),
            ("Neutral Lock", &mut aids.neutral_lock, "Keeps the gearbox from slipping into neutral".into()),
            ("Clutch Help", &mut aids.clutch_help, format!("Works the clutch for you when you shift{H_PATTERN}")),
        ];
        for (n, (label, value, tip)) in rows.into_iter().enumerate() {
            ui.checkbox(value, label).on_hover_text(tip);
            if n % 2 == 1 {
                ui.end_row();
            }
        }
    });
    if ui.small_button("Game defaults").on_hover_text("The game's driver aids").clicked() {
        *aids = DriverAids::default();
    }
    let mut fix = *head_gasket > 1;
    if ui.checkbox(&mut fix, "Head gasket durability fix").on_hover_text(HEAD_GASKET_HELP).changed() {
        *head_gasket = if fix { HEAD_GASKET_FIX } else { 1 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_render::bindings::Device;

    const G923: Device = Device { vendor: 0x046d, product: 0xc266, index: 0 };

    /// The per-car hover: each surface with 20 s of driving, at the car's own sensitivity there.
    #[test]
    fn the_per_car_hover_says_how_often_the_wheel_maxed_out() {
        let loads = ffb::Loads::parse(
            "[ffb_load]\nrsf-81 gravel = 0,0,0,0,0,0,0,0,200\nrsf-81 tarmac = 0,300\nrsf-81 snow = 10\n",
        );
        // x 42.5 maxes out at 30 %, not at 100 %; snow has 1 s only.
        assert_eq!((maxed_out(&loads, "rsf-81", 0, 30), maxed_out(&loads, "rsf-81", 1, 100)), (Some(100), Some(0)));
        assert_eq!((maxed_out(&loads, "rsf-81", 2, 100), maxed_out(&loads, "rsf-1", 0, 100)), (None, None));
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
}
