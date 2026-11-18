//! The Settings section's General page: what the player needs to drive (the game folder, NGP, the cars, the wheel),
//! the physics, the telemetry, the advanced paths and the programs.

use std::path::PathBuf;

use egui::{Align, Layout, RichText};

use crate::disc_install::DiscSetup;
use crate::get_page::GetMore;
use crate::kit::{self, Rows};
use crate::pages::{Page, NGP_WHY, NO_NGP};
use crate::theme::{GOOD, TEXT_2, TEXT_3, WARN};
use crate::{catalog, controls_page, install, job, settings, Form};

/// The width of a status row's label.
const LABEL: f32 = 84.0;

/// A label of a fixed `width`, so the rows' next cells line up.
pub fn label_cell(ui: &mut egui::Ui, width: f32, text: &str) -> egui::Response {
    ui.allocate_ui_with_layout(egui::vec2(width, 18.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(egui::vec2(width, 18.0));
        ui.add(egui::Label::new(RichText::new(text).color(crate::theme::TEXT_LABEL)).truncate())
    })
    .inner
}

/// [`Rows::row`] with `tip` on the label's hover.
pub fn row_tip<R>(rows: &mut Rows, label: &str, tip: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    rows.full(|ui| {
        ui.label(RichText::new(label).color(crate::theme::TEXT_LABEL)).on_hover_text(tip);
        ui.with_layout(Layout::right_to_left(Align::Center), control).inner
    })
}

/// A note in a panel row: wrapping, 12, in `color`.
pub fn note(rows: &mut Rows, color: egui::Color32, text: &str) {
    rows.full(|ui| {
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(color)).wrap());
    });
}

/// A status row: `label`, ✔ or a warning, `what` beside it, and `actions` right-aligned (added right to left).
fn status_row(
    rows: &mut Rows,
    label: &str,
    ok: bool,
    what: impl FnOnce(&mut egui::Ui),
    actions: impl FnOnce(&mut egui::Ui),
) {
    rows.full(|ui| {
        label_cell(ui, LABEL, label);
        ui.label(RichText::new(if ok { "✔" } else { "⚠" }).color(if ok { GOOD } else { WARN }));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            actions(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), what);
        });
    });
}

