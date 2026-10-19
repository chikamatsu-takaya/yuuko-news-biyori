//! データ移行用ZIPの書き出し（データ設計書 §15、判断台帳 D41 / D24）。
//!
//! 方針:
//! - 含めるものは許可リストで決める（`FIXED_FILES` と、ニュース記事Markdown・月次アーカイブZIPの名前規則）。
//!   新しく増えた秘密情報ファイルや設定ファイルが、何もしなくても入らないようにするため。
//!   ニュース取得の git 外設定（`config/news_sources.json` / `config/network_allowlist.json`）、
//!   `logs/` / `cache/` / `state/`、破損退避（`*.corrupt.json`）、一時ファイル（`*.tmp` / `*.bak`）は
//!   許可リストに無いので入らない。APIキーは環境変数で受け取りファイルに保存していない。
//! - シンボリックリンク（Windows のジャンクションを含む）は辿らず、通常ファイルだけを入れる。
//! - 書き出し先はアプリデータ直下の固定フォルダ `exports/`。ファイル名は Rust 側で時刻から作り、
//!   React からパスやファイル名は受け取らない。
//! - 一時ファイル（`<最終名>.tmp`）へ書き、再オープンして検証できたものだけを最終名へ変える。
//!   途中で失敗したら一時ファイルを消すので、中途半端なZIPは残らない。
//! - 各ファイルはZIPへストリームで流し込み、全体をメモリへ読み込まない。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};

use chrono::{DateTime, FixedOffset, SecondsFormat};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::data_export::{
    MigrationExportResultDto, MigrationFolderKind, MigrationManifest, MigrationManifestFile,
    MIGRATION_MANIFEST_VERSION,
};
use crate::error::{AppError, OpenMigrationFolderError};
use crate::infra::external_browser::{self, BrowserOpenError};
use crate::paths::{
    AppPaths, ARTICLE_FAVORITES_RELATIVE_PATH, ARTICLE_NEWS_RELATIVE_DIR, DICTIONARY_RELATIVE_PATH,
    FRIENDSHIP_RELATIVE_PATH, GACHA_STATE_RELATIVE_PATH, MIGRATION_EXPORTS_RELATIVE_DIR,
    MIGRATION_IMPORTS_RELATIVE_DIR, REWARDS_RELATIVE_PATH, SETTINGS_RELATIVE_PATH,
};

pub(crate) const MANIFEST_ENTRY_NAME: &str = "manifest.json";
pub(crate) const EXPORT_FILE_PREFIX: &str = "yuuko_transfer_";
pub(crate) const ARCHIVE_RELATIVE_DIR: &str = "archive";
pub(crate) const ARCHIVE_INDEX_RELATIVE_PATH: &str = "archive/archive_index.json";
/// 同じ秒に書き出しが重なったときに付ける連番の上限。
const MAX_NAME_ATTEMPTS: u32 = 100;

/// 区分の並び（manifest の `included` の順序にも使う）。
pub(crate) const CATEGORY_ORDER: [&str; 8] = [
    "config",
    "news",
    "favorites",
    "dictionary",
    "user",
    "rewards",
    "gacha",
    "archive",
];

/// ガチャ状態の区分。取り込みでこの区分を含まないZIP（ガチャ保存の実装前に書き出したもの）は、
/// 移行先の現在のガチャ状態を置き換えずに残す（`data_import_service`・データ設計書 §15.7）。
pub(crate) const GACHA_CATEGORY: &str = "gacha";

/// 1ファイル単位で許可するもの（アプリデータ直下からの相対パス）。取り込み（`data_import_service`）も同じ一覧を使う。
/// config/ からは settings.json だけを入れる（news_sources.json / network_allowlist.json は §15 の対象外扱い）。
/// ガチャは `gacha/gacha_state.json` だけを入れる（`gacha_state.corrupt.json` / `.bak` / `.tmp` は入れない）。
pub(crate) const FIXED_FILES: [&str; 7] = [
    SETTINGS_RELATIVE_PATH,
    ARTICLE_FAVORITES_RELATIVE_PATH,
    DICTIONARY_RELATIVE_PATH,
    FRIENDSHIP_RELATIVE_PATH,
    REWARDS_RELATIVE_PATH,
    GACHA_STATE_RELATIVE_PATH,
    ARCHIVE_INDEX_RELATIVE_PATH,
];

/// 移行ZIPに入る1ファイルの種類（書き出し・取り込みで共通）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportFileKind {
    Article,
    ArchiveZip,
    Other,
}

/// ZIPへ入れる予定の1ファイル。`entry_name` は検証済みの名前だけから組み立てる。
#[derive(Debug, Clone)]
pub(crate) struct PlannedFile {
    pub(crate) entry_name: String,
    pub(crate) source: PathBuf,
    pub(crate) kind: ExportFileKind,
}

