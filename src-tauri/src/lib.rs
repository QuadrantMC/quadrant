use quadrant_host::{
    QuadrantHost, QuadrantHostOptions,
    app_paths::{self, Launch},
    single_instance::{self, Claim, Serving},
    updates::App,
};
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
use tauri_plugin_deep_link::DeepLinkExt;
#[cfg(feature = "updater")]
use tauri_plugin_updater::UpdaterExt;

/// Defines the Tauri commands that only pass their arguments to the
/// `QuadrantHost` method of the same name and report its error with a stable
/// code. Commands that do more (blocking work, events, a dialog) are written
/// out where they live.
macro_rules! host_commands {
    ($($name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty;)*) => {$(
        #[tauri::command]
        pub async fn $name($($arg: $ty,)* app: tauri::AppHandle) -> Result<$ret, tauri::Error> {
            tauri::Manager::state::<quadrant_host::QuadrantHost>(&app)
                .$name($($arg),*)
                .await
                .map_err($crate::command_error)
        }
    )*};
}

#[cfg(feature = "quadrant_id")]
pub mod account;

mod app_switch;
pub mod config;
pub mod mc_mod;
pub mod modpacks;
pub mod other;
pub mod tauri_adapter;

pub struct AppState {
    pub is_update_enabled: bool,
    pub update: Option<tauri_plugin_updater::Update>,
    pub update_bytes: Vec<u8>,
    /// A downloaded Quadrant-Native, when the channel names it.
    #[cfg(feature = "updater")]
    pub switch: Option<quadrant_host::updates::Update>,
}

/// This app, of the two sharing the data folder.
const RUNNING: App = App::Quadrant;

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
    tray: &tauri::tray::TrayIcon,
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
    tray: &tauri::tray::TrayIcon,
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

/// Every command error leaves the backend through here, so the frontend
/// receives a stable error code wherever the failure has one.
pub(crate) fn command_error(error: impl Into<anyhow::Error>) -> tauri::Error {
    quadrant_core::error::user_facing(error.into()).into()
}

/// Brings the window up from the tray or a later launch. A window that is
/// closing can refuse any step, which is logged instead of panicking the event
/// loop.
fn show_window(window: &tauri::WebviewWindow) {
    if let Err(e) = window.set_enabled(true) {
        log::error!("Failed to enable window: {e}");
    }
    if let Err(e) = window.show() {
        log::error!("Failed to show window: {e}");
    }
    if let Err(e) = window.unminimize() {
        log::error!("Failed to unminimize window: {e}");
    }
    if let Err(e) = window.set_focus() {
        log::error!("Failed to focus window: {e}");
    }
}

/// Brings the window back for a later launch of either app and opens its
/// links, as the launch would have.
fn reveal(app: &tauri::AppHandle, links: &[String]) {
    // Not the links: a sign-in link carries an authorization code.
    log::info!("A later launch handed over {} link(s)", links.len());
    if let Some(window) = app.get_webview_window("main") {
        show_window(&window);
        if let Err(e) = window.center() {
            log::error!("Failed to center window: {e}");
        }
    }
    for link in links {
        app.deep_link()
            .handle_cli_arguments(["quadrant", link.as_str()].into_iter());
    }
}

// Compile-time credentials are embedded only by the features that call the
// Quadrant backend with them, so `--no-default-features` builds need none.
#[cfg(feature = "quadrant_id")]
const QUADRANT_OAUTH2_CLIENT_ID: &str = env!("QUADRANT_OAUTH2_CLIENT_ID");
#[cfg(not(feature = "quadrant_id"))]
const QUADRANT_OAUTH2_CLIENT_ID: &str = "";
#[cfg(feature = "quadrant_id")]
const QUADRANT_OAUTH2_CLIENT_SECRET: &str = env!("QUADRANT_OAUTH2_CLIENT_SECRET");
#[cfg(not(feature = "quadrant_id"))]
const QUADRANT_OAUTH2_CLIENT_SECRET: &str = "";
// Quadrant Share/Sync and telemetry both authenticate with the API key.
#[cfg(any(feature = "quadrant_id", feature = "telemetry"))]
const QUADRANT_API_KEY: &str = env!("QUADRANT_API_KEY");
#[cfg(not(any(feature = "quadrant_id", feature = "telemetry")))]
const QUADRANT_API_KEY: &str = "";

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
        QUADRANT_OAUTH2_CLIENT_ID,
        QUADRANT_OAUTH2_CLIENT_SECRET,
        QUADRANT_API_KEY,
    );
    options.api_base_url = api_base_url;
    options.app_version = app.package_info().version.to_string();
    options.os_name = tauri_plugin_os::platform().to_string().to_uppercase();
    QuadrantHost::new(options)
}

