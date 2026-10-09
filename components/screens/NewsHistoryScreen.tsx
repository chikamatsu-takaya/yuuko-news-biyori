"use client";

import * as React from "react";
import Image from "next/image";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import { AutostartStatus } from "@/components/layout/AutostartStatus";
import { QuitResidentButton } from "@/components/layout/QuitResidentButton";
import {
  listArticleHistory,
  restoreArchivedArticle,
  updateArticleFavorite,
  type ArticleHistoryFilter,
  type ArticleHistoryItemDto,
} from "@/lib/tauri/articles";
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
  Clock,
  BookOpen,
  Sparkles,
  Dice5,
  Settings,
  ArrowLeft,
  Search,
  Filter,
  List,
  Circle,
  CheckCircle,
  Star,
  Archive,
  Bell,
  HelpCircle,
  RotateCcw,
  Share2,
  FolderOpen,
  Info,
} from "lucide-react";

// Types
type ReadState = "read" | "unread";
type NewsCategory = string;

interface HistoryItem {
  id: string;
  title: string;
  source: string;
  datetime: string;
  description: string;
  readState: ReadState;
  isFavorite: boolean;
  isNew: boolean;
  isArchived: boolean;
  category: NewsCategory;
  thumbnailType: "ai" | "energy" | "mobile" | "business" | "robot" | "space" | "lifestyle";
}

interface NavigationItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
}

interface FilterChip {
  id: string;
  label: string;
  icon?: React.ComponentType<{ className?: string }>;
}

// Mock Data
const mockHistoryItems: HistoryItem[] = [
  {
    id: "1",
    title: "AIコーディング支援ツールの最新動向まとめ",
    source: "TechCrunch Japan",
    datetime: "2025/05/20 10:30",
    description:
      "GitHub Copilot や Cursor など、AIが開発現場をどう変えているのか。最新ツールの特徴や活用事例をわかりやすく解説します。",
    readState: "read",
    isFavorite: true,
    isNew: true,
    isArchived: false,
    category: "AI・テクノロジー",
    thumbnailType: "ai",
  },
  {
    id: "2",
    title: "再生可能エネルギーの未来と課題",
    source: "日経クロステック",
    datetime: "2025/05/19 18:45",
    description:
      "再エネの導入拡大に向けた各国の取り組みと、技術・コスト面での課題について専門家が解説します。",
    readState: "unread",
    isFavorite: false,
    isNew: false,
    isArchived: false,
    category: "環境・エネルギー",
    thumbnailType: "energy",
  },
  {
    id: "3",
    title: "iOS 19、注目の新機能を徹底解説",
    source: "ITmedia Mobile",
    datetime: "2025/05/18 14:20",
    description:
      "Appleが発表したiOS 19の新機能を詳しく紹介。プライバシー強化やUIの進化など、注目ポイントをまとめました。",
    readState: "read",
    isFavorite: true,
    isNew: false,
    isArchived: false,
    category: "モバイル",
    thumbnailType: "mobile",
  },
  {
    id: "4",
    title: "日経平均、続伸　半導体株がけん引",
    source: "日本経済新聞",
    datetime: "2025/05/17 09:15",
    description:
      "17日の東京株式市場は続伸。半導体関連株の上昇が相場を押し上げ、投資家心理の改善が進んでいます。",
    readState: "read",
    isFavorite: false,
    isNew: false,
    isArchived: true,
    category: "ビジネス",
    thumbnailType: "business",
  },
  {
    id: "5",
    title: "身近になる生成AI、生活をどう変える？",
    source: "朝日新聞デジタル",
    datetime: "2025/05/16 20:05",
    description:
      "生成AIが日常生活のさまざまな場面で活用され始めています。便利さと向き合うために知っておきたいポイントを紹介します。",
    readState: "unread",
    isFavorite: false,
    isNew: true,
    isArchived: false,
    category: "AI・テクノロジー",
    thumbnailType: "robot",
  },
  {
    id: "6",
    title: "宇宙ビジネスの最前線、2025年の展望",
    source: "WIRED.jp",
    datetime: "2025/05/15 12:00",
    description:
      "宇宙開発の民間企業参入が加速。打ち上げコスト削減や新サービスの最新動向をまとめました。",
    readState: "read",
    isFavorite: false,
    isNew: false,
    isArchived: true,
    category: "宇宙",
    thumbnailType: "space",
  },
  {
    id: "7",
    title: "集中力を高める「朝の習慣」5つのコツ",
    source: "ライフハッカー日本版",
    datetime: "2025/05/14 08:30",
    description:
      "忙しい毎日でも取り入れやすい、朝のルーティンを紹介。小さな習慣で生産性UPを目指しましょう。",
    readState: "read",
    isFavorite: false,
    isNew: false,
    isArchived: false,
    category: "ライフスタイル",
    thumbnailType: "lifestyle",
  },
];

const mockNavigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: false },
  { id: "history", label: "ニュース履歴", icon: Clock, isActive: true },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: false },
  { id: "customize", label: "カスタマイズ", icon: Sparkles, isActive: false },
  { id: "gacha", label: "ガチャ", icon: Dice5, isActive: false },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

const filterChips: FilterChip[] = [
  { id: "all", label: "すべて", icon: List },
  { id: "unread", label: "未読", icon: Circle },
  { id: "read", label: "既読", icon: CheckCircle },
  { id: "favorite", label: "お気に入り", icon: Star },
  { id: "archived", label: "アーカイブ", icon: Archive },
];

const historyFilterIds: ArticleHistoryFilter[] = [
  "all",
  "unread",
  "read",
  "favorite",
  "archived",
];

const isHistoryFilter = (value: string): value is ArticleHistoryFilter =>
  historyFilterIds.includes(value as ArticleHistoryFilter);

const toReadState = (readState: ArticleHistoryItemDto["readState"]): ReadState =>
  readState === "unread" ? "unread" : "read";

const toThumbnailType = (genre: string): HistoryItem["thumbnailType"] => {
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

const mapTauriHistoryItemToUi = (
  article: ArticleHistoryItemDto
): HistoryItem => {
  const readState = toReadState(article.readState);

  return {
    id: article.articleId,
    title: article.title,
    source: article.sourceName,
    datetime: article.publishedAtText || article.fetchedAt,
    description:
      article.summary ??
      "概要はまだありません。記事を開くと保存済みの内容を確認できます。",
    readState,
    isFavorite: article.isFavorite,
    isNew: readState === "unread",
    isArchived: article.isArchived,
    category: article.genre || "未分類",
    thumbnailType: toThumbnailType(article.genre),
  };
};

const matchesHistoryFilter = (
  item: HistoryItem,
  filter: ArticleHistoryFilter
): boolean => {
  switch (filter) {
    case "unread":
      return item.readState === "unread";
    case "read":
      return item.readState === "read";
    case "favorite":
      return item.isFavorite;
    case "archived":
      return item.isArchived;
    case "all":
    default:
      return true;
  }
};

const getFallbackHistoryItems = (filter: ArticleHistoryFilter): HistoryItem[] =>
  mockHistoryItems.filter((item) => matchesHistoryFilter(item, filter));

// Thumbnail component
function HistoryThumbnail({ type }: { type: HistoryItem["thumbnailType"] }) {
  const thumbnailStyles: Record<string, string> = {
    ai: "from-blue-600 to-cyan-400",
    energy: "from-green-500 to-teal-400",
    mobile: "from-purple-500 to-pink-400",
    business: "from-emerald-500 to-green-400",
    robot: "from-indigo-500 to-purple-400",
    space: "from-slate-700 to-blue-900",
    lifestyle: "from-amber-500 to-orange-400",
  };

  const icons: Record<string, React.ReactNode> = {
    ai: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <circle cx="12" cy="12" r="3" />
        <path d="M12 2v4M12 18v4M2 12h4M18 12h4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83" />
      </svg>
    ),
    energy: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
      </svg>
    ),
    mobile: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <rect x="5" y="2" width="14" height="20" rx="2" />
        <circle cx="12" cy="18" r="1" />
      </svg>
    ),
    business: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <path d="M3 3v18h18" />
        <path d="M7 16l4-4 4 4 5-6" />
      </svg>
    ),
    robot: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <rect x="4" y="8" width="16" height="12" rx="2" />
        <circle cx="9" cy="14" r="2" />
        <circle cx="15" cy="14" r="2" />
        <path d="M12 2v4M8 4h8" />
      </svg>
    ),
    space: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <circle cx="12" cy="12" r="10" />
        <ellipse cx="12" cy="12" rx="10" ry="4" />
        <path d="M12 2a15 15 0 010 20M12 2a15 15 0 000 20" />
      </svg>
    ),
    lifestyle: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <path d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8zM6 1v3M10 1v3M14 1v3" />
      </svg>
    ),
  };

  return (
    <div
      className={`w-16 h-16 rounded-lg bg-gradient-to-br ${thumbnailStyles[type]} flex items-center justify-center flex-shrink-0`}
      aria-hidden="true"
    >
      {icons[type]}
    </div>
  );
}

