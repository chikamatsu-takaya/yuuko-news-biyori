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
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import {
  getAutostartEnabled,
  getUserSettings,
  isSettingsCorruptError,
  saveUserSettings,
  setAutostartEnabled,
  resetUserSettings,
  testAiProvider,
  type AiProviderConnectionTestResult,
  type WorkTimeRangeDto,
  type UserSettingsDto,
  type ExplanationLevel,
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
import { TimeInput } from "@/components/settings/TimeInput";
import { DataMigrationPanel } from "@/components/settings/DataMigrationPanel";
import { ArchiveManagePanel } from "@/components/settings/ArchiveManagePanel";
import {
  GENRE_OPTIONS,
  NICKNAME_MAX_LENGTH,
  normalizeWorkTimeRanges,
} from "@/lib/settings-options";

// Types
type NotificationSettings = {
  enabled: boolean;
  workTimeRanges: WorkTimeRangeDto[];
  frequency: string;
  maxPerDay: number;
};


type SuppressionSettings = {
  suppressInMeeting: boolean;
  suppressWhenMicInUse: boolean;
  suppressWhenFullscreen: boolean;
};

type AiSettings = {
  explanationDetail: string;
  autoSuggestLongSummary: boolean;
  provider: string;
  providerStatus: string;
  autoSummaryEnabled: boolean;
};

type UserProfileSettingsState = {
  nickname: string;
};

type NewsSettingsState = {
  genres: string[];
  maxRecommendations: number;
};

type IntegrationSettings = {
  autoStartOnPcBoot: boolean;
};

type SettingsState = {
  notification: NotificationSettings;
  suppression: SuppressionSettings;
  ai: AiSettings;
  user: UserProfileSettingsState;
  news: NewsSettingsState;
  integration: IntegrationSettings;
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

// 設定画面の既定値（読み込み前・プレビュー時の初期表示）。Tauri では読み込んだ保存値で上書きする。
const mockSettings: SettingsState = {
  notification: {
    enabled: true,
    workTimeRanges: [
      { start: "09:00", end: "12:00" },
      { start: "13:00", end: "18:00" },
    ],
    frequency: "1日3回まで",
    maxPerDay: 3,
  },
  suppression: {
    suppressInMeeting: true,
    suppressWhenMicInUse: true,
    suppressWhenFullscreen: true,
  },
  ai: {
    explanationDetail: "ふつう",
    autoSuggestLongSummary: true,
    provider: "mock",
    providerStatus: "MockProviderで動作中",
    autoSummaryEnabled: false,
  },
  user: {
    nickname: "",
  },
  news: {
    genres: ["AI", "IT"],
    maxRecommendations: 10,
  },
  integration: {
    autoStartOnPcBoot: false,
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
  workTimeRanges: [
    { start: "09:00", end: "12:00" },
    { start: "13:00", end: "18:00" },
  ],
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
  maxDailyRecommendations: 10,
  autoSummaryEnabled: false,
};

// 通知頻度ドロップダウンの表示文言と、永続値 notifyMaxPerDay（= notification.maxPerDay）の対応。
// 通知頻度は notifyMaxPerDay として保存・復元する（独立フィールドは増やさない）。
// 「制限なし」は Rust 側の検証上限（0〜20）に合わせて 20 として扱う。
const UNLIMITED_MAX_PER_DAY = 20;

const frequencyToMaxPerDay = (frequency: string): number => {
  switch (frequency) {
    case "1日1回まで":
      return 1;
    case "1日5回まで":
      return 5;
    case "制限なし":
      return UNLIMITED_MAX_PER_DAY;
    case "1日3回まで":
    default:
      return 3;
  }
};

const maxPerDayToFrequency = (maxPerDay: number): string => {
  if (maxPerDay <= 1) {
    return "1日1回まで";
  }
  if (maxPerDay <= 3) {
    return "1日3回まで";
  }
  if (maxPerDay <= 5) {
    return "1日5回まで";
  }
  return "制限なし";
};

// 解説レベル（DTO: simple/normal/detailed）と「解説の詳しさ」ドロップダウン表示の対応。
// UI 表示ラベルを explanationLevel として保存・復元する（独立フィールドは増やさない）。
const explanationLevelToLabel = (level: ExplanationLevel): string => {
  switch (level) {
    case "simple":
      return "簡潔に";
    case "detailed":
      return "詳しく";
    case "normal":
    default:
      return "ふつう";
  }
};

const labelToExplanationLevel = (label: string): ExplanationLevel => {
  switch (label) {
    case "簡潔に":
      return "simple";
    case "詳しく":
      return "detailed";
    case "ふつう":
    default:
      return "normal";
  }
};

const mapSettingsFromDto = (
  base: SettingsState,
  dto: UserSettingsDto
): SettingsState => {
  const workTimeRanges = normalizeWorkTimeRanges(dto.workTimeRanges);
  // OpenAI は選択肢から外した。以前に保存された "openai" は実AI未実装で
  // もともと MockProvider で動いていたため、表示も MockProvider に合わせる（空の選択欄にしない）。
  const providerForDisplay =
    dto.aiProvider === "openai" ? "mock" : dto.aiProvider;

  return {
    ...base,
    notification: {
      ...base.notification,
      enabled: dto.enableYuukoPopup,
      workTimeRanges,
      // 保存値（notifyMaxPerDay）から復元する。保存値が無い場合は Rust 既定の 3（=「1日3回まで」）。
      maxPerDay: dto.notifyMaxPerDay,
      frequency: maxPerDayToFrequency(dto.notifyMaxPerDay),
    },
    suppression: {
      ...base.suppression,
      suppressInMeeting: dto.suppressDuringMeeting,
      suppressWhenMicInUse: dto.suppressDuringMicUse,
      suppressWhenFullscreen: dto.suppressDuringFullscreen,
    },
    ai: {
      ...base.ai,
      provider: providerForDisplay,
      providerStatus: providerForDisplay,
      // 解説レベル（explanationLevel）を「解説の詳しさ」ドロップダウン表示へ反映する。
      explanationDetail: explanationLevelToLabel(dto.explanationLevel),
      autoSummaryEnabled: dto.autoSummaryEnabled ?? false,
    },
    user: {
      ...base.user,
      nickname: dto.nickname || "",
    },
    news: {
      ...base.news,
      genres: dto.genres || [],
      maxRecommendations: dto.maxDailyRecommendations || 10,
    },
    integration: {
      ...base.integration,
      autoStartOnPcBoot: dto.autoStartOnPcBoot ?? false,
    },
  };
};

const buildDtoForSave = (
  settingsState: SettingsState,
  baseDto: UserSettingsDto | null
): UserSettingsDto => {
  const source = baseDto ?? fallbackUserSettingsDto;
  const normalizedProvider =
    settingsState.ai.provider === "mock" ||
      settingsState.ai.provider === "gemini" ||
      settingsState.ai.provider === "local"
      ? settingsState.ai.provider
      : source.aiProvider;
  const workTimeRanges = normalizeWorkTimeRanges(
    settingsState.notification.workTimeRanges
  );

  return {
    genres: settingsState.news.genres,
    notifyStartTime: workTimeRanges[0].start,
    notifyEndTime: workTimeRanges[workTimeRanges.length - 1].end,
    workTimeRanges,
    notifyMaxPerDay: settingsState.notification.maxPerDay,
    enableYuukoPopup: settingsState.notification.enabled,
    suppressDuringMeeting: settingsState.suppression.suppressInMeeting,
    suppressDuringMicUse: settingsState.suppression.suppressWhenMicInUse,
    suppressDuringFullscreen: settingsState.suppression.suppressWhenFullscreen,
    autoStartOnPcBoot: settingsState.integration.autoStartOnPcBoot,
    // 解説の詳しさドロップダウンの表示値を explanationLevel として保存する（画面値と保存値を一致させる）。
    explanationLevel: labelToExplanationLevel(settingsState.ai.explanationDetail),
    selectedThemeId: source.selectedThemeId,
    selectedToneId: source.selectedToneId,
    selectedPersonalityId: source.selectedPersonalityId,
    nickname: settingsState.user.nickname,
    aiProvider: normalizedProvider,
    maxDailyRecommendations: settingsState.news.maxRecommendations,
    autoSummaryEnabled: settingsState.ai.autoSummaryEnabled,
  };
};

// 未保存の変更があるか（画面詳細設計書 SCR-003 §7.7「設定変更あり → 保存ボタンを有効化」）。
// 比較は「保存したら書かれる DTO」同士で行う。保存済み DTO も一度画面状態へ写してから
// 保存と同じ正規化（通知時間帯・解説レベル・ジャンル既定値など）を通すため、
// 読み込んだだけで「未保存」になる誤表示を防ぐ。
// ただし aiProvider だけは保存値そのものと比べる。選択肢から外した openai は画面上 MockProvider と
// 表示され、保存すると mock へ書き換わるため、ファイルとずれている事実を「未保存」として示す。
const toComparableDto = (dto: UserSettingsDto): UserSettingsDto => ({
  ...dto,
  // ジャンルは選択の順序に意味が無いため、付け外しで順序だけ変わっても変更なしとみなす。
  genres: [...dto.genres].sort(),
});

const hasUnsavedSettingsChanges = (
  settingsState: SettingsState,
  savedDto: UserSettingsDto | null
): boolean => {
  const baseline = savedDto
    ? {
        ...buildDtoForSave(mapSettingsFromDto(mockSettings, savedDto), savedDto),
        aiProvider: savedDto.aiProvider,
      }
    : buildDtoForSave(mockSettings, null);
  const current = buildDtoForSave(settingsState, savedDto);
  return (
    JSON.stringify(toComparableDto(current)) !==
    JSON.stringify(toComparableDto(baseline))
  );
};

// AI接続テストの画面表示（固定文言のみ）。
type AiConnectionTestView = {
  tone: "success" | "warning";
  message: string;
  // Gemini で APIキー未設定のときだけ MockProvider を案内する（画面詳細設計書 SCR-003 §7.6/§7.7）。
  recommendMock: boolean;
};

// 接続テスト結果 → 表示。Rust から受け取る status / errorKind を固定文言へ写像し、
// APIキー・生エラー文は画面へ出さない（セキュリティ詳細設計書 §8.6）。
// 特定 Provider 専用の分岐は持たず、未知の値・Provider未確定は安全側の「接続失敗」に倒す。
const mapAiConnectionTestResult = (
  result: AiProviderConnectionTestResult
): AiConnectionTestView => {
  if (result.status === "available") {
    return { tone: "success", message: "利用可能です", recommendMock: false };
  }
  if (result.errorKind === "api_key_missing") {
    return {
      tone: "warning",
      message: "APIキーが未設定です",
      recommendMock: result.provider === "gemini",
    };
  }
  if (
    result.status === "not_implemented" ||
    result.errorKind === "provider_not_implemented"
  ) {
    return {
      tone: "warning",
      message: "未対応のAIプロバイダーです",
      recommendMock: false,
    };
  }
  return {
    tone: "warning",
    message: "接続に失敗しました",
    recommendMock: false,
  };
};

// command 自体が reject した場合（通常は起きない）も、生エラーを出さず接続失敗として表示する。
const aiConnectionTestFailedView: AiConnectionTestView = {
  tone: "warning",
  message: "接続に失敗しました",
  recommendMock: false,
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
    // 既定ウィンドウ（800×600）では本文列が狭く、見出しが1文字ずつ折り返すため、入力欄は次の行へ折り返す。
    <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 py-3 border-b border-border/50 last:border-b-0">
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

export default function SettingsScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [settings, setSettings] = React.useState<SettingsState>(mockSettings);
  const [backendSettings, setBackendSettings] =
    React.useState<UserSettingsDto | null>(null);
  // 保存済みジャンルに合う取得元が無く、全取得元から取得している状態か（D10）。
  // 判定は Rust 側（取得元ファイルとの照合）で行い、設定読み込み時の DTO から受け取るだけにする。
  const [genreFilterFallback, setGenreFilterFallback] = React.useState(false);
  const [activeMenu, setActiveMenu] = React.useState("notification");
  // 右サイドバーの「アーカイブを管理」から来たときだけ、データ管理タブの描画後にアーカイブ管理へスクロールする。
  const [scrollToArchiveManage, setScrollToArchiveManage] =
    React.useState(false);
  const handleOpenArchiveManage = React.useCallback(() => {
    setActiveMenu("data");
    setScrollToArchiveManage(true);
  }, []);
  React.useEffect(() => {
    if (!scrollToArchiveManage || activeMenu !== "data") {
      return;
    }
    setScrollToArchiveManage(false);
    document
      .getElementById("archive-manage")
      ?.scrollIntoView({ block: "start", behavior: "smooth" });
  }, [scrollToArchiveManage, activeMenu]);
  const [resetDialogOpen, setResetDialogOpen] = React.useState(false);
  const [isLoading, setIsLoading] = React.useState(true);
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);
  const [loadNoticeKind, setLoadNoticeKind] = React.useState<"info" | "error">(
    "info"
  );
  // 読み込み失敗が設定ファイル破損によるものか。破損時だけ「設定を初期化する」導線を出す（判断台帳 D28）。
  const [isSettingsCorrupt, setIsSettingsCorrupt] = React.useState(false);
  const [isTestingAi, setIsTestingAi] = React.useState(false);
  const [aiTestView, setAiTestView] =
    React.useState<AiConnectionTestView | null>(null);
  const [aiTestUnavailableInPreview, setAiTestUnavailableInPreview] =
    React.useState(false);
  // 自動起動: OS 状態を読めなかったときは誤操作を防ぐためスイッチを無効化する。
  const [isUpdatingAutostart, setIsUpdatingAutostart] = React.useState(false);
  const [autostartReadFailed, setAutostartReadFailed] = React.useState(false);
  const [autostartError, setAutostartError] = React.useState<string | null>(
    null
  );
  // state 更新前の連打でも二重実行しないよう、同期的に参照できる ref でも実行中を保持する。
  const isTestingAiRef = React.useRef(false);
  const isUpdatingAutostartRef = React.useRef(false);
  const isMountedRef = React.useRef(true);
  // データ取り込み（全置き換え）の実行中か。settings.json には書き込みロックが無いため、
  // 取り込み中は保存・リセット・自動起動の切り替えを止める（データ設計書 §15.7）。
  const [isMigrationImporting, setIsMigrationImporting] = React.useState(false);
  const isMigrationImportingRef = React.useRef(false);
  const handleMigrationImportingChange = React.useCallback(
    (importing: boolean) => {
      isMigrationImportingRef.current = importing;
      setIsMigrationImporting(importing);
    },
    []
  );

  const { toast } = useToast();

  const handleNavigate = (screen: string) => {
    if (onNavigate) {
      onNavigate(screen);
    }
  };

  // 自動起動は OS の登録状態を正とする。画面表示と「キャンセル」の戻り先（backendSettings）の
  // 両方を OS 状態へ合わせ、保存済み設定値とずれていても OS 側を表示する。
  const applyAutostartState = React.useCallback((enabled: boolean) => {
    setSettings((prev) => ({
      ...prev,
      integration: { ...prev.integration, autoStartOnPcBoot: enabled },
    }));
    setBackendSettings((prev) =>
      prev ? { ...prev, autoStartOnPcBoot: enabled } : prev
    );
  }, []);

  const refreshAutostartState = React.useCallback(async () => {
    try {
      const enabled = await getAutostartEnabled();
      if (!isMountedRef.current || enabled === null) {
        return;
      }
      applyAutostartState(enabled);
      setAutostartReadFailed(false);
      setAutostartError(null);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      // 調査用に種別だけ残す（OS エラー文・レジストリパスは画面へ出さない）。
      console.error(
        "Failed to read autostart state via tauri command:",
        error instanceof Error ? error.name : typeof error
      );
      setAutostartReadFailed(true);
      setAutostartError(
        "自動起動の状態を確認できなかったよ。画面を開き直してみてね。"
      );
    }
  }, [applyAutostartState]);

  const loadSettings = React.useCallback(async () => {
    setIsLoading(true);
    setLoadNotice(null);
    setLoadNoticeKind("info");
    setIsSettingsCorrupt(false);

    try {
      const dto = await getUserSettings();
      if (!isMountedRef.current) {
        return;
      }

      if (!dto) {
        return;
      }

      setBackendSettings(dto);
      setGenreFilterFallback(dto.genreFilterFallback ?? false);
      setSettings((prev) => mapSettingsFromDto(prev, dto));
      // 保存値を反映した後に OS 状態で上書きする（順序が逆だと保存値で戻ってしまう）。
      await refreshAutostartState();
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error("Failed to load settings from tauri command:", error);
      // 破損時も自動では上書きしない。原因だけを固定文言で伝え、初期化はユーザー操作（確認ダイアログ経由）に任せる。
      // 生エラー文・ファイルパスは画面へ出さない。
      const corrupt = isSettingsCorruptError(error);
      setIsSettingsCorrupt(corrupt);
      setLoadNotice(
        corrupt
          ? "設定ファイルが壊れていて、読み込めなかったよ。「設定を初期化する」で初期状態に戻せるよ（壊れたファイルは別名でコピーを1つだけ残すよ）。"
          : "設定の読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
      );
      setLoadNoticeKind("error");
    } finally {
      if (isMountedRef.current) {
        setIsLoading(false);
      }
    }
  }, [refreshAutostartState]);

  React.useEffect(() => {
    isMountedRef.current = true;
    void loadSettings();
    return () => {
      isMountedRef.current = false;
    };
  }, [loadSettings]);

  const updateNotification = (
    key: keyof NotificationSettings,
    value: boolean | string | number | WorkTimeRangeDto[]
  ) => {
    setSettings((prev) => ({
      ...prev,
      notification: { ...prev.notification, [key]: value },
    }));
  };

  // 通知頻度ドロップダウン: 表示文言と保存値(maxPerDay)を常に同期させる。
  // これにより保存・再読み込みで選択値が維持される（maxPerDay として永続化）。
  const updateNotificationFrequency = (frequency: string) => {
    setSettings((prev) => ({
      ...prev,
      notification: {
        ...prev.notification,
        frequency,
        maxPerDay: frequencyToMaxPerDay(frequency),
      },
    }));
  };

  // 1日の最大通知件数スライダー: maxPerDay を更新し、頻度ドロップダウン表示も追従させる。
  const updateNotificationMaxPerDay = (maxPerDay: number) => {
    setSettings((prev) => ({
      ...prev,
      notification: {
        ...prev.notification,
        maxPerDay,
        frequency: maxPerDayToFrequency(maxPerDay),
      },
    }));
  };

  const updateNotificationRange = (
    rangeIndex: 0 | 1,
    key: keyof WorkTimeRangeDto,
    value: string
  ) => {
    setSettings((prev) => {
      const workTimeRanges = normalizeWorkTimeRanges(
        prev.notification.workTimeRanges
      );

      if (workTimeRanges[rangeIndex] === undefined) {
        return prev;
      }

      workTimeRanges[rangeIndex] = {
        ...workTimeRanges[rangeIndex],
        [key]: value,
      };

      return {
        ...prev,
        notification: { ...prev.notification, workTimeRanges },
      };
    });
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

  const updateUser = (key: keyof UserProfileSettingsState, value: string) => {
    setSettings((prev) => ({
      ...prev,
      user: { ...prev.user, [key]: value },
    }));
  };

  const updateNews = (key: keyof NewsSettingsState, value: string[]) => {
    setSettings((prev) => ({
      ...prev,
      news: { ...prev.news, [key]: value },
    }));
  };

  const updateNewsCount = (key: keyof NewsSettingsState, value: number) => {
    setSettings((prev) => ({
      ...prev,
      news: { ...prev.news, [key]: value },
    }));
  };

  // 自動起動は OS への登録・解除なので、保存ボタンを待たずスイッチ操作で即時反映する。
  // 失敗時は表示を変えず（OS 状態のまま）固定文言で知らせる。
  const handleToggleAutostart = async (enabled: boolean) => {
    if (isUpdatingAutostartRef.current || isMigrationImportingRef.current) {
      return;
    }
    isUpdatingAutostartRef.current = true;
    setIsUpdatingAutostart(true);
    setAutostartError(null);

    try {
      const actual = await setAutostartEnabled(enabled);
      if (!isMountedRef.current) {
        return;
      }
      // 非Tauri（プレビュー）では OS へ触れないため、表示だけ切り替える。
      applyAutostartState(actual ?? enabled);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      console.error(
        "Failed to change autostart via tauri command:",
        error instanceof Error ? error.name : typeof error
      );
      setAutostartError(
        "自動起動の設定を変更できなかったよ。もう一度試してみてね。"
      );
    } finally {
      isUpdatingAutostartRef.current = false;
      if (isMountedRef.current) {
        setIsUpdatingAutostart(false);
      }
    }
  };

  // 注記の表示用に、保存済み設定から取得元フォールバックの有無だけを読む。
  // 読めなくても保存自体は成功しているため、注記は直前の状態のまま残す。
  const refreshGenreFilterFallback = async () => {
    try {
      const dto = await getUserSettings();
      if (dto && isMountedRef.current) {
        setGenreFilterFallback(dto.genreFilterFallback ?? false);
      }
    } catch (error) {
      console.error(
        "Failed to reload genre filter state:",
        error instanceof Error ? error.name : typeof error
      );
    }
  };

  const handleSave = async () => {
    // 自動起動の切り替え中は Rust 側が同じ設定ファイルへ写しを書くため、読み書きが重ならないよう待たせる。
    // データ取り込み中も、置き換え中の設定ファイルへ書き込まないよう止める。
    if (isUpdatingAutostartRef.current || isMigrationImportingRef.current) {
      return;
    }
    try {
      const dto = buildDtoForSave(settings, backendSettings);
      await saveUserSettings(dto);
      // save_user_settings は DTO を返さず、受け取った値を検証してそのまま保存する。
      // 送った DTO を新しい比較基準にして「保存済み」へ戻す（失敗時は基準を変えず未保存のまま）。
      // 自動起動は保存中に切り替えられることがあり、OS 状態が正のため、最新の値を残す（古い DTO で上書きしない）。
      setBackendSettings((prev) => ({
        ...dto,
        autoStartOnPcBoot: prev?.autoStartOnPcBoot ?? dto.autoStartOnPcBoot,
      }));
      toast({
        title: "設定を保存したよ",
        description: "新しい設定が反映されたよ。ありがとう！",
      });
      // 保存したジャンルでの取得元フォールバック有無だけを読み直す（他の画面状態は触らない）。
      void refreshGenreFilterFallback();
    } catch (error) {
      console.error("Failed to save settings via tauri command:", error);
      toast({
        variant: "destructive",
        title: "保存に失敗しちゃった",
        description: "設定の保存ができなかったよ。もう一度試してみてね。",
      });
    }
  };

  // キャンセルは変更を破棄してメイン画面へ戻る（画面詳細設計書 SCR-003 §7.5 / §7.9）。
  // 設計に確認ダイアログの指定が無いため、未保存の変更があっても確認せずに破棄する。
  // 自動起動は OS へ即時反映済みのため戻さない（backendSettings も OS 状態に合わせてある）。
  const handleCancel = () => {
    const rollback = backendSettings
      ? mapSettingsFromDto(mockSettings, backendSettings)
      : mockSettings;
    setSettings(rollback);
    handleNavigate("home");
  };

  // リセットは破壊的操作のため確認ダイアログを挟む（画面詳細設計書 SCR-003 §7.6）。
  // 実際の初期化はRust側 reset_user_settings が担当し、React側は結果DTOを反映するだけにする。
  const handleConfirmReset = async () => {
    // 自動起動の切り替え中は Rust 側が同じ設定ファイルへ写しを書くため、読み書きが重ならないよう待たせる。
    if (isUpdatingAutostartRef.current || isMigrationImportingRef.current) {
      return;
    }
    try {
      const dto = await resetUserSettings();
      if (dto) {
        setBackendSettings(dto);
        setGenreFilterFallback(dto.genreFilterFallback ?? false);
        setSettings(mapSettingsFromDto(mockSettings, dto));
        // リセットは OS の自動起動登録を変えないため、表示を OS 状態へ戻す。
        await refreshAutostartState();
      } else {
        // 非Tauri（プレビュー）時は表示のみ初期化する。
        setSettings(mockSettings);
      }
      // 初期化で読める設定ファイルになったため、読み込み失敗の表示は解除する。
      setLoadNotice(null);
      setLoadNoticeKind("info");
      setIsSettingsCorrupt(false);
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

  const hasUnsavedChanges = hasUnsavedSettingsChanges(settings, backendSettings);

  // AI接続テスト（画面詳細設計書 SCR-003 §7.5 / §7.8）。Rust 側は保存済み設定の Provider を確認するため、
  // 画面上の未保存の選択は反映されない（UI にその旨を明記する）。失敗してもアプリは止めず警告表示のみ。
  const handleTestAiProvider = async () => {
    if (isTestingAiRef.current) {
      return;
    }
    isTestingAiRef.current = true;
    setIsTestingAi(true);
    setAiTestView(null);
    setAiTestUnavailableInPreview(false);

    try {
      const result = await testAiProvider();
      if (!isMountedRef.current) {
        return;
      }
      if (!result) {
        setAiTestUnavailableInPreview(true);
        return;
      }
      setAiTestView(mapAiConnectionTestResult(result));
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      // 調査用に種別だけ残す（生エラー文は画面へ出さない）。
      console.error(
        "Failed to test AI provider via tauri command:",
        error instanceof Error ? error.name : typeof error
      );
      setAiTestView(aiConnectionTestFailedView);
    } finally {
      isTestingAiRef.current = false;
      if (isMountedRef.current) {
        setIsTestingAi(false);
      }
    }
  };

  // テーマ現在値（読み取り専用表示）。未読込・空文字は安全な既定 "default" を表示する。
  const currentThemeId = backendSettings?.selectedThemeId?.trim() || "default";

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
            <h1 className="text-xl font-bold text-foreground whitespace-nowrap shrink-0">設定</h1>
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
                  <div className="flex shrink-0 gap-2 self-start sm:self-auto">
                    {/* 破損時の初期化導線。既存のリセット確認ダイアログを開くだけで、直接は上書きしない。 */}
                    {isSettingsCorrupt && (
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                        onClick={() => setResetDialogOpen(true)}
                        disabled={isLoading}
                      >
                        設定を初期化する
                      </Button>
                    )}
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                      onClick={() => void loadSettings()}
                      disabled={isLoading}
                    >
                      再試行
                    </Button>
                  </div>
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
                  <SettingRow label={settings.notification.workTimeRanges.length > 1 ? "午前の通知時間帯" : "通知時間帯"}>
                    <div className="flex items-center gap-2">
                      <TimeInput
                        value={settings.notification.workTimeRanges[0].start}
                        onChange={(v) => updateNotificationRange(0, "start", v)}
                      />
                      <span className="text-muted-foreground">〜</span>
                      <TimeInput
                        value={settings.notification.workTimeRanges[0].end}
                        onChange={(v) => updateNotificationRange(0, "end", v)}
                      />
                    </div>
                  </SettingRow>
                  {settings.notification.workTimeRanges.length > 1 && (
                    <SettingRow label="午後の通知時間帯">
                      <div className="flex items-center gap-2">
                        <TimeInput
                          value={settings.notification.workTimeRanges[1].start}
                          onChange={(v) => updateNotificationRange(1, "start", v)}
                        />
                        <span className="text-muted-foreground">〜</span>
                        <TimeInput
                          value={settings.notification.workTimeRanges[1].end}
                          onChange={(v) => updateNotificationRange(1, "end", v)}
                        />
                      </div>
                    </SettingRow>
                  )}
                  <SettingRow label="通知頻度">
                    <Select
                      value={settings.notification.frequency}
                      onValueChange={(v) => updateNotificationFrequency(v)}
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
                  <SettingRow label="1日の最大通知件数" helpText>
                    <div className="flex items-center gap-3">
                      <Slider
                        value={[settings.notification.maxPerDay]}
                        onValueChange={(v) =>
                          updateNotificationMaxPerDay(v[0])
                        }
                        min={1}
                        max={20}
                        step={1}
                        className="w-32"
                      />
                      <span className="text-sm text-foreground w-8">
                        {settings.notification.maxPerDay}件
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
                  {/* テーマは selectedThemeId を読み取り専用で表示する。テーマ変更機能はMVP対象外（設計 §7:枠のみ）。 */}
                  {/* 他設定の保存でも selectedThemeId は buildDtoForSave が維持するため失われない。 */}
                  <SettingRow label="現在のテーマ">
                    <div className="flex flex-col items-end gap-0.5">
                      <span
                        className="text-sm text-foreground"
                        data-testid="current-theme-id"
                      >
                        {currentThemeId}
                      </span>
                      <span className="text-[10px] text-muted-foreground">
                        変更機能は準備中
                      </span>
                    </div>
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
                  {/* 会議中・マイク使用中は保存DTO(suppressDuringMeeting/MicUse)へ保存し、Rust 側が通知判定のたびに
                      Windows のマイク利用記録・会議アプリの有無で判定する（判断台帳 D65 / D68 / D69）ため操作可能。 */}
                  <SettingRow label="会議中は通知を抑制する">
                    <Switch
                      aria-label="会議中は通知を抑制する"
                      checked={settings.suppression.suppressInMeeting}
                      onCheckedChange={(checked) =>
                        updateSuppression("suppressInMeeting", checked)
                      }
                    />
                  </SettingRow>
                  <SettingRow label="マイク使用中は通知を抑制する">
                    <Switch
                      aria-label="マイク使用中は通知を抑制する"
                      checked={settings.suppression.suppressWhenMicInUse}
                      onCheckedChange={(checked) =>
                        updateSuppression("suppressWhenMicInUse", checked)
                      }
                    />
                  </SettingRow>
                  {/* フルスクリーン抑制は Rust 側で notification.suppressInFullscreen を参照して判定済み（D44）のため操作可能。 */}
                  <SettingRow label="フルスクリーン時は通知を抑制する">
                    <Switch
                      aria-label="フルスクリーン時は通知を抑制する"
                      checked={settings.suppression.suppressWhenFullscreen}
                      onCheckedChange={(checked) =>
                        updateSuppression("suppressWhenFullscreen", checked)
                      }
                    />
                  </SettingRow>
                  {/* ゲーム実行中の抑制は保存先も個別判定も無いため画面から外した（判断台帳 D70。再検討の余地あり）。
                      全画面で動くゲームはフルスクリーン抑制の対象になる。 */}
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
                  <SettingRow label="AIプロバイダー">
                    <Select
                      value={settings.ai.provider}
                      onValueChange={(v) => {
                        // 前回の接続テスト結果を新しいプロバイダーの結果と誤解させないため、切替時に消す。
                        setAiTestView(null);
                        updateAi("provider", v);
                      }}
                    >
                      <SelectTrigger className="w-48">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="mock">MockProvider（APIキー不要）</SelectItem>
                        <SelectItem value="gemini">Gemini</SelectItem>
                        {/* local は enum 上は保存できるが、実AI呼び出しは未実装（常に mock 動作）。 */}
                        {/* MVP 未検証のため「準備中」と明示し、選んでも安全側で mock で動くことを案内する。 */}
                        {/* OpenAI は選択肢から外した。保存済みの openai は読込時に mock として扱う。 */}
                        <SelectItem value="local">ローカル（準備中）</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>
                  {/* Mock 選択中は開発・デモ用であることを示す（画面詳細設計書 SCR-003 §7.7）。 */}
                  {settings.ai.provider === "mock" && (
                    <p
                      className="text-xs text-muted-foreground py-2"
                      data-testid="mock-provider-note"
                    >
                      MockProvider は開発・デモ用です（外部AIは使いません）。
                    </p>
                  )}
                  <div className="py-3 border-b border-border/50">
                    <div className="flex items-center justify-between">
                      <div className="flex flex-col gap-0.5">
                        <span className="text-sm text-foreground">AI接続テスト</span>
                        <span className="text-xs text-muted-foreground">
                          保存済みの設定でテストします（未保存の変更は反映されません）
                        </span>
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => void handleTestAiProvider()}
                        disabled={isTestingAi}
                        aria-busy={isTestingAi}
                      >
                        {isTestingAi ? "テスト中…" : "接続テスト"}
                      </Button>
                    </div>
                    {/* 結果は固定文言のみ表示（APIキー・生エラー文は出さない）。 */}
                    <div role="status" aria-live="polite" data-testid="ai-connection-test-result">
                      {aiTestView && (
                        <div
                          className={`mt-2 text-xs leading-relaxed p-2 rounded-lg border ${
                            aiTestView.tone === "success"
                              ? "border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/40 text-[var(--yuuko-green)]"
                              : "border-amber-300 bg-amber-50 text-amber-800"
                          }`}
                        >
                          <p>{aiTestView.message}</p>
                          {aiTestView.recommendMock && (
                            <p className="mt-1">
                              Gemini を使うにはAPIキーの設定が必要です。キーを設定するまでは <strong>MockProvider</strong> の利用をおすすめします。
                            </p>
                          )}
                        </div>
                      )}
                      {aiTestUnavailableInPreview && (
                        <p className="mt-2 text-xs text-muted-foreground">
                          接続テストはアプリ内でのみ実行できます。
                        </p>
                      )}
                    </div>
                  </div>
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
                  {/* 取得後の自動要約。外部AIの利用枠を使い切らないよう既定は無効。保存ボタンで他のAI設定と一緒に保存する。 */}
                  {/* 実際に動くかは Rust 側が判定する（実AI＝現在は Gemini のときだけ）。ここでは案内だけ出す。 */}
                  <div className="py-3 border-b border-border/50">
                    <div className="flex items-center justify-between">
                      <div className="flex flex-col gap-0.5">
                        <span className="text-sm text-foreground">ニュース取得後に自動で要約する</span>
                        <span
                          className="text-xs text-muted-foreground"
                          data-testid="auto-summary-help"
                        >
                          AIの設定がMockのときは動きません（実AIのときだけ、1件ずつ順番に要約します）
                        </span>
                      </div>
                      <Switch
                        aria-label="ニュース取得後に自動で要約する"
                        checked={settings.ai.autoSummaryEnabled}
                        onCheckedChange={(checked) =>
                          updateAi("autoSummaryEnabled", checked)
                        }
                      />
                    </div>
                  </div>
                  {/* 長文要点説明の自動候補は保存DTOに対応フィールドが無く永続化されないため非活性＋「準備中」。
                      AIプロバイダー(aiProvider)・解説の詳しさ(explanationLevel)はDTO保存されるため操作可能のまま。 */}
                  <SettingRow label="長文要点説明の自動候補（準備中）">
                    <Switch
                      aria-label="長文要点説明の自動候補（準備中）"
                      checked={settings.ai.autoSuggestLongSummary}
                      disabled
                      aria-disabled
                      onCheckedChange={(checked) =>
                        updateAi("autoSuggestLongSummary", checked)
                      }
                    />
                  </SettingRow>
                  {/* APIキー安全案内（CLAUDE.md §7 セキュリティ / §4.4 禁止事項）。 */}
                  {/* APIキーはこの画面で扱わず表示・保存もしない。未設定時は MockProvider で安全に動く。 */}
                  <p className="text-xs text-muted-foreground mt-4 leading-relaxed bg-muted/40 p-3 rounded-lg border border-border/50">
                    💡 APIキーが未設定の場合は <strong>MockProvider</strong> を推奨します。APIキーはこの画面には表示・保存されません（安全のため別途管理されます）。現在、実際の外部AIを利用できるのは <strong>Gemini</strong>（APIキー設定時）のみで、Gemini でもキー未設定時は自動的に MockProvider で動作します。ローカルは準備中のため、選んでも現在は MockProvider で動作します。
                  </p>
                </CardContent>
              </Card>
            )}

            {/* Other Settings */}
            {activeMenu === "other" && (
              <Card className="border-0 shadow-sm">
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
                    <Settings className="w-5 h-5" />
                    ユーザー・ニュース設定
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <SettingRow label="ニックネーム">
                    <div className="flex flex-col items-end gap-1">
                      <Input
                        value={settings.user.nickname}
                        onChange={(e) => updateUser("nickname", e.target.value)}
                        placeholder="ゆうこに呼んでほしい名前"
                        className="w-48"
                        maxLength={NICKNAME_MAX_LENGTH}
                      />
                      <span className="text-[10px] text-muted-foreground">
                        {NICKNAME_MAX_LENGTH}文字以内
                      </span>
                    </div>
                  </SettingRow>

                  <SettingRow label="ホームに表示するおすすめ記事数">
                    <div className="flex items-center gap-3">
                      <Slider
                        value={[settings.news.maxRecommendations]}
                        onValueChange={(v) =>
                          updateNewsCount("maxRecommendations", v[0])
                        }
                        min={1}
                        max={50}
                        step={1}
                        className="w-32"
                      />
                      <span className="text-sm text-foreground w-8">
                        {settings.news.maxRecommendations}件
                      </span>
                    </div>
                  </SettingRow>

                  <div className="py-3">
                    <div className="flex items-center gap-1.5 mb-3">
                      <span className="text-sm text-foreground">関心のあるジャンル</span>
                    </div>
                    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
                      {GENRE_OPTIONS.map((genre) => (
                        <div key={genre} className="flex items-center space-x-2">
                          <Checkbox
                            id={`genre-${genre}`}
                            checked={settings.news.genres.includes(genre)}
                            onCheckedChange={(checked) => {
                              const currentGenres = settings.news.genres;
                              if (checked) {
                                updateNews("genres", [...currentGenres, genre]);
                              } else {
                                updateNews("genres",
                                  currentGenres.filter((g) => g !== genre)
                                );
                              }
                            }}
                          />
                          <label
                            htmlFor={`genre-${genre}`}
                            className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
                          >
                            {genre}
                          </label>
                        </div>
                      ))}
                    </div>
                    {genreFilterFallback && (
                      <p
                        role="status"
                        className="mt-3 text-xs text-muted-foreground"
                        data-testid="genre-filter-fallback-note"
                      >
                        選んだジャンルに合う取得元がないため、すべての取得元から取得しています
                      </p>
                    )}
                  </div>
                </CardContent>
              </Card>
            )}

            {/* データ管理: データ移行（ZIPの書き出し・読み込み）とアーカイブ管理（古い月の削除。判断台帳 D26）。
                ストレージ状況は右サイドバーで準備中のまま。 */}
            {activeMenu === "data" && (
              <>
                <DataMigrationPanel
                  onImportingChange={handleMigrationImportingChange}
                />
                <ArchiveManagePanel />
              </>
            )}

            {/* Integration Settings */}
            {activeMenu === "integration" && (
              <Card className="border-0 shadow-sm">
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2 text-base text-[var(--yuuko-green)]">
                    <Link2 className="w-5 h-5" />
                    起動・連携設定
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <SettingRow label="PC起動時の自動起動設定">
                    <Switch
                      aria-label="PC起動時の自動起動"
                      checked={settings.integration.autoStartOnPcBoot}
                      disabled={isUpdatingAutostart || autostartReadFailed}
                      onCheckedChange={(checked) =>
                        void handleToggleAutostart(checked)
                      }
                    />
                  </SettingRow>
                  {autostartError && (
                    <p
                      role="alert"
                      data-testid="autostart-error"
                      className="text-xs text-destructive mt-2"
                    >
                      {autostartError}
                    </p>
                  )}
                  <p className="text-xs text-muted-foreground mt-4 leading-relaxed bg-muted/40 p-3 rounded-lg border border-border/50">
                    💡 この設定は切り替えるとすぐに反映されるよ（「保存する」は不要）。PC起動時はメイン画面を出さず、トレイで待機して始まるよ。
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
                ストレージ状況
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              {/* 実容量の取得は新しい command が要るため別途判断。固定の仮値は実値と誤認されるので出さない（SCR-003）。 */}
              <p className="text-xs text-muted-foreground" data-testid="storage-status-placeholder">
                保存データの使用状況の表示は準備中です。
              </p>
              <div className="space-y-2">
                {/* 辞書の単独書き出しは作らず、データ移行（ZIP 書き出し）で兼ねるため項目を置かない（判断台帳 D24） */}
                {/* アーカイブ管理の本体は「データ管理」タブに置き、ここからはそこへ移動するだけにする（判断台帳 D26） */}
                <Button
                  variant="outline"
                  className="w-full justify-start gap-2 text-sm"
                  onClick={handleOpenArchiveManage}
                >
                  <Archive className="w-4 h-4" />
                  アーカイブを管理
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
          {/* 保存済み DTO と画面の値の差分で切り替える（§7.7）。 */}
          <span role="status" aria-live="polite" data-testid="settings-save-state">
            {hasUnsavedChanges ? (
              <Badge
                variant="outline"
                className="bg-amber-50 text-amber-800 border-amber-300 gap-1"
              >
                未保存の変更あり
              </Badge>
            ) : (
              <Badge
                variant="outline"
                className="bg-[var(--yuuko-green-light)] text-[var(--yuuko-green)] border-[var(--yuuko-green)]/30 gap-1"
              >
                <Check className="w-3 h-3" />
                保存済み
              </Badge>
            )}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="outline" onClick={handleCancel}>
            キャンセル
          </Button>
          <Button
            className="bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white gap-2"
            onClick={handleSave}
            // 設定ファイル破損中は Rust 側も保存を拒否するため、初期化するまで保存させない（判断台帳 D28）。
            // 変更が無いときも押せない（§7.7「設定変更あり → 保存ボタンを有効化」）。
            // 破損以外の読み込み失敗中も、画面の値は保存済み設定ではなく既定値なので、保存で上書きさせない（再試行で読み直す）。
            disabled={
              !hasUnsavedChanges ||
              isUpdatingAutostart ||
              isSettingsCorrupt ||
              loadNoticeKind === "error" ||
              // 初回読み込み・再試行の最中も、画面の値はまだ保存値ではないため保存させない。
              isLoading ||
              isMigrationImporting
            }
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
              {/* 破損時は Rust 側が壊れたファイルを別名で1つだけ残してから初期化する（判断台帳 D57）。パスは出さない。 */}
              {isSettingsCorrupt
                ? "通知・ゆうこ表示・抑制条件・解説/AI設定などが既定値に戻ります。壊れた設定ファイルは、初期化の前に別名でコピーを1つだけ残します。"
                : "通知・ゆうこ表示・抑制条件・解説/AI設定などが既定値に戻ります。保存済みの設定も上書きされ、この操作は取り消せません。"}
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
