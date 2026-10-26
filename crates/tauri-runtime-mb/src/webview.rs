//! Webview creation and the [`WebviewDispatch`] implementation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tauri_runtime::dpi::{PhysicalPosition, PhysicalSize, Position, Rect, Size};
use tauri_runtime::webview::{
    DetachedWebview, PageLoadEvent, PendingWebview, UriSchemeProtocolHandler, WebviewIpcHandler,
};
use tauri_runtime::window::{WebviewEvent, WindowId};
use tauri_runtime::{
    Cookie, Error, Result, RunEvent, UserEvent, WebviewDispatch, WebviewEventId,
};
use tauri_utils::config::Color;
use url::Url;

use crate::shared::Shared;
use crate::sys;
use crate::window::{physical_position, physical_size, WindowInner};
use crate::MbRuntime;

type WebviewEventHandler = Arc<Mutex<Box<dyn Fn(&WebviewEvent) + Send>>>;

/// Handle to the underlying Miniblink view.
#[derive(Debug, Clone, Copy)]
pub struct MbPlatformWebview {
    /// Host window handle.
    pub hwnd: isize,
    /// Raw `mbWebView` handle.
    pub raw_webview: i64,
}

pub(crate) struct WebviewInner<T: UserEvent> {
    pub(crate) label: String,
    pub(crate) window: Arc<WindowInner<T>>,
    pub(crate) webview: mb132::WebView,
    pub(crate) handlers: Mutex<HashMap<WebviewEventId, WebviewEventHandler>>,
    pub(crate) auto_resize: AtomicBool,
    pub(crate) bounds: Mutex<(i32, i32, u32, u32)>,
    pub(crate) use_https_scheme: bool,
    pub(crate) devtools: Option<bool>,
    /// Whether the engine owns the native window (no offscreen blitting).
    pub(crate) windowed: bool,
    /// Handle of the window owned by the engine (window mode).
    pub(crate) engine_hwnd: std::sync::atomic::AtomicIsize,
    /// Device pixel ratio the engine actually uses, stored as `f64` bits.
    ///
    /// Read back from `window.devicePixelRatio` because the value the engine
    /// picks does not always follow the window dpi (and it changes when the
    /// window moves to another monitor).
    pub(crate) dpr: std::sync::atomic::AtomicU64,
}

impl<T: UserEvent> WebviewInner<T> {
    pub(crate) fn auto_resize(&self) -> bool {
        self.auto_resize.load(Ordering::SeqCst)
    }

    /// Scale factor that maps css pixels to the physical size of the view.
    pub(crate) fn device_pixel_ratio(&self) -> f64 {
        let value = f64::from_bits(self.dpr.load(Ordering::SeqCst));
        if value > 0.0 { value } else { 1.0 }
    }



    pub(crate) fn set_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        let width = width.max(1);
        let height = height.max(1);
        *self.bounds.lock().unwrap() = (x, y, width as u32, height as u32);

        let engine = self.engine();
        if engine != 0 {
            // SAFETY: engine is a live child window.
            unsafe {
                sys::SetWindowPos(
                    engine,
                    0,
                    x,
                    y,
                    width,
                    height,
                    sys::SWP_NOZORDER | sys::SWP_NOACTIVATE | sys::SWP_SHOWWINDOW,
                )
            };
            return;
        }

        // viewport == physical client area
        self.webview
            .set_bounds(mb132::Rect::new(x, y, width, height));
    }

    /// Handle of the window owned by the engine (0 in offscreen mode).
    pub(crate) fn engine(&self) -> isize {
        self.engine_hwnd.load(Ordering::SeqCst)
    }

    pub(crate) fn focus(&self) {
        let engine = self.engine();
        if engine != 0 {
            // SAFETY: engine is a live child window.
            unsafe { sys::SetFocus(engine) };
            return;
        }
        self.webview.focus();
    }

    pub(crate) fn blur(&self) {
        self.webview.blur();
    }

    pub(crate) fn destroy(&self) {
        self.webview.destroy();
    }

    pub(crate) fn forward(
        &self,
        hwnd: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> Option<isize> {
        self.webview.forward_message(hwnd, message, wparam, lparam)
    }

    /// Republishes the last frame, used when the window needs a repaint.
    pub(crate) fn blit(&self, hdc: isize) {
        if self.windowed {
            return;
        }
        let (_, _, width, height) = *self.bounds.lock().unwrap();
        if width == 0 || height == 0 {
            return;
        }
        let _ = self.webview.with_locked_dc(|source| unsafe {
            sys::BitBlt(
                hdc,
                0,
                0,
                width as i32,
                height as i32,
                source,
                0,
                0,
                sys::SRCCOPY,
            );
        });
    }

    /// Delivers a webview event to the registered listeners and to the run loop.
    #[allow(dead_code)]
    fn emit(&self, event: WebviewEvent) {
        let handlers: Vec<WebviewEventHandler> = {
            let handlers = self.handlers.lock().unwrap();
            handlers.values().cloned().collect()
        };
        for handler in handlers {
            let callback = handler.lock().unwrap();
            callback(&event);
        }
        self.window.shared.push_event(RunEvent::WebviewEvent {
            label: self.label.clone(),
            event,
        });
    }
}

