// ai-subtask-import-preview.mjs の単体テスト（node:test）。
//
// 範囲（docs/00_project/ai-subtask-import-spec.md §8）:
// - プレビュー状態の作成（複製・全件included・許可11項目・配列非共有・入力非破壊・unknown/system除外）。
// - 編集（文字列/配列項目のみ更新・不明/システム項目は不可・元task非破壊）。
// - 除外/再追加・件数・全件除外判定。
// - 並べ替え（上下・境界no-op・組み合わせ維持・元配列非破壊）。
// - 再検証用データ抽出（included順のみ・UI専用メタ除去・既存バリデータで成功/失敗）。
//
// 実行: node --test task-management/ai-subtask-import-preview.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  createAiSubtaskPreviewState,
  setAiSubtaskPreviewField,
  setAiSubtaskPreviewIncluded,
  moveAiSubtaskPreviewItem,
  countAiSubtaskIncluded,
  countAiSubtaskExcluded,
  isAiSubtaskAllExcluded,
  toAiSubtaskRevalidationInput,
  linesToArray,
  arrayToLines,
  PREVIEW_STRING_FIELDS,
  PREVIEW_ARRAY_FIELDS,
} from "./ai-subtask-import-preview.mjs";
import { ALLOWED_TASK_KEYS, validateAiSubtaskImport } from "./ai-subtask-import-validator.mjs";

// 許可11項目すべてを持つ子タスク。
function fullTask(overrides = {}) {
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

function validatedValue(tasks) {
  return { schemaVersion: 1, splitSummary: "要約", tasks };
}

// --- フィールド区分の整合 ---

test("PREVIEW_STRING_FIELDS ∪ PREVIEW_ARRAY_FIELDS は ALLOWED_TASK_KEYS と一致（正本の分割）", () => {
  const union = [...PREVIEW_STRING_FIELDS, ...PREVIEW_ARRAY_FIELDS].sort();
  assert.deepEqual(union, [...ALLOWED_TASK_KEYS].sort());
  // 重複なし。
  assert.equal(new Set(union).size, union.length);
});

// --- プレビュー状態作成 ---

test("作成: 全件 included=true・schemaVersion/splitSummary を保持", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask(), fullTask({ title: "t2" })]));
  assert.equal(state.schemaVersion, 1);
  assert.equal(state.splitSummary, "要約");
  assert.equal(state.items.length, 2);
  assert.ok(state.items.every((it) => it.included === true));
});

test("作成: previewId が重複しない", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask(), fullTask(), fullTask()]));
  const ids = state.items.map((it) => it.previewId);
  assert.equal(new Set(ids).size, ids.length);
});

test("作成: 許可11項目が保持される", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const task = state.items[0].task;
  for (const key of ALLOWED_TASK_KEYS) {
    assert.ok(key in task, `task に ${key} が含まれる`);
  }
});

test("作成: 配列が参照共有されない（複製される）", () => {
  const src = fullTask();
  const state = createAiSubtaskPreviewState(validatedValue([src]));
  const task = state.items[0].task;
  assert.notEqual(task.scope, src.scope, "scope は別配列");
  assert.deepEqual(task.scope, src.scope, "内容は一致");
  assert.notEqual(task.doneWhen, src.doneWhen);
});

test("作成: validation.value を破壊しない", () => {
  const value = validatedValue([fullTask()]);
  const snapshot = JSON.parse(JSON.stringify(value));
  const state = createAiSubtaskPreviewState(value);
  // state を編集しても value は不変。
  setAiSubtaskPreviewField(state, state.items[0].previewId, "title", "変更");
  assert.deepEqual(value, snapshot);
});

test("作成: unknown key / システムフィールドを取り込まない", () => {
  const dirty = fullTask({ status: "Doing", taskCode: "TASK-1", parentTaskId: "p", foo: "bar", order: 3 });
  const state = createAiSubtaskPreviewState(validatedValue([dirty]));
  const task = state.items[0].task;
  for (const bad of ["status", "taskCode", "parentTaskId", "foo", "order"]) {
    assert.ok(!(bad in task), `${bad} を取り込まない`);
  }
  assert.deepEqual(Object.keys(task).sort(), [...ALLOWED_TASK_KEYS].sort());
});

// --- 編集 ---

test("編集: 文字列項目を更新できる（新しい state を返す・元 state 非破壊）", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const id = state.items[0].previewId;
  const next = setAiSubtaskPreviewField(state, id, "title", "新タイトル");
  assert.equal(next.items[0].task.title, "新タイトル");
  assert.equal(state.items[0].task.title, "子タスク", "元 state は変わらない");
  assert.notEqual(next, state);
});

