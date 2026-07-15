// ai-subtask-delete.mjs の単体テスト（node:test）。
//
// 範囲（docs/00_project/ai-subtask-import-spec.md §3.7）:
// - AI分割子タスクの削除可能条件（source/status/completed/branchName/protected/parentTaskId）。
// - 親整合性（taskRole/autoStatusUpdateDisabled/splitChildCount）。
// - 削除後の親カウント計算・最後の子判定・件数超過エラー。
// - 一括取り消しのバッチ整合性（同一importBatchId・単一親・全件条件・上限・非破壊）。
//
// Firestore 依存値（deleteField / DocumentReference / serverTimestamp）は本モジュールに持ち込まないため、
// ここでは接続なしの純粋関数のみをテストする。
//
// 実行: node --test task-management/ai-subtask-delete.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  AI_SUBTASK_DELETE_MAX_BATCH,
  validateAiSubtaskDeleteCandidate,
  validateAiSubtaskDeleteParent,
  calculateAiSubtaskParentAfterDeletion,
  validateAiSubtaskBatchMembers,
  canStartAiSubtaskDeleteOperation,
} from "./ai-subtask-delete.mjs";

// 削除可能な子タスク data() の雛形。
function child(overrides = {}) {
  return {
    id: "child-1",
    title: "子タスク",
    source: "ai-subtask-import",
    status: "Todo",
    completed: false,
    branchName: null,
    protected: false,
    parentTaskId: "parent-1",
    importBatchId: "ai-batch-1",
    ...overrides,
  };
}

// 整合した「分割済み親」data() の雛形。
function parent(overrides = {}) {
  return {
    taskRole: "split-parent",
    autoStatusUpdateDisabled: true,
    splitChildCount: 3,
    ...overrides,
  };
}

// --- 子タスク削除条件（1〜11） ---

test("1: 正常なTodo子を許可", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child()).ok, true);
});

test("2: sourceが違う場合は拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ source: "manual-poc" })).ok, false);
});

test("3: Doingを拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ status: "Doing" })).ok, false);
});

test("4: Reviewを拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ status: "Review" })).ok, false);
});

test("5: Blockedを拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ status: "Blocked" })).ok, false);
});

test("6: Doneを拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ status: "Done" })).ok, false);
});

test("7: completed=trueを拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ completed: true })).ok, false);
});

test("8: branchName設定済みを拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ branchName: "feature/x" })).ok, false);
});

test("9: branchName空文字を許可", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ branchName: "   " })).ok, true);
});

test("10: protected=trueを拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ protected: true })).ok, false);
});

test("11: parentTaskId未設定を拒否", () => {
  assert.equal(validateAiSubtaskDeleteCandidate(child({ parentTaskId: "" })).ok, false);
  assert.equal(validateAiSubtaskDeleteCandidate(child({ parentTaskId: undefined })).ok, false);
});

// --- 親条件（12〜19） ---

test("12: 正常なsplit-parentを許可", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent()).ok, true);
});

test("13: taskRole不一致を拒否", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent({ taskRole: "" })).ok, false);
});

test("14: autoStatusUpdateDisabled!==trueを拒否", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent({ autoStatusUpdateDisabled: false })).ok, false);
});

test("15: splitChildCount未設定を拒否", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent({ splitChildCount: undefined })).ok, false);
});

test("16: splitChildCount=0を拒否", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent({ splitChildCount: 0 })).ok, false);
});

test("17: 負数を拒否", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent({ splitChildCount: -1 })).ok, false);
});

test("18: 小数を拒否", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent({ splitChildCount: 2.5 })).ok, false);
});

test("19: 文字列を拒否", () => {
  assert.equal(validateAiSubtaskDeleteParent(parent({ splitChildCount: "3" })).ok, false);
});

// --- 個別削除後の計算（20〜23） ---

test("20: 3件から1件削除で2（親フラグ維持）", () => {
  const r = calculateAiSubtaskParentAfterDeletion(3, 1);
  assert.equal(r.ok, true);
  assert.equal(r.nextCount, 2);
  assert.equal(r.clearParentFields, false);
});

test("21: 1件から1件削除で0かつ親解除", () => {
  const r = calculateAiSubtaskParentAfterDeletion(1, 1);
  assert.equal(r.ok, true);
  assert.equal(r.nextCount, 0);
  assert.equal(r.clearParentFields, true);
});

