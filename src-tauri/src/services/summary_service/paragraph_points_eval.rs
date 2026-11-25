//! 段落ごとに1行ずつまとめる方式（記事の最後まで扱う）の評価ハーネス（Firestore タスク t3JFzB・手動実行専用）。
//!
//! 製品の要約（タイトル＋本文抜粋の先頭約1500字から「1〜2文」）は記事の後半を落とす。2B のモデルは長い入力・
//! 長い出力で誤りが増えた（D126・D167-C）ため、短い入力（段落）から短い出力（1行）を記事の最後まで作り、
//! 1行ずつその段落と機械で突き合わせて怪しい行を落とす方式を試す。**製品の経路には組み込まない**（評価だけ）。
//!
//! 方式:
//! 1. 本文（`HtmlFetcher` の抽出規則のまま上限だけを外した全文。段落は空行区切り）を段落に分け、短い段落
//!    （見出し・箇条書きの1項目・短い段落など。`MIN_BLOCK_CHARS` 未満）は次の段落の頭へつなぎ、長い段落
//!    （`MAX_CHUNK_CHARS` 超）は「。」で文に分けて数文ずつ（`MAX_CHUNK_SENTENCES` 文まで）のかたまりにする。
//! 2. かたまりごとに、タイトルと前のかたまりの最初の文を参考として渡し、40〜60字くらいの1行を同梱ローカルLLMに頼む。
//! 3. 1行ずつ、その行のかたまり（＋タイトル）と製品の要約の点検（`check_japanese_summary`: 数字・年・反対の意味
//!    〔勝敗を含む〕・推測の言い切り・記事にない因果・指示の写し・簡体字など）と、この評価だけの固有名詞の点検
//!    （`unknown_names`: 行のカタカナ語・英字語が材料にない）で照らし、引っかかった行を落とす（作り直さない）。
//!
//! 安全のための決まり:
//! - `YUUKO_LLM_EVAL_OUT` が無ければ何もしない。実際のアプリデータ（app-data）には書かない。
//! - `fulltext` 段の取得は、`local_llm_quality_eval` の `fetch` 段が作った一時データフォルダの許可リスト
//!   （`<ARTICLES のフォルダ>/appdata`）で、製品と同じ URL 検証・取得の部品を使う。
//! - 全文は出力先フォルダ（一時フォルダ）にだけ置き、評価の後で消す。リポジトリへ入れない。
//!   製品に入れるなら全文はメモリ上だけで扱い、保存しない（取得元の利用条件の確認が前提）。
//!
//! 環境変数:
//! - `YUUKO_LLM_EVAL_OUT` / `YUUKO_LLM_EVAL_LABEL`: 出力先（`local_llm_quality_eval` と同じ）。
//! - `YUUKO_PARA_EVAL_PHASE`: `fulltext`（全文を取る）/ `lines`（1行ずつまとめる）。
//! - `YUUKO_LLM_EVAL_ARTICLES`: `fulltext` 段で読む `articles.json`（`local_llm_quality_eval` の `fetch` 段の出力）。
//! - `YUUKO_PARA_EVAL_FULLTEXT`: `lines` 段で読む `fulltext.json`。
//! - `YUUKO_LOCAL_LLM_DIR`: 同梱物の場所（`local_llm_quality_eval` と同じ）。
//!
//! 実行例（PowerShell）:
//! ```text
//! $env:YUUKO_LLM_EVAL_OUT = "<一時フォルダ>"; $env:YUUKO_LLM_EVAL_LABEL = "new1"
//! $env:YUUKO_PARA_EVAL_PHASE = "lines"; $env:YUUKO_PARA_EVAL_FULLTEXT = "<一時フォルダ>\fulltext.json"
//! cargo test --manifest-path src-tauri/Cargo.toml paragraph_points_eval -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::consistency_check::strip_heading_labels;
use super::eval_model_override::eval_local_llm;
use super::local_llm_quality_eval::{append_line, bundle_dir, recorded_keys, write_json};
use super::output_check::{check_japanese_summary, to_halfwidth, IssueKind};
use super::{neutralize_seed_text, seed_title_without_site_suffix};
use crate::infra::html_fetcher::{ExcerptSource, HtmlFetcher};
use crate::paths::AppPaths;
use crate::services::local_llm_service::LocalLlmService;

