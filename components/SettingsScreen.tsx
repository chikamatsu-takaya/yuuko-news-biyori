"use client";

import * as React from "react";
import {
  Home,
  Bell,
  Cat,
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
  Minus,
  Square,
  X,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Progress } from "@/components/ui/progress";

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
  { id: "yuuko", label: "ゆうこ表示", icon: Cat },
  { id: "suppression", label: "抑制条件", icon: ShieldOff },
  { id: "ai", label: "解説・AI設定", icon: Sparkles },
  { id: "data", label: "データ管理", icon: Database },
  { id: "integration", label: "起動・連携", icon: Link2 },
  { id: "other", label: "その他", icon: Settings },
];

// Sub Components
function SettingsMenuItem({
  item,
  isActive,
  onClick,
}: {
  item: SettingsMenuItem;
  isActive: boolean;
  onClick: () => void;
}) {
  const Icon = item.icon;
  return (
    <button
      className={`w-full flex items-center gap-3 px-4 py-2.5 rounded-lg transition-all text-sm ${
        isActive
          ? "bg-[var(--yuuko-green-light)] text-[var(--yuuko-green)] font-medium border border-[var(--yuuko-green)]/30"
          : "text-muted-foreground hover:bg-muted hover:text-foreground"
      }`}
      onClick={onClick}
    >
      <Icon
        className={`w-5 h-5 ${isActive ? "text-[var(--yuuko-green)]" : ""}`}
      />
      <span>{item.label}</span>
    </button>
  );
}

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
  const [activeMenu, setActiveMenu] = React.useState("notification");

  const handleNavigate = (screen: string) => {
    if (onNavigate) {
      onNavigate(screen);
    }
  };

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

  const handleSave = () => {
    console.log("Settings saved:", settings);
  };

  const handleCancel = () => {
    console.log("Settings cancelled");
    setSettings(mockSettings);
  };

  const handleResetToDefault = () => {
    console.log("Reset to default");
    setSettings(mockSettings);
  };

  const handleClearCache = () => {
    console.log("Clear cache");
  };

  const handleExportDictionary = () => {
    console.log("Export dictionary data");
  };

  const handleManageArchive = () => {
    console.log("Manage archive");
  };

  const storagePercentage =
    (settings.data.usedStorageGb / settings.data.maxStorageGb) * 100;

  return (
    <div className="min-h-screen bg-background flex flex-col">
      {/* Title Bar */}
      <header className="h-10 bg-white border-b border-border flex items-center justify-between px-4 shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-6 h-6 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center">
            <span className="text-white text-xs">🐾</span>
          </div>
          <span className="text-sm font-medium text-foreground">
            ゆうこと、ニュースを読みやすく。
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button className="w-8 h-8 flex items-center justify-center text-muted-foreground hover:bg-muted rounded">
            <Minus className="w-4 h-4" />
          </button>
          <button className="w-8 h-8 flex items-center justify-center text-muted-foreground hover:bg-muted rounded">
            <Square className="w-3.5 h-3.5" />
          </button>
          <button className="w-8 h-8 flex items-center justify-center text-muted-foreground hover:bg-red-100 hover:text-red-600 rounded">
            <X className="w-4 h-4" />
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

          <nav className="flex-1 px-2 space-y-1">
            {settingsMenuItems.map((item) => (
              <SettingsMenuItem
                key={item.id}
                item={item}
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
              onClick={handleResetToDefault}
            >
              <RotateCcw className="w-4 h-4" />
              設定を初期状態に戻す
            </Button>
          </div>
        </aside>

        {/* Center Main Area */}
        <main className="flex-1 overflow-y-auto p-6">
          {/* Page Title */}
          <div className="flex items-center gap-3 mb-6">
            <Settings className="w-6 h-6 text-foreground" />
            <h1 className="text-xl font-bold text-foreground">設定</h1>
            <span className="text-sm text-muted-foreground">
              ゆうことの過ごし方を、あなた好みにカスタマイズできます。
            </span>
          </div>

          {/* Settings Cards */}
          <div className="space-y-4 max-w-2xl">
            {/* Notification Settings */}
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

            {/* Yuuko Display Settings */}
            <Card className="border-0 shadow-sm">
              <CardHeader className="pb-2">
                <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
                  <Cat className="w-5 h-5" />
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

            {/* Suppression Settings */}
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

            {/* AI Settings */}
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
          </div>
        </main>

        {/* Right Sidebar */}
        <aside className="w-72 bg-muted/30 border-l border-border flex flex-col shrink-0 p-4 gap-4">
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
            <img
              src="/assets/yuuko.png"
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
                <Button
                  variant="outline"
                  className="w-full justify-start gap-2 text-sm"
                  onClick={handleClearCache}
                >
                  <Trash2 className="w-4 h-4" />
                  キャッシュを削除
                </Button>
                <Button
                  variant="outline"
                  className="w-full justify-start gap-2 text-sm"
                  onClick={handleExportDictionary}
                >
                  <Download className="w-4 h-4" />
                  辞書データをエクスポート
                </Button>
                <Button
                  variant="outline"
                  className="w-full justify-start gap-2 text-sm"
                  onClick={handleManageArchive}
                >
                  <Archive className="w-4 h-4" />
                  アーカイブを管理
                </Button>
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
