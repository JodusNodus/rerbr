//! The rally-school filter sub (`E19FltSub`, 0x2c bytes) and check (`E19Chk`, 0x20 bytes) primitives
//! (`RBR_E19_Filter.cpp`, `RBR_E19.h:502-560`): the in-bank judge entries the lesson's `.dls` replay stream fills.
//!
//! The original stores several words in two views: `m_00` is the sub-mode tag as an int and the speed reference as a
//! float (`SchoolIncident` 0x498e10 reads it as a float), `m_08` is the mode select as an int and a window bound as
//! float bits, and `m_1c` is a trip latch as an int (`Trip` writes -1/1) and a proximity bound as a float. This port
//! keeps the raw bits and converts at each use, exactly as the asm does (a `movss` keeps bits, a `cvtsi2ss` would
//! convert; the port spells which one each method is).
//!
//! Every comparison follows the asm's branch sense so NaN goes the same way as the original: `jb`/`jbe` are taken
//! when unordered, `ja`/`jae` are not.

// The comparisons follow the asm's, so a NaN takes the original's branch: `!(a > b)` is not `a <= b`.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

/// The +20.0 window `Init` (0x497d20) and the check fill (`ReadStream` 0x498840) build around the centers.
pub const WINDOW: f32 = 20.0;

/// `E19MaxAbs` (0x497b10): the largest absolute lane. The x87 `fcomip`+`jbe` keeps the current best when
/// unordered, so the first NaN sticks and later lanes can never replace it; `b > m` spells that.
pub fn max_abs(v: [f32; 3]) -> f32 {
    let mut m = v[0].abs();
    if v[1].abs() > m {
        m = v[1].abs();
    }
    if v[2].abs() > m {
        m = v[2].abs();
    }
    m
}

/// One 0x2c-byte filter sub. Field names are the exe's where known (`m_00i` etc. are ours); offsets in the doc
/// comments are the original's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterSub {
    /// +0x00: the sub-mode tag (`SchoolTick`'s switch, `Tracker`'s enlist pointer kind).
    pub tag: i32,
    /// +0x04: the judge value (`EvalFilters`, `EndRace`, `WindowHit`'s zero gate).
    pub value: i32,
    /// +0x08: the mode select (`Classify`/`Pick`/`WindowHit`/`Over`/`Update` read it as an int).
    pub class: i32,
    /// +0x0c: flags (`FlagMap`, `Update`'s bit 3, `Publish`'s ack bit).
    pub flags: i32,
    /// +0x10: the window center.
    pub center: f32,
    /// +0x14: the high bound.
    pub hi: f32,
    /// +0x18: the low bound.
    pub lo: f32,
    /// +0x1c: the trip latch as int bits (`Trip`) and the near bound as float bits (`Update` and friends).
    pub near_bits: u32,
    /// +0x20/+0x24/+0x28: the judge limits.
    pub lim0: i32,
    /// +0x24.
    pub lim1: i32,
    /// +0x28.
    pub lim2: i32,
}

impl FilterSub {
    /// All words zero (what `ReadStream` 0x498840 fills; `E19FltSubInit` runs first).
    pub const ZERO: Self = Self {
        tag: 0,
        value: 0,
        class: 0,
        flags: 0,
        center: 0.0,
        hi: 0.0,
        lo: 0.0,
        near_bits: 0,
        lim0: 0,
        lim1: 0,
        lim2: 0,
    };

    /// `E19FltSubInit` (0x498660), the `NLB_ApplyAll` element init `ReadStream` runs over every new sub.
    pub fn init_sub() -> Self {
        Self { class: 1, lim0: -1, lim1: -1, lim2: -1, ..Self::ZERO }
    }

    /// +0x00 read as a float (`SchoolIncident`'s untripped delta path).
    pub fn tag_f(&self) -> f32 {
        f32::from_bits(self.tag as u32)
    }

    /// +0x04 read as a float (`InRange` 0x497cf0 subtracts it with `subss`).
    pub fn value_f(&self) -> f32 {
        f32::from_bits(self.value as u32)
    }

    /// +0x08 read as a float (`InRange` 0x497cf0, `Over2` 0x498420 load it with `movss`).
    pub fn class_f(&self) -> f32 {
        f32::from_bits(self.class as u32)
    }

    /// +0x0c read as a float (`IsZeroAny` 0x4981a0, `Category` 0x4981e0 load it with `movss`).
    pub fn flags_f(&self) -> f32 {
        f32::from_bits(self.flags as u32)
    }

    /// +0x1c read as a float (`Update`, `WindowHit`, `Over`).
    pub fn near_f(&self) -> f32 {
        f32::from_bits(self.near_bits)
    }

    /// +0x1c read as an int (`Classify` 0x497e70 converts it with `cvtsi2ss`).
    pub fn near_i(&self) -> i32 {
        self.near_bits as i32
    }

