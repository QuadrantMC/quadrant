use quadrant_host::{QuadrantHost, QuadrantHostOptions};
use std::path::PathBuf;
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
};
use tauri_plugin_cli::CliExt;
use tokio::sync::Mutex;

#[cfg(feature = "updater")]
use tauri::Url;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tauri_plugin_deep_link::DeepLinkExt;
#[cfg(feature = "updater")]
use tauri_plugin_updater::UpdaterExt;

#[cfg(feature = "quadrant_id")]
pub mod account;

pub mod config;
pub mod mc_mod;
pub mod modpacks;
pub mod other;
pub mod tauri_adapter;

#[cfg(not(any(feature = "wry", feature = "cef")))]
compile_error!(
    "a webview runtime is required: enable either the `wry` (default) or the `cef` feature"
);

/// The webview runtime this build was compiled against.
///
/// Tauri only defaults the `R` type parameter on `AppHandle`, `Context`,
/// `TrayIcon`, … to `Wry` when its `wry` feature is enabled, so a CEF-only
/// build cannot write bare `tauri::AppHandle`. Everything in this crate goes
/// through the aliases below instead, which keeps both runtimes buildable from
/// one set of command signatures.
#[cfg(feature = "cef")]
pub type TauriRuntime = tauri::Cef;
#[cfg(all(feature = "wry", not(feature = "cef")))]
pub type TauriRuntime = tauri::Wry;

/// `tauri::AppHandle` bound to [`TauriRuntime`]. Import this — not
/// `tauri::AppHandle` — in command signatures.
pub type AppHandle = tauri::AppHandle<TauriRuntime>;

#[derive(Clone)]
pub struct AppState {
    pub updated_modpacks: Vec<String>,
    pub is_update_enabled: bool,
    pub update: Option<tauri_plugin_updater::Update>,
    pub update_bytes: Vec<u8>,
}

/// Decides whether the built-in auto-updater should run for this launch.
///
/// The updater is on by default and turned off by `--noupdater` (used by the
/// AUR/flatpak packaging, where the package manager owns updates) or by an
/// msstore build, which ships through the Store instead. `matches` is `None`
/// when the CLI args failed to parse; that must not be read as "the flag was
/// absent", but there is nothing better to fall back to than the default.
fn resolve_autoupdate(matches: Option<&tauri_plugin_cli::Matches>, version: &str) -> bool {
    if version.contains("msstore") {
        return false;
    }
    let disabled = matches
        .and_then(|matches| matches.args.get("noupdater"))
        .map(|arg| arg.value == true)
        .unwrap_or(false);
    !disabled
}

/// The directory Flatpak shares between the sandbox and the host, if we're
/// running inside one. Returns `None` everywhere else, including a normal
/// Linux install, so callers don't need to guard on the platform.
///
/// Flatpak bind-mounts `$XDG_RUNTIME_DIR/app/$FLATPAK_ID` through to the host
/// at the same path; everything else under `$XDG_RUNTIME_DIR` is private to
/// the sandbox.
fn flatpak_host_shared_dir() -> Option<PathBuf> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let flatpak_id = std::env::var_os("FLATPAK_ID")?;
    Some(PathBuf::from(runtime_dir).join("app").join(flatpak_id))
}

/// Works around a blank tray icon under Flatpak.
///
/// `tray-icon` hands libappindicator a path on disk rather than pixels, and it
/// writes that file to `$XDG_RUNTIME_DIR/tray-icon`. Inside a Flatpak sandbox
/// that directory is invisible to the host, so the StatusNotifierHost creates
/// the tray slot but has no icon to draw. Pointing the crate at the shared
/// directory instead puts the file somewhere the host can actually read.
///
/// The path is only consulted the next time the icon is written, so the icon
/// has to be re-applied afterwards — the tray was already built from the
/// config by the time `setup` runs.
fn try_redirect_tray_icon(
    tray: &tauri::tray::TrayIcon<TauriRuntime>,
    image: Option<&tauri::image::Image<'static>>,
    shared_dir: &std::path::Path,
) -> Result<(), anyhow::Error> {
    std::fs::create_dir_all(shared_dir)?;
    tray.set_temp_dir_path(Some(shared_dir))?;
    let image = image
        .ok_or_else(|| anyhow::anyhow!("no tray icon is embedded in the app context"))?
        .clone();
    tray.set_icon(Some(image))?;
    Ok(())
}

