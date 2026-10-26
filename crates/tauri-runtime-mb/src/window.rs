//! Native window creation and the [`WindowDispatch`] implementation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

use tauri_runtime::dpi::{
    LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position, Rect, Size,
};
use tauri_runtime::window::{
    DetachedWindow, DetachedWindowWebview, PendingWindow, RawWindow, WindowBuilder,
    WindowBuilderBase, WindowEvent, WindowSizeConstraints, WindowId,
};
use tauri_runtime::{
    Error, Icon, ProgressBarState, ResizeDirection, Result, UserAttentionType, UserEvent,
    WindowDispatch, WindowEventId,
};
use tauri_utils::config::{Color, WindowConfig};
use tauri_utils::{Theme, TitleBarStyle};

use crate::shared::{EventHandler, Shared};
use crate::sys;
use crate::webview::{WebviewInner, create_webview_in_window};
use crate::window_frame;
use crate::MbRuntime;

const WINDOW_CLASS: &str = "TauriMiniblinkWindow";
const MESSAGE_CLASS: &str = "TauriMiniblinkMessageWindow";
const HWND_MESSAGE: isize = -3;
const HWND_BOTTOM: isize = 1;
const HWND_TOPMOST: isize = -1;
const HWND_NOTOPMOST: isize = -2;
const WM_NCLBUTTONDOWN: u32 = 0x00A1;
const HTCAPTION: isize = 2;
const MONITOR_DEFAULTTOPRIMARY: u32 = 1;

static REGISTER_CLASSES: std::sync::Once = std::sync::Once::new();

/// Context stored in `GWLP_USERDATA` and used by the window procedure.
pub(crate) struct WindowProcCtx {
    on_message: Box<dyn Fn(isize, u32, usize, isize) -> Option<isize> + Send + Sync>,
}

