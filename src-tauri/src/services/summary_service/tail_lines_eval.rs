//! 要約に記事の後半を少しだけ足す方式の評価ハーネス（Firestore タスク Oy5uHSDgQnwYOaGdJQ4m・手動実行専用）。
//!
//! 製品の要約・要点は本文抜粋の先頭約1500字（`SEED_EXCERPT_MAX_CHARS`）しか見ないため、記事の後半の事実が
//! 伝わらない。§14（段落ごとに1行ずつ・記事の最後まで）は誤りが約4倍に増えて不採用になった。そこで §14.4 の
//! 「次に試すなら」から誤りが増えにくい形だけを試す: **今の要約・要点はそのまま**にして、先頭約1500字より後ろの
//! 本文から**短い追加の行を2〜3行だけ**作る。**製品の経路には組み込まない**（評価だけ）。
//!
//! 方式（`YUUKO_TAIL_EVAL_MODE`）:
//! - `block`: 後ろの本文（段落の切れ目で `TAIL_BLOCK_MAX_CHARS` 字まで）を1回で渡し、2〜3行にまとめさせる。
//! - `paras`: 後ろの本文を段落のかたまり（§14 と同じ分け方）にし、均等に選んだ3つまでを1行ずつまとめさせる。
//!   主語が段落に書かれていず段落だけでは分からないときは「なし」と答えさせ、行を作らない。
//!
//! どちらも §14.4 の手当てを入れる: 行の数を絞る（3行まで）・タイトルや前の段落を手がかりに渡さない・
//! 主語を推測で補わせない・点検は製品の要約の点検（作り直しの理由にする種類）と英字の語の点検だけで、
//! 長さ・カタカナ語では落とさない（§14.2 で誤検出が多かった）。引っかかった行は落とす（作り直さない）。
//!
//! 安全のための決まり:
//! - `YUUKO_LLM_EVAL_OUT` が無ければ何もしない。実際のアプリデータ（app-data）には書かない。
//! - 全文は `paragraph_points_eval` の `fulltext` 段が一時フォルダに書いたものを読むだけ。通信しない。
//!   出力も一時フォルダにだけ置く。リポジトリへ入れない。
//!
//! 環境変数:
//! - `YUUKO_LLM_EVAL_OUT` / `YUUKO_LLM_EVAL_LABEL`: 出力先（`local_llm_quality_eval` と同じ）。
//! - `YUUKO_TAIL_EVAL_FULLTEXT`: 読む `fulltext.json`（`paragraph_points_eval` の `fulltext` 段の出力）。
//! - `YUUKO_TAIL_EVAL_MODE`: `block` / `paras`。
//! - `YUUKO_LOCAL_LLM_DIR`: 同梱物の場所（`local_llm_quality_eval` と同じ）。
//!
//! 実行例（PowerShell）:
//! ```text
//! $env:YUUKO_LLM_EVAL_OUT = "<一時フォルダ>"; $env:YUUKO_LLM_EVAL_LABEL = "tailA1"
//! $env:YUUKO_TAIL_EVAL_MODE = "block"; $env:YUUKO_TAIL_EVAL_FULLTEXT = "<一時フォルダ>\fulltext.json"
//! cargo test --manifest-path src-tauri/Cargo.toml --lib tail_lines_eval::tail_lines_eval -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::eval_model_override::eval_local_llm;
use super::local_llm_quality_eval::{append_line, bundle_dir, recorded_keys};
use super::neutralize_seed_text;
use super::output_check::check_japanese_summary;
use super::paragraph_points_eval::{
    clean_line, longest_common_run, names_in, normalize_name, split_chunks,
};
use crate::services::local_llm_service::LocalLlmService;

const OUT_ENV: &str = "YUUKO_LLM_EVAL_OUT";
const LABEL_ENV: &str = "YUUKO_LLM_EVAL_LABEL";
const FULLTEXT_ENV: &str = "YUUKO_TAIL_EVAL_FULLTEXT";
const MODE_ENV: &str = "YUUKO_TAIL_EVAL_MODE";

