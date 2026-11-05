//! A built event (ours): the itinerary drawn from the stage pool, the field, the results so far, the standings and
//! points, the player's next turn, and the save in the player's folder.

use crate::config::{Daylight, EventConfig, Length, StagePool, StageRef, Surfaces, Weather};
use crate::field::{self, AiResult, Entrant};
use rbr_gameplay::crt::CrtRand;

/// One stage of a rally's itinerary.
#[derive(Debug, Clone, PartialEq)]
pub struct ItineraryStage {
    pub stage: StageRef,
    /// The start clock (hours, 0 to 24): the launcher passes it as the stage's time of day.
    pub clock: f32,
    pub conditions: Conditions,
    /// The service after this stage (minutes), none when the next stage follows directly.
    pub service_after: Option<u32>,
}

/// A stage's road and sky, which the launcher maps to the stage's own weathers (a dry, damp or wet row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conditions {
    Dry,
    Damp,
    Wet,
}

/// One rally of the event.
#[derive(Debug, Clone, PartialEq)]
pub struct Rally {
    pub name: String,
    pub stages: Vec<ItineraryStage>,
}

/// The player's result on a stage, as the stage's run reports it (`--result-out`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerResult {
    pub time: f32,
    pub penalties: f32,
    pub retired: bool,
}

/// What the player does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    /// The rally's start: its itinerary, before its first stage.
    Welcome {
        rally: usize,
    },
    Stage {
        rally: usize,
        stage: usize,
    },
    /// A service after `stage`, `minutes` long.
    Service {
        rally: usize,
        stage: usize,
        minutes: u32,
    },
    /// The rally is over; the next one, if any, follows.
    RallyOver {
        rally: usize,
    },
    EventOver,
}

/// A standings row.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    /// `None` for the player.
    pub entrant: Option<usize>,
    /// The total time (s) with penalties; `None` once retired.
    pub total: Option<f32>,
    pub points: u32,
}

/// The points for the top 10 of a rally, modern 25-18-15-12-10-8-6-4-2-1.
pub const POINTS: [u32; 10] = [25, 18, 15, 12, 10, 8, 6, 4, 2, 1];
/// The mid-rally service (minutes).
const SERVICE_MID: u32 = 20;
/// The overnight service between a weekend's legs (minutes).
const SERVICE_NIGHT: u32 = 45;
/// The adaptive step: the most a stage moves an entrant's skill.
const ADAPT_STEP: f32 = 0.003;

/// An event under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub config: EventConfig,
    pub rallies: Vec<Rally>,
    pub field: Vec<Entrant>,
    /// Per rally, per stage: the player's result and each entrant's.
    pub results: Vec<Vec<(Option<PlayerResult>, Vec<AiResult>)>>,
    pub turn: Turn,
}

impl Conditions {
    fn wetness(self) -> f32 {
        match self {
            Conditions::Dry => 0.0,
            Conditions::Damp => 0.5,
            Conditions::Wet => 1.0,
        }
    }
}

/// The stage key's family: the location the stages share. Retail keys are `track-NN_X` (the map); RSF and BTB keys
/// start with the location's id or folder (`101-...`).
fn family(key: &str) -> &str {
    if let Some(rest) = key.strip_prefix("track-") {
        let end = rest.find('_').map_or(rest.len(), |i| i + "track-".len());
        return &key[..end];
    }
    match key.find('-') {
        Some(i) => &key[..i],
        None => key,
    }
}

/// A rally's name from its stages: the first stage's name without a trailing variant (`II`, `2`, `B`) and `Rally`.
fn rally_name(stages: &[StageRef]) -> String {
    let base = stages.first().map(|s| s.name.as_str()).unwrap_or("Rally");
    let words: Vec<&str> = base.split_whitespace().collect();
    let mut end = words.len();
    if end > 1 {
        let last = words[end - 1];
        let variant = last.len() == 1
            || last.parse::<u32>().is_ok()
            || matches!(last, "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII");
        if variant {
            end -= 1;
        }
    }
    format!("{} Rally", words[..end.max(1)].join(" "))
}

fn pool_stages<'a>(config: &EventConfig, stages: &'a [StageRef]) -> Vec<&'a StageRef> {
    stages
        .iter()
        .filter(|s| match config.pool {
            StagePool::Retail => !s.community,
            StagePool::Community => s.community,
            StagePool::All => true,
        })
        .collect()
}

