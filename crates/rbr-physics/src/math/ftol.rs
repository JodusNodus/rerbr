//! The CRT's float-to-int (`_ftol2`, which this emulates): an x87 value truncated toward zero to an int64. The callers
//! keep the low word. The conversion is pure, so it is here and not behind a host.

/// Truncates `x` toward zero, as the CRT does. A NaN or a value outside the i64 range gives `i64::MIN` (the int64
/// indefinite the CRT's `fistp` stores), whose low word is 0.
pub fn trunc_i64(x: f64) -> i64 {
    let t = x.trunc();
    if (-9223372036854775808.0..9223372036854775808.0).contains(&t) {
        t as i64
    } else {
        i64::MIN
    }
}
