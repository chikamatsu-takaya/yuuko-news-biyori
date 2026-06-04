//! HTML fetcher for article excerpt extraction.
//!
//! Security boundary:
//! - article URLs must pass the slice 1 allowlist and scheme guard
//! - resolved IPs must stay on public addresses before connecting
//! - redirects are followed manually and every `Location` hop is re-validated
//! - raw HTML is never returned; only a sanitized excerpt may leave this layer
#![allow(dead_code)]

use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

use reqwest::{header::LOCATION, redirect::Policy, Client, Response};
use scraper::{ElementRef, Html, Selector};
use url::{Host, Url};

use super::{
    allowlist::NetworkAllowlist,
    url_guard::{is_disallowed_ip_addr, validate_parsed_url, validate_url, UrlPurpose},
};
use crate::{error::AppError, paths::AppPaths};

const MAX_REDIRECTS: usize = 5;
const REQUEST_TIMEOUT_SECS: u64 = 15;
const HTML_USER_AGENT: &str = "yuuko-news-biyori/0.1";
const MAX_EXCERPT_CHARS: usize = 2_000;
const MIN_BLOCK_CHARS: usize = 3;
const ARTICLE_CONTAINER_SELECTORS: &[&str] = &[
    "article",
    "main article",
    "main",
    "[role='main']",
    ".article-body",
    ".articleBody",
    ".article-content",
    ".article__body",
    ".entry-content",
    ".entry-body",
    ".post-content",
    ".story-body",
    ".content-body",
    ".news-body",
];
const META_DESCRIPTION_SELECTORS: &[&str] = &[
    "meta[property='og:description']",
    "meta[name='description']",
    "meta[name='twitter:description']",
];
const NOISE_PATTERNS: &[&str] = &[
    "続きを読む",
    "スポンサーリンク",
    "広告",
    "シェア",
    "関連記事",
    "ランキング",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlFetchResult {
    pub excerpt: Option<String>,
    pub html_extracted: bool,
}

#[derive(Debug, Clone)]
pub struct HtmlFetcher {
    allowlist_path: PathBuf,
    max_redirects: usize,
    request_timeout: Duration,
}

impl HtmlFetcher {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            allowlist_path: paths.network_allowlist_path.clone(),
            max_redirects: MAX_REDIRECTS,
            request_timeout: Duration::from_secs(REQUEST_TIMEOUT_SECS),
        }
    }

    pub async fn fetch_and_extract(&self, article_url: &str) -> Result<HtmlFetchResult, AppError> {
        let allowlist = NetworkAllowlist::load(&self.allowlist_path)?;
        let article_url =
            canonicalize_request_url(validate_url(article_url, UrlPurpose::Article, &allowlist)?)?;
        let (_final_url, html) = self.fetch_html(article_url, &allowlist).await?;
        let excerpt = extract_excerpt(&html);

        Ok(HtmlFetchResult {
            html_extracted: excerpt.is_some(),
            excerpt,
        })
    }

    async fn fetch_html(
        &self,
        initial_url: Url,
        allowlist: &NetworkAllowlist,
    ) -> Result<(Url, String), AppError> {
        let mut current_url = initial_url;
        let mut visited = HashSet::from([current_url.to_string()]);

        for redirect_count in 0..=self.max_redirects {
            let response = self.send_request(&current_url).await?;
            if response.status().is_redirection() {
                if redirect_count == self.max_redirects {
                    return Err(AppError::Network(format!(
                        "HTML fetch exceeded redirect limit ({})",
                        self.max_redirects
                    )));
                }

                let next_url = resolve_redirect_target(&current_url, &response, allowlist)?;
                let next_key = next_url.to_string();
                if !visited.insert(next_key) {
                    return Err(AppError::Network(
                        "HTML redirect loop detected; refusing to continue".to_string(),
                    ));
                }

                current_url = next_url;
                continue;
            }

            if !response.status().is_success() {
                return Err(AppError::Network(format!(
                    "HTML fetch failed with HTTP status {}",
                    response.status()
                )));
            }

            let body = response.text().await.map_err(|error| {
                AppError::Network(format!("failed to read HTML response body: {error}"))
            })?;
            return Ok((current_url, body));
        }

        Err(AppError::Network(
            "HTML fetch did not complete within the redirect limit".to_string(),
        ))
    }

    async fn send_request(&self, url: &Url) -> Result<Response, AppError> {
        let client = self.build_client_for(url)?;
        client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| AppError::Network(format!("failed to fetch article HTML: {error}")))
    }

    fn build_client_for(&self, url: &Url) -> Result<Client, AppError> {
        let mut builder = Client::builder()
            .redirect(Policy::none())
            .timeout(self.request_timeout)
            .user_agent(HTML_USER_AGENT);

        if let Some(Host::Domain(domain)) = url.host() {
            let resolved_addrs = resolve_public_socket_addrs(domain, url)?;
            builder = builder.resolve_to_addrs(domain, &resolved_addrs);
        }

        builder.build().map_err(|error| {
            AppError::Network(format!("failed to build HTML HTTP client: {error}"))
        })
    }
}

