//! Synced modpack and collaboration APIs.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    Result,
    account::{backend_base_url, send_with_token_refresh},
    models::{LocalModpack, ModLoader},
    ports::SecretStore,
};

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
            reqwest::Client::new()
                .get(format!("{}/quadrant/sync/get", backend_base_url()))
                .query(query.as_slice())
                .bearer_auth(token)
                .header("User-Agent", user_agent)
                .send()
        },
    )
    .await?;
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
            reqwest::Client::new()
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
            reqwest::Client::new()
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
            reqwest::Client::new()
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

/// Uploads local modpack state to Quadrant Sync and returns the sync timestamp used.
pub async fn sync_modpack(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    modpack: LocalModpack,
    overwrite: bool,
    connection_id: Option<&str>,
) -> Result<i64> {
    log::info!(
        "Syncing modpack \"{}\" (overwrite={overwrite})",
        modpack.name
    );
    let timestamp = Utc::now().timestamp();
    let body = json!({
        "name": modpack.name,
        "mc_version": modpack.version,
        "mod_loader": modpack.mod_loader.to_string(),
        "overwrite": overwrite,
        "mods": serde_json::to_string_pretty(&modpack.mods)?,
        "last_synced": &timestamp,
    });
    let connection_id = connection_id.filter(|value| !value.is_empty());
    let res = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            let mut request = reqwest::Client::new()
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
    if res.status() != 200 {
        return Err(anyhow::anyhow!(res.text().await?));
    }
    Ok(timestamp)
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
            reqwest::Client::new()
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
    use super::{get_synced_modpacks, sync_modpack};
    use crate::{
        Result,
        models::{InstalledMod, LocalModpack, ModLoader, ModSource},
        ports::SecretStore,
    };
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };
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
        }
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
