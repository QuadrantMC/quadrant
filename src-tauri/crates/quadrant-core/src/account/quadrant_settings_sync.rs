//! Settings synchronization helpers for Quadrant cloud settings.

use std::collections::HashMap;

use anyhow::anyhow;
use chrono::DateTime;
use serde_json::{Value, json};

use crate::{
    Result,
    account::{backend_base_url, get_account_token},
    ports::{SecretStore, SettingsStore},
};

/// Setting keys that currently participate in cloud settings sync.
pub const SYNCED_KEYS: &[&str] = &[
    "collectUserData",
    "modrinth",
    "curseforge",
    "rssFeeds",
    "silentNews",
    "autoQuadrantSync",
    "showUnupgradeableMods",
    "lastPage",
    "uiScale",
    "experimentalFeatures",
    "cacheKeepAlive",
    "clipIcons",
];

/// Pulls synced settings from the cloud into the local settings store.
pub async fn get_quadrant_settings(
    settings_store: &impl SettingsStore,
    secret_store: &impl SecretStore,
    user_agent: &str,
) -> Result<()> {
    log::debug!("Pulling settings from cloud");
    let token = get_account_token(secret_store)?;
    let json = reqwest::Client::new()
        .get(format!("{}/quadrant/settings_sync/get", backend_base_url()))
        .header("User-Agent", user_agent)
        .bearer_auth(token)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let last_settings_updated = DateTime::parse_from_rfc3339(
        &settings_store
            .get_string("lastSettingsUpdated")?
            .ok_or_else(|| anyhow!("lastSettingsUpdated missing"))?,
    )?;
    let sync_time = DateTime::parse_from_rfc3339(
        json["sync_date"]
            .as_str()
            .ok_or_else(|| anyhow!("sync_date missing"))?,
    )?;

    if last_settings_updated == sync_time {
        log::debug!("Cloud settings are up-to-date, skipping sync");
        return Ok(());
    }
    if last_settings_updated > sync_time {
        return Err(anyhow!("Current settings are newer"));
    }

    log::info!("Applying cloud settings (sync_time={sync_time})");
    if let Some(new_settings) = json["settings"].as_str() {
        let new_settings: Value = serde_json::from_str(new_settings)?;
        for (key, value) in new_settings
            .as_object()
            .ok_or_else(|| anyhow!("No valid settings"))?
        {
            if SYNCED_KEYS.contains(&key.as_str()) {
                settings_store.set_value(key, value.to_owned())?;
            }
        }
        settings_store.set_string("lastSettingsUpdated", sync_time.to_rfc3339())?;
        return Ok(());
    }

    Err(anyhow!("No valid settings"))
}

/// Pushes the current synced settings subset to the Quadrant backend.
pub async fn submit_quadrant_settings(
    settings_store: &impl SettingsStore,
    secret_store: &impl SecretStore,
    user_agent: &str,
) -> Result<()> {
    log::info!("Pushing settings to cloud");
    let token = get_account_token(secret_store)?;
    let entries = settings_store.entries()?;
    let new_sync_date = settings_store
        .get_string("lastSettingsUpdated")?
        .ok_or_else(|| anyhow!("lastSettingsUpdated missing"))?;
    let new_sync_date = DateTime::parse_from_rfc3339(&new_sync_date)?;

    let mut settings_map = HashMap::new();
    for (key, value) in entries {
        if SYNCED_KEYS.contains(&key.as_str()) {
            settings_map.insert(key, value);
        }
    }

    let response = reqwest::Client::new()
        .post(format!(
            "{}/quadrant/settings_sync/submit",
            backend_base_url()
        ))
        .header("User-Agent", user_agent)
        .bearer_auth(token)
        .json(&json!({
            "settings": serde_json::to_string_pretty(&settings_map)?,
            "sync_date": new_sync_date.to_rfc3339(),
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!(response.text().await?));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SYNCED_KEYS, get_quadrant_settings, submit_quadrant_settings};
    use crate::{
        Result,
        ports::{SecretStore, SettingsStore},
    };
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };
    use serde_json::Value;

    struct MemorySettingsStore;

    impl SettingsStore for MemorySettingsStore {
        fn get_value(&self, key: &str) -> Result<Option<Value>> {
            Ok(match key {
                "lastSettingsUpdated" => {
                    Some(Value::String("2024-01-01T00:00:00+00:00".to_string()))
                }
                "modrinth" => Some(Value::Bool(true)),
                _ => None,
            })
        }

        fn set_value(&self, _key: &str, _value: Value) -> Result<()> {
            Ok(())
        }

        fn entries(&self) -> Result<Vec<(String, Value)>> {
            Ok(vec![
                ("modrinth".to_string(), Value::Bool(true)),
                (
                    "hardwareId".to_string(),
                    Value::String("hw-123".to_string()),
                ),
            ])
        }
    }

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

    #[test]
    fn machine_identity_and_usage_counters_are_not_cloud_synced() {
        assert!(!SYNCED_KEYS.contains(&"hardwareId"));
        assert!(!SYNCED_KEYS.contains(&"curseforgeUsage"));
        assert!(!SYNCED_KEYS.contains(&"modrinthUsage"));
    }

    #[tokio::test]
    async fn submit_quadrant_settings_surfaces_auth_failure_body() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/settings_sync/submit")
                .header("authorization", "Bearer token-123");
            then.status(401)
                .header("content-type", "text/plain")
                .body("invalid token");
        });

        let error =
            submit_quadrant_settings(&MemorySettingsStore, &MemorySecretStore, "test-agent")
                .await
                .unwrap_err();
        assert_eq!(error.to_string(), "invalid token");
    }

    #[tokio::test]
    async fn get_quadrant_settings_errors_on_payload_without_sync_date() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path("/quadrant/settings_sync/get")
                .header("authorization", "Bearer token-123");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"settings":"{}"}"#);
        });

        let error = get_quadrant_settings(&MemorySettingsStore, &MemorySecretStore, "test-agent")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("sync_date missing"));
    }
}
