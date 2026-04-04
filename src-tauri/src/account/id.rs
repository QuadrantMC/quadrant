use std::{collections::HashMap, time::Duration};

use anyhow::anyhow;
use futures::StreamExt;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use url::Url;

use crate::{
    AppState,
    account::{
        self,
        quadrant_settings_sync::{get_quadrant_settings, submit_quadrant_settings},
        quadrant_sync::{SyncedModpack, get_synced_modpacks, persist_sync_metadata},
    },
    mc_mod::get_user_agent,
    modpacks::general::{InstalledModpack, LocalModpack, install_modpack},
    tauri_adapter::{TauriSecretStore, mc_folder},
};

pub use quadrant_core::account::id::{
    AccountInfo, Notification, NotificationCursor, NotificationWsFrame, OAuth2Response,
};

const NOTIFICATION_CURSOR_CREATED_AT_KEY: &str = "notificationCursorCreatedAt";
const NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY: &str = "notificationCursorNotificationId";
const SHOWN_NOTIFICATIONS_KEY: &str = "shownNotifications";
const SETTINGS_SYNC_INTERVAL_SECS: u64 = 120;
const WS_REPLAY_LIMIT: usize = 500;
const NOTIFICATION_TITLE: &str = "Quadrant ID";
const REFRESH_SYNCED_MODPACKS_EVENT: &str = "refreshSyncedModpacks";

