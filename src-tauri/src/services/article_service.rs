use crate::domain::article::{
    ArchiveMonthArticlesDto, ArchiveMonthDeleteParams, ArchiveMonthDeletePreviewDto,
    ArchiveMonthDeleteResultDto, ArchiveMonthDto, ArchiveRetirementSummaryDto, ArchiveSummaryDto,
    ArticleDetailDto, ArticleHistoryItemDto, ArticleReadState, ArticleSummaryDto,
    FavoriteUpdateResult, GetArticleDetailParams, GetRecommendedArticlesParams,
    ListArchiveMonthArticlesParams, ListArticleHistoryParams, OpenOriginalArticleParams,
    RestoreArchivedArticleParams, RestoreArchivedArticleResult, SummaryState,
    UpdateArticleFavoriteParams,
};
use crate::error::{AppError, OpenOriginalArticleError};
use crate::infra::external_browser::{self, BrowserOpenError};
use crate::repositories::article_repository::ArticleRepository;
use crate::services::gacha_service::{log_grant_failure, GachaService};
use crate::services::recommendation_service::RecommendationService;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct ArticleService {
    repository: ArticleRepository,
    /// 記事を読んだときの流れ星のかけら付与先（データ設計書 §12.4）。未設定なら付与しない。
    gacha_service: Option<GachaService>,
}

impl ArticleService {
    pub fn new(repository: ArticleRepository) -> Self {
        Self {
            repository,
            gacha_service: None,
        }
    }

    /// 記事を読んだときにかけらを付与するガチャサービスを設定する（アプリ起動時の組み立て用）。
    pub fn with_gacha_service(mut self, gacha_service: GachaService) -> Self {
        self.gacha_service = Some(gacha_service);
        self
    }

    /// おすすめ一覧を返す。基準時刻は現在UTC（要件定義書 §7.4A.6 / §7.4A.8）。
    pub fn get_recommended_articles(
        &self,
        params: GetRecommendedArticlesParams,
    ) -> Result<Vec<ArticleSummaryDto>, AppError> {
        self.get_recommended_articles_at(params, Utc::now())
    }

    /// `get_recommended_articles` の本体（基準時刻注入版。テストで時刻を固定するため分離）。
    ///
    /// 保存スコアは取得時点の鮮度・未読前提で固定されているため、そのまま並べると
    /// 古い記事が上位に残り続け、既読記事も下がらない。取得のたびに現在時刻の鮮度係数と
    /// 現在の readState 係数で再計算し、そのスコア順で返す（既読記事は除外せず順位を下げるだけ）。
    /// 返却する `recommendation_score` も並び順と一致させるため再計算後の値にする。
    fn get_recommended_articles_at(
        &self,
        params: GetRecommendedArticlesParams,
        now: DateTime<Utc>,
    ) -> Result<Vec<ArticleSummaryDto>, AppError> {
        let limit = params.normalized_limit()?;
        Ok(self
            .rank_by_recommendation_at(now)?
            .into_iter()
            .take(limit)
            .collect())
    }

    /// 未要約の記事IDを、おすすめ一覧と同じ並び順で返す（自動要約キューの投入順）。
    /// 並び順をおすすめ一覧とそろえるため、同じ再計算・同じ同点処理を使う。
    pub fn list_unsummarized_article_ids(&self) -> Result<Vec<String>, AppError> {
        self.list_unsummarized_article_ids_at(Utc::now())
    }

    fn list_unsummarized_article_ids_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<String>, AppError> {
        Ok(self
            .rank_by_recommendation_at(now)?
            .into_iter()
            .filter(|summary| summary.summary_state != SummaryState::Done)
            .map(|summary| summary.article_id)
            .collect())
    }

    /// 記事ファイルで要約済み（status.summarized）かを返す。既読状態は進めない（読み取りのみ）。
    pub fn is_article_summarized(&self, article_id: &str) -> Result<bool, AppError> {
        self.repository.is_article_summarized(article_id)
    }

