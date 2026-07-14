use std::path::PathBuf;

use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::{
    Result,
    models::{LocalModpack, ModLoader, ModSource},
    modpacks::get_modpacks,
    ports::{EventSink, SettingsStore},
};

use super::http::provider_cached_client;
use super::{
    GetModArgs, IdentifiedMod, InstalledMod, Mod, ModType, SearchCategory, SearchModsArgs,
    UniversalModFile, get_file, get_mod_url,
};

pub(crate) fn modrinth_api_base() -> String {
    #[cfg(test)]
    if let Ok(base_url) = std::env::var("QUADRANT_TEST_MODRINTH_API_BASE") {
        return base_url;
    }

    "https://api.modrinth.com".to_string()
}

pub async fn search_mods_modrinth(args: SearchModsArgs) -> Result<Vec<Mod>> {
    let mod_type = ModType::from(args.mod_type.clone());

    // Facet groups are AND-ed together; identifiers within a single group are
    // OR-ed. `project_type` scopes the content type, loader/version/open-source
    // each constrain further, and the selected categories form one OR group.
    let mut facet_groups: Vec<String> = vec![format!("[\"project_type:{mod_type}\"]")];

    if !args.game_version.is_empty() && args.game_version != "any" {
        facet_groups.push(format!("[\"versions:{}\"]", args.game_version));
    }

    let loader = ModLoader::from(args.mod_loader.clone());
    if loader != ModLoader::Unknown {
        if let Some(loader_slug) = loader.modrinth_slug() {
            facet_groups.push(format!("[\"categories:{loader_slug}\"]"));
        }
    }

    if !args.categories.is_empty() {
        let inner = args
            .categories
            .iter()
            .map(|category| format!("\"categories:{category}\""))
            .collect::<Vec<_>>()
            .join(",");
        facet_groups.push(format!("[{inner}]"));
    }

    if args.open_source {
        facet_groups.push("[\"open_source:true\"]".to_string());
    }

    let facets_param = format!("[{}]", facet_groups.join(","));

    let index = match args.sort_by.as_str() {
        "downloads" => "downloads",
        "updated" => "updated",
        _ => "relevance",
    };

    let offset = args.offset.to_string();
    let raw_uri = reqwest::Url::parse_with_params(
        format!("{}/v2/search", modrinth_api_base()).as_str(),
        [
            ("query", args.query.as_str()),
            ("limit", "100"),
            ("offset", offset.as_str()),
            ("index", index),
            ("facets", facets_param.as_str()),
        ],
    )?;

    let response_json: serde_json::Value = provider_cached_client()
        .get(raw_uri)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut mods = Vec::new();
    if let Some(hits) = response_json["hits"].as_array() {
        for mod_data in hits {
            let mut screenshots = Vec::new();
            if let Some(gallery) = mod_data["gallery"].as_array() {
                for screenshot in gallery {
                    if let Some(url) = screenshot.as_str() {
                        screenshots.push(url.to_string());
                    }
                }
            }

            let icon = mod_data["icon_url"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("https://github.com/QuadrantMC/quadrant/raw/next/public/logoNoBg.png")
                .to_string();
            let slug = mod_data["slug"].as_str().unwrap_or_default().to_string();

            mods.push(Mod {
                name: mod_data["title"].as_str().unwrap_or_default().to_string(),
                id: mod_data["project_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                download_count: mod_data["downloads"].as_i64().unwrap_or_default(),
                version: String::new(),
                date_modified: mod_data["date_modified"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                mod_type,
                source: ModSource::Modrinth,
                slug: slug.clone(),
                thumbnail_urls: screenshots,
                url: get_mod_url(slug, mod_type, ModSource::Modrinth),
                description: mod_data["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                license: mod_data["license"].as_str().unwrap_or_default().to_string(),
                mod_icon_url: icon,
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

/// Fetches the Modrinth tag list and maps it into selectable search facets for
/// the given content type. Modrinth groups tags by `header` (categories,
/// resolutions, features, performance impact); loader tags are filtered out
/// because the loader is chosen separately.
pub async fn get_categories_modrinth(mod_type: ModType) -> Result<Vec<SearchCategory>> {
    let project_type = mod_type.to_string();

    let response_json: serde_json::Value = provider_cached_client()
        .get(format!("{}/v2/tag/category", modrinth_api_base()))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut categories = Vec::new();
    if let Some(tags) = response_json.as_array() {
        for tag in tags {
            if tag["project_type"].as_str().unwrap_or_default() != project_type {
                continue;
            }
            let slug = tag["name"].as_str().unwrap_or_default().to_string();
            if slug.is_empty() {
                continue;
            }
            let header = tag["header"].as_str().unwrap_or("categories").to_string();
            categories.push(SearchCategory {
                name: prettify_slug(&slug),
                id: slug,
                header,
                source: ModSource::Modrinth,
            });
        }
    }

    Ok(categories)
}

/// Turns a Modrinth tag slug (e.g. `game-mechanics`) into a display label
/// (`Game Mechanics`).
fn prettify_slug(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub async fn get_mod_modrinth(args: GetModArgs) -> Result<Mod> {
    let res_json: serde_json::Value = provider_cached_client()
        .get(format!("{}/v2/project/{}", modrinth_api_base(), args.id))
        .send()
        .await?
        .json()
        .await?;

    let mut screenshots = Vec::new();
    if let Some(gallery) = res_json["gallery"].as_array() {
        for screenshot in gallery {
            if let Some(url) = screenshot["url"].as_str() {
                screenshots.push(url.to_string());
            }
        }
    }

    let mod_type = match res_json["project_type"].as_str().unwrap_or_default() {
        "mod" => ModType::Mod,
        "resourcepack" => ModType::ResourcePack,
        "shader" => ModType::ShaderPack,
        _ => return Err(anyhow::anyhow!("Unsupported Mod Type")),
    };

    Ok(Mod {
        name: res_json["title"].as_str().unwrap_or_default().to_string(),
        id: args.id.clone(),
        download_count: res_json["downloads"].as_i64().unwrap_or_default(),
        version: res_json["versions"]
            .as_array()
            .and_then(|versions| versions.last())
            .and_then(|version| version.as_str())
            .unwrap_or_default()
            .to_string(),
        date_modified: res_json["updated"].as_str().unwrap_or_default().to_string(),
        mod_type,
        source: ModSource::Modrinth,
        slug: res_json["slug"].as_str().unwrap_or_default().to_string(),
        thumbnail_urls: screenshots,
        url: get_mod_url(
            res_json["slug"].as_str().unwrap_or_default().to_string(),
            mod_type,
            ModSource::Modrinth,
        ),
        description: res_json["description"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        license: res_json["license"]["name"]
            .as_str()
            .or_else(|| res_json["license"]["id"].as_str())
            .or_else(|| res_json["license"].as_str())
            .unwrap_or_default()
            .to_string(),
        mod_icon_url: res_json["icon_url"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
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

pub async fn get_mod_owners_modrinth(id: String) -> Result<Vec<String>> {
    let res_json: serde_json::Value = provider_cached_client()
        .get(format!("{}/v2/project/{}/members", modrinth_api_base(), id))
        .send()
        .await?
        .json()
        .await?;
    let mut owners = Vec::new();
    if let Some(members) = res_json.as_array() {
        for member in members {
            if let Some(username) = member["user"]["username"].as_str() {
                owners.push(username.to_string());
            }
        }
    }
    Ok(owners)
}

pub async fn get_mod_deps_modrinth(id: String) -> Result<Vec<Mod>> {
    let res_json: serde_json::Value = provider_cached_client()
        .get(format!(
            "{}/v2/project/{}/dependencies",
            modrinth_api_base(),
            id
        ))
        .send()
        .await?
        .json()
        .await?;
    let mut dependencies = Vec::new();
    if let Some(deps) = res_json["projects"].as_array() {
        for dependency in deps {
            if let Some(id) = dependency["id"].as_str() {
                dependencies.push(id.to_string());
            }
        }
    }

    let mut mods = Vec::new();
    for dependency in dependencies {
        mods.push(
            get_mod_modrinth(GetModArgs {
                deletable: false,
                id: dependency,
                downloadable: true,
                show_previous_version: false,
                version_target: String::new(),
                modpack: String::new(),
                mod_loader: ModLoader::Unknown,
                selectable: false,
                select_url: None,
            })
            .await?,
        );
    }

    Ok(mods)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModrinthVersion {
    pub date_published: DateTime<Utc>,
    pub files: Vec<ModrinthFile>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModrinthHash {
    pub sha512: String,
    pub sha1: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModrinthFile {
    pub hashes: ModrinthHash,
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub size: u64,
}

impl From<ModrinthFile> for UniversalModFile {
    fn from(value: ModrinthFile) -> Self {
        Self {
            id: None,
            file_name: value.filename,
            download_url: value.url,
            sha1: value.hashes.sha1,
            size: value.size,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModrinthVersionIdentifier {
    pub files: Vec<ModrinthFile>,
    pub project_id: String,
}

pub async fn get_latest_mod_version_modrinth(
    id: String,
    minecraft_version: String,
    mod_loader: ModLoader,
    mod_type: ModType,
) -> Result<Option<ModrinthFile>> {
    let mut query = vec![("game_versions", format!("[\"{}\"]", minecraft_version))];
    if mod_type == ModType::Mod {
        if let Some(loader_slug) = mod_loader.modrinth_slug() {
            query.push(("loaders", format!("[\"{}\"]", loader_slug)));
        }
    }
    let url = reqwest::Url::parse_with_params(
        format!("{}/v2/project/{}/version", modrinth_api_base(), id).as_str(),
        query.iter().map(|(key, value)| (*key, value.as_str())),
    )?;

    let response = provider_cached_client()
        .get(url)
        .send()
        .await?
        .error_for_status()?;

    let mut res_json: Vec<ModrinthVersion> = serde_json::from_str(&response.text().await?)?;
    res_json.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    if res_json.is_empty() {
        return Err(anyhow::anyhow!("noVersion"));
    }
    Ok(res_json[0]
        .files
        .iter()
        .find(|file| file.primary)
        .or_else(|| res_json[0].files.first())
        .cloned())
}

pub async fn download_mod_modrinth(
    settings: &impl SettingsStore,
    event_sink: &impl EventSink,
    id: String,
    minecraft_version: String,
    mod_loader: ModLoader,
    mod_type: ModType,
) -> Result<(PathBuf, String)> {
    log::info!("Downloading Modrinth mod {id} for {minecraft_version} ({mod_loader:?})");
    let file = get_latest_mod_version_modrinth(id.clone(), minecraft_version, mod_loader, mod_type)
        .await?
        .ok_or_else(|| anyhow::anyhow!("noVersion"))?;
    let current_usage = settings.get_i64("modrinthUsage")?.unwrap_or_default();
    settings.set_i64("modrinthUsage", current_usage + 1)?;
    get_file(file.into(), id, event_sink).await
}

pub async fn identify_modpack_modrinth(
    mc_folder: &PathBuf,
    modpack: String,
) -> Result<Vec<IdentifiedMod>> {
    log::info!("Identifying Modrinth mods in modpack \"{modpack}\"");
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

    let mut mods = Vec::new();
    for file in unknown_files {
        let original_file_name = file.clone();
        let file = modpack_folder.join(file);
        let mut hasher = Sha1::new();
        hasher.update(std::fs::read(file)?);
        let hash = hex::encode(hasher.finalize());
        let res = provider_cached_client()
            .get(format!("{}/v2/version_file/{}", modrinth_api_base(), hash))
            .send()
            .await?;
        let identifier = match res.json::<ModrinthVersionIdentifier>().await {
            Ok(identifier) => identifier,
            Err(_) => continue,
        };
        let file = match identifier
            .files
            .iter()
            .find(|file| file.hashes.sha1 == hash)
        {
            Some(file) => file,
            None => continue,
        };
        mods.push(IdentifiedMod {
            installed_mod: InstalledMod::minimal(
                identifier.project_id,
                ModSource::Modrinth,
                file.url.clone(),
            ),
            file_name: original_file_name,
        });
    }

    Ok(mods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mc_mod::http::{PROVIDER_HTTP_TEST_MUTEX, clear_provider_http_cache},
        models::{InstalledModpack, ModLoader},
    };
    use httpmock::prelude::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn modrinth_search_args(query: &str) -> SearchModsArgs {
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

    fn modrinth_get_mod_args(id: &str) -> GetModArgs {
        GetModArgs {
            id: id.to_string(),
            downloadable: true,
            show_previous_version: false,
            deletable: false,
            version_target: String::new(),
            mod_loader: ModLoader::Fabric,
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
                mod_loader: ModLoader::Fabric,
                mods: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn get_mod_modrinth_uses_shared_http_cache() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let project = server.mock(|when, then| {
            when.method(GET).path("/v2/project/demo-mod");
            then.status(200)
                .header("cache-control", "public, max-age=300")
                .json_body(json!({
                    "title": "Demo Mod",
                    "downloads": 42,
                    "versions": ["1.0.0"],
                    "project_type": "mod",
                    "slug": "demo-mod",
                    "description": "cached",
                    "license": "MIT",
                    "icon_url": "https://example.invalid/icon.png",
                    "gallery": [{ "url": "https://example.invalid/screenshot.png" }]
                }));
        });

        let args = modrinth_get_mod_args("demo-mod");
        get_mod_modrinth(args.clone()).await.unwrap();
        get_mod_modrinth(args).await.unwrap();

        project.assert_calls(1);

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn identify_modpack_modrinth_uses_shared_http_cache() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let temp_dir = tempdir().unwrap();
        let mc_folder = temp_dir.path().join(".minecraft");
        let modpack_dir = mc_folder.join("modpacks").join("alpha");
        setup_modpack(&mc_folder, "alpha");

        let file_bytes = b"modrinth-jar";
        let mut hasher = Sha1::new();
        hasher.update(file_bytes);
        let hash = hex::encode(hasher.finalize());
        std::fs::write(modpack_dir.join("unknown.jar"), file_bytes).unwrap();

        let version_lookup = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/v2/version_file/{hash}").as_str());
            then.status(200)
                .header("cache-control", "public, max-age=300")
                .json_body(json!({
                    "files": [{
                        "hashes": {
                            "sha512": "unused",
                            "sha1": hash
                        },
                        "url": "https://example.invalid/mod.jar",
                        "filename": "unknown.jar",
                        "primary": true,
                        "size": 12
                    }],
                    "project_id": "demo-project"
                }));
        });

        let first = identify_modpack_modrinth(&mc_folder, "alpha".to_string())
            .await
            .unwrap();
        let second = identify_modpack_modrinth(&mc_folder, "alpha".to_string())
            .await
            .unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        version_lookup.assert_calls(1);

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn search_mods_modrinth_serializes_query_and_facets() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let search = server.mock(|when, then| {
            when.method(GET)
                .path("/v2/search")
                .query_param("query", "sodium")
                .query_param("limit", "100")
                .query_param("offset", "20")
                .query_param("index", "downloads")
                .query_param(
                    "facets",
                    concat!(
                        "[[\"project_type:mod\"],[\"versions:1.20.1\"],",
                        "[\"categories:fabric\"],",
                        "[\"categories:optimization\",\"categories:utility\"],",
                        "[\"open_source:true\"]]"
                    ),
                );
            then.status(200).json_body(json!({
                "hits": [{
                    "title": "Sodium",
                    "project_id": "AANobbMI",
                    "downloads": 1000,
                    "date_modified": "2024-01-01T00:00:00Z",
                    "slug": "sodium",
                    "description": "fast",
                    "license": "LGPL-3.0",
                    "icon_url": "https://example.invalid/icon.png",
                    "gallery": ["https://example.invalid/screenshot.png"]
                }]
            }));
        });

        let mut args = modrinth_search_args("sodium");
        args.game_version = "1.20.1".to_string();
        args.mod_loader = "Fabric".to_string();
        args.categories = vec!["optimization".to_string(), "utility".to_string()];
        args.open_source = true;
        args.sort_by = "downloads".to_string();
        args.offset = 20;

        let mods = search_mods_modrinth(args).await.unwrap();
        search.assert();

        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "Sodium");
        assert_eq!(mods[0].id, "AANobbMI");
        assert_eq!(mods[0].slug, "sodium");
        assert_eq!(mods[0].source, ModSource::Modrinth);
        assert_eq!(mods[0].download_count, 1000);
        assert_eq!(
            mods[0].thumbnail_urls,
            vec!["https://example.invalid/screenshot.png".to_string()]
        );

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn search_mods_modrinth_errors_on_http_error_status() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/v2/search");
            then.status(500).body("search backend exploded");
        });

        let error = search_mods_modrinth(modrinth_search_args("broken"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("500"));

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn search_mods_modrinth_errors_on_malformed_json() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/v2/search");
            then.status(200)
                .header("content-type", "application/json")
                .body("{not-json");
        });

        search_mods_modrinth(modrinth_search_args("malformed"))
            .await
            .unwrap_err();

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn get_latest_mod_version_modrinth_prefers_newest_primary_file() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let versions = server.mock(|when, then| {
            when.method(GET)
                .path("/v2/project/demo-mod/version")
                .query_param("game_versions", "[\"1.20.1\"]")
                .query_param("loaders", "[\"fabric\"]");
            then.status(200).json_body(json!([
                {
                    "date_published": "2024-01-01T00:00:00Z",
                    "files": [{
                        "hashes": { "sha512": "unused", "sha1": "old" },
                        "url": "https://example.invalid/old.jar",
                        "filename": "old.jar",
                        "primary": true,
                        "size": 1
                    }]
                },
                {
                    "date_published": "2024-06-01T00:00:00Z",
                    "files": [
                        {
                            "hashes": { "sha512": "unused", "sha1": "extra" },
                            "url": "https://example.invalid/extra.jar",
                            "filename": "extra.jar",
                            "primary": false,
                            "size": 2
                        },
                        {
                            "hashes": { "sha512": "unused", "sha1": "new" },
                            "url": "https://example.invalid/new.jar",
                            "filename": "new.jar",
                            "primary": true,
                            "size": 3
                        }
                    ]
                }
            ]));
        });

        let file = get_latest_mod_version_modrinth(
            "demo-mod".to_string(),
            "1.20.1".to_string(),
            ModLoader::Fabric,
            ModType::Mod,
        )
        .await
        .unwrap()
        .unwrap();

        versions.assert();
        assert_eq!(file.filename, "new.jar");
        assert_eq!(file.hashes.sha1, "new");

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn get_latest_mod_version_modrinth_errors_when_no_versions() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/v2/project/empty-mod/version");
            then.status(200).json_body(json!([]));
        });

        let error = get_latest_mod_version_modrinth(
            "empty-mod".to_string(),
            "1.20.1".to_string(),
            ModLoader::Fabric,
            ModType::Mod,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("noVersion"));

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }

    #[tokio::test]
    async fn get_categories_modrinth_maps_and_filters_tags() {
        let _guard = PROVIDER_HTTP_TEST_MUTEX.lock().await;
        clear_provider_http_cache().await;

        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_TEST_MODRINTH_API_BASE", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/v2/tag/category");
            then.status(200).json_body(json!([
                { "name": "game-mechanics", "project_type": "mod", "header": "categories" },
                { "name": "optimization", "project_type": "mod" },
                { "name": "32x", "project_type": "resourcepack", "header": "resolutions" },
                { "name": "", "project_type": "mod", "header": "categories" }
            ]));
        });

        let categories = get_categories_modrinth(ModType::Mod).await.unwrap();

        assert_eq!(categories.len(), 2);
        assert_eq!(categories[0].id, "game-mechanics");
        assert_eq!(categories[0].name, "Game Mechanics");
        assert_eq!(categories[0].header, "categories");
        assert_eq!(categories[0].source, ModSource::Modrinth);
        assert_eq!(categories[1].id, "optimization");
        assert_eq!(categories[1].name, "Optimization");
        assert_eq!(categories[1].header, "categories");

        unsafe {
            std::env::remove_var("QUADRANT_TEST_MODRINTH_API_BASE");
        }
        clear_provider_http_cache().await;
    }
}
