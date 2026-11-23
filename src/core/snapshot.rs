//! Snapshots of a scope taken before every write.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::model::{Scope, Var};
use super::records::{self, Record};

/// Number of snapshot files kept across both scopes.
pub const KEEP: usize = 50;

/// `%LOCALAPPDATA%\envedit`, the base of snapshots and helper files.
pub fn app_data_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("envedit"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_string())
}

pub fn snapshot_dir() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("snapshots"))
}

/// Encodes all variables of a scope as snapshot records.
pub fn encode(scope: Scope, vars: &[Var]) -> String {
    let records: Vec<Record> = vars
        .iter()
        .map(|v| Record::new("var", [scope.token(), &v.name, &v.ty.token(), &v.value]))
        .collect();
    records::encode(&records)
}

/// Writes a snapshot of a scope into `dir` and prunes old snapshots.
pub fn write(dir: &Path, scope: Scope, vars: &[Var], now: SystemTime) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}-{}.txt", timestamp(now), scope.token()));
    fs::write(&path, encode(scope, vars)).map_err(|e| format!("write {}: {e}", path.display()))?;
    prune(dir, KEEP);
    Ok(path)
}

/// Deletes all but the newest `keep` snapshot files. Errors are ignored:
/// a leftover old snapshot is harmless.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| is_snapshot_name(name))
        .collect();
    // Names start with a UTC timestamp, so lexical order is age order.
    names.sort();
    let excess = names.len().saturating_sub(keep);
    for name in &names[..excess] {
        let _ = fs::remove_file(dir.join(name));
    }
}

fn is_snapshot_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".txt") else { return false };
    let Some((stamp, scope)) = stem.rsplit_once('-') else { return false };
    Scope::from_token(scope).is_some()
        && stamp.len() == 19
        && stamp.chars().enumerate().all(|(i, c)| if i == 8 || i == 15 { c == '-' } else { c.is_ascii_digit() })
}

/// `yyyyMMdd-HHmmss-fff` in UTC. UTC keeps the order stable across DST changes.
pub fn timestamp(now: SystemTime) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{:03}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        since.subsec_millis()
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::ValueType;
    use std::time::Duration;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("envedit-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn at(secs: u64, millis: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(millis)
    }

    #[test]
    fn timestamp_format() {
        // 2026-11-23 08:18:47.123 UTC
        assert_eq!(timestamp(at(1_795_421_927, 123)), "20261123-081847-123");
        assert_eq!(timestamp(at(0, 0)), "19700101-000000-000");
        // Leap day.
        assert_eq!(timestamp(at(951_782_400, 5)), "20000229-000000-005");
    }

    #[test]
    fn writes_content_and_name() {
        let tmp = TempDir::new("content");
        let vars = vec![Var::new("Path", r"C:\a;C:\b", ValueType::Sz), Var::new("X", "%Y%", ValueType::ExpandSz)];
        let path = write(&tmp.0, Scope::User, &vars, at(1_795_421_927, 123)).unwrap();
        assert_eq!(path.file_name().unwrap(), "20261123-081847-123-user.txt");
        let decoded = records::decode(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            decoded,
            vec![
                Record::new("var", ["user", "Path", "REG_SZ", r"C:\a;C:\b"]),
                Record::new("var", ["user", "X", "REG_EXPAND_SZ", "%Y%"]),
            ]
        );
    }

    #[test]
    fn keeps_newest_fifty() {
        let tmp = TempDir::new("retention");
        fs::create_dir_all(&tmp.0).unwrap();
        fs::write(tmp.0.join("unrelated.txt"), "keep me").unwrap();
        for i in 0..51 {
            write(&tmp.0, Scope::Machine, &[], at(1_000_000 + i, 0)).unwrap();
        }
        let mut names: Vec<_> = fs::read_dir(&tmp.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names.len(), 51, "50 snapshots plus the unrelated file");
        assert!(names.contains(&"unrelated.txt".to_string()));
        assert!(!names.contains(&format!("{}-machine.txt", timestamp(at(1_000_000, 0)))));
        assert!(names.contains(&format!("{}-machine.txt", timestamp(at(1_000_050, 0)))));
    }

    #[test]
    fn write_failure_is_reported() {
        let tmp = TempDir::new("failure");
        fs::create_dir_all(tmp.0.parent().unwrap()).unwrap();
        // A file where the directory should be makes the write fail.
        fs::write(&tmp.0, "not a directory").unwrap();
        assert!(write(&tmp.0, Scope::User, &[], SystemTime::now()).is_err());
        fs::remove_file(&tmp.0).unwrap();
    }
}