const OUT_ENV: &str = "YUUKO_LLM_EVAL_OUT";
const LABEL_ENV: &str = "YUUKO_LLM_EVAL_LABEL";
const PHASE_ENV: &str = "YUUKO_PARA_EVAL_PHASE";
const ARTICLES_ENV: &str = "YUUKO_LLM_EVAL_ARTICLES";
const FULLTEXT_ENV: &str = "YUUKO_PARA_EVAL_FULLTEXT";

/// 全文を取るときの抜粋の上限（評価だけ）。ふつうの記事は収まり、巨大なページの負荷は抑える。
const FULLTEXT_LIMIT_CHARS: usize = 30_000;
/// これより短いブロック（見出し・箇条書きの1項目・画像の説明・1〜2文の短い段落など）は単独で1行にせず、
/// 次の段落の頭へつなぐ。1行（40〜60字）にまとめる元が短すぎると、まとめではなく段落の写しになるため。
const MIN_BLOCK_CHARS: usize = 120;
/// これより長い段落は「。」で文に分け、数文ずつのかたまりにする。
const MAX_CHUNK_CHARS: usize = 250;
/// 長い段落を分けたかたまりの文の数の上限。
const MAX_CHUNK_SENTENCES: usize = 3;
/// 参考に渡す「前のかたまりの最初の文」の上限。
const PREVIOUS_SENTENCE_MAX_CHARS: usize = 80;
/// 参考に渡すタイトルの上限（製品の種と同じ）。
const TITLE_MAX_CHARS: usize = 200;
/// かたまりの上限（1文がとても長い段落でも入力が長くなりすぎないように）。
const CHUNK_MAX_CHARS: usize = 600;
/// 1行の出力トークンの上限（60字前後の日本語に足り、言い足しの長い出力を途中で止める）。
const LINE_MAX_TOKENS: u32 = 160;
/// 空白を除いてこの文字数を超える行は「長すぎる」として落とす（頼むのは 40〜60 字）。
const LINE_MAX_CHARS: usize = 80;

/// 1行の指示（固定文）。指示の写しの点検に使うので、記事を含まない行だけにする。
const LINE_INSTRUCTIONS: [&str; 8] = [
    "次の「まとめる段落」を、日本語の1行で短くまとめてください。",
    "- 40〜60文字くらいの1文にする。",
    "- まとめる段落に書かれた事実だけを使う。段落にない数値・日付・名前・出来事を足さない。",
    "- 推測・予定・意見として書かれたことは、推測・予定・意見のまま書く。",
    "- 誰が何をしたのか（主語と目的語の関係）は段落のとおりに書く。",
    "- 段落の文をそのまま写さず、いちばん大事な点だけを書く。",
    "- 記事のタイトルと前の段落の書き出しは、話の流れを知るための参考。まとめには使わない。",
    "- 出力はまとめの1行だけ。見出し・記号・説明は付けない。",
];

/// 1行にまとめる単位（段落、または長い段落を分けたかたまり）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Chunk {
    pub(super) text: String,
    /// 本文の中での開始位置（文字数）。記事の前半・後半を分けるため。
    pub(super) start_char: usize,
}

/// 本文を1行にまとめる単位へ分ける（純粋関数）。段落は空行区切り（`HtmlFetcher` のブロックのつなぎ方）。
pub(super) fn split_chunks(body: &str) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut pending = String::new();
    let mut pending_start = None;
    let mut offset = 0;
    for raw_block in body.split("\n\n") {
        let block_start = offset + leading_chars(raw_block);
        offset += raw_block.chars().count() + 2;
        let block = raw_block.trim();
        if block.is_empty() {
            continue;
        }
        let start = pending_start.unwrap_or(block_start);
        let merged = if pending.is_empty() {
            block.to_string()
        } else {
            format!("{pending} {block}")
        };
        if merged.chars().count() < MIN_BLOCK_CHARS {
            pending = merged;
            pending_start = Some(start);
            continue;
        }
        pending.clear();
        pending_start = None;
        push_paragraph(&mut chunks, &merged, start);
    }
    if !pending.is_empty() {
        // 最後に残った短いブロック（記事末尾の短い一文など）は前のかたまりの後ろへつなぐ。
        match chunks.last_mut() {
            Some(last) => {
                last.text.push(' ');
                last.text.push_str(&pending);
            }
            None => chunks.push(Chunk {
                text: pending,
                start_char: pending_start.unwrap_or(0),
            }),
        }
    }
    chunks
}

