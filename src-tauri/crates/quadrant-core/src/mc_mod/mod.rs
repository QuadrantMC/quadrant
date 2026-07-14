//! Mod provider integration, install flows, and mod identification APIs.

use std::{
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use anyhow::anyhow;
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::{
    Result,
    events::{BackendEvent, ModProgressPayload},
    models::{InstalledMod, InstalledModpack, LocalModpack, ModLoader, ModSource},
    ports::{EventSink, SettingsStore},
};

use cache::{add_cache_index, file_hash, get_cache_index, init_cache};
#[cfg(feature = "curseforge")]
use curseforge::{
    download_mod_curseforge, get_latest_mod_version_curseforge, search_mods_curseforge,
};
use http::{provider_cached_client, provider_http_client};
use modrinth::{download_mod_modrinth, get_latest_mod_version_modrinth, search_mods_modrinth};

static MODPACK_MANIFEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub mod cache;
#[cfg(feature = "curseforge")]
pub mod curseforge;
#[cfg(feature = "curseforge")]
pub mod curseforge_fingerprint;
pub(crate) mod http;
pub mod modrinth;

/// Broad category of downloadable Minecraft content.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Copy)]
pub enum ModType {
    Mod,
    ResourcePack,
    ShaderPack,
    Modpack,
    DataPack,
    Unknown,
}

impl From<String> for ModType {
    fn from(value: String) -> Self {
        match value.to_lowercase().as_str() {
            "shader" | "shaderpack" => Self::ShaderPack,
            "mod" => Self::Mod,
            "resourcepack" => Self::ResourcePack,
            "modpack" => Self::Modpack,
            "datapack" => Self::DataPack,
            _ => Self::Unknown,
        }
    }
}

impl ModType {
    /// Returns the CurseForge class identifier for this content type.
    pub fn curseforge_id(&self) -> i32 {
        match *self {
            Self::Mod => 6,
            Self::ResourcePack => 12,
            Self::ShaderPack => 6552,
            Self::Modpack => 4471,
            Self::DataPack => 6945,
            Self::Unknown => 999,
        }
    }

    /// Maps a CurseForge class identifier into a `ModType`.
    pub fn from_curseforge_class(class_id: i64) -> Self {
        match class_id {
            6 => Self::Mod,
            12 => Self::ResourcePack,
            6552 => Self::ShaderPack,
            4471 => Self::Modpack,
            6945 => Self::DataPack,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for ModType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match *self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
            Self::Modpack => "modpack",
            Self::DataPack => "datapack",
            Self::Unknown => "unknown",
        };
        f.write_str(label)
    }
}

/// Search or detail result for a mod, resource pack, or shader pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    /// Display name.
    pub name: String,
    /// Provider-specific identifier.
    pub id: String,
    /// Aggregate download count from the upstream provider.
    pub download_count: i64,
    /// Selected or primary version label.
    pub version: String,
    /// Provider "last updated" timestamp (RFC 3339), empty when unknown.
    pub date_modified: String,
    /// Broad content type.
    pub mod_type: ModType,
    /// Upstream source provider.
    pub source: ModSource,
    /// Provider slug used to build URLs.
    pub slug: String,
    /// Preview images exposed by the provider.
    pub thumbnail_urls: Vec<String>,
    /// Canonical provider page URL.
    pub url: String,
    /// Human-readable description.
    pub description: String,
    /// License label, if known.
    pub license: String,
    /// Primary icon URL.
    pub mod_icon_url: String,
    /// Whether the item can currently be downloaded by Quadrant.
    pub downloadable: bool,
    /// Whether old-version information should still be shown in the UI.
    pub show_previous_version: bool,
    /// Newer file candidate when checking for updates.
    pub new_version: Option<UniversalModFile>,
    /// Whether the item may be deleted from a local modpack.
    pub deleteable: bool,
    /// Whether the current host can auto-install this item.
    pub autoinstallable: bool,
    /// Whether the item is user-selectable in the current flow.
    pub selectable: bool,
    /// Optional target modpack name.
    pub modpack: Option<String>,
    /// Optional selection URL used by some frontend flows.
    pub select_url: Option<String>,
}

