//! 要約の機械点検（`output_check`）と1回の作り直しの評価ハーネス（判断台帳 D166。`#[ignore]`・手動実行専用）。
//!
//! 国内記事の評価データ（`eval_data/domestic_articles_synthetic.json`。評価のために書いた架空の記事で、
//! 実在の記事の転載ではない）で、アプリと同じ要約（`summary_v1`）を同梱ローカルLLMで解説レベルごとに作り、
//! 点検の結果と、アプリと同じ作り直し（`check_and_retry_summary`）の結果・所要時間を JSONL に書く。
//! 誤検出（問題がないのに問題ありとする）かどうかの判定は、人（Claude）が出力を読んで行う。
//!
//! 段階（`YUUKO_SUMMARY_CHECK_EVAL_PHASE`）:
//! - `generate`: `<OUT>/summary_checks.jsonl` に1記事×1解説レベルで1行を追記する（記録済みは飛ばす）。
//! - `analyze`: `summary_checks.jsonl` を読み、種類ごとの引っかかりの数と、合格した要約に人工の誤り
//!   （桁違い・材料にない数字・簡体字・指示の写し）を入れたときの検出の率を `<OUT>/analysis.json` に書く。
//!   省略時は両方。
//!
//! 環境変数: `YUUKO_SUMMARY_CHECK_EVAL_OUT`（出力先。無ければ何もしない）、`YUUKO_LLM_EVAL_ARTICLES`
//! （記事の JSON。省略時は上の評価データ）、`YUUKO_LOCAL_LLM_DIR`（同梱物の場所。読み取り専用で参照する）。
//!
//! 実行例（PowerShell）:
//! ```text
//! $env:CARGO_TARGET_DIR = "<一時の target フォルダ>"
//! $env:YUUKO_LOCAL_LLM_DIR = "<同梱物のフォルダ>"
//! $env:YUUKO_SUMMARY_CHECK_EVAL_OUT = "<一時フォルダ>\summary-check"
//! cargo test --manifest-path src-tauri/Cargo.toml -j 4 summary_output_check_eval -- --ignored --nocapture
//! ```

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};

use super::local_llm_quality_eval::{
    append_line, bundle_dir, check_json, error_label, import_articles, load_article_ids,
    recorded_keys, write_json,
};
use super::output_check::{check_japanese_summary, japanese_numbers, CheckReport, IssueKind};
use super::{
    build_focus_points, build_summary_seed, check_and_retry_summary, neutralize_seed_article,
    request_validated_text_via, FallbackPolicy, SummaryOutputKind,
};
use crate::domain::settings::{AiProvider, ExplanationLevel, PersistedSettings};
use crate::domain::summary::{AiRequest, SUMMARY_RETRY_PROMPT_ID};
use crate::paths::AppPaths;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::ai_provider_service::{summary_fixed_prompt_texts, AiProviderService};
use crate::services::local_llm_service::LocalLlmService;

const OUT_ENV: &str = "YUUKO_SUMMARY_CHECK_EVAL_OUT";
const PHASE_ENV: &str = "YUUKO_SUMMARY_CHECK_EVAL_PHASE";
const ARTICLES_ENV: &str = "YUUKO_LLM_EVAL_ARTICLES";
const LEVELS: [ExplanationLevel; 3] = [
    ExplanationLevel::Simple,
    ExplanationLevel::Normal,
    ExplanationLevel::Detailed,
];

#[test]
#[ignore = "manual: requires the real local LLM bundle and YUUKO_SUMMARY_CHECK_EVAL_OUT"]
fn summary_output_check_eval() {
    let Some(out_dir) = std::env::var_os(OUT_ENV).map(PathBuf::from) else {
        eprintln!("{OUT_ENV} is not set; the evaluation does nothing");
        return;
    };
    std::fs::create_dir_all(&out_dir).expect("create output dir");
    let phase = std::env::var(PHASE_ENV).unwrap_or_default();
    if phase.is_empty() || phase == "generate" {
        generate(&out_dir);
    }
    if phase.is_empty() || phase == "analyze" {
        analyze(&out_dir);
    }
}

fn articles_file() -> PathBuf {
    std::env::var_os(ARTICLES_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/services/summary_service/eval_data/domestic_articles_synthetic.json")
        })
}

