//! Building a championship: the itinerary drawn from the stage pool by the config, the clocks, conditions and
//! services, and the field seated in the roster's teams and cars.

use super::*;

/// The mid-leg service (minutes).
const SERVICE_MID: u32 = 20;

/// The overnight service between a rally's legs (minutes).
pub(super) const SERVICE_NIGHT: u32 = 45;

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
        let Length { rallies, legs, stages: count } = config.length;
        if rallies == 0 || count == 0 {
            return Err("an event needs rallies with stages".to_owned());
        }
        if legs == 0 || legs > count {
            return Err("a rally's legs need a stage each".to_owned());
        }
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
        for _ in 0..rallies {
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
        for &surface in &rally_surface {
            let mut candidates: Vec<&StageRef> = pool.iter().filter(|s| s.surface == surface).copied().collect();
            if candidates.is_empty() {
                return Err(format!("no {:?} stages in the pool", surface.name()));
            }
            // Prefer one stage family (the same location): the fullest, ties drawn by the seed.
            let mut families: Vec<(&str, usize)> = Vec::new();
            for s in &candidates {
                let f = s.family.as_str();
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
                candidates.drain(..).partition(|s| s.family.as_str() == home);
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
            built.push(draw_rally(&config, &picked, legs, &mut rng, profile));
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
}

/// The stages of each of `legs` legs over `stages`, the earlier legs the longer.
fn leg_sizes(stages: usize, legs: usize) -> Vec<usize> {
    (0..legs).map(|leg| stages / legs + usize::from(leg < stages % legs)).collect()
}

/// Draws one rally's itinerary: each leg a day, its start clocks from 08:00 (21:00 at night) with service gaps; the
/// conditions by the weather (changeable per leg); on realistic daylight the first day's last stages at dusk or night.
fn draw_rally(
    config: &EventConfig,
    picked: &[StageRef],
    legs: u8,
    rng: &mut CrtRand,
    profile: &dyn Fn(&StageRef) -> StageProfile,
) -> Rally {
    let sizes = leg_sizes(picked.len(), legs.clamp(1, picked.len().max(1) as u8) as usize);
    // Changeable weather is drawn per leg.
    let mut leg_conditions = Vec::new();
    for _ in &sizes {
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
    let morning: f32 = match config.daylight {
        Daylight::Night => 21.0,
        _ => 8.0,
    };
    let mut stages = Vec::new();
    let mut next = picked.iter();
    for (leg, &size) in sizes.iter().enumerate() {
        // A leg is a day: it starts the next morning (evening at night).
        let mut clock = morning;
        let dusk_from = size.saturating_sub(2);
        for pos in 0..size {
            let Some(stage) = next.next() else { break };
            let conditions = leg_conditions[leg];
            let wet = conditions.wetness();
            let profile = profile(stage);
            let reference = profile.reference(wet);
            if matches!(config.daylight, Daylight::Realistic) && leg == 0 && pos >= dusk_from && size > 1 {
                clock = clock.max(19.0 + 0.5 * (pos - dusk_from) as f32);
            }
            let at = clock % 24.0;
            // The next stage follows the reference time and a gap, longer after a wet stage.
            clock += reference / 3600.0 + 0.45 + 0.15 * wet;
            // Services: about 20 minutes mid-leg, 45 overnight at the end of a leg but the last.
            let last_leg = leg + 1 == sizes.len();
            let service_after = if pos + 1 == size && !last_leg {
                Some(SERVICE_NIGHT)
            } else if pos + 1 == size.div_ceil(2) && !(last_leg && pos + 1 == size) {
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
    }
    Rally { name: rally_name(picked), stages }
}
