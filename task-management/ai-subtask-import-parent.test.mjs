// ai-subtask-import-parent.mjs の単体テスト（node:test）。
//
// 範囲（docs/00_project/ai-subtask-import-spec.md §3.10 / §8）:
// - 親候補の判定（status / completed / archived / taskRole / splitChildCount）。
// - autoStatusUpdateDisabled は候補判定に使わないこと。
// - 入力配列を破壊しないこと。
// - 表示ラベル（taskCode 未設定でも不自然にならない）と Doing+branchName 注意の要否。
//
// 実行: node --test task-management/ai-subtask-import-parent.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  getEligibleAiSubtaskParentTasks,
  isEligibleAiSubtaskParent,
  formatParentTaskLabel,
  shouldWarnSplitParentAutoUpdate,
  resolveEligibleParentForModal,
  AI_SUBTASK_PARENT_ELIGIBLE_STATUSES,
} from "./ai-subtask-import-parent.mjs";

// 画面用モデル相当の親タスク（既定は候補になる Todo）。
function parentTask(overrides = {}) {
  return {
    firestoreId: "t1",
    taskCode: "TASK-1",
    text: "Firestoreタスク連携を実装する",
    status: "Todo",
    completed: false,
    branch: "",
    taskRole: "",
    splitChildCount: 0,
    ...overrides,
  };
}

// --- status ---

test("Todo は候補になる", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Todo" })), true);
});

test("Doing は候補になる", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Doing" })), true);
});

test("Blocked は候補になる", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Blocked" })), true);
});

test("Review は候補にならない", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Review" })), false);
});

test("Done は候補にならない", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Done" })), false);
});

test("Next / 空 status は候補にならない（Todo/Doing/Blocked のみ）", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Next" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "" })), false);
});

// --- completed / archived ---

test("completed=true は候補にならない", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ completed: true })), false);
});

test("archived=true は候補にならない", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ archived: true })), false);
});

// --- 分割済み親（taskRole / splitChildCount） ---

test('taskRole="split-parent" は候補にならない', () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ taskRole: "split-parent" })), false);
});

test("splitChildCount > 0 は候補にならない", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ splitChildCount: 3 })), false);
});

test("splitChildCount 未設定は候補になる", () => {
  const t = parentTask();
  delete t.splitChildCount;
  assert.equal(isEligibleAiSubtaskParent(t), true);
});

test("splitChildCount = 0 は候補になる", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ splitChildCount: 0 })), true);
});

// --- autoStatusUpdateDisabled は候補判定に使わない ---

test("autoStatusUpdateDisabled=true だけでは候補判定条件を変えない（Todo なら候補のまま）", () => {
  // 分割済み判定は taskRole / splitChildCount のみ。autoStatusUpdateDisabled は無関係。
  assert.equal(isEligibleAiSubtaskParent(parentTask({ autoStatusUpdateDisabled: true })), true);
});

// --- 配列版・非破壊 ---

test("getEligibleAiSubtaskParentTasks: 候補のみ返し、元配列を破壊しない", () => {
  const tasks = [
    parentTask({ firestoreId: "a", status: "Todo" }),
    parentTask({ firestoreId: "b", status: "Review" }),
    parentTask({ firestoreId: "c", status: "Doing" }),
    parentTask({ firestoreId: "d", status: "Done" }),
    parentTask({ firestoreId: "e", status: "Blocked", taskRole: "split-parent" }),
    parentTask({ firestoreId: "f", status: "Blocked" }),
  ];
  const snapshot = JSON.parse(JSON.stringify(tasks));
  const eligible = getEligibleAiSubtaskParentTasks(tasks);
  assert.deepEqual(eligible.map((t) => t.firestoreId), ["a", "c", "f"]);
  // 元配列・要素は変更されない。
  assert.deepEqual(tasks, snapshot);
  assert.equal(tasks.length, 6);
});

test("getEligibleAiSubtaskParentTasks: 配列以外は空配列", () => {
  assert.deepEqual(getEligibleAiSubtaskParentTasks(null), []);
  assert.deepEqual(getEligibleAiSubtaskParentTasks(undefined), []);
  assert.deepEqual(getEligibleAiSubtaskParentTasks({}), []);
});

// --- 表示ラベル ---

test("formatParentTaskLabel: taskCode ありは 'CODE｜title'", () => {
  assert.equal(
    formatParentTaskLabel(parentTask({ taskCode: "TASK-123", text: "連携を実装する" })),
    "TASK-123｜連携を実装する",
  );
});

test("formatParentTaskLabel: taskCode 未設定なら区切り記号を出さず title だけ", () => {
  assert.equal(formatParentTaskLabel(parentTask({ taskCode: "", text: "タイトルのみ" })), "タイトルのみ");
  const noCode = parentTask({ text: "タイトルのみ" });
  delete noCode.taskCode;
  assert.equal(formatParentTaskLabel(noCode), "タイトルのみ");
});

// --- Doing + branchName 注意 ---

