//! クレート内共通の小さなユーティリティ。

/// 先頭の UTF-8 BOM (U+FEFF) を除去する。
///
/// Windows のエディタで設定JSONを手編集した際に付与されることがある BOM のみを
/// 許容するための前処理。BOM 以外は一切変更しないため、破損JSONの判定や
/// fail-close 挙動は呼び出し側のまま維持される。
pub(crate) fn strip_utf8_bom(input: &str) -> &str {
    input.strip_prefix('\u{feff}').unwrap_or(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_leading_bom() {
        assert_eq!(strip_utf8_bom("\u{feff}{}"), "{}");
    }

    #[test]
    fn leaves_non_bom_input_unchanged() {
        assert_eq!(strip_utf8_bom("{}"), "{}");
        assert_eq!(strip_utf8_bom(""), "");
        // 先頭以外の U+FEFF は除去しない
        assert_eq!(strip_utf8_bom("a\u{feff}b"), "a\u{feff}b");
    }
}
