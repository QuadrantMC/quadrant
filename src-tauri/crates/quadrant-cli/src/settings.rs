use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use quadrant_core::account::quadrant_settings_sync::SettingsPull;
use serde_json::Value;

use crate::{Ctx, config, output::Report};

const COLLECT_USER_DATA: &str = "collectUserData";

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// List every stored setting.
    List,
    /// Print one setting.
    Get { key: String },
    /// Store a setting. JSON literals (true, 3, {"a":1}) are stored as JSON,
    /// anything else as text; a setting that holds text keeps holding text.
    Set { key: String, value: String },
    /// Remove a setting so its default applies again.
    Unset { key: String },
    /// Print the Minecraft folder, or change or reset it.
    McFolder {
        path: Option<PathBuf>,
        /// Go back to the platform's default Minecraft folder.
        #[arg(long, conflicts_with = "path")]
        reset: bool,
    },
    /// Upload synced settings to Quadrant ID.
    Push,
    /// Download synced settings from Quadrant ID when the cloud copy is newer.
    Pull,
}

pub async fn run(command: SettingsCommand, ctx: &Ctx) -> Result<Report> {
    let host = &ctx.host;
    match command {
        SettingsCommand::List => {
            let options = host.options();
            let path = options.data_dir.join(&options.config_store_name);
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let settings: BTreeMap<String, Value> = serde_json::from_str(&raw)?;
            Report::new(&settings, |settings| {
                settings
                    .iter()
                    .map(|(key, value)| format!("{key} = {}", display_value(value)))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        SettingsCommand::Get { key } => {
            let value = host.get_config_value(&key)?.unwrap_or(Value::Null);
            Report::new(&value, display_value)
        }
        SettingsCommand::Set { key, value } => {
            let value = config::parse_value(&key, &value, host.get_config_value(&key)?.as_ref());
            config::set(host, &key, value.clone())?;
            if key == COLLECT_USER_DATA {
                // Settings.tsx sends or withdraws telemetry the moment the
                // toggle flips, not on the next launch.
                match value {
                    Value::Bool(true) => host.send_telemetry().await?,
                    Value::Bool(false) => host.remove_telemetry().await?,
                    _ => {}
                }
            }
            Ok(Report::message(format!(
                "{key} = {}",
                display_value(&value)
            )))
        }
        SettingsCommand::Unset { key } => {
            config::unset(host, &key)?;
            Ok(Report::message(format!("Removed {key}.")))
        }
        SettingsCommand::McFolder { path, reset } => {
            let new_folder = if reset {
                Some(
                    quadrant_core::config::get_mc_folder()?
                        .ok_or_else(|| anyhow!("no default Minecraft folder on this system"))?,
                )
            } else {
                path.map(std::path::absolute).transpose()?
            };
            if let Some(folder) = new_folder {
                config::set(
                    host,
                    "mcFolder",
                    Value::String(folder.to_string_lossy().to_string()),
                )?;
            }
            let folder = host.get_minecraft_folder()?;
            Report::new(&folder, |folder| folder.display().to_string())
        }
        SettingsCommand::Push => {
            ctx.require_account_build()?;
            host.submit_quadrant_settings().await?;
            Ok(Report::message("Uploaded settings."))
        }
        SettingsCommand::Pull => {
            ctx.require_account_build()?;
            let (outcome, message) = match host.get_quadrant_settings().await? {
                SettingsPull::UpToDate => ("upToDate", "Settings are already up to date."),
                SettingsPull::Applied => ("applied", "Applied the cloud settings."),
                SettingsPull::LocalNewer => (
                    "localNewer",
                    "The settings on this computer are newer; nothing was pulled.",
                ),
                SettingsPull::NoRemote => ("noRemote", "This account has no cloud settings yet."),
            };
            Report::new(&outcome, |_| message.to_string())
        }
    }
}

/// Strings print bare, everything else as compact JSON.
fn display_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "(unset)".to_string(),
        other => other.to_string(),
    }
}
