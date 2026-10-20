// テーマの地に敷く模様（判断台帳 D152〜D154）の ID 解決と一覧を持つ純粋関数（DOM非依存）。
// 模様の形は public/assets/patterns/<file>（黒1色の SVG）で、app/globals.css の `[data-pattern="<id>"]` が
// その SVG を mask-image の型にして、色テーマの文字色（--yuuko-green-text）で塗る。ここでは
// 「どの ID を適用してよいか」と、一覧の表示名・推奨値（濃さ・大きさ）だけを持つ。
// 外部（設定ファイル）由来の文字列をそのまま DOM 属性へ流さないよう、既知の ID 以外は「なし」へ倒す。
// 選べるか（初期所持・ガチャ所持）の判定は Rust（get_pattern_state / set_active_pattern）の責務で、ここでは扱わない。
//
// 模様を増やすとき（docs/02_design/テーマ模様_素材受け入れ基準.md）:
//   1. public/assets/patterns に SVG を置く
//   2. UI_PATTERNS に 1 件足す（ガチャなら Rust GACHA_MASTER に同じ ID・名前の行も足す）
//   3. app/globals.css に同じ ID の `[data-pattern]` ブロックを足す（濃さ・大きさはここと揃える）

/** 模様なし（既定。Rust domain/pattern.rs の PATTERN_NONE_ID と一致させる）。 */
export const PATTERN_NONE_ID = "none";

/**
 * 模様の一覧（表示順）。
 * - source: "none" は模様なし、"initial" は最初から使える、"gacha" はガチャで手に入る。
 * - gacha の id / label は Rust GACHA_MASTER の item_id / name と一致させる。
 * - opacity は濃さ（11%〜19% でコントラスト比 1.15〜1.30 相当。6 テーマとも同じ）、
 *   size は大きさ（100% = SVG の実寸）。調査メモの推奨値をそのまま使い、利用者は調整しない。
 * 値は app/globals.css の `[data-pattern]` ブロックが正で、ここはテスト・表示用の写し。
 */
export const UI_PATTERNS = Object.freeze([
  Object.freeze({ id: PATTERN_NONE_ID, label: "なし", source: "none", file: null, width: 0, height: 0, opacity: 0, size: 0 }),
  Object.freeze({ id: "pattern_dots", label: "水玉", source: "initial", file: "dots.svg", width: 36, height: 36, opacity: 0.12, size: 1 }),
  Object.freeze({ id: "pattern_paw", label: "肉球", source: "initial", file: "paw.svg", width: 64, height: 64, opacity: 0.12, size: 1 }),
  Object.freeze({ id: "gacha_pattern_sparkle", label: "キラキラ", source: "gacha", file: "sparkle.svg", width: 60, height: 60, opacity: 0.13, size: 1 }),
  Object.freeze({ id: "gacha_pattern_gingham", label: "ギンガムチェック", source: "gacha", file: "gingham.svg", width: 28, height: 28, opacity: 0.12, size: 1.15 }),
  Object.freeze({ id: "gacha_pattern_bone", label: "ほねっこ", source: "gacha", file: "bone.svg", width: 64, height: 64, opacity: 0.12, size: 1 }),
  Object.freeze({ id: "gacha_pattern_grid", label: "方眼ノート", source: "gacha", file: "grid.svg", width: 64, height: 64, opacity: 0.16, size: 1 }),
  Object.freeze({ id: "gacha_pattern_kotoba", label: "ことばのかけら", source: "gacha", file: "kotoba.svg", width: 72, height: 72, opacity: 0.13, size: 1 }),
  Object.freeze({ id: "gacha_pattern_stripe", label: "マルシェストライプ", source: "gacha", file: "stripe.svg", width: 24, height: 24, opacity: 0.06, size: 1.25 }),
  Object.freeze({ id: "gacha_pattern_heart", label: "ぷっくりハート", source: "gacha", file: "heart.svg", width: 56, height: 56, opacity: 0.12, size: 1 }),
  Object.freeze({ id: "gacha_pattern_cloud", label: "雲とおひさま", source: "gacha", file: "cloud.svg", width: 72, height: 72, opacity: 0.11, size: 1 }),
  Object.freeze({ id: "gacha_pattern_sakura", label: "桜ひらり（春）", source: "gacha", file: "sakura.svg", width: 64, height: 64, opacity: 0.12, size: 1 }),
  Object.freeze({ id: "gacha_pattern_seigaiha", label: "青海波（夏）", source: "gacha", file: "seigaiha.svg", width: 40, height: 20, opacity: 0.15, size: 1.2 }),
  Object.freeze({ id: "gacha_pattern_ginkgo", label: "いちょう（秋）", source: "gacha", file: "ginkgo.svg", width: 64, height: 64, opacity: 0.12, size: 1 }),
  Object.freeze({ id: "gacha_pattern_snow", label: "ゆきのひ（冬）", source: "gacha", file: "snow.svg", width: 64, height: 64, opacity: 0.16, size: 1 }),
]);

