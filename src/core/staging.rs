//! Staged, not yet written changes with undo.

use std::collections::BTreeMap;

use super::model::{Key, Scope, VALUE_LIMIT, ValueType, Var, utf16_len, validate_name};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Set { value: String, ty: ValueType },
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Staged {
    /// Display name; for created variables the name the user typed.
    name: String,
    change: Change,
    /// The user picked the type explicitly (only matters for created variables).
    explicit_type: bool,
}

/// Status of a variable row relative to the loaded state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Unchanged,
    Modified,
    Added,
    Deleted,
}

/// One variable as the UI shows it: loaded state plus staged change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: Key,
    pub var: Var,
    pub state: RowState,
}

/// A change ready to be written, with the state it expects to find.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedChange {
    pub scope: Scope,
    pub name: String,
    /// Loaded value and type; `None` when the variable is being created.
    pub before: Option<(String, ValueType)>,
    /// New value and type; `None` when the variable is being deleted.
    pub after: Option<(String, ValueType)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    TypeOnly,
    Deleted,
}

impl PlannedChange {
    pub fn kind(&self) -> ChangeKind {
        match (&self.before, &self.after) {
            (None, _) => ChangeKind::Added,
            (Some(_), None) => ChangeKind::Deleted,
            (Some((old, _)), Some((new, _))) if old == new => ChangeKind::TypeOnly,
            _ => ChangeKind::Modified,
        }
    }
}

#[derive(Debug, Default)]
pub struct Staging {
    loaded: BTreeMap<Key, Var>,
    staged: BTreeMap<Key, Staged>,
    undo: Vec<BTreeMap<Key, Staged>>,
}

impl Staging {
    pub fn new(user: Vec<Var>, machine: Vec<Var>) -> Staging {
        let mut staging = Staging::default();
        staging.replace_loaded(user, machine);
        staging
    }

    /// Replaces the loaded state, e.g. after Apply. Staged changes that now
    /// match the registry are dropped; the undo history is cleared because it
    /// refers to the old state.
    pub fn replace_loaded(&mut self, user: Vec<Var>, machine: Vec<Var>) {
        self.loaded = user
            .into_iter()
            .map(|v| (Key::new(Scope::User, &v.name), v))
            .chain(machine.into_iter().map(|v| (Key::new(Scope::Machine, &v.name), v)))
            .collect();
        let keys: Vec<Key> = self.staged.keys().cloned().collect();
        for key in keys {
            let staged = self.staged.remove(&key).expect("key from map");
            if let Some(staged) = self.normalize(&key, staged) {
                self.staged.insert(key, staged);
            }
        }
        self.undo.clear();
    }

    /// Replaces one loaded variable with the registry's current state and
    /// discards its staged change (used to resolve conflicts).
    pub fn reload_var(&mut self, scope: Scope, name: &str, current: Option<(String, ValueType)>) {
        let key = Key::new(scope, name);
        self.push_undo();
        self.staged.remove(&key);
        match current {
            Some((value, ty)) => {
                let display = self.loaded.get(&key).map_or(name.to_string(), |v| v.name.clone());
                self.loaded.insert(key, Var { name: display, value, ty });
            }
            None => {
                self.loaded.remove(&key);
            }
        }
    }

    pub fn loaded(&self, scope: Scope, name: &str) -> Option<&Var> {
        self.loaded.get(&Key::new(scope, name))
    }

    /// The variable as it will be after Apply; `None` if absent or deleted.
    pub fn effective(&self, scope: Scope, name: &str) -> Option<Var> {
        let key = Key::new(scope, name);
        match self.staged.get(&key) {
            Some(Staged { change: Change::Delete, .. }) => None,
            Some(Staged { name, change: Change::Set { value, ty }, .. }) => {
                let display = self.loaded.get(&key).map_or(name.clone(), |v| v.name.clone());
                Some(Var { name: display, value: value.clone(), ty: *ty })
            }
            None => self.loaded.get(&key).cloned(),
        }
    }