    /// 全候補をおすすめ順（現在時刻で再計算したスコア順）に並べて返す。
    fn rank_by_recommendation_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<ArticleSummaryDto>, AppError> {
        let recommendation_service = RecommendationService::new();
        let mut ranked = self
            .repository
            .list_recommendation_candidates()?
            .into_iter()
            .map(|candidate| {
                let mut summary = candidate.summary;
                summary.recommendation_score = recommendation_service.rescore_stored(
                    summary.recommendation_score,
                    &summary.read_state,
                    age_hours_since(&candidate.fetched_at, now),
                );
                (summary, candidate.fetched_at)
            })
            .collect::<Vec<_>>();
        // 同点時は従来どおり取得日時の新しい順 → 記事ID順で並びを安定させる。
        ranked.sort_by(|(left, left_fetched_at), (right, right_fetched_at)| {
            right
                .recommendation_score
                .partial_cmp(&left.recommendation_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right_fetched_at.cmp(left_fetched_at))
                .then_with(|| left.article_id.cmp(&right.article_id))
        });

        Ok(ranked.into_iter().map(|(summary, _)| summary).collect())
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
        if self.advance_read_state_or_log(&article_id, ArticleReadState::DetailViewed) {
            // 初めて詳細を開いた（既読になった）ときだけ、その日の既読数に数える（データ設計書 §12.3）。
            // DetailViewed は一方向の最終状態なので、同じ記事を二重に数えることはない。
            // 記事側のロックは advance の中で手放し済み。付与の失敗は詳細表示を止めない。
            if let Some(gacha_service) = &self.gacha_service {
                log_grant_failure("news_read", gacha_service.grant_for_news_read());
            }
        }
        Ok(detail)
    }

    /// 保存済みの AI 要約だけを返す（本文抜粋で補わない）。ゆうこのデスクトップ通知用。
    /// get_article_detail と違い、既読状態は進めない（通知を出しただけで既読にしない）。
    pub fn get_saved_summary(&self, article_id: &str) -> Result<Option<String>, AppError> {
        self.repository.get_saved_summary(article_id)
    }

    /// ゆうこ軽量プレビュー表示（初回クリック）時に、紹介記事を Previewed へ進める（詳細設計書 §11.2）。
    /// プレビュー表示を止めないよう、失敗はログのみとする。
    /// ゆうこの通知への反応なので、かけらは付与しない（D74。既読数にも数えない）。
    pub fn mark_article_previewed(&self, article_id: &str) {
        self.advance_read_state_or_log(article_id, ArticleReadState::Previewed);
    }

    /// 既読状態を前進方向にだけ保存し、失敗時は調査用ログだけ残す。
    /// 実際に状態を進めて保存できたときだけ true を返す（後退・同じ状態・失敗は false）。
    /// ログには記事IDとエラー種別・理由のみを出し、本文は出さない。
    fn advance_read_state_or_log(&self, article_id: &str, target: ArticleReadState) -> bool {
        match self
            .repository
            .advance_article_read_state(article_id, target)
        {
            Ok(advanced) => advanced,
            Err(error) => {
                log::warn!("Failed to persist article read state for {article_id}: {error}");
                false
            }
        }
    }

