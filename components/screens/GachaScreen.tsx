"use client";

/**
 * ガチャ画面（画面詳細設計書 §13.2）。
 * 所持かけら・1回引く・結果モーダル・コレクション一覧を表示する。
 * 抽選・かけらの消費・保存はすべて Rust 側（get_gacha_state / draw_gacha_once / mark_gacha_items_seen）が行い、
 * この画面は結果を受け取って表示するだけ。10連・レアリティ・提供割合・購入導線は置かない（D72）。
 */

import * as React from "react";
import Image from "next/image";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import { AutostartStatus } from "@/components/layout/AutostartStatus";
import { QuitResidentButton } from "@/components/layout/QuitResidentButton";
import {
  drawGachaOnce,
  getGachaState,
  markGachaItemsSeen,
  type GachaDrawResult,
  type GachaItemKind,
  type GachaState,
} from "@/lib/tauri/gacha";
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
  ChevronRight,
  MessageCircle,
  MessageSquare,
  Star,
  Palette,
  Gift,
} from "lucide-react";

// ============================================
// TypeScript Types
// ============================================

interface NavigationItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive: boolean;
}

/** 結果モーダル・コレクション詳細で見せる 1 件（獲得済みのものだけ）。 */
interface DisplayItem {
  itemId: string;
  kind: GachaItemKind;
  name: string;
  text: string | null;
}

/** loading: 読み込み中 / ready: 表示可能 / unavailable: Tauri 外 / error: 読み込み失敗。 */
type LoadStatus = "loading" | "ready" | "unavailable" | "error";

// ============================================
// Constants
// ============================================

const navigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home, isActive: false },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: false },
  { id: "history", label: "ニュース履歴", icon: Clock, isActive: false },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen, isActive: false },
  { id: "customize", label: "カスタマイズ", icon: Sparkles, isActive: false },
  { id: "gacha", label: "ガチャ", icon: Dices, isActive: true },
  { id: "settings", label: "設定", icon: Settings, isActive: false },
];

const KIND_LABELS: Record<GachaItemKind, string> = {
  card: "カード",
  theme: "テーマ",
  deco: "飾り",
  balloon: "吹き出し",
};

const KIND_ICONS: Record<GachaItemKind, React.ComponentType<{ className?: string }>> = {
  card: MessageCircle,
  theme: Palette,
  deco: Gift,
  balloon: MessageSquare,
};

// 生のエラー文・内部パスは画面へ出さず、固定文言だけを見せる。
const UNAVAILABLE_MESSAGE = "ガチャはアプリ内でのみ使えます。";
const LOAD_ERROR_MESSAGE =
  "ガチャの情報を読み込めなかったよ。少し時間を置いてから、もう一度試してみてね。";
const DRAW_ERROR_MESSAGE =
  "ガチャをまわせなかったよ。少し時間を置いてから、もう一度試してみてね。";

/** 引けない理由（かけら不足 / コンプリート）。引けるときは null。 */
function drawBlockedReason(state: GachaState): string | null {
  if (state.canDraw) {
    return null;
  }
  if (state.isComplete) {
    return "コンプリート！ぜんぶ集めたよ。ありがとう！";
  }
  if (state.starFragments < state.cost) {
    const shortage = state.cost - state.starFragments;
    return `かけらが足りないよ（あと ${shortage.toLocaleString()} 個）。ニュースを読んだり、用語を調べたりすると集まるよ。`;
  }
  return "いまはガチャをまわせないよ。";
}

/**
 * 抽選結果を手元の状態へ反映する。保存済みの正は Rust 側にあり、
 * 直後の mark_gacha_items_seen の戻り値（最新状態）で上書きされる。
 */
function applyDrawResult(prev: GachaState, result: GachaDrawResult): GachaState {
  const drawn = result.status === "drawn" ? result.item : null;
  let ownedCount = prev.ownedCount;
  const items = prev.items.map((item) => {
    if (!drawn || item.itemId !== drawn.itemId || item.owned) {
      return item;
    }
    ownedCount += 1;
    return { ...item, owned: true, isNew: true, name: drawn.name, text: drawn.text };
  });
  return {
    ...prev,
    items,
    ownedCount,
    starFragments: result.starFragments,
    cost: result.cost,
    isComplete: result.isComplete,
    canDraw: !result.isComplete && result.starFragments >= result.cost,
  };
}

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

