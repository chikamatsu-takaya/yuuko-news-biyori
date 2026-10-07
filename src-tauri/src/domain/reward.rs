//! ランク報酬のドメイン（要件定義書 §7.6.4 / §7.6.8・データ設計書 §11）。
//!
//! - 報酬マスタ `REWARD_MASTER`（アプリ同梱の静的定義）。
//! - 永続化用 `RewardsState`（`rewards/rewards.json`・§11.2 のフィールド名に合わせる）。
//! - 読み取り用 DTO `RewardStateDto`（`get_reward_state` が返す形）。
//!
//! 仕様:
//! - ランク r に到達したら `unlock_rank <= r` の報酬をすべて解放する（飛び越えても途中の報酬を取りこぼさない）。
//! - 解放済み ID は増えるだけで、未解放へ戻さない（ランクは累計から導出され下がらないが、念のため保証する）。
//! - 解放した報酬は「未確認（pendingRewards）」に積み、`confirm_rank_up_reward` で確認済みにする。
//!   確認済み＝解放済みかつ未確認でないもの（確認済みだけの一覧は持たず、二重管理を避ける）。
//! - 適用中のテーマは既存の設定 `ui.themeId`（selectedThemeId）を正とし、rewards.json には持たない
//!   （§11.2 の `active` は設定と二重管理になるため書かない。詳細は `RewardsState` のコメント）。
//!
//! 時刻は引数で受け取り、本モジュールは純粋に保つ（テスト容易化）。

use serde::{Deserialize, Serialize};

/// 報酬を何も適用していない既定テーマの ID（設定 `ui.themeId` の既定値と一致させる）。
pub const DEFAULT_THEME_ID: &str = "default";

/// 報酬の種類。当面は UI テーマのみ（D34: 素材がそろうまで少数で始める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RewardType {
    Theme,
}

/// 報酬マスタの 1 件（§11.3 の rewardId / type / name / unlockRank に対応）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardDefinition {
    pub reward_id: &'static str,
    pub reward_type: RewardType,
    pub name: &'static str,
    pub unlock_rank: u32,
}

/// 報酬マスタ（アプリ同梱）。
///
/// §11.3 は同梱 JSON（resources/reward_master.json）を想定しているが、リソース同梱設定
/// （tauri.conf.json）を変えずに済み、外部から書き換えられない値を正にできるため、バイナリに埋め込む。
/// ID は §11.3 の命名（theme_001…）に合わせる。表示名は仮の名前で、実際の配色はテーマ設計タスクで決める。
/// 呼び名は解放条件なし（2026-10-05 決定）のため報酬に含めない。
/// ランク 10 の報酬枠は空けておく（中身は後で決める）。他のランクは当面ランクだけ上がる。
pub const REWARD_MASTER: &[RewardDefinition] = &[
    RewardDefinition {
        reward_id: "theme_001",
        reward_type: RewardType::Theme,
        name: "テーマ①",
        unlock_rank: 3,
    },
    RewardDefinition {
        reward_id: "theme_002",
        reward_type: RewardType::Theme,
        name: "テーマ②",
        unlock_rank: 7,
    },
];

/// マスタに存在する報酬か。外部（保存ファイル・フロント）由来の ID を UI へ流す前の検証に使う。
pub fn find_reward(reward_id: &str) -> Option<&'static RewardDefinition> {
    REWARD_MASTER.iter().find(|def| def.reward_id == reward_id)
}

/// 未確認の報酬（§11.2 `pendingRewards` の要素）。通知タスクが「いつ解放されたか」を使えるよう時刻を持つ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct PendingReward {
    pub reward_id: String,
    /// 解放時刻（UTC・"YYYY-MM-DDThh:mm:ssZ"）。
    pub unlocked_at: String,
}

/// 永続化する報酬状態（`rewards/rewards.json`・§11.2）。
///
/// §11.2 の `active`（適用中のテーマ等）は書かない。適用中テーマは既存の設定 `ui.themeId` が
/// 保存・変更（save_user_settings）の正であり、ここにも持つと食い違いが起こり得るため。
/// 未知のフィールドは読み込み時に無視する（将来 `active` 等が書かれていても読み込める）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct RewardsState {
    pub version: u32,
    /// 解放済みの報酬 ID（増えるだけ）。マスタに無い ID も保存データを失わないよう保持する。
    pub unlocked_reward_ids: Vec<String>,
    /// 解放済みだが未確認の報酬。
    pub pending_rewards: Vec<PendingReward>,
}

