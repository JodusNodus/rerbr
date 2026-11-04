//! The rally-school filter bank (`E19Bank`, `RBR_E19.h:839-893`) and its construction from the lesson's `.dls`
//! replay stream (`CReplayManager::InitState` 0x4980c0, `ReadStream` 0x498840, `E12_AnimSet::Load020` 0x479020).
//!
//! The bank is not built by code: `Load020` allocates a 0x60-byte bank per animation record, runs `InitState`, then
//! `ReadStream` over the record's word-16 stream (`rbr_formats::dls::DlsReplayStream`: a 7-word header, ten-word sub
//! records, six-word check records, two-word pairs). This module is that constructor plus the trip-flag accessors
//! (`Get44`/`Get48`/`Set44`/`Set48`/`Set48_1`/`Set44_1`/`Clear40`) and the BSS-mirror snapshot
//! (`Publish` 0x498520).
//!
//! `ReadStream` stores the header's three times reversed: `m_08` (the limit) from word 6, `m_0c` (the pass time) from
//! word 5, `m_10` (the perfect time) from word 4. A check's window is `f1..f2` and its speed band `f0 -+ 20`, which
//! words 4 and 5 replace (with word 3 as its code) only when both are ordered-nonzero.

use super::filter::FilterSub;
use rbr_formats::dls::DlsReplayStream;

/// Magic of a filter-bank stream (`ReadStream` returns a freed bank otherwise).
pub const STREAM_MAGIC: u32 = 6;
/// Words per stream record.
pub const SUB_WORDS: usize = 10;
/// Words per check record.
pub const CHECK_WORDS: usize = 6;
/// The `f0 - 20` / `f1 + 20` check spread.
pub const CHECK_SPREAD: f32 = 20.0;

/// One 0x20-byte check entry (`E19Chk`, `RBR_E19.h:547`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Check {
    /// +0x00.
    pub pos0: f32,
    /// +0x04: the window start `Tracker` scans (`m_04 <= f <= m_04 + m_08`).
    pub pos: f32,
    /// +0x08: the window length.
    pub span: f32,
    /// +0x0c.
    pub lane: i32,
    /// +0x10: the pair code (`ReadStream` writes the third stream word here when `f4`/`f5` are set).
    pub code: i32,
    /// +0x14: the high speed bound (`SchoolIncident`'s band).
    pub hi: f32,
    /// +0x18: the low speed bound.
    pub lo: f32,
    /// +0x1c: the trip state (`EvalFilters` fails the lesson on +-1; `SchoolIncident` trips it through the lead).
    pub state: i32,
}

impl Check {
    /// `E19Chk::Clear` (0x498630).
    pub const fn clear() -> Self {
        Self { pos0: 0.0, pos: 0.0, span: 0.0, lane: 0, code: 0, hi: 0.0, lo: 0.0, state: 0 }
    }
}

/// One 8-byte threshold/code pair (`E19Pair`, `RBR_E19.h:542`): `Trigger` fires the code once `v` reaches the
/// threshold. The code is the animation block base plus the stream word (`E12_AnimSet::BlockAddB` 0x475c90, a plain
/// `base + v` add).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    pub threshold: f32,
    pub code: i32,
}

/// Which object the bank's +0x34 lead word points at. `Tracker` 0x499360 sets it to a check; `SchoolIncident`
/// 0x498e10 reads it back as a sub, so a check lead is read through the sub's first five words (the deeper words are
/// never touched through the lead and read as zero here, matching a fresh bank).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    Sub(usize),
    Check(usize),
}

/// The 0x60-byte filter bank. Offsets are the original's.
#[derive(Debug, Clone, PartialEq)]
pub struct Bank {
    /// +0x00: written by `Tracker`, read by `Trigger` and `Publish`.
    pub origin: i32,
    /// +0x04: the car speed in km/h (`SchoolIncident`).
    pub speed: f32,
    /// +0x08/+0x0c/+0x10: the time ladder (`EvalFilters`): slower than the limit or the pass time fails, at most the
    /// perfect time is perfect (4), between them passes (3). All 0 is untimed.
    pub time_limit: f32,
    /// +0x0c.
    pub pass_time: f32,
    /// +0x10.
    pub perfect_time: f32,
    /// +0x14/+0x18: the sub array.
    pub subs: Vec<FilterSub>,
    /// +0x1c: the four enlisted sub slots (`Tracker` fills, `SchoolTick`/`SchoolIncident` run).
    pub slots: [Option<usize>; 4],
    /// +0x2c/+0x30: the check array.
    pub checks: Vec<Check>,
    /// +0x34: the lead entry.
    pub lead: Option<Lead>,
    /// +0x38/+0x3c: the threshold/code pairs.
    pub pairs: Vec<Pair>,
    /// +0x40: the sound-pair ack mask (`Trigger` sets, `Clear40` clears).
    pub ack: u32,
    /// +0x44: trip latch A (`Checkpoint` sets on the stop cone).
    pub latched_a: i32,
    /// +0x48: trip latch B (`Set48_1`, the school retire path, sets).
    pub latched_b: i32,
    /// +0x4c: no ported function reads it; `InitState` zeroes it, the rest preserves it.
    pub kept4c: u32,
    /// +0x50: the end-race timer (`Checkpoint` rearms to 10, `EndRace` decays).
    pub timer: f32,
    /// +0x54: the sound-pair gate (`RallySchoolSound` sets).
    pub sounded: i32,
    /// +0x58: the pair-scan live flag (`InitState` sets 1; `Trigger` returns while set).
    pub live: i32,
    /// +0x5c: whether `ReadStream` allocated the repair handler (the `SeverityMax` damage source).
    pub has_handler: bool,
}

