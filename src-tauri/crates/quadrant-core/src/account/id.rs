//! Account identity, login, refresh, and notification APIs.

use std::{collections::HashMap, env};

use reqwest::StatusCode;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    Result,
    account::{QNT_BASE_URL, get_account_token, get_refresh_token, set_secret},
    ports::SecretStore,
};

use super::quadrant_sync::SyncedModpack;

const DEFAULT_NOTIFICATION_HISTORY_LIMIT: usize = 500;
const DEFAULT_NOTIFICATION_HISTORY_SINCE: &str = "1970-01-01T00:00:00Z";
const EMPTY_NOTIFICATION_HISTORY_BODY: &str = "Notification history request returned an empty body";
const EMPTY_ACCOUNT_INFO_BODY: &str = "Account info request returned an empty body";
const EMPTY_OAUTH_BODY: &str = "OAuth2 token endpoint returned an empty body";

fn backend_base_url() -> String {
    env::var("QUADRANT_API_BASE_URL").unwrap_or_else(|_| QNT_BASE_URL.to_string())
}

/// Account profile returned by the Quadrant backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    /// Stable account identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Email address associated with the account.
    pub email: String,
    /// Maximum synced modpack quota for the account.
    pub quadrant_sync_limit: i32,
    /// Maximum share quota for the account.
    pub quadrant_share_limit: i32,
    /// Login or username.
    pub login: String,
    /// Current notification list.
    pub notifications: Vec<Notification>,
}

/// Account notification returned by the Quadrant backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Notification {
    /// Stable notification identifier.
    pub notification_id: String,
    /// Owning user identifier.
    pub user_id: String,
    /// Backend notification category when available.
    #[serde(default)]
    pub notification_type: Option<String>,
    /// Stable resource identifier when available.
    #[serde(default)]
    pub resource_id: Option<String>,
    /// Human-readable notification message as a JSON string.
    pub message: String,
    /// RFC3339 creation timestamp for new APIs, or a normalized string from legacy payloads.
    #[serde(deserialize_with = "deserialize_notification_created_at")]
    pub created_at: String,
    /// Creation timestamp in seconds since the Unix epoch.
    #[serde(default, deserialize_with = "deserialize_notification_created_at_unix")]
    pub created_at_unix: i64,
    /// Whether the notification has been marked as read.
    pub read: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NotificationCreatedAt {
    String(String),
    Integer(i64),
}

fn deserialize_notification_created_at<'de, D>(
    deserializer: D,
) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let created_at = NotificationCreatedAt::deserialize(deserializer)?;
    Ok(match created_at {
        NotificationCreatedAt::String(value) => value,
        NotificationCreatedAt::Integer(value) => value.to_string(),
    })
}

fn deserialize_notification_created_at_unix<'de, D>(
    deserializer: D,
) -> std::result::Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Integer(i64),
        String(String),
        Null,
    }

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Integer(value) => Ok(value),
        Value::String(value) => Ok(value.parse::<i64>().unwrap_or_default()),
        Value::Null => Ok(0),
    }
}

fn notification_history_query(
    cursor: Option<&NotificationCursor>,
    read: Option<bool>,
    limit: Option<usize>,
    include_modpack_sync: bool,
) -> Vec<(String, String)> {
    let cursor = cursor.cloned().unwrap_or_default();
    let limit = limit
        .unwrap_or(DEFAULT_NOTIFICATION_HISTORY_LIMIT)
        .min(DEFAULT_NOTIFICATION_HISTORY_LIMIT);
    let since = cursor
        .created_at
        .unwrap_or_else(|| DEFAULT_NOTIFICATION_HISTORY_SINCE.to_string());

    let mut query = vec![
        ("since".to_string(), since),
        ("limit".to_string(), limit.to_string()),
    ];
    if let Some(after_id) = cursor.notification_id {
        query.push(("after_id".to_string(), after_id));
    }
    if let Some(read) = read {
        query.push(("read".to_string(), read.to_string()));
    }
    if include_modpack_sync {
        query.push(("modpack_sync".to_string(), "true".to_string()));
    }
    query
}

fn notification_cursor_from_last(
    notifications: &[Notification],
    fallback: NotificationCursor,
) -> NotificationCursor {
    notifications.last().map_or(fallback, |last| NotificationCursor {
        created_at: Some(last.created_at.clone()),
        notification_id: Some(last.notification_id.clone()),
    })
}

/// Paginated notification history response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationHistoryResponse {
    pub notifications: Vec<Notification>,
    pub has_more: bool,
    pub next_since: Option<String>,
    pub next_after_id: Option<String>,
}

/// Persisted notification cursor.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct NotificationCursor {
    pub created_at: Option<String>,
    pub notification_id: Option<String>,
}

