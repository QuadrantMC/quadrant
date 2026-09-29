//! Link parsing ported from `src/deepLinks.ts`, and `open`, which acts on
//! a link the way the desktop app does when the OS hands one to it.

use anyhow::{Result, anyhow, bail};
use clap::Args;
use quadrant_core::models::{ModLoader, ModSource};
use url::Url;

use crate::{Ctx, account, args, modpack, mods, output::Report};

#[derive(Debug, Args)]
pub struct OpenArgs {
    /// A curseforge://, modrinth:// or quadrantnext:// link, or a
    /// https://usequadrant.dev/modpack/<code> share link.
    url: String,
    /// For mod links: the modpack to install into.
    #[arg(long)]
    modpack: Option<String>,
    /// For mod links: the Minecraft version to install for.
    #[arg(long)]
    version: Option<String>,
    /// For mod links: the mod loader to install for.
    #[arg(long, value_parser = args::loader)]
    loader: Option<ModLoader>,
    /// For modpack links: install under a different name.
    #[arg(long)]
    name: Option<String>,
    /// For modpack links: replace a local modpack with the same name without
    /// asking.
    #[arg(long, short)]
    yes: bool,
}

pub async fn open(link: OpenArgs, ctx: &Ctx) -> Result<Report> {
    let resolved =
        resolve(&link.url).map_err(|error| anyhow!("{:?} is not a link: {error}", link.url))?;
    match resolved {
        DeepLink::InstallMod {
            source,
            id,
            file_id,
        } => {
            mods::install(
                ctx,
                mods::InstallArgs {
                    id,
                    source,
                    modpack: link.modpack,
                    version: link.version,
                    loader: link.loader,
                    file_id,
                    location: None,
                    with_deps: false,
                },
            )
            .await
        }
        DeepLink::ImportModpack { code } => modpack::import(ctx, &code, link.name, link.yes).await,
        DeepLink::OauthLogin {
            state,
            code,
            redirect_uri,
        } => account::login_from_link(ctx, state, code, redirect_uri).await,
        DeepLink::Unsupported => bail!("Quadrant can't open this kind of link"),
        DeepLink::NotQuadrant => bail!("not a Quadrant, CurseForge or Modrinth link"),
    }
}

/// What a link asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    InstallMod {
        source: ModSource,
        id: String,
        file_id: Option<String>,
    },
    /// An OAuth redirect; the caller still checks `state` against the one it
    /// stored when the sign-in started.
    OauthLogin {
        state: Option<String>,
        code: Option<String>,
        redirect_uri: String,
    },
    ImportModpack {
        code: String,
    },
    /// A Quadrant, CurseForge or Modrinth link this app can't act on.
    Unsupported,
    /// Not a link this app handles at all.
    NotQuadrant,
}

