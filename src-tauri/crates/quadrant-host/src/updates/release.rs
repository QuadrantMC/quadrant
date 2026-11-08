//! Asking the update server for a release, downloading it and checking its
//! minisign signature, as Tauri's updater does.

use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use reqwest::StatusCode;
use serde::Deserialize;

use super::{App, Bundle, Channel, NO_VERSION, Platform, arch, install, target};

/// The minisign public key releases of both apps are signed with, base64 as
/// in Quadrant's `tauri.conf.json`. A build can swap it
/// (`QUADRANT_UPDATER_PUBKEY`) to test against a throwaway key.
pub const PUBKEY: &str = match option_env!("QUADRANT_UPDATER_PUBKEY") {
    Some(key) => key,
    None => QUADRANT_PUBKEY,
};
const QUADRANT_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEE3MEJDMDAxMjA4NjlBQ0MKUldUTW1vWWdBY0FMcCtkS3FwaTlKSzBaMHpXaW4ramRLS1ludmVyUEZpSUY3Mmc0cm1zeGUvRGIK";

/// The server's answer: the newest release for a platform and variant.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    /// Base64 of the artifact's minisign `.sig` file.
    pub signature: String,
}

impl Release {
    pub fn parse(body: &[u8]) -> Result<Self> {
        let mut release: Release =
            serde_json::from_slice(body).context("the update server's answer isn't a release")?;
        if let Some(version) = release.version.strip_prefix('v') {
            release.version = version.to_string();
        }
        Ok(release)
    }

    /// Whether to install it. Like Tauri's updater as Quadrant configures
    /// it, any other version counts, so moving from preview back to stable
    /// moves to stable's release.
    pub fn is_update_for(&self, current_version: &str) -> bool {
        self.version != current_version
    }

    /// The download must use TLS unless a test server serves it.
    pub fn check_url(&self, allow_http: bool) -> Result<()> {
        let url = url::Url::parse(&self.url).context("the update's download URL is invalid")?;
        match url.scheme() {
            "https" => Ok(()),
            "http" if allow_http => Ok(()),
            scheme => bail!("refusing to download an update over {scheme}"),
        }
    }
}

/// Checks `bytes` against a minisign signature, both as Tauri writes them:
/// base64 of the `.sig` file and base64 of the public key file.
pub fn verify(bytes: &[u8], signature: &str, pubkey: &str) -> Result<()> {
    let pubkey = PublicKey::decode(&decode_base64(pubkey)?).context("bad updater public key")?;
    let signature =
        Signature::decode(&decode_base64(signature)?).context("bad update signature")?;
    pubkey
        .verify(bytes, &signature, true)
        .context("the update's signature doesn't match")
}

fn decode_base64(text: &str) -> Result<String> {
    Ok(String::from_utf8(STANDARD.decode(text.trim())?)?)
}

/// What a check needs to know about the running app.
pub struct Updater {
    pub running: App,
    pub current_version: String,
    pub bundle: Bundle,
    /// The update server, without the `/api/...` path.
    pub server: String,
    /// Plain-http downloads, for a test server only.
    pub allow_http: bool,
    pub user_agent: String,
    /// The key releases must be signed with: [`PUBKEY`] outside tests.
    pub pubkey: String,
}

/// A downloaded release whose signature checked out, ready to install.
pub struct Update {
    pub app: App,
    pub version: String,
    running: App,
    bundle: Bundle,
    bytes: Vec<u8>,
}

impl Updater {
    /// The URLs to ask for `channel`'s release. The running app asks with
    /// its version; the other app's release is asked for as a fresh install.
    pub fn urls(&self, channel: Channel) -> Vec<String> {
        let version = if channel.app() == self.running {
            self.current_version.as_str()
        } else {
            NO_VERSION
        };
        channel.urls(
            &self.server,
            &Platform {
                target: target(),
                arch: arch(),
                version,
                variant: self.bundle.variant(),
            },
        )
    }

