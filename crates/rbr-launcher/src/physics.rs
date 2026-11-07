//! The physics the launcher drives with (Settings page): NGP 7's, or ReRBR experimental physics (built on NGP 7),
//! `rbr-standalone --physics`.

use egui::Ui;

use crate::catalog::Car;
use crate::pages::{combo, fields, section, Choice};

/// The physics every drive runs (`[launcher] physics`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PhysicsProfile {
    /// NGP 7's, as the original drives.
    #[default]
    Ngp,
    /// ReRBR experimental physics, built on NGP 7: its engine model first (`rbr_engine`).
    Experimental,
}

/// Where the experimental physics is explained.
const PAGE: &str = "https://github.com/JodusNodus/rerbr/blob/main/docs/experimental-physics.md";

/// What the experimental physics leaves unsaved, shown beside the choice while it is picked.
pub const NO_RECORDS: &str = "Experimental: no records, ghosts or championship results are saved.";

impl PhysicsProfile {
    /// `rbr-standalone`'s arguments for it on `car`: none for NGP 7's, so its command lines stay as they were; else
    /// the profile and the car's group and year where the catalog has them.
    pub fn args(self, car: &Car) -> Vec<String> {
        match self {
            PhysicsProfile::Ngp => Vec::new(),
            PhysicsProfile::Experimental => {
                let group = car.group.iter().flat_map(|g| ["--car-group".to_owned(), g.clone()]);
                let year = car.year.iter().flat_map(|y| ["--car-year".to_owned(), y.to_string()]);
                ["--physics".to_owned(), self.name().to_owned()].into_iter().chain(group).chain(year).collect()
            }
        }
    }

    /// Whether a run on it keeps records (ghosts, the stage record and personal best, championship results): NGP 7's
    /// only, while ReRBR's is experimental (a user decision; `rbr-standalone` writes no records then either).
    pub fn keeps_records(self) -> bool {
        self == PhysicsProfile::Ngp
    }
}

impl Choice for PhysicsProfile {
    const ALL: &[Self] = &[PhysicsProfile::Ngp, PhysicsProfile::Experimental];
    fn name(self) -> &'static str {
        match self {
            PhysicsProfile::Ngp => "ngp",
            PhysicsProfile::Experimental => "experimental",
        }
    }
    fn label(self) -> &'static str {
        match self {
            PhysicsProfile::Ngp => "NGP 7 (original)",
            PhysicsProfile::Experimental => "ReRBR experimental (built on NGP 7)",
        }
    }
}

/// The Settings page's Physics row: the choice, the link to its explanation, and while the experimental physics is
/// picked what it leaves unsaved.
pub fn physics_section(ui: &mut Ui, physics: &mut PhysicsProfile) {
    section(ui, "Physics");
    fields(ui, "physics", |ui| {
        ui.label("Model");
        ui.horizontal(|ui| {
            combo(ui, "physics", None, physics, PhysicsProfile::ALL, PhysicsProfile::label);
            if ui.link("What is this?").on_hover_text(PAGE).clicked() {
                crate::crash::open(PAGE.as_ref());
            }
        });
        ui.end_row();
        if !physics.keeps_records() {
            ui.label("");
            ui.weak(NO_RECORDS);
            ui.end_row();
        }
    });
}
