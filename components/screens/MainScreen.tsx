"use client";

import React from "react";
import Image from "next/image";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import { Spinner } from "@/components/ui/spinner";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import {
  Home,
  Newspaper,
  History,
  BookOpen,
  Palette,
  Gift,
  Settings,
  Star,
  Plus,
  Bell,
  HelpCircle,
  ChevronRight,
  Sparkles,
  PlayCircle,
  RefreshCw,
  Info,
} from "lucide-react";
import {
  getRecommendedArticles,
  updateArticleFavorite,
  type ArticleSummaryDto as TauriArticleSummary,
} from "@/lib/tauri/articles";
import {
  refreshNews,
  type RefreshNewsResult as TauriRefreshNewsResult,
} from "@/lib/tauri/news";
import { getYuukoNotificationState } from "@/lib/tauri/yuuko";
import { useToast } from "@/hooks/use-toast";

// ============================================
// TypeScript Types
// ============================================

type NavigationItem = {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive?: boolean;
};

type Article = {
  id: string;
  category: string;
  categoryColor: string;
  title: string;
  description: string;
  source: string;
  timeAgo: string;
  isNew: boolean;
  isFavorite: boolean;
  thumbnailType: "ai" | "business" | "quantum";
};

type QuickAccessItem = {
  id: string;
  label: string;
  icon: React.ElementType;
};

type UserStats = {
  rank: number;
  currentExp: number;
  maxExp: number;
  starFragments: number;
  unclaimedRewards: number;
};

type RefreshMetricProps = {
  label: string;
  value: number;
};

// ============================================
// Mock Data
// ============================================

const mockNavigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: true },
  { id: "news", label: "ニュースを見る", icon: Newspaper },
  { id: "history", label: "ニュース履歴", icon: History },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen },
  { id: "customize", label: "カスタマイズ", icon: Palette },
  { id: "gacha", label: "ガチャ", icon: Gift },
  { id: "settings", label: "設定", icon: Settings },
];

const fallbackMockArticles: Article[] = [
  {
    id: "article-001",
    category: "AI・テクノロジー",
    categoryColor: "bg-blue-500",
    title: "生成AIが変えるソフトウェア開発の未来",
    description:
      "AIがコードの自動生成やレビューを支援し、開発効率が大きく向上。今後のエンジニアの役割も変わっていくかもしれません。",
    source: "TechCrunch Japan",
    timeAgo: "5分前",
    isNew: true,
    isFavorite: false,
    thumbnailType: "ai",
  },
  {
    id: "article-002",
    category: "ビジネス",
    categoryColor: "bg-emerald-500",
    title: "国内スタートアップの資金調達、過去最高に",
    description:
      "2024年の資金調達額は前年比120%増。AI・SaaS領域を中心に大型調達が相次いでいます。",
    source: "日経ビジネス",
    timeAgo: "1時間前",
    isNew: true,
    isFavorite: false,
    thumbnailType: "business",
  },
  {
    id: "article-003",
    category: "テクノロジー",
    categoryColor: "bg-purple-500",
    title: "量子コンピュータの実用化に向けた新たな一歩",
    description:
      "エラー耐性の高い量子ビットの実現に成功。産業応用が現実味を帯びてきました。",
    source: "ITmedia NEWS",
    timeAgo: "2時間前",
    isNew: true,
    isFavorite: false,
    thumbnailType: "quantum",
  },
];

const mockQuickAccessItems: QuickAccessItem[] = [
  { id: "news", label: "ニュースを見る", icon: Newspaper },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen },
  { id: "history", label: "ニュース履歴", icon: History },
  { id: "customize", label: "カスタマイズ", icon: Palette },
  { id: "gacha", label: "ガチャ", icon: Gift },
  { id: "settings", label: "設定", icon: Settings },
];

const mockUserStats: UserStats = {
  rank: 15,
  currentExp: 350,
  maxExp: 1000,
  starFragments: 1250,
  unclaimedRewards: 2,
};

const fallbackYuukoMessage = `ふむふむ…
これなんか
おもしろそうだよ〜！
気になるのあったら
教えてねっ♪`;

const mockYuukoComment = `今日もいろんな
ニュースがあるよ〜！
気になるの、
一緒に見てこっ♪`;

