use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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
    fn with_path(dictionary_path: PathBuf) -> Self {
        Self { dictionary_path }
    }

    pub fn explain_selected_term(
        &self,
        article_id: &str,
        selected_text: &str,
    ) -> Result<DictionaryEntryDto, AppError> {
        let article_title = article_title_for(article_id)
            .ok_or_else(|| AppError::NotFound(format!("article not found: {article_id}")))?;
        let normalized_text = normalize_text(selected_text);

        if let Some(saved_entry) = self.find_saved_entry(article_id, &normalized_text)? {
            return Ok(saved_entry);
        }

        let entry = sample_dictionary_entries()
            .into_iter()
            .find(|entry| {
                entry.article_id == article_id && normalize_text(entry.key_text) == normalized_text
            })
            .map(|entry| entry.to_dto())
            .unwrap_or_else(|| build_generic_entry(article_id, article_title, selected_text));

        Ok(entry)
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

    fn find_saved_entry(
        &self,
        article_id: &str,
        normalized_text: &str,
    ) -> Result<Option<DictionaryEntryDto>, AppError> {
        let store = self.load_store_or_default()?;
        Ok(store
            .entries
            .into_iter()
            .find(|entry| {
                entry.normalized_text == normalized_text
                    && entry
                        .source_article_ids
                        .iter()
                        .any(|source_article_id| source_article_id == article_id)
            })
            .map(|entry| entry.to_dto()))
    }

    fn load_store_or_default(&self) -> Result<PersistedDictionaryStore, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.dictionary_path.exists() {
            return Ok(PersistedDictionaryStore::with_current_version());
        }

        let raw = std::fs::read_to_string(&self.dictionary_path)?;
        let mut store = serde_json::from_str::<PersistedDictionaryStore>(&raw)?;
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

#[derive(Debug, Clone)]
struct SampleDictionaryEntry {
    article_id: &'static str,
    article_title: &'static str,
    key_text: &'static str,
    entry_type: DictionaryEntryType,
    short_explanation: &'static str,
    detail_explanation: &'static str,
}

impl SampleDictionaryEntry {
    fn to_dto(&self) -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: build_entry_id(self.article_id, self.key_text),
            key_text: self.key_text.to_string(),
            entry_type: self.entry_type.clone(),
            short_explanation: self.short_explanation.to_string(),
            detail_explanation: self.detail_explanation.to_string(),
            related_article_id: Some(self.article_id.to_string()),
            related_article_title: Some(self.article_title.to_string()),
            is_starred: false,
        }
    }
}

fn build_entry_id(article_id: &str, selected_text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    article_id.hash(&mut hasher);
    normalize_text(selected_text).hash(&mut hasher);
    format!("entry-{}-{:x}", article_id, hasher.finish())
}

fn build_generic_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
) -> DictionaryEntryDto {
    DictionaryEntryDto {
        entry_id: build_entry_id(article_id, selected_text),
        key_text: selected_text.to_string(),
        entry_type: DictionaryEntryType::Term,
        short_explanation: format!(
            "「{selected_text}」はこの記事を理解するための補助キーワードです。"
        ),
        detail_explanation: format!(
            "「{selected_text}」は記事「{article_title}」の文脈で重要な用語です。現時点では記事理解のヒントになる簡易解説として返しています。"
        ),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
    }
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

fn article_title_for(article_id: &str) -> Option<&'static str> {
    sample_dictionary_entries()
        .iter()
        .find(|entry| entry.article_id == article_id)
        .map(|entry| entry.article_title)
}

