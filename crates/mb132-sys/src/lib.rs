//! Raw FFI declarations for the Miniblink (mb132) browser engine.
//!
//! The bindings are hand written against `mb.h` shipped with the SDK and are
//! loaded at runtime with [`api::initialize`], so no import library is needed.
//!
//! Only the subset of the C API that is required to host a webview inside a
//! Tauri runtime is declared here; adding a new function is a matter of adding
//! one line to the [`api_table!`] invocation in [`api`].

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

#[cfg(not(all(windows, target_pointer_width = "64")))]
compile_error!(
    "mb132-sys only supports 64-bit Windows: these bindings mirror the x64 ABI of `mb132_x64.dll` \
     (pointer sized handles, `GetWindowLongPtrW`, ...)"
);

pub mod api;

pub use api::{api, is_initialized, Api, Error as ApiError};

use std::ffi::c_void;

/// `BOOL`
pub type BOOL = i32;
/// `HWND` (windows)
pub type HWND = isize;
/// `HDC` (windows)
pub type HDC = isize;
/// `WPARAM`
pub type WPARAM = usize;
/// `LPARAM`
pub type LPARAM = isize;
/// `LRESULT`
pub type LRESULT = isize;
/// `UINT`
pub type UINT = u32;
/// `DWORD`
pub type DWORD = u32;

/// `mbWebView` — an opaque webview handle (pointer sized on 32 bit, `__int64` on 64 bit).
pub type mbWebView = i64;
/// `mbJsExecState`
pub type mbJsExecState = *mut c_void;
/// `mbJsValue`
pub type mbJsValue = i64;
/// `mbWebFrameHandle`
pub type mbWebFrameHandle = *mut c_void;
/// `mbNetJob`
pub type mbNetJob = *mut c_void;
/// `mbStringPtr`
pub type mbStringPtr = *mut c_void;

/// `kMbVersion`
pub const KM_B_VERSION: isize = 20210809;
/// `kMbMaxVersion`
pub const KM_B_MAX_VERSION: isize = 20600319;

/// `NULL_WEBVIEW`
pub const NULL_WEBVIEW: mbWebView = 0;

