//! Account token helpers and Quadrant cloud entrypoints.

use std::env;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;

use crate::{Result, ports::SecretStore};

pub mod id;
pub mod pkce;
pub mod quadrant_settings_sync;
pub mod quadrant_share;
pub mod quadrant_sync;

#[cfg(test)]
pub(crate) static ACCOUNT_ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Returns the default backend base URL.
/// The compile-time default can be overridden by setting the `QUADRANT_API_BASE_URL`
/// environment variable at build time. Runtime overrides (via env var or CLI) take
/// precedence over this default.
const DEFAULT_BACKEND_BASE_URL: &str = "https://api.usequadrant.dev/api/v3";

fn qnt_base_url() -> &'static str {
    match option_env!("QUADRANT_API_BASE_URL").map(str::trim) {
        Some(value) if !value.is_empty() => value,
        _ => DEFAULT_BACKEND_BASE_URL,
    }
}
/// Stable keyring service name used by the current app.
pub const KEYRING_SERVICE: &str = "dev.mrquantumoff.mcmodpackmanager";

/// Secret key storing the unix timestamp (seconds) at which the current access
/// token should be proactively refreshed by the background worker.
const TOKEN_REFRESH_AT_KEY: &str = "accountTokenRefreshAt";
/// Fraction of the access token lifetime kept as a safety margin: the token is
/// refreshed once `lifetime - lifetime / N` seconds have elapsed.
const TOKEN_REFRESH_MARGIN_DIVISOR: i64 = 4;
/// Lower bound on the refresh margin, so very short-lived tokens still refresh
/// ahead of expiry instead of exactly at it.
const TOKEN_REFRESH_MIN_MARGIN_SECS: i64 = 60;

/// Current unix time in seconds, or 0 if the clock is before the unix epoch.
fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Set by the host from `--api-url`. A lock instead of the process
/// environment, which isn't safe to write while other threads read it.
static BACKEND_BASE_URL_OVERRIDE: RwLock<Option<String>> = RwLock::new(None);

/// Whether tokens may be sent to `value`: HTTPS anywhere, plain HTTP only to
/// this machine, so a development API still works.
pub fn is_allowed_backend_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }),
        _ => false,
    }
}

/// Sets, or with `None` clears, the backend base URL the host was started
/// with. Fails for a URL that isn't allowed by [`is_allowed_backend_url`].
pub fn set_backend_base_url_override(value: Option<&str>) -> Result<()> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    if let Some(value) = value
        && !is_allowed_backend_url(value)
    {
        return Err(anyhow::anyhow!(
            "the API URL {value:?} must use https (http is only allowed for localhost)"
        ));
    }
    *BACKEND_BASE_URL_OVERRIDE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = value.map(str::to_string);
    Ok(())
}

/// Resolves the backend base URL.
/// Precedence: the host's `--api-url` > `QUADRANT_API_BASE_URL` env var at
/// runtime > `QUADRANT_API_BASE_URL` env var at compile time > hardcoded
/// default. A value that isn't allowed by [`is_allowed_backend_url`] is
/// skipped, so tokens never go out over plain HTTP to another machine.
pub fn backend_base_url() -> String {
    let overridden = BACKEND_BASE_URL_OVERRIDE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let from_env = env::var("QUADRANT_API_BASE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    [overridden, from_env, Some(qnt_base_url().to_string())]
        .into_iter()
        .flatten()
        .find(|value| {
            let allowed = is_allowed_backend_url(value);
            if !allowed {
                log::error!("Ignoring the API URL {value:?}: it must use https");
            }
            allowed
        })
        .unwrap_or_else(|| DEFAULT_BACKEND_BASE_URL.to_string())
}

/// Carries no default `User-Agent`: every account call site sets its own from
/// the host-supplied string.
static ACCOUNT_HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    crate::mc_mod::http::shared_client_builder()
        .build()
        .expect("failed to build shared account HTTP client")
});

pub(crate) fn account_http_client() -> &'static reqwest::Client {
    &ACCOUNT_HTTP_CLIENT
}

/// Persists a named secret in the host secret store.
pub fn set_secret(secret_store: &impl SecretStore, key: &str, value: &str) -> Result<()> {
    log::info!("Setting secret: {key}");
    secret_store.set_secret(key, value)
}

/// Reads the current account access token.
pub fn get_account_token(secret_store: &impl SecretStore) -> Result<String> {
    secret_store
        .get_secret("accountToken")?
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::SignedOut))
}

