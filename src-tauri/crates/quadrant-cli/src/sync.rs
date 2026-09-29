use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use quadrant_core::{
    account::quadrant_sync::SyncedModpack,
    error::is_cloud_sync_conflict,
    models::{InstalledMod, InstalledModpack, LocalModpack, quadrant_version},
};
use serde::Serialize;

use crate::{
    Ctx,
    modpack::{SHARE_URL, confirm_replace, find_modpack},
    output::{Report, confirm, sync_date, table},
};

#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    /// List your Quadrant Sync modpacks and the local copy of each.
    List,
    /// Upload a local modpack to Quadrant Sync.
    Push {
        modpack: String,
        /// Overwrite the cloud copy even when it is newer.
        #[arg(long)]
        force: bool,
    },
    /// Install the cloud copy of a synced modpack, replacing the local one.
    Pull {
        modpack_id: String,
        /// Install under this name [default: the linked local modpack's, or
        /// the cloud name]
        #[arg(long)]
        name: Option<String>,
        /// Replace the local modpack with that name without asking.
        #[arg(long, short)]
        yes: bool,
    },
    /// List who can see and edit a synced modpack.
    Members { modpack_id: String },
    /// Invite someone to a synced modpack.
    Invite {
        modpack_id: String,
        username: String,
        /// Let them manage members too.
        #[arg(long)]
        admin: bool,
    },
    /// Remove someone from a synced modpack.
    Kick {
        modpack_id: String,
        username: String,
    },
    /// Delete a modpack from Quadrant Sync (the local copy stays).
    Delete {
        modpack_id: String,
        /// Don't ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Share the cloud copy of a synced modpack through Quadrant Share.
    Share { modpack_id: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    #[serde(flatten)]
    synced: SyncedModpack,
    local_name: Option<String>,
}

pub async fn run(command: SyncCommand, ctx: &Ctx) -> Result<Report> {
    ctx.require_account_build()?;
    let host = &ctx.host;
    match command {
        SyncCommand::List => {
            let locals = host.get_modpacks(true).await?;
            let listed: Vec<Listed> = host
                .get_synced_modpacks(true, None)
                .await?
                .into_iter()
                .map(|synced| Listed {
                    local_name: local_copy(&synced, &locals).map(|local| local.name.clone()),
                    synced,
                })
                .collect();
            Report::new(&listed, |listed| {
                let rows: Vec<[String; 5]> = listed
                    .iter()
                    .map(|entry| {
                        [
                            entry.synced.name.clone(),
                            entry.synced.modpack_id.clone(),
                            format!(
                                "{} {}",
                                entry.synced.minecraft_version, entry.synced.mod_loader
                            ),
                            sync_date(entry.synced.last_synced),
                            match &entry.local_name {
                                Some(name) => format!("local: {name}"),
                                None => "cloud only".to_string(),
                            },
                        ]
                    })
                    .collect();
                table(&rows)
            })
        }
        SyncCommand::Push { modpack, force } => {
            let local = find_modpack(host, &modpack).await?;
            match host.sync_modpack(local, force).await {
                Err(error) if !force && is_cloud_sync_conflict(&error) => bail!(
                    "The cloud copy of {modpack} is newer. Pull it with `sync pull`, or pass --force to overwrite it."
                ),
                result => result?,
            }
            Ok(Report::message(format!("Pushed {modpack}.")))
        }
        SyncCommand::Pull {
            modpack_id,
            name,
            yes,
        } => {
            let synced = synced_modpack(ctx, &modpack_id, false).await?;
            let name = match name {
                Some(name) => name,
                None => local_copy(&synced, &host.get_modpacks(true).await?)
                    .map(|local| local.name.clone())
                    .unwrap_or_else(|| synced.name.clone()),
            };
            confirm_replace(host, &name, yes)?;
            let last_synced = u64::try_from(synced.last_synced).unwrap_or_default();
            let modpack = to_installed(synced, name.clone())?;
            let _progress = ctx.out.track_progress(host);
            host.install_modpack(modpack).await?;
            host.set_modpack_sync_date(last_synced, name.clone(), Some(modpack_id))?;
            Ok(Report::message(format!("Pulled {name}.")))
        }
        SyncCommand::Members { modpack_id } => {
            let synced = synced_modpack(ctx, &modpack_id, true).await?;
            Report::new(&synced.owners, |owners| {
                let rows: Vec<[String; 2]> = owners
                    .iter()
                    .map(|owner| {
                        [
                            owner.username.clone(),
                            if owner.admin { "admin" } else { "member" }.to_string(),
                        ]
                    })
                    .collect();
                table(&rows)
            })
        }
        SyncCommand::Invite {
            modpack_id,
            username,
            admin,
        } => {
            host.invite_member(modpack_id, username.clone(), admin)
                .await?;
            Ok(Report::message(format!("Invited {username}.")))
        }
        SyncCommand::Kick {
            modpack_id,
            username,
        } => {
            host.kick_member(modpack_id, username.clone()).await?;
            Ok(Report::message(format!("Removed {username}.")))
        }
        SyncCommand::Delete { modpack_id, yes } => {
            confirm(
                &format!("Delete {modpack_id} from Quadrant Sync for every member?"),
                yes,
            )?;
            host.delete_synced_modpack(modpack_id.clone()).await?;
            Ok(Report::message(format!("Deleted {modpack_id}.")))
        }
        SyncCommand::Share { modpack_id } => {
            ctx.require_api_key()?;
            let synced = synced_modpack(ctx, &modpack_id, false).await?;
            let name = synced.name.clone();
            let response = host.share_modpack_raw(to_installed(synced, name)?).await?;
            let url = format!("{SHARE_URL}{}", response.code);
            Report::new(&url, |url| {
                format!("{url}\n{} uses left", response.uses_left)
            })
        }
    }
}

async fn synced_modpack(ctx: &Ctx, modpack_id: &str, show_owners: bool) -> Result<SyncedModpack> {
    ctx.host
        .get_synced_modpacks(show_owners, Some(modpack_id.to_string()))
        .await?
        .into_iter()
        .find(|synced| synced.modpack_id == modpack_id)
        .ok_or_else(|| anyhow!("no synced modpack {modpack_id}; `sync list` shows yours"))
}

fn to_installed(synced: SyncedModpack, name: String) -> Result<InstalledModpack> {
    let mods: Vec<InstalledMod> = serde_json::from_str(&synced.mods)
        .map_err(|error| anyhow!("the cloud copy's mod list is unreadable: {error}"))?;
    Ok(InstalledModpack {
        mod_config_version: "2".to_string(),
        quadrant_version: quadrant_version(),
        name,
        version: synced.minecraft_version,
        mod_loader: synced.mod_loader,
        mods,
    })
}

/// The local modpack a synced one belongs to (`mergeModpacks.ts`): the one
/// recording its id, else one recording no id under the same name, as long
/// as no other local modpack already claims that id.
pub fn local_copy<'a>(
    synced: &SyncedModpack,
    locals: &'a [LocalModpack],
) -> Option<&'a LocalModpack> {
    let id = synced.modpack_id.as_str();
    if let Some(linked) = locals
        .iter()
        .find(|local| local.modpack_id.as_deref() == Some(id))
    {
        return Some(linked);
    }
    locals
        .iter()
        .find(|local| local.modpack_id.is_none() && local.name == synced.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quadrant_core::models::ModLoader;

    fn synced(id: &str, name: &str) -> SyncedModpack {
        SyncedModpack {
            name: name.to_string(),
            minecraft_version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: "[]".to_string(),
            owners: Vec::new(),
            last_synced: 1_700_000_000,
            modpack_id: id.to_string(),
        }
    }

    fn local(name: &str, id: Option<&str>) -> LocalModpack {
        LocalModpack {
            name: name.to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: Vec::new(),
            unknown_mods: false,
            is_applied: false,
            last_synced: 0,
            modpack_id: id.map(str::to_string),
        }
    }

    #[test]
    fn local_copies_pair_by_id_before_name() {
        let locals = [local("Renamed", Some("id-1")), local("Pack", None)];
        assert_eq!(
            local_copy(&synced("id-1", "Pack"), &locals).map(|local| local.name.as_str()),
            Some("Renamed")
        );
        assert_eq!(
            local_copy(&synced("id-2", "Pack"), &locals).map(|local| local.name.as_str()),
            Some("Pack")
        );
        assert!(local_copy(&synced("id-3", "Other"), &locals).is_none());
    }

    #[test]
    fn a_name_match_never_steals_a_modpack_linked_elsewhere() {
        let locals = [local("Pack", Some("id-9"))];
        assert!(local_copy(&synced("id-1", "Pack"), &locals).is_none());
    }

    #[test]
    fn cloud_copies_become_installable_modpacks() {
        let mut cloud = synced("id-1", "Pack");
        cloud.mods = r#"[{"id":"sodium","source":"ModSource.modRinth","downloadUrl":"https://x/sodium.jar"}]"#.to_string();
        let modpack = to_installed(cloud, "Local".to_string()).unwrap();
        assert_eq!(modpack.name, "Local");
        assert_eq!(modpack.mods.len(), 1);
        assert_eq!(modpack.mods[0].id, "sodium");
    }
}
