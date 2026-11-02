//! 要約一式（要点・要約・再説明・一言）の「意味の食い違い」と「見出しの残り」の点検（判断台帳 D170）。
//!
//! 実記事の評価（D169）で、数字の点検（`output_check`）をすり抜けて次の誤りが出た。ニュースの誤情報は
//! 重大な誤りなので、人が読む前に機械的に引っかける。すべて純粋関数（通信・ファイル操作なし）。
//!
//! - **見出しの残り**: 「**中心となる出来事の要約:**」のような Markdown の太字・見出し、「〜の要約:」の
//!   ラベル。消しても文が壊れないもの（行頭のラベル・太字だけの行・太字の記号）は `strip_heading_labels`
//!   で消し、消せない位置（文の途中）に残ったものは `IssueKind::Heading` として報告する。`#` の見出しの行は
//!   従来どおり保存前の検証（詳細設計書 §12.5）が出力ごと拒否する（消さない）。
//! - **反対の意味**（`IssueKind::Contradiction`）: 出力に「勝ち」があるのに、材料（記事のタイトル＋本文抜粋）
//!   には「勝」がなく「負け」がある、のように、出力の語の反対の語だけが材料にあるとき。語の組は
//!   `OPPOSITE_PAIRS` の固定の一覧（勝ち⇔負け・増加⇔減少・可決⇔否決・決定⇐検討/案・勝訴⇔敗訴・
//!   訴えを認め⇔退け など）。
//!   誤検出を減らすため:
//!   - 材料の側は、出力の語より広い言い方（「勝」の1字など、`also`）がどれか1つでもあれば「材料にもある」とする。
//!   - 打ち消し（「負けなかった」「決めておらず」「決定していない」「決定には至っていない」「未定」
//!     「連敗を止めた」）の語は、出力・材料のどちらでも数えない。
//!   - 出力が両方の側を言っているとき（「停止していた運転を再開した」）は報告しない。勝敗の組で主語ごとに勝敗を
//!     入れ替えた出力（「Aが勝った。Bは敗れた。」）と、足した勝敗の言い方（大勝・快勝・大敗 など）は、記録だけ
//!     （`contradiction_findings` の `recorded_only`。Firestore タスク R7cWic）。
//!   - 勝敗は、出力の語と材料の反対の語の主語（「は」「が」の前の名前、「Aの勝利」「勝ったのはA」の A）が
//!     同じときだけ報告する（「AがBを下した」を「BはAに敗れた」と言い換えた出力を誤検出しないため）。
//!   - 語の一部が別の意味になる複合語（「勝ち点」「勝負」「決勝」「準優勝」「勝利投手」「負担」）と、結果でない
//!     言い方（「勝利を目指し」「勝ってほしい」「連勝が止まった」）は数えない。
//!   - 材料に両方の語があるとき（両チームの勝敗が書かれた記事など）は報告しない。
//! - **年の組み合わせ**（`IssueKind::YearMismatch`）: 出力の文の「2026年」と出来事の語（「契約」など、
//!   `EVENT_WORDS`）の組が、材料のどの文にもないとき。材料ではその語が別の年（「2023年12月に…契約」）と
//!   同じ文にあり、出力の文の年がどれもその語と同じ文に出てこないときだけ報告する
//!   （材料の別の文に「2026年」があるので、値だけを見る数字の点検では拾えなかった）。出来事の語が2つ以上ある
//!   文と、材料にある年月日（記事の日付）として書かれた年は見ない。
//! - **材料にない年**（要点・再説明・一言用。`IssueKind::UnknownNumber`）: 「2025年」のような年が材料の
//!   数字のどれとも合わないとき。要約は `output_check` の数字の点検が年も含めて見ているので使わない。
//! - **推測・伝聞・訴えの言い切り**（`IssueKind::AssertedSpeculation`。D175）: 材料では「流出した可能性」
//!   「無償化を主張」のように推測・伝聞・訴えとしてだけ書かれた出来事の語を、出力が「流出した」「無償化が
//!   決まった」と言い切ったとき（`find_asserted_speculation`）。タグには使わない。
//! - **意味をなさない語・言い換えた指示の写し**（再説明・一言だけ。`check_speech_consistency`。Firestore タスク NkBSwN）:
//!   後処理で消せなかった「うんこ」など（`IssueKind::BrokenWord`）と、固定指示にだけ出る語句を含む文（`IssueKind::PromptEcho`）。
//!
//! 理由（`CheckIssue::reason`）に入る出力由来の文字は、固定の一覧の語と数字だけ（記事の文は入れない）。

use super::output_check::{
    contains_value, japanese_source_numbers, parse_number_at, to_halfwidth, CheckIssue, IssueKind,
};
use crate::util::speech_cleanup::{broken_words_in, instruction_phrases_in};

// ---------------------------------------------------------------------------
// 見出しの残り
// ---------------------------------------------------------------------------

/// 行頭のラベル・見出しとみなす長さの上限（文字数）。これより長い行頭の太字・ラベルは本文として残す。
const HEADING_MAX_CHARS: usize = 30;

/// 「〜の要約:」「ゆうこの一言：」のようなラベルの末尾の語。プロンプトの段の名前と、その言い換え。
/// 「説明」「回答」は「市の説明：『…』」のように本文の話し手を表すことがあるので入れない。
/// 話し手の言葉にも使う語（`SPEAKER_LABEL_WORDS`）は、語だけか、出力の自分を指す前置き（`SELF_LABEL_PREFIXES`）と
/// 語だけのラベル（「ゆうこの一言」「今回の感想」「AIまとめ」）のときだけラベルとみなす
/// （「首相の結論：」「監督の一言：」「今回の会見での首相の結論：」は本文として残す）。
const LABEL_TAIL_WORDS: [&str; 14] = [
    "要約",
    "まとめ",
    "感想",
    "一言",
    "ひとこと",
    "再説明",
    "解説",
    "コメント",
    "要点",
    "ポイント",
    "結論",
    "概要",
    "出力",
    "本文",
];

/// 話し手の言葉にも使うラベルの語（「首相の結論：」「監督の一言：」）。
const SPEAKER_LABEL_WORDS: [&str; 7] = [
    "結論",
    "解説",
    "コメント",
    "まとめ",
    "感想",
    "一言",
    "ひとこと",
];

/// 出力の自分（ゆうこ・AI・記事の要約）を指すラベルの前置き。`SPEAKER_LABEL_WORDS` は、ラベルがちょうど
/// 「前置き＋語」か「前置き＋の＋語」のときだけ消す（前置きで始まるだけの長いラベルは消さない）。
const SELF_LABEL_PREFIXES: [&str; 8] = [
    "ゆうこ",
    "AI",
    "今回",
    "この記事",
    "記事",
    "ニュース",
    "全体",
    "出来事",
];

/// 出力から、消しても文が壊れない見出し・ラベルを消す。戻り値の2つ目は消した数（ログ用）。
///
/// - 太字だけの短い行（`**要約**`）は行ごと消す。
/// - 行頭の太字のラベル（`**中心となる出来事の要約:**`）・ラベル（`要約：`・`【ゆうこの一言】`）は、その部分だけ消す。
/// - 残った太字の記号 `**` は消す（画面では Markdown として表示しないため、記号がそのまま出る）。
///
/// 消した結果がすべて空になるときは、元の文をそのまま返す（保存前の検証が空・見出しとして拒否する）。
/// `#` で始まる行は消さない（保存前の検証が見出しとして出力ごと拒否する。§12.5 の従来の決まり）。
pub(super) fn strip_heading_labels(text: &str) -> (String, usize) {
    let mut removed = 0;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let (kept, count) = strip_heading_line(line);
        removed += count;
        if let Some(kept) = kept {
            lines.push(kept);
        }
    }
    if removed == 0 {
        return (text.to_string(), 0);
    }
    let joined = lines.join("\n");
    if joined.trim().is_empty() {
        return (text.to_string(), 0);
    }
    (joined.trim().to_string(), removed)
}

/// 1行の見出し・ラベルを消す。行ごと消すときは None。
fn strip_heading_line(line: &str) -> (Option<String>, usize) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return (Some(String::new()), 0);
    }
    // `#` で始まる行は消さない。記事Markdownの見出しと衝突しうるので、保存前の検証（§12.5）が出力ごと拒否する
    // （書き換えて残すより丸ごと捨てる、の従来の決まりを変えない）。
    if trimmed.starts_with('#') {
        return (Some(line.to_string()), 0);
    }
    // `**# 要約**` は太字の記号だけを外し、行頭の `#` を残す（§12.5 の検証が出力ごと拒否する）。
    if let Some(rest) = trimmed.strip_prefix("**") {
        if rest.trim_start().starts_with('#') {
            return (Some(trimmed.replace("**", "")), 1);
        }
    }
    let mut count = 0;
    let mut current = trimmed.to_string();
    // 行頭の太字。
    if let Some(rest) = current.strip_prefix("**") {
        if let Some(close) = rest.find("**") {
            let inner = rest[..close].trim();
            let after = rest[close + 2..].trim_start();
            let after_colon = after.strip_prefix([':', '：']);
            let inner_is_label =
                inner.ends_with([':', '：']) || after_colon.is_some() || after.is_empty();
            if inner_is_label && is_heading_body(inner) {
                let after = after_colon.unwrap_or(after).trim_start();
                if after.is_empty() {
                    return (None, 1);
                }
                current = after.to_string();
                count += 1;
            }
        }
    }
    // 行頭のラベル（「要約：」「【ゆうこの一言】」）。
    if let Some(rest) = strip_leading_label(&current) {
        if rest.is_empty() {
            return (None, count + 1);
        }
        current = rest.to_string();
        count += 1;
    }
    if current.contains("**") {
        current = current.replace("**", "");
        count += 1;
    }
    if count == 0 {
        return (Some(line.to_string()), 0);
    }
    (Some(current.trim().to_string()), count)
}

/// 見出しの中身として短く、文の終わり（「。」）を持たないか。
fn is_heading_body(body: &str) -> bool {
    let body = body.trim().trim_end_matches([':', '：']).trim();
    !body.is_empty() && body.chars().count() <= HEADING_MAX_CHARS && !body.contains('。')
}

/// 文の頭のラベル（「〜の要約:」「【ゆうこの一言】」）を除いた残りを返す。ラベルがなければ None。
fn strip_leading_label(text: &str) -> Option<&str> {
    let text = text.trim_start();
    if let Some(rest) = text.strip_prefix('【') {
        let close = rest.find('】')?;
        let inner = &rest[..close];
        if is_label(inner) {
            return Some(rest[close + '】'.len_utf8()..].trim_start());
        }
        return None;
    }
    let (index, colon) = text
        .char_indices()
        .take(HEADING_MAX_CHARS + 1)
        .find(|(_, c)| matches!(c, ':' | '：'))?;
    let label = &text[..index];
    is_label(label).then(|| text[index + colon.len_utf8()..].trim_start())
}

/// ラベルの語か（読点・句点・括弧を含まず、`LABEL_TAIL_WORDS` のどれかで終わる）。
/// 話し手の言葉にも使う語は、語だけか、ちょうど「自分を指す前置き（の）＋語」のときだけ（`SPEAKER_LABEL_WORDS`）。
fn is_label(label: &str) -> bool {
    let label = label.trim();
    if label.is_empty() || label.contains(['。', '、', '「', '」', '(', '（']) {
        return false;
    }
    LABEL_TAIL_WORDS
        .iter()
        .filter(|word| label.ends_with(**word))
        .any(|word| {
            !SPEAKER_LABEL_WORDS.contains(word)
                || label == *word
                || SELF_LABEL_PREFIXES.iter().any(|prefix| {
                    label == format!("{prefix}{word}") || label == format!("{prefix}の{word}")
                })
        })
}

/// 消せずに残った見出し（文の途中のラベル・太字・`#` の行）を探す。
pub(super) fn find_heading_residue(text: &str) -> Option<CheckIssue> {
    let has_markdown = text.contains("**") || text.lines().any(|line| line.trim().starts_with('#'));
    let has_label = text
        .split(['。', '\n', '！', '？'])
        .any(|segment| strip_leading_label(segment).is_some());
    (has_markdown || has_label).then(|| CheckIssue {
        kind: IssueKind::Heading,
        reason: "見出し・ラベル（「要約:」や「**」など）が混ざっていた。見出しを付けず、本文だけを書く。"
            .to_string(),
    })
}

// ---------------------------------------------------------------------------
// 反対の意味
// ---------------------------------------------------------------------------

/// 反対の意味の語の組の片側。`terms` は出力で探す語（材料では反対の語の証拠にもなる）、
/// `also` は材料にあれば「この側の内容が材料にもある」とみなす、より広い言い方。
struct Side {
    terms: &'static [&'static str],
    also: &'static [&'static str],
}

/// 反対の意味の語の組。`a_only` が真の組は、出力が a・材料が b のときだけ報告する
/// （「検討」と書いた要約が「決定」の記事を言い過ぎていないのと同じく、逆向きは誤りになりにくいため）。
struct OppositePair {
    a: Side,
    b: Side,
    a_only: bool,
    /// 真の組（勝敗）は、出力の語と材料の反対の語が同じ主語について言っているときだけ報告する（`same_subject`）。
    subject_bound: bool,
}

