//! What the player picks for an event, and the presets that fill it.

pub use rbr_formats::rsf::Surface;
use rbr_gameplay::crt::CrtRand;

/// How long the event is: `rallies` rallies of `stages` stages each, every rally over `legs` legs. One rally is an
/// event of its own; more make a championship with points after each. A leg is a day: an overnight service ends it,
/// and the next leg starts the next day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Length {
    pub rallies: u8,
    pub legs: u8,
    pub stages: u8,
}

/// Which installed stages the itinerary is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagePool {
    Retail,
    Community,
    All,
}

/// The surfaces the itinerary may use; a rally keeps to one surface, a season mixes them by its rallies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surfaces {
    Only(Surface),
    Mixed,
}

/// The time of day of the stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daylight {
    Day,
    /// Mostly day, the first day's last stages at dusk or night.
    Realistic,
    Night,
}

/// The weather of the stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weather {
    Dry,
    /// Drawn per leg from the stage's own weathers, so a forecast and tyre choice matter.
    Changeable,
    Wet,
}

/// The car damage rules (the game's `CARDAM`: off, reduced, realistic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Damage {
    Off,
    Reduced,
    Realistic,
}

/// The driver's skill level (the game's `SSKILL`): what the call for help costs and whether a stage can restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    Rookie,
    Professional,
    Champion,
}

/// A stage the itinerary can use: the launcher's catalog fills these from the installed stages.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRef {
    /// The launcher's key for launching it; opaque here.
    pub key: String,
    pub name: String,
    /// The location the stage shares with others (an RSF stage's country, a retail map's rally): a rally keeps to one
    /// where the pool allows.
    pub family: String,
    pub surface: Surface,
    /// The stage is a community one (RSF or BTB), for [`StagePool`].
    pub community: bool,
    /// The driveline's length (m).
    pub length: f32,
}

/// An event's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct EventConfig {
    /// The car class the field drives (the launcher catalog's car group, e.g. `Rally2`).
    pub class: String,
    /// The player's car (the catalog car's key).
    pub car: String,
    /// The team whose service crew works on the player's car (the roster's team index; see [`crate::team`]).
    pub team: u8,
    pub length: Length,
    pub pool: StagePool,
    pub surfaces: Surfaces,
    pub daylight: Daylight,
    pub weather: Weather,
    pub damage: Damage,
    pub skill: Skill,
    /// The field's pace, 0 (gentle) to 1 (the reference pace is the winner's).
    pub difficulty: f32,
    /// The difficulty follows the player's results so the fight stays close.
    pub adaptive: bool,
    /// The Super Rally rule: a retired crew restarts at the next leg.
    pub restart: bool,
    /// The power-stage rule: the rally's last stage pays 5-4-3-2-1 to its five fastest classified finishers.
    pub power_stage: bool,
    /// The AI entrants besides the player.
    pub field_size: u8,
    /// Draws the itinerary, the weather and the field: the same seed and stages build the same event.
    pub seed: u32,
}

/// The presets the event builder starts from, each after a real format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// A one-day gravel rally of six stages, as most national rallies run.
    QuickRally,
    /// A WRC-style round: three legs of four stages, the power stage last.
    Weekend,
    /// Four one-day rallies of six stages on mixed surfaces, as a national championship.
    Championship,
    /// Six two-leg rallies of six stages on mixed surfaces with power stages, as a WRC season shortened.
    Season,
    /// A random event from the seed.
    Random,
}

impl Preset {
    pub const ALL: [Preset; 5] =
        [Preset::QuickRally, Preset::Weekend, Preset::Championship, Preset::Season, Preset::Random];

    /// The preset's settings for `class` and `car`.
    pub fn config(self, class: &str, car: &str, seed: u32) -> EventConfig {
        let base = EventConfig {
            class: class.to_owned(),
            car: car.to_owned(),
            team: 0,
            length: Length { rallies: 1, legs: 1, stages: 6 },
            pool: StagePool::All,
            surfaces: Surfaces::Only(Surface::Gravel),
            daylight: Daylight::Day,
            weather: Weather::Dry,
            damage: Damage::Realistic,
            skill: Skill::Professional,
            difficulty: 0.6,
            adaptive: false,
            restart: true,
            power_stage: false,
            field_size: 12,
            seed,
        };
        match self {
            Preset::QuickRally => base,
            // The first day ends at dusk; the weather changes by the leg.
            Preset::Weekend => EventConfig {
                length: Length { rallies: 1, legs: 3, stages: 12 },
                surfaces: Surfaces::Mixed,
                daylight: Daylight::Realistic,
                weather: Weather::Changeable,
                power_stage: true,
                ..base
            },
            Preset::Championship => EventConfig {
                length: Length { rallies: 4, legs: 1, stages: 6 },
                surfaces: Surfaces::Mixed,
                weather: Weather::Changeable,
                ..base
            },
            Preset::Season => EventConfig {
                length: Length { rallies: 6, legs: 2, stages: 6 },
                surfaces: Surfaces::Mixed,
                weather: Weather::Changeable,
                power_stage: true,
                ..base
            },
            // A seeded mix of everything else (length, pool, surfaces, daylight, weather).
            Preset::Random => {
                let mut rng = CrtRand::srand(seed);
                let draw = |rng: &mut CrtRand, n: usize| (rng.draw() as usize) % n;
                let rallies = match draw(&mut rng, 3) {
                    0 | 1 => 1,
                    _ => 3 + draw(&mut rng, 4) as u8,
                };
                let legs = 1 + draw(&mut rng, 3) as u8;
                let stages = 4 + draw(&mut rng, 5) as u8 + 2 * (legs - 1);
                let length = Length { rallies, legs, stages };
                let pool = match draw(&mut rng, 3) {
                    0 => StagePool::Retail,
                    1 => StagePool::Community,
                    _ => StagePool::All,
                };
                let surfaces = match draw(&mut rng, 4) {
                    0 => Surfaces::Only(Surface::Tarmac),
                    1 => Surfaces::Only(Surface::Gravel),
                    2 => Surfaces::Only(Surface::Snow),
                    _ => Surfaces::Mixed,
                };
                let daylight = match draw(&mut rng, 3) {
                    0 => Daylight::Day,
                    1 => Daylight::Realistic,
                    _ => Daylight::Night,
                };
                let weather = match draw(&mut rng, 3) {
                    0 => Weather::Dry,
                    1 => Weather::Changeable,
                    _ => Weather::Wet,
                };
                // A WRC-style rally over several legs pays the power stage, a one-day rally does not.
                let power_stage = legs > 1;
                EventConfig { length, pool, surfaces, daylight, weather, power_stage, ..base }
            }
        }
    }
}
