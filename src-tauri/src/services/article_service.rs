use crate::domain::article::{
    ArchiveRetirementSummaryDto, ArchiveSummaryDto, ArticleDetailDto, ArticleHistoryItemDto,
    ArticleReadState, ArticleSummaryDto, FavoriteUpdateResult, GetArticleDetailParams,
    GetRecommendedArticlesParams, ListArticleHistoryParams, RestoreArchivedArticleParams,
    RestoreArchivedArticleResult, SummaryState, UpdateArticleFavoriteParams,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;
use crate::services::recommendation_service::RecommendationService;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct ArticleService {
    repository: ArticleRepository,
}

impl ArticleService {
    pub fn new(repository: ArticleRepository) -> Self {
        Self { repository }
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
        self.advance_read_state_or_log(&article_id, ArticleReadState::DetailViewed);
        Ok(detail)
    }

    /// 保存済みの AI 要約だけを返す（本文抜粋で補わない）。ゆうこのデスクトップ通知用。
    /// get_article_detail と違い、既読状態は進めない（通知を出しただけで既読にしない）。
    pub fn get_saved_summary(&self, article_id: &str) -> Result<Option<String>, AppError> {
        self.repository.get_saved_summary(article_id)
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