/// Runs the app. `launch` is this launch's marker, which `main` takes from
/// the environment before any thread starts.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(deprecated)]
pub async fn run(launch: Launch) {
    log::info!("Initializing Tauri...");
    let context = tauri::generate_context!();
    // The tray image is embedded in the context at build time and the tray is
    // built before `setup` runs, so keep a copy here — the Flatpak workaround
    // in `setup` has to re-apply it to move the file it writes.
    let tray_image = context.tray_icon().map(|image| image.clone().to_owned());

    // Before anything opens: the channel may name Quadrant-Native, and
    // either app may already run for this data folder.
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let data_dir = app_switch::data_dir(&context.config().identifier);
    let version = context.config().version.clone().unwrap_or_default();
    if let Some(data_dir) = &data_dir
        && app_switch::may_redirect(
            &args,
            &version,
            tauri::is_dev(),
            flatpak_host_shared_dir().is_some(),
        )
        && app_paths::redirect(data_dir, RUNNING, &args, launch)
    {
        return;
    }
    let handoff = launch == Launch::Handoff;
    let claim =
        data_dir.map(|dir| single_instance::claim(&dir, &app_switch::links(&args), handoff));
    let primary = match claim {
        Some(Ok(Claim::Forwarded)) => return,
        Some(Ok(Claim::Primary(primary))) => Some(primary),
        Some(Err(error)) => {
            log::warn!("Later launches will start instances of their own: {error:#}");
            None
        }
        None => None,
    };

    let builder = tauri::Builder::default()
        .manage(Mutex::new(AppState {
            is_update_enabled: false,
            update: None,
            update_bytes: vec![],
            #[cfg(feature = "updater")]
            switch: None,
        }))
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
            let backfill_host = host.clone();
            tauri::async_runtime::spawn(async move {
                match backfill_host.backfill_mod_hashes().await {
                    Ok(0) => {}
                    Ok(count) => log::info!("Recorded the file hashes of {count} mod(s)"),
                    Err(error) => log::warn!("Failed to record mod file hashes: {error}"),
                }
                if let Err(error) = backfill_host.prune_mod_cache().await {
                    log::warn!("Failed to prune the mod cache: {error}");
                }
            });

            if let Some(primary) = primary {
                let handle = app.handle().clone();
                let serving = primary.serve(move |links| {
                    let app = handle.clone();
                    let shown = handle.run_on_main_thread(move || reveal(&app, &links));
                    if let Err(error) = shown {
                        log::error!("Failed to hand a later launch to the window: {error}");
                    }
                });
                app.manage(serving);
            }
            if app_switch::installed()
                && let Some(path) = app_paths::current_executable()
                && let Err(error) = app_paths::record(&host, RUNNING, &path)
            {
                log::warn!("Failed to record where Quadrant is installed: {error}");
            }

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
                // The matches hold the launch's positional link, which a
                // sign-in redirect fills with an authorization code.
                log::info!("Launched with {} argument(s)", matches.args.len());
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
                        show_window(&window);
                    }
                });
                tray.on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                if let Err(e) = window.hide() {
                                    log::error!("Failed to hide window: {e}");
                                }
                            } else {
                                show_window(&window);
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
            modpacks::prism::get_prism_instances,
            modpacks::prism::get_prism_sync_plans,
            modpacks::prism::apply_modpack_to_prism_instance,
            modpacks::prism::detach_prism_instance,
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
            mc_mod::content::get_installed_content,
            mc_mod::content::open_content_folder,
            mc_mod::content::copy_content,
            mc_mod::content::delete_content,
            config::init_config,
            config::get_config_value,
            config::set_config_value,
            config::remove_config_value,
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
async fn request_check_for_updates(app: tauri::AppHandle) -> Result<(), tauri::Error> {
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
    check_update(app).await.map_err(crate::command_error)
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
        use quadrant_host::updates::{API_BASE, Channel, Platform};

        let app = _app;
        let ms_store_build = app
            .config()
            .version
            .clone()
            .unwrap_or_default()
            .contains("msstore");
        if ms_store_build {
            return Ok(());
        }

        let channel = Channel::read(&app.state::<QuadrantHost>());
        {
            let state = app.state::<Mutex<AppState>>();
            let mut state = state.lock().await;
            state.update = None;
            state.update_bytes.clear();
            state.switch = None;
        }
        // The title bar names what the download is for: an update of this
        // app, or the app the channel switches to.
        let switch_to = (channel.app() != RUNNING).then(|| channel.app().key());
        app.emit("updateSwitch", switch_to)?;
        app.emit("updateDownloadProgress", 0)?;
        if switch_to.is_some() {
            return check_switch(&app, channel).await;
        }

        let update_urls = channel
            .urls(
                API_BASE,
                &Platform {
                    target: "{{target}}",
                    arch: "{{arch}}",
                    version: "{{current_version}}",
                    variant: "{{bundle_type}}",
                },
            )
            .iter()
            .map(|url| Url::parse(url))
            .collect::<Result<Vec<_>, _>>()?;
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
            .version_comparator(|current, update| {
                quadrant_host::updates::is_update(&current.to_string(), &update.version.to_string())
            })
            .header("User-Agent", quadrant_core::mc_mod::get_user_agent())?
            // Per read rather than `timeout`, which would cap the whole
            // installer download.
            .configure_client(|client| {
                client
                    .connect_timeout(std::time::Duration::from_secs(30))
                    .read_timeout(std::time::Duration::from_secs(60))
            })
            .build()?;

        if let Some(update) = updater.check().await? {
            let mut downloaded = 0;

            // alternatively we could also call update.download() and update.install() separately
            let downloaded_update = update
                .download(
                    |chunk_length, content_length| {
                        downloaded += chunk_length;
                        if let Some(content_length) = content_length.filter(|length| *length > 0) {
                            let progress = downloaded as f64 / content_length as f64;
                            emit_download_progress(&app, progress);
                            log::info!("Downloaded {}%", (progress * 100.0).round() as i32);
                        }
                    },
                    || {
                        log::info!("Download finished");
                    },
                )
                .await?;

            log::info!("Update downloaded");
            let state = app.state::<Mutex<AppState>>();
            let mut state = state.lock().await;
            state.update = Some(update);
            state.update_bytes = downloaded_update;
            app.emit("updateDownloadProgress", 1)?;
        }
    }
    Ok(())
}

