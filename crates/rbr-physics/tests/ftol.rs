//! `math::ftol::trunc_i64` (the CRT's `_ftol2`, 0x70c2c4): the truncation toward zero, the low word the callers keep, and the
//! indefinite value for NaN and out-of-range inputs.

use rbr_physics::math::ftol::trunc_i64;

#[test]
fn trunc_i64_truncates_like_the_crt() {
    assert_eq!(trunc_i64(2.9), 2);
    assert_eq!(trunc_i64(-2.9), -2);
    assert_eq!(trunc_i64(-0.5), 0);
    assert_eq!(trunc_i64(2147483653.0), 2147483653);
    // The callers keep the low word, so a negative value is its two's complement.
    assert_eq!(trunc_i64(-1.0) as u32, 0xFFFF_FFFF);
    // NaN and values outside the i64 range give i64::MIN, whose low word is 0.
    assert_eq!(trunc_i64(f64::NAN), i64::MIN);
    assert_eq!(trunc_i64(9223372036854775808.0), i64::MIN);
    assert_eq!(trunc_i64(-1e30), i64::MIN);
    assert_eq!(trunc_i64(f64::INFINITY), i64::MIN);
    assert_eq!(trunc_i64(f64::NAN) as u32, 0);
}
