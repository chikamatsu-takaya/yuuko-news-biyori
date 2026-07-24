"use client";

// 今日のニュース画面（当日「取得」したニュースの一覧）。
// - ホーム画面のおすすめ表示件数設定は適用せず、その日にアプリが取得した記事を表示する。
// - 「取得日時」は media の公開日時（publishedAt）ではなく、保存時に付与される fetchedAt を正とする。
// - ニュース履歴画面（過去全期間・既読/お気に入り/アーカイブ管理）とは目的・データを分ける。
//   本画面はアーカイブ・履歴専用フィルタ・再閲覧文言を持ち込まず、通常の記事選択で記事詳細を開く。

import * as React from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
  ArrowLeft,
  Sparkles,
  Info,
  Star,
} from "lucide-react";
import {
  listArticleHistory,
  type ArticleHistoryItemDto,
} from "@/lib/tauri/articles";

// 当日ニュースカードの表示モデル（履歴とは別目的のため独自に持つ）。
type TodayArticle = {
  id: string;
  title: string;
  source: string;
  datetime: string;
  description: string;
  genre: string;
  isNew: boolean;
  isFavorite: boolean;
  thumbnailType: "ai" | "energy" | "mobile" | "business" | "space" | "lifestyle";
};

type NavigationItem = {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
};

// ニュース一覧表示中はサイドバー「ニュースを見る」を選択状態にする。
const navigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: true },
  { id: "history", label: "ニュース履歴", icon: History, isActive: false },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: false },
  { id: "customize", label: "カスタマイズ", icon: Palette, isActive: false },
  { id: "gacha", label: "ガチャ", icon: Gift, isActive: false },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

const toThumbnailType = (genre: string): TodayArticle["thumbnailType"] => {
  if (genre.includes("AI")) {
    return "ai";
  }
  if (genre.includes("エネルギー") || genre.includes("環境")) {
    return "energy";
  }
  if (genre.includes("モバイル") || genre.includes("スマホ")) {
    return "mobile";
  }
  if (genre.includes("ビジネス")) {
    return "business";
  }
  if (genre.includes("宇宙")) {
    return "space";
  }
  return "lifestyle";
};

// MVPの取得上限。list_article_history は fetched_at 降順で返るため、当日分は先頭に集まる。
// ただし「当日に200件を超えて取得」した場合は超過分が表示されない（MVP制限。ページング・専用commandは未導入）。
const MAX_TODAY_ARTICLES = 200;

// 「アプリがその日に取得したか」を fetchedAt（RFC3339 / UTC保存）から端末ローカルの当日で判定する。
// publishedAt（媒体公開日時）は当日取得判定に使わない。
// 端末ローカル日付で判定するのは、ニュース取得スケジューラ（news_scheduler の local_today()）と
// 日次アーカイブ処理の日付境界が端末ローカル日付のため、それに合わせる（独自の日付基準を作らない）。
const isAcquiredToday = (fetchedAt: string): boolean => {
  const fetched = new Date(fetchedAt);
  if (Number.isNaN(fetched.getTime())) {
    // 取得日時が壊れている記事は安全側で当日扱いにしない（誤表示を避ける）。
    return false;
  }
  const now = new Date();
  return (
    fetched.getFullYear() === now.getFullYear() &&
    fetched.getMonth() === now.getMonth() &&
    fetched.getDate() === now.getDate()
  );
};

const mapToTodayArticle = (article: ArticleHistoryItemDto): TodayArticle => ({
  id: article.articleId,
  title: article.title,
  source: article.sourceName,
  // 表示は媒体の公開日時テキストを優先し、無ければ取得日時を出す（判定は fetchedAt のみで行う）。
  datetime: article.publishedAtText || article.fetchedAt,
  description:
    article.summary ??
    "概要はまだありません。記事を開くと保存済みの内容を確認できます。",
  genre: article.genre || "未分類",
  isNew: article.readState === "unread",
  isFavorite: article.isFavorite,
  thumbnailType: toThumbnailType(article.genre),
});

// ブラウザ単体プレビュー（非Tauri）向けのサンプル。実データが取れないときだけ使う。
const fallbackTodayArticles: TodayArticle[] = [
  {
    id: "today-sample-1",
    title: "今日取得したAIニュースのサンプル",
    source: "E2E News",
    datetime: "今日",
    description:
      "ブラウザ単体プレビュー用のサンプルです。実際には当日取得したニュースが表示されます。",
    genre: "AI・テクノロジー",
    isNew: true,
    isFavorite: false,
    thumbnailType: "ai",
  },
];

