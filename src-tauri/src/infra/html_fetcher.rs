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
    "この記事の著者",
    "この記事の筆者",
    "この記事の執筆者",
    "この記事を書いた人",
    "Related articles",
    "Related Articles",
];
/// 本文の先頭・末尾に付く操作ボタンのラベル（ブロック全体がこの文言と一致するときだけ）。
/// 「3D プリンターで印刷する技術」のような本文の文を落とさないため、部分一致にしない。
const EDGE_ACTION_LABELS: &[&str] = &[
    "印刷する",
    "印刷",
    "印刷用ページ",
    "この記事を印刷",
    "この記事を印刷する",
];
/// 本文の末尾に並ぶサイト案内のラベル（ブロック全体がこの文言と一致するときだけ）。
/// 末尾から続く短いラベルの並びの中にあるときだけ除き、本文の途中にあれば残す。
const TRAILING_SITE_NAV_LABELS: &[&str] = &[
    "お問い合わせ",
    "お問合せ",
    "お問合わせ",
    "問い合わせ",
    "利用規約",
    "プライバシーポリシー",
    "個人情報保護方針",
    "会社概要",
    "運営会社",
    "サイトマップ",
    "広告掲載",
    "ページトップへ",
    "ページの先頭へ",
    "このページの先頭へ",
    "トップへ戻る",
];
/// 末尾のサイト案内のうち「ITmedia NEWS SNS」のような SNS への案内とみなす行の最大文字数。
const TRAILING_SNS_LABEL_MAX_CHARS: usize = 20;
/// 著作権表示の行（「Copyright © …」「(C) …」「… All Rights Reserved.」）とみなす最大文字数。
/// 著作権表示に一言の注記（生成 AI の利用の断りなど）が続く行もこの長さに収まり、
/// 著作権に触れる本文の段落はふつうこれより長い。末尾の並びの中にあるときだけ除く
/// （サイト全体の著作権表示の強い形だけは本文の途中でも除く。`is_site_copyright_notice`）。
const COPYRIGHT_LINE_MAX_CHARS: usize = 150;
/// 終端の目印の後ろにこの文字数以上の段落（p / blockquote）があれば、まだ本文が続いているとみなす。
/// 他記事の見出し（li・短い p）はふつうこれより短い。
const END_MARKER_BODY_PARAGRAPH_CHARS: usize = 60;
/// 判定ありの抽出結果が、判定なし（body 全体）の結果の 1/LENIENT_RATIO 未満かつ
/// LENIENT_MAX_STRICT_CHARS 未満なら、判定が本文を落としたとみなして判定なしの結果を使う。
const LENIENT_RATIO: usize = 4;
const LENIENT_MAX_STRICT_CHARS: usize = 400;
/// 1 つのコンテナから集める文字数の目安。抜粋の上限の 2 倍あれば、抜粋の組み立てにも比率の判定にも足りる。
const EXTRACTION_COLLECT_LIMIT_CHARS: usize = MAX_EXCERPT_CHARS * 2;
/// 【…】で始まる誘導・告知の行（「【画像】…」「【あわせて読みたい】…」）。この文字数以下の
/// ブロックが BRACKET_NOISE_LABELS のどれかの見出しで始まるときだけ除く（strict のみ）。
/// 誘導行は見出し＋他記事のタイトル程度の長さに収まり、本文の段落はふつうこれより長い。
const BRACKET_NOISE_MAX_BLOCK_CHARS: usize = 100;
/// 誘導・告知を表す隅付き括弧の見出し。「【速報】」「【独自】」のような本文の見出しは入れない。
const BRACKET_NOISE_LABELS: &[&str] = &[
    "あわせて読みたい",
    "合わせて読みたい",
    "関連記事",
    "関連",
    "関連リンク",
    "画像",
    "写真",
    "動画",
    "PR",
    "広告",
    "元記事に戻る",
    "元記事で読む",
    "おすすめ",
];
/// PR の見出し（BRACKET_NOISE_LABELS の "PR"）の前にこの文字数以上の本文があれば、
/// その見出しから後ろは広告の囲みとみなして打ち切る（記事末尾の配信サービス告知など）。
/// 前に本文がほとんどない（記事自体が PR 記事の）ときは見出しの行だけを除く。
const PR_SECTION_MIN_PRECEDING_CHARS: usize = 200;
/// 寄付・会員登録・メルマガなどの呼びかけを除く対象の長さ（strict のみ）。
/// 本文中で寄付や会員登録に触れる長い段落は残すため、短いブロックに限る。
const APPEAL_MAX_BLOCK_CHARS: usize = 300;
/// 呼びかけの話題を表す語。APPEAL_CALL_MARKERS と両方を含むときだけ呼びかけとみなす。
const APPEAL_KEYWORDS: &[&str] = &[
    "寄付",
    "支援のお願い",
    "ご支援",
    "会員登録",
    "メルマガ",
    "メールマガジン",
    "ニュースレター",
];
/// 読者へ直接行動を促す、呼びかけに特有の言い回し。「寄付を呼びかけた」のような報道の文には現れにくいものに絞る。
/// 「今すぐ」「お願いします」は記事の引用に、単独の「はこちら」「こちらから」は「詳細はこちらから確認できる」の
/// ような報道の文にも出るため入れない（「登録はこちら」「購読はこちら」のように呼びかけの語と一体のものだけ入れる）。
const APPEAL_CALL_MARKERS: &[&str] = &[
    "ボタンから",
    "読者の皆さま",
    "読者の皆様",
    "読者のみなさま",
    "ご登録ください",
    "ぜひご登録",
    "登録はこちら",
    "購読はこちら",
];
/// 1 ブロックの文字のうちリンク文字がこの割合以上なら「リンクが大半のブロック」とみなす。
const LINK_DENSE_BLOCK_RATIO: f64 = 0.8;
/// リンクが大半のブロックがこの数以上連続したら、関連記事・参考リンクなどの一覧とみなして除く（strict のみ）。
/// 単独のリンク段落（「詳しくは公式サイト」など）は残す。
const LINK_LIST_MIN_RUN: usize = 2;
/// ページ送り（「1 2 >」）とみなすブロックの最大文字数。
const PAGINATION_MAX_BLOCK_CHARS: usize = 20;
/// 記事コンテナ候補の比較では、候補内のリンク文字の割合だけ長さを割り引く
/// （長さ × (1 − リンク密度)。他記事カード・広告カードはリンクがほとんどを占める）。
/// 候補のリンク密度がこの値以上なら、短い抜粋の差し替え（段落のかたまり）を許す。
const LINK_DENSE_CANDIDATE_RATIO: f64 = 0.5;
/// 段落のかたまりへの差し替えを考える「短い抜粋」の上限。
const CLUSTER_SWITCH_MAX_CHOSEN_CHARS: usize = 400;
/// 段落のかたまりに差し替えるには、その抜粋がこの文字数以上で、選んだ抜粋の CLUSTER_SWITCH_RATIO 倍以上、
/// 直下の段落が CLUSTER_MIN_PARAGRAPHS 個以上あること（同意画面などの短い説明文に差し替えないため）。
const CLUSTER_MIN_CHARS: usize = 300;
/// li の中の候補を「文字がごく少ないカード」とみなす上限（見出しと媒体名だけのカード程度）。
const CARD_MAX_TEXT_CHARS: usize = 100;
/// 【PR】の囲みを祖先へ広げるときの文字数の上限（空白を除く）。これを超える祖先は本文を包んでいるとみなし、
/// 囲みに含めない（記事末尾の配信サービス告知は 2 件並んでも 1,000 文字程度に収まる）。
const PR_BOX_MAX_CHARS: usize = 1_000;
/// 【PR】の囲みが直下に持ってよい長い段落の数。これより多い祖先は本文の区画とみなして囲みに含めない。
const PR_BOX_MAX_DIRECT_PARAGRAPHS: usize = 2;
const CLUSTER_SWITCH_RATIO: usize = 3;
const CLUSTER_MIN_PARAGRAPHS: usize = 3;

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
//    strict ではさらに、テキストだけで分かる定型ブロックを除く（いずれも短いブロックに限る）:
//    「【画像】」「【あわせて読みたい】」などの誘導行、本文の後ろの「【PR】」の囲み（後ろに本文が続けば囲みだけ）、
//    リンクが大半のブロックの連続（関連記事・参考リンクの一覧）、寄付・会員登録・メルマガの呼びかけ、ページ送り、
//    「Copyright …」「… All Rights Reserved.」のサイトの著作権表示。最後に、選んだブロックの先頭の「印刷する」と、
//    末尾から続く短いラベル（著作権表示・「この記事の著者」・「お問い合わせ」「ITmedia NEWS SNS」などのサイト案内）を除く。
//    lenient には足さない（判定が想定外に当たったときの保険として、修正前と同じ規則のまま残す）。
// 3. 候補ごとのリンク密度（リンク文字 / 全文字）で長さを割り引き、割り引き後に最長の strict と最長の lenient を
//    比べる。strict が空、または lenient の 1/4 未満かつ 400 文字未満なら lenient を、それ以外は strict を使う
//    （class / id の判定が本文を落とした場合の保証）。リンク密度が 0 の候補どうしでは修正前と同じ選び方になる。
// 4. 候補から何も取れなければ body で 2〜3 を行う（body 直下側のサイト全体の header も除く）。
// 5. 3 の抜粋が短く（400 文字未満）、その候補がカード（リンク密度 0.5 以上か、一覧の li の中にあってリンクを含むか
//    100 文字未満の他記事・広告のカード）のときだけ、「段落のかたまり」（直下の p の文字数が最も多い要素。readability 系の
//    親要素の採点と同じ考え方）も 2 の規則で抽出し、十分に長ければ（300 文字以上かつ 3 倍以上、段落 3 個以上）、
//    かつ候補とかたまりの共通の祖先が body / html より内側にあるときに差し替える。カードでない短い記事や 4 の body の結果は差し替えない（同意バナー・サイト案内・次の記事など、
//    近くの無関係な長い文章で正しい短い記事を置き換えないため。リードだけの article の兄弟にある本文も拾わない）。
// 6. それでも空なら meta description、それも空なら呼び出し側（news_service）が RSS 概要を使う。
//
// 対象外: 本文が別 URL・別ページにあるもの（RSS のリンク先が要約ページで全文は別 URL、2 ページ目以降）。
// 追加の取得が要るため、この関数では扱わない。
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

