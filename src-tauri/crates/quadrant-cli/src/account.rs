use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use serde_json::Value;

use crate::{
    Ctx, config, oauth,
    output::{Report, table},
};

const ACCOUNT_URL: &str = "https://mrquantumoff.dev/account";
const REGISTER_URL: &str = "https://mrquantumoff.dev/account/register";
const OAUTH_STATE: &str = "oauthState";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Sign in to Quadrant ID through the browser.
    Login {
        /// Only print the sign-in link.
        #[arg(long)]
        no_browser: bool,
    },
    /// Sign out of Quadrant ID on this computer, the desktop app included.
    Logout,
    /// Show the signed-in account.
    Info,
    /// Open the Quadrant ID account page.
    Open,
    /// Open the Quadrant ID sign-up page.
    Register,
}

pub async fn run(command: AccountCommand, ctx: &Ctx) -> Result<Report> {
    let host = &ctx.host;
    match command {
        AccountCommand::Login { no_browser } => login(ctx, no_browser).await,
        AccountCommand::Logout => {
            host.clear_account_token()?;
            Ok(Report::message("Signed out."))
        }
        AccountCommand::Info => {
            ctx.require_account_build()?;
            let info = host.get_account_info().await?;
            let unread = info
                .notifications
                .iter()
                .filter(|notification| !notification.read)
                .count();
            Report::new(&info, |info| {
                table(&[
                    ["name".to_string(), info.name.clone()],
                    ["login".to_string(), info.login.clone()],
                    ["email".to_string(), info.email.clone()],
                    [
                        "sync limit".to_string(),
                        info.quadrant_sync_limit.to_string(),
                    ],
                    [
                        "share limit".to_string(),
                        info.quadrant_share_limit.to_string(),
                    ],
                    ["unread notifications".to_string(), unread.to_string()],
                ])
            })
        }
        AccountCommand::Open => Ok(browse(ctx, ACCOUNT_URL)),
        AccountCommand::Register => Ok(browse(ctx, REGISTER_URL)),
    }
}

fn browse(ctx: &Ctx, url: &str) -> Report {
    if let Err(error) = open::that_detached(url) {
        ctx.out.note(format!("Couldn't open a browser: {error}"));
    }
    Report::message(url)
}

async fn login(ctx: &Ctx, no_browser: bool) -> Result<Report> {
    ctx.require_account_build()?;
    let state = oauth::new_state()?;
    config::set(&ctx.host, OAUTH_STATE, Value::String(state.clone()))?;
    let (listener, port) = oauth::bind().await?;
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let url = oauth::authorize_url(&ctx.host.oauth2_client_id(), &redirect_uri, &state);

    // Printed even with --quiet: without a browser this link is the only way in.
    eprintln!("Sign in to Quadrant ID at:\n{url}");
    if !no_browser && let Err(error) = open::that_detached(url.as_str()) {
        ctx.out.note(format!("Couldn't open a browser: {error}"));
    }

    let code = tokio::time::timeout(LOGIN_TIMEOUT, oauth::wait_for_callback(listener, &state))
        .await
        .map_err(|_| anyhow!("the sign-in timed out; run `account login` again"))??;
    finish_login(ctx, code, redirect_uri).await
}

/// Completes a sign-in from a `quadrantnext://login` link, which only counts
/// when its state is the one `account login` stored.
pub async fn login_from_link(
    ctx: &Ctx,
    state: Option<String>,
    code: Option<String>,
    redirect_uri: String,
) -> Result<Report> {
    ctx.require_account_build()?;
    let expected = config::get_string(&ctx.host, OAUTH_STATE)?;
    if state.is_none() || state != expected {
        bail!("this sign-in link doesn't belong to the sign-in in progress");
    }
    let code = code.ok_or_else(|| anyhow!("the sign-in link has no code"))?;
    finish_login(ctx, code, redirect_uri).await
}

async fn finish_login(ctx: &Ctx, code: String, redirect_uri: String) -> Result<Report> {
    ctx.host.oauth2_login(code, redirect_uri).await?;
    let info = ctx.host.get_account_info().await?;
    Report::new(&info, |info| format!("Signed in as {}.", info.name))
}
