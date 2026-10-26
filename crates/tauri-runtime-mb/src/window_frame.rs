//! Native nonclient handling, following the Windows window model used by Tao.
//!
//! Keep the normal window styles so DWM owns the shadow and resize borders.
//! `WM_NCCALCSIZE` removes the title bar instead of turning the window into a
//! popup, while `WM_NCHITTEST` restores the missing client-side resize edges.

use std::sync::atomic::Ordering;

use tauri_runtime::UserEvent;

use crate::sys;
use crate::window::WindowInner;

pub(crate) fn window_style(resizable: bool, maximizable: bool, minimizable: bool) -> u32 {
    let mut style = sys::WS_CAPTION | sys::WS_SYSMENU | sys::WS_CLIPSIBLINGS | sys::WS_CLIPCHILDREN;
    if resizable {
        style |= sys::WS_THICKFRAME;
    }
    if maximizable {
        style |= sys::WS_MAXIMIZEBOX;
    }
    if minimizable {
        style |= sys::WS_MINIMIZEBOX;
    }
    style
}

pub(crate) fn borderless_insets(dpi: u32, shadow: bool) -> sys::RECT {
    if !shadow {
        return sys::RECT::default();
    }
    // Windows 10 draws a complete title bar if any top nonclient area remains.
    // Windows 11 needs a DPI-sized top inset to avoid clipping the first pixels.
    let frame = unsafe {
        sys::GetSystemMetricsForDpi(sys::SM_CXFRAME, dpi)
            + sys::GetSystemMetricsForDpi(sys::SM_CXPADDEDBORDER, dpi)
    };
    sys::RECT {
        left: frame,
        top: if sys::windows_build_number() >= 22000 {
            (dpi as f64 / 96.0).round() as i32
        } else {
            0
        },
        right: frame,
        bottom: frame,
    }
}

pub(crate) fn adjust_rect(
    rect: &mut sys::RECT,
    style: u32,
    ex_style: u32,
    decorations: bool,
    shadow: bool,
    dpi: u32,
) {
    if decorations {
        unsafe { sys::AdjustWindowRectEx(rect, style, 0, ex_style) };
    } else {
        let insets = borderless_insets(dpi, shadow);
        rect.left -= insets.left;
        rect.top -= insets.top;
        rect.right += insets.right;
        rect.bottom += insets.bottom;
    }
}

/// Refresh after installing the window procedure or changing frame attributes.
pub(crate) fn apply<T: UserEvent>(window: &WindowInner<T>) {
    let hwnd = window.hwnd();
    if !window.fullscreen.load(Ordering::SeqCst) {
        let old = unsafe { sys::GetWindowLongPtrW(hwnd, sys::GWL_STYLE) } as u32;
        let frame_bits = sys::WS_OVERLAPPEDWINDOW | sys::WS_POPUP;
        let style = (old & !frame_bits)
            | window_style(
                window.resizable.load(Ordering::SeqCst),
                window.maximizable.load(Ordering::SeqCst),
                window.minimizable.load(Ordering::SeqCst),
            );
        unsafe { sys::SetWindowLongPtrW(hwnd, sys::GWL_STYLE, style as isize) };
    }
    unsafe {
        sys::SetWindowPos(
            hwnd,
            0,
            0,
            0,
            0,
            0,
            sys::SWP_NOMOVE
                | sys::SWP_NOSIZE
                | sys::SWP_NOZORDER
                | sys::SWP_NOACTIVATE
                | sys::SWP_FRAMECHANGED,
        );
    }
}

