//! The filter bank (`E19Bank`, 0x60 bytes, `RBR_E19.h:839-893`) and its construction from the lesson's `.dls`
//! stream (`CReplayManager::ReadStream` 0x498840, which `E12_AnimSet::Load020` 0x479020 runs on a fresh bank per
//! animation record).
//!
//! The stream (`rbr_formats::dls::DlsReplayStream`) is a 7-word header, ten-word sub records, six-word check records
//! and two-word pairs. `ReadStream` stores the header's three times reversed: `m_08` (the limit) from word 6, `m_0c`
//! (the pass time) from word 5, `m_10` (the perfect time) from word 4.

use super::filter::FilterSub;
use rbr_formats::dls::DlsReplayStream;

/// Magic of a filter-bank stream (`ReadStream` returns a freed bank otherwise).
const STREAM_MAGIC: u32 = 6;
/// The `f0 - 20` / `f0 + 20` speed band of a check.
const CHECK_SPREAD: f32 = 20.0;

/// One 0x20-byte check entry (`E19Chk`, `RBR_E19.h:547`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Check {
    /// +0x00.
    pub pos0: f32,
    /// +0x04: the window start `Tracker` scans (`m_04 <= f <= m_04 + m_08`).
    pub pos: f32,
    /// +0x08: the window length.
    pub span: f32,
    /// +0x0c: the flag `SchoolIncident` sets through the lead.
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

/// One 8-byte threshold/code pair (`E19Pair`, `RBR_E19.h:542`): `Trigger` fires the code once `v` reaches the
/// threshold. The code is the animation block base plus the stream word (`E12_AnimSet::BlockAddB` 0x475c90).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    pub threshold: f32,
    pub code: i32,
}

/// Which entry the bank's +0x34 lead word points at. `Tracker` 0x499360 sets it to a check; `SchoolIncident`
/// 0x498e10 reads it back as a sub, so a check lead is read through the sub's words (see [`LeadEntry`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    Sub(usize),
    Check(usize),
}

/// The lead read through a sub's words, as `SchoolIncident` reads it: `m_00` as a float, the `m_0c` flag, the
/// `m_14`/`m_18` bounds, and `Trip` on `m_1c`.
pub(crate) trait LeadEntry {
    /// `m_00` as a float, `m_0c`, `m_14` and `m_18`.
    fn words(&self) -> (f32, i32, f32, f32);
    /// `E19FltSub::Trip` (0x497dd0) on the entry.
    fn trip(&mut self, v: f32);
    /// The `m_0c = 1` write.
    fn set_flag(&mut self);
}

impl LeadEntry for FilterSub {
    fn words(&self) -> (f32, i32, f32, f32) {
        (self.tag_f(), self.flags, self.hi, self.lo)
    }
    fn trip(&mut self, v: f32) {
        FilterSub::trip(self, v);
    }
    fn set_flag(&mut self) {
        self.flags = 1;
    }
}

impl LeadEntry for Check {
    fn words(&self) -> (f32, i32, f32, f32) {
        (self.pos0, self.lane, self.hi, self.lo)
    }
    /// `Trip` on the check's `m_14`/`m_18`/`m_1c` (its hi, lo and state): armed and unlatched only.
    fn trip(&mut self, v: f32) {
        if self.hi == 0.0 || self.lo == 0.0 || self.state != 0 {
            return;
        }
        if self.lo > v {
            self.state = -1;
        } else if v > self.hi {
            self.state = 1;
        }
    }
    fn set_flag(&mut self) {
        self.lane = 1;
    }
}

/// The 0x60-byte filter bank; the offsets are the original's.
#[derive(Debug, Clone, PartialEq)]
pub struct Bank {
    /// +0x00: the driveline parameter's word, written by `Tracker`, read as a float by the listeners.
    pub origin: i32,
    /// +0x04: the car speed in km/h (`SchoolIncident`).
    pub speed: f32,
    /// +0x08/+0x0c/+0x10: the time ladder (`EvalFilters`): slower than the limit or the pass time fails, at most the
    /// perfect time is perfect (4), between them passes (3). All 0 is untimed.
    pub time_limit: f32,
    pub pass_time: f32,
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
    /// +0x44: trip latch A (`Checkpoint` sets it on a brake-wall crossing).
    pub latched_a: bool,
    /// +0x48: trip latch B (`Set48_1`, the school's retire).
    pub latched_b: bool,
    /// +0x50: the end-race timer (`Checkpoint` rearms it to 10, `EndRace` decays it).
    pub timer: f32,
    /// +0x54: the sound-pair gate (`Trigger` sets it in an untimed lesson's drive).
    pub sounded: bool,
    /// +0x58: the pair scan's hold (`InitState` sets it; `Trigger` returns while set).
    pub live: bool,
}

