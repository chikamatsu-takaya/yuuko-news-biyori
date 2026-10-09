//! ランク報酬サービス。報酬状態（rewards.json）の読み取り・ランクに応じた解放・確認済みへの更新を担う。
//!
//! 報酬状態の保存先は rewards.json に一本化する（2026-10-05 決定 D35）。friendship.json の
//! `pendingRewardIds` / `confirmedRewardIds` は rewards.json が無いときの互換読込にだけ使い、書き込まない。
//!
//! 解放はランクから冪等に導出する（`RewardsState::unlock_up_to_rank`）。ランクアップ時・起動時・
//! 状態取得時のどこで呼ばれても結果は同じなので、どれかで保存に失敗しても次の機会に追いつく。
//!
//! 新しく解放した報酬は未確認（pendingRewards）に積むだけで、ゆうこ通知状態（reward_notification）は
//! ここでは変更しない。未確認の報酬は YuukoService::request_yuuko_notification が通知ゲートを通った後に
//! 読み、ニュースより優先して報酬通知にする（§6.4。通知回数・クールタイムはニュースと共通）。

use std::sync::{Arc, Mutex, MutexGuard};

use chrono::Utc;

use crate::domain::friendship::FriendshipState;
use crate::domain::reward::{find_reward, RewardStateDto, RewardsState, DEFAULT_THEME_ID};
use crate::error::AppError;
use crate::repositories::friendship_repository::FriendshipRepository;
use crate::repositories::reward_repository::RewardRepository;
use crate::repositories::settings_repository::SettingsRepository;

#[derive(Debug, Clone)]
pub struct RewardService {
    reward_repository: RewardRepository,
    friendship_repository: FriendshipRepository,
    settings_repository: SettingsRepository,
    store_lock: Arc<Mutex<()>>,
    /// friendship.json 用のロック。FriendshipService と同じものを共有する。
    /// load_or_default の bak 復旧（存在確認→rename）が加算の保存と競合すると、新しい
    /// friendship.json を古い bak で上書きしてポイントが巻き戻り得るため、読むときも必ず取る。
    /// 取る順序は常に friendship → reward（デッドロック防止）。
    friendship_lock: Arc<Mutex<()>>,
}

/// 確認処理の結果（確認済みにした ID と、まだ未確認の ID）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRewardsOutcome {
    pub confirmed_reward_ids: Vec<String>,
    pub remaining_pending_reward_ids: Vec<String>,
}

impl RewardService {
    pub fn new(
        reward_repository: RewardRepository,
        friendship_repository: FriendshipRepository,
        settings_repository: SettingsRepository,
    ) -> Self {
        Self {
            reward_repository,
            friendship_repository,
            settings_repository,
            store_lock: Arc::new(Mutex::new(())),
            friendship_lock: Arc::new(Mutex::new(())),
        }
    }

