"use client";

import * as React from "react";
import Image from "next/image";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import {
  Home,
  Bell,
  ShieldOff,
  Sparkles,
  Database,
  Link2,
  Settings,
  Lightbulb,
  RotateCcw,
  HelpCircle,
  Trash2,
  Download,
  Archive,
  Check,
  Info,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Progress } from "@/components/ui/progress";
import {
  getUserSettings,
  saveUserSettings,
  resetUserSettings,
  type UserSettingsDto,
} from "@/lib/tauri/settings";
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

import { useToast } from "@/hooks/use-toast";

// Types
type NotificationSettings = {
  enabled: boolean;
  startTime: string;
  endTime: string;
  frequency: string;
  minRecommendCount: number;
};

type YuukoDisplaySettings = {
  showResident: boolean;
  balloonMode: string;
  talkFrequency: string;
  animationMode: string;
};

type SuppressionSettings = {
  suppressInMeeting: boolean;
  suppressWhenMicInUse: boolean;
  suppressWhenFullscreen: boolean;
  suppressWhenGaming: boolean;
};

type AiSettings = {
  explanationDetail: string;
  termExplanationLevel: string;
  autoSuggestLongSummary: boolean;
  priorityMode: string;
  provider: string;
  providerStatus: string;
};

type DataManagementState = {
  usedStorageGb: number;
  maxStorageGb: number;
};

type SettingsState = {
  notification: NotificationSettings;
  yuuko: YuukoDisplaySettings;
  suppression: SuppressionSettings;
  ai: AiSettings;
  data: DataManagementState;
};

type SettingsMenuItem = {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
};

function YuukoDisplayMenuIcon({ className }: { className?: string }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <circle cx="12" cy="12" r="8.2" />
      <path d="M14.7 7.6c-.7-.5-1.5-.8-2.5-.8-1.8 0-3 .9-3 2.2 0 1.5 1.4 2 3 2.4 1.7.4 3 .9 3 2.5 0 1.4-1.3 2.4-3.2 2.4-1.2 0-2.2-.4-3-1" />
    </svg>
  );
}

// Mock Data
const mockSettings: SettingsState = {
  notification: {
    enabled: true,
    startTime: "07:00",
    endTime: "22:00",
    frequency: "1日3回まで",
    minRecommendCount: 3,
  },
  yuuko: {
    showResident: true,
    balloonMode: "控えめに",
    talkFrequency: "ふつう",
    animationMode: "通常",
  },
  suppression: {
    suppressInMeeting: true,
    suppressWhenMicInUse: true,
    suppressWhenFullscreen: true,
    suppressWhenGaming: false,
  },
  ai: {
    explanationDetail: "ふつう",
    termExplanationLevel: "中学生レベル",
    autoSuggestLongSummary: true,
    priorityMode: "バランス重視",
    provider: "mock",
    providerStatus: "MockProviderで動作中",
  },
  data: {
    usedStorageGb: 1.24,
    maxStorageGb: 5.0,
  },
};

const settingsMenuItems: SettingsMenuItem[] = [
  { id: "notification", label: "通知", icon: Bell },
  { id: "yuuko", label: "ゆうこ表示", icon: YuukoDisplayMenuIcon },
  { id: "suppression", label: "抑制条件", icon: ShieldOff },
  { id: "ai", label: "解説・AI設定", icon: Sparkles },
  { id: "data", label: "データ管理", icon: Database },
  { id: "integration", label: "起動・連携", icon: Link2 },
  { id: "other", label: "その他", icon: Settings },
];

const fallbackUserSettingsDto: UserSettingsDto = {
  genres: ["AI", "IT"],
  notifyStartTime: "09:00",
  notifyEndTime: "18:00",
  notifyMaxPerDay: 3,
  enableYuukoPopup: true,
  suppressDuringMeeting: true,
  suppressDuringMicUse: true,
  suppressDuringFullscreen: true,
  autoStartOnPcBoot: false,
  explanationLevel: "normal",
  selectedThemeId: "default",
  selectedToneId: "gentle",
  selectedPersonalityId: "standard",
  nickname: "",
  aiProvider: "mock",
};

