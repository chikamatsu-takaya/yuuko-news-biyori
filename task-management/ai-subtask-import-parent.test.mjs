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
  isAlreadySplitParent,
  classifyAiSubtaskParentState,
  classifyParentTaskIdField,
  hasNonEmptyParentTaskId,
  computeAiSubtaskUiFlags,
  formatParentTaskLabel,
  shouldWarnSplitParentAutoUpdate,
  resolveEligibleParentForModal,
  AI_SUBTASK_PARENT_ELIGIBLE_STATUSES,
} from "./ai-subtask-import-parent.mjs";

// 整合した分割済み親（taskRole / autoStatusUpdateDisabled / 正の安全整数 splitChildCount が揃う）。
function splitParent(overrides = {}) {
  return parentTask({
    taskRole: "split-parent",
    autoStatusUpdateDisabled: true,
    splitChildCount: 3,
    ...overrides,
  });
}

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

// --- 整合した分割済み親は候補になる（既存子を保ったまま追加登録できる） ---

test("整合した分割済み親（3フィールド揃い）は候補になる（追加登録可）", () => {
  assert.equal(isEligibleAiSubtaskParent(splitParent()), true);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ status: "Doing" })), true);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ status: "Blocked" })), true);
});

test("整合した分割済み親でも archived / completed / status 条件は維持される", () => {
  assert.equal(isEligibleAiSubtaskParent(splitParent({ archived: true })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ status: "Done" })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ status: "Review" })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ completed: true })), false);
});

test("完全な未分割（splitChildCount 未設定/0・管理フィールドなし）は候補になる", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ splitChildCount: 0 })), true);
  const t = parentTask();
  delete t.splitChildCount;
  assert.equal(isEligibleAiSubtaskParent(t), true);
});

// --- 不整合な分割管理フィールドは候補にならない（P1-2） ---

test("不整合な分割済み親は候補にならない（片方だけ・0/負/小数/文字列/NaN/Infinity 等）", () => {
  // taskRole のみ / autoStatusUpdateDisabled のみ / splitChildCount のみ
  assert.equal(isEligibleAiSubtaskParent(parentTask({ taskRole: "split-parent" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ autoStatusUpdateDisabled: true })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ splitChildCount: 3 })), false);
  // 3フィールド揃いだが splitChildCount が不正
  assert.equal(isEligibleAiSubtaskParent(splitParent({ splitChildCount: 0 })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ splitChildCount: -1 })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ splitChildCount: 1.5 })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ splitChildCount: "3" })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ splitChildCount: NaN })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ splitChildCount: Infinity })), false);
  // taskRole が別値 / autoStatusUpdateDisabled=false
  assert.equal(isEligibleAiSubtaskParent(splitParent({ taskRole: "child" })), false);
  assert.equal(isEligibleAiSubtaskParent(splitParent({ autoStatusUpdateDisabled: false })), false);
});

// --- parentTaskId を持つ子タスクは候補にならない（P1-1・孫タスク化防止） ---

test("parentTaskId を持つ子タスクは候補にならない（Todo/Doing/Blocked いずれも）", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Todo", parentTaskId: "p1" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Doing", parentTaskId: "p1" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Blocked", parentTaskId: "p1" })), false);
  // 空白のみは trim 後に空＝親なし扱いで候補になる。
  assert.equal(isEligibleAiSubtaskParent(parentTask({ parentTaskId: "   " })), true);
  // parentTaskId と分割親管理フィールドの併存は inconsistent（P3）。いずれにせよ候補外。
  assert.equal(isEligibleAiSubtaskParent(splitParent({ parentTaskId: "p1" })), false);
});

test("parentTaskId 未設定 / null / 空文字 / 空白文字列は「親なし」として候補になる", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ parentTaskId: null })), true);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ parentTaskId: "" })), true);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ parentTaskId: "   " })), true);
  const t = parentTask();
  delete t.parentTaskId;
  assert.equal(isEligibleAiSubtaskParent(t), true);
});