/// WebSocket frame emitted by the notification backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum NotificationWsFrame {
    Connected {
        server_time: String,
    },
    Notification {
        delivery: String,
        notification: Notification,
    },
    ReplayComplete {
        count: usize,
        truncated: bool,
    },
    Error {
        error: String,
    },
}

/// OAuth token response returned by the Quadrant backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Response {
    /// Access token used for authenticated requests.
    pub access_token: String,
    /// Token type, typically `Bearer`.
    pub token_type: String,
    /// Access token lifetime in seconds.
    pub expires_in: i64,
    /// Optional refresh token.
    pub refresh_token: Option<String>,
    /// Granted OAuth scopes.
    pub scope: String,
}

/// Description of a cloud modpack update pending local application.
#[derive(Debug, Clone)]
pub struct PendingModpackUpdate {
    /// Remote synced modpack metadata.
    pub synced_modpack: SyncedModpack,
    /// Matching local modpack name.
    pub local_name: String,
    /// Matching local modpack version.
    pub local_version: String,
    /// Local sync time in seconds since the Unix epoch.
    pub local_sync_time: i64,
}

/// Uses the refresh token to obtain and persist a fresh access token.
pub async fn try_refresh_token(
    secret_store: &impl SecretStore,
    client_id: &str,
    client_secret: &str,
    user_agent: &str,
) -> Result<()> {
    log::info!("Attempting token refresh");
    let refresh_token = get_refresh_token(secret_store)?;
    let mut body = HashMap::new();
    body.insert("client_id", client_id);
    body.insert("client_secret", client_secret);
    body.insert("grant_type", "refresh_token");
    body.insert("refresh_token", refresh_token.as_str());

    let response = reqwest::Client::new()
        .post(format!("{}/oauth2/token", backend_base_url()))
        .form(&body)
        .header("User-Agent", user_agent)
        .send()
        .await?;

    let res = parse_oauth_token_response(response, "Token refresh failed").await?;
    set_secret(secret_store, "accountToken", &res.access_token)?;
    if let Some(new_refresh_token) = res.refresh_token {
        set_secret(secret_store, "refreshToken", &new_refresh_token)?;
    }
    log::info!("Token refresh successful");
    Ok(())
}

/// Fetches the current account profile using the stored access token.
pub async fn get_account_info(
    secret_store: &impl SecretStore,
    user_agent: &str,
) -> Result<AccountInfo> {
    log::info!("Fetching account info");
    let token = get_account_token(secret_store)?;
    let client = reqwest::Client::new();
    let url = format!("{}/account/info/get", backend_base_url());

    let response = client
        .get(&url)
        .header("User-Agent", user_agent)
        .bearer_auth(&token)
        .send()
        .await?;

    parse_account_info_response(response).await
}

/// Fetches account info and attempts a token refresh if the request is unauthorized.
pub async fn get_account_info_with_refresh(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<AccountInfo> {
    let token = get_account_token(secret_store)?;
    let client = reqwest::Client::new();
    let url = format!("{}/account/info/get", backend_base_url());

    let response = client
        .get(&url)
        .header("User-Agent", user_agent)
        .bearer_auth(&token)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        log::info!("Account info request unauthorized, attempting token refresh");
        try_refresh_token(secret_store, client_id, client_secret, user_agent).await?;
        let new_token = get_account_token(secret_store)?;
        let retry = client
            .get(&url)
            .header("User-Agent", user_agent)
            .bearer_auth(&new_token)
            .send()
            .await?;
        return parse_account_info_response(retry).await;
    }

    parse_account_info_response(response).await
}

/// Completes the OAuth authorization code flow and persists returned tokens.
pub async fn oauth2_login(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    code: String,
    redirect_uri: String,
) -> Result<()> {
    log::info!("Starting OAuth2 login flow");
    let mut body = HashMap::new();
    body.insert("client_id", client_id);
    body.insert("client_secret", client_secret);
    body.insert("grant_type", "authorization_code");
    body.insert("code", code.as_str());
    body.insert("redirect_uri", redirect_uri.as_str());

    let response = reqwest::Client::new()
        .post(format!("{}/oauth2/token", backend_base_url()))
        .form(&body)
        .header("User-Agent", user_agent)
        .send()
        .await?;

    let res = parse_oauth_token_response(response, "OAuth2 login failed").await?;
    if !res.scope.contains("profile:read")
        || !res.scope.contains("sync:read")
        || !res.scope.contains("notifications:read")
    {
        return Err(anyhow::anyhow!("Invalid scope"));
    }
    set_secret(secret_store, "accountToken", &res.access_token)?;
    if let Some(refresh_token) = res.refresh_token {
        set_secret(secret_store, "refreshToken", &refresh_token)?;
    }
    log::info!("OAuth2 login successful");
    Ok(())
}

