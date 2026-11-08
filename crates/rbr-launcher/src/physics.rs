//! The physics the launcher drives with (Settings page): NGP 7's, or ReRBR experimental physics (built on NGP 7),
//! `rbr-standalone --physics`. The brush tyre in development is not offered there: `rerbr --physics
//! experimental-brush` drives it for one session, unsaved.

use egui::Ui;

use crate::catalog::Car;
use crate::pages::{combo, fields, section, Choice};

/// The physics every drive runs (`[launcher] physics`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PhysicsProfile {
    /// NGP 7's, as the original drives.
    #[default]
    Ngp,
    /// ReRBR experimental physics, built on NGP 7: its engine model (`rbr_engine`).
    Experimental,
    /// The same with the brush tyre in development (`rbr_tyre`): only from `rerbr --physics experimental-brush`, never
    /// offered or saved.
    ExperimentalBrush,
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
            PhysicsProfile::Experimental | PhysicsProfile::ExperimentalBrush => {
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

    /// `rerbr --physics NAME`'s profile: any of them, the brush tyre included.
    pub fn from_flag(name: &str) -> Option<Self> {
        [PhysicsProfile::Ngp, PhysicsProfile::Experimental, PhysicsProfile::ExperimentalBrush]
            .into_iter()
            .find(|p| p.name() == name)
    }
}

impl Choice for PhysicsProfile {
    const ALL: &[Self] = &[PhysicsProfile::Ngp, PhysicsProfile::Experimental];
    fn name(self) -> &'static str {
        match self {
            PhysicsProfile::Ngp => "ngp",
            PhysicsProfile::Experimental => "experimental",
            PhysicsProfile::ExperimentalBrush => "experimental-brush",
        }
    }
    fn label(self) -> &'static str {
        match self {
            PhysicsProfile::Ngp => "NGP 7 (original)",
            PhysicsProfile::Experimental => "ReRBR experimental (built on NGP 7)",
            PhysicsProfile::ExperimentalBrush => "ReRBR experimental with the brush tyre (in development)",
        }
    }
}

/// The Settings page's Physics row: the choice, the link to its explanation, the session's `rerbr --physics` when it
/// overrides the choice, and while the experimental physics drives what it leaves unsaved.
pub fn physics_section(ui: &mut Ui, physics: &mut PhysicsProfile, session: Option<PhysicsProfile>) {
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
        if let Some(session) = session {
            ui.label("");
            ui.weak(format!("This session: {} (rerbr --physics {})", session.label(), session.name()));
            ui.end_row();
        }
        if !session.unwrap_or(*physics).keeps_records() {
            ui.label("");
            ui.weak(NO_RECORDS);
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::PhysicsProfile;
    use crate::catalog::Car;
    use crate::pages::Choice;

    /// The brush tyre comes only from `rerbr --physics`: the Settings page neither offers nor reads it back.
    #[test]
    fn the_brush_tyre_is_a_session_flag_only() {
        assert_eq!(PhysicsProfile::from_flag("experimental-brush"), Some(PhysicsProfile::ExperimentalBrush));
        assert_eq!(PhysicsProfile::from_flag("ngp"), Some(PhysicsProfile::Ngp));
        assert_eq!(PhysicsProfile::from_flag("experimental"), Some(PhysicsProfile::Experimental));
        assert_eq!(PhysicsProfile::from_flag("brush"), None);
        assert!(!PhysicsProfile::ALL.contains(&PhysicsProfile::ExperimentalBrush));
        assert_eq!(PhysicsProfile::parse("experimental-brush"), None);
        assert!(!PhysicsProfile::ExperimentalBrush.keeps_records());
        let car = Car { group: Some("Rally 2".into()), year: Some(2019), ..Car::default() };
        let args = ["--physics", "experimental-brush", "--car-group", "Rally 2", "--car-year", "2019"];
        assert_eq!(PhysicsProfile::ExperimentalBrush.args(&car), args.map(String::from));
    }
}
