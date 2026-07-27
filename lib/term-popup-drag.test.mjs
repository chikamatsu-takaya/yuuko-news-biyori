// lib/term-popup-drag.mjs（TermPopup ドラッグ位置の純粋関数）の単体テスト（node:test）。
// 実行: node --test lib/term-popup-drag.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { clampTermPopupOffset } from "./term-popup-drag.mjs";

// main=1000x800、popup=320x240 を基準にする。
// center = ((1000-320)/2, (800-240)/2) = (340, 280)。margin=8。
const BASE = {
  mainWidth: 1000,
  mainHeight: 800,
  popupWidth: 320,
  popupHeight: 240,
  margin: 8,
};

test("clampTermPopupOffset: オフセット0（中央）はそのまま", () => {
  const { dx, dy } = clampTermPopupOffset({ ...BASE, offsetX: 0, offsetY: 0 });
  assert.equal(dx, 0);
  assert.equal(dy, 0);
});

test("clampTermPopupOffset: 範囲内の移動はそのまま通す", () => {
  const { dx, dy } = clampTermPopupOffset({ ...BASE, offsetX: 100, offsetY: -50 });
  assert.equal(dx, 100);
  assert.equal(dy, -50);
});

test("clampTermPopupOffset: 右下へ出過ぎたら max（center-margin）へ制限", () => {
  const { dx, dy } = clampTermPopupOffset({
    ...BASE,
    offsetX: 100000,
    offsetY: 100000,
  });
  // maxDx = 340-8 = 332, maxDy = 280-8 = 272。
  assert.equal(dx, 332);
  assert.equal(dy, 272);
});

test("clampTermPopupOffset: 左上へ出過ぎたら min（margin-center）へ制限", () => {
  const { dx, dy } = clampTermPopupOffset({
    ...BASE,
    offsetX: -100000,
    offsetY: -100000,
  });
  // minDx = 8-340 = -332, minDy = 8-280 = -272。
  assert.equal(dx, -332);
  assert.equal(dy, -272);
});

test("clampTermPopupOffset: 端に置いてもダイアログは表示領域内（margin確保）", () => {
  // 右下端へ最大移動したときの左上座標 = center + max = 340 + 332 = 672。
  // 右端 = 672 + 320 = 992 <= 1000 - 8 = 992（margin ちょうど）。
  const centerLeft = (BASE.mainWidth - BASE.popupWidth) / 2;
  const { dx } = clampTermPopupOffset({ ...BASE, offsetX: 100000, offsetY: 0 });
  const left = centerLeft + dx;
  assert.ok(left >= BASE.margin, "左端が margin 以上");
  assert.ok(
    left + BASE.popupWidth <= BASE.mainWidth - BASE.margin,
    "右端が (幅 - margin) 以下"
  );
});

test("clampTermPopupOffset: main がダイアログより小さいと上端/左端を margin へ固定", () => {
  // main=200x150 < popup=320x240。center=((200-320)/2,(150-240)/2)=(-60,-45)。
  // min=8-(-60)=68, max=(-60)-8=-68 → min>max → dx=68。左端 = center+dx = -60+68 = 8 = margin。
  const small = {
    mainWidth: 200,
    mainHeight: 150,
    popupWidth: 320,
    popupHeight: 240,
    margin: 8,
  };
  const { dx, dy } = clampTermPopupOffset({ ...small, offsetX: 0, offsetY: 0 });
  assert.equal(dx, 68);
  assert.equal(dy, 53); // -45→min=8-(-45)=53
  const left = (small.mainWidth - small.popupWidth) / 2 + dx;
  const top = (small.mainHeight - small.popupHeight) / 2 + dy;
  assert.equal(left, 8, "左端が margin に固定（見出し・閉じるを残す）");
  assert.equal(top, 8, "上端が margin に固定");
});

test("clampTermPopupOffset: 非有限値は安全に0扱いで壊れない", () => {
  const r1 = clampTermPopupOffset({
    ...BASE,
    offsetX: Number.NaN,
    offsetY: Number.POSITIVE_INFINITY,
  });
  assert.ok(Number.isFinite(r1.dx));
  assert.ok(Number.isFinite(r1.dy));

  const r2 = clampTermPopupOffset({
    offsetX: 10,
    offsetY: 10,
    mainWidth: Number.NaN,
    mainHeight: Number.NaN,
    popupWidth: Number.NaN,
    popupHeight: Number.NaN,
  });
  assert.ok(Number.isFinite(r2.dx));
  assert.ok(Number.isFinite(r2.dy));
});
