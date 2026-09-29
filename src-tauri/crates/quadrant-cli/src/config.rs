//! Config writes, which have to keep settings sync working the way the
//! desktop frontend does.

use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use quadrant_host::QuadrantHost;
use serde_json::Value;

/// Compared against the cloud copy's sync date by settings sync
/// (`quadrant_settings_sync.rs`). The desktop frontend bumps it on every other
/// config change (`src/App.tsx`); the host does not, so a CLI write has to.
const LAST_SETTINGS_UPDATED: &str = "lastSettingsUpdated";

pub fn set(host: &QuadrantHost, key: &str, value: Value) -> Result<()> {
    host.set_config_value(key, value)?;
    touch(host, key)
}

pub fn unset(host: &QuadrantHost, key: &str) -> Result<()> {
    host.remove_config_value(key)?;
    touch(host, key)
}

fn touch(host: &QuadrantHost, key: &str) -> Result<()> {
    if key == LAST_SETTINGS_UPDATED {
        return Ok(());
    }
    host.set_config_value(
        LAST_SETTINGS_UPDATED,
        Value::String(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
    )
}

pub fn get_string(host: &QuadrantHost, key: &str) -> Result<Option<String>> {
    Ok(host
        .get_config_value(key)?
        .and_then(|value| value.as_str().map(str::to_string)))
}

pub fn get_bool(host: &QuadrantHost, key: &str) -> Result<Option<bool>> {
    Ok(host
        .get_config_value(key)?
        .and_then(|value| value.as_bool()))
}

/// Settings the app stores as text even when the text looks like JSON, such
/// as a `1.21` version. Keys missing from a fresh config are listed too.
const TEXT_KEYS: &[&str] = &[
    "channel",
    "hardwareId",
    "lastPageName",
    "lastRSSfetched",
    LAST_SETTINGS_UPDATED,
    "lastUsedAPI",
    "lastUsedModpack",
    "lastUsedVersion",
    "locale",
    "mcFolder",
    "oauthState",
    "prismLauncherFolder",
];

/// Turns a typed value into what gets stored. JSON literals (`true`, `3`,
/// `{"a":1}`) are stored as JSON and anything else as text, except that a
/// text setting stays text.
pub fn parse_value(key: &str, raw: &str, current: Option<&Value>) -> Value {
    let is_text = TEXT_KEYS.contains(&key) || matches!(current, Some(Value::String(_)));
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::String(text)) => Value::String(text),
        Ok(value) if !is_text => value,
        _ => Value::String(raw.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_literals_parse_for_new_or_non_string_keys() {
        assert_eq!(parse_value("devMode", "true", None), json!(true));
        assert_eq!(parse_value("uiScale", "150", Some(&json!(100))), json!(150));
        assert_eq!(parse_value("custom", r#"{"a":1}"#, None), json!({"a": 1}));
        assert_eq!(
            parse_value("custom", "hello there", None),
            json!("hello there")
        );
    }

    fn scratch_host() -> (tempfile::TempDir, QuadrantHost) {
        let dir = tempfile::tempdir().unwrap();
        let options = quadrant_host::QuadrantHostOptions::new(dir.path().to_path_buf(), "", "", "");
        let host = QuadrantHost::new(options).unwrap();
        (dir, host)
    }

    fn stored_timestamp(host: &QuadrantHost) -> Option<String> {
        host.get_config_value(LAST_SETTINGS_UPDATED)
            .unwrap()
            .and_then(|value| value.as_str().map(str::to_string))
    }

    #[test]
    fn writes_bump_the_settings_sync_timestamp() {
        let (_dir, host) = scratch_host();
        host.set_config_value(LAST_SETTINGS_UPDATED, json!("1970-01-01T00:00:00+00:00"))
            .unwrap();

        set(&host, "silentNews", json!(true)).unwrap();
        let bumped = stored_timestamp(&host).unwrap();
        assert!(bumped.starts_with("20"), "{bumped}");
        assert!(bumped.ends_with('Z'), "{bumped}");

        set(
            &host,
            LAST_SETTINGS_UPDATED,
            json!("2000-01-01T00:00:00.000Z"),
        )
        .unwrap();
        assert_eq!(
            stored_timestamp(&host).as_deref(),
            Some("2000-01-01T00:00:00.000Z")
        );

        unset(&host, "silentNews").unwrap();
        assert_eq!(host.get_config_value("silentNews").unwrap(), None);
        assert_ne!(
            stored_timestamp(&host).as_deref(),
            Some("2000-01-01T00:00:00.000Z")
        );
    }

    #[test]
    fn string_keys_stay_strings() {
        assert_eq!(parse_value("lastUsedVersion", "1.21", None), json!("1.21"));
        assert_eq!(
            parse_value("custom", "1.21", Some(&json!("1.20.1"))),
            json!("1.21")
        );
        assert_eq!(parse_value("channel", "true", None), json!("true"));
        assert_eq!(
            parse_value("custom", r#""quoted""#, Some(&json!("x"))),
            json!("quoted")
        );
    }
}
