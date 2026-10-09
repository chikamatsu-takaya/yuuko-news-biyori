"use client";

// 過去ニュース画面（月別アーカイブの閲覧。判断台帳 D14 / D90）。
// - 月一覧 → その月の記事一覧 → 記事詳細、の順にたどる専用画面。
// - 月・記事の一覧は Rust 側（archive_index.json）から受け取った値だけを表示し、ZIP やパスは扱わない。
// - 記事を開くときは、ニュース履歴画面（#303）と同じく確認してからアーカイブを1記事だけ取り出す。
//   復元の確認ロジックは NewsHistoryScreen と同じ流れを最小限で複製している（共通化は履歴画面の変更を伴うため見送り）。
// - 古い形式（catalogComplete=false）の月は記事一覧を持たないため、開けない旨だけを案内する。

import * as React from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Spinner } from "@/components/ui/spinner";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import { AutostartStatus } from "@/components/layout/AutostartStatus";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Home,
  Newspaper,
  History,
  Archive,
  BookOpen,
  Palette,
  Gift,
  Settings,
  ArrowLeft,
  ChevronRight,
  Info,
  Star,
} from "lucide-react";
import {
  listArchiveMonthArticles,
  listArchiveMonths,
  restoreArchivedArticle,
  type ArchiveMonthDto,
  type ArticleHistoryItemDto,
} from "@/lib/tauri/articles";

type NavigationItem = {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
};

// 過去ニュース表示中はサイドバー「過去ニュース」を選択状態にする。
const navigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: false },
  { id: "history", label: "ニュース履歴", icon: History, isActive: false },
  { id: "past-news", label: "過去ニュース", icon: Archive, isActive: true },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: false },
  { id: "customize", label: "カスタマイズ", icon: Palette, isActive: false },
  { id: "gacha", label: "ガチャ", icon: Gift, isActive: false },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

// 記事詳細から「戻る」で再表示したとき、同じ月の記事一覧へ戻すための記憶。
// 画面は記事詳細の表示中にアンマウントされるため、共有ファイル（app/page.tsx）へ state を足さずに
// モジュール内で保持する（D90: 近松さんのナビ整理と衝突しないよう page.tsx の差分を最小にする）。
// 記事を開く直前にだけ記録し、次にこの画面を表示したときに一度だけ読み出して消す。
let monthToRestoreOnReturn: string | null = null;
const rememberMonthForReturn = (month: string) => {
  monthToRestoreOnReturn = month;
};
const peekMonthForReturn = (): string | null => monthToRestoreOnReturn;
const clearMonthForReturn = () => {
  monthToRestoreOnReturn = null;
};

const MONTH_PATTERN = /^(\d{4})-(\d{2})$/;

// "YYYY-MM" を「YYYY年M月」に整える。想定外の形式は加工せずそのまま文字列として出す（React が文字列として描画する）。
const formatMonthLabel = (month: string): string => {
  const matched = MONTH_PATTERN.exec(month);
  if (!matched) {
    return month;
  }
  return `${matched[1]}年${Number(matched[2])}月`;
};

const OLD_FORMAT_NOTE =
  "この月は古い形式で保存されているため、記事一覧を表示できません。";
const RESTORE_ERROR_MESSAGE =
  "アーカイブから記事を取り出せませんでした。少し時間を置いてから、もう一度お試しください。";

type NoticeKind = "info" | "error" | "restore-error";

