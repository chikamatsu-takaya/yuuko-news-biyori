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
    collections::{HashMap, HashSet},
    hash::Hash,
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
/// ユーティリティ class とみなす接頭辞（Tailwind の余白・大きさ・位置）。`is_utility_class` 参照。
const UTILITY_CLASS_PREFIXES: &[&str] = &[
    "p", "px", "py", "pt", "pr", "pb", "pl", "ps", "pe", "m", "mx", "my", "mt", "mr", "mb", "ml",
    "ms", "me", "w", "h", "min-w", "max-w", "min-h", "max-h", "size", "gap", "gap-x", "gap-y",
    "space-x", "space-y", "top", "left", "right", "bottom", "inset", "inset-x", "inset-y",
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
/// 1 つのコンテナから集める文字数の目安。抜粋の上限の 2 倍あれば、抜粋の組み立てにも比率の判定にも足りる。
const EXTRACTION_COLLECT_LIMIT_CHARS: usize = MAX_EXCERPT_CHARS * 2;

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

// ---------------------------------------------------------------------------
// 本文抜粋の抽出（判断の流れのまとめ）
//
// 1. 記事コンテナ候補（ARTICLE_CONTAINER_SELECTORS）を順に見る。同じ要素は 1 回だけ扱い、
//    nav / aside / footer / header や、ノイズを表す class / id の内側にある候補（関連記事カード等）は使わない。
// 2. 候補ごとに 1 回の走査で p / li / blockquote / h2 / h3 の文字を集め、2 種類の抜粋を作る。
//    - 判定あり（strict）: 要素名による除外 ＋ class / id の語による除外 ＋ 終端の目印での打ち切り。
//    - 判定なし（lenient）: 要素名による除外だけ（class / id の語と終端の目印は使わない）。
//    どちらも空・画像だけのブロックは飛ばし、短いノイズ文言と重複を除く。
//    集めた文字数が EXTRACTION_COLLECT_LIMIT_CHARS を超えたら走査をやめる（巨大ページの負荷対策）。
// 3. 候補の中で最長の strict と最長の lenient を比べる。strict が空、または lenient の 1/4 未満かつ
//    400 文字未満なら lenient を、それ以外は strict を使う（class / id の判定が本文を落とした場合の保証）。
// 4. 候補から何も取れなければ body で 2〜3 を行う（body 直下側のサイト全体の header も除く）。
// 5. それでも空なら meta description、それも空なら呼び出し側（news_service）が RSS 概要を使う。
// ---------------------------------------------------------------------------

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

/// 上のまとめの 1〜4 を行う。
fn find_best_candidate_excerpt(document: &Html) -> Option<String> {
    let block_selector = Selector::parse("p, li, blockquote, h2, h3").ok()?;
    let mut cache = NoiseCache::new(|element| element.id());
    let mut visited = HashSet::new();
    let mut best_strict: Option<String> = None;
    let mut best_lenient: Option<String> = None;

    for selector_text in ARTICLE_CONTAINER_SELECTORS {
        let selector = match Selector::parse(selector_text) {
            Ok(selector) => selector,
            Err(_) => continue,
        };

        for element in document.select(&selector) {
            // `article` と `main article` のように、同じ要素が複数の候補に当たる場合は 1 回だけ扱う。
            if !visited.insert(element.id()) {
                continue;
            }
            if cache.region_of(&element, None) != Region::Body {
                continue;
            }
            let pair = extract_excerpt_pair(&element, &block_selector, &mut cache);
            keep_longer(&mut best_strict, pair.strict);
            keep_longer(&mut best_lenient, pair.lenient);
        }
    }

    if best_strict.is_some() || best_lenient.is_some() {
        return choose_strict_or_lenient(best_strict, best_lenient);
    }

    let body_selector = Selector::parse("body").ok()?;
    let body = document.select(&body_selector).next()?;
    let pair = extract_excerpt_pair(&body, &block_selector, &mut cache);
    choose_strict_or_lenient(pair.strict, pair.lenient)
}

fn keep_longer(best: &mut Option<String>, candidate: Option<String>) {
    let Some(candidate) = candidate else {
        return;
    };
    let longer = best
        .as_ref()
        .is_none_or(|current| candidate.chars().count() > current.chars().count());
    if longer {
        *best = Some(candidate);
    }
}

/// strict が lenient より極端に短ければ lenient を採る（まとめの 3）。
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

/// 1 つのコンテナから作った判定あり（strict）と判定なし（lenient）の抜粋。
#[derive(Debug, Default)]
struct ExcerptPair {
    strict: Option<String>,
    lenient: Option<String>,
}

/// コンテナ内のブロックを 1 回だけ走査し、strict と lenient の抜粋を作る（まとめの 2）。
/// - 空・画像だけ・空白（&nbsp; を含む）だけのブロックは飛ばす。以前は `?` で抽出全体を
///   捨てていたため、そうした段落が 1 つあるだけで本文が取れず RSS 概要に落ちていた。
/// - strict では「関連記事」などの終端の目印が来たとき、後ろに長い本文の段落がなければ
///   以降を他記事の一覧とみなして打ち切り、あれば目印だけを飛ばして続ける。
fn extract_excerpt_pair<K: Hash + Eq>(
    container: &ElementRef<'_>,
    block_selector: &Selector,
    cache: &mut NoiseCache<K>,
) -> ExcerptPair {
    // strict 用は (テキスト, 長い本文の段落か)。終端の目印の後ろを先読みするため、先に集める。
    let mut strict_candidates: Vec<(String, bool)> = Vec::new();
    let mut lenient_blocks: Vec<String> = Vec::new();
    let mut strict_chars = 0;
    let mut lenient_chars = 0;

    for block in container.select(block_selector) {
        // strict は lenient の部分集合なので、strict が上限に届けば lenient も届いている。
        if strict_chars >= EXTRACTION_COLLECT_LIMIT_CHARS {
            break;
        }
        let region = cache.region_of(&block, Some(container));
        if region == Region::Structural {
            continue;
        }
        let Some(text) = sanitize_plain_text(&block.text().collect::<Vec<_>>().join(" ")) else {
            continue;
        };
        let text_chars = text.chars().count();

        if region == Region::Body {
            let is_long_paragraph = matches!(block.value().name(), "p" | "blockquote")
                && text_chars >= END_MARKER_BODY_PARAGRAPH_CHARS;
            strict_chars += text_chars;
            strict_candidates.push((text.clone(), is_long_paragraph));
        }
        // lenient は比率の判定に足りる分だけ持てばよい。
        if lenient_chars < EXTRACTION_COLLECT_LIMIT_CHARS {
            lenient_chars += text_chars;
            lenient_blocks.push(text);
        }
    }

    let mut strict_blocks = Vec::new();
    for (index, (text, _)) in strict_candidates.iter().enumerate() {
        if is_end_of_article_marker(text) {
            let body_continues = strict_candidates[index + 1..].iter().any(|(_, long)| *long);
            if body_continues {
                continue;
            }
            break;
        }
        strict_blocks.push(text.clone());
    }

    ExcerptPair {
        strict: join_body_blocks(strict_blocks),
        lenient: join_body_blocks(lenient_blocks),
    }
}

/// 短いノイズ文言と重複を除いてつなぐ。何も残らなければ None。
fn join_body_blocks(texts: Vec<String>) -> Option<String> {
    let mut seen = HashSet::new();
    let blocks: Vec<String> = texts
        .into_iter()
        .filter(|text| !is_noise_block(text))
        .filter(|text| seen.insert(text.clone()))
        .collect();
    if blocks.is_empty() {
        return None;
    }
    Some(join_excerpt_blocks(&blocks, MAX_EXCERPT_CHARS))
}

/// ブロック（または候補コンテナ）がどの領域にあるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    /// 本文として扱う。
    Body,
    /// nav / aside / footer / header の内側。strict・lenient の両方で除く。
    Structural,
    /// ノイズを表す class / id の内側。strict だけで除く。
    NoiseClass,
}

