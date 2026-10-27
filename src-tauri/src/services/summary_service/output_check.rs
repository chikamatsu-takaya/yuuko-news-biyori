//! 要約の機械点検（判断台帳 D166。D165 の試作 `overseas_output_check` を国内記事の要約にも使える形にしたもの）。
//!
//! 小さいローカルLLM（Qwen3.5-2B）の要約は、プロンプトで禁じても「材料にない数字・桁の取り違え」
//! 「中国語の簡体字の混入」「指示の写し」を出すことがある。人が読む前に材料（AI へ渡した記事の抜粋）と
//! 突き合わせて機械的に引っかけ、summary_service が理由を添えて1回だけ作り直すための点検をここに置く。
//! すべて純粋関数（通信・ファイル操作なし）。
//!
//! 点検の種類（`IssueKind`）:
//! - **数字**: 要約の算用数字（全角も可。「万・億・兆・千」やカンマを読み、掛けた値にする）が、材料の数字の
//!   どれかと一致するか。一致せず 10 のべき乗倍だけずれていれば「桁違い」、それ以外は「材料にない数字」。
//!   材料の側は、算用数字（単位を掛けた値・掛けない値）に加えて漢数字（「三つ」「百年」「千八百円」）・
//!   元号の年（令和8年 → 2026）・「割」（3割 → 30）・時刻の12時間表記（午後3時 ⇔ 15時）も読む。
//!   **要約の側の漢数字は照合しない**: 「一部」「一方」「一時」「十分」「一人ひとり」のように数ではない
//!   言い回しが多く、読むと誤検出になるため（漢数字で書いた数字の誤りは見逃す）。
//! - **簡体字**: 日本語の文字コードに無い簡体字（`util::speech_cleanup` の一覧）と、中国語にしかない
//!   2字の並び（「及其」「相干」など）。材料にもある字・並びは数えない。
//! - **指示の写し**: プロンプトの見出し・語（「記事情報」「###」など）と、固定指示の文の写し
//!   （`strip_prompt_echo_sentences`）。材料にもある語は数えない。
//! - **注記**: 記事の内容ではなく材料についての注記（「記事情報には」「明記されていない」など）。
//! - **長さ**: 空白を除いた文字数の上限（上限を渡したときだけ）。空の出力。
//! - **日本語でない**: かなの割合が 10% 未満（英語のまま返した出力）。
//!
//! どの種類で作り直すかは呼び出し側が決める（`IssueKind::triggers_summary_retry`）。国内記事の評価
//! （`docs/00_project/要約の機械点検の評価_2026-10.md`）で誤検出の多かった種類は、作り直しの理由にしない。

use crate::util::speech_cleanup::{simplified_only_hanzi_not_in, strip_prompt_echo_sentences};

/// 点検で見つかった問題の1件。`reason` は作り直しのプロンプトに添える日本語の説明。
/// 理由に入る出力由来の文字は、数字・単位・簡体字の字・固定の語だけ（記事の文はそのまま入れない）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CheckIssue {
    pub kind: IssueKind,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum IssueKind {
    /// 材料のどの数字とも合わない数字。
    UnknownNumber,
    /// 材料の数字と 10 のべき乗倍ずれた数字（単位の換算の誤り）。
    ScaleMismatch,
    SimplifiedChinese,
    PromptEcho,
    /// 「記事情報には数字が書かれていない」のような、記事の内容ではなく材料についての注記。
    MetaComment,
    TooLong,
    Empty,
    NotJapanese,
}

impl IssueKind {
    /// ログ・評価記録に出す固定ラベル（本文は出さない）。
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::UnknownNumber => "unknown_number",
            Self::ScaleMismatch => "scale_mismatch",
            Self::SimplifiedChinese => "simplified_chinese",
            Self::PromptEcho => "prompt_echo",
            Self::MetaComment => "meta_comment",
            Self::TooLong => "too_long",
            Self::Empty => "empty",
            Self::NotJapanese => "not_japanese",
        }
    }

    /// 国内記事の要約で、作り直しの理由にする種類か（判断台帳 D166 の評価で決めた）。
    pub(super) fn triggers_summary_retry(self) -> bool {
        match self {
            Self::UnknownNumber
            | Self::ScaleMismatch
            | Self::SimplifiedChinese
            | Self::PromptEcho
            | Self::NotJapanese => true,
            // 注記: 国内記事では「〜は明らかにされていない」のような記事そのものの言い回しと区別しにくい。
            // 長さ: 要約は「1〜2文」で頼んでおり文字数の上限を決めていない（上限は保存前の検証 3000 字）。
            // 空: 保存前の検証（validate_summary_output）が先に拒否するので、ここには来ない。
            Self::MetaComment | Self::TooLong | Self::Empty => false,
        }
    }
}

