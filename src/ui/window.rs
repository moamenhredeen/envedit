//! Main window: state, message loop, layout and the Apply flow.
//!
//! The `App` lives in a thread-local `RefCell`. Messages that arrive while
//! it is borrowed (notifications caused by our own control updates) fall
//! through to `DefWindowProcW`, which is what we want for them. Dialogs are
//! only shown after the borrow is released.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_BTNFACE, GetSysColorBrush, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    EM_SETCUEBANNER, EM_SETLIMITTEXT, EM_SETSEL, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVS_EDITLABELS, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_EX_LABELTIP,
    LVS_NOSORTHEADER, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, SB_SETPARTS, SB_SETTEXTW, SBARS_SIZEGRIP,
    STATUSCLASSNAMEW, WC_BUTTONW, WC_COMBOBOXW, WC_EDITW, WC_LISTVIEWW, WC_STATICW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_DELETE, VK_DOWN, VK_F2, VK_INSERT, VK_UP};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    ACCEL, BS_PUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT, CreateAcceleratorTableW, CreateWindowExW,
    DefWindowProcW, DestroyAcceleratorTable, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, FCONTROL, FVIRTKEY, GetMessageW, GetWindowRect, IDC_ARROW,
    IDI_APPLICATION, IDNO, IDYES, IsDialogMessageW, LoadCursorW, LoadIconW, MB_ICONERROR, MB_ICONQUESTION,
    MB_ICONWARNING, MB_OK, MB_YESNOCANCEL, MINMAXINFO, MSG, MessageBoxW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SW_SHOWDEFAULT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SetWindowPos, ShowWindow,
    TranslateAcceleratorW, TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_DPICHANGED,
    WM_GETMINMAXINFO, WM_KEYDOWN, WM_NOTIFY, WM_SIZE, WM_SYSKEYDOWN, WNDCLASSEXW, WS_CLIPCHILDREN,
    WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::dpi::{self, scale};
use super::win::{self, place, send, show};
use crate::core::apply::{Outcome, VarResult, apply_scope};
use crate::core::broadcast;
use crate::core::diagnostics::ExistenceCache;
use crate::core::elevation::{self, Exchange, HelperEnd};
use crate::core::model::{Key, Scope, VALUE_LIMIT, Var};
use crate::core::registry::{EnvStore, RegistryStore};
use crate::core::snapshot;
use crate::core::staging::{PlannedChange, Row, Staging};

const CLASS: PCWSTR = w!("enveditMain");

// Control ids.
pub(super) const ID_FILTER: u16 = 100;
pub(super) const ID_DISCARD: u16 = 101;
pub(super) const ID_UNDO: u16 = 102;
pub(super) const ID_PENDING: u16 = 103;
pub(super) const ID_APPLY: u16 = 104;
pub(super) const ID_VARLIST: u16 = 110;
pub(super) const ID_NEW: u16 = 111;
pub(super) const ID_DELETE: u16 = 112;
pub(super) const ID_HEADER: u16 = 120;
pub(super) const ID_TYPE: u16 = 121;
pub(super) const ID_MODE: u16 = 122;
pub(super) const ID_LENGTH: u16 = 123;
pub(super) const ID_ENTRIES: u16 = 124;
pub(super) const ID_TEXT: u16 = 125;
pub(super) const ID_ADD: u16 = 126;
pub(super) const ID_REMOVE: u16 = 127;
pub(super) const ID_UP: u16 = 128;
pub(super) const ID_DOWN: u16 = 129;
pub(super) const ID_PREVIEW: u16 = 130;
pub(super) const ID_CREATE_TITLE: u16 = 140;
pub(super) const ID_NAME_LABEL: u16 = 141;
pub(super) const ID_NAME: u16 = 142;
pub(super) const ID_SCOPE_LABEL: u16 = 143;
pub(super) const ID_SCOPE: u16 = 144;
pub(super) const ID_VALUE_LABEL: u16 = 145;
pub(super) const ID_VALUE: u16 = 146;
pub(super) const ID_CREATE: u16 = 147;
pub(super) const ID_CANCEL_CREATE: u16 = 148;
pub(super) const ID_PENDING_LIST: u16 = 150;
pub(super) const ID_RELOAD: u16 = 151;
pub(super) const ID_BACK: u16 = 152;
pub(super) const ID_STATUS: u16 = 160;

// Commands without a control.
pub(super) const CMD_FOCUS_FILTER: u16 = 200;
pub(super) const CMD_UNDO: u16 = 201;
pub(super) const CMD_APPLY: u16 = 202;
pub(super) const CMD_EDIT_ENTRY: u16 = 203;

// Dialog-manager commands (Enter / Esc).
const IDOK: u16 = 1;
const IDCANCEL: u16 = 2;

// Messages posted by workers and to ourselves.
pub(super) const WM_APP_DIRS: u32 = WM_APP + 1;
pub(super) const WM_APP_HELPER: u32 = WM_APP + 2;
pub(super) const WM_APP_REFRESH_EDITOR: u32 = WM_APP + 3;

