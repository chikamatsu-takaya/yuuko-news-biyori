// ai-subtask-import-validator.mjs の単体テスト（node:test）。
//
// 範囲（docs/00_project/ai-subtask-import-spec.md §3.1〜§3.4 / §9 / §11-1）:
// - 前処理（trim / BOM / 1組コードフェンス除去）と解析エラー。
// - ルート/タスクの構造・型・長さ・件数・システム管理項目拒否の検証。
// - 黙った補正・切り詰め・型変換をしないことの確認。
//
// 実行: node --test task-management/ai-subtask-import-validator.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  validateAiSubtaskImport,
  validateImportObject,
  preprocessInput,
  LIMITS,
  ERROR_CODES,
} from "./ai-subtask-import-validator.mjs";

// --- テスト用ヘルパ ---

// 最小の正常タスク（title のみ）。
function minimalTask(overrides = {}) {
  return { title: "設定読み込みを確認する", ...overrides };
}

// 正常なルートオブジェクトを作る。
function validObject(overrides = {}) {
  return { schemaVersion: 1, tasks: [minimalTask()], ...overrides };
}

// 指定文字を n 個並べた文字列（境界値テスト用）。
function repeat(ch, n) {
  return ch.repeat(n);
}

// errors に指定 path/code の要素が含まれるか。
function hasError(result, path, code) {
  return result.errors.some((e) => e.path === path && e.code === code);
}

// =====================================================================
// 正常系
// =====================================================================

test("正常系: 通常のJSON", () => {
  const text = JSON.stringify(validObject({ splitSummary: "読み込みと保存で分割" }));
  const result = validateAiSubtaskImport(text);
  assert.equal(result.ok, true);
  assert.equal(result.errors.length, 0);
  assert.equal(result.value.schemaVersion, 1);
  assert.equal(result.value.tasks.length, 1);
});

test("正常系: 前後空白あり", () => {
  const text = `   \n${JSON.stringify(validObject())}\n\t `;
  assert.equal(validateAiSubtaskImport(text).ok, true);
});

test("正常系: UTF-8 BOMあり", () => {
  const text = `﻿${JSON.stringify(validObject())}`;
  assert.equal(validateAiSubtaskImport(text).ok, true);
});

test("正常系: ```json フェンスあり", () => {
  const text = "```json\n" + JSON.stringify(validObject()) + "\n```";
  assert.equal(validateAiSubtaskImport(text).ok, true);
});

test("正常系: 言語指定なしの ``` フェンスあり", () => {
  const text = "```\n" + JSON.stringify(validObject()) + "\n```";
  assert.equal(validateAiSubtaskImport(text).ok, true);
});

test("回帰: Bufferが無いブラウザ相当環境でも正常JSONを検証できる", () => {
  // 本体モジュールが Node 固有の Buffer に依存しないことを確認する（TextEncoder 使用）。
  const original = globalThis.Buffer;
  try {
    // ブラウザ相当（Buffer 未定義）を模す。
    globalThis.Buffer = undefined;
    const result = validateAiSubtaskImport(JSON.stringify(validObject()));
    assert.equal(result.ok, true);
  } finally {
    globalThis.Buffer = original;
  }
});

test("正常系: フェンス＋前後空白＋BOM の複合", () => {
  const text = "﻿  \n```json\n" + JSON.stringify(validObject()) + "\n```\n  ";
  assert.equal(validateAiSubtaskImport(text).ok, true);
});

test("正常系: splitSummary省略", () => {
  const result = validateAiSubtaskImport(JSON.stringify(validObject()));
  assert.equal(result.ok, true);
});

test("正常系: 任意項目を省略（titleのみ）", () => {
  const result = validateImportObject(validObject());
  assert.equal(result.ok, true);
});

test("正常系: 空配列の任意項目", () => {
  const result = validateImportObject(
    validObject({ tasks: [minimalTask({ doneWhen: [], notes: [] })] }),
  );
  assert.equal(result.ok, true);
});

test("正常系: tasksが1件", () => {
  assert.equal(validateImportObject(validObject({ tasks: [minimalTask()] })).ok, true);
});

