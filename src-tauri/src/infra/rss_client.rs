//! Feed client for the news ingestion pipeline. Supports RSS 2.0 and Atom 1.0.
//! Detects the feed format automatically and routes RSS 2.0 and Atom
//! through the same fetch, validation, and parsing pipeline.
//! Security boundary:
//! - feed URLs must pass the slice 1 allowlist and scheme guard
//! - resolved IPs must stay on public addresses before connecting
//! - redirects are followed manually and every `Location` hop is re-validated
//! - RSS payloads are converted into sanitized article candidates only
//! - 受信本文は MAX_FEED_BODY_BYTES まで（超過は取得失敗）。タイトル・概要は文字数上限で切り詰める
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
    http_body::{read_async_response_body_capped, truncate_chars, BodyReadError},
    url_guard::{is_disallowed_ip_addr, validate_parsed_url, validate_url, UrlPurpose},
};
use crate::{error::AppError, paths::AppPaths};

const MAX_REDIRECTS: usize = 5;
const REQUEST_TIMEOUT_SECS: u64 = 15;
const RSS_USER_AGENT: &str = "yuuko-news-biyori/0.1";
/// RSS / Atom 応答本文の受信バイト上限（4 MiB）。セキュリティ詳細設計書 §9.1。
/// 一般的なフィードは数十〜数百 KiB のため十分な余裕を取りつつ、巨大応答の全量展開を防ぐ（常駐負荷・AGENTS.md §2）。
pub(crate) const MAX_FEED_BODY_BYTES: usize = 4 * 1024 * 1024;
/// 保存するタイトルの最大文字数（Unicode 文字数）。セキュリティ詳細設計書 §15.2「長すぎる文字列を制限する」。
pub(crate) const MAX_TITLE_CHARS: usize = 300;
/// 保存する RSS 概要の最大文字数。記事抜粋の上限（html_fetcher の MAX_EXCERPT_CHARS = 2,000）に揃える。
pub(crate) const MAX_SUMMARY_CHARS: usize = 2_000;

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
            let mut response = self.send_request(&current_url).await?;
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

            // 本文は受信上限付きで読む。超過はこのフィードの取得失敗として扱う（URL は載せない）。
            let body = read_async_response_body_capped(&mut response, MAX_FEED_BODY_BYTES)
                .await
                .map_err(feed_body_error)?;
            return Ok((current_url, body));
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

