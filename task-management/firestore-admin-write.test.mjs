// firestore-admin-write.mjs の純粋関数の単体テスト（node:test）。
//
// 範囲:
// - mapTaskFieldsToApplyData: Firestore REST の fields → apply ガード用 data 変換。
//   特に autoStatusUpdateDisabled が「boolean true のときだけ true」で、false / 未設定 /
//   文字列 "true" を true 扱いしないことを確認する（post-merge 二段目ガードの live 対応の正本）。
// - 実 Firestore へは接続しない（純粋関数のみ）。
//
// 実行: node --test task-management/firestore-admin-write.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  mapTaskFieldsToApplyData,
  ALLOWED_WRITE_FIELDS,
  buildDoneUpdatePayload,
  buildReviewUpdatePayload,
  buildIssuePrWritebackPayload,
} from "./firestore-admin-write.mjs";

// Firestore REST の型付き値ヘルパ。
const boolField = (value) => ({ booleanValue: value });
const strField = (value) => ({ stringValue: value });

test("mapTaskFieldsToApplyData: autoStatusUpdateDisabled=boolean true → true", () => {
  const data = mapTaskFieldsToApplyData({ autoStatusUpdateDisabled: boolField(true) });
  assert.equal(data.autoStatusUpdateDisabled, true);
});

test("mapTaskFieldsToApplyData: autoStatusUpdateDisabled=boolean false → false", () => {
  const data = mapTaskFieldsToApplyData({ autoStatusUpdateDisabled: boolField(false) });
  assert.equal(data.autoStatusUpdateDisabled, false);
});

test("mapTaskFieldsToApplyData: autoStatusUpdateDisabled 未設定 → false", () => {
  const data = mapTaskFieldsToApplyData({ status: strField("Doing") });
  assert.equal(data.autoStatusUpdateDisabled, false);
});

test('mapTaskFieldsToApplyData: autoStatusUpdateDisabled=文字列 "true" は true 扱いにしない → false', () => {
  // Firestore で文字列として保存されると stringValue になり booleanValue は undefined。
  const data = mapTaskFieldsToApplyData({ autoStatusUpdateDisabled: strField("true") });
  assert.equal(data.autoStatusUpdateDisabled, false, '文字列 "true" は boolean true として扱わない');
});

test("mapTaskFieldsToApplyData: 空 fields でも既定形（autoStatusUpdateDisabled=false・紐づけキー=null）", () => {
  assert.deepEqual(mapTaskFieldsToApplyData({}), {
    status: null,
    completed: false,
    archived: false,
    autoStatusUpdateDisabled: false,
    branchName: null,
    taskCode: null,
    issuePr: null,
  });
});

test("mapTaskFieldsToApplyData: 引数なし（undefined）でも例外なく既定形を返す", () => {
  assert.equal(mapTaskFieldsToApplyData().autoStatusUpdateDisabled, false);
});

test("mapTaskFieldsToApplyData: 既存フィールドの整形（status/completed/archived/紐づけキー）", () => {
  const data = mapTaskFieldsToApplyData({
    status: strField("Doing"),
    completed: boolField(false),
    archived: boolField(false),
    autoStatusUpdateDisabled: boolField(true),
    branchName: strField("feature/x"),
    taskCode: strField("TASK-1"),
    issuePr: strField("#123"),
  });
  assert.deepEqual(data, {
    status: "Doing",
    completed: false,
    archived: false,
    autoStatusUpdateDisabled: true,
    branchName: "feature/x",
    taskCode: "TASK-1",
    issuePr: "#123",
  });
});

// --- MVP区分（mvpScope）は post-merge の書き込み対象外＝更新後も消えない ---

test("post-merge: mvpScope は書き込み許可フィールドに含まれない（PATCHで触れない）", () => {
  assert.ok(!ALLOWED_WRITE_FIELDS.has("mvpScope"));
});

test("post-merge: Done/Review/issuePr の updateMask に mvpScope を含めない", () => {
  // updateMask に無いフィールドは Firestore PATCH で変更されないため、既存 mvpScope が保持される。
  assert.ok(!buildDoneUpdatePayload().updateMaskFields.includes("mvpScope"));
  assert.ok(!buildReviewUpdatePayload().updateMaskFields.includes("mvpScope"));
  assert.ok(!buildIssuePrWritebackPayload(123).updateMaskFields.includes("mvpScope"));
});

test("post-merge: 各ペイロードの data にも mvpScope を含めない", () => {
  assert.ok(!("mvpScope" in buildDoneUpdatePayload().data));
  assert.ok(!("mvpScope" in buildReviewUpdatePayload().data));
  assert.ok(!("mvpScope" in buildIssuePrWritebackPayload(123).data));
});
