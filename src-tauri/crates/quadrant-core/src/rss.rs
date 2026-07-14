//! RSS/news fetching for the Quadrant blog.

use crate::{Result, models::Article};
use anyhow::anyhow;
use chrono::{DateTime, Days, Utc};

/// Fetches the Quadrant RSS feed and converts it into typed articles.
pub async fn get_news() -> Result<Vec<Article>> {
    log::info!("Fetching RSS news feed");
    let new_qualifier = Utc::now().checked_sub_days(Days::new(14)).unwrap();
    let content = reqwest::get("https://blog.mrquantumoff.dev/rss/")
        .await
        .map_err(|error| anyhow!(error))?
        .bytes()
        .await
        .map_err(|error| anyhow!(error))?;
    let articles = parse_feed(&content, new_qualifier)?;
    log::info!("Fetched {} article(s) from RSS feed", articles.len());
    Ok(articles)
}

/// Converts raw RSS bytes into articles; items published after
/// `new_qualifier` are marked as new.
fn parse_feed(content: &[u8], new_qualifier: DateTime<Utc>) -> Result<Vec<Article>> {
    let rss = rss::Channel::read_from(content).map_err(|error| anyhow!(error))?;

    let mut articles = Vec::new();
    for item in rss.items {
        let Some(date) = item
            .pub_date
            .as_deref()
            .and_then(|date| DateTime::parse_from_rfc2822(date).ok())
            .map(|date| date.to_utc())
        else {
            log::warn!("Skipping RSS item with an invalid or missing publication date");
            continue;
        };
        let link = item.link.unwrap_or_default();
        let guid = item
            .guid
            .map(|guid| guid.value)
            .unwrap_or_else(|| link.clone());
        articles.push(Article {
            title: item.title.unwrap_or_default(),
            link,
            summary: item.description.unwrap_or_default(),
            new: date > new_qualifier,
            date,
            guid,
        });
    }
    Ok(articles)
}

#[cfg(test)]
mod tests {
    use super::parse_feed;
    use chrono::{DateTime, Utc};

    fn feed(items: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>Blog</title><link>https://example.com</link><description>d</description>{items}</channel></rss>"#
        )
    }

    fn qualifier() -> DateTime<Utc> {
        "2026-03-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
    }

    #[test]
    fn parse_feed_marks_recent_articles_as_new() {
        let xml = feed(
            r#"<item><title>Fresh</title><link>https://example.com/fresh</link><description>a</description><pubDate>Mon, 02 Mar 2026 12:00:00 GMT</pubDate><guid>g-fresh</guid></item>
<item><title>Old</title><link>https://example.com/old</link><description>b</description><pubDate>Sun, 01 Feb 2026 12:00:00 GMT</pubDate><guid>g-old</guid></item>"#,
        );

        let articles = parse_feed(xml.as_bytes(), qualifier()).unwrap();
        assert_eq!(articles.len(), 2);
        assert!(articles[0].new);
        assert_eq!(articles[0].title, "Fresh");
        assert_eq!(articles[0].guid, "g-fresh");
        assert!(!articles[1].new);
    }

    #[test]
    fn parse_feed_skips_items_without_a_valid_date() {
        let xml = feed(
            r#"<item><title>No date</title><link>https://example.com/x</link></item>
<item><title>Bad date</title><pubDate>not a date</pubDate></item>
<item><title>Ok</title><link>https://example.com/ok</link><pubDate>Mon, 02 Mar 2026 12:00:00 GMT</pubDate></item>"#,
        );

        let articles = parse_feed(xml.as_bytes(), qualifier()).unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].title, "Ok");
    }

    #[test]
    fn parse_feed_falls_back_to_link_for_missing_guid() {
        let xml = feed(
            r#"<item><title>t</title><link>https://example.com/only-link</link><pubDate>Mon, 02 Mar 2026 12:00:00 GMT</pubDate></item>"#,
        );

        let articles = parse_feed(xml.as_bytes(), qualifier()).unwrap();
        assert_eq!(articles[0].guid, "https://example.com/only-link");
    }

    #[test]
    fn parse_feed_rejects_malformed_xml() {
        assert!(parse_feed(b"this is not xml", qualifier()).is_err());
    }
}
