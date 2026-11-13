//! The physics the launcher drives with (Settings page, Experimental): NGP 7's, or ReRBR's experimental engine and
//! brush tyre over it, each switched on its own (`rbr-standalone --physics`); `rerbr --physics NAME` drives one for a
//! session, unsaved.

use egui::Ui;

use crate::catalog::Car;
use crate::pages::{fields, section, Choice};

/// The physics every drive runs (`[launcher] physics`): the Settings page's experimental engine and tyre switches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PhysicsProfile {
    /// NGP 7's, as the original drives.
    #[default]
    Ngp,
    /// ReRBR experimental physics, built on NGP 7: its engine model (`rbr_engine`), aero and four-wheel-drive
    /// drivetrain loss (`rbr_standalone`'s `Profile::EXPERIMENTAL`).
    Experimental,
    /// The same with the brush tyre in development (`rbr_tyre`).
    ExperimentalBrush,
    /// NGP 7's with the brush tyre.
    NgpBrush,
}

/// Where each experimental switch is explained.
const DOCS: &str = "https://github.com/JodusNodus/rerbr/blob/main/docs/";
const ENGINE_PAGE: &str = "experimental-physics.md";
const TYRE_PAGE: &str = "tyre-model.md";
const FFB_PAGE: &str = "force-feedback.md";

/// What the experimental physics leaves unsaved, shown beside the choice while it is picked.
pub const NO_RECORDS: &str = "Experimental: no records, ghosts or championship results are saved.";

impl PhysicsProfile {
    /// `rbr-standalone`'s arguments for it on `car`: none for NGP 7's, so its command lines stay as they were; else
    /// the profile and the car's group and year where the catalog has them.
    pub fn args(self, car: &Car) -> Vec<String> {
        if self == PhysicsProfile::Ngp {
            return Vec::new();
        }
        let group = car.group.iter().flat_map(|g| ["--car-group".to_owned(), g.clone()]);
        let year = car.year.iter().flat_map(|y| ["--car-year".to_owned(), y.to_string()]);
        ["--physics".to_owned(), self.name().to_owned()].into_iter().chain(group).chain(year).collect()
    }

    /// Whether a run on it keeps records (ghosts, the stage record and personal best, championship results): NGP 7's
    /// only, while ReRBR's is experimental (a user decision; `rbr-standalone` writes no records then either).
    pub fn keeps_records(self) -> bool {
        self == PhysicsProfile::Ngp
    }

    /// `rerbr --physics NAME`'s profile.
    pub fn from_flag(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.name() == name)
    }

    /// Whether it runs the experimental engine (with its aero and drivetrain loss), and the brush tyre.
    pub fn parts(self) -> (bool, bool) {
        match self {
            PhysicsProfile::Ngp => (false, false),
            PhysicsProfile::Experimental => (true, false),
            PhysicsProfile::ExperimentalBrush => (true, true),
            PhysicsProfile::NgpBrush => (false, true),
        }
    }

    /// The profile with the experimental `engine` and the brush `tyre`, each on or off.
    pub fn of_parts(engine: bool, tyre: bool) -> Self {
        Self::ALL.iter().copied().find(|p| p.parts() == (engine, tyre)).unwrap_or_default()
    }
}

impl Choice for PhysicsProfile {
    const ALL: &[Self] = &[
        PhysicsProfile::Ngp,
        PhysicsProfile::Experimental,
        PhysicsProfile::ExperimentalBrush,
        PhysicsProfile::NgpBrush,
    ];
    fn name(self) -> &'static str {
        match self {
            PhysicsProfile::Ngp => "ngp",
            PhysicsProfile::Experimental => "experimental",
            PhysicsProfile::ExperimentalBrush => "experimental-brush",
            PhysicsProfile::NgpBrush => "ngp-brush",
        }
    }
    fn label(self) -> &'static str {
        match self {
            PhysicsProfile::Ngp => "NGP 7 (original)",
            PhysicsProfile::Experimental => "experimental engine",
            PhysicsProfile::ExperimentalBrush => "experimental engine and tyre",
            PhysicsProfile::NgpBrush => "experimental tyre",
        }
    }
}

/// The Settings page's Experimental section: a switch each for the experimental engine (with its aero and
/// drivetrain), the brush tyre and the force feedback from the tyres (`tyres`, `[ffb] tyres`), each with a link to its
/// page; the session's `rerbr --physics` when it overrides the switches, and while the experimental physics drives
/// what it leaves unsaved.
pub fn experimental_section(
    ui: &mut Ui,
    physics: &mut PhysicsProfile,
    session: Option<PhysicsProfile>,
    tyres: &mut bool,
) {
    section(ui, "Experimental");
    fields(ui, "experimental", |ui| {
        let (mut engine, mut tyre) = physics.parts();
        switch(ui, "Engine", &mut engine, "ReRBR's engine model, aero and drivetrain", ENGINE_HELP, ENGINE_PAGE);
        switch(ui, "Tyre", &mut tyre, "ReRBR's brush tyre (in development)", TYRE_HELP, TYRE_PAGE);
        *physics = PhysicsProfile::of_parts(engine, tyre);
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
        switch(ui, "Force feedback", tyres, "From the tyres", FFB_HELP, FFB_PAGE);
    });
}

/// A row of the section: `label`, the switch `on` with its `text` and `help`, and the link to `page`.
fn switch(ui: &mut Ui, label: &str, on: &mut bool, text: &str, help: &str, page: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.checkbox(on, text).on_hover_text(help);
        let url = format!("{DOCS}{page}");
        if ui.link("What is this?").on_hover_text(&url).clicked() {
            crate::crash::open(url.as_ref());
        }
    });
    ui.end_row();
}

const ENGINE_HELP: &str = "ReRBR's engine model, aero and four-wheel-drive drivetrain loss, in place of NGP 7's.";
const TYRE_HELP: &str = "ReRBR's brush tyre, in place of NGP 7's. Still in development.";
const FFB_HELP: &str = "The wheel's force from the front tyres' grip, as Assetto Corsa EVO's, mapped straight: it \
                        goes light as the front lets go. Off: NGP's force and curve. Records are kept either way; the \
                        sensitivity may want a new value.";

#[cfg(test)]
mod tests {
    use super::PhysicsProfile;
    use crate::catalog::Car;
    use crate::pages::Choice;

    /// Each switch pair is one profile, saved and passed by its name.
    #[test]
    fn the_switches_name_a_profile() {
        for profile in PhysicsProfile::ALL.iter().copied() {
            let (engine, tyre) = profile.parts();
            assert_eq!(PhysicsProfile::of_parts(engine, tyre), profile);
            assert_eq!(PhysicsProfile::from_flag(profile.name()), Some(profile));
            assert_eq!(PhysicsProfile::parse(profile.name()), Some(profile));
            assert_eq!(profile.keeps_records(), profile == PhysicsProfile::Ngp);
        }
        assert_eq!(PhysicsProfile::from_flag("brush"), None);
        let car = Car { group: Some("Rally 2".into()), year: Some(2019), ..Car::default() };
        let args = ["--physics", "ngp-brush", "--car-group", "Rally 2", "--car-year", "2019"];
        assert_eq!(PhysicsProfile::NgpBrush.args(&car), args.map(String::from));
    }
}
