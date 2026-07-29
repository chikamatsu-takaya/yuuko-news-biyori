use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::dictionary::{
    normalize_text, DictionaryEntryDto, DictionaryEntryListItemDto, DictionaryEntryType,
    PersistedDictionaryEntry, PersistedDictionaryStore,
};
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct DictionaryRepository {
    dictionary_path: PathBuf,
}

impl DictionaryRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dictionary_path: paths.dictionary_path.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_path(dictionary_path: PathBuf) -> Self {
        Self { dictionary_path }
    }

    /// 保存済み辞書を正規化済み用語の完全一致で検索する（記事解決は Service 側で済み）。
    /// 命中なら `Some(DictionaryEntryDto)`、未命中なら `Ok(None)`、読み込み・JSON破損など
    /// 検索失敗は `Err(AppError)`（未命中と区別）。固定サンプル解説・汎用文の生成は担当しない。
    ///
    /// 同じ正規化語を持つエントリが複数あるときの選択規則:
    ///   1. `source_article_ids` に現在の `article_id` を含むエントリを最優先。
    ///   2. 現在の記事に紐づく候補が無ければ、記事横断候補から決定的に1件を選ぶ。
    /// どちらの母集団でも、`entry_timestamp_key`（更新/作成日時）降順→`dictionary_id` 昇順の
    /// 全順序で1件に決めるため、ファイル内の格納順や逆順構築に依存しない。
    pub fn find_saved_entry(
        &self,
        article_id: &str,
        normalized_text: &str,
    ) -> Result<Option<DictionaryEntryDto>, AppError> {
        let store = self.load_store_or_default()?;

        let (current_article, cross_article): (Vec<_>, Vec<_>) = store
            .entries
            .into_iter()
            .filter(|entry| entry.normalized_text == normalized_text)
            .partition(|entry| {
                entry
                    .source_article_ids
                    .iter()
                    .any(|source_article_id| source_article_id == article_id)
            });

        // 現在記事に紐づく候補を最優先。無ければ記事横断候補から決定的に選ぶ。
        let pool = if current_article.is_empty() {
            cross_article
        } else {
            current_article
        };

        Ok(pick_saved_entry(pool).map(|entry| entry.to_dto()))
    }

    pub fn save_dictionary_entry(
        &self,
        entry: DictionaryEntryDto,
    ) -> Result<DictionaryEntryDto, AppError> {
        let mut store = self.load_store_or_default()?;
        let now_text = current_unix_timestamp_text();

        if let Some(existing_entry) = store
            .entries
            .iter_mut()
            .find(|existing_entry| existing_entry.dictionary_id == entry.entry_id)
        {
            existing_entry.apply_from_dto(entry, now_text);
            let saved_entry = existing_entry.to_dto();
            self.save_store(&store)?;
            return Ok(saved_entry);
        }

        let persisted_entry = PersistedDictionaryEntry::from_dto(entry, now_text);
        let saved_entry = persisted_entry.to_dto();
        store.entries.push(persisted_entry);
        self.save_store(&store)?;
        Ok(saved_entry)
    }

    pub fn list_dictionary_entries(
        &self,
        keyword: Option<&str>,
        entry_type: Option<DictionaryEntryType>,
        starred_only: bool,
    ) -> Result<Vec<DictionaryEntryListItemDto>, AppError> {
        let mut entries = self.load_store_or_default()?.entries;
        entries.sort_by(|left, right| entry_timestamp_key(right).cmp(entry_timestamp_key(left)));

        Ok(entries
            .into_iter()
            .filter(|entry| matches_filters(entry, keyword, entry_type.as_ref(), starred_only))
            .map(|entry| entry.to_list_item_dto())
            .collect())
    }

    pub fn update_dictionary_memo(
        &self,
        entry_id: &str,
        memo: Option<String>,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let mut store = self.load_store_or_default()?;
        let entry = store
            .entries
            .iter_mut()
            .find(|entry| entry.dictionary_id == entry_id)
            .ok_or_else(|| AppError::NotFound(format!("dictionary entry not found: {entry_id}")))?;
        entry.memo = memo;
        let updated = entry.to_list_item_dto();
        self.save_store(&store)?;
        Ok(updated)
    }

    pub fn update_dictionary_favorite(
        &self,
        entry_id: &str,
        is_starred: bool,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let mut store = self.load_store_or_default()?;
        let entry = store
            .entries
            .iter_mut()
            .find(|entry| entry.dictionary_id == entry_id)
            .ok_or_else(|| AppError::NotFound(format!("dictionary entry not found: {entry_id}")))?;
        entry.favorite = is_starred;
        let updated = entry.to_list_item_dto();
        self.save_store(&store)?;
        Ok(updated)
    }

    pub fn delete_dictionary_entry(&self, entry_id: &str) -> Result<String, AppError> {
        let mut store = self.load_store_or_default()?;
        let before = store.entries.len();
        store
            .entries
            .retain(|entry| entry.dictionary_id != entry_id);
        if store.entries.len() == before {
            return Err(AppError::NotFound(format!(
                "dictionary entry not found: {entry_id}"
            )));
        }
        self.save_store(&store)?;
        Ok(entry_id.to_string())
    }

    fn load_store_or_default(&self) -> Result<PersistedDictionaryStore, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.dictionary_path.exists() {
            return Ok(PersistedDictionaryStore::with_current_version());
        }

        // 読み込み・パース失敗は「検索失敗（内部エラー）」。生のファイルパスや破損内容を
        // 公開エラーへ載せず、詳細は調査用ログにのみ残す（未命中＝Ok(default) とは区別する）。
        // ログにも辞書の保存先パス・本文を出さない（エラー種別・行/列の位置情報だけ残す）。
        let raw = std::fs::read_to_string(&self.dictionary_path).map_err(|error| {
            log::error!("Failed to read dictionary store (kind: {:?})", error.kind());
            AppError::Io(io::Error::new(
                io::ErrorKind::Other,
                "dictionary store could not be read",
            ))
        })?;
        let mut store =
            serde_json::from_str::<PersistedDictionaryStore>(&raw).map_err(|error| {
                log::error!(
                    "Failed to parse dictionary store (line: {}, column: {})",
                    error.line(),
                    error.column()
                );
                AppError::Parse("dictionary store is corrupted".to_string())
            })?;
        if store.version == 0 {
            store.version = 1;
        }
        Ok(store)
    }

    fn save_store(&self, store: &PersistedDictionaryStore) -> Result<(), AppError> {
        if let Some(parent) = self.dictionary_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let temp_path = self.dictionary_path.with_extension("json.tmp");
        let backup_path = self.dictionary_path.with_extension("json.bak");
        let payload = serde_json::to_vec_pretty(store)?;
        std::fs::write(&temp_path, payload)?;

        let had_existing = self.dictionary_path.exists();
        if had_existing {
            if backup_path.exists() {
                std::fs::remove_file(&backup_path)?;
            }
            std::fs::rename(&self.dictionary_path, &backup_path)?;
        }

        match std::fs::rename(&temp_path, &self.dictionary_path) {
            Ok(()) => {
                if had_existing && backup_path.exists() {
                    if let Err(error) = std::fs::remove_file(&backup_path) {
                        log::warn!("Failed to remove dictionary backup: {error}");
                    }
                }
                Ok(())
            }
            Err(error) => {
                log::error!("Failed to promote temporary dictionary file: {error}");

                if had_existing && backup_path.exists() {
                    if let Err(restore_error) = std::fs::rename(&backup_path, &self.dictionary_path)
                    {
                        log::error!("Failed to restore dictionary backup: {restore_error}");
                    }
                }

                if temp_path.exists() {
                    let _ = std::fs::remove_file(&temp_path);
                }

                Err(error.into())
            }
        }
    }

    fn restore_backup_if_primary_missing(&self) {
        if self.dictionary_path.exists() {
            return;
        }

        let backup_path = self.dictionary_path.with_extension("json.bak");
        if !backup_path.exists() {
            return;
        }

        log::warn!("dictionary entries file is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.dictionary_path) {
            log::error!("Failed to restore dictionary backup: {error}");
        }
    }
}

