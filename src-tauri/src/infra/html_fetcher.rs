//! HTML fetcher for article excerpt extraction.
//!
//! Security boundary:
//! - article URLs must pass the slice 1 allowlist and scheme guard
//! - resolved IPs must stay on public addresses before connecting
//! - redirects are followed manually and every `Location` hop is re-validated
//! - raw HTML is never returned; only a sanitized excerpt may leave this layer
//! - 受信本文は MAX_HTML_BODY_BYTES まで（超過はこの記事の本文取得失敗として扱う）
#![allow(dead_code)]

use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

use reqwest::{
    header::{HeaderValue, CONTENT_TYPE, LOCATION},
    redirect::Policy,
    Client, Response,
};
use scraper::{ElementRef, Html, Selector};
use url::{Host, Url};

use super::{
    allowlist::NetworkAllowlist,
    http_body::{read_async_response_body_capped, BodyReadError},
    url_guard::{is_disallowed_ip_addr, validate_parsed_url, validate_url, UrlPurpose},
};
use crate::{error::AppError, paths::AppPaths};

const MAX_REDIRECTS: usize = 5;
const REQUEST_TIMEOUT_SECS: u64 = 15;
const HTML_USER_AGENT: &str = "yuuko-news-biyori/0.1";
const MAX_EXCERPT_CHARS: usize = 2_000;
/// 記事HTML応答本文の受信バイト上限（3 MiB）。セキュリティ詳細設計書 §9.2。
/// 通常の記事ページは数百 KiB〜1 MiB 程度のため余裕を取りつつ、巨大応答の全量展開と
/// 解析負荷を防ぐ（常駐負荷・AGENTS.md §2）。
const MAX_HTML_BODY_BYTES: usize = 3 * 1024 * 1024;
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
/// NOISE_PATTERNS を当てるのは、この文字数以下の短いブロックだけにする。
/// 「市場シェアが拡大」「広告事業の売上」のような本文の段落まで落とさないため
/// （ボタン・ラベル・小見出しはこの長さに収まる）。
const NOISE_PATTERN_MAX_BLOCK_CHARS: usize = 30;
/// 本文の外側として扱う要素名。記事コンテナの内側にあっても、ここから下のブロックは拾わない。
/// form は入れない（ページ全体を form で包む CMS があり、本文ごと落とすため）。
const NOISE_ANCESTOR_TAGS: &[&str] = &["nav", "aside", "footer", "header"];
/// class / id による判定を当てない要素名（`is_noise_element` 参照）。
const CONTAINER_TAGS_EXEMPT_FROM_CLASS_CHECK: &[&str] = &["html", "body", "article", "main"];
/// class / id を語に分けたとき（`-` `_` 空白・camelCase で区切り、小文字化）、
/// どれかの語がこの一覧と一致する要素の内側は本文の外として扱う（意味のはっきりした語）。
/// 部分一致にしないのは "stage" の "tag" のような誤判定を避けるため。
const NOISE_CLASS_TOKENS: &[&str] = &[
    "tag",
    "tags",
    "tagcloud",
    "taglist",
    "author",
    "authors",
    "byline",
    "related",
    "recommend",
    "recommended",
    "recommendation",
    "ranking",
    "share",
    "sharing",
    "social",
    "breadcrumb",
    "breadcrumbs",
    "sidebar",
    "comments",
    "newsletter",
    "advertisement",
    "sponsor",
    "sponsored",
];
/// 短く別の意味にもなりやすい語（`pr-4` の pr、`ad-hoc` の ad、`side-by-side` の side など）。
/// class / id の値全体がこの語と一致するか、他の語がすべてノイズ語・部品語
/// （NOISE_CLASS_TOKENS / この一覧 / NOISE_COMPOUND_PARTS）のときだけノイズとみなす。
/// 2 つ目の値が false の語（comment / widget）は、値全体が一致するだけではノイズとしない
/// （`class="comment"` の囲み記事や `widget` で包んだ本文を落とさないため）。
const AMBIGUOUS_NOISE_TOKENS: &[(&str, bool)] = &[
    ("pr", true),
    ("ad", true),
    ("ads", true),
    ("side", true),
    ("sns", true),
    ("follow", true),
    ("profile", true),
    ("comment", false),
    ("widget", false),
];
/// 曖昧な語と組み合わさったときにノイズとみなす部品語（`ad-area` `pr-box` `side-bar` `ad-banner` など）。
const NOISE_COMPOUND_PARTS: &[&str] = &[
    "area",
    "box",
    "bar",
    "banner",
    "block",
    "unit",
    "slot",
    "wrap",
    "wrapper",
    "link",
    "links",
    "list",
    "btn",
    "button",
    "buttons",
    "frame",
    "col",
    "column",
    "inner",
    "container",
    "space",
    "label",
];
/// ユーティリティ class（`pr-4` `md:pr-8` `mt-2` `px-0.5`）の値部分とみなす語。数字始まりも同様。
const UTILITY_SIZE_SUFFIXES: &[&str] = &[
    "px", "auto", "full", "screen", "none", "xs", "sm", "md", "lg", "xl", "2xl",
];
/// 本文の終わりを示す見出し・ラベル。短いブロック（NOISE_PATTERN_MAX_BLOCK_CHARS 以下）が
/// これで始まり、その後ろに長い本文の段落（END_MARKER_BODY_PARAGRAPH_CHARS 以上）がなければ、
/// 以降は他記事の一覧などとみなして抽出を打ち切る。後ろに長い段落があれば、本文途中の囲みとして
/// その目印だけを飛ばして続ける。どの媒体でも意味のはっきりした文言だけに絞る。
const END_OF_ARTICLE_MARKERS: &[&str] = &[
    "関連記事",
    "関連リンク",
    "関連ニュース",
    "あわせて読みたい",
    "合わせて読みたい",
    "おすすめ記事",
    "人気記事",
    "この記事を読んだ人",
    "この記事のタグ",
    "著者プロフィール",
    "Related articles",
    "Related Articles",
];
/// 終端の目印の後ろにこの文字数以上の段落（p / blockquote）があれば、まだ本文が続いているとみなす。
/// 他記事の見出し（li・短い p）はふつうこれより短い。
const END_MARKER_BODY_PARAGRAPH_CHARS: usize = 60;
/// 判定ありの抽出結果が、判定なし（body 全体）の結果の 1/LENIENT_RATIO 未満かつ
/// LENIENT_MAX_STRICT_CHARS 未満なら、判定が本文を落としたとみなして判定なしの結果を使う。
const LENIENT_RATIO: usize = 4;
const LENIENT_MAX_STRICT_CHARS: usize = 400;

