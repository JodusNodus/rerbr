//! The season's seed from the clock.

/// The season's seed over the local time.
pub fn seed(second: u32, minute: u32, millisecond: u32, hour: u32, weekday: u32, day: u32, year: u32) -> u32 {
    second + minute + millisecond + hour + weekday + day + year
}

/// The season's seed from a Unix time in milliseconds (ours: UTC; the original reads the local time).
pub fn seed_from_unix_ms(ms: u64) -> u32 {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days to the civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (yoe + era * 400 + i64::from(month <= 2)) as u32;
    let weekday = ((days + 4).rem_euclid(7)) as u32;
    seed((rem % 60) as u32, (rem / 60 % 60) as u32, (ms % 1000) as u32, (rem / 3600) as u32, weekday, day, year)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_adds_the_time_fields() {
        // 2026-10-17 12:34:56.789 UTC, a Saturday (6).
        assert_eq!(seed_from_unix_ms(1_792_240_496_789), 56 + 34 + 789 + 12 + 6 + 17 + 2026);
    }
}