function TodayThumbnail({ type }: { type: TodayArticle["thumbnailType"] }) {
  const styles: Record<TodayArticle["thumbnailType"], string> = {
    ai: "from-blue-600 to-cyan-400",
    energy: "from-green-500 to-teal-400",
    mobile: "from-purple-500 to-pink-400",
    business: "from-emerald-500 to-green-400",
    space: "from-slate-700 to-blue-900",
    lifestyle: "from-amber-500 to-orange-400",
  };
  return (
    <div
      className={`w-16 h-16 rounded-lg bg-gradient-to-br ${styles[type]} flex items-center justify-center flex-shrink-0`}
      aria-hidden="true"
    >
      <Newspaper className="w-7 h-7 text-white/90" />
    </div>
  );
}

// 当日ニュース1件分のカード。通常の記事選択（クリック）で記事詳細を開く。
function TodayArticleCard({
  article,
  onClick,
}: {
  article: TodayArticle;
  onClick: () => void;
}) {
  return (
    <Card
      className="cursor-pointer transition-all py-3 px-0 border-0 shadow-sm hover:shadow-md"
      onClick={onClick}
    >
      <CardContent className="p-3 flex gap-3">
        <TodayThumbnail type={article.thumbnailType} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <h3 className="font-semibold text-sm text-foreground truncate">
              {article.title}
            </h3>
          </div>
          <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground mb-1">
            <span>{article.source}</span>
            <span aria-hidden="true">・</span>
            {/* 日付は媒体の公開日時（publishedAtText）。取得日と混同しないよう「公開日」ラベルを付ける。 */}
            <span>公開日: {article.datetime}</span>
          </div>
          <p className="text-xs text-muted-foreground line-clamp-2">
            {article.description}
          </p>
        </div>
        <div className="flex flex-col items-end gap-1.5 flex-shrink-0">
          <div className="flex items-center gap-1.5">
            <Badge className="bg-[var(--yuuko-green)]/15 text-[var(--yuuko-green)] border-0 text-[10px] px-1.5 py-0">
              {article.genre}
            </Badge>
            {article.isNew && (
              <Badge className="bg-red-500 text-white border-0 text-[10px] px-1.5 py-0">
                NEW
              </Badge>
            )}
          </div>
          <Star
            className={`w-4 h-4 ${
              article.isFavorite
                ? "fill-yellow-400 text-yellow-400"
                : "text-muted-foreground/40"
            }`}
            aria-hidden="true"
          />
        </div>
      </CardContent>
    </Card>
  );
}