test("非文字列 parentTaskId（型不整合）は候補にならない（P2-1・空文字へ補正しない）", () => {
  for (const bad of [123, true, [], {}, ["parent-id"]]) {
    assert.equal(
      isEligibleAiSubtaskParent(parentTask({ status: "Todo", parentTaskId: bad })),
      false,
      `parentTaskId=${JSON.stringify(bad)} は候補外`,
    );
    // Doing / Blocked でも同様。
    assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Doing", parentTaskId: bad })), false);
    assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Blocked", parentTaskId: bad })), false);
  }
});

// --- classifyParentTaskIdField（absent / child / invalid） ---

test("classifyParentTaskIdField: absent（未設定・undefined・null・空文字・空白文字列）", () => {
  assert.equal(classifyParentTaskIdField(undefined), "absent");
  assert.equal(classifyParentTaskIdField(null), "absent");
  assert.equal(classifyParentTaskIdField(""), "absent");
  assert.equal(classifyParentTaskIdField("   "), "absent");
});

test("classifyParentTaskIdField: child（trim後の非空文字列）", () => {
  assert.equal(classifyParentTaskIdField("p1"), "child");
  assert.equal(classifyParentTaskIdField(" p1 "), "child");
});

test("classifyParentTaskIdField: invalid（null以外の非文字列）", () => {
  for (const bad of [123, 0, true, false, [], {}, ["parent-id"], NaN]) {
    assert.equal(classifyParentTaskIdField(bad), "invalid", `${JSON.stringify(bad)} は invalid`);
  }
});

// --- classifyAiSubtaskParentState（分類の正本） ---

test("classifyAiSubtaskParentState: child（parentTaskId あり・分割管理フィールドは未設定側のみ）", () => {
  // parentTaskId だけの通常の子タスク。
  assert.deepEqual(classifyAiSubtaskParentState({ parentTaskId: "p1" }), {
    state: "child",
    existingChildCount: 0,
  });
  // 未分割側のデフォルト値（空文字 / false / 0）だけなら child。
  assert.deepEqual(
    classifyAiSubtaskParentState({ parentTaskId: "p1", taskRole: "", autoStatusUpdateDisabled: false, splitChildCount: 0 }),
    { state: "child", existingChildCount: 0 },
  );
});

test("classifyAiSubtaskParentState: parentTaskId と分割親管理フィールドの併存は inconsistent（child にしない・P3）", () => {
  const coexist = [
    // 整合した split-parent 情報を併せ持つ（要修復）。
    splitParent({ parentTaskId: "p1" }),
    { parentTaskId: "p1", taskRole: "split-parent" },
    { parentTaskId: "p1", autoStatusUpdateDisabled: true },
    { parentTaskId: "p1", splitChildCount: 2 },
    // 型不整合の分割管理フィールドを併せ持つ。
    { parentTaskId: "p1", splitChildCount: "2" },
    { parentTaskId: "p1", taskRole: 123 },
  ];
  for (const data of coexist) {
    const r = classifyAiSubtaskParentState(data);
    assert.equal(r.state, "inconsistent", `${JSON.stringify(data)} は inconsistent`);
    assert.equal(r.existingChildCount, 0);
  }
});

test("classifyAiSubtaskParentState: unsplit（既存子数0）", () => {
  assert.deepEqual(classifyAiSubtaskParentState({}), { state: "unsplit", existingChildCount: 0 });
  assert.deepEqual(classifyAiSubtaskParentState({ splitChildCount: 0 }), {
    state: "unsplit",
    existingChildCount: 0,
  });
  assert.equal(classifyAiSubtaskParentState({ splitChildCount: null }).state, "unsplit");
  // autoStatusUpdateDisabled=false は未分割側。
  assert.equal(classifyAiSubtaskParentState({ autoStatusUpdateDisabled: false, taskRole: "" }).state, "unsplit");
});

test("classifyAiSubtaskParentState: split-parent（既存子数=splitChildCount）", () => {
  assert.deepEqual(
    classifyAiSubtaskParentState({ taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: 3 }),
    { state: "split-parent", existingChildCount: 3 },
  );
  // 上限近辺の安全整数も split-parent。
  assert.equal(
    classifyAiSubtaskParentState({
      taskRole: "split-parent",
      autoStatusUpdateDisabled: true,
      splitChildCount: Number.MAX_SAFE_INTEGER,
    }).existingChildCount,
    Number.MAX_SAFE_INTEGER,
  );
});