// Static styles not exported by the bindings.
pub(super) const SS_NOPREFIX: u32 = 0x80;
pub(super) const SS_CENTERIMAGE: u32 = 0x200;
pub(super) const SS_EDITCONTROL: u32 = 0x2000;
pub(super) const SS_ENDELLIPSIS: u32 = 0x4000;

pub(super) const GROUP_USER: i32 = 1;
pub(super) const GROUP_MACHINE: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
    Editor,
    Create,
    Pending,
}

pub(super) struct Controls {
    pub filter: HWND,
    pub discard: HWND,
    pub undo: HWND,
    pub pending: HWND,
    pub apply: HWND,
    pub varlist: HWND,
    pub new_var: HWND,
    pub delete_var: HWND,
    pub header: HWND,
    pub type_combo: HWND,
    pub mode: HWND,
    pub length: HWND,
    pub entries: HWND,
    pub text: HWND,
    pub add: HWND,
    pub remove: HWND,
    pub up: HWND,
    pub down: HWND,
    pub preview: HWND,
    pub create_title: HWND,
    pub name_label: HWND,
    pub name: HWND,
    pub scope_label: HWND,
    pub scope: HWND,
    pub value_label: HWND,
    pub value: HWND,
    pub create: HWND,
    pub cancel_create: HWND,
    pub pending_list: HWND,
    pub reload: HWND,
    pub back: HWND,
    pub status: HWND,
}

impl Controls {
    fn create(parent: HWND) -> Controls {
        let button = |text: &str, id| win::create_child(parent, WC_BUTTONW, text, WS_TABSTOP.0 | BS_PUSHBUTTON as u32, 0, id);
        let edit = |style: u32, id| {
            win::create_child(parent, WC_EDITW, "", WS_TABSTOP.0 | style, WS_EX_CLIENTEDGE.0, id)
        };
        let label = |text: &str, style: u32, id| win::create_child(parent, WC_STATICW, text, SS_NOPREFIX | style, 0, id);
        let combo = |id| {
            win::create_child(parent, WC_COMBOBOXW, "", WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST as u32, 0, id)
        };
        let list = |extra: u32, id| {
            let lv = win::create_child(
                parent,
                WC_LISTVIEWW,
                "",
                WS_TABSTOP.0 | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOSORTHEADER | extra,
                WS_EX_CLIENTEDGE.0,
                id,
            );
            let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP;
            send(lv, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as usize, ex as isize);
            lv
        };

        let c = Controls {
            filter: edit(ES_AUTOHSCROLL as u32, ID_FILTER),
            discard: button("Discard", ID_DISCARD),
            undo: button("Undo", ID_UNDO),
            pending: button("Pending", ID_PENDING),
            apply: button("Apply", ID_APPLY),
            varlist: list(0, ID_VARLIST),
            new_var: button("New", ID_NEW),
            delete_var: button("Delete", ID_DELETE),
            header: label("", SS_ENDELLIPSIS | SS_CENTERIMAGE, ID_HEADER),
            type_combo: combo(ID_TYPE),
            mode: button("Text mode", ID_MODE),
            length: label("", SS_ENDELLIPSIS, ID_LENGTH),
            entries: list(LVS_EDITLABELS, ID_ENTRIES),
            text: edit((ES_MULTILINE | ES_AUTOVSCROLL) as u32 | WS_VSCROLL.0, ID_TEXT),
            add: button("Add", ID_ADD),
            remove: button("Remove", ID_REMOVE),
            up: button("Move up", ID_UP),
            down: button("Move down", ID_DOWN),
            preview: label("", SS_EDITCONTROL, ID_PREVIEW),
            create_title: label("New variable", SS_CENTERIMAGE, ID_CREATE_TITLE),
            name_label: label("Name", SS_CENTERIMAGE, ID_NAME_LABEL),
            name: edit(ES_AUTOHSCROLL as u32, ID_NAME),
            scope_label: label("Scope", SS_CENTERIMAGE, ID_SCOPE_LABEL),
            scope: combo(ID_SCOPE),
            value_label: label("Value", SS_CENTERIMAGE, ID_VALUE_LABEL),
            value: edit((ES_MULTILINE | ES_AUTOVSCROLL) as u32 | WS_VSCROLL.0, ID_VALUE),
            create: button("Create", ID_CREATE),
            cancel_create: button("Cancel", ID_CANCEL_CREATE),
            pending_list: list(0, ID_PENDING_LIST),
            reload: button("Reload from registry", ID_RELOAD),
            back: button("Back to editor", ID_BACK),
            status: win::create_child(parent, STATUSCLASSNAMEW, "", SBARS_SIZEGRIP, 0, ID_STATUS),
        };

        send(c.filter, EM_SETCUEBANNER, 1, w!("Filter by name or value (Ctrl+F)").as_ptr() as isize);
        send(c.text, EM_SETLIMITTEXT, VALUE_LIMIT, 0);
        send(c.value, EM_SETLIMITTEXT, VALUE_LIMIT, 0);
        send(c.name, EM_SETLIMITTEXT, 255, 0);
        win::combo_fill(c.type_combo, &["REG_SZ", "REG_EXPAND_SZ"]);
        win::combo_fill(c.scope, &["User", "Machine"]);
        win::combo_select(c.scope, Some(0));
        win::lv_enable_groups(c.varlist, &[(GROUP_USER, "User"), (GROUP_MACHINE, "Machine")]);
        for (i, title) in ["Name", "Value"].iter().enumerate() {
            win::lv_add_column(c.varlist, i, title, 100);
        }
        for (i, title) in ["Entry", "Status", "Expanded"].iter().enumerate() {
            win::lv_add_column(c.entries, i, title, 100);
        }
        for (i, title) in ["Change", "Scope", "Name", "Old value", "New value", "Last result"].iter().enumerate() {
            win::lv_add_column(c.pending_list, i, title, 100);
        }
        // SAFETY: the subclass proc only posts messages to the main window passed as ref data.
        unsafe {
            let _ = SetWindowSubclass(c.entries, Some(entries_subclass), 1, parent.0 as usize);
        }
        c
    }

