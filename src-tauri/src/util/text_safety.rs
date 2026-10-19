//! AI 出力（要約・再説明・感想・用語解説）を保存・表示する前に使う、表示上危険な文字列の判定と無害化。
//!
//! 詳細設計書 §12.5 / セキュリティ詳細設計書 §7.5 の「HTML・制御文字を含まないか」の確認を、
//! 要約側（summary_service）と用語解説側（dictionary_service）で同じ基準にするために共通化している。
//! 要約側は判定に落ちた出力を拒否して Mock へ切り替え、用語解説側は拒否せず無害化して返す（D62）。
//! Markdown 見出し・区切り行の判定は記事 Markdown の構造に依存するため、ここには置かない（要約側の責務）。

/// HTMLタグらしき並び（`<` の直後が英字・`/`・`!`・`?`）を含むかを判定する。
/// `<script>` `</p>` `<!-- -->` `<?xml` などを拾う。「1 < 2」のような比較表現は対象外。
/// 英字の比較（`a<b`）も拒否側に倒れるが、Mock / 既存値へ切り替わるだけなので安全側として許容する。
pub(crate) fn contains_html_tag(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '<' {
            continue;
        }
        if let Some(next) = chars.peek() {
            if next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?') {
                return true;
            }
        }
    }
    false
}

/// 改行（`\n` `\r`）・タブ以外の制御文字を含むかを判定する。
/// ESC などの制御文字は表示上危険なため拒否対象にする（§12.5「表示上危険な文字列」）。
pub(crate) fn contains_disallowed_control_char(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

/// 出力の種（Mock 出力に埋め込む外部由来・ユーザー由来の文字列）や用語解説の AI 出力（short / detail）を、
/// 上の2つの判定に落ちない形へ無害化する。
///
/// 制御文字は改行（`\n`）・タブ以外を除去し、`<` は全角 `＜` へ置換する（文字数は増えない）。
/// `\r` も除去するのは、種の行分割を `\n` だけで扱えるようにするため。
pub(crate) fn neutralize_html_and_control(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .map(|c| if c == '<' { '＜' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_tag_detects_tag_like_sequences() {
        assert!(contains_html_tag("<script>alert(1)</script>"));
        assert!(contains_html_tag("文末</p>"));
        assert!(contains_html_tag("<!-- comment -->"));
        assert!(contains_html_tag("<?xml version"));
        assert!(contains_html_tag("Vec<String>"));
    }

    #[test]
    fn html_tag_ignores_comparisons_and_fullwidth() {
        assert!(!contains_html_tag("1 < 2"));
        assert!(!contains_html_tag("x <= y"));
        assert!(!contains_html_tag("末尾が<"));
        assert!(!contains_html_tag("Vec＜String>"));
    }

    #[test]
    fn control_char_allows_newline_cr_tab_only() {
        assert!(!contains_disallowed_control_char(
            "一行目\n二行目\r\n\tタブ"
        ));
        assert!(contains_disallowed_control_char("ESC\u{1b}[31m"));
        assert!(contains_disallowed_control_char("NUL\u{0}"));
        assert!(contains_disallowed_control_char("DEL\u{7f}"));
    }

    #[test]
    fn neutralize_makes_text_pass_both_checks() {
        let neutralized = neutralize_html_and_control("Vec<String>\u{1b}\r\n\t<script>");
        assert_eq!(neutralized, "Vec＜String>\n\t＜script>");
        assert!(!contains_html_tag(&neutralized));
        assert!(!contains_disallowed_control_char(&neutralized));
    }
}