/// 製品の要約が見る本文の先頭の文字数（`SEED_EXCERPT_MAX_CHARS` と同じ）。これより後ろを「後ろの本文」とする。
const HEAD_CHARS: usize = 1_500;
/// 後ろの本文がこれより短い記事は行を作らない（まとめる事実がほとんど無く、写しになりやすい）。
const TAIL_MIN_CHARS: usize = 200;
/// `block` で1回に渡す後ろの本文の上限（製品の種と同じ長さ。長い入力で 2B の誤りが増えるため）。
const TAIL_BLOCK_MAX_CHARS: usize = 1_500;
/// `paras` でまとめるかたまりの数の上限。
const MAX_LINES: usize = 3;
/// `paras` で渡すかたまりの上限（`paragraph_points_eval` と同じ）。
const CHUNK_MAX_CHARS: usize = 600;
const BLOCK_MAX_TOKENS: u32 = 320;
const LINE_MAX_TOKENS: u32 = 160;

/// `block` の指示（固定文）。指示の写しの点検に使うので、記事を含まない行だけにする。
const BLOCK_INSTRUCTIONS: [&str; 8] = [
    "次の「記事の続き」は、ニュース記事の途中から後ろの本文です（記事の前半は別にまとめてあります）。",
    "記事の続きに書かれた大事な事実を、日本語で2〜3行にまとめてください。",
    "- 1行は40〜60文字くらいの1文にし、行の頭に「・」を付ける。",
    "- 記事の続きに書かれた事実だけを使う。書かれていない数値・日付・名前・出来事を足さない。",
    "- 誰が何をしたのか（主語）が記事の続きに書かれている事実だけを書く。主語が省かれていて誰（何）のことか分からない事実は書かない。主語を推測で補わない。",
    "- 推測・予定・意見として書かれたことは、推測・予定・意見のまま書く。",
    "- 本文の文をそのまま写さない。",
    "- 出力は2〜3行だけ。見出し・説明は付けない。",
];

/// `paras` の指示（固定文）。
const PARA_INSTRUCTIONS: [&str; 7] = [
    "次の「段落」は、ニュース記事の後半の1つの段落です。日本語の1行で短くまとめてください。",
    "- 40〜60文字くらいの1文にする。",
    "- 段落に書かれた事実だけを使う。段落にない数値・日付・名前・出来事を足さない。",
    "- 誰が何をしたのか（主語）は段落に書かれたとおりに書く。段落に主語が書かれておらず、誰（何）のことか段落だけでは分からないときは、まとめずに「なし」とだけ答える。主語を推測で補わない。",
    "- 推測・予定・意見として書かれたことは、推測・予定・意見のまま書く。",
    "- 段落の文をそのまま写さず、いちばん大事な点だけを書く。",
    "- 出力はまとめの1行だけ。見出し・記号・説明は付けない。",
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Block,
    Paras,
}

/// 後ろの本文: 先頭 `HEAD_CHARS` 字をまたぐ段落は、後ろ側が半分以上ならその段落から含める（読み落としを避ける）。
/// 返すのは（後ろの本文の全部, 本文の中での開始位置）。短すぎるときは None。
/// 開始位置は trim で落とした先頭の空白の分だけ進め、返す本文の先頭と揃える（記録の `position` がずれないように）。
fn tail_text(body: &str) -> Option<(String, usize)> {
    let mut offset = 0;
    let mut start = None;
    for block in body.split("\n\n") {
        let length = block.chars().count();
        let end = offset + length;
        if end > HEAD_CHARS {
            let after = end - HEAD_CHARS;
            let before = HEAD_CHARS.saturating_sub(offset);
            start = Some(if after >= before { offset } else { end + 2 });
            break;
        }
        offset = end + 2;
    }
    let start = start?;
    let tail: String = body.chars().skip(start).collect();
    let leading = tail.chars().take_while(|c| c.is_whitespace()).count();
    let tail = tail.trim().to_string();
    (tail.chars().count() >= TAIL_MIN_CHARS).then_some((tail, start + leading))
}

