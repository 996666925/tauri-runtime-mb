//! Offscreen webview hosted inside a native window.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

use miniblink_sys as sys;

use crate::net::{self, NetJob, RequestAction, RequestHandler};
use crate::{Error, Result, bool_to_c, cstr, from_cstr, wide};

/// Rectangle expressed in physical pixels, relative to the host window client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left offset.
    pub x: i32,
    /// Top offset.
    pub y: i32,
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

impl Rect {
    /// Creates a rectangle.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rectangle anchored at the origin.
    pub const fn size(width: i32, height: i32) -> Self {
        Self::new(0, 0, width, height)
    }

}

/// Callbacks invoked by the engine.
///
/// All closures run on the engine's blink thread, so they must be `Send` and
/// must not block.
#[derive(Default)]
pub struct Handlers {
    /// Document title changed.
    pub title_changed: Option<Box<dyn Fn(&str) + Send>>,
    /// Url changed; the booleans are `can_go_back` and `can_go_forward`.
    pub url_changed: Option<Box<dyn Fn(&str, bool, bool) + Send>>,
    /// The document finished parsing.
    pub document_ready: Option<Box<dyn Fn() + Send>>,
    /// A navigation finished; the boolean is `true` on success.
    pub loading_finished: Option<Box<dyn Fn(&str, bool) + Send>>,
    /// Console message: level, message, source.
    pub console: Option<Box<dyn Fn(i32, &str, &str) + Send>>,
    /// Navigation request. Returning `false` blocks the navigation.
    pub navigation: Option<Box<dyn Fn(&str) -> bool + Send>>,
    /// A frame was painted; arguments are `hdc, x, y, width, height`.
    pub paint: Option<Box<dyn Fn(isize, i32, i32, i32, i32) + Send>>,
    /// The engine asks to close the view. Returning `true` allows the close.
    pub close_requested: Option<Box<dyn Fn() -> bool + Send>>,
    /// A new javascript context was created (main frame and sub frames).
    pub script_context_created: Option<Box<dyn Fn(*mut c_void) + Send>>,
}

pub(crate) struct Inner {
    raw: sys::mbWebView,
    handlers: Mutex<Handlers>,
    request_handler: Mutex<Option<Arc<dyn RequestHandler>>>,
    scripts: Mutex<Vec<String>>,
    paint_hwnd: AtomicIsize,
    visible: AtomicBool,
    destroyed: AtomicBool,
}

impl Inner {
    fn handlers(&self) -> std::sync::MutexGuard<'_, Handlers> {
        self.handlers.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the engine view is still alive.
    ///
    /// Every call into the engine has to be guarded by this: the handle is
    /// dangling once [`WebView::destroy`] ran.
    fn alive(&self) -> bool {
        !self.destroyed.load(Ordering::SeqCst)
    }
}

/// Builds a [`WebView`].
pub struct WebViewBuilder {
    hwnd: isize,
    rect: Rect,
    url: Option<String>,
    html: Option<(String, Option<String>)>,
    transparent: bool,
    background_color: Option<u32>,
    context_menu: bool,
    user_agent: Option<String>,
    zoom: Option<f32>,
    language: Option<String>,
    cache_dir: Option<PathBuf>,
    local_storage: Option<PathBuf>,
    cookie_jar: Option<PathBuf>,
    init_scripts: Vec<String>,
    handlers: Handlers,
    request_handler: Option<Arc<dyn RequestHandler>>,
    auto_draw: bool,
    device_scale_factor: Option<f32>,
    windowed: bool,
}

impl WebViewBuilder {
    /// Creates a builder for a webview hosted by `hwnd`.
    pub fn new(hwnd: isize) -> Self {
        Self {
            hwnd,
            rect: Rect::default(),
            url: None,
            html: None,
            transparent: false,
            background_color: None,
            context_menu: true,
            user_agent: None,
            zoom: None,
            language: None,
            cache_dir: None,
            local_storage: None,
            cookie_jar: None,
            init_scripts: Vec::new(),
            handlers: Handlers::default(),
            request_handler: None,
            auto_draw: true,
            device_scale_factor: None,
            windowed: false,
        }
    }

    /// Hosts the view in a native child window created by the engine.
    ///
    /// This is the reliable mode: the engine owns the window, so it handles
    /// mouse/keyboard input, dpi scaling and painting itself (no coordinate
    /// translation on our side).
    pub fn windowed(mut self, windowed: bool) -> Self {
        self.windowed = windowed;
        self
    }

    /// Device scale factor used to map logical (css) pixels to the physical
    /// size of the view. Must match the scale factor of the host window.
    pub fn device_scale_factor(mut self, factor: f32) -> Self {
        self.device_scale_factor = Some(factor);
        self
    }

