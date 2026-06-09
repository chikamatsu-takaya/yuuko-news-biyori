import { invoke } from "@tauri-apps/api/core";

export type AiProvider = "mock" | "gemini" | "openai" | "local";
export type ExplanationLevel = "simple" | "normal" | "detailed";

export type UserSettingsDto = {
  genres: string[];
  notifyStartTime: string;
  notifyEndTime: string;
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

// 設定を既定値へ初期化する。破壊的操作のため呼び出し側で確認を挟む（画面詳細設計書 SCR-003 §7.6）。
// 非Tauri（ブラウザプレビュー）では null を返し、画面側はローカル表示のみ初期化する。
export const resetUserSettings = async (): Promise<UserSettingsDto | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<UserSettingsDto>("reset_user_settings");
};
