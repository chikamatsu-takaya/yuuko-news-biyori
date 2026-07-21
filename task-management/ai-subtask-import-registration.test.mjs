// ai-subtask-import-registration.mjs の単体テスト（node:test）。
//
// 範囲（docs/00_project/ai-subtask-import-spec.md §3.3 / §3.6 / §3.9 / §5 / §6 / §7）:
// - 登録用スナップショットの最終検証（AI JSON部分＋継承値）。
// - 親データの登録可否判定。
// - 子タスク保存値・親更新値の組み立て（ホワイトリスト・order採番・importBatchId・改行保持・非破壊）。
// - Firestore 依存値（serverTimestamp / DocumentReference / crypto）は本モジュールに持ち込まないため、
//   ここでは接続なしの純粋関数のみをテストする。
//
// 実行: node --test task-management/ai-subtask-import-registration.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  validateAiSubtaskRegistrationSnapshot,
  validateAiSubtaskRegistrationParent,
  buildAiSubtaskChildPayloads,
  buildAiSubtaskParentUpdate,
} from "./ai-subtask-import-registration.mjs";

// 許可11項目を持つ子タスク（登録用スナップショットの tasks 要素）。
function childTask(overrides = {}) {
  return {
    title: "子タスク",
    purpose: "目的",
    splitReason: "分割理由",
    scope: ["対象1"],
    outOfScope: ["非対象1"],
    doneWhen: ["完了1"],
    notes: ["メモ1"],
    implementationPrompt: "実装",
    reviewPoints: ["観点1"],
    reviewPrompt: "レビュー依頼",
    verificationCommands: ["pnpm lint"],
    ...overrides,
  };
}

// 登録用スナップショット。
function snapshot(overrides = {}) {
  return {
    schemaVersion: 1,
    splitSummary: "要約",
    inheritedValues: { category: "タスク管理機能", subcategory: "AI分割タスク取込", priority: "P1", owner: "近松" },
    tasks: [childTask()],
    ...overrides,
  };
}

// --- 登録用スナップショット検証 ---

test("スナップショット検証: 正常なら受理", () => {
  const r = validateAiSubtaskRegistrationSnapshot(snapshot());
  assert.equal(r.ok, true, JSON.stringify(r.jsonResult.errors));
});

test("スナップショット検証: tasks 0件なら拒否", () => {
  const r = validateAiSubtaskRegistrationSnapshot(snapshot({ tasks: [] }));
  assert.equal(r.ok, false);
  assert.equal(r.jsonResult.ok, false);
});

test("スナップショット検証: 不正な子タスク項目（title上限超過）を拒否", () => {
  const r = validateAiSubtaskRegistrationSnapshot(snapshot({ tasks: [childTask({ title: "a".repeat(121) })] }));
  assert.equal(r.ok, false);
  assert.ok(r.jsonResult.errors.some((e) => e.path === "tasks[0].title"));
});

test("スナップショット検証: category 空を拒否", () => {
  const r = validateAiSubtaskRegistrationSnapshot(
    snapshot({ inheritedValues: { category: "", subcategory: "", priority: "P1", owner: "" } }),
  );
  assert.equal(r.ok, false);
  assert.equal(r.inheritedResult.ok, false);
  assert.ok(r.inheritedResult.errors.some((e) => e.field === "category"));
});

test("スナップショット検証: 不正 priority を拒否", () => {
  const r = validateAiSubtaskRegistrationSnapshot(
    snapshot({ inheritedValues: { category: "x", subcategory: "", priority: "P9", owner: "" } }),
  );
  assert.equal(r.ok, false);
  assert.ok(r.inheritedResult.errors.some((e) => e.field === "priority"));
});

test("スナップショット検証: システム項目（status等）混入を拒否・継承値へ混ぜない", () => {
  const r = validateAiSubtaskRegistrationSnapshot(snapshot({ tasks: [childTask({ status: "Doing" })] }));
  assert.equal(r.ok, false);
  assert.equal(r.jsonResult.ok, false);
  // 継承値検証に category/inheritedValues の unknown key エラーは出ない。
  const paths = r.jsonResult.errors.map((e) => e.path);
  assert.ok(!paths.includes("inheritedValues"));
});

// --- 子タスク保存値の組み立て ---

