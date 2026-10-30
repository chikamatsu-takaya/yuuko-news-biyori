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
//!   - 出力が両方の側を言っているとき（「停止していた運転を再開した」）は報告しない。
//!   - 勝敗は、出力の語と材料の反対の語の主語（「は」「が」の前の名前）が同じときだけ報告する
//!     （「AがBを下した」を「BはAに敗れた」と言い換えた出力を誤検出しないため）。
//!   - 語の一部が別の意味になる複合語（「勝ち点」「勝負」「決勝」「負担」）は数えない。
//!   - 材料に両方の語があるとき（両チームの勝敗が書かれた記事など）は報告しない。
//! - **年の組み合わせ**（`IssueKind::YearMismatch`）: 出力の文の「2026年」と出来事の語（「契約」など、
//!   `EVENT_WORDS`）の組が、材料のどの文にもないとき。材料ではその語が別の年（「2023年12月に…契約」）と
//!   同じ文にあり、出力の文の年がどれもその語と同じ文に出てこないときだけ報告する
//!   （材料の別の文に「2026年」があるので、値だけを見る数字の点検では拾えなかった）。出来事の語が2つ以上ある
//!   文と、材料にある年月日（記事の日付）として書かれた年は見ない。
//! - **材料にない年**（要点・再説明・一言用。`IssueKind::UnknownNumber`）: 「2025年」のような年が材料の
//!   数字のどれとも合わないとき。要約は `output_check` の数字の点検が年も含めて見ているので使わない。
//!
//! 理由（`CheckIssue::reason`）に入る出力由来の文字は、固定の一覧の語と数字だけ（記事の文は入れない）。