/// Reads the current account refresh token.
pub fn get_refresh_token(secret_store: &impl SecretStore) -> Result<String> {
    secret_store
        .get_secret("refreshToken")?
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::SignedOut))
}

/// Removes any persisted account and refresh tokens.
pub fn clear_account_token(secret_store: &impl SecretStore) -> Result<()> {
    log::info!("Clearing account and refresh tokens");
    let mut first_error = None;
    for key in ["accountToken", "refreshToken", TOKEN_REFRESH_AT_KEY] {
        if let Err(error) = secret_store.delete_secret(key) {
            // Still remove the remaining credentials when one deletion fails.
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}

/// Records when the current access token should be proactively refreshed,
/// derived from its `expires_in` lifetime (in seconds). The deadline leaves a
/// margin ahead of expiry so a refresh completes before any request would fail.
pub fn set_token_refresh_deadline(secret_store: &impl SecretStore, expires_in: i64) -> Result<()> {
    let lifetime = expires_in.max(0);
    let margin = (lifetime / TOKEN_REFRESH_MARGIN_DIVISOR).max(TOKEN_REFRESH_MIN_MARGIN_SECS);
    let refresh_at = now_unix_secs() + (lifetime - margin).max(0);
    set_secret(secret_store, TOKEN_REFRESH_AT_KEY, &refresh_at.to_string())
}

/// Reads the proactive-refresh deadline (unix seconds), if one is recorded.
pub fn get_token_refresh_deadline(secret_store: &impl SecretStore) -> Result<Option<i64>> {
    Ok(secret_store
        .get_secret(TOKEN_REFRESH_AT_KEY)?
        .and_then(|value| value.parse::<i64>().ok()))
}

/// Whether the access token is due for a proactive refresh.
///
/// Returns `true` when the stored deadline has passed, or when a refresh token
/// exists without a recorded deadline (e.g. a token acquired before proactive
/// refresh tracking existed) so a single refresh re-establishes the deadline.
pub fn account_token_needs_refresh(secret_store: &impl SecretStore) -> Result<bool> {
    if get_refresh_token(secret_store).is_err() {
        return Ok(false);
    }
    match get_token_refresh_deadline(secret_store)? {
        Some(refresh_at) => Ok(now_unix_secs() >= refresh_at),
        None => Ok(true),
    }
}

/// Sends an authenticated request, refreshing the access token and retrying once
/// if the server rejects the first attempt with `401 Unauthorized`.
///
/// `send` is handed the current access token and must build and send a fresh
/// request each time; it may be invoked twice (before and after a refresh). This
/// is the reactive complement to the proactive background refresh: even if the
/// stored token is stale, a token-dependent action refreshes inline instead of
/// surfacing the failure to the user.
pub async fn send_with_token_refresh<F, Fut>(
    secret_store: &impl SecretStore,
    user_agent: &str,
    client_id: &str,
    client_secret: &str,
    send: F,
) -> Result<reqwest::Response>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = reqwest::Result<reqwest::Response>>,
{
    let token = get_account_token(secret_store)?;
    let response = send(token).await?;
    if response.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Ok(response);
    }

    log::info!("Authenticated request unauthorized, refreshing token and retrying");
    id::try_refresh_token(secret_store, client_id, client_secret, user_agent).await?;
    let refreshed_token = get_account_token(secret_store)?;
    Ok(send(refreshed_token).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemorySecretStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl SecretStore for MemorySecretStore {
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

    #[test]
    fn refresh_deadline_leaves_a_margin_before_expiry() {
        let store = MemorySecretStore::default();
        let expires_in = 3600;
        set_token_refresh_deadline(&store, expires_in).unwrap();

        let deadline = get_token_refresh_deadline(&store).unwrap().unwrap();
        let expiry = now_unix_secs() + expires_in;
        // Refreshes strictly before expiry, leaving at least the minimum margin.
        assert!(deadline < expiry);
        assert!(expiry - deadline >= TOKEN_REFRESH_MIN_MARGIN_SECS);
        // With a one-hour token the margin is a quarter of the lifetime.
        assert_eq!(expiry - deadline, expires_in / TOKEN_REFRESH_MARGIN_DIVISOR);
    }

    #[test]
    fn short_lived_token_keeps_minimum_margin() {
        let store = MemorySecretStore::default();
        // A quarter of 120s is 30s, below the floor, so the floor wins.
        set_token_refresh_deadline(&store, 120).unwrap();

        let deadline = get_token_refresh_deadline(&store).unwrap().unwrap();
        let expiry = now_unix_secs() + 120;
        assert_eq!(expiry - deadline, TOKEN_REFRESH_MIN_MARGIN_SECS);
    }

    #[test]
    fn needs_refresh_is_false_without_a_refresh_token() {
        let store = MemorySecretStore::default();
        set_token_refresh_deadline(&store, 0).unwrap();
        assert!(!account_token_needs_refresh(&store).unwrap());
    }

    #[test]
    fn clear_account_token_reports_failures_and_attempts_every_deletion() {
        struct FailingSecretStore {
            deleted: Mutex<Vec<String>>,
        }

        impl SecretStore for FailingSecretStore {
            fn get_secret(&self, _key: &str) -> Result<Option<String>> {
                Ok(None)
            }

            fn set_secret(&self, _key: &str, _value: &str) -> Result<()> {
                Ok(())
            }

            fn delete_secret(&self, key: &str) -> Result<()> {
                self.deleted.lock().unwrap().push(key.to_string());
                if key == "accountToken" {
                    return Err(anyhow::anyhow!("keyring is locked"));
                }
                Ok(())
            }
        }

        let store = FailingSecretStore {
            deleted: Mutex::new(Vec::new()),
        };
        let error = clear_account_token(&store).unwrap_err();

        assert!(error.to_string().contains("keyring is locked"));
        assert_eq!(
            *store.deleted.lock().unwrap(),
            ["accountToken", "refreshToken", TOKEN_REFRESH_AT_KEY]
        );
    }

    #[test]
    fn needs_refresh_is_true_when_deadline_has_passed() {
        let store = MemorySecretStore::default();
        store.set_secret("refreshToken", "refresh-abc").unwrap();
        // Deadline in the past.
        store
            .set_secret(TOKEN_REFRESH_AT_KEY, &(now_unix_secs() - 5).to_string())
            .unwrap();
        assert!(account_token_needs_refresh(&store).unwrap());
    }

    #[test]
    fn needs_refresh_is_false_before_the_deadline() {
        let store = MemorySecretStore::default();
        store.set_secret("refreshToken", "refresh-abc").unwrap();
        store
            .set_secret(TOKEN_REFRESH_AT_KEY, &(now_unix_secs() + 3600).to_string())
            .unwrap();
        assert!(!account_token_needs_refresh(&store).unwrap());
    }

    #[test]
    fn needs_refresh_when_refresh_token_exists_without_deadline() {
        let store = MemorySecretStore::default();
        store.set_secret("refreshToken", "refresh-abc").unwrap();
        // No deadline recorded (e.g. legacy token) — refresh to establish one.
        assert!(account_token_needs_refresh(&store).unwrap());
    }

    #[test]
    fn clearing_tokens_removes_the_refresh_deadline() {
        let store = MemorySecretStore::default();
        store.set_secret("accountToken", "access-abc").unwrap();
        store.set_secret("refreshToken", "refresh-abc").unwrap();
        set_token_refresh_deadline(&store, 3600).unwrap();

        clear_account_token(&store).unwrap();

        assert!(get_token_refresh_deadline(&store).unwrap().is_none());
        assert!(!account_token_needs_refresh(&store).unwrap());
    }

    #[test]
    fn tokens_only_go_over_https_or_to_this_machine() {
        for allowed in [
            "https://api.usequadrant.dev/api/v3",
            "http://localhost:8080",
            "http://127.0.0.1:3000/api",
            "http://[::1]:3000",
        ] {
            assert!(is_allowed_backend_url(allowed), "{allowed}");
        }
        for refused in [
            "http://api.usequadrant.dev",
            "http://192.168.1.5:3000",
            "http://localhost.evil.test",
            "ftp://localhost",
            "https://",
            "not a url",
            "",
        ] {
            assert!(!is_allowed_backend_url(refused), "{refused}");
        }
    }

    #[test]
    fn a_refused_override_is_an_error_and_changes_nothing() {
        let _guard = ACCOUNT_ENV_TEST_MUTEX.lock().unwrap();
        assert!(set_backend_base_url_override(Some("http://example.test")).is_err());
        set_backend_base_url_override(Some("https://example.test/api")).unwrap();
        assert_eq!(backend_base_url(), "https://example.test/api");
        set_backend_base_url_override(None).unwrap();
        assert_ne!(backend_base_url(), "https://example.test/api");
    }
}
