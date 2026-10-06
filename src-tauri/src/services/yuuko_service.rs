use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Utc};

use crate::domain::article::GetRecommendedArticlesParams;
use crate::domain::fullscreen_suppression::{
    grace_from_seed, time_seed, FullscreenGate, FullscreenSuppressionTracker,
};
use crate::domain::yuuko::{
    ConfirmRankUpRewardParams, ConfirmRankUpRewardResult, NotificationGate, PersistedYuukoState,
    RequestYuukoNotificationResult, YuukoNotificationState, YuukoResidentState,
};
use crate::error::AppError;
use crate::infra::fullscreen_detector::{
    FullscreenDetector, FullscreenStatus, SystemFullscreenDetector,
};
use crate::repositories::settings_repository::SettingsRepository;
use crate::repositories::yuuko_state_repository::YuukoStateRepository;
use crate::services::article_service::ArticleService;

/// Clone しても全画面抑制の記録（Arc 内）は共有され、アプリ内通知とデスクトップ通知スレッドで
/// 同じ猶予を見る。
#[derive(Debug, Clone)]
pub struct YuukoService {
    settings_repository: SettingsRepository,
    yuuko_state_repository: YuukoStateRepository,
    article_service: ArticleService,
    fullscreen_detector: Arc<dyn FullscreenDetector>,
    fullscreen_tracker: Arc<Mutex<FullscreenSuppressionTracker>>,
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
            fullscreen_detector: Arc::new(SystemFullscreenDetector),
            fullscreen_tracker: Arc::new(Mutex::new(FullscreenSuppressionTracker::default())),
        }
    }

    /// テストで OS に依存しない全画面判定を差し込む。
    #[cfg(test)]
    fn with_fullscreen_detector(mut self, detector: Arc<dyn FullscreenDetector>) -> Self {
        self.fullscreen_detector = detector;
        self
    }

    /// 全画面抑制の解除後の猶予中なら残り時間を返す（デスクトップ通知スレッドの次回判定用）。
    pub fn fullscreen_grace_remaining(&self) -> Option<std::time::Duration> {
        self.lock_fullscreen_tracker()
            .grace_remaining(Utc::now())
            .and_then(|remaining| remaining.to_std().ok())
    }

    /// 保存済みの active 通知を出し直してよいかの全画面判定（デスクトップ通知スレッド用）。
    ///
    /// request_yuuko_notification は already_active を全画面判定より先に返す（既存の理由の優先順位を
    /// 保つため）。その結果を使ってゆうこ用ウィンドウ（最前面）に出し直す経路でも、全画面中・猶予中は
    /// 出さないようにするために使う。設定を読めない場合は既定（抑制 ON）で判定する（安全側）。
    pub fn fullscreen_gate_for_redisplay(&self) -> FullscreenGate {
        let suppress_in_fullscreen = self
            .settings_repository
            .load_or_default()
            .map(|settings| settings.notification.suppress_in_fullscreen)
            .unwrap_or(true);
        self.check_fullscreen(Utc::now(), suppress_in_fullscreen)
    }

    /// 全画面・プレゼン中の抑制判定（設計書 §5.2〜§5.4）。設定 OFF なら保留中の抑制も捨てて通知可。
    /// OS 判定に失敗した場合は抑制しない（fail-open）。詳細は出さず警告だけ残す。
    fn check_fullscreen(&self, now: DateTime<Utc>, suppress_in_fullscreen: bool) -> FullscreenGate {
        let mut tracker = self.lock_fullscreen_tracker();
        if !suppress_in_fullscreen {
            tracker.reset();
            return FullscreenGate::Allowed;
        }
        let busy = match self.fullscreen_detector.detect() {
            FullscreenStatus::Busy => true,
            FullscreenStatus::Free => false,
            FullscreenStatus::Unknown => {
                log::warn!("全画面状態を取得できなかったため、全画面による通知抑制を行いません");
                false
            }
        };
        tracker.observe(now, busy, grace_from_seed(time_seed()))
    }

    /// 記録はメモリ上の小さな状態だけなので、他スレッドの panic で毒化していても続行する。
    fn lock_fullscreen_tracker(&self) -> MutexGuard<'_, FullscreenSuppressionTracker> {
        self.fullscreen_tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

        Ok(response)
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

    /// ゆうこにニュース通知を出させる。MVP抑制条件（enabled / 日次上限 / クールタイム / cooldown /
    /// 全画面・プレゼン中と解除後の猶予）と
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

        // 全画面・プレゼン中と解除後の猶予中は出さない。候補選定（紹介済み記録・通知枠の消費）の前に
        // 止めるため、候補は捨てられず解除後の判定で再び選ばれる（§5.3 次回判定まで保留）。
        match self.check_fullscreen(now, settings.notification.suppress_in_fullscreen) {
            FullscreenGate::Suppressed => {
                return Ok(notification_result(false, "fullscreen", &state));
            }
            FullscreenGate::GracePeriod { .. } => {
                return Ok(notification_result(false, "fullscreen_grace", &state));
            }
            FullscreenGate::Allowed => {}
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

    /// テスト用の全画面判定。実行端末の画面状態に依存させない（既定は通知可）。
    #[derive(Debug)]
    struct FakeFullscreenDetector(Mutex<FullscreenStatus>);

    impl FakeFullscreenDetector {
        fn set(&self, status: FullscreenStatus) {
            *self.0.lock().unwrap() = status;
        }
    }

    impl FullscreenDetector for FakeFullscreenDetector {
        fn detect(&self) -> FullscreenStatus {
            *self.0.lock().unwrap()
        }
    }

    /// 一時ディレクトリに各リポジトリを構成し、Drop で後始末する。
    struct ServiceContext {
        service: YuukoService,
        settings_repository: SettingsRepository,
        yuuko_state_repository: YuukoStateRepository,
        article_repository: ArticleRepository,
        fullscreen: Arc<FakeFullscreenDetector>,
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
        let fullscreen = Arc::new(FakeFullscreenDetector(Mutex::new(FullscreenStatus::Free)));
        let service = YuukoService::new(
            settings_repository.clone(),
            yuuko_state_repository.clone(),
            article_service,
        )
        .with_fullscreen_detector(fullscreen.clone());

        ServiceContext {
            service,
            settings_repository,
            yuuko_state_repository,
            article_repository,
            fullscreen,
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

    /// 通知枠・紹介済みを消費していないこと（候補が保留されていること）を確認する。
    fn assert_nothing_consumed(ctx: &ServiceContext) {
        let state = load_state(ctx);
        assert!(!state.has_active_notification());
        assert_eq!(state.daily_notification.count, 0);
        assert!(state.introduced_article_ids.is_empty());
    }

    /// 11. 全画面中は fullscreen で抑制し、解除後は猶予（30〜180秒）を置いてから同じ候補を出す。
    #[test]
    fn request_holds_candidate_while_fullscreen_and_notifies_after_grace() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 3, all_day_ranges());
        ctx.fullscreen.set(FullscreenStatus::Busy);

        let suppressed = ctx.service.request_yuuko_notification().unwrap();
        assert!(!suppressed.notified);
        assert_eq!(suppressed.reason, "fullscreen");
        assert_nothing_consumed(&ctx);
        assert!(ctx.service.fullscreen_grace_remaining().is_none());

        // 解除直後は猶予中で、まだ出さない。
        ctx.fullscreen.set(FullscreenStatus::Free);
        let grace = ctx.service.request_yuuko_notification().unwrap();
        assert!(!grace.notified);
        assert_eq!(grace.reason, "fullscreen_grace");
        assert_nothing_consumed(&ctx);
        let remaining = ctx
            .service
            .fullscreen_grace_remaining()
            .expect("grace should be pending");
        assert!(remaining <= std::time::Duration::from_secs(180));
        assert!(remaining > std::time::Duration::from_secs(25));

        // 猶予が明けた時点を観測させる（実時間を待たない）。
        ctx.service.lock_fullscreen_tracker().observe(
            Utc::now() + Duration::seconds(181),
            false,
            Duration::seconds(30),
        );
        let notified = ctx.service.request_yuuko_notification().unwrap();
        assert!(notified.notified);
        assert_eq!(notified.reason, "notified");
        // 保留していた最上位候補がそのまま選ばれる。
        assert_eq!(
            notified.state.current_article_id.as_deref(),
            Some("article-001")
        );
    }

    /// 11b. active 通知が残ったまま全画面になった場合、already_active の優先順位は変えずに、
    /// 出し直し用の判定では抑制する（最前面のゆうこ用ウィンドウを全画面アプリの上に出さない）。
    #[test]
    fn redisplay_of_active_notification_is_blocked_while_fullscreen() {
        let ctx = make_context();
        save_notification_settings(&ctx, true, 3, all_day_ranges());
        let active_state = PersistedYuukoState {
            state: YuukoResidentState::BalloonVisible,
            current_article_id: Some("existing-article".to_string()),
            ..PersistedYuukoState::default()
        };
        ctx.yuuko_state_repository.save(&active_state).unwrap();
        ctx.fullscreen.set(FullscreenStatus::Busy);

        let result = ctx.service.request_yuuko_notification().unwrap();
        assert_eq!(result.reason, "already_active");
        assert_eq!(
            ctx.service.fullscreen_gate_for_redisplay(),
            FullscreenGate::Suppressed
        );

        // 解除直後は猶予中のため、まだ出し直さない。
        ctx.fullscreen.set(FullscreenStatus::Free);
        assert!(matches!(
            ctx.service.fullscreen_gate_for_redisplay(),
            FullscreenGate::GracePeriod { .. }
        ));
    }

    /// 11c. 全画面でなければ出し直してよい。
    #[test]
    fn redisplay_of_active_notification_is_allowed_when_not_fullscreen() {
        let ctx = make_context();
        save_notification_settings(&ctx, true, 3, all_day_ranges());

        assert_eq!(
            ctx.service.fullscreen_gate_for_redisplay(),
            FullscreenGate::Allowed
        );
    }

    /// 12. 設定「全画面中は抑制」が OFF なら、全画面中でも通知する。
    #[test]
    fn request_ignores_fullscreen_when_setting_is_off() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        let mut settings = PersistedSettings::default();
        settings.notification.work_time_ranges = all_day_ranges();
        settings.notification.suppress_in_fullscreen = false;
        ctx.settings_repository.save(&settings).unwrap();
        ctx.fullscreen.set(FullscreenStatus::Busy);

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(result.notified);
        assert_eq!(result.reason, "notified");
    }

    /// 13. 全画面状態を取得できない場合は抑制しない（fail-open）。
    #[test]
    fn request_notifies_when_fullscreen_state_is_unknown() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 3, all_day_ranges());
        ctx.fullscreen.set(FullscreenStatus::Unknown);

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert!(result.notified);
    }

    /// 14. 日次上限などの既存理由は全画面判定より優先され、全画面を観測しない。
    #[test]
    fn existing_gates_take_precedence_over_fullscreen() {
        let ctx = make_context();
        ctx.article_repository
            .initialize_default_if_missing()
            .unwrap();
        save_notification_settings(&ctx, true, 0, all_day_ranges());
        ctx.fullscreen.set(FullscreenStatus::Busy);

        let result = ctx.service.request_yuuko_notification().unwrap();

        assert_eq!(result.reason, "daily_limit");
    }
}
