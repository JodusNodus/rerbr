//! What the car's group, year and turbo flag tell of its engine's hardware (the FIA articles per class mapped onto the
//! RSF group names), and a few facts by folder no rule can know (sources in `notes/engine-model.md`).

use crate::Car;

/// How the pedal moves the throttle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actuation {
    Cable,
    /// Drive-by-wire: the plate aims at a position from the pedal, behind the electronic throttle's lag.
    Dbw,
}

/// The intake's throttles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intake {
    /// One throttle body before a plenum.
    Single,
    /// A throttle per cylinder (ITBs, twin-choke carburettors, mechanical injection slides).
    PerCylinder,
}

/// What holds the idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idle {
    /// A control unit's idle air and spark loops (from 1987).
    Ecu,
    /// A fixed throttle stop (carburettors, mechanical injection): idle speed comes out of the air and friction.
    Stop,
}

/// The engine's hardware class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Class {
    pub actuation: Actuation,
    pub intake: Intake,
    pub idle: Idle,
    pub anti_lag: bool,
    /// The turbo restrictor's bore, m.
    pub restrictor: Option<f64>,
    /// The boost cap, absolute pressure over the reference atmosphere.
    pub boost_cap: Option<f64>,
    pub cylinders: u32,
    /// The geometric compression ratio.
    pub compression: f64,
}

/// The year a group's car most likely has when its metadata gives none (rule 9: the group's middle era).
fn middle_year(group: &str) -> u32 {
    match group {
        "Group 4" => 1978,
        "Group B" => 1985,
        "Group A7" | "Group A8" => 1992,
        "Group N4" => 2005,
        "Super 1600" => 2003,
        "Super 2000" => 2009,
        "WRC 2.0" => 2004,
        "WRC 1.6" => 2014,
        "Group R1" | "Group R2" | "Group R3" | "Group R4" => 2012,
        "Group R5" | "Group RGT" => 2015,
        "Rally 2" | "Rally 4" => 2021,
        "Rally 3" => 2022,
        _ => 2004,
    }
}

/// The FIA restrictor for a turbo of this group and year (Appendix J 1990–2001; FIA 254, 255, 255A, 260, 261), m.
fn restrictor(group: &str, year: u32) -> Option<f64> {
    let mm = match group {
        "Group A7" | "Group A8" => match year {
            ..=1989 => return None,
            1990..=1991 => 40.0,
            1992..=1994 => 38.0,
            _ => 34.0,
        },
        "WRC 2.0" => 34.0,
        "WRC 1.6" if year <= 2016 => 33.0,
        "WRC 1.6" => 36.0,
        // Group N as Group A's years (Appendix J), 33 mm from 2010 (RallySport Magazine 2010-01-19).
        "Group N4" | "Group R4" => match year {
            ..=1989 => return None,
            1990..=1991 => 40.0,
            1992..=1994 => 36.0,
            1995..=2009 => 32.0,
            _ => 33.0,
        },
        "Group R5" | "Rally 2" => 32.0,
        "Rally 3" if year <= 2021 => 30.0,
        "Rally 3" => 31.0,
        "Rally 4" => 30.0,
        _ => return None,
    };
    Some(mm / 1000.0)
}

/// A turbo's compression ratio by its group (`notes/engine-model.md`): Group B 7.0 (the road ur-quattro's 2.1 7.0, the
/// 205 T16's 6.5, the S1 E2's and the Delta S4's 7.5), Group A and N/R4 on their road engines 8.5 (YB, EJ20 and
/// Integrale 8.0, 3S-GTE 8.5–8.8, 4G63 8.5–9.0, EJ257 8.2, 4B11T 9.0), the rest 9.5 (the race engines' unpublished,
/// their road bases' 10.0–10.5).
fn compression(group: &str) -> f64 {
    match group {
        "Group B" => 7.0,
        "Group A7" | "Group A8" | "Group N4" | "Group R4" => 8.5,
        _ => 9.5,
    }
}

/// Facts by folder (without `_ngp6`/`_ngp7`) that the rules cannot know (tech-racingcars, FIA 254, Wikipedia;
/// `notes/engine-model.md`): cylinders where they are not 4, and the sourced exceptions to the rules.
struct Fact {
    folder: &'static str,
    cylinders: Option<u32>,
    actuation: Option<Actuation>,
    idle: Option<Idle>,
    anti_lag: Option<bool>,
}

