//! Days since 1970 (UTC) and their dates. Every table keys its day as an integer; the dashboard
//! turns it into `YYYY-MM-DD`.

pub const DAY: i64 = 86_400;

pub fn day_of(t: i64) -> i64 {
    t.div_euclid(DAY)
}

pub fn day_start(day: i64) -> i64 {
    day * DAY
}

/// `YYYY-MM-DD` of a day number (proleptic Gregorian; Howard Hinnant's civil_from_days).
pub fn ymd(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(day_of(1_769_000_000)), "2026-01-21");
        assert_eq!(day_of(-1), -1);
    }
}
