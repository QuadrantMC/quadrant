use std::collections::HashSet;

use anyhow::{Result, anyhow};
use clap::Subcommand;
use quadrant_core::account::id::{AccountInfo, Notification};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use crate::{Ctx, config, output::Report};

const MODPACK_SYNC: &str = "modpack_sync";
const INVITE_TO_SYNC: &str = "invite_to_sync";
const INVITE_PREFIX: &str = "You have been invited to collaborate on a modpack by ";

#[derive(Debug, Subcommand)]
pub enum NotificationsCommand {
    /// List notifications, newest first.
    List {
        /// Include the modpack update notifications the app hides.
        #[arg(long)]
        all: bool,
    },
    /// Mark a notification as read.
    Read { id: String },
    /// Accept the Quadrant Sync invite a notification carries.
    Accept { id: String },
    /// Decline the Quadrant Sync invite a notification carries.
    Decline { id: String },
    /// Print notifications until Ctrl+C while running the desktop app's
    /// background workers, which sync settings and can update modpacks.
    ///
    /// Those workers change state: settings sync may pull the cloud settings
    /// over the local ones or push the local ones, modpacks with a remote
    /// update are updated when autoQuadrantSync is on, and the notification
    /// cursor the desktop app shares moves forward.
    Watch,
}

/// A notification with the text and invite the app shows for it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Shown {
    #[serde(flatten)]
    notification: Notification,
    text: String,
    invite: Option<Invite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub invite_id: String,
    pub inviter: Option<String>,
}

/// The JSON a notification's `message` carries.
#[derive(Debug, Default, Deserialize)]
struct Details {
    notification_type: Option<String>,
    simple_message: Option<String>,
    message: Option<String>,
    invite_id: Option<String>,
    updated_by: Option<String>,
}

pub async fn run(command: NotificationsCommand, ctx: &Ctx) -> Result<Report> {
    ctx.require_account_build()?;
    let host = &ctx.host;
    match command {
        NotificationsCommand::List { all } => {
            let info = host.get_account_info().await?;
            let show_updates =
                config::get_bool(host, "showModpackUpdateNotifications")?.unwrap_or(true);
            let identities = identities(&info);
            let mut shown: Vec<Shown> = info
                .notifications
                .into_iter()
                .filter(|notification| all || is_visible(notification, show_updates, &identities))
                .map(show)
                .collect();
            shown.sort_by_key(|shown| std::cmp::Reverse(shown.notification.created_at_unix));
            Report::new(&shown, |shown| {
                if shown.is_empty() {
                    return "No notifications.".to_string();
                }
                shown.iter().map(describe).collect::<Vec<_>>().join("\n\n")
            })
        }
        NotificationsCommand::Read { id } => {
            host.read_notification(id.clone()).await?;
            Ok(Report::message(format!("Marked {id} as read.")))
        }
        NotificationsCommand::Accept { id } => answer(ctx, id, true).await,
        NotificationsCommand::Decline { id } => answer(ctx, id, false).await,
        NotificationsCommand::Watch => watch(ctx).await,
    }
}

async fn answer(ctx: &Ctx, id: String, accept: bool) -> Result<Report> {
    let info = ctx.host.get_account_info().await?;
    let notification = info
        .notifications
        .iter()
        .find(|notification| notification.notification_id == id)
        .ok_or_else(|| anyhow!("no notification {id}"))?;
    let invite = invite(notification).ok_or_else(|| anyhow!("{id} is not an invite"))?;
    ctx.host.answer_invite(invite.invite_id, id, accept).await?;
    Ok(Report::message(if accept {
        "Accepted the invite."
    } else {
        "Declined the invite."
    }))
}

