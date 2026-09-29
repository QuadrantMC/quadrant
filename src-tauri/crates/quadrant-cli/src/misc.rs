use anyhow::{Result, bail};
use clap::Subcommand;
use quadrant_host::QuadrantHost;
use serde_json::Value;

use crate::{
    Ctx,
    output::{Report, table},
};

pub async fn versions(ctx: &Ctx) -> Result<Report> {
    let versions = ctx.host.get_versions().await?;
    Report::new(&versions, |versions| {
        versions
            .iter()
            .map(|version| version.version.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    })
}

pub async fn news(ctx: &Ctx) -> Result<Report> {
    let articles = ctx.host.get_news().await?;
    Report::new(&articles, |articles| {
        articles
            .iter()
            .map(|article| {
                let marker = if article.new { " (new)" } else { "" };
                format!(
                    "{}{marker}\n{}  {}\n{}",
                    article.title,
                    article.date.format("%Y-%m-%d"),
                    article.link,
                    strip_html(&article.summary)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}

/// Article summaries are HTML; the desktop app renders their text only.
pub fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Subcommand)]
pub enum TelemetryCommand {
    /// Show the telemetry payload this installation would send.
    Info,
    /// Send telemetry now, if `collectUserData` is on.
    Send,
    /// Ask the server to delete this installation's telemetry.
    Remove,
}

pub async fn telemetry(command: TelemetryCommand, ctx: &Ctx) -> Result<Report> {
    match command {
        TelemetryCommand::Info => {
            let info = ctx.host.get_telemetry_info().await?;
            let hardware_id = info.hardware_id.clone();
            Report::new(&info, |info| {
                table(&[
                    ["version".to_string(), info.version.clone()],
                    ["os".to_string(), info.os.clone()],
                    ["country".to_string(), info.country.clone()],
                    ["modrinthUsage".to_string(), info.modrinth_usage.to_string()],
                    [
                        "curseforgeUsage".to_string(),
                        info.curseforge_usage.to_string(),
                    ],
                    ["hardwareId".to_string(), hardware_id.clone()],
                    [
                        "dashboard".to_string(),
                        format!(
                            "https://mrquantumoff.dev/projects/quadrant/analytics/{hardware_id}"
                        ),
                    ],
                ])
            })
        }
        TelemetryCommand::Send => {
            ctx.require_api_key()?;
            ctx.host.send_telemetry().await?;
            Ok(Report::message("Telemetry sent."))
        }
        TelemetryCommand::Remove => {
            ctx.require_api_key()?;
            ctx.host.remove_telemetry().await?;
            Ok(Report::message("Telemetry removed."))
        }
    }
}

pub async fn invoke(
    command: Option<String>,
    payload: Option<String>,
    list: bool,
    ctx: &Ctx,
) -> Result<Report> {
    if list {
        let commands = QuadrantHost::supported_commands();
        return Report::new(&commands, |commands| commands.join("\n"));
    }
    let Some(command) = command else {
        bail!("name a host command, or pass --list to see them");
    };
    let payload = match payload {
        Some(raw) => serde_json::from_str(&raw)
            .map_err(|error| anyhow::anyhow!("the payload is not valid JSON: {error}"))?,
        None => Value::Null,
    };
    let result = ctx.host.invoke(&command, payload).await?;
    Report::new(&result, |result| match result {
        Value::Null => String::new(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_keeps_text_and_decodes_entities() {
        assert_eq!(
            strip_html("<p>Hello&nbsp;<b>world</b> &amp; friends</p>\n<p>Bye</p>"),
            "Hello world & friends Bye"
        );
    }
}