    /// friendship.json 用の共有ロック。FriendshipService はこれを自分の store_lock として使う。
    pub fn friendship_store_lock(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.friendship_lock)
    }

    /// rewards.json のロック。データ移行の取り込みが差し替え中に保持する（friendship ロックの後に取る）。
    pub fn rewards_store_lock(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.store_lock)
    }

    /// 報酬状態を返す（読み取り）。現ランクまでの未解放があればここで解放して保存する
    /// （既に高ランクの既存利用者への移行もこの経路で冪等に行う）。
    /// 保存に失敗しても表示は止めない（次回の取得・ランクアップで再度追いつく）。
    pub fn get_reward_state(&self) -> Result<RewardStateDto, AppError> {
        let _friendship_guard = self.lock_friendship()?;
        let _guard = self.lock_store()?;
        let friendship = self.friendship_repository.load_or_default()?;
        let (state, _, changed) = self.load_synced(&friendship, &now_timestamp())?;
        if changed {
            if let Err(error) = self.reward_repository.save(&state) {
                log::warn!("報酬状態を保存できませんでした（次回に再試行します）: {error}");
            }
        }
        Ok(state.to_dto(friendship.current_rank, &self.selected_theme_id()))
    }

    /// 友情ランクに合わせて報酬を解放し、変更があれば保存する。新たに解放した ID を返す。
    /// ポイント加算でのランクアップ時と起動時に呼ぶ。
    /// 呼び出し側が friendship ロックを保持している前提（ここでは reward ロックだけを取る。
    /// std の Mutex は再入できないため、ここで friendship ロックを取り直すと自己デッドロックになる）。
    pub fn sync_with_friendship(
        &self,
        friendship: &FriendshipState,
    ) -> Result<Vec<String>, AppError> {
        let _guard = self.lock_store()?;
        let (state, newly_unlocked, changed) = self.load_synced(friendship, &now_timestamp())?;
        if changed {
            self.reward_repository.save(&state)?;
        }
        Ok(newly_unlocked)
    }

    /// 起動時に現在のランクへ追いつかせる。失敗してもアプリ起動は止めない（取得時に再試行される）。
    pub fn sync_on_startup(&self) {
        let result = self.lock_friendship().and_then(|_friendship_guard| {
            let friendship = self.friendship_repository.load_or_default()?;
            self.sync_with_friendship(&friendship)
        });
        if let Err(error) = result {
            log::warn!("起動時の報酬状態の更新に失敗しました: {error}");
        }
    }

    /// 未確認の報酬を確認済みにして保存する。未確認でない ID は無視する。
    /// 確認の前に現ランクへ追いつかせるので、未保存の移行分もそのまま確認できる。
    pub fn confirm_rewards(
        &self,
        reward_ids: &[String],
    ) -> Result<ConfirmRewardsOutcome, AppError> {
        let _friendship_guard = self.lock_friendship()?;
        let _guard = self.lock_store()?;
        let friendship = self.friendship_repository.load_or_default()?;
        let (mut state, _, changed) = self.load_synced(&friendship, &now_timestamp())?;
        let confirmed_reward_ids = state.confirm(reward_ids);
        if changed || !confirmed_reward_ids.is_empty() {
            // 確認は利用者の操作結果なので、保存失敗はエラーとして返す（確認済みと誤認させない）。
            self.reward_repository.save(&state)?;
        }
        Ok(ConfirmRewardsOutcome {
            confirmed_reward_ids,
            // UI へ返すため、マスタに存在する報酬だけに絞る（保存ファイル由来の未知 ID を流さない）。
            remaining_pending_reward_ids: state
                .pending_reward_ids()
                .into_iter()
                .filter(|id| find_reward(id).is_some())
                .collect(),
        })
    }

    /// 保存済み状態（無ければ旧データから作成）を読み、整合を取って現ランクまで解放する。
    /// 新たに解放した ID と、保存が必要な変更があったかを併せて返す（保存はしない）。
    fn load_synced(
        &self,
        friendship: &FriendshipState,
        now: &str,
    ) -> Result<(RewardsState, Vec<String>, bool), AppError> {
        // 未保存なら移行結果を必ず書き出す（以降は rewards.json を正とし、旧項目を読まない）。
        let mut changed = !self.reward_repository.exists();
        let mut state = self.load_or_migrate(friendship, now)?;
        changed |= state.normalize();
        let newly_unlocked = state.unlock_up_to_rank(friendship.current_rank, now);
        changed |= !newly_unlocked.is_empty();
        Ok((state, newly_unlocked, changed))
    }

    /// rewards.json を読む。無ければ friendship.json の旧報酬項目から作る（互換読込のみ）。
    fn load_or_migrate(
        &self,
        friendship: &FriendshipState,
        now: &str,
    ) -> Result<RewardsState, AppError> {
        match self.reward_repository.load()? {
            Some(state) => Ok(state),
            None => Ok(RewardsState::from_legacy_friendship(
                &friendship.pending_reward_ids,
                &friendship.confirmed_reward_ids,
                now,
            )),
        }
    }

    /// 適用中テーマの正は設定 `ui.themeId`。読めなければ既定テーマへ倒す（表示を止めない）。
    fn selected_theme_id(&self) -> String {
        match self.settings_repository.load_or_default() {
            Ok(settings) => settings.ui.theme_id,
            Err(error) => {
                log::warn!("設定を読めなかったため既定テーマとして扱います: {error}");
                DEFAULT_THEME_ID.to_string()
            }
        }
    }

    fn lock_friendship(&self) -> Result<MutexGuard<'_, ()>, AppError> {
        self.friendship_lock
            .lock()
            .map_err(|_| AppError::Io(std::io::Error::other("friendship store lock was poisoned")))
    }

    fn lock_store(&self) -> Result<MutexGuard<'_, ()>, AppError> {
        self.store_lock
            .lock()
            .map_err(|_| AppError::Io(std::io::Error::other("rewards store lock was poisoned")))
    }
}

