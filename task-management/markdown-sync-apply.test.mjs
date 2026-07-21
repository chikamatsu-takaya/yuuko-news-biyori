// ブラウザ版 Markdown 反映の純粋関数（markdown-sync-apply-core.js）テスト（node:test）。
//
// 範囲（Codex 指摘: mvpScope が更新 mask / 作成 payload に入っていなかった問題の回帰防止）:
// - buildCreateData: 新規作成 payload に taskCode / mvpScope が入り、値を推測・正規化しないこと。
// - buildUpdateFromDiffs: mvpScope の更新 diff が updateMask / writeData に入ること。
// - createdAt / completedAt / archived / source / protected を更新 mask に入れないこと。
// - COMPARE_FIELDS が Node 側 sync-markdown-to-firestore.mjs と mvpScope / taskCode を含めて一致すること。
//
// 実行: node --test task-management/markdown-sync-apply.test.mjs
// 注: 純粋関数は Firebase 非依存の core モジュールに分離済み。firebase-config.js（.gitignore 対象）へ
//     依存しないため、クリーン checkout の CI でも import に失敗せずオフラインで安全にテストできる。

import test from "node:test";
import assert from "node:assert/strict";

// 純粋関数は Firebase 非依存の core モジュールから import する（P1修正）。
// markdown-sync-apply.js を直接 import すると firebase-config.js（.gitignore 対象）へ依存し、
// クリーン checkout の CI で ERR_MODULE_NOT_FOUND になるため、core だけを参照する。
import {
  buildCreateData,
  buildUpdateFromDiffs,
  COMPARE_FIELDS,
} from "./markdown-sync-apply-core.js";
import { COMPARE_FIELDS as NODE_COMPARE_FIELDS } from "./sync-markdown-to-firestore.mjs";

// diff からの更新結果を取り出すヘルパ。
function updateFor(diffs) {
  return buildUpdateFromDiffs({ diffs });
}

// --- COMPARE_FIELDS の Node 一致 ---

test("COMPARE_FIELDS: Node 側と同一集合（mvpScope / taskCode を含む）", () => {
  assert.deepEqual([...COMPARE_FIELDS].sort(), [...NODE_COMPARE_FIELDS].sort());
  assert.ok(COMPARE_FIELDS.includes("mvpScope"));
  assert.ok(COMPARE_FIELDS.includes("taskCode"));
});

// --- 新規作成 payload（buildCreateData） ---

test("作成: mvpScope=Required が作成 payload へ入る", () => {
  const data = buildCreateData({ data: { title: "t", category: "c", mvpScope: "Required" } });
  assert.equal(data.mvpScope, "Required");
});

test("作成: mvpScope=Additional が作成 payload へ入る", () => {
  const data = buildCreateData({ data: { title: "t", category: "c", mvpScope: "Additional" } });
  assert.equal(data.mvpScope, "Additional");
});

test("作成: mvpScope=Undecided が作成 payload へ入る", () => {
  const data = buildCreateData({ data: { title: "t", category: "c", mvpScope: "Undecided" } });
  assert.equal(data.mvpScope, "Undecided");
});

test("作成: mvpScope 未設定は null（Required / Undecided へ推測しない）", () => {
  const data = buildCreateData({ data: { title: "t", category: "c" } });
  assert.equal(data.mvpScope, null);
});

test("作成: mvpScope=null をそのまま null にする（推測しない）", () => {
  const data = buildCreateData({ data: { title: "t", category: "c", mvpScope: null } });
  assert.equal(data.mvpScope, null);
});

test("作成: taskCode も Node 側 data をそのまま保存（未設定は null）", () => {
  assert.equal(buildCreateData({ data: { title: "t", category: "c", taskCode: "TASK-9" } }).taskCode, "TASK-9");
  assert.equal(buildCreateData({ data: { title: "t", category: "c" } }).taskCode, null);
});

test("作成: 既存の category / status / owner などを壊さない", () => {
  const data = buildCreateData({
    data: {
      title: "タイトル",
      category: "カテゴリ",
      subcategory: "サブ",
      priority: "P1",
      status: "Doing",
      owner: "@foo",
      mvpScope: "Required",
    },
  });
  assert.equal(data.title, "タイトル");
  assert.equal(data.category, "カテゴリ");
  assert.equal(data.subcategory, "サブ");
  assert.equal(data.priority, "P1");
  assert.equal(data.status, "Doing");
  assert.equal(data.owner, "@foo");
  assert.equal(data.completed, false); // status=Doing → completed=false
  assert.equal(data.source, "md-import");
  assert.equal(data.updatedBy, "md-import");
});

test("作成: フラット形（data ラップなし）でも mvpScope を拾う", () => {
  const data = buildCreateData({ title: "t", category: "c", mvpScope: "Additional" });
  assert.equal(data.mvpScope, "Additional");
});

// --- 更新（buildUpdateFromDiffs） ---

test("更新: Undecided → Required で mask / writeData に mvpScope が入る", () => {
  const { mask, writeData } = updateFor([{ field: "mvpScope", before: "Undecided", after: "Required" }]);
  assert.ok(mask.includes("mvpScope"));
  assert.equal(writeData.mvpScope, "Required");
  // メタは従来どおり。
  assert.ok(mask.includes("updatedAt") && mask.includes("updatedBy"));
  assert.equal(writeData.updatedBy, "md-import");
  assert.equal(typeof writeData.updatedAt, "string");
});

test("更新: Required → Additional", () => {
  const { mask, writeData } = updateFor([{ field: "mvpScope", before: "Required", after: "Additional" }]);
  assert.ok(mask.includes("mvpScope"));
  assert.equal(writeData.mvpScope, "Additional");
});

test("更新: Additional → Undecided", () => {
  const { mask, writeData } = updateFor([{ field: "mvpScope", before: "Additional", after: "Undecided" }]);
  assert.ok(mask.includes("mvpScope"));
  assert.equal(writeData.mvpScope, "Undecided");
});

test("更新: taskCode の diff も mask / writeData に入る", () => {
  const { mask, writeData } = updateFor([{ field: "taskCode", before: "TASK-1", after: "TASK-2" }]);
  assert.ok(mask.includes("taskCode"));
  assert.equal(writeData.taskCode, "TASK-2");
});

// --- 非対象フィールドは mask へ入れない ---

test("更新: createdAt/completedAt/archived/source/protected は更新 mask へ入らない", () => {
  const { mask } = updateFor([
    { field: "mvpScope", after: "Required" },
    // COMPARE_FIELDS 外の diff は無視される。
    { field: "createdAt", after: "2020-01-01T00:00:00Z" },
    { field: "completedAt", after: "2020-01-01T00:00:00Z" },
    { field: "archived", after: true },
    { field: "source", after: "manual-poc" },
    { field: "protected", after: true },
  ]);
  for (const forbidden of ["createdAt", "completedAt", "archived", "source", "protected"]) {
    assert.ok(!mask.includes(forbidden), `${forbidden} は mask 外`);
  }
  assert.ok(mask.includes("mvpScope"));
});

test("更新: status 変更時は completed も連動（既存挙動を維持）", () => {
  const { mask, writeData } = updateFor([{ field: "status", before: "Doing", after: "Done" }]);
  assert.ok(mask.includes("status") && mask.includes("completed"));
  assert.equal(writeData.completed, true);
});
