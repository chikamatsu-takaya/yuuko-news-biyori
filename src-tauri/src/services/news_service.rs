//! ニュース取得オーケストレーション（詳細設計書 §5.3.1 NewsService）。
//!
//! 取得元は `config/news_sources.json` に保存された許可URLのみ。任意URLは扱わない。
//! フロー: 取得元読込 → feed_url 検証 → RSS取得 → 各記事URL検証 → 本文抽出 →
//!         おすすめ採点 → 重複排除 → 保存。
//!
//! セキュリティ方針:
//! - news_sources.json は deny-by-default（空配列）。破損時は fail-close（Err）。
//! - feed_url / 記事URL は RssClient / HtmlFetcher に渡す前に UrlGuard で再検証する。
//! - HtmlFetcher 失敗時は本文なしで保存を継続するが、エラー件数・対象URLを結果へ含める。
//! - 結果の errors はサニタイズ済み（対象URLと固定カテゴリのみ）。内部パスや
//!   低レベルなエラー詳細は UI へ返さず、ログにのみ残す。
//! - fetched_at は UTC で保存する。ローカル深夜0時などのトリガーは ⑥b で扱う。

use std::path::Path;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::article::{ArticleReadState, FetchedArticle};
use crate::error::AppError;
use crate::infra::allowlist::NetworkAllowlist;
use crate::infra::html_fetcher::HtmlFetcher;
use crate::infra::rss_client::RssClient;
use crate::infra::url_guard::{validate_url, UrlPurpose};
use crate::paths::AppPaths;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::recommendation_service::{
    RecommendationContext, RecommendationInput, RecommendationService,
};

// 結果の errors に載せる固定カテゴリ（UI返却用・低レベル詳細を含めない）。
const ERROR_FEED_URL_REJECTED: &str = "feed_url_rejected";
const ERROR_FEED_FETCH_FAILED: &str = "feed_fetch_failed";
const ERROR_ARTICLE_URL_REJECTED: &str = "article_url_rejected";
const ERROR_ARTICLE_FETCH_FAILED: &str = "article_fetch_failed";

/// 取得元1件（許可URLとジャンル）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsSource {
    pub url: String,
    #[serde(default)]
    pub genre: String,
}

/// 取得元設定。`config/news_sources.json` として保存・編集する。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsSourcesConfig {
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<NewsSource>,
}

impl Default for NewsSourcesConfig {
    /// deny-by-default。取得元は空で初期化し、明示追加するまで取得しない。
    fn default() -> Self {
        Self {
            version: 1,
            sources: Vec::new(),
        }
    }
}

impl NewsSourcesConfig {
    /// 取得元設定を読み込む。
    /// - ファイル無し: 初回扱い。空（deny-by-default）を返す。
    /// - 破損: **fail-close**。デフォルトへ戻さず Err を返し、取得を中止する。
    pub fn load(path: &Path) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str::<Self>(crate::util::strip_utf8_bom(&raw)).map_err(|error| {
            AppError::Validation(format!(
                "news sources config is corrupted; refusing to fetch (fail-close): {error}"
            ))
        })
    }

    /// 設定ファイルが無い場合のみ空のデフォルトを書き出す。
    pub fn initialize_default_if_missing(path: &Path) -> Result<(), AppError> {
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_vec_pretty(&Self::default())?;
        std::fs::write(path, payload)?;
        Ok(())
    }
}

/// 取得時のエラー（UI返却用にサニタイズ済み）。
/// `url` は対象URL（公開情報）、`kind` は固定カテゴリ。内部パスや低レベル詳細は含めない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshError {
    pub url: String,
    pub kind: String,
}

impl RefreshError {
    fn new(url: &str, kind: &str) -> Self {
        Self {
            url: url.to_string(),
            kind: kind.to_string(),
        }
    }
}

/// 取得結果サマリ（UI返却用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshNewsResult {
    /// 処理対象とした取得元の件数。
    pub sources_processed: usize,
    /// RSSから得たアイテム総数（重複・既存含む）。
    pub fetched: usize,
    /// 新規に保存した記事数。
    pub saved: usize,
    /// サニタイズ済みエラー一覧。
    pub errors: Vec<RefreshError>,
}

/// ニュース取得オーケストレーションサービス。
#[derive(Debug, Clone)]
pub struct NewsService {
    rss_client: RssClient,
    html_fetcher: HtmlFetcher,
    recommendation_service: RecommendationService,
    article_repository: ArticleRepository,
    settings_repository: SettingsRepository,
    sources_path: std::path::PathBuf,
    allowlist_path: std::path::PathBuf,
}

impl NewsService {
    pub fn new(
        paths: &AppPaths,
        article_repository: ArticleRepository,
        settings_repository: SettingsRepository,
        recommendation_service: RecommendationService,
    ) -> Self {
        Self {
            rss_client: RssClient::new(paths),
            html_fetcher: HtmlFetcher::new(paths),
            recommendation_service,
            article_repository,
            settings_repository,
            sources_path: paths.news_sources_path.clone(),
            allowlist_path: paths.network_allowlist_path.clone(),
        }
    }