    /// Position and size inside the host window.
    pub fn bounds(mut self, rect: Rect) -> Self {
        self.rect = rect;
        self
    }

    /// Url loaded once the view is ready.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Html loaded once the view is ready.
    pub fn html(mut self, html: impl Into<String>, base_url: Option<String>) -> Self {
        self.html = Some((html.into(), base_url));
        self
    }

    /// Enables a transparent page background.
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Background color, as `0xRRGGBB`.
    pub fn background_color(mut self, color: u32) -> Self {
        self.background_color = Some(color & 0x00FF_FFFF);
        self
    }

    /// Enables or disables the built in context menu.
    pub fn context_menu(mut self, enabled: bool) -> Self {
        self.context_menu = enabled;
        self
    }

    /// Overrides the user agent.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Initial zoom factor.
    pub fn zoom(mut self, factor: f32) -> Self {
        self.zoom = Some(factor);
        self
    }

    /// `Accept-Language` header value.
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Root directory for the disk cache.
    pub fn cache_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(path.into());
        self
    }

    /// Local storage file path.
    pub fn local_storage(mut self, path: impl Into<PathBuf>) -> Self {
        self.local_storage = Some(path.into());
        self
    }

    /// Cookie jar file path.
    pub fn cookie_jar(mut self, path: impl Into<PathBuf>) -> Self {
        self.cookie_jar = Some(path.into());
        self
    }

    /// Script executed in every new javascript context, before page scripts.
    pub fn initial_script(mut self, script: impl Into<String>) -> Self {
        self.init_scripts.push(script.into());
        self
    }

    /// Registers the network interceptor.
    pub fn request_handler(mut self, handler: Arc<dyn RequestHandler>) -> Self {
        self.request_handler = Some(handler);
        self
    }

    /// Registers the paint callback. Disables automatic blitting to the window.
    pub fn on_paint(mut self, handler: impl Fn(isize, i32, i32, i32, i32) + Send + 'static) -> Self {
        self.handlers.paint = Some(Box::new(handler));
        self.auto_draw = false;
        self
    }

    /// Registers the title change callback.
    pub fn on_title_changed(mut self, handler: impl Fn(&str) + Send + 'static) -> Self {
        self.handlers.title_changed = Some(Box::new(handler));
        self
    }

    /// Registers the url change callback.
    pub fn on_url_changed(mut self, handler: impl Fn(&str, bool, bool) + Send + 'static) -> Self {
        self.handlers.url_changed = Some(Box::new(handler));
        self
    }

    /// Registers the document ready callback.
    pub fn on_document_ready(mut self, handler: impl Fn() + Send + 'static) -> Self {
        self.handlers.document_ready = Some(Box::new(handler));
        self
    }

    /// Registers the load finished callback.
    pub fn on_loading_finished(mut self, handler: impl Fn(&str, bool) + Send + 'static) -> Self {
        self.handlers.loading_finished = Some(Box::new(handler));
        self
    }

    /// Registers the console callback.
    pub fn on_console(mut self, handler: impl Fn(i32, &str, &str) + Send + 'static) -> Self {
        self.handlers.console = Some(Box::new(handler));
        self
    }

    /// Registers the navigation guard.
    pub fn on_navigation(mut self, handler: impl Fn(&str) -> bool + Send + 'static) -> Self {
        self.handlers.navigation = Some(Box::new(handler));
        self
    }

    /// Registers the close request callback.
    pub fn on_close_requested(mut self, handler: impl Fn() -> bool + Send + 'static) -> Self {
        self.handlers.close_requested = Some(Box::new(handler));
        self
    }

    /// Creates the webview.
    pub fn build(self) -> Result<WebView> {
        let api = sys::api().ok_or(Error::NotInitialized)?;
        if self.hwnd == 0 {
            return Err(Error::CreateWebView);
        }

        // SAFETY: engine is initialized; all pointers below outlive the calls.
        let raw = unsafe {
            if self.windowed {
                (api.mbCreateWebWindow)(sys::MB_WINDOW_TYPE_CONTROL)
            } else {
                (api.mbCreateWebView)()
            }
        };
        if raw == sys::NULL_WEBVIEW {
            return Err(Error::CreateWebView);
        }

        let inner = Arc::new(Inner {
            raw,
            handlers: Mutex::new(self.handlers),
            request_handler: Mutex::new(self.request_handler),
            scripts: Mutex::new(self.init_scripts),
            paint_hwnd: AtomicIsize::new(self.hwnd),
            visible: AtomicBool::new(true),
            destroyed: AtomicBool::new(false),
        });

        // Leaked on purpose: the engine keeps the pointer for the lifetime of the
        // view and may fire callbacks after the `WebView` is dropped.
        let param = Arc::into_raw(Arc::clone(&inner)) as *mut c_void;

        unsafe {
            if !self.windowed {
                (api.mbSetHandle)(raw, self.hwnd);
                (api.mbSetHandleOffset)(raw, self.rect.x, self.rect.y);
                (api.mbResize)(raw, self.rect.width.max(1), self.rect.height.max(1));
                (api.mbSetAutoDrawToHwnd)(raw, bool_to_c(self.auto_draw));
            }
            (api.mbSetTransparent)(raw, bool_to_c(self.transparent));
            (api.mbSetContextMenuEnabled)(raw, bool_to_c(self.context_menu));
            (api.mbSetMouseEnabled)(raw, 1);
            (api.mbSetTouchEnabled)(raw, 0);

            if let Some(color) = self.background_color {
                let settings = sys::mbViewSettings {
                    size: std::mem::size_of::<sys::mbViewSettings>() as i32,
                    bg_color: color,
                };
                (api.mbSetViewSettings)(raw, &settings);
            }
            if let Some(user_agent) = &self.user_agent
                && let Ok(value) = cstr(user_agent)
            {
                (api.mbSetUserAgent)(raw, value.as_ptr());
            }
            if let Some(language) = &self.language
                && let Ok(value) = cstr(language)
            {
                (api.mbSetLanguage)(raw, value.as_ptr());
            }
            if let Some(zoom) = self.zoom {
                (api.mbSetZoomFactor)(raw, zoom);
            }
            if let Some(factor) = self.device_scale_factor {
                let key = cstr("deviceScaleFactor")?;
                let value = cstr(&format!("{factor}"))?;
                (api.mbSetDeviceParameter)(raw, key.as_ptr(), value.as_ptr(), 1, factor);
            }
            if let Some(dir) = &self.cache_dir {
                let cache = dir.join("cache");
                if let Ok(path) = wide(&cache) {
                    (api.mbSetDiskCachePath)(raw, path.as_ptr());
                    (api.mbSetDiskCacheEnabled)(raw, 1);
                }
            }
            if let Some(path) = &self.local_storage
                && let Ok(path) = wide(path)
            {
                (api.mbSetLocalStorageFullPath)(raw, path.as_ptr());
            }
            if let Some(path) = &self.cookie_jar
                && let Ok(path) = wide(path)
            {
                (api.mbSetCookieJarFullPath)(raw, path.as_ptr());
            }

            (api.mbOnTitleChanged)(raw, Some(cb_title_changed), param);
            (api.mbOnURLChanged)(raw, Some(cb_url_changed), param);
            (api.mbOnDocumentReady)(raw, Some(cb_document_ready), param);
            (api.mbOnLoadingFinish)(raw, Some(cb_loading_finished), param);
            (api.mbOnConsole)(raw, Some(cb_console), param);
            (api.mbOnNavigation)(raw, Some(cb_navigation), param);
            (api.mbOnLoadUrlBegin)(raw, Some(cb_load_url_begin), param);
            if !self.windowed {
                (api.mbOnPaintUpdated)(raw, Some(cb_paint), param);
            }
            (api.mbOnDidCreateScriptContext)(raw, Some(cb_script_context), param);
            (api.mbOnClose)(raw, Some(cb_close), param);

            if let Some((html, base)) = self.html {
                let html = cstr(&html)?;
                let base = base.as_deref().map(cstr).transpose()?;
                (api.mbLoadHtmlWithBaseUrl)(
                    raw,
                    html.as_ptr(),
                    base.as_ref().map(|b| b.as_ptr()).unwrap_or(std::ptr::null()),
                );
            } else if let Some(url) = self.url {
                let url = cstr(&url)?;
                (api.mbLoadURL)(raw, url.as_ptr());
            }
        }

        Ok(WebView { inner })
    }
}

