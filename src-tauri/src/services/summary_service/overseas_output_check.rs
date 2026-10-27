//! 海外（英語）記事の日本語要約・訳の機械点検（判断台帳 D165 の試作。テストビルド専用で、アプリの挙動は変えない）。
//!
//! 第1弾（D164）で、プロンプトだけでは「数字・単位の取り違え」「中国語の簡体字の混入」「指示の写し」「長さ超過」を
//! 防げなかった。小さいモデルの出力を人が読む前に、要旨（英語）と突き合わせて機械的に引っかけ、
//! 理由を添えて1回だけ作り直すための点検をここに置く。すべて純粋関数（通信・ファイル操作なし）。
//!
//! 点検の考え方:
//! - **数字**: 要約に出る数字（万・億などの単位を掛けた値）は、要旨のどれかの数字（million などを掛けた値、
//!   掛けない値、one〜twenty などの英単語）と一致しなければならない。一致せず 10 のべき乗倍だけずれているものは
//!   「桁違い」として別に知らせる（$2.5M →「2500万ドル」のような換算の誤り）。数字の落ちは要約では起こりうるので、
//!   作り直しの理由にせず、残った割合だけを返す。
//! - **簡体字**: 日本語では使わない簡体字（「经」「网」「络」など）と、中国語にしかない2字の並び（「及其」など）。
//!   日本語の新字体と同じ字（「国」「写」「与」など）は入れない（誤検出を避けるため）。
//! - **指示の写し**: プロンプトの見出し・きまりの語（「きまり」「記事情報」「###」など）と、
//!   既存の `strip_prompt_echo_sentences` による固定指示の文の写し。
//! - **長さ**: 文字数の上限（空白を除く）。空の出力も問題にする。
//! - **日本語でない**: かなの割合が低すぎる（英語のまま返した）出力。
//!
//! 国内記事の要約へ流用するときは、数字の照合の「要旨の数字」を日本語の抜粋の数字に替えれば同じ形で使える
//! （簡体字・写し・長さ・日本語らしさの点検は言語に依存しない）。

use crate::util::speech_cleanup::strip_prompt_echo_sentences;

/// 点検で見つかった問題の1件。`reason` は作り直しのプロンプトにそのまま添える日本語の説明。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CheckIssue {
    pub kind: IssueKind,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IssueKind {
    /// 要旨のどの数字とも合わない数字。
    UnknownNumber,
    /// 要旨の数字と 10 のべき乗倍ずれた数字（単位の換算の誤り）。
    ScaleMismatch,
    SimplifiedChinese,
    PromptEcho,
    /// 「要旨には数字が書かれていない」のような、記事の内容ではなく材料についての注記。
    MetaComment,
    TooLong,
    Empty,
    NotJapanese,
}

impl IssueKind {
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
}

/// 点検の結果。`issues` が空なら合格。`source_number_kept` は要旨の数字のうち要約に残った割合（参考値）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CheckReport {
    pub issues: Vec<CheckIssue>,
    pub source_number_kept: Option<f64>,
}

impl CheckReport {
    pub(super) fn passed(&self) -> bool {
        self.issues.is_empty()
    }
}