/// 抽出に使うセレクタ一式（1 回の抽出で 1 度だけ組み立てる）。
struct BlockSelectors {
    blocks: Selector,
    links: Selector,
}

/// 割り引き後の長さで比べている最良の抜粋と、それを出した候補要素。
struct BestExcerpt<'a> {
    text: String,
    score: f64,
    element: ElementRef<'a>,
    link_density: f64,
}

/// 上のまとめの 1〜5 を行う。
fn find_best_candidate_excerpt(document: &Html) -> Option<String> {
    let selectors = BlockSelectors {
        blocks: Selector::parse("p, li, blockquote, h2, h3").ok()?,
        links: Selector::parse("a").ok()?,
    };
    let mut cache = NoiseCache::new(|element| element.id());
    let mut visited = HashSet::new();
    let mut best_strict: Option<BestExcerpt> = None;
    let mut best_lenient: Option<BestExcerpt> = None;

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
            let pair = extract_excerpt_pair(&element, &selectors, &mut cache);
            keep_best(&mut best_strict, pair.strict, element, pair.link_density);
            keep_best(&mut best_lenient, pair.lenient, element, pair.link_density);
        }
    }

    if best_strict.is_none() && best_lenient.is_none() {
        // 記事コンテナ候補がないページは body で比べる。body はページ全体なので、かたまりへの差し替えはしない。
        let body_selector = Selector::parse("body").ok()?;
        let body = document.select(&body_selector).next()?;
        let pair = extract_excerpt_pair(&body, &selectors, &mut cache);
        return choose_strict_or_lenient(pair.strict, pair.lenient);
    }
    let (chosen, anchor, anchor_link_density) = choose_best_pair(best_strict, best_lenient)?;

    // まとめの 5: 選んだ抜粋が短く、選んだ候補がカードらしいときだけ、本文は別の場所にあるとみなして
    // 段落のかたまりを調べる。カードでない短い記事は、近くに長い文章（同意バナー・サイト案内・次の記事）が
    // あっても差し替えない（短い記事そのものが本文でありうるため）。
    let chosen_chars = chosen.chars().count();
    if chosen_chars >= CLUSTER_SWITCH_MAX_CHOSEN_CHARS
        || !looks_like_card(&anchor, anchor_link_density, chosen_chars)
    {
        return Some(chosen);
    }
    let Some((cluster, paragraphs)) = find_paragraph_cluster(document, &selectors, &mut cache)
    else {
        return Some(chosen);
    };
    if paragraphs < CLUSTER_MIN_PARAGRAPHS || !shares_content_ancestor(&anchor, &cluster) {
        return Some(chosen);
    }
    let pair = extract_excerpt_pair(&cluster, &selectors, &mut cache);
    match choose_strict_or_lenient(pair.strict, pair.lenient) {
        Some(cluster_text)
            if should_switch_to_cluster(chosen.chars().count(), cluster_text.chars().count()) =>
        {
            Some(cluster_text)
        }
        _ => Some(chosen),
    }
}

/// 候補が他記事・広告のカードらしいか。リンク密度が LINK_DENSE_CANDIDATE_RATIO 以上か、
/// 一覧の li の中にあって、リンクを含むか文字がごく少ない（CARD_MAX_TEXT_CHARS 未満）とき。
/// li の中というだけではカードとしない（タイムライン・無限スクロールのページは本物の短信も li で包むため）。
fn looks_like_card(candidate: &ElementRef<'_>, link_density: f64, text_chars: usize) -> bool {
    if link_density >= LINK_DENSE_CANDIDATE_RATIO {
        return true;
    }
    let in_list_item = candidate
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|node| node.value().name() == "li");
    in_list_item && (link_density > 0.0 || text_chars < CARD_MAX_TEXT_CHARS)
}

/// 候補とかたまりの最も近い共通の祖先が body / html より内側にあるか（同じ本文領域の中にあるか）。
/// body 直下で別々に置かれた同意バナー・サイト案内などへ差し替えないため。
fn shares_content_ancestor(candidate: &ElementRef<'_>, cluster: &ElementRef<'_>) -> bool {
    let candidate_ancestors: HashSet<_> = candidate.ancestors().map(|node| node.id()).collect();
    cluster
        .ancestors()
        .find(|node| candidate_ancestors.contains(&node.id()))
        .and_then(ElementRef::wrap)
        .is_some_and(|common| !matches!(common.value().name(), "body" | "html"))
}

/// 短い抜粋（chosen）を段落のかたまりの抜粋（cluster）へ差し替えるか（長さの条件だけ）。
fn should_switch_to_cluster(chosen_chars: usize, cluster_chars: usize) -> bool {
    chosen_chars < CLUSTER_SWITCH_MAX_CHOSEN_CHARS
        && cluster_chars >= CLUSTER_MIN_CHARS
        && cluster_chars >= chosen_chars.saturating_mul(CLUSTER_SWITCH_RATIO)
}

