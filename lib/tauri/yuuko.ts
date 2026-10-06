import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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

/** request_yuuko_notification の結果理由（Rust側と一致）。 */
export type NotificationReason =
  | "notified"
  | "disabled"
  | "reward_pending"
  | "already_active"
  | "daily_limit"
  | "cooling_down"
  | "no_candidate"
  | "outside_time_range"
  // 全画面・プレゼン中（設計書 §5.2）/ 解除後 30〜180 秒の猶予中（§5.4）。
  | "fullscreen"
  | "fullscreen_grace";

export type RequestYuukoNotificationResult = {
  notified: boolean;
  reason: NotificationReason;
  state: YuukoNotificationState;
};

/** ゆうこの通知を閉じる（Rust側でクールタイム設定・報酬は保持）。非Tauriは null。 */
export const dismissYuukoNotification =
  async (): Promise<YuukoNotificationState | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<YuukoNotificationState>("dismiss_yuuko_notification");
  };

/** ゆうこクリックの2段階遷移（1回目=軽量プレビュー、2回目=確定）。非Tauriは null。 */
export const handleYuukoClicked =
  async (): Promise<YuukoNotificationState | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<YuukoNotificationState>("handle_yuuko_clicked");
  };

/**
 * ゆうこにニュース通知を出させる。抑制条件（日次上限・クールタイム等）と候補選定は Rust 側。
 * 通知が出たか・理由・最新状態を返す。非Tauriは null。
 */
export const requestYuukoNotification =
  async (): Promise<RequestYuukoNotificationResult | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<RequestYuukoNotificationResult>("request_yuuko_notification");
  };

// --- 常駐ゆうこ用ウィンドウ（デスクトップ通知） ---

/**
 * メインウィンドウ非表示・最小化中に、Rust の判定スレッドがゆうこ用ウィンドウへ送るイベント名。
 * Rust 側 yuuko_desktop_notifier::YUUKO_DESKTOP_NOTIFICATION_EVENT と一致させる。
 */
export const YUUKO_DESKTOP_NOTIFICATION_EVENT = "yuuko-desktop-notification";

/**
 * ゆうこ用ウィンドウへ渡す最小限の表示用データ（Rust YuukoDesktopNotification と一致）。
 * title / balloonText は外部由来を含み得るため、必ずテキストとして描画する。
 */
export type YuukoDesktopNotification = {
  articleId: string;
  title: string;
  balloonText?: string;
  /**
   * 既に軽量プレビュー段階（PreviewVisible）か。吹き出しからやり直すと、
   * 次のクリックが Rust 側で「確定（記事を開く）」扱いになりずれるため、段階を合わせる。
   */
  previewVisible: boolean;
};

/**
 * 通知状態から、ゆうこ用ウィンドウの表示用データを取り出す（Rust desktop_notification_from_state と同基準）。
 *
 * ウィンドウ初回生成時はページの購読開始より先にイベントが送られ得るため、
 * ゆうこ用ウィンドウはマウント時に getYuukoNotificationState の結果をこれで変換して初期表示に使う。
 * active なニュース通知でない、またはタイトルが無い場合は null（表示しない）。
 */
export const toYuukoDesktopNotification = (
  state: YuukoNotificationState | null
): YuukoDesktopNotification | null => {
  if (
    !state ||
    !["Appearing", "BalloonVisible", "PreviewVisible"].includes(state.state) ||
    !state.previewArticle
  ) {
    return null;
  }
  return {
    articleId: state.currentArticleId ?? state.previewArticle.articleId,
    title: state.previewArticle.title,
    ...(state.balloonText ? { balloonText: state.balloonText } : {}),
    previewVisible: state.state === "PreviewVisible",
  };
};

/**
 * ゆうこ用ウィンドウで、Rust から届くデスクトップ通知イベントを購読する。
 * 非Tauri（ブラウザプレビュー）では何もしない解除関数を返す。
 */
export const listenYuukoDesktopNotification = async (
  handler: (notification: YuukoDesktopNotification) => void
): Promise<() => void> => {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  return listen<YuukoDesktopNotification>(
    YUUKO_DESKTOP_NOTIFICATION_EVENT,
    (event) => handler(event.payload)
  );
};

/**
 * ゆうこ用ウィンドウの「詳しく見る」確定時に、Rust がメインウィンドウへ送るイベント名。
 * Rust 側 yuuko_desktop_notifier::YUUKO_OPEN_ARTICLE_EVENT と一致させる。
 */
export const YUUKO_OPEN_ARTICLE_EVENT = "yuuko-open-article";

/** メインウィンドウで開く記事（Rust が永続状態の紹介中記事から決める）。 */
export type YuukoOpenArticleRequest = {
  articleId: string;
};

/**
 * メインウィンドウで「ゆうこ用ウィンドウから記事を開く」要求を購読する。
 * 記事IDは Rust の状態由来だが、念のため空でない文字列だけを渡す。非Tauriでは何もしない。
 */
export const listenYuukoOpenArticle = async (
  handler: (articleId: string) => void
): Promise<() => void> => {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  return listen<YuukoOpenArticleRequest>(YUUKO_OPEN_ARTICLE_EVENT, (event) => {
    const articleId = event.payload?.articleId;
    if (typeof articleId === "string" && articleId.length > 0) {
      handler(articleId);
    }
  });
};

/** 無操作タイムアウト（無視）を記録する。自動退場タイマー側から呼ぶ。非Tauriは null。 */
export const markYuukoIgnored =
  async (): Promise<YuukoNotificationState | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<YuukoNotificationState>("mark_yuuko_ignored");
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
