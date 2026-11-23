//! The pending-changes pane: what Apply will write, and conflict reload.

use super::win;
use super::window::App;
use crate::core::model::{Key, ValueType};
use crate::core::registry::EnvStore;
use crate::core::staging::ChangeKind;

fn state_text(state: &Option<(String, ValueType)>) -> String {
    match state {
        Some((value, ty)) => format!("[{ty}] {value}"),
        None => "(none)".to_string(),
    }
}

impl App {
    pub(super) fn fill_pending(&mut self) {
        self.pending_rows = self.staging.planned();
        win::lv_clear(self.c.pending_list);
        for (i, change) in self.pending_rows.iter().enumerate() {
            let kind = match change.kind() {
                ChangeKind::Added => "Add",
                ChangeKind::Modified => "Modify",
                ChangeKind::TypeOnly => "Type",
                ChangeKind::Deleted => "Delete",
            };
            let problem = self.last_problems.get(&Key::new(change.scope, &change.name)).cloned().unwrap_or_default();
            let cells = [
                kind.to_string(),
                change.scope.label().to_string(),
                change.name.clone(),
                state_text(&change.before),
                state_text(&change.after),
                problem,
            ];
            win::lv_append(self.c.pending_list, &cells, i as isize, None);
        }
        let reloadable = !self.pending_rows.is_empty();
        win::enable(self.c.reload, reloadable);
    }

    /// Replaces the selected change's variable with the registry's current
    /// value, discarding the staged edit (resolves a conflict).
    pub(super) fn reload_selected_pending(&mut self) {
        let Some(change) = win::lv_selected(self.c.pending_list).and_then(|i| self.pending_rows.get(i)).cloned() else {
            self.set_status("Select a change in the list to reload it.");
            return;
        };
        match self.store.read(change.scope, &change.name) {
            Ok(current) => {
                self.staging.reload_var(change.scope, &change.name, current);
                self.last_problems.remove(&Key::new(change.scope, &change.name));
                self.set_status(&format!(
                    "Reloaded {} {} from the registry; your edit was discarded.",
                    change.scope.label(),
                    change.name
                ));
            }
            Err(e) => self.set_status(&e),
        }
        self.after_change();
    }
}
