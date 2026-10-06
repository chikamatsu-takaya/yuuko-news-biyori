use chrono::Utc;

use crate::domain::article::GetRecommendedArticlesParams;
use crate::domain::yuuko::{
    short_preview_summary, ConfirmRankUpRewardParams, ConfirmRankUpRewardResult, NotificationGate,
    PersistedYuukoState, RequestYuukoNotificationResult, YuukoNotificationState,
    YuukoResidentState,
};
use crate::error::AppError;
use crate::repositories::settings_repository::SettingsRepository;
use crate::repositories::yuuko_state_repository::YuukoStateRepository;
use crate::services::article_service::ArticleService;

#[derive(Debug, Clone)]
pub struct YuukoService {
    settings_repository: SettingsRepository,
    yuuko_state_repository: YuukoStateRepository,
    article_service: ArticleService,
}

impl YuukoService {
    pub fn new(
        settings_repository: SettingsRepository,
        yuuko_state_repository: YuukoStateRepository,
        article_service: ArticleService,
    ) -> Self {
        Self {
            settings_repository,
            yuuko_state_repository,
            article_service,
        }
    }

    pub fn get_yuuko_notification_state(&self) -> Result<YuukoNotificationState, AppError> {
        let settings = self.settings_repository.load_or_default()?;
        let mut yuuko_state = self.yuuko_state_repository.load_or_default()?;
        let mut response = yuuko_state.to_notification_state();

        if !settings.notification.enabled {
            response.state = YuukoResidentState::Suppressed;
            response.has_notification = false;
            response.balloon_text = Some("通知設定がOFFになっているよ。".to_string());
            response.preview_article = None;
            response.current_article_id = None;
            response.reward_notification = None;

            // The persisted state is not updated here because this is a read-only command.
            return Ok(response);
        }

        if response.balloon_text.is_none() {
            yuuko_state.balloon_text = Some("気になるニュースを見つけたら教えるね。".to_string());
            self.yuuko_state_repository.save(&yuuko_state)?;
            response.balloon_text = yuuko_state.balloon_text.clone();
        }

        // ゆうこ用ウィンドウはマウント時にこの結果で初期表示するため、イベント経路と同じ短い要約を詰める。
        self.attach_preview_short_summary(&mut response);
        Ok(response)
    }

    /// active なニュース通知のとき、紹介中記事の保存済み AI 要約を短く切り詰めて状態へ詰める。
    ///
    /// preview_article.summary は要約が無いと本文抜粋で補われる（メイン画面の表示用でその意味は変えない）ため、
    /// デスクトップ通知が本文を送らないよう補う前の要約を記事から読み直す。preview_article は通知時点の
    /// 写しなので、後から生成された要約も反映できる。読めない場合は要約なし（固定の一言）に倒す。
    ///
    /// 記事の読み込みを伴うため、実際に表示へ使う場面（初期表示の状態取得と、デスクトップ通知の表示直前）
    /// だけで呼ぶ。メイン画面も5分ごとに呼ぶ request_yuuko_notification では呼ばない（常駐時の負荷を抑える）。
    pub fn attach_preview_short_summary(&self, state: &mut YuukoNotificationState) {
        let is_active_news = matches!(
            state.state,
            YuukoResidentState::Appearing
                | YuukoResidentState::BalloonVisible
                | YuukoResidentState::PreviewVisible
        );
        if !is_active_news {
            return;
        }
        let Some(article) = state.preview_article.as_ref() else {
            return;
        };
        let article_id = state
            .current_article_id
            .clone()
            .unwrap_or_else(|| article.article_id.clone());
        match self.article_service.get_saved_summary(&article_id) {
            Ok(summary) => {
                state.preview_short_summary = summary.as_deref().and_then(short_preview_summary);
            }
            Err(error) => {
                // 本文・要約はログに出さない。
                log::warn!("ゆうこ通知の短い要約を読み込めませんでした ({article_id}): {error}");
            }
        }
    }

