// カスタマイズ画面のテーマ一覧を組み立てる純粋関数（DOM非依存）。
// 解放・所持の判定と保存は Rust（get_reward_state / get_gacha_state / set_active_theme）が正で、
// ここは受け取った状態を「選べる／ロック中／適用中」の表示用データへ並べ替えるだけ。
// 並び: 既定テーマ → ランク報酬テーマ（解放ランク順）→ ガチャの色違いテーマ（ガチャマスタ順）。

import { DEFAULT_UI_THEME_ID, UI_THEMES, resolveUiThemeId, uiThemeLabel } from "./ui-theme.mjs";

/** ガチャテーマが未所持のときの表示名（ガチャのコレクションと同じく名前は伏せる）。 */
export const HIDDEN_THEME_LABEL = "？？？";

/** ガチャテーマのロック表示（固定文言）。 */
export const GACHA_THEME_LOCK_TEXT = "ガチャで手に入るよ";

/**
 * @typedef {{ rewardId: string, type: string, name: string, unlockRank: number, unlocked: boolean }} RewardThemeInput
 * @typedef {{ itemId: string, kind: string, owned: boolean, name: string | null }} GachaItemInput
 * @typedef {{
 *   id: string,
 *   label: string,
 *   source: "default" | "reward" | "gacha",
 *   selectable: boolean,
 *   selected: boolean,
 *   lockText: string | null,
 *   hasPalette: boolean,
 * }} ThemeOption
 */

/**
 * 表示名を決める。配色を持つ既知テーマは UI_THEMES の名前、それ以外は Rust から来た名前を使う
 * （画面にはテキストとしてだけ出す）。空なら代わりの固定文言にする。
 *
 * @param {string} id
 * @param {unknown} name
 * @param {string} fallback
 * @returns {string}
 */
const themeLabel = (id, name, fallback) => {
  const known = UI_THEMES.find((theme) => theme.id === id);
  if (known) {
    return known.label;
  }
  return typeof name === "string" && name.trim() !== "" ? name.trim() : fallback;
};

/** 配色（app/globals.css の `[data-theme]` ブロックと UI_THEMES）を持つテーマか。 */
const hasPalette = (/** @type {string} */ id) => resolveUiThemeId(id) === id;

/**
 * テーマ一覧（表示用）を作る。
 *
 * @param {{
 *   rewards?: readonly RewardThemeInput[] | null,
 *   gachaItems?: readonly GachaItemInput[] | null,
 *   activeThemeId?: unknown,
 * }} input - rewards は get_reward_state の rewards、gachaItems は get_gacha_state の items、
 *   activeThemeId は get_reward_state の activeThemeId（Rust で選べるか判定済み）。
 * @returns {ThemeOption[]}
 */
export function buildThemeOptions({ rewards, gachaItems, activeThemeId }) {
  /** @type {Omit<ThemeOption, "selected">[]} */
  const options = [
    {
      id: DEFAULT_UI_THEME_ID,
      label: uiThemeLabel(DEFAULT_UI_THEME_ID),
      source: "default",
      selectable: true,
      lockText: null,
      hasPalette: true,
    },
  ];
  const seen = new Set([DEFAULT_UI_THEME_ID]);

  for (const reward of rewards ?? []) {
    if (reward.type !== "theme" || typeof reward.rewardId !== "string" || seen.has(reward.rewardId)) {
      continue;
    }
    seen.add(reward.rewardId);
    const rank = Number.isInteger(reward.unlockRank) && reward.unlockRank > 0 ? reward.unlockRank : null;
    options.push({
      id: reward.rewardId,
      label: themeLabel(reward.rewardId, reward.name, "テーマ"),
      source: "reward",
      selectable: reward.unlocked === true,
      lockText: reward.unlocked === true ? null : rank !== null ? `ランク${rank}で解放` : "まだ解放されていないよ",
      hasPalette: hasPalette(reward.rewardId),
    });
  }

  for (const item of gachaItems ?? []) {
    if (item.kind !== "theme" || typeof item.itemId !== "string" || seen.has(item.itemId)) {
      continue;
    }
    seen.add(item.itemId);
    const owned = item.owned === true;
    options.push({
      id: item.itemId,
      label: owned ? themeLabel(item.itemId, item.name, "ガチャテーマ") : HIDDEN_THEME_LABEL,
      source: "gacha",
      selectable: owned,
      lockText: owned ? null : GACHA_THEME_LOCK_TEXT,
      hasPalette: hasPalette(item.itemId),
    });
  }

  // 適用中が一覧に無い（読み込み失敗・未知の ID）ときは既定テーマを適用中として示す。
  const active =
    typeof activeThemeId === "string" && options.some((option) => option.id === activeThemeId && option.selectable)
      ? activeThemeId
      : DEFAULT_UI_THEME_ID;
  return options.map((option) => ({ ...option, selected: option.id === active }));
}