/// 点検の結果。`issues` が空なら合格。`source_number_kept` は材料の算用数字のうち要約に残った割合（参考値）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CheckReport {
    pub issues: Vec<CheckIssue>,
    pub source_number_kept: Option<f64>,
}

impl CheckReport {
    #[cfg(test)]
    pub(super) fn passed(&self) -> bool {
        self.issues.is_empty()
    }

    /// `filter` を満たす問題の種類（重複なし・並びは固定）。ログ・作り直しの判断に使う。
    pub(super) fn kinds_where(&self, filter: impl Fn(IssueKind) -> bool) -> Vec<IssueKind> {
        let mut kinds: Vec<IssueKind> = self
            .issues
            .iter()
            .map(|issue| issue.kind)
            .filter(|kind| filter(*kind))
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    }
}

/// 材料から読んだ数字。`allowed` は照合で許す値の一覧、`listed` は材料の算用数字1つごとの値の候補
/// （要約に残った割合を出すため）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SourceNumbers {
    pub allowed: Vec<f64>,
    pub listed: Vec<Vec<f64>>,
}

/// 点検の設定。言語（国内・海外）ごとに、写し・注記の語や桁違いの理由の添え書きを変える。
pub(super) struct CheckOptions<'a> {
    /// AI へ渡した材料（記事の抜粋など）。材料にもある字・語は、簡体字・写し・注記として数えない。
    pub input_text: &'a str,
    /// 空白を除いた文字数の上限。`None` なら長さを点検しない。
    pub max_chars: Option<usize>,
    pub echo_markers: &'a [&'a str],
    pub meta_markers: &'a [&'a str],
    /// プロンプトの固定指示（記事を含まない部分）。文の写しの検出に使う。
    pub fixed_prompt_texts: &'a [&'a str],
    /// 桁違いの理由の末尾に添える説明（例: 「単位は材料のとおりに書く。」）。
    pub scale_hint: &'a str,
}

/// 国内記事の要約のプロンプトに出る見出し・語。要約の本文に出るのは写しのときだけ（材料にある語は除く）。
/// 「事実だけを使」は固定指示（`FACTS_ONLY_RULE`）の言い回しで、評価では「…という事実だけを使いました。」
/// として要約の末尾に混ざった（文全体は指示文と一致しないので、文の写しの照合では見つからない）。
pub(super) const JAPANESE_SUMMARY_ECHO_MARKERS: [&str; 8] = [
    "記事情報",
    "事実だけを使",
    "外部データ",
    "厳守事項",
    "###",
    "命令として解釈",
    "要約してください",
    "1〜2文",
];

/// 国内記事の要約で、材料そのものについての注記の言い回し（評価の記録用。作り直しの理由にはしない）。
/// 「記事情報に」は指示の写し（「記事情報」）として数えるので、ここには入れない。
pub(super) const JAPANESE_SUMMARY_META_MARKERS: [&str; 5] = [
    "記事には",
    "提供された",
    "明記されていな",
    "記載されていな",
    "言及されていな",
];

/// 国内記事（日本語の材料）の要約を点検する。
///
/// - `source`: AI へ渡した材料（要約の入力そのもの）。数字の照合と、字・語の除外の元になる。
/// - `fixed_prompt_texts`: プロンプトの固定指示（記事を含まない部分）。
pub(super) fn check_japanese_summary(
    source: &str,
    output: &str,
    max_chars: Option<usize>,
    fixed_prompt_texts: &[&str],
) -> CheckReport {
    check_output(
        &japanese_source_numbers(source),
        output,
        &CheckOptions {
            input_text: source,
            max_chars,
            echo_markers: &JAPANESE_SUMMARY_ECHO_MARKERS,
            meta_markers: &JAPANESE_SUMMARY_META_MARKERS,
            fixed_prompt_texts,
            scale_hint: "単位（万・億など）は記事情報のとおりに書く。",
        },
    )
}

