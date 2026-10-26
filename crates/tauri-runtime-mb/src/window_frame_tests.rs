//! Exercise the real nonclient procedure without showing a desktop window.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tauri_runtime::WindowDispatch;
use tauri_runtime::dpi::{PhysicalSize, Size};
use tauri_runtime::window::{PendingWindow, RawWindow, WindowBuilder};

use super::*;
use crate::shared::Shared;
use crate::window::{MbWindowBuilder, MbWindowDispatcher, create_window};

#[link(name = "user32")]
unsafe extern "system" {
    fn SetThreadDpiAwarenessContext(context: isize) -> isize;
}

struct HiddenWindow {
    dispatcher: MbWindowDispatcher<()>,
    previous_dpi: isize,
}

impl HiddenWindow {
    fn new(decorations: bool, shadow: bool, resizable: bool) -> Self {
        let previous_dpi = unsafe { SetThreadDpiAwarenessContext(-4) }; // Per-monitor v2.
        let shared = Arc::new(Shared::new("frame-regression".into()));
        let pending = PendingWindow::new(
            MbWindowBuilder::new()
                .visible(false)
                .focused(false)
                .inner_size(480.0, 320.0)
                .decorations(decorations)
                .shadow(shadow)
                .resizable(resizable),
            "frame-regression",
        )
        .unwrap();
        let dispatcher = create_window(&shared, pending, None::<fn(RawWindow<'_>)>)
            .unwrap()
            .dispatcher;
        Self {
            dispatcher,
            previous_dpi,
        }
    }

    fn hwnd(&self) -> isize {
        self.dispatcher.inner.hwnd()
    }

    fn origin(&self) -> sys::POINT {
        let mut origin = sys::POINT::default();
        unsafe { sys::ClientToScreen(self.hwnd(), &mut origin) };
        origin
    }

    fn hit(&self, x: i32, y: i32) -> isize {
        let point = (x as u16 as usize | ((y as u16 as usize) << 16)) as isize;
        unsafe { sys::SendMessageW(self.hwnd(), sys::WM_NCHITTEST, 0, point) }
    }

    fn assert_client_size(&self, width: u32, height: u32) {
        assert_eq!(
            self.dispatcher.inner_size().unwrap(),
            PhysicalSize::new(width, height)
        );
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe {
            sys::DestroyWindow(self.hwnd());
            if self.previous_dpi != 0 {
                SetThreadDpiAwarenessContext(self.previous_dpi);
            }
        }
    }
}

#[test]
fn borderless_creation_and_resize_keep_requested_client_size() {
    for shadow in [false, true] {
        for resizable in [false, true] {
            let window = HiddenWindow::new(false, shadow, resizable);
            let scale = window.dispatcher.scale_factor().unwrap();
            window.assert_client_size(
                (480.0 * scale).round() as u32,
                (320.0 * scale).round() as u32,
            );
            let style = unsafe { sys::GetWindowLongPtrW(window.hwnd(), sys::GWL_STYLE) } as u32;
            assert_eq!(style & sys::WS_POPUP, 0);
            assert_ne!(
                style & sys::WS_CAPTION,
                0,
                "DWM requires the native styles to remain"
            );
            assert_eq!(style & sys::WS_THICKFRAME != 0, resizable);
            let (_, _, width, height) = sys::window_rect(window.hwnd());
            let insets = borderless_insets(unsafe { sys::GetDpiForWindow(window.hwnd()) }, shadow);
            assert_eq!(
                width,
                (480.0 * scale).round() as i32 + insets.left + insets.right
            );
            assert_eq!(
                height,
                (320.0 * scale).round() as i32 + insets.top + insets.bottom
            );
            window
                .dispatcher
                .set_size(Size::Physical(PhysicalSize::new(500, 350)))
                .unwrap();
            window.assert_client_size(500, 350);
            assert_eq!(unsafe { sys::IsWindowVisible(window.hwnd()) }, 0);
        }
    }
}

#[test]
fn borderless_shadow_and_decorations_can_toggle_without_changing_resize_policy() {
    let window = HiddenWindow::new(false, true, false);
    let (x, y, width, height) = sys::window_rect(window.hwnd());
    let borderless_top = window.origin().y - y;
    window.dispatcher.set_decorations(true).unwrap();
    assert!(
        window.origin().y - y > borderless_top,
        "native caption should return"
    );
    let style = unsafe { sys::GetWindowLongPtrW(window.hwnd(), sys::GWL_STYLE) } as u32;
    assert_eq!(
        style & sys::WS_THICKFRAME,
        0,
        "decorations must not re-enable resizing"
    );
    window.dispatcher.set_decorations(false).unwrap();
    assert_eq!(window.origin().y - y, borderless_top);
    window.dispatcher.set_shadow(false).unwrap();
    assert_eq!((window.origin().x, window.origin().y), (x, y));
    window.assert_client_size(width as u32, height as u32);
    window.dispatcher.set_shadow(true).unwrap();
    assert!(window.origin().x > x);
    assert_eq!(sys::window_rect(window.hwnd()), (x, y, width, height));
    assert_eq!(unsafe { sys::IsWindowVisible(window.hwnd()) }, 0);

    // Standard decorated windows keep their native caption and client sizing.
    let decorated = HiddenWindow::new(true, true, true);
    decorated
        .dispatcher
        .set_size(Size::Physical(PhysicalSize::new(500, 350)))
        .unwrap();
    decorated.assert_client_size(500, 350);
    decorated.dispatcher.set_maximizable(false).unwrap();
    decorated.dispatcher.set_minimizable(false).unwrap();
    let style = unsafe { sys::GetWindowLongPtrW(decorated.hwnd(), sys::GWL_STYLE) } as u32;
    assert_eq!(style & (sys::WS_MAXIMIZEBOX | sys::WS_MINIMIZEBOX), 0);
}

#[test]
fn borderless_native_hit_testing_covers_all_edges_and_respects_resizable() {
    let window = HiddenWindow::new(false, false, true);
    let (left, top, width, height) = sys::window_rect(window.hwnd());
    let right = left + width;
    let bottom = top + height;
    for (x, y, expected) in [
        (left, top, sys::HTTOPLEFT),
        (right - 1, top, sys::HTTOPRIGHT),
        (left, bottom - 1, sys::HTBOTTOMLEFT),
        (right - 1, bottom - 1, sys::HTBOTTOMRIGHT),
        (left, top + height / 2, sys::HTLEFT),
        (right - 1, top + height / 2, sys::HTRIGHT),
        (left + width / 2, top, sys::HTTOP),
        (left + width / 2, bottom - 1, sys::HTBOTTOM),
        (left + width / 2, top + height / 2, sys::HTCLIENT),
    ] {
        assert_eq!(
            window.hit(x, y),
            expected as isize,
            "screen point ({x},{y})"
        );
    }
    window.dispatcher.set_resizable(false).unwrap();
    assert_eq!(window.hit(left, top), sys::HTCLIENT as isize);
    window.dispatcher.set_resizable(true).unwrap();
    window.dispatcher.set_shadow(true).unwrap();
    let origin = window.origin();
    assert_eq!(
        window.hit(origin.x + 100, origin.y + 1),
        sys::HTTOP as isize
    );
}

#[test]
fn borderless_maximized_client_uses_work_area_but_fullscreen_does_not() {
    let window = HiddenWindow::new(false, true, true);
    let monitor = unsafe { sys::MonitorFromWindow(window.hwnd(), sys::MONITOR_DEFAULTTONEAREST) };
    let mut info = sys::MONITORINFOEXW {
        cbSize: std::mem::size_of::<sys::MONITORINFOEXW>() as u32,
        ..Default::default()
    };
    assert_ne!(unsafe { sys::GetMonitorInfoW(monitor, &mut info) }, 0);
    // Set the maximized flag without ShowWindow, keeping this native test hidden.
    const WS_MAXIMIZE: u32 = 0x0100_0000;
    let style = unsafe { sys::GetWindowLongPtrW(window.hwnd(), sys::GWL_STYLE) } as u32;
    unsafe {
        sys::SetWindowLongPtrW(
            window.hwnd(),
            sys::GWL_STYLE,
            (style | WS_MAXIMIZE) as isize,
        )
    };
    assert_ne!(unsafe { sys::IsZoomed(window.hwnd()) }, 0);
    let calculate = || {
        let mut params = sys::NCCALCSIZE_PARAMS {
            rgrc: [info.rcMonitor; 3],
            lppos: std::ptr::null_mut(),
        };
        let result = unsafe {
            sys::SendMessageW(
                window.hwnd(),
                sys::WM_NCCALCSIZE,
                1,
                &mut params as *mut _ as isize,
            )
        };
        assert_eq!(result, 0);
        params.rgrc[0]
    };
    let client = calculate();
    assert!((0..=1).contains(&(client.left - info.rcWork.left)));
    assert!((0..=1).contains(&(client.top - info.rcWork.top)));
    assert!((0..=1).contains(&(info.rcWork.right - client.right)));
    assert!((0..=1).contains(&(info.rcWork.bottom - client.bottom)));
    window
        .dispatcher
        .inner
        .fullscreen
        .store(true, Ordering::SeqCst);
    let fullscreen = calculate();
    assert_eq!(
        (
            fullscreen.left,
            fullscreen.top,
            fullscreen.right,
            fullscreen.bottom
        ),
        (
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom
        )
    );
}

#[test]
fn borderless_resize_hit_tests_preserve_signed_monitor_coordinates() {
    let rect = sys::RECT {
        left: -900,
        top: -600,
        right: -300,
        bottom: -200,
    };
    assert_eq!(
        hit_test(&rect, -900, -600, 8, 8),
        Some(sys::HTTOPLEFT as isize)
    );
    assert_eq!(
        hit_test(&rect, -301, -201, 8, 8),
        Some(sys::HTBOTTOMRIGHT as isize)
    );
    assert_eq!(hit_test(&rect, -600, -400, 8, 8), None);
    assert_eq!(hit_test(&rect, -901, -600, 8, 8), None);
}