/// 要素ごとの判定のメモ。各ブロックから祖先をたどるたびに同じ要素の class を照合し直さないため。
/// キーは `ElementRef::id()`（ego_tree の NodeId）。NodeId 型は依存を増やさずに名前で書けないため、
/// 型引数にして `NoiseCache::new` に渡すキー関数から推論させる（`visited` の HashSet と同じ扱い）。
struct NoiseCache<K: Hash + Eq> {
    elements: HashMap<K, ElementNoise>,
    key_of: fn(&ElementRef<'_>) -> K,
}

#[derive(Debug, Clone, Copy)]
struct ElementNoise {
    /// nav / aside / footer / header のどれか。
    structural_tag: bool,
    is_header: bool,
    /// class / id の語がノイズを表す（html / body / article / main 自身は見ない）。
    noise_class: bool,
}

impl<K: Hash + Eq> NoiseCache<K> {
    fn new(key_of: fn(&ElementRef<'_>) -> K) -> Self {
        Self {
            elements: HashMap::new(),
            key_of,
        }
    }

    /// 要素自身と、その祖先（`stop_at` の手前まで。`stop_at` 自身は判定しない）を見て領域を決める。
    /// `stop_at` が None のときは文書の根までたどる。
    /// `stop_at` が記事コンテナ（body / html 以外）のとき、その内側の `header` は除かない
    /// （記事の `<header>` にはリード文が入ることがあるため。サイト全体のヘッダーは body 直下側にある）。
    fn region_of(&mut self, element: &ElementRef<'_>, stop_at: Option<&ElementRef<'_>>) -> Region {
        let keep_header =
            stop_at.is_some_and(|stop| !matches!(stop.value().name(), "body" | "html"));
        let mut region = Region::Body;
        let mut current = Some(*element);
        while let Some(node) = current {
            if stop_at.is_some_and(|stop| stop.id() == node.id()) {
                break;
            }
            let noise = self.element_noise(&node);
            if noise.structural_tag && !(keep_header && noise.is_header) {
                return Region::Structural;
            }
            if noise.noise_class {
                region = Region::NoiseClass;
            }
            current = node.parent().and_then(ElementRef::wrap);
        }
        region
    }

    fn element_noise(&mut self, element: &ElementRef<'_>) -> ElementNoise {
        let key = (self.key_of)(element);
        *self
            .elements
            .entry(key)
            .or_insert_with(|| classify_element(element))
    }
}

fn classify_element(element: &ElementRef<'_>) -> ElementNoise {
    let value = element.value();
    let name = value.name();
    // html / body / article / main の class は見ない。WordPress 等は記事の `<article>` や `<body>` に
    // `tag-ai` `author-1` `has-sidebar` のような分類用 class を付けるため、本文ごと落としてしまう。
    let noise_class = !CONTAINER_TAGS_EXEMPT_FROM_CLASS_CHECK.contains(&name)
        && value.classes().chain(value.id()).any(is_noise_identifier);
    ElementNoise {
        structural_tag: NOISE_ANCESTOR_TAGS.contains(&name),
        is_header: name == "header",
        noise_class,
    }
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
                // `ad-1` `ad-300x250` `ads-2` のような番号付きの広告枠。余白の `pr-4` などは
                // 先に is_utility_class で除外済みなので、ここに来る数字はユーティリティではない。
                || token.starts_with(|ch: char| ch.is_ascii_digit())
        })
}

