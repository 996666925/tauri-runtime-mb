//! Exercise the real nonclient procedure without showing a desktop window.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tauri_runtime::WindowDispatch;
use tauri_runtime::dpi::{PhysicalSize, Size};
use tauri_runtime::webview::{PendingWebview, WebviewAttributes};
use tauri_runtime::window::{PendingWindow, RawWindow, WindowBuilder};
use tauri_utils::config::WebviewUrl;

use super::*;
use crate::shared::Shared;
use crate::window::{MbWindowBuilder, MbWindowDispatcher, create_window};

#[link(name = "user32")]
unsafe extern "system" {
    fn SetThreadDpiAwarenessContext(context: isize) -> isize;
    fn GetCursor() -> isize;
    fn GetCapture() -> isize;
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
    assert_eq!(window.hit(origin.x, origin.y + 1), sys::HTTOPLEFT as isize);
    let (_, _, width, _) = sys::client_rect(window.hwnd());
    assert_eq!(
        window.hit(origin.x + width - 1, origin.y + 1),
        sys::HTTOPRIGHT as isize
    );
}

#[test]
fn native_resize_edges_use_system_cursors() {
    let previous = unsafe { GetCursor() };
    for decorations in [false, true] {
        for shadow in [false, true] {
            let window = HiddenWindow::new(decorations, shadow, true);
            for (hit, cursor) in [
                (sys::HTLEFT, 32644), // IDC_SIZEWE
                (sys::HTRIGHT, 32644),
                (sys::HTTOP, 32645), // IDC_SIZENS
                (sys::HTBOTTOM, 32645),
                (sys::HTTOPLEFT, 32642), // IDC_SIZENWSE
                (sys::HTBOTTOMRIGHT, 32642),
                (sys::HTTOPRIGHT, 32643), // IDC_SIZENESW
                (sys::HTBOTTOMLEFT, 32643),
            ] {
                let expected = unsafe { sys::LoadCursorW(0, cursor as *const u16) };
                assert_ne!(expected, 0);
                unsafe {
                    sys::SendMessageW(
                        window.hwnd(),
                        sys::WM_SETCURSOR,
                        window.hwnd() as usize,
                        (hit as isize) | ((sys::WM_MOUSEMOVE as isize) << 16),
                    );
                }
                let actual = unsafe { GetCursor() };
                unsafe { sys::SetCursor(previous) };
                assert_eq!(
                    actual, expected,
                    "decorations={decorations} shadow={shadow} hit={hit}"
                );
            }
        }
    }
}

#[test]
fn resize_dragging_posts_native_directions_and_releases_capture() {
    let window = HiddenWindow::new(false, false, true);
    for (direction, expected) in [
        (tauri_runtime::ResizeDirection::East, sys::HTRIGHT),
        (tauri_runtime::ResizeDirection::North, sys::HTTOP),
        (tauri_runtime::ResizeDirection::NorthEast, sys::HTTOPRIGHT),
        (tauri_runtime::ResizeDirection::NorthWest, sys::HTTOPLEFT),
        (tauri_runtime::ResizeDirection::South, sys::HTBOTTOM),
        (
            tauri_runtime::ResizeDirection::SouthEast,
            sys::HTBOTTOMRIGHT,
        ),
        (tauri_runtime::ResizeDirection::SouthWest, sys::HTBOTTOMLEFT),
        (tauri_runtime::ResizeDirection::West, sys::HTLEFT),
    ] {
        unsafe { sys::SetCapture(window.hwnd()) };
        assert_eq!(unsafe { GetCapture() }, window.hwnd());
        window.dispatcher.start_resize_dragging(direction).unwrap();
        assert_eq!(unsafe { GetCapture() }, 0);
        let mut message = sys::MSG::default();
        // Inspect without dispatching: the test must not enter a modal drag loop.
        assert_ne!(
            unsafe {
                sys::PeekMessageW(
                    &mut message,
                    window.hwnd(),
                    sys::WM_NCLBUTTONDOWN,
                    sys::WM_NCLBUTTONDOWN,
                    sys::PM_REMOVE,
                )
            },
            0
        );
        assert_eq!(message.wParam, expected as usize);
        let mut point = sys::POINT::default();
        unsafe { sys::GetCursorPos(&mut point) };
        assert_eq!(message.lParam as u16, point.x as u16);
        assert_eq!((message.lParam >> 16) as u16, point.y as u16);
    }
    window.dispatcher.set_resizable(false).unwrap();
    window
        .dispatcher
        .start_resize_dragging(tauri_runtime::ResizeDirection::East)
        .unwrap();
    let mut message = sys::MSG::default();
    assert_eq!(
        unsafe {
            sys::PeekMessageW(
                &mut message,
                window.hwnd(),
                sys::WM_NCLBUTTONDOWN,
                sys::WM_NCLBUTTONDOWN,
                sys::PM_REMOVE,
            )
        },
        0
    );
}

