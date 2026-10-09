"use client";

/**
 * カスタマイズ画面（画面詳細設計書 §13.1・判断台帳 D83）。
 * - テーマ: 既定・ランク報酬テーマ・ガチャの色違いテーマを並べ、選べるものだけ切り替える。
 *   選べるかの判定と保存は Rust の set_active_theme が行い、ここは結果（activeThemeId）を表示・適用するだけ。
 * - 呼び名: 現在の値を読み取り専用で出し、変更は設定画面へ案内する。
 * - 飾り・吹き出し: 「準備中」と表示する（素材ができてから追加。D75）。口調・性格は扱わない（D83）。
 */

import * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import { AutostartStatus } from "@/components/layout/AutostartStatus";
import { QuitResidentButton } from "@/components/layout/QuitResidentButton";
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
  Palette,
  Heart,
  Tag,
  Lock,
  Check,
  Gift,
  Star,
  Moon,
  Rainbow,
  Clover,
} from "lucide-react";
import Image from "next/image";
import { getUserSettings, isTauriRuntime } from "@/lib/tauri/settings";
import { getFriendshipState, type FriendshipState } from "@/lib/tauri/yuuko";
import { getRewardState, setActiveTheme, type RewardItem } from "@/lib/tauri/rewards";
import { getGachaState, type GachaCollectionItem } from "@/lib/tauri/gacha";
import { applyUiTheme } from "@/hooks/use-ui-theme";
import { buildThemeOptions } from "@/lib/customize-themes.mjs";

// Types
type CustomizeTab = "theme" | "name" | "deco" | "balloon";

/**
 * 友情ランク・報酬などの読み込み状態。
 * preview はブラウザ確認（Tauri 外）で実データが無いとき。サンプル値をプレビューとして明示して出す。
 */
type RankLoadStatus = "loading" | "ready" | "preview" | "error";

/** ランク表示に使う値（get_friendship_state の DTO の一部）。 */
type FriendshipView = Pick<FriendshipState, "currentRank" | "currentPoint" | "nextRequiredPoint">;

/** テーマ一覧の 1 件（lib/customize-themes.mjs の buildThemeOptions の戻り値）。 */
type ThemeOption = ReturnType<typeof buildThemeOptions>[number];

/** テーマ切り替えの結果表示（固定文言のみ。生のエラー文は出さない）。 */
type ThemeMessage = { kind: "success" | "error"; text: string } | null;

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

// Tauri 外のプレビュー専用のサンプル値。実データと誤解されないよう「サンプル」表示と組で使う。
const previewFriendship: FriendshipView = {
  currentRank: 15,
  currentPoint: 350,
  nextRequiredPoint: 1000,
};

const mockRankRewardItems = [
  { id: "clover", name: "クローバー", icon: Clover, color: "bg-emerald-100" },
  { id: "star", name: "星", icon: Star, color: "bg-yellow-100" },
  { id: "rainbow", name: "虹", icon: Rainbow, color: "bg-gradient-to-r from-red-100 via-yellow-100 to-blue-100" },
  { id: "moon", name: "月", icon: Moon, color: "bg-indigo-100" },
];

// カテゴリ（D83）: テーマ・呼び名は使える。飾り・吹き出しは「準備中」。口調・性格は置かない。
const tabItems = [
  { id: "theme" as CustomizeTab, label: "テーマ", icon: Palette },
  { id: "name" as CustomizeTab, label: "呼び名", icon: Tag },
  { id: "deco" as CustomizeTab, label: "飾り", icon: Sparkles },
  { id: "balloon" as CustomizeTab, label: "吹き出し", icon: MessageCircle },
];

// Sub-components

/**
 * テーマの色見本。配色を持つテーマは `data-theme` 付きの要素の中で CSS 変数を読み、そのテーマの色を出す
 * （app/globals.css の `[data-theme="<id>"]`）。配色が未登録のテーマは、適用中の色と誤解されないよう点線の枠だけ出す。
 */
