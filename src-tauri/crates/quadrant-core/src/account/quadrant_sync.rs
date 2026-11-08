//! Synced modpack and collaboration APIs.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    Result,
    account::{account_http_client, backend_base_url, send_with_token_refresh},
    error::ErrorCode,
    models::{LocalModpack, ModLoader},
    ports::SecretStore,
};

/// Body the backend answers a sync conflict with when the submitted modpack is
/// older than the stored one.
const CLOUD_SYNC_NEWER_BODY: &str = "Cloud sync is newer";

/// Owner information for a synced modpack.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModpackOwner {
    /// Owner username.
    pub username: String,
    /// Whether the owner has admin rights on the synced modpack.
    pub admin: bool,
}

/// Cloud-backed modpack metadata returned by Quadrant Sync.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SyncedModpack {
    /// Human-readable modpack name.
    pub name: String,
    /// Target Minecraft version.
    pub minecraft_version: String,
    /// Target mod loader.
    pub mod_loader: ModLoader,
    /// Serialized mod list payload.
    pub mods: String,
    /// Owners and collaborators of the modpack.
    pub owners: Vec<ModpackOwner>,
    /// Last sync time in seconds since the Unix epoch.
    pub last_synced: i64,
    /// Stable cloud modpack identifier.
    pub modpack_id: String,
}

/// What the backend recorded for a submitted modpack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSubmission {
    /// Cloud modpack identifier the submission is stored under, when known.
    pub modpack_id: Option<String>,
    /// Sync time the backend recorded, in seconds since the Unix epoch.
    pub last_synced: i64,
}

/// Fetches synced modpacks visible to the current account.
pub async fn get_synced_modpacks(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    show_owners: bool,
    modpack_id: Option<String>,
) -> Result<Vec<SyncedModpack>> {
    log::debug!("Fetching synced modpacks (show_owners={show_owners}, modpack_id={modpack_id:?})");
    let mut query = vec![("show_owners", show_owners.to_string())];
    if let Some(modpack_id) = modpack_id {
        query.push(("modpack_id", modpack_id));
    }
    let response = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            account_http_client()
                .get(format!("{}/quadrant/sync/get", backend_base_url()))
                .query(query.as_slice())
                .bearer_auth(token)
                .header("User-Agent", user_agent)
                .send()
        },
    )
    .await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(response.text().await?));
    }
    Ok(response.json().await?)
}

/// Removes a collaborator from a synced modpack.
pub async fn kick_member(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    modpack_id: String,
    username: String,
) -> Result<()> {
    log::info!("Kicking member {username} from modpack {modpack_id}");
    let body = json!({ "modpack_id": modpack_id, "username": username });
    let res = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            account_http_client()
                .delete(format!("{}/quadrant/sync/kick", backend_base_url()))
                .bearer_auth(token)
                .json(&body)
                .header("User-Agent", user_agent)
                .send()
        },
    )
    .await?;
    if res.status() != 200 {
        return Err(anyhow::anyhow!(res.text().await?));
    }
    Ok(())
}

/// Invites a collaborator to a synced modpack.
pub async fn invite_member(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    modpack_id: String,
    username: String,
    admin: bool,
) -> Result<()> {
    log::info!("Inviting member {username} to modpack {modpack_id} (admin={admin})");
    let body = json!({ "modpack_id": modpack_id, "username": username, "admin": admin });
    let res = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            account_http_client()
                .post(format!("{}/quadrant/sync/invite", backend_base_url()))
                .bearer_auth(token)
                .json(&body)
                .header("User-Agent", user_agent)
                .send()
        },
    )
    .await?;
    if res.status() != 200 {
        return Err(anyhow::anyhow!(res.text().await?));
    }
    Ok(())
}

/// Permanently deletes a synced modpack from the cloud.
pub async fn delete_synced_modpack(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    modpack_id: String,
) -> Result<()> {
    log::info!("Deleting synced modpack {modpack_id}");
    let body = json!({ "modpack_id": modpack_id });
    let res = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            account_http_client()
                .delete(format!("{}/quadrant/sync/delete", backend_base_url()))
                .bearer_auth(token)
                .json(&body)
                .header("User-Agent", user_agent)
                .send()
        },
    )
    .await?;
    if res.status() != 200 {
        return Err(anyhow::anyhow!(res.text().await?));
    }
    Ok(())
}