test("classifyAiSubtaskParentState: inconsistent（混在・不正値）", () => {
  const inconsistent = [
    { taskRole: "split-parent" },
    { autoStatusUpdateDisabled: true },
    { splitChildCount: 3 },
    { taskRole: "split-parent", autoStatusUpdateDisabled: true }, // count 欠落
    { taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: 0 },
    { taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: -1 },
    { taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: 1.5 },
    { taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: "3" },
    { taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: NaN },
    { taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: Infinity },
    { taskRole: "foo", autoStatusUpdateDisabled: true, splitChildCount: 3 },
    { taskRole: "split-parent", autoStatusUpdateDisabled: false, splitChildCount: 3 },
    // P2-1: 非文字列 parentTaskId（型不整合）。空文字へ補正せず inconsistent。
    { status: "Todo", parentTaskId: 123 },
    { status: "Todo", parentTaskId: true },
    { status: "Todo", parentTaskId: [] },
    { status: "Todo", parentTaskId: {} },
    { status: "Todo", parentTaskId: ["parent-id"] },
    // P2-2: 非文字列 taskRole（型不整合）。空文字へ補正せず inconsistent。
    { status: "Todo", taskRole: 123 },
    { status: "Todo", taskRole: true },
    // 型不整合 autoStatusUpdateDisabled（"true"/1 は false へ補正せず inconsistent）。
    { status: "Todo", autoStatusUpdateDisabled: "true" },
    { status: "Todo", autoStatusUpdateDisabled: 1 },
  ];
  for (const data of inconsistent) {
    const r = classifyAiSubtaskParentState(data);
    assert.equal(r.state, "inconsistent", `${JSON.stringify(data)} は inconsistent`);
    assert.equal(r.existingChildCount, 0);
  }
});

test("hasNonEmptyParentTaskId: trim後の非空文字列のみ true（非文字列は child ではない=false）", () => {
  assert.equal(hasNonEmptyParentTaskId({ parentTaskId: "p1" }), true);
  assert.equal(hasNonEmptyParentTaskId({ parentTaskId: " p1 " }), true);
  assert.equal(hasNonEmptyParentTaskId({ parentTaskId: "" }), false);
  assert.equal(hasNonEmptyParentTaskId({ parentTaskId: "   " }), false);
  assert.equal(hasNonEmptyParentTaskId({ parentTaskId: null }), false);
  // 非文字列は child ではない（false）が、分類上は invalid＝inconsistent 扱い（classifyParentTaskIdField 参照）。
  assert.equal(hasNonEmptyParentTaskId({ parentTaskId: 123 }), false);
  assert.equal(hasNonEmptyParentTaskId({}), false);
  assert.equal(hasNonEmptyParentTaskId(null), false);
});

// --- isAlreadySplitParent（候補可否には使わず、ボタン文言などの表示切替に使う） ---

test("isAlreadySplitParent: 整合した分割済み親のみ true", () => {
  assert.equal(isAlreadySplitParent(splitParent()), true);
  // 不整合・未分割・child は false。
  assert.equal(isAlreadySplitParent(parentTask({ taskRole: "split-parent" })), false);
  assert.equal(isAlreadySplitParent(parentTask({ splitChildCount: 2 })), false);
  assert.equal(isAlreadySplitParent(parentTask({ splitChildCount: 0 })), false);
  assert.equal(isAlreadySplitParent(splitParent({ parentTaskId: "p1" })), false);
  const t = parentTask();
  delete t.splitChildCount;
  assert.equal(isAlreadySplitParent(t), false);
  assert.equal(isAlreadySplitParent(null), false);
});

// --- computeAiSubtaskUiFlags（生データからのUIフラグ・P2-2） ---
// firestore-source.js は正規化前の生データをこの関数へ渡し、結果を画面モデルへ設定する。