test("子タスク: 2件へ同じ importBatchId を設定する", () => {
  const payloads = buildAiSubtaskChildPayloads({
    snapshot: snapshot({ tasks: [childTask({ title: "a" }), childTask({ title: "b" })] }),
    parentTaskId: "parent1",
    importBatchId: "ai-batch-1",
    baseOrder: 0,
  });
  assert.equal(payloads.length, 2);
  assert.ok(payloads.every((p) => p.importBatchId === "ai-batch-1"));
});

test("子タスク: order は base+10, base+20（表示順維持）", () => {
  const payloads = buildAiSubtaskChildPayloads({
    snapshot: snapshot({ tasks: [childTask({ title: "a" }), childTask({ title: "b" }), childTask({ title: "c" })] }),
    parentTaskId: "p",
    importBatchId: "b",
    baseOrder: 100,
  });
  assert.deepEqual(payloads.map((p) => p.order), [110, 120, 130]);
  assert.deepEqual(payloads.map((p) => p.title), ["a", "b", "c"], "現在の表示順を維持");
});

test("子タスク: 作業内容11項目を保持する", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.equal(p.title, "子タスク");
  assert.equal(p.purpose, "目的");
  assert.equal(p.splitReason, "分割理由");
  assert.deepEqual(p.scope, ["対象1"]);
  assert.deepEqual(p.outOfScope, ["非対象1"]);
  assert.deepEqual(p.doneWhen, ["完了1"]);
  assert.deepEqual(p.notes, ["メモ1"]);
  assert.equal(p.implementationPrompt, "実装");
  assert.deepEqual(p.reviewPoints, ["観点1"]);
  assert.equal(p.reviewPrompt, "レビュー依頼");
  assert.deepEqual(p.verificationCommands, ["pnpm lint"]);
});

test("子タスク: 継承4項目を保持する", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.equal(p.category, "タスク管理機能");
  assert.equal(p.subcategory, "AI分割タスク取込");
  assert.equal(p.priority, "P1");
  assert.equal(p.owner, "近松");
});

test("子タスク: 親の MVP区分を継承する（正式値・別名・不正値・未設定）", () => {
  // 親 Required → 子 Required。
  const [req] = buildAiSubtaskChildPayloads({
    snapshot: snapshot({ inheritedValues: { category: "c", priority: "P1", mvpScope: "Required" } }),
    parentTaskId: "p",
    importBatchId: "b",
    baseOrder: 0,
  });
  assert.equal(req.mvpScope, "Required");
  // 親 Additional（日本語別名）→ 子 Additional。
  const [add] = buildAiSubtaskChildPayloads({
    snapshot: snapshot({ inheritedValues: { category: "c", mvpScope: "追加機能" } }),
    parentTaskId: "p",
    importBatchId: "b",
    baseOrder: 0,
  });
  assert.equal(add.mvpScope, "Additional");
  // 親 不正値 → 子 Undecided（安全側）。
  const [bad] = buildAiSubtaskChildPayloads({
    snapshot: snapshot({ inheritedValues: { category: "c", mvpScope: "Support" } }),
    parentTaskId: "p",
    importBatchId: "b",
    baseOrder: 0,
  });
  assert.equal(bad.mvpScope, "Undecided");
  // 親 未設定（inheritedValues に mvpScope なし）→ 子 Undecided。
  const [none] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.equal(none.mvpScope, "Undecided");
});

test("子タスク: implementationPrompt / reviewPrompt の改行を保持する", () => {
  const multi = "1行目\n2行目\n\n4行目";
  const [p] = buildAiSubtaskChildPayloads({
    snapshot: snapshot({ tasks: [childTask({ implementationPrompt: multi, reviewPrompt: multi })] }),
    parentTaskId: "p",
    importBatchId: "b",
    baseOrder: 0,
  });
  assert.equal(p.implementationPrompt, multi);
  assert.equal(p.reviewPrompt, multi);
});

test("子タスク: status は必ず Todo", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.equal(p.status, "Todo");
});

test("子タスク: source は必ず ai-subtask-import", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.equal(p.source, "ai-subtask-import");
});

test("子タスク: branchName/completedAt/issuePr/sourceLine が null", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.equal(p.branchName, null);
  assert.equal(p.completedAt, null);
  assert.equal(p.issuePr, null);
  assert.equal(p.sourceLine, null);
});

test("子タスク: completed/archived/protected が false", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.equal(p.completed, false);
  assert.equal(p.archived, false);
  assert.equal(p.protected, false);
});

test("子タスク: parentTaskId は固定親の docId を設定・taskCode は空", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "parent-xyz", importBatchId: "b", baseOrder: 0 });
  assert.equal(p.parentTaskId, "parent-xyz");
  assert.equal(p.taskCode, "");
});

