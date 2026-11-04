"use client";

/**
 * 保存済みの模様（設定 `ui.patternId`）を `<html data-pattern>` へ反映する（判断台帳 D152〜D154）。
 * 形・濃さ・色は app/globals.css の `[data-pattern]` と CSS 変数で切り替えるため、ここでは属性を付け替えるだけ。
 * 模様を敷くのはメイン画面だけ（components/layout/PatternGround.tsx から使う）。常駐ゆうこウィンドウは
 * get_pattern_state を許可していないため呼ばない（属性はルートレイアウトの既定 "none" のまま）。
 * 未知の ID は Rust 側と、念のためここ（lib/ui-pattern.mjs）でも「なし」へ倒す。
 */

import React from "react";
import { getPatternState } from "@/lib/tauri/patterns";
import { resolveUiPatternId } from "@/lib/ui-pattern.mjs";

/** 模様 ID を `<html data-pattern>` へ適用し、実際に適用した ID を返す。 */
export const applyUiPattern = (patternId: unknown): string => {
  const resolved = resolveUiPatternId(patternId);
  if (typeof document !== "undefined") {
    document.documentElement.dataset.pattern = resolved;
  }
  return resolved;
};

/**
 * 保存済みの模様を読み直して適用する。読み込みに失敗しても画面は止めず、現在の表示を保つ
 * （初回は模様なしのまま）。非Tauri（ブラウザプレビュー）では何もしない。
 */
export const refreshUiPatternFromSettings = async (): Promise<void> => {
  try {
    const state = await getPatternState();
    if (state !== null) {
      applyUiPattern(state.activePatternId);
    }
  } catch (error) {
    // 模様は見た目だけの機能。生のエラー文は画面へ出さず、調査用に warn で残す。
    console.warn("Failed to load pattern state:", error);
  }
};

/** マウント時に保存済みの模様を適用する。メイン画面の外枠から 1 回だけ使う。 */
export const useUiPattern = (): void => {
  React.useEffect(() => {
    void refreshUiPatternFromSettings();
  }, []);
};
