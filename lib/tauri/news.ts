import { invoke } from "@tauri-apps/api/core";
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
