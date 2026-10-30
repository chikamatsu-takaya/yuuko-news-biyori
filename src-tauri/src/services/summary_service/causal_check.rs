//! 文の途中の因果のつなぎ（「〜したことで」「〜したため、」「その結果」など）で、記事にない因果を結んだ出力の点検
//! （Firestore タスク 6jL23k。`IssueKind::UnsupportedCause`）。
//!
//! 小さいローカルLLM は、記事に別々に書かれた2つの事実を「〜したため」「〜したことで」でつなぎ、記事にない因果を
//! 言い足す（実記事の評価・D167-C の段階2b に残った作り話の型。要約の機械点検の評価 §10.4）。文頭のつなぎの語は
//! `leading_connector` が消すが、文の途中のつなぎは消すと文が壊れるので、点検で拾う。純粋関数（通信・ファイル操作なし）。
//!
//! 方法（照らす単位は語。記事の文と出力の文の対応づけはしない）:
//! - 出力の文を、因果のつなぎ（`OUTPUT_LINKS` と、`leading_connector` と共有する「その結果」「これにより」などの語）の
//!   前と後ろに分ける。かぎかっこ「」『』の中は見ない（引用）。
//! - 前と後ろそれぞれの内容の語（漢字・カタカナ・英数字の2字の連なり。`leading_connector::content_bigrams`）のうち、
//!   材料（タイトル＋本文抜粋）にあるものを取る。前と後ろの両方に出る語（主語など）は除く。どちらかが空なら見ない
//!   （材料にない語の作り話は別の点検の対象で、因果だけを見るため）。
//! - 材料の文（文頭のつなぎの語で始まる文は前の文とつなげる。かぎかっこの中の句点では分けない）のうち、因果の
//!   言い方（`SOURCE_CAUSAL_MARKERS`。「ので」「から」「で」「て、」と連用形の読点〔「本格化し、」〕などの言い換えを
//!   含む広い一覧）を含み、前の語と後ろの語の両方を含む文が1つも無いとき報告する。
//!
//! 誤検出を減らすため（報告しない側に倒す）、出力の側のつなぎは因果にしか読めない形に絞る:
//! - 「ため」は過去・形容詞の形（「〜したため」「〜だったため」「多いため」）だけ。目的の「〜するため」「〜のため」
//!   「〜しないため」は見ない（「少ないため」のような漢字＋「ない」は見る）。
//! - 「により」「によって」は直後に読点があるときだけ（「警察により逮捕」のような受け身の行為者・手段を見ない）。
//! - 「〜ことで」は、合意・一致の中身（「〜することで合意した」）を見ない。
//! - 「で」「て」「ので」「から」は出力の側では見ない（意味が広く、因果と言い切れないため）。
//!
//! 理由（`CheckIssue::reason`）に入る出力由来の文字は、固定の一覧のつなぎの語だけ（記事の文は入れない）。

use super::leading_connector::{connector_at, content_bigrams, is_content_char, normalize};
use super::output_check::{CheckIssue, IssueKind};

/// 出力の側で見る、文の途中の因果のつなぎ（「その結果」などの語は `leading_connector::connector_at` で見る）。
struct OutputLink {
    /// つなぎの語（理由の文に入れる）。
    word: &'static str,
    /// 直前の字がこのどれかのときだけ、つなぎとみなす（空なら見ない）。
    after_chars: &'static [char],
    /// 直後がこれで始まるときだけ、つなぎとみなす（空なら見ない）。
    followed_by: &'static [&'static str],
    /// 直後がこれで始まるときは、つなぎとみなさない。
    not_followed_by: &'static [&'static str],
}

/// 直前の字の決まり: 動詞・形容詞の終わり（「する・した・多い」）。
const VERB_ENDS: &[char] = &['る', 'た', 'い'];
/// 直後の読点・空白（「により、」「を受けて、」）。
const COMMA: &[&str] = &["、", "，", ","];
/// 「〜することで合意した」のように、「ことで」が合意・一致の中身を表す続き。「〜したことで知られる」
/// （評判の中身）も入れる。
const AGREEMENT_FOLLOWERS: &[&str] = &[
    "合意",
    "一致",
    "決着",
    "妥結",
    "折り合",
    "話がまとま",
    "和解",
    "同意",
    "了承",
    "調整",
    "知られ",
    "す",
    "し",
    "は",
    "も",
];

