use std::sync::atomic::AtomicBool;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetStockObject, GetSysColorBrush, COLOR_BTNFACE, DEFAULT_GUI_FONT, HBRUSH, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, GetSystemMetrics, GetWindowRect, HICON, HWND_TOP,
    IsDialogMessageW, LoadIconW, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowPos, TranslateMessage,
};

pub const IDCANCEL: usize = 2;

/// Pump messages until `done` is set. `hwnd` is treated as a dialog so that
/// Enter/Escape/Tab navigation behave like a real dialog.
///
/// # Safety
/// `hwnd` must be a valid window handle for the lifetime of this loop.
pub unsafe fn run_modal(hwnd: HWND, done: &AtomicBool) {
    let mut msg = MSG::default();
    while !done.load(std::sync::atomic::Ordering::SeqCst) {
        let result = GetMessageW(&mut msg, HWND::default(), 0, 0);
        let code = result.0;
        if code == 0 {
            break; // WM_QUIT
        }
        if code > 0 {
            if IsDialogMessageW(hwnd, &msg).0 == 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// The font that standard dialogs use.
pub fn gui_font() -> HGDIOBJ {
    unsafe { GetStockObject(DEFAULT_GUI_FONT) }
}

/// A system brush painted in COLOR_BTNFACE (dialog gray).
pub fn btnface_brush() -> HBRUSH {
    unsafe { GetSysColorBrush(COLOR_BTNFACE) }
}

/// Centres `hwnd` on the primary display.
pub unsafe fn center_window(hwnd: HWND) {
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return;
    }
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    let x = (screen_w - w) / 2;
    let y = (screen_h - h) / 2;
    let _ = SetWindowPos(hwnd, HWND_TOP, x, y, w, h, SWP_NOSIZE | SWP_NOZORDER);
}

/// The process instance handle, used when creating windows.
pub fn instance_handle() -> HINSTANCE {
    unsafe {
        GetModuleHandleW(None)
            .map(|m| HINSTANCE(m.0))
            .unwrap_or_default()
    }
}

/// The standard application icon.
pub fn default_icon() -> HICON {
    unsafe { LoadIconW(None, PCWSTR::from_raw(32512usize as *const u16)).unwrap_or_default() }
}