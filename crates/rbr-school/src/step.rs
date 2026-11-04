//! The bank's listeners (`RBR_E19_School.cpp`): the control tick, the incident classifier, the brake-wall
//! checkpoint, the end-race scan, the sound-pair trigger and the tracker.
//!
//! They update the bank and return the signals they emit, in order (`CSigSoundRallySchool`, `CSigSchoolIncident`,
//! `CSigEndRace`). Each runs only in flow mode 7 (`E60Flow::M566240`, 0x566240), which is the crate's; the flow
//! words they read are inputs: the step counter (gameflow +0xca8, `M00565d20`), the lesson (+0xca4), +0xcb4, the
//! application state (`*(0x7eac48) + 0x728`) and the HUD time (`*(0x165fc68) + 0x140`).

// The comparisons follow the asm's, so a NaN takes the original's branch: `!(a > b)` is not `a <= b`.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::bank::{Bank, Lead};
use super::filter::max_abs;

/// The 0.04 input gate `SchoolTick` runs the enlisted subs on.
const INPUT_GATE: f32 = 0.04;
/// `Checkpoint`'s rearm of the end-race timer.
const CHECKPOINT_REARM: f32 = 10.0;
/// `SchoolIncident`'s standing speed (km/h) and drive-line deviation (m) for the stop subs.
const STANDING: f32 = 1.0;
const DEVIATION_LIMIT: f32 = 2.0;

/// One signal emit a step function makes, in emit order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchoolEvent {
    /// `CSigSchoolIncident` (4 bytes): the incident code.
    Incident { code: i32 },
    /// `CSigSoundRallySchool` (16 bytes): the four payload words.
    Sound { a: i32, b: i32, c: i32, d: i32 },
    /// `CSigEndRace` (20 bytes): word 0 is the HUD time at `*(0x165fc68) + 0x140`; the original leaves the other
    /// words as stack residue, which the port does not model.
    EndRace { time: u32 },
}

/// `CReplayManager::SchoolTick` (0x498bb0): runs the enlisted subs whose selected input exceeds 0.04. The sub reads
/// `v[1]` for modes 1/6, `v[0]` for 2/7 and `v[2]` for 3/8; modes 2/7 skip on `v[0] <= 0.04` exactly like the others
/// (`comiss`+`jbe` in all three arms). The `Update` argument is the bank's `m_00` word as a float.
pub fn school_tick(bank: &mut Bank, v: [f32; 3], substate: u32) {
    if substate == 2 || substate == 3 {
        return;
    }
    let arg = f32::from_bits(bank.origin as u32);
    for slot in bank.slots {
        let Some(i) = slot else { continue };
        let Some(sub) = bank.subs.get_mut(i) else { continue };
        let input = match sub.tag {
            1 | 6 => v[1],
            2 | 7 => v[0],
            3 | 8 => v[2],
            _ => continue,
        };
        if input > INPUT_GATE {
            sub.update(arg);
        }
    }
}

/// Inputs `SchoolIncident` reads outside the bank.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IncidentInput {
    /// The car-state sample at `v + 0x30`: speed is its x87 length times 3.6 (FUN_00473b30).
    pub speed_vec: [f32; 3],
    /// The car position at `v + 0x10` (`v[4..6]`), for the driveline deviation.
    pub car_pos: [f32; 3],
    /// `*(0x7eac48) + 0x728`.
    pub app_state: u32,
    /// `M00565d20()` (gameflow +0xca8).
    pub substate: u32,
    /// `CDriveLine::Sample(bank.m_00)` (drive line 0x1659184), which the exe takes only in lessons 0 and 10
    /// (gameflow +0xca4, [`crate::Lesson::stands_by_the_line`]).
    pub driveline: Option<[f32; 3]>,
    /// `E19ClsGate()` on the host at 0x893634.
    pub cls_gate: bool,
}

/// `CReplayManager::SchoolIncident` (0x498e10): the speed-band classifier plus the enlisted mode-4/5 sub loop (four
/// slots, not three as the reconstruction has it).
pub fn school_incident(bank: &mut Bank, input: &IncidentInput) -> Vec<SchoolEvent> {
    let mut events = Vec::new();
    if (input.app_state != 1 && input.app_state != 8) || input.substate == 2 || input.substate == 3 {
        return events;
    }
    let [x, y, z] = input.speed_vec;
    let f = ((x * x + y * y) + z * z).sqrt() * 3.6;
    bank.speed = f;
    if let Some(lead) = bank.lead_mut() {
        lead.trip(f);
        let (reference, flag, hi, lo) = lead.words();
        let code = if flag != 0 {
            None
        } else if hi != 0.0 && lo != 0.0 {
            // An armed lead (`IsArmed`): the speed band.
            if f > hi + 10.0 {
                Some(5)
            } else if f > hi - 5.0 {
                Some(4)
            } else if lo - 10.0 > f {
                Some(2)
            } else if lo + 5.0 > f {
                Some(3)
            } else {
                None
            }
        } else {
            // The speed against `m_00`, ordered compares only (`jbe` continues unordered): code 5 is shadowed by 4.
            let d = f - reference;
            if -50.0 > d {
                Some(2)
            } else if -25.0 > d {
                Some(3)
            } else if d > 25.0 {
                Some(4)
            } else if d > 50.0 {
                Some(5)
            } else {
                None
            }
        };
        if let Some(code) = code {
            events.push(SchoolEvent::Incident { code });
            lead.set_flag();
        }
    }
    let arg = f32::from_bits(bank.origin as u32);
    for slot in bank.slots {
        let Some(i) = slot else { continue };
        let mode = bank.subs.get(i).map(|sub| sub.tag - 4);
        if mode == Some(0) {
            let deviation = input.driveline.map_or(0.0, |sample| {
                max_abs([sample[0] - input.car_pos[0], sample[1] - input.car_pos[1], sample[2] - input.car_pos[2]])
            });
            if (STANDING >= f && deviation < DEVIATION_LIMIT) || (input.substate == 6 && input.cls_gate) {
                if let Some(sub) = bank.subs.get_mut(i) {
                    sub.update(arg);
                }
            }
        } else if mode == Some(1) && STANDING >= f {
            if let Some(sub) = bank.subs.get_mut(i) {
                sub.update(arg);
            }
        }
    }
    events
}