fn leading_chars(text: &str) -> usize {
    text.chars().take_while(|c| c.is_whitespace()).count()
}

/// 段落を1つ以上のかたまりにして足す。長い段落は文に分け、`MAX_CHUNK_CHARS`・`MAX_CHUNK_SENTENCES` まで束ねる。
fn push_paragraph(chunks: &mut Vec<Chunk>, paragraph: &str, start: usize) {
    if paragraph.chars().count() <= MAX_CHUNK_CHARS {
        chunks.push(Chunk {
            text: paragraph.to_string(),
            start_char: start,
        });
        return;
    }
    let mut current = String::new();
    let mut current_start = start;
    let mut sentences = 0;
    let mut position = start;
    for sentence in split_sentences(paragraph) {
        let length = sentence.chars().count();
        let over = !current.is_empty()
            && (current.chars().count() + length > MAX_CHUNK_CHARS
                || sentences >= MAX_CHUNK_SENTENCES);
        if over {
            chunks.push(Chunk {
                text: std::mem::take(&mut current).trim().to_string(),
                start_char: current_start,
            });
            sentences = 0;
            current_start = position;
        }
        current.push_str(&sentence);
        sentences += 1;
        position += length;
    }
    if !current.trim().is_empty() {
        chunks.push(Chunk {
            text: current.trim().to_string(),
            start_char: current_start,
        });
    }
}

/// 「。」で文に分ける（直後の閉じ括弧は前の文に含める）。句点のない残りも1文とする。
fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut index = 0;
    while index < chars.len() {
        current.push(chars[index]);
        if chars[index] == '。' {
            while index + 1 < chars.len() && matches!(chars[index + 1], '」' | '』' | '）' | ')')
            {
                index += 1;
                current.push(chars[index]);
            }
            sentences.push(std::mem::take(&mut current));
        }
        index += 1;
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    sentences
}

/// かたまりの最初の文（参考として次のかたまりに渡す）。
fn first_sentence(text: &str) -> String {
    let sentence = split_sentences(text).into_iter().next().unwrap_or_default();
    sentence
        .trim()
        .chars()
        .take(PREVIOUS_SENTENCE_MAX_CHARS)
        .collect()
}

/// 1行のプロンプト。外部由来の文（タイトル・本文）は製品の種と同じ無害化（`neutralize_seed_text`）をかける。
fn build_line_prompt(title: &str, previous: Option<&str>, chunk: &str) -> String {
    let previous = previous
        .map(|text| neutralize_seed_text(text, PREVIOUS_SENTENCE_MAX_CHARS))
        .unwrap_or_else(|| "（なし。これが最初の段落）".to_string());
    format!(
        "{}\n\n記事のタイトル: {}\n前の段落の書き出し: {}\nまとめる段落:\n{}",
        LINE_INSTRUCTIONS.join("\n"),
        neutralize_seed_text(title, TITLE_MAX_CHARS),
        previous,
        neutralize_seed_text(chunk, CHUNK_MAX_CHARS),
    )
}