fn redirect_tray_icon_for_flatpak(
    tray: &tauri::tray::TrayIcon<TauriRuntime>,
    image: Option<&tauri::image::Image<'static>>,
) {
    let Some(shared_dir) = flatpak_host_shared_dir() else {
        return;
    };
    match try_redirect_tray_icon(tray, image, &shared_dir) {
        Ok(()) => log::info!("Tray icon redirected to {shared_dir:?} for Flatpak."),
        Err(error) => log::error!(
            "Failed to redirect the tray icon to {shared_dir:?}; it will likely render blank under Flatpak: {error}"
        ),
    }
}

fn build_quadrant_host(
    app: &AppHandle,
    api_base_url: Option<String>,
) -> Result<QuadrantHost, anyhow::Error> {
    let data_dir = app
        .path()
        .app_data_dir()
        .ok()
        .or_else(|| quadrant_core::config::get_config_dir().ok().flatten())
        .unwrap_or_else(|| PathBuf::from("."));
    let mut options = QuadrantHostOptions::new(
        data_dir,
        env!("QUADRANT_OAUTH2_CLIENT_ID"),
        env!("QUADRANT_OAUTH2_CLIENT_SECRET"),
        env!("QUADRANT_API_KEY"),
    );
    options.api_base_url = api_base_url.or_else(|| std::env::var("QUADRANT_API_BASE_URL").ok());
    options.app_version = app.package_info().version.to_string();
    options.os_name = tauri_plugin_os::platform().to_string().to_uppercase();
    QuadrantHost::new(options)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(deprecated)]
