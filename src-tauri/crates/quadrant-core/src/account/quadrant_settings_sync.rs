//! Settings synchronization helpers for Quadrant cloud settings.

use std::collections::HashMap;

use anyhow::anyhow;
use chrono::DateTime;
use serde_json::{Value, json};

use crate::{
    Result,
    account::{account_http_client, backend_base_url, send_with_token_refresh},
    events::BackendEvent,
    ports::{EventSink, SecretStore, SettingsStore},
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
    "lastPageName",
    "uiScale",
    "experimentalFeatures",
    "cacheKeepAlive",
    "clipIcons",
    // Quadrant-Native's color theme. The desktop app keeps the key without
    // reading it, so a theme picked on one machine follows the account.
    "colorTheme",
];

/// Outcome of a cloud settings pull.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPull {
    /// The cloud copy carries the same sync date as the local settings.
    UpToDate,
    /// The cloud copy was newer and has been written to the settings store.
    Applied,
    /// The local settings are newer than the cloud copy.
    LocalNewer,
    /// The account has no cloud settings yet.
    NoRemote,
}

/// Pulls synced settings from the cloud into the local settings store.
pub async fn get_quadrant_settings(
    settings_store: &impl SettingsStore,
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    event_sink: &impl EventSink,
) -> Result<SettingsPull> {
    log::debug!("Pulling settings from cloud");
    let response = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            account_http_client()
                .get(format!("{}/quadrant/settings_sync/get", backend_base_url()))
                .header("User-Agent", user_agent)
                .bearer_auth(token)
                .send()
        },
    )
    .await?;

    // A fresh account has never pushed settings and answers 404 with a plain
    // text body, so the decode has to be gated behind the status.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        log::debug!("Account has no cloud settings yet");
        return Ok(SettingsPull::NoRemote);
    }
    if !response.status().is_success() {
        return Err(anyhow!(response.text().await?));
    }
    let json = response.json::<serde_json::Value>().await?;

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
        return Ok(SettingsPull::UpToDate);
    }
    if last_settings_updated > sync_time {
        return Ok(SettingsPull::LocalNewer);
    }

    log::info!("Applying cloud settings (sync_time={sync_time})");
    if let Some(new_settings) = json["settings"].as_str() {
        let new_settings: Value = serde_json::from_str(new_settings)?;
        for (key, value) in new_settings
            .as_object()
            .ok_or_else(|| anyhow!("No valid settings"))?
        {
            if SYNCED_KEYS.contains(&key.as_str()) {
                if !is_acceptable(settings_store, key, value) {
                    log::warn!("Ignoring the cloud value of {key}: it isn't a valid setting");
                    continue;
                }
                settings_store.set_value(key, value.to_owned())?;
                // Frontend subscribers only learn about changes through this
                // event; without it a synced UI scale or theme sits unapplied
                // until the next restart.
                event_sink.publish(BackendEvent::ConfigChanged(key.clone()))?;
            }
        }
        settings_store.set_string("lastSettingsUpdated", sync_time.to_rfc3339())?;
        event_sink.publish(BackendEvent::ConfigChanged(
            "lastSettingsUpdated".to_string(),
        ))?;
        return Ok(SettingsPull::Applied);
    }

    Err(anyhow!("No valid settings"))
}

/// Whether a cloud value may overwrite the local one: the type the readers
/// expect, inside the range they handle. A copy that was tampered with, or
/// written by a client with other ideas, can't break them or move a limit.
/// Data collection is only ever switched off by a pull, never on, because
/// only the person at this machine can agree to it.
fn is_acceptable(settings_store: &impl SettingsStore, key: &str, value: &Value) -> bool {
    let in_range = |min: f64, max: f64| {
        value
            .as_f64()
            .is_some_and(|number| (min..=max).contains(&number))
    };
    match key {
        "collectUserData" => match value.as_bool() {
            Some(true) => settings_store
                .get_bool("collectUserData")
                .is_ok_and(|local| local == Some(true)),
            Some(false) => true,
            None => false,
        },
        "modrinth"
        | "curseforge"
        | "rssFeeds"
        | "silentNews"
        | "autoQuadrantSync"
        | "showUnupgradeableMods"
        | "experimentalFeatures"
        | "clipIcons" => value.is_boolean(),
        "lastPage" => value.is_i64() && in_range(0.0, 1000.0),
        "lastPageName" | "colorTheme" => value.as_str().is_some_and(|text| text.len() <= 256),
        "uiScale" => value.is_number() && in_range(50.0, 200.0),
        "cacheKeepAlive" => value.is_i64() && in_range(1.0, 3650.0),
        _ => false,
    }
}

