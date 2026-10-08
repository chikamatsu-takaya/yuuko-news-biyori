//! データ移行用ZIPの取り込み（データ設計書 §15.7、判断台帳 D41）。
//!
//! 方針:
//! - 取り込めるのは、アプリデータ直下の固定フォルダ `imports/` に置かれた `yuuko_transfer_<名前>.zip` だけ。
//!   React からはファイル名だけを受け取り、`imports/` の一覧に同じ名前の通常ファイルがあるときだけ開く
//!   （パスは受け取らない・シンボリックリンクやジャンクションは辿らない）。
//! - 受け付けるファイルの集合は書き出し（`data_export_service`）と同じ許可リスト（`classify_entry_name`）で決める。
//!   manifest の `files` とZIPの実際のエントリが過不足なく一致しないものは拒否する。
//! - ZIP爆弾対策として、エントリ数・1ファイルの大きさ・合計の大きさに上限を設け、宣言値ではなく
//!   実際に読んだバイト数で確かめる。中央ディレクトリを読み込む前に、末尾のレコードで件数と大きさも確かめる。
//! - 検証は一時フォルダ（`.migration-import-staging/`）へ展開しながら行い、JSON は読み込み時と同じ型として
//!   読めることを確かめる。ここまでは現在のデータに触れない。
//! - 差し替えは「現在のデータを `migration-backups/<日時>/` へ移す → 展開したものを所定の場所へ移す」の順で行い、
//!   途中で失敗したら置いたものを消して退避分を戻す。成功後は退避フォルダを1世代だけ残す。
//! - ガチャ状態（`gacha/gacha_state.json`）を含まないZIP（ガチャ保存の実装前に書き出したもの）では、
//!   移行先の現在のガチャ状態を退避・置き換えせずに残す（仮の設計判断。獲得済みのものを失わないため）。
//! - 差し替え中は、他の書き込み（記事・お気に入り・アーカイブ・辞書・友情ランク・報酬・ガチャ）と同じロックを取る。
//!   書き出しとも同じロックを共有し、書き出しと取り込みを同時に走らせない。

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, FixedOffset, SecondsFormat};
use zip::{CompressionMethod, ZipArchive};

use crate::domain::data_export::{
    MigrationImportCandidateDto, MigrationImportResultDto, MigrationManifest,
    MIGRATION_MANIFEST_VERSION,
};
use crate::domain::dictionary::PersistedDictionaryStore;
use crate::domain::friendship::FriendshipState;
use crate::domain::gacha::GachaState;
use crate::domain::reward::RewardsState;
use crate::domain::settings::PersistedSettings;
use crate::error::AppError;
use crate::paths::{
    AppPaths, ARTICLE_FAVORITES_RELATIVE_PATH, DICTIONARY_RELATIVE_PATH, FRIENDSHIP_RELATIVE_PATH,
    GACHA_STATE_RELATIVE_PATH, MIGRATION_BACKUPS_RELATIVE_DIR, MIGRATION_IMPORTS_RELATIVE_DIR,
    MIGRATION_IMPORT_STAGING_RELATIVE_DIR, REWARDS_RELATIVE_PATH, SETTINGS_RELATIVE_PATH,
};
use crate::repositories::article_repository::{
    validate_imported_archive_index_json, validate_imported_favorites_json,
    RETIREMENT_COMMITTED_DIR, RETIREMENT_ROLLBACK_DIR,
};
use crate::services::data_export_service::{
    category_of, classify_entry_name, is_month_archive_file_name, is_real_dir,
    is_safe_name_component, parents_are_real_dirs, plan_migration_files, ExportFileKind,
    ARCHIVE_INDEX_RELATIVE_PATH, ARCHIVE_RELATIVE_DIR, CATEGORY_ORDER, EXPORT_FILE_PREFIX,
    FIXED_FILES, GACHA_CATEGORY, MANIFEST_ENTRY_NAME,
};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// 差し替えが終わっていない退避フォルダに置く印。これがある間は次の取り込みを始めない
/// （途中で止まった取り込みのデータが退避フォルダにしか無い可能性があるため）。
const INCOMPLETE_MARKER: &str = ".incomplete";
/// 取り込み候補として返す件数の上限（新しい名前順）。
const MAX_LISTED_CANDIDATES: usize = 100;
/// 同じ秒に退避フォルダ名が重なったときに付ける連番の上限。
const MAX_BACKUP_NAME_ATTEMPTS: u32 = 100;
/// 展開時に一度に読み書きする大きさ。
const COPY_CHUNK_BYTES: usize = 64 * 1024;

/// 取り込みZIPの大きさの上限（データ設計書 §15.7）。
///
/// 1日あたり数十〜百件の記事を数年分ためても届かない程度に余裕を持たせつつ、
/// 展開で一時フォルダがディスクを使い切ったり、中央ディレクトリの読み込みでメモリを使い切ったりしない値にする。
#[derive(Debug, Clone, Copy)]
struct ImportLimits {
    /// 取り込みZIPファイル自体の大きさ。
    max_zip_bytes: u64,
    /// エントリ数（`manifest.json` を含む）。
    max_entries: u64,
    /// 中央ディレクトリの大きさ（エントリ数の上限分の名前が入る大きさ）。
    max_central_directory_bytes: u64,
    /// 展開後の合計。
    max_total_bytes: u64,
    max_manifest_bytes: u64,
    /// 設定・お気に入り・辞書などのJSON 1ファイル。
    max_json_bytes: u64,
    /// 記事Markdown 1ファイル。
    max_article_bytes: u64,
    /// 月次アーカイブZIP 1ファイル（展開せずそのまま置く）。
    max_archive_zip_bytes: u64,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_zip_bytes: 4 * GIB,
            max_entries: 50_000,
            max_central_directory_bytes: 64 * MIB,
            max_total_bytes: 4 * GIB,
            max_manifest_bytes: 16 * MIB,
            max_json_bytes: 64 * MIB,
            max_article_bytes: 4 * MIB,
            max_archive_zip_bytes: GIB,
        }
    }
}

impl ImportLimits {
    fn per_entry(&self, kind: ExportFileKind) -> u64 {
        match kind {
            ExportFileKind::Article => self.max_article_bytes,
            ExportFileKind::ArchiveZip => self.max_archive_zip_bytes,
            ExportFileKind::Other => self.max_json_bytes,
        }
    }
}

/// 差し替え中に取る、他の書き込みと共有のロック。
///
/// 取る順番は 記事 → 辞書 → 友情ランク → 報酬 → ガチャ で固定する。記事・辞書のロックは他のロックと入れ子にならず、
/// 友情ランク → 報酬 の順は `RewardService` / `FriendshipService` と同じなので、デッドロックしない。
/// ガチャのロックは最後に取る。`GachaService` はロック中に他のサービスを呼ばない（他のロックを取らない）ので、
/// ガチャのロックを持ったまま別のロックを待つ処理は無く、最後に取ればデッドロックしない。
/// 設定（settings.json）には書き込みロックが無いため、差し替え中の設定保存とは競合し得る（§15.7）。
#[derive(Clone, Default)]
pub struct MigrationWriteLocks {
    article: Arc<Mutex<()>>,
    dictionary: Arc<Mutex<()>>,
    friendship: Arc<Mutex<()>>,
    rewards: Arc<Mutex<()>>,
    gacha: Arc<Mutex<()>>,
}

impl MigrationWriteLocks {
    pub fn new(
        article: Arc<Mutex<()>>,
        dictionary: Arc<Mutex<()>>,
        friendship: Arc<Mutex<()>>,
        rewards: Arc<Mutex<()>>,
        gacha: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            article,
            dictionary,
            friendship,
            rewards,
            gacha,
        }
    }

    fn acquire(&self) -> [MutexGuard<'_, ()>; 5] {
        [
            lock_ignoring_poison(&self.article),
            lock_ignoring_poison(&self.dictionary),
            lock_ignoring_poison(&self.friendship),
            lock_ignoring_poison(&self.rewards),
            lock_ignoring_poison(&self.gacha),
        ]
    }
}

