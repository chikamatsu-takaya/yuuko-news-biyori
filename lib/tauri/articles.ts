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

export type ArticleDetailDto = {
  articleId: string;
  title: string;
  sourceName: string;
  originalUrl: string;
  publishedAtText: string;
  genre: string;
  summary?: string;
  excerpt?: string;
  yuukoExplanation?: string;
  focusPoints: string[];
  yuukoComment?: string;
  isFavorite: boolean;
  keywordCandidates: string[];
};

export type GetRecommendedArticlesParams = {
  limit?: number;
};

export type GetArticleDetailParams = {
  articleId: string;
};

export type UpdateArticleFavoriteParams = {
  articleId: string;
  isFavorite: boolean;
};

export type FavoriteUpdateResult = {
  articleId: string;
  isFavorite: boolean;
};

export type GenerateArticleSummaryParams = {
  articleId: string;
};

export type GeneratedArticleSummaryDto = {
  articleId: string;
  summary: string;
  yuukoExplanation: string;
  focusPoints: string[];
  yuukoComment: string;
};

export const getRecommendedArticles = async (
  params: GetRecommendedArticlesParams = {}
): Promise<ArticleSummaryDto[] | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<ArticleSummaryDto[]>("get_recommended_articles", { params });
};

export const getArticleDetail = async (
  params: GetArticleDetailParams
): Promise<ArticleDetailDto | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<ArticleDetailDto>("get_article_detail", { params });
};

export const updateArticleFavorite = async (
  params: UpdateArticleFavoriteParams
): Promise<FavoriteUpdateResult> => {
  if (!isTauriRuntime()) {
    return params;
  }

  return invoke<FavoriteUpdateResult>("update_article_favorite", { params });
};

export const generateArticleSummary = async (
  params: GenerateArticleSummaryParams
): Promise<GeneratedArticleSummaryDto | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<GeneratedArticleSummaryDto>("generate_article_summary", {
    params,
  });
};
