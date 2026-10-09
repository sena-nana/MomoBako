//! 时间戳按本机时区显示，和 Vue `new Date(value).toLocaleString("zh-CN")` 同一种写法：
//! `2026/10/8 16:00:00`（年月日不补零，时分秒补零，24 小时制）。
//!
//! 只解析 RFC 3339 / ISO 8601 的 `YYYY-MM-DDTHH:MM:SS[.fff][Z|±HH:MM]`。Windows 上用
//! `SystemTimeToTzSpecificLocalTime` 按当天的时区规则换算；其他平台读不到时区时按 UTC 显示并记日志。

/// 公历日期时间，字段都是本地或 UTC 的墙上时间。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Civil {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

/// 把时间戳写成本机时间。空串或解析不了时返回 `None`。
pub(crate) fn format(raw: &str) -> Option<String> {
    let seconds = parse_epoch_seconds(raw.trim())?;
    let local = to_local(seconds);
    Some(format!(
        "{}/{}/{} {:02}:{:02}:{:02}",
        local.year, local.month, local.day, local.hour, local.minute, local.second
    ))
}

/// 空值写 `fallback`；解析不了的原样显示，不写成「Invalid Date」。
pub(crate) fn format_or(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    format(trimmed).unwrap_or_else(|| {
        eprintln!("Nana 时间戳无法解析，原样显示：{trimmed}");
        trimmed.to_string()
    })
}

/// 解析成 Unix 秒。没有时区后缀的按 UTC。
fn parse_epoch_seconds(raw: &str) -> Option<i64> {
    let (date, rest) = raw.split_once(['T', ' '])?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset_seconds) = split_offset(rest)?;
    let clock = clock.split('.').next()?;
    let mut clock_parts = clock.split(':');
    let hour: u32 = clock_parts.next()?.parse().ok()?;
    let minute: u32 = clock_parts.next()?.parse().ok()?;
    let second: u32 = clock_parts.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second) - offset_seconds)
}

/// 拆出时区后缀：`Z`、`+08:00`、`-0530`。没有后缀时偏移为 0。
fn split_offset(rest: &str) -> Option<(&str, i64)> {
    if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        return Some((clock, 0));
    }
    let Some(index) = rest.rfind(['+', '-']) else {
        return Some((rest, 0));
    };
    let (clock, offset) = rest.split_at(index);
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let digits: String = offset[1..].chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 4 {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    Some((clock, sign * (hours * 3_600 + minutes * 60)))
}

/// 公历日期到 1970-01-01 起的天数（Howard Hinnant 的 days_from_civil）。
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// 天数换回公历日期（civil_from_days），再补上一天里的时分秒。
fn civil_from_seconds(seconds: i64) -> Civil {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    Civil {
        year,
        month,
        day,
        hour: (rest / 3_600) as u32,
        minute: ((rest % 3_600) / 60) as u32,
        second: (rest % 60) as u32,
    }
}

#[cfg(windows)]
fn to_local(seconds: i64) -> Civil {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SystemTimeToTzSpecificLocalTime(zone: *const std::ffi::c_void, universal: *const SystemTime, local: *mut SystemTime) -> i32;
    }
    let utc = civil_from_seconds(seconds);
    let Ok(year) = u16::try_from(utc.year) else {
        eprintln!("Nana 时间戳年份超出系统时间范围，按 UTC 显示：{}", utc.year);
        return utc;
    };
    let universal = SystemTime {
        year,
        month: utc.month as u16,
        day: utc.day as u16,
        hour: utc.hour as u16,
        minute: utc.minute as u16,
        second: utc.second as u16,
        ..SystemTime::default()
    };
    let mut local = SystemTime::default();
    // SAFETY: 两个指针都指向本函数栈上的完整 SYSTEMTIME；时区参数为空表示当前时区。
    let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &universal, &mut local) };
    if ok == 0 {
        eprintln!("Nana 换算本地时间失败，按 UTC 显示：{}", std::io::Error::last_os_error());
        return utc;
    }
    Civil {
        year: i64::from(local.year),
        month: u32::from(local.month),
        day: u32::from(local.day),
        hour: u32::from(local.hour),
        minute: u32::from(local.minute),
        second: u32::from(local.second),
    }
}

#[cfg(not(windows))]
fn to_local(seconds: i64) -> Civil {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| eprintln!("Nana 这个平台读不到本机时区，时间按 UTC 显示"));
    civil_from_seconds(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_offsets_into_the_same_instant() {
        let utc = parse_epoch_seconds("2026-10-08T08:00:00Z").expect("UTC");
        assert_eq!(parse_epoch_seconds("2026-10-08T16:00:00+08:00"), Some(utc));
        assert_eq!(parse_epoch_seconds("2026-10-08T08:00:00.250Z"), Some(utc));
        assert_eq!(parse_epoch_seconds("2026-10-08T02:30:00-0530"), Some(utc));
        assert_eq!(utc, 1_791_446_400);
    }

    #[test]
    fn civil_round_trip_holds_across_eras_and_leap_days() {
        for (year, month, day) in [(1970, 1, 1), (2000, 2, 29), (2026, 10, 8), (1969, 12, 31), (2400, 3, 1)] {
            let seconds = days_from_civil(year, month, day) * 86_400 + 3_661;
            assert_eq!(civil_from_seconds(seconds), Civil { year, month, day, hour: 1, minute: 1, second: 1 });
        }
    }

    #[test]
    fn rejects_text_that_is_not_a_timestamp() {
        assert_eq!(parse_epoch_seconds("昨天"), None);
        assert_eq!(parse_epoch_seconds("2026-13-01T00:00:00Z"), None);
        assert_eq!(format_or("", "未记录"), "未记录");
        assert_eq!(format_or("昨天", "未记录"), "昨天");
    }

    #[test]
    fn formats_like_zh_cn_locale_strings() {
        let text = format("2026-10-08T08:00:00Z").expect("格式化");
        let (date, clock) = text.split_once(' ').expect("日期和时间");
        assert_eq!(date.split('/').count(), 3);
        assert!(!date.contains("/0"), "月日不补零：{text}");
        assert_eq!(clock.len(), 8, "时分秒补零：{text}");
    }
}