function ThemeSwatch({ option }: { option: ThemeOption }) {
  if (!option.hasPalette) {
    return (
      <span
        className="w-10 h-10 rounded-lg border border-dashed border-border bg-muted/50 flex items-center justify-center shrink-0"
        aria-hidden="true"
      >
        <Palette className="w-4 h-4 text-muted-foreground" />
      </span>
    );
  }
  return (
    <span
      data-theme={option.id}
      className={`w-10 h-10 rounded-lg border border-border overflow-hidden flex shrink-0 ${
        option.selectable ? "" : "opacity-50"
      }`}
      aria-hidden="true"
    >
      <span className="flex-1 bg-[var(--yuuko-cream)]" />
      <span className="flex-1 bg-[var(--yuuko-green-light)]" />
      <span className="flex-1 bg-[var(--yuuko-green)]" />
    </span>
  );
}

/** テーマ 1 件の選択ボタン。ロック中は押せず、解放条件を出す。 */
function ThemeOptionButton({
  option,
  showSelected,
  isSaving,
  disabled,
  onSelect,
}: {
  option: ThemeOption;
  showSelected: boolean;
  isSaving: boolean;
  disabled: boolean;
  onSelect: (option: ThemeOption) => void;
}) {
  const isSelected = showSelected && option.selected;
  return (
    <button
      type="button"
      className={`w-full flex items-center gap-3 p-3 rounded-xl text-left transition-all ${
        isSelected
          ? "bg-[var(--yuuko-green-light)] border-2 border-[var(--yuuko-green)]"
          : option.selectable
            ? "bg-white border border-border hover:border-[var(--yuuko-green)]/50"
            : "bg-muted/50 border border-border/50 opacity-70"
      }`}
      onClick={() => onSelect(option)}
      disabled={!option.selectable || disabled}
      aria-pressed={isSelected}
      data-testid="customize-theme-option"
    >
      <ThemeSwatch option={option} />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span
            className={`font-medium text-sm truncate ${option.selectable ? "" : "text-muted-foreground"}`}
          >
            {option.label}
          </span>
          {isSelected && (
            <Badge className="bg-[var(--yuuko-green)] text-white text-[10px] px-1.5 py-0 shrink-0">
              使用中
            </Badge>
          )}
        </div>
        {option.lockText !== null && (
          <div className="flex items-center gap-1 text-xs text-muted-foreground mt-0.5">
            <Lock className="w-3 h-3 shrink-0" aria-hidden="true" />
            <span>{option.lockText}</span>
          </div>
        )}
        {option.selectable && !option.hasPalette && (
          <div className="text-[10px] text-muted-foreground mt-0.5">配色はじゅんび中だよ</div>
        )}
        {isSaving && <div className="text-[10px] text-muted-foreground mt-0.5">保存中…</div>}
      </div>
      {!option.selectable && <Lock className="w-4 h-4 text-muted-foreground shrink-0" aria-hidden="true" />}
    </button>
  );
}

/** 飾り・吹き出しの「準備中」表示（D83 / D75）。 */
function ComingSoonPanel({ title }: { title: string }) {
  return (
    <div className="space-y-2" data-testid="customize-coming-soon">
      <div className="flex items-center gap-2">
        <span className="text-sm font-medium">{title}</span>
        <Badge variant="outline" className="text-[10px] px-1.5 py-0">
          準備中
        </Badge>
      </div>
      <p className="text-xs text-muted-foreground leading-relaxed">
        いま準備中だよ。できあがったら、ここで選べるようになるよ。
      </p>
    </div>
  );
}

