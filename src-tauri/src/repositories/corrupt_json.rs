//! 保存 JSON が壊れていたときの共通処理（退避して既定値で作り直す）。
//!
//! セキュリティ詳細設計書 §11.4（破損ファイルを退避・初期設定で再作成・アプリ全体は止めない）と
//! 判断台帳 D57（設定ファイルと同じ退避方法: `<name>.corrupt.json` へ1世代だけ複製）に従う。
//! 「壊れている」は JSON として解釈できないこと（`AppError::Json`）だけを指す。
//! 権限などの IO エラーでは作り直さない（一時的な失敗でデータを消さないため）。
//!
//! 辞書（dictionary/entries.json）には使わない。利用者が書き溜めた内容を失わないよう、
//! 読めないときの扱いは別途決める（自動では初期化しない）。

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::error::AppError;

/// 破損ファイルの退避先（`foo.json` → 同じフォルダの `foo.corrupt.json`）。名前固定で1世代だけ残す。
pub(crate) fn corrupt_backup_path(path: &Path) -> PathBuf {
    path.with_extension("corrupt.json")
}

/// 破損したファイルを、上書きする前に別名でバイト列のまま複製する。
/// 元ファイルは動かさない（作り直しが失敗しても元のまま残すため）。
/// 一時ファイルへ複製してから差し替えるので、途中で失敗しても前回の退避ファイルは壊れない。
/// 失敗時は Err を返し、呼び出し側は作り直しを中止する。`kind` はログ用の種類名（パスは出さない）。
pub(crate) fn backup_corrupt_file(path: &Path, kind: &str) -> Result<(), AppError> {
    let backup_path = corrupt_backup_path(path);
    let temp_path = path.with_extension("corrupt.json.tmp");

    let result =
        std::fs::copy(path, &temp_path).and_then(|_| std::fs::rename(&temp_path, &backup_path));
    if let Err(error) = result {
        // 中身やフルパスはログへ出さない。
        log::error!("Failed to back up corrupt {kind} file: {}", error.kind());
        if temp_path.exists() {
            let _ = std::fs::remove_file(&temp_path);
        }
        return Err(error.into());
    }
    Ok(())
}

/// 既存の JSON ファイルを読む。JSON として読めなければ退避してから `reset` で既定値を保存し、その値を返す。
///
/// - バイト列で読み、先頭の UTF-8 BOM を除いてから解析する（手編集の BOM や不正な UTF-8 も
///   「JSON として読めない」側に含め、BOM だけで初期化されないようにする）。
/// - 読み込み自体の IO エラーはそのまま返す（作り直さない）。
/// - 退避に失敗したら上書きせずエラーを返す（元ファイルはそのまま）。
/// - `reset` は既定値を保存して返す処理。保存に失敗したらそのエラーを返す（次回の読み込みで再試行される）。
pub(crate) fn read_json_or_reset<T, F>(path: &Path, kind: &str, reset: F) -> Result<T, AppError>
where
    T: DeserializeOwned,
    F: FnOnce() -> Result<T, AppError>,
{
    let raw = std::fs::read(path)?;
    match serde_json::from_slice::<T>(strip_utf8_bom_bytes(&raw)) {
        Ok(value) => Ok(value),
        Err(error) => {
            // 種類と分類だけを残す（パス・中身・解析位置付きのメッセージは出さない）。
            log::warn!(
                "{kind} file is not valid JSON ({:?}); backing it up and resetting to defaults",
                error.classify()
            );
            backup_corrupt_file(path, kind)?;
            reset()
        }
    }
}

fn strip_utf8_bom_bytes(input: &[u8]) -> &[u8] {
    input.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_backup_path_uses_fixed_name_next_to_original() {
        let path = Path::new("state").join("yuuko_notification_state.json");
        assert_eq!(
            corrupt_backup_path(&path),
            Path::new("state").join("yuuko_notification_state.corrupt.json")
        );
    }

    #[test]
    fn strip_utf8_bom_bytes_removes_only_leading_bom() {
        assert_eq!(strip_utf8_bom_bytes(b"\xEF\xBB\xBF{}"), b"{}");
        assert_eq!(strip_utf8_bom_bytes(b"{}"), b"{}");
    }
}