pub async fn run() {
    log::info!("Initializing Tauri...");
    let context = tauri::generate_context!();
    // The tray image is embedded in the context at build time and the tray is
    // built before `setup` runs, so keep a copy here — the Flatpak workaround
    // in `setup` has to re-apply it to move the file it writes.
    let tray_image = context.tray_icon().map(|image| image.clone().to_owned());
    let mut builder = tauri::Builder::<TauriRuntime>::new().manage(Mutex::new(AppState {
        updated_modpacks: vec![],
        is_update_enabled: false,
        update: None,
        update_bytes: vec![],
    }));

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            log::info!("a new app instance was opened with {argv:?} and the deep link event was already triggered");
            let w = app.get_webview_window("main").expect("no main window");
            if let Err(e) = w.set_enabled(true) {
                log::error!("Failed to enable window: {e}");
            }
            match w.show() {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Failed to show window: {}", e);
                }
            }
            match w.set_focus() {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Failed to focus window: {}", e);
                }
            }
            match w.unminimize() {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Failed to unminimize window: {}", e);
                }
            }
            match w.center() {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Failed to center window: {}", e);
                }
            }
        }));
    }

    builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_persisted_scope::init())
        .plugin(tauri_plugin_cli::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_oauth::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            // Don't silently discard a parse failure: if the CLI args fail to
            // parse, flags like `--noupdater` are lost, so log loudly rather
            // than falling back to the defaults without a trace.
            let matches = match app.cli().matches() {
                Ok(matches) => Some(matches),
                Err(error) => {
                    log::error!("Failed to parse CLI arguments: {error}");
                    None
                }
            };
            let api_base_url = matches
                .as_ref()
                .and_then(|m| m.args.get("api-url"))
                .and_then(|a| a.value.as_str().map(String::from));
            let host = build_quadrant_host(&app.handle().clone(), api_base_url)?;
            let mut host_events = host.subscribe_events();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match host_events.recv().await {
                        Ok(event) => {
                            let _ = app_handle.emit(&event.event, event.payload.clone());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            log::warn!("Dropped {skipped} host events while forwarding to Tauri");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            app.manage(host.clone());
            log::info!("Initializing app...\nInitializing config...");
            host.init_config()?;

            // One-time migration: the update channel used to live in a
            // separate updateConfig.json store. Fold it into the single config
            // store so the app has exactly one source of truth for settings.
            if host.get_config_value("channel").ok().flatten().is_none() {
                if let Ok(data_dir) = app.path().app_data_dir() {
                    let legacy_path = data_dir.join("updateConfig.json");
                    if let Ok(contents) = std::fs::read_to_string(&legacy_path) {
                        if let Some(channel) = serde_json::from_str::<serde_json::Value>(&contents)
                            .ok()
                            .and_then(|json| json.get("channel").cloned())
                        {
                            if let Err(e) = host.set_config_value("channel", channel) {
                                log::warn!("Failed to migrate update channel: {e}");
                            }
                        }
                    }
                }
            }

            // Apply the persisted native-decorations preference before the
            // window is shown so the app launches with the correct frame
            // instead of relying on the renderer to flip it at runtime. Read
            // it from the host config store — the single source of truth that
            // `get_config_value`/`set_config_value` and the UI all go through.
            if let Some(window) = app.get_webview_window("main") {
                let native_decorations = host
                    .get_config_value("nativeDecorations")
                    .ok()
                    .flatten()
                    .and_then(|value| value.as_bool())
                    .unwrap_or(cfg!(target_os = "macos"));
                if let Err(e) = window.set_decorations(native_decorations) {
                    log::error!("Failed to apply native decorations preference: {e}");
                }
            }

            log::info!("Initializing deep links and autostart...");
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                match app.deep_link().register_all() {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("Failed to register deep links: {}", e);
                    }
                }
            }

            #[cfg(desktop)]
            {
                use tauri_plugin_autostart::MacosLauncher;
                use tauri_plugin_autostart::ManagerExt;

                app.handle().plugin(tauri_plugin_autostart::init(
                    MacosLauncher::LaunchAgent,
                    Some(vec!["--autostart"]),
                ))?;

                // Get the autostart manager
                let autostart_manager = app.autolaunch();
                match autostart_manager.is_enabled() {
                    Ok(enabled) => log::info!("registered for autostart? {enabled}"),
                    Err(error) => log::warn!("Failed to query autostart state: {error}"),
                }
            }

            if cfg!(dev) == false {
                app.emit("disableRightClick", true).unwrap();
                log::info!("Disabling right click...");
            }

            if let Some(ref matches) = matches {
                log::info!("Matches: {:?}", matches);
                let autostart = matches.args.get_key_value("autostart");
                if autostart.is_some() {
                    let autostart = autostart.unwrap();
                    if autostart.1.value == true {
                        log::info!("Autostarting");
                        for window in app.webview_windows() {
                            match window.1.hide() {
                                Ok(_) => {}
                                Err(e) => {
                                    log::error!("Failed to hide window: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            let handle = app.handle().clone();
            let version = app.config().version.clone().unwrap_or_default();
            let autoupdate = resolve_autoupdate(matches.as_ref(), &version);
            log::info!("Autoupdate enabled: {}\nInitializing state...", autoupdate);

            if let Ok(mut state) = app.state::<Mutex<AppState>>().try_lock() {
                state.is_update_enabled = autoupdate;
            } else {
                log::warn!("Failed to lock AppState during setup to set autoupdate.");
            }
            if autoupdate {
                log::info!("Checking for updates...");
                tauri::async_runtime::spawn(async move {
                    let res = check_update(handle).await;
                    if res.is_err() {
                        log::error!("Failed to update, {}", res.err().unwrap());
                        return;
                    }
                    res.unwrap();
                });
            }
            #[cfg(feature = "telemetry")]
            {
                log::info!("Initializing telemetry...");
                let host = host.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = host.send_telemetry().await;
                });
            }
            #[cfg(feature = "quadrant_id")]
            {
                log::info!("Starting Quadrant notification and sync workers...");
                let worker_host = host.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = worker_host.start_background_workers().await {
                        log::error!("Failed to start Quadrant host workers: {error}");
                    }
                });
            }
            log::info!("Initializing tray...");
            let tray = app.tray_by_id("main");
            if tray.is_some() {
                let tray = tray.unwrap();
                // On macOS the tray title renders as text beside the menu bar
                // icon; keep only the icon there. Other platforms use it as a
                // tooltip/label, so keep the app name.
                #[cfg(target_os = "macos")]
                tray.set_title(None::<&str>)?;
                #[cfg(not(target_os = "macos"))]
                tray.set_title(Some("Quadrant"))?;
                redirect_tray_icon_for_flatpak(&tray, tray_image.as_ref());
                let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let show_w = MenuItem::with_id(app, "show", "Show/Hide", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_w, &quit_i])?;

                tray.set_menu(Some(menu))?;
                tray.set_show_menu_on_left_click(false)?;
                tray.on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                        && let Some(window) = tray.app_handle().get_webview_window("main")
                    {
                        window.set_enabled(true).unwrap();

                        window.show().unwrap();
                        window.set_focus().unwrap();
                        window.unminimize().unwrap();
                    }
                });
                tray.on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            window.set_enabled(true).unwrap();
                            if window.is_visible().unwrap() {
                                window.hide().unwrap();
                            } else {
                                window.show().unwrap();
                                window.set_focus().unwrap();
                                window.unminimize().unwrap();
                            }
                        }
                    }
                    _ => {}
                });
            }

            log::info!("Updating Minecraft versions...");
            let version_host = host.clone();
            let _task = tokio::task::spawn(async move {
                let _res = version_host.get_versions().await;
                match _res {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("Failed to update Minecraft versions: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            modpacks::general::get_modpacks,
            modpacks::general::frontend_apply_modpack,
            modpacks::manage_modpack::delete_mod,
            modpacks::manage_modpack::update_modpack,
            modpacks::manage_modpack::create_modpack,
            modpacks::manage_modpack::delete_modpack,
            modpacks::manage_modpack::get_modpacks_folder,
            modpacks::manage_modpack::open_modpacks_folder,
            modpacks::manage_modpack::register_mod,
            modpacks::general::install_modpack,
            modpacks::general::export_modpack,
            modpacks::general::export_modpack_to,
            mc_mod::modrinth::get_mod_modrinth,
            mc_mod::search_mods,
            mc_mod::get_categories,
            mc_mod::get_versions,
            mc_mod::get_user_url,
            mc_mod::install_mod,
            mc_mod::modrinth::get_mod_owners_modrinth,
            mc_mod::modrinth::get_mod_deps_modrinth,
            mc_mod::check_mod_updates,
            mc_mod::install_remote_file,
            mc_mod::identify_modpack,
            config::init_config,
            config::get_config_value,
            config::set_config_value,
            config::get_minecraft_folder,
            config::get_default_minecraft_folder,
            other::open_link,
            modpacks::general::set_modpack_sync_date,
            request_check_for_updates,
            is_autoupdate_enabled,
            install_update,
            other::rss::get_news,
            #[cfg(feature = "curseforge")]
            mc_mod::curseforge::get_mod_curseforge,
            #[cfg(feature = "curseforge")]
            mc_mod::curseforge::get_mod_owners_curseforge,
            #[cfg(feature = "curseforge")]
            mc_mod::curseforge::get_mod_deps_curseforge,
            #[cfg(feature = "telemetry")]
            other::telemetry::send_telemetry,
            #[cfg(feature = "telemetry")]
            other::telemetry::remove_telemetry,
            #[cfg(feature = "quadrant_id")]
            account::set_secret,
            #[cfg(feature = "quadrant_id")]
            account::clear_account_token,
            #[cfg(feature = "quadrant_id")]
            account::id::get_account_info,
            #[cfg(feature = "quadrant_id")]
            account::id::oauth2_login,
            #[cfg(feature = "quadrant_id")]
            account::id::oauth2_client_id,
            #[cfg(feature = "quadrant_id")]
            account::id::read_notification,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_share::share_modpack,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_share::share_modpack_raw,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_sync::get_synced_modpacks,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_sync::kick_member,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_sync::invite_member,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_sync::sync_modpack,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_sync::delete_synced_modpack,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_sync::answer_invite,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_share::get_quadrant_share_modpack,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_settings_sync::get_quadrant_settings,
            #[cfg(feature = "quadrant_id")]
            account::quadrant_settings_sync::submit_quadrant_settings,
        ])
        .on_window_event(|window, event| {
            // The custom titlebar close button hides the window to the tray
            // instead of quitting. With native decorations the OS-drawn close
            // button issues a real close, so intercept it here and mirror that
            // hide-to-tray behavior. Quitting stays exclusive to the tray's
            // "Quit" item (app.exit), which does not emit this event.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                let _ = window.hide();
                // Match the custom button, which disables the hidden window on
                // Windows to keep it out of taskbar reactivation. The tray
                // re-enables it on show.
                #[cfg(target_os = "windows")]
                let _ = window.set_enabled(false);
            }
        });

    builder
        .run(context)
        .expect("error while running tauri application");
}