const mapSettingsFromDto = (
  base: SettingsState,
  dto: UserSettingsDto
): SettingsState => ({
  ...base,
  notification: {
    ...base.notification,
    enabled: dto.enableYuukoPopup,
    startTime: dto.notifyStartTime,
    endTime: dto.notifyEndTime,
    minRecommendCount: dto.notifyMaxPerDay,
  },
  suppression: {
    ...base.suppression,
    suppressInMeeting: dto.suppressDuringMeeting,
    suppressWhenMicInUse: dto.suppressDuringMicUse,
    suppressWhenFullscreen: dto.suppressDuringFullscreen,
  },
  ai: {
    ...base.ai,
    provider: dto.aiProvider,
    providerStatus: dto.aiProvider,
  },
});

const buildDtoForSave = (
  settingsState: SettingsState,
  baseDto: UserSettingsDto | null
): UserSettingsDto => {
  const source = baseDto ?? fallbackUserSettingsDto;
  const normalizedProvider =
    settingsState.ai.provider === "mock" ||
      settingsState.ai.provider === "gemini" ||
      settingsState.ai.provider === "openai" ||
      settingsState.ai.provider === "local"
      ? settingsState.ai.provider
      : source.aiProvider;

  return {
    genres: source.genres,
    notifyStartTime: settingsState.notification.startTime,
    notifyEndTime: settingsState.notification.endTime,
    notifyMaxPerDay: settingsState.notification.minRecommendCount,
    enableYuukoPopup: settingsState.notification.enabled,
    suppressDuringMeeting: settingsState.suppression.suppressInMeeting,
    suppressDuringMicUse: settingsState.suppression.suppressWhenMicInUse,
    suppressDuringFullscreen: settingsState.suppression.suppressWhenFullscreen,
    autoStartOnPcBoot: source.autoStartOnPcBoot,
    explanationLevel: source.explanationLevel,
    selectedThemeId: source.selectedThemeId,
    selectedToneId: source.selectedToneId,
    selectedPersonalityId: source.selectedPersonalityId,
    nickname: source.nickname,
    aiProvider: normalizedProvider,
  };
};

// Sub Components
function SettingRow({
  label,
  helpText,
  children,
}: {
  label: string;
  helpText?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between py-3 border-b border-border/50 last:border-b-0">
      <div className="flex items-center gap-1.5">
        <span className="text-sm text-foreground">{label}</span>
        {helpText && (
          <HelpCircle className="w-4 h-4 text-muted-foreground cursor-help" />
        )}
      </div>
      <div>{children}</div>
    </div>
  );
}

function TimeInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="flex items-center gap-1 bg-white border border-border rounded-lg px-3 py-1.5">
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-12 text-sm text-center bg-transparent outline-none"
      />
      <div className="flex flex-col">
        <button
          className="text-muted-foreground hover:text-foreground text-[10px] leading-none"
          onClick={() => {
            const [h, m] = value.split(":").map(Number);
            const newH = (h + 1) % 24;
            onChange(`${String(newH).padStart(2, "0")}:${String(m).padStart(2, "0")}`);
          }}
        >
          ▲
        </button>
        <button
          className="text-muted-foreground hover:text-foreground text-[10px] leading-none"
          onClick={() => {
            const [h, m] = value.split(":").map(Number);
            const newH = (h - 1 + 24) % 24;
            onChange(`${String(newH).padStart(2, "0")}:${String(m).padStart(2, "0")}`);
          }}
        >
          ▼
        </button>
      </div>
    </div>
  );
}