impl Event {
    /// Builds the event: the itinerary from `stages` (the installed pool) by the config's length, pool, surfaces,
    /// daylight and weather, and the field from `class_cars` (catalog keys of the config's class).
    /// `reference` gives a stage's reference time for the class at a wetness (`pace::reference_time` over its
    /// driveline).
    pub fn build(
        config: EventConfig,
        stages: &[StageRef],
        class_cars: &[String],
        reference: &dyn Fn(&StageRef, f32) -> f32,
    ) -> Result<Event, String> {
        let (rallies, legs): (Vec<u8>, Vec<u8>) = match config.length {
            Length::Rally { stages: n } => {
                if n == 0 {
                    return Err("a rally needs at least one stage".to_owned());
                }
                (vec![n], vec![1])
            }
            Length::Weekend { stages: n } => {
                if n == 0 {
                    return Err("a rally needs at least one stage".to_owned());
                }
                (vec![n], vec![2])
            }
            Length::Season { rallies: r, stages: n } => {
                if r == 0 || n == 0 {
                    return Err("a season needs rallies with stages".to_owned());
                }
                (vec![n; r as usize], vec![1; r as usize])
            }
        };
        let pool = pool_stages(&config, stages);
        if pool.is_empty() {
            return Err("no stages in the pool".to_owned());
        }
        let mut rng = CrtRand::srand(config.seed);
        // Mixed surfaces draw one surface per rally from the pool's own.
        let mut surfaces: Vec<crate::config::Surface> = pool.iter().map(|s| s.surface).collect();
        surfaces.sort_by_key(|s| *s as u8);
        surfaces.dedup_by_key(|s| *s as u8);
        let mut rally_surface = Vec::new();
        for _ in &rallies {
            match config.surfaces {
                Surfaces::Only(s) => {
                    if !surfaces.contains(&s) {
                        return Err(format!("no {:?} stages in the pool", s.name()));
                    }
                    rally_surface.push(s);
                }
                Surfaces::Mixed => rally_surface.push(surfaces[(rng.draw() as usize) % surfaces.len()]),
            }
        }
        let mut built = Vec::new();
        for (rally, (&count, &surface)) in rallies.iter().zip(rally_surface.iter()).enumerate() {
            let mut candidates: Vec<&StageRef> = pool.iter().filter(|s| s.surface == surface).copied().collect();
            if candidates.is_empty() {
                return Err(format!("no {:?} stages in the pool", surface.name()));
            }
            // Prefer one stage family (the same location): the fullest, ties drawn by the seed.
            let mut families: Vec<(&str, usize)> = Vec::new();
            for s in &candidates {
                let f = family(&s.key);
                match families.iter_mut().find(|(k, _)| *k == f) {
                    Some(slot) => slot.1 += 1,
                    None => families.push((f, 1)),
                }
            }
            let top = families.iter().map(|(_, n)| *n).max().unwrap_or(0);
            let fullest: Vec<&str> = families.into_iter().filter(|(_, n)| *n == top).map(|(k, _)| k).collect();
            let home = fullest[(rng.draw() as usize) % fullest.len()];
            let refill: Vec<StageRef> = candidates.iter().map(|s| (*s).clone()).collect();
            let (mut first, mut rest): (Vec<&StageRef>, Vec<&StageRef>) =
                candidates.drain(..).partition(|s| family(&s.key) == home);
            // Shuffle each group with the seed, then take without repeating unless the pool is too small.
            for group in [&mut first, &mut rest] {
                for i in (1..group.len()).rev() {
                    let j = (rng.draw() as usize) % (i + 1);
                    group.swap(i, j);
                }
            }
            let mut picked: Vec<StageRef> = Vec::new();
            while picked.len() < count as usize {
                match first.pop().or_else(|| rest.pop()) {
                    Some(s) => picked.push((*s).clone()),
                    // Too small a pool: run the rally's stages again.
                    None => picked.extend(refill.iter().take(count as usize - picked.len()).cloned()),
                }
            }
            let leg_count = legs[rally.min(legs.len() - 1)];
            built.push(draw_rally(&config, &picked, leg_count, &mut rng, reference));
        }
        let field = field::field(class_cars, config.field_size as usize, config.difficulty, config.seed ^ 0xF1E1D);
        let results = built.iter().map(|r| vec![(None, Vec::new()); r.stages.len()]).collect();
        Ok(Event { config, rallies: built, field, results, turn: Turn::Welcome { rally: 0 } })
    }

    /// The rally and stage the results are being driven on, if the turn is a stage.
    fn at(&self) -> Option<(usize, usize)> {
        match self.turn {
            Turn::Stage { rally, stage } => Some((rally, stage)),
            _ => None,
        }
    }

    /// The player's stage result: the field drives the same stage, and the turn moves on.
    pub fn stage_done(&mut self, result: PlayerResult, reference: f32) {
        let Some((rally, stage)) = self.at() else { return };
        let wet = self.rallies[rally].stages[stage].conditions.wetness();
        let mut rng = CrtRand::srand(
            self.config
                .seed
                .wrapping_add((rally as u32 + 1).wrapping_mul(0x9E37_79B1))
                .wrapping_add((stage as u32 + 1).wrapping_mul(0x85EB_CA6B)),
        );
        let ai = self.field.iter().map(|e| field::drive_stage(e, reference, wet, &mut rng)).collect();
        self.results[rally][stage] = (Some(result), ai);
        if self.config.adaptive {
            self.adapt(rally);
        }
        self.turn = next_turn(&self.rallies, rally, stage);
    }

    /// Nudges the field toward the player finishing around 2nd to 4th, by at most a small step a stage.
    fn adapt(&mut self, rally: usize) {
        let place = self.rally_standings(rally).iter().position(|row| row.entrant.is_none());
        let Some(place) = place else { return };
        let delta = if place < 1 {
            ADAPT_STEP
        } else if place > 3 {
            -ADAPT_STEP
        } else {
            0.0
        };
        if delta != 0.0 {
            for e in &mut self.field {
                e.skill = (e.skill + delta).clamp(0.85, 1.03);
            }
        }
    }