export default function CustomizeScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [activeTab, setActiveTab] = React.useState<CustomizeTab>("theme");
  const [friendshipStatus, setFriendshipStatus] = React.useState<RankLoadStatus>("loading");
  const [friendship, setFriendship] = React.useState<FriendshipView | null>(null);
  const [rewardStatus, setRewardStatus] = React.useState<RankLoadStatus>("loading");
  const [rewards, setRewards] = React.useState<RewardItem[]>([]);
  const [activeThemeId, setActiveThemeId] = React.useState<string | null>(null);
  const [gachaStatus, setGachaStatus] = React.useState<RankLoadStatus>("loading");
  const [gachaItems, setGachaItems] = React.useState<GachaCollectionItem[]>([]);
  const [nicknameStatus, setNicknameStatus] = React.useState<RankLoadStatus>("loading");
  const [nickname, setNickname] = React.useState("");
  const [savingThemeId, setSavingThemeId] = React.useState<string | null>(null);
  const [themeMessage, setThemeMessage] = React.useState<ThemeMessage>(null);

  // 友情ランク・報酬・ガチャ所持・呼び名は Rust（get_friendship_state / get_reward_state /
  // get_gacha_state / get_user_settings）が正。ここでは表示するだけ。
  // どれかの取得に失敗しても、ほかの表示と画面全体は続ける。
  React.useEffect(() => {
    if (!isTauriRuntime()) {
      setFriendship(previewFriendship);
      setFriendshipStatus("preview");
      setRewardStatus("preview");
      setGachaStatus("preview");
      setNicknameStatus("preview");
      return;
    }

    let cancelled = false;

    getFriendshipState()
      .then((state) => {
        if (cancelled) return;
        if (!state) {
          setFriendshipStatus("error");
          return;
        }
        setFriendship(state);
        setFriendshipStatus("ready");
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        // 生のエラー文は画面へ出さず、調査用に warn で残す。
        console.warn("Failed to load friendship state:", error);
        setFriendshipStatus("error");
      });

    getRewardState()
      .then((state) => {
        if (cancelled) return;
        if (!state) {
          setRewardStatus("error");
          return;
        }
        setRewards(state.rewards);
        setActiveThemeId(state.activeThemeId);
        setRewardStatus("ready");
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        console.warn("Failed to load reward state:", error);
        setRewardStatus("error");
      });

    getGachaState()
      .then((state) => {
        if (cancelled) return;
        if (!state) {
          setGachaStatus("error");
          return;
        }
        setGachaItems(state.items);
        setGachaStatus("ready");
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        console.warn("Failed to load gacha state:", error);
        setGachaStatus("error");
      });

    getUserSettings()
      .then((settings) => {
        if (cancelled) return;
        if (!settings) {
          setNicknameStatus("error");
          return;
        }
        setNickname(settings.nickname);
        setNicknameStatus("ready");
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        console.warn("Failed to load user settings:", error);
        setNicknameStatus("error");
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const themeOptions = React.useMemo(
    () =>
      buildThemeOptions({
        rewards: rewardStatus === "ready" ? rewards : [],
        gachaItems: gachaStatus === "ready" ? gachaItems : [],
        activeThemeId,
      }),
    [rewardStatus, rewards, gachaStatus, gachaItems, activeThemeId]
  );
  // 適用中の表示は、Rust の判定済み activeThemeId が読めたときだけ出す（読めないのに既定を「使用中」と見せない）。
  const canShowActiveTheme = rewardStatus === "ready";
  const activeThemeOption = canShowActiveTheme
    ? themeOptions.find((option) => option.selected) ?? null
    : null;
  const unlockedThemeOptions = themeOptions.filter(
    (option) => option.source !== "default" && option.selectable
  );

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const handleTabChange = (tab: CustomizeTab) => {
    setActiveTab(tab);
  };

  // テーマは選んだ時点で保存する。選べるかは Rust が判定し、選べない ID は reject される（表示側のロックは補助）。
  // 保存できたら Rust が返した activeThemeId をすぐ画面へ適用する（再起動後は useUiTheme が設定から適用する）。
  const handleSelectTheme = (option: ThemeOption) => {
    if (!option.selectable || savingThemeId !== null || !canShowActiveTheme) return;
    if (option.selected) return;
    setSavingThemeId(option.id);
    setThemeMessage(null);
    setActiveTheme(option.id)
      .then((state) => {
        if (!state) {
          setThemeMessage({ kind: "error", text: "テーマを保存できなかったよ。もう一度ためしてね。" });
          return;
        }
        setRewards(state.rewards);
        setActiveThemeId(state.activeThemeId);
        applyUiTheme(state.activeThemeId);
        setThemeMessage({ kind: "success", text: `テーマを「${option.label}」にしたよ。` });
      })
      .catch((error: unknown) => {
        console.warn("Failed to save active theme:", error);
        setThemeMessage({ kind: "error", text: "テーマを保存できなかったよ。もう一度ためしてね。" });
      })
      .finally(() => {
        setSavingThemeId(null);
      });
  };

  // 上限ランクでは nextRequiredPoint が 0 になるため、進捗は満タン扱いにする（0 除算を避ける）。
  const isMaxRank = friendship !== null && friendship.nextRequiredPoint <= 0;
  const progressPercent =
    friendship === null
      ? 0
      : isMaxRank
        ? 100
        : Math.min(100, Math.max(0, (friendship.currentPoint / friendship.nextRequiredPoint) * 100));

  return (
    <div className="h-dvh flex flex-col bg-[var(--yuuko-cream)] overflow-hidden">
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

          {/* Yuuko's Comment */}
          <div className="p-3">
            <Card className="border-[var(--yuuko-green)]/20 bg-[var(--yuuko-green-light)]/30">
              <CardHeader className="p-3 pb-1">
                <CardTitle className="text-xs font-medium text-[var(--yuuko-green)] flex items-center gap-1">
                  <MessageCircle className="w-3 h-3" aria-hidden="true" />
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
                  <span className="text-[var(--yuuko-green)]" aria-hidden="true">🐾</span>
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
          <div className="flex-1 p-4 overflow-y-auto">
            {/* Breadcrumb */}
            <div className="flex items-center gap-2 text-xs text-muted-foreground mb-3">
              {/* パンくず「ホーム」。button にしてクリックとキーボード（Tab→Enter/Space）の両方でホームへ戻れるようにする。 */}
              <button
                type="button"
                className="rounded-sm transition-colors hover:text-[var(--yuuko-green)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--yuuko-green)]"
                onClick={() => handleNavigate("home")}
              >
                ホーム
              </button>
              <ChevronRight className="w-3 h-3" aria-hidden="true" />
              <span className="text-foreground">カスタマイズ</span>
            </div>

            {/* Title */}
            <div className="flex items-center gap-2 mb-1">
              <Sparkles className="w-6 h-6 text-[var(--yuuko-green)]" aria-hidden="true" />
              <h1 className="text-xl font-bold text-foreground">ゆうこカスタマイズ</h1>
            </div>
            <p className="text-sm text-muted-foreground mb-4">
              テーマや呼び名を設定して、もっと仲良くなろう！
            </p>

            {/* Tabs */}
            <div className="flex gap-1 mb-4 overflow-x-auto pb-1" role="tablist">
              {tabItems.map((tab) => {
                const Icon = tab.icon;
                const isActive = activeTab === tab.id;
                return (
                  <button
                    key={tab.id}
                    role="tab"
                    aria-selected={isActive}
                    className={`flex items-center gap-1.5 px-4 py-2 rounded-lg text-sm font-medium transition-all shrink-0 ${
                      isActive
                        ? "bg-[var(--yuuko-green)] text-white"
                        : "bg-white text-muted-foreground hover:bg-muted border border-border"
                    }`}
                    onClick={() => handleTabChange(tab.id)}
                  >
                    <Icon className="w-4 h-4" aria-hidden="true" />
                    {tab.label}
                  </button>
                );
              })}
            </div>

            {/* Main Customize Area（カテゴリの中身＋プレビューの2列）: 既定ウィンドウ（800×600）では横に並ばないため、プレビューを下へ折り返す */}
            <div className="flex flex-wrap gap-4">
              {/* Category Panel */}
              <Card className="w-64 shrink-0" role="tabpanel" aria-label={tabItems.find((tab) => tab.id === activeTab)?.label}>
                <CardHeader className="p-3 pb-2">
                  <CardTitle className="text-sm font-medium">
                    {tabItems.find((tab) => tab.id === activeTab)?.label}
                  </CardTitle>
                </CardHeader>
                <CardContent className="p-3 pt-0 space-y-2">
                  {activeTab === "theme" && (
                    <>
                      {themeOptions.map((option) => (
                        <ThemeOptionButton
                          key={option.id}
                          option={option}
                          showSelected={canShowActiveTheme}
                          isSaving={savingThemeId === option.id}
                          disabled={savingThemeId !== null || !canShowActiveTheme}
                          onSelect={handleSelectTheme}
                        />
                      ))}
                      {rewardStatus === "loading" && (
                        <p className="text-xs text-muted-foreground">読み込み中…</p>
                      )}
                      {rewardStatus === "error" && (
                        <p className="text-xs text-muted-foreground" data-testid="customize-theme-load-error">
                          テーマの状態を読み込めなかったよ。
                        </p>
                      )}
                      {gachaStatus === "error" && (
                        <p className="text-xs text-muted-foreground" data-testid="customize-gacha-theme-load-error">
                          ガチャのテーマを読み込めなかったよ。
                        </p>
                      )}
                      {rewardStatus === "preview" && (
                        <p className="text-xs text-muted-foreground">
                          プレビューではテーマを切り替えられないよ。アプリで選んでね。
                        </p>
                      )}
                      {themeMessage !== null && (
                        <p
                          role="status"
                          className={`text-xs ${
                            themeMessage.kind === "error" ? "text-destructive" : "text-[var(--yuuko-green)]"
                          }`}
                          data-testid="customize-theme-message"
                        >
                          {themeMessage.text}
                        </p>
                      )}
                    </>
                  )}
                  {activeTab === "name" && (
                    <div className="space-y-2">
                      <p className="text-xs text-muted-foreground">ゆうこがあなたを呼ぶときの名前だよ。</p>
                      <div className="rounded-lg border border-border bg-white px-3 py-2 text-sm break-all" data-testid="customize-nickname">
                        {nicknameStatus === "ready"
                          ? nickname.trim() !== ""
                            ? nickname
                            : "まだ決めていないよ"
                          : nicknameStatus === "error"
                            ? "呼び名を読み込めなかったよ。"
                            : nicknameStatus === "preview"
                              ? "アプリで設定した呼び名が表示されるよ。"
                              : "読み込み中…"}
                      </div>
                      <Button
                        variant="outline"
                        className="w-full text-sm"
                        onClick={() => handleNavigate("settings")}
                      >
                        設定で変える
                        <ChevronRight className="w-4 h-4 ml-1" aria-hidden="true" />
                      </Button>
                    </div>
                  )}
                  {activeTab === "deco" && <ComingSoonPanel title="飾り" />}
                  {activeTab === "balloon" && <ComingSoonPanel title="吹き出し" />}
                </CardContent>
              </Card>

              {/* Yuuko Preview */}
              <Card className="flex-1 min-w-[280px] overflow-hidden">
                <div className="relative h-80 bg-gradient-to-b from-[#E8F4EA] to-[#F5EFE0]">
                  {/* Room Background Elements */}
                  <div className="absolute inset-0 overflow-hidden" aria-hidden="true">
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

                  {/* Yuuko Character */}
                  <div className="absolute bottom-8 left-1/2 -translate-x-1/2">
                    <Image
                      src="/assets/yuuko.png"
                      alt="ゆうこ"
                      width={963}
                      height={1174}
                      className="w-[180px] h-auto drop-shadow-lg animate-[float_3s_ease-in-out_infinite]"
                      style={{
                        animation: "float 3s ease-in-out infinite",
                      }}
                    />
                  </div>
                </div>

                {/* Save Note: テーマは選んだ時点で保存するため、別の保存ボタンは置かない */}
                <div className="flex flex-wrap items-center justify-center gap-2 p-4 bg-white border-t border-border text-xs text-muted-foreground">
                  <Check className="w-4 h-4 text-[var(--yuuko-green)]" aria-hidden="true" />
                  <span>テーマは選ぶとすぐに保存されるよ。</span>
                </div>
              </Card>
            </div>

            {/* Bottom Section: 上段と同じく、幅が足りないときはランク報酬を下へ折り返す */}
            <div className="flex flex-wrap gap-4 mt-4">
              {/* Unlocked Items: 解放済みの報酬テーマと所持済みのガチャテーマ（実データ） */}
              <Card className="flex-1 min-w-[280px]">
                <CardHeader className="p-3 pb-2">
                  <CardTitle className="text-sm font-medium flex items-center gap-2">
                    <Gift className="w-4 h-4 text-[var(--yuuko-green)]" aria-hidden="true" />
                    解放済みアイテム
                  </CardTitle>
                </CardHeader>
                <CardContent className="p-3 pt-0">
                  {unlockedThemeOptions.length > 0 ? (
                    <ul className="flex gap-3 overflow-x-auto pb-1">
                      {unlockedThemeOptions.map((option) => (
                        <li
                          key={option.id}
                          className="flex flex-col items-center gap-1 shrink-0 w-14"
                          data-testid="customize-unlocked-item"
                        >
                          <ThemeSwatch option={option} />
                          <span className="text-[10px] text-foreground w-full text-center truncate">
                            {option.label}
                          </span>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <p className="text-xs text-muted-foreground">
                      {rewardStatus === "loading" || gachaStatus === "loading"
                        ? "読み込み中…"
                        : "まだ解放したアイテムはないよ。"}
                    </p>
                  )}
                </CardContent>
              </Card>

              {/* Rank Rewards */}
              <Card className="w-72 shrink-0">
                <CardHeader className="p-3 pb-2">
                  <CardTitle className="text-sm font-medium flex items-center gap-1">
                    ランク報酬で解放
                    {rewardStatus === "preview" && (
                      <Badge variant="outline" className="ml-auto text-[10px] px-1.5 py-0">
                        サンプル
                      </Badge>
                    )}
                  </CardTitle>
                </CardHeader>
                <CardContent className="p-3 pt-0">
                  {rewardStatus === "ready" ? (
                    // 実データ: 報酬マスタ全件の解放状態を読み取り専用で出す（ここでは適用・保存しない）。
                    rewards.length > 0 ? (
                      <ul className="space-y-1.5 mb-2">
                        {rewards.map((reward) => (
                          <li
                            key={reward.rewardId}
                            className="flex items-center gap-2 text-xs"
                            data-testid="customize-rank-reward-item"
                          >
                            {reward.unlocked ? (
                              <Check className="w-3.5 h-3.5 text-[var(--yuuko-green)] shrink-0" aria-hidden="true" />
                            ) : (
                              <Lock className="w-3.5 h-3.5 text-muted-foreground shrink-0" aria-hidden="true" />
                            )}
                            <span
                              className={`flex-1 min-w-0 truncate ${
                                reward.unlocked ? "text-foreground" : "text-muted-foreground"
                              }`}
                            >
                              {reward.name}
                            </span>
                            {reward.unlocked ? (
                              <Badge className="bg-[var(--yuuko-green)] text-white text-[10px] px-1.5 py-0 shrink-0">
                                解放済み
                              </Badge>
                            ) : (
                              <span className="text-[10px] text-muted-foreground shrink-0">
                                ランク{reward.unlockRank}で解放
                              </span>
                            )}
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <p className="text-xs text-muted-foreground mb-2">まだ報酬はないよ。</p>
                    )
                  ) : rewardStatus === "loading" ? (
                    <p className="text-xs text-muted-foreground mb-2">読み込み中…</p>
                  ) : rewardStatus === "error" ? (
                    <p className="text-xs text-muted-foreground mb-2">報酬の状態を読み込めなかったよ。</p>
                  ) : (
                  // Tauri 外のプレビュー: 従来のサンプル表示（すべて未解放）をそのまま出す。
                  <div className="flex gap-2 mb-2" data-testid="customize-rank-reward-preview">
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
                  )}
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
            {/* Current Settings（実データ。口調・性格は扱わないため出さない。D83） */}
            <Card className="border-border">
              <CardHeader className="p-3 pb-2">
                <CardTitle className="text-sm font-medium">現在の設定</CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0 space-y-2">
                <div className="flex items-center gap-2 text-xs">
                  <Palette className="w-3.5 h-3.5 text-[var(--yuuko-green)] shrink-0" aria-hidden="true" />
                  <span className="text-muted-foreground w-12 shrink-0">テーマ</span>
                  <span className="text-foreground min-w-0 truncate" data-testid="customize-current-theme">
                    {activeThemeOption !== null
                      ? activeThemeOption.label
                      : rewardStatus === "loading"
                        ? "読み込み中…"
                        : "—"}
                  </span>
                </div>
                <div className="flex items-center gap-2 text-xs">
                  <MessageCircle className="w-3.5 h-3.5 text-[var(--yuuko-green)] shrink-0" aria-hidden="true" />
                  <span className="text-muted-foreground w-12 shrink-0">吹き出し</span>
                  <span className="text-muted-foreground">準備中</span>
                </div>
                <div className="flex items-center gap-2 text-xs">
                  <Tag className="w-3.5 h-3.5 text-[var(--yuuko-green)] shrink-0" aria-hidden="true" />
                  <span className="text-muted-foreground w-12 shrink-0">呼び名</span>
                  <span className="text-foreground min-w-0 truncate">
                    {nicknameStatus === "ready"
                      ? nickname.trim() !== ""
                        ? nickname
                        : "未設定"
                      : "—"}
                  </span>
                </div>
              </CardContent>
            </Card>

            {/* Yuuko's Comment */}
            <Card className="border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/50">
              <CardHeader className="p-3 pb-1">
                <CardTitle className="text-xs font-medium text-[var(--yuuko-green)] flex items-center gap-1">
                  <Heart className="w-3 h-3 fill-[var(--yuuko-green)]" aria-hidden="true" />
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
                  <span className="text-[var(--yuuko-green)]" aria-hidden="true">🐾</span>
                </div>
              </CardContent>
            </Card>

            {/* Friendship Rank */}
            <Card className="border-border">
              <CardHeader className="p-3 pb-2">
                <CardTitle className="text-sm font-medium flex items-center gap-1">
                  <Sparkles className="w-4 h-4 text-yellow-500" aria-hidden="true" />
                  なかよしランク
                  {friendshipStatus === "preview" && (
                    <Badge variant="outline" className="ml-auto text-[10px] px-1.5 py-0">
                      サンプル
                    </Badge>
                  )}
                </CardTitle>
              </CardHeader>
              <CardContent className="p-3 pt-0">
                {friendship !== null && (friendshipStatus === "ready" || friendshipStatus === "preview") ? (
                  <>
                    <div className="flex items-baseline gap-1 mb-1">
                      <span className="text-xs text-muted-foreground">ランク</span>
                      <span
                        className="text-3xl font-bold text-[var(--yuuko-green)]"
                        data-testid="customize-friendship-rank"
                      >
                        {friendship.currentRank}
                      </span>
                    </div>
                    <div
                      className="text-[10px] text-muted-foreground mb-1"
                      data-testid="customize-friendship-progress-text"
                    >
                      {isMaxRank
                        ? "いちばん上のランクだよ！"
                        : `つぎのランクまで ${friendship.currentPoint} / ${friendship.nextRequiredPoint}`}
                    </div>
                    <Progress value={progressPercent} className="h-2 mb-3" aria-label="ランク進捗" />
                    {friendshipStatus === "preview" && (
                      <p className="text-[10px] text-muted-foreground mb-2">
                        プレビュー用のサンプル値だよ。アプリでは今のランクが表示されるよ。
                      </p>
                    )}
                  </>
                ) : (
                  <p className="text-xs text-muted-foreground mb-3" data-testid="customize-friendship-status">
                    {friendshipStatus === "error" ? "ランクを読み込めなかったよ。" : "読み込み中…"}
                  </p>
                )}
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