// ---------------------------------------------------------------------------
// structures
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct mbRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct mbPoint {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct mbSize {
    pub w: i32,
    pub h: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct mbProxy {
    pub proxy_type: i32,
    pub hostname: [i8; 100],
    pub port: u16,
    pub username: [i8; 50],
    pub password: [i8; 50],
}

impl Default for mbProxy {
    fn default() -> Self {
        // SAFETY: plain-old-data, all zeroes is a valid value.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct mbSettings {
    pub proxy: mbProxy,
    pub mask: u32,
    pub blink_thread_init_callback: Option<unsafe extern "system" fn(*mut c_void)>,
    pub blink_thread_init_callback_param: *mut c_void,
    pub version: isize,
    pub main_dll_path: *const u16,
    pub main_dll_handle: isize,
    pub config: *const i8,
}

impl Default for mbSettings {
    fn default() -> Self {
        // SAFETY: all fields are plain-old-data, zero is a valid value.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct mbViewSettings {
    pub size: i32,
    pub bg_color: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct mbMemBuf {
    pub unuse: i32,
    pub data: *mut c_void,
    pub length: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct mbPrintSettings {
    pub struct_size: i32,
    pub dpi: i32,
    pub width: i32,
    pub height: i32,
    pub margin_top: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    pub margin_right: i32,
    pub is_print_page_head_and_footer: BOOL,
    pub is_print_backgroud: BOOL,
    pub is_landscape: BOOL,
    pub is_print_to_multi_page: BOOL,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct mbPostBodyElement {
    pub size: i32,
    pub element_type: i32,
    pub data: *mut mbMemBuf,
    pub file_path: mbStringPtr,
    pub file_start: i64,
    pub file_length: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct mbPostBodyElements {
    pub size: i32,
    pub element: *mut *mut mbPostBodyElement,
    pub element_size: usize,
    pub is_dirty: bool,
}

// ---------------------------------------------------------------------------
// enums / constants
// ---------------------------------------------------------------------------

pub const MB_SETTING_PROXY: u32 = 1;
pub const MB_ENABLE_NODEJS: u32 = 1 << 3;

pub const MB_LBUTTON: u32 = 0x01;
pub const MB_RBUTTON: u32 = 0x02;
pub const MB_SHIFT: u32 = 0x04;
pub const MB_CONTROL: u32 = 0x08;
pub const MB_MBUTTON: u32 = 0x10;

pub const MB_MSG_MOUSEMOVE: u32 = 0x0200;
pub const MB_MSG_LBUTTONDOWN: u32 = 0x0201;
pub const MB_MSG_LBUTTONUP: u32 = 0x0202;
pub const MB_MSG_LBUTTONDBLCLK: u32 = 0x0203;
pub const MB_MSG_RBUTTONDOWN: u32 = 0x0204;
pub const MB_MSG_RBUTTONUP: u32 = 0x0205;
pub const MB_MSG_RBUTTONDBLCLK: u32 = 0x0206;
pub const MB_MSG_MBUTTONDOWN: u32 = 0x0207;
pub const MB_MSG_MBUTTONUP: u32 = 0x0208;
pub const MB_MSG_MBUTTONDBLCLK: u32 = 0x0209;
pub const MB_MSG_MOUSEWHEEL: u32 = 0x020A;

/// `mbJsType`
pub const K_MB_JS_TYPE_NUMBER: i32 = 0;
pub const K_MB_JS_TYPE_STRING: i32 = 1;
pub const K_MB_JS_TYPE_BOOL: i32 = 2;
pub const K_MB_JS_TYPE_UNDEFINED: i32 = 5;
pub const K_MB_JS_TYPE_V8_VALUE: i32 = 8;

/// `mbLoadingResult`
pub const MB_LOADING_SUCCEEDED: i32 = 0;
pub const MB_LOADING_FAILED: i32 = 1;
pub const MB_LOADING_CANCELED: i32 = 2;

/// `mbConsoleLevel`
pub const MB_LEVEL_DEBUG: i32 = 4;
pub const MB_LEVEL_LOG: i32 = 1;
pub const MB_LEVEL_INFO: i32 = 5;
pub const MB_LEVEL_WARNING: i32 = 2;
pub const MB_LEVEL_ERROR: i32 = 3;

/// `mbNavigationType`
pub const MB_NAVIGATION_TYPE_LINKCLICK: i32 = 0;
pub const MB_NAVIGATION_TYPE_FORMSUBMITTE: i32 = 1;
pub const MB_NAVIGATION_TYPE_BACKFORWARD: i32 = 2;
pub const MB_NAVIGATION_TYPE_RELOAD: i32 = 3;
pub const MB_NAVIGATION_TYPE_FORMRESUBMITT: i32 = 4;
pub const MB_NAVIGATION_TYPE_OTHER: i32 = 5;

/// `mbCursorInfoType`
pub const K_MB_CURSOR_INFO_POINTER: i32 = 0;
pub const K_MB_CURSOR_INFO_CROSS: i32 = 1;
pub const K_MB_CURSOR_INFO_HAND: i32 = 2;
pub const K_MB_CURSOR_INFO_IBEAM: i32 = 3;
pub const K_MB_CURSOR_INFO_WAIT: i32 = 4;
pub const K_MB_CURSOR_INFO_HELP: i32 = 5;

/// `mbCookieCommand`
pub const MB_COOKIE_COMMAND_CLEAR_ALL_COOKIES: i32 = 0;
pub const MB_COOKIE_COMMAND_CLEAR_SESSION_COOKIES: i32 = 1;
pub const MB_COOKIE_COMMAND_FLUSH_COOKIES_TO_FILE: i32 = 2;
pub const MB_COOKIE_COMMAND_RELOAD_COOKIES_FROM_FILE: i32 = 3;

/// `mbWindowType`
pub const MB_WINDOW_TYPE_POPUP: i32 = 0;
pub const MB_WINDOW_TYPE_TRANSPARENT: i32 = 1;
pub const MB_WINDOW_TYPE_CONTROL: i32 = 2;

/// `mbAsynRequestState`
pub const K_MB_ASYN_REQUEST_STATE_OK: i32 = 0;

// ---------------------------------------------------------------------------
// callback types
// ---------------------------------------------------------------------------

pub type MbOnBlinkThreadInitCallback = Option<unsafe extern "system" fn(*mut c_void)>;
pub type MbRunJsCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, mbJsExecState, mbJsValue)>;
pub type MbJsQueryCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, mbJsExecState, i64, i32, *const i8)>;
pub type MbTitleChangedCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, *const i8)>;
pub type MbUrlChangedCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, *const i8, BOOL, BOOL)>;
pub type MbPaintUpdatedCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, HDC, i32, i32, i32, i32)>;
pub type MbDocumentReadyCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, mbWebFrameHandle)>;
pub type MbLoadingFinishCallback = Option<
    unsafe extern "system" fn(mbWebView, *mut c_void, mbWebFrameHandle, *const i8, i32, *const i8),
>;
pub type MbConsoleCallback = Option<
    unsafe extern "system" fn(mbWebView, *mut c_void, i32, *const i8, *const i8, u32, *const i8),
>;
pub type MbNavigationCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, i32, *const i8) -> BOOL>;
pub type MbLoadUrlBeginCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, *const i8, mbNetJob) -> BOOL>;
pub type MbLoadUrlEndCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, *const i8, mbNetJob, *mut c_void, i32)>;
pub type MbDidCreateScriptContextCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, mbWebFrameHandle, *mut c_void, i32, i32)>;
pub type MbCloseCallback = Option<unsafe extern "system" fn(mbWebView, *mut c_void, *mut c_void) -> BOOL>;
pub type MbDestroyCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, *mut c_void) -> BOOL>;
pub type MbGetCookieCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, i32, *const i8)>;
pub type MbCanGoBackForwardCallback =
    Option<unsafe extern "system" fn(mbWebView, *mut c_void, i32, BOOL)>;
