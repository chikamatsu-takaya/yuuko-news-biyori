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
//!   `OPPOSITE_PAIRS` の固定の一覧（勝ち⇔負け・増加⇔減少・可決⇔否決・決定⇐検討/案 など）。
//!   誤検出を減らすため:
//!   - 材料の側は、出力の語より広い言い方（「勝」の1字など、`also`）がどれか1つでもあれば「材料にもある」とする。
//!   - 打ち消し（「負けなかった」「決めておらず」「未定」「連敗を止めた」）の語は、出力・材料のどちらでも数えない。
//!   - 語の一部が別の意味になる複合語（「勝ち点」「勝負」「決勝」「負担」）は数えない。
//!   - 材料に両方の語があるとき（両チームの勝敗が書かれた記事など）は報告しない。
//! - **年の組み合わせ**（`IssueKind::YearMismatch`）: 出力の文の「2026年」と出来事の語（「契約」など、
//!   `EVENT_WORDS`）の組が、材料のどの文にもないとき。材料ではその語が別の年（「2023年12月に…契約」）と
//!   同じ文にあり、出力の文の年がどれもその語と同じ文に出てこないときだけ報告する
//!   （材料の別の文に「2026年」があるので、値だけを見る数字の点検では拾えなかった）。
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
fn is_label(label: &str) -> bool {
    let label = label.trim();
    !label.is_empty()
        && !label.contains(['。', '、', '「', '」', '(', '（'])
        && LABEL_TAIL_WORDS.iter().any(|word| label.ends_with(word))
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
    },
    OppositePair {
        a: Side {
            terms: &["開始", "始まっ", "始まる", "スタート"],
            also: &["始ま", "始め", "開幕", "開業"],
        },
        b: Side {
            terms: &["終了", "終わっ", "終わる", "打ち切"],
            also: &["終わ", "終え", "閉幕", "廃止"],
        },
        a_only: false,
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
    },
];

/// 語の一部が別の意味になる複合語（反対の意味の語として数えない）。
const MASKED_COMPOUNDS: [&str; 8] = [
    "勝ち点",
    "勝ち越",
    "勝負",
    "決勝",
    "負担",
    "負傷",
    "負債",
    "反対側",
];

/// 語の直後がこれで始まれば打ち消し（「負けなかった」「決めておらず」「勝利を逃した」）とみなして数えない。
/// 「な」1字にしないのは、「増加など」「増加しながら」を打ち消しと読まないため。
const NEGATING_FOLLOWERS: [&str; 30] = [
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
    "されず",
    "されな",
    "ておらず",
    "ていない",
    "ていなかっ",
    "てない",
    "てなかっ",
    "てず",
    "ではな",
    "とはな",
    "はない",
    "はなかっ",
    "はなく",
    "ならず",
    "できず",
    "できな",
    "を逃",
    "を許",
    "を止め",
];

/// 語の直後がこれで始まれば、打ち消しと同じく数えない（「連敗を脱した」「勝利を阻まれた」「可決に至らなかった」）。
const REVERSING_FOLLOWERS: [&str; 3] = ["を脱", "を阻", "に至らな"];

/// 語の直前がこれなら打ち消し（「未定」「不成立」「無敗」）とみなして数えない。
const NEGATING_PREFIXES: [char; 4] = ['未', '不', '非', '無'];

/// 反対の意味の点検。`source` は材料（記事のタイトル＋本文抜粋）、`output` は点検する出力。
pub(super) fn find_contradictions(source: &str, output: &str) -> Vec<CheckIssue> {
    let source = mask_compounds(source);
    let output = mask_compounds(output);
    let mut issues = Vec::new();
    for pair in OPPOSITE_PAIRS {
        if let Some(issue) = contradiction_on(&pair.a, &pair.b, &source, &output) {
            issues.push(issue);
        } else if !pair.a_only {
            if let Some(issue) = contradiction_on(&pair.b, &pair.a, &source, &output) {
                issues.push(issue);
            }
        }
    }
    issues
}

