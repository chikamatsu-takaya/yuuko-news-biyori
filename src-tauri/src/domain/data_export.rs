//! データ移行用ZIP書き出しのドメイン型（データ設計書 §15.4、判断台帳 D41）。
//!
//! - `MigrationManifest`: ZIP内の `manifest.json`。取り込みが中身と形式を確認するために使う。
//! - `MigrationExportResultDto`: `export_migration_data` が React へ返す結果。
//!   保存先のフルパスは返さず、Rust 側で決めたファイル名と件数だけを返す。
//! - `MigrationImportCandidateDto` / `MigrationImportResultDto`: 取り込み（§15.7）の候補一覧と結果。
//!   こちらもフルパスは返さず、`imports/` 内のファイル名だけを扱う。

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

/// `list_migration_imports` の1件（`imports/` に置かれた取り込み候補）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationImportCandidateDto {
    /// `imports/` 内のファイル名だけ。`import_migration_data` にはこの値をそのまま渡す。
    pub file_name: String,
    pub size_bytes: u64,
    /// manifest の `createdAt`（RFC 3339 として読めたときだけ、Rust 側で整形し直した値）。
    /// ZIPの中身の検証は取り込み時に行うため、ここで値があっても取り込めるとは限らない。
    pub created_at: Option<String>,
}

/// `import_migration_data` の戻り値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationImportResultDto {
    /// 取り込んだZIPのファイル名だけ。
    pub file_name: String,
    /// `manifest.json` を除いた取り込みファイル数。
    pub file_count: usize,
    pub article_count: usize,
    pub archive_count: usize,
    /// 取り込んだファイルの合計サイズ（展開後）。
    pub total_bytes: u64,
    /// 取り込み後はアプリの再起動を勧めるか。現状は常に true
    /// （画面が持つ表示中の値や自動要約キューの記事ID、起動時の報酬同期が取り込み前のデータに基づくため）。
    pub restart_required: bool,
}
