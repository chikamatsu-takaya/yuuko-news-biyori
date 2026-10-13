//! 同梱ローカルLLMの品質評価ハーネス（手動実行専用・CI では動かさない）。
//!
//! 実ニュースを**アプリと同じ取得経路**（許可リスト検証 → RSS取得 → HTML本文抽出 → 記事保存）で集め、
//! アプリと同じ要約生成（要約 → 要点・タグ → 再説明 → 一言）と用語解説を provider = local で走らせ、
//! 出力・検証結果・所要時間を JSON で書き出す。品質の判定（採点）は人が出力を読んで行う。
//!
//! 安全のための決まり:
//! - `YUUKO_LLM_EVAL_OUT`（出力先フォルダ）が無ければ何もしない（`--ignored` で動かしても空振りする）。
//! - 実際のアプリデータ（app-data）には書かない。取得元設定・許可リストは
//!   `YUUKO_LLM_EVAL_CONFIG_DIR` のフォルダから**読むだけ**で、出力先の一時データフォルダへ複写して使う。
//! - 出力（記事の本文抜粋を含む）は出力先フォルダにだけ置く。リポジトリへ入れない。
//!
//! 段階（`YUUKO_LLM_EVAL_PHASE`。省略時は fetch → summary → terms をすべて）:
//! - `fetch`: `<OUT>/appdata` を一時データフォルダとして `NewsService::refresh` を1回走らせ、
//!   保存された記事を新しい順に最大 `YUUKO_LLM_EVAL_MAX_ARTICLES`（既定 25）件 `<OUT>/articles.json` へ書く。
//! - `summary`: 各記事に `generate_article_summary` と同じ処理（手動生成の経路）を provider = local で走らせ、
//!   段ごとの生出力・検証結果・所要時間を `<OUT>/summaries.jsonl` へ1記事1行で追記する（記録済みの記事は飛ばす）。
//! - `terms`: `<OUT>/terms.json`（`[{"articleId": "...", "terms": ["..."]}]`。人が記事を読んで選ぶ）の語を
//!   `DictionaryService::explain_selected_term` で解説させ、`<OUT>/terms.jsonl` へ追記する。
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

use super::{
    clean_ai_speech, select_valid_output, validate_article_points, validate_summary_output,
    FallbackPolicy, SummaryGeneration, SummaryOutputKind, SummaryService,
};
use crate::domain::dictionary::ExplainSelectedTermParams;
use crate::domain::settings::{AiProvider, ExplanationLevel, PersistedSettings};
use crate::domain::summary::{AiRequest, AiResponse, GenerateArticleSummaryParams};
use crate::error::AppError;
use crate::paths::{AppPaths, NETWORK_ALLOWLIST_RELATIVE_PATH, NEWS_SOURCES_RELATIVE_PATH};
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::dictionary_repository::DictionaryRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::ai_provider_service::AiProviderService;
use crate::services::dictionary_service::{DictionaryService, TermExplanationAi};
use crate::services::local_llm_service::LocalLlmService;
use crate::services::news_service::NewsService;
use crate::services::recommendation_service::RecommendationService;

const OUT_ENV: &str = "YUUKO_LLM_EVAL_OUT";
const CONFIG_DIR_ENV: &str = "YUUKO_LLM_EVAL_CONFIG_DIR";
const PHASE_ENV: &str = "YUUKO_LLM_EVAL_PHASE";
const MAX_ARTICLES_ENV: &str = "YUUKO_LLM_EVAL_MAX_ARTICLES";
const BUNDLE_DIR_ENV: &str = "YUUKO_LOCAL_LLM_DIR";
const DEFAULT_MAX_ARTICLES: usize = 25;

#[test]
#[ignore = "manual: requires network, the real local LLM bundle and YUUKO_LLM_EVAL_OUT"]
fn local_llm_quality_eval() {
    let Some(out_dir) = std::env::var_os(OUT_ENV).map(PathBuf::from) else {
        eprintln!("{OUT_ENV} is not set; the quality evaluation does nothing");
        return;
    };
    std::fs::create_dir_all(&out_dir).expect("create output dir");
    let paths = AppPaths::new(out_dir.join("appdata"));
    paths.ensure_storage_dirs().expect("create temp data dirs");
    // 要約・用語解説は保存済み設定の provider と解説レベルを読む。初回起動時の初期値（provider = local・
    // 解説レベル normal）を一時データフォルダに置く。
    SettingsRepository::new(&paths)
        .save(&PersistedSettings::initial())
        .expect("save temp settings");

    let phase = std::env::var(PHASE_ENV).unwrap_or_default();
    let run_all = phase.is_empty();
    if run_all || phase == "fetch" {
        run_fetch(&out_dir, &paths);
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
        run_summaries(&out_dir, &paths, &ai);
    }
    if run_all || phase == "terms" {
        run_terms(&out_dir, &paths, &ai);
    }
    local.shutdown();
}

fn bundle_dir() -> PathBuf {
    std::env::var_os(BUNDLE_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("local_llm")
        })
}