#[derive(Default, Clone)]
pub struct NotificationRuntimeState {
    pub by_key: HashMap<String, Notification>,
    pub key_by_notification_id: HashMap<String, String>,
    pub ordered_keys: Vec<String>,
    pub processed_modpack_sync_by_key: HashMap<String, String>,
    pub cursor: NotificationCursor,
    pub reconnect_attempt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UpsertOutcome {
    changed: bool,
    inserted: bool,
}

struct MergeNotificationsOutcome {
    notifications_for_ui: Option<Vec<Notification>>,
    modpack_sync_notifications: Vec<Notification>,
}

impl NotificationRuntimeState {
    fn upsert(&mut self, notification: Notification) -> UpsertOutcome {
        self.advance_cursor(&notification);
        let identity_key = notification_identity_key(&notification);
        match self.by_key.get(&identity_key) {
            Some(existing) if existing == &notification => UpsertOutcome {
                changed: false,
                inserted: false,
            },
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
                UpsertOutcome {
                    changed: true,
                    inserted: false,
                }
            }
            None => {
                self.ordered_keys.push(identity_key.clone());
                self.by_key
                    .insert(identity_key.clone(), notification.clone());
                self.key_by_notification_id
                    .insert(notification.notification_id.clone(), identity_key);
                self.sort_keys();
                UpsertOutcome {
                    changed: true,
                    inserted: true,
                }
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

#[derive(Debug, Deserialize)]
struct ModpackSyncPayload {
    notification_type: String,
    modpack: SyncedModpack,
}

#[tauri::command]
pub async fn get_account_info() -> Result<AccountInfo, tauri::Error> {
    quadrant_core::account::id::get_account_info_with_refresh(
        &TauriSecretStore,
        &get_user_agent(),
        env!("QUADRANT_OAUTH2_CLIENT_ID"),
        env!("QUADRANT_OAUTH2_CLIENT_SECRET"),
    )
    .await
    .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn oauth2_login(
    code: String,
    redirect_uri: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    quadrant_core::account::id::oauth2_login(
        &TauriSecretStore,
        &get_user_agent(),
        env!("QUADRANT_OAUTH2_CLIENT_ID"),
        env!("QUADRANT_OAUTH2_CLIENT_SECRET"),
        code,
        redirect_uri,
    )
    .await
    .map_err(tauri::Error::from)?;
    app.emit("recheckAccountToken", "")?;
    Ok(())
}

#[tauri::command]
pub fn oauth2_client_id() -> String {
    env!("QUADRANT_OAUTH2_CLIENT_ID").to_string()
}

#[tauri::command]
pub async fn read_notification(
    notification_id: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    quadrant_core::account::id::read_notification(
        &TauriSecretStore,
        &get_user_agent(),
        notification_id.clone(),
    )
    .await
    .map_err(tauri::Error::from)?;

    if let Some(notifications) = mark_notification_read_and_snapshot(&app, &notification_id).await {
        emit_notification_refresh(&app, notifications)?;
    }

    Ok(())
}

pub fn start_notification_worker(app: AppHandle) {
    tokio::task::spawn(async move {
        loop {
            if let Err(error) = bootstrap_notifications(&app).await {
                log::error!("Notification bootstrap failed: {error}");
            }

            let stream_result = run_notification_socket(app.clone()).await;
            if let Err(error) = stream_result {
                log::warn!("Notification socket cycle ended: {error}");
            }

            if let Err(error) = bootstrap_notifications(&app).await {
                log::warn!("Notification catch-up failed after socket cycle: {error}");
            }

            let delay = {
                let state = app.state::<Mutex<AppState>>();
                let mut state = state.lock().await;
                state.notification_state.reconnect_attempt =
                    state.notification_state.reconnect_attempt.saturating_add(1);
                reconnect_delay(state.notification_state.reconnect_attempt)
            };
            tokio::time::sleep(delay).await;
        }
    });
}

pub fn start_settings_sync_worker(app: AppHandle) {
    tokio::task::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(SETTINGS_SYNC_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if let Err(error) = sync_remote_settings(app.clone()).await {
                log::warn!("Settings sync worker failed: {error}");
            }
        }
    });
}

async fn bootstrap_notifications(app: &AppHandle) -> Result<(), anyhow::Error> {
    let cursor = load_notification_cursor(app)?;
    let (notifications, next_cursor) =
        quadrant_core::account::id::get_notification_history_all_since(
            &TauriSecretStore,
            &get_user_agent(),
            Some(&cursor),
            None,
            true,
        )
        .await?;

    if notifications.is_empty() && next_cursor == cursor {
        return Ok(());
    }

    let merge_outcome = {
        let state = app.state::<Mutex<AppState>>();
        let mut state = state.lock().await;
        let runtime = &mut state.notification_state;
        let merge_outcome = merge_notifications_and_collect_updates(runtime, notifications);
        if runtime.cursor != next_cursor {
            runtime.cursor = next_cursor.clone();
        }
        merge_outcome
    };

    persist_notification_cursor(app, &next_cursor)?;

    if let Some(notifications) = merge_outcome.notifications_for_ui {
        emit_notification_refresh(app, notifications)?;
    }

    for notification in merge_outcome.modpack_sync_notifications {
        match handle_modpack_sync_notification(app, &notification).await {
            Ok(()) => {
                mark_modpack_sync_processed(app, &notification).await;
            }
            Err(error) => {
                log::warn!("Failed to process bootstrapped modpack sync notification: {error}");
            }
        }
    }

    set_reconnect_attempt(app, 0).await;

    Ok(())
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
        let should_queue_modpack_sync = runtime.should_process_modpack_sync_notification(&notification);
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

async fn run_notification_socket(app: AppHandle) -> Result<(), anyhow::Error> {
    let token = account::get_account_token()?;
    let (cursor, connection_id) = {
        let state = app.state::<Mutex<AppState>>();
        let state = state.lock().await;
        (
            state.notification_state.cursor.clone(),
            state.notification_connection_id.clone(),
        )
    };

    let ws_url = build_notification_ws_url(&cursor, &connection_id)?;
    let mut request = ws_url.as_str().into_client_request()?;
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse()?);
    request
        .headers_mut()
        .insert("User-Agent", get_user_agent().parse()?);

    let (mut socket, _) = connect_async(request).await?;
    set_reconnect_attempt(&app, 0).await;

    while let Some(frame) = socket.next().await {
        match frame? {
            Message::Text(payload) => {
                let payload = payload.to_string();
                handle_notification_ws_frame(&app, &payload).await?;
            }
            Message::Close(frame) => {
                let reason = frame
                    .map(|f| f.reason.to_string())
                    .unwrap_or_else(|| "closed".to_string());
                return Err(anyhow!("notification websocket closed: {reason}"));
            }
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
        }
    }

    Err(anyhow!("notification websocket stream ended"))
}

async fn handle_notification_ws_frame(app: &AppHandle, payload: &str) -> Result<(), anyhow::Error> {
    let frame = serde_json::from_str::<NotificationWsFrame>(payload)?;
    match frame {
        NotificationWsFrame::Connected { server_time } => {
            log::info!("Notification websocket connected at {server_time}");
        }
        NotificationWsFrame::Notification {
            delivery,
            notification,
        } => {
            let should_notify = delivery == "live" && !notification.read;
            let notification_id = notification.notification_id.clone();
            let simple_message = notification_simple_message(&notification);
            let is_modpack_sync = is_modpack_sync_notification(&notification);
            let notification_for_processing = notification.clone();

            let notifications_to_emit = {
                let state = app.state::<Mutex<AppState>>();
                let mut state = state.lock().await;
                let outcome = state.notification_state.upsert(notification);
                outcome.changed.then(|| {
                    (
                        state.notification_state.notifications_for_ui(),
                        outcome.inserted,
                    )
                })
            };

            if let Some((notifications, inserted)) = notifications_to_emit {
                emit_notification_refresh(app, notifications)?;
                if should_notify && inserted {
                    maybe_show_native_notification(
                        app,
                        &notification_for_processing,
                        &notification_id,
                        simple_message.as_deref(),
                    )
                    .await?;
                }
            }

            if is_modpack_sync {
                match handle_modpack_sync_notification(app, &notification_for_processing).await {
                    Ok(()) => {
                        mark_modpack_sync_processed(app, &notification_for_processing).await;
                    }
                    Err(error) => {
                        log::warn!("Failed to process modpack sync notification: {error}");
                    }
                }
            }
        }
        NotificationWsFrame::ReplayComplete { truncated, .. } => {
            if truncated {
                bootstrap_notifications(app).await?;
            }
        }
        NotificationWsFrame::Error { error } => {
            if error.to_lowercase().contains("lagged") {
                bootstrap_notifications(app).await?;
            }
            return Err(anyhow!("notification websocket error frame: {error}"));
        }
    }

    Ok(())
}

fn build_notification_ws_url(
    cursor: &NotificationCursor,
    connection_id: &str,
) -> Result<Url, anyhow::Error> {
    let base = std::env::var("QUADRANT_API_BASE_URL")
        .unwrap_or_else(|_| quadrant_core::account::QNT_BASE_URL.to_string());
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
    url.set_path("/api/v3/account/notifications/ws");
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

fn reconnect_delay(attempt: u32) -> Duration {
    let secs = match attempt {
        0 => 1,
        1 => 1,
        2 => 2,
        3 => 5,
        4 => 10,
        5 => 30,
        _ => 60,
    };
    Duration::from_secs(secs)
}

fn emit_notification_refresh(
    app: &AppHandle,
    notifications: Vec<Notification>,
) -> Result<(), anyhow::Error> {
    app.emit("refreshNotifications", notifications)?;
    Ok(())
}

fn emit_synced_modpacks_refresh(app: &AppHandle, modpack_id: &str) -> Result<(), anyhow::Error> {
    app.emit(REFRESH_SYNCED_MODPACKS_EVENT, modpack_id)?;
    Ok(())
}

async fn mark_notification_read_and_snapshot(
    app: &AppHandle,
    notification_id: &str,
) -> Option<Vec<Notification>> {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    state
        .notification_state
        .mark_read(notification_id)
        .then(|| state.notification_state.notifications_for_ui())
}

async fn set_reconnect_attempt(app: &AppHandle, attempt: u32) {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    state.notification_state.reconnect_attempt = attempt;
}

async fn mark_modpack_sync_processed(app: &AppHandle, notification: &Notification) {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    state
        .notification_state
        .mark_modpack_sync_processed(notification);
}

fn load_notification_cursor(app: &AppHandle) -> Result<NotificationCursor, anyhow::Error> {
    let store = app.store("config.json")?;
    Ok(NotificationCursor {
        created_at: store
            .get(NOTIFICATION_CURSOR_CREATED_AT_KEY)
            .and_then(|value| value.as_str().map(ToOwned::to_owned)),
        notification_id: store
            .get(NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY)
            .and_then(|value| value.as_str().map(ToOwned::to_owned)),
    })
}

fn persist_notification_cursor(
    app: &AppHandle,
    cursor: &NotificationCursor,
) -> Result<(), anyhow::Error> {
    let store = app.store("config.json")?;
    match &cursor.created_at {
        Some(created_at) => {
            store.set(
                NOTIFICATION_CURSOR_CREATED_AT_KEY,
                serde_json::Value::String(created_at.clone()),
            );
        }
        None => {
            store.delete(NOTIFICATION_CURSOR_CREATED_AT_KEY);
        }
    };
    match &cursor.notification_id {
        Some(notification_id) => {
            store.set(
                NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY,
                serde_json::Value::String(notification_id.clone()),
            );
        }
        None => {
            store.delete(NOTIFICATION_CURSOR_NOTIFICATION_ID_KEY);
        }
    };
    store.save()?;
    Ok(())
}

async fn maybe_show_native_notification(
    app: &AppHandle,
    notification: &Notification,
    notification_id: &str,
    body: Option<&str>,
) -> Result<(), anyhow::Error> {
    let body = match body {
        Some(body) if !body.is_empty() => body,
        _ => return Ok(()),
    };

    if is_modpack_sync_notification(notification)
        && !should_show_modpack_sync_notification(app, notification).await
    {
        return Ok(());
    }

    let store = app.store("config.json")?;
    let mut shown_notifications = store
        .get(SHOWN_NOTIFICATIONS_KEY)
        .and_then(|value| serde_json::from_value::<Vec<String>>(value).ok())
        .unwrap_or_default();
    if shown_notifications.iter().any(|id| id == notification_id) {
        return Ok(());
    }

    app.notification()
        .builder()
        .title(NOTIFICATION_TITLE)
        .body(body)
        .show()?;

    shown_notifications.push(notification_id.to_string());
    store.set(
        SHOWN_NOTIFICATIONS_KEY,
        serde_json::to_value(shown_notifications)?,
    );
    store.save()?;

    Ok(())
}

fn notification_simple_message(notification: &Notification) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(&notification.message)
        .ok()
        .and_then(|message| {
            message
                .get("simple_message")
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
        })
}

fn notification_updated_by(notification: &Notification) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(&notification.message)
        .ok()
        .and_then(|message| {
            message
                .get("updated_by")
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
        })
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

async fn should_show_modpack_sync_notification(
    app: &AppHandle,
    notification: &Notification,
) -> bool {
    let Ok(config) = app.store("config.json") else {
        return true;
    };
    if !config
        .get("showModpackUpdateNotifications")
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
    {
        return false;
    }

    let Some(updated_by) = notification_updated_by(notification) else {
        return true;
    };

    let updated_by = normalize_identity(updated_by.as_str());
    if updated_by.is_empty() {
        return true;
    }

    match quadrant_core::account::id::get_account_info_with_refresh(
        &TauriSecretStore,
        &get_user_agent(),
        env!("QUADRANT_OAUTH2_CLIENT_ID"),
        env!("QUADRANT_OAUTH2_CLIENT_SECRET"),
    )
    .await
    {
        Ok(account_info) => {
            let identities = [account_info.name, account_info.login];
            !identities
                .iter()
                .map(|identity| normalize_identity(identity.as_str()))
                .any(|identity| !identity.is_empty() && identity == updated_by)
        }
        Err(error) => {
            log::warn!("Failed to resolve current account for notification filtering: {error}");
            true
        }
    }
}

fn normalize_identity(identity: &str) -> String {
    identity.trim().to_lowercase()
}

async fn handle_modpack_sync_notification(
    app: &AppHandle,
    notification: &Notification,
) -> Result<(), anyhow::Error> {
    let synced_modpack = resolve_synced_modpack_from_notification(notification).await?;

    let config = app.store("config.json")?;
    let auto_quadrant_sync = config
        .get("autoQuadrantSync")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if auto_quadrant_sync {
        let local_modpack = resolve_local_modpack_for_sync(app, &synced_modpack).await?;
        let local_modpack =
            maybe_backfill_local_modpack_id(app, local_modpack, &synced_modpack).await?;

        if let Some(local_modpack) = local_modpack {
            maybe_apply_remote_modpack_update(app, &local_modpack, &synced_modpack).await?;
        }
    }

    emit_synced_modpacks_refresh(app, &synced_modpack.modpack_id)?;

    Ok(())
}

async fn resolve_synced_modpack_from_notification(
    notification: &Notification,
) -> Result<SyncedModpack, anyhow::Error> {
    if let Some(payload) = parse_notification_message(notification) {
        return Ok(payload.modpack);
    }

    let resource_id = notification
        .resource_id
        .as_deref()
        .ok_or_else(|| anyhow!("modpack sync notification missing resource_id"))?;
    let mut synced_modpacks = get_synced_modpacks(false, Some(resource_id.to_string())).await?;
    synced_modpacks
        .drain(..)
        .next()
        .ok_or_else(|| anyhow!("modpack sync fallback fetch returned no modpack"))
}

async fn resolve_local_modpack_for_sync(
    app: &AppHandle,
    synced_modpack: &SyncedModpack,
) -> Result<Option<LocalModpack>, anyhow::Error> {
    let modpacks = quadrant_core::modpacks::get_modpacks(&mc_folder(app)?, true)?;

    if let Some(local_modpack) = modpacks
        .iter()
        .find(|modpack| modpack.modpack_id.as_deref() == Some(synced_modpack.modpack_id.as_str()))
        .cloned()
    {
        return Ok(Some(local_modpack));
    }

    Ok(modpacks
        .into_iter()
        .find(|modpack| modpack.last_synced != 0 && modpack.name == synced_modpack.name))
}

async fn maybe_backfill_local_modpack_id(
    app: &AppHandle,
    local_modpack: Option<LocalModpack>,
    synced_modpack: &SyncedModpack,
) -> Result<Option<LocalModpack>, anyhow::Error> {
    let Some(mut local_modpack) = local_modpack else {
        return Ok(None);
    };

    if local_modpack.modpack_id.as_deref() == Some(synced_modpack.modpack_id.as_str()) {
        return Ok(Some(local_modpack));
    }

    let local_sync_time = u64::try_from(local_modpack.last_synced / 1000).unwrap_or_default();
    persist_sync_metadata(
        app,
        &local_modpack.name,
        local_sync_time,
        Some(synced_modpack.modpack_id.as_str()),
    )?;
    local_modpack.modpack_id = Some(synced_modpack.modpack_id.clone());

    Ok(Some(local_modpack))
}

async fn maybe_apply_remote_modpack_update(
    app: &AppHandle,
    local_modpack: &LocalModpack,
    synced_modpack: &SyncedModpack,
) -> Result<(), anyhow::Error> {
    let local_sync_time = local_modpack.last_synced / 1000;
    if synced_modpack.last_synced <= local_sync_time {
        return Ok(());
    }

    if !begin_modpack_update(app, &local_modpack.name).await {
        return Ok(());
    }

    let install_result = install_modpack(
        InstalledModpack {
            mod_loader: synced_modpack.mod_loader,
            name: synced_modpack.name.clone(),
            version: synced_modpack.minecraft_version.clone(),
            mods: serde_json::from_str(&synced_modpack.mods)?,
        },
        app.clone(),
    )
    .await;

    if let Err(error) = install_result {
        finish_modpack_update(app, &local_modpack.name).await;
        return Err(error.into());
    }

    persist_sync_metadata(
        app,
        &local_modpack.name,
        synced_modpack.last_synced as u64,
        Some(synced_modpack.modpack_id.as_str()),
    )?;
    finish_modpack_update(app, &local_modpack.name).await;

    Ok(())
}

async fn begin_modpack_update(app: &AppHandle, modpack_name: &str) -> bool {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
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

async fn finish_modpack_update(app: &AppHandle, modpack_name: &str) {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    state.updated_modpacks.retain(|name| name != modpack_name);
}

async fn sync_remote_settings(app: AppHandle) -> Result<(), anyhow::Error> {
    let config = app.store("config.json")?;
    let auto_settings_sync = config
        .get("syncSettings")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !auto_settings_sync {
        return Ok(());
    }

    let res = get_quadrant_settings(app.clone()).await;
    if let Err(error) = res
        && error.to_string() == "Current settings are newer"
    {
        submit_quadrant_settings(app).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Notification, NotificationCursor, NotificationRuntimeState, build_notification_ws_url,
        merge_notifications_and_collect_updates,
    };

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
    fn merge_notifications_deduplicates_and_orders() {
        let mut state = NotificationRuntimeState::default();
        assert!(merge_notifications_and_collect_updates(
            &mut state,
            vec![notification("n1", 1, false), notification("n2", 2, false)]
        )
        .notifications_for_ui
        .is_some());
        assert!(
            merge_notifications_and_collect_updates(&mut state, vec![notification("n1", 1, false)])
                .notifications_for_ui
                .is_none()
        );

        let notifications = state.notifications_for_ui();
        assert_eq!(notifications[0].notification_id, "n2");
        assert_eq!(notifications[1].notification_id, "n1");
        assert_eq!(
            state.cursor,
            NotificationCursor {
                created_at: Some("2026-03-20T10:02:00Z".to_string()),
                notification_id: Some("n2".to_string()),
            }
        );
    }

    #[test]
    fn mark_notification_read_local_updates_cached_notification() {
        let mut state = NotificationRuntimeState::default();
        let _ = merge_notifications_and_collect_updates(&mut state, vec![notification("n1", 1, false)]);
        assert!(state.mark_read("n1"));
        assert!(state.by_key.get("n1").unwrap().read);
        assert!(!state.mark_read("n1"));
    }

    #[test]
    fn modpack_sync_notifications_replace_previous_resource_entry() {
        let mut state = NotificationRuntimeState::default();
        let first = Notification {
            notification_id: "n1".to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("modpack_sync".to_string()),
            resource_id: Some("modpack-1".to_string()),
            message: "{\"notification_type\":\"modpack_sync\",\"simple_message\":\"hello\"}"
                .to_string(),
            created_at: "2026-03-20T10:01:00Z".to_string(),
            created_at_unix: 1,
            read: false,
        };
        let second = Notification {
            notification_id: "n2".to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("modpack_sync".to_string()),
            resource_id: Some("modpack-1".to_string()),
            message: "{\"notification_type\":\"modpack_sync\",\"simple_message\":\"updated\"}"
                .to_string(),
            created_at: "2026-03-20T10:02:00Z".to_string(),
            created_at_unix: 2,
            read: false,
        };

        assert!(
            merge_notifications_and_collect_updates(&mut state, vec![first])
                .notifications_for_ui
                .is_some()
        );
        assert!(
            merge_notifications_and_collect_updates(&mut state, vec![second])
                .notifications_for_ui
                .is_some()
        );

        let notifications = state.notifications_for_ui();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].notification_id, "n2");
        assert_eq!(
            state.key_by_notification_id.get("n2").map(String::as_str),
            Some("modpack_sync:modpack-1")
        );
    }

    #[test]
    fn notification_websocket_url_includes_modpack_sync_and_connection_id() {
        let cursor = NotificationCursor {
            created_at: Some("2026-03-20T10:02:00Z".to_string()),
            notification_id: Some("n2".to_string()),
        };

        let url = build_notification_ws_url(&cursor, "client-connection-123").unwrap();
        let query = url.query().unwrap_or_default();

        assert!(query.contains("since=2026-03-20T10%3A02%3A00Z"));
        assert!(query.contains("replay_limit=500"));
        assert!(query.contains("modpack_sync=true"));
        assert!(query.contains("connection_id=client-connection-123"));
    }

    #[test]
    fn bootstrap_merge_collects_latest_changed_modpack_sync_per_resource() {
        let mut state = NotificationRuntimeState::default();
        let invite = notification("n0", 0, false);
        let sync_first = Notification {
            notification_id: "n1".to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("modpack_sync".to_string()),
            resource_id: Some("modpack-1".to_string()),
            message: "{\"notification_type\":\"modpack_sync\",\"simple_message\":\"hello\"}"
                .to_string(),
            created_at: "2026-03-20T10:01:00Z".to_string(),
            created_at_unix: 1,
            read: false,
        };
        let sync_second = Notification {
            notification_id: "n2".to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("modpack_sync".to_string()),
            resource_id: Some("modpack-1".to_string()),
            message: "{\"notification_type\":\"modpack_sync\",\"simple_message\":\"updated\"}"
                .to_string(),
            created_at: "2026-03-20T10:02:00Z".to_string(),
            created_at_unix: 2,
            read: false,
        };
        let sync_other = Notification {
            notification_id: "n3".to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("modpack_sync".to_string()),
            resource_id: Some("modpack-2".to_string()),
            message: "{\"notification_type\":\"modpack_sync\",\"simple_message\":\"other\"}"
                .to_string(),
            created_at: "2026-03-20T10:03:00Z".to_string(),
            created_at_unix: 3,
            read: false,
        };

        let outcome = merge_notifications_and_collect_updates(
            &mut state,
            vec![sync_second.clone(), invite, sync_first, sync_other.clone()],
        );

        assert!(outcome.notifications_for_ui.is_some());
        assert_eq!(outcome.modpack_sync_notifications.len(), 2);
        assert_eq!(outcome.modpack_sync_notifications[0].notification_id, "n2");
        assert_eq!(outcome.modpack_sync_notifications[1].notification_id, "n3");
    }

    #[test]
    fn bootstrap_merge_retries_unprocessed_modpack_sync_even_when_unchanged() {
        let mut state = NotificationRuntimeState::default();
        let sync_notification = Notification {
            notification_id: "n1".to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("modpack_sync".to_string()),
            resource_id: Some("modpack-1".to_string()),
            message: "{\"notification_type\":\"modpack_sync\",\"simple_message\":\"updated\"}"
                .to_string(),
            created_at: "2026-03-20T10:02:00Z".to_string(),
            created_at_unix: 2,
            read: false,
        };

        let _ = merge_notifications_and_collect_updates(&mut state, vec![sync_notification.clone()]);
        let outcome = merge_notifications_and_collect_updates(&mut state, vec![sync_notification.clone()]);

        assert!(outcome.notifications_for_ui.is_none());
        assert_eq!(outcome.modpack_sync_notifications.len(), 1);
        assert_eq!(outcome.modpack_sync_notifications[0].notification_id, "n1");
    }

    #[test]
    fn bootstrap_merge_skips_processed_modpack_sync_when_unchanged() {
        let mut state = NotificationRuntimeState::default();
        let sync_notification = Notification {
            notification_id: "n1".to_string(),
            user_id: "u1".to_string(),
            notification_type: Some("modpack_sync".to_string()),
            resource_id: Some("modpack-1".to_string()),
            message: "{\"notification_type\":\"modpack_sync\",\"simple_message\":\"updated\"}"
                .to_string(),
            created_at: "2026-03-20T10:02:00Z".to_string(),
            created_at_unix: 2,
            read: false,
        };

        let _ = merge_notifications_and_collect_updates(&mut state, vec![sync_notification.clone()]);
        state.mark_modpack_sync_processed(&sync_notification);
        let outcome = merge_notifications_and_collect_updates(&mut state, vec![sync_notification]);

        assert!(outcome.notifications_for_ui.is_none());
        assert!(outcome.modpack_sync_notifications.is_empty());
    }
}
