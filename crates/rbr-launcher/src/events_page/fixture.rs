//! A stand-in for `rbr_season` for the screenshots and the tests (not the events' rules): presets with fixed
//! settings, an itinerary drawn in order from the pool, a field whose times are the reference over its skill, and the
//! same turn order as the model's: the welcome, the stages, a service where the itinerary has one, the rally's end,
//! the next rally or the event's end.

use rbr_season::config::{
    Damage, Daylight, EventConfig, Length, Preset, StagePool, StageRef, Surface, Surfaces, Weather,
};
use rbr_season::event::{Conditions, Event, ItineraryStage, PlayerResult, Rally, Standing, Turn};
use rbr_season::field::{AiResult, Entrant};

use super::model::{Model, Play, Reference};

pub const DOUBLE: Model = Model { config, build, load };

/// The double's save: only the turn, over the fixture event ([`fixture_event`]).
const SAVED: &str = "double";

const DRIVERS: [&str; 14] = [
    "Aino Lehtonen",
    "Marco Bellini",
    "Sophie Durand",
    "Tom Harding",
    "Kenji Arai",
    "Lena Vogel",
    "Pablo Ortiz",
    "Ingrid Solberg",
    "Owen Price",
    "Nadia Kowalska",
    "Rhys Morgan",
    "Elif Kaya",
    "Jonas Berg",
    "Clara Moreau",
];
const RALLIES: [&str; 6] =
    ["Forest Rally", "Rally of the Lakes", "Mountain Rally", "Coastal Rally", "Rally of the North", "Desert Rally"];

fn config(preset: Preset, class: &str, car: &str, seed: u32) -> EventConfig {
    let base = EventConfig {
        class: class.to_owned(),
        car: car.to_owned(),
        length: Length::Rally { stages: 6 },
        pool: StagePool::All,
        surfaces: Surfaces::Only(Surface::Gravel),
        daylight: Daylight::Day,
        weather: Weather::Changeable,
        damage: Damage::Realistic,
        difficulty: 0.5,
        adaptive: true,
        field_size: 12,
        seed,
    };
    match preset {
        Preset::QuickRally => base,
        Preset::Weekend => {
            EventConfig { length: Length::Weekend { stages: 10 }, daylight: Daylight::Realistic, ..base }
        }
        Preset::Season => {
            EventConfig { length: Length::Season { rallies: 6, stages: 6 }, surfaces: Surfaces::Mixed, ..base }
        }
        Preset::Random => EventConfig { length: Length::Weekend { stages: 4 + seed as u8 % 7 }, ..base },
    }
}

/// The rallies and stages per rally of `length`.
fn shape(length: Length) -> (usize, usize) {
    match length {
        Length::Rally { stages } | Length::Weekend { stages } => (1, stages as usize),
        Length::Season { rallies, stages } => (rallies as usize, stages as usize),
    }
}

fn build(config: EventConfig, stages: &[StageRef], cars: &[String], _: Reference) -> Result<Box<dyn Play>, String> {
    let pool: Vec<&StageRef> = stages
        .iter()
        .filter(|s| match config.pool {
            StagePool::Retail => !s.community,
            StagePool::Community => s.community,
            StagePool::All => true,
        })
        .filter(|s| match config.surfaces {
            Surfaces::Only(surface) => s.surface == surface,
            Surfaces::Mixed => true,
        })
        .collect();
    if pool.is_empty() {
        return Err("no installed stage fits the stage pool and surface".into());
    }
    let (rallies, count) = shape(config.length);
    let mut drawn = pool.iter().cycle();
    let rallies = (0..rallies)
        .map(|r| {
            let stages = (0..count)
                .map(|k| {
                    let night = config.daylight == Daylight::Night
                        || (config.daylight == Daylight::Realistic && k + 2 >= count);
                    let conditions = match (config.weather, k % 3) {
                        (Weather::Dry, _) | (Weather::Changeable, 0) => Conditions::Dry,
                        (Weather::Wet, _) | (Weather::Changeable, 1) => Conditions::Wet,
                        _ => Conditions::Damp,
                    };
                    let service = matches!(k, 1 | 3) && k + 1 < count;
                    ItineraryStage {
                        stage: (*drawn.next().unwrap()).clone(),
                        clock: if night { 20.5 + k as f32 * 0.4 } else { 8.5 + k as f32 * 1.25 },
                        conditions,
                        service_after: service.then_some(if k == 3 { 45 } else { 20 }),
                    }
                })
                .collect();
            Rally { name: RALLIES[r % RALLIES.len()].to_owned(), stages }
        })
        .collect::<Vec<_>>();
    let field = (0..config.field_size as usize)
        .map(|i| Entrant {
            name: DRIVERS[i % DRIVERS.len()].to_owned(),
            car: cars.get(i % cars.len().max(1)).cloned().unwrap_or_default(),
            skill: 1.0 - i as f32 * 0.012,
            consistency: 0.9,
        })
        .collect();
    let results = rallies.iter().map(|_| Vec::new()).collect();
    Ok(Box::new(Double(Event { config, rallies, field, results, turn: Turn::Welcome { rally: 0 } })))
}

