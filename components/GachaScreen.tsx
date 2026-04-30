"use client";

import * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import {
  Home,
  Newspaper,
  Clock,
  BookOpen,
  Sparkles,
  Dices,
  Settings,
  ArrowLeft,
  Bell,
  HelpCircle,
  Minus,
  Square,
  X,
  ChevronRight,
  MessageCircle,
  Star,
  Plus,
  ShoppingCart,
  History,
  Palette,
  Heart,
  Tag,
  AudioWaveform,
  Smile,
  Gift,
  Clover,
  Rainbow,
} from "lucide-react";

// ============================================
// TypeScript Types
// ============================================

interface GachaState {
  starFragments: number;
  friendshipRank: number;
  currentPoints: number;
  nextRankPoints: number;
  bonusText: string;
  dailyDiscountText: string;
  resetText: string;
}

interface PickupItem {
  name: string;
  description: string;
  period: string;
  rarity: number;
}

interface GachaButtonConfig {
  id: string;
  label: string;
  cost: number;
  guaranteedText: string | null;
}

interface LineupItem {
  id: string;
  name: string;
  icon: React.ReactNode;
  rarity: number;
}

interface RecentResult {
  time: string;
  name: string;
  icon: React.ReactNode;
  rarity: number;
}

interface NewItem {
  id: string;
  name: string;
  icon: React.ReactNode;
  isNew: boolean;
}

interface NavigationItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
}

// ============================================
// Mock Data
// ============================================

const mockGachaState: GachaState = {
  starFragments: 1250,
  friendshipRank: 15,
  currentPoints: 350,
  nextRankPoints: 1000,
  bonusText: "ランク15特典：★3以上の出現率 +1.5%",
  dailyDiscountText: "1日1回限定割引",
  resetText: "毎日5:00にリセットされます",
};

const mockPickup: PickupItem = {
  name: "緑のずきん",
  description: "出現率UP！",
  period: "〜 6/23 13:59まで",
  rarity: 4,
};

const mockGachaButtons: GachaButtonConfig[] = [
  { id: "single", label: "1回まわす", cost: 100, guaranteedText: null },
  { id: "ten", label: "10回まわす", cost: 1000, guaranteedText: "★3以上 1個確定！" },
];

const mockLineup: LineupItem[] = [
  { id: "theme", name: "テーマ", icon: <Palette className="w-4 h-4" />, rarity: 4 },
  { id: "balloon", name: "吹き出し", icon: <MessageCircle className="w-4 h-4" />, rarity: 3 },
  { id: "deco_head", name: "デコ（頭）", icon: <Sparkles className="w-4 h-4" />, rarity: 4 },
  { id: "deco_collar", name: "デコ（首輪）", icon: <Heart className="w-4 h-4" />, rarity: 3 },
  { id: "deco_badge", name: "デコ（バッジ）", icon: <Gift className="w-4 h-4" />, rarity: 2 },
  { id: "tone", name: "口調", icon: <AudioWaveform className="w-4 h-4" />, rarity: 4 },
  { id: "expression", name: "表情", icon: <Smile className="w-4 h-4" />, rarity: 3 },
  { id: "nickname", name: "呼び名", icon: <Tag className="w-4 h-4" />, rarity: 2 },
];

const mockRecentResults: RecentResult[] = [
  { time: "12:34", name: "緑のずきん", icon: <div className="w-5 h-5 rounded bg-[var(--yuuko-green)]" />, rarity: 4 },
  { time: "12:33", name: "ふんわり（吹き出し）", icon: <div className="w-5 h-5 rounded bg-sky-300" />, rarity: 2 },
  { time: "12:32", name: "ひまわりバッジ", icon: <div className="w-5 h-5 rounded bg-yellow-400" />, rarity: 2 },
  { time: "12:31", name: "やさしい（口調）", icon: <div className="w-5 h-5 rounded bg-pink-300" />, rarity: 3 },
  { time: "12:30", name: "おしえてくれる（性格）", icon: <div className="w-5 h-5 rounded bg-purple-300" />, rarity: 3 },
];