/// `CReplayManager::Checkpoint` (0x4991c0): latches `m_f44` on the stop cone (flag bit 1), else emits incident code
/// 1 and rearms the timer to 10 while it is not positive (the reconstruction has the sense backwards). `flags` is
/// the byte at `p + 0x18`.
pub fn checkpoint(bank: &mut Bank, flags: u8, app_state: u32) -> Vec<SchoolEvent> {
    if flags & 4 == 0 || (app_state != 1 && app_state != 8) {
        return Vec::new();
    }
    if flags & 2 != 0 {
        bank.latched_a = true;
        return Vec::new();
    }
    if !(bank.timer > 0.0) {
        bank.timer = CHECKPOINT_REARM;
        return vec![SchoolEvent::Incident { code: 1 }];
    }
    Vec::new()
}

/// `CReplayManager::EndRace` (0x499110): decays the timer by `delta`, then emits `CSigEndRace` with the HUD time for
/// every mode-4 sub whose flag/value select matches (flag bit 2 set pairs with nonzero, clear with zero).
pub fn end_race(bank: &mut Bank, delta: f32, host_time: u32) -> Vec<SchoolEvent> {
    bank.timer -= delta;
    let mut events = Vec::new();
    for sub in &bank.subs {
        if sub.tag != 4 {
            continue;
        }
        if (sub.flags & 4) != 0 {
            if sub.value != 0 {
                continue;
            }
        } else if sub.value == 0 {
            continue;
        }
        events.push(SchoolEvent::EndRace { time: host_time });
    }
    events
}

/// `CReplayManager::Trigger` (0x4992a0): the threshold/code-pair scan. The ack mask is a fresh `1 << (i & 31)` per
/// index (the reconstruction's `bit <<= i` accumulates); the scan aborts whole on the first unmet threshold
/// (`v < thr`, taken unordered). `cb4` is gameflow +0xcb4.
pub fn trigger(bank: &mut Bank, v: f32, cb4: i32, substate: u32) -> Vec<SchoolEvent> {
    if cb4 != 0 && substate == 4 {
        bank.sounded = true;
    }
    if bank.live || (!bank.sounded && substate != 6) {
        return Vec::new();
    }
    let mut events = Vec::new();
    for (i, pair) in bank.pairs.iter().enumerate() {
        let bit = 1u32 << (i as u32 & 31);
        if bank.ack & bit != 0 {
            continue;
        }
        if !(v >= pair.threshold) {
            return events;
        }
        events.push(SchoolEvent::Sound { a: 2, b: i as i32, c: 0, d: pair.code });
        bank.ack |= bit;
    }
    events
}

/// `CReplayManager::Tracker` (0x499360): enlists up to four window-hitting subs, picks the matching check entry,
/// then runs `Trigger` on the bank's `m_00` word as a float. `p_bits` is the `int*` argument's word (stored to
/// `m_00` and read back as the float). In substates 2/3 it only runs `Trigger` on the argument.
pub fn tracker(bank: &mut Bank, p_bits: u32, substate: u32, cb4: i32) -> Vec<SchoolEvent> {
    let f = f32::from_bits(p_bits);
    if substate == 2 || substate == 3 {
        return trigger(bank, f, cb4, substate);
    }
    bank.origin = p_bits as i32;
    bank.slots = [None; 4];
    let mut found = 0;
    for (i, sub) in bank.subs.iter().enumerate() {
        if found >= 4 {
            break;
        }
        if sub.window_hit(f) {
            bank.slots[found] = Some(i);
            found += 1;
        }
    }
    bank.lead = None;
    for (i, check) in bank.checks.iter().enumerate() {
        let d = f - check.pos;
        if d >= 0.0 && check.span >= d {
            bank.lead = Some(Lead::Check(i));
            break;
        }
    }
    let v = f32::from_bits(bank.origin as u32);
    trigger(bank, v, cb4, substate)
}