export default function SettingsScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [settings, setSettings] = React.useState<SettingsState>(mockSettings);
  const [backendSettings, setBackendSettings] =
    React.useState<UserSettingsDto | null>(null);
  const [activeMenu, setActiveMenu] = React.useState("notification");
  const [resetDialogOpen, setResetDialogOpen] = React.useState(false);
  const [isLoading, setIsLoading] = React.useState(true);
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);
  const [loadNoticeKind, setLoadNoticeKind] = React.useState<"info" | "error">(
    "info"
  );
  const isMountedRef = React.useRef(true);

  const { toast } = useToast();

  const handleNavigate = (screen: string) => {
    if (onNavigate) {
      onNavigate(screen);
    }
  };

  const loadSettings = React.useCallback(async () => {
    setIsLoading(true);
    setLoadNotice(null);
    setLoadNoticeKind("info");

    try {
      const dto = await getUserSettings();
      if (!isMountedRef.current) {
        return;
      }

      if (!dto) {
        return;
      }

      setBackendSettings(dto);
      setSettings((prev) => mapSettingsFromDto(prev, dto));
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error("Failed to load settings from tauri command:", error);
      setLoadNotice(
        "設定の読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
      );
      setLoadNoticeKind("error");
    } finally {
      if (isMountedRef.current) {
        setIsLoading(false);
      }
    }
  }, []);

  React.useEffect(() => {
    isMountedRef.current = true;
    void loadSettings();
    return () => {
      isMountedRef.current = false;
    };
  }, [loadSettings]);

  const updateNotification = (
    key: keyof NotificationSettings,
    value: boolean | string | number
  ) => {
    setSettings((prev) => ({
      ...prev,
      notification: { ...prev.notification, [key]: value },
    }));
  };

  const updateYuuko = (
    key: keyof YuukoDisplaySettings,
    value: boolean | string
  ) => {
    setSettings((prev) => ({
      ...prev,
      yuuko: { ...prev.yuuko, [key]: value },
    }));
  };

  const updateSuppression = (
    key: keyof SuppressionSettings,
    value: boolean
  ) => {
    setSettings((prev) => ({
      ...prev,
      suppression: { ...prev.suppression, [key]: value },
    }));
  };

  const updateAi = (key: keyof AiSettings, value: boolean | string) => {
    setSettings((prev) => ({
      ...prev,
      ai: { ...prev.ai, [key]: value },
    }));
  };

  const handleSave = async () => {
    try {
      const dto = buildDtoForSave(settings, backendSettings);
      await saveUserSettings(dto);
      setBackendSettings(dto);
      toast({
        title: "設定を保存したよ",
        description: "新しい設定が反映されたよ。ありがとう！",
      });
    } catch (error) {
      console.error("Failed to save settings via tauri command:", error);
      toast({
        variant: "destructive",
        title: "保存に失敗しちゃった",
        description: "設定の保存ができなかったよ。もう一度試してみてね。",
      });
    }
  };

  const handleCancel = () => {
    const rollback = backendSettings
      ? mapSettingsFromDto(mockSettings, backendSettings)
      : mockSettings;
    setSettings(rollback);
  };

  // リセットは破壊的操作のため確認ダイアログを挟む（画面詳細設計書 SCR-003 §7.6）。
  // 実際の初期化はRust側 reset_user_settings が担当し、React側は結果DTOを反映するだけにする。
  const handleConfirmReset = async () => {
    try {
      const dto = await resetUserSettings();
      if (dto) {
        setBackendSettings(dto);
        setSettings(mapSettingsFromDto(mockSettings, dto));
      } else {
        // 非Tauri（プレビュー）時は表示のみ初期化する。
        setSettings(mockSettings);
      }
      toast({
        title: "設定をリセットしたよ",
        description: "すべての設定が初期状態に戻ったよ。",
      });
    } catch (error) {
      // 破壊的操作のため、失敗は黙殺せずユーザーへ伝える（CLAUDE.md §10.1/§10.2）。
      console.error("Failed to reset settings via tauri command:", error);
      toast({
        variant: "destructive",
        title: "リセットに失敗しちゃった",
        description: "設定を戻せなかったよ。もう一度試してみてね。",
      });
    } finally {
      setResetDialogOpen(false);
    }
  };

  const storagePercentage =
    (settings.data.usedStorageGb / settings.data.maxStorageGb) * 100;

  return (
    <div className="h-dvh bg-background flex flex-col overflow-hidden">
      <AppTitleBar />

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar */}
        <aside className="w-52 bg-white border-r border-border flex flex-col shrink-0">
          <div className="p-3">
            <Button
              variant="outline"
              className="w-full justify-start gap-2 text-[var(--yuuko-green)] border-[var(--yuuko-green)]/30 hover:bg-[var(--yuuko-green-light)]"
              onClick={() => handleNavigate("home")}
            >
              <Home className="w-4 h-4" />
              ホームへ戻る
            </Button>
          </div>

          <div className="px-4 py-2">
            <span className="text-xs font-medium text-[var(--yuuko-green)]">
              設定メニュー
            </span>
          </div>

          <nav className="flex-1 px-2 space-y-1 overflow-y-auto">
            {settingsMenuItems.map((item) => (
              <SidebarNavItem
                key={item.id}
                label={item.label}
                icon={item.icon}
                isActive={activeMenu === item.id}
                onClick={() => setActiveMenu(item.id)}
              />
            ))}
          </nav>

          {/* Settings Hint Card */}
          <div className="p-3">
            <Card className="border-[var(--yuuko-green)]/20 bg-[var(--yuuko-green-light)]/30">
              <CardContent className="p-3">
                <div className="flex items-center gap-1.5 mb-2">
                  <Lightbulb className="w-4 h-4 text-[var(--yuuko-green)]" />
                  <span className="text-xs font-medium text-[var(--yuuko-green)]">
                    設定のヒント
                  </span>
                </div>
                <p className="text-xs text-muted-foreground leading-relaxed">
                  日常の使い方に合わせて
                  <br />
                  調整すると、より快適に
                  <br />
                  ゆうこと過ごせますよ♪
                </p>
                <div className="flex justify-end mt-1">
                  <span className="text-[var(--yuuko-green)]">🐾</span>
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Reset Button */}
          <div className="p-3 border-t border-border">
            <Button
              variant="ghost"
              className="w-full justify-start gap-2 text-muted-foreground hover:text-foreground text-sm"
              onClick={() => {
                setResetDialogOpen(true);
              }}
            >
              <RotateCcw className="w-4 h-4" />
              設定を初期状態に戻す
            </Button>
          </div>
        </aside>

        {/* Center Main Area */}
        <main className="flex-1 min-w-0 overflow-y-auto p-6">
          {/* Page Title */}
          <div className="flex items-center gap-3 mb-6">
            <Settings className="w-6 h-6 text-foreground" />
            <h1 className="text-xl font-bold text-foreground">設定</h1>
            <span className="text-sm text-muted-foreground">
              ゆうことの過ごし方を、あなた好みにカスタマイズできます。
            </span>
          </div>

          {loadNotice && (
            <Alert role="presentation" className="mb-4 border-[var(--yuuko-green)]/30 bg-white shadow-sm max-w-2xl">
              <Info className="h-4 w-4 text-[var(--yuuko-green)]" aria-hidden="true" />
              <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green)]">お知らせ</AlertTitle>
              <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
                <span role={loadNoticeKind === "error" ? "alert" : "status"}>
                  {loadNotice}
                </span>
                {loadNoticeKind === "error" && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
                    onClick={() => void loadSettings()}
                    disabled={isLoading}
                  >
                    再試行
                  </Button>
                )}
              </AlertDescription>
            </Alert>
          )}

          {isLoading && (
            <div className="mb-4 flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-3 text-xs text-muted-foreground max-w-2xl">
              <span className="h-4 w-4 animate-spin rounded-full border-2 border-[var(--yuuko-green)] border-t-transparent" />
              <span>設定を読み込んでいます…</span>
            </div>
          )}

          {/* Settings Cards */}
          <div className="space-y-4 max-w-2xl">
            {/* Notification Settings */}
            {activeMenu === "notification" && (
              <Card className="border-0 shadow-sm">
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
                    <Bell className="w-5 h-5" />
                    通知設定
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <SettingRow label="ニュース通知を受け取る">
                    <Switch
                      checked={settings.notification.enabled}
                      onCheckedChange={(checked) =>
                        updateNotification("enabled", checked)
                      }
                    />
                  </SettingRow>
                  <SettingRow label="通知時間帯">
                    <div className="flex items-center gap-2">
                      <TimeInput
                        value={settings.notification.startTime}
                        onChange={(v) => updateNotification("startTime", v)}
                      />
                      <span className="text-muted-foreground">〜</span>
                      <TimeInput
                        value={settings.notification.endTime}
                        onChange={(v) => updateNotification("endTime", v)}
                      />
                    </div>
                  </SettingRow>
                  <SettingRow label="通知頻度">
                    <Select
                      value={settings.notification.frequency}
                      onValueChange={(v) => updateNotification("frequency", v)}
                    >
                      <SelectTrigger className="w-36">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="1日1回まで">1日1回まで</SelectItem>
                        <SelectItem value="1日3回まで">1日3回まで</SelectItem>
                        <SelectItem value="1日5回まで">1日5回まで</SelectItem>
                        <SelectItem value="制限なし">制限なし</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                  <SettingRow label="おすすめニュースの最小件数" helpText>
                    <div className="flex items-center gap-3">
                      <Slider
                        value={[settings.notification.minRecommendCount]}
                        onValueChange={(v) =>
                          updateNotification("minRecommendCount", v[0])
                        }
                        min={1}
                        max={10}
                        step={1}
                        className="w-32"
                      />
                      <span className="text-sm text-foreground w-8">
                        {settings.notification.minRecommendCount}件
                      </span>
                    </div>
                  </SettingRow>
                </CardContent>
              </Card>
            )}

            {/* Yuuko Display Settings */}
            {activeMenu === "yuuko" && (
              <Card className="border-0 shadow-sm">
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
                    <YuukoDisplayMenuIcon className="w-5 h-5" />
                    ゆうこ表示設定
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <SettingRow label="常駐時のゆうこを表示する">
                    <Switch
                      checked={settings.yuuko.showResident}
                      onCheckedChange={(checked) =>
                        updateYuuko("showResident", checked)
                      }
                    />
                  </SettingRow>
                  <SettingRow label="吹き出しの自動表示">
                    <Select
                      value={settings.yuuko.balloonMode}
                      onValueChange={(v) => updateYuuko("balloonMode", v)}
                    >
                      <SelectTrigger className="w-32">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="常に表示">常に表示</SelectItem>
                        <SelectItem value="控えめに">控えめに</SelectItem>
                        <SelectItem value="ほぼ表示しない">
                          ほぼ表示しない
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                  <SettingRow label="ゆうこの話しかけ頻度">
                    <Select
                      value={settings.yuuko.talkFrequency}
                      onValueChange={(v) => updateYuuko("talkFrequency", v)}
                    >
                      <SelectTrigger className="w-32">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="多め">多め</SelectItem>
                        <SelectItem value="ふつう">ふつう</SelectItem>
                        <SelectItem value="少なめ">少なめ</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                  <SettingRow label="ゆうこのアニメーション">
                    <Select
                      value={settings.yuuko.animationMode}
                      onValueChange={(v) => updateYuuko("animationMode", v)}
                    >
                      <SelectTrigger className="w-32">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="派手">派手</SelectItem>
                        <SelectItem value="通常">通常</SelectItem>
                        <SelectItem value="控えめ">控えめ</SelectItem>
                        <SelectItem value="なし">なし</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                </CardContent>
              </Card>
            )}

            {/* Suppression Settings */}
            {activeMenu === "suppression" && (
              <Card className="border-0 shadow-sm">
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
                    <ShieldOff className="w-5 h-5" />
                    抑制条件設定
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <SettingRow label="会議中は通知を抑制する">
                    <Switch
                      checked={settings.suppression.suppressInMeeting}
                      onCheckedChange={(checked) =>
                        updateSuppression("suppressInMeeting", checked)
                      }
                    />
                  </SettingRow>
                  <SettingRow label="マイク使用中は通知を抑制する">
                    <Switch
                      checked={settings.suppression.suppressWhenMicInUse}
                      onCheckedChange={(checked) =>
                        updateSuppression("suppressWhenMicInUse", checked)
                      }
                    />
                  </SettingRow>
                  <SettingRow label="フルスクリーン時は通知を抑制する">
                    <Switch
                      checked={settings.suppression.suppressWhenFullscreen}
                      onCheckedChange={(checked) =>
                        updateSuppression("suppressWhenFullscreen", checked)
                      }
                    />
                  </SettingRow>
                  <SettingRow label="ゲーム実行中は通知を抑制する">
                    <Switch
                      checked={settings.suppression.suppressWhenGaming}
                      onCheckedChange={(checked) =>
                        updateSuppression("suppressWhenGaming", checked)
                      }
                    />
                  </SettingRow>
                </CardContent>
              </Card>
            )}

            {/* AI Settings */}
            {activeMenu === "ai" && (
              <Card className="border-0 shadow-sm">
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
                    <Sparkles className="w-5 h-5" />
                    解説・AI設定
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <SettingRow label="解説の詳しさ">
                    <Select
                      value={settings.ai.explanationDetail}
                      onValueChange={(v) => updateAi("explanationDetail", v)}
                    >
                      <SelectTrigger className="w-32">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="詳しく">詳しく</SelectItem>
                        <SelectItem value="ふつう">ふつう</SelectItem>
                        <SelectItem value="簡潔に">簡潔に</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                  <SettingRow label="専門用語の解説レベル">
                    <Select
                      value={settings.ai.termExplanationLevel}
                      onValueChange={(v) => updateAi("termExplanationLevel", v)}
                    >
                      <SelectTrigger className="w-36">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="小学生レベル">小学生レベル</SelectItem>
                        <SelectItem value="中学生レベル">中学生レベル</SelectItem>
                        <SelectItem value="高校生レベル">高校生レベル</SelectItem>
                        <SelectItem value="専門家レベル">専門家レベル</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                  <SettingRow label="長文要点説明の自動候補">
                    <Switch
                      checked={settings.ai.autoSuggestLongSummary}
                      onCheckedChange={(checked) =>
                        updateAi("autoSuggestLongSummary", checked)
                      }
                    />
                  </SettingRow>
                  <SettingRow label="AI処理の優先モード">
                    <Select
                      value={settings.ai.priorityMode}
                      onValueChange={(v) => updateAi("priorityMode", v)}
                    >
                      <SelectTrigger className="w-36">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="速度重視">速度重視</SelectItem>
                        <SelectItem value="バランス重視">バランス重視</SelectItem>
                        <SelectItem value="品質重視">品質重視</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                </CardContent>
              </Card>
            )}

            {/* Placeholder categories */}
            {(activeMenu === "data" ||
              activeMenu === "integration" ||
              activeMenu === "other") && (
                <Card className="border-0 shadow-sm">
                  <CardHeader className="pb-2">
                    <CardTitle className="flex items-center gap-2 text-base text-muted-foreground">
                      {activeMenu === "data" && <Database className="w-5 h-5" />}
                      {activeMenu === "integration" && (
                        <Link2 className="w-5 h-5" />
                      )}
                      {activeMenu === "other" && <Settings className="w-5 h-5" />}
                      {settingsMenuItems.find((m) => m.id === activeMenu)?.label}
                    </CardTitle>
                  </CardHeader>
                  <CardContent className="py-8 flex flex-col items-center justify-center text-center">
                    <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center mb-3">
                      <Settings className="w-6 h-6 text-muted-foreground" />
                    </div>
                    <h3 className="text-sm font-medium text-foreground mb-1">
                      準備中だよ
                    </h3>
                    <p className="text-xs text-muted-foreground max-w-[240px]">
                      この設定項目は今後のアップデートで追加される予定です。
                      楽しみにしていてね♪
                    </p>
                  </CardContent>
                </Card>
              )}
          </div>
        </main>

        {/* Right Sidebar */}
        <aside className="w-72 bg-muted/30 border-l border-border flex flex-col shrink-0 p-4 gap-4 overflow-y-auto">
          {/* Yuuko's Comment Card */}
          <Card className="border-0 shadow-sm overflow-hidden">
            <div className="bg-[var(--yuuko-green)] px-4 py-2 flex items-center gap-2">
              <Bell className="w-4 h-4 text-white" />
              <span className="text-sm font-medium text-white">
                ゆうこの一言
              </span>
            </div>
            <CardContent className="p-4">
              <div className="relative bg-white rounded-xl p-3 border border-border shadow-sm mb-3">
                <p className="text-sm text-foreground leading-relaxed">
                  この設定で今日も
                  <br />
                  あなたにぴったりの
                  <br />
                  ニュースをお届けするね！
                </p>
                <div className="absolute -bottom-2 right-4 w-0 h-0 border-l-8 border-r-8 border-t-8 border-l-transparent border-r-transparent border-t-white" />
                <span className="absolute top-2 right-2 text-[var(--yuuko-green)]">
                  🐾
                </span>
              </div>
            </CardContent>
          </Card>

          {/* Yuuko Character */}
          <div className="flex-1 flex items-center justify-center relative">
            <div className="absolute top-2 right-2 text-[var(--yuuko-green-light)] opacity-50">
              🐾
            </div>
            <div className="absolute bottom-4 left-2 text-[var(--yuuko-green-light)] opacity-50">
              🐾
            </div>
            <Image
              src="/assets/yuuko.png"
              width={963}
              height={1174}
              alt="ゆうこ"
              className="w-48 h-auto object-contain animate-[float_3s_ease-in-out_infinite]"
              onError={(e) => {
                const target = e.target as HTMLImageElement;
                target.style.display = "none";
              }}
            />
          </div>

          {/* Data Management Card */}
          <Card className="border-0 shadow-sm">
            <CardHeader className="pb-2">
              <CardTitle className="flex items-center gap-2 text-sm text-[var(--yuuko-green)]">
                <Database className="w-4 h-4" />
                データ管理
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div>
                <div className="flex items-center justify-between mb-1.5">
                  <span className="text-xs text-muted-foreground">
                    使用中のストレージ
                  </span>
                </div>
                <div className="text-lg font-bold text-foreground mb-1">
                  {settings.data.usedStorageGb.toFixed(2)} GB /{" "}
                  {settings.data.maxStorageGb.toFixed(2)} GB
                </div>
                <Progress value={storagePercentage} className="h-2" />
              </div>
              <div className="space-y-2">
                {/* 未実装アクションは誤解を避けるため非活性＋「準備中」表示にする（候補4 方針整理 / SCR-003） */}
                <Button
                  variant="outline"
                  className="w-full justify-start gap-2 text-sm"
                  disabled
                  aria-disabled
                >
                  <Trash2 className="w-4 h-4" />
                  キャッシュを削除（準備中）
                </Button>
                <Button
                  variant="outline"
                  className="w-full justify-start gap-2 text-sm"
                  disabled
                  aria-disabled
                >
                  <Download className="w-4 h-4" />
                  辞書データをエクスポート（準備中）
                </Button>
                <Button
                  variant="outline"
                  className="w-full justify-start gap-2 text-sm"
                  disabled
                  aria-disabled
                >
                  <Archive className="w-4 h-4" />
                  アーカイブを管理（準備中）
                </Button>
                <p className="text-[11px] text-muted-foreground leading-relaxed pt-1">
                  「準備中」の機能は今後のアップデートで対応予定です。
                </p>
              </div>
            </CardContent>
          </Card>
        </aside>
      </div>

      {/* Footer */}
      <footer className="h-14 bg-white border-t border-border flex items-center justify-between px-6 shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted-foreground">設定の保存状況</span>
          <Badge
            variant="outline"
            className="bg-[var(--yuuko-green-light)] text-[var(--yuuko-green)] border-[var(--yuuko-green)]/30 gap-1"
          >
            <Check className="w-3 h-3" />
            保存済み
          </Badge>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="outline" onClick={handleCancel}>
            キャンセル
          </Button>
          <Button
            className="bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white gap-2"
            onClick={handleSave}
          >
            <Check className="w-4 h-4" />
            保存する
          </Button>
        </div>
      </footer>

      {/* リセット確認（破壊的操作・画面詳細設計書 SCR-003 §7.6） */}
      <AlertDialog open={resetDialogOpen} onOpenChange={setResetDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>設定を初期状態に戻しますか？</AlertDialogTitle>
            <AlertDialogDescription>
              通知・ゆうこ表示・抑制条件・解説/AI設定などが既定値に戻ります。保存済みの設定も上書きされ、この操作は取り消せません。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>キャンセル</AlertDialogCancel>
            <AlertDialogAction onClick={handleConfirmReset}>
              初期状態に戻す
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
