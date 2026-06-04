//! Feed client for the news ingestion pipeline. Supports RSS 2.0 and Atom 1.0.
//! Detects the feed format automatically and routes RSS 2.0 and Atom
//! through the same fetch, validation, and parsing pipeline.
//! Security boundary:
//! - feed URLs must pass the slice 1 allowlist and scheme guard
//! - resolved IPs must stay on public addresses before connecting
//! - redirects are followed manually and every `Location` hop is re-validated
//! - RSS payloads are converted into sanitized article candidates only
#![allow(dead_code)]

use std::{
    collections::HashSet,
    io::Cursor,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

use atom_syndication::{Entry as AtomEntry, Feed as AtomFeed};
use reqwest::{header::LOCATION, redirect::Policy, Client, Response};
use rss::{Channel, Guid, Item};
use url::{Host, Url};

use super::{
    allowlist::NetworkAllowlist,
    url_guard::{is_disallowed_ip_addr, validate_parsed_url, validate_url, UrlPurpose},
};
use crate::{error::AppError, paths::AppPaths};

const MAX_REDIRECTS: usize = 5;
const REQUEST_TIMEOUT_SECS: u64 = 15;
const RSS_USER_AGENT: &str = "yuuko-news-biyori/0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RssItem {
    pub title: String,
    pub article_url: String,
    pub summary: Option<String>,
    pub source_name: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RssClient {
    allowlist_path: PathBuf,
    max_redirects: usize,
    request_timeout: Duration,
}

impl RssClient {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            allowlist_path: paths.network_allowlist_path.clone(),
            max_redirects: MAX_REDIRECTS,
            request_timeout: Duration::from_secs(REQUEST_TIMEOUT_SECS),
        }
    }

    pub async fn fetch(&self, feed_url: &str) -> Result<Vec<RssItem>, AppError> {
        let allowlist = NetworkAllowlist::load(&self.allowlist_path)?;
        let feed_url =
            canonicalize_request_url(validate_url(feed_url, UrlPurpose::Rss, &allowlist)?)?;
        let (final_feed_url, bytes) = self.fetch_feed_bytes(feed_url, &allowlist).await?;
        parse_feed_items(&bytes, &final_feed_url, &allowlist)
    }

    async fn fetch_feed_bytes(
        &self,
        initial_url: Url,
        allowlist: &NetworkAllowlist,
    ) -> Result<(Url, Vec<u8>), AppError> {
        let mut current_url = initial_url;
        let mut visited = HashSet::from([current_url.to_string()]);

        for redirect_count in 0..=self.max_redirects {
            let response = self.send_request(&current_url).await?;
            if response.status().is_redirection() {
                if redirect_count == self.max_redirects {
                    return Err(AppError::Network(format!(
                        "RSS fetch exceeded redirect limit ({})",
                        self.max_redirects
                    )));
                }

                let next_url = resolve_redirect_target(&current_url, &response, allowlist)?;
                let next_url_key = next_url.to_string();
                if !visited.insert(next_url_key) {
                    return Err(AppError::Network(
                        "RSS redirect loop detected; refusing to continue".to_string(),
                    ));
                }

                current_url = next_url;
                continue;
            }

            if !response.status().is_success() {
                return Err(AppError::Network(format!(
                    "RSS fetch failed with HTTP status {}",
                    response.status()
                )));
            }

            let body = response.bytes().await.map_err(|error| {
                AppError::Network(format!("failed to read RSS response body: {error}"))
            })?;
            return Ok((current_url, body.to_vec()));
        }

        Err(AppError::Network(
            "RSS fetch did not complete within the redirect limit".to_string(),
        ))
    }

    async fn send_request(&self, url: &Url) -> Result<Response, AppError> {
        let client = self.build_client_for(url)?;
        client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| AppError::Network(format!("failed to fetch RSS feed: {error}")))
    }

    fn build_client_for(&self, url: &Url) -> Result<Client, AppError> {
        let mut builder = Client::builder()
            .redirect(Policy::none())
            .timeout(self.request_timeout)
            .user_agent(RSS_USER_AGENT);

        if let Some(Host::Domain(domain)) = url.host() {
            let resolved_addrs = resolve_public_socket_addrs(domain, url)?;
            builder = builder.resolve_to_addrs(domain, &resolved_addrs);
        }

        builder
            .build()
            .map_err(|error| AppError::Network(format!("failed to build RSS HTTP client: {error}")))
    }
}