const fallbackStatusMessage = "新しいニュースが3件届いてるよ！";

const toCategoryColor = (genre: string): string => {
  if (genre.includes("AI")) {
    return "bg-blue-500";
  }
  if (genre.includes("ビジネス")) {
    return "bg-emerald-500";
  }
  return "bg-purple-500";
};

const toThumbnailType = (genre: string): Article["thumbnailType"] => {
  if (genre.includes("AI")) {
    return "ai";
  }
  if (genre.includes("ビジネス")) {
    return "business";
  }
  return "quantum";
};

const mapTauriArticleToUi = (article: TauriArticleSummary): Article => ({
  id: article.articleId,
  category: article.genre,
  categoryColor: toCategoryColor(article.genre),
  title: article.title,
  description: article.summary ?? "要約は準備中だよ。気になったら開いてみてね。",
  source: article.sourceName,
  timeAgo: article.publishedAtText,
  isNew: article.readState === "unread",
  isFavorite: article.isFavorite,
  thumbnailType: toThumbnailType(article.genre),
});

const toRefreshErrorLabel = (kind: string): string => {
  switch (kind) {
    case "feed_url_rejected":
      return "フィードURL拒否";
    case "feed_fetch_failed":
      return "フィード取得失敗";
    case "article_url_rejected":
      return "記事URL拒否";
    case "article_fetch_failed":
      return "記事取得失敗";
    default:
      return kind;
  }
};

const summarizeRefreshErrors = (
  result: TauriRefreshNewsResult
): string | null => {
  if (result.errors.length === 0) {
    return null;
  }

  const kinds = Array.from(
    new Set(result.errors.map((error) => toRefreshErrorLabel(error.kind)))
  );

  return `エラー ${result.errors.length} 件（${kinds.join(" / ")}）`;
};

// ============================================
// Sub Components
// ============================================

function PawIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      className={className}
      xmlns="http://www.w3.org/2000/svg"
    >
      <ellipse cx="12" cy="16" rx="5" ry="4" />
      <circle cx="6" cy="10" r="2.5" />
      <circle cx="18" cy="10" r="2.5" />
      <circle cx="9" cy="6" r="2" />
      <circle cx="15" cy="6" r="2" />
    </svg>
  );
}

function ThumbnailPlaceholder({ type }: { type: Article["thumbnailType"] }) {
  const configs = {
    ai: {
      gradient: "from-blue-400 to-cyan-400",
      icon: (
        <svg
          viewBox="0 0 64 64"
          className="w-12 h-12 text-white/90"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
        >
          <ellipse cx="32" cy="32" rx="20" ry="16" />
          <path d="M20 28 Q32 20 44 28" />
          <path d="M20 36 Q32 44 44 36" />
          <circle cx="24" cy="32" r="2" fill="currentColor" />
          <circle cx="40" cy="32" r="2" fill="currentColor" />
          <path d="M12 20 L8 12" />
          <path d="M52 20 L56 12" />
          <path d="M12 44 L8 52" />
          <path d="M52 44 L56 52" />
        </svg>
      ),
    },
    business: {
      gradient: "from-emerald-400 to-green-500",
      icon: (
        <svg
          viewBox="0 0 64 64"
          className="w-12 h-12 text-white/90"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
        >
          <path d="M12 52 L12 36 L20 36 L20 52" fill="currentColor" />
          <path d="M24 52 L24 28 L32 28 L32 52" fill="currentColor" />
          <path d="M36 52 L36 20 L44 20 L44 52" fill="currentColor" />
          <path d="M48 52 L48 12 L56 12 L56 52" fill="currentColor" />
          <path d="M8 52 L60 52" strokeWidth="3" />
          <path d="M10 40 Q32 8 58 16" strokeDasharray="4 2" />
        </svg>
      ),
    },
    quantum: {
      gradient: "from-purple-500 to-indigo-500",
      icon: (
        <svg
          viewBox="0 0 64 64"
          className="w-12 h-12 text-white/90"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
        >
          <ellipse cx="32" cy="32" rx="24" ry="8" />
          <ellipse
            cx="32"
            cy="32"
            rx="24"
            ry="8"
            transform="rotate(60 32 32)"
          />
          <ellipse
            cx="32"
            cy="32"
            rx="24"
            ry="8"
            transform="rotate(120 32 32)"
          />
          <circle cx="32" cy="32" r="4" fill="currentColor" />
          <circle cx="32" cy="24" r="2" fill="currentColor" />
          <circle cx="32" cy="40" r="2" fill="currentColor" />
          <circle cx="24" cy="28" r="2" fill="currentColor" />
          <circle cx="40" cy="36" r="2" fill="currentColor" />
        </svg>
      ),
    },
  };

  const config = configs[type];

  return (
    <div
      className={`w-24 h-20 rounded-lg bg-gradient-to-br ${config.gradient} flex items-center justify-center shrink-0`}
    >
      {config.icon}
    </div>
  );
}