    /// The service (or welcome, or rally over) is done: the turn moves on.
    pub fn turn_done(&mut self) {
        match self.turn.clone() {
            Turn::Welcome { rally } => {
                if self.rallies.get(rally).is_some_and(|r| !r.stages.is_empty()) {
                    self.turn = Turn::Stage { rally, stage: 0 };
                }
            }
            Turn::Service { rally, stage, .. } => {
                let len = self.rallies.get(rally).map_or(0, |r| r.stages.len());
                self.turn =
                    if stage + 1 < len { Turn::Stage { rally, stage: stage + 1 } } else { Turn::RallyOver { rally } };
            }
            Turn::RallyOver { rally } => {
                self.turn =
                    if rally + 1 < self.rallies.len() { Turn::Welcome { rally: rally + 1 } } else { Turn::EventOver };
            }
            Turn::Stage { .. } | Turn::EventOver => {}
        }
    }

    /// Rally `rally`'s standings after the stages run so far, best first.
    pub fn rally_standings(&self, rally: usize) -> Vec<Standing> {
        let Some(run) = self.results.get(rally).map(|r| run_stages(r)) else { return Vec::new() };
        let mut rows = Vec::new();
        rows.push(Standing { entrant: None, total: self.player_total(rally, run), points: 0 });
        for (i, _) in self.field.iter().enumerate() {
            rows.push(Standing { entrant: Some(i), total: self.entrant_total(rally, i, run), points: 0 });
        }
        rows.sort_by(|a, b| match (a.total, b.total) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        for (i, row) in rows.iter_mut().enumerate() {
            row.points = POINTS.get(i).copied().unwrap_or(0);
        }
        rows
    }

    fn player_total(&self, rally: usize, run: usize) -> Option<f32> {
        let mut total = 0.0;
        for (result, _) in self.results[rally].iter().take(run) {
            match result {
                Some(p) if !p.retired => total += p.time + p.penalties,
                _ => return None,
            }
        }
        Some(total)
    }

    fn entrant_total(&self, rally: usize, entrant: usize, run: usize) -> Option<f32> {
        let mut total = 0.0;
        for (_, ai) in self.results[rally].iter().take(run) {
            match ai.get(entrant) {
                Some(AiResult::Time(t)) => total += t,
                _ => return None,
            }
        }
        Some(total)
    }

    /// The event's points standings (a season's; a single rally's are its rally standings), best first.
    pub fn points(&self) -> Vec<Standing> {
        // Points come from the finished rallies only; the totals add the finished ones for the tie-break.
        let mut won = vec![0u32; self.field.len() + 1];
        let mut totals = vec![Some(0.0f32); self.field.len() + 1];
        for rally in 0..self.rallies.len() {
            if self.results[rally].iter().all(|(p, ai)| p.is_some() && !ai.is_empty()) {
                for row in self.rally_standings(rally) {
                    let at = row.entrant.map_or(self.field.len(), |i| i);
                    won[at] += row.points;
                    totals[at] = match (totals[at], row.total) {
                        (Some(a), Some(b)) => Some(a + b),
                        _ => None,
                    };
                }
            }
        }
        let mut rows: Vec<Standing> = (0..=self.field.len())
            .map(|at| Standing {
                entrant: if at == self.field.len() { None } else { Some(at) },
                total: totals[at],
                points: won[at],
            })
            .collect();
        rows.sort_by(|a, b| {
            b.points.cmp(&a.points).then_with(|| match (a.total, b.total) {
                (Some(x), Some(y)) => x.total_cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
        });
        rows
    }

    /// The event as text for the player's folder.
    pub fn save(&self) -> String {
        let mut out = String::from("rbr-season 1\n");
        let c = &self.config;
        out.push_str(&format!(
            "C|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            esc(&c.class),
            esc(&c.car),
            length_save(&c.length),
            pool_save(c.pool),
            surfaces_save(&c.surfaces),
            daylight_save(c.daylight),
            weather_save(c.weather),
            damage_save(c.damage),
            c.difficulty,
            if c.adaptive { 1 } else { 0 },
            c.field_size,
            c.seed,
            skill_save(c.skill)
        ));
        for rally in &self.rallies {
            out.push_str(&format!("R|{}\n", esc(&rally.name)));
            for s in &rally.stages {
                out.push_str(&format!(
                    "S|{}|{}|{}|{}|{}|{}|{}|{}\n",
                    esc(&s.stage.key),
                    esc(&s.stage.name),
                    s.stage.surface.name(),
                    if s.stage.community { 1 } else { 0 },
                    s.stage.length,
                    s.clock,
                    conditions_save(s.conditions),
                    s.service_after.map_or(String::new(), |m| m.to_string())
                ));
            }
        }
        for e in &self.field {
            out.push_str(&format!("F|{}|{}|{}|{}\n", esc(&e.name), esc(&e.car), e.skill, e.consistency));
        }
        for (rally, stages) in self.results.iter().enumerate() {
            for (stage, (player, ai)) in stages.iter().enumerate() {
                let p = match player {
                    None => "-".to_owned(),
                    Some(r) if r.retired => "R".to_owned(),
                    Some(r) => format!("{}+{}", r.time, r.penalties),
                };
                let a: Vec<String> = ai
                    .iter()
                    .map(|r| match r {
                        AiResult::Time(t) => t.to_string(),
                        AiResult::Retired => "R".to_owned(),
                    })
                    .collect();
                out.push_str(&format!("T|{rally}|{stage}|{p}|{}\n", a.join(",")));
            }
        }
        out.push_str(&format!("U|{}\n", turn_save(&self.turn)));
        out
    }

    /// An event read back from [`Event::save`]'s text.
    pub fn load(text: &str) -> Result<Event, String> {
        let mut lines = text.lines();
        match lines.next() {
            Some("rbr-season 1") => {}
            Some(head) => return Err(format!("not an rbr-season save: {head}")),
            None => return Err("empty save".to_owned()),
        }
        let mut config: Option<EventConfig> = None;
        let mut rallies: Vec<Rally> = Vec::new();
        let mut field: Vec<Entrant> = Vec::new();
        let mut results: Vec<Vec<(Option<PlayerResult>, Vec<AiResult>)>> = Vec::new();
        let mut turn: Option<Turn> = None;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<String> = split(line);
            match parts.first().map(String::as_str) {
                Some("C") => {
                    if parts.len() != 14 {
                        return Err("bad config line".to_owned());
                    }
                    config = Some(EventConfig {
                        class: parts[1].clone(),
                        car: parts[2].clone(),
                        length: length_load(&parts[3])?,
                        pool: pool_load(&parts[4])?,
                        surfaces: surfaces_load(&parts[5])?,
                        daylight: daylight_load(&parts[6])?,
                        weather: weather_load(&parts[7])?,
                        damage: damage_load(&parts[8])?,
                        difficulty: parts[9].parse().map_err(|_| "bad difficulty".to_owned())?,
                        adaptive: match parts[10].as_str() {
                            "1" => true,
                            "0" => false,
                            _ => return Err("bad adaptive".to_owned()),
                        },
                        field_size: parts[11].parse().map_err(|_| "bad field size".to_owned())?,
                        seed: parts[12].parse().map_err(|_| "bad seed".to_owned())?,
                        skill: skill_load(&parts[13])?,
                    });
                }
                Some("R") => {
                    if parts.len() != 2 {
                        return Err("bad rally line".to_owned());
                    }
                    rallies.push(Rally { name: parts[1].clone(), stages: Vec::new() });
                    results.push(Vec::new());
                }
                Some("S") => {
                    if parts.len() != 9 {
                        return Err("bad stage line".to_owned());
                    }
                    let rally = rallies.last_mut().ok_or("stage outside a rally".to_owned())?;
                    rally.stages.push(ItineraryStage {
                        stage: StageRef {
                            key: parts[1].clone(),
                            name: parts[2].clone(),
                            surface: surface_load(&parts[3])?,
                            community: match parts[4].as_str() {
                                "1" => true,
                                "0" => false,
                                _ => return Err("bad community".to_owned()),
                            },
                            length: parts[5].parse().map_err(|_| "bad length".to_owned())?,
                        },
                        clock: parts[6].parse().map_err(|_| "bad clock".to_owned())?,
                        conditions: conditions_load(&parts[7])?,
                        service_after: if parts[8].is_empty() {
                            None
                        } else {
                            Some(parts[8].parse().map_err(|_| "bad service".to_owned())?)
                        },
                    });
                    results.last_mut().ok_or("stage outside a rally".to_owned())?.push((None, Vec::new()));
                }
                Some("F") => {
                    if parts.len() != 5 {
                        return Err("bad entrant line".to_owned());
                    }
                    field.push(Entrant {
                        name: parts[1].clone(),
                        car: parts[2].clone(),
                        skill: parts[3].parse().map_err(|_| "bad skill".to_owned())?,
                        consistency: parts[4].parse().map_err(|_| "bad consistency".to_owned())?,
                    });
                }
                Some("T") => {
                    if parts.len() != 5 {
                        return Err("bad result line".to_owned());
                    }
                    let (rally, stage) = (
                        parts[1].parse::<usize>().map_err(|_| "bad result rally".to_owned())?,
                        parts[2].parse::<usize>().map_err(|_| "bad result stage".to_owned())?,
                    );
                    let player = match parts[3].as_str() {
                        "-" => None,
                        "R" => Some(PlayerResult { time: 0.0, penalties: 0.0, retired: true }),
                        t => {
                            let (time, pen) = t.split_once('+').ok_or("bad player result".to_owned())?;
                            Some(PlayerResult {
                                time: time.parse().map_err(|_| "bad player time".to_owned())?,
                                penalties: pen.parse().map_err(|_| "bad penalties".to_owned())?,
                                retired: false,
                            })
                        }
                    };
                    let mut ai = Vec::new();
                    if !parts[4].is_empty() {
                        for cell in parts[4].split(',') {
                            ai.push(match cell {
                                "R" => AiResult::Retired,
                                t => AiResult::Time(t.parse().map_err(|_| "bad AI time".to_owned())?),
                            });
                        }
                    }
                    let row = results
                        .get_mut(rally)
                        .and_then(|r| r.get_mut(stage))
                        .ok_or("result outside the itinerary".to_owned())?;
                    *row = (player, ai);
                }
                Some("U") => {
                    if parts.len() != 2 {
                        return Err("bad turn line".to_owned());
                    }
                    turn = Some(turn_load(&parts[1])?);
                }
                _ => return Err(format!("bad save line: {line}")),
            }
        }
        Ok(Event {
            config: config.ok_or("no config".to_owned())?,
            rallies,
            field,
            results,
            turn: turn.ok_or("no turn".to_owned())?,
        })
    }
}

/// The stages run in a rally's results: every row the player or the field drove.
fn run_stages(results: &[(Option<PlayerResult>, Vec<AiResult>)]) -> usize {
    results.iter().filter(|(p, ai)| p.is_some() || !ai.is_empty()).count()
}

/// The turn after stage `stage` of `rally` ran.
fn next_turn(rallies: &[Rally], rally: usize, stage: usize) -> Turn {
    let len = rallies.get(rally).map_or(0, |r| r.stages.len());
    if stage + 1 < len {
        if let Some(m) = rallies[rally].stages[stage].service_after {
            return Turn::Service { rally, stage, minutes: m };
        }
        return Turn::Stage { rally, stage: stage + 1 };
    }
    Turn::RallyOver { rally }
}

/// Draws one rally's itinerary: start clocks from 08:00 (21:00 at night) with service gaps, conditions by the
/// weather (changeable per leg), and the last leg at dusk or night on realistic daylight.
fn draw_rally(
    config: &EventConfig,
    picked: &[StageRef],
    legs: u8,
    rng: &mut CrtRand,
    reference: &dyn Fn(&StageRef, f32) -> f32,
) -> Rally {
    let legs = legs.max(1) as usize;
    let per_leg = picked.len().div_ceil(legs);
    // Changeable weather is drawn per leg.
    let mut leg_conditions = Vec::new();
    for _ in 0..legs {
        leg_conditions.push(match config.weather {
            Weather::Dry => Conditions::Dry,
            Weather::Wet => Conditions::Wet,
            Weather::Changeable => {
                let r = (rng.draw() as usize) % 10;
                if r < 5 {
                    Conditions::Dry
                } else if r < 8 {
                    Conditions::Damp
                } else {
                    Conditions::Wet
                }
            }
        });
    }
    let mut clock: f32 = match config.daylight {
        Daylight::Night => 21.0,
        _ => 8.0,
    };
    let mut stages = Vec::new();
    let last_start = if legs > 1 { (legs - 1) * per_leg } else { picked.len().saturating_sub(2) };
    for (i, stage) in picked.iter().enumerate() {
        let leg = (i / per_leg.max(1)).min(legs - 1);
        let conditions = leg_conditions[leg];
        let wet = conditions.wetness();
        let reference = reference(stage, wet);
        // Realistic daylight puts the last leg at dusk or night.
        if matches!(config.daylight, Daylight::Realistic) && i >= last_start && picked.len() > 1 {
            clock = clock.max(19.0 + 0.5 * (i - last_start) as f32);
        }
        let at = clock % 24.0;
        // The next stage follows the reference time and a gap, longer after a wet stage.
        clock += reference / 3600.0 + 0.45 + 0.15 * wet;
        // Services: about 20 minutes mid-rally (mid-leg), 45 overnight between a weekend's legs.
        let end_of_leg = (i + 1) % per_leg.max(1) == 0 && i + 1 < picked.len();
        let pos = i - leg * per_leg.max(1);
        let mid_leg = pos + 1 == per_leg.max(1).div_ceil(2);
        let service_after = if end_of_leg && legs > 1 {
            Some(SERVICE_NIGHT)
        } else if end_of_leg || mid_leg {
            Some(SERVICE_MID)
        } else {
            None
        };
        if let Some(m) = service_after {
            clock += m as f32 / 60.0;
        }
        stages.push(ItineraryStage { stage: stage.clone(), clock: at, conditions, service_after });
    }
    Rally { name: rally_name(picked), stages }
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\p"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

fn split(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => cur.push('\\'),
                Some('p') => cur.push('|'),
                Some('n') => cur.push('\n'),
                Some(x) => {
                    cur.push('\\');
                    cur.push(x);
                }
                None => cur.push('\\'),
            }
        } else if c == '|' {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    parts.push(cur);
    parts
}

fn length_save(length: &Length) -> String {
    match length {
        Length::Rally { stages } => format!("rally:{stages}"),
        Length::Weekend { stages } => format!("weekend:{stages}"),
        Length::Season { rallies, stages } => format!("season:{rallies}:{stages}"),
    }
}

fn length_load(text: &str) -> Result<Length, String> {
    let mut it = text.split(':');
    match (it.next(), it.next(), it.next(), it.next()) {
        (Some("rally"), Some(n), None, _) => {
            Ok(Length::Rally { stages: n.parse().map_err(|_| "bad rally length".to_owned())? })
        }
        (Some("weekend"), Some(n), None, _) => {
            Ok(Length::Weekend { stages: n.parse().map_err(|_| "bad weekend length".to_owned())? })
        }
        (Some("season"), Some(r), Some(n), None) => Ok(Length::Season {
            rallies: r.parse().map_err(|_| "bad season rallies".to_owned())?,
            stages: n.parse().map_err(|_| "bad season stages".to_owned())?,
        }),
        _ => Err("bad length".to_owned()),
    }
}

fn pool_save(pool: StagePool) -> &'static str {
    match pool {
        StagePool::Retail => "retail",
        StagePool::Community => "community",
        StagePool::All => "all",
    }
}

fn pool_load(text: &str) -> Result<StagePool, String> {
    match text {
        "retail" => Ok(StagePool::Retail),
        "community" => Ok(StagePool::Community),
        "all" => Ok(StagePool::All),
        _ => Err("bad pool".to_owned()),
    }
}

fn surfaces_save(surfaces: &Surfaces) -> String {
    match surfaces {
        Surfaces::Only(s) => format!("only:{}", s.name()),
        Surfaces::Mixed => "mixed".to_owned(),
    }
}

fn surfaces_load(text: &str) -> Result<Surfaces, String> {
    match text {
        "mixed" => Ok(Surfaces::Mixed),
        only => match only.strip_prefix("only:") {
            Some(name) => Ok(Surfaces::Only(surface_load(name)?)),
            None => Err("bad surfaces".to_owned()),
        },
    }
}

fn surface_load(text: &str) -> Result<crate::config::Surface, String> {
    use crate::config::Surface;
    match text {
        "tarmac" => Ok(Surface::Tarmac),
        "gravel" => Ok(Surface::Gravel),
        "snow" => Ok(Surface::Snow),
        _ => Err("bad surface".to_owned()),
    }
}

fn daylight_save(daylight: Daylight) -> &'static str {
    match daylight {
        Daylight::Day => "day",
        Daylight::Realistic => "realistic",
        Daylight::Night => "night",
    }
}

