use crate::config::Config;
use crate::scan;
use crate::ui;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_BTNFACE, COLOR_WINDOW, HBRUSH, HGDIOBJ};
use windows::Win32::UI::Controls::{IPM_GETADDRESS, IPM_SETADDRESS};
use windows::Win32::UI::WindowsAndMessaging::*;

const IDC_IP_LABEL: usize = 101;
const IDC_IP: usize = 102;
const IDC_SCAN: usize = 103;
const IDC_APPID_LABEL: usize = 104;
const IDC_APPID: usize = 105;
const IDC_SAVE: usize = 106;
const IDC_CANCEL: usize = 107;

#[allow(dead_code)]
struct SettingsDialog {
    hwnd: HWND,
    h_ip: HWND,
    h_appid: HWND,
    closed: AtomicBool,
    saved: bool,
    config: Config,
}

/// Shows the settings dialog. Returns `true` if the user pressed Save.
pub fn show_settings(config: &mut Config) -> bool {
    static OPEN: AtomicBool = AtomicBool::new(false);
    if OPEN.swap(true, Ordering::SeqCst) {
        return false; // already open, don't nest
    }
    let result = unsafe { show_settings_inner(config) };
    OPEN.store(false, Ordering::SeqCst);
    result
}

unsafe fn show_settings_inner(config: &mut Config) -> bool {
    unsafe {
register_class();

        let dlg = SettingsDialog {
            hwnd: HWND::default(),
            h_ip: HWND::default(),
            h_appid: HWND::default(),
            closed: AtomicBool::new(false),
            saved: false,
            config: config.clone(),
        };
        let ptr = Box::into_raw(Box::new(dlg));

        let hwnd = create_settings_window(ptr);
        if hwnd.0.is_null() {
            drop(Box::from_raw(ptr));
            return false;
        }

        let dlg = &mut *ptr;
        ui::run_modal(hwnd, &dlg.closed);
        let saved = dlg.saved;
        if saved {
            *config = dlg.config.clone();
        }
        drop(Box::from_raw(ptr));
        saved
    }
}

unsafe fn create_settings_window(ptr: *mut SettingsDialog) -> HWND {
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let ex_style = WS_EX_DLGMODALFRAME;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 344,
        bottom: 180,
        ..Default::default()
    };
    let _ = AdjustWindowRectEx(&mut rect, style, false, ex_style);
let hwnd = CreateWindowExW(
        ex_style,
        w!("CellPresenceSettings"),
        w!("CellPresence Settings"),
        style,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        rect.right - rect.left,
        rect.bottom - rect.top,
        None,
        None,
        ui::instance_handle(),
        Some(ptr as *const core::ffi::c_void),
    )
    .unwrap_or_default();
    if !hwnd.0.is_null() {
let _ = ShowWindow(hwnd, SW_SHOW);
        ui::center_window(hwnd);
    }
    hwnd
}

