//! The rally-school in-step filter runners (`RBR_E19_School.cpp`): tick, incident classifier, checkpoint gate,
//! end-race scan, sound trigger, threshold/code-pair scan and tracker.
//!
//! None of these writes car or physics state; they update the filter bank, read the gameflow, application and HUD
//! globals below, and return the signal emits they make in emit order. The three signal buses live on the signal
//! manager (0x893600): `CSigSoundRallySchool` at +0x8a30, `CSigSchoolIncident` at +0xa998, `CSigEndRace` at +0x7970.
//!
//! Globals read, as explicit inputs: `E60Flow::M566240()` (0x566240, `*(int*)gameflow == 7`) on gameflow 0x165fa90,
//! the substate `M00565d20()` (0x565d20, gameflow +0xca8), the lesson index (gameflow +0xca4), the select latch
//! (gameflow +0xcb4), the application state at `*(0x7eac48) + 0x728`, the HUD time at `*(0x165fc68) + 0x140`, the
//! drive line at 0x1659184 (`CDriveLine::Sample` 0x50b180) and the class gate host at 0x893634
//! (`E19ClsGate` 0x49a0f0).

// The comparisons follow the asm's, so a NaN takes the original's branch: `!(a > b)` is not `a <= b`.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::bank::{Bank, Lead};
use super::filter::max_abs;

