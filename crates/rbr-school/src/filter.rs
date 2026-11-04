//! The filter sub (`E19FltSub`, 0x2c bytes, `RBR_E19_Filter.cpp`): the bank's judge entries the lesson's `.dls`
//! stream fills, and the methods the school's listeners run on them.
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

/// One 0x2c-byte filter sub; the offsets are the original's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterSub {
    /// +0x00: the sub-mode tag (`SchoolTick`'s switch, `Tracker`'s enlist pointer kind).
    pub tag: i32,
    /// +0x04: the judge value (`EvalFilters`, `EndRace`, `WindowHit`'s zero gate).
    pub value: i32,
    /// +0x08: the mode select (`WindowHit` and `Update` read it as an int).
    pub class: i32,
    /// +0x0c: flags (`WindowHit`'s bits 0 and 1, `Update`'s bit 3, `EvalFilters`' and `EndRace`'s bit 2).
    pub flags: i32,
    /// +0x10: the window center.
    pub center: f32,
    /// +0x14: the high bound.
    pub hi: f32,
    /// +0x18: the low bound.
    pub lo: f32,
    /// +0x1c: the trip latch as int bits (`Trip`) and the near bound as float bits (`Update`, `WindowHit`).
    pub near_bits: u32,
    /// +0x20/+0x24/+0x28: the judge limits (read by the results page, not ported).
    pub lim0: i32,
    /// +0x24.
    pub lim1: i32,
    /// +0x28.
    pub lim2: i32,
}

impl FilterSub {
    /// `E19FltSubInit` (0x498660), the element init `ReadStream` runs over every new sub.
    pub fn init_sub() -> Self {
        Self {
            tag: 0,
            value: 0,
            class: 1,
            flags: 0,
            center: 0.0,
            hi: 0.0,
            lo: 0.0,
            near_bits: 0,
            lim0: -1,
            lim1: -1,
            lim2: -1,
        }
    }

    /// +0x00 read as a float (`SchoolIncident`'s untripped delta path).
    pub fn tag_f(&self) -> f32 {
        f32::from_bits(self.tag as u32)
    }

    /// +0x1c read as a float (`Update`, `WindowHit`).
    fn near_f(&self) -> f32 {
        f32::from_bits(self.near_bits)
    }

    /// `E19FltSub::IsArmed` (0x497da0): `m_14` and `m_18` both ordered-nonzero. Each is a `ucomiss` against zero
    /// plus `lahf`/`test 0x44`/`jnp`, false only on ordered equality: a NaN bound is armed, `-0.0` is not, as
    /// `!= 0.0` gives.
    pub fn is_armed(&self) -> bool {
        self.hi != 0.0 && self.lo != 0.0
    }

    /// `E19FltSub::Trip` (0x497dd0): an armed, unlatched sub trips once: `m_1c = -1` when `m_18 > v`, else
    /// `m_1c = 1` when `v > m_14`; NaN stores nothing (`jbe` is taken unordered).
    pub fn trip(&mut self, v: f32) {
        if !self.is_armed() || self.near_bits != 0 {
            return;
        }
        if self.lo > v {
            self.near_bits = (-1i32) as u32;
        } else if v > self.hi {
            self.near_bits = 1;
        }
    }

    /// The bound `WindowHit` (and `Pick`) select by the mode word: `m_18` for 2, `m_1c` for 3, else `m_14`.
    fn mode_lim(&self) -> f32 {
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
    fn trip_fires_once_when_armed() {
        let mut s = FilterSub::init_sub();
        s.hi = 10.0;
        s.lo = -10.0;
        s.trip(20.0);
        assert_eq!(s.near_bits as i32, 1);
        // A latched sub never trips again.
        s.trip(-20.0);
        assert_eq!(s.near_bits as i32, 1);
        // Zero bounds disarm, NaN bounds do not, NaN input stores nothing.
        let mut z = FilterSub::init_sub();
        z.hi = 10.0;
        z.trip(20.0);
        assert_eq!(z.near_bits as i32, 0, "zero m_18 disarms");
        z.lo = -10.0;
        z.trip(f32::NAN);
        assert_eq!(z.near_bits as i32, 0, "NaN stores nothing");
        z.lo = f32::NAN;
        z.hi = f32::NAN;
        z.trip(20.0);
        assert_eq!(z.near_bits as i32, 0, "NaN bounds are armed but compare unordered");
    }

    #[test]
    fn is_armed_needs_nonzero_bounds() {
        // `init_sub` zeroes `m_14`/`m_18`.
        assert!(!FilterSub::init_sub().is_armed());
        let mut s = FilterSub::init_sub();
        s.hi = 1.0;
        s.lo = 2.0;
        assert!(s.is_armed());
        s.lo = f32::NAN;
        assert!(s.is_armed(), "NaN counts as armed");
        s.lo = -0.0;
        assert!(!s.is_armed(), "-0.0 compares equal");
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
