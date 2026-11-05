//! What the player picks for an event, and the presets that fill it.

pub use rbr_formats::rsf::Surface;
use rbr_gameplay::crt::CrtRand;

/// How long the event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    /// One rally of `stages` stages, a service halfway.
    Rally { stages: u8 },
    /// One rally over two legs with an overnight service between them, `stages` stages in all.
    Weekend { stages: u8 },
    /// `rallies` rallies of `stages` stages each, points after each rally.
    Season { rallies: u8, stages: u8 },
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
    /// Mostly day, the rally's last stages at dusk or night.
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
    Amateur,
    Professional,
}

/// A stage the itinerary can use: the launcher's catalog fills these from the installed stages.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRef {
    /// The launcher's key for launching it (a retail `track-NN_X`, an RSF stage folder, a BTB stage).
    pub key: String,
    pub name: String,
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
    /// The AI entrants besides the player.
    pub field_size: u8,
    /// Draws the itinerary, the weather and the field: the same seed and stages build the same event.
    pub seed: u32,
}

/// The presets the event builder starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// One gravel rally of six stages.
    QuickRally,
    /// A two-leg rally of ten stages, the second leg at night.
    Weekend,
    /// Six rallies of six stages on mixed surfaces.
    Season,
    /// A random weekend from the seed.
    Random,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::QuickRally, Preset::Weekend, Preset::Season, Preset::Random];

    /// The preset's settings for `class` and `car`.
    pub fn config(self, class: &str, car: &str, seed: u32) -> EventConfig {
        let base = EventConfig {
            class: class.to_owned(),
            car: car.to_owned(),
            length: Length::Rally { stages: 6 },
            pool: StagePool::All,
            surfaces: Surfaces::Only(Surface::Gravel),
            daylight: Daylight::Day,
            weather: Weather::Dry,
            damage: Damage::Realistic,
            skill: Skill::Professional,
            difficulty: 0.6,
            adaptive: false,
            field_size: 12,
            seed,
        };
        match self {
            // One gravel rally of six stages.
            Preset::QuickRally => base,
            // A two-leg rally of ten stages, the second leg at dusk or night, with changeable weather.
            Preset::Weekend => EventConfig {
                length: Length::Weekend { stages: 10 },
                surfaces: Surfaces::Mixed,
                daylight: Daylight::Realistic,
                weather: Weather::Changeable,
                ..base
            },
            // Six rallies of six stages on mixed surfaces.
            Preset::Season => EventConfig {
                length: Length::Season { rallies: 6, stages: 6 },
                surfaces: Surfaces::Mixed,
                weather: Weather::Changeable,
                ..base
            },
            // A seeded mix of everything else (length, pool, surfaces, daylight, weather).
            Preset::Random => {
                let mut rng = CrtRand::srand(seed);
                let draw = |rng: &mut CrtRand, n: usize| (rng.draw() as usize) % n;
                let length = match draw(&mut rng, 3) {
                    0 => Length::Rally { stages: 4 + draw(&mut rng, 5) as u8 },
                    1 => Length::Weekend { stages: 8 + draw(&mut rng, 5) as u8 },
                    _ => Length::Season { rallies: 3 + draw(&mut rng, 4) as u8, stages: 4 + draw(&mut rng, 5) as u8 },
                };
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
                EventConfig { length, pool, surfaces, daylight, weather, ..base }
            }
        }
    }
}
