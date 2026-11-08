use quadrant_core::ports::Shell;
use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

use crate::mc_mod::InstalledMod;
use crate::tauri_adapter::TauriShell;

use super::general::ModLoader;

host_commands! {
    delete_mod(modpack_name: String, mod_id: String) -> ();
    update_modpack(
        modpack_source: String,
        name: Option<String>,
        version: Option<String>,
        mod_loader: Option<ModLoader>,
    ) -> ();
    create_modpack(name: String, version: String, mod_loader: ModLoader) -> ();
    delete_modpack(name: String) -> ();
    register_mod(mod_: InstalledMod, modpack: String) -> ();
}

#[tauri::command]
pub async fn open_modpacks_folder(app: AppHandle) -> Result<(), tauri::Error> {
    let modpacks_path = app
        .state::<QuadrantHost>()
        .get_modpacks_folder()
        .map_err(crate::command_error)?;
    std::fs::create_dir_all(&modpacks_path).map_err(crate::command_error)?;
    TauriShell::new(app)
        .open_path(&modpacks_path)
        .map_err(crate::command_error)
}

#[tauri::command]
pub fn get_modpacks_folder(app: AppHandle) -> Result<String, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_modpacks_folder()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(crate::command_error)
}