/// 「地域によって、異なる」のように、「によって」「により」が「〜ごとに」の意味（違い・差）になる続き
/// （直後の読点を含む。全角の読点は `normalize` で半角になる）。
const DEPENDING_ON: &[&str] = &["、異な", "、違", "、差", ",異な", ",違", ",差"];

/// 長いものから並べる（「ことによって」を「ことで」より先に）。
const OUTPUT_LINKS: [OutputLink; 8] = [
    OutputLink {
        word: "ことによって",
        after_chars: VERB_ENDS,
        followed_by: &[],
        not_followed_by: &[],
    },
    OutputLink {
        word: "ことにより",
        after_chars: VERB_ENDS,
        followed_by: &[],
        not_followed_by: &[],
    },
    OutputLink {
        word: "ことで",
        after_chars: VERB_ENDS,
        followed_by: &[],
        not_followed_by: AGREEMENT_FOLLOWERS,
    },
    OutputLink {
        word: "によって",
        after_chars: &[],
        followed_by: COMMA,
        not_followed_by: DEPENDING_ON,
    },
    OutputLink {
        word: "により",
        after_chars: &[],
        followed_by: COMMA,
        not_followed_by: DEPENDING_ON,
    },
    OutputLink {
        word: "を受けて",
        after_chars: &[],
        followed_by: COMMA,
        not_followed_by: &[],
    },
    OutputLink {
        word: "結果",
        after_chars: &['た'],
        followed_by: COMMA,
        not_followed_by: &[],
    },
    // 「ため」は `cause_tame_at` で直前の形を見る（「したため」「多いため」）。
    OutputLink {
        word: "ため",
        after_chars: &[],
        followed_by: &[],
        not_followed_by: &["の"],
    },
];

/// 材料の文が因果を書いているとみなす言い方。出力の側より広く、言い換え（「〜ので」「〜から」「〜で」「〜て、」
/// 「〜の影響で」「〜をきっかけに」「〜したところ」「〜を背景に」）を含む（材料が別のつなぎで同じ因果を書いた出力を誤検出しないため）。
/// 「〜による」は入れない: 「人間による監督」のような行為者・手段にも使い、入れると記事にない因果の正しい指摘を
/// 消した（実記事の記録で誤検出は減らず、正しい指摘が 2 件消えた。要約の機械点検の評価 §9.9.6）。材料が「円安による
/// 値上がり」と書いた因果を「円安により、」と書いた出力は、既知の誤検出として報告する。
/// 「で」は「地震で通行止め」のような原因の「で」（見出しに多い）を読むために入れる。このほか、動詞の連用形で文を
/// つなぐ読点（「本格化し、」「増え、」。`has_continuative_comma`）も因果の言い方とみなす。
const SOURCE_CAUSAL_MARKERS: [&str; 26] = [
    "ため",
    "ことで",
    "ところ",
    "により",
    "背景",
    "によって",
    "よって",
    "結果",
    "受け",
    "ので",
    "から",
    "で",
    "て、",
    "て,",
    "おかげ",
    "せい",
    "影響",
    "伴い",
    "伴う",
    "つなが",
    "きっかけ",
    "契機",
    "を機に",
    "原因",
    "理由",
    "招い",
];

/// 読点の前がこの字なら、動詞の連用形で文をつないだ形（「本格化し、」「下がり、」「増え、」）とみなす。
/// 助詞の「に、」「へ、」は入れない。
const CONTINUATIVE_ENDS: [char; 17] = [
    'し', 'り', 'き', 'ち', 'い', 'ひ', 'み', 'び', 'ぎ', 'じ', 'え', 'け', 'せ', 'れ', 'め', 'べ',
    'げ',
];

