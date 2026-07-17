use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, Once, OnceLock},
    time::Duration,
};

use anyhow::anyhow;
use futures::StreamExt;
use keyring_core::{Entry, Error as KeyringError, set_default_store};
use quadrant_core::{
    Error, Result,
    account::{
        KEYRING_SERVICE, account_token_needs_refresh, clear_account_token,
        id::{
            AccountInfo, Notification, NotificationCursor, NotificationWsFrame,
            get_account_info_with_refresh, get_notification_history_all_since_with_refresh,
            oauth2_login, read_notification, try_refresh_token,
        },
        quadrant_settings_sync,
        quadrant_share::{
            QuadrantShareSubmissionResponse, get_quadrant_share_modpack, share_modpack_raw,
        },
        quadrant_sync::{
            SyncedModpack, answer_invite, delete_synced_modpack, get_synced_modpacks,
            invite_member, kick_member,
        },
        set_secret,
    },
    config::{ensure_default_app_config, get_mc_folder},
    events::BackendEvent,
    mc_mod::{
        GetModArgs, GlobalSearchModsArgs, IdentifiedMod, MinecraftVersion, Mod, ModType,
        SearchCategory, UniversalModFile, check_mod_updates, get_categories, get_user_agent,
        get_versions, identify_modpack, install_mod, install_remote_file, search_mods,
    },
    models::{Article, InstalledMod, InstalledModpack, LocalModpack, ModLoader, ModSource},
    modpacks::{
        apply_modpack, create_modpack, delete_mod, delete_modpack, export_modpack_to, get_modpacks,
        install_modpack, register_mod, set_modpack_sync_date, update_modpack,
    },
    ports::{EventSink, SecretStore, SettingsStore},
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

#[cfg(feature = "curseforge")]
pub use quadrant_core::mc_mod::curseforge::{
    get_mod_curseforge, get_mod_deps_curseforge, get_mod_owners_curseforge,
};
pub use quadrant_core::mc_mod::modrinth::{
    get_mod_deps_modrinth, get_mod_modrinth, get_mod_owners_modrinth,
};
pub use quadrant_core::mc_mod::{get_mod_url, get_user_url};

const NOTIFICATION_CURSOR_CREATED_AT_KEY: &str = "notificationCursorCreatedAt";
const NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY: &str = "notificationCursorNotificationId";
const SETTINGS_SYNC_INTERVAL_SECS: u64 = 120;
/// How often the background worker checks whether the access token is due for a
/// proactive refresh. Refresh timing is governed by the stored deadline, not
/// this interval, so a short poll just bounds how late a refresh can fire.
const TOKEN_REFRESH_CHECK_INTERVAL_SECS: u64 = 60;
const WS_REPLAY_LIMIT: usize = 500;
const REFRESH_SYNCED_MODPACKS_EVENT: &str = "refreshSyncedModpacks";
static LOGGER_INIT: Once = Once::new();
static KEYRING_STORE_INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

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
    values: Arc<Mutex<HashMap<String, Value>>>,
}

impl JsonFileStore {
    fn new(path: PathBuf) -> Result<Self> {
        let values = Self::read_values_from_disk(&path)?;
        Ok(Self {
            path,
            values: Arc::new(Mutex::new(values)),
        })
    }

    fn read_values_from_disk(path: &PathBuf) -> Result<HashMap<String, Value>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let raw = fs::read_to_string(path)?;
        let parsed = serde_json::from_str::<Value>(&raw)
            .map_err(|error| anyhow!("failed to parse {}: {error}", path.display()))?;
        Ok(parsed
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<_, _>>())
    }

    fn reload(&self, values: &mut HashMap<String, Value>) -> Result<()> {
        match Self::read_values_from_disk(&self.path) {
            Ok(updated_values) => *values = updated_values,
            Err(error) => {
                log::warn!(
                    "Failed to reload settings store {}: {error}",
                    self.path.display()
                );
            }
        }
        Ok(())
    }

    fn delete_key(&self, key: &str) -> Result<()> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| anyhow!("settings store is busy"))?;
        self.reload(&mut values)?;
        values.remove(key);
        self.persist(&values)
    }

    fn persist(&self, values: &HashMap<String, Value>) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = self.path.with_extension(format!("{}.tmp", Uuid::now_v7()));
        fs::write(
            &temp_path,
            serde_json::to_vec_pretty(&Value::Object(values.clone().into_iter().collect()))?,
        )?;
        fs::rename(temp_path, &self.path)?;
        Ok(())
    }
}

