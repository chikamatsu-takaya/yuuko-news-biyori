"use client";

import * as React from "react";
import Image from "next/image";
import {
  Home,
  Newspaper,
  Clock,
  BookOpen,
  Sparkles,
  Gift,
  Settings,
  ArrowLeft,
  Search,
  Filter,
  List,
  Star,
  Bot,
  Brain,
  Database,
  Cpu,
  CircuitBoard,
  ChevronLeft,
  ChevronRight,
  Share2,
  Trash2,
  Pencil,
  Bell,
  HelpCircle,
  Save,
  X,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Spinner } from "@/components/ui/spinner";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  listDictionaryEntries,
  updateDictionaryMemo,
  updateDictionaryFavorite,
  deleteDictionaryEntry,
  type DictionaryEntryListItemDto as TauriDictionaryEntryListItemDto,
  type DictionaryEntryType as TauriDictionaryEntryType,
} from "@/lib/tauri/dictionary";

import { useToast } from "@/hooks/use-toast";

type NavigationItem = {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
};

type FilterType = "all" | TauriDictionaryEntryType | "favorite";
type SortOption = "recent" | "name" | "favorite";
type DictionaryIconType = "robot" | "brain" | "database" | "chip" | "circuit";

type DictionaryEntry = {
  id: string;
  term: string;
  type: TauriDictionaryEntryType;
  shortDescription: string;
  fullDescription: string;
  lastViewedText: string;
  lastViewedRaw?: string;
  relatedArticleId?: string;
  relatedArticle?: string;
  isFavorite: boolean;
  memo?: string;
  iconType: DictionaryIconType;
};

const navigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: false },
  { id: "history", label: "ニュース履歴", icon: Clock, isActive: false },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: true },
  { id: "customize", label: "カスタマイズ", icon: Sparkles, isActive: false },
  { id: "gacha", label: "ガチャ", icon: Gift, isActive: false },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

const fallbackDictionaryEntries: DictionaryEntry[] = [
  {
    id: "entry-article-001-generated-ai",
    term: "生成AI",
    type: "term",
    shortDescription: "文章や画像などを自動生成する AI 全般を指す言葉です。",
    fullDescription:
      "生成AIは、入力された指示に応じて文章・画像・音声などを自動生成する技術群です。この記事では、生成AIそのものの新規性よりも、業務課題の解決にどう結びついているかが注目点になっています。",
    lastViewedText: "2025/05/20",
    lastViewedRaw: "1747699200",
    relatedArticleId: "article-001",
    relatedArticle: "生成AIスタートアップの資金調達が再加速",
    isFavorite: true,
    memo: "最近よく聞くので覚えておきたい。",
    iconType: "robot",
  },
  {
    id: "entry-article-001-fundraising",
    term: "資金調達",
    type: "phrase",
    shortDescription: "企業が事業拡大のために投資や融資で資金を集めることです。",
    fullDescription:
      "資金調達は、企業が新しい開発や採用、営業活動を進めるために必要なお金を外部から集めることです。この記事では、生成AI関連企業に再び投資が集まり始めている流れを示しています。",
    lastViewedText: "2025/05/20",
    lastViewedRaw: "1747699200",
    relatedArticleId: "article-001",
    relatedArticle: "生成AIスタートアップの資金調達が再加速",
    isFavorite: false,
    iconType: "brain",
  },
  {
    id: "entry-article-002-saas",
    term: "SaaS",
    type: "term",
    shortDescription: "インターネット経由で利用するソフトウェア提供形態です。",
    fullDescription:
      "SaaS は Software as a Service の略で、クラウド上で提供されるソフトウェアを必要なときに利用する形態です。この記事では、機能そのものに加えて導入後の支援体制が差別化要因として扱われています。",
    lastViewedText: "2025/05/18",
    lastViewedRaw: "1747526400",
    relatedArticleId: "article-002",
    relatedArticle: "国内SaaS企業、業務改善支援の新施策を発表",
    isFavorite: false,
    iconType: "database",
  },
  {
    id: "entry-article-002-business-improvement",
    term: "業務改善",
    type: "key_point",
    shortDescription: "仕事の流れを見直して効率や成果を高めることです。",
    fullDescription:
      "業務改善は、現場の手間や無駄を減らしながら成果を上げるための取り組みです。この記事では、単なるツール導入ではなく、改善が定着する運用設計までが主題になっています。",
    lastViewedText: "2025/05/18",
    lastViewedRaw: "1747526400",
    relatedArticleId: "article-002",
    relatedArticle: "国内SaaS企業、業務改善支援の新施策を発表",
    isFavorite: false,
    iconType: "chip",
  },
  {
    id: "entry-article-003-quantum",
    term: "量子コンピュータ",
    type: "term",
    shortDescription: "量子力学の性質を利用して計算する新しい計算機です。",
    fullDescription:
      "量子コンピュータは、通常のコンピュータとは異なる量子の性質を使って計算する技術です。この記事では高速化よりも、安定して正確に動かすための仕組みに焦点が当たっています。",
    lastViewedText: "2025/05/16",
    lastViewedRaw: "1747353600",
    relatedArticleId: "article-003",
    relatedArticle: "量子コンピュータ研究で新たな誤り訂正手法",
    isFavorite: false,
    iconType: "circuit",
  },
];