/// 解放時刻（UTC・"YYYY-MM-DDThh:mm:ssZ"）。friendship の lastRankUpAt と同じ形式。
fn now_timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::PersistedSettings;
    use crate::paths::AppPaths;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct Ctx {
        service: RewardService,
        friendship_repository: FriendshipRepository,
        settings_repository: SettingsRepository,
        paths: AppPaths,
    }

    impl Drop for Ctx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.paths.app_data_dir);
        }
    }

    fn ctx() -> Ctx {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "yuuko-reward-service-tests-{}-{n}",
            std::process::id()
        ));
        let paths = AppPaths::new(root);
        let friendship_repository = FriendshipRepository::new(&paths);
        let settings_repository = SettingsRepository::new(&paths);
        let service = RewardService::new(
            RewardRepository::new(&paths),
            friendship_repository.clone(),
            settings_repository.clone(),
        );
        Ctx {
            service,
            friendship_repository,
            settings_repository,
            paths,
        }
    }

    /// 累計 `total` の friendship.json を保存する（ランクは読み込み時に累計から導出される）。
    fn save_friendship_total(ctx: &Ctx, total: u32) -> FriendshipState {
        let mut state = FriendshipState {
            total_points: total,
            ..FriendshipState::default()
        };
        state.normalize_rank_from_total();
        ctx.friendship_repository.save(&state).unwrap();
        state
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn rank_one_user_gets_empty_rewards_file() {
        let ctx = ctx();
        let dto = ctx.service.get_reward_state().unwrap();
        assert_eq!(dto.current_rank, 1);
        assert!(dto.pending_reward_ids.is_empty());
        assert!(dto.rewards.iter().all(|r| !r.unlocked));
        assert_eq!(dto.active_theme_id, "default");
        assert!(ctx.paths.rewards_path.exists());
    }

    #[test]
    fn existing_high_rank_user_is_migrated_with_pending_rewards_and_reloads() {
        let ctx = ctx();
        // 累計 160pt → Rank7（到達累計 135）。旧データからの移行で途中の報酬もすべて解放する。
        let friendship = save_friendship_total(&ctx, 160);
        assert_eq!(friendship.current_rank, 7);

        let dto = ctx.service.get_reward_state().unwrap();
        assert_eq!(dto.pending_reward_ids, ids(&["theme_001", "theme_002"]));

        // 新しいサービス（＝再起動）で読み直しても同じ状態で、二重に解放しない。
        let reloaded = RewardService::new(
            RewardRepository::new(&ctx.paths),
            ctx.friendship_repository.clone(),
            ctx.settings_repository.clone(),
        );
        let again = reloaded.get_reward_state().unwrap();
        assert_eq!(again, dto);
        let saved = RewardRepository::new(&ctx.paths).load().unwrap().unwrap();
        assert_eq!(saved.unlocked_reward_ids, ids(&["theme_001", "theme_002"]));
        assert_eq!(saved.pending_rewards.len(), 2);
    }

    #[test]
    fn corrupt_rewards_file_is_reset_and_unlocks_are_rederived_from_rank() {
        let ctx = ctx();
        save_friendship_total(&ctx, 160); // Rank7
        std::fs::create_dir_all(ctx.paths.rewards_path.parent().unwrap()).unwrap();
        std::fs::write(&ctx.paths.rewards_path, b"{ broken").unwrap();

        // 壊れた rewards.json は退避・初期化され、解放済み報酬はランクから導出し直される。
        // 確認済みだった報酬も未確認として再び現れる（確認状態は退避ファイルにだけ残る）。
        let dto = ctx.service.get_reward_state().unwrap();
        assert_eq!(dto.pending_reward_ids, ids(&["theme_001", "theme_002"]));
        assert_eq!(
            std::fs::read(ctx.paths.rewards_path.with_extension("corrupt.json")).unwrap(),
            b"{ broken"
        );
        let saved = RewardRepository::new(&ctx.paths).load().unwrap().unwrap();
        assert_eq!(saved.unlocked_reward_ids, ids(&["theme_001", "theme_002"]));
    }

    #[test]
    fn sync_unlocks_on_rank_up_and_confirm_persists() {
        let ctx = ctx();
        let friendship = save_friendship_total(&ctx, 25); // Rank3
        assert_eq!(friendship.current_rank, 3);
        assert_eq!(
            ctx.service.sync_with_friendship(&friendship).unwrap(),
            ids(&["theme_001"])
        );
        // 同じランクで再同期しても増えない。
        assert!(ctx
            .service
            .sync_with_friendship(&friendship)
            .unwrap()
            .is_empty());

        let outcome = ctx
            .service
            .confirm_rewards(&ids(&["theme_001", "theme_002"]))
            .unwrap();
        assert_eq!(outcome.confirmed_reward_ids, ids(&["theme_001"]));
        assert!(outcome.remaining_pending_reward_ids.is_empty());

        // 保存後に読み直しても確認済み（解放済み・未確認でない）のまま。
        let dto = ctx.service.get_reward_state().unwrap();
        assert!(dto.pending_reward_ids.is_empty());
        assert!(dto.rewards[0].unlocked && !dto.rewards[0].pending);
        assert!(!dto.rewards[1].unlocked);
    }

    #[test]
    fn legacy_friendship_reward_ids_are_read_only_for_compat() {
        let ctx = ctx();
        let mut friendship = save_friendship_total(&ctx, 0);
        friendship.confirmed_reward_ids = ids(&["theme_002", "deco_001"]);
        ctx.friendship_repository.save(&friendship).unwrap();

        let dto = ctx.service.get_reward_state().unwrap();
        // 旧項目の確認済み theme_002 は解放済み・確認済みとして引き継ぐ（Rank1 でも未解放に戻さない）。
        assert!(dto.rewards[1].unlocked && !dto.rewards[1].pending);
        assert!(dto.pending_reward_ids.is_empty());
        // friendship.json の旧項目は書き換えない。
        let raw = std::fs::read_to_string(&ctx.paths.friendship_path).unwrap();
        assert!(raw.contains("deco_001"));
    }

    #[test]
    fn active_theme_comes_from_settings_only_when_unlocked() {
        let ctx = ctx();
        let mut settings = PersistedSettings::default();
        settings.ui.theme_id = "theme_001".to_string();
        ctx.settings_repository.save(&settings).unwrap();

        assert_eq!(
            ctx.service.get_reward_state().unwrap().active_theme_id,
            "default"
        );
        save_friendship_total(&ctx, 25); // Rank3 で theme_001 解放
        assert_eq!(
            ctx.service.get_reward_state().unwrap().active_theme_id,
            "theme_001"
        );
    }
}
