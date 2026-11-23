//! Thin wrappers over Win32 calls used by the UI. All `unsafe` of the UI
//! that is not message handling lives here.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_STATE_FLAGS, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVGF_GROUPID, LVGF_HEADER, LVGROUP, LVIF_GROUPID,
    LVIF_PARAM, LVIF_STATE, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS, LVM_ENABLEGROUPVIEW,
    LVM_ENSUREVISIBLE, LVM_GETITEMCOUNT, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTGROUP,
    LVM_INSERTITEMW, LVM_SETCOLUMNWIDTH, LVM_SETITEMSTATE, LVM_SETITEMTEXTW, LVNI_SELECTED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CreateWindowExW, GetClassNameW, GetWindowTextLengthW,
    GetWindowTextW, HMENU, MoveWindow, SW_HIDE, SW_SHOW, SendMessageW, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_SETFONT, WS_CHILD, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// Text shown in list cells is cut here; full values are in the editor.
const CELL_LIMIT: usize = 1000;

pub fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: plain message send; pointer-carrying callers pass pointers that outlive the call.
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))).0 }
}

pub fn create_child(parent: HWND, class: PCWSTR, text: &str, style: u32, ex_style: u32, id: u16) -> HWND {
    // SAFETY: valid parent and class; text is a NUL-terminated HSTRING.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            class,
            &HSTRING::from(text),
            WINDOW_STYLE(style) | WS_CHILD | WS_VISIBLE,
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(id as usize as _)),
            None,
            None,
        )
        .expect("create child control")
    }
}

pub fn set_text(hwnd: HWND, text: &str) {
    // SAFETY: valid window, NUL-terminated text.
    unsafe {
        let _ = SetWindowTextW(hwnd, &HSTRING::from(text));
    }
}

pub fn get_text(hwnd: HWND) -> String {
    // SAFETY: buffer sized from the reported length plus NUL.
    unsafe {
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let copied = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..copied])
    }
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: buffer and its length match.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..len])
}

pub fn set_font(hwnd: HWND, font: HFONT) {
    send(hwnd, WM_SETFONT, font.0 as usize, 1);
}

pub fn show(hwnd: HWND, visible: bool) {
    // SAFETY: valid window.
    unsafe {
        let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    }
}

pub fn enable(hwnd: HWND, enabled: bool) {
    // SAFETY: valid window.
    unsafe {
        let _ = EnableWindow(hwnd, enabled);
    }
}

pub fn place(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    // SAFETY: valid window.
    unsafe {
        let _ = MoveWindow(hwnd, x, y, w.max(0), h.max(0), true);
    }
}

pub fn cell_text(text: &str) -> String {
    match text.char_indices().nth(CELL_LIMIT) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---- combo box ----

pub fn combo_fill(combo: HWND, items: &[&str]) {
    send(combo, CB_RESETCONTENT, 0, 0);
    for item in items {
        let text = wide(item);
        send(combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
    }
}

pub fn combo_select(combo: HWND, index: Option<usize>) {
    send(combo, CB_SETCURSEL, index.unwrap_or(usize::MAX), 0);
}

pub fn combo_selected(combo: HWND) -> Option<usize> {
    usize::try_from(send(combo, CB_GETCURSEL, 0, 0)).ok()
}

// ---- list view ----

pub fn lv_add_column(lv: HWND, index: usize, title: &str, width: i32) {
    let mut text = wide(title);
    let column = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH,
        cx: width,
        pszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    send(lv, LVM_INSERTCOLUMNW, index, &raw const column as isize);
}

pub fn lv_set_column_width(lv: HWND, index: usize, width: i32) {
    send(lv, LVM_SETCOLUMNWIDTH, index, width as isize);
}

pub fn lv_enable_groups(lv: HWND, groups: &[(i32, &str)]) {
    send(lv, LVM_ENABLEGROUPVIEW, 1, 0);
    for &(id, header) in groups {
        let mut text = wide(header);
        let group = LVGROUP {
            cbSize: size_of::<LVGROUP>() as u32,
            mask: LVGF_HEADER | LVGF_GROUPID,
            pszHeader: PWSTR(text.as_mut_ptr()),
            iGroupId: id,
            ..Default::default()
        };
        send(lv, LVM_INSERTGROUP, usize::MAX, &raw const group as isize);
    }
}

pub fn lv_clear(lv: HWND) {
    send(lv, LVM_DELETEALLITEMS, 0, 0);
}

pub fn lv_count(lv: HWND) -> usize {
    send(lv, LVM_GETITEMCOUNT, 0, 0).max(0) as usize
}

/// Appends a row; `cells[0]` is the item text, the rest are subitems.
pub fn lv_append(lv: HWND, cells: &[String], param: isize, group: Option<i32>) {
    let index = lv_count(lv);
    let mut text = wide(&cell_text(&cells[0]));
    let mut mask = LVIF_TEXT | LVIF_PARAM;
    if group.is_some() {
        mask |= LVIF_GROUPID;
    }
    let item = LVITEMW {
        mask,
        iItem: index as i32,
        pszText: PWSTR(text.as_mut_ptr()),
        lParam: LPARAM(param),
        iGroupId: group.unwrap_or_default(),
        ..Default::default()
    };
    let inserted = send(lv, LVM_INSERTITEMW, 0, &raw const item as isize);
    for (sub, cell) in cells.iter().enumerate().skip(1) {
        lv_set_text(lv, inserted as usize, sub, cell);
    }
}

pub fn lv_set_text(lv: HWND, item: usize, sub: usize, text: &str) {
    let mut text = wide(&cell_text(text));
    let lvitem = LVITEMW {
        iSubItem: sub as i32,
        pszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    send(lv, LVM_SETITEMTEXTW, item, &raw const lvitem as isize);
}

pub fn lv_selected(lv: HWND) -> Option<usize> {
    usize::try_from(send(lv, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize)).ok()
}

/// Selects and focuses one row (or clears the selection) and scrolls it into view.
pub fn lv_select(lv: HWND, item: Option<usize>) {
    let state = LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
    let clear = LVITEMW {
        mask: LVIF_STATE,
        stateMask: state,
        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
        ..Default::default()
    };
    send(lv, LVM_SETITEMSTATE, usize::MAX, &raw const clear as isize);
    if let Some(item) = item {
        let set = LVITEMW {
            mask: LVIF_STATE,
            stateMask: state,
            state,
            ..Default::default()
        };
        send(lv, LVM_SETITEMSTATE, item, &raw const set as isize);
        send(lv, LVM_ENSUREVISIBLE, item, 0);
    }
}
