//! Updates for both desktop apps, Quadrant (Tauri) and Quadrant-Native
//! (GPUI), which share a data folder and its `channel` setting.
//!
//! The channel picks the app as well as the release line: a Quadrant channel
//! running in Quadrant-Native, or the other way round, means "switch apps".
//! The channel model and the update server's URLs are always here; checking,
//! verifying and installing a release need the `updates` feature.

use std::fmt;

use serde_json::Value;

use crate::QuadrantHost;

#[cfg(feature = "updates")]
mod install;
#[cfg(feature = "updates")]
mod release;

#[cfg(feature = "updates")]
pub use install::detect;
#[cfg(feature = "updates")]
pub use release::{PUBKEY, Release, Update, Updater, verify};

/// Quadrant's update server.
pub const API_BASE: &str = "https://api.usequadrant.dev";
/// The config key both apps keep the channel under.
pub const CHANNEL_KEY: &str = "channel";
/// The version a switch reports, so the server answers with its newest
/// release of the other app.
pub const NO_VERSION: &str = "0.0.0";

/// A release version split into what orders it: `26.11.1-gpui-preview.2` is
/// `([26, 11, 1], "gpui-preview", 2)`.
fn parse_release_version(version: &str) -> Option<([u64; 3], &str, u64)> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let (numbers, suffix) = version.split_once('-').unwrap_or((version, ""));
    let mut parts = numbers.split('.');
    let triple = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    if parts.next().is_some() {
        return None;
    }
    // A trailing `.N` counts releases within one channel tag.
    let (tag, counter) = match suffix.rsplit_once('.') {
        Some((tag, counter)) => match counter.parse() {
            Ok(counter) => (tag, counter),
            Err(_) => (suffix, 0),
        },
        None => (suffix, 0),
    };
    Some((triple, tag, counter))
}

/// Whether `candidate` is a release to install over `current`. Within one
/// release line (the suffix after the number, `stable` in `26.7.0-stable`)
/// it has to be newer, so a server or channel that answers with an older
/// signed build can't roll the app back. A different line is a channel
/// switch, like preview back to stable, and any version counts. Versions
/// that don't follow `YY.MM.REVISION-<line>` count when they differ.
pub fn is_update(current: &str, candidate: &str) -> bool {
    match (
        parse_release_version(current),
        parse_release_version(candidate),
    ) {
        (Some((current, current_line, current_n)), Some((candidate, line, n))) => {
            if current_line == line {
                (candidate, n) > (current, current_n)
            } else {
                true
            }
        }
        _ => current != candidate,
    }
}

/// One of the two apps sharing the data folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum App {
    /// The Tauri app.
    Quadrant,
    /// The GPUI app.
    QuadrantNative,
}

impl App {
    /// Its app on the update server.
    pub const fn slug(self) -> &'static str {
        match self {
            App::Quadrant => "quadrant",
            App::QuadrantNative => "quadrant-native",
        }
    }

    /// Its product name, which is also its `.app` and AppImage file name.
    pub const fn name(self) -> &'static str {
        match self {
            App::Quadrant => "Quadrant",
            App::QuadrantNative => "Quadrant-Native",
        }
    }

    /// Its key in the `appPaths` setting.
    pub const fn key(self) -> &'static str {
        match self {
            App::Quadrant => "quadrant",
            App::QuadrantNative => "quadrantNative",
        }
    }

    pub const fn other(self) -> App {
        match self {
            App::Quadrant => App::QuadrantNative,
            App::QuadrantNative => App::Quadrant,
        }
    }
}

/// The `channel` setting. Its value is also the channel's name on the update
/// server, under its app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Channel {
    #[default]
    Stable,
    Preview,
    GpuiPreview,
    /// Less stable than `GpuiPreview`.
    GpuiExperimental,
}

impl Channel {
    /// In the order the settings list them.
    pub const ALL: [Channel; 4] = [
        Channel::Stable,
        Channel::Preview,
        Channel::GpuiPreview,
        Channel::GpuiExperimental,
    ];

    /// Reads a stored value. Anything unknown is Quadrant's stable channel,
    /// as an unset one is.
    pub fn parse(value: &str) -> Channel {
        Channel::ALL
            .into_iter()
            .find(|channel| channel.as_str() == value)
            .unwrap_or_default()
    }

