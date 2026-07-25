use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::anyhow;
use keyring_core::{Entry, Error as KeyringError, set_default_store};
use quadrant_core::{
    Result,
    events::BackendEvent,
    ports::{EventSink, Notifier, RuntimeState, SecretStore, Shell},
};
use serde_json::Value;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

use crate::{AppHandle, AppState};

static KEYRING_STORE_INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

#[derive(Clone)]
pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriEventSink {
    fn publish(&self, event: BackendEvent) -> Result<()> {
        match event {
            BackendEvent::ModDownloadProgress(payload) => {
                self.app.emit("modDownloadProgress", payload)?;
            }
            BackendEvent::ModInstallProgress(payload) => {
                self.app.emit("modInstallProgress", payload)?;
            }
            BackendEvent::ModpackDownloadProgress(progress) => {
                self.app.emit("modpackDownloadProgress", progress)?;
            }
            BackendEvent::QuadrantExportProgress(progress) => {
                self.app.emit("quadrantExportProgress", progress)?;
            }
            BackendEvent::QuadrantShareSubmission(payload) => {
                self.app.emit("quadrantShareSubmission", payload)?;
            }
            BackendEvent::RefreshNotifications(payload) => {
                self.app.emit("refreshNotifications", payload)?;
            }
            BackendEvent::RecheckAccountToken => {
                self.app.emit("recheckAccountToken", "")?;
            }
        }
        Ok(())
    }
}

pub struct TauriSecretStore;

impl SecretStore for TauriSecretStore {
    fn get_secret(&self, key: &str) -> Result<Option<String>> {
        ensure_keyring_store()?;
        let entry = Entry::new("dev.mrquantumoff.mcmodpackmanager", key)?;
        map_keyring_secret_result(entry.get_password())
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        ensure_keyring_store()?;
        let entry = Entry::new("dev.mrquantumoff.mcmodpackmanager", key)?;
        entry.set_password(value)?;
        Ok(())
    }

    fn delete_secret(&self, key: &str) -> Result<()> {
        ensure_keyring_store()?;
        let entry = Entry::new("dev.mrquantumoff.mcmodpackmanager", key)?;
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn ensure_keyring_store() -> Result<()> {
    match KEYRING_STORE_INIT.get_or_init(|| init_keyring_store().map_err(|error| error.to_string()))
    {
        Ok(()) => Ok(()),
        Err(error) => Err(anyhow!("failed to initialize keyring store: {error}").into()),
    }
}

fn init_keyring_store() -> std::result::Result<(), KeyringError> {
    #[cfg(target_os = "windows")]
    {
        set_default_store(windows_native_keyring_store::Store::new()?);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        set_default_store(apple_native_keyring_store::keychain::Store::new()?);
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        set_default_store(dbus_secret_service_keyring_store::Store::new()?);
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(KeyringError::NotSupportedByStore(
        "no production keyring store is configured for this platform".to_string(),
    ))
}

fn map_keyring_secret_result(
    result: std::result::Result<String, KeyringError>,
) -> Result<Option<String>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone)]
pub struct TauriShell {
    app: AppHandle,
}

impl TauriShell {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Shell for TauriShell {
    fn open_url(&self, url: &str) -> Result<()> {
        open::that_detached(url).map_err(anyhow::Error::from)
    }

    fn open_path(&self, path: &Path) -> Result<()> {
        open::that_detached(path).map_err(anyhow::Error::from)
    }

    fn choose_export_path(&self, suggested_name: &str) -> Result<Option<PathBuf>> {
        let selected = self
            .app
            .dialog()
            .file()
            .add_filter("Quadrant Export", &["quadrantExport.zip"])
            .set_file_name(suggested_name)
            .blocking_save_file();
        Ok(selected.and_then(|file| file.into_path().ok()))
    }
}

#[derive(Clone)]
pub struct TauriNotifier {
    app: AppHandle,
}

impl TauriNotifier {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Notifier for TauriNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<()> {
        self.app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(anyhow::Error::from)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct TauriRuntimeState {
    app: AppHandle,
}

impl TauriRuntimeState {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl RuntimeState for TauriRuntimeState {
    fn get_value(&self, key: &str) -> Result<Option<Value>> {
        let state = self.app.state::<Mutex<AppState>>();
        let state = state
            .try_lock()
            .map_err(|_| anyhow!("runtime state is busy"))?;
        match key {
            "is_update_enabled" => Ok(Some(Value::Bool(state.is_update_enabled))),
            "updated_modpacks" => Ok(Some(serde_json::to_value(&state.updated_modpacks)?)),
            _ => Ok(None),
        }
    }

    fn set_value(&self, key: &str, value: Value) -> Result<()> {
        let state = self.app.state::<Mutex<AppState>>();
        let mut state = state
            .try_lock()
            .map_err(|_| anyhow!("runtime state is busy"))?;
        match key {
            "is_update_enabled" => {
                state.is_update_enabled = value.as_bool().unwrap_or(false);
            }
            "updated_modpacks" => {
                state.updated_modpacks = serde_json::from_value(value)?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::map_keyring_secret_result;
    use keyring_core::Error as KeyringError;

    #[test]
    fn missing_secret_maps_to_none() {
        let result = map_keyring_secret_result(Err(KeyringError::NoEntry)).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn non_missing_secret_errors_are_preserved() {
        let error = map_keyring_secret_result(Err(KeyringError::PlatformFailure(
            std::io::Error::other("backend unavailable").into(),
        )))
        .unwrap_err();
        assert!(error.to_string().contains("backend unavailable"));
    }
}