/// Uploads local modpack state to Quadrant Sync and returns what the backend recorded.
///
/// The submitted `last_synced` is the pack's stored sync time, not the current
/// clock: the backend compares it against the cloud copy to detect a conflict.
pub async fn sync_modpack(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    modpack: LocalModpack,
    overwrite: bool,
    connection_id: Option<&str>,
) -> Result<SyncSubmission> {
    log::info!(
        "Syncing modpack \"{}\" (overwrite={overwrite})",
        modpack.name
    );
    let mut body = json!({
        "name": modpack.name,
        "mc_version": modpack.version,
        "mod_loader": modpack.mod_loader.to_string(),
        "overwrite": overwrite,
        "mods": serde_json::to_string(&modpack.mods)?,
        "last_synced": modpack.last_synced / 1000,
    });
    if let Some(modpack_id) = modpack.modpack_id.as_deref() {
        body["modpack_id"] = json!(modpack_id);
    }
    let connection_id = connection_id.filter(|value| !value.is_empty());
    let res = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            let mut request = account_http_client()
                .post(format!("{}/quadrant/sync/submit", backend_base_url()))
                .bearer_auth(token)
                .json(&body)
                .header("User-Agent", user_agent);
            if let Some(connection_id) = connection_id {
                request = request.header("X-Quadrant-Connection-Id", connection_id);
            }
            request.send()
        },
    )
    .await?;
    if !res.status().is_success() {
        let body = res.text().await?;
        if body.trim() == CLOUD_SYNC_NEWER_BODY {
            return Err(ErrorCode::CloudSyncNewer.into());
        }
        return Err(anyhow::anyhow!(body));
    }

    // The submission succeeded even if a server answers without the stored
    // modpack; falling back keeps that sync from being reported as a failure.
    Ok(match res.json::<SyncedModpack>().await {
        Ok(synced) => SyncSubmission {
            modpack_id: Some(synced.modpack_id),
            last_synced: synced.last_synced,
        },
        Err(error) => {
            log::warn!("Sync submit response was not a synced modpack: {error}");
            SyncSubmission {
                modpack_id: modpack.modpack_id,
                last_synced: Utc::now().timestamp(),
            }
        }
    })
}