impl Default for RewardsState {
    fn default() -> Self {
        Self {
            version: 1,
            unlocked_reward_ids: Vec::new(),
            pending_rewards: Vec::new(),
        }
    }
}

impl RewardsState {
    /// rewards.json が無いとき、friendship.json の旧報酬項目から作る（互換読込のみ）。
    ///
    /// 旧項目は「空運用」だったが、万一値があってもマスタにある ID だけを取り込む
    /// （未検証の文字列を新しい保存先へ広げない）。確認済み → 解放済み、未確認 → 解放済みかつ未確認。
    pub fn from_legacy_friendship(
        pending_reward_ids: &[String],
        confirmed_reward_ids: &[String],
        now_rfc3339: &str,
    ) -> Self {
        let mut state = Self::default();
        for id in confirmed_reward_ids {
            if find_reward(id).is_some() && !state.is_unlocked(id) {
                state.unlocked_reward_ids.push(id.clone());
            }
        }
        for id in pending_reward_ids {
            if find_reward(id).is_none() || state.is_unlocked(id) {
                continue;
            }
            state.unlocked_reward_ids.push(id.clone());
            state.pending_rewards.push(PendingReward {
                reward_id: id.clone(),
                unlocked_at: now_rfc3339.to_string(),
            });
        }
        state
    }

    /// 読み込んだ状態の整合を取る（手編集・旧版で崩れていても安全に扱うため）。
    /// 重複を除き、未確認なのに解放済みに無い ID は解放済みへ足す（未解放へ戻す方向には直さない）。
    /// 変更があれば true（呼び出し側が保存する）。
    pub fn normalize(&mut self) -> bool {
        let before = self.clone();
        let mut unlocked: Vec<String> = Vec::with_capacity(self.unlocked_reward_ids.len());
        for id in self.unlocked_reward_ids.drain(..) {
            if !id.trim().is_empty() && !unlocked.contains(&id) {
                unlocked.push(id);
            }
        }
        let mut pending: Vec<PendingReward> = Vec::with_capacity(self.pending_rewards.len());
        for reward in self.pending_rewards.drain(..) {
            if reward.reward_id.trim().is_empty()
                || pending.iter().any(|p| p.reward_id == reward.reward_id)
            {
                continue;
            }
            if !unlocked.contains(&reward.reward_id) {
                unlocked.push(reward.reward_id.clone());
            }
            pending.push(reward);
        }
        self.unlocked_reward_ids = unlocked;
        self.pending_rewards = pending;
        *self != before
    }

    pub fn is_unlocked(&self, reward_id: &str) -> bool {
        self.unlocked_reward_ids.iter().any(|id| id == reward_id)
    }

    pub fn is_pending(&self, reward_id: &str) -> bool {
        self.pending_rewards
            .iter()
            .any(|p| p.reward_id == reward_id)
    }

    /// 現ランク以下で未解放の報酬をすべて解放し、未確認に積む。新たに解放した ID を返す。
    ///
    /// ランクアップ時だけでなく、読み込み時にも呼んで冪等に追いつかせる。累計から導出し直した
    /// ランク（#230 の移行）で既に高ランクの利用者にも、途中の報酬を取りこぼさず渡すため。
    pub fn unlock_up_to_rank(&mut self, rank: u32, now_rfc3339: &str) -> Vec<String> {
        let mut newly_unlocked = Vec::new();
        for def in REWARD_MASTER {
            if def.unlock_rank > rank || self.is_unlocked(def.reward_id) {
                continue;
            }
            self.unlocked_reward_ids.push(def.reward_id.to_string());
            self.pending_rewards.push(PendingReward {
                reward_id: def.reward_id.to_string(),
                unlocked_at: now_rfc3339.to_string(),
            });
            newly_unlocked.push(def.reward_id.to_string());
        }
        newly_unlocked
    }

    /// 指定 ID のうち未確認のものを確認済みにする（未確認一覧から外すだけで、解放済みはそのまま）。
    /// 実際に確認済みにした ID を返す。未確認でない ID は無視する。
    pub fn confirm(&mut self, reward_ids: &[String]) -> Vec<String> {
        let mut confirmed = Vec::new();
        for id in reward_ids {
            if let Some(index) = self.pending_rewards.iter().position(|p| &p.reward_id == id) {
                self.pending_rewards.remove(index);
                confirmed.push(id.clone());
            }
        }
        confirmed
    }

