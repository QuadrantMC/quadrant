use crate::AppHandle;
use quadrant_core::ports::Shell;
use quadrant_host::QuadrantHost;
use tauri::Manager;

use crate::tauri_adapter::TauriShell;

use super::general::ModLoader;

#[tauri::command]
pub async fn delete_mod(
    modpack_name: String,
    mod_id: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .delete_mod(modpack_name, mod_id)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn update_modpack(
    modpack_source: String,
    name: Option<String>,
    version: Option<String>,
    mod_loader: Option<ModLoader>,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .update_modpack(modpack_source, name, version, mod_loader)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn create_modpack(
    name: String,
    version: String,
    mod_loader: ModLoader,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .create_modpack(name, version, mod_loader)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn delete_modpack(name: String, app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .delete_modpack(name)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn open_modpacks_folder(app: AppHandle) -> Result<(), tauri::Error> {
    let modpacks_path = app
        .state::<QuadrantHost>()
        .get_modpacks_folder()
        .map_err(tauri::Error::from)?;
    std::fs::create_dir_all(&modpacks_path).map_err(tauri::Error::from)?;
    TauriShell::new(app)
        .open_path(&modpacks_path)
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub fn get_modpacks_folder(app: AppHandle) -> Result<String, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_modpacks_folder()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn register_mod(
    mod_: crate::mc_mod::InstalledMod,
    modpack: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .register_mod(mod_, modpack)
        .await
        .map_err(tauri::Error::from)
}
