//! 実AIの要約・再説明・一言の文頭のつなぎの語（「これにより、」「その結果、」）を消す（Firestore タスク I9iRpb）。
//!
//! 架空の評価データ（D169）でも実記事（D171・D175）でも、小さいローカルLLM は前の文と次の文を「これにより」
//! 「その結果」でつなぎ、記事に書かれていない因果を言い足した（文の中身は記事にあっても、因果だけが誤りになる）。
//! 語と直後の読点だけを消し、文は残す。追加の AI 呼び出しはしない（純粋関数。通信・ファイル操作なし）。
//! 保留ブランチ `feature/summary-length-by-article-zyfz3f`（D167-C・不採用）の `sentence_support.rs` の
//! つなぎの語の処理だけを移したもの（評価・見通しの語の文を落とす処理は移していない）。
//!
//! 消さないもの（どれも、消すと文が壊れる・意味が変わるおそれがある）:
//! - **1文目の頭**: 出力の中に前の文がないので、出力の文どうしの因果の言い足しではない。
//! - **かぎかっこ「」『』の中**: 引用（人の発言）を書き換えない。
//! - **語の一部**: 直後が読点・空白でないときは消さない（「この結果発表」「このための」「これを受け入れ」）。
//!   ただし、後ろに何が続いてもつなぎの語にしかならない「これにより」「これによって」「これを受けて」は、
//!   直後が内容の字（漢字・カタカナ・英数字。「これによりチームは」）でも消す。
//! - **記事自体が同じ因果を書いているとき**（材料の同じ位置に同じつなぎの語がある）: 材料（タイトル＋本文抜粋）に
//!   同じ組のつなぎの語（「これにより・これによって」「このため・そのため」「この結果・その結果」
//!   「これを受け・これを受けて」）があり、材料でその語から文の終わりまでの部分と、出力でその語の後ろの文が
//!   内容（2字の連なり）を `SOURCE_SHARED_MIN` 以上共有するとき。出力の因果が記事の因果の写しなので残す。
//! - つなぎの語のほかに中身のない文（「その結果。」）。

/// 文頭のつなぎの語。`bare_ok` は、直後が読点・空白でなく内容の字でも消してよい語（語の一部になりえない語）。
struct Connector {
    word: &'static str,
    /// 同じ意味の語の組（材料の照合に使う）。
    family: &'static [&'static str],
    bare_ok: bool,
}

const FAMILY_THEREBY: &[&str] = &["これにより", "これによって"];
const FAMILY_THEREFORE: &[&str] = &["このため", "そのため"];
const FAMILY_AS_A_RESULT: &[&str] = &["この結果", "その結果"];
const FAMILY_IN_RESPONSE: &[&str] = &["これを受けて", "これを受け"];

/// 長いものから並べる（「これを受けて」を「これを受け」より先に）。
/// 「このため」「そのため」は直後に「に」「の」が続くと目的の言い方（「このために」）、漢字が続くと別の語
/// （「このため池」）になりうるので、読点・空白が続くときだけ消す。「この結果」「その結果」は名詞句
/// （「この結果発表」「その結果を」）、「これを受け」は動詞の一部（「これを受け入れ」「これを受けた」）になりうる。
const CONNECTORS: [Connector; 8] = [
    Connector {
        word: "これを受けて",
        family: FAMILY_IN_RESPONSE,
        bare_ok: true,
    },
    Connector {
        word: "これによって",
        family: FAMILY_THEREBY,
        bare_ok: true,
    },
    Connector {
        word: "これを受け",
        family: FAMILY_IN_RESPONSE,
        bare_ok: false,
    },
    Connector {
        word: "これにより",
        family: FAMILY_THEREBY,
        bare_ok: true,
    },
    Connector {
        word: "このため",
        family: FAMILY_THEREFORE,
        bare_ok: false,
    },
    Connector {
        word: "そのため",
        family: FAMILY_THEREFORE,
        bare_ok: false,
    },
    Connector {
        word: "この結果",
        family: FAMILY_AS_A_RESULT,
        bare_ok: false,
    },
    Connector {
        word: "その結果",
        family: FAMILY_AS_A_RESULT,
        bare_ok: false,
    },
];

