use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

use crate::mc_mod::{GetModArgs, Mod};

pub use quadrant_core::mc_mod::curseforge::ModFile;

#[tauri::command]
pub async fn get_mod_curseforge(args: GetModArgs, app: AppHandle) -> Result<Mod, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_mod_curseforge(args)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn get_mod_owners_curseforge(
    id: String,
    app: AppHandle,
) -> Result<Vec<String>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_mod_owners_curseforge(id)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn get_mod_deps_curseforge(id: String, app: AppHandle) -> Result<Vec<Mod>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_mod_deps_curseforge(id)
        .await
        .map_err(tauri::Error::from)
}
