//! 友情ランクサービス。状態の読み取りと、イベントによるポイント加算（ランクアップ判定込み）を担う。
//!
//! サーバ(Rust)を正とし、**デイリー上限・有効イベント検証・ランクアップ判定をここで強制**する。
//! フロントはイベント発生を `record_friendship_event` で伝えるだけで、加算量や上限は決められない。

use crate::domain::friendship::{
    FriendshipEventType, FriendshipStateDto, RecordFriendshipEventResult,
};
use crate::domain::yuuko::local_date_key;
use crate::error::AppError;
use crate::repositories::friendship_repository::FriendshipRepository;
use chrono::{DateTime, TimeZone, Utc};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct FriendshipService {
    friendship_repository: FriendshipRepository,
    store_lock: Arc<Mutex<()>>,
}

impl FriendshipService {
    pub fn new(friendship_repository: FriendshipRepository) -> Self {
        Self {
            friendship_repository,
            store_lock: Arc::new(Mutex::new(())),
        }
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
        let event = FriendshipEventType::from_storage(event_type).ok_or_else(|| {
            AppError::Validation(format!("unknown friendship event type: {event_type}"))
        })?;

        let _guard = self.lock_store()?;
        let mut state = self.friendship_repository.load_or_default()?;
        let today = local_date_key(now, tz);
        let outcome = state.earn(event, &today, &format_utc_timestamp(now));
        self.friendship_repository.save(&state)?;

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
        let service = FriendshipService::new(FriendshipRepository::new(&paths));
        (service, root)
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
}