fn wait_for<T>(rx: &mpsc::Receiver<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(value) = rx.try_recv() {
            return value;
        }
        assert!(Instant::now() < deadline, "mb132 resize callback timed out");
        unsafe {
            let mut message = sys::MSG::default();
            while sys::PeekMessageW(&mut message, 0, 0, 0, sys::PM_REMOVE) != 0 {
                sys::TranslateMessage(&message);
                sys::DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "loads the bundled mb132 DLL and runs a Win32 message loop"]
fn native_engine_keeps_host_resize_hit_tests_and_viewport() {
    // mb132 binds initialization to its first UI thread. Rust's test harness
    // uses a fresh thread per test, so isolate this from the native IME test.
    const CHILD_ENV: &str = "MB_NATIVE_RESIZE_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "window_frame::tests::native_engine_keeps_host_resize_hit_tests_and_viewport",
                "--include-ignored",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "native resize regression failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let dll_path = mb132::sys::dll_path_from_env().unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mb132-sys/vendor")
            .join(mb132::sys::DLL_NAME)
    });
    mb132::init(mb132::InitOptions {
        dll_path,
        ..Default::default()
    })
    .unwrap();
    for (decorations, shadow) in [(true, true), (false, true), (false, false)] {
        let window = HiddenWindow::new(decorations, shadow, true);
        let url = "http://resize-test.localhost/";
        let mut pending = PendingWebview::new(
            WebviewAttributes::new(WebviewUrl::External(url.parse().unwrap())),
            crate::MbWebviewAttributes,
            "resize-regression",
        )
        .unwrap();
        pending.url = url.into();
        pending.register_uri_scheme_protocol("resize-test", |_, _, respond| {
            respond(
                http::Response::builder()
                    .header("Content-Type", "text/html")
                    .body(std::borrow::Cow::Borrowed(
                        b"<!doctype html><title>Resize regression</title>".as_slice(),
                    ))
                    .unwrap(),
            );
        });
        let (tx, rx) = mpsc::channel();
        pending.on_page_load_handler = Some(Box::new(move |_, _| {
            let _ = tx.send(());
        }));
        let dispatcher =
            crate::webview::create_webview_in_window(&window.dispatcher.inner, pending).unwrap();
        wait_for(&rx);
        let (left, top, width, height) = sys::window_rect(window.hwnd());
        for (x, y, expected) in [
            (left, top, sys::HTTOPLEFT),
            (left + width - 1, top, sys::HTTOPRIGHT),
            (left, top + height - 1, sys::HTBOTTOMLEFT),
            (left + width - 1, top + height - 1, sys::HTBOTTOMRIGHT),
            (left, top + height / 2, sys::HTLEFT),
            (left + width - 1, top + height / 2, sys::HTRIGHT),
            (left + width / 2, top, sys::HTTOP),
            (left + width / 2, top + height - 1, sys::HTBOTTOM),
            (left + width / 2, top + height / 2, sys::HTCLIENT),
        ] {
            assert_eq!(
                window.hit(x, y),
                expected as isize,
                "decorations={decorations} shadow={shadow} point=({x},{y})"
            );
        }
        window
            .dispatcher
            .set_size(Size::Physical(PhysicalSize::new(640, 420)))
            .unwrap();
        assert_eq!(*dispatcher.inner.bounds.lock().unwrap(), (0, 0, 640, 420));
        window.assert_client_size(640, 420);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let (tx, rx) = mpsc::channel();
            dispatcher.inner.webview.eval_with_callback(
                "JSON.stringify([innerWidth, innerHeight, devicePixelRatio])",
                move |value| {
                    let _ = tx.send(value);
                },
            );
            let viewport: Vec<f64> = serde_json::from_str(&wait_for(&rx)).unwrap();
            let dpr = viewport[2];
            if (viewport[0] * dpr - 640.0).abs() <= dpr && (viewport[1] * dpr - 420.0).abs() <= dpr
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "viewport did not resize: {viewport:?}"
            );
        }
    }
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