/// Custom protocol and IPC bridge for a single webview.
struct WebviewNet<T: UserEvent> {
    /// Label of the owning webview, handed to the protocol handlers.
    label: String,
    protocols: Mutex<HashMap<String, Box<UriSchemeProtocolHandler>>>,
    ipc_handler: Mutex<Option<WebviewIpcHandler<T, MbRuntime<T>>>>,
    self_ref: Mutex<Weak<WebviewInner<T>>>,
}

fn to_miniblink_response(
    response: http::Response<std::borrow::Cow<'static, [u8]>>,
) -> mb132::Response {
    let (parts, body) = response.into_parts();
    let mut result = mb132::Response {
        status: parts.status.as_u16(),
        headers: Vec::new(),
        mime: None,
        body: body.into_owned(),
    };

    for (name, value) in parts.headers.iter() {
        let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
        if name.as_str().eq_ignore_ascii_case("content-type") {
            result.mime = Some(value.clone());
        }
        result.headers.push((name.as_str().to_string(), value));
    }

    result
}

impl<T: UserEvent> mb132::RequestHandler for WebviewNet<T> {
    fn on_request(&self, request: mb132::Request) -> mb132::RequestAction {
        let Some((scheme, path)) = split_custom_url(&request.url) else {
            if trace_enabled() {
                eprintln!("[mb] pass-through {}", request.url);
            }
            return mb132::RequestAction::Continue;
        };

        if trace_enabled() {
            let headers: Vec<&str> = request.headers.iter().map(|(name, _)| name.as_str()).collect();
            eprintln!(
                "[mb] intercept scheme={scheme} path={path} method={} headers={headers:?} body={}",
                request.method,
                String::from_utf8_lossy(&request.body)
            );
        }

        // Internal channel: position the native candidates at the actual text
        // caret, including input padding, selection, scrolling and page zoom.
        if scheme == "mbcaret" {
            if let Some((caret, dpr)) = crate::ime::parse_report(&request.body)
                && let Some(webview) = self.self_ref.lock().ok().and_then(|weak| weak.upgrade())
            {
                *webview.window.ime_caret.lock().unwrap() = caret;
                webview.dpr.store(dpr.to_bits(), Ordering::SeqCst);
                let window = Arc::clone(&webview.window);
                webview.window.shared.post_task(Box::new(move || {
                    if window.ime_composing.load(Ordering::SeqCst)
                        && !window.destroyed.load(Ordering::SeqCst)
                    {
                        crate::ime::position_window(&window, window.hwnd());
                    }
                }));
            }
            return mb132::RequestAction::Respond(mb132::Response::ok(
                Vec::new(),
                "text/plain",
            ));
        }

        // Every scheme registered by tauri is served by its protocol handler,
        // including `ipc`: tauri 3 encodes the command in the url path
        // (`http://ipc.localhost/<cmd>`) and sends the payload as the body.
        let handler = {
            let mut protocols = self.protocols.lock().unwrap();
            protocols.remove(&scheme)
        };
        let Some(handler) = handler else {
            // Nothing registered for the scheme: fall back to the
            // `window.ipc.postMessage` bridge when it is the ipc scheme.
            if scheme == "ipc" {
                return self.handle_ipc(request);
            }
            return mb132::RequestAction::Continue;
        };

        let mut builder = http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let Ok(http_request) = builder.body(request.body.clone()) else {
            // Put the handler back and let the network stack deal with it.
            self.protocols.lock().unwrap().insert(scheme, handler);
            return mb132::RequestAction::Continue;
        };

        let (tx, rx) = std::sync::mpsc::channel();
        let responder: Box<
            dyn FnOnce(http::Response<std::borrow::Cow<'static, [u8]>>) + Send,
        > = Box::new(move |response| {
            let _ = tx.send(response);
        });

        (handler)(&self.label, http_request, responder);

        // Put the handler back so subsequent requests are served too.
        self.protocols
            .lock()
            .unwrap()
            .insert(scheme.clone(), handler);

        match rx.try_recv() {
            Ok(response) => {
                if trace_enabled() {
                    eprintln!(
                        "[mb] protocol {scheme} -> status={} len={} body={}",
                        response.status().as_u16(),
                        response.body().len(),
                        String::from_utf8_lossy(response.body())
                    );
                }
                mb132::RequestAction::Respond(to_miniblink_response(response))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                let job = request.job;
                job.hold();
                std::thread::spawn(move || {
                    if let Ok(response) = rx.recv() {
                        job.respond_async(to_miniblink_response(response));
                    }
                });
                mb132::RequestAction::Hold
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                mb132::RequestAction::Continue
            }
        }
    }
}

/// Whether the `MB_TRACE` environment variable is set.
pub(crate) fn trace_enabled() -> bool {
    std::env::var_os("MB_TRACE").is_some()
}

impl<T: UserEvent> WebviewNet<T> {
    fn handle_ipc(&self, request: mb132::Request) -> mb132::RequestAction {
        if trace_enabled() {
            eprintln!("[mb] ipc body_len={}", request.body.len());
        }
        let handler = {
            let mut handler = self.ipc_handler.lock().unwrap();
            handler.take()
        };
        let Some(handler) = handler else {
            return mb132::RequestAction::Continue;
        };

        let inner = self.self_ref.lock().unwrap().upgrade();
        if let Some(inner) = inner {
            let dispatcher = MbWebviewDispatcher {
                inner: Arc::clone(&inner),
            };
            let detached = DetachedWebview {
                label: inner.label.clone(),
                dispatcher,
            };
            let body = String::from_utf8_lossy(&request.body).into_owned();

            // tauri derives the origin (and therefore the ACL context) of the
            // IPC message from the request URI, so it must be the document URL
            // rather than the internal `ipc.localhost` endpoint.
            let document_url = inner.webview.url();
            let uri = if document_url.is_empty() {
                request.url.as_str()
            } else {
                document_url.as_str()
            };

            let http_request = http::Request::builder()
                .method("POST")
                .uri(uri)
                .body(body)
                .unwrap_or_else(|_| {
                    http::Request::builder()
                        .method("POST")
                        .uri("http://tauri.localhost/")
                        .body(String::new())
                        .expect("static request is valid")
                });
            handler(detached, http_request);
        }

        // tauri re-registers the handler only when the webview is rebuilt, so
        // keep it around for the next message.
        *self.ipc_handler.lock().unwrap() = Some(handler);

        mb132::RequestAction::Respond(
            mb132::Response::ok(Vec::new(), "application/json")
                .with_header("Access-Control-Allow-Origin", "*"),
        )
    }
}

/// Converts a physical pixel rectangle into css pixels.
fn to_css_rect(x: i32, y: i32, width: i32, height: i32, scale: f64) -> (i32, i32, i32, i32) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    (
        (x as f64 / scale).round() as i32,
        (y as f64 / scale).round() as i32,
        (width as f64 / scale).round().max(1.0) as i32,
        (height as f64 / scale).round().max(1.0) as i32,
    )
}