fn daylight_load(text: &str) -> Result<Daylight, String> {
    match text {
        "day" => Ok(Daylight::Day),
        "realistic" => Ok(Daylight::Realistic),
        "night" => Ok(Daylight::Night),
        _ => Err("bad daylight".to_owned()),
    }
}

fn weather_save(weather: Weather) -> &'static str {
    match weather {
        Weather::Dry => "dry",
        Weather::Changeable => "changeable",
        Weather::Wet => "wet",
    }
}

fn weather_load(text: &str) -> Result<Weather, String> {
    match text {
        "dry" => Ok(Weather::Dry),
        "changeable" => Ok(Weather::Changeable),
        "wet" => Ok(Weather::Wet),
        _ => Err("bad weather".to_owned()),
    }
}

fn damage_save(damage: crate::config::Damage) -> &'static str {
    use crate::config::Damage;
    match damage {
        Damage::Off => "off",
        Damage::Reduced => "reduced",
        Damage::Realistic => "realistic",
    }
}

fn damage_load(text: &str) -> Result<crate::config::Damage, String> {
    use crate::config::Damage;
    match text {
        "off" => Ok(Damage::Off),
        "reduced" => Ok(Damage::Reduced),
        "realistic" => Ok(Damage::Realistic),
        _ => Err("bad damage".to_owned()),
    }
}