test("正常系: tasksが20件（上限ちょうど）", () => {
  const tasks = Array.from({ length: LIMITS.MAX_TASKS }, (_, i) => minimalTask({ title: `t${i}` }));
  assert.equal(validateImportObject(validObject({ tasks })).ok, true);
});

test("正常系: 各文字数・配列件数が上限ちょうど", () => {
  const task = {
    title: repeat("あ", LIMITS.TITLE_MAX),
    purpose: repeat("p", LIMITS.PURPOSE_MAX),
    splitReason: repeat("s", LIMITS.SPLIT_REASON_MAX),
    implementationPrompt: repeat("i", LIMITS.PROMPT_MAX),
    reviewPrompt: repeat("r", LIMITS.PROMPT_MAX),
    doneWhen: Array.from({ length: LIMITS.ARRAY_MAX_ITEMS }, () => repeat("x", LIMITS.ARRAY_ELEMENT_MAX)),
  };
  const result = validateImportObject(validObject({ tasks: [task] }));
  assert.equal(result.ok, true, JSON.stringify(result.errors));
});

test("正常系: 日本語を含むJSON", () => {
  const task = minimalTask({
    title: "設定画面の現在値読み込みを確認する",
    purpose: "保存済み設定が初期表示へ反映されることを確認する",
    doneWhen: ["関心カテゴリが現在設定を反映する"],
  });
  const text = JSON.stringify(validObject({ splitSummary: "元タスクを分割", tasks: [task] }));
  assert.equal(validateAiSubtaskImport(text).ok, true);
});

test("正常系: value は補正されず parse 結果と一致する", () => {
  const obj = validObject({ splitSummary: "そのまま", tasks: [minimalTask({ scope: ["A", "B"] })] });
  const result = validateImportObject(obj);
  assert.equal(result.ok, true);
  assert.deepEqual(result.value, obj);
});

// =====================================================================
// 前処理（単体）
// =====================================================================

test("前処理: BOM＋空白＋フェンスを除去して中身を返す", () => {
  const inner = JSON.stringify(validObject());
  assert.equal(preprocessInput(`﻿  \n\`\`\`json\n${inner}\n\`\`\`  `), inner);
});

test("前処理: フェンスがなければ素通し（trimのみ）", () => {
  assert.equal(preprocessInput("  {\"a\":1}  "), '{"a":1}');
});

test("前処理: 非文字列は空文字を返す", () => {
  assert.equal(preprocessInput(null), "");
  assert.equal(preprocessInput(42), "");
});

// =====================================================================
// 解析エラー
// =====================================================================

test("解析エラー: 空文字", () => {
  const result = validateAiSubtaskImport("");
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "", ERROR_CODES.EMPTY_INPUT));
});

test("解析エラー: 空白のみ", () => {
  assert.ok(hasError(validateAiSubtaskImport("   \n\t"), "", ERROR_CODES.EMPTY_INPUT));
});

test("解析エラー: JSONではない文字列", () => {
  assert.ok(hasError(validateAiSubtaskImport("これはJSONではありません"), "", ERROR_CODES.PARSE_ERROR));
});

test("解析エラー: JSONの前後に説明文がある", () => {
  const text = "以下がタスクです\n" + JSON.stringify(validObject()) + "\n以上です";
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.PARSE_ERROR));
});

test("解析エラー: 壊れたカンマ（trailing comma）", () => {
  const text = '{"schemaVersion":1,"tasks":[{"title":"a"},],}';
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.PARSE_ERROR));
});

test("解析エラー: シングルクォートJSON", () => {
  const text = "{'schemaVersion':1,'tasks':[{'title':'a'}]}";
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.PARSE_ERROR));
});

test("解析エラー: 閉じていないコードフェンス", () => {
  // 未閉じフェンスは除去されず、先頭に ``` が残るため parse 失敗になる。
  const text = "```json\n" + JSON.stringify(validObject());
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.PARSE_ERROR));
});

test("解析エラー: 複数コードフェンス（推測抽出しない）", () => {
  const inner = JSON.stringify(validObject());
  const text = "```json\n" + inner + "\n```\n```json\n" + inner + "\n```";
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.PARSE_ERROR));
});