    fn all(&self) -> [HWND; 32] {
        [
            self.filter,
            self.discard,
            self.undo,
            self.pending,
            self.apply,
            self.varlist,
            self.new_var,
            self.delete_var,
            self.header,
            self.type_combo,
            self.mode,
            self.length,
            self.entries,
            self.text,
            self.add,
            self.remove,
            self.up,
            self.down,
            self.preview,
            self.create_title,
            self.name_label,
            self.name,
            self.scope_label,
            self.scope,
            self.value_label,
            self.value,
            self.create,
            self.cancel_create,
            self.pending_list,
            self.reload,
            self.back,
            self.status,
        ]
    }
}

/// Machine part of an Apply waiting for the elevated helper.
struct PendingApply {
    results: Vec<VarResult>,
    machine: Vec<PlannedChange>,
    exchange: Exchange,
}

pub(super) struct App {
    pub hwnd: HWND,
    pub c: Controls,
    pub dpi: u32,
    font: HFONT,
    pub store: RegistryStore,
    pub staging: Staging,
    elevated: bool,
    pub view: View,
    /// Rows currently shown in the variable list; the item param indexes this.
    pub shown_rows: Vec<Row>,
    pub selected: Option<(Scope, String)>,
    /// Per-variable list/text mode chosen by the user (true = list).
    pub mode_override: HashMap<Key, bool>,
    /// Entries shown in list mode; may hold an empty placeholder while adding.
    pub draft: Vec<String>,
    pub adding: Option<usize>,
    /// Entry to select when the editor is next filled.
    pub entry_focus: Option<usize>,
    pub text_dirty: bool,
    pub dirs: ExistenceCache,
    pub dirs_in_flight: HashSet<String>,
    /// Problems of the last Apply, shown in the pending view.
    pub last_problems: HashMap<Key, String>,
    pub pending_rows: Vec<PlannedChange>,
    applying: Option<PendingApply>,
    close_after_apply: bool,
    /// Message to show in a dialog once the App borrow is released.
    dialog: Option<String>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the App unless it is absent or already borrowed.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut app| app.as_mut().map(f)))
}

fn app_gone() -> bool {
    APP.with(|cell| cell.try_borrow().is_ok_and(|app| app.is_none()))
}

