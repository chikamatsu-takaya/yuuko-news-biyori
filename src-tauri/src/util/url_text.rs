//! 文字列中の URL（`http://` / `https://` / `www.` で始まる部分）を除く処理。
//!
//! 記事の本文抜粋（RSS の本文由来）には t.co の短縮URLなどがそのまま入ることがあり、画面の抜粋や
//! Mock の要約・要点に URL が並ぶと読みにくい。Mock（ai_provider_service）と、抜粋を画面へ返す
//! DTO 変換（article_repository）で同じ「どこまでが URL か」の基準を使うために共通化している。
//! URL は空白か ASCII 以外の文字（「。」など）の手前までとみなすので、日本語の文に続けて書かれた
//! URL でも前後の文は残る。

/// URL の始まりとみなす印。
const URL_MARKERS: [&str; 3] = ["http://", "https://", "www."];

/// `text` の中で最初に現れる URL の範囲（バイト位置の `start..end`）を返す。
fn find_url(text: &str) -> Option<(usize, usize)> {
    let start = URL_MARKERS
        .iter()
        .filter_map(|marker| text.find(marker))
        .min()?;
    let len = text[start..]
        .find(|c: char| c.is_whitespace() || !c.is_ascii())
        .unwrap_or(text.len() - start);
    Some((start, start + len))
}

/// 文字列から URL を除く（前後の空白・括弧は触らない）。改行は残す。
/// Mock の要約・要点の組み立て用（#400）。挙動を変えると Mock の文面が変わるため、整形は足さない。
pub(crate) fn remove_urls(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((start, end)) = find_url(rest) {
        kept.push_str(&rest[..start]);
        rest = &rest[end..];
    }
    kept.push_str(rest);
    kept
}

/// 画面に出す本文抜粋から URL を除く（保存済みの値は変えず、DTO へ渡す直前に使う）。
///
/// - URL を含まない行は一切変えない（URL の無い抜粋は従来どおりの文面のまま）。
/// - URL を含む行は、URL を除いたあと、URL だけを囲んでいた括弧（「（https://…）」「(https://…)」など）を
///   空のまま残さないよう取り除き、行末に残った「：」「:」、続いた空白（半角）を詰める。全角の空白・句読点は残す。
/// - URL だけだった行は行ごと除く。URL を除いて全体が空になったら `None`（抜粋なしと同じ扱い）。
pub(crate) fn excerpt_without_urls(text: &str) -> Option<String> {
    if find_url(text).is_none() {
        return Some(text.to_string());
    }
    let lines = text
        .lines()
        .filter_map(|line| {
            if find_url(line).is_none() {
                return Some(line.to_string());
            }
            Some(line_without_urls(line)).filter(|line| !line.is_empty())
        })
        .collect::<Vec<_>>();
    let joined = lines.join("\n");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// URL を含む1行から URL を除き、空になった括弧・行末の「：」・続いた半角空白を詰める。
fn line_without_urls(line: &str) -> String {
    let mut kept = String::with_capacity(line.len());
    let mut rest = line;
    while let Some((start, end)) = find_url(rest) {
        kept.push_str(&rest[..start]);
        let url = &rest[start..end];
        let mut after = &rest[end..];
        // URL だけを囲んでいた括弧は、開き括弧と閉じ括弧をまとめて除く。
        // ASCII の閉じ括弧は URL の一部として読まれる（`(https://x.com/a)` の `)`）ので、その場合も対にする。
        let before = kept.trim_end();
        if let Some(close) = before.chars().last().and_then(closing_bracket) {
            let closed = if url.ends_with(close) {
                true
            } else if let Some(stripped) = after.trim_start().strip_prefix(close) {
                after = stripped;
                true
            } else {
                false
            };
            if closed {
                let open_len = before.chars().last().map_or(0, char::len_utf8);
                kept.truncate(before.len() - open_len);
            }
        }
        rest = after;
    }
    let url_was_last = rest.trim().is_empty();
    kept.push_str(rest);

    let mut tidied = collapse_ascii_spaces(&kept);
    if url_was_last {
        // 「詳しくはこちら：https://…」の「：」だけが行末に残らないようにする。
        let trimmed_len = tidied.trim_end_matches([':', '：']).trim_end().len();
        tidied.truncate(trimmed_len);
    }
    tidied
}

/// 開き括弧に対応する閉じ括弧（URL を囲む使い方がありうるものだけ）。
fn closing_bracket(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '（' => Some('）'),
        '［' => Some('］'),
        '「' => Some('」'),
        '『' => Some('』'),
        '【' => Some('】'),
        '〈' => Some('〉'),
        _ => None,
    }
}

