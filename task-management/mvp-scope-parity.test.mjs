// Node版（mvp-scope.mjs）とブラウザ版（mvp-scope.browser.js）の一致テスト（node:test）。
//
// ブラウザ版はクラシックスクリプト（window.MvpScope へ代入）で ESM import できないため、
// node:vm で読み込んで sandbox.MvpScope を取り出し、Node版 ESM と同一入力で結果を突き合わせる。
// 規則を片方だけ変えたら（別名表・表示名・バッジクラス・正規化）このテストで検出できる。
//
// 実行: node --test task-management/mvp-scope-parity.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import * as esm from "./mvp-scope.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

// ブラウザ版（クラシックスクリプト）を vm で評価し、window.MvpScope 相当を取り出す。
// IIFE 末尾の (typeof window !== "undefined" ? window : this) の this は、
// vm コンテキストのグローバル（sandbox）を指すため、sandbox.MvpScope へ代入される。
function loadBrowserMvpScope() {
  const code = readFileSync(resolve(HERE, "mvp-scope.browser.js"), "utf8");
  const sandbox = {};
  vm.createContext(sandbox);
  vm.runInContext(code, sandbox);
  return sandbox.MvpScope;
}

const browser = loadBrowserMvpScope();

// 正常値・別名・境界値・不正値を網羅した共通入力。
const INPUTS = [
  "Required",
  "required",
  "REQUIRED",
  "MVP必須",
  "Additional",
  "追加機能",
  "Undecided",
  "要判断",
  "Support",
  "", // 空文字
  "   ", // 空白
  null,
  undefined,
  123, // 数値
  true, // boolean
  ["Required"], // 配列
  { v: "Required" }, // オブジェクト
  "<script>alert(1)</script>", // HTML文字列
];

test("parity: window.MvpScope が読み込め、必要な関数を公開している", () => {
  assert.ok(browser, "sandbox.MvpScope が取得できる");
  for (const fn of [
    "normalizeMvpScope",
    "getMvpScopeDisplayName",
    "getMvpScopeBadgeClass",
    "isExplicitMvpScope",
    "resolveMvpScopeFilter",
    "matchesMvpScope",
    "filterTasksByMvpScope",
  ]) {
    assert.equal(typeof browser[fn], "function", `${fn} を公開`);
  }
});

test("parity: MVP_SCOPE_VALUES / DEFAULT_MVP_SCOPE が一致", () => {
  // ブラウザ版の配列は vm レルム由来で prototype が異なるため、主レルムへコピーして比較する。
  assert.deepEqual(Array.from(browser.MVP_SCOPE_VALUES), [...esm.MVP_SCOPE_VALUES]);
  assert.equal(browser.DEFAULT_MVP_SCOPE, esm.DEFAULT_MVP_SCOPE);
});

const FUNCTIONS = [
  "normalizeMvpScope",
  "getMvpScopeDisplayName",
  "getMvpScopeBadgeClass",
  "isExplicitMvpScope",
];

for (const fn of FUNCTIONS) {
  test(`parity: ${fn} は全入力で Node版とブラウザ版が一致`, () => {
    for (const input of INPUTS) {
      const a = esm[fn](input);
      const b = browser[fn](input);
      assert.equal(
        b,
        a,
        `${fn}(${JSON.stringify(input)}) 不一致: node=${JSON.stringify(a)} / browser=${JSON.stringify(b)}`,
      );
    }
  });
}

// 絞り込み関数の一致（選択値の解決・1タスク一致・配列絞り込み）。
test("parity: resolveMvpScopeFilter は全入力で一致", () => {
  for (const input of [...INPUTS, "all", "Required", "Additional", "Undecided", "  all  "]) {
    assert.equal(browser.resolveMvpScopeFilter(input), esm.resolveMvpScopeFilter(input), `resolve(${JSON.stringify(input)})`);
  }
});

test("parity: matchesMvpScope は全 mvpScope × 全フィルタで一致", () => {
  const scopes = ["all", "Required", "Additional", "Undecided", "Support", ""];
  for (const mv of INPUTS) {
    for (const scope of scopes) {
      const t = { mvpScope: mv };
      assert.equal(
        browser.matchesMvpScope(t, scope),
        esm.matchesMvpScope(t, scope),
        `matches(mvpScope=${JSON.stringify(mv)}, scope=${JSON.stringify(scope)})`,
      );
    }
  }
});

test("parity: filterTasksByMvpScope は同一タスク配列で同じ結果", () => {
  const tasks = INPUTS.map((mv, i) => ({ id: i, mvpScope: mv }));
  for (const scope of ["all", "Required", "Additional", "Undecided", "Support"]) {
    const a = esm.filterTasksByMvpScope(tasks, scope).map((t) => t.id);
    const b = browser.filterTasksByMvpScope(tasks, scope).map((t) => t.id);
    assert.deepEqual(b, a, `filter(scope=${JSON.stringify(scope)})`);
  }
});
