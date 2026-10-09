//! 友情ランクサービス。状態の読み取りと、イベントによるポイント加算（ランクアップ判定込み）を担う。
//!
//! サーバ(Rust)を正とし、**デイリー上限・有効イベント検証・ランクアップ判定をここで強制**する。
//! フロントはイベント発生を `record_friendship_event` で伝えるだけで、加算量や上限は決められない。
//! ランクアップ時はランク報酬の解放（rewards.json）と、流れ星のかけらの付与（gacha_state.json）も併せて行う。
//! 実際に加算できたときは、直近の加算イベント履歴（friendship_events.json・データ設計書 §10.3）にも追記する。

use crate::domain::friendship::{
    FriendshipEventRecord, FriendshipEventType, FriendshipStateDto, RecordFriendshipEventResult,
};
use crate::domain::yuuko::local_date_key;
use crate::error::AppError;
use crate::repositories::friendship_event_repository::FriendshipEventRepository;
use crate::repositories::friendship_repository::FriendshipRepository;
use crate::services::gacha_service::{log_grant_failure, GachaService};
use crate::services::reward_service::RewardService;
use chrono::{DateTime, TimeZone, Utc};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct FriendshipService {
    friendship_repository: FriendshipRepository,
    reward_service: RewardService,
    store_lock: Arc<Mutex<()>>,
    /// ランクアップ時の流れ星のかけら付与先（データ設計書 §12.4）。未設定なら付与しない。
    gacha_service: Option<GachaService>,
    /// 加算イベント履歴の保存先（データ設計書 §10.3）。未設定なら履歴を残さない。
    event_repository: Option<FriendshipEventRepository>,
}

impl FriendshipService {
    pub fn new(friendship_repository: FriendshipRepository, reward_service: RewardService) -> Self {
        Self {
            friendship_repository,
            // 報酬側も friendship.json を読むため、同じロックを共有する（順序は friendship → reward）。
            store_lock: reward_service.friendship_store_lock(),
            reward_service,
            gacha_service: None,
            event_repository: None,
        }
    }

    /// ランクアップ時にかけらを付与するガチャサービスを設定する（アプリ起動時の組み立て用）。
    pub fn with_gacha_service(mut self, gacha_service: GachaService) -> Self {
        self.gacha_service = Some(gacha_service);
        self
    }

    /// 加算イベント履歴の保存先を設定する（アプリ起動時の組み立て用）。
    pub fn with_event_repository(mut self, event_repository: FriendshipEventRepository) -> Self {
        self.event_repository = Some(event_repository);
        self
    }

