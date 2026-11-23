//! The editor pane: list mode, text mode, type, diagnostics, and creating variables.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::{
    EM_SETLIMITTEXT, EM_SETREADONLY, LVIF_STATE, LVIS_SELECTED, LVM_EDITLABELW, LVM_GETEDITCONTROL,
    LVN_BEGINLABELEDITW, LVN_ENDLABELEDITW, LVN_ITEMCHANGED, NM_DBLCLK, NMLISTVIEW, NMLVDISPINFOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::win::{self, send};
use super::window::{App, View, WM_APP_DIRS, WM_APP_REFRESH_EDITOR, post_payload};
use crate::core::diagnostics::{self, Presence};
use crate::core::expand::{Expanded, expand};
use crate::core::model::{Key, Scope, VALUE_LIMIT, ValueType, Var, is_list_var, join_entries, split_entries};
use crate::core::staging::Staging;

/// What the editor shows for the selected variable.
struct Selected {
    scope: Scope,
    var: Var,
    deleted: bool,
}

impl Selected {
    fn editable(&self) -> bool {
        !self.deleted && self.var.ty.is_editable()
    }
}

/// Diagnostics of one list entry.
struct EntryInfo {
    expanded: Expanded,
    status: Vec<String>,
}

fn other(scope: Scope) -> Scope {
    match scope {
        Scope::User => Scope::Machine,
        Scope::Machine => Scope::User,
    }
}

/// `%VAR%` lookup: User, then Machine (both including staged edits), then the process environment.
fn lookup(staging: &Staging) -> impl Fn(&str) -> Option<String> + '_ {
    move |name| {
        staging
            .effective(Scope::User, name)
            .or_else(|| staging.effective(Scope::Machine, name))
            .map(|v| v.value)
            .or_else(|| std::env::var(name).ok())
    }
}

fn type_index(ty: ValueType) -> Option<usize> {
    match ty {
        ValueType::Sz => Some(0),
        ValueType::ExpandSz => Some(1),
        ValueType::Other(_) => None,
    }
}