/// つなぎの語の直後で一緒に消す区切り（読点・空白）。
const SEPARATORS: [char; 5] = ['、', '，', ',', ' ', '　'];

/// 文の終わりの字。
const SENTENCE_ENDS: [char; 5] = ['。', '！', '？', '!', '?'];

/// 材料のつなぎの語の後ろの部分と、出力の文が共有する内容（2字の連なり）の最小数。これ以上なら、記事の同じ因果の
/// 写しとみなして消さない。根拠: 記事の文を言い換えた出力でも、出来事の語（「試合」「中止」）を2つは共有する。
/// 1 にすると「市」「日」のような1字の連なりでも一致してしまう。
const SOURCE_SHARED_MIN: usize = 2;

/// 文頭のつなぎの語と直後の読点・空白を消した出力と、消した数を返す。`source` は点検の材料（タイトル＋本文抜粋）。
pub(super) fn strip_leading_connectors(source: &str, text: &str) -> (String, usize) {
    let source_norm = normalize(source);
    let mut out = String::with_capacity(text.len());
    let mut removed = 0;
    let mut quote_depth = 0usize;
    // 1文目の頭は見ない（出力の中に前の文がない）。
    let mut at_sentence_start = false;
    // 先頭の空白・改行の後もまだ1文目の頭。
    let mut started = false;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if !c.is_whitespace() {
            started = true;
        }
        if at_sentence_start && !c.is_whitespace() {
            at_sentence_start = false;
            if quote_depth == 0 {
                if let Some(skip) = removable_connector_len(&source_norm, rest) {
                    rest = &rest[skip..];
                    removed += 1;
                    continue;
                }
            }
        }
        match c {
            '「' | '『' => quote_depth += 1,
            '」' | '』' => quote_depth = quote_depth.saturating_sub(1),
            '\n' => at_sentence_start = started,
            c if SENTENCE_ENDS.contains(&c) => at_sentence_start = true,
            _ => {}
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    (out, removed)
}

/// `rest` がつなぎの語（`CONNECTORS`）として使われた語で始まるなら、その語と、語＋直後の読点・空白の後ろ
/// （`body`）。語の一部（「この結果発表」「これを受け入れ」）は None。
fn connector_match(rest: &str) -> Option<(&'static Connector, &str)> {
    CONNECTORS.iter().find_map(|connector| {
        let after = rest.strip_prefix(connector.word)?;
        let body = after.trim_start_matches(SEPARATORS);
        let separated = body.len() < after.len();
        let bare = connector.bare_ok && after.chars().next().is_some_and(is_content_char);
        (separated || bare).then_some((connector, body))
    })
}

/// `connector_match` の語だけを返す版。文の途中の因果の点検（`causal_check`）と共有する。
pub(super) fn connector_at(rest: &str) -> Option<(&'static str, &str)> {
    connector_match(rest).map(|(connector, body)| (connector.word, body))
}

/// `rest`（文の頭から後ろ）が消せるつなぎの語で始まるなら、消す長さ（語＋直後の読点・空白のバイト数）。
fn removable_connector_len(source_norm: &str, rest: &str) -> Option<usize> {
    let (connector, body) = connector_match(rest)?;
    let sentence = sentence_body(body);
    // つなぎの語のほかに中身のない文（「その結果。」）は残す。
    if !sentence
        .chars()
        .any(|c| !c.is_whitespace() && !SEPARATORS.contains(&c))
    {
        return None;
    }
    if source_has_same_cause(source_norm, connector.family, &normalize(sentence)) {
        return None;
    }
    Some(rest.len() - body.len())
}

/// 文の終わり（句点・感嘆符・疑問符・改行）の手前まで。
fn sentence_body(text: &str) -> &str {
    let end = text
        .find(|c: char| c == '\n' || SENTENCE_ENDS.contains(&c))
        .unwrap_or(text.len());
    &text[..end]
}

/// 材料に同じ組のつなぎの語があり、その後ろ（文の終わりまで）が出力の文と内容を共有するか。
fn source_has_same_cause(source_norm: &str, family: &[&str], sentence_norm: &str) -> bool {
    let sentence_grams = content_bigrams(sentence_norm);
    family.iter().any(|word| {
        source_norm.match_indices(word).any(|(index, _)| {
            let after = sentence_body(&source_norm[index + word.len()..]);
            let source_grams = content_bigrams(after);
            sentence_grams
                .iter()
                .filter(|gram| source_grams.contains(gram))
                .count()
                >= SOURCE_SHARED_MIN
        })
    })
}

/// 照合用に正規化する: 全角英数記号を半角に、英字を小文字に、空白を除く。
pub(super) fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// 内容の字（漢字・カタカナ・英数字）か。ひらがな・記号は語の区切りとして扱う。
pub(super) fn is_content_char(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '々' | '〆' | '\u{30A0}'..='\u{30FF}')
        || c.is_ascii_alphanumeric()
        || matches!(c, 'Ａ'..='Ｚ' | 'ａ'..='ｚ' | '０'..='９')
}