const fn fact(folder: &'static str, cylinders: u32) -> Fact {
    Fact { folder, cylinders: Some(cylinders), actuation: None, idle: None, anti_lag: None }
}

const FACTS: [Fact; 12] = [
    fact("Audi_quattro_A2_GrpB", 5),
    fact("Porsche_911_SC_3.0_Grp4", 6),
    fact("Porsche_911_SC_RS_GrpB", 6),
    fact("Ford_Fiesta_Rally3", 3),
    fact("Ford_Fiesta_Rally3_Evo", 3),
    fact("Ford_Fiesta_Rally4", 3),
    fact("Peugeot_208_Rally4", 3),
    fact("Toyota_GR_Yaris_Rally2", 3),
    // cars.md 24: the road Evo IX's cable throttle.
    Fact {
        folder: "Mitsubishi_Lancer_Evo_IX_N4",
        cylinders: None,
        actuation: Some(Actuation::Cable),
        idle: None,
        anti_lag: None,
    },
    // cars.md 33: the road F4R 197's motorised throttle (rule 3 gives cable before 2008).
    Fact {
        folder: "Renault_Clio_III_R3",
        cylinders: None,
        actuation: Some(Actuation::Dbw),
        idle: None,
        anti_lag: None,
    },
    // cars.md 40: Bosch K-Jetronic, mechanical injection: a fixed idle stop.
    Fact {
        folder: "VW_Golf_II_GTI_16V_GrpA",
        cylinders: None,
        actuation: None,
        idle: Some(Idle::Stop),
        anti_lag: None,
    },
    // cars.md 12: an NA R2 that NGP flags turbo; an R2 has no anti-lag.
    Fact { folder: "Ford_Fiesta_R2", cylinders: None, actuation: None, idle: None, anti_lag: Some(false) },
];

fn key(folder: &str) -> &str {
    folder.strip_suffix("_ngp6").or_else(|| folder.strip_suffix("_ngp7")).unwrap_or(folder)
}

