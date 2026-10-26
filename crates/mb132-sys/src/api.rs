//! Runtime (lazy) binding of the Miniblink export table.
//!
//! Miniblink ships as a single DLL (`mb132_x64.dll`) that exports a flat C API.
//! Instead of linking against an import library we resolve every symbol with
//! [`libloading`], exactly like the reference `mb.h` does with `LoadLibraryW` +
//! `GetProcAddress`.

use std::ffi::c_void;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::*;

/// Failure while loading the Miniblink shared library.
#[derive(Debug)]
pub struct Error {
    /// Path that failed to load.
    pub path: PathBuf,
    /// Underlying loader error (also covers missing symbols).
    pub source: libloading::Error,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to load miniblink from {}: {}", self.path.display(), self.source)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

macro_rules! api_table {
    ($( $name:ident : $ty:ty , $doc:literal ; )*) => {
        /// Resolved Miniblink export table.
        #[allow(non_snake_case)]
        pub struct Api {
            $( #[doc = $doc] pub $name: $ty, )*
        }

        impl Api {
            unsafe fn resolve(lib: &libloading::Library) -> Result<Self, libloading::Error> {
                Ok(Self {
                    $( $name: {
                        let symbol = unsafe { lib.get::<$ty>(concat!(stringify!($name), "\0").as_bytes()) }?;
                        *symbol
                    }, )*
                })
            }
        }
    };
}

api_table! {
    mbInit: unsafe extern "system" fn(*const mbSettings), "Initialize the engine";
    mbUninit: unsafe extern "system" fn(), "Shut the engine down";
    mbEnableHighDPISupport: unsafe extern "system" fn(), "Enable per-monitor DPI awareness";
    mbQueryState: unsafe extern "system" fn(mbWebView, *const i8) -> i32, "Query internal state";

    mbCreateString: unsafe extern "system" fn(*const i8, usize) -> mbStringPtr, "Create a non-owning mbString";
    mbCreateStringWithCopy: unsafe extern "system" fn(*const i8, usize) -> mbStringPtr, "Create a copying mbString";
    mbDeleteString: unsafe extern "system" fn(mbStringPtr), "Release a mbString";
    mbGetString: unsafe extern "system" fn(mbStringPtr) -> *const i8, "Borrow a mbString buffer";
    mbGetStringLen: unsafe extern "system" fn(mbStringPtr) -> usize, "Length of a mbString";

    mbCreateWebView: unsafe extern "system" fn() -> mbWebView, "Create an offscreen webview";
    mbDestroyWebView: unsafe extern "system" fn(mbWebView), "Destroy a webview";
    mbSetHandle: unsafe extern "system" fn(mbWebView, HWND), "Bind the webview to a host window";
    mbSetHandleOffset: unsafe extern "system" fn(mbWebView, i32, i32), "Offset of the webview inside the host window";
    mbSetAutoDrawToHwnd: unsafe extern "system" fn(mbWebView, BOOL), "Blit painted frames directly to the host window";
    mbGetHostHWND: unsafe extern "system" fn(mbWebView) -> HWND, "Host window of the webview";
    mbCreateWebWindow: unsafe extern "system" fn(i32) -> mbWebView, "Create a webview with its own native window";
    mbGetPlatformWindowHandle: unsafe extern "system" fn(mbWebView) -> *mut c_void, "Platform window handle";
    mbGetLockedViewDC: unsafe extern "system" fn(mbWebView) -> HDC, "Lock the engine memory DC";
    mbUnlockViewDC: unsafe extern "system" fn(mbWebView), "Unlock the engine memory DC";
    mbResize: unsafe extern "system" fn(mbWebView, i32, i32), "Resize the webview";
    mbMoveWindow: unsafe extern "system" fn(mbWebView, i32, i32, i32, i32), "Move/resize the webview";
    mbShowWindow: unsafe extern "system" fn(mbWebView, i32), "Show or hide the webview";
    mbSetFocus: unsafe extern "system" fn(mbWebView), "Give keyboard focus to the webview";
    mbKillFocus: unsafe extern "system" fn(mbWebView), "Remove keyboard focus from the webview";
    mbWake: unsafe extern "system" fn(mbWebView), "Wake the blink thread";

    mbLoadURL: unsafe extern "system" fn(mbWebView, *const i8), "Navigate to an URL";
    mbLoadHtmlWithBaseUrl: unsafe extern "system" fn(mbWebView, *const i8, *const i8), "Load html from memory";
    mbReload: unsafe extern "system" fn(mbWebView), "Reload the current page";
    mbStopLoading: unsafe extern "system" fn(mbWebView), "Stop loading";
    mbGoBack: unsafe extern "system" fn(mbWebView), "Navigate back";
    mbGoForward: unsafe extern "system" fn(mbWebView), "Navigate forward";
    mbCanGoBackOrForward: unsafe extern "system" fn(mbWebView, BOOL) -> BOOL, "Can go back / forward";
    mbGetNavigateIndex: unsafe extern "system" fn(mbWebView) -> i32, "Current navigation index";
    mbGoToIndex: unsafe extern "system" fn(mbWebView, i32), "Go to a navigation index";

    mbGetTitle: unsafe extern "system" fn(mbWebView) -> *const i8, "Current document title";
    mbSetWindowTitle: unsafe extern "system" fn(mbWebView, *const i8), "Set the window title";
    mbGetUrl: unsafe extern "system" fn(mbWebView) -> *const i8, "Current url";
    mbSetUserAgent: unsafe extern "system" fn(mbWebView, *const i8), "Override the user agent";
    mbSetZoomFactor: unsafe extern "system" fn(mbWebView, f32), "Set the zoom factor";
    mbGetZoomFactor: unsafe extern "system" fn(mbWebView) -> f32, "Get the zoom factor";
    mbSetTransparent: unsafe extern "system" fn(mbWebView, BOOL), "Enable transparent background";
    mbSetViewSettings: unsafe extern "system" fn(mbWebView, *const mbViewSettings), "Per view settings";
    mbSetContextMenuEnabled: unsafe extern "system" fn(mbWebView, BOOL), "Enable the context menu";
    mbSetNavigationToNewWindowEnable: unsafe extern "system" fn(mbWebView, BOOL), "Open target=_blank in a new window";
    mbSetCspCheckEnable: unsafe extern "system" fn(mbWebView, BOOL), "Enable CSP checks";
    mbSetDebugConfig: unsafe extern "system" fn(mbWebView, *const i8, *const i8), "Set a debug flag";
    mbSetDeviceParameter: unsafe extern "system" fn(mbWebView, *const i8, *const i8, i32, f32), "Set a device parameter";
    mbSetLanguage: unsafe extern "system" fn(mbWebView, *const i8), "Set the accept-language value";
    mbSetMouseEnabled: unsafe extern "system" fn(mbWebView, BOOL), "Enable mouse input";
    mbSetTouchEnabled: unsafe extern "system" fn(mbWebView, BOOL), "Enable touch input";
    mbSetDragDropEnable: unsafe extern "system" fn(mbWebView, BOOL), "Enable drag and drop";
    mbSetDragEnable: unsafe extern "system" fn(mbWebView, BOOL), "Enable file drag into the page";
    mbSetEditable: unsafe extern "system" fn(mbWebView, bool), "Toggle editing mode";
    mbGetSize: unsafe extern "system" fn(mbWebView, *mut mbRect), "Current webview rectangle";
    mbGetCursorInfoType: unsafe extern "system" fn(mbWebView) -> i32, "Current cursor type";

    mbSetCookieEnabled: unsafe extern "system" fn(mbWebView, BOOL), "Enable cookies";
    mbSetCookie: unsafe extern "system" fn(mbWebView, *const i8, *const i8), "Set a cookie";
    mbGetCookie: unsafe extern "system" fn(mbWebView, MbGetCookieCallback, *mut c_void), "Read cookies";
    mbClearCookie: unsafe extern "system" fn(mbWebView), "Drop all cookies";
    mbPerformCookieCommand: unsafe extern "system" fn(mbWebView, i32), "Run a cookie command";
    mbSetDiskCacheEnabled: unsafe extern "system" fn(mbWebView, BOOL), "Enable the disk cache";
    mbSetDiskCachePath: unsafe extern "system" fn(mbWebView, *const u16), "Disk cache directory";
    mbSetLocalStorageFullPath: unsafe extern "system" fn(mbWebView, *const u16), "Local storage file";
    mbSetCookieJarFullPath: unsafe extern "system" fn(mbWebView, *const u16), "Cookie jar file";

    mbRunJs: unsafe extern "system" fn(mbWebView, mbWebFrameHandle, *const i8, BOOL, MbRunJsCallback, *mut c_void, *mut c_void), "Run javascript asynchronously";
    mbRunJsSync: unsafe extern "system" fn(mbWebView, mbWebFrameHandle, *const i8, BOOL) -> mbJsValue, "Run javascript synchronously";
    mbJsToString: unsafe extern "system" fn(mbJsExecState, mbJsValue) -> *const i8, "Convert a js value to a string";
    mbJsToDouble: unsafe extern "system" fn(mbJsExecState, mbJsValue) -> f64, "Convert a js value to a number";
    mbJsToBoolean: unsafe extern "system" fn(mbJsExecState, mbJsValue) -> BOOL, "Convert a js value to a boolean";
    mbGetJsValueType: unsafe extern "system" fn(mbJsExecState, mbJsValue) -> i32, "Type of a js value";
    mbWebFrameGetMainFrame: unsafe extern "system" fn(mbWebView) -> mbWebFrameHandle, "Main frame handle";
    mbOnJsQuery: unsafe extern "system" fn(mbWebView, MbJsQueryCallback, *mut c_void), "Bind the js query callback";
    mbResponseQuery: unsafe extern "system" fn(mbWebView, i64, i32, *const i8), "Reply to a js query";

    mbOnPaintUpdated: unsafe extern "system" fn(mbWebView, MbPaintUpdatedCallback, *mut c_void), "Paint callback (HDC based)";
    mbOnTitleChanged: unsafe extern "system" fn(mbWebView, MbTitleChangedCallback, *mut c_void), "Title changed callback";
    mbOnURLChanged: unsafe extern "system" fn(mbWebView, MbUrlChangedCallback, *mut c_void), "Url changed callback";
    mbOnDocumentReady: unsafe extern "system" fn(mbWebView, MbDocumentReadyCallback, *mut c_void), "Document ready callback";
    mbOnLoadingFinish: unsafe extern "system" fn(mbWebView, MbLoadingFinishCallback, *mut c_void), "Loading finished callback";
    mbOnConsole: unsafe extern "system" fn(mbWebView, MbConsoleCallback, *mut c_void), "Console message callback";
    mbOnNavigation: unsafe extern "system" fn(mbWebView, MbNavigationCallback, *mut c_void), "Navigation callback";
    mbOnLoadUrlBegin: unsafe extern "system" fn(mbWebView, MbLoadUrlBeginCallback, *mut c_void), "Network request hook";
    mbOnLoadUrlEnd: unsafe extern "system" fn(mbWebView, MbLoadUrlEndCallback, *mut c_void), "Network response hook";
    mbOnDidCreateScriptContext: unsafe extern "system" fn(mbWebView, MbDidCreateScriptContextCallback, *mut c_void), "Script context created";
    mbOnClose: unsafe extern "system" fn(mbWebView, MbCloseCallback, *mut c_void) -> BOOL, "Close requested";
    mbOnDestroy: unsafe extern "system" fn(mbWebView, MbDestroyCallback, *mut c_void) -> BOOL, "Webview destroyed";

    mbFireWindowsMessage: unsafe extern "system" fn(mbWebView, HWND, UINT, WPARAM, LPARAM, *mut LRESULT) -> BOOL, "Forward a native window message";
    mbFireMouseEvent: unsafe extern "system" fn(mbWebView, UINT, i32, i32, UINT) -> BOOL, "Send a mouse event";
    mbFireMouseWheelEvent: unsafe extern "system" fn(mbWebView, i32, i32, i32, UINT) -> BOOL, "Send a wheel event";
    mbFireKeyDownEvent: unsafe extern "system" fn(mbWebView, UINT, UINT, BOOL) -> BOOL, "Send a key down event";
    mbFireKeyUpEvent: unsafe extern "system" fn(mbWebView, UINT, UINT, BOOL) -> BOOL, "Send a key up event";
    mbFireKeyPressEvent: unsafe extern "system" fn(mbWebView, UINT, UINT, BOOL) -> BOOL, "Send a key press event";

    mbNetSetData: unsafe extern "system" fn(mbNetJob, *mut c_void, i32), "Replace the response body";
    mbNetSetMIMEType: unsafe extern "system" fn(mbNetJob, *const i8), "Set the response mime type";
    mbNetSetHTTPHeaderFieldUtf8: unsafe extern "system" fn(mbNetJob, *const i8, *const i8, BOOL), "Set a response header";
    mbNetHoldJobToAsynCommit: unsafe extern "system" fn(mbNetJob), "Hold the request until the response is ready";
    mbNetContinueJob: unsafe extern "system" fn(mbNetJob), "Resume a held request";
    mbNetCancelRequest: unsafe extern "system" fn(mbNetJob), "Cancel a request";
    mbNetChangeRequestUrl: unsafe extern "system" fn(mbNetJob, *const i8), "Rewrite the request url";
    mbNetGetPostBody: unsafe extern "system" fn(mbNetJob) -> *mut mbPostBodyElements, "Read the POST body";
    mbNetGetRawHttpHeadInBlinkThread: unsafe extern "system" fn(mbNetJob) -> *const mbSlist, "Read the request headers (blink thread only)";
    mbNetGetRequestMethod: unsafe extern "system" fn(mbNetJob) -> i32, "Request method";
    mbNetFreePostBodyElements: unsafe extern "system" fn(*mut mbPostBodyElements), "Release a POST body";

    mbCallBlinkThreadAsync: unsafe extern "system" fn(MbThreadCallback, *mut c_void, *mut c_void), "Run on the blink thread (async)";
    mbCallBlinkThreadSync: unsafe extern "system" fn(MbThreadCallback, *mut c_void, *mut c_void), "Run on the blink thread (sync)";
    mbPostToUiThread: unsafe extern "system" fn(Option<unsafe extern "system" fn(mbWebView, *mut c_void)>, *mut c_void), "Run on the ui thread";

    mbUtilPrint: unsafe extern "system" fn(mbWebView, mbWebFrameHandle, *const mbPrintSettings) -> BOOL, "Print the page";
    mbUtilIsRegistered: unsafe extern "system" fn(*const u16) -> BOOL, "Check the licence";
}

static API: OnceLock<Api> = OnceLock::new();
static LIBRARY: OnceLock<&'static libloading::Library> = OnceLock::new();

/// Loads the Miniblink shared library and resolves every symbol.
///
/// Subsequent calls ignore `dll_path` and return the already loaded table.
pub fn initialize(dll_path: &Path) -> Result<&'static Api, Error> {
    if let Some(api) = API.get() {
        return Ok(api);
    }

    // SAFETY: loading a shared library is inherently unsafe, the caller is
    // responsible for the dll being a genuine Miniblink build.
    let lib = unsafe { libloading::Library::new(dll_path) }.map_err(|source| Error {
        path: dll_path.to_path_buf(),
        source,
    })?;

    // Keep the library loaded for the whole process lifetime: function pointers
    // stored in `Api` borrow from it.
    let lib: &'static libloading::Library = LIBRARY.get_or_init(|| Box::leak(Box::new(lib)));

    // SAFETY: the symbol signatures mirror `mb.h`.
    let api = unsafe { Api::resolve(lib) }.map_err(|source| Error {
        path: dll_path.to_path_buf(),
        source,
    })?;

    let _ = API.set(api);
    Ok(API.get().expect("api was just initialized"))
}

/// Returns the export table if [`initialize`] has been called.
pub fn api() -> Option<&'static Api> {
    API.get()
}

/// Whether [`initialize`] has completed.
pub fn is_initialized() -> bool {
    API.get().is_some()
}

/// Returns the export table, panicking when the engine has not been initialized.
#[track_caller]
pub fn expect_api() -> &'static Api {
    API.get().expect("mb132 is not initialized; call mb132::init() first")
}
