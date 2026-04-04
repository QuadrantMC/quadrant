use config::init_config;
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
};
use tauri_plugin_cli::CliExt;
use tokio::sync::Mutex;

#[cfg(feature = "updater")]
use tauri::Url;
use tauri_plugin_deep_link::DeepLinkExt;
#[cfg(feature = "updater")]
use tauri_plugin_store::StoreExt;
#[cfg(feature = "updater")]
use tauri_plugin_updater::UpdaterExt;
#[cfg(feature = "quadrant_id")]
use uuid::Uuid;

#[allow(dead_code)] // This is used in the Quadrant ID feature
pub(crate) const QNT_BASE_URL: &str = "https://api.usequadrant.dev/api/v3";

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
    #[cfg(feature = "quadrant_id")]
    pub notification_connection_id: String,
    #[cfg(feature = "quadrant_id")]
    pub notification_state: account::id::NotificationRuntimeState,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(deprecated)]
pub async fn run() {
    colog::init();

    log::info!("Initializing Tauri...");
    let mut builder = tauri::Builder::default().manage(Mutex::new(AppState {
        is_update_enabled: false,
        updated_modpacks: vec![],
        update: None,
        update_bytes: vec![],
        #[cfg(feature = "quadrant_id")]
        notification_connection_id: Uuid::now_v7().to_string(),
        #[cfg(feature = "quadrant_id")]
        notification_state: account::id::NotificationRuntimeState::default(),
    }));

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            log::info!("a new app instance was opened with {argv:?} and the deep link event was already triggered");
            let w = app.get_webview_window("main").expect("no main window");
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
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_oauth::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            init_config(app.handle().clone())?;
            log::info!("Initializing app...\nInitializing config...");
            let mut autoupdate = true;

            log::info!("Initializing deep links and autostart...");
            #[cfg(desktop)]
            {
                match app.deep_link().register_all() {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("Failed to register deep links: {}", e);
                    }
                }

                use tauri_plugin_autostart::MacosLauncher;
                use tauri_plugin_autostart::ManagerExt;

                app.handle()
                    .plugin(tauri_plugin_autostart::init(
                        MacosLauncher::LaunchAgent,
                        Some(vec!["--autostart"]),
                    ))
                    .unwrap();

                // Get the autostart manager
                let autostart_manager = app.autolaunch();
                // Enable autostart
                let _ = autostart_manager.enable();

                // Check enable state
                log::info!(
                    "registered for autostart? {}",
                    autostart_manager.is_enabled().unwrap()
                );
            }

            if cfg!(dev) == false {
                app.emit("disableRightClick", true).unwrap();
                log::info!("Disabling right click...");
            }

            match app.cli().matches() {
                Ok(matches) => {
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
                Err(_) => {
                    log::error!("No matches");
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
                let handle = app.handle().clone();
                log::info!("Initializing telemetry...");
                tauri::async_runtime::spawn(async move {
                    other::telemetry::send_telemetry(handle.clone().to_owned()).await;
                });
            }
            #[cfg(feature = "quadrant_id")]
            {
                log::info!("Starting Quadrant notification and sync workers...");
                let app_handle = app.handle().clone();
                account::id::start_notification_worker(app_handle.clone());
                account::id::start_settings_sync_worker(app_handle);
            }
            log::info!("Initializing tray...");
            let tray = app.tray_by_id("main");
            if tray.is_some() {
                let tray = tray.unwrap();
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
            let _task = tokio::task::spawn(async move {
                let _res = mc_mod::get_versions().await;
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
            modpacks::manage_modpack::open_modpacks_folder,
            modpacks::manage_modpack::register_mod,
            modpacks::general::install_modpack,
            modpacks::general::export_modpack,
            mc_mod::modrinth::get_mod_modrinth,
            mc_mod::search_mods,
            mc_mod::get_versions,
            mc_mod::get_user_url,
            mc_mod::install_mod,
            mc_mod::modrinth::get_mod_owners_modrinth,
            mc_mod::modrinth::get_mod_deps_modrinth,
            mc_mod::check_mod_updates,
            mc_mod::install_remote_file,
            mc_mod::identify_modpack,
            config::init_config,
            config::get_minecraft_folder,
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
        ]);

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

        let update_config = app.store("updateConfig.json")?;

        let ms_store_build = app
            .config()
            .version
            .clone()
            .unwrap_or_default()
            .contains("msstore");

        let defualt_channel = "stable";

        if update_config.get("channel").is_some() {
            let channel = update_config.get("channel").unwrap();
            let channel = channel.as_str().unwrap_or(defualt_channel);
            if channel != "stable" {
                update_urls.push(Url::parse(&format!("https://api.usequadrant.dev/api/any/quadrant/updates/{}/{{{{target}}}}/{{{{arch}}}}/{{{{current_version}}}}",channel))?);
            }
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
                        let progress = downloaded as f64 / content_length.unwrap() as f64;
                        app.emit("updateDownloadProgress", progress).unwrap();
                        log::info!("Downloaded {}%", (progress * 100.0).round() as i32);
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
    Ok(())
}
