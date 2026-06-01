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
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

// ============ Type Definitions ============

type DictionaryEntryType = "単語" | "フレーズ" | "要点説明";

interface DictionaryEntry {
  id: string;
  term: string;
  type: DictionaryEntryType;
  shortDescription: string;
  fullDescription: string;
  lastViewed: string;
  relatedArticle: string;
  isFavorite: boolean;
  iconType: "robot" | "brain" | "database" | "chip" | "circuit";
}

interface RelatedNews {
  id: string;
  title: string;
  source: string;
  datetime: string;
  isNew: boolean;
  thumbnailType: string;
}

interface NavigationItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
}

// ============ Mock Data ============

const mockNavigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: false },
  { id: "history", label: "ニュース履歴", icon: Clock, isActive: false },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: true },
  { id: "customize", label: "カスタマイズ", icon: Sparkles, isActive: false },
  { id: "gacha", label: "ガチャ", icon: Gift, isActive: false },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

const mockDictionaryEntries: DictionaryEntry[] = [
  {
    id: "1",
    term: "AIエージェント",
    type: "単語",
    shortDescription: "自ら考え、判断し、目標に向かって行動できるAIのこと。",
    fullDescription:
      "AIエージェントとは、自分で考えて、目標を決めて、その目標を達成するために行動できるAIのことだよ。\nたとえば、「スケジュールを整理して」「予約を取って」とお願いすると、必要な情報を集めて、自分で手順を考えて作業を進めてくれるんだ！",
    lastViewed: "2025/05/20",
    relatedArticle: "AIコーディング支援ツールの最新動向まとめ",
    isFavorite: true,
    iconType: "robot",
  },
  {
    id: "2",
    term: "マルチモーダル",
    type: "単語",
    shortDescription:
      "テキスト・画像・音声など、複数の種類の情報を同時に扱うこと。",
    fullDescription:
      "マルチモーダルとは、テキスト、画像、音声、動画など、異なる種類のデータを組み合わせて処理できる技術のことだよ。\n最近のAIは、文章だけでなく画像を見て説明したり、音声を聞いて返答したりできるようになってきているんだ！",
    lastViewed: "2025/05/18",
    relatedArticle: "iOS 19、注目の新機能を徹底解説",
    isFavorite: false,
    iconType: "brain",
  },
  {
    id: "3",
    term: "RAG",
    type: "単語",
    shortDescription: "外部の情報を検索してAIの回答に活用する仕組みのこと。",
    fullDescription:
      "RAG（Retrieval-Augmented Generation）は、AIが回答を生成する前に、外部のデータベースや文書から関連情報を検索して取り込む技術だよ。\nこれにより、AIは最新の情報や専門的な知識を使って、より正確な回答ができるようになるんだ！",
    lastViewed: "2025/05/16",
    relatedArticle: "再生可能エネルギーの未来と課題",
    isFavorite: false,
    iconType: "database",
  },
  {
    id: "4",
    term: "半導体不足",
    type: "要点説明",
    shortDescription:
      "世界的に半導体の供給が需要に追いつかない状態のこと。",
    fullDescription:
      "半導体不足とは、世界中で半導体（チップ）の需要が供給を大きく上回っている状態のことだよ。\nスマホ、パソコン、自動車など、あらゆる電子機器に半導体が使われているから、不足すると製品が作れなくなってしまうんだ。",
    lastViewed: "2025/05/14",
    relatedArticle: "日経平均、続伸　半導体株がけん引",
    isFavorite: false,
    iconType: "chip",
  },
  {
    id: "5",
    term: "エッジAI",
    type: "単語",
    shortDescription: "データをクラウドに送らず、端末側でAI処理を行う技術。",
    fullDescription:
      "エッジAIとは、スマホやカメラなどの端末（エッジデバイス）で直接AI処理を行う技術のことだよ。\nクラウドにデータを送らなくていいから、処理が速くて、プライバシーも守りやすいんだ！",
    lastViewed: "2025/05/12",
    relatedArticle: "身近になる生成AI、生活をどう変える？",
    isFavorite: false,
    iconType: "circuit",
  },
];

const mockRelatedNews: RelatedNews = {
  id: "news-1",
  title: "AIコーディング支援ツールの最新動向まとめ",
  source: "TechCrunch Japan",
  datetime: "2025/05/20 10:30",
  isNew: true,
  thumbnailType: "ai",
};

