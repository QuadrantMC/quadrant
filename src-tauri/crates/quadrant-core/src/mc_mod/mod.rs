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
    /// Older frontends and N-API consumers send `Shader`, so both labels have
    /// to arrive as a shader pack.
    #[serde(alias = "Shader")]
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
        "QuadrantMC/quadrant/v{} (usequadrant.dev) (QUADRANT NEXT/TAURI)",
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
    // Online mods cannot be re-downloaded through a provider, so never offer
    // one-click install for them.
    new_mod.autoinstallable = mod_to_update.source != ModSource::Online;

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
                    // A file the author barred from third-party download is
                    // not an update the user can install.
                    match UniversalModFile::try_from(latest_file) {
                        Ok(file) => new_mod.new_version = Some(file),
                        Err(_) if !show_unupgradeable_mods => return Ok(None),
                        Err(_) => {}
                    }
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

/// Downloads and installs a provider-backed mod into every requested root.
///
/// The file is resolved and enriched once and then placed into each root, so a
/// resource pack that follows a modpack into several game directories costs one
/// download and one run of progress events. An empty `install_roots` is
/// [`ErrorCode::InvalidRequest`](crate::error::ErrorCode::InvalidRequest).
///
/// Progress is emitted through [`BackendEvent::ModDownloadProgress`] and
/// [`BackendEvent::ModInstallProgress`].
pub async fn install_mod(
    install_roots: &[PathBuf],
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
    if install_roots.is_empty() {
        return Err(anyhow::Error::from(crate::error::ErrorCode::InvalidRequest));
    }
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
                return Err(anyhow::Error::from(
                    crate::error::ErrorCode::CurseforgeDisabled,
                ));
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
        // Online mods have no provider to resolve a file from. Fail cleanly so
        // the caller's promise settles instead of panicking the command task.
        ModSource::Online => return Err(anyhow!("unsupportedDownload")),
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

    let updated_modpack = place_in_every_root(
        install_roots,
        &download_path.0,
        &mod_to_install,
        mod_type,
        modpack.as_deref(),
    )?;

    event_sink.publish(BackendEvent::ModInstallProgress(ModProgressPayload {
        mod_id: id,
        progress: 100,
    }))?;

    Ok(updated_modpack)
}

/// Installs one downloaded file into every root, returning the last modpack a
/// root updated. Only [`ModType::Mod`] updates a modpack, and it always has
/// exactly one root.
fn place_in_every_root(
    install_roots: &[PathBuf],
    file: &Path,
    local_mod: &InstalledMod,
    mod_type: ModType,
    modpack: Option<&str>,
) -> Result<Option<LocalModpack>> {
    let mut updated_modpack = None;
    for root in install_roots {
        updated_modpack = install_local_file(
            root,
            file.to_path_buf(),
            local_mod.clone(),
            mod_type,
            modpack.map(ToOwned::to_owned),
        )?;
    }
    Ok(updated_modpack)
}

/// Files also arrive from the frontend, where a provider's missing download
/// URL (the author barred third-party downloads) is an empty string.
fn ensure_downloadable(file: &UniversalModFile) -> Result<()> {
    if file.download_url.trim().is_empty() {
        return Err(crate::error::ErrorCode::ThirdPartyDownloadDisabled.into());
    }
    Ok(())
}