// Filter Chip
function FilterChipButton({
  chip,
  isActive,
  disabled,
  onClick,
}: {
  chip: FilterChip;
  isActive: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  const Icon = chip.icon;
  return (
    <button
      className={`flex items-center gap-1.5 px-3 py-1.5 rounded-full text-sm transition-all border disabled:cursor-not-allowed disabled:opacity-50 ${
        isActive
          ? "bg-[var(--yuuko-green)] text-white border-[var(--yuuko-green)]"
          : "bg-white text-muted-foreground border-border hover:border-[var(--yuuko-green)]/50"
      }`}
      onClick={onClick}
      aria-pressed={isActive}
      disabled={disabled}
    >
      {Icon && <Icon className="w-4 h-4" aria-hidden="true" />}
      <span>{chip.label}</span>
    </button>
  );
}

// History Item Card
function HistoryItemCard({
  item,
  isSelected,
  onClick,
}: {
  item: HistoryItem;
  isSelected: boolean;
  onClick: () => void;
}) {
  return (
    <Card
      className={`cursor-pointer transition-all py-3 px-0 ${
        isSelected
          ? "border-[var(--yuuko-green)] border-2 bg-[var(--yuuko-green-light)]/50 shadow-md"
          : "border-0 shadow-sm hover:shadow-md"
      }`}
      onClick={onClick}
    >
      <CardContent className="p-3 flex gap-3">
        <HistoryThumbnail type={item.thumbnailType} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <h3 className="font-semibold text-sm text-foreground truncate">{item.title}</h3>
          </div>
          <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground mb-1">
            <span>{item.source}</span>
            <span>・</span>
            <span>{item.datetime}</span>
          </div>
          <p className="text-xs text-muted-foreground line-clamp-2">{item.description}</p>
        </div>
        <div className="flex flex-col items-end gap-1.5 flex-shrink-0">
          <div className="flex items-center gap-1.5">
            <Badge
              className={`text-[10px] px-1.5 py-0 border-0 ${
                item.readState === "read"
                  ? "bg-[var(--yuuko-green)]/20 text-[var(--yuuko-green)]"
                  : "bg-blue-100 text-blue-600"
              }`}
            >
              {item.readState === "read" ? "既読" : "未読"}
            </Badge>
            {item.isNew && (
              <Badge className="bg-red-500 text-white border-0 text-[10px] px-1.5 py-0">NEW</Badge>
            )}
          </div>
          <div className="flex items-center gap-1.5">
            <Star
              role="img"
              aria-label={item.isFavorite ? "お気に入り登録済み" : "お気に入り未登録"}
              className={`w-4 h-4 ${
                item.isFavorite ? "fill-yellow-400 text-yellow-400" : "text-muted-foreground/40"
              }`}
            />
          </div>
          {item.isArchived && (
            <Badge className="bg-gray-200 text-gray-600 border-0 text-[10px] px-1.5 py-0">
              アーカイブ済み
            </Badge>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

// Main Component
export default function NewsHistoryScreen({
  onNavigate,
  onOpenArticle,
}: {
  onNavigate?: (screen: string) => void;
  onOpenArticle?: (articleId: string) => void;
}) {
  const [searchQuery, setSearchQuery] = React.useState("");
  const [activeFilter, setActiveFilter] =
    React.useState<ArticleHistoryFilter>("all");
  const [historyItems, setHistoryItems] = React.useState<HistoryItem[]>([]);
  const [selectedItemId, setSelectedItemId] = React.useState<string | null>(null);
  const [isLoading, setIsLoading] = React.useState(true);
  // 更新中のお気に入りの目標状態（true=登録 / false=解除 / null=更新なし）。
  // 処理中に選択が変わっても「登録中/解除中」の表示が要求内容とずれないよう、選択中の記事からは導かない。
  const [favoriteUpdatingTo, setFavoriteUpdatingTo] = React.useState<boolean | null>(null);
  // state反映前の連打も止め、古い一覧応答が解除結果を上書きしないようにする。
  const favoriteUpdateInFlightRef = React.useRef(false);
  const historyRequestRef = React.useRef(0);
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);
  const [loadNoticeKind, setLoadNoticeKind] = React.useState<
    // favorite-error / restore-error は個別操作の失敗。一覧再読込の「再試行」では解決しないため再試行ボタンを出さない。
    "info" | "error" | "empty" | "favorite-error" | "restore-error"
  >("info");
  // アーカイブから取り出す確認の対象記事。確認中に選択が変わっても対象をずらさないよう固定で持つ。
  // 閉じるアニメーション中にタイトルが空にならないよう、開閉は別の state で持ち、対象は次に開くまで残す。
  const [restoreTarget, setRestoreTarget] = React.useState<{
    id: string;
    title: string;
  } | null>(null);
  const [isRestoreDialogOpen, setIsRestoreDialogOpen] = React.useState(false);
  // 復元中に画面を離れた後で、記事詳細へ遷移したり state を更新したりしないためのフラグ。
  const isMountedRef = React.useRef(true);
  React.useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);
  const [isRestoring, setIsRestoring] = React.useState(false);
  // state反映前の連打でも復元を二重に走らせないためのガード。
  const restoreInFlightRef = React.useRef(false);

  const visibleHistoryItems = React.useMemo(() => {
    const normalizedQuery = searchQuery.trim().toLowerCase();
    if (!normalizedQuery) {
      return historyItems;
    }

    return historyItems.filter((item) =>
      [item.title, item.source, item.description, item.category].some((value) =>
        value.toLowerCase().includes(normalizedQuery)
      )
    );
  }, [historyItems, searchQuery]);

  const selectedItem =
    visibleHistoryItems.find((item) => item.id === selectedItemId) ??
    visibleHistoryItems[0] ??
    null;

  const loadHistoryItems = React.useCallback(async () => {
    if (favoriteUpdateInFlightRef.current) return;
    const requestId = ++historyRequestRef.current;
    setIsLoading(true);
    setLoadNotice(null);
    setLoadNoticeKind("info");

    try {
      const articles = await listArticleHistory({
        filter: activeFilter,
        limit: 200,
      });
      if (requestId !== historyRequestRef.current) return;

      if (!articles) {
        const fallbackItems = getFallbackHistoryItems(activeFilter);
        setHistoryItems(fallbackItems);
        setLoadNotice(
          "ブラウザ単体プレビューのため、サンプル履歴を表示しています。"
        );
        setLoadNoticeKind("info");
        setSelectedItemId((currentId) =>
          fallbackItems.some((item) => item.id === currentId)
            ? currentId
            : fallbackItems[0]?.id ?? null
        );
        return;
      }

      const mappedItems = articles.map(mapTauriHistoryItemToUi);
      setHistoryItems(mappedItems);
      if (mappedItems.length === 0) {
        setLoadNotice("この条件に一致する保存済み記事はまだありません。");
        setLoadNoticeKind("empty");
      } else {
        setLoadNotice(null);
      }
      setSelectedItemId((currentId) =>
        mappedItems.some((item) => item.id === currentId)
          ? currentId
          : mappedItems[0]?.id ?? null
      );
    } catch (error) {
      if (requestId !== historyRequestRef.current) return;
      setHistoryItems([]);
      setSelectedItemId(null);
      setLoadNotice(
        "ニュース履歴の読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
      );
      setLoadNoticeKind("error");
      console.warn("Failed to load article history:", error);
    } finally {
      if (requestId === historyRequestRef.current) setIsLoading(false);
    }
  }, [activeFilter]);

  React.useEffect(() => {
    void loadHistoryItems();
    return () => { historyRequestRef.current += 1; };
  }, [loadHistoryItems]);

  React.useEffect(() => {
    if (
      selectedItemId &&
      visibleHistoryItems.some((item) => item.id === selectedItemId)
    ) {
      return;
    }

    setSelectedItemId(visibleHistoryItems[0]?.id ?? null);
  }, [selectedItemId, visibleHistoryItems]);

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const handleOpenSelectedArticle = () => {
    if (!selectedItem || restoreInFlightRef.current) {
      return;
    }

    // アーカイブ済みの記事は本文がZIP内にあるため、確認してから取り出す（確認なしに復元しない）。
    if (selectedItem.isArchived) {
      setRestoreTarget({ id: selectedItem.id, title: selectedItem.title });
      setIsRestoreDialogOpen(true);
      return;
    }

    if (onOpenArticle) {
      onOpenArticle(selectedItem.id);
      return;
    }

    onNavigate?.("news");
  };

  // 確認で「アーカイブから取り出して開く」を選んだときだけ、記事IDだけを渡して1記事を復元する。
  // 復元できた（restored / already_available）同じ記事IDのときだけ記事詳細へ進み、
  // 失敗時は履歴画面に留まって固定文言だけを出す（パスや生のエラーは画面へ出さない）。
  // 記事詳細から戻ると履歴画面が再マウントされ一覧を読み直すため、アーカイブ済みバッジも更新される。
  const handleConfirmRestore = async () => {
    const target = restoreTarget;
    setIsRestoreDialogOpen(false);
    if (!target || restoreInFlightRef.current) {
      return;
    }

    restoreInFlightRef.current = true;
    setIsRestoring(true);
    setLoadNotice(null);
    setLoadNoticeKind("info");
    try {
      const result = await restoreArchivedArticle({ articleId: target.id });
      // 画面を離れた後に完了した場合は、遷移も表示更新もしない。
      if (!isMountedRef.current) {
        return;
      }
      const isRestored =
        result.articleId === target.id &&
        (result.status === "restored" || result.status === "already_available");
      if (!isRestored) {
        throw new Error("unexpected restore result");
      }

      if (onOpenArticle) {
        onOpenArticle(target.id);
      } else {
        onNavigate?.("news");
      }
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      setLoadNotice(
        "アーカイブから記事を取り出せませんでした。少し時間を置いてから、もう一度お試しください。"
      );
      setLoadNoticeKind("restore-error");
      console.warn("Failed to restore archived article:", error);
    } finally {
      restoreInFlightRef.current = false;
      if (isMountedRef.current) {
        setIsRestoring(false);
      }
    }
  };

  // 詳細パネルのお気に入り登録/解除。Rust の更新結果を確認してから一覧・選択中の記事へ反映する
  // （楽観更新はしない）。失敗時は状態を変えないため、表示は自動的に変更前のままになる。
  const handleToggleFavorite = async () => {
    if (!selectedItem || isLoading || favoriteUpdateInFlightRef.current) {
      return;
    }

    const nextIsFavorite = !selectedItem.isFavorite;
    favoriteUpdateInFlightRef.current = true;
    historyRequestRef.current += 1;
    setFavoriteUpdatingTo(nextIsFavorite);
    setLoadNotice(null);
    setLoadNoticeKind("info");
    try {
      const result = await updateArticleFavorite({
        articleId: selectedItem.id,
        isFavorite: nextIsFavorite,
      });
      setHistoryItems((items) =>
        // 「お気に入り」フィルタ中に解除した記事は、条件に合わなくなるので一覧から外す（従来どおり）。
        activeFilter === "favorite" && !result.isFavorite
          ? items.filter((item) => item.id !== result.articleId)
          : items.map((item) =>
              item.id === result.articleId
                ? { ...item, isFavorite: result.isFavorite }
                : item
            )
      );
    } catch (error) {
      // 生のエラー文言は画面へ出さず、固定文言だけを案内する。
      setLoadNotice(
        nextIsFavorite
          ? "お気に入りの登録に失敗しました。もう一度お試しください。"
          : "お気に入りの解除に失敗しました。もう一度お試しください。"
      );
      setLoadNoticeKind("favorite-error");
      console.warn("Failed to update article favorite:", error);
    } finally {
      favoriteUpdateInFlightRef.current = false;
      setFavoriteUpdatingTo(null);
    }
  };

  const getCategoryColor = (category: NewsCategory) => {
    if (category.includes("AI")) {
      return "bg-[var(--yuuko-green)] text-white";
    }
    if (category.includes("エネルギー") || category.includes("環境")) {
      return "bg-teal-500 text-white";
    }
    if (category.includes("モバイル") || category.includes("スマホ")) {
      return "bg-purple-500 text-white";
    }
    if (category.includes("ビジネス")) {
      return "bg-emerald-500 text-white";
    }
    if (category.includes("宇宙")) {
      return "bg-indigo-500 text-white";
    }
    return "bg-orange-500 text-white";
  };

  return (
    <div className="h-dvh bg-[var(--yuuko-cream)] flex flex-col overflow-hidden">
      <AppTitleBar />

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar */}
        <aside className="w-52 bg-white border-r border-border flex flex-col p-3 flex-shrink-0">
          {/* Back Button */}
          <Button
            variant="outline"
            className="mb-4 border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] justify-start gap-2"
            onClick={() => handleNavigate("home")}
          >
            <ArrowLeft className="w-4 h-4" />
            ホームへ戻る
          </Button>

          {/* Navigation */}
          <nav className="space-y-1 flex-1 overflow-y-auto">
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

          {/* Yuuko Comment Card */}
          <Card className="mt-4 border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/30 py-3">
            <CardContent className="p-3">
              <h4 className="text-xs font-semibold text-[var(--yuuko-green)] mb-2">ゆうこの一言</h4>
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                過去の記事も大切な
                <br />
                学びの宝物だよ〜！
                <br />
                気になるテーマを
                <br />
                見つけてねっ♪
              </p>
              <div className="flex justify-end mt-1">
                <span className="text-[var(--yuuko-green)]" aria-hidden="true">🐾</span>
              </div>
            </CardContent>
          </Card>

          {/* Auto Start */}
          <div className="mt-4 space-y-2">
            <AutostartStatus />
            <QuitResidentButton className="w-full text-xs" />
          </div>
        </aside>

        {/* Center - History List */}
        <main className="flex-1 min-w-0 flex flex-col p-4 overflow-hidden">
          {/* Breadcrumb */}
          <div className="flex items-center gap-2 text-sm text-muted-foreground mb-3">
            {/* パンくず「ホーム」。button にしてクリックとキーボード（Tab→Enter/Space）の両方でホームへ戻れるようにする。 */}
            <button
              type="button"
              className="rounded-sm transition-colors hover:text-[var(--yuuko-green)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--yuuko-green)]"
              onClick={() => handleNavigate("home")}
            >
              ホーム
            </button>
            <span aria-hidden="true">&gt;</span>
            <span className="text-foreground">ニュース履歴</span>
          </div>

          {/* Title */}
          <div className="flex items-center gap-2 mb-4">
            <Clock className="w-6 h-6 text-[var(--yuuko-green)]" aria-hidden="true" />
            <h1 className="text-xl font-bold text-foreground">ニュース履歴</h1>
          </div>

          {/* Search */}
          <div className="flex items-center gap-2 mb-3">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" aria-hidden="true" />
              <Input
                placeholder="キーワードで検索（記事タイトル・本文など）"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-9 bg-white"
                aria-label="ニュース履歴を検索"
              />
            </div>
            <Button variant="outline" className="gap-2">
              <Filter className="w-4 h-4" aria-hidden="true" />
              絞り込み
            </Button>
          </div>

          {/* Filter Chips */}
          <div className="flex items-center gap-2 mb-4 flex-wrap">
            {filterChips.map((chip) => (
              <FilterChipButton
                key={chip.id}
                chip={chip}
                isActive={activeFilter === chip.id}
                disabled={favoriteUpdatingTo !== null}
                onClick={() => {
                  if (!favoriteUpdateInFlightRef.current && chip.id !== activeFilter && isHistoryFilter(chip.id)) {
                    historyRequestRef.current += 1;
                    setIsLoading(true);
                    setActiveFilter(chip.id);
                  }
                }}
              />
            ))}
          </div>

          {loadNotice && (
            <Alert role="presentation" className="mb-4 border-[var(--yuuko-green)]/30 bg-white shadow-sm">
              <Info className="h-4 w-4 text-[var(--yuuko-green)]" aria-hidden="true" />
              <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green)]">お知らせ</AlertTitle>
              <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
                <span
                  role={
                    loadNoticeKind === "error" ||
                    loadNoticeKind === "favorite-error" ||
                    loadNoticeKind === "restore-error"
                      ? "alert"
                      : "status"
                  }
                >
                  {loadNotice}
                </span>
                {loadNoticeKind === "error" && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
                    onClick={() => void loadHistoryItems()}
                    disabled={isLoading}
                  >
                    再試行
                  </Button>
                )}
              </AlertDescription>
            </Alert>
          )}

          {/* History List */}
          <div className="flex-1 overflow-y-auto space-y-2 pr-1">
            {isLoading ? (
              <Card className="border-0 shadow-sm py-6">
                <CardContent className="p-4 text-center text-sm text-muted-foreground">
                  ニュース履歴を読み込んでいます...
                </CardContent>
              </Card>
            ) : visibleHistoryItems.length > 0 ? (
              visibleHistoryItems.map((item) => (
                <HistoryItemCard
                  key={item.id}
                  item={item}
                  isSelected={selectedItem?.id === item.id}
                  onClick={() => setSelectedItemId(item.id)}
                />
              ))
            ) : loadNotice ? null : (
              // loadNotice（0件案内・エラー・mock案内）が出ている時は重複を避け、
              // 純粋にクライアント側フィルタで0件の時だけ「見直して」を表示する。
              <Card className="border-0 shadow-sm py-6">
                <CardContent className="p-4 text-center text-sm text-muted-foreground">
                  表示できる履歴がありません。検索条件やフィルタを見直してください。
                </CardContent>
              </Card>
            )}
          </div>
        </main>

        {/* Right Sidebar */}
        <aside className="w-72 bg-[var(--yuuko-cream-dark)] border-l border-border flex flex-col p-3 flex-shrink-0 overflow-y-auto">
          {/* Yuuko Speech Bubble */}
          <Card className="border-[var(--yuuko-green)]/30 bg-white mb-2 py-2">
            <CardContent className="p-3">
              <p className="text-xs text-foreground leading-relaxed">
                前に読んだニュースも、
                <br />
                すぐ見返せるよ〜！
                <br />
                気になる記事を
                <br />
                もう一度見てみよっ♪
              </p>
              <div className="flex justify-end">
                <span className="text-[var(--yuuko-green)]" aria-hidden="true">🐾</span>
              </div>
            </CardContent>
          </Card>

          {/* Yuuko Character */}
          <div className="flex justify-center mb-3">
            <div className="relative">
              <Image
                src="/assets/yuuko.png"
                width={160}
                height={160}
                alt="ゆうこ"
                className="w-40 h-40 object-contain animate-[float_3s_ease-in-out_infinite]"
                onError={(e) => {
                  const target = e.target as HTMLImageElement;
                  target.style.display = "none";
                }}
              />
            </div>
          </div>

          {/* Selected Item Detail */}
          {selectedItem && (
            <Card className="flex-1 border-0 shadow-sm py-3">
              <CardContent className="p-3 flex flex-col h-full">
                {/* Header badges */}
                <div className="flex items-center gap-1.5 flex-wrap mb-2">
                  <Badge className={`${getCategoryColor(selectedItem.category)} border-0 text-[10px] px-1.5 py-0`}>
                    {selectedItem.category}
                  </Badge>
                  <Badge
                    className={`text-[10px] px-1.5 py-0 border-0 ${
                      selectedItem.readState === "read"
                        ? "bg-[var(--yuuko-green)]/20 text-[var(--yuuko-green)]"
                        : "bg-blue-100 text-blue-600"
                    }`}
                  >
                    {selectedItem.readState === "read" ? "既読" : "未読"}
                  </Badge>
                  {selectedItem.isNew && (
                    <Badge className="bg-red-500 text-white border-0 text-[10px] px-1.5 py-0">NEW</Badge>
                  )}
                  <Star
                    className={`w-4 h-4 ml-auto ${
                      selectedItem.isFavorite ? "fill-yellow-400 text-yellow-400" : "text-muted-foreground/40"
                    }`}
                  />
                </div>

                {/* Title */}
                <h3 className="font-bold text-sm text-foreground mb-2 leading-snug">
                  {selectedItem.title}
                </h3>

                {/* Source & Date */}
                <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground mb-2">
                  <Newspaper className="w-3 h-3" aria-hidden="true" />
                  <span>{selectedItem.source}</span>
                  <span>・</span>
                  <span>{selectedItem.datetime}</span>
                </div>

                {/* Description */}
                <p className="text-xs text-muted-foreground leading-relaxed mb-4">
                  {selectedItem.description}
                </p>

                <div className="border-t border-border my-2" aria-hidden="true" />

                {/* Action Buttons */}
                <div className="space-y-2 mt-auto">
                  <Button
                    className="w-full bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white gap-2"
                    onClick={handleOpenSelectedArticle}
                    disabled={isRestoring}
                  >
                    <RotateCcw className="w-4 h-4" aria-hidden="true" />
                    {isRestoring ? "取り出し中..." : "もう一度見る"}
                  </Button>
                  <Button
                    variant="outline"
                    className="w-full gap-2"
                    onClick={() => void handleToggleFavorite()}
                    disabled={isLoading || favoriteUpdatingTo !== null}
                  >
                    <Star
                      className={`w-4 h-4 ${selectedItem.isFavorite ? "fill-yellow-400 text-yellow-400" : ""}`}
                      aria-hidden="true"
                    />
                    {/* 現在の状態に応じて「登録」/「解除」を切り替える（ラベル自体で操作が分かるため aria-pressed は使わない） */}
                    {favoriteUpdatingTo !== null
                      ? favoriteUpdatingTo
                        ? "登録中..."
                        : "解除中..."
                      : selectedItem.isFavorite
                        ? "お気に入り解除"
                        : "お気に入り登録"}
                  </Button>
                  <Button
                    variant="outline"
                    className="w-full gap-2"
                    onClick={() => console.log("アーカイブを展開:", selectedItem.id)}
                  >
                    <FolderOpen className="w-4 h-4" aria-hidden="true" />
                    アーカイブを展開
                  </Button>
                  <Button
                    variant="outline"
                    className="w-full gap-2"
                    onClick={() => console.log("記事をシェア:", selectedItem.id)}
                  >
                    <Share2 className="w-4 h-4" aria-hidden="true" />
                    記事をシェア
                  </Button>
                </div>
              </CardContent>
            </Card>
          )}
        </aside>
      </div>

      {/* Status Bar */}
      <footer className="h-8 bg-white border-t border-border flex items-center justify-between px-4 text-xs text-muted-foreground flex-shrink-0">
        <div className="flex items-center gap-2">
          <Bell className="w-3.5 h-3.5" aria-hidden="true" />
          <span>お知らせ</span>
          <span className="mx-1" aria-hidden="true">|</span>
          <span className="text-[var(--yuuko-green)]" aria-hidden="true">●</span>
          <span>
            {visibleHistoryItems.length}件を表示中 / 保存済み履歴
            {historyItems.length}件
          </span>
        </div>
        <div className="flex items-center gap-3">
          <button
            className="hover:text-foreground transition-colors"
            aria-label="ヘルプ"
          >
            <HelpCircle className="w-4 h-4" />
          </button>
          <span className="text-[var(--yuuko-green)]" aria-hidden="true">🐾</span>
        </div>
      </footer>

      {/* アーカイブ済み記事を開く前の確認。「取り出して開く」を選んだときだけ復元する。 */}
      <AlertDialog
        open={isRestoreDialogOpen}
        onOpenChange={setIsRestoreDialogOpen}
      >
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

      <style jsx>{`
        @keyframes float {
          0%,
          100% {
            transform: translateY(0);
          }
          50% {
            transform: translateY(-8px);
          }
        }
      `}</style>
    </div>
  );
}
