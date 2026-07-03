// post-merge-firestore-status.mjs の report-only 判定ロジック回帰テスト（最小構成）。
//
// 範囲:
// - Firestore 実通信・--apply・issuePr 実書き戻しはテストしない（判定結果のみ）。
// - evaluate() + buildReport() を組み合わせ、report.decision の result / reasonIds /
//   reasonLabels（Summary/artifact 用の日本語ラベル）を検証する。
//
// 実行: node --test task-management/post-merge-firestore-status.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { evaluate, buildReport } from "./post-merge-firestore-status.mjs";

// Firestore タスク（Doing・issuePr 空）。実通信はせず、この配列だけを使う。
const TASKS = [
  {
    id: "t1",
    data: {
      taskCode: "TASK-1",
      branchName: "feature/x",
      issuePr: "",
      status: "Doing",
      completed: false,
      archived: false,
    },
  },
];

// buildReport が参照するフィールドを満たす PR コンテキストを作る。
function makePr(overrides = {}) {
  return {
    number: 123,
    headRef: "feature/x",
    baseRef: "develop",
    merged: true,
    author: "someuser",
    body: "- branchName: feature/x",
    files: ["docs/a.md"],
    fileCountExpected: 1,
    fileCountFetched: 1,
    filesTruncated: false,
    ...overrides,
  };
}

// evaluate → buildReport の一連で report.decision を得る（report-only 経路）。
function decisionFor(pr) {
  return buildReport(pr, evaluate(pr, TASKS)).decision;
}

// reasonLabels に指定 ID の非空ラベルが含まれることを確認する。
function assertLabel(decision, id) {
  const entry = decision.reasonLabels.find((x) => x.id === id);
  assert.ok(entry, `reasonLabels に ${id} が含まれること`);
  assert.equal(typeof entry.label, "string");
  assert.ok(entry.label.length > 0, `${id} のラベルが非空であること`);
}

test("docs/Markdown のみ変更 → done_candidate / D3 / ラベルあり", () => {
  const d = decisionFor(makePr({ files: ["docs/01_setup/memo.md"] }));
  assert.equal(d.result, "done_candidate");
  assert.ok(d.reasonIds.includes("D3"), "reasonIds に D3 を含む");
  assertLabel(d, "D3");
});

test("UI変更を含む → review_candidate / R1 / ラベルあり", () => {
  const d = decisionFor(makePr({ files: ["components/screens/MainScreen.tsx"] }));
  assert.equal(d.result, "review_candidate");
  assert.ok(d.reasonIds.includes("R1"), "reasonIds に UI変更(R1) を含む");
  assert.ok(d.reasonLabels.length > 0, "reasonLabels が出る");
  assertLabel(d, "R1");
});

test("対象タスクが見つからない → no_change / G1 / ラベルあり", () => {
  const d = decisionFor(makePr({ headRef: "feature/unknown", body: "no keys", files: ["docs/a.md"] }));
  assert.equal(d.result, "no_change");
  assert.ok(d.reasonIds.includes("G1"), "reasonIds に対象タスクなし(G1) を含む");
  assertLabel(d, "G1");
});