/// Minecraft version entry returned by the provider.
#[derive(Clone, Serialize, Deserialize)]
pub struct MinecraftVersion {
    /// Version label, such as `1.20.1`.
    pub version: String,
    /// Version type, such as `release`.
    pub version_type: String,
}

/// Provider-agnostic downloadable file representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalModFile {
    /// Optional provider-specific file identifier.
    pub id: Option<String>,
    /// File name to store locally.
    pub file_name: String,
    /// Direct download URL.
    pub download_url: String,
    /// Expected SHA-1 file hash.
    pub sha1: String,
    /// Expected file size in bytes.
    pub size: u64,
}

/// Cross-provider search input used by the host API surface.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchModsArgs {
    /// Source provider to query.
    pub source: ModSource,
    /// Free-text search query.
    pub query: String,
    /// Requested content type.
    pub mod_type: String,
    /// Whether results should be treated as auto-installable into the current
    /// modpack context (mirrors the "match current modpack" toggle).
    #[serde(default)]
    pub filter_on: bool,
    /// Explicit Minecraft version to filter on. Empty or `"any"` disables it.
    #[serde(default)]
    pub game_version: String,
    /// Explicit mod loader to filter on. Empty/`Unknown` disables it.
    #[serde(default)]
    pub mod_loader: String,
    /// Provider-specific category identifiers (CurseForge numeric ids as
    /// strings, or Modrinth tag slugs). Matched as OR.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Restrict to open-source projects (Modrinth only).
    #[serde(default)]
    pub open_source: bool,
    /// Sort key: `relevance` | `downloads` | `name` | `updated`.
    #[serde(default)]
    pub sort_by: String,
    /// Zero-based offset into the provider's result set, for pagination /
    /// "search further". `0` returns the first page.
    #[serde(default)]
    pub offset: u32,
}

/// Provider-local search input.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SearchModsArgs {
    /// Free-text search query.
    pub query: String,
    /// Requested content type.
    pub mod_type: String,
    /// Whether results should be treated as auto-installable.
    pub filter_on: bool,
    /// Explicit Minecraft version to filter on. Empty or `"any"` disables it.
    pub game_version: String,
    /// Explicit mod loader to filter on. Empty/`Unknown` disables it.
    pub mod_loader: String,
    /// Provider-specific category identifiers, matched as OR.
    pub categories: Vec<String>,
    /// Restrict to open-source projects (Modrinth only).
    pub open_source: bool,
    /// Sort key: `relevance` | `downloads` | `name` | `updated`.
    pub sort_by: String,
    /// Zero-based offset into the provider's result set, for pagination.
    pub offset: u32,
}

/// A selectable search facet exposed by a provider (category, resolution,
/// feature, or performance tag).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCategory {
    /// Provider-specific identifier passed back in `SearchModsArgs::categories`
    /// (CurseForge numeric id as a string, or Modrinth tag slug).
    pub id: String,
    /// Human-readable display label.
    pub name: String,
    /// Grouping header: `categories` | `resolutions` | `features` |
    /// `performance impact`.
    pub header: String,
    /// Provider this facet belongs to.
    pub source: ModSource,
}

/// Mod detail and install input exposed to hosts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetModArgs {
    /// Provider-specific mod identifier.
    pub id: String,
    /// Whether the item should be presented as downloadable.
    pub downloadable: bool,
    /// Whether previous-version info should be displayed.
    pub show_previous_version: bool,
    /// Whether the item may be deleted locally.
    pub deletable: bool,
    /// Target Minecraft version.
    pub version_target: String,
    /// Target mod loader.
    pub mod_loader: ModLoader,
    /// Target modpack name.
    pub modpack: String,
    /// Whether the item is user-selectable.
    pub selectable: bool,
    /// Optional selection URL used by frontend flows.
    pub select_url: Option<String>,
}