/// 直下の p（本文の領域にあり、リンクが大半でないもの）の文字数の合計が最も多い要素と、その段落数を返す。
/// 記事コンテナ用の class が付いていない本文（CSS Modules の自動命名など）や、`article` の外に置かれた本文を拾うため。
fn find_paragraph_cluster<'a, K: Hash + Eq>(
    document: &'a Html,
    selectors: &BlockSelectors,
    cache: &mut NoiseCache<K>,
) -> Option<(ElementRef<'a>, usize)> {
    let paragraph_selector = Selector::parse("body p").ok()?;
    // 親要素ごとの (要素, 文字数, 段落数)。キーは要素の id（NoiseCache と同じく型は推論させる）。
    let mut scores = HashMap::new();
    for paragraph in document.select(&paragraph_selector) {
        if cache.region_of(&paragraph, None) != Region::Body {
            continue;
        }
        let (chars, link_chars) = text_and_link_chars(&paragraph, &selectors.links);
        if chars == 0 || is_link_dense(chars, link_chars) {
            continue;
        }
        let Some(parent) = paragraph.parent().and_then(ElementRef::wrap) else {
            continue;
        };
        let entry = scores
            .entry(parent.id())
            .or_insert((parent, 0usize, 0usize));
        entry.1 += chars;
        entry.2 += 1;
    }
    scores
        .into_values()
        .max_by_key(|(_, chars, _)| *chars)
        .map(|(element, _, paragraphs)| (element, paragraphs))
}

/// 候補の長さをリンク密度で割り引いた値（長さ × (1 − リンク密度)）。
fn link_weighted_score(text: &str, link_density: f64) -> f64 {
    text.chars().count() as f64 * (1.0 - link_density)
}

fn keep_best<'a>(
    best: &mut Option<BestExcerpt<'a>>,
    candidate: Option<String>,
    element: ElementRef<'a>,
    link_density: f64,
) {
    let Some(text) = candidate else {
        return;
    };
    let score = link_weighted_score(&text, link_density);
    if best.as_ref().is_none_or(|current| score > current.score) {
        *best = Some(BestExcerpt {
            text,
            score,
            element,
            link_density,
        });
    }
}

/// 最良の strict と lenient から抜粋を選び（まとめの 3）、その抜粋を出した候補要素とリンク密度を添えて返す。
fn choose_best_pair<'a>(
    strict: Option<BestExcerpt<'a>>,
    lenient: Option<BestExcerpt<'a>>,
) -> Option<(String, ElementRef<'a>, f64)> {
    let strict_text = strict.as_ref().map(|best| best.text.clone());
    let lenient_text = lenient.as_ref().map(|best| best.text.clone());
    let chosen = choose_strict_or_lenient(strict_text, lenient_text)?;
    let source = match (strict, lenient) {
        (Some(strict), _) if strict.text == chosen => strict,
        (_, Some(lenient)) => lenient,
        (Some(strict), None) => strict,
        (None, None) => return None,
    };
    Some((chosen, source.element, source.link_density))
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
    /// lenient で集めたブロックのリンク文字の割合（0.0〜1.0）。候補の比較で長さを割り引くのに使う。
    link_density: f64,
}

/// strict 用に集めたブロック。終端の目印・リンク一覧の判定で前後を見るため、先に集めてから選ぶ。
struct StrictBlock<'a> {
    /// ブロックの要素（【PR】の囲みの範囲を DOM で判定するのに使う）。
    element: ElementRef<'a>,
    text: String,
    /// 長い本文の段落（p / blockquote で END_MARKER_BODY_PARAGRAPH_CHARS 以上）か。
    is_long_paragraph: bool,
    /// リンク文字が大半（LINK_DENSE_BLOCK_RATIO 以上）か。
    is_link_dense: bool,
}

/// コンテナ内のブロックを 1 回だけ走査し、strict と lenient の抜粋を作る（まとめの 2）。
/// - 空・画像だけ・空白（&nbsp; を含む）だけのブロックは飛ばす。以前は `?` で抽出全体を
///   捨てていたため、そうした段落が 1 つあるだけで本文が取れず RSS 概要に落ちていた。
/// - strict では「関連記事」などの終端の目印が来たとき、後ろに長い本文の段落がなければ
///   以降を他記事の一覧とみなして打ち切り、あれば目印だけを飛ばして続ける。
/// - strict ではテキストだけで分かる定型ブロック（`select_strict_blocks` 参照）も除く。
fn extract_excerpt_pair<'a, K: Hash + Eq>(
    container: &ElementRef<'a>,
    selectors: &BlockSelectors,
    cache: &mut NoiseCache<K>,
) -> ExcerptPair {
    let mut strict_candidates: Vec<StrictBlock> = Vec::new();
    let mut lenient_blocks: Vec<String> = Vec::new();
    let mut strict_chars = 0;
    let mut lenient_chars = 0;
    // リンク密度は空白を除いた文字数で数える（lenient に入れたブロックだけ）。
    let mut lenient_dense_chars = 0usize;
    let mut lenient_link_chars = 0usize;

    for block in container.select(&selectors.blocks) {
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
        let (dense_chars, link_chars) = text_and_link_chars(&block, &selectors.links);

        if region == Region::Body {
            let is_long_paragraph = matches!(block.value().name(), "p" | "blockquote")
                && text_chars >= END_MARKER_BODY_PARAGRAPH_CHARS;
            strict_chars += text_chars;
            strict_candidates.push(StrictBlock {
                element: block,
                text: text.clone(),
                is_long_paragraph,
                is_link_dense: is_link_dense(dense_chars, link_chars),
            });
        }
        // lenient は比率の判定に足りる分だけ持てばよい。
        if lenient_chars < EXTRACTION_COLLECT_LIMIT_CHARS {
            lenient_chars += text_chars;
            lenient_dense_chars += dense_chars;
            lenient_link_chars += link_chars;
            lenient_blocks.push(text);
        }
    }

    let link_density = if lenient_dense_chars == 0 {
        0.0
    } else {
        (lenient_link_chars.min(lenient_dense_chars) as f64) / lenient_dense_chars as f64
    };
    ExcerptPair {
        strict: join_body_blocks(select_strict_blocks(
            container,
            &strict_candidates,
            selectors,
        )),
        lenient: join_body_blocks(lenient_blocks),
        link_density,
    }
}

/// strict の抜粋に入れるブロックを選ぶ。次の順に判定する。
/// 1. 終端の目印（「関連記事」など）: 後ろに長い段落がなければ打ち切り、あれば目印だけ飛ばす。
/// 2. 「【PR】」の見出し: 前に本文が PR_SECTION_MIN_PRECEDING_CHARS 未満なら（PR 記事の冒頭）見出しだけ飛ばす。
///    以上なら、見出しを含む囲み（`pr_section_box`）の外に長い段落が続くかを先読みし、続かなければ以降を打ち切り、
///    続けば囲みの中のブロックだけを飛ばす（囲みの要素がない平らな構造では見出しの行だけを飛ばすことになる）。
/// 3. 「【画像】」「【あわせて読みたい】」などの誘導行、寄付・会員登録の呼びかけ、ページ送り、
///    サイトの著作権表示（`is_site_copyright_notice`。後ろに連載の紹介文などが続いて末尾にない場合）は飛ばす。
/// 4. リンクが大半のブロックが LINK_LIST_MIN_RUN 個以上続く部分（リンク一覧）は飛ばす。
/// 5. 選び終えたら、先頭・末尾の短いラベルを除く（`trim_edge_label_blocks`）。
fn select_strict_blocks(
    container: &ElementRef<'_>,
    candidates: &[StrictBlock],
    selectors: &BlockSelectors,
) -> Vec<String> {
    let mut selected = Vec::new();
    let mut selected_chars = 0;
    // 本文途中の【PR】の囲み。この要素の内側のブロックは飛ばす。
    let mut skipped_pr_box: Option<ElementRef> = None;
    for (index, block) in candidates.iter().enumerate() {
        let text = &block.text;
        if skipped_pr_box.is_some_and(|pr_box| is_inside(&block.element, &pr_box)) {
            continue;
        }
        if is_end_of_article_marker(text) {
            let body_continues = candidates[index + 1..]
                .iter()
                .any(|later| later.is_long_paragraph);
            if body_continues {
                continue;
            }
            break;
        }
        if is_pr_section_header(text) {
            if selected_chars < PR_SECTION_MIN_PRECEDING_CHARS || index == 0 {
                continue;
            }
            let pr_box = pr_section_box(
                container,
                &block.element,
                &candidates[index - 1].element,
                selectors,
            );
            let body_continues = candidates[index + 1..]
                .iter()
                .any(|later| later.is_long_paragraph && !is_inside(&later.element, &pr_box));
            if !body_continues {
                break;
            }
            skipped_pr_box = Some(pr_box);
            continue;
        }
        if is_bracket_noise_line(text)
            || is_appeal_block(text)
            || is_pagination_block(text)
            || is_site_copyright_notice(text)
        {
            continue;
        }
        if block.is_link_dense && is_in_link_run(candidates, index) {
            continue;
        }
        selected_chars += text.chars().count();
        selected.push(text.clone());
    }
    trim_edge_label_blocks(&mut selected);
    selected
}

