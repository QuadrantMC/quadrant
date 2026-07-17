//! Account token helpers and Quadrant cloud entrypoints.

use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Result, ports::SecretStore};

pub mod id;
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

/// Resolves the backend base URL.
/// Precedence: CLI arg > `QUADRANT_API_BASE_URL` env var at runtime >
/// `QUADRANT_API_BASE_URL` env var at compile time > hardcoded default.
pub fn backend_base_url() -> String {
    env::var("QUADRANT_API_BASE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| qnt_base_url().to_string())
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
        .ok_or_else(|| anyhow::anyhow!("No account token"))
}

/// Reads the current account refresh token.
pub fn get_refresh_token(secret_store: &impl SecretStore) -> Result<String> {
    secret_store
        .get_secret("refreshToken")?
        .ok_or_else(|| anyhow::anyhow!("No refresh token"))
}

/// Removes any persisted account and refresh tokens.
pub fn clear_account_token(secret_store: &impl SecretStore) -> Result<()> {
    log::info!("Clearing account and refresh tokens");
    let _ = secret_store.delete_secret("accountToken");
    let _ = secret_store.delete_secret("refreshToken");
    let _ = secret_store.delete_secret(TOKEN_REFRESH_AT_KEY);
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
}