/// Result of identifying a local file as a known upstream mod.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IdentifiedMod {
    /// Installed mod metadata to persist if selected.
    pub installed_mod: InstalledMod,
    /// File name of the local mod file.
    pub file_name: String,
}

/// Returns the default Quadrant user agent used for upstream requests.
pub fn get_user_agent() -> String {
    format!(
        "mrquantumoff/quadrant/v{} (mrquantumoff.dev) (QUADRANT NEXT/TAURI)",
        env!("CARGO_PKG_VERSION")
    )
}

/// Fetches release Minecraft versions from Modrinth.
pub async fn get_versions() -> Result<Vec<MinecraftVersion>> {
    let response = provider_cached_client()
        .get(format!(
            "{}/v2/tag/game_version",
            modrinth::modrinth_api_base()
        ))
        .send()
        .await?;
    let body: Vec<MinecraftVersion> = response.json().await?;
    Ok(body
        .into_iter()
        .filter(|version| version.version_type == "release" && !version.version.is_empty())
        .collect())
}

/// Checks whether a mod has an upgrade available for the given target environment.
pub async fn check_mod_updates(
    mod_to_update: Mod,
    minecraft_version: String,
    mod_loader: ModLoader,
    modpack: LocalModpack,
    show_unupgradeable_mods: bool,
) -> Result<Option<Mod>> {
    let mut new_mod = mod_to_update.clone();
    let existing_mod = modpack.mods.iter().find(|mod_| mod_.id == mod_to_update.id);

    new_mod.show_previous_version = true;
    new_mod.deleteable = false;
    new_mod.autoinstallable = true;

    match mod_to_update.source {
        ModSource::CurseForge => {
            #[cfg(feature = "curseforge")]
            {
                if let Some(latest_file) = get_latest_mod_version_curseforge(
                    mod_to_update.id,
                    minecraft_version,
                    mod_loader,
                    mod_to_update.mod_type,
                    None,
                )
                .await?
                {
                    new_mod.new_version = Some(latest_file.into());
                }
            }
            #[cfg(not(feature = "curseforge"))]
            {
                return Ok(None);
            }
        }
        ModSource::Modrinth => {
            if let Some(latest_file) = get_latest_mod_version_modrinth(
                mod_to_update.id,
                minecraft_version,
                mod_loader,
                mod_to_update.mod_type,
            )
            .await?
            {
                new_mod.new_version = Some(latest_file.into());
            }
        }
        ModSource::Online => {}
    }

    if let (Some(existing_mod), Some(new_file)) = (existing_mod, new_mod.new_version.clone()) {
        if existing_mod.download_url != new_file.download_url {
            new_mod.downloadable = true;
        }
        if existing_mod.download_url == new_file.download_url && !show_unupgradeable_mods {
            return Ok(None);
        }
    }

    Ok(Some(new_mod))
}

/// Searches mods from the requested provider and sorts them by download count.
pub async fn search_mods(args: GlobalSearchModsArgs) -> Result<Vec<Mod>> {
    let search_args = SearchModsArgs {
        query: args.query,
        mod_type: args.mod_type,
        filter_on: args.filter_on,
        game_version: args.game_version,
        mod_loader: args.mod_loader,
        categories: args.categories,
        open_source: args.open_source,
        sort_by: args.sort_by,
        offset: args.offset,
    };

    let mods = match args.source {
        ModSource::CurseForge => {
            #[cfg(feature = "curseforge")]
            {
                search_mods_curseforge(search_args).await?
            }
            #[cfg(not(feature = "curseforge"))]
            {
                Vec::new()
            }
        }
        ModSource::Modrinth => search_mods_modrinth(search_args).await?,
        ModSource::Online => Vec::new(),
    };

    // Final ordering is applied client-side across the merged provider results,
    // so no server-side re-sort is imposed here.
    Ok(mods)
}