test("computeAiSubtaskUiFlags: 型不整合の生データは eligible=false / alreadySplit=false", () => {
  // 表示用正規化（"3"→0・123→""）で unsplit へ潰れず、候補外になること。
  assert.deepEqual(
    computeAiSubtaskUiFlags({ status: "Doing", taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: "3" }),
    { eligible: false, alreadySplit: false },
  );
  assert.deepEqual(
    computeAiSubtaskUiFlags({ status: "Doing", taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: Infinity }),
    { eligible: false, alreadySplit: false },
  );
  assert.deepEqual(computeAiSubtaskUiFlags({ status: "Todo", parentTaskId: 123 }), { eligible: false, alreadySplit: false });
  assert.deepEqual(computeAiSubtaskUiFlags({ status: "Todo", parentTaskId: [] }), { eligible: false, alreadySplit: false });
  assert.deepEqual(computeAiSubtaskUiFlags({ status: "Todo", parentTaskId: {} }), { eligible: false, alreadySplit: false });
  assert.deepEqual(computeAiSubtaskUiFlags({ status: "Todo", taskRole: 123 }), { eligible: false, alreadySplit: false });
});

test("computeAiSubtaskUiFlags: 整合した分割済み親は eligible=true / alreadySplit=true", () => {
  assert.deepEqual(
    computeAiSubtaskUiFlags({ status: "Doing", taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: 2 }),
    { eligible: true, alreadySplit: true },
  );
});

test("computeAiSubtaskUiFlags: 完全な未分割は eligible=true / alreadySplit=false", () => {
  assert.deepEqual(computeAiSubtaskUiFlags({ status: "Todo" }), { eligible: true, alreadySplit: false });
  assert.deepEqual(computeAiSubtaskUiFlags({ status: "Blocked", splitChildCount: 0 }), { eligible: true, alreadySplit: false });
});

test("computeAiSubtaskUiFlags: Review / Done / completed / archived は eligible=false", () => {
  assert.equal(computeAiSubtaskUiFlags({ status: "Review" }).eligible, false);
  assert.equal(computeAiSubtaskUiFlags({ status: "Done" }).eligible, false);
  assert.equal(computeAiSubtaskUiFlags({ status: "Todo", completed: true }).eligible, false);
  assert.equal(computeAiSubtaskUiFlags({ status: "Todo", archived: true }).eligible, false);
});

test("computeAiSubtaskUiFlags: 生データが不整合なら正規化後の0/空文字を理由に eligible=true にならない（UI判定＝登録時判定）", () => {
  // 生データ（型情報あり）と、表示用に正規化した値の両方で分類が一致することを確認する。
  const raw = { status: "Doing", taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: "5" };
  // UI候補判定（生データ）＝false。
  assert.equal(computeAiSubtaskUiFlags(raw).eligible, false);
  // 一方、表示用に正規化（splitChildCount:"5"→0 等）した値だと unsplit へ潰れて誤って候補になりうる。
  const normalized = { status: "Doing", taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: 0 };
  assert.equal(classifyAiSubtaskParentState(normalized).state, "inconsistent"); // 0 でも role/flag と食い違い→inconsistent
  // どちらの入力でも eligible=true にはならない（不整合を隠さない）。
  assert.equal(isEligibleAiSubtaskParent(normalized), false);
});

// --- 配列版・非破壊 ---

