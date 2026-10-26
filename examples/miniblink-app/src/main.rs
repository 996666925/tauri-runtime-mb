#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// The static dispatch runtime type: Tauri's event loop message plus the
/// Miniblink runtime.
type Runtime = tauri_runtime_mb::MbRuntime<tauri::EventLoopMessage>;

/// Round-trips a string through the Rust backend to prove IPC works.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("hello {name}, this page runs on Miniblink mb132")
}

/// Returns basic information about the host.
#[tauri::command]
fn engine() -> serde_json::Value {
    serde_json::json!({
        "engine": miniblink::version(),
        "runtime": "tauri-runtime-mb",
    })
}

fn main() {
    tauri::Builder::<Runtime>::new()
        .runtime(tauri_runtime_mb::MbInitAttrs::default())
        .invoke_handler(tauri::generate_handler![greet, engine])
        .run(tauri::generate_context!())
        .expect("failed to run the miniblink runtime");
}
