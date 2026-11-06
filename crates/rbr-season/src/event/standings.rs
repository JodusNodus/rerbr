//! The standings: a rally's times after the stages run, and the points over the championship.

use super::*;

impl Event {
    /// Rally `rally`'s standings after the stages run so far, best first.
    pub fn rally_standings(&self, rally: usize) -> Vec<Standing> {
        match self.results.get(rally) {
            Some(r) => self.standings_after(rally, run_stages(r)),
            None => Vec::new(),
        }
    }

    /// Rally `rally`'s standings after its first `run` stages, best first. The points go to the finishers only.
    pub fn standings_after(&self, rally: usize, run: usize) -> Vec<Standing> {
        let mut rows = Vec::new();
        rows.push(Standing { entrant: None, total: self.player_total(rally, run), points: 0, bonus: 0 });
        for (i, _) in self.field.iter().enumerate() {
            rows.push(Standing { entrant: Some(i), total: self.entrant_total(rally, i, run), points: 0, bonus: 0 });
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
        // The power stage: once the rally is complete, its last stage pays the five fastest classified finishers.
        if self.config.power_stage && self.rallies.get(rally).is_some_and(|r| run > 0 && run == r.stages.len()) {
            let (player, ai) = &self.results[rally][run - 1];
            let time = |row: &Standing| match row.entrant {
                None => player.filter(|p| !p.retired).map(|p| p.time),
                Some(e) => match ai.get(e) {
                    Some(AiResult::Time(t)) => Some(*t),
                    _ => None,
                },
            };
            let mut fastest: Vec<(usize, f32)> = rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.total.is_some())
                .filter_map(|(at, r)| Some((at, time(r)?)))
                .collect();
            // The sort is stable: a tie goes to the better placed.
            fastest.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (&(at, _), bonus) in fastest.iter().zip(POWER_POINTS) {
                rows[at].bonus = bonus;
                rows[at].points += bonus;
            }
        }
        rows
    }

    /// The fastest finished time on `rally`'s `stage` (the field is one class).
    fn fastest(&self, rally: usize, stage: usize) -> Option<f32> {
        let (player, ai) = self.results.get(rally)?.get(stage)?;
        let ai = ai.iter().filter_map(|r| match r {
            AiResult::Time(t) => Some(*t),
            AiResult::Retired | AiResult::Out => None,
        });
        player.filter(|p| !p.retired).map(|p| p.time).into_iter().chain(ai).reduce(f32::min)
    }

    /// Super Rally's time for `who` (the player as `None`) on `rally`'s `stage`, a stage run that the crew did not
    /// complete but restarts after: the stage's fastest time plus [`RESTART_CHARGE`] (its reference when no one
    /// finished). `None` without restart, for a completed or unrun stage, for a crew out for good, or when no leg
    /// follows.
    pub fn charged(&self, rally: usize, stage: usize, who: Option<usize>) -> Option<f32> {
        if !self.config.restart || stage >= run_stages(self.results.get(rally)?) {
            return None;
        }
        let (player, ai) = &self.results[rally][stage];
        let missed = match who {
            None => player.is_none_or(|p| p.retired),
            Some(e) => ai.get(e) == Some(&AiResult::Retired),
        };
        if !missed {
            return None;
        }
        self.rejoin_after(rally, stage)?;
        let reference = self.rallies.get(rally)?.stages.get(stage)?.reference;
        Some(self.fastest(rally, stage).unwrap_or(reference) + RESTART_CHARGE)
    }

    fn player_total(&self, rally: usize, run: usize) -> Option<f32> {
        let mut total = 0.0;
        for (stage, (result, _)) in self.results[rally].iter().take(run).enumerate() {
            match result {
                Some(p) if !p.retired => total += p.time + p.penalties,
                // Under Super Rally a stage missed before a restart is charged.
                _ => total += self.charged(rally, stage, None)?,
            }
        }
        Some(total)
    }

    fn entrant_total(&self, rally: usize, entrant: usize, run: usize) -> Option<f32> {
        let mut total = self.penalties.get(rally).and_then(|p| p.get(entrant)).copied().unwrap_or(0.0);
        for (stage, (_, ai)) in self.results[rally].iter().take(run).enumerate() {
            match ai.get(entrant) {
                Some(AiResult::Time(t)) => total += t,
                _ => total += self.charged(rally, stage, Some(entrant))?,
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
    pub(super) fn points_through(&self, upto: usize) -> Vec<Standing> {
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
                bonus: 0,
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
}
