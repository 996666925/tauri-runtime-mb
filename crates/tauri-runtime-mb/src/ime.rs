//! IMM32 bridge for the offscreen editor. The page renders preedit text; the
//! system input method continues to own conversion and the candidate window.

use std::sync::Arc;

use tauri_runtime::UserEvent;

use crate::sys;
use crate::window::WindowInner;

pub(crate) const SCRIPT: &str = include_str!("ime.js");

#[derive(Debug, Clone, Copy)]
pub(crate) struct Caret {
    x: f64,
    y: f64,
    height: f64,
    viewport: f64,
}

impl Caret {
    fn client_rect(self, bounds: (i32, i32, u32, u32), dpr: f64) -> sys::RECT {
        let (x, y, width, _) = bounds;
        // The viewport ratio also accounts for page zoom, unlike the OS
        // window DPI. Use the reported DPR as a resize fallback.
        let scale = if width > 0 {
            width as f64 / self.viewport
        } else {
            dpr
        };
        let left = x.saturating_add((self.x * scale).round() as i32);
        let top = y.saturating_add((self.y * scale).round() as i32);
        let height = (self.height * scale).round().max(1.0) as i32;
        sys::RECT {
            left,
            top,
            right: left.saturating_add(1),
            bottom: top.saturating_add(height),
        }
    }
}

/// Reject malformed reports instead of shifting fields or retaining a stale
/// caret after focus leaves an editable element.
pub(crate) fn parse_report(body: &[u8]) -> Option<(Option<Caret>, f64)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let x = value.get("x")?.as_f64()?;
    let y = value.get("y")?.as_f64()?;
    let height = value.get("height")?.as_f64()?;
    let viewport = value.get("viewport")?.as_f64()?;
    let dpr = value.get("dpr")?.as_f64()?;
    if ![x, y, height, viewport, dpr]
        .into_iter()
        .all(f64::is_finite)
        || height < 0.0
        || viewport <= 0.0
        || dpr <= 0.0
    {
        return None;
    }
    let caret = (height > 0.0).then_some(Caret {
        x,
        y,
        height,
        viewport,
    });
    Some((caret, dpr))
}

pub(crate) fn start<T: UserEvent>(inner: &Arc<WindowInner<T>>, hwnd: isize) {
    if let Some(view) = inner.webview() {
        view.webview
            .eval("window.__mbIme && window.__mbIme.start()");
    }
    position_window(inner, hwnd);
}

pub(crate) fn end<T: UserEvent>(inner: &Arc<WindowInner<T>>) {
    if let Some(view) = inner.webview() {
        view.webview.eval("window.__mbIme && window.__mbIme.end()");
    }
}

pub(crate) fn position_window<T: UserEvent>(inner: &Arc<WindowInner<T>>, hwnd: isize) {
    let caret = *inner.ime_caret.lock().unwrap();
    let view = inner.webview();
    // SAFETY: called on the UI thread for a live host window. Every acquired
    // input context is released before returning.
    unsafe {
        let imc = sys::ImmGetContext(hwnd);
        if imc == 0 {
            return;
        }
        let mut point = sys::POINT::default();
        let area = if let (Some(caret), Some(view)) = (caret, view) {
            caret.client_rect(*view.bounds.lock().unwrap(), view.device_pixel_ratio())
        } else {
            sys::GetCursorPos(&mut point);
            sys::ScreenToClient(hwnd, &mut point);
            let height = (16.0 * inner.scale_factor()).round() as i32;
            sys::RECT {
                left: point.x,
                top: point.y,
                right: point.x.saturating_add(1),
                bottom: point.y.saturating_add(height),
            }
        };
        let form = sys::CANDIDATEFORM {
            index: 0,
            style: sys::CFS_EXCLUDE,
            current_pos: sys::POINT {
                x: area.left,
                y: area.bottom,
            },
            area,
        };
        sys::ImmSetCandidateWindow(imc, &form);
        sys::ImmReleaseContext(hwnd, imc);
    }
}