#[tauri::command]
async fn request_check_for_updates(app: AppHandle) -> Result<(), tauri::Error> {
    // Gate every caller, not just the startup check: the renderer asks for an
    // update check on its own, so `--noupdater` (and msstore builds) have to be
    // honoured here too.
    {
        let state = app.state::<Mutex<AppState>>();
        let state = state.lock().await;
        if !state.is_update_enabled {
            log::info!("Skipping update check: the updater is disabled.");
            return Ok(());
        }
    }
    check_update(app).await.map_err(tauri::Error::from)
}

#[tauri::command]
async fn is_autoupdate_enabled(app: AppHandle) -> Result<bool, tauri::Error> {
    let state = app.state::<Mutex<AppState>>();
    let state = state.lock().await;
    Ok(state.is_update_enabled)
}
async fn check_update(_app: AppHandle) -> Result<(), anyhow::Error> {
    #[cfg(feature = "updater")]
    {
        let app = _app;
        let update_url = Url::parse(
            "https://api.usequadrant.dev/api/any/quadrant/updates/stable/{{target}}/{{arch}}/{{current_version}}?variant={{bundle_type}}",
        )?;

        let mut update_urls = vec![update_url];

        let ms_store_build = app
            .config()
            .version
            .clone()
            .unwrap_or_default()
            .contains("msstore");

        let defualt_channel = "stable";

        let channel = app
            .state::<QuadrantHost>()
            .get_config_value("channel")
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| defualt_channel.to_string());
        if channel != "stable" {
            update_urls.push(Url::parse(&format!("https://api.usequadrant.dev/api/any/quadrant/updates/{}/{{{{target}}}}/{{{{arch}}}}/{{{{current_version}}}}?variant={{{{bundle_type}}}}",channel))?);
        }
        // Prefer the preview version if we're updating from a preview version
        update_urls.reverse();

        log::debug!(
            "Update URLs: {:?}",
            update_urls
                .iter()
                .map(|url| url.as_str())
                .collect::<Vec<_>>()
        );
        log::info!("Checking for updates...");

        let updater = app
            .updater_builder()
            .endpoints(update_urls)?
            .version_comparator(|current, update| update.version != current)
            .header("User-Agent", quadrant_core::mc_mod::get_user_agent())?;

        if ms_store_build {
            return Ok(());
        }

        let updater = updater.build()?;

        if let Some(update) = updater.check().await? {
            let mut downloaded = 0;

            // alternatively we could also call update.download() and update.install() separately
            let downloaded_update = update
                .download(
                    |chunk_length, content_length| {
                        downloaded += chunk_length;
                        if let Some(content_length) = content_length.filter(|length| *length > 0) {
                            let progress = downloaded as f64 / content_length as f64;
                            if let Err(error) = app.emit("updateDownloadProgress", progress) {
                                log::warn!("Failed to emit update progress: {error}");
                            }
                            log::info!("Downloaded {}%", (progress * 100.0).round() as i32);
                        }
                    },
                    || {
                        log::info!("Download finished");
                    },
                )
                .await?;

            log::info!("Update downloaded");
            app.emit("updateDownloadProgress", 1).unwrap();
            let state = app.state::<Mutex<AppState>>();
            let mut state = state.lock().await;
            state.update = Some(update);
            state.update_bytes = downloaded_update;
        }
    }
    Ok(())
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), tauri::Error> {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    let update = state.update.take();
    if update.is_none() {
        return Err(anyhow::anyhow!("No update available").into());
    }
    let update = update.unwrap();
    let update_bytes = state.update_bytes.clone();
    update
        .install(update_bytes)
        .map_err(|e| tauri::Error::from(anyhow::Error::from(e)))?;
    // On Windows the NSIS `passive` installer shuts down and relaunches the app
    // itself. On macOS/Linux `install()` swaps the bundle in place and returns
    // without restarting, so we must relaunch explicitly.
    #[cfg(not(target_os = "windows"))]
    app.restart();
    #[cfg(target_os = "windows")]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tauri_plugin_cli::{ArgData, Matches};

    fn matches_with(name: &str, value: Value) -> Matches {
        let mut matches = Matches::default();
        let mut arg = ArgData::default();
        arg.value = value;
        arg.occurrences = 1;
        matches.args.insert(name.to_string(), arg);
        matches
    }

    /// The plugin reports every declared flag, using `Bool(false)` for the ones
    /// that weren't passed — so "present in the map" must not mean "disabled".
    fn matches_without_noupdater() -> Matches {
        matches_with("noupdater", Value::Bool(false))
    }

    #[test]
    fn autoupdate_is_enabled_by_default() {
        assert!(resolve_autoupdate(
            Some(&matches_without_noupdater()),
            "26.7.8-stable"
        ));
        assert!(resolve_autoupdate(None, "26.7.8-stable"));
    }

    #[test]
    fn noupdater_flag_disables_autoupdate() {
        let matches = matches_with("noupdater", Value::Bool(true));
        assert!(!resolve_autoupdate(Some(&matches), "26.7.8-stable"));
    }

    #[test]
    fn msstore_builds_never_autoupdate() {
        assert!(!resolve_autoupdate(
            Some(&matches_without_noupdater()),
            "26.7.8-msstore"
        ));
        assert!(!resolve_autoupdate(None, "26.7.8-msstore"));
    }

    fn cli_args() -> Vec<Value> {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json should be valid JSON");
        config["plugins"]["cli"]["args"]
            .as_array()
            .expect("the CLI plugin should declare args")
            .clone()
    }

    fn cli_arg(name: &str) -> Value {
        cli_args()
            .into_iter()
            .find(|arg| arg["name"] == name)
            .unwrap_or_else(|| panic!("`{name}` should be declared in the CLI config"))
    }

    /// `--noupdater` and `--autostart` are boolean flags: giving them
    /// `takesValue` would make clap demand an argument and reject the bare form
    /// the desktop entries use.
    #[test]
    fn boolean_flags_do_not_take_a_value() {
        for name in ["noupdater", "autostart"] {
            assert_ne!(cli_arg(name)["takesValue"], Value::Bool(true));
        }
    }

    /// Without `takesValue`, `--api-url https://…` makes clap reject the value,
    /// which drops the whole match set — taking `--noupdater` down with it.
    #[test]
    fn api_url_takes_a_value() {
        assert_eq!(cli_arg("api-url")["takesValue"], Value::Bool(true));
    }

    /// Deep links and file associations launch the app with a positional URL
    /// (`Exec=quadrant --noupdater %u`). Without a positional arg to absorb it,
    /// clap errors out and every flag on that command line is lost.
    #[test]
    fn a_positional_arg_absorbs_deep_link_urls() {
        let positional = cli_args()
            .into_iter()
            .find(|arg| arg["index"].is_number())
            .expect("the CLI config should declare a positional arg for deep links");
        assert_eq!(positional["index"], Value::from(1));
        assert_eq!(positional["takesValue"], Value::Bool(true));
        assert_eq!(positional["multiple"], Value::Bool(true));
    }
}

