//! DPI scaling and the DPI-dependent UI font.

use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, HFONT, HGDIOBJ};
use windows::Win32::UI::HiDpi::SystemParametersInfoForDpi;
use windows::Win32::UI::WindowsAndMessaging::{NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS};

pub const BASE_DPI: u32 = 96;

/// Scales a length given at 96 DPI to `dpi`.
pub fn scale(value: i32, dpi: u32) -> i32 {
    (i64::from(value) * i64::from(dpi) / i64::from(BASE_DPI)) as i32
}

/// The system message font at `dpi`. Owned; release with [`delete_font`].
pub fn message_font(dpi: u32) -> HFONT {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    // SAFETY: metrics is a NONCLIENTMETRICSW with cbSize set.
    let ok = unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some((&raw mut metrics).cast()),
            0,
            dpi,
        )
    };
    if ok.is_err() {
        return HFONT::default();
    }
    // SAFETY: valid LOGFONTW from the system.
    unsafe { CreateFontIndirectW(&metrics.lfMessageFont) }
}

pub fn delete_font(font: HFONT) {
    if !font.is_invalid() {
        // SAFETY: font was created by message_font and is no longer selected anywhere we own.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_lengths() {
        assert_eq!(scale(100, 96), 100);
        assert_eq!(scale(100, 144), 150);
        assert_eq!(scale(8, 120), 10);
    }
}
