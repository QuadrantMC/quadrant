pub const BASE_URL: &str = "https://api.curseforge.com";
pub const MINECRAFT_ID: i32 = 432;

use std::path::PathBuf;
use std::time::Duration;

use chrono::prelude::*;
use moka::future::Cache as MokaCache;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha1::Digest;

use crate::{
    Result,
    models::{InstalledMod, LocalModpack, ModLoader, ModSource},
    modpacks::get_modpacks,
    ports::{EventSink, SettingsStore},
};

use super::{
    GetModArgs, IdentifiedMod, Mod, ModType, SearchCategory, SearchModsArgs, UniversalModFile,
    get_file, get_mod_url,
};
use crate::mc_mod::curseforge_fingerprint::*;
use crate::mc_mod::http::{provider_cached_client, provider_http_client};

pub(crate) fn curseforge_api_base() -> String {
    #[cfg(test)]
    if let Ok(base_url) = std::env::var("QUADRANT_TEST_CURSEFORGE_API_BASE") {
        return base_url;
    }

    BASE_URL.to_string()
}

static CURSEFORGE_FINGERPRINT_CACHE: Lazy<MokaCache<String, ExactMatchesResponse>> =
    Lazy::new(|| {
        MokaCache::builder()
            .max_capacity(128)
            .time_to_live(Duration::from_secs(600))
            .build()
    });

#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct ModFilesResponse {
    data: Vec<ModFile>,
}

#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct ModFileResponse {
    data: ModFile,
}

#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct ExactMatchesResponse {
    data: ExactMatches,
}

#[derive(Serialize, Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub id: u64,
    pub game_id: u64,
    pub mod_id: u64,
    pub is_available: bool,
    pub file_name: String,
    pub hashes: Vec<Hash>,
    pub file_date: String,
    pub file_length: u64,
    pub download_url: Option<String>,
}

impl From<ModFile> for UniversalModFile {
    fn from(value: ModFile) -> Self {
        Self {
            id: Some(value.id.to_string()),
            file_name: value.file_name,
            download_url: value.download_url.unwrap_or_default(),
            sha1: value
                .hashes
                .iter()
                .find(|hash| hash.algo == 1)
                .map(|hash| hash.value.clone())
                .unwrap_or_default(),
            size: value.file_length,
        }
    }
}

#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct Hash {
    pub value: String,
    pub algo: u8,
}

#[derive(Serialize, Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExactMatches {
    pub exact_matches: Vec<ExactMatch>,
}

#[derive(Serialize, Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExactMatch {
    pub file: ModFile,
}

