//! Feed client for the news ingestion pipeline. Supports RSS 2.0, RSS 1.0 (RDF) and Atom 1.0.
//! Detects the feed format automatically and routes every format
//! through the same fetch, validation, and parsing pipeline.
//! - RSS 1.0（RDF, 判断台帳 D125）は朝日・毎日・時事・ナショジオ日本版・Impress などが使う。
//!   ルート `rdf:RDF` が RSS 1.0 / RDF 構文の名前空間を宣言しているときだけ受け付け、`rss` クレートで
//!   解析する（`<item>` が `<channel>` の兄弟でも子でも読める）。公開日時は `dc:date`（W3CDTF）を使う。
//!
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
    /// RSS 1.0（RDF）。解析は RSS 2.0 と同じ `rss` クレートで行い、公開日時だけ `dc:date` を使う。
    Rdf,
    Atom,
}

/// RSS 1.0 の名前空間。RDF 文書のうち RSS 1.0 フィードだけを受け付けるために使う。
const RSS10_NAMESPACE: &str = "http://purl.org/rss/1.0/";
/// RDF 構文の名前空間。`rdf:` 接頭辞の宣言があることも確認する。
const RDF_SYNTAX_NAMESPACE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// Detects the feed format from the root element.
/// 複数の候補タグが見つかった場合は、文書中で最も手前にあるものをルートとみなす。
/// `rdf:RDF` は開始タグで RSS 1.0 / RDF 構文の名前空間を宣言している場合だけ RSS 1.0 とする
/// （RSS ではない RDF 文書を記事候補にしない）。未対応・判別不能は `None`。
fn detect_feed_format(feed_bytes: &[u8]) -> Option<FeedFormat> {
    let text = String::from_utf8_lossy(feed_bytes);
    let candidates = [
        (find_root_tag(&text, "rss"), FeedFormat::Rss),
        (find_root_tag(&text, "rdf:RDF"), FeedFormat::Rdf),
        (find_root_tag(&text, "feed"), FeedFormat::Atom),
    ];
    let (root_at, format) = candidates
        .into_iter()
        .filter_map(|(position, format)| position.map(|position| (position, format)))
        .min_by_key(|(position, _)| *position)?;

    if format == FeedFormat::Rdf && !declares_rss10_namespaces(&text[root_at..]) {
        return None;
    }
    Some(format)
}

