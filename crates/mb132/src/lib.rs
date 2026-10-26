//! Safe(r) Rust wrapper around the Miniblink (mb132) engine.
//!
//! The wrapper is deliberately small: it exposes just enough of `mb.h` to host
//! a webview inside a native window, intercept network requests (used by Tauri
//! to serve `tauri://localhost`) and push input events.
//!
//! ```no_run
//! let webview = mb132::WebViewBuilder::new(hwnd)
//!     .bounds(mb132::Rect::new(0, 0, 1024, 768))
//!     .url("http://tauri.localhost/")
//!     .build()
//!     .unwrap();
//! ```

#![allow(non_snake_case)]

pub mod net;
pub mod webview;

pub use mb132_sys as sys;
pub use net::{NetJob, Request, RequestAction, RequestHandler, Response};
pub use webview::{Handlers, Rect, WebView, WebViewBuilder};

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sys::{mbSettings, BOOL};

/// Errors returned by the wrapper.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The engine could not be loaded.
    #[error(transparent)]
    Load(#[from] sys::ApiError),
    /// The engine could not create a webview.
    #[error("miniblink failed to create a webview")]
    CreateWebView,
    /// The engine has not been initialized.
    #[error("miniblink is not initialized")]
    NotInitialized,
    /// A path could not be converted to a wide string.
    #[error("path contains a nul byte: {0}")]
    InvalidPath(PathBuf),
    /// A string contained an interior nul byte.
    #[error("string contains a nul byte")]
    NulByte,
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Options used to bring up the engine.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Full path of `mb132_x64.dll`.
    pub dll_path: PathBuf,
    /// Enable the Node.js integration.
    pub nodejs: bool,
    /// Extra engine configuration, passed as `mbSettings::config`.
    pub config: Option<String>,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            dll_path: default_dll_path(),
            nodejs: false,
            config: None,
        }
    }
}

/// Locates the engine library, in this order:
///
/// 1. the directory named by the `MB132_DLL_DIR` environment variable
///    (see [`mb132_sys::DLL_DIR_ENV`]),
/// 2. next to the current executable (how it is shipped),
/// 3. in the working directory,
/// 4. the bare file name, resolved by the system loader (for example `PATH`).
pub fn default_dll_path() -> PathBuf {
    if let Some(configured) = mb132_sys::dll_path_from_env() {
        return configured;
    }

    let name = mb132_sys::DLL_NAME;

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(name);
        if candidate.exists() {
            return candidate;
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(name);
        if candidate.exists() {
            return candidate;
        }
    }

    PathBuf::from(name)
}

static ENGINE: OnceLock<()> = OnceLock::new();

/// Initializes the engine. Calling it more than once is a no-op.
///
/// The engine requires a Win32 message loop on the calling thread; the Tauri
/// runtime pumps it.
pub fn init(options: InitOptions) -> Result<()> {
    if ENGINE.get().is_some() {
        return Ok(());
    }

    let api = sys::api::initialize(&options.dll_path)?;

    // SAFETY: must be called before `mbInit` on the UI thread.
    unsafe { (api.mbEnableHighDPISupport)() };

    let needs_settings = options.nodejs || options.config.is_some();
    if !needs_settings {
        // Matches the reference demo (`mbInit(nullptr)`).
        unsafe { (api.mbInit)(std::ptr::null()) };
    } else {
        let mut settings = mbSettings {
            version: sys::KM_B_VERSION,
            ..Default::default()
        };
        if options.nodejs {
            settings.mask |= sys::MB_ENABLE_NODEJS;
        }
        let config = options.config.as_deref().map(|c| keep_cstr(c)).transpose()?;
        settings.config = config.map(|c| c.as_ptr()).unwrap_or(std::ptr::null());
        unsafe { (api.mbInit)(&settings) };
    }

    let _ = ENGINE.set(());
    Ok(())
}

/// Whether [`init`] completed.
pub fn is_initialized() -> bool {
    ENGINE.get().is_some()
}

/// Engine version reported by Miniblink.
pub fn version() -> &'static str {
    "mb132"
}

/// Shuts the engine down. Usually called automatically by the runtime at exit.
pub fn shutdown() {
    if ENGINE.get().is_some()
        && let Some(api) = sys::api()
    {
        // SAFETY: engine was initialized.
        unsafe { (api.mbUninit)() };
    }
}

/// Leaks a nul terminated copy of `s` for the process lifetime, which is what
/// the C API expects for long lived configuration strings.
pub(crate) fn keep_cstr(s: &str) -> Result<&'static std::ffi::CStr> {
    let owned = std::ffi::CString::new(s).map_err(|_| Error::NulByte)?;
    Ok(Box::leak(owned.into_boxed_c_str()))
}

/// Converts a Rust string into a temporary nul terminated buffer.
pub(crate) fn cstr(s: &str) -> Result<std::ffi::CString> {
    std::ffi::CString::new(s).map_err(|_| Error::NulByte)
}

/// Converts a nul terminated C string into an owned `String`.
///
/// # Safety
/// `ptr` must be null or point to a valid nul terminated string.
pub(crate) unsafe fn from_cstr(ptr: *const i8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Renders a path as a nul terminated UTF-16 buffer.
pub(crate) fn wide(path: &Path) -> Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let mut buffer: Vec<u16> = path.as_os_str().encode_wide().collect();
    if buffer.contains(&0) {
        return Err(Error::InvalidPath(path.to_path_buf()));
    }
    buffer.push(0);
    Ok(buffer)
}

/// Truthy conversion for the C `BOOL` type.
pub(crate) fn bool_to_c(value: bool) -> BOOL {
    if value { 1 } else { 0 }
}