    /// `E19FltSub::InRange` (0x497cf0): `0 <= v - m_04` and `m_08 >= v - m_04` (constant `0.0` at `0x795e50`),
    /// both via `comiss`+`jb`, so NaN is out of range either way.
    pub fn in_range(&self, v: f32) -> bool {
        let d = v - self.value_f();
        0.0 <= d && self.class_f() >= d
    }

    /// `E19FltSub::Init` (0x497d20): `m_08 = c - b`, `m_04 = b`, `m_00 = a`, `m_14 = a + 20`, `m_18 = a - 20`.
    /// The int-view words take the float bits (`movss` stores).
    pub fn init(&mut self, a: f32, b: f32, c: f32) {
        self.class = (c - b).to_bits() as i32;
        self.value = b.to_bits() as i32;
        self.tag = a.to_bits() as i32;
        self.hi = a + WINDOW;
        self.lo = a - WINDOW;
    }

    /// `E19FltSub::Deref` (0x497d70): `m_00` as a float (`flds`).
    pub fn deref(&self) -> f32 {
        self.tag_f()
    }

    /// `E19FltSub::SetThresh` (0x497d80): plain word stores, `m_14 = b`, `m_10 = a`, `m_18 = c` (the exe's order,
    /// which the reconstruction swaps).
    pub fn set_thresh(&mut self, a: i32, b: i32, c: i32) {
        self.hi = f32::from_bits(b as u32);
        self.center = f32::from_bits(a as u32);
        self.lo = f32::from_bits(c as u32);
    }

    /// `E19FltSub::IsArmed` (0x497da0): 1 when `m_14` and `m_18` are both ordered-nonzero. Each is a `ucomiss`
    /// against zero plus `lahf`/`test 0x44`/`jnp`, which returns 0 only on ordered equality: NaN continues (so a
    /// NaN bound counts as armed) while `-0.0` compares equal (disarmed), exactly like `!= 0.0`.
    pub fn is_armed(&self) -> i32 {
        if self.hi != 0.0 && self.lo != 0.0 {
            1
        } else {
            0
        }
    }

    /// `E19FltSub::Trip` (0x497dd0): only an armed, unlatched sub trips. The `ucomiss`+`lahf`+`test 0x44`+`jnp`
    /// pairs return at once when `m_14`/`m_18` is ordered-zero (NaN counts as armed), and a nonzero `m_1c` returns
    /// at once, so the latch fires once. Then `m_1c = -1` when `m_18 > v`, else `m_1c = 1` when `v > m_14`;
    /// NaN stores nothing (`jbe` is taken unordered).
    pub fn trip(&mut self, v: f32) {
        if self.hi == 0.0 || self.lo == 0.0 {
            return;
        }
        if self.near_bits != 0 {
            return;
        }
        if self.lo > v {
            self.near_bits = (-1i32) as u32;
        } else if v > self.hi {
            self.near_bits = 1u32;
        }
    }

    /// `E19FltSub::SetLim` (0x497e20).
    pub fn set_lim(&mut self, a: i32, b: i32, c: i32) {
        self.lim0 = a;
        self.lim1 = b;
        self.lim2 = c;
    }

    /// `E19FltSub::Get20` (0x497e40).
    pub fn get20(&self) -> i32 {
        self.lim0
    }

    /// `E19FltSub::Get24` (0x497e50).
    pub fn get24(&self) -> i32 {
        self.lim1
    }

    /// `E19FltSub::Get28` (0x497e60).
    pub fn get28(&self) -> i32 {
        self.lim2
    }

    /// The mode select `Pick`, `WindowHit`, `Over` and `Update` switch on (`m_08 == 2/3`, unsigned compares).
    fn mode_lim(&self) -> f32 {
        if self.class == 2 {
            self.lo
        } else if self.class == 3 {
            self.near_f()
        } else {
            self.hi
        }
    }

    /// `E19FltSub::Classify` (0x497e70). All bound loads are floats (`movss`); the reconstruction's int converts
    /// are wrong. Case 3 reads the low byte of `m_0c` (the reconstruction reads the high byte of `m_00`).
    pub fn classify(&self, v: f32) -> u32 {
        if self.tag < 6 || self.tag > 8 {
            if self.value != 0 {
                match self.value {
                    1 => return 0,
                    2 => return 4,
                    3 => return (((!(self.flags as u8)) & 8) | 4) as u32 >> 2,
                    4 => return 2,
                    _ => {}
                }
            }
            let mut d = v - self.center;
            let neg = d < 0.0;
            if neg {
                d = 0.0 - d;
            }
            if !(d <= self.hi) {
                if self.lo >= d && (self.class as u32) > 1 {
                    return (!neg as u32) * 2 + 1;
                }
                if self.near_f() >= d && (self.class as u32) > 2 {
                    return (neg as u32).wrapping_sub(1) & 4;
                }
                return (neg as u32) + 0x270e;
            }
        } else if self.value != 4 {
            // `comiss`+`jae` returns 0 only when the sum covers `v` ordered; unordered returns 1.
            if !(self.hi + self.center >= v) {
                return 1;
            }
            return 0;
        }
        2
    }

