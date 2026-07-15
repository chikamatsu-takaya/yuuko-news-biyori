// ai-subtask-display.mjs の単体テスト（node:test）。
//
// 範囲:
// - source バッジ分類（md-import / manual-poc / ai-subtask-import / 未設定 / 未知 / 空白正規化 / 非破壊）。
// - AI分割子タスクの親解決（親あり/taskCodeなし/親なし/空/型不正/自己参照/AI分割以外/非破壊）。
// - split-parent 表示情報（正の整数/未設定/0/負数/小数/文字列/親以外）。
//
// DOM / Firestore 非依存の純粋関数のみをテストする。
// 実行: node --test task-management/ai-subtask-display.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  classifySourceBadge,
  buildTaskIndexById,
  resolveAiSubtaskParentRelation,
  resolveAiSubtaskSplitParentInfo,
} from "./ai-subtask-display.mjs";

// --- source バッジ分類 ---

test("source: md-import は既存結果（Markdown管理・source-md・補足なし）", () => {
  const b = classifySourceBadge("md-import");
  assert.equal(b.key, "md-import");
  assert.equal(b.label, "Markdown管理");
  assert.equal(b.badgeClass, "source-md");
  assert.equal(b.sourceText, "source: md-import");
  assert.equal(b.supplementaryText, "");
});

test("source: manual-poc は既存結果（DB追加 / md未反映・source-manual）", () => {
  const b = classifySourceBadge("manual-poc");
  assert.equal(b.key, "manual-poc");
  assert.equal(b.label, "DB追加 / md未反映");
  assert.equal(b.badgeClass, "source-manual");
  assert.equal(b.sourceText, "source: manual-poc");
  assert.equal(b.supplementaryText, "");
});

test("source: ai-subtask-import はAI分割タスク・専用固定クラス・Markdown未反映", () => {
  const b = classifySourceBadge("ai-subtask-import");
  assert.equal(b.key, "ai-subtask-import");
  assert.equal(b.label, "AI分割タスク");
  assert.equal(b.badgeClass, "source-ai");
  assert.equal(b.sourceText, "source: ai-subtask-import");
  assert.equal(b.supplementaryText, "Markdown未反映");
});

test("未設定（null/空）は由来不明・sourceなし", () => {
  for (const v of [null, undefined, "", "   "]) {
    const b = classifySourceBadge(v);
    assert.equal(b.key, "unknown");
    assert.equal(b.label, "由来不明");
    assert.equal(b.badgeClass, "source-unknown");
    assert.equal(b.sourceText, "sourceなし");
    assert.equal(b.supplementaryText, "");
  }
});

test("未知の source は由来不明で実値を添える（AI分割扱いにしない）", () => {
  const b = classifySourceBadge("something-else");
  assert.equal(b.key, "unknown");
  assert.equal(b.label, "由来不明");
  assert.equal(b.sourceText, "source: something-else");
});

test("前後空白は正規化して分類する", () => {
  assert.equal(classifySourceBadge("  ai-subtask-import  ").key, "ai-subtask-import");
  assert.equal(classifySourceBadge("\tmd-import\n").key, "md-import");
});

test("非文字列 source は由来不明（型で落ちる）", () => {
  for (const v of [0, 1, true, {}, []]) {
    assert.equal(classifySourceBadge(v).key, "unknown");
  }
});

// --- 親子関係の解決 ---

function childTask(overrides = {}) {
  return {
    firestoreId: "child-1",
    source: "ai-subtask-import",
    parentTaskId: "parent-1",
    taskCode: "",
    text: "子タスク",
    status: "Todo",
    ...overrides,
  };
}

function parentTask(overrides = {}) {
  return {
    firestoreId: "parent-1",
    source: "manual-poc",
    taskCode: "TASK-010",
    text: "元タスク",
    status: "Doing",
    taskRole: "split-parent",
    splitChildCount: 3,
    autoStatusUpdateDisabled: true,
    ...overrides,
  };
}

test("AI分割子の parentTaskId から親を解決できる（taskCodeあり）", () => {
  const parent = parentTask();
  const index = buildTaskIndexById([parent, childTask()]);
  const rel = resolveAiSubtaskParentRelation(childTask(), index);
  assert.equal(rel.type, "child");
  assert.equal(rel.parentFound, true);
  assert.equal(rel.parentTaskCode, "TASK-010");
  assert.equal(rel.parentTitle, "元タスク");
  assert.equal(rel.parentStatus, "Doing");
  assert.equal(rel.invalidRelation, false);
});

test("親の taskCode が空でも解決できる（タイトルだけ）", () => {
  const parent = parentTask({ taskCode: "" });
  const index = buildTaskIndexById([parent]);
  const rel = resolveAiSubtaskParentRelation(childTask(), index);
  assert.equal(rel.parentFound, true);
  assert.equal(rel.parentTaskCode, "");
  assert.equal(rel.parentTitle, "元タスク");
});