    /// All variables of both scopes, including pending deletions, sorted by scope and name.
    pub fn rows(&self) -> Vec<Row> {
        let mut keys: Vec<&Key> = self.loaded.keys().chain(self.staged.keys()).collect();
        keys.sort();
        keys.dedup();
        keys.into_iter()
            .map(|key| {
                let loaded = self.loaded.get(key);
                let (var, state) = match (self.staged.get(key), loaded) {
                    (None, Some(var)) => (var.clone(), RowState::Unchanged),
                    (Some(Staged { change: Change::Delete, .. }), Some(var)) => (var.clone(), RowState::Deleted),
                    (Some(Staged { name, change: Change::Set { value, ty }, .. }), loaded) => {
                        let display = loaded.map_or(name.clone(), |v| v.name.clone());
                        let state = if loaded.is_some() { RowState::Modified } else { RowState::Added };
                        (Var { name: display, value: value.clone(), ty: *ty }, state)
                    }
                    (_, None) => unreachable!("staged delete without loaded variable"),
                };
                Row { key: key.clone(), var, state }
            })
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        self.staged.len()
    }

    #[cfg(test)]
    pub fn is_pending(&self, scope: Scope, name: &str) -> bool {
        self.staged.contains_key(&Key::new(scope, name))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Pending changes with old and new state, sorted by scope and name.
    pub fn planned(&self) -> Vec<PlannedChange> {
        self.staged
            .iter()
            .map(|(key, staged)| {
                let loaded = self.loaded.get(key);
                PlannedChange {
                    scope: key.scope,
                    name: loaded.map_or(staged.name.clone(), |v| v.name.clone()),
                    before: loaded.map(|v| (v.value.clone(), v.ty)),
                    after: match &staged.change {
                        Change::Set { value, ty } => Some((value.clone(), *ty)),
                        Change::Delete => None,
                    },
                }
            })
            .collect()
    }

    /// Stages a new value for an existing or created variable.
    pub fn set_value(&mut self, scope: Scope, name: &str, value: &str) -> Result<(), String> {
        check_value(value)?;
        let key = Key::new(scope, name);
        let current = self.effective(scope, name).ok_or_else(|| format!("{name} does not exist."))?;
        check_editable(&current)?;
        let explicit_type = self.staged.get(&key).is_some_and(|s| s.explicit_type);
        let ty = if self.loaded.contains_key(&key) || explicit_type {
            current.ty
        } else {
            ValueType::default_for(value)
        };
        self.stage(key, name, Change::Set { value: value.to_string(), ty }, explicit_type);
        Ok(())
    }

    /// Stages an explicit type change.
    pub fn set_type(&mut self, scope: Scope, name: &str, ty: ValueType) -> Result<(), String> {
        if !ty.is_editable() {
            return Err("Only REG_SZ and REG_EXPAND_SZ can be chosen.".to_string());
        }
        let current = self.effective(scope, name).ok_or_else(|| format!("{name} does not exist."))?;
        check_editable(&current)?;
        if current.ty == ty {
            return Ok(());
        }
        self.stage(Key::new(scope, name), name, Change::Set { value: current.value, ty }, true);
        Ok(())
    }

    /// Stages a new variable. A pending deletion of the same name becomes a modification.
    pub fn create(&mut self, scope: Scope, name: &str, value: &str) -> Result<(), String> {
        validate_name(name)?;
        check_value(value)?;
        if self.effective(scope, name).is_some() {
            return Err(format!("{} variable {name} already exists.", scope.label()));
        }
        let key = Key::new(scope, name);
        let ty = match self.loaded.get(&key) {
            Some(loaded) if loaded.ty.is_editable() => loaded.ty,
            _ => ValueType::default_for(value),
        };
        self.stage(key, name, Change::Set { value: value.to_string(), ty }, false);
        Ok(())
    }

    /// Stages deletion of a variable; a created variable simply disappears.
    pub fn delete(&mut self, scope: Scope, name: &str) -> Result<(), String> {
        if self.effective(scope, name).is_none() {
            return Err(format!("{name} does not exist."));
        }
        self.stage(Key::new(scope, name), name, Change::Delete, false);
        Ok(())
    }

    /// Reverts the most recent edit. Returns false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(previous) => {
                self.staged = previous;
                true
            }
            None => false,
        }
    }

    pub fn discard_all(&mut self) {
        if !self.staged.is_empty() {
            self.push_undo();
            self.staged.clear();
        }
    }

    fn push_undo(&mut self) {
        self.undo.push(self.staged.clone());
    }

    fn stage(&mut self, key: Key, name: &str, change: Change, explicit_type: bool) {
        let staged = Staged { name: name.to_string(), change, explicit_type };
        let normalized = self.normalize(&key, staged);
        if self.staged.get(&key) == normalized.as_ref() {
            return;
        }
        self.push_undo();
        match normalized {
            Some(staged) => self.staged.insert(key, staged),
            None => self.staged.remove(&key),
        };
    }

    /// Drops changes that would leave the registry as loaded.
    fn normalize(&self, key: &Key, staged: Staged) -> Option<Staged> {
        match (&staged.change, self.loaded.get(key)) {
            (Change::Set { value, ty }, Some(loaded)) if *value == loaded.value && *ty == loaded.ty => None,
            (Change::Delete, None) => None,
            _ => Some(staged),
        }
    }
}

