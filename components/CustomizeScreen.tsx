"use client";

import * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import { Input } from "@/components/ui/input";
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
  Palette,
  AudioWaveform,
  Heart,
  Tag,
  Lock,
  Check,
  RotateCcw,
  Shuffle,
  Eye,
  Pencil,
  Gift,
  Star,
  Moon,
  Rainbow,
  Clover,
} from "lucide-react";
import Image from "next/image";

// Types
type CustomizeTab = "deco" | "balloon" | "theme" | "tone" | "personality" | "name";

interface DecoItem {
  id: string;
  name: string;
  unlocked: boolean;
  equipped: boolean;
  unlockCondition: string | null;
  icon: React.ReactNode;
}

interface CustomizeState {
  activeTab: CustomizeTab;
  theme: string;
  balloonStyle: string;
  tone: string;
  personality: string;
  displayName: string;
  friendshipRank: number;
  currentPoints: number;
  nextRankPoints: number;
}

interface NavigationItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
}

// Mock Data
const mockNavigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: false },
  { id: "history", label: "ニュース履歴", icon: Clock, isActive: false },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: false },
  { id: "customize", label: "カスタマイズ", icon: Sparkles, isActive: true },
  { id: "gacha", label: "ガチャ", icon: Dices, isActive: false },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

const initialCustomizeState: CustomizeState = {
  activeTab: "deco",
  theme: "ナチュラルルーム",
  balloonStyle: "ふんわり",
  tone: "やさしい",
  personality: "おしえてくれる",
  displayName: "ゆうこ",
  friendshipRank: 15,
  currentPoints: 350,
  nextRankPoints: 1000,
};

const mockDecoItems: DecoItem[] = [
  {
    id: "green_hood",
    name: "緑のずきん",
    unlocked: true,
    equipped: true,
    unlockCondition: null,
    icon: (
      <div className="w-10 h-10 rounded-lg bg-[var(--yuuko-green)] flex items-center justify-center">
        <svg viewBox="0 0 24 24" className="w-6 h-6 text-white" fill="currentColor">
          <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z" />
        </svg>
      </div>
    ),
  },
  {
    id: "red_collar",
    name: "赤い首輪",
    unlocked: true,
    equipped: true,
    unlockCondition: null,
    icon: (
      <div className="w-10 h-10 rounded-lg bg-red-500 flex items-center justify-center">
        <svg viewBox="0 0 24 24" className="w-6 h-6 text-white" fill="currentColor">
          <circle cx="12" cy="12" r="8" />
        </svg>
      </div>
    ),
  },
  {
    id: "sunflower_badge",
    name: "ひまわりバッジ",
    unlocked: false,
    equipped: false,
    unlockCondition: "ランク20で解放",
    icon: (
      <div className="w-10 h-10 rounded-lg bg-yellow-400 flex items-center justify-center opacity-50">
        <svg viewBox="0 0 24 24" className="w-6 h-6 text-yellow-700" fill="currentColor">
          <circle cx="12" cy="12" r="4" />
          <path d="M12 2v4M12 18v4M2 12h4M18 12h4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83" stroke="currentColor" strokeWidth="2" fill="none" />
        </svg>
      </div>
    ),
  },
  {
    id: "blue_muffler",
    name: "青いマフラー",
    unlocked: false,
    equipped: false,
    unlockCondition: "ランク30で解放",
    icon: (
      <div className="w-10 h-10 rounded-lg bg-blue-400 flex items-center justify-center opacity-50">
        <svg viewBox="0 0 24 24" className="w-6 h-6 text-white" fill="currentColor">
          <rect x="4" y="8" width="16" height="8" rx="2" />
        </svg>
      </div>
    ),
  },
  {
    id: "sparkle_crown",
    name: "きらきら王冠",
    unlocked: false,
    equipped: false,
    unlockCondition: "ランク20で解放",
    icon: (
      <div className="w-10 h-10 rounded-lg bg-yellow-500 flex items-center justify-center opacity-50">
        <svg viewBox="0 0 24 24" className="w-6 h-6 text-yellow-200" fill="currentColor">
          <path d="M5 16L3 5l5.5 5L12 4l3.5 6L21 5l-2 11H5z" />
        </svg>
      </div>
    ),
  },
];

const mockUnlockedItems = [
  { id: "green_hood", name: "緑のずきん", color: "bg-[var(--yuuko-green)]" },
  { id: "red_collar", name: "赤い首輪", color: "bg-red-500" },
  { id: "green_rug", name: "緑のラグ", color: "bg-emerald-400" },
  { id: "plant", name: "観葉植物", color: "bg-green-600" },
  { id: "frame", name: "額縁", color: "bg-amber-600" },
  { id: "balloon", name: "ふんわり吹き出し", color: "bg-sky-300" },
  { id: "note", name: "音符", color: "bg-pink-400" },
  { id: "heart", name: "ハート", color: "bg-rose-400" },
];

