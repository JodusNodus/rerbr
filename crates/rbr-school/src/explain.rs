//! Why a failed lesson failed: the verdict panel's reasons (`CFailedSchoolExplanationUI::Vf0C` 0x542330).
//!
//! The page lists one line per failed judge entry, in the bank's order, de-duplicated, at most 7: per filter sub
//! the limit word `FlagMap` (0x4980a0) selects (`Get20`/`Get24`/`Get28` 0x497e40/50/60), per check its code, and the
//! time ladder's `TIMEFAIL`/`TIMEBAD` last. A non-negative limit is the lesson's own string-table key (read
//! NUL-terminated from the `.dls` table-7 string block at the offset, `BlockAddB` 0x475c90 with block base 0 in
//! ours); a more negative one names a built-in key by the sub's tag. `q == -1` gives no reason.
//!
//! Verified against the 1.02 disassembly, where the reconstruction (`RBR_E52_X87.cpp`, 26% matched) diverges:
//! only `FlagMap` 0, 1 and 2 name a limit (`dec; jne skip`: 3 and up give nothing), the built-in key comes from
//! the sub's tag (`mov edi,[edi]`: the reconstruction switches on the negative limit, whose cases never hit),
//! and the first table is `m_str8[9..16]` (`ebx+0x74..0xc8`), not `[6..13]`. `IsZeroAny` (0x4981a0) gates the time
//! pair on all three ladder times being ordered-nonzero; each compare is strict `>`, unordered taking the skip
//! arm. `FAILHELP` and `OOBFAIL`/`OOBYAFAIL`/`OOBSTFAIL`/`OOB_TTFAIL`/`CRASHFAIL` (`m_str8[1]`, `[4..8]`) are set in
//! the ctor (0x542110) but never resolved here (the out-of-bounds ones feed the fade's warning instead,
//! `RBR_E70_Fade.cpp`); the title is `FAILURE1` and the advice a random `SCOM_FAIL1-4`, neither picked from the
//! result.
//!
//! Nothing here reads game files: the caller keeps the string block with the run.

use super::bank::Bank;

/// One failure reason: a locale key, resolved through the English string table at the viewer's end (an unknown
/// key, like the absent `HALTEARLY`, is skipped there, as the original's null `Resolve` skips it here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    /// The key: a built-in (`BRKFAIL`, `TIMEFAIL`, ...) or the lesson's own (from the string block).
    pub key: String,
    /// The key is the lesson's own, read from the `.dls` string block.
    pub lesson: bool,
}

impl Reason {
    fn built_in(key: &'static str) -> Self {
        Reason { key: key.to_owned(), lesson: false }
    }

    fn lesson(key: String) -> Self {
        Reason { key, lesson: true }
    }
}

/// The built-ins a `FlagMap` 0 names by tag 1..8 (`m_str8[9..16]`: `BRKFAIL`..`NOHBFAIL`).
const SUB_LIMIT0: [&str; 8] =
    ["BRKFAIL", "THRFAIL", "HBFAIL", "STOPFAIL", "HALTFAIL", "NOBRKFAIL", "NOTHRFAIL", "NOHBFAIL"];
/// The built-ins a `FlagMap` 1 names by tag 1..5 (`m_str8[17..21]`).
const SUB_LIMIT1: [&str; 5] = ["BRKEARLY", "THREARLY", "HBEARLY", "STOPEARLY", "HALTEARLY"];
/// The built-ins a `FlagMap` 2 names by tag 1..5 (`m_str8[22..26]`).
const SUB_LIMIT2: [&str; 5] = ["BRKLATE", "THRLATE", "HBLATE", "STOPLATE", "HALTLATE"];
/// A failed lesson lists at most 7 reasons.
const MAX_REASONS: usize = 7;

