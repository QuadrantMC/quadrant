//! Config defaults and path resolution helpers for `quadrant-core`.

use crate::{Result, ports::SettingsStore};
use chrono::{Days, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Typed representation of the persisted `config.json` data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// Whether to visually clip mod or modpack icons in the UI.
    pub clip_icons: bool,
    /// RFC3339 timestamp of the last successful RSS fetch.
    pub last_rss_fetched: String,
    /// Whether CurseForge integrations are enabled.
    pub curseforge: bool,
    /// Whether Modrinth integrations are enabled.
    pub modrinth: bool,
    /// Usage counter for CurseForge-backed downloads.
    pub curseforge_usage: i64,
    /// Usage counter for Modrinth-backed downloads.
    pub modrinth_usage: i64,
    /// Whether developer-oriented functionality is enabled.
    pub dev_mode: bool,
    /// Stable installation identifier used for telemetry and sharing.
    pub hardware_id: String,
    /// Whether news feeds should be shown.
    pub rss_feeds: bool,
    /// Whether news should avoid interruptive presentation.
    pub silent_news: bool,
    /// Whether automatic Quadrant sync is enabled.
    pub auto_quadrant_sync: bool,
    /// Whether modpack update notifications should be displayed.
    pub show_modpack_update_notifications: bool,
    /// Whether to show mods that cannot currently be upgraded.
    pub show_unupgradeable_mods: bool,
    /// Last selected page index in the current app navigation.
    pub last_page: i64,
    /// UI scale percentage applied by the frontend (100 = default).
    pub ui_scale: i64,
    /// Whether experimental features are enabled.
    pub experimental_features: bool,
    /// Whether the user has dismissed the data collection recommendation.
    pub dont_show_user_data_recommendation: bool,
    /// Cache retention window in days.
    pub cache_keep_alive: i64,
    /// Whether settings sync is enabled.
    pub sync_settings: bool,
    /// RFC3339 timestamp of the last local settings update.
    pub last_settings_updated: String,
    /// Resolved Minecraft root folder path.
    pub mc_folder: String,
    /// Whether the user opted into telemetry and related data collection.
    pub collect_user_data: bool,
}

/// Typed representation of the persisted update configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Selected update channel, such as `stable`.
    pub channel: String,
}

/// Resolves the base directory used to derive the Minecraft folder.
pub fn get_config_dir() -> Result<Option<PathBuf>> {
    #[cfg(target_os = "windows")]
    {
        Ok(dirs::config_dir())
    }
    #[cfg(target_os = "macos")]
    {
        Ok(dirs::data_dir())
    }
    #[cfg(target_os = "linux")]
    {
        Ok(dirs::home_dir())
    }
}

/// Resolves the default Minecraft folder for the current platform.
pub fn get_mc_folder() -> Result<Option<PathBuf>> {
    let mut path = get_config_dir()?;
    if path.is_none() {
        path = Some(PathBuf::from(".minecraft"));
    }
    let mut path = path.expect("mc folder path should always resolve");
    #[cfg(target_os = "macos")]
    path.push("minecraft");
    #[cfg(not(target_os = "macos"))]
    path.push(".minecraft");
    Ok(Some(path))
}

/// Builds the default application configuration used for first-run bootstrap.
pub fn default_app_config() -> AppConfig {
    let fourteen_days_ago = Utc::now().checked_sub_days(Days::new(14)).unwrap();

    AppConfig {
        clip_icons: true,
        last_rss_fetched: fourteen_days_ago.to_rfc3339(),
        curseforge: true,
        modrinth: true,
        curseforge_usage: 0,
        modrinth_usage: 0,
        dev_mode: false,
        hardware_id: Uuid::now_v7().to_string(),
        rss_feeds: true,
        silent_news: false,
        auto_quadrant_sync: true,
        show_modpack_update_notifications: true,
        show_unupgradeable_mods: false,
        last_page: 0,
        ui_scale: 100,
        experimental_features: false,
        dont_show_user_data_recommendation: false,
        cache_keep_alive: 30,
        sync_settings: true,
        // A fresh installation has no local user edits. Using the epoch lets
        // an existing cloud profile win the first synchronization instead of
        // uploading defaults with a misleadingly fresh timestamp.
        last_settings_updated: "1970-01-01T00:00:00+00:00".to_string(),
        mc_folder: get_mc_folder()
            .ok()
            .flatten()
            .and_then(|path| path.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| ".minecraft".to_string()),
        collect_user_data: cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )),
    }
}

/// Builds the default updater configuration.
pub fn default_update_config() -> UpdateConfig {
    UpdateConfig {
        channel: "stable".to_string(),
    }
}