impl Bank {
    /// A zeroed bank: what `ReadStream` fills (the engine zeroes a fresh allocation, then `FreeState` wipes
    /// the arrays, `m_00` and the ack before the fill).
    pub fn fresh() -> Self {
        Self {
            origin: 0,
            speed: 0.0,
            time_limit: 0.0,
            pass_time: 0.0,
            perfect_time: 0.0,
            subs: Vec::new(),
            slots: [None; 4],
            checks: Vec::new(),
            lead: None,
            pairs: Vec::new(),
            ack: 0,
            latched_a: 0,
            latched_b: 0,
            kept4c: 0,
            timer: 0.0,
            sounded: 0,
            live: 1,
            has_handler: false,
        }
    }

    /// `CReplayManager::InitState` (0x4980c0): zeroes everything but the speed (`m_04`) and the ack mask
    /// (`m_40`), sets the live flag. `m_5c` is cleared with no handler release.
    pub fn init_state(&mut self) {
        let (speed, ack) = (self.speed, self.ack);
        *self = Self::fresh();
        self.speed = speed;
        self.ack = ack;
    }

    /// `CReplayManager::ReadStream` (0x498840) as a constructor from the parsed stream and the prior bank:
    /// `FreeState` first (arrays freed, `m_00` and the ack cleared, the speed, latches, `m_4c`, timer, sounded
    /// and live kept), then on magic 6 the header, the sub/check/pair arrays, and the handler when anything
    /// was read or the game is in school mode (any other magic returns the freed bank, cursor unmoved).
    ///
    /// `block_base` is the animation block word `BlockAddB` adds to the pair codes; `is_school` is
    /// `E60Flow::M566240()` (0x566240), the mode-7 predicate.
    pub fn from_stream(prior: &Bank, stream: &DlsReplayStream, block_base: u32, is_school: bool) -> Self {
        let mut bank = Self {
            speed: prior.speed,
            latched_a: prior.latched_a,
            latched_b: prior.latched_b,
            kept4c: prior.kept4c,
            timer: prior.timer,
            sounded: prior.sounded,
            live: prior.live,
            ..Self::fresh()
        };
        if stream.header[0] != STREAM_MAGIC {
            return bank;
        }
        bank.time_limit = f32::from_bits(stream.header[6]);
        bank.pass_time = f32::from_bits(stream.header[5]);
        bank.perfect_time = f32::from_bits(stream.header[4]);
        bank.subs = stream
            .ten_word_records
            .iter()
            .map(|w| {
                // The record order is tag, class, center, hi, lo, near, flags, lim0, lim1, lim2: the fill
                // stores words 4/5/6 to `m_18`/`m_1c`/`m_0c` (the reconstruction reads them in order).
                let mut sub = FilterSub::init_sub();
                sub.tag = w[0] as i32;
                sub.class = w[1] as i32;
                sub.center = f32::from_bits(w[2]);
                sub.hi = f32::from_bits(w[3]);
                sub.lo = f32::from_bits(w[4]);
                sub.near_bits = w[5];
                sub.flags = w[6] as i32;
                sub.lim0 = w[7] as i32;
                sub.lim1 = w[8] as i32;
                sub.lim2 = w[9] as i32;
                sub
            })
            .collect();
        // A `ucomiss`+`lahf`+`test 0x44`+`jnp` pair gates each word on ordered-nonzero, like `IsArmed`
        // (NaN passes, `-0.0` fails). `[esp+0x28]` is refreshed with the current r4 before the gate reads
        // it, so there is no cross-record staleness.
        bank.checks = stream
            .six_word_records
            .iter()
            .map(|w| {
                let f = |i: usize| f32::from_bits(w[i]);
                let mut check = Check::clear();
                check.pos0 = f(0);
                check.pos = f(1);
                check.span = f(2) - f(1);
                check.lo = f(0) - CHECK_SPREAD;
                check.hi = f(0) + CHECK_SPREAD;
                if f(5) != 0.0 && f(4) != 0.0 {
                    // `mov`, not a convert: the code is the word's bits.
                    check.code = w[3] as i32;
                    check.hi = f(5);
                    check.lo = f(4);
                }
                check
            })
            .collect();
        bank.pairs = stream
            .pairs
            .iter()
            .map(|w| Pair { threshold: f32::from_bits(w[0]), code: block_base.wrapping_add(w[1]) as i32 })
            .collect();
        bank.has_handler = !bank.pairs.is_empty() || !bank.checks.is_empty() || !bank.subs.is_empty() || is_school;
        bank
    }