fn resolve_public_socket_addrs(host: &str, url: &Url) -> Result<Vec<SocketAddr>, AppError> {
    let port = url.port_or_known_default().ok_or_else(|| {
        AppError::Validation("RSS URL must use a known http/https port".to_string())
    })?;
    let resolved = (host, port).to_socket_addrs().map_err(|error| {
        AppError::Network(format!("failed to resolve RSS host '{host}': {error}"))
    })?;

    validate_resolved_socket_addrs(host, resolved)
}

fn validate_resolved_socket_addrs<I>(host: &str, addrs: I) -> Result<Vec<SocketAddr>, AppError>
where
    I: IntoIterator<Item = SocketAddr>,
{
    let mut validated = Vec::new();
    let mut seen = HashSet::new();

    for addr in addrs {
        let ip = addr.ip();
        if is_disallowed_ip_addr(ip) {
            return Err(AppError::Validation(format!(
                "RSS host '{host}' resolved to disallowed IP '{ip}'"
            )));
        }

        if seen.insert(ip) {
            validated.push(SocketAddr::new(ip, 0));
        }
    }

    if validated.is_empty() {
        return Err(AppError::Network(format!(
            "RSS host '{host}' did not resolve to any public IP addresses"
        )));
    }

    Ok(validated)
}

fn resolve_redirect_target(
    current_url: &Url,
    response: &Response,
    allowlist: &NetworkAllowlist,
) -> Result<Url, AppError> {
    let location = response
        .headers()
        .get(LOCATION)
        .ok_or_else(|| AppError::Network("redirect response did not include Location".to_string()))?
        .to_str()
        .map_err(|error| {
            AppError::Network(format!("redirect Location header is invalid: {error}"))
        })?;

    resolve_redirect_location(current_url, location, allowlist)
}

fn resolve_redirect_location(
    current_url: &Url,
    location: &str,
    allowlist: &NetworkAllowlist,
) -> Result<Url, AppError> {
    let location = location.trim();
    if location.is_empty() {
        return Err(AppError::Network(
            "redirect Location header must not be empty".to_string(),
        ));
    }

    let next_url = current_url.join(location).map_err(|error| {
        AppError::Validation(format!("redirect Location is not a valid URL: {error}"))
    })?;
    validate_parsed_url(&next_url, UrlPurpose::Rss, allowlist)?;
    canonicalize_request_url(next_url)
}