/// 取得元設定・許可リストを一時データフォルダへ複写し、アプリの取得処理（NewsService::refresh）を1回走らせる。
fn run_fetch(out_dir: &Path, paths: &AppPaths) {
    let config_dir = std::env::var_os(CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("{CONFIG_DIR_ENV} must point to the folder with news_sources.json")
        });
    for relative in [NEWS_SOURCES_RELATIVE_PATH, NETWORK_ALLOWLIST_RELATIVE_PATH] {
        let name = Path::new(relative).file_name().expect("file name");
        // 読むだけ。書き込み先は一時データフォルダの config/ だけ。
        std::fs::copy(config_dir.join(name), paths.app_data_dir.join(relative))
            .unwrap_or_else(|error| panic!("copy {relative}: {error}"));
    }

    let article_repository = ArticleRepository::new(paths);
    let news = NewsService::new(
        paths,
        article_repository.clone(),
        SettingsRepository::new(paths),
        RecommendationService::new(),
    );
    let started = Instant::now();
    let result = tauri::async_runtime::block_on(news.refresh()).expect("refresh news");
    eprintln!(
        "fetch: {:.1}s sources={} fetched={} saved={} errors={}",
        started.elapsed().as_secs_f32(),
        result.sources_processed,
        result.fetched,
        result.saved,
        result.errors.len()
    );

    let max_articles = std::env::var(MAX_ARTICLES_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_MAX_ARTICLES);
    let mut articles = article_repository
        .existing_article_ids()
        .expect("list articles")
        .into_iter()
        .map(|id| article_repository.get_article_detail(&id).expect("article"))
        .collect::<Vec<_>>();
    articles.sort_by(|a, b| b.published_at_text.cmp(&a.published_at_text));
    let records = articles
        .iter()
        .take(max_articles)
        .map(|article| {
            json!({
                "articleId": article.article_id,
                "title": article.title,
                "sourceName": article.source_name,
                "originalUrl": article.original_url,
                "publishedAt": article.published_at_text,
                "genre": article.genre,
                "excerptChars": article.excerpt.as_deref().map_or(0, |text| text.chars().count()),
                "excerpt": article.excerpt,
            })
        })
        .collect::<Vec<_>>();
    eprintln!("selected {} of {} articles", records.len(), articles.len());
    std::fs::write(
        out_dir.join("articles.json"),
        serde_json::to_string_pretty(&records).expect("serialize articles"),
    )
    .expect("write articles.json");
}

fn load_article_ids(out_dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(out_dir.join("articles.json"))
        .expect("articles.json (run the fetch phase first)");
    let records: Vec<Value> = serde_json::from_str(&text).expect("parse articles.json");
    records
        .iter()
        .filter_map(|record| record["articleId"].as_str().map(str::to_string))
        .collect()
}

/// 追記先の JSONL に記録済みのキー（記事ID など）を読む。途中で止めても続きから再開できるようにする。
fn recorded_keys(path: &Path, key: impl Fn(&Value) -> Option<String>) -> HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| key(&value))
        .collect()
}

fn append_line(path: &Path, value: &Value) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open jsonl");
    writeln!(file, "{value}").expect("append jsonl");
}

fn error_label(error: &AppError) -> String {
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
        "ms": elapsed_ms,
        "inputChars": input_chars,
        "speechInput": speech_input,
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
        let cleaned = clean_ai_speech(kind, raw.clone());
        stage["cleaned"] = json!(cleaned.text);
        if let Err(rejection) = validate_summary_output(kind, &cleaned.text) {
            stage["rejection"] = json!(rejection.label());
        }
    }
    let selected = select_valid_output(kind, raw);
    if let Ok(response) = &selected {
        stage["accepted"] = json!(response.text);
    }
    stages.borrow_mut().push(stage);
    selected
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
    let Ok(text) = std::fs::read_to_string(out_dir.join("terms.json")) else {
        eprintln!("terms.json is missing; the term phase is skipped");
        return;
    };
    let selections: Vec<TermSelection> = serde_json::from_str(&text).expect("parse terms.json");
    let recording = Arc::new(RecordingTermAi {
        inner: ai.clone(),
        last: Mutex::new(None),
    });
    // 辞書は一時データフォルダの空の辞書（保存済み辞書の命中で AI 呼び出しが省かれないようにする）。
    let service = DictionaryService::new(
        DictionaryRepository::new(paths),
        ArticleRepository::new(paths),
        SettingsRepository::new(paths),
        recording.clone(),
    );
    let jsonl = out_dir.join("terms.jsonl");
    let done = recorded_keys(&jsonl, |value| {
        Some(format!(
            "{}\u{1f}{}",
            value["articleId"].as_str()?,
            value["term"].as_str()?
        ))
    });
    for selection in &selections {
        for term in &selection.terms {
            if done.contains(&format!("{}\u{1f}{term}", selection.article_id)) {
                continue;
            }
            *recording.last.lock().expect("lock") = None;
            let started = Instant::now();
            let result = service.explain_selected_term(ExplainSelectedTermParams {
                article_id: selection.article_id.clone(),
                selected_text: term.clone(),
            });
            let mut record = json!({
                "articleId": selection.article_id,
                "term": term,
                "totalMs": started.elapsed().as_millis(),
                "ai": recording.last.lock().expect("lock").take(),
            });
            match result {
                Ok(entry) => {
                    record["status"] = json!("ok");
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
