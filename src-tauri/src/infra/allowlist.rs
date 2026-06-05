//! 外部通信の許可リスト（セキュリティ詳細設計書 §6.2 準拠）。
//!
//! RSS取得先・記事HTML取得先・AI接続先をドメイン単位で管理し、危険スキームを
//! 明示的に拒否する。初期値は deny-by-default とし、許可ドメインを明示的に
//! 追加するまで外部取得を許可しない。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// 許可リスト本体。`config/network_allowlist.json` として保存・編集する。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAllowlist {
    pub version: u32,
    /// RSSフィード取得を許可するドメイン。
    pub allowed_rss_domains: Vec<String>,
    /// 記事HTML取得を許可するドメイン。
    pub allowed_article_domains: Vec<String>,
    /// AI APIの接続先として許可するエンドポイントドメイン。
    pub allowed_ai_endpoints: Vec<String>,
    /// 明示的に拒否するスキーム。http/https 以外は既定で拒否されるが、
    /// 監査性のため危険スキームを列挙して保持する。
    pub blocked_schemes: Vec<String>,
}

impl Default for NetworkAllowlist {
    /// deny-by-default。RSS/記事ドメインは空（=取得不可）とし、
    /// AIエンドポイントのみ将来のGemini接続先を既定許可にする。
    fn default() -> Self {
        Self {
            version: 1,
            allowed_rss_domains: Vec::new(),
            allowed_article_domains: Vec::new(),
            allowed_ai_endpoints: vec!["generativelanguage.googleapis.com".to_string()],
            blocked_schemes: vec![
                "file".to_string(),
                "ftp".to_string(),
                "data".to_string(),
                "javascript".to_string(),
            ],
        }
    }
}

impl NetworkAllowlist {
    /// 許可リストを読み込む。
    ///
    /// - ファイルが存在しない: 初回起動扱い。安全なデフォルト（deny-by-default）を返す。
    /// - ファイルが存在し破損している: **フェイルクローズ**。デフォルトへ戻さず `Err` を
    ///   返し、呼び出し側（実通信スライス）は取得を中止する。壊れた設定で意図せず
    ///   通信を許可しないための方針。
    pub fn load(path: &Path) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str::<Self>(crate::util::strip_utf8_bom(&raw)).map_err(|error| {
            AppError::Validation(format!(
                "network allowlist is corrupted; refusing external access (fail-close): {error}"
            ))
        })
    }

    /// 設定ファイルが無い場合のみデフォルトを書き出す（settings と同じ初期化方針）。
    pub fn initialize_default_if_missing(path: &Path) -> Result<(), AppError> {
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_vec_pretty(&Self::default())?;
        std::fs::write(path, payload)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// テスト用の一意な一時ファイルパス（プロセスID＋連番で衝突を避ける）。
    fn unique_temp_path() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "yuuko_allowlist_test_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn missing_file_returns_safe_default() {
        let path = unique_temp_path();
        let _ = std::fs::remove_file(&path);
        let allow = NetworkAllowlist::load(&path).expect("missing file should yield default");
        assert_eq!(allow, NetworkAllowlist::default());
        // deny-by-default: RSS/記事ドメインは空
        assert!(allow.allowed_rss_domains.is_empty());
        assert!(allow.allowed_article_domains.is_empty());
    }

    #[test]
    fn corrupted_file_fails_close() {
        // セキュリティ要件: 破損時はデフォルトに戻さず Err（フェイルクローズ）
        let path = unique_temp_path();
        std::fs::write(&path, b"{ this is not valid json").unwrap();
        let result = NetworkAllowlist::load(&path);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "corrupted allowlist must fail-close");
    }

    #[test]
    fn initialize_writes_default_then_loads_back() {
        let path = unique_temp_path();
        let _ = std::fs::remove_file(&path);
        NetworkAllowlist::initialize_default_if_missing(&path).unwrap();
        let loaded = NetworkAllowlist::load(&path).unwrap();
        assert_eq!(loaded, NetworkAllowlist::default());
        // 既存ファイルは上書きしない
        NetworkAllowlist::initialize_default_if_missing(&path).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_tolerates_utf8_bom() {
        // BOM付きの正当JSONは読めること（BOM以外の破損は fail-close を維持）。
        let path = unique_temp_path();
        let json = serde_json::to_string_pretty(&NetworkAllowlist::default()).unwrap();
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();
        let loaded = NetworkAllowlist::load(&path).expect("BOM-prefixed allowlist should load");
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, NetworkAllowlist::default());
    }
}