    /// 未確認の報酬 ID（保存順）。
    pub fn pending_reward_ids(&self) -> Vec<String> {
        self.pending_rewards
            .iter()
            .map(|p| p.reward_id.clone())
            .collect()
    }

    /// 適用中として扱ってよいテーマ ID を返す。
    /// 設定の themeId が既定テーマか解放済みのテーマならそのまま、そうでなければ既定へ倒す
    /// （未解放・未知のテーマを適用中として UI に出さない）。
    pub fn effective_theme_id(&self, selected_theme_id: &str) -> String {
        let usable = selected_theme_id == DEFAULT_THEME_ID
            || (find_reward(selected_theme_id)
                .is_some_and(|def| def.reward_type == RewardType::Theme)
                && self.is_unlocked(selected_theme_id));
        if usable {
            selected_theme_id.to_string()
        } else {
            DEFAULT_THEME_ID.to_string()
        }
    }

    /// 読み取り用 DTO へ変換する。UI へはマスタにある報酬だけを渡す。
    pub fn to_dto(&self, current_rank: u32, selected_theme_id: &str) -> RewardStateDto {
        let rewards = REWARD_MASTER
            .iter()
            .map(|def| RewardItemDto {
                reward_id: def.reward_id.to_string(),
                reward_type: def.reward_type,
                name: def.name.to_string(),
                unlock_rank: def.unlock_rank,
                unlocked: self.is_unlocked(def.reward_id),
                pending: self.is_pending(def.reward_id),
            })
            .collect();
        let pending_reward_ids = self
            .pending_rewards
            .iter()
            .filter(|p| find_reward(&p.reward_id).is_some())
            .map(|p| p.reward_id.clone())
            .collect();
        RewardStateDto {
            current_rank,
            rewards,
            pending_reward_ids,
            active_theme_id: self.effective_theme_id(selected_theme_id),
        }
    }
}

/// 報酬 1 件の表示用 DTO。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardItemDto {
    pub reward_id: String,
    #[serde(rename = "type")]
    pub reward_type: RewardType,
    pub name: String,
    pub unlock_rank: u32,
    pub unlocked: bool,
    /// 解放済みだが未確認か。
    pub pending: bool,
}

