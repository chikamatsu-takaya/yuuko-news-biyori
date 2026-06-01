"use client";

import * as React from "react";
import Image from "next/image";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
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
  ChevronDown,
  Bell,
  HelpCircle,
  RotateCcw,
  Share2,
  FolderOpen,
} from "lucide-react";

// Types
type ReadState = "read" | "unread";
type NewsCategory =
  | "AI・テクノロジー"
  | "環境・エネルギー"
  | "モバイル"
  | "ビジネス"
  | "宇宙"
  | "ライフスタイル";

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

const mockFilterChips: FilterChip[] = [
  { id: "all", label: "すべて", icon: List },
  { id: "unread", label: "未読", icon: Circle },
  { id: "read", label: "既読", icon: CheckCircle },
  { id: "favorite", label: "お気に入り", icon: Star },
  { id: "archive", label: "アーカイブ", icon: Archive },
  { id: "ai-tech", label: "AI・テクノロジー" },
  { id: "date", label: "日付順", icon: ChevronDown },
];

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
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="12" cy="12" r="3" />
        <path d="M12 2v4M12 18v4M2 12h4M18 12h4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83" />
      </svg>
    ),
    energy: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
      </svg>
    ),
    mobile: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="5" y="2" width="14" height="20" rx="2" />
        <circle cx="12" cy="18" r="1" />
      </svg>
    ),
    business: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <path d="M3 3v18h18" />
        <path d="M7 16l4-4 4 4 5-6" />
      </svg>
    ),
    robot: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="4" y="8" width="16" height="12" rx="2" />
        <circle cx="9" cy="14" r="2" />
        <circle cx="15" cy="14" r="2" />
        <path d="M12 2v4M8 4h8" />
      </svg>
    ),
    space: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="12" cy="12" r="10" />
        <ellipse cx="12" cy="12" rx="10" ry="4" />
        <path d="M12 2a15 15 0 010 20M12 2a15 15 0 000 20" />
      </svg>
    ),
    lifestyle: (
      <svg className="w-8 h-8 text-white/90" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <path d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8zM6 1v3M10 1v3M14 1v3" />
      </svg>
    ),
  };

  return (
    <div
      className={`w-16 h-16 rounded-lg bg-gradient-to-br ${thumbnailStyles[type]} flex items-center justify-center flex-shrink-0`}
    >
      {icons[type]}
    </div>
  );
}