test("shouldWarnSplitParentAutoUpdate: Doing かつ branch 設定済みで true", () => {
  assert.equal(shouldWarnSplitParentAutoUpdate(parentTask({ status: "Doing", branch: "feature/x" })), true);
});

test("shouldWarnSplitParentAutoUpdate: Doing でも branch 未設定なら false", () => {
  assert.equal(shouldWarnSplitParentAutoUpdate(parentTask({ status: "Doing", branch: "" })), false);
});

test("shouldWarnSplitParentAutoUpdate: Doing 以外なら false", () => {
  assert.equal(shouldWarnSplitParentAutoUpdate(parentTask({ status: "Todo", branch: "feature/x" })), false);
});

test("AI_SUBTASK_PARENT_ELIGIBLE_STATUSES は Todo/Doing/Blocked", () => {
  assert.deepEqual([...AI_SUBTASK_PARENT_ELIGIBLE_STATUSES], ["Todo", "Doing", "Blocked"]);
});

// --- 「AIで分割」ボタンの表示判定（カード描画は task.aiSubtaskEligible = isEligibleAiSubtaskParent(task) を使う） ---

test("ボタン表示判定: 候補タスクは表示対象（true）", () => {
  // 各カードは isEligibleAiSubtaskParent の結果でボタン表示を決める。候補は表示できる。
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Todo" })), true);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Doing" })), true);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Blocked" })), true);
});

test("ボタン表示判定: Review / Done / completed / archived / 分割済み親は表示対象外（false）", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Review" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Done" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ completed: true })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ archived: true })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ taskRole: "split-parent" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ splitChildCount: 2 })), false);
});

test("ボタン表示判定: autoStatusUpdateDisabled=true だけでは表示対象外にしない（Todo なら表示できる）", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ autoStatusUpdateDisabled: true })), true);
});

// --- resolveEligibleParentForModal（最新1件を再取得して候補判定・依存注入でテスト） ---

// 呼び出し記録付きの fetch スタブ（返すタスク or throw を差し替えられる）。
function makeFetch(result) {
  const calls = [];
  const fetchTaskById = async (id) => {
    calls.push(id);
    if (typeof result === "function") return result(id);
    return result;
  };
  return { fetchTaskById, calls };
}

test("再判定: 一覧Todo→再読込Review はモーダルを開かない（ineligible・最新値で判定）", async () => {
  // fetch は最新値（Review）を返す。stale な Todo は使わない。
  const { fetchTaskById } = makeFetch(parentTask({ status: "Review" }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test("再判定: 一覧Doing→再読込Done はモーダルを開かない", async () => {
  const { fetchTaskById } = makeFetch(parentTask({ status: "Done" }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test("再判定: 再読込で archived=true はモーダルを開かない", async () => {
  const { fetchTaskById } = makeFetch(parentTask({ archived: true }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test("再判定: 再読込で completed=true はモーダルを開かない", async () => {
  const { fetchTaskById } = makeFetch(parentTask({ completed: true }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test('再判定: 再読込で taskRole="split-parent" はモーダルを開かない', async () => {
  const { fetchTaskById } = makeFetch(parentTask({ taskRole: "split-parent" }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test("再判定: 再読込で splitChildCount>0 はモーダルを開かない", async () => {
  const { fetchTaskById } = makeFetch(parentTask({ splitChildCount: 5 }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test("再判定: 再読込でも eligible なら再取得した freshTask を親として使う", async () => {
  const fresh = parentTask({ status: "Doing", text: "最新タイトル", branch: "feature/new" });
  const { fetchTaskById, calls } = makeFetch(fresh);
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, true);
  assert.equal(r.task, fresh, "返すのは再取得した最新タスク");
  assert.deepEqual(calls, ["t1"], "対象IDで1件だけ再取得する");
});

test("再判定: 一覧と再読込で title / branchName が変わっていたら最新値を返す", async () => {
  // 一覧時（stale）は古い値でも、resolve は fetch の最新値（fresh）だけを返す。
  const fresh = parentTask({ status: "Doing", text: "新タイトル", branch: "feature/renamed" });
  const { fetchTaskById } = makeFetch(fresh);
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, true);
  assert.equal(r.task.text, "新タイトル");
  assert.equal(r.task.branch, "feature/renamed");
});

test("再判定: 対象ドキュメントが存在しない（null）はモーダルを開かない", async () => {
  const { fetchTaskById } = makeFetch(null);
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "not-found");
});

test("再判定: 再取得エラーは古いデータへフォールバックせず開かない", async () => {
  const fetchTaskById = async () => {
    throw new Error("network error");
  };
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "fetch-error");
  assert.equal(r.task, undefined, "古いデータを task として返さない");
});

test("再判定: taskId 空 / fetch 未指定は invalid（開かない）", async () => {
  assert.equal((await resolveEligibleParentForModal("", async () => null)).reason, "invalid");
  assert.equal((await resolveEligibleParentForModal("t1", null)).reason, "invalid");
});
