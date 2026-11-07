//! What Quadrant hands to quadrant-host to share the data folder with
//! Quadrant-Native: which launches may follow the channel to the other app,
//! what a second launch forwards, and how this copy was bundled.

use std::{ffi::OsString, path::PathBuf};

/// Flags that take a value, which isn't a link.
const VALUE_FLAGS: [&str; 1] = ["--api-url"];

/// The data folder Tauri's `app_data_dir` resolves to, found before the app
/// is built so a second launch can hand over without starting.
pub fn data_dir(identifier: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(identifier))
}

/// The links among a launch's arguments: every argument that isn't a flag
/// or a flag's value.
pub fn links(args: &[OsString]) -> Vec<String> {
    let mut links = Vec::new();
    let mut args = args.iter().map(|arg| arg.to_string_lossy());
    while let Some(arg) = args.next() {
        if VALUE_FLAGS.contains(&arg.as_ref()) {
            args.next();
        } else if !arg.starts_with('-') {
            links.push(arg.into_owned());
        }
    }
    links
}

/// Whether this launch may follow the channel to Quadrant-Native: an
/// installed copy that updates itself. Store, Flatpak and `--noupdater`
/// builds leave updates, and so the app, to their package manager.
pub fn may_redirect(args: &[OsString], version: &str, dev: bool, flatpak: bool) -> bool {
    !dev && !flatpak && !version.contains("msstore") && !args.iter().any(|arg| arg == "--noupdater")
}

/// Whether this is an installed copy, whose path the other app may start.
pub fn installed() -> bool {
    !tauri::is_dev() && tauri::utils::platform::bundle_type().is_some()
}

/// The bundle a switch installs Quadrant-Native's matching artifact for.
#[cfg(feature = "updater")]
pub fn bundle() -> Option<quadrant_host::updates::Bundle> {
    use quadrant_host::updates::Bundle;
    use tauri::utils::config::BundleType;
    match tauri::utils::platform::bundle_type()? {
        BundleType::Nsis => Some(Bundle::Nsis),
        BundleType::App => Some(Bundle::App),
        BundleType::AppImage => Some(Bundle::AppImage),
        BundleType::Deb => Some(Bundle::Deb),
        BundleType::Rpm => Some(Bundle::Rpm),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn links_skip_flags_and_their_values() {
        assert_eq!(
            links(&args(&[
                "--autostart",
                "--api-url",
                "https://api.test",
                "modrinth://mod/sodium",
                "--noupdater",
            ])),
            ["modrinth://mod/sodium"]
        );
        assert!(links(&[]).is_empty());
    }

    #[test]
    fn only_self_updating_installs_follow_the_channel() {
        assert!(may_redirect(
            &args(&["x://y"]),
            "26.11.0-stable",
            false,
            false
        ));
        assert!(!may_redirect(&args(&[]), "26.11.0-stable", true, false));
        assert!(!may_redirect(&args(&[]), "26.11.0-stable", false, true));
        assert!(!may_redirect(&args(&[]), "26.11.0-msstore", false, false));
        assert!(!may_redirect(
            &args(&["--noupdater"]),
            "26.11.0-stable",
            false,
            false
        ));
    }
}