function GachaMachine() {
  return (
    <div className="relative w-24 h-36" aria-hidden="true">
      {/* Machine body */}
      <div className="absolute bottom-0 w-full h-24 bg-gradient-to-b from-[var(--yuuko-green)] to-emerald-600 rounded-xl shadow-lg">
        {/* Glass dome */}
        <div className="absolute -top-8 left-1/2 -translate-x-1/2 w-18 h-18 bg-gradient-to-br from-white/80 to-white/40 rounded-full border-4 border-[var(--yuuko-green-light)] overflow-hidden" style={{ width: '4.5rem', height: '4.5rem' }}>
          {/* Capsules inside */}
          <div className="absolute top-2 left-2 w-4 h-4 rounded-full bg-pink-400 opacity-80" />
          <div className="absolute top-5 right-2 w-3 h-3 rounded-full bg-blue-400 opacity-80" />
          <div className="absolute bottom-3 left-3 w-3 h-3 rounded-full bg-yellow-400 opacity-80" />
          <div className="absolute bottom-4 right-3 w-4 h-4 rounded-full bg-green-400 opacity-80" />
          <div className="absolute top-4 left-5 w-2.5 h-2.5 rounded-full bg-red-400 opacity-80" />
        </div>
        {/* Dispenser */}
        <div className="absolute bottom-3 left-1/2 -translate-x-1/2 w-10 h-5 bg-[var(--yuuko-cream)] rounded-b-lg" />
        {/* Crank */}
        <div className="absolute -right-2 top-1/2 -translate-y-1/2 w-4 h-4 rounded-full bg-red-500 shadow-md" />
      </div>
    </div>
  );
}

function FloatingCapsule({ color, className, size = "md" }: { color: string; className?: string; size?: "sm" | "md" | "lg" }) {
  const sizes = {
    sm: "w-8 h-8",
    md: "w-12 h-12",
    lg: "w-16 h-16"
  };
  return (
    <div className={`absolute ${className}`} aria-hidden="true">
      <div className={`${sizes[size]} rounded-full ${color} opacity-50 shadow-lg`} />
    </div>
  );
}

function Sparkle({ className }: { className?: string }) {
  return (
    <div className={`absolute ${className}`} aria-hidden="true">
      <svg viewBox="0 0 24 24" className="w-5 h-5 text-yellow-400 fill-yellow-400 opacity-60">
        <path d="M12 0L14.59 9.41L24 12L14.59 14.59L12 24L9.41 14.59L0 12L9.41 9.41L12 0Z" />
      </svg>
    </div>
  );
}

function YuukoCharacter() {
  return (
    <div className="relative">
      <Image
        src="/assets/yuuko.png"
        width={963}
        height={1174}
        alt="ゆうこ"
        priority
        className="w-auto h-[clamp(180px,34vh,380px)] drop-shadow-lg"
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
      <div className="hidden w-52 h-64 bg-gradient-to-b from-gray-800 to-gray-900 rounded-3xl flex flex-col items-center justify-center shadow-xl">
        <div className="w-28 h-28 bg-gray-700 rounded-full flex items-center justify-center mb-2">
          <PawIcon className="w-16 h-16 text-pink-300" />
        </div>
        <span className="text-white text-base font-medium">ゆうこ</span>
      </div>
    </div>
  );
}

/**
 * ガチャ結果・コレクションの 1 件を見せるモーダル。
 * 文面は外部由来ではないが、HTML としては扱わずプレーンテキストで表示する。
 */