/// `get_reward_state` の戻り値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardStateDto {
    /// 判定に使った現在のランク（friendship.json の累計から導出）。
    pub current_rank: u32,
    /// 報酬マスタ全件と、それぞれの解放・未確認状態（解放ランク順）。
    pub rewards: Vec<RewardItemDto>,
    /// 未確認の報酬 ID（`confirm_rank_up_reward` に渡す値）。
    pub pending_reward_ids: Vec<String>,
    /// 適用中のテーマ ID（設定 `ui.themeId` が正。未解放なら "default"）。
    pub active_theme_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn master_has_two_themes_at_rank_three_and_seven() {
        let ranks: Vec<(&str, u32)> = REWARD_MASTER
            .iter()
            .map(|def| (def.reward_id, def.unlock_rank))
            .collect();
        assert_eq!(ranks, vec![("theme_001", 3), ("theme_002", 7)]);
        assert!(REWARD_MASTER
            .iter()
            .all(|def| def.reward_type == RewardType::Theme));
        // ランク 10 は当面報酬なし（枠だけ空けておく）。
        assert!(REWARD_MASTER.iter().all(|def| def.unlock_rank != 10));
    }

    #[test]
    fn ranks_without_rewards_unlock_nothing() {
        let mut state = RewardsState::default();
        assert!(state.unlock_up_to_rank(1, "t").is_empty());
        assert!(state.unlock_up_to_rank(2, "t").is_empty());
        assert!(state.unlocked_reward_ids.is_empty());
    }

    #[test]
    fn reaching_rank_three_unlocks_first_theme_as_pending() {
        let mut state = RewardsState::default();
        assert_eq!(state.unlock_up_to_rank(3, "t3"), ids(&["theme_001"]));
        assert!(state.is_unlocked("theme_001"));
        assert_eq!(state.pending_reward_ids(), ids(&["theme_001"]));
        assert_eq!(state.pending_rewards[0].unlocked_at, "t3");
        // 同じランクで呼び直しても増えない（冪等）。
        assert!(state.unlock_up_to_rank(3, "t3b").is_empty());
        assert!(state.unlock_up_to_rank(6, "t6").is_empty());
        assert_eq!(state.unlock_up_to_rank(7, "t7"), ids(&["theme_002"]));
    }

    #[test]
    fn skipping_ranks_unlocks_all_intermediate_rewards() {
        let mut state = RewardsState::default();
        assert_eq!(
            state.unlock_up_to_rank(11, "t"),
            ids(&["theme_001", "theme_002"])
        );
        assert_eq!(state.pending_reward_ids(), ids(&["theme_001", "theme_002"]));
    }

    #[test]
    fn unlocked_rewards_never_relock_even_if_rank_is_lower() {
        let mut state = RewardsState::default();
        state.unlock_up_to_rank(7, "t");
        state.confirm(&ids(&["theme_001"]));
        // ランクが下がったように見えても（不正データ等）解放済みは維持する。
        assert!(state.unlock_up_to_rank(1, "t2").is_empty());
        assert_eq!(state.unlocked_reward_ids, ids(&["theme_001", "theme_002"]));
        assert_eq!(state.pending_reward_ids(), ids(&["theme_002"]));
    }

    #[test]
    fn confirm_removes_only_pending_ids() {
        let mut state = RewardsState::default();
        state.unlock_up_to_rank(7, "t");
        assert_eq!(
            state.confirm(&ids(&["theme_002", "unknown"])),
            ids(&["theme_002"])
        );
        assert_eq!(state.pending_reward_ids(), ids(&["theme_001"]));
        // 確認済みのものを再確認しても何も起きない。
        assert!(state.confirm(&ids(&["theme_002"])).is_empty());
        assert!(state.is_unlocked("theme_002"));
    }

    #[test]
    fn legacy_friendship_ids_are_imported_only_when_known() {
        let state = RewardsState::from_legacy_friendship(
            &ids(&["theme_002", "deco_999"]),
            &ids(&["theme_001", "deco_001"]),
            "t",
        );
        assert_eq!(state.unlocked_reward_ids, ids(&["theme_001", "theme_002"]));
        assert_eq!(state.pending_reward_ids(), ids(&["theme_002"]));
    }

    #[test]
    fn normalize_dedupes_and_keeps_pending_unlocked() {
        let mut state: RewardsState = serde_json::from_str(
            r#"{"version":1,"unlockedRewardIds":["theme_001","theme_001",""],
            "active":{"themeId":"theme_001"},
            "pendingRewards":[{"rewardId":"theme_002","unlockedAt":"t"},{"rewardId":"theme_002","unlockedAt":"t"}]}"#,
        )
        .unwrap();
        assert!(state.normalize());
        assert_eq!(state.unlocked_reward_ids, ids(&["theme_001", "theme_002"]));
        assert_eq!(state.pending_reward_ids(), ids(&["theme_002"]));
        assert!(!state.normalize());
    }

    #[test]
    fn active_theme_falls_back_to_default_unless_unlocked() {
        let mut state = RewardsState::default();
        assert_eq!(state.effective_theme_id("default"), "default");
        assert_eq!(state.effective_theme_id("theme_001"), "default");
        assert_eq!(state.effective_theme_id("<script>"), "default");
        state.unlock_up_to_rank(3, "t");
        assert_eq!(state.effective_theme_id("theme_001"), "theme_001");
        assert_eq!(state.effective_theme_id("theme_002"), "default");
    }

    #[test]
    fn dto_lists_master_with_status_and_hides_unknown_ids() {
        let mut state = RewardsState {
            unlocked_reward_ids: ids(&["mystery"]),
            pending_rewards: vec![PendingReward {
                reward_id: "mystery".to_string(),
                unlocked_at: "t".to_string(),
            }],
            ..RewardsState::default()
        };
        state.unlock_up_to_rank(3, "t");
        let dto = state.to_dto(3, "theme_001");
        assert_eq!(dto.current_rank, 3);
        assert_eq!(dto.rewards.len(), REWARD_MASTER.len());
        assert!(dto.rewards[0].unlocked && dto.rewards[0].pending);
        assert!(!dto.rewards[1].unlocked && !dto.rewards[1].pending);
        assert_eq!(dto.pending_reward_ids, ids(&["theme_001"]));
        assert_eq!(dto.active_theme_id, "theme_001");
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["rewards"][0]["type"], "theme");
        assert_eq!(json["rewards"][0]["unlockRank"], 3);
    }
}
