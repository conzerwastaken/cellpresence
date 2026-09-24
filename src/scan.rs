use crate::ui;
use std::mem::size_of;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{COLOR_BTNFACE, HBRUSH, HGDIOBJ};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
    GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Shared state read by the scan worker threads and the dialog timer.
pub struct ScanShared {
    pub found: Vec<String>,
    pub done: bool,
    pub cancelled: bool,
    pub total: usize,
}

impl ScanShared {
    fn new() -> Self {
        ScanShared {
            found: Vec::new(),
            done: false,
            cancelled: false,
            total: 0,
        }
    }
}

const IDC_STATUS: usize = 3001;
const IDC_LIST: usize = 3002;
const IDC_USE: usize = 3003;
const IDC_CANCEL_BTN: usize = 3004;

const TIMER_SCAN: usize = 1;

pub struct ScanDialog {
    hwnd: HWND,
    h_list: HWND,
    h_status: HWND,
    shared: Arc<Mutex<ScanShared>>,
    closed: AtomicBool,
    result: Option<String>,
    items: Vec<String>,
    dots: u32,
    finished: bool,
}

/// Enumerates IPv4 hosts on every local subnet the machine belongs to,
/// deduplicated across adapters. Result is capped to keep scans fast.
pub fn subnet_hosts() -> Vec<IpAddr> {
    let mut hosts = std::collections::BTreeSet::new();
    let adapters = unsafe { local_ipv4_ranges() };
    for (ip, prefix) in adapters {
        let n = u32::from_be_bytes(ip.octets());
        let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
        let base = n & mask;
        let last = base | !mask;
        let mut cur = base.wrapping_add(1);
        while cur < last && hosts.len() < 4096 {
            if cur != n {
                hosts.insert(Ipv4Addr::from(cur));
            }
            cur = cur.wrapping_add(1);
        }
    }
    hosts.into_iter().map(IpAddr::V4).collect()
}

unsafe fn local_ipv4_ranges() -> Vec<(Ipv4Addr, u8)> {
    let mut out = Vec::new();
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let family = AF_INET.0 as u32;

    let mut size: u32 = 0;
    let first = GetAdaptersAddresses(family, flags, None, None, &mut size);
    if first != ERROR_BUFFER_OVERFLOW.0 && first != ERROR_SUCCESS.0 {
        return out;
    }
    if size == 0 {
        return out;
    }

    let mut buf = vec![0u8; size as usize];
    let ret = GetAdaptersAddresses(
        family,
        flags,
        None,
        Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
        &mut size,
    );
    if ret != ERROR_SUCCESS.0 {
        return out;
    }

    let mut adapter = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        let a = &*adapter;
        let mut unicast = a.FirstUnicastAddress;
        while !unicast.is_null() {
            let u = &*unicast;
            if u.Address.iSockaddrLength >= size_of::<SOCKADDR_IN>() as i32 {
                let sin = &*(u.Address.lpSockaddr as *const SOCKADDR_IN);
                if sin.sin_family == AF_INET {
                    let b = sin.sin_addr.S_un.S_un_b;
                    out.push((
                        Ipv4Addr::new(b.s_b1, b.s_b2, b.s_b3, b.s_b4),
                        u.OnLinkPrefixLength,
                    ));
                }
            }
            unicast = u.Next;
        }
        adapter = a.Next;
    }
    out
}

