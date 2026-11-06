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