/// 守る値を持たないロックなので、別の処理が panic していても使い続ける。
fn lock_ignoring_poison(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 検証して一時フォルダへ展開した1ファイル。
#[derive(Debug, Clone)]
struct ExpectedFile {
    entry_name: String,
    kind: ExportFileKind,
    size_bytes: u64,
}

/// 差し替えの途中経過（失敗時に戻すため）。
#[derive(Debug, Default)]
struct SwapJournal {
    /// アプリデータ直下から退避フォルダへ移したもの。
    moved_out: Vec<String>,
    /// 一時フォルダからアプリデータ直下へ置いたもの。
    placed: Vec<String>,
}

#[derive(Clone)]
pub struct DataImportService {
    app_data_dir: PathBuf,
    /// 書き出しと共有するロック（`DataExportService::migration_lock`）。
    migration_lock: Arc<Mutex<()>>,
    write_locks: MigrationWriteLocks,
    limits: ImportLimits,
}

impl DataImportService {
    pub fn new(
        paths: &AppPaths,
        migration_lock: Arc<Mutex<()>>,
        write_locks: MigrationWriteLocks,
    ) -> Self {
        Self {
            app_data_dir: paths.app_data_dir.clone(),
            migration_lock,
            write_locks,
            limits: ImportLimits::default(),
        }
    }

    /// `imports/` に置かれた取り込み候補を、新しい名前順に返す（ファイル名・大きさ・作成日時だけ）。
    pub fn list_migration_imports(&self) -> Result<Vec<MigrationImportCandidateDto>, AppError> {
        let imports_dir = self.imports_dir()?;
        let mut candidates = list_candidate_files(&imports_dir)?;
        candidates.sort_by(|left, right| right.0.cmp(&left.0));
        candidates.truncate(MAX_LISTED_CANDIDATES);
        Ok(candidates
            .into_iter()
            .map(|(file_name, size_bytes)| {
                let created_at =
                    read_candidate_created_at(&imports_dir.join(&file_name), &self.limits);
                MigrationImportCandidateDto {
                    file_name,
                    size_bytes,
                    created_at,
                }
            })
            .collect())
    }

    /// `imports/` の `file_name` を検証して取り込み、現在のデータを置き換える。
    pub fn import_migration_data(
        &self,
        file_name: &str,
    ) -> Result<MigrationImportResultDto, AppError> {
        self.import_at(file_name, chrono::Local::now().fixed_offset())
    }

    fn import_at(
        &self,
        file_name: &str,
        now: DateTime<FixedOffset>,
    ) -> Result<MigrationImportResultDto, AppError> {
        let _migration_guard = lock_ignoring_poison(&self.migration_lock);
        let root = &self.app_data_dir;

        if !is_import_file_name(file_name) {
            return Err(AppError::Validation(
                "import file name is not allowed".to_string(),
            ));
        }
        let imports_dir = self.imports_dir()?;
        // 一覧に同じ名前の通常ファイルがあるときだけ開く（名前の大文字小文字違いやリンクを通さない）。
        let candidate_exists = list_candidate_files(&imports_dir)?
            .iter()
            .any(|(name, _)| name == file_name);
        if !candidate_exists {
            return Err(AppError::NotFound("import file".to_string()));
        }
        let zip_path = imports_dir.join(file_name);

        let backups_root = root.join(MIGRATION_BACKUPS_RELATIVE_DIR);
        ensure_no_incomplete_backup(&backups_root)?;

        let staging = root.join(MIGRATION_IMPORT_STAGING_RELATIVE_DIR);
        reset_staging_dir(&staging)?;
        let files = match extract_and_validate(&zip_path, &staging, &self.limits) {
            Ok(files) => files,
            Err(error) => {
                remove_staging_dir(&staging);
                // ZIP由来の名前や中身は出さない（理由は固定文言だけ）。
                log::warn!("Migration import was rejected: {error}");
                return Err(error);
            }
        };

        let swap_result = {
            let _write_guards = self.write_locks.acquire();
            swap_in(root, &staging, &backups_root, &files, now)
        };
        remove_staging_dir(&staging);
        let backup_dir = swap_result?;
        prune_old_backups(&backups_root, &backup_dir);

        let mut result = MigrationImportResultDto {
            file_name: file_name.to_string(),
            file_count: files.len(),
            article_count: 0,
            archive_count: 0,
            total_bytes: 0,
            restart_required: true,
        };
        for file in &files {
            result.total_bytes += file.size_bytes;
            match file.kind {
                ExportFileKind::Article => result.article_count += 1,
                ExportFileKind::ArchiveZip => result.archive_count += 1,
                ExportFileKind::Other => {}
            }
        }
        log::info!(
            "Migration data imported: files={} articles={} archives={}",
            result.file_count,
            result.article_count,
            result.archive_count
        );
        Ok(result)
    }

    /// `imports/` を（無ければ作って）返す。リンクやファイルになっていたら使わない。
    fn imports_dir(&self) -> Result<PathBuf, AppError> {
        let imports_dir = self.app_data_dir.join(MIGRATION_IMPORTS_RELATIVE_DIR);
        std::fs::create_dir_all(&imports_dir)?;
        if !is_real_dir(&imports_dir) {
            return Err(AppError::Validation(
                "import folder is not a regular directory".to_string(),
            ));
        }
        Ok(imports_dir)
    }
}

/// 取り込み候補のファイル名か（`yuuko_transfer_<英数字・-・_>.zip`）。`/` `\` `..` `:` は通らない。
fn is_import_file_name(name: &str) -> bool {
    name.strip_prefix(EXPORT_FILE_PREFIX)
        .and_then(|rest| rest.strip_suffix(".zip"))
        .is_some_and(is_safe_name_component)
}

/// `imports/` 直下の、名前規則に合う通常ファイル（リンクは除く）とその大きさ。
fn list_candidate_files(imports_dir: &Path) -> Result<Vec<(String, u64)>, AppError> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(imports_dir)? {
        let entry = entry?;
        // DirEntry::file_type はリンク先を辿らない（Windows のジャンクションも通常ファイル扱いにならない）。
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if is_import_file_name(&name) {
            files.push((name, entry.metadata()?.len()));
        }
    }
    Ok(files)
}

/// 一覧表示用に manifest の作成日時だけを読む。読めなければ `None`（取り込み時に改めて検証する）。
fn read_candidate_created_at(path: &Path, limits: &ImportLimits) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    if file_len > limits.max_zip_bytes {
        return None;
    }
    precheck_central_directory(&mut file, file_len, limits).ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let manifest = read_manifest(&mut archive, limits.max_manifest_bytes).ok()?;
    // ZIP由来の文字列をそのまま画面へ出さないよう、日時として読めたものだけを整形し直す。
    DateTime::parse_from_rfc3339(&manifest.created_at)
        .ok()
        .map(|created_at| created_at.to_rfc3339_opts(SecondsFormat::Secs, false))
}

fn rejected(reason: &str) -> AppError {
    AppError::ImportRejected(reason.to_string())
}