    /// The stored value and the server's channel name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Preview => "preview",
            Channel::GpuiPreview => "gpui-preview",
            Channel::GpuiExperimental => "gpui-experimental",
        }
    }

    /// The locale key both apps name it with.
    pub const fn label_key(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Preview => "preview",
            Channel::GpuiPreview => "gpuiPreview",
            Channel::GpuiExperimental => "gpuiExperimental",
        }
    }

    /// The app this channel's releases are for.
    pub const fn app(self) -> App {
        match self {
            Channel::Stable | Channel::Preview => App::Quadrant,
            Channel::GpuiPreview | Channel::GpuiExperimental => App::QuadrantNative,
        }
    }

    /// The server channels to ask, in order. Quadrant's preview falls back
    /// to stable, as it always has; Quadrant-Native has no stable channel
    /// and never reads Quadrant's.
    pub const fn server_channels(self) -> &'static [&'static str] {
        match self {
            Channel::Stable => &["stable"],
            Channel::Preview => &["preview", "stable"],
            Channel::GpuiPreview => &["gpui-preview"],
            Channel::GpuiExperimental => &["gpui-experimental"],
        }
    }

    /// The setting as stored in a config's JSON.
    pub fn from_config(value: Option<&Value>) -> Channel {
        value
            .and_then(Value::as_str)
            .map(Channel::parse)
            .unwrap_or_default()
    }

    /// The host's setting.
    pub fn read(host: &QuadrantHost) -> Channel {
        Channel::from_config(host.get_config_value(CHANNEL_KEY).ok().flatten().as_ref())
    }

    /// The update URLs for this channel's app, in the order to try them.
    /// The parts are plain strings so Tauri's updater can pass its
    /// `{{target}}`-style placeholders.
    pub fn urls(self, base: &str, platform: &Platform<'_>) -> Vec<String> {
        let base = base.trim_end_matches('/');
        let app = self.app().slug();
        let Platform {
            target,
            arch,
            version,
            variant,
        } = platform;
        self.server_channels()
            .iter()
            .map(|channel| {
                format!("{base}/api/any/{app}/updates/{channel}/{target}/{arch}/{version}?variant={variant}")
            })
            .collect()
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the update server matches a release against.
pub struct Platform<'a> {
    /// Tauri's `{{target}}`: `windows`, `darwin` or `linux`.
    pub target: &'a str,
    /// Tauri's `{{arch}}`.
    pub arch: &'a str,
    /// The running version, or [`NO_VERSION`] for a switch.
    pub version: &'a str,
    /// Tauri's `{{bundle_type}}`.
    pub variant: &'a str,
}

/// Tauri's `{{target}}` for this machine.
pub fn target() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

/// Tauri's `{{arch}}` for this machine.
pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "i686",
        "arm" => "armv7",
        other => other,
    }
}

/// How an app was installed: the artifact the server answers with (Tauri's
/// `{{bundle_type}}`) and how a release is put in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bundle {
    /// The NSIS setup, run passively.
    Nsis,
    /// A macOS `.app`, from an `.app.tar.gz`.
    App,
    AppImage,
    Deb,
    Rpm,
}

