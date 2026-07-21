// MVP区分（mvpScope）正規化・パーサ・md→fs 変換の単体テスト。
import test from "node:test";
import assert from "node:assert/strict";

import {
  normalizeMvpScope,
  getMvpScopeDisplayName,
  getMvpScopeBadgeClass,
  isExplicitMvpScope,
  MVP_SCOPE_VALUES,
} from "./mvp-scope.mjs";
import { parseMarkdownTasks } from "./markdown-task-parser.mjs";

test("normalizeMvpScope: 正常値（正式・英大小・日本語）を正式値へ寄せる", () => {
  assert.equal(normalizeMvpScope("Required"), "Required");
  assert.equal(normalizeMvpScope("required"), "Required");
  assert.equal(normalizeMvpScope("REQUIRED"), "Required");
  assert.equal(normalizeMvpScope("MVP必須"), "Required");

  assert.equal(normalizeMvpScope("Additional"), "Additional");
  assert.equal(normalizeMvpScope("additional"), "Additional");
  assert.equal(normalizeMvpScope("ADDITIONAL"), "Additional");
  assert.equal(normalizeMvpScope("追加機能"), "Additional");

  assert.equal(normalizeMvpScope("Undecided"), "Undecided");
  assert.equal(normalizeMvpScope("undecided"), "Undecided");
  assert.equal(normalizeMvpScope("UNDECIDED"), "Undecided");
  assert.equal(normalizeMvpScope("要判断"), "Undecided");
});

test("normalizeMvpScope: 前後空白を除去して判定する", () => {
  assert.equal(normalizeMvpScope("  Required  "), "Required");
  assert.equal(normalizeMvpScope("\tMVP必須\n"), "Required");
});

test("normalizeMvpScope: 異常・境界値はすべて Undecided（安全側）", () => {
  assert.equal(normalizeMvpScope(undefined), "Undecided"); // 属性未設定
  assert.equal(normalizeMvpScope(""), "Undecided"); // 空文字
  assert.equal(normalizeMvpScope("   "), "Undecided"); // 空白のみ
  assert.equal(normalizeMvpScope(null), "Undecided"); // null
  assert.equal(normalizeMvpScope(123), "Undecided"); // 数値
  assert.equal(normalizeMvpScope(true), "Undecided"); // boolean
  assert.equal(normalizeMvpScope(["Required"]), "Undecided"); // 配列
  assert.equal(normalizeMvpScope({ v: "Required" }), "Undecided"); // オブジェクト
  assert.equal(normalizeMvpScope("Support"), "Undecided"); // 未知値（Support は不採用）
  assert.equal(normalizeMvpScope("<script>"), "Undecided"); // HTML文字列
});

test("getMvpScopeDisplayName: 表示名は3種のみ", () => {
  assert.equal(getMvpScopeDisplayName("Required"), "MVP必須");
  assert.equal(getMvpScopeDisplayName("Additional"), "追加機能");
  assert.equal(getMvpScopeDisplayName("Undecided"), "要判断");
  assert.equal(getMvpScopeDisplayName("不正値"), "要判断");
  assert.equal(getMvpScopeDisplayName(null), "要判断");
});

test("getMvpScopeBadgeClass: 生値を連結せず固定クラスだけ返す", () => {
  assert.equal(getMvpScopeBadgeClass("Required"), "mvp-scope-required");
  assert.equal(getMvpScopeBadgeClass("追加機能"), "mvp-scope-additional");
  assert.equal(getMvpScopeBadgeClass("要判断"), "mvp-scope-undecided");
  // 不正値・HTMLでも固定クラスのいずれか（DOM/クラス汚染なし）。
  assert.equal(getMvpScopeBadgeClass('"><img>'), "mvp-scope-undecided");
  assert.equal(getMvpScopeBadgeClass(42), "mvp-scope-undecided");
});

test("isExplicitMvpScope: 明示された正常値だけ true（未記載と区別）", () => {
  assert.equal(isExplicitMvpScope("Required"), true);
  assert.equal(isExplicitMvpScope("要判断"), true);
  assert.equal(isExplicitMvpScope(""), false);
  assert.equal(isExplicitMvpScope("   "), false);
  assert.equal(isExplicitMvpScope(null), false);
  assert.equal(isExplicitMvpScope("Support"), false);
  assert.equal(isExplicitMvpScope(123), false);
});

test("MVP_SCOPE_VALUES: 正式値は3種（Support を含まない）", () => {
  assert.deepEqual(MVP_SCOPE_VALUES, ["Required", "Additional", "Undecided"]);
});

test("parser: MVP scope 属性（英語）を正式値へ正規化して格納", () => {
  const md = [
    "## 1. テスト",
    "- [ ] タスクA",
    "  - Priority: P1",
    "  - MVP scope: required",
    "  - Status: Todo",
  ].join("\n");
  const { tasks } = parseMarkdownTasks(md);
  assert.equal(tasks.length, 1);
  assert.equal(tasks[0].mvpScope, "Required");
});

test("parser: MVP区分 属性（日本語）を正式値へ正規化して格納", () => {
  const md = [
    "## 1. テスト",
    "- [ ] タスクB",
    "  - MVP区分: 追加機能",
  ].join("\n");
  const { tasks } = parseMarkdownTasks(md);
  assert.equal(tasks[0].mvpScope, "Additional");
});

test("parser: 未知値の MVP scope は生値のまま保持する（Undecided へ丸めない・原文の妥当性を保持）", () => {
  // 未知値を早期に Undecided へ正規化すると、明示された正常値 Undecided と区別できず、
  // 同期側で「未知値＝同期対象外」判定ができなくなる。よって生値のまま保持する。
  const md = ["## 1. テスト", "- [ ] タスクC", "  - MVP scope: Support"].join("\n");
  const { tasks } = parseMarkdownTasks(md);
  assert.equal(tasks[0].mvpScope, "Support");
});

test("parser: 明示された Undecided は正式値として格納（未知値 Support と区別）", () => {
  const md = ["## 1. テスト", "- [ ] タスクC2", "  - MVP scope: undecided"].join("\n");
  const { tasks } = parseMarkdownTasks(md);
  assert.equal(tasks[0].mvpScope, "Undecided");
});

test("parser: MVP属性が無いタスクは mvpScope='' のまま（未記載＝空を保持）", () => {
  const md = ["## 1. テスト", "- [ ] タスクD", "  - Priority: P2"].join("\n");
  const { tasks } = parseMarkdownTasks(md);
  assert.equal(tasks[0].mvpScope, "");
});

test("parser: 既存属性のみのタスクを壊さない（mvpScope 追加後も従来フィールド維持）", () => {
  const md = [
    "## 1. テスト",
    "- [ ] タスクE",
    "  - Priority: P1",
    "  - Status: Doing",
    "  - Owner: @foo",
  ].join("\n");
  const { tasks } = parseMarkdownTasks(md);
  assert.equal(tasks[0].priority, "P1");
  assert.equal(tasks[0].status, "Doing");
  assert.equal(tasks[0].owner, "@foo");
  assert.equal(tasks[0].mvpScope, "");
});