    /// `E19FltSub::Pick` (0x497f70).
    pub fn pick(&self) -> f32 {
        if self.class == 2 {
            self.lo
        } else if self.class == 3 {
            self.near_f()
        } else {
            self.hi
        }
    }

    /// `E19FltSub::WindowHit` (0x497f90). The sign test is `!(d < 0)` (the `jbe` is taken unordered), and the limit
    /// test is `!(lim >= a)`: NaN takes the bit-2 path and then misses.
    pub fn window_hit(&self, v: f32) -> bool {
        if self.value != 0 {
            return false;
        }
        let d = v - self.center;
        let a = d.abs();
        if self.hi >= a {
            return true;
        }
        let bits = self.flags as u32;
        let mask = if !(d < 0.0) { bits & 2 } else { bits & 1 };
        if mask != 0 {
            return false;
        }
        self.mode_lim() >= a
    }

    /// `E19FltSub::Set7` (0x498000).
    pub fn set7(&mut self, a: i32, b: i32, c: f32, d: f32, e: f32, f: f32) {
        self.center = c;
        self.hi = d;
        self.lo = e;
        self.class = a;
        self.flags = b;
        self.near_bits = f.to_bits();
    }

    /// `E19FltSub::MaxStore` (0x498040): `if (m_04 < a || b == 0) m_04 = a` (signed compare).
    pub fn max_store(&mut self, a: i32, b: i32) {
        if self.value < a || b == 0 {
            self.value = a;
        }
    }

    /// `E19FltSub::Over` (0x498060): `m_10 + lim < v` (`jbe` taken unordered, so NaN is not over).
    pub fn over(&self, v: f32) -> bool {
        self.center + self.mode_lim() < v
    }

    /// `E19FltSub::FlagMap` (0x4980a0): `(m_04 != 0) - 1 & 4` when bit 2 is set, else `m_04`.
    pub fn flag_map(&self) -> i32 {
        if self.flags & 4 != 0 {
            (i32::from(self.value != 0).wrapping_sub(1)) & 4
        } else {
            self.value
        }
    }

    /// `E19FltSub::IsZeroAny` (0x4981a0): 0 when `m_08`, `m_0c` and `m_10` are all nonzero as floats (NaN counts as
    /// nonzero, `-0.0` as zero, per the `lahf`/`test 0x44` shape), else 1.
    pub fn is_zero_any(&self) -> i32 {
        if self.class_f() != 0.0 && self.flags_f() != 0.0 && self.center != 0.0 {
            0
        } else {
            1
        }
    }

    /// `E19FltSub::Category` (0x4981e0): the reconstruction inverts the first two rungs; the original returns 0
    /// while `v` is above the bound (`ja` not taken unordered, so NaN falls through to 4).
    pub fn category(&self, v: f32) -> u32 {
        if self.class_f() == 0.0 || self.flags_f() == 0.0 || self.center == 0.0 {
            return 4;
        }
        if v > self.class_f() {
            return 0;
        }
        if v > self.flags_f() {
            return 0;
        }
        if v > self.center {
            return 3;
        }
        4
    }

    /// `E19FltSub::Over2` (0x498420): `o.m_08 + (float bits of o.m_04) < m_00` (the `addss` reinterprets the int
    /// bits; the reconstruction converts instead).
    pub fn over2(&self, other: &FilterSub) -> bool {
        self.tag_f() > other.class_f() + f32::from_bits(other.value as u32)
    }

    /// `E19FltSub::Over3` (0x498450): `o.m_10 + lim(o) < m_00`.
    pub fn over3(&self, other: &FilterSub) -> bool {
        self.tag_f() > other.center + other.mode_lim()
    }

    /// `E19FltSub::Get0c` (0x4983f0).
    pub fn get0c(&self) -> f32 {
        f32::from_bits(self.flags as u32)
    }

    /// `E19FltSub::Get08` (0x498400).
    pub fn get08(&self) -> f32 {
        self.class_f()
    }

    /// `E19FltSub::Get10` (0x498410).
    pub fn get10(&self) -> f32 {
        self.center
    }

