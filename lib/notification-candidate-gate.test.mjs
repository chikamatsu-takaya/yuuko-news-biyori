// lib/notification-candidate-gate.mjs（通知候補生成の可否判定）の単体テスト（node:test）。
// 実行: node --test lib/notification-candidate-gate.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { canGenerateNotificationCandidates } from "./notification-candidate-gate.mjs";

test("canGenerateNotificationCandidates: 表示中かつ閲覧画面外なら生成する", () => {
  assert.equal(
    canGenerateNotificationCandidates({
      isWindowVisible: true,
      isReadingArticle: false,
    }),
    true
  );
});

test("canGenerateNotificationCandidates: ニュース閲覧画面の表示中は生成しない", () => {
  assert.equal(
    canGenerateNotificationCandidates({
      isWindowVisible: true,
      isReadingArticle: true,
    }),
    false
  );
});

test("canGenerateNotificationCandidates: ウィンドウ非表示中は生成しない", () => {
  assert.equal(
    canGenerateNotificationCandidates({
      isWindowVisible: false,
      isReadingArticle: false,
    }),
    false
  );
  assert.equal(
    canGenerateNotificationCandidates({
      isWindowVisible: false,
      isReadingArticle: true,
    }),
    false
  );
});
