import { invoke } from "@tauri-apps/api/core";

export type AiProvider = "mock" | "gemini" | "openai" | "local";
export type ExplanationLevel = "simple" | "normal" | "detailed";
// 通知モード（Rust: domain::notification_mode の保存値。判断台帳 D143）。
export type NotificationMode = "random_in_work_time" | "random" | "fixed_time" | "none";

export type WorkTimeRangeDto = {
  start: string;
  end: string;
};

export type UserSettingsDto = {
  genres: string[];
  notifyStartTime: string;
  notifyEndTime: string;
  workTimeRanges?: WorkTimeRangeDto[];
  // 通知モード。読み込み時は常に返る。旧データ互換のため任意（未指定で保存すると既存値のまま）。
  notificationMode?: NotificationMode;
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
  // 初回起動時の案内を完了／スキップ済みか。読み込み時は常に返る（旧データは true 扱い）。
  // 保存時は true のときだけ完了として記録され、未指定なら既存値のまま（設定画面の保存では送らない）。
  onboardingCompleted?: boolean;
  // 読み取り専用。保存済みジャンルに合う取得元が無く、全取得元から取得する状態か（D10）。
  // Rust 側は保存時にこの値を読まないため、保存 DTO には含めない。
  genreFilterFallback?: boolean;
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

/**
 * 適用中の UI テーマ ID だけを取得する（判断台帳 D114）。
 * 設定全体を返す get_user_settings と違い、常駐ゆうこウィンドウにも許可された最小限の command。
 * Rust 側で既知のテーマ以外・読み込み失敗を既定 "default" へ倒して返す。非Tauri では null。
 */
export const getUiThemeId = async (): Promise<string | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<string>("get_ui_theme_id");
};

// get_user_settings の失敗が「設定ファイルの破損（JSONとして読めない）」かを判定する。
// Rust 側 AppError::Json は CommandError.code = "JSON_ERROR" で返る（読み込み時はJSON解析でしか発生しない）。
// 破損時も黙って既定値へ置き換えず、ユーザー操作で初期化させるための判定（判断台帳 D28）。
export const isSettingsCorruptError = (error: unknown): boolean =>
  typeof error === "object" &&
  error !== null &&
  (error as { code?: unknown }).code === "JSON_ERROR";

export const saveUserSettings = async (
  settings: UserSettingsDto
): Promise<CommandOk | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<CommandOk>("save_user_settings", { params: { settings } });
};

/**
 * 呼び名だけを保存し、保存した呼び名（前後の空白を除いたもの）を返す（カスタマイズ画面。判断台帳 D161）。
 * 設定全体を上書きする saveUserSettings と違い、ほかの欄に触れない。検証（32文字・制御文字なし）は
 * Rust 側が行い、不正な入力は reject される（何も保存しない）。非Tauri では null。
 */
export const setNickname = async (nickname: string): Promise<string | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<string>("set_nickname", { params: { nickname } });
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
  | "local_ai_missing"
  | "local_ai_broken"
  | "local_ai_start_failed"
  | "local_ai_request_failed"
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