/// 連続する半角空白・タブを1つにし、日本語の句読点・閉じ括弧の直前の半角空白と、行の前後の空白を除く。
fn collapse_ascii_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let is_space = c == ' ' || c == '\t';
        if is_space {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        if matches!(c, '。' | '、' | '，' | '．' | '）' | '」' | '』' | '】') && out.ends_with(' ')
        {
            out.pop();
        }
        out.push(c);
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_urls_keeps_surrounding_text_as_is() {
        assert_eq!(
            remove_urls("新製品を発表した https://t.co/abc。価格は据え置きだ。"),
            "新製品を発表した 。価格は据え置きだ。"
        );
        assert_eq!(remove_urls("a www.example.com b\nhttp://x"), "a  b\n");
    }

    #[test]
    fn excerpt_without_urls_leaves_text_without_urls_unchanged() {
        let text = "  前半の本文。\n\n後半の　本文  です。";
        assert_eq!(excerpt_without_urls(text).as_deref(), Some(text));
    }

    #[test]
    fn excerpt_without_urls_drops_tweet_short_urls() {
        assert_eq!(
            excerpt_without_urls(
                "Deno が Cloudflare に参加。Deno is joining Cloudflare! https://t.co/hC5HOUbtt4 — Deno (@deno_land) October 9"
            )
            .as_deref(),
            Some("Deno が Cloudflare に参加。Deno is joining Cloudflare! — Deno (@deno_land) October 9")
        );
    }

    #[test]
    fn excerpt_without_urls_keeps_japanese_sentences_and_brackets_intact() {
        // 日本語の文に続く URL は、「。」の手前までだけを除き、句点の前の空白も詰める。
        assert_eq!(
            excerpt_without_urls("新製品を発表した https://t.co/abc。価格は据え置き（税込）だ。")
                .as_deref(),
            Some("新製品を発表した。価格は据え置き（税込）だ。")
        );
        // URL だけを囲んでいた括弧は対で除く（全角・半角とも）。中身のある括弧は残す。
        assert_eq!(
            excerpt_without_urls("詳細は公式サイト（https://example.com/a）で公開。").as_deref(),
            Some("詳細は公式サイトで公開。")
        );
        assert_eq!(
            excerpt_without_urls("詳細は公式サイト (https://example.com/a) で公開。").as_deref(),
            Some("詳細は公式サイト で公開。")
        );
        assert_eq!(
            excerpt_without_urls("発表（2026年、https://example.com/a）があった。").as_deref(),
            Some("発表（2026年、）があった。")
        );
        // 行末の「：」は残さない。
        assert_eq!(
            excerpt_without_urls("詳しくはこちら：https://example.com/a").as_deref(),
            Some("詳しくはこちら")
        );
    }

    #[test]
    fn excerpt_without_urls_drops_url_only_lines_and_keeps_other_lines() {
        assert_eq!(
            excerpt_without_urls("1行目の本文。\nhttps://t.co/abc\n　字下げの行。").as_deref(),
            Some("1行目の本文。\n　字下げの行。")
        );
        assert_eq!(
            excerpt_without_urls("https://t.co/abc www.example.com"),
            None
        );
    }
}