/// 同じ正規化語の候補群から、格納順に依存しない決定的な規則で1件を選ぶ。
/// 規則: 更新/作成日時（entry_timestamp_key）の降順、同値は dictionary_id 昇順で先頭。
/// dictionary_id は一意なので全順序になり、母集団を逆順で構築しても同じエントリが選ばれる。
fn pick_saved_entry(
    mut entries: Vec<PersistedDictionaryEntry>,
) -> Option<PersistedDictionaryEntry> {
    entries.sort_by(|left, right| {
        entry_timestamp_key(right)
            .cmp(entry_timestamp_key(left))
            .then_with(|| left.dictionary_id.cmp(&right.dictionary_id))
    });
    entries.into_iter().next()
}

fn current_unix_timestamp_text() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().to_string(),
        Err(_) => "0".to_string(),
    }
}

fn matches_filters(
    entry: &PersistedDictionaryEntry,
    keyword: Option<&str>,
    entry_type: Option<&DictionaryEntryType>,
    starred_only: bool,
) -> bool {
    if starred_only && !entry.favorite {
        return false;
    }

    if let Some(entry_type) = entry_type {
        if &entry.entry_type != entry_type {
            return false;
        }
    }

    if let Some(keyword) = keyword {
        let related_article_title = entry.related_article_title.as_deref().unwrap_or_default();
        let keyword_matched = [
            entry.target_text.as_str(),
            entry.short_explanation.as_str(),
            entry.detail_explanation.as_str(),
            related_article_title,
        ]
        .into_iter()
        .map(normalize_text)
        .any(|value| value.contains(keyword));

        if !keyword_matched {
            return false;
        }
    }

    true
}