fn skill_save(skill: crate::config::Skill) -> &'static str {
    use crate::config::Skill;
    match skill {
        Skill::Rookie => "rookie",
        Skill::Amateur => "amateur",
        Skill::Professional => "professional",
    }
}

fn skill_load(text: &str) -> Result<crate::config::Skill, String> {
    use crate::config::Skill;
    match text {
        "rookie" => Ok(Skill::Rookie),
        "amateur" => Ok(Skill::Amateur),
        "professional" => Ok(Skill::Professional),
        _ => Err("bad skill".to_owned()),
    }
}

fn conditions_save(conditions: Conditions) -> &'static str {
    match conditions {
        Conditions::Dry => "dry",
        Conditions::Damp => "damp",
        Conditions::Wet => "wet",
    }
}

fn conditions_load(text: &str) -> Result<Conditions, String> {
    match text {
        "dry" => Ok(Conditions::Dry),
        "damp" => Ok(Conditions::Damp),
        "wet" => Ok(Conditions::Wet),
        _ => Err("bad conditions".to_owned()),
    }
}

fn turn_save(turn: &Turn) -> String {
    match turn {
        Turn::Welcome { rally } => format!("welcome:{rally}"),
        Turn::Stage { rally, stage } => format!("stage:{rally}:{stage}"),
        Turn::Service { rally, stage, minutes } => format!("service:{rally}:{stage}:{minutes}"),
        Turn::RallyOver { rally } => format!("rallyover:{rally}"),
        Turn::EventOver => "over".to_owned(),
    }
}

