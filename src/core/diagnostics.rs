//! Passive markers: length, missing directories, duplicates, expansion.
//! Nothing here changes a value.

use std::collections::HashMap;
use std::path::Path;

use super::model::{LEGACY_DIALOG_LIMIT, VALUE_LIMIT, ValueType, utf16_len};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthInfo {
    pub len: usize,
    pub limit: usize,
    /// Longer than the built-in Windows dialog can edit.
    pub over_legacy: bool,
}

pub fn length_info(value: &str) -> LengthInfo {
    let len = utf16_len(value);
    LengthInfo { len, limit: VALUE_LIMIT, over_legacy: len > LEGACY_DIALOG_LIMIT }
}

/// Whether Windows will leave `%VAR%` references in this value unexpanded.
pub fn reg_sz_warning(ty: ValueType, value: &str) -> bool {
    ty == ValueType::Sz && value.contains('%')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// Absolute drive-letter path: checked for existence.
    Local,
    /// `\\server\share`: never checked (can hang).
    Unc,
    /// Anything else, e.g. `.COM` in `PATHEXT`.
    NotAPath,
}

/// Classifies an already expanded entry.
pub fn classify(expanded: &str) -> PathKind {
    let bytes = expanded.as_bytes();
    if expanded.starts_with(r"\\") {
        PathKind::Unc
    } else if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        PathKind::Local
    } else {
        PathKind::NotAPath
    }
}

/// Comparison key for duplicates: case-insensitive, trailing `\` ignored.
pub fn dup_key(expanded: &str) -> String {
    expanded.trim_end_matches('\\').to_lowercase()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DupFlags {
    pub same_scope: bool,
    pub cross_scope: bool,
}

/// Duplicate flags for each entry of `entries` (expanded), compared with
/// each other and with `other_scope` (expanded entries of the variable with
/// the same name in the other scope, if it exists).
pub fn duplicate_flags(entries: &[String], other_scope: &[String]) -> Vec<DupFlags> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        *counts.entry(dup_key(entry)).or_default() += 1;
    }
    let other: Vec<String> = other_scope.iter().map(|e| dup_key(e)).collect();
    entries
        .iter()
        .map(|entry| {
            let key = dup_key(entry);
            DupFlags { same_scope: counts[&key] > 1, cross_scope: other.contains(&key) }
        })
        .collect()
}

/// Session cache of directory-existence results, keyed by expanded path.
#[derive(Debug, Default)]
pub struct ExistenceCache {
    known: HashMap<String, bool>,
}

impl ExistenceCache {
    pub fn get(&self, expanded: &str) -> Option<bool> {
        self.known.get(&dup_key(expanded)).copied()
    }

    pub fn insert(&mut self, expanded: &str, exists: bool) {
        self.known.insert(dup_key(expanded), exists);
    }

    /// Local paths among `expanded` that are not cached yet.
    pub fn unknown<'a>(&self, expanded: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for path in expanded {
            if classify(path) == PathKind::Local && self.get(path).is_none() && !out.contains(path) {
                out.push(path.clone());
            }
        }
        out
    }
}

/// Checks whether each path exists as a directory. Can be slow; run on a worker.
pub fn check_dirs(paths: Vec<String>) -> Vec<(String, bool)> {
    paths
        .into_iter()
        .map(|p| {
            let exists = Path::new(&p).is_dir();
            (p, exists)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Present,
    Missing,
    /// UNC path, not checked.
    Unchecked,
    /// Check still running.
    Pending,
    NotAPath,
}

pub fn presence(expanded: &str, cache: &ExistenceCache) -> Presence {
    match classify(expanded) {
        PathKind::NotAPath => Presence::NotAPath,
        PathKind::Unc => Presence::Unchecked,
        PathKind::Local => match cache.get(expanded) {
            Some(true) => Presence::Present,
            Some(false) => Presence::Missing,
            None => Presence::Pending,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn live_length_after_adding_entry() {
        let path = "x".repeat(2469);
        let entry = "y".repeat(40);
        let info = length_info(&format!("{path};{entry}"));
        assert_eq!(info.len, 2510);
        assert!(info.over_legacy);
        assert_eq!(info.limit, 32_767);
    }

    #[test]
    fn legacy_limit_boundary() {
        assert!(!length_info(&"x".repeat(2047)).over_legacy);
        assert!(length_info(&"x".repeat(2048)).over_legacy);
    }

    #[test]
    fn classification() {
        assert_eq!(classify(r"C:\Users\me\go\bin"), PathKind::Local);
        assert_eq!(classify(r"\\server\share\bin"), PathKind::Unc);
        assert_eq!(classify(".COM"), PathKind::NotAPath);
        assert_eq!(classify(r"relative\dir"), PathKind::NotAPath);
        assert_eq!(classify("C:"), PathKind::NotAPath);
    }

    #[test]
    fn same_scope_duplicates() {
        let flags = duplicate_flags(&s(&[r"C:\Tools", r"C:\Other", r"c:\tools\"]), &[]);
        assert_eq!(flags.iter().map(|f| f.same_scope).collect::<Vec<_>>(), vec![true, false, true]);
        assert!(flags.iter().all(|f| !f.cross_scope));
    }

    #[test]
    fn cross_scope_duplicates() {
        let git = r"C:\Program Files\Git\cmd";
        let user = s(&[git, r"C:\mine"]);
        let machine = s(&[r"C:\Windows", git]);
        let user_flags = duplicate_flags(&user, &machine);
        let machine_flags = duplicate_flags(&machine, &user);
        assert!(user_flags[0].cross_scope && !user_flags[1].cross_scope);
        assert!(machine_flags[1].cross_scope && !machine_flags[0].cross_scope);
        assert!(user_flags.iter().all(|f| !f.same_scope));
    }

    #[test]
    fn reg_sz_percent_warning() {
        assert!(reg_sz_warning(ValueType::Sz, r"%USERPROFILE%\bin"));
        assert!(!reg_sz_warning(ValueType::ExpandSz, r"%USERPROFILE%\bin"));
        assert!(!reg_sz_warning(ValueType::Sz, r"C:\bin"));
    }

    #[test]
    fn existence_check_and_cache() {
        let existing = std::env::temp_dir().to_string_lossy().into_owned();
        let missing = format!(r"{existing}\envedit-definitely-missing-{}", std::process::id());
        let mut cache = ExistenceCache::default();
        let candidates = s(&[&existing, &missing, ".COM", r"\\server\share"]);
        let unknown = cache.unknown(&candidates);
        assert_eq!(unknown, s(&[&existing, &missing]));
        for (path, exists) in check_dirs(unknown) {
            cache.insert(&path, exists);
        }
        assert_eq!(presence(&existing, &cache), Presence::Present);
        assert_eq!(presence(&missing, &cache), Presence::Missing);
        assert_eq!(presence(".COM", &cache), Presence::NotAPath);
        assert_eq!(presence(r"\\server\share", &cache), Presence::Unchecked);
        assert_eq!(presence(r"Z:\not-checked-yet", &cache), Presence::Pending);
        assert!(cache.unknown(&candidates).is_empty());
    }
}
