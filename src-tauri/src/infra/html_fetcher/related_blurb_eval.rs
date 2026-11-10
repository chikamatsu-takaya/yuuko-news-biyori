//! 記事末尾の関連記事の紹介文を除く規則の、実記事での前後比較ハーネス（Firestore タスク mMHWfv・手動実行専用）。
//!
//! 安全のための決まり:
//! - `YUUKO_RELSTRIP_EVAL_OUT`（一時フォルダ）が無ければ何もしない。実際のアプリデータ（app-data）には書かない。
//! - `fetch` 段は、`YUUKO_RELSTRIP_EVAL_CONFIG_DIR`（app-data の config）の取得元・許可リストを**読むだけ**で、
//!   一時フォルダへ複写した許可リスト（＋同梱の既定取得元の許可ホスト）で、製品と同じ URL 検証・取得の部品を使う。
//! - 取得した HTML・抽出結果は一時フォルダにだけ置き、評価の後でフォルダごと消す。リポジトリへ入れない。
//!
//! 段（`YUUKO_RELSTRIP_EVAL_PHASE`）:
//! - `fetch`: 取得元ごとに RSS の先頭から `YUUKO_RELSTRIP_EVAL_PER_SOURCE` 件（既定 4）の HTML を `<OUT>/html/` へ保存する。
//! - `extract`: 保存した HTML を、上限を外した抽出（30,000 字）にかけ、`<OUT>/<LABEL>.json` に書く
//!   （規則を変える前と後の 2 回走らせ、外側のスクリプトで比べる）。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{extract_excerpt_with_limit, ExcerptSource, HtmlFetcher};
use crate::infra::allowlist::NetworkAllowlist;
use crate::infra::rss_client::RssClient;
use crate::infra::url_guard::{validate_url, UrlPurpose};
use crate::paths::{AppPaths, NETWORK_ALLOWLIST_RELATIVE_PATH, NEWS_SOURCES_RELATIVE_PATH};
use crate::services::default_news_sources::{bundled_allowlist, BUNDLED_NEWS_SOURCES};
use crate::services::news_service::NewsSourcesConfig;

const OUT_ENV: &str = "YUUKO_RELSTRIP_EVAL_OUT";
const CONFIG_DIR_ENV: &str = "YUUKO_RELSTRIP_EVAL_CONFIG_DIR";
const PHASE_ENV: &str = "YUUKO_RELSTRIP_EVAL_PHASE";
const LABEL_ENV: &str = "YUUKO_RELSTRIP_EVAL_LABEL";
const PER_SOURCE_ENV: &str = "YUUKO_RELSTRIP_EVAL_PER_SOURCE";
const FULLTEXT_LIMIT_CHARS: usize = 30_000;

#[test]
#[ignore = "manual: requires network (fetch phase) and YUUKO_RELSTRIP_EVAL_OUT"]
fn related_blurb_eval() {
    let Some(out_dir) = std::env::var_os(OUT_ENV).map(PathBuf::from) else {
        eprintln!("{OUT_ENV} is not set; the evaluation does nothing");
        return;
    };
    std::fs::create_dir_all(&out_dir).expect("create output dir");
    match std::env::var(PHASE_ENV).unwrap_or_default().as_str() {
        "fetch" => run_fetch(&out_dir),
        "extract" => run_extract(&out_dir),
        other => panic!("{PHASE_ENV} must be fetch or extract (got {other:?})"),
    }
}

fn write_json(path: &Path, value: &Value) {
    let text = serde_json::to_string_pretty(value).expect("serialize");
    std::fs::write(path, text).expect("write json");
}

