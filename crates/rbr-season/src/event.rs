//! A built event (ours): the itinerary drawn from the stage pool, the field, the results so far, the standings and
//! points, the player's next turn, and the save in the player's folder.

use crate::ai::{self, Bulletin, RunLog, TeamCosts};
use crate::config::{Daylight, EventConfig, Length, StagePool, StageRef, Surfaces, Weather};
use crate::field::{self, AiResult, Contender, Entrant};
use crate::road::road_wear;
use crate::service::Crew;
use crate::team::crew_of;
use crate::track::{StageProfile, PAR_SCALE};
use rbr_formats::ai_ini::{Master, Roster, Section, Team, Track};
use rbr_gameplay::crt::CrtRand;

/// One stage of a rally's itinerary.
#[derive(Debug, Clone, PartialEq)]
pub struct ItineraryStage {
    pub stage: StageRef,
    /// The start clock (hours, 0 to 24): the launcher passes it as the stage's time of day.
    pub clock: f32,
    pub conditions: Conditions,
    /// The dry reference time (s) of the stage for the field's class.
    pub reference: f32,
    /// The stage's AI track (dry, at the field's best pace): the 1.02 AI engine drives it.
    pub track: Track,
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

/// The field's pace over its tracks' pars at difficulty `d`: 1 at the top, about 9 % slower at the bottom.
fn pace_for(difficulty: f32) -> f32 {
    1.0 / (1.0 - 0.08 * (1.0 - difficulty.clamp(0.0, 1.0)))
}

/// The pace the field may be adapted to (times the pars).
const PACE_RANGE: (f32, f32) = (0.97, 1.18);

/// An event under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub config: EventConfig,
    pub rallies: Vec<Rally>,
    pub field: Vec<Entrant>,
    /// What the 1.02 AI engine carries for each entrant (parallel to `field`): driver and car state, team, false starts.
    pub contenders: Vec<Contender>,
    /// The roster's teams and their repair spending (`ai::TeamCosts`).
    pub teams: Vec<Team>,
    pub costs: Vec<TeamCosts>,
    /// The field's pace over its tracks' pars: the difficulty sets it and the adaptive field moves it.
    pub pace: f32,
    /// Per rally, per stage: the player's result and each entrant's.
    pub results: Vec<Vec<(Option<PlayerResult>, Vec<AiResult>)>>,
    /// Per rally, per entrant: the late penalties (s) of its services.
    pub penalties: Vec<Vec<f32>>,
    /// Per rally: what the bulletins reported, in order.
    pub news: Vec<Vec<String>>,
    pub turn: Turn,
}