#[derive(Clone)]
pub struct DataExportService {
    app_data_dir: PathBuf,
    /// 書き出しを1件ずつにする（残った一時ファイルの掃除と連番の決定を、別の書き出しと競合させないため）。
    /// 取り込み（`DataImportService`）とも共有し、書き出しと取り込みを同時に走らせない。
    export_lock: Arc<Mutex<()>>,
}

impl DataExportService {
    pub fn new(paths: &AppPaths) -> Self {
        Self::with_app_data_dir(paths.app_data_dir.clone())
    }

    fn with_app_data_dir(app_data_dir: PathBuf) -> Self {
        Self {
            app_data_dir,
            export_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 移行用ZIPを `exports/` へ書き出し、ファイル名と件数を返す。
    pub fn export_migration_data(&self) -> Result<MigrationExportResultDto, AppError> {
        self.export_at(chrono::Local::now().fixed_offset())
    }

    fn export_at(&self, now: DateTime<FixedOffset>) -> Result<MigrationExportResultDto, AppError> {
        // 前回の書き出しが panic していてもロック自体は使い続けられる（守る値を持たないため）。
        let _guard = self
            .export_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let exports_dir = self.app_data_dir.join(MIGRATION_EXPORTS_RELATIVE_DIR);
        std::fs::create_dir_all(&exports_dir)?;
        if !is_real_dir(&exports_dir) {
            return Err(AppError::Validation(
                "export folder is not a regular directory".to_string(),
            ));
        }
        remove_stale_temp_files(&exports_dir);

        let plan = plan_migration_files(&self.app_data_dir)?;
        write_export(&exports_dir, &plan, now)
    }

    /// 書き出しと取り込みで共有するロック（取り込み側の `DataImportService` に渡す）。
    pub fn migration_lock(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.export_lock)
    }

    /// 書き出し・取り込みのどちらかが実行中か（再起動の前に確かめる。途中で止めると退避の印が残り得るため）。
    pub fn is_migration_running(&self) -> bool {
        matches!(self.export_lock.try_lock(), Err(TryLockError::WouldBlock))
    }

    /// 書き出し先 `exports/` / 取り込み元 `imports/` をエクスプローラーで開く（本番用）。
    pub fn open_migration_folder(
        &self,
        kind: MigrationFolderKind,
    ) -> Result<(), OpenMigrationFolderError> {
        self.open_migration_folder_with(kind, external_browser::open_verified_folder)
    }

    /// 種類に対応する固定フォルダを（無ければ作って）実体のフォルダと確かめ、`opener` で開く。
    ///
    /// パスは React から受け取らず、アプリデータ直下の固定名からだけ組み立てる。
    /// リンク（ジャンクションを含む）やファイルになっていたら開かない（別の場所を開かせないため）。
    /// 実際の起動（OS 連携）は `opener` として受け取り、テストでは起動せずに確かめる。ログにパスは出さない。
    pub fn open_migration_folder_with(
        &self,
        kind: MigrationFolderKind,
        opener: impl FnOnce(&Path) -> Result<(), BrowserOpenError>,
    ) -> Result<(), OpenMigrationFolderError> {
        let relative = match kind {
            MigrationFolderKind::Exports => MIGRATION_EXPORTS_RELATIVE_DIR,
            MigrationFolderKind::Imports => MIGRATION_IMPORTS_RELATIVE_DIR,
        };
        let dir = self.app_data_dir.join(relative);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            log::warn!(
                "Failed to prepare migration folder {kind:?}: {:?}",
                error.kind()
            );
            return Err(OpenMigrationFolderError::Unavailable);
        }
        if !is_real_dir(&dir) {
            log::warn!("Refused to open migration folder {kind:?}: not a regular directory");
            return Err(OpenMigrationFolderError::Unavailable);
        }
        opener(&dir).map_err(|error| {
            log::warn!("Failed to open migration folder {kind:?}: {error:?}");
            match error {
                BrowserOpenError::Unsupported => OpenMigrationFolderError::Unsupported,
                BrowserOpenError::LaunchFailed => OpenMigrationFolderError::LaunchFailed,
            }
        })
    }
}