/// `CFailedSchoolExplanationUI::Vf0C` (0x542330): the failed lesson's reasons over `bank` at race time `time`,
/// in the original's order: each sub's, each tripped check's, then the time ladder's. `strings` is the lesson
/// `.dls`'s table-7 string block the limit offsets read through.
pub fn failure_reasons(bank: &Bank, time: f32, strings: &[u8]) -> Vec<Reason> {
    let mut out: Vec<Reason> = Vec::new();
    for sub in &bank.subs {
        let (lim, table): (i32, &[&str]) = match sub.flag_map() {
            0 => (sub.get20(), &SUB_LIMIT0),
            1 => (sub.get24(), &SUB_LIMIT1),
            2 => (sub.get28(), &SUB_LIMIT2),
            _ => continue,
        };
        if lim >= 0 {
            if let Some(key) = lesson_key(strings, lim) {
                push(&mut out, Reason::lesson(key));
            }
        } else if lim != -1 {
            // The switch reads the sub's tag (`m_00`), 1-based into the table.
            if let Some(key) = sub.tag.checked_sub(1).and_then(|t| table.get(t as usize)) {
                push(&mut out, Reason::built_in(key));
            }
        }
    }
    for check in &bank.checks {
        if (check.state == 1 || check.state == -1) && check.code > 0 {
            if let Some(key) = lesson_key(strings, check.code) {
                push(&mut out, Reason::lesson(key));
            }
        }
    }
    // The tail: a timed lesson names being too slow, over its limit or only over the pass time (an unordered
    // time names neither).
    let timed = bank.time_limit != 0.0 && bank.pass_time != 0.0 && bank.perfect_time != 0.0;
    if timed && time > bank.time_limit {
        push(&mut out, Reason::built_in("TIMEFAIL"));
    } else if timed && time > bank.pass_time {
        push(&mut out, Reason::built_in("TIMEBAD"));
    }
    out
}

/// Appends `reason` unless the list is full or holds its key: the page dedups by resolved pointer and caps
/// at 7; equal keys resolve to one table entry, so key equality matches.
fn push(out: &mut Vec<Reason>, reason: Reason) {
    if out.len() < MAX_REASONS && !out.contains(&reason) {
        out.push(reason);
    }
}