/// The team a roster without teams gets.
fn default_team() -> Team {
    Team {
        name: "Works team".into(),
        budget: 4,
        manager_style: 3,
        mechanics: 6,
        engineering: 3,
        specialities: vec![],
        difficulties: vec![],
        history: 0,
        nationality: 0,
        manager: String::new(),
    }
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
    /// daylight and weather, and the field from `class_cars` (catalog keys of the config's class), seated in `roster`'s
    /// teams and cars. `profile` gives a stage's profile for the class (`track::profile` over its driveline, or
    /// `StageProfile::flat` without one): its reference times and the AI track the engine drives.
    pub fn build(
        config: EventConfig,
        stages: &[StageRef],
        class_cars: &[String],
        roster: &Roster,
        profile: &dyn Fn(&StageRef) -> StageProfile,
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
            built.push(draw_rally(&config, &picked, leg_count, &mut rng, profile));
        }
        let (field, contenders) =
            field::field(class_cars, config.field_size as usize, config.difficulty, config.seed ^ 0xF1E1D, roster);
        let teams = if roster.teams.is_empty() { vec![default_team()] } else { roster.teams.clone() };
        let costs = teams.iter().map(TeamCosts::new).collect();
        let results = built.iter().map(|r| vec![(None, Vec::new()); r.stages.len()]).collect();
        let penalties = built.iter().map(|_| vec![0.0; field.len()]).collect();
        let news = built.iter().map(|_| Vec::new()).collect();
        let pace = pace_for(config.difficulty);
        Ok(Event {
            config,
            rallies: built,
            field,
            contenders,
            teams,
            costs,
            pace,
            results,
            penalties,
            news,
            turn: Turn::Welcome { rally: 0 },
        })
    }

    /// The player's service crew: the engineer and mechanics of the team the config picked.
    pub fn crew(&self) -> Crew {
        self.teams.get(self.config.team as usize).map_or_else(Crew::default, crew_of)
    }

    /// The rally and stage the results are being driven on, if the turn is a stage.
    fn at(&self) -> Option<(usize, usize)> {
        match self.turn {
            Turn::Stage { rally, stage } => Some((rally, stage)),
            _ => None,
        }
    }

    /// The player's stage result: the field drives the same stage with the 1.02 AI engine on `master`, and the turn
    /// moves on. A retired player is out of the rally: the field drives its remaining stages and the turn goes to the
    /// rally's end.
    pub fn stage_done(&mut self, master: &Master, result: PlayerResult) {
        let Some((rally, stage)) = self.at() else { return };
        self.results[rally][stage].0 = Some(result);
        self.drive_field(master, rally, stage);
        if result.retired {
            for later in stage + 1..self.rallies[rally].stages.len() {
                self.drive_field(master, rally, later);
            }
            self.turn = Turn::RallyOver { rally };
            return;
        }
        if self.config.adaptive {
            self.adapt(rally);
        }
        self.turn = next_turn(&self.rallies, rally, stage);
    }

    /// The field's stage, its bulletins and the service after it.
    fn drive_field(&mut self, master: &Master, rally: usize, stage: usize) {
        let it = &self.rallies[rally].stages[stage];
        let (conditions, service_after) = (it.conditions, it.service_after);
        let mut track = it.track;
        track.wet = u32::from(conditions == Conditions::Wet);
        // A damp road costs half the engine's wet penalty.
        let scale = self.pace * if conditions == Conditions::Damp { 1.045 } else { 1.0 };
        track.sections.iter_mut().for_each(|s| s.par *= scale);
        if stage == 0 {
            field::rally_start(&mut self.contenders);
            self.costs.iter_mut().for_each(|c| (c.service, c.rally) = (0, 0));
        }
        let order = self.start_order(rally, stage);
        let seed = self
            .config
            .seed
            .wrapping_add((rally as u32 + 1).wrapping_mul(0x9E37_79B1))
            .wrapping_add((stage as u32 + 1).wrapping_mul(0x85EB_CA6B));
        let mut rng = CrtRand::srand(seed);
        let mut log = RunLog::default();
        let mut out = vec![AiResult::Retired; self.field.len()];
        for &e in order.iter().flatten() {
            // Out of the rally for good once retired.
            if self.results[rally][..stage].iter().any(|(_, ai)| ai.get(e) == Some(&AiResult::Retired)) {
                continue;
            }
            let c = &mut self.contenders[e];
            let team = &self.teams[c.team.min(self.teams.len() - 1)];
            out[e] = field::drive_stage(master, &track, stage as u32, e as i32, c, team, &mut rng, &mut log);
        }
        let mut news = self.render(&log, stage);
        // The service after the stage: the AI cars' manager, repairs and late penalty.
        if let Some(minutes) = service_after {
            let long = minutes >= 40;
            let mut rng = CrtRand::srand(seed ^ 0x5E21_1CE0);
            let mut log = RunLog::default();
            for &e in order.iter().flatten() {
                if out[e] == AiResult::Retired {
                    continue;
                }
                let c = &mut self.contenders[e];
                let t = c.team.min(self.teams.len() - 1);
                let used = ai::service(
                    master,
                    e as i32,
                    &mut c.driver,
                    &mut c.car,
                    &self.teams[t],
                    &mut self.costs[t],
                    long,
                    &mut rng,
                    &mut log,
                );
                self.penalties[rally][e] += ai::service_penalty(e as i32, used, long, &mut log);
            }
            log.bulletins.retain(|b| matches!(b, Bulletin::ServicePenalty { .. }));
            news.extend(self.render(&log, stage));
        }
        self.news[rally].extend(news);
        self.results[rally][stage].1 = out;
    }

    /// The log as the bulletin lines of stage `stage`, in the engine's order: its bulletins, then the accidents that
    /// ended a run without one of their own.
    fn render(&self, log: &RunLog, stage: usize) -> Vec<String> {
        let name = |id: i32| self.field.get(id as usize).map_or_else(|| "A driver".to_owned(), |e| e.name.clone());
        let line = |text: String| format!("SS{}: {text}", stage + 1);
        let mut out = Vec::new();
        let mut explained = Vec::new();
        for b in &log.bulletins {
            match *b {
                // The manager's orders are the team's business.
                Bulletin::ManagerOrder { .. } | Bulletin::CarUnlock { .. } | Bulletin::Uneventful { .. } => {}
                Bulletin::DamageRetire { driver } | Bulletin::Hurt { driver, .. } | Bulletin::Dead { driver, .. } => {
                    explained.push(driver);
                    out.push(line(b.text(&name)));
                }
                _ => out.push(line(b.text(&name))),
            }
        }
        for a in log.accidents.iter().filter(|a| a.retired && a.accident != 0xff && !explained.contains(&a.driver)) {
            out.push(line(format!("{} retired after an accident in section {}", name(a.driver), a.section + 1)));
        }
        if log.accidents.is_empty() && log.bulletins.iter().all(|b| matches!(b, Bulletin::ManagerOrder { .. })) {
            out.push(line(Bulletin::Uneventful { stage: stage as u32 }.text(&name)));
        }
        out
    }

    /// What the bulletins reported in rally `rally`, in order: accidents, injuries, broken cars, false starts and late
    /// services, each with its stage.
    pub fn bulletins(&self, rally: usize) -> Vec<String> {
        self.news.get(rally).cloned().unwrap_or_default()
    }

    /// The start order of `rally`'s `stage`, the player as `None`. The first stage starts by the season points so far
    /// (the seed's draw for the first rally and for ties); later stages by the rally standings, leader first and the
    /// retired last.
    pub fn start_order(&self, rally: usize, stage: usize) -> Vec<Option<usize>> {
        let mut order: Vec<Option<usize>> = std::iter::once(None).chain((0..self.field.len()).map(Some)).collect();
        let mut rng = CrtRand::srand(self.config.seed ^ 0x57A2_7001);
        for i in (1..order.len()).rev() {
            let j = (rng.draw() as usize) % (i + 1);
            order.swap(i, j);
        }
        if stage > 0 {
            return self.standings_after(rally, stage).into_iter().map(|row| row.entrant).collect();
        }
        if rally > 0 {
            let points = self.points_through(rally);
            order.sort_by_key(|who| {
                std::cmp::Reverse(points.iter().find(|r| r.entrant == *who).map_or(0, |r| r.points))
            });
        }
        order
    }

    /// The road's wear for the player on `stage` (0 new, 1 normal, 2 worn): the 1.02 rule over the player's start place.
    pub fn road_wear(&self, rally: usize, stage: usize) -> f32 {
        let place = self.start_order(rally, stage).iter().position(Option::is_none);
        road_wear(place) as f32
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
        // A faster field drives its pars with a smaller pace.
        self.pace = (self.pace - delta).clamp(PACE_RANGE.0, PACE_RANGE.1);
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
        match self.results.get(rally) {
            Some(r) => self.standings_after(rally, run_stages(r)),
            None => Vec::new(),
        }
    }

    /// Rally `rally`'s standings after its first `run` stages, best first. The points go to the finishers only.
    fn standings_after(&self, rally: usize, run: usize) -> Vec<Standing> {
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
            row.points = if row.total.is_some() { POINTS.get(i).copied().unwrap_or(0) } else { 0 };
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
        let mut total = self.penalties.get(rally).and_then(|p| p.get(entrant)).copied().unwrap_or(0.0);
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
        self.points_through(self.rallies.len())
    }

    /// The points standings of the rallies before `upto` that are over: a rally is over once the field has driven all
    /// its stages, a retired player's included (who scores nothing in it).
    fn points_through(&self, upto: usize) -> Vec<Standing> {
        // Points come from the finished rallies only; the totals add the finished ones for the tie-break.
        let mut won = vec![0u32; self.field.len() + 1];
        let mut totals = vec![Some(0.0f32); self.field.len() + 1];
        for rally in 0..upto.min(self.rallies.len()) {
            if self.results[rally].iter().all(|(_, ai)| !ai.is_empty()) {
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
        let mut out = format!("{SAVE_HEAD}\n");
        let c = &self.config;
        out.push_str(&format!(
            "C|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
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
            skill_save(c.skill),
            c.team
        ));
        out.push_str(&format!("P|{}\n", self.pace));
        for rally in &self.rallies {
            out.push_str(&format!("R|{}\n", esc(&rally.name)));
            for s in &rally.stages {
                let sections: Vec<String> = s
                    .track
                    .sections
                    .iter()
                    .map(|k| {
                        format!(
                            "{}|{}|{}|{}|{}|{}|{}",
                            k.par,
                            k.material,
                            k.checks[0].0,
                            k.checks[0].1,
                            k.checks[1].0,
                            k.checks[1].1,
                            k.accident_table
                        )
                    })
                    .collect();
                out.push_str(&format!(
                    "S|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
                    esc(&s.stage.key),
                    esc(&s.stage.name),
                    s.stage.surface.name(),
                    if s.stage.community { 1 } else { 0 },
                    s.stage.length,
                    s.clock,
                    conditions_save(s.conditions),
                    s.service_after.map_or(String::new(), |m| m.to_string()),
                    s.reference,
                    sections.join("|")
                ));
            }
        }
        for t in 0..self.teams.len() {
            let (team, costs) = (&self.teams[t], &self.costs[t]);
            let list = |v: &[i32]| v.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
            out.push_str(&format!(
                "M|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
                esc(&team.name),
                team.budget,
                team.manager_style,
                team.mechanics,
                team.engineering,
                list(&team.specialities),
                list(&team.difficulties),
                team.history,
                team.nationality,
                esc(&team.manager),
                costs.budget,
                costs.service,
                costs.rally,
                costs.style
            ));
        }
        for (e, c) in self.field.iter().zip(&self.contenders) {
            let (d, st, car) = (&c.driver.driver, &c.driver, &c.car);
            let join = |v: &[String]| v.join(",");
            let nums = |v: &[u32]| join(&v.iter().map(u32::to_string).collect::<Vec<_>>());
            let fields = [
                esc(&e.name),
                esc(&e.car),
                e.skill.to_string(),
                e.consistency.to_string(),
                c.team.to_string(),
                c.false_starts.to_string(),
                esc(&d.name),
                d.photo.to_string(),
                d.technique.to_string(),
                d.temperament.to_string(),
                join(&d.skills.map(|v| v.to_string())),
                d.speed.to_string(),
                d.endurance.to_string(),
                d.condition.to_string(),
                d.history.to_string(),
                d.nationality.to_string(),
                esc(&d.codriver),
                st.injury.to_string(),
                st.injury_len.to_string(),
                st.co_injury.to_string(),
                st.co_injury_len.to_string(),
                st.vs_par.to_string(),
                join(&st.sections.map(|v| v.to_string())),
                esc(&car.car.name),
                car.car.control.to_string(),
                car.car.power.to_string(),
                car.car.handling.to_string(),
                car.car.durability.to_string(),
                nums(&car.parts),
                nums(&car.hits),
            ];
            out.push_str(&format!("F|{}\n", fields.join("|")));
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
        for (rally, p) in self.penalties.iter().enumerate() {
            out.push_str(&format!("Y|{rally}|{}\n", p.iter().map(f32::to_string).collect::<Vec<_>>().join(",")));
        }
        for (rally, lines) in self.news.iter().enumerate() {
            for line in lines {
                out.push_str(&format!("N|{rally}|{}\n", esc(line)));
            }
        }
        out.push_str(&format!("U|{}\n", turn_save(&self.turn)));
        out
    }

    /// An event read back from [`Event::save`]'s text.
    pub fn load(text: &str) -> Result<Event, String> {
        let mut lines = text.lines();
        match lines.next() {
            Some(SAVE_HEAD) => {}
            Some(head) if head.starts_with("rbr-season ") => {
                return Err(format!("an older save ({head}): not readable"))
            }
            Some(head) => return Err(format!("not an rbr-season save: {head}")),
            None => return Err("empty save".to_owned()),
        }
        let mut config: Option<EventConfig> = None;
        let mut rallies: Vec<Rally> = Vec::new();
        let mut field: Vec<Entrant> = Vec::new();
        let mut contenders: Vec<Contender> = Vec::new();
        let mut teams: Vec<Team> = Vec::new();
        let mut costs: Vec<TeamCosts> = Vec::new();
        let mut pace = None;
        let mut penalties: Vec<Vec<f32>> = Vec::new();
        let mut news: Vec<Vec<String>> = Vec::new();
        let mut results: Vec<Vec<(Option<PlayerResult>, Vec<AiResult>)>> = Vec::new();
        let mut turn: Option<Turn> = None;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<String> = split(line);
            let mut f = Fields::new(&parts);
            match f.next().unwrap_or("") {
                "C" => {
                    f.expect(14, "config")?;
                    let (class, car) = (f.text()?, f.text()?);
                    let length = length_load(f.raw()?)?;
                    let pool = pool_load(f.raw()?)?;
                    let surfaces = surfaces_load(f.raw()?)?;
                    let daylight = daylight_load(f.raw()?)?;
                    let weather = weather_load(f.raw()?)?;
                    let damage = damage_load(f.raw()?)?;
                    let difficulty = f.num("difficulty")?;
                    let adaptive = match f.raw()? {
                        "1" => true,
                        "0" => false,
                        _ => return Err("bad adaptive".to_owned()),
                    };
                    let (field_size, seed) = (f.num("field size")?, f.num("seed")?);
                    let skill = skill_load(f.raw()?)?;
                    let team = f.num("team")?;
                    config = Some(EventConfig {
                        class,
                        car,
                        team,
                        length,
                        pool,
                        surfaces,
                        daylight,
                        weather,
                        damage,
                        skill,
                        difficulty,
                        adaptive,
                        field_size,
                        seed,
                    });
                }
                "P" => pace = Some(f.num("pace")?),
                "R" => {
                    f.expect(1, "rally")?;
                    rallies.push(Rally { name: f.text()?, stages: Vec::new() });
                    results.push(Vec::new());
                    penalties.push(Vec::new());
                    news.push(Vec::new());
                }
                "S" => {
                    f.expect(30, "stage")?;
                    let rally = rallies.last_mut().ok_or("stage outside a rally".to_owned())?;
                    let key = f.text()?;
                    let name = f.text()?;
                    let surface = surface_load(f.raw()?)?;
                    let community = match f.raw()? {
                        "1" => true,
                        "0" => false,
                        _ => return Err("bad community".to_owned()),
                    };
                    let length = f.num("length")?;
                    let clock = f.num("clock")?;
                    let conditions = conditions_load(f.raw()?)?;
                    let service_after = match f.raw()? {
                        "" => None,
                        m => Some(m.parse().map_err(|_| "bad service".to_owned())?),
                    };
                    let reference = f.num("reference")?;
                    let mut sections = Vec::new();
                    for _ in 0..3 {
                        sections.push(Section {
                            par: f.num("par")?,
                            material: f.num("material")?,
                            checks: [(f.num("check")?, f.num("check")?), (f.num("check")?, f.num("check")?)],
                            accident_table: f.num("accident table")?,
                        });
                    }
                    let track = Track { stage: 0, wet: 0, sections: [sections[0], sections[1], sections[2]] };
                    rally.stages.push(ItineraryStage {
                        stage: StageRef { key, name, surface, community, length },
                        clock,
                        conditions,
                        reference,
                        track,
                        service_after,
                    });
                    results.last_mut().ok_or("stage outside a rally".to_owned())?.push((None, Vec::new()));
                }
                "M" => {
                    f.expect(14, "team")?;
                    let team = Team {
                        name: f.text()?,
                        budget: f.num("budget")?,
                        manager_style: f.num("style")?,
                        mechanics: f.num("mechanics")?,
                        engineering: f.num("engineering")?,
                        specialities: f.list("specialities")?,
                        difficulties: f.list("difficulties")?,
                        history: f.num("history")?,
                        nationality: f.num("nationality")?,
                        manager: f.text()?,
                    };
                    costs.push(TeamCosts {
                        budget: f.num("budget")?,
                        service: f.num("service")?,
                        rally: f.num("rally cost")?,
                        style: f.num("style")?,
                    });
                    teams.push(team);
                }
                "F" => {
                    f.expect(30, "entrant")?;
                    let (name, car, skill, consistency) =
                        (f.text()?, f.text()?, f.num("skill")?, f.num("consistency")?);
                    let (team, false_starts) = (f.num("team")?, f.num("false starts")?);
                    let driver = rbr_formats::ai_ini::Driver {
                        name: f.text()?,
                        photo: f.num("photo")?,
                        technique: f.num("technique")?,
                        temperament: f.num("temperament")?,
                        skills: f.array("skills")?,
                        speed: f.num("speed")?,
                        endurance: f.num("endurance")?,
                        condition: f.num("condition")?,
                        history: f.num("history")?,
                        nationality: f.num("nationality")?,
                        codriver: f.text()?,
                    };
                    let mut state = crate::ai::DriverState::new(driver);
                    state.injury = f.num("injury")?;
                    state.injury_len = f.num("injury length")?;
                    state.co_injury = f.num("co-driver injury")?;
                    state.co_injury_len = f.num("co-driver injury length")?;
                    state.vs_par = f.num("vs par")?;
                    state.sections = f.array("sections")?;
                    let mut car_state = crate::ai::CarState::new(rbr_formats::ai_ini::Car {
                        name: f.text()?,
                        control: f.num("control")?,
                        power: f.num("power")?,
                        handling: f.num("handling")?,
                        durability: f.num("durability")?,
                    });
                    car_state.parts = f.array("parts")?;
                    car_state.hits = f.array("hits")?;
                    field.push(Entrant { name, car, skill, consistency });
                    contenders.push(Contender { team, driver: state, car: car_state, false_starts });
                }
                "T" => {
                    f.expect(4, "result")?;
                    let (rally, stage) = (f.num::<usize>("result rally")?, f.num::<usize>("result stage")?);
                    let player = match f.raw()? {
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
                    let cells = f.raw()?;
                    if !cells.is_empty() {
                        for cell in cells.split(',') {
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
                "Y" => {
                    f.expect(2, "penalties")?;
                    let rally: usize = f.num("penalty rally")?;
                    let row = penalties.get_mut(rally).ok_or("penalties outside the itinerary".to_owned())?;
                    *row = f.list("penalties")?;
                }
                "N" => {
                    f.expect(2, "news")?;
                    let rally: usize = f.num("news rally")?;
                    news.get_mut(rally).ok_or("news outside the itinerary".to_owned())?.push(f.text()?);
                }
                "U" => {
                    f.expect(1, "turn")?;
                    turn = Some(turn_load(f.raw()?)?);
                }
                _ => return Err(format!("bad save line: {line}")),
            }
        }
        if field.len() != contenders.len() || teams.is_empty() || penalties.iter().any(|p| p.len() != field.len()) {
            return Err("an incomplete field".to_owned());
        }
        Ok(Event {
            config: config.ok_or("no config".to_owned())?,
            rallies,
            field,
            contenders,
            teams,
            costs,
            pace: pace.ok_or("no pace".to_owned())?,
            results,
            penalties,
            news,
            turn: turn.ok_or("no turn".to_owned())?,
        })
    }
}

/// The save's first line.
const SAVE_HEAD: &str = "rbr-season 2";

/// A save line's fields in order, read as the writer wrote them.
struct Fields<'a> {
    parts: std::slice::Iter<'a, String>,
    left: usize,
}

impl<'a> Fields<'a> {
    fn new(parts: &'a [String]) -> Self {
        Fields { parts: parts.iter(), left: parts.len() }
    }

    /// The line's tag (the first field).
    fn next(&mut self) -> Option<&'a str> {
        self.left = self.left.saturating_sub(1);
        self.parts.next().map(String::as_str)
    }

    /// The line has at least `n` fields after its tag.
    fn expect(&self, n: usize, what: &str) -> Result<(), String> {
        if self.left >= n {
            Ok(())
        } else {
            Err(format!("bad {what} line"))
        }
    }

    fn raw(&mut self) -> Result<&'a str, String> {
        self.parts.next().map(String::as_str).ok_or_else(|| "a short line".to_owned())
    }

    fn text(&mut self) -> Result<String, String> {
        self.raw().map(str::to_owned)
    }

    fn num<T: std::str::FromStr>(&mut self, what: &str) -> Result<T, String> {
        self.raw()?.parse().map_err(|_| format!("bad {what}"))
    }

    fn list<T: std::str::FromStr>(&mut self, what: &str) -> Result<Vec<T>, String> {
        let text = self.raw()?;
        if text.is_empty() {
            return Ok(Vec::new());
        }
        text.split(',').map(|v| v.parse().map_err(|_| format!("bad {what}"))).collect()
    }

    fn array<T: std::str::FromStr, const N: usize>(&mut self, what: &str) -> Result<[T; N], String> {
        self.list::<T>(what)?.try_into().map_err(|_| format!("bad {what}"))
    }
}

/// The stages run in a rally's results: every row the player or the field drove.
pub fn run_stages(results: &[(Option<PlayerResult>, Vec<AiResult>)]) -> usize {
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
    profile: &dyn Fn(&StageRef) -> StageProfile,
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
        let profile = profile(stage);
        let reference = profile.reference(wet);
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
        stages.push(ItineraryStage {
            stage: stage.clone(),
            clock: at,
            conditions,
            reference: profile.dry,
            track: profile.track(0, PAR_SCALE),
            service_after,
        });
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
        Skill::Professional => "professional",
        Skill::Champion => "champion",
    }
}

fn skill_load(text: &str) -> Result<crate::config::Skill, String> {
    use crate::config::Skill;
    match text {
        "rookie" => Ok(Skill::Rookie),
        "professional" => Ok(Skill::Professional),
        "champion" => Ok(Skill::Champion),
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

    fn profile(stage: &StageRef) -> StageProfile {
        StageProfile::flat(stage.length, stage.surface, &crate::pace::class_pace("Rally 2"))
    }

    fn roster() -> Roster {
        Roster {
            drivers: vec![],
            teams: (0..7)
                .map(|k| Team { name: format!("Team {k}"), engineering: 1 + k % 5, ..crate::ai::fixtures::team() })
                .collect(),
            cars: (0..8)
                .map(|k| rbr_formats::ai_ini::Car {
                    name: format!("c{k}"),
                    control: 3,
                    power: 3,
                    handling: 3,
                    durability: 3,
                })
                .collect(),
        }
    }

    fn master() -> Master {
        crate::ai::fixtures::master()
    }

    fn build(config: EventConfig, pool: &[StageRef], cars: &[String]) -> Event {
        Event::build(config, pool, cars, &roster(), &profile).unwrap()
    }

    fn finish(time: f32) -> PlayerResult {
        PlayerResult { time, penalties: 0.0, retired: false }
    }

    #[test]
    fn the_itinerary_respects_pool_surface_and_length() {
        let pool = mixed_pool();
        let cars = vec!["car".to_owned()];
        // Only gravel, retail only: the two Himalaya stages, twice over for a 4-stage rally.
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 4 };
        config.pool = StagePool::Retail;
        let event = build(config, &pool, &cars);
        assert_eq!(event.rallies.len(), 1);
        assert_eq!(event.rallies[0].stages.len(), 4);
        assert!(event.rallies[0].stages.iter().all(|s| !s.stage.community));
        assert!(event.rallies[0].stages.iter().all(|s| s.stage.surface == crate::config::Surface::Gravel));
        // No repeat while the pool suffices.
        let keys: Vec<&str> = event.rallies[0].stages.iter().map(|s| s.stage.key.as_str()).collect();
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 2 };
        config.pool = StagePool::Retail;
        let event = build(config, &pool, &cars);
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
        let event = build(config, &pool, &cars);
        assert_eq!(event.rallies.len(), 4);
        // Each rally keeps one surface.
        for rally in &event.rallies {
            assert_eq!(rally.stages.len(), 2);
            assert!(rally.stages.windows(2).all(|w| w[0].stage.surface == w[1].stage.surface));
        }
        let mut config = Preset::Weekend.config("Rally2", "car", 5);
        config.length = Length::Weekend { stages: 4 };
        let event = build(config, &pool, &cars);
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
        let event = build(config, &pool, &cars);
        let clocks: Vec<f32> = event.rallies[0].stages.iter().map(|s| s.clock).collect();
        assert!((clocks[0] - 8.0).abs() < 0.01, "{clocks:?}");
        assert!(clocks.windows(2).all(|w| w[1] > w[0]), "{clocks:?}");
        assert!(event.rallies[0].stages.iter().all(|s| s.conditions == Conditions::Dry));
        // Night puts every stage at night.
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 3 };
        config.daylight = Daylight::Night;
        let event = build(config, &pool, &cars);
        assert!(event.rallies[0].stages.iter().all(|s| s.clock >= 21.0 || s.clock < 8.0));
    }

    #[test]
    fn standings_score_the_top_ten_and_retirements() {
        let pool = gravel_pool();
        let cars = vec!["car-a".to_owned(), "car-b".to_owned(), "car-c".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 5);
        config.length = Length::Rally { stages: 2 };
        config.field_size = 3;
        let mut event = build(config, &pool, &cars);
        event.turn_done(); // Welcome -> stage 0.
        event.stage_done(&master(), finish(300.0));
        // Force known AI times for the standings check.
        event.results[0][0].1 = vec![AiResult::Time(290.0), AiResult::Time(310.0), AiResult::Retired];
        let standings = event.rally_standings(0);
        assert_eq!(standings.len(), 4);
        assert_eq!(standings[0], Standing { entrant: Some(0), total: Some(290.0), points: 25 });
        assert_eq!(standings[1], Standing { entrant: None, total: Some(300.0), points: 18 });
        assert_eq!(standings[2].points, 15);
        // A retired entrant scores nothing, whatever its place.
        assert_eq!(standings[3], Standing { entrant: Some(2), total: None, points: 0 });
        // The retired AI is out of the rally: still last after stage 2.
        event.turn_done();
        event.stage_done(&master(), PlayerResult { time: 300.0, penalties: 10.0, retired: false });
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
        let mut event = build(config, &pool, &cars);
        let before = event.pace;
        event.turn_done();
        // The player wins easily: the field speeds up (a smaller pace over its pars), by at most the step.
        event.stage_done(&master(), finish(100.0));
        assert!(event.pace < before && before - event.pace <= ADAPT_STEP + 1e-6, "{before} -> {}", event.pace);
        // The player crawls: the field slows down.
        let mid = event.pace;
        event.turn_done();
        event.stage_done(&master(), finish(900.0));
        assert!(event.pace > mid);
        assert!((PACE_RANGE.0..=PACE_RANGE.1).contains(&event.pace));
        assert_eq!(event.config.damage, Damage::Realistic);
    }

    #[test]
    fn the_save_round_trips() {
        let pool = mixed_pool();
        let cars = vec!["car-a".to_owned(), "car|b\\c".to_owned()];
        let mut config = Preset::Season.config("Rally 2", "car|b\\c", 5);
        config.length = Length::Season { rallies: 2, stages: 3 };
        config.team = 4;
        let mut event = build(config, &pool, &cars);
        event.turn_done();
        event.stage_done(&master(), PlayerResult { time: 301.5, penalties: 10.0, retired: false });
        event.turn_done();
        assert!(!event.bulletins(0).is_empty());
        let text = event.save();
        assert!(text.starts_with("rbr-season 2\n"));
        let back = Event::load(&text).unwrap();
        assert_eq!(event, back);
        assert_eq!(Event::load(&back.save()).unwrap(), back);
        assert_eq!(back.config.team, 4);
        assert!(Event::load("bogus").is_err());
        assert!(Event::load("rbr-season 1\nC|x").unwrap_err().contains("older"));
        assert!(Event::load("rbr-season 2\nbogus|line").is_err());
    }

    /// Drives the whole event with a fixed player time, returning the stages driven.
    fn run(event: &mut Event, time: f32) -> usize {
        let m = master();
        let mut stages = 0;
        loop {
            match event.turn.clone() {
                Turn::Welcome { .. } | Turn::Service { .. } | Turn::RallyOver { .. } => event.turn_done(),
                Turn::Stage { .. } => {
                    event.stage_done(&m, finish(time));
                    stages += 1;
                }
                Turn::EventOver => break,
            }
            assert!(stages < 200, "the event must end");
        }
        stages
    }

    #[test]
    fn a_full_season_runs_with_a_fixed_player_time() {
        let pool = gravel_pool();
        let cars = vec!["car-a".to_owned(), "car-b".to_owned()];
        let mut config = Preset::Season.config("Rally2", "car-a", 99);
        config.length = Length::Season { rallies: 2, stages: 6 };
        config.field_size = 12;
        let mut event = build(config, &pool, &cars);
        let ref_time = event.rallies[0].stages[0].reference;
        assert_eq!(run(&mut event, ref_time * 1.05), 12);
        assert_eq!(event.turn, Turn::EventOver);
        let points = event.points();
        assert_eq!(points.len(), 13);
        assert!(points.windows(2).all(|w| w[0].points >= w[1].points));
        // The points go to finishers, 25 + 18 + ... a rally at most.
        assert!(points.iter().map(|r| r.points).sum::<u32>() <= 2 * (25 + 18 + 15 + 12 + 10 + 8 + 6 + 4 + 2 + 1));
        assert!(!event.bulletins(0).is_empty());
        // The AI drove its stages with the engine: the times scatter around the pars.
        let times: Vec<f32> = event.results[0][0]
            .1
            .iter()
            .filter_map(|r| if let AiResult::Time(t) = r { Some(*t) } else { None })
            .collect();
        assert!(times.len() > 6 && times.iter().any(|t| (t - times[0]).abs() > 0.01), "{times:?}");
        // The save still round-trips at the end.
        assert_eq!(Event::load(&event.save()).unwrap(), event);
    }

    #[test]
    fn a_retired_player_ends_the_rally_and_scores_nothing() {
        let pool = gravel_pool();
        let cars = vec!["car".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 21);
        config.length = Length::Season { rallies: 2, stages: 4 };
        config.field_size = 8;
        let mut event = build(config, &pool, &cars);
        event.turn_done();
        event.stage_done(&master(), finish(300.0));
        event.turn_done();
        if matches!(event.turn, Turn::Service { .. }) {
            event.turn_done();
        }
        event.stage_done(&master(), PlayerResult { time: 0.0, penalties: 0.0, retired: true });
        assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
        // The field drove all four stages.
        assert!(event.results[0].iter().all(|(_, ai)| ai.len() == 8));
        let rows = event.rally_standings(0);
        let player = rows.iter().find(|r| r.entrant.is_none()).unwrap();
        assert_eq!((player.total, player.points), (None, 0));
        assert!(rows.iter().filter(|r| r.points > 0).count() >= 1);
        // The event goes on to the next rally, and the retired rally's points count for the field alone.
        event.turn_done();
        assert_eq!(event.turn, Turn::Welcome { rally: 1 });
        let points = event.points();
        assert_eq!(points.iter().find(|r| r.entrant.is_none()).unwrap().points, 0);
        assert!(points[0].points > 0);
    }

    #[test]
    fn the_start_order_follows_points_then_standings() {
        let pool = gravel_pool();
        let cars = vec!["car".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 33);
        config.length = Length::Season { rallies: 2, stages: 3 };
        config.field_size = 6;
        let mut event = build(config, &pool, &cars);
        let first = event.start_order(0, 0);
        assert_eq!(first.len(), 7);
        assert_eq!(first, event.start_order(0, 0));
        let mut sorted = first.clone();
        sorted.sort();
        assert_eq!(sorted, std::iter::once(None).chain((0..6).map(Some)).collect::<Vec<_>>());
        run_rally(&mut event, 0);
        // Stage 2 of the first rally starts by its standings, leader first.
        let standings: Vec<Option<usize>> = event.standings_after(0, 1).into_iter().map(|r| r.entrant).collect();
        assert_eq!(event.start_order(0, 1), standings);
        // The second rally's first stage starts by the season points.
        let points = event.points();
        let order = event.start_order(1, 0);
        let pts = |who: Option<usize>| points.iter().find(|r| r.entrant == who).unwrap().points;
        assert!(order.windows(2).all(|w| pts(w[0]) >= pts(w[1])), "{order:?}");
        // The road wear follows the player's place: the front three drive a new road.
        let place = event.start_order(0, 1).iter().position(Option::is_none).unwrap();
        assert_eq!(
            event.road_wear(0, 1),
            if place <= 2 {
                0.0
            } else if place <= 10 {
                1.0
            } else {
                2.0
            }
        );
    }

    fn run_rally(event: &mut Event, rally: usize) {
        let m = master();
        while !matches!(event.turn, Turn::RallyOver { rally: r } if r == rally) {
            match event.turn.clone() {
                Turn::Welcome { .. } | Turn::Service { .. } => event.turn_done(),
                Turn::Stage { .. } => event.stage_done(&m, finish(300.0)),
                _ => unreachable!(),
            }
        }
        event.turn_done();
        event.turn_done();
    }

    #[test]
    fn the_crew_is_the_picked_teams() {
        let pool = gravel_pool();
        let cars = vec!["car".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 1);
        config.team = 3;
        let event = build(config, &pool, &cars);
        assert_eq!(event.crew(), crate::service::Crew { a: 4, b: 6 });
        // An out-of-range team falls back to the default crew.
        let mut config = Preset::QuickRally.config("Rally2", "car", 1);
        config.team = 99;
        assert_eq!(build(config, &pool, &cars).crew(), crate::service::Crew::default());
    }

    #[test]
    fn the_ai_is_serviced_between_stages() {
        let pool = gravel_pool();
        let cars = vec!["car".to_owned()];
        let mut config = Preset::QuickRally.config("Rally2", "car", 4);
        config.length = Length::Rally { stages: 3 };
        config.field_size = 4;
        let mut event = build(config, &pool, &cars);
        event.turn_done();
        event.stage_done(&master(), finish(300.0));
        // The service falls after the second of three stages: a worn car is repaired in it, at the team's cost.
        event.turn_done();
        for c in &mut event.contenders {
            c.car.parts[2] = 3;
        }
        assert_eq!(event.costs.iter().map(|c| c.rally).sum::<u32>(), 0);
        event.stage_done(&master(), finish(300.0));
        assert!(matches!(event.turn, Turn::Service { minutes: 20, .. }));
        let spent: u32 = event.costs.iter().map(|c| c.rally).sum();
        assert!(spent > 0, "the service repaired nothing");
        let alive = (0..4).filter(|&e| event.results[0][1].1[e] != AiResult::Retired).collect::<Vec<_>>();
        assert!(!alive.is_empty() && alive.iter().all(|&e| event.contenders[e].car.parts[2] < 3));
    }
}
