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

/**
 * ニュース取得成功イベントを購読する。ペイロードは件数のみのため、ハンドラへは何も渡さない
 * （受信側は「取得が終わった」契機としてだけ使う）。非Tauri（ブラウザプレビュー）では何もしない解除関数を返す。
 */
export const listenNewsRefreshed = async (
  handler: () => void
): Promise<() => void> => {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  return listen(NEWS_REFRESHED_EVENT, () => handler());
};