/// ZIPを検証しながら一時フォルダへ展開する。戻り値はエントリ名の順に並べた展開済みファイル。
fn extract_and_validate(
    zip_path: &Path,
    staging: &Path,
    limits: &ImportLimits,
) -> Result<Vec<ExpectedFile>, AppError> {
    let mut file = File::open(zip_path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(rejected("not a regular file"));
    }
    if metadata.len() > limits.max_zip_bytes {
        return Err(rejected("the zip file is too large"));
    }
    precheck_central_directory(&mut file, metadata.len(), limits)?;
    file.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(file).map_err(|_| rejected("not a valid zip file"))?;
    if archive.len() as u64 > limits.max_entries {
        return Err(rejected("too many entries"));
    }

    let manifest = read_manifest(&mut archive, limits.max_manifest_bytes)?;
    let expected = validate_manifest(&manifest, limits)?;

    // manifest の一覧と実際のエントリが過不足なく一致すること（manifest.json 自身を除く）。
    if archive.len() != expected.len() + 1 {
        return Err(rejected("zip entries do not match the manifest"));
    }
    let expected_names: HashSet<&str> = expected
        .iter()
        .map(|file| file.entry_name.as_str())
        .collect();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|_| rejected("not a valid zip file"))?;
        let name = entry.name();
        if name.as_bytes() != entry.name_raw() || !is_safe_entry_path(name) {
            return Err(rejected("an entry has an unsafe name"));
        }
        if entry.is_dir() || entry.is_symlink() {
            return Err(rejected("an entry is not a regular file"));
        }
        if entry.encrypted() {
            return Err(rejected("encrypted entries are not supported"));
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(rejected("unsupported compression method"));
        }
        if name == MANIFEST_ENTRY_NAME {
            continue;
        }
        if !expected_names.contains(name) {
            return Err(rejected("zip entries do not match the manifest"));
        }
    }

    for file in &expected {
        let entry = archive
            .by_name(&file.entry_name)
            .map_err(|_| rejected("zip entries do not match the manifest"))?;
        // 先に宣言値（ヘッダ）でも弾く。実際の大きさは下で読んだバイト数で確かめる。
        if entry.size() != file.size_bytes {
            return Err(rejected("a file size does not match the manifest"));
        }
        let target = create_real_parent_dirs(staging, &file.entry_name)?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        match file.kind {
            ExportFileKind::ArchiveZip => {
                // 月次アーカイブZIPは展開せず、そのまま置く（大きさの上限は同じく実バイト数で確かめる）。
                copy_exact(entry, &mut out, file.size_bytes)?;
                drop(out);
                verify_nested_archive(&target, limits)?;
            }
            ExportFileKind::Article | ExportFileKind::Other => {
                let bytes = read_limited(entry, file.size_bytes)?;
                if bytes.len() as u64 != file.size_bytes {
                    return Err(rejected("a file size does not match the manifest"));
                }
                validate_content(&file.entry_name, file.kind, &bytes)?;
                out.write_all(&bytes)?;
            }
        }
    }
    Ok(expected)
}

/// manifest の形式と、`files` の各項目が許可リスト・大きさの上限に収まるかを確かめる。
fn validate_manifest(
    manifest: &MigrationManifest,
    limits: &ImportLimits,
) -> Result<Vec<ExpectedFile>, AppError> {
    if manifest.version != MIGRATION_MANIFEST_VERSION {
        return Err(rejected("unsupported manifest version"));
    }
    if manifest.encrypted {
        return Err(rejected("encrypted migration data is not supported"));
    }
    if !manifest
        .included
        .iter()
        .all(|category| CATEGORY_ORDER.contains(&category.as_str()))
    {
        return Err(rejected("the manifest has an unknown category"));
    }
    if manifest.files.len() as u64 + 1 > limits.max_entries {
        return Err(rejected("too many entries"));
    }

    let mut seen = HashSet::new();
    let mut total: u64 = 0;
    let mut expected = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let kind = if is_safe_entry_path(&file.path) {
            classify_entry_name(&file.path)
        } else {
            None
        };
        let Some(kind) = kind else {
            return Err(rejected("the zip contains an unexpected file"));
        };
        if !seen.insert(file.path.as_str()) {
            return Err(rejected("the manifest lists a file twice"));
        }
        if file.size_bytes > limits.per_entry(kind) {
            return Err(rejected("a file is too large"));
        }
        total = total
            .checked_add(file.size_bytes)
            .filter(|total| *total <= limits.max_total_bytes)
            .ok_or_else(|| rejected("the data is too large in total"))?;
        expected.push(ExpectedFile {
            entry_name: file.path.clone(),
            kind,
            size_bytes: file.size_bytes,
        });
    }
    expected.sort_by(|left, right| left.entry_name.cmp(&right.entry_name));
    Ok(expected)
}

/// `/` 区切りの相対パスとして安全か（許可リストの前に重ねて確かめる）。
fn is_safe_entry_path(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0'])
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn read_manifest<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    limit: u64,
) -> Result<MigrationManifest, AppError> {
    let entry = archive
        .by_name(MANIFEST_ENTRY_NAME)
        .map_err(|_| rejected("manifest.json is missing"))?;
    let bytes = read_limited(entry, limit)?;
    if bytes.len() as u64 > limit {
        return Err(rejected("manifest.json is too large"));
    }
    serde_json::from_slice::<MigrationManifest>(&bytes)
        .map_err(|_| rejected("manifest.json could not be read"))
}

/// 最大 `limit + 1` バイトまで読む（上限を超えたかは呼び出し側が長さで判断する）。
/// 壊れた圧縮データ・CRC 不一致は ZIP の不正として扱う。
fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| rejected("zip data is corrupted"))?;
    Ok(bytes)
}

/// ちょうど `expected` バイトを書き写す。多くても少なくても拒否する。
fn copy_exact(mut reader: impl Read, out: &mut File, expected: u64) -> Result<(), AppError> {
    let mut buffer = vec![0u8; COPY_CHUNK_BYTES];
    let mut copied: u64 = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| rejected("zip data is corrupted"))?;
        if read == 0 {
            break;
        }
        copied += read as u64;
        if copied > expected {
            return Err(rejected("a file size does not match the manifest"));
        }
        out.write_all(&buffer[..read])?;
    }
    if copied != expected {
        return Err(rejected("a file size does not match the manifest"));
    }
    Ok(())
}

/// 月次アーカイブZIPとして開けるか（中身は展開しない）。
fn verify_nested_archive(path: &Path, limits: &ImportLimits) -> Result<(), AppError> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    precheck_central_directory(&mut file, len, limits)
        .map_err(|_| rejected("a monthly archive is not a valid zip file"))?;
    file.seek(SeekFrom::Start(0))?;
    ZipArchive::new(file).map_err(|_| rejected("a monthly archive is not a valid zip file"))?;
    Ok(())
}

/// 記事は UTF-8 であること、JSON は読み込み時と同じ型として読めることを確かめる。
///
/// 記事の Front Matter は確かめない（読み込み側が壊れた記事を1件ずつ飛ばすため、
/// 移行元で読めていたデータが1件の破損で取り込めなくなるのを避ける）。
fn validate_content(entry_name: &str, kind: ExportFileKind, bytes: &[u8]) -> Result<(), AppError> {
    match kind {
        ExportFileKind::Article => std::str::from_utf8(bytes)
            .map(|_| ())
            .map_err(|_| rejected("an article is not valid UTF-8")),
        ExportFileKind::Other => validate_json_entry(entry_name, bytes),
        ExportFileKind::ArchiveZip => Ok(()),
    }
}

/// 固定ファイルのJSONを、各リポジトリの読み込みと同じ型で読む。
/// 許可リストに増えた固定ファイルは、ここへ型を足すまで取り込めない（読めないデータを入れないため）。
fn validate_json_entry(entry_name: &str, bytes: &[u8]) -> Result<(), AppError> {
    let invalid = || rejected("a data file is not in the expected format");
    let raw = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let without_bom = crate::util::strip_utf8_bom(raw);
    let parsed: Result<(), AppError> = match entry_name {
        SETTINGS_RELATIVE_PATH => serde_json::from_str::<PersistedSettings>(without_bom)
            .map(drop)
            .map_err(AppError::from),
        FRIENDSHIP_RELATIVE_PATH => serde_json::from_str::<FriendshipState>(without_bom)
            .map(drop)
            .map_err(AppError::from),
        REWARDS_RELATIVE_PATH => serde_json::from_str::<RewardsState>(without_bom)
            .map(drop)
            .map_err(AppError::from),
        GACHA_STATE_RELATIVE_PATH => serde_json::from_str::<GachaState>(without_bom)
            .map(drop)
            .map_err(AppError::from),
        // 辞書の読み込みは BOM を除かないので、ここでも除かずに読む。
        DICTIONARY_RELATIVE_PATH => serde_json::from_str::<PersistedDictionaryStore>(raw)
            .map(drop)
            .map_err(AppError::from),
        ARTICLE_FAVORITES_RELATIVE_PATH => validate_imported_favorites_json(raw),
        ARCHIVE_INDEX_RELATIVE_PATH => validate_imported_archive_index_json(raw),
        _ => Err(invalid()),
    };
    parsed.map_err(|_| invalid())
}

