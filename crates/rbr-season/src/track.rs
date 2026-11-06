//! A stage's AI [`Track`] (ours) for any stage: the 1.02 AI engine (`ai::stage_time`) drives three sections with two
//! checks each; this module makes them from the stage's driveline.
//!
//! - The sections are the stage's splits (the retail `ai.ini` cuts its sections there), thirds of the length when the
//!   stage has none.
//! - A section's par is its share of the speed plan's time ([`pace`]), scaled by [`PAR_SCALE`] so that the field's
//!   best driver lands on the reference time (the 1.02 AI's best on the retail stages).
//! - Each check describes one half of its section. A half with a long straight is a straights check (type 3, the
//!   value from the straight's share); any other is a curves check (type 0, the value from how far the corners hold the
//!   plan below top speed). The loosest curves half of a section on snow takes the rough check (type 1) and one on
//!   gravel the obstacles check (type 2), as the retail rows do.
//! - The material is the surface's; the accident table is the master's first (the retail rows all use it).

use rbr_formats::ai_ini::{Section, Track};

use crate::config::Surface;
use crate::pace::{self, ClassPace};

/// The sum of a track's section pars over its reference time. The engine's par draws (negative for a good driver) and
/// mistakes (rare for one) net out for the field's best driver, whose mean time lands within 0.2 % of the reference
/// (measured by `examples/track.rs`: 0.999 at a scale of 1, sd 0.0015 over the retail stages), so the pars are the
/// reference's shares as they are.
pub const PAR_SCALE: f32 = 1.0;

/// A straight counts when the plan runs at top speed over at least this length (m).
const LONG_STRAIGHT: f32 = 150.0;

/// One half of a section, as the checks see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Half {
    /// 0 (flat out) to 1 (a hairpin all the way): how far the corners hold the plan below top speed.
    pub tightness: f32,
    /// The share of the half's length on long straights.
    pub straight: f32,
}

/// One section of a stage's profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionShape {
    /// The section's share of the stage's time.
    pub share: f32,
    pub halves: [Half; 2],
}

/// What the AI needs to know of a stage on a surface for a class: its reference time and its sections' character.
#[derive(Debug, Clone, PartialEq)]
pub struct StageProfile {
    pub surface: Surface,
    /// The dry reference time (s) and the wet one.
    pub dry: f32,
    pub wet: f32,
    pub sections: [SectionShape; 3],
}

impl StageProfile {
    /// The reference time at `wetness` 0 (dry) to 1.
    pub fn reference(&self, wetness: f32) -> f32 {
        self.dry + (self.wet - self.dry) * wetness.clamp(0.0, 1.0)
    }

    /// A profile for a stage without a driveline (a BTB stage): `length` metres at the class's average gravel pace on
    /// the surface, a middling stage in thirds.
    pub fn flat(length: f32, surface: Surface, class: &ClassPace) -> Self {
        let pace = match surface {
            Surface::Tarmac => 0.62,
            Surface::Gravel => 0.55,
            Surface::Snow => 0.45,
        };
        let dry = length.max(1.0) / (class.top_speed * pace);
        let half = Half { tightness: 0.45, straight: 0.0 };
        Self { surface, dry, wet: dry * 1.09, sections: [SectionShape { share: 1.0 / 3.0, halves: [half; 2] }; 3] }
    }

    /// The AI track for map `stage` (dry; the engine's `wet` flag adds its 9 %). The pars are `scale` times the
    /// profile's share of the dry reference.
    pub fn track(&self, stage: u32, scale: f32) -> Track {
        let material = match self.surface {
            Surface::Gravel => 0,
            Surface::Tarmac => 1,
            Surface::Snow => 2,
        };
        let sections = std::array::from_fn(|k| {
            let s = &self.sections[k];
            Section { par: self.dry * s.share * scale, material, checks: self.checks(s), accident_table: 0 }
        });
        Track { stage, wet: 0, sections }
    }

    fn checks(&self, s: &SectionShape) -> [(u32, u32); 2] {
        let mut checks = s.halves.map(|h| {
            if h.straight >= STRAIGHT_SHARE || h.tightness < STRAIGHT_TIGHTNESS {
                (3, ((3.0 + 6.0 * (MEAN_STRAIGHT - h.tightness)).round() as i32).clamp(1, 5) as u32)
            } else {
                (0, ((3.0 + 6.0 * (h.tightness - MEAN_TIGHTNESS)).round() as i32).clamp(1, 5) as u32)
            }
        });
        // The section's second half, when it is a curves half looser than the first, takes the surface's own check:
        // rough on snow, obstacles on gravel (the retail rows have them there; tarmac's few are not predicted).
        let loose = match self.surface {
            Surface::Snow => Some((1, 1.0)),
            Surface::Gravel => Some((2, LOOSE_GRAVEL)),
            Surface::Tarmac => None,
        };
        if let Some((kind, ratio)) = loose {
            if checks[1].0 == 0 && s.halves[1].tightness < ratio * s.halves[0].tightness {
                checks[1] = (kind, 2);
            }
        }
        checks
    }
}

/// A half with this share of long straights is a straights check.
const STRAIGHT_SHARE: f32 = 0.7;
/// A half looser than this is a straights check too: the retail rows' straights are their stages' loosest halves
/// (28 % of the halves; this cut takes 29 %, half of them retail straights against their 28 % base rate).
const STRAIGHT_TIGHTNESS: f32 = 0.235;
/// The retail stages' mean tightness of the halves on either side of the cut (the value 3 of each check), so the
/// values spread as the retail ones do (curves 2/3/4 at 23/61/13 %).
const MEAN_STRAIGHT: f32 = 0.18;
const MEAN_TIGHTNESS: f32 = 0.33;
/// A gravel half looser than this fraction of the section's first half takes the obstacles check.
const LOOSE_GRAVEL: f32 = 0.4;