/// 反対の意味の語の組（D170）。D169 の実記事の誤り（サヨナラ負け → 勝ち、検討中の案 → 決まった）と、
/// ニュースでよく逆にされる組を入れた。語を足すときは、誤検出の評価（要約の機械点検の評価 §9）をやり直す。
const OPPOSITE_PAIRS: &[OppositePair] = &[
    // 勝敗。材料の側は「勝」「敗」「負」の1字でもあれば、その側があるとみなす（「決勝」「勝負」も含めて控えめに）。
    // 「大勝」「快勝」「接戦を制し」「大敗」「屈し」「苦杯」などは Firestore タスク R7cWic で足した（評価 §9.11）。
    // 出力の語・材料の反対の語としては記録だけ（`RECORD_ONLY_TERMS`）で、材料の側の言い方には使う。
    // 「下し」「破っ」は入れない（「判断を下し」「記録を破っ」「均衡を破っ」〔先制の意味〕で誤検出しやすいため）。
    // 「制し」は試合・大会の語の付いた形だけ（「中盤を制し」「空中戦を制し」は試合の勝ちではないため）。
    OppositePair {
        a: Side {
            terms: &[
                "勝利",
                "勝ち",
                "勝っ",
                "白星",
                "連勝",
                "優勝",
                "大勝",
                "圧勝",
                "快勝",
                "完勝",
                "辛勝",
                "雪辱",
                "試合を制し",
                "接戦を制し",
                "熱戦を制し",
                "激戦を制し",
                "延長戦を制し",
                "攻防戦を制し",
                "打ち合いを制し",
                "大会を制し",
                "シリーズを制し",
            ],
            also: &["勝"],
        },
        b: Side {
            terms: &[
                "敗れ", "敗戦", "敗北", "負け", "黒星", "惜敗", "完敗", "連敗", "敗退", "大敗",
                "惨敗", "零敗", "屈し", "苦杯",
            ],
            also: &["敗", "負"],
        },
        a_only: false,
        subject_bound: true,
    },
    OppositePair {
        a: Side {
            terms: &["増加", "増え", "増や", "増大", "急増", "倍増"],
            also: &["増", "伸び", "拡大", "上回", "多く", "多い", "倍"],
        },
        b: Side {
            terms: &["減少", "減っ", "減ら", "減る", "急減", "半減", "縮小"],
            also: &["減", "下回", "低下", "落ち込", "少な"],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["上昇", "値上がり", "高騰", "急騰"],
            also: &["上が", "上げ", "高く", "高ま", "増"],
        },
        b: Side {
            terms: &["下落", "値下がり", "急落", "暴落"],
            also: &["下が", "下げ", "安く", "低", "減"],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["増収"],
            also: &[],
        },
        b: Side {
            terms: &["減収"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["増益"],
            also: &[],
        },
        b: Side {
            terms: &["減益"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["黒字"],
            also: &[],
        },
        b: Side {
            terms: &["赤字"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["値上げ"],
            also: &["引き上げ"],
        },
        b: Side {
            terms: &["値下げ"],
            also: &["引き下げ"],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["上方修正"],
            also: &[],
        },
        b: Side {
            terms: &["下方修正"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["賛成"],
            also: &["賛", "支持"],
        },
        b: Side {
            terms: &["反対"],
            also: &["反発", "否定"],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["可決"],
            also: &["成立", "承認"],
        },
        b: Side {
            terms: &["否決"],
            also: &["廃案"],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["有罪"],
            also: &[],
        },
        b: Side {
            terms: &["無罪"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    // 判決・裁判の語の組（D175 のタスク。D167 の評価で、訴え〔権利〕を退けた判決を一言が「権利が認められた」と
    // 逆に書いた誤りが両版で 4 件あった）。選んだ理由は 要約の機械点検の評価 §9.6。
    // 勝訴⇔敗訴は、勝敗と同じく主語を見る（「原告が敗訴」を「国が勝訴」と言い換えた出力を誤検出しないため）。
    OppositePair {
        a: Side {
            terms: &["勝訴"],
            also: &[],
        },
        b: Side {
            terms: &["敗訴"],
            also: &[],
        },
        a_only: false,
        subject_bound: true,
    },
    OppositePair {
        a: Side {
            terms: &["合憲"],
            also: &[],
        },
        b: Side {
            terms: &["違憲"],
            also: &["憲法違反", "憲法に違反"],
        },
        a_only: false,
        subject_bound: false,
    },
    // 「不適法」「非合法」は、直前が「不」「非」なので適法・合法の側には数えず（打ち消しの規則）、違法の側の語として数える。
    OppositePair {
        a: Side {
            terms: &["適法", "合法"],
            also: &[],
        },
        b: Side {
            terms: &["違法", "不適法", "非合法"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["有効"],
            also: &[],
        },
        b: Side {
            terms: &["無効"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    // 「不起訴」の中の「起訴」は直前が「不」なので起訴の側に数えない。起訴猶予は不起訴の一種なので不起訴の側に入れ、
    // 中の「起訴」は直後の「猶予」（打ち消しの続き）で起訴の側に数えない（材料が「起訴猶予処分」の記事で、
    // 出力の「起訴された」を拾うため）。「起訴を見送」は不起訴の側。
    OppositePair {
        a: Side {
            terms: &["起訴"],
            also: &[],
        },
        b: Side {
            terms: &["不起訴", "起訴猶予", "起訴を見送"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    // 訴え・請求を認めた⇔退けた。「認め」「退け」「棄却」だけでは、誰の訴え・主張か（「国の主張を退け」は原告の
    // 勝ち）で意味が逆になるため、出力の語・材料の反対の語は、訴えた側の語（訴え・請求・申し立て・権利）に限る。
    // 材料の側は「認め」「棄却」などの1語でもあれば、その側があるとみなす（「請求を一部認め」の記事で、
    // 「請求を認めた」と書いた出力を誤検出しないため。控えめな側に倒す）。
    OppositePair {
        a: Side {
            terms: &[
                "訴えを認め",
                "訴えが認められ",
                "訴えは認められ",
                "請求を認め",
                "請求が認められ",
                "請求は認められ",
                "申し立てを認め",
                "申し立てが認められ",
                "申し立ては認められ",
                "権利を認め",
                "権利が認められ",
                "権利は認められ",
            ],
            also: &["認め", "容認", "認容", "勝訴"],
        },
        b: Side {
            terms: &[
                "訴えを退け",
                "訴えが退けられ",
                "訴えは退けられ",
                "訴えを棄却",
                "訴えを却下",
                "訴えが却下",
                "請求を退け",
                "請求が退けられ",
                "請求は退けられ",
                "請求を棄却",
                "請求が棄却",
                "請求は棄却",
                "請求をいずれも棄却",
                "請求をすべて棄却",
                "請求を全て棄却",
                "申し立てを退け",
                "申し立てを却下",
                "申し立てが却下",
                "申し立ては却下",
            ],
            also: &["退け", "棄却", "却下", "敗訴"],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["当選"],
            also: &[],
        },
        b: Side {
            terms: &["落選"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["成功"],
            also: &[],
        },
        b: Side {
            terms: &["失敗"],
            also: &[],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["開始", "始まっ", "始まる", "スタート"],
            also: &["始ま", "始め", "開幕", "開業"],
        },
        b: Side {
            terms: &["終了", "終わっ", "終わる", "打ち切"],
            also: &["終わ", "終え", "閉幕", "廃止", "締め切", "締切"],
        },
        a_only: false,
        subject_bound: false,
    },
    OppositePair {
        a: Side {
            terms: &["再開"],
            also: &[],
        },
        b: Side {
            terms: &["停止", "中止", "中断", "休止", "見合わせ"],
            also: &["止め", "止ま"],
        },
        a_only: false,
        subject_bound: false,
    },
    // 決定と、検討・案・方針・主張。出力が「決まった」で、材料が検討・案の段階のときだけ報告する。
    OppositePair {
        a: Side {
            terms: &["決定", "決まっ", "決まり", "決まる", "決めた", "確定"],
            also: &["決ま", "決め", "成立", "可決", "承認", "合意", "締結"],
        },
        b: Side {
            terms: &[
                "検討", "方針", "提案", "素案", "原案", "試案", "主張", "協議", "構想", "案を",
                "案に", "案は", "案が", "案で", "案と", "案の",
            ],
            also: &[],
        },
        a_only: true,
        subject_bound: false,
    },
];

/// 語の一部が別の意味になる複合語（反対の意味の語として数えない）。
/// 「選挙無効」「解雇無効」「特許無効」「無効確認」「無効を求め」「無効の請求」は訴えの名前・求めで、判決の結論ではない
/// （一票の格差の訴訟で、出力の「選挙は有効とされた」を誤検出しないため）。「無効化」は「攻撃を無効化」のように防ぐ意味。
/// 「勝利投手」「敗戦投手」などは投手の記録の名前（負けたチームの記事にも勝利投手が出る）、「優勝候補」「優勝争い」「勝利数」は
/// 結果ではない言い方・数の名前、「機先を制」は「先手を取る」の意味、「準優勝」は決勝で負けたこと（Firestore タスク R7cWic）。
const MASKED_COMPOUNDS: [&str; 29] = [
    "勝ち点",
    "勝ち越",
    "勝負",
    "決勝",
    "負担",
    "負傷",
    "負債",
    "反対側",
    "選挙無効",
    "解雇無効",
    "特許無効",
    "無効確認",
    "無効を求め",
    "無効の請求",
    "無効化",
    "勝利投手",
    "勝ち投手",
    "敗戦投手",
    "負け投手",
    "優勝候補",
    "優勝争い",
    "機先を制",
    "準優勝",
    "勝利数",
    "勝ち筋",
    "勝利の方程式",
    "勝ち気",
    "負けん気",
    "負け越",
];

/// 語の直後がこれで始まれば打ち消し（「負けなかった」「決めておらず」「決まっていません」）とみなして数えない。
/// 「な」1字にしないのは、「増加など」「増加しながら」を打ち消しと読まないため。
/// 判決の言い方の「違法性はない」「違法とまではいえない」と、求めただけでまだ起きていない
/// 「請求を退けるよう求めた」「権利を認めることを求めた」、不起訴の一種の「起訴猶予」も数えない（判決の語の組。§9.6）。
/// 「勝ち切れなかった」「勝ちきれず」と、願い・仮定の「勝ちたい」「勝ってほしい」「負けたくない」「勝ってたら」も数えない
/// （Firestore タスク R7cWic）。
const NEGATING_FOLLOWERS: [&str; 66] = [
    "ない",
    "なかっ",
    "なく",
    "ならな",
    "なるか",
    "ず",
    "じと",
    "せず",
    "しない",
    "しなかっ",
    "しなく",
    "しません",
    "されず",
    "されな",
    "されません",
    "ません",
    "ておらず",
    "ていない",
    "ていなかっ",
    "ていません",
    "てない",
    "てなかっ",
    "てず",
    "てはいな",
    "てはいません",
    "てはおらず",
    "ではな",
    "とはな",
    "はない",
    "はなかっ",
    "はなく",
    "ならず",
    "できず",
    "できな",
    "性はな",
    "性がな",
    "とはいえな",
    "とは言えな",
    "とまではいえな",
    "とまでは言えな",
    "るよう求め",
    "るよう訴え",
    "るよう主張",
    "ることを求め",
    "猶予",
    "切れな",
    "切れず",
    "きれな",
    "きれず",
    "たい",
    "たく",
    "てほし",
    "たか",
    "たら",
    "てたら",
    "ていたら",
    "ていれば",
    "てれば",
    "て当然",
    "られな",
    "なし",
    "抜けば",
    "て当たり前",
    "したい",
    "したく",
    "してほし",
];

/// 語の直後がこれで始まれば、打ち消しと同じく数えない（「勝利を逃した」「連敗を脱した」「決定を見送った」）。
/// ただし、その後ろがさらに打ち消し（`DOUBLE_NEGATING_TAILS`）なら二重の打ち消しで、語の意味のまま
/// （「勝ちを逃さなかった」「勝利を逃さず」「連敗を止められなかった」は数える）。
/// 「勝利を献上」「勝利を目指した」「勝利に向けて」「勝利目前」「勝ちから遠ざかる」も結果の勝ちではないので数えない
/// （Firestore タスク R7cWic。勝敗以外の組でも、目指す・向ける語は結果を言っていない）。
const REVERSING_FOLLOWERS: [&str; 26] = [
    "を逃",
    "を許",
    "を止め",
    "を脱",
    "を阻",
    "を見送",
    "を献上",
    "を譲",
    "をストップ",
    "から遠ざか",
    "に見放",
    "を目指",
    "を狙",
    "に向け",
    "を誓",
    "目前",
    "まであと",
    "を期",
    "を願",
    "を祈",
    "を信じ",
    "が見え",
    "の予感",
    "を持っていか",
    "をさらわ",
    "はお預",
];

/// 連勝・連敗の記録が止まったことを言う語（「連勝が7で止まった」「連勝ストップ」「連敗が途切れた」）。
/// `STREAK_TERMS` の語の後ろの、読点・句点までの 8 字以内にこれがあれば、その語を数えない
/// （「連勝が止まった」は負けの意味で、勝ちと読まないため。Firestore タスク R7cWic）。
const STREAK_TERMS: [&str; 2] = ["連勝", "連敗"];
const STREAK_ENDINGS: [&str; 6] = ["止ま", "止め", "ストップ", "途切", "阻ま", "阻ん"];
const STREAK_WINDOW_CHARS: usize = 8;

/// 連勝・連敗の語（`term`）の直後（`after`）が、記録が止まったことを言っているか。
fn streak_ended(term: &str, after: &str) -> bool {
    if !STREAK_TERMS.contains(&term) {
        return false;
    }
    let window: String = after
        .chars()
        .take_while(|c| !matches!(c, '。' | '、' | '\n' | '！' | '？'))
        .take(STREAK_WINDOW_CHARS)
        .collect();
    STREAK_ENDINGS.iter().any(|ending| window.contains(ending))
}

/// `REVERSING_FOLLOWERS` の後ろの打ち消し（「逃さなかった」「逃さず」「止めなかった」「見送らなかった」）。
const DOUBLE_NEGATING_TAILS: [&str; 14] = [
    "さなかっ",
    "さない",
    "さず",
    "さなく",
    "しません",
    "らなかっ",
    "らない",
    "らず",
    "られなかっ",
    "なかっ",
    "ない",
    "ず",
    "ません",
    "ませんでした",
];

/// 語＋（は・も）＋「し」「され」の後がこれで始まれば打ち消し（「決定していない」「決定されておらず」
/// 「増加はしていない」「開始しなかった」「決定していません」「開始しませんでした」）。
const VERB_NEGATING_TAILS: [&str; 14] = [
    "ていない",
    "ていなかっ",
    "ていません",
    "ておらず",
    "てはいな",
    "てはいません",
    "てはおらず",
    "てない",
    "てなかっ",
    "ない",
    "なかっ",
    "なく",
    "ません",
    "ず",
];

/// 語＋これ＋`REACH_NEGATING_TAILS` は打ち消し（「決定には至らなかった」「可決に至っていない」）。
const REACH_LEADS: [&str; 2] = ["には至", "に至"];
const REACH_NEGATING_TAILS: [&str; 4] = ["ら", "っていな", "っておらず", "ってはいな"];

/// 語の直前がこれなら打ち消し（「未定」「不成立」「無敗」）とみなして数えない。
const NEGATING_PREFIXES: [char; 4] = ['未', '不', '非', '無'];

/// 反対の意味の点検。`source` は材料（記事のタイトル＋本文抜粋）、`output` は点検する出力。
pub(super) fn find_contradictions(source: &str, output: &str) -> Vec<CheckIssue> {
    contradiction_findings(source, output)
        .into_iter()
        .filter(|finding| !finding.recorded_only)
        .map(|finding| finding.issue)
        .collect()
}

/// 反対の意味の1件。`recorded_only` が true のものは記録だけ（評価で数える）で、`find_contradictions` は返さない。
/// Firestore タスク R7cWic で足した勝敗の言い方（`RECORD_ONLY_TERMS`）と、勝敗の両方の側を書いた出力の中の
/// 入れ替え（`swapped_subject`）は、架空の意地悪な文で直しきれない誤検出（別の試合・前の試合の勝ち、主語を省いた
/// 負けの文）があったため記録だけにした（評価 §9.11）。
#[derive(Debug, Clone)]
pub(super) struct ContradictionFinding {
    pub issue: CheckIssue,
    pub recorded_only: bool,
}

/// 反対の意味を探す（`find_contradictions` の本体。評価では記録だけのものも数える）。
/// 組ごとに、まず作り直しの理由にする規則（`strict`）で探し、無ければ記録だけの規則でも探す。
pub(super) fn contradiction_findings(source: &str, output: &str) -> Vec<ContradictionFinding> {
    let source = mask_compounds(source);
    let output = mask_compounds(output);
    let mut findings = Vec::new();
    for pair in OPPOSITE_PAIRS {
        for strict in [true, false] {
            let found =
                contradiction_on(pair, &pair.a, &pair.b, &source, &output, strict).or_else(|| {
                    (!pair.a_only)
                        .then(|| contradiction_on(pair, &pair.b, &pair.a, &source, &output, strict))
                        .flatten()
                });
            if let Some(issue) = found {
                findings.push(ContradictionFinding {
                    issue,
                    recorded_only: !strict,
                });
                break;
            }
        }
    }
    findings
}

/// Firestore タスク R7cWic で足した勝敗の言い方。出力の語・材料の反対の語としては記録だけに使う
/// （「Aは1回戦でBに快勝した」のように、記事の別の試合の勝ち負けを書いた出力を、今の規則では見分けられないため。
/// 従来の「勝っ」「勝利」でも同じ誤検出は起こるが、語を足すと誤検出の機会が増える）。材料の側の言い方
/// （「材料にもある」の判定）と、出力が両方の側を言っているかの判定には使う（どちらも報告を減らす向き）。
const RECORD_ONLY_TERMS: [&str; 20] = [
    "大勝",
    "圧勝",
    "快勝",
    "完勝",
    "辛勝",
    "雪辱",
    "試合を制し",
    "接戦を制し",
    "熱戦を制し",
    "激戦を制し",
    "延長戦を制し",
    "攻防戦を制し",
    "打ち合いを制し",
    "大会を制し",
    "シリーズを制し",
    "大敗",
    "惨敗",
    "零敗",
    "屈し",
    "苦杯",
];

/// 出力が `said` の側を言い、材料は `said` の側を言わず `opposite` の側だけを言うとき、その問題を返す。
/// 出力が両方の側を言っているとき（「停止していた運転を再開した」）は報告しない。ただし勝敗の組
/// （`subject_bound`）は、記録だけの規則（`strict` が false）で、主語ごとに勝敗を入れ替えていないかを見る
/// （`swapped_subject`。Firestore タスク R7cWic）。`strict` が true のときは、出力の語・材料の反対の語に
/// `RECORD_ONLY_TERMS` を使わない。
fn contradiction_on(
    pair: &OppositePair,
    said: &Side,
    opposite: &Side,
    source: &str,
    output: &str,
    strict: bool,
) -> Option<CheckIssue> {
    let said_terms = checked_terms(said, strict);
    let opposite_terms = checked_terms(opposite, strict);
    let said_term = said_terms
        .iter()
        .find(|term| occurs_affirmed(output, term))?;
    // 両方の側の判定には記録だけの語も使う（報告を減らす向き）。
    let output_says_both = opposite
        .terms
        .iter()
        .any(|term| occurs_affirmed(output, term));
    if output_says_both && (strict || !pair.subject_bound) {
        return None;
    }
    let source_says_it = said
        .terms
        .iter()
        .chain(said.also.iter())
        .any(|term| occurs_affirmed(source, term));
    if source_says_it {
        return None;
    }
    let opposite_term = opposite_terms
        .iter()
        .find(|term| occurs_affirmed(source, term))?;
    if output_says_both {
        if !swapped_subject(&said_terms, &opposite_terms, source, output) {
            return None;
        }
    } else if pair.subject_bound && !same_subject(&said_terms, &opposite_terms, source, output) {
        return None;
    }
    Some(CheckIssue {
        kind: IssueKind::Contradiction,
        reason: format!(
            "「{said_term}」と書いたが、記事情報は「{opposite_term}」で、反対の意味になっていた。記事情報のとおりに書く。"
        ),
    })
}

/// 点検に使う語（`strict` のときは `RECORD_ONLY_TERMS` を除く）。
fn checked_terms(side: &Side, strict: bool) -> Vec<&'static str> {
    side.terms
        .iter()
        .copied()
        .filter(|term| !strict || !RECORD_ONLY_TERMS.contains(term))
        .collect()
}

/// 勝敗の組（`subject_bound`）で、出力の語と材料の反対の語が同じ主語（チーム・人の名前）について言っているか。
/// 「AがBを下した」記事を「BはAに敗れた」と言い換えた出力（主語の入れ替え）を、反対の意味と誤検出しないため。
/// - 主語の候補は、語の前で最も近い「は」「が」の直前の名前と、文の最初の「〜は」の名前（`subjects_at`。
///   「Aは打線がつながらず敗れた」では「打線」と「A」）。
/// - 出力の語に主語の候補が無いとき（「サヨナラ勝ちしたんだね」）は、記事と同じ主語の話とみなす
///   （一言・再説明は主語を省くことが多いため）。
/// - 材料の反対の語に主語の候補があれば名前どうしを（片方がもう片方を含めば同じとみなす）、無ければその語の
///   ある文に出力の主語が出てくるかで比べる。
/// - 語の直前の「Aの」（「Aのサヨナラ勝ち」「相手の勝利」。`possessive_subject_before`）があれば、その A も記事の
///   主語と同じときだけ報告する（Firestore タスク R7cWic。「Bは、相手の勝利で…」の誤検出を減らす。報告を減らす向き
///   だけに使い、ほかの候補が合わないのに A だけで報告することはしない）。
fn same_subject(said: &[&str], opposite: &[&str], source: &str, output: &str) -> bool {
    let same = |a: &str, b: &str| a.contains(b) || b.contains(a);
    // 材料の反対の語の主語に、出力の主語が当たるか。
    let matches_source = |subject: &str| {
        opposite.iter().any(|term| {
            affirmed_positions(source, term).any(|index| {
                let source_subjects = subjects_at(source, index);
                if source_subjects.is_empty() {
                    sentence_around(source, index).contains(subject)
                } else {
                    source_subjects
                        .iter()
                        .any(|source_subject| same(source_subject, subject))
                }
            })
        })
    };
    said.iter().any(|term| {
        affirmed_positions(output, term).any(|index| {
            let subjects = output_subjects_at(output, index, term);
            let matched = subjects.is_empty() || subjects.iter().any(|s| matches_source(s));
            matched
                && possessive_subject_before(output, index)
                    .is_none_or(|owner| matches_source(&owner))
        })
    })
}

/// 勝敗の両方の側を書いた出力（「Aは勝ち、Bは負けた」）で、勝敗を入れ替えたか（Firestore タスク R7cWic）。
/// 記録だけの規則（`contradiction_on` の `strict` が false）でだけ使う（「Aは前日Bに勝った。この日はCに敗れた。」の
/// ように負けの文の主語を省いた正しい出力も真になるため）。材料が `opposite` の側だけを言う記事（`contradiction_on` で確かめ済み）で、次のときに真:
/// - 出力の `said` の語に主語の候補（`subjects_at`）があり、その主語には出力が `opposite` の語を言っていない
///   （「Aは勝利を目指したが、敗れた」のように、同じ主語の勝ちと負けを両方書いた出力は見ない）。
///   出力の `opposite` の語の主語は、文の最初の「〜は」も含めて広く取る（「Aは前回勝ったが、今回は負けた」の
///   「負け」の主語は「今回」と「A」）。そのため「Aは勝ち、Bは負けた」の 1 文の入れ替えは拾えず（「負け」の主語に
///   A も入る）、「Aが勝ち、Bは負けた」「Aが勝った。Bは負けた。」の形だけを拾う（誤検出を避ける側に倒す）。
/// - 材料の `opposite` の語の主語の候補に、その主語と同じ名前がある（材料の語に主語の候補が無ければ見ない）。
///
/// 主語の無い出力の語は見ない（両方の側を書いた出力で主語を省くと、どちらのチームの話か決められないため）。
fn swapped_subject(said: &[&str], opposite: &[&str], source: &str, output: &str) -> bool {
    let same = |a: &str, b: &str| a.contains(b) || b.contains(a);
    let opposite_in_output: Vec<String> = opposite
        .iter()
        .flat_map(|term| affirmed_positions(output, term))
        .flat_map(|index| subjects_at(output, index))
        .collect();
    let said_subjects: Vec<String> = said
        .iter()
        .flat_map(|term| affirmed_positions(output, term).map(move |index| (*term, index)))
        .flat_map(|(term, index)| output_subjects_at(output, index, term))
        .filter(|subject| !opposite_in_output.iter().any(|other| same(other, subject)))
        .collect();
    if said_subjects.is_empty() {
        return false;
    }
    opposite.iter().any(|term| {
        affirmed_positions(source, term).any(|index| {
            subjects_at(source, index).iter().any(|source_subject| {
                said_subjects
                    .iter()
                    .any(|subject| same(source_subject, subject))
            })
        })
    })
}

/// `index` の語の主語の候補: 語の前で最も近い「は」「が」の直前の名前と、同じ文の最初の「〜は」の名前（重複なし）。
fn subjects_at(text: &str, index: usize) -> Vec<String> {
    let mut subjects: Vec<String> = subject_before(text, index).into_iter().collect();
    let start = text[..index]
        .char_indices()
        .rev()
        .find(|(_, c)| matches!(c, '。' | '\n' | '！' | '？'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    if let Some(topic) = first_topic(&text[start..index]) {
        if !subjects.contains(&topic) {
            subjects.push(topic);
        }
    }
    subjects
}

/// 出力の語（`term`。`index` の位置）の主語の候補: `subjects_at` が空のときは、「Aのサヨナラ勝ち」の A
/// （`possessive_subject_before`）、それも無ければ「勝ったのはAだ」の A（`cleft_subject_after`）。
/// 出力の側だけに使う（主語の無い出力は記事と同じ主語とみなして報告するので、候補が増えると報告は減る向きになる）。
fn output_subjects_at(output: &str, index: usize, term: &str) -> Vec<String> {
    let mut subjects = subjects_at(output, index);
    if subjects.is_empty() {
        subjects.extend(possessive_subject_before(output, index));
    }
    if subjects.is_empty() {
        subjects.extend(cleft_subject_after(output, index + term.len()));
    }
    subjects
}

/// 語の前に主語が無い文の「Aのサヨナラ勝ち」「Aの勝利で」の A（Firestore タスク R7cWic）。語の直前の
/// 漢字・カタカナ・数字（「サヨナラ」「逆転」「3」など 6 字まで）の前の「の」の前の名前。
fn possessive_subject_before(text: &str, index: usize) -> Option<String> {
    let chars: Vec<char> = text[..index].chars().collect();
    let mut cursor = chars.len();
    while cursor > 0 && chars.len() - cursor < 6 && !is_hiragana(chars[cursor - 1]) {
        let c = chars[cursor - 1];
        if matches!(
            c,
            '。' | '、' | '\n' | '！' | '？' | '「' | '」' | ' ' | '　'
        ) {
            return None;
        }
        cursor -= 1;
    }
    if cursor < 2 || chars[cursor - 1] != 'の' {
        return None;
    }
    name_before(&chars, cursor - 1)
}

/// 語の前に主語が無い文の「〜したのはAだ」「〜ったのがA」の A（Firestore タスク R7cWic）。
/// ゆうこの話し言葉に多い形で、主語の無い文として記事と同じ主語とみなすと、勝ったのが相手のチームのときに
/// 誤検出するため。語の後ろ（`term_end`）から「のは」「のが」までがひらがなだけ（活用の語尾。6 字まで）のときだけ見る。
fn cleft_subject_after(text: &str, term_end: usize) -> Option<String> {
    let tail = &text[term_end..];
    let marker = ["のは", "のが"]
        .iter()
        .filter_map(|marker| tail.find(marker).map(|i| (i, marker.len())))
        .min()?;
    let between: Vec<char> = tail[..marker.0].chars().collect();
    if between.len() > 6 || !between.iter().all(|c| is_hiragana(*c)) {
        return None;
    }
    let name: String = tail[marker.0 + marker.1..]
        .chars()
        .take_while(|c| {
            !matches!(
                c,
                '、' | '。'
                    | '\n'
                    | '！'
                    | '？'
                    | ' '
                    | '　'
                    | '」'
                    | 'だ'
                    | 'で'
                    | 'よ'
                    | 'ね'
                    | 'の'
                    | 'に'
                    | 'を'
                    | 'が'
                    | 'は'
                    | 'も'
                    | 'な'
                    | 'さ'
                    | 'じ'
            )
        })
        .take(15)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// 文の頭から見て最初の「〜は」（「は」の前がひらがなでないもの）の名前。
fn first_topic(sentence_head: &str) -> Option<String> {
    let chars: Vec<char> = sentence_head.chars().collect();
    let position = (1..chars.len()).find(|&i| chars[i] == 'は' && !is_hiragana(chars[i - 1]))?;
    name_before(&chars, position)
}

/// `index` の前で最も近い主語の「は」「が」の直前の名前（同じ文の中だけ。「は」「が」の前がひらがなのときは
/// 「ながら」「では」などの一部とみなして飛ばす）。
fn subject_before(text: &str, index: usize) -> Option<String> {
    let before: Vec<char> = text[..index].chars().collect();
    let mut cursor = before.len();
    while cursor > 0 {
        cursor -= 1;
        let c = before[cursor];
        if matches!(c, '。' | '\n' | '！' | '？') {
            return None;
        }
        if !matches!(c, 'は' | 'が') || cursor == 0 || is_hiragana(before[cursor - 1]) {
            continue;
        }
        return name_before(&before, cursor);
    }
    None
}

/// `chars[particle]`（「は」「が」）の直前の名前。区切り（読点・括弧・空白）か、助詞「に・で・を・の・も・へ・は・が」
/// まで、最大 15 字。
fn name_before(chars: &[char], particle: usize) -> Option<String> {
    let mut start = particle;
    while start > 0
        && particle - start < 15
        && !matches!(
            chars[start - 1],
            '、' | '，'
                | ','
                | '。'
                | '\n'
                | ' '
                | '　'
                | '「'
                | '」'
                | '（'
                | '）'
                | '('
                | ')'
                | 'に'
                | 'で'
                | 'を'
                | 'の'
                | 'も'
                | 'へ'
                | 'は'
                | 'が'
        )
    {
        start -= 1;
    }
    let name: String = chars[start..particle].iter().collect();
    (!name.is_empty()).then_some(name)
}

/// `index` を含む文（「。」・改行で区切る）。
fn sentence_around(text: &str, index: usize) -> &str {
    let is_end = |c: char| matches!(c, '。' | '\n' | '！' | '？');
    let start = text[..index]
        .char_indices()
        .rev()
        .find(|(_, c)| is_end(*c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let end = text[index..]
        .char_indices()
        .find(|(_, c)| is_end(*c))
        .map_or(text.len(), |(i, _)| index + i);
    &text[start..end]
}

fn is_hiragana(c: char) -> bool {
    ('\u{3041}'..='\u{309f}').contains(&c)
}

/// 複合語（`MASKED_COMPOUNDS`）を、同じ文字数の記号に置き換える（語の点検で数えないため）。
/// 置き換える語も記号も UTF-8 で1字3バイトなので、置き換えの前後で位置（バイト）は変わらない。
fn mask_compounds(text: &str) -> String {
    let mut masked = text.to_string();
    for word in MASKED_COMPOUNDS {
        masked = masked.replace(word, &"＿".repeat(word.chars().count()));
    }
    masked
}

/// `term` が打ち消しでない形で `text` に出てくるか。
fn occurs_affirmed(text: &str, term: &str) -> bool {
    affirmed_positions(text, term).next().is_some()
}

/// `term` が打ち消しでない形で出てくる位置（バイト）。
fn affirmed_positions<'a>(text: &'a str, term: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(term)
        .map(|(index, _)| index)
        .filter(move |index| {
            let before = text[..*index].chars().next_back();
            let after = &text[*index + term.len()..];
            !before.is_some_and(|c| NEGATING_PREFIXES.contains(&c))
                && !is_negated_after(after)
                && !streak_ended(term, after)
        })
}

/// 語の直後（`after`）が打ち消しか。
/// - 直後の打ち消し（`NEGATING_FOLLOWERS`）。`REVERSING_FOLLOWERS`（「を逃」など）は、後ろがさらに打ち消し
///   （「逃さなかった」）なら打ち消しにしない。
/// - 語＋（は・も）＋「し」「され」＋打ち消し（`VERB_NEGATING_TAILS`。「決定していない」「決定されておらず」）。
/// - 語＋「には至」「に至」＋「ら」「っていない」（「決定には至らなかった」「決定には至っていない」）。
fn is_negated_after(after: &str) -> bool {
    if NEGATING_FOLLOWERS
        .iter()
        .any(|follower| after.starts_with(follower))
    {
        return true;
    }
    if let Some(follower) = REVERSING_FOLLOWERS
        .iter()
        .find(|follower| after.starts_with(**follower))
    {
        let rest = &after[follower.len()..];
        return !DOUBLE_NEGATING_TAILS
            .iter()
            .any(|tail| rest.starts_with(tail));
    }
    for particle in ["", "は", "も"] {
        let Some(rest) = after.strip_prefix(particle) else {
            continue;
        };
        for verb in ["され", "し"] {
            if let Some(tail) = rest.strip_prefix(verb) {
                if VERB_NEGATING_TAILS.iter().any(|t| tail.starts_with(t)) {
                    return true;
                }
            }
        }
    }
    REACH_LEADS.iter().any(|lead| {
        after
            .strip_prefix(lead)
            .is_some_and(|rest| REACH_NEGATING_TAILS.iter().any(|t| rest.starts_with(t)))
    })
}

// ---------------------------------------------------------------------------
// 推測・伝聞・訴えの言い切り
// ---------------------------------------------------------------------------

/// 出力で、出来事の語（漢字・カタカナの語）の直後にこれが続けば、その出来事が起きたと言い切っているとみなす
/// （「流出した」「流出している」「無償化が決まった」）。「したら」「したい」は除く（`asserting_tail_at`）。
const ASSERTING_TAILS: [&str; 28] = [
    "した",
    "された",
    "させた",
    "している",
    "していた",
    "していま",
    "されている",
    "されていた",
    "されていま",
    "させている",
    "させていた",
    "しまし",
    "されまし",
    "させまし",
    "しちゃっ",
    "されちゃっ",
    "が決まっ",
    "が決まりまし",
    "が確定し",
    "が実現し",
    "が成立し",
    "が判明し",
    "が発生し",
    "が起きた",
    "が起きてい",
    "が起きまし",
    "があった",
    "がありまし",
];

/// 材料で、出来事の語の直後（同じ区切りの中・`HEDGE_WINDOW_CHARS` 字以内）にこれがあれば、その語は推測・伝聞・
/// 訴え・主張として書かれているとみなす（「流出した可能性」「感染したとみられる」「無償化を主張」「減税を訴え」
/// 「増えたとしている」「盗んだとして」）。出力の文にあれば、言い切っていないとみなす（`OUTPUT_ONLY_HEDGES` も）。
/// 「〜したという」は入れない: 記者が取材で確かめた事実を書く普通の言い方で、「送付したという」を「送付した」と
/// 書いた要約を誤りとしないため（実記事の評価で誤検出になった。要約の機械点検の評価 §9.7）。
const HEDGES: [&str; 32] = [
    "可能性",
    "かもしれ",
    "とみられ",
    "と見られ",
    "とみて",
    "との見方",
    "疑い",
    "恐れ",
    "おそれ",
    "見通し",
    "見込み",
    "とされ",
    "といわれ",
    "と言われ",
    "と報じ",
    "と訴え",
    "を訴え",
    "と主張",
    "を主張",
    "と求め",
    "を求め",
    "よう求め",
    "と要求",
    "を要求",
    "と指摘",
    "と批判",
    "と非難",
    "としてい",
    "とした",
    "とする",
    "たとして",
    "るとして",
];

/// 出力の文にあれば、言い切っていない（推測・伝聞・人の発言として書いた・問いかけ）とみなす言い方。
/// 材料では見ない（「によって」の「って」、「ような」の「よう」のように、言い切りの文にも出るため）。
/// 「求め」は「〜することが求められた」のように、求めの形のまま書いた出力を言い切りとしないため。
/// 「疑われ」「だとして」は、送り仮名の動詞の形（「車を盗んだと疑われている」「盗んだとして逮捕」）を
/// 言い切りとしないため（Firestore タスク RWW00G）。
const OUTPUT_ONLY_HEDGES: [&str; 36] = [
    "という",
    "疑われ",
    "だとして",
    "求め",
    "かも",
    "だろう",
    "でしょう",
    "らしい",
    "ようだ",
    "ようです",
    "みたい",
    "そうだ",
    "そうです",
    "って",
    "かな",
    "かどうか",
    "によると",
    "によれば",
    "と発表",
    "と述べ",
    "と説明",
    "と話",
    "と語",
    "と伝え",
    "と明らか",
    "と明か",
    "と認め",
    "と強調",
    "と報告",
    "と答え",
    "と判断",
    "とのこと",
    "と見て",
    "わけではな",
    "とは限らな",
    "?",
];

/// 起きたことではなく、考え・つもり・ようす見を表す語。「検討している」「調査した」と書いても、記事の「検討するとしている」
/// と食い違う出来事を言い切ったことにはならないので見ない（架空の判決記事の評価で「控訴を検討しているとしている」を
/// 「控訴を検討している」と書いた注目ポイントが誤検出になった）。
const NON_EVENT_WORDS: [&str; 22] = [
    "検討", "予定", "計画", "方針", "協議", "調査", "調整", "準備", "模索", "要望", "要求", "主張",
    "説明", "発表", "表明", "期待", "懸念", "注目", "予想", "想定", "警戒", "注意",
];

/// 材料で、出来事の語の後ろの推測・伝聞の言い方（`HEDGES`）を探す字数。語から遠い言い方は、別の語に
/// かかることが多いので見ない（その出現は言い切りとみなし、報告しない側に倒す）。
const HEDGE_WINDOW_CHARS: usize = 12;

/// 推測・伝聞・訴えの言い切りの点検（`IssueKind::AssertedSpeculation`）。
///
/// 材料（タイトル＋本文抜粋）で、推測・伝聞・訴え・主張の形（「流出した可能性」「無償化を主張」）でだけ出てくる
/// 出来事の語を、出力が打ち消し・推測・伝聞の印なしに「〜した」「〜が決まった」と言い切ったときに報告する。
/// 誤検出を減らすため（報告しない側に倒す）:
/// - 出力の語は、言い切りの続き（`ASSERTING_TAILS`）の直前の漢字・カタカナの語（2字以上）。材料にある最も長い
///   後ろの部分（「情報流出」なら「情報流出」→「流出」）で照らす。材料に語が無ければ見ない（作り話は別の点検）。
/// - 材料のどこか1か所でも、その語が推測・伝聞の印なしに出てくれば（言い切り・名詞のままの見出しを含む）、報告しない。
///   打ち消された出現（「流出はしていない」）は、どちらにも数えない。
/// - 出力の文の、言い切った語より後ろに推測・伝聞・人の発言の言い方（`HEDGES`・`OUTPUT_ONLY_HEDGES`）があるか、
///   文全体にかかる言い方（「〜によると」）があるか、問いかけ（「？」「〜か」）で終わる文は、言い切っていないと
///   みなす（`output_assertion`）。語より前にだけある言い方（「…と発表し、顧客情報が流出した」）は別の出来事に
///   かかるとみなす（Firestore タスク RWW00G）。ただし、その言い方が読点をはさまずに主語・主題にかかる文
///   （「原因とみられるのは漏電だ」）は言い切っていないとみなす。語より前にだけ言い方がある文での言い切り（`mixed`）は
///   記録だけで、ここでは報告しない（架空の文で誤検出が続いたため）。
///
/// D175 の形（「〜した」「〜が決まった」）に加え、Firestore タスク RWW00G で次の形を足した（評価は要約の機械点検の
/// 評価 §9.7.7）。材料の側では、主張・訴え・「〜とした」は推測として数えない（`SPECULATION_HEDGES`）。
/// - 名詞・形容動詞の言い切り（`copula_findings`。材料「原因は漏電とみられる」→ 出力「原因は漏電だ」）。
/// - 送り仮名の動詞（`kana_verb_findings`。材料「車を盗んだとして逮捕」→ 出力「車を盗んだ」）。
/// - 確かになったことの言い方（`certainty_findings`。出力「原因は漏電と判明した」「漏電の可能性を断定した」）。
///
/// 記事の文と出力を照らす単位は語で、文と文の対応（保留ブランチの「文ごとの裏付け」の点検）は見ない。
pub(super) fn find_asserted_speculation(source: &str, output: &str) -> Vec<CheckIssue> {
    let mut issues: Vec<CheckIssue> = Vec::new();
    // 語より前にだけ推測・伝聞の言い方がある文（`mixed`）は記録だけにし、作り直しの理由にしない（架空の文で
    // 「可能性が高い原因は漏電だ」のような誤検出が続き、止める条件を後追いしきれなかったため。評価 §9.7.7）。
    for finding in speculation_findings(source, output)
        .into_iter()
        .filter(|finding| !finding.mixed)
    {
        let hedge = finding.hedge;
        let reason = format!(
            "記事情報で推測・伝聞・訴え（「{hedge}」など）として書かれた出来事を、起きたこととして言い切っていた。記事情報と同じく推測・伝聞・訴えとして書く。"
        );
        if !issues.iter().any(|issue| issue.reason == reason) {
            issues.push(CheckIssue {
                kind: IssueKind::AssertedSpeculation,
                reason,
            });
        }
    }
    issues
}

/// 推測の言い切りの形（評価の内訳に使う。どの形も同じ `IssueKind::AssertedSpeculation` として扱う）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpeculationForm {
    /// 漢字・カタカナの語＋「した」「が決まった」など（D175 の形）。
    Verb,
    /// 名詞・形容動詞＋「だ」「です」「である」（材料「原因は漏電とみられる」→ 出力「原因は漏電だ」）。
    Copula,
    /// 送り仮名のある動詞（材料「車を盗んだとして逮捕」→ 出力「車を盗んだ」）。
    KanaVerb,
    /// 「〜と判明した」「〜と断定した」「〜の可能性を断定した」。
    Certainty,
}

/// 推測の言い切りの1件。`mixed` が true のものは記録だけ（評価で数える）で、`find_asserted_speculation` は報告しない。
/// `mixed` は、出力の文の語より前にだけ推測・伝聞の言い方がある（D175 の版では文ごと
/// 見なかった）ときに true。`hedge` は材料の側の推測の言い方（理由に入れる固定の語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SpeculationFinding {
    pub form: SpeculationForm,
    pub mixed: bool,
    pub hedge: &'static str,
}

/// 推測の言い切りを形ごとに探す（`find_asserted_speculation` の本体）。
pub(super) fn speculation_findings(source: &str, output: &str) -> Vec<SpeculationFinding> {
    let mut findings = Vec::new();
    verb_findings(source, output, &mut findings);
    copula_findings(source, output, &mut findings);
    kana_verb_findings(source, output, &mut findings);
    certainty_findings(source, output, &mut findings);
    findings
}

/// D175 の形: 言い切りの続き（`ASSERTING_TAILS`）の直前の漢字・カタカナの語。
fn verb_findings(source: &str, output: &str, findings: &mut Vec<SpeculationFinding>) {
    for tail in ASSERTING_TAILS {
        for (index, _) in output.match_indices(tail) {
            if !asserting_tail_at(output, index + tail.len()) {
                continue;
            }
            let Some(word) = longest_source_word(source, &output[..index]) else {
                continue;
            };
            if is_non_event(word) {
                continue;
            }
            let Some(mixed) = output_assertion(output, index - word.len()) else {
                continue;
            };
            let Some(hedge) = hedge_only_in_source(source, word, &HEDGES, source_hedge_after)
            else {
                continue;
            };
            findings.push(SpeculationFinding {
                form: SpeculationForm::Verb,
                mixed,
                hedge,
            });
        }
    }
}

/// 名詞・形容動詞の言い切り（`SpeculationForm::Copula`）・「〜と判明した」（`SpeculationForm::Certainty`）・
/// 送り仮名の動詞（`SpeculationForm::KanaVerb`）で、材料の側の推測の言い方として数えるもの。主張・訴え・
/// 「〜とした」「〜としている」は入れない（「対象とした」「解雇は相当とはいえないとした」のように、推測でない
/// 書き方にも出て、名詞・ひらがなの語では見分けにくいため）。
const SPECULATION_HEDGES: [&str; 14] = [
    "可能性",
    "かもしれ",
    "とみられ",
    "と見られ",
    "とみて",
    "との見方",
    "疑い",
    "恐れ",
    "おそれ",
    "見通し",
    "見込み",
    "とされ",
    "といわれ",
    "と言われ",
];

/// 名詞・形容動詞の言い切りの続き。後ろの字で、言い切りでないもの（「だけ」「だろう」「だと」「であると」
/// 「であっても」）を除く（`copula_asserts`）。
const COPULA_TAILS: [&str; 5] = ["だ", "です", "でした", "である", "であっ"];

/// 名詞・形容動詞の言い切りか（`tail` の後ろが `rest`）。「だと」「であると」「であること」は「〜と判明」の形
/// （`certainty_findings`）か引用なので、ここでは見ない。
fn copula_asserts(tail: &str, rest: &str) -> bool {
    match tail {
        "だ" => !rest.starts_with(['け', 'ろ', 'と', 'ら']),
        "である" => !rest.starts_with('と') && !rest.starts_with("こと"),
        "であっ" => !rest.starts_with("ても"),
        _ => true,
    }
}

/// 名詞の言い切りで、語の前にあれば容疑の名前を述べた文とみなす主題（「男の容疑は窃盗だ」「容疑は車の窃盗だ」。
/// 材料「窃盗の疑いで逮捕」の「窃盗」を言い切ったことにはならない）。「〜の疑いで逮捕され、動機は金銭目的だ」の
/// ように、容疑の語が別の主題にかかる文は止めない。
const SUSPICION_TOPICS: [&str; 3] = ["容疑は", "容疑が", "疑いは"];

/// 出力の `start`（語の始まり）を含む文で、語より前に最後に出る容疑の主題（`SUSPICION_TOPICS`）から語までが、
/// 漢字・カタカナ・「の」・「、」だけか（「容疑は、窃盗」「容疑は乗用車の窃盗」）。
fn suspicion_before(output: &str, start: usize) -> bool {
    let sentence = sentence_around(output, start);
    let sentence_start = sentence.as_ptr() as usize - output.as_ptr() as usize;
    let prefix = &output[sentence_start..start];
    let Some(topic_end) = SUSPICION_TOPICS
        .iter()
        .filter_map(|topic| prefix.rfind(topic).map(|i| i + topic.len()))
        .max()
    else {
        return false;
    };
    prefix[topic_end..]
        .chars()
        .all(|c| is_word_char(c) || matches!(c, 'の' | '、'))
}

/// 名詞・形容動詞の言い切り（材料「原因は漏電とみられる」→ 出力「原因は漏電だ」「漏電でした」）。
fn copula_findings(source: &str, output: &str, findings: &mut Vec<SpeculationFinding>) {
    for tail in COPULA_TAILS {
        for (index, _) in output.match_indices(tail) {
            if !copula_asserts(tail, &output[index + tail.len()..]) {
                continue;
            }
            let Some(word) = longest_source_word(source, &output[..index]) else {
                continue;
            };
            if is_non_event(word) {
                continue;
            }
            let word_start = index - word.len();
            if suspicion_before(output, word_start) {
                continue;
            }
            let Some(mixed) = output_assertion(output, word_start) else {
                continue;
            };
            let Some(hedge) =
                hedge_only_in_source(source, word, &SPECULATION_HEDGES, adjacent_hedge_after)
            else {
                continue;
            };
            findings.push(SpeculationFinding {
                form: SpeculationForm::Copula,
                mixed,
                hedge,
            });
        }
    }
}

/// 送り仮名の動詞の形で、材料の側の推測・訴えの言い方（動詞の過去形の直後に続くもの）。`SPECULATION_HEDGES`
/// に加え、「〜と訴え」「〜と主張」と、逮捕・提訴の理由の「〜として」（「〜としてい」は除く）。
const KANA_VERB_HEDGES: [&str; 13] = [
    "可能性",
    "かもしれ",
    "とみられ",
    "と見られ",
    "とみて",
    "疑い",
    "恐れ",
    "おそれ",
    "とされ",
    "といわれ",
    "と言われ",
    "と訴え",
    "と主張",
];

/// 動詞の目的語・主語につく助詞（送り仮名の動詞の形で、照らす句の区切りに使う）。
const KANA_VERB_PARTICLES: [char; 8] = ['を', 'が', 'に', 'で', 'へ', 'と', 'は', 'も'];

/// `after`（材料の動詞の過去形の直後）が、送り仮名の動詞の形の推測・訴えの言い方で始まるか。
fn kana_verb_hedge_at(after: &str) -> Option<&'static str> {
    if after.starts_with("として") && !after.starts_with("としてい") {
        return Some("として");
    }
    KANA_VERB_HEDGES
        .iter()
        .find(|hedge| after.starts_with(**hedge))
        .copied()
}

/// 送り仮名の動詞の句を探すとき、材料の推測・訴えの言い方の前を後ろから見る字数。
const KANA_VERB_SCAN_CHARS: usize = 40;

/// 材料の推測・訴えの言い方の直前の「〈助詞の前の1字〉〈助詞〉〈漢字〉〈送り仮名（2字以上・「た」「だ」で終わる）〉」
/// （「車を盗んだ」「被害を受けた」「現金をだまし取った」）。漢字2字以上＋「し」「さ」で始まる送り仮名は D175 の形
/// （`verb_findings`）が見る。
fn kana_verb_phrase_before(before: &str) -> Option<&str> {
    if !before.ends_with(['た', 'だ']) {
        return None;
    }
    // 後ろから見る字数の上限（句はこれより短い。長い材料で毎回先頭から字を集めないため）。
    let tail_start = before
        .char_indices()
        .rev()
        .nth(KANA_VERB_SCAN_CHARS - 1)
        .map_or(0, |(i, _)| i);
    let before = &before[tail_start..];
    let chars: Vec<(usize, char)> = before.char_indices().collect();
    let mut i = chars.len();
    let kana_end = i;
    while i > 0 && is_hiragana(chars[i - 1].1) {
        i -= 1;
    }
    let kana_len = kana_end - i;
    let kana_first = chars.get(i).map(|(_, c)| *c)?;
    let kanji_end = i;
    while i > 0 && is_kanji(chars[i - 1].1) {
        i -= 1;
    }
    let kanji_len = kanji_end - i;
    if kana_len < 2 || kanji_len == 0 || (kanji_len >= 2 && matches!(kana_first, 'し' | 'さ')) {
        return None;
    }
    // 複合動詞（「受け取った」「だまし取った」）は、前の動詞の部分（助詞でないひらがな＋漢字）も1つだけ含める。
    if i > 0 && is_hiragana(chars[i - 1].1) && !KANA_VERB_PARTICLES.contains(&chars[i - 1].1) {
        while i > 0 && is_hiragana(chars[i - 1].1) && !KANA_VERB_PARTICLES.contains(&chars[i - 1].1)
        {
            i -= 1;
        }
        while i > 0 && is_kanji(chars[i - 1].1) {
            i -= 1;
        }
    }
    if i < 2 || !KANA_VERB_PARTICLES.contains(&chars[i - 1].1) {
        return None;
    }
    // 助詞の前の1字（「車を盗んだ」の「車」、「眠りを妨げられた」の「り」）まで含めて照らす。
    let (noun_start, noun) = chars[i - 2];
    (is_word_char(noun) || is_hiragana(noun)).then(|| &before[noun_start..])
}

/// 送り仮名の動詞の言い切り（材料「男が車を盗んだとして逮捕」→ 出力「男が車を盗んだ」）。照らす句は材料から作り、
/// 出力に同じ句があるときだけ見る（言い換えた動詞は見ない）。材料でその句が1か所でも推測・訴えの言い方なしに
/// 出てくれば報告しない。
fn kana_verb_findings(source: &str, output: &str, findings: &mut Vec<SpeculationFinding>) {
    let mut phrases: Vec<(&str, &'static str)> = Vec::new();
    for (index, _) in source.char_indices() {
        let Some(hedge) = kana_verb_hedge_at(&source[index..]) else {
            continue;
        };
        if let Some(phrase) = kana_verb_phrase_before(&source[..index]) {
            if !phrases.iter().any(|(p, _)| *p == phrase) {
                phrases.push((phrase, hedge));
            }
        }
    }
    for (phrase, hedge) in phrases {
        let hedged_everywhere = source
            .match_indices(phrase)
            .all(|(index, _)| kana_verb_hedge_at(&source[index + phrase.len()..]).is_some());
        if !hedged_everywhere {
            continue;
        }
        for (index, _) in output.match_indices(phrase) {
            if output[index + phrase.len()..].starts_with('ら') {
                continue;
            }
            let Some(mixed) = output_assertion(output, index) else {
                continue;
            };
            findings.push(SpeculationFinding {
                form: SpeculationForm::KanaVerb,
                mixed,
                hedge,
            });
        }
    }
}

/// 確かになったことを表す語。「〜と判明した」「〜と断定された」のように、材料が推測として書いた中身を確かな
/// こととして書いた出力を見る（`certainty_findings`）。
const CERTAINTY_STEMS: [&str; 6] = ["判明", "断定", "特定", "確定", "確認", "発覚"];

/// 確かになったことを表す語の後ろの言い切り（「判明していない」などの打ち消しは `asserting_tail_at` で除く）。
const CERTAINTY_DONE: [&str; 8] = [
    "した",
    "している",
    "していた",
    "しました",
    "された",
    "されている",
    "されていた",
    "されました",
];

/// 確かになったことを表す語の前のつなぎ（長いものから）。「が判明した」は D175 の形（主語の名詞を照らす）が見る。
const CERTAINTY_LEADS: [&str; 5] = ["であることが", "であると", "ことが", "だと", "と"];

/// 中身の語の後ろの言い切りの続き（「関与したと断定」の「した」、「漏電であると判明」の「である」）。
const CONTENT_ENDINGS: [&str; 8] = [
    "されていた",
    "されている",
    "された",
    "していた",
    "している",
    "した",
    "である",
    "だ",
];

/// 「〜と判明した」「〜と断定した」「〜の可能性を断定した」（材料「原因は漏電とみられる」→ 出力「原因は漏電と
/// 判明した」）。照らす語は、確かになったことを表す語の前の中身の語。
fn certainty_findings(source: &str, output: &str, findings: &mut Vec<SpeculationFinding>) {
    for stem in CERTAINTY_STEMS {
        for (index, _) in output.match_indices(stem) {
            let end = index + stem.len();
            let Some(done) = CERTAINTY_DONE
                .iter()
                .find(|done| output[end..].starts_with(**done))
            else {
                continue;
            };
            if !asserting_tail_at(output, end + done.len()) {
                continue;
            }
            let before = &output[..index];
            let (content, masked) = match possibility_object(before, stem, source) {
                Some((content, possibility)) => {
                    // 「可能性」を同じバイト数の記号にして、文の推測の言い方として数えないようにする。
                    let mut masked = output.to_string();
                    masked.replace_range(possibility..possibility + "可能性".len(), "＿＿＿");
                    (content, Some(masked))
                }
                None => {
                    let Some(lead) = CERTAINTY_LEADS.iter().find(|lead| before.ends_with(**lead))
                    else {
                        continue;
                    };
                    (&before[..before.len() - lead.len()], None)
                }
            };
            let content = CONTENT_ENDINGS
                .iter()
                .find_map(|ending| content.strip_suffix(ending))
                .unwrap_or(content);
            let Some(word) = longest_source_word(source, content) else {
                continue;
            };
            if is_non_event(word) {
                continue;
            }
            let checked = masked.as_deref().unwrap_or(output);
            let Some(mixed) = output_assertion(checked, content.len() - word.len()) else {
                continue;
            };
            let Some(hedge) =
                hedge_only_in_source(source, word, &SPECULATION_HEDGES, adjacent_hedge_after)
            else {
                continue;
            };
            findings.push(SpeculationFinding {
                form: SpeculationForm::Certainty,
                mixed,
                hedge,
            });
        }
    }
}

/// 「〜（の）可能性を断定した」「〜可能性が確定した」の、「可能性」より前と「可能性」の位置（バイト）。
/// 可能性は断定・確定できないので、この形の「可能性」は推測の印として数えない。材料が「断定」「確定」を
/// 打ち消しなしに書いていれば None（材料の言い方をなぞった出力とみなす）。
fn possibility_object<'a>(before: &'a str, stem: &str, source: &str) -> Option<(&'a str, usize)> {
    if !matches!(stem, "断定" | "確定") || occurs_affirmed(source, stem) {
        return None;
    }
    let rest = before
        .strip_suffix('を')
        .or_else(|| before.strip_suffix('が'))?;
    let rest = rest.strip_suffix("可能性")?;
    let possibility = rest.len();
    Some((rest.strip_suffix('の').unwrap_or(rest), possibility))
}

/// 考え・つもりを表す語（`NON_EVENT_WORDS`）で終わるか。
fn is_non_event(word: &str) -> bool {
    NON_EVENT_WORDS
        .iter()
        .any(|non_event| word.ends_with(non_event))
}

/// 言い切りの続き（`ASSERTING_TAILS`）の後ろがこれで始まれば打ち消し（「時期が決まっていない」「流出していません」
/// 「法案が成立しなかった」「事故は起きていない」）とみなし、言い切りとして数えない。
const TAIL_NEGATIONS: [&str; 11] = [
    "ていな",
    "ていませ",
    "てはいな",
    "ておらず",
    "てな",
    "なかっ",
    "ない",
    "なく",
    "ません",
    "せん",
    "ず",
];

/// 言い切りの続きの後ろ（`end` から）が、仮定（「したら」）・願い（「したい」）・打ち消し（`TAIL_NEGATIONS`）でないか。
fn asserting_tail_at(output: &str, end: usize) -> bool {
    let rest = &output[end..];
    !rest.starts_with(['ら', 'い'])
        && !TAIL_NEGATIONS
            .iter()
            .any(|negation| rest.starts_with(negation))
}

/// 漢字（CJK 統合漢字・拡張A・「々」）か。
fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々')
}

/// 出来事の語の字（漢字・カタカナ・長音）か。
fn is_word_char(c: char) -> bool {
    is_kanji(c) || matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー')
}

/// `before`（出力の言い切りの続きの前まで）の末尾の漢字・カタカナの語のうち、材料にある最も長い後ろの部分（2字以上）。
/// カタカナを含む語は、語全体か、末尾の漢字だけの部分で照らす（「ータ流出」のような切れ端で照らさない）。
fn longest_source_word<'a>(source: &str, before: &'a str) -> Option<&'a str> {
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, _)| i)?;
    let run = &before[start..];
    let kanji_start = run
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_kanji(*c))
        .last()
        .map_or(run.len(), |(i, _)| i);
    let mut candidates: Vec<&str> = vec![run];
    candidates.extend(
        run[kanji_start..]
            .char_indices()
            .map(|(i, _)| &run[kanji_start + i..])
            .filter(|suffix| *suffix != run),
    );
    candidates
        .into_iter()
        .filter(|word| word.chars().count() >= 2)
        .find(|word| source.contains(*word))
}

/// 文全体にかかる伝聞の言い方と推測の副詞（文のどこにあっても、その文は言い切っていないとみなす。「おそらく」
/// 「たぶん」は語より前に置かれる〔「原因はおそらく漏電だ」〕ため、語より後ろだけを見る一覧には入れない）。
const SENTENCE_WIDE_HEDGES: [&str; 6] = [
    "によると",
    "によれば",
    "によりますと",
    "おそらく",
    "恐らく",
    "たぶん",
];

/// 推測の言い方の直後のひらがな（活用の語尾）にこれがあれば、推測の言い方が主語・主題にかかる
/// （「原因とみられるのは漏電だ」「流出した可能性があるのは顧客情報だ」）。
const HEDGED_TOPIC_MARKERS: [&str; 2] = ["のは", "のが"];

/// 「〈推測の言い方〉〈連体形の語尾〉〈名詞1語〉は／が」（「漏電とされる原因は」「可能性がある顧客情報が」）とみなす、
/// 推測の言い方の後ろから「は」「が」までの字数の上限。
const HEDGED_NOUN_TOPIC_CHARS: usize = 8;

/// 出力の `start`（言い切った語の始まり）を含む文が、その語を言い切っているか。
/// - None: 言い切っていない。語より後ろに推測・伝聞・人の発言の言い方（`HEDGES`・`OUTPUT_ONLY_HEDGES`）がある、
///   文全体にかかる言い方（「〜によると」）がある、または問いかけ（「？」「〜か」）で終わる文。
/// - Some(mixed): 言い切っている。語より前にだけ推測・伝聞の言い方がある文（「…と発表し、顧客情報が流出した」）は
///   `mixed` = true（D175 の版は文ごと見なかった。記録だけにし、`find_asserted_speculation` は報告しない）。
fn output_assertion(output: &str, start: usize) -> Option<bool> {
    let sentence = sentence_around(output, start);
    let sentence_start = sentence.as_ptr() as usize - output.as_ptr() as usize;
    let sentence_end = sentence_start + sentence.len();
    let output_hedges = || HEDGES.iter().chain(OUTPUT_ONLY_HEDGES.iter());
    if output[sentence_end..].starts_with('？')
        || sentence.trim_end().ends_with('か')
        || SENTENCE_WIDE_HEDGES
            .iter()
            .any(|hedge| sentence.contains(hedge))
        || output_hedges().any(|hedge| output[start..sentence_end].contains(hedge))
        || hedged_topic_before(&output[sentence_start..start])
    {
        return None;
    }
    Some(output_hedges().any(|hedge| sentence.contains(hedge)))
}

/// 文の語より前（`prefix`）に、推測・伝聞の言い方が主語・主題にかかる形があるか。止めるのは次の2つだけ:
/// - 推測の言い方の直後のひらがな（「るのは」「があるのは」「ているのが」）に「のは」「のが」がある。
/// - 推測の言い方の直後が「〈ひらがなの連体形の語尾（る・た・い で終わる）〉〈漢字・カタカナの名詞1語〉は／が」で、
///   推測の言い方から `HEDGED_NOUN_TOPIC_CHARS` 字以内（読点も動詞も間に入らない）。
///
/// 「可能性が指摘されたが、」「可能性が話題となり、」のように推測の言い方そのものが主語で動詞が続く形、
/// 「…と発表し、顧客情報が流出した」のように読点で区切られた形は止めない。
fn hedged_topic_before(prefix: &str) -> bool {
    HEDGES
        .iter()
        .chain(OUTPUT_ONLY_HEDGES.iter())
        .chain(SPECULATION_HEDGES.iter())
        .any(|hedge| {
            prefix
                .match_indices(hedge)
                .any(|(index, _)| hedged_topic_after(&prefix[index + hedge.len()..]))
        })
}

/// 推測の言い方の直後（`rest`）が、主語・主題にかかる形か（`hedged_topic_before`）。
fn hedged_topic_after(rest: &str) -> bool {
    let kana_end = rest
        .char_indices()
        .find(|(_, c)| !is_hiragana(*c))
        .map_or(rest.len(), |(i, _)| i);
    let kana = &rest[..kana_end];
    if HEDGED_TOPIC_MARKERS
        .iter()
        .any(|marker| kana.contains(marker))
    {
        return true;
    }
    if !kana.ends_with(['る', 'た', 'い']) {
        return false;
    }
    let noun = &rest[kana_end..];
    let noun_end = noun
        .char_indices()
        .find(|(_, c)| !is_word_char(*c))
        .map_or(noun.len(), |(i, _)| i);
    noun_end > 0
        && noun[noun_end..].starts_with(['は', 'が'])
        && rest[..kana_end + noun_end].chars().count() < HEDGED_NOUN_TOPIC_CHARS
}

/// 材料で `word` が推測・伝聞・訴えの形（`hedges` の語を `hedge_after` で探す）でだけ出てくるとき、その言い方（見出しの「〜か」は「か」）。
/// 1か所でも印なしに出てくれば None（報告しない）。打ち消された出現（「流出はしていない」「未定」）は数えない。
fn hedge_only_in_source(
    source: &str,
    word: &str,
    hedges: &[&'static str],
    hedge_after: fn(&str, &[&'static str]) -> Option<&'static str>,
) -> Option<&'static str> {
    let mut found = None;
    for (index, _) in source.match_indices(word) {
        let before = source[..index].chars().next_back();
        let after = &source[index + word.len()..];
        if before.is_some_and(|c| NEGATING_PREFIXES.contains(&c)) || is_negated_after(after) {
            continue;
        }
        let hedge = hedge_after(after, hedges)?;
        found.get_or_insert(hedge);
    }
    found
}

/// 材料の語のすぐ後ろ（`after`。間に「の」1字まで）に続く推測の言い方（「漏電とみられる」「漏電の可能性」）と、
/// 見出しの「〜か」。名詞の形・「〜と判明」の形で使う（名詞は文の中の別の語にかかる推測〔「台所の配線の漏電と
/// みられる」の「台所」〕を拾いやすいので、`HEDGE_WINDOW_CHARS` 字の範囲では見ない）。
fn adjacent_hedge_after(after: &str, hedges: &[&'static str]) -> Option<&'static str> {
    let rest = after.strip_prefix('の').unwrap_or(after);
    if let Some(hedge) = hedges.iter().find(|hedge| rest.starts_with(**hedge)) {
        return Some(hedge);
    }
    let clause_end = after
        .find(['。', '、', '\n', '！', '？', '!', '?', '　', ' '])
        .unwrap_or(after.len());
    (&after[..clause_end] == "か").then_some("か")
}

/// 材料の語の直後（`after`）の、同じ区切りの中・`HEDGE_WINDOW_CHARS` 字以内にある推測・伝聞・訴えの言い方。
/// 見出しの「〜か」（「情報流出か」「流出したか」）も推測とみなす。
fn source_hedge_after(after: &str, hedges: &[&'static str]) -> Option<&'static str> {
    let clause_end = after
        .find(['。', '、', '\n', '！', '？', '!', '?', '　', ' '])
        .unwrap_or(after.len());
    let clause = &after[..clause_end];
    let window: String = clause.chars().take(HEDGE_WINDOW_CHARS).collect();
    if let Some(hedge) = hedges.iter().find(|hedge| window.contains(**hedge)) {
        return Some(hedge);
    }
    ["か", "したか", "されたか", "させたか"]
        .contains(&clause)
        .then_some("か")
}

// ---------------------------------------------------------------------------
// 年
// ---------------------------------------------------------------------------

/// 年と同じ文にあるとき、年との組み合わせを照合する出来事の語。「発表」「開始」は入れない
/// （「2027年春に発売予定の新型車を発表した」のように、発表した年と出来事の年が別の文で誤検出しやすいため）。
const EVENT_WORDS: [&str; 28] = [
    "契約",
    "発売",
    "公開",
    "設立",
    "創業",
    "就任",
    "退任",
    "開業",
    "開催",
    "導入",
    "締結",
    "合意",
    "移籍",
    "入団",
    "退団",
    "引退",
    "施行",
    "成立",
    "開幕",
    "結婚",
    "受賞",
    "優勝",
    "逮捕",
    "判決",
    "上場",
    "買収",
    "加入",
    "デビュー",
];

/// 元号と元年の前の年（`output_check` と同じ。令和N年 = 2018 + N）。
const ERAS: [(&str, u32); 3] = [("令和", 2018), ("平成", 1988), ("昭和", 1925)];

/// 文の中の年（「2026年」「２０２６年」「令和8年」。1900〜2100 年）。
fn years_in(text: &str) -> Vec<u32> {
    let chars: Vec<char> = text.chars().map(to_halfwidth).collect();
    let mut years = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let Some((value, end)) = parse_number_at(&chars, index) else {
            index += 1;
            continue;
        };
        let mut next = end;
        while chars.get(next) == Some(&' ') {
            next += 1;
        }
        if chars.get(next) == Some(&'年') && value.fract() == 0.0 {
            let value = value as u32;
            let before: String = chars[index.saturating_sub(2)..index].iter().collect();
            let year = ERAS
                .iter()
                .find(|(era, _)| before == *era)
                .map_or(value, |(_, base)| base + value);
            if (1900..=2100).contains(&year) {
                years.push(year);
            }
        }
        index = end;
    }
    years
}

/// 文の中の年月日（「2026年10月28日」「2026.10.28」「2026/10/28」「2026-10-28」。全角も可）と、その位置（バイト）。
/// 記事の日付を書いた出力を、材料にない年・年の組み合わせの誤りとしないため。日付の年は、出力が同じ年月日を
/// 書いたときだけ許す（`mask_source_dates`）。年だけを数として許すと、「2026年に契約」のような作り話の年が通るため。
fn date_spans(text: &str) -> Vec<((u32, u32, u32), std::ops::Range<usize>)> {
    let offsets: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let chars: Vec<char> = text.chars().map(to_halfwidth).collect();
    let read = |start: usize, max_digits: usize| -> Option<(u32, usize)> {
        let digits: String = chars
            .get(start..)?
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits.is_empty() || digits.len() > max_digits {
            return None;
        }
        Some((digits.parse().ok()?, start + digits.len()))
    };
    let mut spans = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() || (index > 0 && chars[index - 1].is_ascii_digit()) {
            index += 1;
            continue;
        }
        let Some((year, after_year)) = read(index, 4) else {
            index += 1;
            continue;
        };
        let date = (|| {
            if !(1900..=2100).contains(&year) {
                return None;
            }
            let separator = *chars.get(after_year)?;
            let (month_end_mark, day_end_mark) = match separator {
                '年' => (Some('月'), Some('日')),
                '.' | '/' | '-' => (Some(separator), None),
                _ => return None,
            };
            let (month, after_month) = read(after_year + 1, 2)?;
            if chars.get(after_month).copied() != month_end_mark {
                return None;
            }
            let (day, mut end) = read(after_month + 1, 2)?;
            if let Some(mark) = day_end_mark {
                if chars.get(end) != Some(&mark) {
                    return None;
                }
                end += 1;
            }
            ((1..=12).contains(&month) && (1..=31).contains(&day))
                .then_some(((year, month, day), end))
        })();
        if let Some((date, end)) = date {
            spans.push((date, offsets[index]..offsets[end]));
        }
        index = after_year;
    }
    spans
}

/// 文の中の年月日（`date_spans` の日付だけ）。
pub(super) fn dates_in(text: &str) -> Vec<(u32, u32, u32)> {
    date_spans(text).into_iter().map(|(date, _)| date).collect()
}

/// 出力のうち、材料にある年月日と同じ日付を書いた部分を、同じ字数の記号に置き換える（年・数字の点検で数えないため）。
/// 「2026年10月28日時点の…」は、材料に「2026.10.28」（記事の日付）があれば数えない。年だけ（「2026年に」）は
/// 置き換えない。
pub(super) fn mask_source_dates(source: &str, output: &str) -> String {
    let source_dates = dates_in(source);
    let mut masked = output.to_string();
    for (date, range) in date_spans(output).into_iter().rev() {
        if source_dates.contains(&date) {
            let chars = output[range.clone()].chars().count();
            masked.replace_range(range, &"＿".repeat(chars));
        }
    }
    masked
}

fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.split(['。', '\n', '！', '？', '!', '?'])
        .filter(|sentence| !sentence.trim().is_empty())
}

/// 年の組み合わせの点検（`IssueKind::YearMismatch`）。材料にある年だけを見る（材料にない年は、
/// 要約では数字の点検、ほかの段では `find_unknown_years` が拾う）。誤検出を減らすため:
/// - 出来事の語が2つ以上ある文は見ない（「2023年に契約し、2026年に移籍した」のように、どの年がどの出来事か
///   を語の並びだけでは決められないため）。
/// - 材料にある年月日（記事の日付。「2026.10.28」の形を含む）と同じ日付として書かれた年は見ない
///   （「2026年10月28日時点の契約内容」のように、記事の日付を書いた文を誤りとしないため。`mask_source_dates`）。
/// - 材料の年には、年月日の形（「2026.10.28」）の年も含める。
pub(super) fn find_year_mismatches(source: &str, output: &str) -> Vec<CheckIssue> {
    let source_sentences: Vec<(Vec<u32>, &str)> = sentences(source)
        .map(|sentence| (years_in(sentence), sentence))
        .collect();
    // 材料の年は「2026年」の形に加え、年月日の形（「2026.10.28」）の年も含める（年の組み合わせを照合するため）。
    let source_years: Vec<u32> = source_sentences
        .iter()
        .flat_map(|(years, sentence)| {
            years
                .iter()
                .copied()
                .chain(dates_in(sentence).into_iter().map(|(year, _, _)| year))
                .collect::<Vec<_>>()
        })
        .collect();
    let paired_years = |sentence: &str| -> Vec<u32> {
        years_in(sentence)
            .into_iter()
            .chain(dates_in(sentence).into_iter().map(|(year, _, _)| year))
            .collect()
    };
    let output = mask_source_dates(source, output);
    let mut issues: Vec<CheckIssue> = Vec::new();
    for sentence in sentences(&output) {
        let words: Vec<&&str> = EVENT_WORDS
            .iter()
            .filter(|word| sentence.contains(**word))
            .collect();
        if words.len() != 1 {
            continue;
        }
        let years: Vec<u32> = years_in(sentence)
            .into_iter()
            .filter(|year| source_years.contains(year))
            .collect();
        if years.is_empty() {
            continue;
        }
        let word = words[0];
        let paired: Vec<u32> = source_sentences
            .iter()
            .filter(|(_, source_sentence)| source_sentence.contains(word))
            .flat_map(|(_, source_sentence)| paired_years(source_sentence))
            .collect();
        if paired.is_empty() || years.iter().any(|year| paired.contains(year)) {
            continue;
        }
        let reason = format!(
            "「{}年」と「{word}」の組み合わせは記事情報になかった。年は記事情報のとおりに書く。",
            years[0]
        );
        if !issues.iter().any(|issue| issue.reason == reason) {
            issues.push(CheckIssue {
                kind: IssueKind::YearMismatch,
                reason,
            });
        }
    }
    issues
}

/// 材料の数字のどれとも合わない年（要点・再説明・一言用）。材料にある年月日と同じ日付を書いた部分の年は
/// 数えない（`mask_source_dates`）。年月日の形の年を、年だけの数としては許さない（作り話の年を通さないため）。
fn find_unknown_years(source: &str, output: &str) -> Vec<CheckIssue> {
    let allowed = japanese_source_numbers(source).allowed;
    let mut issues: Vec<CheckIssue> = Vec::new();
    for year in years_in(&mask_source_dates(source, output)) {
        if contains_value(&allowed, f64::from(year)) {
            continue;
        }
        let reason = format!("「{year}年」は記事情報にない年だった。記事情報にない年は書かない。");
        if !issues.iter().any(|issue| issue.reason == reason) {
            issues.push(CheckIssue {
                kind: IssueKind::UnknownNumber,
                reason,
            });
        }
    }
    issues
}

/// 要約の点検（`output_check::check_japanese_summary`）に足す点検: 見出しの残り・反対の意味・年の組み合わせ・
/// 推測・伝聞・訴えの言い切り（D175）・文の途中の記事にない因果（Firestore タスク 6jL23k。`causal_check`）。
pub(super) fn check_summary_consistency(source: &str, output: &str) -> Vec<CheckIssue> {
    let mut issues: Vec<CheckIssue> = find_heading_residue(output).into_iter().collect();
    issues.extend(find_contradictions(source, output));
    issues.extend(find_year_mismatches(source, output));
    issues.extend(find_asserted_speculation(source, output));
    issues.extend(super::causal_check::find_unsupported_causes(source, output));
    issues
}

/// 要点・注目ポイント・再説明・一言の点検: 見出しの残り・反対の意味・材料にない年・年の組み合わせ・推測の言い切り・
/// 記事にない因果。
/// タグは名詞だけで言い切りの形を持たないので、反対の意味だけを見る（`filter_checked_points`）。
/// 数字全般の照合はしない（再説明・一言は要約・要点を言い換えるので、「3つ」「2人」のような数の数え方で
/// 誤検出しやすい。年だけは D169 で誤りが出たので見る）。
pub(super) fn check_stage_consistency(source: &str, output: &str) -> Vec<CheckIssue> {
    let mut issues = check_summary_consistency(source, output);
    issues.extend(find_unknown_years(source, output));
    issues
}

/// 再説明・一言の点検: `check_stage_consistency` に、後処理で消せなかった次の2つを足す（Firestore タスク NkBSwN）。
/// - 意味をなさない語・下品な語（`IssueKind::BrokenWord`）: 文の途中に残った「うんこ」など
///   （`util::speech_cleanup::broken_words_in`。文の書き出しの語は後処理が消す）。
/// - 言い換えた指示の写し（`IssueKind::PromptEcho`）: 固定指示にだけ出る語句（「ゆうこの言葉」「ト書き」「1段落」など。
///   `instruction_phrases_in`）を含む文。後処理はほかの文が残るときだけ消すので、出力がその文だけのときにここへ来る。
///
/// どちらも材料（記事）にある語は数えない。理由は固定の文と一覧の語だけ（記事の文は入れない）。
/// 要点・注目ポイントは見ない（要点の検証・別の点検がある。対象を再説明・一言に絞る）。
pub(super) fn check_speech_consistency(source: &str, output: &str) -> Vec<CheckIssue> {
    let mut issues = check_stage_consistency(source, output);
    let broken = broken_words_in(output, source);
    if !broken.is_empty() {
        issues.push(CheckIssue {
            kind: IssueKind::BrokenWord,
            reason: "意味をなさない語・下品な語が混ざっていた。記事の中身だけを話す。".to_string(),
        });
    }
    let phrases = instruction_phrases_in(output, source);
    if !phrases.is_empty() {
        issues.push(CheckIssue {
            kind: IssueKind::PromptEcho,
            reason: format!(
                "指示の写し（「{}」など）が混ざっていた。記事の中身だけを話す。",
                phrases[0]
            ),
        });
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(issues: &[CheckIssue]) -> Vec<IssueKind> {
        issues.iter().map(|issue| issue.kind).collect()
    }

    #[test]
    fn markdown_bold_label_at_the_start_is_stripped() {
        // D169 の A9: 要約の先頭に太字の見出しが残った形。
        let (text, removed) =
            strip_heading_labels("**中心となる出来事の要約:** 新しい生成AIが公開された。");
        assert_eq!(text, "新しい生成AIが公開された。");
        assert_eq!(removed, 1);
        let (text, _) = strip_heading_labels("**要約**: 市は図書館を延長する。");
        assert_eq!(text, "市は図書館を延長する。");
        let (text, _) = strip_heading_labels("**要約**\n市は図書館を延長する。");
        assert_eq!(text, "市は図書館を延長する。");
    }

    #[test]
    fn heading_lines_and_plain_labels_are_stripped() {
        let (text, _) = strip_heading_labels("ゆうこの一言：図書館が夜まで開くのはうれしいね");
        assert_eq!(text, "図書館が夜まで開くのはうれしいね");
        let (text, _) = strip_heading_labels("【感想】図書館が夜まで開くのはうれしいね");
        assert_eq!(text, "図書館が夜まで開くのはうれしいね");
        let (text, _) = strip_heading_labels("結論：図書館は夜まで開く。");
        assert_eq!(text, "図書館は夜まで開く。");
        // 本文の中の太字は記号だけを消す。
        let (text, _) = strip_heading_labels("みなとシーガルズは**サヨナラ負け**だった。");
        assert_eq!(text, "みなとシーガルズはサヨナラ負けだった。");
    }

    #[test]
    fn text_without_headings_or_with_content_labels_is_unchanged() {
        for text in [
            "市は図書館を延長する。",
            "市の説明：「夜まで開ける」とした。",
            // 話し手の名詞の付いたラベル（「首相の結論：」）は本文として残す。
            "首相の結論：増税は見送る。",
            "監督の一言：よく粘った。",
            "時刻は10:30に始まる。",
            // `#` の行は消さない（保存前の検証が出力ごと拒否する）。
            "## 要約\n市は図書館を延長する。",
        ] {
            assert_eq!(strip_heading_labels(text), (text.to_string(), 0), "{text}");
        }
        // ラベルだけの出力は空にしない（保存前の検証に任せる）。
        assert_eq!(
            strip_heading_labels("**要約**"),
            ("**要約**".to_string(), 0)
        );
    }

    #[test]
    fn bold_markdown_heading_is_left_for_the_save_validation() {
        // `**# …**` は太字の記号だけを外し、行頭の `#` を残す（§12.5 の検証が出力ごと拒否する）。
        let (text, _) = strip_heading_labels("**# 要約**\n市は図書館を延長する。");
        assert!(text.lines().next().unwrap().starts_with('#'), "{text}");
    }

    #[test]
    fn heading_residue_in_the_middle_is_reported() {
        let residue = find_heading_residue("市は図書館を延長する。感想：うれしいね");
        assert_eq!(residue.map(|issue| issue.kind), Some(IssueKind::Heading));
        assert!(find_heading_residue("市は図書館を延長する。").is_none());
        assert!(find_heading_residue("時刻は10:30に始まる。").is_none());
        assert!(find_heading_residue("会見があった。首相の結論：増税は見送る。").is_none());
    }

    /// 架空のチームの試合（D169 の A2 の形を、架空の名前と文で書いたもの）。
    const LOSS: &str = "みなとシーガルズ、最終回にサヨナラ負け\nみなとシーガルズは9回裏に逆転打を許してやまびこベアーズに敗れた。";

    #[test]
    fn win_and_loss_reversal_is_detected() {
        // D169 の A2 の形: 記事はサヨナラ負け、一言は「サヨナラ勝ちしたんだね」。
        let issues =
            find_contradictions(LOSS, "みなとシーガルズは主砲の活躍でサヨナラ勝ちしたんだね");
        assert_eq!(kinds(&issues), vec![IssueKind::Contradiction]);
        assert!(issues[0].reason.contains("勝ち"));
        assert!(issues[0].reason.contains("敗れ") || issues[0].reason.contains("負け"));
        // 主語の無い一言も、記事と同じ主語の話とみなして拾う。
        assert_eq!(
            kinds(&find_contradictions(LOSS, "サヨナラ勝ちしたんだね")),
            vec![IssueKind::Contradiction]
        );
        // 正しい言い換えは通る。
        assert!(find_contradictions(LOSS, "みなとシーガルズはサヨナラ負けしちゃったね").is_empty());
        // 逆向き（記事が勝ち、出力が負け）も拾う。
        let won = "みなとFCが山城FCを2－0で下し、3連勝を飾った。";
        assert_eq!(
            kinds(&find_contradictions(won, "みなとFCは山城FCに敗れた。")),
            vec![IssueKind::Contradiction]
        );
    }

    /// 架空の試合（Firestore タスク R7cWic の単体テスト用。どれも架空の名前と文）。
    const WALKOFF_LOSS: &str = "みなとシーガルズは1日、やまびこベアーズに3－4でサヨナラ負けを喫した。チームは3連敗となった。";
    const CLOSE_WIN: &str =
        "山城FCは港南ユナイテッドとの接戦を制し、首位に浮上した。港南ユナイテッドは2位に後退した。";

    /// 記録だけ（評価用の `contradiction_findings` は返すが、作り直しの理由の `find_contradictions` は返さない）か。
    fn recorded_only(source: &str, output: &str) -> bool {
        find_contradictions(source, output).is_empty()
            && contradiction_findings(source, output)
                .iter()
                .any(|finding| {
                    finding.recorded_only && finding.issue.kind == IssueKind::Contradiction
                })
    }

    #[test]
    fn other_win_and_loss_expressions_are_recorded_only() {
        // 「勝ち」「白星」以外の勝敗の言い方（快勝・接戦を制し・大敗・苦杯・圧勝）は記録だけ（評価 §9.11）。
        for output in [
            "シーガルズは延長戦を制したね",
            "シーガルズは快勝したね",
            "みなとシーガルズは圧勝した。",
        ] {
            assert!(recorded_only(WALKOFF_LOSS, output), "{output}");
        }
        assert!(recorded_only(CLOSE_WIN, "山城FCは港南に大敗した。"));
        let won = "ひばり高校がつばめ工業に勝利した。";
        assert!(recorded_only(won, "ひばり高校は苦杯をなめたね"));
        let election = "あおば党は総選挙で議席を大きく減らし、大敗した。";
        assert!(recorded_only(election, "あおば党が圧勝した。"));
        assert!(contradiction_findings(election, "ひかり党が圧勝した。").is_empty());
        // 記録だけの語が材料にあるときは、従来の語の出力でも作り直しの理由にしない（材料の反対の語に使わない）。
        assert!(recorded_only(
            CLOSE_WIN,
            "山城FCは港南ユナイテッドに敗れた。"
        ));
        // 材料の側の言い方としては使う（「接戦を制し」の記事で「山城FCが勝った」は報告しない）。
        assert!(contradiction_findings(CLOSE_WIN, "山城FCが勝った。").is_empty());
    }

    #[test]
    fn possessive_and_cleft_subjects_are_compared() {
        // 「Aのサヨナラ勝ち」「勝ったのはAだ」の A を主語として照らす（ゆうこの話し言葉に多い形）。
        assert_eq!(
            kinds(&find_contradictions(
                WALKOFF_LOSS,
                "シーガルズのサヨナラ勝ちだね"
            )),
            vec![IssueKind::Contradiction]
        );
        assert_eq!(
            kinds(&find_contradictions(
                WALKOFF_LOSS,
                "サヨナラ勝ちしたのはシーガルズだね"
            )),
            vec![IssueKind::Contradiction]
        );
        // 勝ったのが相手のチームなら正しい言い換え（主語の無い文として扱うと誤検出していた形）。
        for output in [
            "サヨナラ勝ちしたのはベアーズだね",
            "最後に勝ったのがやまびこベアーズだったんだね",
            "ベアーズのサヨナラ勝ちで試合が決まったね",
            "延長戦を制したのはやまびこベアーズ。シーガルズは連敗だね",
        ] {
            assert!(
                contradiction_findings(WALKOFF_LOSS, output).is_empty(),
                "{output}"
            );
        }
    }

    #[test]
    fn swapped_win_and_loss_in_a_two_sided_output_is_recorded_only() {
        // 出力が勝敗の両方を書き、主語ごとに入れ替えた形（評価 §9.4 の既知の限界だったもの）は記録だけ
        // （「Aは前日Bに勝った。この日はCに敗れた。」のように、負けの文の主語を省いた正しい出力を見分けられないため）。
        for (source, output) in [
            (WALKOFF_LOSS, "シーガルズが勝った。ベアーズは敗れた。"),
            (CLOSE_WIN, "港南ユナイテッドが勝ち、山城FCは敗れた。"),
            (
                CLOSE_WIN,
                "港南ユナイテッドが接戦を制した。山城FCは敗れた。",
            ),
            // 略称（記事の名前の一部）でも同じ主語とみなす。
            (CLOSE_WIN, "港南が勝ち、山城は敗れた。"),
            (
                "国を相手にした訴訟で、原告が敗訴した。",
                "原告が勝訴し、国は敗訴した。",
            ),
        ] {
            assert!(recorded_only(source, output), "{output}");
        }
        // 記録だけにした理由の形: 負けの文の主語が省かれている（「この日は」）と、正しい出力でも記録される。
        assert!(recorded_only(
            "みなとシーガルズはやまびこベアーズに敗れた。",
            "みなとシーガルズは前日うみねこ団に勝った。この日はベアーズに敗れた。"
        ));
    }

    #[test]
    fn two_sided_outputs_that_follow_the_article_are_not_reported() {
        for (source, output) in [
            (WALKOFF_LOSS, "ベアーズが勝ち、シーガルズは負けた。"),
            (WALKOFF_LOSS, "ベアーズは勝ち、シーガルズは敗れた。"),
            (CLOSE_WIN, "山城FCが勝ち、港南ユナイテッドは敗れた。"),
            (CLOSE_WIN, "山城FCは勝ち、港南ユナイテッドが敗れた。"),
            (CLOSE_WIN, "山城FCは接戦を制し、港南ユナイテッドは敗れた。"),
            // 同じ主語の勝ちと負けを両方書いた出力（目指した・前回の勝ち・勝ったものの敗れた）。
            (
                WALKOFF_LOSS,
                "シーガルズは勝利を目指したけど、サヨナラ負けしちゃったね",
            ),
            (
                WALKOFF_LOSS,
                "シーガルズは前回ベアーズに勝っていたけど、今回はサヨナラ負けだったね",
            ),
            (
                WALKOFF_LOSS,
                "シーガルズは前回の対戦では勝利したけど、今回はベアーズに敗れて3連敗",
            ),
            (
                "みなとFCは決勝で港南FCに敗れ、準優勝だった。",
                "みなとFCは準決勝で山城FCに勝ったものの、決勝で港南FCに敗れた。",
            ),
            // 主語の無い出力（どちらのチームの話か決められない）。
            (WALKOFF_LOSS, "勝っても負けても応援したいね"),
            (
                "みなとFCは決勝で港南FCに敗れ、準優勝だった。",
                "準決勝では勝ったけど、決勝で敗れちゃったね",
            ),
            (
                "国を相手にした訴訟で、原告が敗訴した。",
                "国が勝訴し、原告は敗訴した。",
            ),
            // 両チームの勝敗が書かれた記事は、出力が何を書いても報告しない（従来どおり）。
            (
                "うみねこ団は前日にかもめ団に勝ったが、この日は敗れた。",
                "うみねこ団は勝った",
            ),
        ] {
            assert!(
                contradiction_findings(source, output).is_empty(),
                "{output}"
            );
        }
        // 既知の限界: 1 文で「Aは勝ち、Bは敗れた」と書くと、「敗れ」の主語に文の最初の A も入るので、
        // 入れ替えても拾わない（誤検出を避ける側に倒した。評価 §9.11）。
        assert!(
            find_contradictions(WALKOFF_LOSS, "シーガルズは勝ち、ベアーズは敗れた。").is_empty()
        );
    }

    #[test]
    fn compounds_wishes_and_non_result_win_words_are_not_reported() {
        for output in [
            // 複合語・記録の名前（勝ち点・勝負・勝利投手・優勝争い）。
            "勝ち点の話じゃないけど、シーガルズは3連敗だね",
            "勝負どころでベアーズに打たれ、シーガルズは負けたね",
            "勝利投手はベアーズの投手で、シーガルズは敗れた。",
            "シーガルズは優勝争いから一歩後退し、3連敗となった。",
            // 結果でない勝ち（目指す・目前・願い・仮定・当然・遠ざかる・勝ちきれない・見えていた）。
            "シーガルズは勝利目前だったのに、延長で敗れたよ",
            "シーガルズは勝利まであと一歩だった",
            "次こそ勝ってほしいな！",
            "監督は「次は勝ちたい」と話していたね",
            "シーガルズは「勝ちたかった」と悔しがった。",
            "シーガルズ、勝ってたら首位だったのにね",
            "シーガルズは勝って当然の試合を落としちゃったね",
            "シーガルズは勝ちから遠ざかっていて、3連敗だね",
            "シーガルズは勝ちきれず、3連敗となった。",
            "シーガルズの勝ちが見えてたのに残念だったね",
            "シーガルズは雪辱を期して次の試合に臨むね",
            // 試合の勝ちでない「制し」。
            "シーガルズは中盤を制したけど、最後に打たれちゃったね",
            // 連勝が止まった（負けの意味）。
            "シーガルズは連勝がストップしちゃったね",
        ] {
            assert!(
                contradiction_findings(WALKOFF_LOSS, output).is_empty(),
                "{output}"
            );
        }
        let won = "ひばり高校がつばめ工業に勝利した。";
        for output in [
            "ひばり高校にとって負けられない試合だったね",
            "ひばり高校は前回敗れたつばめ工業に勝ったね",
            "ひばり高校は前回屈したつばめ工業に雪辱した。",
            "つばめ工業は苦杯をなめたね",
        ] {
            assert!(contradiction_findings(won, output).is_empty(), "{output}");
        }
        // 決勝で負けた記事の「準優勝」、敗者復活戦の記事。
        assert!(contradiction_findings(
            "みなとFCは決勝で港南FCに敗れ、準優勝だった。",
            "みなとFCは準優勝だったね"
        )
        .is_empty());
        assert!(contradiction_findings(
            "山本は3位決定戦に勝利し、銅メダルを獲得した。",
            "山本は敗者復活戦を経て銅メダルを獲得した。"
        )
        .is_empty());
        // 「判断を下した」（「下し」は勝敗の語に入れていない）。
        assert!(contradiction_findings(
            "あおば商事は価格競争に敗れ、撤退を決めた。",
            "あおば商事は撤退の判断を下したね"
        )
        .is_empty());
        // 連勝が止まった記事を「連勝ストップ」と書いた出力。
        let streak = "北辰ホークスはみなみオーシャンズに敗れ、連勝が7で止まった。";
        for output in ["ホークスの連勝ストップだね", "連勝が止まったのは悔しいね"]
        {
            assert!(
                contradiction_findings(streak, output).is_empty(),
                "{output}"
            );
        }
    }

    #[test]
    fn win_and_loss_from_the_other_side_is_not_reported() {
        // 主語の入れ替え（「AがBを下した」→「BはAに敗れた」「やまびこベアーズが勝った」）は反対の意味ではない。
        let won = "みなとFCが山城FCを2－0で下し、3連勝を飾った。";
        assert!(find_contradictions(won, "山城FCはみなとFCに敗れた。").is_empty());
        assert!(find_contradictions(LOSS, "やまびこベアーズがサヨナラ勝ちしたんだね").is_empty());
    }

    #[test]
    fn negations_compounds_and_mixed_sources_are_not_reported() {
        let source = "みなとシーガルズはやまびこベアーズに5－6でサヨナラ負けした。";
        // 打ち消し。
        assert!(find_contradictions(source, "みなとシーガルズは勝利を逃した。").is_empty());
        assert!(find_contradictions(source, "みなとシーガルズは勝てなかった。").is_empty());
        // 複合語（勝ち点・勝ち越し）。
        assert!(find_contradictions(source, "みなとシーガルズは勝ち越しを許した。").is_empty());
        // 両方の側が材料にある記事（両チームの勝敗）。
        let both = "うみねこ団はかもめ団に敗れ、やまびこベアーズはみなとシーガルズに勝利した。";
        assert!(
            find_contradictions(both, "やまびこベアーズが勝ち、うみねこ団は負けた。").is_empty()
        );
        // 材料に広い言い方（「増」）があれば報告しない。
        assert!(find_contradictions(
            "利用者は前年より増えた一方、減少した店もある。",
            "利用者は増加した。"
        )
        .is_empty());
        // 「無敗」は「敗」の打ち消し。
        assert!(
            find_contradictions("みなとFCは開幕から無敗。", "みなとFCは勝ち続けている。")
                .is_empty()
        );
    }

    #[test]
    fn verb_forms_of_negation_are_not_counted() {
        let proposal = "政府は削減を検討している。";
        for output in [
            "削減はまだ決定していない。",
            "削減は決定されていない。",
            "削減の決定には至っていない。",
            "削減は決定には至らなかった。",
            "削減は決定しておらず、検討が続く。",
            "削減は決定されておらず、検討が続く。",
            "削減が決まってはいない。",
            "削減は決定はしていない。",
        ] {
            assert!(find_contradictions(proposal, output).is_empty(), "{output}");
        }
        let decreasing = "利用者は前年より8%減少した。";
        assert!(find_contradictions(decreasing, "利用者は増加していない。").is_empty());
        assert!(
            find_contradictions("受付は11月末で終了した。", "受付は開始していない。").is_empty()
        );
    }

    #[test]
    fn output_with_both_sides_is_not_reported() {
        // 「停止していた運転を再開した」は両方の側を言っている。
        assert!(find_contradictions(
            "地震で止まっていた工場が運転を再開した。",
            "地震で停止していた工場が運転を再開した。"
        )
        .is_empty());
        // 「開始・締め切り11月末」の記事を「11月末で受付終了」と書いても報告しない（締め切りは終わりの側）。
        assert!(find_contradictions(
            "市は給付金の申請の受け付けを開始した。締め切りは11月末。",
            "給付金の申請は11月末で受付終了になる。"
        )
        .is_empty());
    }

    #[test]
    fn decided_versus_proposal_is_detected_in_a_simplified_a10_case() {
        // D169 の A10 を簡単にした架空の形（記事は「削減する案」「党として決めておらず」、出力は「決まった」）。
        // 実際の A10 は記事に「連立合意」（決まった側の語）があり、この点検では拾えない（評価 §9.3）。
        let source = "みどり党の代表は、議員定数を30削減する案を採るなら将来の削減も法案に書くべきだと主張した。党としては賛否を決めておらず、他党と協議する。";
        let issues = find_contradictions(source, "議員定数が30減るって決まったんだね");
        assert_eq!(kinds(&issues), vec![IssueKind::Contradiction]);
        // 記事が決定なら通る。
        assert!(
            find_contradictions("政府は削減を正式に決定した。", "削減が決まったんだね").is_empty()
        );
        // 逆向き（記事が決定・出力が検討）は報告しない。
        assert!(
            find_contradictions("政府は削減を決定した。", "政府は削減を検討している。").is_empty()
        );
        // 出力の打ち消し（「決まっていない」）は報告しない。
        assert!(find_contradictions(source, "まだ決まっていないんだね").is_empty());
    }

    #[test]
    fn increase_versus_decrease_is_detected() {
        let source = "県内の出生数は前年より8%減少した。";
        assert_eq!(
            kinds(&find_contradictions(source, "県内の出生数は8%増加した。")),
            vec![IssueKind::Contradiction]
        );
        assert!(find_contradictions(source, "県内の出生数は8%減った。").is_empty());
    }

    /// 架空の選手の契約（D169 の A5 の形を、架空の名前と数字で書いたもの）。
    const CONTRACT: &str = "2023年12月にうみかぜ球団と10年の大型契約を結んだ青山投手。2024年に300万ドル、2026年に900万ドルの年俸を受け取った。";

    #[test]
    fn year_paired_with_a_different_event_is_detected() {
        // D169 の A5 の形: 契約は 2023 年。材料の別の文に 2026 年（年俸の年）がある。
        let issues = find_year_mismatches(
            CONTRACT,
            "青山投手は2026年にうみかぜ球団と大型契約を結んだ。",
        );
        assert_eq!(kinds(&issues), vec![IssueKind::YearMismatch]);
        assert!(issues[0].reason.contains("2026年"));
        // 正しい組み合わせ・年が複数ある文・出来事の語のない文は通る。
        assert!(find_year_mismatches(CONTRACT, "2023年に球団と契約した。").is_empty());
        assert!(
            find_year_mismatches(CONTRACT, "2023年に契約し、2026年に900万ドルを受け取った。")
                .is_empty()
        );
        assert!(find_year_mismatches(CONTRACT, "2026年の年俸は900万ドルだった。").is_empty());
        // 元号の年も読む。
        assert!(
            find_year_mismatches("令和5年に契約を結んだ。", "2023年に契約を結んだ。").is_empty()
        );
    }

    #[test]
    fn year_mismatch_false_positives_are_avoided() {
        // 「発表」は出来事の語にしない（発表の年と発売の年は別）。
        let source = "ひかり自動車は2026年10月、新型車を2027年春に発売すると発表した。";
        assert!(find_year_mismatches(source, "2027年春に発売予定の新型車を発表した。").is_empty());
        // 出来事の語が2つある文は見ない。
        assert!(find_year_mismatches(
            CONTRACT,
            "青山投手は2026年に契約内容が見直され、移籍も話題になった。"
        )
        .is_empty());
        // 記事の日付（「2026.10.28」の形）として書かれた年は見ない。
        let dated = format!("{CONTRACT}\n2026.10.28");
        assert!(find_year_mismatches(
            &dated,
            "2026年10月28日時点での契約内容を見ると、年俸は格安だ。"
        )
        .is_empty());
        assert_eq!(
            dates_in("2026.10.28 と 2026/1/5 と 2026年10月28日"),
            vec![(2026, 10, 28), (2026, 1, 5), (2026, 10, 28)]
        );
        assert!(dates_in("2026.5 と 3.14 と 2026年10月").is_empty());
    }

    #[test]
    fn stage_check_reports_years_missing_from_the_source() {
        let source = "2023年12月に球団と契約を結んだ。";
        let issues = check_stage_consistency(source, "2025年に契約したんだね");
        assert_eq!(kinds(&issues), vec![IssueKind::UnknownNumber]);
        assert!(check_stage_consistency(source, "2023年に契約したんだね").is_empty());
        // 年でない数字（「3年間」「24歳」）は見ない。
        assert!(check_stage_consistency(source, "3年間で24歳になったんだね").is_empty());
        // 記事の日付の形（「2026.10.28」「2026/10/28」）と同じ年月日を書いた部分の年は数えない。
        let dated = format!("{source}\n2026.10.28");
        assert!(check_stage_consistency(&dated, "2026年10月28日の時点で話題だね").is_empty());
        let slashed = format!("{source}\n2026/10/28");
        assert!(check_stage_consistency(&slashed, "2026年10月28日の時点で話題だね").is_empty());
        // 日付の年を、年だけの数としては許さない（作り話の年を通さないため）。
        assert_eq!(
            kinds(&check_stage_consistency(&dated, "2026年の今も話題だね")),
            vec![IssueKind::UnknownNumber]
        );
        // 別の日付（2026年10月27日）は記事の日付と同じではないので数える。
        assert_eq!(
            kinds(&check_stage_consistency(
                &dated,
                "2026年10月27日の時点で話題だね"
            )),
            vec![IssueKind::UnknownNumber]
        );
    }

    #[test]
    fn year_of_the_article_date_does_not_let_a_fabricated_year_through() {
        // 記事は 2023年12月の契約、本文に記事の日付「2026.10.28」がある。出力は「2026年に契約」（作り話の年）。
        let source = "うみかぜ球団の青山投手は2023年12月に10年の大型契約を結んだ。\n2026.10.28";
        let output = "青山投手は2026年にうみかぜ球団と大型契約を結んだ。";
        let stage = check_stage_consistency(source, output);
        assert!(
            stage
                .iter()
                .any(|issue| issue.kind == IssueKind::UnknownNumber),
            "{stage:?}"
        );
        assert!(
            stage
                .iter()
                .any(|issue| issue.kind == IssueKind::YearMismatch),
            "{stage:?}"
        );
        let summary = super::super::output_check::check_japanese_summary(source, output, None, &[]);
        assert!(
            summary
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::UnknownNumber),
            "{summary:?}"
        );
        assert!(summary.blocks_use(), "{summary:?}");
        // 記事の日付と同じ年月日を書いた要約は、数字・年の点検に引っかからない。
        let dated =
            "2026年10月28日時点で、青山投手は2023年12月に結んだ10年の大型契約を続けている。";
        let report = super::super::output_check::check_japanese_summary(source, dated, None, &[]);
        assert!(report.passed(), "{report:?}");
    }

    #[test]
    fn subject_stops_at_particles_and_falls_back_to_the_topic() {
        // 「Aは打線がつながらず敗れた」の主語は「打線」と、文の最初の「〜は」の「A」。
        let source = "みなとシーガルズは打線がつながらず敗れた。";
        assert_eq!(
            kinds(&find_contradictions(
                source,
                "みなとシーガルズはサヨナラ勝ちした。"
            )),
            vec![IssueKind::Contradiction]
        );
        let source = "みなとシーガルズは敗れた。";
        assert_eq!(
            kinds(&find_contradictions(
                source,
                "みなとシーガルズは打線が爆発して勝利した。"
            )),
            vec![IssueKind::Contradiction]
        );
        // 別のチームが主語なら報告しない。
        assert!(
            find_contradictions(source, "やまびこベアーズは打線が爆発して勝利した。").is_empty()
        );
        assert_eq!(
            subject_before(
                "みなとシーガルズは打線がつながらず",
                "みなとシーガルズは打線がつながらず".len()
            ),
            Some("打線".to_string())
        );
    }

    #[test]
    fn polite_and_double_negations_are_read() {
        let proposal = "政府は削減を検討している。";
        for output in [
            "削減は決定していません。",
            "削減はまだ決まっていません。",
            "削減の決定を見送った。",
        ] {
            assert!(find_contradictions(proposal, output).is_empty(), "{output}");
        }
        assert!(
            find_contradictions("受付は11月末で終了した。", "受付は開始しませんでした。")
                .is_empty()
        );
        // 二重の打ち消し（「勝ちを逃さなかった」「勝利を逃さず」）は勝ちの意味のまま。
        assert_eq!(
            kinds(&find_contradictions(
                LOSS,
                "みなとシーガルズは最後まで勝ちを逃さなかった。"
            )),
            vec![IssueKind::Contradiction]
        );
        assert_eq!(
            kinds(&find_contradictions(
                LOSS,
                "みなとシーガルズは勝利を逃さず喜んだ。"
            )),
            vec![IssueKind::Contradiction]
        );
        // 一重の打ち消しは数えない。
        assert!(find_contradictions(LOSS, "みなとシーガルズは勝利を逃した。").is_empty());
    }

    #[test]
    fn long_labels_starting_with_a_self_prefix_are_kept() {
        for text in [
            "今回の会見での首相の結論：増税は見送る。",
            "ニュースで話題の監督の一言：よく粘った。",
        ] {
            assert_eq!(strip_heading_labels(text), (text.to_string(), 0), "{text}");
        }
        let (text, _) = strip_heading_labels("今回の感想：図書館が夜まで開くのはうれしいね");
        assert_eq!(text, "図書館が夜まで開くのはうれしいね");
        let (text, _) = strip_heading_labels("AIまとめ：図書館は夜まで開く。");
        assert_eq!(text, "図書館は夜まで開く。");
    }

    /// 架空の訴訟（D167 の評価で出た形: 訴え〔権利〕を退けた判決を、一言が「権利が認められた」と書いた）。
    const DISMISSED: &str = "うみかぜ食品の表示訴訟、原告の訴えを退ける\nみなと地裁は、うみかぜ食品の商品名をめぐり原告が求めた表示の権利について、原告の請求を棄却する判決を言い渡した。";

    #[test]
    fn verdict_reversal_is_detected_in_every_stage() {
        // 一言・再説明（check_stage_consistency）、要約（check_summary_consistency）、要点（項目ごと）。
        for output in [
            "原告の権利が認められたんだね、よかった！",
            "裁判所は、原告の訴えを認めたんだよ。",
            "みなと地裁は原告の請求を認める判決を言い渡した。",
            "原告の請求が認められた",
        ] {
            assert_eq!(
                kinds(&check_stage_consistency(DISMISSED, output)),
                vec![IssueKind::Contradiction],
                "{output}"
            );
            assert_eq!(
                kinds(&check_summary_consistency(DISMISSED, output)),
                vec![IssueKind::Contradiction],
                "{output}"
            );
        }
        // 逆向き（記事は認めた、出力は退けた）も拾う。
        let granted = "みなと地裁は原告の請求を認め、被告に支払いを命じた。";
        assert_eq!(
            kinds(&check_stage_consistency(
                granted,
                "地裁は原告の請求を棄却した。"
            )),
            vec![IssueKind::Contradiction]
        );
        // 勝訴⇔敗訴（主語が同じとき）。
        let lost = "やまなみ市の住民が起こした訴訟で、住民側が敗訴した。";
        let issues = check_stage_consistency(lost, "住民側が勝訴したんだね");
        assert_eq!(kinds(&issues), vec![IssueKind::Contradiction]);
        assert!(issues[0].reason.contains("勝訴") && issues[0].reason.contains("敗訴"));
        assert_eq!(
            kinds(&check_stage_consistency(lost, "勝訴できてよかったね")),
            vec![IssueKind::Contradiction]
        );
        // 合憲⇔違憲・適法⇔違法・有効⇔無効・起訴⇔不起訴。
        for (source, output) in [
            (
                "最高裁は、その規定を違憲と判断した。",
                "最高裁は規定を合憲と判断した。",
            ),
            (
                "高裁は、選挙は合憲だと判断した。",
                "高裁は選挙を違憲と判断した。",
            ),
            (
                "地裁は、市の処分を違法と判断した。",
                "地裁は市の処分を適法とした。",
            ),
            (
                "地裁は、市の処分を適法と判断した。",
                "地裁は市の処分を違法とした。",
            ),
            (
                "地裁は、その契約を無効とした。",
                "地裁は契約を有効と認めた。",
            ),
            ("地検は、男性を不起訴処分とした。", "地検は男性を起訴した。"),
            ("地検は、男性を起訴した。", "男性は不起訴になったんだね"),
            ("地検は、男性の起訴を見送った。", "地検は男性を起訴した。"),
            (
                "地検は、男性を起訴猶予処分とした。",
                "男性は起訴されたんだね",
            ),
        ] {
            assert_eq!(
                kinds(&check_stage_consistency(source, output)),
                vec![IssueKind::Contradiction],
                "{source} / {output}"
            );
        }
    }

    #[test]
    fn verdict_words_starting_with_a_negating_character_count_as_the_opposite_side() {
        // 「不起訴」「無効」「不適法」「非合法」は、語の直前が打ち消しの字のときに数えない規則と食い合わず、
        // 反対の側として数える。中の「起訴」「適法」「合法」は直前が「不」「非」なので、起訴・適法・合法の側に数えない。
        assert!(occurs_affirmed("男性を不起訴とした。", "不起訴"));
        assert!(!occurs_affirmed("男性を不起訴とした。", "起訴"));
        assert!(occurs_affirmed("契約は無効とした。", "無効"));
        assert!(!occurs_affirmed("契約は無効とした。", "有効"));
        assert!(occurs_affirmed("訴えは不適法として却下した。", "不適法"));
        assert!(!occurs_affirmed("訴えは不適法として却下した。", "適法"));
        assert!(occurs_affirmed("非合法の取引だった。", "非合法"));
        assert!(!occurs_affirmed("非合法の取引だった。", "合法"));
        assert!(occurs_affirmed("規定を違憲とした。", "違憲"));
        // 記事が「不起訴」で出力も「不起訴」なら、出力の「起訴」の字で報告しない（逆も同じ）。
        let not_indicted = "地検は男性を不起訴処分とした。";
        assert!(check_stage_consistency(not_indicted, "男性は不起訴になった。").is_empty());
        assert!(check_stage_consistency(
            "地裁は契約を無効と判断した。",
            "契約は無効と判断されたんだね"
        )
        .is_empty());
        assert!(check_stage_consistency(
            "訴えは不適法として却下された。",
            "訴えは不適法とされた。"
        )
        .is_empty());
        // 打ち消しの字で始まる語そのものの打ち消し（「無効ではない」「不起訴とはならず」は語の直後で読む）。
        assert!(!occurs_affirmed("契約は無効ではないとした。", "無効"));
        assert!(!occurs_affirmed("規定は違憲とはいえないとした。", "違憲"));
        // 起訴猶予は不起訴の一種: 記事が起訴猶予で出力が不起訴なら報告しない。
        assert!(check_stage_consistency(
            "地検は男性を起訴猶予処分とした。",
            "男性は不起訴になったんだね"
        )
        .is_empty());
    }

    #[test]
    fn verdict_paraphrases_and_both_sides_are_not_reported() {
        // 言い換え（「訴えが認められなかった」＝敗訴、「請求を認めず」）は打ち消しなので数えない。
        for output in [
            "原告の訴えは認められなかったんだね",
            "原告の訴えが認められず、残念な結果になった。",
            "地裁は原告の請求を認めなかった。",
            "裁判所は原告の権利を認めず、請求を棄却した。",
            "原告側の敗訴だね",
            "原告の請求は退けられた。",
        ] {
            assert!(
                check_stage_consistency(DISMISSED, output).is_empty(),
                "{output}"
            );
            assert!(
                check_summary_consistency(DISMISSED, output).is_empty(),
                "{output}"
            );
        }
        // 両方の側を書いた出力（一審は認め、二審は退けた）は報告しない。
        assert!(check_stage_consistency(
            DISMISSED,
            "一審は原告の請求を認めたが、今回は原告の請求を棄却した。"
        )
        .is_empty());
        // 記事が一部を認め、残りを退けたとき（材料に両方の側）は報告しない。
        let partly = "地裁は原告の請求を一部認め、その他の請求を棄却した。";
        assert!(check_stage_consistency(partly, "原告の請求が認められた。").is_empty());
        assert!(check_stage_consistency(partly, "原告の請求は棄却された。").is_empty());
        // 主語の入れ替え（「原告が敗訴」→「国が勝訴」）は反対の意味ではない。
        let lost = "国を相手にした訴訟で、原告が敗訴した。";
        assert!(check_stage_consistency(lost, "国が勝訴した。").is_empty());
        // 求めただけの「退けるよう求めた」「認めるよう求めた」は、その側の語として数えない
        // （判決が逆なら拾い、判決どおりなら通す）。
        let asked = "被告側は請求を退けるよう求めていたが、地裁は原告の請求を認めた。";
        assert!(check_stage_consistency(asked, "地裁は原告の請求を認めたんだね").is_empty());
        assert_eq!(
            kinds(&check_stage_consistency(
                asked,
                "地裁は原告の請求を退けた。"
            )),
            vec![IssueKind::Contradiction]
        );
        assert_eq!(
            kinds(&check_stage_consistency(
                "原告は権利を認めるよう求めていたが、地裁は原告の請求を棄却した。",
                "原告の権利が認められたんだね"
            )),
            vec![IssueKind::Contradiction]
        );
        // 「違法性はない」「違法とまではいえない」は違法の側に数えない（記事がそう書き、出力が適法としても報告しない）。
        assert!(check_stage_consistency(
            "地裁は、市の対応に違法性はないと判断した。",
            "地裁は市の対応を適法とした。"
        )
        .is_empty());
        assert!(check_stage_consistency(
            "高裁は、処分が違法とまではいえないとした。",
            "処分は適法とされたんだね"
        )
        .is_empty());
        // 判決と関係のない「有効」「認め」（記事にも同じ側がある）は報告しない。
        assert!(check_stage_consistency(
            "新しい薬は有効だと認められた。無効だった旧薬は使わない。",
            "新しい薬は有効と認められた。"
        )
        .is_empty());
        assert!(check_stage_consistency(
            "被告は起訴内容を認めた。",
            "被告は起訴内容を認めたんだね"
        )
        .is_empty());
        // 訴えの名前の「選挙無効」「解雇無効」・求めの「無効を求め」と、防ぐ意味の「無効化」は無効の側に数えない。
        for (source, output) in [
            (
                "高裁は違憲状態と判断したが、選挙無効の請求は棄却した。",
                "選挙そのものは有効とされた。",
            ),
            (
                "元社員が解雇無効を求めた訴訟で、地裁は請求を退けた。",
                "会社の解雇は有効と判断されたんだね",
            ),
            (
                "新しい機能は、マルウェアの攻撃を無効化する。",
                "マルウェア対策に有効な新機能だね",
            ),
        ] {
            assert!(
                check_stage_consistency(source, output).is_empty(),
                "{source} / {output}"
            );
        }
        // 判決の「一部認容」は認めた側としても数える（材料に両方の側があるので報告しない）。
        assert!(check_stage_consistency(
            "地裁は原告の請求を一部認容し、その余の請求を棄却した。",
            "原告の請求が認められた。"
        )
        .is_empty());
    }

    /// 架空の記事: 情報流出の「可能性が高い」記事（推測としてだけ書かれた出来事）。
    const LEAK: &str = "あおば通販、顧客情報流出の可能性\nあおば通販は、外部からの不正アクセスにより顧客約2万人分の情報が流出した可能性が高いと発表した。";

    #[test]
    fn asserting_a_possible_leak_is_reported() {
        for output in [
            "あおば通販で顧客約2万人分の情報が流出した。",
            "顧客情報が流出しちゃったんだね",
            "不正アクセスで顧客情報が流出しました。",
            // 材料の語より長い語（「情報流出」）でも、材料にある後ろの部分（「流出」）で照らす。
            "あおば通販は顧客情報流出していた。",
        ] {
            let issues = find_asserted_speculation(LEAK, output);
            assert_eq!(
                kinds(&issues),
                vec![IssueKind::AssertedSpeculation],
                "{output}"
            );
            assert!(issues[0].reason.contains("可能性"), "{output}");
        }
    }

    #[test]
    fn hedged_or_negated_paraphrases_are_not_reported() {
        for output in [
            "顧客情報が流出したかもしれない。",
            "顧客情報が流出したとされる。",
            "顧客情報が流出した可能性が高い。",
            "顧客情報が流出したとみられる。",
            "あおば通販は、顧客情報が流出した可能性があると発表した。",
            "顧客情報が流出したの？",
            "顧客情報が流出したら大変だね",
            "顧客情報はまだ流出していない。",
            "顧客情報流出の可能性があるんだって",
        ] {
            assert!(
                find_asserted_speculation(LEAK, output).is_empty(),
                "{output}"
            );
        }
    }

    #[test]
    fn source_that_asserts_the_event_somewhere_is_not_reported() {
        // 材料のどこかで言い切られていれば報告しない（見出しの名詞のままの出現も言い切りとみなす）。
        for source in [
            "あおば通販、顧客情報が流出\n顧客情報が流出した可能性が高いと発表した。",
            "顧客情報が流出した可能性があるとしていたが、調査の結果、流出したと発表した。",
        ] {
            assert!(
                find_asserted_speculation(source, "顧客情報が流出した。").is_empty(),
                "{source}"
            );
        }
        // 材料に語がなければ見ない（材料にない内容は別の点検の対象）。
        assert!(find_asserted_speculation(LEAK, "店舗の営業を停止した。").is_empty());
        // 見出しの「〜か」は推測として数える。
        assert_eq!(
            kinds(&find_asserted_speculation(
                "あおば通販で顧客情報流出か\n同社が調べている。",
                "あおば通販で顧客情報が流出した。"
            )),
            vec![IssueKind::AssertedSpeculation]
        );
    }

    #[test]
    fn negated_outputs_are_not_reported() {
        // 独立レビューの指摘: 言い切りの続きの後ろの打ち消しを言い切りとしない（架空の文）。
        for (source, output) in [
            (
                "再開の時期は年内となる見通し。",
                "再開の時期が決まっていない",
            ),
            (
                "顧客情報が流出した可能性がある。",
                "顧客情報は流出していません。",
            ),
            ("法案は今国会で成立する見通し。", "法案が成立しなかった。"),
            ("停電が起きる恐れがある。", "停電が起きていない。"),
        ] {
            assert!(
                find_asserted_speculation(source, output).is_empty(),
                "{source} / {output}"
            );
        }
        // 打ち消しでない続きは従来どおり拾う。
        assert_eq!(
            kinds(&find_asserted_speculation(
                "法案は今国会で成立する見通し。",
                "法案が成立した。"
            )),
            vec![IssueKind::AssertedSpeculation]
        );
    }

    #[test]
    fn reported_facts_and_intentions_are_not_reported() {
        // 実記事・架空の判決記事の評価で誤検出になった形（架空の文で書いた）。
        for (source, output) in [
            // 「〜したという」は記者が確かめた事実の普通の書き方なので、推測にしない。
            (
                "あおば楽器は、模倣品の販売元に警告書を送付したという。",
                "あおば楽器は模倣品の販売元に警告書を送付した。",
            ),
            // 考え・つもりを表す語（検討など）は、出来事の言い切りとみなさない。
            (
                "団体側は控訴を検討するとしている。",
                "控訴を検討している団体が今後に注目している",
            ),
            // 出力が求めの形のまま書いたもの。
            (
                "指針は、利用者の意図を理解した上で行動するよう求めている。",
                "利用者の意図を理解した上で行動することが求められた。",
            ),
        ] {
            assert!(
                find_asserted_speculation(source, output).is_empty(),
                "{source} / {output}"
            );
        }
    }

    #[test]
    fn asserting_a_complaint_or_a_party_claim_is_reported() {
        // 「〜と訴えている」を「〜した」と書いた形（架空の文）。
        let complaint = "住民、工場の騒音で体調悪化と訴え\n工場の近くの住民は、夜間の騒音で体調が悪化したと訴えている。";
        assert_eq!(
            kinds(&find_asserted_speculation(
                complaint,
                "工場の騒音で住民の体調が悪化した。"
            )),
            vec![IssueKind::AssertedSpeculation]
        );
        assert!(
            find_asserted_speculation(complaint, "住民は騒音で体調が悪化したと訴えた。").is_empty()
        );
        // D169 の A10 を簡単にした形（架空の文）: 党の主張を「決まった」と書く。材料に「合意」があり、
        // 反対の意味の点検（決定⇐主張）では拾えない。
        let claim = "あおば党、保育料の無償化を主張\nあおば党の代表は会見で、来年度から保育料を無償化すると主張した。与党とは協議を続けることで合意した。";
        let said = "来年度から保育料の無償化が決まったんだね";
        assert!(find_contradictions(claim, said).is_empty());
        assert_eq!(
            kinds(&check_stage_consistency(claim, said)),
            vec![IssueKind::AssertedSpeculation]
        );
        assert!(
            check_stage_consistency(claim, "あおば党は保育料の無償化を主張したんだね").is_empty()
        );
    }

    #[test]
    fn asserted_speculation_triggers_the_summary_retry_but_does_not_block_use() {
        let report = super::super::output_check::check_japanese_summary(
            LEAK,
            "あおば通販で顧客約2万人分の情報が流出した。",
            None,
            &[],
        );
        assert_eq!(
            report.kinds_where(IssueKind::triggers_summary_retry),
            vec![IssueKind::AssertedSpeculation]
        );
        assert!(!report.blocks_use());
    }

    /// 架空の記事: 火災の原因が「漏電とみられる」記事（見出しは「〜か」）。
    const FIRE: &str = "あおば市の住宅火災、原因は漏電か\nあおば市で26日、住宅1棟が全焼した。出火の原因は台所の配線の漏電とみられる。けが人はいなかった。";

    fn forms(source: &str, output: &str) -> Vec<(SpeculationForm, bool)> {
        speculation_findings(source, output)
            .into_iter()
            .map(|finding| (finding.form, finding.mixed))
            .collect()
    }

    #[test]
    fn asserting_a_presumed_noun_or_its_certainty_is_reported() {
        // Firestore タスク RWW00G: 「〜とみられる」を「〜だ」「〜と判明」「〜の可能性を断定」と書いた形（架空の文）。
        for (output, form) in [
            ("出火の原因は漏電だ。", SpeculationForm::Copula),
            ("原因は台所の配線の漏電でした。", SpeculationForm::Copula),
            ("火災の原因は漏電である。", SpeculationForm::Copula),
            ("出火の原因は漏電と判明した。", SpeculationForm::Certainty),
            (
                "出火の原因は漏電だと判明しました。",
                SpeculationForm::Certainty,
            ),
            (
                "消防は出火原因を漏電と断定した。",
                SpeculationForm::Certainty,
            ),
            ("消防は漏電の可能性を断定した。", SpeculationForm::Certainty),
        ] {
            assert_eq!(forms(FIRE, output), vec![(form, false)], "{output}");
            assert_eq!(
                kinds(&find_asserted_speculation(FIRE, output)),
                vec![IssueKind::AssertedSpeculation],
                "{output}"
            );
        }
    }

    #[test]
    fn hedged_or_negated_noun_paraphrases_are_not_reported() {
        for output in [
            "出火の原因は漏電とみられる。",
            "原因は漏電の可能性がある。",
            "原因は漏電かもしれない。",
            "原因は漏電だろう。",
            "原因は漏電とされる。",
            "原因は漏電だったのか",
            "消防によると、原因は漏電だ。",
            "原因は漏電と判明していない。",
            "原因が漏電かどうかは確認されていない。",
            "漏電の可能性が判明した。",
            "漏電だけが原因とは限らない。",
            // 独立レビューの指摘: 推測の言い方が主語・主題にかかる文、推測の副詞、丁寧な伝聞、別の語にかかる推測。
            "火災の原因とみられるのは漏電だ。",
            "出火原因とみられるのは、配線の漏電です。",
            "原因として疑われているのは漏電です。",
            "原因はおそらく漏電だ。",
            "原因はたぶん漏電だ。",
            "消防によりますと、原因は漏電です。",
            "火元は台所だ。",
            "出火したのは台所です。",
        ] {
            assert!(speculation_findings(FIRE, output).is_empty(), "{output}");
        }
        for output in [
            "流出した可能性があるのは顧客情報だ。",
            "流出したおそれがあるのは顧客情報です。",
        ] {
            assert!(speculation_findings(LEAK, output).is_empty(), "{output}");
        }
        // 容疑の名前を述べた文（材料「窃盗の疑いで逮捕」）。
        for output in [
            "男の容疑は窃盗です。",
            "逮捕容疑は窃盗だ。",
            "男は窃盗の疑いで逮捕された。容疑は窃盗だ。",
            "男の容疑は、窃盗です。",
            "男の容疑は乗用車の窃盗です。",
            "容疑は車の窃盗だ。",
        ] {
            assert!(speculation_findings(ARREST, output).is_empty(), "{output}");
        }
        // 材料が言い切っていれば報告しない。
        let asserted = "出火の原因は台所の配線の漏電とみられていたが、消防は漏電と断定した。";
        for output in [
            "原因は漏電だ。",
            "原因は漏電と判明した。",
            "消防は漏電の可能性を断定した。",
        ] {
            assert!(
                speculation_findings(asserted, output).is_empty(),
                "{output}"
            );
        }
        // 材料の主張・「〜とした」は、名詞の言い切りでは推測として数えない。
        for (source, output) in [
            ("判決は、解雇は不当とした。", "解雇は不当だ。"),
            ("団体は、表示は違法だと主張している。", "表示は違法である。"),
            ("会社は、対象を正社員とした。", "対象は正社員だ。"),
        ] {
            assert!(
                speculation_findings(source, output).is_empty(),
                "{source} / {output}"
            );
        }
    }

    /// 架空の記事: 窃盗の疑いでの逮捕（容疑者は否認）。
    const ARREST: &str = "あおば署、車盗んだ疑いで男を逮捕\nあおば署は、駐車場で乗用車を盗んだとして、無職の男を窃盗の疑いで逮捕した。男は容疑を否認しているという。";

    #[test]
    fn asserting_a_suspected_or_claimed_kana_verb_is_reported() {
        for output in [
            "無職の男が駐車場で乗用車を盗んだ。",
            "男は車を盗んだんだって思うと怖いね",
        ] {
            // 「って」が語より後ろにあるので、2つ目は言い切りでない。
            let expected = if output.contains("って") {
                vec![]
            } else {
                vec![(SpeculationForm::KanaVerb, false)]
            };
            assert_eq!(forms(ARREST, output), expected, "{output}");
        }
        // 複合動詞（「だまし取った」「受け取った」）。
        let fraud = "男は、高齢の女性から現金をだまし取ったとして逮捕された。補助金を不正に受け取った疑いもある。";
        for output in [
            "男は女性から現金をだまし取った。",
            "男は補助金を不正に受け取った。",
        ] {
            assert_eq!(
                forms(fraud, output),
                vec![(SpeculationForm::KanaVerb, false)],
                "{output}"
            );
        }
        // 「〜と主張」を言い切った形。
        let claim = "住民は、工場の騒音で眠りを妨げられたと主張している。";
        assert_eq!(
            forms(claim, "住民は騒音で眠りを妨げられた。"),
            vec![(SpeculationForm::KanaVerb, false)]
        );
    }

    #[test]
    fn hedged_or_supported_kana_verbs_are_not_reported() {
        for output in [
            "男は乗用車を盗んだとして逮捕された。",
            "男は乗用車を盗んだ疑いで逮捕された。",
            "男が車を盗んだ可能性がある。",
            "車を盗んだら罪になる。",
            "男は車を盗んだと疑われている。",
            "警察によると、男は車を盗んだ。",
            "男は駐車場で乗用車を盗み、逃げた。",
        ] {
            assert!(speculation_findings(ARREST, output).is_empty(), "{output}");
        }
        // 材料の別の箇所で言い切られていれば報告しない。
        let seen = format!("{ARREST}防犯カメラには、男が乗用車を盗んだ様子が映っていた。");
        assert!(speculation_findings(&seen, "男が乗用車を盗んだ。").is_empty());
        // 判決・報告の「〜とした」「〜としている」は推測として数えない。
        for (source, output) in [
            (
                "判決は、会社が安全への配慮を怠ったとした。",
                "会社は安全への配慮を怠った。",
            ),
            (
                "同社は、障害の影響は一部の利用者に限られたとしている。",
                "障害の影響は一部の利用者に限られた。",
            ),
        ] {
            assert!(
                speculation_findings(source, output).is_empty(),
                "{source} / {output}"
            );
        }
    }

    #[test]
    fn assertion_after_a_hedge_for_another_event_is_recorded_only() {
        // 推測・伝聞の言い方が語より前にだけある文（別の出来事にかかる）は、記録だけ（作り直しの理由にしない）。
        let output = "あおば通販は不正アクセスを受けたと発表し、顧客情報が流出した。";
        assert_eq!(forms(LEAK, output), vec![(SpeculationForm::Verb, true)]);
        assert!(find_asserted_speculation(LEAK, output).is_empty());
        for output in [
            "あおば通販は、顧客情報が流出した可能性があると発表した。",
            "あおば通販によれば、顧客情報が流出した。",
            "不正アクセスを受けたと発表し、顧客情報が流出したかもしれない。",
        ] {
            assert!(speculation_findings(LEAK, output).is_empty(), "{output}");
        }
    }

    #[test]
    fn assertions_after_a_hedge_that_is_not_the_topic_are_recorded_only() {
        // 2 回目の独立レビューの指摘: 推測の言い方そのものが主語で動詞が続く文・別の主題の文は記録する。
        // 3 回目の指摘で、語より前に推測の言い方がある文（mixed）は記録だけにした（作り直しの理由にしない）。
        let motive = format!("{ARREST}動機は金銭目的とみられる。");
        for (source, output, form) in [
            (
                FIRE,
                "漏電の可能性が指摘されたが、原因は漏電だ。",
                SpeculationForm::Copula,
            ),
            (
                LEAK,
                "不正アクセスの可能性が話題となり、顧客情報が流出した。",
                SpeculationForm::Verb,
            ),
            (
                ARREST,
                "車を盗んだとして逮捕された男は、駐車場で乗用車を盗んだ。",
                SpeculationForm::KanaVerb,
            ),
            (
                motive.as_str(),
                "男は窃盗の疑いで逮捕され、動機は金銭目的だ。",
                SpeculationForm::Copula,
            ),
        ] {
            assert_eq!(forms(source, output), vec![(form, true)], "{output}");
            assert!(
                find_asserted_speculation(source, output).is_empty(),
                "{output}"
            );
        }
        // 3 回目の独立レビューの指摘: 推測の言い方が主題の名詞にかかる文。記録されても作り直しの理由にはならない。
        for output in [
            "最も可能性が高いのは漏電だ。",
            "可能性の高い原因は漏電だ。",
            "可能性が高い原因は漏電だ。",
            "疑いの強い原因は漏電です。",
            "原因とみられる台所の配線は漏電です。",
            "原因とみられることは漏電だ。",
        ] {
            assert!(
                find_asserted_speculation(FIRE, output).is_empty(),
                "{output}"
            );
            assert!(
                speculation_findings(FIRE, output)
                    .iter()
                    .all(|finding| finding.mixed),
                "{output}"
            );
        }
        // 連体形＋名詞1語＋は／が（推測の言い方が主題の名詞にかかる）は止める。
        for output in [
            "漏電とされる原因は漏電だ。",
            "出火原因とみられている場所は台所の配線の漏電だ。",
        ] {
            assert!(speculation_findings(FIRE, output).is_empty(), "{output}");
        }
    }

    #[test]
    fn speech_check_reports_broken_words_and_paraphrased_instructions() {
        let source = "みなと市は図書館の開館時間を午後9時まで延長する。";
        assert_eq!(
            kinds(&check_speech_consistency(
                source,
                "図書館がうんこのように夜も開くんだね。"
            )),
            vec![IssueKind::BrokenWord]
        );
        let echo = check_speech_consistency(source, "ゆうこの言葉で1段落だけ話すね。");
        assert_eq!(kinds(&echo), vec![IssueKind::PromptEcho]);
        // 理由は固定の文と一覧の語だけ。
        assert!(echo[0].reason.contains("ゆうこの言葉"));
        // ふつうの再説明・材料にある語は報告しない。要点などの点検（check_stage_consistency）は変えない。
        assert!(check_speech_consistency(source, "図書館が夜9時まで開くんだね。").is_empty());
        assert!(check_speech_consistency(
            "うんこドリルの新刊が発売された。",
            "うんこドリルの新刊が出たんだね。"
        )
        .is_empty());
        assert!(check_stage_consistency(source, "ゆうこの言葉で1段落だけ話すね。").is_empty());

        // 写しの句＋記事の中身の文は後処理で消えず、点検で作り直しになる。
        let text =
            "ゆうこが横で教えてくれる案内役として、図書館は午後9時まで開くんだね。便利だね。";
        let (kept, removed) =
            crate::util::speech_cleanup::strip_instruction_echo_sentences(text, source);
        assert_eq!((kept.as_str(), removed), (text, 0));
        assert_eq!(
            kinds(&check_speech_consistency(source, &kept)),
            vec![IssueKind::PromptEcho]
        );
    }
}
