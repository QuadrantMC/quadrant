use anyhow::{Result, bail};
use clap::Subcommand;

use crate::{
    Ctx, config,
    output::{Report, table},
};

#[derive(Debug, Subcommand)]
pub enum PrismCommand {
    /// List Prism Launcher instances (needs experimentalFeatures on).
    List,
    /// Show what applying a modpack would change about each instance.
    Plan { modpack: String },
    /// Link an instance to a modpack and match its version and loader.
    Apply { modpack: String, instance: String },
    /// Give an instance its own mods folder and components back.
    Detach { instance: String },
}

const DISABLED: &str = "Prism Launcher support is experimental; turn it on with `quadrantmc settings set experimentalFeatures true`.";

pub async fn run(command: PrismCommand, ctx: &Ctx) -> Result<Report> {
    let host = &ctx.host;
    // The host reports a disabled feature as a refused request, which reads
    // like a sign-in problem, so say what is actually off.
    if !config::get_bool(host, "experimentalFeatures")?.unwrap_or(false) {
        match command {
            PrismCommand::List | PrismCommand::Plan { .. } => ctx.out.note(DISABLED),
            PrismCommand::Apply { .. } | PrismCommand::Detach { .. } => bail!(DISABLED),
        }
    }
    match command {
        PrismCommand::List => {
            let instances = host.get_prism_instances()?;
            Report::new(&instances, |instances| {
                let rows: Vec<[String; 5]> = instances
                    .iter()
                    .map(|instance| {
                        [
                            instance.id.clone(),
                            instance.name.clone(),
                            instance.minecraft_version.clone().unwrap_or_default(),
                            instance.mod_loader.to_string(),
                            instance
                                .applied_modpack
                                .as_ref()
                                .map(|modpack| format!("-> {modpack}"))
                                .unwrap_or_default(),
                        ]
                    })
                    .collect();
                table(&rows)
            })
        }
        PrismCommand::Plan { modpack } => {
            let plans = host.get_prism_sync_plans(modpack).await?;
            Report::new(&plans, |plans| {
                let rows: Vec<[String; 3]> = plans
                    .iter()
                    .map(|plan| {
                        [
                            plan.instance_id.clone(),
                            plan.minecraft_version
                                .as_ref()
                                .map(|version| format!("Minecraft -> {version}"))
                                .unwrap_or_else(|| "Minecraft unchanged".to_string()),
                            plan.mod_loader
                                .map(|loader| format!("loader -> {loader}"))
                                .unwrap_or_else(|| "loader unchanged".to_string()),
                        ]
                    })
                    .collect();
                table(&rows)
            })
        }
        PrismCommand::Apply { modpack, instance } => {
            host.apply_modpack_to_prism_instance(modpack.clone(), instance.clone())
                .await?;
            Ok(Report::message(format!("Linked {instance} to {modpack}.")))
        }
        PrismCommand::Detach { instance } => {
            host.detach_prism_instance(instance.clone())?;
            Ok(Report::message(format!("Detached {instance}.")))
        }
    }
}