pub fn run(select: Option<String>) -> i32 {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES | ICC_STANDARD_CLASSES,
    };
    // SAFETY: icc is initialized with its size.
    unsafe {
        let _ = InitCommonControlsEx(&icc);
    }

    let store = RegistryStore::system();
    let loaded = store.load(Scope::User).and_then(|u| Ok((u, store.load(Scope::Machine)?)));
    let (user, machine) = match loaded {
        Ok(vars) => vars,
        Err(e) => {
            error_box(None, &format!("Could not read environment variables:\n\n{e}"));
            return 1;
        }
    };

    // SAFETY: standard window class registration and creation with valid, static arguments.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None).expect("module handle");
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
        let elevated = elevation::is_elevated();
        let title = if elevated { "envedit (Administrator)" } else { "envedit" };
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            &HSTRING::from(title),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("create main window")
    };

    // SAFETY: hwnd is our freshly created window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    // SAFETY: hwnd is valid.
    unsafe {
        let _ = SetWindowPos(hwnd, None, 0, 0, scale(1080, dpi), scale(700, dpi), SWP_NOMOVE | SWP_NOZORDER);
    }
    let c = Controls::create(hwnd);
    let app = App {
        hwnd,
        c,
        dpi,
        font: HFONT::default(),
        store,
        staging: Staging::new(user, machine),
        elevated: elevation::is_elevated(),
        view: View::Editor,
        shown_rows: Vec::new(),
        selected: None,
        mode_override: HashMap::new(),
        draft: Vec::new(),
        adding: None,
        entry_focus: None,
        text_dirty: false,
        dirs: ExistenceCache::default(),
        dirs_in_flight: HashSet::new(),
        last_problems: HashMap::new(),
        pending_rows: Vec::new(),
        applying: None,
        close_after_apply: false,
        dialog: None,
    };
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    with_app(|app| {
        app.apply_font();
        // Size the list before selecting, so scrolling the selection into view
        // does not happen in a zero-height list.
        app.layout();
        app.initial_select(select);
        app.update_toolbar();
        app.set_status("Ready. Changes are staged until you Apply (Ctrl+S).");
    });

    // SAFETY: hwnd is valid; the accelerator table is destroyed after the loop.
    let code = unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWDEFAULT);
        with_app(|app| {
            let _ = SetFocus(Some(app.c.varlist));
        });
        let accels = [
            ACCEL { fVirt: FVIRTKEY | FCONTROL, key: u16::from(b'F'), cmd: CMD_FOCUS_FILTER },
            ACCEL { fVirt: FVIRTKEY | FCONTROL, key: u16::from(b'Z'), cmd: CMD_UNDO },
            ACCEL { fVirt: FVIRTKEY | FCONTROL, key: u16::from(b'S'), cmd: CMD_APPLY },
        ];
        let haccel = CreateAcceleratorTableW(&accels).expect("accelerator table");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if !keeps_edit_undo(&msg) && TranslateAcceleratorW(hwnd, haccel, &msg) != 0 {
                continue;
            }
            if IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyAcceleratorTable(haccel);
        msg.wParam.0 as i32
    };
    APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().take() {
            dpi::delete_font(app.font);
        }
    });
    code
}

/// Ctrl+Z inside an edit control undoes typing there, not the staged change.
fn keeps_edit_undo(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN || msg.wParam.0 != usize::from(b'Z') {
        return false;
    }
    // SAFETY: GetFocus has no preconditions.
    let focus = unsafe { GetFocus() };
    !focus.is_invalid() && win::class_name(focus).eq_ignore_ascii_case("Edit")
}

pub(super) fn error_box(owner: Option<HWND>, text: &str) {
    // SAFETY: valid strings; owner is a live window or none.
    unsafe {
        MessageBoxW(owner, &HSTRING::from(text), w!("envedit"), MB_OK | MB_ICONERROR);
    }
}

fn show_deferred_dialog(hwnd: HWND) {
    if let Some(text) = with_app(|app| app.dialog.take()).flatten() {
        // SAFETY: hwnd is our live main window.
        unsafe {
            MessageBoxW(Some(hwnd), &HSTRING::from(text), w!("envedit"), MB_OK | MB_ICONWARNING);
        }
    }
}

fn loword(value: usize) -> u16 {
    (value & 0xffff) as u16
}

fn hiword(value: usize) -> u32 {
    ((value >> 16) & 0xffff) as u32
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            on_close(hwnd);
            return LRESULT(0);
        }
        WM_DESTROY => {
            // SAFETY: no preconditions.
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        WM_APP_DIRS | WM_APP_HELPER => {
            if app_gone() {
                drop_worker_payload(msg, lparam);
            } else if with_app(|app| app.on_worker(msg, lparam)).is_none() {
                // Busy in a nested message loop: try again once it unwinds.
                // SAFETY: re-posting our own message to our own window.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), msg, wparam, lparam);
                }
            }
            show_deferred_dialog(hwnd);
            return LRESULT(0);
        }
        _ => {}
    }
    let handled = with_app(|app| app.handle(msg, wparam, lparam)).flatten();
    show_deferred_dialog(hwnd);
    // SAFETY: forwarding the original message.
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) })
}