export default function NewsListScreen({
  onNavigate,
  onOpenArticle,
}: {
  onNavigate?: (screen: string) => void;
  onOpenArticle?: (articleId: string) => void;
}) {
  const [articles, setArticles] = React.useState<TodayArticle[]>([]);
  const [isLoading, setIsLoading] = React.useState(true);
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);
  const [loadNoticeKind, setLoadNoticeKind] = React.useState<
    "info" | "error" | "empty"
  >("info");

  const handleNavigate = (id: string) => {
    onNavigate?.(id);
  };

  const loadTodayArticles = React.useCallback(async () => {
    setIsLoading(true);
    setLoadNotice(null);
    setLoadNoticeKind("info");

    try {
      // ホームの表示件数設定（maxDailyRecommendations）は適用しない。
      // 保存済み記事（fetched_at 降順）を最大 MAX_TODAY_ARTICLES 件まで取得し、当日取得分を抽出する。
      // 当日取得が MAX_TODAY_ARTICLES 件を超える場合は超過分が表示されない（MVP制限）。
      const saved = await listArticleHistory({
        filter: "all",
        limit: MAX_TODAY_ARTICLES,
      });

      if (!saved) {
        // 非Tauri（ブラウザ単体プレビュー）ではサンプルを表示する。
        setArticles(fallbackTodayArticles);
        setLoadNotice(
          "ブラウザ単体プレビューのため、サンプルを表示しています。"
        );
        setLoadNoticeKind("info");
        return;
      }

      const todayArticles = saved
        .filter((article) => isAcquiredToday(article.fetchedAt))
        .map(mapToTodayArticle);

      setArticles(todayArticles);
      if (todayArticles.length === 0) {
        setLoadNotice("今日取得したニュースはまだないみたい。");
        setLoadNoticeKind("empty");
      } else {
        setLoadNotice(null);
      }
    } catch (error) {
      setArticles([]);
      setLoadNotice(
        "今日のニュースの読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
      );
      setLoadNoticeKind("error");
      console.warn("Failed to load today's articles:", error);
    } finally {
      setIsLoading(false);
    }
  }, []);

  React.useEffect(() => {
    void loadTodayArticles();
  }, [loadTodayArticles]);

  const handleOpenArticle = (articleId: string) => {
    onOpenArticle?.(articleId);
  };

  return (
    <div className="h-dvh bg-[var(--yuuko-cream)] flex flex-col overflow-hidden">
      <AppTitleBar />

      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar */}
        <aside className="w-52 bg-white border-r border-border flex flex-col p-3 flex-shrink-0">
          <Button
            variant="outline"
            className="mb-4 border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] justify-start gap-2"
            onClick={() => handleNavigate("home")}
          >
            <ArrowLeft className="w-4 h-4" />
            ホームへ戻る
          </Button>

          <nav className="space-y-1 flex-1 overflow-y-auto">
            {navigationItems.map((item) => (
              <SidebarNavItem
                key={item.id}
                label={item.label}
                icon={item.icon}
                isActive={item.isActive}
                onClick={() => handleNavigate(item.id)}
              />
            ))}
          </nav>

          <Card className="mt-4 border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/30 py-3">
            <CardContent className="p-3">
              <h4 className="text-xs font-semibold text-[var(--yuuko-green)] mb-2">
                ゆうこの一言
              </h4>
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                今日はこんなニュースが
                <br />
                届いてるよ〜！
                <br />
                気になるの、
                <br />
                のぞいてみてねっ♪
              </p>
              <div className="flex justify-end mt-1">
                <span className="text-[var(--yuuko-green)]" aria-hidden="true">
                  🐾
                </span>
              </div>
            </CardContent>
          </Card>
        </aside>

        {/* Center - Today's News List */}
        <main className="flex-1 min-w-0 flex flex-col p-4 overflow-hidden">
          {/* Breadcrumb */}
          <div className="flex items-center gap-2 text-sm text-muted-foreground mb-3">
            <button
              className="hover:text-[var(--yuuko-green)]"
              onClick={() => handleNavigate("home")}
            >
              ホーム
            </button>
            <span aria-hidden="true">&gt;</span>
            <span className="text-foreground">本日取得したニュース</span>
          </div>

          {/* Title + count */}
          <div className="flex items-center gap-2 mb-1">
            <Sparkles
              className="w-6 h-6 text-[var(--yuuko-green)]"
              aria-hidden="true"
            />
            <h1 className="text-xl font-bold text-foreground">
              本日取得したニュース
            </h1>
            {!isLoading && (
              <Badge className="bg-[var(--yuuko-green)] text-white border-0 text-xs">
                {articles.length}件
              </Badge>
            )}
          </div>
          {/* 「今日取得」＝取得日時(fetchedAt)基準であることを明示し、公開日との誤解を避ける補足。 */}
          <p className="text-xs text-muted-foreground mb-4">
            今日、アプリが新しく取得したニュースを表示しています。
          </p>

          {loadNotice && (
            <Alert
              role="presentation"
              className="mb-4 border-[var(--yuuko-green)]/30 bg-white shadow-sm"
            >
              <Info
                className="h-4 w-4 text-[var(--yuuko-green)]"
                aria-hidden="true"
              />
              <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green)]">
                お知らせ
              </AlertTitle>
              <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
                <span role={loadNoticeKind === "error" ? "alert" : "status"}>
                  {loadNotice}
                </span>
                {loadNoticeKind === "error" && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
                    onClick={() => void loadTodayArticles()}
                    disabled={isLoading}
                  >
                    再試行
                  </Button>
                )}
              </AlertDescription>
            </Alert>
          )}

          {/* List */}
          <div className="flex-1 overflow-y-auto space-y-2 pr-1">
            {isLoading ? (
              <Card className="border-0 shadow-sm py-6">
                <CardContent className="p-4 flex items-center justify-center gap-2 text-sm text-muted-foreground">
                  <Spinner className="size-4" />
                  今日のニュースを読み込んでいます...
                </CardContent>
              </Card>
            ) : articles.length > 0 ? (
              articles.map((article) => (
                <TodayArticleCard
                  key={article.id}
                  article={article}
                  onClick={() => handleOpenArticle(article.id)}
                />
              ))
            ) : loadNotice ? null : (
              // loadNotice（空案内・エラー・プレビュー案内）が出ている時は重複を避ける。
              <Card className="border-0 shadow-sm py-6">
                <CardContent className="p-4 text-center text-sm text-muted-foreground">
                  今日取得したニュースはまだないみたい。
                </CardContent>
              </Card>
            )}
          </div>
        </main>
      </div>

      {/* Status Bar */}
      <footer className="h-8 bg-white border-t border-border flex items-center justify-between px-4 text-xs text-muted-foreground flex-shrink-0">
        <span>
          {isLoading
            ? "読み込み中..."
            : `今日取得したニュース ${articles.length}件`}
        </span>
        <span className="text-[var(--yuuko-green)]" aria-hidden="true">
          🐾
        </span>
      </footer>
    </div>
  );
}