const itemsPerPage = 5;
const previewNotice =
  "ブラウザプレビューではモック辞書を表示しています。Tauri で起動すると保存済みの辞書が表示されます。";

const filterOptions: Array<{
  id: FilterType;
  label: string;
  icon?: React.ComponentType<{ className?: string }>;
}> = [
  { id: "all", label: "すべて", icon: List },
  { id: "term", label: "単語", icon: Search },
  { id: "phrase", label: "フレーズ", icon: Sparkles },
  { id: "key_point", label: "要点説明", icon: Pencil },
  { id: "favorite", label: "お気に入り", icon: Star },
];

function PawIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      className={className}
      xmlns="http://www.w3.org/2000/svg"
    >
      <ellipse cx="12" cy="17" rx="5" ry="4" />
      <circle cx="6" cy="10" r="2.5" />
      <circle cx="18" cy="10" r="2.5" />
      <circle cx="9" cy="6" r="2" />
      <circle cx="15" cy="6" r="2" />
    </svg>
  );
}

function typeLabel(type: TauriDictionaryEntryType): string {
  if (type === "phrase") {
    return "フレーズ";
  }
  if (type === "key_point") {
    return "要点説明";
  }
  return "単語";
}

function iconTypeFor(type: TauriDictionaryEntryType): DictionaryIconType {
  if (type === "phrase") {
    return "brain";
  }
  if (type === "key_point") {
    return "chip";
  }
  return "robot";
}

function formatLastViewedText(value?: string): string {
  if (!value) {
    return "未参照";
  }

  const unixSeconds = Number(value);
  if (Number.isFinite(unixSeconds) && unixSeconds > 0) {
    const date = new Date(unixSeconds * 1000);
    if (!Number.isNaN(date.getTime())) {
      const year = date.getFullYear();
      const month = `${date.getMonth() + 1}`.padStart(2, "0");
      const day = `${date.getDate()}`.padStart(2, "0");
      return `${year}/${month}/${day}`;
    }
  }

  return value;
}

function toUiEntry(entry: TauriDictionaryEntryListItemDto): DictionaryEntry {
  return {
    id: entry.entryId,
    term: entry.keyText,
    type: entry.type,
    shortDescription: entry.shortExplanation,
    fullDescription: entry.detailExplanation,
    lastViewedText: formatLastViewedText(entry.lastViewedAtText),
    lastViewedRaw: entry.lastViewedAtText,
    relatedArticleId: entry.relatedArticleId,
    relatedArticle: entry.relatedArticleTitle,
    isFavorite: entry.isStarred,
    memo: entry.memo,
    iconType: iconTypeFor(entry.type),
  };
}

function filterFallbackEntries(
  entries: DictionaryEntry[],
  keyword: string,
  filter: FilterType
): DictionaryEntry[] {
  const normalizedKeyword = keyword.trim().toLowerCase();

  return entries.filter((entry) => {
    if (filter === "favorite" && !entry.isFavorite) {
      return false;
    }

    if (filter !== "all" && filter !== "favorite" && entry.type !== filter) {
      return false;
    }

    if (!normalizedKeyword) {
      return true;
    }

    return [
      entry.term,
      entry.shortDescription,
      entry.fullDescription,
      entry.relatedArticle,
    ]
      .filter(Boolean)
      .some((value) => value!.toLowerCase().includes(normalizedKeyword));
  });
}

