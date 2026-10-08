//! 把仓库记录的 ISO 时间写成 Vue `new Date(…).toLocaleString("zh-CN")` 的样子：
//! 本地时区、`年/月/日 时:分:秒`，月和日不补零，时分秒补零。
//!
//! 只认 `YYYY-MM-DDTHH:MM:SS`，可带小数秒和 `Z` / `±HH:MM` 时区。没有时区时和浏览器一样按本地时间理解。
//! 认不出的写法原样返回，不编造时间。

/// 本地时区下的年月日时分秒。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Parts {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

/// `2026-10-08T08:00:00Z` → 东八区的 `2026/10/8 16:00:00`。
pub(super) fn zh_cn_local(iso: &str) -> String {
    let text = iso.trim();
    match parse(text) {
        Some(parts) => format!(
            "{}/{}/{} {:02}:{:02}:{:02}",
            parts.year, parts.month, parts.day, parts.hour, parts.minute, parts.second
        ),
        None => text.to_string(),
    }
}

/// 拆出日期、时间和时区。带时区的换算到 Unix 秒再转本地；不带时区的就是本地时间本身。
fn parse(text: &str) -> Option<Parts> {
    let (date, rest) = text.split_once('T').or_else(|| text.split_once(' '))?;
    let mut fields = date.split('-');
    let year: i64 = fields.next()?.parse().ok()?;
    let month: u32 = fields.next()?.parse().ok()?;
    let day: u32 = fields.next()?.parse().ok()?;
    if fields.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset) = split_offset(rest)?;
    let mut fields = clock.split(':');
    let hour: u32 = fields.next()?.parse().ok()?;
    let minute: u32 = fields.next()?.parse().ok()?;
    let second: u32 = fields.next().map(|value| value.split('.').next().unwrap_or("0")).unwrap_or("0").parse().ok()?;
    if fields.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let written = Parts { year, month, day, hour, minute, second };
    let Some(offset) = offset else {
        return Some(written);
    };
    let seconds = days_from_civil(year, month, day) * 86_400
        + i64::from(hour) * 3_600
        + i64::from(minute) * 60
        + i64::from(second)
        - offset;
    Some(local_parts(seconds).unwrap_or_else(|| utc_parts(seconds)))
}

/// 时间后面的时区：`Z` 是 0，`+08:00` 是 28800 秒；没有时区返回 `None` 偏移。
fn split_offset(rest: &str) -> Option<(&str, Option<i64>)> {
    if let Some(clock) = rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
        return Some((clock, Some(0)));
    }
    let Some(index) = rest.rfind(['+', '-']) else {
        return Some((rest, None));
    };
    let (clock, zone) = rest.split_at(index);
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let zone = &zone[1..];
    let (hours, minutes) = zone.split_once(':').unwrap_or((zone.get(..2)?, zone.get(2..).unwrap_or("0")));
    let hours: i64 = hours.parse().ok()?;
    let minutes: i64 = if minutes.is_empty() { 0 } else { minutes.parse().ok()? };
    Some((clock, Some(sign * (hours * 3_600 + minutes * 60))))
}

/// 算法：Howard Hinnant 的 days_from_civil。以 3 月为年首，把闰日放到年尾，
/// 按 400 年一个纪元（146097 天）折算出 1970-01-01 起的天数。
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// days_from_civil 的逆运算，取不到本地时区时按 UTC 显示。
fn utc_parts(seconds: i64) -> Parts {
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
    Parts {
        year,
        month,
        day,
        hour: (rest / 3_600) as u32,
        minute: (rest % 3_600 / 60) as u32,
        second: (rest % 60) as u32,
    }
}

/// Windows 用 C 运行库的 `_localtime64_s` 按系统时区（含夏令时）换算，和浏览器用的是同一个时区。
#[cfg(windows)]
fn local_parts(seconds: i64) -> Option<Parts> {
    #[repr(C)]
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
    }
    unsafe extern "C" {
        fn _localtime64_s(tm: *mut Tm, time: *const i64) -> i32;
    }
    let mut tm = Tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0 };
    // SAFETY：两个指针都指向本函数里有效、对齐的局部变量；CRT 只写 `tm`，不保留指针。
    let code = unsafe { _localtime64_s(&mut tm, &seconds) };
    if code != 0 {
        eprintln!("Nana 本地时区换算失败：{seconds}，错误码 {code}");
        return None;
    }
    Some(Parts {
        year: i64::from(tm.tm_year) + 1900,
        month: u32::try_from(tm.tm_mon + 1).ok()?,
        day: u32::try_from(tm.tm_mday).ok()?,
        hour: u32::try_from(tm.tm_hour).ok()?,
        minute: u32::try_from(tm.tm_min).ok()?,
        second: u32::try_from(tm.tm_sec).ok()?,
    })
}

#[cfg(not(windows))]
fn local_parts(_seconds: i64) -> Option<Parts> {
    None
}

#[cfg(test)]
mod tests {
    use super::{days_from_civil, parse, utc_parts, zh_cn_local, Parts};

    #[test]
    fn civil_days_round_trip_across_leap_years_and_the_epoch() {
        for (year, month, day) in [(1970, 1, 1), (2000, 2, 29), (2026, 10, 8), (1969, 12, 31), (2100, 3, 1)] {
            let seconds = days_from_civil(year, month, day) * 86_400 + 3_661;
            assert_eq!(utc_parts(seconds), Parts { year, month, day, hour: 1, minute: 1, second: 1 });
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn local_times_without_a_zone_keep_their_digits_like_the_browser() {
        assert_eq!(zh_cn_local("2026-10-08T08:00:00"), "2026/10/8 08:00:00");
        assert_eq!(zh_cn_local("2026-01-05 07:03:09.250"), "2026/1/5 07:03:09");
    }

    #[test]
    fn zoned_times_convert_through_the_same_instant() {
        let utc = parse("2026-10-08T08:00:00Z").expect("UTC");
        let east = parse("2026-10-08T16:00:00+08:00").expect("东八区");
        assert_eq!(utc, east);
        let text = zh_cn_local("2026-10-08T08:00:00Z");
        let (date, clock) = text.split_once(' ').expect("日期和时间");
        assert_eq!(date.split('/').count(), 3);
        assert_eq!(clock.len(), 8);
    }

    #[test]
    fn unreadable_text_is_returned_as_is() {
        assert_eq!(zh_cn_local("昨天"), "昨天");
        assert_eq!(zh_cn_local("2026-13-01T00:00:00Z"), "2026-13-01T00:00:00Z");
        assert_eq!(zh_cn_local(""), "");
    }
}