/// Classifies a link the way the desktop app's deep-link handler does.
pub fn resolve(raw: &str) -> Result<DeepLink, url::ParseError> {
    let url = Url::parse(raw)?;
    let host = url.host_str().unwrap_or_default();
    let parts = path_parts(&url);
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    // quadrantnext:// and curseforge:// links name their action in the host,
    // or in the first path segment when there is no host.
    let action = if host.is_empty() {
        parts.first().copied().unwrap_or_default()
    } else {
        host
    }
    .to_lowercase();

    Ok(match url.scheme() {
        "curseforge" => {
            let id = param("addonId").unwrap_or_default().trim().to_string();
            if action != "install" || id.is_empty() {
                DeepLink::Unsupported
            } else {
                DeepLink::InstallMod {
                    source: ModSource::CurseForge,
                    id,
                    file_id: param("fileId"),
                }
            }
        }
        "modrinth" => {
            let mut parts: Vec<&str> = std::iter::once(host)
                .chain(parts)
                .filter(|part| !part.is_empty())
                .collect();
            // Links can wrap a website address, as in
            // modrinth://https://modrinth.com/mod/sodium.
            for wrapper in ["https", "modrinth.com"] {
                if parts
                    .first()
                    .is_some_and(|part| part.eq_ignore_ascii_case(wrapper))
                {
                    parts.remove(0);
                }
            }
            match parts.as_slice() {
                [kind, id, ..]
                    if ["mod", "resourcepack", "shader"]
                        .contains(&kind.to_lowercase().as_str()) =>
                {
                    DeepLink::InstallMod {
                        source: ModSource::Modrinth,
                        id: id.to_string(),
                        file_id: None,
                    }
                }
                _ => DeepLink::Unsupported,
            }
        }
        "quadrantnext" => {
            let path_value = if host.is_empty() {
                parts.get(1)
            } else {
                parts.first()
            }
            .map(|value| value.to_string());
            if action == "login" || url.path().split('/').any(|part| part == "login") {
                let without_fragment = raw.split('#').next().unwrap_or(raw);
                DeepLink::OauthLogin {
                    state: param("state"),
                    code: param("code"),
                    redirect_uri: without_fragment
                        .split('?')
                        .next()
                        .unwrap_or(without_fragment)
                        .to_string(),
                }
            } else if action == "modrinth" || action == "curseforge" {
                let id = param("modId")
                    .or_else(|| param("addonId"))
                    .or(path_value)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if id.is_empty() {
                    DeepLink::Unsupported
                } else {
                    DeepLink::InstallMod {
                        source: if action == "modrinth" {
                            ModSource::Modrinth
                        } else {
                            ModSource::CurseForge
                        },
                        id,
                        file_id: param("fileId"),
                    }
                }
            } else if action == "modpack" {
                let code = param("code")
                    .or_else(|| param("sharedCode"))
                    .or(path_value)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if code.is_empty() {
                    DeepLink::Unsupported
                } else {
                    DeepLink::ImportModpack { code }
                }
            } else {
                DeepLink::NotQuadrant
            }
        }
        "https" => match parse_share_code(raw) {
            Some(code) => DeepLink::ImportModpack { code },
            None => DeepLink::NotQuadrant,
        },
        _ => DeepLink::NotQuadrant,
    })
}

fn path_parts(url: &Url) -> Vec<&str> {
    url.path()
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .collect()
}

/// A bare 7-digit Quadrant Share code, or the code in a
/// `https://usequadrant.dev/modpack/<code>` link.
pub fn parse_share_code(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if is_share_code(trimmed) {
        return Some(trimmed.to_string());
    }
    let url = Url::parse(trimmed).ok()?;
    if !matches!(
        url.host_str(),
        Some("usequadrant.dev" | "www.usequadrant.dev")
    ) {
        return None;
    }
    let path = url.path().strip_prefix("/modpack/")?;
    let code = path.strip_suffix('/').unwrap_or(path);
    is_share_code(code).then(|| code.to_string())
}