/// Downloads a specific file, using the shared cache when possible.
pub async fn get_file(
    file: UniversalModFile,
    id: String,
    event_sink: &impl EventSink,
) -> Result<(PathBuf, String)> {
    ensure_downloadable(&file)?;

    if let Some(cached_file) = get_cache_index(file.sha1.clone()).await? {
        log::info!("Cache hit for mod {id} (sha1={})", file.sha1);
        match std::fs::read(&cached_file.file_name) {
            Ok(cached_file_bytes)
                if file_hash(&cached_file_bytes).eq_ignore_ascii_case(&file.sha1) =>
            {
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
            Ok(_) => {
                log::warn!("Discarding corrupt cache entry for sha1={}", file.sha1);
                let _ = std::fs::remove_file(&cached_file.file_name);
            }
            Err(error) => {
                // A missing or unreadable cached file is not fatal: prune the
                // stale index entry and fall through to a fresh download.
                log::warn!(
                    "Cached file for sha1={} is unreadable ({error}); re-downloading",
                    file.sha1
                );
            }
        }
        // Never block the async worker here; the index lock is a tokio mutex.
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
        .error_for_status()
        .map_err(|error| match error.status().map(|status| status.as_u16()) {
            Some(401 | 403) => {
                anyhow::Error::from(error).context(crate::error::ErrorCode::DownloadRefused)
            }
            _ => error.into(),
        })?;
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
        return Err(anyhow::Error::from(crate::error::ErrorCode::Checksum));
    }
    let file_path = add_cache_index(file.file_name.clone(), &file_bytes, hash).await?;
    event_sink.publish(BackendEvent::ModDownloadProgress(ModProgressPayload {
        mod_id: id,
        progress: 100,
    }))?;
    Ok((file_path, file.download_url))
}

/// Resolves the file name a download URL installs as.
///
/// Lenient on purpose, unlike `crate::modpacks::safe_download_file_name`: an
/// entry written under an older URL policy must still resolve to the jar it
/// created, so it can be removed.
fn download_file_name(download_url: &str) -> Option<String> {
    reqwest::Url::parse(download_url)
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
}

/// Installs an already-downloaded file into a modpack or game content folder.
pub fn install_local_file(
    mc_folder: &Path,
    file: PathBuf,
    local_mod: InstalledMod,
    mod_type: ModType,
    modpack: Option<String>,
) -> Result<Option<LocalModpack>> {
    let id = local_mod.id.clone();
    let source = local_mod.source.clone();
    let target_file_name = download_file_name(&local_mod.download_url)
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::UnsafeDownload))?;
    let _manifest_guard = if mod_type == ModType::Mod {
        Some(
            MODPACK_MANIFEST_LOCK
                .lock()
                .map_err(|_| anyhow!("Modpack manifest lock poisoned"))?,
        )
    } else {
        None
    };

    let (target_path, updated_modpack, manifest_path, old_file_paths) = match mod_type {
        ModType::Mod => {
            let modpack_name = modpack.ok_or_else(|| anyhow!("modpackRequired"))?;
            crate::modpacks::validate_modpack_name(&modpack_name)?;
            let manifest_path =
                crate::models::modpack_path(mc_folder, &modpack_name).join("modConfigV2.json");
            // Re-read under a process-wide lock so concurrent downloads merge
            // their entries instead of each writing an old host snapshot.
            let manifest: InstalledModpack = serde_json::from_reader(
                std::fs::File::open(&manifest_path)
                    .map_err(|_| anyhow::Error::from(crate::error::ErrorCode::ModpackMissing))?,
            )?;
            let mut modpack = LocalModpack::from((manifest, false, 0));
            let is_replaced = |existing: &InstalledMod| existing.is_same_mod(&local_mod);
            let old_file_paths: Vec<PathBuf> = modpack
                .mods
                .iter()
                .filter(|existing| is_replaced(existing))
                .filter_map(|existing| download_file_name(&existing.download_url))
                .map(|name| crate::models::modpack_path(mc_folder, &modpack_name).join(name))
                .collect();
            modpack.mods.retain(|existing| !is_replaced(existing));
            modpack.mods.push(local_mod.clone());

            (
                crate::models::modpack_path(mc_folder, &modpack_name).join(&target_file_name),
                Some(modpack),
                Some(manifest_path),
                old_file_paths,
            )
        }
        ModType::ResourcePack | ModType::ShaderPack => (
            mc_folder
                .join(
                    crate::content::content_subfolder(mod_type)
                        .ok_or_else(|| anyhow!("unsupportedDownload"))?,
                )
                .join(&target_file_name),
            None,
            None,
            Vec::new(),
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
    // Compare resolved paths: on a case-insensitive filesystem `Sodium.jar`
    // and `sodium.jar` are the jar that was just written.
    let installed_path = std::fs::canonicalize(&target_path)?;
    // Old jars go before the manifest is committed: if one is locked (the
    // game is running), its entry survives and a retry still finds the jar.
    for old_file_path in old_file_paths {
        if let Ok(old_path) = std::fs::canonicalize(&old_file_path)
            && old_path != installed_path
            && old_path.is_file()
        {
            std::fs::remove_file(old_path)?;
        }
    }
    if let (Some(manifest_path), Some(updated_modpack)) = (manifest_path, updated_modpack.as_ref())
    {
        crate::modpacks::write_file_atomically(
            manifest_path,
            serde_json::to_string_pretty(&InstalledModpack::from(updated_modpack.clone()))?,
        )?;
    }
    Ok(updated_modpack)
}

/// Downloads a remote file once and installs it into every requested root.
///
/// An empty `install_roots` is
/// [`ErrorCode::InvalidRequest`](crate::error::ErrorCode::InvalidRequest).
pub async fn install_remote_file(
    install_roots: &[PathBuf],
    event_sink: &impl EventSink,
    file: UniversalModFile,
    mod_type: ModType,
    modpack: Option<String>,
    source: ModSource,
    id: String,
) -> Result<Option<LocalModpack>> {
    if install_roots.is_empty() {
        return Err(anyhow::Error::from(crate::error::ErrorCode::InvalidRequest));
    }
    ensure_downloadable(&file)?;
    // Apply the same URL policy that later delete/install-modpack paths
    // enforce, so every manifest entry this creates can be removed again.
    crate::modpacks::safe_download_file_name(&file.download_url, &source)?;
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
    place_in_every_root(
        install_roots,
        &downloaded_file.0,
        &mod_to_install,
        mod_type,
        modpack.as_deref(),
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
    fn mod_type_reads_every_label_a_frontend_sends() {
        for (label, expected) in [
            ("\"Mod\"", ModType::Mod),
            ("\"ResourcePack\"", ModType::ResourcePack),
            ("\"ShaderPack\"", ModType::ShaderPack),
            ("\"Shader\"", ModType::ShaderPack),
            ("\"DataPack\"", ModType::DataPack),
            ("\"Unknown\"", ModType::Unknown),
        ] {
            assert_eq!(
                serde_json::from_str::<ModType>(label).unwrap(),
                expected,
                "{label}"
            );
        }
        assert_eq!(
            serde_json::to_string(&ModType::ShaderPack).unwrap(),
            "\"ShaderPack\""
        );
    }

    #[test]
    fn install_local_file_rejects_modpack_path_traversal_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let mc_folder = dir.path().join("minecraft");
        std::fs::create_dir_all(mc_folder.join("modpacks")).unwrap();
        let outside = mc_folder.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let manifest = serde_json::to_vec(&InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: String::new(),
            name: "outside".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: Vec::new(),
        })
        .unwrap();
        std::fs::write(outside.join("modConfigV2.json"), &manifest).unwrap();
        let source_file = dir.path().join("download.jar");
        std::fs::write(&source_file, "mod bytes").unwrap();
        let local_mod = InstalledMod::minimal(
            "mod".to_string(),
            ModSource::Online,
            "https://example.invalid/mod.jar".to_string(),
        );

        for name in [
            "../outside".to_string(),
            outside.to_string_lossy().to_string(),
        ] {
            assert!(
                install_local_file(
                    &mc_folder,
                    source_file.clone(),
                    local_mod.clone(),
                    ModType::Mod,
                    Some(name),
                )
                .is_err()
            );
            assert!(!outside.join("mod.jar").exists());
            assert_eq!(
                std::fs::read(outside.join("modConfigV2.json")).unwrap(),
                manifest
            );
        }
    }

    #[test]
    fn install_local_file_installs_into_valid_modpack() {
        let dir = tempfile::tempdir().unwrap();
        crate::modpacks::create_modpack(dir.path(), &[], "alpha", "1.20.1", ModLoader::Fabric)
            .unwrap();
        let source_file = dir.path().join("download.jar");
        std::fs::write(&source_file, "mod bytes").unwrap();
        let updated = install_local_file(
            dir.path(),
            source_file,
            InstalledMod::minimal(
                "mod".to_string(),
                ModSource::Online,
                "https://example.invalid/my%20mod.jar".to_string(),
            ),
            ModType::Mod,
            Some("alpha".to_string()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.mods.len(), 1);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("modpacks/alpha/my mod.jar")).unwrap(),
            "mod bytes"
        );
    }

    fn seed_modpack(mc_folder: &Path, name: &str, mods: Vec<InstalledMod>) {
        crate::modpacks::create_modpack(mc_folder, &[], name, "1.20.1", ModLoader::Fabric).unwrap();
        std::fs::write(
            crate::models::modpack_path(mc_folder, name).join("modConfigV2.json"),
            serde_json::to_vec(&InstalledModpack {
                mod_config_version: "2".to_string(),
                quadrant_version: String::new(),
                name: name.to_string(),
                version: "1.20.1".to_string(),
                mod_loader: ModLoader::Fabric,
                mods,
            })
            .unwrap(),
        )
        .unwrap();
    }

    fn installed_jar(mc_folder: &Path, modpack: &str, file_name: &str) -> PathBuf {
        let path = crate::models::modpack_path(mc_folder, modpack).join(file_name);
        std::fs::write(&path, "old bytes").unwrap();
        path
    }

    fn mod_entry(id: &str, source: ModSource, slug: &str, file_name: &str) -> InstalledMod {
        InstalledMod {
            slug: slug.to_string(),
            ..InstalledMod::minimal(
                id.to_string(),
                source,
                format!("https://example.invalid/{file_name}"),
            )
        }
    }

    fn install_into_alpha(mc_folder: &Path, mod_: InstalledMod) -> LocalModpack {
        let source_file = mc_folder.join("download.jar");
        std::fs::write(&source_file, "new bytes").unwrap();
        install_local_file(
            mc_folder,
            source_file,
            mod_,
            ModType::Mod,
            Some("alpha".to_string()),
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn install_local_file_replaces_same_mod_with_new_file_name() {
        let dir = tempfile::tempdir().unwrap();
        seed_modpack(
            dir.path(),
            "alpha",
            vec![mod_entry(
                "sodium",
                ModSource::Modrinth,
                "",
                "sodium-old.jar",
            )],
        );
        let old_jar = installed_jar(dir.path(), "alpha", "sodium-old.jar");

        let updated = install_into_alpha(
            dir.path(),
            mod_entry("sodium", ModSource::Modrinth, "", "sodium-new.jar"),
        );

        assert_eq!(updated.mods.len(), 1);
        assert!(!old_jar.exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("modpacks/alpha/sodium-new.jar")).unwrap(),
            "new bytes"
        );
    }

    #[test]
    fn install_local_file_overwrites_same_mod_with_same_file_name() {
        let dir = tempfile::tempdir().unwrap();
        seed_modpack(
            dir.path(),
            "alpha",
            vec![mod_entry("sodium", ModSource::Modrinth, "", "sodium.jar")],
        );
        let jar = installed_jar(dir.path(), "alpha", "sodium.jar");

        let updated = install_into_alpha(
            dir.path(),
            mod_entry("sodium", ModSource::Modrinth, "", "sodium.jar"),
        );

        assert_eq!(updated.mods.len(), 1);
        assert_eq!(std::fs::read_to_string(&jar).unwrap(), "new bytes");
    }

    // On a case-insensitive filesystem both names are one file, and deleting
    // the "old" jar would delete the one just installed.
    #[test]
    fn install_local_file_keeps_the_new_jar_when_names_differ_only_by_case() {
        let dir = tempfile::tempdir().unwrap();
        seed_modpack(
            dir.path(),
            "alpha",
            vec![mod_entry(
                "394468",
                ModSource::CurseForge,
                "sodium",
                "Sodium.jar",
            )],
        );
        installed_jar(dir.path(), "alpha", "Sodium.jar");

        let updated = install_into_alpha(
            dir.path(),
            mod_entry("AANobbMI", ModSource::Modrinth, "sodium", "sodium.jar"),
        );

        assert_eq!(updated.mods.len(), 1);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("modpacks/alpha/sodium.jar")).unwrap(),
            "new bytes"
        );
    }

    // A jar the game holds open cannot be deleted on Windows. Its manifest
    // entry must survive so the next install still knows to remove it.
    #[cfg(windows)]
    #[test]
    fn install_local_file_keeps_the_entry_of_a_jar_it_could_not_delete() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        seed_modpack(
            dir.path(),
            "alpha",
            vec![mod_entry(
                "394468",
                ModSource::CurseForge,
                "sodium",
                "sodium-curseforge.jar",
            )],
        );
        let locked_jar = installed_jar(dir.path(), "alpha", "sodium-curseforge.jar");
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked_jar)
            .unwrap();
        let source_file = dir.path().join("download.jar");
        std::fs::write(&source_file, "new bytes").unwrap();
        let install = || {
            install_local_file(
                dir.path(),
                source_file.clone(),
                mod_entry(
                    "AANobbMI",
                    ModSource::Modrinth,
                    "sodium",
                    "sodium-modrinth.jar",
                ),
                ModType::Mod,
                Some("alpha".to_string()),
            )
        };

        assert!(install().is_err());
        let manifest: InstalledModpack = serde_json::from_reader(
            std::fs::File::open(dir.path().join("modpacks/alpha/modConfigV2.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.mods[0].id, "394468");

        drop(lock);
        let updated = install().unwrap().unwrap();
        assert_eq!(updated.mods.len(), 1);
        assert!(!locked_jar.exists());
    }

    #[test]
    fn install_local_file_replaces_same_slug_from_other_provider() {
        let dir = tempfile::tempdir().unwrap();
        seed_modpack(
            dir.path(),
            "alpha",
            vec![mod_entry(
                "394468",
                ModSource::CurseForge,
                "Sodium",
                "sodium-curseforge.jar",
            )],
        );
        let old_jar = installed_jar(dir.path(), "alpha", "sodium-curseforge.jar");

        let updated = install_into_alpha(
            dir.path(),
            mod_entry(
                "AANobbMI",
                ModSource::Modrinth,
                "sodium",
                "sodium-modrinth.jar",
            ),
        );

        assert_eq!(updated.mods.len(), 1);
        assert_eq!(updated.mods[0].id, "AANobbMI");
        assert!(!old_jar.exists());
        assert!(
            dir.path()
                .join("modpacks/alpha/sodium-modrinth.jar")
                .is_file()
        );
    }

    #[test]
    fn install_local_file_keeps_other_mods_from_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        seed_modpack(
            dir.path(),
            "alpha",
            vec![
                mod_entry("360438", ModSource::CurseForge, "lithium", "lithium.jar"),
                mod_entry("238222", ModSource::CurseForge, "", "jei.jar"),
            ],
        );
        let lithium_jar = installed_jar(dir.path(), "alpha", "lithium.jar");
        let jei_jar = installed_jar(dir.path(), "alpha", "jei.jar");

        let updated = install_into_alpha(
            dir.path(),
            mod_entry(
                "AANobbMI",
                ModSource::Modrinth,
                "sodium",
                "sodium-modrinth.jar",
            ),
        );

        assert_eq!(updated.mods.len(), 3);
        assert!(lithium_jar.is_file());
        assert!(jei_jar.is_file());
    }

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
        assert!(agent.starts_with("QuadrantMC/quadrant/v"));
        assert!(agent.contains(env!("CARGO_PKG_VERSION")));
    }

    struct NullSettings;

    impl crate::ports::SettingsStore for NullSettings {
        fn get_value(&self, _key: &str) -> Result<Option<serde_json::Value>> {
            Ok(None)
        }

        fn set_value(&self, _key: &str, _value: serde_json::Value) -> Result<()> {
            Ok(())
        }

        fn entries(&self) -> Result<Vec<(String, serde_json::Value)>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct CollectingEvents {
        events: std::sync::Mutex<Vec<BackendEvent>>,
    }

    impl EventSink for CollectingEvents {
        fn publish(&self, event: BackendEvent) -> Result<()> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    #[tokio::test]
    async fn install_mod_rejects_online_source_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let error = install_mod(
            &[dir.path().to_path_buf()],
            &NullSettings,
            &CollectingEvents::default(),
            "mod".to_string(),
            "1.20.1".to_string(),
            ModLoader::Fabric,
            ModSource::Online,
            Some("alpha".to_string()),
            ModType::Mod,
            None,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("unsupportedDownload"));
    }

    #[tokio::test]
    async fn install_remote_file_downloads_once_and_places_the_file_in_every_root() {
        let _guard = crate::mc_mod::cache::tests::set_cache_dir();
        let dir = tempfile::tempdir().unwrap();
        let roots = [dir.path().join("minecraft"), dir.path().join("instance")];
        let bytes = b"pack bytes";
        let server = httpmock::MockServer::start();
        let download = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/Faithful.zip");
            then.status(200).body(bytes);
        });
        let file = UniversalModFile {
            id: None,
            file_name: "Faithful.zip".to_string(),
            download_url: format!("{}/Faithful.zip", server.base_url()),
            sha1: crate::mc_mod::cache::file_hash(bytes),
            size: bytes.len() as u64,
        };
        let events = CollectingEvents::default();

        let updated = install_remote_file(
            &roots,
            &events,
            file.clone(),
            ModType::ResourcePack,
            None,
            ModSource::Online,
            "faithful".to_string(),
        )
        .await
        .unwrap();

        assert!(updated.is_none());
        download.assert_calls(1);
        for root in &roots {
            assert_eq!(
                std::fs::read(root.join("resourcepacks").join("Faithful.zip")).unwrap(),
                bytes
            );
        }

        let error = install_remote_file(
            &[],
            &events,
            file,
            ModType::ResourcePack,
            None,
            ModSource::Online,
            "faithful".to_string(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("errorInvalidRequest"));
        download.assert_calls(1);
    }

    #[tokio::test]
    async fn check_mod_updates_never_marks_online_mods_autoinstallable() {
        let online = Mod {
            name: "mod".to_string(),
            id: "mod".to_string(),
            download_count: 0,
            version: String::new(),
            date_modified: String::new(),
            mod_type: ModType::Mod,
            source: ModSource::Online,
            slug: String::new(),
            thumbnail_urls: Vec::new(),
            url: String::new(),
            description: String::new(),
            license: String::new(),
            mod_icon_url: String::new(),
            downloadable: false,
            show_previous_version: false,
            new_version: None,
            deleteable: false,
            autoinstallable: true,
            selectable: false,
            modpack: None,
            select_url: None,
        };
        let modpack = LocalModpack {
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: Vec::new(),
            unknown_mods: false,
            is_applied: false,
            last_synced: 0,
            modpack_id: None,
        };
        let checked = check_mod_updates(
            online,
            "1.20.1".to_string(),
            ModLoader::Fabric,
            modpack,
            true,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!checked.autoinstallable);
    }

    #[tokio::test]
    async fn get_file_redownloads_when_cached_file_is_missing() {
        let _guard = crate::mc_mod::cache::tests::set_cache_dir();
        let server = httpmock::MockServer::start();
        let download = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/mod.jar");
            then.status(200).body("fresh bytes");
        });
        let bytes = b"fresh bytes";
        let sha1 = crate::mc_mod::cache::file_hash(bytes);
        let cached =
            crate::mc_mod::cache::add_cache_index("mod.jar".to_string(), bytes, sha1.clone())
                .await
                .unwrap();
        std::fs::remove_file(&cached).unwrap();

        let (path, _) = get_file(
            UniversalModFile {
                id: None,
                file_name: "mod.jar".to_string(),
                download_url: format!("{}/mod.jar", server.base_url()),
                sha1,
                size: bytes.len() as u64,
            },
            "mod".to_string(),
            &CollectingEvents::default(),
        )
        .await
        .unwrap();

        download.assert();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[tokio::test]
    async fn get_file_reports_a_refused_download_without_blaming_sign_in() {
        let _guard = crate::mc_mod::cache::tests::set_cache_dir();
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/refused.jar");
            then.status(403);
        });

        let error = get_file(
            UniversalModFile {
                id: None,
                file_name: "refused.jar".to_string(),
                download_url: format!("{}/refused.jar", server.base_url()),
                sha1: "0000000000000000000000000000000000000000".to_string(),
                size: 1,
            },
            "mod".to_string(),
            &CollectingEvents::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            crate::error::user_facing(error).to_string(),
            "errorDownloadRefused"
        );
    }

    #[tokio::test]
    async fn get_file_rejects_a_file_without_a_download_url() {
        let events = CollectingEvents::default();

        let error = get_file(
            UniversalModFile {
                id: None,
                file_name: "mod.jar".to_string(),
                download_url: String::new(),
                sha1: String::new(),
                size: 0,
            },
            "mod".to_string(),
            &events,
        )
        .await
        .unwrap_err();

        assert_eq!(error.to_string(), "thirdPartyDownloadDisabled");
        assert!(events.events.lock().unwrap().is_empty());
    }
}
