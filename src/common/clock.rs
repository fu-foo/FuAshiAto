//! 時刻の取得と整形。時刻はすべてローカル時刻（`DateTime<Local>`）で扱う。

use chrono::{DateTime, Local, NaiveDate, TimeDelta, TimeZone};

pub type Time = DateTime<Local>;

pub fn now() -> Time {
    Local::now()
}

/// RFC 3339、ミリ秒、UTCオフセット付き（例：`2026-09-28T09:12:03.412+09:00`）
pub fn fmt(t: &Time) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string()
}

pub fn parse(s: &str) -> Option<Time> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Local))
}

/// その日の 00:00:00.000（ローカル時刻）
pub fn start_of_day(d: NaiveDate) -> Time {
    let naive = d.and_hms_opt(0, 0, 0).expect("valid midnight");
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t,
        chrono::LocalResult::Ambiguous(t, _) => t,
        // 0時が存在しない（DST）地域向けのフォールバック
        chrono::LocalResult::None => Local.from_utc_datetime(&naive),
    }
}

/// その日の 23:59:59.999
pub fn end_of_day(d: NaiveDate) -> Time {
    let next = d.succ_opt().unwrap_or(d);
    start_of_day(next) - TimeDelta::milliseconds(1)
}

pub fn ms(n: i64) -> TimeDelta {
    TimeDelta::milliseconds(n)
}

/// 現在のUTCオフセット（例：`+09:00`）
pub fn tz_offset() -> String {
    now().format("%:z").to_string()
}

/// `5h42m` / `18m` 形式
pub fn fmt_duration(d: TimeDelta) -> String {
    let mins = d.num_minutes().max(0);
    let (h, m) = (mins / 60, mins % 60);
    if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

/// `30m`、`2h`、`1h30m` を分数に変換する
pub fn parse_minutes(s: &str) -> Option<u32> {
    let mut total: u64 = 0;
    let mut num = String::new();
    let mut any = false;
    for c in s.trim().chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: u64 = num.parse().ok()?;
            num.clear();
            match c.to_ascii_lowercase() {
                'h' => total += n * 60,
                'm' => total += n,
                _ => return None,
            }
            any = true;
        }
    }
    if !num.is_empty() || !any || total == 0 || total > u32::MAX as u64 {
        return None;
    }
    Some(total as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes() {
        assert_eq!(parse_minutes("30m"), Some(30));
        assert_eq!(parse_minutes("2h"), Some(120));
        assert_eq!(parse_minutes("1h30m"), Some(90));
        assert_eq!(parse_minutes("30"), None);
        assert_eq!(parse_minutes("0m"), None);
        assert_eq!(parse_minutes("abc"), None);
        assert_eq!(parse_minutes(""), None);
    }

    #[test]
    fn duration() {
        assert_eq!(fmt_duration(TimeDelta::minutes(342)), "5h42m");
        assert_eq!(fmt_duration(TimeDelta::minutes(18)), "18m");
        assert_eq!(fmt_duration(TimeDelta::minutes(61)), "1h01m");
    }

    #[test]
    fn roundtrip() {
        let t = Local.with_ymd_and_hms(2026, 9, 28, 9, 12, 3).unwrap() + ms(412);
        let s = fmt(&t);
        assert!(s.starts_with("2026-09-28T09:12:03.412"));
        assert_eq!(parse(&s), Some(t));
    }
}