/// 本文読み取り失敗を固定文言の AppError にする（本文・URL・生のエラー文を含めない）。
fn feed_body_error(error: BodyReadError) -> AppError {
    match error {
        BodyReadError::TooLarge => {
            AppError::Network("RSS response body exceeded the receive size limit".to_string())
        }
        BodyReadError::Read => AppError::Network("failed to read RSS response body".to_string()),
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
            // ホスト名・解決先 IP は画面へ返さず debug ログにだけ残す（§16.3）。
            log::debug!("RSS host '{host}' resolved to disallowed IP '{ip}'");
            return Err(AppError::Validation(
                "RSS host resolved to a disallowed IP".to_string(),
            ));
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
        log::debug!("redirect Location could not be joined: {error}");
        AppError::Validation("redirect Location is not a valid URL".to_string())
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
                log::debug!("failed to normalize URL host for request: {error}");
                AppError::Validation("failed to normalize URL host for request".to_string())
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

    let title = sanitize_title(&entry.title().value, &article_url);

    let summary = entry
        .summary()
        .map(|text| text.value.clone())
        .or_else(|| entry.content().and_then(|content| content.value.clone()))
        .as_deref()
        .and_then(sanitize_summary);

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

    let title = sanitize_title(item.title().unwrap_or_default(), &article_url);
    let summary = item
        .content()
        .or_else(|| item.description())
        .and_then(sanitize_summary);
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

/// タイトルをサニタイズし、MAX_TITLE_CHARS 文字以内に切り詰める（空なら記事URLで代替）。
/// 重複判定と保存は同じ切り詰め後の値で行われる。
fn sanitize_title(raw: &str, article_url: &str) -> String {
    let title = sanitize_plain_text(raw).unwrap_or_else(|| article_url.to_string());
    truncate_chars(&title, MAX_TITLE_CHARS)
}

/// 概要（HTML断片）をタグ除去・サニタイズし、MAX_SUMMARY_CHARS 文字以内に切り詰める。
fn sanitize_summary(raw: &str) -> Option<String> {
    sanitize_html_fragment(raw).map(|summary| truncate_chars(&summary, MAX_SUMMARY_CHARS))
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
                SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 443),
                SocketAddr::new(IpAddr::from([10, 0, 0, 8]), 443),
            ],
        )
        .expect_err("private address in DNS results must fail closed");

        let message = error.to_string();
        assert!(message.contains("disallowed IP"));
        // ホスト名・解決先 IP は文言へ含めない（§16.3）。
        assert!(!message.contains("rss.example.com"));
        assert!(!message.contains("10.0.0.8"));
    }

    #[test]
    fn keeps_unique_public_dns_results() {
        let addrs = validate_resolved_socket_addrs(
            "rss.example.com",
            vec![
                SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 443),
                SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 8443),
                SocketAddr::new(IpAddr::from([8, 8, 4, 4]), 443),
            ],
        )
        .expect("public DNS results should pass");

        assert_eq!(
            addrs,
            vec![
                SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 0),
                SocketAddr::new(IpAddr::from([8, 8, 4, 4]), 0),
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

        let message = error.to_string();
        assert!(message.contains("not present in the allowlist"));
        assert!(!message.contains("evil.example.com"));
    }

    #[test]
    fn invalid_redirect_location_message_excludes_parse_details() {
        let current_url = Url::parse("https://rss.example.com/feed.xml").unwrap();
        let error = resolve_redirect_location(&current_url, "https://[secret", &allowlist())
            .expect_err("unparseable redirect must be rejected");

        match error {
            AppError::Validation(message) => {
                assert_eq!(message, "redirect Location is not a valid URL");
                assert!(!message.contains("secret"));
            }
            other => panic!("expected validation error, got {other:?}"),
        }
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

    // 組み直した応答は Content-Length を持つため、ここでは読む前の早期拒否経路を確認する。
    // Content-Length のない受信中の上限判定は http_body の append_chunk_capped 単体テストで確認する。
    #[test]
    fn rejects_feed_body_over_limit_as_fetch_failure() {
        // 上限 + 1 バイトの本文は TooLarge になり、固定文言の取得失敗（URL・本文なし）へ変換される。
        let body = vec![b'a'; MAX_FEED_BODY_BYTES + 1];
        let mut response = Response::from(tauri::http::Response::new(body));
        let error = tauri::async_runtime::block_on(read_async_response_body_capped(
            &mut response,
            MAX_FEED_BODY_BYTES,
        ))
        .expect_err("over-limit feed must be rejected");
        assert_eq!(error, BodyReadError::TooLarge);
        let message = feed_body_error(error).to_string();
        assert!(message.contains("exceeded the receive size limit"));
        assert!(!message.contains("http"));
    }

    #[test]
    fn accepts_feed_body_at_limit() {
        let body = vec![b'a'; MAX_FEED_BODY_BYTES];
        let mut response = Response::from(tauri::http::Response::new(body));
        let bytes = tauri::async_runtime::block_on(read_async_response_body_capped(
            &mut response,
            MAX_FEED_BODY_BYTES,
        ))
        .expect("body exactly at the limit must be accepted");
        assert_eq!(bytes.len(), MAX_FEED_BODY_BYTES);
    }

    #[test]
    fn feed_limits_are_the_agreed_values() {
        assert_eq!(MAX_FEED_BODY_BYTES, 4 * 1024 * 1024);
        assert_eq!(MAX_TITLE_CHARS, 300);
        assert_eq!(MAX_SUMMARY_CHARS, 2_000);
    }

    #[test]
    fn truncates_long_title_and_summary_by_chars() {
        let feed_url = Url::parse("https://rss.example.com/feed.xml").unwrap();
        let long_title = "題".repeat(MAX_TITLE_CHARS + 50);
        let long_summary = "要".repeat(MAX_SUMMARY_CHARS + 50);
        let feed = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Feed</title>
    <link>https://rss.example.com/</link>
    <description>test feed</description>
    <item>
      <title>{long_title}</title>
      <link>https://news.example.com/articles/long</link>
      <description>{long_summary}</description>
    </item>
    <item>
      <title>短いタイトル</title>
      <link>https://news.example.com/articles/short</link>
      <description>短い概要</description>
    </item>
  </channel>
</rss>"#
        );
        let items =
            parse_feed_items(feed.as_bytes(), &feed_url, &allowlist()).expect("feed should parse");

        assert_eq!(items[0].title.chars().count(), MAX_TITLE_CHARS);
        assert!(items[0].title.ends_with("..."));
        let summary = items[0].summary.as_deref().unwrap();
        assert_eq!(summary.chars().count(), MAX_SUMMARY_CHARS);
        assert!(summary.ends_with("..."));
        // 上限以内はそのまま。
        assert_eq!(items[1].title, "短いタイトル");
        assert_eq!(items[1].summary.as_deref(), Some("短い概要"));
    }

    #[test]
    fn truncates_long_atom_title() {
        let feed_url = Url::parse("https://rss.example.com/atom.xml").unwrap();
        let long_title = "題".repeat(MAX_TITLE_CHARS + 1);
        let feed = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>urn:example:feed</id>
  <title>Example Atom</title>
  <updated>2026-06-03T16:00:00Z</updated>
  <entry>
    <id>urn:example:long</id>
    <title>{long_title}</title>
    <link rel="alternate" href="https://news.example.com/articles/atom-long"/>
    <updated>2026-06-03T16:00:00Z</updated>
  </entry>
</feed>"#
        );
        let items = parse_feed_items(feed.as_bytes(), &feed_url, &allowlist())
            .expect("atom feed should parse");

        assert_eq!(items[0].title.chars().count(), MAX_TITLE_CHARS);
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
