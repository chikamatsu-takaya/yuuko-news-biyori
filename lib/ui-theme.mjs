// UIテーマ（報酬テーマ）の ID 解決を行う純粋関数（DOM非依存）。
// 配色そのものは app/globals.css の `[data-theme="<id>"]` に置き、ここでは「どの ID を適用してよいか」だけを決める。
// 外部（設定ファイル）由来の文字列をそのまま DOM 属性へ流さないよう、既知の ID 以外は既定へ倒す。
// テーマを増やすとき（ガチャ報酬など）は、UI_THEMES に 1 件足し、globals.css に同じ ID の配色ブロックを足すだけでよい。
// 解放済みかどうかの判定はカスタマイズ画面側（Rust の effective_theme_id）の責務で、ここでは扱わない。

/** 報酬を何も適用していない既定テーマ（Rust domain/reward.rs の DEFAULT_THEME_ID と一致させる）。 */
export const DEFAULT_UI_THEME_ID = "default";

/**
 * 適用できるテーマの一覧。id は Rust REWARD_MASTER の reward_id と一致させる。
 * label は REWARD_MASTER の name と同じ表示名にする（2026-10-09 確定・D96）。
 */
export const UI_THEMES = Object.freeze([
  Object.freeze({ id: DEFAULT_UI_THEME_ID, label: "クリーム" }),
  Object.freeze({ id: "theme_001", label: "そらいろ" }),
  Object.freeze({ id: "theme_002", label: "さくら" }),
]);

const KNOWN_IDS = new Set(UI_THEMES.map((theme) => theme.id));

/**
 * 設定に保存されたテーマ ID を、実際に適用する ID へ解決する（純粋関数）。
 * 未設定・空文字・未知の ID・文字列以外は既定テーマにする（安全側）。
 *
 * @param {unknown} themeId - 設定 `ui.themeId`（DTO の selectedThemeId）
 * @returns {string} 適用するテーマ ID
 */
export function resolveUiThemeId(themeId) {
  if (typeof themeId !== "string") {
    return DEFAULT_UI_THEME_ID;
  }
  const trimmed = themeId.trim();
  return KNOWN_IDS.has(trimmed) ? trimmed : DEFAULT_UI_THEME_ID;
}

/**
 * 設定に保存されたテーマ ID を、画面に出す表示名（UI_THEMES の label）へ変換する（純粋関数）。
 * 未知の ID は resolveUiThemeId と同じく既定テーマの表示名にする（生の ID を画面へ出さない）。
 *
 * @param {unknown} themeId - 設定 `ui.themeId`（DTO の selectedThemeId）
 * @returns {string} 表示名
 */
export function uiThemeLabel(themeId) {
  const resolved = resolveUiThemeId(themeId);
  const theme = UI_THEMES.find((entry) => entry.id === resolved);
  return theme ? theme.label : UI_THEMES[0].label;
}