const mockRankRewardItems = [
  { id: "clover", name: "クローバー", icon: Clover, color: "bg-emerald-100" },
  { id: "star", name: "星", icon: Star, color: "bg-yellow-100" },
  { id: "rainbow", name: "虹", icon: Rainbow, color: "bg-gradient-to-r from-red-100 via-yellow-100 to-blue-100" },
  { id: "moon", name: "月", icon: Moon, color: "bg-indigo-100" },
];

const tabItems = [
  { id: "deco" as CustomizeTab, label: "デコ", icon: Sparkles },
  { id: "balloon" as CustomizeTab, label: "吹き出し", icon: MessageCircle },
  { id: "theme" as CustomizeTab, label: "テーマ", icon: Palette },
  { id: "tone" as CustomizeTab, label: "口調", icon: AudioWaveform },
  { id: "personality" as CustomizeTab, label: "性格", icon: Heart },
  { id: "name" as CustomizeTab, label: "呼び名", icon: Tag },
];

// Sub-components
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

function DecoItemCard({
  item,
  isSelected,
  onSelect,
}: {
  item: DecoItem;
  isSelected: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      className={`w-full flex items-center gap-3 p-3 rounded-xl transition-all ${
        isSelected
          ? "bg-[var(--yuuko-green-light)] border-2 border-[var(--yuuko-green)]"
          : item.unlocked
            ? "bg-white border border-border hover:border-[var(--yuuko-green)]/50"
            : "bg-muted/50 border border-border/50 opacity-70"
      }`}
      onClick={() => item.unlocked && onSelect(item.id)}
      disabled={!item.unlocked}
    >
      {item.icon}
      <div className="flex-1 text-left">
        <div className="flex items-center gap-2">
          <span className={`font-medium text-sm ${!item.unlocked ? "text-muted-foreground" : ""}`}>
            {item.name}
          </span>
          {item.equipped && (
            <Badge className="bg-[var(--yuuko-green)] text-white text-[10px] px-1.5 py-0">
              装着中
            </Badge>
          )}
        </div>
        {item.unlockCondition && (
          <div className="flex items-center gap-1 text-xs text-muted-foreground mt-0.5">
            <Lock className="w-3 h-3" />
            <span>{item.unlockCondition}</span>
          </div>
        )}
      </div>
      {!item.unlocked && <Lock className="w-4 h-4 text-muted-foreground" />}
    </button>
  );
}