/// Reads UTF-16 in bytes, including supplementary characters and empty preedit.
/// A failed IMM read must not be interpreted as a cancellation or an empty commit.
unsafe fn read_string(imc: sys::HIMC, index: u32) -> Option<String> {
    let length = unsafe { sys::ImmGetCompositionStringW(imc, index, std::ptr::null_mut(), 0) };
    if length < 0 || length % 2 != 0 {
        return None;
    }
    if length == 0 {
        return Some(String::new());
    }
    let mut buffer = vec![0u16; length as usize / 2];
    let written =
        unsafe { sys::ImmGetCompositionStringW(imc, index, buffer.as_mut_ptr(), length as u32) };
    if written < 0 || written > length || written % 2 != 0 {
        return None;
    }
    buffer.truncate(written as usize / 2);
    Some(String::from_utf16_lossy(&buffer))
}

pub(crate) fn composition<T: UserEvent>(inner: &Arc<WindowInner<T>>, hwnd: isize, flags: u32) {
    let Some(view) = inner.webview() else {
        return;
    };
    // SAFETY: live host window on the UI thread, reads use sized buffers.
    let message = unsafe {
        let imc = sys::ImmGetContext(hwnd);
        if imc == 0 {
            return;
        }
        let result = (flags & sys::GCS_RESULTSTR != 0)
            .then(|| read_string(imc, sys::GCS_RESULTSTR))
            .flatten();
        let text = if flags == 0 {
            Some(String::new())
        } else if flags & (sys::GCS_COMPSTR | sys::GCS_CURSORPOS) != 0 {
            read_string(imc, sys::GCS_COMPSTR)
        } else {
            None
        };
        let cursor =
            sys::ImmGetCompositionStringW(imc, sys::GCS_CURSORPOS, std::ptr::null_mut(), 0).max(0);
        sys::ImmReleaseContext(hwnd, imc);
        serde_json::json!({ "result": result, "text": text, "cursor": cursor })
    };
    view.webview.eval(&format!(
        "window.__mbIme && window.__mbIme.update({message})"
    ));
    position_window(inner, hwnd);
}

#[cfg(test)]
#[path = "ime_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_report_preserves_fractional_coordinates_and_engine_dpr() {
        let (caret, dpr) =
            parse_report(br#"{"x":22.5,"y":31.25,"height":24,"viewport":1024,"dpr":1.75}"#)
                .unwrap();
        let caret = caret.unwrap();
        assert_eq!(
            (caret.x, caret.y, caret.height, caret.viewport, dpr),
            (22.5, 31.25, 24.0, 1024.0, 1.75)
        );
    }

    #[test]
    fn candidate_position_uses_viewport_scale_and_webview_offset() {
        let caret = Caret {
            x: 100.0,
            y: 50.0,
            height: 20.0,
            viewport: 1024.0,
        };
        let rect = caret.client_rect((10, 20, 1792, 1260), 1.75);
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (185, 108, 186, 143)
        );
        // Page zoom may differ from DPR; the actual viewport remains authoritative.
        let rect = caret.client_rect((10, 20, 2048, 1440), 1.75);
        assert_eq!((rect.left, rect.top, rect.bottom), (210, 120, 160));
    }

    #[test]
    fn caret_report_clears_non_editable_focus_and_accepts_scrolled_carets() {
        assert!(
            parse_report(br#"{"x":-1,"y":-1,"height":0,"viewport":1024,"dpr":1}"#)
                .unwrap()
                .0
                .is_none()
        );
        assert!(
            parse_report(br#"{"x":-5,"y":-10,"height":24,"viewport":1024,"dpr":1}"#)
                .unwrap()
                .0
                .is_some()
        );
    }

    #[test]
    fn caret_report_rejects_invalid_fields() {
        for body in [
            r#"{"x":1,"y":2,"height":24,"viewport":0,"dpr":1}"#,
            r#"{"x":1,"y":2,"height":24,"viewport":1024,"dpr":0}"#,
            r#"{"x":1,"y":2,"height":-1,"viewport":1024,"dpr":1}"#,
            r#"{"x":"1","y":2,"height":24,"viewport":1024,"dpr":1}"#,
            r#"{"x":1,"y":2}"#,
        ] {
            assert!(parse_report(body.as_bytes()).is_none(), "{body}");
        }
    }
}