    pub fn confirm_rank_up_reward(
        &self,
        params: ConfirmRankUpRewardParams,
    ) -> Result<ConfirmRankUpRewardResult, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        let result = state.confirm_rank_up_reward(&params.reward_ids)?;
        self.yuuko_state_repository.save(&state)?;
        Ok(result)
    }

    /// ゆうこの通知を閉じる。pending 報酬は保持し、再通知抑制（クールタイム）を設定する。
    pub fn dismiss_yuuko_notification(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        state.dismiss_notification(Utc::now());
        self.yuuko_state_repository.save(&state)?;
        Ok(state.to_notification_state())
    }

    /// 無操作タイムアウト（無視）を記録する。フロントの自動退場タイマーから呼ぶ想定
    /// （Rust側はタイマー/ポーリングを持たない）。pending 報酬は保持する。
    pub fn mark_yuuko_ignored(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        state.mark_ignored(Utc::now());
        self.yuuko_state_repository.save(&state)?;
        Ok(state.to_notification_state())
    }

    /// ゆうこにニュース通知を出させる。MVP抑制条件（enabled / 日次上限 / クールタイム / cooldown）と
    /// 候補選定（未紹介・未読・スコア順／お気に入り除外）を満たす場合のみ、おすすめから1件を
    /// 「紹介中」状態にする（設計書 §4.3/§5.2/§6/§12）。報酬 pending 時は誤消し防止のため何もしない。
    pub fn request_yuuko_notification(&self) -> Result<RequestYuukoNotificationResult, AppError> {
        let settings = self.settings_repository.load_or_default()?;
        let mut state = self.yuuko_state_repository.load_or_default()?;

        if !settings.notification.enabled {
            return Ok(notification_result(false, "disabled", &state));
        }

        // 報酬通知が出ている間はニュース通知で上書きしない（報酬を誤って消さない・§6.4 報酬優先）。
        if state
            .reward_notification
            .as_ref()
            .is_some_and(|reward| reward.pending)
        {
            return Ok(notification_result(false, "reward_pending", &state));
        }

        // 既にアクティブな通知（ユーザー未対応）が出ている場合は上書きしない（再起動後も潰さない）。
        if state.has_active_notification() {
            return Ok(notification_result(false, "already_active", &state));
        }

        let now = Utc::now();
        match state.can_notify(
            now,
            settings.notification.max_per_day,
            settings
                .notification
                .work_time_ranges
                .iter()
                .map(|range| (range.start.as_str(), range.end.as_str())),
        ) {
            NotificationGate::DailyLimitReached => {
                return Ok(notification_result(false, "daily_limit", &state));
            }
            NotificationGate::CoolingDown => {
                return Ok(notification_result(false, "cooling_down", &state));
            }
            NotificationGate::OutsideTimeRange => {
                return Ok(notification_result(false, "outside_time_range", &state));
            }
            NotificationGate::Allowed => {}
        }

        // おすすめ候補（スコア順）から未紹介・未読を優先して1件選ぶ。選定はRust側責務（§2.3）。
        let candidates = self
            .article_service
            .get_recommended_articles(GetRecommendedArticlesParams::default())?;
        let Some(article) = state.pick_introducible(&candidates) else {
            return Ok(notification_result(false, "no_candidate", &state));
        };

        state.mark_notified(now, article);
        self.yuuko_state_repository.save(&state)?;
        Ok(notification_result(true, "notified", &state))
    }

    /// ゆうこクリックの2段階遷移（最小実装）。遷移が起きた場合のみ保存する。
    pub fn handle_yuuko_clicked(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        if state.handle_click() {
            self.yuuko_state_repository.save(&state)?;
            // 初回クリック（PreviewVisible 遷移）で軽量プレビューを見せた記事を Previewed にする
            // （詳細設計書 §11.2）。ドメインの遷移は副作用を持たないため、既読保存はサービス側で行う。
            // 既読保存の失敗はログのみで、クリック遷移の結果は返す。
            if state.state == YuukoResidentState::PreviewVisible {
                let article_id = state.current_article_id.clone().or_else(|| {
                    state
                        .preview_article
                        .as_ref()
                        .map(|article| article.article_id.clone())
                });
                if let Some(article_id) = article_id {
                    self.article_service.mark_article_previewed(&article_id);
                }
            }
        }
        Ok(state.to_notification_state())
    }

    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        if !self.yuuko_state_repository.exists() {
            let state = self.yuuko_state_repository.load_or_default()?;
            self.yuuko_state_repository.save(&state)?;
        }
        Ok(())
    }
}

