//! Dependency-free timestamp helpers.
//!
//! The TS source stores `Date` objects, which serialise to ISO-8601 strings via
//! `Date.prototype.toISOString()`. To stay wire-compatible without pulling in
//! `chrono`, we format the current time the same way the sibling Rust ports do
//! (`memory-store`, `artifact-store`).

use std::time::{SystemTime, UNIX_EPOCH};

/// Current time as an ISO-8601 / RFC-3339 UTC string with millisecond
/// precision, e.g. `2026-06-11T09:21:00.000Z` — matching JS
/// `new Date().toISOString()`.
pub(crate) fn now_iso8601() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = since_epoch.as_secs();
    let millis = since_epoch.subsec_millis();

    let secs_of_day = total_secs % 86_400;
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;

    let days = (total_secs / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Convert a count of days since 1970-01-01 into a `(year, month, day)` civil
/// date. Port of Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}