test("子タスク: UI専用の previewId/included を含めない", () => {
  const [p] = buildAiSubtaskChildPayloads({
    snapshot: snapshot({ tasks: [childTask()] }),
    parentTaskId: "p",
    importBatchId: "b",
    baseOrder: 0,
  });
  assert.ok(!("previewId" in p));
  assert.ok(!("included" in p));
});

test("子タスク: 親専用の autoStatusUpdateDisabled/taskRole/splitChildCount を含めない", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.ok(!("autoStatusUpdateDisabled" in p));
  assert.ok(!("taskRole" in p));
  assert.ok(!("splitChildCount" in p));
});

test("子タスク: createdAt/updatedAt は含めない（書き込み層で serverTimestamp を付与）", () => {
  const [p] = buildAiSubtaskChildPayloads({ snapshot: snapshot(), parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.ok(!("createdAt" in p));
  assert.ok(!("updatedAt" in p));
  assert.equal(p.updatedBy, "ai-subtask-import");
});

test("子タスク: 元 snapshot を破壊しない", () => {
  const s = snapshot();
  const snap = JSON.parse(JSON.stringify(s));
  buildAiSubtaskChildPayloads({ snapshot: s, parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.deepEqual(s, snap);
});

test("子タスク: 配列を参照共有しない", () => {
  const s = snapshot();
  const [p] = buildAiSubtaskChildPayloads({ snapshot: s, parentTaskId: "p", importBatchId: "b", baseOrder: 0 });
  assert.notEqual(p.scope, s.tasks[0].scope, "scope は別配列");
  assert.deepEqual(p.scope, s.tasks[0].scope);
});

// --- 親データの登録可否 ---

test("親可否: Todo / Doing / Blocked を許可", () => {
  for (const status of ["Todo", "Doing", "Blocked"]) {
    assert.equal(validateAiSubtaskRegistrationParent({ status }).ok, true, `${status} は許可`);
  }
});

test("親可否: Review を拒否", () => {
  const r = validateAiSubtaskRegistrationParent({ status: "Review" });
  assert.equal(r.ok, false);
  assert.match(r.reason, /Review/);
});

test("親可否: Done を拒否", () => {
  assert.equal(validateAiSubtaskRegistrationParent({ status: "Done" }).ok, false);
});

test("親可否: archived=true を拒否", () => {
  assert.equal(validateAiSubtaskRegistrationParent({ status: "Doing", archived: true }).ok, false);
});

test("親可否: completed=true を拒否", () => {
  assert.equal(validateAiSubtaskRegistrationParent({ status: "Doing", completed: true }).ok, false);
});

test('親可否: taskRole="split-parent" を拒否', () => {
  const r = validateAiSubtaskRegistrationParent({ status: "Doing", taskRole: "split-parent" });
  assert.equal(r.ok, false);
  assert.match(r.reason, /分割済み/);
});

test("親可否: splitChildCount > 0 を拒否", () => {
  assert.equal(validateAiSubtaskRegistrationParent({ status: "Doing", splitChildCount: 2 }).ok, false);
});

test("親可否: 未存在（null）を拒否", () => {
  const r = validateAiSubtaskRegistrationParent(null);
  assert.equal(r.ok, false);
  assert.match(r.reason, /見つかりません/);
});

test("親可否: splitChildCount 未設定 / 0 は許可", () => {
  assert.equal(validateAiSubtaskRegistrationParent({ status: "Doing" }).ok, true);
  assert.equal(validateAiSubtaskRegistrationParent({ status: "Doing", splitChildCount: 0 }).ok, true);
});

// --- 親更新値 ---

test("親更新値: 正しい管理フィールド（status/branchName/issuePr は含めない）", () => {
  const upd = buildAiSubtaskParentUpdate(3);
  assert.equal(upd.autoStatusUpdateDisabled, true);
  assert.equal(upd.taskRole, "split-parent");
  assert.equal(upd.splitChildCount, 3);
  assert.equal(upd.updatedBy, "ai-subtask-import");
  for (const forbidden of ["status", "branchName", "issuePr", "completed", "category", "subcategory", "priority", "owner"]) {
    assert.ok(!(forbidden in upd), `${forbidden} を更新値へ含めない`);
  }
  // createdAt/updatedAt は書き込み層で付与するため含めない。
  assert.ok(!("updatedAt" in upd));
});
