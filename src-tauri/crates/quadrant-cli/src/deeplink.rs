//! Link parsing ported from `src/deepLinks.ts`.

use url::Url;

/// A bare 7-digit Quadrant Share code, or the code in a
/// `https://usequadrant.dev/modpack/<code>` link.
pub fn parse_share_code(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if is_share_code(trimmed) {
        return Some(trimmed.to_string());
    }
    let url = Url::parse(trimmed).ok()?;
    if !matches!(
        url.host_str(),
        Some("usequadrant.dev" | "www.usequadrant.dev")
    ) {
        return None;
    }
    let path = url.path().strip_prefix("/modpack/")?;
    let code = path.strip_suffix('/').unwrap_or(path);
    is_share_code(code).then(|| code.to_string())
}

fn is_share_code(value: &str) -> bool {
    value.len() == 7 && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_codes_accept_bare_codes_and_links() {
        assert_eq!(parse_share_code("1234567").as_deref(), Some("1234567"));
        assert_eq!(
            parse_share_code("https://usequadrant.dev/modpack/1234567").as_deref(),
            Some("1234567")
        );
        assert_eq!(
            parse_share_code("https://www.usequadrant.dev/modpack/7654321").as_deref(),
            Some("7654321")
        );
        assert_eq!(
            parse_share_code("https://usequadrant.dev/modpack/1234567/").as_deref(),
            Some("1234567")
        );
        assert_eq!(parse_share_code("  1234567  ").as_deref(), Some("1234567"));
        assert_eq!(
            parse_share_code("  https://usequadrant.dev/modpack/1234567  ").as_deref(),
            Some("1234567")
        );
    }

    #[test]
    fn share_codes_reject_other_shapes() {
        assert_eq!(parse_share_code("123456"), None);
        assert_eq!(parse_share_code("12345678"), None);
        assert_eq!(
            parse_share_code("https://usequadrant.dev/modpack/123456"),
            None
        );
        assert_eq!(
            parse_share_code("https://evil.example.com/modpack/1234567"),
            None
        );
        assert_eq!(parse_share_code("not a url"), None);
    }
}
