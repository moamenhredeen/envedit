//! Basic types shared by all core modules.

use std::fmt;

/// Maximum length of an environment variable value, in UTF-16 code units.
pub const VALUE_LIMIT: usize = 32_767;

/// Length the built-in Windows dialog refuses to edit beyond.
pub const LEGACY_DIALOG_LIMIT: usize = 2047;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    User,
    Machine,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::User => "User",
            Scope::Machine => "Machine",
        }
    }

    /// Lowercase token used in record files.
    pub fn token(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Machine => "machine",
        }
    }

    pub fn from_token(s: &str) -> Option<Scope> {
        match s {
            "user" => Some(Scope::User),
            "machine" => Some(Scope::Machine),
            _ => None,
        }
    }
}

/// Registry value type of a variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Sz,
    ExpandSz,
    /// Any other registry type. Shown, never edited.
    Other(u32),
}

impl ValueType {
    pub const REG_SZ: u32 = 1;
    pub const REG_EXPAND_SZ: u32 = 2;

    pub fn from_raw(raw: u32) -> ValueType {
        match raw {
            Self::REG_SZ => ValueType::Sz,
            Self::REG_EXPAND_SZ => ValueType::ExpandSz,
            other => ValueType::Other(other),
        }
    }

    pub fn raw(self) -> u32 {
        match self {
            ValueType::Sz => Self::REG_SZ,
            ValueType::ExpandSz => Self::REG_EXPAND_SZ,
            ValueType::Other(raw) => raw,
        }
    }

    pub fn is_editable(self) -> bool {
        !matches!(self, ValueType::Other(_))
    }

    /// Token used in record files and shown in the UI.
    pub fn token(self) -> String {
        match self {
            ValueType::Sz => "REG_SZ".to_string(),
            ValueType::ExpandSz => "REG_EXPAND_SZ".to_string(),
            ValueType::Other(raw) => format!("TYPE:{raw}"),
        }
    }

    pub fn from_token(s: &str) -> Option<ValueType> {
        match s {
            "REG_SZ" => Some(ValueType::Sz),
            "REG_EXPAND_SZ" => Some(ValueType::ExpandSz),
            _ => s.strip_prefix("TYPE:")?.parse().ok().map(ValueType::Other),
        }
    }

    /// Type a newly created variable gets when the user did not pick one.
    pub fn default_for(value: &str) -> ValueType {
        if value.contains('%') {
            ValueType::ExpandSz
        } else {
            ValueType::Sz
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.token())
    }
}

/// A persistent environment variable as stored in the registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    /// Raw, unexpanded value.
    pub value: String,
    pub ty: ValueType,
}

#[cfg(test)]
impl Var {
    pub fn new(name: &str, value: &str, ty: ValueType) -> Var {
        Var {
            name: name.to_string(),
            value: value.to_string(),
            ty,
        }
    }
}

/// Identifies a variable. Names compare case-insensitively, like Windows does.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    pub scope: Scope,
    folded: String,
}

impl Key {
    pub fn new(scope: Scope, name: &str) -> Key {
        Key {
            scope,
            folded: fold_name(name),
        }
    }
}

pub fn fold_name(name: &str) -> String {
    name.to_uppercase()
}

pub fn names_equal(a: &str, b: &str) -> bool {
    fold_name(a) == fold_name(b)
}

/// Length in UTF-16 code units, the unit of the Windows value limit.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Variables that are always shown as a list, whatever their value.
const LIST_VARS: [&str; 3] = ["PATH", "PATHEXT", "PSMODULEPATH"];

/// Whether a value opens in list mode by default.
pub fn is_list_var(name: &str, value: &str) -> bool {
    value.contains(';') || LIST_VARS.contains(&fold_name(name).as_str())
}

/// Splits a value into its `;`-separated entries, dropping empty ones.
pub fn split_entries(value: &str) -> Vec<String> {
    value
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect()
}

/// Joins entries with `;`, dropping empty ones.
pub fn join_entries<S: AsRef<str>>(entries: &[S]) -> String {
    entries
        .iter()
        .map(AsRef::as_ref)
        .filter(|e| !e.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

/// Checks a variable name for characters Windows does not allow.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Name must not be empty.".to_string());
    }
    if name.contains('=') {
        return Err("Name must not contain '='.".to_string());
    }
    if name.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("Name must not contain control characters.".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_drops_empty_entries() {
        assert_eq!(split_entries(r"C:\a;;C:\b;"), vec![r"C:\a", r"C:\b"]);
        assert!(split_entries("").is_empty());
        assert_eq!(split_entries("single"), vec!["single"]);
    }

    #[test]
    fn join_drops_empty_entries() {
        assert_eq!(join_entries(&[r"C:\a", "", r"C:\b", ""]), r"C:\a;C:\b");
        assert_eq!(join_entries::<&str>(&[]), "");
    }

    #[test]
    fn split_join_round_trip_normalizes() {
        let entries = split_entries(r"C:\a;;C:\b;");
        assert_eq!(join_entries(&entries), r"C:\a;C:\b");
    }

    #[test]
    fn list_mode_rule() {
        assert!(is_list_var("Path", r"C:\only"));
        assert!(is_list_var("PATHEXT", ".COM"));
        assert!(is_list_var("psmodulepath", "x"));
        assert!(is_list_var("OTHER", "a;b"));
        assert!(!is_list_var("TEMP", r"C:\Temp"));
    }

    #[test]
    fn keys_are_case_insensitive() {
        assert_eq!(Key::new(Scope::User, "Path"), Key::new(Scope::User, "PATH"));
        assert_ne!(Key::new(Scope::User, "Path"), Key::new(Scope::Machine, "Path"));
    }

    #[test]
    fn type_tokens_round_trip() {
        for ty in [ValueType::Sz, ValueType::ExpandSz, ValueType::Other(3)] {
            assert_eq!(ValueType::from_token(&ty.token()), Some(ty));
        }
        assert_eq!(ValueType::from_token("nope"), None);
    }

    #[test]
    fn default_type_follows_percent() {
        assert_eq!(ValueType::default_for(r"%USERPROFILE%\tools"), ValueType::ExpandSz);
        assert_eq!(ValueType::default_for(r"C:\tools"), ValueType::Sz);
    }

    #[test]
    fn name_validation() {
        assert!(validate_name("TOOLS").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("A=B").is_err());
    }

    #[test]
    fn utf16_length_counts_code_units() {
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("\u{1F600}"), 2);
    }
}
