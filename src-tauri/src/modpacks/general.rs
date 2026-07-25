use std::path::{Path, PathBuf};

use crate::AppHandle;
use quadrant_core::ports::Shell;
use quadrant_host::QuadrantHost;
use tauri::Manager;

pub use quadrant_core::models::{InstalledModpack, LocalModpack, ModLoader, SyncInfo};

use crate::tauri_adapter::TauriShell;

pub fn get_modpack_path(mc_folder: &Path, modpack: &LocalModpack) -> PathBuf {
    quadrant_core::models::modpack_path(mc_folder, &modpack.name)
}

#[tauri::command]
pub async fn get_modpacks(hide_free: bool, app: AppHandle) -> Vec<LocalModpack> {
    app.state::<QuadrantHost>()
        .get_modpacks(hide_free)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub fn frontend_apply_modpack(name: String, app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .frontend_apply_modpack(name)
        .map_err(tauri::Error::from)
}

pub fn apply_modpack(name: String, app: AppHandle) -> Result<(), anyhow::Error> {
    app.state::<QuadrantHost>().frontend_apply_modpack(name)
}

#[tauri::command]
pub async fn install_modpack(
    mod_config: InstalledModpack,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .install_modpack(mod_config)
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
    app.state::<QuadrantHost>()
        .set_modpack_sync_date(time, modpack, modpack_id)
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

    app.state::<QuadrantHost>()
        .export_modpack_to(modpack, destination)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn export_modpack_to(
    modpack: String,
    destination: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .export_modpack_to(modpack, destination.into())
        .await
        .map_err(tauri::Error::from)
}
