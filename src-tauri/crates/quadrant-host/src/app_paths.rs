//! Where each installed app lives, so the other can start it: the `appPaths`
//! setting, `{"quadrant": "...", "quadrantNative": "..."}`, and the startup
//! redirect that follows the `channel` setting to the app it names.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::{Map, Value};

use crate::{
    QuadrantHost, Result,
    updates::{App, CHANNEL_KEY, Channel},
};

pub const KEY: &str = "appPaths";
/// Set on a redirected launch, so it never redirects again: two copies that
/// disagree about the channel can't bounce a launch back and forth.
const REDIRECTED: &str = "QUADRANT_REDIRECTED";

/// The file to start the running app again with: the AppImage rather than
/// the binary mounted from it, which is gone once it quits.
pub fn current_executable() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

/// Remembers `path` as `app`'s. Each install records itself at startup.
///
/// The paths belong to this machine, so, like `hardwareId`, they aren't
/// synced and don't bump `lastSettingsUpdated`. Writes only on a change.
pub fn record(host: &QuadrantHost, app: App, path: &Path) -> Result<()> {
    let mut paths = match host.get_config_value(KEY)? {
        Some(Value::Object(paths)) => paths,
        _ => Map::new(),
    };
    let path = Value::from(path.to_string_lossy());
    if paths.get(app.key()) == Some(&path) {
        return Ok(());
    }
    paths.insert(app.key().to_string(), path);
    host.set_config_value(KEY, Value::Object(paths))
}

/// The app to hand this launch to: the other app, when the channel in
/// `config` names it and it has recorded where it lives.
pub fn redirect_target(config: &Value, running: App) -> Option<PathBuf> {
    let app = Channel::from_config(config.get(CHANNEL_KEY)).app();
    if app == running {
        return None;
    }
    config.get(KEY)?.get(app.key())?.as_str().map(PathBuf::from)
}

/// Starts the other app with `args` and returns `true` when the channel in
/// the data folder at `data_dir` names it and it is installed; the caller
/// then exits before claiming the single instance or opening a window.
/// Reads the config without starting the host, which would write to it.
pub fn redirect(data_dir: &Path, running: App, args: &[OsString]) -> bool {
    if std::env::var_os(REDIRECTED).is_some() {
        return false;
    }
    let Some(config) = fs::read(data_dir.join("config.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
    else {
        return false;
    };
    let Some(target) = redirect_target(&config, running).filter(|path| path.is_file()) else {
        return false;
    };
    match Command::new(&target)
        .args(args)
        .env(REDIRECTED, "1")
        .spawn()
    {
        Ok(_) => {
            log::info!("The channel names {}; started it", target.display());
            true
        }
        Err(error) => {
            log::warn!("Couldn't start {}: {error}", target.display());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::QuadrantHostOptions;

    #[test]
    fn a_channel_of_the_other_app_redirects_to_its_recorded_path() {
        let config = json!({
            "channel": "gpui-preview",
            "appPaths": {"quadrant": "/q", "quadrantNative": "/n"},
        });
        assert_eq!(
            redirect_target(&config, App::Quadrant),
            Some(PathBuf::from("/n"))
        );
        assert_eq!(redirect_target(&config, App::QuadrantNative), None);
        let stable = json!({"appPaths": {"quadrant": "/q"}});
        assert_eq!(
            redirect_target(&stable, App::QuadrantNative),
            Some(PathBuf::from("/q"))
        );
        assert_eq!(redirect_target(&stable, App::Quadrant), None);
    }

    #[test]
    fn nothing_recorded_means_no_redirect() {
        assert_eq!(
            redirect_target(&json!({"channel": "gpui-experimental"}), App::Quadrant),
            None
        );
        assert_eq!(
            redirect_target(
                &json!({"channel": "gpui-preview", "appPaths": {"quadrant": "/q"}}),
                App::Quadrant
            ),
            None
        );
    }

    #[test]
    fn redirect_skips_a_missing_config_or_a_vanished_app() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!redirect(dir.path(), App::Quadrant, &[]));
        fs::write(
            dir.path().join("config.json"),
            json!({
                "channel": "gpui-preview",
                "appPaths": {"quadrantNative": dir.path().join("gone.exe")},
            })
            .to_string(),
        )
        .unwrap();
        assert!(!redirect(dir.path(), App::Quadrant, &[]));
    }

    #[test]
    fn record_keeps_the_other_apps_path_and_leaves_sync_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = QuadrantHostOptions::new(dir.path().join("data"), "", "", "");
        options.mc_folder = Some(dir.path().join("mc"));
        options.keyring_service_name = "quadrant-host-app-paths-test".into();
        let host = QuadrantHost::new(options).unwrap();
        host.init_config().unwrap();
        let stamp = host.get_config_value("lastSettingsUpdated").unwrap();

        record(&host, App::Quadrant, Path::new("/opt/quadrant")).unwrap();
        record(&host, App::QuadrantNative, Path::new("/opt/native")).unwrap();
        record(&host, App::Quadrant, Path::new("/opt/quadrant2")).unwrap();
        assert_eq!(
            host.get_config_value(KEY).unwrap(),
            Some(json!({"quadrant": "/opt/quadrant2", "quadrantNative": "/opt/native"}))
        );
        assert_eq!(host.get_config_value("lastSettingsUpdated").unwrap(), stamp);
    }
}
