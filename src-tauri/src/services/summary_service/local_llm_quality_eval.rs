//! 同梱ローカルLLMの品質評価ハーネス（手動実行専用・CI では動かさない）。
//!
//! 実ニュースを**アプリと同じ取得の部品**（許可リスト検証 → RSS取得 → HTML本文抽出 → 記事保存）で集め、
//! アプリと同じ要約生成（要約 → 要点・タグ → 再説明 → 一言）と用語解説を provider = local で走らせ、
//! 出力・検証結果・所要時間を JSON で書き出す。品質の判定（採点）は人が出力を読んで行う。
//!
//! 安全のための決まり:
//! - `YUUKO_LLM_EVAL_OUT`（出力先フォルダ）が無ければ何もしない（`--ignored` で動かしても空振りする）。
//! - 実際のアプリデータ（app-data）には書かない。取得元設定・許可リストは
//!   `YUUKO_LLM_EVAL_CONFIG_DIR` のフォルダから**読むだけ**で、出力先の一時データフォルダへ複写して使う。
//! - 評価だけで使う取得元は `YUUKO_LLM_EVAL_EXTRA_SOURCES` の JSON で、**複写した側**へだけ足す。
//! - 出力（記事の本文抜粋を含む）は出力先フォルダにだけ置く。リポジトリへ入れない。
//!
//! 環境変数:
//! - `YUUKO_LLM_EVAL_LABEL`: 回の名前（英数字・`-`・`_`）。指定すると `<OUT>/<LABEL>/` に書く
//!   （改善前後の回を並べて比べるため）。以下 `<RUN>` は書き込み先のフォルダ。
//! - `YUUKO_LLM_EVAL_PHASE`: `fetch` / `summary` / `terms`。省略時はすべて。
//! - `YUUKO_LLM_EVAL_ARTICLES`: 保存済みの `articles.json`。指定すると取得（通信）をせず、この記事を
//!   一時データフォルダへ取り込む（別の回で同じ記事を評価するため）。
//! - `YUUKO_LLM_EVAL_PER_SOURCE`: 取得元1件あたりの記事数（既定 4。追加取得元の `maxArticles` が優先）。
//! - `YUUKO_LLM_EVAL_MAX_ARTICLES`: 全体の上限（既定 30）。
//! - `YUUKO_LLM_EVAL_TERMS`: 用語の指定ファイル（既定 `<RUN>/terms.json`）。
//! - `YUUKO_LLM_EVAL_FORCE_TERM_PROMPT`: 指定すると、フレーズも用語解説のプロンプトで作らせる
//!   （フレーズ対応 D21 の前の挙動との比較用。後処理前の出力は記録の `raw` にある）。
//!
//! 段階:
//! - `fetch`: 取得元ごとに RSS を取り、先頭から上限件数まで本文を取得・抽出して保存し、`<RUN>/articles.json`
//!   （本文の出どころ `excerptSource` = `html` / `rss_summary` / `none` と、`html` のときの抽出元
//!   `htmlExtraction` = `body` / `meta_description` 付き）と `<RUN>/fetch_report.json` を書く。
//!   1件ごとの扱い（記事URLの検証 → 本文取得 → 失敗・抽出なしなら RSS 概要）は `NewsService` の取得本体と同じ。
//!   取得元ごとに件数を絞るため、`NewsService::refresh`（1フィード最大30件の本文を取る）は使わない。
//! - `summary`: 各記事に `generate_article_summary` と同じ処理（手動生成の経路）を provider = local で走らせ、
//!   段ごとの生出力・検証結果・所要時間を `<RUN>/summaries.jsonl` へ1記事1行で追記する（記録済みの記事は飛ばす）。
//! - `terms`: 用語の指定（`[{"articleId": "...", "terms": ["..."]}]`。人が記事を読んで選ぶ）の語を
//!   `DictionaryService::explain_selected_term` で解説させ、`<RUN>/terms.jsonl` へ追記する。
//!   語ごとに空の辞書を使い、保存済み辞書の命中で AI 呼び出しが省かれないようにする。
//!   長めの文章（フレーズ・D21）を指定すると、アプリと同じ判定でフレーズの説明になる（記録の `type` で分かる）。
//!
//! 追加取得元の JSON（`YUUKO_LLM_EVAL_EXTRA_SOURCES`）:
//! ```text
//! {"sources": [{"url": "https://...", "genre": "社会", "maxArticles": 3}],
//!  "allowedRssDomains": ["..."], "allowedArticleDomains": ["..."]}
//! ```
//!
//! 実行例（PowerShell）:
//! ```text
//! $env:YUUKO_LLM_EVAL_OUT = "<一時フォルダ>\llm-eval"
//! $env:YUUKO_LLM_EVAL_CONFIG_DIR = "$env:APPDATA\jp.star-system.yuuko-news\config"
//! cargo test --manifest-path src-tauri/Cargo.toml local_llm_quality_eval -- --ignored --nocapture
//! ```
//! 同梱物（モデル・llama-server）は `scripts/local-llm/place-bundle.mjs` で `resources/local_llm` に置くか、
//! `YUUKO_LOCAL_LLM_DIR` で場所を指す。

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::output_check::{check_japanese_summary, IssueKind};
use super::{
    clean_ai_speech, select_valid_output_in_tone, validate_article_points, validate_summary_output,
    FallbackPolicy, SummaryGeneration, SummaryOutputKind, SummaryService,
};
use crate::domain::article::{ArticleReadState, FetchedArticle};
use crate::domain::dictionary::ExplainSelectedTermParams;
use crate::domain::settings::{AiProvider, ExplanationLevel, PersistedSettings};
use crate::domain::speech_tone::SpeechTone;
use crate::domain::summary::{
    AiRequest, AiResponse, GenerateArticleSummaryParams, TERM_EXPLANATION_PROMPT_ID,
};
use crate::error::AppError;
use crate::infra::allowlist::NetworkAllowlist;
use crate::infra::html_fetcher::{ExcerptSource, HtmlFetcher};
use crate::infra::rss_client::RssClient;
use crate::infra::url_guard::{validate_url, UrlPurpose};
use crate::paths::{AppPaths, NETWORK_ALLOWLIST_RELATIVE_PATH, NEWS_SOURCES_RELATIVE_PATH};
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::dictionary_repository::DictionaryRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::ai_provider_service::summary_fixed_prompt_texts;
use crate::services::ai_provider_service::AiProviderService;
use crate::services::dictionary_service::{DictionaryService, TermExplanationAi};
use crate::services::local_llm_service::LocalLlmService;
use crate::services::news_service::{
    article_id_from_url, normalize_published_at, utc_now_rfc3339, NewsSource, NewsSourcesConfig,
};