impl SettingsStore for JsonFileStore {
    fn get_value(&self, key: &str) -> Result<Option<Value>> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| anyhow!("settings store is busy"))?;
        self.reload(&mut values)?;
        Ok(values.get(key).cloned())
    }

    fn set_value(&self, key: &str, value: Value) -> Result<()> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| anyhow!("settings store is busy"))?;
        self.reload(&mut values)?;
        values.insert(key.to_string(), value);
        self.persist(&values)
    }

    fn entries(&self) -> Result<Vec<(String, Value)>> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| anyhow!("settings store is busy"))?;
        self.reload(&mut values)?;
        Ok(values
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
    match KEYRING_STORE_INIT.get_or_init(|| init_keyring_store().map_err(|error| error.to_string()))
    {
        Ok(()) => Ok(()),
        Err(error) => Err(anyhow!("failed to initialize keyring store: {error}").into()),
    }
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
        }
    }
}

impl QuadrantHost {
    pub fn new(options: QuadrantHostOptions) -> Result<Self> {
        LOGGER_INIT.call_once(|| {
            colog::init();
        });
        fs::create_dir_all(&options.data_dir)?;
        if let Some(api_base_url) = &options.api_base_url {
            unsafe {
                std::env::set_var("QUADRANT_API_BASE_URL", api_base_url);
            }
        }

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

    pub fn get_minecraft_folder(&self) -> Result<PathBuf> {
        self.inner
            .config_store
            .get_string("mcFolder")?
            .map(PathBuf::from)
            .or_else(|| get_mc_folder().ok().flatten())
            .ok_or_else(|| anyhow!("mcFolder is not configured"))
    }

    pub fn get_modpacks_folder(&self) -> Result<PathBuf> {
        Ok(self.get_minecraft_folder()?.join("modpacks"))
    }

    pub async fn get_modpacks(&self, hide_free: bool) -> Result<Vec<LocalModpack>> {
        get_modpacks(&self.get_minecraft_folder()?, hide_free).await
    }

    pub fn frontend_apply_modpack(&self, name: String) -> Result<()> {
        apply_modpack(&self.get_minecraft_folder()?, &name)
    }

    pub async fn delete_mod(&self, modpack_name: String, mod_id: String) -> Result<()> {
        let modpack = delete_mod(
            &self.get_minecraft_folder()?,
            &self.get_modpacks(false).await?,
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
        let modpack = update_modpack(
            &self.get_minecraft_folder()?,
            &self.get_modpacks(false).await?,
            &modpack_source,
            name,
            version,
            mod_loader,
        )?;
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
            &self.get_modpacks(false).await?,
            &name,
            &version,
            mod_loader,
        )
    }

    pub async fn delete_modpack(&self, name: String) -> Result<()> {
        delete_modpack(
            &self.get_minecraft_folder()?,
            &self.get_modpacks(false).await?,
            &name,
        )
    }

    pub async fn register_mod(&self, mod_: InstalledMod, modpack: String) -> Result<()> {
        register_mod(
            &self.get_minecraft_folder()?,
            &self.get_modpacks(false).await?,
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
        export_modpack_to(
            &self.get_minecraft_folder()?,
            &modpack,
            &destination,
            &self.inner.event_sink,
        )
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
        let modpack = self
            .get_modpacks(false)
            .await?
            .into_iter()
            .find(|modpack| modpack.name == modpack_name)
            .ok_or_else(|| anyhow!("Modpack not found"))?;
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
    ) -> Result<()> {
        let updated_modpack = install_mod(
            &self.get_minecraft_folder()?,
            &self.get_modpacks(false).await?,
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
        let updated_modpack = install_remote_file(
            &self.get_minecraft_folder()?,
            &self.get_modpacks(false).await?,
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

    pub fn set_secret(&self, key: String, value: String) -> Result<()> {
        set_secret(&self.inner.secret_store, &key, &value)
    }

    pub fn clear_account_token(&self) -> Result<()> {
        clear_account_token(&self.inner.secret_store)
    }

    pub fn logout(&self) -> Result<()> {
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

    pub async fn oauth2_login(&self, code: String, redirect_uri: String) -> Result<()> {
        oauth2_login(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            code,
            redirect_uri,
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
        let connection_id = {
            let runtime = self.inner.runtime_state.lock().await;
            runtime.notification_connection_id.clone()
        };

        let timestamp = quadrant_core::account::quadrant_sync::sync_modpack(
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            modpack.clone(),
            overwrite,
            Some(connection_id.as_str()),
        )
        .await?;

        let persisted_modpack_id = self
            .resolve_submitted_modpack_id(&modpack, timestamp)
            .await?
            .or_else(|| modpack.modpack_id.clone());
        self.persist_sync_metadata(
            &modpack.name,
            timestamp as u64,
            persisted_modpack_id.as_deref(),
        )
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
        let modpack = self
            .get_modpacks(false)
            .await?
            .into_iter()
            .find(|modpack| modpack.name == modpack_name)
            .ok_or_else(|| anyhow!("Modpack not found"))?;
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

    pub async fn get_quadrant_settings(&self) -> Result<()> {
        quadrant_settings_sync::get_quadrant_settings(
            &self.inner.config_store,
            &self.inner.secret_store,
            &self.inner.options.user_agent,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
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
            "set_secret",
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
            "set_secret" => {
                let args: SetSecretArgs = from_value(payload)?;
                self.set_secret(args.key, args.value)?;
                Ok(Value::Null)
            }
            "clear_account_token" => {
                self.clear_account_token()?;
                Ok(Value::Null)
            }
            "get_account_info" => to_value(self.get_account_info().await?),
            "oauth2_login" => {
                let args: OAuthLoginArgs = from_value(payload)?;
                self.oauth2_login(args.code, args.redirect_uri).await?;
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

        if modpack.last_synced != 0 && auto_sync {
            self.sync_modpack(modpack, true).await?;
        }

        Ok(())
    }

    async fn resolve_submitted_modpack_id(
        &self,
        modpack: &LocalModpack,
        timestamp: i64,
    ) -> Result<Option<String>> {
        let synced_modpacks = self.get_synced_modpacks(false, None).await?;

        let mut matching = synced_modpacks.into_iter().filter(|synced_modpack| {
            synced_modpack.name == modpack.name
                && synced_modpack.minecraft_version == modpack.version
                && synced_modpack.mod_loader == modpack.mod_loader
                && synced_modpack.last_synced == timestamp
        });

        let first = matching.next().map(|modpack| modpack.modpack_id);
        if matching.next().is_some() {
            return Ok(None);
        }

        Ok(first)
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
        loop {
            if let Err(error) = self.bootstrap_notifications().await {
                log::error!("Notification bootstrap failed: {error}");
            }

            if let Err(error) = self.run_notification_socket().await {
                log::warn!("Notification socket cycle ended: {error}");
            }

            if let Err(error) = self.bootstrap_notifications().await {
                log::warn!("Notification catch-up failed after socket cycle: {error}");
            }

            let delay = {
                let mut state = self.inner.runtime_state.lock().await;
                state.notification_state.reconnect_attempt =
                    state.notification_state.reconnect_attempt.saturating_add(1);
                reconnect_delay(state.notification_state.reconnect_attempt)
            };
            tokio::time::sleep(delay).await;
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
        loop {
            interval.tick().await;
            if let Err(error) = self.refresh_account_token_if_due().await {
                log::warn!("Proactive token refresh worker failed: {error}");
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
    /// the frontend to re-read account state.
    pub async fn refresh_account_token(&self) -> Result<()> {
        try_refresh_token(
            &self.inner.secret_store,
            &self.inner.options.oauth_client_id,
            &self.inner.options.oauth_client_secret,
            &self.inner.options.user_agent,
        )
        .await?;
        self.inner
            .event_sink
            .emit("recheckAccountToken", Value::Null)
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

        for notification in merge_outcome.modpack_sync_notifications {
            if let Err(error) = self.handle_modpack_sync_notification(&notification).await {
                log::warn!("Failed to process bootstrapped modpack sync notification: {error}");
            } else {
                self.mark_modpack_sync_processed(&notification).await;
            }
        }

        self.set_reconnect_attempt(0).await;
        Ok(())
    }

    async fn run_notification_socket(&self) -> Result<()> {
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

        let (mut socket, _) = connect_async(request).await?;
        self.set_reconnect_attempt(0).await;

        while let Some(frame) = socket.next().await {
            match frame? {
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

        Err(anyhow!("notification websocket stream ended"))
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

        if auto_quadrant_sync {
            let local_modpack = self.resolve_local_modpack_for_sync(&synced_modpack).await?;
            let local_modpack = self
                .maybe_backfill_local_modpack_id(local_modpack, &synced_modpack)
                .await?;

            if let Some(local_modpack) = local_modpack {
                self.maybe_apply_remote_modpack_update(&local_modpack, &synced_modpack)
                    .await?;
            }
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

    async fn resolve_local_modpack_for_sync(
        &self,
        synced_modpack: &SyncedModpack,
    ) -> Result<Option<LocalModpack>> {
        let modpacks = get_modpacks(&self.get_minecraft_folder()?, true).await?;

        if let Some(local_modpack) = modpacks
            .iter()
            .find(|modpack| {
                modpack.modpack_id.as_deref() == Some(synced_modpack.modpack_id.as_str())
            })
            .cloned()
        {
            return Ok(Some(local_modpack));
        }

        Ok(modpacks.into_iter().find(|modpack| {
            modpack.modpack_id.is_none()
                && modpack.last_synced != 0
                && modpack.name == synced_modpack.name
        }))
    }

    async fn maybe_backfill_local_modpack_id(
        &self,
        local_modpack: Option<LocalModpack>,
        synced_modpack: &SyncedModpack,
    ) -> Result<Option<LocalModpack>> {
        let Some(mut local_modpack) = local_modpack else {
            return Ok(None);
        };

        if local_modpack.modpack_id.as_deref() == Some(synced_modpack.modpack_id.as_str()) {
            return Ok(Some(local_modpack));
        }
        if local_modpack.modpack_id.is_some() {
            return Ok(None);
        }

        let local_sync_time = u64::try_from(local_modpack.last_synced / 1000).unwrap_or_default();
        self.persist_sync_metadata(
            &local_modpack.name,
            local_sync_time,
            Some(synced_modpack.modpack_id.as_str()),
        )?;
        local_modpack.modpack_id = Some(synced_modpack.modpack_id.clone());
        Ok(Some(local_modpack))
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

        let remote_mods = serde_json::from_str(&synced_modpack.mods)?;
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
        result
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

        if let Err(error) = self.get_quadrant_settings().await {
            if error.to_string() == "Current settings are newer" {
                self.submit_quadrant_settings().await?;
            } else {
                return Err(error);
            }
        }

        Ok(())
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

    async fn set_reconnect_attempt(&self, attempt: u32) {
        let mut state = self.inner.runtime_state.lock().await;
        state.notification_state.reconnect_attempt = attempt;
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
    ordered_keys: Vec<String>,
    processed_modpack_sync_by_key: HashMap<String, String>,
    cursor: NotificationCursor,
    reconnect_attempt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UpsertOutcome {
    changed: bool,
}

struct MergeNotificationsOutcome {
    notifications_for_ui: Option<Vec<Notification>>,
    modpack_sync_notifications: Vec<Notification>,
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
                self.sort_keys();
                UpsertOutcome { changed: true }
            }
            None => {
                self.ordered_keys.push(identity_key.clone());
                self.by_key
                    .insert(identity_key.clone(), notification.clone());
                self.key_by_notification_id
                    .insert(notification.notification_id.clone(), identity_key);
                self.sort_keys();
                UpsertOutcome { changed: true }
            }
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
        let mut notifications = self
            .ordered_keys
            .iter()
            .filter_map(|key| self.by_key.get(key))
            .cloned()
            .collect::<Vec<_>>();
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

    fn sort_keys(&mut self) {
        self.ordered_keys.sort_by(|left, right| {
            let left_notification = self.by_key.get(left).expect("missing notification");
            let right_notification = self.by_key.get(right).expect("missing notification");
            left_notification
                .created_at_unix
                .cmp(&right_notification.created_at_unix)
                .then_with(|| {
                    left_notification
                        .notification_id
                        .cmp(&right_notification.notification_id)
                })
        });
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

    for notification in notifications {
        let should_queue_modpack_sync =
            runtime.should_process_modpack_sync_notification(&notification);
        let outcome = runtime.upsert(notification.clone());
        changed |= outcome.changed;

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
        .filter(|payload| payload.notification_type == "modpack_sync")
}

fn is_modpack_sync_notification(notification: &Notification) -> bool {
    notification.notification_type.as_deref() == Some("modpack_sync")
}

fn notification_identity_key(notification: &Notification) -> String {
    if is_modpack_sync_notification(notification)
        && let Some(resource_id) = notification.resource_id.as_deref()
    {
        return format!("modpack_sync:{resource_id}");
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
struct SetSecretArgs {
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthLoginArgs {
    code: String,
    redirect_uri: String,
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
        NotificationRuntimeState, QuadrantHost, QuadrantHostOptions,
        merge_notifications_and_collect_updates,
    };
    use quadrant_core::{
        account::KEYRING_SERVICE,
        events::{BackendEvent, ModProgressPayload},
        ports::{EventSink, SettingsStore},
    };
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use tempfile::tempdir;

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

    #[test]
    fn json_store_persists_updates() {
        let temp_dir = tempdir().unwrap();
        let store = JsonFileStore::new(temp_dir.path().join("config.json")).unwrap();
        store.set_bool("modrinth", true).unwrap();
        let reloaded = JsonFileStore::new(temp_dir.path().join("config.json")).unwrap();
        assert_eq!(reloaded.get_bool("modrinth").unwrap(), Some(true));
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
}
