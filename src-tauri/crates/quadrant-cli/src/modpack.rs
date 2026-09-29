use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use futures::future::join_all;
use quadrant_core::{
    error::ErrorCode,
    mc_mod::{GetModArgs, Mod},
    models::{InstalledMod, LocalModpack, ModLoader, ModSource},
};
use quadrant_host::QuadrantHost;
use serde::Serialize;

use crate::{
    Ctx, args, deeplink,
    output::{Report, confirm, sync_date, table},
    provider,
};

/// The modpack `modpack clear` applies, which has no mods.
const FREE_MODPACK: &str = "free";
pub const SHARE_URL: &str = "https://usequadrant.dev/modpack/";
const WINDOWS_APPLY_HELP: &str =
    "https://github.com/QuadrantMC/quadrant/wiki/Fixing-Windows-issues";

#[derive(Debug, Subcommand)]
pub enum ModpackCommand {
    /// List modpacks, the applied one first, then the most recently synced.
    List {
        /// Only modpacks whose name, version or loader contains this text.
        #[arg(long)]
        query: Option<String>,
        /// Include the empty "free" modpack that `modpack clear` applies.
        #[arg(long)]
        include_free: bool,
    },
    /// Show a modpack and its mods.
    Show {
        name: String,
        /// List the mods as the modpack file records them, without asking the
        /// providers for details.
        #[arg(long)]
        no_details: bool,
    },
    /// Create an empty modpack.
    Create {
        name: String,
        /// Minecraft version [default: the latest release]
        #[arg(long)]
        version: Option<String>,
        /// Mod loader, such as fabric, forge, neoforge or quilt.
        #[arg(long, value_parser = args::loader)]
        loader: ModLoader,
    },
    /// Rename a modpack or change its Minecraft version or loader.
    Edit {
        name: String,
        #[arg(long, value_name = "NAME")]
        rename: Option<String>,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, value_parser = args::loader)]
        loader: Option<ModLoader>,
    },
    /// Delete a modpack and its files.
    Delete {
        name: String,
        /// Don't ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Make a modpack the one Minecraft loads.
    Apply { name: String },
    /// Apply an empty modpack so Minecraft loads no mods.
    Clear,
    /// Export a modpack as a .quadrantExport.zip archive.
    Export {
        name: String,
        /// Archive path [default: ./<NAME>.quadrantExport.zip]
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// List mods with a newer file for the modpack's version and loader.
    Updates {
        name: String,
        /// Install every available update.
        #[arg(long)]
        apply: bool,
    },
    /// Match files the modpack doesn't track to mods on the providers.
    Identify { name: String },
    /// Record an identified file as a mod of the modpack.
    Register {
        name: String,
        #[arg(long)]
        id: String,
        /// curseforge (cf) or modrinth (mr)
        #[arg(long, value_parser = args::source)]
        source: ModSource,
        #[arg(long)]
        download_url: String,
    },
    /// Share a modpack through Quadrant Share and print its link.
    Share { name: String },
    /// Install a modpack from a Quadrant Share code or link.
    Import {
        /// A 7-digit code or a https://usequadrant.dev/modpack/<code> link.
        code: String,
        /// Install under a different name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Print the modpacks folder.
    Folder {
        /// Open it in the file manager.
        #[arg(long)]
        open: bool,
    },
}

pub async fn run(command: ModpackCommand, ctx: &Ctx) -> Result<Report> {
    let host = &ctx.host;
    match command {
        ModpackCommand::List {
            query,
            include_free,
        } => {
            let modpacks =
                order_modpacks(host.get_modpacks(!include_free).await?, query.as_deref());
            Report::new(&modpacks, |modpacks| {
                let rows: Vec<[String; 5]> = modpacks
                    .iter()
                    .map(|modpack| {
                        [
                            modpack.name.clone(),
                            modpack.version.clone(),
                            modpack.mod_loader.to_string(),
                            mod_count(modpack.mods.len()),
                            modpack_flags(modpack),
                        ]
                    })
                    .collect();
                table(&rows)
            })
        }
        ModpackCommand::Show { name, no_details } => {
            let modpack = find_modpack(host, &name).await?;
            if no_details {
                return Report::new(&modpack, |modpack| {
                    describe_modpack(modpack, &installed_rows(&modpack.mods))
                });
            }
            let mut mods = lookup_mods(ctx, &modpack).await;
            mods.sort_by_key(|mod_| std::cmp::Reverse(mod_.download_count));
            let shown = ModpackWithMods { modpack, mods };
            Report::new(&shown, |shown| {
                let rows = shown
                    .mods
                    .iter()
                    .map(|mod_| {
                        [
                            mod_.name.clone(),
                            mod_.id.clone(),
                            args::source_name(&mod_.source).to_string(),
                            installed_file(&shown.modpack, &mod_.id),
                        ]
                    })
                    .collect::<Vec<_>>();
                describe_modpack(&shown.modpack, &rows)
            })
        }
        ModpackCommand::Create {
            name,
            version,
            loader,
        } => {
            let name = valid_name(&name)?;
            let version = match version {
                Some(version) => version,
                None => latest_version(host).await?,
            };
            host.create_modpack(name.clone(), version.clone(), loader)
                .await?;
            Ok(Report::message(format!(
                "Created {name} ({version}, {loader})."
            )))
        }
        ModpackCommand::Edit {
            name,
            rename,
            version,
            loader,
        } => {
            if rename.is_none() && version.is_none() && loader.is_none() {
                bail!("nothing to change; pass --rename, --version or --loader");
            }
            let rename = rename.as_deref().map(valid_name).transpose()?;
            let new_name = rename.clone().unwrap_or_else(|| name.clone());
            host.update_modpack(name, rename, version, loader).await?;
            Ok(Report::message(format!("Updated {new_name}.")))
        }
        ModpackCommand::Delete { name, yes } => {
            find_modpack(host, &name).await?;
            confirm(&format!("Delete the modpack {name} and its mods?"), yes)?;
            host.delete_modpack(name.clone()).await?;
            Ok(Report::message(format!("Deleted {name}.")))
        }
        ModpackCommand::Apply { name } => {
            if find_modpack(host, &name).await?.is_applied {
                return Ok(Report::message(format!("{name} is already applied.")));
            }
            apply(ctx, &name)?;
            Ok(Report::message(format!("Applied {name}.")))
        }
        ModpackCommand::Clear => {
            // Rebuilt every time, so a "free" pack someone put mods in or
            // pinned to an old version still clears the mods folder.
            let _ = host.delete_modpack(FREE_MODPACK.to_string()).await;
            host.create_modpack(
                FREE_MODPACK.to_string(),
                latest_version(host).await?,
                ModLoader::Unknown,
            )
            .await?;
            apply(ctx, FREE_MODPACK)?;
            Ok(Report::message("Cleared the mods folder."))
        }
        ModpackCommand::Export { name, output } => {
            find_modpack(host, &name).await?;
            let destination = std::path::absolute(
                output.unwrap_or_else(|| PathBuf::from(format!("{name}.quadrantExport.zip"))),
            )?;
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _progress = ctx.out.track_progress(host);
            host.export_modpack_to(name, destination.clone()).await?;
            Report::new(&destination, |destination| {
                format!("Exported to {}", destination.display())
            })
        }
        ModpackCommand::Updates { name, apply } => updates(ctx, &name, apply).await,
        ModpackCommand::Identify { name } => identify(ctx, &name).await,
        ModpackCommand::Register {
            name,
            id,
            source,
            download_url,
        } => {
            host.register_mod(
                InstalledMod::minimal(id.clone(), source, download_url),
                name.clone(),
            )
            .await?;
            Ok(Report::message(format!("Registered {id} in {name}.")))
        }
        ModpackCommand::Share { name } => {
            ctx.require_account_build()?;
            ctx.require_api_key()?;
            let response = host.share_modpack(name).await?;
            let shared = Shared {
                url: format!("{SHARE_URL}{}", response.code),
                code: response.code,
                uses_left: response.uses_left,
            };
            Report::new(&shared, |shared| {
                format!("{}\n{} uses left", shared.url, shared.uses_left)
            })
        }
        ModpackCommand::Import { code, name } => import(ctx, &code, name).await,
        ModpackCommand::Folder { open } => {
            let folder = host.get_modpacks_folder()?;
            if open {
                std::fs::create_dir_all(&folder)?;
                open::that_detached(&folder)?;
            }
            Report::new(&folder, |folder| folder.display().to_string())
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModpackWithMods {
    modpack: LocalModpack,
    mods: Vec<Mod>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Shared {
    url: String,
    code: i32,
    uses_left: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentifiedFile {
    file_name: String,
    candidates: Vec<Candidate>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    id: String,
    source: ModSource,
    download_url: String,
    name: Option<String>,
}

pub async fn find_modpack(host: &QuadrantHost, name: &str) -> Result<LocalModpack> {
    host.get_modpacks(false)
        .await?
        .into_iter()
        .find(|modpack| modpack.name == name)
        .ok_or_else(|| ErrorCode::ModpackMissing.into())
}

pub async fn latest_version(host: &QuadrantHost) -> Result<String> {
    host.get_versions()
        .await?
        .into_iter()
        .next()
        .map(|version| version.version)
        .ok_or_else(|| anyhow!("no Minecraft versions are available"))
}

fn apply(ctx: &Ctx, name: &str) -> Result<()> {
    let result = ctx.host.frontend_apply_modpack(name.to_string());
    if result.is_err() && cfg!(target_os = "windows") {
        ctx.out.note(format!(
            "If applying keeps failing, see {WINDOWS_APPLY_HELP}"
        ));
    }
    result
}

/// Looks every mod of the modpack up on its provider, dropping the ones that
/// can't be found, as `ModpackView` does.
async fn lookup_mods(ctx: &Ctx, modpack: &LocalModpack) -> Vec<Mod> {
    let lookups = modpack.mods.iter().map(|mod_| {
        provider::get_mod(
            &ctx.host,
            &mod_.source,
            provider::modpack_entry(&mod_.id, modpack),
        )
    });
    let mut mods = Vec::new();
    for (installed, result) in modpack.mods.iter().zip(join_all(lookups).await) {
        match result {
            Ok(mod_) => mods.push(mod_),
            Err(error) => ctx.out.note(format!(
                "Couldn't look up {} ({}): {}",
                installed.name,
                installed.id,
                crate::i18n::describe(error)
            )),
        }
    }
    mods
}

async fn updates(ctx: &Ctx, name: &str, apply: bool) -> Result<Report> {
    let host = &ctx.host;
    let modpack = find_modpack(host, name).await?;
    let checks = lookup_mods(ctx, &modpack).await.into_iter().map(|mod_| {
        host.check_mod_updates(
            mod_,
            modpack.version.clone(),
            modpack.mod_loader,
            modpack.name.clone(),
        )
    });
    let mut updates = Vec::new();
    for result in join_all(checks).await {
        match result {
            Ok(Some(update)) => updates.push(update),
            Ok(None) => {}
            Err(error) => ctx.out.note(format!(
                "Couldn't check a mod for updates: {}",
                crate::i18n::describe(error)
            )),
        }
    }

    if apply {
        let _progress = ctx.out.track_progress(host);
        for update in &updates {
            let (true, Some(file)) = (update.downloadable, update.new_version.clone()) else {
                continue;
            };
            host.install_remote_file(
                file,
                update.mod_type,
                Some(modpack.name.clone()),
                update.source.clone(),
                update.id.clone(),
            )
            .await?;
        }
    }

    Report::new(&updates, |updates| {
        let rows: Vec<[String; 3]> = updates
            .iter()
            .map(|update| {
                let new_file = match (&update.new_version, update.downloadable) {
                    (Some(file), true) if apply => format!("updated to {}", file.file_name),
                    (Some(file), true) => format!("-> {}", file.file_name),
                    _ => "up to date".to_string(),
                };
                [
                    update.name.clone(),
                    installed_file(&modpack, &update.id),
                    new_file,
                ]
            })
            .collect();
        if rows.is_empty() {
            "Every mod is up to date.".to_string()
        } else {
            table(&rows)
        }
    })
}

async fn identify(ctx: &Ctx, name: &str) -> Result<Report> {
    let host = &ctx.host;
    let modpack = find_modpack(host, name).await?;
    let mut by_file: BTreeMap<String, Vec<InstalledMod>> = BTreeMap::new();
    for identified in host.identify_modpack(name.to_string()).await? {
        by_file
            .entry(identified.file_name)
            .or_default()
            .push(identified.installed_mod);
    }

    let mut files = Vec::new();
    for (file_name, installed) in by_file {
        let lookups = installed.iter().map(|candidate| {
            provider::get_mod(
                host,
                &candidate.source,
                GetModArgs {
                    selectable: true,
                    select_url: Some(candidate.download_url.clone()),
                    deletable: false,
                    ..provider::modpack_entry(&candidate.id, &modpack)
                },
            )
        });
        let candidates = installed
            .iter()
            .zip(join_all(lookups).await)
            .map(|(candidate, details)| Candidate {
                id: candidate.id.clone(),
                source: candidate.source.clone(),
                download_url: candidate.download_url.clone(),
                name: details.ok().map(|details| details.name),
            })
            .collect();
        files.push(IdentifiedFile {
            file_name,
            candidates,
        });
    }

    Report::new(&files, |files| {
        if files.is_empty() {
            return "No files could be identified.".to_string();
        }
        files
            .iter()
            .map(|file| {
                let candidates = file
                    .candidates
                    .iter()
                    .map(|candidate| {
                        format!(
                            "  {} ({} {})\n    quadrant-cli modpack register {:?} --id {} --source {} --download-url {}",
                            candidate.name.as_deref().unwrap_or("unknown mod"),
                            args::source_name(&candidate.source),
                            candidate.id,
                            name,
                            candidate.id,
                            args::source_name(&candidate.source),
                            candidate.download_url
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}\n{candidates}", file.file_name)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}

pub async fn import(ctx: &Ctx, code: &str, name: Option<String>) -> Result<Report> {
    let code = deeplink::parse_share_code(code)
        .ok_or_else(|| anyhow!("expected a 7-digit share code or a {SHARE_URL}<code> link"))?;
    ctx.require_api_key()?;
    let mut modpack = ctx.host.get_quadrant_share_modpack(code).await?;
    if let Some(name) = name {
        modpack.name = valid_name(&name)?;
    }
    let installed = modpack.name.clone();
    let _progress = ctx.out.track_progress(&ctx.host);
    ctx.host.install_modpack(modpack).await?;
    Ok(Report::message(format!("Installed {installed}.")))
}

fn describe_modpack<const N: usize>(modpack: &LocalModpack, rows: &[[String; N]]) -> String {
    let flags = modpack_flags(modpack);
    let mut text = format!(
        "{}  {}  {}{}{flags}\n{}",
        modpack.name,
        modpack.version,
        modpack.mod_loader,
        if flags.is_empty() { "" } else { "  " },
        mod_count(modpack.mods.len())
    );
    if !rows.is_empty() {
        text.push_str("\n\n");
        text.push_str(&table(rows));
    }
    text
}

fn mod_count(count: usize) -> String {
    if count == 1 {
        "1 mod".to_string()
    } else {
        format!("{count} mods")
    }
}

fn installed_rows(mods: &[InstalledMod]) -> Vec<[String; 4]> {
    mods.iter()
        .map(|mod_| {
            [
                mod_.name.clone(),
                mod_.id.clone(),
                args::source_name(&mod_.source).to_string(),
                file_name(&mod_.download_url),
            ]
        })
        .collect()
}

/// The file a modpack entry was installed from, which says more than the
/// provider's version label (a date on CurseForge, an opaque id on Modrinth).
fn installed_file(modpack: &LocalModpack, id: &str) -> String {
    modpack
        .mods
        .iter()
        .find(|mod_| mod_.id == id)
        .map(|mod_| file_name(&mod_.download_url))
        .unwrap_or_default()
}

fn file_name(download_url: &str) -> String {
    let encoded = download_url.rsplit('/').next().unwrap_or(download_url);
    urlencoding::decode(encoded)
        .map(|name| name.into_owned())
        .unwrap_or_else(|_| encoded.to_string())
}

fn modpack_flags(modpack: &LocalModpack) -> String {
    let mut flags = Vec::new();
    if modpack.is_applied {
        flags.push("applied".to_string());
    }
    if modpack.last_synced != 0 {
        flags.push(format!("synced {}", sync_date(modpack.last_synced)));
    }
    if modpack.unknown_mods {
        flags.push("untracked files".to_string());
    }
    flags.join(", ")
}

/// `getModpacks` in `src/tools.ts`: most recently synced first, the applied
/// modpack ahead of all, then an optional case-insensitive filter on name,
/// version and loader.
pub fn order_modpacks(mut modpacks: Vec<LocalModpack>, query: Option<&str>) -> Vec<LocalModpack> {
    modpacks.sort_by_key(|modpack| std::cmp::Reverse(modpack.last_synced));
    modpacks.sort_by_key(|modpack| !modpack.is_applied);
    let Some(query) = query
        .map(str::to_lowercase)
        .filter(|query| !query.is_empty())
    else {
        return modpacks;
    };
    modpacks
        .into_iter()
        .filter(|modpack| {
            [
                modpack.name.to_lowercase(),
                modpack.version.to_lowercase(),
                modpack.mod_loader.to_string().to_lowercase(),
            ]
            .iter()
            .any(|field| field.contains(&query))
        })
        .collect()
}

/// Strips the characters the create dialog refuses, since they can't appear
/// in a folder name on every platform.
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|character| !r#"<>:"/\|?*"#.contains(*character))
        .collect()
}

fn valid_name(name: &str) -> Result<String> {
    let sanitized = sanitize_name(name);
    if sanitized.trim().is_empty() {
        bail!(ErrorCode::InvalidModpackName);
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modpack(name: &str, last_synced: i64, is_applied: bool) -> LocalModpack {
        LocalModpack {
            name: name.to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: Vec::new(),
            unknown_mods: false,
            is_applied,
            last_synced,
            modpack_id: None,
        }
    }

    fn names(modpacks: &[LocalModpack]) -> Vec<&str> {
        modpacks
            .iter()
            .map(|modpack| modpack.name.as_str())
            .collect()
    }

    #[test]
    fn orders_applied_first_then_newest_sync() {
        let ordered = order_modpacks(
            vec![
                modpack("old", 10, false),
                modpack("applied", 0, true),
                modpack("new", 30, false),
                modpack("never", 0, false),
            ],
            None,
        );
        assert_eq!(names(&ordered), ["applied", "new", "old", "never"]);
    }

    #[test]
    fn filters_by_name_version_or_loader_ignoring_case() {
        let mut forge = modpack("Tech", 0, false);
        forge.mod_loader = ModLoader::Forge;
        forge.version = "1.12.2".to_string();
        let packs = vec![modpack("Skyblock", 0, false), forge];
        assert_eq!(
            names(&order_modpacks(packs.clone(), Some("SKY"))),
            ["Skyblock"]
        );
        assert_eq!(
            names(&order_modpacks(packs.clone(), Some("forge"))),
            ["Tech"]
        );
        assert_eq!(
            names(&order_modpacks(packs.clone(), Some("1.12"))),
            ["Tech"]
        );
        assert_eq!(order_modpacks(packs, Some("")).len(), 2);
    }

    #[test]
    fn sanitize_strips_reserved_characters() {
        assert_eq!(sanitize_name(r#"My<Pack>: "1/2" \ | ? *"#), "MyPack 12    ");
        assert!(valid_name("<>").is_err());
        assert_eq!(valid_name("a/b").unwrap(), "ab");
    }
}