const OUT_ENV: &str = "YUUKO_LLM_EVAL_OUT";
const CONFIG_DIR_ENV: &str = "YUUKO_LLM_EVAL_CONFIG_DIR";
const EXTRA_SOURCES_ENV: &str = "YUUKO_LLM_EVAL_EXTRA_SOURCES";
const LABEL_ENV: &str = "YUUKO_LLM_EVAL_LABEL";
const PHASE_ENV: &str = "YUUKO_LLM_EVAL_PHASE";
const ARTICLES_ENV: &str = "YUUKO_LLM_EVAL_ARTICLES";
const PER_SOURCE_ENV: &str = "YUUKO_LLM_EVAL_PER_SOURCE";
const MAX_ARTICLES_ENV: &str = "YUUKO_LLM_EVAL_MAX_ARTICLES";
const TERMS_ENV: &str = "YUUKO_LLM_EVAL_TERMS";
const BUNDLE_DIR_ENV: &str = "YUUKO_LOCAL_LLM_DIR";
const FORCE_TERM_PROMPT_ENV: &str = "YUUKO_LLM_EVAL_FORCE_TERM_PROMPT";
const DEFAULT_PER_SOURCE: usize = 4;
const DEFAULT_MAX_ARTICLES: usize = 30;

#[test]
#[ignore = "manual: requires network, the real local LLM bundle and YUUKO_LLM_EVAL_OUT"]
fn local_llm_quality_eval() {
    let Some(out_dir) = std::env::var_os(OUT_ENV).map(PathBuf::from) else {
        eprintln!("{OUT_ENV} is not set; the quality evaluation does nothing");
        return;
    };
    let run_dir = match std::env::var(LABEL_ENV)
        .ok()
        .filter(|label| !label.is_empty())
    {
        Some(label) => {
            assert!(
                label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{LABEL_ENV} must contain only ASCII letters, digits, '-' and '_'"
            );
            out_dir.join(label)
        }
        None => out_dir,
    };
    std::fs::create_dir_all(&run_dir).expect("create output dir");
    let paths = AppPaths::new(run_dir.join("appdata"));
    paths.ensure_storage_dirs().expect("create temp data dirs");
    // 要約・用語解説は保存済み設定の provider と解説レベルを読む。初回起動時の初期値（provider = local・
    // 解説レベル normal）を一時データフォルダに置く。
    SettingsRepository::new(&paths)
        .save(&PersistedSettings::initial())
        .expect("save temp settings");

    let phase = std::env::var(PHASE_ENV).unwrap_or_default();
    let run_all = phase.is_empty();
    if let Some(articles_file) = std::env::var_os(ARTICLES_ENV).map(PathBuf::from) {
        // 保存済みの記事を使う回は通信しない（fetch 段を指定していても取得し直さない）。
        import_articles(&articles_file, &run_dir, &paths);
    } else if run_all || phase == "fetch" {
        run_fetch(&run_dir, &paths);
    }
    if !(run_all || phase == "summary" || phase == "terms") {
        return;
    }
    let local = LocalLlmService::new(Some(bundle_dir()));
    let ai = AiProviderService::new(&paths).with_local_llm(local.clone());
    // 1回目の呼び出しに起動時間が乗らないよう、先に接続確認（検証・起動・短い生成）を済ませて別に記録する。
    let started = Instant::now();
    let check = ai.test_connection(AiProvider::Local);
    eprintln!(
        "warm-up (connection test): {:.1}s {:?}",
        started.elapsed().as_secs_f32(),
        check.status
    );
    if run_all || phase == "summary" {
        run_summaries(&run_dir, &paths, &ai);
    }
    if run_all || phase == "terms" {
        run_terms(&run_dir, &paths, &ai);
    }
    local.shutdown();
}

