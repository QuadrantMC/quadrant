//! Per-provider lookups, so callers pick by `ModSource` once instead of
//! naming `_modrinth` and `_curseforge` host methods everywhere.

use anyhow::Result;
use quadrant_core::{
    error::ErrorCode,
    mc_mod::{GetModArgs, Mod},
    models::{LocalModpack, ModSource},
};
use quadrant_host::QuadrantHost;

pub async fn get_mod(host: &QuadrantHost, source: &ModSource, args: GetModArgs) -> Result<Mod> {
    match source {
        ModSource::Modrinth => host.get_mod_modrinth(args).await,
        #[cfg(feature = "curseforge")]
        ModSource::CurseForge => host.get_mod_curseforge(args).await,
        #[cfg(not(feature = "curseforge"))]
        ModSource::CurseForge => Err(ErrorCode::CurseforgeDisabled.into()),
        ModSource::Online => Err(ErrorCode::NotFound.into()),
    }
}

/// A mod looked up as an entry of a modpack, as `ModpackView` does.
pub fn modpack_entry(id: &str, modpack: &LocalModpack) -> GetModArgs {
    GetModArgs {
        id: id.to_string(),
        downloadable: false,
        show_previous_version: false,
        deletable: true,
        version_target: String::new(),
        mod_loader: modpack.mod_loader,
        modpack: modpack.name.clone(),
        selectable: false,
        select_url: None,
    }
}
