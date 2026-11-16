//! ゆうこの解説（再説明）の追加の点検（Firestore タスク 722cmK）。純粋関数（通信・ファイル操作なし）。
//!
//! Gemma 4 E2B の再説明の誤りは、記事にない理由・つながりの言い足し・記事にない評価・主体が多かった
//! （要約の機械点検の評価 §23）。既存の点検（`check_speech_consistency`）に次を足す。**本番の既定では使わない**
//! （`ExplanationChecks::Off`）。評価の版（`YUUKO_METHOD_EVAL_VARIANT` の `checks` / `checks_summary`）でだけ有効にする。
//!
//! - (a) 理由のつながり（`causal_check::find_unsupported_causes_broad`）: 出力の側のつなぎを広げ（「ので」「から、」
//!   目的の「〜するため」）、材料に要約も入れる。材料に同じ因果の文（因果の言い方と前後の語）があれば通す。
//! - (b) 記事にない名前（`unknown_names`）: カタカナ語・英字の語・「〜氏」「〜選手」の前の漢字の名前のうち、
//!   材料（タイトル＋本文抜粋）にないもの。よくある一般の語（`COMMON_WORDS`）は除く。一覧は評価の出力を見る前に
//!   決めた（§12 の教訓: 一覧を決めたデータで効果を測らない）。
//! - (c) 言い切り（`asserted_hedges`）: 材料では「予定」「見通し」「可能性」などの言い方の直前にある出来事を、
//!   出力が推測・予定の言い方なしに書いた。**記録だけ**（作り直し・文を落とす理由にしない）。
//! - 要約との照合（`unsupported_by_summary`。版 `checks_summary` だけ）: 評価・見通し・誇張の語（§10 の
//!   保留の点検と同じ一覧）が要約にない文を落とす。

use super::causal_check::find_unsupported_causes_broad;
use super::leading_connector::{content_bigrams, normalize};

/// 再説明の追加の点検の使い方。本番の既定は `Off`（評価のハーネスだけが変える）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ExplanationChecks {
    /// 既存の点検と定型文への切り替えだけ（本番）。
    #[default]
    Off,
    /// (a)(b) で1回作り直し、残った文を落とす。全部落ちたら定型文。
    Checks,
    /// `Checks` に加え、要約に裏付けのない評価の語の文を落とす。
    ChecksSummary,
}

/// カタカナ語・英字の語のうち、記事になくても作り話とみなさない一般の語・ゆうこの話し言葉（小文字で照合）。
#[rustfmt::skip]
const COMMON_WORDS: &[&str] = &[
    "ニュース", "ゲーム", "サービス", "ユーザー", "ユーザ", "データ", "システム", "チーム", "ファン", "サポーター", "プレー", "プレイ",
    "ポイント", "シーズン", "スポーツ", "イベント", "ネット", "インターネット", "スマホ", "スマートフォン", "アプリ", "ソフト", "ソフトウェア",
    "ハードウェア", "パソコン", "メーカー", "ブランド", "モデル", "デザイン", "テクノロジー", "テスト", "トラブル", "リスク", "セキュリティ",
    "プライバシー", "コスト", "ビジネス", "ルール", "メリット", "イメージ", "ケース", "レベル", "ペース", "スタート", "ゴール", "チャンス",
    "ピンチ", "スタイル", "テーマ", "サポート", "バージョン", "ファイル", "ツール", "コンテンツ", "メディア", "コメント", "メッセージ",
    "プロジェクト", "プラン", "グループ", "メンバー", "リーダー", "トップ", "ベスト", "シリーズ", "キャラクター", "ストーリー", "オンライン",
    "クラウド", "ウェブ", "エンジニア", "チェック", "アイデア", "パワー", "スピード", "エネルギー", "ワクワク", "ドキドキ", "ビックリ", "ホッ",
    "ニコニコ", "キラキラ", "ガッカリ", "スゴイ", "ナイス", "ハラハラ", "ピッタリ", "ニッコリ", "ai", "it", "pc", "sns", "os",
    "web", "cpu", "gpu",
];

/// 名前の後ろに付く語（この前の漢字の連なりを人・団体の名前とみなす）。
const NAME_SUFFIXES: &[&str] = &["氏", "さん", "選手", "監督", "社長", "容疑者", "被告"];