    /// The lead read as a sub's first words (`SchoolIncident` 0x498e10): tag as float, flags, high and low bounds.
    /// A check lead maps `m_00/m_0c/m_14/m_18` (words past the check are never read through the lead).
    pub fn lead_vals(&self) -> Option<(f32, i32, f32, f32)> {
        match self.lead? {
            Lead::Sub(i) => {
                let sub = self.subs.get(i)?;
                Some((sub.tag_f(), sub.flags, sub.hi, sub.lo))
            }
            Lead::Check(i) => {
                let check = self.checks.get(i)?;
                Some((check.pos0, check.lane, check.hi, check.lo))
            }
        }
    }

    /// `Trip` through the lead (`SchoolIncident` writes the trip latch: a sub's `m_1c`, a check's `m_1c` state).
    pub fn lead_trip(&mut self, v: f32) {
        match self.lead {
            Some(Lead::Sub(i)) => {
                if let Some(sub) = self.subs.get_mut(i) {
                    sub.trip(v);
                }
            }
            Some(Lead::Check(i)) => {
                // A check lead goes through `Trip` too (`m_14`/`m_18`/`m_1c` are its hi/lo/state): armed
                // and unlatched only.
                if let Some(check) = self.checks.get_mut(i) {
                    if check.hi == 0.0 || check.lo == 0.0 || check.state != 0 {
                        return;
                    }
                    if check.lo > v {
                        check.state = -1;
                    } else if v > check.hi {
                        check.state = 1;
                    }
                }
            }
            None => {}
        }
    }

    /// The `m_0c = 1` trip-flag write through the lead (a sub's flags, a check's lane).
    pub fn lead_set_flag(&mut self) {
        match self.lead {
            Some(Lead::Sub(i)) => {
                if let Some(sub) = self.subs.get_mut(i) {
                    sub.flags = 1;
                }
            }
            Some(Lead::Check(i)) => {
                if let Some(check) = self.checks.get_mut(i) {
                    check.lane = 1;
                }
            }
            None => {}
        }
    }

    /// `CReplayManager::Get44` (0x4984a0).
    pub fn latched_a(&self) -> i32 {
        self.latched_a
    }

    /// `CReplayManager::Get48` (0x4984b0).
    pub fn latched_b(&self) -> i32 {
        self.latched_b
    }

    /// `CReplayManager::Set44` (0x4984c0).
    pub fn set_latched_a(&mut self, v: i32) {
        self.latched_a = v;
    }

    /// `CReplayManager::Set48` (0x4984d0).
    pub fn set_latched_b(&mut self, v: i32) {
        self.latched_b = v;
    }

    /// `CReplayManager::Set48_1` (0x4984e0): the school retire write (`E60Flow::M5662c0` 0x5662c0 calls it and
    /// returns, the only in-step school store).
    pub fn set_latched_b_1(&mut self) {
        self.latched_b = 1;
    }

    /// `CReplayManager::Set44_1` (0x498500).
    pub fn set_latched_a_1(&mut self) {
        self.latched_a = 1;
    }

    /// `CReplayManager::Clear40` (0x498510).
    pub fn clear_ack(&mut self) {
        self.ack = 0;
    }

    /// `CReplayManager::Publish` (0x498520): snapshots the BSS mirrors into the bank, the inverse of `Capture`
    /// (the reconstruction has the two bodies swapped). Bytes past the 16-entry mirrors are read from the
    /// neighboring BSS in the original; the port takes full mirrors and indexes the bank's own arrays, so banks
    /// with more than 16 subs or checks are not modeled.
    ///
    /// Returns the value the original stores at `DAT_0165fc68 + 0x140` (the message owner's word from `misc20`).
    pub fn publish(&mut self, mirrors: &Mirrors) -> u32 {
        for (i, (sub, &button)) in self.subs.iter_mut().zip(&mirrors.buttons).enumerate() {
            sub.value = button as i32;
            if mirrors.btn_mask & (1u32 << (i as u32 & 31)) != 0 {
                sub.flags |= 8;
            }
        }
        if mirrors.flags & 2 != 0 {
            self.latched_b = 1;
        }
        if mirrors.flags & 1 != 0 {
            self.latched_a = 1;
        }
        for (check, &state) in self.checks.iter_mut().zip(&mirrors.checks) {
            check.state = state as i8 as i32;
        }
        self.origin = mirrors.misc04 as i32;
        mirrors.misc20
    }
}