/// ユーティリティ class か。Tailwind 等の余白・大きさ・位置の接頭辞（UTILITY_CLASS_PREFIXES）＋`-`＋
/// 数字始まり・`[`始まり・サイズ語の値のときだけ該当とする（`md:` などの前置きと先頭の `-` は外して判定）。
/// `pr-4` `md:pr-8` `mt-2` `px-0.5` `-mt-1` `w-full` `max-w-[60ch]` は該当、
/// `ad-1` `tag-1` `ad-300x250` `ads-2` `ad-hoc` `pr-box` は該当しない（広告枠の番号付き class を残さないため）。
fn is_utility_class(identifier: &str) -> bool {
    let lower = identifier.to_ascii_lowercase();
    // `md:` `hover:` などの前置きを外す。
    let base = lower.rsplit(':').next().unwrap_or(&lower);
    let base = base.strip_prefix('-').unwrap_or(base);
    // 接頭辞は長いもの（`min-w` など）から照合されるよう、一致したもののうち最長を採る。
    let Some(rest) = UTILITY_CLASS_PREFIXES
        .iter()
        .filter_map(|prefix| base.strip_prefix(prefix)?.strip_prefix('-'))
        .min_by_key(|rest| rest.len())
    else {
        return false;
    };
    if rest.starts_with('[') {
        return true;
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
        // 判定が本文を落として日付だけが残った場合、記事コンテナの判定なし抽出を使う（1/4 の保証）。
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
    fn short_article_is_not_replaced_by_huge_navigation() {
        // 比較の基準は記事コンテナの判定なし抽出なので、body の nav が長くても短い記事本文を保つ。
        let menu: String = (0..120)
            .map(|i| format!("<li><a href=\"/c/{i}\">メニュー項目その{i}</a></li>"))
            .collect();
        let html = format!(
            r#"<!doctype html><html><body><nav><ul>{menu}</ul></nav><article><p>{BODY_1}</p><p>{BODY_2}</p></article><footer><p>著作権表示のフッターです。</p></footer></body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        assert_eq!(excerpt.unwrap(), format!("{BODY_1}\n\n{BODY_2}"));
    }

    fn menu_items(count: usize) -> String {
        (0..count)
            .map(|i| format!("<li><a href=\"/c/{i}\">メニュー項目その{i}</a></li>"))
            .collect()
    }

    #[test]
    fn short_article_is_not_replaced_by_nav_inside_main() {
        // lenient でも要素名（nav / aside / footer）の除外は保つので、main の中の nav は混ざらない。
        let menu = menu_items(120);
        let html = format!(
            r#"<!doctype html><html><body><main><nav><ul>{menu}</ul></nav><article><p>{BODY_1}</p><p>{BODY_2}</p></article></main></body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        assert_eq!(excerpt.unwrap(), format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn short_article_does_not_get_aside_inside_main_appended() {
        let menu = menu_items(120);
        let html = format!(
            r#"<!doctype html><html><body><main><article><p>{BODY_1}</p><p>{BODY_2}</p></article><aside><ul>{menu}</ul></aside></main></body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        assert_eq!(excerpt.unwrap(), format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn extraction_stops_collecting_after_enough_text() {
        // 上限の 2 倍を超えたら走査をやめても、抜粋は上限いっぱいまで本文で埋まる。
        let paragraphs: String = (0..2_000)
            .map(|i| format!("<p>段落{i}です。{BODY_1}</p>"))
            .collect();
        let excerpt = extract_body(&paragraphs);
        assert!(excerpt.starts_with("段落0です。"));
        assert!(excerpt.chars().count() > MAX_EXCERPT_CHARS - 100);
        assert!(excerpt.chars().count() <= MAX_EXCERPT_CHARS);
    }

    #[test]
    fn uses_body_lenient_extraction_only_without_container_candidates() {
        // 記事コンテナ候補がないページでは、body の判定あり／なしで比較する。
        let long_body = BODY_1.repeat(6);
        let html = format!(
            r#"<!doctype html><html><body><div class="share-wrapper"><p>{long_body}</p></div><p>2026年10月13日</p></body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        assert!(excerpt.unwrap().contains(&long_body));
    }

    #[test]
    fn utility_class_detection_is_limited_to_spacing_and_size_prefixes() {
        for utility in [
            "pr-4",
            "md:pr-8",
            "mt-2",
            "px-0.5",
            "-mt-1",
            "w-full",
            "max-w-[60ch]",
            "min-w-0",
            "gap-x-2",
            "space-y-4",
            "lg:p-6",
            "top-0",
        ] {
            assert!(is_utility_class(utility), "{utility}");
        }
        for not_utility in [
            "ad-1",
            "tag-1",
            "ad-300x250",
            "ads-2",
            "ad-hoc",
            "pr-box",
            "side-bar",
        ] {
            assert!(!is_utility_class(not_utility), "{not_utility}");
        }
        // 番号付きの広告枠・タグはノイズ、余白の class はノイズではない。
        for noise in ["ad-1", "tag-1", "ad-300x250", "ads-2"] {
            assert!(is_noise_identifier(noise), "{noise}");
        }
        for body in ["pr-4", "md:pr-8", "p-4"] {
            assert!(!is_noise_identifier(body), "{body}");
        }
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

    /// 抽出の所要時間を測る手動実行用（`cargo test extraction_timing_on_extreme_page -- --ignored --nocapture`）。
    /// 約 2.8MB・ブロック数の多いページで、修正前後の比較に使う。時間の閾値は置かない。
    #[test]
    #[ignore]
    fn extraction_timing_on_extreme_page() {
        let mut html =
            String::from("<!doctype html><html><body><header class=\"site-header\"><nav><ul>");
        for i in 0..3_000 {
            html.push_str(&format!(
                "<li><a href=\"/c/{i}\">メニュー項目その{i}</a></li>"
            ));
        }
        html.push_str("</ul></nav></header><main><article class=\"article-body\">");
        for i in 0..12_000 {
            html.push_str(&format!(
                "<div class=\"row pr-4\"><p>段落{i}です。本文のテキストが入ります。料金や提供時期について説明します。</p><ul><li>項目{i}</li></ul></div>"
            ));
        }
        html.push_str("</article><aside class=\"sidebar\"><ul>");
        for i in 0..3_000 {
            html.push_str(&format!("<li>サイドバーの記事見出し{i}</li>"));
        }
        html.push_str("</ul></aside></main></body></html>");
        let started = std::time::Instant::now();
        let excerpt = extract_excerpt(&html);
        eprintln!(
            "extreme page: {} bytes, extraction took {:?}",
            html.len(),
            started.elapsed()
        );
        assert!(excerpt.is_some());
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
