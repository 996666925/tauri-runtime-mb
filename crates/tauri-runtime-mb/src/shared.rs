//! Process wide state shared by every runtime handle.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tauri_runtime::dpi::{PhysicalPosition, PhysicalRect, PhysicalSize};
use tauri_runtime::monitor::Monitor;
use tauri_runtime::window::{WindowEvent, WindowId};
use tauri_runtime::{Error, Result, RunEvent, UserEvent};
use tauri_utils::Theme;

use crate::sys;
use crate::window::WindowInner;

pub(crate) type Task = Box<dyn FnOnce() + Send>;
pub(crate) type EventHandler = Arc<Mutex<Box<dyn Fn(&WindowEvent) + Send>>>;

/// State owned by the runtime and shared with all of its handles.
pub(crate) struct Shared<T: UserEvent> {
    pub(crate) main_thread: u32,
    /// Hidden message only window used to wake the event loop.
    pub(crate) msg_hwnd: AtomicIsize,
    pub(crate) tasks: Mutex<VecDeque<Task>>,
    pub(crate) events: Mutex<VecDeque<RunEvent<T>>>,
    pub(crate) windows: Mutex<HashMap<WindowId, Arc<WindowInner<T>>>>,
    pub(crate) next_window_id: AtomicU32,
    pub(crate) next_event_id: AtomicU32,
    pub(crate) theme: Mutex<Option<Theme>>,
    pub(crate) exiting: AtomicBool,
    pub(crate) exit_code: AtomicI32,
    pub(crate) app_id: String,
}

impl<T: UserEvent> Shared<T> {
    pub(crate) fn new(app_id: String) -> Self {
        Self {
            main_thread: sys::current_thread_id(),
            msg_hwnd: AtomicIsize::new(0),
            tasks: Mutex::new(VecDeque::new()),
            events: Mutex::new(VecDeque::new()),
            windows: Mutex::new(HashMap::new()),
            next_window_id: AtomicU32::new(1),
            next_event_id: AtomicU32::new(1),
            theme: Mutex::new(None),
            exiting: AtomicBool::new(false),
            exit_code: AtomicI32::new(0),
            app_id,
        }
    }

    pub(crate) fn is_main_thread(&self) -> bool {
        sys::current_thread_id() == self.main_thread
    }

    pub(crate) fn next_event_id(&self) -> u32 {
        self.next_event_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Creates the hidden wake-up window. Must run on the main thread.
    pub(crate) fn ensure_message_window(&self) {
        if self.msg_hwnd.load(Ordering::SeqCst) != 0 {
            return;
        }
        let hwnd = crate::window::create_message_window();
        self.msg_hwnd.store(hwnd, Ordering::SeqCst);
    }

    pub(crate) fn wake(&self) {
        let hwnd = self.msg_hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            // SAFETY: hwnd is a valid message-only window owned by the runtime.
            unsafe { sys::PostMessageW(hwnd, sys::WM_RUNTIME_WAKE, 0, 0) };
        }
    }

    pub(crate) fn post_task(&self, task: Task) {
        self.tasks.lock().unwrap().push_back(task);
        self.wake();
    }

    /// Runs a closure on the main thread and waits for its result.
    pub(crate) fn run_on_main_thread<R, F>(self: &Arc<Self>, f: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        if self.is_main_thread() {
            return Ok(f());
        }

        let (tx, rx) = std::sync::mpsc::channel();
        self.post_task(Box::new(move || {
            let _ = tx.send(f());
        }));
        rx.recv().map_err(|_| Error::EventLoopClosed)
    }

    pub(crate) fn drain_tasks(&self) {
        loop {
            let task = { self.tasks.lock().unwrap().pop_front() };
            match task {
                Some(task) => task(),
                None => break,
            }
        }
    }

    pub(crate) fn push_event(&self, event: RunEvent<T>) {
        self.events.lock().unwrap().push_back(event);
        self.wake();
    }

    pub(crate) fn drain_events(&self) -> Vec<RunEvent<T>> {
        let mut queue = self.events.lock().unwrap();
        queue.drain(..).collect()
    }

    pub(crate) fn request_exit(&self, code: i32) {
        self.exit_code.store(code, Ordering::SeqCst);
        self.exiting.store(true, Ordering::SeqCst);
        self.wake();
    }

