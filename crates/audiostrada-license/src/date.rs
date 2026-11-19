/// Today's date in UTC (YYYY-MM-DD).
pub fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    civil(days)
}

/// The date `days` after `date` (YYYY-MM-DD; before it when negative).
pub fn days_after(date: &str, days: i64) -> Option<String> {
    let mut parts = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970-01-01 from a civil date (Howard Hinnant's algorithm).
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(civil(era * 146_097 + doe - 719_468 + days))
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_move_by_days_across_months_and_leap_years() {
        assert_eq!(days_after("2026-11-19", 0).as_deref(), Some("2026-11-19"));
        assert_eq!(days_after("2027-01-10", -30).as_deref(), Some("2026-12-11"));
        assert_eq!(days_after("2028-02-28", 1).as_deref(), Some("2028-02-29"));
        assert_eq!(days_after("1970-01-01", 0).as_deref(), Some("1970-01-01"));
        assert_eq!(days_after("9999-12-31", -30).as_deref(), Some("9999-12-01"));
        assert_eq!(days_after("someday", 1), None);
    }
}