    /// 保存済みの取得元からニュースを取得・保存する。
    ///
    /// 取得元設定・許可リストの破損時は fail-close（Err）。個々のフィード/記事の
    /// 失敗は結果の `errors` に集約し、処理全体は継続する。
    pub async fn refresh(&self) -> Result<RefreshNewsResult, AppError> {
        // fail-close 対象（破損で取得中止）。
        let config = NewsSourcesConfig::load(&self.sources_path)?;
        let allowlist = NetworkAllowlist::load(&self.allowlist_path)?;

        let context = RecommendationContext {
            preferred_genres: self.load_preferred_genres(),
            important_keywords: Vec::new(),
        };

        // 既存記事IDを起点に重複排除（既知記事は本文取得もスキップして通信を抑える）。
        let mut seen_ids = self.article_repository.existing_article_ids()?;
        let mut errors: Vec<RefreshError> = Vec::new();
        let mut fetched = 0usize;
        let mut to_save: Vec<FetchedArticle> = Vec::new();

        for source in &config.sources {
            // (制約5) feed_url を RssClient へ渡す前に検証する。
            let validated_feed = match validate_url(&source.url, UrlPurpose::Rss, &allowlist) {
                Ok(url) => url,
                Err(error) => {
                    log::warn!("rejected feed url '{}': {error}", source.url);
                    errors.push(RefreshError::new(&source.url, ERROR_FEED_URL_REJECTED));
                    continue;
                }
            };

            let items = match self.rss_client.fetch(validated_feed.as_str()).await {
                Ok(items) => items,
                Err(error) => {
                    log::warn!("failed to fetch feed '{}': {error}", source.url);
                    errors.push(RefreshError::new(&source.url, ERROR_FEED_FETCH_FAILED));
                    continue;
                }
            };

            for item in items {
                fetched += 1;
                let article_id = article_id_from_url(&item.article_url);
                if seen_ids.contains(&article_id) {
                    continue;
                }

                // (制約6) 記事URLを HtmlFetcher へ渡す前に Article 用検証を通す。
                if let Err(error) = validate_url(&item.article_url, UrlPurpose::Article, &allowlist)
                {
                    log::warn!("rejected article url '{}': {error}", item.article_url);
                    errors.push(RefreshError::new(
                        &item.article_url,
                        ERROR_ARTICLE_URL_REJECTED,
                    ));
                    continue;
                }

                // (制約7) 本文取得失敗時も保存を継続。失敗は errors に記録する。
                let excerpt = match self.html_fetcher.fetch_and_extract(&item.article_url).await {
                    Ok(result) => result.excerpt,
                    Err(error) => {
                        log::warn!("failed to fetch article '{}': {error}", item.article_url);
                        errors.push(RefreshError::new(
                            &item.article_url,
                            ERROR_ARTICLE_FETCH_FAILED,
                        ));
                        None
                    }
                };

                let excerpt = excerpt.or_else(|| item.summary.clone());
                let fetched_at = utc_now_rfc3339();
                let published_at_text =
                    normalize_published_at(item.published_at.as_deref(), &fetched_at);
                let score = self.recommendation_service.calculate_score(
                    &RecommendationInput {
                        title: &item.title,
                        genre: &source.genre,
                        tags: &[],
                        read_state: &ArticleReadState::Unread,
                        age_hours: Some(0.0),
                    },
                    &context,
                );

                seen_ids.insert(article_id.clone());
                to_save.push(FetchedArticle {
                    article_id,
                    title: item.title,
                    source_name: item.source_name,
                    original_url: item.article_url,
                    fetched_at,
                    published_at_text,
                    genre: source.genre.clone(),
                    tags: Vec::new(),
                    excerpt,
                    recommendation_score: score,
                    read_state: ArticleReadState::Unread,
                });
            }
        }

        let saved = self.article_repository.save_fetched_articles(to_save)?;

        Ok(RefreshNewsResult {
            sources_processed: config.sources.len(),
            fetched,
            saved,
            errors,
        })
    }

    /// 推薦コンテキスト用の選好ジャンルを設定から取得する。
    /// 設定の読み込み失敗は取得の致命傷ではないため、空ジャンルへフォールバックする
    /// （fail-close は news_sources / allowlist 側で担保する）。
    fn load_preferred_genres(&self) -> Vec<String> {
        match self.settings_repository.load_or_default() {
            Ok(settings) => settings.news.categories,
            Err(error) => {
                log::warn!("failed to load settings for recommendation context: {error}");
                Vec::new()
            }
        }
    }
}