const mockMemo =
  "ツールによって得意分野が違うので目的に応じて使い分けたい。RAGとの組み合わせも注目！";

type FilterType = "all" | "単語" | "フレーズ" | "要点説明" | "favorite";

const filterOptions: { id: FilterType; label: string; icon?: React.ComponentType<{ className?: string }> }[] = [
  { id: "all", label: "すべて", icon: List },
  { id: "単語", label: "単語", icon: Search },
  { id: "フレーズ", label: "フレーズ", icon: Sparkles },
  { id: "要点説明", label: "要点説明", icon: Pencil },
  { id: "favorite", label: "付き", icon: Star },
];

// ============ Sub Components ============

function PawIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      className={className}
    >
      <ellipse cx="12" cy="17" rx="5" ry="4" />
      <circle cx="6" cy="10" r="2.5" />
      <circle cx="18" cy="10" r="2.5" />
      <circle cx="9" cy="6" r="2" />
      <circle cx="15" cy="6" r="2" />
    </svg>
  );
}

function EntryIcon({ type }: { type: DictionaryEntry["iconType"] }) {
  const iconMap = {
    robot: Bot,
    brain: Brain,
    database: Database,
    chip: Cpu,
    circuit: CircuitBoard,
  };
  const Icon = iconMap[type];
  const bgColors = {
    robot: "bg-emerald-100",
    brain: "bg-cyan-100",
    database: "bg-blue-100",
    chip: "bg-purple-100",
    circuit: "bg-teal-100",
  };

  return (
    <div
      className={`w-12 h-12 rounded-lg ${bgColors[type]} flex items-center justify-center flex-shrink-0`}
    >
      <Icon className="w-6 h-6 text-[var(--yuuko-green)]" />
    </div>
  );
}

function TypeBadge({ type }: { type: DictionaryEntryType }) {
  const colors = {
    単語: "bg-[var(--yuuko-green)] text-white",
    フレーズ: "bg-blue-500 text-white",
    要点説明: "bg-amber-500 text-white",
  };
  return (
    <Badge className={`${colors[type]} border-0 text-[10px] px-2 py-0`}>
      {type}
    </Badge>
  );
}