test("解析エラー: ```yaml フェンスは除去せずPARSE_ERROR", () => {
  const text = "```yaml\n" + JSON.stringify(validObject()) + "\n```";
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.PARSE_ERROR));
});

test("解析エラー: ```javascript フェンスは除去せずPARSE_ERROR", () => {
  const text = "```javascript\n" + JSON.stringify(validObject()) + "\n```";
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.PARSE_ERROR));
});

test("正常系（フェンス限定）: ```json は引き続き正常・言語指定なしも正常", () => {
  const inner = JSON.stringify(validObject());
  assert.equal(validateAiSubtaskImport("```json\n" + inner + "\n```").ok, true);
  assert.equal(validateAiSubtaskImport("```\n" + inner + "\n```").ok, true);
});

test("解析エラー: 200KB超過", () => {
  // notes 要素は300文字以内だが、大量の要素で全体サイズだけを 200KB 超にする。
  // ※サイズ判定はバリデーションより前段（前処理後テキストのバイト数）で行う。
  const big = "x".repeat(LIMITS.MAX_JSON_BYTES + 1000);
  const text = '{"schemaVersion":1,"splitSummary":"' + big + '","tasks":[{"title":"a"}]}';
  assert.ok(Buffer.byteLength(text, "utf8") > LIMITS.MAX_JSON_BYTES);
  assert.ok(hasError(validateAiSubtaskImport(text), "", ERROR_CODES.TOO_LARGE));
});

test("境界値: 200KBちょうどは超過扱いにしない（サイズでは弾かれない）", () => {
  // ちょうど MAX_JSON_BYTES のテキストは TOO_LARGE にしない（超過は > で判定）。
  const filler = '{"schemaVersion":1,"splitSummary":"","tasks":[{"title":"a"}]}';
  const pad = LIMITS.MAX_JSON_BYTES - Buffer.byteLength(filler, "utf8");
  const text = '{"schemaVersion":1,"splitSummary":"' + "x".repeat(pad) + '","tasks":[{"title":"a"}]}';
  assert.equal(Buffer.byteLength(text, "utf8"), LIMITS.MAX_JSON_BYTES);
  const result = validateAiSubtaskImport(text);
  assert.ok(!hasError(result, "", ERROR_CODES.TOO_LARGE));
  assert.equal(result.ok, true);
});

// =====================================================================
// ルートエラー
// =====================================================================

test("ルートエラー: null", () => {
  assert.ok(hasError(validateImportObject(null), "", ERROR_CODES.NOT_OBJECT));
});

test("ルートエラー: 配列", () => {
  assert.ok(hasError(validateImportObject([]), "", ERROR_CODES.NOT_OBJECT));
});

test("ルートエラー: schemaVersionなし", () => {
  const obj = validObject();
  delete obj.schemaVersion;
  assert.ok(hasError(validateImportObject(obj), "schemaVersion", ERROR_CODES.REQUIRED));
});

test('ルートエラー: schemaVersionが文字列の"1"', () => {
  assert.ok(
    hasError(validateImportObject(validObject({ schemaVersion: "1" })), "schemaVersion", ERROR_CODES.INVALID_TYPE),
  );
});

test("ルートエラー: schemaVersionが2", () => {
  assert.ok(
    hasError(validateImportObject(validObject({ schemaVersion: 2 })), "schemaVersion", ERROR_CODES.INVALID_VALUE),
  );
});

test("ルートエラー: tasksなし", () => {
  const obj = validObject();
  delete obj.tasks;
  assert.ok(hasError(validateImportObject(obj), "tasks", ERROR_CODES.REQUIRED));
});

test("ルートエラー: tasksが配列ではない", () => {
  assert.ok(hasError(validateImportObject(validObject({ tasks: {} })), "tasks", ERROR_CODES.INVALID_TYPE));
});

test("ルートエラー: tasksが0件", () => {
  assert.ok(hasError(validateImportObject(validObject({ tasks: [] })), "tasks", ERROR_CODES.TASKS_TOO_FEW));
});

