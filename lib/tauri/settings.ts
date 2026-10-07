import { invoke } from "@tauri-apps/api/core";

export type AiProvider = "mock" | "gemini" | "openai" | "local";
export type ExplanationLevel = "simple" | "normal" | "detailed";

export type WorkTimeRangeDto = {
  start: string;
  end: string;
};

export type UserSettingsDto = {
  genres: string[];
  notifyStartTime: string;
  notifyEndTime: string;
  workTimeRanges?: WorkTimeRangeDto[];
  notifyMaxPerDay: number;
  enableYuukoPopup: boolean;
  suppressDuringMeeting: boolean;
  suppressDuringMicUse: boolean;
  suppressDuringFullscreen: boolean;
  autoStartOnPcBoot: boolean;
  explanationLevel: ExplanationLevel;
  selectedThemeId: string;
  selectedToneId: string;
  selectedPersonalityId: string;
  nickname: string;
  aiProvider: AiProvider;
  maxDailyRecommendations: number;
  // ニュース取得後の自動要約（既定は無効）。旧データでは欠落し得るため任意扱い。
  autoSummaryEnabled?: boolean;
};

type CommandOk = {
  ok: boolean;
};

export const isTauriRuntime = (): boolean => {
  if (typeof window === "undefined") {
    return false;
  }

  const runtimeWindow = window as Window & {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
  };

  return Boolean(runtimeWindow.__TAURI__ || runtimeWindow.__TAURI_INTERNALS__);
};

export const getUserSettings = async (): Promise<UserSettingsDto | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<UserSettingsDto>("get_user_settings");
};

export const saveUserSettings = async (
  settings: UserSettingsDto
): Promise<CommandOk | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<CommandOk>("save_user_settings", { params: { settings } });
};

// AI接続テスト結果（Rust: domain/ai_connection.rs の AiProviderConnectionTestResult と一致させる）。
// 状態・エラー種別は固定値のみで、APIキー・生エラー文・URL等は含まれない（セキュリティ詳細設計書 §8.6）。
export type AiProviderConnectionStatus =
  | "available"
  | "unavailable"
  | "not_implemented";

export type AiProviderConnectionErrorKind =
  | "api_key_missing"
  | "network"
  | "timeout"
  | "unauthorized"
  | "rate_limited"
  | "failed_precondition"
  | "invalid_response"
  | "provider_not_implemented"
  | "internal";

export type AiProviderConnectionTestResult = {
  // 設定読込失敗などで Provider を決定できない場合は null。
  provider: AiProvider | null;
  checkedProvider: AiProvider | null;
  status: AiProviderConnectionStatus;
  errorKind: AiProviderConnectionErrorKind | null;
  mockAvailable: boolean;
};

// 保存済みの AI Provider 設定で接続確認する（引数なし: 画面上の未保存値は送らない）。
// Rust 側は失敗も固定の結果DTOで返す設計。非Tauri（ブラウザプレビュー）では null を返す。
export const testAiProvider =
  async (): Promise<AiProviderConnectionTestResult | null> => {
    if (!isTauriRuntime()) {
      return null;
    }

    return invoke<AiProviderConnectionTestResult>("test_ai_provider");
  };

// 設定を既定値へ初期化する。破壊的操作のため呼び出し側で確認を挟む（画面詳細設計書 SCR-003 §7.6）。
// 非Tauri（ブラウザプレビュー）では null を返し、画面側はローカル表示のみ初期化する。
export const resetUserSettings = async (): Promise<UserSettingsDto | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<UserSettingsDto>("reset_user_settings");
};

// PC起動時の自動起動（要件定義書 §7.1.6）。OS の登録状態を正とし、Rust 側 command からだけ操作する
// （React には自動起動プラグインの権限を渡さない）。非Tauri（ブラウザプレビュー）では null を返す。
export const getAutostartEnabled = async (): Promise<boolean | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<boolean>("get_autostart_enabled");
};

// 自動起動を登録・解除し、反映後の OS 状態を返す。設定画面の保存ボタンを待たずに即時反映する。
export const setAutostartEnabled = async (
  enabled: boolean
): Promise<boolean | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<boolean>("set_autostart_enabled", { params: { enabled } });
};
