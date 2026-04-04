//! Synced modpack and collaboration APIs.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

use crate::{
    Result,
    account::{QNT_BASE_URL, get_account_token},
    models::{LocalModpack, ModLoader},
    ports::SecretStore,
};

fn backend_base_url() -> String {
    env::var("QUADRANT_API_BASE_URL").unwrap_or_else(|_| QNT_BASE_URL.to_string())
}

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
    show_owners: bool,
    modpack_id: Option<String>,
) -> Result<Vec<SyncedModpack>> {
    log::debug!("Fetching synced modpacks (show_owners={show_owners}, modpack_id={modpack_id:?})");
    let mut query = vec![("show_owners", show_owners.to_string())];
    if let Some(modpack_id) = modpack_id {
        query.push(("modpack_id", modpack_id));
    }
    let response = reqwest::Client::new()
        .get(format!("{}/quadrant/sync/get", backend_base_url()))
        .query(query.as_slice())
        .bearer_auth(get_account_token(secret_store)?)
        .header("User-Agent", user_agent)
        .send()
        .await?;
    Ok(response.json().await?)
}

/// Removes a collaborator from a synced modpack.
pub async fn kick_member(
    secret_store: &impl SecretStore,
    user_agent: &str,
    modpack_id: String,
    username: String,
) -> Result<()> {
    log::info!("Kicking member {username} from modpack {modpack_id}");
    let res = reqwest::Client::new()
        .delete(format!("{}/quadrant/sync/kick", backend_base_url()))
        .bearer_auth(get_account_token(secret_store)?)
        .json(&json!({ "modpack_id": modpack_id, "username": username }))
        .header("User-Agent", user_agent)
        .send()
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
    modpack_id: String,
    username: String,
    admin: bool,
) -> Result<()> {
    log::info!("Inviting member {username} to modpack {modpack_id} (admin={admin})");
    let res = reqwest::Client::new()
        .post(format!("{}/quadrant/sync/invite", backend_base_url()))
        .bearer_auth(get_account_token(secret_store)?)
        .json(&json!({ "modpack_id": modpack_id, "username": username, "admin": admin }))
        .header("User-Agent", user_agent)
        .send()
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
    modpack_id: String,
) -> Result<()> {
    log::info!("Deleting synced modpack {modpack_id}");
    let res = reqwest::Client::new()
        .delete(format!("{}/quadrant/sync/delete", backend_base_url()))
        .bearer_auth(get_account_token(secret_store)?)
        .json(&json!({ "modpack_id": modpack_id }))
        .header("User-Agent", user_agent)
        .send()
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
    modpack: LocalModpack,
    overwrite: bool,
    connection_id: Option<&str>,
) -> Result<i64> {
    log::info!(
        "Syncing modpack \"{}\" (overwrite={overwrite})",
        modpack.name
    );
    let timestamp = Utc::now().timestamp();
    let mut request = reqwest::Client::new()
        .post(format!("{}/quadrant/sync/submit", backend_base_url()))
        .bearer_auth(get_account_token(secret_store)?)
        .json(&json!({
            "name": modpack.name,
            "mc_version": modpack.version,
            "mod_loader": modpack.mod_loader.to_string(),
            "overwrite": overwrite,
            "mods": serde_json::to_string_pretty(&modpack.mods)?,
            "last_synced": &timestamp,
        }))
        .header("User-Agent", user_agent);
    if let Some(connection_id) = connection_id.filter(|value| !value.is_empty()) {
        request = request.header("X-Quadrant-Connection-Id", connection_id);
    }
    let res = request.send().await?;
    if res.status() != 200 {
        return Err(anyhow::anyhow!(res.text().await?));
    }
    Ok(timestamp)
}

/// Accepts or declines an invitation to a synced modpack.
pub async fn answer_invite(
    secret_store: &impl SecretStore,
    user_agent: &str,
    modpack_id: String,
    answer: bool,
) -> Result<()> {
    log::info!("Answering invite for modpack {modpack_id}: accepted={answer}");
    let response = reqwest::Client::new()
        .post(format!("{}/quadrant/sync/respond", backend_base_url()))
        .bearer_auth(get_account_token(secret_store)?)
        .json(&json!({ "modpack_id": modpack_id, "accept": answer }))
        .header("User-Agent", user_agent)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(response.text().await?));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sync_modpack;
    use crate::{
        Result,
        models::{InstalledMod, LocalModpack, ModLoader, ModSource},
        ports::SecretStore,
    };
    use httpmock::{Method::POST, MockServer};
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

    fn local_modpack() -> LocalModpack {
        LocalModpack {
            name: "Better Create".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: vec![InstalledMod {
                id: "abc".to_string(),
                source: ModSource::Modrinth,
                download_url: "https://example.com/mod.jar".to_string(),
            }],
            unknown_mods: false,
            is_applied: false,
            last_synced: 0,
            modpack_id: Some("modpack-1".to_string()),
        }
    }

    #[tokio::test]
    async fn sync_modpack_sends_quadrant_connection_header_when_present() {
        let _guard = ENV_LOCK.lock().unwrap();
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
            local_modpack(),
            true,
            Some("client-connection-123"),
        )
        .await
        .unwrap();
    }
}