pub type MbThreadCallback = Option<unsafe extern "system" fn(*mut c_void, *mut c_void)>;

/// Request method reported by the engine.
pub const MB_REQUEST_TYPE_INVALIDATION: i32 = 0;
/// `GET` request.
pub const MB_REQUEST_TYPE_GET: i32 = 1;
/// `POST` request.
pub const MB_REQUEST_TYPE_POST: i32 = 2;
/// `PUT` request.
pub const MB_REQUEST_TYPE_PUT: i32 = 3;

/// Singly linked list used by the engine for raw http headers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct mbSlist {
    /// Header line, e.g. `Content-Type: application/json`.
    pub data: *const i8,
    /// Next node.
    pub next: *const mbSlist,
}

/// File name of the engine binary.
pub const DLL_NAME: &str = "mb132_x64.dll";

/// Environment variable that names the directory holding [`DLL_NAME`].
///
/// The value is read at runtime, so the engine location can be configured per
/// machine or per deployment without rebuilding. Empty values are ignored.
pub const DLL_DIR_ENV: &str = "MB132_DLL_DIR";

/// Full path of the engine binary inside [`DLL_DIR_ENV`], if the variable is set.
pub fn dll_path_from_env() -> Option<std::path::PathBuf> {
    std::env::var_os(DLL_DIR_ENV)
        .filter(|dir| !dir.is_empty())
        .map(|dir| std::path::Path::new(&dir).join(DLL_NAME))
}