function GachaItemDialog({
  item,
  heading,
  onClose,
}: {
  item: DisplayItem | null;
  heading: string;
  onClose: () => void;
}) {
  const KindIcon = item ? KIND_ICONS[item.kind] : Sparkles;
  return (
    <Dialog
      open={item !== null}
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
    >
      <DialogContent className="sm:max-w-sm" showCloseButton={false}>
        {item && (
          <>
            <DialogHeader>
              <div className="mx-auto mb-2 flex h-12 w-12 items-center justify-center rounded-full bg-[var(--yuuko-green-light)]">
                <KindIcon className="h-6 w-6 text-[var(--yuuko-green)]" />
              </div>
              <p className="text-center text-xs text-muted-foreground">{heading}</p>
              <DialogTitle className="text-center">{item.name}</DialogTitle>
              <DialogDescription className="text-center">
                種類：{KIND_LABELS[item.kind]}
              </DialogDescription>
            </DialogHeader>
            {item.text && (
              <p
                className="rounded-lg bg-[var(--yuuko-cream)] p-3 text-sm leading-relaxed whitespace-pre-wrap break-words"
                data-testid="gacha-item-text"
              >
                {item.text}
              </p>
            )}
            {item.kind === "theme" && (
              <p className="text-center text-xs text-muted-foreground">
                カスタマイズ画面で切り替えられるよ
              </p>
            )}
            <DialogFooter className="sm:justify-center">
              <Button
                className="bg-[var(--yuuko-green)] text-white hover:bg-[var(--yuuko-green)]/90"
                onClick={onClose}
              >
                とじる
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
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
  const [loadStatus, setLoadStatus] = React.useState<LoadStatus>("loading");
  const [gacha, setGacha] = React.useState<GachaState | null>(null);
  const [isDrawing, setIsDrawing] = React.useState(false);
  const [drawError, setDrawError] = React.useState<string | null>(null);
  const [dialogItem, setDialogItem] = React.useState<DisplayItem | null>(null);
  const [dialogHeading, setDialogHeading] = React.useState("");
  // state 更新は非同期なので、連打による二重抽選は ref で確実に止める。
  const drawingRef = React.useRef(false);
  // 抽選の世代。抽選の開始・反映で進め、それより前に出した NEW 解除の応答で抽選後の状態を上書きしないようにする。
  const drawGenerationRef = React.useRef(0);
  const isMountedRef = React.useRef(true);

  React.useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const loadState = React.useCallback(async () => {
    setLoadStatus("loading");
    try {
      const state = await getGachaState();
      if (!isMountedRef.current) {
        return;
      }
      if (!state) {
        setLoadStatus("unavailable");
        return;
      }
      setGacha(state);
      setLoadStatus("ready");
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      // console.error だと開発時のエラー表示に生のエラー文が出るため warn で調査用に残す。
      console.warn("Failed to load gacha state:", error);
      setLoadStatus("error");
    }
  }, []);

  React.useEffect(() => {
    void loadState();
  }, [loadState]);

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  // 確認したものの「NEW」を外す。失敗しても NEW が残るだけなので画面には出さない。
  const markSeen = React.useCallback(async (itemId: string) => {
    const generation = drawGenerationRef.current;
    try {
      const state = await markGachaItemsSeen([itemId]);
      if (state && isMountedRef.current && generation === drawGenerationRef.current) {
        setGacha(state);
      }
    } catch (error) {
      console.warn("Failed to mark gacha item as seen:", error);
    }
  }, []);

  const handleDraw = async () => {
    if (drawingRef.current || !gacha?.canDraw) {
      return;
    }
    drawingRef.current = true;
    drawGenerationRef.current += 1;
    setIsDrawing(true);
    setDrawError(null);
    try {
      const result = await drawGachaOnce();
      if (!isMountedRef.current) {
        return;
      }
      if (!result) {
        setLoadStatus("unavailable");
        return;
      }
      // insufficient / complete は何も消費しないので、状態だけ合わせて理由表示に任せる。
      drawGenerationRef.current += 1;
      setGacha((prev) => (prev ? applyDrawResult(prev, result) : prev));
      if (result.status === "drawn" && result.item) {
        setDialogHeading("ガチャの結果");
        setDialogItem({
          itemId: result.item.itemId,
          kind: result.item.kind,
          name: result.item.name,
          text: result.item.text,
        });
      }
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.warn("Failed to draw gacha:", error);
      setDrawError(DRAW_ERROR_MESSAGE);
    } finally {
      drawingRef.current = false;
      if (isMountedRef.current) {
        setIsDrawing(false);
      }
    }
  };

  // 結果モーダル・コレクション詳細のどちらも、閉じたときに NEW を外す（確認した扱い）。
  const handleCloseDialog = () => {
    const closing = dialogItem;
    setDialogItem(null);
    if (!closing) {
      return;
    }
    const current = gacha?.items.find((item) => item.itemId === closing.itemId);
    if (current?.isNew) {
      void markSeen(closing.itemId);
    }
  };

  const handleOpenCollectionItem = (item: DisplayItem) => {
    setDialogHeading("コレクション");
    setDialogItem(item);
  };

  const isReady = loadStatus === "ready" && gacha !== null;
  const blockedReason = isReady ? drawBlockedReason(gacha) : null;
  const canPressDraw = isReady && gacha.canDraw && !isDrawing;
  const remainingCount = isReady ? gacha.totalCount - gacha.ownedCount : 0;

  // 実行ボタン下の案内。読み込み失敗・Tauri 外・抽選失敗・引けない理由の順に 1 つだけ出す。
  const statusMessage =
    loadStatus === "unavailable"
      ? UNAVAILABLE_MESSAGE
      : loadStatus === "error"
        ? LOAD_ERROR_MESSAGE
        : (drawError ?? blockedReason);

  return (
    <div className="h-dvh flex flex-col bg-[var(--yuuko-cream)] overflow-hidden">
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

      <AppTitleBar />

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
                  きらきらのかけらで、
                  <br />
                  なにが出るかな？
                  <br />
                  わくわくっ♪
                </p>
                <div className="flex justify-end mt-1">
                  <PawIcon className="w-4 h-4 text-pink-300" aria-hidden="true" />
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Auto Start & Exit */}
          <div className="p-3 border-t border-border space-y-2">
            <AutostartStatus />
            <QuitResidentButton className="w-full text-xs text-muted-foreground" />
          </div>
        </aside>

        {/* Center Content */}
        <main className="flex-1 min-w-0 flex flex-col overflow-hidden">
          {/* Top Bar */}
          <div className="flex items-center justify-between p-4 pb-2 shrink-0">
            {/* Breadcrumb */}
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              {/* パンくず「ホーム」。button にしてクリックとキーボード（Tab→Enter/Space）の両方でホームへ戻れるようにする。 */}
              <button
                type="button"
                className="rounded-sm transition-colors hover:text-[var(--yuuko-green)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--yuuko-green)]"
                onClick={() => handleNavigate("home")}
              >
                ホーム
              </button>
              <ChevronRight className="w-3 h-3" aria-hidden="true" />
              <span className="text-foreground">ガチャ</span>
            </div>

            {/* Star Fragments（Tauri 外・読み込み前は数値を出さない） */}
            <Card className="py-2 px-4 flex items-center gap-3 border-yellow-300 bg-gradient-to-r from-yellow-50 to-orange-50">
              <Star className="w-6 h-6 text-yellow-500 fill-yellow-500" aria-hidden="true" />
              <div className="flex flex-col">
                <span className="text-[10px] text-muted-foreground">流れ星のかけら</span>
                <span
                  className="text-xl font-bold text-foreground"
                  data-testid="gacha-star-fragments"
                >
                  {isReady ? gacha.starFragments.toLocaleString() : "—"}
                </span>
              </div>
            </Card>
          </div>

          {/* Title */}
          <div className="px-4 shrink-0">
            <div className="flex items-center gap-2 mb-1">
              <PawIcon className="w-7 h-7 text-[var(--yuuko-green)]" aria-hidden="true" />
              <h1 className="text-2xl font-bold text-foreground">ゆうこガチャ</h1>
            </div>
            <p className="text-sm text-muted-foreground mb-2">
              流れ星のかけらで、ゆうこのひとことカードやテーマを集めよう！
            </p>
          </div>

          {/* Main Gacha Area - Grid Layout */}
          <div className="flex-1 min-h-0 px-4 pb-4 overflow-x-auto">
            <div
              className="flex flex-col lg:grid lg:grid-cols-[minmax(420px,1fr)_280px] gap-4 h-full lg:min-w-[720px]"
            >
              {/* Center: Gacha Animation Area */}
              <Card className="overflow-hidden relative h-full min-h-[420px]">
                <div className="absolute inset-0 bg-gradient-to-b from-[#E8F4EA] to-[#F5EFE0]">
                  {/* Background decorations */}
                  <div className="absolute inset-0 overflow-hidden" aria-hidden="true">
                    {/* Paw print pattern */}
                    <div className="absolute top-8 left-12 text-[var(--yuuko-green)]/10">
                      <PawIcon className="w-10 h-10" />
                    </div>
                    <div className="absolute top-24 right-16 text-[var(--yuuko-green)]/10">
                      <PawIcon className="w-8 h-8" />
                    </div>
                    <div className="absolute bottom-40 left-20 text-[var(--yuuko-green)]/10">
                      <PawIcon className="w-6 h-6" />
                    </div>

                    {/* Room elements - Bookshelf */}
                    <div className="absolute top-4 right-6 w-20 h-28 bg-amber-700/80 rounded">
                      <div className="absolute top-3 left-1.5 right-1.5 h-4 bg-amber-600/50 rounded-sm" />
                      <div className="absolute top-10 left-1.5 right-1.5 h-4 bg-amber-600/50 rounded-sm" />
                      <div className="absolute top-[68px] left-1.5 right-1.5 h-4 bg-amber-600/50 rounded-sm" />
                      <div className="absolute top-4 left-3 w-2.5 h-5 bg-red-400 rounded-sm" />
                      <div className="absolute top-4 left-7 w-2.5 h-4 bg-blue-400 rounded-sm" />
                      <div className="absolute top-11 left-4 w-2.5 h-4 bg-green-400 rounded-sm" />
                      <div className="absolute top-11 left-9 w-3 h-3 bg-yellow-400 rounded-sm" />
                    </div>

                    {/* Plant */}
                    <div className="absolute bottom-28 right-12">
                      <div className="w-8 h-14 bg-green-600 rounded-t-full" />
                      <div className="w-6 h-4 bg-amber-700 rounded-b mx-auto" />
                    </div>

                    {/* Green rug */}
                    <div className="absolute bottom-4 left-1/2 -translate-x-1/2 w-80 h-20 bg-[var(--yuuko-green)]/20 rounded-[50%]" />

                    {/* Floating capsules */}
                    <FloatingCapsule color="bg-gradient-to-br from-cyan-300 to-cyan-500" className="top-16 left-1/4 animate-[float_4s_ease-in-out_infinite]" size="lg" />
                    <FloatingCapsule color="bg-gradient-to-br from-pink-300 to-pink-500" className="top-32 right-1/4 animate-[float_5s_ease-in-out_infinite_0.5s]" size="lg" />
                    <FloatingCapsule color="bg-gradient-to-br from-green-300 to-green-500" className="top-12 right-1/3 animate-[float_4.5s_ease-in-out_infinite_1s]" size="md" />
                    <FloatingCapsule color="bg-gradient-to-br from-yellow-300 to-yellow-500" className="bottom-48 left-1/3 animate-[float_5s_ease-in-out_infinite_0.3s]" size="md" />

                    {/* Sparkles */}
                    <Sparkle className="top-20 left-1/3 animate-[twinkle_2s_ease-in-out_infinite]" />
                    <Sparkle className="top-36 right-1/3 animate-[twinkle_2.5s_ease-in-out_infinite_0.5s]" />
                    <Sparkle className="bottom-44 left-1/4 animate-[twinkle_2s_ease-in-out_infinite_1s]" />
                    <Sparkle className="top-14 right-1/4 animate-[twinkle_3s_ease-in-out_infinite_0.2s]" />
                  </div>

                  {/* Gacha Machine */}
                  <div className="absolute bottom-24 left-12">
                    <GachaMachine />
                  </div>

                  {/* Yuuko Character */}
                  <div className="absolute bottom-20 left-1/2 -translate-x-1/2">
                    <YuukoCharacter />
                  </div>
                </div>

                {/* Gacha Button（1回引く・1個もらえる。かけら不足・コンプリート・抽選中は押せない） */}
                <div className="absolute bottom-0 left-0 right-0 p-3 bg-gradient-to-t from-white via-white/95 to-transparent">
                  <div className="flex items-center justify-center">
                    <Button
                      className="h-16 px-10 text-lg font-bold rounded-xl bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white shadow-lg"
                      onClick={() => void handleDraw()}
                      disabled={!canPressDraw}
                      aria-busy={isDrawing}
                    >
                      <span className="flex flex-col items-center gap-0.5">
                        <span>{isDrawing ? "まわしています…" : "1回まわす"}</span>
                        {isReady && (
                          <span className="flex items-center gap-1 text-sm font-medium">
                            <Star className="w-4 h-4 text-yellow-300 fill-yellow-300" aria-hidden="true" />
                            {gacha.cost.toLocaleString()}
                          </span>
                        )}
                      </span>
                    </Button>
                  </div>
                  <p
                    role="status"
                    className="mt-2 min-h-4 text-center text-xs text-muted-foreground"
                    data-testid="gacha-status-message"
                  >
                    {statusMessage}
                  </p>
                </div>
              </Card>

              {/* Right: Collection（D80。未所持は「？」、残り数を出す） */}
              <Card className="flex flex-col overflow-hidden h-full min-h-[240px]">
                <CardHeader className="p-3 pb-2 shrink-0">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-medium">コレクション</CardTitle>
                    {isReady && (
                      <span
                        className="text-xs text-muted-foreground"
                        data-testid="gacha-collection-count"
                      >
                        {gacha.ownedCount} / {gacha.totalCount}
                      </span>
                    )}
                  </div>
                  {isReady && (
                    <p
                      className="text-xs text-[var(--yuuko-green)] font-medium"
                      data-testid="gacha-collection-remaining"
                    >
                      {gacha.isComplete ? "コンプリート！" : `のこり ${remainingCount}`}
                    </p>
                  )}
                </CardHeader>
                <CardContent className="p-3 pt-0 flex-1 overflow-y-auto">
                  {loadStatus === "loading" && (
                    <p className="text-xs text-muted-foreground">読み込み中…</p>
                  )}
                  {loadStatus === "unavailable" && (
                    <p className="text-xs text-muted-foreground">{UNAVAILABLE_MESSAGE}</p>
                  )}
                  {loadStatus === "error" && (
                    <div className="space-y-2">
                      <p className="text-xs text-muted-foreground">{LOAD_ERROR_MESSAGE}</p>
                      <Button
                        variant="outline"
                        size="sm"
                        className="text-xs"
                        onClick={() => void loadState()}
                      >
                        もう一度読み込む
                      </Button>
                    </div>
                  )}
                  {isReady && (
                    <ul className="grid grid-cols-3 gap-2" aria-label="コレクション一覧">
                      {gacha.items.map((item) => {
                        if (!item.owned || item.name === null) {
                          return (
                            <li
                              key={item.itemId}
                              className="aspect-square rounded-lg border border-dashed border-border bg-muted/40 flex items-center justify-center text-xl font-bold text-muted-foreground"
                              data-testid="gacha-collection-unowned"
                            >
                              <span aria-hidden="true">？</span>
                              <span className="sr-only">まだ持っていないもの</span>
                            </li>
                          );
                        }
                        const KindIcon = KIND_ICONS[item.kind];
                        const name = item.name;
                        return (
                          <li key={item.itemId} data-testid="gacha-collection-owned">
                            <button
                              type="button"
                              className="relative w-full aspect-square rounded-lg border border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/40 p-1 flex flex-col items-center justify-center gap-1 hover:bg-[var(--yuuko-green-light)] transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--yuuko-green)]"
                              aria-label={`${name}（${KIND_LABELS[item.kind]}）${item.isNew ? " NEW" : ""}`}
                              onClick={() => {
                                handleOpenCollectionItem({
                                  itemId: item.itemId,
                                  kind: item.kind,
                                  name,
                                  text: item.text,
                                });
                              }}
                            >
                              {item.isNew && (
                                <Badge className="absolute -top-1.5 -right-1.5 bg-red-500 text-white text-[9px] px-1 py-0">
                                  NEW
                                </Badge>
                              )}
                              <KindIcon className="w-4 h-4 text-[var(--yuuko-green)]" aria-hidden="true" />
                              <span className="w-full truncate text-center text-[10px] font-medium">
                                {name}
                              </span>
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  )}
                </CardContent>
              </Card>
            </div>
          </div>
        </main>
      </div>

      {/* Bottom Status Bar */}
      <footer className="h-8 bg-white border-t border-border flex items-center justify-between px-4 shrink-0">
        <div className="flex items-center gap-2">
          <Bell className="w-3.5 h-3.5 text-muted-foreground" aria-hidden="true" />
          <span className="text-xs text-muted-foreground">お知らせ</span>
          <span className="text-xs text-[var(--yuuko-green)]">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--yuuko-green)] inline-block mr-1" aria-hidden="true" />
            {/* 以前は全画面共通の固定文言（3件届いてるよ）だった。実データと食い違うため、取得済みのコレクション状況を出す。 */}
            {loadStatus === "loading"
              ? "読み込み中..."
              : gacha
                ? `コレクション ${gacha.ownedCount} / ${gacha.totalCount}`
                : "ガチャ"}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <button
            className="text-muted-foreground hover:text-foreground transition-colors"
            aria-label="ヘルプ"
          >
            <HelpCircle className="w-4 h-4" />
          </button>
          <div className="w-6 h-6 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center" aria-hidden="true">
            <PawIcon className="w-4 h-4 text-white" />
          </div>
        </div>
      </footer>

      <GachaItemDialog item={dialogItem} heading={dialogHeading} onClose={handleCloseDialog} />
    </div>
  );
}