/// 正規化したURLから安定な article_id を生成する。
/// 正規化: URLパース → フラグメント除去 → 末尾スラッシュ除去（ホストは url が小文字化）。
fn article_id_from_url(raw: &str) -> String {
    let normalized = normalize_url_for_id(raw);
    format!("news_{:016x}", fnv1a_64(normalized.as_bytes()))
}

fn normalize_url_for_id(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(mut url) => {
            url.set_fragment(None);
            url.as_str().trim_end_matches('/').to_string()
        }
        Err(_) => raw.trim().to_string(),
    }
}

/// FNV-1a 64bit。Rustバージョンに依存せず安定なハッシュを得るために自前実装する
/// （`DefaultHasher` はリリース間でアルゴリズムが変わり得るため article_id には使わない）。
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// 現在時刻を UTC の RFC3339（秒精度）で返す。
fn utc_now_rfc3339() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// フィードの公開日時を UTC ISO に正規化する。RSS の pubDate(RFC2822) と
/// Atom の published(RFC3339/ISO8601) の両方に対応する。
/// パースできない・存在しない場合は fetched_at（UTC）へフォールバックする。
fn normalize_published_at(raw: Option<&str>, fallback_utc: &str) -> String {
    let Some(value) = raw else {
        return fallback_utc.to_string();
    };
    let trimmed = value.trim();

    if let Ok(datetime) = chrono::DateTime::parse_from_rfc2822(trimmed) {
        return to_utc_seconds(datetime);
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return to_utc_seconds(datetime);
    }
    fallback_utc.to_string()
}

fn to_utc_seconds(datetime: chrono::DateTime<chrono::FixedOffset>) -> String {
    datetime
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_path() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "yuuko_news_sources_test_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn article_id_ignores_fragment_and_trailing_slash() {
        let base = article_id_from_url("https://news.example.com/articles/1");
        assert_eq!(
            base,
            article_id_from_url("https://news.example.com/articles/1#section")
        );
        assert_eq!(
            base,
            article_id_from_url("https://news.example.com/articles/1/")
        );
        assert!(base.starts_with("news_"));
    }

    #[test]
    fn article_id_differs_for_different_urls() {
        assert_ne!(
            article_id_from_url("https://news.example.com/a"),
            article_id_from_url("https://news.example.com/b")
        );
    }

    #[test]
    fn news_sources_missing_file_is_deny_by_default() {
        let path = unique_temp_path();
        let _ = std::fs::remove_file(&path);
        let config = NewsSourcesConfig::load(&path).expect("missing file yields empty default");
        assert!(config.sources.is_empty());
    }

    #[test]
    fn news_sources_corrupted_file_fails_close() {
        let path = unique_temp_path();
        std::fs::write(&path, b"{ not valid json").unwrap();
        let result = NewsSourcesConfig::load(&path);
        let _ = std::fs::remove_file(&path);
        assert!(
            result.is_err(),
            "corrupted news sources config must fail-close"
        );
    }

    #[test]
    fn news_sources_initialize_writes_empty_then_loads_back() {
        let path = unique_temp_path();
        let _ = std::fs::remove_file(&path);
        NewsSourcesConfig::initialize_default_if_missing(&path).unwrap();
        let loaded = NewsSourcesConfig::load(&path).unwrap();
        assert_eq!(loaded, NewsSourcesConfig::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn normalize_published_at_converts_rfc2822_to_utc() {
        let normalized =
            normalize_published_at(Some("Thu, 04 Jun 2026 10:00:00 +0900"), "fallback");
        assert_eq!(normalized, "2026-06-04T01:00:00Z");
    }

    #[test]
    fn normalize_published_at_falls_back_when_unparseable_or_missing() {
        assert_eq!(
            normalize_published_at(Some("not a date"), "2026-06-04T00:00:00Z"),
            "2026-06-04T00:00:00Z"
        );
        assert_eq!(
            normalize_published_at(None, "2026-06-04T00:00:00Z"),
            "2026-06-04T00:00:00Z"
        );
    }

    #[test]
    fn normalize_published_at_converts_iso8601_to_utc() {
        // Atom の published（RFC3339/ISO8601）も UTC へ正規化する。
        assert_eq!(
            normalize_published_at(Some("2026-06-03T15:11:06Z"), "fallback"),
            "2026-06-03T15:11:06Z"
        );
        assert_eq!(
            normalize_published_at(Some("2026-06-04T10:00:00+09:00"), "fallback"),
            "2026-06-04T01:00:00Z"
        );
    }

    #[test]
    fn news_sources_load_tolerates_utf8_bom() {
        let path = unique_temp_path();
        let config = NewsSourcesConfig {
            version: 1,
            sources: vec![NewsSource {
                url: "https://www.publickey1.jp/atom.xml".to_string(),
                genre: "テクノロジー".to_string(),
            }],
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();
        let loaded = NewsSourcesConfig::load(&path).expect("BOM-prefixed news sources should load");
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, config);
    }
}