pub(super) fn bundle_dir() -> PathBuf {
    std::env::var_os(BUNDLE_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("local_llm")
        })
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// 同じファイルへの複写（読み込み元を一時データフォルダに向けた設定ミス）を止める。
/// `std::fs::copy` は同じファイルへの複写で中身を空にしうるため、複写の前に必ず確かめる。
fn assert_not_same_file(source: &Path, destination: &Path) {
    let (Ok(source), Ok(destination)) = (source.canonicalize(), destination.canonicalize()) else {
        return;
    };
    assert_ne!(
        source, destination,
        "refusing to copy a file onto itself: check {CONFIG_DIR_ENV} / {ARTICLES_ENV}"
    );
}

/// 評価だけで使う取得元（`YUUKO_LLM_EVAL_EXTRA_SOURCES`）。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtraSources {
    #[serde(default)]
    sources: Vec<ExtraSource>,
    #[serde(default)]
    allowed_rss_domains: Vec<String>,
    #[serde(default)]
    allowed_article_domains: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtraSource {
    url: String,
    #[serde(default)]
    genre: String,
    max_articles: Option<usize>,
}

/// 取得元設定・許可リストを一時データフォルダへ複写し（追加取得元はその複写へだけ足す）、
/// 取得元ごとに上限件数までの記事を、アプリの取得部品で集めて保存する。
fn run_fetch(run_dir: &Path, paths: &AppPaths) {
    let config_dir = std::env::var_os(CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("{CONFIG_DIR_ENV} must point to the folder with news_sources.json")
        });
    for relative in [NEWS_SOURCES_RELATIVE_PATH, NETWORK_ALLOWLIST_RELATIVE_PATH] {
        let name = Path::new(relative).file_name().expect("file name");
        let source = config_dir.join(name);
        let destination = paths.app_data_dir.join(relative);
        assert_not_same_file(&source, &destination);
        // 読むだけ。書き込み先は一時データフォルダの config/ だけ。
        std::fs::copy(&source, &destination)
            .unwrap_or_else(|error| panic!("copy {relative}: {error}"));
    }

    let mut config = NewsSourcesConfig::load(&paths.news_sources_path).expect("load sources");
    let mut allowlist =
        NetworkAllowlist::load(&paths.network_allowlist_path).expect("load allowlist");
    let mut caps = Vec::new();
    if let Some(extra_file) = std::env::var_os(EXTRA_SOURCES_ENV).map(PathBuf::from) {
        let text = std::fs::read_to_string(&extra_file).expect("read extra sources");
        let extra: ExtraSources = serde_json::from_str(&text).expect("parse extra sources");
        for source in extra.sources {
            if let Some(cap) = source.max_articles {
                caps.push((source.url.clone(), cap));
            }
            config.sources.push(NewsSource {
                url: source.url,
                genre: source.genre,
            });
        }
        for domain in extra.allowed_rss_domains {
            if !allowlist.allowed_rss_domains.contains(&domain) {
                allowlist.allowed_rss_domains.push(domain);
            }
        }
        for domain in extra.allowed_article_domains {
            if !allowlist.allowed_article_domains.contains(&domain) {
                allowlist.allowed_article_domains.push(domain);
            }
        }
        // 複写した側（一時データフォルダ）だけを書き換える。RssClient / HtmlFetcher はこのファイルを読む。
        write_json(
            &paths.news_sources_path,
            &serde_json::to_value(&config).expect("serialize sources"),
        );
        write_json(
            &paths.network_allowlist_path,
            &serde_json::to_value(&allowlist).expect("serialize allowlist"),
        );
    }

    let per_source = env_usize(PER_SOURCE_ENV, DEFAULT_PER_SOURCE);
    let rss_client = RssClient::new(paths);
    let html_fetcher = HtmlFetcher::new(paths);
    let started = Instant::now();
    let mut seen = HashSet::new();
    let mut fetched = Vec::new();
    let mut report = Vec::new();
    for (source_index, source) in config.sources.iter().enumerate() {
        let cap = caps
            .iter()
            .find(|(url, _)| *url == source.url)
            .map_or(per_source, |(_, cap)| *cap);
        let mut stats = json!({
            "sourceIndex": source_index,
            "genre": source.genre,
            "cap": cap,
            "taken": 0,
            "html": 0,
            "htmlBody": 0,
            "htmlMeta": 0,
            "rssSummary": 0,
            "none": 0,
            "articleUrlRejected": 0,
            "articleFetchFailed": 0,
        });
        let items = validate_url(&source.url, UrlPurpose::Rss, &allowlist)
            .and_then(|url| tauri::async_runtime::block_on(rss_client.fetch(url.as_str())));
        let items = match items {
            Ok(items) => items,
            Err(error) => {
                stats["feedError"] = json!(error_label(&error));
                report.push(stats);
                continue;
            }
        };
        stats["feedItems"] = json!(items.len());
        let mut taken = 0;
        for item in items {
            if taken >= cap {
                break;
            }
            let article_id = article_id_from_url(&item.article_url);
            if !seen.insert(article_id.clone()) {
                continue;
            }
            if validate_url(&item.article_url, UrlPurpose::Article, &allowlist).is_err() {
                bump(&mut stats, "articleUrlRejected");
                continue;
            }
            // NewsService の取得本体と同じく、本文の取得・抽出に失敗したら RSS の概要を使う。
            let fetch_result =
                tauri::async_runtime::block_on(html_fetcher.fetch_and_extract(&item.article_url));
            let (html_excerpt, html_extraction) = match fetch_result {
                Ok(result) => (result.excerpt, Some(result.excerpt_source)),
                Err(error) => {
                    bump(&mut stats, "articleFetchFailed");
                    // 失敗の種類だけを残す（同じ種類は1回）。
                    let label = error_label(&error);
                    let errors = stats["fetchErrors"].as_array().cloned().unwrap_or_default();
                    if !errors.iter().any(|known| known == &json!(label)) {
                        stats["fetchErrors"] =
                            json!(errors.into_iter().chain([json!(label)]).collect::<Vec<_>>());
                    }
                    (None, None)
                }
            };
            // 本文から取れたのか、meta description で代用したのかを分けて数える（抽出の成否を見るため）。
            let html_extraction = html_extraction.map(|source| match source {
                ExcerptSource::Body => "body",
                ExcerptSource::MetaDescription => "meta_description",
                ExcerptSource::None => "none",
            });
            match html_extraction {
                Some("body") => bump(&mut stats, "htmlBody"),
                Some("meta_description") => bump(&mut stats, "htmlMeta"),
                _ => {}
            }
            let (excerpt_source, stats_key) = match (&html_excerpt, &item.summary) {
                (Some(_), _) => ("html", "html"),
                (None, Some(_)) => ("rss_summary", "rssSummary"),
                (None, None) => ("none", "none"),
            };
            bump(&mut stats, stats_key);
            let fetched_at = utc_now_rfc3339();
            let published_at_text =
                normalize_published_at(item.published_at.as_deref(), &fetched_at);
            fetched.push((
                (excerpt_source, html_extraction),
                FetchedArticle {
                    article_id,
                    title: item.title,
                    source_name: item.source_name,
                    original_url: item.article_url,
                    fetched_at,
                    published_at_text,
                    genre: source.genre.clone(),
                    tags: Vec::new(),
                    excerpt: html_excerpt.or(item.summary),
                    recommendation_score: 0.0,
                    read_state: ArticleReadState::Unread,
                },
            ));
            taken += 1;
        }
        stats["taken"] = json!(taken);
        report.push(stats);
    }
    eprintln!(
        "fetch: {:.1}s sources={} articles={}",
        started.elapsed().as_secs_f32(),
        config.sources.len(),
        fetched.len()
    );

    // 取得元の順（ジャンルの偏りを抑える）のまま全体の上限で切り、公開日時の新しい順に並べる。
    fetched.truncate(env_usize(MAX_ARTICLES_ENV, DEFAULT_MAX_ARTICLES));
    fetched.sort_by_key(|(_, article)| {
        std::cmp::Reverse(published_timestamp(&article.published_at_text))
    });
    let records = fetched
        .iter()
        .map(|((excerpt_source, html_extraction), article)| {
            article_record(article, excerpt_source, *html_extraction)
        })
        .collect::<Vec<_>>();
    ArticleRepository::new(paths)
        .save_fetched_articles(fetched.into_iter().map(|(_, article)| article).collect())
        .expect("save fetched articles");
    write_json(&run_dir.join("articles.json"), &json!(records));
    write_json(&run_dir.join("fetch_report.json"), &json!(report));
}