/// 点検の本体（言語に依らない部分）。数字の照合は、材料から読んだ数字（`source_numbers`）と比べる。
pub(super) fn check_output(
    source_numbers: &SourceNumbers,
    output: &str,
    options: &CheckOptions<'_>,
) -> CheckReport {
    let mut issues = Vec::new();
    let body_chars = output.chars().filter(|c| !c.is_whitespace()).count();
    if body_chars == 0 {
        issues.push(issue(IssueKind::Empty, "要約が空だった。".to_string()));
        return CheckReport {
            issues,
            source_number_kept: None,
        };
    }
    let (number_issues, kept) = check_numbers(source_numbers, output, options.scale_hint);
    issues.extend(number_issues);
    issues.extend(check_simplified_chinese(output, options.input_text));
    issues.extend(check_prompt_echo(
        output,
        options.input_text,
        options.echo_markers,
        options.fixed_prompt_texts,
    ));
    issues.extend(check_meta_comment(
        output,
        options.input_text,
        options.meta_markers,
    ));
    if let Some(max_chars) = options.max_chars {
        if body_chars > max_chars {
            issues.push(issue(
                IssueKind::TooLong,
                format!("要約が{body_chars}文字で、上限の{max_chars}文字を超えていた。"),
            ));
        }
    }
    if !looks_japanese(output) {
        issues.push(issue(
            IssueKind::NotJapanese,
            "要約が日本語になっていなかった。".to_string(),
        ));
    }
    CheckReport {
        issues,
        source_number_kept: kept,
    }
}

fn issue(kind: IssueKind, reason: String) -> CheckIssue {
    CheckIssue { kind, reason }
}

// ---------------------------------------------------------------------------
// 数字
// ---------------------------------------------------------------------------

/// 文中で見つけた1つの数字。`values` はその数字が表しうる値の候補（先頭が主な値）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FoundNumber {
    pub text: String,
    pub values: Vec<f64>,
}

/// 日本語の数字の単位（「2500万」「10億」「1.5兆」「67百万」「3千」）。長いものから照合する。
const JAPANESE_MULTIPLIERS: [(&str, f64); 7] = [
    ("百万", 1_000_000.0),
    ("千万", 10_000_000.0),
    ("十億", 1_000_000_000.0),
    ("万", 10_000.0),
    ("億", 100_000_000.0),
    ("兆", 1_000_000_000_000.0),
    ("千", 1_000.0),
];

/// 英語の数字の後ろに付く倍率（"2.5M"・"3 billion"・"10K"）。要約に英語の倍率が残ったときにも読む。
pub(super) fn english_multiplier(word: &str) -> Option<f64> {
    match word {
        "k" | "thousand" => Some(1_000.0),
        "m" | "mn" | "million" | "millions" => Some(1_000_000.0),
        "b" | "bn" | "billion" | "billions" => Some(1_000_000_000.0),
        "t" | "trillion" => Some(1_000_000_000_000.0),
        _ => None,
    }
}

/// 元号と元年の前の年（令和N年 = 2018 + N）。材料・要約の「令和8年」と「2026年」を同じ年として扱う。
const JAPANESE_ERAS: [(&str, f64); 3] = [("令和", 2018.0), ("平成", 1988.0), ("昭和", 1925.0)];

/// 日本語の文の算用数字（全角も可）を取り出す。直後の「万・億・兆・千」や M・billion を掛けた値を主な値にする。
/// 「1億2000万」「1兆2,300億」「1万8000」のような続きの数は、足し合わせた値にする。
/// 値の候補には、同じ数の別の書き方（3割 ⇔ 30%、午後3時 ⇔ 15時、令和8年 ⇔ 2026年）も加える。
pub(super) fn japanese_numbers(text: &str) -> Vec<FoundNumber> {
    let chars: Vec<char> = text.chars().map(to_halfwidth).collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let Some((value, end)) = parse_number_at(&chars, index) else {
            index += 1;
            continue;
        };
        let (total, cursor) = read_japanese_units(&chars, value, end);
        let mut values = vec![total];
        add_alternative_readings(&chars, index, cursor, total, &mut values);
        found.push(FoundNumber {
            text: chars[index..cursor].iter().collect(),
            values,
        });
        index = cursor.max(end);
    }
    found
}