/// Lists the selectable category/facet tags a provider exposes for a content
/// type, used to populate the search filter sidebar.
pub async fn get_categories(source: ModSource, mod_type: String) -> Result<Vec<SearchCategory>> {
    match source {
        ModSource::CurseForge => {
            #[cfg(feature = "curseforge")]
            {
                curseforge::get_categories_curseforge(ModType::from(mod_type)).await
            }
            #[cfg(not(feature = "curseforge"))]
            {
                let _ = mod_type;
                Ok(Vec::new())
            }
        }
        ModSource::Modrinth => modrinth::get_categories_modrinth(ModType::from(mod_type)).await,
        ModSource::Online => Ok(Vec::new()),
    }
}

/// Builds the canonical provider page URL for a mod or content item.
pub fn get_mod_url(slug: String, mod_type: ModType, source: ModSource) -> String {
    let base_url = match source {
        ModSource::CurseForge => "https://curseforge.com/minecraft",
        ModSource::Modrinth => "https://modrinth.com",
        ModSource::Online => "",
    };

    let mod_type = match source {
        ModSource::CurseForge => match mod_type {
            ModType::Mod => "mc-mods",
            ModType::ResourcePack => "texture-packs",
            ModType::ShaderPack => "customization",
            ModType::Modpack => "modpacks",
            ModType::DataPack => "data-packs",
            ModType::Unknown => "",
        },
        ModSource::Modrinth => match mod_type {
            ModType::Mod => "mod",
            ModType::ResourcePack => "resourcepack",
            ModType::ShaderPack => "shader",
            ModType::Modpack => "modpack",
            ModType::DataPack => "datapack",
            ModType::Unknown => "unknown",
        },
        ModSource::Online => "",
    };

    format!("{}/{}/{}", base_url, mod_type, slug)
}

/// Builds the canonical provider page URL for a user or author profile.
pub fn get_user_url(username: String, source: ModSource) -> String {
    let base_url = match source {
        ModSource::CurseForge => "https://curseforge.com/members",
        ModSource::Modrinth => "https://modrinth.com/user",
        ModSource::Online => "",
    };

    format!("{}/{}", base_url, username)
}

/// Fetches full mod metadata from the upstream provider to enrich an `InstalledMod`
/// that only has basic fields populated.
pub async fn enrich_installed_mod(mod_: InstalledMod) -> Result<InstalledMod> {
    if !mod_.name.is_empty() {
        return Ok(mod_);
    }
    let args = GetModArgs {
        id: mod_.id.clone(),
        downloadable: false,
        show_previous_version: false,
        deletable: false,
        version_target: String::new(),
        mod_loader: ModLoader::Unknown,
        modpack: String::new(),
        selectable: false,
        select_url: None,
    };
    let details = match mod_.source {
        ModSource::Modrinth => modrinth::get_mod_modrinth(args).await,
        ModSource::CurseForge => {
            #[cfg(feature = "curseforge")]
            {
                curseforge::get_mod_curseforge(args).await
            }
            #[cfg(not(feature = "curseforge"))]
            {
                return Ok(mod_);
            }
        }
        ModSource::Online => return Ok(mod_),
    }?;
    let mod_type_str = match details.mod_type {
        ModType::Mod => "Mod",
        ModType::ResourcePack => "ResourcePack",
        ModType::ShaderPack => "ShaderPack",
        ModType::Modpack => "Modpack",
        ModType::DataPack => "DataPack",
        ModType::Unknown => "Unknown",
    };
    Ok(InstalledMod {
        name: details.name,
        download_count: details.download_count,
        version: details.version,
        mod_type: mod_type_str.to_string(),
        slug: details.slug,
        thumbnail_urls: details.thumbnail_urls,
        description: details.description,
        license: details.license,
        mod_icon_url: details.mod_icon_url,
        ..mod_
    })
}