/// Accepts or declines an invitation to a synced modpack.
pub async fn answer_invite(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    modpack_id: String,
    answer: bool,
) -> Result<()> {
    log::info!("Answering invite for modpack {modpack_id}: accepted={answer}");
    let body = json!({ "modpack_id": modpack_id, "accept": answer });
    let response = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            account_http_client()
                .post(format!("{}/quadrant/sync/respond", backend_base_url()))
                .bearer_auth(token)
                .json(&body)
                .header("User-Agent", user_agent)
                .send()
        },
    )
    .await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(response.text().await?));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SyncSubmission, get_synced_modpacks, sync_modpack};
    use crate::{
        Result,
        error::is_cloud_sync_conflict,
        models::{InstalledMod, LocalModpack, ModLoader, ModSource},
        ports::SecretStore,
    };
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

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

    /// Secret store that persists writes, so a token refresh performed mid-request
    /// is observable on the retry.
    struct StatefulSecretStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl StatefulSecretStore {
        fn with_tokens(access: &str, refresh: &str) -> Self {
            let mut values = HashMap::new();
            values.insert("accountToken".to_string(), access.to_string());
            values.insert("refreshToken".to_string(), refresh.to_string());
            Self {
                values: Mutex::new(values),
            }
        }
    }

    impl SecretStore for StatefulSecretStore {
        fn get_secret(&self, key: &str) -> Result<Option<String>> {
            Ok(self.values.lock().unwrap().get(key).cloned())
        }

        fn set_secret(&self, key: &str, value: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete_secret(&self, key: &str) -> Result<()> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn local_modpack() -> LocalModpack {
        LocalModpack {
            name: "Better Create".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: vec![InstalledMod::minimal(
                "abc".to_string(),
                ModSource::Modrinth,
                "https://example.com/mod.jar".to_string(),
            )],
            unknown_mods: false,
            is_applied: false,
            last_synced: 0,
            modpack_id: Some("modpack-1".to_string()),
            manifest_error: None,
        }
    }

    /// The 200 body the backend answers a submit with: the modpack as stored.
    fn synced_modpack_body(modpack_id: &str, last_synced: i64) -> serde_json::Value {
        json!({
            "name": "Better Create",
            "minecraft_version": "1.20.1",
            "mod_loader": "Fabric",
            "mods": "[]",
            "owners": [],
            "last_synced": last_synced,
            "modpack_id": modpack_id,
        })
    }

    async fn submit(modpack: LocalModpack) -> Result<SyncSubmission> {
        sync_modpack(
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            modpack,
            false,
            None,
        )
        .await
    }

    #[tokio::test]
    async fn sync_modpack_sends_quadrant_connection_header_when_present() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/sync/submit")
                .header("authorization", "Bearer token-123")
                .header("x-quadrant-connection-id", "client-connection-123");
            then.status(200).body("ok");
        });

        sync_modpack(
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            local_modpack(),
            true,
            Some("client-connection-123"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sync_modpack_refreshes_token_on_unauthorized_and_retries() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        // First attempt with the stale token is rejected.
        let _unauthorized = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/sync/submit")
                .header("authorization", "Bearer stale-token");
            then.status(401).body("token expired");
        });
        // The refresh exchange mints a new access token.
        let _refresh = server.mock(|when, then| {
            when.method(POST).path("/oauth2/token");
            then.status(200).json_body_obj(&serde_json::json!({
                "access_token": "fresh-token",
                "token_type": "Bearer",
                "expires_in": 3600,
                "refresh_token": "fresh-refresh",
                "scope": "profile:read sync:read notifications:read"
            }));
        });
        // The retry with the refreshed token succeeds.
        let _retry = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/sync/submit")
                .header("authorization", "Bearer fresh-token");
            then.status(200).body("ok");
        });

        let store = StatefulSecretStore::with_tokens("stale-token", "stale-refresh");
        sync_modpack(
            &store,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            local_modpack(),
            true,
            None,
        )
        .await
        .unwrap();

        // The refreshed token was persisted for subsequent requests.
        assert_eq!(
            store.get_secret("accountToken").unwrap().unwrap(),
            "fresh-token"
        );
    }

    #[tokio::test]
    async fn sync_modpack_surfaces_auth_failure_body() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        // 403 (not 401) so the response body surfaces directly without the
        // account token refresh path kicking in.
        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/sync/submit")
                .header("authorization", "Bearer token-123");
            then.status(403)
                .header("content-type", "text/plain")
                .body("invalid token");
        });

        let error = sync_modpack(
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            local_modpack(),
            false,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "invalid token");
    }

    #[tokio::test]
    async fn sync_modpack_submits_the_stored_sync_time_and_a_compact_mod_list() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let mut modpack = local_modpack();
        modpack.last_synced = 1_700_000_000_000;
        let mods = serde_json::to_string(&modpack.mods).unwrap();
        assert!(!mods.contains('\n'), "mod list must be compact JSON");

        // An exact body match: the backend compares the submitted seconds
        // against its own copy, and a pretty mod list would not match either.
        let request = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/sync/submit")
                .json_body(json!({
                    "name": "Better Create",
                    "mc_version": "1.20.1",
                    "mod_loader": "Fabric",
                    "overwrite": false,
                    "mods": mods,
                    "last_synced": 1_700_000_000_i64,
                    "modpack_id": "modpack-1",
                }));
            then.status(200)
                .json_body(synced_modpack_body("modpack-1", 1_700_000_500));
        });

        let submission = submit(modpack).await;
        request.assert();
        submission.unwrap();
    }

    #[tokio::test]
    async fn sync_modpack_omits_modpack_id_when_the_pack_has_none() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let mut modpack = local_modpack();
        modpack.modpack_id = None;
        let mods = serde_json::to_string(&modpack.mods).unwrap();

        let request = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/sync/submit")
                .json_body(json!({
                    "name": "Better Create",
                    "mc_version": "1.20.1",
                    "mod_loader": "Fabric",
                    "overwrite": false,
                    "mods": mods,
                    "last_synced": 0_i64,
                }));
            then.status(200)
                .json_body(synced_modpack_body("assigned-by-server", 42));
        });

        let submission = submit(modpack).await;
        request.assert();
        assert_eq!(
            submission.unwrap(),
            SyncSubmission {
                modpack_id: Some("assigned-by-server".to_string()),
                last_synced: 42,
            }
        );
    }

    #[tokio::test]
    async fn sync_modpack_reports_what_the_server_recorded() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/quadrant/sync/submit");
            then.status(200)
                .json_body(synced_modpack_body("cloud-9", 1_800_000_000));
        });

        // The server's id and clock win over the local pack's.
        assert_eq!(
            submit(local_modpack()).await.unwrap(),
            SyncSubmission {
                modpack_id: Some("cloud-9".to_string()),
                last_synced: 1_800_000_000,
            }
        );
    }

    #[tokio::test]
    async fn sync_modpack_falls_back_to_the_local_id_when_the_body_is_not_a_modpack() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/quadrant/sync/submit");
            then.status(200).body("ok");
        });

        let submission = submit(local_modpack()).await.unwrap();
        assert_eq!(submission.modpack_id.as_deref(), Some("modpack-1"));
        assert!(submission.last_synced > 0);
    }

    #[tokio::test]
    async fn sync_modpack_classifies_a_cloud_sync_conflict() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/quadrant/sync/submit");
            then.status(400)
                .header("content-type", "text/plain")
                .body("Cloud sync is newer");
        });

        let error = submit(local_modpack()).await.unwrap_err();
        assert!(is_cloud_sync_conflict(&error));
        assert_eq!(error.to_string(), "errorCloudSyncNewer");
    }

    #[tokio::test]
    async fn sync_modpack_leaves_other_rejections_unclassified() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/quadrant/sync/submit");
            then.status(400)
                .header("content-type", "text/plain")
                .body("modpack name is too long");
        });

        let error = submit(local_modpack()).await.unwrap_err();
        assert!(!is_cloud_sync_conflict(&error));
        assert_eq!(error.to_string(), "modpack name is too long");
    }

    #[tokio::test]
    async fn get_synced_modpacks_surfaces_a_failure_body() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/quadrant/sync/get");
            then.status(403)
                .header("content-type", "text/plain")
                .body("invalid token");
        });

        let error = get_synced_modpacks(
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            true,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "invalid token");
    }

    #[tokio::test]
    async fn get_synced_modpacks_errors_on_malformed_payload() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path("/quadrant/sync/get")
                .query_param("show_owners", "true")
                .header("authorization", "Bearer token-123");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"unexpected":"shape"}"#);
        });

        get_synced_modpacks(
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            true,
            None,
        )
        .await
        .unwrap_err();
    }
}