/// Ensures all expected app config keys exist in the provided settings store.
///
/// Existing keys are preserved. Missing keys are materialized with the current
/// defaults used by Quadrant.
pub fn ensure_default_app_config(store: &impl SettingsStore) -> Result<()> {
    log::info!("Ensuring default app config is materialized");
    let defaults = default_app_config();

    ensure_bool(store, "clipIcons", defaults.clip_icons)?;
    ensure_string(store, "lastRSSfetched", defaults.last_rss_fetched)?;
    ensure_bool(store, "curseforge", defaults.curseforge)?;
    ensure_bool(store, "modrinth", defaults.modrinth)?;
    ensure_i64(store, "curseforgeUsage", defaults.curseforge_usage)?;
    ensure_i64(store, "modrinthUsage", defaults.modrinth_usage)?;
    ensure_bool(store, "devMode", defaults.dev_mode)?;
    ensure_string(store, "hardwareId", defaults.hardware_id)?;
    ensure_bool(store, "rssFeeds", defaults.rss_feeds)?;
    ensure_bool(store, "silentNews", defaults.silent_news)?;
    ensure_bool(store, "autoQuadrantSync", defaults.auto_quadrant_sync)?;
    ensure_bool(
        store,
        "showModpackUpdateNotifications",
        defaults.show_modpack_update_notifications,
    )?;
    ensure_bool(
        store,
        "showUnupgradeableMods",
        defaults.show_unupgradeable_mods,
    )?;
    ensure_i64(store, "lastPage", defaults.last_page)?;
    ensure_i64(store, "uiScale", defaults.ui_scale)?;
    ensure_bool(
        store,
        "experimentalFeatures",
        defaults.experimental_features,
    )?;
    ensure_bool(
        store,
        "dontShowUserDataRecommendation",
        defaults.dont_show_user_data_recommendation,
    )?;
    ensure_i64(store, "cacheKeepAlive", defaults.cache_keep_alive)?;
    ensure_bool(store, "syncSettings", defaults.sync_settings)?;
    ensure_string(store, "lastSettingsUpdated", defaults.last_settings_updated)?;
    ensure_string(store, "mcFolder", defaults.mc_folder)?;
    ensure_bool(store, "collectUserData", defaults.collect_user_data)?;
    Ok(())
}

fn ensure_bool(store: &impl SettingsStore, key: &str, default: bool) -> Result<()> {
    if store.get_bool(key)?.is_none() {
        store.set_bool(key, default)?;
    }
    Ok(())
}

fn ensure_i64(store: &impl SettingsStore, key: &str, default: i64) -> Result<()> {
    if store.get_i64(key)?.is_none() {
        store.set_i64(key, default)?;
    }
    Ok(())
}

fn ensure_string(store: &impl SettingsStore, key: &str, default: String) -> Result<()> {
    if store.get_string(key)?.is_none() {
        store.set_string(key, default)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::SettingsStore;
    use serde_json::Value;
    use std::{cell::RefCell, collections::HashMap};

    struct MemoryStore {
        values: RefCell<HashMap<String, Value>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                values: RefCell::new(HashMap::new()),
            }
        }
    }

    impl SettingsStore for MemoryStore {
        fn get_value(&self, key: &str) -> Result<Option<Value>> {
            Ok(self.values.borrow().get(key).cloned())
        }

        fn set_value(&self, key: &str, value: Value) -> Result<()> {
            self.values.borrow_mut().insert(key.to_string(), value);
            Ok(())
        }

        fn entries(&self) -> Result<Vec<(String, Value)>> {
            Ok(self
                .values
                .borrow()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect())
        }
    }

    #[test]
    fn app_config_defaults_are_materialized() {
        let store = MemoryStore::new();
        ensure_default_app_config(&store).unwrap();

        assert_eq!(store.get_bool("clipIcons").unwrap(), Some(true));
        assert_eq!(store.get_bool("curseforge").unwrap(), Some(true));
        assert_eq!(
            store.get_bool("showModpackUpdateNotifications").unwrap(),
            Some(true)
        );
        assert_eq!(store.get_i64("cacheKeepAlive").unwrap(), Some(30));
        assert_eq!(
            store.get_string("lastSettingsUpdated").unwrap().as_deref(),
            Some("1970-01-01T00:00:00+00:00")
        );
        assert!(store.get_string("hardwareId").unwrap().is_some());
        let mc_folder = store.get_string("mcFolder").unwrap().unwrap();
        #[cfg(target_os = "macos")]
        assert!(mc_folder.ends_with("/Library/Application Support/minecraft"));
        #[cfg(not(target_os = "macos"))]
        assert!(mc_folder.ends_with(".minecraft"));
    }

    #[test]
    fn ensure_default_app_config_is_idempotent() {
        let store = MemoryStore::new();
        ensure_default_app_config(&store).unwrap();
        let mut first = store.entries().unwrap();
        first.sort_by(|a, b| a.0.cmp(&b.0));
        assert!(!first.is_empty());

        ensure_default_app_config(&store).unwrap();
        let mut second = store.entries().unwrap();
        second.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(first, second);
    }

    #[test]
    fn ensure_default_app_config_preserves_existing_values() {
        let store = MemoryStore::new();
        store.set_bool("clipIcons", false).unwrap();
        store.set_i64("uiScale", 150).unwrap();
        store
            .set_string("hardwareId", "fixed-id".to_string())
            .unwrap();
        store
            .set_string("mcFolder", "/custom/minecraft".to_string())
            .unwrap();

        ensure_default_app_config(&store).unwrap();

        assert_eq!(store.get_bool("clipIcons").unwrap(), Some(false));
        assert_eq!(store.get_i64("uiScale").unwrap(), Some(150));
        assert_eq!(
            store.get_string("hardwareId").unwrap().as_deref(),
            Some("fixed-id")
        );
        assert_eq!(
            store.get_string("mcFolder").unwrap().as_deref(),
            Some("/custom/minecraft")
        );
        assert_eq!(store.get_bool("modrinth").unwrap(), Some(true));
        assert_eq!(store.get_i64("lastPage").unwrap(), Some(0));
    }

    #[test]
    fn mc_folder_resolves_to_minecraft_path() {
        let mc_folder = get_mc_folder().unwrap().unwrap();
        #[cfg(target_os = "macos")]
        assert!(
            mc_folder
                .to_string_lossy()
                .ends_with("/Library/Application Support/minecraft")
        );
        #[cfg(not(target_os = "macos"))]
        assert!(mc_folder.to_string_lossy().ends_with(".minecraft"));
    }
}
