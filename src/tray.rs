use std::mem::size_of;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICONDATAW, NOTIFYICON_VERSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, MF_SEPARATOR, MF_STRING,
    PostMessageW, SetForegroundWindow, TrackPopupMenu, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TPM_TOPALIGN, WM_APP, WM_COMMAND, WM_LBUTTONUP, WM_RBUTTONUP,
};

/// Custom message sent to the host window when the tray icon is clicked.
pub const WM_TRAYICON: u32 = WM_APP + 1;

pub const ID_SETTINGS: usize = 1001;
pub const ID_EXIT: usize = 1002;

const TRAY_ID: u32 = 1;

pub struct TrayIcon {
    data: NOTIFYICONDATAW,
}

impl TrayIcon {
    /// Registers the tray icon. Returns `None` if the shell refused it.
    pub fn add(hwnd: HWND) -> Option<TrayIcon> {
        unsafe {
            let mut data = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_ID,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
                hIcon: crate::ui::default_icon(),
                ..Default::default()
            };
            set_tip(&mut data.szTip, "CellPresence");
            if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                return None;
            }
            data.Anonymous.uVersion = NOTIFYICON_VERSION;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
            Some(TrayIcon { data })
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
    }
}

fn set_tip(buf: &mut [u16; 128], text: &str) {
    for (slot, ch) in buf.iter_mut().zip(text.encode_utf16().chain(std::iter::once(0))) {
        *slot = ch;
    }
}

/// Handles `WM_TRAYICON` on the host window: clicking the icon pops the menu.
pub fn on_tray_message(hwnd: HWND, lparam: LPARAM) {
    match lparam.0 as u32 {
        WM_LBUTTONUP | WM_RBUTTONUP => show_menu(hwnd),
        _ => {}
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, &HSTRING::from("Open Settings"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, &HSTRING::new());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, &HSTRING::from("Exit CellPresence"));

        let _ = SetForegroundWindow(hwnd);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let cmd = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        )
        .0;
        if cmd != 0 {
            let _ = PostMessageW(hwnd, WM_COMMAND, WPARAM(cmd as usize), LPARAM(0));
        }
        let _ = DestroyMenu(menu);
    }
}