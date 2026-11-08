use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Once},
    time::Duration,
};

pub mod app_paths;
#[cfg(feature = "single-instance")]
pub mod single_instance;
pub mod updates;

use anyhow::{Context, anyhow};
use futures::StreamExt;
use keyring_core::{Entry, Error as KeyringError, set_default_store};
use quadrant_core::{
    Error, Result,
    account::{
        KEYRING_SERVICE, account_token_needs_refresh, clear_account_token,
        id::{
            AccountInfo, Notification, NotificationCursor, NotificationWsFrame,
            get_account_info_with_refresh, get_notification_history_all_since_with_refresh,
            oauth2_login, read_notification, revoke_session, try_refresh_token,
        },
        quadrant_settings_sync::{self, SettingsPull},
        quadrant_share::{
            QuadrantShareSubmissionResponse, get_quadrant_share_modpack, share_modpack_raw,
        },
        quadrant_sync::{
            SyncedModpack, answer_invite, delete_synced_modpack, get_synced_modpacks,
            invite_member, kick_member,
        },
    },
    config::{ensure_default_app_config, get_mc_folder},
    content::{
        ContentLocation, ContentLocationKind, LocationRef, MINECRAFT_LOCATION_ID, content_location,
        content_location_header, content_subfolder, copy_content_files, delete_content_files,
        parse_location_id, prism_location_id,
    },
    error::{is_cloud_sync_conflict, is_signed_out},
    events::BackendEvent,
    mc_mod::{
        GetModArgs, GlobalSearchModsArgs, IdentifiedMod, MinecraftVersion, Mod, ModType,
        SearchCategory, UniversalModFile, check_mod_updates, get_categories, get_user_agent,
        get_versions, identify_modpack, install_mod, install_remote_file, search_mods,
    },
    models::{Article, InstalledMod, InstalledModpack, LocalModpack, ModLoader, ModSource},
    modpacks::{
        apply_modpack, backfill_mod_hashes, check_manifest_readable, create_modpack, delete_mod,
        delete_modpack, export_modpack_to, get_modpack, get_modpacks, install_modpack,
        register_mod, review_incoming_mods, set_modpack_sync_date, update_modpack,
    },
    ports::{EventSink, SecretStore, SettingsStore},
    prism::{self, PrismInstance, PrismSyncPlan},
    telemetry::{AppInfo, get_telemetry_info, remove_telemetry, send_telemetry},
};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tokio::{
    sync::{Mutex as AsyncMutex, broadcast},
    task::JoinHandle,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use url::Url;
use uuid::Uuid;

pub use quadrant_core::error::{ErrorCode, user_facing};
#[cfg(feature = "curseforge")]
pub use quadrant_core::mc_mod::curseforge::{
    get_mod_curseforge, get_mod_deps_curseforge, get_mod_owners_curseforge,
};
pub use quadrant_core::mc_mod::modrinth::{
    get_mod_deps_modrinth, get_mod_modrinth, get_mod_owners_modrinth,
};
pub use quadrant_core::mc_mod::{get_mod_url, get_user_url};

/// How a game directory is turned into a listing entry, which is either with
/// its installed files or without reading them.
type DescribeLocation = fn(&str, ContentLocationKind, &str, &Path) -> ContentLocation;

const NOTIFICATION_CURSOR_CREATED_AT_KEY: &str = "notificationCursorCreatedAt";
const NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY: &str = "notificationCursorNotificationId";
const SETTINGS_SYNC_INTERVAL_SECS: u64 = 120;
/// How often the background worker checks whether the access token is due for a
/// proactive refresh. Refresh timing is governed by the stored deadline, not
/// this interval, so a short poll just bounds how late a refresh can fire.
const TOKEN_REFRESH_CHECK_INTERVAL_SECS: u64 = 60;
const WS_REPLAY_LIMIT: usize = 500;
const NOTIFICATION_HISTORY_LIMIT: usize = WS_REPLAY_LIMIT;
/// How long signing out waits for the server before it deletes the tokens anyway.
const REVOKE_TIMEOUT: Duration = Duration::from_secs(5);
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// The API pings every 30 seconds, so this much silence means the connection
/// is gone even though no error reached the socket.
const WS_READ_TIMEOUT: Duration = Duration::from_secs(90);
const REFRESH_SYNCED_MODPACKS_EVENT: &str = "refreshSyncedModpacks";
const MODPACK_SYNC_WARNING_EVENT: &str = "modpackSyncWarning";
const MODPACK_SYNC_NOTIFICATION_TYPE: &str = "modpack_sync";
const MODPACK_MEMBERSHIP_NOTIFICATION_TYPE: &str = "modpack_membership";
static LOGGER_INIT: Once = Once::new();
/// Only success is remembered: the Secret Service can come up after an app
/// that starts with the session, and a later call should find it.
static KEYRING_STORE_READY: Mutex<bool> = Mutex::new(false);

#[derive(Debug, Clone)]
pub struct QuadrantHostOptions {
    pub data_dir: PathBuf,
    pub mc_folder: Option<PathBuf>,
    pub api_base_url: Option<String>,
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub quadrant_api_key: String,
    pub config_store_name: String,
    pub update_store_name: String,
    pub keyring_service_name: String,
    pub app_version: String,
    pub os_name: String,
    pub user_agent: String,
}

impl QuadrantHostOptions {
    pub fn new(
        data_dir: PathBuf,
        oauth_client_id: impl Into<String>,
        oauth_client_secret: impl Into<String>,
        quadrant_api_key: impl Into<String>,
    ) -> Self {
        Self {
            data_dir,
            mc_folder: None,
            api_base_url: None,
            oauth_client_id: oauth_client_id.into(),
            oauth_client_secret: oauth_client_secret.into(),
            quadrant_api_key: quadrant_api_key.into(),
            config_store_name: "config.json".to_string(),
            update_store_name: "updateConfig.json".to_string(),
            keyring_service_name: KEYRING_SERVICE.to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os_name: std::env::consts::OS.to_uppercase(),
            user_agent: get_user_agent(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostEventEnvelope {
    pub event: String,
    pub payload: Value,
}

#[derive(Clone)]
pub struct QuadrantHost {
    inner: Arc<QuadrantHostInner>,
}

struct QuadrantHostInner {
    options: QuadrantHostOptions,
    config_store: JsonFileStore,
    _update_store: JsonFileStore,
    secret_store: KeyringSecretStore,
    event_sink: HostEventBridge,
    runtime_state: AsyncMutex<HostRuntimeState>,
    worker_handles: AsyncMutex<WorkerHandles>,
}

#[derive(Default)]
struct WorkerHandles {
    notification: Option<JoinHandle<()>>,
    settings_sync: Option<JoinHandle<()>>,
    token_refresh: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct HostRuntimeState {
    updated_modpacks: Vec<String>,
    notification_connection_id: String,
    notification_state: NotificationRuntimeState,
}

#[derive(Clone)]
struct JsonFileStore {
    path: PathBuf,
    cached: Arc<Mutex<CachedValues>>,
}

/// The values as last read or written, with the file's stamp at that time.
#[derive(Default)]
struct CachedValues {
    values: HashMap<String, Value>,
    stamp: Option<FileStamp>,
}

/// What a file's metadata says about its contents. `None` when the file
/// does not exist.
type FileStamp = Option<(std::time::SystemTime, u64)>;

/// A file changed this recently may change again within the timestamp
/// granularity of some file systems, so its stamp proves nothing yet.
const STAMP_SETTLE_TIME: Duration = Duration::from_secs(2);

impl JsonFileStore {
    fn new(path: PathBuf) -> Result<Self> {
        let store = Self {
            path,
            cached: Arc::new(Mutex::new(CachedValues::default())),
        };
        let values = store.load_or_set_aside()?;
        store.lock_cached()?.values = values;
        Ok(store)
    }

    fn lock_cached(&self) -> Result<std::sync::MutexGuard<'_, CachedValues>> {
        self.cached
            .lock()
            .map_err(|_| anyhow::Error::from(quadrant_core::error::ErrorCode::Busy))
    }

    /// The values on disk. A file that does not parse (a bad hand edit) is
    /// moved aside to `<name>.corrupt-<unix ms>` and the store starts empty, so
    /// the host fills in defaults instead of failing to start.
    fn load_or_set_aside(&self) -> Result<HashMap<String, Value>> {
        let error = match Self::read_values_from_disk(&self.path) {
            Ok(values) => return Ok(values),
            Err(error) if error.downcast_ref::<serde_json::Error>().is_some() => error,
            Err(error) => return Err(error),
        };
        let _lock = self.lock_file()?;
        // Another process starting at the same time may have done this already.
        match Self::read_values_from_disk(&self.path) {
            Ok(values) => return Ok(values),
            Err(error) if error.downcast_ref::<serde_json::Error>().is_some() => {}
            Err(error) => return Err(error),
        }
        let file_name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let backup = self.path.with_file_name(format!(
            "{file_name}.corrupt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        fs::rename(&self.path, &backup)?;
        log::error!(
            "{error:#}. Moved it to {} and started from the defaults",
            backup.display()
        );
        Ok(HashMap::new())
    }

    fn read_values_from_disk(path: &Path) -> Result<HashMap<String, Value>> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(HashMap::new());
            }
            Err(error) => return Err(error.into()),
        };
        serde_json::from_str::<HashMap<String, Value>>(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    /// Held around every reload-modify-persist, so a write from another
    /// process sharing the data folder is never overwritten with an older copy.
    fn lock_file(&self) -> Result<fs::File> {
        let mut lock_path = self.path.clone().into_os_string();
        lock_path.push(".lock");
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(quadrant_core::modpacks::lock_file(Path::new(&lock_path))?)
    }

    fn stamp(&self) -> Result<FileStamp> {
        match fs::metadata(&self.path) {
            Ok(metadata) => Ok(Some((metadata.modified()?, metadata.len()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn reload(&self, cached: &mut CachedValues) -> Result<()> {
        // The stamp is taken first, so a write landing during the read leaves
        // a stamp that no longer matches and the next read loads it.
        let stamp = self.stamp()?;
        // Do not let a failed reload turn the next write into an overwrite of
        // disk contents using a stale in-memory snapshot.
        cached.values = Self::read_values_from_disk(&self.path)?;
        cached.stamp = Some(stamp);
        Ok(())
    }

    /// Reloads only when the file changed since it was last read or written,
    /// so the many settings reads per command don't each parse config.json.
    fn refresh(&self, cached: &mut CachedValues) -> Result<()> {
        let stamp = self.stamp()?;
        let settled = stamp.is_none_or(|(modified, _)| {
            modified.elapsed().is_ok_and(|age| age >= STAMP_SETTLE_TIME)
        });
        if settled && cached.stamp == Some(stamp) {
            return Ok(());
        }
        self.reload(cached)
    }

    fn delete_key(&self, key: &str) -> Result<()> {
        let mut cached = self.lock_cached()?;
        let _lock = self.lock_file()?;
        self.reload(&mut cached)?;
        cached.values.remove(key);
        self.persist(&mut cached)
    }

    fn persist(&self, cached: &mut CachedValues) -> Result<()> {
        quadrant_core::modpacks::write_file_atomically(
            &self.path,
            serde_json::to_vec_pretty(&Value::Object(cached.values.clone().into_iter().collect()))?,
        )?;
        cached.stamp = Some(self.stamp()?);
        Ok(())
    }
}

impl SettingsStore for JsonFileStore {
    fn get_value(&self, key: &str) -> Result<Option<Value>> {
        let mut cached = self.lock_cached()?;
        self.refresh(&mut cached)?;
        Ok(cached.values.get(key).cloned())
    }

    fn set_value(&self, key: &str, value: Value) -> Result<()> {
        let mut cached = self.lock_cached()?;
        let _lock = self.lock_file()?;
        self.reload(&mut cached)?;
        cached.values.insert(key.to_string(), value);
        self.persist(&mut cached)
    }

    fn entries(&self) -> Result<Vec<(String, Value)>> {
        let mut cached = self.lock_cached()?;
        self.refresh(&mut cached)?;
        Ok(cached
            .values
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect())
    }
}

#[derive(Clone)]
struct KeyringSecretStore {
    service_name: String,
}

impl SecretStore for KeyringSecretStore {
    fn get_secret(&self, key: &str) -> Result<Option<String>> {
        ensure_keyring_store()?;
        let entry = Entry::new(&self.service_name, key)?;
        map_keyring_secret_result(entry.get_password())
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        ensure_keyring_store()?;
        let entry = Entry::new(&self.service_name, key)?;
        entry.set_password(value)?;
        Ok(())
    }

    fn delete_secret(&self, key: &str) -> Result<()> {
        ensure_keyring_store()?;
        let entry = Entry::new(&self.service_name, key)?;
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn ensure_keyring_store() -> Result<()> {
    ensure_initialized(&KEYRING_STORE_READY, init_keyring_store)
}

fn ensure_initialized<E: std::fmt::Display>(
    ready: &Mutex<bool>,
    init: impl FnOnce() -> std::result::Result<(), E>,
) -> Result<()> {
    let mut ready = ready
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !*ready {
        init().map_err(|error| anyhow!("failed to initialize keyring store: {error}"))?;
        *ready = true;
    }
    Ok(())
}

fn init_keyring_store() -> std::result::Result<(), KeyringError> {
    #[cfg(target_os = "windows")]
    {
        set_default_store(windows_native_keyring_store::Store::new()?);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        set_default_store(apple_native_keyring_store::keychain::Store::new()?);
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        set_default_store(dbus_secret_service_keyring_store::Store::new()?);
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(KeyringError::NotSupportedByStore(
        "no production keyring store is configured for this platform".to_string(),
    ))
}

fn map_keyring_secret_result(
    result: std::result::Result<String, KeyringError>,
) -> Result<Option<String>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone)]
struct HostEventBridge {
    sender: broadcast::Sender<HostEventEnvelope>,
}

impl HostEventBridge {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    fn subscribe(&self) -> broadcast::Receiver<HostEventEnvelope> {
        self.sender.subscribe()
    }

    fn emit<T: Serialize>(&self, event: impl Into<String>, payload: T) -> Result<()> {
        let _ = self.sender.send(HostEventEnvelope {
            event: event.into(),
            payload: serde_json::to_value(payload)?,
        });
        Ok(())
    }

    fn emit_value(&self, event: impl Into<String>, payload: Value) {
        let _ = self.sender.send(HostEventEnvelope {
            event: event.into(),
            payload,
        });
    }
}

impl EventSink for HostEventBridge {
    fn publish(&self, event: BackendEvent) -> Result<()> {
        match event {
            BackendEvent::ModDownloadProgress(payload) => self.emit("modDownloadProgress", payload),
            BackendEvent::ModInstallProgress(payload) => self.emit("modInstallProgress", payload),
            BackendEvent::ModpackDownloadProgress(payload) => {
                self.emit("modpackDownloadProgress", payload)
            }
            BackendEvent::QuadrantExportProgress(payload) => {
                self.emit("quadrantExportProgress", payload)
            }
            BackendEvent::QuadrantShareSubmission(payload) => {
                self.emit_value("quadrantShareSubmission", payload);
                Ok(())
            }
            BackendEvent::RefreshNotifications(payload) => {
                self.emit_value("refreshNotifications", payload);
                Ok(())
            }
            BackendEvent::RecheckAccountToken => self.emit("recheckAccountToken", Value::Null),
            BackendEvent::ConfigChanged(key) => self.emit("configChanged", key),
            BackendEvent::ModpackSyncWarning {
                modpack_id,
                name,
                applied,
                online_mods,
                unverified_mods,
            } => self.emit(
                MODPACK_SYNC_WARNING_EVENT,
                serde_json::json!({
                    "modpackId": modpack_id,
                    "name": name,
                    "applied": applied,
                    "onlineMods": online_mods,
                    "unverifiedMods": unverified_mods,
                }),
            ),
        }
    }
}

impl QuadrantHost {
    pub fn new(options: QuadrantHostOptions) -> Result<Self> {
        LOGGER_INIT.call_once(|| {
            colog::init();
        });
        fs::create_dir_all(&options.data_dir)?;
        quadrant_core::account::set_backend_base_url_override(options.api_base_url.as_deref())?;

        let config_store = JsonFileStore::new(options.data_dir.join(&options.config_store_name))?;
        let update_store = JsonFileStore::new(options.data_dir.join(&options.update_store_name))?;
        ensure_default_app_config(&config_store)?;

        quadrant_core::models::set_quadrant_version(options.app_version.clone());

        if let Some(mc_folder) = &options.mc_folder {
            config_store.set_string("mcFolder", mc_folder.to_string_lossy().to_string())?;
        }

        Ok(Self {
            inner: Arc::new(QuadrantHostInner {
                secret_store: KeyringSecretStore {
                    service_name: options.keyring_service_name.clone(),
                },
                event_sink: HostEventBridge::new(),
                runtime_state: AsyncMutex::new(HostRuntimeState {
                    notification_connection_id: Uuid::now_v7().to_string(),
                    ..HostRuntimeState::default()
                }),
                worker_handles: AsyncMutex::new(WorkerHandles::default()),
                config_store,
                _update_store: update_store,
                options,
            }),
        })
    }

    pub fn options(&self) -> &QuadrantHostOptions {
        &self.inner.options
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<HostEventEnvelope> {
        self.inner.event_sink.subscribe()
    }

    pub async fn start_background_workers(&self) -> Result<()> {
        let mut worker_handles = self.inner.worker_handles.lock().await;

        if worker_handles.notification.is_none() {
            let host = self.clone();
            worker_handles.notification = Some(tokio::spawn(async move {
                host.notification_worker_loop().await;
            }));
        }

        if worker_handles.settings_sync.is_none() {
            let host = self.clone();
            worker_handles.settings_sync = Some(tokio::spawn(async move {
                host.settings_sync_loop().await;
            }));
        }

        if worker_handles.token_refresh.is_none() {
            let host = self.clone();
            worker_handles.token_refresh = Some(tokio::spawn(async move {
                host.token_refresh_loop().await;
            }));
        }

        Ok(())
    }

    pub async fn stop_background_workers(&self) -> Result<()> {
        let mut worker_handles = self.inner.worker_handles.lock().await;

        if let Some(handle) = worker_handles.notification.take() {
            handle.abort();
            let _ = handle.await;
        }

        if let Some(handle) = worker_handles.settings_sync.take() {
            handle.abort();
            let _ = handle.await;
        }

        if let Some(handle) = worker_handles.token_refresh.take() {
            handle.abort();
            let _ = handle.await;
        }

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.stop_background_workers().await
    }

    pub fn init_config(&self) -> Result<()> {
        ensure_default_app_config(&self.inner.config_store)?;
        self.ensure_override_mc_folder()
    }

    pub fn get_config_value(&self, key: &str) -> Result<Option<Value>> {
        self.inner.config_store.get_value(key)
    }

    pub fn set_config_value(&self, key: &str, value: Value) -> Result<()> {
        self.inner.config_store.set_value(key, value)
    }

    /// Drops a config key entirely, so a later read falls back to the default
    /// rather than seeing a stored `null`.
    pub fn remove_config_value(&self, key: &str) -> Result<()> {
        self.inner.config_store.delete_key(key)
    }

    pub fn get_minecraft_folder(&self) -> Result<PathBuf> {
        self.inner
            .config_store
            .get_string("mcFolder")?
            .map(PathBuf::from)
            .or_else(|| get_mc_folder().ok().flatten())
            .ok_or_else(|| anyhow::Error::from(quadrant_core::error::ErrorCode::NoMinecraftFolder))
    }

    pub fn get_modpacks_folder(&self) -> Result<PathBuf> {
        Ok(self.get_minecraft_folder()?.join("modpacks"))
    }

    pub async fn get_modpacks(&self, hide_free: bool) -> Result<Vec<LocalModpack>> {
        get_modpacks(&self.get_minecraft_folder()?, hide_free).await
    }

    /// The listing of just the pack `name` (empty when it does not exist), for
    /// the core functions that look one pack up in a listing.
    async fn modpack_listing(&self, name: &str) -> Result<Vec<LocalModpack>> {
        Ok(get_modpack(&self.get_minecraft_folder()?, name)
            .await?
            .into_iter()
            .collect())
    }

    /// Records the file hashes older manifests lack. Hosts run it in the
    /// background at every launch; see [`backfill_mod_hashes`].
    pub async fn backfill_mod_hashes(&self) -> Result<usize> {
        backfill_mod_hashes(&self.get_minecraft_folder()?).await
    }

    /// Prunes the shared mod cache by the `cacheKeepAlive` setting (days).
    /// Hosts run it once in the background at launch; lookups never prune.
    pub async fn prune_mod_cache(&self) -> Result<()> {
        let keep_alive_days = self
            .inner
            .config_store
            .get_i64("cacheKeepAlive")?
            .unwrap_or(quadrant_core::config::default_app_config().cache_keep_alive);
        quadrant_core::mc_mod::cache::init_cache(keep_alive_days).await
    }

    pub fn frontend_apply_modpack(&self, name: String) -> Result<()> {
        apply_modpack(&self.get_minecraft_folder()?, &name)
    }

    fn experimental_features(&self) -> Result<bool> {
        Ok(self
            .inner
            .config_store
            .get_bool("experimentalFeatures")?
            .unwrap_or(false))
    }

    /// Resolves the Prism instances folder without the experimental gate, for
    /// keeping links Quadrant already made from going stale. A modpack renamed
    /// or deleted while the feature is off must still not leave an instance
    /// pointing at a folder that is not there.
    fn prism_instances_dir_for_maintenance(&self) -> Result<Option<PathBuf>> {
        let override_dir = self
            .inner
            .config_store
            .get_string("prismLauncherFolder")?
            .filter(|folder| !folder.trim().is_empty())
            .map(PathBuf::from);
        Ok(prism::find_data_dir(override_dir).map(|data_dir| prism::instances_dir(&data_dir)))
    }

    /// The Prism instances folder for callers that list, or `None` when the
    /// feature is off or Prism is not installed where Quadrant can see it. The
    /// single gate every read-only Prism path goes through.
    fn prism_instances_dir(&self) -> Result<Option<PathBuf>> {
        if !self.experimental_features()? {
            return Ok(None);
        }
        self.prism_instances_dir_for_maintenance()
    }

    /// The Prism instances folder for callers that act on one instance, which
    /// have to tell a disabled feature apart from a missing Prism install.
    fn require_prism_instances_dir(&self) -> Result<PathBuf> {
        if !self.experimental_features()? {
            return Err(anyhow::Error::from(ErrorCode::Forbidden));
        }
        self.prism_instances_dir_for_maintenance()?
            .ok_or_else(|| anyhow::Error::from(ErrorCode::PrismInstanceMissing))
    }

    /// Lists Prism instances, or nothing when the feature is off or Prism is
    /// not installed.
    pub fn get_prism_instances(&self) -> Result<Vec<PrismInstance>> {
        let Some(instances_dir) = self.prism_instances_dir()? else {
            return Ok(Vec::new());
        };
        prism::list_instances(&instances_dir, &self.get_minecraft_folder()?)
    }

    /// The named modpack from the on-disk listing, which is where the version
    /// and loader an instance is synced to come from.
    async fn require_modpack(&self, name: &str) -> Result<LocalModpack> {
        get_modpack(&self.get_minecraft_folder()?, name)
            .await?
            .ok_or_else(|| anyhow::Error::from(ErrorCode::ModpackMissing))
    }

    pub async fn apply_modpack_to_prism_instance(
        &self,
        name: String,
        instance_id: String,
    ) -> Result<()> {
        let instances_dir = self.require_prism_instances_dir()?;
        let modpack = self.require_modpack(&name).await?;
        modpack.ensure_manifest_readable()?;

        let instance_dir = prism::resolve_instance_dir(&instances_dir, &instance_id)?;
        let pack = prism::read_pack_manifest(&instance_dir)?;
        let plan = prism::plan_sync(&pack, &modpack.version, modpack.mod_loader)?;
        let loader_version = if plan.needs_loader_version {
            Some(prism::resolve_loader_version(modpack.mod_loader, &modpack.version).await?)
        } else {
            None
        };

        prism::apply_modpack_to_instance(
            &self.get_minecraft_folder()?,
            &instance_dir,
            pack,
            &modpack,
            loader_version.as_deref(),
        )
    }

    /// What applying the named modpack would change about each listed
    /// instance's components, so a frontend can show the move without knowing
    /// the component rules. Nothing while the feature is off, and no network.
    pub async fn get_prism_sync_plans(&self, name: String) -> Result<Vec<PrismSyncPlan>> {
        let Some(instances_dir) = self.prism_instances_dir()? else {
            return Ok(Vec::new());
        };
        let modpack = self.require_modpack(&name).await?;

        let mut plans = Vec::new();
        for instance in prism::list_instances(&instances_dir, &self.get_minecraft_folder()?)? {
            let planned = prism::resolve_instance_dir(&instances_dir, &instance.id)
                .and_then(|instance_dir| prism::read_pack_manifest(&instance_dir))
                .and_then(|pack| prism::plan_sync(&pack, &modpack.version, modpack.mod_loader));
            match planned {
                Ok(plan) => plans.push(PrismSyncPlan {
                    instance_id: instance.id,
                    minecraft_version: plan.minecraft_version,
                    mod_loader: plan.mod_loader,
                }),
                // One unreadable manifest must not cost the user the plans for
                // the instances Quadrant can read.
                Err(error) => log::warn!(
                    "Skipping Prism instance \"{}\" while planning: {error}",
                    instance.id
                ),
            }
        }
        Ok(plans)
    }

    pub fn detach_prism_instance(&self, instance_id: String) -> Result<()> {
        let instances_dir = self.require_prism_instances_dir()?;
        prism::detach_instance(&prism::resolve_instance_dir(&instances_dir, &instance_id)?)
    }

    /// The instance folders whose `mods` folder links to the named modpack,
    /// gate or no gate. Empty when Prism is not installed, and a Prism folder
    /// that cannot be read is not worth failing a modpack operation over.
    fn prism_instances_linked_to(&self, mc_folder: &Path, modpack: &str) -> Vec<PathBuf> {
        match self.prism_instances_dir_for_maintenance() {
            Ok(Some(instances_dir)) => {
                prism::instances_linked_to(&instances_dir, mc_folder, modpack)
                    .into_iter()
                    .map(|id| instances_dir.join(id))
                    .collect()
            }
            Ok(None) => Vec::new(),
            Err(error) => {
                log::warn!("Ignoring Prism instances linked to \"{modpack}\": {error}");
                Vec::new()
            }
        }
    }

    /// Gives every instance linked to the modpack its own `mods` folder and
    /// components back, before the modpack they point at stops existing.
    fn detach_prism_instances_linked_to(&self, mc_folder: &Path, modpack: &str) {
        for instance_dir in self.prism_instances_linked_to(mc_folder, modpack) {
            if let Err(error) = prism::detach_instance(&instance_dir) {
                log::warn!(
                    "Failed to unlink Prism instance \"{}\": {error}",
                    instance_dir.display()
                );
            }
        }
    }

    /// Points instances that were linked to a renamed modpack at its new
    /// folder. A link Quadrant cannot repair is worth a warning, not a failed
    /// rename the user would have to undo by hand.
    fn relink_prism_instances(&self, mc_folder: &Path, instance_dirs: &[PathBuf], modpack: &str) {
        for instance_dir in instance_dirs {
            if let Err(error) = prism::relink_instance(instance_dir, mc_folder, modpack) {
                log::warn!(
                    "Failed to relink Prism instance \"{}\" to \"{modpack}\": {error}",
                    instance_dir.display()
                );
            }
        }
    }

    /// Every place installed resource packs and shader packs can live: the
    /// Minecraft folder, plus one location per Prism instance while the
    /// experimental feature is on.
    ///
    /// With `include_files` false the pack folders are never read, which is
    /// what a caller that only needs the ids and names of the locations pays
    /// for.
    pub fn get_installed_content(&self, include_files: bool) -> Result<Vec<ContentLocation>> {
        let describe = if include_files {
            content_location
        } else {
            content_location_header
        };
        let mut locations = vec![describe(
            MINECRAFT_LOCATION_ID,
            ContentLocationKind::Minecraft,
            "",
            &self.get_minecraft_folder()?,
        )];
        // An unreadable Prism folder must not cost the user the Minecraft
        // listing, which is the one every install has.
        match self.prism_content_locations(describe) {
            Ok(prism_locations) => locations.extend(prism_locations),
            Err(error) => log::warn!("Ignoring Prism instances while listing content: {error}"),
        }
        Ok(locations)
    }

    fn prism_content_locations(&self, describe: DescribeLocation) -> Result<Vec<ContentLocation>> {
        let Some(instances_dir) = self.prism_instances_dir()? else {
            return Ok(Vec::new());
        };
        Ok(
            prism::list_instances(&instances_dir, &self.get_minecraft_folder()?)?
                .into_iter()
                .map(|instance| {
                    describe(
                        &prism_location_id(&instance.id),
                        ContentLocationKind::Prism,
                        &instance.name,
                        &prism::game_dir(&instances_dir.join(&instance.id)),
                    )
                })
                .collect(),
        )
    }

    /// The game directory a content location id points at.
    ///
    /// Takes a location id from [`Self::get_installed_content`] rather than a
    /// path, so a frontend never names a folder for itself.
    fn content_game_dir(&self, location_id: &str) -> Result<PathBuf> {
        match parse_location_id(location_id).ok_or(ErrorCode::InvalidRequest)? {
            LocationRef::Minecraft => self.get_minecraft_folder(),
            LocationRef::Prism(instance_id) => {
                let instances_dir = self.require_prism_instances_dir()?;
                Ok(prism::game_dir(&prism::resolve_instance_dir(
                    &instances_dir,
                    instance_id,
                )?))
            }
        }
    }

    /// The folder `mod_type` is installed into for one content location.
    pub fn content_folder(&self, location_id: &str, mod_type: ModType) -> Result<PathBuf> {
        let subfolder = content_subfolder(mod_type).ok_or(ErrorCode::InvalidRequest)?;
        Ok(self.content_game_dir(location_id)?.join(subfolder))
    }

    /// Copies installed packs from one content location into another, returning
    /// how many were copied.
    pub fn copy_content(
        &self,
        from_location: &str,
        to_location: &str,
        mod_type: ModType,
        file_names: Vec<String>,
    ) -> Result<usize> {
        copy_content_files(
            &self.content_folder(from_location, mod_type)?,
            &self.content_folder(to_location, mod_type)?,
            &file_names,
        )
    }

    /// Permanently removes installed packs from one content location,
    /// returning how many were removed.
    pub fn delete_content(
        &self,
        location: &str,
        mod_type: ModType,
        file_names: Vec<String>,
    ) -> Result<usize> {
        delete_content_files(&self.content_folder(location, mod_type)?, &file_names)
    }

    /// Where a single install of `mod_type` should land, with the content
    /// location a caller named resolved to a game directory first.
    fn install_roots(
        &self,
        mod_type: ModType,
        modpack: Option<&str>,
        content_location: Option<&str>,
    ) -> Result<Vec<PathBuf>> {
        let explicit_game_dir = content_location
            .map(|location_id| self.content_game_dir(location_id))
            .transpose()?;
        Ok(prism::install_roots(
            &self.get_minecraft_folder()?,
            self.prism_instances_dir()?.as_deref(),
            mod_type,
            modpack,
            explicit_game_dir,
        ))
    }

    pub async fn delete_mod(&self, modpack_name: String, mod_id: String) -> Result<()> {
        let modpack = delete_mod(
            &self.get_minecraft_folder()?,
            &self.modpack_listing(&modpack_name).await?,
            &modpack_name,
            &mod_id,
        )?;
        self.maybe_auto_sync_updated_modpack(Some(modpack)).await
    }

    pub async fn update_modpack(
        &self,
        modpack_source: String,
        name: Option<String>,
        version: Option<String>,
        mod_loader: Option<ModLoader>,
    ) -> Result<()> {
        let mc_folder = self.get_minecraft_folder()?;
        // Collected before the rename, while the links still name the old
        // folder and can still be read back.
        let linked = match name.as_deref() {
            Some(name) if name != modpack_source => {
                self.prism_instances_linked_to(&mc_folder, &modpack_source)
            }
            _ => Vec::new(),
        };
        let modpack = update_modpack(
            &mc_folder,
            &self.modpack_listing(&modpack_source).await?,
            &modpack_source,
            name,
            version,
            mod_loader,
        )?;
        self.relink_prism_instances(&mc_folder, &linked, &modpack.name);
        self.maybe_auto_sync_updated_modpack(Some(modpack)).await
    }

    pub async fn create_modpack(
        &self,
        name: String,
        version: String,
        mod_loader: ModLoader,
    ) -> Result<()> {
        create_modpack(
            &self.get_minecraft_folder()?,
            &self.modpack_listing(&name).await?,
            &name,
            &version,
            mod_loader,
        )
    }

    pub async fn delete_modpack(&self, name: String) -> Result<()> {
        let mc_folder = self.get_minecraft_folder()?;
        let modpacks = self.modpack_listing(&name).await?;
        // Unlinked first: once the folder is gone an instance links to nothing,
        // has no mods, and reports no modpack to unlink it from.
        self.detach_prism_instances_linked_to(&mc_folder, &name);
        delete_modpack(&mc_folder, &modpacks, &name)
    }

    pub async fn register_mod(&self, mod_: InstalledMod, modpack: String) -> Result<()> {
        register_mod(
            &self.get_minecraft_folder()?,
            &self.modpack_listing(&modpack).await?,
            mod_,
            &modpack,
        )
        .await
    }

    pub async fn install_modpack(&self, mod_config: InstalledModpack) -> Result<()> {
        install_modpack(
            &self.get_minecraft_folder()?,
            mod_config,
            &self.inner.config_store,
            &self.inner.event_sink,
        )
        .await
    }

    pub async fn export_modpack_to(&self, modpack: String, destination: PathBuf) -> Result<()> {
        let mc_folder = self.get_minecraft_folder()?;
        let event_sink = self.inner.event_sink.clone();
        tokio::task::spawn_blocking(move || {
            export_modpack_to(&mc_folder, &modpack, &destination, &event_sink)
        })
        .await?
    }

    pub fn set_modpack_sync_date(
        &self,
        time: u64,
        modpack: String,
        modpack_id: Option<String>,
    ) -> Result<()> {
        set_modpack_sync_date(
            &self.get_minecraft_folder()?,
            time,
            &modpack,
            modpack_id.as_deref(),
        )
    }

    pub async fn get_versions(&self) -> Result<Vec<MinecraftVersion>> {
        get_versions().await
    }

    pub async fn search_mods(&self, args: GlobalSearchModsArgs) -> Result<Vec<Mod>> {
        search_mods(args).await
    }

    pub async fn get_categories(
        &self,
        source: ModSource,
        mod_type: String,
    ) -> Result<Vec<SearchCategory>> {
        get_categories(source, mod_type).await
    }

    pub async fn check_mod_updates(
        &self,
        mod_to_update: Mod,
        minecraft_version: String,
        mod_loader: ModLoader,
        modpack_name: String,
    ) -> Result<Option<Mod>> {
        let modpack = self.require_modpack(&modpack_name).await?;
        let show_unupgradeable_mods = self
            .inner
            .config_store
            .get_bool("showUnupgradeableMods")?
            .unwrap_or(false);

        check_mod_updates(
            mod_to_update,
            minecraft_version,
            mod_loader,
            modpack,
            show_unupgradeable_mods,
        )
        .await
    }

    pub async fn install_mod(
        &self,
        id: String,
        minecraft_version: String,
        mod_loader: ModLoader,
        source: ModSource,
        modpack: Option<String>,
        mod_type: ModType,
        file_id: Option<String>,
        content_location: Option<String>,
    ) -> Result<()> {
        let install_roots =
            self.install_roots(mod_type, modpack.as_deref(), content_location.as_deref())?;
        let updated_modpack = install_mod(
            &install_roots,
            &self.inner.config_store,
            &self.inner.event_sink,
            id,
            minecraft_version,
            mod_loader,
            source,
            modpack,
            mod_type,
            file_id,
        )
        .await?;
        self.maybe_auto_sync_updated_modpack(updated_modpack).await
    }

    pub async fn install_remote_file(
        &self,
        file: UniversalModFile,
        mod_type: ModType,
        modpack: Option<String>,
        source: ModSource,
        id: String,
    ) -> Result<()> {
        let install_roots = self.install_roots(mod_type, modpack.as_deref(), None)?;
        let updated_modpack = install_remote_file(
            &install_roots,
            &self.inner.event_sink,
            file,
            mod_type,
            modpack,
            source,
            id,
        )
        .await?;
        self.maybe_auto_sync_updated_modpack(updated_modpack).await
    }

    pub async fn identify_modpack(&self, modpack: String) -> Result<Vec<IdentifiedMod>> {
        let curseforge_enabled = self
            .inner
            .config_store
            .get_bool("curseforge")?
            .unwrap_or(false);
        let modrinth_enabled = self
            .inner
            .config_store
            .get_bool("modrinth")?
            .unwrap_or(false);
        identify_modpack(
            &self.get_minecraft_folder()?,
            modpack,
            curseforge_enabled,
            modrinth_enabled,
        )
        .await
    }

    pub async fn get_mod_modrinth(&self, args: GetModArgs) -> Result<Mod> {
        get_mod_modrinth(args).await
    }

    pub async fn get_mod_owners_modrinth(&self, id: String) -> Result<Vec<String>> {
        get_mod_owners_modrinth(id).await
    }

    pub async fn get_mod_deps_modrinth(&self, id: String) -> Result<Vec<Mod>> {
        get_mod_deps_modrinth(id).await
    }

    #[cfg(feature = "curseforge")]
    pub async fn get_mod_curseforge(&self, args: GetModArgs) -> Result<Mod> {
        get_mod_curseforge(args).await
    }

    #[cfg(feature = "curseforge")]
    pub async fn get_mod_owners_curseforge(&self, id: String) -> Result<Vec<String>> {
        get_mod_owners_curseforge(id).await
    }

    #[cfg(feature = "curseforge")]
    pub async fn get_mod_deps_curseforge(&self, id: String) -> Result<Vec<Mod>> {
        get_mod_deps_curseforge(id).await
    }

    /// Signs out: asks the server to end the session, then deletes the local
    /// tokens. A server that can't be reached or refuses doesn't keep the user
    /// signed in here.
    pub async fn clear_account_token(&self) -> Result<()> {
        let revoked = tokio::time::timeout(
            REVOKE_TIMEOUT,
            revoke_session(
                &self.inner.secret_store,
                &self.inner.options.user_agent,
                &self.inner.options.oauth_client_id,
                &self.inner.options.oauth_client_secret,
            ),
        )
        .await;
        match revoked {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("Couldn't revoke the session on sign-out: {error}"),
            Err(_) => log::warn!("Revoking the session on sign-out timed out"),
        }
        clear_account_token(&self.inner.secret_store)
    }

    pub fn oauth2_client_id(&self) -> String {
        self.inner.options.oauth_client_id.clone()
    }

    pub async fn get_account_info(&self) -> Result<AccountInfo> {
        get_account_info_with_refresh(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
        )
        .await
    }

    pub async fn oauth2_login(
        &self,
        code: String,
        redirect_uri: String,
        code_verifier: Option<String>,
    ) -> Result<()> {
        oauth2_login(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            code,
            redirect_uri,
            code_verifier,
        )
        .await?;
        self.inner
            .event_sink
            .emit("recheckAccountToken", Value::Null)
    }

    pub async fn read_notification(&self, notification_id: String) -> Result<()> {
        read_notification(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            notification_id.clone(),
        )
        .await?;

        if let Some(notifications) = self
            .mark_notification_read_and_snapshot(&notification_id)
            .await
        {
            self.inner
                .event_sink
                .emit("refreshNotifications", notifications)?;
        }

        Ok(())
    }

    pub async fn get_synced_modpacks(
        &self,
        show_owners: bool,
        modpack_id: Option<String>,
    ) -> Result<Vec<SyncedModpack>> {
        get_synced_modpacks(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            show_owners,
            modpack_id,
        )
        .await
    }

    pub async fn kick_member(&self, modpack_id: String, username: String) -> Result<()> {
        kick_member(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            modpack_id,
            username,
        )
        .await
    }

    pub async fn invite_member(
        &self,
        modpack_id: String,
        username: String,
        admin: bool,
    ) -> Result<()> {
        invite_member(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            modpack_id,
            username,
            admin,
        )
        .await
    }

    pub async fn delete_synced_modpack(&self, modpack_id: String) -> Result<()> {
        delete_synced_modpack(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            modpack_id,
        )
        .await
    }

    pub async fn sync_modpack(&self, modpack: LocalModpack, overwrite: bool) -> Result<()> {
        modpack.ensure_manifest_readable()?;
        check_manifest_readable(&self.get_minecraft_folder()?, &modpack.name)?;
        let connection_id = {
            let runtime = self.inner.runtime_state.lock().await;
            runtime.notification_connection_id.clone()
        };

        let submission = quadrant_core::account::quadrant_sync::sync_modpack(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            modpack.clone(),
            overwrite,
            Some(connection_id.as_str()),
        )
        .await?;

        self.persist_sync_metadata(
            &modpack.name,
            submission.last_synced as u64,
            submission.modpack_id.as_deref(),
        )?;

        // An empty id tells the renderer to refresh the whole list, since there
        // is no single pack to refresh. The push already succeeded, so a failed
        // refresh signal must not report it as an error.
        if let Err(error) = self.inner.event_sink.emit(
            REFRESH_SYNCED_MODPACKS_EVENT,
            submission.modpack_id.unwrap_or_default(),
        ) {
            log::warn!("Failed to signal a synced modpack refresh: {error}");
        }
        Ok(())
    }

    pub async fn answer_invite(
        &self,
        modpack_id: String,
        notification_id: String,
        answer: bool,
    ) -> Result<()> {
        answer_invite(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            modpack_id,
            answer,
        )
        .await?;
        self.read_notification(notification_id).await
    }

    pub async fn share_modpack(
        &self,
        modpack_name: String,
    ) -> Result<QuadrantShareSubmissionResponse> {
        let modpack = self.require_modpack(&modpack_name).await?;
        modpack.ensure_manifest_readable()?;
        self.share_modpack_raw(modpack.into()).await
    }

    pub async fn share_modpack_raw(
        &self,
        mut mod_config: InstalledModpack,
    ) -> Result<QuadrantShareSubmissionResponse> {
        // Renderer-built configs (e.g. the Sync tab) omit the manifest metadata,
        // which serde otherwise defaults to a V1 config.
        mod_config.mod_config_version = "2".to_string();
        mod_config.quadrant_version = quadrant_core::models::quadrant_version();
        let response = share_modpack_raw(
            &self.inner.config_store,
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            mod_config,
            &self.inner.options.quadrant_api_key,
        )
        .await?;
        self.inner
            .event_sink
            .emit("quadrantShareSubmission", &response)?;
        if let Err(error) = self.send_telemetry().await {
            log::warn!("Share succeeded but follow-up telemetry failed: {error}");
        }
        Ok(response)
    }

    pub async fn get_quadrant_share_modpack(&self, code: String) -> Result<InstalledModpack> {
        get_quadrant_share_modpack(
            &self.inner.options.user_agent,
            &self.inner.options.quadrant_api_key,
            code,
        )
        .await
    }

    pub async fn get_quadrant_settings(&self) -> Result<SettingsPull> {
        quadrant_settings_sync::get_quadrant_settings(
            &self.inner.config_store,
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            &self.inner.event_sink,
        )
        .await
    }

    pub async fn submit_quadrant_settings(&self) -> Result<()> {
        quadrant_settings_sync::submit_quadrant_settings(
            &self.inner.config_store,
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
        )
        .await
    }

    pub async fn get_news(&self) -> Result<Vec<Article>> {
        quadrant_core::rss::get_news().await
    }

    pub async fn get_telemetry_info(&self) -> Result<AppInfo> {
        get_telemetry_info(
            &self.inner.config_store,
            self.inner.options.app_version.clone(),
            self.inner.options.os_name.clone(),
        )
        .await
    }

    pub async fn send_telemetry(&self) -> Result<()> {
        send_telemetry(
            &self.inner.config_store,
            &self.inner.options.user_agent,
            self.inner.options.app_version.clone(),
            self.inner.options.os_name.clone(),
            &self.inner.options.quadrant_api_key,
        )
        .await
    }

    pub async fn remove_telemetry(&self) -> Result<()> {
        remove_telemetry(
            &self.inner.config_store,
            &self.inner.options.user_agent,
            &self.inner.options.quadrant_api_key,
        )
        .await
    }

    pub fn supported_commands() -> &'static [&'static str] {
        &[
            "get_modpacks",
            "frontend_apply_modpack",
            "get_prism_instances",
            "get_prism_sync_plans",
            "apply_modpack_to_prism_instance",
            "detach_prism_instance",
            "get_installed_content",
            "copy_content",
            "delete_content",
            "delete_mod",
            "update_modpack",
            "create_modpack",
            "delete_modpack",
            "register_mod",
            "install_modpack",
            "export_modpack_to",
            "set_modpack_sync_date",
            "get_news",
            "get_minecraft_folder",
            "get_modpacks_folder",
            "init_config",
            "get_config_value",
            "set_config_value",
            "remove_config_value",
            "search_mods",
            "get_categories",
            "get_versions",
            "get_user_url",
            "install_mod",
            "install_remote_file",
            "identify_modpack",
            "check_mod_updates",
            "get_mod_modrinth",
            "get_mod_owners_modrinth",
            "get_mod_deps_modrinth",
            "clear_account_token",
            "get_account_info",
            "oauth2_login",
            "oauth2_client_id",
            "read_notification",
            "get_synced_modpacks",
            "kick_member",
            "invite_member",
            "delete_synced_modpack",
            "sync_modpack",
            "answer_invite",
            "share_modpack",
            "share_modpack_raw",
            "get_quadrant_share_modpack",
            "get_quadrant_settings",
            "submit_quadrant_settings",
            "send_telemetry",
            "remove_telemetry",
            #[cfg(feature = "curseforge")]
            "get_mod_curseforge",
            #[cfg(feature = "curseforge")]
            "get_mod_owners_curseforge",
            #[cfg(feature = "curseforge")]
            "get_mod_deps_curseforge",
        ]
    }

    #[allow(clippy::too_many_lines)]
    pub async fn invoke(&self, command: &str, payload: Value) -> Result<Value> {
        match command {
            "get_modpacks" => {
                let args: HideFreeArgs = from_value(payload)?;
                to_value(self.get_modpacks(args.hide_free).await?)
            }
            "frontend_apply_modpack" => {
                let args: NameArgs = from_value(payload)?;
                self.frontend_apply_modpack(args.name)?;
                Ok(Value::Null)
            }
            "get_prism_instances" => to_value(self.get_prism_instances()?),
            "get_prism_sync_plans" => {
                let args: NameArgs = from_value(payload)?;
                to_value(self.get_prism_sync_plans(args.name).await?)
            }
            "apply_modpack_to_prism_instance" => {
                let args: PrismApplyArgs = from_value(payload)?;
                self.apply_modpack_to_prism_instance(args.name, args.instance_id)
                    .await?;
                Ok(Value::Null)
            }
            "detach_prism_instance" => {
                let args: PrismInstanceArgs = from_value(payload)?;
                self.detach_prism_instance(args.instance_id)?;
                Ok(Value::Null)
            }
            "get_installed_content" => {
                // A caller that wants the files sends no payload at all, which
                // is a JSON null rather than an object.
                let args: GetInstalledContentArgs = from_value(payload).unwrap_or_default();
                to_value(self.get_installed_content(args.include_files.unwrap_or(true))?)
            }
            "copy_content" => {
                let args: CopyContentArgs = from_value(payload)?;
                to_value(self.copy_content(
                    &args.from_location,
                    &args.to_location,
                    args.mod_type,
                    args.file_names,
                )?)
            }
            "delete_content" => {
                let args: DeleteContentArgs = from_value(payload)?;
                to_value(self.delete_content(&args.location, args.mod_type, args.file_names)?)
            }
            "delete_mod" => {
                let args: DeleteModArgs = from_value(payload)?;
                self.delete_mod(args.modpack_name, args.mod_id).await?;
                Ok(Value::Null)
            }
            "update_modpack" => {
                let args: UpdateModpackArgs = from_value(payload)?;
                self.update_modpack(
                    args.modpack_source,
                    args.name,
                    args.version,
                    args.mod_loader,
                )
                .await?;
                Ok(Value::Null)
            }
            "create_modpack" => {
                let args: CreateModpackArgs = from_value(payload)?;
                self.create_modpack(args.name, args.version, args.mod_loader)
                    .await?;
                Ok(Value::Null)
            }
            "delete_modpack" => {
                let args: NameArgs = from_value(payload)?;
                self.delete_modpack(args.name).await?;
                Ok(Value::Null)
            }
            "register_mod" => {
                let args: RegisterModArgs = from_value(payload)?;
                self.register_mod(args.mod_, args.modpack).await?;
                Ok(Value::Null)
            }
            "install_modpack" => {
                let args: InstallModpackArgs = from_value(payload)?;
                self.install_modpack(args.mod_config).await?;
                Ok(Value::Null)
            }
            "export_modpack_to" => {
                let args: ExportModpackArgs = from_value(payload)?;
                self.export_modpack_to(args.modpack, PathBuf::from(args.destination))
                    .await?;
                Ok(Value::Null)
            }
            "set_modpack_sync_date" => {
                let args: SetModpackSyncDateArgs = from_value(payload)?;
                self.set_modpack_sync_date(args.time, args.modpack, args.modpack_id)?;
                Ok(Value::Null)
            }
            "get_news" => to_value(self.get_news().await?),
            "get_minecraft_folder" => to_value(self.get_minecraft_folder()?),
            "get_modpacks_folder" => to_value(self.get_modpacks_folder()?),
            "init_config" => {
                self.init_config()?;
                Ok(Value::Null)
            }
            "get_config_value" => {
                let args: ConfigKeyArgs = from_value(payload)?;
                Ok(self.get_config_value(&args.key)?.unwrap_or(Value::Null))
            }
            "set_config_value" => {
                let args: SetConfigValueArgs = from_value(payload)?;
                self.set_config_value(&args.key, args.value)?;
                Ok(Value::Null)
            }
            "remove_config_value" => {
                let args: ConfigKeyArgs = from_value(payload)?;
                self.remove_config_value(&args.key)?;
                Ok(Value::Null)
            }
            "search_mods" => {
                let args: SearchModsCompatArgs = from_value(payload)?;
                to_value(self.search_mods(args.args).await?)
            }
            "get_categories" => {
                let args: GetCategoriesArgs = from_value(payload)?;
                to_value(self.get_categories(args.source, args.mod_type).await?)
            }
            "get_versions" => to_value(self.get_versions().await?),
            "get_user_url" => {
                let args: GetUserUrlArgs = from_value(payload)?;
                to_value(get_user_url(args.username, args.source))
            }
            "install_mod" => {
                let args: InstallModArgs = from_value(payload)?;
                self.install_mod(
                    args.id,
                    args.minecraft_version,
                    args.mod_loader,
                    args.source,
                    args.modpack,
                    args.mod_type,
                    args.file_id,
                    args.content_location,
                )
                .await?;
                Ok(Value::Null)
            }
            "install_remote_file" => {
                let args: InstallRemoteFileArgs = from_value(payload)?;
                self.install_remote_file(
                    args.file,
                    args.mod_type,
                    args.modpack,
                    args.source,
                    args.id,
                )
                .await?;
                Ok(Value::Null)
            }
            "identify_modpack" => {
                let args: ModpackOnlyArgs = from_value(payload)?;
                to_value(self.identify_modpack(args.modpack).await?)
            }
            "check_mod_updates" => {
                let args: CheckModUpdatesArgs = from_value(payload)?;
                to_value(
                    self.check_mod_updates(
                        args.mod_to_update,
                        args.minecraft_version,
                        args.mod_loader,
                        args.modpack_name,
                    )
                    .await?,
                )
            }
            "get_mod_modrinth" => {
                let args: GetModArgsWrapper = from_value(payload)?;
                to_value(self.get_mod_modrinth(args.args).await?)
            }
            "get_mod_owners_modrinth" => {
                let args: IdArgs = from_value(payload)?;
                to_value(self.get_mod_owners_modrinth(args.id).await?)
            }
            "get_mod_deps_modrinth" => {
                let args: IdArgs = from_value(payload)?;
                to_value(self.get_mod_deps_modrinth(args.id).await?)
            }
            #[cfg(feature = "curseforge")]
            "get_mod_curseforge" => {
                let args: GetModArgsWrapper = from_value(payload)?;
                to_value(self.get_mod_curseforge(args.args).await?)
            }
            #[cfg(feature = "curseforge")]
            "get_mod_owners_curseforge" => {
                let args: IdArgs = from_value(payload)?;
                to_value(self.get_mod_owners_curseforge(args.id).await?)
            }
            #[cfg(feature = "curseforge")]
            "get_mod_deps_curseforge" => {
                let args: IdArgs = from_value(payload)?;
                to_value(self.get_mod_deps_curseforge(args.id).await?)
            }
            "clear_account_token" => {
                self.clear_account_token().await?;
                Ok(Value::Null)
            }
            "get_account_info" => to_value(self.get_account_info().await?),
            "oauth2_login" => {
                let args: OAuthLoginArgs = from_value(payload)?;
                self.oauth2_login(args.code, args.redirect_uri, args.code_verifier)
                    .await?;
                Ok(Value::Null)
            }
            "oauth2_client_id" => to_value(self.oauth2_client_id()),
            "read_notification" => {
                let args: NotificationIdArgs = from_value(payload)?;
                self.read_notification(args.notification_id).await?;
                Ok(Value::Null)
            }
            "get_synced_modpacks" => {
                let args: GetSyncedModpacksArgs = from_value(payload)?;
                to_value(
                    self.get_synced_modpacks(args.show_owners, args.modpack_id)
                        .await?,
                )
            }
            "kick_member" => {
                let args: KickMemberArgs = from_value(payload)?;
                self.kick_member(args.modpack_id, args.username).await?;
                Ok(Value::Null)
            }
            "invite_member" => {
                let args: InviteMemberArgs = from_value(payload)?;
                self.invite_member(args.modpack_id, args.username, args.admin)
                    .await?;
                Ok(Value::Null)
            }
            "delete_synced_modpack" => {
                let args: ModpackIdArgs = from_value(payload)?;
                self.delete_synced_modpack(args.modpack_id).await?;
                Ok(Value::Null)
            }
            "sync_modpack" => {
                let args: SyncModpackArgs = from_value(payload)?;
                self.sync_modpack(args.modpack, args.overwrite).await?;
                Ok(Value::Null)
            }
            "answer_invite" => {
                let args: AnswerInviteArgs = from_value(payload)?;
                self.answer_invite(args.modpack_id, args.notification_id, args.answer)
                    .await?;
                Ok(Value::Null)
            }
            "share_modpack" => {
                let args: ModpackNameArgs = from_value(payload)?;
                to_value(self.share_modpack(args.modpack_name).await?)
            }
            "share_modpack_raw" => {
                let args: InstallModpackArgs = from_value(payload)?;
                to_value(self.share_modpack_raw(args.mod_config).await?)
            }
            "get_quadrant_share_modpack" => {
                let args: CodeArgs = from_value(payload)?;
                to_value(self.get_quadrant_share_modpack(args.code).await?)
            }
            "get_quadrant_settings" => {
                self.get_quadrant_settings().await?;
                Ok(Value::Null)
            }
            "submit_quadrant_settings" => {
                self.submit_quadrant_settings().await?;
                Ok(Value::Null)
            }
            "send_telemetry" => {
                self.send_telemetry().await?;
                Ok(Value::Null)
            }
            "remove_telemetry" => {
                self.remove_telemetry().await?;
                Ok(Value::Null)
            }
            _ => Err(anyhow!("Unsupported command: {command}")),
        }
    }

    fn ensure_override_mc_folder(&self) -> Result<()> {
        if let Some(mc_folder) = &self.inner.options.mc_folder {
            self.inner
                .config_store
                .set_string("mcFolder", mc_folder.to_string_lossy().to_string())?;
        }
        Ok(())
    }

    async fn maybe_auto_sync_updated_modpack(
        &self,
        updated_modpack: Option<LocalModpack>,
    ) -> Result<()> {
        let Some(modpack) = updated_modpack else {
            return Ok(());
        };

        let auto_sync = self
            .inner
            .config_store
            .get_bool("autoQuadrantSync")?
            .unwrap_or(false);

        if modpack.last_synced == 0 || !auto_sync {
            return Ok(());
        }

        let modpack_name = modpack.name.clone();
        let modpack_id = modpack.modpack_id.clone().unwrap_or_default();
        match self.sync_modpack(modpack, false).await {
            // Installing or updating a mod must not fail just because the cloud
            // copy moved ahead; the refresh lets the user resolve it.
            Err(error) if is_cloud_sync_conflict(&error) => {
                log::warn!("Skipped auto sync of {modpack_name}: the cloud copy is newer");
                self.inner
                    .event_sink
                    .emit(REFRESH_SYNCED_MODPACKS_EVENT, modpack_id)
            }
            result => result,
        }
    }

    fn persist_sync_metadata(
        &self,
        modpack_name: &str,
        last_synced: u64,
        modpack_id: Option<&str>,
    ) -> Result<()> {
        let modpack_folder = self.get_modpacks_folder()?.join(modpack_name);
        if !modpack_folder.exists() {
            return Ok(());
        }

        set_modpack_sync_date(
            &self.get_minecraft_folder()?,
            last_synced,
            modpack_name,
            modpack_id,
        )
    }

    async fn notification_worker_loop(&self) {
        let mut reconnect_attempt = 0;
        loop {
            if let Err(error) = self.bootstrap_notifications().await {
                log::error!("Notification bootstrap failed: {error}");
            }

            let mut connected_for = None;
            if let Err(error) = self.run_notification_socket(&mut connected_for).await {
                log::warn!("Notification socket cycle ended: {error}");
            }

            if let Err(error) = self.bootstrap_notifications().await {
                log::warn!("Notification catch-up failed after socket cycle: {error}");
            }

            reconnect_attempt = next_reconnect_attempt(reconnect_attempt, connected_for);
            tokio::time::sleep(reconnect_delay(reconnect_attempt)).await;
        }
    }

    async fn settings_sync_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(SETTINGS_SYNC_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if let Err(error) = self.sync_remote_settings().await {
                log::warn!("Settings sync worker failed: {error}");
            }
        }
    }

    /// Keeps the access token fresh in the background so token-dependent actions
    /// never fire with an expired token. The first tick fires immediately, so a
    /// token that expired while the app was closed is refreshed on startup.
    async fn token_refresh_loop(&self) {
        let mut interval =
            tokio::time::interval(Duration::from_secs(TOKEN_REFRESH_CHECK_INTERVAL_SECS));
        let mut failures = 0;
        let mut ticks_to_skip = 0;
        loop {
            interval.tick().await;
            if ticks_to_skip > 0 {
                ticks_to_skip -= 1;
                continue;
            }
            match self.refresh_account_token_if_due().await {
                Ok(()) => failures = 0,
                Err(error) => {
                    failures += 1;
                    ticks_to_skip = token_refresh_ticks_to_skip(failures);
                    log::warn!("Proactive token refresh worker failed: {error}");
                }
            }
        }
    }

    /// Refreshes the access token when its recorded deadline has passed. A no-op
    /// when the user is signed out or the token is not yet due.
    async fn refresh_account_token_if_due(&self) -> Result<()> {
        if account_token_needs_refresh(&self.inner.secret_store)? {
            self.refresh_account_token().await?;
        }
        Ok(())
    }

    /// Forces an access token refresh using the stored refresh token and signals
    /// the frontend to re-read account state, also when the refresh ended the
    /// session.
    pub async fn refresh_account_token(&self) -> Result<()> {
        let refreshed = try_refresh_token(
            &self.inner.secret_store,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            &self.inner.options.user_agent,
        )
        .await;
        if refreshed.is_ok() || refreshed.as_ref().is_err_and(is_signed_out) {
            self.inner
                .event_sink
                .emit("recheckAccountToken", Value::Null)?;
        }
        refreshed
    }

    async fn bootstrap_notifications(&self) -> Result<()> {
        let cursor = self.load_notification_cursor()?;
        let (notifications, next_cursor) = get_notification_history_all_since_with_refresh(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            Some(&cursor),
            None,
            true,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
        )
        .await?;

        if notifications.is_empty() && next_cursor == cursor {
            return Ok(());
        }

        let merge_outcome = {
            let mut state = self.inner.runtime_state.lock().await;
            let runtime = &mut state.notification_state;
            let merge_outcome = merge_notifications_and_collect_updates(runtime, notifications);
            if runtime.cursor != next_cursor {
                runtime.cursor = next_cursor.clone();
            }
            merge_outcome
        };

        self.persist_notification_cursor(&next_cursor)?;

        if let Some(notifications) = merge_outcome.notifications_for_ui {
            self.inner
                .event_sink
                .emit("refreshNotifications", notifications)?;
        }

        for modpack_id in merge_outcome.membership_modpack_ids {
            self.inner
                .event_sink
                .emit(REFRESH_SYNCED_MODPACKS_EVENT, modpack_id)?;
        }

        for notification in merge_outcome.modpack_sync_notifications {
            if let Err(error) = self.handle_modpack_sync_notification(&notification).await {
                log::warn!("Failed to process bootstrapped modpack sync notification: {error}");
            } else {
                self.mark_modpack_sync_processed(&notification).await;
            }
        }

        Ok(())
    }

    /// Runs one socket connection until it fails, recording in `connected_for`
    /// how long it stayed open once the handshake succeeded.
    async fn run_notification_socket(&self, connected_for: &mut Option<Duration>) -> Result<()> {
        let token = quadrant_core::account::get_account_token(&self.inner.secret_store)?;
        let (cursor, connection_id) = {
            let state = self.inner.runtime_state.lock().await;
            (
                state.notification_state.cursor.clone(),
                state.notification_connection_id.clone(),
            )
        };

        let ws_url = self.build_notification_ws_url(&cursor, &connection_id)?;
        let mut request = ws_url.as_str().into_client_request()?;
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse()?);
        request
            .headers_mut()
            .insert("User-Agent", self.inner.options.user_agent.parse()?);

        let (mut socket, _) = tokio::time::timeout(WS_CONNECT_TIMEOUT, connect_async(request))
            .await
            .map_err(|_| anyhow!("notification websocket connect timed out"))??;
        let connected_at = tokio::time::Instant::now();
        let result = self.read_notification_frames(&mut socket).await;
        *connected_for = Some(connected_at.elapsed());
        result
    }

    async fn read_notification_frames(
        &self,
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> Result<()> {
        loop {
            let frame = match tokio::time::timeout(WS_READ_TIMEOUT, socket.next()).await {
                Ok(Some(frame)) => frame?,
                Ok(None) => return Err(anyhow!("notification websocket stream ended")),
                Err(_) => return Err(anyhow!("notification websocket went silent")),
            };
            match frame {
                Message::Text(payload) => self.handle_notification_ws_frame(&payload).await?,
                Message::Close(frame) => {
                    let reason = frame
                        .map(|frame| frame.reason.to_string())
                        .unwrap_or_else(|| "closed".to_string());
                    return Err(anyhow!("notification websocket closed: {reason}"));
                }
                Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
            }
        }
    }

    async fn handle_notification_ws_frame(&self, payload: &str) -> Result<()> {
        match serde_json::from_str::<NotificationWsFrame>(payload)? {
            NotificationWsFrame::Connected { server_time } => {
                log::info!("Notification websocket connected at {server_time}");
            }
            NotificationWsFrame::Notification {
                delivery: _,
                notification,
            } => {
                let notifications_to_emit = {
                    let mut state = self.inner.runtime_state.lock().await;
                    let outcome = state.notification_state.upsert(notification.clone());
                    outcome
                        .changed
                        .then(|| state.notification_state.notifications_for_ui())
                };

                if let Some(notifications) = notifications_to_emit {
                    self.inner
                        .event_sink
                        .emit("refreshNotifications", notifications)?;
                }

                if is_modpack_sync_notification(&notification) {
                    if let Err(error) = self.handle_modpack_sync_notification(&notification).await {
                        log::warn!("Failed to process modpack sync notification: {error}");
                    } else {
                        self.mark_modpack_sync_processed(&notification).await;
                    }
                }

                if is_modpack_membership_notification(&notification)
                    && let Some(modpack_id) = notification.resource_id.as_deref()
                {
                    self.inner
                        .event_sink
                        .emit(REFRESH_SYNCED_MODPACKS_EVENT, modpack_id)?;
                }
            }
            NotificationWsFrame::ReplayComplete { truncated, .. } => {
                if truncated {
                    self.bootstrap_notifications().await?;
                }
            }
            NotificationWsFrame::Error { error } => {
                if error.to_lowercase().contains("lagged") {
                    self.bootstrap_notifications().await?;
                }
                return Err(anyhow!("notification websocket error frame: {error}"));
            }
            NotificationWsFrame::Unknown => {
                // Newer backends may add event types; ignoring them keeps the
                // socket alive instead of forcing a reconnect/re-bootstrap loop.
                log::debug!("Ignoring unknown notification websocket frame: {payload}");
            }
        }

        Ok(())
    }

    fn build_notification_ws_url(
        &self,
        cursor: &NotificationCursor,
        connection_id: &str,
    ) -> Result<Url> {
        let base = self
            .inner
            .options
            .api_base_url
            .clone()
            .unwrap_or_else(quadrant_core::account::backend_base_url);
        let mut url = Url::parse(&base)?;
        match url.scheme() {
            "https" => url
                .set_scheme("wss")
                .map_err(|_| anyhow!("invalid ws scheme"))?,
            "http" => url
                .set_scheme("ws")
                .map_err(|_| anyhow!("invalid ws scheme"))?,
            "wss" | "ws" => {}
            _ => return Err(anyhow!("unsupported backend scheme")),
        }
        let base_path = url.path().trim_end_matches('/');
        url.set_path(&format!("{base_path}/account/notifications/ws"));
        {
            let mut query = url.query_pairs_mut();
            if let Some(created_at) = cursor.created_at.as_deref() {
                query.append_pair("since", created_at);
            }
            query.append_pair("replay_limit", &WS_REPLAY_LIMIT.to_string());
            query.append_pair("modpack_sync", "true");
            query.append_pair("connection_id", connection_id);
        }
        Ok(url)
    }

    async fn handle_modpack_sync_notification(&self, notification: &Notification) -> Result<()> {
        let synced_modpack = self
            .resolve_synced_modpack_from_notification(notification)
            .await?;
        let auto_quadrant_sync = self
            .inner
            .config_store
            .get_bool("autoQuadrantSync")?
            .unwrap_or(false);

        if auto_quadrant_sync
            && let Some(local_modpack) =
                self.resolve_local_modpack_for_sync(&synced_modpack).await?
        {
            self.maybe_apply_remote_modpack_update(&local_modpack, &synced_modpack)
                .await?;
        }

        self.inner
            .event_sink
            .emit(REFRESH_SYNCED_MODPACKS_EVENT, synced_modpack.modpack_id)?;
        Ok(())
    }

    async fn resolve_synced_modpack_from_notification(
        &self,
        notification: &Notification,
    ) -> Result<SyncedModpack> {
        if let Some(payload) = parse_notification_message(notification) {
            return Ok(payload.modpack);
        }

        let resource_id = notification
            .resource_id
            .as_deref()
            .ok_or_else(|| anyhow!("modpack sync notification missing resource_id"))?;
        let mut synced_modpacks = self
            .get_synced_modpacks(false, Some(resource_id.to_string()))
            .await?;
        synced_modpacks
            .drain(..)
            .next()
            .ok_or_else(|| anyhow!("modpack sync fallback fetch returned no modpack"))
    }

    /// Only a local pack recording the synced pack's id takes its updates
    /// unasked. A pack that merely shares its name may be a different one.
    async fn resolve_local_modpack_for_sync(
        &self,
        synced_modpack: &SyncedModpack,
    ) -> Result<Option<LocalModpack>> {
        Ok(get_modpacks(&self.get_minecraft_folder()?, true)
            .await?
            .into_iter()
            .find(|modpack| {
                modpack.modpack_id.as_deref() == Some(synced_modpack.modpack_id.as_str())
            }))
    }

    async fn maybe_apply_remote_modpack_update(
        &self,
        local_modpack: &LocalModpack,
        synced_modpack: &SyncedModpack,
    ) -> Result<()> {
        let local_sync_time = local_modpack.last_synced / 1000;
        if synced_modpack.last_synced <= local_sync_time {
            return Ok(());
        }

        local_modpack.ensure_manifest_readable()?;
        let remote_mods: Vec<InstalledMod> = serde_json::from_str(&synced_modpack.mods)?;
        // Whoever can push to a pack decides which files its members run, so
        // a link to an arbitrary host waits for the user to apply it.
        let review = review_incoming_mods(&local_modpack.mods, &remote_mods);
        let warning = |applied| {
            let names = |mods: &[InstalledMod]| {
                mods.iter()
                    .map(|mod_| mod_.display_name().to_string())
                    .collect()
            };
            BackendEvent::ModpackSyncWarning {
                modpack_id: synced_modpack.modpack_id.clone(),
                name: local_modpack.name.clone(),
                applied,
                online_mods: names(&review.online),
                unverified_mods: names(&review.unverified),
            }
        };
        if !review.online.is_empty() {
            log::warn!(
                "Held back a synced update of \"{}\": it adds or changes Online mods",
                local_modpack.name
            );
            return self.inner.event_sink.publish(warning(false));
        }
        if !self.begin_modpack_update(&local_modpack.name).await {
            return Ok(());
        }

        let result = async {
            self.install_modpack(InstalledModpack {
                mod_config_version: String::new(),
                quadrant_version: String::new(),
                mod_loader: synced_modpack.mod_loader,
                name: local_modpack.name.clone(),
                version: synced_modpack.minecraft_version.clone(),
                mods: remote_mods,
            })
            .await?;
            self.persist_sync_metadata(
                &local_modpack.name,
                synced_modpack.last_synced as u64,
                Some(synced_modpack.modpack_id.as_str()),
            )
        }
        .await;
        self.finish_modpack_update(&local_modpack.name).await;
        result?;
        if !review.unverified.is_empty() {
            self.inner.event_sink.publish(warning(true))?;
        }
        Ok(())
    }

    async fn begin_modpack_update(&self, modpack_name: &str) -> bool {
        let mut state = self.inner.runtime_state.lock().await;
        if state
            .updated_modpacks
            .iter()
            .any(|name| name == modpack_name)
        {
            return false;
        }
        state.updated_modpacks.push(modpack_name.to_string());
        true
    }

    async fn finish_modpack_update(&self, modpack_name: &str) {
        let mut state = self.inner.runtime_state.lock().await;
        state.updated_modpacks.retain(|name| name != modpack_name);
    }

    async fn sync_remote_settings(&self) -> Result<()> {
        if !self
            .inner
            .config_store
            .get_bool("syncSettings")?
            .unwrap_or(false)
        {
            return Ok(());
        }

        match self.get_quadrant_settings().await? {
            SettingsPull::LocalNewer | SettingsPull::NoRemote => {
                self.submit_quadrant_settings().await
            }
            SettingsPull::UpToDate | SettingsPull::Applied => Ok(()),
        }
    }

    async fn mark_notification_read_and_snapshot(
        &self,
        notification_id: &str,
    ) -> Option<Vec<Notification>> {
        let mut state = self.inner.runtime_state.lock().await;
        state
            .notification_state
            .mark_read(notification_id)
            .then(|| state.notification_state.notifications_for_ui())
    }

    async fn mark_modpack_sync_processed(&self, notification: &Notification) {
        let mut state = self.inner.runtime_state.lock().await;
        state
            .notification_state
            .mark_modpack_sync_processed(notification);
    }

    fn load_notification_cursor(&self) -> Result<NotificationCursor> {
        Ok(NotificationCursor {
            created_at: self
                .inner
                .config_store
                .get_string(NOTIFICATION_CURSOR_CREATED_AT_KEY)?,
            notification_id: self
                .inner
                .config_store
                .get_string(NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY)?,
        })
    }

    fn persist_notification_cursor(&self, cursor: &NotificationCursor) -> Result<()> {
        match &cursor.created_at {
            Some(created_at) => self
                .inner
                .config_store
                .set_string(NOTIFICATION_CURSOR_CREATED_AT_KEY, created_at.clone())?,
            None => self
                .inner
                .config_store
                .delete_key(NOTIFICATION_CURSOR_CREATED_AT_KEY)?,
        }
        match &cursor.notification_id {
            Some(notification_id) => self.inner.config_store.set_string(
                NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY,
                notification_id.clone(),
            )?,
            None => self
                .inner
                .config_store
                .delete_key(NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY)?,
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
struct NotificationRuntimeState {
    by_key: HashMap<String, Notification>,
    key_by_notification_id: HashMap<String, String>,
    processed_modpack_sync_by_key: HashMap<String, String>,
    cursor: NotificationCursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UpsertOutcome {
    changed: bool,
}

struct MergeNotificationsOutcome {
    notifications_for_ui: Option<Vec<Notification>>,
    modpack_sync_notifications: Vec<Notification>,
    membership_modpack_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ModpackSyncPayload {
    notification_type: String,
    modpack: SyncedModpack,
}

impl NotificationRuntimeState {
    fn upsert(&mut self, notification: Notification) -> UpsertOutcome {
        self.advance_cursor(&notification);
        let identity_key = notification_identity_key(&notification);
        match self.by_key.get(&identity_key) {
            Some(existing) if existing == &notification => UpsertOutcome { changed: false },
            Some(_) => {
                if let Some(previous) = self
                    .by_key
                    .insert(identity_key.clone(), notification.clone())
                {
                    self.key_by_notification_id
                        .remove(previous.notification_id.as_str());
                }
                self.key_by_notification_id
                    .insert(notification.notification_id.clone(), identity_key);
                UpsertOutcome { changed: true }
            }
            None => {
                self.by_key
                    .insert(identity_key.clone(), notification.clone());
                self.key_by_notification_id
                    .insert(notification.notification_id.clone(), identity_key);
                if self.by_key.len() > NOTIFICATION_HISTORY_LIMIT {
                    self.forget_oldest();
                }
                UpsertOutcome { changed: true }
            }
        }
    }

    /// Keeps a long session from growing the list without bound: the server
    /// replays no more than this many on a reconnect either.
    fn forget_oldest(&mut self) {
        let Some(oldest_key) = self
            .by_key
            .iter()
            .min_by(|(_, left), (_, right)| {
                left.created_at_unix
                    .cmp(&right.created_at_unix)
                    .then_with(|| left.notification_id.cmp(&right.notification_id))
            })
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        if let Some(oldest) = self.by_key.remove(&oldest_key) {
            self.key_by_notification_id
                .remove(oldest.notification_id.as_str());
        }
    }

    fn mark_read(&mut self, notification_id: &str) -> bool {
        let Some(identity_key) = self.key_by_notification_id.get(notification_id).cloned() else {
            return false;
        };
        let Some(notification) = self.by_key.get_mut(identity_key.as_str()) else {
            return false;
        };
        if notification.read {
            return false;
        }
        notification.read = true;
        true
    }

    fn notifications_for_ui(&self) -> Vec<Notification> {
        let mut notifications = self.by_key.values().cloned().collect::<Vec<_>>();
        notifications.sort_by(|a, b| {
            b.created_at_unix
                .cmp(&a.created_at_unix)
                .then_with(|| b.notification_id.cmp(&a.notification_id))
        });
        notifications
    }

    fn advance_cursor(&mut self, notification: &Notification) {
        let current_created_at = self.cursor.created_at.as_deref();
        let current_notification_id = self.cursor.notification_id.as_deref();
        let is_newer = match current_created_at {
            None => true,
            Some(created_at) => {
                notification.created_at.as_str() > created_at
                    || (notification.created_at.as_str() == created_at
                        && current_notification_id
                            .map(|id| notification.notification_id.as_str() > id)
                            .unwrap_or(true))
            }
        };

        if is_newer {
            self.cursor = NotificationCursor {
                created_at: Some(notification.created_at.clone()),
                notification_id: Some(notification.notification_id.clone()),
            };
        }
    }

    fn should_process_modpack_sync_notification(&self, notification: &Notification) -> bool {
        if !is_modpack_sync_notification(notification) {
            return false;
        }

        let identity_key = notification_identity_key(notification);
        self.processed_modpack_sync_by_key
            .get(identity_key.as_str())
            .map(|notification_id| notification_id != &notification.notification_id)
            .unwrap_or(true)
    }

    fn mark_modpack_sync_processed(&mut self, notification: &Notification) {
        if !is_modpack_sync_notification(notification) {
            return;
        }

        let identity_key = notification_identity_key(notification);
        self.processed_modpack_sync_by_key
            .insert(identity_key, notification.notification_id.clone());
    }
}

fn merge_notifications_and_collect_updates(
    runtime: &mut NotificationRuntimeState,
    mut notifications: Vec<Notification>,
) -> MergeNotificationsOutcome {
    notifications.sort_by(|left, right| {
        left.created_at_unix
            .cmp(&right.created_at_unix)
            .then_with(|| left.notification_id.cmp(&right.notification_id))
    });

    let mut changed = false;
    let mut latest_modpack_sync_by_key = HashMap::new();
    let mut modpack_sync_order = Vec::new();
    let mut membership_modpack_ids: Vec<String> = Vec::new();

    for notification in notifications {
        let should_queue_modpack_sync =
            runtime.should_process_modpack_sync_notification(&notification);
        let outcome = runtime.upsert(notification.clone());
        changed |= outcome.changed;

        // Gating on `changed` keeps a re-bootstrap of the same history from
        // re-emitting refreshes the renderer has already acted on.
        if outcome.changed
            && is_modpack_membership_notification(&notification)
            && let Some(resource_id) = notification.resource_id.as_deref()
            && !membership_modpack_ids.iter().any(|id| id == resource_id)
        {
            membership_modpack_ids.push(resource_id.to_string());
        }

        if should_queue_modpack_sync {
            let identity_key = notification_identity_key(&notification);
            latest_modpack_sync_by_key.insert(identity_key.clone(), notification);
            if let Some(index) = modpack_sync_order
                .iter()
                .position(|existing| existing == &identity_key)
            {
                modpack_sync_order.remove(index);
            }
            modpack_sync_order.push(identity_key);
        }
    }

    MergeNotificationsOutcome {
        notifications_for_ui: changed.then(|| runtime.notifications_for_ui()),
        modpack_sync_notifications: modpack_sync_order
            .into_iter()
            .filter_map(|identity_key| latest_modpack_sync_by_key.remove(&identity_key))
            .collect(),
        membership_modpack_ids,
    }
}

/// After `failures` refreshes in a row failed, how many checks to let pass
/// before the next try: 1, 3, 7, ... up to 31, so an unreachable server is
/// asked at most every 32 minutes instead of every minute.
fn token_refresh_ticks_to_skip(failures: u32) -> u32 {
    (1 << failures.min(5)) - 1
}

/// A connection that stayed open this long was healthy, so the next drop is
/// retried quickly again. One the server accepts and then drops at once keeps
/// backing off instead of being retried every second.
const WS_STABLE_CONNECTION: Duration = Duration::from_secs(30);

fn next_reconnect_attempt(previous: u32, connected_for: Option<Duration>) -> u32 {
    if connected_for.is_some_and(|uptime| uptime >= WS_STABLE_CONNECTION) {
        1
    } else {
        previous.saturating_add(1)
    }
}

fn reconnect_delay(attempt: u32) -> Duration {
    let secs = match attempt {
        0 | 1 => 1,
        2 => 2,
        3 => 5,
        4 => 10,
        5 => 30,
        _ => 60,
    };
    Duration::from_secs(secs)
}

fn parse_notification_message(notification: &Notification) -> Option<ModpackSyncPayload> {
    serde_json::from_str::<ModpackSyncPayload>(&notification.message)
        .ok()
        .filter(|payload| payload.notification_type == MODPACK_SYNC_NOTIFICATION_TYPE)
}

fn is_modpack_sync_notification(notification: &Notification) -> bool {
    notification.notification_type.as_deref() == Some(MODPACK_SYNC_NOTIFICATION_TYPE)
}

fn is_modpack_membership_notification(notification: &Notification) -> bool {
    notification.notification_type.as_deref() == Some(MODPACK_MEMBERSHIP_NOTIFICATION_TYPE)
}

fn notification_identity_key(notification: &Notification) -> String {
    if is_modpack_sync_notification(notification)
        && let Some(resource_id) = notification.resource_id.as_deref()
    {
        return format!("{MODPACK_SYNC_NOTIFICATION_TYPE}:{resource_id}");
    }
    notification.notification_id.clone()
}

fn from_value<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(Error::from)
}

fn to_value<T: Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(Error::from)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HideFreeArgs {
    hide_free: bool,
}

#[derive(Deserialize)]
struct ConfigKeyArgs {
    key: String,
}

#[derive(Deserialize)]
struct SetConfigValueArgs {
    key: String,
    value: Value,
}

#[derive(Deserialize)]
struct NameArgs {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismApplyArgs {
    name: String,
    instance_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismInstanceArgs {
    instance_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteModArgs {
    modpack_name: String,
    mod_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateModpackArgs {
    modpack_source: String,
    name: Option<String>,
    version: Option<String>,
    mod_loader: Option<ModLoader>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateModpackArgs {
    name: String,
    version: String,
    mod_loader: ModLoader,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterModArgs {
    #[serde(alias = "mod")]
    mod_: InstalledMod,
    modpack: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallModpackArgs {
    mod_config: InstalledModpack,
}

#[derive(Deserialize)]
struct ExportModpackArgs {
    modpack: String,
    destination: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetModpackSyncDateArgs {
    #[serde(deserialize_with = "deserialize_integral_u64")]
    time: u64,
    modpack: String,
    modpack_id: Option<String>,
}

#[derive(Deserialize)]
struct SearchModsCompatArgs {
    args: GlobalSearchModsArgs,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetCategoriesArgs {
    source: ModSource,
    mod_type: String,
}

#[derive(Deserialize)]
struct GetUserUrlArgs {
    username: String,
    source: ModSource,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallModArgs {
    id: String,
    minecraft_version: String,
    mod_loader: ModLoader,
    source: ModSource,
    modpack: Option<String>,
    mod_type: ModType,
    file_id: Option<String>,
    #[serde(default)]
    content_location: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallRemoteFileArgs {
    file: UniversalModFile,
    mod_type: ModType,
    modpack: Option<String>,
    source: ModSource,
    id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GetInstalledContentArgs {
    /// Absent means true: a caller that does not ask gets the pack listings.
    include_files: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyContentArgs {
    from_location: String,
    to_location: String,
    mod_type: ModType,
    file_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteContentArgs {
    location: String,
    mod_type: ModType,
    file_names: Vec<String>,
}

#[derive(Deserialize)]
struct ModpackOnlyArgs {
    modpack: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckModUpdatesArgs {
    mod_to_update: Mod,
    minecraft_version: String,
    mod_loader: ModLoader,
    modpack_name: String,
}

#[derive(Deserialize)]
struct GetModArgsWrapper {
    args: GetModArgs,
}

#[derive(Deserialize)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthLoginArgs {
    code: String,
    redirect_uri: String,
    #[serde(default)]
    code_verifier: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotificationIdArgs {
    notification_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetSyncedModpacksArgs {
    show_owners: bool,
    modpack_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KickMemberArgs {
    modpack_id: String,
    username: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InviteMemberArgs {
    modpack_id: String,
    username: String,
    admin: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModpackIdArgs {
    modpack_id: String,
}

#[derive(Deserialize)]
struct SyncModpackArgs {
    modpack: LocalModpack,
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswerInviteArgs {
    modpack_id: String,
    notification_id: String,
    answer: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModpackNameArgs {
    modpack_name: String,
}

fn deserialize_integral_u64<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                return Ok(value);
            }
            if let Some(value) = number.as_i64() {
                return u64::try_from(value)
                    .map_err(|_| D::Error::custom("integer is out of range for u64"));
            }
            if let Some(value) = number.as_f64()
                && value.is_finite()
                && value.fract() == 0.0
                && value >= 0.0
                && value <= u64::MAX as f64
            {
                return Ok(value as u64);
            }
            Err(D::Error::custom("expected an integer-valued number"))
        }
        other => Err(D::Error::custom(format!(
            "expected a number for integer deserialization, got {other}"
        ))),
    }
}

#[derive(Deserialize)]
struct CodeArgs {
    code: String,
}

#[cfg(test)]
mod tests {
    use super::{
        HostEventBridge, HostEventEnvelope, JsonFileStore, Notification, NotificationCursor,
        NotificationRuntimeState, QuadrantHost, QuadrantHostOptions, Uuid,
        is_modpack_membership_notification, merge_notifications_and_collect_updates,
    };
    use quadrant_core::{
        account::KEYRING_SERVICE,
        account::quadrant_sync::SyncedModpack,
        content::{ContentLocation, ContentLocationKind, PACK_TYPES},
        events::{BackendEvent, ModProgressPayload},
        mc_mod::ModType,
        models::{
            InstalledMod, InstalledModpack, LocalModpack, ModLoader, ModSource, SyncInfo,
            modpack_path,
        },
        ports::{EventSink, SettingsStore},
        prism::PrismSyncPlan,
    };
    use serde_json::{Value, json};
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    use tempfile::tempdir;

    #[test]
    fn keyring_initialization_is_retried_until_it_succeeds() {
        let ready = std::sync::Mutex::new(false);
        let mut calls = 0;
        let mut init = |succeed: bool| {
            calls += 1;
            if succeed {
                Ok(())
            } else {
                Err("no secret service")
            }
        };

        assert!(super::ensure_initialized(&ready, || init(false)).is_err());
        assert!(super::ensure_initialized(&ready, || init(true)).is_ok());
        assert!(super::ensure_initialized(&ready, || init(false)).is_ok());
        assert_eq!(calls, 2);
    }

    #[test]
    fn failed_token_refreshes_back_off() {
        let skips: Vec<_> = (1..=7).map(super::token_refresh_ticks_to_skip).collect();
        assert_eq!(skips, [1, 3, 7, 15, 31, 31, 31]);
    }

    #[test]
    fn reconnects_back_off_until_a_connection_stays_up() {
        use super::{next_reconnect_attempt, reconnect_delay};
        use std::time::Duration;

        let mut attempt = 0;
        for _ in 0..6 {
            attempt = next_reconnect_attempt(attempt, Some(Duration::from_secs(1)));
        }
        assert_eq!(reconnect_delay(attempt), Duration::from_secs(60));
        assert_eq!(next_reconnect_attempt(attempt, None), 7);

        attempt = next_reconnect_attempt(attempt, Some(Duration::from_secs(45)));
        assert_eq!(reconnect_delay(attempt), Duration::from_secs(1));
    }

    fn notification(id: &str, unix: i64, read: bool) -> Notification {
        Notification {
            notification_id: id.to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("invite_to_sync".to_string()),
            resource_id: None,
            message: "{\"simple_message\":\"hello\"}".to_string(),
            created_at: format!("2026-03-20T10:{unix:02}:00Z"),
            created_at_unix: unix,
            read,
        }
    }

    fn membership_notification(id: &str, unix: i64, resource_id: &str) -> Notification {
        Notification {
            notification_type: Some("modpack_membership".to_string()),
            resource_id: Some(resource_id.to_string()),
            message: "{\"notification_type\":\"modpack_membership\"}".to_string(),
            ..notification(id, unix, false)
        }
    }

    /// A host whose Minecraft folder is a temp dir holding one modpack. Its
    /// keyring service name is unique per run, so a test that unintentionally
    /// reaches a signed-in code path fails on the missing token instead of
    /// talking to the developer's real account.
    fn sync_host(temp_dir: &Path) -> (QuadrantHost, PathBuf) {
        let mc_folder = temp_dir.join(".minecraft");
        fs::create_dir_all(mc_folder.join("modpacks").join("pack")).unwrap();

        let mut options =
            QuadrantHostOptions::new(temp_dir.join("data"), "client", "secret", "api-key");
        options.keyring_service_name = format!("quadrant-test-{}", Uuid::now_v7());

        let host = QuadrantHost::new(options).unwrap();
        host.set_config_value("mcFolder", json!(mc_folder)).unwrap();
        (host, mc_folder)
    }

    fn local_modpack(last_synced: i64) -> LocalModpack {
        LocalModpack {
            name: "pack".to_string(),
            version: "1.21.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: Vec::new(),
            unknown_mods: false,
            is_applied: false,
            last_synced,
            modpack_id: Some("modpack-7".to_string()),
            manifest_error: None,
        }
    }

    #[test]
    fn json_store_persists_updates() {
        let temp_dir = tempdir().unwrap();
        let store = JsonFileStore::new(temp_dir.path().join("config.json")).unwrap();
        store.set_bool("modrinth", true).unwrap();
        let reloaded = JsonFileStore::new(temp_dir.path().join("config.json")).unwrap();
        assert_eq!(reloaded.get_bool("modrinth").unwrap(), Some(true));
    }

    #[test]
    fn json_store_sets_unreadable_documents_aside_and_starts_empty() {
        for document in ["[]", "null", "true", "42", "\"settings\"", "{\"mcFolder\":"] {
            let temp_dir = tempdir().unwrap();
            let path = temp_dir.path().join("config.json");
            fs::write(&path, document).unwrap();

            let store = JsonFileStore::new(path.clone()).unwrap();

            assert!(store.entries().unwrap().is_empty(), "kept {document}");
            assert!(!path.exists());
            let backups: Vec<_> = fs::read_dir(temp_dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("config.json.corrupt-")
                })
                .collect();
            assert_eq!(backups.len(), 1);
            assert_eq!(fs::read_to_string(&backups[0]).unwrap(), document);

            store.set_bool("modrinth", true).unwrap();
            let reloaded = JsonFileStore::new(path).unwrap();
            assert_eq!(reloaded.get_bool("modrinth").unwrap(), Some(true));
        }
    }

    #[test]
    fn json_stores_sharing_a_file_keep_each_others_writes() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("config.json");
        // Separate stores have separate in-process locks, like two processes.
        let stores = [
            JsonFileStore::new(path.clone()).unwrap(),
            JsonFileStore::new(path.clone()).unwrap(),
        ];
        std::thread::scope(|scope| {
            for (index, store) in stores.iter().enumerate() {
                scope.spawn(move || {
                    for key in 0..40 {
                        store.set_i64(&format!("key-{index}-{key}"), key).unwrap();
                    }
                });
            }
        });
        assert_eq!(
            JsonFileStore::new(path).unwrap().entries().unwrap().len(),
            80
        );
    }

    #[test]
    fn json_store_reads_the_file_again_only_when_it_changed() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("config.json");
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let write_aged = |contents: &str, modified: std::time::SystemTime| {
            fs::write(&path, contents).unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        };
        write_aged(r#"{"mcFolder":"/a"}"#, an_hour_ago);
        let store = JsonFileStore::new(path.clone()).unwrap();
        assert_eq!(store.get_string("mcFolder").unwrap().as_deref(), Some("/a"));

        // Same size and time: the cached copy answers without a parse.
        write_aged(r#"{"mcFolder":"/b"}"#, an_hour_ago);
        assert_eq!(store.get_string("mcFolder").unwrap().as_deref(), Some("/a"));

        // Another process's write moves the stamp.
        write_aged(
            r#"{"mcFolder":"/b"}"#,
            an_hour_ago + std::time::Duration::from_secs(60),
        );
        assert_eq!(store.get_string("mcFolder").unwrap().as_deref(), Some("/b"));

        let other = JsonFileStore::new(path.clone()).unwrap();
        other.set_string("mcFolder", "/c".to_string()).unwrap();
        assert_eq!(store.get_string("mcFolder").unwrap().as_deref(), Some("/c"));
        fs::remove_file(&path).unwrap();
        assert_eq!(store.get_string("mcFolder").unwrap(), None);
    }

    #[test]
    fn json_store_preserves_invalid_disk_contents_on_mutation() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("config.json");
        let store = JsonFileStore::new(path.clone()).unwrap();
        store.set_bool("modrinth", true).unwrap();

        for document in ["{\"mcFolder\":", "[]"] {
            fs::write(&path, document).unwrap();
            assert!(store.set_bool("curseforge", false).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), document);
            assert!(store.delete_key("modrinth").is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), document);
        }

        fs::write(&path, r#"{"mcFolder":"/repaired/minecraft"}"#).unwrap();
        store.set_bool("curseforge", false).unwrap();
        let repaired = JsonFileStore::new(path).unwrap();
        assert_eq!(
            repaired.get_string("mcFolder").unwrap().as_deref(),
            Some("/repaired/minecraft")
        );
        assert_eq!(repaired.get_bool("curseforge").unwrap(), Some(false));
        assert_eq!(repaired.get_bool("modrinth").unwrap(), None);
    }

    #[test]
    fn notification_merge_deduplicates_and_orders() {
        let mut state = NotificationRuntimeState::default();
        assert!(
            merge_notifications_and_collect_updates(
                &mut state,
                vec![notification("n1", 1, false), notification("n2", 2, false)]
            )
            .notifications_for_ui
            .is_some()
        );
        assert!(
            merge_notifications_and_collect_updates(&mut state, vec![notification("n1", 1, false)])
                .notifications_for_ui
                .is_none()
        );

        let notifications = state.notifications_for_ui();
        assert_eq!(notifications[0].notification_id, "n2");
        assert_eq!(
            state.cursor,
            NotificationCursor {
                created_at: Some("2026-03-20T10:02:00Z".to_string()),
                notification_id: Some("n2".to_string()),
            }
        );
    }

    #[test]
    fn notification_state_keeps_only_the_newest_history() {
        let mut state = NotificationRuntimeState::default();
        let total = super::NOTIFICATION_HISTORY_LIMIT + 5;
        merge_notifications_and_collect_updates(
            &mut state,
            (0..total)
                .rev()
                .map(|index| notification(&format!("n{index:04}"), index as i64, false))
                .collect(),
        );

        let notifications = state.notifications_for_ui();
        assert_eq!(notifications.len(), super::NOTIFICATION_HISTORY_LIMIT);
        assert_eq!(
            notifications[0].notification_id,
            format!("n{:04}", total - 1)
        );
        assert_eq!(notifications.last().unwrap().notification_id, "n0005");
        assert!(!state.mark_read("n0004"));
        assert!(state.mark_read("n0005"));
    }

    #[test]
    fn membership_notifications_are_told_apart_by_type() {
        assert!(is_modpack_membership_notification(
            &membership_notification("m1", 1, "pack-1")
        ));
        assert!(!is_modpack_membership_notification(&notification(
            "n1", 1, false
        )));
    }

    #[test]
    fn notification_merge_collects_each_membership_modpack_once() {
        let mut state = NotificationRuntimeState::default();
        let outcome = merge_notifications_and_collect_updates(
            &mut state,
            vec![
                membership_notification("m1", 1, "pack-1"),
                membership_notification("m2", 2, "pack-1"),
                membership_notification("m3", 3, "pack-2"),
            ],
        );
        assert_eq!(outcome.membership_modpack_ids, ["pack-1", "pack-2"]);

        let replayed = merge_notifications_and_collect_updates(
            &mut state,
            vec![membership_notification("m1", 1, "pack-1")],
        );
        assert!(replayed.membership_modpack_ids.is_empty());
    }

    #[test]
    fn persist_sync_metadata_records_what_the_server_returned() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder) = sync_host(temp_dir.path());

        host.persist_sync_metadata("pack", 1_762_000_000, Some("modpack-7"))
            .unwrap();

        let sync_info: SyncInfo = serde_json::from_str(
            &fs::read_to_string(modpack_path(&mc_folder, "pack").join("quadrantSync.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(sync_info.last_synced, 1_762_000_000);
        assert_eq!(sync_info.modpack_id.as_deref(), Some("modpack-7"));
    }

    /// Both guards must hold without a keyring or network, so reaching the
    /// submit at all would fail the test rather than pass it.
    #[tokio::test]
    async fn auto_sync_waits_for_the_setting_and_a_previous_sync() {
        let temp_dir = tempdir().unwrap();
        let (host, _mc_folder) = sync_host(temp_dir.path());

        host.set_config_value("autoQuadrantSync", json!(false))
            .unwrap();
        host.maybe_auto_sync_updated_modpack(Some(local_modpack(1_762_000_000_000)))
            .await
            .unwrap();

        host.set_config_value("autoQuadrantSync", json!(true))
            .unwrap();
        host.maybe_auto_sync_updated_modpack(Some(local_modpack(0)))
            .await
            .unwrap();
    }

    /// The listing the renderer sends can predate the damage, so the push
    /// checks the file itself, before it needs an account.
    #[tokio::test]
    async fn a_pack_with_an_unreadable_manifest_is_never_pushed() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder) = sync_host(temp_dir.path());
        fs::write(
            modpack_path(&mc_folder, "pack").join("modConfigV2.json"),
            "{not json",
        )
        .unwrap();

        let error = host
            .sync_modpack(local_modpack(1_762_000_000_000), false)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("can't read"), "{error}");

        let listed = host.get_modpacks(false).await.unwrap();
        let error = host
            .sync_modpack(listed[0].clone(), true)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("can't read"), "{error}");
        assert!(host.share_modpack("pack".to_string()).await.is_err());
    }

    fn synced_modpack(mods: &[InstalledMod]) -> SyncedModpack {
        SyncedModpack {
            name: "pack".to_string(),
            minecraft_version: "1.21.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: serde_json::to_string(mods).unwrap(),
            owners: Vec::new(),
            last_synced: 1_762_000_100,
            modpack_id: "modpack-7".to_string(),
        }
    }

    fn seed_synced_pack(host: &QuadrantHost, mc_folder: &Path, mods: Vec<InstalledMod>) -> String {
        let manifest = serde_json::to_string_pretty(&InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: String::new(),
            name: "pack".to_string(),
            version: "1.21.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods,
        })
        .unwrap();
        fs::write(
            modpack_path(mc_folder, "pack").join("modConfigV2.json"),
            &manifest,
        )
        .unwrap();
        host.persist_sync_metadata("pack", 1_762_000_000, Some("modpack-7"))
            .unwrap();
        manifest
    }

    #[tokio::test]
    async fn an_automatic_update_with_online_mods_waits_for_the_user() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder) = sync_host(temp_dir.path());
        let kept = InstalledMod::minimal(
            "kept".to_string(),
            ModSource::Modrinth,
            "https://cdn.modrinth.com/kept.jar".to_string(),
        );
        let manifest = seed_synced_pack(&host, &mc_folder, vec![kept.clone()]);
        let mut events = host.subscribe_events();
        let local = host.get_modpacks(false).await.unwrap().remove(0);
        let planted = InstalledMod {
            name: "Sodium".to_string(),
            ..InstalledMod::minimal(
                "sodium".to_string(),
                ModSource::Online,
                "https://evil.example/sodium.jar".to_string(),
            )
        };

        host.maybe_apply_remote_modpack_update(&local, &synced_modpack(&[kept, planted]))
            .await
            .unwrap();

        let envelope = events.recv().await.unwrap();
        assert_eq!(envelope.event, "modpackSyncWarning");
        assert_eq!(
            envelope.payload,
            json!({
                "modpackId": "modpack-7",
                "name": "pack",
                "applied": false,
                "onlineMods": ["Sodium"],
                "unverifiedMods": ["Sodium"],
            })
        );
        assert_eq!(
            fs::read_to_string(modpack_path(&mc_folder, "pack").join("modConfigV2.json")).unwrap(),
            manifest
        );
        let listed = host.get_modpacks(false).await.unwrap();
        assert_eq!(listed[0].last_synced, 1_762_000_000_000);
    }

    #[tokio::test]
    async fn automatic_updates_reach_only_the_pack_recording_the_synced_id() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder) = sync_host(temp_dir.path());
        seed_synced_pack(&host, &mc_folder, Vec::new());
        let synced = synced_modpack(&[]);
        assert!(
            host.resolve_local_modpack_for_sync(&synced)
                .await
                .unwrap()
                .is_some()
        );

        // Synced once under the same name, but recording no id.
        host.persist_sync_metadata("pack", 1_762_000_000, None)
            .unwrap();
        assert!(
            host.resolve_local_modpack_for_sync(&synced)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn supported_commands_include_settings_sync_surface() {
        let commands = QuadrantHost::supported_commands();
        assert!(commands.contains(&"get_quadrant_settings"));
        assert!(commands.contains(&"submit_quadrant_settings"));
        assert!(commands.contains(&"get_config_value"));
        assert!(commands.contains(&"set_config_value"));
        assert!(!commands.contains(&"open_modpacks_folder"));
        assert!(!commands.contains(&"request_check_for_updates"));
    }

    #[test]
    fn host_options_default_to_compatible_keyring_name() {
        let options =
            QuadrantHostOptions::new(PathBuf::from("C:/quadrant"), "client", "secret", "api-key");
        assert_eq!(options.keyring_service_name, KEYRING_SERVICE);
    }

    #[test]
    fn config_commands_round_trip_through_the_host_store() {
        let temp_dir = tempdir().unwrap();
        let host = QuadrantHost::new(QuadrantHostOptions::new(
            temp_dir.path().to_path_buf(),
            "client",
            "secret",
            "api-key",
        ))
        .unwrap();

        host.set_config_value("custom", Value::String("value".to_string()))
            .unwrap();
        assert_eq!(
            host.get_config_value("custom").unwrap(),
            Some(Value::String("value".to_string()))
        );
    }

    fn prism_host(temp_dir: &std::path::Path) -> (QuadrantHost, PathBuf, PathBuf) {
        let mc_folder = temp_dir.join(".minecraft");
        let prism_dir = temp_dir.join("PrismLauncher");
        fs::create_dir_all(mc_folder.join("modpacks").join("pack")).unwrap();
        fs::create_dir_all(prism_dir.join("instances")).unwrap();

        let host = QuadrantHost::new(QuadrantHostOptions::new(
            temp_dir.join("data"),
            "client",
            "secret",
            "api-key",
        ))
        .unwrap();
        host.set_config_value("mcFolder", json!(mc_folder)).unwrap();
        host.set_config_value("prismLauncherFolder", json!(prism_dir))
            .unwrap();
        (host, mc_folder, prism_dir)
    }

    fn write_prism_instance(prism_dir: &Path, id: &str, name: &str) -> PathBuf {
        let instance = prism_dir.join("instances").join(id);
        fs::create_dir_all(instance.join("minecraft")).unwrap();
        fs::write(instance.join("instance.cfg"), format!("name={name}\n")).unwrap();
        fs::write(
            instance.join("mmc-pack.json"),
            json!({
                "formatVersion": 1,
                "components": [{ "uid": "net.minecraft", "version": "1.21.1" }]
            })
            .to_string(),
        )
        .unwrap();
        instance
    }

    fn write_pack_manifest(instance: &Path, components: Value) {
        fs::write(
            instance.join("mmc-pack.json"),
            json!({ "formatVersion": 1, "components": components }).to_string(),
        )
        .unwrap();
    }

    /// A modpack manifest, so the listing reports a version and loader to sync
    /// an instance to.
    fn write_modpack_manifest(mc_folder: &Path, name: &str, version: &str, mod_loader: &str) {
        fs::create_dir_all(modpack_path(mc_folder, name)).unwrap();
        fs::write(
            modpack_path(mc_folder, name).join("modConfigV2.json"),
            json!({
                "modConfigVersion": "2",
                "quadrantVersion": "26.9.1",
                "name": name,
                "version": version,
                "modLoader": mod_loader,
                "mods": []
            })
            .to_string(),
        )
        .unwrap();
    }

    /// What one content type holds in a listed location.
    fn pack_names(location: &ContentLocation, mod_type: ModType) -> Vec<&str> {
        location
            .sections
            .iter()
            .find(|section| section.mod_type == mod_type)
            .map(|section| {
                section
                    .files
                    .iter()
                    .map(|file| file.file_name.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn removing_the_prism_folder_override_falls_back_to_discovery() {
        let temp_dir = tempdir().unwrap();
        let (host, _mc_folder, prism_dir) = prism_host(temp_dir.path());
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();

        assert_eq!(
            host.prism_instances_dir().unwrap(),
            Some(prism_dir.join("instances"))
        );

        host.remove_config_value("prismLauncherFolder").unwrap();

        assert_eq!(host.get_config_value("prismLauncherFolder").unwrap(), None);
        // Falls back to platform discovery, which finds nothing under a temp dir.
        assert_ne!(
            host.prism_instances_dir().unwrap(),
            Some(prism_dir.join("instances"))
        );
    }

    /// Older configs can still hold the JSON null the frontend used to write to
    /// clear a key, and that has to read as no override rather than as a path.
    #[test]
    fn a_stored_null_prism_folder_is_treated_as_no_override() {
        let temp_dir = tempdir().unwrap();
        let (host, _mc_folder, prism_dir) = prism_host(temp_dir.path());
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();

        host.set_config_value("prismLauncherFolder", Value::Null)
            .unwrap();

        assert_eq!(
            host.get_config_value("prismLauncherFolder").unwrap(),
            Some(Value::Null)
        );
        assert_ne!(
            host.prism_instances_dir().unwrap(),
            Some(prism_dir.join("instances"))
        );
    }

    #[test]
    fn prism_locations_are_listed_without_reading_the_pack_folders() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        fs::create_dir_all(mc_folder.join("resourcepacks")).unwrap();
        fs::write(mc_folder.join("resourcepacks").join("Faithful.zip"), "pack").unwrap();
        let game_dir = write_prism_instance(&prism_dir, "alpha", "Alpha").join("minecraft");
        fs::create_dir_all(game_dir.join("shaderpacks")).unwrap();
        fs::write(game_dir.join("shaderpacks").join("Bliss.zip"), "shader").unwrap();
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();

        let locations = host.get_installed_content(false).unwrap();

        assert_eq!(locations.len(), 2);
        assert_eq!(locations[1].id, "prism:alpha");
        assert_eq!(locations[1].name, "Alpha");
        assert_eq!(locations[1].path, game_dir.to_string_lossy());
        for location in &locations {
            assert_eq!(location.sections.len(), PACK_TYPES.len(), "{}", location.id);
            assert!(
                location
                    .sections
                    .iter()
                    .all(|section| section.files.is_empty()),
                "{}",
                location.id
            );
        }
    }

    #[tokio::test]
    async fn prism_instances_are_hidden_while_the_feature_is_off() {
        let temp_dir = tempdir().unwrap();
        let (host, _mc_folder, prism_dir) = prism_host(temp_dir.path());
        write_prism_instance(&prism_dir, "alpha", "Alpha");

        assert!(host.get_prism_instances().unwrap().is_empty());
        assert!(
            host.get_prism_sync_plans("pack".to_string())
                .await
                .unwrap()
                .is_empty()
        );
        for error in [
            host.apply_modpack_to_prism_instance("pack".to_string(), "alpha".to_string())
                .await
                .unwrap_err(),
            // Unlinking is behind the flag like everything else Prism: a user
            // who wants it back turns the feature on again.
            host.detach_prism_instance("alpha".to_string()).unwrap_err(),
        ] {
            assert!(error.to_string().contains("errorForbidden"), "{error}");
        }
    }

    #[tokio::test]
    async fn sync_plans_report_the_move_each_instance_would_make() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        write_pack_manifest(
            &write_prism_instance(&prism_dir, "alpha", "Alpha"),
            json!([
                { "uid": "net.minecraft", "version": "1.21.1" },
                { "uid": "net.fabricmc.fabric-loader", "version": "0.16.9" }
            ]),
        );
        write_pack_manifest(
            &write_prism_instance(&prism_dir, "beta", "Beta"),
            json!([{ "uid": "net.minecraft", "version": "1.20.1" }]),
        );
        write_modpack_manifest(&mc_folder, "pack", "1.21.1", "Fabric");
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();

        let plans = host.get_prism_sync_plans("pack".to_string()).await.unwrap();

        assert_eq!(
            plans,
            [
                // Already on 1.21.1, and Fabric needs no build of its own.
                PrismSyncPlan {
                    instance_id: "alpha".to_string(),
                    minecraft_version: None,
                    mod_loader: None,
                },
                PrismSyncPlan {
                    instance_id: "beta".to_string(),
                    minecraft_version: Some("1.21.1".to_string()),
                    mod_loader: Some(ModLoader::Fabric),
                },
            ]
        );
        assert!(
            host.get_prism_sync_plans("ghost".to_string())
                .await
                .unwrap_err()
                .to_string()
                .contains("errorModpackMissing")
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn resource_packs_follow_a_modpack_into_its_prism_instances() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        let instance = write_prism_instance(&prism_dir, "alpha", "Alpha");
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();

        // "pack" has no manifest, so it reports version "-" and no loader
        // version lookup is attempted.
        host.apply_modpack_to_prism_instance("pack".to_string(), "alpha".to_string())
            .await
            .unwrap();

        let instances = host.get_prism_instances().unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].applied_modpack.as_deref(), Some("pack"));
        assert_eq!(
            host.install_roots(ModType::ResourcePack, Some("pack"), None)
                .unwrap(),
            [instance.join("minecraft")]
        );
        // Mods still install into the Minecraft folder, whose own `mods` link is
        // what decides the modpack folder they land in.
        assert_eq!(
            host.install_roots(ModType::Mod, Some("pack"), None)
                .unwrap(),
            [mc_folder.clone()]
        );

        host.set_config_value("experimentalFeatures", json!(false))
            .unwrap();
        assert_eq!(
            host.install_roots(ModType::ResourcePack, Some("pack"), None)
                .unwrap(),
            [mc_folder]
        );

        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();
        host.detach_prism_instance("alpha".to_string()).unwrap();
        assert_eq!(host.get_prism_instances().unwrap()[0].applied_modpack, None);
    }

    #[test]
    fn installed_content_covers_the_minecraft_folder_and_every_prism_instance() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        fs::create_dir_all(mc_folder.join("resourcepacks")).unwrap();
        fs::write(mc_folder.join("resourcepacks").join("Faithful.zip"), "pack").unwrap();
        let game_dir = write_prism_instance(&prism_dir, "alpha", "Alpha").join("minecraft");
        fs::create_dir_all(game_dir.join("shaderpacks")).unwrap();
        fs::write(game_dir.join("shaderpacks").join("Bliss.zip"), "shader").unwrap();

        let locations = host.get_installed_content(true).unwrap();
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].id, "minecraft");
        assert_eq!(locations[0].kind, ContentLocationKind::Minecraft);
        assert_eq!(locations[0].name, "");
        assert_eq!(locations[0].path, mc_folder.to_string_lossy());
        assert_eq!(
            pack_names(&locations[0], ModType::ResourcePack),
            ["Faithful.zip"]
        );
        assert!(pack_names(&locations[0], ModType::ShaderPack).is_empty());

        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();
        let locations = host.get_installed_content(true).unwrap();
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[1].id, "prism:alpha");
        assert_eq!(locations[1].kind, ContentLocationKind::Prism);
        assert_eq!(locations[1].name, "Alpha");
        assert_eq!(locations[1].path, game_dir.to_string_lossy());
        assert_eq!(
            pack_names(&locations[1], ModType::ShaderPack),
            ["Bliss.zip"]
        );
        assert!(pack_names(&locations[1], ModType::ResourcePack).is_empty());
    }

    #[test]
    fn content_folder_resolves_only_known_locations_and_pack_types() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        let game_dir = write_prism_instance(&prism_dir, "alpha", "Alpha").join("minecraft");

        assert_eq!(
            host.content_folder("minecraft", ModType::ResourcePack)
                .unwrap(),
            mc_folder.join("resourcepacks")
        );
        assert_eq!(
            host.content_folder("minecraft", ModType::ShaderPack)
                .unwrap(),
            mc_folder.join("shaderpacks")
        );
        for (location_id, mod_type, expected) in [
            ("minecraft", ModType::Mod, "errorInvalidRequest"),
            ("nowhere", ModType::ResourcePack, "errorInvalidRequest"),
            // Prism locations stay closed while the feature is off.
            ("prism:alpha", ModType::ResourcePack, "errorForbidden"),
        ] {
            assert!(
                host.content_folder(location_id, mod_type)
                    .unwrap_err()
                    .to_string()
                    .contains(expected),
                "{location_id:?}"
            );
        }

        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();
        assert_eq!(
            host.content_folder("prism:alpha", ModType::ShaderPack)
                .unwrap(),
            game_dir.join("shaderpacks")
        );
        for (location_id, expected) in [
            ("prism:ghost", "errorPrismInstanceMissing"),
            ("prism:../alpha", "errorInvalidRequest"),
            ("prism:", "errorInvalidRequest"),
        ] {
            assert!(
                host.content_folder(location_id, ModType::ResourcePack)
                    .unwrap_err()
                    .to_string()
                    .contains(expected),
                "{location_id:?}"
            );
        }
    }

    #[test]
    fn install_roots_resolve_a_named_content_location_and_stay_gated() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        let game_dir = write_prism_instance(&prism_dir, "alpha", "Alpha").join("minecraft");
        let minecraft_only = vec![mc_folder];
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();

        assert_eq!(
            host.install_roots(ModType::ResourcePack, Some("pack"), Some("prism:alpha"))
                .unwrap(),
            [game_dir]
        );
        assert_eq!(
            host.install_roots(ModType::ShaderPack, None, Some("minecraft"))
                .unwrap(),
            minecraft_only
        );
        // No location falls back to following the modpack, which is applied
        // nowhere here.
        assert_eq!(
            host.install_roots(ModType::ResourcePack, Some("pack"), None)
                .unwrap(),
            minecraft_only
        );

        for (location_id, expected) in [
            ("nowhere", "errorInvalidRequest"),
            // The automatic placement is an absent location, not an empty one.
            ("", "errorInvalidRequest"),
            ("prism:ghost", "errorPrismInstanceMissing"),
        ] {
            assert!(
                host.install_roots(ModType::ResourcePack, None, Some(location_id))
                    .unwrap_err()
                    .to_string()
                    .contains(expected),
                "{location_id:?}"
            );
        }

        host.set_config_value("experimentalFeatures", json!(false))
            .unwrap();
        assert!(
            host.install_roots(ModType::ResourcePack, None, Some("prism:alpha"))
                .unwrap_err()
                .to_string()
                .contains("errorForbidden")
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn deleting_a_modpack_gives_its_prism_instances_their_mods_back() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        let instance = write_prism_instance(&prism_dir, "alpha", "Alpha");
        // "pack" has no manifest, so it reports version "-" and applying it
        // syncs no components and resolves no loader build.
        let mods_path = instance.join("minecraft").join("mods");
        fs::create_dir_all(&mods_path).unwrap();
        fs::write(mods_path.join("loose.jar"), "jar").unwrap();
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();
        host.apply_modpack_to_prism_instance("pack".to_string(), "alpha".to_string())
            .await
            .unwrap();
        // Unlinking is maintenance, so it happens with the feature off too.
        host.set_config_value("experimentalFeatures", json!(false))
            .unwrap();

        host.delete_modpack("pack".to_string()).await.unwrap();

        assert!(!mods_path.is_symlink());
        assert!(mods_path.join("loose.jar").exists());
        assert!(
            !instance
                .join(quadrant_core::prism::PACK_MANIFEST_BACKUP)
                .exists()
        );
        assert!(!modpack_path(&mc_folder, "pack").exists());
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn renaming_a_modpack_follows_it_in_every_prism_instance() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        let instance = write_prism_instance(&prism_dir, "alpha", "Alpha");
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();
        host.apply_modpack_to_prism_instance("pack".to_string(), "alpha".to_string())
            .await
            .unwrap();
        host.set_config_value("experimentalFeatures", json!(false))
            .unwrap();

        host.update_modpack("pack".to_string(), Some("renamed".to_string()), None, None)
            .await
            .unwrap();

        let mods_path = instance.join("minecraft").join("mods");
        assert_eq!(
            mods_path.read_link().unwrap(),
            modpack_path(&mc_folder, "renamed")
        );
        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();
        assert_eq!(
            host.get_prism_instances().unwrap()[0]
                .applied_modpack
                .as_deref(),
            Some("renamed")
        );
    }

    #[tokio::test]
    async fn modpack_lifecycle_link_maintenance_is_a_no_op_without_prism() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, _prism_dir) = prism_host(temp_dir.path());
        host.set_config_value("prismLauncherFolder", json!(temp_dir.path().join("gone")))
            .unwrap();

        host.update_modpack("pack".to_string(), Some("renamed".to_string()), None, None)
            .await
            .unwrap();
        host.delete_modpack("renamed".to_string()).await.unwrap();

        assert!(!modpack_path(&mc_folder, "renamed").exists());
    }

    #[test]
    fn copy_content_copies_packs_between_locations_both_ways() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        let game_dir = write_prism_instance(&prism_dir, "alpha", "Alpha").join("minecraft");
        fs::create_dir_all(mc_folder.join("resourcepacks")).unwrap();
        fs::write(mc_folder.join("resourcepacks").join("Faithful.zip"), "pack").unwrap();

        assert!(
            host.copy_content(
                "minecraft",
                "prism:alpha",
                ModType::ResourcePack,
                vec!["Faithful.zip".to_string()],
            )
            .unwrap_err()
            .to_string()
            .contains("errorForbidden")
        );

        host.set_config_value("experimentalFeatures", json!(true))
            .unwrap();
        assert_eq!(
            host.copy_content(
                "minecraft",
                "prism:alpha",
                ModType::ResourcePack,
                vec!["Faithful.zip".to_string()],
            )
            .unwrap(),
            1
        );
        assert_eq!(
            fs::read_to_string(game_dir.join("resourcepacks").join("Faithful.zip")).unwrap(),
            "pack"
        );

        fs::write(game_dir.join("resourcepacks").join("Bliss.zip"), "other").unwrap();
        assert_eq!(
            host.copy_content(
                "prism:alpha",
                "minecraft",
                ModType::ResourcePack,
                vec!["Bliss.zip".to_string()],
            )
            .unwrap(),
            1
        );
        assert_eq!(
            pack_names(
                &host.get_installed_content(true).unwrap()[0],
                ModType::ResourcePack
            ),
            ["Bliss.zip", "Faithful.zip"]
        );
    }

    #[test]
    fn delete_content_removes_packs_only_from_locations_it_can_reach() {
        let temp_dir = tempdir().unwrap();
        let (host, mc_folder, prism_dir) = prism_host(temp_dir.path());
        let game_dir = write_prism_instance(&prism_dir, "alpha", "Alpha").join("minecraft");
        fs::create_dir_all(mc_folder.join("resourcepacks")).unwrap();
        fs::write(mc_folder.join("resourcepacks").join("Faithful.zip"), "pack").unwrap();
        fs::write(mc_folder.join("resourcepacks").join("Keep.zip"), "kept").unwrap();
        fs::create_dir_all(game_dir.join("resourcepacks")).unwrap();
        fs::write(game_dir.join("resourcepacks").join("Bliss.zip"), "other").unwrap();

        assert!(
            host.delete_content(
                "prism:alpha",
                ModType::ResourcePack,
                vec!["Bliss.zip".to_string()],
            )
            .unwrap_err()
            .to_string()
            .contains("errorForbidden")
        );
        assert!(
            host.delete_content("minecraft", ModType::Mod, vec!["Faithful.zip".to_string()])
                .unwrap_err()
                .to_string()
                .contains("errorInvalidRequest")
        );
        assert!(game_dir.join("resourcepacks").join("Bliss.zip").is_file());

        assert_eq!(
            host.delete_content(
                "minecraft",
                ModType::ResourcePack,
                vec!["Faithful.zip".to_string()],
            )
            .unwrap(),
            1
        );
        assert_eq!(
            pack_names(
                &host.get_installed_content(true).unwrap()[0],
                ModType::ResourcePack
            ),
            ["Keep.zip"]
        );
    }

    #[test]
    fn notification_websocket_preserves_custom_api_base_path() {
        let temp_dir = tempdir().unwrap();
        let mut options =
            QuadrantHostOptions::new(temp_dir.path().to_path_buf(), "client", "secret", "api-key");
        options.api_base_url = Some("https://example.test/custom/api".to_string());
        let host = QuadrantHost::new(options).unwrap();

        let url = host
            .build_notification_ws_url(&NotificationCursor::default(), "connection")
            .unwrap();
        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.path(), "/custom/api/account/notifications/ws");
    }

    #[tokio::test]
    async fn event_subscription_receives_emitted_envelopes() {
        let temp_dir = tempdir().unwrap();
        let host = QuadrantHost::new(QuadrantHostOptions::new(
            temp_dir.path().to_path_buf(),
            "client",
            "secret",
            "api-key",
        ))
        .unwrap();

        let mut receiver = host.subscribe_events();
        host.inner
            .event_sink
            .emit("refreshSyncedModpacks", "modpack-1")
            .unwrap();

        let envelope: HostEventEnvelope = receiver.recv().await.unwrap();
        assert_eq!(envelope.event, "refreshSyncedModpacks");
        assert_eq!(envelope.payload, Value::String("modpack-1".to_string()));
    }

    #[test]
    fn json_store_entries_and_deletes_round_trip_across_instances() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("config.json");

        let store = JsonFileStore::new(path.clone()).unwrap();
        store.set_string("mcFolder", "/tmp/mc").unwrap();
        store.set_i64("lastPage", 3).unwrap();
        store.set_bool("modrinth", true).unwrap();

        let reloaded = JsonFileStore::new(path.clone()).unwrap();
        let entries = reloaded.entries().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            reloaded.get_string("mcFolder").unwrap().as_deref(),
            Some("/tmp/mc")
        );
        assert_eq!(reloaded.get_i64("lastPage").unwrap(), Some(3));
        assert_eq!(reloaded.get_bool("modrinth").unwrap(), Some(true));

        reloaded.delete_key("lastPage").unwrap();

        let after_delete = JsonFileStore::new(path).unwrap();
        assert_eq!(after_delete.get_i64("lastPage").unwrap(), None);
        assert_eq!(after_delete.entries().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn event_bridge_broadcasts_published_backend_events() {
        let bridge = HostEventBridge::new();
        let mut receiver = bridge.subscribe();

        bridge
            .publish(BackendEvent::ModDownloadProgress(ModProgressPayload {
                mod_id: "mod-1".to_string(),
                progress: 50,
            }))
            .unwrap();
        bridge.publish(BackendEvent::RecheckAccountToken).unwrap();

        let envelope = receiver.recv().await.unwrap();
        assert_eq!(envelope.event, "modDownloadProgress");
        assert_eq!(
            envelope.payload,
            json!({ "modId": "mod-1", "progress": 50 })
        );

        let envelope = receiver.recv().await.unwrap();
        assert_eq!(envelope.event, "recheckAccountToken");
        assert_eq!(envelope.payload, Value::Null);
    }

    #[test]
    fn host_new_materializes_defaults_and_mc_folder() {
        let temp_dir = tempdir().unwrap();
        let mc_folder = temp_dir.path().join("mc");
        let mut options =
            QuadrantHostOptions::new(temp_dir.path().to_path_buf(), "client", "secret", "api-key");
        options.mc_folder = Some(mc_folder.clone());
        let host = QuadrantHost::new(options).unwrap();

        assert_eq!(
            host.get_config_value("curseforge").unwrap(),
            Some(Value::Bool(true))
        );
        assert_eq!(
            host.get_config_value("mcFolder").unwrap(),
            Some(Value::String(mc_folder.to_string_lossy().to_string()))
        );
    }

    #[tokio::test]
    async fn event_bridge_forwards_config_changes() {
        let bridge = HostEventBridge::new();
        let mut receiver = bridge.subscribe();

        bridge
            .publish(BackendEvent::ConfigChanged("uiScale".to_string()))
            .unwrap();

        let envelope = receiver.recv().await.unwrap();
        assert_eq!(envelope.event, "configChanged");
        assert_eq!(envelope.payload, json!("uiScale"));
    }

    #[tokio::test]
    async fn event_bridge_forwards_sync_warnings_in_camel_case() {
        let bridge = HostEventBridge::new();
        let mut receiver = bridge.subscribe();

        bridge
            .publish(BackendEvent::ModpackSyncWarning {
                modpack_id: "modpack-7".to_string(),
                name: "pack".to_string(),
                applied: true,
                online_mods: Vec::new(),
                unverified_mods: vec!["Sodium".to_string(), "Lithium".to_string()],
            })
            .unwrap();

        let envelope = receiver.recv().await.unwrap();
        assert_eq!(envelope.event, "modpackSyncWarning");
        assert_eq!(
            envelope.payload,
            json!({
                "modpackId": "modpack-7",
                "name": "pack",
                "applied": true,
                "onlineMods": [],
                "unverifiedMods": ["Sodium", "Lithium"],
            })
        );
    }
}