/// `rdf:RDF` の開始タグ（最初の `>` まで）が RSS 1.0 と RDF 構文の名前空間を宣言しているか。
/// RSS 1.0 名前空間は既定（`xmlns=`）でも接頭辞付き（`xmlns:rss=` など）でもよい。
fn declares_rss10_namespaces(root_onwards: &str) -> bool {
    let Some(tag_end) = root_onwards.find('>') else {
        return false;
    };
    let root_tag = &root_onwards[..tag_end];
    root_tag.contains(RSS10_NAMESPACE) && root_tag.contains(RDF_SYNTAX_NAMESPACE)
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
/// Supports RSS 2.0, RSS 1.0 (RDF) and Atom.
fn parse_feed_items(
    feed_bytes: &[u8],
    feed_url: &Url,
    allowlist: &NetworkAllowlist,
) -> Result<Vec<RssItem>, AppError> {
    match detect_feed_format(feed_bytes) {
        Some(format @ (FeedFormat::Rss | FeedFormat::Rdf)) => {
            parse_rss_items(feed_bytes, feed_url, allowlist, format)
        }
        Some(FeedFormat::Atom) => parse_atom_items(feed_bytes, feed_url, allowlist),
        None => Err(AppError::Parse(
            "unsupported or unrecognized feed format (expected RSS 2.0, RSS 1.0 (RDF) or Atom)"
                .to_string(),
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

/// RSS 2.0 と RSS 1.0（RDF）を解析する。`rss` クレートの `Channel::read_from` はルート
/// `rss` / `rdf:RDF` の両方を受け付け、RSS 1.0 で `<channel>` の兄弟に置かれた `<item>` も
/// チャンネルの記事として集める（`<channel>` の子に置くフィードもそのまま読める）。
/// 以降の URL 検証・サニタイズ・重複排除は形式によらず共通。
fn parse_rss_items(
    feed_bytes: &[u8],
    feed_url: &Url,
    allowlist: &NetworkAllowlist,
    format: FeedFormat,
) -> Result<Vec<RssItem>, AppError> {
    let channel = Channel::read_from(Cursor::new(feed_bytes)).map_err(|error| {
        let label = if format == FeedFormat::Rdf {
            "RSS 1.0 (RDF)"
        } else {
            "RSS"
        };
        AppError::Parse(format!("failed to parse {label} feed: {error}"))
    })?;

    let source_name = sanitize_plain_text(channel.title())
        .or_else(|| feed_url.host_str().and_then(sanitize_plain_text))
        .unwrap_or_else(|| "unknown".to_string());

    let mut results = Vec::new();
    let mut seen_urls = HashSet::new();

    for item in channel.items() {
        if let Some(candidate) = map_feed_item(item, &source_name, allowlist, format)? {
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
    format: FeedFormat,
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
    // RSS 2.0 は従来どおり pubDate だけを使う（挙動を変えない）。
    // RSS 1.0 は通常 pubDate を持たないため dc:date を使い、UTC の ISO 8601 へ正規化する。
    let published_at = match format {
        FeedFormat::Rdf => item
            .pub_date()
            .and_then(sanitize_plain_text)
            .or_else(|| rdf_item_date(item)),
        _ => item.pub_date().and_then(sanitize_plain_text),
    };

    Ok(Some(RssItem {
        title,
        article_url,
        summary,
        source_name: source_name.to_string(),
        published_at,
    }))
}

/// RSS 1.0 の記事の `dc:date` を取り出す。
/// `rss` クレートは Dublin Core の名前空間 URI が完全一致するときだけ `dublin_core_ext` に入れるため、
/// URI の表記ゆれ（末尾スラッシュ違いなど）で `dc` 接頭辞の拡張に残った場合も拾う。
fn rdf_item_date(item: &Item) -> Option<String> {
    let raw = item
        .dublin_core_ext()
        .and_then(|dc| dc.dates().first().map(String::as_str))
        .or_else(|| {
            item.extensions()
                .get("dc")
                .and_then(|elements| elements.get("date"))
                .and_then(|values| values.first())
                .and_then(|extension| extension.value())
        })?;
    let sanitized = sanitize_plain_text(raw)?;
    // 解釈できない値は加工せずに渡し、news_service::normalize_published_at の取得時刻フォールバックに任せる。
    Some(normalize_w3cdtf(&sanitized).unwrap_or(sanitized))
}

/// `dc:date`（W3CDTF = ISO 8601 の部分集合）を UTC の `YYYY-MM-DDTHH:MM:SSZ` へ正規化する。
/// RFC3339 に加え、W3CDTF で許される秒なし（`2026-10-13T09:30+09:00`）も受け付ける。
/// 日付だけ（`2026-10-13`）など時差の分からない値は推測せず `None` を返す。
fn normalize_w3cdtf(value: &str) -> Option<String> {
    let to_utc = |datetime: chrono::DateTime<chrono::FixedOffset>| {
        datetime
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(to_utc(datetime));
    }
    // %:z は `Z` を受け付けないため、末尾の `Z` は +00:00 として読む。
    let with_offset = match value.strip_suffix(['Z', 'z']) {
        Some(head) => format!("{head}+00:00"),
        None => value.to_string(),
    };
    chrono::DateTime::parse_from_str(&with_offset, "%Y-%m-%dT%H:%M%:z")
        .ok()
        .map(to_utc)
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

    // ---- RSS 1.0（RDF, D125）----
    // 以下のフィクスチャは日本の報道系 RDF フィードの典型構造を模した合成データ（実フィードは取得しない）。

    fn rdf_allowlist() -> NetworkAllowlist {
        NetworkAllowlist {
            allowed_rss_domains: vec!["rss.example.com".to_string()],
            allowed_article_domains: vec![
                "news.example.com".to_string(),
                "watch.example.com".to_string(),
            ],
            ..NetworkAllowlist::default()
        }
    }

    /// 朝日新聞型: 既定名前空間が RSS 1.0、`<item>` は `<channel>` の兄弟、`dc:date` は秒付きの +09:00。
    const ASAHI_LIKE_RDF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:dc="http://purl.org/dc/elements/1.1/" xml:lang="ja">
  <channel rdf:about="https://rss.example.com/headlines/newsheadlines.rdf">
    <title> 例示新聞 ニュース速報 </title>
    <link>https://news.example.com/</link>
    <description>例示新聞の速報ニュース</description>
    <dc:language>ja</dc:language>
    <items>
      <rdf:Seq>
        <rdf:li rdf:resource="https://news.example.com/articles/A001.html"/>
        <rdf:li rdf:resource="https://news.example.com/articles/A002.html"/>
        <rdf:li rdf:resource="https://evil.example.com/articles/A003.html"/>
      </rdf:Seq>
    </items>
  </channel>
  <item rdf:about="https://news.example.com/articles/A001.html">
    <title>首相、経済対策を指示</title>
    <link>https://news.example.com/articles/A001.html</link>
    <dc:subject>政治</dc:subject>
    <dc:date>2026-10-13T09:30:15+09:00</dc:date>
  </item>
  <item rdf:about="https://news.example.com/articles/A002.html">
    <title>  台風  が接近
      交通に影響 </title>
    <link>https://news.example.com/articles/A002.html</link>
    <description>&lt;p&gt;各地で&lt;b&gt;運休&lt;/b&gt;が相次ぐ。&lt;/p&gt;</description>
    <dc:date>2026-10-13T08:05+09:00</dc:date>
  </item>
  <item rdf:about="https://evil.example.com/articles/A003.html">
    <title>許可リスト外</title>
    <link>https://evil.example.com/articles/A003.html</link>
    <dc:date>2026-10-13T08:00:00+09:00</dc:date>
  </item>
</rdf:RDF>"#;

    /// Impress 型: `content:encoded` と多数の拡張名前空間（dc / content / sy / admin / hatena）を持つ。
    const IMPRESS_LIKE_RDF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF
  xmlns="http://purl.org/rss/1.0/"
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
  xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:content="http://purl.org/rss/1.0/modules/content/"
  xmlns:sy="http://purl.org/rss/1.0/modules/syndication/"
  xmlns:admin="http://webns.net/mvcb/"
  xmlns:hatena="http://www.hatena.ne.jp/info/xmlns#">
  <channel rdf:about="https://rss.example.com/watch/index.rdf">
    <title>例示 Watch</title>
    <link>https://watch.example.com/</link>
    <description>IT 総合ニュース</description>
    <dc:date>2026-10-13T12:00:00+09:00</dc:date>
    <sy:updatePeriod>hourly</sy:updatePeriod>
    <sy:updateFrequency>1</sy:updateFrequency>
    <admin:generatorAgent rdf:resource="https://watch.example.com/generator"/>
    <items>
      <rdf:Seq>
        <rdf:li rdf:resource="https://watch.example.com/docs/news/2001.html"/>
        <rdf:li rdf:resource="https://watch.example.com/docs/news/2002.html"/>
      </rdf:Seq>
    </items>
  </channel>
  <item rdf:about="https://watch.example.com/docs/news/2001.html">
    <title>新型ノート PC が登場</title>
    <link>https://watch.example.com/docs/news/2001.html?ref=rss</link>
    <description>短い説明</description>
    <content:encoded><![CDATA[<p>新型<strong>ノート PC</strong>が発表された。<script>alert(1)</script></p>]]></content:encoded>
    <dc:creator>例示 太郎</dc:creator>
    <dc:subject>PC</dc:subject>
    <dc:date>2026-10-13T11:45:00+09:00</dc:date>
    <hatena:bookmarkcount>3</hatena:bookmarkcount>
  </item>
  <item rdf:about="https://watch.example.com/docs/news/2002.html">
    <title>同じ記事の重複</title>
    <link>https://watch.example.com/docs/news/2001.html?ref=rss</link>
    <dc:date>2026-10-13T11:00:00+09:00</dc:date>
  </item>
</rdf:RDF>"#;

    #[test]
    fn detects_rss10_rdf_format() {
        assert_eq!(
            detect_feed_format(ASAHI_LIKE_RDF.as_bytes()),
            Some(FeedFormat::Rdf)
        );
        assert_eq!(
            detect_feed_format(IMPRESS_LIKE_RDF.as_bytes()),
            Some(FeedFormat::Rdf)
        );
    }

    #[test]
    fn rejects_rdf_document_without_rss10_namespace() {
        // RSS 1.0 名前空間を宣言しない RDF 文書（RSS ではない RDF）は未対応形式として扱う。
        let feed = br#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:foaf="http://xmlns.com/foaf/0.1/">
  <foaf:Person rdf:about="https://news.example.com/people/1"><foaf:name>x</foaf:name></foaf:Person>
</rdf:RDF>"#;
        assert_eq!(detect_feed_format(feed), None);
        let feed_url = Url::parse("https://rss.example.com/index.rdf").unwrap();
        let error = parse_feed_items(feed, &feed_url, &rdf_allowlist())
            .expect_err("non-RSS RDF must be rejected");
        assert!(matches!(error, AppError::Parse(_)));
    }

    #[test]
    fn parses_asahi_like_rdf_with_items_as_channel_siblings() {
        let feed_url = Url::parse("https://rss.example.com/headlines/newsheadlines.rdf").unwrap();
        let items = parse_feed_items(ASAHI_LIKE_RDF.as_bytes(), &feed_url, &rdf_allowlist())
            .expect("RSS 1.0 feed should parse");

        // 許可リスト外の記事は落ち、並び順は保たれる。
        assert_eq!(
            items,
            vec![
                RssItem {
                    title: "首相、経済対策を指示".to_string(),
                    article_url: "https://news.example.com/articles/A001.html".to_string(),
                    summary: None,
                    source_name: "例示新聞 ニュース速報".to_string(),
                    published_at: Some("2026-10-13T00:30:15Z".to_string()),
                },
                RssItem {
                    title: "台風 が接近 交通に影響".to_string(),
                    article_url: "https://news.example.com/articles/A002.html".to_string(),
                    summary: Some("各地で運休が相次ぐ。".to_string()),
                    source_name: "例示新聞 ニュース速報".to_string(),
                    // W3CDTF の秒なし表記も UTC へ正規化する。
                    published_at: Some("2026-10-12T23:05:00Z".to_string()),
                },
            ]
        );
    }

    #[test]
    fn parses_impress_like_rdf_with_content_encoded_and_mixed_namespaces() {
        let feed_url = Url::parse("https://rss.example.com/watch/index.rdf").unwrap();
        let items = parse_feed_items(IMPRESS_LIKE_RDF.as_bytes(), &feed_url, &rdf_allowlist())
            .expect("RSS 1.0 feed with extensions should parse");

        // 同一 URL の 2 件目は重複として落ちる。
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.title, "新型ノート PC が登場");
        assert_eq!(
            item.article_url,
            "https://watch.example.com/docs/news/2001.html?ref=rss"
        );
        // content:encoded を description より優先し、タグは除去する。
        assert_eq!(
            item.summary.as_deref(),
            Some("新型ノート PCが発表された。alert(1)")
        );
        assert!(!item.summary.as_deref().unwrap().contains('<'));
        assert_eq!(item.source_name, "例示 Watch");
        assert_eq!(item.published_at.as_deref(), Some("2026-10-13T02:45:00Z"));
    }

    #[test]
    fn parses_rdf_with_items_inside_channel() {
        // 一部のフィードは RSS 1.0 でも <item> を <channel> の子に置く。
        let feed_url = Url::parse("https://rss.example.com/index.rdf").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel rdf:about="https://rss.example.com/index.rdf">
    <title>Inner Items</title>
    <link>https://news.example.com/</link>
    <description>items inside channel</description>
    <item rdf:about="https://news.example.com/articles/inner-1">
      <title>Inner 1</title>
      <link>https://news.example.com/articles/inner-1</link>
      <dc:date>2026-10-13T10:00:00Z</dc:date>
    </item>
  </channel>
  <item rdf:about="https://news.example.com/articles/outer-1">
    <title>Outer 1</title>
    <link>https://news.example.com/articles/outer-1</link>
  </item>
</rdf:RDF>"#,
            &feed_url,
            &rdf_allowlist(),
        )
        .expect("RDF with items inside channel should parse");

        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].article_url,
            "https://news.example.com/articles/inner-1"
        );
        assert_eq!(
            items[0].published_at.as_deref(),
            Some("2026-10-13T10:00:00Z")
        );
        assert_eq!(
            items[1].article_url,
            "https://news.example.com/articles/outer-1"
        );
        // dc:date がない記事は None（news_service 側で取得時刻へフォールバックする）。
        assert_eq!(items[1].published_at, None);
    }

    #[test]
    fn parses_rdf_with_prefixed_rss_namespace_and_nonstandard_dc_uri() {
        // RSS 1.0 名前空間を既定でなく接頭辞で宣言し、Dublin Core の URI に表記ゆれがあるケース。
        let feed_url = Url::parse("https://rss.example.com/index.rdf").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:rss="http://purl.org/rss/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1">
  <channel rdf:about="https://rss.example.com/index.rdf">
    <title>Mixed NS</title>
    <link>https://news.example.com/</link>
    <description>mixed</description>
  </channel>
  <item rdf:about="https://news.example.com/articles/ns-1">
    <title>NS 1</title>
    <link>https://news.example.com/articles/ns-1</link>
    <dc:date>2026-10-13T09:00+09:00</dc:date>
  </item>
</rdf:RDF>"#,
            &feed_url,
            &rdf_allowlist(),
        )
        .expect("RDF with mixed namespaces should parse");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "NS 1");
        assert_eq!(
            items[0].published_at.as_deref(),
            Some("2026-10-13T00:00:00Z")
        );
    }

    #[test]
    fn rdf_item_link_with_dangerous_scheme_is_dropped() {
        let feed_url = Url::parse("https://rss.example.com/index.rdf").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <channel rdf:about="https://rss.example.com/index.rdf">
    <title>Unsafe</title>
    <link>https://news.example.com/</link>
    <description>unsafe links</description>
  </channel>
  <item rdf:about="javascript:alert(1)">
    <title>JS</title>
    <link>javascript:alert(1)</link>
  </item>
  <item rdf:about="http://127.0.0.1/admin">
    <title>Local</title>
    <link>http://127.0.0.1/admin</link>
  </item>
  <item rdf:about="https://news.example.com/articles/ok">
    <title>OK</title>
    <link>https://news.example.com/articles/ok</link>
  </item>
</rdf:RDF>"#,
            &feed_url,
            &rdf_allowlist(),
        )
        .expect("feed should parse even when some links are rejected");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].article_url, "https://news.example.com/articles/ok");
    }

    #[test]
    fn truncates_long_rdf_title_and_summary() {
        let feed_url = Url::parse("https://rss.example.com/index.rdf").unwrap();
        let long_title = "題".repeat(MAX_TITLE_CHARS + 10);
        let long_summary = "要".repeat(MAX_SUMMARY_CHARS + 10);
        let feed = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <channel rdf:about="https://rss.example.com/index.rdf">
    <title>Long</title>
    <link>https://news.example.com/</link>
    <description>long</description>
  </channel>
  <item rdf:about="https://news.example.com/articles/long">
    <title>{long_title}</title>
    <link>https://news.example.com/articles/long</link>
    <description>{long_summary}</description>
  </item>
</rdf:RDF>"#
        );
        let items = parse_feed_items(feed.as_bytes(), &feed_url, &rdf_allowlist())
            .expect("feed should parse");

        assert_eq!(items[0].title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(
            items[0].summary.as_deref().unwrap().chars().count(),
            MAX_SUMMARY_CHARS
        );
    }

    #[test]
    fn rejects_malformed_rdf_feeds() {
        let feed_url = Url::parse("https://rss.example.com/index.rdf").unwrap();
        let header = r#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#;
        let cases = [
            // 記事の途中で途切れている。
            format!(
                r#"{header}
  <channel rdf:about="x"><title>T</title><link>https://news.example.com/</link><description>d</description></channel>
  <item rdf:about="https://news.example.com/a"><title>A</title><link>https://news.example.com/a</link>"#
            ),
            // 終了タグの対応が崩れている。
            format!(
                r#"{header}
  <channel rdf:about="x"><title>T</title><link>https://news.example.com/</link><description>d</description></channel>
  <item rdf:about="https://news.example.com/a"><title>A</link></item>
</rdf:RDF>"#
            ),
            // <channel> がない。
            format!(
                r#"{header}
  <item rdf:about="https://news.example.com/a"><title>A</title><link>https://news.example.com/a</link></item>
</rdf:RDF>"#
            ),
            // ルートの開始タグが閉じていない。
            r#"<?xml version="1.0"?><rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#""#.to_string(),
        ];

        for (index, feed) in cases.iter().enumerate() {
            let error = parse_feed_items(feed.as_bytes(), &feed_url, &rdf_allowlist())
                .expect_err(&format!("malformed RDF case {index} must fail"));
            assert!(
                matches!(error, AppError::Parse(_)),
                "case {index} should be a parse error, got {error:?}"
            );
        }
    }

    #[test]
    fn normalizes_w3cdtf_dates() {
        assert_eq!(
            normalize_w3cdtf("2026-10-13T09:30:15+09:00").as_deref(),
            Some("2026-10-13T00:30:15Z")
        );
        assert_eq!(
            normalize_w3cdtf("2026-10-13T09:30+09:00").as_deref(),
            Some("2026-10-13T00:30:00Z")
        );
        assert_eq!(
            normalize_w3cdtf("2026-10-13T09:30Z").as_deref(),
            Some("2026-10-13T09:30:00Z")
        );
        assert_eq!(
            normalize_w3cdtf("2026-10-13T09:30:15.250+09:00").as_deref(),
            Some("2026-10-13T00:30:15Z")
        );
        // 時差の分からない値は推測しない。
        assert_eq!(normalize_w3cdtf("2026-10-13"), None);
        assert_eq!(normalize_w3cdtf("not a date"), None);
    }

    #[test]
    fn rdf_dates_flow_through_news_service_normalization() {
        use crate::services::news_service::normalize_published_at;

        let feed_url = Url::parse("https://rss.example.com/headlines/newsheadlines.rdf").unwrap();
        let items = parse_feed_items(ASAHI_LIKE_RDF.as_bytes(), &feed_url, &rdf_allowlist())
            .expect("RSS 1.0 feed should parse");
        let fallback = "2000-01-01T00:00:00Z";
        assert_eq!(
            normalize_published_at(items[0].published_at.as_deref(), fallback),
            "2026-10-13T00:30:15Z"
        );
        assert_eq!(
            normalize_published_at(items[1].published_at.as_deref(), fallback),
            "2026-10-12T23:05:00Z"
        );
        // 解釈できない dc:date は加工せず渡され、取得時刻へフォールバックする。
        assert_eq!(
            normalize_published_at(Some("2026-10-13"), fallback),
            fallback
        );
    }

    #[test]
    fn rss2_ignores_dc_date_to_keep_existing_behavior() {
        // RSS 2.0 の公開日時は従来どおり pubDate だけを見る（dc:date を拾わない）。
        let feed_url = Url::parse("https://rss.example.com/feed.xml").unwrap();
        let items = parse_feed_items(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>Feed</title>
    <link>https://rss.example.com/</link>
    <description>test feed</description>
    <item>
      <title>No pubDate</title>
      <link>https://news.example.com/articles/dc-only</link>
      <dc:date>2026-10-13T09:30:00+09:00</dc:date>
    </item>
  </channel>
</rss>"#,
            &feed_url,
            &allowlist(),
        )
        .expect("RSS 2.0 feed should parse");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].published_at, None);
    }
}