use super::output_check::{
    contains_value, japanese_source_numbers, parse_number_at, to_halfwidth, CheckIssue, IssueKind,
};

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
    OppositePair {
        a: Side {
            terms: &["勝利", "勝ち", "勝っ", "白星", "連勝", "優勝"],
            also: &["勝"],
        },
        b: Side {
            terms: &[
                "敗れ", "敗戦", "敗北", "負け", "黒星", "惜敗", "完敗", "連敗", "敗退",
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
const MASKED_COMPOUNDS: [&str; 15] = [
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
];

/// 語の直後がこれで始まれば打ち消し（「負けなかった」「決めておらず」「決まっていません」）とみなして数えない。
/// 「な」1字にしないのは、「増加など」「増加しながら」を打ち消しと読まないため。
/// 判決の言い方の「違法性はない」「違法とまではいえない」と、求めただけでまだ起きていない
/// 「請求を退けるよう求めた」「権利を認めることを求めた」、不起訴の一種の「起訴猶予」も数えない（判決の語の組。§9.6）。
const NEGATING_FOLLOWERS: [&str; 45] = [
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
];

/// 語の直後がこれで始まれば、打ち消しと同じく数えない（「勝利を逃した」「連敗を脱した」「決定を見送った」）。
/// ただし、その後ろがさらに打ち消し（`DOUBLE_NEGATING_TAILS`）なら二重の打ち消しで、語の意味のまま
/// （「勝ちを逃さなかった」「勝利を逃さず」「連敗を止められなかった」は数える）。
const REVERSING_FOLLOWERS: [&str; 6] = ["を逃", "を許", "を止め", "を脱", "を阻", "を見送"];

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
    let source = mask_compounds(source);
    let output = mask_compounds(output);
    let mut issues = Vec::new();
    for pair in OPPOSITE_PAIRS {
        if let Some(issue) = contradiction_on(pair, &pair.a, &pair.b, &source, &output) {
            issues.push(issue);
        } else if !pair.a_only {
            if let Some(issue) = contradiction_on(pair, &pair.b, &pair.a, &source, &output) {
                issues.push(issue);
            }
        }
    }
    issues
}

/// 出力が `said` の側を言い、材料は `said` の側を言わず `opposite` の側だけを言うとき、その問題を返す。
/// 出力が両方の側を言っているとき（「停止していた運転を再開した」）は報告しない。
fn contradiction_on(
    pair: &OppositePair,
    said: &Side,
    opposite: &Side,
    source: &str,
    output: &str,
) -> Option<CheckIssue> {
    let said_term = said
        .terms
        .iter()
        .find(|term| occurs_affirmed(output, term))?;
    if opposite
        .terms
        .iter()
        .any(|term| occurs_affirmed(output, term))
    {
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
    let opposite_term = opposite
        .terms
        .iter()
        .find(|term| occurs_affirmed(source, term))?;
    if pair.subject_bound && !same_subject(said, opposite, source, output) {
        return None;
    }
    Some(CheckIssue {
        kind: IssueKind::Contradiction,
        reason: format!(
            "「{said_term}」と書いたが、記事情報は「{opposite_term}」で、反対の意味になっていた。記事情報のとおりに書く。"
        ),
    })
}

/// 勝敗の組（`subject_bound`）で、出力の語と材料の反対の語が同じ主語（チーム・人の名前）について言っているか。
/// 「AがBを下した」記事を「BはAに敗れた」と言い換えた出力（主語の入れ替え）を、反対の意味と誤検出しないため。
/// - 主語の候補は、語の前で最も近い「は」「が」の直前の名前と、文の最初の「〜は」の名前（`subjects_at`。
///   「Aは打線がつながらず敗れた」では「打線」と「A」）。
/// - 出力の語に主語の候補が無いとき（「サヨナラ勝ちしたんだね」）は、記事と同じ主語の話とみなす
///   （一言・再説明は主語を省くことが多いため）。
/// - 材料の反対の語に主語の候補があれば名前どうしを（片方がもう片方を含めば同じとみなす）、無ければその語の
///   ある文に出力の主語が出てくるかで比べる。
fn same_subject(said: &Side, opposite: &Side, source: &str, output: &str) -> bool {
    let output_subjects: Vec<Vec<String>> = said
        .terms
        .iter()
        .flat_map(|term| affirmed_positions(output, term).map(|index| subjects_at(output, index)))
        .collect();
    if output_subjects.iter().any(Vec::is_empty) {
        return true;
    }
    let same = |a: &str, b: &str| a.contains(b) || b.contains(a);
    opposite.terms.iter().any(|term| {
        affirmed_positions(source, term).any(|index| {
            let source_subjects = subjects_at(source, index);
            let sentence = sentence_around(source, index);
            output_subjects.iter().flatten().any(|subject| {
                if source_subjects.is_empty() {
                    sentence.contains(subject.as_str())
                } else {
                    source_subjects
                        .iter()
                        .any(|source_subject| same(source_subject, subject))
                }
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
            !before.is_some_and(|c| NEGATING_PREFIXES.contains(&c))
                && !is_negated_after(&text[*index + term.len()..])
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

/// 要約の点検（`output_check::check_japanese_summary`）に足す点検: 見出しの残り・反対の意味・年の組み合わせ。
pub(super) fn check_summary_consistency(source: &str, output: &str) -> Vec<CheckIssue> {
    let mut issues: Vec<CheckIssue> = find_heading_residue(output).into_iter().collect();
    issues.extend(find_contradictions(source, output));
    issues.extend(find_year_mismatches(source, output));
    issues
}

/// 要点・注目ポイント・タグ・再説明・一言の点検: 見出しの残り・反対の意味・材料にない年・年の組み合わせ。
/// 数字全般の照合はしない（再説明・一言は要約・要点を言い換えるので、「3つ」「2人」のような数の数え方で
/// 誤検出しやすい。年だけは D169 で誤りが出たので見る）。
pub(super) fn check_stage_consistency(source: &str, output: &str) -> Vec<CheckIssue> {
    let mut issues = check_summary_consistency(source, output);
    issues.extend(find_unknown_years(source, output));
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
}