/// The BSS mirrors `Publish` reads (`RBR_E19.h` globals): button bytes at 0x893624, the mask at 0x89360c, flags at
/// 0x893608, check bytes at 0x893610, `misc20` at 0x893620 and `misc04` at 0x893604.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mirrors {
    pub buttons: [u8; 16],
    pub btn_mask: u32,
    pub flags: u32,
    pub checks: [u8; 16],
    pub misc20: u32,
    pub misc04: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(header: [u32; 7]) -> DlsReplayStream {
        DlsReplayStream {
            offset: 0,
            header,
            ten_word_records: Vec::new(),
            six_word_records: Vec::new(),
            pairs: Vec::new(),
        }
    }

    #[test]
    fn non_magic_stream_is_a_freed_bank() {
        let bank = Bank::from_stream(&Bank::fresh(), &stream([0, 3, 1, 2, 0, 0, 0]), 0, true);
        assert_eq!(bank, Bank::fresh());
        assert!(bank.subs.is_empty() && !bank.has_handler);
    }

    #[test]
    fn sub_words_keep_the_exe_order() {
        // [tag, class bits, center, hi, lo, near, flags, lim0, lim1, lim2]: words 4/5/6 land in
        // `m_18`/`m_1c`/`m_0c` (the reconstruction reads them in order).
        let mut raw = stream([6, 1, 0, 0, 11, 10, 9]);
        raw.ten_word_records.push([
            6,
            1,
            79.9f32.to_bits(),
            36.4f32.to_bits(),
            0x4225cccd,
            46.4f32.to_bits(),
            4.0f32.to_bits(),
            10,
            (-2i32) as u32,
            (-2i32) as u32,
        ]);
        let bank = Bank::from_stream(&Bank::fresh(), &raw, 0, false);
        assert_eq!(bank.subs.len(), 1);
        let sub = bank.subs[0];
        assert_eq!((sub.tag, sub.class), (6, 1));
        assert_eq!((sub.center, sub.hi), (79.9, 36.4));
        assert_eq!(sub.lo, f32::from_bits(0x4225cccd));
        assert_eq!(sub.near_bits, 46.4f32.to_bits());
        assert_eq!(sub.flags, 4.0f32.to_bits() as i32);
        assert_eq!((sub.lim0, sub.lim1, sub.lim2), (10, -2, -2));
        assert!(bank.has_handler);
    }

    #[test]
    fn check_fill_overrides_only_when_both_set() {
        let mut raw = stream([6, 0, 1, 0, 0, 0, 0]);
        raw.six_word_records.push([
            100.0f32.to_bits(),
            113.314f32.to_bits(),
            115.914f32.to_bits(),
            50.0f32.to_bits(),
            87.0f32.to_bits(),
            130.0f32.to_bits(),
        ]);
        let bank = Bank::from_stream(&Bank::fresh(), &raw, 0x1000, false);
        let check = bank.checks[0];
        assert_eq!((check.pos0, check.pos, check.span), (100.0, 113.314, 115.914 - 113.314));
        assert_eq!((check.code, check.hi, check.lo), (50.0f32.to_bits() as i32, 130.0, 87.0));

        // A zero word 4 skips the override: the computed spread stays.
        let mut raw = stream([6, 0, 1, 0, 0, 0, 0]);
        raw.six_word_records.push([
            100.0f32.to_bits(),
            113.314f32.to_bits(),
            115.914f32.to_bits(),
            50.0f32.to_bits(),
            0,
            130.0f32.to_bits(),
        ]);
        let bank = Bank::from_stream(&Bank::fresh(), &raw, 0x1000, false);
        let check = bank.checks[0];
        assert_eq!((check.hi, check.lo), (100.0 + 20.0, 100.0 - 20.0));
    }

    #[test]
    fn publish_snapshots_the_mirrors_into_the_bank() {
        let mut bank = Bank::fresh();
        bank.subs.push(FilterSub::init_sub());
        bank.checks.push(Check::clear());
        let mirrors = Mirrors {
            buttons: [7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            btn_mask: 1,
            flags: 3,
            checks: [0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            misc20: 0x1234,
            misc04: 0x5678,
        };
        assert_eq!(bank.publish(&mirrors), 0x1234);
        assert_eq!((bank.subs[0].value, bank.subs[0].flags), (7, 8));
        assert_eq!((bank.latched_a, bank.latched_b), (1, 1));
        assert_eq!(bank.checks[0].state, -1);
        assert_eq!(bank.origin, 0x5678);
    }
}