fn register_class() {
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    unsafe {
        let wc = WNDCLASSW {
            style: WNDCLASS_STYLES(0),
            lpfnWndProc: Some(settings_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: ui::instance_handle(),
            hIcon: ui::default_icon(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH(((COLOR_BTNFACE.0 + 1) as isize) as *mut core::ffi::c_void),
            lpszMenuName: PCWSTR::null(),
lpszClassName: w!("CellPresenceSettings"),
        };
        let _ = RegisterClassW(&wc);
    }
}

unsafe extern "system" fn settings_proc(
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
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsDialog;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let this = &mut *ptr;
    match msg {
WM_CREATE => {
            this.on_create(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => this.on_command(wparam),
        WM_CTLCOLORSTATIC => LRESULT(ui::btnface_brush().0 as isize),
        WM_CTLCOLORBTN => LRESULT(ui::btnface_brush().0 as isize),
        WM_CTLCOLOREDIT => LRESULT(brush_color_window().0 as isize),
        WM_CLOSE => {
            this.on_cancel();
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn brush_color_window() -> HBRUSH {
    unsafe { windows::Win32::Graphics::Gdi::GetSysColorBrush(COLOR_WINDOW) }
}

impl SettingsDialog {
    unsafe fn on_create(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        let font = ui::gui_font();
        let inst = ui::instance_handle();

        create_label(hwnd, inst, "PS3 IP Address", 16, 18, 180, 18, IDC_IP_LABEL, &font);
        create_label(
            hwnd,
            inst,
            "Discord Application ID",
            16,
            78,
            220,
            18,
            IDC_APPID_LABEL,
            &font,
        );

        self.h_ip = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("SysIPAddress32"),
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
            16,
            40,
            140,
            22,
            hwnd,
            HMENU(IDC_IP as isize as *mut core::ffi::c_void),
            inst,
            None,
        )
        .unwrap_or_default();

        self.h_appid = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | (ES_AUTOHSCROLL as u32)),
            16,
            100,
            250,
            22,
            hwnd,
            HMENU(IDC_APPID as isize as *mut core::ffi::c_void),
            inst,
            None,
        )
        .unwrap_or_default();

        create_button(hwnd, inst, "SCAN", 166, 40, 64, 24, IDC_SCAN, false, &font);
        create_button(hwnd, inst, "Save", 200, 140, 62, 24, IDC_SAVE, true, &font);
        create_button(hwnd, inst, "Cancel", 268, 140, 62, 24, IDC_CANCEL, false, &font);

        for control in [self.h_ip, self.h_appid] {
            let _ = SendMessageW(control, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }

        // Pre-fill values from the saved configuration.
        if let Some(ip) = &self.config.ps3_ip {
            if let Ok(ipv4) = ip.parse::<Ipv4Addr>() {
                let o = ipv4.octets();
                let value = (o[0] as u32)
                    | ((o[1] as u32) << 8)
                    | ((o[2] as u32) << 16)
                    | ((o[3] as u32) << 24);
                let _ = SendMessageW(self.h_ip, IPM_SETADDRESS, WPARAM(0), LPARAM(value as isize));
            }
        }
        if let Some(app_id) = &self.config.discord_app_id {
            let _ = SetWindowTextW(self.h_appid, &HSTRING::from(app_id.as_str()));
        }
    }

    fn on_command(&mut self, wparam: WPARAM) -> LRESULT {
        match (wparam.0 & 0xffff) as usize {
            IDC_SCAN => self.on_scan(),
            IDC_SAVE => self.on_save(),
            IDC_CANCEL | ui::IDCANCEL => self.on_cancel(),
            _ => {}
        }
        LRESULT(0)
    }

    fn on_scan(&mut self) {
        if let Some(ip) = scan::run_scan(Some(self.hwnd)) {
            if let Ok(ipv4) = ip.parse::<Ipv4Addr>() {
                let o = ipv4.octets();
                let value = (o[0] as u32)
                    | ((o[1] as u32) << 8)
                    | ((o[2] as u32) << 16)
                    | ((o[3] as u32) << 24);
                unsafe {
                    let _ =
                        SendMessageW(self.h_ip, IPM_SETADDRESS, WPARAM(0), LPARAM(value as isize));
                }
            }
        }
    }

    fn on_save(&mut self) {
        // Read the IP address control.
        let mut value: u32 = 0;
        unsafe {
            let _ = SendMessageW(
                self.h_ip,
                IPM_GETADDRESS,
                WPARAM(0),
                LPARAM(&mut value as *mut u32 as isize),
            );
        }
let ps3_ip = if value == u32::MAX || value == 0 {
            None
        } else {
            let o = [
                (value & 0xff) as u8,
                ((value >> 8) & 0xff) as u8,
                ((value >> 16) & 0xff) as u8,
                ((value >> 24) & 0xff) as u8,
            ];
            Some(format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3]))
        };

        // Read the Discord application id.
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(self.h_appid, &mut buf) };
        let discord_app_id = if n > 0 {
            let text = String::from_utf16_lossy(&buf[..n as usize]);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        } else {
            None
        };

        let mut config = self.config.clone();
        config.ps3_ip = ps3_ip;
        config.discord_app_id = discord_app_id;

        match config.save() {
            Ok(()) => {
                self.config = config;
                self.saved = true;
                self.close();
            }
            Err(e) => {
                unsafe {
                    let _ = MessageBoxW(
                        self.hwnd,
                        &HSTRING::from(format!("Could not save settings:\n{e}")),
                        &HSTRING::from("CellPresence"),
                        MESSAGEBOX_STYLE(MB_OK.0 | MB_ICONERROR.0),
                    );
                }
            }
        }
    }

    fn on_cancel(&mut self) {
        self.close();
    }

    fn close(&mut self) {
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
        self.closed.store(true, Ordering::SeqCst);
    }
}

unsafe fn create_label(
    hwnd: HWND,
    inst: HINSTANCE,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: usize,
    font: &HGDIOBJ,
) -> HWND {
    let label = CreateWindowExW(
        WS_EX_TRANSPARENT,
        w!("STATIC"),
        &HSTRING::from(text),
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
        x,
        y,
        w,
        h,
        hwnd,
        HMENU(id as isize as *mut core::ffi::c_void),
        inst,
        None,
    )
    .unwrap_or_default();
    let _ = SendMessageW(label, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
    label
}

unsafe fn create_button(
    hwnd: HWND,
    inst: HINSTANCE,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: usize,
    default: bool,
    font: &HGDIOBJ,
) -> HWND {
    let btn_style = if default {
        BS_DEFPUSHBUTTON as u32
    } else {
        BS_PUSHBUTTON as u32
    };
    let button = CreateWindowExW(
        WS_EX_TRANSPARENT,
        w!("BUTTON"),
        &HSTRING::from(text),
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | btn_style),
        x,
        y,
        w,
        h,
        hwnd,
        HMENU(id as isize as *mut core::ffi::c_void),
        inst,
        None,
    )
    .unwrap_or_default();
    let _ = SendMessageW(button, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
    button
}