fn level_label(level: ExplanationLevel) -> &'static str {
    match level {
        ExplanationLevel::Simple => "simple",
        ExplanationLevel::Normal => "normal",
        ExplanationLevel::Detailed => "detailed",
    }
}

/// 記事ごと・解説レベルごとに、アプリと同じ種で要約を作り、点検と作り直しを記録する。
fn generate(out_dir: &Path) {
    let paths = AppPaths::new(out_dir.join("appdata"));
    paths.ensure_storage_dirs().expect("create temp data dirs");
    SettingsRepository::new(&paths)
        .save(&PersistedSettings::initial())
        .expect("save temp settings");
    import_articles(&articles_file(), out_dir, &paths);
    let repository = ArticleRepository::new(&paths);
    let local = LocalLlmService::new(Some(bundle_dir()));
    let ai = AiProviderService::new(&paths).with_local_llm(local.clone());
    let started = Instant::now();
    let check = ai.test_connection(AiProvider::Local);
    eprintln!(
        "warm-up (connection test): {:.1}s {:?}",
        started.elapsed().as_secs_f32(),
        check.status
    );
    let jsonl = out_dir.join("summary_checks.jsonl");
    let done = recorded_keys(&jsonl, |value| {
        Some(format!(
            "{}/{}",
            value["articleId"].as_str()?,
            value["level"].as_str()?
        ))
    });
    for article_id in load_article_ids(out_dir) {
        let article = repository
            .get_article_detail(&article_id)
            .expect("load article");
        for level in LEVELS {
            if done.contains(&format!("{article_id}/{}", level_label(level))) {
                continue;
            }
            let seed_article = neutralize_seed_article(&article);
            let focus = build_focus_points(&seed_article, level);
            let seed = build_summary_seed(&seed_article, level, &focus);
            let started = Instant::now();
            let first = request_validated_text_via(
                &ai,
                AiRequest {
                    prompt_id: "summary_v1".to_string(),
                    input_text: seed.clone(),
                    context: Some(article.title.clone()),
                },
                SummaryOutputKind::Summary,
                AiProvider::Local,
                level,
            );
            let first_ms = started.elapsed().as_millis();
            let mut record = json!({
                "articleId": article_id,
                "level": level_label(level),
                "firstMs": first_ms,
            });
            let first = match first {
                Ok(first) => first,
                Err(error) => {
                    record["error"] = json!(error_label(&error));
                    append_line(&jsonl, &record);
                    continue;
                }
            };
            record["first"] = json!(first.text);
            record["firstCheck"] = check_json(&seed, &first.text);
            // アプリと同じ作り直しの判断を、取得手段に記録を挟んで呼ぶ。
            let retry = RefCell::new(None::<Value>);
            let check_started = Instant::now();
            let adopted = check_and_retry_summary(
                first.clone(),
                &seed,
                AiProvider::Local,
                level,
                FallbackPolicy::SaveFallback,
                &|request: AiRequest, kind, provider, level| {
                    assert_eq!(request.prompt_id, SUMMARY_RETRY_PROMPT_ID);
                    let reasons = request.context.clone();
                    let started = Instant::now();
                    let result = request_validated_text_via(&ai, request, kind, provider, level);
                    let mut stage = json!({
                        "ms": started.elapsed().as_millis(),
                        "reasons": reasons,
                    });
                    match &result {
                        Ok(response) => {
                            stage["text"] = json!(response.text);
                            stage["check"] = check_json(&seed, &response.text);
                        }
                        Err(error) => stage["error"] = json!(error_label(error)),
                    }
                    retry.replace(Some(stage));
                    result
                },
            )
            // 手動の経路（SaveFallback）と同じ扱い。停止のエラーはこの評価では起きない。
            .unwrap_or_else(|_| first.clone());
            let retry = retry.into_inner();
            let retry_ms = retry.as_ref().and_then(|stage| stage["ms"].as_u64());
            // 点検そのものの時間（作り直しの生成時間を除く）。
            let check_us = (check_started.elapsed().as_micros() as u64)
                .saturating_sub(retry_ms.unwrap_or(0) * 1000);
            record["checkUs"] = json!(check_us);
            record["retry"] = json!(retry);
            record["adopted"] = json!(if retry.is_none() {
                "first"
            } else if adopted.text == first.text {
                "kept_first"
            } else {
                "retried"
            });
            eprintln!(
                "{article_id}/{}: first {:.1}s retry {:?} -> {}",
                level_label(level),
                first_ms as f64 / 1000.0,
                retry_ms,
                record["adopted"]
            );
            append_line(&jsonl, &record);
        }
    }
    local.shutdown();
}