fn bump(stats: &mut Value, key: &str) {
    stats[key] = json!(stats[key].as_u64().unwrap_or(0) + 1);
}

/// 公開日時（UTC の RFC3339）を並べ替え用の時刻にする。読めない値は最も古い扱い。
fn published_timestamp(text: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(text.trim()).map_or(i64::MIN, |time| time.timestamp())
}

/// `html_extraction` は本文取得に成功したときの抽出元（`body` / `meta_description` / `none`）。
/// 取得に失敗した・保存済みの記事を取り込んだときは `None`（JSON では null）。
fn article_record(
    article: &FetchedArticle,
    excerpt_source: &str,
    html_extraction: Option<&str>,
) -> Value {
    json!({
        "articleId": article.article_id,
        "title": article.title,
        "sourceName": article.source_name,
        "originalUrl": article.original_url,
        "fetchedAt": article.fetched_at,
        "publishedAt": article.published_at_text,
        "genre": article.genre,
        "excerptSource": excerpt_source,
        "htmlExtraction": html_extraction,
        "excerptChars": article.excerpt.as_deref().map_or(0, |text| text.chars().count()),
        "excerpt": article.excerpt,
    })
}

pub(super) fn write_json(path: &Path, value: &Value) {
    std::fs::write(
        path,
        serde_json::to_string_pretty(value).expect("serialize json"),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

/// 保存済みの `articles.json` の記事を一時データフォルダへ取り込み（既にある記事は飛ばす）、
/// `<RUN>/articles.json` として複写する。別の回で同じ記事を評価するため。
pub(super) fn import_articles(articles_file: &Path, run_dir: &Path, paths: &AppPaths) {
    let text = std::fs::read_to_string(articles_file).expect("read saved articles.json");
    let records: Vec<Value> = serde_json::from_str(&text).expect("parse saved articles.json");
    let repository = ArticleRepository::new(paths);
    let existing = repository.existing_article_ids().expect("list articles");
    let field = |record: &Value, key: &str| record[key].as_str().unwrap_or_default().to_string();
    let articles = records
        .iter()
        .filter(|record| !existing.contains(record["articleId"].as_str().unwrap_or_default()))
        .map(|record| {
            let published_at_text = field(record, "publishedAt");
            let fetched_at = record["fetchedAt"]
                .as_str()
                .map_or_else(|| published_at_text.clone(), str::to_string);
            FetchedArticle {
                article_id: field(record, "articleId"),
                title: field(record, "title"),
                source_name: field(record, "sourceName"),
                original_url: field(record, "originalUrl"),
                fetched_at,
                published_at_text,
                genre: field(record, "genre"),
                tags: Vec::new(),
                excerpt: record["excerpt"].as_str().map(str::to_string),
                recommendation_score: 0.0,
                read_state: ArticleReadState::Unread,
            }
        })
        .collect::<Vec<_>>();
    eprintln!(
        "imported {} of {} saved articles",
        articles.len(),
        records.len()
    );
    repository
        .save_fetched_articles(articles)
        .expect("save imported articles");
    let destination = run_dir.join("articles.json");
    if articles_file.canonicalize().ok() != destination.canonicalize().ok() {
        std::fs::write(&destination, text).expect("copy articles.json");
    }
}

pub(super) fn load_article_ids(out_dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(out_dir.join("articles.json"))
        .expect("articles.json (run the fetch phase first)");
    let records: Vec<Value> = serde_json::from_str(&text).expect("parse articles.json");
    records
        .iter()
        .filter_map(|record| record["articleId"].as_str().map(str::to_string))
        .collect()
}

/// 追記先の JSONL に記録済みのキー（記事ID など）を読む。途中で止めても続きから再開できるようにする。
pub(super) fn recorded_keys(
    path: &Path,
    key: impl Fn(&Value) -> Option<String>,
) -> HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| key(&value))
        .collect()
}

