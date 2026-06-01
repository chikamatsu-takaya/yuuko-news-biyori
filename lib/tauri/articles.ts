import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

export type ArticleReadState = "unread" | "previewed" | "detail_viewed";

export type ArticleSummaryDto = {
  articleId: string;
  title: string;
  sourceName: string;
  publishedAtText: string;
  genre: string;
  summary?: string;
  isFavorite: boolean;
  readState: ArticleReadState;
  recommendationScore: number;
};

export type GetRecommendedArticlesParams = {
  limit?: number;
};

export const getRecommendedArticles = async (
  params: GetRecommendedArticlesParams = {}
): Promise<ArticleSummaryDto[] | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<ArticleSummaryDto[]>("get_recommended_articles", { params });
};