/// `chars[end..]` の単位（日本語の「万・億」など、なければ英語の倍率）を読み、掛けた値と読み終えた位置を返す。
fn read_japanese_units(chars: &[char], value: f64, end: usize) -> (f64, usize) {
    // 小さいモデルは「250 万ドル」のように数字と単位の間に空白を入れることがある（D165 の1回目で誤検出した）。
    let unit_start = skip_spaces(chars, end);
    let Some((unit, multiplier)) = japanese_unit_at(chars, unit_start) else {
        let word_start = skip_spaces(chars, end);
        let word: String = chars[word_start..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        return match english_multiplier(&word) {
            Some(multiplier) => (value * multiplier, word_start + word.len()),
            None => (value, end),
        };
    };
    let mut total = value * multiplier;
    let mut cursor = unit_start + unit.chars().count();
    let mut last_multiplier = multiplier;
    // 続きの数（「1億2000万」「1兆2,300億」「1万8000」）を足す。単位は前より小さいものだけ。
    // 小さいモデルは「80 万 4,300 人」「2 億 1000 万円」のように単位の前後に空白を入れるので、空白は読み飛ばす
    // （2026-10-27 の国内記事の評価で、読み飛ばさずに桁違いと誤検出した）。
    loop {
        let next_start = skip_spaces(chars, cursor);
        let Some((next_value, next_end)) = parse_number_at(chars, next_start) else {
            break;
        };
        let next_unit_start = skip_spaces(chars, next_end);
        match japanese_unit_at(chars, next_unit_start) {
            Some((next_unit, next_multiplier)) if next_multiplier < last_multiplier => {
                total += next_value * next_multiplier;
                cursor = next_unit_start + next_unit.chars().count();
                last_multiplier = next_multiplier;
            }
            // 「1万8000人」: 万の後ろの単位のない数（1万未満）は、そのまま足す。
            // ただし「80 万 2026 年」のような日付・時刻の数は続きにしない。
            None if last_multiplier >= 10_000.0
                && next_value < 10_000.0
                && !matches!(
                    chars.get(next_unit_start),
                    Some('年' | '月' | '日' | '時' | '分' | '秒' | '回' | '位')
                ) =>
            {
                total += next_value;
                cursor = next_end;
                break;
            }
            _ => break,
        }
    }
    (total, cursor)
}

fn japanese_unit_at(chars: &[char], start: usize) -> Option<(&'static str, f64)> {
    let rest: String = chars.get(start..)?.iter().take(2).collect();
    JAPANESE_MULTIPLIERS
        .iter()
        .find(|(unit, _)| rest.starts_with(unit))
        .copied()
}

fn skip_spaces(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && chars[index] == ' ' {
        index += 1;
    }
    index
}

/// 同じ数の別の書き方を値の候補に足す（材料と要約で書き方が変わっても同じ数として照合するため）。
fn add_alternative_readings(
    chars: &[char],
    start: usize,
    end: usize,
    total: f64,
    values: &mut Vec<f64>,
) {
    let after = chars.get(end).copied();
    // 「3割」は 30%。
    if after == Some('割') {
        values.push(total * 10.0);
    }
    // 時刻の12時間表記: 「午後3時」⇔「15時」。
    if after == Some('時') && (0.0..=24.0).contains(&total) && total.fract() == 0.0 {
        if total > 12.0 {
            values.push(total - 12.0);
        } else {
            values.push(total + 12.0);
        }
    }
    // 元号の年: 「令和8年」⇔「2026年」。
    let before: String = chars[start.saturating_sub(2)..start].iter().collect();
    if after == Some('年') {
        if let Some((_, base)) = JAPANESE_ERAS.iter().find(|(era, _)| before == *era) {
            values.push(base + total);
        }
    }
}

/// 材料（日本語）から照合で許す数字を読む。算用数字は単位を掛けた値と掛けない値の両方を許し、
/// 漢数字も読む（材料の「三つ」を要約で「3つ」と書くのは正しいため）。
pub(super) fn japanese_source_numbers(source: &str) -> SourceNumbers {
    let chars: Vec<char> = source.chars().map(to_halfwidth).collect();
    let found = japanese_numbers(source);
    let mut allowed: Vec<f64> = found
        .iter()
        .flat_map(|number| number.values.iter().copied())
        .collect();
    // 単位を掛けない値（「250万円」の 250）も許す（要約で「250万」を「2.5百万」などと崩さない限り使わないが、
    // 「250万円のうち…」の言い換えで単位を落とした数を桁違いと誤検出しないため）。
    let mut index = 0;
    while index < chars.len() {
        if let Some((value, end)) = parse_number_at(&chars, index) {
            allowed.push(value);
            index = end;
        } else {
            index += 1;
        }
    }
    allowed.extend(kanji_numbers(source));
    SourceNumbers {
        allowed,
        listed: found.into_iter().map(|number| number.values).collect(),
    }
}

/// 漢数字の数字（「三」「十二」「百」「千八百」「二〇二六」「三万」）を読む。材料の側だけで使う。
/// 「一部」「一方」のような言い回しの「一」も 1 として読むが、許す値が増えるだけなので害はない。
fn kanji_numbers(text: &str) -> Vec<f64> {
    let mut values = Vec::new();
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if kanji_digit(c).is_some()
            || kanji_small_unit(c).is_some()
            || kanji_large_unit(c).is_some()
        {
            run.push(c);
        } else if !run.is_empty() {
            if let Some(value) = parse_kanji_run(&run) {
                values.push(value);
            }
            run.clear();
        }
    }
    values
}