function DictionaryEntryCard({
  entry,
  isSelected,
  onClick,
}: {
  entry: DictionaryEntry;
  isSelected: boolean;
  onClick: () => void;
}) {
  return (
    <Card
      className={`cursor-pointer transition-all hover:shadow-md py-2 px-0 ${
        isSelected
          ? "border-[var(--yuuko-green)] border-2 bg-[var(--yuuko-green-light)]/30"
          : "border-border/50 hover:border-border"
      }`}
      onClick={onClick}
    >
      <CardContent className="p-3 flex items-start gap-3">
        <EntryIcon type={entry.iconType} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className="font-semibold text-sm text-foreground">
              {entry.term}
            </span>
            <TypeBadge type={entry.type} />
          </div>
          <p className="text-xs text-muted-foreground line-clamp-1 mb-1.5">
            {entry.shortDescription}
          </p>
          <div className="flex items-center gap-1 text-[10px] text-muted-foreground">
            <span>最終閲覧：{entry.lastViewed}</span>
            <span className="mx-1">|</span>
            <span className="truncate">関連記事：{entry.relatedArticle}</span>
          </div>
        </div>
        <button
          className="flex-shrink-0 p-1"
          onClick={(e) => {
            e.stopPropagation();
            console.log("Toggle favorite:", entry.id);
          }}
        >
          <Star
            className={`w-5 h-5 ${
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
  const startItem = (currentPage - 1) * itemsPerPage + 1;
  const endItem = Math.min(currentPage * itemsPerPage, totalItems);

  return (
    <div className="flex items-center justify-between mt-4">
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
          <ChevronLeft className="w-4 h-4" />
        </Button>
        {[1, 2, 3].map((page) => (
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
        <span className="text-muted-foreground px-1">…</span>
        <Button
          variant="ghost"
          size="sm"
          className="h-7 w-7 p-0"
          onClick={() => onPageChange(totalPages)}
        >
          {totalPages}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2"
          onClick={() => onPageChange(currentPage + 1)}
          disabled={currentPage === totalPages}
        >
          <ChevronRight className="w-4 h-4" />
        </Button>
      </div>
    </div>
  );
}

function RelatedNewsThumbnail() {
  return (
    <div className="w-14 h-14 rounded-lg bg-gradient-to-br from-blue-900 to-indigo-900 flex items-center justify-center flex-shrink-0 overflow-hidden">
      <svg viewBox="0 0 40 40" className="w-8 h-8">
        <circle cx="20" cy="20" r="8" fill="rgba(100,200,255,0.3)" />
        <circle cx="20" cy="20" r="4" fill="rgba(150,220,255,0.5)" />
        {[0, 60, 120, 180, 240, 300].map((angle, i) => (
          <circle
            key={i}
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

// ============ Main Component ============

export default function DictionaryScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [searchQuery, setSearchQuery] = React.useState("");
  const [activeFilter, setActiveFilter] = React.useState<FilterType>("all");
  const [selectedEntryId, setSelectedEntryId] = React.useState("1");
  const [currentPage, setCurrentPage] = React.useState(1);
  const [isAutoStart, setIsAutoStart] = React.useState(true);

  const selectedEntry = mockDictionaryEntries.find(
    (e) => e.id === selectedEntryId
  );

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const handleGoHome = () => {
    if (onNavigate) {
      onNavigate("home");
    }
  };

  return (
    <div className="h-dvh w-full bg-background flex flex-col overflow-hidden">
      <AppTitleBar className="bg-white border-border/50" />

      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar */}
        <aside className="w-52 bg-white border-r border-border/50 flex flex-col flex-shrink-0">
          {/* Back to Home Button */}
          <div className="p-3">
            <Button
              variant="outline"
              className="w-full justify-start gap-2 text-[var(--yuuko-green)] border-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
              onClick={handleGoHome}
            >
              <ArrowLeft className="w-4 h-4" />
              ホームへ戻る
            </Button>
          </div>

          {/* Navigation */}
          <nav className="flex-1 px-3 space-y-1 overflow-y-auto">
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
          <div className="p-3">
            <Card className="border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/30 py-3">
              <CardContent className="p-3">
                <p className="text-xs font-medium text-[var(--yuuko-green)] mb-2">
                  ゆうこの一言
                </p>
                <p className="text-[11px] text-foreground leading-relaxed">
                  難しい言葉も、
                  <br />
                  少しずつ覚えれば
                  <br />
                  こわくないよ〜！
                  <br />
                  一緒にレベルアップしよっ♪
                </p>
                <div className="flex justify-end mt-2">
                  <PawIcon className="w-4 h-4 text-[var(--yuuko-green)]/50" />
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Auto Start & Exit */}
          <div className="p-3 border-t border-border/50">
            <div className="flex items-center gap-2 mb-2">
              <span className="text-xs text-muted-foreground">自動起動：</span>
              <button
                className={`text-xs font-medium ${
                  isAutoStart ? "text-[var(--yuuko-green)]" : "text-muted-foreground"
                }`}
                onClick={() => setIsAutoStart(!isAutoStart)}
              >
                {isAutoStart ? "ON" : "OFF"}
              </button>
              {isAutoStart && (
                <span className="w-2 h-2 rounded-full bg-[var(--yuuko-green)]" />
              )}
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

        {/* Main Content Area */}
        <main className="flex-1 min-w-0 flex overflow-hidden">
          {/* Center - Dictionary List */}
          <div className="flex-1 p-6 overflow-y-auto">
            {/* Breadcrumb */}
            <div className="flex items-center gap-2 text-xs text-muted-foreground mb-4">
              <button
                className="hover:text-foreground transition-colors"
                onClick={handleGoHome}
              >
                ホーム
              </button>
              <ChevronRight className="w-3 h-3" />
              <span className="text-foreground">ゆうこ辞書</span>
            </div>

            {/* Title */}
            <div className="flex items-center gap-2 mb-6">
              <BookOpen className="w-6 h-6 text-[var(--yuuko-green)]" />
              <h1 className="text-xl font-bold text-foreground">ゆうこ辞書</h1>
            </div>

            {/* Search */}
            <div className="flex items-center gap-3 mb-4">
              <div className="relative flex-1">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
                <Input
                  placeholder="単語やフレーズで検索"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-10 bg-white"
                />
              </div>
              <Button variant="outline" className="gap-2">
                <Filter className="w-4 h-4" />
                絞り込み
              </Button>
            </div>

            {/* Filter Chips */}
            <div className="flex items-center gap-2 mb-6">
              {filterOptions.map((filter) => {
                const Icon = filter.icon;
                const isActive = activeFilter === filter.id;
                return (
                  <button
                    key={filter.id}
                    className={`flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium transition-all ${
                      isActive
                        ? "bg-[var(--yuuko-green)] text-white"
                        : "bg-white border border-border text-muted-foreground hover:border-[var(--yuuko-green)]/50"
                    }`}
                    onClick={() => setActiveFilter(filter.id)}
                  >
                    {Icon && <Icon className="w-3.5 h-3.5" />}
                    {filter.label}
                  </button>
                );
              })}
            </div>

            {/* Dictionary List */}
            <Card className="border-border/50 py-0">
              <CardContent className="p-4">
                {/* List Header */}
                <div className="flex items-center justify-between mb-4">
                  <h2 className="text-sm font-semibold text-foreground">
                    登録済みの辞書
                  </h2>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">
                      並び替え：
                    </span>
                    <Select defaultValue="recent">
                      <SelectTrigger className="w-[120px] h-8 text-xs">
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

                {/* Entry List */}
                <div className="space-y-2">
                  {mockDictionaryEntries.map((entry) => (
                    <DictionaryEntryCard
                      key={entry.id}
                      entry={entry}
                      isSelected={entry.id === selectedEntryId}
                      onClick={() => setSelectedEntryId(entry.id)}
                    />
                  ))}
                </div>

                {/* Pagination */}
                <Pagination
                  currentPage={currentPage}
                  totalPages={6}
                  totalItems={28}
                  itemsPerPage={5}
                  onPageChange={setCurrentPage}
                />
              </CardContent>
            </Card>
          </div>

          {/* Right Sidebar - Yuuko & Detail Panel */}
        <aside className="w-72 bg-[var(--yuuko-cream-dark)]/30 border-l border-border/50 p-4 overflow-y-auto flex-shrink-0">
            {/* Yuuko Speech Bubble */}
            <div className="relative mb-2">
              <div className="bg-white border-2 border-[var(--yuuko-green)]/30 rounded-2xl p-3 relative">
                <p className="text-xs text-foreground leading-relaxed">
                  わからない言葉は
                  <br />
                  ここで見返せるよ〜！
                </p>
                <div className="absolute -bottom-2 left-8 w-0 h-0 border-l-8 border-r-8 border-t-8 border-l-transparent border-r-transparent border-t-white" />
                <div className="absolute -bottom-3 left-8 w-0 h-0 border-l-8 border-r-8 border-t-8 border-l-transparent border-r-transparent border-t-[var(--yuuko-green)]/30" />
                <PawIcon className="absolute top-2 right-2 w-4 h-4 text-[var(--yuuko-green)]/40" />
              </div>
            </div>

            {/* Yuuko Character */}
            <div className="flex justify-center mb-4">
              <div className="relative">
                <Image
                  src="/assets/yuuko.png"
                  width={144}
                  height={144}
                  alt="ゆうこ"
                  className="w-36 h-auto animate-float"
                  style={{
                    animation: "float 3s ease-in-out infinite",
                  }}
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

            {/* Detail Panel */}
            {selectedEntry && (
              <div className="space-y-3">
                {/* Entry Header */}
                <Card className="border-border/50 py-0">
                  <CardContent className="p-3">
                    <div className="flex items-start justify-between">
                      <div className="flex items-center gap-2">
                        <h3 className="font-bold text-base text-foreground">
                          {selectedEntry.term}
                        </h3>
                        <TypeBadge type={selectedEntry.type} />
                      </div>
                      <div className="flex items-center gap-1 text-xs">
                        <Star
                          className={`w-4 h-4 ${
                            selectedEntry.isFavorite
                              ? "fill-yellow-400 text-yellow-400"
                              : "text-muted-foreground"
                          }`}
                        />
                        <span className="text-muted-foreground">お気に入り</span>
                      </div>
                    </div>
                  </CardContent>
                </Card>

                {/* Full Description */}
                <Card className="border-border/50 py-0">
                  <CardContent className="p-3">
                    <div className="flex items-center gap-1.5 mb-2">
                      <BookOpen className="w-4 h-4 text-[var(--yuuko-green)]" />
                      <h4 className="text-sm font-semibold text-foreground">
                        やさしい説明
                      </h4>
                    </div>
                    <p className="text-xs text-foreground leading-relaxed whitespace-pre-line">
                      {selectedEntry.fullDescription}
                    </p>
                    <div className="flex justify-end mt-2">
                      <PawIcon className="w-4 h-4 text-[var(--yuuko-green)]/40" />
                    </div>
                  </CardContent>
                </Card>

                {/* Related News */}
                <Card className="border-border/50 py-0">
                  <CardContent className="p-3">
                    <div className="flex items-center justify-between mb-2">
                      <div className="flex items-center gap-1.5">
                        <Newspaper className="w-4 h-4 text-[var(--yuuko-green)]" />
                        <h4 className="text-sm font-semibold text-foreground">
                          関連ニュース
                        </h4>
                      </div>
                      <button className="text-[10px] text-[var(--yuuko-green)] hover:underline flex items-center gap-0.5">
                        すべて見る
                        <ChevronRight className="w-3 h-3" />
                      </button>
                    </div>
                    <div className="flex items-start gap-2">
                      <RelatedNewsThumbnail />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-start gap-1">
                          <h5 className="text-xs font-medium text-foreground line-clamp-2 flex-1">
                            {mockRelatedNews.title}
                          </h5>
                          {mockRelatedNews.isNew && (
                            <Badge className="bg-red-500 text-white border-0 text-[9px] px-1 py-0 flex-shrink-0">
                              NEW
                            </Badge>
                          )}
                        </div>
                        <p className="text-[10px] text-muted-foreground mt-1">
                          {mockRelatedNews.source} ・ {mockRelatedNews.datetime}
                        </p>
                      </div>
                    </div>
                  </CardContent>
                </Card>

                {/* Memo */}
                <Card className="border-amber-200 bg-amber-50/50 py-0">
                  <CardContent className="p-3">
                    <div className="flex items-center justify-between mb-2">
                      <div className="flex items-center gap-1.5">
                        <Pencil className="w-4 h-4 text-amber-600" />
                        <h4 className="text-sm font-semibold text-foreground">
                          メモ（自分用の付箋）
                        </h4>
                      </div>
                      <button className="text-[10px] text-[var(--yuuko-green)] hover:underline flex items-center gap-0.5 bg-white px-2 py-0.5 rounded border border-[var(--yuuko-green)]/30">
                        編集
                      </button>
                    </div>
                    <p className="text-xs text-foreground leading-relaxed">
                      {mockMemo}
                    </p>
                    <div className="flex justify-end mt-2">
                      <PawIcon className="w-4 h-4 text-amber-400/50" />
                    </div>
                  </CardContent>
                </Card>

                {/* Action Buttons */}
                <div className="flex items-center gap-2 pt-2">
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1 text-[var(--yuuko-green)] border-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                    onClick={() => console.log("Share:", selectedEntry.id)}
                  >
                    <Share2 className="w-4 h-4 mr-1" />
                    共有する
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1 text-red-500 border-red-300 hover:bg-red-50"
                    onClick={() => console.log("Delete:", selectedEntry.id)}
                  >
                    <Trash2 className="w-4 h-4 mr-1" />
                    削除する
                  </Button>
                </div>
              </div>
            )}
          </aside>
        </main>
      </div>

      {/* Footer / Status Bar */}
      <footer className="h-8 bg-white border-t border-border/50 flex items-center justify-between px-4 flex-shrink-0">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1.5">
            <Bell className="w-3.5 h-3.5 text-muted-foreground" />
            <span className="text-xs text-muted-foreground">お知らせ</span>
          </div>
          <span className="text-xs text-muted-foreground">|</span>
          <div className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-[var(--yuuko-green)]" />
            <span className="text-xs text-foreground">
              新しいニュースが3件届いてるよ！
            </span>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <button className="text-muted-foreground hover:text-foreground transition-colors">
            <HelpCircle className="w-4 h-4" />
          </button>
          <PawIcon className="w-4 h-4 text-muted-foreground" />
        </div>
      </footer>
    </div>
  );
}