/// 材料で、直前の出来事を推測・予定として書く言い方。
#[rustfmt::skip]
const HEDGE_MARKERS: &[&str] = &[
    "ようだ", "予定", "見通し", "とみられ", "と見られ", "可能性", "見込み", "方針", "予想", "恐れ", "おそれ", "かもしれ",
];
/// 出力がこれを含む文は言い切っていないとみなす（ゆうこの話し言葉の推測を含む）。
#[rustfmt::skip]
const OUTPUT_HEDGES: &[&str] = &[
    "みたい", "そう", "らしい", "かも", "だろう", "でしょう", "検討", "目指", "計画",
];
/// 材料の言い方の前で、出来事の語を探す字数。
const HEDGE_WINDOW_CHARS: usize = 12;

/// 要約にないと言い足しになる評価・見通し・誇張・意図の語（§10 の保留の点検 `sentence_support` と同じ一覧）。
#[rustfmt::skip]
const JUDGMENT_WORDS: &[&str] = &[
    "大幅", "劇的", "飛躍的", "急激", "急速", "著しく", "圧倒的", "驚異的", "極めて", "深刻", "重大", "甚大", "惨敗", "完敗", "大敗",
    "圧勝", "快勝", "歴史的", "史上", "画期的", "異例", "完全に", "懸念", "危惧", "不安", "危う", "危機", "脅威", "混乱", "波紋",
    "衝撃", "批判", "反発", "非難", "称賛", "注目", "失望", "打撃", "見通し", "見込", "予想", "予測", "加速", "拍車", "促進", "広が",
    "高ま", "強ま", "波及", "恐れ", "可能性", "期待", "狙い", "狙う", "狙っ", "目的", "意図", "誘導", "図る", "図っ", "原因", "誤解",
    "せいで",
];
/// 評価の語を含むが別の意味の複合語。
const JUDGMENT_COMPOUND_EXCEPTIONS: &[&str] = &["不安定", "危機管理", "目的地"];

/// 1文の点検の結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SentenceFindings {
    /// (a) 材料にない理由のつながりがある。
    pub unsupported_link: bool,
    /// (b) 材料にない名前。
    pub unknown_names: Vec<String>,
    /// (c) 材料では推測・予定の出来事を言い切った（記録だけ）。材料の言い方。
    pub asserted: Vec<&'static str>,
    /// 要約にない評価の語（版 `checks_summary` だけで使う）。
    pub unsupported_by_summary: Vec<&'static str>,
}

impl SentenceFindings {
    /// (a)(b) のどれか（作り直し・文を落とす理由）。(c) と要約との照合は含めない。
    pub(crate) fn flagged(&self) -> bool {
        self.unsupported_link || !self.unknown_names.is_empty()
    }
}

/// 文の終わり（区切りの字を含めて文に残す。かぎかっこの中では分けない）で分ける。空白だけの文は除く。
pub(crate) fn split_keeping_ends(text: &str) -> Vec<&str> {
    let mut sentences: Vec<&str> = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '「' | '『' => depth += 1,
            '」' | '』' => depth = depth.saturating_sub(1),
            '\n' => depth = 0,
            _ => {}
        }
        let is_end = c == '\n' || (depth == 0 && matches!(c, '。' | '！' | '？' | '!' | '?'));
        // 続く区切りの字（「！？」）は同じ文に含める。
        if is_end
            && !chars
                .peek()
                .is_some_and(|(_, n)| matches!(n, '！' | '？' | '!' | '?'))
        {
            let end = index + c.len_utf8();
            sentences.push(&text[start..end]);
            start = end;
        }
    }
    sentences.push(&text[start..]);
    sentences.retain(|s| !s.trim().is_empty());
    sentences
}

/// 1文を点検する。`source` はタイトル＋本文抜粋、`summary` は同じ生成の要約（無ければ空）。
pub(crate) fn check_sentence(source: &str, summary: &str, sentence: &str) -> SentenceFindings {
    let material = format!("{source}\n{summary}");
    SentenceFindings {
        unsupported_link: !find_unsupported_causes_broad(&material, sentence).is_empty(),
        unknown_names: unknown_names(source, sentence),
        asserted: asserted_hedges(source, sentence),
        unsupported_by_summary: unsupported_by_summary(summary, sentence),
    }
}