pub(super) fn append_line(path: &Path, value: &Value) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open jsonl");
    writeln!(file, "{value}").expect("append jsonl");
}

pub(super) fn error_label(error: &AppError) -> String {
    format!("{error:?}")
}

/// 要約生成の本体（generate_article_summary_with）を、本番と同じ取得手段（request_text → 検証）で呼ぶ。
/// 差し替えるのは「段ごとの生出力・検証結果・所要時間を記録する」ことだけで、入力・順序・検証・保存は本番のまま。
fn run_summaries(out_dir: &Path, paths: &AppPaths, ai: &AiProviderService) {
    let service = SummaryService::new(
        ai.clone(),
        ArticleRepository::new(paths),
        SettingsRepository::new(paths),
    );
    let jsonl = out_dir.join("summaries.jsonl");
    let done = recorded_keys(&jsonl, |value| {
        value["articleId"].as_str().map(str::to_string)
    });
    let article_ids = load_article_ids(out_dir);
    for (index, article_id) in article_ids.iter().enumerate() {
        if done.contains(article_id) {
            continue;
        }
        let stages = RefCell::new(Vec::<Value>::new());
        let started = Instant::now();
        let result = service.generate_article_summary_with(
            GenerateArticleSummaryParams {
                article_id: article_id.clone(),
            },
            FallbackPolicy::SaveFallback,
            |request, kind, provider, level| {
                record_stage(&service, &stages, request, kind, provider, level)
            },
        );
        let total_ms = started.elapsed().as_millis();
        let (status, error) = match &result {
            Ok(SummaryGeneration::Generated(_)) => ("ok", None),
            Ok(SummaryGeneration::AlreadySummarized) => ("already_summarized", None),
            Err(error) => ("error", Some(error_label(error))),
        };
        let tags = ArticleRepository::new(paths)
            .get_article_tags(article_id)
            .unwrap_or_default();
        eprintln!(
            "[{}/{}] {article_id}: {status} {:.1}s",
            index + 1,
            article_ids.len(),
            total_ms as f64 / 1000.0
        );
        append_line(
            &jsonl,
            &json!({
                "articleId": article_id,
                "status": status,
                "error": error,
                "totalMs": total_ms,
                "savedTags": tags,
                "stages": stages.into_inner(),
            }),
        );
    }
}