/// 引っかかりの集計と、人工の誤りの検出の率。
fn analyze(out_dir: &Path) {
    let text = std::fs::read_to_string(out_dir.join("summary_checks.jsonl"))
        .expect("summary_checks.jsonl (run the generate phase first)");
    let repository = ArticleRepository::new(&AppPaths::new(out_dir.join("appdata")));
    let fixed = summary_fixed_prompt_texts();
    let fixed_refs: Vec<&str> = fixed.iter().map(String::as_str).collect();
    let mut flagged_by_kind = serde_json::Map::new();
    let mut first_count = 0;
    let mut retried = 0;
    let mut adopted_retry = 0;
    let mut flagged: Vec<Value> = Vec::new();
    let mut mutations: Vec<(String, usize, usize)> = Vec::new();
    let mut bump = |name: &str, detected: bool| {
        if let Some(entry) = mutations.iter_mut().find(|(n, _, _)| n == name) {
            entry.1 += 1;
            entry.2 += usize::from(detected);
        } else {
            mutations.push((name.to_string(), 1, usize::from(detected)));
        }
    };
    for line in text.lines() {
        let record: Value = serde_json::from_str(line).expect("parse record");
        let Some(first) = record["first"].as_str() else {
            continue;
        };
        first_count += 1;
        let article_id = record["articleId"].as_str().unwrap_or_default();
        let level = match record["level"].as_str() {
            Some("simple") => ExplanationLevel::Simple,
            Some("detailed") => ExplanationLevel::Detailed,
            _ => ExplanationLevel::Normal,
        };
        let Some(source) = seed_for(&repository, article_id, level) else {
            continue;
        };
        // 点検は記録時の結果ではなく、今の点検でかけ直す（点検を直した後に、生成し直さずに比べるため）。
        let first_report = check_japanese_summary(&source, first, None, &fixed_refs);
        for issue in &first_report.issues {
            let entry = flagged_by_kind
                .entry(issue.kind.label().to_string())
                .or_insert(json!(0));
            *entry = json!(entry.as_u64().unwrap_or(0) + 1);
        }
        if !first_report.issues.is_empty() {
            let retry_text = record["retry"]["text"].as_str();
            flagged.push(json!({
                "articleId": article_id,
                "level": record["level"],
                "first": first,
                "issues": first_report.issues.iter().map(|issue| issue.reason.clone()).collect::<Vec<_>>(),
                "retryKinds": first_report.kinds_where(IssueKind::triggers_summary_retry).iter().map(|k| k.label()).collect::<Vec<_>>(),
                "recordedRetry": retry_text,
                "recordedRetryIssues": retry_text.map(|text| check_japanese_summary(&source, text, None, &fixed_refs)
                    .issues.into_iter().map(|issue| issue.reason).collect::<Vec<_>>()),
            }));
        }
        if !record["retry"].is_null() {
            retried += 1;
        }
        if record["adopted"] == "retried" {
            adopted_retry += 1;
        }
        // 人工の誤りは、全種類の点検に合格した要約にだけ入れる（元から引っかかる要約では検出を区別できない）。
        if !first_report.issues.is_empty() {
            continue;
        }
        let check = |output: &str| check_japanese_summary(&source, output, None, &fixed_refs);
        let has = |report: &CheckReport, kinds: &[IssueKind]| {
            report
                .issues
                .iter()
                .any(|issue| kinds.contains(&issue.kind))
        };
        let numbers = [IssueKind::ScaleMismatch, IssueKind::UnknownNumber];
        if let Some(mutated) = mutate_scale(first) {
            bump("scale_x10", has(&check(&mutated), &numbers));
        }
        if let Some(mutated) = mutate_unknown_number(first, &source) {
            bump("unknown_number", has(&check(&mutated), &numbers));
        }
        if let Some(mutated) = mutate_simplified(first) {
            bump(
                "simplified_char",
                has(&check(&mutated), &[IssueKind::SimplifiedChinese]),
            );
        }
        bump(
            "echo_heading_word",
            has(
                &check(&format!("記事情報によると、{first}")),
                &[IssueKind::PromptEcho],
            ),
        );
        bump(
            "echo_rule_sentence",
            has(
                &check(&format!(
                    "{first}記事情報に書かれている事実だけを使い、記事情報にない数値・順位・日付・期間・出来事を足したり、数や日付を別の言い方に変えたりしないでください。"
                )),
                &[IssueKind::PromptEcho],
            ),
        );
    }
    let analysis = json!({
        "firstSummaries": first_count,
        "flaggedByKind": flagged_by_kind,
        "retried": retried,
        "adoptedRetry": adopted_retry,
        "flagged": flagged,
        "mutations": mutations.iter().map(|(name, total, detected)| json!({
            "name": name, "total": total, "detected": detected,
        })).collect::<Vec<_>>(),
    });
    eprintln!("{}", serde_json::to_string_pretty(&analysis).expect("json"));
    write_json(&out_dir.join("analysis.json"), &analysis);
}

