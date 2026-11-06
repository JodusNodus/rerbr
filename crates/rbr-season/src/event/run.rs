//! Running a stage: the field drives it on the 1.02 AI engine in start order, the AI's services, the bulletins, the
//! road order and wear, and the adaptive field.

use super::*;

/// The adaptive step: the most a stage moves an entrant's skill.
pub(super) const ADAPT_STEP: f32 = 0.003;

impl Event {
    /// Super Rally (WRC): a crew that retires on `stage` of `rally` restarts at the next leg. The last stage of the leg
    /// (the overnight service after it), the crew sitting out to it; `None` when restart is off or no leg follows.
    pub fn rejoin_after(&self, rally: usize, stage: usize) -> Option<usize> {
        if !self.config.restart {
            return None;
        }
        let stages = &self.rallies.get(rally)?.stages;
        (stage..stages.len()).find(|&k| stages[k].ends_leg() && k + 1 < stages.len())
    }

    /// Whether entrant `e` sits out `rally`'s `stage`, and as what: out for good once hurt or excluded, retired until
    /// the leg after its retirement (for the rest of the rally without Super Rally or a leg to follow).
    fn sits_out(&self, rally: usize, stage: usize, e: usize) -> Option<AiResult> {
        let before = &self.results[rally][..stage];
        if before.iter().any(|(_, ai)| ai.get(e) == Some(&AiResult::Out)) {
            return Some(AiResult::Out);
        }
        let retired = before.iter().rposition(|(_, ai)| ai.get(e) == Some(&AiResult::Retired))?;
        match self.rejoin_after(rally, retired) {
            Some(k) if k < stage => None,
            _ => Some(AiResult::Retired),
        }
    }

    /// The player's stage result: the field drives the same stage with the 1.02 AI engine on `master`, and the turn
    /// moves on. A retired player restarts at the next leg under Super Rally (the field drives to the overnight service);
    /// otherwise the player is out of the rally: the field drives its remaining stages and the turn goes to the rally's
    /// end.
    pub fn stage_done(&mut self, master: &Master, result: PlayerResult) {
        let Some((rally, stage)) = self.at() else { return };
        self.results[rally][stage].0 = Some(result);
        self.drive_field(master, rally, stage);
        if result.retired {
            if let Some(k) = self.rejoin_after(rally, stage) {
                for later in stage + 1..=k {
                    self.drive_field(master, rally, later);
                }
                let minutes = self.rallies[rally].stages[k].service_after.unwrap_or(0);
                self.turn = Turn::Service { rally, stage: k, minutes };
                return;
            }
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
        let car_factors: Vec<f32> = (0..self.field.len()).map(|e| it.car_factor(e)).collect();
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
        let mut restarts = Vec::new();
        for &e in order.iter().flatten() {
            if let Some(sitting) = self.sits_out(rally, stage, e) {
                out[e] = sitting;
                continue;
            }
            if stage > 0 && self.results[rally][stage - 1].1.get(e) == Some(&AiResult::Retired) {
                restarts.push(format!("SS{}: {} restarts under Super Rally", stage + 1, self.field[e].name));
            }
            let c = &mut self.contenders[e];
            let team = &self.teams[c.team.min(self.teams.len() - 1)];
            // The entrant's car model: its pars by the car's pace (the engine itself untouched).
            let mut own = track;
            own.sections.iter_mut().for_each(|s| s.par *= car_factors[e]);
            out[e] = field::drive_stage(master, &own, stage as u32, e as i32, c, team, &mut rng, &mut log);
        }
        let mut news = restarts;
        news.extend(self.render(&log, stage, news.is_empty()));
        // The service after the stage: the AI cars' manager, repairs and late penalty.
        if let Some(minutes) = service_after {
            let long = minutes >= 40;
            let mut rng = CrtRand::srand(seed ^ 0x5E21_1CE0);
            let mut log = RunLog::default();
            for &e in order.iter().flatten() {
                if !matches!(out[e], AiResult::Time(_)) {
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
            news.extend(self.render(&log, stage, false));
        }
        // Overnight the retired cars that restart the next leg are rebuilt.
        if self.rallies[rally].stages[stage].ends_leg() && self.config.restart {
            for (e, c) in self.contenders.iter_mut().enumerate() {
                if !matches!(out[e], AiResult::Time(_)) {
                    (c.car.parts, c.car.hits) = ([0; 8], [0; 8]);
                }
            }
        }
        self.news[rally].extend(news);
        self.results[rally][stage].1 = out;
    }

    /// The log as the news of stage `stage` ([`news`]), with the field's names.
    fn render(&self, log: &RunLog, stage: usize, quiet: bool) -> Vec<String> {
        let name = |id: i32| self.field.get(id as usize).map_or_else(|| "A driver".to_owned(), |e| e.name.clone());
        news(log, stage, &name, quiet)
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
}

/// Under this a driver's accident losses on a stage are not news (s).
pub(super) const MINOR_LOSS: f32 = 5.0;

/// A stage's log as its news lines, "SS2: ...", in the engine's order without the manager's orders: each driver's
/// accident losses summed into one line where they first lost time (none under [`MINOR_LOSS`]), the first time a
/// driver was shaken, then broken cars, injuries, false starts, late services, and the accidents that ended a run
/// without a line of their own. With nothing to tell, "an uneventful stage" when `quiet` (a stage's log, not a
/// service's).
pub(super) fn news(log: &RunLog, stage: usize, name: &dyn Fn(i32) -> String, quiet: bool) -> Vec<String> {
    let line = |text: String| format!("SS{}: {text}", stage + 1);
    // Each driver's accidents: the time lost and how many.
    let mut lost: Vec<(i32, f32, usize)> = Vec::new();
    for b in &log.bulletins {
        if let Bulletin::Accident { driver, time, .. } = *b {
            match lost.iter_mut().find(|l| l.0 == driver) {
                Some(l) => (l.1, l.2) = (l.1 + time, l.2 + 1),
                None => lost.push((driver, time, 1)),
            }
        }
    }
    let (mut out, mut explained, mut told) = (Vec::new(), Vec::new(), Vec::new());
    for b in &log.bulletins {
        match *b {
            Bulletin::ManagerOrder { .. } | Bulletin::CarUnlock { .. } | Bulletin::Uneventful { .. } => {}
            Bulletin::Accident { driver, .. } => {
                if told.contains(&(driver, 0)) {
                    continue;
                }
                told.push((driver, 0));
                let Some(&(_, time, n)) = lost.iter().find(|l| l.0 == driver).filter(|l| l.1 >= MINOR_LOSS) else {
                    continue;
                };
                let how = if n == 1 { "an accident".to_owned() } else { format!("{n} accidents") };
                out.push(line(format!("{} lost {time:.1} s in {how}", name(driver))));
            }
            Bulletin::Shaken { driver, .. } => {
                if !told.contains(&(driver, 1)) {
                    told.push((driver, 1));
                    out.push(line(b.text(name)));
                }
            }
            Bulletin::DamageRetire { driver } | Bulletin::Hurt { driver, .. } | Bulletin::Dead { driver, .. } => {
                explained.push(driver);
                out.push(line(b.text(name)));
            }
            _ => out.push(line(b.text(name))),
        }
    }
    for a in log.accidents.iter().filter(|a| a.retired && a.accident != 0xff && !explained.contains(&a.driver)) {
        out.push(line(format!("{} retired after an accident in section {}", name(a.driver), a.section + 1)));
    }
    if out.is_empty() && quiet {
        out.push(line(Bulletin::Uneventful { stage: stage as u32 }.text(name)));
    }
    out
}