fn kanji_digit(c: char) -> Option<f64> {
    "〇一二三四五六七八九"
        .chars()
        .position(|digit| digit == c)
        .map(|position| position as f64)
        .or((c == '零').then_some(0.0))
}

fn kanji_small_unit(c: char) -> Option<f64> {
    match c {
        '十' => Some(10.0),
        '百' => Some(100.0),
        '千' => Some(1_000.0),
        _ => None,
    }
}

fn kanji_large_unit(c: char) -> Option<f64> {
    match c {
        '万' => Some(10_000.0),
        '億' => Some(100_000_000.0),
        '兆' => Some(1_000_000_000_000.0),
        _ => None,
    }
}

/// 漢数字の並びを値にする。位取りの字（十・百・千）がなく数字だけが並ぶとき（「二〇二六」）は桁として読む。
/// 単位だけ（「万」「億」）の並びは数にしない（「万一」「億劫」などの語の一部のため）。
fn parse_kanji_run(run: &str) -> Option<f64> {
    if !run
        .chars()
        .any(|c| kanji_digit(c).is_some() || kanji_small_unit(c).is_some())
    {
        return None;
    }
    if run.chars().all(|c| kanji_digit(c).is_some()) {
        return run
            .chars()
            .try_fold(0.0, |acc, c| kanji_digit(c).map(|digit| acc * 10.0 + digit));
    }
    let mut total = 0.0;
    let mut section = 0.0;
    let mut current: Option<f64> = None;
    for c in run.chars() {
        if let Some(digit) = kanji_digit(c) {
            current = Some(current.unwrap_or(0.0) * 10.0 + digit);
        } else if let Some(unit) = kanji_small_unit(c) {
            section += current.take().unwrap_or(1.0) * unit;
        } else if let Some(unit) = kanji_large_unit(c) {
            total += (section + current.take().unwrap_or(0.0)).max(1.0) * unit;
            section = 0.0;
        }
    }
    Some(total + section + current.unwrap_or(0.0))
}

/// `chars[start..]` が数字で始まるとき、その値と終わりの位置を返す（3桁区切りのカンマ・小数点を含める）。
/// 英字の直後の数字（"GPT4" の 4 など）も数字として扱う（名前の中の数字は材料にも同じ形で出るため照合で通る）。
/// `chars` は `to_halfwidth` で半角にしたもの。
pub(super) fn parse_number_at(chars: &[char], start: usize) -> Option<(f64, usize)> {
    if !chars.get(start)?.is_ascii_digit() {
        return None;
    }
    // 数字の途中から読み始めない。
    if start > 0 && (chars[start - 1].is_ascii_digit() || is_decimal_point_before(chars, start)) {
        return None;
    }
    let mut end = start;
    let mut digits = String::new();
    while end < chars.len() {
        let c = chars[end];
        if c.is_ascii_digit() {
            digits.push(c);
            end += 1;
        } else if c == ','
            && chars.get(end + 1).is_some_and(char::is_ascii_digit)
            && chars.get(end + 2).is_some_and(char::is_ascii_digit)
            && chars.get(end + 3).is_some_and(char::is_ascii_digit)
            && !chars.get(end + 4).is_some_and(char::is_ascii_digit)
        {
            end += 1;
        } else if c == '.'
            && !digits.contains('.')
            && chars.get(end + 1).is_some_and(char::is_ascii_digit)
        {
            digits.push('.');
            end += 1;
        } else {
            break;
        }
    }
    digits.parse().ok().map(|value| (value, end))
}