fn entry_timestamp_key(entry: &PersistedDictionaryEntry) -> &str {
    entry
        .last_referenced_at
        .as_deref()
        .unwrap_or(&entry.created_at)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::dictionary::{
        DictionaryEntryDto, DictionaryEntryType, PersistedDictionaryEntry, PersistedDictionaryStore,
    };

    use super::DictionaryRepository;

    // 固定の created_at / dictionary_id / source_article_ids を持つ保存済みエントリを作る。
    // 選択規則（記事優先・記事横断の決定的選択）を格納順に依存せず検証するための土台。
    fn persisted_entry(
        dictionary_id: &str,
        normalized_text: &str,
        created_at: &str,
        source_article_ids: &[&str],
        short_explanation: &str,
    ) -> PersistedDictionaryEntry {
        PersistedDictionaryEntry {
            version: 1,
            dictionary_id: dictionary_id.to_string(),
            target_text: normalized_text.to_string(),
            normalized_text: normalized_text.to_string(),
            entry_type: DictionaryEntryType::Term,
            short_explanation: short_explanation.to_string(),
            detail_explanation: format!("{short_explanation}（詳細）"),
            created_at: created_at.to_string(),
            last_referenced_at: None,
            reference_count: 1,
            source_article_ids: source_article_ids.iter().map(|id| id.to_string()).collect(),
            favorite: false,
            memo: None,
            related_article_title: None,
            related_article_id: source_article_ids.first().map(|id| id.to_string()),
        }
    }

    struct TestRepositoryContext {
        repository: DictionaryRepository,
        root_dir: PathBuf,
        dictionary_path: PathBuf,
    }

    impl TestRepositoryContext {
        fn new() -> Self {
            let unique_suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root_dir = std::env::temp_dir().join(format!(
                "yuuko-dictionary-tests-{}-{unique_suffix}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root_dir).unwrap();
            let dictionary_path = root_dir.join("dictionary").join("entries.json");
            let repository = DictionaryRepository::with_path(dictionary_path.clone());
            Self {
                repository,
                root_dir,
                dictionary_path,
            }
        }

        // 破損した辞書ストアを書き込む（検索失敗＝内部エラーの検証用）。
        fn write_raw_store(&self, contents: &str) {
            std::fs::create_dir_all(self.dictionary_path.parent().unwrap()).unwrap();
            std::fs::write(&self.dictionary_path, contents).unwrap();
        }

        // 任意の永続エントリ列で辞書ストアを構築する（選択規則の格納順非依存を検証するため）。
        fn write_store(&self, entries: Vec<PersistedDictionaryEntry>) {
            let store = PersistedDictionaryStore {
                version: 1,
                entries,
            };
            let payload = serde_json::to_vec_pretty(&store).unwrap();
            std::fs::create_dir_all(self.dictionary_path.parent().unwrap()).unwrap();
            std::fs::write(&self.dictionary_path, payload).unwrap();
        }
    }

    impl Drop for TestRepositoryContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root_dir);
        }
    }

    fn saved_entry() -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: "entry-article-001-generated-ai".to_string(),
            key_text: "生成AI".to_string(),
            entry_type: DictionaryEntryType::Term,
            short_explanation: "保存済みの短い説明".to_string(),
            detail_explanation: "保存済みの詳しい説明".to_string(),
            related_article_id: Some("article-001".to_string()),
            related_article_title: Some("生成AIスタートアップの資金調達が再加速".to_string()),
            is_starred: true,
        }
    }

    #[test]
    fn find_saved_entry_returns_none_when_store_missing() {
        // 保存済みが無い＝未命中は Ok(None)（検索失敗ではない）。ファイルも作らない。
        let context = TestRepositoryContext::new();
        let hit = context
            .repository
            .find_saved_entry("article-001", "生成ai")
            .unwrap();

        assert!(hit.is_none());
        assert!(
            !context.dictionary_path.exists(),
            "検索だけで辞書ストアを作成してはならない"
        );
    }

    #[test]
    fn find_saved_entry_returns_saved_hit() {
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let hit = context
            .repository
            .find_saved_entry("article-001", "生成ai")
            .unwrap()
            .expect("保存済みエントリが命中するはず");

        assert_eq!(hit.short_explanation, "保存済みの短い説明");
        assert_eq!(hit.detail_explanation, "保存済みの詳しい説明");
        assert!(hit.is_starred);
    }

    #[test]
    fn find_saved_entry_reuses_across_articles() {
        // 保存元は article-001 だが、別記事IDで同じ正規化用語を引いても再利用する（記事横断）。
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let hit = context
            .repository
            .find_saved_entry("rss-20260728-other-7", "生成ai")
            .unwrap()
            .expect("記事横断でも命中するはず");

        assert_eq!(hit.short_explanation, "保存済みの短い説明");
        assert!(hit.is_starred);
    }

    #[test]
    fn find_saved_entry_prefers_entry_linked_to_current_article_regardless_of_order() {
        // 同じ正規化語 "用語" の2件。記事横断側の方が新しくても、現在記事に紐づく方を最優先する。
        let context = TestRepositoryContext::new();
        let cross = persisted_entry(
            "dict-cross",
            "用語",
            "200",
            &["other-article"],
            "記事横断の説明",
        );
        let linked = persisted_entry(
            "dict-linked",
            "用語",
            "100",
            &["article-current"],
            "現在記事の説明",
        );

        // 紐づく方を配列末尾に置いても（ファイル順に依存せず）選ばれる。
        context.write_store(vec![cross.clone(), linked.clone()]);
        let hit = context
            .repository
            .find_saved_entry("article-current", "用語")
            .unwrap()
            .unwrap();
        assert_eq!(hit.short_explanation, "現在記事の説明");

        // 逆順で構築しても同じエントリが選ばれる。
        context.write_store(vec![linked, cross]);
        let hit_reversed = context
            .repository
            .find_saved_entry("article-current", "用語")
            .unwrap()
            .unwrap();
        assert_eq!(hit_reversed.short_explanation, "現在記事の説明");
    }

    #[test]
    fn find_saved_entry_cross_article_choice_is_deterministic_and_order_independent() {
        // 現在記事に紐づく候補が無い場合の決定規則:
        //   entry_timestamp_key（更新/作成日時）降順 → dictionary_id 昇順の先頭。
        // ここは created_at 同値なので dictionary_id 昇順先頭 "dict-a" が、格納順・逆順に関係なく選ばれる。
        let context = TestRepositoryContext::new();
        let entry_a = persisted_entry("dict-a", "用語", "100", &["article-x"], "A の説明");
        let entry_b = persisted_entry("dict-b", "用語", "100", &["article-y"], "B の説明");

        context.write_store(vec![entry_b.clone(), entry_a.clone()]);
        let hit = context
            .repository
            .find_saved_entry("article-current", "用語")
            .unwrap()
            .unwrap();
        assert_eq!(hit.short_explanation, "A の説明");

        context.write_store(vec![entry_a, entry_b]);
        let hit_reversed = context
            .repository
            .find_saved_entry("article-current", "用語")
            .unwrap()
            .unwrap();
        assert_eq!(hit_reversed.short_explanation, "A の説明");
    }

    #[test]
    fn find_saved_entry_cross_article_prefers_newer_timestamp() {
        // 記事横断候補は、日時が異なる場合は新しい方（entry_timestamp_key 降順）を選ぶ。
        let context = TestRepositoryContext::new();
        let older = persisted_entry("dict-older", "用語", "100", &["article-x"], "古い説明");
        let newer = persisted_entry("dict-newer", "用語", "200", &["article-y"], "新しい説明");

        context.write_store(vec![older, newer]);
        let hit = context
            .repository
            .find_saved_entry("article-current", "用語")
            .unwrap()
            .unwrap();
        assert_eq!(hit.short_explanation, "新しい説明");
    }

    #[test]
    fn find_saved_entry_does_not_persist_store() {
        // 検索（未命中）だけでは辞書ファイルを作成・更新しない。
        let context = TestRepositoryContext::new();
        context
            .repository
            .find_saved_entry("article-001", "未保存の用語")
            .unwrap();

        assert!(
            !context.dictionary_path.exists(),
            "検索だけで辞書ストアを作成してはならない"
        );
    }

    #[test]
    fn find_saved_entry_reports_corrupted_store_as_safe_internal_error() {
        // JSON破損は「検索失敗＝内部エラー」。未命中(Ok(None))とは区別され、生の内容・パスを公開しない。
        let context = TestRepositoryContext::new();
        context.write_raw_store("{ this is not valid json :: 生成AIの本文 }");

        let error = context
            .repository
            .find_saved_entry("article-001", "生成ai")
            .unwrap_err();

        let command_error = crate::error::CommandError::from(error);
        assert_eq!(command_error.code, "PARSE_ERROR");
        assert_eq!(
            command_error.message,
            "parse error: dictionary store is corrupted"
        );
        assert!(!command_error.message.contains("生成AIの本文"));
        assert!(!command_error.message.contains(".json"));
        assert!(!command_error.message.contains("this is not valid json"));
    }

    #[test]
    fn find_saved_entry_reports_io_read_failure_as_safe_internal_error() {
        // entries.json の位置をディレクトリにして read_to_string を失敗させる（クロスプラットフォーム）。
        let context = TestRepositoryContext::new();
        std::fs::create_dir_all(&context.dictionary_path).unwrap();

        let error = context
            .repository
            .find_saved_entry("article-001", "生成ai")
            .unwrap_err();

        let command_error = crate::error::CommandError::from(error);
        assert_eq!(command_error.code, "IO_ERROR");
        assert_eq!(
            command_error.message,
            "io error: dictionary store could not be read"
        );
        // 公開エラーに保存先パス・拡張子・一時ディレクトリ名・OS絶対パス・生の io メッセージを含めない。
        assert!(!command_error.message.contains(".json"));
        assert!(!command_error.message.contains("yuuko-dictionary-tests"));
        assert!(!command_error.message.contains(":\\"));
        assert!(!command_error.message.contains("/tmp"));
    }

    #[test]
    fn save_dictionary_entry_updates_existing_entry() {
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let mut updated_entry = saved_entry();
        updated_entry.short_explanation = "更新後の短い説明".to_string();
        updated_entry.detail_explanation = "更新後の詳しい説明".to_string();
        let updated = context
            .repository
            .save_dictionary_entry(updated_entry)
            .unwrap();

        assert_eq!(updated.short_explanation, "更新後の短い説明");
        assert_eq!(updated.detail_explanation, "更新後の詳しい説明");
    }

    #[test]
    fn list_dictionary_entries_returns_saved_entries_only() {
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let entries = context
            .repository
            .list_dictionary_entries(None, None, false)
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key_text, "生成AI");
        assert!(entries[0].is_starred);
    }

    #[test]
    fn list_dictionary_entries_filters_by_keyword_and_starred() {
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let non_starred = DictionaryEntryDto {
            entry_id: "entry-article-002-saas".to_string(),
            key_text: "SaaS".to_string(),
            entry_type: DictionaryEntryType::Term,
            short_explanation: "クラウドで提供されるソフトウェア".to_string(),
            detail_explanation: "導入支援を含むSaaSの説明".to_string(),
            related_article_id: Some("article-002".to_string()),
            related_article_title: Some("国内SaaS企業、業務改善支援の新施策を発表".to_string()),
            is_starred: false,
        };
        context
            .repository
            .save_dictionary_entry(non_starred)
            .unwrap();

        let entries = context
            .repository
            .list_dictionary_entries(Some("生成ai"), Some(DictionaryEntryType::Term), true)
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key_text, "生成AI");
    }

    #[test]
    fn update_dictionary_memo_sets_and_returns_memo() {
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let updated = context
            .repository
            .update_dictionary_memo(
                "entry-article-001-generated-ai",
                Some("あとで読む".to_string()),
            )
            .unwrap();
        assert_eq!(updated.memo.as_deref(), Some("あとで読む"));

        let entries = context
            .repository
            .list_dictionary_entries(None, None, false)
            .unwrap();
        assert_eq!(entries[0].memo.as_deref(), Some("あとで読む"));
    }

    #[test]
    fn update_dictionary_memo_reject_unknown_entry() {
        let context = TestRepositoryContext::new();
        let error = context
            .repository
            .update_dictionary_memo("entry-unknown", Some("x".to_string()))
            .unwrap_err();
        assert!(error.to_string().contains("dictionary entry not found"));
    }

    #[test]
    fn update_dictionary_favorite_sets_and_returns_favorite() {
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let updated = context
            .repository
            .update_dictionary_favorite("entry-article-001-generated-ai", false)
            .unwrap();
        assert!(!updated.is_starred);

        let entries = context
            .repository
            .list_dictionary_entries(None, None, false)
            .unwrap();
        assert!(!entries[0].is_starred);
    }

    #[test]
    fn update_dictionary_favorite_rejects_unknown_entry() {
        let context = TestRepositoryContext::new();
        let error = context
            .repository
            .update_dictionary_favorite("entry-unknown", true)
            .unwrap_err();
        assert!(error.to_string().contains("dictionary entry not found"));
    }

    #[test]
    fn delete_dictionary_entry_removes_entry() {
        let context = TestRepositoryContext::new();
        context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        let deleted = context
            .repository
            .delete_dictionary_entry("entry-article-001-generated-ai")
            .unwrap();
        assert_eq!(deleted, "entry-article-001-generated-ai");

        let entries = context
            .repository
            .list_dictionary_entries(None, None, false)
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn delete_dictionary_entry_rejects_unknown_entry() {
        let context = TestRepositoryContext::new();
        let error = context
            .repository
            .delete_dictionary_entry("entry-unknown")
            .unwrap_err();
        assert!(error.to_string().contains("dictionary entry not found"));
    }
}