fn fingerprint_cache_key(fingerprints: &[u32]) -> String {
    let mut fingerprints = fingerprints.to_vec();
    fingerprints.sort_unstable();
    let fingerprints = fingerprints
        .into_iter()
        .map(|fingerprint| fingerprint.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("{MINECRAFT_ID}:{fingerprints}")
}

pub async fn get_mod_curseforge(args: GetModArgs) -> Result<Mod> {
    let res_json: serde_json::Value = provider_cached_client()
        .get(format!("{}/v1/mods/{}", curseforge_api_base(), args.id))
        .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
        .send()
        .await?
        .json()
        .await?;
    let res_data = &res_json["data"];
    let mod_class =
        ModType::from_curseforge_class(res_data["classId"].as_i64().unwrap_or_default());
    let screenshots: Vec<String> = res_data["screenshots"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|screenshot| {
            screenshot["thumbnailUrl"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    let logo = res_data["logo"]["url"]
        .as_str()
        .unwrap_or("https://github.com/QuadrantMC/quadrant/raw/next/public/logoNoBg.png")
        .to_string();
    Ok(Mod {
        id: args.id,
        name: res_data["name"].as_str().unwrap_or_default().to_string(),
        description: res_data["summary"].as_str().unwrap_or_default().to_string(),
        download_count: res_data["downloadCount"].as_i64().unwrap_or_default(),
        version: res_data["dateModified"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        date_modified: res_data["dateModified"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        mod_type: mod_class,
        source: ModSource::CurseForge,
        slug: res_data["slug"].as_str().unwrap_or_default().to_string(),
        thumbnail_urls: screenshots,
        url: res_data["links"]["websiteUrl"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        license: "Unknown".to_string(),
        mod_icon_url: logo,
        downloadable: args.downloadable,
        show_previous_version: args.show_previous_version,
        new_version: None,
        deleteable: args.deletable,
        autoinstallable: false,
        selectable: args.selectable,
        select_url: args.select_url,
        modpack: Some(args.modpack),
    })
}

pub async fn get_mod_owners_curseforge(id: String) -> Result<Vec<String>> {
    let res_json: serde_json::Value = provider_cached_client()
        .get(format!("{}/v1/mods/{}", curseforge_api_base(), id))
        .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
        .send()
        .await?
        .json()
        .await?;
    let mut owners = Vec::new();
    if let Some(authors) = res_json["data"]["authors"].as_array() {
        for author in authors {
            if let Some(username) = author["name"].as_str() {
                owners.push(username.to_string());
            }
        }
    }
    Ok(owners)
}

pub async fn get_mod_deps_curseforge(id: String) -> Result<Vec<Mod>> {
    let res_json: serde_json::Value = provider_cached_client()
        .get(format!("{}/v1/mods/{}", curseforge_api_base(), id))
        .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
        .send()
        .await?
        .json()
        .await?;
    let mods_to_get: Vec<String> = res_json["data"]["latestFiles"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|file| file["dependencies"].as_array().into_iter().flatten())
        .filter_map(|mod_info| {
            if let (Some(mod_info_id), Some(relation_type)) = (
                mod_info["modId"].as_u64(),
                mod_info["relationType"].as_i64(),
            ) && relation_type == 3
            {
                return Some(mod_info_id.to_string());
            }
            None
        })
        .collect();

    let mut mods = Vec::new();
    for mod_to_get in mods_to_get {
        mods.push(
            get_mod_curseforge(GetModArgs {
                id: mod_to_get,
                show_previous_version: false,
                downloadable: true,
                deletable: false,
                mod_loader: ModLoader::Unknown,
                version_target: String::new(),
                modpack: String::new(),
                selectable: false,
                select_url: None,
            })
            .await?,
        );
    }
    Ok(mods)
}

pub async fn search_mods_curseforge(args: SearchModsArgs) -> Result<Vec<Mod>> {
    let mod_type = ModType::from(args.mod_type.clone());

    let (sort_field, sort_order) = match args.sort_by.as_str() {
        "downloads" => ("6", "desc"), // TotalDownloads
        "updated" => ("3", "desc"),   // LastUpdated
        "name" => ("4", "asc"),       // Name
        _ => ("2", "desc"),           // Popularity (relevance default)
    };

    let mut query = vec![
        ("gameId", MINECRAFT_ID.to_string()),
        ("searchFilter", args.query.clone()),
        ("sortField", sort_field.to_string()),
        ("sortOrder", sort_order.to_string()),
        ("classId", mod_type.curseforge_id().to_string()),
        // Zero-based pagination offset. CurseForge caps `index + pageSize` at
        // 10000, and `pageSize` itself at 50.
        ("index", args.offset.to_string()),
        ("pageSize", "50".to_string()),
    ];

    let has_concrete_game_version = !args.game_version.is_empty() && args.game_version != "any";

    if has_concrete_game_version {
        let mut game_version = args.game_version.clone();
        // Resource packs, shaders and data packs are indexed on CurseForge by
        // the major.minor version only.
        if mod_type != ModType::Mod && mod_type != ModType::Modpack {
            let trimmed_version = game_version.split('.').collect::<Vec<&str>>();
            if trimmed_version.len() >= 2 {
                game_version = format!("{}.{}", trimmed_version[0], trimmed_version[1]);
            }
        }
        query.push(("gameVersion", game_version));
    }

    if has_concrete_game_version && (mod_type == ModType::Mod || mod_type == ModType::Modpack) {
        let mod_loader_type = ModLoader::from(args.mod_loader.clone());
        if mod_loader_type != ModLoader::Unknown {
            let Some(curseforge_id) = mod_loader_type.curseforge_id() else {
                return Ok(Vec::new());
            };
            query.push(("modLoaderType", curseforge_id.to_string()));
        }
    }

    if !args.categories.is_empty() {
        let ids = args
            .categories
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(",");
        query.push(("categoryIds", format!("[{ids}]")));
    }
    // CurseForge search exposes no open-source facet; that filter is
    // Modrinth-only and the frontend restricts the source set accordingly.

    let raw_uri = reqwest::Url::parse_with_params(
        format!("{}/v1/mods/search", curseforge_api_base()).as_str(),
        query.iter().map(|(key, value)| (*key, value.as_str())),
    )?;

    let response_json: serde_json::Value = provider_cached_client()
        .get(raw_uri)
        .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
        .send()
        .await?
        .json()
        .await?;

    let mut mods = Vec::new();
    if let Some(data) = response_json["data"].as_array() {
        for mod_data in data {
            mods.push(Mod {
                id: mod_data["id"].as_i64().unwrap_or_default().to_string(),
                name: mod_data["name"].as_str().unwrap_or_default().to_string(),
                description: mod_data["summary"].as_str().unwrap_or_default().to_string(),
                download_count: mod_data["downloadCount"].as_i64().unwrap_or_default(),
                version: mod_data["dateModified"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                date_modified: mod_data["dateModified"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                mod_type: ModType::from_curseforge_class(
                    mod_data["classId"].as_i64().unwrap_or_default(),
                ),
                source: ModSource::CurseForge,
                slug: mod_data["slug"].as_str().unwrap_or_default().to_string(),
                thumbnail_urls: mod_data["screenshots"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|screenshot| {
                        screenshot["thumbnailUrl"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string()
                    })
                    .collect(),
                url: get_mod_url(
                    mod_data["slug"].as_str().unwrap_or_default().to_string(),
                    ModType::from_curseforge_class(
                        mod_data["classId"].as_i64().unwrap_or_default(),
                    ),
                    ModSource::CurseForge,
                ),
                license: "Unknown".to_string(),
                mod_icon_url: mod_data["logo"]["url"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                downloadable: true,
                show_previous_version: false,
                new_version: None,
                deleteable: false,
                autoinstallable: args.filter_on,
                selectable: false,
                select_url: None,
                modpack: None,
            });
        }
    }
    Ok(mods)
}

/// Fetches the CurseForge category list for a content type and maps it into
/// selectable search facets. CurseForge exposes numeric category ids (passed
/// back verbatim as `categoryIds`); the class rows and non-leaf groupings are
/// skipped so only user-facing categories remain.
pub async fn get_categories_curseforge(mod_type: ModType) -> Result<Vec<SearchCategory>> {
    let class_id = mod_type.curseforge_id();
    let raw_uri = reqwest::Url::parse_with_params(
        format!("{}/v1/categories", curseforge_api_base()).as_str(),
        [
            ("gameId", MINECRAFT_ID.to_string()),
            ("classId", class_id.to_string()),
        ]
        .iter()
        .map(|(key, value)| (*key, value.as_str())),
    )?;

    let response_json: serde_json::Value = provider_cached_client()
        .get(raw_uri)
        .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut categories = Vec::new();
    if let Some(data) = response_json["data"].as_array() {
        for category in data {
            // Skip the class row itself; only real categories are selectable.
            if category["isClass"].as_bool().unwrap_or(false) {
                continue;
            }
            let id = category["id"].as_i64().unwrap_or_default();
            let name = category["name"].as_str().unwrap_or_default().to_string();
            if id == 0 || name.is_empty() {
                continue;
            }
            categories.push(SearchCategory {
                id: id.to_string(),
                name,
                header: "categories".to_string(),
                source: ModSource::CurseForge,
            });
        }
    }

    Ok(categories)
}

pub async fn get_latest_mod_version_curseforge(
    id: String,
    minecraft_version: String,
    mod_loader: ModLoader,
    mod_type: ModType,
    file_id: Option<String>,
) -> Result<Option<ModFile>> {
    let mut url = format!("{}/v1/mods/{}/files", curseforge_api_base(), id);
    let mut query = vec![("gameVersion", minecraft_version)];
    if let Some(file_id) = file_id.clone() {
        // Explicit file installs do not use modLoaderType, so preserve them even if
        // the last selected UI loader is Modrinth-only.
        url = format!("{}/{}", url, file_id);
    } else if mod_type == ModType::Mod && mod_loader != ModLoader::Unknown {
        let Some(curseforge_id) = mod_loader.curseforge_id() else {
            return Ok(None);
        };
        query.push(("modLoaderType", curseforge_id.to_string()));
    }

    let response = if file_id.is_some() {
        provider_cached_client()
            .get(&url)
            .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
            .send()
            .await?
    } else {
        let url = reqwest::Url::parse_with_params(
            url.as_str(),
            query.iter().map(|(key, value)| (*key, value.as_str())),
        )?;
        provider_cached_client()
            .get(url)
            .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
            .send()
            .await?
    };

    Ok(if file_id.is_some() {
        Some(response.json::<ModFileResponse>().await?.data)
    } else {
        let mut data = response.json::<ModFilesResponse>().await?.data;
        if data.is_empty() {
            return Err(anyhow::anyhow!("noVersion"));
        }
        data.sort_by(|a, b| {
            let date_a = DateTime::parse_from_rfc3339(&a.file_date).ok();
            let date_b = DateTime::parse_from_rfc3339(&b.file_date).ok();
            date_b.cmp(&date_a)
        });
        data.first().cloned()
    })
}

pub async fn download_mod_curseforge(
    settings: &impl SettingsStore,
    event_sink: &impl EventSink,
    id: String,
    minecraft_version: String,
    mod_loader: ModLoader,
    mod_type: ModType,
    file_id: Option<String>,
) -> Result<(PathBuf, String)> {
    log::info!(
        "Downloading CurseForge mod {id} for {minecraft_version} ({mod_loader:?}, file_id={file_id:?})"
    );
    let file = get_latest_mod_version_curseforge(
        id.clone(),
        minecraft_version,
        mod_loader,
        mod_type,
        file_id,
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("noVersion"))?;
    let current_usage = settings.get_i64("curseforgeUsage")?.unwrap_or_default();
    settings.set_i64("curseforgeUsage", current_usage + 1)?;
    get_file(file.into(), id, event_sink).await
}

pub async fn identify_modpack_curseforge(
    mc_folder: &PathBuf,
    modpack: String,
) -> Result<Vec<IdentifiedMod>> {
    log::info!("Identifying CurseForge mods in modpack \"{modpack}\"");
    let modpack_folder = mc_folder.join("modpacks").join(&modpack);
    let existing_modpack: Vec<LocalModpack> = get_modpacks(mc_folder, false)
        .await?
        .into_iter()
        .filter(|existing| existing.name == modpack)
        .collect();
    if existing_modpack.is_empty() {
        return Err(anyhow::anyhow!("Modpack doesn't exist"));
    }
    let existing_modpack = existing_modpack.first().unwrap();
    let existing_files: Vec<String> = existing_modpack
        .mods
        .iter()
        .map(|mod_| {
            urlencoding::decode(&mod_.download_url)
                .unwrap_or_default()
                .to_string()
                .split('/')
                .last()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    let unknown_files: Vec<String> = std::fs::read_dir(&modpack_folder)?
        .filter(|file| match file {
            Ok(file) => {
                let file_name = file.file_name().to_string_lossy().to_string();
                !existing_files.contains(&file_name) && file_name.ends_with(".jar")
            }
            Err(_) => false,
        })
        .map(|file| file.unwrap().file_name().to_string_lossy().to_string())
        .collect();

    let mut hashes = Vec::new();
    for file in unknown_files {
        let file = modpack_folder.join(file);
        let contents = get_jar_contents(file.to_string_lossy().as_ref());
        let hash = compute_hash(&contents);
        let mut hasher = sha1::Sha1::new();
        hasher.update(contents);
        let sha1 = hex::encode(hasher.finalize());
        hashes.push((
            hash,
            sha1,
            file.file_name().unwrap().to_string_lossy().to_string(),
        ));
    }

    let fingerprints: Vec<u32> = hashes.iter().map(|hash| hash.0).collect();
    let cache_key = fingerprint_cache_key(&fingerprints);

    let response = match CURSEFORGE_FINGERPRINT_CACHE.get(&cache_key).await {
        Some(response) => response,
        None => {
            let response = provider_http_client()
                .post(format!(
                    "{}/v1/fingerprints/{}",
                    curseforge_api_base(),
                    MINECRAFT_ID
                ))
                .header("X-API-Key", env!("ETERNAL_API_TOKEN"))
                .json(&serde_json::json!({
                    "fingerprints": fingerprints,
                }))
                .send()
                .await?
                .json::<ExactMatchesResponse>()
                .await?;
            CURSEFORGE_FINGERPRINT_CACHE
                .insert(cache_key, response.clone())
                .await;
            response
        }
    };
    let matches = response.data.exact_matches;

    let mut mods = Vec::new();
    for match_ in matches {
        let file = match_.file;
        let Some(hash) = file.hashes.iter().find(|hash| hash.algo == 1) else {
            continue;
        };
        let Some(original_file) = hashes
            .iter()
            .find(|(_, sha1, _)| sha1 == &hash.value)
            .map(|hash_info| hash_info.2.clone())
        else {
            continue;
        };
        let Some(download_url) = file.download_url else {
            continue;
        };
        mods.push(IdentifiedMod {
            installed_mod: InstalledMod::minimal(
                file.mod_id.to_string(),
                ModSource::CurseForge,
                download_url,
            ),
            file_name: original_file,
        });
    }
    Ok(mods)
}

#[cfg(test)]
pub(crate) async fn clear_curseforge_fingerprint_cache() {
    CURSEFORGE_FINGERPRINT_CACHE.invalidate_all();
    CURSEFORGE_FINGERPRINT_CACHE.run_pending_tasks().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc_mod::http::{PROVIDER_HTTP_TEST_MUTEX, clear_provider_http_cache};
    use crate::models::{InstalledModpack, ModLoader};
    use httpmock::prelude::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn curseforge_search_args(query: &str) -> SearchModsArgs {
        SearchModsArgs {
            query: query.to_string(),
            mod_type: "mod".to_string(),
            filter_on: false,
            game_version: String::new(),
            mod_loader: String::new(),
            categories: Vec::new(),
            open_source: false,
            sort_by: "relevance".to_string(),
            offset: 0,
        }
    }

    fn curseforge_get_mod_args(id: &str) -> GetModArgs {
        GetModArgs {
            id: id.to_string(),
            downloadable: true,
            show_previous_version: false,
            deletable: false,
            version_target: String::new(),
            mod_loader: ModLoader::Forge,
            modpack: String::new(),
            selectable: false,
            select_url: None,
        }
    }

    fn setup_modpack(mc_folder: &std::path::Path, name: &str) {
        let modpack_dir = mc_folder.join("modpacks").join(name);
        std::fs::create_dir_all(&modpack_dir).unwrap();
        std::fs::write(
            modpack_dir.join("modConfigV2.json"),
            serde_json::to_vec(&InstalledModpack {
                mod_config_version: String::new(),
                quadrant_version: String::new(),
                name: name.to_string(),
                version: "1.20.1".to_string(),
                mod_loader: ModLoader::Forge,
                mods: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn get_mod_curseforge_uses_shared_http_cache() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CURSEFORGE_API_BASE", server.base_url());
        }

        let project = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/mods/42")
                .header("x-api-key", env!("ETERNAL_API_TOKEN"));
            then.status(200)
                .header("cache-control", "public, max-age=300")
                .json_body(json!({
                    "data": {
                        "id": 42,
                        "classId": 6,
                        "name": "Demo CurseForge Mod",
                        "summary": "cached",
                        "downloadCount": 5,
                        "dateModified": "2024-01-01T00:00:00Z",
                        "slug": "demo-curseforge-mod",
                        "screenshots": [{
                            "thumbnailUrl": "https://example.invalid/screenshot.png"
                        }],
                        "links": {
                            "websiteUrl": "https://example.invalid/mod"
                        },
                        "logo": {
                            "url": "https://example.invalid/logo.png"
                        }
                    }
                }));
        });

        let args = curseforge_get_mod_args("42");
        get_mod_curseforge(args.clone()).await.unwrap();
        get_mod_curseforge(args).await.unwrap();

        project.assert_calls(1);

        unsafe {
            std::env::remove_var("QUADRANT_TEST_CURSEFORGE_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn identify_modpack_curseforge_uses_manual_fingerprint_cache() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;
        clear_curseforge_fingerprint_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CURSEFORGE_API_BASE", server.base_url());
        }

        let temp_dir = tempdir().unwrap();
        let mc_folder = temp_dir.path().join(".minecraft");
        let modpack_dir = mc_folder.join("modpacks").join("alpha");
        setup_modpack(&mc_folder, "alpha");

        let file_bytes = b"curseforge-jar";
        std::fs::write(modpack_dir.join("unknown.jar"), file_bytes).unwrap();
        let sha1 = hex::encode(sha1::Sha1::digest(file_bytes));

        let fingerprint = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/fingerprints/432")
                .header("x-api-key", env!("ETERNAL_API_TOKEN"));
            then.status(200).json_body(json!({
                "data": {
                    "exactMatches": [{
                        "file": {
                            "id": 1,
                            "gameId": 432,
                            "modId": 123,
                            "isAvailable": true,
                            "fileName": "unknown.jar",
                            "hashes": [{
                                "value": sha1,
                                "algo": 1
                            }],
                            "fileDate": "2024-01-01T00:00:00+00:00",
                            "fileLength": 15,
                            "downloadUrl": "https://example.invalid/mod.jar"
                        }
                    }]
                }
            }));
        });

        let first = identify_modpack_curseforge(&mc_folder, "alpha".to_string())
            .await
            .unwrap();
        let second = identify_modpack_curseforge(&mc_folder, "alpha".to_string())
            .await
            .unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        fingerprint.assert_calls(1);

        unsafe {
            std::env::remove_var("QUADRANT_TEST_CURSEFORGE_API_BASE");
        }
        clear_curseforge_fingerprint_cache().await;
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn search_mods_curseforge_serializes_query_and_pagination_params() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CURSEFORGE_API_BASE", server.base_url());
        }

        let search = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/mods/search")
                .header("x-api-key", env!("ETERNAL_API_TOKEN"))
                .query_param("gameId", "432")
                .query_param("searchFilter", "jei")
                .query_param("sortField", "3")
                .query_param("sortOrder", "desc")
                .query_param("classId", "6")
                .query_param("index", "50")
                .query_param("pageSize", "50")
                .query_param("gameVersion", "1.20.1")
                .query_param("modLoaderType", "1")
                .query_param("categoryIds", "[423,5191]");
            then.status(200).json_body(json!({
                "data": [{
                    "id": 238222,
                    "name": "Just Enough Items",
                    "summary": "item viewer",
                    "downloadCount": 999,
                    "dateModified": "2024-01-01T00:00:00Z",
                    "classId": 6,
                    "slug": "jei",
                    "screenshots": [{
                        "thumbnailUrl": "https://example.invalid/screenshot.png"
                    }],
                    "logo": {
                        "url": "https://example.invalid/logo.png"
                    }
                }]
            }));
        });

        let mut args = curseforge_search_args("jei");
        args.game_version = "1.20.1".to_string();
        args.mod_loader = "Forge".to_string();
        args.categories = vec!["423".to_string(), "5191".to_string()];
        args.sort_by = "updated".to_string();
        args.offset = 50;

        let mods = search_mods_curseforge(args).await.unwrap();
        search.assert();

        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].id, "238222");
        assert_eq!(mods[0].name, "Just Enough Items");
        assert_eq!(mods[0].slug, "jei");
        assert_eq!(mods[0].source, ModSource::CurseForge);
        assert_eq!(mods[0].mod_type, ModType::Mod);

        unsafe {
            std::env::remove_var("QUADRANT_TEST_CURSEFORGE_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn search_mods_curseforge_errors_on_http_error_status() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CURSEFORGE_API_BASE", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/v1/mods/search");
            then.status(500)
                .header("content-type", "text/plain")
                .body("search backend exploded");
        });

        search_mods_curseforge(curseforge_search_args("broken"))
            .await
            .unwrap_err();

        unsafe {
            std::env::remove_var("QUADRANT_TEST_CURSEFORGE_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn get_latest_mod_version_curseforge_sorts_files_by_date() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CURSEFORGE_API_BASE", server.base_url());
        }

        let files = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/mods/77/files")
                .header("x-api-key", env!("ETERNAL_API_TOKEN"))
                .query_param("gameVersion", "1.20.1")
                .query_param("modLoaderType", "1");
            then.status(200).json_body(json!({
                "data": [
                    {
                        "id": 1,
                        "gameId": 432,
                        "modId": 77,
                        "isAvailable": true,
                        "fileName": "old.jar",
                        "hashes": [{ "value": "old", "algo": 1 }],
                        "fileDate": "2024-01-01T00:00:00+00:00",
                        "fileLength": 1,
                        "downloadUrl": "https://example.invalid/old.jar"
                    },
                    {
                        "id": 2,
                        "gameId": 432,
                        "modId": 77,
                        "isAvailable": true,
                        "fileName": "new.jar",
                        "hashes": [{ "value": "new", "algo": 1 }],
                        "fileDate": "2024-06-01T00:00:00+00:00",
                        "fileLength": 2,
                        "downloadUrl": "https://example.invalid/new.jar"
                    }
                ]
            }));
        });

        let file = get_latest_mod_version_curseforge(
            "77".to_string(),
            "1.20.1".to_string(),
            ModLoader::Forge,
            ModType::Mod,
            None,
        )
        .await
        .unwrap()
        .unwrap();

        files.assert();
        assert_eq!(file.id, 2);
        assert_eq!(file.file_name, "new.jar");

        unsafe {
            std::env::remove_var("QUADRANT_TEST_CURSEFORGE_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn get_latest_mod_version_curseforge_uses_explicit_file_id() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CURSEFORGE_API_BASE", server.base_url());
        }

        let file_lookup = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/mods/77/files/999")
                .header("x-api-key", env!("ETERNAL_API_TOKEN"));
            then.status(200).json_body(json!({
                "data": {
                    "id": 999,
                    "gameId": 432,
                    "modId": 77,
                    "isAvailable": true,
                    "fileName": "pinned.jar",
                    "hashes": [{ "value": "pinned", "algo": 1 }],
                    "fileDate": "2024-03-01T00:00:00+00:00",
                    "fileLength": 3,
                    "downloadUrl": "https://example.invalid/pinned.jar"
                }
            }));
        });

        let file = get_latest_mod_version_curseforge(
            "77".to_string(),
            "1.20.1".to_string(),
            ModLoader::Forge,
            ModType::Mod,
            Some("999".to_string()),
        )
        .await
        .unwrap()
        .unwrap();

        file_lookup.assert();
        assert_eq!(file.id, 999);
        assert_eq!(file.file_name, "pinned.jar");

        unsafe {
            std::env::remove_var("QUADRANT_TEST_CURSEFORGE_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn get_categories_curseforge_skips_class_rows_and_invalid_entries() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CURSEFORGE_API_BASE", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/categories")
                .header("x-api-key", env!("ETERNAL_API_TOKEN"))
                .query_param("gameId", "432")
                .query_param("classId", "6");
            then.status(200).json_body(json!({
                "data": [
                    { "id": 6, "name": "Mods", "isClass": true },
                    { "id": 423, "name": "Map and Information" },
                    { "id": 0, "name": "Broken" },
                    { "id": 5, "name": "" }
                ]
            }));
        });

        let categories = get_categories_curseforge(ModType::Mod).await.unwrap();

        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].id, "423");
        assert_eq!(categories[0].name, "Map and Information");
        assert_eq!(categories[0].header, "categories");
        assert_eq!(categories[0].source, ModSource::CurseForge);

        unsafe {
            std::env::remove_var("QUADRANT_TEST_CURSEFORGE_API_BASE");
        }
        clear_provider_http_cache().await;
    }
}