    /// 初回起動時に既定状態を保存する（未保存なら）。
    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        let _guard = self.lock_store()?;
        if !self.friendship_repository.exists() {
            let state = self.friendship_repository.load_or_default()?;
            self.friendship_repository.save(&state)?;
        }
        Ok(())
    }

    /// 現在の友情ランク状態（読み取り）。
    pub fn get_friendship_state(&self) -> Result<FriendshipStateDto, AppError> {
        let _guard = self.lock_store()?;
        Ok(self.friendship_repository.load_or_default()?.to_dto())
    }

    /// イベントによるポイント加算。無効なイベント種別は Validation エラーとする。
    /// デイリー上限・ランクアップ判定はドメイン側で行い、結果を保存して返す。
    pub fn record_friendship_event(
        &self,
        event_type: &str,
    ) -> Result<RecordFriendshipEventResult, AppError> {
        self.record_friendship_event_in(&chrono::Local, Utc::now(), event_type)
    }

    /// タイムゾーンと現在時刻を注入できる本体。テストで実行端末のタイムゾーンに依存せず
    /// 日付境界（ローカル深夜0時でリセット・JST 9時ではリセットしない）を検証するために分けている。
    ///
    /// デイリー上限の日付はローカル日付で判定する（要件定義書 §7.6.3）。通知の日次上限と同じく、
    /// UTC 日付で区切ると日本時間の朝9時に上限がリセットされてしまうため。
    ///
    /// 旧バージョンは daily_points.date を UTC 日付で保存していた。friendship.json には最終加算時刻が
    /// 無く、保存形式も変えないため、保存済み date はそのまま「ローカル日付」とみなして比較する。
    /// 比較は不一致ならリセットする方式なので、旧データでも加算が恒久的に止まることはない。
    /// 唯一のずれは、更新前の最終加算が JST 00:00〜09:00（UTC では前日）だった場合で、
    /// 更新後の同じローカル日に一度だけリセットされ、最大で1日分（上限 25pt）余分に加算され得る。
    /// その時点で date はローカル日付で保存し直されるため、以降は通常どおり上限で頭打ちになる。
    fn record_friendship_event_in<Tz: TimeZone>(
        &self,
        tz: &Tz,
        now: DateTime<Utc>,
        event_type: &str,
    ) -> Result<RecordFriendshipEventResult, AppError> {
        // 入力値はエラー文言へ含めない（§16.3）。
        let event = FriendshipEventType::from_storage(event_type)
            .ok_or_else(|| AppError::Validation("unknown friendship event type".to_string()))?;

        let guard = self.lock_store()?;
        let mut state = self.friendship_repository.load_or_default()?;
        // 読み込み時に累計からランクを導出し直しているので、加算前のランクとしてそのまま使える。
        let rank_before = state.current_rank;
        let today = local_date_key(now, tz);
        let created_at = format_utc_timestamp(now);
        let outcome = state.earn(event, &today, &created_at);
        self.friendship_repository.save(&state)?;

        // 状態の保存に成功し、実際に加算できた（上限で 0pt ではない）ときだけ履歴へ追記する。
        // 同じ友情ロック下で書くので、追記の読み込み〜保存が並行加算と入れ違わない（別ロックを増やさない）。
        // 履歴は補助的な記録なので、失敗してもポイント加算（保存済み）は成功として返す。
        if outcome.earned_points > 0 {
            if let Some(event_repository) = &self.event_repository {
                let record = FriendshipEventRecord {
                    event_type: event.as_storage().to_string(),
                    points: outcome.earned_points,
                    created_at,
                };
                if let Err(error) = event_repository.append(record) {
                    log::warn!("友情ポイント加算イベント履歴の保存に失敗しました: {error}");
                }
            }
        }

        // ランクアップしたら、飛び越えたランクの分も含めて報酬を解放する。
        // 失敗してもポイント加算（保存済み）は取り消さない。解放はランクから冪等に導出するため、
        // 次回の報酬状態取得・起動時に追いつく。
        if outcome.ranked_up {
            if let Err(error) = self.reward_service.sync_with_friendship(&state) {
                log::warn!(
                    "ランクアップ時の報酬解放の保存に失敗しました（次回に再試行します）: {error}"
                );
            }
        }
        drop(guard);

        // ランクアップしたら、上がったランク数ぶんかけらを付与する（§12.4）。デッドロックを避けるため
        // 友情側のロックを手放してから呼ぶ。付与に失敗してもポイント加算（保存済み）は成功として返す。
        let ranks_gained = state.current_rank.saturating_sub(rank_before);
        if ranks_gained > 0 {
            if let Some(gacha_service) = &self.gacha_service {
                log_grant_failure("rank_up", gacha_service.grant_for_rank_up(ranks_gained));
            }
        }

        Ok(RecordFriendshipEventResult {
            ranked_up: outcome.ranked_up,
            new_rank: state.current_rank,
            earned_point: outcome.earned_points,
            state: state.to_dto(),
        })
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        self.store_lock
            .lock()
            .map_err(|_| AppError::Io(std::io::Error::other("friendship store lock was poisoned")))
    }
}

