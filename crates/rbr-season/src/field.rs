//! The AI field in the player's class (ours): entrants with a driver record and a car, driven by the 1.02 AI engine
//! (`ai::stage_time`) over the stage's generated [`Track`] ([`crate::track`]).
//!
//! The drivers are drawn by the difficulty: technique, the four material skills, speed and endurance around means that
//! rise with it, the temperament from a retail-like spread. The spreads are numbers only (read off the retail
//! `aidrv.ini`'s fourteen drivers: technique 2-5 around 3.7, speed 1-5 around 3.1, endurance 3-5 around 3.6, skills 1-5
//! around 3.3; temperaments mostly 3, then 1, 0 and 4); the names are fictional. A driver's team and car come from the
//! roster, as the 1.02 season seats two drivers to a team: the team's engineer, mechanics, budget and manager style
//! drive its service, the car's four ratings the car's stage. The stage, accident, injury, damage, false-start and
//! retirement rules are the engine's. All draws come from the CRT rand (`rbr_gameplay::crt::CrtRand`), so the seed
//! decides everything.

use rbr_formats::ai_ini::{Car, Driver, Roster, Team, Track};
use rbr_gameplay::crt::CrtRand;

use crate::ai::{self, Bulletin, CarState, DriverState, Run, RunLog, RETIRED};

/// An AI entrant.
#[derive(Debug, Clone, PartialEq)]
pub struct Entrant {
    pub name: String,
    /// The catalog car key it drives (shown in the standings), a car of the player's class.
    pub car: String,
    /// Its overall pace as the driver's record rates it, 0.91 (the weakest record) to 1 (the strongest).
    pub skill: f32,
    /// 0.55 (erratic) to 1 (a steady record): the technique.
    pub consistency: f32,
}

/// An AI entrant's result on one stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiResult {
    Time(f32),
    Retired,
}

/// What the engine carries for an entrant over the event: the driver (injuries), the car (damage), the team it drives
/// for and its false starts in the rally.
#[derive(Debug, Clone, PartialEq)]
pub struct Contender {
    /// The index into the event's teams.
    pub team: usize,
    pub driver: DriverState,
    pub car: CarState,
    pub false_starts: u8,
}

/// `aidrv.ini`'s team cars (the 1.02 season gives team `t` car `TEAM_CARS[t]`).
const TEAM_CARS: [usize; 7] = [0, 1, 3, 4, 5, 6, 7];

/// Fictional international crews (ours: nothing from the game's roster, which stays out of the source).
const NAMES: [&str; 48] = [
    "J. Lindqvist",
    "T. Moreau",
    "S. Callaghan",
    "D. Novak",
    "P. Virtanen",
    "A. Costa",
    "R. Tanaka",
    "K. Okafor",
    "E. Johansson",
    "F. Rossi",
    "N. Petrov",
    "C. Doyle",
    "L. Fontaine",
    "M. Kowalski",
    "H. Weber",
    "J. Niemi",
    "O. Bennett",
    "G. Marchetti",
    "S. Duarte",
    "A. Larsen",
    "P. Dubois",
    "T. Gallagher",
    "R. Silveira",
    "K. Aalto",
    "D. Fontaine",
    "M. Sakamoto",
    "I. Bergström",
    "C. Navarro",
    "P. Janssen",
    "V. Korhonen",
    "A. Murphy",
    "L. Bianchi",
    "E. Dumont",
    "N. Hansen",
    "R. Kowalski",
    "S. Moreau",
    "T. Virtanen",
    "D. Costa",
    "K. Fitzgerald",
    "M. Andersen",
    "J. Fontaine",
    "P. Gallagher",
    "A. Niemi",
    "R. Johansson",
    "C. Weber",
    "L. Tanaka",
    "E. Novak",
    "O. Rossi",
];

/// A draw in 0..1.
fn unit(rng: &mut CrtRand) -> f32 {
    rng.draw() as f32 / 32767.0
}