fn record_stage(
    service: &SummaryService,
    stages: &RefCell<Vec<Value>>,
    request: AiRequest,
    kind: SummaryOutputKind,
    provider: AiProvider,
    level: ExplanationLevel,
) -> Result<AiResponse, AppError> {
    let input_chars = request.input_text.chars().count();
    let input_text = request.input_text.clone();
    // 要約の作り直し（D166 の summary_retry_v2）を最初の要約と分けて数えるため。
    let prompt_id = request.prompt_id.clone();
    // 本番（request_validated_text）と同じく、再説明・一言の口調を入力から判定する（D122）。
    let tone = SpeechTone::for_request(&request.prompt_id, &request.input_text);
    // 再説明・一言の入力は同じ生成の AI 出力（要約・要点）だけなので、確認用に残す。
    let speech_input = matches!(
        kind,
        SummaryOutputKind::Explanation | SummaryOutputKind::Comment
    )
    .then(|| request.input_text.clone());
    let started = Instant::now();
    let raw = service
        .ai_provider_service
        .request_text(request, provider, level);
    let elapsed_ms = started.elapsed().as_millis();
    let mut stage = json!({
        "kind": kind.label(),
        "promptId": prompt_id,
        "ms": elapsed_ms,
        "inputChars": input_chars,
        "speechInput": speech_input,
        "calmTone": tone == SpeechTone::Calm,
    });
    let raw = match raw {
        Ok(raw) => raw,
        Err(error) => {
            stage["error"] = json!(error_label(&error));
            stages.borrow_mut().push(stage);
            return Err(error);
        }
    };
    stage["provider"] = json!(raw.provider);
    stage["raw"] = json!(raw.text);
    stage["rawChars"] = json!(raw.text.chars().count());
    if kind == SummaryOutputKind::Points {
        match validate_article_points(&raw.text) {
            Ok(validated) => {
                stage["tagsRejection"] = json!(validated.tags_rejection.map(|r| r.label()));
            }
            Err(rejection) => stage["rejection"] = json!(rejection.label()),
        }
    } else {
        let cleaned = clean_ai_speech(kind, tone, &input_text, raw.clone());
        stage["cleaned"] = json!(cleaned.text);
        if let Err(rejection) = validate_summary_output(kind, &cleaned.text) {
            stage["rejection"] = json!(rejection.label());
        }
    }
    let selected = select_valid_output_in_tone(kind, tone, &input_text, raw);
    if let Ok(response) = &selected {
        stage["accepted"] = json!(response.text);
        // 機械点検（D166）の結果。要約は作り直しの判断と同じ点検、要点・注目ポイントは記録だけ（作り直さない）。
        let checked = match kind {
            SummaryOutputKind::Summary => Some(response.text.clone()),
            SummaryOutputKind::Points => validate_article_points(&response.text).ok().map(|v| {
                [v.points.key_points, v.points.focus_points].concat().join(
                    "
",
                )
            }),
            _ => None,
        };
        if let Some(checked) = checked {
            stage["check"] = check_json(&input_text, &checked);
        }
    }
    stages.borrow_mut().push(stage);
    selected
}