/// The 0.04 input gate `SchoolTick` runs the enlisted subs on.
pub const INPUT_GATE: f32 = 0.04;
/// `Checkpoint`'s rearm of the end-race timer.
pub const CHECKPOINT_REARM: f32 = 10.0;
/// `SchoolIncident`'s speed bands and stop thresholds.
pub const SPEED_STOP: f32 = 1.0;
pub const DEVIATION_LIMIT: f32 = 2.0;

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
pub fn school_tick(bank: &mut Bank, v: [f32; 3], is_school: bool, substate: u32) {
    if !is_school || substate == 2 || substate == 3 {
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
    /// `E60Flow::M566240()` (0x566240).
    pub is_school: bool,
    /// `*(0x7eac48) + 0x728`.
    pub app_state: u32,
    /// `M00565d20()` (gameflow +0xca8).
    pub substate: u32,
    /// Gameflow +0xca4: the driveline is sampled only for lessons 0 and 10.
    pub lesson: u32,
    /// `CDriveLine::Sample(bank.m_00)` (drive line 0x1659184) when `lesson` is 0 or 10.
    pub driveline: Option<[f32; 3]>,
    /// `E19ClsGate()` on the host at 0x893634.
    pub cls_gate: bool,
}

/// `CReplayManager::SchoolIncident` (0x498e10): the speed-band classifier plus the enlisted mode-4/5 sub loop (four
/// slots, not three as the reconstruction has it).
pub fn school_incident(bank: &mut Bank, input: &IncidentInput) -> Vec<SchoolEvent> {
    let mut events = Vec::new();
    if !input.is_school || (input.app_state != 1 && input.app_state != 8) {
        return events;
    }
    if input.substate == 2 || input.substate == 3 {
        return events;
    }
    let [x, y, z] = input.speed_vec;
    let f = ((x * x + y * y) + z * z).sqrt() * 3.6;
    bank.speed = f;
    if bank.lead.is_some() {
        bank.lead_trip(f);
        let tripped = match bank.lead {
            Some(Lead::Sub(i)) => bank.subs.get(i).is_some_and(|sub| sub.flags != 0),
            Some(Lead::Check(i)) => bank.checks.get(i).is_some_and(|check| check.lane != 0),
            None => true,
        };
        if !tripped {
            let armed = match bank.lead {
                Some(Lead::Sub(i)) => bank.subs.get(i).is_some_and(|sub| sub.is_armed() != 0),
                Some(Lead::Check(_)) => true,
                None => true,
            };
            if armed {
                let (_, _, hi, lo) = bank.lead_vals().unwrap_or((0.0, 0, 0.0, 0.0));
                let code = if f > hi + 10.0 {
                    Some(5)
                } else if f > hi - 5.0 {
                    Some(4)
                } else if lo - 10.0 > f {
                    Some(2)
                } else if lo + 5.0 > f {
                    Some(3)
                } else {
                    None
                };
                if let Some(code) = code {
                    events.push(SchoolEvent::Incident { code });
                    bank.lead_set_flag();
                }
            } else if let Some((tag_f, _, _, _)) = bank.lead_vals() {
                // Ordered compares only (`jbe` continues unordered): code 5 is shadowed by code 4.
                let d = f - tag_f;
                let code = if -50.0 > d {
                    Some(2)
                } else if -25.0 > d {
                    Some(3)
                } else if d > 25.0 {
                    Some(4)
                } else if d > 50.0 {
                    Some(5)
                } else {
                    None
                };
                if let Some(code) = code {
                    events.push(SchoolEvent::Incident { code });
                    bank.lead_set_flag();
                }
            }
        }
    }
    let arg = f32::from_bits(bank.origin as u32);
    for slot in bank.slots {
        let Some(i) = slot else { continue };
        let mode = bank.subs.get(i).map(|sub| sub.tag - 4);
        if mode == Some(0) {
            let mut deviation = 0.0;
            if input.lesson == 0 || input.lesson == 10 {
                if let Some(sample) = input.driveline {
                    deviation = max_abs([
                        sample[0] - input.car_pos[0],
                        sample[1] - input.car_pos[1],
                        sample[2] - input.car_pos[2],
                    ]);
                }
            }
            if (1.0 >= f && deviation < DEVIATION_LIMIT) || (input.substate == 6 && input.cls_gate) {
                if let Some(sub) = bank.subs.get_mut(i) {
                    sub.update(arg);
                }
            }
        } else if mode == Some(1) && 1.0 >= f {
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
pub fn checkpoint(bank: &mut Bank, flags: u8, is_school: bool, app_state: u32) -> Vec<SchoolEvent> {
    if !is_school || flags & 4 == 0 || (app_state != 1 && app_state != 8) {
        return Vec::new();
    }
    if flags & 2 != 0 {
        bank.latched_a = 1;
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
pub fn end_race(bank: &mut Bank, delta: f32, host_time: u32, is_school: bool) -> Vec<SchoolEvent> {
    if !is_school {
        return Vec::new();
    }
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

/// `CReplayManager::RallySchoolSound` (0x499240): emits `{1, 0, lesson, 0}` (the reconstruction's `{2, 1, 0,
/// lesson}` is wrong) and sets the sound-pair gate. `lesson` is gameflow +0xca4. No guards.
pub fn rally_sound(bank: &mut Bank, lesson: u32) -> Vec<SchoolEvent> {
    bank.sounded = 1;
    vec![SchoolEvent::Sound { a: 1, b: 0, c: lesson as i32, d: 0 }]
}

/// `CReplayManager::Trigger` (0x4992a0): the threshold/code-pair scan. The ack mask is a fresh `1 << (i & 31)` per
/// index (the reconstruction's `bit <<= i` accumulates); the scan aborts whole on the first unmet threshold
/// (`v < thr`, taken unordered). `cb4` is gameflow +0xcb4.
pub fn trigger(bank: &mut Bank, v: f32, cb4: i32, substate: u32) -> Vec<SchoolEvent> {
    if cb4 != 0 && substate == 4 {
        bank.sounded = 1;
    }
    if bank.live != 0 {
        return Vec::new();
    }
    if bank.sounded == 0 && substate != 6 {
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
pub fn tracker(bank: &mut Bank, p_bits: u32, is_school: bool, substate: u32, cb4: i32) -> Vec<SchoolEvent> {
    let f = f32::from_bits(p_bits);
    if !is_school {
        return Vec::new();
    }
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
        school_tick(&mut bank, [0.0, 0.5, 0.0], true, 0);
        assert_eq!(bank.subs[0].value, 4);
        assert_eq!(bank.subs[1].value, 0, "v[0] is 0, under the gate");
        school_tick(&mut bank, [0.5, 0.0, 0.0], true, 0);
        assert_eq!(bank.subs[1].value, 4);
        school_tick(&mut bank, [0.5, 0.5, 0.5], false, 0);
        school_tick(&mut bank, [0.5, 0.5, 0.5], true, 2);
        assert_eq!(bank.subs[2].value, 0, "mode 9 is never selected");
    }

    #[test]
    fn checkpoint_latches_arms_and_rearms() {
        let mut bank = Bank::fresh();
        assert!(checkpoint(&mut bank, 0, true, 1).is_empty());
        assert!(checkpoint(&mut bank, 4, false, 1).is_empty());
        assert!(checkpoint(&mut bank, 4, true, 2).is_empty());
        assert!(checkpoint(&mut bank, 6, true, 1).is_empty());
        assert_eq!(bank.latched_a, 1);
        let events = checkpoint(&mut bank, 4, true, 8);
        assert_eq!(events, vec![SchoolEvent::Incident { code: 1 }]);
        assert_eq!(bank.timer, 10.0);
        // Positive timer: no emit (the reconstruction emits here).
        assert!(checkpoint(&mut bank, 4, true, 1).is_empty());
        bank.timer = -1.0;
        assert_eq!(checkpoint(&mut bank, 4, true, 1), vec![SchoolEvent::Incident { code: 1 }]);
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
        let events = end_race(&mut bank, 0.5, 77, true);
        assert_eq!(events, vec![SchoolEvent::EndRace { time: 77 }, SchoolEvent::EndRace { time: 77 }]);
        assert_eq!(bank.timer, -0.5);
    }

    #[test]
    fn trigger_scans_pairs_in_order_with_fresh_masks() {
        let mut bank = Bank::fresh();
        bank.live = 0;
        bank.sounded = 1;
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
        bank.live = 0;
        bank.sounded = 1;
        let mut s = sub(3, 44.0, 13.6);
        s.lo = 20.4;
        bank.subs.push(s);
        bank.checks.push(Check { pos: 100.0, span: 5.0, ..Check::clear() });
        bank.pairs.push(Pair { threshold: 1.0, code: 9 });
        let events = tracker(&mut bank, 44.0f32.to_bits(), true, 0, 0);
        assert_eq!(bank.slots[0], Some(0));
        assert_eq!(bank.lead, None, "44 is outside the check window");
        assert_eq!(events.len(), 1);
        // The pair already acked: the second scan picks the check but emits nothing new.
        let events = tracker(&mut bank, 102.0f32.to_bits(), true, 0, 0);
        assert_eq!(bank.lead, Some(Lead::Check(0)));
        assert!(events.is_empty());
        bank.clear_ack();
        let events = tracker(&mut bank, 102.0f32.to_bits(), true, 0, 0);
        assert_eq!(events.len(), 1);
    }
}