fn resolve_public_socket_addrs(host: &str, url: &Url) -> Result<Vec<SocketAddr>, AppError> {
    let port = url.port_or_known_default().ok_or_else(|| {
        AppError::Validation("article URL must use a known http/https port".to_string())
    })?;
    let resolved = (host, port).to_socket_addrs().map_err(|error| {
        AppError::Network(format!("failed to resolve article host '{host}': {error}"))
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
                "article host '{host}' resolved to disallowed IP '{ip}'"
            )));
        }

        if seen.insert(ip) {
            validated.push(SocketAddr::new(ip, 0));
        }
    }

    if validated.is_empty() {
        return Err(AppError::Network(format!(
            "article host '{host}' did not resolve to any public IP addresses"
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
    validate_parsed_url(&next_url, UrlPurpose::Article, allowlist)?;
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

fn extract_excerpt(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    find_best_candidate_excerpt(&document)
        .or_else(|| extract_meta_description(&document))
        .map(|excerpt| truncate_text(&excerpt, MAX_EXCERPT_CHARS))
}

fn find_best_candidate_excerpt(document: &Html) -> Option<String> {
    let mut best_excerpt = None;
    let mut best_score = 0;

    for selector_text in ARTICLE_CONTAINER_SELECTORS {
        let selector = match Selector::parse(selector_text) {
            Ok(selector) => selector,
            Err(_) => continue,
        };

        for element in document.select(&selector) {
            if let Some(excerpt) = extract_excerpt_from_element(&element) {
                let score = excerpt.chars().count();
                if score > best_score {
                    best_score = score;
                    best_excerpt = Some(excerpt);
                }
            }
        }
    }

    if best_excerpt.is_some() {
        return best_excerpt;
    }

    let body_selector = Selector::parse("body").ok()?;
    document
        .select(&body_selector)
        .find_map(|body| extract_excerpt_from_element(&body))
}

fn extract_excerpt_from_element(element: &ElementRef<'_>) -> Option<String> {
    let block_selector = Selector::parse("p, li, blockquote, h2, h3").ok()?;
    let mut blocks = Vec::new();
    let mut seen = HashSet::new();

    for block in element.select(&block_selector) {
        let text = sanitize_plain_text(&block.text().collect::<Vec<_>>().join(" "))?;
        if is_noise_block(&text) {
            continue;
        }

        if seen.insert(text.clone()) {
            blocks.push(text);
        }
    }

    if blocks.is_empty() {
        return None;
    }

    Some(join_excerpt_blocks(&blocks, MAX_EXCERPT_CHARS))
}

fn extract_meta_description(document: &Html) -> Option<String> {
    for selector_text in META_DESCRIPTION_SELECTORS {
        let selector = match Selector::parse(selector_text) {
            Ok(selector) => selector,
            Err(_) => continue,
        };

        if let Some(content) = document
            .select(&selector)
            .filter_map(|element| element.value().attr("content"))
            .find_map(sanitize_plain_text)
        {
            if !is_noise_block(&content) {
                return Some(content);
            }
        }
    }

    None
}

fn join_excerpt_blocks(blocks: &[String], limit: usize) -> String {
    let mut excerpt = String::new();

    for block in blocks {
        let next_len = if excerpt.is_empty() {
            block.chars().count()
        } else {
            excerpt.chars().count() + 2 + block.chars().count()
        };

        if next_len > limit {
            break;
        }

        if !excerpt.is_empty() {
            excerpt.push_str("\n\n");
        }
        excerpt.push_str(block);
    }

    if excerpt.is_empty() {
        truncate_text(&blocks.join("\n\n"), limit)
    } else {
        excerpt
    }
}

fn is_noise_block(text: &str) -> bool {
    if text.chars().count() < MIN_BLOCK_CHARS {
        return true;
    }

    NOISE_PATTERNS.iter().any(|pattern| text.contains(pattern))
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

fn truncate_text(value: &str, max_chars: usize) -> String {
    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return value.to_string();
    }

    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
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
            allowed_article_domains: vec!["news.example.com".to_string()],
            ..NetworkAllowlist::default()
        }
    }

    #[test]
    fn rejects_private_dns_results() {
        let error = validate_resolved_socket_addrs(
            "news.example.com",
            vec![
                SocketAddr::new(IpAddr::from([203, 0, 113, 42]), 443),
                SocketAddr::new(IpAddr::from([192, 168, 1, 20]), 443),
            ],
        )
        .expect_err("private address in DNS results must fail closed");

        assert!(error.to_string().contains("disallowed IP"));
    }

    #[test]
    fn resolves_relative_redirects_through_url_guard() {
        let current_url = Url::parse("https://news.example.com/articles/1").unwrap();
        let next_url = resolve_redirect_location(&current_url, "/articles/latest", &allowlist())
            .expect("redirect should pass");

        assert_eq!(
            next_url.as_str(),
            "https://news.example.com/articles/latest"
        );
    }

    #[test]
    fn rejects_redirects_to_non_allowlisted_hosts() {
        let current_url = Url::parse("https://news.example.com/articles/1").unwrap();
        let error = resolve_redirect_location(
            &current_url,
            "https://evil.example.com/articles/1",
            &allowlist(),
        )
        .expect_err("redirect must stay inside the article allowlist");

        assert!(error.to_string().contains("not present in the allowlist"));
    }

    #[test]
    fn extracts_primary_article_excerpt() {
        let excerpt = extract_excerpt(
            r#"
<!doctype html>
<html>
  <head>
    <title>Example</title>
    <meta name="description" content="fallback description">
  </head>
  <body>
    <aside>
      <p>広告</p>
    </aside>
    <article class="article-body">
      <h2>見出し</h2>
      <p>最初の段落です。本文の導入を説明します。</p>
      <script>alert("ignore")</script>
      <p>二つ目の段落です。重要な背景情報をここに含めます。</p>
      <p>シェア</p>
    </article>
  </body>
</html>
"#,
        )
        .expect("article excerpt should be extracted");

        assert!(excerpt.contains("見出し"));
        assert!(excerpt.contains("最初の段落です。本文の導入を説明します。"));
        assert!(excerpt.contains("二つ目の段落です。重要な背景情報をここに含めます。"));
        assert!(!excerpt.contains("alert"));
        assert!(!excerpt.contains("シェア"));
    }

    #[test]
    fn falls_back_to_meta_description_when_body_text_is_missing() {
        let excerpt = extract_excerpt(
            r#"
<!doctype html>
<html>
  <head>
    <meta property="og:description" content="本文がなくても説明文だけは取得する。">
  </head>
  <body>
    <div></div>
  </body>
</html>
"#,
        )
        .expect("meta description should be used as fallback");

        assert_eq!(excerpt, "本文がなくても説明文だけは取得する。");
    }

    #[test]
    fn returns_none_when_no_meaningful_content_exists() {
        let excerpt = extract_excerpt(
            r#"
<!doctype html>
<html>
  <body>
    <article><p>広告</p><p>シェア</p></article>
  </body>
</html>
"#,
        );

        assert!(excerpt.is_none());
    }

    #[test]
    fn truncates_long_excerpt() {
        let long_text = "あ".repeat(MAX_EXCERPT_CHARS + 100);
        let truncated = truncate_text(&long_text, MAX_EXCERPT_CHARS);

        assert_eq!(truncated.chars().count(), MAX_EXCERPT_CHARS);
        assert!(truncated.ends_with("..."));
    }
}
