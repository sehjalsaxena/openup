//! Tiny append-only log (`openup.log`) so you can see what the background daemon did
//! without attaching a debugger. Never write credentials here.

use std::fs;
use std::io::Write;
use std::path::Path;

pub const LOG_FILE: &str = "openup.log";
const MAX_BYTES: u64 = 256 * 1024;

/// Appends a timestamped line; rotates to `openup.log.1` past 256 KiB. Failures are
/// ignored: logging must never break the daemon.
pub fn log(config_dir: &Path, msg: &str) {
    let path = config_dir.join(LOG_FILE);
    let _ = fs::create_dir_all(config_dir);

    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(&path, config_dir.join(format!("{LOG_FILE}.1")));
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{}] {}", format_local(now), msg.replace('\n', " "));
    }
}

/// Last `n` log lines (oldest first).
pub fn tail(config_dir: &Path, n: usize) -> Vec<String> {
    let content = fs::read_to_string(config_dir.join(LOG_FILE)).unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].iter().map(|s| s.to_string()).collect()
}

/// `YYYY-MM-DD HH:MM:SS` in the system local timezone.
pub fn format_local(epoch_secs: u64) -> String {
    let (y, m, d, hh, mm, ss) = local_parts(epoch_secs);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
}

/// `HH:MM:SS` in the system local timezone.
pub fn format_local_time(epoch_secs: u64) -> String {
    let (_, _, _, hh, mm, ss) = local_parts(epoch_secs);
    format!("{hh:02}:{mm:02}:{ss:02}")
}

#[cfg(target_os = "windows")]
fn local_parts(epoch_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        w_year: u16,
        w_month: u16,
        w_day_of_week: u16,
        w_day: u16,
        w_hour: u16,
        w_minute: u16,
        w_second: u16,
        w_milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToSystemTime(lpFileTime: *const u64, lpSystemTime: *mut SystemTime) -> i32;
        fn SystemTimeToTzSpecificLocalTime(
            lpTimeZoneInformation: *const std::ffi::c_void,
            lpUniversalTime: *const SystemTime,
            lpLocalTime: *mut SystemTime,
        ) -> i32;
    }
    unsafe {
        let ft: u64 = epoch_secs
            .saturating_mul(10_000_000)
            .saturating_add(116_444_736_000_000_000);
        let mut utc_st: SystemTime = std::mem::zeroed();
        let mut local_st: SystemTime = std::mem::zeroed();
        if FileTimeToSystemTime(&ft, &mut utc_st) != 0
            && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc_st, &mut local_st) != 0
        {
            return (
                local_st.w_year as i32,
                local_st.w_month as u32,
                local_st.w_day as u32,
                local_st.w_hour as u32,
                local_st.w_minute as u32,
                local_st.w_second as u32,
            );
        }
    }
    civil_utc_parts(epoch_secs)
}

#[cfg(unix)]
fn local_parts(epoch_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
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
        _extra: [u8; 64],
    }
    unsafe extern "C" {
        fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    }
    unsafe {
        let t = epoch_secs.min(i64::MAX as u64) as i64;
        let mut tm: Tm = std::mem::zeroed();
        if !localtime_r(&t, &mut tm).is_null() {
            return (
                tm.tm_year + 1900,
                (tm.tm_mon + 1).max(1) as u32,
                tm.tm_mday.max(1) as u32,
                tm.tm_hour.max(0) as u32,
                tm.tm_min.max(0) as u32,
                tm.tm_sec.max(0) as u32,
            );
        }
    }
    civil_utc_parts(epoch_secs)
}

#[cfg(not(any(target_os = "windows", unix)))]
fn local_parts(epoch_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    civil_utc_parts(epoch_secs)
}

fn civil_utc_parts(epoch_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (epoch_secs / 86_400) as i64;
    let rem = epoch_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    (
        y as i32,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Parses `YYYY-MM-DD HH:MM:SSZ` (UTC) into epoch seconds.
pub fn parse_utc_timestamp(ts: &str) -> Option<u64> {
    let clean = ts.strip_suffix('Z')?;
    let (date_str, time_str) = clean.split_once(' ')?;
    let mut date_parts = date_str.split('-');
    let y: i64 = date_parts.next()?.parse().ok()?;
    let m: u32 = date_parts.next()?.parse().ok()?;
    let d: u32 = date_parts.next()?.parse().ok()?;

    let mut time_parts = time_str.split(':');
    let hh: u64 = time_parts.next()?.parse().ok()?;
    let mm: u64 = time_parts.next()?.parse().ok()?;
    let ss: u64 = time_parts.next()?.parse().ok()?;

    let days = days_from_civil(y, m, d)?;
    if days < 0 {
        return None;
    }
    let epoch_secs = (days as u64) * 86_400 + hh * 3600 + mm * 60 + ss;
    Some(epoch_secs)
}

/// Howard Hinnant's civil date -> days-since-epoch inverse algorithm.
fn days_from_civil(y: i64, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + (d as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// `YYYY-MM-DD HH:MM:SSZ` (UTC) without pulling in a date/time crate.
pub fn format_utc(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let rem = epoch_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's days-since-epoch -> civil date algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_timestamps() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00Z");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14 22:13:20Z");
        assert_eq!(format_utc(1_709_164_800), "2024-02-29 00:00:00Z");
    }

    #[test]
    fn parses_utc_timestamps_back_to_epoch() {
        assert_eq!(parse_utc_timestamp("1970-01-01 00:00:00Z"), Some(0));
        assert_eq!(
            parse_utc_timestamp("2023-11-14 22:13:20Z"),
            Some(1_700_000_000)
        );
        assert_eq!(
            parse_utc_timestamp("2024-02-29 00:00:00Z"),
            Some(1_709_164_800)
        );
        assert_eq!(parse_utc_timestamp("invalid"), None);
    }

    #[test]
    fn formats_local_timestamps() {
        let local_str = format_local(1_700_000_000);
        assert_eq!(local_str.len(), 19);
        assert!(local_str.contains('-') && local_str.contains(':'));

        let local_time = format_local_time(1_700_000_000);
        assert_eq!(local_time.len(), 8);
        assert_eq!(&local_str[11..19], &local_time);
    }

    #[test]
    fn log_tail_and_rotation() {
        let dir = std::env::temp_dir().join(format!("openup_log_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        log(&dir, "first");
        log(&dir, "multi\nline");
        log(&dir, "third");
        let lines = tail(&dir, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("multi line"));
        assert!(lines[1].ends_with("third"));

        fs::write(dir.join(LOG_FILE), vec![b'x'; (MAX_BYTES + 10) as usize]).unwrap();
        log(&dir, "after rotate");
        assert!(dir.join("openup.log.1").exists());
        assert_eq!(tail(&dir, 10).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
