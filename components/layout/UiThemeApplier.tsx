"use client";

/**
 * 保存済みの UI テーマをアプリ全体へ反映するだけの描画なしコンポーネント。
 * ルートレイアウト（サーバーコンポーネント）から hooks を使うためのクライアント境界。
 */

import { useUiTheme } from "@/hooks/use-ui-theme";

export default function UiThemeApplier() {
  useUiTheme();
  return null;
}
