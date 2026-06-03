use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::article::{
    ArticleDetailDto, ArticleReadState, ArticleSummaryDto, FavoriteUpdateResult,
};
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct ArticleRepository {
    article_favorites_path: PathBuf,
}

impl ArticleRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            article_favorites_path: paths.article_favorites_path.clone(),
        }
    }

    #[cfg(test)]
    fn with_path(article_favorites_path: PathBuf) -> Self {
        Self {
            article_favorites_path,
        }
    }

    pub fn list_recommended(&self, limit: usize) -> Result<Vec<ArticleSummaryDto>, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        Ok(sample_article_records()
            .into_iter()
            .map(|record| record.to_summary_dto(favorite_store.contains(record.article_id)))
            .take(limit)
            .collect())
    }

    pub fn get_article_detail(&self, article_id: &str) -> Result<ArticleDetailDto, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        sample_article_records()
            .into_iter()
            .find(|record| record.article_id == article_id)
            .map(|record| record.to_detail_dto(favorite_store.contains(record.article_id)))
            .ok_or_else(|| AppError::NotFound(format!("article not found: {article_id}")))
    }

    pub fn update_article_favorite(
        &self,
        article_id: &str,
        is_favorite: bool,
    ) -> Result<FavoriteUpdateResult, AppError> {
        let article_exists = sample_article_records()
            .iter()
            .any(|record| record.article_id == article_id);
        if !article_exists {
            return Err(AppError::NotFound(format!(
                "article not found: {article_id}"
            )));
        }

        let mut favorite_store = self.load_favorite_store_or_default()?;
        favorite_store.set(article_id, is_favorite);
        self.save_favorite_store(&favorite_store)?;

        Ok(FavoriteUpdateResult {
            article_id: article_id.to_string(),
            is_favorite,
        })
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

        let temp_path = self.article_favorites_path.with_extension("json.tmp");
        let backup_path = self.article_favorites_path.with_extension("json.bak");
        let payload = serde_json::to_vec_pretty(store)?;
        std::fs::write(&temp_path, payload)?;

        let had_existing = self.article_favorites_path.exists();
        if had_existing {
            if backup_path.exists() {
                std::fs::remove_file(&backup_path)?;
            }
            std::fs::rename(&self.article_favorites_path, &backup_path)?;
        }

        match std::fs::rename(&temp_path, &self.article_favorites_path) {
            Ok(()) => {
                if had_existing && backup_path.exists() {
                    if let Err(error) = std::fs::remove_file(&backup_path) {
                        log::warn!("Failed to remove article favorites backup: {error}");
                    }
                }
                Ok(())
            }
            Err(error) => {
                log::error!("Failed to promote temporary article favorites file: {error}");

                if had_existing && backup_path.exists() {
                    if let Err(restore_error) =
                        std::fs::rename(&backup_path, &self.article_favorites_path)
                    {
                        log::error!("Failed to restore article favorites backup: {restore_error}");
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
struct SampleArticleRecord {
    article_id: &'static str,
    title: &'static str,
    source_name: &'static str,
    original_url: &'static str,
    published_at_text: &'static str,
    genre: &'static str,
    summary: &'static str,
    yuuko_explanation: &'static str,
    focus_points: [&'static str; 3],
    yuuko_comment: &'static str,
    keyword_candidates: [&'static str; 3],
    read_state: ArticleReadState,
    recommendation_score: f32,
}

impl SampleArticleRecord {
    fn to_summary_dto(&self, is_favorite: bool) -> ArticleSummaryDto {
        ArticleSummaryDto {
            article_id: self.article_id.to_string(),
            title: self.title.to_string(),
            source_name: self.source_name.to_string(),
            published_at_text: self.published_at_text.to_string(),
            genre: self.genre.to_string(),
            summary: Some(self.summary.to_string()),
            is_favorite,
            read_state: self.read_state.clone(),
            recommendation_score: self.recommendation_score,
        }
    }

    fn to_detail_dto(&self, is_favorite: bool) -> ArticleDetailDto {
        ArticleDetailDto {
            article_id: self.article_id.to_string(),
            title: self.title.to_string(),
            source_name: self.source_name.to_string(),
            original_url: self.original_url.to_string(),
            published_at_text: self.published_at_text.to_string(),
            genre: self.genre.to_string(),
            summary: Some(self.summary.to_string()),
            yuuko_explanation: Some(self.yuuko_explanation.to_string()),
            focus_points: self
                .focus_points
                .iter()
                .map(|item| item.to_string())
                .collect(),
            yuuko_comment: Some(self.yuuko_comment.to_string()),
            is_favorite,
            keyword_candidates: self
                .keyword_candidates
                .iter()
                .map(|item| item.to_string())
                .collect(),
        }
    }
}

fn sample_article_records() -> Vec<SampleArticleRecord> {
    vec![
        SampleArticleRecord {
            article_id: "article-001",
            title: "生成AIスタートアップの資金調達が再加速",
            source_name: "TechCrunch Japan",
            original_url: "https://example.com/articles/article-001",
            published_at_text: "5分前",
            genre: "AI・テクノロジー",
            summary:
                "生成AIを活用するスタートアップへの投資が再び活発化し、業務支援や自動化領域の案件に注目が集まっています。",
            yuuko_explanation:
                "この記事は、生成AIそのものよりも『どんな仕事に役立てられているか』を見ると理解しやすいです。企業が導入効果を数字で示せるかが評価の分かれ目になっています。",
            focus_points: [
                "投資対象が研究寄りから業務課題の解決寄りへ移っている",
                "導入効果を定量化できるサービスが評価されやすい",
                "既存業務フローへ自然に組み込める点が差別化要因になっている",
            ],
            yuuko_comment:
                "AIそのものの新しさより、使ったあとに何が楽になるかが大事そうですね。",
            keyword_candidates: ["生成AI", "資金調達", "業務自動化"],
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.92,
        },
        SampleArticleRecord {
            article_id: "article-002",
            title: "国内SaaS企業、業務改善支援の新施策を発表",
            source_name: "日経ビジネス",
            original_url: "https://example.com/articles/article-002",
            published_at_text: "1時間前",
            genre: "ビジネス",
            summary:
                "国内SaaS企業が中堅企業向けの業務改善プログラムを発表し、導入支援と教育体制をセットで提供する方針を示しました。",
            yuuko_explanation:
                "製品を売るだけでなく、導入後の運用まで支援する流れが強まっています。特に現場定着の支援があるかどうかは、導入成功率に直結します。",
            focus_points: [
                "導入支援と社内教育を一体で提供している",
                "中堅企業の現場定着を重視した設計になっている",
                "単発導入ではなく継続改善を前提にしている",
            ],
            yuuko_comment:
                "仕組みを入れるだけではなく、使い続けられるかまで考えているのがポイントですね。",
            keyword_candidates: ["SaaS", "業務改善", "導入支援"],
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.84,
        },
        SampleArticleRecord {
            article_id: "article-003",
            title: "量子コンピュータ研究で新たな誤り訂正手法",
            source_name: "ITmedia NEWS",
            original_url: "https://example.com/articles/article-003",
            published_at_text: "2時間前",
            genre: "テクノロジー",
            summary:
                "量子コンピュータの安定運用に向けて、従来より少ない負荷で誤りを検知・補正できる新手法が報告されました。",
            yuuko_explanation:
                "量子コンピュータは計算能力だけでなく、誤差に弱い点が課題です。今回の話は『速さ』より『正確さを保つ工夫』に注目すると読みやすいです。",
            focus_points: [
                "誤り訂正の計算コスト削減が主題",
                "安定運用への実用面で前進があった",
                "研究成果は今後の実装方式に影響する可能性がある",
            ],
            yuuko_comment:
                "難しそうに見えても、安定して動かすための工夫だと思うと掴みやすいですね。",
            keyword_candidates: ["量子コンピュータ", "誤り訂正", "研究成果"],
            read_state: ArticleReadState::Previewed,
            recommendation_score: 0.79,
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::ArticleRepository;

    struct TestRepositoryContext {
        repository: ArticleRepository,
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
            std::fs::create_dir_all(&root_dir).unwrap();
            let favorites_path = root_dir.join("favorites").join("article_favorites.json");
            let repository = ArticleRepository::with_path(favorites_path);
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

    #[test]
    fn list_recommended_respects_limit() {
        let context = TestRepositoryContext::new();
        let articles = context.repository.list_recommended(2).unwrap();
        assert_eq!(articles.len(), 2);
    }

    #[test]
    fn list_recommended_keeps_recommendation_order() {
        let context = TestRepositoryContext::new();
        let articles = context.repository.list_recommended(3).unwrap();
        assert!(articles[0].recommendation_score >= articles[1].recommendation_score);
        assert!(articles[1].recommendation_score >= articles[2].recommendation_score);
    }

    #[test]
    fn get_article_detail_returns_matching_article() {
        let context = TestRepositoryContext::new();
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
    fn update_article_favorite_removes_existing_favorite() {
        let context = TestRepositoryContext::new();
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
}