/// 生出力から1行を取り出す: 最初の空でない行の、見出し・箇条書きの記号を外し、空白をつめる。
pub(super) fn clean_line(raw: &str) -> String {
    let first = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let (stripped, _) = strip_heading_labels(first);
    let stripped = stripped
        .trim()
        .trim_start_matches(['・', '-', '*', '●', '■', '•', '＊', '－'])
        .trim();
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 行の中の固有名詞らしい語（カタカナ 3 字以上・英字で始まる 2 字以上の英数字）で、材料にないもの。
/// 漢字の名前（人名・社名）は形で見分けられないので見ない。
fn unknown_names(source: &str, line: &str) -> Vec<String> {
    let source = normalize_name(source);
    names_in(line)
        .into_iter()
        .filter(|name| !source.contains(&normalize_name(name)))
        .collect()
}

pub(super) fn names_in(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().map(to_halfwidth).collect();
    let mut names = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let (is_part, min_len): (fn(char) -> bool, usize) = if is_katakana(c) && c != 'ー' {
            (|c| is_katakana(c) || c == '・', 3)
        } else if c.is_ascii_alphabetic() {
            (
                |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'),
                2,
            )
        } else {
            index += 1;
            continue;
        };
        let start = index;
        while index < chars.len() && is_part(chars[index]) {
            index += 1;
        }
        let word: String = chars[start..index]
            .iter()
            .collect::<String>()
            .trim_end_matches(['・', '.', '-'])
            .to_string();
        if word.chars().filter(|c| *c != '・').count() >= min_len {
            names.push(word);
        }
    }
    names
}

fn is_katakana(c: char) -> bool {
    matches!(c, 'ァ'..='ヺ' | 'ー')
}

pub(super) fn normalize_name(text: &str) -> String {
    text.chars()
        .map(to_halfwidth)
        .filter(|c| !c.is_whitespace() && *c != '・')
        .flat_map(char::to_lowercase)
        .collect()
}

/// 行と材料が共有する、いちばん長い連続した文字列の長さ（段落の文の写しの度合いを見るため。落とす理由にはしない）。
pub(super) fn longest_common_run(source: &str, line: &str) -> usize {
    let a: Vec<char> = source.chars().collect();
    let b: Vec<char> = line.chars().collect();
    let mut previous = vec![0usize; b.len() + 1];
    let mut best = 0;
    for x in &a {
        let mut current = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            if x == y {
                current[j + 1] = previous[j] + 1;
                best = best.max(current[j + 1]);
            }
        }
        previous = current;
    }
    best
}

/// 1行の点検。落とす理由のラベル（重複なし）を返す。空なら残す。
/// 製品の要約と同じ点検（作り直しの理由にする種類＝`triggers_summary_retry`）と長さ、固有名詞の点検を使う。
fn check_line(source: &str, line: &str) -> Vec<&'static str> {
    if line.chars().all(char::is_whitespace) {
        return vec![IssueKind::Empty.label()];
    }
    let report = check_japanese_summary(source, line, Some(LINE_MAX_CHARS), &LINE_INSTRUCTIONS);
    let mut labels: Vec<&'static str> = report
        .issues
        .iter()
        .filter(|issue| issue.kind.triggers_summary_retry() || issue.kind == IssueKind::TooLong)
        .map(|issue| issue.kind.label())
        .collect();
    if !unknown_names(source, line).is_empty() {
        labels.push("unknown_name");
    }
    labels.sort_unstable();
    labels.dedup();
    labels
}