fn check_value(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("Value must not be empty. Delete the variable instead.".to_string());
    }
    let len = utf16_len(value);
    if len > VALUE_LIMIT {
        return Err(format!("Value is {len} characters long; the Windows limit is {VALUE_LIMIT}."));
    }
    Ok(())
}

fn check_editable(var: &Var) -> Result<(), String> {
    if var.ty.is_editable() {
        Ok(())
    } else {
        Err(format!("{} has registry type {} and is read-only here.", var.name, var.ty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staging() -> Staging {
        Staging::new(
            vec![
                Var::new("Path", r"C:\a;C:\b", ValueType::Sz),
                Var::new("TEMP", r"C:\Temp", ValueType::ExpandSz),
                Var::new("OLD_VAR", "x", ValueType::Sz),
                Var::new("BIN", "", ValueType::Other(3)),
            ],
            vec![Var::new("Path", r"C:\Windows", ValueType::ExpandSz)],
        )
    }

    #[test]
    fn set_value_keeps_loaded_type() {
        let mut s = staging();
        s.set_value(Scope::Machine, "path", r"C:\Windows;%X%").unwrap();
        let planned = s.planned();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].after, Some((r"C:\Windows;%X%".to_string(), ValueType::ExpandSz)));
        assert_eq!(planned[0].name, "Path");
        assert_eq!(planned[0].kind(), ChangeKind::Modified);
    }

    #[test]
    fn create_picks_type_from_percent() {
        let mut s = staging();
        s.create(Scope::User, "TOOLS", r"%USERPROFILE%\tools").unwrap();
        s.create(Scope::User, "PLAIN", r"C:\tools").unwrap();
        assert_eq!(s.effective(Scope::User, "TOOLS").unwrap().ty, ValueType::ExpandSz);
        assert_eq!(s.effective(Scope::User, "PLAIN").unwrap().ty, ValueType::Sz);
        // Editing a created variable re-evaluates the default type.
        s.set_value(Scope::User, "PLAIN", r"%X%\tools").unwrap();
        assert_eq!(s.effective(Scope::User, "PLAIN").unwrap().ty, ValueType::ExpandSz);
        assert_eq!(s.planned()[0].kind(), ChangeKind::Added);
    }

    #[test]
    fn explicit_type_sticks_for_created_variable() {
        let mut s = staging();
        s.create(Scope::User, "NEW", r"C:\x").unwrap();
        s.set_type(Scope::User, "NEW", ValueType::ExpandSz).unwrap();
        s.set_value(Scope::User, "NEW", r"C:\y").unwrap();
        assert_eq!(s.effective(Scope::User, "NEW").unwrap().ty, ValueType::ExpandSz);
    }

    #[test]
    fn type_only_change() {
        let mut s = staging();
        s.set_type(Scope::User, "Path", ValueType::ExpandSz).unwrap();
        let planned = s.planned();
        assert_eq!(planned[0].kind(), ChangeKind::TypeOnly);
        assert_eq!(planned[0].after, Some((r"C:\a;C:\b".to_string(), ValueType::ExpandSz)));
    }

    #[test]
    fn create_existing_fails() {
        let mut s = staging();
        assert!(s.create(Scope::User, "path", "x").is_err());
        assert!(s.create(Scope::User, "A=B", "x").is_err());
    }

    #[test]
    fn delete_then_create_becomes_modification() {
        let mut s = staging();
        s.delete(Scope::User, "OLD_VAR").unwrap();
        s.create(Scope::User, "OLD_VAR", "y").unwrap();
        let planned = s.planned();
        assert_eq!(planned[0].kind(), ChangeKind::Modified);
        assert_eq!(planned[0].after, Some(("y".to_string(), ValueType::Sz)));
    }

    #[test]
    fn deleting_created_variable_removes_it() {
        let mut s = staging();
        s.create(Scope::User, "NEW", "x").unwrap();
        s.delete(Scope::User, "NEW").unwrap();
        assert_eq!(s.pending_count(), 0);
        assert!(s.effective(Scope::User, "NEW").is_none());
    }

    #[test]
    fn deletion_rows_stay_visible() {
        let mut s = staging();
        s.delete(Scope::User, "OLD_VAR").unwrap();
        let row = s.rows().into_iter().find(|r| r.var.name == "OLD_VAR").unwrap();
        assert_eq!(row.state, RowState::Deleted);
        assert_eq!(s.planned()[0].kind(), ChangeKind::Deleted);
    }

    #[test]
    fn rows_are_sorted_with_states() {
        let mut s = staging();
        s.create(Scope::User, "AAA", "1").unwrap();
        s.set_value(Scope::User, "TEMP", r"D:\Temp").unwrap();
        let rows = s.rows();
        let summary: Vec<_> = rows.iter().map(|r| (r.key.scope, r.var.name.as_str(), r.state)).collect();
        assert_eq!(
            summary,
            vec![
                (Scope::User, "AAA", RowState::Added),
                (Scope::User, "BIN", RowState::Unchanged),
                (Scope::User, "OLD_VAR", RowState::Unchanged),
                (Scope::User, "Path", RowState::Unchanged),
                (Scope::User, "TEMP", RowState::Modified),
                (Scope::Machine, "Path", RowState::Unchanged),
            ]
        );
    }

    #[test]
    fn empty_value_refused() {
        let mut s = staging();
        let err = s.set_value(Scope::User, "Path", "").unwrap_err();
        assert!(err.contains("Delete the variable"));
        assert_eq!(s.pending_count(), 0);
    }

    #[test]
    fn over_limit_value_refused() {
        let mut s = staging();
        assert!(s.set_value(Scope::User, "Path", &"x".repeat(VALUE_LIMIT + 1)).is_err());
        assert!(s.set_value(Scope::User, "Path", &"x".repeat(VALUE_LIMIT)).is_ok());
    }

    #[test]
    fn read_only_types_refused() {
        let mut s = staging();
        assert!(s.set_value(Scope::User, "BIN", "x").is_err());
        assert!(s.set_type(Scope::User, "BIN", ValueType::Sz).is_err());
    }

    #[test]
    fn setting_loaded_value_clears_pending() {
        let mut s = staging();
        s.set_value(Scope::User, "TEMP", r"D:\Temp").unwrap();
        assert!(s.is_pending(Scope::User, "TEMP"));
        s.set_value(Scope::User, "TEMP", r"C:\Temp").unwrap();
        assert!(!s.is_pending(Scope::User, "TEMP"));
    }

    #[test]
    fn repeated_undo_restores_earlier_states() {
        let mut s = staging();
        s.set_value(Scope::User, "Path", r"C:\a").unwrap();
        s.set_value(Scope::User, "Path", r"C:\a;C:\c").unwrap();
        s.delete(Scope::User, "OLD_VAR").unwrap();
        assert_eq!(s.pending_count(), 2);

        assert!(s.undo());
        assert!(!s.is_pending(Scope::User, "OLD_VAR"));
        assert_eq!(s.effective(Scope::User, "Path").unwrap().value, r"C:\a;C:\c");

        assert!(s.undo());
        assert_eq!(s.effective(Scope::User, "Path").unwrap().value, r"C:\a");

        assert!(s.undo());
        assert!(!s.is_pending(Scope::User, "Path"));
        assert_eq!(s.pending_count(), 0);
        assert!(!s.undo());
    }

    #[test]
    fn no_op_edit_adds_no_undo_step() {
        let mut s = staging();
        s.set_value(Scope::User, "Path", r"C:\a;C:\b").unwrap();
        assert!(!s.can_undo());
    }

    #[test]
    fn discard_all_is_undoable() {
        let mut s = staging();
        s.set_value(Scope::User, "TEMP", r"D:\Temp").unwrap();
        s.delete(Scope::User, "OLD_VAR").unwrap();
        s.discard_all();
        assert_eq!(s.pending_count(), 0);
        assert_eq!(s.effective(Scope::User, "TEMP").unwrap().value, r"C:\Temp");
        assert!(s.undo());
        assert_eq!(s.pending_count(), 2);
    }

    #[test]
    fn replace_loaded_drops_applied_changes() {
        let mut s = staging();
        s.set_value(Scope::User, "TEMP", r"D:\Temp").unwrap();
        s.set_value(Scope::User, "Path", r"C:\z").unwrap();
        s.replace_loaded(
            vec![
                Var::new("Path", r"C:\a;C:\b", ValueType::Sz),
                Var::new("TEMP", r"D:\Temp", ValueType::ExpandSz),
            ],
            vec![],
        );
        assert!(!s.is_pending(Scope::User, "TEMP"));
        assert!(s.is_pending(Scope::User, "Path"));
        assert!(!s.can_undo());
    }

    #[test]
    fn reload_var_discards_staged_edit() {
        let mut s = staging();
        s.set_value(Scope::User, "Path", r"C:\mine").unwrap();
        s.reload_var(Scope::User, "Path", Some((r"C:\theirs".to_string(), ValueType::Sz)));
        assert!(!s.is_pending(Scope::User, "Path"));
        assert_eq!(s.effective(Scope::User, "Path").unwrap().value, r"C:\theirs");
    }

    #[test]
    fn planned_lists_all_kinds() {
        let mut s = staging();
        s.create(Scope::User, "NEW", "1").unwrap();
        s.set_value(Scope::User, "TEMP", r"D:\Temp").unwrap();
        s.delete(Scope::User, "OLD_VAR").unwrap();
        s.set_type(Scope::Machine, "Path", ValueType::Sz).unwrap();
        let kinds: Vec<_> = s.planned().iter().map(|p| (p.scope, p.name.clone(), p.kind())).collect();
        assert_eq!(
            kinds,
            vec![
                (Scope::User, "NEW".to_string(), ChangeKind::Added),
                (Scope::User, "OLD_VAR".to_string(), ChangeKind::Deleted),
                (Scope::User, "TEMP".to_string(), ChangeKind::Modified),
                (Scope::Machine, "Path".to_string(), ChangeKind::TypeOnly),
            ]
        );
        let deleted = &s.planned()[1];
        assert_eq!(deleted.before, Some(("x".to_string(), ValueType::Sz)));
        assert_eq!(deleted.after, None);
    }
}