function ArticleCard({
  article,
  onClick,
  onToggleFavorite,
  isFavoriteSaving,
}: {
  article: Article;
  onClick: (id: string) => void;
  onToggleFavorite: (id: string, nextValue: boolean) => void;
  isFavoriteSaving: boolean;
}) {
  return (
    <Card
      className="border-0 shadow-sm hover:shadow-md transition-shadow py-3 px-0 cursor-pointer"
      onClick={() => onClick(article.id)}
    >
      <CardContent className="p-4 flex gap-4">
        <ThumbnailPlaceholder type={article.thumbnailType} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1.5">
            <Badge
              className={`${article.categoryColor} text-white border-0 text-[10px] px-2 py-0`}
            >
              {article.category}
            </Badge>
            {article.isNew && (
              <Badge className="bg-red-500 text-white border-0 text-[10px] px-1.5 py-0">
                NEW
              </Badge>
            )}
          </div>
          <h3 className="font-semibold text-sm text-foreground mb-1 line-clamp-1">
            {article.title}
          </h3>
          <p className="text-xs text-muted-foreground line-clamp-2 mb-2">
            {article.description}
          </p>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
              <span>{article.source}</span>
              <span>・</span>
              <span>{article.timeAgo}</span>
            </div>
            <button
              className="text-muted-foreground hover:text-yellow-500 transition-colors disabled:cursor-not-allowed disabled:opacity-50"
              disabled={isFavoriteSaving}
              onClick={(e) => {
                e.stopPropagation();
                onToggleFavorite(article.id, !article.isFavorite);
              }}
            >
              <Star
                className={`w-4 h-4 ${article.isFavorite ? "fill-yellow-500 text-yellow-500" : ""}`}
              />
            </button>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function QuickAccessButton({
  item,
  onNavigate,
}: {
  item: QuickAccessItem;
  onNavigate: (id: string) => void;
}) {
  const Icon = item.icon;
  return (
    <button
      className="flex flex-col items-center justify-center gap-1.5 p-3 rounded-xl bg-white hover:bg-muted border border-border/50 transition-all hover:shadow-sm"
      onClick={() => onNavigate(item.id)}
    >
      <Icon className="w-5 h-5 text-muted-foreground" />
      <span className="text-[11px] text-muted-foreground">{item.label}</span>
    </button>
  );
}

function RefreshMetric({ label, value }: RefreshMetricProps) {
  return (
    <div className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-center">
      <div className="text-[10px] uppercase tracking-[0.12em] text-muted-foreground">
        {label}
      </div>
      <div className="text-sm font-semibold text-foreground">{value}</div>
    </div>
  );
}

function YuukoSpeechBubble({ message }: { message: string }) {
  return (
    <div className="relative bg-white rounded-2xl px-4 py-3 shadow-md border border-border/50 max-w-[180px]">
      <p className="text-xs text-foreground whitespace-pre-line leading-relaxed">
        {message}
      </p>
      {/* Speech bubble tail */}
      <div className="absolute -right-2 bottom-4 w-4 h-4 bg-white border-r border-b border-border/50 transform rotate-[-45deg]" />
    </div>
  );
}

function YuukoCharacter() {
  return (
    <div className="relative">
      <Image
        src="/assets/yuuko.png"
        width={224}
        height={224}
        alt="ゆうこ"
        className="w-56 h-auto drop-shadow-lg animate-float"
        onError={(e) => {
          // Fallback to placeholder if image fails to load
          const target = e.target as HTMLImageElement;
          target.style.display = "none";
          target.nextElementSibling?.classList.remove("hidden");
        }}
      />
      {/* Fallback placeholder */}
      <div className="hidden w-56 h-56 bg-gradient-to-b from-gray-800 to-gray-900 rounded-full flex items-center justify-center">
        <PawIcon className="w-20 h-20 text-pink-300" />
      </div>
    </div>
  );
}

// ============================================
// Main Component
// ============================================

export default function MainScreen({
  onNavigate,
  onOpenArticle,
}: {
  onNavigate?: (screen: string) => void;
  onOpenArticle?: (articleId: string) => void;
}) {
  const [isAutoStart] = React.useState(true);
  const [articles, setArticles] = React.useState<Article[]>(fallbackMockArticles);
  const [favoriteSavingArticleId, setFavoriteSavingArticleId] =
    React.useState<string | null>(null);
  const [articleNotice, setArticleNotice] = React.useState<string | null>(null);
  const [refreshNotice, setRefreshNotice] = React.useState<string | null>(null);
  const [refreshResult, setRefreshResult] =
    React.useState<TauriRefreshNewsResult | null>(null);
  const [isRefreshingNews, setIsRefreshingNews] = React.useState(false);
  const [isLoadingArticles, setIsLoadingArticles] = React.useState(true);
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);
  const [loadNoticeKind, setLoadNoticeKind] = React.useState<"info" | "error">(
    "info"
  );
  const [yuukoBalloonMessage, setYuukoBalloonMessage] =
    React.useState(fallbackYuukoMessage);
  const [statusMessage, setStatusMessage] = React.useState(fallbackStatusMessage);

  const { toast } = useToast();

  React.useEffect(() => {
    let active = true;

    const loadYuukoNotificationState = async () => {
      try {
        const state = await getYuukoNotificationState();
        if (!active || !state) {
          return;
        }

        const rewardMessage =
          state.rewardNotification?.pending && state.rewardNotification.message
            ? state.rewardNotification.message
            : undefined;
        setYuukoBalloonMessage(
          rewardMessage ?? state.balloonText ?? fallbackYuukoMessage
        );

        if (state.rewardNotification?.pending) {
          setStatusMessage(
            `未確認の報酬が${state.rewardNotification.rewardIds.length}件あるよ！`
          );
          return;
        }

        if (state.hasNotification) {
          setStatusMessage("新しいニュース通知があるよ！");
          return;
        }

        if (state.state === "Suppressed") {
          setStatusMessage("通知はOFF中だよ。設定からいつでも変更できるよ。");
          return;
        }

        setStatusMessage(fallbackStatusMessage);
      } catch (error) {
        console.warn("Failed to load yuuko notification state:", error);
      }
    };

    void loadYuukoNotificationState();

    return () => {
      active = false;
    };
  }, []);

  const loadRecommendedArticles = React.useCallback(async (): Promise<boolean> => {
    setIsLoadingArticles(true);
    setLoadNotice(null);
    try {
      const recommendedArticles = await getRecommendedArticles({ limit: 10 });
      if (!recommendedArticles) {
        return false;
      }

      if (recommendedArticles.length > 0) {
        setArticles(recommendedArticles.map(mapTauriArticleToUi));
      }

      return true;
    } catch (error) {
      setLoadNotice("おすすめニュースの読み込みに失敗しちゃった。少し待ってから、もう一度試してみてね。");
      setLoadNoticeKind("error");
      console.warn("Failed to load recommended articles:", error);
      return false;
    } finally {
      setIsLoadingArticles(false);
    }
  }, []);

  React.useEffect(() => {
    void loadRecommendedArticles();
  }, [loadRecommendedArticles]);

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const handleArticleClick = (articleId: string) => {
    if (onOpenArticle) {
      onOpenArticle(articleId);
      return;
    }

    if (onNavigate) {
      onNavigate("news");
    }
  };

  const handleToggleFavorite = React.useCallback(
    async (articleId: string, nextValue: boolean) => {
      const previousArticles = articles;

      setFavoriteSavingArticleId(articleId);
      setArticleNotice(null);
      setArticles((currentArticles) =>
        currentArticles.map((article) =>
          article.id === articleId
            ? {
                ...article,
                isFavorite: nextValue,
              }
            : article
        )
      );

      try {
        const result = await updateArticleFavorite({
          articleId,
          isFavorite: nextValue,
        });

        setArticles((currentArticles) =>
          currentArticles.map((article) =>
            article.id === result.articleId
              ? {
                  ...article,
                  isFavorite: result.isFavorite,
                }
              : article
          )
        );
      } catch (error) {
        setArticles(previousArticles);
        setArticleNotice(
          "お気に入りの更新に失敗しました。時間をおいてもう一度お試しください。"
        );
        toast({
          variant: "destructive",
          title: "更新に失敗しちゃった",
          description: "お気に入りの更新ができなかったよ。もう一度試してみてね。",
        });
        console.warn("Failed to update article favorite:", error);
      } finally {
        setFavoriteSavingArticleId(null);
      }
    },
    [articles, toast]
  );

  const handleRefreshNews = React.useCallback(async () => {
    setIsRefreshingNews(true);
    setRefreshNotice(null);

    try {
      const result = await refreshNews();
      if (!result) {
        setRefreshResult(null);
        setRefreshNotice(
          "ニュース更新はTauriデスクトップ実行時にのみ利用できます。"
        );
        return;
      }

      setRefreshResult(result);
      const reloaded = await loadRecommendedArticles();
      if (!reloaded) {
        setRefreshNotice(
          "更新は完了しましたが、一覧の再読み込みはできませんでした。"
        );
        return;
      }

      if (result.errors.length === 0) {
        setRefreshNotice("ニュースを更新しました。");
        return;
      }

      setRefreshNotice("ニュース更新は完了しましたが、一部ソースでエラーがありました。");
    } catch (error) {
      setRefreshResult(null);
      setRefreshNotice(
        "ニュース更新に失敗しました。設定ファイルやネットワークを確認してください。"
      );
      toast({
        variant: "destructive",
        title: "更新に失敗しちゃった",
        description: "ニュースの取得ができなかったよ。ネットワークを確認してみてね。",
      });
      console.warn("Failed to refresh news:", error);
    } finally {
      setIsRefreshingNews(false);
    }
  }, [loadRecommendedArticles, toast]);

  return (
    <div className="h-dvh w-full overflow-hidden bg-[var(--yuuko-cream)] flex flex-col">
      <AppTitleBar className="bg-white border-border/50" />

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar */}
        <aside className="w-52 bg-white border-r border-border/50 flex flex-col shrink-0">
          {/* Navigation */}
          <nav className="flex-1 p-3 space-y-1 overflow-y-auto">
            {mockNavigationItems.map((item) => (
              <SidebarNavItem
                key={item.id}
                label={item.label}
                icon={item.icon}
                isActive={item.isActive}
                onClick={() => handleNavigate(item.id)}
              />
            ))}
          </nav>

          {/* Yuuko's Comment Card */}
          <div className="px-3 pb-3">
            <Card className="bg-white border border-[var(--yuuko-green)]/20 py-3">
              <CardContent className="p-3">
                <div className="flex items-center gap-1.5 mb-2">
                  <span className="text-xs font-medium text-[var(--yuuko-green)]">
                    ゆうこの一言
                  </span>
                </div>
                <p className="text-xs text-muted-foreground whitespace-pre-line leading-relaxed">
                  {mockYuukoComment}
                </p>
                <div className="flex justify-end mt-2">
                  <PawIcon className="w-4 h-4 text-pink-300" />
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Auto Start & Exit */}
          <div className="p-3 border-t border-border/50">
            <div className="flex items-center gap-2 mb-2">
              <span className="text-xs text-muted-foreground">自動起動：</span>
              <span
                className={`text-xs font-medium ${isAutoStart ? "text-[var(--yuuko-green)]" : "text-muted-foreground"}`}
              >
                {isAutoStart ? "ON" : "OFF"}
              </span>
              <span
                className={`w-2 h-2 rounded-full ${isAutoStart ? "bg-[var(--yuuko-green)]" : "bg-muted-foreground"}`}
              />
            </div>
            <Button
              variant="outline"
              size="sm"
              className="w-full text-xs h-8"
              onClick={() => console.log("Exit resident mode")}
            >
              常駐を終了する
            </Button>
          </div>
        </aside>

        {/* Center Content */}
        <main className="flex-1 min-w-0 flex flex-col overflow-hidden relative">
          {/* News Section */}
          <div className="flex-1 min-h-0 flex flex-col p-6 pb-0">
            <div className="shrink-0">
              <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
                <h2 className="text-lg font-semibold text-foreground flex items-center gap-2">
                  <Sparkles className="w-5 h-5 text-yellow-500" />
                  今日のおすすめニュース
                </h2>
                <div className="flex items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    className="text-xs"
                    onClick={() => void handleRefreshNews()}
                    disabled={isRefreshingNews}
                  >
                    {isRefreshingNews ? (
                      <Spinner className="size-4" />
                    ) : (
                      <RefreshCw className="w-4 h-4" />
                    )}
                    {isRefreshingNews ? "更新中..." : "ニュースを更新"}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-xs text-muted-foreground hover:text-foreground"
                    onClick={() => console.log("View all news")}
                  >
                    すべて見る
                    <ChevronRight className="w-4 h-4 ml-1" />
                  </Button>
                </div>
              </div>

              {loadNotice && (
                <Alert role="presentation" className="mb-4 border-[var(--yuuko-green)]/30 bg-white shadow-sm">
                  <Info className="h-4 w-4 text-[var(--yuuko-green)]" aria-hidden="true" />
                  <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green)]">お知らせ</AlertTitle>
                  <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
                    <span role={loadNoticeKind === "error" ? "alert" : "status"}>
                      {loadNotice}
                    </span>
                    {loadNoticeKind === "error" && (
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
                        onClick={() => void loadRecommendedArticles()}
                        disabled={isLoadingArticles}
                      >
                        再試行
                      </Button>
                    )}
                  </AlertDescription>
                </Alert>
              )}

              {articleNotice ? (
                <p className="mb-3 text-xs text-amber-700">{articleNotice}</p>
              ) : null}

              {refreshNotice ? (
                <p className="mb-3 text-xs text-[var(--yuuko-green)]">
                  {refreshNotice}
                </p>
              ) : null}

              {refreshResult ? (
                <Card className="mb-4 border border-[var(--yuuko-green)]/20 bg-white/90 py-0 shadow-none">
                  <CardContent className="flex flex-col gap-3 p-4">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <p className="text-sm font-medium text-foreground">
                          取得結果を反映しました
                        </p>
                        <p className="text-xs text-muted-foreground">
                          {refreshResult.errors.length === 0
                            ? "すべての対象ソースを処理できました。"
                            : summarizeRefreshErrors(refreshResult)}
                        </p>
                      </div>
                      <Badge
                        className={
                          refreshResult.errors.length === 0
                            ? "border-0 bg-[var(--yuuko-green)] text-white"
                            : "border-0 bg-amber-500 text-white"
                        }
                      >
                        {refreshResult.errors.length === 0
                          ? "更新成功"
                          : "一部エラーあり"}
                      </Badge>
                    </div>

                    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                      <RefreshMetric
                        label="sources"
                        value={refreshResult.sourcesProcessed}
                      />
                      <RefreshMetric label="fetched" value={refreshResult.fetched} />
                      <RefreshMetric label="saved" value={refreshResult.saved} />
                      <RefreshMetric
                        label="errors"
                        value={refreshResult.errors.length}
                      />
                    </div>
                  </CardContent>
                </Card>
              ) : null}
            </div>

            <div className="flex-1 min-h-0 overflow-y-auto space-y-3 pr-1">
              {articles.map((article) => (
                <ArticleCard
                  key={article.id}
                  article={article}
                  onClick={handleArticleClick}
                  onToggleFavorite={handleToggleFavorite}
                  isFavoriteSaving={favoriteSavingArticleId === article.id}
                />
              ))}
            </div>
          </div>

          {/* Yuuko Character Area */}
          <div className="shrink-0 relative flex items-end justify-center pb-4 pt-2">
            <div className="flex items-end gap-2">
              <YuukoSpeechBubble message={yuukoBalloonMessage} />
              <YuukoCharacter />
            </div>
          </div>
        </main>

        {/* Right Sidebar */}
        <aside className="w-64 p-4 space-y-4 overflow-y-auto shrink-0">
          {/* Friendship Rank Card */}
          <Card className="bg-[var(--yuuko-green)] text-white border-0 py-4">
            <CardContent className="p-4">
              <div className="flex items-center gap-2 mb-3">
                <svg
                  viewBox="0 0 24 24"
                  className="w-5 h-5"
                  fill="currentColor"
                >
                  <path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z" />
                </svg>
                <span className="text-sm font-medium">ゆうことのきずな</span>
              </div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className="text-xs opacity-80">ランク</span>
                <span className="text-4xl font-bold">{mockUserStats.rank}</span>
              </div>
              <div className="text-xs opacity-80 mb-2">つぎのランクまで</div>
              <div className="text-right text-sm mb-1">
                <span className="font-semibold">{mockUserStats.currentExp}</span>
                <span className="opacity-80"> / {mockUserStats.maxExp}</span>
              </div>
              <Progress
                value={(mockUserStats.currentExp / mockUserStats.maxExp) * 100}
                className="h-2 bg-white/30"
              />
            </CardContent>
          </Card>

          {/* Star Fragments Card */}
          <Card className="border border-border/50 py-3">
            <CardContent className="p-4">
              <div className="flex items-center gap-2 mb-2">
                <Star className="w-4 h-4 text-yellow-500 fill-yellow-500" />
                <span className="text-xs text-muted-foreground">
                  流れ星のかけら
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-2xl font-bold text-foreground">
                  {mockUserStats.starFragments.toLocaleString()}
                </span>
                <Button
                  variant="outline"
                  size="icon-sm"
                  className="rounded-full border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                  onClick={() => console.log("Add star fragments")}
                >
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Unclaimed Rewards Card */}
          <Card className="border border-border/50 py-3">
            <CardContent className="p-4">
              <div className="flex items-center gap-2 mb-2">
                <Gift className="w-4 h-4 text-red-500" />
                <span className="text-xs text-muted-foreground">
                  未受取の報酬
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-2xl font-bold text-foreground">
                  {mockUserStats.unclaimedRewards}
                </span>
                <Button
                  size="sm"
                  className="bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white text-xs h-8"
                  onClick={() => console.log("Claim rewards")}
                >
                  受け取る
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Quick Access Card */}
          <Card className="border border-border/50 py-3">
            <CardContent className="p-4">
              <div className="text-xs font-medium text-[var(--yuuko-green)] mb-3">
                クイックアクセス
              </div>
              <div className="grid grid-cols-2 gap-2">
                {mockQuickAccessItems.map((item) => (
                  <QuickAccessButton
                    key={item.id}
                    item={item}
                    onNavigate={handleNavigate}
                  />
                ))}
              </div>
            </CardContent>
          </Card>

          {/* Onboarding Button */}
          <Button
            variant="outline"
            className="w-full border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] h-10"
            onClick={() => handleNavigate("onboarding")}
          >
            <PlayCircle className="w-4 h-4 mr-2" />
            オンボーディングを見る
          </Button>
        </aside>
      </div>

      {/* Status Bar */}
      <footer className="h-9 bg-white border-t border-border/50 flex items-center justify-between px-4 shrink-0">
        <div className="flex items-center gap-3">
          <Bell className="w-4 h-4 text-muted-foreground" />
          <span className="text-xs text-muted-foreground">お知らせ</span>
          <span className="text-xs text-muted-foreground">|</span>
          <span className="text-xs text-[var(--yuuko-green)] flex items-center gap-1">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--yuuko-green)]" />
            {statusMessage}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <button className="text-muted-foreground hover:text-foreground transition-colors">
            <HelpCircle className="w-4 h-4" />
          </button>
          <PawIcon className="w-4 h-4 text-pink-300" />
        </div>
      </footer>

      {/* Custom animation styles */}
      <style jsx>{`
        @keyframes float {
          0%,
          100% {
            transform: translateY(0px);
          }
          50% {
            transform: translateY(-6px);
          }
        }
        .animate-float {
          animation: float 3s ease-in-out infinite;
        }
      `}</style>
    </div>
  );
}