/// 要約の機械点検（`output_check`）の結果を記録用の JSON にする（すべての種類・作り直しの対象かどうか）。
pub(super) fn check_json(source: &str, output: &str) -> Value {
    let fixed = summary_fixed_prompt_texts();
    let fixed_refs: Vec<&str> = fixed.iter().map(String::as_str).collect();
    let report = check_japanese_summary(source, output, None, &fixed_refs);
    json!({
        "issues": report.issues.iter().map(|issue| json!({
            "kind": issue.kind.label(),
            "retry": issue.kind.triggers_summary_retry(),
            "reason": issue.reason,
        })).collect::<Vec<_>>(),
        "retryKinds": report
            .kinds_where(IssueKind::triggers_summary_retry)
            .iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>(),
        "sourceNumberKept": report.source_number_kept,
        "chars": output.chars().filter(|c| !c.is_whitespace()).count(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TermSelection {
    article_id: String,
    terms: Vec<String>,
}

/// 用語解説の AI 呼び出しを本番の AiProviderService へそのまま委ね、生出力と所要時間だけを横で記録する。
#[derive(Debug)]
struct RecordingTermAi {
    inner: AiProviderService,
    last: Mutex<Option<Value>>,
}

impl TermExplanationAi for RecordingTermAi {
    fn generate_term_explanation(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        let context_chars = request.context.as_deref().map_or(0, |c| c.chars().count());
        // 比較用: フレーズも従来どおり用語解説のプロンプトで作らせる（D21 の前の挙動）。
        // 後処理は dictionary_service 側で行われるため、比べるときは記録の `raw` を見る。
        let mut request = request;
        if std::env::var_os(FORCE_TERM_PROMPT_ENV).is_some() {
            request.prompt_id = TERM_EXPLANATION_PROMPT_ID.to_string();
        }
        let started = Instant::now();
        let result = self
            .inner
            .generate_term_explanation(request, provider, explanation_level);
        let mut record = json!({
            "ms": started.elapsed().as_millis(),
            "contextChars": context_chars,
        });
        match &result {
            Ok(response) => {
                record["provider"] = json!(response.provider);
                record["raw"] = json!(response.text);
            }
            Err(error) => record["aiError"] = json!(error_label(error)),
        }
        *self.last.lock().expect("lock") = Some(record);
        result
    }
}

fn run_terms(out_dir: &Path, paths: &AppPaths, ai: &AiProviderService) {
    let terms_file = std::env::var_os(TERMS_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| out_dir.join("terms.json"));
    let Ok(text) = std::fs::read_to_string(&terms_file) else {
        eprintln!("terms file is missing; the term phase is skipped");
        return;
    };
    let selections: Vec<TermSelection> = serde_json::from_str(&text).expect("parse terms file");
    let recording = Arc::new(RecordingTermAi {
        inner: ai.clone(),
        last: Mutex::new(None),
    });
    let jsonl = out_dir.join("terms.jsonl");
    let done = recorded_keys(&jsonl, |value| {
        Some(format!(
            "{}\u{1f}{}",
            value["articleId"].as_str()?,
            value["term"].as_str()?
        ))
    });
    for (selection_index, selection) in selections.iter().enumerate() {
        for (term_index, term) in selection.terms.iter().enumerate() {
            if done.contains(&format!("{}\u{1f}{term}", selection.article_id)) {
                continue;
            }
            // 語ごとに新しい空の辞書を使う。別の記事で同じ語を選んでも、保存済み辞書の命中で
            // AI 呼び出しが省かれ、時間と採点が実態とずれることがないようにする。
            let dictionary_paths = AppPaths::new(
                out_dir
                    .join("term-dictionaries")
                    .join(format!("{selection_index}-{term_index}")),
            );
            let service = DictionaryService::new(
                DictionaryRepository::new(&dictionary_paths),
                ArticleRepository::new(paths),
                SettingsRepository::new(paths),
                recording.clone(),
            );
            *recording.last.lock().expect("lock") = None;
            let started = Instant::now();
            let result = service.explain_selected_term(ExplainSelectedTermParams {
                article_id: selection.article_id.clone(),
                selected_text: term.clone(),
            });
            let ai_record = recording.last.lock().expect("lock").take();
            let mut record = json!({
                "articleId": selection.article_id,
                "term": term,
                "totalMs": started.elapsed().as_millis(),
                // AI を呼ばずに返った（辞書命中など）ときは true。採点・時間の集計から外す目印。
                "aiSkipped": ai_record.is_none(),
                "ai": ai_record,
            });
            match result {
                Ok(entry) => {
                    record["status"] = json!("ok");
                    // 用語 / フレーズの判定結果（D21）。フレーズは別のプロンプトで説明させるため、集計で分ける。
                    record["type"] = json!(entry.entry_type);
                    record["short"] = json!(entry.short_explanation);
                    record["detail"] = json!(entry.detail_explanation);
                }
                Err(error) => {
                    record["status"] = json!("error");
                    record["error"] = json!(error_label(&error));
                }
            }
            eprintln!(
                "term {} / {term}: {}",
                selection.article_id, record["status"]
            );
            append_line(&jsonl, &record);
        }
    }
}