// お知らせ（読み込み失敗・取り出し失敗・プレビュー案内）。文言は固定文だけを受け取る。
function PastNewsNotice({
  notice,
  kind,
  onRetry,
  isRetrying,
}: {
  notice: string;
  kind: NoticeKind;
  onRetry: () => void;
  isRetrying: boolean;
}) {
  return (
    <Alert
      role="presentation"
      className="mb-4 border-[var(--yuuko-green)]/30 bg-white shadow-sm"
    >
      <Info className="h-4 w-4 text-[var(--yuuko-green)]" aria-hidden="true" />
      <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green)]">
        お知らせ
      </AlertTitle>
      <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
        <span role={kind === "info" ? "status" : "alert"}>{notice}</span>
        {kind === "error" && (
          <Button
            variant="outline"
            size="sm"
            className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
            onClick={onRetry}
            disabled={isRetrying}
          >
            再試行
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}

// 月一覧。古い形式（catalogComplete=false）の月は開けないため、ボタンにせず案内だけを出す。
function PastNewsMonthList({
  isLoading,
  months,
  hasNotice,
  onSelectMonth,
}: {
  isLoading: boolean;
  months: ArchiveMonthDto[];
  hasNotice: boolean;
  onSelectMonth: (month: string) => void;
}) {
  if (isLoading) {
    return (
      <Card className="border-0 shadow-sm py-6">
        <CardContent className="p-4 flex items-center justify-center gap-2 text-sm text-muted-foreground">
          <Spinner className="size-4" />
          過去ニュースを読み込んでいます...
        </CardContent>
      </Card>
    );
  }
  if (months.length === 0) {
    // 失敗・プレビュー案内が出ているときは重複を避ける。
    return hasNotice ? null : (
      <Card className="border-0 shadow-sm py-6" data-testid="past-news-empty">
        <CardContent className="p-4 text-center text-sm text-muted-foreground">
          まだアーカイブされた過去ニュースはないみたい。取得から30日ほど経った記事が、月ごとにここへまとまるよ。
        </CardContent>
      </Card>
    );
  }
  return (
    <ul className="space-y-2" data-testid="past-news-month-list">
      {months.map((entry) => (
        <li key={entry.month} data-testid={`past-news-month-${entry.month}`}>
          {entry.catalogComplete ? (
            <button
              type="button"
              className="w-full text-left rounded-xl bg-white shadow-sm border border-transparent px-4 py-3 flex items-center justify-between gap-3 transition-colors hover:border-[var(--yuuko-green)]/40 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--yuuko-green)]"
              onClick={() => onSelectMonth(entry.month)}
            >
              <span className="font-medium text-foreground">
                {formatMonthLabel(entry.month)}
              </span>
              <span className="flex items-center gap-2 text-xs text-muted-foreground">
                {entry.articleCount}件
                <ChevronRight className="w-4 h-4" aria-hidden="true" />
              </span>
            </button>
          ) : (
            <div className="rounded-xl bg-white/70 shadow-sm px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <span className="font-medium text-muted-foreground">
                  {formatMonthLabel(entry.month)}
                </span>
                <span className="text-xs text-muted-foreground">
                  {entry.articleCount}件
                </span>
              </div>
              <p className="mt-1 text-xs text-muted-foreground">{OLD_FORMAT_NOTE}</p>
            </div>
          )}
        </li>
      ))}
    </ul>
  );
}

// 選択した月の記事一覧。記事の選択は親で確認・復元を行う。
function PastNewsArticleList({
  isLoading,
  catalogComplete,
  articles,
  hasNotice,
  isRestoring,
  onSelectArticle,
}: {
  isLoading: boolean;
  catalogComplete: boolean;
  articles: ArticleHistoryItemDto[];
  hasNotice: boolean;
  isRestoring: boolean;
  onSelectArticle: (article: ArticleHistoryItemDto) => void;
}) {
  if (isLoading) {
    return (
      <Card className="border-0 shadow-sm py-6">
        <CardContent className="p-4 flex items-center justify-center gap-2 text-sm text-muted-foreground">
          <Spinner className="size-4" />
          記事一覧を読み込んでいます...
        </CardContent>
      </Card>
    );
  }
  if (!catalogComplete) {
    return (
      <Card className="border-0 shadow-sm py-6">
        <CardContent className="p-4 text-center text-sm text-muted-foreground">
          {OLD_FORMAT_NOTE}
        </CardContent>
      </Card>
    );
  }
  if (articles.length === 0) {
    // 読み込み失敗・プレビューの案内が出ているときは重複を避ける。
    return hasNotice ? null : (
      <Card className="border-0 shadow-sm py-6">
        <CardContent className="p-4 text-center text-sm text-muted-foreground">
          この月の記事は見つからなかったよ。
        </CardContent>
      </Card>
    );
  }
  return (
    <ul className="space-y-2" data-testid="past-news-article-list">
      {articles.map((article) => (
        <li key={article.articleId}>
          <button
            type="button"
            className="w-full text-left rounded-xl bg-white shadow-sm border border-transparent px-4 py-3 transition-colors hover:border-[var(--yuuko-green)]/40 disabled:opacity-60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--yuuko-green)]"
            onClick={() => onSelectArticle(article)}
            disabled={isRestoring}
          >
            <span className="flex items-center gap-2 mb-1">
              {article.genre && (
                <Badge className="bg-[var(--yuuko-green)] text-white border-0 text-[10px] px-1.5 py-0">
                  {article.genre}
                </Badge>
              )}
              {article.isArchived && (
                <Badge className="bg-gray-200 text-gray-600 border-0 text-[10px] px-1.5 py-0">
                  アーカイブ済み
                </Badge>
              )}
              {article.isFavorite && (
                <Star
                  className="w-3.5 h-3.5 fill-yellow-400 text-yellow-400"
                  aria-label="お気に入り"
                />
              )}
            </span>
            <span className="block font-medium text-foreground break-words">
              {article.title}
            </span>
            <span className="mt-1 block text-xs text-muted-foreground">
              {article.sourceName}
              {article.publishedAtText ? ` ・ 公開日: ${article.publishedAtText}` : ""}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

export default function PastNewsScreen({
  onNavigate,
  onOpenArticle,
}: {
  onNavigate?: (screen: string) => void;
  onOpenArticle?: (articleId: string) => void;
}) {
  // 記事詳細から戻ったときだけ、開いていた月を初期値にする（読み出したら消す）。
  // 初期値の読み出しと消去を分けるのは、開発時の StrictMode で初期化関数が2回呼ばれても月を失わないため。
  const [selectedMonth, setSelectedMonth] = React.useState<string | null>(
    peekMonthForReturn
  );
  React.useEffect(() => {
    clearMonthForReturn();
  }, []);

  const [months, setMonths] = React.useState<ArchiveMonthDto[]>([]);
  const [isMonthsLoading, setIsMonthsLoading] = React.useState(true);
  const [monthsNotice, setMonthsNotice] = React.useState<string | null>(null);
  const [monthsNoticeKind, setMonthsNoticeKind] = React.useState<NoticeKind>("info");
  const monthsRequestRef = React.useRef(0);

  const [articles, setArticles] = React.useState<ArticleHistoryItemDto[]>([]);
  const [articlesCatalogComplete, setArticlesCatalogComplete] = React.useState(true);
  const [isArticlesLoading, setIsArticlesLoading] = React.useState(false);
  const [articlesNotice, setArticlesNotice] = React.useState<string | null>(null);
  const [articlesNoticeKind, setArticlesNoticeKind] =
    React.useState<NoticeKind>("info");
  const articlesRequestRef = React.useRef(0);

  // アーカイブから取り出す確認の対象記事。閉じるアニメーション中にタイトルが空にならないよう、
  // 開閉は別の state で持ち、対象は次に開くまで残す（ニュース履歴画面と同じ扱い）。
  const [restoreTarget, setRestoreTarget] = React.useState<{
    id: string;
    title: string;
    month: string;
  } | null>(null);
  const [isRestoreDialogOpen, setIsRestoreDialogOpen] = React.useState(false);
  const [isRestoring, setIsRestoring] = React.useState(false);
  // state 反映前の連打でも復元を二重に走らせないためのガード。
  const restoreInFlightRef = React.useRef(false);
  // 復元中に画面を離れた後で、記事詳細へ遷移したり state を更新したりしないためのフラグ。
  const isMountedRef = React.useRef(true);
  React.useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const loadMonths = React.useCallback(async () => {
    const requestId = ++monthsRequestRef.current;
    setIsMonthsLoading(true);
    setMonthsNotice(null);
    setMonthsNoticeKind("info");
    try {
      const result = await listArchiveMonths();
      if (requestId !== monthsRequestRef.current) return;
      if (!result) {
        // ブラウザ単体プレビューでは Tauri が無く、アーカイブを読めない。
        setMonths([]);
        setMonthsNotice(
          "ブラウザ単体プレビューのため、過去ニュースは表示できません。"
        );
        return;
      }
      setMonths(result);
    } catch (error) {
      if (requestId !== monthsRequestRef.current) return;
      setMonths([]);
      setMonthsNotice(
        "過去ニュースの読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
      );
      setMonthsNoticeKind("error");
      console.warn("Failed to load archive months:", error);
    } finally {
      if (requestId === monthsRequestRef.current) setIsMonthsLoading(false);
    }
  }, []);

  const loadMonthArticles = React.useCallback(async (month: string) => {
    const requestId = ++articlesRequestRef.current;
    setIsArticlesLoading(true);
    setArticles([]);
    setArticlesCatalogComplete(true);
    setArticlesNotice(null);
    setArticlesNoticeKind("info");
    try {
      const result = await listArchiveMonthArticles({ month });
      if (requestId !== articlesRequestRef.current) return;
      if (!result) {
        setArticlesNotice(
          "ブラウザ単体プレビューのため、過去ニュースは表示できません。"
        );
        return;
      }
      setArticlesCatalogComplete(result.catalogComplete);
      setArticles(result.catalogComplete ? result.articles : []);
    } catch (error) {
      if (requestId !== articlesRequestRef.current) return;
      setArticlesNotice(
        "この月の記事一覧の読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
      );
      setArticlesNoticeKind("error");
      console.warn("Failed to load archive month articles:", error);
    } finally {
      if (requestId === articlesRequestRef.current) setIsArticlesLoading(false);
    }
  }, []);

  React.useEffect(() => {
    void loadMonths();
    return () => {
      monthsRequestRef.current += 1;
    };
  }, [loadMonths]);

  React.useEffect(() => {
    if (!selectedMonth) {
      // 月一覧へ戻ったら、読み込み中の記事一覧の応答は採用しない。
      articlesRequestRef.current += 1;
      return;
    }
    void loadMonthArticles(selectedMonth);
    return () => {
      articlesRequestRef.current += 1;
    };
  }, [selectedMonth, loadMonthArticles]);

  const handleBackToMonths = () => {
    setSelectedMonth(null);
    setArticlesNotice(null);
  };

  const handleNavigate = (id: string) => {
    // 表示中に「過去ニュース」を押したときは画面が作り直されないため、ここで月の一覧へ戻す。
    if (id === "past-news") {
      handleBackToMonths();
    }
    onNavigate?.(id);
  };

  // 記事を開く直前に、戻ったときの月を記録してから記事詳細へ進む。
  const openArticle = (articleId: string, month: string) => {
    rememberMonthForReturn(month);
    onOpenArticle?.(articleId);
  };

  const handleSelectArticle = (article: ArticleHistoryItemDto) => {
    if (!selectedMonth || restoreInFlightRef.current) {
      return;
    }
    // アーカイブ済みの記事は本文が ZIP 内にあるため、確認してから取り出す（確認なしに復元しない）。
    if (article.isArchived) {
      setRestoreTarget({
        id: article.articleId,
        title: article.title,
        month: selectedMonth,
      });
      setIsRestoreDialogOpen(true);
      return;
    }
    openArticle(article.articleId, selectedMonth);
  };

  // 確認で「取り出して開く」を選んだときだけ、記事IDだけを渡して1記事を復元する。
  // 復元できた（restored / already_available）同じ記事IDのときだけ記事詳細へ進み、
  // 失敗時はこの画面に留まって固定文言だけを出す（パスや生のエラーは画面へ出さない）。
  const handleConfirmRestore = async () => {
    const target = restoreTarget;
    setIsRestoreDialogOpen(false);
    if (!target || restoreInFlightRef.current) {
      return;
    }

    restoreInFlightRef.current = true;
    setIsRestoring(true);
    setArticlesNotice(null);
    setArticlesNoticeKind("info");
    try {
      const result = await restoreArchivedArticle({ articleId: target.id });
      if (!isMountedRef.current) {
        return;
      }
      const isRestored =
        result.articleId === target.id &&
        (result.status === "restored" || result.status === "already_available");
      if (!isRestored) {
        throw new Error("unexpected restore result");
      }
      openArticle(target.id, target.month);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      setArticlesNotice(RESTORE_ERROR_MESSAGE);
      setArticlesNoticeKind("restore-error");
      console.warn("Failed to restore archived article:", error);
    } finally {
      restoreInFlightRef.current = false;
      if (isMountedRef.current) {
        setIsRestoring(false);
      }
    }
  };

  const selectedMonthLabel = selectedMonth ? formatMonthLabel(selectedMonth) : "";


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
                前の月のニュースも
                <br />
                ちゃんとしまってあるよ〜！
                <br />
                ふり返ってみてねっ♪
              </p>
              <div className="flex justify-end mt-1">
                <span className="text-[var(--yuuko-green)]" aria-hidden="true">
                  🐾
                </span>
              </div>
            </CardContent>
          </Card>

          <div className="mt-4 space-y-2">
            <AutostartStatus />
          </div>
        </aside>

        {/* Center */}
        <main className="flex-1 min-w-0 flex flex-col p-4 overflow-hidden">
          {/* Breadcrumb */}
          <div className="flex items-center gap-2 text-sm text-muted-foreground mb-3">
            <button
              type="button"
              className="rounded-sm transition-colors hover:text-[var(--yuuko-green)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--yuuko-green)]"
              onClick={() => handleNavigate("home")}
            >
              ホーム
            </button>
            <span aria-hidden="true">&gt;</span>
            {selectedMonth ? (
              <>
                <button
                  type="button"
                  className="rounded-sm transition-colors hover:text-[var(--yuuko-green)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--yuuko-green)]"
                  onClick={handleBackToMonths}
                >
                  過去ニュース
                </button>
                <span aria-hidden="true">&gt;</span>
                <span className="text-foreground">{selectedMonthLabel}</span>
              </>
            ) : (
              <span className="text-foreground">過去ニュース</span>
            )}
          </div>

          {/* Title */}
          <div className="flex items-center gap-2 mb-1">
            <Archive className="w-6 h-6 text-[var(--yuuko-green)]" aria-hidden="true" />
            <h1 className="text-xl font-bold text-foreground">
              {selectedMonth ? `${selectedMonthLabel}の過去ニュース` : "過去ニュース"}
            </h1>
            {selectedMonth && !isArticlesLoading && articlesCatalogComplete && !articlesNotice && (
              <Badge className="bg-[var(--yuuko-green)] text-white border-0 text-xs">
                {articles.length}件
              </Badge>
            )}
          </div>
          <p className="text-xs text-muted-foreground mb-4">
            {selectedMonth
              ? "読みたい記事を選ぶと、アーカイブから取り出して開くよ。"
              : "アーカイブにまとめた過去のニュースを、月ごとに見られます。"}
          </p>

          {selectedMonth && (
            <div className="mb-3">
              <Button
                variant="outline"
                size="sm"
                className="gap-1 border-[var(--yuuko-green)]/40 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                onClick={handleBackToMonths}
                disabled={isRestoring}
              >
                <ArrowLeft className="w-4 h-4" aria-hidden="true" />
                月の一覧へ戻る
              </Button>
            </div>
          )}

          {selectedMonth
            ? articlesNotice &&
              <PastNewsNotice
                notice={articlesNotice}
                kind={articlesNoticeKind}
                onRetry={() => void loadMonthArticles(selectedMonth)}
                isRetrying={isArticlesLoading}
              />
            : monthsNotice &&
              <PastNewsNotice
                notice={monthsNotice}
                kind={monthsNoticeKind}
                onRetry={() => void loadMonths()}
                isRetrying={isMonthsLoading}
              />}

          {isRestoring && (
            <p className="mb-2 text-xs text-muted-foreground" role="status">
              アーカイブから取り出し中...
            </p>
          )}

          <div className="flex-1 overflow-y-auto pr-1">
            {selectedMonth ? (
              <PastNewsArticleList
                isLoading={isArticlesLoading}
                catalogComplete={articlesCatalogComplete}
                articles={articles}
                hasNotice={Boolean(articlesNotice)}
                isRestoring={isRestoring}
                onSelectArticle={handleSelectArticle}
              />
            ) : (
              <PastNewsMonthList
                isLoading={isMonthsLoading}
                months={months}
                hasNotice={Boolean(monthsNotice)}
                onSelectMonth={setSelectedMonth}
              />
            )}
          </div>
        </main>
      </div>

      {/* Status Bar */}
      <footer className="h-8 bg-white border-t border-border flex items-center justify-between px-4 text-xs text-muted-foreground flex-shrink-0">
        <span>
          {selectedMonth
            ? `${selectedMonthLabel}の過去ニュース`
            : isMonthsLoading
              ? "読み込み中..."
              : `アーカイブ ${months.length}か月分`}
        </span>
        <span className="text-[var(--yuuko-green)]" aria-hidden="true">
          🐾
        </span>
      </footer>

      {/* アーカイブ済み記事を開く前の確認。「取り出して開く」を選んだときだけ復元する。 */}
      <AlertDialog open={isRestoreDialogOpen} onOpenChange={setIsRestoreDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>アーカイブから取り出して開く</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2 text-sm text-muted-foreground">
                <p className="break-all">
                  「{restoreTarget?.title}」はアーカイブに保管されています。
                </p>
                <p>アーカイブから取り出して、記事詳細を開きますか？</p>
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>キャンセル</AlertDialogCancel>
            <AlertDialogAction onClick={() => void handleConfirmRestore()}>
              取り出して開く
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
