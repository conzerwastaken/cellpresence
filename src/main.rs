#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

mod config;
mod scan;
mod settings;
mod tray;
mod ui;

use std::mem::size_of;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, INITCOMMONCONTROLSEX, ICC_INTERNET_CLASSES,
};
use windows::Win32::UI::WindowsAndMessaging::*;

fn main() {
    unsafe {
        let cc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_INTERNET_CLASSES,
        };
        let _ = InitCommonControlsEx(&cc);

        let mut config = config::Config::load();

        register_host_class();

        let host_hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("CellPresenceHost"),
            w!("CellPresence"),
            WINDOW_STYLE(0),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            ui::instance_handle(),
            Some(&mut config as *mut config::Config as *const core::ffi::c_void),
        )
        .unwrap_or_default();

        let _tray = tray::TrayIcon::add(host_hwnd);

        // First run: no configuration anywhere yet, so open settings for setup.
        if !config::Config::exists() {
            settings::show_settings(&mut config);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn register_host_class() {
    unsafe {
        let wc = WNDCLASSW {
            style: WNDCLASS_STYLES(0),
            lpfnWndProc: Some(host_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: ui::instance_handle(),
            hIcon: ui::default_icon(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: w!("CellPresenceHost"),
        };
        let _ = RegisterClassW(&wc);
    }
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = lparam.0 as *const CREATESTRUCTW;
        let this = (*cs).lpCreateParams as isize;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, this);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    if msg == tray::WM_TRAYICON {
        tray::on_tray_message(hwnd, lparam);
        return LRESULT(0);
    }
    if msg == WM_COMMAND {
        match (wparam.0 & 0xffff) as usize {
            tray::ID_SETTINGS => {
                let cfg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut config::Config;
                if !cfg.is_null() {
                    settings::show_settings(&mut *cfg);
                }
            }
            tray::ID_EXIT => {
                PostQuitMessage(0);
            }
            _ => {}
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}