/// 文の区切り（全角・半角の感嘆符・疑問符を含む）。改行も区切る（`split_sentences`）。
const SENTENCE_ENDS: [char; 5] = ['。', '！', '？', '!', '?'];

/// 文の途中の因果のつなぎで、記事にない因果を結んだ出力の点検（`IssueKind::UnsupportedCause`）。
pub(super) fn find_unsupported_causes(source: &str, output: &str) -> Vec<CheckIssue> {
    let source_norm = normalize(source);
    let units: Vec<Vec<String>> = causal_units(source)
        .into_iter()
        .map(|unit| content_bigrams(&unit))
        .collect();
    let in_source = |gram: &String| source_norm.contains(gram.as_str());
    let mut issues: Vec<CheckIssue> = Vec::new();
    for sentence in split_sentences(output) {
        let sentence = normalize(sentence);
        let sentence = sentence.as_str();
        let links = links_in(sentence);
        for (i, (word, start, end)) in links.iter().enumerate() {
            let segment_start = if i == 0 { 0 } else { links[i - 1].2 };
            let segment_end = links.get(i + 1).map_or(sentence.len(), |next| next.1);
            let before: Vec<String> = content_bigrams(&sentence[segment_start..*start])
                .into_iter()
                .filter(in_source)
                .collect();
            let after: Vec<String> = content_bigrams(&sentence[*end..segment_end])
                .into_iter()
                .filter(in_source)
                .collect();
            // 前と後ろの両方に出る語（主語の繰り返しなど）は、どちらの側の語ともみなさない。
            let (before, after): (Vec<&String>, Vec<&String>) = (
                before.iter().filter(|g| !after.contains(g)).collect(),
                after.iter().filter(|g| !before.contains(g)).collect(),
            );
            if before.is_empty() || after.is_empty() {
                continue;
            }
            let supported = units.iter().any(|unit| {
                before.iter().any(|g| unit.contains(g)) && after.iter().any(|g| unit.contains(g))
            });
            if supported {
                continue;
            }
            let reason = format!(
                "「{word}」でつないだ因果は記事情報になかった。記事情報が因果として書いていない出来事どうしを、因果のつなぎの語で結ばない。"
            );
            if !issues.iter().any(|issue| issue.reason == reason) {
                issues.push(CheckIssue {
                    kind: IssueKind::UnsupportedCause,
                    reason,
                });
            }
        }
    }
    issues
}

