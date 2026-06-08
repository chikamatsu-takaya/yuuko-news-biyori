import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

export type YuukoResidentState =
  | "Hidden"
  | "Waiting"
  | "Suppressed"
  | "Preparing"
  | "Appearing"
  | "BalloonVisible"
  | "PreviewVisible"
  | "Leaving"
  | "TransitionPending"
  | "RewardNotifying"
  | "Paused";

export type YuukoPositionMode =
  | "RightBottom"
  | "LeftBottom"
  | "RightCenter"
  | "LeftCenter";

export type ArticleSummary = {
  articleId: string;
  title: string;
  sourceName: string;
  publishedAtText: string;
  genre: string;
  summary?: string;
  isFavorite: boolean;
  readState: string;
  recommendationScore: number;
};

export type RewardNotificationState = {
  pending: boolean;
  rank: number;
  rewardIds: string[];
  message: string;
};

export type YuukoNotificationState = {
  state: YuukoResidentState;
  positionMode: YuukoPositionMode;
  balloonText?: string;
  previewArticle?: ArticleSummary;
  hasNotification: boolean;
  currentArticleId?: string;
  rewardNotification?: RewardNotificationState;
};

export type ConfirmRankUpRewardParams = {
  rewardIds: string[];
};

export type ConfirmRankUpRewardResult = {
  ok: boolean;
  confirmedRewardIds: string[];
  remainingPendingRewardIds: string[];
};

export const getYuukoNotificationState =
  async (): Promise<YuukoNotificationState | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<YuukoNotificationState>("get_yuuko_notification_state");
  };

export const confirmRankUpReward = async (
  params: ConfirmRankUpRewardParams
): Promise<ConfirmRankUpRewardResult | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<ConfirmRankUpRewardResult>("confirm_rank_up_reward", { params });
};

// --- 友情ランク（friendship） ---

export type FriendshipState = {
  currentRank: number;
  currentPoint: number;
  nextRequiredPoint: number;
  dailyEarnedPoint: number;
  dailyPointLimit: number;
  lastPointDate?: string;
};

/** 友情ポイント加算イベント種別（Rust側 §10.4 と一致）。 */
export type FriendshipEventType =
  | "yuuko_to_main"
  | "news_detail_opened"
  | "explanation_viewed"
  | "term_explained";

export type RecordFriendshipEventResult = {
  state: FriendshipState;
  /** この加算でランクアップしたか（true のとき RankUpDialog を表示）。 */
  rankedUp: boolean;
  newRank: number;
  /** 実際に加算されたポイント（デイリー上限により 0 のこともある）。 */
  earnedPoint: number;
};

export const getFriendshipState =
  async (): Promise<FriendshipState | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<FriendshipState>("get_friendship_state");
  };

/**
 * 友情ポイント加算イベントを記録する。
 * 上限・有効イベント検証・ランクアップ判定は Rust 側で行われる（フロントは通知のみ）。
 * 非Tauri（ブラウザプレビュー）では null を返す。
 */
export const recordFriendshipEvent = async (
  eventType: FriendshipEventType
): Promise<RecordFriendshipEventResult | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<RecordFriendshipEventResult>("record_friendship_event", {
    params: { eventType },
  });
};