fn is_share_code(value: &str) -> bool {
    value.len() == 7 && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(source: ModSource, id: &str, file_id: Option<&str>) -> DeepLink {
        DeepLink::InstallMod {
            source,
            id: id.to_string(),
            file_id: file_id.map(str::to_string),
        }
    }

    fn import(code: &str) -> DeepLink {
        DeepLink::ImportModpack {
            code: code.to_string(),
        }
    }

    #[test]
    fn curseforge_install_links() {
        assert_eq!(
            resolve("curseforge://install?addonId=238222&fileId=99").unwrap(),
            install(ModSource::CurseForge, "238222", Some("99"))
        );
        for url in [
            "curseforge://open?addonId=1",
            "curseforge://install",
            "curseforge://install?addonId=%20",
        ] {
            assert_eq!(resolve(url).unwrap(), DeepLink::Unsupported, "{url}");
        }
    }

    #[test]
    fn modrinth_project_links() {
        for url in [
            "modrinth://mod/sodium",
            "modrinth://mod/sodium/",
            "modrinth:///mod/sodium",
            "modrinth://modrinth.com/mod/sodium",
            "modrinth://https://modrinth.com/mod/sodium",
        ] {
            assert_eq!(
                resolve(url).unwrap(),
                install(ModSource::Modrinth, "sodium", None),
                "{url}"
            );
        }
        assert_eq!(
            resolve("modrinth://resourcepack/faithful").unwrap(),
            install(ModSource::Modrinth, "faithful", None)
        );
        assert_eq!(
            resolve("modrinth://shader/complementary").unwrap(),
            install(ModSource::Modrinth, "complementary", None)
        );
        for url in [
            "modrinth://mod",
            "modrinth://mod/",
            "modrinth:///mod/",
            "modrinth://user/mod-fan",
            "modrinth://user/jellysquid",
            "modrinth://unsupported-mod/sodium",
        ] {
            assert_eq!(resolve(url).unwrap(), DeepLink::Unsupported, "{url}");
        }
    }

    #[test]
    fn quadrantnext_links() {
        assert_eq!(
            resolve("quadrantnext://login?state=abc&code=xyz#fragment").unwrap(),
            DeepLink::OauthLogin {
                state: Some("abc".to_string()),
                code: Some("xyz".to_string()),
                redirect_uri: "quadrantnext://login".to_string(),
            }
        );
        assert_eq!(
            resolve("quadrantnext://modrinth?modId=sodium").unwrap(),
            install(ModSource::Modrinth, "sodium", None)
        );
        assert_eq!(
            resolve("quadrantnext://curseforge?addonId=1&fileId=2").unwrap(),
            install(ModSource::CurseForge, "1", Some("2"))
        );
        assert_eq!(
            resolve("quadrantnext://modrinth/sodium").unwrap(),
            install(ModSource::Modrinth, "sodium", None)
        );
        assert_eq!(
            resolve("quadrantnext:///curseforge/238222").unwrap(),
            install(ModSource::CurseForge, "238222", None)
        );
        assert_eq!(
            resolve("quadrantnext://modrinth").unwrap(),
            DeepLink::Unsupported
        );
        assert_eq!(
            resolve("quadrantnext://modpack?code=1234567").unwrap(),
            import("1234567")
        );
        assert_eq!(
            resolve("quadrantnext://modpack?sharedCode=222").unwrap(),
            import("222")
        );
        assert_eq!(
            resolve("quadrantnext://mystery").unwrap(),
            DeepLink::NotQuadrant
        );
    }

    #[test]
    fn https_share_links_and_other_input() {
        assert_eq!(
            resolve("https://usequadrant.dev/modpack/1234567").unwrap(),
            import("1234567")
        );
        assert_eq!(
            resolve("https://www.usequadrant.dev/modpack/7654321").unwrap(),
            import("7654321")
        );
        for url in [
            "https://usequadrant.dev/modpack/12",
            "https://evil.example.com/modpack/1234567",
            "mailto:someone@example.com",
        ] {
            assert_eq!(resolve(url).unwrap(), DeepLink::NotQuadrant, "{url}");
        }
        assert!(resolve("not a url").is_err());
    }

    #[test]
    fn share_codes_accept_bare_codes_and_links() {
        for (input, code) in [
            ("1234567", "1234567"),
            ("https://usequadrant.dev/modpack/1234567", "1234567"),
            ("https://www.usequadrant.dev/modpack/7654321", "7654321"),
            ("https://usequadrant.dev/modpack/1234567/", "1234567"),
            ("  1234567  ", "1234567"),
            ("  https://usequadrant.dev/modpack/1234567  ", "1234567"),
        ] {
            assert_eq!(parse_share_code(input).as_deref(), Some(code), "{input}");
        }
    }

    #[test]
    fn share_codes_reject_other_shapes() {
        for input in [
            "123456",
            "12345678",
            "https://usequadrant.dev/modpack/123456",
            "https://evil.example.com/modpack/1234567",
            "not a url",
        ] {
            assert_eq!(parse_share_code(input), None, "{input}");
        }
    }
}