test("編集: 配列項目を更新できる（複製される）", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const id = state.items[0].previewId;
  const arr = ["a", "b", "c"];
  const next = setAiSubtaskPreviewField(state, id, "scope", arr);
  assert.deepEqual(next.items[0].task.scope, ["a", "b", "c"]);
  assert.notEqual(next.items[0].task.scope, arr, "渡した配列と参照共有しない");
});

test("編集: 不明なフィールドは更新できない（state 非変更）", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const id = state.items[0].previewId;
  const next = setAiSubtaskPreviewField(state, id, "unknownField", "x");
  assert.equal(next, state, "変更なしで同じ state を返す");
  assert.ok(!("unknownField" in next.items[0].task));
});

test("編集: taskCode / status 等のシステム項目を追加できない", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const id = state.items[0].previewId;
  for (const bad of ["taskCode", "status", "parentTaskId", "order", "autoStatusUpdateDisabled"]) {
    const next = setAiSubtaskPreviewField(state, id, bad, "x");
    assert.ok(!(bad in next.items[0].task), `${bad} は追加されない`);
  }
});

test("編集: implementationPrompt / reviewPrompt に改行を含む文字列を設定でき、改行が保持される", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const id = state.items[0].previewId;
  const multi = "1行目\n2行目\n\n4行目";
  let next = setAiSubtaskPreviewField(state, id, "implementationPrompt", multi);
  next = setAiSubtaskPreviewField(next, id, "reviewPrompt", multi);
  assert.equal(next.items[0].task.implementationPrompt, multi, "implementationPrompt の改行が保持される");
  assert.equal(next.items[0].task.reviewPrompt, multi, "reviewPrompt の改行が保持される");
});

test("再検証用: implementationPrompt / reviewPrompt の改行が保持される", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const id = state.items[0].previewId;
  const multi = "手順1\n手順2\n手順3";
  let next = setAiSubtaskPreviewField(state, id, "implementationPrompt", multi);
  next = setAiSubtaskPreviewField(next, id, "reviewPrompt", multi);
  const input = toAiSubtaskRevalidationInput(next);
  assert.equal(input.tasks[0].implementationPrompt, multi, "再検証データでも改行保持");
  assert.equal(input.tasks[0].reviewPrompt, multi, "再検証データでも改行保持");
});

test("編集: prompt 改行編集は元 state / validation.value を破壊しない", () => {
  const value = validatedValue([fullTask()]);
  const snapshot = JSON.parse(JSON.stringify(value));
  const state = createAiSubtaskPreviewState(value);
  const before = JSON.parse(JSON.stringify(state.items[0].task));
  setAiSubtaskPreviewField(state, state.items[0].previewId, "implementationPrompt", "a\nb");
  assert.deepEqual(state.items[0].task, before, "元 item.task は不変");
  assert.deepEqual(value, snapshot, "validation.value は不変");
});

test("編集: 元の task を破壊しない", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const id = state.items[0].previewId;
  const before = JSON.parse(JSON.stringify(state.items[0].task));
  setAiSubtaskPreviewField(state, id, "scope", ["x"]);
  assert.deepEqual(state.items[0].task, before, "元 item.task は不変");
});

// --- 除外 ---

test("除外: included を切り替えられる・除外しても item は消えない", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask(), fullTask({ title: "t2" })]));
  const id = state.items[0].previewId;
  const excluded = setAiSubtaskPreviewIncluded(state, id, false);
  assert.equal(excluded.items.length, 2, "item は残る");
  assert.equal(excluded.items[0].included, false);
  const included = setAiSubtaskPreviewIncluded(excluded, id, true);
  assert.equal(included.items[0].included, true, "再度含められる");
});

test("除外: 件数（included/excluded）が正しい・全件除外を判定できる", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask(), fullTask(), fullTask()]));
  assert.equal(countAiSubtaskIncluded(state), 3);
  assert.equal(countAiSubtaskExcluded(state), 0);
  assert.equal(isAiSubtaskAllExcluded(state), false);
  let s = state;
  for (const it of state.items) s = setAiSubtaskPreviewIncluded(s, it.previewId, false);
  assert.equal(countAiSubtaskIncluded(s), 0);
  assert.equal(countAiSubtaskExcluded(s), 3);
  assert.equal(isAiSubtaskAllExcluded(s), true);
});

// --- 並べ替え ---