fn clean_text(text: &str) -> String {
    text.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

impl App {
    fn selected_var(&self) -> Option<Selected> {
        let (scope, name) = self.selected.as_ref()?;
        match self.staging.effective(*scope, name) {
            Some(var) => Some(Selected { scope: *scope, var, deleted: false }),
            None => self.staging.loaded(*scope, name).map(|var| Selected { scope: *scope, var: var.clone(), deleted: true }),
        }
    }

    pub(super) fn editor_list_mode(&self) -> bool {
        self.selected_var().is_some_and(|s| self.is_list_mode(s.scope, &s.var))
    }

    fn is_list_mode(&self, scope: Scope, var: &Var) -> bool {
        self.mode_override
            .get(&Key::new(scope, &var.name))
            .copied()
            .unwrap_or_else(|| is_list_var(&var.name, &var.value))
    }

    /// Fills the editor from the staged state of the selected variable.
    pub(super) fn load_editor(&mut self) {
        self.text_dirty = false;
        self.adding = None;
        let Some(sel) = self.selected_var() else {
            self.selected = None;
            self.draft.clear();
            win::set_text(self.c.header, "Select a variable on the left, or create one with New.");
            win::lv_clear(self.c.entries);
            self.set_editor_text("");
            win::set_text(self.c.length, "");
            win::set_text(self.c.preview, "");
            win::combo_select(self.c.type_combo, None);
            for hwnd in [self.c.type_combo, self.c.mode, self.c.add, self.c.remove, self.c.up, self.c.down, self.c.delete_var] {
                win::enable(hwnd, false);
            }
            self.layout();
            return;
        };

        let mut header = format!("{} variable  {}", sel.scope.label(), sel.var.name);
        if sel.deleted {
            header.push_str("  (pending deletion)");
        } else if !sel.var.ty.is_editable() {
            header.push_str(&format!("  (read-only, {})", sel.var.ty));
        }
        win::set_text(self.c.header, &header);
        let editable = sel.editable();
        win::combo_select(self.c.type_combo, type_index(sel.var.ty));
        win::enable(self.c.type_combo, editable);
        win::enable(self.c.delete_var, !sel.deleted);
        let list_mode = self.is_list_mode(sel.scope, &sel.var);
        // Keep the mode for the session, so removing entries never flips a list to text.
        self.mode_override.insert(Key::new(sel.scope, &sel.var.name), list_mode);
        win::set_text(self.c.mode, if list_mode { "Text mode" } else { "List mode" });
        win::enable(self.c.mode, sel.var.ty.is_editable());
        for hwnd in [self.c.add, self.c.remove, self.c.up, self.c.down] {
            win::enable(hwnd, editable);
        }
        send(self.c.text, EM_SETREADONLY, usize::from(!editable), 0);

        if list_mode {
            self.draft = split_entries(&sel.var.value);
            let focus = self.entry_focus.take().map(|i| i.min(self.draft.len().saturating_sub(1)));
            self.fill_entries(focus);
        } else {
            self.draft.clear();
            self.set_editor_text(&sel.var.value);
        }
        self.update_meta(&sel.var.value, sel.var.ty, sel.deleted);
        self.layout();
    }

    /// Sets the text-mode edit without marking it dirty.
    fn set_editor_text(&mut self, text: &str) {
        win::set_text(self.c.text, text);
        self.text_dirty = false;
    }

    /// Length meter and preview line for `value`.
    fn update_meta(&mut self, value: &str, ty: ValueType, deleted: bool) {
        if deleted {
            win::set_text(self.c.length, "Pending deletion. Undo (Ctrl+Z) or Discard keeps the variable.");
        } else {
            let info = diagnostics::length_info(value);
            let mut text = format!("Length: {} / {} characters", info.len, info.limit);
            if info.over_legacy {
                text.push_str("  (longer than 2047: the built-in Windows dialog cannot edit this value)");
            }
            win::set_text(self.c.length, &text);
        }

        let mut lines = Vec::new();
        if !ty.is_editable() {
            lines.push(format!("Registry type {ty} is shown read-only."));
        }
        if diagnostics::reg_sz_warning(ty, value) {
            lines.push("Warning: this value is REG_SZ, so Windows will not expand its %VAR% references. Switch the type to REG_EXPAND_SZ.".to_string());
        }
        if self.editor_list_mode() {
            if let Some(i) = win::lv_selected(self.c.entries)
                && let (Some(entry), Some(info)) = (self.draft.get(i), self.entries_info().into_iter().nth(i))
            {
                if info.expanded.text != *entry {
                    lines.push(format!("Expands to: {}", info.expanded.text));
                }
                if !info.status.is_empty() {
                    lines.push(format!("Entry {}: {}", i + 1, info.status.join(", ")));
                }
            }
        } else {
            let expanded = expand(value, &lookup(&self.staging));
            if expanded.text != value {
                lines.push(format!("Expands to: {}", expanded.text));
            }
            if !expanded.unresolved.is_empty() {
                lines.push(format!("Unresolved: {}", percent_list(&expanded.unresolved)));
            }
        }
        win::set_text(self.c.preview, &lines.join("\r\n"));
    }

    fn refresh_meta(&mut self) {
        if let Some(sel) = self.selected_var() {
            let value = if self.text_dirty { clean_text(&win::get_text(self.c.text)) } else { sel.var.value.clone() };
            self.update_meta(&value, sel.var.ty, sel.deleted);
        }
    }

    /// Expanded entries of the same-named variable in the other scope.
    fn other_scope_entries(&self) -> Vec<String> {
        let Some((scope, name)) = &self.selected else { return Vec::new() };
        let lookup = lookup(&self.staging);
        self.staging
            .effective(other(*scope), name)
            .map(|v| split_entries(&v.value).iter().map(|e| expand(e, &lookup).text).collect())
            .unwrap_or_default()
    }

    /// Diagnostics for every entry of `draft`.
    fn entries_info(&self) -> Vec<EntryInfo> {
        let other_entries = self.other_scope_entries();
        let lookup = lookup(&self.staging);
        let expanded: Vec<Expanded> = self.draft.iter().map(|e| expand(e, &lookup)).collect();
        let texts: Vec<String> = expanded.iter().map(|e| e.text.clone()).collect();
        let dups = diagnostics::duplicate_flags(&texts, &other_entries);
        let other_label = self.selected.as_ref().map(|(s, n)| format!("also in {} {}", other(*s).label(), n));
        expanded
            .into_iter()
            .zip(dups)
            .map(|(expanded, dup)| {
                let mut status = Vec::new();
                match diagnostics::presence(&expanded.text, &self.dirs) {
                    Presence::Missing => status.push("missing".to_string()),
                    Presence::Unchecked => status.push("network path, not checked".to_string()),
                    Presence::Pending => status.push("checking…".to_string()),
                    Presence::Present | Presence::NotAPath => {}
                }
                if dup.same_scope {
                    status.push("duplicate".to_string());
                }
                if dup.cross_scope {
                    status.push(other_label.clone().unwrap_or_default());
                }
                if !expanded.unresolved.is_empty() {
                    status.push(format!("unresolved {}", percent_list(&expanded.unresolved)));
                }
                EntryInfo { expanded, status }
            })
            .collect()
    }

    /// Fills the entry list from `draft` with diagnostics and starts directory checks.
    fn fill_entries(&mut self, select: Option<usize>) {
        let infos = self.entries_info();
        win::lv_clear(self.c.entries);
        for (i, (entry, info)) in self.draft.iter().zip(&infos).enumerate() {
            let shown_expansion = if info.expanded.text != *entry { info.expanded.text.clone() } else { String::new() };
            win::lv_append(self.c.entries, &[entry.clone(), info.status.join(", "), shown_expansion], i as isize, None);
        }
        win::lv_select(self.c.entries, select.filter(|&i| i < self.draft.len()));

        let expanded: Vec<String> = infos.into_iter().map(|i| i.expanded.text).collect();
        let unknown = self.dirs.unknown(&expanded);
        self.check_dirs(unknown);
    }

    fn check_dirs(&mut self, paths: Vec<String>) {
        let paths: Vec<String> = paths.into_iter().filter(|p| self.dirs_in_flight.insert(p.clone())).collect();
        if paths.is_empty() {
            return;
        }
        let hwnd_raw = self.hwnd.0 as isize;
        std::thread::spawn(move || post_payload(hwnd_raw, WM_APP_DIRS, diagnostics::check_dirs(paths)));
    }

    pub(super) fn on_dirs_checked(&mut self, results: Vec<(String, bool)>) {
        for (path, exists) in results {
            self.dirs_in_flight.remove(&path);
            self.dirs.insert(&path, exists);
        }
        if self.view == View::Editor && self.editor_list_mode() && self.adding.is_none() {
            let selected = win::lv_selected(self.c.entries);
            self.fill_entries(selected);
            self.refresh_meta();
        }
    }

    // ---- text mode ----

    pub(super) fn on_text_changed(&mut self) {
        if !self.text_dirty {
            self.text_dirty = true;
            self.update_toolbar();
        }
        self.refresh_meta();
    }

    /// Stages typing from the text-mode edit.
    pub(super) fn commit_text(&mut self) {
        if !self.text_dirty {
            return;
        }
        self.text_dirty = false;
        let Some((scope, name)) = self.selected.clone() else { return };
        let value = clean_text(&win::get_text(self.c.text));
        match self.staging.set_value(scope, &name, &value) {
            Ok(()) => {}
            Err(e) => {
                self.set_status(&e);
                self.load_editor();
            }
        }
        self.after_change();
        self.refresh_meta();
    }

    // ---- type and mode ----

    pub(super) fn on_type_selected(&mut self) {
        let Some((scope, name)) = self.selected.clone() else { return };
        let ty = match win::combo_selected(self.c.type_combo) {
            Some(0) => ValueType::Sz,
            Some(1) => ValueType::ExpandSz,
            _ => return,
        };
        self.commit_text();
        if let Err(e) = self.staging.set_type(scope, &name, ty) {
            self.set_status(&e);
        }
        self.after_change();
        self.load_editor();
    }

    pub(super) fn toggle_mode(&mut self) {
        let Some(sel) = self.selected_var() else { return };
        self.commit_text();
        let list = self.is_list_mode(sel.scope, &sel.var);
        self.mode_override.insert(Key::new(sel.scope, &sel.var.name), !list);
        self.load_editor();
    }

    // ---- list mode ----

    fn list_editable(&self) -> bool {
        self.view == View::Editor && self.editor_list_mode() && self.selected_var().is_some_and(|s| s.editable())
    }

    /// Stages the joined draft as the new value and refills the editor.
    fn commit_entries(&mut self, select: Option<usize>) {
        let Some((scope, name)) = self.selected.clone() else { return };
        let value = join_entries(&self.draft);
        if let Err(e) = self.staging.set_value(scope, &name, &value) {
            self.set_status(&e);
        }
        self.entry_focus = select;
        self.after_change();
    }

    pub(super) fn add_entry(&mut self) {
        if !self.list_editable() || self.adding.is_some() {
            return;
        }
        let at = win::lv_selected(self.c.entries).map_or(self.draft.len(), |i| i + 1);
        self.draft.insert(at, String::new());
        self.adding = Some(at);
        self.fill_entries(Some(at));
        self.begin_label_edit(at);
    }

    pub(super) fn edit_entry(&mut self) {
        if !self.list_editable() {
            return;
        }
        if let Some(i) = win::lv_selected(self.c.entries) {
            self.begin_label_edit(i);
        }
    }

    fn begin_label_edit(&self, index: usize) {
        // SAFETY: entries is a live child window.
        unsafe {
            let _ = SetFocus(Some(self.c.entries));
        }
        send(self.c.entries, LVM_EDITLABELW, index, 0);
    }

    pub(super) fn remove_entry(&mut self) {
        if !self.list_editable() {
            return;
        }
        let Some(i) = win::lv_selected(self.c.entries) else { return };
        if i >= self.draft.len() {
            return;
        }
        self.draft.remove(i);
        let next = i.min(self.draft.len().saturating_sub(1));
        self.commit_entries(Some(next));
        self.load_editor();
    }

    pub(super) fn move_entry(&mut self, delta: isize) {
        if !self.list_editable() {
            return;
        }
        let Some(i) = win::lv_selected(self.c.entries) else { return };
        let Some(j) = i.checked_add_signed(delta).filter(|&j| j < self.draft.len()) else { return };
        self.draft.swap(i, j);
        self.commit_entries(Some(j));
        self.load_editor();
    }

    pub(super) fn on_entries_notify(&mut self, code: u32, lparam: LPARAM) -> Option<LRESULT> {
        match code {
            LVN_ITEMCHANGED => {
                // SAFETY: LVN_ITEMCHANGED carries an NMLISTVIEW.
                let nm = unsafe { &*(lparam.0 as *const NMLISTVIEW) };
                if nm.uChanged.0 & LVIF_STATE.0 != 0 && (nm.uNewState ^ nm.uOldState) & LVIS_SELECTED.0 != 0 {
                    self.refresh_meta();
                }
                Some(LRESULT(0))
            }
            LVN_BEGINLABELEDITW => {
                if !self.list_editable() {
                    return Some(LRESULT(1)); // cancel editing
                }
                let edit = send(self.c.entries, LVM_GETEDITCONTROL, 0, 0);
                if edit != 0 {
                    send(HWND(edit as _), EM_SETLIMITTEXT, VALUE_LIMIT, 0);
                }
                Some(LRESULT(0))
            }
            LVN_ENDLABELEDITW => {
                // SAFETY: LVN_ENDLABELEDITW carries an NMLVDISPINFOW; pszText is null when cancelled.
                let (index, text) = unsafe {
                    let info = &*(lparam.0 as *const NMLVDISPINFOW);
                    let text = (!info.item.pszText.is_null()).then(|| info.item.pszText.to_string().unwrap_or_default());
                    (info.item.iItem, text)
                };
                self.end_label_edit(index, text);
                // The list is refilled after this notification returns.
                // SAFETY: posting to our own window.
                unsafe {
                    let _ = PostMessageW(Some(self.hwnd), WM_APP_REFRESH_EDITOR, WPARAM(0), LPARAM(0));
                }
                Some(LRESULT(0))
            }
            NM_DBLCLK => {
                self.edit_entry();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn end_label_edit(&mut self, index: i32, text: Option<String>) {
        let adding = self.adding.take();
        let Ok(index) = usize::try_from(index) else { return };
        match text {
            None => {
                if adding == Some(index) && index < self.draft.len() {
                    self.draft.remove(index);
                }
                self.entry_focus = Some(index.saturating_sub(usize::from(adding.is_some())));
            }
            Some(text) => {
                if index < self.draft.len() {
                    self.draft[index] = text;
                    self.commit_entries(Some(index));
                }
            }
        }
    }

    // ---- create and delete ----

    pub(super) fn begin_create(&mut self) {
        self.commit_text();
        win::set_text(self.c.name, "");
        win::set_text(self.c.value, "");
        let scope = self.selected.as_ref().map_or(Scope::User, |(s, _)| *s);
        win::combo_select(self.c.scope, Some(if scope == Scope::User { 0 } else { 1 }));
        self.show_view(View::Create);
        // SAFETY: name is a live child window.
        unsafe {
            let _ = SetFocus(Some(self.c.name));
        }
    }

    pub(super) fn create_var(&mut self) {
        let name = win::get_text(self.c.name).trim().to_string();
        let value = clean_text(&win::get_text(self.c.value));
        let scope = if win::combo_selected(self.c.scope) == Some(1) { Scope::Machine } else { Scope::User };
        match self.staging.create(scope, &name, &value) {
            Ok(()) => {
                self.selected = Some((scope, name.clone()));
                self.entry_focus = None;
                self.set_status(&format!("Created {} variable {name} (pending until Apply).", scope.label()));
                let key = Key::new(scope, &name);
                self.refresh_var_list();
                if !self.shown_rows.iter().any(|r| r.key == key) {
                    win::set_text(self.c.filter, "");
                }
                self.rebuild_var_list();
                self.show_view(View::Editor);
            }
            Err(e) => self.set_status(&e),
        }
    }

    pub(super) fn delete_selected(&mut self) {
        let Some((scope, name)) = self.selected.clone() else { return };
        self.commit_text();
        match self.staging.delete(scope, &name) {
            Ok(()) => self.set_status(&format!("{} variable {name} will be deleted on Apply.", scope.label())),
            Err(e) => self.set_status(&e),
        }
        if self.staging.effective(scope, &name).is_none() && self.staging.loaded(scope, &name).is_none() {
            // A created variable vanished entirely.
            self.selected = None;
        }
        self.after_change();
        if self.view != View::Editor {
            self.show_view(View::Editor);
        } else {
            self.load_editor();
        }
    }
}

fn percent_list(names: &[String]) -> String {
    names.iter().map(|n| format!("%{n}%")).collect::<Vec<_>>().join(", ")
}