#[cfg(test)]
mod tests {
    use super::super::bank::{Bank, Check, Pair};
    use super::super::filter::FilterSub;
    use super::*;

    fn sub(tag: i32, center: f32, hi: f32) -> FilterSub {
        let mut s = FilterSub::init_sub();
        s.tag = tag;
        s.center = center;
        s.hi = hi;
        s
    }

    #[test]
    fn tick_runs_the_selected_inputs_past_the_gate() {
        let mut bank = Bank::fresh();
        bank.subs.push(sub(1, 0.0, 100.0));
        bank.subs.push(sub(2, 0.0, 100.0));
        bank.subs.push(sub(9, 0.0, 100.0));
        bank.slots = [Some(0), Some(1), Some(2), None];
        school_tick(&mut bank, [0.0, 0.5, 0.0], 0);
        assert_eq!(bank.subs[0].value, 4);
        assert_eq!(bank.subs[1].value, 0, "v[0] is 0, under the gate");
        school_tick(&mut bank, [0.5, 0.0, 0.0], 0);
        assert_eq!(bank.subs[1].value, 4);
        school_tick(&mut bank, [0.5, 0.5, 0.5], 2);
        assert_eq!(bank.subs[2].value, 0, "mode 9 is never selected");
    }

    #[test]
    fn checkpoint_latches_arms_and_rearms() {
        let mut bank = Bank::fresh();
        assert!(checkpoint(&mut bank, 0, 1).is_empty());
        assert!(checkpoint(&mut bank, 4, 2).is_empty());
        assert!(checkpoint(&mut bank, 6, 1).is_empty());
        assert!(bank.latched_a);
        let events = checkpoint(&mut bank, 4, 8);
        assert_eq!(events, vec![SchoolEvent::Incident { code: 1 }]);
        assert_eq!(bank.timer, 10.0);
        // Positive timer: no emit (the reconstruction emits here).
        assert!(checkpoint(&mut bank, 4, 1).is_empty());
        bank.timer = -1.0;
        assert_eq!(checkpoint(&mut bank, 4, 1), vec![SchoolEvent::Incident { code: 1 }]);
    }

    #[test]
    fn end_race_selects_mode_4_by_flag_and_value() {
        let mut bank = Bank::fresh();
        let mut a = sub(4, 0.0, 0.0);
        a.value = 0;
        a.flags = 4;
        let mut b = sub(4, 0.0, 0.0);
        b.value = 0;
        let mut c = sub(4, 0.0, 0.0);
        c.value = 3;
        bank.subs = vec![a, b, c, sub(5, 0.0, 0.0)];
        let events = end_race(&mut bank, 0.5, 77);
        assert_eq!(events, vec![SchoolEvent::EndRace { time: 77 }, SchoolEvent::EndRace { time: 77 }]);
        assert_eq!(bank.timer, -0.5);
    }

    #[test]
    fn trigger_scans_pairs_in_order_with_fresh_masks() {
        let mut bank = Bank::fresh();
        bank.live = false;
        bank.sounded = true;
        bank.pairs.push(Pair { threshold: 3.0, code: 65 });
        bank.pairs.push(Pair { threshold: 100.0, code: 73 });
        assert!(trigger(&mut bank, 2.0, 0, 6).is_empty());
        let events = trigger(&mut bank, 50.0, 0, 6);
        assert_eq!(events, vec![SchoolEvent::Sound { a: 2, b: 0, c: 0, d: 65 }]);
        assert_eq!(bank.ack, 1);
        // Whole-scan abort on the first unmet threshold keeps the earlier emit out of this call.
        let events = trigger(&mut bank, 50.0, 0, 6);
        assert!(events.is_empty());
        assert_eq!(bank.ack, 1);
        let events = trigger(&mut bank, 150.0, 0, 6);
        assert_eq!(events, vec![SchoolEvent::Sound { a: 2, b: 1, c: 0, d: 73 }]);
        assert_eq!(bank.ack, 0b11);
    }

    #[test]
    fn tracker_enlists_hits_and_picks_the_check() {
        let mut bank = Bank::fresh();
        bank.live = false;
        bank.sounded = true;
        let mut s = sub(3, 44.0, 13.6);
        s.lo = 20.4;
        bank.subs.push(s);
        bank.checks.push(Check { pos: 100.0, span: 5.0, ..Check::default() });
        bank.pairs.push(Pair { threshold: 1.0, code: 9 });
        let events = tracker(&mut bank, 44.0f32.to_bits(), 0, 0);
        assert_eq!(bank.slots[0], Some(0));
        assert_eq!(bank.lead, None, "44 is outside the check window");
        assert_eq!(events.len(), 1);
        // The pair already acked: the second scan picks the check but emits nothing new.
        let events = tracker(&mut bank, 102.0f32.to_bits(), 0, 0);
        assert_eq!(bank.lead, Some(Lead::Check(0)));
        assert!(events.is_empty());
        bank.ack = 0;
        let events = tracker(&mut bank, 102.0f32.to_bits(), 0, 0);
        assert_eq!(events.len(), 1);
    }
}
