//! Plain timestamped log lines on stdout (what `docker logs` and journald expect).
//! `FLUNKI_LOG=debug` also logs every throw, hit and foul.

use std::time::{SystemTime, UNIX_EPOCH};

fn stamp() -> String {
    let s = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
    // days since 1970 → calendar date (Howard Hinnant's civil_from_days)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

pub fn debug_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("FLUNKI_LOG").map_or(false, |v| v == "debug"))
}
pub fn line(level: &str, msg: &str) {
    println!("{} {level:<5} {msg}", stamp());
}
#[macro_export]
macro_rules! info { ($($a:tt)*) => { $crate::log::line("INFO", &format!($($a)*)) } }
#[macro_export]
macro_rules! warn { ($($a:tt)*) => { $crate::log::line("WARN", &format!($($a)*)) } }
#[macro_export]
macro_rules! debug { ($($a:tt)*) => { if $crate::log::debug_on() { $crate::log::line("DEBUG", &format!($($a)*)) } } }

#[cfg(test)]
mod tests {
    #[test]
    fn stamp_looks_like_a_date() {
        let s = super::stamp();
        assert_eq!(s.len(), 20, "{s}");
        assert!(s.starts_with("20") && s.ends_with('Z'), "{s}");
    }
}