/// 許可リストに一致する通常ファイルだけを、エントリ名の順に並べて返す。
/// 取り込み時は、置き換え前に退避する「現在のデータ」の一覧としても使う。
pub(crate) fn plan_migration_files(root: &Path) -> Result<Vec<PlannedFile>, AppError> {
    let mut plan = Vec::new();

    for relative in FIXED_FILES {
        let source = root.join(relative);
        if parents_are_real_dirs(root, relative) && is_regular_file(&source) {
            plan.push(PlannedFile {
                entry_name: relative.to_string(),
                source,
                kind: ExportFileKind::Other,
            });
        }
    }

    // 記事: news/<id>.md と news/<月フォルダ>/<id>.md（データ設計書 §4.2）。それより深い階層は入れない。
    let news_dir = root.join(ARTICLE_NEWS_RELATIVE_DIR);
    if is_real_dir(&news_dir) {
        for entry in std::fs::read_dir(&news_dir)? {
            let entry = entry?;
            // DirEntry::file_type はリンク先を辿らない。
            let file_type = entry.file_type()?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if file_type.is_file() && is_article_file_name(&name) {
                plan.push(PlannedFile {
                    entry_name: format!("{ARTICLE_NEWS_RELATIVE_DIR}/{name}"),
                    source: entry.path(),
                    kind: ExportFileKind::Article,
                });
            } else if file_type.is_dir() && is_safe_name_component(&name) {
                for child in std::fs::read_dir(entry.path())? {
                    let child = child?;
                    let Some(child_name) = child.file_name().to_str().map(str::to_string) else {
                        continue;
                    };
                    if child.file_type()?.is_file() && is_article_file_name(&child_name) {
                        plan.push(PlannedFile {
                            entry_name: format!("{ARTICLE_NEWS_RELATIVE_DIR}/{name}/{child_name}"),
                            source: child.path(),
                            kind: ExportFileKind::Article,
                        });
                    }
                }
            }
        }
    }

    // 月次アーカイブ: archive/YYYY-MM.zip だけ（退避用フォルダや *.zip.tmp / *.zip.bak は入れない）。
    let archive_dir = root.join(ARCHIVE_RELATIVE_DIR);
    if is_real_dir(&archive_dir) {
        for entry in std::fs::read_dir(&archive_dir)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if entry.file_type()?.is_file() && is_month_archive_file_name(&name) {
                plan.push(PlannedFile {
                    entry_name: format!("{ARCHIVE_RELATIVE_DIR}/{name}"),
                    source: entry.path(),
                    kind: ExportFileKind::ArchiveZip,
                });
            }
        }
    }

    plan.sort_by(|left, right| left.entry_name.cmp(&right.entry_name));
    Ok(plan)
}

/// 計画したファイルを一時ファイルへ書き、検証できたら最終名へ変える。失敗時は一時ファイルを消す。
fn write_export(
    exports_dir: &Path,
    plan: &[PlannedFile],
    now: DateTime<FixedOffset>,
) -> Result<MigrationExportResultDto, AppError> {
    let (transfer_id, file_name, temp_path) = create_unique_temp(exports_dir, now)?;
    let final_path = exports_dir.join(&file_name);

    let written = match write_and_verify(&temp_path, plan, &transfer_id, now) {
        Ok(written) => written,
        Err(error) => {
            let _ = std::fs::remove_file(&temp_path);
            // パスや中身は出さない（§16.3）。
            log::error!("Failed to export migration data: {error}");
            return Err(error);
        }
    };
    if let Err(error) = std::fs::rename(&temp_path, &final_path) {
        let _ = std::fs::remove_file(&temp_path);
        log::error!("Failed to finalize migration export: {}", error.kind());
        return Err(error.into());
    }

    let mut result = MigrationExportResultDto {
        file_name,
        file_count: written.len(),
        article_count: 0,
        archive_count: 0,
        total_bytes: 0,
    };
    for (kind, size) in &written {
        result.total_bytes += size;
        match kind {
            ExportFileKind::Article => result.article_count += 1,
            ExportFileKind::ArchiveZip => result.archive_count += 1,
            ExportFileKind::Other => {}
        }
    }
    log::info!(
        "Migration data exported: files={} articles={} archives={}",
        result.file_count,
        result.article_count,
        result.archive_count
    );
    Ok(result)
}