/// request_yuuko_notification の結果を組み立てる（最新状態を通知DTOへ変換）。
fn notification_result(
    notified: bool,
    reason: &str,
    state: &PersistedYuukoState,
) -> RequestYuukoNotificationResult {
    RequestYuukoNotificationResult {
        notified,
        reason: reason.to_string(),
        state: state.to_notification_state(),
    }
}

#[cfg(test)]
mod tests {
    //! サービス層テスト: `request_yuuko_notification` が保存済み設定値
    //! （通知ON/OFF・notifyMaxPerDay・workTimeRanges）を通知判定へ正しく渡していることを保証する。
    //! 各リポジトリは一時ディレクトリ（実ファイル）で構成し、本番コードへテスト用分岐は追加しない。
    use super::*;
    use crate::domain::article::{ArticleHistoryFilter, ArticleReadState, GetArticleDetailParams};
    use crate::domain::settings::{PersistedSettings, WorkTimeRange};
    use crate::paths::AppPaths;
    use crate::repositories::article_repository::ArticleRepository;
    use chrono::{Duration, Local, Timelike};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 一時ディレクトリに各リポジトリを構成し、Drop で後始末する。
    struct ServiceContext {
        service: YuukoService,
        settings_repository: SettingsRepository,
        yuuko_state_repository: YuukoStateRepository,
        article_repository: ArticleRepository,
        root: PathBuf,
    }

    impl Drop for ServiceContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn make_context() -> ServiceContext {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("yuuko-service-tests-{}-{}", std::process::id(), n));
        let paths = AppPaths::new(root.clone());
        paths.ensure_storage_dirs().expect("create storage dirs");

        let settings_repository = SettingsRepository::new(&paths);
        let yuuko_state_repository = YuukoStateRepository::new(&paths);
        let article_repository = ArticleRepository::new(&paths);
        let article_service = ArticleService::new(article_repository.clone());
        let service = YuukoService::new(
            settings_repository.clone(),
            yuuko_state_repository.clone(),
            article_service,
        );

