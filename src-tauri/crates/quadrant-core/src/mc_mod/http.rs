use std::time::Duration;

use http_cache_reqwest::{Cache, CacheMode, HttpCache, HttpCacheOptions, MokaManager};
use moka::future::Cache as MokaCache;
use once_cell::sync::Lazy;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

use super::get_user_agent;

/// How long a connection may take to open, and how long an open one may go
/// without sending a byte. A stalled server fails the request instead of
/// hanging the command (and any per-pack update guard) forever, while a slow
/// but moving download still finishes.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Builder for the process-wide `reqwest` clients.
///
/// Under `cfg(test)` the clients keep no idle connections. Each
/// `#[tokio::test]` runs on its own runtime, and a pooled connection's driver
/// task lives on the runtime that opened it. httpmock recycles its servers, so
/// the next test asks for the same `127.0.0.1:port` and can check out that
/// connection while the previous test's runtime is shutting down, failing with
/// "runtime dropped the dispatch task". Production runs one runtime per
/// process, so it keeps the pool.
pub(crate) fn shared_client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT);
    if cfg!(test) {
        builder.pool_max_idle_per_host(0)
    } else {
        builder
    }
}

static PROVIDER_HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    shared_client_builder()
        .user_agent(get_user_agent())
        .build()
        .expect("failed to build shared provider HTTP client")
});

static PROVIDER_HTTP_CACHE_MANAGER: Lazy<MokaManager> =
    Lazy::new(|| MokaManager::new(MokaCache::new(512)));

static PROVIDER_CACHED_CLIENT: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(provider_http_client().clone())
        .with(Cache(HttpCache {
            mode: CacheMode::Default,
            manager: PROVIDER_HTTP_CACHE_MANAGER.clone(),
            options: HttpCacheOptions::default(),
        }))
        .build()
});

pub(crate) fn provider_http_client() -> &'static reqwest::Client {
    &PROVIDER_HTTP_CLIENT
}

pub(crate) fn provider_cached_client() -> &'static ClientWithMiddleware {
    &PROVIDER_CACHED_CLIENT
}

#[cfg(test)]
pub(crate) static PROVIDER_HTTP_TEST_MUTEX: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

#[cfg(test)]
pub(crate) async fn clear_provider_http_cache() {
    PROVIDER_HTTP_CACHE_MANAGER
        .clear()
        .await
        .expect("failed to clear provider HTTP cache");
}