/// 選んだブロックの先頭・末尾にある短いラベルを除く（strict のみ。まとめの 2）。
/// - 先頭: 「印刷する」などの操作ボタンのラベル（EDGE_ACTION_LABELS と完全に一致するブロック）が続く間。
/// - 末尾: 操作ボタンのラベル・著作権表示・著者のラベル・サイト案内のラベル（`is_trailing_label_block`）が
///   続く間。末尾から見て最初の本文のブロックで止めるので、本文の途中にある同じ文言は残す。
///
/// どれも短いブロックの完全一致か行頭の定型だけを見るため、本文の段落は落ちない。すべて除いて空になったときは
/// 抜粋なしとなり、判定なしの抜粋（このラベルを除かない）が使われる（まとめの 3 の保証）。
fn trim_edge_label_blocks(blocks: &mut Vec<String>) {
    let leading = blocks
        .iter()
        .take_while(|text| is_edge_action_label(text))
        .count();
    blocks.drain(..leading);
    while blocks
        .last()
        .is_some_and(|text| is_trailing_label_block(text))
    {
        blocks.pop();
    }
}

/// 「印刷する」のような操作ボタンのラベルだけのブロックか。
fn is_edge_action_label(text: &str) -> bool {
    EDGE_ACTION_LABELS.contains(&text.trim())
}

/// 本文の末尾に並ぶ短いラベルか（操作ボタン・著作権表示・著者のラベル・サイト案内）。
fn is_trailing_label_block(text: &str) -> bool {
    let text = text.trim();
    is_edge_action_label(text)
        || is_copyright_line(text)
        || is_author_label(text)
        || TRAILING_SITE_NAV_LABELS.contains(&text)
        || is_sns_nav_label(text)
}

/// 「この記事の著者」のような著者欄の見出しだけのブロックか（終端の目印と同じ文言の完全一致）。
/// 終端の目印は後ろに長い段落があると目印だけを飛ばすため、末尾ではここでも除く。
fn is_author_label(text: &str) -> bool {
    matches!(
        text,
        "この記事の著者"
            | "この記事の筆者"
            | "この記事の執筆者"
            | "この記事を書いた人"
            | "著者プロフィール"
            | "著者"
            | "筆者"
            | "執筆者"
    )
}

/// 著作権表示の行か（末尾の並びの中で使う）。COPYRIGHT_LINE_MAX_CHARS 以下で、英文の文（`is_sentence_like_english`）
/// でなく、次のどれかに当たるときだけ。
/// - 「©」で始まる。
/// - 「Copyright」「(C)」「（C）」で始まり、直後が空白・©・括弧・英数字（「Copyright」は英字を除く）で、
///   その後ろ（印自身は除く）が著作権表示の形（`has_copyright_notice_shape`: ©・「(C)」・4 桁の年・会社を表す語）をしている
///   （「Copyright © ITmedia, Inc. …」「Copyright 2026 Example Inc.」「(C) 2026 Example」）。
///   「Copyright Officeは…」「Copyright law remains unsettled …」「(C) は回答しなかった」は当たらない。
/// - 「All Rights Reserved」（大文字小文字は問わない。末尾の「.」は除く）で終わる。
fn is_copyright_line(text: &str) -> bool {
    if text.chars().count() > COPYRIGHT_LINE_MAX_CHARS {
        return false;
    }
    if text.starts_with('©') {
        return true;
    }
    if is_sentence_like_english(text) {
        return false;
    }
    if let Some(rest) = text.strip_prefix("Copyright") {
        let starts_like_notice = rest.chars().next().is_none_or(|ch| {
            ch.is_whitespace() || ch.is_ascii_digit() || matches!(ch, '©' | '(' | '（')
        });
        if starts_like_notice && has_copyright_notice_shape(rest) {
            return true;
        }
    }
    for prefix in COPYRIGHT_C_MARKS {
        if let Some(rest) = text.strip_prefix(prefix) {
            let starts_like_notice = rest
                .chars()
                .next()
                .is_none_or(|ch| ch.is_whitespace() || ch.is_ascii_alphanumeric());
            if starts_like_notice && has_copyright_notice_shape(rest) {
                return true;
            }
        }
    }
    ends_with_rights_reserved(text)
}

/// 「(C)」の書き方。
const COPYRIGHT_C_MARKS: [&str; 4] = ["(C)", "(c)", "（C）", "（c）"];
/// 著作権表示に付く会社を表す語（英語は語単位で照合する）。
const COPYRIGHT_COMPANY_WORDS: &[&str] = &["Inc", "Ltd", "Co", "Corp", "LLC", "Company"];
const COPYRIGHT_COMPANY_JA: &[&str] = &["株式会社", "有限会社"];

/// 著作権表示の形か。©・「(C)」・4 桁の年（19xx / 20xx）・会社を表す語のどれかを含む。
fn has_copyright_notice_shape(text: &str) -> bool {
    if text.contains('©') || COPYRIGHT_C_MARKS.iter().any(|mark| text.contains(mark)) {
        return true;
    }
    if COPYRIGHT_COMPANY_JA.iter().any(|word| text.contains(word)) {
        return true;
    }
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|word| {
            COPYRIGHT_COMPANY_WORDS.contains(&word)
                || (word.len() == 4
                    && word.chars().all(|ch| ch.is_ascii_digit())
                    && (word.starts_with("19") || word.starts_with("20")))
        })
}

/// 「All Rights Reserved」（大文字小文字は問わない。末尾の「.」は除く）で終わるか。
fn ends_with_rights_reserved(text: &str) -> bool {
    text.trim_end_matches('.')
        .to_ascii_lowercase()
        .ends_with("all rights reserved")
}

/// 英文の文らしいか。小文字で始まる英単語（定型の all / rights / reserved / by は数えない）が 2 つ以上ある。
/// 著作権表示は社名・年・定型句でできていて小文字の語をほとんど含まないため、
/// 「Copyright law remains unsettled …」「The studio said all rights reserved」のような本文の文と分けられる。
fn is_sentence_like_english(text: &str) -> bool {
    text.split(|ch: char| !ch.is_ascii_alphabetic())
        .filter(|word| {
            word.chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase())
                && !matches!(*word, "all" | "rights" | "reserved" | "by")
        })
        .count()
        >= 2
}

/// サイト全体の著作権表示の行か（本文の途中でも飛ばしてよい強い形だけ）。著作権表示の行
/// （`is_copyright_line`）のうち、「Copyright」で始まり © /「(C)」を含むもの（「Copyright © ITmedia, Inc. All Rights Reserved.」）。
/// 「All Rights Reserved」で終わるだけの行（引用したライセンス文・英文）、「© Reuters」「(C) 2026 Example」のような
/// 写真・図のクレジットは本文の途中に置かれうるため、末尾の並びでだけ除く。
fn is_site_copyright_notice(text: &str) -> bool {
    is_copyright_line(text)
        && text.strip_prefix("Copyright").is_some_and(|rest| {
            rest.contains('©') || COPYRIGHT_C_MARKS.iter().any(|mark| rest.contains(mark))
        })
}

