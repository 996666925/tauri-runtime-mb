//! A [Tauri 3](https://tauri.app) runtime backed by the Miniblink browser engine.
//!
//! The runtime hosts Miniblink in offscreen mode: the engine renders into its
//! own memory DC and the runtime blits each dirty rectangle into a plain Win32
//! window. Everything Tauri needs (custom protocols, IPC, window events, the
//! event loop) is implemented on top of `mb.h`.
//!
//! ```no_run
//! tauri::Builder::default()
//!     .runtime(tauri_runtime_mb::MbInitAttrs::default())
//!     .run(tauri::generate_context!())
//!     .unwrap();
//! ```

#![allow(non_snake_case)]

#[cfg(not(all(windows, target_pointer_width = "64")))]
compile_error!(
    "tauri-runtime-mb only supports 64-bit Windows: the Win32 base (`sys`) and the engine bindings \
     assume the x64 ABI"
);

mod handle;
mod ime;
mod shared;
pub mod sys;
mod webview;
mod window;
mod window_frame;

use std::path::PathBuf;
use std::sync::Arc;

use tauri_runtime::window::{DetachedWindow, PendingWindow, RawWindow, WindowId};
use tauri_runtime::{
    DeviceEventFilter, Error, ExitRequestedEventAction, Result, RunEvent, Runtime,
    RuntimeInitArgs, RuntimeInitAttrs, UserEvent,
};
use tauri_utils::Theme;

pub use handle::{MbEventLoopProxy, MbRuntimeHandle};
pub use webview::{MbPlatformWebview, MbWebviewDispatcher};
pub use window::{MbWindowBuilder, MbWindowDispatcher};

use crate::shared::Shared;

/// Runtime specific attributes used to select and configure this runtime.
///
/// Pass it to `tauri::Builder::runtime`:
/// `builder.runtime(MbInitAttrs::default())`.
#[derive(Debug, Clone, Default)]
pub struct MbInitAttrs {
    /// Full path of `mb132_x64.dll`. Defaults to a sibling of the executable.
    pub dll_path: Option<PathBuf>,
    /// Enable the Node.js integration of the engine.
    pub nodejs: bool,
    /// Extra engine configuration string (`mbSettings::config`).
    pub config: Option<String>,
}

impl<T: UserEvent> RuntimeInitAttrs<T> for MbInitAttrs {
    type Runtime = MbRuntime<T>;
}

/// Runtime specific webview attributes. Not used by this runtime yet.
#[derive(Debug, Clone, Default)]
pub struct MbWebviewAttributes;

/// Data of the window that requested a new window.
#[derive(Debug, Clone, Default)]
pub struct MbWindowOpener;

/// The Miniblink runtime.
pub struct MbRuntime<T: UserEvent> {
    shared: Arc<Shared<T>>,
}

impl<T: UserEvent> std::fmt::Debug for MbRuntime<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbRuntime")
            .field("app", &self.shared.app_id)
            .finish_non_exhaustive()
    }
}

impl<T: UserEvent> MbRuntime<T> {
    fn create(args: RuntimeInitArgs<MbInitAttrs>) -> Result<Self> {
        let attrs = args.runtime_init_attrs;
        let options = miniblink::InitOptions {
            dll_path: attrs.dll_path.unwrap_or_else(miniblink::default_dll_path),
            nodejs: attrs.nodejs,
            config: attrs.config,
        };

        miniblink::init(options).map_err(|error| Error::CreateWebview(Box::new(error)))?;

        let shared = Arc::new(Shared::new(args.identifier));
        shared.ensure_message_window();

        Ok(Self { shared })
    }