fn load(text: &str) -> Result<Box<dyn Play>, String> {
    let turn = text.strip_prefix(SAVED).ok_or("not the double's save")?;
    let mut event = fixture_event();
    if turn.trim() == "stage" {
        event.turn = Turn::Stage { rally: 0, stage: 0 };
    }
    Ok(Box::new(Double(event)))
}

/// The double's event.
pub struct Double(pub Event);

impl Play for Double {
    fn event(&self) -> &Event {
        &self.0
    }

    fn stage_done(&mut self, result: PlayerResult, reference: f32) {
        let Turn::Stage { rally, stage } = self.0.turn else { return };
        let ai = (self.0.field.iter().enumerate())
            .map(|(i, e)| {
                // Every seventh entrant retires on the third stage and stays out; the rest spread a little by stage.
                if (i + 1) % 7 == 0 && stage >= 2 {
                    AiResult::Retired
                } else {
                    AiResult::Time(reference / e.skill * (1.0 + ((i * 7 + stage * 3) % 11) as f32 * 0.002))
                }
            })
            .collect();
        let results = &mut self.0.results[rally];
        results.truncate(stage);
        results.push((Some(result), ai));
        let stages = &self.0.rallies[rally].stages;
        self.0.turn = match stages[stage].service_after {
            _ if result.retired || stage + 1 == stages.len() => Turn::RallyOver { rally },
            Some(minutes) => Turn::Service { rally, stage, minutes },
            None => Turn::Stage { rally, stage: stage + 1 },
        };
    }

    fn turn_done(&mut self) {
        self.0.turn = match self.0.turn {
            Turn::Welcome { rally } => Turn::Stage { rally, stage: 0 },
            Turn::Service { rally, stage, .. } => Turn::Stage { rally, stage: stage + 1 },
            Turn::RallyOver { rally } if rally + 1 < self.0.rallies.len() => Turn::Welcome { rally: rally + 1 },
            Turn::RallyOver { .. } | Turn::EventOver => Turn::EventOver,
            Turn::Stage { .. } => return,
        };
    }

    fn rally_standings(&self, rally: usize) -> Vec<Standing> {
        let results = &self.0.results[rally];
        let player = results.iter().try_fold(0.0, |t, (p, _)| match p {
            Some(p) if !p.retired => Some(t + p.time + p.penalties),
            _ => None,
        });
        let mut lines: Vec<Standing> = vec![Standing { entrant: None, total: player, points: 0 }];
        for i in 0..self.0.field.len() {
            let total = results.iter().try_fold(0.0, |t, (_, ai)| match ai.get(i) {
                Some(AiResult::Time(s)) => Some(t + s),
                _ => None,
            });
            lines.push(Standing { entrant: Some(i), total, points: 0 });
        }
        lines.sort_by(|a, b| a.total.unwrap_or(f32::MAX).total_cmp(&b.total.unwrap_or(f32::MAX)));
        let over = results.len() == self.0.rallies[rally].stages.len()
            || results.iter().any(|(p, _)| p.is_none_or(|p| p.retired));
        if over {
            for (line, points) in lines.iter_mut().filter(|l| l.total.is_some()).zip(POINTS) {
                line.points = points;
            }
        }
        lines
    }

    fn points(&self) -> Vec<Standing> {
        let mut lines: Vec<Standing> =
            (0..=self.0.field.len()).map(|i| Standing { entrant: i.checked_sub(1), total: None, points: 0 }).collect();
        for rally in 0..self.0.rallies.len() {
            for line in self.rally_standings(rally) {
                lines[line.entrant.map_or(0, |e| e + 1)].points += line.points;
            }
        }
        lines.sort_by_key(|l| std::cmp::Reverse(l.points));
        lines
    }

    fn save(&self) -> String {
        SAVED.to_owned()
    }
}

const POINTS: [u32; 10] = [25, 18, 15, 12, 10, 8, 6, 4, 2, 1];

/// The stages the screenshots draw an event on.
pub fn stages() -> Vec<StageRef> {
    [
        ("Harwood Forest", 9_870.0, false),
        ("Mustaselka", 7_120.0, false),
        ("Falstone", 12_420.0, false),
        ("Kaihuavaara", 6_840.0, false),
        ("Sipirkakim", 10_090.0, false),
        ("Ouninpohja", 14_310.0, true),
        ("Chirdonhead", 8_350.0, false),
        ("Pirka Menoko", 11_750.0, false),
    ]
    .iter()
    .enumerate()
    .map(|(k, &(name, length, community))| StageRef {
        key: (20 + k).to_string(),
        name: name.to_owned(),
        surface: Surface::Gravel,
        community,
        length,
    })
    .collect()
}

/// A built event on [`stages`] (a season of two rallies of six stages) at its welcome.
pub fn fixture_event() -> Event {
    let config =
        EventConfig { length: Length::Season { rallies: 2, stages: 6 }, ..config(Preset::Season, "Rally2", "", 7) };
    let config = EventConfig { surfaces: Surfaces::Only(Surface::Gravel), ..config };
    let cars = ["Skoda Fabia Rally2".to_owned(), "Citroen C3 Rally2".to_owned()];
    let built = build(config, &stages(), &cars, &|s, _| s.length / 22.0).unwrap();
    built.event().clone()
}