/// The NUL-terminated key at `offset` in the string block, if there is a non-empty one.
fn lesson_key(strings: &[u8], offset: i32) -> Option<String> {
    let offset = u32::try_from(offset).ok()? as usize;
    let tail = strings.get(offset..)?;
    let end = tail.iter().position(|&b| b == 0).map_or(tail.len(), |n| n);
    (end != 0).then(|| String::from_utf8_lossy(&tail[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::super::filter::FilterSub;
    use super::*;

    fn sub(tag: i32, value: i32, flags: i32, lim0: i32, lim1: i32, lim2: i32) -> FilterSub {
        FilterSub { tag, value, flags, lim0, lim1, lim2, ..FilterSub::init_sub() }
    }

    fn bank_with(subs: Vec<FilterSub>) -> Bank {
        Bank { subs, ..Bank::fresh() }
    }

    #[test]
    fn non_negative_limits_name_the_lesson_strings() {
        let strings = b"RS_A\0RS_B\0";
        let bank = bank_with(vec![sub(6, 0, 0, 0, -1, -1), sub(6, 1, 0, -1, 5, -1), sub(6, 2, 0, -1, -1, 5)]);
        assert_eq!(
            failure_reasons(&bank, 0.0, strings),
            vec![Reason::lesson("RS_A".into()), Reason::lesson("RS_B".into())],
            "the third repeats RS_B and dedups away"
        );
    }

    #[test]
    fn negative_limits_name_built_ins_by_tag() {
        let bank = bank_with(vec![
            sub(1, 0, 0, -2, -1, -1),
            sub(2, 1, 0, -1, -2, -1),
            sub(5, 2, 0, -1, -1, -2),
            sub(3, 0, 0, -1, -1, -1),
            sub(0, 0, 0, -2, -1, -1),
            sub(9, 0, 0, -2, -1, -1),
        ]);
        assert_eq!(
            failure_reasons(&bank, 0.0, &[]),
            ["BRKFAIL", "THREARLY", "HALTLATE"].map(Reason::built_in),
            "-1, tag 0 and tag 9 give nothing"
        );
    }

    #[test]
    fn flag_map_above_two_gives_nothing() {
        // `flags & 4` with a zero value maps to 4 (the reconstruction's default arm would read `lim2`).
        assert_eq!(sub(6, 0, 4, 0, -1, -1).flag_map(), 4);
        assert_eq!(sub(6, 2, 4, 0, -1, -1).flag_map(), 0, "value 2 with bit 2 maps to lim0");
        assert_eq!(sub(6, 3, 0, 0, -1, -1).flag_map(), 3);
        let bank = bank_with(vec![sub(6, 0, 4, 0, -1, -1), sub(6, 3, 0, -1, -1, 7)]);
        assert!(failure_reasons(&bank, 0.0, b"RS_A\0").is_empty());
    }

    #[test]
    fn bit_two_set_with_a_value_names_lim0() {
        // `(value != 0) - 1 & 4` is 0 for value 1, so `lim0` applies.
        let bank = bank_with(vec![sub(1, 1, 4, -2, -1, -1)]);
        assert_eq!(failure_reasons(&bank, 0.0, &[]), [Reason::built_in("BRKFAIL")]);
    }

    #[test]
    fn tripped_checks_name_their_codes() {
        let block = b"\0RS_SPDCHK_F\0";
        let mut bank = Bank::fresh();
        bank.checks = vec![
            super::super::Check { state: 1, code: 1, ..Default::default() },
            super::super::Check { state: -1, code: 0, ..Default::default() },
            super::super::Check { state: 0, code: 1, ..Default::default() },
        ];
        assert_eq!(failure_reasons(&bank, 0.0, block), [Reason::lesson("RS_SPDCHK_F".into())]);
    }

    #[test]
    fn the_time_ladder_comes_last() {
        let mut bank = Bank::fresh();
        (bank.time_limit, bank.pass_time, bank.perfect_time) = (11.0, 10.0, 9.0);
        assert_eq!(failure_reasons(&bank, 12.0, &[]), [Reason::built_in("TIMEFAIL")]);
        assert_eq!(failure_reasons(&bank, 10.5, &[]), [Reason::built_in("TIMEBAD")]);
        assert!(failure_reasons(&bank, 9.5, &[]).is_empty());
        assert!(failure_reasons(&bank, f32::NAN, &[]).is_empty(), "unordered takes the skip arm");
        assert!(failure_reasons(&Bank::fresh(), 99.0, &[]).is_empty(), "untimed lessons skip the tail");
    }

    /// Every lesson's limit words resolve: each non-negative sub limit and each positive check code names a
    /// non-empty key in the lesson's string block, and every built-in reason key but `HALTEARLY` is in the
    /// English table (`RBR_DATA`; skipped without it).
    #[test]
    fn retail_lessons_reasons_resolve() {
        let Some(data) = rbr_formats::data::Data::from_env() else { return };
        let strings = rbr_formats::strings::english(&data).expect("bstrings.eng");
        for key in
            SUB_LIMIT0.iter().chain(SUB_LIMIT1.iter()).chain(SUB_LIMIT2.iter()).copied().chain(["TIMEFAIL", "TIMEBAD"])
        {
            if key == "HALTEARLY" {
                assert!(strings.get(key).is_none(), "HALTEARLY is absent in 1.02 English (skipped at runtime)");
            } else {
                assert!(strings.get(key).is_some(), "{key} is no English string");
            }
        }
        for lesson in super::super::Lesson::all() {
            let path = rbr_formats::stage_files::file(&data, &format!("track-{}_M", lesson.map()), "dls")
                .expect("lesson .dls");
            let bytes = std::fs::read(&path).expect("lesson .dls");
            let file = rbr_formats::dls::parse(&bytes).expect("lesson .dls");
            let replay = rbr_formats::markers::driveline_entry(&file).expect("Driveline set").replay.clone();
            let bank = super::super::Bank::from_stream(&super::super::Bank::fresh(), &replay, 0);
            for (i, sub) in bank.subs.iter().enumerate() {
                for (name, lim) in [("lim0", sub.lim0), ("lim1", sub.lim1), ("lim2", sub.lim2)] {
                    if lim >= 0 {
                        assert!(
                            lesson_key(&file.string_bytes, lim).is_some(),
                            "lesson {} sub {i} {name}={lim} resolves to nothing",
                            lesson.index()
                        );
                    }
                }
            }
            for (i, check) in bank.checks.iter().enumerate() {
                if check.code > 0 {
                    assert!(
                        lesson_key(&file.string_bytes, check.code).is_some(),
                        "lesson {} check {i} code={} resolves to nothing",
                        lesson.index(),
                        check.code
                    );
                }
            }
        }
    }

    #[test]
    fn at_most_seven_in_sub_check_time_order() {
        let strings = b"K0\0K1\0K2\0K3\0K4\0K5\0K6\0K7\0";
        let subs: Vec<FilterSub> = (0..6).map(|i| sub(6, 0, 0, 3 * i, -1, -1)).collect();
        let mut bank = bank_with(subs);
        bank.time_limit = 11.0;
        bank.pass_time = 10.0;
        bank.perfect_time = 9.0;
        bank.checks = vec![super::super::Check { state: 1, code: 21, ..Default::default() }];
        let reasons = failure_reasons(&bank, 12.0, strings);
        assert_eq!(reasons.len(), 7);
        assert_eq!(
            reasons[..6].iter().map(|r| r.key.clone()).collect::<Vec<_>>(),
            ["K0", "K1", "K2", "K3", "K4", "K5"]
        );
        assert_eq!(reasons[6].key, "K7", "the check's key fills the last slot, TIMEFAIL is capped out");
    }
}