function sortEntries(
  entries: DictionaryEntry[],
  sortOption: SortOption
): DictionaryEntry[] {
  const sortedEntries = [...entries];

  if (sortOption === "name") {
    sortedEntries.sort((left, right) =>
      left.term.localeCompare(right.term, "ja")
    );
    return sortedEntries;
  }

  if (sortOption === "favorite") {
    sortedEntries.sort((left, right) => {
      if (left.isFavorite !== right.isFavorite) {
        return left.isFavorite ? -1 : 1;
      }
      return left.term.localeCompare(right.term, "ja");
    });
    return sortedEntries;
  }

  sortedEntries.sort((left, right) => {
    const leftTimestamp = Number(left.lastViewedRaw ?? 0);
    const rightTimestamp = Number(right.lastViewedRaw ?? 0);
    return rightTimestamp - leftTimestamp;
  });
  return sortedEntries;
}

function EntryIcon({ type }: { type: DictionaryIconType }) {
  const iconMap = {
    robot: Bot,
    brain: Brain,
    database: Database,
    chip: Cpu,
    circuit: CircuitBoard,
  };
  const backgroundColors = {
    robot: "bg-emerald-100",
    brain: "bg-cyan-100",
    database: "bg-blue-100",
    chip: "bg-purple-100",
    circuit: "bg-teal-100",
  };
  const Icon = iconMap[type];

  return (
    <div
      className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-lg ${backgroundColors[type]}`}
    >
      <Icon className="h-6 w-6 text-[var(--yuuko-green)]" />
    </div>
  );
}

function TypeBadge({ type }: { type: TauriDictionaryEntryType }) {
  const colors = {
    term: "bg-[var(--yuuko-green)] text-white",
    phrase: "bg-blue-500 text-white",
    key_point: "bg-amber-500 text-white",
  } as const;

  return (
    <Badge className={`${colors[type]} border-0 px-2 py-0 text-[10px]`}>
      {typeLabel(type)}
    </Badge>
  );
}

function DictionaryEntryCard({
  entry,
  isSelected,
  onClick,
  onToggleFavorite,
}: {
  entry: DictionaryEntry;
  isSelected: boolean;
  onClick: () => void;
  onToggleFavorite: (entryId: string, current: boolean) => void;
}) {
  return (
    <Card
      className={`cursor-pointer px-0 py-2 transition-all hover:shadow-md ${
        isSelected
          ? "border-2 border-[var(--yuuko-green)] bg-[var(--yuuko-green-light)]/30"
          : "border-border/50 hover:border-border"
      }`}
      onClick={onClick}
    >
      <CardContent className="flex items-start gap-3 p-3">
        <EntryIcon type={entry.iconType} />
        <div className="min-w-0 flex-1">
          <div className="mb-1 flex items-center gap-2">
            <span className="text-sm font-semibold text-foreground">
              {entry.term}
            </span>
            <TypeBadge type={entry.type} />
          </div>
          <p className="mb-1.5 line-clamp-1 text-xs text-muted-foreground">
            {entry.shortDescription}
          </p>
          <div className="flex items-center gap-1 text-[10px] text-muted-foreground">
            <span>最終閲覧：{entry.lastViewedText}</span>
            {entry.relatedArticle ? (
              <>
                <span className="mx-1">|</span>
                <span className="truncate">関連記事：{entry.relatedArticle}</span>
              </>
            ) : null}
          </div>
        </div>
        <button
          type="button"
          className="shrink-0 p-1"
          onClick={(event) => {
            event.stopPropagation();
            onToggleFavorite(entry.id, entry.isFavorite);
          }}
        >
          <Star
            className={`h-5 w-5 ${
              entry.isFavorite
                ? "fill-yellow-400 text-yellow-400"
                : "text-muted-foreground/50"
            }`}
          />
        </button>
      </CardContent>
    </Card>
  );
}

function Pagination({
  currentPage,
  totalPages,
  totalItems,
  itemsPerPage,
  onPageChange,
}: {
  currentPage: number;
  totalPages: number;
  totalItems: number;
  itemsPerPage: number;
  onPageChange: (page: number) => void;
}) {
  if (totalPages <= 1) {
    return (
      <div className="mt-4 text-right text-xs text-muted-foreground">
        全 {totalItems} 件
      </div>
    );
  }

  const startItem = (currentPage - 1) * itemsPerPage + 1;
  const endItem = Math.min(currentPage * itemsPerPage, totalItems);
  const visiblePages = Array.from(
    new Set([
      Math.max(1, currentPage - 1),
      currentPage,
      Math.min(totalPages, currentPage + 1),
    ])
  );

  return (
    <div className="mt-4 flex items-center justify-between">
      <span className="text-xs text-muted-foreground">
        全 {totalItems} 件中 {startItem}〜{endItem} 件を表示
      </span>
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2"
          onClick={() => onPageChange(currentPage - 1)}
          disabled={currentPage === 1}
        >
          <ChevronLeft className="h-4 w-4" />
        </Button>
        {visiblePages[0] > 1 ? (
          <span className="px-1 text-muted-foreground">…</span>
        ) : null}
        {visiblePages.map((page) => (
          <Button
            key={page}
            variant={currentPage === page ? "default" : "ghost"}
            size="sm"
            className={`h-7 w-7 p-0 ${
              currentPage === page
                ? "bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90"
                : ""
            }`}
            onClick={() => onPageChange(page)}
          >
            {page}
          </Button>
        ))}
        {visiblePages[visiblePages.length - 1] < totalPages ? (
          <span className="px-1 text-muted-foreground">…</span>
        ) : null}
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2"
          onClick={() => onPageChange(currentPage + 1)}
          disabled={currentPage === totalPages}
        >
          <ChevronRight className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}

function RelatedNewsThumbnail() {
  return (
    <div className="flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-gradient-to-br from-blue-900 to-indigo-900">
      <svg viewBox="0 0 40 40" className="h-8 w-8">
        <circle cx="20" cy="20" r="8" fill="rgba(100,200,255,0.3)" />
        <circle cx="20" cy="20" r="4" fill="rgba(150,220,255,0.5)" />
        {[0, 60, 120, 180, 240, 300].map((angle) => (
          <circle
            key={angle}
            cx={20 + 12 * Math.cos((angle * Math.PI) / 180)}
            cy={20 + 12 * Math.sin((angle * Math.PI) / 180)}
            r="2"
            fill="rgba(200,230,255,0.6)"
          />
        ))}
      </svg>
    </div>
  );
}

export default function DictionaryScreen({
  onNavigate,
  onOpenArticle,
}: {
  onNavigate?: (screen: string) => void;
  onOpenArticle?: (articleId: string) => void;
}) {
  const [searchQuery, setSearchQuery] = React.useState("");
  const [activeFilter, setActiveFilter] = React.useState<FilterType>("all");
  const [sortOption, setSortOption] = React.useState<SortOption>("recent");
  const [selectedEntryId, setSelectedEntryId] = React.useState<string | null>(
    null
  );
  const [currentPage, setCurrentPage] = React.useState(1);
  const [isAutoStart, setIsAutoStart] = React.useState(true);
  const [entries, setEntries] = React.useState<DictionaryEntry[]>([]);
  const [isLoading, setIsLoading] = React.useState(true);
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);

  const { toast } = useToast();

  // Memo edit states
  const [isEditingMemo, setIsEditingMemo] = React.useState(false);
  const [editMemoValue, setEditMemoValue] = React.useState("");

  const trimmedSearchQuery = searchQuery.trim();

  // Reset edit state when selection changes
  React.useEffect(() => {
    setIsEditingMemo(false);
    setEditMemoValue("");
  }, [selectedEntryId]);

  const loadEntries = React.useCallback(async () => {
    setIsLoading(true);
    setLoadNotice(null);

    try {
      const dictionaryEntries = await listDictionaryEntries({
        keyword: trimmedSearchQuery || undefined,
        type:
          activeFilter !== "all" && activeFilter !== "favorite"
            ? activeFilter
            : undefined,
        starredOnly: activeFilter === "favorite" ? true : undefined,
      });

      if (!dictionaryEntries) {
        setEntries(
          filterFallbackEntries(
            fallbackDictionaryEntries,
            trimmedSearchQuery,
            activeFilter
          )
        );
        setLoadNotice(previewNotice);
        return;
      }

      setEntries(dictionaryEntries.map(toUiEntry));
    } catch (error) {
      setEntries([]);
      setLoadNotice(
        "辞書一覧の取得に失敗しました。時間をおいてもう一度お試しください。"
      );
      console.warn("Failed to load dictionary entries:", error);
    } finally {
      setIsLoading(false);
    }
  }, [activeFilter, trimmedSearchQuery]);

  React.useEffect(() => {
    void loadEntries();
  }, [loadEntries]);

  React.useEffect(() => {
    setCurrentPage(1);
  }, [activeFilter, trimmedSearchQuery, sortOption]);

  const sortedEntries = React.useMemo(
    () => sortEntries(entries, sortOption),
    [entries, sortOption]
  );
  const totalPages = Math.max(1, Math.ceil(sortedEntries.length / itemsPerPage));

  React.useEffect(() => {
    if (sortedEntries.length === 0) {
      if (selectedEntryId !== null) {
        setSelectedEntryId(null);
      }
      return;
    }

    if (
      !selectedEntryId ||
      !sortedEntries.some((entry) => entry.id === selectedEntryId)
    ) {
      setSelectedEntryId(sortedEntries[0].id);
    }
  }, [selectedEntryId, sortedEntries]);

  React.useEffect(() => {
    if (currentPage > totalPages) {
      setCurrentPage(totalPages);
    }
  }, [currentPage, totalPages]);

  const paginatedEntries = React.useMemo(() => {
    const startIndex = (currentPage - 1) * itemsPerPage;
    return sortedEntries.slice(startIndex, startIndex + itemsPerPage);
  }, [currentPage, sortedEntries]);

  const selectedEntry =
    sortedEntries.find((entry) => entry.id === selectedEntryId) ?? null;

  const handleNavigate = (screen: string) => {
    onNavigate?.(screen);
  };

  const handleGoHome = () => {
    onNavigate?.("home");
  };

  const handleOpenRelatedArticle = () => {
    if (!selectedEntry?.relatedArticleId) {
      return;
    }

    onOpenArticle?.(selectedEntry.relatedArticleId);
  };

  const handleToggleFavorite = async (entryId: string, current: boolean) => {
    try {
      await updateDictionaryFavorite({ entryId, isStarred: !current });
      await loadEntries();
    } catch (error) {
      console.error("Failed to toggle favorite:", error);
      toast({
        variant: "destructive",
        title: "更新に失敗しちゃった",
        description: "お気に入りの更新ができなかったよ。もう一度試してみてね。",
      });
    }
  };

  const handleStartEditMemo = () => {
    setEditMemoValue(selectedEntry?.memo || "");
    setIsEditingMemo(true);
  };

  const handleSaveMemo = async () => {
    if (!selectedEntry) {
      return;
    }
    try {
      await updateDictionaryMemo({
        entryId: selectedEntry.id,
        memo: editMemoValue,
      });
      setIsEditingMemo(false);
      await loadEntries();
    } catch (error) {
      console.error("Failed to save memo:", error);
      toast({
        variant: "destructive",
        title: "保存に失敗しちゃった",
        description: "メモの保存ができなかったよ。もう一度試してみてね。",
      });
    }
  };

  const handleDeleteEntry = async () => {
    if (!selectedEntry) {
      return;
    }
    if (!window.confirm(`「${selectedEntry.term}」を辞書から削除しますか？`)) {
      return;
    }

    try {
      await deleteDictionaryEntry({ entryId: selectedEntry.id });
      setSelectedEntryId(null);
      await loadEntries();
    } catch (error) {
      console.error("Failed to delete entry:", error);
      toast({
        variant: "destructive",
        title: "削除に失敗しちゃった",
        description: "辞書項目の削除ができなかったよ。もう一度試してみてね。",
      });
    }
  };

  return (
    <div className="flex h-dvh w-full flex-col overflow-hidden bg-background">
      <AppTitleBar className="border-border/50 bg-white" />

      <div className="flex flex-1 overflow-hidden">
        <aside className="flex w-52 shrink-0 flex-col border-r border-border/50 bg-white">
          <div className="p-3">
            <Button
              variant="outline"
              className="w-full justify-start gap-2 border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
              onClick={handleGoHome}
            >
              <ArrowLeft className="h-4 w-4" />
              ホームへ戻る
            </Button>
          </div>

          <nav className="flex-1 space-y-1 overflow-y-auto px-3">
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

          <div className="p-3">
            <Card className="border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/30 py-3">
              <CardContent className="p-3">
                <p className="mb-2 text-xs font-medium text-[var(--yuuko-green)]">
                  ゆうこの一言
                </p>
                <p className="text-[11px] leading-relaxed text-foreground">
                  難しい言葉も、
                  <br />
                  少しずつ覚えれば
                  <br />
                  こわくないよ〜！
                  <br />
                  一緒にレベルアップしよっ♪
                </p>
                <div className="mt-2 flex justify-end">
                  <PawIcon className="h-4 w-4 text-[var(--yuuko-green)]/50" />
                </div>
              </CardContent>
            </Card>
          </div>

          <div className="border-t border-border/50 p-3">
            <div className="mb-2 flex items-center gap-2">
              <span className="text-xs text-muted-foreground">自動起動：</span>
              <button
                type="button"
                className={`text-xs font-medium ${
                  isAutoStart
                    ? "text-[var(--yuuko-green)]"
                    : "text-muted-foreground"
                }`}
                onClick={() => setIsAutoStart((currentValue) => !currentValue)}
              >
                {isAutoStart ? "ON" : "OFF"}
              </button>
              {isAutoStart ? (
                <span className="h-2 w-2 rounded-full bg-[var(--yuuko-green)]" />
              ) : null}
            </div>
            <Button
              variant="outline"
              size="sm"
              className="w-full text-xs"
              onClick={() => console.log("Exit app")}
            >
              常駐を終了する
            </Button>
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 overflow-hidden">
          <div className="flex-1 overflow-y-auto p-6">
            <div className="mb-4 flex items-center gap-2 text-xs text-muted-foreground">
              <button
                type="button"
                className="transition-colors hover:text-foreground"
                onClick={handleGoHome}
              >
                ホーム
              </button>
              <ChevronRight className="h-3 w-3" />
              <span className="text-foreground">ゆうこ辞書</span>
            </div>

            <div className="mb-6 flex items-center gap-2">
              <BookOpen className="h-6 w-6 text-[var(--yuuko-green)]" />
              <h1 className="text-xl font-bold text-foreground">ゆうこ辞書</h1>
            </div>

            <div className="mb-4 flex items-center gap-3">
              <div className="relative flex-1">
                <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  placeholder="単語やフレーズで検索"
                  value={searchQuery}
                  onChange={(event) => setSearchQuery(event.target.value)}
                  className="bg-white pl-10"
                />
              </div>
              <Button variant="outline" className="gap-2" disabled>
                <Filter className="h-4 w-4" />
                絞り込み
              </Button>
            </div>

            <div className="mb-6 flex flex-wrap items-center gap-2">
              {filterOptions.map((filter) => {
                const Icon = filter.icon;
                const isActive = activeFilter === filter.id;
                return (
                  <button
                    key={filter.id}
                    type="button"
                    className={`flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium transition-all ${
                      isActive
                        ? "bg-[var(--yuuko-green)] text-white"
                        : "border border-border bg-white text-muted-foreground hover:border-[var(--yuuko-green)]/50"
                    }`}
                    onClick={() => setActiveFilter(filter.id)}
                  >
                    {Icon ? <Icon className="h-3.5 w-3.5" /> : null}
                    {filter.label}
                  </button>
                );
              })}
            </div>

            {loadNotice ? (
              <p className="mb-4 text-xs text-amber-700">{loadNotice}</p>
            ) : null}

            <Card className="border-border/50 py-0">
              <CardContent className="p-4">
                <div className="mb-4 flex items-center justify-between gap-3">
                  <h2 className="text-sm font-semibold text-foreground">
                    登録済みの辞書
                  </h2>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">
                      並び替え：
                    </span>
                    <Select
                      value={sortOption}
                      onValueChange={(value) =>
                        setSortOption(value as SortOption)
                      }
                    >
                      <SelectTrigger className="h-8 w-[132px] text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="recent">最近見た順</SelectItem>
                        <SelectItem value="name">名前順</SelectItem>
                        <SelectItem value="favorite">お気に入り順</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>

                {isLoading ? (
                  <div className="flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-4 text-sm text-muted-foreground">
                    <Spinner className="size-4" />
                    <span>辞書一覧を読み込んでいます...</span>
                  </div>
                ) : null}

                {!isLoading && paginatedEntries.length === 0 ? (
                  <Card className="border-dashed border-border/60 py-0 shadow-none">
                    <CardContent className="flex flex-col items-center gap-2 p-8 text-center">
                      <BookOpen className="h-8 w-8 text-[var(--yuuko-green)]/60" />
                      <p className="text-sm font-medium text-foreground">
                        まだ表示できる辞書項目がありません
                      </p>
                      <p className="text-xs leading-relaxed text-muted-foreground">
                        ニュース詳細画面で用語を保存すると、ここに一覧表示されます。
                      </p>
                    </CardContent>
                  </Card>
                ) : null}

                {!isLoading && paginatedEntries.length > 0 ? (
                  <div className="space-y-2">
                    {paginatedEntries.map((entry) => (
                      <DictionaryEntryCard
                        key={entry.id}
                        entry={entry}
                        isSelected={entry.id === selectedEntryId}
                        onClick={() => setSelectedEntryId(entry.id)}
                        onToggleFavorite={handleToggleFavorite}
                      />
                    ))}
                  </div>
                ) : null}

                {!isLoading ? (
                  <Pagination
                    currentPage={currentPage}
                    totalPages={totalPages}
                    totalItems={sortedEntries.length}
                    itemsPerPage={itemsPerPage}
                    onPageChange={setCurrentPage}
                  />
                ) : null}
              </CardContent>
            </Card>
          </div>

          <aside className="w-72 shrink-0 overflow-y-auto border-l border-border/50 bg-[var(--yuuko-cream-dark)]/30 p-4">
            <div className="relative mb-2">
              <div className="relative rounded-2xl border-2 border-[var(--yuuko-green)]/30 bg-white p-3">
                <p className="text-xs leading-relaxed text-foreground">
                  わからない言葉は
                  <br />
                  ここで見返せるよ〜！
                </p>
                <div className="absolute -bottom-2 left-8 h-0 w-0 border-l-8 border-r-8 border-t-8 border-l-transparent border-r-transparent border-t-white" />
                <div className="absolute -bottom-3 left-8 h-0 w-0 border-l-8 border-r-8 border-t-8 border-l-transparent border-r-transparent border-t-[var(--yuuko-green)]/30" />
                <PawIcon className="absolute right-2 top-2 h-4 w-4 text-[var(--yuuko-green)]/40" />
              </div>
            </div>

            <div className="mb-4 flex justify-center">
              <div className="relative">
                <Image
                  src="/assets/yuuko.png"
                  width={144}
                  height={144}
                  alt="ゆうこ"
                  className="h-auto w-36 animate-float"
                  style={{ animation: "float 3s ease-in-out infinite" }}
                />
                <style jsx>{`
                  @keyframes float {
                    0%,
                    100% {
                      transform: translateY(0);
                    }
                    50% {
                      transform: translateY(-6px);
                    }
                  }
                `}</style>
              </div>
            </div>

            {selectedEntry ? (
              <div className="space-y-3">
                <Card className="border-border/50 py-0">
                  <CardContent className="p-3">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex items-center gap-2">
                        <h3 className="text-base font-bold text-foreground">
                          {selectedEntry.term}
                        </h3>
                        <TypeBadge type={selectedEntry.type} />
                      </div>
                      <button
                        type="button"
                        className="flex items-center gap-1 text-xs"
                        onClick={() =>
                          handleToggleFavorite(
                            selectedEntry.id,
                            selectedEntry.isFavorite
                          )
                        }
                      >
                        <Star
                          className={`h-4 w-4 ${
                            selectedEntry.isFavorite
                              ? "fill-yellow-400 text-yellow-400"
                              : "text-muted-foreground"
                          }`}
                        />
                        <span className="text-muted-foreground">
                          お気に入り
                        </span>
                      </button>
                    </div>
                  </CardContent>
                </Card>

                <Card className="border-border/50 py-0">
                  <CardContent className="p-3">
                    <div className="mb-2 flex items-center gap-1.5">
                      <BookOpen className="h-4 w-4 text-[var(--yuuko-green)]" />
                      <h4 className="text-sm font-semibold text-foreground">
                        やさしい説明
                      </h4>
                    </div>
                    <p className="whitespace-pre-line text-xs leading-relaxed text-foreground">
                      {selectedEntry.fullDescription}
                    </p>
                    <div className="mt-2 flex justify-end">
                      <PawIcon className="h-4 w-4 text-[var(--yuuko-green)]/40" />
                    </div>
                  </CardContent>
                </Card>

                <Card className="border-border/50 py-0">
                  <CardContent className="p-3">
                    <div className="mb-2 flex items-center justify-between gap-2">
                      <div className="flex items-center gap-1.5">
                        <Newspaper className="h-4 w-4 text-[var(--yuuko-green)]" />
                        <h4 className="text-sm font-semibold text-foreground">
                          関連ニュース
                        </h4>
                      </div>
                      <button
                        type="button"
                        className="flex items-center gap-0.5 text-[10px] text-[var(--yuuko-green)] hover:underline"
                        onClick={handleOpenRelatedArticle}
                        disabled={!selectedEntry.relatedArticleId}
                      >
                        開く
                        <ChevronRight className="h-3 w-3" />
                      </button>
                    </div>
                    {selectedEntry.relatedArticle ? (
                      <div className="flex items-start gap-2">
                        <RelatedNewsThumbnail />
                        <div className="min-w-0 flex-1">
                          <h5 className="line-clamp-2 text-xs font-medium text-foreground">
                            {selectedEntry.relatedArticle}
                          </h5>
                          <p className="mt-1 text-[10px] text-muted-foreground">
                            関連記事から保存された辞書項目です
                          </p>
                        </div>
                      </div>
                    ) : (
                      <p className="text-xs text-muted-foreground">
                        関連記事情報はまだ登録されていません。
                      </p>
                    )}
                  </CardContent>
                </Card>

                <Card className="border-amber-200 bg-amber-50/50 py-0">
                  <CardContent className="p-3">
                    <div className="mb-2 flex items-center justify-between gap-2">
                      <div className="flex items-center gap-1.5">
                        <Pencil className="h-4 w-4 text-amber-600" />
                        <h4 className="text-sm font-semibold text-foreground">
                          メモ
                        </h4>
                      </div>
                      {!isEditingMemo ? (
                        <button
                          type="button"
                          className="rounded border border-[var(--yuuko-green)]/30 bg-white px-2 py-0.5 text-[10px] text-[var(--yuuko-green)] hover:underline"
                          onClick={handleStartEditMemo}
                        >
                          編集
                        </button>
                      ) : (
                        <div className="flex items-center gap-1">
                          <button
                            type="button"
                            className="rounded border border-[var(--yuuko-green)]/30 bg-white px-2 py-0.5 text-[10px] text-[var(--yuuko-green)] hover:underline"
                            onClick={handleSaveMemo}
                          >
                            <Save className="h-3 w-3" />
                          </button>
                          <button
                            type="button"
                            className="rounded border border-red-200 bg-white px-2 py-0.5 text-[10px] text-red-500 hover:underline"
                            onClick={() => setIsEditingMemo(false)}
                          >
                            <X className="h-3 w-3" />
                          </button>
                        </div>
                      )}
                    </div>
                    {!isEditingMemo ? (
                      <p className="text-xs leading-relaxed text-foreground">
                        {selectedEntry.memo || "メモはまだありません。"}
                      </p>
                    ) : (
                      <Textarea
                        value={editMemoValue}
                        onChange={(e) => setEditMemoValue(e.target.value)}
                        className="min-h-[80px] text-xs"
                        placeholder="メモを入力..."
                      />
                    )}
                    <div className="mt-2 flex justify-end">
                      <PawIcon className="h-4 w-4 text-amber-400/50" />
                    </div>
                  </CardContent>
                </Card>

                <div className="flex items-center gap-2 pt-2">
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1 border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                    onClick={() =>
                      console.log("Share dictionary entry:", selectedEntry.id)
                    }
                  >
                    <Share2 className="mr-1 h-4 w-4" />
                    共有する
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1 border-red-300 text-red-500 hover:bg-red-50"
                    onClick={handleDeleteEntry}
                  >
                    <Trash2 className="mr-1 h-4 w-4" />
                    削除する
                  </Button>
                </div>
              </div>
            ) : (
              <Card className="border-dashed border-border/60 py-0 shadow-none">
                <CardContent className="p-6 text-center">
                  <BookOpen className="mx-auto mb-2 h-8 w-8 text-[var(--yuuko-green)]/60" />
                  <p className="text-sm font-medium text-foreground">
                    辞書項目を選ぶと詳細が見られます
                  </p>
                </CardContent>
              </Card>
            )}
          </aside>
        </main>
      </div>

      <footer className="flex h-8 shrink-0 items-center justify-between border-t border-border/50 bg-white px-4">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1.5">
            <Bell className="h-3.5 w-3.5 text-muted-foreground" />
            <span className="text-xs text-muted-foreground">お知らせ</span>
          </div>
          <span className="text-xs text-muted-foreground">|</span>
          <div className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-full bg-[var(--yuuko-green)]" />
            <span className="text-xs text-foreground">
              新しいニュースが3件届いてるよ！
            </span>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <button
            type="button"
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <HelpCircle className="h-4 w-4" />
          </button>
          <PawIcon className="h-4 w-4 text-muted-foreground" />
        </div>
      </footer>
    </div>
  );
}