impl Bank {
    /// The bank `InitState` (0x4980c0) leaves on a fresh allocation, before `ReadStream` fills it.
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
            latched_a: false,
            latched_b: false,
            timer: 0.0,
            sounded: false,
            live: true,
        }
    }

    /// `CReplayManager::ReadStream` (0x498840) as a constructor from the parsed stream and the prior bank:
    /// `FreeState` first (arrays, slots, lead, `m_00` and the ack cleared; the speed, latches, timer, gate and hold
    /// kept), then on magic 6 the header's times and the sub, check and pair arrays (any other magic gives the freed
    /// bank). `block_base` is the animation block word `BlockAddB` adds to the pair codes.
    pub fn from_stream(prior: &Bank, stream: &DlsReplayStream, block_base: u32) -> Self {
        let mut bank = Self {
            speed: prior.speed,
            latched_a: prior.latched_a,
            latched_b: prior.latched_b,
            timer: prior.timer,
            sounded: prior.sounded,
            live: prior.live,
            ..Self::fresh()
        };
        if stream.header[0] != STREAM_MAGIC {
            return bank;
        }
        let float = |w: u32| f32::from_bits(w);
        bank.time_limit = float(stream.header[6]);
        bank.pass_time = float(stream.header[5]);
        bank.perfect_time = float(stream.header[4]);
        // The record order is tag, class, center, hi, lo, near, flags, lim0, lim1, lim2: the fill stores words
        // 4/5/6 to `m_18`/`m_1c`/`m_0c` (the reconstruction reads them in order).
        bank.subs = stream
            .ten_word_records
            .iter()
            .map(|w| FilterSub {
                tag: w[0] as i32,
                class: w[1] as i32,
                center: float(w[2]),
                hi: float(w[3]),
                lo: float(w[4]),
                near_bits: w[5],
                flags: w[6] as i32,
                lim0: w[7] as i32,
                lim1: w[8] as i32,
                lim2: w[9] as i32,
                ..FilterSub::init_sub()
            })
            .collect();
        // A check's window is `f1..f2` and its speed band `f0 -+ 20`, which words 4 and 5 replace (with word 3's
        // bits as its code, a `mov`) only when both are ordered-nonzero (`ucomiss`+`lahf`+`test 0x44`+`jnp`: NaN
        // passes, `-0.0` fails).
        bank.checks = stream
            .six_word_records
            .iter()
            .map(|w| {
                let f = |i: usize| float(w[i]);
                let (code, hi, lo) = if f(5) != 0.0 && f(4) != 0.0 {
                    (w[3] as i32, f(5), f(4))
                } else {
                    (0, f(0) + CHECK_SPREAD, f(0) - CHECK_SPREAD)
                };
                Check { pos0: f(0), pos: f(1), span: f(2) - f(1), code, hi, lo, ..Check::default() }
            })
            .collect();
        bank.pairs = stream
            .pairs
            .iter()
            .map(|w| Pair { threshold: float(w[0]), code: block_base.wrapping_add(w[1]) as i32 })
            .collect();
        bank
    }

    /// The lead entry, read through a sub's words.
    pub(crate) fn lead_mut(&mut self) -> Option<&mut dyn LeadEntry> {
        match self.lead? {
            Lead::Sub(i) => self.subs.get_mut(i).map(|sub| sub as &mut dyn LeadEntry),
            Lead::Check(i) => self.checks.get_mut(i).map(|check| check as &mut dyn LeadEntry),
        }
    }
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
        let bank = Bank::from_stream(&Bank::fresh(), &stream([0, 3, 1, 2, 0, 0, 0]), 0);
        assert_eq!(bank, Bank::fresh());
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
        let bank = Bank::from_stream(&Bank::fresh(), &raw, 0);
        assert_eq!(bank.subs.len(), 1);
        let sub = bank.subs[0];
        assert_eq!((sub.tag, sub.class), (6, 1));
        assert_eq!((sub.center, sub.hi), (79.9, 36.4));
        assert_eq!(sub.lo, f32::from_bits(0x4225cccd));
        assert_eq!(sub.near_bits, 46.4f32.to_bits());
        assert_eq!(sub.flags, 4.0f32.to_bits() as i32);
        assert_eq!((sub.lim0, sub.lim1, sub.lim2), (10, -2, -2));
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
        let bank = Bank::from_stream(&Bank::fresh(), &raw, 0x1000);
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
        let bank = Bank::from_stream(&Bank::fresh(), &raw, 0x1000);
        let check = bank.checks[0];
        assert_eq!((check.hi, check.lo), (100.0 + 20.0, 100.0 - 20.0));
    }
}