/// Downloads and installs a provider-backed mod into the requested target.
///
/// Progress is emitted through [`BackendEvent::ModDownloadProgress`] and
/// [`BackendEvent::ModInstallProgress`].
pub async fn install_mod(
    mc_folder: &Path,
    existing_modpacks: &[LocalModpack],
    settings: &impl SettingsStore,
    event_sink: &impl EventSink,
    id: String,
    minecraft_version: String,
    mod_loader: ModLoader,
    source: ModSource,
    modpack: Option<String>,
    mod_type: ModType,
    #[allow(unused_variables)] file_id: Option<String>,
) -> Result<Option<LocalModpack>> {
    log::info!("Installing mod {id} from {source:?} (type={mod_type:?})");
    let download_path = match source {
        ModSource::CurseForge => {
            #[cfg(feature = "curseforge")]
            {
                download_mod_curseforge(
                    settings,
                    event_sink,
                    id.clone(),
                    minecraft_version,
                    mod_loader,
                    mod_type,
                    file_id,
                )
                .await?
            }
            #[cfg(not(feature = "curseforge"))]
            {
                return Err(anyhow!("CurseForge is not enabled"));
            }
        }
        ModSource::Modrinth => {
            download_mod_modrinth(
                settings,
                event_sink,
                id.clone(),
                minecraft_version,
                mod_loader,
                mod_type,
            )
            .await?
        }
        ModSource::Online => unimplemented!(),
    };

    event_sink.publish(BackendEvent::ModInstallProgress(ModProgressPayload {
        mod_id: id.clone(),
        progress: 50,
    }))?;

    let mod_to_install = enrich_installed_mod(InstalledMod::minimal(
        id.clone(),
        source.clone(),
        download_path.1.clone(),
    ))
    .await
    .unwrap_or_else(|e| {
        log::warn!("Failed to enrich mod {id} on install: {}", e);
        InstalledMod::minimal(id.clone(), source, download_path.1.clone())
    });

    let updated_modpack = install_local_file(
        mc_folder,
        existing_modpacks,
        download_path.0,
        mod_to_install,
        mod_type,
        modpack,
    )?;

    event_sink.publish(BackendEvent::ModInstallProgress(ModProgressPayload {
        mod_id: id,
        progress: 100,
    }))?;

    Ok(updated_modpack)
}

/// Downloads a specific file, using the shared cache when possible.
pub async fn get_file(
    file: UniversalModFile,
    id: String,
    event_sink: &impl EventSink,
) -> Result<(PathBuf, String)> {
    if let Some(cached_file) = get_cache_index(file.sha1.clone()).await? {
        log::info!("Cache hit for mod {id} (sha1={})", file.sha1);
        let cached_file_bytes = std::fs::read(&cached_file.file_name).map_err(|error| {
            let _ = futures::executor::block_on(init_cache());
            anyhow!(error)
        })?;
        if file_hash(&cached_file_bytes).eq_ignore_ascii_case(&file.sha1) {
            let file_path = add_cache_index(
                file.file_name.clone(),
                cached_file_bytes.as_slice(),
                file.sha1.clone(),
            )
            .await?;
            event_sink.publish(BackendEvent::ModDownloadProgress(ModProgressPayload {
                mod_id: id,
                progress: 100,
            }))?;
            return Ok((file_path, file.download_url));
        }
        log::warn!("Discarding corrupt cache entry for sha1={}", file.sha1);
        let _ = std::fs::remove_file(cached_file.file_name);
        init_cache().await?;
    }

    log::info!(
        "Cache miss for mod {id}, downloading from {}",
        file.download_url
    );
    let request = provider_http_client().get(&file.download_url).build()?;
    let response = provider_http_client()
        .execute(request)
        .await?
        .error_for_status()?;
    let mut body = response.bytes_stream();
    let mut file_bytes = Vec::new();
    while let Some(next) = body.next().await {
        let new_bytes = next?;
        file_bytes.append(&mut new_bytes.to_vec());
        let progress = if file.size == 0 {
            0
        } else {
            ((file_bytes.len() as f64 / file.size as f64) * 100_f64)
                .round()
                .clamp(0.0, 99.0) as i32
        };
        event_sink.publish(BackendEvent::ModDownloadProgress(ModProgressPayload {
            mod_id: id.clone(),
            progress,
        }))?;
    }
    let hash = file_hash(file_bytes.as_slice());
    if !file.sha1.is_empty() && !hash.eq_ignore_ascii_case(&file.sha1) {
        return Err(anyhow!("Downloaded file failed SHA-1 verification"));
    }
    let file_path = add_cache_index(file.file_name.clone(), &file_bytes, hash).await?;
    event_sink.publish(BackendEvent::ModDownloadProgress(ModProgressPayload {
        mod_id: id,
        progress: 100,
    }))?;
    Ok((file_path, file.download_url))
}