// Filter Chip
function FilterChipButton({
  chip,
  isActive,
  onClick,
}: {
  chip: FilterChip;
  isActive: boolean;
  onClick: () => void;
}) {
  const Icon = chip.icon;
  return (
    <button
      className={`flex items-center gap-1.5 px-3 py-1.5 rounded-full text-sm transition-all border ${
        isActive
          ? "bg-[var(--yuuko-green)] text-white border-[var(--yuuko-green)]"
          : "bg-white text-muted-foreground border-border hover:border-[var(--yuuko-green)]/50"
      }`}
      onClick={onClick}
    >
      {Icon && <Icon className="w-4 h-4" />}
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
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [searchQuery, setSearchQuery] = React.useState("");
  const [activeFilter, setActiveFilter] = React.useState("all");
  const [selectedItemId, setSelectedItemId] = React.useState("1");

  const selectedItem = mockHistoryItems.find((item) => item.id === selectedItemId);

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const getCategoryColor = (category: NewsCategory) => {
    const colors: Record<NewsCategory, string> = {
      "AI・テクノロジー": "bg-[var(--yuuko-green)] text-white",
      "環境・エネルギー": "bg-teal-500 text-white",
      モバイル: "bg-purple-500 text-white",
      ビジネス: "bg-emerald-500 text-white",
      宇宙: "bg-indigo-500 text-white",
      ライフスタイル: "bg-orange-500 text-white",
    };
    return colors[category];
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
                <span className="text-[var(--yuuko-green)]">🐾</span>
              </div>
            </CardContent>
          </Card>

          {/* Auto Start */}
          <div className="mt-4 space-y-2">
            <div className="flex items-center gap-2 text-sm">
              <span className="text-muted-foreground">自動起動：ON</span>
              <span className="w-2 h-2 rounded-full bg-[var(--yuuko-green)]" />
            </div>
            <Button variant="outline" size="sm" className="w-full text-xs">
              常駐を終了する
            </Button>
          </div>
        </aside>

        {/* Center - History List */}
        <main className="flex-1 min-w-0 flex flex-col p-4 overflow-hidden">
          {/* Breadcrumb */}
          <div className="flex items-center gap-2 text-sm text-muted-foreground mb-3">
            <span className="hover:text-[var(--yuuko-green)] cursor-pointer">ホーム</span>
            <span>&gt;</span>
            <span className="text-foreground">ニュース履歴</span>
          </div>

          {/* Title */}
          <div className="flex items-center gap-2 mb-4">
            <Clock className="w-6 h-6 text-[var(--yuuko-green)]" />
            <h1 className="text-xl font-bold text-foreground">ニュース履歴</h1>
          </div>

          {/* Search */}
          <div className="flex items-center gap-2 mb-3">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
              <Input
                placeholder="キーワードで検索（記事タイトル・本文など）"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-9 bg-white"
              />
            </div>
            <Button variant="outline" className="gap-2">
              <Filter className="w-4 h-4" />
              絞り込み
            </Button>
          </div>

          {/* Filter Chips */}
          <div className="flex items-center gap-2 mb-4 flex-wrap">
            {mockFilterChips.map((chip) => (
              <FilterChipButton
                key={chip.id}
                chip={chip}
                isActive={activeFilter === chip.id}
                onClick={() => setActiveFilter(chip.id)}
              />
            ))}
          </div>

          {/* History List */}
          <div className="flex-1 overflow-y-auto space-y-2 pr-1">
            {mockHistoryItems.map((item) => (
              <HistoryItemCard
                key={item.id}
                item={item}
                isSelected={selectedItemId === item.id}
                onClick={() => setSelectedItemId(item.id)}
              />
            ))}
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
                <span className="text-[var(--yuuko-green)]">🐾</span>
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
                  <Newspaper className="w-3 h-3" />
                  <span>{selectedItem.source}</span>
                  <span>・</span>
                  <span>{selectedItem.datetime}</span>
                </div>

                {/* Description */}
                <p className="text-xs text-muted-foreground leading-relaxed mb-4">
                  {selectedItem.description}
                </p>

                <div className="border-t border-border my-2" />

                {/* Action Buttons */}
                <div className="space-y-2 mt-auto">
                  <Button
                    className="w-full bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white gap-2"
                    onClick={() => console.log("もう一度見る:", selectedItem.id)}
                  >
                    <RotateCcw className="w-4 h-4" />
                    もう一度見る
                  </Button>
                  <Button
                    variant="outline"
                    className="w-full gap-2"
                    onClick={() => console.log("お気に入り解除:", selectedItem.id)}
                  >
                    <Star className="w-4 h-4" />
                    お気に入り解除
                  </Button>
                  <Button
                    variant="outline"
                    className="w-full gap-2"
                    onClick={() => console.log("アーカイブを展開:", selectedItem.id)}
                  >
                    <FolderOpen className="w-4 h-4" />
                    アーカイブを展開
                  </Button>
                  <Button
                    variant="outline"
                    className="w-full gap-2"
                    onClick={() => console.log("記事をシェア:", selectedItem.id)}
                  >
                    <Share2 className="w-4 h-4" />
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
          <Bell className="w-3.5 h-3.5" />
          <span>お知らせ</span>
          <span className="mx-1">|</span>
          <span className="text-[var(--yuuko-green)]">●</span>
          <span>履歴から3件、お気に入り登録されています！</span>
        </div>
        <div className="flex items-center gap-3">
          <HelpCircle className="w-4 h-4 cursor-pointer hover:text-foreground" />
          <span className="text-[var(--yuuko-green)]">🐾</span>
        </div>
      </footer>

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
