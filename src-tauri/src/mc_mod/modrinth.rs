use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

use crate::mc_mod::{GetModArgs, Mod};

pub use quadrant_core::mc_mod::modrinth::{ModrinthFile, ModrinthHash};

#[tauri::command]
pub async fn get_mod_modrinth(args: GetModArgs, app: AppHandle) -> Result<Mod, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_mod_modrinth(args)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn get_mod_owners_modrinth(
    id: String,
    app: AppHandle,
) -> Result<Vec<String>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_mod_owners_modrinth(id)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn get_mod_deps_modrinth(id: String, app: AppHandle) -> Result<Vec<Mod>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_mod_deps_modrinth(id)
        .await
        .map_err(tauri::Error::from)
}