/// A Miniblink webview rendering offscreen into a native window.
pub struct WebView {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for WebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebView")
            .field("raw", &self.inner.raw)
            .finish()
    }
}

impl WebView {
    /// Raw engine handle.
    pub fn raw(&self) -> sys::mbWebView {
        self.inner.raw
    }

    /// Native window owned by the engine (window mode only).
    pub fn host_hwnd(&self) -> isize {
        match sys::api() {
            // SAFETY: engine is initialized and the view is alive.
            Some(api) => unsafe { (api.mbGetHostHWND)(self.inner.raw) },
            None => 0,
        }
    }

    /// Runs `f` with the engine memory DC while it is locked.
    ///
    /// Used to republish the last painted frame when the host window is
    /// uncovered.
    pub fn with_locked_dc<R>(&self, f: impl FnOnce(isize) -> R) -> Option<R> {
        if !self.inner.alive() {
            return None;
        }
        let api = sys::api()?;
        // SAFETY: the engine is initialized and the view is alive.
        let hdc = unsafe { (api.mbGetLockedViewDC)(self.inner.raw) };
        if hdc == 0 {
            return None;
        }
        let result = f(hdc);
        unsafe { (api.mbUnlockViewDC)(self.inner.raw) };
        Some(result)
    }

    /// Navigates to `url`.
    pub fn load_url(&self, url: &str) {
        if !self.inner.alive() {
            return;
        }
        if let (Some(api), Ok(url)) = (sys::api(), cstr(url)) {
            unsafe { (api.mbLoadURL)(self.inner.raw, url.as_ptr()) };
        }
    }

