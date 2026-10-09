//! 日志和插件执行记录的时间显示。
//!
//! Vue 用 `Intl.DateTimeFormat("zh-CN")` 按本机时区显示 `MM/DD HH:mm:ss`。这里把 RFC 3339 时间
//! 解析成 UTC 秒，再用 C 运行库的 `localtime` 取本机时区偏移（含夏令时），格式和 Vue 一致。
//! 解析不了的时间原样返回，和 Vue 对无效日期的处理相同。

use std::time::{SystemTime, UNIX_EPOCH};

/// 日志卡片右上角的时间：`MM/DD HH:mm:ss`，本机时区。
pub fn log_time_label(timestamp: &str) -> String {
    match local_parts(timestamp) {
        Some(parts) => format!("{:02}/{:02} {:02}:{:02}:{:02}", parts.month, parts.day, parts.hour, parts.minute, parts.second),
        None => timestamp.to_string(),
    }
}

/// 插件执行记录的时间：`MM/DD HH:mm`，本机时区。
pub fn hook_time_label(timestamp: &str) -> String {
    match local_parts(timestamp) {
        Some(parts) => format!("{:02}/{:02} {:02}:{:02}", parts.month, parts.day, parts.hour, parts.minute),
        None => timestamp.to_string(),
    }
}

/// 当前 UTC 时间，`toISOString()` 的格式（毫秒精度）。
pub fn now_iso8601() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let parts = civil_from_epoch(seconds);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        parts.year,
        parts.month,
        parts.day,
        parts.hour,
        parts.minute,
        parts.second,
        now.subsec_millis()
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Parts {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

fn local_parts(timestamp: &str) -> Option<Parts> {
    let utc = parse_rfc3339(timestamp)?;
    let offset = local_offset_seconds(utc).unwrap_or_else(|| {
        eprintln!("Nana 读不到本机时区，按 UTC 显示时间");
        0
    });
    Some(civil_from_epoch(utc + offset))
}

/// 解析 `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`，返回 UTC 秒。
pub(crate) fn parse_rfc3339(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.len() < 19 {
        return None;
    }
    let bytes = text.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || !(bytes[10] == b'T' || bytes[10] == b't' || bytes[10] == b' ') || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(stripped) = rest.strip_prefix('.') {
        let digits = stripped.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(stripped.len());
        rest = &stripped[digits..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let body = &rest[1..];
            let (hours, minutes) = body.split_once(':').unwrap_or((body.get(0..2)?, body.get(2..4).unwrap_or("0")));
            sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60)
        }
    };
    Some(days_from_civil(year, month as u32, day as u32) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// 公历日期到 1970-01-01 起的天数（Howard Hinnant 的算法）。
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 1970-01-01 起的秒数到公历日期和时刻。
fn civil_from_epoch(seconds: i64) -> Parts {
    let days = seconds.div_euclid(86_400);
    let secs = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Parts { year, month, day, hour: (secs / 3600) as u32, minute: ((secs % 3600) / 60) as u32, second: (secs % 60) as u32 }
}

/// C 运行库 `struct tm`。只读前六个字段。
#[repr(C)]
#[derive(Default)]
struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    /// glibc 和 BSD 在 `tm_isdst` 后还有 `tm_gmtoff`、`tm_zone`，这里留足空间。
    #[cfg(not(windows))]
    tm_gmtoff: i64,
    #[cfg(not(windows))]
    tm_zone: usize,
}

#[cfg(windows)]
unsafe extern "C" {
    fn _localtime64_s(tm: *mut Tm, time: *const i64) -> i32;
}

#[cfg(not(windows))]
unsafe extern "C" {
    fn localtime_r(time: *const i64, tm: *mut Tm) -> *mut Tm;
}

/// 某个 UTC 时刻的本机时区偏移（秒）。拿不到时返回 `None`。
fn local_offset_seconds(utc: i64) -> Option<i64> {
    let mut tm = Tm::default();
    #[cfg(windows)]
    // SAFETY: `tm` 和 `utc` 都是本函数里的有效内存，函数只写 `tm`。
    let ok = unsafe { _localtime64_s(&mut tm, &utc) } == 0;
    #[cfg(not(windows))]
    // SAFETY: 同上；`localtime_r` 是线程安全版本，只写 `tm`。
    let ok = unsafe { !localtime_r(&utc, &mut tm).is_null() };
    if !ok {
        return None;
    }
    let local = days_from_civil(i64::from(tm.tm_year) + 1900, (tm.tm_mon + 1) as u32, tm.tm_mday as u32) * 86_400
        + i64::from(tm.tm_hour) * 3600
        + i64::from(tm.tm_min) * 60
        + i64::from(tm.tm_sec);
    Some(local - utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_and_offsets() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-10-08T07:51:00Z"), Some(1_791_445_860));
        assert_eq!(parse_rfc3339("2026-10-08T15:51:00+08:00"), Some(1_791_445_860));
        assert_eq!(parse_rfc3339("2026-10-08T07:51:00.250Z"), Some(1_791_445_860));
        assert_eq!(parse_rfc3339("昨天"), None);
    }

    #[test]
    fn civil_round_trips() {
        let parts = civil_from_epoch(1_791_445_860);
        assert_eq!((parts.year, parts.month, parts.day, parts.hour, parts.minute), (2026, 10, 8, 7, 51));
    }

    #[test]
    fn invalid_time_is_shown_as_is() {
        assert_eq!(log_time_label("not a time"), "not a time");
    }
}