/// 出力が `said` の側を言い、材料は `said` の側を言わず `opposite` の側だけを言うとき、その問題を返す。
fn contradiction_on(
    said: &Side,
    opposite: &Side,
    source: &str,
    output: &str,
) -> Option<CheckIssue> {
    let said_term = said
        .terms
        .iter()
        .find(|term| occurs_affirmed(output, term))?;
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
    Some(CheckIssue {
        kind: IssueKind::Contradiction,
        reason: format!(
            "「{said_term}」と書いたが、記事情報は「{opposite_term}」で、反対の意味になっていた。記事情報のとおりに書く。"
        ),
    })
}

/// 複合語（`MASKED_COMPOUNDS`）を、同じ文字数の記号に置き換える（語の点検で数えないため）。
fn mask_compounds(text: &str) -> String {
    let mut masked = text.to_string();
    for word in MASKED_COMPOUNDS {
        masked = masked.replace(word, &"＿".repeat(word.chars().count()));
    }
    masked
}

/// `term` が打ち消しでない形で `text` に出てくるか。
fn occurs_affirmed(text: &str, term: &str) -> bool {
    text.match_indices(term).any(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = &text[index + term.len()..];
        !before.is_some_and(|c| NEGATING_PREFIXES.contains(&c))
            && !NEGATING_FOLLOWERS
                .iter()
                .chain(REVERSING_FOLLOWERS.iter())
                .any(|follower| after.starts_with(follower))
    })
}

// ---------------------------------------------------------------------------
// 年
// ---------------------------------------------------------------------------

/// 年と同じ文にあるとき、年との組み合わせを照合する出来事の語。
const EVENT_WORDS: [&str; 30] = [
    "契約",
    "発売",
    "発表",
    "公開",
    "開始",
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

fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.split(['。', '\n', '！', '？', '!', '?'])
        .filter(|sentence| !sentence.trim().is_empty())
}

