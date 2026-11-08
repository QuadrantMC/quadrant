use quadrant_core::models::ModLoader;

pub use quadrant_core::mc_mod::{
    GetModArgs, GlobalSearchModsArgs, IdentifiedMod, MinecraftVersion, Mod, ModType,
    SearchCategory, SearchModsArgs, UniversalModFile, get_mod_url, get_user_agent,
};
pub use quadrant_core::models::{InstalledMod, ModSource};

pub mod content;
#[cfg(feature = "curseforge")]
pub mod curseforge;
pub mod modrinth;

host_commands! {
    get_versions() -> Vec<MinecraftVersion>;
    check_mod_updates(
        mod_to_update: Mod,
        minecraft_version: String,
        mod_loader: ModLoader,
        modpack_name: String,
    ) -> Option<Mod>;
    search_mods(args: GlobalSearchModsArgs) -> Vec<Mod>;
    get_categories(source: ModSource, mod_type: String) -> Vec<SearchCategory>;
    install_mod(
        id: String,
        minecraft_version: String,
        mod_loader: ModLoader,
        source: ModSource,
        modpack: Option<String>,
        mod_type: ModType,
        file_id: Option<String>,
        content_location: Option<String>,
    ) -> ();
    install_remote_file(
        file: UniversalModFile,
        mod_type: ModType,
        modpack: Option<String>,
        source: ModSource,
        id: String,
    ) -> ();
    identify_modpack(modpack: String) -> Vec<IdentifiedMod>;
}

#[tauri::command]
pub fn get_user_url(username: String, source: ModSource) -> String {
    quadrant_core::mc_mod::get_user_url(username, source)
}