/// Keyboard shortcuts of the entry list that are not accelerators.
unsafe extern "system" fn entries_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    main: usize,
) -> LRESULT {
    let key = wparam.0 as u16;
    let command = match msg {
        WM_SYSKEYDOWN if key == VK_UP.0 => Some(ID_UP),
        WM_SYSKEYDOWN if key == VK_DOWN.0 => Some(ID_DOWN),
        WM_KEYDOWN if key == VK_DELETE.0 => Some(ID_REMOVE),
        WM_KEYDOWN if key == VK_INSERT.0 => Some(ID_ADD),
        WM_KEYDOWN if key == VK_F2.0 => Some(CMD_EDIT_ENTRY),
        _ => None,
    };
    match command {
        Some(id) => {
            // SAFETY: main is the main window handle passed at subclass time.
            unsafe {
                let _ = PostMessageW(Some(HWND(main as _)), WM_COMMAND, WPARAM(usize::from(id)), LPARAM(0));
            }
            LRESULT(0)
        }
        // SAFETY: forwarding to the next subclass / original proc.
        None => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

/// Moves a payload to the UI thread through a posted message.
pub(super) fn post_payload<T>(hwnd_raw: isize, msg: u32, payload: T) {
    let ptr = Box::into_raw(Box::new(payload));
    // SAFETY: posting to a window handle; on failure the payload is reclaimed here.
    unsafe {
        if PostMessageW(Some(HWND(hwnd_raw as _)), msg, WPARAM(0), LPARAM(ptr as isize)).is_err() {
            drop(Box::from_raw(ptr));
        }
    }
}

/// Takes back a payload sent with [`post_payload`].
///
/// # Safety
/// `lparam` must come from `post_payload::<T>` for this message and be taken once.
pub(super) unsafe fn take_payload<T>(lparam: LPARAM) -> T {
    // SAFETY: guaranteed by the caller.
    *unsafe { Box::from_raw(lparam.0 as *mut T) }
}

type DirResults = Vec<(String, bool)>;

fn drop_worker_payload(msg: u32, lparam: LPARAM) {
    // SAFETY: each message carries the payload type posted for it.
    unsafe {
        match msg {
            WM_APP_DIRS => drop(take_payload::<DirResults>(lparam)),
            WM_APP_HELPER => drop(take_payload::<HelperEnd>(lparam)),
            _ => {}
        }
    }
}

fn on_close(hwnd: HWND) {
    let state = with_app(|app| {
        app.commit_text();
        (app.staging.pending_count(), app.applying.is_some())
    });
    let destroy = || {
        // SAFETY: hwnd is our main window.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    };
    match state {
        None => {}
        Some((_, true)) => {
            with_app(|app| {
                app.close_after_apply = true;
                app.set_status("Will close once the Machine changes are applied.");
            });
        }
        Some((0, false)) => destroy(),
        Some((count, false)) => {
            let text = format!(
                "You have {count} pending change(s).\n\nApply them before closing?\n\n\
                 Yes: apply, then close\nNo: discard them and close\nCancel: keep editing"
            );
            // SAFETY: hwnd is our main window.
            let answer = unsafe { MessageBoxW(Some(hwnd), &HSTRING::from(text), w!("envedit"), MB_YESNOCANCEL | MB_ICONQUESTION) };
            if answer == IDYES {
                with_app(|app| {
                    app.close_after_apply = true;
                    app.apply();
                });
                show_deferred_dialog(hwnd);
            } else if answer == IDNO {
                destroy();
            }
        }
    }
}

impl App {
    fn handle(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_SIZE => {
                self.layout();
                Some(LRESULT(0))
            }
            WM_GETMINMAXINFO => {
                // SAFETY: lparam points to a MINMAXINFO for this message.
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize.x = self.s(720);
                info.ptMinTrackSize.y = self.s(440);
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                self.dpi = u32::from(loword(wparam.0));
                // SAFETY: lparam points to the suggested RECT for this message.
                let r = unsafe { *(lparam.0 as *const RECT) };
                self.apply_font();
                // SAFETY: hwnd is valid.
                unsafe {
                    let _ = SetWindowPos(self.hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                self.layout();
                Some(LRESULT(0))
            }
            WM_COMMAND => {
                self.on_command(loword(wparam.0), hiword(wparam.0));
                Some(LRESULT(0))
            }
            WM_NOTIFY => self.on_notify(lparam),
            WM_APP_REFRESH_EDITOR => {
                self.load_editor();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn on_command(&mut self, id: u16, code: u32) {
        use windows::Win32::UI::WindowsAndMessaging::{CBN_SELCHANGE, EN_CHANGE, EN_KILLFOCUS};
        match id {
            ID_FILTER if code == EN_CHANGE => self.rebuild_var_list(),
            ID_TEXT if code == EN_CHANGE => self.on_text_changed(),
            ID_TEXT if code == EN_KILLFOCUS => self.commit_text(),
            ID_TYPE if code == CBN_SELCHANGE => self.on_type_selected(),
            ID_FILTER | ID_TEXT | ID_TYPE | ID_NAME | ID_VALUE | ID_SCOPE => {}
            ID_DISCARD => self.discard(),
            ID_UNDO | CMD_UNDO => self.undo(),
            ID_PENDING => self.toggle_pending(),
            ID_BACK => self.show_view(View::Editor),
            ID_APPLY | CMD_APPLY => self.apply(),
            CMD_FOCUS_FILTER => {
                // SAFETY: filter is a live child window.
                unsafe {
                    let _ = SetFocus(Some(self.c.filter));
                }
                send(self.c.filter, EM_SETSEL, 0, -1);
            }
            ID_NEW => self.begin_create(),
            ID_DELETE => self.delete_selected(),
            ID_MODE => self.toggle_mode(),
            ID_ADD => self.add_entry(),
            ID_REMOVE => self.remove_entry(),
            ID_UP => self.move_entry(-1),
            ID_DOWN => self.move_entry(1),
            CMD_EDIT_ENTRY => self.edit_entry(),
            ID_CREATE => self.create_var(),
            ID_CANCEL_CREATE => self.show_view(View::Editor),
            IDOK if self.view == View::Create => self.create_var(),
            IDCANCEL if self.view == View::Create => self.show_view(View::Editor),
            ID_RELOAD => self.reload_selected_pending(),
            _ => {}
        }
    }

    pub(super) fn s(&self, value: i32) -> i32 {
        scale(value, self.dpi)
    }

    fn apply_font(&mut self) {
        let old = self.font;
        self.font = dpi::message_font(self.dpi);
        for hwnd in self.c.all() {
            win::set_font(hwnd, self.font);
        }
        dpi::delete_font(old);
    }

    pub(super) fn set_status(&mut self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        send(self.c.status, SB_SETTEXTW, 1, wide.as_ptr() as isize);
    }

    /// Enables toolbar buttons and updates the pending count.
    pub(super) fn update_toolbar(&mut self) {
        let pending = self.staging.pending_count();
        let busy = self.applying.is_some();
        win::enable(self.c.apply, pending > 0 && !busy);
        win::enable(self.c.discard, pending > 0 && !busy);
        win::enable(self.c.undo, (self.staging.can_undo() || self.text_dirty) && !busy);
        win::set_text(self.c.pending, &if self.view == View::Pending { "Editor".to_string() } else { format!("Pending ({pending})") });
        let count = match pending {
            0 => "No pending changes".to_string(),
            1 => "1 pending change".to_string(),
            n => format!("{n} pending changes"),
        };
        let wide: Vec<u16> = count.encode_utf16().chain(std::iter::once(0)).collect();
        send(self.c.status, SB_SETTEXTW, 0, wide.as_ptr() as isize);
    }

    /// Refreshes everything that depends on staged state, except the editor contents.
    pub(super) fn after_change(&mut self) {
        self.refresh_var_list();
        if self.view == View::Pending {
            self.fill_pending();
        }
        self.update_toolbar();
    }

    pub(super) fn show_view(&mut self, view: View) {
        if self.view == View::Editor && view != View::Editor {
            self.commit_text();
        }
        self.view = view;
        match view {
            View::Editor => self.load_editor(),
            View::Pending => self.fill_pending(),
            View::Create => {}
        }
        self.layout();
        self.update_toolbar();
    }

    fn toggle_pending(&mut self) {
        let next = if self.view == View::Pending { View::Editor } else { View::Pending };
        self.show_view(next);
    }

    fn undo(&mut self) {
        if self.text_dirty {
            // Typing not yet staged: undo throws it away.
            self.text_dirty = false;
            self.load_editor();
            self.set_status("Discarded unsaved typing.");
        } else if self.staging.undo() {
            self.set_status("Undid the last change.");
            self.after_change();
            self.load_editor();
        } else {
            self.set_status("Nothing to undo.");
        }
        self.update_toolbar();
    }

    fn discard(&mut self) {
        self.text_dirty = false;
        let count = self.staging.pending_count();
        self.staging.discard_all();
        self.last_problems.clear();
        self.after_change();
        self.load_editor();
        self.set_status(&format!("Discarded {count} pending change(s). Undo (Ctrl+Z) brings them back."));
    }

    // ---- layout ----

    pub(super) fn layout(&mut self) {
        let mut rc = RECT::default();
        // SAFETY: hwnd and status are valid; rc is a local.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut rc);
        }
        send(self.c.status, WM_SIZE, 0, 0);
        let parts = [self.s(170), -1];
        send(self.c.status, SB_SETPARTS, parts.len(), parts.as_ptr() as isize);
        let mut src = RECT::default();
        // SAFETY: status is valid; src is a local.
        unsafe {
            let _ = GetWindowRect(self.c.status, &mut src);
        }
        let status_h = src.bottom - src.top;

        let (w, h) = (rc.right, rc.bottom - status_h);
        let m = self.s(8);
        let gap = self.s(6);
        let bh = self.s(26);
        let bw = self.s(84);

        // Top row: filter and toolbar.
        let left_w = (w * 34 / 100).clamp(self.s(240), self.s(480));
        place(self.c.filter, m, m, left_w, bh);
        let mut x = w - m;
        for button in [self.c.apply, self.c.pending, self.c.undo, self.c.discard] {
            let bw = if button == self.c.pending { self.s(110) } else { bw };
            x -= bw;
            place(button, x, m, bw, bh);
            x -= gap;
        }

        // Left: variable list with New / Delete below.
        let top = m + bh + m;
        let bottom = h - m;
        place(self.c.varlist, m, top, left_w, bottom - top - bh - gap);
        place(self.c.new_var, m, bottom - bh, bw, bh);
        place(self.c.delete_var, m + bw + gap, bottom - bh, bw, bh);
        let name_w = self.s(170);
        win::lv_set_column_width(self.c.varlist, 0, name_w);
        win::lv_set_column_width(self.c.varlist, 1, (left_w - name_w - self.s(28)).max(self.s(60)));

        // Right pane.
        let rx = m + left_w + m;
        let rw = (w - rx - m).max(0);
        let editor = self.view == View::Editor;
        let create = self.view == View::Create;
        let pending = self.view == View::Pending;

        // Editor.
        let list_mode = editor && self.editor_list_mode();
        for hwnd in [self.c.header, self.c.type_combo, self.c.mode, self.c.length, self.c.preview] {
            show(hwnd, editor);
        }
        for hwnd in [self.c.entries, self.c.add, self.c.remove, self.c.up, self.c.down] {
            show(hwnd, list_mode);
        }
        show(self.c.text, editor && !list_mode);
        if editor {
            let combo_w = self.s(140);
            let mode_w = self.s(100);
            place(self.c.mode, rx + rw - mode_w, top, mode_w, bh);
            place(self.c.type_combo, rx + rw - mode_w - gap - combo_w, top, combo_w, self.s(200));
            place(self.c.header, rx, top, (rw - mode_w - combo_w - 2 * gap).max(0), bh);
            let len_y = top + bh + gap;
            let line = self.s(20);
            place(self.c.length, rx, len_y, rw, line);
            let preview_h = self.s(54);
            let body_y = len_y + line + gap;
            let preview_y = bottom - preview_h;
            let buttons_y = preview_y - gap - bh;
            if list_mode {
                place(self.c.entries, rx, body_y, rw, buttons_y - gap - body_y);
                let mut bx = rx;
                for button in [self.c.add, self.c.remove, self.c.up, self.c.down] {
                    place(button, bx, buttons_y, bw, bh);
                    bx += bw + gap;
                }
                let entry_w = rw * 45 / 100;
                let status_w = rw * 22 / 100;
                win::lv_set_column_width(self.c.entries, 0, entry_w);
                win::lv_set_column_width(self.c.entries, 1, status_w);
                win::lv_set_column_width(self.c.entries, 2, (rw - entry_w - status_w - self.s(28)).max(self.s(60)));
            } else {
                place(self.c.text, rx, body_y, rw, preview_y - gap - body_y);
            }
            place(self.c.preview, rx, preview_y, rw, preview_h);
        }

        // Create.
        for hwnd in [
            self.c.create_title,
            self.c.name_label,
            self.c.name,
            self.c.scope_label,
            self.c.scope,
            self.c.value_label,
            self.c.value,
            self.c.create,
            self.c.cancel_create,
        ] {
            show(hwnd, create);
        }
        if create {
            let label_w = self.s(60);
            let field_x = rx + label_w + gap;
            let field_w = (rw - label_w - gap).max(0);
            place(self.c.create_title, rx, top, rw, bh);
            let mut y = top + bh + gap;
            place(self.c.name_label, rx, y, label_w, bh);
            place(self.c.name, field_x, y, field_w, bh);
            y += bh + gap;
            place(self.c.scope_label, rx, y, label_w, bh);
            place(self.c.scope, field_x, y, self.s(140), self.s(120));
            y += bh + gap;
            place(self.c.value_label, rx, y, label_w, bh);
            place(self.c.value, field_x, y, field_w, self.s(120));
            y += self.s(120) + gap;
            place(self.c.create, field_x, y, bw, bh);
            place(self.c.cancel_create, field_x + bw + gap, y, bw, bh);
        }

        // Pending.
        for hwnd in [self.c.pending_list, self.c.reload, self.c.back] {
            show(hwnd, pending);
        }
        if pending {
            place(self.c.pending_list, rx, top, rw, bottom - top - bh - gap);
            let reload_w = self.s(150);
            place(self.c.reload, rx, bottom - bh, reload_w, bh);
            place(self.c.back, rx + reload_w + gap, bottom - bh, self.s(120), bh);
            let widths = [9, 8, 15, 25, 25, 18];
            let usable = rw - self.s(28);
            for (i, pct) in widths.iter().enumerate() {
                win::lv_set_column_width(self.c.pending_list, i, usable * pct / 100);
            }
        }
    }

    // ---- workers ----

    fn on_worker(&mut self, msg: u32, lparam: LPARAM) {
        // SAFETY: each message carries the payload type posted for it, taken once here.
        unsafe {
            match msg {
                WM_APP_DIRS => self.on_dirs_checked(take_payload::<DirResults>(lparam)),
                WM_APP_HELPER => self.on_helper_done(take_payload::<HelperEnd>(lparam)),
                _ => {}
            }
        }
    }

    // ---- apply ----

    pub(super) fn apply(&mut self) {
        if self.applying.is_some() {
            return;
        }
        self.commit_text();
        let planned = self.staging.planned();
        if planned.is_empty() {
            self.set_status("Nothing to apply.");
            self.close_after_apply = false;
            return;
        }
        let (user, machine): (Vec<_>, Vec<_>) = planned.into_iter().partition(|c| c.scope == Scope::User);

        let mut results = apply_scope(&mut self.store, Scope::User, &user, &mut |vars| take_snapshot(Scope::User, vars));
        if machine.is_empty() {
            self.finish_apply(results);
        } else if self.elevated {
            results.extend(apply_scope(&mut self.store, Scope::Machine, &machine, &mut |vars| {
                take_snapshot(Scope::Machine, vars)
            }));
            self.finish_apply(results);
        } else {
            self.start_helper(results, machine);
        }
    }

    /// Snapshots Machine as the invoking user, then hands Machine changes to the elevated helper.
    fn start_helper(&mut self, mut results: Vec<VarResult>, machine: Vec<PlannedChange>) {
        let prepared = self
            .store
            .load(Scope::Machine)
            .and_then(|vars| take_snapshot(Scope::Machine, &vars))
            .map_err(|e| format!("not written, snapshot failed: {e}"))
            .and_then(|()| Exchange::create(&snapshot::app_data_dir()?, &machine));
        let exchange = match prepared {
            Ok(exchange) => exchange,
            Err(e) => {
                results.extend(fail_all(&machine, &e));
                self.finish_apply(results);
                return;
            }
        };
        let hwnd_raw = self.hwnd.0 as isize;
        let worker_exchange = exchange.clone();
        std::thread::spawn(move || {
            let end = elevation::run_elevated_helper(&worker_exchange);
            post_payload(hwnd_raw, WM_APP_HELPER, end);
        });
        self.applying = Some(PendingApply { results, machine, exchange });
        self.update_toolbar();
        self.set_status("Waiting for administrator approval to apply Machine changes…");
    }

    fn on_helper_done(&mut self, end: HelperEnd) {
        let Some(PendingApply { mut results, machine, exchange }) = self.applying.take() else { return };
        match end {
            HelperEnd::Cancelled => results.extend(fail_all(&machine, "administrator approval was declined")),
            HelperEnd::LaunchFailed(e) => results.extend(fail_all(&machine, &format!("could not start the elevated helper: {e}"))),
            HelperEnd::Exited(code) => {
                let decoded = std::fs::read_to_string(&exchange.result)
                    .map_err(|e| e.to_string())
                    .and_then(|text| elevation::decode_results(&text));
                match decoded {
                    Ok(helper_results) if code == 0 => results.extend(helper_results),
                    _ => results.extend(fail_all(&machine, &format!("the elevated helper failed (exit code {code})"))),
                }
            }
        }
        exchange.remove();
        self.finish_apply(results);
    }

    fn finish_apply(&mut self, results: Vec<VarResult>) {
        let reloaded = self.store.load(Scope::User).and_then(|u| Ok((u, self.store.load(Scope::Machine)?)));
        match reloaded {
            Ok((user, machine)) => self.staging.replace_loaded(user, machine),
            Err(e) => self.dialog = Some(format!("Changes were written, but reloading failed:\n\n{e}")),
        }

        let applied: Vec<&VarResult> = results.iter().filter(|r| r.outcome == Outcome::Applied).collect();
        let problems: Vec<&VarResult> = results.iter().filter(|r| r.outcome != Outcome::Applied).collect();
        for r in &applied {
            self.last_problems.remove(&Key::new(r.scope, &r.name));
        }
        for r in &problems {
            let text = match &r.outcome {
                Outcome::Conflict(m) => format!("conflict: {m}"),
                Outcome::Failed(m) => format!("failed: {m}"),
                Outcome::Applied => unreachable!(),
            };
            self.last_problems.insert(Key::new(r.scope, &r.name), text);
        }
        if !applied.is_empty() {
            broadcast::environment_changed_async();
        }

        let names = |items: &[&VarResult]| {
            items.iter().map(|r| format!("{} {}", r.scope.label(), r.name)).collect::<Vec<_>>().join(", ")
        };
        let mut status = if applied.is_empty() {
            "Nothing was applied.".to_string()
        } else {
            format!("Applied: {}. Open terminals need a restart to see changes.", names(&applied))
        };
        if !problems.is_empty() {
            status.push_str(&format!(" Not applied: {} (see Pending).", names(&problems)));
            let mut text = String::from("Some changes were not applied:\n\n");
            for r in &problems {
                text.push_str(&format!("{} {}: {}\n", r.scope.label(), r.name, self.last_problems[&Key::new(r.scope, &r.name)]));
            }
            if !applied.is_empty() {
                text.push_str(&format!("\nApplied: {}.\n", names(&applied)));
            }
            text.push_str(
                "\nThe changes that were not applied stay pending. In the Pending view, \
                 \"Reload from registry\" takes the current registry value; Apply again overwrites it.",
            );
            self.dialog = Some(text);
        }
        self.set_status(&status);
        self.after_change();
        if self.view == View::Editor {
            self.load_editor();
        }

        if self.close_after_apply {
            self.close_after_apply = false;
            if self.staging.pending_count() == 0 {
                // SAFETY: posting WM_CLOSE to our own window.
                unsafe {
                    let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
}

fn take_snapshot(scope: Scope, vars: &[Var]) -> Result<(), String> {
    snapshot::write(&snapshot::snapshot_dir()?, scope, vars, SystemTime::now()).map(|_| ())
}

fn fail_all(changes: &[PlannedChange], reason: &str) -> Vec<VarResult> {
    changes
        .iter()
        .map(|c| VarResult { scope: c.scope, name: c.name.clone(), outcome: Outcome::Failed(reason.to_string()) })
        .collect()
}
