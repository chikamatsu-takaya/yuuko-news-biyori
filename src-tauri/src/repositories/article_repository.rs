use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::article::{
    ArchiveRestoreStatus, ArchiveSummaryDto, ArchiveZipInfoDto, ArticleDetailDto,
    ArticleHistoryFilter, ArticleHistoryItemDto, ArticleReadState, ArticleSummaryDto,
    ArticleSummaryUpdate, FavoriteUpdateResult, FetchedArticle, RestoreArchivedArticleResult,
};
use crate::error::AppError;
use crate::paths::AppPaths;

const FRONT_MATTER_DELIMITER: &str = "---";
const ARCHIVE_INDEX_VERSION: u32 = 2;
const MAX_ARCHIVE_ZIP_SIZE: u64 = 128 * 1024 * 1024;
const MAX_ARCHIVE_ENTRY_SIZE: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ArticleRepository {
    article_favorites_path: PathBuf,
    article_news_dir: PathBuf,
    archive_dir: PathBuf,
    archive_index_path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

impl ArticleRepository {
    pub fn new(paths: &AppPaths) -> Self {
        let archive_dir = paths.app_data_dir.join("archive");
        Self {
            article_favorites_path: paths.article_favorites_path.clone(),
            article_news_dir: paths.article_news_dir.clone(),
            archive_index_path: archive_dir.join("archive_index.json"),
            archive_dir,
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    #[cfg(test)]
    fn with_paths(
        article_news_dir: PathBuf,
        article_favorites_path: PathBuf,
        archive_dir: PathBuf,
    ) -> Self {
        Self {
            article_favorites_path,
            article_news_dir,
            archive_index_path: archive_dir.join("archive_index.json"),
            archive_dir,
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.article_news_dir)?;
        if news_dir_contains_markdown_files(&self.article_news_dir)? {
            return Ok(());
        }

        for article in seed_articles() {
            self.save_article_record(&article)?;
        }

        Ok(())
    }

    pub fn list_recommended(&self, limit: usize) -> Result<Vec<ArticleSummaryDto>, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        let mut articles = self.load_article_records()?;
        articles.sort_by(compare_article_records);

        Ok(articles
            .into_iter()
            .map(|article| {
                article.to_summary_dto(is_effectively_favorite(&article, &favorite_store))
            })
            .take(limit)
            .collect())
    }

    pub fn list_history(
        &self,
        filter: ArticleHistoryFilter,
        limit: usize,
    ) -> Result<Vec<ArticleHistoryItemDto>, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        let active_records = self.load_article_records()?;
        let mut seen_ids = active_records
            .iter()
            .map(|article| article.article_id.clone())
            .collect::<HashSet<_>>();
        let mut articles = active_records
            .into_iter()
            .map(|article| {
                let is_favorite = is_effectively_favorite(&article, &favorite_store);
                article.to_history_item_dto(is_favorite)
            })
            .collect::<Vec<_>>();

        // 元Markdown削除後も履歴を軽量表示できるよう、ZIPを開かず記事カタログを統合する。
        // 同一IDのMarkdownが存在する間は、更新可能な実データ側を優先する。
        match self.load_archive_index_or_default() {
            Ok(archive_index) => articles.extend(
                archive_index
                    .archives
                    .iter()
                    .flat_map(|archive| archive.articles.iter())
                    .filter(|article| seen_ids.insert(article.article_id.clone()))
                    .map(|article| {
                        article.to_history_item_dto(favorite_store.contains(&article.article_id))
                    }),
            ),
            Err(error) => {
                // indexはZIPから再構築できる派生データなので、通常履歴までは停止させない。
                log::warn!("Failed to load archive index for article history: {error}");
            }
        }
        articles.sort_by(compare_history_items);

        Ok(articles
            .into_iter()
            .filter(|article| matches_history_item_filter(article, &filter))
            .take(limit)
            .collect())
    }

    /// アーカイブ退避候補（お気に入りでない・archivedでない・取得から約1か月以上経過）を
    /// 全件走査して返す（データ設計書 §14）。`list_history` と違い件数制限を設けない
    /// （古い記事ほど候補になるため、最近N件では取りこぼす）。実ZIP圧縮は後続。
    pub fn list_archive_candidates(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<ArticleHistoryItemDto>, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        let mut articles = self.load_article_records()?;
        // 退避は古い記事から処理するのが自然なため、fetched_at 昇順（古い順）で返す。
        // 履歴表示用の compare_history_records（新しい順）とは順序の意図が異なる。
        articles.sort_by(|left, right| {
            left.fetched_at
                .cmp(&right.fetched_at)
                .then_with(|| left.article_id.cmp(&right.article_id))
        });

        Ok(articles
            .into_iter()
            .filter_map(|article| {
                if article.archive_state == PersistedArchiveState::Restored {
                    return None;
                }
                let is_favorite = is_effectively_favorite(&article, &favorite_store);
                crate::domain::article::is_archive_candidate(
                    &article.fetched_at,
                    is_favorite,
                    article.is_archived,
                    now,
                )
                .then(|| article.to_history_item_dto(is_favorite))
            })
            .collect())
    }

    pub fn get_article_detail(&self, article_id: &str) -> Result<ArticleDetailDto, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        let article = self.find_article_record(article_id)?;
        Ok(article.to_detail_dto(is_effectively_favorite(&article, &favorite_store)))
    }

    /// 退避候補を月次ZIPへ圧縮し、`archive_index.json` を更新して archived 印を付ける（増分1・非破壊）。
    /// 元の記事Markdownは削除せず保持する（容量解放＝元.md削除は後続）。候補が無ければ何もしない。
    /// ZIP整合性検証に成功した月だけ archived 印を付ける（破損時は印を付けない＝安全側）。
    pub fn archive_candidates(&self, now: DateTime<Utc>) -> Result<ArchiveSummaryDto, AppError> {
        use std::collections::BTreeMap;

        let _write_guard = self.lock_writes()?;

        let favorite_store = self.load_favorite_store_or_default()?;
        let records = self.load_article_records()?;
        let archive_index = self.load_archive_index_or_default()?;

        // 新規候補と既存アーカイブ済み記事を月別に分ける。
        // 同じ月を再ZIP化する際、既存ZIP内の記事を落とさないため archived 済みも再同梱する。
        let mut by_month: BTreeMap<String, Vec<PersistedArticleRecord>> = BTreeMap::new();
        let mut archived_by_month: BTreeMap<String, Vec<PersistedArticleRecord>> = BTreeMap::new();
        for record in records {
            let bucket = record.month_bucket();
            if record.is_archived {
                archived_by_month.entry(bucket).or_default().push(record);
                continue;
            }
            if record.archive_state == PersistedArchiveState::Restored {
                continue;
            }

            let is_favorite = is_effectively_favorite(&record, &favorite_store);
            if crate::domain::article::is_archive_candidate(
                &record.fetched_at,
                is_favorite,
                false,
                now,
            ) {
                by_month.entry(bucket).or_default().push(record);
            }
        }

        let mut zip_files = Vec::new();
        let mut archived_article_count = 0usize;

        for (bucket, mut month_records) in by_month {
            month_records.sort_by(|left, right| left.article_id.cmp(&right.article_id));
            let mut zip_records = archived_by_month.remove(&bucket).unwrap_or_default();
            self.ensure_month_catalog_records_available(
                &format_month_label(&bucket),
                &archive_index,
                &zip_records,
            )?;
            zip_records.extend(month_records.iter().cloned());
            zip_records.sort_by(|left, right| left.article_id.cmp(&right.article_id));

            // 記事Markdownを再直列化してZIPエントリにする。
            let entries: Vec<crate::infra::archive_storage::ArchiveEntry> = zip_records
                .iter()
                .map(|record| {
                    let markdown = serialize_article_markdown(record)?;
                    Ok(crate::infra::archive_storage::ArchiveEntry {
                        name: archive_entry_name(&record.article_id)?,
                        contents: markdown.into_bytes(),
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            let article_count = entries.len();

            let month_label = format_month_label(&bucket);
            let file_name = format!("{month_label}.zip");
            let zip_path = self.archive_dir.join(&file_name);
            let zip_backup_path = self.prepare_archive_zip_backup(&zip_path)?;
            // ZIP作成＋再オープン検証（検証失敗時はここで中断し、archived 印は付けない）。
            let size_bytes =
                match crate::infra::archive_storage::write_verified_zip(&zip_path, &entries) {
                    Ok(size_bytes) => size_bytes,
                    Err(error) => {
                        self.restore_archive_zip_backup(&zip_path, zip_backup_path.as_deref());
                        return Err(error);
                    }
                };

            let zip_info = ArchiveZipInfoDto {
                month: month_label,
                file: file_name,
                article_count,
                size_bytes,
            };

            // 検証成功後に archived 印を付ける（非破壊：元.mdは残す）。
            let mut saved_originals = Vec::new();
            for mut record in month_records {
                let original = record.clone();
                record.archive_state = PersistedArchiveState::Archived;
                record.is_archived = true;
                record.status.archived = true;
                if let Err(error) = self.save_article_record(&record) {
                    self.rollback_archived_records(&saved_originals);
                    self.restore_archive_zip_backup(&zip_path, zip_backup_path.as_deref());
                    return Err(error);
                }
                saved_originals.push(original);
                archived_article_count += 1;
            }

            // 月ごとにindexへ反映しておく。後続月で失敗しても、成功済みZIPを孤立させないため。
            if let Err(error) = self.update_archive_index(now, &zip_info, &zip_records) {
                self.rollback_archived_records(&saved_originals);
                self.restore_archive_zip_backup(&zip_path, zip_backup_path.as_deref());
                return Err(error);
            }

            self.remove_archive_zip_backup(zip_backup_path.as_deref());
            zip_files.push(zip_info);
        }

        Ok(ArchiveSummaryDto {
            archived_article_count,
            zip_files,
        })
    }

    /// ZIPを直接読み戻せない段階では、既存ZIPの全記事Markdownが揃う月だけ再構築する。
    /// カタログ内記事が欠けた状態で上書きすると、ZIPにだけ残る記事を失うため安全側で停止する。
    fn ensure_month_catalog_records_available(
        &self,
        month: &str,
        archive_index: &ArchiveIndex,
        archived_records: &[PersistedArticleRecord],
    ) -> Result<(), AppError> {
        let Some(archive) = archive_index
            .archives
            .iter()
            .find(|archive| archive.month == month)
        else {
            return Ok(());
        };
        if !archive.catalog_complete {
            return Err(AppError::Archive(format!(
                "refusing to rebuild month with incomplete archive catalog: {month}"
            )));
        }

        let available_ids = archived_records
            .iter()
            .map(|record| record.article_id.as_str())
            .collect::<HashSet<_>>();
        let missing_ids = archive
            .articles
            .iter()
            .filter(|article| !available_ids.contains(article.article_id.as_str()))
            .map(|article| article.article_id.as_str())
            .collect::<Vec<_>>();
        if !missing_ids.is_empty() {
            return Err(AppError::Archive(format!(
                "refusing to rebuild {month}: archived markdown is missing for {}",
                missing_ids.join(", ")
            )));
        }
        Ok(())
    }

    fn load_archive_index_or_default(&self) -> Result<ArchiveIndex, AppError> {
        if !self.archive_index_path.exists() {
            return Ok(ArchiveIndex::default());
        }
        let raw = std::fs::read_to_string(&self.archive_index_path)?;
        let index = serde_json::from_str::<ArchiveIndex>(crate::util::strip_utf8_bom(&raw))?;
        index.validate()?;
        Ok(index)
    }

    fn update_archive_index(
        &self,
        now: DateTime<Utc>,
        zip_info: &ArchiveZipInfoDto,
        records: &[PersistedArticleRecord],
    ) -> Result<(), AppError> {
        let mut index = self.load_archive_index_or_default()?;
        index.version = ARCHIVE_INDEX_VERSION;
        let created_at = format_archive_timestamp(now);
        let articles = records
            .iter()
            .map(ArchiveArticleIndexEntry::from_record)
            .collect::<Result<Vec<_>, AppError>>()?;

        // 同じ月のエントリは置き換える（再実行時の重複防止）。
        index.archives.retain(|entry| entry.month != zip_info.month);
        index.archives.push(ArchiveIndexEntry {
            month: zip_info.month.clone(),
            file: zip_info.file.clone(),
            article_count: zip_info.article_count,
            created_at,
            size_bytes: zip_info.size_bytes,
            catalog_complete: true,
            articles,
        });
        index
            .archives
            .sort_by(|left, right| left.month.cmp(&right.month));
        if let Some(parent) = self.archive_index_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_vec_pretty(&index)?;
        atomic_write(&self.archive_index_path, &payload, "archive index")
    }

    fn rollback_archived_records(&self, originals: &[PersistedArticleRecord]) {
        for record in originals {
            if let Err(error) = self.save_article_record(record) {
                log::error!(
                    "Failed to rollback archived article state for {}: {error}",
                    record.article_id
                );
            }
        }
    }

    fn prepare_archive_zip_backup(&self, zip_path: &Path) -> Result<Option<PathBuf>, AppError> {
        if !zip_path.exists() {
            return Ok(None);
        }

        let backup_path = zip_path.with_extension("zip.rollback");
        if backup_path.exists() {
            std::fs::remove_file(&backup_path)?;
        }
        std::fs::rename(zip_path, &backup_path)?;
        Ok(Some(backup_path))
    }

    fn restore_archive_zip_backup(&self, zip_path: &Path, backup_path: Option<&Path>) {
        if zip_path.exists() {
            let _ = std::fs::remove_file(zip_path);
        }

        if let Some(backup_path) = backup_path {
            if backup_path.exists() {
                if let Err(error) = std::fs::rename(backup_path, zip_path) {
                    log::error!("Failed to restore previous archive zip: {error}");
                }
            }
        }
    }

    fn remove_archive_zip_backup(&self, backup_path: Option<&Path>) {
        if let Some(backup_path) = backup_path {
            if backup_path.exists() {
                if let Err(error) = std::fs::remove_file(backup_path) {
                    log::warn!("Failed to remove archive zip rollback backup: {error}");
                }
            }
        }
    }

    pub fn update_article_favorite(
        &self,
        article_id: &str,
        is_favorite: bool,
    ) -> Result<FavoriteUpdateResult, AppError> {
        let _write_guard = self.lock_writes()?;
        let mut article = self.find_article_record(article_id)?;
        article.favorite = is_favorite;
        self.save_article_record(&article)?;

        let mut favorite_store = self.load_favorite_store_or_default()?;
        favorite_store.set(article_id, is_favorite);
        if let Err(error) = self.save_favorite_store(&favorite_store) {
            log::warn!("Failed to persist favorite override JSON: {error}");
        }

        Ok(FavoriteUpdateResult {
            article_id: article_id.to_string(),
            is_favorite,
        })
    }

    /// 生成済み要約を記事Markdownへ永続化する（B-4）。
    /// 既存記事をロードして要約系フィールド＋メタ（summarized / summary_generated_at / ai_provider）を
    /// 更新し Markdownへ保存する。再表示は get_article_detail がこの保存値を返す（=キャッシュ）。
    pub fn update_article_summary(
        &self,
        article_id: &str,
        update: ArticleSummaryUpdate,
    ) -> Result<(), AppError> {
        let _write_guard = self.lock_writes()?;
        let mut article = self.find_article_record(article_id)?;
        article.summary = Some(update.summary);
        article.yuuko_explanation = Some(update.yuuko_explanation);
        article.focus_points = update.focus_points;
        article.yuuko_comment = Some(update.yuuko_comment);
        article.status.summarized = true;
        article.summary_generated_at = Some(update.generated_at);
        article.ai_provider = Some(update.ai_provider);
        self.save_article_record(&article)?;
        Ok(())
    }

    /// 既存記事の article_id 集合を返す（取得時の重複排除に使う）。
    pub fn existing_article_ids(&self) -> Result<HashSet<String>, AppError> {
        let mut article_ids = self
            .load_article_records()?
            .into_iter()
            .map(|article| article.article_id)
            .collect::<HashSet<_>>();
        article_ids.extend(
            self.load_archive_index_or_default()?
                .archives
                .into_iter()
                .flat_map(|archive| archive.articles)
                .map(|article| article.article_id),
        );
        Ok(article_ids)
    }

    /// 取得済みの新規記事を保存する（内部Rust API・Tauri commandとして公開しない）。
    /// 既存 article_id はスキップして重複排除し、新規保存できた件数を返す。
    pub fn save_fetched_articles(&self, articles: Vec<FetchedArticle>) -> Result<usize, AppError> {
        let _write_guard = self.lock_writes()?;
        let mut existing = self.existing_article_ids()?;
        let mut saved = 0usize;

        for article in articles {
            // 同一 article_id は新規保存しない（既存記事の上書きを避ける）。
            if !existing.insert(article.article_id.clone()) {
                continue;
            }
            let record = PersistedArticleRecord::from_fetched(article);
            self.save_article_record(&record)?;
            saved += 1;
        }

        Ok(saved)
    }

    /// 記事IDから月次ZIP内の1記事だけを検証・復元する。任意パスは受け取らない。
    pub fn restore_archived_article(
        &self,
        article_id: &str,
    ) -> Result<RestoreArchivedArticleResult, AppError> {
        let _write_guard = self.lock_writes()?;

        if let Some(mut article) = self.find_article_record_optional(article_id)? {
            if article.archive_state == PersistedArchiveState::Archived {
                article.archive_state = PersistedArchiveState::Restored;
                article.is_archived = false;
                article.status.archived = false;
                self.save_article_record(&article)?;
                return Ok(RestoreArchivedArticleResult {
                    article_id: article_id.to_string(),
                    status: ArchiveRestoreStatus::Restored,
                });
            }
            return Ok(RestoreArchivedArticleResult {
                article_id: article_id.to_string(),
                status: ArchiveRestoreStatus::AlreadyAvailable,
            });
        }

        let index = self.load_archive_index_or_default()?;
        let (archive, catalog_article) = index.find_article(article_id)?;
        validate_archive_location(archive)?;
        let zip_path = self.archive_dir.join(&archive.file);
        let bytes = crate::infra::archive_storage::read_verified_entry(
            &zip_path,
            &catalog_article.entry_name,
            MAX_ARCHIVE_ZIP_SIZE,
            MAX_ARCHIVE_ENTRY_SIZE,
        )?;
        let raw = String::from_utf8(bytes)
            .map_err(|_| AppError::Archive("archive article is not valid UTF-8".to_string()))?;
        let mut article = parse_article_record(&raw, "archive article")?;
        if article.article_id != article_id
            || format_month_label(&article.month_bucket()) != archive.month
        {
            return Err(AppError::Archive(
                "archive article metadata does not match the index".to_string(),
            ));
        }

        article.archive_state = PersistedArchiveState::Restored;
        article.is_archived = false;
        article.status.archived = false;
        self.save_article_record(&article)?;
        Ok(RestoreArchivedArticleResult {
            article_id: article_id.to_string(),
            status: ArchiveRestoreStatus::Restored,
        })
    }

    fn lock_writes(&self) -> Result<MutexGuard<'_, ()>, AppError> {
        self.write_lock
            .lock()
            .map_err(|_| AppError::Archive("article write lock is poisoned".to_string()))
    }

    fn find_article_record(&self, article_id: &str) -> Result<PersistedArticleRecord, AppError> {
        self.find_article_record_optional(article_id)?
            .ok_or_else(|| AppError::NotFound(format!("article not found: {article_id}")))
    }

    fn find_article_record_optional(
        &self,
        article_id: &str,
    ) -> Result<Option<PersistedArticleRecord>, AppError> {
        Ok(self
            .load_article_records()?
            .into_iter()
            .find(|article| article.article_id == article_id))
    }

    fn load_article_records(&self) -> Result<Vec<PersistedArticleRecord>, AppError> {
        let mut markdown_paths = Vec::new();
        collect_markdown_files(&self.article_news_dir, &mut markdown_paths)?;
        markdown_paths.sort();

        markdown_paths
            .into_iter()
            .map(|path| self.load_article_record(&path))
            .collect()
    }

    fn load_article_record(&self, path: &Path) -> Result<PersistedArticleRecord, AppError> {
        let raw = std::fs::read_to_string(path)?;
        parse_article_record(&raw, &path.display().to_string())
    }

    fn save_article_record(&self, article: &PersistedArticleRecord) -> Result<(), AppError> {
        let path = self.article_path(article);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let payload = serialize_article_markdown(article)?;
        atomic_write(&path, payload.as_bytes(), "article markdown")
    }

    fn article_path(&self, article: &PersistedArticleRecord) -> PathBuf {
        self.article_news_dir
            .join(article.month_bucket())
            .join(format!("{}.md", article.article_id))
    }

    fn load_favorite_store_or_default(&self) -> Result<ArticleFavoriteStore, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.article_favorites_path.exists() {
            return Ok(ArticleFavoriteStore::default());
        }

        let raw = std::fs::read_to_string(&self.article_favorites_path)?;
        let mut store = serde_json::from_str::<ArticleFavoriteStore>(&raw)?;
        if store.version == 0 {
            store.version = 1;
        }
        Ok(store)
    }

    fn save_favorite_store(&self, store: &ArticleFavoriteStore) -> Result<(), AppError> {
        if let Some(parent) = self.article_favorites_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let payload = serde_json::to_vec_pretty(store)?;
        atomic_write(&self.article_favorites_path, &payload, "article favorites")
    }

    fn restore_backup_if_primary_missing(&self) {
        if self.article_favorites_path.exists() {
            return;
        }

        let backup_path = self.article_favorites_path.with_extension("json.bak");
        if !backup_path.exists() {
            return;
        }

        log::warn!("article favorites file is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.article_favorites_path) {
            log::error!("Failed to restore article favorites backup: {error}");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct ArticleFavoriteStore {
    version: u32,
    favorite_article_ids: Vec<String>,
}

impl Default for ArticleFavoriteStore {
    fn default() -> Self {
        Self {
            version: 1,
            favorite_article_ids: Vec::new(),
        }
    }
}

impl ArticleFavoriteStore {
    fn contains(&self, article_id: &str) -> bool {
        self.favorite_article_ids
            .iter()
            .any(|favorite_article_id| favorite_article_id == article_id)
    }

    fn set(&mut self, article_id: &str, is_favorite: bool) {
        if is_favorite {
            if !self.contains(article_id) {
                self.favorite_article_ids.push(article_id.to_string());
            }
            return;
        }

        self.favorite_article_ids
            .retain(|favorite_article_id| favorite_article_id != article_id);
    }
}

#[derive(Debug, Clone)]
struct PersistedArticleRecord {
    version: u32,
    article_id: String,
    title: String,
    source_name: String,
    source_key: String,
    original_url: String,
    fetched_at: String,
    published_at_text: String,
    genre: String,
    tags: Vec<String>,
    status: PersistedArticleStatus,
    read_state: ArticleReadState,
    favorite: bool,
    archive_state: PersistedArchiveState,
    is_archived: bool,
    recommendation_score: f32,
    summary_generated_at: Option<String>,
    ai_provider: Option<String>,
    content_hash: Option<String>,
    excerpt: Option<String>,
    summary: Option<String>,
    yuuko_explanation: Option<String>,
    focus_points: Vec<String>,
    yuuko_comment: Option<String>,
    keyword_candidates: Vec<String>,
}

impl PersistedArticleRecord {
    fn from_parts(
        front_matter: PersistedArticleFrontMatter,
        sections: ArticleBodySections,
    ) -> Result<Self, AppError> {
        let source_key = if front_matter.source_key.trim().is_empty() {
            build_source_key(&front_matter.source_name)
        } else {
            front_matter.source_key
        };

        let published_at_text = front_matter
            .published_at
            .clone()
            .unwrap_or_else(|| front_matter.fetched_at.clone());
        let declared_archive_state = front_matter
            .archive_state
            .unwrap_or(PersistedArchiveState::Active);
        let archive_state = match front_matter.archived {
            Some(true) => PersistedArchiveState::Archived,
            Some(false) if declared_archive_state == PersistedArchiveState::Restored => {
                PersistedArchiveState::Restored
            }
            Some(false) => PersistedArchiveState::Active,
            None => declared_archive_state,
        };

        Ok(Self {
            version: normalize_version(front_matter.version),
            article_id: required_field(front_matter.article_id, "articleId")?,
            title: required_field(front_matter.title, "title")?,
            source_name: required_field(front_matter.source_name, "sourceName")?,
            source_key,
            original_url: required_field(front_matter.url, "url")?,
            fetched_at: required_field(front_matter.fetched_at, "fetchedAt")?,
            published_at_text,
            genre: required_field(front_matter.genre, "genre")?,
            tags: front_matter.tags,
            status: front_matter.status,
            read_state: front_matter.read_state,
            favorite: front_matter.favorite,
            archive_state,
            is_archived: archive_state == PersistedArchiveState::Archived,
            recommendation_score: front_matter.recommendation_score,
            summary_generated_at: front_matter.summary_generated_at,
            ai_provider: front_matter.ai_provider,
            content_hash: front_matter.content_hash,
            excerpt: sections.excerpt,
            summary: sections.summary,
            yuuko_explanation: sections.yuuko_explanation,
            focus_points: sections.focus_points,
            yuuko_comment: sections.yuuko_comment,
            keyword_candidates: sections.keyword_candidates,
        })
    }

    /// 取得パイプラインの `FetchedArticle` から永続化レコードを構築する。
    /// AI要約前の段階なので summary 系は空、status は取得・抽出済みを反映する。
    fn from_fetched(article: FetchedArticle) -> Self {
        let source_key = build_source_key(&article.source_name);
        let html_extracted = article.excerpt.is_some();
        Self {
            version: 1,
            article_id: article.article_id,
            title: article.title,
            source_name: article.source_name,
            source_key,
            original_url: article.original_url,
            fetched_at: article.fetched_at,
            published_at_text: article.published_at_text,
            genre: article.genre,
            tags: article.tags,
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted,
                markdown_generated: true,
                summarized: false,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: article.read_state,
            favorite: false,
            archive_state: PersistedArchiveState::Active,
            is_archived: false,
            recommendation_score: article.recommendation_score,
            summary_generated_at: None,
            ai_provider: None,
            content_hash: None,
            excerpt: article.excerpt,
            summary: None,
            yuuko_explanation: None,
            focus_points: Vec::new(),
            yuuko_comment: None,
            keyword_candidates: Vec::new(),
        }
    }

    fn to_summary_dto(&self, is_favorite: bool) -> ArticleSummaryDto {
        ArticleSummaryDto {
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            published_at_text: self.published_at_text.clone(),
            genre: self.genre.clone(),
            summary: self.summary.clone().or_else(|| self.excerpt.clone()),
            is_favorite,
            read_state: self.read_state.clone(),
            recommendation_score: self.recommendation_score,
        }
    }

    fn to_history_item_dto(&self, is_favorite: bool) -> ArticleHistoryItemDto {
        ArticleHistoryItemDto {
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            published_at_text: self.published_at_text.clone(),
            fetched_at: self.fetched_at.clone(),
            genre: self.genre.clone(),
            summary: self.summary.clone().or_else(|| self.excerpt.clone()),
            is_favorite,
            read_state: self.read_state.clone(),
            is_archived: self.is_archived,
            recommendation_score: self.recommendation_score,
        }
    }

    fn to_detail_dto(&self, is_favorite: bool) -> ArticleDetailDto {
        ArticleDetailDto {
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            original_url: self.original_url.clone(),
            published_at_text: self.published_at_text.clone(),
            genre: self.genre.clone(),
            summary: self.summary.clone().or_else(|| self.excerpt.clone()),
            excerpt: self.excerpt.clone(),
            yuuko_explanation: self.yuuko_explanation.clone(),
            focus_points: self.focus_points.clone(),
            yuuko_comment: self.yuuko_comment.clone(),
            is_favorite,
            keyword_candidates: self.keyword_candidates.clone(),
        }
    }

    fn to_front_matter(&self) -> PersistedArticleFrontMatter {
        PersistedArticleFrontMatter {
            version: self.version,
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            source_key: self.source_key.clone(),
            url: self.original_url.clone(),
            fetched_at: self.fetched_at.clone(),
            published_at: Some(self.published_at_text.clone()),
            genre: self.genre.clone(),
            tags: self.tags.clone(),
            status: self.status.clone(),
            read_state: self.read_state.clone(),
            favorite: self.favorite,
            archive_state: Some(self.archive_state),
            archived: Some(self.is_archived),
            recommendation_score: self.recommendation_score,
            summary_generated_at: self.summary_generated_at.clone(),
            ai_provider: self.ai_provider.clone(),
            content_hash: self.content_hash.clone(),
        }
    }

    fn month_bucket(&self) -> String {
        month_bucket_from_text(&self.published_at_text)
            .or_else(|| month_bucket_from_text(&self.fetched_at))
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedArticleFrontMatter {
    #[serde(default = "default_version")]
    version: u32,
    article_id: String,
    title: String,
    source_name: String,
    #[serde(default)]
    source_key: String,
    url: String,
    fetched_at: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default, alias = "category")]
    genre: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    status: PersistedArticleStatus,
    #[serde(default = "default_read_state")]
    read_state: ArticleReadState,
    #[serde(default)]
    favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archive_state: Option<PersistedArchiveState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archived: Option<bool>,
    #[serde(default)]
    recommendation_score: f32,
    #[serde(default)]
    summary_generated_at: Option<String>,
    #[serde(default)]
    ai_provider: Option<String>,
    #[serde(default)]
    content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedArticleStatus {
    fetched: bool,
    html_extracted: bool,
    markdown_generated: bool,
    summarized: bool,
    recommended: bool,
    introduced_by_yuuko: bool,
    archived: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PersistedArchiveState {
    Active,
    Archived,
    Restored,
}

#[derive(Debug, Default)]
struct ArticleBodySections {
    excerpt: Option<String>,
    summary: Option<String>,
    yuuko_explanation: Option<String>,
    focus_points: Vec<String>,
    yuuko_comment: Option<String>,
    keyword_candidates: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum ArticleBodySection {
    Excerpt,
    Summary,
    Explanation,
    FocusPoints,
    Comment,
    Keywords,
}

impl ArticleBodySection {
    fn from_heading(line: &str) -> Option<Self> {
        match line.trim() {
            "## 本文抜粋" => Some(Self::Excerpt),
            "## AI要約" => Some(Self::Summary),
            "## ゆうこの用語解説" => Some(Self::Explanation),
            "## 注目ポイント" => Some(Self::FocusPoints),
            "## ゆうこの一言" => Some(Self::Comment),
            "## 解説対象キーワード" => Some(Self::Keywords),
            _ => None,
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Excerpt => "本文抜粋",
            Self::Summary => "AI要約",
            Self::Explanation => "ゆうこの用語解説",
            Self::FocusPoints => "注目ポイント",
            Self::Comment => "ゆうこの一言",
            Self::Keywords => "解説対象キーワード",
        }
    }
}

fn news_dir_contains_markdown_files(dir: &Path) -> Result<bool, AppError> {
    if !dir.exists() {
        return Ok(false);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if news_dir_contains_markdown_files(&path)? {
                return Ok(true);
            }
            continue;
        }

        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("md"))
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), AppError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
            continue;
        }

        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("md"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn parse_article_record(raw: &str, source_label: &str) -> Result<PersistedArticleRecord, AppError> {
    let (front_matter_raw, body_raw) = split_front_matter(raw, source_label)?;
    let front_matter: PersistedArticleFrontMatter = serde_yaml::from_str(&front_matter_raw)
        .map_err(|error| {
            AppError::Parse(format!(
                "failed to parse article front matter '{source_label}': {error}"
            ))
        })?;
    let sections = parse_article_body(&body_raw);
    PersistedArticleRecord::from_parts(front_matter, sections)
}

fn split_front_matter(raw: &str, source_label: &str) -> Result<(String, String), AppError> {
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some(FRONT_MATTER_DELIMITER) {
        return Err(AppError::Parse(format!(
            "article markdown '{source_label}' is missing YAML front matter"
        )));
    }

    let mut front_matter_lines = Vec::new();
    let mut found_closing_delimiter = false;
    for line in &mut lines {
        if line.trim() == FRONT_MATTER_DELIMITER {
            found_closing_delimiter = true;
            break;
        }
        front_matter_lines.push(line);
    }

    if !found_closing_delimiter {
        return Err(AppError::Parse(format!(
            "article markdown '{source_label}' has an unterminated YAML front matter block"
        )));
    }

    Ok((
        front_matter_lines.join("\n"),
        lines.collect::<Vec<_>>().join("\n"),
    ))
}

fn parse_article_body(body: &str) -> ArticleBodySections {
    let mut sections = ArticleBodySections::default();
    let mut current_section = None;
    let mut current_lines = Vec::new();

    for line in body.lines() {
        if let Some(next_section) = ArticleBodySection::from_heading(line) {
            flush_body_section(&mut sections, current_section, &current_lines);
            current_section = Some(next_section);
            current_lines.clear();
            continue;
        }

        if current_section.is_some() {
            current_lines.push(line.to_string());
        }
    }

    flush_body_section(&mut sections, current_section, &current_lines);
    sections
}

fn flush_body_section(
    sections: &mut ArticleBodySections,
    section: Option<ArticleBodySection>,
    lines: &[String],
) {
    let Some(section) = section else {
        return;
    };

    match section {
        ArticleBodySection::Excerpt => {
            sections.excerpt = normalize_text_block(lines);
        }
        ArticleBodySection::Summary => {
            sections.summary = normalize_text_block(lines);
        }
        ArticleBodySection::Explanation => {
            sections.yuuko_explanation = normalize_text_block(lines);
        }
        ArticleBodySection::Comment => {
            sections.yuuko_comment = normalize_text_block(lines);
        }
        ArticleBodySection::FocusPoints => {
            sections.focus_points = normalize_list_block(lines);
        }
        ArticleBodySection::Keywords => {
            sections.keyword_candidates = normalize_list_block(lines);
        }
    }
}

fn normalize_text_block(lines: &[String]) -> Option<String> {
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn normalize_list_block(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            let value = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .unwrap_or(trimmed)
                .trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        })
        .collect()
}

fn serialize_article_markdown(article: &PersistedArticleRecord) -> Result<String, AppError> {
    let mut front_matter = serde_yaml::to_string(&article.to_front_matter()).map_err(|error| {
        AppError::Parse(format!("failed to serialize article front matter: {error}"))
    })?;
    if let Some(stripped) = front_matter.strip_prefix("---\n") {
        front_matter = stripped.to_string();
    }

    let body = compose_article_body(article);
    Ok(format!(
        "{FRONT_MATTER_DELIMITER}\n{front_matter}{FRONT_MATTER_DELIMITER}\n\n{body}"
    ))
}

fn compose_article_body(article: &PersistedArticleRecord) -> String {
    let mut sections = Vec::new();

    push_text_section(
        &mut sections,
        ArticleBodySection::Excerpt,
        article.excerpt.as_deref(),
    );
    push_text_section(
        &mut sections,
        ArticleBodySection::Summary,
        article.summary.as_deref(),
    );
    push_text_section(
        &mut sections,
        ArticleBodySection::Explanation,
        article.yuuko_explanation.as_deref(),
    );
    push_list_section(
        &mut sections,
        ArticleBodySection::FocusPoints,
        &article.focus_points,
    );
    push_text_section(
        &mut sections,
        ArticleBodySection::Comment,
        article.yuuko_comment.as_deref(),
    );
    push_list_section(
        &mut sections,
        ArticleBodySection::Keywords,
        &article.keyword_candidates,
    );

    sections.join("\n\n")
}

fn push_text_section(buffer: &mut Vec<String>, section: ArticleBodySection, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    buffer.push(format!("## {}\n{}", section.heading(), value));
}

fn push_list_section(buffer: &mut Vec<String>, section: ArticleBodySection, values: &[String]) {
    if values.is_empty() {
        return;
    }

    let items = values
        .iter()
        .map(|value| format!("- {}", value.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    buffer.push(format!("## {}\n{}", section.heading(), items));
}

fn compare_article_records(
    left: &PersistedArticleRecord,
    right: &PersistedArticleRecord,
) -> Ordering {
    right
        .recommendation_score
        .partial_cmp(&left.recommendation_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| right.fetched_at.cmp(&left.fetched_at))
        .then_with(|| left.article_id.cmp(&right.article_id))
}

fn compare_history_items(left: &ArticleHistoryItemDto, right: &ArticleHistoryItemDto) -> Ordering {
    right
        .fetched_at
        .cmp(&left.fetched_at)
        .then_with(|| left.article_id.cmp(&right.article_id))
}

fn matches_history_item_filter(
    article: &ArticleHistoryItemDto,
    filter: &ArticleHistoryFilter,
) -> bool {
    match filter {
        ArticleHistoryFilter::All => true,
        ArticleHistoryFilter::Unread => article.read_state == ArticleReadState::Unread,
        // Previewed は軽量プレビューを見た状態なので、履歴UIでは既読側へまとめる。
        ArticleHistoryFilter::Read => article.read_state != ArticleReadState::Unread,
        ArticleHistoryFilter::Favorite => article.is_favorite,
        ArticleHistoryFilter::Archived => article.is_archived,
    }
}

fn is_effectively_favorite(
    article: &PersistedArticleRecord,
    favorite_store: &ArticleFavoriteStore,
) -> bool {
    article.favorite || favorite_store.contains(&article.article_id)
}

fn archive_entry_name(article_id: &str) -> Result<String, AppError> {
    if article_id.is_empty()
        || !article_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(AppError::Archive(format!(
            "unsafe article id for archive entry: {article_id}"
        )));
    }

    Ok(format!("{article_id}.md"))
}

/// 月バケット "YYYYMM" を表示用ラベル "YYYY-MM" へ変換する（アーカイブZIP名・index 用）。
/// 6桁数字でなければそのまま返す（"unknown" 等）。
fn format_month_label(bucket: &str) -> String {
    if bucket.len() == 6 && bucket.chars().all(|ch| ch.is_ascii_digit()) {
        format!("{}-{}", &bucket[0..4], &bucket[4..6])
    } else {
        bucket.to_string()
    }
}

/// アーカイブ index の createdAt 用タイムスタンプ（UTC・他サービスと同形式）。
fn format_archive_timestamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `archive/archive_index.json` の構造（データ設計書 §14.4）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveIndex {
    #[serde(default = "legacy_archive_index_version")]
    version: u32,
    #[serde(default)]
    archives: Vec<ArchiveIndexEntry>,
}

impl Default for ArchiveIndex {
    fn default() -> Self {
        Self {
            version: ARCHIVE_INDEX_VERSION,
            archives: Vec::new(),
        }
    }
}

impl ArchiveIndex {
    fn validate(&self) -> Result<(), AppError> {
        if !(1..=ARCHIVE_INDEX_VERSION).contains(&self.version) {
            return Err(AppError::Archive(format!(
                "unsupported archive index version: {}",
                self.version
            )));
        }

        let mut article_ids = HashSet::new();
        for archive in &self.archives {
            if !archive.catalog_complete && !archive.articles.is_empty() {
                return Err(AppError::Archive(format!(
                    "incomplete archive catalog contains article metadata for {}",
                    archive.month
                )));
            }
            if archive.catalog_complete && archive.article_count != archive.articles.len() {
                return Err(AppError::Archive(format!(
                    "archive index article count mismatch for {}",
                    archive.month
                )));
            }
            for article in &archive.articles {
                let expected_entry_name = archive_entry_name(&article.article_id)?;
                if article.entry_name != expected_entry_name {
                    return Err(AppError::Archive(format!(
                        "archive entry name mismatch for {}",
                        article.article_id
                    )));
                }
                if !article_ids.insert(article.article_id.as_str()) {
                    return Err(AppError::Archive(format!(
                        "duplicate article id in archive index: {}",
                        article.article_id
                    )));
                }
            }
        }
        Ok(())
    }

    fn find_article(
        &self,
        article_id: &str,
    ) -> Result<(&ArchiveIndexEntry, &ArchiveArticleIndexEntry), AppError> {
        for archive in &self.archives {
            if let Some(article) = archive
                .articles
                .iter()
                .find(|article| article.article_id == article_id)
            {
                if !archive.catalog_complete {
                    return Err(AppError::Archive(
                        "archive article catalog is incomplete".to_string(),
                    ));
                }
                return Ok((archive, article));
            }
        }
        Err(AppError::NotFound(format!(
            "archived article not found: {article_id}"
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveIndexEntry {
    month: String,
    file: String,
    article_count: usize,
    created_at: String,
    size_bytes: u64,
    /// v1から未移行の月を識別し、不完全なカタログを削除判断に使わないための印。
    #[serde(default)]
    catalog_complete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    articles: Vec<ArchiveArticleIndexEntry>,
}

/// 履歴表示と重複取得防止に必要な最小メタデータ。本文はZIP内Markdownだけに保持する。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveArticleIndexEntry {
    article_id: String,
    entry_name: String,
    title: String,
    source_name: String,
    published_at_text: String,
    fetched_at: String,
    genre: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    read_state: ArticleReadState,
    recommendation_score: f32,
}

impl ArchiveArticleIndexEntry {
    fn from_record(record: &PersistedArticleRecord) -> Result<Self, AppError> {
        Ok(Self {
            article_id: record.article_id.clone(),
            entry_name: archive_entry_name(&record.article_id)?,
            title: record.title.clone(),
            source_name: record.source_name.clone(),
            published_at_text: record.published_at_text.clone(),
            fetched_at: record.fetched_at.clone(),
            genre: record.genre.clone(),
            summary: record.summary.clone().or_else(|| record.excerpt.clone()),
            read_state: record.read_state.clone(),
            recommendation_score: record.recommendation_score,
        })
    }

    fn to_history_item_dto(&self, is_favorite: bool) -> ArticleHistoryItemDto {
        ArticleHistoryItemDto {
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            published_at_text: self.published_at_text.clone(),
            fetched_at: self.fetched_at.clone(),
            genre: self.genre.clone(),
            summary: self.summary.clone(),
            is_favorite,
            read_state: self.read_state.clone(),
            is_archived: true,
            recommendation_score: self.recommendation_score,
        }
    }
}

fn legacy_archive_index_version() -> u32 {
    1
}

fn validate_archive_location(archive: &ArchiveIndexEntry) -> Result<(), AppError> {
    let bytes = archive.month.as_bytes();
    let month_number = archive
        .month
        .get(5..7)
        .and_then(|value| value.parse::<u8>().ok());
    let valid_month = bytes.len() == 7
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && month_number.is_some_and(|value| (1..=12).contains(&value));
    if !valid_month || archive.file != format!("{}.zip", archive.month) {
        return Err(AppError::Archive(
            "archive index contains an unsafe archive location".to_string(),
        ));
    }
    Ok(())
}

fn month_bucket_from_text(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(6)
        .collect::<String>();
    if digits.len() == 6 {
        Some(digits)
    } else {
        None
    }
}

fn build_source_key(source_name: &str) -> String {
    let mut key = String::new();
    let mut last_was_separator = false;

    for ch in source_name.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_lowercase());
            last_was_separator = false;
            continue;
        }

        if !last_was_separator && !key.is_empty() {
            key.push('_');
            last_was_separator = true;
        }
    }

    key.trim_matches('_')
        .to_string()
        .chars()
        .take(32)
        .collect::<String>()
}

fn required_field(value: String, field_name: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Parse(format!(
            "article front matter field '{field_name}' must not be empty"
        )));
    }

    Ok(trimmed.to_string())
}

fn normalize_version(version: u32) -> u32 {
    if version == 0 {
        1
    } else {
        version
    }
}

fn default_version() -> u32 {
    1
}

fn default_read_state() -> ArticleReadState {
    ArticleReadState::Unread
}

fn atomic_write(path: &Path, payload: &[u8], label: &str) -> Result<(), AppError> {
    let temp_path = with_extension_suffix(path, "tmp");
    let backup_path = with_extension_suffix(path, "bak");
    std::fs::write(&temp_path, payload)?;

    let had_existing = path.exists();
    if had_existing {
        if backup_path.exists() {
            std::fs::remove_file(&backup_path)?;
        }
        std::fs::rename(path, &backup_path)?;
    }

    match std::fs::rename(&temp_path, path) {
        Ok(()) => {
            if had_existing && backup_path.exists() {
                if let Err(error) = std::fs::remove_file(&backup_path) {
                    log::warn!("Failed to remove {label} backup: {error}");
                }
            }
            Ok(())
        }
        Err(error) => {
            log::error!("Failed to promote temporary {label} file: {error}");

            if had_existing && backup_path.exists() {
                if let Err(restore_error) = std::fs::rename(&backup_path, path) {
                    log::error!("Failed to restore {label} backup: {restore_error}");
                }
            }

            if temp_path.exists() {
                let _ = std::fs::remove_file(&temp_path);
            }

            Err(error.into())
        }
    }
}

fn with_extension_suffix(path: &Path, suffix: &str) -> PathBuf {
    let base_extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.{suffix}"))
        .unwrap_or_else(|| suffix.to_string());
    path.with_extension(base_extension)
}

fn seed_articles() -> Vec<PersistedArticleRecord> {
    vec![
        PersistedArticleRecord {
            version: 1,
            article_id: "article-001".to_string(),
            title: "生成AIスタートアップの資金調達が活発化".to_string(),
            source_name: "TechCrunch Japan".to_string(),
            source_key: "techcrunch_japan".to_string(),
            original_url: "https://example.com/articles/article-001".to_string(),
            fetched_at: "2026-06-04T09:10:00+09:00".to_string(),
            published_at_text: "5分前".to_string(),
            genre: "AI・テクノロジー".to_string(),
            tags: vec!["AI".to_string(), "生成AI".to_string(), "資金調達".to_string()],
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted: true,
                markdown_generated: true,
                summarized: true,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: ArticleReadState::Unread,
            favorite: false,
            archive_state: PersistedArchiveState::Active,
            is_archived: false,
            recommendation_score: 0.92,
            summary_generated_at: Some("2026-06-04T09:12:00+09:00".to_string()),
            ai_provider: Some("mock".to_string()),
            content_hash: Some("content-hash-001".to_string()),
            excerpt: Some(
                "複数の生成AIスタートアップが国内外で大型の資金調達を発表し、企業向け活用の広がりが改めて注目されています。".to_string(),
            ),
            summary: Some(
                "生成AIを活用するスタートアップへの投資が再び活発になっており、法人向け導入支援や運用最適化の分野に資金が集まっています。".to_string(),
            ),
            yuuko_explanation: Some(
                "この記事では、生成AIそのものよりも、それをどう実務に組み込むかを支える企業に期待が集まっている点が大切です。".to_string(),
            ),
            focus_points: vec![
                "投資対象がモデル開発だけでなく運用支援まで広がっている".to_string(),
                "法人導入の具体策を持つ企業が評価されやすい".to_string(),
                "生成AIの実装コストを下げるサービスが増えている".to_string(),
            ],
            yuuko_comment: Some(
                "技術そのものより、使いこなす仕組みに注目が移ってきたのが面白い流れですね。".to_string(),
            ),
            keyword_candidates: vec![
                "生成AI".to_string(),
                "資金調達".to_string(),
                "法人導入".to_string(),
            ],
        },
        PersistedArticleRecord {
            version: 1,
            article_id: "article-002".to_string(),
            title: "SaaS企業が中堅市場向け新プランを発表".to_string(),
            source_name: "日経ビジネス".to_string(),
            source_key: "nikkei_business".to_string(),
            original_url: "https://example.com/articles/article-002".to_string(),
            fetched_at: "2026-06-04T08:40:00+09:00".to_string(),
            published_at_text: "1時間前".to_string(),
            genre: "ビジネス".to_string(),
            tags: vec!["SaaS".to_string(), "中堅企業".to_string()],
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted: true,
                markdown_generated: true,
                summarized: true,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: ArticleReadState::Unread,
            favorite: false,
            archive_state: PersistedArchiveState::Active,
            is_archived: false,
            recommendation_score: 0.84,
            summary_generated_at: Some("2026-06-04T08:45:00+09:00".to_string()),
            ai_provider: Some("mock".to_string()),
            content_hash: Some("content-hash-002".to_string()),
            excerpt: Some(
                "大企業向け中心だったSaaS製品を、中堅企業でも導入しやすい価格とサポート体制に見直す動きが広がっています。".to_string(),
            ),
            summary: Some(
                "SaaS各社が中堅企業向けに導入支援を強化し、価格だけでなく運用設計までセットで提供する新プランを打ち出しました。".to_string(),
            ),
            yuuko_explanation: Some(
                "単に安くするだけでなく、導入後にどう使い続けてもらうかまで含めて設計している点が重要です。".to_string(),
            ),
            focus_points: vec![
                "中堅企業向けに支援内容を明確化している".to_string(),
                "価格だけでなく運用支援を合わせて提供している".to_string(),
                "導入障壁を下げることが競争力になっている".to_string(),
            ],
            yuuko_comment: Some(
                "続けやすさまで商品に含める流れは、SaaSらしい成熟のしかたですね。".to_string(),
            ),
            keyword_candidates: vec![
                "SaaS".to_string(),
                "中堅企業".to_string(),
                "運用支援".to_string(),
            ],
        },
        PersistedArticleRecord {
            version: 1,
            article_id: "article-003".to_string(),
            title: "新型コンピュータ実験で省電力な推論手法を確認".to_string(),
            source_name: "ITmedia NEWS".to_string(),
            source_key: "itmedia_news".to_string(),
            original_url: "https://example.com/articles/article-003".to_string(),
            fetched_at: "2026-06-04T07:50:00+09:00".to_string(),
            published_at_text: "2時間前".to_string(),
            genre: "テクノロジー".to_string(),
            tags: vec!["半導体".to_string(), "推論".to_string(), "省電力".to_string()],
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted: true,
                markdown_generated: true,
                summarized: true,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: ArticleReadState::Previewed,
            favorite: false,
            archive_state: PersistedArchiveState::Active,
            is_archived: false,
            recommendation_score: 0.79,
            summary_generated_at: Some("2026-06-04T07:56:00+09:00".to_string()),
            ai_provider: Some("mock".to_string()),
            content_hash: Some("content-hash-003".to_string()),
            excerpt: Some(
                "研究チームは、新型コンピュータ構成でAI推論時の消費電力を抑えられる可能性を実験で示しました。".to_string(),
            ),
            summary: Some(
                "新しい計算構成を用いた推論実験で、省電力性と処理効率の両立が期待できる結果が報告されました。".to_string(),
            ),
            yuuko_explanation: Some(
                "推論は学習より身近な場面でたくさん実行されるので、電力効率の改善は実用面でとても効いてきます。".to_string(),
            ),
            focus_points: vec![
                "推論処理での省電力性が主な評価軸になっている".to_string(),
                "研究段階でも実運用を意識した測定が行われている".to_string(),
                "将来の端末実装にも影響する可能性がある".to_string(),
            ],
            yuuko_comment: Some(
                "派手さはなくても、日常的に動く技術ほど省電力化の価値が大きいですね。".to_string(),
            ),
            keyword_candidates: vec![
                "半導体".to_string(),
                "推論".to_string(),
                "省電力".to_string(),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::article::{ArchiveRestoreStatus, ArticleHistoryFilter, ArticleReadState};

    use super::{
        archive_entry_name, month_bucket_from_text, ArticleRepository, ArticleSummaryUpdate,
        PersistedArchiveState, PersistedArticleRecord,
    };

    struct TestRepositoryContext {
        repository: ArticleRepository,
        news_dir: PathBuf,
        root_dir: PathBuf,
    }

    impl TestRepositoryContext {
        fn new() -> Self {
            let unique_suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root_dir = std::env::temp_dir().join(format!(
                "yuuko-article-tests-{}-{unique_suffix}",
                std::process::id()
            ));
            let news_dir = root_dir.join("news");
            let favorites_path = root_dir.join("favorites").join("article_favorites.json");
            std::fs::create_dir_all(&news_dir).unwrap();
            let archive_dir = root_dir.join("archive");
            let repository =
                ArticleRepository::with_paths(news_dir.clone(), favorites_path, archive_dir);
            Self {
                repository,
                news_dir,
                root_dir,
            }
        }
    }

    impl Drop for TestRepositoryContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root_dir);
        }
    }

    #[test]
    fn initialize_default_if_missing_writes_seed_markdown_files() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let files = std::fs::read_dir(context.news_dir.join("202606"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(files, 3);
    }

    #[test]
    fn list_recommended_respects_limit() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context.repository.list_recommended(2).unwrap();
        assert_eq!(articles.len(), 2);
    }

    #[test]
    fn list_recommended_keeps_recommendation_order() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context.repository.list_recommended(3).unwrap();
        assert!(articles[0].recommendation_score >= articles[1].recommendation_score);
        assert!(articles[1].recommendation_score >= articles[2].recommendation_score);
    }

    #[test]
    fn list_history_returns_articles_by_fetched_at_desc() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::All, 3)
            .unwrap();

        assert_eq!(articles.len(), 3);
        assert_eq!(articles[0].article_id, "article-001");
        assert!(articles[0].fetched_at >= articles[1].fetched_at);
        assert!(articles[1].fetched_at >= articles[2].fetched_at);
    }

    #[test]
    fn list_history_uses_article_id_when_fetched_at_is_equal() {
        let context = TestRepositoryContext::new();
        let common_fetched_at = "2026-06-05T10:00:00+09:00".to_string();
        let article_b = PersistedArticleRecord {
            article_id: "article-b".to_string(),
            fetched_at: common_fetched_at.clone(),
            published_at_text: "5分前".to_string(),
            ..super::seed_articles().remove(0)
        };
        let article_a = PersistedArticleRecord {
            article_id: "article-a".to_string(),
            fetched_at: common_fetched_at,
            published_at_text: "1時間前".to_string(),
            ..super::seed_articles().remove(1)
        };
        context.repository.save_article_record(&article_b).unwrap();
        context.repository.save_article_record(&article_a).unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::All, 10)
            .unwrap();

        assert_eq!(articles[0].article_id, "article-a");
        assert_eq!(articles[1].article_id, "article-b");
    }

    #[test]
    fn list_archive_candidates_returns_only_old_non_favorite_non_archived() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();

        // 古い・非お気に入り・非archived → 候補
        let old_plain = PersistedArticleRecord {
            article_id: "old-plain".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        // 古い・お気に入り → 除外
        let old_favorite = PersistedArticleRecord {
            article_id: "old-favorite".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            favorite: true,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        // 古い・archived → 除外
        let old_archived = PersistedArticleRecord {
            article_id: "old-archived".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: true,
            ..super::seed_articles().remove(0)
        };
        // 最近 → 除外
        let recent = PersistedArticleRecord {
            article_id: "recent".to_string(),
            fetched_at: "2026-07-10T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        for record in [&old_plain, &old_favorite, &old_archived, &recent] {
            context.repository.save_article_record(record).unwrap();
        }

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        let candidates = context.repository.list_archive_candidates(now).unwrap();
        let ids: Vec<&str> = candidates
            .iter()
            .map(|candidate| candidate.article_id.as_str())
            .collect();
        assert_eq!(ids, vec!["old-plain"]);
    }

    #[test]
    fn list_archive_candidates_excludes_store_favorite() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();

        // Markdownフラグは非お気に入り（古い・非archived）の記事。
        let old_store_fav = PersistedArticleRecord {
            article_id: "old-store-fav".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&old_store_fav)
            .unwrap();

        // JSON上書き（ArticleFavoriteStore）のみでお気に入り登録する（record フラグは false のまま）。
        // is_effectively_favorite が store 経由のお気に入りも除外することを統合確認する。
        let favorites_path = context
            .root_dir
            .join("favorites")
            .join("article_favorites.json");
        std::fs::create_dir_all(favorites_path.parent().unwrap()).unwrap();
        std::fs::write(
            &favorites_path,
            r#"{"version":1,"favorite_article_ids":["old-store-fav"]}"#,
        )
        .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        let candidates = context.repository.list_archive_candidates(now).unwrap();
        assert!(candidates
            .iter()
            .all(|candidate| candidate.article_id != "old-store-fav"));
    }

    #[test]
    fn archive_candidates_zips_old_articles_and_marks_archived_non_destructively() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();

        // 古い候補2件（同月 2026-05）＋ 最近1件。
        // 月バケットは published_at_text 優先のため、ZIP月を固定するよう明示する。
        let old_a = PersistedArticleRecord {
            article_id: "old-a".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        let old_b = PersistedArticleRecord {
            article_id: "old-b".to_string(),
            fetched_at: "2026-05-02T00:00:00Z".to_string(),
            published_at_text: "2026-05-02T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        let recent = PersistedArticleRecord {
            article_id: "recent".to_string(),
            fetched_at: "2026-07-10T00:00:00Z".to_string(),
            published_at_text: "2026-07-10T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        for record in [&old_a, &old_b, &recent] {
            context.repository.save_article_record(record).unwrap();
        }

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        let summary = context.repository.archive_candidates(now).unwrap();

        // 古い2件が archived 化され、月次ZIPが1つ（2026-05）作られる。
        assert_eq!(summary.archived_article_count, 2);
        assert_eq!(summary.zip_files.len(), 1);
        assert_eq!(summary.zip_files[0].month, "2026-05");
        assert_eq!(summary.zip_files[0].article_count, 2);
        assert!(summary.zip_files[0].size_bytes > 0);

        // ZIPと index が作られている。
        let archive_dir = context.root_dir.join("archive");
        assert!(archive_dir.join("2026-05.zip").exists());
        assert!(archive_dir.join("archive_index.json").exists());

        // 非破壊: 元の記事Markdownは残っている（archived 印は付く）。
        assert!(context.news_dir.join("202605").join("old-a.md").exists());

        // 再実行すると候補は無い（古い2件はarchived済み・recentは新しいため）。
        let candidates_after = context.repository.list_archive_candidates(now).unwrap();
        assert!(candidates_after.is_empty());
    }

    #[test]
    fn restore_archived_article_restores_zip_only_article_idempotently() {
        use chrono::{TimeZone, Utc};

        let context = TestRepositoryContext::new();
        let old_article = PersistedArticleRecord {
            article_id: "restore-target".to_string(),
            title: "アーカイブ時のタイトル".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&old_article)
            .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        let article_path = context.news_dir.join("202605").join("restore-target.md");
        std::fs::remove_file(&article_path).unwrap();

        let result = context
            .repository
            .restore_archived_article("restore-target")
            .unwrap();
        assert_eq!(result.status, ArchiveRestoreStatus::Restored);
        assert!(article_path.exists());

        let mut restored = context
            .repository
            .find_article_record("restore-target")
            .unwrap();
        assert_eq!(restored.archive_state, PersistedArchiveState::Restored);
        assert!(!restored.is_archived);
        assert!(!restored.status.archived);
        assert!(context
            .repository
            .get_article_detail("restore-target")
            .is_ok());
        assert!(context
            .repository
            .list_archive_candidates(now)
            .unwrap()
            .is_empty());

        restored.title = "復元後に編集したタイトル".to_string();
        context.repository.save_article_record(&restored).unwrap();
        let second = context
            .repository
            .restore_archived_article("restore-target")
            .unwrap();
        assert_eq!(second.status, ArchiveRestoreStatus::AlreadyAvailable);
        assert_eq!(
            context
                .repository
                .find_article_record("restore-target")
                .unwrap()
                .title,
            "復元後に編集したタイトル"
        );
    }

    #[test]
    fn restore_archived_article_reactivates_retained_markdown() {
        use chrono::{TimeZone, Utc};

        let context = TestRepositoryContext::new();
        let old_article = PersistedArticleRecord {
            article_id: "retained-archive".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&old_article)
            .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        let result = context
            .repository
            .restore_archived_article("retained-archive")
            .unwrap();

        assert_eq!(result.status, ArchiveRestoreStatus::Restored);
        let restored = context
            .repository
            .find_article_record("retained-archive")
            .unwrap();
        assert_eq!(restored.archive_state, PersistedArchiveState::Restored);
        assert!(!restored.is_archived);
        assert!(context
            .repository
            .list_archive_candidates(now)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn restore_archived_article_rejects_unsafe_index_location() {
        use chrono::{TimeZone, Utc};

        let context = TestRepositoryContext::new();
        let old_article = PersistedArticleRecord {
            article_id: "unsafe-index".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&old_article)
            .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        let article_path = context.news_dir.join("202605").join("unsafe-index.md");
        std::fs::remove_file(&article_path).unwrap();

        let mut index = context.repository.load_archive_index_or_default().unwrap();
        index.archives[0].file = "../2026-05.zip".to_string();
        std::fs::write(
            context.root_dir.join("archive").join("archive_index.json"),
            serde_json::to_vec_pretty(&index).unwrap(),
        )
        .unwrap();

        assert!(context
            .repository
            .restore_archived_article("unsafe-index")
            .is_err());
        assert!(!article_path.exists());
    }

    #[test]
    fn restore_archived_article_rejects_markdown_that_disagrees_with_index() {
        use chrono::{TimeZone, Utc};

        let context = TestRepositoryContext::new();
        let archived_article = PersistedArticleRecord {
            article_id: "metadata-target".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&archived_article)
            .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        let article_path = context.news_dir.join("202605").join("metadata-target.md");
        std::fs::remove_file(&article_path).unwrap();

        let mismatched_article = PersistedArticleRecord {
            article_id: "different-article".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            ..archived_article
        };
        let mismatched_markdown = super::serialize_article_markdown(&mismatched_article).unwrap();
        crate::infra::archive_storage::write_verified_zip(
            &context.root_dir.join("archive").join("2026-05.zip"),
            &[crate::infra::archive_storage::ArchiveEntry {
                name: "metadata-target.md".to_string(),
                contents: mismatched_markdown.into_bytes(),
            }],
        )
        .unwrap();

        assert!(context
            .repository
            .restore_archived_article("metadata-target")
            .is_err());
        assert!(!article_path.exists());
    }

    #[test]
    fn archive_candidates_rebuilds_existing_month_zip_with_archived_records() {
        use chrono::{TimeZone, Utc};
        use zip::ZipArchive;

        let context = TestRepositoryContext::new();
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();

        let old_a = PersistedArticleRecord {
            article_id: "old-a".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        let old_b = PersistedArticleRecord {
            article_id: "old-b".to_string(),
            fetched_at: "2026-05-02T00:00:00Z".to_string(),
            published_at_text: "2026-05-02T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context.repository.save_article_record(&old_a).unwrap();
        context.repository.save_article_record(&old_b).unwrap();

        let first_summary = context.repository.archive_candidates(now).unwrap();
        assert_eq!(first_summary.archived_article_count, 2);
        assert_eq!(first_summary.zip_files[0].article_count, 2);

        let old_c = PersistedArticleRecord {
            article_id: "old-c".to_string(),
            fetched_at: "2026-05-03T00:00:00Z".to_string(),
            published_at_text: "2026-05-03T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context.repository.save_article_record(&old_c).unwrap();

        let second_summary = context.repository.archive_candidates(now).unwrap();
        assert_eq!(second_summary.archived_article_count, 1);
        assert_eq!(second_summary.zip_files[0].article_count, 3);

        let archive_dir = context.root_dir.join("archive");
        assert!(!archive_dir.join("2026-05.zip.rollback").exists());
        let file = std::fs::File::open(archive_dir.join("2026-05.zip")).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for index in 0..archive.len() {
            names.push(archive.by_index(index).unwrap().name().to_string());
        }
        names.sort();
        assert_eq!(names, vec!["old-a.md", "old-b.md", "old-c.md"]);

        let index = context.repository.load_archive_index_or_default().unwrap();
        assert_eq!(index.version, 2);
        assert_eq!(index.archives.len(), 1);
        assert_eq!(index.archives[0].article_count, 3);
        assert!(index.archives[0].catalog_complete);
        assert_eq!(index.archives[0].articles.len(), 3);
        assert_eq!(index.archives[0].articles[0].entry_name, "old-a.md");
    }

    #[test]
    fn archive_index_reads_legacy_v1_without_article_catalog() {
        let context = TestRepositoryContext::new();
        let archive_dir = context.root_dir.join("archive");
        std::fs::create_dir_all(&archive_dir).unwrap();
        std::fs::write(
            archive_dir.join("archive_index.json"),
            r#"{
  "archives": [
    {
      "month": "2026-05",
      "file": "2026-05.zip",
      "articleCount": 2,
      "createdAt": "2026-07-15T00:00:00Z",
      "sizeBytes": 1024
    }
  ]
}"#,
        )
        .unwrap();

        let index = context.repository.load_archive_index_or_default().unwrap();

        assert_eq!(index.version, 1);
        assert_eq!(index.archives.len(), 1);
        assert!(!index.archives[0].catalog_complete);
        assert!(index.archives[0].articles.is_empty());
    }

    #[test]
    fn archive_index_upgrade_keeps_unmigrated_month_incomplete() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();
        let archive_dir = context.root_dir.join("archive");
        std::fs::create_dir_all(&archive_dir).unwrap();
        std::fs::write(
            archive_dir.join("archive_index.json"),
            r#"{
  "archives": [
    {
      "month": "2026-05",
      "file": "2026-05.zip",
      "articleCount": 2,
      "createdAt": "2026-07-15T00:00:00Z",
      "sizeBytes": 1024
    }
  ]
}"#,
        )
        .unwrap();
        let june_article = PersistedArticleRecord {
            article_id: "june-article".to_string(),
            fetched_at: "2026-06-01T00:00:00Z".to_string(),
            published_at_text: "2026-06-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&june_article)
            .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 8, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        let index = context.repository.load_archive_index_or_default().unwrap();

        assert_eq!(index.version, 2);
        assert_eq!(index.archives.len(), 2);
        assert!(!index.archives[0].catalog_complete);
        assert!(index.archives[0].articles.is_empty());
        assert!(index.archives[1].catalog_complete);
        assert_eq!(index.archives[1].articles.len(), 1);
    }

    #[test]
    fn archive_index_rejects_mismatched_entry_name() {
        let context = TestRepositoryContext::new();
        let archive_dir = context.root_dir.join("archive");
        std::fs::create_dir_all(&archive_dir).unwrap();
        std::fs::write(
            archive_dir.join("archive_index.json"),
            r#"{
  "version": 2,
  "archives": [
    {
      "month": "2026-05",
      "file": "2026-05.zip",
      "articleCount": 1,
      "createdAt": "2026-07-15T00:00:00Z",
      "sizeBytes": 1024,
      "articles": [
        {
          "articleId": "safe-id",
          "entryName": "../evil.md",
          "title": "記事",
          "sourceName": "Example",
          "publishedAtText": "2026-05-01T00:00:00Z",
          "fetchedAt": "2026-05-01T00:00:00Z",
          "genre": "テクノロジー",
          "readState": "unread",
          "recommendationScore": 0.5
        }
      ]
    }
  ]
}"#,
        )
        .unwrap();

        let result = context.repository.load_archive_index_or_default();

        assert!(result.is_err());
    }

    #[test]
    fn list_history_keeps_active_articles_when_archive_index_is_corrupt() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();
        let archive_dir = context.root_dir.join("archive");
        std::fs::create_dir_all(&archive_dir).unwrap();
        std::fs::write(archive_dir.join("archive_index.json"), "{broken").unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::All, 10)
            .unwrap();

        assert_eq!(articles.len(), 3);
    }

    #[test]
    fn list_history_includes_catalog_article_after_markdown_is_removed() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();
        let old_article = PersistedArticleRecord {
            article_id: "catalog-only".to_string(),
            title: "ZIPにだけ残る記事".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&old_article)
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        std::fs::remove_file(context.news_dir.join("202605").join("catalog-only.md")).unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::Archived, 10)
            .unwrap();

        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].article_id, "catalog-only");
        assert_eq!(articles[0].title, "ZIPにだけ残る記事");
        assert!(articles[0].is_archived);
    }

    #[test]
    fn list_history_prefers_active_markdown_over_catalog_snapshot() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();
        let old_article = PersistedArticleRecord {
            article_id: "active-wins".to_string(),
            title: "アーカイブ時のタイトル".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&old_article)
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();

        let mut active_article = context
            .repository
            .find_article_record("active-wins")
            .unwrap();
        active_article.title = "Markdown側で更新したタイトル".to_string();
        context
            .repository
            .save_article_record(&active_article)
            .unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::All, 10)
            .unwrap();

        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].title, "Markdown側で更新したタイトル");
    }

    #[test]
    fn existing_article_ids_include_catalog_article_without_markdown() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();
        let old_article = PersistedArticleRecord {
            article_id: "archived-id".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&old_article)
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        std::fs::remove_file(context.news_dir.join("202605").join("archived-id.md")).unwrap();

        let article_ids = context.repository.existing_article_ids().unwrap();

        assert!(article_ids.contains("archived-id"));
    }

    #[test]
    fn archive_candidates_refuses_month_rebuild_when_catalog_article_markdown_is_missing() {
        use chrono::{TimeZone, Utc};
        use std::io::Read;
        use zip::ZipArchive;

        let context = TestRepositoryContext::new();
        let archived_article = PersistedArticleRecord {
            article_id: "zip-only".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&archived_article)
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        context.repository.archive_candidates(now).unwrap();
        std::fs::remove_file(context.news_dir.join("202605").join("zip-only.md")).unwrap();

        let new_article = PersistedArticleRecord {
            article_id: "new-same-month".to_string(),
            fetched_at: "2026-05-02T00:00:00Z".to_string(),
            published_at_text: "2026-05-02T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&new_article)
            .unwrap();

        let result = context.repository.archive_candidates(now);

        assert!(result.is_err());
        let new_article_after = context
            .repository
            .find_article_record("new-same-month")
            .unwrap();
        assert!(!new_article_after.is_archived);

        let archive_dir = context.root_dir.join("archive");
        let file = std::fs::File::open(archive_dir.join("2026-05.zip")).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 1);
        let mut zip_only = archive.by_name("zip-only.md").unwrap();
        let mut contents = String::new();
        zip_only.read_to_string(&mut contents).unwrap();
        assert!(contents.contains("articleId: zip-only"));
        drop(zip_only);
        assert!(archive.by_name("new-same-month.md").is_err());

        let index = context.repository.load_archive_index_or_default().unwrap();
        assert_eq!(index.archives.len(), 1);
        assert_eq!(index.archives[0].articles.len(), 1);
        assert_eq!(index.archives[0].articles[0].article_id, "zip-only");
    }

    #[test]
    fn archive_candidates_restores_existing_month_zip_when_rebuild_fails() {
        use chrono::{TimeZone, Utc};
        use zip::ZipArchive;

        let context = TestRepositoryContext::new();
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();

        let old_a = PersistedArticleRecord {
            article_id: "old-a".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context.repository.save_article_record(&old_a).unwrap();
        context.repository.archive_candidates(now).unwrap();

        let old_b = PersistedArticleRecord {
            article_id: "old-b".to_string(),
            fetched_at: "2026-05-02T00:00:00Z".to_string(),
            published_at_text: "2026-05-02T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context.repository.save_article_record(&old_b).unwrap();
        std::fs::create_dir_all(context.news_dir.join("202605").join("old-b.md.tmp")).unwrap();

        let result = context.repository.archive_candidates(now);
        assert!(result.is_err());

        let archive_dir = context.root_dir.join("archive");
        assert!(!archive_dir.join("2026-05.zip.rollback").exists());
        let file = std::fs::File::open(archive_dir.join("2026-05.zip")).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 1);
        assert!(archive.by_name("old-a.md").is_ok());
        assert!(archive.by_name("old-b.md").is_err());

        let index = context.repository.load_archive_index_or_default().unwrap();
        assert_eq!(index.archives.len(), 1);
        assert_eq!(index.archives[0].article_count, 1);

        let old_a_after = context.repository.find_article_record("old-a").unwrap();
        let old_b_after = context.repository.find_article_record("old-b").unwrap();
        assert!(old_a_after.is_archived);
        assert!(!old_b_after.is_archived);
    }

    #[test]
    fn archive_candidates_indexes_successful_month_when_later_month_fails() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();

        let old_may = PersistedArticleRecord {
            article_id: "old-may".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        let old_june = PersistedArticleRecord {
            article_id: "old-june".to_string(),
            fetched_at: "2026-06-01T00:00:00Z".to_string(),
            published_at_text: "2026-06-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context.repository.save_article_record(&old_may).unwrap();
        context.repository.save_article_record(&old_june).unwrap();

        let archive_dir = context.root_dir.join("archive");
        std::fs::create_dir_all(archive_dir.join("2026-06.zip.tmp")).unwrap();

        let now = Utc.with_ymd_and_hms(2026, 8, 15, 0, 0, 0).unwrap();
        let result = context.repository.archive_candidates(now);
        assert!(result.is_err());

        let index = context.repository.load_archive_index_or_default().unwrap();
        assert_eq!(index.archives.len(), 1);
        assert_eq!(index.archives[0].month, "2026-05");
        assert!(archive_dir.join("2026-05.zip").exists());

        let may_record = context.repository.find_article_record("old-may").unwrap();
        assert!(may_record.is_archived);

        let june_record = context.repository.find_article_record("old-june").unwrap();
        assert!(!june_record.is_archived);
    }

    #[test]
    fn archive_candidates_rolls_back_archived_marks_when_save_fails() {
        use chrono::{TimeZone, Utc};
        let context = TestRepositoryContext::new();

        let old_a = PersistedArticleRecord {
            article_id: "old-a".to_string(),
            fetched_at: "2026-05-01T00:00:00Z".to_string(),
            published_at_text: "2026-05-01T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        let old_b = PersistedArticleRecord {
            article_id: "old-b".to_string(),
            fetched_at: "2026-05-02T00:00:00Z".to_string(),
            published_at_text: "2026-05-02T00:00:00Z".to_string(),
            favorite: false,
            is_archived: false,
            ..super::seed_articles().remove(0)
        };
        context.repository.save_article_record(&old_a).unwrap();
        context.repository.save_article_record(&old_b).unwrap();

        // 2件目保存時の atomic_write を失敗させ、1件目だけ archived 済みで残らないことを確認する。
        std::fs::create_dir_all(context.news_dir.join("202605").join("old-b.md.tmp")).unwrap();

        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        let result = context.repository.archive_candidates(now);
        assert!(result.is_err());

        let old_a_after = context.repository.find_article_record("old-a").unwrap();
        let old_b_after = context.repository.find_article_record("old-b").unwrap();
        assert!(!old_a_after.is_archived);
        assert!(!old_a_after.status.archived);
        assert!(!old_b_after.is_archived);
        assert!(!old_b_after.status.archived);

        let archive_dir = context.root_dir.join("archive");
        assert!(!archive_dir.join("2026-05.zip").exists());
        let index = context.repository.load_archive_index_or_default().unwrap();
        assert!(index.archives.is_empty());
    }

    #[test]
    fn archive_entry_name_rejects_path_like_article_ids() {
        assert_eq!(
            archive_entry_name("news_0123-abcd").unwrap(),
            "news_0123-abcd.md"
        );
        assert!(archive_entry_name("../evil").is_err());
        assert!(archive_entry_name("evil/name").is_err());
        assert!(archive_entry_name("").is_err());
    }

    #[test]
    fn list_history_respects_limit() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::All, 2)
            .unwrap();

        assert_eq!(articles.len(), 2);
    }

    #[test]
    fn list_history_filters_read_state() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let unread_articles = context
            .repository
            .list_history(ArticleHistoryFilter::Unread, 10)
            .unwrap();
        assert!(!unread_articles.is_empty());
        assert!(unread_articles
            .iter()
            .all(|article| article.read_state == ArticleReadState::Unread));

        let read_articles = context
            .repository
            .list_history(ArticleHistoryFilter::Read, 10)
            .unwrap();
        assert!(!read_articles.is_empty());
        assert!(read_articles
            .iter()
            .all(|article| article.read_state != ArticleReadState::Unread));
    }

    #[test]
    fn list_history_filters_favorites() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();
        context
            .repository
            .update_article_favorite("article-002", true)
            .unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::Favorite, 10)
            .unwrap();

        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].article_id, "article-002");
        assert!(articles[0].is_favorite);
    }

    #[test]
    fn list_history_filters_archived_articles() {
        let context = TestRepositoryContext::new();
        let archived_article = PersistedArticleRecord {
            article_id: "article-archived".to_string(),
            fetched_at: "2026-06-05T10:00:00+09:00".to_string(),
            is_archived: true,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&archived_article)
            .unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::Archived, 10)
            .unwrap();

        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].article_id, "article-archived");
        assert!(articles[0].is_archived);
    }

    #[test]
    fn get_article_detail_returns_matching_article() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let article = context
            .repository
            .get_article_detail("article-002")
            .unwrap();

        assert_eq!(article.article_id, "article-002");
        assert_eq!(article.source_name, "日経ビジネス");
        assert!(!article.focus_points.is_empty());
    }

    #[test]
    fn get_article_detail_returns_not_found_for_unknown_id() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let error = context
            .repository
            .get_article_detail("article-999")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "not found: article not found: article-999"
        );
    }

    #[test]
    fn update_article_favorite_persists_state_for_list_and_detail() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let result = context
            .repository
            .update_article_favorite("article-001", true)
            .unwrap();
        assert!(result.is_favorite);

        let articles = context.repository.list_recommended(3).unwrap();
        assert!(
            articles
                .iter()
                .find(|article| article.article_id == "article-001")
                .unwrap()
                .is_favorite
        );

        let detail = context
            .repository
            .get_article_detail("article-001")
            .unwrap();
        assert!(detail.is_favorite);
    }

    #[test]
    fn update_article_summary_persists_into_markdown() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let update = ArticleSummaryUpdate {
            summary: "新しいAI要約テキスト".to_string(),
            yuuko_explanation: "新しい再説明".to_string(),
            focus_points: vec!["観点A".to_string(), "観点B".to_string()],
            yuuko_comment: "新しい一言".to_string(),
            ai_provider: "gemini".to_string(),
            generated_at: "2026-06-08T00:00:00Z".to_string(),
        };
        context
            .repository
            .update_article_summary("article-002", update)
            .unwrap();

        // 再表示（DTO）で保存値がキャッシュとして返ることを確認。
        let detail = context
            .repository
            .get_article_detail("article-002")
            .unwrap();
        assert_eq!(detail.summary.as_deref(), Some("新しいAI要約テキスト"));
        assert_eq!(detail.yuuko_explanation.as_deref(), Some("新しい再説明"));
        assert_eq!(detail.yuuko_comment.as_deref(), Some("新しい一言"));
        assert_eq!(
            detail.focus_points,
            vec!["観点A".to_string(), "観点B".to_string()]
        );

        // front matter のメタ情報も永続化されていることを確認。
        let record = context
            .repository
            .find_article_record("article-002")
            .unwrap();
        assert!(record.status.summarized);
        assert_eq!(record.ai_provider.as_deref(), Some("gemini"));
        assert_eq!(
            record.summary_generated_at.as_deref(),
            Some("2026-06-08T00:00:00Z")
        );
    }

    #[test]
    fn update_article_favorite_removes_existing_favorite() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();
        context
            .repository
            .update_article_favorite("article-001", true)
            .unwrap();

        let result = context
            .repository
            .update_article_favorite("article-001", false)
            .unwrap();
        assert!(!result.is_favorite);

        let detail = context
            .repository
            .get_article_detail("article-001")
            .unwrap();
        assert!(!detail.is_favorite);
    }

    #[test]
    fn save_article_record_uses_month_bucket_directory() {
        let context = TestRepositoryContext::new();
        let article = PersistedArticleRecord {
            article_id: "article-custom".to_string(),
            published_at_text: "2026-07-01T10:00:00+09:00".to_string(),
            fetched_at: "2026-07-01T10:05:00+09:00".to_string(),
            ..super::seed_articles().remove(0)
        };

        context.repository.save_article_record(&article).unwrap();

        assert!(context
            .news_dir
            .join("202607")
            .join("article-custom.md")
            .exists());
    }

    #[test]
    fn month_bucket_prefers_first_six_digits() {
        assert_eq!(
            month_bucket_from_text("2026-06-04T09:10:00+09:00").as_deref(),
            Some("202606")
        );
        assert_eq!(month_bucket_from_text("5分前"), None);
    }
}