    pub(crate) fn is_exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    pub(crate) fn window(&self, id: WindowId) -> Option<Arc<WindowInner<T>>> {
        self.windows.lock().unwrap().get(&id).cloned()
    }

    pub(crate) fn has_windows(&self) -> bool {
        !self.windows.lock().unwrap().is_empty()
    }
}

// ---------------------------------------------------------------------------
// monitors / cursor
// ---------------------------------------------------------------------------

unsafe extern "system" fn monitor_enum_proc(
    monitor: sys::HMONITOR,
    _hdc: sys::HDC,
    _rect: *mut sys::RECT,
    lparam: sys::LPARAM,
) -> sys::BOOL {
    // SAFETY: lparam is the `Vec<Monitor>` passed by `available_monitors`.
    let list = unsafe { &mut *(lparam as *mut Vec<Monitor>) };
    if let Some(descriptor) = unsafe { describe_monitor(monitor) } {
        list.push(descriptor);
    }
    1
}

unsafe fn describe_monitor(monitor: sys::HMONITOR) -> Option<Monitor> {
    if monitor == 0 {
        return None;
    }
    let mut info = sys::MONITORINFOEXW {
        cbSize: std::mem::size_of::<sys::MONITORINFOEXW>() as u32,
        ..Default::default()
    };
    // SAFETY: info is properly sized and monitor is a valid handle.
    if unsafe { sys::GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }

    let name_len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
    let name = String::from_utf16_lossy(&info.szDevice[..name_len]);

    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    // SAFETY: valid monitor handle.
    let hr = unsafe {
        sys::GetDpiForMonitor(monitor, sys::MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)
    };
    let scale_factor = if hr == 0 && dpi_x != 0 { dpi_x as f64 / 96.0 } else { 1.0 };

    Some(Monitor {
        name: Some(name),
        size: PhysicalSize::new(info.rcMonitor.width() as u32, info.rcMonitor.height() as u32),
        position: PhysicalPosition::new(info.rcMonitor.left, info.rcMonitor.top),
        work_area: PhysicalRect {
            position: PhysicalPosition::new(info.rcWork.left, info.rcWork.top),
            size: PhysicalSize::new(info.rcWork.width() as u32, info.rcWork.height() as u32),
        },
        scale_factor,
    })
}

/// Every monitor attached to the desktop.
pub(crate) fn available_monitors() -> Vec<Monitor> {
    let mut list: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only touches the vec we hand over.
    unsafe {
        sys::EnumDisplayMonitors(
            0,
            std::ptr::null(),
            Some(monitor_enum_proc),
            &mut list as *mut Vec<Monitor> as sys::LPARAM,
        );
    }
    list
}

pub(crate) fn monitor_from_handle(monitor: sys::HMONITOR) -> Option<Monitor> {
    // SAFETY: monitor comes from a Win32 call.
    unsafe { describe_monitor(monitor) }
}

/// Monitor that contains the given point.
pub(crate) fn monitor_from_point(x: f64, y: f64) -> Option<Monitor> {
    let point = sys::POINT {
        x: x as i32,
        y: y as i32,
    };
    // SAFETY: plain struct to struct call.
    let monitor = unsafe { sys::MonitorFromPoint(point, sys::MONITOR_DEFAULTTONEAREST) };
    monitor_from_handle(monitor)
}

/// Primary monitor of the desktop.
pub(crate) fn primary_monitor() -> Option<Monitor> {
    // The centre of the virtual screen can land on a secondary display (or in
    // the gap between two monitors), ask the system for the primary one.
    const MONITOR_DEFAULTTOPRIMARY: u32 = 1;
    let point = sys::POINT { x: 0, y: 0 };
    // SAFETY: plain struct to struct call.
    let monitor = unsafe { sys::MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };
    monitor_from_handle(monitor)
}

/// Position of the mouse cursor in screen coordinates.
pub(crate) fn cursor_position() -> Result<PhysicalPosition<f64>> {
    let mut point = sys::POINT::default();
    // SAFETY: out parameter.
    if unsafe { sys::GetCursorPos(&mut point) } == 0 {
        return Err(Error::FailedToGetCursorPosition);
    }
    Ok(PhysicalPosition::new(point.x as f64, point.y as f64))
}