test("ルートエラー: tasksが21件", () => {
  const tasks = Array.from({ length: LIMITS.MAX_TASKS + 1 }, (_, i) => minimalTask({ title: `t${i}` }));
  assert.ok(hasError(validateImportObject(validObject({ tasks })), "tasks", ERROR_CODES.TASKS_TOO_MANY));
});

test("ルートエラー: splitSummaryが文字列ではない", () => {
  assert.ok(
    hasError(validateImportObject(validObject({ splitSummary: 123 })), "splitSummary", ERROR_CODES.INVALID_TYPE),
  );
});

test("ルートエラー: 未知のルート項目", () => {
  assert.ok(
    hasError(validateImportObject(validObject({ extraRoot: true })), "extraRoot", ERROR_CODES.UNKNOWN_FIELD),
  );
});

test("ルートエラー: システム項目がルートに存在", () => {
  assert.ok(
    hasError(validateImportObject(validObject({ importBatchId: "x" })), "importBatchId", ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED),
  );
});

// =====================================================================
// タスクエラー
// =====================================================================

test("タスクエラー: タスク要素がnull", () => {
  assert.ok(hasError(validateImportObject(validObject({ tasks: [null] })), "tasks[0]", ERROR_CODES.NOT_OBJECT));
});

test("タスクエラー: タスク要素が配列", () => {
  assert.ok(hasError(validateImportObject(validObject({ tasks: [[]] })), "tasks[0]", ERROR_CODES.NOT_OBJECT));
});

test("タスクエラー: titleなし", () => {
  assert.ok(hasError(validateImportObject(validObject({ tasks: [{}] })), "tasks[0].title", ERROR_CODES.REQUIRED));
});

test("タスクエラー: titleが文字列ではない", () => {
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [{ title: 123 }] })), "tasks[0].title", ERROR_CODES.INVALID_TYPE),
  );
});

test("タスクエラー: titleが空文字", () => {
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [{ title: "" }] })), "tasks[0].title", ERROR_CODES.EMPTY),
  );
});

test("タスクエラー: titleが空白のみ", () => {
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [{ title: "   \t" }] })), "tasks[0].title", ERROR_CODES.EMPTY),
  );
});

test("タスクエラー: titleが121文字", () => {
  const task = minimalTask({ title: repeat("a", LIMITS.TITLE_MAX + 1) });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].title", ERROR_CODES.STRING_TOO_LONG),
  );
});

test("タスクエラー: purposeが1,001文字", () => {
  const task = minimalTask({ purpose: repeat("p", LIMITS.PURPOSE_MAX + 1) });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].purpose", ERROR_CODES.STRING_TOO_LONG),
  );
});

test("タスクエラー: splitReasonが1,001文字", () => {
  const task = minimalTask({ splitReason: repeat("s", LIMITS.SPLIT_REASON_MAX + 1) });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].splitReason", ERROR_CODES.STRING_TOO_LONG),
  );
});

test("タスクエラー: implementationPromptが8,001文字", () => {
  const task = minimalTask({ implementationPrompt: repeat("i", LIMITS.PROMPT_MAX + 1) });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].implementationPrompt", ERROR_CODES.STRING_TOO_LONG),
  );
});

test("タスクエラー: reviewPromptが8,001文字", () => {
  const task = minimalTask({ reviewPrompt: repeat("r", LIMITS.PROMPT_MAX + 1) });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].reviewPrompt", ERROR_CODES.STRING_TOO_LONG),
  );
});

test("タスクエラー: 配列項目が配列ではない", () => {
  const task = minimalTask({ doneWhen: "not-array" });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].doneWhen", ERROR_CODES.INVALID_TYPE),
  );
});

test("タスクエラー: 配列が21要素", () => {
  const task = minimalTask({ scope: Array.from({ length: LIMITS.ARRAY_MAX_ITEMS + 1 }, () => "x") });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].scope", ERROR_CODES.ARRAY_TOO_LONG),
  );
});

test("タスクエラー: 配列要素が文字列ではない", () => {
  const task = minimalTask({ notes: ["ok", 123] });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].notes[1]", ERROR_CODES.INVALID_TYPE),
  );
});