/// 「ITmedia NEWS SNS」「公式SNS」のような SNS への案内のラベルか（末尾の並びの中だけで使う）。
/// 「SNS」の前がサイト名らしい英数字の名前（空白区切り）か「公式」のときだけ当てる。
/// 「若者の間で広がるSNS」のような本文の短い行は当たらない。
fn is_sns_nav_label(text: &str) -> bool {
    if text.chars().count() > TRAILING_SNS_LABEL_MAX_CHARS {
        return false;
    }
    let Some(name) = text.strip_suffix("SNS") else {
        return false;
    };
    let name = name.trim_end();
    if name.is_empty() || name.ends_with("公式") {
        return true;
    }
    // サイト名＋空白＋SNS（「ITmedia NEWS SNS」）。空白を挟まない「…広がるSNS」は本文の語とみなす。
    text[..text.len() - "SNS".len()].ends_with(char::is_whitespace)
        && name.chars().any(|ch| ch.is_ascii_alphanumeric())
        && name.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || ch.is_whitespace() || matches!(ch, '.' | '-' | '&')
        })
}

/// 【PR】の見出しを含む囲みの要素。見出し自身から祖先へたどり、次のどれかに当たる手前で止める（コンテナ自身は含めない）。
/// - 直前のブロック（本文）を含む祖先。
/// - 長い段落（p / blockquote で END_MARKER_BODY_PARAGRAPH_CHARS 以上）を直下に 3 つ以上持つ祖先。見出しが本文の段落と
///   同じ親に並ぶ構造（本文を包む section の先頭に【PR】の行がある等）では、その親を囲みにすると後ろの本文を落とすため。
/// - 文字数が PR_BOX_MAX_CHARS を超える祖先（本文を包んでいるとみなす）。
///
/// 見出しと本文が同じ親に並ぶ平らな構造では見出し自身になる。
fn pr_section_box<'a>(
    container: &ElementRef<'_>,
    header: &ElementRef<'a>,
    previous: &ElementRef<'_>,
    selectors: &BlockSelectors,
) -> ElementRef<'a> {
    let mut pr_box = *header;
    for node in header.ancestors().filter_map(ElementRef::wrap) {
        if node.id() == container.id()
            || is_inside(previous, &node)
            || has_direct_long_paragraph(&node, &pr_box)
            || text_and_link_chars(&node, &selectors.links).0 > PR_BOX_MAX_CHARS
        {
            break;
        }
        pr_box = node;
    }
    pr_box
}

/// 直下の子（`path` 自身を除く。見出しや、そこまでに広げた囲み）に長い段落
/// （p / blockquote で END_MARKER_BODY_PARAGRAPH_CHARS 以上）が PR_BOX_MAX_DIRECT_PARAGRAPHS より多くあるか。
/// 宣伝の囲みは見出し＋数段落程度で、それより多い段落を直接並べる要素は本文の区画とみなす。
fn has_direct_long_paragraph(element: &ElementRef<'_>, path: &ElementRef<'_>) -> bool {
    element
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|child| {
            child.id() != path.id()
                && matches!(child.value().name(), "p" | "blockquote")
                && child
                    .text()
                    .map(|text| text.chars().filter(|ch| !ch.is_whitespace()).count())
                    .sum::<usize>()
                    >= END_MARKER_BODY_PARAGRAPH_CHARS
        })
        .count()
        > PR_BOX_MAX_DIRECT_PARAGRAPHS
}

/// element が ancestor 自身か、その内側にあるか。
fn is_inside(element: &ElementRef<'_>, ancestor: &ElementRef<'_>) -> bool {
    element.id() == ancestor.id() || element.ancestors().any(|node| node.id() == ancestor.id())
}

/// index のブロックを含む「リンクが大半のブロック」の連続が LINK_LIST_MIN_RUN 個以上か。
fn is_in_link_run(candidates: &[StrictBlock], index: usize) -> bool {
    let before = candidates[..index]
        .iter()
        .rev()
        .take_while(|block| block.is_link_dense)
        .count();
    let after = candidates[index + 1..]
        .iter()
        .take_while(|block| block.is_link_dense)
        .count();
    before + 1 + after >= LINK_LIST_MIN_RUN
}

/// 要素内の文字数とリンク（a 要素）内の文字数を、空白を除いて数える。
fn text_and_link_chars(element: &ElementRef<'_>, link_selector: &Selector) -> (usize, usize) {
    let count = |texts: &mut dyn Iterator<Item = &str>| -> usize {
        texts
            .map(|text| text.chars().filter(|ch| !ch.is_whitespace()).count())
            .sum()
    };
    let chars = count(&mut element.text());
    let link_chars = element
        .select(link_selector)
        .map(|link| count(&mut link.text()))
        .sum::<usize>();
    // a の中に a があるような壊れた HTML でも 1.0 を超えないようにする。
    (chars, link_chars.min(chars))
}

fn is_link_dense(chars: usize, link_chars: usize) -> bool {
    chars > 0 && (link_chars as f64) >= (chars as f64) * LINK_DENSE_BLOCK_RATIO
}

/// 【…】（または［…］[…]）の見出しを外した残りを返す。見出しが BRACKET_NOISE_LABELS に当たらなければ None。
fn strip_bracket_noise_label(text: &str) -> Option<(&str, &str)> {
    for (open, close) in [('【', '】'), ('［', '］'), ('[', ']')] {
        let Some(rest) = text.strip_prefix(open) else {
            continue;
        };
        let (label, after) = rest.split_once(close)?;
        let label = label.trim();
        if BRACKET_NOISE_LABELS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(label))
        {
            return Some((label, after));
        }
        return None;
    }
    None
}

/// 「【画像】…」「【あわせて読みたい】…」のような短い誘導・告知の行か。
fn is_bracket_noise_line(text: &str) -> bool {
    text.chars().count() <= BRACKET_NOISE_MAX_BLOCK_CHARS
        && strip_bracket_noise_label(text).is_some()
}

/// 「【PR】…」で始まる短い見出しか（PR の囲みの始まり）。
fn is_pr_section_header(text: &str) -> bool {
    text.chars().count() <= BRACKET_NOISE_MAX_BLOCK_CHARS
        && strip_bracket_noise_label(text)
            .is_some_and(|(label, _)| label.eq_ignore_ascii_case("PR"))
}

/// 寄付・会員登録・メルマガなどの短い呼びかけか。話題の語と、読者へ行動を促す言い回しの両方を含むときだけ。
fn is_appeal_block(text: &str) -> bool {
    text.chars().count() <= APPEAL_MAX_BLOCK_CHARS
        && APPEAL_KEYWORDS.iter().any(|word| text.contains(word))
        && APPEAL_CALL_MARKERS
            .iter()
            .any(|marker| text.contains(marker))
}