/// Fetches a single page of notification history from the Quadrant backend.
pub async fn get_notification_history_page(
    secret_store: &impl SecretStore,
    user_agent: &str,
    cursor: Option<&NotificationCursor>,
    read: Option<bool>,
    limit: Option<usize>,
    include_modpack_sync: bool,
) -> Result<NotificationHistoryResponse> {
    let token = get_account_token(secret_store)?;
    let query = notification_history_query(cursor, read, limit, include_modpack_sync);

    let response = reqwest::Client::new()
        .get(format!("{}/account/notifications/get", backend_base_url()))
        .header("User-Agent", user_agent)
        .bearer_auth(&token)
        .query(&query)
        .send()
        .await?;

    if response.status() == StatusCode::UNAUTHORIZED {
        log::info!("Notification history request unauthorized, attempting token refresh");
        try_refresh_token(
            secret_store,
            env!("QUADRANT_OAUTH2_CLIENT_ID"),
            env!("QUADRANT_OAUTH2_CLIENT_SECRET"),
            user_agent,
        )
        .await?;
        let new_token = get_account_token(secret_store)?;
        let retry = reqwest::Client::new()
            .get(format!("{}/account/notifications/get", backend_base_url()))
            .header("User-Agent", user_agent)
            .bearer_auth(new_token)
            .query(&query)
            .send()
            .await?;
        return parse_notification_history_response(retry).await;
    }

    parse_notification_history_response(response).await
}

/// Fetches all notification history pages from the provided cursor onward.
pub async fn get_notification_history_all_since(
    secret_store: &impl SecretStore,
    user_agent: &str,
    cursor: Option<&NotificationCursor>,
    read: Option<bool>,
    include_modpack_sync: bool,
) -> Result<(Vec<Notification>, NotificationCursor)> {
    let mut page_cursor = cursor.cloned().unwrap_or_default();
    let mut notifications = Vec::new();

    loop {
        let page = get_notification_history_page(
            secret_store,
            user_agent,
            Some(&page_cursor),
            read,
            Some(DEFAULT_NOTIFICATION_HISTORY_LIMIT),
            include_modpack_sync,
        )
        .await?;

        page_cursor = notification_cursor_from_last(&page.notifications, page_cursor);
        notifications.extend(page.notifications);

        if !page.has_more {
            break;
        }

        page_cursor.created_at = page.next_since.or(page_cursor.created_at);
        page_cursor.notification_id = page.next_after_id.or(page_cursor.notification_id);
    }

    Ok((notifications, page_cursor))
}

async fn parse_notification_history_response(
    response: reqwest::Response,
) -> Result<NotificationHistoryResponse> {
    parse_json_response(response, "Notification history request failed", EMPTY_NOTIFICATION_HISTORY_BODY).await
}

async fn parse_account_info_response(response: reqwest::Response) -> Result<AccountInfo> {
    parse_json_response(response, "Account info request failed", EMPTY_ACCOUNT_INFO_BODY).await
}

async fn parse_oauth_token_response(
    response: reqwest::Response,
    error_prefix: &str,
) -> Result<OAuth2Response> {
    parse_json_response(response, error_prefix, EMPTY_OAUTH_BODY).await
}

async fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    error_prefix: &str,
    empty_body_error: &str,
) -> Result<T> {
    let status = response.status();
    let response_raw = response.text().await?;
    if !status.is_success() {
        let body_preview = response_preview(&response_raw);
        return Err(anyhow::anyhow!(
            "{} with {}: {}",
            error_prefix,
            status,
            body_preview
        ));
    }

    let trimmed = response_raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("{}", empty_body_error));
    }

    serde_json::from_str(trimmed).map_err(|error| {
        anyhow::anyhow!(
            "{}: invalid JSON response: {}; body preview: {}",
            error_prefix,
            error,
            response_preview(trimmed)
        )
    })
}

fn response_preview(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "<empty body>".to_string();
    }
    let preview = if trimmed.len() > 300 {
        &trimmed[..300]
    } else {
        trimmed
    };
    preview.to_string()
}