/// generate 段で取り込んだ記事から、アプリと同じ要約の種を作り直す（analyze 段で点検をかけ直すため）。
fn seed_for(
    repository: &ArticleRepository,
    article_id: &str,
    level: ExplanationLevel,
) -> Option<String> {
    let article = repository.get_article_detail(article_id).ok()?;
    let seed_article = neutralize_seed_article(&article);
    let focus = build_focus_points(&seed_article, level);
    Some(build_summary_seed(&seed_article, level, &focus))
}

/// 要約の最初の「単位も区切りもない算用数字」を 10 倍にする（「12億」→「120億」）。
fn mutate_scale(text: &str) -> Option<String> {
    let found = japanese_numbers(text)
        .into_iter()
        .find(|found| found.text.chars().all(|c| c.is_ascii_digit()) && found.text != "0")?;
    let position = text.find(&found.text)?;
    Some(format!(
        "{}0{}",
        &text[..position + found.text.len()],
        &text[position + found.text.len()..]
    ))
}

/// 要約の最初の算用数字を、材料にない数（材料の数のどれとも 10 のべき乗倍でない数）に置き換える。
fn mutate_unknown_number(text: &str, source: &str) -> Option<String> {
    let found = japanese_numbers(text)
        .into_iter()
        .find(|found| found.text.chars().all(|c| c.is_ascii_digit()))?;
    let value: u64 = found.text.parse().ok()?;
    let source_numbers = super::output_check::japanese_source_numbers(source);
    let replacement = (1..50).map(|n| value * 3 + 7 * n).find(|candidate| {
        let candidate = *candidate as f64;
        !source_numbers.allowed.iter().any(|allowed| {
            let ratio = (candidate / allowed).log10();
            *allowed != 0.0 && (ratio - ratio.round()).abs() < 1e-9
        })
    })?;
    let position = text.find(&found.text)?;
    Some(format!(
        "{}{replacement}{}",
        &text[..position],
        &text[position + found.text.len()..]
    ))
}

/// 要約の最初の「日本語の字で、簡体字の形が別にある字」を簡体字に置き換える。
fn mutate_simplified(text: &str) -> Option<String> {
    const PAIRS: [(char, char); 17] = [
        ('発', '发'),
        ('電', '电'),
        ('経', '经'),
        ('済', '济'),
        ('説', '说'),
        ('対', '对'),
        ('時', '时'),
        ('間', '间'),
        ('問', '问'),
        ('題', '题'),
        ('業', '业'),
        ('東', '东'),
        ('車', '车'),
        ('長', '长'),
        ('門', '门'),
        ('図', '图'),
        ('億', '亿'),
    ];
    let (from, to) = text
        .chars()
        .find_map(|c| PAIRS.iter().find(|(from, _)| *from == c).copied())?;
    Some(text.replacen(from, &to.to_string(), 1))
}