/// 文に分ける（区切りの字は含めない。空の文は除く）。かぎかっこ「」『』の中の句点では分けない
/// （発言は、引用の外の文の一部として1つの文に数える。話し手が2文で述べた理由と結論を1つの文として照らすため）。
fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences: Vec<&str> = Vec::new();
    let mut quote_depth = 0usize;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match c {
            '「' | '『' => quote_depth += 1,
            '」' | '』' => quote_depth = quote_depth.saturating_sub(1),
            // 改行は引用の中でも区切る（閉じ忘れたかぎかっこで文がつながり続けないため）。
            '\n' => {
                sentences.push(&text[start..index]);
                start = index + c.len_utf8();
                quote_depth = 0;
            }
            c if quote_depth == 0 && SENTENCE_ENDS.contains(&c) => {
                sentences.push(&text[start..index]);
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    sentences.push(&text[start..]);
    sentences.retain(|sentence| !sentence.trim().is_empty());
    sentences
}

/// 読点の前が動詞の連用形の終わり（`CONTINUATIVE_ENDS`）の形を含むか。
fn has_continuative_comma(unit: &str) -> bool {
    let mut previous = None;
    for c in unit.chars() {
        if matches!(c, '、' | '，' | ',')
            && previous.is_some_and(|p| CONTINUATIVE_ENDS.contains(&p))
        {
            return true;
        }
        previous = Some(c);
    }
    false
}

/// 材料の文（正規化したもの）のうち、因果の言い方を含むもの。文頭のつなぎの語（「その結果、」「このため、」）で
/// 始まる文は、前の文とつなげて1つにする（記事が2文に分けて書いた因果）。
fn causal_units(source: &str) -> Vec<String> {
    let mut units: Vec<String> = Vec::new();
    for sentence in split_sentences(source) {
        let sentence = normalize(sentence);
        let sentence = sentence.as_str();
        match units.last_mut() {
            Some(previous) if connector_at(sentence).is_some() => {
                previous.push('。');
                previous.push_str(sentence);
            }
            _ => units.push(sentence.to_string()),
        }
    }
    units.retain(|unit| {
        SOURCE_CAUSAL_MARKERS.iter().any(|m| unit.contains(m)) || has_continuative_comma(unit)
    });
    units
}

/// 文の中の因果のつなぎ（語・始まり・終わりのバイト位置）。かぎかっこの中と文頭は見ない。
fn links_in(sentence: &str) -> Vec<(&'static str, usize, usize)> {
    let mut links: Vec<(&'static str, usize, usize)> = Vec::new();
    let mut quote_depth = 0usize;
    let mut index = 0;
    while let Some(c) = sentence[index..].chars().next() {
        match c {
            '「' | '『' => quote_depth += 1,
            '」' | '』' => quote_depth = quote_depth.saturating_sub(1),
            _ => {}
        }
        if quote_depth == 0 && index > 0 {
            if let Some((word, len)) = link_at(sentence, index) {
                links.push((word, index, index + len));
                index += len;
                continue;
            }
        }
        index += c.len_utf8();
    }
    links
}

/// `sentence` の `index` から因果のつなぎが始まるなら、その語と長さ（バイト数。直後の読点は含めない）。
fn link_at(sentence: &str, index: usize) -> Option<(&'static str, usize)> {
    let rest = &sentence[index..];
    let previous = sentence[..index].chars().next_back();
    if let Some((word, _)) = connector_at(rest) {
        return Some((word, word.len()));
    }
    OUTPUT_LINKS.iter().find_map(|link| {
        let after = rest.strip_prefix(link.word)?;
        if link.word == "ため" && !cause_tame_at(&sentence[..index]) {
            return None;
        }
        let previous_ok =
            link.after_chars.is_empty() || previous.is_some_and(|c| link.after_chars.contains(&c));
        let followed_ok =
            link.followed_by.is_empty() || link.followed_by.iter().any(|f| after.starts_with(f));
        let not_excluded = !link.not_followed_by.iter().any(|f| after.starts_with(f));
        (previous_ok && followed_ok && not_excluded).then_some((link.word, link.word.len()))
    })
}

/// 「ため」の前（`before`）が、理由にしか読めない形か: 過去（「〜したため」「〜だったため」「〜んだため」）、
/// 「〜ている（てる）ため」、形容詞（「多いため」「少ないため」）。目的の「〜するため」「〜のため」と、目的にも読める
/// 「〜しないため」（「な」の前がひらがな）は見ない。
fn cause_tame_at(before: &str) -> bool {
    let mut chars = before.chars().rev();
    match chars.next() {
        Some('た' | 'だ') => true,
        Some('い') => match chars.next() {
            Some('な') => chars.next().is_some_and(is_content_char),
            Some(c) => c != 'た',
            None => false,
        },
        _ => before.ends_with("ている") || before.ends_with("てる"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 架空の記事（部品の流出）。材料は、部品が公開のものだったことと、被害が確認されていないことを別の文で書く。
    const LEAK: &str = "あおば重工の試作部品の図面が外部に流出した。\
        流出した図面は、すでに公開済みの部品のものだった。\
        同社によると、現時点で被害は確認されていない。";

    fn kinds(source: &str, output: &str) -> Vec<IssueKind> {
        find_unsupported_causes(source, output)
            .into_iter()
            .map(|issue| issue.kind)
            .collect()
    }

    #[test]
    fn a_cause_that_joins_two_separate_facts_is_reported() {
        for output in [
            "あおば重工の図面が流出したが、部品は公開済みのものだったため、被害はなかった。",
            "図面は公開済みの部品のものだったため被害は確認されていない。",
            "部品が公開済みだったことで、被害は確認されていない。",
            "図面は公開済みの部品のもので、その結果、被害は確認されていない。",
            "図面は公開済みの部品のものだった結果、被害は確認されていない。",
        ] {
            assert_eq!(
                kinds(LEAK, output),
                vec![IssueKind::UnsupportedCause],
                "{output}"
            );
        }
    }

    #[test]
    fn the_reason_names_only_the_connector() {
        let issues = find_unsupported_causes(LEAK, "部品は公開済みだったため被害はなかった。");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].reason.starts_with("「ため」でつないだ因果"));
        assert!(!issues[0].reason.contains("部品"));
    }

    #[test]
    fn the_same_cause_written_with_another_connector_in_the_source_is_not_reported() {
        let output = "大雨で川が増水したため、みなと線は運転を見合わせた。";
        for source in [
            "大雨で川が増水したので、みなと線は午後から運転を見合わせた。",
            "川が増水したから、みなと線は運転を見合わせた。",
            "大雨による川の増水で、みなと線は運転を見合わせた。",
            "大雨で川が増水した。このため、みなと線は運転を見合わせた。",
            "川の増水を受け、みなと線は運転を見合わせた。",
        ] {
            assert!(kinds(source, output).is_empty(), "{source}");
        }
        // 記事が同じ語で書いている因果。
        assert!(kinds(
            "川が増水したことで、みなと線は運転を見合わせた。",
            "川が増水したことで、みなと線は運転を見合わせた。"
        )
        .is_empty());
    }

    #[test]
    fn non_causal_uses_are_not_counted() {
        for output in [
            // 目的の「ため」。
            "あおば重工は被害を防ぐため、公開済みの部品の図面を調べた。",
            "あおば重工は再発防止のため、被害の調査を進める。",
            "あおば重工は流出を繰り返さないため、図面の管理を見直す。",
            // 受け身の行為者・手段の「により」（読点なし）。
            "図面は外部の人物により流出した。被害は確認されていない。",
            // 合意の中身の「ことで」。
            "両社は公開済みの部品の図面を共有することで合意し、被害の確認を進める。",
            // 引用の中のつなぎ。
            "同社は「部品は公開済みだったため被害はない」と説明した。",
        ] {
            assert!(kinds(LEAK, output).is_empty(), "{output}");
        }
    }

    #[test]
    fn words_missing_from_the_source_or_shared_by_both_sides_are_not_judged() {
        // 前か後ろの語が材料に無い（作り話は別の点検の対象）。
        assert!(kinds(LEAK, "株価が急落したため、被害が確認されていない。").is_empty());
        assert!(kinds(LEAK, "部品が公開済みだったため、株価が急落した。").is_empty());
        // 前と後ろで同じ語しか材料にない（主語の繰り返し）。
        assert!(kinds(LEAK, "あおば重工は流出したため、あおば重工は流出した。").is_empty());
    }

    #[test]
    fn a_cause_written_in_the_same_source_sentence_is_not_reported() {
        let source = "部品は公開済みのものだったため、被害は出ていない。";
        assert!(kinds(source, "部品が公開済みだったことで、被害は出ていない。").is_empty());
    }

    #[test]
    fn continuative_commas_headline_de_and_quoted_reasons_count_as_the_same_cause() {
        // 連用形の読点（「本格化し、」）で書いた因果。
        let source = "新米の出回りが本格化し、価格はゆるやかに下がると見る関係者が多い。";
        assert!(kinds(
            source,
            "新米が出回り始めているため、価格は緩やかに下がる見込み。"
        )
        .is_empty());
        // 見出しの原因の「で」（読点なし）。
        let source =
            "地震で県道が通行止め\n4日、県北部で地震があった。町によると、県道で落石があった。";
        assert!(kinds(source, "4日の地震により、県道が通行止めになった。").is_empty());
        // 話し手が2文で述べた理由と結論（かぎかっこの中の句点では文を分けない）。
        let source =
            "教授は「想定外の手段は以前から指摘されてきた。二重に守る必要がある」と話した。";
        assert!(kinds(
            source,
            "想定外の手段は以前から指摘されてきたため、二重に守る必要がある。"
        )
        .is_empty());
        // かぎかっこの外で文が分かれていれば、話し手の文でも別の文として照らす。
        let source = "教授は「想定外の手段は以前から指摘されてきた」と話した。社は「二重に守る必要がある」とした。";
        assert_eq!(
            kinds(
                source,
                "想定外の手段は以前から指摘されてきたため、二重に守る必要がある。"
            ),
            vec![IssueKind::UnsupportedCause]
        );
    }

    #[test]
    fn connectors_inside_a_quote_with_several_sentences_are_not_counted() {
        let output = "同社は「部品は公開済みだった。そのため、被害はない」と説明した。";
        assert!(kinds(LEAK, output).is_empty());
    }

    #[test]
    fn unsupported_cause_triggers_the_retry_in_every_stage() {
        let output = "部品は公開済みのものだったため、被害はなかった。";
        let report = super::super::output_check::check_japanese_summary(LEAK, output, None, &[]);
        assert_eq!(
            report.kinds_where(IssueKind::triggers_summary_retry),
            vec![IssueKind::UnsupportedCause]
        );
        assert!(!report.blocks_use());
        assert_eq!(
            super::super::consistency_check::check_stage_consistency(LEAK, output)
                .into_iter()
                .map(|issue| issue.kind)
                .collect::<Vec<_>>(),
            vec![IssueKind::UnsupportedCause]
        );
    }

    #[test]
    fn tokoro_and_haikei_in_the_source_count_as_the_same_cause() {
        let source = "市が現地を調査したところ、水道管の破損が判明した。";
        assert!(kinds(source, "市が現地を調査した結果、水道管の破損が判明した。").is_empty());
        // 既知の誤検出: 材料の「〜による」は因果の言い方と数えない（行為者の「人間による監督」と見分けられず、
        // 数えると記事にない因果の正しい指摘を消すため。誤報を拾うことを誤検出より優先する）。
        let source = "円安による輸入品の値上がりが続いている。";
        assert_eq!(
            kinds(source, "円安により、輸入品が値上がりした。"),
            vec![IssueKind::UnsupportedCause]
        );
        let source = "円安を背景に、輸入品の値上がりが続いている。";
        assert!(kinds(source, "円安が続いたため、輸入品が値上がりした。").is_empty());
    }

    #[test]
    fn reputation_and_depending_on_are_not_counted() {
        let source = "あおば工業は小型電池の開発で知られる。同社の業界での順位は3位だ。";
        assert!(kinds(
            source,
            "あおば工業は小型電池を開発したことで知られる業界の会社だ。"
        )
        .is_empty());
        // 同じ材料で、評判でない「ことで」は数える（対照）。
        assert_eq!(
            kinds(
                source,
                "あおば工業は小型電池を開発したことで業界の順位を上げた。"
            ),
            vec![IssueKind::UnsupportedCause]
        );
        let source = "料金は地域で異なる。新しい額は4月に適用する。";
        assert!(kinds(source, "料金は地域によって、異なる額を4月に適用する。").is_empty());
        assert_eq!(
            kinds(source, "料金は地域によって、4月に適用する。"),
            vec![IssueKind::UnsupportedCause]
        );
    }

    #[test]
    fn tame_forms_are_read() {
        assert!(cause_tame_at("増水した"));
        assert!(cause_tame_at("休みだった"));
        assert!(cause_tame_at("参加者が多い"));
        assert!(cause_tame_at("参加者が少ない"));
        assert!(cause_tame_at("調査している"));
        assert!(!cause_tame_at("確保する"));
        assert!(!cause_tame_at("再発防止の"));
        assert!(!cause_tame_at("繰り返さない"));
        assert!(!cause_tame_at("調べたい"));
    }
}