    /// 記事IDから保存済みの元記事 URL を引き、検証できたものだけを `opener` で開く（判断台帳 D13）。
    ///
    /// 実際の起動処理（OS 連携）は `opener` として受け取り、テストでは起動せずに検証だけ確認する。
    /// ログには記事IDと失敗種別だけを出し、URL は出さない。既読状態は進めない。
    pub fn open_original_article_with(
        &self,
        params: OpenOriginalArticleParams,
        opener: impl FnOnce(&url::Url) -> Result<(), BrowserOpenError>,
    ) -> Result<(), OpenOriginalArticleError> {
        let article_id = params.validated_article_id()?;
        let raw_url = self.repository.get_original_url(&article_id)?;
        let url = external_browser::validate_openable_url(&raw_url).map_err(|rejection| {
            log::warn!("Refused to open original article {article_id}: {rejection:?}");
            OpenOriginalArticleError::UrlRejected
        })?;
        opener(&url).map_err(|error| {
            log::warn!("Failed to open original article {article_id}: {error:?}");
            match error {
                BrowserOpenError::Unsupported => OpenOriginalArticleError::Unsupported,
                BrowserOpenError::LaunchFailed => OpenOriginalArticleError::LaunchFailed,
            }
        })
    }

    /// 元記事を既定のブラウザで開く（本番用。OS 連携は infra::external_browser）。
    pub fn open_original_article(
        &self,
        params: OpenOriginalArticleParams,
    ) -> Result<(), OpenOriginalArticleError> {
        self.open_original_article_with(params, external_browser::open_in_default_browser)
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

    /// 過去ニュース画面の月別アーカイブ一覧（新しい月から）を返す。
    pub fn list_archive_months(&self) -> Result<Vec<ArchiveMonthDto>, AppError> {
        self.repository.list_archive_months(chrono::Utc::now())
    }

    /// 指定月のアーカイブ記事一覧を記事カタログから返す。年月はここで形式を検証する。
    pub fn list_archive_month_articles(
        &self,
        params: ListArchiveMonthArticlesParams,
    ) -> Result<ArchiveMonthArticlesDto, AppError> {
        let month = params.validated_month()?;
        self.repository.list_archive_month_articles(&month)
    }

    /// 古い月のアーカイブ削除の事前確認（件数・ZIPサイズ）。年月はここで形式を検証する。
    pub fn get_archive_month_delete_preview(
        &self,
        params: ArchiveMonthDeleteParams,
    ) -> Result<ArchiveMonthDeletePreviewDto, AppError> {
        let month = params.validated_month()?;
        self.repository
            .get_archive_month_delete_preview(&month, chrono::Utc::now())
    }

    /// 古い月の月次ZIPと index の月エントリを削除する（判断台帳 D26）。ローカルのMarkdownは消さない。
    pub fn delete_archive_month(
        &self,
        params: ArchiveMonthDeleteParams,
    ) -> Result<ArchiveMonthDeleteResultDto, AppError> {
        let month = params.validated_month()?;
        self.repository
            .delete_archive_month(&month, chrono::Utc::now())
    }

    /// 完全な月次ZIPと記事カタログで検証できたarchived Markdownだけを退避付きで削除する。
    pub fn retire_archived_markdown(&self) -> Result<ArchiveRetirementSummaryDto, AppError> {
        self.repository.retire_archived_markdown()
    }
}

/// 取得日時（RFC3339）から基準時刻までの経過時間（時間単位）を求める。
/// 鮮度はおすすめ判定ポリシー §2 のとおり「取得から」の経過で測る（公開日時は表示用文字列のため使わない）。
/// 解釈できない取得日時は `None` とし、鮮度加点なし（安全側）で扱う。
fn age_hours_since(fetched_at: &str, now: DateTime<Utc>) -> Option<f64> {
    let fetched_at = DateTime::parse_from_rfc3339(fetched_at.trim()).ok()?;
    let elapsed = now.signed_duration_since(fetched_at.with_timezone(&Utc));
    Some(elapsed.num_seconds() as f64 / 3600.0)
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

    /// おすすめ再計算テストの基準時刻（シード記事 2026-06-04 取得からは鮮度窓を大きく超える）。
    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-06T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// 取得時と同じく未読で保存スコアを持つ記事を、基準時刻の `hours_ago` 時間前に取得したものとして保存する。
    fn save_fetched(ctx: &ServiceContext, article_id: &str, stored_score: f32, hours_ago: i64) {
        let fetched_at = (fixed_now() - chrono::Duration::hours(hours_ago)).to_rfc3339();
        let saved = ctx
            .service
            .repository
            .save_fetched_articles(vec![crate::domain::article::FetchedArticle {
                article_id: article_id.to_string(),
                title: format!("テスト記事 {article_id}"),
                source_name: "テストソース".to_string(),
                original_url: format!("https://example.com/{article_id}"),
                fetched_at: fetched_at.clone(),
                published_at_text: fetched_at,
                genre: "テクノロジー".to_string(),
                tags: Vec::new(),
                excerpt: Some("抜粋".to_string()),
                recommendation_score: stored_score,
                read_state: ArticleReadState::Unread,
            }])
            .unwrap();
        assert_eq!(saved, 1);
    }

    fn recommended_at(ctx: &ServiceContext, limit: Option<u32>) -> Vec<ArticleSummaryDto> {
        ctx.service
            .get_recommended_articles_at(GetRecommendedArticlesParams { limit }, fixed_now())
            .unwrap()
    }

    fn ids(articles: &[ArticleSummaryDto]) -> Vec<&str> {
        articles
            .iter()
            .map(|article| article.article_id.as_str())
            .collect()
    }

    #[test]
    fn recommended_articles_drop_articles_far_beyond_freshness_window() {
        let ctx = make_context();
        // 保存スコアだけなら stale 系が上位だが、30日・10日前の取得で鮮度加点を失う。
        save_fetched(&ctx, "stale-30d", 0.97, 24 * 30);
        save_fetched(&ctx, "stale-10d", 0.96, 24 * 10);
        save_fetched(&ctx, "fresh-1h", 0.93, 1);
        save_fetched(&ctx, "fresh-2h", 0.92, 2);

        let articles = recommended_at(&ctx, None);
        assert_eq!(
            &ids(&articles)[..4],
            &["fresh-1h", "fresh-2h", "stale-30d", "stale-10d"]
        );
        // 返却スコアは再計算後の値で、並び順と一致する（降順）。
        assert!(articles
            .windows(2)
            .all(|pair| pair[0].recommendation_score >= pair[1].recommendation_score));
        // 件数制限は再計算後の順位に対して適用される。
        assert_eq!(
            ids(&recommended_at(&ctx, Some(2))),
            ["fresh-1h", "fresh-2h"]
        );
    }

    #[test]
    fn detail_viewed_article_is_ranked_lower_but_not_excluded() {
        let ctx = make_context();
        save_fetched(&ctx, "read-target", 0.95, 1);
        save_fetched(&ctx, "unread-mid", 0.90, 1);
        let before = recommended_at(&ctx, None);
        assert_eq!(&ids(&before)[..2], &["read-target", "unread-mid"]);

        ctx.service
            .get_article_detail(detail_params("read-target"))
            .unwrap();

        let after = recommended_at(&ctx, None);
        let position = |id: &str| after.iter().position(|a| a.article_id == id).unwrap();
        // 一覧から除外されず、未読の同等記事より下位になる。
        assert_eq!(after.len(), before.len());
        assert!(position("read-target") > position("unread-mid"));
        assert_eq!(
            after[position("read-target")].read_state,
            ArticleReadState::DetailViewed
        );
    }

    #[test]
    fn unsummarized_ids_follow_recommendation_order_and_skip_summarized_articles() {
        let ctx = make_context();
        save_fetched(&ctx, "fresh-1h", 0.93, 1);
        save_fetched(&ctx, "summarized", 0.95, 1);
        save_fetched(&ctx, "fresh-2h", 0.92, 2);
        ctx.service
            .repository
            .update_article_summary(
                "summarized",
                crate::domain::article::ArticleSummaryUpdate {
                    summary: "要約".to_string(),
                    yuuko_explanation: "再説明".to_string(),
                    key_points: Vec::new(),
                    focus_points: Vec::new(),
                    yuuko_comment: "感想".to_string(),
                    ai_provider: "mock".to_string(),
                    generated_at: "2026-10-06T12:00:00Z".to_string(),
                },
            )
            .unwrap();

        // 一覧 DTO には記事ファイル由来の要約状態が載る（保存済み=done、未要約=none）。
        let articles = recommended_at(&ctx, None);
        let state_of = |id: &str| {
            articles
                .iter()
                .find(|article| article.article_id == id)
                .unwrap()
                .summary_state
        };
        assert_eq!(state_of("summarized"), SummaryState::Done);
        assert_eq!(state_of("fresh-1h"), SummaryState::None);
        let detail = ctx
            .service
            .get_article_detail(detail_params("summarized"))
            .unwrap();
        assert_eq!(detail.summary_state, SummaryState::Done);

        // 自動要約の投入順は、おすすめ一覧から要約済みを除いた並びと一致する。
        let expected: Vec<String> = articles
            .iter()
            .filter(|article| article.summary_state != SummaryState::Done)
            .map(|article| article.article_id.clone())
            .collect();
        let queued = ctx
            .service
            .list_unsummarized_article_ids_at(fixed_now())
            .unwrap();
        assert_eq!(queued, expected);
        assert_eq!(&queued[..2], &["fresh-1h", "fresh-2h"]);

        // 自動要約の上書き防止チェックは読み取りのみで、既読状態を進めない。
        assert!(ctx.service.is_article_summarized("summarized").unwrap());
        assert!(!ctx.service.is_article_summarized("fresh-1h").unwrap());
        let after_check = recommended_at(&ctx, None);
        let fresh = after_check
            .iter()
            .find(|article| article.article_id == "fresh-1h")
            .unwrap();
        assert_eq!(fresh.read_state, ArticleReadState::Unread);
    }

    #[test]
    fn age_hours_since_handles_offsets_and_invalid_values() {
        let now = fixed_now();
        let age = age_hours_since("2026-10-06T18:00:00+09:00", now).unwrap();
        assert!((age - 3.0).abs() < 1e-9);
        assert!(age_hours_since("not-a-date", now).is_none());
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
    fn first_detail_view_counts_for_gacha_once_and_preview_does_not() {
        use crate::domain::gacha::{INITIAL_FRAGMENTS, NEWS_DAILY_GRANT};
        use crate::repositories::gacha_repository::GachaRepository;

        let ctx = make_context();
        let gacha_repository = GachaRepository::new(&AppPaths::new(ctx.root.clone()));
        let service = ctx.service.clone().with_gacha_service(
            GachaService::new(gacha_repository.clone())
                .with_today(std::sync::Arc::new(|| "2026-10-08".to_string())),
        );
        let read_count = || {
            gacha_repository
                .load()
                .unwrap()
                .map(|state| state.daily_grant.news_read_count)
                .unwrap_or(0)
        };

        // 同じ記事を何度開いても1件として数える。
        service
            .get_article_detail(detail_params("article-001"))
            .unwrap();
        service
            .get_article_detail(detail_params("article-001"))
            .unwrap();
        assert_eq!(read_count(), 1);

        // ゆうこの軽量プレビュー（通知への反応）では数えない（D74）。
        service.mark_article_previewed("article-002");
        assert_eq!(read_count(), 1);
        // プレビュー済みの記事も、詳細を開いて読んだら1件として数える。
        service
            .get_article_detail(detail_params("article-002"))
            .unwrap();
        assert_eq!(read_count(), 2);

        service
            .get_article_detail(detail_params("article-003"))
            .unwrap();
        let saved = gacha_repository.load().unwrap().unwrap();
        assert_eq!(saved.daily_grant.news_read_count, 3);
        assert!(saved.daily_grant.news_granted);
        assert_eq!(saved.star_fragments, INITIAL_FRAGMENTS + NEWS_DAILY_GRANT);
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

    /// 保存済みの元記事 URL を指定して記事を1件保存する（元記事を開く処理のテスト用）。
    fn save_with_original_url(ctx: &ServiceContext, article_id: &str, original_url: &str) {
        let fetched_at = fixed_now().to_rfc3339();
        let saved = ctx
            .service
            .repository
            .save_fetched_articles(vec![crate::domain::article::FetchedArticle {
                article_id: article_id.to_string(),
                title: format!("テスト記事 {article_id}"),
                source_name: "テストソース".to_string(),
                original_url: original_url.to_string(),
                fetched_at: fetched_at.clone(),
                published_at_text: fetched_at,
                genre: "テクノロジー".to_string(),
                tags: Vec::new(),
                excerpt: Some("抜粋".to_string()),
                recommendation_score: 0.5,
                read_state: ArticleReadState::Unread,
            }])
            .unwrap();
        assert_eq!(saved, 1);
    }

    fn open_with_recorder(
        ctx: &ServiceContext,
        article_id: &str,
        result: Result<(), BrowserOpenError>,
    ) -> (Result<(), OpenOriginalArticleError>, Option<String>) {
        let mut opened = None;
        let outcome = ctx.service.open_original_article_with(
            OpenOriginalArticleParams {
                article_id: article_id.to_string(),
            },
            |url| {
                opened = Some(url.as_str().to_string());
                result
            },
        );
        (outcome, opened)
    }

    #[test]
    fn open_original_article_opens_saved_https_url() {
        let ctx = make_context();
        save_with_original_url(&ctx, "open-ok", "https://example.com/news/1");

        let (outcome, opened) = open_with_recorder(&ctx, "open-ok", Ok(()));
        assert!(outcome.is_ok());
        assert_eq!(opened.as_deref(), Some("https://example.com/news/1"));
    }

    #[test]
    fn open_original_article_rejects_unknown_or_empty_id_without_opening() {
        let ctx = make_context();

        let (outcome, opened) = open_with_recorder(&ctx, "no-such-article", Ok(()));
        assert!(matches!(
            outcome,
            Err(OpenOriginalArticleError::Article(AppError::NotFound(_)))
        ));
        assert!(opened.is_none());

        let (outcome, opened) = open_with_recorder(&ctx, "   ", Ok(()));
        assert!(matches!(
            outcome,
            Err(OpenOriginalArticleError::Article(AppError::Validation(_)))
        ));
        assert!(opened.is_none());
    }

    #[test]
    fn open_original_article_refuses_unsafe_saved_url_without_opening() {
        let ctx = make_context();
        save_with_original_url(&ctx, "open-js", "javascript:alert(1)");
        save_with_original_url(&ctx, "open-file", "file:///C:/Windows/System32/calc.exe");

        for article_id in ["open-js", "open-file"] {
            let (outcome, opened) = open_with_recorder(&ctx, article_id, Ok(()));
            assert!(
                matches!(outcome, Err(OpenOriginalArticleError::UrlRejected)),
                "{article_id}"
            );
            assert!(opened.is_none(), "{article_id}");
        }
    }

    #[test]
    fn open_original_article_maps_launch_failures_to_fixed_errors() {
        let ctx = make_context();
        save_with_original_url(&ctx, "open-fail", "https://example.com/news/2");

        let (outcome, _) =
            open_with_recorder(&ctx, "open-fail", Err(BrowserOpenError::LaunchFailed));
        assert!(matches!(
            outcome,
            Err(OpenOriginalArticleError::LaunchFailed)
        ));

        let (outcome, _) =
            open_with_recorder(&ctx, "open-fail", Err(BrowserOpenError::Unsupported));
        let error: crate::error::CommandError = outcome.unwrap_err().into();
        assert_eq!(error.code, "OPEN_BROWSER_UNSUPPORTED");
        assert!(!error.message.contains("example.com"));
    }
}