/// Shows a download's progress, short of 1: the title bar offers to install
/// at 1, which waits until the release is verified and stored.
#[cfg(feature = "updater")]
fn emit_download_progress(app: &tauri::AppHandle, progress: f64) {
    if let Err(error) = app.emit("updateDownloadProgress", progress.min(0.99)) {
        log::warn!("Failed to emit update progress: {error}");
    }
}

/// Downloads Quadrant-Native's newest release on `channel`, for the title
/// bar to offer as a switch.
#[cfg(feature = "updater")]
async fn check_switch(
    app: &tauri::AppHandle,
    channel: quadrant_host::updates::Channel,
) -> Result<(), anyhow::Error> {
    let Some(bundle) = app_switch::bundle() else {
        log::info!(
            "Not an installed copy; can't switch to {}",
            channel.app().name()
        );
        return Ok(());
    };
    let updater = quadrant_host::updates::Updater {
        running: RUNNING,
        current_version: app.package_info().version.to_string(),
        bundle,
        server: quadrant_host::updates::API_BASE.to_string(),
        allow_http: false,
        user_agent: quadrant_core::mc_mod::get_user_agent(),
        pubkey: quadrant_host::updates::PUBKEY.to_string(),
    };
    let Some(update) = updater
        .check(channel, |progress| {
            emit_download_progress(app, progress.into())
        })
        .await?
    else {
        return Ok(());
    };
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    state.switch = Some(update);
    app.emit("updateDownloadProgress", 1)?;
    Ok(())
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), tauri::Error> {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    #[cfg(feature = "updater")]
    if let Some(switch) = state.switch.take() {
        drop(state);
        // Installs Quadrant-Native beside this app and starts it; this one
        // quits so the new one becomes the single instance.
        let serving = app
            .try_state::<Serving>()
            .map(|serving| serving.inner().clone());
        let (switch, result) = tauri::async_runtime::spawn_blocking(move || {
            let result = switch.install(|| release(serving.as_ref()));
            (switch, result)
        })
        .await?;
        if let Err(error) = result {
            app.state::<Mutex<AppState>>().lock().await.switch = Some(switch);
            return Err(crate::command_error(error));
        }
        app.exit(0);
        return Ok(());
    }
    let update = state.update.take();
    if update.is_none() {
        return Err(anyhow::anyhow!("No update available").into());
    }
    let update = update.unwrap();
    let update_bytes = std::mem::take(&mut state.update_bytes);
    update
        .install(update_bytes)
        .map_err(|e| tauri::Error::from(anyhow::Error::from(e)))?;
    // On Windows the NSIS `passive` installer shuts down and relaunches the app
    // itself. On macOS/Linux `install()` swaps the bundle in place and returns
    // without restarting, so we must relaunch explicitly.
    // The relaunched copy claims the single instance socket, so this one
    // lets go of it first.
    #[cfg(not(target_os = "windows"))]
    {
        release(
            app.try_state::<Serving>()
                .as_ref()
                .map(|serving| serving.inner()),
        );
        app.restart();
    }
    #[cfg(target_os = "windows")]
    Ok(())
}

/// Lets go of the single instance socket for the app about to start.
fn release(serving: Option<&Serving>) {
    if let Some(serving) = serving {
        serving.release();
    }
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
