//! Writing staged changes of one scope with conflict detection.

use super::model::{Scope, ValueType, Var};
use super::registry::EnvStore;
use super::staging::PlannedChange;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    Conflict(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarResult {
    pub scope: Scope,
    pub name: String,
    pub outcome: Outcome,
}

/// Writes `changes` (all of `scope`) to `store`.
///
/// `snapshot` receives the scope's current variables before anything is
/// written; if it fails, nothing is written and every change fails. Each
/// change is written only if the store still holds the change's `before`
/// state. A failing write does not stop the remaining ones.
pub fn apply_scope(
    store: &mut dyn EnvStore,
    scope: Scope,
    changes: &[PlannedChange],
    snapshot: &mut dyn FnMut(&[Var]) -> Result<(), String>,
) -> Vec<VarResult> {
    if changes.is_empty() {
        return Vec::new();
    }
    let result = |change: &PlannedChange, outcome| VarResult { scope, name: change.name.clone(), outcome };

    let snapshot_result = store.load(scope).and_then(|vars| snapshot(&vars));
    if let Err(e) = snapshot_result {
        let reason = format!("not written, snapshot failed: {e}");
        return changes.iter().map(|c| result(c, Outcome::Failed(reason.clone()))).collect();
    }

    changes
        .iter()
        .map(|change| {
            debug_assert_eq!(change.scope, scope);
            let outcome = match store.read(scope, &change.name) {
                Err(e) => Outcome::Failed(e),
                Ok(current) => match conflict(change.before.as_ref(), current.as_ref()) {
                    Some(reason) => Outcome::Conflict(reason),
                    None => write(store, scope, change),
                },
            };
            result(change, outcome)
        })
        .collect()
}

fn conflict(expected: Option<&(String, ValueType)>, current: Option<&(String, ValueType)>) -> Option<String> {
    match (expected, current) {
        (None, None) => None,
        (Some(e), Some(c)) if e == c => None,
        (Some(_), Some(_)) => Some("changed by another program since it was loaded".to_string()),
        (Some(_), None) => Some("deleted by another program since it was loaded".to_string()),
        (None, Some(_)) => Some("created by another program since it was loaded".to_string()),
    }
}

fn write(store: &mut dyn EnvStore, scope: Scope, change: &PlannedChange) -> Outcome {
    let written = match &change.after {
        Some((value, ty)) => store.write(scope, &change.name, value, *ty),
        None => store.delete(scope, &change.name),
    };
    match written {
        Ok(()) => Outcome::Applied,
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::MemoryStore;

    const U: Scope = Scope::User;

    fn store() -> MemoryStore {
        MemoryStore::with(&[
            (U, "Path", r"C:\a", ValueType::Sz),
            (U, "A", "1", ValueType::Sz),
            (U, "B", "2", ValueType::Sz),
            (U, "C", "3", ValueType::Sz),
        ])
    }

    fn set(name: &str, before: &str, after: &str) -> PlannedChange {
        PlannedChange {
            scope: U,
            name: name.to_string(),
            before: Some((before.to_string(), ValueType::Sz)),
            after: Some((after.to_string(), ValueType::Sz)),
        }
    }

    fn ok_snapshot(taken: &mut Vec<Var>) -> impl FnMut(&[Var]) -> Result<(), String> + '_ {
        move |vars| {
            taken.extend_from_slice(vars);
            Ok(())
        }
    }

    #[test]
    fn applies_when_unchanged_and_snapshots_first() {
        let mut store = store();
        let mut taken = Vec::new();
        let results = apply_scope(&mut store, U, &[set("Path", r"C:\a", r"C:\b")], &mut ok_snapshot(&mut taken));
        assert_eq!(results[0].outcome, Outcome::Applied);
        assert_eq!(store.get(U, "Path").unwrap().value, r"C:\b");
        let snap_path = taken.iter().find(|v| v.name == "Path").unwrap();
        assert_eq!(snap_path.value, r"C:\a", "snapshot holds the state before the write");
    }

    #[test]
    fn external_modification_is_a_conflict() {
        let mut store = store();
        store.write(U, "Path", r"C:\installer", ValueType::Sz).unwrap();
        let mut taken = Vec::new();
        let results = apply_scope(&mut store, U, &[set("Path", r"C:\a", r"C:\b")], &mut ok_snapshot(&mut taken));
        assert!(matches!(results[0].outcome, Outcome::Conflict(_)));
        assert_eq!(store.get(U, "Path").unwrap().value, r"C:\installer");
    }

    #[test]
    fn external_type_change_is_a_conflict() {
        let mut store = store();
        store.write(U, "Path", r"C:\a", ValueType::ExpandSz).unwrap();
        let results = apply_scope(&mut store, U, &[set("Path", r"C:\a", r"C:\b")], &mut |_| Ok(()));
        assert!(matches!(results[0].outcome, Outcome::Conflict(_)));
    }

    #[test]
    fn external_deletion_is_a_conflict_and_not_recreated() {
        let mut store = store();
        store.delete(U, "Path").unwrap();
        let results = apply_scope(&mut store, U, &[set("Path", r"C:\a", r"C:\b")], &mut |_| Ok(()));
        assert!(matches!(results[0].outcome, Outcome::Conflict(_)));
        assert!(store.get(U, "Path").is_none());
    }

    #[test]
    fn external_creation_is_a_conflict() {
        let mut store = store();
        store.write(U, "NEW", "theirs", ValueType::Sz).unwrap();
        let change = PlannedChange {
            scope: U,
            name: "NEW".to_string(),
            before: None,
            after: Some(("mine".to_string(), ValueType::Sz)),
        };
        let results = apply_scope(&mut store, U, &[change], &mut |_| Ok(()));
        assert!(matches!(results[0].outcome, Outcome::Conflict(_)));
        assert_eq!(store.get(U, "NEW").unwrap().value, "theirs");
    }

    #[test]
    fn create_and_delete() {
        let mut store = store();
        let changes = [
            PlannedChange { scope: U, name: "NEW".into(), before: None, after: Some(("x".into(), ValueType::ExpandSz)) },
            PlannedChange { scope: U, name: "A".into(), before: Some(("1".into(), ValueType::Sz)), after: None },
        ];
        let results = apply_scope(&mut store, U, &changes, &mut |_| Ok(()));
        assert!(results.iter().all(|r| r.outcome == Outcome::Applied));
        assert_eq!(store.get(U, "NEW").unwrap().ty, ValueType::ExpandSz);
        assert!(store.get(U, "A").is_none());
    }

    #[test]
    fn one_failing_write_does_not_stop_others() {
        let mut store = store();
        store.failing.push("B".to_string());
        let changes = [set("A", "1", "10"), set("B", "2", "20"), set("C", "3", "30")];
        let results = apply_scope(&mut store, U, &changes, &mut |_| Ok(()));
        let outcomes: Vec<_> = results.iter().map(|r| (r.name.as_str(), matches!(r.outcome, Outcome::Applied))).collect();
        assert_eq!(outcomes, vec![("A", true), ("B", false), ("C", true)]);
        assert!(matches!(results[1].outcome, Outcome::Failed(_)));
        assert_eq!(store.get(U, "A").unwrap().value, "10");
        assert_eq!(store.get(U, "B").unwrap().value, "2");
        assert_eq!(store.get(U, "C").unwrap().value, "30");
    }

    #[test]
    fn snapshot_failure_blocks_all_writes() {
        let mut store = store();
        let changes = [set("A", "1", "10"), set("B", "2", "20")];
        let results = apply_scope(&mut store, U, &changes, &mut |_| Err("disk full".to_string()));
        assert!(results.iter().all(|r| matches!(&r.outcome, Outcome::Failed(m) if m.contains("disk full"))));
        assert_eq!(store.get(U, "A").unwrap().value, "1");
        assert_eq!(store.get(U, "B").unwrap().value, "2");
    }

    #[test]
    fn nothing_to_do_takes_no_snapshot() {
        let mut store = store();
        let mut called = false;
        let results = apply_scope(&mut store, U, &[], &mut |_| {
            called = true;
            Ok(())
        });
        assert!(results.is_empty());
        assert!(!called);
    }
}
