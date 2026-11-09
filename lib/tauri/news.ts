import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "@/lib/tauri/settings";

export type RefreshError = {
  url: string;
  kind: string;
};

export type RefreshNewsResult = {
  sourcesProcessed: number;
  fetched: number;
  saved: number;
  errors: RefreshError[];
  // 興味ジャンルに合う取得元が無く、すべての取得元から取得したか（D10）。
  genreFilterFallback: boolean;
};

export const refreshNews = async (): Promise<RefreshNewsResult | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<RefreshNewsResult>("refresh_news");
};

/**
 * スケジューラによるニュース取得（refresh）成功時に、Rust がメインウィンドウへ送るイベント名。
 * Rust 側 news_scheduler::NEWS_REFRESHED_EVENT と一致させる。
 */
export const NEWS_REFRESHED_EVENT = "news-refreshed";

/** ニュース取得成功イベントのうち、受信側が使う情報（Rust 側 NewsRefreshedPayload）。 */
export type NewsRefreshedInfo = {
  /** 起動時の取得か（起動直後のホーム一覧はすぐ読み直すために使う）。 */
  startup: boolean;
};

/**
 * ニュース取得成功イベントを購読する。受信側は「取得が終わった」契機と、起動時の取得かだけを使う
 * （件数は渡さない）。非Tauri（ブラウザプレビュー）では何もしない解除関数を返す。
 */
export const listenNewsRefreshed = async (
  handler: (info: NewsRefreshedInfo) => void
): Promise<() => void> => {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  return listen<{ startup?: unknown }>(NEWS_REFRESHED_EVENT, (event) =>
    handler({ startup: event.payload?.startup === true })
  );
};
