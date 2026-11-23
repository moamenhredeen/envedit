//! The variable list on the left, its filter, and notification dispatch.

use windows::Win32::Foundation::{LPARAM, LRESULT};
use windows::Win32::UI::Controls::{LVIF_STATE, LVIS_SELECTED, LVN_ITEMCHANGED, NMHDR, NMLISTVIEW};

use super::win;
use super::window::{App, GROUP_MACHINE, GROUP_USER, View};
use crate::core::model::{Key, Scope, names_equal};
use crate::core::staging::{Row, RowState};

fn row_label(row: &Row) -> String {
    match row.state {
        RowState::Unchanged => row.var.name.clone(),
        RowState::Modified => format!("* {}", row.var.name),
        RowState::Added => format!("* {} (new)", row.var.name),
        RowState::Deleted => format!("* {} (deleted)", row.var.name),
    }
}

fn row_cells(row: &Row) -> [String; 2] {
    let value = if row.var.ty.is_editable() { row.var.value.clone() } else { format!("({})", row.var.ty) };
    [row_label(row), value]
}

impl App {
    fn filtered_rows(&self) -> Vec<Row> {
        let filter = win::get_text(self.c.filter).to_lowercase();
        self.staging
            .rows()
            .into_iter()
            .filter(|row| {
                filter.is_empty()
                    || row.var.name.to_lowercase().contains(&filter)
                    || row.var.value.to_lowercase().contains(&filter)
            })
            .collect()
    }

    /// Rebuilds the list from scratch and restores the selection.
    pub(super) fn rebuild_var_list(&mut self) {
        let rows = self.filtered_rows();
        win::lv_clear(self.c.varlist);
        for (i, row) in rows.iter().enumerate() {
            let group = if row.key.scope == Scope::User { GROUP_USER } else { GROUP_MACHINE };
            win::lv_append(self.c.varlist, &row_cells(row), i as isize, Some(group));
        }
        self.shown_rows = rows;
        self.select_in_list();
    }

    /// Updates row texts in place when the set of rows is unchanged (keeps
    /// clicks in progress intact); otherwise rebuilds.
    pub(super) fn refresh_var_list(&mut self) {
        let rows = self.filtered_rows();
        let same_rows = rows.len() == self.shown_rows.len() && rows.iter().zip(&self.shown_rows).all(|(a, b)| a.key == b.key);
        if !same_rows {
            self.rebuild_var_list();
            return;
        }
        for (i, (new, old)) in rows.iter().zip(&self.shown_rows).enumerate() {
            if new != old {
                for (sub, cell) in row_cells(new).iter().enumerate() {
                    win::lv_set_text(self.c.varlist, i, sub, cell);
                }
            }
        }
        self.shown_rows = rows;
    }

    fn select_in_list(&self) {
        let index = self.selected.as_ref().and_then(|(scope, name)| {
            let key = Key::new(*scope, name);
            self.shown_rows.iter().position(|r| r.key == key)
        });
        win::lv_select(self.c.varlist, index);
    }

    /// Initial selection: the variable named on the command line (User
    /// preferred), or the first row. An unknown name goes into the filter.
    pub(super) fn initial_select(&mut self, name: Option<String>) {
        if let Some(name) = name {
            let found = [Scope::User, Scope::Machine]
                .into_iter()
                .find_map(|scope| self.staging.effective(scope, &name).map(|v| (scope, v.name)));
            match found {
                Some(selected) => self.selected = Some(selected),
                None => win::set_text(self.c.filter, &name),
            }
        }
        self.rebuild_var_list();
        if self.selected.is_none() {
            self.selected = self.shown_rows.first().map(|r| (r.key.scope, r.var.name.clone()));
            self.select_in_list();
        }
        self.load_editor();
    }

    fn on_var_selected(&mut self, index: usize) {
        let Some(row) = self.shown_rows.get(index) else { return };
        let target = (row.key.scope, row.var.name.clone());
        let same = self.selected.as_ref().is_some_and(|(s, n)| *s == target.0 && names_equal(n, &target.1));
        if same && self.view == View::Editor {
            return;
        }
        self.commit_text();
        self.selected = Some(target);
        self.entry_focus = None;
        if self.view == View::Editor {
            self.load_editor();
            self.layout();
        } else {
            self.show_view(View::Editor);
        }
    }

    pub(super) fn on_notify(&mut self, lparam: LPARAM) -> Option<LRESULT> {
        // SAFETY: lparam points to an NMHDR for WM_NOTIFY.
        let hdr = unsafe { &*(lparam.0 as *const NMHDR) };
        if hdr.hwndFrom == self.c.varlist {
            if hdr.code == LVN_ITEMCHANGED {
                // SAFETY: LVN_ITEMCHANGED carries an NMLISTVIEW.
                let nm = unsafe { &*(lparam.0 as *const NMLISTVIEW) };
                let became_selected = nm.uChanged.0 & LVIF_STATE.0 != 0
                    && nm.uNewState & LVIS_SELECTED.0 != 0
                    && nm.uOldState & LVIS_SELECTED.0 == 0;
                if became_selected && let Ok(index) = usize::try_from(nm.lParam.0) {
                    self.on_var_selected(index);
                }
            }
            return Some(LRESULT(0));
        }
        if hdr.hwndFrom == self.c.entries {
            return self.on_entries_notify(hdr.code, lparam);
        }
        None
    }
}