/// Installs an already-downloaded file into a modpack or game content folder.
pub fn install_local_file(
    mc_folder: &Path,
    _existing_modpacks: &[LocalModpack],
    file: PathBuf,
    local_mod: InstalledMod,
    mod_type: ModType,
    modpack: Option<String>,
) -> Result<Option<LocalModpack>> {
    let id = local_mod.id.clone();
    let source = local_mod.source.clone();
    let target_file_name = reqwest::Url::parse(&local_mod.download_url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(ToOwned::to_owned))
        .and_then(|name| {
            urlencoding::decode(&name)
                .ok()
                .map(|name| name.into_owned())
        })
        .filter(|name| {
            let mut components = Path::new(name).components();
            matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
        })
        .ok_or_else(|| anyhow!("Invalid download file name"))?;
    let _manifest_guard = if mod_type == ModType::Mod {
        Some(
            MODPACK_MANIFEST_LOCK
                .lock()
                .map_err(|_| anyhow!("Modpack manifest lock poisoned"))?,
        )
    } else {
        None
    };

    let (target_path, updated_modpack, manifest_path, old_file_path) = match mod_type {
        ModType::Mod => {
            let modpack_name = modpack.ok_or_else(|| anyhow!("modpackRequired"))?;
            let manifest_path =
                crate::models::modpack_path(mc_folder, &modpack_name).join("modConfigV2.json");
            // Re-read under a process-wide lock so concurrent downloads merge
            // their entries instead of each writing an old host snapshot.
            let manifest: InstalledModpack = serde_json::from_reader(
                std::fs::File::open(&manifest_path).map_err(|_| anyhow!("Modpack not found"))?,
            )?;
            let mut modpack = LocalModpack::from((manifest, false, 0));
            let old_file_path = modpack
                .mods
                .iter()
                .find(|mod_| mod_.id == id)
                .and_then(|mod_| reqwest::Url::parse(&mod_.download_url).ok())
                .and_then(|url| url.path_segments()?.next_back().map(ToOwned::to_owned))
                .and_then(|name| {
                    urlencoding::decode(&name)
                        .ok()
                        .map(|name| name.into_owned())
                })
                .filter(|name| {
                    let mut components = Path::new(name).components();
                    matches!(components.next(), Some(Component::Normal(_)))
                        && components.next().is_none()
                })
                .map(|name| crate::models::modpack_path(mc_folder, &modpack_name).join(name));
            if modpack.mods.iter().any(|mod_| mod_.id == id) {
                modpack.mods.retain(|mod_| mod_.id != id);
            }
            modpack.mods.push(local_mod.clone());

            (
                crate::models::modpack_path(mc_folder, &modpack_name).join(&target_file_name),
                Some(modpack),
                Some(manifest_path),
                old_file_path,
            )
        }
        ModType::ResourcePack => (
            mc_folder.join("resourcepacks").join(&target_file_name),
            None,
            None,
            None,
        ),
        ModType::ShaderPack => (
            mc_folder.join("shaderpacks").join(&target_file_name),
            None,
            None,
            None,
        ),
        // Modpacks and data packs are browsable in search but are not installed
        // through the single-file mod path (modpacks use the import flow; data
        // packs are per-world). The UI surfaces "open in web" for these instead.
        ModType::Modpack | ModType::DataPack => {
            return Err(anyhow!("unsupportedDownload"));
        }
        ModType::Unknown => return Err(anyhow!("unsupportedDownload")),
    };

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    log::info!("Installed mod {id} ({mod_type:?}) from {source:?}");
    std::fs::copy(file, &target_path)?;
    if let (Some(manifest_path), Some(updated_modpack)) = (manifest_path, updated_modpack.as_ref())
    {
        std::fs::write(
            manifest_path,
            serde_json::to_string_pretty(&InstalledModpack::from(updated_modpack.clone()))?,
        )?;
    }
    if let Some(old_file_path) = old_file_path
        && old_file_path != target_path
        && old_file_path.is_file()
    {
        std::fs::remove_file(old_file_path)?;
    }
    Ok(updated_modpack)
}