test("並べ替え: 上へ移動できる", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask({ title: "a" }), fullTask({ title: "b" })]));
  const second = state.items[1].previewId;
  const next = moveAiSubtaskPreviewItem(state, second, "up");
  assert.deepEqual(next.items.map((it) => it.task.title), ["b", "a"]);
});

test("並べ替え: 下へ移動できる", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask({ title: "a" }), fullTask({ title: "b" })]));
  const first = state.items[0].previewId;
  const next = moveAiSubtaskPreviewItem(state, first, "down");
  assert.deepEqual(next.items.map((it) => it.task.title), ["b", "a"]);
});

test("並べ替え: 先頭の上・末尾の下は no-op（state 非変更）", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask({ title: "a" }), fullTask({ title: "b" })]));
  assert.equal(moveAiSubtaskPreviewItem(state, state.items[0].previewId, "up"), state);
  assert.equal(moveAiSubtaskPreviewItem(state, state.items[1].previewId, "down"), state);
});

test("並べ替え: previewId/included/task の組み合わせが崩れない・元配列を破壊しない", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask({ title: "a" }), fullTask({ title: "b" })]));
  const excluded = setAiSubtaskPreviewIncluded(state, state.items[1].previewId, false);
  const originalItems = excluded.items;
  const b = excluded.items[1]; // title=b, included=false
  const next = moveAiSubtaskPreviewItem(excluded, b.previewId, "up");
  // b が先頭へ来ても previewId/included/task の組み合わせは保持。
  assert.equal(next.items[0].previewId, b.previewId);
  assert.equal(next.items[0].included, false);
  assert.equal(next.items[0].task.title, "b");
  // 元配列は破壊しない。
  assert.equal(excluded.items, originalItems);
  assert.deepEqual(excluded.items.map((it) => it.task.title), ["a", "b"]);
});

// --- 再検証用データ ---

test("再検証用: included=true だけを表示順で抽出・UI専用メタを出力しない", () => {
  const state = createAiSubtaskPreviewState(
    validatedValue([fullTask({ title: "a" }), fullTask({ title: "b" }), fullTask({ title: "c" })]),
  );
  const s2 = setAiSubtaskPreviewIncluded(state, state.items[1].previewId, false); // b を除外
  const input = toAiSubtaskRevalidationInput(s2);
  assert.deepEqual(input.tasks.map((t) => t.title), ["a", "c"], "included のみ・順序維持");
  for (const t of input.tasks) {
    assert.ok(!("previewId" in t) && !("included" in t), "UI専用メタを出力しない");
  }
  assert.deepEqual(Object.keys(input).sort(), ["schemaVersion", "splitSummary", "tasks"]);
});

test("再検証用: 正常データは既存バリデータで成功する", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const input = toAiSubtaskRevalidationInput(state);
  const r = validateAiSubtaskImport(JSON.stringify(input));
  assert.equal(r.ok, true, JSON.stringify(r.errors));
});

test("再検証用: 不正編集（title上限超過）は既存バリデータで失敗する", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const bad = setAiSubtaskPreviewField(state, state.items[0].previewId, "title", "a".repeat(121));
  const r = validateAiSubtaskImport(JSON.stringify(toAiSubtaskRevalidationInput(bad)));
  assert.equal(r.ok, false);
  assert.ok(r.errors.some((e) => e.path === "tasks[0].title"));
});

test("再検証用: 全件除外は既存バリデータで失敗する（tasks 最小件数）", () => {
  const state = createAiSubtaskPreviewState(validatedValue([fullTask()]));
  const excluded = setAiSubtaskPreviewIncluded(state, state.items[0].previewId, false);
  const input = toAiSubtaskRevalidationInput(excluded);
  assert.deepEqual(input.tasks, []);
  const r = validateAiSubtaskImport(JSON.stringify(input));
  assert.equal(r.ok, false, "0件は失敗");
});

// --- linesToArray / arrayToLines ---

test("linesToArray: 空文字は空配列（要素なし）", () => {
  assert.deepEqual(linesToArray(""), []);
});

test("linesToArray: 改行分割・空行も要素として保持（黙って削除しない）", () => {
  assert.deepEqual(linesToArray("a\n\nb"), ["a", "", "b"]);
  assert.deepEqual(linesToArray("a\n"), ["a", ""]);
  assert.deepEqual(linesToArray("a\r\nb"), ["a", "b"]);
});

test("arrayToLines: 1要素=1行で結合（linesToArray の逆）", () => {
  assert.equal(arrayToLines(["a", "", "b"]), "a\n\nb");
  assert.equal(arrayToLines([]), "");
});