/// A draw around 0 with a standard deviation of 1 (four uniforms).
fn normal(rng: &mut CrtRand) -> f32 {
    ((unit(rng) + unit(rng) + unit(rng) + unit(rng)) - 2.0) * 1.732
}

/// An integer rating 1-5 around `mean` with deviation `sd`.
fn rating(rng: &mut CrtRand, mean: f32, sd: f32, low: i32) -> i32 {
    ((mean + sd * normal(rng)).round() as i32).clamp(low, 5)
}

/// The temperament by a retail-like spread: mostly 3, then 1, 0 and 4.
fn temperament(rng: &mut CrtRand) -> i32 {
    match (unit(rng) * 100.0) as u32 {
        0..=14 => 0,
        15..=34 => 1,
        35..=89 => 3,
        _ => 4,
    }
}

/// The driver record for `difficulty`: the means rise with it.
fn driver(name: &str, codriver: &str, difficulty: f32, rng: &mut CrtRand) -> Driver {
    let d = difficulty.clamp(0.0, 1.0);
    Driver {
        name: name.to_owned(),
        photo: 0,
        technique: rating(rng, 2.9 + 1.1 * d, 0.7, 2),
        temperament: temperament(rng),
        skills: std::array::from_fn(|_| rating(rng, 2.6 + 1.0 * d, 1.0, 1)),
        speed: rating(rng, 2.0 + 1.4 * d, 1.2, 1),
        endurance: rating(rng, 3.0 + 0.8 * d, 0.8, 2),
        condition: 9,
        history: 0,
        nationality: 0,
        codriver: codriver.to_owned(),
    }
}

/// `size` entrants for `difficulty` (0 to 1), driving `cars` (catalog keys of the class; repeated as needed), seated in
/// the roster's teams and cars.
pub fn field(
    cars: &[String],
    size: usize,
    difficulty: f32,
    seed: u32,
    roster: &Roster,
) -> (Vec<Entrant>, Vec<Contender>) {
    let mut rng = CrtRand::srand(seed);
    // Shuffle the names so the seed picks the crews.
    let mut names: Vec<&str> = NAMES.to_vec();
    for i in (1..names.len()).rev() {
        let j = (rng.draw() as usize) % (i + 1);
        names.swap(i, j);
    }
    let mut entrants = Vec::with_capacity(size);
    let mut contenders = Vec::with_capacity(size);
    for i in 0..size {
        let name = names[i % names.len()];
        let codriver = names[(i + names.len() / 2) % names.len()];
        let driver = driver(name, codriver, difficulty, &mut rng);
        let team = (i / 2) % roster.teams.len().max(1);
        let car = roster.cars.get(TEAM_CARS[team % TEAM_CARS.len()]).or(roster.cars.first()).cloned().unwrap_or(Car {
            name: String::new(),
            control: 4,
            power: 4,
            handling: 4,
            durability: 3,
        });
        let quality = (driver.technique + driver.speed + driver.skills.iter().sum::<i32>() / 4) as f32 / 15.0;
        entrants.push(Entrant {
            name: name.to_owned(),
            car: if cars.is_empty() { format!("car-{}", i + 1) } else { cars[i % cars.len()].clone() },
            skill: 0.91 + 0.09 * quality,
            consistency: 0.55 + 0.45 * driver.technique as f32 / 5.0,
        });
        contenders.push(Contender { team, driver: DriverState::new(driver), car: CarState::new(car), false_starts: 0 });
    }
    (entrants, contenders)
}

/// A rally starts: every car is repaired and the false starts are forgotten (`AiField::rally_start`).
pub fn rally_start(contenders: &mut [Contender]) {
    for c in contenders {
        c.car.parts = [0; 8];
        c.car.hits = [0; 8];
        c.false_starts = 0;
    }
}