/// (b) 材料（`source`）にない名前（カタカナ語・英字の語・敬称などの前の漢字の名前）。一般の語は除く。
pub(crate) fn unknown_names(source: &str, text: &str) -> Vec<String> {
    let source = normalize(source);
    let mut names: Vec<String> = Vec::new();
    for name in name_candidates(&normalize(text)) {
        let known = source.contains(&name)
            || COMMON_WORDS.contains(&name.as_str())
            || is_compound_of_known(&source, &name);
        if !known && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn is_katakana(c: char) -> bool {
    matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー' | '・')
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '々')
}

/// 正規化した文から名前の候補を取る。
fn name_candidates(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let kind: Option<fn(char) -> bool> = if is_katakana(c) && c != 'ー' && c != '・' {
            Some(is_katakana)
        } else if c.is_ascii_alphabetic() {
            Some(|c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
        } else {
            None
        };
        let Some(kind) = kind else {
            i += 1;
            continue;
        };
        let start = i;
        while i < chars.len() && kind(chars[i]) {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let word = word.trim_end_matches(['・', '.', '-']).to_string();
        if word.chars().count() >= 2 {
            found.push(word);
        }
    }
    // 敬称・役職の前の漢字の名前（2〜4字）。
    for suffix in NAME_SUFFIXES {
        for (index, _) in text.match_indices(suffix) {
            let name: String = text[..index]
                .chars()
                .rev()
                .take_while(|&c| is_kanji(c))
                .collect();
            let name: String = name.chars().rev().collect();
            if (2..=4).contains(&name.chars().count()) {
                found.push(name);
            }
        }
    }
    found
}

/// カタカナ語が、材料にある 2 字以上の語 2 つをつないだ語か（「ゲームプレイ」）。
fn is_compound_of_known(source: &str, name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    (2..chars.len().saturating_sub(1)).any(|cut| {
        let (head, tail): (String, String) =
            (chars[..cut].iter().collect(), chars[cut..].iter().collect());
        let known = |part: &str| source.contains(part) || COMMON_WORDS.contains(&part);
        known(head.trim_end_matches('・')) && known(tail.trim_start_matches('・'))
    })
}

/// (c) 材料で推測・予定の言い方の直前にある出来事（2字の連なり 2 つ以上）を、推測・予定の言い方なしに書いた。
pub(crate) fn asserted_hedges(source: &str, sentence: &str) -> Vec<&'static str> {
    let sentence = normalize(sentence);
    if HEDGE_MARKERS
        .iter()
        .chain(OUTPUT_HEDGES.iter())
        .any(|h| sentence.contains(h))
    {
        return Vec::new();
    }
    let source = normalize(source);
    let mut found: Vec<&'static str> = Vec::new();
    for marker in HEDGE_MARKERS {
        for (index, _) in source.match_indices(marker) {
            let window: String = source[..index]
                .chars()
                .rev()
                .take(HEDGE_WINDOW_CHARS)
                .collect();
            let window: String = window.chars().rev().collect();
            let shared = content_bigrams(&window)
                .iter()
                .filter(|gram| sentence.contains(gram.as_str()))
                .count();
            if shared >= 2 && !found.contains(marker) {
                found.push(marker);
            }
        }
    }
    found
}

/// 要約にない評価・見通し・誇張の語（固定の一覧の語だけ）。
pub(crate) fn unsupported_by_summary(summary: &str, sentence: &str) -> Vec<&'static str> {
    let mut stripped = sentence.to_string();
    for exception in JUDGMENT_COMPOUND_EXCEPTIONS {
        stripped = stripped.replace(exception, "");
    }
    JUDGMENT_WORDS
        .iter()
        .copied()
        .filter(|word| stripped.contains(word) && !summary.contains(word))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 架空の記事（新しい家庭用ゲーム機）。
    const SOURCE: &str = "タイトル: あおばゲームスが新型機を発表\n本文抜粋:\n\
        あおばゲームスは20日、新型の家庭用ゲーム機「ミナトボックス」を発表した。\
        部品の値上がりのため、価格は従来機より5000円高い。\
        発売は来年3月の予定で、海外でも販売する見通しだ。佐藤社長は「長く遊べる機械にした」と話した。";
    const SUMMARY: &str =
        "あおばゲームスが新型機「ミナトボックス」を発表し、価格は従来機より5000円高い。";

    #[test]
    fn splits_sentences_keeping_the_ends() {
        let text = "新しいゲーム機だよ！値段は高いね。「楽しい。長い」と話したよ\n次の文？！";
        assert_eq!(
            split_keeping_ends(text),
            vec![
                "新しいゲーム機だよ！",
                "値段は高いね。",
                "「楽しい。長い」と話したよ\n",
                "次の文？！"
            ]
        );
        assert_eq!(split_keeping_ends(text).concat(), text);
        assert!(split_keeping_ends("").is_empty());
        assert_eq!(
            split_keeping_ends("終わりの字がない文"),
            vec!["終わりの字がない文"]
        );
    }

    #[test]
    fn a_reason_link_in_the_article_passes_and_an_added_one_is_found() {
        // 記事に同じ理由の言い回しと語があれば通す（言い換えの「ので」も）。
        for ok in [
            "部品の値上がりのため、価格は従来機より5000円高くなったんだって。",
            "部品が値上がりしたので、価格が高いんだよ。",
            "ミナトボックスは長く遊べる機械なんだって。",
        ] {
            assert!(
                !check_sentence(SOURCE, SUMMARY, ok).unsupported_link,
                "{ok}"
            );
        }
        // 記事にない因果（海外で売るので値上がり）。
        for added in [
            "海外でも販売するので、価格が高いんだって。",
            "海外でも販売するため、価格を高くしたんだよ。",
        ] {
            let findings = check_sentence(SOURCE, SUMMARY, added);
            assert!(findings.unsupported_link && findings.flagged(), "{added}");
        }
        // 場所・起点の「から」は見ない。
        assert!(
            !check_sentence(SOURCE, SUMMARY, "来年3月から海外でも販売する予定だよ。")
                .unsupported_link
        );
    }

    #[test]
    fn names_not_in_the_article_are_found_and_common_words_pass() {
        assert!(unknown_names(
            SOURCE,
            "ミナトボックスは新しいゲーム機だよ。ワクワクするね！"
        )
        .is_empty());
        assert!(
            unknown_names(SOURCE, "佐藤社長は、ファンのためにデザインしたんだって。").is_empty()
        );
        assert!(unknown_names(SOURCE, "あおばゲームスのゲームプレイが楽しみ！").is_empty());
        assert_eq!(
            unknown_names(SOURCE, "ソニーのPlayStationより安いんだって。"),
            vec!["ソニー".to_string(), "playstation".to_string()]
        );
        assert_eq!(
            unknown_names(SOURCE, "鈴木社長が話したよ。"),
            vec!["鈴木".to_string()]
        );
        // 全角の英字は半角と同じとみなす。
        assert!(unknown_names("AIの新機能", "ＡＩの新しい機能だよ。").is_empty());
    }

    #[test]
    fn asserting_a_planned_event_is_recorded_only() {
        let asserted = check_sentence(SOURCE, SUMMARY, "海外でも販売したんだよ。");
        assert_eq!(asserted.asserted, vec!["見通し"]);
        assert!(!asserted.flagged(), "記録だけで作り直しの理由にしない");
        for hedged in [
            "海外でも販売する見通しなんだって。",
            "海外でも販売するみたいだよ。",
            "価格は従来機より5000円高いよ。",
        ] {
            assert!(asserted_hedges(SOURCE, hedged).is_empty(), "{hedged}");
        }
    }

    #[test]
    fn judgment_words_not_in_the_summary_are_listed() {
        assert_eq!(
            unsupported_by_summary(SUMMARY, "今後の展開に注目だね！"),
            vec!["注目"]
        );
        assert!(
            unsupported_by_summary("市場の注目を集めた。", "注目を集めたんだって。").is_empty()
        );
        assert!(unsupported_by_summary(SUMMARY, "不安定な天気だね。").is_empty());
    }
}