/// `block` で渡す分: 段落の切れ目で `TAIL_BLOCK_MAX_CHARS` 字まで。最初の段落だけで超えるときは「。」で切る。
fn block_input(tail: &str) -> String {
    let mut out = String::new();
    for block in tail.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        let next = if out.is_empty() {
            block.to_string()
        } else {
            format!("{out}\n\n{block}")
        };
        if next.chars().count() > TAIL_BLOCK_MAX_CHARS {
            if out.is_empty() {
                let head: String = block.chars().take(TAIL_BLOCK_MAX_CHARS).collect();
                out = match head.rfind('。') {
                    Some(index) => head[..index + '。'.len_utf8()].to_string(),
                    None => head,
                };
            }
            break;
        }
        out = next;
    }
    out
}

/// `paras` で選ぶかたまりの番号（均等に `MAX_LINES` 個まで。最初と最後を含む）。
fn spread_indices(count: usize) -> Vec<usize> {
    if count <= MAX_LINES {
        return (0..count).collect();
    }
    let mut indices: Vec<usize> = (0..MAX_LINES)
        .map(|i| (i * (count - 1) + (MAX_LINES - 1) / 2) / (MAX_LINES - 1))
        .collect();
    indices.dedup();
    indices
}

/// 生出力の行を取り出す（空行を飛ばし、箇条書きの記号を外す）。最大 `MAX_LINES` 行。
fn block_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(clean_line)
        .filter(|line| !line.is_empty())
        .take(MAX_LINES)
        .collect()
}

/// 「なし」の答え（主語が分からないので行を作らない）か。
fn is_none_answer(line: &str) -> bool {
    let trimmed = line.trim_matches(|c: char| c.is_whitespace() || "。．.「」『』".contains(c));
    trimmed == "なし" || trimmed.starts_with("なし（") || trimmed.starts_with("なし(")
}

/// 行の英字の語（英字で始まる 2 字以上）で、材料にないもの。§14.2 で誤りの行をやや選べた点検だけを使う。
fn unknown_latin_names(source: &str, line: &str) -> Vec<String> {
    let source = normalize_name(source);
    names_in(line)
        .into_iter()
        .filter(|name| name.starts_with(|c: char| c.is_ascii_alphabetic()))
        .filter(|name| !source.contains(&normalize_name(name)))
        .collect()
}

/// 1行の点検。落とす理由のラベル（重複なし）を返す。空なら残す。
/// 製品の要約の点検のうち作り直しの理由にする種類と、英字の語の点検だけ（長さ・カタカナ語では落とさない）。
fn check_line(source: &str, line: &str, instructions: &[&str]) -> Vec<&'static str> {
    let report = check_japanese_summary(source, line, None, instructions);
    let mut labels: Vec<&'static str> = report
        .issues
        .iter()
        .filter(|issue| issue.kind.triggers_summary_retry())
        .map(|issue| issue.kind.label())
        .collect();
    if !unknown_latin_names(source, line).is_empty() {
        labels.push("unknown_latin_name");
    }
    labels.sort_unstable();
    labels.dedup();
    labels
}

fn block_prompt(input: &str) -> String {
    format!(
        "{}\n\n記事の続き:\n{}",
        BLOCK_INSTRUCTIONS.join("\n"),
        neutralize_seed_text(input, TAIL_BLOCK_MAX_CHARS)
    )
}