fn is_decimal_point_before(chars: &[char], start: usize) -> bool {
    start >= 2 && chars[start - 1] == '.' && chars[start - 2].is_ascii_digit()
}

/// 全角の数字・小数点・カンマ・空白を半角にする（文字の数は変えない）。
pub(super) fn to_halfwidth(c: char) -> char {
    match c {
        '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
        '．' => '.',
        '，' => ',',
        '　' => ' ',
        _ => c,
    }
}

pub(super) fn contains_value(values: &[f64], target: f64) -> bool {
    values.iter().any(|value| same_value(*value, target))
}

fn same_value(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// `a` が `b` の 10 のべき乗倍（10〜10^9 倍、または 1/10〜1/10^9）か。
fn differs_by_power_of_ten(a: f64, b: f64) -> bool {
    if a == 0.0 || b == 0.0 {
        return false;
    }
    let ratio = (a / b).abs().log10();
    let rounded = ratio.round();
    rounded != 0.0 && rounded.abs() <= 9.0 && (ratio - rounded).abs() < 1e-9
}

/// 要約の数字を材料と照合する。戻り値の2つ目は、材料の算用数字のうち要約に残った割合（材料に数字がなければ None）。
fn check_numbers(
    source: &SourceNumbers,
    output: &str,
    scale_hint: &str,
) -> (Vec<CheckIssue>, Option<f64>) {
    let output_numbers = japanese_numbers(output);
    let mut issues = Vec::new();
    for found in &output_numbers {
        if found
            .values
            .iter()
            .any(|value| contains_value(&source.allowed, *value))
        {
            continue;
        }
        let value = found.values[0];
        if let Some(near) = source
            .allowed
            .iter()
            .find(|candidate| differs_by_power_of_ten(value, **candidate))
        {
            issues.push(issue(
                IssueKind::ScaleMismatch,
                format!(
                    "「{}」は記事情報の数字（{}）と桁が合わなかった。{scale_hint}",
                    found.text,
                    format_value(*near)
                ),
            ));
        } else {
            issues.push(issue(
                IssueKind::UnknownNumber,
                format!(
                    "「{}」は記事情報にない数字だった。記事情報にない数字は書かない。",
                    found.text
                ),
            ));
        }
    }
    let kept = (!source.listed.is_empty()).then(|| {
        let output_values: Vec<f64> = output_numbers
            .iter()
            .flat_map(|found| found.values.iter().copied())
            .collect();
        let kept = source
            .listed
            .iter()
            .filter(|values| values.iter().any(|v| contains_value(&output_values, *v)))
            .count();
        kept as f64 / source.listed.len() as f64
    });
    (issues, kept)
}

fn format_value(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

// ---------------------------------------------------------------------------
// 簡体字
// ---------------------------------------------------------------------------

/// 中国語にしかない2字の並び（字だけでは日本語と区別できないもの）。D164 の「及其」、D165 の「相干」「良率」
/// 「自愿」（字は日本語にもあるが、語としては中国語。日本語は「コヒーレント」「歩留まり」「希望」）を含む。
const SIMPLIFIED_ONLY_BIGRAMS: [&str; 8] = [
    "及其", "以及", "我们", "他们", "一个", "相干", "良率", "自愿",
];

fn check_simplified_chinese(output: &str, input: &str) -> Vec<CheckIssue> {
    let mut hits: Vec<String> = simplified_only_hanzi_not_in(output, input)
        .into_iter()
        .map(String::from)
        .collect();
    hits.extend(
        SIMPLIFIED_ONLY_BIGRAMS
            .iter()
            .filter(|bigram| output.contains(**bigram) && !input.contains(**bigram))
            .map(|bigram| bigram.to_string()),
    );
    if hits.is_empty() {
        return Vec::new();
    }
    vec![issue(
        IssueKind::SimplifiedChinese,
        format!(
            "中国語の字（{}）が混ざっていた。日本語の字だけで書く。",
            hits.join("・")
        ),
    )]
}

// ---------------------------------------------------------------------------
// 指示の写し・注記・日本語らしさ
// ---------------------------------------------------------------------------

fn check_prompt_echo(
    output: &str,
    input: &str,
    markers: &[&str],
    fixed_prompt_texts: &[&str],
) -> Vec<CheckIssue> {
    let mut hits: Vec<&str> = markers
        .iter()
        .copied()
        .filter(|marker| output.contains(marker) && !input.contains(marker))
        .collect();
    let (_, removed) = strip_prompt_echo_sentences(output, fixed_prompt_texts);
    if removed > 0 {
        hits.push("指示の文");
    }
    if hits.is_empty() {
        return Vec::new();
    }
    vec![issue(
        IssueKind::PromptEcho,
        format!(
            "指示の写し（{}）が混ざっていた。要約の本文だけを書く。",
            hits.join("・")
        ),
    )]
}

fn check_meta_comment(output: &str, input: &str, markers: &[&str]) -> Vec<CheckIssue> {
    let hits: Vec<&str> = markers
        .iter()
        .copied()
        .filter(|marker| output.contains(marker) && !input.contains(marker))
        .collect();
    if hits.is_empty() {
        return Vec::new();
    }
    vec![issue(
        IssueKind::MetaComment,
        format!(
            "記事の内容ではない注記（{}）が混ざっていた。書かれていないことには触れず、記事の内容だけを書く。",
            hits.join("・")
        ),
    )]
}

/// かなが本文（空白・記号・数字を除く）の 10% 以上あれば日本語とみなす（固有名詞の英字が多い要約でも通る下限）。
fn looks_japanese(output: &str) -> bool {
    let letters: Vec<char> = output
        .chars()
        .filter(|c| c.is_alphabetic() || is_kana(*c) || is_cjk(*c))
        .collect();
    if letters.is_empty() {
        return false;
    }
    let kana = letters.iter().filter(|c| is_kana(**c)).count();
    kana * 100 >= letters.len() * 10
}

fn is_kana(c: char) -> bool {
    ('\u{3041}'..='\u{30ff}').contains(&c)
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(report: &CheckReport) -> Vec<IssueKind> {
        report.issues.iter().map(|issue| issue.kind).collect()
    }

    fn check(source: &str, output: &str) -> CheckReport {
        check_japanese_summary(source, output, None, &[])
    }

    #[test]
    fn japanese_units_commas_and_fullwidth_digits_match() {
        let source =
            "対象は約1万8000人で、負担は年間約12億円。月額4,800円。売上高は３，４２０億円。";
        let report = check(
            source,
            "対象は約18,000人、負担は12億円で、月額4800円。売上高は3420億円。",
        );
        assert!(report.passed(), "{report:?}");
        assert_eq!(report.source_number_kept, Some(1.0));
    }

    #[test]
    fn compound_large_units_are_summed() {
        let values = |text: &str| -> Vec<f64> {
            japanese_numbers(text)
                .into_iter()
                .map(|found| found.values[0])
                .collect()
        };
        assert_eq!(values("1兆2,300億円"), vec![1.23e12]);
        assert_eq!(values("2兆800億円"), vec![2.08e12]);
        assert_eq!(values("33万2,150人"), vec![332_150.0]);
        assert_eq!(values("1億2000万人"), vec![1.2e8]);
        assert_eq!(values("3千人"), vec![3000.0]);
        assert_eq!(values("6700万人と250 万ドル"), vec![6.7e7, 2.5e6]);
        // 単位の前より大きい単位は続きにしない（「5万 3億」は別の数）。
        assert_eq!(values("5万と3億"), vec![5e4, 3e8]);
        // 単位の前後の空白（2026-10-27 の評価で誤検出した形）。
        assert_eq!(values("80 万 4,300 人"), vec![804_300.0]);
        assert_eq!(values("総額 2 億 1000 万円"), vec![2.1e8]);
        assert_eq!(
            values("1 兆 2,300 億円と 2 兆 800 億円"),
            vec![1.23e12, 2.08e12]
        );
        // 日付の数は続きにしない。
        assert_eq!(values("80 万人、2026 年"), vec![8e5, 2026.0]);
        assert_eq!(values("約80万 2026年に"), vec![8e5, 2026.0]);
    }

    #[test]
    fn scale_mismatch_and_unknown_numbers_are_reported() {
        let source = "市の負担は年間約12億円になる見込み。対象は28校。";
        let scale = check(source, "市の負担は年間約120億円になる。");
        assert_eq!(kinds(&scale), vec![IssueKind::ScaleMismatch]);
        assert!(scale.issues[0].reason.contains("120億"));
        let unknown = check(source, "市は35校で実施し、負担は12億円。");
        assert_eq!(kinds(&unknown), vec![IssueKind::UnknownNumber]);
        assert!(unknown.issues[0].reason.contains("35"));
    }

    #[test]
    fn kanji_numerals_in_the_source_are_read_but_not_in_the_output() {
        let source = "創業百年を迎え、三代目の店主が一本千八百円で十一月末まで売る。";
        let report = check(
            source,
            "創業100年の店が、3代目の店主の羊羹を1本1,800円で11月末まで売る。",
        );
        assert!(report.passed(), "{report:?}");
        // 要約の漢数字（「一部」「十分」）は照合しない。
        let idioms = check("店は改装する。", "店は一部を改装し、十分な広さにする。");
        assert!(idioms.passed(), "{idioms:?}");
        assert_eq!(parse_kanji_run("二〇二六"), Some(2026.0));
        assert_eq!(parse_kanji_run("三万五千"), Some(35_000.0));
        assert_eq!(parse_kanji_run("万"), None);
    }

    #[test]
    fn eras_wari_and_clock_hours_are_alternative_readings() {
        let report = check(
            "令和8年度の予算案。生産能力を2割引き上げる。午後9時まで開館し、15時に閉める日もある。",
            "2026年度の予算案で、生産能力を20%上げる。21時まで開館し、午後3時に閉める日もある。",
        );
        assert!(report.passed(), "{report:?}");
        let reverse = check("生産能力を20%上げる。", "生産能力を2割上げる。");
        assert!(reverse.passed(), "{reverse:?}");
    }

    #[test]
    fn simplified_chinese_outside_the_input_is_detected() {
        let report = check("新しい電池を発売する。", "新しい电池を发売する。");
        assert_eq!(kinds(&report), vec![IssueKind::SimplifiedChinese]);
        assert!(report.issues[0].reason.contains('发'));
        // 材料にもある字（中国の地名など）は数えない。
        let in_input = check("中国の东莞で展示会。", "中国の东莞で展示会が開かれる。");
        assert!(in_input.passed(), "{in_input:?}");
    }

    #[test]
    fn prompt_echo_is_detected_unless_the_word_is_in_the_input() {
        let echo = check(
            "市は給食費を無償化する。",
            "記事情報によると、市は給食費を無償化する。",
        );
        assert_eq!(kinds(&echo), vec![IssueKind::PromptEcho]);
        let fixed = ["記事情報に書かれている事実だけを使ってください。"];
        let sentence = check_japanese_summary(
            "市は給食費を無償化する。",
            "市は給食費を無償化する。記事情報に書かれている事実だけを使ってください。",
            None,
            &fixed,
        );
        assert_eq!(kinds(&sentence), vec![IssueKind::PromptEcho]);
        // 評価で見つかった、指示の言い回しの混入。
        let phrase = check(
            "大企業では67.2％。",
            "大企業では67.2％と高いという事実だけを使いました。",
        );
        assert_eq!(kinds(&phrase), vec![IssueKind::PromptEcho]);
        let in_input = check(
            "外部データを使った新しい分析を始める。",
            "外部データを使った分析を始める。",
        );
        assert!(in_input.passed(), "{in_input:?}");
    }

    #[test]
    fn meta_comments_are_reported_but_do_not_trigger_a_retry() {
        let report = check(
            "市は図書館を延長する。",
            "市は図書館を延長する。費用は記事には明記されていない。",
        );
        assert_eq!(kinds(&report), vec![IssueKind::MetaComment]);
        assert!(report
            .kinds_where(IssueKind::triggers_summary_retry)
            .is_empty());
    }

    #[test]
    fn length_empty_and_language_are_checked() {
        let long = check_japanese_summary("材料。", &"あ".repeat(121), Some(120), &[]);
        assert_eq!(kinds(&long), vec![IssueKind::TooLong]);
        let empty = check("材料。", " \n");
        assert_eq!(kinds(&empty), vec![IssueKind::Empty]);
        let english = check("材料。", "The city will make school lunches free.");
        assert_eq!(kinds(&english), vec![IssueKind::NotJapanese]);
    }

    #[test]
    fn retry_kinds_are_sorted_and_deduplicated() {
        let report = check("対象は28校。", "対象は35校と40校で、电池を使う。");
        assert_eq!(
            report.kinds_where(IssueKind::triggers_summary_retry),
            vec![IssueKind::UnknownNumber, IssueKind::SimplifiedChinese]
        );
    }
}
