//! データ移行用ZIP書き出しのドメイン型（データ設計書 §15.4、判断台帳 D41）。
//!
//! - `MigrationManifest`: ZIP内の `manifest.json`。取り込み（別タスク）が中身と形式を確認するために使う。
//! - `MigrationExportResultDto`: `export_migration_data` が React へ返す結果。
//!   保存先のフルパスは返さず、Rust 側で決めたファイル名と件数だけを返す。

use serde::{Deserialize, Serialize};

/// `manifest.json` の形式バージョン。項目の意味を変えたら上げる。
pub const MIGRATION_MANIFEST_VERSION: u32 = 1;

/// ZIP内 `manifest.json` の内容（§15.4）。
///
/// ローカル書き出し（D41）は Google Drive へ置かないため暗号化しない（§15.5 の暗号化は Drive 経由の移行が対象）。
/// そのため `encrypted` は常に false とし、`encryption` / `expiresAt`（Drive 経由の受け渡し用）は持たない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationManifest {
    pub version: u32,
    pub app_version: String,
    pub transfer_id: String,
    pub created_at: String,
    /// ZIPに1件以上含めた区分（`config` / `news` / `favorites` / `dictionary` / `user` / `rewards` / `archive`）。
    pub included: Vec<String>,
    pub encrypted: bool,
    /// `manifest.json` 自身を除く、ZIP内の全ファイル。
    pub files: Vec<MigrationManifestFile>,
}

/// `manifest.json` に載せる1ファイル分の情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationManifestFile {
    /// ZIP内のエントリ名（`/` 区切りの相対パス）。
    pub path: String,
    pub size_bytes: u64,
}

/// `export_migration_data` の戻り値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationExportResultDto {
    /// 書き出したZIPのファイル名だけ（例: `yuuko_transfer_tr_20261008140000.zip`）。保存先パスは含めない。
    pub file_name: String,
    /// `manifest.json` を除いた格納ファイル数。
    pub file_count: usize,
    /// 格納したニュース記事Markdownの数。
    pub article_count: usize,
    /// 格納した月次アーカイブZIPの数。
    pub archive_count: usize,
    /// 格納ファイルの合計サイズ（圧縮前）。
    pub total_bytes: u64,
}