    /// Finds `channel`'s release and downloads and verifies it, reporting
    /// the download's progress from 0 to 1. `None` when there is nothing to
    /// install: no release, or the running version is the channel's.
    pub async fn check(
        &self,
        channel: Channel,
        mut progress: impl FnMut(f32),
    ) -> Result<Option<Update>> {
        let client = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        let Some(release) = fetch(&client, &self.urls(channel)).await? else {
            log::info!("No {} release on {channel}", channel.app().name());
            return Ok(None);
        };
        let app = channel.app();
        if app == self.running && !release.is_update_for(&self.current_version) {
            log::info!("{} {} is up to date", app.name(), self.current_version);
            return Ok(None);
        }
        release.check_url(self.allow_http)?;
        log::info!(
            "Downloading {} {} from {}",
            app.name(),
            release.version,
            release.url
        );
        let bytes = download(&client, &release.url, &mut progress).await?;
        verify(&bytes, &release.signature, &self.pubkey)?;
        log::info!(
            "{} {} is downloaded and its signature checks out",
            app.name(),
            release.version
        );
        Ok(Some(Update {
            app,
            version: release.version,
            running: self.running,
            bundle: self.bundle,
            bytes,
        }))
    }
}

impl Update {
    /// The app this update switches to, when it isn't the running one.
    pub fn switches_to(&self) -> Option<App> {
        (self.app != self.running).then_some(self.app)
    }

    /// Puts the release in place and starts it, replacing the running app
    /// or installing the other one beside it. `release` lets go of the
    /// single instance socket (`Serving::release`) just before the installed
    /// app can start and claim it. Blocks; the caller quits once it returns.
    pub fn install(&self, release: impl FnOnce()) -> Result<()> {
        install::install(self.app, self.running, self.bundle, &self.bytes, release)
    }

    /// A ready update without a download, for screenshots and tests.
    pub fn preview(app: App, running: App, version: &str) -> Update {
        Update {
            app,
            version: version.to_string(),
            running,
            bundle: Bundle::Nsis,
            bytes: Vec::new(),
        }
    }
}

