//! The lesson's verdict (`CReplayManager::EvalFilters` 0x4982e0, `SeverityMax` 0x498240).
//!
//! `EvalFilters` folds the sub judge values, the three-word time ladder, the check trip states and the damage
//! severity into one 0-4 verdict. It returns 0 outright once either trip latch is set (the reconstruction returns 4
//! there). The time ladder is strict `<` with no equality side-condition (the reconstruction adds one).

use super::bank::Bank;

/// `CReplayManager::SeverityMax` (0x498240): the damage-severity aggregate over the 256 repair parts
/// (`CCarRepairHandler::GetDamage` 0x4f2b40, `E39` owns the workers). `lesson` is gameflow +0xca4; `damage` holds the
/// 256 part severities, compared unsigned. Lessons 4 and 0x10 force 4.
pub fn severity_max(lesson: i32, damage: &[u32; 256]) -> u32 {
    if lesson == 4 || lesson == 0x10 {
        return 4;
    }
    let mut top: u32 = 0;
    for &s in damage {
        if top < s {
            top = s;
        }
    }
    match top {
        0 | 1 => 4,
        2 => 3,
        3..=5 => 2,
        _ => top,
    }
}

/// `CReplayManager::EvalFilters` (0x4982e0): the 0-4 verdict over `v` (the HUD time at the call sites). The sub loop
/// compares signed ints; the check loop zeroes the verdict on any `+-1` trip state without stopping.
pub fn eval_filters(bank: &Bank, v: f32, lesson: i32, damage: &[u32; 256]) -> u32 {
    if bank.latched_a || bank.latched_b {
        return 0;
    }
    let mut worst: i32 = 4;
    for sub in &bank.subs {
        let raw = sub.value;
        let sv = if sub.flags & 4 != 0 { (i32::from(raw != 0).wrapping_sub(1)) & 4 } else { raw };
        if sv < worst {
            worst = sv;
        }
    }
    if bank.time_limit != 0.0 && bank.pass_time != 0.0 && bank.perfect_time != 0.0 {
        // The first two rungs share the outcome but not the compare; `||` keeps the NaN fall-through.
        let s: i32 = if bank.time_limit < v || bank.pass_time < v {
            0
        } else if bank.perfect_time < v {
            3
        } else {
            4
        };
        if s < worst {
            worst = s;
        }
    }
    for check in &bank.checks {
        if check.state == 1 || check.state == -1 {
            worst = 0;
        }
    }
    let sev = severity_max(lesson, damage);
    if (sev as i32) < worst {
        worst = sev as i32;
    }
    worst as u32
}

#[cfg(test)]
mod tests {
    use super::super::bank::Check;
    use super::super::filter::FilterSub;
    use super::*;

    fn quiet() -> [u32; 256] {
        [0; 256]
    }

    #[test]
    fn latches_force_zero_not_four() {
        let mut bank = Bank::fresh();
        assert_eq!(eval_filters(&bank, 5.0, 2, &quiet()), 4);
        bank.latched_a = true;
        assert_eq!(eval_filters(&bank, 5.0, 2, &quiet()), 0);
    }

    #[test]
    fn ladder_is_strict() {
        let mut bank = Bank::fresh();
        bank.time_limit = 11.0;
        bank.pass_time = 10.0;
        bank.perfect_time = 9.0;
        assert_eq!(eval_filters(&bank, 12.0, 2, &quiet()), 0);
        assert_eq!(eval_filters(&bank, 11.0, 2, &quiet()), 0, "equal is not less");
        assert_eq!(eval_filters(&bank, 9.5, 2, &quiet()), 3);
        assert_eq!(eval_filters(&bank, 8.5, 2, &quiet()), 4);
        assert_eq!(eval_filters(&bank, f32::NAN, 2, &quiet()), 4);
    }

    #[test]
    fn checks_and_damage_fold_in() {
        let mut bank = Bank::fresh();
        let mut sub = FilterSub::init_sub();
        sub.value = 2;
        bank.subs.push(sub);
        assert_eq!(eval_filters(&bank, 0.0, 2, &quiet()), 2);
        bank.checks.push(Check { state: -1, ..Check::default() });
        assert_eq!(eval_filters(&bank, 0.0, 2, &quiet()), 0);
    }

    #[test]
    fn severity_maps_the_top_damage() {
        let mut damage = [0u32; 256];
        assert_eq!(severity_max(2, &damage), 4);
        damage[7] = 2;
        assert_eq!(severity_max(2, &damage), 3);
        damage[7] = 5;
        assert_eq!(severity_max(2, &damage), 2);
        damage[7] = 9;
        assert_eq!(severity_max(2, &damage), 9);
        assert_eq!(severity_max(4, &damage), 4);
        assert_eq!(severity_max(0x10, &damage), 4);
    }
}