test("親が一覧に見つからない場合は parentFound=false", () => {
  const index = buildTaskIndexById([childTask()]);
  const rel = resolveAiSubtaskParentRelation(childTask(), index);
  assert.equal(rel.type, "child");
  assert.equal(rel.parentFound, false);
  assert.equal(rel.parentTaskId, "parent-1");
  assert.equal(rel.parentTaskCode, "");
});

test("parentTaskId が空なら親関係を表示しない（null）", () => {
  assert.equal(resolveAiSubtaskParentRelation(childTask({ parentTaskId: "" }), new Map()), null);
  assert.equal(resolveAiSubtaskParentRelation(childTask({ parentTaskId: "   " }), new Map()), null);
});

test("parentTaskId が型不正なら親関係を表示しない（null）", () => {
  for (const v of [123, true, {}, [], null, undefined]) {
    assert.equal(resolveAiSubtaskParentRelation(childTask({ parentTaskId: v }), new Map()), null);
  }
});

test("自分自身を親とする不正関係は invalidRelation=true・親情報なし", () => {
  const rel = resolveAiSubtaskParentRelation(childTask({ firestoreId: "x", parentTaskId: "x" }), new Map());
  assert.equal(rel.type, "child");
  assert.equal(rel.parentFound, false);
  assert.equal(rel.invalidRelation, true);
  assert.equal(rel.parentTitle, "");
});

test("AI分割以外のタスクは子として扱わない（null）", () => {
  assert.equal(resolveAiSubtaskParentRelation(childTask({ source: "md-import" }), new Map()), null);
  assert.equal(resolveAiSubtaskParentRelation(childTask({ source: "manual-poc" }), new Map()), null);
});

test("親解決は元の配列・オブジェクトを破壊しない", () => {
  const parent = parentTask();
  const child = childTask();
  const arr = [parent, child];
  const before = JSON.stringify(arr);
  const index = buildTaskIndexById(arr);
  resolveAiSubtaskParentRelation(child, index);
  assert.equal(JSON.stringify(arr), before);
});

// --- split-parent 表示情報 ---

test("split-parent かつ正の整数 count は確定表示（除外状態 true）", () => {
  const info = resolveAiSubtaskSplitParentInfo(parentTask({ splitChildCount: 3, autoStatusUpdateDisabled: true }));
  assert.equal(info.type, "parent");
  assert.equal(info.childCountKnown, true);
  assert.equal(info.childCount, 3);
  assert.equal(info.postMergeAutoStatusDisabled, true);
});

test("split-parent かつ autoStatusUpdateDisabled=false は親情報を返し除外状態 false", () => {
  const info = resolveAiSubtaskSplitParentInfo(parentTask({ autoStatusUpdateDisabled: false }));
  assert.equal(info.type, "parent");
  assert.equal(info.postMergeAutoStatusDisabled, false);
});

test("split-parent かつ autoStatusUpdateDisabled 未設定は除外状態 false", () => {
  const t = parentTask();
  delete t.autoStatusUpdateDisabled;
  const info = resolveAiSubtaskSplitParentInfo(t);
  assert.equal(info.type, "parent");
  assert.equal(info.postMergeAutoStatusDisabled, false);
});

test("autoStatusUpdateDisabled が boolean true 以外（型不正）は除外状態 false", () => {
  for (const v of ["true", 1, {}, [], null, "false", 0]) {
    const info = resolveAiSubtaskSplitParentInfo(parentTask({ autoStatusUpdateDisabled: v }));
    assert.equal(info.postMergeAutoStatusDisabled, false, `autoStatusUpdateDisabled=${Object.prototype.toString.call(v)}`);
  }
});

test("split-parent以外は autoStatusUpdateDisabled=true でも null（除外だけを理由に親表示しない）", () => {
  assert.equal(resolveAiSubtaskSplitParentInfo({ taskRole: "", autoStatusUpdateDisabled: true }), null);
});

test("resolveAiSubtaskSplitParentInfo は入力を破壊しない", () => {
  const t = parentTask({ autoStatusUpdateDisabled: false, splitChildCount: 2 });
  const before = JSON.stringify(t);
  resolveAiSubtaskSplitParentInfo(t);
  assert.equal(JSON.stringify(t), before);
});

test("split-parent だが count 未設定は確認が必要", () => {
  const info = resolveAiSubtaskSplitParentInfo(parentTask({ splitChildCount: undefined }));
  assert.equal(info.childCountKnown, false);
  assert.equal(info.childCount, null);
});

test("split-parent の count 0 / 負数 / 小数 / 文字列は確認が必要", () => {
  for (const v of [0, -1, 2.5, "3", NaN]) {
    const info = resolveAiSubtaskSplitParentInfo(parentTask({ splitChildCount: v }));
    assert.equal(info.childCountKnown, false, `count=${String(v)}`);
    assert.equal(info.childCount, null);
  }
});

test("split-parent 以外は親表示対象外（null）", () => {
  assert.equal(resolveAiSubtaskSplitParentInfo(parentTask({ taskRole: "" })), null);
  assert.equal(resolveAiSubtaskSplitParentInfo(parentTask({ taskRole: "child" })), null);
  assert.equal(resolveAiSubtaskSplitParentInfo({}), null);
});
