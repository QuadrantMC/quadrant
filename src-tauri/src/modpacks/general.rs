use std::path::{Path, PathBuf};

use quadrant_core::ports::Shell;
use tauri::AppHandle;

pub use quadrant_core::models::{InstalledModpack, LocalModpack, ModLoader, SyncInfo};

use crate::tauri_adapter::{TauriEventSink, TauriSettingsStore, TauriShell, mc_folder};

pub fn get_modpack_path(mc_folder: &Path, modpack: &LocalModpack) -> PathBuf {
    quadrant_core::models::modpack_path(mc_folder, &modpack.name)
}

#[tauri::command]
pub async fn get_modpacks(hide_free: bool, app: AppHandle) -> Vec<LocalModpack> {
    quadrant_core::modpacks::get_modpacks(&mc_folder(&app).unwrap(), hide_free).unwrap_or_default()
}

#[tauri::command]
pub fn frontend_apply_modpack(name: String, app: AppHandle) -> Result<(), tauri::Error> {
    apply_modpack(name, app).map_err(tauri::Error::from)
}

pub fn apply_modpack(name: String, app: AppHandle) -> Result<(), anyhow::Error> {
    quadrant_core::modpacks::apply_modpack(&mc_folder(&app)?, &name)
}

#[tauri::command]
pub async fn install_modpack(
    mod_config: InstalledModpack,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    quadrant_core::modpacks::install_modpack(
        &mc_folder(&app).map_err(tauri::Error::from)?,
        mod_config,
        &TauriSettingsStore::new(app.clone(), "config.json"),
        &TauriEventSink::new(app),
    )
    .await
    .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn set_modpack_sync_date(
    time: u64,
    modpack: String,
    modpack_id: Option<String>,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    quadrant_core::modpacks::set_modpack_sync_date(
        &mc_folder(&app).map_err(tauri::Error::from)?,
        time,
        &modpack,
        modpack_id.as_deref(),
    )
    .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn export_modpack(modpack: String, app: AppHandle) -> Result<(), tauri::Error> {
    let shell = TauriShell::new(app.clone());
    let destination = shell
        .choose_export_path(&format!("{modpack}.quadrantExport.zip"))
        .map_err(tauri::Error::from)?;
    let Some(destination) = destination else {
        return Ok(());
    };

    quadrant_core::modpacks::export_modpack_to(
        &mc_folder(&app).map_err(tauri::Error::from)?,
        &modpack,
        &destination,
        &TauriEventSink::new(app),
    )
    .map_err(tauri::Error::from)
}