async fn watch(ctx: &Ctx) -> Result<Report> {
    let host = &ctx.host;
    let info = host.get_account_info().await?;
    let identities = identities(&info);
    let show_updates = config::get_bool(host, "showModpackUpdateNotifications")?.unwrap_or(true);
    let mut seen: HashSet<String> = info
        .notifications
        .iter()
        .map(|notification| notification.notification_id.clone())
        .collect();
    let mut events = host.subscribe_events();
    host.start_background_workers().await?;
    ctx.out
        .note("Waiting for notifications; press Ctrl+C to stop.");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            event = events.recv() => match event {
                Ok(event) if event.event == "refreshNotifications" => {
                    let notifications: Vec<Notification> =
                        serde_json::from_value(event.payload).unwrap_or_default();
                    for notification in notifications {
                        if !seen.insert(notification.notification_id.clone())
                            || !is_visible(&notification, show_updates, &identities)
                        {
                            continue;
                        }
                        let shown = show(notification);
                        if ctx.out.json {
                            println!("{}", serde_json::to_string(&shown)?);
                        } else {
                            println!("{}\n", describe(&shown));
                        }
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
        }
    }
    host.stop_background_workers().await?;
    Ok(Report::streamed())
}

fn show(notification: Notification) -> Shown {
    Shown {
        text: text(&notification),
        invite: invite(&notification),
        notification,
    }
}

fn describe(shown: &Shown) -> String {
    let marker = if shown.notification.read {
        ""
    } else {
        " (unread)"
    };
    let mut text = format!(
        "[{}] {}{marker}\n{}",
        shown.notification.notification_id, shown.notification.created_at, shown.text
    );
    if shown.invite.is_some() {
        text.push_str(&format!(
            "\n`quadrantmc notifications accept {id}` or `quadrantmc notifications decline {id}`",
            id = shown.notification.notification_id
        ));
    }
    text
}

fn identities(info: &AccountInfo) -> Vec<String> {
    [&info.name, &info.login]
        .into_iter()
        .filter_map(|identity| normalize(identity))
        .collect()
}

fn normalize(identity: &str) -> Option<String> {
    let normalized = identity.trim().to_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

fn details(notification: &Notification) -> Details {
    serde_json::from_str(&notification.message).unwrap_or_default()
}

fn kind(notification: &Notification, details: &Details) -> Option<String> {
    details
        .notification_type
        .clone()
        .or_else(|| notification.notification_type.clone())
}

/// Whether the app's notification list shows this one (`Notifications.tsx`):
/// modpack update notices are hidden when the user turned them off, or when
/// the user made the update themselves.
pub fn is_visible(
    notification: &Notification,
    show_modpack_updates: bool,
    identities: &[String],
) -> bool {
    let details = details(notification);
    if kind(notification, &details).as_deref() != Some(MODPACK_SYNC) {
        return true;
    }
    if !show_modpack_updates {
        return false;
    }
    match details.updated_by.as_deref().and_then(normalize) {
        Some(updated_by) => !identities.contains(&updated_by),
        None => true,
    }
}

pub fn text(notification: &Notification) -> String {
    let details = details(notification);
    if let Some(invite) = invite(notification) {
        return format!(
            "You have been invited to collaborate on a Quadrant Sync modpack by {}",
            invite.inviter.unwrap_or_default()
        );
    }
    details
        .simple_message
        .unwrap_or_else(|| notification.message.clone())
}

pub fn invite(notification: &Notification) -> Option<Invite> {
    let details = details(notification);
    if kind(notification, &details).as_deref() != Some(INVITE_TO_SYNC) {
        return None;
    }
    let invite_id = details
        .invite_id
        .clone()
        .or_else(|| notification.resource_id.clone())?;
    let message = details
        .message
        .or(details.simple_message)
        .unwrap_or_else(|| notification.message.clone());
    let inviter = message
        .split_once(INVITE_PREFIX)
        .map(|(_, inviter)| inviter.trim().to_string());
    Some(Invite { invite_id, inviter })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn notification(message: serde_json::Value, kind: Option<&str>) -> Notification {
        Notification {
            notification_id: "n1".to_string(),
            user_id: "u1".to_string(),
            notification_type: kind.map(str::to_string),
            resource_id: Some("resource".to_string()),
            message: match message {
                serde_json::Value::String(text) => text,
                other => other.to_string(),
            },
            created_at: "2026-01-01T00:00:00Z".to_string(),
            created_at_unix: 0,
            read: false,
        }
    }

    fn me() -> Vec<String> {
        vec!["alice".to_string()]
    }

    #[test]
    fn modpack_updates_hide_when_disabled_or_made_by_the_user() {
        let own = notification(
            json!({"notification_type": "modpack_sync", "updated_by": " Alice "}),
            None,
        );
        let others = notification(
            json!({"notification_type": "modpack_sync", "updated_by": "bob"}),
            None,
        );
        let anonymous = notification(json!({}), Some("modpack_sync"));
        assert!(!is_visible(&own, true, &me()));
        assert!(is_visible(&others, true, &me()));
        assert!(!is_visible(&others, false, &me()));
        assert!(is_visible(&anonymous, true, &me()));
    }

    #[test]
    fn other_notifications_always_show() {
        let plain = notification(json!("not json at all"), None);
        assert!(is_visible(&plain, false, &me()));
        assert_eq!(text(&plain), "not json at all");
        let simple = notification(json!({"simple_message": "Hello"}), None);
        assert_eq!(text(&simple), "Hello");
    }

    #[test]
    fn invites_carry_their_id_and_inviter() {
        let with_id = notification(
            json!({
                "notification_type": "invite_to_sync",
                "invite_id": "pack-1",
                "message": "You have been invited to collaborate on a modpack by bob"
            }),
            None,
        );
        assert_eq!(
            invite(&with_id),
            Some(Invite {
                invite_id: "pack-1".to_string(),
                inviter: Some("bob".to_string()),
            })
        );
        assert!(text(&with_id).ends_with("by bob"));

        let fallback = notification(json!({}), Some("invite_to_sync"));
        assert_eq!(
            invite(&fallback).map(|invite| invite.invite_id),
            Some("resource".to_string())
        );
        assert_eq!(invite(&notification(json!({}), None)), None);
    }
}
