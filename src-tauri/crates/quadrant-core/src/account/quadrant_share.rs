//! Quadrant Share submission and retrieval APIs.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{
    Result,
    account::backend_base_url,
    models::InstalledModpack,
    ports::{SecretStore, SettingsStore},
};

/// Response returned after sharing a modpack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuadrantShareSubmissionResponse {
    /// Share code returned by the backend.
    pub code: i32,
    /// Remaining number of allowed uses for the share submission.
    pub uses_left: i64,
}

/// Payload submitted when sharing a modpack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuadrantShareSubmission {
    /// Stable installation identifier of the submitting host.
    pub hardware_id: String,
    /// Serialized modpack manifest.
    pub mod_config: String,
}

/// Response returned when resolving a share code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuadrantShareResponse {
    /// Share code returned by the backend.
    pub code: i32,
    /// Serialized modpack manifest.
    pub mod_config: String,
}

const EMPTY_SHARE_SUBMISSION_BODY: &str = "Quadrant Share submission returned an empty body";
const EMPTY_SHARE_GET_BODY: &str = "Quadrant Share retrieval returned an empty body";

async fn parse_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
    error_prefix: &str,
    empty_body_error: &str,
) -> Result<T> {
    let status = response.status();
    let response_raw = response.text().await?;
    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "{} with {}: {}",
            error_prefix,
            status,
            response_preview(&response_raw)
        ));
    }

    let trimmed = response_raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("{empty_body_error}"));
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

    trimmed.chars().take(300).collect()
}

/// Submits a modpack to Quadrant Share using the current settings and account state.
pub async fn share_modpack_raw(
    settings_store: &impl SettingsStore,
    secret_store: &impl SecretStore,
    user_agent: &str,
    mod_config: InstalledModpack,
    api_key: &str,
) -> Result<QuadrantShareSubmissionResponse> {
    log::info!("Sharing modpack: {}", mod_config.name);
    let data_collection_enabled = settings_store.get_bool("collectUserData")?.unwrap_or(false);
    if !data_collection_enabled {
        log::info!("Modpack share aborted: data collection is disabled");
        return Err(anyhow::anyhow!("enableDataSharing"));
    }

    let token = secret_store.get_secret("accountToken")?;
    let mut url = format!("{}/quadrant/share/submit", backend_base_url());
    if token.is_some() {
        url = format!("{}/id", url);
    }

    let mut request = reqwest::Client::new()
        .post(&url)
        .header("User-Agent", user_agent)
        .json(&QuadrantShareSubmission {
            hardware_id: settings_store.get_string("hardwareId")?.unwrap_or_default(),
            mod_config: serde_json::to_string_pretty(&mod_config)?,
        });

    if let Some(token) = token {
        request = request.bearer_auth(token);
    } else {
        request = request.header("Authorization", api_key);
    }

    parse_json_response(
        request.send().await?,
        "Quadrant Share submission failed",
        EMPTY_SHARE_SUBMISSION_BODY,
    )
    .await
}

/// Resolves a share code into an installed modpack manifest.
pub async fn get_quadrant_share_modpack(
    user_agent: &str,
    api_key: &str,
    code: String,
) -> Result<InstalledModpack> {
    log::info!("Fetching shared modpack with code: {code}");
    let response: QuadrantShareResponse = parse_json_response(
        reqwest::Client::new()
            .get(format!("{}/quadrant/share/get", backend_base_url()))
            .query(&[("code", code)])
            .header("User-Agent", user_agent)
            .header("Authorization", api_key)
            .send()
            .await?,
        "Quadrant Share retrieval failed",
        EMPTY_SHARE_GET_BODY,
    )
    .await?;

    serde_json::from_str(&response.mod_config).map_err(|error| {
        anyhow::anyhow!(
            "Quadrant Share retrieval failed: invalid modpack payload: {}; body preview: {}",
            error,
            response_preview(&response.mod_config)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{get_quadrant_share_modpack, share_modpack_raw};
    use crate::{
        Result,
        models::{InstalledMod, InstalledModpack, ModLoader, ModSource},
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
                "collectUserData" => Some(Value::Bool(true)),
                "hardwareId" => Some(Value::String("hw-123".to_string())),
                _ => None,
            })
        }

        fn set_value(&self, _key: &str, _value: Value) -> Result<()> {
            Ok(())
        }

        fn entries(&self) -> Result<Vec<(String, Value)>> {
            Ok(vec![])
        }
    }

    struct EmptySecretStore;

    impl SecretStore for EmptySecretStore {
        fn get_secret(&self, _key: &str) -> Result<Option<String>> {
            Ok(None)
        }

        fn set_secret(&self, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }

        fn delete_secret(&self, _key: &str) -> Result<()> {
            Ok(())
        }
    }

    fn installed_modpack() -> InstalledModpack {
        InstalledModpack {
            mod_config_version: String::new(),
            quadrant_version: String::new(),
            name: "Test Pack".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: vec![InstalledMod::minimal(
                "mod-1".to_string(),
                ModSource::Modrinth,
                "https://example.com/mod.jar".to_string(),
            )],
        }
    }

    #[tokio::test]
    async fn share_modpack_raw_sends_json_and_surfaces_http_body() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST)
                .path("/quadrant/share/submit")
                .header("authorization", "test-api-key")
                .header("content-type", "application/json");
            then.status(500)
                .header("content-type", "text/plain")
                .body("share backend exploded");
        });

        let error = share_modpack_raw(
            &MemorySettingsStore,
            &EmptySecretStore,
            "test-agent",
            installed_modpack(),
            "test-api-key",
        )
        .await
        .unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("Quadrant Share submission failed with 500 Internal Server Error")
        );
        assert!(message.contains("share backend exploded"));
    }

    #[tokio::test]
    async fn get_quadrant_share_modpack_surfaces_invalid_success_payload() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path("/quadrant/share/get")
                .query_param("code", "abc123")
                .header("authorization", "test-api-key");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"code":123,"mod_config":"not-json"}"#);
        });

        let error = get_quadrant_share_modpack("test-agent", "test-api-key", "abc123".to_string())
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("Quadrant Share retrieval failed: invalid modpack payload"));
        assert!(message.contains("not-json"));
    }

    #[tokio::test]
    async fn share_modpack_raw_surfaces_invalid_json_success_body() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/quadrant/share/submit");
            then.status(200)
                .header("content-type", "application/json")
                .body("{definitely-not-json");
        });

        let error = share_modpack_raw(
            &MemorySettingsStore,
            &EmptySecretStore,
            "test-agent",
            installed_modpack(),
            "test-api-key",
        )
        .await
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("Quadrant Share submission failed: invalid JSON response"));
        assert!(message.contains("{definitely-not-json"));
    }

    #[tokio::test]
    async fn get_quadrant_share_modpack_surfaces_auth_failure() {
        let _guard = crate::account::ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        let server = MockServer::start();
        unsafe {
            std::env::set_var("QUADRANT_API_BASE_URL", server.base_url());
        }

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path("/quadrant/share/get")
                .query_param("code", "abc123");
            then.status(401)
                .header("content-type", "text/plain")
                .body("invalid api key");
        });

        let error = get_quadrant_share_modpack("test-agent", "test-api-key", "abc123".to_string())
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("Quadrant Share retrieval failed with 401 Unauthorized"));
        assert!(message.contains("invalid api key"));
    }
}