/// Pushes the current synced settings subset to the Quadrant backend.
pub async fn submit_quadrant_settings(
    settings_store: &impl SettingsStore,
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<()> {
    log::info!("Pushing settings to cloud");
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

    let body = json!({
        "settings": serde_json::to_string_pretty(&settings_map)?,
        "sync_date": new_sync_date.to_rfc3339(),
    });
    let response = send_with_token_refresh(
        secret_store,
        user_agent,
        client_id,
        client_secret,
        |token| {
            account_http_client()
                .post(format!(
                    "{}/quadrant/settings_sync/submit",
                    backend_base_url()
                ))
                .header("User-Agent", user_agent)
                .bearer_auth(token)
                .json(&body)
                .send()
        },
    )
    .await?;
    if !response.status().is_success() {
        return Err(anyhow!(response.text().await?));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SYNCED_KEYS, SettingsPull, get_quadrant_settings, submit_quadrant_settings};
    use crate::{
        Result,
        events::BackendEvent,
        ports::{EventSink, SecretStore, SettingsStore},
    };
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };
    use serde_json::Value;

    struct NoopEvents;

    impl EventSink for NoopEvents {
        fn publish(&self, _event: BackendEvent) -> Result<()> {
            Ok(())
        }
    }

    const CLOUD_SYNC_DATE: &str = "2025-01-01T00:00:00+00:00";

    struct MemorySettingsStore {
        last_settings_updated: &'static str,
    }

    impl MemorySettingsStore {
        fn updated_at(last_settings_updated: &'static str) -> Self {
            Self {
                last_settings_updated,
            }
        }
    }

    impl SettingsStore for MemorySettingsStore {
        fn get_value(&self, key: &str) -> Result<Option<Value>> {
            Ok(match key {
                "lastSettingsUpdated" => {
                    Some(Value::String(self.last_settings_updated.to_string()))
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

    #[test]
    fn the_native_color_theme_is_cloud_synced() {
        assert!(SYNCED_KEYS.contains(&"colorTheme"));
    }

    #[tokio::test]
    async fn submit_quadrant_settings_surfaces_auth_failure_body() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        // 403 (not 401) so the response body surfaces directly without the
        // account token refresh path kicking in.
        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/settings_sync/submit")
                .header("authorization", "Bearer token-123");
            then.status(403)
                .header("content-type", "text/plain")
                .body("invalid token");
        });

        let error = submit_quadrant_settings(
            &MemorySettingsStore::updated_at("2024-01-01T00:00:00+00:00"),
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
        )
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

        let error = get_quadrant_settings(
            &MemorySettingsStore::updated_at("2024-01-01T00:00:00+00:00"),
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            &NoopEvents,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("sync_date missing"));
    }

    #[tokio::test]
    async fn get_quadrant_settings_notifies_each_applied_key() {
        struct CollectingEvents(std::sync::Mutex<Vec<BackendEvent>>);

        impl EventSink for CollectingEvents {
            fn publish(&self, event: BackendEvent) -> Result<()> {
                self.0.lock().unwrap().push(event);
                Ok(())
            }
        }

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
                .body(
                    r#"{"sync_date":"2025-01-01T00:00:00+00:00","settings":"{\"uiScale\":110,\"hardwareId\":\"other-machine\"}"}"#,
                );
        });

        let events = CollectingEvents(Default::default());
        let outcome = get_quadrant_settings(
            &MemorySettingsStore::updated_at("2024-01-01T00:00:00+00:00"),
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            &events,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SettingsPull::Applied);

        let keys: Vec<String> = events
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|event| match event {
                BackendEvent::ConfigChanged(key) => key.clone(),
                other => panic!("unexpected event {other:?}"),
            })
            .collect();
        assert_eq!(keys, ["uiScale", "lastSettingsUpdated"]);
    }

    #[test]
    fn cloud_values_must_fit_what_the_readers_expect() {
        use super::is_acceptable;
        use serde_json::json;

        let store = MemorySettingsStore::updated_at("2024-01-01T00:00:00+00:00");
        for (key, value) in [
            ("modrinth", json!(false)),
            ("uiScale", json!(100)),
            ("uiScale", json!(150.0)),
            ("cacheKeepAlive", json!(30)),
            ("lastPage", json!(2)),
            ("lastPageName", json!("search")),
            ("colorTheme", json!("custom:mine")),
            // Switching collection off is always fine.
            ("collectUserData", json!(false)),
        ] {
            assert!(is_acceptable(&store, key, &value), "{key} {value}");
        }
        for (key, value) in [
            ("modrinth", json!("yes")),
            ("clipIcons", json!(1)),
            ("uiScale", json!(10)),
            ("uiScale", json!(1e9)),
            ("uiScale", json!("100")),
            ("cacheKeepAlive", json!(0)),
            ("cacheKeepAlive", json!(-5)),
            ("cacheKeepAlive", json!(1.5)),
            ("lastPage", json!("1")),
            ("lastPageName", json!("x".repeat(257))),
            ("colorTheme", json!(null)),
            ("collectUserData", json!("true")),
            // Only this machine's user turns collection on.
            ("collectUserData", json!(true)),
            ("hardwareId", json!("x")),
        ] {
            assert!(!is_acceptable(&store, key, &value), "{key} {value}");
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn get_quadrant_settings_skips_invalid_values_and_still_applies_the_rest() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }
        let _mock = mock_settings_get(
            &server,
            200,
            r#"{"sync_date":"2025-01-01T00:00:00+00:00","settings":"{\"uiScale\":9000,\"collectUserData\":true,\"clipIcons\":false}"}"#,
        );

        let store = RecordingSettingsStore::default();
        let outcome = get_quadrant_settings(
            &store,
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            &NoopEvents,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SettingsPull::Applied);
        let written = store.0.lock().unwrap();
        assert_eq!(written.get("clipIcons"), Some(&Value::Bool(false)));
        assert!(!written.contains_key("uiScale"));
        assert!(!written.contains_key("collectUserData"));
    }

    #[derive(Default)]
    struct RecordingSettingsStore(std::sync::Mutex<std::collections::HashMap<String, Value>>);

    impl SettingsStore for RecordingSettingsStore {
        fn get_value(&self, key: &str) -> Result<Option<Value>> {
            if key == "lastSettingsUpdated" {
                return Ok(Some(Value::String("2024-01-01T00:00:00+00:00".to_string())));
            }
            Ok(self.0.lock().unwrap().get(key).cloned())
        }

        fn set_value(&self, key: &str, value: Value) -> Result<()> {
            self.0.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }

        fn entries(&self) -> Result<Vec<(String, Value)>> {
            Ok(Vec::new())
        }
    }

    async fn pull(last_settings_updated: &'static str) -> Result<SettingsPull> {
        get_quadrant_settings(
            &MemorySettingsStore::updated_at(last_settings_updated),
            &MemorySecretStore,
            "test-agent",
            "test-client-id",
            "test-client-secret",
            &NoopEvents,
        )
        .await
    }

    /// Serves the cloud settings the pull tests compare against.
    fn mock_settings_get<'a>(
        server: &'a MockServer,
        status: u16,
        body: &str,
    ) -> httpmock::Mock<'a> {
        server.mock(|when, then| {
            when.method(GET)
                .path("/quadrant/settings_sync/get")
                .header("authorization", "Bearer token-123");
            then.status(status)
                .header("content-type", "text/plain")
                .body(body);
        })
    }

    #[tokio::test]
    async fn get_quadrant_settings_reports_no_remote_for_an_account_without_cloud_settings() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }
        // A plain-text 404 is what a never-synced account gets; decoding it as
        // JSON used to fail and stop the account from ever bootstrapping.
        let _mock = mock_settings_get(&server, 404, "No settings found");

        assert_eq!(
            pull("2024-01-01T00:00:00+00:00").await.unwrap(),
            SettingsPull::NoRemote
        );
    }

    #[tokio::test]
    async fn get_quadrant_settings_reports_local_newer_without_applying() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }
        let _mock = mock_settings_get(
            &server,
            200,
            &format!(r#"{{"sync_date":"{CLOUD_SYNC_DATE}","settings":"{{}}"}}"#),
        );

        assert_eq!(
            pull("2026-01-01T00:00:00+00:00").await.unwrap(),
            SettingsPull::LocalNewer
        );
    }

    #[tokio::test]
    async fn get_quadrant_settings_reports_up_to_date_on_matching_sync_dates() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }
        let _mock = mock_settings_get(
            &server,
            200,
            &format!(r#"{{"sync_date":"{CLOUD_SYNC_DATE}","settings":"{{}}"}}"#),
        );

        assert_eq!(pull(CLOUD_SYNC_DATE).await.unwrap(), SettingsPull::UpToDate);
    }

    #[tokio::test]
    async fn get_quadrant_settings_surfaces_a_server_failure_body() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }
        let _mock = mock_settings_get(&server, 500, "settings store unavailable");

        let error = pull("2024-01-01T00:00:00+00:00").await.unwrap_err();
        assert_eq!(error.to_string(), "settings store unavailable");
    }
}