/// 年の組み合わせの点検（`IssueKind::YearMismatch`）。材料にある年だけを見る（材料にない年は、
/// 要約では数字の点検、ほかの段では `find_unknown_years` が拾う）。
pub(super) fn find_year_mismatches(source: &str, output: &str) -> Vec<CheckIssue> {
    let source_sentences: Vec<(Vec<u32>, &str)> = sentences(source)
        .map(|sentence| (years_in(sentence), sentence))
        .collect();
    let source_years: Vec<u32> = source_sentences
        .iter()
        .flat_map(|(years, _)| years.iter().copied())
        .collect();
    let mut issues: Vec<CheckIssue> = Vec::new();
    for sentence in sentences(output) {
        let years: Vec<u32> = years_in(sentence)
            .into_iter()
            .filter(|year| source_years.contains(year))
            .collect();
        if years.is_empty() {
            continue;
        }
        for word in EVENT_WORDS.iter().filter(|word| sentence.contains(**word)) {
            let paired: Vec<u32> = source_sentences
                .iter()
                .filter(|(_, source_sentence)| source_sentence.contains(word))
                .flat_map(|(years, _)| years.iter().copied())
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
    }
    issues
}

/// 材料の数字のどれとも合わない年（要点・再説明・一言用）。
fn find_unknown_years(source: &str, output: &str) -> Vec<CheckIssue> {
    let allowed = japanese_source_numbers(source).allowed;
    let mut issues: Vec<CheckIssue> = Vec::new();
    for year in years_in(output) {
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
        // 本文の中の太字は記号だけを消す。
        let (text, _) = strip_heading_labels("楽天は**サヨナラ負け**だった。");
        assert_eq!(text, "楽天はサヨナラ負けだった。");
    }

    #[test]
    fn text_without_headings_or_with_content_labels_is_unchanged() {
        for text in [
            "市は図書館を延長する。",
            "市の説明：「夜まで開ける」とした。",
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
    fn heading_residue_in_the_middle_is_reported() {
        let residue = find_heading_residue("市は図書館を延長する。感想：うれしいね");
        assert_eq!(residue.map(|issue| issue.kind), Some(IssueKind::Heading));
        assert!(find_heading_residue("市は図書館を延長する。").is_none());
        assert!(find_heading_residue("時刻は10:30に始まる。").is_none());
    }

    #[test]
    fn win_and_loss_reversal_is_detected() {
        // D169 の A2: 記事はサヨナラ負け、一言は「サヨナラ勝ちしたんだね」。
        let source = "楽天、痛恨のサヨナラ負け\n楽天は9回に逆転サヨナラ3ランを浴びて敗れた。";
        let issues = find_contradictions(source, "楽天は安田選手の活躍でサヨナラ勝ちしたんだね");
        assert_eq!(kinds(&issues), vec![IssueKind::Contradiction]);
        assert!(issues[0].reason.contains("勝ち"));
        assert!(issues[0].reason.contains("敗れ") || issues[0].reason.contains("負け"));
        // 正しい言い換えは通る。
        assert!(find_contradictions(source, "楽天はサヨナラ負けしちゃったね").is_empty());
        // 逆向き（記事が勝ち、出力が負け）も拾う。
        let won = "みなとFCが山城FCを2－0で下し、3連勝を飾った。";
        assert_eq!(
            kinds(&find_contradictions(won, "みなとFCは山城FCに敗れた。")),
            vec![IssueKind::Contradiction]
        );
    }

    #[test]
    fn negations_compounds_and_mixed_sources_are_not_reported() {
        let source = "楽天は阪神に5－6でサヨナラ負けした。";
        // 打ち消し。
        assert!(find_contradictions(source, "楽天は勝利を逃した。").is_empty());
        assert!(find_contradictions(source, "楽天は勝てなかった。").is_empty());
        // 複合語（勝ち点・勝ち越し）。
        assert!(find_contradictions(source, "楽天は勝ち越しを許した。").is_empty());
        // 両方の側が材料にある記事（両チームの勝敗）。
        let both = "オリックスは中日に敗れ、阪神は楽天に勝利した。";
        assert!(find_contradictions(both, "阪神が勝ち、オリックスは負けた。").is_empty());
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
    fn decided_versus_proposal_is_detected_one_way() {
        // D169 の A10: 記事は「削減する案」「党として決めておらず」、再説明は「45議席減るって決まった」。
        let source = "比例定数を45議席削減する案を採るなら、将来の削減も法案に明記すべきだと主張した。党として賛否を決めておらず、他党と協議する。";
        let issues = find_contradictions(source, "比例定数が45議席減るって決まったんだね");
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

    #[test]
    fn year_paired_with_a_different_event_is_detected() {
        // D169 の A5: 契約は 2023 年。材料の別の文に 2026 年（年俸の年）がある。
        let source = "2023年12月に球団と12年の超大型契約を結んだ。2024年に500万ドル、2026年に1200万ドルの年俸を受け取った。";
        let issues = find_year_mismatches(source, "山本投手は2026年に球団と超大型契約を結んだ。");
        assert_eq!(kinds(&issues), vec![IssueKind::YearMismatch]);
        assert!(issues[0].reason.contains("2026年"));
        // 正しい組み合わせ・年が複数ある文・出来事の語のない文は通る。
        assert!(find_year_mismatches(source, "2023年に球団と契約した。").is_empty());
        assert!(
            find_year_mismatches(source, "2023年に契約し、2026年に1200万ドルを受け取った。")
                .is_empty()
        );
        assert!(find_year_mismatches(source, "2026年の年俸は1200万ドルだった。").is_empty());
        // 元号の年も読む。
        assert!(
            find_year_mismatches("令和5年に契約を結んだ。", "2023年に契約を結んだ。").is_empty()
        );
    }

    #[test]
    fn stage_check_reports_years_missing_from_the_source() {
        let source = "2023年12月に球団と契約を結んだ。";
        let issues = check_stage_consistency(source, "2025年に契約したんだね");
        assert_eq!(kinds(&issues), vec![IssueKind::UnknownNumber]);
        assert!(check_stage_consistency(source, "2023年に契約したんだね").is_empty());
        // 年でない数字（「3年間」「24歳」）は見ない。
        assert!(check_stage_consistency(source, "3年間で24歳になったんだね").is_empty());
    }
}