/// 内容の字の連なりごとの2字の連なり（1字だけの連なりは数えない）。重複は除く。
pub(super) fn content_bigrams(text: &str) -> Vec<String> {
    let mut grams: Vec<String> = Vec::new();
    let mut run: Vec<char> = Vec::new();
    for c in text.chars().chain(std::iter::once('。')) {
        if is_content_char(c) {
            run.push(c);
            continue;
        }
        grams.extend(run.windows(2).map(|pair| pair.iter().collect::<String>()));
        run.clear();
    }
    grams.sort();
    grams.dedup();
    grams
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "みなと市は20日、来年度から小中学校の給食費を無償化すると発表した。\
        対象は約1万8000人で、市の負担は年間約12億円になる。";

    fn strip(source: &str, text: &str) -> (String, usize) {
        strip_leading_connectors(source, text)
    }

    #[test]
    fn each_connector_and_its_comma_are_removed_and_the_sentence_is_kept() {
        for connector in [
            "これにより",
            "これによって",
            "このため",
            "そのため",
            "この結果",
            "その結果",
            "これを受け",
            "これを受けて",
        ] {
            let text = format!(
                "市は給食費の無償化を発表した。{connector}、市の負担は年間約12億円になる。"
            );
            assert_eq!(
                strip(SOURCE, &text),
                (
                    "市は給食費の無償化を発表した。市の負担は年間約12億円になる。".to_string(),
                    1
                ),
                "{connector}"
            );
        }
    }

    #[test]
    fn every_later_sentence_and_speech_endings_are_handled() {
        let text = "市は無償化を発表したよ！その結果、約1万8000人が対象になるんだって。\
            これにより、市の負担は約12億円になるね？";
        assert_eq!(
            strip(SOURCE, text),
            (
                "市は無償化を発表したよ！約1万8000人が対象になるんだって。市の負担は約12億円になるね？"
                    .to_string(),
                2
            )
        );
        // 改行のあとの文・空白で区切った文。全角の読点・半角の空白も一緒に消す。
        assert_eq!(
            strip(SOURCE, "市は無償化を発表した。\nこのため，対象は広い。"),
            ("市は無償化を発表した。\n対象は広い。".to_string(), 1)
        );
        assert_eq!(
            strip(
                SOURCE,
                "市は無償化を発表した。 これにより Fender は動いた。"
            ),
            ("市は無償化を発表した。 Fender は動いた。".to_string(), 1)
        );
    }

    #[test]
    fn connector_that_cannot_be_part_of_a_word_is_removed_without_a_comma() {
        assert_eq!(
            strip(
                SOURCE,
                "市は無償化を発表した。これによりチームは強くなった。"
            ),
            ("市は無償化を発表した。チームは強くなった。".to_string(), 1)
        );
        assert_eq!(
            strip(SOURCE, "市は無償化を発表した。これを受けて国も動いた。"),
            ("市は無償化を発表した。国も動いた。".to_string(), 1)
        );
    }

    #[test]
    fn connector_in_the_first_sentence_is_kept() {
        let text = "その結果、市は給食費を無償化した。これにより、負担が減る。";
        assert_eq!(
            strip(SOURCE, text),
            (
                "その結果、市は給食費を無償化した。負担が減る。".to_string(),
                1
            )
        );
        // 先頭の空白・改行のあとも1文目。
        assert_eq!(strip(SOURCE, "\nこのため、市は発表した。").1, 0);
    }

    #[test]
    fn connector_inside_quotes_is_kept() {
        let text = "市長は「財源は確保した。このため、無償化できる。」と述べた。『その結果、負担が減る』とも話した。";
        assert_eq!(strip(SOURCE, text), (text.to_string(), 0));
        // かぎかっこを閉じた後の文は消す。
        let text = "市長は「無償化する。」と述べた。その結果、反響が広がった。";
        assert_eq!(
            strip(SOURCE, text),
            (
                "市長は「無償化する。」と述べた。反響が広がった。".to_string(),
                1
            )
        );
    }

    #[test]
    fn connector_that_is_part_of_a_longer_phrase_is_kept() {
        for text in [
            "市は無償化を発表した。この結果発表は20日だった。",
            "市は無償化を発表した。その結果を市議会に報告した。",
            "市は無償化を発表した。このための財源は12億円だ。",
            "市は無償化を発表した。そのために予算を組んだ。",
            "市は無償化を発表した。このため池は古い。",
            "市は無償化を発表した。これを受け入れた家庭は多い。",
            "市は無償化を発表した。これを受けた国の対応が注目される。",
            "市は無償化を発表した。その結果として負担が減った。",
            "市は無償化を発表した。これにはいくつかの理由がある。",
        ] {
            assert_eq!(strip(SOURCE, text), (text.to_string(), 0), "{text}");
        }
    }

    #[test]
    fn connector_only_sentence_is_kept() {
        let text = "市は無償化を発表した。その結果。";
        assert_eq!(strip(SOURCE, text), (text.to_string(), 0));
        let text = "市は無償化を発表した。その結果、。";
        assert_eq!(strip(SOURCE, text), (text.to_string(), 0));
    }

    #[test]
    fn connector_also_written_by_the_article_for_the_same_effect_is_kept() {
        // 記事自体が「このため」で同じ因果を書いている。
        let source =
            "台風10号が接近している。このため、みなと市は20日の花火大会を中止すると決めた。";
        let text = "台風10号が近づいている。このため、花火大会は中止になった。";
        assert_eq!(strip(source, text), (text.to_string(), 0));
        // 同じ組の別の語（記事「そのため」・出力「このため」）でも、記事の因果の写しとして残す。
        let source_other =
            "台風10号が接近している。そのため、みなと市は花火大会を中止すると決めた。";
        assert_eq!(strip(source_other, text), (text.to_string(), 0));
        // 記事のつなぎの語が別の事柄に付いているときは、出力の因果は記事にないので消す。
        let unrelated = "台風10号が接近している。このため、鉄道各社は計画運休を検討している。花火大会は中止になった。";
        assert_eq!(
            strip(unrelated, text),
            (
                "台風10号が近づいている。花火大会は中止になった。".to_string(),
                1
            )
        );
        // 記事につなぎの語がなければ消す。
        let plain = "台風10号が接近している。みなと市は花火大会を中止すると決めた。";
        assert_eq!(
            strip(plain, text),
            (
                "台風10号が近づいている。花火大会は中止になった。".to_string(),
                1
            )
        );
    }

    #[test]
    fn text_without_connectors_is_unchanged() {
        let text = "市は給食費を無償化すると発表した。対象は約1万8000人。";
        assert_eq!(strip(SOURCE, text), (text.to_string(), 0));
        assert_eq!(strip(SOURCE, ""), (String::new(), 0));
        // 文の途中のつなぎの語は見ない（文頭だけ）。
        let text = "市は無償化を発表し、その結果、負担が減る。";
        assert_eq!(strip(SOURCE, text), (text.to_string(), 0));
    }
}
