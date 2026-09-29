use std::sync::LazyLock;

use serde_json::{Map, Value};

static ENGLISH: LazyLock<Map<String, Value>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../src/locales/en.json")).unwrap_or_default()
});

/// The English text for an i18n key the desktop app ships, if it has one.
pub fn translate(key: &str) -> Option<&'static str> {
    ENGLISH.get(key).and_then(Value::as_str)
}

/// The text to show for an error message, mirroring `describeError` in
/// `src/errors.ts`: the backend sends i18n keys for failures it classified,
/// and anything else is shown as it came.
pub fn describe_error(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return translate("errorUnknownNoDetails")
            .unwrap_or("Something went wrong.")
            .to_string();
    }
    // A key never contains whitespace, so an English sentence can't shadow one.
    if !raw.contains(char::is_whitespace)
        && let Some(text) = translate(raw)
    {
        return text.to_string();
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_known_error_keys() {
        assert_eq!(
            describe_error("errorCloudSyncNewer"),
            "The cloud copy of that modpack is newer than the one on this computer."
        );
    }

    #[test]
    fn keeps_unknown_or_sentence_messages_verbatim() {
        assert_eq!(describe_error("errorNotARealKey"), "errorNotARealKey");
        assert_eq!(
            describe_error("failed to parse config.json"),
            "failed to parse config.json"
        );
    }

    #[test]
    fn empty_messages_get_a_generic_text() {
        assert_eq!(
            describe_error("  "),
            "Something went wrong. Please try again."
        );
    }
}