fn sample_dictionary_entries() -> Vec<SampleDictionaryEntry> {
    vec![
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "生成AI",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "文章や画像などを自動生成する AI 全般を指す言葉です。",
            detail_explanation: "生成AIは、入力された指示に応じて文章・画像・音声などを自動生成する技術群です。この記事では、生成AIそのものの新規性よりも、業務課題の解決にどう結びついているかが注目点になっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "資金調達",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "企業が事業拡大のために投資や融資で資金を集めることです。",
            detail_explanation: "資金調達は、企業が新しい開発や採用、営業活動を進めるために必要なお金を外部から集めることです。この記事では、生成AI関連企業に再び投資が集まり始めている流れを示しています。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "業務自動化",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "定型業務を仕組み化して人手を減らす考え方です。",
            detail_explanation: "業務自動化は、繰り返し作業や定型処理をシステム化して効率を上げる取り組みです。生成AIが評価されやすい背景には、この自動化効果を現場で示しやすいことがあります。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "SaaS",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "インターネット経由で利用するソフトウェア提供形態です。",
            detail_explanation: "SaaS は Software as a Service の略で、クラウド上で提供されるソフトウェアを必要なときに利用する形態です。この記事では、機能そのものに加えて導入後の支援体制が差別化要因として扱われています。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "導入支援",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "サービスを使い始める際の設定や定着を支える取り組みです。",
            detail_explanation: "導入支援は、ツールの初期設定だけでなく、使い方の教育や運用への定着まで含めて支援することです。この記事では、導入後の継続活用まで見据えた支援が価値として語られています。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "業務改善",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "仕事の流れを見直して効率や成果を高めることです。",
            detail_explanation: "業務改善は、現場の手間や無駄を減らしながら成果を上げるための取り組みです。この記事では、単なるツール導入ではなく、改善が定着する運用設計までが主題になっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "量子コンピュータ",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "量子力学の性質を利用して計算する新しい計算機です。",
            detail_explanation: "量子コンピュータは、通常のコンピュータとは異なる量子の性質を使って計算する技術です。この記事では高速化よりも、安定して正確に動かすための仕組みに焦点が当たっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "誤り訂正",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "計算中の誤差を検知し、補正するための仕組みです。",
            detail_explanation: "誤り訂正は、計算途中で起こるノイズや誤差を見つけて結果を安定させる考え方です。量子コンピュータでは特に重要で、実用化に向けた大きな課題の一つです。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "研究成果",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "研究や実験によって得られた新しい知見や結果です。",
            detail_explanation: "研究成果は、学術研究や実験を通じて得られた知見のことです。この記事では、新たな誤り訂正手法が今後の実装方式に影響を与える可能性がある点が重要です。",
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::dictionary::{DictionaryEntryDto, DictionaryEntryType};

    use super::DictionaryRepository;

    struct TestRepositoryContext {
        repository: DictionaryRepository,
        root_dir: PathBuf,
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
            let repository = DictionaryRepository::with_path(dictionary_path);
            Self {
                repository,
                root_dir,
            }
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
    fn explain_selected_term_returns_exact_match() {
        let context = TestRepositoryContext::new();
        let entry = context
            .repository
            .explain_selected_term("article-001", "生成AI")
            .unwrap();

        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert!(!entry.is_starred);
    }

    #[test]
    fn explain_selected_term_matches_ascii_case_insensitively() {
        let context = TestRepositoryContext::new();
        let entry = context
            .repository
            .explain_selected_term("article-002", "saas")
            .unwrap();

        assert_eq!(entry.key_text, "SaaS");
    }

    #[test]
    fn explain_selected_term_falls_back_for_unknown_term() {
        let context = TestRepositoryContext::new();
        let entry = context
            .repository
            .explain_selected_term("article-001", "評価指標")
            .unwrap();

        assert_eq!(entry.key_text, "評価指標");
        assert!(entry.detail_explanation.contains("記事"));
    }

    #[test]
    fn explain_selected_term_rejects_unknown_article() {
        let context = TestRepositoryContext::new();
        let error = context
            .repository
            .explain_selected_term("article-999", "生成AI")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "not found: article not found: article-999"
        );
    }

    #[test]
    fn save_dictionary_entry_persists_and_returns_starred_entry() {
        let context = TestRepositoryContext::new();
        let saved = context
            .repository
            .save_dictionary_entry(saved_entry())
            .unwrap();

        assert!(saved.is_starred);
        assert_eq!(saved.key_text, "生成AI");

        let explained = context
            .repository
            .explain_selected_term("article-001", "生成AI")
            .unwrap();
        assert!(explained.is_starred);
        assert_eq!(explained.short_explanation, "保存済みの短い説明");
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