fn para_prompt(chunk: &str) -> String {
    format!(
        "{}\n\n段落:\n{}",
        PARA_INSTRUCTIONS.join("\n"),
        neutralize_seed_text(chunk, CHUNK_MAX_CHARS)
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FullText {
    article_id: String,
    body: String,
}

#[test]
#[ignore = "manual: requires the real local LLM bundle and YUUKO_LLM_EVAL_OUT"]
fn tail_lines_eval() {
    let Some(out_dir) = std::env::var_os(OUT_ENV).map(PathBuf::from) else {
        eprintln!("{OUT_ENV} is not set; the tail-lines evaluation does nothing");
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
    let mode = match std::env::var(MODE_ENV).unwrap_or_default().as_str() {
        "block" => Mode::Block,
        "paras" => Mode::Paras,
        other => panic!("{MODE_ENV} must be block or paras (got {other:?})"),
    };
    let fulltext = std::env::var_os(FULLTEXT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{FULLTEXT_ENV} must point to fulltext.json"));
    let articles: Vec<FullText> =
        serde_json::from_str(&std::fs::read_to_string(fulltext).expect("read fulltext.json"))
            .expect("parse fulltext.json");
    let jsonl = run_dir.join("tail_lines.jsonl");
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
        let record = tail_article(&local, article, mode);
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

fn line_record(source: &str, line: &str, instructions: &[&str]) -> Value {
    let issues = check_line(source, line, instructions);
    json!({
        "line": line,
        "lineChars": line.chars().filter(|c| !c.is_whitespace()).count(),
        "copyRun": longest_common_run(source, line),
        "issues": issues,
        "dropped": !issues.is_empty(),
    })
}

fn tail_article(local: &LocalLlmService, article: &FullText, mode: Mode) -> Value {
    let body_chars = article.body.chars().count();
    let Some((tail, tail_start)) = tail_text(&article.body) else {
        return json!({
            "articleId": article.article_id,
            "bodyChars": body_chars,
            "tailChars": 0,
            "totalMs": 0,
            "calls": 0,
            "dropped": 0,
            "lines": [],
        });
    };
    let started = Instant::now();
    let mut lines = Vec::new();
    let mut calls = Vec::new();
    match mode {
        Mode::Block => {
            let input = block_input(&tail);
            let call_started = Instant::now();
            let generated = local.generate(&block_prompt(&input), BLOCK_MAX_TOKENS);
            let (raw, error) = match generated {
                Ok(text) => (text.trim().to_string(), None),
                Err(failure) => (String::new(), Some(format!("{failure:?}"))),
            };
            calls.push(json!({
                "inputChars": input.chars().count(),
                "ms": call_started.elapsed().as_millis(),
                "raw": raw,
                "error": error,
            }));
            // 点検の材料は渡した後ろの本文だけ（モデルが見た範囲）。
            for line in block_lines(&raw) {
                lines.push(line_record(&input, &line, &BLOCK_INSTRUCTIONS));
            }
        }
        Mode::Paras => {
            let chunks = split_chunks(&tail);
            for index in spread_indices(chunks.len()) {
                let chunk = &chunks[index];
                let call_started = Instant::now();
                let generated = local.generate(&para_prompt(&chunk.text), LINE_MAX_TOKENS);
                let (raw, error) = match generated {
                    Ok(text) => (text.trim().to_string(), None),
                    Err(failure) => (String::new(), Some(format!("{failure:?}"))),
                };
                let line = clean_line(&raw);
                let none = is_none_answer(&line);
                calls.push(json!({
                    "chunkIndex": index,
                    "chunks": chunks.len(),
                    "position": (tail_start + chunk.start_char) as f64 / body_chars.max(1) as f64,
                    "inputChars": chunk.text.chars().count(),
                    "ms": call_started.elapsed().as_millis(),
                    "raw": raw,
                    "noneAnswer": none,
                    "error": error,
                }));
                if !none && !line.is_empty() {
                    lines.push(line_record(&chunk.text, &line, &PARA_INSTRUCTIONS));
                }
            }
        }
    }
    let dropped = lines
        .iter()
        .filter(|line| line["dropped"] == json!(true))
        .count();
    json!({
        "articleId": article.article_id,
        "bodyChars": body_chars,
        "tailStart": tail_start,
        "tailChars": tail.chars().count(),
        "totalMs": started.elapsed().as_millis(),
        "calls": calls,
        "dropped": dropped,
        "lines": lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_starts_at_the_paragraph_mostly_after_the_head() {
        let first = "あ".repeat(1_400);
        // 1,402 字目から始まる 300 字の段落は、後ろ側（1,500 字より後ろ）が 202 字で半分以上なので含める。
        let second = "い".repeat(300);
        let third = "う".repeat(100);
        let body = format!("{first}\n\n{second}\n\n{third}");
        let (tail, start) = tail_text(&body).unwrap();
        assert_eq!(start, 1_402);
        assert!(tail.starts_with('い'));
        // 後ろ側が半分未満の段落は前の側（製品の要約が見る範囲）とみなし、次の段落から始める。
        let second = "い".repeat(150);
        let body = format!("{first}\n\n{second}\n\n{}", "う".repeat(300));
        let (tail, _) = tail_text(&body).unwrap();
        assert!(tail.starts_with('う'));
    }

    #[test]
    fn tail_start_skips_the_trimmed_leading_spaces() {
        // 段落の頭の全角空白は trim で落とすので、開始位置もその分だけ進める。
        let body = format!(
            "{}\n\n\u{3000}\u{3000}{}",
            "あ".repeat(1_400),
            "い".repeat(300)
        );
        let (tail, start) = tail_text(&body).unwrap();
        assert!(tail.starts_with('い'));
        assert_eq!(start, 1_404);
        assert_eq!(body.chars().nth(start), Some('い'));
    }

    #[test]
    fn short_bodies_and_short_tails_have_no_tail() {
        assert!(tail_text(&"あ".repeat(1_400)).is_none());
        let body = format!("{}\n\n{}", "あ".repeat(1_500), "い".repeat(100));
        assert!(tail_text(&body).is_none());
    }

    #[test]
    fn block_input_stops_at_paragraph_boundaries() {
        let tail = format!("{}\n\n{}", "あ".repeat(1_000), "い".repeat(800));
        assert_eq!(block_input(&tail), "あ".repeat(1_000));
        let long = format!("{}。{}", "あ".repeat(900), "い".repeat(900));
        assert_eq!(block_input(&long), format!("{}。", "あ".repeat(900)));
    }

    #[test]
    fn spread_indices_include_first_and_last() {
        assert_eq!(spread_indices(2), vec![0, 1]);
        assert_eq!(spread_indices(3), vec![0, 1, 2]);
        assert_eq!(spread_indices(7), vec![0, 3, 6]);
        assert_eq!(spread_indices(4), vec![0, 2, 3]);
    }

    #[test]
    fn block_lines_strip_bullets_and_keep_three() {
        let raw = "・一つ目の行です。\n\n・二つ目の行です。\n- 三つ目の行です。\n・四つ目";
        assert_eq!(
            block_lines(raw),
            vec!["一つ目の行です。", "二つ目の行です。", "三つ目の行です。"]
        );
    }

    #[test]
    fn none_answers_are_detected() {
        assert!(is_none_answer("なし"));
        assert!(is_none_answer("「なし」。"));
        assert!(!is_none_answer("なしくずしに決まった。"));
    }

    #[test]
    fn only_latin_names_missing_from_the_source_are_flagged() {
        let source = "OpenAI はデータを公開した。";
        assert!(unknown_latin_names(source, "OpenAI がシステムを公開").is_empty());
        assert_eq!(
            unknown_latin_names(source, "Google が公開した"),
            vec!["Google"]
        );
        assert!(check_line(source, "OpenAI がシステムを公開した。", &PARA_INSTRUCTIONS).is_empty());
    }
}