const KNOWN_IDS = new Set(UI_PATTERNS.map((pattern) => pattern.id));

/**
 * 設定に保存された模様 ID を、実際に適用する ID へ解決する（純粋関数）。
 * 未設定・空文字・未知の ID・文字列以外は「なし」にする（安全側）。
 *
 * @param {unknown} patternId - Rust get_pattern_state の activePatternId
 * @returns {string} 適用する模様 ID
 */
export function resolveUiPatternId(patternId) {
  if (typeof patternId !== "string") {
    return PATTERN_NONE_ID;
  }
  const trimmed = patternId.trim();
  return KNOWN_IDS.has(trimmed) ? trimmed : PATTERN_NONE_ID;
}

/**
 * 模様 ID を表示名へ変換する（純粋関数）。未知の ID は「なし」の表示名にする（生の ID を画面へ出さない）。
 *
 * @param {unknown} patternId
 * @returns {string}
 */
export function uiPatternLabel(patternId) {
  const resolved = resolveUiPatternId(patternId);
  const pattern = UI_PATTERNS.find((entry) => entry.id === resolved);
  return pattern ? pattern.label : UI_PATTERNS[0].label;
}

/** ガチャ模様が未所持のときの表示名（ガチャのコレクションと同じく名前は伏せる）。 */
export const HIDDEN_PATTERN_LABEL = "？？？";

/** ガチャ模様のロック表示（固定文言）。 */
export const GACHA_PATTERN_LOCK_TEXT = "ガチャで手に入るよ";

/**
 * @typedef {{
 *   id: string,
 *   label: string,
 *   source: "none" | "initial" | "gacha",
 *   selectable: boolean,
 *   selected: boolean,
 *   lockText: string | null,
 * }} PatternOption
 */

/**
 * カスタマイズ画面の模様一覧（表示用）を作る。並びは UI_PATTERNS の順（なし → 初期 → ガチャ）。
 * 選べるかは Rust が返した selectablePatternIds だけで決める（ここで所持を推測しない）。
 * ただし「なし」は常に選べる（読み込みに失敗しても模様を外せるように）。
 *
 * @param {{ selectablePatternIds?: readonly unknown[] | null, activePatternId?: unknown }} input
 * @returns {PatternOption[]}
 */
export function buildPatternOptions({ selectablePatternIds, activePatternId }) {
  const selectable = new Set(
    (selectablePatternIds ?? []).filter((id) => typeof id === "string" && KNOWN_IDS.has(id)),
  );
  selectable.add(PATTERN_NONE_ID);
  const active = resolveUiPatternId(activePatternId);
  const activeId = selectable.has(active) ? active : PATTERN_NONE_ID;
  return UI_PATTERNS.map((pattern) => {
    const owned = selectable.has(pattern.id);
    const hidden = pattern.source === "gacha" && !owned;
    return {
      id: pattern.id,
      label: hidden ? HIDDEN_PATTERN_LABEL : pattern.label,
      source: /** @type {"none" | "initial" | "gacha"} */ (pattern.source),
      selectable: owned,
      selected: pattern.id === activeId,
      lockText: owned ? null : pattern.source === "gacha" ? GACHA_PATTERN_LOCK_TEXT : "まだ使えないよ",
    };
  });
}
