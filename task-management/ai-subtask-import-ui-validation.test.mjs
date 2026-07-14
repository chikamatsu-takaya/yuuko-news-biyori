// UI が使う既存バリデータ公開API（validateAiSubtaskImport）の連携代表テスト（node:test）。
//
// 目的:
// - ステップ2の「JSONを検証」は task-management/ai-subtask-import-validator.mjs の
//   validateAiSubtaskImport(rawText) をそのまま呼ぶ。UI 連携で通る代表ケースだけを確認する。
// - 網羅ケースは ai-subtask-import-validator.test.mjs 側にあるため、ここでは重複させない。
//
// 実行: node --test task-management/ai-subtask-import-ui-validation.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { validateAiSubtaskImport, LIMITS, ERROR_CODES } from "./ai-subtask-import-validator.mjs";

function validObject(overrides = {}) {
  return { schemaVersion: 1, splitSummary: "分割方針", tasks: [{ title: "子タスク" }], ...overrides };
}
const json = (obj) => JSON.stringify(obj);
const hasCode = (result, code) => result.errors.some((e) => e.code === code);

test("UI連携: 生JSONで成功", () => {
  const r = validateAiSubtaskImport(json(validObject()));
  assert.equal(r.ok, true);
  assert.equal(r.value.tasks.length, 1);
});

test("UI連携: ```json コードフェンスで成功", () => {
  assert.equal(validateAiSubtaskImport("```json\n" + json(validObject()) + "\n```").ok, true);
});

test("UI連携: 言語指定なし ``` コードフェンスで成功", () => {
  assert.equal(validateAiSubtaskImport("```\n" + json(validObject()) + "\n```").ok, true);
});

test("UI連携: ```yaml / ```javascript コードフェンスは失敗（PARSE_ERROR）", () => {
  assert.ok(hasCode(validateAiSubtaskImport("```yaml\n" + json(validObject()) + "\n```"), ERROR_CODES.PARSE_ERROR));
  assert.ok(hasCode(validateAiSubtaskImport("```javascript\n" + json(validObject()) + "\n```"), ERROR_CODES.PARSE_ERROR));
});

test("UI連携: schemaVersion 不正で失敗", () => {
  assert.ok(hasCode(validateAiSubtaskImport(json(validObject({ schemaVersion: 2 }))), ERROR_CODES.INVALID_VALUE));
});

test("UI連携: tasks 0件で失敗", () => {
  assert.ok(hasCode(validateAiSubtaskImport(json(validObject({ tasks: [] }))), ERROR_CODES.TASKS_TOO_FEW));
});

test("UI連携: tasks 21件で失敗", () => {
  const tasks = Array.from({ length: LIMITS.MAX_TASKS + 1 }, (_, i) => ({ title: `t${i}` }));
  assert.ok(hasCode(validateAiSubtaskImport(json(validObject({ tasks }))), ERROR_CODES.TASKS_TOO_MANY));
});

test("UI連携: システムフィールド混入で失敗", () => {
  const r = validateAiSubtaskImport(json(validObject({ tasks: [{ title: "x", status: "Doing" }] })));
  assert.ok(hasCode(r, ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED));
});

test("UI連携: title 上限超過で失敗", () => {
  const r = validateAiSubtaskImport(json(validObject({ tasks: [{ title: "a".repeat(LIMITS.TITLE_MAX + 1) }] })));
  assert.ok(hasCode(r, ERROR_CODES.STRING_TOO_LONG));
});

test("UI連携: JSON全体の上限超過で失敗（TOO_LARGE）", () => {
  const big = "x".repeat(LIMITS.MAX_JSON_BYTES + 1000);
  const text = '{"schemaVersion":1,"splitSummary":"' + big + '","tasks":[{"title":"a"}]}';
  assert.ok(hasCode(validateAiSubtaskImport(text), ERROR_CODES.TOO_LARGE));
});

test("UI連携: 許可11項目すべてを持つ正常JSONは成功し、value.tasks に11項目が欠落なく残る（プレビュー表示元）", () => {
  // 読み取り専用プレビューは validation.value.tasks の各フィールドを表示する。
  // scope / outOfScope / notes / verificationCommands 等が黙って落ちないことをデータレベルで確認する。
  const child = {
    title: "子タスク",
    purpose: "目的",
    splitReason: "分割理由",
    scope: ["対象1", "対象2"],
    outOfScope: ["非対象1"],
    doneWhen: ["完了条件1"],
    notes: ["メモ1"],
    implementationPrompt: "実装指示",
    reviewPoints: ["観点1"],
    reviewPrompt: "レビュー依頼",
    verificationCommands: ["pnpm lint", "pnpm build"],
  };
  const r = validateAiSubtaskImport(json(validObject({ tasks: [child] })));
  assert.equal(r.ok, true);
  const t = r.value.tasks[0];
  for (const key of Object.keys(child)) {
    assert.ok(key in t, `value に ${key} が残る`);
  }
  // 配列項目の内容が保持される（切り詰め・除外されない）。
  assert.deepEqual(t.scope, ["対象1", "対象2"]);
  assert.deepEqual(t.outOfScope, ["非対象1"]);
  assert.deepEqual(t.notes, ["メモ1"]);
  assert.deepEqual(t.verificationCommands, ["pnpm lint", "pnpm build"]);
});

test("UI連携: 失敗時に入力を自動修正しない（value を返さず errors を返す・入力文字列は不変）", () => {
  const input = json(validObject({ schemaVersion: 2 }));
  const r = validateAiSubtaskImport(input);
  assert.equal(r.ok, false);
  assert.equal(r.value, undefined, "失敗時は value を返さない（勝手に修正した値を返さない）");
  assert.ok(r.errors.length > 0);
  // 入力文字列は呼び出しで変化しない。
  assert.equal(input, json(validObject({ schemaVersion: 2 })));
});
