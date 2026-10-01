//! Per-provider lookups, so callers pick by `ModSource` once instead of
//! naming `_modrinth` and `_curseforge` host methods everywhere.

use anyhow::Result;
use quadrant_core::{
    error::ErrorCode,
    mc_mod::{GetModArgs, Mod},
    models::{LocalModpack, ModLoader, ModSource},
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

pub async fn get_deps(host: &QuadrantHost, source: &ModSource, id: String) -> Result<Vec<Mod>> {
    match source {
        ModSource::Modrinth => host.get_mod_deps_modrinth(id).await,
        #[cfg(feature = "curseforge")]
        ModSource::CurseForge => host.get_mod_deps_curseforge(id).await,
        #[cfg(not(feature = "curseforge"))]
        ModSource::CurseForge => Err(ErrorCode::CurseforgeDisabled.into()),
        ModSource::Online => Ok(Vec::new()),
    }
}

pub async fn get_owners(
    host: &QuadrantHost,
    source: &ModSource,
    id: String,
) -> Result<Vec<String>> {
    match source {
        ModSource::Modrinth => host.get_mod_owners_modrinth(id).await,
        #[cfg(feature = "curseforge")]
        ModSource::CurseForge => host.get_mod_owners_curseforge(id).await,
        #[cfg(not(feature = "curseforge"))]
        ModSource::CurseForge => Err(ErrorCode::CurseforgeDisabled.into()),
        ModSource::Online => Ok(Vec::new()),
    }
}

/// A mod looked up to install it, as the deep-link handler and install page do.
pub fn install_lookup(id: &str) -> GetModArgs {
    GetModArgs {
        id: id.to_string(),
        downloadable: true,
        show_previous_version: false,
        deletable: false,
        version_target: String::new(),
        mod_loader: ModLoader::Unknown,
        modpack: String::new(),
        selectable: false,
        select_url: None,
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
