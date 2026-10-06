use crate::domain::article::{
    ArchiveRetirementSummaryDto, ArchiveSummaryDto, ArticleDetailDto, ArticleHistoryItemDto,
    ArticleReadState, ArticleSummaryDto, FavoriteUpdateResult, GetArticleDetailParams,
    GetRecommendedArticlesParams, ListArticleHistoryParams, RestoreArchivedArticleParams,
    RestoreArchivedArticleResult, UpdateArticleFavoriteParams,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;

#[derive(Debug, Clone)]
pub struct ArticleService {
    repository: ArticleRepository,
}

impl ArticleService {
    pub fn new(repository: ArticleRepository) -> Self {
        Self { repository }
    }

    pub fn get_recommended_articles(
        &self,
        params: GetRecommendedArticlesParams,
    ) -> Result<Vec<ArticleSummaryDto>, AppError> {
        let limit = params.normalized_limit()?;
        self.repository.list_recommended(limit)
    }

    pub fn list_article_history(
        &self,
        params: ListArticleHistoryParams,
    ) -> Result<Vec<ArticleHistoryItemDto>, AppError> {
        let limit = params.normalized_limit()?;
        let filter = params.normalized_filter();
        self.repository.list_history(filter, limit)
    }

    pub fn get_article_detail(
        &self,
        params: GetArticleDetailParams,
    ) -> Result<ArticleDetailDto, AppError> {
        let article_id = params.validated_article_id()?;
        let detail = self.repository.get_article_detail(&article_id)?;
        // 詳細設計書 §10.3: 詳細表示に成功した記事を DetailViewed へ進める。
        // 既読保存は付随処理なので、失敗しても詳細表示自体は成功させる。
        self.advance_read_state_or_log(&article_id, ArticleReadState::DetailViewed);
        Ok(detail)
    }

    /// ゆうこ軽量プレビュー表示（初回クリック）時に、紹介記事を Previewed へ進める（詳細設計書 §11.2）。
    /// プレビュー表示を止めないよう、失敗はログのみとする。
    pub fn mark_article_previewed(&self, article_id: &str) {
        self.advance_read_state_or_log(article_id, ArticleReadState::Previewed);
    }

    /// 既読状態を前進方向にだけ保存し、失敗時は調査用ログだけ残す。
    /// ログには記事IDとエラー種別・理由のみを出し、本文は出さない。
    fn advance_read_state_or_log(&self, article_id: &str, target: ArticleReadState) {
        if let Err(error) = self
            .repository
            .advance_article_read_state(article_id, target)
        {
            log::warn!("Failed to persist article read state for {article_id}: {error}");
        }
    }

    pub fn update_article_favorite(
        &self,
        params: UpdateArticleFavoriteParams,
    ) -> Result<FavoriteUpdateResult, AppError> {
        let (article_id, is_favorite) = params.validated_inputs()?;
        self.repository
            .update_article_favorite(&article_id, is_favorite)
    }

    /// アーカイブ退避候補を返す（read-only）。判定の基準時刻は現在UTC。
    /// 実ZIP圧縮は後続のため、ここでは候補の列挙のみを行う。
    pub fn list_archive_candidates(&self) -> Result<Vec<ArticleHistoryItemDto>, AppError> {
        self.repository.list_archive_candidates(chrono::Utc::now())
    }

    /// 退避候補を月次ZIPへ圧縮し、archived 印を付ける（増分1・非破壊／元.mdは保持）。
    /// 判定の基準時刻は現在UTC。結果サマリーを返す。
    pub fn archive_candidates(&self) -> Result<ArchiveSummaryDto, AppError> {
        self.repository.archive_candidates(chrono::Utc::now())
    }

    /// アーカイブから指定記事だけを安全に復元する。パスの解決とZIP検証はrepository側で行う。
    pub fn restore_archived_article(
        &self,
        params: RestoreArchivedArticleParams,
    ) -> Result<RestoreArchivedArticleResult, AppError> {
        let article_id = params.validated_article_id()?;
        self.repository.restore_archived_article(&article_id)
    }

    /// 完全な月次ZIPと記事カタログで検証できたarchived Markdownだけを退避付きで削除する。
    pub fn retire_archived_markdown(&self) -> Result<ArchiveRetirementSummaryDto, AppError> {
        self.repository.retire_archived_markdown()
    }
}

#[cfg(test)]
mod tests {
    //! サービス層テスト: get_article_detail が保存した既読状態を、履歴の未読フィルタと
    //! おすすめ一覧（ゆうこの候補選定が参照する readState）が実ファイル経由で読み取ることを確認する。
    use super::*;
    use crate::domain::article::ArticleHistoryFilter;
    use crate::paths::AppPaths;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct ServiceContext {
        service: ArticleService,
        news_dir: PathBuf,
        root: PathBuf,
    }

    impl Drop for ServiceContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn make_context() -> ServiceContext {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "article-service-tests-{}-{}",
            std::process::id(),
            n
        ));
        let paths = AppPaths::new(root.clone());
        paths.ensure_storage_dirs().expect("create storage dirs");
        let repository = ArticleRepository::new(&paths);
        repository
            .initialize_default_if_missing()
            .expect("seed articles");
        ServiceContext {
            service: ArticleService::new(repository),
            news_dir: paths.article_news_dir.clone(),
            root,
        }
    }

    fn detail_params(article_id: &str) -> GetArticleDetailParams {
        GetArticleDetailParams {
            article_id: article_id.to_string(),
        }
    }

    fn history_ids(ctx: &ServiceContext, filter: ArticleHistoryFilter) -> Vec<String> {
        ctx.service
            .list_article_history(ListArticleHistoryParams {
                filter: Some(filter),
                ..ListArticleHistoryParams::default()
            })
            .unwrap()
            .into_iter()
            .map(|article| article.article_id)
            .collect()
    }

    fn recommended_read_state(ctx: &ServiceContext, article_id: &str) -> ArticleReadState {
        ctx.service
            .get_recommended_articles(GetRecommendedArticlesParams::default())
            .unwrap()
            .into_iter()
            .find(|article| article.article_id == article_id)
            .unwrap()
            .read_state
    }

    #[test]
    fn get_article_detail_marks_detail_viewed_for_history_and_recommendation() {
        let ctx = make_context();
        assert!(
            history_ids(&ctx, ArticleHistoryFilter::Unread).contains(&"article-001".to_string())
        );

        ctx.service
            .get_article_detail(detail_params("article-001"))
            .unwrap();

        // 未読フィルタから外れ、既読フィルタへ移る。
        assert!(
            !history_ids(&ctx, ArticleHistoryFilter::Unread).contains(&"article-001".to_string())
        );
        assert!(history_ids(&ctx, ArticleHistoryFilter::Read).contains(&"article-001".to_string()));
        // おすすめ一覧の readState にも反映される（ゆうこの候補選定は未読を優先する）。
        assert_eq!(
            recommended_read_state(&ctx, "article-001"),
            ArticleReadState::DetailViewed
        );
    }

    #[test]
    fn mark_article_previewed_does_not_regress_detail_viewed() {
        let ctx = make_context();
        ctx.service
            .get_article_detail(detail_params("article-002"))
            .unwrap();

        ctx.service.mark_article_previewed("article-002");

        assert_eq!(
            recommended_read_state(&ctx, "article-002"),
            ArticleReadState::DetailViewed
        );
    }

    #[test]
    fn get_article_detail_succeeds_even_when_read_state_save_fails() {
        let ctx = make_context();
        // 一時ファイルの位置にディレクトリを置き、保存（atomic_write）だけを失敗させる。
        let blocker = ctx.news_dir.join("202606").join("article-001.md.tmp");
        std::fs::create_dir_all(&blocker).unwrap();

        let detail = ctx
            .service
            .get_article_detail(detail_params("article-001"))
            .unwrap();

        assert_eq!(detail.article_id, "article-001");
        assert_eq!(
            recommended_read_state(&ctx, "article-001"),
            ArticleReadState::Unread
        );
    }
}