/// Downloads a remote file and installs it into the requested target.
pub async fn install_remote_file(
    mc_folder: &Path,
    existing_modpacks: &[LocalModpack],
    event_sink: &impl EventSink,
    file: UniversalModFile,
    mod_type: ModType,
    modpack: Option<String>,
    source: ModSource,
    id: String,
) -> Result<Option<LocalModpack>> {
    let downloaded_file = get_file(file, id.clone(), event_sink).await?;
    let mod_to_install = enrich_installed_mod(InstalledMod::minimal(
        id.clone(),
        source.clone(),
        downloaded_file.1.clone(),
    ))
    .await
    .unwrap_or_else(|e| {
        log::warn!("Failed to enrich mod {id} on remote install: {}", e);
        InstalledMod::minimal(id.clone(), source, downloaded_file.1.clone())
    });
    install_local_file(
        mc_folder,
        existing_modpacks,
        downloaded_file.0,
        mod_to_install,
        mod_type,
        modpack,
    )
}

/// Attempts to identify local mod files in a modpack using enabled providers.
pub async fn identify_modpack(
    mc_folder: &Path,
    modpack: String,
    curseforge_enabled: bool,
    modrinth_enabled: bool,
) -> Result<Vec<IdentifiedMod>> {
    log::info!(
        "Identifying mods in modpack \"{modpack}\" (curseforge={curseforge_enabled}, modrinth={modrinth_enabled})"
    );
    let mut mods = Vec::new();
    if curseforge_enabled {
        #[cfg(feature = "curseforge")]
        {
            if let Ok(mut curse_mods) =
                curseforge::identify_modpack_curseforge(&mc_folder.to_path_buf(), modpack.clone())
                    .await
            {
                mods.append(&mut curse_mods);
            }
        }
    }
    if modrinth_enabled {
        let mut modrinth_mods =
            modrinth::identify_modpack_modrinth(&mc_folder.to_path_buf(), modpack).await?;
        mods.append(&mut modrinth_mods);
    }
    log::info!("Identified {} mod(s)", mods.len());
    Ok(mods)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_type_from_string_covers_aliases_and_fallback() {
        let cases = [
            ("mod", ModType::Mod),
            ("Mod", ModType::Mod),
            ("shader", ModType::ShaderPack),
            ("shaderpack", ModType::ShaderPack),
            ("ShaderPack", ModType::ShaderPack),
            ("resourcepack", ModType::ResourcePack),
            ("modpack", ModType::Modpack),
            ("datapack", ModType::DataPack),
            ("", ModType::Unknown),
            ("plugin", ModType::Unknown),
        ];
        for (input, expected) in cases {
            assert_eq!(ModType::from(input.to_string()), expected, "{input:?}");
        }
    }

    #[test]
    fn mod_type_curseforge_class_ids_round_trip() {
        let cases = [
            (ModType::Mod, 6),
            (ModType::ResourcePack, 12),
            (ModType::ShaderPack, 6552),
            (ModType::Modpack, 4471),
            (ModType::DataPack, 6945),
        ];
        for (mod_type, class_id) in cases {
            assert_eq!(mod_type.curseforge_id(), class_id as i32);
            assert_eq!(ModType::from_curseforge_class(class_id), mod_type);
        }
        assert_eq!(ModType::Unknown.curseforge_id(), 999);
        assert_eq!(ModType::from_curseforge_class(0), ModType::Unknown);
        assert_eq!(ModType::from_curseforge_class(999), ModType::Unknown);
    }

    #[test]
    fn mod_type_display_labels() {
        assert_eq!(ModType::Mod.to_string(), "mod");
        assert_eq!(ModType::ResourcePack.to_string(), "resourcepack");
        assert_eq!(ModType::ShaderPack.to_string(), "shader");
        assert_eq!(ModType::Modpack.to_string(), "modpack");
        assert_eq!(ModType::DataPack.to_string(), "datapack");
        assert_eq!(ModType::Unknown.to_string(), "unknown");
    }

    #[test]
    fn get_mod_url_builds_curseforge_urls() {
        let cases = [
            (ModType::Mod, "https://curseforge.com/minecraft/mc-mods/jei"),
            (
                ModType::ResourcePack,
                "https://curseforge.com/minecraft/texture-packs/jei",
            ),
            (
                ModType::ShaderPack,
                "https://curseforge.com/minecraft/customization/jei",
            ),
            (
                ModType::Modpack,
                "https://curseforge.com/minecraft/modpacks/jei",
            ),
            (
                ModType::DataPack,
                "https://curseforge.com/minecraft/data-packs/jei",
            ),
            (ModType::Unknown, "https://curseforge.com/minecraft//jei"),
        ];
        for (mod_type, expected) in cases {
            assert_eq!(
                get_mod_url("jei".to_string(), mod_type, ModSource::CurseForge),
                expected
            );
        }
    }

    #[test]
    fn get_mod_url_builds_modrinth_urls() {
        let cases = [
            (ModType::Mod, "https://modrinth.com/mod/sodium"),
            (
                ModType::ResourcePack,
                "https://modrinth.com/resourcepack/sodium",
            ),
            (ModType::ShaderPack, "https://modrinth.com/shader/sodium"),
            (ModType::Modpack, "https://modrinth.com/modpack/sodium"),
            (ModType::DataPack, "https://modrinth.com/datapack/sodium"),
            (ModType::Unknown, "https://modrinth.com/unknown/sodium"),
        ];
        for (mod_type, expected) in cases {
            assert_eq!(
                get_mod_url("sodium".to_string(), mod_type, ModSource::Modrinth),
                expected
            );
        }
    }

    #[test]
    fn get_mod_url_online_has_no_base() {
        assert_eq!(
            get_mod_url("thing".to_string(), ModType::Mod, ModSource::Online),
            "//thing"
        );
    }

    #[test]
    fn get_user_url_builds_provider_profile_urls() {
        assert_eq!(
            get_user_url("dev".to_string(), ModSource::CurseForge),
            "https://curseforge.com/members/dev"
        );
        assert_eq!(
            get_user_url("dev".to_string(), ModSource::Modrinth),
            "https://modrinth.com/user/dev"
        );
        assert_eq!(get_user_url("dev".to_string(), ModSource::Online), "/dev");
    }

    #[test]
    fn get_user_agent_embeds_crate_version() {
        let agent = get_user_agent();
        assert!(agent.starts_with("mrquantumoff/quadrant/v"));
        assert!(agent.contains(env!("CARGO_PKG_VERSION")));
    }
}