/// 現在時刻（UTC・"YYYY-MM-DDThh:mm:ssZ"）。ランクアップ時刻記録に使う（形式は従来どおり UTC）。
fn format_utc_timestamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::friendship::{DailyPoints, FriendshipState, DEFAULT_DAILY_LIMIT};
    use crate::paths::AppPaths;
    use crate::repositories::reward_repository::RewardRepository;
    use crate::repositories::settings_repository::SettingsRepository;
    use chrono::FixedOffset;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_service() -> (FriendshipService, std::path::PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "yuuko-friendship-service-tests-{}-{unique}",
            std::process::id()
        ));
        let paths = AppPaths::new(root.clone());
        let reward_service = RewardService::new(
            RewardRepository::new(&paths),
            FriendshipRepository::new(&paths),
            SettingsRepository::new(&paths),
        );
        let service = FriendshipService::new(FriendshipRepository::new(&paths), reward_service)
            .with_event_repository(FriendshipEventRepository::new(&paths));
        (service, root)
    }

    #[test]
    fn unknown_event_type_message_excludes_input_value() {
        let (service, root) = temp_service();
        let result = service.record_friendship_event("secret_event<script>");
        let _ = std::fs::remove_dir_all(root);
        match result {
            Err(AppError::Validation(message)) => {
                assert_eq!(message, "unknown friendship event type");
                assert!(!message.contains("secret_event"));
            }
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn cloned_services_serialize_friendship_event_updates() {
        let (service, root) = temp_service();
        let handles = (0..20)
            .map(|_| {
                let service = service.clone();
                std::thread::spawn(move || {
                    service.record_friendship_event("term_explained").unwrap();
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let state = service.get_friendship_state().unwrap();
        // 累計20pt（取りこぼしなし）→ Rank2（到達10pt）・進捗10。
        assert_eq!(state.current_rank, 2);
        assert_eq!(state.current_point, 10);
        assert_eq!(state.daily_earned_point, 20);

        let _ = std::fs::remove_dir_all(root);
    }

    /// テスト用の固定タイムゾーン（JST, +09:00）。実行端末のタイムゾーンに依存させない。
    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    /// JST の壁時計時刻を UTC に変換する。
    fn jst_at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        jst()
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// yuuko_to_main(5pt) を5回記録して、その時刻のローカル日で上限 25pt に到達させる。
    fn fill_daily_limit(service: &FriendshipService, now: DateTime<Utc>) {
        for _ in 0..5 {
            service
                .record_friendship_event_in(&jst(), now, "yuuko_to_main")
                .unwrap();
        }
        let capped = service
            .record_friendship_event_in(&jst(), now, "yuuko_to_main")
            .unwrap();
        assert_eq!(capped.earned_point, 0);
        assert_eq!(capped.state.daily_earned_point, 25);
    }

    /// 旧バージョン相当の状態（daily_points.date が UTC 日付）を保存する。
    fn save_legacy_daily(service: &FriendshipService, utc_date: &str, points: u32) {
        let state = FriendshipState {
            current_points: points,
            total_points: points,
            daily_points: DailyPoints {
                date: utc_date.to_string(),
                points,
                limit: DEFAULT_DAILY_LIMIT,
            },
            ..FriendshipState::default()
        };
        service.friendship_repository.save(&state).unwrap();
    }

    #[test]
    fn daily_limit_resets_at_local_midnight() {
        let (service, root) = temp_service();
        // JST 23:30（UTC では同日 14:30）で上限到達。
        fill_daily_limit(&service, jst_at(2026, 6, 8, 23, 30));

        // JST 翌日 00:05（UTC ではまだ 06-08）。ローカル日付が変わったのでリセットされる。
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 0, 5), "yuuko_to_main")
            .unwrap();
        assert_eq!(result.earned_point, 5);
        assert_eq!(result.state.daily_earned_point, 5);
        assert_eq!(result.state.last_point_date.as_deref(), Some("2026-06-09"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn daily_limit_does_not_reset_at_jst_nine_oclock() {
        let (service, root) = temp_service();
        // JST 00:30（UTC では前日 06-08 15:30）で上限到達。
        fill_daily_limit(&service, jst_at(2026, 6, 9, 0, 30));

        // JST 09:30（UTC 日付は 06-09 に変わる）でも同じローカル日なのでリセットしない。
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 9, 30), "yuuko_to_main")
            .unwrap();
        assert_eq!(result.earned_point, 0);
        assert_eq!(result.state.daily_earned_point, 25);
        assert_eq!(result.state.last_point_date.as_deref(), Some("2026-06-09"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_utc_date_matching_local_today_keeps_limit() {
        let (service, root) = temp_service();
        // 旧版で JST 06-08 19:00（UTC 10:00）に上限到達 → date は UTC の "2026-06-08"。
        save_legacy_daily(&service, "2026-06-08", 25);

        // 同じローカル日の JST 22:00 は上限のまま加算されない。
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 8, 22, 0), "yuuko_to_main")
            .unwrap();
        assert_eq!(result.earned_point, 0);

        // 翌ローカル日には通常どおりリセットされる（恒久的に止まらない）。
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 1, 0), "yuuko_to_main")
            .unwrap();
        assert_eq!(result.earned_point, 5);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_utc_date_from_early_morning_over_grants_at_most_one_day_once() {
        let (service, root) = temp_service();
        // 旧版で JST 06-09 02:00（UTC 06-08 17:00）に上限到達 → date は UTC の "2026-06-08"。
        save_legacy_daily(&service, "2026-06-08", 25);

        // 更新後の同じローカル日（JST 06-09 03:00）は date 不一致で一度だけリセットされる。
        // 余分な加算は最大で1日分（25pt）に収まり、その後は上限で頭打ちになる。
        let now = jst_at(2026, 6, 9, 3, 0);
        let mut extra = 0;
        for _ in 0..10 {
            extra += service
                .record_friendship_event_in(&jst(), now, "yuuko_to_main")
                .unwrap()
                .earned_point;
        }
        assert_eq!(extra, DEFAULT_DAILY_LIMIT);

        // JST 9時を過ぎても再リセットされない（date はローカル日付で保存し直されている）。
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 9, 30), "yuuko_to_main")
            .unwrap();
        assert_eq!(result.earned_point, 0);
        assert_eq!(result.state.last_point_date.as_deref(), Some("2026-06-09"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_date_ahead_of_local_today_does_not_block_points() {
        let (service, root) = temp_service();
        // 西側タイムゾーン等で保存済み date がローカル今日より先の日付になっていても、
        // 不一致としてリセットされ、加算が止まり続けることはない。
        save_legacy_daily(&service, "2026-06-10", 25);
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 0), "yuuko_to_main")
            .unwrap();
        assert_eq!(result.earned_point, 5);
        assert_eq!(result.state.last_point_date.as_deref(), Some("2026-06-09"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rank_up_by_event_unlocks_rewards_into_rewards_json() {
        let (service, root) = temp_service();
        // 累計 24pt（Rank2・あと1pt で Rank3）から 1pt で Rank3 → テーマ①を解放。
        save_legacy_daily(&service, "2026-06-01", 24);
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 0), "term_explained")
            .unwrap();
        assert!(result.ranked_up);
        assert_eq!(result.new_rank, 3);

        let saved = RewardRepository::new(&AppPaths::new(root.clone()))
            .load()
            .unwrap()
            .expect("rewards.json is written on rank up");
        assert_eq!(saved.unlocked_reward_ids, vec!["theme_001".to_string()]);
        assert_eq!(saved.pending_reward_ids(), vec!["theme_001".to_string()]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rank_up_grants_star_fragments_and_non_rank_up_does_not() {
        use crate::domain::gacha::{INITIAL_FRAGMENTS, RANK_UP_GRANT};
        use crate::repositories::gacha_repository::GachaRepository;

        let (service, root) = temp_service();
        let gacha_repository = GachaRepository::new(&AppPaths::new(root.clone()));
        let service = service.with_gacha_service(GachaService::new(gacha_repository.clone()));

        // 累計 23pt（Rank2）から 1pt ではランクアップしない → 付与しない。
        save_legacy_daily(&service, "2026-06-01", 23);
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 0), "term_explained")
            .unwrap();
        assert!(!result.ranked_up);
        assert!(gacha_repository.load().unwrap().is_none());

        // 次の 1pt で Rank3 → +30。
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 1), "term_explained")
            .unwrap();
        assert!(result.ranked_up);
        assert_eq!(
            gacha_repository.load().unwrap().unwrap().star_fragments,
            INITIAL_FRAGMENTS + RANK_UP_GRANT
        );

        // 同じランク内の加算では二重に付与しない。
        service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 2), "term_explained")
            .unwrap();
        assert_eq!(
            gacha_repository.load().unwrap().unwrap().star_fragments,
            INITIAL_FRAGMENTS + RANK_UP_GRANT
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reward_service_reads_friendship_under_the_same_lock() {
        let (service, root) = temp_service();
        assert!(Arc::ptr_eq(
            &service.store_lock,
            &service.reward_service.friendship_store_lock()
        ));

        // 友情側のロック保持中は、報酬側の friendship.json 読み込みが待たされる。
        let guard = service.lock_store().unwrap();
        let reward_service = service.reward_service.clone();
        let handle = std::thread::spawn(move || reward_service.get_reward_state());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!handle.is_finished());
        drop(guard);
        assert!(handle.join().unwrap().is_ok());

        // ランクアップ（friendship → reward の順でロック）でもデッドロックしない。
        save_legacy_daily(&service, "2026-06-01", 24);
        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 0), "term_explained")
            .unwrap();
        assert!(result.ranked_up);

        let _ = std::fs::remove_dir_all(root);
    }

    fn load_events(root: &std::path::Path) -> crate::domain::friendship::FriendshipEventLog {
        FriendshipEventRepository::new(&AppPaths::new(root.to_path_buf()))
            .load()
            .unwrap()
    }

    #[test]
    fn successful_addition_appends_event_history_and_capped_addition_does_not() {
        let (service, root) = temp_service();
        let now = jst_at(2026, 6, 9, 12, 0);
        // 5pt × 5 で上限 25pt → 6回目は 0pt（履歴に残さない）。
        fill_daily_limit(&service, now);

        let log = load_events(&root);
        assert_eq!(log.events.len(), 5);
        let first = &log.events[0];
        assert_eq!(first.event_type, "yuuko_to_main");
        assert_eq!(first.points, 5);
        assert_eq!(first.created_at, "2026-06-09T03:00:00Z");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn event_history_is_truncated_to_most_recent_limit() {
        use crate::domain::friendship::FRIENDSHIP_EVENT_HISTORY_LIMIT;

        let (service, root) = temp_service();
        // term_explained(1pt) は1日25回まで加算できるので、日を変えながら上限を超える件数を記録する。
        let total = FRIENDSHIP_EVENT_HISTORY_LIMIT + 10;
        for i in 0..total {
            let day = 1 + (i / 25) as u32;
            let minute = (i % 25) as u32;
            let result = service
                .record_friendship_event_in(
                    &jst(),
                    jst_at(2026, 7, day, 12, minute),
                    "term_explained",
                )
                .unwrap();
            assert_eq!(result.earned_point, 1);
        }

        let log = load_events(&root);
        assert_eq!(log.events.len(), FRIENDSHIP_EVENT_HISTORY_LIMIT);
        // 最古の10件（7/1 12:00〜12:09 JST）が捨てられ、最新が末尾に残る。
        assert_eq!(log.events[0].created_at, "2026-07-01T03:10:00Z");
        assert_eq!(
            log.events.last().unwrap().created_at,
            "2026-07-09T03:09:00Z"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn event_history_save_failure_does_not_fail_point_addition() {
        let (service, root) = temp_service();
        // 履歴ファイルの位置をフォルダにして、読み込み・保存とも失敗させる。
        std::fs::create_dir_all(root.join("user").join("friendship_events.json")).unwrap();

        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 0), "news_detail_opened")
            .unwrap();
        assert_eq!(result.earned_point, 3);
        // ポイント加算そのものは保存されている。
        assert_eq!(
            service.get_friendship_state().unwrap().daily_earned_point,
            3
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_event_history_is_reset_without_failing_addition() {
        let (service, root) = temp_service();
        let path = root.join("user").join("friendship_events.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json").unwrap();

        let result = service
            .record_friendship_event_in(&jst(), jst_at(2026, 6, 9, 12, 0), "explanation_viewed")
            .unwrap();
        assert_eq!(result.earned_point, 4);

        let log = load_events(&root);
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].event_type, "explanation_viewed");
        assert_eq!(
            std::fs::read(root.join("user").join("friendship_events.corrupt.json")).unwrap(),
            b"not json"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