        ServiceContext {
            service,
            settings_repository,
            yuuko_state_repository,
            article_repository,
            root,
        }
    }

    /// 終日 in-range な時間帯（範囲判定がいつ実行されても通る）。
    fn all_day_ranges() -> Vec<WorkTimeRange> {
        vec![WorkTimeRange {
            start: "00:00".to_string(),
            end: "23:59".to_string(),
        }]
    }

    /// 現在のローカル時刻を含まない「午前/午後2枠」（実行時刻に依存せず常に範囲外）。
    fn ranges_excluding_now() -> Vec<WorkTimeRange> {
        let to_hhmm = |t: chrono::DateTime<Local>| format!("{:02}:{:02}", t.hour(), t.minute());
        let now = Local::now();
        vec![
            WorkTimeRange {
                start: to_hhmm(now + Duration::minutes(10)),
                end: to_hhmm(now + Duration::minutes(11)),
            },
            WorkTimeRange {
                start: to_hhmm(now + Duration::minutes(20)),
                end: to_hhmm(now + Duration::minutes(21)),
            },
        ]
    }

    fn save_notification_settings(
        ctx: &ServiceContext,
        enabled: bool,
        max_per_day: u32,
        work_time_ranges: Vec<WorkTimeRange>,
    ) {
        let mut settings = PersistedSettings::default();
        settings.notification.enabled = enabled;
        settings.notification.max_per_day = max_per_day;
        settings.notification.work_time_ranges = work_time_ranges;
        ctx.settings_repository
            .save(&settings)
            .expect("save settings");
    }

    fn load_state(ctx: &ServiceContext) -> PersistedYuukoState {
        ctx.yuuko_state_repository.load_or_default().unwrap()
    }

    /// 1. 通知OFFなら、候補があっても disabled。通知枠・紹介済みIDを消費しない。
    #[test]
    fn request_returns_disabled_when_notifications_are_off() {
        let ctx = make_context();
        // 候補記事は存在するが、通知OFF。
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, false, 3, all_day_ranges());

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(!result.notified);
        assert_eq!(result.reason, "disabled");
        // active 通知を作らず、紹介済みID・通知枠を消費しない。
        let state = load_state(&ctx);
        assert!(!state.has_active_notification());
        assert!(state.introduced_article_ids.is_empty());
        assert_eq!(state.daily_notification.count, 0);
    }

    /// 2. 保存済み max_per_day=1 が日次上限として効く（2回目は daily_limit）。
    #[test]
    fn request_respects_saved_max_per_day_as_daily_limit() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 1, all_day_ranges());

        // 1回目: 通知される（当日カウント=1）。
        let first = ctx.service.request_yuuko_notification().unwrap();
        assert!(first.notified, "first request should notify");
        assert_eq!(first.reason, "notified");

        // active を解消する（has_active_notification の短絡を避け、日次上限を検証する）。
        ctx.service.dismiss_yuuko_notification().unwrap();

        // 2回目: 同日・上限到達のため通知されない。日次上限はクールタイムより先に判定される。
        let second = ctx.service.request_yuuko_notification().unwrap();
        assert!(!second.notified);
        assert_eq!(second.reason, "daily_limit");
    }

    /// 3. 現在時刻が workTimeRanges（午前/午後2枠）の範囲外なら outside_time_range。
    #[test]
    fn request_returns_outside_time_range_when_now_is_not_in_work_ranges() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 3, ranges_excluding_now());

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(!result.notified);
        assert_eq!(result.reason, "outside_time_range");
        assert!(!load_state(&ctx).has_active_notification());
    }

    /// 4. 条件は満たすが候補記事が無い場合は no_candidate。通知枠を消費しない。
    #[test]
    fn request_returns_no_candidate_when_no_articles_available() {
        let ctx = make_context();
        // 記事を seed しない → 候補なし。
        save_notification_settings(&ctx, true, 3, all_day_ranges());

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(!result.notified);
        assert_eq!(result.reason, "no_candidate");
        let state = load_state(&ctx);
        assert!(!state.has_active_notification());
        assert_eq!(state.daily_notification.count, 0);
        assert!(state.introduced_article_ids.is_empty());
    }

    /// 5. 通知可能条件を満たす場合は notified。表示用の値と永続状態が更新される。
    #[test]
    fn request_notifies_and_populates_display_fields_when_conditions_met() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 3, all_day_ranges());

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(result.notified);
        assert_eq!(result.reason, "notified");
        // 表示に必要な値が入る。
        assert_eq!(result.state.state, YuukoResidentState::BalloonVisible);
        assert!(result.state.current_article_id.is_some());
        assert!(result.state.preview_article.is_some());
        assert!(result.state.balloon_text.is_some());

        // 永続状態: active 通知・通知履歴・紹介済みIDが更新される。
        let saved = load_state(&ctx);
        assert!(saved.has_active_notification());
        assert_eq!(saved.daily_notification.count, 1);
        assert_eq!(
            saved.daily_notification.date,
            Local::now().format("%Y-%m-%d").to_string()
        );
        assert_eq!(saved.introduced_article_ids.len(), 1);
        assert!(saved.last_notified_at.is_some());
    }

    /// 6. 既に active 通知がある場合は already_active。新規候補を消費しない。
    #[test]
    fn request_returns_already_active_without_consuming_new_candidate() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 3, all_day_ranges());

        // 既存の active 通知を永続化しておく。
        let active_state = PersistedYuukoState {
            state: YuukoResidentState::BalloonVisible,
            balloon_text: Some("既存の通知だよ".to_string()),
            current_article_id: Some("existing-article".to_string()),
            ..PersistedYuukoState::default()
        };
        ctx.yuuko_state_repository.save(&active_state).unwrap();

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(!result.notified);
        assert_eq!(result.reason, "already_active");
        // 既存 active がそのまま返り、新規消費（カウント加算・紹介済み追加）は起きない。
        assert_eq!(
            result.state.current_article_id.as_deref(),
            Some("existing-article")
        );
        let saved = load_state(&ctx);
        assert_eq!(saved.daily_notification.count, 0);
        assert!(saved.introduced_article_ids.is_empty());
    }

    fn read_state_of(ctx: &ServiceContext, article_id: &str) -> ArticleReadState {
        ctx.article_repository
            .list_history(ArticleHistoryFilter::All, 10)
            .unwrap()
            .into_iter()
            .find(|article| article.article_id == article_id)
            .unwrap()
            .read_state
    }

    /// 7. 初回クリック（PreviewVisible 遷移）で紹介記事が Unread→Previewed になり、未読フィルタから外れる。
    #[test]
    fn first_click_marks_introduced_article_previewed() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 3, all_day_ranges());
        let notified = ctx.service.request_yuuko_notification().unwrap();
        let article_id = notified.state.current_article_id.clone().unwrap();
        assert_eq!(read_state_of(&ctx, &article_id), ArticleReadState::Unread);

        let clicked = ctx.service.handle_yuuko_clicked().unwrap();

        assert_eq!(clicked.state, YuukoResidentState::PreviewVisible);
        assert_eq!(
            read_state_of(&ctx, &article_id),
            ArticleReadState::Previewed
        );
        assert!(!ctx
            .article_repository
            .list_history(ArticleHistoryFilter::Unread, 10)
            .unwrap()
            .iter()
            .any(|article| article.article_id == article_id));

        // 再クリック（Leaving 確定）では既読状態を変えない。
        let left = ctx.service.handle_yuuko_clicked().unwrap();
        assert_eq!(left.state, YuukoResidentState::Leaving);
        assert_eq!(
            read_state_of(&ctx, &article_id),
            ArticleReadState::Previewed
        );
    }

    /// 8. 詳細閲覧済み（DetailViewed）の記事は、プレビュー表示で Previewed へ後退しない。
    #[test]
    fn first_click_does_not_regress_detail_viewed_article() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        ctx.article_repository
            .advance_article_read_state("article-001", ArticleReadState::DetailViewed)
            .unwrap();
        let active_state = PersistedYuukoState {
            state: YuukoResidentState::BalloonVisible,
            current_article_id: Some("article-001".to_string()),
            ..PersistedYuukoState::default()
        };
        ctx.yuuko_state_repository.save(&active_state).unwrap();

        let clicked = ctx.service.handle_yuuko_clicked().unwrap();

        assert_eq!(clicked.state, YuukoResidentState::PreviewVisible);
        assert_eq!(
            read_state_of(&ctx, "article-001"),
            ArticleReadState::DetailViewed
        );
    }

    /// 9. 記事詳細で保存された DetailViewed は、おすすめ候補の未読優先選定に反映される。
    #[test]
    fn request_skips_detail_viewed_article_in_favor_of_unread() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 3, all_day_ranges());
        // 最高スコアの article-001 を記事詳細で閲覧済みにする。
        ctx.service
            .article_service
            .get_article_detail(GetArticleDetailParams {
                article_id: "article-001".to_string(),
            })
            .unwrap();

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(result.notified);
        assert_eq!(
            result.state.current_article_id.as_deref(),
            Some("article-002")
        );
    }

    /// 10. 紹介記事が見つからず既読保存に失敗しても、クリック遷移自体は成功する。
    #[test]
    fn first_click_succeeds_even_when_article_is_missing() {
        let ctx = make_context();
        let active_state = PersistedYuukoState {
            state: YuukoResidentState::BalloonVisible,
            current_article_id: Some("missing-article".to_string()),
            ..PersistedYuukoState::default()
        };
        ctx.yuuko_state_repository.save(&active_state).unwrap();

        let clicked = ctx.service.handle_yuuko_clicked().unwrap();

        assert_eq!(clicked.state, YuukoResidentState::PreviewVisible);
    }

    /// AI 要約前（excerpt のみ）の記事を1件だけ保存する。
    fn save_excerpt_only_article(ctx: &ServiceContext, excerpt: &str) {
        ctx.article_repository
            .save_fetched_articles(vec![crate::domain::article::FetchedArticle {
                article_id: "excerpt-only".to_string(),
                title: "要約前の記事".to_string(),
                source_name: "テストソース".to_string(),
                original_url: "https://example.com/news".to_string(),
                fetched_at: "2026-06-20T00:00:00Z".to_string(),
                published_at_text: "2026-06-20T00:00:00Z".to_string(),
                genre: "テクノロジー".to_string(),
                tags: Vec::new(),
                excerpt: Some(excerpt.to_string()),
                recommendation_score: 0.5,
                read_state: ArticleReadState::Unread,
            }])
            .unwrap();
    }

    /// 要約前の記事では、preview_article.summary が本文抜粋で補われても短い要約は付けない
    /// （デスクトップ通知に本文を送らず、固定の一言を出させる）。初期表示経路も同じ。
    #[test]
    fn excerpt_only_article_has_no_preview_short_summary() {
        let ctx = make_context();
        save_excerpt_only_article(&ctx, "本文抜粋の先頭です。");
        save_notification_settings(&ctx, true, 3, all_day_ranges());

        let mut result = ctx.service.request_yuuko_notification().unwrap();
        assert!(result.notified);
        assert_eq!(
            result
                .state
                .preview_article
                .as_ref()
                .and_then(|article| article.summary.as_deref()),
            Some("本文抜粋の先頭です。"),
            "preview_article の意味（excerpt で補う）は変えない"
        );
        // デスクトップ通知の表示直前と同じく詰めても、要約前の記事では付かない。
        ctx.service.attach_preview_short_summary(&mut result.state);
        assert_eq!(result.state.preview_short_summary, None);

        let current = ctx.service.get_yuuko_notification_state().unwrap();
        assert_eq!(current.preview_short_summary, None);
    }

    /// 保存済み AI 要約がある記事では、初期表示経路とデスクトップ通知の表示直前で同じ短い要約を付ける。
    /// request_yuuko_notification（メイン画面も5分ごとに呼ぶ）自体は記事を読まず、要約を詰めない。
    #[test]
    fn saved_summary_is_attached_only_where_displayed() {
        let ctx = make_context();
        save_excerpt_only_article(&ctx, "本文抜粋の先頭です。");
        let long_summary = "保存済みの要約です。".repeat(10);
        ctx.article_repository
            .update_article_summary(
                "excerpt-only",
                crate::domain::article::ArticleSummaryUpdate {
                    summary: long_summary.clone(),
                    yuuko_explanation: "説明".to_string(),
                    focus_points: Vec::new(),
                    yuuko_comment: "ひとこと".to_string(),
                    ai_provider: "mock".to_string(),
                    generated_at: "2026-06-20T00:00:00Z".to_string(),
                },
            )
            .unwrap();
        save_notification_settings(&ctx, true, 3, all_day_ranges());

        let expected = short_preview_summary(&long_summary);
        assert!(expected.as_deref().is_some_and(|s| s.ends_with('…')));
        let mut result = ctx.service.request_yuuko_notification().unwrap();
        assert!(result.notified);
        // 要約が保存済みでも、request の戻り値には詰めない（記事を読まない）。
        assert_eq!(result.state.preview_short_summary, None);
        let already_active = ctx.service.request_yuuko_notification().unwrap();
        assert_eq!(already_active.reason, "already_active");
        assert_eq!(already_active.state.preview_short_summary, None);

        // デスクトップ通知の表示直前（yuuko_desktop_notifier::run_judgement）と初期表示で同じ値になる。
        ctx.service.attach_preview_short_summary(&mut result.state);
        assert_eq!(result.state.preview_short_summary, expected);
        let current = ctx.service.get_yuuko_notification_state().unwrap();
        assert_eq!(current.preview_short_summary, expected);
    }
}