    fn run_loop<F>(self, mut callback: F) -> i32
    where
        F: FnMut(RunEvent<T>) + 'static,
    {
        let shared = Arc::clone(&self.shared);
        shared.ensure_message_window();
        let trace = std::env::var_os("MB_TRACE").is_some();
        if trace {
            eprintln!(
                "[mb] run_loop start threads={} windows={} msg_hwnd={}",
                shared.main_thread,
                shared.windows.lock().unwrap().len(),
                shared
                    .msg_hwnd
                    .load(std::sync::atomic::Ordering::SeqCst)
            );
        }
        callback(RunEvent::Ready);

        let mut had_window = shared.has_windows();
        let mut exit_announced = false;
        let mut msg = sys::MSG::default();

        loop {
            let mut dispatched = false;

            loop {
                let result = unsafe { sys::PeekMessageW(&mut msg, 0, 0, 0, sys::PM_REMOVE) };
                if result == 0 {
                    break;
                }
                dispatched = true;

                if msg.message == sys::WM_QUIT {
                    shared
                        .exit_code
                        .store(msg.wParam as i32, std::sync::atomic::Ordering::SeqCst);
                    shared.exiting.store(true, std::sync::atomic::Ordering::SeqCst);
                    continue;
                }

                unsafe {
                    sys::TranslateMessage(&msg);
                    sys::DispatchMessageW(&msg);
                }
            }

            shared.drain_tasks();
            for event in shared.drain_events() {
                callback(event);
            }
            callback(RunEvent::MainEventsCleared);

            if shared.has_windows() {
                had_window = true;
                exit_announced = false;
            }

            let out_of_windows = had_window && !shared.has_windows();
            if shared.is_exiting() || out_of_windows {
                if exit_announced {
                    // Already asked the app whether it wants to exit and it
                    // prevented it: keep running without spamming the callback.
                    if !shared.has_windows() && !dispatched {
                        let result = unsafe { sys::GetMessageW(&mut msg, 0, 0, 0) };
                        if result <= 0 {
                            break;
                        }
                        unsafe {
                            sys::TranslateMessage(&msg);
                            sys::DispatchMessageW(&msg);
                        }
                    }
                    continue;
                }

                let code = shared.exit_code.load(std::sync::atomic::Ordering::SeqCst);
                let (tx, rx) = std::sync::mpsc::channel();
                callback(RunEvent::ExitRequested {
                    code: Some(code),
                    tx,
                });
                let prevent = matches!(rx.try_recv(), Ok(ExitRequestedEventAction::Prevent));
                if prevent {
                    shared.exiting.store(false, std::sync::atomic::Ordering::SeqCst);
                    exit_announced = true;
                    continue;
                }

                break;
            }

            if !dispatched {
                let result = unsafe { sys::GetMessageW(&mut msg, 0, 0, 0) };
                if result <= 0 {
                    if trace {
                        eprintln!("[mb] GetMessageW -> {result}, leaving the loop");
                    }
                    break;
                }
                unsafe {
                    sys::TranslateMessage(&msg);
                    sys::DispatchMessageW(&msg);
                }
            }
        }

        callback(RunEvent::Exit);
        shared.exit_code.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl<T: UserEvent> Runtime<T> for MbRuntime<T> {
    type WindowDispatcher = MbWindowDispatcher<T>;
    type WebviewDispatcher = MbWebviewDispatcher<T>;
    type Handle = MbRuntimeHandle<T>;
    type EventLoopProxy = MbEventLoopProxy<T>;
    type RuntimeWebviewAttributes = MbWebviewAttributes;
    type Webview = MbPlatformWebview;
    type RuntimeInitAttrs = MbInitAttrs;
    type WindowOpener = MbWindowOpener;

    fn new(args: RuntimeInitArgs<Self::RuntimeInitAttrs>) -> Result<Self> {
        Self::create(args)
    }

    fn new_any_thread(args: RuntimeInitArgs<Self::RuntimeInitAttrs>) -> Result<Self> {
        Self::create(args)
    }

    fn create_proxy(&self) -> Self::EventLoopProxy {
        MbEventLoopProxy {
            shared: Arc::clone(&self.shared),
        }
    }

    fn handle(&self) -> Self::Handle {
        MbRuntimeHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    fn create_window<F: Fn(RawWindow<'_>) + Send + 'static>(
        &self,
        pending: PendingWindow<T, Self>,
        after_window_creation: Option<F>,
    ) -> Result<DetachedWindow<T, Self>> {
        window::create_window(&self.shared, pending, after_window_creation)
    }

    fn create_webview(
        &self,
        window_id: WindowId,
        pending: tauri_runtime::webview::PendingWebview<T, Self>,
    ) -> Result<tauri_runtime::webview::DetachedWebview<T, Self>> {
        let window = self
            .shared
            .window(window_id)
            .ok_or(Error::WindowNotFound)?;
        let dispatcher = webview::create_webview_in_window(&window, pending)?;
        Ok(tauri_runtime::webview::DetachedWebview {
            label: dispatcher.inner.label.clone(),
            dispatcher,
        })
    }

    fn primary_monitor(&self) -> Option<tauri_runtime::monitor::Monitor> {
        shared::primary_monitor()
    }

    fn monitor_from_point(&self, x: f64, y: f64) -> Option<tauri_runtime::monitor::Monitor> {
        shared::monitor_from_point(x, y)
    }

    fn available_monitors(&self) -> Vec<tauri_runtime::monitor::Monitor> {
        shared::available_monitors()
    }

    fn cursor_position(&self) -> Result<tauri_runtime::dpi::PhysicalPosition<f64>> {
        shared::cursor_position()
    }

    fn set_theme(&self, theme: Option<Theme>) {
        *self.shared.theme.lock().unwrap() = theme;
    }

    fn set_device_event_filter(&mut self, _filter: DeviceEventFilter) {}

    fn run_iteration<F: FnMut(RunEvent<T>) + 'static>(&mut self, mut callback: F) {
        self.shared.ensure_message_window();

        let mut msg = sys::MSG::default();
        loop {
            let result = unsafe { sys::PeekMessageW(&mut msg, 0, 0, 0, sys::PM_REMOVE) };
            if result == 0 {
                break;
            }
            unsafe {
                sys::TranslateMessage(&msg);
                sys::DispatchMessageW(&msg);
            }
        }

        self.shared.drain_tasks();
        for event in self.shared.drain_events() {
            callback(event);
        }
        callback(RunEvent::MainEventsCleared);
    }

    fn run_return<F: FnMut(RunEvent<T>) + 'static>(self, callback: F) -> i32 {
        self.run_loop(callback)
    }

    fn run<F: FnMut(RunEvent<T>) + 'static>(self, callback: F) {
        let code = self.run_loop(callback);
        std::process::exit(code);
    }
}