/// One entrant's stage on `track` (the engine at the 1.02 level 2, the par as the track holds it): its time, or
/// retired. `id` names it in the bulletins; false starts count in the rally.
#[allow(clippy::too_many_arguments)]
pub fn drive_stage(
    master: &rbr_formats::ai_ini::Master,
    track: &Track,
    stage: u32,
    id: i32,
    c: &mut Contender,
    team: &Team,
    rng: &mut CrtRand,
    log: &mut RunLog,
) -> AiResult {
    let run = Run { track, level: 2, stage, extra: 0 };
    let before = c.false_starts;
    let time = ai::stage_time(master, &run, id, &mut c.driver, &mut c.car, team, &mut c.false_starts, rng, log);
    let after = c.false_starts;
    if before < after {
        log.bulletins.push(if after < 4 {
            Bulletin::FalseStart { driver: id, stage }
        } else {
            Bulletin::FalseStartRetire { driver: id, stage }
        });
    }
    if time == RETIRED || time < 0.0 {
        AiResult::Retired
    } else {
        AiResult::Time(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::fixtures;

    fn cars() -> Vec<String> {
        vec!["car-a".to_owned(), "car-b".to_owned()]
    }

    fn roster() -> Roster {
        Roster {
            drivers: vec![],
            teams: vec![fixtures::team(); 7],
            cars: (0..8)
                .map(|k| Car { name: format!("c{k}"), control: 3, power: 3, handling: 3, durability: 3 })
                .collect(),
        }
    }

    #[test]
    fn the_field_is_deterministic_and_rises_with_difficulty() {
        let r = roster();
        let (a, b) = (field(&cars(), 12, 0.6, 42, &r), field(&cars(), 12, 0.6, 42, &r));
        assert_eq!(a, b);
        assert_ne!(field(&cars(), 12, 0.6, 42, &r).0, field(&cars(), 12, 0.6, 43, &r).0);
        let mean = |difficulty: f32| {
            let (e, _) = field(&cars(), 200, difficulty, 7, &r);
            e.iter().map(|e| e.skill).sum::<f32>() / e.len() as f32
        };
        assert!(mean(1.0) > mean(0.5) && mean(0.5) > mean(0.0), "{} {} {}", mean(0.0), mean(0.5), mean(1.0));
        let (e, c) = field(&cars(), 12, 0.6, 7, &r);
        for (e, c) in e.iter().zip(&c) {
            assert!((0.91..=1.0).contains(&e.skill) && (0.55..=1.0).contains(&e.consistency));
            assert!((1..=5).contains(&c.driver.driver.technique) && c.driver.driver.condition == 9);
            assert!(c.team < 7 && c.car.car.name.starts_with('c'));
        }
        // Two drivers to a team.
        assert_eq!((c[0].team, c[1].team, c[2].team), (0, 0, 1));
    }

    #[test]
    fn stage_times_follow_the_track_and_the_seed() {
        let (m, t) = (fixtures::master(), fixtures::track());
        let (_, c) = field(&cars(), 4, 1.0, 9, &roster());
        let run = |seed| {
            let mut c = c.clone();
            let mut rng = CrtRand::srand(seed);
            (0..4)
                .map(|i| {
                    let mut log = RunLog::default();
                    drive_stage(&m, &t, 0, i as i32, &mut c[i], &fixtures::team(), &mut rng, &mut log)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(1), run(1));
        assert_ne!(run(1), run(2));
        let par: f32 = t.sections.iter().map(|s| s.par).sum();
        for seed in 0..50 {
            for r in run(seed) {
                if let AiResult::Time(time) = r {
                    assert!(time > par - 5.0, "{time} vs par {par}");
                }
            }
        }
    }

    #[test]
    fn a_new_rally_repairs_the_cars() {
        let (_, mut c) = field(&cars(), 2, 0.5, 3, &roster());
        c[0].car.parts[2] = 4;
        c[0].false_starts = 2;
        rally_start(&mut c);
        assert_eq!((c[0].car.parts, c[0].false_starts), ([0; 8], 0));
    }
}