/// ZIPの末尾レコードから、エントリ数と中央ディレクトリの大きさを読んで上限を確かめる。
///
/// `ZipArchive::new` は中央ディレクトリを全部メモリへ読み込むため、その前に件数で弾く。
/// 末尾にコメント以外のデータが付いたZIPは受け付けない（書き出しは付けない）。
fn precheck_central_directory(
    file: &mut File,
    file_len: u64,
    limits: &ImportLimits,
) -> Result<(), AppError> {
    const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const EOCD_LEN: usize = 22;
    const ZIP64_LOCATOR_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
    const ZIP64_LOCATOR_LEN: u64 = 20;
    const ZIP64_EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
    const ZIP64_EOCD_LEN: usize = 56;

    let invalid = || rejected("not a valid zip file");
    if file_len < EOCD_LEN as u64 {
        return Err(invalid());
    }
    let tail_len = file_len.min(EOCD_LEN as u64 + u64::from(u16::MAX));
    let tail_start = file_len - tail_len;
    let mut tail = vec![0u8; tail_len as usize];
    file.seek(SeekFrom::Start(tail_start))?;
    file.read_exact(&mut tail)?;

    let eocd_pos = (0..=tail.len() - EOCD_LEN)
        .rev()
        .find(|&pos| {
            let comment_len = usize::from(u16::from_le_bytes([tail[pos + 20], tail[pos + 21]]));
            tail[pos..pos + 4] == EOCD_SIGNATURE && pos + EOCD_LEN + comment_len == tail.len()
        })
        .ok_or_else(invalid)?;
    let eocd = &tail[eocd_pos..];
    let mut entries = u64::from(u16::from_le_bytes([eocd[10], eocd[11]]));
    let mut directory_size =
        u64::from(u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]));

    if entries == u64::from(u16::MAX) || directory_size == u64::from(u32::MAX) {
        // ZIP64: 直前のロケータから ZIP64 の末尾レコードを読む。
        let eocd_offset = tail_start + eocd_pos as u64;
        if eocd_offset < ZIP64_LOCATOR_LEN {
            return Err(invalid());
        }
        let mut locator = [0u8; ZIP64_LOCATOR_LEN as usize];
        file.seek(SeekFrom::Start(eocd_offset - ZIP64_LOCATOR_LEN))?;
        file.read_exact(&mut locator)?;
        if locator[..4] != ZIP64_LOCATOR_SIGNATURE {
            return Err(invalid());
        }
        let record_offset = u64::from_le_bytes(locator[8..16].try_into().map_err(|_| invalid())?);
        if record_offset.saturating_add(ZIP64_EOCD_LEN as u64) > file_len {
            return Err(invalid());
        }
        let mut record = [0u8; ZIP64_EOCD_LEN];
        file.seek(SeekFrom::Start(record_offset))?;
        file.read_exact(&mut record)?;
        if record[..4] != ZIP64_EOCD_SIGNATURE {
            return Err(invalid());
        }
        entries = u64::from_le_bytes(record[32..40].try_into().map_err(|_| invalid())?);
        directory_size = u64::from_le_bytes(record[40..48].try_into().map_err(|_| invalid())?);
    }

    if entries > limits.max_entries {
        return Err(rejected("too many entries"));
    }
    if directory_size > limits.max_central_directory_bytes {
        return Err(rejected("the zip directory is too large"));
    }
    Ok(())
}

