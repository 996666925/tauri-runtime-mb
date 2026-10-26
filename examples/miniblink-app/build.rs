use std::path::PathBuf;

fn main() {
    // Required: embeds the application manifest (Common-Controls v6, DPI
    // awareness). Without it the executable fails at load time with
    // `0xC0000139` because `comctl32!TaskDialogIndirect` only exists in v6.
    tauri_build::build();

    copy_engine();
}

/// Copies the engine next to the produced executable so the demo can be started
/// with `cargo run`.
///
/// The engine binary lives in `crates/miniblink-sys/vendor`; a release build
/// ships it beside the executable (`miniblink::default_dll_path` looks there
/// first).
fn copy_engine() {
    let source = miniblink_sys::vendor_dir().join(miniblink_sys::DLL_NAME);
    if !source.exists() {
        println!(
            "cargo:warning=miniblink engine not found at {}",
            source.display()
        );
        return;
    }

    let (Ok(out_dir), Ok(profile)) = (std::env::var("OUT_DIR"), std::env::var("PROFILE")) else {
        return;
    };

    // `OUT_DIR` lives under `<target>/<profile>/build/...` but the intermediate
    // component count varies between cargo versions, so walk up until the
    // profile directory is reached.
    let out_dir = PathBuf::from(out_dir);
    let Some(profile_dir) = out_dir
        .ancestors()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(profile.as_str()))
    else {
        return;
    };

    println!("cargo:rerun-if-changed={}", source.display());
    let _ = std::fs::copy(&source, profile_dir.join(miniblink_sys::DLL_NAME));
}