fn canonicalize_request_url(mut url: Url) -> Result<Url, AppError> {
    let normalized_host = match url.host() {
        Some(Host::Domain(domain)) => {
            NormalizedRequestHost::Domain(domain.trim_end_matches('.').to_ascii_lowercase())
        }
        Some(Host::Ipv4(ip)) => NormalizedRequestHost::Ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => NormalizedRequestHost::Ip(IpAddr::V6(ip)),
        None => return Ok(url),
    };

    match normalized_host {
        NormalizedRequestHost::Domain(domain) => {
            url.set_host(Some(&domain)).map_err(|error| {
                AppError::Validation(format!("failed to normalize URL host for request: {error}"))
            })?;
        }
        NormalizedRequestHost::Ip(ip) => {
            url.set_ip_host(ip).map_err(|_| {
                AppError::Validation("failed to normalize URL IP host for request".to_string())
            })?;
        }
    }

    Ok(url)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedFormat {
    Rss,
    Atom,
}

/// Detects the feed format from the root element.
/// Unsupported formats such as RSS 1.0 (RDF) return `None`.
fn detect_feed_format(feed_bytes: &[u8]) -> Option<FeedFormat> {
    let text = String::from_utf8_lossy(feed_bytes);
    let rss_at = find_root_tag(&text, "rss");
    let feed_at = find_root_tag(&text, "feed");
    match (rss_at, feed_at) {
        (Some(rss_pos), Some(feed_pos)) => Some(if rss_pos <= feed_pos {
            FeedFormat::Rss
        } else {
            FeedFormat::Atom
        }),
        (Some(_), None) => Some(FeedFormat::Rss),
        (None, Some(_)) => Some(FeedFormat::Atom),
        (None, None) => None,
    }
}

/// Returns the position of the opening `<tag` token when it is followed by a
/// delimiter, so similarly named elements do not match by accident.
fn find_root_tag(text: &str, tag: &str) -> Option<usize> {
    let needle = format!("<{tag}");
    let mut from = 0;
    while let Some(relative) = text[from..].find(&needle) {
        let position = from + relative;
        let next = text[position + needle.len()..].chars().next();
        match next {
            None | Some(' ') | Some('\t') | Some('\r') | Some('\n') | Some('>') | Some('/') => {
                return Some(position);
            }
            _ => from = position + needle.len(),
        }
    }
    None
}

/// Parses a feed payload into sanitized article candidates.
/// Supports both RSS 2.0 and Atom.
fn parse_feed_items(
    feed_bytes: &[u8],
    feed_url: &Url,
    allowlist: &NetworkAllowlist,
) -> Result<Vec<RssItem>, AppError> {
    match detect_feed_format(feed_bytes) {
        Some(FeedFormat::Rss) => parse_rss_items(feed_bytes, feed_url, allowlist),
        Some(FeedFormat::Atom) => parse_atom_items(feed_bytes, feed_url, allowlist),
        None => Err(AppError::Parse(
            "unsupported or unrecognized feed format (expected RSS 2.0 or Atom)".to_string(),
        )),
    }
}

fn parse_atom_items(
    feed_bytes: &[u8],
    feed_url: &Url,
    allowlist: &NetworkAllowlist,
) -> Result<Vec<RssItem>, AppError> {
    let feed = AtomFeed::read_from(Cursor::new(feed_bytes))
        .map_err(|error| AppError::Parse(format!("failed to parse Atom feed: {error}")))?;

    let source_name = sanitize_plain_text(&feed.title().value)
        .or_else(|| feed_url.host_str().and_then(sanitize_plain_text))
        .unwrap_or_else(|| "unknown".to_string());

    let mut results = Vec::new();
    let mut seen_urls = HashSet::new();

    for entry in feed.entries() {
        if let Some(candidate) = map_atom_entry(entry, &source_name, allowlist)? {
            if seen_urls.insert(candidate.article_url.clone()) {
                results.push(candidate);
            }
        }
    }

    Ok(results)
}

fn map_atom_entry(
    entry: &AtomEntry,
    source_name: &str,
    allowlist: &NetworkAllowlist,
) -> Result<Option<RssItem>, AppError> {
    let raw_article_url = match atom_entry_link(entry) {
        Some(url) if !url.trim().is_empty() => url,
        _ => return Ok(None),
    };

    let article_url = match validate_url(raw_article_url, UrlPurpose::Article, allowlist) {
        Ok(url) => canonicalize_request_url(url)?.to_string(),
        Err(AppError::Validation(_)) => return Ok(None),
        Err(error) => return Err(error),
    };

    let title = sanitize_plain_text(&entry.title().value).unwrap_or_else(|| article_url.clone());

    let summary = entry
        .summary()
        .map(|text| text.value.clone())
        .or_else(|| entry.content().and_then(|content| content.value.clone()))
        .as_deref()
        .and_then(sanitize_html_fragment);

    let published_at = entry.published().map(|datetime| {
        datetime
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    });

    Ok(Some(RssItem {
        title,
        article_url,
        summary,
        source_name: source_name.to_string(),
        published_at,
    }))
}

/// Extracts the article URL from an Atom entry.
///
/// Priority:
/// 1. `rel="alternate"`
/// 2. the first link whose `rel` is empty or omitted
/// 3. a URL-shaped `id`, but only when the entry has no `<link>` elements
///
/// `rel="self"` is never treated as an article URL.
fn atom_entry_link(entry: &AtomEntry) -> Option<&str> {
    let links = entry.links();

    links
        .iter()
        .find(|link| link.rel() == "alternate")
        .map(|link| link.href())
        .filter(|href| !href.trim().is_empty())
        .or_else(|| {
            links
                .iter()
                .find(|link| link.rel().trim().is_empty())
                .map(|link| link.href())
                .filter(|href| !href.trim().is_empty())
        })
        .or_else(|| {
            if links.is_empty() {
                let id = entry.id();
                if id.starts_with("https://") || id.starts_with("http://") {
                    Some(id)
                } else {
                    None
                }
            } else {
                None
            }
        })
}

fn parse_rss_items(
    feed_bytes: &[u8],
    feed_url: &Url,
    allowlist: &NetworkAllowlist,
) -> Result<Vec<RssItem>, AppError> {
    let channel = Channel::read_from(Cursor::new(feed_bytes))
        .map_err(|error| AppError::Parse(format!("failed to parse RSS feed: {error}")))?;

    let source_name = sanitize_plain_text(channel.title())
        .or_else(|| feed_url.host_str().and_then(sanitize_plain_text))
        .unwrap_or_else(|| "unknown".to_string());

    let mut results = Vec::new();
    let mut seen_urls = HashSet::new();

    for item in channel.items() {
        if let Some(candidate) = map_feed_item(item, &source_name, allowlist)? {
            if seen_urls.insert(candidate.article_url.clone()) {
                results.push(candidate);
            }
        }
    }

    Ok(results)
}

fn map_feed_item(
    item: &Item,
    source_name: &str,
    allowlist: &NetworkAllowlist,
) -> Result<Option<RssItem>, AppError> {
    let raw_article_url = item
        .link()
        .or_else(|| guid_permalink(item.guid()))
        .unwrap_or_default();

    if raw_article_url.trim().is_empty() {
        return Ok(None);
    }

    let article_url = match validate_url(raw_article_url, UrlPurpose::Article, allowlist) {
        Ok(url) => canonicalize_request_url(url)?.to_string(),
        Err(AppError::Validation(_)) => return Ok(None),
        Err(error) => return Err(error),
    };

    let title = sanitize_plain_text(item.title().unwrap_or_default())
        .unwrap_or_else(|| article_url.clone());
    let summary = item
        .content()
        .or_else(|| item.description())
        .and_then(sanitize_html_fragment);
    let published_at = item.pub_date().and_then(sanitize_plain_text);

    Ok(Some(RssItem {
        title,
        article_url,
        summary,
        source_name: source_name.to_string(),
        published_at,
    }))
}

fn guid_permalink(guid: Option<&Guid>) -> Option<&str> {
    guid.and_then(|guid| guid.is_permalink().then_some(guid.value()))
}

fn sanitize_html_fragment(value: &str) -> Option<String> {
    sanitize_plain_text(&strip_html_tags(value))
}

fn sanitize_plain_text(value: &str) -> Option<String> {
    let mut sanitized = String::with_capacity(value.len());
    let mut last_was_space = false;

    for ch in value.chars() {
        let normalized = match ch {
            '\u{0000}'..='\u{0008}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000E}'..='\u{001F}'
            | '\u{007F}' => ' ',
            _ => ch,
        };

        if normalized.is_whitespace() {
            if !last_was_space && !sanitized.is_empty() {
                sanitized.push(' ');
            }
            last_was_space = true;
            continue;
        }

        sanitized.push(normalized);
        last_was_space = false;
    }

    let sanitized = sanitized.trim().to_string();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn strip_html_tags(value: &str) -> String {
    let mut stripped = String::with_capacity(value.len());
    let mut in_tag = false;

    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => stripped.push(ch),
            _ => {}
        }
    }

    stripped
}

