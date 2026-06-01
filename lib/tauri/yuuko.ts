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
