//! The time, for the few things the engine does by the clock.
//!
//! In the browser the engine reads JavaScript's clock; at build time and in
//! tests, the system's. Nothing below a route handler reads it: code that
//! depends on the time takes it as an argument, and a handler passes
//! [`now_ms`]. That keeps such code testable at any time of day.

/// Now, in milliseconds since 1970-01-01 UTC.
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as f64)
            .unwrap_or(0.0)
    }
}

/// The date (UTC) of a moment, as `YYYY-MM-DD`.
pub fn date_of(ms: f64) -> String {
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's
    // `civil_from_days`): count from 0000-03-01 in 400-year eras, so that a
    // leap day is the last day of its year.
    let days = (ms / 86_400_000.0).floor() as i64;
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// A moment (UTC) as text, to the millisecond: `YYYY-MM-DDTHH:MM:SS.mmmZ`,
/// which is how JavaScript writes one.
pub fn moment_of(ms: f64) -> String {
    let in_day = ms.rem_euclid(86_400_000.0).floor() as u64;
    format!(
        "{}T{:02}:{:02}:{:02}.{:03}Z",
        date_of(ms),
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1_000 % 60,
        in_day % 1_000
    )
}

/// Today's date (UTC), as `YYYY-MM-DD`.
pub fn today() -> String {
    date_of(now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: f64 = 86_400_000.0;

    #[test]
    fn a_moment_is_written_as_javascript_writes_it() {
        assert_eq!(moment_of(0.0), "1970-01-01T00:00:00.000Z");
        // 2026-11-18T20:15:30.123Z, as `new Date(1795032930123).toISOString()` gives.
        assert_eq!(moment_of(1_795_032_930_123.0), "2026-11-18T20:15:30.123Z");
        assert_eq!(moment_of(20_774.0 * DAY - 1.0), "2026-11-16T23:59:59.999Z");
    }

    #[test]
    fn a_moment_has_the_date_it_falls_on_in_utc() {
        assert_eq!(date_of(0.0), "1970-01-01");
        assert_eq!(date_of(DAY - 1.0), "1970-01-01");
        assert_eq!(date_of(DAY), "1970-01-02");
        // 2026-11-17 00:00:00 UTC is 20,774 days after the epoch.
        assert_eq!(date_of(20_774.0 * DAY), "2026-11-17");
        assert_eq!(date_of(20_774.0 * DAY + DAY - 1.0), "2026-11-17");
    }

    #[test]
    fn leap_days_and_year_ends_fall_right() {
        // 2024-02-29 is day 19,782; 2000-02-29 is day 11,016.
        assert_eq!(date_of(19_782.0 * DAY), "2024-02-29");
        assert_eq!(date_of(19_783.0 * DAY), "2024-03-01");
        assert_eq!(date_of(11_016.0 * DAY), "2000-02-29");
        // 2023 was not a leap year: 28 February is followed by 1 March.
        assert_eq!(date_of(19_416.0 * DAY), "2023-02-28");
        assert_eq!(date_of(19_417.0 * DAY), "2023-03-01");
        assert_eq!(date_of(20_453.0 * DAY), "2025-12-31");
        assert_eq!(date_of(20_454.0 * DAY), "2026-01-01");
    }

    #[test]
    fn today_is_a_date() {
        let today = today();
        assert_eq!(today.len(), 10);
        assert!(today.as_str() >= "2026-01-01", "{}", today);
        assert!(now_ms() > 1.7e12);
    }
}