/// Splits `http(s)://<scheme>.localhost/<path>` into `(scheme, /path)`.
fn split_custom_url(url: &str) -> Option<(String, String)> {
    let (protocol, rest) = url.split_once("://")?;
    if protocol != "http" && protocol != "https" {
        return None;
    }
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let host = authority.split(':').next().unwrap_or(authority);
    let scheme = host.strip_suffix(".localhost")?;
    if scheme.is_empty() {
        return None;
    }
    Some((scheme.to_string(), path.to_string()))
}

/// Creates the webview described by `pending` inside `window`.
pub(crate) fn create_webview_in_window<T: UserEvent>(
    window: &Arc<WindowInner<T>>,
    pending: PendingWebview<T, MbRuntime<T>>,
) -> Result<MbWebviewDispatcher<T>> {
    let PendingWebview {
        label,
        webview_attributes: attributes,
        url,
        ipc_handler,
        uri_scheme_protocols,
        navigation_handler,
        document_title_changed_handler,
        on_page_load_handler,
        ..
    } = pending;

    let hwnd = window.hwnd();
    let scale = window.scale_factor();

    if std::env::var_os("MB_TRACE").is_some() {
        eprintln!(
            "[mb] create_webview label={label} url={url} hwnd={hwnd} hwnd_alive={}",
            hwnd != 0
        );
    }

    // The webview attached to the window always fills its client area: the
    // window may have been resized by the system (dpi, min/max constraints) and
    // the client size is the only source of truth.
    let is_primary = window.webview().is_none();

    let (x, y, width, height) = match attributes.bounds {
        Some(rect) if !is_primary => {
            let (x, y) = physical_position(rect.position, scale);
            let (width, height) = physical_size(rect.size, scale);
            (x, y, width as i32, height as i32)
        }
        _ => {
            let (_, _, width, height) = sys::client_rect(hwnd);
            (0, 0, width, height)
        }
    };
    let (x, y, width, height) = (x, y, width.max(1), height.max(1));

    let net = Arc::new(WebviewNet::<T> {
        label: label.clone(),
        protocols: Mutex::new(uri_scheme_protocols),
        ipc_handler: Mutex::new(ipc_handler),
        self_ref: Mutex::new(Weak::new()),
    });

    // `mbResize` takes physical pixels: the engine divides them by the device
    // scale factor to get the css viewport and multiplies them back when
    // painting, so the surface matches the window exactly.
    // The engine sizes the render surface exactly as told (it does not scale it
    // by the device factor), so the viewport has to be the physical client
    // area: that keeps the page filling the window and mouse coordinates 1:1.
    let mut builder = mb132::WebViewBuilder::new(hwnd)
        .bounds(mb132::Rect::new(x, y, width, height))
        .transparent(attributes.transparent)
        .device_scale_factor(1.0)
        .initial_script(crate::ime::SCRIPT)
        .url(url.clone())
        .request_handler(Arc::clone(&net) as Arc<dyn mb132::RequestHandler>);

    if let Some(user_agent) = &attributes.user_agent {
        builder = builder.user_agent(user_agent.clone());
    }
    if let Some(directory) = &attributes.data_directory {
        builder = builder
            .cache_dir(directory.clone())
            .local_storage(directory.join("Local Storage"))
            .cookie_jar(directory.join("Cookies"));
    }
    for script in &attributes.initialization_scripts {
        builder = builder.initial_script(script.script.clone());
    }

    if trace_enabled() {
        eprintln!(
            "[mb] create_webview label={label} url={url} init_scripts={} bounds=({x},{y},{width},{height})",
            attributes.initialization_scripts.len()
        );
        builder = builder.on_console(|level, message, source| {
            eprintln!("[mb] console({level}) {source}: {message}");
        });
    }

    if let Some(handler) = document_title_changed_handler {
        if trace_enabled() {
            builder = builder.on_title_changed(move |title| {
                eprintln!("[mb] title changed: {title}");
                handler(title.to_string());
            });
        } else {
            builder = builder.on_title_changed(move |title| handler(title.to_string()));
        }
    } else if trace_enabled() {
        builder = builder.on_title_changed(|title| eprintln!("[mb] title changed: {title}"));
    }

    if let Some(handler) = navigation_handler {
        builder = builder.on_navigation(move |url| match Url::parse(url) {
            Ok(url) => handler(&url),
            Err(_) => true,
        });
    }

    if let Some(handler) = on_page_load_handler {
        let handler = Arc::new(Mutex::new(handler));

        let started = Arc::clone(&handler);
        let start_url = url.clone();
        builder = builder.on_document_ready(move || {
            if let Ok(parsed) = Url::parse(&start_url)
                && let Ok(callback) = started.lock()
            {
                let callback: &dyn Fn(Url, PageLoadEvent) = &**callback;
                callback(parsed, PageLoadEvent::Started);
            }
        });

        let finished = Arc::clone(&handler);
        let finish_url = url.clone();
        builder = builder.on_loading_finished(move |loaded, success| {
            if !success {
                return;
            }
            let parsed = Url::parse(loaded).or_else(|_| Url::parse(&finish_url));
            if let Ok(parsed) = parsed
                && let Ok(callback) = finished.lock()
            {
                let callback: &dyn Fn(Url, PageLoadEvent) = &**callback;
                callback(parsed, PageLoadEvent::Finished);
            }
        });
    }

    let webview = builder
        .build()
        .map_err(|error| Error::CreateWebview(Box::new(error)))?;



    let inner = Arc::new(WebviewInner::<T> {
        label,
        window: Arc::clone(window),
        webview,
        handlers: Mutex::new(HashMap::new()),
        auto_resize: AtomicBool::new(attributes.auto_resize || is_primary),
        bounds: Mutex::new((x, y, width as u32, height as u32)),
        use_https_scheme: attributes.use_https_scheme,
        devtools: attributes.devtools,
        dpr: std::sync::atomic::AtomicU64::new(scale.to_bits()),
        windowed: false,
        engine_hwnd: std::sync::atomic::AtomicIsize::new(0),
    });



    *net.self_ref.lock().unwrap() = Arc::downgrade(&inner);

    if is_primary {
        window.set_webview(Arc::clone(&inner));
    }

    if attributes.focus {
        inner.focus();
    }

    Ok(MbWebviewDispatcher { inner })
}