enum NormalizedRequestHost {
    Domain(String),
    Ip(IpAddr),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::allowlist::NetworkAllowlist;

    fn allowlist() -> NetworkAllowlist {
        NetworkAllowlist {
            allowed_rss_domains: vec!["rss.example.com".to_string()],
            allowed_article_domains: vec!["news.example.com".to_string()],
            ..NetworkAllowlist::default()
        }
    }

    #[test]
    fn rejects_private_dns_results() {
        let error = validate_resolved_socket_addrs(
            "rss.example.com",
            vec![
                SocketAddr::new(IpAddr::from([203, 0, 113, 10]), 443),
                SocketAddr::new(IpAddr::from([10, 0, 0, 8]), 443),
            ],
        )
        .expect_err("private address in DNS results must fail closed");

        assert!(error.to_string().contains("disallowed IP"));
    }

    #[test]
    fn keeps_unique_public_dns_results() {
        let addrs = validate_resolved_socket_addrs(
            "rss.example.com",
            vec![
                SocketAddr::new(IpAddr::from([203, 0, 113, 10]), 443),
                SocketAddr::new(IpAddr::from([203, 0, 113, 10]), 8443),
                SocketAddr::new(IpAddr::from([203, 0, 113, 11]), 443),
            ],
        )
        .expect("public DNS results should pass");

        assert_eq!(
            addrs,
            vec![
                SocketAddr::new(IpAddr::from([203, 0, 113, 10]), 0),
                SocketAddr::new(IpAddr::from([203, 0, 113, 11]), 0),
            ]
        );
    }