impl Bundle {
    pub const fn variant(self) -> &'static str {
        match self {
            Bundle::Nsis => "nsis",
            Bundle::App => "app",
            Bundle::AppImage => "appimage",
            Bundle::Deb => "deb",
            Bundle::Rpm => "rpm",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_map_to_an_app_and_a_server_channel() {
        let table = [
            ("stable", App::Quadrant, &["stable"][..]),
            ("preview", App::Quadrant, &["preview", "stable"][..]),
            ("gpui-preview", App::QuadrantNative, &["gpui-preview"][..]),
            (
                "gpui-experimental",
                App::QuadrantNative,
                &["gpui-experimental"][..],
            ),
        ];
        for (value, app, servers) in table {
            let channel = Channel::parse(value);
            assert_eq!(channel.as_str(), value);
            assert_eq!(channel.to_string(), value);
            assert_eq!(channel.app(), app, "{value}");
            assert_eq!(channel.server_channels(), servers, "{value}");
        }
    }

    #[test]
    fn unknown_and_unset_values_are_quadrant_stable() {
        for value in ["", "alpha", "gpuiPreview", "experimental", "Stable"] {
            assert_eq!(Channel::parse(value), Channel::Stable, "{value}");
        }
        assert_eq!(Channel::from_config(None), Channel::Stable);
        assert_eq!(Channel::from_config(Some(&Value::Null)), Channel::Stable);
        assert_eq!(
            Channel::from_config(Some(&Value::from("gpui-experimental"))),
            Channel::GpuiExperimental
        );
    }

    #[test]
    fn quadrant_native_never_reads_quadrants_channels() {
        for channel in Channel::ALL {
            if channel.app() == App::QuadrantNative {
                assert!(
                    channel
                        .server_channels()
                        .iter()
                        .all(|name| name.starts_with("gpui-"))
                );
            }
        }
    }

    #[test]
    fn label_keys_are_the_locale_keys() {
        let keys = Channel::ALL.map(Channel::label_key);
        assert_eq!(
            keys,
            ["stable", "preview", "gpuiPreview", "gpuiExperimental"]
        );
    }

    #[test]
    fn a_release_must_be_newer_within_its_line() {
        for (current, candidate, expected) in [
            ("26.11.0-stable", "26.11.1-stable", true),
            ("26.11.0-stable", "26.12.0-stable", true),
            ("26.11.0-stable", "27.1.0-stable", true),
            ("26.11.0-stable", "26.11.0-stable", false),
            ("26.11.1-stable", "26.11.0-stable", false),
            ("26.11.0-stable", "26.7.9-stable", false),
            ("26.11.0-stable", "v26.11.1-stable", true),
            // A channel switch may land on an older number.
            ("26.11.0-alpha", "26.7.0-stable", true),
            ("26.11.0-gpui-preview.2", "26.11.0-gpui-preview.3", true),
            ("26.11.0-gpui-preview.2", "26.11.0-gpui-preview.1", false),
            ("26.11.0-gpui-preview.2", "26.10.9-gpui-preview.9", false),
            (
                "26.11.0-gpui-preview.2",
                "26.11.0-gpui-experimental.1",
                true,
            ),
            ("0.0.0", "26.7.0-stable", true),
            // Not a Quadrant version: the old rule, any difference.
            ("dev", "26.7.0-stable", true),
            ("dev", "dev", false),
        ] {
            assert_eq!(
                is_update(current, candidate),
                expected,
                "{current} -> {candidate}"
            );
        }
    }

    #[test]
    fn apps_know_each_other() {
        assert_eq!(App::Quadrant.other(), App::QuadrantNative);
        assert_eq!(App::QuadrantNative.other(), App::Quadrant);
        assert_eq!(App::QuadrantNative.slug(), "quadrant-native");
        assert_eq!(App::QuadrantNative.key(), "quadrantNative");
    }

    #[test]
    fn urls_name_the_channels_app_and_fall_back_only_for_quadrant() {
        let platform = Platform {
            target: "windows",
            arch: "x86_64",
            version: "26.11.0-stable",
            variant: "nsis",
        };
        assert_eq!(
            Channel::Preview.urls("https://api.test/", &platform),
            [
                "https://api.test/api/any/quadrant/updates/preview/windows/x86_64/26.11.0-stable?variant=nsis",
                "https://api.test/api/any/quadrant/updates/stable/windows/x86_64/26.11.0-stable?variant=nsis",
            ]
        );
        assert_eq!(
            Channel::GpuiExperimental.urls(API_BASE, &platform),
            [
                "https://api.usequadrant.dev/api/any/quadrant-native/updates/gpui-experimental/windows/x86_64/26.11.0-stable?variant=nsis"
            ]
        );
        let tauri = Platform {
            target: "{{target}}",
            arch: "{{arch}}",
            version: "{{current_version}}",
            variant: "{{bundle_type}}",
        };
        assert_eq!(
            Channel::Stable.urls(API_BASE, &tauri),
            [
                "https://api.usequadrant.dev/api/any/quadrant/updates/stable/{{target}}/{{arch}}/{{current_version}}?variant={{bundle_type}}"
            ]
        );
    }

    #[test]
    fn platform_names_match_tauris() {
        assert!(["windows", "darwin", "linux"].contains(&target()));
        assert!(["x86_64", "aarch64", "i686", "armv7"].contains(&arch()));
    }
}
