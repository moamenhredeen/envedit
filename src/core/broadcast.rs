//! Telling running programs that the environment changed.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
use windows::core::w;

const HWND_BROADCAST: HWND = HWND(0xffff as _);

/// Per-window timeout in milliseconds.
const TIMEOUT_MS: u32 = 5000;

/// Broadcasts `WM_SETTINGCHANGE("Environment")` to all top-level windows.
/// Blocks until every window answered or timed out, so call it from a worker thread.
pub fn environment_changed() {
    let param = w!("Environment");
    // SAFETY: the string is a static NUL-terminated literal; the result pointer is not needed.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(param.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            TIMEOUT_MS,
            None,
        );
    }
}

/// Runs the broadcast on a background thread.
pub fn environment_changed_async() {
    std::thread::spawn(environment_changed);
}
