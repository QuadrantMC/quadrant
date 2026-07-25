// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn maybe_reexec_with_linux_wayland_nvidia_workaround() {
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    if std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_some()
        || !is_wayland_session()
        || !has_nvidia_driver()
    {
        return;
    }

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Failed to resolve current executable: {error}");
            return;
        }
    };
    let args: Vec<_> = std::env::args_os().skip(1).collect();

    let error = Command::new(current_exe)
        .args(args)
        .env("__NV_DISABLE_EXPLICIT_SYNC", "1")
        .exec();
    eprintln!("Failed to relaunch with NVIDIA Wayland workaround: {error}");

    fn is_wayland_session() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || matches!(
                std::env::var("XDG_SESSION_TYPE"),
                Ok(session) if session.eq_ignore_ascii_case("wayland")
            )
    }

    fn has_nvidia_driver() -> bool {
        Path::new("/proc/driver/nvidia/version").exists()
            || matches!(std::env::var("GBM_BACKEND"), Ok(v) if gbm_backend_is_nvidia(&v))
            || matches!(std::env::var("LIBVA_DRIVER_NAME"), Ok(v) if v.eq_ignore_ascii_case("nvidia"))
            || matches!(std::env::var("MESA_LOADER_DRIVER_OVERRIDE"), Ok(v) if v.eq_ignore_ascii_case("nvidia"))
    }

    fn gbm_backend_is_nvidia(value: &str) -> bool {
        value.eq_ignore_ascii_case("nvidia") || value.eq_ignore_ascii_case("nvidia-drm")
    }
}

#[cfg(not(target_os = "linux"))]
fn maybe_reexec_with_linux_wayland_nvidia_workaround() {}

// CEF is multi-process: the same executable is re-launched as the renderer, GPU
// and utility processes, distinguished only by a `--type=` argument. The macro
// inserts that check at the very top of `main` and hands those launches to
// `tauri::run_cef_helper_process()` — before the tokio runtime, the Wayland
// re-exec, or anything else this binary would otherwise do per process.
#[cfg_attr(feature = "cef", tauri::cef_entry_point)]
fn main() {
    maybe_reexec_with_linux_wayland_nvidia_workaround();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");

    runtime.block_on(async {
        quadrant_next_lib::run().await;
    });
}