/// `relative` の親フォルダを、リンクを辿らずに1段ずつ作る（既存のものは実体のフォルダであること）。
/// 戻り値は `relative` のフルパス。
fn create_real_parent_dirs(root: &Path, relative: &str) -> Result<PathBuf, AppError> {
    let mut parts: Vec<&str> = relative.split('/').collect();
    let file_name = parts.pop().unwrap_or_default();
    let mut current = root.to_path_buf();
    for part in parts {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(AppError::Validation(
                    "a data folder is not a regular directory".to_string(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    current.push(file_name);
    Ok(current)
}

/// 前回の取り込みが差し替えの途中で止まっていたら、新しい取り込みを始めない。
fn ensure_no_incomplete_backup(backups_root: &Path) -> Result<(), AppError> {
    if std::fs::symlink_metadata(backups_root).is_err() {
        return Ok(());
    }
    if !is_real_dir(backups_root) {
        return Err(AppError::Validation(
            "backup folder is not a regular directory".to_string(),
        ));
    }
    for entry in std::fs::read_dir(backups_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && std::fs::symlink_metadata(entry.path().join(INCOMPLETE_MARKER)).is_ok()
        {
            log::error!("A previous migration import did not finish; manual restore is required");
            return Err(AppError::ImportIncompletePrevious);
        }
    }
    Ok(())
}

/// 一時フォルダを空の状態で用意する（前回の中断で残ったものは取り込みデータの写しなので消してよい）。
fn reset_staging_dir(staging: &Path) -> Result<(), AppError> {
    match std::fs::symlink_metadata(staging) {
        Ok(metadata) if metadata.file_type().is_dir() => std::fs::remove_dir_all(staging)?,
        Ok(_) => {
            return Err(AppError::Validation(
                "import staging folder is not a regular directory".to_string(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    std::fs::create_dir(staging)?;
    Ok(())
}

fn remove_staging_dir(staging: &Path) {
    if is_real_dir(staging) {
        if let Err(error) = std::fs::remove_dir_all(staging) {
            log::warn!(
                "Failed to remove the migration import staging folder: {}",
                error.kind()
            );
        }
    }
}

/// 取り込みで置き換える「現在のデータ」の一覧（アプリデータ直下からの相対パス）。
///
/// 許可リストのファイルに加え、置き換え後に古いデータが復元されないよう、各リポジトリが起動時・読み込み時に
/// 自動で戻す退避物（`*.json.bak` / `*.json.tmp`、月次ZIPの `*.zip.rollback` 等、Markdown退避の一時フォルダ）も
/// 一緒に退避フォルダへ移す。`state/`・ニュース取得設定・ログなど許可リスト外のものは触らない。
///
/// `replace_gacha` が false（取り込むZIPにガチャ状態が無い）のときは、ガチャ状態とその退避物
/// （`gacha_state.json.bak` / `.tmp`）を一覧に入れず、移行先の現在のガチャ状態をそのまま残す。
fn current_migration_entries(root: &Path, replace_gacha: bool) -> Result<Vec<String>, AppError> {
    let mut entries: Vec<String> = plan_migration_files(root)?
        .into_iter()
        .map(|planned| planned.entry_name)
        .filter(|entry_name| replace_gacha || category_of(entry_name) != GACHA_CATEGORY)
        .collect();
    for fixed in FIXED_FILES {
        if !replace_gacha && category_of(fixed) == GACHA_CATEGORY {
            continue;
        }
        for suffix in [".bak", ".tmp"] {
            let relative = format!("{fixed}{suffix}");
            if parents_are_real_dirs(root, &relative)
                && std::fs::symlink_metadata(root.join(&relative)).is_ok()
            {
                entries.push(relative);
            }
        }
    }
    let archive_dir = root.join(ARCHIVE_RELATIVE_DIR);
    if is_real_dir(&archive_dir) {
        for entry in std::fs::read_dir(&archive_dir)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let is_leftover = name == RETIREMENT_ROLLBACK_DIR
                || name == RETIREMENT_COMMITTED_DIR
                || [".rollback", ".tmp", ".bak"].iter().any(|suffix| {
                    name.strip_suffix(suffix)
                        .is_some_and(is_month_archive_file_name)
                });
            if is_leftover {
                entries.push(format!("{ARCHIVE_RELATIVE_DIR}/{name}"));
            }
        }
    }
    entries.sort();
    entries.dedup();
    Ok(entries)
}

/// 現在のデータを退避フォルダへ移し、展開したファイルを所定の場所へ置く。
/// 失敗したら置いたものを消して退避分を戻し、元のエラーを返す。成功時は退避フォルダのパスを返す。
fn swap_in(
    root: &Path,
    staging: &Path,
    backups_root: &Path,
    files: &[ExpectedFile],
    now: DateTime<FixedOffset>,
) -> Result<PathBuf, AppError> {
    let backup_dir = create_backup_dir(backups_root, now)?;
    let marker = backup_dir.join(INCOMPLETE_MARKER);
    if let Err(error) = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
    {
        let _ = std::fs::remove_dir(&backup_dir);
        return Err(error.into());
    }

    let mut journal = SwapJournal::default();
    let result = move_and_place(root, staging, &backup_dir, files, &mut journal);
    match result {
        Ok(()) => {
            if let Err(error) = std::fs::remove_file(&marker) {
                // 取り込み自体は終わっている。印が残ると次の取り込みが止まるので記録だけ残す。
                log::warn!(
                    "Failed to clear the migration import marker: {}",
                    error.kind()
                );
            }
            Ok(backup_dir)
        }
        Err(error) => {
            log::warn!("Migration import failed while replacing data; restoring the backup");
            match rollback(root, &backup_dir, &journal) {
                Ok(()) => {
                    // 戻し終えた退避フォルダは空のフォルダだけなので消す（印から先に消す）。
                    if std::fs::remove_file(&marker).is_ok() {
                        let _ = std::fs::remove_dir_all(&backup_dir);
                    }
                }
                Err(rollback_error) => {
                    // 退避フォルダと印は残す（手動で戻すためのデータが入っている）。
                    log::error!(
                        "Failed to restore data after a migration import failure: {rollback_error}"
                    );
                }
            }
            Err(error)
        }
    }
}

fn move_and_place(
    root: &Path,
    staging: &Path,
    backup_dir: &Path,
    files: &[ExpectedFile],
    journal: &mut SwapJournal,
) -> Result<(), AppError> {
    // ガチャ状態を含まない古いZIPでは、現在のガチャ状態を残す（仮の設計判断・データ設計書 §15.7）。
    // 全置き換えにすると、ガチャ保存の実装前に書き出したZIPを取り込んだだけで獲得済みのものとかけらが消えるため。
    let replace_gacha = files
        .iter()
        .any(|file| category_of(&file.entry_name) == GACHA_CATEGORY);
    for relative in current_migration_entries(root, replace_gacha)? {
        let destination = create_real_parent_dirs(backup_dir, &relative)?;
        std::fs::rename(root.join(&relative), destination)?;
        journal.moved_out.push(relative);
    }
    for file in files {
        let target = create_real_parent_dirs(root, &file.entry_name)?;
        // 退避後も残っているもの（リンク・フォルダなど許可リストで退避しなかったもの）は上書きしない。
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(AppError::Archive(
                "an import target already exists".to_string(),
            ));
        }
        std::fs::rename(staging.join(&file.entry_name), &target)?;
        journal.placed.push(file.entry_name.clone());
    }
    Ok(())
}

/// 置いたファイルを消し、退避したものを元の場所へ戻す。1件でも戻せなければエラー。
fn rollback(root: &Path, backup_dir: &Path, journal: &SwapJournal) -> Result<(), AppError> {
    let mut failures = 0usize;
    for relative in journal.placed.iter().rev() {
        if let Err(error) = std::fs::remove_file(root.join(relative)) {
            if error.kind() != std::io::ErrorKind::NotFound {
                failures += 1;
            }
        }
    }
    for relative in journal.moved_out.iter().rev() {
        let restored = create_real_parent_dirs(root, relative)
            .and_then(|target| Ok(std::fs::rename(backup_dir.join(relative), target)?));
        if restored.is_err() {
            failures += 1;
        }
    }
    if failures > 0 {
        return Err(AppError::Archive(format!(
            "{failures} item(s) could not be restored"
        )));
    }
    Ok(())
}

/// `migration-backups/<YYYYMMDDhhmmss>` を新しく作る（同じ名前があれば `_2` 以降を付ける）。
fn create_backup_dir(backups_root: &Path, now: DateTime<FixedOffset>) -> Result<PathBuf, AppError> {
    std::fs::create_dir_all(backups_root)?;
    if !is_real_dir(backups_root) {
        return Err(AppError::Validation(
            "backup folder is not a regular directory".to_string(),
        ));
    }
    let base = now.format("%Y%m%d%H%M%S").to_string();
    for attempt in 1..=MAX_BACKUP_NAME_ATTEMPTS {
        let name = if attempt == 1 {
            base.clone()
        } else {
            format!("{base}_{attempt}")
        };
        let path = backups_root.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(AppError::Validation(
        "could not choose a backup folder name".to_string(),
    ))
}

/// 退避フォルダを最新の1世代だけ残す。消せなくても取り込み結果は変えない。
fn prune_old_backups(backups_root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(backups_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_old_backup = path != keep
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry.file_name().to_str().is_some_and(is_backup_dir_name)
            && std::fs::symlink_metadata(path.join(INCOMPLETE_MARKER)).is_err();
        if is_old_backup {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                log::warn!("Failed to remove an old migration backup: {}", error.kind());
            }
        }
    }
}

/// `YYYYMMDDhhmmss` または `YYYYMMDDhhmmss_<連番>`。利用者が置いた別のフォルダは消さない。
fn is_backup_dir_name(name: &str) -> bool {
    let (stamp, suffix) = match name.split_once('_') {
        Some((stamp, suffix)) => (stamp, Some(suffix)),
        None => (name, None),
    };
    stamp.len() == 14
        && stamp.bytes().all(|byte| byte.is_ascii_digit())
        && suffix.is_none_or(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::data_export::MigrationManifestFile;
    use crate::services::data_export_service::DataExportService;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "yuuko-migration-import-{label}-{}-{nanos}",
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

    fn read(root: &Path, relative: &str) -> String {
        std::fs::read_to_string(root.join(relative)).unwrap()
    }

    fn fixed_now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-10-08T15:00:00+09:00").unwrap()
    }

    fn service(root: &Path) -> DataImportService {
        service_with_limits(root, ImportLimits::default())
    }

    fn service_with_limits(root: &Path, limits: ImportLimits) -> DataImportService {
        DataImportService {
            app_data_dir: root.to_path_buf(),
            migration_lock: Arc::new(Mutex::new(())),
            write_locks: MigrationWriteLocks::default(),
            limits,
        }
    }

    fn friendship_json(total: u32) -> String {
        format!(
            r#"{{"version":1,"currentRank":1,"currentPoints":{total},"totalPoints":{total},"dailyPoints":{{"date":"2026-10-08","points":0,"limit":25}},"rankMax":10}}"#
        )
    }

    /// 移行元のデータ（書き出しサービスで実際にZIPを作る）。
    fn seed_source(root: &Path) {
        write(root, "config/settings.json", r#"{"aiProvider":"mock"}"#);
        write(
            root,
            "favorites/article_favorites.json",
            r#"{"version":1,"favorite_article_ids":["new-1"]}"#,
        );
        write(
            root,
            "dictionary/entries.json",
            r#"{"version":1,"entries":[]}"#,
        );
        write(root, "user/friendship.json", &friendship_json(42));
        write(root, "rewards/rewards.json", "{}");
        write(root, "gacha/gacha_state.json", GACHA_SOURCE);
        write(root, "news/202610/new-1.md", "---\n---\nnew article");
        write(root, "news/new-2.md", "---\n---\nnew article 2");
        write(
            root,
            "archive/archive_index.json",
            r#"{"version":1,"archives":[]}"#,
        );
        write_month_zip(&root.join("archive/2026-05.zip"));
    }

    /// 移行先の現在のデータ（取り込みで置き換わるものと、残るべきもの）。
    fn seed_target(root: &Path) {
        write(root, "config/settings.json", r#"{"aiProvider":"gemini"}"#);
        write(root, "config/settings.json.bak", r#"{"aiProvider":"old"}"#);
        write(
            root,
            "dictionary/entries.json",
            r#"{"version":1,"entries":[]}"#,
        );
        write(root, "user/friendship.json", &friendship_json(7));
        write(root, "gacha/gacha_state.json", GACHA_TARGET);
        // ガチャ保存の退避物（取り込みで置き換えるときは一緒に退避する）と破損退避（触らない）。
        write(root, "gacha/gacha_state.json.bak", GACHA_STALE);
        write(root, "gacha/gacha_state.corrupt.json", "CORRUPT");
        write(root, "news/202609/old-1.md", "---\n---\nold article");
        write(root, "archive/2026-04.zip.rollback", "rollback");
        // 許可リスト外（取り込みで触らない）
        write(root, "config/news_sources.json", "SOURCES");
        write(root, "config/network_allowlist.json", "ALLOWLIST");
        write(root, "state/yuuko_notification_state.json", "{}");
        write(root, "logs/app.log", "log");
    }

    fn write_month_zip(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        writer
            .start_file("news_0123456789abcdef.md", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"---\n---\narchived").unwrap();
        writer.finish().unwrap();
    }

    /// 移行元から書き出したZIPを、移行先の `imports/` へ置いてファイル名を返す。
    fn place_exported_zip(source: &Path, target: &Path) -> String {
        let result = DataExportService::new(&AppPaths::new(source.to_path_buf()))
            .export_migration_data()
            .unwrap();
        std::fs::create_dir_all(target.join("imports")).unwrap();
        std::fs::copy(
            source.join("exports").join(&result.file_name),
            target.join("imports").join(&result.file_name),
        )
        .unwrap();
        result.file_name
    }

    fn manifest_for(entries: &[(&str, &[u8])]) -> MigrationManifest {
        MigrationManifest {
            version: MIGRATION_MANIFEST_VERSION,
            app_version: "0.1.0".to_string(),
            transfer_id: "tr_test".to_string(),
            created_at: "2026-10-08T14:00:00+09:00".to_string(),
            included: vec!["config".to_string()],
            encrypted: false,
            files: entries
                .iter()
                .map(|(path, bytes)| MigrationManifestFile {
                    path: path.to_string(),
                    size_bytes: bytes.len() as u64,
                })
                .collect(),
        }
    }

    /// 任意のエントリと manifest でZIPを作り、移行先の `imports/` に置く。
    fn place_crafted_zip(
        target: &Path,
        file_name: &str,
        entries: &[(&str, &[u8])],
        manifest: &MigrationManifest,
    ) {
        let path = target.join("imports").join(file_name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer
            .start_file(MANIFEST_ENTRY_NAME, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(manifest).unwrap())
            .unwrap();
        writer.finish().unwrap();
    }

    /// （ラベル, ZIPに入れるエントリ, manifest）
    type RejectCase<'a> = (&'a str, Vec<(&'a str, &'a [u8])>, MigrationManifest);

    const SETTINGS: &[u8] = br#"{"aiProvider":"mock"}"#;
    const GACHA_SOURCE: &str = r#"{"starFragments":43,"ownedItemIds":["card_001"]}"#;
    const GACHA_TARGET: &str = r#"{"starFragments":7,"ownedItemIds":["gacha_theme_001"]}"#;
    const GACHA_STALE: &str = r#"{"starFragments":1}"#;

    /// 拒否されたあとも現在のデータがそのままで、一時フォルダ・退避フォルダが残っていないこと。
    fn assert_target_untouched(target: &Path) {
        assert_eq!(
            read(target, "config/settings.json"),
            r#"{"aiProvider":"gemini"}"#
        );
        assert_eq!(
            read(target, "config/settings.json.bak"),
            r#"{"aiProvider":"old"}"#
        );
        assert_eq!(read(target, "user/friendship.json"), friendship_json(7));
        assert_eq!(read(target, "gacha/gacha_state.json"), GACHA_TARGET);
        assert_eq!(read(target, "gacha/gacha_state.json.bak"), GACHA_STALE);
        assert_eq!(
            read(target, "news/202609/old-1.md"),
            "---\n---\nold article"
        );
        assert_eq!(read(target, "archive/2026-04.zip.rollback"), "rollback");
        assert!(!target.join(MIGRATION_IMPORT_STAGING_RELATIVE_DIR).exists());
        let backups = target.join(MIGRATION_BACKUPS_RELATIVE_DIR);
        assert!(!backups.exists() || std::fs::read_dir(&backups).unwrap().next().is_none());
    }

    fn assert_rejected(result: Result<MigrationImportResultDto, AppError>) {
        assert!(
            matches!(result, Err(AppError::ImportRejected(_))),
            "{result:?}"
        );
    }

    #[test]
    fn import_replaces_allowlisted_data_and_keeps_one_backup() {
        let source = temp_dir("source");
        let target = temp_dir("target");
        seed_source(&source);
        seed_target(&target);
        let file_name = place_exported_zip(&source, &target);
        // 前回の取り込みの退避フォルダ（1世代だけ残すので消える）と、利用者が置いた別のフォルダ（消さない）。
        write(
            &target,
            "migration-backups/20260101000000/config/settings.json",
            "{}",
        );
        write(&target, "migration-backups/keep-me/note.txt", "note");

        let result = service(&target).import_at(&file_name, fixed_now()).unwrap();

        assert_eq!(result.file_name, file_name);
        assert_eq!(result.file_count, 10);
        assert_eq!(result.article_count, 2);
        assert_eq!(result.archive_count, 1);
        assert!(result.restart_required);
        // 取り込んだデータに置き換わる。
        for relative in [
            "config/settings.json",
            "favorites/article_favorites.json",
            "dictionary/entries.json",
            "user/friendship.json",
            "rewards/rewards.json",
            "gacha/gacha_state.json",
            "news/202610/new-1.md",
            "news/new-2.md",
            "archive/archive_index.json",
        ] {
            assert_eq!(
                read(&target, relative),
                read(&source, relative),
                "{relative}"
            );
        }
        assert_eq!(
            std::fs::read(target.join("archive/2026-05.zip")).unwrap(),
            std::fs::read(source.join("archive/2026-05.zip")).unwrap()
        );
        // 取り込みに無い現在のデータ・自動復元される退避物は退避フォルダへ移る。
        assert!(!target.join("news/202609/old-1.md").exists());
        assert!(!target.join("config/settings.json.bak").exists());
        assert!(!target.join("archive/2026-04.zip.rollback").exists());
        let backup = target.join("migration-backups/20261008150000");
        assert_eq!(
            read(&backup, "config/settings.json"),
            r#"{"aiProvider":"gemini"}"#
        );
        assert_eq!(
            read(&backup, "config/settings.json.bak"),
            r#"{"aiProvider":"old"}"#
        );
        assert_eq!(
            read(&backup, "news/202609/old-1.md"),
            "---\n---\nold article"
        );
        assert_eq!(read(&backup, "user/friendship.json"), friendship_json(7));
        assert_eq!(read(&backup, "archive/2026-04.zip.rollback"), "rollback");
        // ガチャ状態も置き換わり、古い .bak は退避フォルダへ移る（次回の読み込みで復元されない）。
        assert!(!target.join("gacha/gacha_state.json.bak").exists());
        assert_eq!(read(&backup, "gacha/gacha_state.json"), GACHA_TARGET);
        assert_eq!(read(&backup, "gacha/gacha_state.json.bak"), GACHA_STALE);
        // 破損退避は許可リスト外なので触らない。
        assert_eq!(read(&target, "gacha/gacha_state.corrupt.json"), "CORRUPT");
        assert!(!backup.join(INCOMPLETE_MARKER).exists());
        assert!(!target.join("migration-backups/20260101000000").exists());
        assert_eq!(read(&target, "migration-backups/keep-me/note.txt"), "note");
        // 許可リスト外は触らない。一時フォルダは残らない。
        assert_eq!(read(&target, "config/news_sources.json"), "SOURCES");
        assert_eq!(read(&target, "config/network_allowlist.json"), "ALLOWLIST");
        assert_eq!(read(&target, "state/yuuko_notification_state.json"), "{}");
        assert_eq!(read(&target, "logs/app.log"), "log");
        assert!(!target.join(MIGRATION_IMPORT_STAGING_RELATIVE_DIR).exists());
        // 取り込みZIP自体は残す。
        assert!(target.join("imports").join(&file_name).exists());

        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn import_without_gacha_state_keeps_the_current_gacha_state() {
        let source = temp_dir("no-gacha-source");
        let target = temp_dir("no-gacha-target");
        seed_source(&source);
        // ガチャ保存の実装前に書き出したZIP（gacha 区分が無い）を再現する。
        std::fs::remove_file(source.join("gacha/gacha_state.json")).unwrap();
        seed_target(&target);
        let file_name = place_exported_zip(&source, &target);

        let result = service(&target).import_at(&file_name, fixed_now()).unwrap();

        assert_eq!(result.file_count, 9);
        // 他のデータは置き換わる。
        assert_eq!(read(&target, "user/friendship.json"), friendship_json(42));
        assert!(!target.join("news/202609/old-1.md").exists());
        // 現在のガチャ状態とその退避物は、退避も置き換えもせずに残す。
        assert_eq!(read(&target, "gacha/gacha_state.json"), GACHA_TARGET);
        assert_eq!(read(&target, "gacha/gacha_state.json.bak"), GACHA_STALE);
        assert_eq!(read(&target, "gacha/gacha_state.corrupt.json"), "CORRUPT");
        let backup = target.join("migration-backups/20261008150000");
        assert!(!backup.join("gacha").exists());

        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn import_waits_for_the_gacha_store_lock() {
        let source = temp_dir("gacha-lock-source");
        let target = temp_dir("gacha-lock-target");
        seed_source(&source);
        seed_target(&target);
        let file_name = place_exported_zip(&source, &target);
        let gacha_lock = Arc::new(Mutex::new(()));
        let import = DataImportService {
            write_locks: MigrationWriteLocks::new(
                Arc::default(),
                Arc::default(),
                Arc::default(),
                Arc::default(),
                Arc::clone(&gacha_lock),
            ),
            ..service(&target)
        };

        // ガチャの保存（引く・かけら付与）がロックを持っている間は差し替えを始めない。
        let held = gacha_lock.lock().unwrap();
        let handle = std::thread::spawn(move || import.import_at(&file_name, fixed_now()));
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(read(&target, "gacha/gacha_state.json"), GACHA_TARGET);
        assert_eq!(read(&target, "user/friendship.json"), friendship_json(7));
        drop(held);
        handle.join().unwrap().unwrap();
        assert_eq!(read(&target, "gacha/gacha_state.json"), GACHA_SOURCE);

        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn list_returns_only_regular_files_matching_the_name_rule() {
        let source = temp_dir("list-source");
        let target = temp_dir("list-target");
        seed_source(&source);
        let exported = place_exported_zip(&source, &target);
        write(&target, "imports/yuuko_transfer_broken.zip", "not a zip");
        write(&target, "imports/other.zip", "x");
        write(&target, "imports/yuuko_transfer_a.zip.tmp", "x");
        write(&target, "imports/yuuko_transfer_bad name.zip", "x");
        std::fs::create_dir_all(target.join("imports/yuuko_transfer_dir.zip")).unwrap();

        let candidates = service(&target).list_migration_imports().unwrap();

        let names: Vec<&str> = candidates.iter().map(|c| c.file_name.as_str()).collect();
        assert_eq!(names, vec![exported.as_str(), "yuuko_transfer_broken.zip"]);
        let exported_candidate = &candidates[0];
        assert!(exported_candidate.size_bytes > 0);
        assert!(exported_candidate.created_at.is_some());
        assert_eq!(candidates[1].created_at, None);
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn list_creates_the_import_folder_when_missing() {
        let target = temp_dir("list-empty");

        assert!(service(&target)
            .list_migration_imports()
            .unwrap()
            .is_empty());
        assert!(is_real_dir(&target.join("imports")));
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn import_rejects_names_that_are_not_candidates() {
        let target = temp_dir("names");
        seed_target(&target);
        write(&target, "yuuko_transfer_outside.zip", "x");
        write(&target, "imports/yuuko_transfer_ok.zip.tmp", "x");
        let service = service(&target);

        for name in [
            "../yuuko_transfer_outside.zip",
            "..\\yuuko_transfer_outside.zip",
            "imports/yuuko_transfer_ok.zip",
            "C:\\yuuko_transfer_ok.zip",
            "/tmp/yuuko_transfer_ok.zip",
            "yuuko_transfer_ok.zip.tmp",
            "other.zip",
            "",
        ] {
            assert!(
                matches!(
                    service.import_at(name, fixed_now()),
                    Err(AppError::Validation(_))
                ),
                "{name}"
            );
        }
        assert!(matches!(
            service.import_at("yuuko_transfer_missing.zip", fixed_now()),
            Err(AppError::NotFound(_))
        ));
        assert_target_untouched(&target);
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn import_rejects_invalid_zips_without_touching_current_data() {
        let target = temp_dir("invalid");
        seed_target(&target);
        let service = service(&target);
        let good: &[(&str, &[u8])] = &[("config/settings.json", SETTINGS)];

        let mut cases: Vec<RejectCase> = Vec::new();
        // zip-slip / 許可リスト外の名前
        for bad_name in [
            "../config/settings.json",
            "config/../../evil.json",
            "/config/settings.json",
            "config\\settings.json",
            "C:/config/settings.json",
            "config/news_sources.json",
            "secrets/api_key.txt",
            "gacha/gacha_state.corrupt.json",
            "gacha/gacha_state.json.bak",
        ] {
            let entries = vec![(bad_name, SETTINGS)];
            let manifest = manifest_for(&entries);
            cases.push(("unsafe-name", entries, manifest));
        }
        // manifest に無いエントリ
        let extra = vec![
            ("config/settings.json", SETTINGS),
            ("news/x.md", b"x" as &[u8]),
        ];
        cases.push(("extra-entry", extra, manifest_for(good)));
        // manifest にあるがZIPに無いエントリ
        let mut missing_manifest = manifest_for(good);
        missing_manifest.files.push(MigrationManifestFile {
            path: "news/x.md".to_string(),
            size_bytes: 1,
        });
        cases.push(("missing-entry", good.to_vec(), missing_manifest));
        // 未対応の版・暗号化
        let mut future = manifest_for(good);
        future.version = MIGRATION_MANIFEST_VERSION + 1;
        cases.push(("version", good.to_vec(), future));
        let mut encrypted = manifest_for(good);
        encrypted.encrypted = true;
        cases.push(("encrypted", good.to_vec(), encrypted));
        // 大きさが manifest と合わない（実際の方が大きい）
        let mut smaller = manifest_for(good);
        smaller.files[0].size_bytes = 3;
        cases.push(("size-mismatch", good.to_vec(), smaller));
        // JSON が型として読めない
        for (path, bytes) in [
            ("config/settings.json", b"{not json" as &[u8]),
            ("user/friendship.json", b"{}"),
            ("dictionary/entries.json", b"\"x\""),
            ("favorites/article_favorites.json", b"\"x\""),
            ("rewards/rewards.json", b"{\"version\":\"one\"}"),
            ("gacha/gacha_state.json", b"{\"starFragments\":\"many\"}"),
            ("archive/archive_index.json", b"{\"version\":99}"),
        ] {
            let entries = vec![(path, bytes)];
            let manifest = manifest_for(&entries);
            cases.push(("bad-json", entries, manifest));
        }
        // 記事が UTF-8 でない・月次ZIPがZIPでない
        let entries = vec![("news/a.md", b"\xff\xfe" as &[u8])];
        let manifest = manifest_for(&entries);
        cases.push(("bad-article", entries, manifest));
        let entries = vec![("archive/2026-05.zip", b"not a zip" as &[u8])];
        let manifest = manifest_for(&entries);
        cases.push(("bad-month-zip", entries, manifest));

        for (index, (label, entries, manifest)) in cases.iter().enumerate() {
            let file_name = format!("yuuko_transfer_case{index}.zip");
            place_crafted_zip(&target, &file_name, entries, manifest);
            let result = service.import_at(&file_name, fixed_now());
            assert!(result.is_err(), "{label} was accepted");
            assert_rejected(result);
            assert_target_untouched(&target);
        }

        // ZIPでないファイル
        write(&target, "imports/yuuko_transfer_text.zip", "not a zip");
        assert_rejected(service.import_at("yuuko_transfer_text.zip", fixed_now()));
        // manifest.json が無い
        let path = target.join("imports/yuuko_transfer_nomanifest.zip");
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        writer
            .start_file("config/settings.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(SETTINGS).unwrap();
        writer.finish().unwrap();
        assert_rejected(service.import_at("yuuko_transfer_nomanifest.zip", fixed_now()));
        assert_target_untouched(&target);
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn import_rejects_directory_and_symlink_entries() {
        let target = temp_dir("entry-types");
        seed_target(&target);
        let service = service(&target);

        let path = target.join("imports/yuuko_transfer_dir.zip");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        writer
            .add_directory("config/settings.json/", SimpleFileOptions::default())
            .unwrap();
        let mut manifest = manifest_for(&[]);
        manifest.files.push(MigrationManifestFile {
            path: "config/settings.json".to_string(),
            size_bytes: 0,
        });
        writer
            .start_file(MANIFEST_ENTRY_NAME, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer.finish().unwrap();
        assert_rejected(service.import_at("yuuko_transfer_dir.zip", fixed_now()));

        let path = target.join("imports/yuuko_transfer_link.zip");
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        writer
            .add_symlink(
                "config/settings.json",
                "../../outside.json",
                SimpleFileOptions::default(),
            )
            .unwrap();
        let mut manifest = manifest_for(&[]);
        manifest.files.push(MigrationManifestFile {
            path: "config/settings.json".to_string(),
            size_bytes: "../../outside.json".len() as u64,
        });
        writer
            .start_file(MANIFEST_ENTRY_NAME, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer.finish().unwrap();
        assert_rejected(service.import_at("yuuko_transfer_link.zip", fixed_now()));

        assert_target_untouched(&target);
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn import_enforces_size_and_entry_limits() {
        let target = temp_dir("limits");
        seed_target(&target);
        let article = vec![b'a'; 64];
        let entries: Vec<(&str, &[u8])> = vec![
            ("config/settings.json", SETTINGS),
            ("news/a.md", &article),
            ("news/b.md", &article),
        ];
        place_crafted_zip(
            &target,
            "yuuko_transfer_big.zip",
            &entries,
            &manifest_for(&entries),
        );

        let small_article = ImportLimits {
            max_article_bytes: 63,
            ..ImportLimits::default()
        };
        let small_total = ImportLimits {
            max_total_bytes: 64 * 2,
            ..ImportLimits::default()
        };
        let few_entries = ImportLimits {
            max_entries: 3,
            ..ImportLimits::default()
        };
        let small_zip = ImportLimits {
            max_zip_bytes: 100,
            ..ImportLimits::default()
        };
        for limits in [small_article, small_total, few_entries, small_zip] {
            assert_rejected(
                service_with_limits(&target, limits)
                    .import_at("yuuko_transfer_big.zip", fixed_now()),
            );
            assert_target_untouched(&target);
        }
        // 上限内なら取り込める。
        assert!(service(&target)
            .import_at("yuuko_transfer_big.zip", fixed_now())
            .is_ok());
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn manifest_cannot_understate_the_real_size() {
        // manifest とヘッダが小さく宣言していても、実際に読んだバイト数で弾く。
        let target = temp_dir("understate");
        let staging = target.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let mut out = File::create(staging.join("x")).unwrap();
        let data = [0u8; 100];

        let result = copy_exact(&data[..], &mut out, 10);

        assert!(matches!(result, Err(AppError::ImportRejected(_))));
        assert!(read_limited(&data[..], 10).unwrap().len() > 10);
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn failure_while_replacing_restores_the_backup() {
        let source = temp_dir("rollback-source");
        let target = temp_dir("rollback-target");
        seed_source(&source);
        seed_target(&target);
        let file_name = place_exported_zip(&source, &target);
        // 置き換え先がフォルダになっていて置けない（退避の対象外で、archive/・config/ を置いた後に失敗する）。
        std::fs::remove_file(target.join("dictionary/entries.json")).unwrap();
        write(&target, "dictionary/entries.json/blocker.txt", "blocker");

        let result = service(&target).import_at(&file_name, fixed_now());

        assert!(matches!(result, Err(AppError::Archive(_))), "{result:?}");
        // 置いたものは消え、退避したものは元へ戻る。
        assert!(!target.join("archive/2026-05.zip").exists());
        assert!(!target.join("archive/archive_index.json").exists());
        assert!(!target.join("news/new-2.md").exists());
        assert_eq!(
            read(&target, "dictionary/entries.json/blocker.txt"),
            "blocker"
        );
        assert_target_untouched(&target);
        // 失敗後に原因を取り除けば、もう一度取り込める。
        std::fs::remove_dir_all(target.join("dictionary/entries.json")).unwrap();
        assert!(service(&target).import_at(&file_name, fixed_now()).is_ok());
        assert_eq!(read(&target, "user/friendship.json"), friendship_json(42));

        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn unfinished_previous_import_blocks_a_new_import() {
        let source = temp_dir("incomplete-source");
        let target = temp_dir("incomplete-target");
        seed_source(&source);
        seed_target(&target);
        let file_name = place_exported_zip(&source, &target);
        write(&target, "migration-backups/20261001000000/.incomplete", "");
        write(
            &target,
            "migration-backups/20261001000000/news/a.md",
            "only copy",
        );

        let result = service(&target).import_at(&file_name, fixed_now());

        assert!(matches!(result, Err(AppError::ImportIncompletePrevious)));
        assert_eq!(
            read(&target, "config/settings.json"),
            r#"{"aiProvider":"gemini"}"#
        );
        assert_eq!(
            read(&target, "migration-backups/20261001000000/news/a.md"),
            "only copy"
        );
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn leftover_staging_is_replaced_and_removed() {
        let source = temp_dir("staging-source");
        let target = temp_dir("staging-target");
        seed_source(&source);
        let file_name = place_exported_zip(&source, &target);
        write(
            &target,
            ".migration-import-staging/config/settings.json",
            "stale",
        );

        service(&target).import_at(&file_name, fixed_now()).unwrap();

        assert_eq!(
            read(&target, "config/settings.json"),
            r#"{"aiProvider":"mock"}"#
        );
        assert!(!target.join(MIGRATION_IMPORT_STAGING_RELATIVE_DIR).exists());
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn import_and_export_share_one_lock() {
        let root = temp_dir("lock");
        let paths = AppPaths::new(root.clone());
        let export = DataExportService::new(&paths);
        let import = DataImportService::new(
            &paths,
            export.migration_lock(),
            MigrationWriteLocks::default(),
        );
        assert!(Arc::ptr_eq(
            &export.migration_lock(),
            &import.migration_lock
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn name_rules() {
        assert!(is_import_file_name("yuuko_transfer_tr_20261008140000.zip"));
        assert!(is_import_file_name(
            "yuuko_transfer_tr_20261008140000_2.zip"
        ));
        assert!(!is_import_file_name("yuuko_transfer_.zip"));
        assert!(!is_import_file_name("yuuko_transfer_a/b.zip"));
        assert!(!is_import_file_name("yuuko_transfer_..zip"));
        assert!(!is_import_file_name("YUUKO_TRANSFER_a.zip"));
        assert!(is_backup_dir_name("20261008150000"));
        assert!(is_backup_dir_name("20261008150000_2"));
        assert!(!is_backup_dir_name("20261008150000_"));
        assert!(!is_backup_dir_name("keep-me"));
    }

    #[cfg(windows)]
    #[test]
    fn junctions_in_imports_are_not_candidates() {
        let target = temp_dir("junction");
        let outside = temp_dir("junction-outside");
        std::fs::create_dir_all(target.join("imports")).unwrap();
        let link = target.join("imports").join("yuuko_transfer_link.zip");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "mklink /J failed");

        let service = service(&target);
        assert!(service.list_migration_imports().unwrap().is_empty());
        assert!(matches!(
            service.import_at("yuuko_transfer_link.zip", fixed_now()),
            Err(AppError::NotFound(_))
        ));
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(target).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_in_imports_are_not_candidates() {
        let source = temp_dir("symlink-source");
        let target = temp_dir("symlink-target");
        seed_source(&source);
        let file_name = place_exported_zip(&source, &target);
        let real = target.join("imports").join(&file_name);
        let outside = source.join("outside.zip");
        std::fs::rename(&real, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &real).unwrap();

        let service = service(&target);
        assert!(service.list_migration_imports().unwrap().is_empty());
        assert!(matches!(
            service.import_at(&file_name, fixed_now()),
            Err(AppError::NotFound(_))
        ));
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }
}