unsafe extern "system" fn window_proc(
    hwnd: sys::HWND,
    msg: sys::UINT,
    wparam: sys::WPARAM,
    lparam: sys::LPARAM,
) -> sys::LRESULT {
    // SAFETY: USERDATA only ever holds a `Box<WindowProcCtx>` created below.
    let ctx_ptr = unsafe { sys::GetWindowLongPtrW(hwnd, sys::GWLP_USERDATA) } as *mut WindowProcCtx;

    if !ctx_ptr.is_null() {
        let ctx = unsafe { &*ctx_ptr };
        let result = (ctx.on_message)(hwnd, msg, wparam, lparam);

        if msg == sys::WM_NCDESTROY {
            unsafe {
                sys::SetWindowLongPtrW(hwnd, sys::GWLP_USERDATA, 0);
                drop(Box::from_raw(ctx_ptr));
            }
        }

        if let Some(result) = result {
            return result;
        }
    }

    unsafe { sys::DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn register_classes() {
    REGISTER_CLASSES.call_once(|| unsafe {
        let instance = sys::GetModuleHandleW(std::ptr::null());
        let cursor = sys::LoadCursorW(0, sys::IDC_ARROW);
        let class_name = sys::wide(WINDOW_CLASS);
        let class = sys::WNDCLASSW {
            style: sys::CS_HREDRAW | sys::CS_VREDRAW | sys::CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: cursor,
            hbrBackground: 0,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        sys::RegisterClassW(&class);

        let message_class_name = sys::wide(MESSAGE_CLASS);
        let message_class = sys::WNDCLASSW {
            lpfnWndProc: Some(message_window_proc),
            hInstance: instance,
            lpszClassName: message_class_name.as_ptr(),
            ..Default::default()
        };
        sys::RegisterClassW(&message_class);
    });
}

unsafe extern "system" fn message_window_proc(
    hwnd: sys::HWND,
    msg: sys::UINT,
    wparam: sys::WPARAM,
    lparam: sys::LPARAM,
) -> sys::LRESULT {
    unsafe { sys::DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Creates the hidden message-only window used to wake the event loop.
pub(crate) fn create_message_window() -> isize {
    register_classes();
    let class_name = sys::wide(MESSAGE_CLASS);
    // The message window does not need a user data context.
    unsafe {
        sys::CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            0,
            sys::GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        )
    }
}

// ---------------------------------------------------------------------------
// builder
// ---------------------------------------------------------------------------

/// Window builder for the Miniblink runtime.
#[derive(Clone)]
pub struct MbWindowBuilder {
    title: String,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
    min_size: Option<(f64, f64)>,
    max_size: Option<(f64, f64)>,
    resizable: bool,
    maximizable: bool,
    minimizable: bool,
    closable: bool,
    decorations: bool,
    visible: bool,
    transparent: bool,
    focused: bool,
    maximized: bool,
    fullscreen: bool,
    always_on_top: bool,
    always_on_bottom: bool,
    skip_taskbar: bool,
    center: bool,
    shadow: bool,
    content_protected: bool,
    theme: Option<Theme>,
    background_color: Option<Color>,
    owner: Option<isize>,
    parent: Option<isize>,
    drag_and_drop: bool,
}

impl std::fmt::Debug for MbWindowBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbWindowBuilder")
            .field("title", &self.title)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("x", &self.x)
            .field("y", &self.y)
            .field("resizable", &self.resizable)
            .field("decorations", &self.decorations)
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

impl WindowBuilderBase for MbWindowBuilder {}

impl WindowBuilder for MbWindowBuilder {
    fn new() -> Self {
        Self {
            title: "Tauri App".into(),
            width: 800.0,
            height: 600.0,
            x: None,
            y: None,
            min_size: None,
            max_size: None,
            resizable: true,
            maximizable: true,
            minimizable: true,
            closable: true,
            decorations: true,
            visible: true,
            transparent: false,
            focused: true,
            maximized: false,
            fullscreen: false,
            always_on_top: false,
            always_on_bottom: false,
            skip_taskbar: false,
            center: false,
            shadow: true,
            content_protected: false,
            theme: None,
            background_color: None,
            owner: None,
            parent: None,
            drag_and_drop: true,
        }
    }

    fn with_config(config: &WindowConfig) -> Self {
        let mut builder = Self::new();
        builder.title = config.title.clone();
        builder.width = config.width;
        builder.height = config.height;
        builder.x = config.x;
        builder.y = config.y;
        builder.resizable = config.resizable;
        builder.maximizable = config.maximizable;
        builder.minimizable = config.minimizable;
        builder.closable = config.closable;
        builder.decorations = config.decorations;
        builder.visible = config.visible;
        builder.transparent = config.transparent;
        builder.focused = config.focus;
        builder.maximized = config.maximized;
        builder.fullscreen = config.fullscreen;
        builder.always_on_top = config.always_on_top;
        builder.always_on_bottom = config.always_on_bottom;
        builder.skip_taskbar = config.skip_taskbar;
        builder.content_protected = config.content_protected;
        builder.shadow = config.shadow;
        builder.theme = config.theme.clone();
        builder.background_color = config.background_color.clone();
        builder.center = config.center;
        builder.min_size = match (config.min_width, config.min_height) {
            (Some(width), Some(height)) => Some((width, height)),
            _ => None,
        };
        builder.max_size = match (config.max_width, config.max_height) {
            (Some(width), Some(height)) => Some((width, height)),
            _ => None,
        };
        builder
    }

    fn center(mut self) -> Self {
        self.center = true;
        self
    }

    fn position(mut self, x: f64, y: f64) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self
    }

    fn inner_size(mut self, width: f64, height: f64) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    fn min_inner_size(mut self, min_width: f64, min_height: f64) -> Self {
        self.min_size = Some((min_width, min_height));
        self
    }

    fn max_inner_size(mut self, max_width: f64, max_height: f64) -> Self {
        self.max_size = Some((max_width, max_height));
        self
    }

    fn inner_size_constraints(self, constraints: WindowSizeConstraints) -> Self {
        // `PixelUnit` is not inspectable without pulling extra tauri-utils
        // features; keep the previously configured constraints.
        let _ = constraints;
        self
    }

    fn prevent_overflow(self) -> Self {
        self
    }

    fn prevent_overflow_with_margin(self, _margin: tauri_runtime::dpi::Size) -> Self {
        self
    }

    fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    fn maximizable(mut self, maximizable: bool) -> Self {
        self.maximizable = maximizable;
        self
    }

    fn minimizable(mut self, minimizable: bool) -> Self {
        self.minimizable = minimizable;
        self
    }

    fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    fn title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = title.into();
        self
    }

    fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    fn focusable(self, _focusable: bool) -> Self {
        self
    }

    fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    fn always_on_bottom(mut self, always_on_bottom: bool) -> Self {
        self.always_on_bottom = always_on_bottom;
        self
    }

    fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    fn visible_on_all_workspaces(self, _visible_on_all_workspaces: bool) -> Self {
        self
    }

    fn content_protected(mut self, protected: bool) -> Self {
        self.content_protected = protected;
        self
    }

    fn icon(self, _icon: Icon) -> Result<Self> {
        Ok(self)
    }

    fn skip_taskbar(mut self, skip: bool) -> Self {
        self.skip_taskbar = skip;
        self
    }

    fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    fn shadow(mut self, enable: bool) -> Self {
        self.shadow = enable;
        self
    }

    fn owner(mut self, owner: windows::Win32::Foundation::HWND) -> Self {
        self.owner = Some(sys::from_win32_hwnd(owner));
        self
    }

    fn parent(mut self, parent: windows::Win32::Foundation::HWND) -> Self {
        self.parent = Some(sys::from_win32_hwnd(parent));
        self
    }

    fn drag_and_drop(mut self, enabled: bool) -> Self {
        self.drag_and_drop = enabled;
        self
    }

    fn theme(mut self, theme: Option<Theme>) -> Self {
        self.theme = theme;
        self
    }

    fn has_icon(&self) -> bool {
        false
    }

    fn get_theme(&self) -> Option<Theme> {
        self.theme.clone()
    }

    fn window_classname<S: Into<String>>(self, _window_classname: S) -> Self {
        self
    }

    fn no_redirection_bitmap(self, _enable: bool) -> Self {
        self
    }
}

// ---------------------------------------------------------------------------
// window state
// ---------------------------------------------------------------------------

pub(crate) struct WindowInner<T: UserEvent> {
    pub(crate) id: WindowId,
    pub(crate) label: String,
    pub(crate) hwnd: AtomicIsize,
    pub(crate) shared: Arc<Shared<T>>,
    pub(crate) title: Mutex<String>,
    pub(crate) visible: AtomicBool,
    pub(crate) focused: AtomicBool,
    pub(crate) resizable: AtomicBool,
    pub(crate) maximizable: AtomicBool,
    pub(crate) minimizable: AtomicBool,
    pub(crate) closable: AtomicBool,
    pub(crate) decorations: AtomicBool,
    pub(crate) shadow: AtomicBool,
    pub(crate) always_on_top: AtomicBool,
    pub(crate) enabled: AtomicBool,
    pub(crate) skip_taskbar: AtomicBool,
    pub(crate) fullscreen: AtomicBool,
    pub(crate) scale_factor: Mutex<f64>,
    pub(crate) theme: Mutex<Option<Theme>>,
    pub(crate) min_size: Mutex<Option<(i32, i32)>>,
    pub(crate) max_size: Mutex<Option<(i32, i32)>>,
    /// Kept for the future `set_background_color` implementation.
    #[allow(dead_code)]
    pub(crate) background_color: Option<Color>,
    /// Kept for the future transparent window implementation.
    #[allow(dead_code)]
    pub(crate) transparent: bool,
    pub(crate) destroyed: AtomicBool,
    /// Set between `WM_IME_STARTCOMPOSITION` and `WM_IME_ENDCOMPOSITION`.
    ///
    /// While an IME composition is active the raw key events belong to the
    /// IME: replaying them into the engine would type the pinyin into the page
    /// instead of letting the IME turn it into hanzi.
    pub(crate) ime_composing: AtomicBool,
    /// Actual text caret reported in CSS pixels; anchors the native candidates.
    pub(crate) ime_caret: Mutex<Option<crate::ime::Caret>>,

    pub(crate) webview: Mutex<Option<Arc<WebviewInner<T>>>>,
    pub(crate) handlers: Mutex<HashMap<WindowEventId, EventHandler>>,
}

impl<T: UserEvent> WindowInner<T> {
    pub(crate) fn hwnd(&self) -> isize {
        self.hwnd.load(Ordering::SeqCst)
    }

    pub(crate) fn scale_factor(&self) -> f64 {
        *self.scale_factor.lock().unwrap()
    }



    pub(crate) fn set_webview(&self, webview: Arc<WebviewInner<T>>) {
        *self.webview.lock().unwrap() = Some(webview);
    }

    pub(crate) fn webview(&self) -> Option<Arc<WebviewInner<T>>> {
        self.webview.lock().unwrap().clone()
    }

    /// Detaches the webview from the window.
    ///
    /// Used when the window is destroyed so later window messages can no longer
    /// reach the (dead) engine handle.
    pub(crate) fn take_webview(&self) -> Option<Arc<WebviewInner<T>>> {
        self.webview.lock().unwrap().take()
    }

    /// Delivers a window event to the registered listeners and to the run loop.
    pub(crate) fn emit(&self, event: WindowEvent) {
        let handlers: Vec<EventHandler> = {
            let handlers = self.handlers.lock().unwrap();
            handlers.values().cloned().collect()
        };

        for handler in handlers {
            let callback = handler.lock().unwrap();
            callback(&event);
        }

        self.shared.push_event(tauri_runtime::RunEvent::WindowEvent {
            label: self.label.clone(),
            event,
        });
    }
}

fn window_scale(hwnd: isize) -> f64 {
    if hwnd != 0 {
        sys::window_scale_factor(hwnd)
    } else {
        sys::system_scale_factor()
    }
}

/// Creates the Win32 window for a pending Tauri window.
pub(crate) fn create_window<T: UserEvent, F: Fn(RawWindow<'_>) + Send + 'static>(
    shared: &Arc<Shared<T>>,
    pending: PendingWindow<T, MbRuntime<T>>,
    after_window_creation: Option<F>,
) -> Result<DetachedWindow<T, MbRuntime<T>>> {
    register_classes();

    let PendingWindow {
        label,
        window_builder: builder,
        webview: pending_webview,
    } = pending;

    // `GetDpiForSystem` reports the dpi of the primary display at process
    // start, which is wrong on mixed dpi setups: use the display the window is
    // going to live on.
    let target_monitor = match (builder.x, builder.y) {
        (Some(x), Some(y)) => crate::shared::monitor_from_point(x, y),
        _ => None,
    }
    .or_else(crate::shared::primary_monitor);

    let scale = target_monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor)
        .unwrap_or_else(sys::system_scale_factor);

    let width = (builder.width * scale).round().max(1.0) as i32;
    let height = (builder.height * scale).round().max(1.0) as i32;

    let style = window_frame::window_style(
        builder.resizable,
        builder.maximizable,
        builder.minimizable,
    );

    let mut ex_style = sys::WS_EX_WINDOWEDGE;
    if builder.always_on_top {
        ex_style |= sys::WS_EX_TOPMOST;
    }
    if builder.skip_taskbar {
        ex_style |= sys::WS_EX_TOOLWINDOW;
    } else {
        ex_style |= sys::WS_EX_APPWINDOW;
    }

    let mut rect = sys::RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    window_frame::adjust_rect(
        &mut rect,
        style,
        ex_style,
        builder.decorations,
        builder.shadow,
        (scale * 96.0).round() as u32,
    );

    let (mut x, mut y) = match (builder.x, builder.y) {
        (Some(x), Some(y)) => ((x * scale).round() as i32, (y * scale).round() as i32),
        _ => (sys::CW_USEDEFAULT, sys::CW_USEDEFAULT),
    };

    if builder.center || (builder.x.is_none() && builder.y.is_none()) {
        if let Some(monitor) = target_monitor.as_ref() {
            let area = monitor.work_area;
            let area_w = area.size.width as i32;
            let area_h = area.size.height as i32;
            let area_x = area.position.x;
            let area_y = area.position.y;
            x = area_x + (area_w - rect.width()) / 2;
            y = area_y + (area_h - rect.height()) / 2;
        }
    }

    let class_name = sys::wide(WINDOW_CLASS);
    let title = sys::wide(&builder.title);
    let parent = builder.parent.or(builder.owner).unwrap_or(0);

    let hwnd = unsafe {
        sys::CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            rect.width(),
            rect.height(),
            parent,
            0,
            sys::GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        )
    };

    if hwnd == 0 {
        return Err(Error::CreateWindow(Box::new(std::io::Error::last_os_error())));
    }

    // `GetDpiForSystem` is only an estimate: the window may land on a monitor
    // with a different scale factor, so the requested logical size is applied
    // again once the real DPI of the window is known.
    let actual_scale = window_scale(hwnd);
    if (actual_scale - scale).abs() > 0.001 {
        let mut content = sys::RECT {
            left: 0,
            top: 0,
            right: (builder.width * actual_scale).round().max(1.0) as i32,
            bottom: (builder.height * actual_scale).round().max(1.0) as i32,
        };
        window_frame::adjust_rect(
            &mut content,
            style,
            ex_style,
            builder.decorations,
            builder.shadow,
            (actual_scale * 96.0).round() as u32,
        );
        unsafe {
            sys::SetWindowPos(
                hwnd,
                0,
                0,
                0,
                content.width(),
                content.height(),
                sys::SWP_NOMOVE | sys::SWP_NOZORDER | sys::SWP_NOACTIVATE,
            )
        };
    }

    if std::env::var_os("MB_TRACE").is_some() {
        eprintln!(
            "[mb] create_window label={label} hwnd={hwnd} style={style:#x} scale={actual_scale}"
        );
    }

    let id = WindowId::from(shared.next_window_id.fetch_add(1, Ordering::SeqCst));

    let inner = Arc::new(WindowInner::<T> {
        id,
        label: label.clone(),
        hwnd: AtomicIsize::new(hwnd),
        shared: Arc::clone(shared),
        title: Mutex::new(builder.title.clone()),
        visible: AtomicBool::new(builder.visible),
        focused: AtomicBool::new(builder.focused),
        resizable: AtomicBool::new(builder.resizable),
        maximizable: AtomicBool::new(builder.maximizable),
        minimizable: AtomicBool::new(builder.minimizable),
        closable: AtomicBool::new(builder.closable),
        decorations: AtomicBool::new(builder.decorations),
        shadow: AtomicBool::new(builder.shadow),
        always_on_top: AtomicBool::new(builder.always_on_top),
        enabled: AtomicBool::new(true),
        skip_taskbar: AtomicBool::new(builder.skip_taskbar),
        fullscreen: AtomicBool::new(builder.fullscreen),
        scale_factor: Mutex::new(actual_scale),
        theme: Mutex::new(builder.theme.clone()),
        min_size: Mutex::new(builder.min_size.map(|(w, h)| {
            ((w * scale) as i32, (h * scale) as i32)
        })),
        max_size: Mutex::new(builder.max_size.map(|(w, h)| {
            ((w * scale) as i32, (h * scale) as i32)
        })),
        background_color: builder.background_color.clone(),
        transparent: builder.transparent,
        destroyed: AtomicBool::new(false),
        ime_composing: AtomicBool::new(false),
        ime_caret: Mutex::new(None),
        webview: Mutex::new(None),
        handlers: Mutex::new(HashMap::new()),
    });

    {
        let context = Arc::clone(&inner);
        let on_message = Box::new(
            move |hwnd: isize, msg: u32, wparam: usize, lparam: isize| {
                handle_message(&context, hwnd, msg, wparam, lparam)
            },
        );
        let ctx = Box::into_raw(Box::new(WindowProcCtx { on_message }));
        unsafe { sys::SetWindowLongPtrW(hwnd, sys::GWLP_USERDATA, ctx as isize) };
    }

    // Creation initially uses DefWindowProc; recalculate before showing so the
    // installed nonclient handler removes the title bar without a visible flash.
    window_frame::apply(&inner);

    shared
        .windows
        .lock()
        .unwrap()
        .insert(id, Arc::clone(&inner));

    if builder.visible {
        let command = if builder.maximized {
            sys::SW_SHOWMAXIMIZED
        } else {
            sys::SW_SHOW
        };
        unsafe { sys::ShowWindow(hwnd, command) };
        if builder.focused {
            // Keyboard input only reaches the engine once the window owns the
            // focus, which `ShowWindow` alone does not guarantee.
            unsafe {
                sys::SetForegroundWindow(hwnd);
                sys::SetActiveWindow(hwnd);
                sys::SetFocus(hwnd);
            }
        }
    }

    let mut detached_webview = None;
    if let Some(pending_webview) = pending_webview {
        let dispatcher = create_webview_in_window(&inner, pending_webview)?;
        let use_https_scheme = dispatcher.inner.use_https_scheme;
        let devtools = dispatcher.inner.devtools;
        inner.set_webview(Arc::clone(&dispatcher.inner));
        detached_webview = Some(DetachedWindowWebview {
            webview: tauri_runtime::webview::DetachedWebview {
                label: dispatcher.inner.label.clone(),
                dispatcher,
            },
            use_https_scheme,
            devtools,
        });
    }

    if let Some(callback) = after_window_creation {
        let marker = std::marker::PhantomData;
        callback(RawWindow {
            hwnd,
            _marker: &marker,
        });
    }

    Ok(DetachedWindow {
        id,
        label,
        dispatcher: MbWindowDispatcher {
            inner: Arc::clone(&inner),
        },
        webview: detached_webview,
    })
}

fn handle_message<T: UserEvent>(
    inner: &Arc<WindowInner<T>>,
    hwnd: isize,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> Option<isize> {
    if let Some(result) = window_frame::handle_message(inner, hwnd, msg, wparam, lparam) {
        return Some(result);
    }
    if std::env::var_os("MB_TRACE").is_some()
        && matches!(
            msg,
            sys::WM_SETFOCUS
                | sys::WM_KILLFOCUS
                | sys::WM_ACTIVATE
                | sys::WM_KEYDOWN
                | sys::WM_CHAR
                | sys::WM_LBUTTONDOWN
                | sys::WM_LBUTTONUP
        )
    {
        eprintln!("[mb] wnd msg={msg} wparam={wparam:#x} lparam={lparam:#x}");
    }

    match msg {
        sys::WM_SIZE => {
            let (_, _, width, height) = sys::client_rect(hwnd);
            if let Some(webview) = inner.webview()
                && webview.auto_resize()
            {
                webview.set_bounds(0, 0, width, height);
            }

            inner.emit(WindowEvent::Resized(PhysicalSize::new(
                width.max(0) as u32,
                height.max(0) as u32,
            )));
            None
        }
        sys::WM_MOVE => {
            let mut rect = sys::RECT::default();
            unsafe { sys::GetWindowRect(hwnd, &mut rect) };
            inner.emit(WindowEvent::Moved(PhysicalPosition::new(rect.left, rect.top)));
            None
        }
        sys::WM_SETFOCUS => {
            inner.focused.store(true, Ordering::SeqCst);
            if let Some(webview) = inner.webview() {
                webview.focus();
            }
            inner.emit(WindowEvent::Focused(true));
            None
        }
        sys::WM_KILLFOCUS => {
            inner.focused.store(false, Ordering::SeqCst);
            inner.ime_composing.store(false, Ordering::SeqCst);
            crate::ime::end(inner);
            if let Some(webview) = inner.webview() {
                webview.blur();
            }
            inner.emit(WindowEvent::Focused(false));
            None
        }
        // --- IME -----------------------------------------------------------
        sys::WM_IME_SETCONTEXT => {
            // Render preedit with the page's CSS, while retaining the system
            // candidate UI. DefWindowProc must not create a second text window.
            Some(unsafe { sys::DefWindowProcW(
                hwnd, msg, wparam, lparam & !sys::ISC_SHOWUICOMPOSITIONWINDOW,
            ) })
        }
        sys::WM_IME_STARTCOMPOSITION => {
            inner.ime_composing.store(true, Ordering::SeqCst);
            crate::ime::start(inner, hwnd);
            Some(0)
        }
        sys::WM_IME_ENDCOMPOSITION => {
            inner.ime_composing.store(false, Ordering::SeqCst);
            crate::ime::end(inner);
            Some(0)
        }
        sys::WM_IME_COMPOSITION => {
            crate::ime::composition(inner, hwnd, lparam as u32);
            // We consume RESULTSTR ourselves; the default procedure would
            // convert it to WM_IME_CHAR/WM_CHAR and insert it a second time.
            Some(0)
        }
        sys::WM_IME_CHAR => {
            Some(0)
        }
        sys::WM_CLOSE => {
            if std::env::var_os("MB_TRACE").is_some() {
                eprintln!("[mb] WM_CLOSE for {}", inner.label);
            }
            let (tx, rx) = std::sync::mpsc::channel();
            inner.emit(WindowEvent::CloseRequested { signal_tx: tx });
            let prevent = matches!(rx.try_recv(), Ok(true));
            if !prevent {
                unsafe { sys::DestroyWindow(hwnd) };
            }
            Some(0)
        }
        sys::WM_DESTROY => {
            if std::env::var_os("MB_TRACE").is_some() {
                eprintln!("[mb] WM_DESTROY for {}", inner.label);
            }
            inner.destroyed.store(true, Ordering::SeqCst);
            inner.shared.windows.lock().unwrap().remove(&inner.id);
            inner.emit(WindowEvent::Destroyed);
            // Detach the view before destroying it: everything that arrives
            // after `WM_DESTROY` (notably `WM_NCDESTROY`) goes through
            // `forward()`, which must not touch a dead engine handle.
            if let Some(webview) = inner.take_webview() {
                webview.destroy();
            }
            inner.shared.wake();
            None
        }
        sys::WM_PAINT => {
            let mut paint = sys::PAINTSTRUCT::default();
            let hdc = unsafe { sys::BeginPaint(hwnd, &mut paint) };
            if hdc != 0 {
                if let Some(webview) = inner.webview() {
                    webview.blit(hdc);
                }
                unsafe { sys::EndPaint(hwnd, &paint) };
            }
            Some(0)
        }
        sys::WM_ERASEBKGND => Some(1),
        sys::WM_GETMINMAXINFO => {
            let info = lparam as *mut sys::MINMAXINFO;
            if !info.is_null() {
                if let Some((width, height)) = *inner.min_size.lock().unwrap() {
                    unsafe { (*info).ptMinTrackSize = sys::POINT { x: width, y: height } };
                }
                if let Some((width, height)) = *inner.max_size.lock().unwrap() {
                    unsafe { (*info).ptMaxTrackSize = sys::POINT { x: width, y: height } };
                }
            }
            Some(0)
        }
        sys::WM_DPICHANGED => {
            let scale = sys::window_scale_factor(hwnd);
            *inner.scale_factor.lock().unwrap() = scale;

            // follow the rectangle suggested by the system
            let suggested = lparam as *const sys::RECT;
            if !suggested.is_null() {
                let rect = unsafe { *suggested };
                unsafe {
                    sys::SetWindowPos(
                        hwnd,
                        0,
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        sys::SWP_NOZORDER | sys::SWP_NOACTIVATE,
                    )
                };
            }

            if let Some(webview) = inner.webview() {
                // the scale factor stays pinned to 1, only the size changes
                let (_, _, width, height) = sys::client_rect(hwnd);
                webview.set_bounds(0, 0, width, height);
            }

            let (_, _, width, height) = sys::client_rect(hwnd);
            inner.emit(WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
                new_inner_size: PhysicalSize::new(width.max(0) as u32, height.max(0) as u32),
            });
            None
        }
        sys::WM_SETCURSOR => {
            if let Some(result) = forward(inner, hwnd, msg, wparam, lparam) {
                return Some(result);
            }
            unsafe { sys::SetCursor(sys::LoadCursorW(0, sys::IDC_ARROW)) };
            Some(1)
        }
        _ => forward(inner, hwnd, msg, wparam, lparam),
    }
}

/// Converts a `MAKELPARAM` style pair of physical pixels into css pixels.
fn to_css_coords(lparam: isize, scale: f64) -> isize {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let x = ((lparam & 0xFFFF) as u16 as i16 as f64 / scale).round() as i32;
    let y = (((lparam >> 16) & 0xFFFF) as u16 as i16 as f64 / scale).round() as i32;
    ((y as isize) << 16) | (x as isize & 0xFFFF)
}

fn forward<T: UserEvent>(
    inner: &Arc<WindowInner<T>>,
    hwnd: isize,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> Option<isize> {
    let webview = inner.webview()?;

    // In window mode the engine owns a child window and receives input
    // directly; injecting here would double every event.
    if webview.engine() != 0 {
        return None;
    }

    match msg {
        // `mbFireWindowsMessage` does not route input in offscreen mode, the
        // dedicated input APIs have to be used instead.
        sys::WM_KEYDOWN | sys::WM_SYSKEYDOWN | sys::WM_KEYUP | sys::WM_SYSKEYUP | sys::WM_CHAR
        | sys::WM_SYSCHAR => {
            // An active IME composition owns the keyboard: replaying the keys
            // would type the pinyin into the page instead of letting the IME
            // convert it (the committed text arrives through WM_IME_COMPOSITION).
            if inner.ime_composing.load(Ordering::SeqCst) {
                return None;
            }
            let handled = webview.webview.dispatch_keyboard(msg, wparam, lparam);
            if std::env::var_os("MB_TRACE").is_some() {
                eprintln!("[mb] key msg={msg} vk={wparam:#x} handled={handled}");
            }
            if handled { Some(0) } else { None }
        }
        sys::WM_MOUSEMOVE
        | sys::WM_LBUTTONDOWN
        | sys::WM_LBUTTONUP
        | sys::WM_LBUTTONDBLCLK
        | sys::WM_RBUTTONDOWN
        | sys::WM_RBUTTONUP
        | sys::WM_RBUTTONDBLCLK
        | sys::WM_MBUTTONDOWN
        | sys::WM_MBUTTONUP
        | sys::WM_MBUTTONDBLCLK
        | sys::WM_MOUSELEAVE => {
            // viewport == physical client area, coordinates are passed through
            webview.webview.dispatch_mouse(msg, wparam, lparam);
            // Let the default procedure run too: it maintains capture,
            // activation and the system cursor state.
            None
        }
        sys::WM_MOUSEWHEEL => {
            // the wheel message carries screen coordinates
            let mut point = sys::POINT {
                x: (lparam & 0xFFFF) as u16 as i16 as i32,
                y: ((lparam >> 16) & 0xFFFF) as u16 as i16 as i32,
            };
            unsafe { sys::ScreenToClient(hwnd, &mut point) };
            let client = ((point.y as isize) << 16) | (point.x as isize & 0xFFFF);
            webview.webview.dispatch_mouse(msg, wparam, client);
            None
        }
        _ => webview.forward(hwnd, msg, wparam, lparam),
    }
}



// ---------------------------------------------------------------------------
// dispatcher
// ---------------------------------------------------------------------------

/// Handle to a window created by the Miniblink runtime.
pub struct MbWindowDispatcher<T: UserEvent> {
    pub(crate) inner: Arc<WindowInner<T>>,
}

impl<T: UserEvent> Clone for MbWindowDispatcher<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: UserEvent> std::fmt::Debug for MbWindowDispatcher<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbWindowDispatcher")
            .field("label", &self.inner.label)
            .field("hwnd", &self.inner.hwnd())
            .finish()
    }
}