/// 要約（または要旨の訳）を、元の英語の要旨と突き合わせて点検する。
///
/// - `source`: 元の英語の要旨（タイトルを含めてよい。数字の照合の元になる）。
/// - `fixed_prompt_texts`: プロンプトの固定指示（記事を含まない部分）。写しの検出に使う。
pub(super) fn check_overseas_output(
    source: &str,
    output: &str,
    max_chars: usize,
    fixed_prompt_texts: &[&str],
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
    let (number_issues, kept) = check_numbers(source, output);
    issues.extend(number_issues);
    issues.extend(check_simplified_chinese(output));
    issues.extend(check_prompt_echo(output, fixed_prompt_texts));
    issues.extend(check_meta_comment(output));
    if body_chars > max_chars {
        issues.push(issue(
            IssueKind::TooLong,
            format!("要約が{body_chars}文字で、上限の{max_chars}文字を超えていた。"),
        ));
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

/// 英語どうし（要旨と、要旨から抜き出した事実）の数字の照合。抽出段の点検に使う。
/// 抜き出した事実に、要旨にない数字が入っていないかだけを見る。
pub(super) fn check_english_numbers(source: &str, extracted: &str) -> Vec<CheckIssue> {
    let allowed = english_number_values(source);
    let mut issues = Vec::new();
    for found in english_numbers(extracted) {
        if !found
            .values
            .iter()
            .any(|value| contains_value(&allowed, *value))
        {
            issues.push(issue(
                IssueKind::UnknownNumber,
                format!("「{}」は要旨にない数字だった。", found.text),
            ));
        }
    }
    issues
}

fn issue(kind: IssueKind, reason: String) -> CheckIssue {
    CheckIssue { kind, reason }
}

// ---------------------------------------------------------------------------
// 数字
// ---------------------------------------------------------------------------

/// 文中で見つけた1つの数字。`values` はその数字が表しうる値（単位を掛けた値・掛けない値）。
#[derive(Debug, Clone, PartialEq)]
struct FoundNumber {
    text: String,
    values: Vec<f64>,
}

/// 英単語の数（要旨の "three years" と要約の「3年」を突き合わせるため）。序数も同じ値として扱う。
const ENGLISH_NUMBER_WORDS: [(&str, f64); 40] = [
    ("zero", 0.0),
    ("one", 1.0),
    ("single", 1.0),
    ("first", 1.0),
    ("two", 2.0),
    ("second", 2.0),
    ("twice", 2.0),
    ("double", 2.0),
    ("both", 2.0),
    ("three", 3.0),
    ("third", 3.0),
    ("triple", 3.0),
    ("four", 4.0),
    ("fourth", 4.0),
    ("quarter", 4.0),
    ("five", 5.0),
    ("fifth", 5.0),
    ("six", 6.0),
    ("seven", 7.0),
    ("eight", 8.0),
    ("nine", 9.0),
    ("ten", 10.0),
    ("eleven", 11.0),
    ("twelve", 12.0),
    ("dozen", 12.0),
    ("thirteen", 13.0),
    ("fifteen", 15.0),
    ("twenty", 20.0),
    ("thirty", 30.0),
    ("forty", 40.0),
    ("fifty", 50.0),
    ("hundred", 100.0),
    ("hundreds", 100.0),
    ("thousand", 1_000.0),
    ("thousands", 1_000.0),
    ("million", 1_000_000.0),
    ("millions", 1_000_000.0),
    ("billion", 1_000_000_000.0),
    ("billions", 1_000_000_000.0),
    ("half", 2.0),
];

/// 英語の数字の後ろに付く倍率（"2.5M"・"3 billion"・"10K"）。
fn english_multiplier(word: &str) -> Option<f64> {
    match word {
        "k" | "thousand" => Some(1_000.0),
        "m" | "mn" | "million" | "millions" => Some(1_000_000.0),
        "b" | "bn" | "billion" | "billions" => Some(1_000_000_000.0),
        "t" | "trillion" => Some(1_000_000_000_000.0),
        _ => None,
    }
}

/// 英文の数字（算用数字）を取り出す。直後の倍率（M・million など）があれば、掛けた値も候補に入れる。
fn english_numbers(text: &str) -> Vec<FoundNumber> {
    let chars: Vec<char> = text.chars().map(to_halfwidth).collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let Some((value, end)) = parse_number_at(&chars, index) else {
            index += 1;
            continue;
        };
        let mut values = vec![value];
        // 直後の倍率（空白・ハイフンを挟んでもよい）: "2.5M", "1 million", "1-gigawatt" は倍率なし。
        let mut cursor = end;
        while cursor < chars.len() && (chars[cursor] == ' ' || chars[cursor] == '-') {
            cursor += 1;
        }
        let word: String = chars[cursor..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(multiplier) = english_multiplier(&word) {
            values.push(value * multiplier);
        }
        found.push(FoundNumber {
            text: chars[index..end].iter().collect(),
            values,
        });
        index = end;
    }
    found
}

/// 要旨の数字として許す値の一覧（算用数字・倍率を掛けた値・英単語の数）。
fn english_number_values(source: &str) -> Vec<f64> {
    let mut values: Vec<f64> = english_numbers(source)
        .into_iter()
        .flat_map(|found| found.values)
        .collect();
    let lower = source.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
        .filter(|word| !word.is_empty())
        .collect();
    for (index, word) in words.iter().enumerate() {
        let word = word.trim_end_matches('.');
        let number = ENGLISH_NUMBER_WORDS
            .iter()
            .find(|(w, _)| *w == word)
            .map(|(_, value)| *value)
            // 算用数字の語（"five decades" と同じく "5 decades" も見るため）。"inf" などを数にしないよう、数字で始まる語だけ。
            .or_else(|| {
                word.starts_with(|c: char| c.is_ascii_digit())
                    .then(|| word.parse::<f64>().ok())
                    .flatten()
            });
        if let Some(value) = number {
            values.push(value);
            // "five decades" は「50年」と訳すのが正しい（第2弾の1回目で桁違いと誤検出した）。
            if words
                .get(index + 1)
                .is_some_and(|next| next.trim_end_matches('.').starts_with("decade"))
            {
                values.push(value * 10.0);
            }
        }
        // "two-thirds" の "thirds" など、語尾の s を除いた形も見る。
        if let Some(stem) = word.strip_suffix('s') {
            if let Some((_, value)) = ENGLISH_NUMBER_WORDS.iter().find(|(w, _)| *w == stem) {
                values.push(*value);
            }
        }
    }
    // "a million users" のように、単語だけの倍率は 1 × 倍率の値として既に入っている。
    values
}

/// 日本語の数字の単位（「2500万」「10億」「1.5兆」「67百万」）。長いものから照合する。
const JAPANESE_MULTIPLIERS: [(&str, f64); 6] = [
    ("百万", 1_000_000.0),
    ("千万", 10_000_000.0),
    ("十億", 1_000_000_000.0),
    ("万", 10_000.0),
    ("億", 100_000_000.0),
    ("兆", 1_000_000_000_000.0),
];

/// 日本語の要約の数字（算用数字。全角も可）を取り出す。直後の「万・億・百万」などや M・billion を掛けた値にする。
/// 「1億2000万」のような続きの数は、足し合わせた値にする。
fn japanese_numbers(text: &str) -> Vec<FoundNumber> {
    let chars: Vec<char> = text.chars().map(to_halfwidth).collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let Some((value, end)) = parse_number_at(&chars, index) else {
            index += 1;
            continue;
        };
        // 小さいモデルは「250 万ドル」のように数字と単位の間に空白を入れることがある（第2弾の1回目で誤検出した）。
        let mut unit_start = end;
        while unit_start < chars.len() && chars[unit_start] == ' ' {
            unit_start += 1;
        }
        let rest: String = chars[unit_start..].iter().take(3).collect();
        let mut total = value;
        let mut cursor = end;
        if let Some((unit, multiplier)) = JAPANESE_MULTIPLIERS
            .iter()
            .find(|(unit, _)| rest.starts_with(unit))
        {
            total = value * multiplier;
            cursor = unit_start + unit.chars().count();
            // 「1億2000万」: 億のすぐ後ろの数を足す。
            if let Some((next_value, next_end)) = parse_number_at(&chars, cursor) {
                let next_rest: String = chars[next_end..].iter().take(1).collect();
                if *multiplier >= 100_000_000.0 && next_rest == "万" {
                    total += next_value * 10_000.0;
                    cursor = next_end + 1;
                }
            }
        } else {
            let mut word_start = end;
            while word_start < chars.len() && chars[word_start] == ' ' {
                word_start += 1;
            }
            let word: String = chars[word_start..]
                .iter()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect::<String>()
                .to_ascii_lowercase();
            if let Some(multiplier) = english_multiplier(&word) {
                total = value * multiplier;
                cursor = word_start + word.len();
            }
        }
        found.push(FoundNumber {
            text: chars[index..cursor].iter().collect(),
            values: vec![total],
        });
        index = cursor.max(end);
    }
    found
}

/// `chars[start..]` が数字で始まるとき、その値と終わりの位置を返す（3桁区切りのカンマ・小数点を含める）。
/// 英字の直後の数字（"GPT4" の 4 など）も数字として扱う（名前の中の数字は要旨にも同じ形で出るため照合で通る）。
fn parse_number_at(chars: &[char], start: usize) -> Option<(f64, usize)> {
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

fn to_halfwidth(c: char) -> char {
    match c {
        '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
        '．' => '.',
        '，' => ',',
        _ => c,
    }
}

fn contains_value(values: &[f64], target: f64) -> bool {
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

/// 要約の数字を要旨と照合する。戻り値の2つ目は、要旨の算用数字のうち要約に残った割合（要旨に数字がなければ None）。
fn check_numbers(source: &str, output: &str) -> (Vec<CheckIssue>, Option<f64>) {
    let allowed = english_number_values(source);
    let output_numbers = japanese_numbers(output);
    let mut issues = Vec::new();
    for found in &output_numbers {
        let value = found.values[0];
        if contains_value(&allowed, value) {
            continue;
        }
        if let Some(near) = allowed
            .iter()
            .find(|candidate| differs_by_power_of_ten(value, **candidate))
        {
            issues.push(issue(
                IssueKind::ScaleMismatch,
                format!(
                    "「{}」は要旨の数字（{}）と桁が合わなかった。million は百万、billion は十億として換算する。",
                    found.text,
                    format_value(*near)
                ),
            ));
        } else {
            issues.push(issue(
                IssueKind::UnknownNumber,
                format!(
                    "「{}」は材料にない数字だった。材料にない数字は書かない。",
                    found.text
                ),
            ));
        }
    }
    let source_numbers = english_numbers(source);
    let kept = (!source_numbers.is_empty()).then(|| {
        let output_values: Vec<f64> = output_numbers.iter().map(|found| found.values[0]).collect();
        let kept = source_numbers
            .iter()
            .filter(|found| {
                found
                    .values
                    .iter()
                    .any(|v| contains_value(&output_values, *v))
            })
            .count();
        kept as f64 / source_numbers.len() as f64
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

/// 日本語では使わない簡体字（日本語の新字体と同じ形の字は入れない）。第1弾で混入した「经」「网」「络」を含む。
const SIMPLIFIED_ONLY_CHARS: &str = "们这说对时间发现实问题过还进动开关样为门导变让认识语设计论应该专业产术态检测总结读书长网络经种么吗呢个东车亿无处务员华际级线统组织构选择带帮层节视频览载输类显页风险复杂简单观欢环习练运营资讯气电脑软确报图标记录质达并从两讨训评价值证验优势战释执链获码别愿";

/// 中国語にしかない2字の並び（字だけでは日本語と区別できないもの）。第1弾の「及其」を含む。
/// 「相干」「良率」は字は日本語にもあるが、語としては中国語（日本語は「コヒーレント」「歩留まり」。第2弾の1回目で混入）。
const SIMPLIFIED_ONLY_BIGRAMS: [&str; 7] = ["及其", "以及", "我们", "他们", "一个", "相干", "良率"];

fn check_simplified_chinese(output: &str) -> Vec<CheckIssue> {
    let mut hits: Vec<String> = output
        .chars()
        .filter(|c| SIMPLIFIED_ONLY_CHARS.contains(*c))
        .map(String::from)
        .collect();
    hits.extend(
        SIMPLIFIED_ONLY_BIGRAMS
            .iter()
            .filter(|bigram| output.contains(**bigram))
            .map(|bigram| bigram.to_string()),
    );
    hits.sort_unstable();
    hits.dedup();
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
// 指示の写し・日本語らしさ
// ---------------------------------------------------------------------------

/// プロンプトの見出し・きまりの語。要約の本文に出るのは写しのときだけ。
const PROMPT_ECHO_MARKERS: [&str; 12] = [
    "きまり",
    "記事情報",
    "外部データ",
    "厳守事項",
    "###",
    "要旨:",
    "要旨：",
    "タイトル:",
    "タイトル：",
    "事実の一覧",
    "用語の訳",
    "文字以内",
];

fn check_prompt_echo(output: &str, fixed_prompt_texts: &[&str]) -> Vec<CheckIssue> {
    let mut hits: Vec<&str> = PROMPT_ECHO_MARKERS
        .iter()
        .copied()
        .filter(|marker| output.contains(marker))
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

/// 材料・要旨そのものについての注記の言い回し（第2弾の1回目で「抽象書には具体的な数字…が明記されていなかった」
/// 「提供された事実の一覧には…」が出た）。記事の本文に出ることはまれな言い回しに絞る。
const META_COMMENT_MARKERS: [&str; 8] = [
    "抽象書",
    "抽象文",
    "提供された",
    "明記されていな",
    "記載されていな",
    "言及されていな",
    "記事には",
    "要旨には",
];

fn check_meta_comment(output: &str) -> Vec<CheckIssue> {
    let hits: Vec<&str> = META_COMMENT_MARKERS
        .iter()
        .copied()
        .filter(|marker| output.contains(marker))
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

    #[test]
    fn numbers_converted_correctly_pass() {
        let source = "Google is investing $2.5M to help 1 million farmers over three years.";
        let report = check_overseas_output(
            source,
            "Googleは250万ドルを投じ、3年間で100万人の農家を支援する。",
            120,
            &[],
        );
        assert!(report.passed(), "{report:?}");
        assert_eq!(report.source_number_kept, Some(1.0));
    }

    #[test]
    fn scale_mismatch_is_reported_separately() {
        // 第1弾の誤り（$2.5M →「2500万ドル」）。
        let source = "Google is investing $2.5M to help farmers.";
        let report = check_overseas_output(
            source,
            "Googleは2500万ドルを投じて農家を支援する。",
            120,
            &[],
        );
        assert_eq!(kinds(&report), vec![IssueKind::ScaleMismatch]);
        assert!(report.issues[0].reason.contains("2500万"));
    }

    #[test]
    fn invented_number_is_reported() {
        // 第1弾の誤り（20 か所 →「2ヶ所」は桁違い、three years →「3ヶ月」は数字としては合う）。
        let source = "The program reached 20 sites in Kenya.";
        let report = check_overseas_output(source, "ケニアの2か所と15の学校で実施した。", 120, &[]);
        assert_eq!(
            kinds(&report),
            vec![IssueKind::ScaleMismatch, IssueKind::UnknownNumber]
        );
    }

    #[test]
    fn japanese_units_and_compound_numbers_are_parsed() {
        let values = |text: &str| -> Vec<f64> {
            japanese_numbers(text)
                .into_iter()
                .map(|found| found.values[0])
                .collect()
        };
        assert_eq!(values("6700万人"), vec![67_000_000.0]);
        assert_eq!(values("10億ドルと1億2000万人"), vec![1e9, 1.2e8]);
        assert_eq!(values("１，２００件"), vec![1200.0]);
        assert_eq!(values("2.5 billionドル"), vec![2.5e9]);
        assert_eq!(values("3〜4倍"), vec![3.0, 4.0]);
    }

    #[test]
    fn english_numbers_and_words_are_allowed() {
        let source = "A 1-gigawatt plant for 67 million people; accuracy rose from 9.3 to 2,300 cases, a two-thirds cut.";
        let values = english_number_values(source);
        for expected in [1.0, 67.0, 67e6, 9.3, 2300.0, 2.0, 3.0] {
            assert!(
                contains_value(&values, expected),
                "{expected} not in {values:?}"
            );
        }
        let report = check_overseas_output(
            source,
            "1ギガワットの発電所が6700万人に電力を届ける。件数は9.3から2,300に増え、3分の2を削った。",
            200,
            &[],
        );
        assert!(report.passed(), "{report:?}");
    }

    #[test]
    fn names_with_digits_are_not_flagged_when_in_source() {
        let source = "We evaluate GPT-4o and Qwen3.5-2B on 12 benchmarks.";
        let report = check_overseas_output(
            source,
            "GPT-4oとQwen3.5-2Bを12のベンチマークで評価した。",
            120,
            &[],
        );
        assert!(report.passed(), "{report:?}");
    }

    #[test]
    fn spaced_units_and_decades_are_understood() {
        let source = "Google.org is investing $2.5M to help 1 million farmers; we have worked for five decades.";
        let report = check_overseas_output(
            source,
            "Google.org は 250 万ドルで 100 万人の農家を支える。50年の経験がある。",
            120,
            &[],
        );
        assert!(report.passed(), "{report:?}");
    }

    #[test]
    fn meta_comments_are_detected() {
        let report = check_overseas_output(
            "Google partnered with experts.",
            "Google は専門家と組んだ。抽象書には具体的な数字が明記されていなかった。",
            120,
            &[],
        );
        assert_eq!(kinds(&report), vec![IssueKind::MetaComment]);
    }

    #[test]
    fn simplified_chinese_is_detected_but_japanese_shinjitai_is_not() {
        let report =
            check_overseas_output("A model.", "神经网络を使った国の研究を写した。", 120, &[]);
        assert_eq!(kinds(&report), vec![IssueKind::SimplifiedChinese]);
        assert!(report.issues[0].reason.contains("经"));
        let bigram = check_overseas_output("A model.", "家族及其の生活を支える。", 120, &[]);
        assert_eq!(kinds(&bigram), vec![IssueKind::SimplifiedChinese]);
        for text in [
            "自愿退職を提示した。",
            "相干性ノイズを測った。",
            "業界は良率で評価する。",
        ] {
            let report = check_overseas_output("A model.", text, 120, &[]);
            assert_eq!(kinds(&report), vec![IssueKind::SimplifiedChinese], "{text}");
        }
        let japanese = check_overseas_output(
            "A model.",
            "国と地域の学習を与え、写真を図に示した。",
            120,
            &[],
        );
        assert!(japanese.passed(), "{japanese:?}");
    }

    #[test]
    fn prompt_echo_is_detected() {
        let fixed = ["要旨にない数値・日付・人物を足さないでください。"];
        let marker = check_overseas_output(
            "A model.",
            "きまり: 本文だけを書く。モデルを示した。",
            120,
            &fixed,
        );
        assert_eq!(kinds(&marker), vec![IssueKind::PromptEcho]);
        let sentence = check_overseas_output(
            "A model.",
            "新しいモデルを示した。要旨にない数値・日付・人物を足さないでください。",
            120,
            &fixed,
        );
        assert_eq!(kinds(&sentence), vec![IssueKind::PromptEcho]);
    }

    #[test]
    fn length_empty_and_language_are_checked() {
        let long = check_overseas_output("A model.", &"あ".repeat(121), 120, &[]);
        assert_eq!(kinds(&long), vec![IssueKind::TooLong]);
        let spaced = check_overseas_output("A model.", &format!("{} ", "あ".repeat(120)), 120, &[]);
        assert!(spaced.passed());
        let empty = check_overseas_output("A model.", "  \n", 120, &[]);
        assert_eq!(kinds(&empty), vec![IssueKind::Empty]);
        let english = check_overseas_output(
            "A model.",
            "The authors propose a new model for agents.",
            120,
            &[],
        );
        assert_eq!(kinds(&english), vec![IssueKind::NotJapanese]);
        let names = check_overseas_output(
            "A model.",
            "MicrosoftはCopilotにHome、Code、Autopilotを加えたと発表した。",
            120,
            &[],
        );
        assert!(names.passed(), "{names:?}");
    }

    #[test]
    fn extracted_english_facts_are_checked_against_the_abstract() {
        let source = "Accuracy improved from 61% to 78% on 3 datasets.";
        assert!(check_english_numbers(
            source,
            "- Accuracy rose from 61% to 78% on three datasets."
        )
        .is_empty());
        let issues = check_english_numbers(source, "- Accuracy rose to 87% on 3 datasets.");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].reason.contains("87"));
    }
}
