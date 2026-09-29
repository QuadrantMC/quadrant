use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use quadrant_core::account::quadrant_settings_sync::SettingsPull;
use serde_json::Value;

use crate::{Ctx, config, output::Report};

const COLLECT_USER_DATA: &str = "collectUserData";
/// Settings naming a folder Quadrant reads and writes in.
const FOLDER_KEYS: &[&str] = &["mcFolder", "prismLauncherFolder"];

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
            if key == COLLECT_USER_DATA {
                // Checked before the write, so a build that can't send or
                // withdraw telemetry leaves the setting as it was.
                ctx.require_api_key()?;
            }
            let value = if FOLDER_KEYS.contains(&key.as_str()) {
                folder_value(&folder_setting(Path::new(&value))?)
            } else {
                config::parse_value(&key, &value, host.get_config_value(&key)?.as_ref())
            };
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
                path.as_deref().map(folder_setting).transpose()?
            };
            if let Some(folder) = new_folder {
                config::set(host, "mcFolder", folder_value(&folder))?;
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

/// A folder setting has to name a folder that exists. It is stored absolute,
/// so it means the same whatever folder the app or the CLI runs from.
fn folder_setting(raw: &Path) -> Result<PathBuf> {
    if raw.to_string_lossy().trim().is_empty() {
        bail!("the folder path is empty");
    }
    let folder = std::path::absolute(raw)?;
    if !folder.is_dir() {
        bail!("{} is not an existing folder", folder.display());
    }
    Ok(folder)
}

fn folder_value(folder: &Path) -> Value {
    Value::String(folder.to_string_lossy().into_owned())
}

/// Strings print bare, everything else as compact JSON.
fn display_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "(unset)".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_settings_must_name_an_existing_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(folder_setting(dir.path()).unwrap(), dir.path());
        assert!(folder_setting(Path::new(".")).unwrap().is_absolute());

        let file = dir.path().join("file.txt");
        std::fs::write(&file, "").unwrap();
        for bad in [
            Path::new(""),
            Path::new("  "),
            &file,
            &dir.path().join("missing"),
        ] {
            assert!(folder_setting(bad).is_err(), "{}", bad.display());
        }
    }

    #[tokio::test]
    async fn collect_user_data_stays_unchanged_without_an_api_key() {
        let dir = tempfile::tempdir().unwrap();
        let options = quadrant_host::QuadrantHostOptions::new(dir.path().to_path_buf(), "", "", "");
        let ctx = Ctx {
            host: quadrant_host::QuadrantHost::new(options).unwrap(),
            out: crate::output::Output {
                json: false,
                quiet: true,
            },
        };
        let before = ctx.host.get_config_value(COLLECT_USER_DATA).unwrap();
        let flipped = before.as_ref().and_then(Value::as_bool) != Some(true);
        let set = SettingsCommand::Set {
            key: COLLECT_USER_DATA.to_string(),
            value: flipped.to_string(),
        };
        let error = run(set, &ctx).await.err().unwrap();
        assert!(error.to_string().contains("QUADRANT_API_KEY"), "{error}");
        assert_eq!(
            ctx.host.get_config_value(COLLECT_USER_DATA).unwrap(),
            before
        );
    }
}