    #[test]
    fn resolves_relative_redirects_through_url_guard() {
        let current_url = Url::parse("https://rss.example.com/feed.xml").unwrap();
        let next_url = resolve_redirect_location(&current_url, "/latest.xml", &allowlist())
            .expect("redirect should pass");

        assert_eq!(next_url.as_str(), "https://rss.example.com/latest.xml");
    }

    #[test]
    fn rejects_redirects_to_non_allowlisted_hosts() {
        let current_url = Url::parse("https://rss.example.com/feed.xml").unwrap();
        let error = resolve_redirect_location(
            &current_url,
            "https://evil.example.com/feed.xml",
            &allowlist(),
        )
        .expect_err("redirect must stay inside the RSS allowlist");

        assert!(error.to_string().contains("not present in the allowlist"));
    }

    #[test]
    fn parses_feed_items_and_sanitizes_output() {
        let feed_url = Url::parse("https://rss.example.com/feed.xml").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title> Example News </title>
    <link>https://rss.example.com/</link>
    <description>test feed</description>
    <item>
      <title> First	Story </title>
      <link>https://news.example.com/articles/1</link>
      <description><![CDATA[<p>Hello <b>world</b>.</p>]]></description>
      <pubDate>Thu, 04 Jun 2026 10:00:00 +0900</pubDate>
    </item>
    <item>
      <title>Ignored item</title>
      <link>https://evil.example.com/articles/2</link>
      <description>Should not pass allowlist</description>
    </item>
    <item>
      <title> Duplicate </title>
      <link>https://news.example.com/articles/1</link>
      <description>duplicate</description>
    </item>
  </channel>
</rss>"#,
            &feed_url,
            &allowlist(),
        )
        .expect("feed should parse");