#[cfg(test)]
mod icon_tests {
    use serde_json::Value;
    use std::path::{Path, PathBuf};

    fn manifest_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn icons_in(config: &str) -> Vec<String> {
        let config: Value = serde_json::from_str(config).expect("config should be valid JSON");
        config["bundle"]["icon"]
            .as_array()
            .expect("the config should declare bundle.icon")
            .iter()
            .map(|icon| {
                icon.as_str()
                    .expect("icon entries should be strings")
                    .to_string()
            })
            .collect()
    }

    fn base_icons() -> Vec<String> {
        icons_in(include_str!("../tauri.conf.json"))
    }

    fn linux_icons() -> Vec<String> {
        icons_in(include_str!("../tauri.linux.conf.json"))
    }

    /// Config merging is JSON merge-patch, so the Linux array replaces the base
    /// one wholesale. That's the point: none of the dark-plate icons should
    /// reach hicolor on Linux.
    #[test]
    fn linux_icons_are_all_linux_specific() {
        for icon in linux_icons() {
            assert!(
                icon == "icons/logoNoBgLinux.svg" || icon.starts_with("icons/linux/"),
                "`{icon}` is not a Linux-specific icon"
            );
        }
        assert!(linux_icons().contains(&"icons/logoNoBgLinux.svg".to_string()));
    }

