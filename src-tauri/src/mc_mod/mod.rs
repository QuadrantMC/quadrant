use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

pub use quadrant_core::mc_mod::{
    GetModArgs, GlobalSearchModsArgs, IdentifiedMod, MinecraftVersion, Mod, ModType,
    SearchCategory, SearchModsArgs, UniversalModFile, get_mod_url, get_user_agent,
};
pub use quadrant_core::models::{InstalledMod, ModSource};

#[cfg(feature = "curseforge")]
pub mod curseforge;
pub mod modrinth;

#[tauri::command]
pub async fn get_versions(app: AppHandle) -> Result<Vec<MinecraftVersion>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_versions()
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn check_mod_updates(
    mod_to_update: Mod,
    minecraft_version: String,
    mod_loader: quadrant_core::models::ModLoader,
    modpack_name: String,
    app: AppHandle,
) -> Result<Option<Mod>, tauri::Error> {
    app.state::<QuadrantHost>()
        .check_mod_updates(mod_to_update, minecraft_version, mod_loader, modpack_name)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn search_mods(
    args: GlobalSearchModsArgs,
    app: AppHandle,
) -> Result<Vec<Mod>, tauri::Error> {
    app.state::<QuadrantHost>()
        .search_mods(args)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn get_categories(
    source: ModSource,
    mod_type: String,
    app: AppHandle,
) -> Result<Vec<SearchCategory>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_categories(source, mod_type)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn install_mod(
    id: String,
    minecraft_version: String,
    mod_loader: quadrant_core::models::ModLoader,
    source: ModSource,
    modpack: Option<String>,
    mod_type: ModType,
    file_id: Option<String>,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .install_mod(
            id,
            minecraft_version,
            mod_loader,
            source,
            modpack,
            mod_type,
            file_id,
        )
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn install_remote_file(
    file: UniversalModFile,
    mod_type: ModType,
    modpack: Option<String>,
    source: ModSource,
    id: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .install_remote_file(file, mod_type, modpack, source, id)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn identify_modpack(
    modpack: String,
    app: AppHandle,
) -> Result<Vec<IdentifiedMod>, tauri::Error> {
    app.state::<QuadrantHost>()
        .identify_modpack(modpack)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub fn get_user_url(username: String, source: ModSource) -> String {
    quadrant_core::mc_mod::get_user_url(username, source)
}
