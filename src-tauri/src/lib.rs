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

#[derive(Clone)]
pub struct AppState {
    pub updated_modpacks: Vec<String>,
    pub is_update_enabled: bool,
    pub update: Option<tauri_plugin_updater::Update>,
    pub update_bytes: Vec<u8>,
}

fn build_quadrant_host(
    app: &tauri::AppHandle,
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
    let mut builder = tauri::Builder::default().manage(Mutex::new(AppState {
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
        .setup(|app| {
            let matches = app.cli().matches().ok();
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

            let mut autoupdate = true;

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
                let autoupdater_disabled = matches.args.get_key_value("noupdater");
                if autoupdater_disabled.is_some() {
                    let autoupdater_disabled = autoupdater_disabled.unwrap();

                    let value = autoupdater_disabled.1.value == true;

                    autoupdate = !value;
                }
            }
            let handle = app.handle().clone();
            let ms_store_build = app
                .config()
                .version
                .clone()
                .unwrap_or_default()
                .contains("msstore");
            if ms_store_build {
                autoupdate = false;
            }
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn request_check_for_updates(app: tauri::AppHandle) -> Result<(), tauri::Error> {
    check_update(app).await.map_err(tauri::Error::from)
}

#[tauri::command]
async fn is_autoupdate_enabled(app: tauri::AppHandle) -> Result<bool, tauri::Error> {
    let state = app.state::<Mutex<AppState>>();
    let state = state.lock().await;
    Ok(state.is_update_enabled)
}
async fn check_update(_app: tauri::AppHandle) -> Result<(), anyhow::Error> {
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
async fn install_update(app: tauri::AppHandle) -> Result<(), tauri::Error> {
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