/// 「1 2 >」「< 前へ 1 2 3 次へ >」のようなページ送りか（数字を含み、数字・空白・矢印・前へ／次へだけでできている）。
fn is_pagination_block(text: &str) -> bool {
    if text.chars().count() > PAGINATION_MAX_BLOCK_CHARS
        || !text.chars().any(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    let rest = text.replace("次へ", "").replace("前へ", "");
    rest.chars().all(|ch| {
        ch.is_ascii_digit()
            || ch.is_whitespace()
            || matches!(
                ch,
                '<' | '>' | '‹' | '›' | '«' | '»' | '＜' | '＞' | '…' | '|'
            )
    })
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

    // --- 定型ブロックの除去（HJwKSM）。各規則に、本文の段落が残ることを確かめる否定のテストを付ける。

    /// 60 文字以上の本文段落（終端の目印・PR の判定で「長い段落」になる長さ）。
    const LONG_BODY: &str = "三つ目の段落です。同社は来年春までに全国の店舗へ新しい仕組みを広げる計画で、利用者の反応を見ながら料金体系も見直すとしています。";

    #[test]
    fn drops_short_bracket_guide_lines() {
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><p>【画像】 イメチェンした姿はこちら</p><p>【写真】会見の様子</p><p>{BODY_2}</p>\
<p>【関連記事】別の話題の記事の見出しが入ります</p><p>【関連】もう一つの記事の見出し</p><p>{LONG_BODY}</p>\
<p>【あわせて読みたい】 他の記事のタイトルが入ります。反響の声も</p><p>［PR］スポンサーの告知です</p>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}\n\n{LONG_BODY}"));
    }

    #[test]
    fn keeps_bracket_lines_that_are_not_guides() {
        // 見出しが一覧にない（【速報】）・長い段落・途中の【PR】は本文として残す。
        let breaking = "【速報】政府は午後の会見で、新たな支援策を発表しました。";
        let long_photo = format!("【写真】{}", LONG_BODY.repeat(2));
        let inline_pr =
            "この記事は【PR】の表記がある広告記事とは異なり、編集部が独自に取材したものです。";
        let excerpt = extract_body(&format!(
            "<p>{breaking}</p><p>{long_photo}</p><p>{inline_pr}</p><p>{BODY_1}</p>"
        ));
        assert!(excerpt.contains(breaking));
        assert!(excerpt.contains(&long_photo));
        assert!(excerpt.contains(inline_pr));
        assert!(excerpt.contains(BODY_1));
    }

    #[test]
    fn stops_at_pr_section_after_body() {
        // 本文の後ろの【PR】の囲みは、囲みの中に長い段落があっても、囲みの外に本文が続かなければ以降を打ち切る。
        let body = format!(
            r#"<div class="entry"><p>{LONG_BODY}</p><p>{BODY_1}</p><p>{LONG_BODY}1</p><p>{BODY_2}</p><p>{LONG_BODY}2</p></div>"#
        );
        let pr = r#"<div class="free-block"><div class="kakomi"><p>【PR】配信サービスでリーグ戦を楽しもう！</p></div><div class="kakomi"><p>このプランは、スポーツの配信に特化したお得なプランで、月額料金は通常よりも安く、学生向けの割引もあって、今なら初月が無料になります。</p><ol><li>① 全試合をライブで配信！</li></ol></div></div><p>By 編集部</p>"#;
        let excerpt = extract_body(&format!("{body}{pr}"));
        assert!(excerpt.contains(BODY_2));
        assert!(excerpt.ends_with(&format!("{LONG_BODY}2")));
        assert!(!excerpt.contains("配信"));
        assert!(!excerpt.contains("By 編集部"));
    }

    #[test]
    fn skips_only_pr_box_when_body_continues_after_it() {
        // 本文途中の【PR】の囲みは囲みだけを飛ばし、囲みの外に続く長い本文段落は残す。
        let body = format!(
            "<p>{LONG_BODY}</p><p>{BODY_1}</p><p>{LONG_BODY}1</p><p>{BODY_2}</p><p>{LONG_BODY}3</p>"
        );
        let pr_box = r#"<div class="kakomi"><p>【PR】配信サービスのお知らせ</p><p>このプランは、スポーツの配信に特化したお得なプランで、月額料金は通常よりも安く、学生向けの割引もあって、今なら初月が無料になります。</p></div>"#;
        let excerpt = extract_body(&format!("{body}{pr_box}<p>{LONG_BODY}2</p>"));
        assert!(excerpt.ends_with(&format!("{LONG_BODY}2")));
        assert!(!excerpt.contains("配信"));
    }

    #[test]
    fn keeps_body_after_pr_line_at_top_of_wrapping_element() {
        // 【PR】の行が、残りの本文を包む要素（section / div.inner）の先頭にあっても、後ろの本文は落とさない。
        let first: String = (0..6)
            .map(|i| format!("<p>前半{i}。{LONG_BODY}</p>"))
            .collect();
        let rest: String = (0..10)
            .map(|i| format!("<p>後半{i}。{LONG_BODY}</p>"))
            .collect();
        for html in [
            format!(
                "<!doctype html><html><body><article><section>{first}</section><section><p>【PR】スポンサーのお知らせ</p>{rest}</section></article></body></html>"
            ),
            format!(
                r#"<!doctype html><html><body><article><div class="body">{first}<div class="inner"><p>【PR】スポンサーのお知らせ</p>{rest}</div></div></article></body></html>"#
            ),
        ] {
            let excerpt = extract_excerpt(&html).unwrap();
            assert!(excerpt.contains("後半0。"), "{html}");
            assert!(excerpt.contains("後半9。") || excerpt.chars().count() > 1_500);
            assert!(!excerpt.contains("スポンサー"));
        }
    }

    #[test]
    fn keeps_body_when_pr_box_would_wrap_long_text() {
        // 見出しだけを包む要素から広げても、本文（長い文章）を包む祖先までは囲みにしない。
        let rest: String = (0..20)
            .map(|i| format!("<div><p>後半{i}。{LONG_BODY}</p></div>"))
            .collect();
        let html = format!(
            r#"<!doctype html><html><body><article><div class="lead"><p>{LONG_BODY}</p><p>{LONG_BODY}1</p><p>{LONG_BODY}2</p><p>{LONG_BODY}3</p></div><div class="main"><div class="kakomi"><p>【PR】スポンサーのお知らせ</p></div>{rest}</div></article></body></html>"#
        );
        let excerpt = extract_excerpt(&html).unwrap();
        assert!(excerpt.contains("後半0。"));
        assert!(!excerpt.contains("スポンサー"));
    }

    #[test]
    fn skips_only_pr_line_in_flat_body() {
        // 囲みの要素がなく本文と同じ親に並ぶ【PR】の行は、後ろに長い段落が続けば行だけを飛ばす。
        let body = format!("<p>{LONG_BODY}</p><p>{BODY_1}</p><p>{LONG_BODY}1</p><p>{BODY_2}</p>");
        let excerpt = extract_body(&format!(
            "{body}<p>【PR】スポンサーのお知らせ</p><p>{LONG_BODY}2</p>"
        ));
        assert!(excerpt.ends_with(&format!("{BODY_2}\n\n{LONG_BODY}2")));
        assert!(!excerpt.contains("スポンサー"));
    }

    #[test]
    fn keeps_body_after_pr_header_at_top_of_sponsored_article() {
        // 記事自体が PR 記事（見出しが先頭）のときは、見出しの行だけを除いて本文は残す。
        let excerpt = extract_body(&format!(
            "<p>【PR】新サービスのご紹介</p><p>{BODY_1}</p><p>{LONG_BODY}</p>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{LONG_BODY}"));
    }

    #[test]
    fn drops_runs_of_link_only_blocks() {
        let excerpt = extract_body(&format!(
            r#"<p>{BODY_1}</p><p>{BODY_2}</p>
<ul><li><a href="/a/1">幻想的なリング構造をもつ銀河 カメラが撮影</a></li><li><a href="/a/2">花咲くような星のゆりかごの輝き</a></li></ul>
<p><a href="https://example.org/ref">ESO - Ribbons and pearls</a></p>
<p>RECOMMEND</p><p><a href="/r">オススメ記事</a></p><p><a href="/k">注目のタグ</a></p>"#
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}\n\nRECOMMEND"));
    }

    #[test]
    fn keeps_single_link_paragraph_and_inline_links() {
        // 単独のリンク段落、本文中のリンク、リンクを含む箇条書き 1 項目は残す。
        let inline = r#"同社は<a href="/c">公式サイト</a>で詳細を公開しており、利用者は手続きをすれば来月から使えるとのことです。"#;
        let excerpt = extract_body(&format!(
            r#"<p>{BODY_1}</p><p><a href="https://example.com/">詳しくは公式サイトの発表資料へ</a></p><p>{inline}</p><ul><li><a href="/p">対象製品の一覧ページ</a>（全12機種）を確認してください</li></ul>"#
        ));
        assert!(excerpt.contains("詳しくは公式サイトの発表資料へ"));
        assert!(excerpt.contains("で詳細を公開しており"));
        assert!(excerpt.contains("対象製品の一覧ページ"));
    }

    #[test]
    fn drops_short_donation_and_membership_appeals() {
        let donation = "親愛なる読者の皆さまへ。サーバー運営がとても苦しい状態です。たった1円でも良いので支援してもらえれば助かります！今すぐ寄付は上のボタンから！";
        let newsletter = "最新ニュースをお届けするメルマガの登録はこちらから。";
        let membership = "会員登録はこちら。無料で記事が読み放題になります。";
        let excerpt = extract_body(&format!(
            "<p>{donation}</p><p>{BODY_1}</p><p>{newsletter}</p><p>{BODY_2}</p><p>{membership}</p>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn keeps_body_paragraphs_that_mention_appeal_words() {
        // 呼びかけの語があっても、読者へ行動を促す言い回しがない報道の文や長い段落は残す。
        let donation_news =
            "市は被災地への寄付を呼びかけており、受付窓口は市役所の1階に設けられています。";
        let newsletter_news =
            "同社はニュースレターの配信を来月で終了し、会員登録の仕組みも見直すと発表しました。";
        let long_appeal = format!(
            "{}団体の担当者は「寄付はボタンから受け付けている」と話しました。",
            LONG_BODY.repeat(5)
        );
        assert!(long_appeal.chars().count() > APPEAL_MAX_BLOCK_CHARS);
        let donation_with_link =
            "同社は売上の一部を寄付すると発表した。詳細はこちらから確認できる。";
        let excerpt = extract_body(&format!(
            "<p>{donation_news}</p><p>{newsletter_news}</p><p>{long_appeal}</p><p>{donation_with_link}</p>"
        ));
        assert!(excerpt.contains(donation_with_link));
        assert!(excerpt.contains(donation_news));
        assert!(excerpt.contains(newsletter_news));
        assert!(excerpt.contains(&long_appeal));
    }

    #[test]
    fn drops_pagination_but_keeps_numeric_body_text() {
        assert!(is_pagination_block("1 2 >"));
        assert!(is_pagination_block("< 前へ 1 2 3 次へ >"));
        assert!(!is_pagination_block("2026年10月13日"));
        assert!(!is_pagination_block("第2戦は 3-1 で勝利"));
        let excerpt = extract_body(&format!("<p>{BODY_1}</p><p>{BODY_2}</p><p>1 2 &gt;</p>"));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn prefers_low_link_density_candidate_over_longer_link_card() {
        // リンクだけでできた長いカード（他記事の告知）より、短くてもリンクの少ない記事本文を選ぶ。
        let card_title = "他の記事の長いタイトルが入ります。".repeat(8);
        let html = format!(
            r#"<!doctype html><html><body>
<div class="cards"><article><p><a href="/other">{card_title}</a></p></article></div>
<article><p>{BODY_1}</p><p>{BODY_2}</p></article>
</body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        assert_eq!(excerpt.unwrap(), format!("{BODY_1}\n\n{BODY_2}"));
    }

    fn long_paragraphs(count: usize) -> String {
        (0..count)
            .map(|i| format!("<p>本文の段落{i}です。{LONG_BODY}</p>"))
            .collect()
    }

    #[test]
    fn switches_from_card_in_list_to_paragraph_cluster() {
        // 一覧（li）の中の記事カードしか候補に当たらず、本文は class 名が自動生成の div にある構造。
        let body = long_paragraphs(5);
        let html = format!(
            r#"<!doctype html><html><body>
<div id="mainContent"><div class="Block_primary__x1"><div class="Article_body__k2 postContent">{body}</div></div>
<div class="Hot_list__z9"><ul><li><article><p><a href="/k">#話題のキーワード</a></p><p>他の記事のタイトルが入ります</p></article></li></ul></div></div>
</body></html>"#
        );
        let (excerpt, source) = extract_excerpt_with_source(&html);
        assert_eq!(source, ExcerptSource::Body);
        let excerpt = excerpt.unwrap();
        assert!(excerpt.starts_with("本文の段落0です。"));
        assert!(excerpt.contains("本文の段落4です。"));
        assert!(!excerpt.contains("他の記事のタイトル"));
    }

    #[test]
    fn keeps_short_article_in_list_item_over_consent_text() {
        // タイムラインのように li で包まれた本物の短信（リンクなし）は、body 直下の同意文に差し替えない。
        let consent = long_paragraphs(5);
        let html = format!(
            r#"<!doctype html><html><body><ul><li><article><p>{BODY_1}</p><p>{BODY_2}</p></article></li></ul><div id="cmp">{consent}</div></body></html>"#
        );
        assert_eq!(
            extract_excerpt(&html).unwrap(),
            format!("{BODY_1}\n\n{BODY_2}")
        );
        // リンクを含むカードでも、かたまりとの共通の祖先が body なら差し替えない。
        let html = format!(
            r#"<!doctype html><html><body><ul><li><article><p><a href="/k">#話題</a></p><p>{BODY_1}</p></article></li></ul><div id="cmp">{consent}</div></body></html>"#
        );
        assert!(extract_excerpt(&html).unwrap().contains(BODY_1));
    }

    #[test]
    fn lead_only_article_is_not_replaced_by_sibling_text() {
        // リードだけの article の兄弟にある長い文章も拾わない（本文か無関係な文章かを区別できないため。
        // 実記事でこの構造に見えた東洋経済は、全文が別 URL にあり対象外）。
        let lead = "リード文です。記事の要点をまとめています。";
        let body = long_paragraphs(5);
        let html = format!(
            r#"<!doctype html><html><body><div class="page">
<article><h1>タイトル</h1><p>{lead}</p></article>
<div class="content">{body}</div>
</div></body></html>"#
        );
        assert_eq!(extract_excerpt(&html).unwrap(), lead);
    }

    /// カードでない短い記事（短信）は、近くに長い文章があっても差し替えない（レビューで示された 3 つの構造）。
    #[test]
    fn keeps_short_non_card_article_next_to_long_blocks() {
        let short = format!("<article><p>{BODY_1}</p><p>{BODY_2}</p></article>");
        let long = long_paragraphs(5);
        for html in [
            format!(
                r#"<!doctype html><html><body>{short}<div id="cmp-banner">{long}</div></body></html>"#
            ),
            format!(
                r#"<!doctype html><html><body><div class="l-contents">{short}<div class="site-info">{long}</div></div></body></html>"#
            ),
            format!(
                r#"<!doctype html><html><body><div class="stream">{short}<div class="next-entry">{long}</div></div></body></html>"#
            ),
        ] {
            let (excerpt, source) = extract_excerpt_with_source(&html);
            assert_eq!(source, ExcerptSource::Body);
            assert_eq!(excerpt.unwrap(), format!("{BODY_1}\n\n{BODY_2}"), "{html}");
        }
    }

    #[test]
    fn keeps_short_article_over_unrelated_long_text() {
        // 短い記事（カードではない）は、親の外にある無関係な長い説明文に差し替えない。
        let notice = long_paragraphs(5);
        let html = format!(
            r#"<!doctype html><html><body>
<div class="wrap"><article><p>{BODY_1}</p><p>{BODY_2}</p></article></div>
<div class="site-notice">{notice}</div>
</body></html>"#
        );
        assert_eq!(
            extract_excerpt(&html).unwrap(),
            format!("{BODY_1}\n\n{BODY_2}")
        );
    }

    #[test]
    fn does_not_switch_to_cluster_with_few_paragraphs() {
        // 段落が少ないかたまり（同意画面の説明文など）には、カードからでも差し替えない。
        let modal = format!(
            "<p>{}</p><p>{}</p>",
            LONG_BODY.repeat(3),
            LONG_BODY.repeat(3)
        );
        let html = format!(
            r#"<!doctype html><html><body>
<div class="modal">{modal}</div>
<ul><li><article><p>{BODY_1}</p></article></li></ul>
</body></html>"#
        );
        assert_eq!(extract_excerpt(&html).unwrap(), BODY_1);
    }

    #[test]
    fn cluster_switch_requires_much_longer_text() {
        assert!(should_switch_to_cluster(76, 974));
        assert!(!should_switch_to_cluster(150, 400)); // 3 倍未満
        assert!(!should_switch_to_cluster(50, 299)); // 300 文字未満
        assert!(!should_switch_to_cluster(400, 2000)); // 選んだ抜粋が短くない
    }

    #[test]
    fn drops_print_label_copyright_and_author_label_at_edges() {
        // ITmedia NEWS の実記事の形（先頭の「印刷する」、末尾の著作権表示と「この記事の著者」）。
        let excerpt = extract_body(&format!(
            "<p>印刷する</p><p>{BODY_1}</p><p>{BODY_2}</p>\
<p>Copyright © ITmedia, Inc. All Rights Reserved. 本記事は制作段階で生成AIサービスを利用しています。</p>\
<h2>この記事の著者</h2>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn drops_trailing_run_of_site_nav_labels() {
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><p>{BODY_2}</p><p>(C) 2026 Example Media Inc.</p>\
<p>ITmedia NEWS SNS</p><p>お問い合わせ</p><p>利用規約</p><p>公式SNS</p>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn drops_author_label_followed_by_short_profile() {
        // 著者欄の見出しの後ろに長い段落がなければ、見出しから後ろ（短い紹介文）を打ち切る。
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><p>{BODY_2}</p><p>この記事を書いた人</p><p>山田 太郎 編集部記者</p>"
        ));
        assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}"));
    }

    #[test]
    fn keeps_body_text_that_mentions_copyright_or_author() {
        // 著作権・著者・印刷・問い合わせに触れる本文の文は、末尾にあっても残す。
        let tail_blocks = [
            "著作権（Copyright）の侵害で出版社が提訴した。",
            "Copyright Officeは生成AIの扱いについて報告書を公表した。",
            "Copyrighted works may not be used without permission.",
            "この記事の著者である山田氏は、取材の経緯を次のように説明している。調査には半年かかったという。",
            "3Dプリンターで印刷する技術が、住宅の建設にも使われ始めている。",
            "消費者センターへのお問い合わせが急増している。",
            "若者の間で広がるSNS",
            "(C) は回答しなかった",
            "Copyright law remains unsettled for AI-generated works, experts say.",
            "The studio said all rights reserved",
        ];
        for tail in tail_blocks {
            let excerpt = extract_body(&format!("<p>{BODY_1}</p><p>{BODY_2}</p><p>{tail}</p>"));
            assert_eq!(excerpt, format!("{BODY_1}\n\n{BODY_2}\n\n{tail}"), "{tail}");
        }
    }

    #[test]
    fn keeps_labels_in_the_middle_of_the_body() {
        // 末尾の並びにない著作権表示（写真のクレジット）・サイト案内の文言は除かない。
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><p>© 2026 Example Photo</p><p>お問い合わせ</p><p>{BODY_2}</p>"
        ));
        assert_eq!(
            excerpt,
            format!("{BODY_1}\n\n© 2026 Example Photo\n\nお問い合わせ\n\n{BODY_2}")
        );
    }

    #[test]
    fn drops_site_copyright_notice_followed_by_series_blurb() {
        // ITmedia の連載記事の形（著作権表示の後ろに連載の紹介文が続き、末尾にない）。
        let blurb = "2019年にスタートした本連載は、世界中の幅広い分野から最先端の研究論文を独自の視点で紹介しています。";
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><p>{BODY_2}</p><p>Copyright © ITmedia, Inc. All Rights Reserved.</p><p>{blurb}</p>"
        ));
        assert_eq!(
            excerpt,
            format!(
                "{BODY_1}

{BODY_2}

{blurb}"
            )
        );
    }

    #[test]
    fn keeps_copyright_like_lines_in_the_middle_of_the_body() {
        // 本文の途中では、サイト全体の著作権表示の強い形だけを除く。英文の引用・写真のクレジットは残す。
        for middle in [
            "Copyright law reform was approved in 2026",
            "© Reuters",
            "(C) 2026 Example Photo",
            "The studio said all rights reserved",
            "Example Pictures. All Rights Reserved.",
        ] {
            let excerpt = extract_body(&format!("<p>{BODY_1}</p><p>{middle}</p><p>{BODY_2}</p>"));
            assert_eq!(
                excerpt,
                format!(
                    "{BODY_1}

{middle}

{BODY_2}"
                ),
                "{middle}"
            );
        }
    }

    #[test]
    fn keeps_quoted_license_text_in_the_middle_of_the_body() {
        // 本文の途中で引用したライセンス文（All Rights Reserved で終わる）は残す。
        let license = "Licensed under the Example License. All Rights Reserved.";
        let excerpt = extract_body(&format!(
            "<p>{BODY_1}</p><blockquote>{license}</blockquote><p>{BODY_2}</p>"
        ));
        assert_eq!(
            excerpt,
            format!(
                "{BODY_1}

{license}

{BODY_2}"
            )
        );
    }

    #[test]
    fn keeps_long_paragraph_that_ends_with_rights_notice_wording() {
        let long = format!(
            "{}All rights reserved",
            "同社は声明で、すべての権利を保有していると改めて強調し、今後も法的な対応を続ける方針を示した。".repeat(3)
        );
        assert!(long.chars().count() > COPYRIGHT_LINE_MAX_CHARS);
        let excerpt = extract_body(&format!("<p>{BODY_1}</p><p>{long}</p>"));
        assert!(excerpt.ends_with("All rights reserved"));
    }

    #[test]
    fn edge_labels_only_fall_back_to_lenient() {
        // ラベルしかない候補でも抜粋なしにはせず、判定なしの抜粋（修正前と同じ規則）を使う。
        let excerpt = extract_body("<p>印刷する</p><p>Copyright © Example</p>");
        assert_eq!(excerpt, "印刷する\n\nCopyright © Example");
    }

    #[test]
    fn copyright_line_patterns() {
        for text in [
            "Copyright © ITmedia, Inc. All Rights Reserved.",
            "Copyright 2026 Example Inc.",
            "Copyright (C) 2026 Example",
            "© 2026 Example",
            "(C) 2026 Example Media",
            "（C）Example Inc.",
            "Copyright 株式会社サンプル",
            "Example News. All rights reserved.",
            "Copyright © 2026 Nikkei Inc. All rights reserved.",
        ] {
            assert!(is_copyright_line(text), "{text}");
        }
        for text in [
            "Copyright Officeは報告書を公表した。",
            "Copyrighted",
            "著作権（Copyright）の侵害",
            "(C) は回答しなかった",
            "（C）共同通信の配信",
            "Copyright law remains unsettled for AI-generated works, experts say.",
            "The studio said all rights reserved",
            "（C）Example",
        ] {
            assert!(!is_copyright_line(text), "{text}");
        }
    }

    #[test]
    fn sns_nav_label_patterns() {
        for text in ["ITmedia NEWS SNS", "公式SNS", "Example 公式 SNS", "SNS"] {
            assert!(is_sns_nav_label(text), "{text}");
        }
        for text in ["若者の間で広がるSNS", "ITmediaSNS", "人気のSNS"] {
            assert!(!is_sns_nav_label(text), "{text}");
        }
    }
}