/// 抜粋をどこから得たか。本文抽出の失敗をログで追えるようにするために返す
/// （ログには媒体のドメインとこの区分だけを出し、本文は出さない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcerptSource {
    /// 記事本文のブロックから抽出した。
    Body,
    /// 本文が取れず、meta description（og:description 等）で代用した。
    MetaDescription,
    /// 本文も meta description も取れなかった。
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlFetchResult {
    pub excerpt: Option<String>,
    pub html_extracted: bool,
    pub excerpt_source: ExcerptSource,
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
        let (excerpt, excerpt_source) = extract_excerpt_with_source(&html);

        Ok(HtmlFetchResult {
            html_extracted: excerpt.is_some(),
            excerpt,
            excerpt_source,
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
            let mut response = self.send_request(&current_url).await?;
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

            // 本文は受信上限付きで読む。超過はこの記事の本文取得失敗として扱う（URL は載せない）。
            let content_type = response.headers().get(CONTENT_TYPE).cloned();
            let bytes = read_async_response_body_capped(&mut response, MAX_HTML_BODY_BYTES)
                .await
                .map_err(html_body_error)?;
            let body = decode_html_body(bytes, content_type).await?;
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

/// 本文読み取り失敗を固定文言の AppError にする（本文・URL・生のエラー文を含めない）。
fn html_body_error(error: BodyReadError) -> AppError {
    match error {
        BodyReadError::TooLarge => {
            AppError::Network("HTML response body exceeded the receive size limit".to_string())
        }
        BodyReadError::Read => AppError::Network("failed to read HTML response body".to_string()),
    }
}

/// 上限内で受信したバイト列を、従来の `Response::text()` と同じ規則（Content-Type の charset、
/// なければ UTF-8・不正バイトは置換）で文字列にする。
/// Shift_JIS 等の記事を扱うため、依存を増やさず reqwest の文字コード変換をそのまま使う
/// （受信済みバイトから応答を組み直すだけで、追加の通信は発生しない）。
async fn decode_html_body(
    bytes: Vec<u8>,
    content_type: Option<HeaderValue>,
) -> Result<String, AppError> {
    let mut buffered = tauri::http::Response::new(bytes);
    if let Some(content_type) = content_type {
        buffered.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    Response::from(buffered)
        .text()
        .await
        .map_err(|_| AppError::Network("failed to decode HTML response body".to_string()))
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
            // ホスト名・解決先 IP は画面へ返さず debug ログにだけ残す（§16.3）。
            log::debug!("article host '{host}' resolved to disallowed IP '{ip}'");
            return Err(AppError::Validation(
                "article host resolved to a disallowed IP".to_string(),
            ));
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
        log::debug!("redirect Location could not be joined: {error}");
        AppError::Validation("redirect Location is not a valid URL".to_string())
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

fn extract_excerpt(html: &str) -> Option<String> {
    extract_excerpt_with_source(html).0
}

/// 記事HTMLから本文抜粋を取り出す。本文 → meta description の順に試し、どちらも
/// MAX_EXCERPT_CHARS へ切り詰める。生のHTMLは返さず、サニタイズ済みのテキストだけを返す。
fn extract_excerpt_with_source(html: &str) -> (Option<String>, ExcerptSource) {
    let document = Html::parse_document(html);
    if let Some(excerpt) = find_best_candidate_excerpt(&document) {
        return (
            Some(truncate_text(&excerpt, MAX_EXCERPT_CHARS)),
            ExcerptSource::Body,
        );
    }
    match extract_meta_description(&document) {
        Some(excerpt) => (
            Some(truncate_text(&excerpt, MAX_EXCERPT_CHARS)),
            ExcerptSource::MetaDescription,
        ),
        None => (None, ExcerptSource::None),
    }
}

/// 記事コンテナ候補（article / main / 本文用 class 等）からそれぞれ判定ありで抜粋を作り、最も長いものを採る。
/// 関連記事カードの `<article>` のように、ナビゲーション・サイドバー等の内側にある候補は使わない。
/// 候補が全滅したら body 全体から判定ありで取る。
///
/// 保証: 判定（領域除去・終端判定）なしで body 全体から取った抜粋も必ず作り、判定ありの結果が
/// それより極端に短い（1/LENIENT_RATIO 未満かつ LENIENT_MAX_STRICT_CHARS 文字未満、または空）ときは
/// 判定なしの結果を使う。判定が想定外の構造に当たって本文を落とし、日付やパンくずだけが残る
/// 事態を防ぐため。判定なしの結果には修正前と同様にノイズが混ざりうるが、本文は失わない。
fn find_best_candidate_excerpt(document: &Html) -> Option<String> {
    let mut best_excerpt: Option<String> = None;
    let mut best_score = 0;

    for selector_text in ARTICLE_CONTAINER_SELECTORS {
        let selector = match Selector::parse(selector_text) {
            Ok(selector) => selector,
            Err(_) => continue,
        };

        for element in document.select(&selector) {
            if has_noise_ancestor(&element, None) {
                continue;
            }
            if let Some(excerpt) = extract_excerpt_from_element(&element, RegionFilter::Strict) {
                let score = excerpt.chars().count();
                if score > best_score {
                    best_score = score;
                    best_excerpt = Some(excerpt);
                }
            }
        }
    }

    let body_selector = Selector::parse("body").ok()?;
    let body = document.select(&body_selector).next();
    if best_excerpt.is_none() {
        best_excerpt = body
            .as_ref()
            .and_then(|body| extract_excerpt_from_element(body, RegionFilter::Strict));
    }
    let lenient = body
        .as_ref()
        .and_then(|body| extract_excerpt_from_element(body, RegionFilter::Lenient));

    choose_strict_or_lenient(best_excerpt, lenient)
}

/// 判定ありの結果が判定なしの結果より極端に短ければ、判定なしを採る（`find_best_candidate_excerpt` の保証）。
fn choose_strict_or_lenient(strict: Option<String>, lenient: Option<String>) -> Option<String> {
    match (strict, lenient) {
        (Some(strict), Some(lenient)) => {
            let strict_len = strict.chars().count();
            let lenient_len = lenient.chars().count();
            if strict_len < LENIENT_MAX_STRICT_CHARS && strict_len * LENIENT_RATIO < lenient_len {
                Some(lenient)
            } else {
                Some(strict)
            }
        }
        (strict, lenient) => strict.or(lenient),
    }
}

/// 本文の外側の領域（ナビゲーション・タグ一覧・著者紹介等）と終端の目印を判定するかどうか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionFilter {
    /// 判定する（通常）。
    Strict,
    /// 判定しない（保証用の取り直し）。空ブロックの読み飛ばし・短いノイズ文言の除去は行う。
    Lenient,
}

/// コンテナ内の段落・リスト・引用・小見出しを順に集めて抜粋にする。
/// - 空・画像だけ・空白（&nbsp; を含む）だけのブロックは飛ばす。以前は `?` で抽出全体を
///   捨てていたため、そうした段落が 1 つあるだけで本文が取れず RSS 概要に落ちていた。
/// - Strict のとき、ナビゲーション・サイドバー・タグ一覧・著者紹介・共有ボタン等の内側の
///   ブロックは飛ばす。「関連記事」などの終端の目印が来たら、後ろに長い本文の段落がなければ
///   以降を他記事の一覧とみなして打ち切り、あれば目印だけを飛ばして続ける。
fn extract_excerpt_from_element(element: &ElementRef<'_>, filter: RegionFilter) -> Option<String> {
    let block_selector = Selector::parse("p, li, blockquote, h2, h3").ok()?;
    let strict = filter == RegionFilter::Strict;

    // (テキスト, 長い本文の段落か)。終端の目印の後ろを先読みするため、先に集める。
    let mut candidates: Vec<(String, bool)> = Vec::new();
    for block in element.select(&block_selector) {
        if strict && has_noise_ancestor(&block, Some(element)) {
            continue;
        }
        let Some(text) = sanitize_plain_text(&block.text().collect::<Vec<_>>().join(" ")) else {
            continue;
        };
        let is_long_paragraph = matches!(block.value().name(), "p" | "blockquote")
            && text.chars().count() >= END_MARKER_BODY_PARAGRAPH_CHARS;
        candidates.push((text, is_long_paragraph));
    }

    let mut blocks = Vec::new();
    let mut seen = HashSet::new();
    for (index, (text, _)) in candidates.iter().enumerate() {
        if strict && is_end_of_article_marker(text) {
            let body_continues = candidates[index + 1..].iter().any(|(_, long)| *long);
            if body_continues {
                continue;
            }
            break;
        }
        if is_noise_block(text) {
            continue;
        }

        if seen.insert(text.clone()) {
            blocks.push(text.clone());
        }
    }

    if blocks.is_empty() {
        return None;
    }

    Some(join_excerpt_blocks(&blocks, MAX_EXCERPT_CHARS))
}

/// 要素自身か、その祖先（`stop_at` の手前まで。`stop_at` 自身は判定しない）が本文の外側
/// （nav / aside / footer / header、またはタグ一覧・著者紹介・関連記事等の class / id）か。
/// `stop_at` が None のときは文書の根までたどる。
/// `stop_at` が記事コンテナ（body / html 以外）のとき、その内側の `header` は除かない
/// （記事の `<header>` にはリード文が入ることがあるため。サイト全体のヘッダーは body 直下側にある）。
fn has_noise_ancestor(element: &ElementRef<'_>, stop_at: Option<&ElementRef<'_>>) -> bool {
    let keep_header = stop_at.is_some_and(|stop| !matches!(stop.value().name(), "body" | "html"));
    let mut current = Some(*element);
    while let Some(node) = current {
        if stop_at.is_some_and(|stop| stop.id() == node.id()) {
            return false;
        }
        if is_noise_element(&node, keep_header) {
            return true;
        }
        current = node.parent().and_then(ElementRef::wrap);
    }
    false
}

fn is_noise_element(element: &ElementRef<'_>, keep_header: bool) -> bool {
    let value = element.value();
    let name = value.name();
    if NOISE_ANCESTOR_TAGS.contains(&name) && !(keep_header && name == "header") {
        return true;
    }
    // html / body / article / main の class は見ない。WordPress 等は記事の `<article>` や `<body>` に
    // `tag-ai` `author-1` `has-sidebar` のような分類用 class を付けるため、本文ごと落としてしまう。
    if CONTAINER_TAGS_EXEMPT_FROM_CLASS_CHECK.contains(&name) {
        return false;
    }
    value.classes().chain(value.id()).any(is_noise_identifier)
}

/// class / id の値 1 つがノイズ領域を表すか。
/// - ユーティリティ class（`pr-4` `md:pr-8` `mt-2` `px-0.5`）は語の照合をしない。
/// - 意味のはっきりした語（NOISE_CLASS_TOKENS）は、語のどれかが一致すればノイズ。
/// - 曖昧な語（AMBIGUOUS_NOISE_TOKENS）は、値全体が一致するか、他の語がすべてノイズ語・部品語のときだけノイズ。
fn is_noise_identifier(identifier: &str) -> bool {
    if is_utility_class(identifier) {
        return false;
    }
    let tokens = split_identifier_tokens(identifier);
    if tokens
        .iter()
        .any(|token| NOISE_CLASS_TOKENS.contains(&token.as_str()))
    {
        return true;
    }
    let is_ambiguous = |token: &str| {
        AMBIGUOUS_NOISE_TOKENS
            .iter()
            .any(|(word, _)| *word == token)
    };
    if let [single] = tokens.as_slice() {
        return AMBIGUOUS_NOISE_TOKENS
            .iter()
            .any(|(word, whole_ok)| *whole_ok && word == single);
    }
    tokens.iter().any(|token| is_ambiguous(token))
        && tokens.iter().all(|token| {
            is_ambiguous(token)
                || NOISE_CLASS_TOKENS.contains(&token.as_str())
                || NOISE_COMPOUND_PARTS.contains(&token.as_str())
        })
}

/// ユーティリティ class か（`^(?:[a-z0-9-]+:)*-?[a-z]{1,3}-<数字始まり or サイズ語>$` 相当）。
/// `pr-4` `md:pr-8` `mt-2` `px-0.5` `-mt-1` `w-full` は該当、`ad-hoc` `pr-box` `side-bar` は該当しない。
fn is_utility_class(identifier: &str) -> bool {
    let lower = identifier.to_ascii_lowercase();
    // `md:` `hover:` などの前置きを外す。
    let base = lower.rsplit(':').next().unwrap_or(&lower);
    let base = base.strip_prefix('-').unwrap_or(base);
    let Some((head, rest)) = base.split_once('-') else {
        return false;
    };
    if head.is_empty() || head.len() > 3 || !head.chars().all(|ch| ch.is_ascii_lowercase()) {
        return false;
    }
    if rest.is_empty()
        || !rest
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '-'))
    {
        return false;
    }
    rest.starts_with(|ch: char| ch.is_ascii_digit()) || UTILITY_SIZE_SUFFIXES.contains(&rest)
}

/// class / id を語に分ける（`related-posts` → related, posts / `tagList` → tag, list /
/// `colBoxPR` → col, box, pr）。語単位で照合し、部分一致による誤判定を避ける。
fn split_identifier_tokens(identifier: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;

    for ch in identifier.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && prev_lower {
            tokens.push(std::mem::take(&mut current));
        }
        prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        current.push(ch.to_ascii_lowercase());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// 短いブロックが本文の終わりの目印（「関連記事」等）で始まるか。
fn is_end_of_article_marker(text: &str) -> bool {
    text.chars().count() <= NOISE_PATTERN_MAX_BLOCK_CHARS
        && END_OF_ARTICLE_MARKERS
            .iter()
            .any(|marker| text.starts_with(marker))
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

    text.chars().count() <= NOISE_PATTERN_MAX_BLOCK_CHARS
        && NOISE_PATTERNS.iter().any(|pattern| text.contains(pattern))
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
                SocketAddr::new(IpAddr::from([8, 8, 4, 4]), 443),
                SocketAddr::new(IpAddr::from([192, 168, 1, 20]), 443),
            ],
        )
        .expect_err("private address in DNS results must fail closed");

        let message = error.to_string();
        assert!(message.contains("disallowed IP"));
        // ホスト名・解決先 IP は文言へ含めない（§16.3）。
        assert!(!message.contains("news.example.com"));
        assert!(!message.contains("192.168.1.20"));
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

        let message = error.to_string();
        assert!(message.contains("not present in the allowlist"));
        assert!(!message.contains("evil.example.com"));
    }

    #[test]
    fn invalid_redirect_location_message_excludes_parse_details() {
        let current_url = Url::parse("https://news.example.com/articles/1").unwrap();
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

    const BODY_1: &str = "最初の段落です。新しいサービスの提供開始を発表しました。";
    const BODY_2: &str = "二つ目の段落です。料金は従来の半額になるとのことです。";

    fn article_html(inner: &str) -> String {
        format!(
            r#"<!doctype html><html><head>
<meta name="description" content="メタ説明文です。本文が取れないときだけ使う。">
</head><body><article class="article-body">{inner}</article></body></html>"#
        )
    }

    fn extract_body(inner: &str) -> String {
        let (excerpt, source) = extract_excerpt_with_source(&article_html(inner));
        assert_eq!(source, ExcerptSource::Body, "本文から抽出できること");
        excerpt.expect("excerpt")
    }

    #[test]
    fn skips_empty_paragraph_instead_of_aborting() {
        let excerpt = extract_body(&format!("<p>{BODY_1}</p><p></p><p>{BODY_2}</p>"));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn skips_image_only_paragraph_instead_of_aborting() {
        let excerpt = extract_body(&format!(
            r#"<p><img src="https://news.example.com/a.png" alt=""></p><p>{BODY_1}</p><p><a href="/x"><img src="b.png"></a></p><p>{BODY_2}</p>"#
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
        assert!(!excerpt.contains("png"));
    }

    #[test]
    fn skips_nbsp_only_paragraph_instead_of_aborting() {
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><p>&nbsp;</p><p> &nbsp; \u{3000}</p><li> </li><p>{BODY_2}</p>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn drops_trailing_tag_list() {
        let excerpt = extract_body(&format!(
            r#"<p>{BODY_1}</p><p>{BODY_2}</p>
<div class="entry-tags"><ul><li><a href="/t/1">クラウド</a></li><li><a href="/t/2">コンテナ型仮想化</a></li></ul></div>
<ul class="tagList"><li>プログラミング言語 JavaScript / Java</li></ul>
<div id="tagcloud"><p>データベース / 機械学習・AI RDB / NoSQL</p></div>"#
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn drops_author_box_and_follow_prompt() {
        let excerpt = extract_body(&format!(
            r#"<p>{BODY_1}</p><p>{BODY_2}</p>
<div class="author-box"><p>山田 太郎（やまだ たろう） IT系の雑誌編集者を経て独立。</p></div>
<div id="profile"><p>詳しいプロフィールはこちらをご覧ください。</p></div>
<div class="sns-follow"><p>新着情報をチェックしませんか？ Twitterで @example をフォロー</p></div>
<footer><p>Copyright example news. All rights reserved.</p></footer>"#
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn drops_related_article_list() {
        let excerpt = extract_body(&format!(
            r#"<p>{BODY_1}</p><p>{BODY_2}</p>
<section class="related-posts"><h3>この話題の他の記事</h3><ul><li>別の記事の見出しその一が入ります</li></ul></section>
<aside><ul><li>サイドバーの新着記事の見出しが入ります</li></ul></aside>
<nav class="breadcrumb"><ol><li>ホーム</li><li>クラウド</li></ol></nav>"#
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn stops_at_end_of_article_marker() {
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><p>{BODY_2}</p><h2>関連記事</h2><ul><li>他の記事の見出しその一が入ります</li><li>他の記事の見出しその二が入ります</li></ul>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn keeps_body_paragraphs_that_mention_noise_words() {
        // 長い段落は「シェア」「広告」「関連記事」を含んでも本文として残す。
        let share = "同社の国内市場シェアは前年から5ポイント伸び、広告事業も好調だった。";
        let related = "関連記事でも触れたとおり、この機能は昨年から段階的に提供されてきたものだ。";
        let excerpt = extract_body(&format!("<p>{BODY_1}</p><p>{share}</p><p>{related}</p>"));
        assert!(excerpt.contains(share));
        assert!(excerpt.contains(related));
    }

    #[test]
    fn does_not_drop_body_for_classification_classes_on_container() {
        // WordPress の post_class のように、記事要素や body に分類用 class が付いていても本文は落とさない。
        let html = format!(
            r#"<!doctype html><html><body class="single tag-ai has-sidebar">
<div class="site"><article class="post type-post tag-cloud author-1 category-pr"><div class="entry-content"><p>{BODY_1}</p><p>{BODY_2}</p></div></article></div>
</body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        assert_eq!(excerpt.unwrap(), format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn identifier_tokens_split_on_separators_and_camel_case() {
        assert_eq!(
            split_identifier_tokens("related-posts"),
            ["related", "posts"]
        );
        assert_eq!(split_identifier_tokens("tagList"), ["tag", "list"]);
        assert_eq!(split_identifier_tokens("colBoxPR"), ["col", "box", "pr"]);
        // 部分一致では判定しない（stage / inside / header-image は対象外）。
        assert!(!split_identifier_tokens("stage-inside")
            .iter()
            .any(|token| NOISE_CLASS_TOKENS.contains(&token.as_str())));
    }

    #[test]
    fn falls_back_to_unfiltered_body_when_region_filter_removes_everything() {
        // 本文全体が判定対象の class に包まれた想定外の構造でも、修正前と同じく本文は取る。
        let html = format!(
            r#"<!doctype html><html><body><div class="share-wrapper"><p>{BODY_1}</p><p>{BODY_2}</p></div></body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        assert_eq!(excerpt.unwrap(), format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn keeps_body_inside_utility_class_container() {
        // `pr-4`（padding-right）を「PR」と誤認して本文を落とし、日付だけが残ってはいけない。
        let excerpt = extract_body(&format!(
            r#"<div class="prose pr-4"><p>{BODY_1}</p><p>{BODY_2}</p></div><p>2026年10月13日</p>"#
        ));
        assert!(excerpt.contains(BODY_1));
        assert!(excerpt.contains(BODY_2));
    }

    #[test]
    fn keeps_body_inside_ambiguous_but_non_noise_classes() {
        for wrapper in [
            r#"<div class="entry-content md:pr-8">"#,
            r#"<div id="ad-hoc">"#,
            r#"<div class="side-by-side">"#,
            r#"<div class="comment">"#,
            r#"<div class="widget">"#,
        ] {
            let excerpt = extract_body(&format!("{wrapper}<p>{BODY_1}</p><p>{BODY_2}</p></div>"));
            assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"), "{wrapper}");
        }
    }

    #[test]
    fn ambiguous_words_are_noise_only_alone_or_with_noise_parts() {
        for noise in [
            "pr",
            "ad",
            "ad-area",
            "pr-box",
            "side-bar",
            "ad-banner",
            "sns-follow",
        ] {
            assert!(is_noise_identifier(noise), "{noise}");
        }
        for body in [
            "pr-4",
            "md:pr-8",
            "mt-2",
            "px-0.5",
            "ad-hoc",
            "side-by-side",
            "comment",
            "widget",
            "prose",
        ] {
            assert!(!is_noise_identifier(body), "{body}");
        }
        assert!(is_utility_class("md:pr-8"));
        assert!(is_utility_class("-mt-1"));
        assert!(is_utility_class("w-full"));
        assert!(!is_utility_class("pr-box"));
        assert!(!is_utility_class("ad-hoc"));
    }

    #[test]
    fn skips_mid_article_related_box_and_keeps_following_body() {
        let tail = "三つ目の段落です。関連する囲みの後にも本文が続き、この段落は六十文字を超える長さで今後の提供予定と価格について説明しています。";
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><h3>関連記事</h3><ul><li>別の記事の見出しが入ります</li></ul><p>{BODY_2}</p><p>{tail}</p>"
        ));
        assert!(excerpt.contains(BODY_1));
        assert!(excerpt.contains(BODY_2));
        assert!(excerpt.contains(tail));
        assert!(!excerpt.contains("関連記事"));
    }

    #[test]
    fn keeps_lead_paragraph_inside_article_header() {
        let lead = "リード文です。記事の要点を最初にまとめて伝えます。";
        let html = format!(
            r#"<!doctype html><html><body>
<header class="site-header"><p>サイト全体のヘッダーにある説明文です。</p></header>
<article><header><h1>タイトル</h1><p>{lead}</p></header><p>{BODY_1}</p><p>{BODY_2}</p></article>
</body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        let excerpt = excerpt.unwrap();
        assert!(excerpt.starts_with(lead));
        assert!(!excerpt.contains("サイト全体のヘッダー"));
    }

    #[test]
    fn uses_lenient_extraction_when_strict_result_is_far_shorter() {
        // 判定が本文を落として日付だけが残った場合、判定なしの body 抽出を使う（1/4 の保証）。
        let long_body = BODY_1.repeat(6);
        let html = format!(
            r#"<!doctype html><html><body><article><div class="share-wrapper"><p>{long_body}</p><p>{BODY_2}</p></div><p>2026年10月13日</p></article></body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        let excerpt = excerpt.unwrap();
        assert!(excerpt.contains(&long_body));
        assert!(excerpt.contains(BODY_2));
    }

    #[test]
    fn keeps_strict_result_when_it_is_not_far_shorter() {
        // 判定なしとの差がノイズ分だけなら、判定ありの結果を使う。
        assert_eq!(
            choose_strict_or_lenient(Some("あ".repeat(100)), Some("あ".repeat(300))),
            Some("あ".repeat(100))
        );
        assert_eq!(
            choose_strict_or_lenient(Some("あ".repeat(50)), Some("い".repeat(300))),
            Some("い".repeat(300))
        );
        // 絶対値の上限を超える判定ありの結果は、比率に関係なくそのまま使う。
        assert_eq!(
            choose_strict_or_lenient(Some("あ".repeat(450)), Some("い".repeat(2000))),
            Some("あ".repeat(450))
        );
        assert_eq!(
            choose_strict_or_lenient(None, Some("い".to_string())),
            Some("い".to_string())
        );
    }

    #[test]
    fn reports_meta_description_source_when_body_is_missing() {
        let (excerpt, source) =
            extract_excerpt_with_source(&article_html("<p></p><p><img src=\"a.png\"></p>"));
        assert_eq!(source, ExcerptSource::MetaDescription);
        assert_eq!(
            excerpt.unwrap(),
            "メタ説明文です。本文が取れないときだけ使う。"
        );
    }

    #[test]
    fn html_body_limit_is_three_mib() {
        assert_eq!(MAX_HTML_BODY_BYTES, 3 * 1024 * 1024);
    }

    // 組み直した応答は Content-Length を持つため、ここでは読む前の早期拒否経路を確認する。
    // Content-Length のない受信中の上限判定は http_body の append_chunk_capped 単体テストで確認する。
    #[test]
    fn rejects_html_body_over_limit_as_fetch_failure() {
        // 上限 + 1 バイトの本文は TooLarge になり、固定文言の取得失敗（URL・本文なし）へ変換される。
        let body = vec![b'a'; MAX_HTML_BODY_BYTES + 1];
        let mut response = Response::from(tauri::http::Response::new(body));
        let error = tauri::async_runtime::block_on(read_async_response_body_capped(
            &mut response,
            MAX_HTML_BODY_BYTES,
        ))
        .expect_err("over-limit HTML must be rejected");
        assert_eq!(error, BodyReadError::TooLarge);
        let message = html_body_error(error).to_string();
        assert!(message.contains("exceeded the receive size limit"));
        assert!(!message.contains("http"));
    }

    #[test]
    fn accepts_html_body_at_limit() {
        let body = vec![b'a'; MAX_HTML_BODY_BYTES];
        let mut response = Response::from(tauri::http::Response::new(body));
        let bytes = tauri::async_runtime::block_on(read_async_response_body_capped(
            &mut response,
            MAX_HTML_BODY_BYTES,
        ))
        .expect("body exactly at the limit must be accepted");
        assert_eq!(bytes.len(), MAX_HTML_BODY_BYTES);
    }

    #[test]
    fn decodes_html_body_with_declared_charset() {
        // Shift_JIS の「あ」(0x82 0xA0) を、従来の text() と同じく charset に従って復号する。
        let content_type = HeaderValue::from_static("text/html; charset=Shift_JIS");
        let decoded =
            tauri::async_runtime::block_on(decode_html_body(vec![0x82, 0xA0], Some(content_type)))
                .unwrap();
        assert_eq!(decoded, "あ");
        // charset なしは UTF-8。
        let decoded =
            tauri::async_runtime::block_on(decode_html_body("い".as_bytes().to_vec(), None))
                .unwrap();
        assert_eq!(decoded, "い");
    }

    #[test]
    fn truncates_long_excerpt() {
        let long_text = "あ".repeat(MAX_EXCERPT_CHARS + 100);
        let truncated = truncate_text(&long_text, MAX_EXCERPT_CHARS);

        assert_eq!(truncated.chars().count(), MAX_EXCERPT_CHARS);
        assert!(truncated.ends_with("..."));
    }
}
