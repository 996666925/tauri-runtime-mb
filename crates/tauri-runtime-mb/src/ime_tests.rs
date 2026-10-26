//! Regression against the bundled engine, without showing a desktop window.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::sys;

fn wait_for<T>(rx: &mpsc::Receiver<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(value) = rx.try_recv() {
            return value;
        }
        assert!(Instant::now() < deadline, "mb132 callback timed out");
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

/// Engine location: the `MB132_DLL_DIR` directory when set, otherwise the
/// development checkout next to the `mb132-sys` sources.
fn engine_options() -> mb132::InitOptions {
    let dll_path = mb132::sys::dll_path_from_env().unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mb132-sys/vendor")
            .join(mb132::sys::DLL_NAME)
    });

    mb132::InitOptions {
        dll_path,
        ..Default::default()
    }
}

#[test]
#[ignore = "loads the bundled mb132 DLL and runs a Win32 message loop"]
fn native_engine_preedit_commit_cancel_and_caret() {
    eprintln!("IME regression: initializing mb132");
    mb132::init(engine_options()).unwrap();
    eprintln!("IME regression: creating hidden host");
    let class = sys::wide("STATIC");
    let hwnd = unsafe {
        sys::CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            sys::WS_OVERLAPPEDWINDOW,
            0,
            0,
            800,
            600,
            0,
            0,
            sys::GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        )
    };
    assert_ne!(hwnd, 0);
    eprintln!("IME regression: creating view");
    let (tx, rx) = mpsc::channel();
    let view = mb132::WebViewBuilder::new(hwnd)
        .bounds(mb132::Rect::new(0, 0, 800, 600))
        .initial_script(
            r#"
            window.fetch = function (url, options) {
                (window.reports || (window.reports = [])).push(JSON.parse(options.body));
                return Promise.resolve({});
            };
            var attach = Element.prototype.attachShadow;
            Element.prototype.attachShadow = function (options) {
                return window.imeTestRoot = attach.call(this, options);
            };
        "#,
        )
        .initial_script(super::SCRIPT)
        .html(
            r#"<!doctype html><meta charset="utf-8">
            <style>input,textarea { box-sizing: border-box; width:300px;
                padding:10px 12px; border:1px solid; font:20px/1.6 sans-serif;
                color:rgb(230,233,239); background:rgb(13,16,22) }</style>
            <input id="a"><input id="b"><input id="locked" readonly value="locked">
            <textarea id="multi"></textarea><div id="rich" contenteditable></div>"#,
            Some("http://ime-test.localhost/".into()),
        )
        .on_document_ready(move || {
            let _ = tx.send(());
        })
        .on_console(|level, message, _| eprintln!("mb132 console {level}: {message}"))
        .build()
        .unwrap();
    eprintln!("IME regression: waiting for document");
    wait_for(&rx);
    eprintln!("IME regression: running editor checks");
    view.focus();
    let (tx, rx) = mpsc::channel();
    view.eval_with_callback(include_str!("ime_tests.js"), move |value| {
        let _ = tx.send(value);
    });
    let result = wait_for(&rx);
    eprintln!("IME regression result: {result}");
    view.destroy();
    unsafe {
        sys::DestroyWindow(hwnd);
    }
    assert_eq!(result, "ok", "native IME regression: {result}");
}