    /// The base config must keep the plated logo, so Windows and macOS are
    /// untouched by the Linux swap.
    #[test]
    fn base_config_still_uses_the_plated_logo() {
        let icons = base_icons();
        assert!(icons.contains(&"icons/logo.svg".to_string()));
        assert!(!icons.iter().any(|icon| icon.contains("logoNoBgLinux")));
        assert!(!icons.iter().any(|icon| icon.starts_with("icons/linux/")));
    }

    /// A typo in an icon path fails late and confusingly, inside the bundler.
    #[test]
    fn every_declared_icon_exists() {
        let root = manifest_dir();
        for icon in base_icons().into_iter().chain(linux_icons()) {
            assert!(
                root.join(&icon).exists(),
                "declared icon `{icon}` does not exist"
            );
        }
    }

    /// `tauri-codegen` picks the first `.png` in the list as the Unix window
    /// icon, so the Linux list has to contain one.
    #[test]
    fn the_linux_list_has_a_window_icon() {
        let first_png = linux_icons()
            .into_iter()
            .find(|icon| icon.ends_with(".png"))
            .expect("the Linux icon list needs a PNG for the window icon");
        assert_eq!(first_png, "icons/linux/512x512.png");
    }

    fn png_size(path: &Path) -> (u32, u32) {
        let bytes = std::fs::read(path).expect("icon should be readable");
        assert_eq!(&bytes[1..4], b"PNG", "{path:?} is not a PNG");
        let dimension = |offset: usize| {
            u32::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        (dimension(16), dimension(20))
    }

    /// The deb/rpm bundler derives each hicolor directory from the PNG's real
    /// dimensions, not its filename, so a mislabelled file lands in the wrong
    /// theme directory and silently never gets used.
    #[test]
    fn linux_png_dimensions_match_their_names() {
        let root = manifest_dir();
        let expected = [
            ("icons/linux/32x32.png", 32),
            ("icons/linux/128x128.png", 128),
            ("icons/linux/128x128@2x.png", 256),
            ("icons/linux/512x512.png", 512),
        ];
        for (icon, size) in expected {
            assert_eq!(png_size(&root.join(icon)), (size, size), "{icon}");
        }
        assert_eq!(
            linux_icons()
                .into_iter()
                .filter(|icon| icon.ends_with(".png"))
                .count(),
            expected.len(),
            "every Linux PNG should have a pinned size"
        );
    }
}
