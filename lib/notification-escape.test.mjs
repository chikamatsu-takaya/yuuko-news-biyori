// lib/notification-escape.mjs（アプリ内通知の Esc 判定）の単体テスト（node:test）。
// 実行: node --test lib/notification-escape.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { shouldNotificationHandleEscape } from "./notification-escape.mjs";

test("shouldNotificationHandleEscape: どこにもフォーカスが無い Esc は閉じる（従来どおり）", () => {
  assert.equal(shouldNotificationHandleEscape({ key: "Escape" }), true);
});

test("shouldNotificationHandleEscape: Esc 以外のキーは扱わない", () => {
  assert.equal(shouldNotificationHandleEscape({ key: "Enter" }), false);
  assert.equal(
    shouldNotificationHandleEscape({ key: "Enter", focusInsideNotification: true }),
    false
  );
});

test("shouldNotificationHandleEscape: 通知の中にフォーカスがあれば閉じる", () => {
  assert.equal(
    shouldNotificationHandleEscape({ key: "Escape", focusInsideNotification: true }),
    true
  );
  // 別のダイアログが開いていても、通知を操作中なら通知を閉じる。
  assert.equal(
    shouldNotificationHandleEscape({
      key: "Escape",
      focusInsideNotification: true,
      dialogOpen: true,
    }),
    true
  );
});

test("shouldNotificationHandleEscape: 入力欄での Esc は閉じない", () => {
  assert.equal(
    shouldNotificationHandleEscape({ key: "Escape", focusInEditable: true }),
    false
  );
});

test("shouldNotificationHandleEscape: ダイアログ内・ダイアログ表示中の Esc は閉じない", () => {
  assert.equal(
    shouldNotificationHandleEscape({ key: "Escape", focusInDialog: true }),
    false
  );
  assert.equal(
    shouldNotificationHandleEscape({ key: "Escape", dialogOpen: true }),
    false
  );
});

test("shouldNotificationHandleEscape: 他の部品が処理済み・IME 変換中の Esc は閉じない", () => {
  assert.equal(
    shouldNotificationHandleEscape({
      key: "Escape",
      defaultPrevented: true,
      focusInsideNotification: true,
    }),
    false
  );
  assert.equal(
    shouldNotificationHandleEscape({ key: "Escape", isComposing: true }),
    false
  );
});