impl<T: UserEvent> WindowDispatch<T> for MbWindowDispatcher<T> {
    type Runtime = MbRuntime<T>;
    type WindowBuilder = MbWindowBuilder;

    fn run_on_main_thread<F: FnOnce() + Send + 'static>(&self, f: F) -> Result<()> {
        self.inner.shared.post_task(Box::new(f));
        Ok(())
    }

    fn on_window_event<F: Fn(&WindowEvent) + Send + 'static>(&self, f: F) -> WindowEventId {
        let id = self.inner.shared.next_event_id();
        self.inner
            .handlers
            .lock()
            .unwrap()
            .insert(id, Arc::new(Mutex::new(Box::new(f))));
        id
    }

    fn scale_factor(&self) -> Result<f64> {
        Ok(self.inner.scale_factor())
    }

    fn inner_position(&self) -> Result<PhysicalPosition<i32>> {
        let hwnd = self.inner.hwnd();
        let mut point = sys::POINT::default();
        unsafe { sys::ClientToScreen(hwnd, &mut point) };
        Ok(PhysicalPosition::new(point.x, point.y))
    }

    fn outer_position(&self) -> Result<PhysicalPosition<i32>> {
        let (x, y, _, _) = sys::window_rect(self.inner.hwnd());
        Ok(PhysicalPosition::new(x, y))
    }

    fn inner_size(&self) -> Result<PhysicalSize<u32>> {
        let (_, _, width, height) = sys::client_rect(self.inner.hwnd());
        Ok(PhysicalSize::new(width.max(0) as u32, height.max(0) as u32))
    }

    fn outer_size(&self) -> Result<PhysicalSize<u32>> {
        let (_, _, width, height) = sys::window_rect(self.inner.hwnd());
        Ok(PhysicalSize::new(width.max(0) as u32, height.max(0) as u32))
    }

    fn is_fullscreen(&self) -> Result<bool> {
        Ok(self.inner.fullscreen.load(Ordering::SeqCst))
    }

    fn is_minimized(&self) -> Result<bool> {
        Ok(unsafe { sys::IsIconic(self.inner.hwnd()) } != 0)
    }

    fn is_maximized(&self) -> Result<bool> {
        Ok(unsafe { sys::IsZoomed(self.inner.hwnd()) } != 0)
    }

    fn is_focused(&self) -> Result<bool> {
        Ok(unsafe { sys::GetForegroundWindow() } == self.inner.hwnd())
    }

    fn is_decorated(&self) -> Result<bool> {
        Ok(self.inner.decorations.load(Ordering::SeqCst))
    }

    fn is_resizable(&self) -> Result<bool> {
        Ok(self.inner.resizable.load(Ordering::SeqCst))
    }

    fn is_maximizable(&self) -> Result<bool> {
        Ok(self.inner.maximizable.load(Ordering::SeqCst))
    }

    fn is_minimizable(&self) -> Result<bool> {
        Ok(self.inner.minimizable.load(Ordering::SeqCst))
    }

    fn is_closable(&self) -> Result<bool> {
        Ok(self.inner.closable.load(Ordering::SeqCst))
    }

    fn is_visible(&self) -> Result<bool> {
        Ok(unsafe { sys::IsWindowVisible(self.inner.hwnd()) } != 0)
    }

    fn is_enabled(&self) -> Result<bool> {
        Ok(unsafe { sys::IsWindowEnabled(self.inner.hwnd()) } != 0)
    }

    fn is_always_on_top(&self) -> Result<bool> {
        Ok(self.inner.always_on_top.load(Ordering::SeqCst))
    }

    fn title(&self) -> Result<String> {
        Ok(sys::window_title(self.inner.hwnd()))
    }

    fn current_monitor(&self) -> Result<Option<tauri_runtime::monitor::Monitor>> {
        let monitor = unsafe {
            sys::MonitorFromWindow(self.inner.hwnd(), sys::MONITOR_DEFAULTTONEAREST)
        };
        Ok(crate::shared::monitor_from_handle(monitor))
    }

    fn primary_monitor(&self) -> Result<Option<tauri_runtime::monitor::Monitor>> {
        Ok(crate::shared::primary_monitor())
    }

    fn monitor_from_point(&self, x: f64, y: f64) -> Result<Option<tauri_runtime::monitor::Monitor>> {
        Ok(crate::shared::monitor_from_point(x, y))
    }

    fn available_monitors(&self) -> Result<Vec<tauri_runtime::monitor::Monitor>> {
        Ok(crate::shared::available_monitors())
    }

    fn window_handle(
        &self,
    ) -> std::result::Result<
        raw_window_handle::WindowHandle<'_>,
        raw_window_handle::HandleError,
    > {
        let handle = std::num::NonZeroIsize::new(self.inner.hwnd())
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let raw = raw_window_handle::RawWindowHandle::Win32(
            raw_window_handle::Win32WindowHandle::new(handle),
        );
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }

    fn theme(&self) -> Result<Theme> {
        Ok(self
            .inner
            .theme
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Theme::Light))
    }

    fn center(&self) -> Result<()> {
        let hwnd = self.inner.hwnd();
        let (_, _, width, height) = sys::window_rect(hwnd);
        if let Some(monitor) = crate::shared::primary_monitor() {
            let area = monitor.work_area;
            let x = area.position.x + (area.size.width as i32 - width) / 2;
            let y = area.position.y + (area.size.height as i32 - height) / 2;
            unsafe {
                sys::SetWindowPos(
                    hwnd,
                    0,
                    x,
                    y,
                    0,
                    0,
                    sys::SWP_NOSIZE | sys::SWP_NOZORDER | sys::SWP_NOACTIVATE,
                )
            };
        }
        Ok(())
    }

    fn request_user_attention(&self, _request_type: Option<UserAttentionType>) -> Result<()> {
        Ok(())
    }

    fn create_window<F: Fn(RawWindow<'_>) + Send + 'static>(
        &mut self,
        pending: PendingWindow<T, Self::Runtime>,
        after_window_creation: Option<F>,
    ) -> Result<DetachedWindow<T, Self::Runtime>> {
        let shared = Arc::clone(&self.inner.shared);
        create_window(&shared, pending, after_window_creation)
    }

    fn create_webview(
        &mut self,
        pending: tauri_runtime::webview::PendingWebview<T, Self::Runtime>,
    ) -> Result<tauri_runtime::webview::DetachedWebview<T, Self::Runtime>> {
        let dispatcher = create_webview_in_window(&self.inner, pending)?;
        Ok(tauri_runtime::webview::DetachedWebview {
            label: dispatcher.inner.label.clone(),
            dispatcher,
        })
    }

    fn set_resizable(&self, resizable: bool) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        self.inner.shared.run_on_main_thread(move || {
            inner.resizable.store(resizable, Ordering::SeqCst);
            window_frame::apply(&inner);
        })
    }

    fn set_enabled(&self, enabled: bool) -> Result<()> {
        self.inner.enabled.store(enabled, Ordering::SeqCst);
        unsafe { sys::EnableWindow(self.inner.hwnd(), if enabled { 1 } else { 0 }) };
        Ok(())
    }

    fn set_maximizable(&self, maximizable: bool) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        self.inner.shared.run_on_main_thread(move || {
            inner.maximizable.store(maximizable, Ordering::SeqCst);
            window_frame::apply(&inner);
        })
    }

    fn set_minimizable(&self, minimizable: bool) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        self.inner.shared.run_on_main_thread(move || {
            inner.minimizable.store(minimizable, Ordering::SeqCst);
            window_frame::apply(&inner);
        })
    }

    fn set_closable(&self, closable: bool) -> Result<()> {
        self.inner.closable.store(closable, Ordering::SeqCst);
        Ok(())
    }

    fn set_title<S: Into<String>>(&self, title: S) -> Result<()> {
        let title = title.into();
        let wide = sys::wide(&title);
        unsafe { sys::SetWindowTextW(self.inner.hwnd(), wide.as_ptr()) };
        *self.inner.title.lock().unwrap() = title;
        Ok(())
    }

    fn maximize(&self) -> Result<()> {
        unsafe { sys::ShowWindow(self.inner.hwnd(), sys::SW_MAXIMIZE) };
        Ok(())
    }

    fn unmaximize(&self) -> Result<()> {
        unsafe { sys::ShowWindow(self.inner.hwnd(), sys::SW_RESTORE) };
        Ok(())
    }

    fn minimize(&self) -> Result<()> {
        unsafe { sys::ShowWindow(self.inner.hwnd(), sys::SW_MINIMIZE) };
        Ok(())
    }

    fn unminimize(&self) -> Result<()> {
        unsafe { sys::ShowWindow(self.inner.hwnd(), sys::SW_RESTORE) };
        Ok(())
    }

    fn show(&self) -> Result<()> {
        self.inner.visible.store(true, Ordering::SeqCst);
        unsafe { sys::ShowWindow(self.inner.hwnd(), sys::SW_SHOW) };
        Ok(())
    }

    fn hide(&self) -> Result<()> {
        self.inner.visible.store(false, Ordering::SeqCst);
        unsafe { sys::ShowWindow(self.inner.hwnd(), sys::SW_HIDE) };
        Ok(())
    }

    fn close(&self) -> Result<()> {
        unsafe { sys::PostMessageW(self.inner.hwnd(), sys::WM_CLOSE, 0, 0) };
        Ok(())
    }

    fn destroy(&self) -> Result<()> {
        unsafe { sys::DestroyWindow(self.inner.hwnd()) };
        Ok(())
    }

    fn set_decorations(&self, decorations: bool) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        self.inner.shared.run_on_main_thread(move || {
            inner.decorations.store(decorations, Ordering::SeqCst);
            window_frame::apply(&inner);
        })
    }

    fn set_shadow(&self, enable: bool) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        self.inner.shared.run_on_main_thread(move || {
            inner.shadow.store(enable, Ordering::SeqCst);
            window_frame::apply(&inner);
        })
    }

    fn set_always_on_bottom(&self, always_on_bottom: bool) -> Result<()> {
        unsafe {
            sys::SetWindowPos(
                self.inner.hwnd(),
                if always_on_bottom { HWND_BOTTOM } else { HWND_NOTOPMOST },
                0,
                0,
                0,
                0,
                sys::SWP_NOMOVE | sys::SWP_NOSIZE | sys::SWP_NOACTIVATE,
            )
        };
        Ok(())
    }

    fn set_always_on_top(&self, always_on_top: bool) -> Result<()> {
        self.inner.always_on_top.store(always_on_top, Ordering::SeqCst);
        unsafe {
            sys::SetWindowPos(
                self.inner.hwnd(),
                if always_on_top { HWND_TOPMOST } else { HWND_NOTOPMOST },
                0,
                0,
                0,
                0,
                sys::SWP_NOMOVE | sys::SWP_NOSIZE | sys::SWP_NOACTIVATE,
            )
        };
        Ok(())
    }

    fn set_visible_on_all_workspaces(&self, _visible_on_all_workspaces: bool) -> Result<()> {
        Ok(())
    }

    fn set_background_color(&self, _color: Option<Color>) -> Result<()> {
        Ok(())
    }

    fn set_content_protected(&self, _protected: bool) -> Result<()> {
        Ok(())
    }

    fn set_size(&self, size: Size) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        self.inner.shared.run_on_main_thread(move || {
            let hwnd = inner.hwnd();
            let (width, height) = physical_size(size, inner.scale_factor());
            let mut rect = sys::RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            if !inner.fullscreen.load(Ordering::SeqCst) {
                let style = unsafe { sys::GetWindowLongPtrW(hwnd, sys::GWL_STYLE) } as u32;
                let ex_style = unsafe { sys::GetWindowLongPtrW(hwnd, sys::GWL_EXSTYLE) } as u32;
                window_frame::adjust_rect(
                    &mut rect,
                    style,
                    ex_style,
                    inner.decorations.load(Ordering::SeqCst),
                    inner.shadow.load(Ordering::SeqCst),
                    unsafe { sys::GetDpiForWindow(hwnd) },
                );
            }
            unsafe {
                sys::SetWindowPos(
                    hwnd,
                    0,
                    0,
                    0,
                    rect.width(),
                    rect.height(),
                    sys::SWP_NOMOVE | sys::SWP_NOZORDER | sys::SWP_NOACTIVATE,
                );
            }
        })
    }

    fn set_min_size(&self, size: Option<Size>) -> Result<()> {
        let scale = self.inner.scale_factor();
        *self.inner.min_size.lock().unwrap() =
            size.map(|size| {
                let (width, height) = physical_size(size, scale);
                (width as i32, height as i32)
            });
        Ok(())
    }

    fn set_max_size(&self, size: Option<Size>) -> Result<()> {
        let scale = self.inner.scale_factor();
        *self.inner.max_size.lock().unwrap() =
            size.map(|size| {
                let (width, height) = physical_size(size, scale);
                (width as i32, height as i32)
            });
        Ok(())
    }

    fn set_size_constraints(&self, constraints: WindowSizeConstraints) -> Result<()> {
        let _ = constraints;
        Ok(())
    }

    fn set_position(&self, position: Position) -> Result<()> {
        let scale = self.inner.scale_factor();
        let (x, y) = physical_position(position, scale);
        unsafe {
            sys::SetWindowPos(
                self.inner.hwnd(),
                0,
                x,
                y,
                0,
                0,
                sys::SWP_NOSIZE | sys::SWP_NOZORDER | sys::SWP_NOACTIVATE,
            )
        };
        Ok(())
    }

    fn set_fullscreen(&self, fullscreen: bool) -> Result<()> {
        let hwnd = self.inner.hwnd();
        self.inner.fullscreen.store(fullscreen, Ordering::SeqCst);

        if fullscreen {
            let monitor = unsafe { sys::MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY) };
            let mut info = sys::MONITORINFOEXW {
                cbSize: std::mem::size_of::<sys::MONITORINFOEXW>() as u32,
                ..Default::default()
            };
            unsafe { sys::GetMonitorInfoW(monitor, &mut info) };
            unsafe {
                sys::SetWindowLongPtrW(hwnd, sys::GWL_STYLE, (sys::WS_POPUP | sys::WS_VISIBLE) as isize);
                sys::SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    info.rcMonitor.left,
                    info.rcMonitor.top,
                    info.rcMonitor.width(),
                    info.rcMonitor.height(),
                    sys::SWP_SHOWWINDOW,
                );
            }
        } else {
            unsafe {
                sys::SetWindowLongPtrW(
                    hwnd,
                    sys::GWL_STYLE,
                    window_frame::window_style(
                        self.inner.resizable.load(Ordering::SeqCst),
                        self.inner.maximizable.load(Ordering::SeqCst),
                        self.inner.minimizable.load(Ordering::SeqCst),
                    ) as isize,
                );
                let (_, _, width, height) = sys::window_rect(hwnd);
                sys::SetWindowPos(
                    hwnd,
                    HWND_NOTOPMOST,
                    100,
                    100,
                    width,
                    height,
                    sys::SWP_SHOWWINDOW | sys::SWP_FRAMECHANGED,
                );
            }
        }
        Ok(())
    }

    fn set_fullscreen_on_monitor(&self, _position: PhysicalPosition<f64>) -> Result<()> {
        self.set_fullscreen(true)
    }

    fn set_focus(&self) -> Result<()> {
        unsafe { sys::SetFocus(self.inner.hwnd()) };
        Ok(())
    }

    fn set_focusable(&self, _focusable: bool) -> Result<()> {
        Ok(())
    }

    fn set_icon(&self, _icon: Icon) -> Result<()> {
        Ok(())
    }

    fn set_skip_taskbar(&self, skip: bool) -> Result<()> {
        self.inner.skip_taskbar.store(skip, Ordering::SeqCst);
        let hwnd = self.inner.hwnd();
        let mut ex_style = unsafe { sys::GetWindowLongPtrW(hwnd, sys::GWL_EXSTYLE) } as u32;
        if skip {
            ex_style |= sys::WS_EX_TOOLWINDOW;
            ex_style &= !sys::WS_EX_APPWINDOW;
        } else {
            ex_style &= !sys::WS_EX_TOOLWINDOW;
            ex_style |= sys::WS_EX_APPWINDOW;
        }
        unsafe { sys::SetWindowLongPtrW(hwnd, sys::GWL_EXSTYLE, ex_style as isize) };
        Ok(())
    }

    fn set_cursor_grab(&self, _grab: bool) -> Result<()> {
        Ok(())
    }

    fn set_cursor_visible(&self, _visible: bool) -> Result<()> {
        Ok(())
    }

    fn set_cursor_icon(&self, _icon: tauri_runtime::window::CursorIcon) -> Result<()> {
        Ok(())
    }

    fn set_cursor_position<Pos: Into<Position>>(&self, position: Pos) -> Result<()> {
        let scale = self.inner.scale_factor();
        let (x, y) = physical_position(position.into(), scale);
        let mut point = sys::POINT { x, y };
        let hwnd = self.inner.hwnd();
        unsafe {
            sys::ClientToScreen(hwnd, &mut point);
            sys::SetCursorPos(point.x, point.y);
        }
        Ok(())
    }

    fn set_ignore_cursor_events(&self, _ignore: bool) -> Result<()> {
        Ok(())
    }

    fn start_dragging(&self) -> Result<()> {
        let hwnd = self.inner.hwnd();
        unsafe {
            sys::ReleaseCapture();
            sys::SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
        }
        Ok(())
    }

    fn start_resize_dragging(&self, _direction: ResizeDirection) -> Result<()> {
        Ok(())
    }

    fn set_badge_count(&self, _count: Option<i64>, _desktop_filename: Option<String>) -> Result<()> {
        Ok(())
    }

    fn set_badge_label(&self, _label: Option<String>) -> Result<()> {
        Ok(())
    }

    fn set_overlay_icon(&self, _icon: Option<Icon>) -> Result<()> {
        Ok(())
    }

    fn set_progress_bar(&self, _progress_state: ProgressBarState) -> Result<()> {
        Ok(())
    }

    fn set_title_bar_style(&self, _style: TitleBarStyle) -> Result<()> {
        Ok(())
    }

    fn set_traffic_light_position(&self, _position: Position) -> Result<()> {
        Ok(())
    }

    fn set_theme(&self, theme: Option<Theme>) -> Result<()> {
        *self.inner.theme.lock().unwrap() = theme;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

pub(crate) fn physical_size(size: Size, scale: f64) -> (u32, u32) {
    match size {
        Size::Physical(size) => (size.width, size.height),
        Size::Logical(size) => (
            (size.width * scale).round().max(1.0) as u32,
            (size.height * scale).round().max(1.0) as u32,
        ),
    }
}

pub(crate) fn physical_position(position: Position, scale: f64) -> (i32, i32) {
    match position {
        Position::Physical(position) => (position.x, position.y),
        Position::Logical(position) => (
            (position.x * scale).round() as i32,
            (position.y * scale).round() as i32,
        ),
    }
}

#[allow(dead_code)]
fn logical_size(width: u32, height: u32, scale: f64) -> LogicalSize<f64> {
    LogicalSize::new(width as f64 / scale, height as f64 / scale)
}

#[allow(dead_code)]
fn logical_position(x: i32, y: i32, scale: f64) -> LogicalPosition<f64> {
    LogicalPosition::new(x as f64 / scale, y as f64 / scale)
}

#[allow(dead_code)]
fn rect_from_physical(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        position: Position::Physical(PhysicalPosition::new(x, y)),
        size: Size::Physical(PhysicalSize::new(width, height)),
    }
}