test("22: deleteCount=0を拒否", () => {
  assert.equal(calculateAiSubtaskParentAfterDeletion(3, 0).ok, false);
});

test("23: deleteCountが現在件数を超える場合を拒否（newCount<0）", () => {
  assert.equal(calculateAiSubtaskParentAfterDeletion(2, 3).ok, false);
});

// --- 一括取り消し（24〜32） ---

test("24: 同一importBatchId・同一親を許可", () => {
  const r = validateAiSubtaskBatchMembers(
    [child({ id: "a" }), child({ id: "b" }), child({ id: "c" })],
    "ai-batch-1",
  );
  assert.equal(r.ok, true);
  assert.equal(r.parentTaskId, "parent-1");
  assert.equal(r.count, 3);
});

test("25: importBatchId不一致を拒否", () => {
  const r = validateAiSubtaskBatchMembers([child({ id: "a" }), child({ id: "b", importBatchId: "ai-other" })], "ai-batch-1");
  assert.equal(r.ok, false);
});

test("26: parentTaskIdが複数なら拒否", () => {
  const r = validateAiSubtaskBatchMembers([child({ id: "a" }), child({ id: "b", parentTaskId: "parent-2" })], "ai-batch-1");
  assert.equal(r.ok, false);
});

test("27: 1件でもDoingなら全件拒否", () => {
  const r = validateAiSubtaskBatchMembers([child({ id: "a" }), child({ id: "b", status: "Doing" })], "ai-batch-1");
  assert.equal(r.ok, false);
});

test("28: 1件でもbranchName設定済みなら全件拒否", () => {
  const r = validateAiSubtaskBatchMembers([child({ id: "a" }), child({ id: "b", branchName: "feature/x" })], "ai-batch-1");
  assert.equal(r.ok, false);
});

test("29: 1件でもprotectedなら全件拒否", () => {
  const r = validateAiSubtaskBatchMembers([child({ id: "a" }), child({ id: "b", protected: true })], "ai-batch-1");
  assert.equal(r.ok, false);
});

test("30: 空配列を拒否", () => {
  assert.equal(validateAiSubtaskBatchMembers([], "ai-batch-1").ok, false);
});

test("31: 20件を超える場合を拒否", () => {
  const many = Array.from({ length: AI_SUBTASK_DELETE_MAX_BATCH + 1 }, (_, i) => child({ id: `c-${i}` }));
  assert.equal(validateAiSubtaskBatchMembers(many, "ai-batch-1").ok, false);
});

test("32: 元配列やオブジェクトを破壊しない", () => {
  const members = [child({ id: "a" }), child({ id: "b" })];
  const snapshotBefore = JSON.stringify(members);
  validateAiSubtaskBatchMembers(members, "ai-batch-1");
  calculateAiSubtaskParentAfterDeletion(3, 1);
  validateAiSubtaskDeleteCandidate(members[0]);
  validateAiSubtaskDeleteParent(parent());
  assert.equal(JSON.stringify(members), snapshotBefore);
});

// importBatchId 空の一括取り消しは拒否（追加の境界確認）。
test("importBatchId空を拒否", () => {
  assert.equal(validateAiSubtaskBatchMembers([child()], "").ok, false);
  assert.equal(validateAiSubtaskBatchMembers([child()], "   ").ok, false);
});

// --- 操作開始可否（再取得失敗後の再操作禁止・二重操作防止） ---

test("操作開始: 通常状態は許可", () => {
  assert.equal(canStartAiSubtaskDeleteOperation({ processing: false, refreshRequired: false }), true);
  assert.equal(canStartAiSubtaskDeleteOperation({}), true);
  assert.equal(canStartAiSubtaskDeleteOperation(), true);
});

test("操作開始: 処理中は拒否", () => {
  assert.equal(canStartAiSubtaskDeleteOperation({ processing: true, refreshRequired: false }), false);
});

test("操作開始: 再取得失敗（要リロード）は拒否", () => {
  assert.equal(canStartAiSubtaskDeleteOperation({ processing: false, refreshRequired: true }), false);
});

test("操作開始: 両方trueは拒否", () => {
  assert.equal(canStartAiSubtaskDeleteOperation({ processing: true, refreshRequired: true }), false);
});