/// 時刻から移行IDとファイル名を決め、一時ファイルを新規作成する。
/// 同じ名前が既にあれば連番を付ける（既存のZIPは上書きしない）。
fn create_unique_temp(
    exports_dir: &Path,
    now: DateTime<FixedOffset>,
) -> Result<(String, String, PathBuf), AppError> {
    let base_id = format!("tr_{}", now.format("%Y%m%d%H%M%S"));
    for attempt in 1..=MAX_NAME_ATTEMPTS {
        let transfer_id = if attempt == 1 {
            base_id.clone()
        } else {
            format!("{base_id}_{attempt}")
        };
        let file_name = format!("{EXPORT_FILE_PREFIX}{transfer_id}.zip");
        let final_path = exports_dir.join(&file_name);
        let temp_path = exports_dir.join(format!("{file_name}.tmp"));
        if std::fs::symlink_metadata(&final_path).is_ok() {
            continue;
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(_) => return Ok((transfer_id, file_name, temp_path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(AppError::Validation(
        "could not choose an export file name".to_string(),
    ))
}

/// 一時ファイルへZIPを書き、再オープンして件数と manifest を確かめる。
/// 返り値は実際に入れたファイルの種類とサイズ（manifest を除く）。
fn write_and_verify(
    temp_path: &Path,
    plan: &[PlannedFile],
    transfer_id: &str,
    now: DateTime<FixedOffset>,
) -> Result<Vec<(ExportFileKind, u64)>, AppError> {
    let file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(temp_path)?;
    let mut writer = ZipWriter::new(file);
    let mut manifest_files = Vec::with_capacity(plan.len());
    let mut written = Vec::with_capacity(plan.len());
    let mut included: Vec<&str> = Vec::new();

    for planned in plan {
        // 計画後に消えたファイルがあれば書き出し全体を失敗にする。同時に動くアーカイブ保守が記事を
        // 計画に無い新しい月次ZIPへ移した場合、飛ばすとその記事がどこにも入らないため（再実行で解消する）。
        let mut source = File::open(&planned.source).inspect_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                log::warn!("A planned file disappeared during migration export; aborting");
            }
        })?;
        let metadata = source.metadata()?;
        if !metadata.is_file() {
            return Err(AppError::Archive(
                "export source is not a regular file".to_string(),
            ));
        }
        // 圧縮済みの月次ZIPは再圧縮しても縮まないので、CPU を使わない無圧縮で入れる。
        let method = if planned.kind == ExportFileKind::ArchiveZip {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .large_file(metadata.len() > u64::from(u32::MAX));
        writer
            .start_file(planned.entry_name.as_str(), options)
            .map_err(|error| AppError::Archive(format!("failed to start export entry: {error}")))?;
        let size = std::io::copy(&mut source, &mut writer)?;

        manifest_files.push(MigrationManifestFile {
            path: planned.entry_name.clone(),
            size_bytes: size,
        });
        written.push((planned.kind, size));
        let category = category_of(&planned.entry_name);
        if !included.contains(&category) {
            included.push(category);
        }
    }

    let manifest = MigrationManifest {
        version: MIGRATION_MANIFEST_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        transfer_id: transfer_id.to_string(),
        created_at: now.to_rfc3339_opts(SecondsFormat::Secs, false),
        included: CATEGORY_ORDER
            .iter()
            .filter(|category| included.contains(category))
            .map(|category| category.to_string())
            .collect(),
        encrypted: false,
        files: manifest_files,
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)?;
    writer
        .start_file(
            MANIFEST_ENTRY_NAME,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| AppError::Archive(format!("failed to start manifest entry: {error}")))?;
    writer.write_all(&manifest_json)?;
    let file = writer
        .finish()
        .map_err(|error| AppError::Archive(format!("failed to finalize export zip: {error}")))?;
    file.sync_all()?;
    drop(file);

    verify_export(temp_path, written.len() + 1)?;
    Ok(written)
}

/// 書き終えたZIPを開き直し、エントリ数と manifest を読めることを確かめる。
fn verify_export(path: &Path, expected_entries: usize) -> Result<(), AppError> {
    let mut archive = ZipArchive::new(File::open(path)?).map_err(|error| {
        AppError::Archive(format!(
            "failed to reopen export zip for verification: {error}"
        ))
    })?;
    if archive.len() != expected_entries {
        return Err(AppError::Archive(
            "export zip entry count mismatch".to_string(),
        ));
    }
    let manifest = archive
        .by_name(MANIFEST_ENTRY_NAME)
        .map_err(|error| AppError::Archive(format!("export manifest is missing: {error}")))?;
    serde_json::from_reader::<_, MigrationManifest>(manifest)?;
    Ok(())
}

/// 前回の書き出しが途中で止まって残った一時ファイルを消す（ロック中に呼ぶ）。
/// 失敗しても書き出し自体は続ける。
fn remove_stale_temp_files(exports_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(exports_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        let is_stale_temp = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(EXPORT_FILE_PREFIX) && name.ends_with(".zip.tmp"));
        if is_file && is_stale_temp {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                log::warn!(
                    "Failed to remove a stale migration export temp file: {}",
                    error.kind()
                );
            }
        }
    }
}

pub(crate) fn category_of(entry_name: &str) -> &'static str {
    let first = entry_name.split('/').next().unwrap_or_default();
    CATEGORY_ORDER
        .iter()
        .copied()
        .find(|category| *category == first)
        .unwrap_or("other")
}

/// リンクを辿らずに、実体のディレクトリかどうか。
pub(crate) fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