/// The first endpoint's answer, falling back to the next on failure, as
/// Tauri's updater does. 204 and 404 mean there is nothing to install.
async fn fetch(client: &reqwest::Client, urls: &[String]) -> Result<Option<Release>> {
    let mut failure = anyhow!("no update endpoints");
    for url in urls {
        log::info!("Checking for updates at {url}");
        match client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(response)
                if matches!(
                    response.status(),
                    StatusCode::NO_CONTENT | StatusCode::NOT_FOUND
                ) =>
            {
                return Ok(None);
            }
            Ok(response) if response.status().is_success() => {
                return Release::parse(&response.bytes().await?).map(Some);
            }
            Ok(response) => failure = anyhow!("{url} answered {}", response.status()),
            Err(error) => failure = error.into(),
        }
    }
    Err(failure)
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    progress: &mut impl FnMut(f32),
) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    let total = response.content_length().filter(|total| *total > 0);
    let mut bytes = Vec::with_capacity(total.unwrap_or_default() as usize);
    progress(0.);
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if let Some(total) = total {
            progress(bytes.len() as f32 / total as f32);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway key pair made with `tauri signer generate` for these tests.
    const TEST_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEUxRjhGMjBFMDQzNUJDNDEKUldSQnZEVUVEdkw0NFg3Y3dOaThZK0FzNzg3L05VcFZETERJcVZwcEs5UTVmck5WM1NBdm0rSGcK";
    const FIXTURE: &[u8] = b"quadrant-native update fixture\n";
    // `tauri signer sign` of FIXTURE with that key, and with a different key.
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSQnZEVUVEdkw0NFVUTzJxU1orMnErZ1ZXUmYvL1FiMTlXNnlGR3VYMUhXcE11U2oxUk9wM1M3K09WM0hXUnVUWGpHcnpJWG9uYXlCTkV3RWV5bGVHbDBYSHN3bG1tU3djPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMzIxOTMxCWZpbGU6Zml4dHVyZS5iaW4KMkkyR2VHVmJDKzhZMkk2dlJ2VmNNenUwK3Z3Q3AyWVNkN2N3dDJnY2FhZ1pzL1FaRFY5T1ZjZC9Vdk5kTDVoMWtta0tpRExxdzFkTHlYRk9WT3VOQVE9PQo=";
    const OTHER_KEY_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSdW9QQzNTdzlDb1N5MkpkRU5zK0dIUDF4VnZzcnJVUENwS281SzlFVk1VYWh0cUdPK0EzMDRKU09lZnJkTWtlblc2NjJ3UDMzeUV0MVRmL0lrczZZRFhDaWVhdGNTY0FFPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMzIxOTMyCWZpbGU6Zml4dHVyZS5iaW4KSkZlNmFuaTBsOFBYQldHcU51RXdONzg1Z3BvTnhybmpGRXJCUjgxUUVJNmIwOGNDZFE5WDRrVzlOVTdDWEk4c21KakRKVHpLOHU3VXZPNlhsOEwzQnc9PQo=";

    #[test]
    fn accepts_the_signed_bytes_only() {
        verify(FIXTURE, SIGNATURE, TEST_PUBKEY).unwrap();
        let mut tampered = FIXTURE.to_vec();
        tampered[0] ^= 1;
        assert!(verify(&tampered, SIGNATURE, TEST_PUBKEY).is_err());
        assert!(verify(FIXTURE, OTHER_KEY_SIGNATURE, TEST_PUBKEY).is_err());
        assert!(verify(FIXTURE, "not base64", TEST_PUBKEY).is_err());
    }

    #[test]
    fn quadrants_key_is_the_default_and_decodes() {
        if option_env!("QUADRANT_UPDATER_PUBKEY").is_none() {
            assert_eq!(PUBKEY, QUADRANT_PUBKEY);
        }
        PublicKey::decode(&decode_base64(QUADRANT_PUBKEY).unwrap()).unwrap();
        assert!(verify(FIXTURE, SIGNATURE, QUADRANT_PUBKEY).is_err());
    }

    #[test]
    fn the_key_is_the_one_tauris_updater_checks() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../../../tauri.conf.json")).unwrap();
        assert_eq!(config["plugins"]["updater"]["pubkey"], QUADRANT_PUBKEY);
    }

    #[test]
    fn reads_the_servers_row() {
        let body = br#"{
            "product": "quadrant-native", "version": "v26.10.1-gpui-preview.1",
            "pub_date": "2026-10-20T10:00:00+00:00", "arch": "x86_64",
            "signature": "c2ln", "platform": "windows",
            "url": "https://github.com/a/b/releases/download/v26.10.1-gpui-preview.1/setup.exe",
            "branch": "gpui-preview", "public": true, "version_id": "x",
            "platform_variation": "nsis"
        }"#;
        let release = Release::parse(body).unwrap();
        assert_eq!(release.version, "26.10.1-gpui-preview.1");
        assert_eq!(release.signature, "c2ln");
        assert!(release.is_update_for("26.10.0-gpui-preview.1"));
        assert!(!release.is_update_for("26.10.1-gpui-preview.1"));
        assert!(release.check_url(false).is_ok());
        assert!(Release::parse(b"No supported versions").is_err());
    }

    #[test]
    fn plain_http_downloads_need_the_test_server() {
        let release = Release {
            version: "1.0.0".into(),
            url: "http://127.0.0.1:8000/setup.exe".into(),
            signature: String::new(),
        };
        assert!(release.check_url(false).is_err());
        assert!(release.check_url(true).is_ok());
    }

    fn updater(running: App) -> Updater {
        Updater {
            running,
            current_version: "26.11.0-stable".into(),
            bundle: Bundle::AppImage,
            server: "https://api.test".into(),
            allow_http: false,
            user_agent: "test".into(),
            pubkey: TEST_PUBKEY.into(),
        }
    }

    #[test]
    fn the_other_apps_release_is_asked_for_as_a_fresh_install() {
        let tail = format!("{}/{}", target(), arch());
        assert_eq!(
            updater(App::Quadrant).urls(Channel::GpuiPreview),
            [format!(
                "https://api.test/api/any/quadrant-native/updates/gpui-preview/{tail}/0.0.0?variant=appimage"
            )]
        );
        assert_eq!(
            updater(App::Quadrant).urls(Channel::Stable),
            [format!(
                "https://api.test/api/any/quadrant/updates/stable/{tail}/26.11.0-stable?variant=appimage"
            )]
        );
        assert_eq!(
            updater(App::QuadrantNative).urls(Channel::Stable),
            [format!(
                "https://api.test/api/any/quadrant/updates/stable/{tail}/0.0.0?variant=appimage"
            )]
        );
    }

    /// A server with `channel`'s release of `app`: FIXTURE, signed as given.
    fn serve(app: App, channel: &str, version: &str, signature: &str) -> httpmock::MockServer {
        let server = httpmock::MockServer::start();
        let url = server.url("/files/setup.exe");
        server.mock(|when, then| {
            when.path_includes(format!("/api/any/{}/updates/{channel}/", app.slug()));
            then.status(200).json_body(serde_json::json!({
                "version": format!("v{version}"),
                "url": url,
                "signature": signature,
            }));
        });
        server.mock(|when, then| {
            when.path("/files/setup.exe");
            then.status(200).body(FIXTURE);
        });
        server
    }

    fn against(server: &httpmock::MockServer, running: App) -> Updater {
        Updater {
            server: server.base_url(),
            allow_http: true,
            ..updater(running)
        }
    }

    #[tokio::test]
    async fn a_switch_downloads_and_verifies_the_other_apps_release() {
        let server = serve(
            App::QuadrantNative,
            "gpui-preview",
            "26.11.1-gpui-preview.1",
            SIGNATURE,
        );
        let mut progress = Vec::new();
        let update = against(&server, App::Quadrant)
            .check(Channel::GpuiPreview, |done| progress.push(done))
            .await
            .unwrap()
            .expect("a release to switch to");
        assert_eq!(update.switches_to(), Some(App::QuadrantNative));
        assert_eq!(update.version, "26.11.1-gpui-preview.1");
        assert_eq!(update.bytes, FIXTURE);
        assert_eq!(progress.first(), Some(&0.));
        assert_eq!(progress.last(), Some(&1.));
    }

    #[tokio::test]
    async fn the_running_version_is_no_update_but_the_other_apps_is_a_switch() {
        let server = serve(App::Quadrant, "stable", "26.11.0-stable", SIGNATURE);
        let running = against(&server, App::Quadrant);
        assert!(
            running
                .check(Channel::Stable, |_| {})
                .await
                .unwrap()
                .is_none()
        );
        let native = against(&server, App::QuadrantNative);
        let update = native
            .check(Channel::Stable, |_| {})
            .await
            .unwrap()
            .unwrap();
        assert_eq!(update.switches_to(), Some(App::Quadrant));
    }

    #[tokio::test]
    async fn a_release_signed_with_another_key_is_refused() {
        let server = serve(
            App::QuadrantNative,
            "gpui-experimental",
            "1.0.0",
            OTHER_KEY_SIGNATURE,
        );
        let error = against(&server, App::QuadrantNative)
            .check(Channel::GpuiExperimental, |_| {})
            .await
            .err()
            .expect("a refusal");
        assert!(
            format!("{error:#}").contains("signature doesn't match"),
            "{error:#}"
        );
    }

    #[tokio::test]
    async fn no_release_on_the_channel_is_nothing_to_install() {
        let server = httpmock::MockServer::start();
        server.mock(|_, then| {
            then.status(404);
        });
        let updater = against(&server, App::QuadrantNative);
        assert!(
            updater
                .check(Channel::GpuiPreview, |_| {})
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_update_for_the_other_app_is_a_switch() {
        let own = Update::preview(App::Quadrant, App::Quadrant, "1");
        assert_eq!(own.switches_to(), None);
        let other = Update::preview(App::QuadrantNative, App::Quadrant, "1");
        assert_eq!(other.switches_to(), Some(App::QuadrantNative));
    }
}