#[test]
#[ignore = "manual: requires network or the real local LLM bundle and YUUKO_LLM_EVAL_OUT"]
fn paragraph_points_eval() {
    let Some(out_dir) = std::env::var_os(OUT_ENV).map(PathBuf::from) else {
        eprintln!("{OUT_ENV} is not set; the paragraph evaluation does nothing");
        return;
    };
    let run_dir = match std::env::var(LABEL_ENV).ok().filter(|l| !l.is_empty()) {
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
    match std::env::var(PHASE_ENV).unwrap_or_default().as_str() {
        "fulltext" => run_fulltext(&run_dir),
        "lines" => run_lines(&run_dir),
        other => panic!("{PHASE_ENV} must be fulltext or lines (got {other:?})"),
    }
}

/// `articles.json` の記事の全文を、fetch 段の一時データフォルダの許可リストで取り直す（上限だけを外す）。
fn run_fulltext(run_dir: &Path) {
    let articles_file = std::env::var_os(ARTICLES_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{ARTICLES_ENV} must point to articles.json of a fetch run"));
    let fetch_dir = articles_file.parent().expect("articles.json folder");
    let paths = AppPaths::new(fetch_dir.join("appdata"));
    let fetcher = HtmlFetcher::new(&paths);
    let text = std::fs::read_to_string(&articles_file).expect("read articles.json");
    let records: Vec<Value> = serde_json::from_str(&text).expect("parse articles.json");
    let mut out = Vec::new();
    for record in &records {
        let field = |key: &str| record[key].as_str().unwrap_or_default().to_string();
        let result = tauri::async_runtime::block_on(
            fetcher.fetch_and_extract_with_limit(&field("originalUrl"), FULLTEXT_LIMIT_CHARS),
        );
        let (body, extraction) = match result {
            Ok(result) => (result.excerpt, result.excerpt_source),
            Err(_) => (None, ExcerptSource::None),
        };
        let Some(body) = body.filter(|_| extraction == ExcerptSource::Body) else {
            eprintln!("{}: no body", field("articleId"));
            continue;
        };
        out.push(json!({
            "articleId": field("articleId"),
            "title": field("title"),
            "sourceName": field("sourceName"),
            "excerptChars": record["excerptChars"],
            "bodyChars": body.chars().count(),
            "chunks": split_chunks(&body).len(),
            "body": body,
        }));
    }
    eprintln!("fulltext: {} of {} articles", out.len(), records.len());
    write_json(&run_dir.join("fulltext.json"), &json!(out));
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FullText {
    article_id: String,
    title: String,
    source_name: String,
    body: String,
}

/// 全文の記事ごとに、かたまりごとの1行を作って点検し、`<RUN>/lines.jsonl` へ1記事1行で追記する。
fn run_lines(run_dir: &Path) {
    let fulltext = std::env::var_os(FULLTEXT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{FULLTEXT_ENV} must point to fulltext.json"));
    let articles: Vec<FullText> =
        serde_json::from_str(&std::fs::read_to_string(fulltext).expect("read fulltext.json"))
            .expect("parse fulltext.json");
    let jsonl = run_dir.join("lines.jsonl");
    let done = recorded_keys(&jsonl, |value| {
        value["articleId"].as_str().map(str::to_string)
    });
    let local = eval_local_llm(bundle_dir());
    let started = Instant::now();
    let warmup = local.generate("「はい」とだけ答えてください。", 8);
    eprintln!(
        "warm-up: {:.1}s ok={}",
        started.elapsed().as_secs_f32(),
        warmup.is_ok()
    );
    for (index, article) in articles.iter().enumerate() {
        if done.contains(&article.article_id) {
            continue;
        }
        let record = summarize_article(&local, article);
        eprintln!(
            "[{}/{}] {}: lines={} dropped={} {:.1}s",
            index + 1,
            articles.len(),
            article.article_id,
            record["lines"].as_array().map_or(0, Vec::len),
            record["dropped"],
            record["totalMs"].as_u64().unwrap_or(0) as f64 / 1000.0
        );
        append_line(&jsonl, &record);
    }
    local.shutdown();
}

fn summarize_article(local: &LocalLlmService, article: &FullText) -> Value {
    let title = seed_title_without_site_suffix(&article.title, &article.source_name);
    let body_chars = article.body.chars().count();
    let chunks = split_chunks(&article.body);
    let started = Instant::now();
    let mut lines = Vec::new();
    let mut dropped = 0;
    let mut previous: Option<String> = None;
    for (index, chunk) in chunks.iter().enumerate() {
        let prompt = build_line_prompt(title, previous.as_deref(), &chunk.text);
        let line_started = Instant::now();
        let generated = local.generate(&prompt, LINE_MAX_TOKENS);
        let ms = line_started.elapsed().as_millis();
        let (raw, error) = match generated {
            Ok(text) => (text.trim().to_string(), None),
            Err(failure) => (String::new(), Some(format!("{failure:?}"))),
        };
        let line = clean_line(&raw);
        // 点検の材料は、その行のかたまりとタイトル（参考に渡した前のかたまりの文は入れない）。
        let source = format!("{title}\n{}", chunk.text);
        let issues = check_line(&source, &line);
        if !issues.is_empty() {
            dropped += 1;
        }
        lines.push(json!({
            "index": index,
            "startChar": chunk.start_char,
            "position": chunk.start_char as f64 / body_chars.max(1) as f64,
            "chunkChars": chunk.text.chars().count(),
            "ms": ms,
            "raw": raw,
            "rawLines": raw.lines().filter(|l| !l.trim().is_empty()).count(),
            "line": line,
            "lineChars": line.chars().filter(|c| !c.is_whitespace()).count(),
            "copyRun": longest_common_run(&chunk.text, &line),
            "unknownNames": unknown_names(&source, &line),
            "issues": issues,
            "dropped": !issues.is_empty(),
            "error": error,
        }));
        previous = Some(first_sentence(&chunk.text));
    }
    json!({
        "articleId": article.article_id,
        "bodyChars": body_chars,
        "chunks": chunks.len(),
        "totalMs": started.elapsed().as_millis(),
        "dropped": dropped,
        "lines": lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_blocks_join_the_next_paragraph_and_long_ones_split_at_sentences() {
        let long = "一つ目の文です。".repeat(40) + "最後の文です。";
        let body = format!(
            "見出し\n\n{}\n\n{long}\n\n短い結び",
            "この段落は百二十字を超える長さの普通の段落で、そのまま一つのかたまりになります。"
                .repeat(4)
        );
        let chunks = split_chunks(&body);
        assert!(chunks[0].text.starts_with("見出し この段落は"));
        assert_eq!(chunks[0].start_char, 0);
        let long_chunks = &chunks[1..];
        assert!(long_chunks.len() >= 2);
        assert!(long_chunks
            .iter()
            .all(|c| c.text.chars().count() <= MAX_CHUNK_CHARS + 10));
        assert!(long_chunks
            .iter()
            .all(|c| c.text.matches('。').count() <= MAX_CHUNK_SENTENCES));
        assert!(chunks
            .last()
            .unwrap()
            .text
            .ends_with("最後の文です。 短い結び"));
        // 位置は本文の中で増えていく（後半の判定に使う）。
        assert!(chunks.windows(2).all(|w| w[0].start_char < w[1].start_char));
        assert_eq!(
            body.chars()
                .skip(chunks[1].start_char)
                .take(5)
                .collect::<String>(),
            "一つ目の文"
        );
    }

    #[test]
    fn sentences_keep_closing_brackets() {
        assert_eq!(
            split_sentences("彼は「行く。」と言った。残り"),
            vec!["彼は「行く。」", "と言った。", "残り"]
        );
        assert_eq!(first_sentence("最初の文。次の文。"), "最初の文。");
    }

    #[test]
    fn prompt_has_fixed_instructions_and_neutralized_materials() {
        let prompt = build_line_prompt("# タイトル", None, "<b>本文</b>です。");
        assert!(prompt.starts_with(LINE_INSTRUCTIONS[0]));
        assert!(prompt.contains("（なし。これが最初の段落）"));
        assert!(!prompt.contains("<b>"));
        assert!(prompt.contains("＃ タイトル"));
    }

    #[test]
    fn cleaned_line_is_the_first_line_without_bullets() {
        assert_eq!(
            clean_line("\n・ A社が  新製品を発表した。\n説明"),
            "A社が 新製品を発表した。"
        );
    }

    #[test]
    fn unknown_names_are_katakana_or_latin_words_missing_from_the_source() {
        let source = "グーグルはGemini 3を発表した。";
        assert!(unknown_names(source, "グーグルがＧｅｍｉｎｉ 3を出した。").is_empty());
        assert_eq!(
            unknown_names(source, "マイクロソフトがGPT-5を出した。"),
            vec!["マイクロソフト", "GPT-5"]
        );
    }

    #[test]
    fn line_checks_flag_unknown_numbers_names_and_long_lines() {
        let source = "タイトル\nA社は売上が10億円だったと発表した。";
        assert!(check_line(source, "A社の売上は10億円だった。").is_empty());
        assert!(check_line(source, "A社の売上は20億円だった。").contains(&"unknown_number"));
        assert!(check_line(source, "オラクルの売上は10億円だった。").contains(&"unknown_name"));
        assert!(check_line(source, &"あ".repeat(LINE_MAX_CHARS + 1)).contains(&"too_long"));
        assert_eq!(check_line(source, " "), vec!["empty"]);
    }

    #[test]
    fn longest_common_run_counts_copied_characters() {
        assert_eq!(
            longest_common_run("今日は良い天気です。", "明日は良い天気かな"),
            6
        );
    }
}