/// リンクを辿らずに、通常ファイルかどうか。
pub(crate) fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// 相対パスの途中のフォルダがすべて実体のディレクトリか（リンク経由で別の場所を読まないため）。
pub(crate) fn parents_are_real_dirs(root: &Path, relative: &str) -> bool {
    let mut current = root.to_path_buf();
    let mut components: Vec<&str> = relative.split('/').collect();
    components.pop();
    components.into_iter().all(|component| {
        current.push(component);
        is_real_dir(&current)
    })
}

/// 英数字・ハイフン・アンダースコアだけの名前（記事ID・月フォルダ名）。
pub(crate) fn is_safe_name_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn is_article_file_name(name: &str) -> bool {
    name.strip_suffix(".md").is_some_and(is_safe_name_component)
}

/// `YYYY-MM.zip`（月は01〜12）だけを許す。
pub(crate) fn is_month_archive_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".zip") else {
        return false;
    };
    let bytes = stem.as_bytes();
    if bytes.len() != 7 || bytes[4] != b'-' {
        return false;
    }
    let digits_ok = bytes[..4].iter().chain(&bytes[5..]).all(u8::is_ascii_digit);
    digits_ok && matches!(stem[5..].parse::<u32>(), Ok(1..=12))
}

/// ZIP内のエントリ名が許可リストのどれに当たるかを返す（当たらなければ `None`）。
///
/// 書き出し（`plan_migration_files`）が作る名前と同じ規則で判定し、取り込みはこれに通った名前しか受け付けない。
/// 名前の各部分は英数字・`-`・`_`（と固定のファイル名）だけなので、`..`・`\`・絶対パス・ドライブ文字は通らない。
pub(crate) fn classify_entry_name(entry_name: &str) -> Option<ExportFileKind> {
    if FIXED_FILES.contains(&entry_name) {
        return Some(ExportFileKind::Other);
    }
    let parts: Vec<&str> = entry_name.split('/').collect();
    match parts.as_slice() {
        [ARTICLE_NEWS_RELATIVE_DIR, name] if is_article_file_name(name) => {
            Some(ExportFileKind::Article)
        }
        [ARTICLE_NEWS_RELATIVE_DIR, folder, name]
            if is_safe_name_component(folder) && is_article_file_name(name) =>
        {
            Some(ExportFileKind::Article)
        }
        [ARCHIVE_RELATIVE_DIR, name] if is_month_archive_file_name(name) => {
            Some(ExportFileKind::ArchiveZip)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "yuuko-migration-export-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn fixed_now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-10-08T14:00:00+09:00").unwrap()
    }

    /// 移行対象と、入ってはいけないファイルを両方置いたアプリデータ。
    fn seed_app_data(root: &Path) {
        write(root, "config/settings.json", r#"{"aiProvider":"mock"}"#);
        write(root, "favorites/article_favorites.json", "[]");
        write(root, "dictionary/entries.json", "[]");
        write(root, "user/friendship.json", "{}");
        write(root, "rewards/rewards.json", "{}");
        write(root, "gacha/gacha_state.json", r#"{"starFragments":43}"#);
        write(
            root,
            "news/202610/news_0123456789abcdef.md",
            "---\n---\nbody",
        );
        write(root, "news/unknown/article-001.md", "---\n---\nbody");
        write(root, "news/article-002.md", "---\n---\nbody");
        write(root, "archive/archive_index.json", "{}");
        write(root, "archive/2026-05.zip", "zip-bytes");

        // 対象外
        write(root, "config/news_sources.json", "SOURCES");
        write(root, "config/network_allowlist.json", "ALLOWLIST");
        write(root, "config/settings.corrupt.json", "CORRUPT");
        write(root, "config/settings.json.tmp", "TMP");
        write(root, ".env", "GEMINI_API_KEY=secret");
        write(root, "config/.env", "GEMINI_API_KEY=secret");
        write(root, "secrets/api_key.txt", "secret");
        write(root, "logs/app.log", "log");
        write(root, "cache/page.html", "cache");
        write(root, "state/yuuko_notification_state.json", "{}");
        write(root, "user/friendship.corrupt.json", "CORRUPT");
        // 加算イベント履歴は補助的な記録なので移行しない（データ設計書 §10.3）。
        write(
            root,
            "user/friendship_events.json",
            r#"{"version":1,"events":[]}"#,
        );
        write(root, "gacha/gacha_state.corrupt.json", "CORRUPT");
        write(root, "gacha/gacha_state.json.bak", "BAK");
        write(root, "gacha/gacha_state.json.tmp", "TMP");
        write(root, "news/202610/notes.txt", "txt");
        write(root, "news/202610/deep/nested.md", "nested");
        write(root, "news/bad name/x.md", "bad");
        write(root, "archive/2026-05.zip.tmp", "tmp");
        write(root, "archive/2026-05.zip.bak", "bak");
        write(root, "archive/2026-13.zip", "bad month");
        write(
            root,
            "archive/.markdown-retirement.rollback/202605/a.md",
            "a",
        );
        write(root, "exports/yuuko_transfer_tr_old.zip", "previous export");
    }

    fn zip_entry_names(path: &Path) -> Vec<String> {
        let archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
        names.sort();
        names
    }

    fn read_manifest(path: &Path) -> MigrationManifest {
        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut entry = archive.by_name(MANIFEST_ENTRY_NAME).unwrap();
        let mut raw = String::new();
        entry.read_to_string(&mut raw).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn exports_listing(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(root.join("exports"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn export_includes_only_allowlisted_files_with_manifest() {
        let root = temp_dir("allowlist");
        seed_app_data(&root);
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.export_at(fixed_now()).unwrap();

        assert_eq!(result.file_name, "yuuko_transfer_tr_20261008140000.zip");
        assert_eq!(result.file_count, 11);
        assert_eq!(result.article_count, 3);
        assert_eq!(result.archive_count, 1);
        let zip_path = root.join("exports").join(&result.file_name);
        assert_eq!(
            zip_entry_names(&zip_path),
            vec![
                "archive/2026-05.zip",
                "archive/archive_index.json",
                "config/settings.json",
                "dictionary/entries.json",
                "favorites/article_favorites.json",
                "gacha/gacha_state.json",
                "manifest.json",
                "news/202610/news_0123456789abcdef.md",
                "news/article-002.md",
                "news/unknown/article-001.md",
                "rewards/rewards.json",
                "user/friendship.json",
            ]
        );

        let manifest = read_manifest(&zip_path);
        assert_eq!(manifest.version, MIGRATION_MANIFEST_VERSION);
        assert_eq!(manifest.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(manifest.transfer_id, "tr_20261008140000");
        assert_eq!(manifest.created_at, "2026-10-08T14:00:00+09:00");
        assert!(!manifest.encrypted);
        assert_eq!(
            manifest.included,
            vec![
                "config",
                "news",
                "favorites",
                "dictionary",
                "user",
                "rewards",
                "gacha",
                "archive"
            ]
        );
        assert_eq!(manifest.files.len(), result.file_count);
        let settings = manifest
            .files
            .iter()
            .find(|file| file.path == "config/settings.json")
            .unwrap();
        assert_eq!(settings.size_bytes, r#"{"aiProvider":"mock"}"#.len() as u64);
        assert_eq!(
            result.total_bytes,
            manifest
                .files
                .iter()
                .map(|file| file.size_bytes)
                .sum::<u64>()
        );

        // 前回の書き出しは残り、一時ファイルは残らない。
        assert_eq!(
            exports_listing(&root),
            vec![
                "yuuko_transfer_tr_20261008140000.zip",
                "yuuko_transfer_tr_old.zip"
            ]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_keeps_secret_contents_out_of_the_zip() {
        let root = temp_dir("secrets");
        seed_app_data(&root);
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.export_at(fixed_now()).unwrap();

        let mut archive =
            ZipArchive::new(File::open(root.join("exports").join(result.file_name)).unwrap())
                .unwrap();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut raw = Vec::new();
            entry.read_to_end(&mut raw).unwrap();
            let text = String::from_utf8_lossy(&raw);
            for forbidden in [
                "secret",
                "SOURCES",
                "ALLOWLIST",
                "CORRUPT",
                "BAK",
                "TMP",
                "previous export",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "{} contains {forbidden}",
                    entry.name()
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_with_no_data_writes_manifest_only() {
        let root = temp_dir("empty");
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.export_at(fixed_now()).unwrap();

        assert_eq!(result.file_count, 0);
        let zip_path = root.join("exports").join(&result.file_name);
        assert_eq!(zip_entry_names(&zip_path), vec!["manifest.json"]);
        let manifest = read_manifest(&zip_path);
        assert!(manifest.included.is_empty());
        assert!(manifest.files.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_in_the_same_second_does_not_overwrite_previous_zip() {
        let root = temp_dir("collision");
        write(&root, "config/settings.json", "{}");
        let service = DataExportService::with_app_data_dir(root.clone());

        let first = service.export_at(fixed_now()).unwrap();
        let second = service.export_at(fixed_now()).unwrap();

        assert_eq!(first.file_name, "yuuko_transfer_tr_20261008140000.zip");
        assert_eq!(second.file_name, "yuuko_transfer_tr_20261008140000_2.zip");
        let manifest = read_manifest(&root.join("exports").join(&second.file_name));
        assert_eq!(manifest.transfer_id, "tr_20261008140000_2");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_removes_stale_temp_files_from_an_interrupted_run() {
        let root = temp_dir("stale");
        write(
            &root,
            "exports/yuuko_transfer_tr_20260101000000.zip.tmp",
            "half",
        );
        write(&root, "exports/unrelated.tmp", "keep");
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.export_at(fixed_now()).unwrap();

        assert_eq!(
            exports_listing(&root),
            vec!["unrelated.tmp", result.file_name.as_str()]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_write_leaves_no_partial_zip() {
        let root = temp_dir("failure");
        let exports_dir = root.join("exports");
        std::fs::create_dir_all(&exports_dir).unwrap();
        write(&root, "config/settings.json", "{}");
        // 計画後にファイルがディレクトリへ差し替わった状況を作り、書き込み途中で失敗させる。
        std::fs::create_dir_all(root.join("dictionary/entries.json")).unwrap();
        let plan = vec![
            PlannedFile {
                entry_name: "config/settings.json".to_string(),
                source: root.join("config/settings.json"),
                kind: ExportFileKind::Other,
            },
            PlannedFile {
                entry_name: "dictionary/entries.json".to_string(),
                source: root.join("dictionary/entries.json"),
                kind: ExportFileKind::Other,
            },
        ];
        let result = write_export(&exports_dir, &plan, fixed_now());

        assert!(result.is_err());
        // 一時ファイルも最終ZIPも残らない。
        assert!(exports_listing(&root).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_fails_when_the_export_folder_cannot_be_created() {
        let root = temp_dir("failure-service");
        write(&root, "config/settings.json", "{}");
        let service = DataExportService::with_app_data_dir(root.clone());
        // exports/ をファイルにしておくと書き出し先を作れない。
        write(&root, "exports", "not a dir");

        assert!(service.export_at(fixed_now()).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("exports")).unwrap(),
            "not a dir"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_aborts_without_leaving_files_when_a_planned_file_disappears() {
        let root = temp_dir("disappeared");
        let exports_dir = root.join("exports");
        std::fs::create_dir_all(&exports_dir).unwrap();
        write(&root, "config/settings.json", "{}");
        // 計画後にアーカイブ保守が記事を移した状況（計画にあるファイルがもう無い）。
        let plan = vec![
            PlannedFile {
                entry_name: "config/settings.json".to_string(),
                source: root.join("config/settings.json"),
                kind: ExportFileKind::Other,
            },
            PlannedFile {
                entry_name: "news/202605/moved.md".to_string(),
                source: root.join("news/202605/moved.md"),
                kind: ExportFileKind::Article,
            },
        ];

        let result = write_export(&exports_dir, &plan, fixed_now());

        assert!(
            matches!(result, Err(AppError::Io(ref error)) if error.kind() == std::io::ErrorKind::NotFound)
        );
        assert!(exports_listing(&root).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn export_does_not_follow_junctions() {
        let root = temp_dir("junction");
        let outside = temp_dir("junction-outside");
        write(&outside, "inner.md", "secret");
        std::fs::create_dir_all(root.join("news")).unwrap();
        let link = root.join("news").join("202610");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "mklink /J failed");
        assert!(link.join("inner.md").exists());
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.export_at(fixed_now()).unwrap();

        assert_eq!(result.file_count, 0);
        assert_eq!(result.article_count, 0);
        // ジャンクションだけを外してから消す（リンク先を消さないため）。
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn export_does_not_follow_symlinks() {
        let root = temp_dir("symlink");
        let outside = temp_dir("symlink-outside");
        write(&outside, "secret.md", "secret");
        write(&outside, "dir/inner.md", "secret");
        std::fs::create_dir_all(root.join("news")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.md"), root.join("news/linked.md")).unwrap();
        std::os::unix::fs::symlink(outside.join("dir"), root.join("news/202610")).unwrap();
        std::os::unix::fs::symlink(outside.join("dir"), root.join("config")).unwrap();
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.export_at(fixed_now()).unwrap();

        assert_eq!(result.file_count, 0);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn name_rules_accept_only_expected_shapes() {
        assert!(is_article_file_name("news_0123456789abcdef.md"));
        assert!(is_article_file_name("article-001.md"));
        assert!(!is_article_file_name("../x.md"));
        assert!(!is_article_file_name(".md"));
        assert!(!is_article_file_name("a.md.tmp"));
        assert!(!is_article_file_name("a b.md"));
        assert!(is_month_archive_file_name("2026-05.zip"));
        assert!(is_month_archive_file_name("2026-12.zip"));
        assert!(!is_month_archive_file_name("2026-00.zip"));
        assert!(!is_month_archive_file_name("2026-13.zip"));
        assert!(!is_month_archive_file_name("2026-5.zip"));
        assert!(!is_month_archive_file_name("2026-05.zip.tmp"));
        assert!(!is_month_archive_file_name("archive_index.json"));
    }

    #[test]
    fn every_exported_entry_passes_the_shared_entry_rule() {
        let root = temp_dir("classify");
        seed_app_data(&root);

        let plan = plan_migration_files(&root).unwrap();

        assert_eq!(plan.len(), 11);
        for planned in &plan {
            assert_eq!(
                classify_entry_name(&planned.entry_name),
                Some(planned.kind),
                "{}",
                planned.entry_name
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_entry_rule_rejects_unsafe_or_unlisted_names() {
        for name in [
            "manifest.json",
            "../config/settings.json",
            "/config/settings.json",
            "config\\settings.json",
            "C:/config/settings.json",
            "config/news_sources.json",
            "config/network_allowlist.json",
            "config/settings.json.bak",
            "gacha/gacha_state.corrupt.json",
            "gacha/gacha_state.json.bak",
            "gacha/gacha_state.json.tmp",
            "gacha/other.json",
            "user/friendship_events.json",
            "news/../x.md",
            "news/a/b/c.md",
            "news//x.md",
            "news/x.txt",
            "archive/2026-13.zip",
            "archive/2026-05.zip.tmp",
            "archive/.markdown-retirement.rollback/202605/a.md",
            "state/yuuko_notification_state.json",
            "news/",
            "",
        ] {
            assert_eq!(classify_entry_name(name), None, "{name}");
        }
    }

    #[test]
    fn open_migration_folder_creates_and_opens_only_the_fixed_folder() {
        let root = temp_dir("open-folder");
        let service = DataExportService::with_app_data_dir(root.clone());

        for (kind, name) in [
            (MigrationFolderKind::Exports, "exports"),
            (MigrationFolderKind::Imports, "imports"),
        ] {
            let mut opened = None;
            service
                .open_migration_folder_with(kind, |dir| {
                    opened = Some(dir.to_path_buf());
                    Ok(())
                })
                .unwrap();
            assert_eq!(opened, Some(root.join(name)));
            assert!(is_real_dir(&root.join(name)));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_migration_folder_maps_open_failures_to_fixed_errors() {
        let root = temp_dir("open-folder-fail");
        let service = DataExportService::with_app_data_dir(root.clone());

        assert!(matches!(
            service.open_migration_folder_with(MigrationFolderKind::Exports, |_| Err(
                BrowserOpenError::LaunchFailed
            )),
            Err(OpenMigrationFolderError::LaunchFailed)
        ));
        assert!(matches!(
            service.open_migration_folder_with(MigrationFolderKind::Imports, |_| Err(
                BrowserOpenError::Unsupported
            )),
            Err(OpenMigrationFolderError::Unsupported)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_migration_folder_refuses_a_file_in_place_of_the_folder() {
        let root = temp_dir("open-folder-file");
        write(&root, "imports", "not a folder");
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.open_migration_folder_with(MigrationFolderKind::Imports, |_| {
            panic!("must not open a non-directory")
        });

        assert!(matches!(result, Err(OpenMigrationFolderError::Unavailable)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn open_migration_folder_refuses_a_junction() {
        let root = temp_dir("open-folder-junction");
        let outside = temp_dir("open-folder-junction-outside");
        let link = root.join("exports");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "mklink /J failed");
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.open_migration_folder_with(MigrationFolderKind::Exports, |_| {
            panic!("must not open a junction")
        });

        assert!(matches!(result, Err(OpenMigrationFolderError::Unavailable)));
        // ジャンクションだけを外してから消す（リンク先を消さないため）。
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn open_migration_folder_refuses_a_symlink() {
        let root = temp_dir("open-folder-symlink");
        let outside = temp_dir("open-folder-symlink-outside");
        std::os::unix::fs::symlink(&outside, root.join("imports")).unwrap();
        let service = DataExportService::with_app_data_dir(root.clone());

        let result = service.open_migration_folder_with(MigrationFolderKind::Imports, |_| {
            panic!("must not open a symlink")
        });

        assert!(matches!(result, Err(OpenMigrationFolderError::Unavailable)));
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn is_migration_running_reflects_the_shared_lock() {
        let root = temp_dir("running");
        let service = DataExportService::with_app_data_dir(root.clone());
        assert!(!service.is_migration_running());
        let lock = service.migration_lock();
        let guard = lock.lock().unwrap();
        assert!(service.is_migration_running());
        drop(guard);
        assert!(!service.is_migration_running());
        std::fs::remove_dir_all(root).unwrap();
    }
}