fn turn_load(text: &str) -> Result<Turn, String> {
    let mut it = text.split(':');
    match (it.next(), it.next(), it.next(), it.next(), it.next()) {
        (Some("welcome"), Some(r), None, _, _) => {
            Ok(Turn::Welcome { rally: r.parse().map_err(|_| "bad turn".to_owned())? })
        }
        (Some("stage"), Some(r), Some(s), None, _) => Ok(Turn::Stage {
            rally: r.parse().map_err(|_| "bad turn".to_owned())?,
            stage: s.parse().map_err(|_| "bad turn".to_owned())?,
        }),
        (Some("service"), Some(r), Some(s), Some(m), None) => Ok(Turn::Service {
            rally: r.parse().map_err(|_| "bad turn".to_owned())?,
            stage: s.parse().map_err(|_| "bad turn".to_owned())?,
            minutes: m.parse().map_err(|_| "bad turn".to_owned())?,
        }),
        (Some("rallyover"), Some(r), None, _, _) => {
            Ok(Turn::RallyOver { rally: r.parse().map_err(|_| "bad turn".to_owned())? })
        }
        (Some("over"), None, _, _, _) => Ok(Turn::EventOver),
        _ => Err("bad turn".to_owned()),
    }
}

#[cfg(test)]
mod preset_tests {
    //! The presets' shared and differing settings.
    use super::*;
    use crate::config::{Damage, Preset};