/// True if `ip` answered an HTTP GET for `/index.ps3` on port 80.
fn probe_ps3(ip: IpAddr) -> bool {
    let addr = SocketAddr::new(ip, 80);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(700)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let request = format!("GET /index.ps3 HTTP/1.0\r\nHost: {ip}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();
    let mut buf = [0u8; 1024];
    let Ok(bytes) = stream.read(&mut buf) else {
        return false;
    };
    if bytes == 0 {
        return false;
    }
    let head = String::from_utf8_lossy(&buf[..bytes]);
    head.lines().next().map_or(false, |l| l.contains(" 200 "))
}

/// Starts a background scan that probes every candidate host and fills in
/// `shared.found` (and finally `shared.done`).
fn start_scan(mut candidates: Vec<IpAddr>, shared: Arc<Mutex<ScanShared>>) {
    {
        let mut lock = shared.lock().unwrap();
        lock.total = candidates.len();
    }
    candidates.retain(|ip| ip.is_ipv4());
    candidates.sort_by_key(|ip| match ip {
        IpAddr::V4(v4) => u32::from_be_bytes(v4.octets()),
        IpAddr::V6(_) => 0,
    });
    candidates.dedup();

    std::thread::spawn(move || {
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(4, 24);
        let count = candidates.len();

        std::thread::scope(|scope| {
            for wid in 0..workers {
                let candidates = &candidates;
                let shared = &shared;
                scope.spawn(move || {
                    let mut i = wid;
                    while i < count {
                        if shared.lock().unwrap().cancelled {
                            break;
                        }
                        if probe_ps3(candidates[i]) {
                            shared.lock().unwrap().found.push(candidates[i].to_string());
                        }
                        i += workers;
                    }
                });
            }
        });

        let mut lock = shared.lock().unwrap();
        lock.done = true;
    });
}

/// Shows the "Searching for PS3's..." dialog and returns the selected IP, if any.
pub fn run_scan(_owner: Option<HWND>) -> Option<String> {
    unsafe {
        register_class();
        let dlg = ScanDialog {
            hwnd: HWND::default(),
            h_list: HWND::default(),
            h_status: HWND::default(),
            shared: Arc::new(Mutex::new(ScanShared::new())),
            closed: AtomicBool::new(false),
            result: None,
            items: Vec::new(),
            dots: 0,
            finished: false,
        };
        let ptr = Box::into_raw(Box::new(dlg));

        let hwnd = create_scan_window(ptr);
        if hwnd.0.is_null() {
            drop(Box::from_raw(ptr));
            return None;
        }

        let dlg = &mut *ptr;
        ui::run_modal(hwnd, &dlg.closed);
        let result = dlg.result.take();
        drop(Box::from_raw(ptr));
        result
    }
}

unsafe fn create_scan_window(ptr: *mut ScanDialog) -> HWND {
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let ex_style = WS_EX_DLGMODALFRAME;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 320,
        bottom: 244,
        ..Default::default()
    };
    let _ = AdjustWindowRectEx(&mut rect, style, false, ex_style);
    let hwnd = CreateWindowExW(
        ex_style,
w!("CellPresenceScan"),
        w!("CellPresence - Scan"),
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
            lpfnWndProc: Some(scan_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: ui::instance_handle(),
            hIcon: ui::default_icon(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH(((COLOR_BTNFACE.0 + 1) as isize) as *mut core::ffi::c_void),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: w!("CellPresenceScan"),
        };
        let _ = RegisterClassW(&wc);
    }
}

unsafe extern "system" fn scan_proc(
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
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ScanDialog;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let this = &mut *ptr;
    match msg {
        WM_CREATE => {
            this.on_create(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            this.on_timer();
            LRESULT(0)
        }
        WM_COMMAND => this.on_command(wparam),
        WM_CTLCOLORSTATIC => LRESULT(ui::btnface_brush().0 as isize),
        WM_CTLCOLORLISTBOX => LRESULT(ui::btnface_brush().0 as isize),
        WM_CLOSE => {
            this.cancel_and_close();
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl ScanDialog {
    unsafe fn on_create(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        let font = ui::gui_font();
        let inst = ui::instance_handle();

        self.h_status = create_child(
            hwnd,
            inst,
            w!("STATIC"),
            "Searching for PS3's...",
            16,
            14,
            288,
            20,
            IDC_STATUS,
            false,
        );
        let _ = SetWindowTextW(self.h_status, &HSTRING::from("Searching for PS3's..."));

        let list_style = WS_BORDER.0 | WS_VSCROLL.0 | (LBS_NOTIFY as u32) | (LBS_NOINTEGRALHEIGHT as u32);
        self.h_list = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("LISTBOX"),
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | list_style),
            16,
            40,
            288,
            156,
            hwnd,
            HMENU(IDC_LIST as isize as *mut core::ffi::c_void),
            inst,
            None,
        )
        .unwrap_or_default();

        let _ = create_button(hwnd, inst, "Use Selected", 150, 210, 88, 24, IDC_USE, false, &font);
        let _ = create_button(
            hwnd,
            inst,
            "Cancel",
            242,
            210,
            66,
            24,
            IDC_CANCEL_BTN,
            false,
            &font,
        );

        for control in [self.h_list, self.h_status] {
            let _ = SendMessageW(control, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }

        start_scan(subnet_hosts(), self.shared.clone());
        let _ = SetTimer(hwnd, TIMER_SCAN, 150, None);
    }

    fn on_timer(&mut self) {
        let (found, done, cancelled) = {
            let s = self.shared.lock().unwrap();
            (s.found.clone(), s.done, s.cancelled)
        };
        if found.len() != self.items.len() {
            self.items = found;
            unsafe { self.rebuild_list() };
        }
        if self.finished {
            return;
        }
        if done {
            self.finished = true;
            unsafe {
                let _ = KillTimer(self.hwnd, TIMER_SCAN);
                let msg = if cancelled {
                    "Scan stopped."
                } else if self.items.is_empty() {
                    "Scan complete. No PS3s found."
                } else {
                    "Scan complete."
                };
                let _ = SetWindowTextW(self.h_status, &HSTRING::from(msg));
            }
        } else {
            self.dots = (self.dots + 1) % 4;
            let msg = format!("Searching for PS3's{}", ".".repeat(self.dots as usize));
            unsafe {
                let _ = SetWindowTextW(self.h_status, &HSTRING::from(msg));
            }
        }
    }

    unsafe fn rebuild_list(&mut self) {
        let _ = SendMessageW(self.h_list, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
        for ip in &self.items {
            let h = HSTRING::from(ip);
            let _ = SendMessageW(self.h_list, LB_ADDSTRING, WPARAM(0), LPARAM(h.as_ptr() as isize));
        }
    }

    fn on_command(&mut self, wparam: WPARAM) -> LRESULT {
        match (wparam.0 & 0xffff) as usize {
            IDC_USE => self.use_selected(),
            IDC_CANCEL_BTN | ui::IDCANCEL => self.cancel_and_close(),
            _ => {}
        }
        LRESULT(0)
    }

    fn use_selected(&mut self) {
        let sel = unsafe { SendMessageW(self.h_list, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
        if sel >= 0 && (sel as usize) < self.items.len() {
            self.result = Some(self.items[sel as usize].clone());
        }
        self.close_dialog();
    }

    fn cancel_and_close(&mut self) {
        self.shared.lock().unwrap().cancelled = true;
        self.close_dialog();
    }

    fn close_dialog(&mut self) {
        self.finished = true;
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = KillTimer(self.hwnd, TIMER_SCAN);
                let _ = DestroyWindow(self.hwnd);
            }
        }
        self.closed.store(true, Ordering::SeqCst);
    }
}

unsafe fn create_child(
    hwnd: HWND,
    inst: HINSTANCE,
    class: PCWSTR,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: usize,
    tabstop: bool,
) -> HWND {
    let mut style = WS_CHILD.0 | WS_VISIBLE.0;
    if tabstop {
        style |= WS_TABSTOP.0;
    }
    CreateWindowExW(
        WS_EX_TRANSPARENT,
        class,
        &HSTRING::from(text),
        WINDOW_STYLE(style),
        x,
        y,
        w,
        h,
        hwnd,
        HMENU(id as isize as *mut core::ffi::c_void),
        inst,
        None,
    )
    .unwrap_or_default()
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
    let style = WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | btn_style;
    let hwnd_btn = CreateWindowExW(
        WS_EX_TRANSPARENT,
        w!("BUTTON"),
        &HSTRING::from(text),
        WINDOW_STYLE(style),
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
    let _ = SendMessageW(hwnd_btn, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
    hwnd_btn
}