pub(crate) fn handle_message<T: UserEvent>(
    window: &WindowInner<T>,
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> Option<isize> {
    if !matches!(message, sys::WM_NCCALCSIZE | sys::WM_NCHITTEST) {
        return None;
    }
    let decorations = window.decorations.load(Ordering::SeqCst);
    let fullscreen = window.fullscreen.load(Ordering::SeqCst);
    match message {
        sys::WM_NCCALCSIZE if wparam != 0 && (!decorations || fullscreen) && lparam != 0 => {
            // SAFETY: WM_NCCALCSIZE with nonzero wParam provides NCCALCSIZE_PARAMS.
            let params = unsafe { &mut *(lparam as *mut sys::NCCALCSIZE_PARAMS) };
            if !fullscreen {
                if unsafe { sys::IsZoomed(hwnd) } != 0 {
                    let monitor = unsafe {
                        sys::MonitorFromRect(&params.rgrc[0], sys::MONITOR_DEFAULTTONEAREST)
                    };
                    let mut info = sys::MONITORINFOEXW {
                        cbSize: std::mem::size_of::<sys::MONITORINFOEXW>() as u32,
                        ..Default::default()
                    };
                    if unsafe { sys::GetMonitorInfoW(monitor, &mut info) } != 0 {
                        let mut rect = info.rcWork;
                        // Leave the same one-pixel reveal strip as Tao for auto-hide taskbars.
                        for edge in 0..4 {
                            let mut data = sys::APPBARDATA {
                                cbSize: std::mem::size_of::<sys::APPBARDATA>() as u32,
                                uEdge: edge,
                                ..Default::default()
                            };
                            if unsafe { sys::SHAppBarMessage(sys::ABM_GETAUTOHIDEBAR, &mut data) }
                                != 0
                            {
                                match edge {
                                    0 => rect.left += 1,
                                    1 => rect.top += 1,
                                    2 => rect.right -= 1,
                                    _ => rect.bottom -= 1,
                                }
                            }
                        }
                        params.rgrc[0] = rect;
                    }
                } else {
                    let dpi = unsafe { sys::GetDpiForWindow(hwnd) };
                    let insets = borderless_insets(dpi, window.shadow.load(Ordering::SeqCst));
                    params.rgrc[0].left += insets.left;
                    params.rgrc[0].top += insets.top;
                    params.rgrc[0].right -= insets.right;
                    params.rgrc[0].bottom -= insets.bottom;
                }
            }
            Some(0)
        }
        sys::WM_NCHITTEST
            if !decorations
                && !fullscreen
                && window.resizable.load(Ordering::SeqCst)
                && unsafe { sys::IsZoomed(hwnd) } == 0 =>
        {
            // Both coordinates are signed screen pixels, including on monitors left of the primary.
            let (x, y) = (
                (lparam as u16 as i16) as i32,
                ((lparam >> 16) as u16 as i16) as i32,
            );
            let dpi = unsafe { sys::GetDpiForWindow(hwnd) };
            let border_y = unsafe { sys::GetSystemMetricsForDpi(sys::SM_CYFRAME, dpi) };
            if window.shadow.load(Ordering::SeqCst) {
                let mut point = sys::POINT { x, y };
                let mut rect = sys::RECT::default();
                unsafe {
                    sys::ScreenToClient(hwnd, &mut point);
                    sys::GetClientRect(hwnd, &mut rect);
                }
                if point.y >= 0 && point.y <= border_y && point.x >= 0 && point.x <= rect.right {
                    Some(sys::HTTOP as isize)
                } else {
                    None
                }
            } else {
                let mut rect = sys::RECT::default();
                unsafe { sys::GetWindowRect(hwnd, &mut rect) };
                let border_x = unsafe { sys::GetSystemMetricsForDpi(sys::SM_CXFRAME, dpi) };
                hit_test(&rect, x, y, border_x, border_y)
            }
        }
        _ => None,
    }
}

fn hit_test(rect: &sys::RECT, x: i32, y: i32, border_x: i32, border_y: i32) -> Option<isize> {
    if x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom {
        return None;
    }
    let left = x < rect.left + border_x;
    let right = x >= rect.right - border_x;
    let top = y < rect.top + border_y;
    let bottom = y >= rect.bottom - border_y;
    let hit = match (left, right, top, bottom) {
        (true, _, true, _) => sys::HTTOPLEFT,
        (_, true, true, _) => sys::HTTOPRIGHT,
        (true, _, _, true) => sys::HTBOTTOMLEFT,
        (_, true, _, true) => sys::HTBOTTOMRIGHT,
        (true, _, _, _) => sys::HTLEFT,
        (_, true, _, _) => sys::HTRIGHT,
        (_, _, true, _) => sys::HTTOP,
        (_, _, _, true) => sys::HTBOTTOM,
        _ => return None,
    };
    Some(hit as isize)
}

#[cfg(test)]
#[path = "window_frame_tests.rs"]
mod tests;
