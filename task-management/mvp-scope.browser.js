// MVP区分（mvpScope）のブラウザ用ヘルパ（クラシックスクリプト）。
//
// task-dashboard.js / firestore-source.js / markdown-sync-ui.js から window.MvpScope として使う。
// クラシックスクリプトは ESM（mvp-scope.mjs）を import できないため、同じ規則をここに持たせる。
// 規則を変えるときは mvp-scope.mjs と本ファイルを必ず同時に更新すること。
//
// 正式値は "Required" / "Additional" / "Undecided" の3種のみ。
// 未設定・空・空白・null・非文字列・未知値・不正値はすべて安全側で "Undecided"。
// Firestore の生値をそのまま CSS クラスや DOM へ埋め込まないため、
// バッジクラス・表示名は必ず本ヘルパの固定マップ経由で取得する。
(function attachMvpScope(global) {
  "use strict";

  var MVP_SCOPE_VALUES = ["Required", "Additional", "Undecided"];
  var DEFAULT_MVP_SCOPE = "Undecided";

  var ALIASES = {
    required: "Required",
    "mvp必須": "Required",
    additional: "Additional",
    追加機能: "Additional",
    undecided: "Undecided",
    要判断: "Undecided",
  };

  var DISPLAY_NAMES = {
    Required: "MVP必須",
    Additional: "追加機能",
    Undecided: "要判断",
  };

  var BADGE_CLASSES = {
    Required: "mvp-scope-required",
    Additional: "mvp-scope-additional",
    Undecided: "mvp-scope-undecided",
  };

  function normalizeMvpScope(value) {
    if (typeof value !== "string") {
      return DEFAULT_MVP_SCOPE;
    }
    var trimmed = value.trim();
    if (trimmed === "") {
      return DEFAULT_MVP_SCOPE;
    }
    var canonical = ALIASES[trimmed.toLowerCase()];
    return canonical || DEFAULT_MVP_SCOPE;
  }

  function getMvpScopeDisplayName(value) {
    return DISPLAY_NAMES[normalizeMvpScope(value)];
  }

  function getMvpScopeBadgeClass(value) {
    return BADGE_CLASSES[normalizeMvpScope(value)];
  }

  function isExplicitMvpScope(value) {
    if (typeof value !== "string") {
      return false;
    }
    var trimmed = value.trim();
    if (trimmed === "") {
      return false;
    }
    return Object.prototype.hasOwnProperty.call(ALIASES, trimmed.toLowerCase());
  }

  // --- MVP区分による絞り込み（mvp-scope.mjs と同一規則。変更時は両方を更新すること）。---
  var MVP_SCOPE_FILTER_ALL = "all";
  var MVP_SCOPE_FILTER_VALUES = [MVP_SCOPE_FILTER_ALL].concat(MVP_SCOPE_VALUES);

  // 絞り込み選択値を安全な正規値へ解決する（"all" と正式値のみ受理・それ以外は "all"）。
  function resolveMvpScopeFilter(value) {
    if (typeof value !== "string") {
      return MVP_SCOPE_FILTER_ALL;
    }
    var trimmed = value.trim();
    if (trimmed === MVP_SCOPE_FILTER_ALL) {
      return MVP_SCOPE_FILTER_ALL;
    }
    return MVP_SCOPE_VALUES.indexOf(trimmed) >= 0 ? trimmed : MVP_SCOPE_FILTER_ALL;
  }

  // 1タスクが選択中のMVP区分フィルタに一致するか（"all" は常に true）。
  function matchesMvpScope(task, selectedScope) {
    var scope = resolveMvpScopeFilter(selectedScope);
    if (scope === MVP_SCOPE_FILTER_ALL) {
      return true;
    }
    var value = task == null ? undefined : task.mvpScope;
    return normalizeMvpScope(value) === scope;
  }

  // タスク配列を選択中のMVP区分で絞り込む（新しい配列を返す・入力とタスクを破壊しない）。
  function filterTasksByMvpScope(tasks, selectedScope) {
    if (!Array.isArray(tasks)) {
      return [];
    }
    var scope = resolveMvpScopeFilter(selectedScope);
    if (scope === MVP_SCOPE_FILTER_ALL) {
      return tasks.slice();
    }
    return tasks.filter(function (task) {
      return matchesMvpScope(task, scope);
    });
  }

  global.MvpScope = {
    MVP_SCOPE_VALUES: MVP_SCOPE_VALUES,
    DEFAULT_MVP_SCOPE: DEFAULT_MVP_SCOPE,
    MVP_SCOPE_FILTER_ALL: MVP_SCOPE_FILTER_ALL,
    MVP_SCOPE_FILTER_VALUES: MVP_SCOPE_FILTER_VALUES,
    normalizeMvpScope: normalizeMvpScope,
    getMvpScopeDisplayName: getMvpScopeDisplayName,
    getMvpScopeBadgeClass: getMvpScopeBadgeClass,
    isExplicitMvpScope: isExplicitMvpScope,
    resolveMvpScopeFilter: resolveMvpScopeFilter,
    matchesMvpScope: matchesMvpScope,
    filterTasksByMvpScope: filterTasksByMvpScope,
  };
})(typeof window !== "undefined" ? window : this);