impl Form {
    /// What the player needs to drive, a row each with its next step beside it: the game folder, NGP, the cars and the
    /// wheel; beside them the physics, below the UDP telemetry, the advanced fields (NGP's DLL, the paths that
    /// override the folder's parts) and, pinned, the version and the files' folder.
    pub fn settings_page(&mut self, ui: &mut egui::Ui) {
        kit::footer(ui, "settings footer", |ui| {
            ui.horizontal(|ui| {
                let here = self.here();
                ui.label(RichText::new(format!("ReRBR {}", env!("CARGO_PKG_VERSION"))).size(12.0).color(TEXT_2));
                ui.label(RichText::new("·").color(TEXT_3));
                ui.label(RichText::new("Your files:").size(12.0).color(TEXT_2));
                ui.label(RichText::new(here.display().to_string()).font(crate::theme::mono(11.5)).color(TEXT_2))
                    .on_hover_text("Settings, saves, records, ghosts, setups and added cars");
                if kit::link(ui, "Open").clicked() {
                    crate::crash::open(here.as_os_str());
                }
            });
        });
        egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]).show(ui, |ui| {
            kit::body(ui, |ui| {
                ui.columns(2, |columns| {
                    self.game_column(&mut columns[0]);
                    self.physics_column(&mut columns[1]);
                });
            })
        });
    }

    /// The left column: the readiness rows, the telemetry and the advanced fields.
    fn game_column(&mut self, ui: &mut egui::Ui) {
        let running = self.child.is_some();
        // What Start needs, a row each: its heading says whether something still blocks it.
        kit::group_label(ui, if self.readiness().is_none() { "Ready to drive" } else { "Before you can drive" }, None);

        // The game folder, and the disc setup under it.
        let mut set = false;
        let (mut download, mut get_cars, mut to_controls) = (false, false, false);
        let has_ngp = self.ngp_revision.is_some();
        let root = self.game_root();
        let dll_problem = self.ngp_dll.problem(&self.data, &root);
        let ngp_ok = has_ngp && dll_problem.is_none();
        let can_download = self.can_download_ngp();
        let cars = self.catalog.cars.iter().filter(|c| c.unavailable().is_none());
        let (mut from_rsf, mut added, mut from_ngp) = (0, 0, 0);
        for car in cars {
            match &car.physics {
                catalog::Physics::Pack(_) if car.added => added += 1,
                catalog::Physics::Pack(_) => from_rsf += 1,
                catalog::Physics::Archive => from_ngp += 1,
            }
        }
        let count = [(from_rsf, "from RallySimFans"), (added, "added"), (from_ngp, "from NGP")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, from)| format!("{n} {from}"))
            .collect::<Vec<_>>();
        let has_cars = !count.is_empty();
        let pads = self.controls.has_pads();
        let set_up = !controls_page::unbound(&self.bindings);
        let names = self.controls.pad_names().join(", ");

        kit::panel(ui, |rows| {
            rows.full(|ui| {
                label_cell(ui, LABEL, "Game folder");
                ui.label(RichText::new("✔").color(GOOD));
                if running {
                    ui.disable();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Browse…").clicked() {
                        if let Some(dir) = job::pick_folder("Your Richard Burns Rally folder") {
                            self.install_folder = dir;
                            set = true;
                        }
                    }
                    let hint = "the Richard Burns Rally folder, with Cars and Maps";
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.install_folder)
                            .hint_text(hint)
                            .font(crate::theme::mono(12.0))
                            .desired_width(ui.available_width()),
                    );
                    set |= edit.lost_focus();
                });
            });
            status_row(
                rows,
                "NGP",
                ngp_ok,
                |ui| {
                    ui.add(
                        egui::Label::new(match (has_ngp, &dll_problem) {
                            (true, None) => self.ngp_label.as_str(),
                            (true, Some(problem)) => problem.as_str(),
                            (false, _) => NO_NGP,
                        })
                        .wrap(),
                    );
                },
                |ui| download = can_download && ui.button("Download NGP 7.4.778…").clicked(),
            );
            if !ngp_ok {
                rows.hint(NGP_WHY);
            }
            status_row(
                rows,
                "Cars",
                has_cars,
                |ui| {
                    let text = if has_cars { count.join(", ") } else { "None yet".to_owned() };
                    ui.add(egui::Label::new(text).wrap());
                },
                |ui| {
                    get_cars =
                        ui.button("Get cars…").on_hover_text("gvrc.de's NGP car packs, on Library > Cars").clicked()
                },
            );
            status_row(
                rows,
                "Wheel",
                !pads || set_up,
                |ui| {
                    ui.add(
                        egui::Label::new(match (pads, set_up) {
                            (false, _) => "None connected: the keyboard drives (its keys are on Controls)".to_owned(),
                            (true, true) => format!("{names}, bound"),
                            (true, false) => format!("{names}: connected, not bound yet"),
                        })
                        .wrap(),
                    );
                },
                |ui| {
                    let label = if pads && !set_up { "Bind it on Controls" } else { "Controls…" };
                    to_controls = ui.button(label).clicked();
                },
            );
        });
        if download {
            self.ask_download_ngp();
        }
        if get_cars {
            self.open_get_more(GetMore::Cars);
        }
        if to_controls {
            self.page = Page::Controls;
        }
        self.ngp_download_ui(ui);
        ui.add_space(4.0);
        kit::hint(ui, READ_ONLY);
        ui.add_space(2.0);
        ui.add_enabled_ui(!running, |ui| self.disc_setup.open |= kit::link(ui, "Set up from the disc…").clicked());
        if let Some(dir) = ui.add_enabled_ui(!running, |ui| self.disc_setup.ui(ui)).inner {
            self.install_folder = dir;
            set = true;
        }
        if set && install::Folder::read(&self.install_folder) != self.folder {
            self.reload = true;
        }

        // NGP's UDP telemetry, for dashboards and motion rigs.
        ui.add_space(14.0);
        kit::group_label(ui, "Telemetry", None);
        kit::panel(ui, |rows| {
            let mut on = !self.telemetry_udp.trim().is_empty();
            let sending = rows.switch_row("Send NGP telemetry", Some("UDP, for dashboards and motion rigs"), &mut on);
            if sending.changed() {
                self.telemetry_udp = if on { "127.0.0.1:6776".to_owned() } else { String::new() };
            }
            if on {
                rows.row("Address", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.telemetry_udp)
                            .font(crate::theme::mono(12.0))
                            .desired_width(180.0),
                    );
                });
                match settings::udp_address(&self.telemetry_udp) {
                    Some(to) => rows.hint(&format!("Each drive sends NGP's telemetry packet to {to}")),
                    None => note(rows, WARN, "Not sent: give an address and a port, ADDR:PORT"),
                }
            }
        });

        ui.add_space(14.0);
        egui::CollapsingHeader::new(RichText::new("Advanced").size(12.0).family(crate::theme::medium()).color(TEXT_2))
            .show(ui, |ui| {
                kit::panel(ui, |rows| {
                    self.ngp_dll.ui(rows, &self.data, &root, !running);
                    let hint = |path: &Option<PathBuf>, none: &str| {
                        path.as_ref().map_or(none.to_owned(), |p| p.display().to_string())
                    };
                    rows.full(|ui| {
                        label_cell(ui, 110.0, "NGP physics.rbz");
                        let ngp = hint(&self.folder.ngp_physics, "NGP's physics.rbz");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ngp_physics)
                                .hint_text(ngp)
                                .font(crate::theme::mono(12.0))
                                .desired_width(ui.available_width()),
                        );
                    });
                    rows.full(|ui| {
                        label_cell(ui, 110.0, "Game data");
                        ui.label(
                            RichText::new(self.paths.data.display().to_string())
                                .font(crate::theme::mono(11.5))
                                .color(TEXT_2),
                        );
                    });
                    for (label, path) in [("Viewer", &self.paths.viewer), ("Physics program", &self.paths.standalone)] {
                        let ok = path.is_file();
                        rows.full(|ui| {
                            label_cell(ui, 110.0, label);
                            ui.label(RichText::new(if ok { "✔" } else { "✖" }).color(if ok { GOOD } else { WARN }));
                            ui.label(
                                RichText::new(path.display().to_string()).font(crate::theme::mono(11.5)).color(TEXT_2),
                            );
                        });
                    }
                });
                ui.add_space(4.0);
                kit::hint(
                    ui,
                    "An empty field takes the game folder's. --data (or RBR_DATA) sets the game data, RBR_RSF is an \
                     unsaved developer override for it. rerbr --help lists the options.",
                );
            });
    }

    /// The right column: the physics and the force feedback from the tyres.
    fn physics_column(&mut self, ui: &mut egui::Ui) {
        crate::physics::experimental_section(ui, &mut self.physics, self.session_physics, &mut self.ffb.tyres);
    }
}

/// What ReRBR does with the game folder, under its field.
const READ_ONLY: &str = "ReRBR only reads your game folder and never writes to it: its settings, saves and downloads \
                         go to your files' folder.";

/// The game folder's field, its Browse button and the disc setup's, and [`READ_ONLY`]; whether a folder was set
/// (Enter, the field left, picked, or set up from the disc).
pub fn folder_row(ui: &mut egui::Ui, folder: &mut String, setup: &mut DiscSetup) -> bool {
    let set = ui
        .horizontal(|ui| {
            ui.label("Game folder");
            let hint = "the Richard Burns Rally folder, with Cars and Maps";
            let edit = ui.add(egui::TextEdit::singleline(folder).hint_text(hint).desired_width(420.0));
            let picked = ui.button("Browse…").clicked()
                && job::pick_folder("Your Richard Burns Rally folder").map(|dir| *folder = dir).is_some();
            setup.open |= ui.button("Set up from the disc…").clicked();
            picked || edit.lost_focus()
        })
        .inner;
    ui.weak(READ_ONLY);
    set | setup.ui(ui).map(|dir| *folder = dir).is_some()
}