test("getEligibleAiSubtaskParentTasks: 候補のみ返し、元配列を破壊しない", () => {
  const tasks = [
    parentTask({ firestoreId: "a", status: "Todo" }),
    parentTask({ firestoreId: "b", status: "Review" }),
    parentTask({ firestoreId: "c", status: "Doing" }),
    parentTask({ firestoreId: "d", status: "Done" }),
    // 整合した分割済み親（Blocked）も候補に含める（追加登録できる）。
    splitParent({ firestoreId: "e", status: "Blocked", splitChildCount: 2 }),
    parentTask({ firestoreId: "f", status: "Blocked" }),
    // 子タスク（parentTaskId あり・Todo）は候補にしない（孫タスク化防止）。
    parentTask({ firestoreId: "g", status: "Todo", parentTaskId: "e" }),
    // 不整合な分割管理（taskRole のみ）は候補にしない。
    parentTask({ firestoreId: "h", status: "Doing", taskRole: "split-parent" }),
  ];
  const snapshot = JSON.parse(JSON.stringify(tasks));
  const eligible = getEligibleAiSubtaskParentTasks(tasks);
  assert.deepEqual(eligible.map((t) => t.firestoreId), ["a", "c", "e", "f"]);
  // 元配列・要素は変更されない。
  assert.deepEqual(tasks, snapshot);
  assert.equal(tasks.length, 8);
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

test("ボタン表示判定: Review / Done / completed / archived は表示対象外（false）", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Review" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ status: "Done" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ completed: true })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ archived: true })), false);
});

test("ボタン表示判定: 整合した分割済み親も表示対象（true・文言は「子タスクを追加」に切り替える）", () => {
  assert.equal(isEligibleAiSubtaskParent(splitParent()), true);
  assert.equal(isAlreadySplitParent(splitParent()), true);
});

test("ボタン表示判定: 不整合な分割管理 / 子タスクは表示対象外（false）", () => {
  assert.equal(isEligibleAiSubtaskParent(parentTask({ taskRole: "split-parent" })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ autoStatusUpdateDisabled: true })), false);
  assert.equal(isEligibleAiSubtaskParent(parentTask({ parentTaskId: "p1" })), false);
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

test("再判定: 再読込で整合した分割済み親ならモーダルを開く（追加登録可）", async () => {
  const fresh = splitParent({ splitChildCount: 3 });
  const { fetchTaskById } = makeFetch(fresh);
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, true);
  assert.equal(r.task, fresh);
});