test("タスクエラー: 配列要素が301文字", () => {
  const task = minimalTask({ reviewPoints: [repeat("x", LIMITS.ARRAY_ELEMENT_MAX + 1)] });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].reviewPoints[0]", ERROR_CODES.STRING_TOO_LONG),
  );
});

test("タスクエラー: 未知フィールド", () => {
  const task = minimalTask({ foo: "bar" });
  assert.ok(
    hasError(validateImportObject(validObject({ tasks: [task] })), "tasks[0].foo", ERROR_CODES.UNKNOWN_FIELD),
  );
});

test("タスクエラー: statusなどのシステム管理項目", () => {
  const task = minimalTask({ status: "Doing", owner: "近松" });
  const result = validateImportObject(validObject({ tasks: [task] }));
  assert.ok(hasError(result, "tasks[0].status", ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED));
  assert.ok(hasError(result, "tasks[0].owner", ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED));
});

test("タスクエラー: autoStatusUpdateDisabled / taskRole / splitChildCount", () => {
  const task = minimalTask({ autoStatusUpdateDisabled: true, taskRole: "split-parent", splitChildCount: 3 });
  const result = validateImportObject(validObject({ tasks: [task] }));
  assert.ok(hasError(result, "tasks[0].autoStatusUpdateDisabled", ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED));
  assert.ok(hasError(result, "tasks[0].taskRole", ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED));
  assert.ok(hasError(result, "tasks[0].splitChildCount", ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED));
});

test("タスクエラー: 複数タスクの別々の項目にエラーがある場合のpath", () => {
  const tasks = [
    minimalTask({ title: "" }), // tasks[0].title EMPTY
    minimalTask({ purpose: repeat("p", LIMITS.PURPOSE_MAX + 1) }), // tasks[1].purpose STRING_TOO_LONG
  ];
  const result = validateImportObject(validObject({ tasks }));
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "tasks[0].title", ERROR_CODES.EMPTY));
  assert.ok(hasError(result, "tasks[1].purpose", ERROR_CODES.STRING_TOO_LONG));
});

// =====================================================================
// 補正・切り詰め・型変換をしないことの確認
// =====================================================================

test("補正なし: 非文字列要素を黙って除外しない（エラーにする・配列は変えない）", () => {
  const notes = ["ok", 5, "ok2"];
  const task = minimalTask({ notes });
  const result = validateImportObject(validObject({ tasks: [task] }));
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "tasks[0].notes[1]", ERROR_CODES.INVALID_TYPE));
  // 入力配列は改変されていない。
  assert.deepEqual(notes, ["ok", 5, "ok2"]);
});

test("補正なし: 上限超過を黙って切り詰めない（エラーにする）", () => {
  const task = minimalTask({ doneWhen: Array.from({ length: LIMITS.ARRAY_MAX_ITEMS + 5 }, () => "x") });
  const result = validateImportObject(validObject({ tasks: [task] }));
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "tasks[0].doneWhen", ERROR_CODES.ARRAY_TOO_LONG));
  // 元配列の長さは維持される。
  assert.equal(task.doneWhen.length, LIMITS.ARRAY_MAX_ITEMS + 5);
});

test("補正なし: title は trim されず元の値のまま value に残る", () => {
  // 前後に空白があっても（空でなければ）trim せずそのまま通す。
  const task = minimalTask({ title: "  実装する  " });
  const result = validateImportObject(validObject({ tasks: [task] }));
  assert.equal(result.ok, true);
  assert.equal(result.value.tasks[0].title, "  実装する  ");
});

// =====================================================================
// エラー順の決定性
// =====================================================================

test("エラー順: ルート → tasks配列順 → タスク内項目順（固定順→辞書順）", () => {
  const obj = {
    schemaVersion: 2, // ルート: INVALID_VALUE
    tasks: [
      { title: "", zzz: 1, status: "Doing" }, // title EMPTY → 辞書順で status, zzz
    ],
    unknownRoot: true, // ルート許可外
  };
  const paths = validateImportObject(obj).errors.map((e) => e.path);
  assert.deepEqual(paths, [
    "schemaVersion",
    "unknownRoot",
    "tasks[0].title",
    "tasks[0].status",
    "tasks[0].zzz",
  ]);
});