// ---------------------------------------------------------------------------
// dispatcher
// ---------------------------------------------------------------------------

/// Handle to a webview created by the Miniblink runtime.
pub struct MbWebviewDispatcher<T: UserEvent> {
    pub(crate) inner: Arc<WebviewInner<T>>,
}

impl<T: UserEvent> Clone for MbWebviewDispatcher<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: UserEvent> std::fmt::Debug for MbWebviewDispatcher<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbWebviewDispatcher")
            .field("label", &self.inner.label)
            .finish()
    }
}

impl<T: UserEvent> WebviewDispatch<T> for MbWebviewDispatcher<T> {
    type Runtime = MbRuntime<T>;

    fn run_on_main_thread<F: FnOnce() + Send + 'static>(&self, f: F) -> Result<()> {
        self.inner.window.shared.post_task(Box::new(f));
        Ok(())
    }

    fn on_webview_event<F: Fn(&WebviewEvent) + Send + 'static>(&self, f: F) -> WebviewEventId {
        let id = self.inner.window.shared.next_event_id();
        self.inner
            .handlers
            .lock()
            .unwrap()
            .insert(id, Arc::new(Mutex::new(Box::new(f))));
        id
    }

    fn with_webview<F: FnOnce(<Self::Runtime as tauri_runtime::Runtime<T>>::Webview) + Send + 'static>(
        &self,
        f: F,
    ) -> Result<()> {
        let handle = MbPlatformWebview {
            hwnd: self.inner.window.hwnd(),
            raw_webview: self.inner.webview.raw(),
        };
        self.inner
            .window
            .shared
            .run_on_main_thread(move || f(handle))
    }

    fn open_devtools(&self) {
        // The bundled devtools (minidevtools) attach out of process; enabling
        // the debug port is the closest equivalent.
        self.inner.webview.set_debug_config("devtools", "1");
    }

    fn close_devtools(&self) {}

    fn is_devtools_open(&self) -> Result<bool> {
        Ok(false)
    }

    fn url(&self) -> Result<String> {
        Ok(self.inner.webview.url())
    }

    fn bounds(&self) -> Result<Rect> {
        let (x, y, width, height) = *self.inner.bounds.lock().unwrap();
        Ok(Rect {
            position: Position::Physical(PhysicalPosition::new(x, y)),
            size: Size::Physical(PhysicalSize::new(width, height)),
        })
    }

    fn position(&self) -> Result<PhysicalPosition<i32>> {
        let (x, y, _, _) = *self.inner.bounds.lock().unwrap();
        Ok(PhysicalPosition::new(x, y))
    }

    fn size(&self) -> Result<PhysicalSize<u32>> {
        let (_, _, width, height) = *self.inner.bounds.lock().unwrap();
        Ok(PhysicalSize::new(width, height))
    }

    fn navigate(&self, url: Url) -> Result<()> {
        self.inner.webview.load_url(url.as_str());
        Ok(())
    }

    fn reload(&self) -> Result<()> {
        self.inner.webview.reload();
        Ok(())
    }

    fn go_back(&self) -> Result<()> {
        self.inner.webview.go_back();
        Ok(())
    }

    fn can_go_back(&self) -> Result<bool> {
        Ok(self.inner.webview.can_go_back())
    }

    fn go_forward(&self) -> Result<()> {
        self.inner.webview.go_forward();
        Ok(())
    }

    fn can_go_forward(&self) -> Result<bool> {
        Ok(self.inner.webview.can_go_forward())
    }

    fn print(&self) -> Result<()> {
        self.inner.webview.print();
        Ok(())
    }

    fn close(&self) -> Result<()> {
        self.inner.destroy();
        Ok(())
    }

    fn set_bounds(&self, bounds: Rect) -> Result<()> {
        let scale = self.inner.window.scale_factor();
        let (x, y) = physical_position(bounds.position, scale);
        let (width, height) = physical_size(bounds.size, scale);
        self.inner.set_bounds(x, y, width as i32, height as i32);
        Ok(())
    }

    fn set_size(&self, size: Size) -> Result<()> {
        let scale = self.inner.window.scale_factor();
        let (x, y, _, _) = *self.inner.bounds.lock().unwrap();
        let (width, height) = physical_size(size, scale);
        self.inner.set_bounds(x, y, width as i32, height as i32);
        Ok(())
    }

    fn set_position(&self, position: Position) -> Result<()> {
        let scale = self.inner.window.scale_factor();
        let (x, y) = physical_position(position, scale);
        let (_, _, width, height) = *self.inner.bounds.lock().unwrap();
        self.inner.set_bounds(x, y, width as i32, height as i32);
        Ok(())
    }

    fn set_focus(&self) -> Result<()> {
        self.inner.focus();
        Ok(())
    }

    fn hide(&self) -> Result<()> {
        self.inner.webview.set_visible(false);
        Ok(())
    }

    fn show(&self) -> Result<()> {
        self.inner.webview.set_visible(true);
        Ok(())
    }

    fn eval_script<S: Into<String>>(&self, script: S) -> Result<()> {
        self.inner.webview.eval(&script.into());
        Ok(())
    }

    fn eval_script_with_callback<S: Into<String>>(
        &self,
        script: S,
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<()> {
        let script = script.into();
        let wrapped = format!(
            "(function () {{ try {{ return JSON.stringify({script}); }} catch (e) {{ return JSON.stringify(null); }} }})()"
        );
        self.inner.webview.eval_with_callback(&wrapped, callback);
        Ok(())
    }

    fn reparent(&self, _window_id: WindowId) -> Result<()> {
        Ok(())
    }

    fn cookies_for_url(&self, _url: Url) -> Result<Vec<Cookie<'static>>> {
        self.read_cookies()
    }

    fn cookies(&self) -> Result<Vec<Cookie<'static>>> {
        self.read_cookies()
    }

    fn set_cookie(&self, cookie: Cookie<'_>) -> Result<()> {
        let url = self.inner.webview.url();
        let mut value = format!("{}={}", cookie.name(), cookie.value());
        if let Some(domain) = cookie.domain() {
            value.push_str(&format!("; domain={domain}"));
        }
        if let Some(path) = cookie.path() {
            value.push_str(&format!("; path={path}"));
        }
        if cookie.secure().unwrap_or(false) {
            value.push_str("; secure");
        }
        if cookie.http_only().unwrap_or(false) {
            value.push_str("; httponly");
        }
        self.inner.webview.set_cookie(&url, &value);
        Ok(())
    }

    fn delete_cookie(&self, cookie: Cookie<'_>) -> Result<()> {
        let url = self.inner.webview.url();
        let value = format!(
            "{}=; domain={}; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT",
            cookie.name(),
            cookie.domain().unwrap_or_default()
        );
        self.inner.webview.set_cookie(&url, &value);
        Ok(())
    }

    fn set_auto_resize(&self, auto_resize: bool) -> Result<()> {
        self.inner.auto_resize.store(auto_resize, Ordering::SeqCst);
        Ok(())
    }

    fn set_zoom(&self, scale_factor: f64) -> Result<()> {
        self.inner.webview.set_zoom(scale_factor as f32);
        Ok(())
    }

    fn set_background_color(&self, _color: Option<Color>) -> Result<()> {
        Ok(())
    }

    fn clear_all_browsing_data(&self) -> Result<()> {
        self.inner.webview.clear_cookies();
        Ok(())
    }
}

impl<T: UserEvent> MbWebviewDispatcher<T> {
    fn read_cookies(&self) -> Result<Vec<Cookie<'static>>> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.webview.cookies_with(move |cookies| {
            let _ = tx.send(cookies);
        });

        let raw = rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| Error::FailedToReceiveMessage)?;

        let mut result = Vec::new();
        for part in raw.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match Cookie::parse(part.to_string()) {
                Ok(cookie) => result.push(cookie.into_owned()),
                Err(_) => continue,
            }
        }
        Ok(result)
    }
}

/// Unused helper kept so the shared state type is referenced from this module.
#[allow(dead_code)]
fn shared_of<T: UserEvent>(inner: &WebviewInner<T>) -> &Arc<Shared<T>> {
    &inner.window.shared
}
