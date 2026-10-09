"use client";

/**
 * 保存済みの UI テーマ（設定 `ui.themeId` = DTO の selectedThemeId）を `<html data-theme>` へ反映する。
 * 配色は app/globals.css の CSS 変数で切り替えるため、ここでは属性を付け替えるだけ（再描画は CSS 任せ）。
 * メイン画面と常駐ゆうこウィンドウは同じルートレイアウトを通るので、両方で同じ処理が動く。
 * 未知の ID は既定テーマへ倒す（lib/ui-theme.mjs）。解放済みかの判定はカスタマイズ画面側の責務。
 */

import React from "react";
import { getUserSettings } from "@/lib/tauri/settings";
import { resolveUiThemeId } from "@/lib/ui-theme.mjs";

/** テーマ ID を `<html data-theme>` へ適用し、実際に適用した ID を返す。 */
export const applyUiTheme = (themeId: unknown): string => {
  const resolved = resolveUiThemeId(themeId);
  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = resolved;
  }
  return resolved;
};

/**
 * 設定を読み直してテーマを適用する。読み込みに失敗しても画面は止めず、
 * 現在の表示を保つ（初回は既定テーマのまま）。非Tauri（ブラウザプレビュー）では何もしない。
 */
export const refreshUiThemeFromSettings = async (): Promise<void> => {
  try {
    const settings = await getUserSettings();
    if (settings) {
      // ここでは「既知の ID か」だけを見る。解放・所持の判定は書き込み時に Rust の set_active_theme が行い、
      // 設定画面の保存（save_user_settings）では themeId を変えない（タスク qQsEaWBW）。
      // そのため未解放の ID が入るのは設定ファイルの手編集など例外時だけで、影響も配色に限られる。
      // 読み込み時にも判定したい場合は報酬状態の activeThemeId（Rust で判定済み）へ揃えるが、
      // 常駐ゆうこウィンドウで通知のたびに報酬・ガチャ状態を読むことになるため、現時点では行わない。
      applyUiTheme(settings.selectedThemeId);
    }
  } catch {
    // 設定破損などの詳細は設定画面側で案内する。ここでは配色だけ既定を維持する。
  }
};

/** マウント時に保存済みテーマを適用する。ルートレイアウトから 1 回だけ使う。 */
export const useUiTheme = (): void => {
  React.useEffect(() => {
    void refreshUiThemeFromSettings();
  }, []);
};