    /// Loads html from memory.
    pub fn load_html(&self, html: &str, base_url: Option<&str>) {
        if let Some(api) = sys::api()
            && let Ok(html) = cstr(html)
        {
            let base = base_url.and_then(|url| cstr(url).ok());
            unsafe {
                (api.mbLoadHtmlWithBaseUrl)(
                    self.inner.raw,
                    html.as_ptr(),
                    base.as_ref().map(|b| b.as_ptr()).unwrap_or(std::ptr::null()),
                )
            };
        }
    }

    /// Runs script and forgets the result.
    pub fn eval(&self, script: &str) {
        if !self.inner.alive() {
            return;
        }
        if let (Some(api), Ok(script)) = (sys::api(), cstr(script)) {
            unsafe {
                (api.mbRunJs)(
                    self.inner.raw,
                    main_frame(self.inner.raw),
                    script.as_ptr(),
                    0,
                    Some(cb_noop_js),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
        }
    }

    /// Runs script and forwards the stringified result to `callback`.
    pub fn eval_with_callback<F>(&self, script: &str, callback: F)
    where
        F: Fn(String) + Send + 'static,
    {
        if !self.inner.alive() {
            return;
        }
        let (Some(api), Ok(script)) = (sys::api(), cstr(script)) else {
            return;
        };
        let boxed: Box<dyn Fn(String) + Send> = Box::new(callback);
        let param = Box::into_raw(Box::new(boxed)) as *mut c_void;
        unsafe {
            (api.mbRunJs)(
                self.inner.raw,
                main_frame(self.inner.raw),
                script.as_ptr(),
                0,
                Some(cb_run_js),
                param,
                std::ptr::null_mut(),
            )
        };
    }

    /// Resizes and/or moves the view inside its host window.
    pub fn set_bounds(&self, rect: Rect) {
        if !self.inner.alive() {
            return;
        }
        if let Some(api) = sys::api() {
            unsafe {
                (api.mbSetHandleOffset)(self.inner.raw, rect.x, rect.y);
                (api.mbResize)(self.inner.raw, rect.width.max(1), rect.height.max(1));
            }
        }
    }

    /// Current bounds.
    pub fn bounds(&self) -> Rect {
        let mut rect = sys::mbRect::default();
        if let Some(api) = sys::api() {
            unsafe { (api.mbGetSize)(self.inner.raw, &mut rect) };
        }
        Rect::new(rect.x, rect.y, rect.w, rect.h)
    }

    /// Shows or hides the view.
    pub fn set_visible(&self, visible: bool) {
        self.inner.visible.store(visible, Ordering::SeqCst);
        if let Some(api) = sys::api() {
            unsafe { (api.mbShowWindow)(self.inner.raw, bool_to_c(visible)) };
        }
    }

    /// Whether the view is currently visible.
    pub fn is_visible(&self) -> bool {
        self.inner.visible.load(Ordering::SeqCst)
    }

    /// Focuses the view.
    pub fn focus(&self) {
        if !self.inner.alive() {
            return;
        }
        if let Some(api) = sys::api() {
            unsafe { (api.mbSetFocus)(self.inner.raw) };
        }
    }

    /// Removes focus from the view.
    pub fn blur(&self) {
        if !self.inner.alive() {
            return;
        }
        if let Some(api) = sys::api() {
            unsafe { (api.mbKillFocus)(self.inner.raw) };
        }
    }

    /// Reloads the current page.
    pub fn reload(&self) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbReload)(self.inner.raw) };
        }
    }

    /// Stops the current navigation.
    pub fn stop(&self) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbStopLoading)(self.inner.raw) };
        }
    }

    /// Navigates back in history.
    pub fn go_back(&self) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbGoBack)(self.inner.raw) };
        }
    }

    /// Navigates forward in history.
    pub fn go_forward(&self) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbGoForward)(self.inner.raw) };
        }
    }

    /// Whether a back navigation is possible.
    pub fn can_go_back(&self) -> bool {
        sys::api()
            .map(|api| unsafe { (api.mbCanGoBackOrForward)(self.inner.raw, 1) } != 0)
            .unwrap_or(false)
    }

    /// Whether a forward navigation is possible.
    pub fn can_go_forward(&self) -> bool {
        sys::api()
            .map(|api| unsafe { (api.mbCanGoBackOrForward)(self.inner.raw, 0) } != 0)
            .unwrap_or(false)
    }

    /// Current url.
    pub fn url(&self) -> String {
        match sys::api() {
            Some(api) => unsafe { from_cstr((api.mbGetUrl)(self.inner.raw)) },
            None => String::new(),
        }
    }

    /// Current document title.
    pub fn title(&self) -> String {
        match sys::api() {
            Some(api) => unsafe { from_cstr((api.mbGetTitle)(self.inner.raw)) },
            None => String::new(),
        }
    }

    /// Sets the zoom factor.
    pub fn set_zoom(&self, factor: f32) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbSetZoomFactor)(self.inner.raw, factor) };
        }
    }

    /// Updates the device scale factor used by the engine.
    pub fn set_device_scale_factor(&self, factor: f32) {
        if let Some(api) = sys::api()
            && let (Ok(key), Ok(value)) = (cstr("deviceScaleFactor"), cstr(&format!("{factor}")))
        {
            unsafe {
                (api.mbSetDeviceParameter)(self.inner.raw, key.as_ptr(), value.as_ptr(), 1, factor)
            };
        }
    }

    /// Current zoom factor.
    pub fn zoom(&self) -> f32 {
        sys::api()
            .map(|api| unsafe { (api.mbGetZoomFactor)(self.inner.raw) })
            .unwrap_or(1.0)
    }

    /// Enables a transparent background.
    pub fn set_transparent(&self, transparent: bool) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbSetTransparent)(self.inner.raw, bool_to_c(transparent)) };
        }
    }

    /// Sets the page background color (`0xRRGGBB`).
    pub fn set_background_color(&self, color: u32) {
        if let Some(api) = sys::api() {
            let settings = sys::mbViewSettings {
                size: std::mem::size_of::<sys::mbViewSettings>() as i32,
                bg_color: color & 0x00FF_FFFF,
            };
            unsafe { (api.mbSetViewSettings)(self.inner.raw, &settings) };
        }
    }

    /// Overrides the user agent.
    pub fn set_user_agent(&self, user_agent: &str) {
        if let (Some(api), Ok(value)) = (sys::api(), cstr(user_agent)) {
            unsafe { (api.mbSetUserAgent)(self.inner.raw, value.as_ptr()) };
        }
    }

    /// Enables or disables the context menu.
    pub fn set_context_menu(&self, enabled: bool) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbSetContextMenuEnabled)(self.inner.raw, bool_to_c(enabled)) };
        }
    }

    /// Sets an engine debug flag.
    pub fn set_debug_config(&self, key: &str, value: &str) {
        if let (Some(api), Ok(key), Ok(value)) = (sys::api(), cstr(key), cstr(value)) {
            unsafe { (api.mbSetDebugConfig)(self.inner.raw, key.as_ptr(), value.as_ptr()) };
        }
    }

    /// Sets the window the view blits into.
    pub fn set_paint_window(&self, hwnd: isize) {
        self.inner.paint_hwnd.store(hwnd, Ordering::SeqCst);
    }

    /// The window the view blits into.
    pub fn paint_window(&self) -> isize {
        self.inner.paint_hwnd.load(Ordering::SeqCst)
    }

    /// Sets the cookie jar path.
    pub fn set_cookie_jar(&self, path: &std::path::Path) {
        if let (Some(api), Ok(path)) = (sys::api(), wide(path)) {
            unsafe { (api.mbSetCookieJarFullPath)(self.inner.raw, path.as_ptr()) };
        }
    }

    /// Sets a cookie for `url`.
    pub fn set_cookie(&self, url: &str, cookie: &str) {
        if let (Some(api), Ok(url), Ok(cookie)) = (sys::api(), cstr(url), cstr(cookie)) {
            unsafe { (api.mbSetCookie)(self.inner.raw, url.as_ptr(), cookie.as_ptr()) };
        }
    }

    /// Reads the cookie store asynchronously.
    pub fn cookies_with<F>(&self, callback: F)
    where
        F: Fn(String) + Send + 'static,
    {
        let Some(api) = sys::api() else { return };
        let boxed: Box<dyn Fn(String) + Send> = Box::new(callback);
        let param = Box::into_raw(Box::new(boxed)) as *mut c_void;
        unsafe { (api.mbGetCookie)(self.inner.raw, Some(cb_get_cookie), param) };
    }

    /// Clears every cookie.
    pub fn clear_cookies(&self) {
        if let Some(api) = sys::api() {
            unsafe {
                (api.mbClearCookie)(self.inner.raw);
                (api.mbPerformCookieCommand)(self.inner.raw, sys::MB_COOKIE_COMMAND_CLEAR_ALL_COOKIES);
            }
        }
    }

    /// Enables or disables the disk cache.
    pub fn set_disk_cache(&self, enabled: bool) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbSetDiskCacheEnabled)(self.inner.raw, bool_to_c(enabled)) };
        }
    }

    /// Prints the page using the engine's default settings.
    pub fn print(&self) -> bool {
        let Some(api) = sys::api() else { return false };
        let settings = sys::mbPrintSettings {
            struct_size: std::mem::size_of::<sys::mbPrintSettings>() as i32,
            ..Default::default()
        };
        unsafe { (api.mbUtilPrint)(self.inner.raw, main_frame(self.inner.raw), &settings) != 0 }
    }

    /// Feeds a mouse message into the engine through the dedicated input APIs.
    ///
    /// `lparam` must hold client area coordinates (`WM_MOUSEWHEEL` is the
    /// exception, the caller has to convert it first).
    /// Returns whether the message was consumed.
    pub fn dispatch_mouse(&self, message: u32, wparam: usize, lparam: isize) -> bool {
        const WM_MOUSEMOVE: u32 = 0x0200;
        const WM_LBUTTONDOWN: u32 = 0x0201;
        const WM_LBUTTONUP: u32 = 0x0202;
        const WM_LBUTTONDBLCLK: u32 = 0x0203;
        const WM_RBUTTONDOWN: u32 = 0x0204;
        const WM_RBUTTONUP: u32 = 0x0205;
        const WM_RBUTTONDBLCLK: u32 = 0x0206;
        const WM_MBUTTONDOWN: u32 = 0x0207;
        const WM_MBUTTONUP: u32 = 0x0208;
        const WM_MBUTTONDBLCLK: u32 = 0x0209;
        const WM_MOUSEWHEEL: u32 = 0x020A;
        const WM_MOUSELEAVE: u32 = 0x02A3;

        let Some(api) = sys::api() else {
            return false;
        };
        let raw = self.inner.raw;
        let flags = wparam as u32;
        let x = (lparam & 0xFFFF) as u16 as i16 as i32;
        let y = ((lparam >> 16) & 0xFFFF) as u16 as i16 as i32;

        // SAFETY: engine is initialized and the view is alive.
        let handled = unsafe {
            match message {
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK
                | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN
                | WM_MBUTTONUP | WM_MBUTTONDBLCLK => {
                    (api.mbFireMouseEvent)(raw, message, x, y, flags)
                }
                WM_MOUSEWHEEL => {
                    let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16 as i32;
                    (api.mbFireMouseWheelEvent)(raw, x, y, delta, flags)
                }
                WM_MOUSELEAVE => (api.mbFireMouseEvent)(raw, WM_MOUSELEAVE, x, y, 0),
                _ => return false,
            }
        };

        handled != 0
    }

    /// Sends an already committed character to the engine.
    ///
    /// The engine has no IME support of its own, so committed IME strings have
    /// to be replayed as key presses by the host.
    pub fn dispatch_char(&self, code: u32) -> bool {
        if !self.inner.alive() {
            return false;
        }
        let Some(api) = sys::api() else {
            return false;
        };
        // SAFETY: engine is initialized and the view is alive.
        let handled = unsafe { (api.mbFireKeyPressEvent)(self.inner.raw, code, 0, 0) };
        handled != 0
    }

    /// Feeds a keyboard message into the engine through the dedicated input
    /// APIs.
    ///
    /// `mbFireWindowsMessage` does not route keyboard messages in offscreen
    /// mode, which is why the runtime calls this for `WM_KEY*`/`WM_CHAR`.
    /// Returns whether the message was consumed.
    pub fn dispatch_keyboard(&self, message: u32, wparam: usize, lparam: isize) -> bool {
        const WM_KEYDOWN: u32 = 0x0100;
        const WM_KEYUP: u32 = 0x0101;
        const WM_CHAR: u32 = 0x0102;
        const WM_SYSKEYDOWN: u32 = 0x0104;
        const WM_SYSKEYUP: u32 = 0x0105;
        const WM_SYSCHAR: u32 = 0x0106;

        let Some(api) = sys::api() else {
            return false;
        };
        let raw = self.inner.raw;
        let key = wparam as u32;
        let flags = lparam as u32;

        // SAFETY: engine is initialized and the view is alive.
        let handled = unsafe {
            match message {
                WM_KEYDOWN => (api.mbFireKeyDownEvent)(raw, key, flags, 0),
                WM_SYSKEYDOWN => (api.mbFireKeyDownEvent)(raw, key, flags, 1),
                WM_KEYUP => (api.mbFireKeyUpEvent)(raw, key, flags, 0),
                WM_SYSKEYUP => (api.mbFireKeyUpEvent)(raw, key, flags, 1),
                WM_CHAR => (api.mbFireKeyPressEvent)(raw, key, flags, 0),
                WM_SYSCHAR => (api.mbFireKeyPressEvent)(raw, key, flags, 1),
                _ => return false,
            }
        };

        handled != 0
    }

    /// Forwards a native window message into the engine.
    ///
    /// Returns the engine result when the message was consumed.
    pub fn forward_message(
        &self,
        hwnd: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> Option<isize> {
        if !self.inner.alive() {
            return None;
        }
        let api = sys::api()?;
        let mut result: sys::LRESULT = 0;
        let handled =
            unsafe { (api.mbFireWindowsMessage)(self.inner.raw, hwnd, message, wparam, lparam, &mut result) };
        (handled != 0).then_some(result)
    }

    /// Current cursor type (see `K_MB_CURSOR_INFO_*`).
    pub fn cursor_type(&self) -> i32 {
        sys::api()
            .map(|api| unsafe { (api.mbGetCursorInfoType)(self.inner.raw) })
            .unwrap_or(0)
    }

    /// Destroys the underlying engine view.
    pub fn destroy(&self) {
        if !self.inner.destroyed.swap(true, Ordering::SeqCst)
            && let Some(api) = sys::api()
        {
            unsafe { (api.mbDestroyWebView)(self.inner.raw) };
        }
    }

    /// Whether the engine view has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.inner.destroyed.load(Ordering::SeqCst)
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn main_frame(view: sys::mbWebView) -> sys::mbWebFrameHandle {
    match sys::api() {
        // SAFETY: `view` is a live handle owned by the caller.
        Some(api) => unsafe { (api.mbWebFrameGetMainFrame)(view) },
        None => std::ptr::null_mut(),
    }
}

unsafe fn inner_from<'a>(param: *mut c_void) -> Option<&'a Inner> {
    if param.is_null() {
        None
    } else {
        Some(unsafe { &*(param as *const Inner) })
    }
}

unsafe extern "system" fn cb_title_changed(
    _view: sys::mbWebView,
    param: *mut c_void,
    title: *const i8,
) {
    let Some(inner) = (unsafe { inner_from(param) }) else { return };
    let title = unsafe { from_cstr(title) };
    let handlers = inner.handlers();
    if let Some(callback) = &handlers.title_changed {
        callback(&title);
    }
}

unsafe extern "system" fn cb_url_changed(
    _view: sys::mbWebView,
    param: *mut c_void,
    url: *const i8,
    can_go_back: sys::BOOL,
    can_go_forward: sys::BOOL,
) {
    let Some(inner) = (unsafe { inner_from(param) }) else { return };
    let url = unsafe { from_cstr(url) };
    let handlers = inner.handlers();
    if let Some(callback) = &handlers.url_changed {
        callback(&url, can_go_back != 0, can_go_forward != 0);
    }
}

unsafe extern "system" fn cb_document_ready(
    _view: sys::mbWebView,
    param: *mut c_void,
    _frame: sys::mbWebFrameHandle,
) {
    let Some(inner) = (unsafe { inner_from(param) }) else { return };
    let handlers = inner.handlers();
    if let Some(callback) = &handlers.document_ready {
        callback();
    }
}

unsafe extern "system" fn cb_loading_finished(
    _view: sys::mbWebView,
    param: *mut c_void,
    _frame: sys::mbWebFrameHandle,
    _url: *const i8,
    result: i32,
    _failed_reason: *const i8,
) {
    let Some(inner) = (unsafe { inner_from(param) }) else { return };
    let url = unsafe { from_cstr(_url) };
    let handlers = inner.handlers();
    if let Some(callback) = &handlers.loading_finished {
        callback(&url, result == sys::MB_LOADING_SUCCEEDED);
    }
}

unsafe extern "system" fn cb_console(
    _view: sys::mbWebView,
    param: *mut c_void,
    level: i32,
    message: *const i8,
    source: *const i8,
    _line: u32,
    _stack: *const i8,
) {
    let Some(inner) = (unsafe { inner_from(param) }) else { return };
    let message = unsafe { from_cstr(message) };
    let source = unsafe { from_cstr(source) };
    let handlers = inner.handlers();
    if let Some(callback) = &handlers.console {
        callback(level, &message, &source);
    }
}

unsafe extern "system" fn cb_navigation(
    _view: sys::mbWebView,
    param: *mut c_void,
    _navigation_type: i32,
    url: *const i8,
) -> sys::BOOL {
    let Some(inner) = (unsafe { inner_from(param) }) else {
        return 1;
    };
    let url = unsafe { from_cstr(url) };
    let handlers = inner.handlers();
    match &handlers.navigation {
        Some(callback) => bool_to_c(callback(&url)),
        None => 1,
    }
}

unsafe extern "system" fn cb_load_url_begin(
    _view: sys::mbWebView,
    param: *mut c_void,
    url: *const i8,
    job: sys::mbNetJob,
) -> sys::BOOL {
    let Some(inner) = (unsafe { inner_from(param) }) else {
        return 0;
    };
    let handler = inner
        .request_handler
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(handler) = handler else {
        return 0;
    };

    let url = unsafe { from_cstr(url) };
    let body = net::read_post_body(job);
    let headers = net::read_request_headers(job);

    let method = match sys::api() {
        // SAFETY: blink thread, live job.
        Some(api) => match unsafe { (api.mbNetGetRequestMethod)(job) } {
            sys::MB_REQUEST_TYPE_POST => "POST",
            sys::MB_REQUEST_TYPE_PUT => "PUT",
            sys::MB_REQUEST_TYPE_GET => "GET",
            _ if body.is_empty() => "GET",
            _ => "POST",
        },
        None => "GET",
    };

    let request = net::Request {
        url,
        method: method.to_string(),
        headers,
        body,
        job: NetJob::from_raw(job),
    };

    match handler.on_request(request) {
        RequestAction::Continue => 0,
        RequestAction::Hold => 1,
        RequestAction::Respond(response) => {
            NetJob::from_raw(job).respond(&response);
            1
        }
    }
}

unsafe extern "system" fn cb_paint(
    _view: sys::mbWebView,
    param: *mut c_void,
    hdc: sys::HDC,
    x: i32,
    y: i32,
    cx: i32,
    cy: i32,
) {
    let Some(inner) = (unsafe { inner_from(param) }) else { return };
    if !inner.visible.load(Ordering::SeqCst) {
        return;
    }
    let handlers = inner.handlers();
    if let Some(callback) = &handlers.paint {
        callback(hdc, x, y, cx, cy);
    }
}

unsafe extern "system" fn cb_script_context(
    _view: sys::mbWebView,
    param: *mut c_void,
    frame: sys::mbWebFrameHandle,
    context: *mut c_void,
    _extension_group: i32,
    _world_id: i32,
) {
    let Some(inner) = (unsafe { inner_from(param) }) else { return };
    {
        let handlers = inner.handlers();
        if let Some(callback) = &handlers.script_context_created {
            callback(context);
        }
    }

    let scripts = inner
        .scripts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    if trace_enabled() {
        eprintln!(
            "[mb] script ctx view={} frame={frame:?} ctx={context:?} scripts={}",
            inner.raw,
            scripts.len()
        );
    }

    if let Some(api) = sys::api() {
        for script in scripts {
            if let Ok(script) = cstr(&script) {
                // SAFETY: called from the context-created callback, the frame is valid.
                unsafe {
                    (api.mbRunJs)(
                        inner.raw,
                        frame,
                        script.as_ptr(),
                        0,
                        Some(cb_noop_js),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
            }
        }
    }
}

fn trace_enabled() -> bool {
    std::env::var_os("MB_TRACE").is_some()
}

unsafe extern "system" fn cb_close(
    _view: sys::mbWebView,
    param: *mut c_void,
    _unuse: *mut c_void,
) -> sys::BOOL {
    let Some(inner) = (unsafe { inner_from(param) }) else {
        return 1;
    };
    let handlers = inner.handlers();
    match &handlers.close_requested {
        Some(callback) => bool_to_c(callback()),
        None => 1,
    }
}

/// Callback used when the result of a script is not needed.
///
/// The engine only executes scripts reliably when a callback is provided.
unsafe extern "system" fn cb_noop_js(
    _view: sys::mbWebView,
    _param: *mut c_void,
    _es: sys::mbJsExecState,
    _value: sys::mbJsValue,
) {
}

unsafe extern "system" fn cb_run_js(
    _view: sys::mbWebView,
    param: *mut c_void,
    es: sys::mbJsExecState,
    value: sys::mbJsValue,
) {
    if param.is_null() {
        return;
    }
    // SAFETY: allocated with `Box::into_raw` in `eval_with_callback`.
    let callback = unsafe { Box::from_raw(param as *mut Box<dyn Fn(String) + Send>) };
    let result = match sys::api() {
        Some(api) => unsafe { from_cstr((api.mbJsToString)(es, value)) },
        None => String::new(),
    };
    callback(result);
}

unsafe extern "system" fn cb_get_cookie(
    _view: sys::mbWebView,
    param: *mut c_void,
    _state: i32,
    cookie: *const i8,
) {
    if param.is_null() {
        return;
    }
    // SAFETY: allocated with `Box::into_raw` in `cookies_with`.
    let callback = unsafe { Box::from_raw(param as *mut Box<dyn Fn(String) + Send>) };
    let cookie = unsafe { from_cstr(cookie) };
    callback(cookie);
}
