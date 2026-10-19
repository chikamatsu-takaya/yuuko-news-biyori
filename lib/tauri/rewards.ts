import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

/**
 * ランク報酬の状態取得（Rust の get_reward_state）の型付きラッパー。
 * 解放判定・保存は Rust 側が行い、ここは結果を受け取るだけ。
 * 確認済みにする操作は lib/tauri/yuuko.ts の confirmRankUpReward（confirm_rank_up_reward）を使う。
 */

/** 報酬の種類（当面は UI テーマのみ）。 */
export type RewardType = "theme";

export type RewardItem = {
  rewardId: string;
  type: RewardType;
  /** 表示名（仮の名前。実際のテーマ名・配色はテーマ設計で決める）。 */
  name: string;
  unlockRank: number;
  unlocked: boolean;
  /** 解放済みだが未確認か。 */
  pending: boolean;
};

export type RewardState = {
  currentRank: number;
  /** 報酬マスタ全件（解放ランク順）。 */
  rewards: RewardItem[];
  /** 未確認の報酬 ID（confirmRankUpReward に渡す値）。 */
  pendingRewardIds: string[];
  /** 適用中のテーマ ID（設定 selectedThemeId が正。未解放・未所持なら "default"）。 */
  activeThemeId: string;
};

export const getRewardState = async (): Promise<RewardState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<RewardState>("get_reward_state");
};

/**
 * 適用中テーマを切り替える（Rust の set_active_theme）。
 * 選べるか（既定・解放済みの報酬テーマ・所持済みのガチャテーマ）は Rust が判定し、選べない ID は reject される。
 * 戻り値は更新後の報酬状態で、activeThemeId をそのまま画面へ適用する。Tauri 外では null。
 */
export const setActiveTheme = async (themeId: string): Promise<RewardState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<RewardState>("set_active_theme", { params: { themeId } });
};