    /// `E19FltSub::Update` (0x4986a0): the proximity updater. Every float test is `comiss`+`jb`/`jbe` with the
    /// memory operand as the destination, so unordered takes the under arm: `!(d <= bound)` spells each one.
    /// The near/class-3 path returns the (possibly raised) value in `eax`, not the neg flag (the callers ignore
    /// the status either way).
    pub fn update(&mut self, v: f32) -> i32 {
        let mut d = v - self.center;
        let neg = if d < 0.0 {
            d = 0.0 - d;
            1
        } else {
            0
        };
        if !(d <= self.hi) {
            if !(d <= self.lo) || (self.class as u32) < 2 {
                if !(d <= self.near_f()) || (self.class as u32) < 3 {
                    // The latch path returns the value in `eax` (a negative value is stored as 0 first,
                    // but the stale `eax` still returns).
                    if self.value < 0 {
                        let old = self.value;
                        self.value = 0;
                        return old;
                    }
                    return self.value;
                } else if neg == 0 {
                    if self.value < 2 {
                        let old = self.value;
                        self.value = 2;
                        return old;
                    }
                    return self.value;
                } else {
                    if self.value < 1 {
                        self.value = 1;
                    }
                    self.flags |= 8;
                    return self.value;
                }
            } else {
                if self.value < 3 {
                    self.value = 3;
                }
                if neg == 0 {
                    return neg;
                }
                self.flags |= 8;
                return neg;
            }
        } else if self.value < 4 {
            self.value = 4;
        }
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_abs_prefers_the_second_lane_when_unordered() {
        assert_eq!(max_abs([3.0, -4.0, 1.0]), 4.0);
        // The first NaN sticks: later lanes compare unordered and never replace it.
        assert!(max_abs([f32::NAN, 1.0, 0.0]).is_nan());
        assert_eq!(max_abs([1.0, f32::NAN, f32::NAN]), 1.0);
        assert_eq!(max_abs([1.0, f32::NAN, 0.0]), 1.0);
    }

    #[test]
    fn in_range_subtracts_m04() {
        let mut s = FilterSub::init_sub();
        s.center = 5.0;
        s.class = 2.0f32.to_bits() as i32;
        // `m_04` is zero here, so the window is `[0, m_08]`.
        assert!(s.in_range(2.0));
        assert!(s.in_range(0.0));
        assert!(!s.in_range(-0.1));
        assert!(!s.in_range(2.1));
        assert!(!s.in_range(f32::NAN));
        // A nonzero `m_04` shifts the window.
        s.value = 1.0f32.to_bits() as i32;
        assert!(s.in_range(2.0));
        assert!(!s.in_range(0.5));
        assert!(!s.in_range(3.1));
    }

    #[test]
    fn set_thresh_keeps_the_exe_word_order() {
        let mut s = FilterSub::init_sub();
        s.set_thresh(1, 2, 3);
        assert_eq!((s.center.to_bits(), s.hi.to_bits(), s.lo.to_bits()), (1, 2, 3));
    }

    #[test]
    fn trip_fires_once_when_armed() {
        let mut s = FilterSub::init_sub();
        s.hi = 10.0;
        s.lo = -10.0;
        s.trip(20.0);
        assert_eq!(s.near_i(), 1);
        // A latched sub never trips again.
        s.trip(-20.0);
        assert_eq!(s.near_i(), 1);
        // Zero bounds disarm, NaN bounds do not, NaN input stores nothing.
        let mut z = FilterSub::init_sub();
        z.hi = 10.0;
        z.trip(20.0);
        assert_eq!(z.near_i(), 0, "zero m_18 disarms");
        z.lo = -10.0;
        z.trip(f32::NAN);
        assert_eq!(z.near_i(), 0, "NaN stores nothing");
        z.lo = f32::NAN;
        z.hi = f32::NAN;
        z.trip(20.0);
        assert_eq!(z.near_i(), 0, "NaN bounds are armed but compare unordered");
    }

    #[test]
    fn is_armed_needs_nonzero_bounds() {
        // `init_sub` zeroes `m_14`/`m_18`.
        assert_eq!(FilterSub::init_sub().is_armed(), 0);
        let mut s = FilterSub::init_sub();
        s.hi = 1.0;
        s.lo = 2.0;
        assert_eq!(s.is_armed(), 1);
        s.lo = f32::NAN;
        assert_eq!(s.is_armed(), 1, "NaN counts as armed");
        s.lo = -0.0;
        assert_eq!(s.is_armed(), 0, "-0.0 compares equal");
    }

    #[test]
    fn window_hit_uses_unordered_sign_and_limit() {
        let mut s = FilterSub::init_sub();
        s.center = 10.0;
        s.hi = 5.0;
        assert!(s.window_hit(12.0));
        assert!(s.window_hit(10.0));
        // Outside with no flag bits: the limit is hi again.
        assert!(!s.window_hit(20.0));
        s.flags = 2;
        assert!(!s.window_hit(20.0), "bit 2 set, d >= 0 misses");
        s.flags = 1;
        assert!(!s.window_hit(0.0), "bit 1 set, d < 0 misses");
        assert!(!s.window_hit(f32::NAN));
    }
}