export default function CustomizeScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [customizeState, setCustomizeState] = React.useState<CustomizeState>(initialCustomizeState);
  const [selectedDecoId, setSelectedDecoId] = React.useState<string>("green_hood");
  const [displayName, setDisplayName] = React.useState(initialCustomizeState.displayName);

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const handleTabChange = (tab: CustomizeTab) => {
    setCustomizeState((prev) => ({ ...prev, activeTab: tab }));
  };

  const handleSave = () => {
    console.log("Save customization:", { ...customizeState, displayName });
  };

  const handleReset = () => {
    console.log("Reset to default");
    setCustomizeState(initialCustomizeState);
    setDisplayName(initialCustomizeState.displayName);
    setSelectedDecoId("green_hood");
  };

  const handleRandomize = () => {
    console.log("Randomize outfit");
  };

  const handlePreview = () => {
    console.log("Preview changes");
  };

  const handleViewAllDeco = () => {
    console.log("View all deco items");
  };

  const handleCheckRankRewards = () => {
    console.log("Check rank rewards");
  };

  const progressPercent = (customizeState.currentPoints / customizeState.nextRankPoints) * 100;

  return (
    <div className="h-screen flex flex-col bg-[var(--yuuko-cream)] overflow-hidden">
      {/* Header */}
      <header className="h-10 bg-white border-b border-border flex items-center justify-between px-4 shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-6 h-6 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center">
            <span className="text-white text-xs font-bold">ゆ</span>
          </div>
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
                  <MessageCircle className="w-3 h-3" />
                  ゆうこの一言
                </CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0">
                <p className="text-xs text-foreground leading-relaxed">
                  自分らしく
                  <br />
                  コーディネートするのって、
                  <br />
                  とっても楽しいよね♪
                  <br />
                  いっしょに考えよ〜！
                </p>
                <div className="flex justify-end mt-1">
                  <span className="text-[var(--yuuko-green)]">🐾</span>
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
        <main className="flex-1 flex flex-col overflow-hidden">
          <div className="flex-1 p-4 overflow-y-auto">
            {/* Breadcrumb */}
            <div className="flex items-center gap-2 text-xs text-muted-foreground mb-3">
              <span>ホーム</span>
              <ChevronRight className="w-3 h-3" />
              <span className="text-foreground">カスタマイズ</span>
            </div>

            {/* Title */}
            <div className="flex items-center gap-2 mb-1">
              <Sparkles className="w-6 h-6 text-[var(--yuuko-green)]" />
              <h1 className="text-xl font-bold text-foreground">ゆうこカスタマイズ</h1>
            </div>
            <p className="text-sm text-muted-foreground mb-4">
              ゆうこの見た目や話し方、テーマを設定して、もっと仲良くなろう！
            </p>

            {/* Tabs */}
            <div className="flex gap-1 mb-4">
              {tabItems.map((tab) => {
                const Icon = tab.icon;
                const isActive = customizeState.activeTab === tab.id;
                return (
                  <button
                    key={tab.id}
                    className={`flex items-center gap-1.5 px-4 py-2 rounded-lg text-sm font-medium transition-all ${
                      isActive
                        ? "bg-[var(--yuuko-green)] text-white"
                        : "bg-white text-muted-foreground hover:bg-muted border border-border"
                    }`}
                    onClick={() => handleTabChange(tab.id)}
                  >
                    <Icon className="w-4 h-4" />
                    {tab.label}
                  </button>
                );
              })}
            </div>

            {/* Main Customize Area */}
            <div className="flex gap-4">
              {/* Deco Items List */}
              <Card className="w-64 shrink-0">
                <CardHeader className="p-3 pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-medium">デコアイテム</CardTitle>
                    <div className="flex items-center gap-1 text-xs text-muted-foreground">
                      <span>並び替え：</span>
                      <select className="bg-transparent border-none text-xs focus:outline-none cursor-pointer">
                        <option>新しい順</option>
                      </select>
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="p-3 pt-0 space-y-2">
                  {mockDecoItems.map((item) => (
                    <DecoItemCard
                      key={item.id}
                      item={item}
                      isSelected={selectedDecoId === item.id}
                      onSelect={setSelectedDecoId}
                    />
                  ))}
                  <Button
                    variant="outline"
                    className="w-full text-sm mt-2"
                    onClick={handleViewAllDeco}
                  >
                    すべてのデコを見る
                    <ChevronRight className="w-4 h-4 ml-1" />
                  </Button>
                </CardContent>
              </Card>

              {/* Yuuko Preview */}
              <Card className="flex-1 overflow-hidden">
                <div className="relative h-80 bg-gradient-to-b from-[#E8F4EA] to-[#F5EFE0]">
                  {/* Room Background Elements */}
                  <div className="absolute inset-0 overflow-hidden">
                    {/* Window */}
                    <div className="absolute top-4 right-8 w-16 h-20 bg-sky-200 rounded-lg border-4 border-amber-100 shadow-inner">
                      <div className="absolute inset-2 bg-sky-300/50 rounded" />
                    </div>
                    {/* Plant */}
                    <div className="absolute bottom-16 left-4">
                      <div className="w-8 h-12 bg-green-600 rounded-t-full" />
                      <div className="w-6 h-4 bg-amber-700 rounded-b mx-auto" />
                    </div>
                    {/* Shelf */}
                    <div className="absolute top-8 right-28 w-20 h-3 bg-amber-600 rounded" />
                    <div className="absolute top-4 right-30 w-6 h-8 bg-emerald-500 rounded" />
                    {/* Rug */}
                    <div className="absolute bottom-4 left-1/2 -translate-x-1/2 w-40 h-12 bg-[var(--yuuko-green)]/30 rounded-full" />
                    {/* Paw prints pattern */}
                    <div className="absolute top-12 left-12 text-[var(--yuuko-green)]/10 text-2xl">🐾</div>
                    <div className="absolute bottom-20 right-16 text-[var(--yuuko-green)]/10 text-xl">🐾</div>
                  </div>

                  {/* Preview Button */}
                  <Button
                    variant="outline"
                    size="sm"
                    className="absolute top-3 right-3 text-xs bg-white/80 hover:bg-white"
                    onClick={handlePreview}
                  >
                    <Eye className="w-3 h-3 mr-1" />
                    プレビュー
                  </Button>

                  {/* Yuuko Character */}
                  <div className="absolute bottom-8 left-1/2 -translate-x-1/2">
                    <Image
                      src="/assets/yuuko.png"
                      alt="ゆうこ"
                      width={180}
                      height={200}
                      className="drop-shadow-lg animate-[float_3s_ease-in-out_infinite]"
                      style={{
                        animation: "float 3s ease-in-out infinite",
                      }}
                    />
                  </div>
                </div>

                {/* Action Buttons */}
                <div className="flex items-center justify-center gap-3 p-4 bg-white border-t border-border">
                  <Button variant="outline" onClick={handleReset}>
                    <RotateCcw className="w-4 h-4 mr-2" />
                    元に戻す
                  </Button>
                  <Button
                    className="bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white px-8"
                    onClick={handleSave}
                  >
                    <Check className="w-4 h-4 mr-2" />
                    保存する
                  </Button>
                  <Button variant="outline" onClick={handleRandomize}>
                    <Shuffle className="w-4 h-4 mr-2" />
                    ランダムに着せる
                  </Button>
                </div>
              </Card>
            </div>

            {/* Bottom Section */}
            <div className="flex gap-4 mt-4">
              {/* Unlocked Items */}
              <Card className="flex-1">
                <CardHeader className="p-3 pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-medium flex items-center gap-2">
                      <Gift className="w-4 h-4 text-[var(--yuuko-green)]" />
                      解放済みアイテム
                    </CardTitle>
                    <Button variant="link" size="sm" className="text-xs text-[var(--yuuko-green)] p-0 h-auto">
                      すべて見る
                      <ChevronRight className="w-3 h-3 ml-0.5" />
                    </Button>
                  </div>
                </CardHeader>
                <CardContent className="p-3 pt-0">
                  <div className="flex gap-2 overflow-x-auto pb-1">
                    {mockUnlockedItems.map((item) => (
                      <div key={item.id} className="relative shrink-0">
                        <div
                          className={`w-12 h-12 rounded-lg ${item.color} flex items-center justify-center`}
                        >
                          <span className="text-white text-xs font-medium">
                            {item.name.slice(0, 1)}
                          </span>
                        </div>
                        <div className="absolute -bottom-1 -right-1 w-4 h-4 bg-[var(--yuuko-green)] rounded-full flex items-center justify-center">
                          <Check className="w-2.5 h-2.5 text-white" />
                        </div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>

              {/* Rank Rewards */}
              <Card className="w-72 shrink-0">
                <CardHeader className="p-3 pb-2">
                  <CardTitle className="text-sm font-medium">ランク報酬で解放</CardTitle>
                </CardHeader>
                <CardContent className="p-3 pt-0">
                  <div className="flex gap-2 mb-2">
                    {mockRankRewardItems.map((item) => {
                      const Icon = item.icon;
                      return (
                        <div key={item.id} className="relative">
                          <div
                            className={`w-12 h-12 rounded-lg ${item.color} flex items-center justify-center border border-border`}
                          >
                            <Icon className="w-6 h-6 text-muted-foreground" />
                          </div>
                          <div className="absolute -bottom-1 -right-1 w-4 h-4 bg-muted rounded-full flex items-center justify-center">
                            <Lock className="w-2.5 h-2.5 text-muted-foreground" />
                          </div>
                        </div>
                      );
                    })}
                  </div>
                  <p className="text-xs text-muted-foreground">
                    ランクごとに特別なアイテムが解放されるよ！
                  </p>
                </CardContent>
              </Card>
            </div>
          </div>

          {/* Footer Status Bar */}
          <footer className="h-8 bg-white border-t border-border flex items-center justify-between px-4 shrink-0">
            <div className="flex items-center gap-3">
              <div className="flex items-center gap-1.5">
                <Bell className="w-3.5 h-3.5 text-muted-foreground" />
                <span className="text-xs text-muted-foreground">お知らせ</span>
              </div>
              <div className="flex items-center gap-1.5">
                <span className="w-1.5 h-1.5 rounded-full bg-[var(--yuuko-green)]"></span>
                <span className="text-xs text-foreground">新しいニュースが3件届いてるよ！</span>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <button className="p-1 hover:bg-muted rounded transition-colors">
                <HelpCircle className="w-4 h-4 text-muted-foreground" />
              </button>
              <div className="w-5 h-5 rounded-full bg-[var(--yuuko-green-light)] flex items-center justify-center">
                <span className="text-[10px]">🐾</span>
              </div>
            </div>
          </footer>
        </main>

        {/* Right Sidebar */}
        <aside className="w-64 bg-white border-l border-border flex flex-col shrink-0 overflow-y-auto">
          <div className="p-3 space-y-3">
            {/* Current Settings */}
            <Card className="border-border">
              <CardHeader className="p-3 pb-2">
                <CardTitle className="text-sm font-medium">現在の設定</CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0 space-y-2">
                <div className="flex items-center gap-2 text-xs">
                  <Palette className="w-3.5 h-3.5 text-[var(--yuuko-green)]" />
                  <span className="text-muted-foreground w-12">テーマ</span>
                  <span className="text-foreground">{customizeState.theme}</span>
                </div>
                <div className="flex items-center gap-2 text-xs">
                  <MessageCircle className="w-3.5 h-3.5 text-[var(--yuuko-green)]" />
                  <span className="text-muted-foreground w-12">吹き出し</span>
                  <span className="text-foreground">{customizeState.balloonStyle}</span>
                </div>
                <div className="flex items-center gap-2 text-xs">
                  <AudioWaveform className="w-3.5 h-3.5 text-[var(--yuuko-green)]" />
                  <span className="text-muted-foreground w-12">口調</span>
                  <span className="text-foreground">{customizeState.tone}</span>
                </div>
                <div className="flex items-center gap-2 text-xs">
                  <Heart className="w-3.5 h-3.5 text-[var(--yuuko-green)]" />
                  <span className="text-muted-foreground w-12">性格</span>
                  <span className="text-foreground">{customizeState.personality}</span>
                </div>
                <div className="flex items-center gap-2 text-xs">
                  <Tag className="w-3.5 h-3.5 text-[var(--yuuko-green)]" />
                  <span className="text-muted-foreground w-12">呼び名</span>
                  <span className="text-foreground">{customizeState.displayName}</span>
                </div>
              </CardContent>
            </Card>

            {/* Name Setting */}
            <Card className="border-border">
              <CardHeader className="p-3 pb-2">
                <CardTitle className="text-sm font-medium">呼び名の設定</CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0">
                <p className="text-xs text-muted-foreground mb-2">
                  あなたがゆうこを呼ぶときの名前だよ。
                </p>
                <div className="relative">
                  <Input
                    value={displayName}
                    onChange={(e) => setDisplayName(e.target.value.slice(0, 10))}
                    className="pr-8 text-sm"
                    maxLength={10}
                  />
                  <Pencil className="absolute right-2 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
                </div>
                <p className="text-[10px] text-muted-foreground mt-1">最大10文字まで</p>
              </CardContent>
            </Card>

            {/* Yuuko's Comment */}
            <Card className="border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/50">
              <CardHeader className="p-3 pb-1">
                <CardTitle className="text-xs font-medium text-[var(--yuuko-green)] flex items-center gap-1">
                  <Heart className="w-3 h-3 fill-[var(--yuuko-green)]" />
                  ゆうこのおはなし
                </CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0">
                <p className="text-xs text-foreground leading-relaxed">
                  わぁ〜！このコーデ、
                  <br />
                  とっても似合ってる〜♪
                  <br />
                  ありがとう！うれしいよ〜！
                </p>
                <div className="flex justify-end mt-1">
                  <span className="text-[var(--yuuko-green)]">🐾</span>
                </div>
              </CardContent>
            </Card>

            {/* Friendship Rank */}
            <Card className="border-border">
              <CardHeader className="p-3 pb-2">
                <CardTitle className="text-sm font-medium flex items-center gap-1">
                  <Sparkles className="w-4 h-4 text-yellow-500" />
                  なかよしランク
                </CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0">
                <div className="flex items-baseline gap-1 mb-1">
                  <span className="text-xs text-muted-foreground">ランク</span>
                  <span className="text-3xl font-bold text-[var(--yuuko-green)]">
                    {customizeState.friendshipRank}
                  </span>
                </div>
                <div className="text-[10px] text-muted-foreground mb-1">
                  つぎのランクまで {customizeState.currentPoints} / {customizeState.nextRankPoints}
                </div>
                <Progress value={progressPercent} className="h-2 mb-3" />
                <Button
                  variant="outline"
                  size="sm"
                  className="w-full text-xs"
                  onClick={handleCheckRankRewards}
                >
                  <Gift className="w-3 h-3 mr-1" />
                  ランク報酬を確認する
                </Button>
              </CardContent>
            </Card>
          </div>
        </aside>
      </div>

      {/* Float animation keyframes */}
      <style jsx global>{`
        @keyframes float {
          0%, 100% { transform: translateY(0) translateX(-50%); }
          50% { transform: translateY(-8px) translateX(-50%); }
        }
      `}</style>
    </div>
  );
}
