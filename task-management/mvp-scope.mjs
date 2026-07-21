// MVP区分（mvpScope）の正規化・表示・バッジクラスを一元管理する純粋関数モジュール（ESM）。
//
// 役割:
// - Markdown / Firestore / 画面モデルで扱う MVP 区分の正式値を一本化する。
// - 正式値は "Required" / "Additional" / "Undecided" の3種のみ。
// - 別名（英大小・日本語）を正式値へ寄せ、未設定・空・不正・非文字列・未知値は
//   安全側で "Undecided" に倒す（Priority/Status 等からの自動推測は一切しない）。
//
// 注意:
// - Firestore の生値をそのまま HTML クラスや DOM へ埋め込まないため、
//   バッジクラス・表示名は必ず本モジュールの固定マップ経由で取得する。
// - ブラウザ（クラシックスクリプト）側は mvp-scope.browser.js に同じ規則を持たせて揃える
//   （ESM を import できないため。規則を変えるときは両方を必ず同時に更新すること）。

/** MVP区分の正式値。この3種以外は保存・表示に用いない。 */
export const MVP_SCOPE_VALUES = ["Required", "Additional", "Undecided"];

/** 安全側の既定値（未設定・不正・未知値はすべてこれ）。 */
export const DEFAULT_MVP_SCOPE = "Undecided";

// 別名 → 正式値。キーは小文字化後で引く（日本語は小文字化の影響を受けない）。
const MVP_SCOPE_ALIASES = new Map([
  ["required", "Required"],
  ["mvp必須", "Required"],
  ["additional", "Additional"],
  ["追加機能", "Additional"],
  ["undecided", "Undecided"],
  ["要判断", "Undecided"],
]);

// 正式値 → 画面表示名。
const MVP_SCOPE_DISPLAY_NAMES = {
  Required: "MVP必須",
  Additional: "追加機能",
  Undecided: "要判断",
};

// 正式値 → 固定 CSS クラス（生値を連結しない）。
const MVP_SCOPE_BADGE_CLASSES = {
  Required: "mvp-scope-required",
  Additional: "mvp-scope-additional",
  Undecided: "mvp-scope-undecided",
};

/**
 * 任意の入力を MVP区分の正式値（Required/Additional/Undecided）へ正規化する。
 * 非文字列・空・空白・null・未知値・不正値はすべて "Undecided"。
 * @param {unknown} value
 * @returns {"Required"|"Additional"|"Undecided"}
 */
export function normalizeMvpScope(value) {
  if (typeof value !== "string") {
    return DEFAULT_MVP_SCOPE;
  }
  const trimmed = value.trim();
  if (trimmed === "") {
    return DEFAULT_MVP_SCOPE;
  }
  return MVP_SCOPE_ALIASES.get(trimmed.toLowerCase()) ?? DEFAULT_MVP_SCOPE;
}

/**
 * MVP区分の画面表示名（「MVP必須」/「追加機能」/「要判断」）を返す。
 * @param {unknown} value
 * @returns {string}
 */
export function getMvpScopeDisplayName(value) {
  return MVP_SCOPE_DISPLAY_NAMES[normalizeMvpScope(value)];
}

/**
 * MVP区分の固定バッジ CSS クラスを返す（生値は連結しない）。
 * @param {unknown} value
 * @returns {string}
 */
export function getMvpScopeBadgeClass(value) {
  return MVP_SCOPE_BADGE_CLASSES[normalizeMvpScope(value)];
}

/**
 * 入力が MVP区分として「明示的に指定された正常値」かどうか。
 * Markdown 未記載（空）と区別したいときに使う（sync の同期対象判定・warning 用）。
 * 空・空白・非文字列・未知値は false。
 * @param {unknown} value
 * @returns {boolean}
 */
export function isExplicitMvpScope(value) {
  if (typeof value !== "string") {
    return false;
  }
  const trimmed = value.trim();
  if (trimmed === "") {
    return false;
  }
  return MVP_SCOPE_ALIASES.has(trimmed.toLowerCase());
}

// ---------------------------------------------------------------------------
// MVP区分による絞り込み（表示専用の純粋関数）。
// タスク配列・タスクオブジェクトを破壊せず、正規化は normalizeMvpScope を再利用する
// （task-dashboard.js 側へ正規化ロジックを複製しない）。
// ---------------------------------------------------------------------------

/** 絞り込みの内部値。"all" は全件、それ以外は正式値と一致するもの。 */
export const MVP_SCOPE_FILTER_ALL = "all";
export const MVP_SCOPE_FILTER_VALUES = [MVP_SCOPE_FILTER_ALL, ...MVP_SCOPE_VALUES];

/**
 * 絞り込み選択値を安全な正規値へ解決する。
 * "all" と正式値（Required/Additional/Undecided）だけを受理し、
 * それ以外（未知値・空・非文字列）は安全側で "all"（全件）にする。
 * ※ 選択値そのものは normalizeMvpScope に通さない（"all" が Undecided に化けるのを防ぐ）。
 * @param {unknown} value
 * @returns {"all"|"Required"|"Additional"|"Undecided"}
 */
export function resolveMvpScopeFilter(value) {
  if (typeof value !== "string") {
    return MVP_SCOPE_FILTER_ALL;
  }
  const trimmed = value.trim();
  if (trimmed === MVP_SCOPE_FILTER_ALL) {
    return MVP_SCOPE_FILTER_ALL;
  }
  return MVP_SCOPE_VALUES.includes(trimmed) ? trimmed : MVP_SCOPE_FILTER_ALL;
}

/**
 * 1タスクが選択中のMVP区分フィルタに一致するか。
 * "all" は常に true。タスクの mvpScope は normalizeMvpScope で正規化して比較するため、
 * 未設定・空・未知値・非文字列は Undecided 扱いになる（画面表示仕様と同じ）。
 * @param {unknown} task mvpScope を持ちうるタスク（null/undefined でも例外にしない）
 * @param {unknown} selectedScope 絞り込み選択値
 * @returns {boolean}
 */
export function matchesMvpScope(task, selectedScope) {
  const scope = resolveMvpScopeFilter(selectedScope);
  if (scope === MVP_SCOPE_FILTER_ALL) {
    return true;
  }
  const value = task == null ? undefined : task.mvpScope;
  return normalizeMvpScope(value) === scope;
}

/**
 * タスク配列を選択中のMVP区分で絞り込む（新しい配列を返す・入力は破壊しない）。
 * - 配列以外は [] を返す（例外にしない）。
 * - "all" / 不正なフィルタ値は全件のコピーを返す。
 * - タスクオブジェクト自体は書き換えない。
 * @param {unknown} tasks
 * @param {unknown} selectedScope
 * @returns {Array}
 */
export function filterTasksByMvpScope(tasks, selectedScope) {
  if (!Array.isArray(tasks)) {
    return [];
  }
  const scope = resolveMvpScopeFilter(selectedScope);
  if (scope === MVP_SCOPE_FILTER_ALL) {
    return tasks.slice();
  }
  return tasks.filter((task) => matchesMvpScope(task, scope));
}
