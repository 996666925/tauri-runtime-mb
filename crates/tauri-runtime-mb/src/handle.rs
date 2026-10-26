//! Runtime handle and event loop proxy.

use std::sync::Arc;

use tauri_runtime::webview::{DetachedWebview, PendingWebview};
use tauri_runtime::window::{DetachedWindow, PendingWindow, RawWindow, WindowId};
use tauri_runtime::{
    DeviceEventFilter, EventLoopProxy, Result, RunEvent, Runtime, RuntimeHandle, UserEvent,
};
use tauri_utils::Theme;

use crate::shared::Shared;
use crate::webview::create_webview_in_window;
use crate::window::{create_window, MbWindowDispatcher};
use crate::MbRuntime;

/// Proxy that can wake the event loop from any thread.
pub struct MbEventLoopProxy<T: UserEvent> {
    pub(crate) shared: Arc<Shared<T>>,
}

impl<T: UserEvent> Clone for MbEventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T: UserEvent> std::fmt::Debug for MbEventLoopProxy<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbEventLoopProxy").finish_non_exhaustive()
    }
}

impl<T: UserEvent> EventLoopProxy<T> for MbEventLoopProxy<T> {
    fn send_event(&self, event: T) -> Result<()> {
        self.shared.push_event(RunEvent::UserEvent(event));
        Ok(())
    }
}

/// Thread-safe handle to the runtime.
pub struct MbRuntimeHandle<T: UserEvent> {
    pub(crate) shared: Arc<Shared<T>>,
}

impl<T: UserEvent> Clone for MbRuntimeHandle<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T: UserEvent> std::fmt::Debug for MbRuntimeHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbRuntimeHandle").finish_non_exhaustive()
    }
}

impl<T: UserEvent> RuntimeHandle<T> for MbRuntimeHandle<T> {
    type Runtime = MbRuntime<T>;

    fn create_proxy(&self) -> <Self::Runtime as Runtime<T>>::EventLoopProxy {
        MbEventLoopProxy {
            shared: Arc::clone(&self.shared),
        }
    }

    fn request_exit(&self, code: i32) -> Result<()> {
        self.shared.request_exit(code);
        Ok(())
    }

    fn create_window<F: Fn(RawWindow<'_>) + Send + 'static>(
        &self,
        pending: PendingWindow<T, Self::Runtime>,
        after_window_creation: Option<F>,
    ) -> Result<DetachedWindow<T, Self::Runtime>> {
        let shared = Arc::clone(&self.shared);
        let task_shared = Arc::clone(&shared);
        shared.run_on_main_thread(move || {
            create_window(&task_shared, pending, after_window_creation)
        })?
    }

    fn create_webview(
        &self,
        window_id: WindowId,
        pending: PendingWebview<T, Self::Runtime>,
    ) -> Result<DetachedWebview<T, Self::Runtime>> {
        let shared = Arc::clone(&self.shared);
        let task_shared = Arc::clone(&shared);
        shared.run_on_main_thread(move || {
            let window = task_shared
                .window(window_id)
                .ok_or(tauri_runtime::Error::WindowNotFound)?;
            let dispatcher = create_webview_in_window(&window, pending)?;
            Ok(DetachedWebview {
                label: dispatcher.inner.label.clone(),
                dispatcher,
            })
        })?
    }

    fn run_on_main_thread<F: FnOnce() + Send + 'static>(&self, f: F) -> Result<()> {
        self.shared.post_task(Box::new(f));
        Ok(())
    }

    fn display_handle(
        &self,
    ) -> std::result::Result<
        raw_window_handle::DisplayHandle<'_>,
        raw_window_handle::HandleError,
    > {
        let raw = raw_window_handle::RawDisplayHandle::Windows(
            raw_window_handle::WindowsDisplayHandle::new(),
        );
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
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

    fn cursor_position(&self) -> Result<tauri_runtime::dpi::PhysicalPosition<f64>> {
        crate::shared::cursor_position()
    }

    fn set_theme(&self, theme: Option<Theme>) {
        *self.shared.theme.lock().unwrap() = theme;
    }

    fn set_device_event_filter(&self, _filter: DeviceEventFilter) {}

    fn custom_scheme_url(&self, scheme: &str, https: bool) -> String {
        if https {
            format!("https://{scheme}.localhost")
        } else {
            format!("http://{scheme}.localhost")
        }
    }

    fn webview_version(&self) -> Result<String> {
        Ok(format!("miniblink {}", mb132::version()))
    }
}

impl<T: UserEvent> MbRuntimeHandle<T> {
    /// Dispatcher of an already created window.
    pub fn window(&self, id: WindowId) -> Option<MbWindowDispatcher<T>> {
        self.shared
            .window(id)
            .map(|inner| MbWindowDispatcher { inner })
    }
}
