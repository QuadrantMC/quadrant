mod args;
mod config;
mod content;
mod deeplink;
mod i18n;
mod misc;
mod modpack;
mod mods;
mod output;
mod prism;
mod provider;
mod search;
mod settings;

use std::{path::PathBuf, process::ExitCode};

use anyhow::{Context, Result, anyhow, bail};
use clap::{ArgAction, Parser, Subcommand};
use quadrant_host::{QuadrantHost, QuadrantHostOptions};

use crate::output::{Output, Report};

/// The desktop app's Tauri identifier, whose data folder the CLI shares.
const APP_IDENTIFIER: &str = "dev.mrquantumoff.mcmodpackmanager";

#[derive(Debug, Parser)]
#[command(
    name = "quadrant-cli",
    version,
    about = "Manage Minecraft mods and modpacks with Quadrant",
    after_help = "Shares settings, modpacks and the Quadrant ID login with the desktop app."
)]
struct Cli {
    /// Print the result as JSON on stdout.
    #[arg(long, global = true)]
    json: bool,
    /// Hide progress and notes on stderr.
    #[arg(long, short, global = true)]
    quiet: bool,
    /// Show backend logs on stderr; repeat for more detail.
    #[arg(long, short, global = true, action = ArgAction::Count)]
    verbose: u8,
    /// Quadrant data folder [default: the desktop app's]
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,
    /// Quadrant API base URL.
    #[arg(long, global = true, env = "QUADRANT_API_BASE_URL", value_name = "URL")]
    api_url: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the Minecraft release versions mods can target.
    Versions,
    /// Show the Quadrant news feed.
    News,
    /// Create, apply, export and update modpacks.
    #[command(subcommand)]
    Modpack(modpack::ModpackCommand),
    /// Search, inspect, install and update mods and packs.
    #[command(name = "mod", subcommand)]
    Mod(mods::ModCommand),
    /// Manage installed resource packs and shader packs.
    #[command(subcommand)]
    Content(content::ContentCommand),
    /// Link modpacks to Prism Launcher instances.
    #[command(subcommand)]
    Prism(prism::PrismCommand),
    /// Read and change Quadrant settings.
    #[command(subcommand)]
    Settings(settings::SettingsCommand),
    /// Inspect or send usage telemetry.
    #[command(subcommand)]
    Telemetry(misc::TelemetryCommand),
    /// Call a host command directly with a JSON payload.
    Invoke {
        /// Host command name, as listed by --list.
        command: Option<String>,
        /// JSON payload with camelCase fields.
        payload: Option<String>,
        /// List the host commands.
        #[arg(long, conflicts_with = "command")]
        list: bool,
    },
}

/// What every command handler gets: the host and where its output goes.
pub struct Ctx {
    pub host: QuadrantHost,
    pub out: Output,
}

impl Ctx {
    /// Quadrant ID commands need the OAuth client baked in at build time.
    pub fn require_account_build(&self) -> Result<()> {
        let options = self.host.options();
        if options.oauth_client_id.is_empty() || options.oauth_client_secret.is_empty() {
            bail!(
                "this build has no Quadrant ID credentials; rebuild with QUADRANT_OAUTH2_CLIENT_ID and QUADRANT_OAUTH2_CLIENT_SECRET set"
            );
        }
        Ok(())
    }

    /// Quadrant Share and telemetry need the API key baked in at build time.
    pub fn require_api_key(&self) -> Result<()> {
        if self.host.options().quadrant_api_key.is_empty() {
            bail!("this build has no Quadrant API key; rebuild with QUADRANT_API_KEY set");
        }
        Ok(())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    quiet_backend_logs(cli.verbose);
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start the async runtime")
        .and_then(|runtime| runtime.block_on(run(cli)));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {}", i18n::describe(error));
            ExitCode::FAILURE
        }
    }
}

/// The host logs at info level to stderr, which would bury command output;
/// `RUST_LOG` still wins when set.
fn quiet_backend_logs(verbose: u8) {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }
    let level = match verbose {
        0 => "error",
        1 => "warn",
        2 => "info",
        _ => "debug",
    };
    // Runs before the async runtime starts any threads.
    unsafe { std::env::set_var("RUST_LOG", level) };
}

async fn run(cli: Cli) -> Result<()> {
    let out = Output {
        json: cli.json,
        quiet: cli.quiet,
    };
    let ctx = Ctx {
        host: build_host(cli.data_dir, cli.api_url)?,
        out,
    };
    let report = dispatch(cli.command, &ctx).await?;
    out.print(report)
}

async fn dispatch(command: Command, ctx: &Ctx) -> Result<Report> {
    match command {
        Command::Versions => misc::versions(ctx).await,
        Command::News => misc::news(ctx).await,
        Command::Modpack(command) => modpack::run(command, ctx).await,
        Command::Mod(command) => mods::run(command, ctx).await,
        Command::Content(command) => content::run(command, ctx).await,
        Command::Prism(command) => prism::run(command, ctx).await,
        Command::Settings(command) => settings::run(command, ctx).await,
        Command::Telemetry(command) => misc::telemetry(command, ctx).await,
        Command::Invoke {
            command,
            payload,
            list,
        } => misc::invoke(command, payload, list, ctx).await,
    }
}

fn build_host(data_dir: Option<PathBuf>, api_url: Option<String>) -> Result<QuadrantHost> {
    let data_dir = match data_dir {
        Some(data_dir) => data_dir,
        None => dirs::data_dir()
            .ok_or_else(|| anyhow!("no data folder on this system; pass --data-dir"))?
            .join(APP_IDENTIFIER),
    };
    let mut options = QuadrantHostOptions::new(
        data_dir,
        option_env!("QUADRANT_OAUTH2_CLIENT_ID").unwrap_or_default(),
        option_env!("QUADRANT_OAUTH2_CLIENT_SECRET").unwrap_or_default(),
        option_env!("QUADRANT_API_KEY").unwrap_or_default(),
    );
    options.api_base_url = api_url;
    let host = QuadrantHost::new(options)?;
    host.init_config()?;
    Ok(host)
}
