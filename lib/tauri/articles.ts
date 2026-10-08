import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

export type ArticleReadState = "unread" | "previewed" | "detail_viewed";

// 自動要約の状態（Rust の SummaryState と一致させる）。
// done は記事ファイルの要約済みフラグ、waiting / processing / failed は自動要約キューのメモリ上の状態。
// none は未要約でキューに入っていない記事（自動要約が無効のときなど）。
export type ArticleSummaryState =
  | "none"
  | "waiting"
  | "processing"
  | "done"
  | "failed";

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
  summaryState: ArticleSummaryState;
};

export type ArticleHistoryFilter =
  | "all"
  | "unread"
  | "read"
  | "favorite"
  | "archived";

export type ArticleHistoryItemDto = {
  articleId: string;
  title: string;
  sourceName: string;
  publishedAtText: string;
  fetchedAt: string;
  genre: string;
  summary?: string;
  isFavorite: boolean;
  readState: ArticleReadState;
  isArchived: boolean;
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
  summaryState: ArticleSummaryState;
};

export type GetRecommendedArticlesParams = {
  limit?: number;
};

export type ListArticleHistoryParams = {
  limit?: number;
  filter?: ArticleHistoryFilter;
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

export type RestoreArchivedArticleParams = {
  articleId: string;
};

export type RestoreArchivedArticleResult = {
  articleId: string;
  status: "restored" | "already_available";
};

export type ArchiveRetirementSummaryDto = {
  retiredArticleCount: number;
  retiredMonths: string[];
  cleanupPending: boolean;
};

export const getRecommendedArticles = async (
  params: GetRecommendedArticlesParams = {}
): Promise<ArticleSummaryDto[] | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<ArticleSummaryDto[]>("get_recommended_articles", { params });
};

export const listArticleHistory = async (
  params: ListArticleHistoryParams = {}
): Promise<ArticleHistoryItemDto[] | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<ArticleHistoryItemDto[]>("list_article_history", { params });
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

// 「元記事を開く」。記事IDだけを渡し、保存済み URL の検証と既定のブラウザ起動は Rust 側で行う（判断台帳 D13）。
// 画面から URL を渡さないため、任意の URL を開く入口にはしない。
// ブラウザでのプレビュー（Tauri 外）では Rust を呼べないため、開発・E2E 用に画面が持つ URL を新しいタブで開く。
export const openOriginalArticle = async (
  articleId: string,
  previewFallbackUrl?: string
): Promise<void> => {
  if (!isTauriRuntime()) {
    if (previewFallbackUrl) {
      window.open(previewFallbackUrl, "_blank", "noopener,noreferrer");
    }
    return;
  }

  await invoke<void>("open_original_article", { params: { articleId } });
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

export const restoreArchivedArticle = async (
  params: RestoreArchivedArticleParams
): Promise<RestoreArchivedArticleResult> => {
  if (!isTauriRuntime()) {
    return { articleId: params.articleId, status: "already_available" };
  }

  return invoke<RestoreArchivedArticleResult>("restore_archived_article", {
    params,
  });
};

export const retireArchivedMarkdown = async (): Promise<ArchiveRetirementSummaryDto> => {
  if (!isTauriRuntime()) {
    return { retiredArticleCount: 0, retiredMonths: [], cleanupPending: false };
  }

  return invoke<ArchiveRetirementSummaryDto>("retire_archived_markdown");
};