/// The class of `car` with NGP's turbo flag `turbo` (the class rules, then [`FACTS`]; `notes/engine-model.md`).
pub fn classify(car: &Car, turbo: bool) -> Class {
    let group = car.group.as_deref().unwrap_or("");
    let known = !group.is_empty();
    let year = car.year.unwrap_or_else(|| middle_year(group));
    let idle = if known && year < 1987 { Idle::Stop } else { Idle::Ecu };
    let mut class = Class {
        actuation: Actuation::Cable,
        intake: Intake::Single,
        idle,
        anti_lag: false,
        restrictor: None,
        boost_cap: None,
        cylinders: 4,
        compression: if turbo { compression(group) } else { 11.0 },
    };
    if turbo {
        match group {
            // Group B turbo, cable, no restrictor; anti-lag (recirculation) on the Audi from 1985.
            "Group B" => class.anti_lag = car.folder.starts_with("Audi") && year >= 1985,
            // Group A turbo, cable, restrictor by year, anti-lag from 1992.
            "Group A7" | "Group A8" => class.anti_lag = year >= 1992,
            // WRC, DBW from 1999, anti-lag; 2.5 bar from 2011 (FIA 255A), about 4 bar before (forum reports, guess).
            "WRC 2.0" | "WRC 1.6" => {
                class.actuation = if year >= 1999 { Actuation::Dbw } else { Actuation::Cable };
                class.anti_lag = true;
                class.boost_cap = Some(if year >= 2011 { 2.5 } else { 4.0 });
            }
            // Group N and R4 on a road base, DBW from 2008, anti-lag.
            "Group N4" | "Group R4" => {
                class.actuation = if year >= 2008 { Actuation::Dbw } else { Actuation::Cable };
                class.anti_lag = true;
            }
            // The modern turbo classes, DBW, anti-lag, 2.5 bar (Rally2: FIA 261, the pop-off valve); R1–R3 turbos (R3T,
            // NGP's turbo R2) with them.
            "Group R5" | "Rally 2" | "Group RGT" | "Group R1" | "Group R2" | "Group R3" => {
                class.actuation = Actuation::Dbw;
                class.anti_lag = true;
                class.boost_cap = Some(2.5);
            }
            // Rally3 and Rally4 have a restrictor and no boost limit (FIA 260 art. 304), as Group N and R4 (FIA 254):
            // the restrictor and the table set the boost.
            "Rally 3" | "Rally 4" => {
                class.actuation = Actuation::Dbw;
                class.anti_lag = true;
            }
            // An unknown turbo, cable without anti-lag.
            _ => {}
        }
        class.restrictor = restrictor(group, year);
    } else {
        match group {
            // Inferred: the old and the high-revving NA classes have a throttle per cylinder.
            "Group 4" | "Group B" | "Group A8" | "Super 2000" => class.intake = Intake::PerCylinder,
            "Group A7" if year >= 1993 => class.intake = Intake::PerCylinder,
            // The R classes' road engines, DBW from 2008.
            "Group R1" | "Group R2" | "Group R3" | "Rally 4" if year >= 2008 => class.actuation = Actuation::Dbw,
            _ => {}
        }
    }
    let folder = key(&car.folder);
    if let Some(f) = FACTS.iter().find(|f| f.folder == folder) {
        class.cylinders = f.cylinders.unwrap_or(class.cylinders);
        class.actuation = f.actuation.unwrap_or(class.actuation);
        class.idle = f.idle.unwrap_or(class.idle);
        class.anti_lag = f.anti_lag.unwrap_or(class.anti_lag);
    }
    class
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(folder: &str, group: &str, year: u32) -> Car {
        Car { folder: folder.into(), group: Some(group.into()), year: Some(year) }
    }

    #[test]
    fn the_rules_give_the_installed_cars_their_hardware() {
        let c = classify(&car("Ford_Fiesta_Rally2_ngp7", "Rally 2", 2023), true);
        assert_eq!(
            (c.actuation, c.anti_lag, c.restrictor, c.boost_cap),
            (Actuation::Dbw, true, Some(0.032), Some(2.5))
        );
        let c = classify(&car("Lancia_Delta_HF_4WD_GrpA_ngp6", "Group A8", 1988), true);
        assert_eq!((c.actuation, c.anti_lag, c.restrictor), (Actuation::Cable, false, None));
        let c = classify(&car("Fiat_131_Abarth_Grp4_ngp6", "Group 4", 1978), false);
        assert_eq!((c.intake, c.idle), (Intake::PerCylinder, Idle::Stop));
        let c = classify(&car("Citroen_C2_GT_S1600_ngp6", "Super 1600", 2004), false);
        assert_eq!((c.actuation, c.intake, c.idle), (Actuation::Cable, Intake::Single, Idle::Ecu));
        let c = classify(&car("Mitsubishi_Lancer_Evo_IX_N4_ngp6", "Group N4", 2011), true);
        assert_eq!((c.actuation, c.restrictor), (Actuation::Cable, Some(0.033)));
        let c = classify(&car("Ford_Fiesta_Rally3_Evo_ngp7", "Rally 3", 2023), true);
        assert_eq!((c.cylinders, c.restrictor), (3, Some(0.031)));
        let c = classify(&car("Ford_Fiesta_R2_ngp6", "Group R2", 2015), true);
        assert!(!c.anti_lag);
        let r_c = |group: &str, turbo: bool| classify(&car("x", group, 1990), turbo).compression;
        assert_eq!(
            (r_c("Group B", true), r_c("Group A8", true), r_c("Group N4", true), r_c("Rally 2", true)),
            (7.0, 8.5, 8.5, 9.5)
        );
        assert_eq!(r_c("Group A8", false), 11.0);
        assert_eq!(classify(&car("Renault_Clio_III_R3_ngp6", "Group R3", 2007), false).actuation, Actuation::Dbw);
    }

    #[test]
    fn a_car_without_metadata_goes_by_its_turbo_flag() {
        let c = classify(&Car { folder: "slot5".into(), group: None, year: None }, true);
        assert_eq!((c.actuation, c.anti_lag, c.idle, c.cylinders), (Actuation::Cable, false, Idle::Ecu, 4));
    }
}