test("再判定: 再読込で不整合な分割管理（片方だけ）はモーダルを開かない", async () => {
  const { fetchTaskById } = makeFetch(parentTask({ splitChildCount: 5 }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test("再判定: 再読込で parentTaskId を持つ子タスクはモーダルを開かない（孫タスク化防止）", async () => {
  const { fetchTaskById } = makeFetch(splitParent({ parentTaskId: "p1" }));
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
});

test("再判定: 分割済みでも Done へ変わっていればモーダルを開かない（status 条件は維持）", async () => {
  const { fetchTaskById } = makeFetch(splitParent({ status: "Done" }));
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

// --- resolveEligibleParentForModal: 生データ由来 aiSubtaskEligible（boolean）を正本にする（P2） ---
// fetchFirestoreTaskById は正規化済みモデルを返すが、そのモデルには生データから計算済みの
// aiSubtaskEligible / aiSubtaskAlreadySplit が付く。型不整合は表示用に正規化（"3"→0・123→""）されて
// 隠れるため、モーダル直前判定は正規化後モデルの再分類ではなく aiSubtaskEligible（boolean）を尊重する。

// 正規化後モデル（firestoreDocToTaskModel 相当・生データの型不整合は既に丸められている）。
function normalizedModel(overrides = {}) {
  return {
    firestoreId: "t1",
    status: "Todo",
    completed: false,
    archived: false,
    parentTaskId: "", // 生データが非文字列でも表示用は空文字
    taskRole: "", // 生データが非文字列でも表示用は空文字
    splitChildCount: 0, // 生データが "3"/Infinity でも表示用は 0
    autoStatusUpdateDisabled: false,
    ...overrides,
  };
}

test("再判定: aiSubtaskEligible=false の最新モデルはモーダルを開かない（正規化後は未分割に見えても尊重）", async () => {
  const fresh = normalizedModel({ aiSubtaskEligible: false, aiSubtaskAlreadySplit: false });
  const { fetchTaskById } = makeFetch(fresh);
  const r = await resolveEligibleParentForModal("t1", fetchTaskById);
  assert.equal(r.ok, false);
  assert.equal(r.reason, "ineligible");
  assert.equal(r.task, fresh);
});

test("再判定: 生データの型不整合を表す各正規化モデル（aiSubtaskEligible=false）はモーダルを開かない", async () => {
  // それぞれ「生データが型不整合だったが表示用に丸められた」状態を表す。正規化後フィールドだけでは
  // 元の型不整合を再現できないため、aiSubtaskEligible=false を尊重して開かないことを確認する。
  const cases = [
    // 生データ splitChildCount:"3" → 表示用 0（role/flag は split 側）
    normalizedModel({ status: "Doing", taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: 0, aiSubtaskEligible: false }),
    // 生データ splitChildCount:Infinity → 表示用 0
    normalizedModel({ status: "Doing", taskRole: "split-parent", autoStatusUpdateDisabled: true, splitChildCount: 0, aiSubtaskEligible: false }),
    // 生データ parentTaskId:123 → 表示用 ""（親なしに見える）
    normalizedModel({ parentTaskId: "", aiSubtaskEligible: false }),
    // 生データ taskRole:123 → 表示用 ""（未分割に見える）
    normalizedModel({ taskRole: "", aiSubtaskEligible: false }),
    // 生データ autoStatusUpdateDisabled:"true" → 表示用 false
    normalizedModel({ autoStatusUpdateDisabled: false, aiSubtaskEligible: false }),
  ];
  for (const fresh of cases) {
    const { fetchTaskById } = makeFetch(fresh);
    const r = await resolveEligibleParentForModal("t1", fetchTaskById);
    assert.equal(r.ok, false, `${JSON.stringify(fresh)} は開かない`);
    assert.equal(r.reason, "ineligible");
  }
});

test("再判定: aiSubtaskEligible=true の最新モデルはモーダルを開く（生データ判定を尊重）", async () => {
  const unsplit = normalizedModel({ status: "Doing", aiSubtaskEligible: true, aiSubtaskAlreadySplit: false });
  const r1 = await resolveEligibleParentForModal("t1", makeFetch(unsplit).fetchTaskById);
  assert.equal(r1.ok, true);
  assert.equal(r1.task, unsplit);
  // 整合した分割済み親（aiSubtaskAlreadySplit=true）も開ける。フラグはそのまま保持される。
  const split = normalizedModel({
    status: "Doing",
    taskRole: "split-parent",
    autoStatusUpdateDisabled: true,
    splitChildCount: 2,
    aiSubtaskEligible: true,
    aiSubtaskAlreadySplit: true,
  });
  const r2 = await resolveEligibleParentForModal("t1", makeFetch(split).fetchTaskById);
  assert.equal(r2.ok, true);
  assert.equal(r2.task.aiSubtaskAlreadySplit, true);
});

test("再判定: aiSubtaskEligible 未設定はフォールバック（isEligibleAiSubtaskParent で判定）", async () => {
  // 正常な未分割・整合分割済みは許可、child / inconsistent は拒否（フラグを持たない呼び出し元・既存テスト）。
  assert.equal((await resolveEligibleParentForModal("t1", makeFetch(parentTask({ status: "Todo" })).fetchTaskById)).ok, true);
  assert.equal((await resolveEligibleParentForModal("t1", makeFetch(splitParent()).fetchTaskById)).ok, true);
  const child = await resolveEligibleParentForModal("t1", makeFetch(parentTask({ parentTaskId: "p1" })).fetchTaskById);
  assert.equal(child.ok, false);
  assert.equal(child.reason, "ineligible");
  const inconsistent = await resolveEligibleParentForModal("t1", makeFetch(parentTask({ taskRole: "split-parent" })).fetchTaskById);
  assert.equal(inconsistent.ok, false);
});

test("再判定: aiSubtaskEligible が boolean 以外なら正本にせずフォールバックする", async () => {
  // "false" / 0 / 1 / null は typeof !== "boolean" なので分類へフォールバック（falsy/truthy 判定にしない）。
  // 下記モデルは分類上は候補（未分割 Todo）なので、フォールバック結果は ok=true になる。
  for (const flag of ["false", 0, 1, null, undefined]) {
    const fresh = normalizedModel({ status: "Todo", aiSubtaskEligible: flag });
    const r = await resolveEligibleParentForModal("t1", makeFetch(fresh).fetchTaskById);
    assert.equal(r.ok, true, `aiSubtaskEligible=${JSON.stringify(flag)} はフォールバックで候補判定（ok=true）`);
  }
  // 逆に、boolean 以外でも分類上 child なら拒否される（フォールバックが効いている証拠）。
  const childFallback = await resolveEligibleParentForModal(
    "t1",
    makeFetch(normalizedModel({ parentTaskId: "p1", aiSubtaskEligible: 1 })).fetchTaskById,
  );
  assert.equal(childFallback.ok, false);
});