/// app-data の取得元・許可リストを一時フォルダへ複写し（同梱の既定取得元はその複写へだけ足す）、
/// 取得元ごとに先頭から上限件数までの記事 HTML を保存する。
fn run_fetch(out_dir: &Path) {
    let config_dir = std::env::var_os(CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{CONFIG_DIR_ENV} must point to the config folder"));
    let paths = AppPaths::new(out_dir.join("appdata"));
    paths.ensure_storage_dirs().expect("create temp data dirs");
    for relative in [NEWS_SOURCES_RELATIVE_PATH, NETWORK_ALLOWLIST_RELATIVE_PATH] {
        let name = Path::new(relative).file_name().expect("file name");
        let source = config_dir.join(name);
        let destination = paths.app_data_dir.join(relative);
        // 同じファイルへの複写（出力先を app-data に向けた設定ミス）は中身を空にしうるため止める。
        if let (Ok(from), Ok(to)) = (source.canonicalize(), destination.canonicalize()) {
            assert_ne!(from, to, "refusing to copy a file onto itself");
        }
        // 読むだけ。書き込み先は一時フォルダの config/ だけ。
        std::fs::copy(&source, &destination).expect("copy config");
    }
    let mut config = NewsSourcesConfig::load(&paths.news_sources_path).expect("load sources");
    let mut allowlist =
        NetworkAllowlist::load(&paths.network_allowlist_path).expect("load allowlist");
    let bundled = bundled_allowlist();
    for domain in bundled.allowed_rss_domains {
        if !allowlist.allowed_rss_domains.contains(&domain) {
            allowlist.allowed_rss_domains.push(domain);
        }
    }
    for domain in bundled.allowed_article_domains {
        if !allowlist.allowed_article_domains.contains(&domain) {
            allowlist.allowed_article_domains.push(domain);
        }
    }
    let mut names: Vec<String> = config.sources.iter().map(|s| s.url.clone()).collect();
    for source in BUNDLED_NEWS_SOURCES {
        if !config.sources.iter().any(|s| s.url == source.feed_url) {
            config
                .sources
                .push(crate::services::news_service::NewsSource {
                    url: source.feed_url.to_string(),
                    genre: source.genre.to_string(),
                });
            names.push(source.name.to_string());
        }
    }
    write_json(
        &paths.network_allowlist_path,
        &serde_json::to_value(&allowlist).expect("serialize allowlist"),
    );

    let per_source = std::env::var(PER_SOURCE_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4usize);
    let html_dir = out_dir.join("html");
    std::fs::create_dir_all(&html_dir).expect("create html dir");
    let rss = RssClient::new(&paths);
    let fetcher = HtmlFetcher::new(&paths);
    let mut index = Vec::new();
    for (source, name) in config.sources.iter().zip(names) {
        let items = match tauri::async_runtime::block_on(rss.fetch(&source.url)) {
            Ok(items) => items,
            Err(_) => {
                eprintln!("{name}: feed failed");
                continue;
            }
        };
        let mut taken = 0;
        for item in items {
            if taken >= per_source {
                break;
            }
            let Ok(url) = validate_url(&item.article_url, UrlPurpose::Article, &allowlist) else {
                continue;
            };
            let Ok(url) = super::canonicalize_request_url(url) else {
                continue;
            };
            let Ok((_, html)) = tauri::async_runtime::block_on(fetcher.fetch_html(url, &allowlist))
            else {
                eprintln!("{name}: article fetch failed");
                continue;
            };
            let file = format!("{}.html", index.len());
            std::fs::write(html_dir.join(&file), html).expect("write html");
            index.push(json!({ "source": name, "file": file }));
            taken += 1;
        }
        eprintln!("{name}: {taken} articles");
    }
    write_json(&out_dir.join("index.json"), &json!(index));
}

/// 保存した HTML を上限なしの抽出にかけ、`<OUT>/<LABEL>.json` に書く。
fn run_extract(out_dir: &Path) {
    let label = std::env::var(LABEL_ENV).unwrap_or_else(|_| "extract".to_string());
    assert!(label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    let index: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(out_dir.join("index.json")).expect("read index.json"),
    )
    .expect("parse index.json");
    let mut out = Vec::new();
    for entry in &index {
        let file = entry["file"].as_str().unwrap_or_default();
        let html = std::fs::read_to_string(out_dir.join("html").join(file)).expect("read html");
        let (body, source) = extract_excerpt_with_limit(&html, FULLTEXT_LIMIT_CHARS);
        out.push(json!({
            "source": entry["source"],
            "file": file,
            "extraction": format!("{source:?}"),
            "isBody": source == ExcerptSource::Body,
            "body": body.unwrap_or_default(),
        }));
    }
    write_json(&out_dir.join(format!("{label}.json")), &json!(out));
}
