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
/// A release build ships the engine beside the executable
/// (`mb132::default_dll_path` looks there). The source is taken from the
/// `MB132_DLL_DIR` environment directory when set, otherwise from the
/// development checkout in `crates/mb132-sys/vendor`.
fn copy_engine() {
    let source = engine_source();
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
    let _ = std::fs::copy(&source, profile_dir.join(mb132_sys::DLL_NAME));
}

/// Absolute path of the engine binary the demo should ship.
fn engine_source() -> PathBuf {
    println!(
        "cargo:rerun-if-env-changed={}",
        mb132_sys::DLL_DIR_ENV
    );

    mb132_sys::dll_path_from_env().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/mb132-sys/vendor")
            .join(mb132_sys::DLL_NAME)
    })
}