/// The profile of a stage over `driveline` (the timed slice's points, m), with the stage's `splits` (metres from its
/// start; thirds unless there are exactly two, ascending and inside the stage).
pub fn profile(driveline: &[[f32; 3]], splits: &[f32], surface: Surface, class: &ClassPace) -> StageProfile {
    let (Some(dry), Some(wet)) =
        (pace::plan(driveline, surface, 0.0, class), pace::plan(driveline, surface, 1.0, class))
    else {
        return StageProfile::flat(pace::path_length(driveline), surface, class);
    };
    let n = dry.samples.len() - 1;
    let length = n as f32 * pace::STEP;
    let cuts = match splits {
        [a, b] if 0.0 < *a && a < b && *b < length => [dry.index(*a), dry.index(*b)],
        _ => [n / 3, 2 * n / 3],
    };
    let bounds = [0, cuts[0], cuts[1], n];
    let total = dry.time(0, n).max(1e-3);
    let sections = std::array::from_fn(|k| {
        let (from, to) = (bounds[k], bounds[k + 1].max(bounds[k] + 1));
        let mid = (from + to) / 2;
        SectionShape { share: dry.time(from, to) / total, halves: [half(&dry, from, mid), half(&dry, mid, to)] }
    });
    StageProfile { surface, dry: total, wet: wet.time(0, wet.samples.len() - 1), sections }
}

/// The character of samples `from..to`.
fn half(plan: &pace::Plan, from: usize, to: usize) -> Half {
    let to = to.max(from + 1).min(plan.limit.len());
    let top = plan.top_speed;
    let tightness = 1.0 - plan.limit[from..to].iter().map(|l| l / top).sum::<f32>() / (to - from) as f32;
    // Runs at top speed of at least `LONG_STRAIGHT`.
    let (mut run, mut straight) = (0usize, 0usize);
    let need = (LONG_STRAIGHT / pace::STEP) as usize;
    for &l in &plan.limit[from..to] {
        if l >= top {
            run += 1;
        } else {
            if run >= need {
                straight += run;
            }
            run = 0;
        }
    }
    if run >= need {
        straight += run;
    }
    Half { tightness, straight: straight as f32 / (to - from) as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pace::class_pace;

    /// A straight kilometre, then a hairpin loop, then a straight kilometre.
    fn course() -> Vec<[f32; 3]> {
        let mut line: Vec<[f32; 3]> = (0..100).map(|i| [i as f32 * 10.0, 0.0, 0.0]).collect();
        for i in 1..=120 {
            let a = i as f32 / 120.0 * std::f32::consts::TAU * 0.5;
            line.push([1000.0 + 40.0 * a.sin(), 40.0 - 40.0 * a.cos(), 0.0]);
        }
        let end = *line.last().unwrap();
        line.extend((1..=100).map(|i| [end[0] - i as f32 * 10.0, end[1], 0.0]));
        line
    }

    #[test]
    fn the_profile_splits_the_stage_and_reads_its_character() {
        let class = class_pace("WRC 2.0");
        let p = profile(&course(), &[], Surface::Gravel, &class);
        assert!((p.sections.iter().map(|s| s.share).sum::<f32>() - 1.0).abs() < 1e-3);
        // The first third is the straight, the middle one holds the hairpin.
        assert!(p.sections[0].halves[0].straight > 0.5 && p.sections[0].halves[0].tightness < 0.1);
        assert!(p.sections[1].halves[0].tightness > p.sections[0].halves[0].tightness);
        assert!(p.wet > p.dry);
        // The stage's own splits move the sections.
        let cut = profile(&course(), &[500.0, 1500.0], Surface::Gravel, &class);
        assert!(cut.sections[0].share < p.sections[0].share + 0.2 && cut.sections[0].share != p.sections[0].share);
    }

    #[test]
    fn the_track_follows_the_profile_and_the_surface() {
        let class = class_pace("WRC 2.0");
        let p = profile(&course(), &[], Surface::Snow, &class);
        let t = p.track(7, 1.0);
        assert_eq!((t.stage, t.wet), (7, 0));
        assert!((t.sections.iter().map(|s| s.par).sum::<f32>() - p.dry).abs() < 0.01);
        assert!(t.sections.iter().all(|s| s.material == 2 && s.accident_table == 0));
        // The straight is a straights check, the hairpin curves; every value is in range.
        assert_eq!(t.sections[0].checks[0].0, 3);
        assert!(t.sections.iter().flat_map(|s| s.checks).all(|(kind, v)| kind <= 3 && (1..=5).contains(&v)));
        assert_eq!(p.track(7, 0.5).sections[0].par * 2.0, t.sections[0].par);
    }

    #[test]
    fn a_stage_without_a_driveline_gets_a_flat_profile() {
        let class = class_pace("Rally 2");
        let p = profile(&[], &[], Surface::Tarmac, &class);
        assert!(p.dry >= 0.0);
        let flat = StageProfile::flat(9000.0, Surface::Gravel, &class);
        assert!(flat.dry > 150.0 && flat.dry < 400.0, "{}", flat.dry);
        assert_eq!(flat.reference(1.0), flat.wet);
        assert!((flat.sections.iter().map(|s| s.share).sum::<f32>() - 1.0).abs() < 1e-5);
        let t = flat.track(0, 1.0);
        assert!(t.sections.iter().all(|s| s.checks.iter().all(|c| c.0 == 0)));
    }
}