/// Marks a notification as read in the Quadrant backend.
pub async fn read_notification(
    secret_store: &impl SecretStore,
    user_agent: &str,
    notification_id: String,
) -> Result<()> {
    log::info!("Marking notification {notification_id} as read");
    let token = get_account_token(secret_store)?;
    let request = reqwest::Client::new()
        .post(format!("{}/account/notifications/read", backend_base_url()))
        .header("User-Agent", user_agent)
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "notification_id": notification_id,
        }))
        .send()
        .await?;

    if request.status() != 200 {
        let body = request.text().await?;
        return Err(anyhow::anyhow!(body));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Notification, NotificationCursor, NotificationWsFrame, get_notification_history_all_since,
        get_notification_history_page,
    };
    use crate::{Result, ports::SecretStore};
    use httpmock::{Method::GET, MockServer};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct MemorySecretStore;

    impl SecretStore for MemorySecretStore {
        fn get_secret(&self, key: &str) -> Result<Option<String>> {
            match key {
                "accountToken" => Ok(Some("token-123".to_string())),
                _ => Ok(None),
            }
        }

        fn set_secret(&self, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }

        fn delete_secret(&self, _key: &str) -> Result<()> {
            Ok(())
        }
    }

    fn notification(id: &str, ts: &str, unix: i64) -> Notification {
        Notification {
            notification_id: id.to_string(),
            user_id: "user-1".to_string(),
            notification_type: Some("invite_to_sync".to_string()),
            resource_id: None,
            message: "{\"simple_message\":\"hi\"}".to_string(),
            created_at: ts.to_string(),
            created_at_unix: unix,
            read: false,
        }
    }

    #[tokio::test]
    async fn notification_history_page_uses_cursor_query() {
        let _guard = ENV_LOCK.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path("/account/notifications/get")
                .header("authorization", "Bearer token-123")
                .query_param("since", "2026-03-20T00:00:00Z")
                .query_param("after_id", "cursor-1")
                .query_param("limit", "500")
                .query_param("modpack_sync", "true");
            then.status(200).json_body_obj(&serde_json::json!({
                "notifications": [],
                "has_more": false,
                "next_since": null,
                "next_after_id": null
            }));
        });

        let response = get_notification_history_page(
            &MemorySecretStore,
            "test-agent",
            Some(&NotificationCursor {
                created_at: Some("2026-03-20T00:00:00Z".to_string()),
                notification_id: Some("cursor-1".to_string()),
            }),
            None,
            Some(999),
            true,
        )
        .await
        .unwrap();

        assert!(response.notifications.is_empty());
    }

    #[tokio::test]
    async fn notification_history_all_since_follows_pagination() {
        let _guard = ENV_LOCK.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _page_one = server.mock(|when, then| {
            when.method(GET)
                .path("/account/notifications/get")
                .query_param("since", "2026-03-20T00:00:00Z")
                .query_param("limit", "500")
                .query_param("modpack_sync", "true");
            then.status(200).json_body_obj(&serde_json::json!({
                "notifications": [notification("n1", "2026-03-20T10:00:00Z", 1)],
                "has_more": true,
                "next_since": "2026-03-20T10:00:00Z",
                "next_after_id": "n1"
            }));
        });

        let _page_two = server.mock(|when, then| {
            when.method(GET)
                .path("/account/notifications/get")
                .query_param("since", "2026-03-20T10:00:00Z")
                .query_param("after_id", "n1")
                .query_param("limit", "500")
                .query_param("modpack_sync", "true");
            then.status(200).json_body_obj(&serde_json::json!({
                "notifications": [notification("n2", "2026-03-20T10:15:30.123Z", 2)],
                "has_more": false,
                "next_since": "2026-03-20T10:15:30.123Z",
                "next_after_id": "n2"
            }));
        });

        let (notifications, cursor) = get_notification_history_all_since(
            &MemorySecretStore,
            "test-agent",
            Some(&NotificationCursor {
                created_at: Some("2026-03-20T00:00:00Z".to_string()),
                notification_id: None,
            }),
            None,
            true,
        )
        .await
        .unwrap();

        assert_eq!(notifications.len(), 2);
        assert_eq!(
            cursor.created_at.as_deref(),
            Some("2026-03-20T10:15:30.123Z")
        );
        assert_eq!(cursor.notification_id.as_deref(), Some("n2"));
    }

    #[test]
    fn websocket_frames_deserialize() {
        let connected: NotificationWsFrame = serde_json::from_str(
            r#"{"event":"connected","server_time":"2026-03-20T10:20:00.000Z"}"#,
        )
        .unwrap();
        assert!(matches!(connected, NotificationWsFrame::Connected { .. }));

        let replay: NotificationWsFrame = serde_json::from_str(
            r#"{"event":"notification","delivery":"replay","notification":{"notification_id":"n1","user_id":"u1","message":"{\"simple_message\":\"x\"}","created_at":"2026-03-20T10:15:30.123Z","created_at_unix":1774001730,"read":false}}"#,
        )
        .unwrap();
        assert!(matches!(replay, NotificationWsFrame::Notification { .. }));

        let replay_complete: NotificationWsFrame =
            serde_json::from_str(r#"{"event":"replay_complete","count":100,"truncated":true}"#)
                .unwrap();
        assert!(matches!(
            replay_complete,
            NotificationWsFrame::ReplayComplete {
                truncated: true,
                ..
            }
        ));

        let error: NotificationWsFrame =
            serde_json::from_str(r#"{"event":"error","error":"lagged"}"#).unwrap();
        assert!(matches!(error, NotificationWsFrame::Error { .. }));
    }
}