    #[test]
    fn the_presets_share_the_field_and_differ_in_shape() {
        for preset in Preset::ALL {
            let c = preset.config("Rally2", "car", 1);
            assert_eq!((c.field_size, c.difficulty, c.adaptive, c.damage), (12, 0.6, false, Damage::Realistic));
        }
        let quick = Preset::QuickRally.config("Rally2", "car", 1);
        assert_eq!(quick.length, Length::Rally { stages: 6 });
        let weekend = Preset::Weekend.config("Rally2", "car", 1);
        assert_eq!(weekend.length, Length::Weekend { stages: 10 });
        assert_eq!(weekend.daylight, Daylight::Realistic);
        let season = Preset::Season.config("Rally2", "car", 1);
        assert_eq!(season.length, Length::Season { rallies: 6, stages: 6 });
        assert_eq!(season.surfaces, Surfaces::Mixed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Damage, Preset};

    fn gravel_pool() -> Vec<StageRef> {
        use crate::config::Surface;
        let mut stages = Vec::new();
        for map in [4u32, 10, 22, 31] {
            for (i, letter) in ['A', 'B', 'C'].iter().enumerate() {
                stages.push(StageRef {
                    key: format!("track-{map}_{letter}"),
                    name: format!("Map {map} stage {}", i + 1),
                    surface: Surface::Gravel,
                    community: false,
                    length: 8000.0,
                });
            }
        }
        stages
    }

    fn mixed_pool() -> Vec<StageRef> {
        use crate::config::Surface;
        vec![
            StageRef {
                key: "track-4_A".into(),
                name: "Himalaya A".into(),
                surface: Surface::Gravel,
                community: false,
                length: 8000.0,
            },
            StageRef {
                key: "track-4_B".into(),
                name: "Himalaya B".into(),
                surface: Surface::Gravel,
                community: false,
                length: 9000.0,
            },
            StageRef {
                key: "track-10_A".into(),
                name: "Alp A".into(),
                surface: Surface::Tarmac,
                community: false,
                length: 7000.0,
            },
            StageRef {
                key: "track-10_B".into(),
                name: "Alp B".into(),
                surface: Surface::Tarmac,
                community: false,
                length: 7500.0,
            },
            StageRef {
                key: "101-a".into(),
                name: "Autiovaara I".into(),
                surface: Surface::Snow,
                community: true,
                length: 10000.0,
            },
            StageRef {
                key: "101-b".into(),
                name: "Autiovaara II".into(),
                surface: Surface::Snow,
                community: true,
                length: 11000.0,
            },
        ]
    }

    fn reference(_stage: &StageRef, wet: f32) -> f32 {
        300.0 * (1.0 + 0.1 * wet)
    }

    #[test]
    fn the_itinerary_respects_pool_surface_and_length() {
        let pool = mixed_pool();
        let cars = vec!["car".to_owned()];
        // Only gravel, retail only: the two Himalaya stages, twice over for a 4-stage rally.
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 4 };
        config.pool = StagePool::Retail;
        let event = Event::build(config, &pool, &cars, &reference).unwrap();
        assert_eq!(event.rallies.len(), 1);
        assert_eq!(event.rallies[0].stages.len(), 4);
        assert!(event.rallies[0].stages.iter().all(|s| !s.stage.community));
        assert!(event.rallies[0].stages.iter().all(|s| s.stage.surface == crate::config::Surface::Gravel));
        // No repeat while the pool suffices.
        let keys: Vec<&str> = event.rallies[0].stages.iter().map(|s| s.stage.key.as_str()).collect();
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 2 };
        config.pool = StagePool::Retail;
        let event = Event::build(config, &pool, &cars, &reference).unwrap();
        assert_eq!(keys.len(), 4);
        let keys: Vec<&str> = event.rallies[0].stages.iter().map(|s| s.stage.key.as_str()).collect();
        assert_eq!(keys.len(), 2);
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 2, "no repeats when the pool suffices: {keys:?}");
    }

    #[test]
    fn a_season_mixes_surfaces_and_a_weekend_has_two_legs() {
        let pool = mixed_pool();
        let cars = vec!["car".to_owned()];
        let mut config = Preset::Season.config("Rally2", "car", 5);
        config.length = Length::Season { rallies: 4, stages: 2 };
        let event = Event::build(config, &pool, &cars, &reference).unwrap();
        assert_eq!(event.rallies.len(), 4);
        // Each rally keeps one surface.
        for rally in &event.rallies {
            assert_eq!(rally.stages.len(), 2);
            assert!(rally.stages.windows(2).all(|w| w[0].stage.surface == w[1].stage.surface));
        }
        let mut config = Preset::Weekend.config("Rally2", "car", 5);
        config.length = Length::Weekend { stages: 4 };
        let event = Event::build(config, &pool, &cars, &reference).unwrap();
        let nights: Vec<_> = event.rallies[0].stages.iter().filter(|s| s.service_after == Some(45)).collect();
        assert_eq!(nights.len(), 1, "one overnight service between the legs");
        assert!(event.rallies[0].name.ends_with("Rally"));
    }

    #[test]
    fn clocks_and_conditions_follow_daylight_and_weather() {
        let pool = gravel_pool();
        let cars = vec!["car".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 3 };
        let event = Event::build(config, &pool, &cars, &reference).unwrap();
        let clocks: Vec<f32> = event.rallies[0].stages.iter().map(|s| s.clock).collect();
        assert!((clocks[0] - 8.0).abs() < 0.01, "{clocks:?}");
        assert!(clocks.windows(2).all(|w| w[1] > w[0]), "{clocks:?}");
        assert!(event.rallies[0].stages.iter().all(|s| s.conditions == Conditions::Dry));
        // Night puts every stage at night.
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 3 };
        config.daylight = Daylight::Night;
        let event = Event::build(config, &pool, &cars, &reference).unwrap();
        assert!(event.rallies[0].stages.iter().all(|s| s.clock >= 21.0 || s.clock < 8.0));
    }

    #[test]
    fn standings_score_the_top_ten_and_retirements() {
        let pool = gravel_pool();
        let cars = vec!["car-a".to_owned(), "car-b".to_owned(), "car-c".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 2 };
        config.field_size = 3;
        let mut event = Event::build(config, &pool, &cars, &reference).unwrap();
        event.turn_done(); // Welcome -> stage 0.
        event.stage_done(PlayerResult { time: 300.0, penalties: 0.0, retired: false }, 300.0);
        // Force known AI times for the standings check.
        event.results[0][0].1 = vec![AiResult::Time(290.0), AiResult::Time(310.0), AiResult::Retired];
        let standings = event.rally_standings(0);
        assert_eq!(standings.len(), 4);
        assert_eq!(standings[0], Standing { entrant: Some(0), total: Some(290.0), points: 25 });
        assert_eq!(standings[1], Standing { entrant: None, total: Some(300.0), points: 18 });
        assert_eq!(standings[2].points, 15);
        assert_eq!(standings[3], Standing { entrant: Some(2), total: None, points: 12 });
        // The retired AI is out of the rally: still last after stage 2.
        event.turn_done();
        event.stage_done(PlayerResult { time: 300.0, penalties: 10.0, retired: false }, 300.0);
        event.results[0][1].1 = vec![AiResult::Time(290.0), AiResult::Time(310.0), AiResult::Time(200.0)];
        let standings = event.rally_standings(0);
        assert!(standings.last().unwrap().entrant == Some(2));
        assert_eq!(standings.first().unwrap().points, 25);
        // Points of a single rally are its rally standings.
        let points = event.points();
        assert_eq!(points, event.rally_standings(0));
    }

    #[test]
    fn adaptive_moves_the_field_toward_second_to_fourth() {
        let pool = gravel_pool();
        let cars = vec!["car".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 4 };
        config.field_size = 4;
        config.adaptive = true;
        config.difficulty = 1.0;
        let mut event = Event::build(config, &pool, &cars, &reference).unwrap();
        let before: Vec<f32> = event.field.iter().map(|e| e.skill).collect();
        event.turn_done();
        // The player wins easily: the field speeds up, by at most the step.
        event.stage_done(PlayerResult { time: 200.0, penalties: 0.0, retired: false }, 300.0);
        for (a, b) in before.iter().zip(event.field.iter()) {
            assert!((b.skill - a).abs() <= ADAPT_STEP + 1e-6, "{} -> {}", a, b.skill);
        }
        assert!(event.field.iter().map(|e| e.skill).sum::<f32>() > before.iter().sum::<f32>());
        // The player crawls: the field slows down.
        let mid: Vec<f32> = event.field.iter().map(|e| e.skill).collect();
        event.turn_done();
        event.stage_done(PlayerResult { time: 900.0, penalties: 0.0, retired: false }, 300.0);
        assert!(event.field.iter().map(|e| e.skill).sum::<f32>() < mid.iter().sum::<f32>());
        // Skills stay in bounds over a full event.
        assert!(event.field.iter().all(|e| e.skill >= 0.85 && e.skill <= 1.03));
        assert_eq!(event.config.damage, Damage::Realistic);
    }

    #[test]
    fn the_save_round_trips() {
        let pool = mixed_pool();
        let cars = vec!["car-a".to_owned(), "car|b\\c".to_owned()];
        let mut config = Preset::Season.config("Rally 2", "car|b\\c", 5);
        config.length = Length::Season { rallies: 2, stages: 3 };
        let mut event = Event::build(config, &pool, &cars, &reference).unwrap();
        event.turn_done();
        event.stage_done(PlayerResult { time: 301.5, penalties: 10.0, retired: false }, 300.0);
        event.turn_done();
        let text = event.save();
        assert!(text.starts_with("rbr-season 1\n"));
        let back = Event::load(&text).unwrap();
        assert_eq!(event, back);
        assert_eq!(Event::load(&back.save()).unwrap(), back);
        assert!(Event::load("bogus").is_err());
        assert!(Event::load("rbr-season 1\nbogus|line").is_err());
    }

    #[test]
    fn a_full_season_runs_with_a_fixed_player_time() {
        let pool = gravel_pool();
        let cars = vec!["car-a".to_owned(), "car-b".to_owned()];
        let mut config = Preset::Season.config("Rally2", "car-a", 99);
        config.length = Length::Season { rallies: 2, stages: 3 };
        config.field_size = 6;
        let mut event = Event::build(config, &pool, &cars, &reference).unwrap();
        let mut stages = 0;
        loop {
            match event.turn.clone() {
                Turn::Welcome { .. } | Turn::Service { .. } | Turn::RallyOver { .. } => event.turn_done(),
                Turn::Stage { .. } => {
                    event.stage_done(PlayerResult { time: 310.0, penalties: 0.0, retired: false }, 300.0);
                    stages += 1;
                }
                Turn::EventOver => break,
            }
            assert!(stages < 100, "the event must end");
        }
        assert_eq!(stages, 6);
        assert_eq!(event.turn, Turn::EventOver);
        let points = event.points();
        assert_eq!(points.len(), 7);
        assert!(points.windows(2).all(|w| w[0].points >= w[1].points));
        assert_eq!(points.iter().map(|r| r.points).sum::<u32>(), 2 * (25 + 18 + 15 + 12 + 10 + 8 + 6));
        // The save still round-trips at the end.
        assert_eq!(Event::load(&event.save()).unwrap(), event);
    }
}
