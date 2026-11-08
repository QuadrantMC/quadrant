//! Telemetry payload construction and submission helpers.

use chrono::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Result, account::backend_base_url, mc_mod::http::provider_http_client, ports::SettingsStore,
};

/// Telemetry payload submitted to the Quadrant backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    /// Current host application version.
    pub version: String,
    /// Current operating system label supplied by the host.
    pub os: String,
    /// Number of Modrinth-backed actions performed.
    pub modrinth_usage: i64,
    /// Number of CurseForge-backed actions performed.
    pub curseforge_usage: i64,
    /// Reference file usage counter.
    pub reference_file_usage: i64,
    /// Manual input usage counter.
    pub manual_input_usage: i64,
    /// Stable installation identifier.
    pub hardware_id: String,
    /// Submission timestamp.
    pub date: DateTime<Utc>,
    /// Always empty: the client no longer looks up its own IP. The API fills it
    /// in from the edge when it can.
    pub country: String,
}

/// Collects the current telemetry snapshot from settings and the host.
pub async fn get_telemetry_info(
    settings_store: &impl SettingsStore,
    version: String,
    os: String,
) -> Result<AppInfo> {
    let hardware_id = settings_store.get_string("hardwareId")?.unwrap_or_default();
    let date_time = Utc::now();

    Ok(AppInfo {
        version,
        os,
        modrinth_usage: settings_store.get_i64("modrinthUsage")?.unwrap_or(0),
        curseforge_usage: settings_store.get_i64("curseforgeUsage")?.unwrap_or(0),
        reference_file_usage: 0,
        manual_input_usage: 0,
        hardware_id,
        date: date_time,
        country: String::new(),
    })
}

/// Sends telemetry data when collection is enabled in settings.
pub async fn send_telemetry(
    settings_store: &impl SettingsStore,
    user_agent: &str,
    version: String,
    os: String,
    api_key: &str,
) -> Result<()> {
    if !settings_store.get_bool("collectUserData")?.unwrap_or(false) {
        log::info!("Telemetry skipped: data collection is disabled");
        return Ok(());
    }

    log::info!("Sending telemetry for version {version} on {os}");
    let info = get_telemetry_info(settings_store, version, os).await?;
    provider_http_client()
        .post(format!("{}/quadrant/usage/submit", backend_base_url()))
        .json(&info)
        .header("Authorization", api_key)
        .header("User-Agent", user_agent)
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

/// Deletes telemetry data for the current installation.
pub async fn remove_telemetry(
    settings_store: &impl SettingsStore,
    user_agent: &str,
    api_key: &str,
) -> Result<()> {
    log::info!("Removing telemetry data");
    let hardware_id = settings_store.get_string("hardwareId")?.unwrap_or_default();

    provider_http_client()
        .delete(format!("{}/quadrant/usage/delete", backend_base_url()))
        .header("Authorization", api_key)
        .header("User-Agent", user_agent)
        .query(&[("hardware_id", hardware_id)])
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    struct Settings;

    impl SettingsStore for Settings {
        fn get_value(&self, key: &str) -> Result<Option<Value>> {
            Ok(match key {
                "hardwareId" => Some(Value::String("hw-1".to_string())),
                "modrinthUsage" => Some(Value::from(3)),
                _ => None,
            })
        }

        fn set_value(&self, _key: &str, _value: Value) -> Result<()> {
            Ok(())
        }

        fn entries(&self) -> Result<Vec<(String, Value)>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn snapshot_leaves_the_country_for_the_server() {
        let info = get_telemetry_info(&Settings, "1.0.0".into(), "linux".into())
            .await
            .unwrap();
        assert_eq!(info.country, "");
        assert_eq!(info.hardware_id, "hw-1");
        assert_eq!(info.modrinth_usage, 3);
    }

    #[test]
    fn the_user_agent_passes_the_usage_gate() {
        // Quadrant ID takes usage only from a User-Agent naming one of its sites.
        assert!(crate::mc_mod::get_user_agent().contains("usequadrant.dev"));
    }
}