const mockNewItems: NewItem[] = [
  { id: "forest_walk", name: "森のおさんぽ", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-green-400 to-emerald-500" />, isNew: true },
  { id: "star_balloon", name: "キラキラ星ふきだし", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-yellow-300 to-orange-400" />, isNew: false },
  { id: "mushroom_beret", name: "きのこベレー", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-red-400 to-red-600" />, isNew: false },
  { id: "star_collar", name: "星の首輪", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-300 to-blue-500" />, isNew: false },
  { id: "clover_badge", name: "クローバーバッジ", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-green-300 to-green-500" />, isNew: false },
  { id: "tone_item", name: "口調", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-pink-300 to-pink-500" />, isNew: false },
  { id: "expression_item", name: "表情", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-purple-300 to-purple-500" />, isNew: false },
  { id: "yuuko_chan", name: "ゆうこちゃん", icon: <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-amber-200 to-amber-400 flex items-center justify-center text-xs font-bold text-amber-800">ゆうこ</div>, isNew: false },
];

const mockNavigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: false },
  { id: "history", label: "ニュース履歴", icon: Clock, isActive: false },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: false },
  { id: "customize", label: "カスタマイズ", icon: Sparkles, isActive: false },
  { id: "gacha", label: "ガチャ", icon: Dices, isActive: true },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

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

function StarRating({ rating, max = 5 }: { rating: number; max?: number }) {
  return (
    <div className="flex gap-0.5">
      {Array.from({ length: max }).map((_, i) => (
        <Star
          key={i}
          className={`w-3 h-3 ${
            i < rating ? "text-yellow-500 fill-yellow-500" : "text-gray-300"
          }`}
        />
      ))}
    </div>
  );
}

function SidebarNavItem({
  item,
  onNavigate,
}: {
  item: NavigationItem;
  onNavigate: (id: string) => void;
}) {
  const Icon = item.icon;
  return (
    <button
      className={`w-full flex items-center gap-3 px-4 py-2.5 rounded-lg transition-all text-sm ${
        item.isActive
          ? "bg-[var(--yuuko-green-light)] text-[var(--yuuko-green)] font-medium border border-[var(--yuuko-green)]/30"
          : "text-muted-foreground hover:bg-muted hover:text-foreground"
      }`}
      onClick={() => onNavigate(item.id)}
    >
      <Icon
        className={`w-5 h-5 ${item.isActive ? "text-[var(--yuuko-green)]" : ""}`}
      />
      <span>{item.label}</span>
    </button>
  );
}

function GachaMachine() {
  return (
    <div className="relative w-20 h-28">
      {/* Machine body */}
      <div className="absolute bottom-0 w-full h-20 bg-gradient-to-b from-[var(--yuuko-green)] to-emerald-600 rounded-lg shadow-lg">
        {/* Glass dome */}
        <div className="absolute -top-6 left-1/2 -translate-x-1/2 w-14 h-14 bg-gradient-to-br from-white/80 to-white/40 rounded-full border-4 border-[var(--yuuko-green-light)] overflow-hidden">
          {/* Capsules inside */}
          <div className="absolute top-2 left-1 w-3 h-3 rounded-full bg-pink-400 opacity-80" />
          <div className="absolute top-4 right-2 w-2.5 h-2.5 rounded-full bg-blue-400 opacity-80" />
          <div className="absolute bottom-2 left-2 w-2.5 h-2.5 rounded-full bg-yellow-400 opacity-80" />
          <div className="absolute bottom-3 right-1 w-3 h-3 rounded-full bg-green-400 opacity-80" />
          <div className="absolute top-3 left-4 w-2 h-2 rounded-full bg-red-400 opacity-80" />
        </div>
        {/* Dispenser */}
        <div className="absolute bottom-2 left-1/2 -translate-x-1/2 w-8 h-4 bg-[var(--yuuko-cream)] rounded-b-lg" />
        {/* Crank */}
        <div className="absolute right-0 top-1/2 -translate-y-1/2 w-3 h-3 rounded-full bg-red-500 shadow" />
      </div>
    </div>
  );
}

function FloatingCapsule({ color, className }: { color: string; className?: string }) {
  return (
    <div className={`absolute ${className}`}>
      <div className={`w-8 h-8 rounded-full ${color} opacity-60 shadow-lg`} />
    </div>
  );
}

function Sparkle({ className }: { className?: string }) {
  return (
    <div className={`absolute ${className}`}>
      <svg viewBox="0 0 24 24" className="w-4 h-4 text-yellow-400 fill-yellow-400 opacity-60">
        <path d="M12 0L14.59 9.41L24 12L14.59 14.59L12 24L9.41 14.59L0 12L9.41 9.41L12 0Z" />
      </svg>
    </div>
  );
}

function YuukoCharacter() {
  return (
    <div className="relative">
      <img
        src="/assets/yuuko.png"
        alt="ゆうこ"
        className="w-48 h-auto drop-shadow-lg"
        style={{
          animation: "float 3s ease-in-out infinite",
        }}
        onError={(e) => {
          const target = e.target as HTMLImageElement;
          target.style.display = "none";
          const fallback = target.nextElementSibling as HTMLElement;
          if (fallback) fallback.classList.remove("hidden");
        }}
      />
      {/* Fallback placeholder */}
      <div className="hidden w-48 h-52 bg-gradient-to-b from-gray-800 to-gray-900 rounded-3xl flex flex-col items-center justify-center shadow-xl">
        <div className="w-32 h-32 bg-gray-700 rounded-full flex items-center justify-center mb-2">
          <PawIcon className="w-16 h-16 text-pink-300" />
        </div>
        <span className="text-white text-sm font-medium">ゆうこ</span>
      </div>
    </div>
  );
}

// ============================================
// Main Component
// ============================================

export default function GachaScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [gachaState, setGachaState] = React.useState<GachaState>(mockGachaState);
  const [pickup] = React.useState<PickupItem>(mockPickup);
  const [gachaButtons] = React.useState<GachaButtonConfig[]>(mockGachaButtons);
  const [lineup] = React.useState<LineupItem[]>(mockLineup);
  const [recentResults] = React.useState<RecentResult[]>(mockRecentResults);
  const [newItems] = React.useState<NewItem[]>(mockNewItems);

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const handleDrawGacha = (type: string) => {
    console.log(`Draw gacha: ${type}`);
  };

  const handlePurchaseFragments = () => {
    console.log("Purchase fragments clicked");
  };

  const handleViewGachaHistory = () => {
    console.log("View gacha history clicked");
  };

  const handleViewRates = () => {
    console.log("View rates clicked");
  };

  const handleViewRankRewards = () => {
    console.log("View rank rewards clicked");
  };

  const handleViewAllResults = () => {
    console.log("View all results clicked");
  };

  const handleViewAllNewItems = () => {
    console.log("View all new items clicked");
  };

  const progressPercent = (gachaState.currentPoints / gachaState.nextRankPoints) * 100;

  return (
    <div className="h-screen flex flex-col bg-[var(--yuuko-cream)] overflow-hidden">
      {/* Custom CSS for animations */}
      <style jsx global>{`
        @keyframes float {
          0%, 100% { transform: translateY(0); }
          50% { transform: translateY(-8px); }
        }
        @keyframes twinkle {
          0%, 100% { opacity: 0.4; }
          50% { opacity: 1; }
        }
      `}</style>

      {/* Header */}
      <header className="h-10 bg-white border-b border-border flex items-center justify-between px-4 shrink-0">
        <div className="flex items-center gap-2">
          <PawIcon className="w-5 h-5 text-[var(--yuuko-green)]" />
          <span className="text-sm font-medium text-foreground">
            ゆうこと、ニュースを読みやすく。
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button className="p-1.5 hover:bg-muted rounded transition-colors">
            <Minus className="w-4 h-4 text-muted-foreground" />
          </button>
          <button className="p-1.5 hover:bg-muted rounded transition-colors">
            <Square className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
          <button className="p-1.5 hover:bg-red-100 rounded transition-colors">
            <X className="w-4 h-4 text-muted-foreground hover:text-red-500" />
          </button>
        </div>
      </header>

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar */}
        <aside className="w-52 bg-white border-r border-border flex flex-col shrink-0">
          <div className="p-3">
            <Button
              variant="outline"
              className="w-full justify-start gap-2 text-[var(--yuuko-green)] border-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
              onClick={() => handleNavigate("home")}
            >
              <ArrowLeft className="w-4 h-4" />
              ホームへ戻る
            </Button>
          </div>

          <nav className="flex-1 px-3 space-y-1 overflow-y-auto">
            {mockNavigationItems.map((item) => (
              <SidebarNavItem
                key={item.id}
                item={item}
                onNavigate={handleNavigate}
              />
            ))}
          </nav>

          {/* Yuuko's Comment */}
          <div className="p-3">
            <Card className="border-[var(--yuuko-green)]/20 bg-[var(--yuuko-green-light)]/30">
              <CardHeader className="p-3 pb-1">
                <CardTitle className="text-xs font-medium text-[var(--yuuko-green)] flex items-center gap-1">
                  ゆうこのおはなし
                </CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0">
                <p className="text-xs text-foreground leading-relaxed">
                  きらきらのかけら、
                  <br />
                  たまってるよ〜！
                  <br />
                  何が出るかな？
                  <br />
                  わくわくっ♪
                </p>
                <div className="flex justify-end mt-1">
                  <PawIcon className="w-4 h-4 text-pink-300" />
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Auto Start & Exit */}
          <div className="p-3 border-t border-border space-y-2">
            <div className="flex items-center gap-2 text-xs">
              <span className="text-foreground">自動起動：ON</span>
              <span className="w-2 h-2 rounded-full bg-[var(--yuuko-green)]"></span>
            </div>
            <Button
              variant="outline"
              size="sm"
              className="w-full text-xs text-muted-foreground"
            >
              常駐を終了する
            </Button>
          </div>
        </aside>

        {/* Center Content */}
        <main className="flex-1 flex flex-col overflow-hidden p-4">
          {/* Top Bar */}
          <div className="flex items-center justify-between mb-3">
            {/* Breadcrumb */}
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span>ホーム</span>
              <ChevronRight className="w-3 h-3" />
              <span className="text-foreground">ガチャ</span>
            </div>

            {/* Star Fragments & Actions */}
            <div className="flex items-center gap-2">
              <Card className="py-1 px-3 flex items-center gap-2 border-yellow-300 bg-gradient-to-r from-yellow-50 to-orange-50">
                <Star className="w-5 h-5 text-yellow-500 fill-yellow-500" />
                <div className="flex flex-col">
                  <span className="text-[10px] text-muted-foreground">流れ星のかけら</span>
                  <div className="flex items-center gap-1">
                    <span className="text-lg font-bold text-foreground">{gachaState.starFragments.toLocaleString()}</span>
                    <button
                      className="w-5 h-5 rounded-full bg-[var(--yuuko-green)] text-white flex items-center justify-center hover:bg-[var(--yuuko-green)]/90 transition-colors"
                      onClick={handlePurchaseFragments}
                    >
                      <Plus className="w-3 h-3" />
                    </button>
                  </div>
                </div>
              </Card>
              <Button
                variant="outline"
                size="sm"
                className="gap-1 text-xs"
                onClick={handlePurchaseFragments}
              >
                <ShoppingCart className="w-3.5 h-3.5" />
                かけらを購入
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="gap-1 text-xs"
                onClick={handleViewGachaHistory}
              >
                <History className="w-3.5 h-3.5" />
                ガチャ履歴
              </Button>
            </div>
          </div>

          {/* Title */}
          <div className="flex items-center gap-2 mb-1">
            <PawIcon className="w-6 h-6 text-[var(--yuuko-green)]" />
            <h1 className="text-xl font-bold text-foreground">ゆうこガチャ</h1>
          </div>
          <p className="text-sm text-muted-foreground mb-3">
            流れ星のかけらで、ゆうこのデコやテーマを集めよう！
          </p>

          {/* Main Gacha Area */}
          <div className="flex-1 flex gap-4 min-h-0">
            {/* Left: Pickup */}
            <div className="w-36 shrink-0 space-y-3">
              <Card className="border-yellow-400 bg-gradient-to-br from-yellow-50 to-orange-50 overflow-hidden">
                <div className="bg-gradient-to-r from-yellow-400 to-orange-400 text-white text-xs font-bold py-1 px-3 text-center">
                  ピックアップ中！
                </div>
                <CardContent className="p-3">
                  <div className="w-full aspect-square rounded-lg bg-gradient-to-br from-[var(--yuuko-green)] to-emerald-400 mb-2 flex items-center justify-center shadow-inner">
                    <svg viewBox="0 0 24 24" className="w-12 h-12 text-white/90" fill="currentColor">
                      <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z" />
                    </svg>
                  </div>
                  <div className="text-center">
                    <p className="font-bold text-sm text-foreground">{pickup.name}</p>
                    <p className="text-xs text-orange-600 font-medium">{pickup.description}</p>
                    <p className="text-[10px] text-muted-foreground mt-1">{pickup.period}</p>
                  </div>
                </CardContent>
              </Card>
            </div>

            {/* Center: Gacha Animation Area */}
            <Card className="flex-1 overflow-hidden relative">
              <div className="absolute inset-0 bg-gradient-to-b from-[#E8F4EA] to-[#F5EFE0]">
                {/* Background decorations */}
                <div className="absolute inset-0 overflow-hidden">
                  {/* Paw print pattern */}
                  <div className="absolute top-8 left-8 text-[var(--yuuko-green)]/10 text-3xl">
                    <PawIcon className="w-8 h-8" />
                  </div>
                  <div className="absolute top-20 right-12 text-[var(--yuuko-green)]/10 text-2xl">
                    <PawIcon className="w-6 h-6" />
                  </div>
                  <div className="absolute bottom-32 left-16 text-[var(--yuuko-green)]/10 text-xl">
                    <PawIcon className="w-5 h-5" />
                  </div>

                  {/* Room elements */}
                  {/* Bookshelf */}
                  <div className="absolute top-4 right-4 w-16 h-24 bg-amber-700/80 rounded">
                    <div className="absolute top-2 left-1 right-1 h-3 bg-amber-600/50 rounded-sm" />
                    <div className="absolute top-7 left-1 right-1 h-3 bg-amber-600/50 rounded-sm" />
                    <div className="absolute top-12 left-1 right-1 h-3 bg-amber-600/50 rounded-sm" />
                    <div className="absolute top-3 left-2 w-2 h-5 bg-red-400 rounded-sm" />
                    <div className="absolute top-3 left-5 w-2 h-4 bg-blue-400 rounded-sm" />
                    <div className="absolute top-8 left-3 w-2 h-4 bg-green-400 rounded-sm" />
                    <div className="absolute top-8 left-7 w-3 h-3 bg-yellow-400 rounded-sm" />
                  </div>

                  {/* Plant */}
                  <div className="absolute bottom-24 right-8">
                    <div className="w-6 h-10 bg-green-600 rounded-t-full" />
                    <div className="w-5 h-3 bg-amber-700 rounded-b mx-auto" />
                  </div>

                  {/* Green rug */}
                  <div className="absolute bottom-2 left-1/2 -translate-x-1/2 w-64 h-16 bg-[var(--yuuko-green)]/20 rounded-[50%]" />

                  {/* Floating capsules */}
                  <FloatingCapsule color="bg-gradient-to-br from-cyan-300 to-cyan-500" className="top-12 left-1/4 animate-[float_4s_ease-in-out_infinite]" />
                  <FloatingCapsule color="bg-gradient-to-br from-pink-300 to-pink-500" className="top-24 right-1/4 animate-[float_5s_ease-in-out_infinite_0.5s]" />
                  <FloatingCapsule color="bg-gradient-to-br from-green-300 to-green-500" className="top-8 right-1/3 animate-[float_4.5s_ease-in-out_infinite_1s]" />
                  <FloatingCapsule color="bg-gradient-to-br from-yellow-300 to-yellow-500" className="bottom-40 left-1/3 animate-[float_5s_ease-in-out_infinite_0.3s]" />

                  {/* Sparkles */}
                  <Sparkle className="top-16 left-1/3 animate-[twinkle_2s_ease-in-out_infinite]" />
                  <Sparkle className="top-28 right-1/3 animate-[twinkle_2.5s_ease-in-out_infinite_0.5s]" />
                  <Sparkle className="bottom-36 left-1/4 animate-[twinkle_2s_ease-in-out_infinite_1s]" />
                  <Sparkle className="top-10 right-1/4 animate-[twinkle_3s_ease-in-out_infinite_0.2s]" />
                </div>

                {/* Gacha Machine */}
                <div className="absolute bottom-16 left-8">
                  <GachaMachine />
                </div>

                {/* Yuuko Character */}
                <div className="absolute bottom-16 left-1/2 -translate-x-1/2">
                  <YuukoCharacter />
                </div>
              </div>

              {/* Gacha Buttons */}
              <div className="absolute bottom-0 left-0 right-0 p-4 bg-gradient-to-t from-white via-white/95 to-transparent">
                <div className="flex items-center justify-center gap-4">
                  {gachaButtons.map((btn) => (
                    <div key={btn.id} className="relative">
                      {btn.guaranteedText && (
                        <Badge className="absolute -top-3 left-1/2 -translate-x-1/2 bg-red-500 text-white text-[10px] whitespace-nowrap z-10">
                          {btn.guaranteedText}
                        </Badge>
                      )}
                      <Button
                        className={`h-14 px-8 text-base font-bold ${
                          btn.id === "ten"
                            ? "bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white shadow-lg"
                            : "bg-white border-2 border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                        }`}
                        onClick={() => handleDrawGacha(btn.id)}
                      >
                        <span className="flex flex-col items-center">
                          <span>{btn.label}</span>
                          <span className="flex items-center gap-1 text-sm font-normal">
                            <Star className={`w-4 h-4 ${btn.id === "ten" ? "text-yellow-300 fill-yellow-300" : "text-yellow-500 fill-yellow-500"}`} />
                            {btn.cost.toLocaleString()}
                          </span>
                        </span>
                      </Button>
                    </div>
                  ))}
                </div>
                <div className="flex items-center justify-between mt-2 px-4">
                  <div className="text-xs text-muted-foreground">
                    <span className="text-[var(--yuuko-green)]">{gachaState.dailyDiscountText}</span>
                    <br />
                    <span>{gachaState.resetText}</span>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    className="text-xs"
                    onClick={handleViewRates}
                  >
                    提供割合
                  </Button>
                </div>
              </div>
            </Card>

            {/* Right: Lineup & History */}
            <div className="w-56 shrink-0 flex flex-col gap-3">
              {/* Lineup */}
              <Card className="flex-1 overflow-hidden">
                <CardHeader className="p-3 pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-medium">排出ラインナップ</CardTitle>
                    <Button variant="outline" size="sm" className="text-[10px] h-6 px-2">
                      レアリティ
                    </Button>
                  </div>
                </CardHeader>
                <CardContent className="p-3 pt-0 space-y-2 overflow-y-auto max-h-48">
                  {lineup.map((item) => (
                    <div
                      key={item.id}
                      className="flex items-center gap-2 py-1.5 border-b border-border/50 last:border-0"
                    >
                      <div className="w-6 h-6 rounded-full bg-muted flex items-center justify-center text-muted-foreground">
                        {item.icon}
                      </div>
                      <span className="flex-1 text-xs font-medium">{item.name}</span>
                      <StarRating rating={item.rarity} />
                    </div>
                  ))}
                </CardContent>
                <div className="px-3 pb-2 text-[10px] text-muted-foreground">
                  ※各アイテムは重複して獲得することがあります。
                </div>
              </Card>

              {/* Recent Results */}
              <Card>
                <CardHeader className="p-3 pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-medium">最近の獲得履歴</CardTitle>
                    <button
                      className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-0.5"
                      onClick={handleViewAllResults}
                    >
                      すべて見る
                      <ChevronRight className="w-3 h-3" />
                    </button>
                  </div>
                </CardHeader>
                <CardContent className="p-3 pt-0 space-y-1.5">
                  {recentResults.map((result, i) => (
                    <div key={i} className="flex items-center gap-2 text-xs">
                      <span className="text-muted-foreground w-10">{result.time}</span>
                      {result.icon}
                      <span className="flex-1 truncate">{result.name}</span>
                      <StarRating rating={result.rarity} />
                    </div>
                  ))}
                </CardContent>
              </Card>
            </div>
          </div>

          {/* Rank Bonus Card */}
          <Card className="mt-3 bg-gradient-to-r from-[var(--yuuko-green-light)] to-white border-[var(--yuuko-green)]/20">
            <CardContent className="p-3 flex items-center gap-4">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center">
                  <Clover className="w-5 h-5 text-white" />
                </div>
                <div>
                  <div className="flex items-baseline gap-2">
                    <span className="text-xs text-muted-foreground">なかよしランク</span>
                    <span className="text-2xl font-bold text-[var(--yuuko-green)]">{gachaState.friendshipRank}</span>
                  </div>
                  <div className="flex items-center gap-2 text-xs text-muted-foreground">
                    <span>つぎのランクまで</span>
                    <span className="font-medium text-foreground">{gachaState.currentPoints} / {gachaState.nextRankPoints}</span>
                  </div>
                </div>
              </div>
              <div className="flex-1">
                <Progress value={progressPercent} className="h-2 bg-[var(--yuuko-green)]/20" />
              </div>
              <div className="flex items-center gap-3">
                <div className="flex items-center gap-2">
                  <Rainbow className="w-5 h-5 text-pink-400" />
                  <div className="text-xs">
                    <p className="font-medium text-foreground">ランクが上がるとガチャボーナス！</p>
                    <p className="text-muted-foreground">{gachaState.bonusText}</p>
                  </div>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  className="text-xs gap-1"
                  onClick={handleViewRankRewards}
                >
                  <Gift className="w-3.5 h-3.5" />
                  ランク報酬を確認する
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* New Items */}
          <Card className="mt-3">
            <CardHeader className="p-3 pb-2">
              <div className="flex items-center justify-between">
                <CardTitle className="text-sm font-medium flex items-center gap-2">
                  <Sparkles className="w-4 h-4 text-[var(--yuuko-green)]" />
                  新しく解放されたアイテム
                </CardTitle>
                <button
                  className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-0.5"
                  onClick={handleViewAllNewItems}
                >
                  すべて見る
                  <ChevronRight className="w-3 h-3" />
                </button>
              </div>
            </CardHeader>
            <CardContent className="p-3 pt-0">
              <div className="flex gap-2 overflow-x-auto pb-1">
                {newItems.map((item) => (
                  <div
                    key={item.id}
                    className="shrink-0 w-20 p-2 rounded-lg border border-border bg-white hover:shadow-md transition-shadow cursor-pointer relative"
                  >
                    {item.isNew && (
                      <Badge className="absolute -top-1.5 -left-1.5 bg-red-500 text-white text-[8px] px-1.5 py-0">
                        NEW
                      </Badge>
                    )}
                    <div className="flex justify-center mb-1">
                      {item.icon}
                    </div>
                    <p className="text-[10px] text-center text-foreground truncate">{item.name}</p>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>
        </main>
      </div>

      {/* Bottom Status Bar */}
      <footer className="h-8 bg-white border-t border-border flex items-center justify-between px-4 shrink-0">
        <div className="flex items-center gap-2">
          <Bell className="w-3.5 h-3.5 text-muted-foreground" />
          <span className="text-xs text-muted-foreground">お知らせ</span>
          <span className="text-xs text-[var(--yuuko-green)]">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--yuuko-green)] inline-block mr-1" />
            新しいニュースが3件届いてるよ！
          </span>
        </div>
        <div className="flex items-center gap-3">
          <button className="text-muted-foreground hover:text-foreground transition-colors">
            <HelpCircle className="w-4 h-4" />
          </button>
          <div className="w-6 h-6 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center">
            <PawIcon className="w-4 h-4 text-white" />
          </div>
        </div>
      </footer>
    </div>
  );
}
