//! Minimal Win32 bindings.
//!
//! The runtime talks to `user32`/`gdi32` directly instead of going through the
//! `windows` crate: the surface we need is tiny, and hand written declarations
//! keep the ABI explicit. `windows` is still used for the `HWND` newtype that
//! `tauri-runtime` exposes in the `WindowBuilder` trait.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

pub type BOOL = i32;
pub type HWND = isize;
pub type HINSTANCE = isize;
pub type HMONITOR = isize;
pub type HBRUSH = isize;
pub type HCURSOR = isize;
pub type HICON = isize;
pub type HMENU = isize;
pub type HDC = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type UINT = u32;
pub type DWORD = u32;
pub type ATOM = u16;

pub type WNDPROC =
    Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>;
pub type MONITORENUMPROC = Option<
    unsafe extern "system" fn(HMONITOR, HDC, *mut RECT, LPARAM) -> BOOL,
>;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RECT {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

#[repr(C)]
pub struct NCCALCSIZE_PARAMS {
    pub rgrc: [RECT; 3],
    pub lppos: *mut core::ffi::c_void,
}

#[repr(C)]
#[derive(Default)]
pub struct APPBARDATA {
    pub cbSize: DWORD,
    pub hWnd: HWND,
    pub uCallbackMessage: UINT,
    pub uEdge: UINT,
    pub rc: RECT,
    pub lParam: LPARAM,
}

#[repr(C)]
struct OsVersionInfoW {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    service_pack: [u16; 128],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: DWORD,
    pub pt: POINT,
    pub lPrivate: DWORD,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WNDCLASSW {
    pub style: UINT,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
}

impl Default for WNDCLASSW {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MONITORINFOEXW {
    pub cbSize: DWORD,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: DWORD,
    pub szDevice: [u16; 32],
}

impl Default for MONITORINFOEXW {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MINMAXINFO {
    pub ptReserved: POINT,
    pub ptMaxSize: POINT,
    pub ptMaxPosition: POINT,
    pub ptMinTrackSize: POINT,
    pub ptMaxTrackSize: POINT,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct CREATESTRUCTW {
    pub lpCreateParams: *mut core::ffi::c_void,
    pub hInstance: HINSTANCE,
    pub hMenu: HMENU,
    pub hwndParent: HWND,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i64,
    pub lpszName: *const u16,
    pub lpszClass: *const u16,
    pub dwExStyle: DWORD,
}

// ---------------- constants ----------------

pub const WS_OVERLAPPED: u32 = 0x0000_0000;
pub const WS_CAPTION: u32 = 0x00C0_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
pub const WS_OVERLAPPEDWINDOW: u32 =
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const WS_DISABLED: u32 = 0x0800_0000;

pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_WINDOWEDGE: u32 = 0x0000_0100;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;

pub const CS_HREDRAW: u32 = 0x0002;
pub const CS_VREDRAW: u32 = 0x0001;
pub const CS_DBLCLKS: u32 = 0x0008;

pub const CW_USEDEFAULT: i32 = -2147483648;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOWNORMAL: i32 = 1;
pub const SW_SHOW: i32 = 5;
pub const SW_MINIMIZE: i32 = 6;
pub const SW_SHOWMINIMIZED: i32 = 2;
pub const SW_SHOWMAXIMIZED: i32 = 3;
pub const SW_MAXIMIZE: i32 = 3;
pub const SW_RESTORE: i32 = 9;

pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOREDRAW: u32 = 0x0008;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_FRAMECHANGED: u32 = 0x0020;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const SWP_HIDEWINDOW: u32 = 0x0080;
pub const SWP_NOOWNERZORDER: u32 = 0x0200;

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_MOVE: u32 = 0x0003;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_ENABLE: u32 = 0x000A;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SHOWWINDOW: u32 = 0x0018;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_MOUSEACTIVATE: u32 = 0x0021;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_NCHITTEST: u32 = 0x0084;
pub const WM_NCCALCSIZE: u32 = 0x0083;
pub const WM_SYSCOMMAND: u32 = 0x0112;
pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;
pub const WM_SYSCHAR: u32 = 0x0106;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_RBUTTONDBLCLK: u32 = 0x0206;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MBUTTONUP: u32 = 0x0208;
pub const WM_MBUTTONDBLCLK: u32 = 0x0209;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_ENTERSIZEMOVE: u32 = 0x0231;
pub const WM_EXITSIZEMOVE: u32 = 0x0232;
pub const WM_APP: u32 = 0x8000;

pub const WM_RUNTIME_WAKE: u32 = WM_APP + 1;
pub const WM_RUNTIME_TASK: u32 = WM_APP + 2;

pub const PM_NOREMOVE: u32 = 0x0000;
pub const PM_REMOVE: u32 = 0x0001;

pub const GWLP_USERDATA: i32 = -21;
pub const GWL_STYLE: i32 = -16;
pub const GWL_EXSTYLE: i32 = -20;
pub const GWLP_WNDPROC: i32 = -4;

pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;
pub const SM_XVIRTUALSCREEN: i32 = 76;
pub const SM_YVIRTUALSCREEN: i32 = 77;
pub const SM_CXVIRTUALSCREEN: i32 = 78;
pub const SM_CYVIRTUALSCREEN: i32 = 79;

pub const MONITOR_DEFAULTTONEAREST: u32 = 0x0000_0002;
pub const MDT_EFFECTIVE_DPI: i32 = 0;

pub const SRCCOPY: u32 = 0x00CC_0020;
pub const IDC_ARROW: *const u16 = 32512usize as *const u16;
pub const COLOR_WINDOW: usize = 5;
pub const HTCLIENT: i32 = 1;
pub const HTLEFT: i32 = 10;
pub const HTRIGHT: i32 = 11;
pub const HTTOP: i32 = 12;
pub const HTTOPLEFT: i32 = 13;
pub const HTTOPRIGHT: i32 = 14;
pub const HTBOTTOM: i32 = 15;
pub const HTBOTTOMLEFT: i32 = 16;
pub const HTBOTTOMRIGHT: i32 = 17;
pub const SM_CXFRAME: i32 = 32;
pub const SM_CYFRAME: i32 = 33;
pub const SM_CXPADDEDBORDER: i32 = 92;
pub const ABM_GETAUTOHIDEBAR: u32 = 7;
pub const SC_MINIMIZE: usize = 0xF020;
pub const SC_MAXIMIZE: usize = 0xF030;
pub const SC_RESTORE: usize = 0xF120;
pub const SC_CLOSE: usize = 0xF060;

pub const MAPVK_VK_TO_CHAR: u32 = 2;

// --- IME (IMM32) -----------------------------------------------------------

/// Input method context handle.
pub type HIMC = isize;

pub const WM_IME_STARTCOMPOSITION: u32 = 0x010D;
pub const WM_IME_ENDCOMPOSITION: u32 = 0x010E;
pub const WM_IME_COMPOSITION: u32 = 0x010F;
pub const WM_IME_SETCONTEXT: u32 = 0x0281;
pub const WM_IME_NOTIFY: u32 = 0x0282;
pub const WM_IME_CHAR: u32 = 0x0286;

/// `ImmGetCompositionStringW`: the committed string.
pub const GCS_RESULTSTR: u32 = 0x0800;
/// `ImmGetCompositionStringW`: the string being composed (pinyin, ...).
pub const GCS_COMPSTR: u32 = 0x0008;
pub const GCS_CURSORPOS: u32 = 0x0080;

/// The host renders preedit text; the IME still owns its candidate UI.
pub const ISC_SHOWUICOMPOSITIONWINDOW: isize = 0x8000_0000;

/// Anchor the composition/candidate window at `current_pos`.
pub const CFS_POINT: u32 = 0x0002;
pub const CFS_EXCLUDE: u32 = 0x0080;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct COMPOSITIONFORM {
    pub style: DWORD,
    pub current_pos: POINT,
    pub area: RECT,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct CANDIDATEFORM {
    pub index: DWORD,
    pub style: DWORD,
    pub current_pos: POINT,
    pub area: RECT,
}

#[link(name = "user32")]
unsafe extern "system" {
    pub fn RegisterClassW(class: *const WNDCLASSW) -> ATOM;
    pub fn CreateWindowExW(
        ex_style: DWORD,
        class_name: *const u16,
        window_name: *const u16,
        style: DWORD,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: HWND,
        menu: HMENU,
        instance: HINSTANCE,
        param: *mut core::ffi::c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub fn ShowWindow(hwnd: HWND, command: i32) -> BOOL;
    pub fn IsWindowVisible(hwnd: HWND) -> BOOL;
    pub fn SetWindowTextW(hwnd: HWND, text: *const u16) -> BOOL;
    pub fn GetWindowTextW(hwnd: HWND, buffer: *mut u16, max: i32) -> i32;
    pub fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn SetWindowPos(
        hwnd: HWND,
        insert_after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: UINT,
    ) -> BOOL;
    pub fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: UINT, max: UINT) -> BOOL;
    pub fn PeekMessageW(msg: *mut MSG, hwnd: HWND, min: UINT, max: UINT, remove: UINT) -> BOOL;
    pub fn TranslateMessage(msg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostMessageW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> BOOL;
    pub fn SendMessageW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
    pub fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    pub fn LoadCursorW(instance: HINSTANCE, name: *const u16) -> HCURSOR;
    pub fn SetCursor(cursor: HCURSOR) -> HCURSOR;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn GetSystemMetricsForDpi(index: i32, dpi: u32) -> i32;
    pub fn IsZoomed(hwnd: HWND) -> BOOL;
    pub fn IsIconic(hwnd: HWND) -> BOOL;
    pub fn GetFocus() -> HWND;
    pub fn SetFocus(hwnd: HWND) -> HWND;
    pub fn GetForegroundWindow() -> HWND;
    pub fn GetActiveWindow() -> HWND;
    pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    pub fn SetActiveWindow(hwnd: HWND) -> HWND;
    pub fn EnableWindow(hwnd: HWND, enable: BOOL) -> BOOL;
    pub fn IsWindowEnabled(hwnd: HWND) -> BOOL;
    pub fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    pub fn UpdateWindow(hwnd: HWND) -> BOOL;
    pub fn BeginPaint(hwnd: HWND, paint: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hwnd: HWND, paint: *const PAINTSTRUCT) -> BOOL;
    pub fn EnumDisplayMonitors(
        hdc: HDC,
        clip: *const RECT,
        callback: MONITORENUMPROC,
        lparam: LPARAM,
    ) -> BOOL;
    pub fn GetMonitorInfoW(monitor: HMONITOR, info: *mut MONITORINFOEXW) -> BOOL;
    pub fn MonitorFromPoint(point: POINT, flags: DWORD) -> HMONITOR;
    pub fn MonitorFromWindow(hwnd: HWND, flags: DWORD) -> HMONITOR;
    pub fn MonitorFromRect(rect: *const RECT, flags: DWORD) -> HMONITOR;
    pub fn GetCursorPos(point: *mut POINT) -> BOOL;
    pub fn SetParent(hwnd: HWND, parent: HWND) -> HWND;
    pub fn GetParent(hwnd: HWND) -> HWND;
    pub fn EnumWindows(callback: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>, lparam: LPARAM) -> BOOL;
    pub fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
    pub fn GetClassNameW(hwnd: HWND, buffer: *mut u16, max: i32) -> i32;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
    pub fn SetCursorPos(x: i32, y: i32) -> BOOL;
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub fn GetDpiForSystem() -> u32;
    pub fn SetCapture(hwnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> BOOL;
    pub fn AdjustWindowRectEx(rect: *mut RECT, style: DWORD, menu: BOOL, ex_style: DWORD) -> BOOL;
    pub fn ClientToScreen(hwnd: HWND, point: *mut POINT) -> BOOL;
    pub fn ScreenToClient(hwnd: HWND, point: *mut POINT) -> BOOL;
    pub fn GetKeyState(key: i32) -> i16;
    pub fn TrackMouseEvent(event: *mut TRACKMOUSEEVENT) -> BOOL;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn SHAppBarMessage(message: DWORD, data: *mut APPBARDATA) -> usize;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
}

/// Query the real build, independent of the application's compatibility manifest.
pub fn windows_build_number() -> u32 {
    static BUILD: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *BUILD.get_or_init(|| {
        let mut info = OsVersionInfoW {
            size: std::mem::size_of::<OsVersionInfoW>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            service_pack: [0; 128],
        };
        if unsafe { RtlGetVersion(&mut info) } == 0 {
            info.build
        } else {
            0
        }
    })
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TRACKMOUSEEVENT {
    pub cbSize: DWORD,
    pub dwFlags: DWORD,
    pub hwndTrack: HWND,
    pub dwHoverTime: DWORD,
}

pub const TME_LEAVE: u32 = 0x0000_0002;

#[link(name = "imm32")]
unsafe extern "system" {
    pub fn ImmGetContext(hwnd: HWND) -> HIMC;
    pub fn ImmReleaseContext(hwnd: HWND, imc: HIMC) -> BOOL;
    pub fn ImmGetCompositionStringW(
        imc: HIMC,
        index: DWORD,
        buffer: *mut u16,
        length: DWORD,
    ) -> i32;
    pub fn ImmSetCompositionWindow(imc: HIMC, form: *const COMPOSITIONFORM) -> BOOL;
    pub fn ImmSetCandidateWindow(imc: HIMC, form: *const CANDIDATEFORM) -> BOOL;
    pub fn ImmGetOpenStatus(imc: HIMC) -> BOOL;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn BitBlt(
        dest: HDC,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        src: HDC,
        src_x: i32,
        src_y: i32,
        rop: u32,
    ) -> BOOL;
    pub fn CreateSolidBrush(color: u32) -> HBRUSH;
    pub fn DeleteObject(object: isize) -> BOOL;
    pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
    pub fn DeleteDC(hdc: HDC) -> BOOL;
    pub fn SelectObject(hdc: HDC, object: isize) -> isize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetCurrentThreadId() -> u32;
}

#[link(name = "shcore")]
unsafe extern "system" {
    pub fn GetDpiForMonitor(
        monitor: HMONITOR,
        dpi_type: i32,
        dpi_x: *mut u32,
        dpi_y: *mut u32,
    ) -> i32;
}

/// Nul terminated UTF-16 helper.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reinterprets a raw window handle as the `windows` crate newtype used by
/// `tauri-runtime`.
pub fn to_win32_hwnd(hwnd: isize) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void)
}

/// Extracts the raw handle out of the `windows` crate newtype.
pub fn from_win32_hwnd(hwnd: windows::Win32::Foundation::HWND) -> isize {
    hwnd.0 as isize
}

/// Current thread id.
pub fn current_thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

/// Scale factor of a window (`1.0` == 96 dpi).
pub fn window_scale_factor(hwnd: isize) -> f64 {
    if hwnd == 0 {
        return system_scale_factor();
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        system_scale_factor()
    } else {
        dpi as f64 / 96.0
    }
}

/// Scale factor of the primary display.
pub fn system_scale_factor() -> f64 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 }
}

/// Reads the title of a window.
pub fn window_title(hwnd: isize) -> String {
    let mut buffer = vec![0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..len as usize])
}

/// Client area of a window as `(x, y, width, height)`.
pub fn client_rect(hwnd: isize) -> (i32, i32, i32, i32) {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) };
    (0, 0, rect.width(), rect.height())
}

/// Screen rectangle of a window as `(x, y, width, height)`.
pub fn window_rect(hwnd: isize) -> (i32, i32, i32, i32) {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) };
    (rect.left, rect.top, rect.width(), rect.height())
}
