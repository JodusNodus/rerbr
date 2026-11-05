//! The AI field in the player's class (ours): entrants with a skill and a consistency, and their stage times
//! around the stage's reference time.
//!
//! Skills are a pace fraction: 1 drives the reference. The difficulty sets the best entrant: at 1 the best equals
//! the reference, at 0 the best is about 8 % slower; the rest spread below. A stage time is the reference over the
//! skill with a per-stage noise, rarer bigger losses (a spin, a puncture: 5-60 s), a small retire chance, and a wider
//! spread on wet stages. All draws come from the CRT rand (`rbr_gameplay::crt::CrtRand`), so the seed decides
//! everything.

use rbr_gameplay::crt::CrtRand;

/// An AI entrant.
#[derive(Debug, Clone, PartialEq)]
pub struct Entrant {
    pub name: String,
    /// The catalog car key it drives (shown in the standings), a car of the player's class.
    pub car: String,
    /// 1 drives the reference pace; lower is slower.
    pub skill: f32,
    /// 1 never errs; lower spreads its times and risks time losses and retirements.
    pub consistency: f32,
}

/// An AI entrant's result on one stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiResult {
    Time(f32),
    Retired,
}

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

/// `size` entrants for `difficulty` (0 to 1), driving `cars` (catalog keys of the class; repeated as needed).
pub fn field(cars: &[String], size: usize, difficulty: f32, seed: u32) -> Vec<Entrant> {
    let mut rng = CrtRand::srand(seed);
    // Shuffle the names so the seed picks the crews.
    let mut names: Vec<&str> = NAMES.to_vec();
    for i in (1..names.len()).rev() {
        let j = (rng.draw() as usize) % (i + 1);
        names.swap(i, j);
    }
    let best = 1.0 - 0.08 * (1.0 - difficulty.clamp(0.0, 1.0));
    let mut out = Vec::with_capacity(size);
    for i in 0..size {
        let skill = best - 0.09 * unit(&mut rng);
        let consistency = 0.55 + 0.45 * unit(&mut rng);
        let car = if cars.is_empty() { format!("car-{}", i + 1) } else { cars[i % cars.len()].clone() };
        out.push(Entrant { name: names[i % names.len()].to_owned(), car, skill, consistency });
    }
    // The best equals the reference pace at difficulty 1 (about 8 % off it at 0).
    if let Some(top) = out.iter_mut().max_by(|a, b| a.skill.total_cmp(&b.skill)) {
        top.skill = best;
    }
    out
}

/// An entrant's time over a stage with reference `reference` at `wetness` 0 (dry) to 1: the reference over the
/// skill with noise, rarer big losses and a small retire chance; wet stages spread the times.
pub fn drive_stage(entrant: &Entrant, reference: f32, wetness: f32, rng: &mut CrtRand) -> AiResult {
    let wet = wetness.clamp(0.0, 1.0);
    // Retirements first: about 0.1 % a stage at full consistency, 1 % at the erratic end, wetter is worse.
    let p_retire = (1.05 - entrant.consistency).max(0.0) * 0.02 * (1.0 + wet);
    if unit(rng) < p_retire {
        return AiResult::Retired;
    }
    let sigma = 0.012 * (1.15 - 0.65 * entrant.consistency) * (1.0 + 0.6 * wet);
    let noise = (unit(rng) + unit(rng) + unit(rng)) / 3.0 - 0.5;
    let mut time = reference / entrant.skill.max(0.5) * (1.0 + noise * 6.0 * sigma);
    // Bigger losses: a spin or a puncture costs 5-60 s, skewed to the small end.
    let p_loss = (1.15 - entrant.consistency).max(0.0) * 0.10 * (1.0 + 0.5 * wet);
    if unit(rng) < p_loss {
        time += 5.0 + 55.0 * unit(rng) * unit(rng);
    }
    AiResult::Time(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cars() -> Vec<String> {
        vec!["car-a".to_owned(), "car-b".to_owned()]
    }

    #[test]
    fn the_field_is_deterministic_and_spread_by_difficulty() {
        let (a, b) = (field(&cars(), 12, 0.6, 42), field(&cars(), 12, 0.6, 42));
        assert_eq!(a, b);
        assert_ne!(field(&cars(), 12, 0.6, 42), field(&cars(), 12, 0.6, 43));
        for difficulty in [0.0, 0.6, 1.0] {
            let f = field(&cars(), 12, difficulty, 7);
            let best = f.iter().map(|e| e.skill).fold(0.0f32, f32::max);
            let want = 1.0 - 0.08 * (1.0 - difficulty);
            assert!((best - want).abs() < 1e-6, "{difficulty}: best {best} want {want}");
            assert!(f.iter().all(|e| e.skill <= best && e.consistency >= 0.55 && e.consistency <= 1.0));
        }
        // At difficulty 1 the best equals the reference; at 0 it is about 8 % slower.
        assert!((field(&cars(), 12, 1.0, 7).iter().map(|e| e.skill).fold(0.0f32, f32::max) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn stage_times_follow_skill_and_seed() {
        let f = field(&cars(), 4, 1.0, 9);
        let run = |seed| {
            let mut rng = CrtRand::srand(seed);
            f.iter().map(|e| drive_stage(e, 300.0, 0.0, &mut rng)).collect::<Vec<_>>()
        };
        assert_eq!(run(1), run(1));
        assert_ne!(run(1), run(2));
        // The best entrant beats the worst on a clean stage far more often than not.
        let (mut wins, total) = (0, 200);
        for seed in 0..total {
            let mut rng = CrtRand::srand(seed);
            let times: Vec<Option<f32>> = f
                .iter()
                .map(|e| match drive_stage(e, 300.0, 0.0, &mut rng) {
                    AiResult::Time(t) => Some(t),
                    AiResult::Retired => None,
                })
                .collect();
            if let (Some(a), Some(b)) = (times[0], times[3]) {
                if a < b {
                    wins += 1;
                }
            }
        }
        assert!(wins > total / 2, "{wins}/{total}");
    }

    #[test]
    fn wet_stages_spread_the_times() {
        let f = field(&cars(), 8, 0.7, 11);
        let spread = |wet: f32| {
            let mut rng = CrtRand::srand(3);
            let mut ts: Vec<f32> = f
                .iter()
                .filter_map(|e| match drive_stage(e, 300.0, wet, &mut rng) {
                    AiResult::Time(t) => Some(t),
                    AiResult::Retired => None,
                })
                .collect();
            ts.sort_by(|a, b| a.total_cmp(b));
            ts.last().unwrap() - ts.first().unwrap()
        };
        assert!(spread(1.0) > spread(0.0), "{} vs {}", spread(1.0), spread(0.0));
    }
}
