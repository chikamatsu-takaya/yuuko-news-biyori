// MVP区分による表示絞り込み（純粋関数）の単体テスト（node:test）。
//
// 対象: mvp-scope.mjs の resolveMvpScopeFilter / matchesMvpScope / filterTasksByMvpScope。
// - Firestore/Firebase 設定・DOM に依存しない純粋関数のみをテストする
//   （firebase-config.js〔.gitignore 対象〕へ依存しないため、クリーン checkout の CI でも成功する）。
//
// 実行: node --test task-management/mvp-scope-filter.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  resolveMvpScopeFilter,
  matchesMvpScope,
  filterTasksByMvpScope,
  MVP_SCOPE_FILTER_VALUES,
} from "./mvp-scope.mjs";

// mvpScope を持つタスク（絞り込みは mvpScope だけを見る）。
function task(id, mvpScope) {
  return { id, mvpScope };
}

// --- resolveMvpScopeFilter（選択値の安全解決） ---

test("resolveMvpScopeFilter: all と正式値だけ受理し、それ以外は all", () => {
  assert.equal(resolveMvpScopeFilter("all"), "all");
  assert.equal(resolveMvpScopeFilter("Required"), "Required");
  assert.equal(resolveMvpScopeFilter("Additional"), "Additional");
  assert.equal(resolveMvpScopeFilter("Undecided"), "Undecided");
  assert.equal(resolveMvpScopeFilter("  Required  "), "Required"); // 前後空白は除去
  // 不正・未知・非文字列は安全側で all。
  assert.equal(resolveMvpScopeFilter("Support"), "all");
  assert.equal(resolveMvpScopeFilter(""), "all");
  assert.equal(resolveMvpScopeFilter("   "), "all");
  assert.equal(resolveMvpScopeFilter(null), "all");
  assert.equal(resolveMvpScopeFilter(undefined), "all");
  assert.equal(resolveMvpScopeFilter(123), "all");
  assert.equal(resolveMvpScopeFilter(["Required"]), "all");
});

test("MVP_SCOPE_FILTER_VALUES: all + 正式3種の4値", () => {
  assert.deepEqual(MVP_SCOPE_FILTER_VALUES, ["all", "Required", "Additional", "Undecided"]);
});

// --- matchesMvpScope（1タスクの一致判定） ---

test("matchesMvpScope: all は常に一致", () => {
  assert.equal(matchesMvpScope(task("a", "Required"), "all"), true);
  assert.equal(matchesMvpScope(task("b", "Support"), "all"), true);
  assert.equal(matchesMvpScope(task("c", undefined), "all"), true);
  assert.equal(matchesMvpScope(null, "all"), true);
});

test("matchesMvpScope: 正式値は正規化して一致判定", () => {
  assert.equal(matchesMvpScope(task("a", "Required"), "Required"), true);
  assert.equal(matchesMvpScope(task("a", "追加機能"), "Additional"), true); // 別名も正規化
  assert.equal(matchesMvpScope(task("a", "Required"), "Additional"), false);
});

test("matchesMvpScope: 未設定・空・未知値・非文字列は Undecided 扱い", () => {
  for (const bad of [undefined, "", "   ", "Support", 123, true, ["x"], { v: 1 }]) {
    assert.equal(matchesMvpScope(task("a", bad), "Undecided"), true, `${JSON.stringify(bad)} は Undecided`);
    assert.equal(matchesMvpScope(task("a", bad), "Required"), false);
  }
  assert.equal(matchesMvpScope(null, "Undecided"), true); // task 自体が null でも例外にしない
});

// --- filterTasksByMvpScope（配列の絞り込み） ---

const SAMPLE = [
  task("req", "Required"),
  task("add", "Additional"),
  task("und", "Undecided"),
  task("unset", undefined),
  task("empty", ""),
  task("unknown", "Support"),
  task("num", 123),
];

test("filterTasksByMvpScope: all は全件（コピー）を返す", () => {
  const out = filterTasksByMvpScope(SAMPLE, "all");
  assert.equal(out.length, SAMPLE.length);
  assert.notEqual(out, SAMPLE, "新しい配列を返す（参照は別）");
  assert.deepEqual(out.map((t) => t.id), SAMPLE.map((t) => t.id));
});

test("filterTasksByMvpScope: Required のみ", () => {
  const out = filterTasksByMvpScope(SAMPLE, "Required");
  assert.deepEqual(out.map((t) => t.id), ["req"]);
});

test("filterTasksByMvpScope: Additional のみ", () => {
  const out = filterTasksByMvpScope(SAMPLE, "Additional");
  assert.deepEqual(out.map((t) => t.id), ["add"]);
});

test("filterTasksByMvpScope: Undecided は未設定・空・未知値・非文字列を含む", () => {
  const out = filterTasksByMvpScope(SAMPLE, "Undecided");
  assert.deepEqual(out.map((t) => t.id), ["und", "unset", "empty", "unknown", "num"]);
});

test("filterTasksByMvpScope: 配列以外は [] を返す（例外にしない）", () => {
  for (const bad of [null, undefined, "x", 123, { length: 2 }]) {
    assert.deepEqual(filterTasksByMvpScope(bad, "Required"), []);
  }
});

test("filterTasksByMvpScope: 不正なフィルタ値は安全側で全件（all）", () => {
  assert.equal(filterTasksByMvpScope(SAMPLE, "Support").length, SAMPLE.length);
  assert.equal(filterTasksByMvpScope(SAMPLE, "").length, SAMPLE.length);
  assert.equal(filterTasksByMvpScope(SAMPLE, 123).length, SAMPLE.length);
});

test("filterTasksByMvpScope: 入力配列とタスクオブジェクトを破壊しない", () => {
  // JSON 化は mvpScope:undefined を落とすため使わず、参照とプロパティで不変性を確認する。
  const t1 = task("req", "Required");
  const t2 = task("und", undefined);
  const input = [t1, t2];
  const out = filterTasksByMvpScope(input, "Required");
  // 入力配列の件数・並び・要素参照は不変。
  assert.equal(input.length, 2);
  assert.equal(input[0], t1);
  assert.equal(input[1], t2);
  // タスクオブジェクトのプロパティは不変（undefined も保持）。
  assert.equal(t1.mvpScope, "Required");
  assert.ok("mvpScope" in t2 && t2.mvpScope === undefined);
  // 返り値は入力とは別の配列で、フィルタ済み。
  assert.notEqual(out, input);
  assert.deepEqual(out.map((t) => t.id), ["req"]);
});

test("filterTasksByMvpScope: 同じ mvpScope なら由来（Markdown/Firestore）に依らず同じ結果", () => {
  // Markdown 由来（parser が空文字を残す）と Firestore 由来（正規化済み Undecided）でも、
  // 正規化後は同じ Undecided 集合になる。
  const markdownDerived = [task("md-req", "Required"), task("md-unset", ""), task("md-add", "Additional")];
  const firestoreDerived = [task("fs-req", "Required"), task("fs-unset", "Undecided"), task("fs-add", "Additional")];
  assert.deepEqual(
    filterTasksByMvpScope(markdownDerived, "Undecided").map((t) => t.mvpScope === "" ? "U" : t.mvpScope),
    ["U"],
  );
  assert.deepEqual(filterTasksByMvpScope(firestoreDerived, "Undecided").map((t) => t.id), ["fs-unset"]);
  assert.equal(filterTasksByMvpScope(markdownDerived, "Required").length, 1);
  assert.equal(filterTasksByMvpScope(firestoreDerived, "Required").length, 1);
});
