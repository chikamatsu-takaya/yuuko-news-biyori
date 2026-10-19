//! 外部HTTP応答本文の受信バイト上限（RSS・記事HTML・Gemini で共通に使う判定）。
//!
//! 巨大な応答を全量メモリへ展開しないため、次の二段で上限を守る（セキュリティ詳細設計書 §9.1 / §9.2）。
//! 1. Content-Length の宣言値が上限を超えていれば、本文を読み始める前に拒否する。
//! 2. 宣言がない（chunked 等）・宣言が偽りでも、実際に受け取ったバイト数で上限を守る。
//!
//! reqwest は gzip / brotli / deflate の自動展開機能を有効化していないため、ここで数えるのは
//! 圧縮解凍されない生バイト（＝受信バイト）である。
//! エラーには本文断片・URL・可変情報を含めない（呼び出し側が固定文言の AppError へ変換する）。

/// 受信本文の読み取り失敗の種類（本文・URL を含まない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyReadError {
    /// 宣言値または実受信バイト数が上限を超えた。
    TooLarge,
    /// 通信途中の読み取り失敗（生のエラー文は取り込まない）。
    Read,
}

/// 宣言された Content-Length による早期拒否判定（純粋関数・テスト対象）。
/// 宣言があり `max_bytes` を超える場合のみ true（本文を読み始める前に拒否してよい）。
/// 宣言なし(None・chunked 等)は false を返し、実読み取り制限に委ねる。
pub fn declared_length_exceeds(content_length: Option<u64>, max_bytes: usize) -> bool {
    matches!(content_length, Some(len) if len > max_bytes as u64)
}

/// 受信済みバッファへチャンクを足す。足すと上限を超える場合は足さずに TooLarge を返す
/// （超過分を蓄積しない）。上限ちょうどは成功。判定は文字数ではなくバイト数で行う。
pub fn append_chunk_capped(
    buf: &mut Vec<u8>,
    chunk: &[u8],
    max_bytes: usize,
) -> Result<(), BodyReadError> {
    if buf.len().saturating_add(chunk.len()) > max_bytes {
        return Err(BodyReadError::TooLarge);
    }
    buf.extend_from_slice(chunk);
    Ok(())
}

/// 非同期 reqwest 応答の本文を受信上限付きで読む（RSS / 記事HTML 用）。
/// Content-Length 早期拒否 → `chunk()` で段階的に受信し、上限超過を検出した時点で読み取りを止める。
/// `reqwest` の `stream` 機能を追加せずに済むよう `Response::chunk` を使う。
pub async fn read_async_response_body_capped(
    response: &mut reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, BodyReadError> {
    let declared = response.content_length();
    if declared_length_exceeds(declared, max_bytes) {
        return Err(BodyReadError::TooLarge);
    }
    // 宣言値（上限以内に丸める）で先に確保し、追記時の再確保で一時的に上限の約2倍を抱えないようにする。
    let mut buf = Vec::with_capacity(declared.map_or(0, |len| len.min(max_bytes as u64) as usize));
    while let Some(chunk) = response.chunk().await.map_err(|_| BodyReadError::Read)? {
        append_chunk_capped(&mut buf, &chunk, max_bytes)?;
    }
    Ok(buf)
}

/// 文字列を Unicode 文字数で `max_chars` 以内に収める（UTF-8 の文字境界で切るため安全）。
/// 超える場合は末尾を "..." にして合計 `max_chars` 文字にする（記事抜粋の切り詰めと同じ表記）。
pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_length_exceeds_only_when_declared_over_limit() {
        assert!(declared_length_exceeds(Some(9), 8));
        assert!(!declared_length_exceeds(Some(8), 8));
        assert!(!declared_length_exceeds(Some(0), 8));
        // 宣言なし(chunked 等)は実読み取り制限に委ねる。
        assert!(!declared_length_exceeds(None, 8));
    }

    #[test]
    fn append_chunk_capped_accepts_up_to_limit_and_rejects_one_byte_over() {
        let mut buf = Vec::new();
        append_chunk_capped(&mut buf, b"abcd", 8).unwrap();
        append_chunk_capped(&mut buf, b"efgh", 8).unwrap();
        assert_eq!(buf, b"abcdefgh");
        // 上限 + 1 バイトは拒否し、超過チャンクは蓄積しない。
        assert_eq!(
            append_chunk_capped(&mut buf, b"i", 8),
            Err(BodyReadError::TooLarge)
        );
        assert_eq!(buf, b"abcdefgh");
    }

    #[test]
    fn append_chunk_capped_judges_by_bytes_for_multibyte() {
        // "あああ" = 9バイト（3文字）は max=8 で拒否（文字数ではなくバイト数で判定）。
        let mut buf = Vec::new();
        assert_eq!(
            append_chunk_capped(&mut buf, "あああ".as_bytes(), 8),
            Err(BodyReadError::TooLarge)
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn truncate_chars_keeps_short_text_and_cuts_long_text_on_char_boundary() {
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("abcde", 5), "abcde");
        // マルチバイト文字でも文字境界で切り、合計 max_chars 文字になる。
        let cut = truncate_chars(&"あ".repeat(10), 5);
        assert_eq!(cut, "ああ...");
        assert_eq!(cut.chars().count(), 5);
        // サロゲートペア相当（4バイト文字）でも壊れない。
        let cut = truncate_chars(&"😀".repeat(10), 4);
        assert_eq!(cut, "😀...");
    }
}