        assert_eq!(
            items,
            vec![RssItem {
                title: "First Story".to_string(),
                article_url: "https://news.example.com/articles/1".to_string(),
                summary: Some("Hello world.".to_string()),
                source_name: "Example News".to_string(),
                published_at: Some("Thu, 04 Jun 2026 10:00:00 +0900".to_string()),
            }]
        );
    }

    #[test]
    fn uses_permalink_guid_when_link_is_missing() {
        let feed_url = Url::parse("https://rss.example.com/feed.xml").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Feed</title>
    <link>https://rss.example.com/</link>
    <description>test feed</description>
    <item>
      <title>Guid fallback</title>
      <guid isPermaLink="true">https://news.example.com/articles/42</guid>
    </item>
  </channel>
</rss>"#,
            &feed_url,
            &allowlist(),
        )
        .expect("guid permalink should be accepted");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].article_url, "https://news.example.com/articles/42");
    }

    #[test]
    fn detects_rss_and_atom_formats() {
        assert_eq!(
            detect_feed_format(
                br#"<?xml version="1.0"?><rss version="2.0"><channel></channel></rss>"#
            ),
            Some(FeedFormat::Rss)
        );
        assert_eq!(
            detect_feed_format(
                br#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"></feed>"#
            ),
            Some(FeedFormat::Atom)
        );
        assert_eq!(detect_feed_format(b"<html><body></body></html>"), None);
    }

    #[test]
    fn parses_atom_feed_items_and_sanitizes_output() {
        let feed_url = Url::parse("https://rss.example.com/atom.xml").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>urn:example:feed</id>
  <title> Example Atom </title>
  <updated>2026-06-03T16:00:00Z</updated>
  <entry>
    <id>urn:example:atom-1</id>
    <title> First Atom Story </title>
    <link rel="alternate" href="https://news.example.com/articles/atom-1"/>
    <summary type="html"><![CDATA[<p>Hello <b>atom</b>.</p>]]></summary>
    <published>2026-06-03T15:11:06Z</published>
    <updated>2026-06-03T16:00:00Z</updated>
  </entry>
  <entry>
    <id>urn:example:rejected</id>
    <title>Rejected by allowlist</title>
    <link rel="alternate" href="https://evil.example.com/articles/2"/>
    <updated>2026-06-03T16:00:00Z</updated>
  </entry>
</feed>"#,
            &feed_url,
            &allowlist(),
        )
        .expect("atom feed should parse");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "First Atom Story");
        assert_eq!(
            items[0].article_url,
            "https://news.example.com/articles/atom-1"
        );
        assert_eq!(items[0].summary.as_deref(), Some("Hello atom."));
        assert_eq!(
            items[0].published_at.as_deref(),
            Some("2026-06-03T15:11:06Z")
        );
        assert_eq!(items[0].source_name, "Example Atom");
    }

    #[test]
    fn atom_link_fallback_skips_self_and_accepts_empty_rel() {
        let feed_url = Url::parse("https://rss.example.com/atom.xml").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>urn:example:feed</id>
  <title>Example Atom</title>
  <updated>2026-06-03T16:00:00Z</updated>
  <entry>
    <id>urn:example:alternate-wins</id>
    <title>Alternate wins</title>
    <link rel="self" href="https://rss.example.com/entries/alternate-wins.xml"/>
    <link rel="alternate" href="https://news.example.com/articles/alternate-wins"/>
    <updated>2026-06-03T16:00:00Z</updated>
  </entry>
  <entry>
    <id>urn:example:empty-rel</id>
    <title>Empty rel fallback</title>
    <link rel="" href="https://news.example.com/articles/empty-rel"/>
    <updated>2026-06-03T16:00:00Z</updated>
  </entry>
  <entry>
    <id>https://news.example.com/articles/self-only-id</id>
    <title>Self only should skip</title>
    <link rel="self" href="https://rss.example.com/entries/self-only.xml"/>
    <updated>2026-06-03T16:00:00Z</updated>
  </entry>
  <entry>
    <id>https://news.example.com/articles/id-only</id>
    <title>ID only fallback</title>
    <updated>2026-06-03T16:00:00Z</updated>
  </entry>
</feed>"#,
            &feed_url,
            &allowlist(),
        )
        .expect("atom feed should parse");

        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].article_url,
            "https://news.example.com/articles/alternate-wins"
        );
        assert_eq!(
            items[1].article_url,
            "https://news.example.com/articles/empty-rel"
        );
        assert_eq!(
            items[2].article_url,
            "https://news.example.com/articles/id-only"
        );
        assert!(items.iter().all(|item| {
            item.article_url != "https://rss.example.com/entries/self-only.xml"
                && item.article_url != "https://news.example.com/articles/self-only-id"
        }));
    }
}
