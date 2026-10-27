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
//! D166 で、言語に依存しない部分（日本語の数字の読み取り・簡体字・写し・注記・長さ・日本語らしさ）を
//! 国内記事の要約と共通の [`super::output_check`] へ移した。ここには英語の要旨の数字の読み取りと、
//! 海外の試作のプロンプトに合わせた語の一覧だけを残す。

use super::output_check::{
    check_output, contains_value, english_multiplier, parse_number_at, to_halfwidth, CheckIssue,
    CheckOptions, CheckReport, FoundNumber, IssueKind, SourceNumbers,
};

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
    let listed: Vec<Vec<f64>> = english_numbers(source)
        .into_iter()
        .map(|found| found.values)
        .collect();
    check_output(
        &SourceNumbers {
            allowed: english_number_values(source),
            listed,
        },
        output,
        &CheckOptions {
            input_text: source,
            max_chars: Some(max_chars),
            echo_markers: &PROMPT_ECHO_MARKERS,
            meta_markers: &META_COMMENT_MARKERS,
            fixed_prompt_texts,
            scale_hint: "million は百万、billion は十億として換算する。",
        },
    )
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
            issues.push(CheckIssue {
                kind: IssueKind::UnknownNumber,
                reason: format!("「{}」は要旨にない数字だった。", found.text),
            });
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// 英語の数字
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use super::super::output_check::japanese_numbers;
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
